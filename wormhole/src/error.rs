use thiserror::Error;

#[derive(Debug, Error)]
pub enum LarryError {
    #[error("provider transient error: {0}")]
    Transient(String),

    #[error("provider permanent error: {0}")]
    Permanent(String),

    #[error("config error: {0}")]
    Config(String),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("http: {0}")]
    Http(#[from] reqwest::Error),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, LarryError>;
