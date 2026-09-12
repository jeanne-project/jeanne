# 03 - Milestone Specification: OpenAI-Compatible Client & Local PII De-identification

## 1. Overview & Objective
* **Milestone Identifier & Title**: Milestone 3 — OpenAI-Compatible Client & Local PII De-identification.
* **Core Problem Statement**: When users query cloud-hosted LLMs (OpenAI, Anthropic, Mistral, Groq, Ollama), proprietary and personally identifiable information (PII) contained in local notes risks leaking to third-party infrastructure. Milestone 3 implements a secure, streaming, OpenAI-compatible HTTP client coupled with an in-memory, deterministic regex PII masker and demaster.
* **Hardware Ceiling**: Resident RAM must remain strictly **< 150 MB** during active streaming inference. Network connections must immediately abort upon user cancellation without memory or socket leaks.
* **Dependencies & Tooling**:
  * `crates/core`:
    * `reqwest = { version = "0.12", features = ["json", "stream"] }`
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String, // "system" | "user" | "assistant"
    pub content: String,
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

#[derive(Default)]
pub struct PiiSession {
    pub forward_map: HashMap<String, String>, // Original -> Mask (e.g. "alex@corp.com" -> "[EMAIL_1]")
    pub reverse_map: HashMap<String, String>, // Mask -> Original (e.g. "[EMAIL_1]" -> "alex@corp.com")
    counter_email: usize,
    counter_phone: usize,
    counter_financial: usize,
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

### 3.2 Secure Credential Storage via OS Keyring
* API keys are **strictly forbidden** from being stored in plaintext in SQLite, JSON settings, or `.env` files.
* Storage is handled exclusively via `keyring-rs`, binding to:
  * Windows: Windows Credential Manager.
  * Linux: Secret Service API / DBus (freedesktop org.freedesktop.secrets).

### 3.3 Cancellation & Immediate Stream Abort
When the user clicks "Stop Generating" or hits `Escape`:
* The frontend triggers Tauri command `abort_chat_stream(request_id)`.
* Backend triggers `cancellation_token.cancel()`.
* The underlying HTTP stream is immediately terminated, closing the TCP socket and freeing connection resources.

---

## 4. Acceptance Test Matrix (TDD Assertions)

| Test ID | Objective | Inputs / Setup | Action | Expected Assertions |
| :--- | :--- | :--- | :--- | :--- |
| **TEST-03-01** | Outgoing PII redaction | Text containing `john.doe@example.com` and `+33 6 12 34 56 78` | Call `mask_prompt(&session, text)` | Result contains `[EMAIL_1]` and `[PHONE_1]`; wire payload contains 0% plain PII |
| **TEST-03-02** | Fractured token recovery | Mock SSE stream emitting `["Hello, ", "contact [EMAIL", "_1", "] today."]` | Run through streaming transformer | Emits `Hello, `, buffers `[EMAIL`, recovers on `_1]`, outputs `contact john.doe@example.com today.` |
| **TEST-03-03** | Immediate cancellation | Active mock streaming connection emitting 1,000 tokens | Fire `cancellation_token` after 10 tokens | Stream yields `Err(LlmError::Cancelled)` within < 20 ms; connection dropped |

---

## 5. Verification & Sign-off Checklist
- [ ] API keys stored in OS Keyring with zero plaintext artifacts on disk.
- [ ] HTTP requests to `/v1/chat/completions` pass valid JSON and Authorization headers.
- [ ] Sliding buffer correctly handles split tokens without swallowing natural bracket syntax.
- [ ] Resident memory remains `< 150 MB` throughout a 10-minute continuous streaming session.
- [ ] Mandatory source citations appended to factual claims: `[source: filename.md]`.
