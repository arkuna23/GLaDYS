use std::sync::Arc;

use anyhow::{Context, Result};
use gladys_daemon::app::App;
use gladys_daemon::channel_mcp::McpHttp;
use gladys_daemon::client::GatewayClient;
use gladys_daemon::config::{config_path_from_args, Config};
use gladys_daemon::http::{self, AppState};
use gladys_daemon::store::Store;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "gladys_daemon=info,info".into()),
        )
        .init();

    let path = config_path_from_args();
    let cfg = Config::load_path(&path).with_context(|| format!("load {}", path.display()))?;
    let token = cfg.token()?;
    let bind = cfg.bind.clone();
    std::fs::create_dir_all(&cfg.data_dir)?;
    let store = Store::open(cfg.data_dir.join("daemon.db"))?;
    let gateway = Arc::new(GatewayClient::new(
        cfg.gateway_url.clone(),
        cfg.gateway_token()?,
    ));
    let channel = Arc::new(McpHttp::new(cfg.channel_url.clone(), cfg.channel_token()?));
    let memory = Arc::new(McpHttp::new(cfg.memory_url.clone(), cfg.memory_token()?));
    let app = App {
        store,
        gateway,
        channel,
        memory,
    };
    app.spawn_registry_loop();
    http::serve(&bind, AppState { app, token }).await?;
    std::process::exit(0);
}
