use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::{DaemonError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    pub name: String,
    pub level: String,
    pub script: String,
    #[serde(default)]
    pub docs: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Handler {
    pub name: String,
    #[serde(default = "default_event")]
    pub event: String,
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub peer: Option<String>,
    pub script: String,
    #[serde(default)]
    pub docs: String,
}

fn default_event() -> String {
    "message.inbound".into()
}

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
CREATE TABLE IF NOT EXISTS commands (
  name TEXT PRIMARY KEY,
  level TEXT NOT NULL,
  script TEXT NOT NULL,
  docs TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS handlers (
  name TEXT PRIMARY KEY,
  event TEXT NOT NULL,
  account TEXT,
  kind TEXT,
  peer TEXT,
  script TEXT NOT NULL,
  docs TEXT NOT NULL DEFAULT ''
);
"#;

#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let conn = Connection::open(path)?;
        migrate(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        migrate(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| DaemonError::Invalid("store lock".into()))
    }

    pub fn put_command(&self, c: &Command) -> Result<()> {
        if c.name.is_empty() || c.script.is_empty() {
            return Err(DaemonError::Invalid("name and script required".into()));
        }
        if c.level != "owner" && c.level != "user" {
            return Err(DaemonError::Invalid("level must be owner or user".into()));
        }
        self.lock()?.execute(
            "INSERT INTO commands(name, level, script, docs) VALUES(?1,?2,?3,?4)
             ON CONFLICT(name) DO UPDATE SET level=?2, script=?3, docs=?4",
            params![c.name, c.level, c.script, c.docs],
        )?;
        Ok(())
    }

    pub fn get_command(&self, name: &str) -> Result<Command> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT name, level, script, docs FROM commands WHERE name = ?1",
        )?;
        stmt.query_row(params![name], row_command)
            .optional()?
            .ok_or(DaemonError::NotFound)
    }

    pub fn list_commands(&self) -> Result<Vec<Command>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare("SELECT name, level, script, docs FROM commands ORDER BY name")?;
        let rows = stmt.query_map([], row_command)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn delete_command(&self, name: &str) -> Result<bool> {
        let n = self
            .lock()?
            .execute("DELETE FROM commands WHERE name = ?1", params![name])?;
        Ok(n > 0)
    }

    pub fn put_handler(&self, h: &Handler) -> Result<()> {
        if h.name.is_empty() || h.script.is_empty() {
            return Err(DaemonError::Invalid("name and script required".into()));
        }
        let event = if h.event.is_empty() {
            default_event()
        } else {
            h.event.clone()
        };
        self.lock()?.execute(
            "INSERT INTO handlers(name, event, account, kind, peer, script, docs)
             VALUES(?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(name) DO UPDATE SET
               event=?2, account=?3, kind=?4, peer=?5, script=?6, docs=?7",
            params![h.name, event, h.account, h.kind, h.peer, h.script, h.docs],
        )?;
        Ok(())
    }

    pub fn get_handler(&self, name: &str) -> Result<Handler> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT name, event, account, kind, peer, script, docs FROM handlers WHERE name = ?1",
        )?;
        stmt.query_row(params![name], row_handler)
            .optional()?
            .ok_or(DaemonError::NotFound)
    }

    pub fn list_handlers(&self) -> Result<Vec<Handler>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT name, event, account, kind, peer, script, docs FROM handlers ORDER BY name",
        )?;
        let rows = stmt.query_map([], row_handler)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn delete_handler(&self, name: &str) -> Result<bool> {
        let n = self
            .lock()?
            .execute("DELETE FROM handlers WHERE name = ?1", params![name])?;
        Ok(n > 0)
    }

    pub fn registry_snapshot(&self) -> Result<serde_json::Value> {
        let commands: Vec<serde_json::Value> = self
            .list_commands()?
            .into_iter()
            .map(|c| serde_json::json!({"name": c.name, "level": c.level, "docs": c.docs}))
            .collect();
        let handlers: Vec<serde_json::Value> = self
            .list_handlers()?
            .into_iter()
            .map(|h| {
                serde_json::json!({
                    "name": h.name,
                    "event": h.event,
                    "account": h.account,
                    "kind": h.kind,
                    "peer": h.peer,
                })
            })
            .collect();
        Ok(serde_json::json!({"commands": commands, "handlers": handlers}))
    }

    pub fn docs(&self, name: Option<&str>) -> Result<String> {
        let mut s = String::from(PROTOCOL_DOCS);
        s.push_str("\nMCP tools:\n");
        s.push_str(TOOL_DOCS);
        match name {
            Some(n) => {
                if let Ok(c) = self.get_command(n) {
                    s.push_str(&format_command(&c));
                } else if let Ok(h) = self.get_handler(n) {
                    s.push_str(&format_handler(&h));
                } else {
                    return Err(DaemonError::NotFound);
                }
            }
            None => {
                for c in self.list_commands()? {
                    s.push_str(&format_command(&c));
                }
                for h in self.list_handlers()? {
                    s.push_str(&format_handler(&h));
                }
            }
        }
        Ok(s)
    }
}

fn migrate(conn: &Connection) -> Result<()> {
    rebuild_if_legacy(conn, "commands", &[
        "CREATE TABLE commands (
          name TEXT PRIMARY KEY,
          level TEXT NOT NULL,
          script TEXT NOT NULL,
          docs TEXT NOT NULL DEFAULT ''
        )",
        "INSERT INTO commands(name, level, script, docs) SELECT name, level, '', COALESCE(docs,'') FROM commands_old",
    ])?;
    rebuild_if_legacy(conn, "handlers", &[
        "CREATE TABLE handlers (
          name TEXT PRIMARY KEY,
          event TEXT NOT NULL,
          account TEXT,
          kind TEXT,
          peer TEXT,
          script TEXT NOT NULL,
          docs TEXT NOT NULL DEFAULT ''
        )",
        "INSERT INTO handlers(name, event, account, kind, peer, script, docs)
         SELECT name, event, account, kind, peer, '', COALESCE(docs,'') FROM handlers_old",
    ])?;
    conn.execute_batch(SCHEMA)?;
    Ok(())
}

fn rebuild_if_legacy(conn: &Connection, table: &str, sql: &[&str]) -> Result<()> {
    if !table_exists(conn, table)? || has_column(conn, table, "script")? {
        return Ok(());
    }
    if !has_column(conn, table, "command")? {
        return Ok(());
    }
    conn.execute_batch(&format!("ALTER TABLE {table} RENAME TO {table}_old"))?;
    conn.execute_batch(sql[0])?;
    conn.execute_batch(sql[1])?;
    conn.execute_batch(&format!("DROP TABLE {table}_old"))?;
    Ok(())
}

fn table_exists(conn: &Connection, table: &str) -> Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
        params![table],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

fn has_column(conn: &Connection, table: &str, col: &str) -> Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = stmt.query_map([], |r| r.get::<_, String>(1))?;
    for n in names {
        if n? == col {
            return Ok(true);
        }
    }
    Ok(false)
}

fn row_command(row: &rusqlite::Row<'_>) -> rusqlite::Result<Command> {
    Ok(Command {
        name: row.get(0)?,
        level: row.get(1)?,
        script: row.get(2)?,
        docs: row.get(3)?,
    })
}

fn row_handler(row: &rusqlite::Row<'_>) -> rusqlite::Result<Handler> {
    Ok(Handler {
        name: row.get(0)?,
        event: row.get(1)?,
        account: row.get(2)?,
        kind: row.get(3)?,
        peer: row.get(4)?,
        script: row.get(5)?,
        docs: row.get(6)?,
    })
}

fn format_command(c: &Command) -> String {
    format!("\ncommand /{} level={}\n{}\n{}\n", c.name, c.level, c.docs, c.script)

}

fn format_handler(h: &Handler) -> String {
    format!(
        "\nhandler {} event={} filter={}/{}/{}\n{}\n{}\n",
        h.name,
        h.event,
        h.account.as_deref().unwrap_or("-"),
        h.kind.as_deref().unwrap_or("-"),
        h.peer.as_deref().unwrap_or("-"),
        h.docs,
        h.script
    )
}

pub const PROTOCOL_DOCS: &str = "\
Lua: PUT /v1/scripts/{name} with header X-Gladys-Type=command|handler (raw Lua body; X-Gladys-Level/Docs/Event/Account/Kind/Peer). POST /v1/scripts/check dry-runs mocks. 
MCP: daemon_docs, daemon_script (list|get|delete), daemon_job (cron|delay|list|cancel). One Lua 5.4 state per run. 
ctx is account, channel, kind, peer, sender, text, argv, event, payload, lang, name. 
channel.<name>(args) → MCP tool channel with op=name (channel.call → channel_call); fills account/conversation from ctx. 
memory.<name>(args) → MCP tool memory with op=name. job.cron/delay/list/cancel → Gateway. 
agent:prompt(text) then agent:wait() for ACP text. Do not send ACP text unless the script does. 
print logs only. No require/package. Timeout 180s. Group needs @ + /name; DM starts with /name. level owner|user.\n";

pub const TOOL_DOCS: &str = "\
- daemon_docs: this text plus each record

- daemon_script: op=list|get|delete, type=command|handler

- daemon_job: op=cron|delay|list|cancel

";
