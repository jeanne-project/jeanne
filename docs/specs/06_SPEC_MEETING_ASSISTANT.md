# 06 - Milestone Specification: Zero-Compute Stereo Meeting Recorder & Replay Engine

## 1. Overview & Objective
* **Milestone Identifier & Title**: Milestone 6 — Zero-Compute Stereo Meeting Recorder & Replay Engine (`06_SPEC_MEETING_ASSISTANT.md`).
* **Core Problem Statement**: Conventional meeting recording and transcription systems rely on heavy neural diarization models (e.g. PyAnnote, WhisperX) that consume hundreds of megabytes of RAM and heavy GPU/CPU compute, rapidly draining battery and exceeding the strict 16 GB shared-memory hardware ceiling. Milestone 6 implements deterministic, zero-compute stereo diarization by exploiting physical hardware topology (Microphone track = local user vs. Loopback track = remote participants), coupled with perceptual slide change detection (`pHash`) and interactive seekable markdown notes.
* **Hardware Ceiling**:
  * Active recording footprint: resident heap allocation strictly **< 100 MB** throughout a sustained 60-minute session.
  * Idle / Stopped mode: strictly **0 MB** residual audio buffer memory (all streams stopped, temporary buffers purged).
  * Disk I/O Invariant: Streaming chunked spooling with buffers **$\le$ 64 KB** per stream. Zero unspooled in-memory accumulation.
  * Audio clock drift compensation: maximum residual phase misalignment **< 20 ms** over 60 minutes.
* **Dependencies & Tooling**:
  * `crates/core`:
    * `cpal = "0.15"` (Microphone & loopback device abstraction)
    * `rubato = "0.15"` (16 kHz resampling and drift adjustment)
    * `image = "0.25"` (Slide keyframe rendering and grayscale reduction)
    * `tokio = { version = "1.43", features = ["full"] }`
    * `thiserror = "2.0"`
    * `tracing = "0.1"`
    * `serde = { version = "1.0", features = ["derive"] }`
    * `serde_json = "1.0"`
    * `chrono = { version = "0.4", default-features = false, features = ["clock", "std"] }`
  * `apps/desktop`:
    * Tauri v2 IPC commands and permissions in `capabilities/default.json`.

---

## 2. Data Models & Interface Contracts

### 2.1 Rust Domain Structures
```rust
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpeakerTag {
    Me,
    Remote,
    CrossTalk,
    Silence,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiarizationMetrics {
    pub rms_mic: f32,
    pub rms_sys: f32,
    pub ratio: f32,
    pub speaker: SpeakerTag,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingConfig {
    pub sample_rate: u32,             // 16000 Hz
    pub chunk_size_bytes: usize,       // 64 * 1024 = 65536 bytes (64 KB)
    pub ratio_me_threshold: f32,       // 2.0 (R > 2.0 -> Me)
    pub ratio_remote_threshold: f32,   // 0.5 (R < 0.5 -> Remote)
    pub silence_threshold_rms: f32,    // 0.005 RMS (-46 dBFS)
    pub slide_interval_secs: u64,      // 15 seconds
    pub phash_threshold_pct: f32,      // 0.15 (15% Hamming variation -> >= 10 bits / 64)
}

impl Default for MeetingConfig {
    fn default() -> Self {
        Self {
            sample_rate: 16000,
            chunk_size_bytes: 64 * 1024,
            ratio_me_threshold: 2.0,
            ratio_remote_threshold: 0.5,
            silence_threshold_rms: 0.005,
            slide_interval_secs: 15,
            phash_threshold_pct: 0.15,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TranscriptSegment {
    pub id: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub speaker: SpeakerTag,
    pub text: String,
    pub seek_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SlideKeyframe {
    pub timestamp_ms: u64,
    pub file_name: String,
    pub relative_path: String,
    pub phash: u64,
    pub hamming_dist_pct: f32,
    pub ocr_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MeetingSession {
    pub id: String,
    pub title: String,
    pub start_time: chrono::DateTime<chrono::Utc>,
    pub end_time: Option<chrono::DateTime<chrono::Utc>>,
    pub duration_seconds: u64,
    pub mic_file_path: PathBuf,
    pub sys_file_path: PathBuf,
    pub output_note_path: Option<PathBuf>,
    pub segments: Vec<TranscriptSegment>,
    pub slides: Vec<SlideKeyframe>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MeetingStatus {
    pub is_recording: bool,
    pub current_session_id: Option<String>,
    pub elapsed_seconds: u64,
    pub memory_allocated_mb: u64,
    pub mic_level_db: f32,
    pub sys_level_db: f32,
    pub slides_detected: usize,
}

impl Default for MeetingStatus {
    fn default() -> Self {
        Self {
            is_recording: false,
            current_session_id: None,
            elapsed_seconds: 0,
            memory_allocated_mb: 0,
            mic_level_db: -100.0,
            sys_level_db: -100.0,
            slides_detected: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MeetingSummaryResult {
    pub session_id: String,
    pub note_path: String,
    pub duration_seconds: u64,
    pub segment_count: usize,
    pub slide_count: usize,
}
```

### 2.2 Error Handling (`MeetingError`)
```rust
#[derive(Debug, thiserror::Error)]
pub enum MeetingError {
    #[error("Meeting recorder is already active with session: {0}")]
    AlreadyActive(String),
    #[error("Meeting recorder is inactive")]
    Inactive,
    #[error("Audio device unavailable: {0}")]
    DeviceUnavailable(String),
    #[error("Audio capture failed: {0}")]
    Capture(String),
    #[error("I/O error during spooling or alignment: {0}")]
    Io(String),
    #[error("Alignment failure: {0}")]
    Alignment(String),
    #[error("Slide capture failure: {0}")]
    SlideCapture(String),
    #[error("Session not found: {0}")]
    SessionNotFound(String),
    #[error("Internal meeting recorder error: {0}")]
    Internal(String),
}
```

### 2.3 Clock Drift & Dual-File Spooling Protocol
1. **Clock Drift Invariant**:
   Microphone inputs and system loopback devices operate on independent quartz crystals. Over a 60-minute meeting, clocks drift by 50 ms to 300 ms. Storing interleaved stereo in memory causes drift accumulation and frame drops.
2. **Dual-File Spooling**:
   During active recording, Jeanne streams audio directly into two separate raw 16 kHz 16-bit mono PCM files:
   - `<TempDir>/session_{id}_mic.pcm`
   - `<TempDir>/session_{id}_sys.pcm`
   Each stream writes via buffered chunks of $\le 64$ KB, flushing every 1 second.
3. **Post-Recording Clock Alignment**:
   On stopping:
   - Inspect total sample lengths $N_{\text{mic}}$ and $N_{\text{sys}}$.
   - Resample or pad the drift delta $\Delta N$ so both tracks synchronize to $< 20$ ms residual phase difference.
   - Form paired frames for diarization and optionally merge to a stereo replay WAV.

### 2.4 Deterministic RMS Diarization Algorithm
For each frame of $N = 8000$ samples (500 ms at 16 kHz):
$$\text{RMS}_{\text{mic}} = \sqrt{\frac{1}{N} \sum_{i=0}^{N-1} x_{\text{mic}}[i]^2}, \quad \text{RMS}_{\text{sys}} = \sqrt{\frac{1}{N} \sum_{i=0}^{N-1} x_{\text{sys}}[i]^2}$$
$$\text{dBFS} = 20 \log_{10}(\text{RMS} + 10^{-9})$$
- If $\text{RMS}_{\text{mic}} < \tau_{\text{silence}}$ and $\text{RMS}_{\text{sys}} < \tau_{\text{silence}} \implies \text{SpeakerTag::Silence}$
- Else calculate $R = \frac{\text{RMS}_{\text{mic}}}{\text{RMS}_{\text{sys}} + 10^{-6}}$:
  * $R > 2.0 \implies \text{SpeakerTag::Me}$
  * $R < 0.5 \implies \text{SpeakerTag::Remote}$
  * $0.5 \le R \le 2.0 \implies \text{SpeakerTag::CrossTalk}$

Contiguous speech frames sharing the same speaker are merged into `TranscriptSegment`s.

### 2.5 Slide Change Detection Engine (`pHash`)
1. Every 15 seconds, a slide capture backend captures the current presentation or active screen.
2. Rescale the image to an $8 \times 8$ grayscale matrix (64 pixels).
3. Compute the average pixel value $\mu = \frac{1}{64} \sum_{i=0}^{63} p_i$.
4. Generate a 64-bit unsigned hash `u64`:
   $$\text{bit } i = \begin{cases} 1 & \text{si } p_i \ge \mu \\ 0 & \text{sinon} \end{cases}$$
5. Calculate the Hamming distance with previous hash $H_{prev}$:
   $$D = (H_{current} \oplus H_{prev}).\text{count\_ones}()$$
   $$\text{Variation} = \frac{D}{64.0}$$
6. If $\text{Variation} > 0.15$ ($D \ge 10$ bits):
   - A slide transition is confirmed.
   - The keyframe is saved as JPEG to `<vault>/Attachments/Slides/slide_{session_id}_{timestamp}.jpg`.
   - Text metadata is extracted via an OCR backend (`OcrBackend` trait).

### 2.6 Meeting Note Generation ("File-over-App")
The engine synthesizes a Markdown document under `<vault>/Journal/` (or `<vault>/Reunions/`):
```markdown
---
id: reunion-2026-10-06-143000
title: Compte-rendu - Point Hebdomadaire
date_creation: "2026-10-06T14:30:00Z"
date_modification: "2026-10-06T15:30:00Z"
note_type: episodique
statut: actif
tags:
  - reunion
  - diarisation
---

# Compte-rendu - Point Hebdomadaire

- **Date** : 2026-10-06 14:30 UTC
- **Durée** : 3600 s (60 min)
- **Diapositives capturées** : 3

## Diapositives Clés
- [[00:05:15]](seek:315) ![Slide](Attachments/Slides/slide_reunion-2026-10-06-143000_00-05-15.jpg)

## Transcription & Diarisation
- [[00:00:05]](seek:5) **[Me]** : Bonjour à tous, débutons la séance.
- [[00:00:15]](seek:15) **[Remote]** : Bonjour, nous avons terminé le sprint précédent.
- [[00:01:20]](seek:80) **[Cross-talk]** : Accordons-nous sur la priorité du jalon.
```
Format invariant: all timestamps follow `[[HH:MM:SS]](seek:seconds)` to allow one-click playback jumping.

### 2.7 IPC Command Signatures & Capabilities
Tauri v2 Commands:
* `start_meeting_recording(title: Option<String>) -> Result<MeetingStatus, String>`
* `stop_meeting_recording() -> Result<MeetingSummaryResult, String>`
* `get_meeting_recording_status() -> Result<MeetingStatus, String>`
* `list_meeting_sessions() -> Result<Vec<MeetingSession>, String>`
* `seek_meeting_audio(seconds: u64) -> Result<(), String>`

Capabilities declarations in `capabilities/default.json`:
* `allow-start-meeting-recording`
* `allow-stop-meeting-recording`
* `allow-get-meeting-recording-status`
* `allow-list-meeting-sessions`
* `allow-seek-meeting-audio`

Permissions definitions in `apps/desktop/src-tauri/permissions/autogenerated/meeting_recorder.toml`.

---

## 3. Scenarios & Edge Cases

### 3.1 Digital Silence on System Loopback
When no remote participant is speaking, the loopback device receives zero energy. The diarizer detects $\text{RMS}_{\text{sys}} \approx 0.0$ and safely categorizes speech as `[Me]` without zero-division errors due to the $\epsilon = 10^{-6}$ guard.

### 3.2 System Crash / Abrupt Interruption Recovery
If the machine goes to sleep or the application terminates abruptly:
- Spooled `.pcm` files on disk are flushed every 1 second and never lost.
- Upon starting the meeting recorder or booting the application, `recover_orphaned_sessions(&vault_path, &temp_dir)` scans for unfinalized `.pcm` sessions and automatically synthesizes their corresponding Markdown meeting notes.

### 3.3 Visual Noise & Subtle Window Resizing Rejection
Minor mouse movements, clock updates in status bars, or video playback micro-variations produce Hamming distances $< 10$ bits ($\le 15\%$). The threshold prevents false keyframe explosions. Additionally, slide transitions are throttled with a minimum cooldown of 5 seconds.

---

## 4. Acceptance Test Matrix (TDD Assertions)

| Test ID | Objective | Inputs / Setup | Action | Expected Assertions |
| :--- | :--- | :--- | :--- | :--- |
| **TEST-06-01** | Dual-track spooling & chunk bounds | Dual-track spooler writing 60 seconds of synthetic PCM | Spool chunks through `DualTrackSpooler` | Chunk writes never exceed 64 KB in heap; output PCM files match expected size |
| **TEST-06-02** | Deterministic RMS energy diarization | 4 frames: Mic high ($R = 5.0$), Sys high ($R = 0.1$), Both high ($R = 1.0$), Both silence | Run `Diarizer::classify_frame` | Correctly tags `[Me]`, `[Remote]`, `[CrossTalk]`, `[Silence]` with 100% accuracy |
| **TEST-06-03** | Clock drift compensation & alignment | Mic stream 16000 samples, Sys stream 16050 samples (clock drift simulation) | Run `AudioAligner::align_tracks` | Residual sample delta is 0; phase misalignment $< 20$ ms |
| **TEST-06-04** | 8x8 Perceptual hashing (`pHash`) | Synthetic image and modified image with varying pixel patterns | Calculate `PerceptualHasher::hash` and `hamming_distance` | Identical image has distance 0; altered image yields correct bit difference |
| **TEST-06-05** | Slide transition detection | Sequence of 3 frames: frame 1, frame 1 with minor noise (<5%), frame 2 (completely different slide >30%) | Process through `SlideDetector` | Frame 1->2 has 0 transition; Frame 2->3 triggers transition and yields `SlideKeyframe` |
| **TEST-06-06** | Markdown note generation & seek links | Finalized meeting session with 3 segments and 1 slide | Generate Markdown note via `MeetingNoteGenerator` | Note contains `type: episodique`, `statut: actif`, and formatted `[[HH:MM:SS]](seek:seconds)` |
| **TEST-06-07** | Orphaned PCM recovery | Orphaned `session_test_mic.pcm` and `session_test_sys.pcm` left in temp directory | Call `MeetingRecorder::recover_orphaned_sessions` | Recovers session, aligns tracks, and creates finalized note |
| **TEST-06-08** | Memory budget audit (< 100 MB active, 0 MB stop) | Simulated 60-minute recording session | Measure allocated heap throughout recording and after stop | Resident heap remains $< 100$ MB during capture and returns to 0 MB residual upon stop |

---

## 5. Verification & Sign-off Checklist
- [ ] `just pre-review` passes with 0 warning, 0 error, all tests green.
- [ ] Tauri IPC commands registered and permissions validated via `check-permissions`.
- [ ] Zero `unwrap()` and `expect()` in `crates/core`.
- [ ] `docs/reviews/M06_CODE_REVIEW.md` initialised and marked `STATUS: APPROUVÉ` by Reviewer.
- [ ] `docs/reviews/M06_QA_REPORT.md` initialised and marked `STATUS: APPROUVÉ` by QA-Profiler.
- [ ] Merge to `main` executed via `just merge-milestone 06 meeting-recorder`.
