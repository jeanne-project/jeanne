# 05 - Milestone Specification: Audio Subsystem & Bidirectional Voice (STT/TTS)

## 1. Overview & Objective
* **Milestone Identifier & Title**: Milestone 5 — Audio Subsystem & Bidirectional Voice Pipeline.
* **Core Problem Statement**: Natural voice interaction requires low-latency speech-to-text (STT) and text-to-speech (TTS) without continuously monopolizing system memory. Milestone 5 introduces a modular, demand-driven voice pipeline combining `cpal` audio capture, mathematical real-time sample rate conversion (`rubato`), embedded Whisper STT (`whisper-rs`), and sentence-streaming Piper TTS.
* **Hardware Ceiling**: When Voice mode is toggled OFF, audio memory footprint must be **+0 MB** (zero audio structs resident in heap). When active, first audio output byte latency must remain **< 800 ms**.
* **Dependencies & Tooling**:
  * `crates/core`:
    * `cpal = "0.15"`
    * `rubato = "0.15"` (Real-time polynomial resampling)
    * `whisper-rs = "0.11"` (Embedded Whisper.cpp bindings, `whisper-base.bin`)
    * `piper-rs = "0.1"` (or standalone Piper ONNX CPU runtime)
    * `tokio = { version = "1.43", features = ["full"] }`
    * `thiserror = "2.0"`
    * `tracing = "0.1"`

---

## 2. Data Models & Interface Contracts

### 2.1 CRITICAL INVARIANT — Real-time Resampling (`rubato`)
Standard PC hardware microphones captured via `cpal` operate natively at **44,100 Hz** or **48,000 Hz** stereo or mono. `whisper.cpp` algorithms mathematically require strictly **16,000 Hz 16-bit mono float PCM**. Supplying native 44.1/48 kHz audio directly to Whisper leads to fatal assertion panics or complete transcription garbage.

Input streams must pass through an asynchronous resampling buffer:
```rust
use rubato::{Resampler, FastFixedIn, PolynomialDegree};

pub struct AudioResampler {
    resampler: FastFixedIn<f32>,
    input_sample_rate: u32,
    target_sample_rate: u32, // Strictly 16000
}

impl AudioResampler {
    pub fn new(input_rate: u32) -> Result<Self, String> {
        let resampler = FastFixedIn::<f32>::new(
            16000.0 / input_rate as f64,
            1.0,
            PolynomialDegree::Septic,
            1024,
            1,
        ).map_err(|e| e.to_string())?;

        Ok(Self {
            resampler,
            input_sample_rate: input_rate,
            target_sample_rate: 16000,
        })
    }

    pub fn process_chunk(&mut self, input: &[f32]) -> Result<Vec<f32>, String> {
        let wave_in = vec![input.to_vec()];
        let wave_out = self.resampler.process(&wave_in, None).map_err(|e| e.to_string())?;
        Ok(wave_out[0].clone())
    }
}
```

### 2.2 Voice Interaction Domain Models
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub default_sample_rate: u32,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VoiceState {
    Idle,
    Listening,
    Transcribing,
    Thinking,
    Speaking,
}
```

### 2.3 Incremental Sentence-Level TTS Streaming
To eliminate the 3–5 second delay of waiting for full LLM responses before generating speech, the TTS pipeline segments incoming text tokens by punctuation (`.`, `!`, `?`, `\n`) and dispatches individual sentences to Piper immediately:
```rust
pub struct SentenceSplitter {
    buffer: String,
}

impl SentenceSplitter {
    pub fn push_token(&mut self, token: &str) -> Vec<String> {
        self.buffer.push_str(token);
        let mut sentences = Vec::new();
        while let Some(idx) = self.buffer.find(|c| c == '.' || c == '!' || c == '?') {
            let sentence = self.buffer[..=idx].trim().to_string();
            if !sentence.is_empty() {
                sentences.push(sentence);
            }
            self.buffer = self.buffer[idx + 1..].to_string();
        }
        sentences
    }
}
```

---

## 3. Scenarios & Edge Cases

### 3.1 On-Demand Lifecycle
1. User activates voice mode via keyboard shortcut or UI button.
2. `cpal` opens input audio stream; resampler buffers 16 kHz audio into memory.
3. Silence detection (VAD - Voice Activity Detection based on RMS energy threshold) detects end of speech after 700 ms of silence.
4. Whisper processes buffered 16 kHz audio, yields text prompt, and immediately frees raw audio buffers.
5. In economy mode, Whisper weights are unloaded to free RAM.

### 3.2 Sudden Hardware Disconnection
* If user unplugs USB microphone during speech:
  * `cpal` emits stream error.
  * Pipeline catches error, falls back to default system input device, and alerts UI via event `voice:device-lost`.

---

## 4. Acceptance Test Matrix (TDD Assertions)

| Test ID | Objective | Inputs / Setup | Action | Expected Assertions |
| :--- | :--- | :--- | :--- | :--- |
| **TEST-05-01** | Sample rate conversion | 48,000 Hz synthetic sine wave audio buffer (1 second, 48,000 samples) | Process with `AudioResampler` | Output buffer contains exactly 16,000 samples ($\pm 10$ due to filter window); frequency profile preserved |
| **TEST-05-02** | Zero-memory leak on Voice OFF | Toggle Voice Mode ON, transcribe 1 sentence, toggle Voice Mode OFF | Audit process memory via heap profiling | Audio structures and Whisper contexts completely deallocated; 0 MB residual RAM |
| **TEST-05-03** | Latency to first audio byte | LLM streams tokens: "Yes. We can proceed with the migration." | Pipe to `SentenceSplitter` and Piper TTS | First sentence ("Yes.") synthesized and ready for playback in $< 800$ ms |

---

## 5. Verification & Sign-off Checklist
- [ ] Microphones running at 44.1 kHz, 48 kHz, and 96 kHz convert without distortion.
- [ ] Whisper receives strictly 16,000 Hz 16-bit mono PCM.
- [ ] Piper voice outputs clear French and English speech using lightweight ONNX models (~60 MB).
- [ ] Voice mode can be disabled globally with a single click, completely killing audio threads.
