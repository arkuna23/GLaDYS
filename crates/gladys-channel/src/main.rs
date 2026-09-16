use std::sync::Arc;

use anyhow::{Context, Result};
use gladys_channel::config::{config_path_from_args, Config};
use gladys_channel::http::{self, AppState};
use gladys_channel::service::Service;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "gladys_channel=info,info".into()),
        )
        .init();

    let path = config_path_from_args();
    let cfg = Config::load_path(&path).with_context(|| format!("load {}", path.display()))?;
    let token = cfg.token()?;
    let bind = cfg.bind.clone();
    let debug = cfg.debug;
    let service = Arc::new(Service::from_config(&cfg)?);
    let _ingress = service.spawn_adapters();
    http::serve(
        &bind,
        AppState {
            service,
            token,
            debug,
            loopback_blobs: true,
            blob_base: cfg.blob_base(),
        },
    )
    .await?;
    // adapter loops are spawned tasks; don't wait on them
    std::process::exit(0);
}
