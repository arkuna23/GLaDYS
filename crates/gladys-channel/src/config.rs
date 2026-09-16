use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{ChannelError, Result};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
    #[serde(default = "default_token_env")]
    pub token_env: String,
    #[serde(default)]
    pub debug: bool,
    #[serde(default)]
    pub public_base: Option<String>,
    #[serde(default)]
    pub accounts: Vec<AccountConfig>,
}

fn default_bind() -> String {
    "0.0.0.0:3920".into()
}

fn default_data_dir() -> PathBuf {
    PathBuf::from("./data/channel")
}

fn default_token_env() -> String {
    "GLADYS_CHANNEL_TOKEN".into()
}

#[derive(Debug, Clone, Deserialize)]
pub struct AccountConfig {
    pub id: String,
    pub channel: String,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub listen: Option<String>,
    #[serde(default)]
    pub ws_url: Option<String>,
    #[serde(default)]
    pub api_base_url: Option<String>,
    #[serde(default)]
    pub access_token_env: Option<String>,
    #[serde(default)]
    pub download_media: Option<bool>,
}

impl Config {
    pub fn load_path(path: impl AsRef<Path>) -> Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        toml::from_str(&raw).map_err(|e| ChannelError::Invalid(format!("config: {e}")))
    }

    pub fn sqlite_path(&self) -> PathBuf {
        self.data_dir.join("messages.db")
    }

    pub fn blob_dir(&self) -> PathBuf {
        self.data_dir.join("blobs")
    }

    pub fn token(&self) -> Result<String> {
        std::env::var(&self.token_env).map_err(|_| {
            ChannelError::Invalid(format!("missing token env {}", self.token_env))
        })
    }

    pub fn blob_base(&self) -> String {
        if let Some(base) = &self.public_base {
            return base.trim_end_matches('/').to_string();
        }
        blob_base_from_bind(&self.bind)
    }

    pub fn bind_is_loopback(&self) -> bool {
        bind_is_loopback(&self.bind)
    }
}

pub fn blob_base_from_bind(bind: &str) -> String {
    let (host, port) = match bind.rsplit_once(':') {
        Some((h, p)) => (h, p),
        None => return format!("http://{bind}"),
    };
    let host = match host {
        "0.0.0.0" | "::" | "[::]" => "127.0.0.1",
        h => h.trim_start_matches('[').trim_end_matches(']'),
    };
    format!("http://{host}:{port}")
}

pub fn bind_is_loopback(bind: &str) -> bool {
    let host = bind.rsplit_once(':').map(|(h, _)| h).unwrap_or(bind);
    matches!(host, "127.0.0.1" | "localhost" | "::1" | "[::1]")
}

pub fn hosted_blob_url(base: &str, id: &str) -> String {
    format!("{}/v1/blobs/{id}", base.trim_end_matches('/'))
}

pub fn config_path_from_args() -> PathBuf {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--config") => PathBuf::from(args.next().unwrap_or_else(|| "channel.toml".into())),
        Some(p) if !p.starts_with('-') => PathBuf::from(p),
        _ => PathBuf::from("channel.toml"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_base_rewrites_wildcard() {
        assert_eq!(blob_base_from_bind("0.0.0.0:3920"), "http://127.0.0.1:3920");
        assert_eq!(blob_base_from_bind("127.0.0.1:3920"), "http://127.0.0.1:3920");
        assert!(bind_is_loopback("127.0.0.1:3920"));
        assert!(!bind_is_loopback("0.0.0.0:3920"));
    }
}
