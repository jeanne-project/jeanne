use thiserror::Error;

#[derive(Debug, Error)]
pub enum JeanneError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Frontmatter parsing error: {0}")]
    Frontmatter(String),

    #[error("Watcher error: {0}")]
    Watcher(#[from] notify::Error),

    #[error("Vault error: {0}")]
    Vault(String),

    #[error("Feature not implemented: {0}")]
    NotImplemented(String),
}

pub type Result<T> = std::result::Result<T, JeanneError>;
