use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::ChannelError;
use crate::http::AppState;
use crate::service::{
    BusEvent, CallParams, CapabilitiesParams, ConversationsParams, ReactParams, RecallParams,
};

#[derive(Deserialize)]
pub struct GatewayQuery {
    #[serde(default)]
    token: Option<String>,
}

pub async fn upgrade(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<GatewayQuery>,
) -> impl IntoResponse {
    let header_token = bearer(&headers);
    ws.on_upgrade(move |socket| handle(socket, state, header_token, query.token))
}

fn bearer(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer ").map(ToOwned::to_owned))
}

async fn handle(
    mut socket: WebSocket,
    state: AppState,
    header_token: Option<String>,
    query_token: Option<String>,
) {
    let Some(Ok(Message::Text(first))) = socket.recv().await else {
        let _ = socket.send(Message::Close(None)).await;
        return;
    };
    let Ok(frame) = serde_json::from_str::<Value>(&first) else {
        let _ = socket.send(Message::Close(None)).await;
        return;
    };
    if frame.get("type").and_then(Value::as_str) != Some("hello") {
        let _ = socket.send(Message::Close(None)).await;
        return;
    }
    let payload = frame.get("payload").cloned().unwrap_or(Value::Null);
    let body_token = payload
        .get("token")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let token = body_token.or(header_token).or(query_token);
    if token.as_deref() != Some(state.token.as_str()) {
        let _ = socket
            .send(Message::Text(
                json!({"v":1,"type":"res","ok":false,"error":{"code":"unauthorized","message":"unauthorized"}}).to_string().into(),
            ))
            .await;
        let _ = socket.send(Message::Close(None)).await;
        return;
    }
    let hello_ok = json!({
        "v": 1,
        "type": "hello_ok",
        "payload": {
            "server": "gladys-channel",
            "accounts": state.service.list_accounts(),
        }
    });
    if socket
        .send(Message::Text(hello_ok.to_string().into()))
        .await
        .is_err()
    {
        return;
    }
    let health = event_json(0, &state.service.health_event());
    let _ = socket.send(Message::Text(health.to_string().into())).await;

    let mut rx = state.service.subscribe();
    let seq = Arc::new(AtomicU64::new(1));
    loop {
        tokio::select! {
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        if let Some(res) = dispatch(&state, &text).await
                            && socket.send(Message::Text(res.to_string().into())).await.is_err()
                        {
                            break;
                        }
                    }
                    Some(Ok(Message::Ping(p))) => {
                        let _ = socket.send(Message::Pong(p)).await;
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(_)) => break,
                    _ => {}
                }
            }
            ev = rx.recv() => {
                match ev {
                    Ok(ev) => {
                        let n = seq.fetch_add(1, Ordering::Relaxed);
                        let frame = event_json(n, &ev);
                        if socket.send(Message::Text(frame.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
        }
    }
}

fn event_json(seq: u64, ev: &BusEvent) -> Value {
    let (name, payload) = match ev {
        BusEvent::MessageInbound(e) => ("message.inbound", json!(e)),
        BusEvent::MessageOutbound(e) => ("message.outbound", json!(e)),
        BusEvent::Notice(e) => ("notice", json!(e)),
        BusEvent::Request(e) => ("request", json!(e)),
        BusEvent::Connection {
            account,
            up,
            detail,
            self_id,
        } => (
            "connection",
            json!({"account": account, "up": up, "detail": detail, "self_id": self_id}),
        ),
        BusEvent::Health { ok, accounts } => ("health", json!({"ok": ok, "accounts": accounts})),
    };
    json!({"v": 1, "type": "event", "seq": seq, "event": name, "payload": payload})
}

async fn dispatch(state: &AppState, text: &str) -> Option<Value> {
    let frame: Value = serde_json::from_str(text).ok()?;
    if frame.get("type").and_then(Value::as_str) != Some("req") {
        return None;
    }
    let id = frame.get("id").cloned().unwrap_or(Value::Null);
    let method = frame.get("method").and_then(Value::as_str).unwrap_or("");
    let params = frame.get("params").cloned().unwrap_or(json!({}));
    let result = dispatch_method(&state.service, method, params).await;
    Some(match result {
        Ok(payload) => json!({"v":1,"type":"res","id":id,"ok":true,"payload":payload}),
        Err(err) => json!({
            "v":1,
            "type":"res",
            "id":id,
            "ok":false,
            "error": {"code": err.code(), "message": err.to_string()}
        }),
    })
}

async fn dispatch_method(
    service: &crate::service::Service,
    method: &str,
    params: Value,
) -> Result<Value, ChannelError> {
    match method {
        "send" => Ok(json!(service.send(serde_json::from_value(params)?).await?)),
        "get" => Ok(json!(service.get(parse(params)?).await?)),
        "history" => Ok(json!(service.history(parse(params)?)?)),
        "search" => Ok(json!(service.search(parse(params)?)?)),
        "recall" => Ok(json!(service.recall(parse::<RecallParams>(params)?).await?)),
        "react" => {
            service.react(parse::<ReactParams>(params)?).await?;
            Ok(json!({"ok": true}))
        }
        "conversations" => Ok(json!(service.conversations(parse::<ConversationsParams>(params)?)?)),
        "capabilities" => Ok(json!(service.capabilities(parse::<CapabilitiesParams>(params)?)?)),
        "call" => Ok(json!(service.call(parse::<CallParams>(params)?).await?)),
        "list" => Ok(json!(service.list_accounts())),
        other => Err(ChannelError::Invalid(format!("unknown method {other}"))),
    }
}

fn parse<T: serde::de::DeserializeOwned>(params: Value) -> Result<T, ChannelError> {
    serde_json::from_value(params).map_err(|e| ChannelError::Invalid(e.to_string()))
}
