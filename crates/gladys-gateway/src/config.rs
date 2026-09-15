use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::error::{GatewayError, Result};
use crate::types::ListMode;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
    #[serde(default = "default_token_env")]
    pub token_env: String,
    pub channel_ws: String,
    #[serde(default = "default_channel_token_env")]
    pub channel_token_env: String,
    pub memory_url: String,
    #[serde(default = "default_memory_token_env")]
    pub memory_token_env: String,
    #[serde(default = "default_idle")]
    pub idle_group_secs: u64,
    #[serde(default = "default_debounce")]
    pub debounce_ms: u64,
    #[serde(default)]
    pub owners: Vec<String>,
    pub agent: AgentConfig,
    #[serde(default)]
    pub policy: PolicyConfig,
    #[serde(default)]
    pub mcp: Vec<McpConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentConfig {
    #[serde(default = "default_kind")]
    pub kind: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

fn default_kind() -> String {
    "acp".into()
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct PolicyConfig {
    #[serde(default)]
    pub group_mode: Option<String>,
    #[serde(default)]
    pub dm_mode: Option<String>,
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default)]
    pub dms: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct McpConfig {
    pub name: String,
    pub url: String,
    pub token_env: String,
}

fn default_bind() -> String {
    "127.0.0.1:3922".into()
}
fn default_data_dir() -> PathBuf {
    PathBuf::from("./data/gateway")
}
fn default_token_env() -> String {
    "GLADYS_GATEWAY_TOKEN".into()
}
fn default_channel_token_env() -> String {
    "GLADYS_CHANNEL_TOKEN".into()
}
fn default_memory_token_env() -> String {
    "GLADYS_MEMORY_TOKEN".into()
}
fn default_idle() -> u64 {
    30
}
fn default_debounce() -> u64 {
    1000
}

impl Config {
    pub fn load_path(path: impl AsRef<Path>) -> Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        toml::from_str(&raw).map_err(|e| GatewayError::Invalid(format!("config: {e}")))
    }

    pub fn token(&self) -> Result<String> {
        env(&self.token_env)
    }

    pub fn channel_token(&self) -> Result<String> {
        env(&self.channel_token_env)
    }

    pub fn memory_token(&self) -> Result<String> {
        env(&self.memory_token_env)
    }

    pub fn debounce(&self) -> Duration {
        Duration::from_millis(self.debounce_ms)
    }

    pub fn idle(&self) -> Duration {
        Duration::from_secs(self.idle_group_secs)
    }

    pub fn group_mode(&self) -> ListMode {
        ListMode::parse(self.policy.group_mode.as_deref(), ListMode::Whitelist)
    }

    pub fn dm_mode(&self) -> ListMode {
        ListMode::parse(self.policy.dm_mode.as_deref(), ListMode::Blacklist)
    }
}

fn env(key: &str) -> Result<String> {
    std::env::var(key).map_err(|_| GatewayError::Invalid(format!("missing token env {key}")))
}

pub fn config_path_from_args() -> PathBuf {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--config") => PathBuf::from(args.next().unwrap_or_else(|| "gateway.toml".into())),
        Some(p) if !p.starts_with('-') => PathBuf::from(p),
        _ => PathBuf::from("gateway.toml"),
    }
}
