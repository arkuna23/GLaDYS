use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{Result, SchedulerError};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_token_env")]
    pub token_env: String,
    pub gateway_url: String,
    #[serde(default = "default_gateway_token_env")]
    pub gateway_token_env: String,
}

fn default_bind() -> String {
    "127.0.0.1:3923".into()
}

fn default_token_env() -> String {
    "GLADYS_SCHEDULER_TOKEN".into()
}

fn default_gateway_token_env() -> String {
    "GLADYS_GATEWAY_TOKEN".into()
}

impl Config {
    pub fn load_path(path: impl AsRef<Path>) -> Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        toml::from_str(&raw).map_err(|e| SchedulerError::Invalid(format!("config: {e}")))
    }

    pub fn token(&self) -> Result<String> {
        env(&self.token_env)
    }

    pub fn gateway_token(&self) -> Result<String> {
        env(&self.gateway_token_env)
    }
}

fn env(key: &str) -> Result<String> {
    std::env::var(key).map_err(|_| SchedulerError::Invalid(format!("missing token env {key}")))
}

pub fn config_path_from_args() -> PathBuf {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--config") => PathBuf::from(args.next().unwrap_or_else(|| "scheduler.toml".into())),
        Some(p) if !p.starts_with('-') => PathBuf::from(p),
        _ => PathBuf::from("scheduler.toml"),
    }
}
