# 01 - Milestone Specification: Core "File-over-App" Engine & Quick-Access Overlay

## 1. Overview & Objective
* **Milestone Identifier & Title**: Milestone 1 — Core "File-over-App" Engine & Quick-Access Overlay.
* **Core Problem Statement**: Traditional knowledge management tools suffer from either rigid, proprietary databases that trap user data or sluggish web wrappers that consume gigabytes of memory. Milestone 1 establishes Jeanne's foundation: a native, sovereign "File-over-App" architecture where plain Markdown files on disk remain the absolute ground truth, backed by an in-process SQLite FTS5 index and an instant-access floating overlay window.
* **Hardware Ceiling**: Resident process memory must remain strictly **< 80 MB at idle** (excluding transient OS file caching). Startup-to-overlay display latency must not exceed **50 ms**.
* **Dependencies & Tooling**:
  * `crates/core`:
    * `rusqlite = { version = "0.32", features = ["bundled"] }`
    * `notify = "6.1"`
    * `gray_matter = "0.2"`
    * `tokio = { version = "1.43", features = ["full"] }`
    * `serde = { version = "1.0", features = ["derive"] }`
    * `serde_json = "1.0"`
    * `thiserror = "2.0"`
    * `tracing = "0.1"`
  * `apps/desktop/src-tauri`:
    * `tauri = { version = "2.0", features = ["tray-icon"] }`
    * `tauri-plugin-global-shortcut = "2.0"`
    * `tauri-plugin-shell = "2.0"`
* **Vault Path Resolution Strategy**:
  1. Inspect environment variable `JEANNE_VAULT_PATH`. If defined and accessible, use as root.
  2. Fallback to OS user directory:
     * Windows: `%USERPROFILE%\Documents\JeanneVault`
     * Linux: `$HOME/Documents/JeanneVault`
  3. If directory does not exist, recursively create it along with an initial `Welcome.md` introducing Jeanne's markdown conventions.

---

## 2. Data Models & Interface Contracts

### 2.1 Rust Domain Structures (`crates/core::models`)
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NoteFrontmatter {
    pub id: String,
    pub title: String,
    pub date_creation: String,
    pub date_modification: String,
    pub note_type: String, // "semantique" | "episodique" | "procedural"
    pub statut: String,    // "actif" | "obsolete" | "archive"
    pub tags: Vec<String>,
    pub source_media: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub chunk_id: String,
    pub file_path: String,
    pub title: String,
    pub snippet: String,
    pub score: f64,
    pub statut: String,
    pub date_creation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultStats {
    pub total_files: usize,
    pub total_chunks: usize,
    pub last_scan_timestamp: i64,
}
```

### 2.2 SQLite DDL & Automatic FTS5 Synchronization Triggers
Database initialized at `<VaultPath>/.jeanne/database.db`:
```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;

CREATE TABLE IF NOT EXISTS files (
    file_path TEXT PRIMARY KEY,
    file_hash TEXT NOT NULL,
    last_modified INTEGER NOT NULL,
    frontmatter_json TEXT
);

CREATE TABLE IF NOT EXISTS chunks (
    chunk_id TEXT PRIMARY KEY,
    file_path TEXT NOT NULL,
    chunk_index INTEGER NOT NULL,
    content TEXT NOT NULL,
    token_count INTEGER NOT NULL,
    note_type TEXT NOT NULL,
    statut TEXT NOT NULL,
    date_creation INTEGER NOT NULL,
    FOREIGN KEY(file_path) REFERENCES files(file_path) ON DELETE CASCADE
);

CREATE VIRTUAL TABLE IF NOT EXISTS fts_notes USING fts5(
    chunk_id UNINDEXED,
    content,
    file_path UNINDEXED,
    tokenize = 'porter unicode61'
);

-- Automatic synchronization triggers ensuring zero FTS index drift
CREATE TRIGGER IF NOT EXISTS chunks_ai AFTER INSERT ON chunks BEGIN
    INSERT INTO fts_notes(chunk_id, content, file_path)
    VALUES (new.chunk_id, new.content, new.file_path);
END;

CREATE TRIGGER IF NOT EXISTS chunks_ad AFTER DELETE ON chunks BEGIN
    DELETE FROM fts_notes WHERE chunk_id = old.chunk_id;
END;

CREATE TRIGGER IF NOT EXISTS chunks_au AFTER UPDATE ON chunks BEGIN
    DELETE FROM fts_notes WHERE chunk_id = old.chunk_id;
    INSERT INTO fts_notes(chunk_id, content, file_path)
    VALUES (new.chunk_id, new.content, new.file_path);
END;
```

### 2.3 IPC Commands & TypeScript Contract
Backend handlers registered in Tauri v2 (`apps/desktop/src-tauri/src/lib.rs`):
```rust
#[tauri::command]
async fn search_notes(query: String, limit: Option<usize>) -> Result<Vec<SearchResult>, String>;

#[tauri::command]
async fn capture_quick_note(content: String) -> Result<String, String>;

#[tauri::command]
async fn open_note_in_editor(file_path: String) -> Result<(), String>;

#[tauri::command]
async fn get_vault_stats() -> Result<VaultStats, String>;
```

TypeScript client definitions (`apps/desktop/src/lib/types/ipc.ts`):
```typescript
export interface SearchResult {
  chunk_id: string;
  file_path: string;
  title: string;
  snippet: string;
  score: number;
  statut: 'actif' | 'obsolete' | 'archive';
  date_creation: string;
}

export interface VaultStats {
  total_files: number;
  total_chunks: number;
  last_scan_timestamp: number;
}

export interface IpcCommands {
  search_notes(query: string, limit?: number): Promise<SearchResult[]>;
  capture_quick_note(content: string): Promise<string>;
  open_note_in_editor(file_path: string): Promise<void>;
  get_vault_stats(): Promise<VaultStats>;
}
```

---

## 3. Scenarios & Edge Cases

### 3.1 File Watcher Debouncing Invariant
* **Problem**: When a user saves a markdown file in editors such as Obsidian or VS Code, the operating system emits a rapid burst of low-level events (`Create` -> `Modify` -> `Modify` -> `CloseWrite`). Indexing immediately on every event causes database locks and high CPU utilization.
* **Invariant**: The `notify-rs` watcher passes raw events into an unbuffered `tokio::sync::mpsc` channel. An asynchronous debouncer thread accumulates modified paths over a **300 ms sliding debounce window**. Once the window settles with no new events, indexing executes in a single SQLite transaction.

### 3.2 Global Shortcut Conflict & Fallback Registration
* **Primary Shortcut**: `Alt + Space`.
* **Conflict Scenario**: On specific Windows configurations, `Alt + Space` is reserved by the OS Window Management menu or third-party launchers (PowerToys Run).
* **Fallback Protocol**:
  1. Attempt registration of `Alt + Space`.
  2. If registration returns an OS error code or failure, log warning via `tracing::warn!`.
  3. Attempt secondary registration: `Alt + Shift + Space`.
  4. If secondary fails, attempt tertiary registration: `Ctrl + Shift + Space`.
  5. Expose active shortcut mapping in UI settings so the user is informed of the bound key.

### 3.3 Quick-Access Window Mechanics
* Floating overlay dimensions: `width: 720px`, initial `height: 64px`.
* When search results are returned, window dynamically animates height to `400px`.
* Window dismisses instantly (sets visibility to `false`) upon:
  1. `blur` event (loss of focus).
  2. `Escape` keypress.
  3. Enter keypress triggering `open_note_in_editor`.

---

## 4. Acceptance Test Matrix (TDD Assertions)

| Test ID | Objective | Inputs / Setup | Action | Expected Assertions |
| :--- | :--- | :--- | :--- | :--- |
| **TEST-01-01** | Frontmatter parsing | Sample `.md` with valid YAML frontmatter and markdown body | Call `parse_markdown_file()` | `frontmatter.title == "Test"`, body is cleanly separated, missing fields receive default values |
| **TEST-01-02** | Lexical FTS5 search | Insert 3 documents into SQLite with distinct terms | Execute `search_notes("architecture")` | Returns top ranked match with highlighted BM25 snippet in `<mark>` tags within < 15 ms |
| **TEST-01-03** | Watcher debouncing | Rapidly write 10 file modifications within 100 ms | Monitor database transactions | Exactly 1 indexing transaction is executed after the 300 ms quiet window |
| **TEST-01-04** | Cascade deletion | Delete source `.md` file from vault | Trigger watcher reconciliation | `files` entry removed, `chunks` removed via cascade, `fts_notes` matching rows deleted |

---

## 5. Verification & Sign-off Checklist
- [ ] `cargo check --workspace` passes with 0 warnings and 0 errors.
- [ ] `cargo test --workspace` executes and passes all unit tests for `crates/core`.
- [ ] Process resident memory verified at idle: `< 80 MB RAM` using Task Manager / `ps`.
- [ ] Global shortcut `Alt + Space` successfully brings the floating overlay to foreground in `< 50 ms`.
- [ ] `capture_quick_note` correctly creates or appends to `Journal/YYYY-MM-DD.md` with timestamp.
