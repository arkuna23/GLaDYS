use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{MemoryError, Result};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
    #[serde(default = "default_token_env")]
    pub token_env: String,
    #[serde(default = "default_pack_limit")]
    pub pack_limit: u32,
    #[serde(default = "default_pack_max_chars")]
    pub pack_max_chars: usize,
}

fn default_bind() -> String {
    "127.0.0.1:3921".into()
}

fn default_data_dir() -> PathBuf {
    PathBuf::from("./data/memory")
}

fn default_token_env() -> String {
    "GLADYS_MEMORY_TOKEN".into()
}

fn default_pack_limit() -> u32 {
    20
}

fn default_pack_max_chars() -> usize {
    4000
}

impl Config {
    pub fn load_path(path: impl AsRef<Path>) -> Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        toml::from_str(&raw).map_err(|e| MemoryError::Invalid(format!("config: {e}")))
    }

    pub fn sqlite_path(&self) -> PathBuf {
        self.data_dir.join("memory.db")
    }

    pub fn token(&self) -> Result<String> {
        std::env::var(&self.token_env)
            .map_err(|_| MemoryError::Invalid(format!("missing token env {}", self.token_env)))
    }
}

pub fn config_path_from_args() -> PathBuf {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--config") => PathBuf::from(args.next().unwrap_or_else(|| "memory.toml".into())),
        Some(p) if !p.starts_with('-') => PathBuf::from(p),
        _ => PathBuf::from("memory.toml"),
    }
}
