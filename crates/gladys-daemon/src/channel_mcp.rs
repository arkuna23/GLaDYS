use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::error::{DaemonError, Result};

#[async_trait]
pub trait ToolCall: Send + Sync {
    async fn call(&self, name: &str, args: Value) -> Result<Value>;
}

#[derive(Default)]
pub struct RecTools {
    pub calls: Mutex<Vec<(String, Value)>>,
}

#[async_trait]
impl ToolCall for RecTools {
    async fn call(&self, name: &str, args: Value) -> Result<Value> {
        self.calls.lock().await.push((name.into(), args));
        Ok(json!({"ok": true}))
    }
}

pub struct McpHttp {
    http: Client,
    url: String,
    token: String,
    session: Mutex<Option<String>>,
    id: AtomicU64,
}

impl McpHttp {
    pub fn new(url: String, token: String) -> Self {
        Self {
            http: Client::new(),
            url,
            token,
            session: Mutex::new(None),
            id: AtomicU64::new(1),
        }
    }

    async fn ensure(&self) -> Result<String> {
        if let Some(s) = self.session.lock().await.clone() {
            return Ok(s);
        }
        let id = self.id.fetch_add(1, Ordering::Relaxed);
        let body = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "gladys-daemon", "version": "0.1.0"}
            }
        });
        let (sid, _) = self.post(body, None).await?;
        let sid = sid.ok_or_else(|| DaemonError::Mcp("no mcp-session-id".into()))?;
        let _ = self
            .post(
                json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/initialized"
                }),
                Some(&sid),
            )
            .await;
        *self.session.lock().await = Some(sid.clone());
        Ok(sid)
    }

    async fn post(&self, body: Value, session: Option<&str>) -> Result<(Option<String>, Value)> {
        let mut req = self
            .http
            .post(&self.url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/json, text/event-stream")
            .header("Content-Type", "application/json")
            .json(&body);
        if let Some(s) = session {
            req = req.header("mcp-session-id", s);
        }
        let resp = req.send().await?;
        let status = resp.status();
        let sid = resp
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .map(ToOwned::to_owned);
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(DaemonError::Mcp(format!("{status}: {text}")));
        }
        if text.is_empty() {
            return Ok((sid, Value::Null));
        }
        let parsed = parse_rpc(&text)?;
        Ok((sid, parsed))
    }
}

#[async_trait]
impl ToolCall for McpHttp {
    async fn call(&self, name: &str, args: Value) -> Result<Value> {
        let sid = match self.ensure().await {
            Ok(s) => s,
            Err(e) => {
                *self.session.lock().await = None;
                return Err(e);
            }
        };
        let id = self.id.fetch_add(1, Ordering::Relaxed);
        let body = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {"name": name, "arguments": args}
        });
        match self.post(body, Some(&sid)).await {
            Ok((_, v)) => mcp_result(v),
            Err(e) => {
                *self.session.lock().await = None;
                Err(e)
            }
        }
    }
}

pub fn mcp_result(v: Value) -> Result<Value> {
    if let Some(err) = v.get("error") {
        return Err(DaemonError::Mcp(err.to_string()));
    }
    let Some(result) = v.get("result") else {
        return Ok(v);
    };
    if let Some(sc) = result.get("structuredContent") {
        return Ok(sc.clone());
    }
    if let Some(text) = result.pointer("/content/0/text").and_then(Value::as_str) {
        return Ok(serde_json::from_str(text).unwrap_or_else(|_| json!(text)));
    }
    Ok(result.clone())
}

fn parse_rpc(text: &str) -> Result<Value> {
    let trimmed = text.trim();
    if trimmed.starts_with('{') {
        return Ok(serde_json::from_str(trimmed)?);
    }
    for line in trimmed.lines() {
        let line = line.trim();
        if let Some(data) = line.strip_prefix("data:") {
            let data = data.trim();
            if data.starts_with('{') {
                return Ok(serde_json::from_str(data)?);
            }
        }
    }
    Err(DaemonError::Mcp(format!("bad mcp body: {text}")))
}

pub type SharedTools = Arc<dyn ToolCall>;
pub type ChannelMcpHttp = McpHttp;
