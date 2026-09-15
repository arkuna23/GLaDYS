use thiserror::Error;

pub type Result<T, E = SchedulerError> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum SchedulerError {
    #[error("{0}")]
    Invalid(String),
    #[error("gateway: {0}")]
    Gateway(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
}
