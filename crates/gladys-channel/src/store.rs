use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection, OptionalExtension};
use ulid::Ulid;

use crate::error::{ChannelError, Result};
use crate::message::{
    Conversation, ConversationInfo, Envelope, NativePayload, PlatformEvent, ReplyTo,
};

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS messages (
  id TEXT PRIMARY KEY,
  channel TEXT NOT NULL,
  account TEXT NOT NULL,
  conv_kind TEXT NOT NULL,
  conv_peer TEXT NOT NULL,
  direction TEXT NOT NULL,
  sender_id TEXT NOT NULL,
  sender_name TEXT,
  ts INTEGER NOT NULL,
  platform_id TEXT,
  reply_to_id TEXT,
  reply_to_platform_id TEXT,
  body_json TEXT NOT NULL,
  native_json TEXT,
  text TEXT NOT NULL,
  recalled INTEGER NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX IF NOT EXISTS messages_platform
  ON messages(channel, account, platform_id)
  WHERE platform_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS messages_conv
  ON messages(channel, account, conv_kind, conv_peer, ts);

CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
  id UNINDEXED,
  text
);

CREATE TABLE IF NOT EXISTS conversations (
  channel TEXT NOT NULL,
  account TEXT NOT NULL,
  conv_kind TEXT NOT NULL,
  conv_peer TEXT NOT NULL,
  title TEXT,
  last_ts INTEGER NOT NULL,
  last_message_id TEXT,
  PRIMARY KEY (channel, account, conv_kind, conv_peer)
);

CREATE TABLE IF NOT EXISTS events (
  id TEXT PRIMARY KEY,
  channel TEXT NOT NULL,
  account TEXT NOT NULL,
  ts INTEGER NOT NULL,
  kind TEXT NOT NULL,
  conv_kind TEXT,
  conv_peer TEXT,
  native_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS blobs (
  id TEXT PRIMARY KEY,
  mime TEXT,
  filename TEXT,
  size INTEGER NOT NULL,
  path TEXT NOT NULL
);
"#;

#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
    blob_dir: PathBuf,
}

impl Store {
    pub fn open(sqlite: impl AsRef<Path>, blob_dir: impl AsRef<Path>) -> Result<Self> {
        let sqlite = sqlite.as_ref();
        if let Some(parent) = sqlite.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let blob_dir = blob_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&blob_dir)?;
        let conn = Connection::open(sqlite)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            blob_dir,
        })
    }

    pub fn memory(blob_dir: impl AsRef<Path>) -> Result<Self> {
        let blob_dir = blob_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&blob_dir)?;
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            blob_dir,
        })
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| ChannelError::Other("store mutex poisoned".into()))
    }

    /// Returns true if the row was inserted (false = duplicate platform_id).
    pub fn insert_message(&self, env: &Envelope) -> Result<bool> {
        let conn = self.lock()?;
        let body = serde_json::to_string(&env.parts)?;
        let native = env
            .native
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let direction = match env.direction {
            crate::message::Direction::In => "in",
            crate::message::Direction::Out => "out",
        };
        let inserted = conn.execute(
            "INSERT OR IGNORE INTO messages
             (id, channel, account, conv_kind, conv_peer, direction, sender_id, sender_name,
              ts, platform_id, reply_to_id, reply_to_platform_id, body_json, native_json, text, recalled)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                env.id,
                env.channel,
                env.account,
                env.conversation.kind_str(),
                env.conversation.peer,
                direction,
                env.sender.id,
                env.sender.name,
                env.ts,
                env.platform_id,
                env.reply_to.as_ref().and_then(|r| r.id.as_deref()),
                env.reply_to
                    .as_ref()
                    .and_then(|r| r.platform_id.as_deref()),
                body,
                native,
                env.flattened_text(),
                env.recalled as i64,
            ],
        )?;
        if inserted == 0 {
            return Ok(false);
        }
        conn.execute(
            "INSERT INTO messages_fts (id, text) VALUES (?1, ?2)",
            params![env.id, env.flattened_text()],
        )?;
        conn.execute(
            "INSERT INTO conversations (channel, account, conv_kind, conv_peer, last_ts, last_message_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (channel, account, conv_kind, conv_peer) DO UPDATE SET
               last_ts = excluded.last_ts,
               last_message_id = excluded.last_message_id",
            params![
                env.channel,
                env.account,
                env.conversation.kind_str(),
                env.conversation.peer,
                env.ts,
                env.id,
            ],
        )?;
        Ok(true)
    }

    pub fn get_by_id(&self, id: &str) -> Result<Option<Envelope>> {
        let conn = self.lock()?;
        let row = conn
            .query_row(
                "SELECT id, channel, account, conv_kind, conv_peer, direction, sender_id, sender_name,
                        ts, platform_id, reply_to_id, reply_to_platform_id, body_json, native_json, recalled
                 FROM messages WHERE id = ?1",
                params![id],
                row_to_envelope,
            )
            .optional()?;
        Ok(row)
    }

    pub fn get_by_platform(
        &self,
        channel: &str,
        account: &str,
        platform_id: &str,
    ) -> Result<Option<Envelope>> {
        let conn = self.lock()?;
        let row = conn
            .query_row(
                "SELECT id, channel, account, conv_kind, conv_peer, direction, sender_id, sender_name,
                        ts, platform_id, reply_to_id, reply_to_platform_id, body_json, native_json, recalled
                 FROM messages WHERE channel = ?1 AND account = ?2 AND platform_id = ?3",
                params![channel, account, platform_id],
                row_to_envelope,
            )
            .optional()?;
        Ok(row)
    }

    pub fn history(
        &self,
        channel: &str,
        account: &str,
        conv: &Conversation,
        limit: u32,
        before_ts: Option<i64>,
    ) -> Result<Vec<Envelope>> {
        let conn = self.lock()?;
        let limit = limit.clamp(1, 200) as i64;
        let mut rows = if let Some(before) = before_ts {
            let mut stmt = conn.prepare(
                "SELECT id, channel, account, conv_kind, conv_peer, direction, sender_id, sender_name,
                        ts, platform_id, reply_to_id, reply_to_platform_id, body_json, native_json, recalled
                 FROM messages
                 WHERE channel = ?1 AND account = ?2 AND conv_kind = ?3 AND conv_peer = ?4 AND ts < ?5
                 ORDER BY ts DESC LIMIT ?6",
            )?;
            let mapped = stmt.query_map(
                params![channel, account, conv.kind_str(), conv.peer, before, limit],
                row_to_envelope,
            )?;
            mapped.collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, channel, account, conv_kind, conv_peer, direction, sender_id, sender_name,
                        ts, platform_id, reply_to_id, reply_to_platform_id, body_json, native_json, recalled
                 FROM messages
                 WHERE channel = ?1 AND account = ?2 AND conv_kind = ?3 AND conv_peer = ?4
                 ORDER BY ts DESC LIMIT ?5",
            )?;
            let mapped = stmt.query_map(
                params![channel, account, conv.kind_str(), conv.peer, limit],
                row_to_envelope,
            )?;
            mapped.collect::<rusqlite::Result<Vec<_>>>()?
        };
        rows.reverse();
        Ok(rows)
    }

    pub fn search(
        &self,
        query: &str,
        account: Option<&str>,
        limit: u32,
    ) -> Result<Vec<Envelope>> {
        let conn = self.lock()?;
        let limit = limit.clamp(1, 200) as i64;
        let fts_query = fts_safe(query);
        let mut out = Vec::new();
        if let Some(account) = account {
            let mut stmt = conn.prepare(
                "SELECT m.id, m.channel, m.account, m.conv_kind, m.conv_peer, m.direction, m.sender_id, m.sender_name,
                        m.ts, m.platform_id, m.reply_to_id, m.reply_to_platform_id, m.body_json, m.native_json, m.recalled
                 FROM messages_fts f
                 JOIN messages m ON m.id = f.id
                 WHERE messages_fts MATCH ?1 AND m.account = ?2
                 ORDER BY m.ts DESC LIMIT ?3",
            )?;
            let mapped = stmt.query_map(params![fts_query, account, limit], row_to_envelope);
            if let Ok(mapped) = mapped {
                out = mapped.collect::<rusqlite::Result<Vec<_>>>()?;
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT m.id, m.channel, m.account, m.conv_kind, m.conv_peer, m.direction, m.sender_id, m.sender_name,
                        m.ts, m.platform_id, m.reply_to_id, m.reply_to_platform_id, m.body_json, m.native_json, m.recalled
                 FROM messages_fts f
                 JOIN messages m ON m.id = f.id
                 WHERE messages_fts MATCH ?1
                 ORDER BY m.ts DESC LIMIT ?2",
            )?;
            let mapped = stmt.query_map(params![fts_query, limit], row_to_envelope);
            if let Ok(mapped) = mapped {
                out = mapped.collect::<rusqlite::Result<Vec<_>>>()?;
            }
        }
        if out.is_empty() {
            let like = format!("%{query}%");
            if let Some(account) = account {
                let mut stmt = conn.prepare(
                    "SELECT id, channel, account, conv_kind, conv_peer, direction, sender_id, sender_name,
                            ts, platform_id, reply_to_id, reply_to_platform_id, body_json, native_json, recalled
                     FROM messages WHERE account = ?1 AND text LIKE ?2
                     ORDER BY ts DESC LIMIT ?3",
                )?;
                let mapped = stmt.query_map(params![account, like, limit], row_to_envelope)?;
                out = mapped.collect::<rusqlite::Result<Vec<_>>>()?;
            } else {
                let mut stmt = conn.prepare(
                    "SELECT id, channel, account, conv_kind, conv_peer, direction, sender_id, sender_name,
                            ts, platform_id, reply_to_id, reply_to_platform_id, body_json, native_json, recalled
                     FROM messages WHERE text LIKE ?1
                     ORDER BY ts DESC LIMIT ?2",
                )?;
                let mapped = stmt.query_map(params![like, limit], row_to_envelope)?;
                out = mapped.collect::<rusqlite::Result<Vec<_>>>()?;
            }
        }
        Ok(out)
    }

    pub fn mark_recalled(
        &self,
        channel: &str,
        account: &str,
        platform_id: &str,
    ) -> Result<Option<Envelope>> {
        {
            let conn = self.lock()?;
            conn.execute(
                "UPDATE messages SET recalled = 1
                 WHERE channel = ?1 AND account = ?2 AND platform_id = ?3",
                params![channel, account, platform_id],
            )?;
        }
        self.get_by_platform(channel, account, platform_id)
    }

    pub fn insert_event(&self, ev: &PlatformEvent) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT OR IGNORE INTO events
             (id, channel, account, ts, kind, conv_kind, conv_peer, native_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                ev.id,
                ev.channel,
                ev.account,
                ev.ts,
                ev.kind,
                ev.conversation.as_ref().map(|c| c.kind_str()),
                ev.conversation.as_ref().map(|c| c.peer.as_str()),
                serde_json::to_string(&ev.native)?,
            ],
        )?;
        Ok(())
    }

    pub fn conversations(&self, account: Option<&str>) -> Result<Vec<ConversationInfo>> {
        let conn = self.lock()?;
        if let Some(account) = account {
            let mut stmt = conn.prepare(
                "SELECT channel, account, conv_kind, conv_peer, title, last_ts, last_message_id
                 FROM conversations WHERE account = ?1 ORDER BY last_ts DESC",
            )?;
            let mapped = stmt.query_map(params![account], row_to_conversation)?;
            Ok(mapped.collect::<rusqlite::Result<Vec<_>>>()?)
        } else {
            let mut stmt = conn.prepare(
                "SELECT channel, account, conv_kind, conv_peer, title, last_ts, last_message_id
                 FROM conversations ORDER BY last_ts DESC",
            )?;
            let mapped = stmt.query_map([], row_to_conversation)?;
            Ok(mapped.collect::<rusqlite::Result<Vec<_>>>()?)
        }
    }

    pub fn save_blob(&self, bytes: &[u8], mime: Option<&str>, filename: Option<&str>) -> Result<String> {
        let id = Ulid::generate().to_string();
        let path = self.blob_dir.join(&id);
        std::fs::write(&path, bytes)?;
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO blobs (id, mime, filename, size, path) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                id,
                mime,
                filename,
                bytes.len() as i64,
                path.to_string_lossy()
            ],
        )?;
        Ok(id)
    }

    pub fn get_blob(&self, id: &str) -> Result<Option<(PathBuf, Option<String>)>> {
        let conn = self.lock()?;
        let row = conn
            .query_row(
                "SELECT path, mime FROM blobs WHERE id = ?1",
                params![id],
                |row| {
                    Ok((
                        PathBuf::from(row.get::<_, String>(0)?),
                        row.get::<_, Option<String>>(1)?,
                    ))
                },
            )
            .optional()?;
        Ok(row)
    }
}

fn row_to_envelope(row: &rusqlite::Row<'_>) -> rusqlite::Result<Envelope> {
    let kind: String = row.get(3)?;
    let peer: String = row.get(4)?;
    let direction: String = row.get(5)?;
    let body: String = row.get(12)?;
    let native: Option<String> = row.get(13)?;
    let reply_to_id: Option<String> = row.get(10)?;
    let reply_to_platform_id: Option<String> = row.get(11)?;
    let reply_to = match (reply_to_id, reply_to_platform_id) {
        (None, None) => None,
        (id, platform_id) => Some(ReplyTo {
            id,
            platform_id,
            sender: None,
        }),
    };
    Ok(Envelope {
        id: row.get(0)?,
        channel: row.get(1)?,
        account: row.get(2)?,
        conversation: Conversation::from_parts(&kind, peer).unwrap_or_else(|| Conversation::dm(peer_fallback(&kind))),
        direction: if direction == "out" {
            crate::message::Direction::Out
        } else {
            crate::message::Direction::In
        },
        sender: crate::message::Actor {
            id: row.get(6)?,
            name: row.get(7)?,
        },
        ts: row.get(8)?,
        platform_id: row.get(9)?,
        reply_to,
        parts: serde_json::from_str(&body).unwrap_or_default(),
        native: native.and_then(|s| serde_json::from_str::<NativePayload>(&s).ok()),
        recalled: row.get::<_, i64>(14).unwrap_or(0) != 0,
    })
}

fn peer_fallback(_: &str) -> String {
    String::new()
}

fn row_to_conversation(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConversationInfo> {
    let kind: String = row.get(2)?;
    let peer: String = row.get(3)?;
    Ok(ConversationInfo {
        channel: row.get(0)?,
        account: row.get(1)?,
        conversation: Conversation::from_parts(&kind, peer.clone())
            .unwrap_or_else(|| Conversation::dm(peer)),
        title: row.get(4)?,
        last_ts: row.get(5)?,
        last_message_id: row.get(6)?,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{Actor, Direction, Part};

    fn env(id: &str, platform: &str, text: &str, ts: i64) -> Envelope {
        Envelope {
            id: id.into(),
            channel: "onebot".into(),
            account: "qq".into(),
            conversation: Conversation::group("100"),
            direction: Direction::In,
            sender: Actor {
                id: "1".into(),
                name: Some("a".into()),
            },
            ts,
            platform_id: Some(platform.into()),
            reply_to: None,
            parts: vec![Part::text(text)],
            native: None,
            recalled: false,
        }
    }

    #[test]
    fn insert_dedup_history_search_recall() {
        let dir = {
            let p = std::env::temp_dir().join("gladys-store-test");
            std::fs::create_dir_all(&p).expect("temp");
            p
        };
        let store = Store::memory(&dir).expect("memory store");
        assert!(store.insert_message(&env("a", "p1", "hello world", 1)).unwrap());
        assert!(!store.insert_message(&env("b", "p1", "dup", 2)).unwrap());
        store
            .insert_message(&env("c", "p2", "later ping", 3))
            .unwrap();
        let hist = store
            .history("onebot", "qq", &Conversation::group("100"), 10, None)
            .unwrap();
        assert_eq!(hist.len(), 2);
        assert_eq!(hist[0].id, "a");
        assert_eq!(hist[1].id, "c");
        let hits = store.search("hello", Some("qq"), 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "a");
        let recalled = store.mark_recalled("onebot", "qq", "p1").unwrap().unwrap();
        assert!(recalled.recalled);
        let id = store.save_blob(b"png-bytes", Some("image/png"), None).unwrap();
        let (path, mime) = store.get_blob(&id).unwrap().unwrap();
        assert_eq!(mime.as_deref(), Some("image/png"));
        assert_eq!(std::fs::read(path).unwrap(), b"png-bytes");
    }
}
