# 01 - Vision & System Architecture

## 1. Context & Problem Statement
Personal AI Knowledge Assistants ("Second Brains") consistently fail in production due to three architectural flaws:
1. **Context Rot**: As vaults expand, naive vector search injects noisy chunks into the inference window, exhausting the LLM attention budget and triggering confident hallucinations.
2. **Lack of Temporal Arbitration**: In flat vector indexes, an outdated decision from six months ago carries identical semantic weight to a corrective decision made yesterday.
3. **Infrastructure Bloat**: Running multiple Docker containers (vector databases, message queues, heavy runtimes) overwhelms standard desktop machines (16 GB RAM, shared iGPU).

**Jeanne (J.E.A.N.N.E.)** solves this through memory stratification, native temporal decay ranking, deterministic hardware diarization, and a zero-daemon native architecture.

## 2. System Topology

```
┌────────────────────────────────────────────────────────────────────────┐
│               Local Source of Truth (Markdown Vault)                   │
│                     `~/SecondBrain/**/*.md`                            │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │ File System Events (`notify-rs`)
                                    ▼
┌────────────────────────────────────────────────────────────────────────┐
│                        Rust Core (`jeanne-core`)                       │
│  - Semantic Chunker                  - Hybrid RAG Engine               │
│  - Time-Decay Relevance Ranker       - Local Regex PII Masker          │
└──────────────┬──────────────────────────────────────────┬──────────────┘
               │ In-process binding                       │ IPC stdio (JSON-RPC)
               ▼                                          ▼
┌──────────────────────────────┐          ┌──────────────────────────────┐
│   Derived SQLite Index       │          │     Polyglot Plugins         │
│   - `vec_chunks` (sqlite-vec)│          │     - Parsers (PDF in Go...) │
│   - `fts_notes` (BM25 FTS5)  │          │     - Meeting Replay (FFmpeg)│
│   - Validity Metadata        │          │     - Voice (Piper / Whisper)│
└──────────────────────────────┘          └──────────────────────────────┘
               ▲
               │ Native binding / IPC
┌──────────────┴─────────────────────────────────────────────────────────┐
│                 Tauri v2 Desktop Client (`jeanne-desktop`)             │
│  - Main Application Window (Dashboard, Notes, Settings, Media Player)  │
│  - Quick-Access Floating Overlay (`Alt + Space`) : Instant FTS5 Search │
│  - Deterministic Stereo Capture : Mic (Left) / System Loopback (Right) │
└────────────────────────────────────────────────────────────────────────┘
```

## 3. Engineering Trade-Offs & Rationales
* **Rust Edition 2024 for Core & Desktop**: Eliminates Garbage Collection pauses, delivers deterministic memory cleanup (RAII) under strict 16 GB constraints, and provides zero-cost C/C++ FFI bindings for `sqlite-vec` and `llama.cpp`.
* **Go for Document Parsers & Heavy I/O Plugins**: Go combines rapid development with high I/O performance and mature document processing libraries, compiling into standalone binaries that execute on demand.
* **Embedded SQLite + `sqlite-vec`**: Eliminates background server daemons (Qdrant, PostgreSQL). Runs in-process via a single local `.db` file with near-zero idle RAM usage.
