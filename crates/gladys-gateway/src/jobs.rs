use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use cron::Schedule;
use ulid::Ulid;

use crate::dispatch::Dispatch;
use crate::error::{GatewayError, Result};
use crate::store::{Job, Store};
use crate::types::{Actor, Conversation, Envelope, Part};
#[derive(Clone)]
pub struct Jobs {
    inner: Arc<Inner>,
}

struct Inner {
    store: Store,
    dispatch: Dispatch,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct JobSpec {
    pub kind: String,
    pub account: String,
    pub channel: String,
    pub conversation: Conversation,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub cron: Option<String>,
    #[serde(default)]
    pub after_secs: Option<u64>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
}

impl Jobs {
    pub fn new(store: Store, dispatch: Dispatch) -> Self {
        Self {
            inner: Arc::new(Inner { store, dispatch }),
        }
    }

    pub fn start(&self) {
        let this = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                if let Err(e) = this.tick().await {
                    tracing::warn!("daemon tick: {e}");
                }
            }
        });
    }

    pub async fn create(&self, spec: JobSpec) -> Result<Job> {
        let job = build_job(spec)?;
        self.inner.store.insert_job(&job)?;
        Ok(job)
    }

    pub fn list(&self) -> Result<Vec<Job>> {
        self.inner.store.list_jobs()
    }

    pub async fn cancel(&self, id: &str) -> Result<bool> {
        self.inner.store.delete_job(id)
    }

    pub async fn tick(&self) -> Result<()> {
        self.tick_at(now_ts()).await
    }

    pub async fn tick_at(&self, now: i64) -> Result<()> {
        let due = self.inner.store.due_jobs(now)?;
        for job in due {
            let env = envelope(&job, &job.text);
            if let Err(e) = self.inner.dispatch.handle_daemon(env).await {
                tracing::warn!("daemon fire {}: {e}", job.id);
            }
            match job.kind.as_str() {
                "delay" => {
                    self.inner.store.delete_job(&job.id)?;
                }
                "cron" => {
                    let next = job
                        .cron
                        .as_deref()
                        .map(cron_next)
                        .transpose()?
                        .flatten();
                    self.inner.store.update_job_fire(&job.id, now, next)?;
                }
                _ => {}
            }
        }
        Ok(())
    }
}


fn build_job(spec: JobSpec) -> Result<Job> {
    let id = Ulid::generate().to_string();
    match spec.kind.as_str() {
        "cron" => {
            let expr = spec
                .cron
                .clone()
                .ok_or_else(|| GatewayError::Invalid("cron required".into()))?;
            let next = cron_next(&expr)?.ok_or_else(|| GatewayError::Invalid("cron has no next".into()))?;
            Ok(Job {
                id,
                kind: "cron".into(),
                account: spec.account,
                channel: spec.channel,
                conversation: spec.conversation,
                text: spec.text,
                cron: Some(expr),
                command: None,
                args: Vec::new(),
                next_fire: Some(next),
                last_fire: None,
            })
        }
        "delay" => {
            let secs = spec
                .after_secs
                .ok_or_else(|| GatewayError::Invalid("after_secs required".into()))?;
            Ok(Job {
                id,
                kind: "delay".into(),
                account: spec.account,
                channel: spec.channel,
                conversation: spec.conversation,
                text: spec.text,
                cron: None,
                command: None,
                args: Vec::new(),
                next_fire: Some(now_ts() + secs as i64),
                last_fire: None,
            })
        }
        other => Err(GatewayError::Invalid(format!("unknown kind {other}"))),
    }
}

fn cron_next(expr: &str) -> Result<Option<i64>> {
    let schedule =
        Schedule::from_str(expr).map_err(|e| GatewayError::Invalid(format!("cron: {e}")))?;
    Ok(schedule.upcoming(Utc).next().map(|t| t.timestamp()))
}

fn now_ts() -> i64 {
    Utc::now().timestamp()
}

fn envelope(job: &Job, text: &str) -> Envelope {
    Envelope {
        id: String::new(),
        channel: job.channel.clone(),
        account: job.account.clone(),
        conversation: job.conversation.clone(),
        direction: "in".into(),
        sender: Actor {
            id: "daemon".into(),
            name: Some("daemon".into()),
        },
        parts: vec![Part::Text {
            text: text.to_string(),
        }],
        reply_to: None,
    }
}
