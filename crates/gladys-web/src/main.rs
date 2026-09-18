use anyhow::{Context, Result};
use gladys_web::config::{config_path_from_args, Config};
use gladys_web::http::{self, AppState};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "gladys_web=info,info".into()),
        )
        .init();

    let path = config_path_from_args();
    let cfg = Config::load_path(&path).with_context(|| format!("load {}", path.display()))?;
    std::fs::create_dir_all(&cfg.data_dir)?;
    let token = cfg.token()?;
    let bind = cfg.bind.clone();
    http::serve(
        &bind,
        AppState {
            token,
            memory_url: cfg.memory_url.clone(),
            memory_token: cfg.memory_token()?,
            http: reqwest::Client::new(),
        },
    )
    .await?;
    std::process::exit(0);
}
