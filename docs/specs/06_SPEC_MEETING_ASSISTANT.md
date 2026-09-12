# 06 - Milestone Specification: Zero-Compute Stereo Meeting Recorder & Replay

## 1. Overview & Objective
* **Milestone Identifier & Title**: Milestone 6 — Zero-Compute Stereo Meeting Recorder & Replay Engine.
* **Core Problem Statement**: Traditional meeting transcription tools consume heavy neural compute running deep diarization models (e.g. PyAnnote), exhausting consumer RAM and battery. Milestone 6 implements deterministic, zero-compute stereo diarization by exploiting physical hardware topology (local mic vs. loopback audio), accompanied by slide keyframe capture and map-reduce summarization.
* **Hardware Ceiling**: Resident RAM must remain **< 100 MB** throughout an active 60-minute recording session. Disk I/O must use sequential chunked streaming ($\le 64$ KB buffers) with zero unspooled in-memory audio accumulation.
* **Dependencies & Tooling**:
  * `crates/core`:
    * `cpal = "0.15"` (Microphone + WASAPI Loopback / PipeWire loopback capture)
    * `rubato = "0.15"` (16 kHz resampling)
    * `xcap = "0.0.14"` (Low-overhead screen/window capture)
    * `image = "0.25"`
    * `img_hash = "3.2"` (Perceptual hashing `pHash`)
    * `tokio = { version = "1.43", features = ["full"] }`

---

## 2. Data Models & Interface Contracts

### 2.1 CRITICAL INVARIANT — Clock Drift & Dual-File Spooling
Operating systems clock the physical microphone and the system audio loopback device on distinct hardware quartz crystals. Over a 60-minute meeting, the two streams drift apart by **50 ms to 300 ms**. Interleaving audio frames directly in RAM during live capture results in buffer drift, frame dropping, or audio desynchronization.

* **Invariant Protocol**:
  1. During active recording, write **two separate raw 16 kHz mono PCM files** directly to disk:
     * `<TempDir>/meeting_mic_16k.pcm` (User microphone)
     * `<TempDir>/meeting_sys_16k.pcm` (System loopback: Teams, Zoom, Meet)
  2. Audio buffers are flushed to disk every 1 second, holding no more than 64 KB in heap per stream.
  3. Upon stopping recording, the post-meeting processor aligns the two files based on shared start timestamps and combines them into an interleaved stereo WAV file or processes them jointly.

### 2.2 Deterministic RMS Energy Diarization
For each voice activity window (e.g., 500 ms speech frames), calculate the Root-Mean-Square (RMS) energy on both tracks:

$$\text{RMS} = \sqrt{\frac{1}{N} \sum_{i=1}^N x_i^2}$$

Calculate the energy ratio $R = \frac{\text{RMS}_{\text{mic}}}{\text{RMS}_{\text{sys}} + \epsilon}$ (where $\epsilon = 10^{-6}$ prevents division by zero):

* **$R > 2.0$** $\to$ **`[Me]`** (Local user is speaking into the microphone).
* **$R < 0.5$** $\to$ **`[Remote]`** (Remote participant is speaking over Teams/Zoom/Meet).
* **$0.5 \le R \le 2.0$** $\to$ **`[Cross-talk]`** (Both local user and remote participants are speaking simultaneously).

### 2.3 Slide Change Detection Engine (`pHash`)
To capture presentation slides without creating massive video containers:
1. Every **15 seconds**, `xcap` captures the currently shared window or presentation screen.
2. An 8x8 perceptual hash (`pHash`) is computed via `img_hash`.
3. The Hamming distance between current hash and previous hash is evaluated:
   * If distance variation **$> 15\%$**: A slide transition occurred.
   * Save JPEG keyframe to `Vault/Attachments/Slides/Slide_HH-MM-SS.jpg`.
   * Trigger local OCR on the slide image to extract text for semantic indexing.

### 2.4 Map-Reduce Meeting Summarization Architecture
For meetings where transcripts exceed 2,000 tokens:
* **Map Phase**: Segment chronological transcript into 1,500-token chunks with 200-token overlap.
* Generate intermediate bulleted summaries preserving decisions, blockers, and speaker tags.
* **Reduce Phase**: Synthesize intermediate summaries into standard markdown meeting note:
  * Frontmatter: `type: "episodique"`, `statut: "actif"`.
  * Interactive timestamp format: `[[00:14:30]](seek:870)` which triggers the Tauri media player to jump to second 870.

---

## 3. Scenarios & Edge Cases

### 3.1 Loopback Silence Detection
* When no meeting is active, the loopback device emits digital zero (complete silence).
* The recorder must detect silence and avoid writing continuous zero bytes to disk, saving space while maintaining accurate time indexing via zero-run timestamps.

### 3.2 System Sleep or Crash During Meeting
* If the PC goes to sleep or crashes during a 2-hour meeting:
  * Because raw PCM files are spooled continuously every 1 second, audio up to the final second before crash is preserved on disk.
  * On next launch, Jeanne detects orphaned PCM files in the temp directory and prompts the user to recover and finalize the meeting note.

---

## 4. Acceptance Test Matrix (TDD Assertions)

| Test ID | Objective | Inputs / Setup | Action | Expected Assertions |
| :--- | :--- | :--- | :--- | :--- |
| **TEST-06-01** | Dual-track spooling & drift | Mock capture generating 60 minutes of audio at slightly offset hardware clock rates | Spool to dual PCM files; run post-processing alignment | Final stereo track shows $< 20$ ms phase misalignment; zero frame loss |
| **TEST-06-02** | Diarization accuracy | Synthetic stereo PCM with alternate track bursts ($R = 5.0$ and $R = 0.1$) | Run deterministic RMS classifier | Classified with $100\%$ accuracy into `[Me]` and `[Remote]` without neural models |
| **TEST-06-03** | 60-minute recording memory audit | Continuous 60-minute dual-stream recording simulator | Measure heap usage every 5 minutes | Resident RAM stays strictly $< 100$ MB throughout the 60-minute window |

---

## 5. Verification & Sign-off Checklist
- [ ] WASAPI Loopback on Windows and PipeWire on Linux capture clear remote audio.
- [ ] Microphones and system audio save to separate files before alignment.
- [ ] Slide transitions detected with $> 85\%$ accuracy without false positives from video playback.
- [ ] Meeting notes generated with interactive `[[HH:MM:SS]](seek:seconds)` timestamp links.
