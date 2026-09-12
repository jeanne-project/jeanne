# 07 - Milestone Specification: Subprocess JSON-RPC Extensibility & PDF Plugin

## 1. Overview & Objective
* **Milestone Identifier & Title**: Milestone 7 — Subprocess JSON-RPC Extensibility & Reference PDF Parser.
* **Core Problem Statement**: Ingesting complex document formats (PDF, DOCX, XLSX) requires heavy parsing libraries that risk crashing the core application or bloating resident memory. Milestone 7 establishes an isolated, polyglot plugin architecture where third-party document parsers and integrations run as ephemeral subprocesses controlled via JSON-RPC 2.0 over `stdio`.
* **Hardware Ceiling**: Zero resident memory overhead on the main Jeanne desktop process. Plugins are spawned on demand, stream extracted Markdown back to the core engine, and immediately terminate to free host RAM.
* **Dependencies & Tooling**:
  * `crates/core`:
    * `tokio = { version = "1.43", features = ["process", "io-util"] }`
    * `serde_json = "1.0"`
    * `thiserror = "2.0"`
  * Reference Plugin (`plugin-pdf`):
    * Written in Go 1.22+ using `pdfcpu` or `rsc.io/pdf`.
    * Compiles into a single standalone binary (`pdf-parser.exe` / `pdf-parser`).

---

## 2. Data Models & Interface Contracts

### 2.1 JSON-RPC 2.0 Transport & Invariant
* **Transport**: Standard I/O (`stdin` for requests, `stdout` for responses, `stderr` for unstructured log streaming).
* **CRITICAL INVARIANT**: Passing large binary file buffers (e.g. multi-megabyte PDF byte arrays) across JSON-RPC is **strictly prohibited**. The request transmits only the absolute file system path (`file_path`). The plugin accesses the file directly from disk using streaming I/O.

### 2.2 JSON-RPC Request Schema (`parse_document`)
```json
{
  "jsonrpc": "2.0",
  "method": "parse_document",
  "params": {
    "file_path": "D:/Vault/Documents/FinancialReport2026.pdf",
    "options": {
      "extract_tables": true,
      "extract_images": false
    }
  },
  "id": 1
}
```

### 2.3 JSON-RPC Response Schema
```json
{
  "jsonrpc": "2.0",
  "result": {
    "title": "Financial Report 2026",
    "content_markdown": "# Financial Report 2026\n\n## Executive Summary\nRevenue increased by 14%...",
    "metadata": {
      "author": "CFO Office",
      "date": "2026-03-15",
      "page_count": 24
    },
    "attachments": []
  },
  "id": 1
}
```

### 2.4 Standardized JSON-RPC Error Codes
* `-32700`: Parse error (Invalid JSON payload on `stdin`).
* `-32601`: Method not found.
* `-32602`: Invalid params (Missing or inaccessible `file_path`).
* `-32001`: File not found or permission denied.
* `-32002`: Corrupted document structure or encrypted PDF without password.
* `-32003`: Internal processing timeout reached.

---

## 3. Scenarios & Edge Cases

### 3.1 Subprocess Watchdog & Zombie Eradication
To prevent runaway or frozen plugins from leaking background processes:
```rust
pub async fn execute_plugin_with_watchdog(
    executable: PathBuf,
    request: JsonRpcRequest,
    timeout_duration: Duration, // Default: 120 seconds
) -> Result<JsonRpcResponse, PluginError> {
    let mut child = Command::new(executable)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    // Pipe JSON request to child stdin
    // ...

    match tokio::time::timeout(timeout_duration, child.wait_with_output()).await {
        Ok(Ok(output)) => parse_json_rpc_output(output),
        Ok(Err(e)) => Err(PluginError::Io(e)),
        Err(_) => {
            tracing::error!("Plugin timed out after 120s; dispatching SIGTERM");
            let _ = child.kill().await; // Sends SIGKILL on Windows/Linux
            let _ = child.wait().await; // Reap zombie status
            Err(PluginError::Timeout)
        }
    }
}
```

### 3.2 Crash Immunity & Fault Isolation
* If a plugin triggers a segmentation fault or memory exhaustion panic:
  * The main Jeanne desktop process catches the exit code (e.g. non-zero).
  * Returns a structured error `PluginError::SubprocessCrashed(exit_code)` to the UI.
  * The core application remains 100% operational with zero data corruption.

---

## 4. Acceptance Test Matrix (TDD Assertions)

| Test ID | Objective | Inputs / Setup | Action | Expected Assertions |
| :--- | :--- | :--- | :--- | :--- |
| **TEST-07-01** | JSON-RPC roundtrip | Sample 5-page PDF document on disk | Invoke `execute_plugin("plugin-pdf", path)` | Returns valid markdown with `#` headings, markdown tables, and metadata |
| **TEST-07-02** | Watchdog timeout | Mock plugin designed to sleep indefinitely (`sleep(999)`) | Run with timeout = 2 seconds | Child process terminated cleanly; returns `PluginError::Timeout`; 0 zombie processes remain in process table |
| **TEST-07-03** | Crash immunity | Mock plugin that terminates via `abort()` / segfault | Dispatch `parse_document` | Jeanne core catches failure; emits error code `-32002`; main process remains running |

---

## 5. Verification & Sign-off Checklist
- [ ] Plugins discovered automatically in `%APPDATA%/Jeanne/plugins/` via `plugin.json`.
- [ ] No binary payload data transferred over standard I/O (file paths only).
- [ ] Watchdog timer terminates frozen processes within 120 seconds.
- [ ] Reference `plugin-pdf` compiled in Go produces clean markdown headers and tables.
