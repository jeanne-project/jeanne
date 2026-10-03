pub mod error;
pub mod hardware;
pub mod llm;
pub mod local_llm;
pub mod models;
pub mod parser;
pub mod pii;
pub mod productivity;
pub mod rag;
pub mod storage;
pub mod vault;

pub use productivity::{
    MathEvaluationResult, SnippetItem, TaskItem, append_bookmark, append_log_entry, append_todo,
    create_meeting_note, evaluate_math_expression, extract_tasks_from_file, load_snippets,
    toggle_task_in_file,
};

pub use error::{JeanneError, RagError, Result};
pub use hardware::{HardwareInfo, detect_hardware};
pub use llm::{
    ChatMessage, KeyringManager, LlmError, LlmProvider, OpenAiClient, OpenAiConfig,
    build_rag_prompt,
};
pub use local_llm::{
    GgufMetadata, LoadedModel, LocalEngineConfig, LocalInferenceStats, LocalLlmEngine,
    compress_local_prompt, resolve_default_model_dir, validate_gguf_header, verify_model_sha256,
};
pub use models::{
    CoalaType, FileLink, HybridSearchResult, IndexedChunk, NoteFrontmatter, NoteStatus,
    SearchResult, VaultStats,
};
pub use parser::parse_markdown;
pub use pii::{PiiSession, PiiSlidingBuffer};
pub use rag::{RagEngine, check_circuit_breaker};
pub use storage::StorageManager;
pub use tokio_util::sync::CancellationToken;
pub use vault::{VaultWatcher, resolve_vault_path};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
