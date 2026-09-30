# 03 - Milestone Specification: OpenAI-Compatible Client & Local PII De-identification

## 1. Overview & Objective
* **Milestone Identifier & Title**: Milestone 3 — OpenAI-Compatible Client & Local PII De-identification.
* **Core Problem Statement**: When users query cloud-hosted LLMs (OpenAI, Anthropic, Mistral, Groq, Ollama), proprietary and personally identifiable information (PII) contained in local notes risks leaking to third-party infrastructure. Milestone 3 implements a secure, streaming, OpenAI-compatible HTTP client coupled with an in-memory, deterministic regex PII masker and demaster.
* **Hardware Ceiling**: Resident RAM must remain strictly **< 150 MB** during active streaming inference. Network connections must immediately abort upon user cancellation without memory or socket leaks.
* **Dependencies & Tooling**:
  * `crates/core`:
    * `reqwest = { version = "0.12", default-features = false, features = ["json", "stream", "rustls-tls"] }`
    * `eventsource-stream = "0.2"`
    * `regex = "1.10"`
    * `keyring = "3.0"`
    * `tokio-util = "0.7"`
    * `async-trait = "0.1"`
    * `futures-util = "0.3"`
    * `serde = { version = "1.0", features = ["derive"] }`
    * `serde_json = "1.0"`
    * `thiserror = "2.0"`

---

## 2. Data Models & Interface Contracts

### 2.1 Asynchronous Provider Trait & Domain Models
```rust
use std::pin::Pin;
use async_trait::async_trait;
use futures_core::Stream;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Error)]
pub enum LlmError {
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
}

impl PartialEq for LlmError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Api { status: s1, message: m1 }, Self::Api { status: s2, message: m2 }) => {
                s1 == s2 && m1 == m2
            }
            (Self::Auth, Self::Auth) => true,
            (Self::Cancelled, Self::Cancelled) => true,
            (Self::Keyring(s1), Self::Keyring(s2)) => s1 == s2,
            (Self::Config(s1), Self::Config(s2)) => s1 == s2,
            (Self::Stream(s1), Self::Stream(s2)) => s1 == s2,
            (Self::Serialization(e1), Self::Serialization(e2)) => e1.to_string() == e2.to_string(),
            (Self::Network(e1), Self::Network(e2)) => e1.to_string() == e2.to_string(),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: String, // "system" | "user" | "assistant"
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiConfig {
    pub base_url: String, // e.g. "https://api.openai.com/v1"
    pub model: String,    // e.g. "gpt-4o-mini"
    pub api_key: Option<String>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub timeout_secs: Option<u64>,
}

impl Default for OpenAiConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.openai.com/v1".to_string(),
            model: "gpt-4o-mini".to_string(),
            api_key: None,
            temperature: Some(0.7),
            max_tokens: Some(2048),
            timeout_secs: Some(30),
        }
    }
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        cancellation_token: CancellationToken,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String, LlmError>> + Send>>, LlmError>;

    async fn health_check(&self) -> Result<bool, LlmError>;
    async fn fetch_models(&self) -> Result<Vec<String>, LlmError>;
}
```

### 2.2 PII Masking Engine & Replacement State
The PII filter maintains a bi-directional lookup table scoped exclusively to the lifetime of a single chat transaction:
```rust
use std::collections::HashMap;

#[derive(Debug, Default, Clone)]
pub struct PiiSession {
    pub forward_map: HashMap<String, String>, // Original -> Mask (e.g. "alex@corp.com" -> "[EMAIL_1]")
    pub reverse_map: HashMap<String, String>, // Mask -> Original (e.g. "[EMAIL_1]" -> "alex@corp.com")
    counter_email: usize,
    counter_phone: usize,
    counter_financial: usize,
}

impl PiiSession {
    pub fn new() -> Self;
    pub fn mask_text(&mut self, text: &str) -> String;
    pub fn demask_text(&self, text: &str) -> String;
}
```

### 2.3 PII Detection Regex Suite
Deterministic regular expressions executed in sequence:
* **Emails**:
  `(?i)\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b` $\to$ `[EMAIL_N]`
* **Phone Numbers**:
  `\b(?:\+?\d{1,3}[-.\s]?)?\(?\d{2,4}\)?[-.\s]?\d{2,4}[-.\s]?\d{2,4}\b` $\to$ `[PHONE_N]`
* **Financial Data (IBAN & Credit Cards)**:
  `\b(?:[A-Z]{2}\d{2}[A-Z0-9]{11,30}|\d{4}[-\s]?\d{4}[-\s]?\d{4}[-\s]?\d{4})\b` $\to$ `[FINANCIAL_N]`

### 2.4 Streaming PII Sliding Window Transformer
```rust
pub struct PiiSlidingBuffer {
    buffer: String,
    reverse_map: HashMap<String, String>,
}

impl PiiSlidingBuffer {
    pub fn new(reverse_map: HashMap<String, String>) -> Self;
    pub fn process_chunk(&mut self, chunk: &str) -> String;
    pub fn flush(&mut self) -> String;
}
```

### 2.5 Prompt Builder & Mandatory Source Citations
```rust
pub fn build_rag_prompt(
    user_query: &str,
    context_results: &[crate::models::HybridSearchResult],
    custom_system_prompt: Option<&str>,
) -> Vec<ChatMessage>;
```
Each context chunk injects:
```
[source: {file_path}]
{content}
```
System instructions command the LLM:
*"You are Jeanne, a privacy-first AI knowledge assistant. Answer the user's question using ONLY the provided document contexts. Every factual claim MUST cite its source note using the format `[source: filename.md]`."*

---

## 3. Scenarios & Edge Cases

### 3.1 CRITICAL INVARIANT — Streaming PII Sliding Window Buffer
* **Problem**: In Server-Sent Events (SSE) streaming, language models emit tokens in arbitrary chunks. A masked placeholder like `[EMAIL_1]` can easily arrive split across packet boundaries:
  * Chunk 1: `The user's email is [EMAIL`
  * Chunk 2: `_1] and can be contacted.`
  Attempting immediate substitution on Chunk 1 would output `[EMAIL` to the UI, corrupting the reverse substitution when Chunk 2 arrives.
* **Invariant**: The streaming transformer maintains a sliding buffer:
  1. If a chunk contains an open bracket `[` without a corresponding `]`, emit everything before `[` and buffer `[` and subsequent characters.
  2. Continue buffering incoming tokens while length of buffer is $\le 32$ characters.
  3. Once closing bracket `]` is encountered:
     * Check if buffer matches a key in `reverse_map` (e.g. `[EMAIL_1]`).
     * If matched, emit the original PII value (`alex@corp.com`).
     * If not matched, emit raw buffered text verbatim.
  4. If buffer exceeds 32 characters without closing `]`, flush buffer contents (safeguard against open brackets in normal prose).
  5. When the stream terminates, any remaining buffer contents are flushed verbatim.

### 3.2 Secure Credential Storage via OS Keyring
* API keys are **strictly forbidden** from being stored in plaintext in SQLite, JSON settings, or `.env` files.
* Storage is handled exclusively via `keyring-rs`, binding to:
  * Windows: Windows Credential Manager.
  * Linux: Secret Service API / DBus (freedesktop org.freedesktop.secrets).
  * macOS: Apple Keychain Services.
* A fallback test-mode / mock keyring enables zero-daemon headless execution in non-GUI Linux test environments.

### 3.3 Cancellation & Immediate Stream Abort
When the user clicks "Stop Generating" or hits `Escape`:
* The frontend triggers Tauri command `abort_chat_stream(request_id)`.
* Backend triggers `cancellation_token.cancel()`.
* The underlying HTTP stream is immediately terminated, closing the TCP socket and freeing connection resources within $< 20\text{ ms}$.

---

## 4. Acceptance Test Matrix (TDD Assertions)

| Test ID | Objective | Inputs / Setup | Action | Expected Assertions |
| :--- | :--- | :--- | :--- | :--- |
| **TEST-03-01** | Outgoing PII redaction | Text containing `john.doe@example.com` and `+33 6 12 34 56 78` | Call `mask_text(&mut session, text)` | Result contains `[EMAIL_1]` and `[PHONE_1]`; wire payload contains 0% plain PII |
| **TEST-03-02** | Consistent forward mapping | Text repeating `john.doe@example.com` twice | Call `mask_text(&mut session, text)` | Both occurrences map to identical `[EMAIL_1]`; counter does not increment twice |
| **TEST-03-03** | Financial PII redaction | Text containing IBAN `FR7630006000011234567890189` and credit card `4111-2222-3333-4444` | Call `mask_text(&mut session, text)` | Replaced by `[FINANCIAL_1]` and `[FINANCIAL_2]` |
| **TEST-03-04** | Complete demasking | Masked text with `[EMAIL_1]` | Call `demask_text(&session, text)` | Exactly restores original email string |
| **TEST-03-05** | Fractured token recovery | Mock SSE stream emitting `["Hello, contact [EMAIL", "_1", "] today."]` | Run through `PiiSlidingBuffer` | Emits `Hello, contact `, buffers `[EMAIL`, recovers on `_1]`, outputs `john.doe@example.com today.` |
| **TEST-03-06** | Natural bracket preservation | Stream emitting `["See doc ", "[chapter 3]", " and [[source: note.md]]"]` | Run through `PiiSlidingBuffer` | Emits verbatim without swallowing non-PII brackets |
| **TEST-03-07** | Buffer overflow safety | Stream emitting `[` followed by 35 characters of prose without `]` | Run through `PiiSlidingBuffer` | Flushes buffer content after 32 characters, preventing token truncation |
| **TEST-03-08** | Trailing buffer flush | Stream ending with unterminated `"[EMAIL_"` at EOF | Call `flush()` at end of stream | Emits `[EMAIL_` verbatim without dropping characters |
| **TEST-03-09** | OpenAI client request payload | Prompt with messages `system` and `user` | Call `OpenAiClient::chat_stream` against mock server | Sends `POST /v1/chat/completions`, `stream: true`, valid `Authorization: Bearer <key>` |
| **TEST-03-10** | SSE Delta parsing & [DONE] | Mock server returning standard SSE lines and `data: [DONE]` | Stream through client | Emits deltas sequentially and closes cleanly upon `[DONE]` |
| **TEST-03-11** | Immediate cancellation | Active mock streaming connection emitting 1,000 tokens | Fire `cancellation_token` after 5 tokens | Stream yields `Err(LlmError::Cancelled)` within < 20 ms; connection dropped |
| **TEST-03-12** | API error mapping | Mock server returning 401 Unauthorized and 429 Rate Limited | Call `chat_stream` | Returns `Err(LlmError::Auth)` on 401 and `Err(LlmError::Api { status: 429, .. })` on 429 |
| **TEST-03-13** | RAG prompt builder & citations | Query + 2 `HybridSearchResult` objects | Call `build_rag_prompt` | Output contains `[source: file1.md]`, `[source: file2.md]`, context chunks, and system prompt |
| **TEST-03-14** | End-to-end PII masked stream | User query with PII sent to mock server emitting masked PII | Stream through masked client pipeline | Wire traffic contains ONLY masked placeholders; final stream output recovers clear text |

---

## 5. Verification & Sign-off Checklist
- [ ] API keys stored in OS Keyring with zero plaintext artifacts on disk.
- [ ] HTTP requests to `/v1/chat/completions` pass valid JSON and Authorization headers.
- [ ] Sliding buffer correctly handles split tokens without swallowing natural bracket syntax.
- [ ] Resident memory remains `< 150 MB` throughout a continuous streaming session.
- [ ] Mandatory source citations appended to factual claims: `[source: filename.md]`.

