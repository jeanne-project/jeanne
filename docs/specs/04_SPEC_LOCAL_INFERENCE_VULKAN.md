# 04 - Milestone Specification: Embedded Local Inference via llama.cpp Vulkan

## 1. Overview & Objective
* **Milestone Identifier & Title**: Milestone 4 — Embedded Local Inference via `llama.cpp` Vulkan.
* **Core Problem Statement**: Relying exclusively on remote LLM endpoints compromises user privacy, risks network latency, and breaks offline availability. However, running local LLMs on consumer machines often triggers catastrophic Out-Of-Memory (OOM) events and UI stalls. Milestone 4 integrates an embedded local LLM inference engine supporting GGUF 3B quantized models (target: `Qwen2.5-3B-Instruct-Q4_K_M.gguf`), hardware-accelerated via Vulkan compute shaders when available, strictly bounded to a 4.5 GB RAM footprint, with single-tenant mutex serialization, automatic prompt compression for bounded KV contexts ($n_{\text{ctx}} \le 4096$), SHA-256 model verification, and instantaneous RAM deallocation ($< 200$ MB in $< 2$ seconds).
* **Hardware Ceiling**:
  * Active local inference process RAM: strictly $\le$ **4.5 GB**.
  * Idle / unloaded state process RAM: strictly $<$ **200 MB** reached within **2.0 seconds** after `unload_model()`.
  * Inference throughput: target $\ge$ **15 tokens/second** on standard iGPU / Vulkan backend.
* **Target Model**:
  * Default model: `Qwen2.5-3B-Instruct-Q4_K_M.gguf` (~2.1 GB binary size, SHA-256 verified).
  * Default storage directory:
    * Windows: `%APPDATA%\Jeanne\models\`
    * Linux: `~/.local/share/jeanne/models/`
* **Dependencies & Tooling**:
  * `crates/core`:
    * `tokio = { version = "1.43", features = ["full"] }`
    * `thiserror = "2.0"`
    * `tracing = "0.1"`
    * `serde = { version = "1.0", features = ["derive"] }`
    * `serde_json = "1.0"`
    * `tokio-util = "0.7"`
    * `async-trait = "0.1"`
    * `sha2 = "0.10"`

---

## 2. Data Models & Interface Contracts

### 2.1 Hardware Profile & Inference Metrics
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HardwareInfo {
    pub total_system_ram_mb: u64,
    pub available_ram_mb: u64,
    pub vulkan_device_name: Option<String>,
    pub vulkan_supported: bool,
    pub recommended_model_loaded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LocalInferenceStats {
    pub prompt_tokens: usize,
    pub generated_tokens: usize,
    pub tokens_per_second: f64,
    pub memory_allocated_mb: u64,
}

impl Default for LocalInferenceStats {
    fn default() -> Self {
        Self {
            prompt_tokens: 0,
            generated_tokens: 0,
            tokens_per_second: 0.0,
            memory_allocated_mb: 0,
        }
    }
}
```

### 2.2 Local Engine Configuration & GGUF Model Descriptors
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalEngineConfig {
    pub model_path: Option<String>,
    pub context_size: u32, // Strictly capped at 4096
    pub threads: Option<u32>,
    pub use_vulkan: bool,
    pub expected_sha256: Option<String>,
}

impl Default for LocalEngineConfig {
    fn default() -> Self {
        Self {
            model_path: None,
            context_size: 4096,
            threads: None,
            use_vulkan: true,
            expected_sha256: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GgufMetadata {
    pub magic: [u8; 4],
    pub version: u32,
    pub tensor_count: u64,
    pub metadata_kv_count: u64,
    pub architecture: Option<String>,
}
```

### 2.3 Extended Error Model (`LlmError`)
```rust
#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    // Existing variants from Milestone 3
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("API error status {status}: {message}")]
    Api { status: u16, message: String },
    #[error("Authentication failed or API key missing")]
    Auth,
    #[error("Inference cancelled by user")]
    Cancelled,
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Keyring error: {0}")]
    Keyring(String),
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("Stream error: {0}")]
    Stream(String),

    // Milestone 4 variants
    #[error("Local inference engine is currently busy: {0}")]
    Busy(String),
    #[error("Model not loaded: {0}")]
    ModelNotLoaded(String),
    #[error("Model integrity verification failed: {0}")]
    ModelIntegrity(String),
    #[error("Hardware or Vulkan acceleration error: {0}")]
    Hardware(String),
    #[error("Local engine internal error: {0}")]
    LocalEngine(String),
}
```

### 2.4 Local LLM Engine Architecture
To guarantee single-tenant access and prevent concurrent inference over subscription or memory corruption, `LocalLlmEngine` coordinates execution via `tokio::sync::Mutex`:

```rust
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub struct LoadedModel {
    pub model_path: String,
    pub context_size: u32, // Strictly capped at 4096
    pub memory_footprint_mb: u64,
    pub metadata: GgufMetadata,
}

pub struct LocalLlmEngine {
    config: LocalEngineConfig,
    state: Arc<Mutex<Option<LoadedModel>>>,
    stats: Arc<Mutex<LocalInferenceStats>>,
    hardware: HardwareInfo,
}

impl LocalLlmEngine {
    pub fn new(config: LocalEngineConfig) -> Self;
    pub fn hardware_info(&self) -> &HardwareInfo;
    pub async fn is_model_loaded(&self) -> bool;
    pub async fn get_stats(&self) -> LocalInferenceStats;
    pub async fn load_model(&self, model_path: Option<String>) -> Result<(), LlmError>;
    pub async fn unload_model(&self) -> Result<(), LlmError>;
    pub async fn generate_stream(
        &self,
        prompt: String,
        cancellation: CancellationToken,
    ) -> Result<tokio::sync::mpsc::Receiver<String>, LlmError>;
}
```

In addition to `generate_stream`, `LocalLlmEngine` implements the shared `LlmProvider` trait:
```rust
#[async_trait::async_trait]
impl LlmProvider for LocalLlmEngine {
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        cancellation_token: CancellationToken,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String, LlmError>> + Send>>, LlmError>;

    async fn health_check(&self) -> Result<bool, LlmError>;
    async fn fetch_models(&self) -> Result<Vec<String>, LlmError>;
}
```

---

## 3. Scenarios & Edge Cases

### 3.1 Strict KV Cache Bounding & Prompt Compression
* **Invariant**: Context window is strictly bounded at `n_ctx = 4096`. Allocations exceeding 4,096 tokens are prohibited.
* **Token Pruning**: If incoming prompt + retrieved context exceeds 3,500 tokens (approximated at $\sim 4$ chars/token or counted tokens), the prompt compressor `compress_local_prompt`:
  1. Preserves the `system` prompt in its entirety.
  2. Preserves the user question and the top-3 RAG chunks tagged `[source: ...]`.
  3. Truncates the oldest conversational turns until the prompt is under 3,500 tokens.

### 3.2 Single-Tenant Concurrency Control
* If a new generation or chat stream request arrives while the engine mutex is locked by an ongoing generation:
  * Return immediate typed error: `LlmError::Busy("Local inference engine is currently busy")`.
  * No queuing of background generations to prevent memory thrashing and latency spikes.

### 3.3 Mandatory Explicit Unload (`unload_model`)
* When requested by user, switching to remote inference, or application idle timeout:
  1. Acquire engine mutex lock.
  2. Free model context and loaded weights.
  3. Reset memory stats and trigger OS heap trim.
  4. Ensure resident memory drops to $< 200$ MB within 2.0 seconds.

### 3.4 GGUF Model Format & SHA-256 Checksum Validation
* Before loading a GGUF file:
  1. Check file existence.
  2. Verify 4-byte magic signature `GGUF` (ASCII 0x47, 0x47, 0x55, 0x46).
  3. Validate GGUF version ($\ge 2$).
  4. If `expected_sha256` is provided, stream file through `sha2::Sha256` in 64 KB chunks to verify hash without loading file into RAM.
  5. Reject mismatched or corrupted files with `LlmError::ModelIntegrity`.

### 3.5 Hardware Profile & Vulkan Detection
* Memory detection:
  * On Linux: read `/proc/meminfo` (`MemTotal` and `MemAvailable` in kB, converted to MB).
  * On Windows / fallback: system memory detection.
* Vulkan detection:
  * Inspect system for Vulkan shared library (`libvulkan.so.1` or `vulkan-1.dll`).
  * Check device properties for Vulkan-compatible physical devices (e.g. Intel Iris Xe, AMD Radeon).
  * If unavailable, fallback to CPU execution without crashing.

---

## 4. Tauri IPC Commands & Frontend Contracts

### 4.1 Tauri IPC Commands (`apps/desktop/src-tauri/src/lib.rs`)
```rust
#[tauri::command]
async fn load_local_model(
    state: tauri::State<'_, AppState>,
    model_path: Option<String>,
) -> Result<(), String>;

#[tauri::command]
async fn unload_local_model(state: tauri::State<'_, AppState>) -> Result<(), String>;

#[tauri::command]
async fn get_hardware_profile(
    state: tauri::State<'_, AppState>,
) -> Result<HardwareInfo, String>;

#[tauri::command]
async fn get_local_inference_stats(
    state: tauri::State<'_, AppState>,
) -> Result<LocalInferenceStats, String>;
```

### 4.2 Tauri Capabilities (`apps/desktop/src-tauri/capabilities/default.json`)
Permissions added:
- `"allow-load-local-model"`
- `"allow-unload-local-model"`
- `"allow-get-hardware-profile"`
- `"allow-get-local-inference-stats"`

### 4.3 TypeScript Contracts (`apps/desktop/src/lib/types/ipc.ts`)
```typescript
export interface HardwareInfo {
  total_system_ram_mb: number;
  available_ram_mb: number;
  vulkan_device_name: string | null;
  vulkan_supported: boolean;
  recommended_model_loaded: boolean;
}

export interface LocalInferenceStats {
  prompt_tokens: usize;
  generated_tokens: usize;
  tokens_per_second: number;
  memory_allocated_mb: number;
}
```

---

## 5. Acceptance Test Matrix (TDD Assertions)

| Test ID | Objective | Inputs / Setup | Action | Expected Assertions |
| :--- | :--- | :--- | :--- | :--- |
| **TEST-04-01** | Memory ceiling assertion | Active generation with loaded 3B model and 4096 context | Generate 200 tokens | Resident RAM $\le 4.5$ GB throughout |
| **TEST-04-02** | Explicit deallocation | Model actively loaded (~2.1 - 3.8 GB footprint) | Call `unload_model()` | Resident RAM drops $< 200$ MB in $< 2.0$ seconds |
| **TEST-04-03** | Inference throughput | 100 token evaluation prompt | Stream generation | Calculates $\ge 15$ tokens/sec on accelerated backend |
| **TEST-04-04** | Strict KV context bounding | Configuration with context $> 4096$ requested | Create engine config | Clamped strictly to $\le 4096$ tokens |
| **TEST-04-05** | Prompt pruning / compression | 4,000 token multi-turn conversation with system prompt and RAG | Call `compress_local_prompt` | Reduces to $< 3,500$ tokens while preserving system instructions and top-3 RAG chunks |
| **TEST-04-06** | Single-tenant concurrency control | Two concurrent calls to `generate_stream` | Trigger second call during first | Second call immediately returns `Err(LlmError::Busy(...))` |
| **TEST-04-07** | Unload idempotence & state reset | Call `unload_model()` twice | Sequential unloads | Returns `Ok(())`, resets model loaded flag and allocated MB to 0 |
| **TEST-04-08** | GGUF header validation | Valid GGUF header file vs invalid magic bytes | Validate header | Valid header passes; bad magic returns `Err(LlmError::ModelIntegrity)` |
| **TEST-04-09** | SHA-256 integrity verification | Valid model file with correct vs altered hash | Verify checksum | Returns `Ok(true)` for match, `Err(LlmError::ModelIntegrity)` on mismatch |
| **TEST-04-10** | Hardware profile discovery | Detect RAM and Vulkan support | Call `detect_hardware()` | Non-zero RAM, proper boolean flags, non-panicking |
| **TEST-04-11** | Immediate stream cancellation | Active local token generation | Cancel `CancellationToken` | Stream terminates within $< 20$ ms with `Err(LlmError::Cancelled)` |
| **TEST-04-12** | `LlmProvider` trait integration | Local engine passed to `LlmProvider` client | Call `chat_stream`, `health_check`, `fetch_models` | Conforms to standard Jeanne provider contract |
| **TEST-04-13** | Model path auto-resolution | None passed as path | Resolve default path | Resolves to OS-specific Jeanne models folder |
| **TEST-04-14** | Unloaded generation rejection | Call `generate_stream` without model loaded | Stream request | Returns `Err(LlmError::ModelNotLoaded(...))` |

---

## 6. Verification & Sign-off Checklist
- [ ] Resident process RAM $\le 4.5$ GB during active inference.
- [ ] Memory drops back to $< 200$ MB within 2.0s upon `unload_model()`.
- [ ] Single-tenant mutex rejects concurrent inference with typed `LlmError::Busy`.
- [ ] Context window strictly capped at $n_{\text{ctx}} \le 4096$ tokens.
- [ ] Prompt compression safely prunes long dialogues above 3,500 tokens.
- [ ] SHA-256 streaming verification catches altered model files.
- [ ] UI provides live hardware/memory indicators and Model Load/Unload toggle.
- [ ] Pre-review check (`cargo clippy`, `cargo test`, `npm run build`) passes with zero warnings.
