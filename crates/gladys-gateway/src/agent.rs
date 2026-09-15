use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::{Mutex, oneshot};

use crate::error::Result;
use crate::types::{AgentInput, AgentOutput, ConvKey};

#[async_trait]
pub trait AgentBackend: Send + Sync {
    async fn prompt(&self, key: &ConvKey, input: AgentInput) -> Result<AgentOutput>;
    async fn steer(&self, key: &ConvKey, text: &str) -> Result<()>;
    async fn reset(&self, key: &ConvKey) -> Result<()>;
}

#[derive(Default)]
pub struct FakeBackend {
    pub prompts: Mutex<Vec<AgentInput>>,
    pub steers: Mutex<Vec<(ConvKey, String)>>,
    pub resets: Mutex<Vec<ConvKey>>,
    pub reply: Mutex<String>,
    hold: Mutex<Option<oneshot::Receiver<()>>>,
}

impl FakeBackend {
    pub fn new(reply: impl Into<String>) -> Self {
        Self {
            reply: Mutex::new(reply.into()),
            ..Self::default()
        }
    }

    pub async fn set_hold(&self) -> oneshot::Sender<()> {
        let (tx, rx) = oneshot::channel();
        *self.hold.lock().await = Some(rx);
        tx
    }
}

#[async_trait]
impl AgentBackend for FakeBackend {
    async fn prompt(&self, _key: &ConvKey, input: AgentInput) -> Result<AgentOutput> {
        self.prompts.lock().await.push(input);
        if let Some(rx) = self.hold.lock().await.take() {
            let _ = rx.await;
        }
        Ok(AgentOutput {
            text: self.reply.lock().await.clone(),
        })
    }

    async fn steer(&self, key: &ConvKey, text: &str) -> Result<()> {
        self.steers.lock().await.push((key.clone(), text.to_string()));
        Ok(())
    }

    async fn reset(&self, key: &ConvKey) -> Result<()> {
        self.resets.lock().await.push(key.clone());
        Ok(())
    }
}

pub type DynBackend = Arc<dyn AgentBackend>;
