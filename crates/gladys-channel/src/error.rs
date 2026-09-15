use thiserror::Error;

pub type Result<T, E = ChannelError> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum ChannelError {
    #[error("unsupported op {op} on {channel}")]
    Unsupported { op: String, channel: String },
    #[error("not found")]
    NotFound,
    #[error("platform {action}: {message}")]
    Platform { action: String, message: String },
    #[error("unauthorized")]
    Auth,
    #[error("platform unavailable ({account})")]
    PlatformUnavailable { account: String },
    #[error("{0}")]
    Invalid(String),
    #[error("no parts left to send")]
    EmptySend,
    #[error("unknown account {0}")]
    UnknownAccount(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error("{0}")]
    Other(String),
}

impl ChannelError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unsupported { .. } => "unsupported",
            Self::NotFound => "not_found",
            Self::Platform { .. } => "platform",
            Self::Auth => "unauthorized",
            Self::PlatformUnavailable { .. } => "platform_unavailable",
            Self::Invalid(_) => "invalid",
            Self::EmptySend => "empty_send",
            Self::UnknownAccount(_) => "unknown_account",
            Self::Io(_) => "io",
            Self::Sqlite(_) => "store",
            Self::Json(_) => "json",
            Self::Http(_) => "http",
            Self::Other(_) => "error",
        }
    }

    pub fn unsupported(op: impl Into<String>, channel: impl Into<String>) -> Self {
        Self::Unsupported {
            op: op.into(),
            channel: channel.into(),
        }
    }
}
