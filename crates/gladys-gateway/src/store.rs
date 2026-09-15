use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ulid::Ulid;

use crate::error::{GatewayError, Result};
use crate::types::{Conversation, ConversationKind};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub kind: String,
    pub account: String,
    pub channel: String,
    pub conversation: Conversation,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cron: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_fire: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_fire: Option<i64>,
}

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
CREATE TABLE IF NOT EXISTS acp_sessions (
  conv_key TEXT PRIMARY KEY,
  session_id TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS scheduler_triggers (
  id TEXT PRIMARY KEY,
  account TEXT NOT NULL,
  channel TEXT NOT NULL,
  conv_kind TEXT NOT NULL,
  conv_peer TEXT NOT NULL,
  text TEXT,
  payload TEXT,
  ts INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS scheduler_jobs (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  account TEXT NOT NULL,
  channel TEXT NOT NULL,
  conv_kind TEXT NOT NULL,
  conv_peer TEXT NOT NULL,
  text TEXT NOT NULL,
  cron_expr TEXT,
  command TEXT,
  args TEXT NOT NULL DEFAULT '[]',
  next_fire INTEGER,
  last_fire INTEGER
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
            .map_err(|_| GatewayError::Invalid("store mutex poisoned".into()))
    }

    pub fn get_session(&self, conv_key: &str) -> Result<Option<String>> {
        let conn = self.lock()?;
        let v = conn
            .query_row(
                "SELECT session_id FROM acp_sessions WHERE conv_key = ?1",
                params![conv_key],
                |r| r.get(0),
            )
            .optional_row()?;
        Ok(v)
    }

    pub fn put_session(&self, conv_key: &str, session_id: &str) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO acp_sessions (conv_key, session_id) VALUES (?1, ?2)
             ON CONFLICT(conv_key) DO UPDATE SET session_id = excluded.session_id",
            params![conv_key, session_id],
        )?;
        Ok(())
    }

    pub fn clear_sessions(&self) -> Result<()> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM acp_sessions", [])?;
        Ok(())
    }

    pub fn drop_session(&self, conv_key: &str) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "DELETE FROM acp_sessions WHERE conv_key = ?1",
            params![conv_key],
        )?;
        Ok(())
    }

    pub fn log_scheduler(
        &self,
        account: &str,
        channel: &str,
        conv: &Conversation,
        text: Option<&str>,
        payload: Option<&Value>,
    ) -> Result<String> {
        let id = Ulid::generate().to_string();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO scheduler_triggers
             (id, account, channel, conv_kind, conv_peer, text, payload, ts)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id,
                account,
                channel,
                conv.kind.as_str(),
                conv.peer,
                text,
                payload.map(|p| p.to_string()),
                ts,
            ],
        )?;
        Ok(id)
    }

    pub fn insert_job(&self, job: &Job) -> Result<()> {
        let args = serde_json::to_string(&job.args)?;
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO scheduler_jobs
             (id, kind, account, channel, conv_kind, conv_peer, text, cron_expr, command, args, next_fire, last_fire)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                job.id,
                job.kind,
                job.account,
                job.channel,
                job.conversation.kind.as_str(),
                job.conversation.peer,
                job.text,
                job.cron,
                job.command,
                args,
                job.next_fire,
                job.last_fire,
            ],
        )?;
        Ok(())
    }

    pub fn get_job(&self, id: &str) -> Result<Option<Job>> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT id, kind, account, channel, conv_kind, conv_peer, text, cron_expr, command, args, next_fire, last_fire
             FROM scheduler_jobs WHERE id = ?1",
            params![id],
            job_from_row,
        )
        .optional_row()
        .map_err(Into::into)
    }

    pub fn list_jobs(&self) -> Result<Vec<Job>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, kind, account, channel, conv_kind, conv_peer, text, cron_expr, command, args, next_fire, last_fire
             FROM scheduler_jobs ORDER BY id",
        )?;
        let rows = stmt.query_map([], job_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    pub fn delete_job(&self, id: &str) -> Result<bool> {
        let conn = self.lock()?;
        let n = conn.execute("DELETE FROM scheduler_jobs WHERE id = ?1", params![id])?;
        Ok(n > 0)
    }

    pub fn due_jobs(&self, now: i64) -> Result<Vec<Job>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, kind, account, channel, conv_kind, conv_peer, text, cron_expr, command, args, next_fire, last_fire
             FROM scheduler_jobs WHERE next_fire IS NOT NULL AND next_fire <= ?1",
        )?;
        let rows = stmt.query_map(params![now], job_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    pub fn update_job_fire(&self, id: &str, last: i64, next: Option<i64>) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE scheduler_jobs SET last_fire = ?1, next_fire = ?2 WHERE id = ?3",
            params![last, next, id],
        )?;
        Ok(())
    }
}

fn job_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Job> {
    let conv_kind_raw: String = r.get(4)?;
    let conv_kind = ConversationKind::parse(&conv_kind_raw).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            4,
            rusqlite::types::Type::Text,
            format!("bad conv kind {conv_kind_raw}").into(),
        )
    })?;
    let args_raw: String = r.get(9)?;
    let args = serde_json::from_str(&args_raw).unwrap_or_default();
    Ok(Job {
        id: r.get(0)?,
        kind: r.get(1)?,
        account: r.get(2)?,
        channel: r.get(3)?,
        conversation: Conversation {
            kind: conv_kind,
            peer: r.get(5)?,
        },
        text: r.get(6)?,
        cron: r.get(7)?,
        command: r.get(8)?,
        args,
        next_fire: r.get(10)?,
        last_fire: r.get(11)?,
    })
}

trait OptionalRow<T> {
    fn optional_row(self) -> rusqlite::Result<Option<T>>;
}

impl<T> OptionalRow<T> for rusqlite::Result<T> {
    fn optional_row(self) -> rusqlite::Result<Option<T>> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
