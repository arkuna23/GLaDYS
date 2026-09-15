use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::capability::Capabilities;
use crate::config::AccountConfig;
use crate::error::{ChannelError, Result};
use crate::message::{Conversation, Envelope, Ingress, Part, SendAck};
use crate::store::Store;

pub mod loopback;
pub mod onebot;

pub type IngressTx = mpsc::Sender<Ingress>;

#[derive(Debug, Clone)]
pub struct AccountMeta {
    pub id: String,
    pub channel: String,
    pub profile: String,
    pub self_id: Option<String>,
}

#[async_trait]
pub trait Adapter: Send + Sync {
    fn account(&self) -> AccountMeta;
    fn capabilities(&self) -> Capabilities;
    fn connected(&self) -> bool;
    async fn send(&self, dest: &Conversation, parts: &[Part]) -> Result<SendAck>;
    async fn recall(&self, platform_id: &str) -> Result<()>;
    async fn react(&self, platform_id: &str, emoji: &str) -> Result<()>;
    async fn call_native(&self, op: &str, params: Value) -> Result<Value>;
    async fn fetch_message(&self, platform_id: &str) -> Result<Option<Envelope>>;
    async fn run(self: Arc<Self>, ingress: IngressTx) -> Result<()>;
}

pub fn build_adapter(cfg: &AccountConfig, store: Store, blob_base: &str) -> Result<Arc<dyn Adapter>> {
    match cfg.channel.as_str() {
        "loopback" => Ok(Arc::new(loopback::LoopbackAdapter::new(cfg))),
        "onebot" => Ok(Arc::new(onebot::OneBotAdapter::new(cfg, store, blob_base)?)),
        other => Err(ChannelError::Invalid(format!("unknown channel {other}"))),
    }
}
