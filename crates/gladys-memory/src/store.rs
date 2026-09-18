use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{MemoryError, Result};
use crate::types::{Conversation, ConversationKind, Layer, Memory};

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;

CREATE TABLE IF NOT EXISTS memories (
  id TEXT PRIMARY KEY,
  layer TEXT NOT NULL,
  channel TEXT,
  conv_kind TEXT,
  conv_peer TEXT,
  person TEXT,
  text TEXT NOT NULL,
  ts INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS memories_layer_ts ON memories(layer, ts);
CREATE INDEX IF NOT EXISTS memories_conv ON memories(layer, channel, conv_kind, conv_peer, ts);
CREATE INDEX IF NOT EXISTS memories_person ON memories(layer, channel, person, ts);

CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
  id UNINDEXED,
  text
);
"#;

#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
}

impl Store {
    pub fn open(sqlite: impl AsRef<Path>) -> Result<Self> {
        let sqlite = sqlite.as_ref();
        if let Some(parent) = sqlite.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(sqlite)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| MemoryError::Invalid("store mutex poisoned".into()))
    }

    pub fn insert(&self, mem: &Memory) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO memories (id, layer, channel, conv_kind, conv_peer, person, text, ts)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                mem.id,
                mem.layer.as_str(),
                mem.channel,
                mem.conversation.as_ref().map(|c| c.kind.as_str()),
                mem.conversation.as_ref().map(|c| c.peer.as_str()),
                mem.person,
                mem.text,
                mem.ts,
            ],
        )?;
        conn.execute(
            "INSERT INTO memories_fts (id, text) VALUES (?1, ?2)",
            params![mem.id, mem.text],
        )?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<Option<Memory>> {
        let conn = self.lock()?;
        let row = conn
            .query_row(
                "SELECT id, layer, channel, conv_kind, conv_peer, person, text, ts
                 FROM memories WHERE id = ?1",
                params![id],
                row_to_memory,
            )
            .optional()?;
        Ok(row)
    }

    pub fn forget(&self, id: &str) -> Result<bool> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM memories_fts WHERE id = ?1", params![id])?;
        let n = conn.execute("DELETE FROM memories WHERE id = ?1", params![id])?;
        Ok(n > 0)
    }

    pub fn update_text(&self, id: &str, text: &str) -> Result<Memory> {
        let conn = self.lock()?;
        let n = conn.execute(
            "UPDATE memories SET text = ?1 WHERE id = ?2",
            params![text, id],
        )?;
        if n == 0 {
            return Err(MemoryError::NotFound);
        }
        conn.execute("DELETE FROM memories_fts WHERE id = ?1", params![id])?;
        conn.execute(
            "INSERT INTO memories_fts (id, text) VALUES (?1, ?2)",
            params![id, text],
        )?;
        drop(conn);
        self.get(id)?.ok_or(MemoryError::NotFound)
    }

    pub fn list(
        &self,
        layer: Option<Layer>,
        channel: Option<&str>,
        conv: Option<&Conversation>,
        person: Option<&str>,
        before_ts: Option<i64>,
        limit: u32,
    ) -> Result<Vec<Memory>> {
        let conn = self.lock()?;
        let limit = limit.clamp(1, 200) as i64;
        let layer_s = layer.map(|l| l.as_str());
        let kind = conv.map(|c| c.kind.as_str());
        let peer = conv.map(|c| c.peer.as_str());
        let mut stmt = conn.prepare(
            "SELECT id, layer, channel, conv_kind, conv_peer, person, text, ts
             FROM memories
             WHERE (?1 IS NULL OR layer = ?1)
               AND (?2 IS NULL OR channel = ?2)
               AND (?3 IS NULL OR conv_kind = ?3)
               AND (?4 IS NULL OR conv_peer = ?4)
               AND (?5 IS NULL OR person = ?5)
               AND (?6 IS NULL OR ts < ?6)
             ORDER BY ts DESC LIMIT ?7",
        )?;
        let mapped = stmt.query_map(
            params![layer_s, channel, kind, peer, person, before_ts, limit],
            row_to_memory,
        )?;
        Ok(mapped.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn scopes(&self) -> Result<crate::types::Scopes> {
        let conn = self.lock()?;
        let mut channels = Vec::new();
        {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT channel FROM memories
                 WHERE channel IS NOT NULL AND channel != ''
                 ORDER BY channel",
            )?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            for r in rows {
                channels.push(r?);
            }
        }
        let mut conversations = Vec::new();
        {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT channel, conv_kind, conv_peer FROM memories
                 WHERE layer = 'conversation'
                   AND channel IS NOT NULL AND conv_kind IS NOT NULL AND conv_peer IS NOT NULL
                 ORDER BY channel, conv_kind, conv_peer",
            )?;
            let rows = stmt.query_map([], |row| {
                let channel: String = row.get(0)?;
                let kind_s: String = row.get(1)?;
                let peer: String = row.get(2)?;
                let kind = ConversationKind::parse(&kind_s).unwrap_or(ConversationKind::Group);
                Ok(crate::types::ConversationRef {
                    channel,
                    kind,
                    peer,
                })
            })?;
            for r in rows {
                conversations.push(r?);
            }
        }
        let mut persons = Vec::new();
        {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT channel, person FROM memories
                 WHERE layer = 'person'
                   AND channel IS NOT NULL AND person IS NOT NULL AND person != ''
                 ORDER BY channel, person",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(crate::types::PersonRef {
                    channel: row.get(0)?,
                    person: row.get(1)?,
                })
            })?;
            for r in rows {
                persons.push(r?);
            }
        }
        Ok(crate::types::Scopes {
            channels,
            conversations,
            persons,
        })
    }

    pub fn list_layer(
        &self,
        layer: Layer,
        channel: Option<&str>,
        conv: Option<&Conversation>,
        person: Option<&str>,
        limit: u32,
    ) -> Result<Vec<Memory>> {
        let conn = self.lock()?;
        let limit = limit.clamp(1, 200) as i64;
        match layer {
            Layer::Global => {
                let mut stmt = conn.prepare(
                    "SELECT id, layer, channel, conv_kind, conv_peer, person, text, ts
                     FROM memories WHERE layer = 'global'
                     ORDER BY ts DESC LIMIT ?1",
                )?;
                let mapped = stmt.query_map(params![limit], row_to_memory)?;
                Ok(mapped.collect::<rusqlite::Result<Vec<_>>>()?)
            }
            Layer::Conversation => {
                let conv = conv.ok_or_else(|| MemoryError::Invalid("conversation required".into()))?;
                let channel =
                    channel.ok_or_else(|| MemoryError::Invalid("channel required".into()))?;
                let mut stmt = conn.prepare(
                    "SELECT id, layer, channel, conv_kind, conv_peer, person, text, ts
                     FROM memories
                     WHERE layer = 'conversation' AND channel = ?1 AND conv_kind = ?2 AND conv_peer = ?3
                     ORDER BY ts DESC LIMIT ?4",
                )?;
                let mapped = stmt.query_map(
                    params![channel, conv.kind.as_str(), conv.peer, limit],
                    row_to_memory,
                )?;
                Ok(mapped.collect::<rusqlite::Result<Vec<_>>>()?)
            }
            Layer::Person => {
                let channel =
                    channel.ok_or_else(|| MemoryError::Invalid("channel required".into()))?;
                let person = person.ok_or_else(|| MemoryError::Invalid("person required".into()))?;
                let mut stmt = conn.prepare(
                    "SELECT id, layer, channel, conv_kind, conv_peer, person, text, ts
                     FROM memories
                     WHERE layer = 'person' AND channel = ?1 AND person = ?2
                     ORDER BY ts DESC LIMIT ?3",
                )?;
                let mapped = stmt.query_map(params![channel, person, limit], row_to_memory)?;
                Ok(mapped.collect::<rusqlite::Result<Vec<_>>>()?)
            }
        }
    }

    pub fn search(
        &self,
        query: &str,
        layer: Option<Layer>,
        channel: Option<&str>,
        conv: Option<&Conversation>,
        person: Option<&str>,
        limit: u32,
    ) -> Result<Vec<Memory>> {
        let conn = self.lock()?;
        let limit = limit.clamp(1, 200) as i64;
        let fts_query = fts_safe(query);
        let mut rows = match (layer, channel, conv, person) {
            (None, None, None, None) => fts_map(
                &conn,
                "SELECT m.id, m.layer, m.channel, m.conv_kind, m.conv_peer, m.person, m.text, m.ts
                 FROM memories_fts f JOIN memories m ON m.id = f.id
                 WHERE memories_fts MATCH ?1 ORDER BY m.ts DESC LIMIT ?2",
                params![fts_query, limit],
            ),
            (Some(Layer::Global), _, _, _) => fts_map(
                &conn,
                "SELECT m.id, m.layer, m.channel, m.conv_kind, m.conv_peer, m.person, m.text, m.ts
                 FROM memories_fts f JOIN memories m ON m.id = f.id
                 WHERE memories_fts MATCH ?1 AND m.layer = 'global'
                 ORDER BY m.ts DESC LIMIT ?2",
                params![fts_query, limit],
            ),
            (Some(Layer::Conversation), Some(ch), Some(c), _) => fts_map(
                &conn,
                "SELECT m.id, m.layer, m.channel, m.conv_kind, m.conv_peer, m.person, m.text, m.ts
                 FROM memories_fts f JOIN memories m ON m.id = f.id
                 WHERE memories_fts MATCH ?1 AND m.layer = 'conversation'
                   AND m.channel = ?2 AND m.conv_kind = ?3 AND m.conv_peer = ?4
                 ORDER BY m.ts DESC LIMIT ?5",
                params![fts_query, ch, c.kind.as_str(), c.peer, limit],
            ),
            (Some(Layer::Person), Some(ch), _, Some(p)) => fts_map(
                &conn,
                "SELECT m.id, m.layer, m.channel, m.conv_kind, m.conv_peer, m.person, m.text, m.ts
                 FROM memories_fts f JOIN memories m ON m.id = f.id
                 WHERE memories_fts MATCH ?1 AND m.layer = 'person'
                   AND m.channel = ?2 AND m.person = ?3
                 ORDER BY m.ts DESC LIMIT ?4",
                params![fts_query, ch, p, limit],
            ),
            (Some(Layer::Conversation), _, _, _) | (Some(Layer::Person), _, _, _) => {
                return Err(MemoryError::Invalid(
                    "search filters incomplete for layer".into(),
                ));
            }
            (None, Some(ch), Some(c), _) => fts_map(
                &conn,
                "SELECT m.id, m.layer, m.channel, m.conv_kind, m.conv_peer, m.person, m.text, m.ts
                 FROM memories_fts f JOIN memories m ON m.id = f.id
                 WHERE memories_fts MATCH ?1 AND m.channel = ?2
                   AND m.conv_kind = ?3 AND m.conv_peer = ?4
                 ORDER BY m.ts DESC LIMIT ?5",
                params![fts_query, ch, c.kind.as_str(), c.peer, limit],
            ),
            (None, Some(ch), _, Some(p)) => fts_map(
                &conn,
                "SELECT m.id, m.layer, m.channel, m.conv_kind, m.conv_peer, m.person, m.text, m.ts
                 FROM memories_fts f JOIN memories m ON m.id = f.id
                 WHERE memories_fts MATCH ?1 AND m.channel = ?2 AND m.person = ?3
                 ORDER BY m.ts DESC LIMIT ?4",
                params![fts_query, ch, p, limit],
            ),
            (None, Some(ch), _, _) => fts_map(
                &conn,
                "SELECT m.id, m.layer, m.channel, m.conv_kind, m.conv_peer, m.person, m.text, m.ts
                 FROM memories_fts f JOIN memories m ON m.id = f.id
                 WHERE memories_fts MATCH ?1 AND m.channel = ?2
                 ORDER BY m.ts DESC LIMIT ?3",
                params![fts_query, ch, limit],
            ),
            _ => fts_map(
                &conn,
                "SELECT m.id, m.layer, m.channel, m.conv_kind, m.conv_peer, m.person, m.text, m.ts
                 FROM memories_fts f JOIN memories m ON m.id = f.id
                 WHERE memories_fts MATCH ?1 ORDER BY m.ts DESC LIMIT ?2",
                params![fts_query, limit],
            ),
        };
        if rows.is_empty() {
            let like = format!("%{query}%");
            let mut stmt = conn.prepare(
                "SELECT id, layer, channel, conv_kind, conv_peer, person, text, ts
                 FROM memories WHERE text LIKE ?1 ORDER BY ts DESC LIMIT ?2",
            )?;
            rows = stmt
                .query_map(params![like, limit], row_to_memory)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            if let Some(layer) = layer {
                rows.retain(|m| m.layer == layer);
            }
        }
        Ok(rows)
    }
}

fn fts_map(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Vec<Memory> {
    let Ok(mut stmt) = conn.prepare(sql) else {
        return Vec::new();
    };
    stmt.query_map(params, row_to_memory)
        .ok()
        .map(|m| m.collect::<rusqlite::Result<Vec<_>>>().unwrap_or_default())
        .unwrap_or_default()
}

fn row_to_memory(row: &rusqlite::Row<'_>) -> rusqlite::Result<Memory> {
    let layer_s: String = row.get(1)?;
    let kind_s: Option<String> = row.get(3)?;
    let peer: Option<String> = row.get(4)?;
    let conversation = match (kind_s.as_deref().and_then(ConversationKind::parse), peer) {
        (Some(kind), Some(peer)) => Some(Conversation { kind, peer }),
        _ => None,
    };
    Ok(Memory {
        id: row.get(0)?,
        layer: Layer::parse(&layer_s).unwrap_or(Layer::Global),
        channel: row.get(2)?,
        conversation,
        person: row.get(5)?,
        text: row.get(6)?,
        ts: row.get(7)?,
    })
}

fn fts_safe(query: &str) -> String {
    let cleaned: String = query
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' { c } else { ' ' })
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        "\"\"".into()
    } else {
        format!("\"{cleaned}\"")
    }
}
