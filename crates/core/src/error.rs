use thiserror::Error;

/// Erreurs spécifiques au moteur RAG hybride et temporel.
#[derive(Debug, Error)]
pub enum RagError {
    #[error("Information non trouvée (similarité maximale insuffisante: {similarity:.4} < 0.65)")]
    InformationNotFound { similarity: f32 },

    #[error("Erreur de base de données RAG: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Erreur de stockage: {0}")]
    Storage(#[from] Box<JeanneError>),

    #[error("Erreur d'embedding: {0}")]
    Embedding(String),
}

impl From<JeanneError> for RagError {
    fn from(err: JeanneError) -> Self {
        match err {
            JeanneError::Rag(inner) => inner,
            other => RagError::Storage(Box::new(other)),
        }
    }
}

impl PartialEq for RagError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::InformationNotFound { similarity: s1 },
                Self::InformationNotFound { similarity: s2 },
            ) => (s1 - s2).abs() < 1e-6 || s1.to_bits() == s2.to_bits(),
            (Self::Database(e1), Self::Database(e2)) => e1.to_string() == e2.to_string(),
            (Self::Storage(e1), Self::Storage(e2)) => e1.to_string() == e2.to_string(),
            (Self::Embedding(s1), Self::Embedding(s2)) => s1 == s2,
            _ => false,
        }
    }
}

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

    #[error("RAG error: {0}")]
    Rag(#[from] RagError),

    #[error("LLM error: {0}")]
    Llm(#[from] crate::llm::LlmError),

    #[error("Feature not implemented: {0}")]
    NotImplemented(String),
}

impl From<JeanneError> for crate::llm::LlmError {
    fn from(err: JeanneError) -> Self {
        match err {
            JeanneError::Llm(inner) => inner,
            JeanneError::Serialization(e) => crate::llm::LlmError::Serialization(e),
            other => crate::llm::LlmError::Config(other.to_string()),
        }
    }
}

pub type Result<T> = std::result::Result<T, JeanneError>;
