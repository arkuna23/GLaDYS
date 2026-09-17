use thiserror::Error;

pub type Result<T, E = DaemonError> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum DaemonError {
    #[error("{0}")]
    Invalid(String),
    #[error("not found")]
    NotFound,
    #[error("gateway: {0}")]
    Gateway(String),
    #[error("mcp: {0}")]
    Mcp(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
