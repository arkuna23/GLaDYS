use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::Value;
use tokio::sync::{broadcast, Mutex};

use crate::adapter::onebot::recalled_platform_id;
use crate::adapter::{build_adapter, Adapter, IngressTx};
use crate::capability::{AccountStatus, Capabilities};
use crate::config::Config;
use crate::error::{ChannelError, Result};
use crate::message::{
    Actor, Conversation, ConversationInfo, Direction, Envelope, Ingress, Part, PlatformEvent,
    SendAck,
};
use crate::store::Store;

const IDEM_TTL: Duration = Duration::from_secs(300);

#[derive(Debug, Clone)]
pub enum BusEvent {
    MessageInbound(Envelope),
    MessageOutbound(Envelope),
    Notice(PlatformEvent),
    Request(PlatformEvent),
    Connection {
        account: String,
        up: bool,
        detail: String,
        self_id: Option<String>,
    },
    Health {
        ok: bool,
        accounts: Vec<AccountStatus>,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct SendParams {
    pub account: String,
    pub conversation: Conversation,
    pub parts: Vec<Part>,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GetParams {
    pub account: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub platform_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HistoryParams {
    pub account: String,
    pub conversation: Conversation,
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub before_ts: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchParams {
    pub query: String,
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

fn default_limit() -> u32 {
    50
}

#[derive(Debug, Clone, Deserialize)]
pub struct RecallParams {
    pub account: String,
    pub platform_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReactParams {
    pub account: String,
    pub platform_id: String,
    pub emoji: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CallParams {
    pub account: String,
    pub op: String,
    #[serde(default)]
    pub params: Value,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CapabilitiesParams {
    #[serde(default)]
    pub account: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConversationsParams {
    #[serde(default)]
    pub account: Option<String>,
}

#[derive(Clone)]
pub struct Service {
    store: Store,
    adapters: Arc<HashMap<String, Arc<dyn Adapter>>>,
    bus: broadcast::Sender<BusEvent>,
    idem: Arc<Mutex<HashMap<String, (Instant, IdemValue)>>>,
    blob_base: String,
    tickets: Arc<Mutex<HashMap<String, Instant>>>,
}

#[derive(Clone)]
enum IdemValue {
    Send(SendAck),
    Call(Value),
}

impl Service {
    pub fn new(store: Store, adapters: HashMap<String, Arc<dyn Adapter>>) -> Self {
        Self::with_blob_base(store, adapters, "http://127.0.0.1:3920".into())
    }

    pub fn with_blob_base(
        store: Store,
        adapters: HashMap<String, Arc<dyn Adapter>>,
        blob_base: String,
    ) -> Self {
        let (bus, _) = broadcast::channel(256);
        Self {
            store,
            adapters: Arc::new(adapters),
            bus,
            idem: Arc::new(Mutex::new(HashMap::new())),
            blob_base,
            tickets: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn from_config(cfg: &Config) -> Result<Self> {
        std::fs::create_dir_all(&cfg.data_dir)?;
        let store = Store::open(cfg.sqlite_path(), cfg.blob_dir())?;
        let mut adapters = HashMap::new();
        for account in &cfg.accounts {
            let adapter = build_adapter(account, store.clone(), &cfg.blob_base())?;
            adapters.insert(account.id.clone(), adapter);
        }
        Ok(Self::with_blob_base(store, adapters, cfg.blob_base()))
    }

    pub fn subscribe(&self) -> broadcast::Receiver<BusEvent> {
        self.bus.subscribe()
    }

    pub fn spawn_adapters(self: &Arc<Self>) -> IngressTx {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Ingress>(256);
        let svc = self.clone();
        tokio::spawn(async move {
            while let Some(item) = rx.recv().await {
                if let Err(e) = svc.ingest(item).await {
                    tracing::warn!("ingest: {e}");
                }
            }
        });
        for adapter in self.adapters.values() {
            let adapter = adapter.clone();
            let ingress = tx.clone();
            tokio::spawn(async move {
                if let Err(e) = adapter.run(ingress).await {
                    tracing::error!("adapter stopped: {e}");
                }
            });
        }
        tx
    }

    pub fn list_accounts(&self) -> Vec<AccountStatus> {
        self.adapters
            .values()
            .map(|a| {
                let meta = a.account();
                AccountStatus {
                    id: meta.id,
                    channel: meta.channel,
                    profile: meta.profile,
                    up: a.connected(),
                    self_id: meta.self_id,
                }
            })
            .collect()
    }

    pub fn health_event(&self) -> BusEvent {
        let accounts = self.list_accounts();
        BusEvent::Health {
            ok: true,
            accounts,
        }
    }

    pub async fn ingest(&self, item: Ingress) -> Result<()> {
        match item {
            Ingress::Message(env) => {
                let inserted = self.store.insert_message(&env)?;
                if inserted {
                    let ev = if env.direction == Direction::Out {
                        BusEvent::MessageOutbound(env)
                    } else {
                        BusEvent::MessageInbound(env)
                    };
                    let _ = self.bus.send(ev);
                }
            }
            Ingress::Notice(ev) => {
                if let Some(pid) = recalled_platform_id(&ev) {
                    let _ = self.store.mark_recalled(&ev.channel, &ev.account, &pid)?;
                }
                self.store.insert_event(&ev)?;
                let _ = self.bus.send(BusEvent::Notice(ev));
            }
            Ingress::Request(ev) => {
                self.store.insert_event(&ev)?;
                let _ = self.bus.send(BusEvent::Request(ev));
            }
            Ingress::Connection {
                account,
                up,
                detail,
                self_id,
            } => {
                let _ = self.bus.send(BusEvent::Connection {
                    account,
                    up,
                    detail,
                    self_id,
                });
            }
        }
        Ok(())
    }

    fn adapter(&self, account: &str) -> Result<Arc<dyn Adapter>> {
        self.adapters
            .get(account)
            .cloned()
            .ok_or_else(|| ChannelError::UnknownAccount(account.into()))
    }

    async fn idem_get(&self, key: &str) -> Option<IdemValue> {
        let mut map = self.idem.lock().await;
        map.retain(|_, (t, _)| t.elapsed() < IDEM_TTL);
        map.get(key).map(|(_, v)| v.clone())
    }

    async fn idem_put(&self, key: String, value: IdemValue) {
        self.idem.lock().await.insert(key, (Instant::now(), value));
    }

    pub async fn send(&self, params: SendParams) -> Result<SendAck> {
        if let Some(key) = &params.idempotency_key
            && let Some(IdemValue::Send(ack)) = self.idem_get(key).await
        {
            return Ok(ack);
        }
        let adapter = self.adapter(&params.account)?;
        if !adapter.capabilities().common.send {
            return Err(ChannelError::unsupported("send", adapter.account().channel));
        }
        let ack = adapter.send(&params.conversation, &params.parts).await?;
        let meta = adapter.account();
        let env = Envelope {
            id: ack.id.clone(),
            channel: meta.channel,
            account: params.account.clone(),
            conversation: params.conversation,
            direction: Direction::Out,
            sender: Actor {
                id: meta.self_id.unwrap_or_else(|| "self".into()),
                name: None,
            },
            ts: now_ts(),
            platform_id: ack.platform_id.clone(),
            reply_to: None,
            parts: params.parts,
            native: None,
            recalled: false,
        };
        self.store.insert_message(&env)?;
        let _ = self.bus.send(BusEvent::MessageOutbound(env));
        if let Some(key) = params.idempotency_key {
            self.idem_put(key, IdemValue::Send(ack.clone())).await;
        }
        Ok(ack)
    }

    pub async fn get(&self, params: GetParams) -> Result<Envelope> {
        let adapter = self.adapter(&params.account)?;
        let meta = adapter.account();
        if let Some(id) = &params.id
            && let Some(env) = self.store.get_by_id(id)?
        {
            return Ok(env);
        }
        if let Some(pid) = &params.platform_id {
            if let Some(env) = self.store.get_by_platform(&meta.channel, &params.account, pid)? {
                return Ok(env);
            }
            if let Some(env) = adapter.fetch_message(pid).await? {
                let _ = self.store.insert_message(&env)?;
                return Ok(env);
            }
        }
        Err(ChannelError::NotFound)
    }

    pub fn history(&self, params: HistoryParams) -> Result<Vec<Envelope>> {
        let adapter = self.adapter(&params.account)?;
        let meta = adapter.account();
        self.store.history(
            &meta.channel,
            &params.account,
            &params.conversation,
            params.limit,
            params.before_ts,
        )
    }

    pub fn search(&self, params: SearchParams) -> Result<Vec<Envelope>> {
        self.store
            .search(&params.query, params.account.as_deref(), params.limit)
    }

    pub async fn recall(&self, params: RecallParams) -> Result<Option<Envelope>> {
        let adapter = self.adapter(&params.account)?;
        if !adapter.capabilities().common.recall {
            return Err(ChannelError::unsupported(
                "recall",
                adapter.account().channel,
            ));
        }
        adapter.recall(&params.platform_id).await?;
        let meta = adapter.account();
        self.store
            .mark_recalled(&meta.channel, &params.account, &params.platform_id)
    }

    pub async fn react(&self, params: ReactParams) -> Result<()> {
        let adapter = self.adapter(&params.account)?;
        if !adapter.capabilities().common.react {
            return Err(ChannelError::unsupported("react", adapter.account().channel));
        }
        adapter.react(&params.platform_id, &params.emoji).await
    }

    pub fn conversations(&self, params: ConversationsParams) -> Result<Vec<ConversationInfo>> {
        self.store.conversations(params.account.as_deref())
    }

    pub fn capabilities(&self, params: CapabilitiesParams) -> Result<Vec<Capabilities>> {
        if let Some(account) = params.account {
            Ok(vec![self.adapter(&account)?.capabilities()])
        } else {
            Ok(self.adapters.values().map(|a| a.capabilities()).collect())
        }
    }

    pub async fn call(&self, params: CallParams) -> Result<Value> {
        if let Some(key) = &params.idempotency_key
            && let Some(IdemValue::Call(v)) = self.idem_get(key).await
        {
            return Ok(v);
        }
        let adapter = self.adapter(&params.account)?;
        let value = adapter.call_native(&params.op, params.params).await?;
        if let Some(key) = params.idempotency_key {
            self.idem_put(key, IdemValue::Call(value.clone())).await;
        }
        Ok(value)
    }

    pub fn blob(&self, id: &str) -> Result<Option<(std::path::PathBuf, Option<String>)>> {
        self.store.get_blob(id)
    }

    pub fn put_blob(&self, bytes: &[u8], mime: Option<&str>, filename: Option<&str>) -> Result<String> {
        if bytes.is_empty() {
            return Err(ChannelError::Invalid("empty blob".into()));
        }
        self.store.save_blob(bytes, mime, filename)
    }


    pub async fn blob_upload_slot(&self) -> Value {
        self.gc_tickets().await;
        let ticket = ulid::Ulid::generate().to_string();
        self.tickets.lock().await.insert(ticket.clone(), Instant::now());
        let url = format!(
            "{}/v1/blobs/upload/{ticket}",
            self.blob_base.trim_end_matches('/')
        );
        serde_json::json!({ "method": "PUT", "url": url })
    }

    pub async fn consume_upload_ticket(&self, ticket: &str) -> bool {
        self.gc_tickets().await;
        self.tickets.lock().await.remove(ticket).is_some()
    }

    async fn gc_tickets(&self) {
        let mut map = self.tickets.lock().await;
        map.retain(|_, t| t.elapsed() < Duration::from_secs(600));
    }
}

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::loopback::LoopbackAdapter;
    use crate::adapter::onebot::OneBotAdapter;
    use crate::config::AccountConfig;
    use crate::message::Part;

    fn loopback_cfg(id: &str) -> AccountConfig {
        AccountConfig {
            id: id.into(),
            channel: "loopback".into(),
            profile: None,
            mode: None,
            listen: None,
            ws_url: None,
            api_base_url: None,
            access_token_env: None,
            download_media: None,
        }
    }

    fn onebot_public(id: &str) -> AccountConfig {
        AccountConfig {
            id: id.into(),
            channel: "onebot".into(),
            profile: Some("public".into()),
            mode: Some("reverse_ws".into()),
            listen: Some("127.0.0.1:0".into()),
            ws_url: None,
            api_base_url: None,
            access_token_env: None,
            download_media: Some(false),
        }
    }

    #[tokio::test]
    async fn search_and_public_call() {
        let dir = {
            let p = std::env::temp_dir().join("gladys-svc-test");
            std::fs::create_dir_all(&p).expect("temp");
            p
        };
        let store = Store::memory(&dir).expect("store");
        let lb = Arc::new(LoopbackAdapter::new(&loopback_cfg("lb")));
        let ob = Arc::new(
            OneBotAdapter::new(&onebot_public("qq"), store.clone(), "http://127.0.0.1:3920")
                .expect("ob"),
        );
        let mut adapters: HashMap<String, Arc<dyn Adapter>> = HashMap::new();
        adapters.insert("lb".into(), lb);
        adapters.insert("qq".into(), ob);
        let svc = Service::new(store, adapters);

        let env = Envelope {
            id: Envelope::new_id(),
            channel: "loopback".into(),
            account: "lb".into(),
            conversation: Conversation::dm("1"),
            direction: Direction::In,
            sender: Actor {
                id: "1".into(),
                name: None,
            },
            ts: 1,
            platform_id: Some("p1".into()),
            reply_to: None,
            parts: vec![Part::text("alpha beta")],
            native: None,
            recalled: false,
        };
        svc.ingest(Ingress::Message(env)).await.unwrap();
        let hits = svc
            .search(SearchParams {
                query: "alpha".into(),
                account: Some("lb".into()),
                limit: 10,
            })
            .unwrap();
        assert_eq!(hits.len(), 1);

        let err = svc
            .call(CallParams {
                account: "qq".into(),
                op: "send_poke".into(),
                params: serde_json::json!({"user_id": 1}),
                idempotency_key: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, ChannelError::Unsupported { op, .. } if op == "send_poke"));
    }
}
