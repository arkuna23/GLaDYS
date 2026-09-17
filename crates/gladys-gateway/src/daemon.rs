use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::Result;
use crate::types::{Actor, Conversation};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Registry {
    #[serde(default)]
    pub commands: Vec<CommandReg>,
    #[serde(default)]
    pub handlers: Vec<HandlerReg>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommandReg {
    pub name: String,
    pub level: String,
    #[serde(default)]
    pub docs: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HandlerReg {
    pub name: String,
    #[serde(default = "default_event")]
    pub event: String,
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub peer: Option<String>,
}

fn default_event() -> String {
    "message.inbound".into()
}

impl Registry {
    pub fn command_level(&self, name: &str) -> Option<&str> {
        self.commands
            .iter()
            .find(|c| c.name == name)
            .map(|c| c.level.as_str())
    }

    pub fn matching_handlers(
        &self,
        event: &str,
        account: &str,
        kind: &str,
        peer: &str,
    ) -> Vec<&HandlerReg> {
        self.handlers
            .iter()
            .filter(|h| {
                h.event == event
                    && (h.account.as_deref().is_none_or(|a| a == account))
                    && (h.kind.as_deref().is_none_or(|k| k == kind))
                    && (h.peer.as_deref().is_none_or(|p| p == peer))
            })
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRequest {
    pub kind: String,
    pub name: String,
    pub account: String,
    pub channel: String,
    pub conversation: Conversation,
    pub sender: Actor,
    pub text: String,
    pub argv: Vec<String>,
    pub event: String,
    pub payload: Value,
    pub lang: String,
}

#[async_trait]
pub trait DaemonIo: Send + Sync {
    async fn run(&self, req: RunRequest) -> Result<()>;
}

pub struct HttpDaemon {
    http: reqwest::Client,
    url: String,
    token: String,
}

impl HttpDaemon {
    pub fn new(url: String, token: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            url: url.trim_end_matches('/').into(),
            token,
        }
    }
}

#[async_trait]
impl DaemonIo for HttpDaemon {
    async fn run(&self, req: RunRequest) -> Result<()> {
        let resp = self
            .http
            .post(format!("{}/v1/run", self.url))
            .header("Authorization", format!("Bearer {}", self.token))
            .json(&req)
            .send()
            .await?;
        if !resp.status().is_success() {
            tracing::warn!("daemon run {}: {}", req.name, resp.status());
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct RecDaemon {
    pub runs: tokio::sync::Mutex<Vec<RunRequest>>,
}

#[async_trait]
impl DaemonIo for RecDaemon {
    async fn run(&self, req: RunRequest) -> Result<()> {
        self.runs.lock().await.push(req);
        Ok(())
    }
}

pub fn run_json(
    kind: &str,
    name: &str,
    account: &str,
    channel: &str,
    conversation: &Conversation,
    sender: &Actor,
    text: &str,
    argv: Vec<String>,
    event: &str,
    payload: Value,
    lang: &str,
) -> RunRequest {
    RunRequest {
        kind: kind.into(),
        name: name.into(),
        account: account.into(),
        channel: channel.into(),
        conversation: conversation.clone(),
        sender: sender.clone(),
        text: text.into(),
        argv,
        event: event.into(),
        payload,
        lang: lang.into(),
    }
}

pub fn payload_from_env(env: &crate::types::Envelope) -> Value {
    json!(env)
}

pub type SharedDaemon = Arc<dyn DaemonIo>;
