use thiserror::Error;

pub type Result<T, E = WebError> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum WebError {
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
}
