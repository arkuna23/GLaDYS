use std::sync::Arc;

use async_trait::async_trait;

use crate::error::Result;
use crate::types::{Conversation, ConversationKind, Envelope, Pack, Part};

#[async_trait]
pub trait ChannelIo: Send + Sync {
    async fn send(
        &self,
        account: &str,
        conversation: &Conversation,
        parts: Vec<Part>,
    ) -> Result<()>;
}

#[async_trait]
pub trait MemoryIo: Send + Sync {
    async fn pack(
        &self,
        channel: &str,
        kind: ConversationKind,
        peer: &str,
        person: Option<&str>,
    ) -> Result<Pack>;
}

#[derive(Default)]
pub struct NullChannel;

#[async_trait]
impl ChannelIo for NullChannel {
    async fn send(&self, _account: &str, _conversation: &Conversation, _parts: Vec<Part>) -> Result<()> {
        Ok(())
    }
}

#[derive(Default)]
pub struct NullMemory;

#[async_trait]
impl MemoryIo for NullMemory {
    async fn pack(
        &self,
        _channel: &str,
        _kind: ConversationKind,
        _peer: &str,
        _person: Option<&str>,
    ) -> Result<Pack> {
        Ok(Pack::default())
    }
}

/// Recording channel for tests.
#[derive(Default)]
pub struct RecChannel {
    pub sent: tokio::sync::Mutex<Vec<(String, Conversation, Vec<Part>)>>,
}

#[async_trait]
impl ChannelIo for RecChannel {
    async fn send(
        &self,
        account: &str,
        conversation: &Conversation,
        parts: Vec<Part>,
    ) -> Result<()> {
        self.sent
            .lock()
            .await
            .push((account.into(), conversation.clone(), parts));
        Ok(())
    }
}

#[derive(Default)]
pub struct RecMemory {
    pub calls: tokio::sync::Mutex<Vec<MemCall>>,
    pub pack: tokio::sync::Mutex<Pack>,
}

pub type MemCall = (String, ConversationKind, String, Option<String>);

#[async_trait]
impl MemoryIo for RecMemory {
    async fn pack(
        &self,
        channel: &str,
        kind: ConversationKind,
        peer: &str,
        person: Option<&str>,
    ) -> Result<Pack> {
        self.calls.lock().await.push((
            channel.into(),
            kind,
            peer.into(),
            person.map(ToOwned::to_owned),
        ));
        Ok(self.pack.lock().await.clone())
    }
}

pub fn envelope_from_event(payload: &serde_json::Value) -> Option<Envelope> {
    serde_json::from_value(payload.clone()).ok()
}

#[derive(Clone, Default)]
pub struct ChannelSlot {
    inner: Arc<tokio::sync::Mutex<Option<Arc<dyn ChannelIo>>>>,
}

impl ChannelSlot {
    pub async fn set(&self, io: Arc<dyn ChannelIo>) {
        *self.inner.lock().await = Some(io);
    }
}

#[async_trait]
impl ChannelIo for ChannelSlot {
    async fn send(
        &self,
        account: &str,
        conversation: &Conversation,
        parts: Vec<Part>,
    ) -> Result<()> {
        let g = self.inner.lock().await;
        match g.as_ref() {
            Some(ch) => ch.send(account, conversation, parts).await,
            None => Ok(()),
        }
    }
}
