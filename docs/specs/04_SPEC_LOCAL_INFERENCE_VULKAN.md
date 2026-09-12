# 04 - Milestone Specification: Embedded Local Inference via llama.cpp Vulkan

## 1. Overview & Objective
* **Milestone Identifier & Title**: Milestone 4 — Embedded Local Inference via `llama.cpp` Vulkan.
* **Core Problem Statement**: Relying exclusively on remote LLM endpoints compromises privacy and breaks offline functionality. However, local inference often overwhelms consumer machines through unbounded memory allocations. Milestone 4 integrates an embedded `llama.cpp` runtime compiled with cross-platform Vulkan support, strictly tuned to run 3B-parameter quantized models within a 4.5 GB RAM ceiling on integrated GPUs (Intel Iris Xe, AMD Radeon 680M/780M).
* **Hardware Ceiling**: Total process RAM must never exceed **$\le$ 4.5 GB** during active token generation with a full 4,096-token context. Idle RAM after calling `unload_model()` must drop back to **< 200 MB within 2 seconds**. Generation throughput target: **$\ge$ 15 tokens/second**.
* **Target Model**: `Qwen2.5-3B-Instruct-Q4_K_M.gguf` (binary size ~2.1 GB, SHA-256 verified).
  * Storage path: `%APPDATA%\Jeanne\models\` (Windows) or `~/.local/share/jeanne/models/` (Linux).
* **Dependencies & Tooling**:
  * `crates/core`:
    * `llama-cpp-2 = { version = "0.1", features = ["vulkan"] }` (or custom minimal C FFI bindings linking `llama.cpp` static library with Vulkan compute shader support)
    * `tokio = { version = "1.43", features = ["full"] }`
    * `thiserror = "2.0"`
    * `tracing = "0.1"`

---

## 2. Data Models & Interface Contracts

### 2.1 Hardware Profile & Status Structures
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareInfo {
    pub total_system_ram_mb: u64,
    pub available_ram_mb: u64,
    pub vulkan_device_name: Option<String>,
    pub vulkan_supported: bool,
    pub recommended_model_loaded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalInferenceStats {
    pub prompt_tokens: usize,
    pub generated_tokens: usize,
    pub tokens_per_second: f64,
    pub memory_allocated_mb: u64,
}
```

### 2.2 Local Engine Interface & Mutex Protection
To guarantee deterministic execution on shared memory architectures, inference is strictly single-tenant. The engine encapsulates `llama.cpp` state behind an asynchronous `tokio::sync::Mutex`:

```rust
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct LocalLlmEngine {
    inner: Arc<Mutex<Option<LoadedModel>>>,
}

struct LoadedModel {
    model_path: String,
    // Native llama context and model pointers
    context_size: u32, // Strictly capped at 4096
}

impl LocalLlmEngine {
    pub async fn load_model(&self, model_path: Option<String>) -> Result<(), String>;
    pub async fn unload_model(&self) -> Result<(), String>;
    pub async fn generate_stream(
        &self,
        prompt: String,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<tokio::sync::mpsc::Receiver<String>, String>;
}
```

### 2.3 IPC Commands & TypeScript Contract
Tauri v2 commands exposed to the frontend:
```rust
#[tauri::command]
async fn load_local_model(model_path: Option<String>) -> Result<(), String>;

#[tauri::command]
async fn unload_local_model() -> Result<(), String>;

#[tauri::command]
async fn get_hardware_profile() -> Result<HardwareInfo, String>;
```

TypeScript interface:
```typescript
export interface HardwareInfo {
  total_system_ram_mb: number;
  available_ram_mb: number;
  vulkan_device_name: string | null;
  vulkan_supported: boolean;
  recommended_model_loaded: boolean;
}

export interface LocalLlmCommands {
  load_local_model(model_path?: string): Promise<void>;
  unload_local_model(): Promise<void>;
  get_hardware_profile(): Promise<HardwareInfo>;
}
```

---

## 3. Scenarios & Edge Cases

### 3.1 Strict KV Cache Bounding
* **Invariant**: Context window is strictly bounded at `n_ctx = 4096`. Allocating contexts $> 4096$ is prohibited to prevent memory swapping on 16 GB machines.
* **Token Pruning**: If incoming prompt + retrieved context exceeds 3,500 tokens, the prompt compressor truncates the oldest conversational turns while strictly retaining the system prompt and top-3 RAG chunks.

### 3.2 Single-Tenant Concurrency Control
* If a new generation request is received while an existing generation is active on the `Mutex`:
  * Return immediate typed error: `LlmError::Busy("Local inference engine is currently busy")`.
  * Concurrent queuing is disabled to avoid queue bloat and latency spikes.

### 3.3 Mandatory Explicit Unload (`unload_model`)
* When the user navigates away from chat, switches to remote inference, or requests memory release:
  1. Acquire engine lock.
  2. Free `llama_context` and `llama_model` native C structures.
  3. Trigger OS heap trim / deallocation.
  4. Emit telemetry event verifying resident RAM $< 200$ MB.

---

## 4. Acceptance Test Matrix (TDD Assertions)

| Test ID | Objective | Inputs / Setup | Action | Expected Assertions |
| :--- | :--- | :--- | :--- | :--- |
| **TEST-04-01** | Memory ceiling assertion | Load `Qwen2.5-3B` with `n_ctx = 4096`; ingest 3,800 token prompt | Run inference for 200 tokens | Total process resident RAM remains strictly $\le 4.5$ GB throughout execution |
| **TEST-04-02** | Explicit deallocation | Model actively loaded in memory (~3.8 GB footprint) | Call `unload_local_model()` | Process RAM drops to $< 200$ MB in $< 2.0$ seconds |
| **TEST-04-03** | Inference throughput | 100 token standard evaluation prompt on Vulkan backend | Execute generation | Generates $\ge 15$ tokens/second on standard iGPU hardware |

---

## 5. Verification & Sign-off Checklist
- [ ] Vulkan compute shaders successfully initialize across Intel and AMD iGPUs.
- [ ] `n_ctx` hard-coded and validated at maximum 4,096 tokens.
- [ ] UI provides visible "Unload Model" toggle with live RAM consumption indicator.
- [ ] Single-tenant mutex rejects concurrent calls without process panics.
- [ ] GGUF model SHA-256 verified prior to instantiation to prevent corrupted model execution.
