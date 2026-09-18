use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{Result, WebError};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
    #[serde(default = "default_token_env")]
    pub token_env: String,
    #[serde(default = "default_memory_url")]
    pub memory_url: String,
    #[serde(default = "default_memory_token_env")]
    pub memory_token_env: String,
}

fn default_bind() -> String {
    "0.0.0.0:3924".into()
}
fn default_data_dir() -> PathBuf {
    PathBuf::from("./data/web")
}
fn default_token_env() -> String {
    "GLADYS_WEB_TOKEN".into()
}
fn default_memory_url() -> String {
    "http://127.0.0.1:3921".into()
}
fn default_memory_token_env() -> String {
    "GLADYS_MEMORY_TOKEN".into()
}

impl Config {
    pub fn load_path(path: impl AsRef<Path>) -> Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        toml::from_str(&raw).map_err(|e| WebError::Invalid(format!("config: {e}")))
    }

    pub fn token(&self) -> Result<String> {
        env(&self.token_env)
    }

    pub fn memory_token(&self) -> Result<String> {
        env(&self.memory_token_env)
    }
}

fn env(key: &str) -> Result<String> {
    std::env::var(key).map_err(|_| WebError::Invalid(format!("missing token env {key}")))
}

pub fn config_path_from_args() -> PathBuf {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--config") => PathBuf::from(args.next().unwrap_or_else(|| "web.toml".into())),
        Some(p) if !p.starts_with('-') => PathBuf::from(p),
        _ => PathBuf::from("web.toml"),
    }
}
