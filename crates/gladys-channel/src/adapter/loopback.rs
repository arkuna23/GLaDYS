use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::Value;

use super::{AccountMeta, Adapter, IngressTx};
use crate::capability::{Capabilities, CommonCaps, Profile};
use crate::config::AccountConfig;
use crate::error::{ChannelError, Result};
use crate::message::{Conversation, Envelope, Part, SendAck};

pub struct LoopbackAdapter {
    meta: AccountMeta,
    up: AtomicBool,
    seq: AtomicU64,
    pub sent: Mutex<Vec<(Conversation, Vec<Part>)>>,
}

impl LoopbackAdapter {
    pub fn new(cfg: &AccountConfig) -> Self {
        let profile = Profile::parse(cfg.profile.as_deref());
        Self {
            meta: AccountMeta {
                id: cfg.id.clone(),
                channel: "loopback".into(),
                profile: profile.as_str().into(),
                self_id: Some(cfg.id.clone()),
            },
            up: AtomicBool::new(true),
            seq: AtomicU64::new(1),
            sent: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl Adapter for LoopbackAdapter {
    fn account(&self) -> AccountMeta {
        self.meta.clone()
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            channel: "loopback".into(),
            account: self.meta.id.clone(),
            profile: self.meta.profile.clone(),
            common: CommonCaps::full(),
            native_ops: vec![],
            native_message_types: vec![],
        }
    }

    fn connected(&self) -> bool {
        self.up.load(Ordering::Relaxed)
    }

    async fn send(&self, dest: &Conversation, parts: &[Part]) -> Result<SendAck> {
        self.sent
            .lock()
            .map_err(|_| ChannelError::Other("loopback mutex poisoned".into()))?
            .push((dest.clone(), parts.to_vec()));
        let n = self.seq.fetch_add(1, Ordering::Relaxed);
        Ok(SendAck {
            id: Envelope::new_id(),
            platform_id: Some(format!("lb-{n}")),
            dropped: vec![],
        })
    }

    async fn recall(&self, _platform_id: &str) -> Result<()> {
        Ok(())
    }

    async fn react(&self, _platform_id: &str, _emoji: &str) -> Result<()> {
        Ok(())
    }

    async fn call_native(&self, op: &str, _params: Value) -> Result<Value> {
        Err(ChannelError::unsupported(op, "loopback"))
    }

    async fn fetch_message(&self, _platform_id: &str) -> Result<Option<Envelope>> {
        Ok(None)
    }

    async fn run(self: Arc<Self>, ingress: IngressTx) -> Result<()> {
        let _ = ingress
            .send(crate::message::Ingress::Connection {
                account: self.meta.id.clone(),
                up: true,
                detail: "loopback".into(),
                self_id: self.meta.self_id.clone(),
            })
            .await;
        std::future::pending::<()>().await;
        Ok(())
    }
}
