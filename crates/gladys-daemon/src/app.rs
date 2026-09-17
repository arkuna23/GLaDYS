use std::sync::Arc;
use std::time::Duration;

use crate::channel_mcp::SharedTools;
use crate::client::GatewayClient;
use crate::runner::{exec_script, RunRequest};
use crate::store::{Command, Handler, Store};

#[derive(Clone)]
pub struct App {
    pub store: Store,
    pub gateway: Arc<GatewayClient>,
    pub channel: SharedTools,
    pub memory: SharedTools,
}

impl App {
    pub async fn push_registry(&self) {
        let Ok(body) = self.store.registry_snapshot() else {
            return;
        };
        if let Err(e) = self.gateway.put_registry(body).await {
            tracing::warn!("registry push: {e}");
        }
    }

    pub fn spawn_registry_loop(&self) {
        let this = self.clone();
        tokio::spawn(async move {
            loop {
                this.push_registry().await;
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        });
    }

    pub async fn run(&self, req: RunRequest) {
        let script = match req.kind.as_str() {
            "command" => match self.store.get_command(&req.name) {
                Ok(spec) => spec.script,
                Err(e) => {
                    tracing::warn!("command {}: {e}", req.name);
                    return;
                }
            },
            "handler" => match self.store.get_handler(&req.name) {
                Ok(spec) => spec.script,
                Err(e) => {
                    tracing::warn!("handler {}: {e}", req.name);
                    return;
                }
            },
            other => {
                tracing::warn!("unknown run kind {other}");
                return;
            }
        };
        if let Err(e) = exec_script(
            &script,
            &req,
            self.channel.clone(),
            self.memory.clone(),
            (*self.gateway).clone(),
            Duration::from_secs(180),
        )
        .await
        {
            tracing::warn!("{} {}: {e}", req.kind, req.name);
        }
    }

    pub fn put_command(&self, c: &Command) -> crate::error::Result<()> {
        self.store.put_command(c)
    }

    pub fn put_handler(&self, h: &Handler) -> crate::error::Result<()> {
        self.store.put_handler(h)
    }
}
