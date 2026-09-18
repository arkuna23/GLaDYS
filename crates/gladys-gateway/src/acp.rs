use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{Mutex, oneshot};

use crate::config::McpConfig;
use crate::error::{GatewayError, Result};
use crate::store::Store;
use crate::types::{AgentInput, AgentOutput, ConvKey};
use crate::agent::AgentBackend;

pub struct AcpBackend {
    command: String,
    args: Vec<String>,
    cwd: Option<String>,
    mcp: Vec<McpConfig>,
    store: Store,
    procs: Mutex<HashMap<String, Arc<Mutex<AcpProc>>>>,
}

struct AcpProc {
    stdin: ChildStdin,
    next_id: u64,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>,
    out: Arc<Mutex<HashMap<String, StreamBuf>>>,
    _child: Child,
}

impl AcpBackend {
    pub fn new(
        command: String,
        args: Vec<String>,
        cwd: Option<String>,
        mcp: Vec<McpConfig>,
        store: Store,
    ) -> Self {
        Self {
            command,
            args,
            cwd,
            mcp,
            store,
            procs: Mutex::new(HashMap::new()),
        }
    }

    async fn spawn_one(&self) -> Result<AcpProc> {
        tracing::info!(cmd = %self.command, args = ?self.args, "spawn acp");
        let mut child = Command::new(&self.command)
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| GatewayError::Agent(format!("spawn {}: {e}", self.command)))?;
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        if let Some(status) = child
            .try_wait()
            .map_err(|e| GatewayError::Agent(e.to_string()))?
        {
            return Err(GatewayError::Agent(format!("acp exited immediately: {status}")));
        }
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| GatewayError::Agent("no stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| GatewayError::Agent("no stdout".into()))?;
        let pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let out: Arc<Mutex<HashMap<String, StreamBuf>>> = Arc::new(Mutex::new(HashMap::new()));
        let pending_r = pending.clone();
        let out_r = out.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if line.trim().is_empty() {
                    continue;
                }
                let Ok(v) = serde_json::from_str::<Value>(&line) else {
                    tracing::warn!(%line, "acp non-json stdout");
                    continue;
                };
                if let Some(err) = v.get("error") {
                    tracing::warn!(error = %err, "acp rpc error");
                }
                if let Some(id) = v.get("id").and_then(|i| i.as_u64())
                    && let Some(tx) = pending_r.lock().await.remove(&id)
                {
                    let _ = tx.send(v);
                    continue;
                }
                ingest_notify(&v, &out_r).await;
            }
            tracing::warn!("acp stdout closed");
        });
        let mut proc = AcpProc {
            stdin,
            next_id: 1,
            pending,
            out,
            _child: child,
        };
        let init = json!({
            "protocolVersion": 1,
            "clientInfo": { "name": "gladys-gateway", "version": "0.1.0" },
            "capabilities": {}
        });
        tracing::info!("acp initialize");
        let _ = rpc(&mut proc, "initialize", init).await?;
        Ok(proc)
    }

    async fn proc_for(&self, key: &ConvKey) -> Result<Arc<Mutex<AcpProc>>> {
        let ck = key.as_list_key();
        {
            let map = self.procs.lock().await;
            if let Some(p) = map.get(&ck) {
                return Ok(p.clone());
            }
        }
        let proc = self.spawn_one().await?;
        let mut map = self.procs.lock().await;
        if let Some(p) = map.get(&ck) {
            return Ok(p.clone());
        }
        let _ = self.store.drop_session(&ck);
        let arc = Arc::new(Mutex::new(proc));
        map.insert(ck, arc.clone());
        Ok(arc)
    }

    fn mcp_servers(&self) -> Value {
        let list: Vec<Value> = self
            .mcp
            .iter()
            .map(|m| {
                let token = std::env::var(&m.token_env).unwrap_or_default();
                json!({
                    "type": "http",
                    "name": m.name,
                    "url": m.url,
                    "headers": [{ "name": "Authorization", "value": format!("Bearer {token}") }]
                })
            })
            .collect();
        Value::Array(list)
    }

    async fn session_id_locked(&self, proc: &mut AcpProc, key: &ConvKey) -> Result<String> {
        let ck = key.as_list_key();
        if let Some(id) = self.store.get_session(&ck)? {
            return Ok(id);
        }
        tracing::info!(conv = %ck, "acp session/new");
        let resp = rpc(
            proc,
            "session/new",
            json!({
                "cwd": self.cwd.clone().unwrap_or_else(|| {
                    std::env::current_dir()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_else(|_| ".".into())
                }),
                "mcpServers": self.mcp_servers(),
            }),
        )
        .await?;
        let sid = resp
            .pointer("/result/sessionId")
            .and_then(Value::as_str)
            .ok_or_else(|| GatewayError::Agent("session/new missing sessionId".into()))?
            .to_string();
        self.store.put_session(&ck, &sid)?;
        Ok(sid)
    }
}

#[async_trait]
impl AgentBackend for AcpBackend {
    async fn prompt(&self, key: &ConvKey, input: AgentInput) -> Result<AgentOutput> {
        let mut body = String::new();
        body.push_str(&format!(
            "Account: {}\nChat: {} {}\n",
            input.account,
            input.key.kind.as_str(),
            input.key.peer
        ));
        if let Some(id) = &input.self_id {
            body.push_str(&format!("Self: {id}\n"));
        }
        if input.dream {
            body.push_str("Dreaming. Use memory tools to compact, dedupe, and reorganize. Do not channel_send.\n");
        }
        if input.idle {
            body.push_str("You are checking unread chat. Reply only if useful; otherwise empty.\n");
        }
        let pack = input.pack.render();
        if !pack.is_empty() {
            body.push_str("Memory:\n");
            body.push_str(&pack);
            body.push('\n');
        }
        let delta = input.pack_delta.render();
        if !delta.is_empty() {
            body.push_str("Memory updates:\n");
            body.push_str(&delta);
            body.push('\n');
        }
        if !input.messages.is_empty() {
            body.push_str("New messages:\n");
            for env in &input.messages {
                body.push_str(&env.prompt_line());
                body.push('\n');
            }
        }
        let slot = self.proc_for(key).await?;
        let mut proc = slot.lock().await;
        let sid = self.session_id_locked(&mut proc, key).await?;
        tracing::info!(session = %sid, conv = %key.as_list_key(), text = %body, "acp prompt");
        proc.out.lock().await.insert(sid.clone(), StreamBuf::default());
        let resp = rpc(
            &mut proc,
            "session/prompt",
            json!({
                "sessionId": sid,
                "prompt": [{ "type": "text", "text": body }]
            }),
        )
        .await?;
        let streamed = proc.out.lock().await.remove(&sid).unwrap_or_default();
        let text = resp
            .pointer("/result/text")
            .and_then(Value::as_str)
            .or_else(|| resp.pointer("/result/content/0/text").and_then(Value::as_str))
            .unwrap_or("")
            .to_string();
        let text = if text.trim().is_empty() {
            streamed.text
        } else {
            text
        };
        if !streamed.thought.trim().is_empty() {
            tracing::info!(session = %sid, text = %streamed.thought, "acp thought");
        }
        if !text.trim().is_empty() {
            tracing::info!(session = %sid, text = %text, "acp text");
        }
        Ok(AgentOutput { text })
    }

    async fn steer(&self, key: &ConvKey, text: &str) -> Result<()> {
        let slot = self.proc_for(key).await?;
        let mut proc = slot.lock().await;
        let sid = self.session_id_locked(&mut proc, key).await?;
        let body = format!(
            "Chat: {} {}\nSteering / new messages:\n{text}",
            key.kind.as_str(),
            key.peer
        );
        tracing::info!(session = %sid, conv = %key.as_list_key(), text = %body, "acp prompt");
        let _ = rpc(
            &mut proc,
            "session/prompt",
            json!({
                "sessionId": sid,
                "prompt": [{ "type": "text", "text": body }]
            }),
        )
        .await?;
        Ok(())
    }

    async fn reset(&self, key: &ConvKey) -> Result<()> {
        self.store.drop_session(&key.as_list_key())?;
        Ok(())
    }
}

async fn rpc(proc: &mut AcpProc, method: &str, params: Value) -> Result<Value> {
    let id = proc.next_id;
    proc.next_id += 1;
    let (tx, rx) = oneshot::channel();
    proc.pending.lock().await.insert(id, tx);
    let msg = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
    proc.stdin
        .write_all(format!("{msg}\n").as_bytes())
        .await
        .map_err(|e| GatewayError::Agent(e.to_string()))?;
    proc.stdin
        .flush()
        .await
        .map_err(|e| GatewayError::Agent(e.to_string()))?;
    let resp = tokio::time::timeout(std::time::Duration::from_secs(120), rx)
        .await
        .map_err(|_| GatewayError::Agent(format!("{method} timeout")))?
        .map_err(|_| GatewayError::Agent(format!("{method} closed")))?;
    if resp.get("error").is_some() {
        return Err(GatewayError::Agent(format!("{method}: {resp}")));
    }
    Ok(resp)
}


#[derive(Default)]
struct StreamBuf {
    text: String,
    thought: String,
}

async fn ingest_notify(v: &Value, out: &Mutex<HashMap<String, StreamBuf>>) {
    let method = v.get("method").and_then(Value::as_str).unwrap_or("");
    if method != "session/update" {
        return;
    }
    let params = v.get("params").cloned().unwrap_or(Value::Null);
    let sid = params
        .get("sessionId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let update = params.get("update").cloned().unwrap_or(Value::Null);
    let kind = update
        .get("sessionUpdate")
        .and_then(Value::as_str)
        .unwrap_or("update");
    match kind {
        "agent_message_chunk" => {
            let t = update
                .pointer("/content/text")
                .and_then(Value::as_str)
                .unwrap_or("");
            if !t.is_empty() {
                out.lock().await.entry(sid).or_default().text.push_str(t);
            }
        }
        "agent_thought_chunk" => {
            let t = update
                .pointer("/content/text")
                .and_then(Value::as_str)
                .unwrap_or("");
            if !t.is_empty() {
                out.lock().await.entry(sid).or_default().thought.push_str(t);
            }
        }
        "tool_call" => {
            let title = update.get("title").and_then(Value::as_str).unwrap_or("");
            let tool = update.get("kind").and_then(Value::as_str).unwrap_or("");
            let id = update.get("toolCallId").and_then(Value::as_str).unwrap_or("");
            tracing::info!(session = %sid, id, title, tool, "acp tool");
        }
        "tool_call_update" => {
            let status = update.get("status").and_then(Value::as_str).unwrap_or("");
            if matches!(status, "completed" | "failed" | "cancelled") {
                let id = update.get("toolCallId").and_then(Value::as_str).unwrap_or("");
                let title = update.get("title").and_then(Value::as_str).unwrap_or("");
                tracing::info!(session = %sid, id, title, status, "acp tool done");
            }
        }
        _ => {}
    }
}


