pub mod error;
pub mod hardware;
pub mod llm;
pub mod local_llm;
pub mod model_discovery;
pub mod models;
pub mod parser;
pub mod pii;
pub mod productivity;
pub mod rag;
pub mod storage;
pub mod vault;
pub mod voice;

pub use voice::{
    AudioDevice, AudioDevicesReport, AudioResampler, PiperTtsEngine, SentenceSplitter, SttEngine,
    TtsEngine, VadConfig, VadDecision, VoiceActivityDetector, VoiceError, VoicePipeline,
    VoiceState, VoiceStatus, WhisperSttEngine, get_audio_devices,
};

pub use model_discovery::{
    DiscoveredModel, discover_models, discover_models_in_dirs, format_file_size,
    get_candidate_model_dirs, guess_architecture_from_name, resolve_model_path,
    resolve_model_path_in_dirs,
};

pub use productivity::{
    MathEvaluationResult, SnippetItem, TaskItem, append_bookmark, append_log_entry, append_todo,
    create_meeting_note, evaluate_math_detailed, evaluate_math_expression, extract_tasks_from_file,
    format_math_result, load_snippets, sanitize_float_precision, toggle_task_in_file,
};

pub use error::{JeanneError, RagError, Result};
pub use hardware::{HardwareInfo, detect_hardware};
pub use llm::{
    ChatMessage, KeyringManager, LlmError, LlmProvider, OpenAiClient, OpenAiConfig,
    build_rag_prompt,
};
pub use local_llm::{
    GgufMetadata, LoadedModel, LocalEngineConfig, LocalInferenceStats, LocalLlmEngine,
    ModelRecommendedParams, calculate_max_allowed_context, compress_local_prompt,
    correct_french_and_english, get_recommended_params_for_architecture, rephrase_text,
    resolve_default_model_dir, summarize_in_bullets, synthesize_local_response, translate_text,
    validate_gguf_header, verify_model_sha256,
};
pub use models::{
    CoalaType, FileLink, HybridSearchResult, IndexedChunk, NoteFrontmatter, NoteStatus,
    SearchResult, VaultStats,
};
pub use parser::parse_markdown;
pub use pii::{PiiSession, PiiSlidingBuffer};
pub use rag::{RagEngine, check_circuit_breaker};
pub use storage::{StorageManager, extract_search_keywords};
pub use tokio_util::sync::CancellationToken;
pub use vault::{VaultWatcher, resolve_vault_path};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
