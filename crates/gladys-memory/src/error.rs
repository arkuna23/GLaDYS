use thiserror::Error;

pub type Result<T, E = MemoryError> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum MemoryError {
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl MemoryError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::Invalid(_) => "invalid",
            Self::Io(_) => "io",
            Self::Sqlite(_) => "store",
            Self::Json(_) => "json",
        }
    }
}
