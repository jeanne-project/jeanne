# 08 - Milestone Specification: Hardening, Golden Dataset QA & Production Packaging

## 1. Overview & Objective
* **Milestone Identifier & Title**: Milestone 8 — Hardening, Golden Dataset QA & Production Packaging (v1.0.0).
* **Core Problem Statement**: Delivering a production-grade desktop application requires empirical validation against realistic knowledge vaults, bulletproof fault recovery under hardware anomalies, and automated, signed OS installers. Milestone 8 establishes an automated Golden Dataset evaluation framework, implements edge case hardening (audio hot-unplug, concurrent file modifications), and configures production release bundles.
* **Hardware Ceiling**: Release build idle RAM must remain strictly **< 80 MB**. Zero heap memory growth over a 24-hour soak test with continuous background file updates.
* **Release Artifacts**:
  * Windows: Signed `.msi` and `.exe` installer (Tauri v2 NSIS bundle).
  * Linux: Debian package (`.deb`) and standalone portable `AppImage`.
* **Compiler Release Profile (`Cargo.toml`)**:
  ```toml
  [profile.release]
  opt-level = 3
  lto = true
  codegen-units = 1
  panic = "abort"
  strip = true
  ```

---

## 2. Data Models & Interface Contracts

### 2.1 Golden Dataset Schema (`tests/data/eval_golden_dataset.json`)
The automated benchmark tests retrieval accuracy, faithfulness, and resistance to hallucinations:
```json
[
  {
    "id": "EVAL-001",
    "query": "What is the primary key requirement for sqlite-vec in Jeanne?",
    "ground_truth_note_id": "note_arch_sqlite_vec_01",
    "expected_claims": [
      "Must be a 64-bit signed integer rowid",
      "TEXT or UUID primary keys cause runtime SQL errors"
    ],
    "is_out_of_domain": false
  },
  {
    "id": "EVAL-002",
    "query": "Who won the 2030 World Cup in basketball?",
    "ground_truth_note_id": null,
    "expected_claims": [],
    "is_out_of_domain": true
  }
]
```

### 2.2 Evaluation Scoring Harness & Metrics
The automated test runner computes three mandatory quality metrics:
1. **Retrieval Recall ($\ge 90\%$)**: Percentage of non-out-of-domain queries where the top-3 retrieved chunks contain the `ground_truth_note_id`.
2. **Context Faithfulness ($\ge 95\%$)**: Percentage of generated claims directly supported by retrieved note citations.
3. **Out-of-Domain Hallucination Rate ($0\%$)**: For queries marked `is_out_of_domain: true`, the system must short-circuit and emit the exact fallback string:
   > *"Information not found in your documents."*

---

## 3. Scenarios & Edge Cases

### 3.1 Audio Hot-Unplug Recovery
* **Problem**: During meeting recording, a user disconnects their Bluetooth or USB headset.
* **Recovery Protocol**:
  1. `cpal` emits `cpal::StreamError::DeviceNotAvailable`.
  2. Stream listener catches the error without bubbling panics.
  3. Immediately finalize and close the existing PCM temp files on disk.
  4. Prompt user with a non-blocking toast: *"Audio device disconnected. Meeting saved up to interruption."*

### 3.2 Concurrent External File Modification During Indexing
* **Scenario**: While Jeanne is indexing a 50 KB markdown note, an external sync client (Dropbox / Syncthing / Git) writes a new version.
* **Mitigation Protocol**:
  * Before beginning SQLite transaction, compute SHA-256 hash of read file contents.
  * After parsing and embedding, verify disk file last-modified timestamp and re-compute hash.
  * If hash has changed, discard stale transaction and enqueue new indexing event for the updated file.

---

## 4. Acceptance Test Matrix (TDD Assertions)

| Test ID | Objective | Inputs / Setup | Action | Expected Assertions |
| :--- | :--- | :--- | :--- | :--- |
| **TEST-08-01** | Golden Dataset benchmark | 50 curated queries in `eval_golden_dataset.json` | Execute automated test runner | Retrieval Recall $\ge 90\%$, Faithfulness $\ge 95\%$, Out-of-Domain Hallucinations $= 0$ |
| **TEST-08-02** | Hot-unplug resilience | Active audio recording thread; trigger simulated `StreamError` | Fire error event | Recording finalized cleanly; output file is valid 16 kHz WAV; process stays alive |
| **TEST-08-03** | 24-hour soak test | Ingest 5,000 notes; trigger 100 search queries per hour for 24h | Monitor memory footprint | RAM variance between hour 1 and hour 24 is $< 5$ MB (zero memory leaks) |

---

## 5. Verification & Sign-off Checklist
- [ ] Golden Dataset test suite passes in CI with 0 failures.
- [ ] Release binaries compiled with `lto = true` and `strip = true` for minimum size.
- [ ] Windows installer registers deep-linking protocol and system tray integration.
- [ ] Linux AppImage executes cleanly on clean Ubuntu 22.04 and Fedora 40 environments.
- [ ] Final security audit confirms zero hardcoded API keys or test credentials.
