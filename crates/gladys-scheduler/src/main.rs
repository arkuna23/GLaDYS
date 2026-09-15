use std::sync::Arc;

use anyhow::{Context, Result};
use gladys_scheduler::client::GatewayClient;
use gladys_scheduler::config::{config_path_from_args, Config};
use gladys_scheduler::http::{self, AppState};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "gladys_scheduler=info,info".into()),
        )
        .init();

    let path = config_path_from_args();
    let cfg = Config::load_path(&path).with_context(|| format!("load {}", path.display()))?;
    let token = cfg.token()?;
    let bind = cfg.bind.clone();
    let client = Arc::new(GatewayClient::new(cfg.gateway_url.clone(), cfg.gateway_token()?));
    http::serve(&bind, AppState { client, token }).await?;
    std::process::exit(0);
}
