use thiserror::Error;

pub type Result<T, E = GatewayError> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum GatewayError {
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    Invalid(String),
    #[error("channel: {0}")]
    Channel(String),
    #[error("memory: {0}")]
    Memory(String),
    #[error("agent: {0}")]
    Agent(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
}

impl GatewayError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::Invalid(_) => "invalid",
            Self::Channel(_) => "channel",
            Self::Memory(_) => "memory",
            Self::Agent(_) => "agent",
            Self::Io(_) => "io",
            Self::Sqlite(_) => "store",
            Self::Json(_) => "json",
            Self::Http(_) => "http",
        }
    }
}
