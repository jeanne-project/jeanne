pub mod error;
pub mod models;
pub mod parser;
pub mod storage;
pub mod vault;

pub use error::{JeanneError, Result};
pub use models::{IndexedChunk, NoteFrontmatter, SearchResult, VaultStats};
pub use parser::parse_markdown;
pub use storage::StorageManager;
pub use vault::{VaultWatcher, resolve_vault_path};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
