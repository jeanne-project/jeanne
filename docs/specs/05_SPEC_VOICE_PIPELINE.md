# 05 - Milestone Specification: Audio Subsystem & Bidirectional Voice Pipeline (STT/TTS)

## 1. Overview & Objective
* **Milestone Identifier & Title**: Milestone 5 — Audio Subsystem & Bidirectional Voice Pipeline (STT/TTS).
* **Core Problem Statement**: Natural voice interaction requires low-latency speech-to-text (STT) and text-to-speech (TTS) without continuously monopolizing system memory or blocking the main event loop. Milestone 5 introduces a modular, demand-driven voice pipeline combining cross-platform audio device enumeration (`cpal`), mathematical real-time sample rate conversion (`rubato`), Voice Activity Detection (VAD) energy gating, sentence-streaming Piper TTS synthesis, and embedded Whisper STT transcription.
* **Hardware Ceiling**:
  * Voice mode toggled **OFF**: strictly **+0 MB** residual RAM footprint (all audio streams dropped, model weights and worker threads deallocated).
  * Voice mode toggled **ON** (active capture & synthesis): audio subsystem heap footprint strictly $\le$ **250 MB**.
  * Total process footprint with local LLM active: strictly $\le$ **4.5 GB**.
  * Latency to first synthesized audio byte (TTFB): strictly **< 800 ms** via incremental sentence-level streaming.
* **Dependencies & Tooling**:
  * `crates/core`:
    * `cpal = "0.15"`
    * `rubato = "0.15"`
    * `tokio = { version = "1.43", features = ["full"] }`
    * `thiserror = "2.0"`
    * `tracing = "0.1"`
    * `serde = { version = "1.0", features = ["derive"] }`
    * `serde_json = "1.0"`
    * `tokio-util = "0.7"`
    * `async-trait = "0.1"`
  * `apps/desktop`:
    * Tauri v2 IPC commands & permissions in `capabilities/default.json`.
    * Svelte 5 (Runes) microphone indicator and bidirectional voice panel.

---

## 2. Data Models & Interface Contracts

### 2.1 Audio Device Discovery Models
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub default_sample_rate: u32,
    pub is_default: bool,
    pub is_input: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AudioDevicesReport {
    pub input_devices: Vec<AudioDevice>,
    pub output_devices: Vec<AudioDevice>,
    pub default_input_name: Option<String>,
    pub default_output_name: Option<String>,
}
```

### 2.2 Voice Interaction State & Metrics
```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum VoiceState {
    Idle,
    Listening,
    Transcribing,
    Thinking,
    Speaking,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VoiceStatus {
    pub is_active: bool,
    pub state: VoiceState,
    pub input_sample_rate: u32,
    pub memory_allocated_mb: u64,
    pub last_transcription_latency_ms: u64,
    pub last_synthesis_ttfb_ms: u64,
    pub active_device_name: Option<String>,
}

impl Default for VoiceStatus {
    fn default() -> Self {
        Self {
            is_active: false,
            state: VoiceState::Idle,
            input_sample_rate: 16000,
            memory_allocated_mb: 0,
            last_transcription_latency_ms: 0,
            last_synthesis_ttfb_ms: 0,
            active_device_name: None,
        }
    }
}
```

### 2.3 Real-time Resampling Invariant (`rubato`)
Standard PC microphones captured via `cpal` operate natively at 44.1 kHz, 48 kHz, or 96 kHz (mono or stereo). Whisper STT algorithms strictly require **16,000 Hz 16-bit mono float PCM** (`[-1.0, 1.0]`).
Supplying unconverted audio to Whisper causes fatal assertion panics or complete transcription garbage.

The resampling abstraction downmixes multi-channel input to mono and applies polynomial resampling:
```rust
use rubato::{FastFixedIn, PolynomialDegree, Resampler};

pub struct AudioResampler {
    resampler: FastFixedIn<f32>,
    pub input_sample_rate: u32,
    pub target_sample_rate: u32, // Strictly 16000
    pub channels: u16,
}

impl AudioResampler {
    pub fn new(input_rate: u32, channels: u16) -> Result<Self, VoiceError>;
    pub fn process_interleaved_chunk(&mut self, input: &[f32]) -> Result<Vec<f32>, VoiceError>;
    pub fn resample_buffer_mono(input: &[f32], input_rate: u32, target_rate: u32) -> Result<Vec<f32>, VoiceError>;
}
```

### 2.4 Voice Activity Detection (VAD) & Energy Gate
To prevent continuous transmission of ambient noise and detect end-of-speech utterances without external Python runtimes, a native energy-based VAD computes frame Root Mean Square (RMS) and decibels relative to full scale (dBFS):
$$\text{RMS} = \sqrt{\frac{1}{N} \sum_{i=1}^{N} x_i^2}, \quad \text{dBFS} = 20 \log_{10}(\text{RMS} + 10^{-9})$$

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VadConfig {
    pub energy_threshold: f32,      // Default: 0.015 (~ -36 dBFS)
    pub silence_timeout_ms: u64,    // Default: 700 ms
    pub min_speech_duration_ms: u64,// Default: 250 ms
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            energy_threshold: 0.015,
            silence_timeout_ms: 700,
            min_speech_duration_ms: 250,
        }
    }
}

pub struct VoiceActivityDetector {
    config: VadConfig,
    is_speaking: bool,
    silence_samples_count: usize,
    speech_samples_count: usize,
    sample_rate: u32,
}

impl VoiceActivityDetector {
    pub fn new(config: VadConfig, sample_rate: u32) -> Self;
    pub fn calculate_rms(samples: &[f32]) -> f32;
    pub fn process_frame(&mut self, frame: &[f32]) -> VadDecision;
    pub fn reset(&mut self);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VadDecision {
    Silence,
    SpeechOngoing,
    SpeechEnded,
}
```

### 2.5 Incremental Sentence-Level TTS Streaming (`SentenceSplitter`)
To eliminate the 3–5 second delay of waiting for full LLM responses before generating speech, the TTS pipeline segments incoming tokens on punctuation (`.`, `!`, `?`, `\n`, `:`, `;`) followed by whitespace or EOF, and dispatches individual sentences to Piper immediately.

```rust
pub struct SentenceSplitter {
    buffer: String,
}

impl SentenceSplitter {
    pub fn new() -> Self;
    pub fn push_token(&mut self, token: &str) -> Vec<String>;
    pub fn flush(&mut self) -> Option<String>;
    pub fn reset(&mut self);
}
```

### 2.6 STT & TTS Engine Abstractions
```rust
#[async_trait::async_trait]
pub trait SttEngine: Send + Sync {
    async fn transcribe(&self, pcm_16k: &[f32]) -> Result<String, VoiceError>;
}

#[async_trait::async_trait]
pub trait TtsEngine: Send + Sync {
    async fn synthesize_sentence(&self, sentence: &str) -> Result<Vec<f32>, VoiceError>;
}
```

### 2.7 Pipeline Error Model (`VoiceError`)
```rust
#[derive(Debug, thiserror::Error)]
pub enum VoiceError {
    #[error("Audio device unavailable: {0}")]
    DeviceUnavailable(String),
    #[error("Audio capture error: {0}")]
    Capture(String),
    #[error("Audio playback error: {0}")]
    Playback(String),
    #[error("Resampling failed: {0}")]
    Resampling(String),
    #[error("STT transcription failed: {0}")]
    Stt(String),
    #[error("TTS synthesis failed: {0}")]
    Tts(String),
    #[error("Voice pipeline is currently inactive")]
    Inactive,
    #[error("Operation cancelled")]
    Cancelled,
    #[error("Audio format invalid: {0}")]
    Format(String),
    #[error("Internal audio error: {0}")]
    Internal(String),
}
```

### 2.8 Voice Pipeline Coordinator (`VoicePipeline`)
```rust
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct VoicePipeline {
    config: VadConfig,
    state: Arc<Mutex<VoiceStatus>>,
    stt: Arc<dyn SttEngine>,
    tts: Arc<dyn TtsEngine>,
}

impl VoicePipeline {
    pub fn new(vad_config: VadConfig, stt: Arc<dyn SttEngine>, tts: Arc<dyn TtsEngine>) -> Self;
    pub async fn start(&self) -> Result<(), VoiceError>;
    pub async fn stop(&self) -> Result<(), VoiceError>;
    pub async fn is_active(&self) -> bool;
    pub async fn get_status(&self) -> VoiceStatus;
    pub async fn transcribe_buffer(&self, samples: &[f32], input_rate: u32) -> Result<String, VoiceError>;
    pub async fn synthesize_speech(&self, text: &str) -> Result<Vec<f32>, VoiceError>;
}
```

---

## 3. Scenarios & Edge Cases

### 3.1 Nominal Execution Flow
1. **Activation**: User clicks the Voice toggle or triggers keyboard shortcut. `start()` is invoked.
2. **Audio Stream Initialization**: `cpal` opens input audio stream with the default device format.
3. **Continuous Capture & Resampling**: Incoming buffers are resampled in real-time to 16,000 Hz mono via `AudioResampler`.
4. **VAD Gating**: `VoiceActivityDetector` monitors energy. Upon detecting speech, pipeline transitions from `Idle` to `Listening`.
5. **Utterance Boundary Detection**: When 700 ms of silence elapses after speech, VAD emits `SpeechEnded`.
6. **STT Processing**: Pipeline transitions to `Transcribing`. Resampled 16 kHz audio buffer is dispatched to `SttEngine`.
7. **LLM & Sentence-Streaming TTS**: Transcribed text enters LLM pipeline. Incoming tokens pass into `SentenceSplitter`. As each complete sentence is formed, it is dispatched to `TtsEngine` immediately.
8. **Low Latency TTFB**: The first sentence begins playback within $< 800$ ms.
9. **Idle Return**: Upon completion of speech playback, pipeline returns to `Listening` (or `Idle`).

### 3.2 Demand-Driven Deallocation (Voice Mode OFF)
When voice mode is toggled OFF:
1. `stop()` is called on `VoicePipeline`.
2. Audio stream handles and worker tasks are dropped.
3. Internal audio sample buffers are cleared.
4. `memory_allocated_mb` drops to `0`.
5. Residual heap delta: strictly **0 MB**.

### 3.3 Sudden Device Disconnection
If the user unplugs a USB microphone during recording:
1. Stream error is trapped in the capture callback.
2. Pipeline falls back to available system default input device.
3. If no devices remain, pipeline enters `Error` state and returns `VoiceError::DeviceUnavailable`.

---

## 4. Tauri IPC Commands & Frontend Contracts

### 4.1 Tauri IPC Commands (`apps/desktop/src-tauri/src/lib.rs`)
```rust
#[tauri::command]
async fn toggle_voice_pipeline(
    state: tauri::State<'_, AppState>,
    active: bool,
) -> Result<bool, String>;

#[tauri::command]
async fn get_voice_status(
    state: tauri::State<'_, AppState>,
) -> Result<VoiceStatus, String>;

#[tauri::command]
async fn list_audio_devices(
    state: tauri::State<'_, AppState>,
) -> Result<AudioDevicesReport, String>;

#[tauri::command]
async fn transcribe_pcm_chunk(
    state: tauri::State<'_, AppState>,
    samples: Vec<f32>,
    sample_rate: u32,
) -> Result<String, String>;

#[tauri::command]
async fn synthesize_text_to_audio(
    state: tauri::State<'_, AppState>,
    text: String,
) -> Result<Vec<f32>, String>;
```

### 4.2 Tauri Capabilities (`apps/desktop/src-tauri/capabilities/default.json`)
Permissions added:
- `"allow-toggle-voice-pipeline"`
- `"allow-get-voice-status"`
- `"allow-list-audio-devices"`
- `"allow-transcribe-pcm-chunk"`
- `"allow-synthesize-text-to-audio"`

### 4.3 TypeScript Contracts (`apps/desktop/src/lib/types/ipc.ts`)
```typescript
export type VoiceState = 'Idle' | 'Listening' | 'Transcribing' | 'Thinking' | 'Speaking' | 'Error';

export interface AudioDevice {
  id: string;
  name: string;
  default_sample_rate: number;
  is_default: boolean;
  is_input: boolean;
}

export interface AudioDevicesReport {
  input_devices: AudioDevice[];
  output_devices: AudioDevice[];
  default_input_name: string | null;
  default_output_name: string | null;
}

export interface VoiceStatus {
  is_active: boolean;
  state: VoiceState;
  input_sample_rate: number;
  memory_allocated_mb: number;
  last_transcription_latency_ms: number;
  last_synthesis_ttfb_ms: number;
  active_device_name: string | null;
}
```

---

## 5. Acceptance Test Matrix (TDD Assertions)

| Test ID | Objective | Inputs / Setup | Action | Expected Assertions |
| :--- | :--- | :--- | :--- | :--- |
| **TEST-05-01** | Resampling 48kHz to 16kHz | 48,000 samples synthetic 440 Hz sine wave at 48 kHz | Call `resample_buffer_mono` | Output contains $16,000 \pm 15$ samples; non-empty; frequency content preserved |
| **TEST-05-02** | Resampling 44.1kHz to 16kHz | 44,100 samples synthetic sine wave at 44.1 kHz | Call `resample_buffer_mono` | Output contains $16,000 \pm 15$ samples |
| **TEST-05-03** | Interleaved multi-channel downmix | 2-channel interleaved buffer [L0, R0, L1, R1, ...] at 48 kHz | Call `process_interleaved_chunk` | Downmixes stereo to mono $[(L+R)/2]$ before resampling to 16 kHz |
| **TEST-05-04** | VAD silence detection | 16 kHz buffer with RMS below energy threshold for $>700$ ms | Process frames via `VoiceActivityDetector` | Decision transitions to `VadDecision::SpeechEnded` after 700 ms silence |
| **TEST-05-05** | VAD speech onset detection | 16 kHz buffer with RMS $>0.05$ (speech level) | Process frames | Decision transitions to `VadDecision::SpeechOngoing`, `is_speaking` is true |
| **TEST-05-06** | SentenceSplitter punctuation | Token stream: `["Hello", " world", ".", " How", " are", " you", "?"]` | Call `push_token` per token | Emits `["Hello world."]` on `.` and `["How are you?"]` on `?` |
| **TEST-05-07** | SentenceSplitter trailing flush | Token stream: `["Final", " thought", " without", " dot"]` | Call `flush()` | Returns `Some("Final thought without dot")` |
| **TEST-05-08** | SentenceSplitter abbreviation handling | Token stream: `["e.g.", " this", " is", " a", " test."]` | Call `push_token` | Does not split on middle abbreviations like `e.g.`, yields full sentence |
| **TEST-05-09** | Zero-memory leak on Voice OFF | Pipeline started, transcribes, then `stop()` called | Check `memory_allocated_mb` and state | `is_active == false`, `memory_allocated_mb == 0`, `state == VoiceState::Idle` |
| **TEST-05-10** | STT transcription pipeline | Synthetic 16 kHz audio buffer | Call `transcribe_buffer` | Successfully returns transcribed text without panics |
| **TEST-05-11** | Incremental TTS TTFB latency | Stream of 3 sentences | Call `synthesize_speech` | First audio chunk generated in $< 800$ ms |
| **TEST-05-12** | Audio device discovery | Query host audio devices | Call `get_audio_devices` | Returns valid report without throwing panic; default input populated if available |
| **TEST-05-13** | Inactive pipeline rejection | Pipeline stopped | Call `transcribe_buffer` or `synthesize_speech` | Rejects with `VoiceError::Inactive` |
| **TEST-05-14** | Pipeline state idempotence & reset | Multiple calls to `start()` and `stop()` | Sequential invocations | Consecutive `stop()` calls return `Ok(())`, no dangling threads or panics |

---

## 6. Verification & Sign-off Checklist
- [ ] Real-time resampling strictly downsamples to 16,000 Hz 16-bit mono float PCM.
- [ ] VAD detects speech onset and terminates after 700 ms silence.
- [ ] SentenceSplitter delivers incremental sentences without splitting abbreviations.
- [ ] Memory footprint is 0 MB when voice mode is OFF.
- [ ] TTFB for first audio synthesis chunk is $< 800$ ms.
- [ ] 5 Tauri IPC commands registered with capabilities and TypeScript typings.
- [ ] Suite of 14 unit and integration tests passing (`TEST-05-01` → `TEST-05-14`).
- [ ] `cargo clippy -p jeanne-core --all-targets -- -D warnings` passes with 0 warnings.
- [ ] `cd apps/desktop && npm run build` passes with 0 errors.
