pub mod error;
pub mod models;
pub mod parser;
pub mod rag;
pub mod storage;
pub mod vault;

pub use error::{JeanneError, RagError, Result};
pub use models::{
    CoalaType, FileLink, HybridSearchResult, IndexedChunk, NoteFrontmatter, NoteStatus,
    SearchResult, VaultStats,
};
pub use parser::parse_markdown;
pub use rag::{RagEngine, check_circuit_breaker};
pub use storage::StorageManager;
pub use vault::{VaultWatcher, resolve_vault_path};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
