use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_tungstenite::tungstenite::Message;

use crate::dispatch::Dispatch;
use crate::error::{GatewayError, Result};
use crate::io::{envelope_from_event, ChannelIo};
use crate::types::{Conversation, Part};

pub struct ChannelWs {
    tx: mpsc::Sender<String>,
    pending: Mutex<std::collections::HashMap<u64, oneshot::Sender<Value>>>,
    next_id: AtomicU64,
}

impl ChannelWs {
    pub async fn connect(url: &str, token: &str, dispatch: Dispatch) -> Result<Arc<Self>> {
        let (ws, _) = tokio_tungstenite::connect_async(url)
            .await
            .map_err(|e| GatewayError::Channel(e.to_string()))?;
        let (mut sink, mut stream) = ws.split();
        let (tx, mut rx) = mpsc::channel::<String>(32);
        let this = Arc::new(Self {
            tx,
            pending: Mutex::new(std::collections::HashMap::new()),
            next_id: AtomicU64::new(1),
        });
        let hello = json!({
            "v": 1,
            "type": "hello",
            "payload": { "role": "gateway", "token": token }
        });
        sink.send(Message::Text(hello.to_string().into()))
            .await
            .map_err(|e| GatewayError::Channel(e.to_string()))?;
        tokio::spawn(async move {
            while let Some(frame) = rx.recv().await {
                if sink.send(Message::Text(frame.into())).await.is_err() {
                    break;
                }
            }
        });
        let dispatch_c = dispatch.clone();
        let this_r = this.clone();
        tokio::spawn(async move {
            while let Some(Ok(msg)) = stream.next().await {
                let Message::Text(t) = msg else { continue };
                let Ok(v) = serde_json::from_str::<Value>(t.as_str()) else {
                    continue;
                };
                match v.get("type").and_then(Value::as_str) {
                    Some("hello_ok") => {
                        if let Some(accounts) =
                            v.pointer("/payload/accounts").and_then(|a| a.as_array())
                        {
                            for acc in accounts {
                                let id = acc.get("id").and_then(Value::as_str);
                                let self_id = acc.get("self_id").and_then(Value::as_str);
                                if let (Some(id), Some(sid)) = (id, self_id) {
                                    dispatch_c.set_self_id(id.into(), sid.into()).await;
                                }
                            }
                        }
                    }
                    Some("event") => {
                        let name = v.get("event").and_then(Value::as_str).unwrap_or("");
                        let payload = v.get("payload").cloned().unwrap_or(Value::Null);
                        if name == "message.inbound" {
                            if let Some(env) = envelope_from_event(&payload)
                                && let Err(e) = dispatch_c.handle(env).await
                            {
                                tracing::warn!("inbound: {e}");
                            }
                        } else {
                            if name == "connection" {
                                let account = payload.get("account").and_then(Value::as_str);
                                let self_id = payload.get("self_id").and_then(Value::as_str);
                                if let (Some(acc), Some(sid)) = (account, self_id) {
                                    dispatch_c.set_self_id(acc.into(), sid.into()).await;
                                }
                            }
                            dispatch_c.handle_bus_event(name, payload).await;
                        }
                    }
                    Some("res") => {
                        if let Some(id) = v.get("id").and_then(Value::as_u64)
                            && let Some(tx) = this_r.pending.lock().await.remove(&id)
                        {
                            let _ = tx.send(v);
                        }
                    }
                    _ => {}
                }
            }
        });
        Ok(this)
    }
}

#[async_trait]
impl ChannelIo for ChannelWs {
    async fn send(
        &self,
        account: &str,
        conversation: &Conversation,
        parts: Vec<Part>,
    ) -> Result<()> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        let frame = json!({
            "v": 1,
            "type": "req",
            "id": id,
            "method": "send",
            "params": {
                "account": account,
                "conversation": conversation,
                "parts": parts,
            }
        });
        self.tx
            .send(frame.to_string())
            .await
            .map_err(|_| GatewayError::Channel("ws closed".into()))?;
        let resp = tokio::time::timeout(std::time::Duration::from_secs(20), rx)
            .await
            .map_err(|_| GatewayError::Channel("send timeout".into()))?
            .map_err(|_| GatewayError::Channel("send closed".into()))?;
        if resp.get("ok") == Some(&json!(false)) {
            return Err(GatewayError::Channel(resp.to_string()));
        }
        Ok(())
    }
}
