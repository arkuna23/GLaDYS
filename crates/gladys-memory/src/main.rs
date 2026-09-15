use std::sync::Arc;

use anyhow::{Context, Result};
use gladys_memory::config::{config_path_from_args, Config};
use gladys_memory::http::{self, AppState};
use gladys_memory::service::Service;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "gladys_memory=info,info".into()),
        )
        .init();

    let path = config_path_from_args();
    let cfg = Config::load_path(&path).with_context(|| format!("load {}", path.display()))?;
    let token = cfg.token()?;
    let bind = cfg.bind.clone();
    let service = Arc::new(Service::from_config(&cfg)?);
    http::serve(&bind, AppState { service, token }).await?;
    std::process::exit(0);
}
