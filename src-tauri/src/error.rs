use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{0}")]
    Message(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Regex error: {0}")]
    Regex(#[from] regex::Error),
    #[error("Clipboard error: {0}")]
    Clipboard(String),
    #[error("Input error: {0}")]
    Input(String),
}

impl AppError {
    pub fn message(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

pub type AppResult<T> = Result<T, AppError>;
