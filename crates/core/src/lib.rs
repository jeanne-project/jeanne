pub mod error;
pub mod llm;
pub mod models;
pub mod parser;
pub mod pii;
pub mod rag;
pub mod storage;
pub mod vault;

pub use error::{JeanneError, RagError, Result};
pub use llm::{
    ChatMessage, KeyringManager, LlmError, LlmProvider, OpenAiClient, OpenAiConfig,
    build_rag_prompt,
};
pub use models::{
    CoalaType, FileLink, HybridSearchResult, IndexedChunk, NoteFrontmatter, NoteStatus,
    SearchResult, VaultStats,
};
pub use parser::parse_markdown;
pub use pii::{PiiSession, PiiSlidingBuffer};
pub use rag::{RagEngine, check_circuit_breaker};
pub use storage::StorageManager;
pub use vault::{VaultWatcher, resolve_vault_path};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
