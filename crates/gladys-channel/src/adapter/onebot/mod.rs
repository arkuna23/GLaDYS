mod catalog;
mod event;
mod segment;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::http::header::{AUTHORIZATION, USER_AGENT};
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, tungstenite};

use self::catalog::{common_caps, has_native, native_message_types, native_ops};
use self::event::{envelope_from_get_msg, json_id, parse_frame};
use self::segment::parts_to_segments;
use super::{AccountMeta, Adapter, IngressTx};
use crate::capability::{Capabilities, Profile};
use crate::config::AccountConfig;
use crate::error::{ChannelError, Result};
use crate::message::{Conversation, ConversationKind, Envelope, Ingress, Part, SendAck};
use crate::store::Store;

pub use catalog::has_native as profile_has_native;
pub use event::recalled_platform_id;
pub use segment::{parts_to_segments as map_parts, segments_to_parts};

struct WsLink {
    tx: mpsc::Sender<String>,
    pending: Mutex<HashMap<String, oneshot::Sender<Value>>>,
}

pub struct OneBotAdapter {
    meta: AccountMeta,
    profile: Profile,
    mode: Mode,
    listen: Option<String>,
    ws_url: Option<String>,
    api_base_url: Option<String>,
    token: Option<String>,
    download_media: bool,
    blob_base: String,
    up: AtomicBool,
    store: Store,
    http: reqwest::Client,
    link: Mutex<Option<Arc<WsLink>>>,
    live_id: std::sync::Mutex<Option<String>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    ReverseWs,
    ForwardWs,
}

impl OneBotAdapter {
    pub fn new(cfg: &AccountConfig, store: Store, blob_base: impl Into<String>) -> Result<Self> {
        let profile = Profile::parse(cfg.profile.as_deref());
        let mode = match cfg.mode.as_deref() {
            Some("forward_ws") => Mode::ForwardWs,
            _ => Mode::ReverseWs,
        };
        if mode == Mode::ReverseWs && cfg.listen.is_none() {
            return Err(ChannelError::Invalid(format!(
                "account {} reverse_ws needs listen",
                cfg.id
            )));
        }
        if mode == Mode::ForwardWs && cfg.ws_url.is_none() {
            return Err(ChannelError::Invalid(format!(
                "account {} forward_ws needs ws_url",
                cfg.id
            )));
        }
        let token = cfg
            .access_token_env
            .as_deref()
            .and_then(|k| std::env::var(k).ok());
        Ok(Self {
            meta: AccountMeta {
                id: cfg.id.clone(),
                channel: "onebot".into(),
                profile: profile.as_str().into(),
                self_id: None,
            },
            profile,
            mode,
            listen: cfg.listen.clone(),
            ws_url: cfg.ws_url.clone(),
            api_base_url: cfg.api_base_url.clone(),
            token,
            download_media: cfg.download_media.unwrap_or(true),
            blob_base: blob_base.into(),
            up: AtomicBool::new(false),
            store,
            http: reqwest::Client::new(),
            link: Mutex::new(None),
            live_id: std::sync::Mutex::new(None),
        })
    }

    async fn set_link(&self, link: Option<Arc<WsLink>>) {
        *self.link.lock().await = link;
        self.up.store(self.link.lock().await.is_some() || self.api_base_url.is_some(), Ordering::Relaxed);
    }

    fn current_self_id(&self) -> Option<String> {
        self.live_id.lock().ok().and_then(|g| g.clone())
    }

    fn adopt_self_id(&self, id: String) -> bool {
        let Ok(mut g) = self.live_id.lock() else {
            return false;
        };
        if g.as_deref() == Some(id.as_str()) {
            return false;
        }
        *g = Some(id);
        true
    }

    async fn publish_self_id(&self, ingress: &IngressTx, id: String) {
        if !self.adopt_self_id(id.clone()) {
            return;
        }
        tracing::info!(account = %self.meta.id, self_id = %id, "onebot login");
        let _ = ingress
            .send(Ingress::Connection {
                account: self.meta.id.clone(),
                up: true,
                detail: "login".into(),
                self_id: Some(id),
            })
            .await;
    }

    async fn refresh_self_id(&self, ingress: &IngressTx) {
        match self.action("get_login_info", json!({})).await {
            Ok(data) => {
                if let Some(id) = data.get("user_id").and_then(json_id) {
                    self.publish_self_id(ingress, id).await;
                }
            }
            Err(e) => tracing::warn!(account = %self.meta.id, "get_login_info: {e}"),
        }
    }

    async fn action(&self, name: &str, params: Value) -> Result<Value> {
        if let Some(base) = &self.api_base_url {
            return self.http_action(base, name, params).await;
        }
        let link = self
            .link
            .lock()
            .await
            .clone()
            .ok_or_else(|| ChannelError::PlatformUnavailable {
                account: self.meta.id.clone(),
            })?;
        let echo = Envelope::new_id();
        let (tx, rx) = oneshot::channel();
        link.pending.lock().await.insert(echo.clone(), tx);
        let payload = json!({
            "action": name,
            "params": params,
            "echo": echo,
        });
        link.tx
            .send(payload.to_string())
            .await
            .map_err(|_| ChannelError::PlatformUnavailable {
                account: self.meta.id.clone(),
            })?;
        let resp = tokio::time::timeout(Duration::from_secs(20), rx)
            .await
            .map_err(|_| ChannelError::Platform {
                action: name.into(),
                message: "timeout".into(),
            })?
            .map_err(|_| ChannelError::Platform {
                action: name.into(),
                message: "connection closed".into(),
            })?;
        parse_action_result(name, resp)
    }

    async fn http_action(&self, base: &str, name: &str, params: Value) -> Result<Value> {
        let url = format!("{}/{}", base.trim_end_matches('/'), name);
        let mut req = self.http.post(url).json(&params);
        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }
        let resp = req.send().await?;
        let value: Value = resp.json().await?;
        parse_action_result(name, value)
    }

    async fn handle_incoming(&self, ingress: &IngressTx, text: &str) {
        let Ok(value) = serde_json::from_str::<Value>(text) else {
            tracing::warn!(account = %self.meta.id, "dropping non-json onebot frame");
            return;
        };
        if let Some(id) = value.get("self_id").and_then(json_id) {
            self.publish_self_id(ingress, id).await;
        }
        if let (Some(echo), Some(_)) = (
            value.get("echo").and_then(Value::as_str).map(ToOwned::to_owned),
            value.get("retcode"),
        ) && let Some(link) = self.link.lock().await.clone()
            && let Some(tx) = link.pending.lock().await.remove(&echo)
        {
            let _ = tx.send(value);
            return;
        }
        let Some(mut parsed) = parse_frame(&self.meta.id, &value) else {
            return;
        };
        if let Ingress::Message(env) = &mut parsed {
            if self.download_media {
                self.fill_blobs(&mut env.parts).await;
            }
            self.fill_reply_sender(env).await;
        }
        let _ = ingress.send(parsed).await;
    }

    fn parts_with_blobs(&self, parts: &[Part]) -> Vec<Part> {
        parts.iter().cloned().map(|p| self.embed_blob(p)).collect()
    }

    fn embed_blob(&self, part: Part) -> Part {
        let id = match &part {
            Part::Image { blob_id: Some(id), .. }
            | Part::Audio { blob_id: Some(id), .. }
            | Part::Video { blob_id: Some(id), .. }
            | Part::File { blob_id: Some(id), .. } => id.clone(),
            _ => return part,
        };
        let Some(file) = self.blob_as_base64(&id) else {
            return part;
        };
        match part {
            Part::Image { mime, filename, blob_id, .. } => Part::Image {
                url: Some(file),
                blob_id,
                mime,
                filename,
            },
            Part::Audio { mime, filename, blob_id, .. } => Part::Audio {
                url: Some(file),
                blob_id,
                mime,
                filename,
            },
            Part::Video { mime, filename, blob_id, .. } => Part::Video {
                url: Some(file),
                blob_id,
                mime,
                filename,
            },
            Part::File { mime, filename, blob_id, .. } => Part::File {
                url: Some(file),
                blob_id,
                mime,
                filename,
            },
            other => other,
        }
    }

    fn blob_as_base64(&self, id: &str) -> Option<String> {
        use base64::Engine;
        let (path, _) = self.store.get_blob(id).ok().flatten()?;
        let bytes = std::fs::read(path).ok()?;
        Some(format!(
            "base64://{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        ))
    }

    async fn fill_reply_sender(&self, env: &mut Envelope) {
        let Some(pid) = env
            .reply_to
            .as_ref()
            .and_then(|r| r.platform_id.clone())
        else {
            return;
        };
        let sender = if let Ok(Some(orig)) =
            self.store.get_by_platform("onebot", &self.meta.id, &pid)
        {
            Some(orig.sender.id)
        } else {
            match self.fetch_message(&pid).await {
                Ok(Some(orig)) => Some(orig.sender.id),
                _ => None,
            }
        };
        if let Some(id) = sender
            && let Some(rt) = env.reply_to.as_mut()
        {
            rt.sender = Some(id);
        }
    }

    async fn fill_blobs(&self, parts: &mut [Part]) {
        for part in parts {
            let kind = match part {
                Part::Image { blob_id, .. } if blob_id.is_none() => "image",
                Part::Audio { blob_id, .. } if blob_id.is_none() => "record",
                _ => continue,
            };
            let url = match part {
                Part::Image { url, .. } | Part::Audio { url, .. } => url.clone(),
                _ => None,
            };
            let filename = match part {
                Part::Image { filename, .. } | Part::Audio { filename, .. } => filename.clone(),
                _ => None,
            };
            let Some((bytes, mime)) = self
                .fetch_media_bytes(kind, url.as_deref(), filename.as_deref())
                .await
            else {
                continue;
            };
            match self
                .store
                .save_blob(&bytes, mime.as_deref(), filename.as_deref())
            {
                Ok(id) => {
                    let hosted = crate::config::hosted_blob_url(&self.blob_base, &id);
                    match part {
                        Part::Image {
                            blob_id,
                            url,
                            mime: m,
                            ..
                        }
                        | Part::Audio {
                            blob_id,
                            url,
                            mime: m,
                            ..
                        } => {
                            *blob_id = Some(id);
                            *url = Some(hosted);
                            if mime.is_some() {
                                *m = mime;
                            }
                        }
                        _ => {}
                    }
                }
                Err(e) => tracing::warn!("blob save: {e}"),
            }
        }
    }

    async fn fetch_media_bytes(
        &self,
        kind: &str,
        url: Option<&str>,
        filename: Option<&str>,
    ) -> Option<(Vec<u8>, Option<String>)> {
        if let Some(url) = url
            && (url.starts_with("http://") || url.starts_with("https://"))
        {
            return self.http_get_bytes(url).await;
        }
        let file = filename.filter(|s| !s.is_empty()).or(url);
        let file = file?;
        let (action, params) = if kind == "record" {
            ("get_record", json!({"file": file, "out_format": "mp3"}))
        } else {
            ("get_image", json!({"file": file}))
        };
        match self.action(action, params).await {
            Ok(data) => {
                for key in ["url", "file"] {
                    if let Some(v) = data.get(key).and_then(Value::as_str) {
                        if v.starts_with("http://") || v.starts_with("https://") {
                            return self.http_get_bytes(v).await;
                        }
                        if let Ok(bytes) = tokio::fs::read(v).await {
                            return Some((bytes, None));
                        }
                    }
                }
                None
            }
            Err(e) => {
                tracing::warn!("{action}: {e}");
                None
            }
        }
    }

    async fn http_get_bytes(&self, url: &str) -> Option<(Vec<u8>, Option<String>)> {
        match self.http.get(url).send().await {
            Ok(resp) => {
                let mime = resp
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.split(';').next().unwrap_or(s).trim().to_string());
                match resp.bytes().await {
                    Ok(bytes) => Some((bytes.to_vec(), mime)),
                    Err(e) => {
                        tracing::warn!("media body: {e}");
                        None
                    }
                }
            }
            Err(e) => {
                tracing::warn!("media fetch: {e}");
                None
            }
        }
    }

    async fn pump_ws<S>(
        self: &Arc<Self>,
        ingress: IngressTx,
        ws: tokio_tungstenite::WebSocketStream<S>,
        up: &str,
        down: &str,
    ) where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let (mut sink, mut stream) = ws.split();
        let (tx, mut rx) = mpsc::channel::<String>(32);
        let link = Arc::new(WsLink {
            tx,
            pending: Mutex::new(HashMap::new()),
        });
        self.set_link(Some(link)).await;
        let _ = ingress
            .send(Ingress::Connection {
                account: self.meta.id.clone(),
                up: true,
                detail: up.into(),
                self_id: self.current_self_id(),
            })
            .await;
        let login = self.clone();
        let login_ingress = ingress.clone();
        tokio::spawn(async move {
            login.refresh_self_id(&login_ingress).await;
        });
        let writer = tokio::spawn(async move {
            while let Some(frame) = rx.recv().await {
                if sink.send(Message::Text(frame.into())).await.is_err() {
                    break;
                }
            }
        });
        while let Some(Ok(msg)) = stream.next().await {
            match msg {
                Message::Text(t) => self.handle_incoming(&ingress, t.as_str()).await,
                Message::Close(_) => break,
                _ => {}
            }
        }
        writer.abort();
        self.set_link(None).await;
        let _ = ingress
            .send(Ingress::Connection {
                account: self.meta.id.clone(),
                up: false,
                detail: down.into(),
                self_id: self.current_self_id(),
            })
            .await;
    }
}

fn parse_action_result(action: &str, resp: Value) -> Result<Value> {
    let status = resp.get("status").and_then(Value::as_str).unwrap_or("failed");
    let retcode = resp.get("retcode").and_then(Value::as_i64).unwrap_or(-1);
    if status != "ok" && retcode != 0 {
        let message = resp
            .get("message")
            .or_else(|| resp.get("wording"))
            .and_then(Value::as_str)
            .unwrap_or("failed")
            .to_string();
        return Err(ChannelError::Platform {
            action: action.into(),
            message,
        });
    }
    Ok(resp.get("data").cloned().unwrap_or(Value::Null))
}

fn token_from_req(req: &Request, expected: Option<&str>) -> bool {
    let Some(expected) = expected else {
        return true;
    };
    if let Some(h) = req.headers().get("Authorization").and_then(|v| v.to_str().ok())
        && (h.strip_prefix("Bearer ") == Some(expected) || h == expected)
    {
        return true;
    }
    if let Some(query) = req.uri().query() {
        for pair in query.split('&') {
            if let Some(v) = pair.strip_prefix("access_token=")
                && v == expected
            {
                return true;
            }
        }
    }
    false
}

fn forward_ws_request(
    url: &str,
    token: Option<&str>,
) -> Result<tungstenite::http::Request<()>> {
    let mut req = url
        .into_client_request()
        .map_err(|e| ChannelError::Invalid(format!("ws_url: {e}")))?;
    if let Some(token) = token.filter(|t| !t.is_empty()) {
        let value = format!("Bearer {token}");
        let header = HeaderValue::from_str(&value)
            .map_err(|_| ChannelError::Invalid("access token is not a valid HTTP header".into()))?;
        req.headers_mut().insert(AUTHORIZATION, header);
    }
    req.headers_mut()
        .insert(USER_AGENT, HeaderValue::from_static("OneBot/11"));
    Ok(req)
}

#[async_trait]
impl Adapter for OneBotAdapter {
    fn account(&self) -> AccountMeta {
        let mut meta = self.meta.clone();
        meta.self_id = self.current_self_id();
        meta
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            channel: "onebot".into(),
            account: self.meta.id.clone(),
            profile: self.profile.as_str().into(),
            common: common_caps(self.profile),
            native_ops: native_ops(self.profile),
            native_message_types: native_message_types(self.profile),
        }
    }

    fn connected(&self) -> bool {
        self.up.load(Ordering::Relaxed)
    }

    async fn send(&self, dest: &Conversation, parts: &[Part]) -> Result<SendAck> {
        let parts = self.parts_with_blobs(parts);
        let (message, dropped) = parts_to_segments(&parts);
        if message.is_empty() {
            return Err(ChannelError::EmptySend);
        }
        let mut params = json!({ "message": message });
        match dest.kind {
            ConversationKind::Dm => {
                params["user_id"] = json!(dest.peer);
                params["message_type"] = json!("private");
            }
            ConversationKind::Group => {
                params["group_id"] = json!(dest.peer);
                params["message_type"] = json!("group");
            }
        }
        let data = self.action("send_msg", params).await?;
        Ok(SendAck {
            id: Envelope::new_id(),
            platform_id: data.get("message_id").and_then(json_id),
            dropped,
        })
    }

    async fn recall(&self, platform_id: &str) -> Result<()> {
        self.action("delete_msg", json!({"message_id": platform_id}))
            .await?;
        Ok(())
    }

    async fn react(&self, platform_id: &str, emoji: &str) -> Result<()> {
        if !self.capabilities().common.react {
            return Err(ChannelError::unsupported("react", "onebot"));
        }
        self.action(
            "set_msg_emoji_like",
            json!({"message_id": platform_id, "emoji_id": emoji}),
        )
        .await?;
        Ok(())
    }

    async fn call_native(&self, op: &str, params: Value) -> Result<Value> {
        if !has_native(self.profile, op) {
            return Err(ChannelError::unsupported(op, "onebot"));
        }
        self.action(op, params).await
    }

    async fn fetch_message(&self, platform_id: &str) -> Result<Option<Envelope>> {
        match self
            .action("get_msg", json!({"message_id": platform_id}))
            .await
        {
            Ok(data) => Ok(envelope_from_get_msg(&self.meta.id, &data)),
            Err(ChannelError::PlatformUnavailable { .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn run(self: Arc<Self>, ingress: IngressTx) -> Result<()> {
        match self.mode {
            Mode::ReverseWs => {
                let listen = self.listen.clone().expect("checked in new");
                let listener = TcpListener::bind(&listen).await?;
                tracing::info!(account = %self.meta.id, listen, "onebot reverse ws listening");
                loop {
                    let (stream, _) = listener.accept().await?;
                    let token = self.token.clone();
                    let this = self.clone();
                    let ingress = ingress.clone();
                    tokio::spawn(async move {
                        let expected = token.clone();
                        #[allow(clippy::result_large_err)]
                        let callback = move |req: &Request, resp: Response| {
                            if token_from_req(req, expected.as_deref()) {
                                Ok(resp)
                            } else {
                                Err(tungstenite::http::Response::builder()
                                    .status(401)
                                    .body(None)
                                    .expect("static 401"))
                            }
                        };
                        match tokio_tungstenite::accept_hdr_async(stream, callback).await {
                            Ok(ws) => {
                                this.pump_ws(ingress, ws, "onebot reverse ws up", "onebot reverse ws down")
                                    .await;
                            }
                            Err(e) => tracing::warn!("onebot handshake: {e}"),
                        }
                    });
                }
            }
            Mode::ForwardWs => {
                let url = self.ws_url.clone().expect("checked in new");
                loop {
                    match forward_ws_request(&url, self.token.as_deref()) {
                        Ok(req) => match connect_async(req).await {
                            Ok((ws, _)) => {
                                self.pump_ws(
                                    ingress.clone(),
                                    ws,
                                    "onebot forward ws up",
                                    "onebot forward ws down",
                                )
                                .await;
                            }
                            Err(e) => tracing::warn!(
                                account = %self.meta.id,
                                url,
                                "onebot connect: {e}; ws_url must be NapCat websocketServers (not httpServers/SSE)"
                            ),
                        },
                        Err(e) => tracing::warn!(account = %self.meta.id, "onebot ws_url: {e}"),
                    }
                    tokio::time::sleep(Duration::from_secs(3)).await;
                }
            }
        }
    }
}


#[cfg(test)]
mod forward_req_tests {
    use super::forward_ws_request;

    #[test]
    fn bearer_header_not_query() {
        let req = forward_ws_request("ws://127.0.0.1:3001/", Some("secret")).unwrap();
        assert_eq!(
            req.headers().get("authorization").unwrap().to_str().unwrap(),
            "Bearer secret"
        );
        assert!(req.uri().query().is_none());
    }
    
    #[test]
    fn empty_token_skips_auth() {
        let req = forward_ws_request("ws://127.0.0.1:3001/", Some("")).unwrap();
        assert!(req.headers().get("authorization").is_none());
    }
}
