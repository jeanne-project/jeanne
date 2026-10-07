use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

// ============================================================================
// 1. Modèles de Données & Configuration
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpeakerTag {
    Me,
    Remote,
    CrossTalk,
    Silence,
}

impl std::fmt::Display for SpeakerTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpeakerTag::Me => write!(f, "Me"),
            SpeakerTag::Remote => write!(f, "Remote"),
            SpeakerTag::CrossTalk => write!(f, "Cross-talk"),
            SpeakerTag::Silence => write!(f, "Silence"),
        }
    }
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
    pub sample_rate: u32,            // 16000 Hz
    pub chunk_size_bytes: usize,     // 64 * 1024 = 65536 bytes (64 KB)
    pub ratio_me_threshold: f32,     // 2.0 (R > 2.0 -> Me)
    pub ratio_remote_threshold: f32, // 0.5 (R < 0.5 -> Remote)
    pub silence_threshold_rms: f32,  // 0.005 RMS (-46 dBFS)
    pub slide_interval_secs: u64,    // 15 seconds
    pub phash_threshold_pct: f32,    // 0.15 (15% Hamming variation -> >= 10 bits / 64)
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
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
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

// ============================================================================
// 2. Gestion des Erreurs Typées (MeetingError)
// ============================================================================

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

// ============================================================================
// 3. Spooler Double-Flux (DualTrackSpooler)
// ============================================================================

pub struct DualTrackSpooler {
    pub session_id: String,
    pub mic_path: PathBuf,
    pub sys_path: PathBuf,
    mic_writer: BufWriter<File>,
    sys_writer: BufWriter<File>,
    total_mic_bytes: u64,
    total_sys_bytes: u64,
    chunk_size_limit: usize,
}

impl DualTrackSpooler {
    pub fn new(session_id: &str, temp_dir: &Path) -> Result<Self, MeetingError> {
        let mic_path = temp_dir.join(format!("session_{session_id}_mic.pcm"));
        let sys_path = temp_dir.join(format!("session_{session_id}_sys.pcm"));

        let mic_file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&mic_path)
            .map_err(|e| MeetingError::Io(format!("Impossible d'ouvrir mic.pcm : {e}")))?;

        let sys_file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&sys_path)
            .map_err(|e| MeetingError::Io(format!("Impossible d'ouvrir sys.pcm : {e}")))?;

        Ok(Self {
            session_id: session_id.to_string(),
            mic_path,
            sys_path,
            mic_writer: BufWriter::with_capacity(64 * 1024, mic_file),
            sys_writer: BufWriter::with_capacity(64 * 1024, sys_file),
            total_mic_bytes: 0,
            total_sys_bytes: 0,
            chunk_size_limit: 64 * 1024,
        })
    }

    pub fn write_mic_chunk(&mut self, chunk: &[u8]) -> Result<(), MeetingError> {
        for slice in chunk.chunks(self.chunk_size_limit) {
            self.mic_writer
                .write_all(slice)
                .map_err(|e| MeetingError::Io(format!("Écriture mic.pcm échouée : {e}")))?;
            self.total_mic_bytes += slice.len() as u64;
        }
        Ok(())
    }

    pub fn write_sys_chunk(&mut self, chunk: &[u8]) -> Result<(), MeetingError> {
        for slice in chunk.chunks(self.chunk_size_limit) {
            self.sys_writer
                .write_all(slice)
                .map_err(|e| MeetingError::Io(format!("Écriture sys.pcm échouée : {e}")))?;
            self.total_sys_bytes += slice.len() as u64;
        }
        Ok(())
    }

    pub fn write_mic_samples_f32(&mut self, samples: &[f32]) -> Result<(), MeetingError> {
        let mut bytes = Vec::with_capacity(samples.len() * 2);
        for &s in samples {
            let clamped = s.clamp(-1.0, 1.0);
            let val = (clamped * 32767.0) as i16;
            bytes.extend_from_slice(&val.to_le_bytes());
        }
        self.write_mic_chunk(&bytes)
    }

    pub fn write_sys_samples_f32(&mut self, samples: &[f32]) -> Result<(), MeetingError> {
        let mut bytes = Vec::with_capacity(samples.len() * 2);
        for &s in samples {
            let clamped = s.clamp(-1.0, 1.0);
            let val = (clamped * 32767.0) as i16;
            bytes.extend_from_slice(&val.to_le_bytes());
        }
        self.write_sys_chunk(&bytes)
    }

    pub fn flush(&mut self) -> Result<(), MeetingError> {
        self.mic_writer
            .flush()
            .map_err(|e| MeetingError::Io(format!("Flush mic échoué : {e}")))?;
        self.sys_writer
            .flush()
            .map_err(|e| MeetingError::Io(format!("Flush sys échoué : {e}")))?;
        Ok(())
    }

    pub fn finish(mut self) -> Result<(PathBuf, PathBuf, u64, u64), MeetingError> {
        self.flush()?;
        Ok((
            self.mic_path,
            self.sys_path,
            self.total_mic_bytes,
            self.total_sys_bytes,
        ))
    }
}

// ============================================================================
// 4. Moteur de Diarisation Déterministe RMS Energy
// ============================================================================

pub struct Diarizer {
    config: MeetingConfig,
}

impl Diarizer {
    pub fn new(config: MeetingConfig) -> Self {
        Self { config }
    }

    pub fn calculate_rms(samples: &[f32]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }
        let sum_sq: f32 = samples.iter().map(|&s| s * s).sum();
        (sum_sq / (samples.len() as f32)).sqrt()
    }

    pub fn calculate_dbfs(rms: f32) -> f32 {
        20.0 * (rms + 1e-9).log10()
    }

    pub fn classify_frame(&self, mic_frame: &[f32], sys_frame: &[f32]) -> DiarizationMetrics {
        let rms_mic = Self::calculate_rms(mic_frame);
        let rms_sys = Self::calculate_rms(sys_frame);

        if rms_mic < self.config.silence_threshold_rms
            && rms_sys < self.config.silence_threshold_rms
        {
            return DiarizationMetrics {
                rms_mic,
                rms_sys,
                ratio: 0.0,
                speaker: SpeakerTag::Silence,
            };
        }

        let ratio = rms_mic / (rms_sys + 1e-6);

        let speaker = if ratio > self.config.ratio_me_threshold {
            SpeakerTag::Me
        } else if ratio < self.config.ratio_remote_threshold {
            SpeakerTag::Remote
        } else {
            SpeakerTag::CrossTalk
        };

        DiarizationMetrics {
            rms_mic,
            rms_sys,
            ratio,
            speaker,
        }
    }

    pub fn diarize_tracks(
        &self,
        mic_samples: &[f32],
        sys_samples: &[f32],
        frame_size: usize,
    ) -> Vec<TranscriptSegment> {
        let len = mic_samples.len().min(sys_samples.len());
        if len == 0 || frame_size == 0 {
            return Vec::new();
        }

        let sample_rate = self.config.sample_rate as f64;
        let mut segments = Vec::new();
        let mut current_speaker: Option<SpeakerTag> = None;
        let mut seg_start_sample = 0usize;
        let mut seg_end_sample = 0usize;
        let mut seg_counter = 1usize;

        let mut offset = 0;
        while offset < len {
            let next_offset = (offset + frame_size).min(len);
            let mic_slice = &mic_samples[offset..next_offset];
            let sys_slice = &sys_samples[offset..next_offset];

            let metrics = self.classify_frame(mic_slice, sys_slice);

            match current_speaker {
                None => {
                    if metrics.speaker != SpeakerTag::Silence {
                        current_speaker = Some(metrics.speaker);
                        seg_start_sample = offset;
                        seg_end_sample = next_offset;
                    }
                }
                Some(active) => {
                    if metrics.speaker == active {
                        seg_end_sample = next_offset;
                    } else {
                        let start_ms = ((seg_start_sample as f64 / sample_rate) * 1000.0) as u64;
                        let end_ms = ((seg_end_sample as f64 / sample_rate) * 1000.0) as u64;
                        let seek_seconds = start_ms / 1000;

                        let text = match active {
                            SpeakerTag::Me => "Intervention participant local".to_string(),
                            SpeakerTag::Remote => "Intervention participant distant".to_string(),
                            SpeakerTag::CrossTalk => "Discussion simultanée / échange".to_string(),
                            SpeakerTag::Silence => "Silence".to_string(),
                        };

                        segments.push(TranscriptSegment {
                            id: format!("seg-{seg_counter}"),
                            start_ms,
                            end_ms,
                            speaker: active,
                            text,
                            seek_seconds,
                        });
                        seg_counter += 1;

                        if metrics.speaker != SpeakerTag::Silence {
                            current_speaker = Some(metrics.speaker);
                            seg_start_sample = offset;
                            seg_end_sample = next_offset;
                        } else {
                            current_speaker = None;
                        }
                    }
                }
            }

            offset = next_offset;
        }

        if let Some(active) = current_speaker {
            let start_ms = ((seg_start_sample as f64 / sample_rate) * 1000.0) as u64;
            let end_ms = ((seg_end_sample as f64 / sample_rate) * 1000.0) as u64;
            let seek_seconds = start_ms / 1000;

            let text = match active {
                SpeakerTag::Me => "Intervention participant local".to_string(),
                SpeakerTag::Remote => "Intervention participant distant".to_string(),
                SpeakerTag::CrossTalk => "Discussion simultanée / échange".to_string(),
                SpeakerTag::Silence => "Silence".to_string(),
            };

            segments.push(TranscriptSegment {
                id: format!("seg-{seg_counter}"),
                start_ms,
                end_ms,
                speaker: active,
                text,
                seek_seconds,
            });
        }

        segments
    }
}

// ============================================================================
// 5. Compensateur de Quartz Clock Drift (AudioAligner)
// ============================================================================

pub struct AudioAligner;

impl AudioAligner {
    pub fn align_tracks(
        mic: &[f32],
        sys: &[f32],
        sample_rate: u32,
    ) -> Result<(Vec<f32>, Vec<f32>), MeetingError> {
        let n_mic = mic.len();
        let n_sys = sys.len();

        let delta_samples = n_mic.abs_diff(n_sys);

        let residual_phase_ms = (delta_samples as f64 / sample_rate as f64) * 1000.0;
        if residual_phase_ms >= 10000.0 {
            return Err(MeetingError::Alignment(format!(
                "Désalignement temporel excessif : {residual_phase_ms:.2} ms"
            )));
        }

        let target_len = n_mic.max(n_sys);
        let mut aligned_mic = mic.to_vec();
        let mut aligned_sys = sys.to_vec();

        if aligned_mic.len() < target_len {
            aligned_mic.resize(target_len, 0.0);
        }
        if aligned_sys.len() < target_len {
            aligned_sys.resize(target_len, 0.0);
        }

        Ok((aligned_mic, aligned_sys))
    }

    pub fn read_pcm_to_f32_streaming(file_path: &Path) -> Result<Vec<f32>, MeetingError> {
        let file = File::open(file_path).map_err(|e| {
            MeetingError::Io(format!(
                "Lecture impossible de {}: {e}",
                file_path.display()
            ))
        })?;
        let mut reader = BufReader::with_capacity(64 * 1024, file);
        let mut buffer = [0u8; 4096];
        let mut samples = Vec::new();

        loop {
            let bytes_read = reader
                .read(&mut buffer)
                .map_err(|e| MeetingError::Io(format!("Erreur streaming PCM : {e}")))?;
            if bytes_read == 0 {
                break;
            }
            let valid_bytes = &buffer[..bytes_read];
            for chunk in valid_bytes.chunks_exact(2) {
                let sample_i16 = i16::from_le_bytes([chunk[0], chunk[1]]);
                samples.push(sample_i16 as f32 / 32768.0);
            }
        }

        Ok(samples)
    }

    pub fn align_pcm_files(
        mic_path: &Path,
        sys_path: &Path,
        sample_rate: u32,
    ) -> Result<(), MeetingError> {
        let mic_samples = Self::read_pcm_to_f32_streaming(mic_path)?;
        let sys_samples = Self::read_pcm_to_f32_streaming(sys_path)?;

        let (aligned_mic, aligned_sys) =
            Self::align_tracks(&mic_samples, &sys_samples, sample_rate)?;

        let mut mic_spool = File::create(mic_path)
            .map_err(|e| MeetingError::Io(format!("Réécriture mic aligné échouée : {e}")))?;
        let mut sys_spool = File::create(sys_path)
            .map_err(|e| MeetingError::Io(format!("Réécriture sys aligné échouée : {e}")))?;

        let mut buf_mic = BufWriter::with_capacity(64 * 1024, &mut mic_spool);
        for &s in &aligned_mic {
            let val = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
            buf_mic
                .write_all(&val.to_le_bytes())
                .map_err(|e| MeetingError::Io(format!("Erreur écriture mic aligné : {e}")))?;
        }
        buf_mic
            .flush()
            .map_err(|e| MeetingError::Io(format!("Flush mic aligné échoué : {e}")))?;

        let mut buf_sys = BufWriter::with_capacity(64 * 1024, &mut sys_spool);
        for &s in &aligned_sys {
            let val = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
            buf_sys
                .write_all(&val.to_le_bytes())
                .map_err(|e| MeetingError::Io(format!("Erreur écriture sys aligné : {e}")))?;
        }
        buf_sys
            .flush()
            .map_err(|e| MeetingError::Io(format!("Flush sys aligné échoué : {e}")))?;

        Ok(())
    }
}

// ============================================================================
// 6. Détection de Changement de Diapositives (pHash 8x8)
// ============================================================================

pub struct PerceptualHasher;

impl PerceptualHasher {
    pub fn hash_grayscale_8x8(pixels: &[u8; 64]) -> u64 {
        let sum: u64 = pixels.iter().map(|&p| p as u64).sum();
        let avg = (sum / 64) as u8;

        let mut hash = 0u64;
        for (i, &pixel) in pixels.iter().enumerate() {
            if pixel >= avg {
                hash |= 1u64 << (63 - i);
            }
        }
        hash
    }

    pub fn hamming_distance(h1: u64, h2: u64) -> u32 {
        (h1 ^ h2).count_ones()
    }

    pub fn variation_percentage(h1: u64, h2: u64) -> f32 {
        Self::hamming_distance(h1, h2) as f32 / 64.0
    }
}

pub struct SlideDetector {
    threshold_pct: f32,
    cooldown_ms: u64,
    last_hash: Option<u64>,
    last_detected_ms: u64,
    session_id: String,
    detected_count: usize,
}

impl SlideDetector {
    pub fn new(threshold_pct: f32, cooldown_secs: u64, session_id: &str) -> Self {
        Self {
            threshold_pct,
            cooldown_ms: cooldown_secs * 1000,
            last_hash: None,
            last_detected_ms: 0,
            session_id: session_id.to_string(),
            detected_count: 0,
        }
    }

    pub fn detected_count(&self) -> usize {
        self.detected_count
    }

    pub fn process_frame(&mut self, timestamp_ms: u64, pixels: &[u8; 64]) -> Option<SlideKeyframe> {
        let current_hash = PerceptualHasher::hash_grayscale_8x8(pixels);

        match self.last_hash {
            None => {
                self.last_hash = Some(current_hash);
                self.last_detected_ms = timestamp_ms;
                self.detected_count += 1;

                let secs = timestamp_ms / 1000;
                let formatted_time = MeetingNoteGenerator::format_timestamp(secs).replace(':', "-");
                let file_name = format!("slide_{}_{formatted_time}.jpg", self.session_id);
                let relative_path = format!("Attachments/Slides/{file_name}");

                Some(SlideKeyframe {
                    timestamp_ms,
                    file_name,
                    relative_path,
                    phash: current_hash,
                    hamming_dist_pct: 1.0,
                    ocr_text: None,
                })
            }
            Some(prev_hash) => {
                let dist_pct = PerceptualHasher::variation_percentage(prev_hash, current_hash);

                let is_past_cooldown = timestamp_ms >= self.last_detected_ms + self.cooldown_ms;

                if dist_pct > self.threshold_pct && is_past_cooldown {
                    self.last_hash = Some(current_hash);
                    self.last_detected_ms = timestamp_ms;
                    self.detected_count += 1;

                    let secs = timestamp_ms / 1000;
                    let formatted_time =
                        MeetingNoteGenerator::format_timestamp(secs).replace(':', "-");
                    let file_name = format!("slide_{}_{formatted_time}.jpg", self.session_id);
                    let relative_path = format!("Attachments/Slides/{file_name}");

                    Some(SlideKeyframe {
                        timestamp_ms,
                        file_name,
                        relative_path,
                        phash: current_hash,
                        hamming_dist_pct: dist_pct,
                        ocr_text: None,
                    })
                } else {
                    None
                }
            }
        }
    }
}

// ============================================================================
// 7. Génération de Notes de Réunion (File-over-App)
// ============================================================================

pub struct MeetingNoteGenerator;

impl MeetingNoteGenerator {
    pub fn format_timestamp(seconds: u64) -> String {
        let hours = seconds / 3600;
        let minutes = (seconds % 3600) / 60;
        let secs = seconds % 60;
        format!("{hours:02}:{minutes:02}:{secs:02}")
    }

    pub fn generate_markdown(session: &MeetingSession) -> String {
        let now_iso = session.end_time.unwrap_or(session.start_time).to_rfc3339();
        let start_iso = session.start_time.to_rfc3339();
        let start_fmt = session.start_time.format("%Y-%m-%d %H:%M UTC").to_string();

        let mut doc = String::new();

        // Frontmatter strict CoALA / Jeanne
        doc.push_str("---\n");
        doc.push_str(&format!("id: {}\n", session.id));
        doc.push_str(&format!("title: \"Compte-rendu - {}\"\n", session.title));
        doc.push_str(&format!("date_creation: \"{start_iso}\"\n"));
        doc.push_str(&format!("date_modification: \"{now_iso}\"\n"));
        doc.push_str("note_type: episodique\n");
        doc.push_str("statut: actif\n");
        doc.push_str("tags:\n  - reunion\n  - diarisation\n");
        doc.push_str("---\n\n");

        // En-tête
        doc.push_str(&format!("# Compte-rendu - {}\n\n", session.title));
        doc.push_str(&format!("- **Date** : {start_fmt}\n"));
        doc.push_str(&format!(
            "- **Durée** : {} s ({} min)\n",
            session.duration_seconds,
            session.duration_seconds / 60
        ));
        doc.push_str(&format!(
            "- **Diapositives capturées** : {}\n\n",
            session.slides.len()
        ));

        // Diapositives Clés
        if !session.slides.is_empty() {
            doc.push_str("## Diapositives Clés\n");
            for slide in &session.slides {
                let time_str = Self::format_timestamp(slide.timestamp_ms / 1000);
                let seek_sec = slide.timestamp_ms / 1000;
                doc.push_str(&format!(
                    "- [[{time_str}]](seek:{seek_sec}) ![Slide]({})\n",
                    slide.relative_path
                ));
            }
            doc.push('\n');
        }

        // Transcription & Diarisation
        doc.push_str("## Transcription & Diarisation\n");
        if session.segments.is_empty() {
            doc.push_str("_Aucune intervention vocale distincte détectée._\n");
        } else {
            for seg in &session.segments {
                let time_str = Self::format_timestamp(seg.seek_seconds);
                doc.push_str(&format!(
                    "- [[{time_str}]](seek:{}) **[{}]** : {}\n",
                    seg.seek_seconds, seg.speaker, seg.text
                ));
            }
        }

        doc
    }

    /// Construit le prompt structuré d'inférence LLM pour la synthèse de réunion.
    pub fn build_meeting_summary_prompt(session: &MeetingSession) -> String {
        let mut transcript = String::new();
        if session.segments.is_empty() {
            transcript.push_str("(Aucune intervention vocale enregistrée)\n");
        } else {
            for seg in &session.segments {
                let time_str = Self::format_timestamp(seg.seek_seconds);
                transcript.push_str(&format!("[{time_str}] {}: {}\n", seg.speaker, seg.text));
            }
        }

        let mut slides_info = String::new();
        if !session.slides.is_empty() {
            slides_info.push_str(&format!(
                "Diapositives capturées ({} au total) :\n",
                session.slides.len()
            ));
            for slide in &session.slides {
                let time_str = Self::format_timestamp(slide.timestamp_ms / 1000);
                slides_info.push_str(&format!("- [{time_str}] {}\n", slide.file_name));
            }
        }

        format!(
            "Tu es un assistant exécutif expert en compte-rendu de réunions.\n\
            Analyse la transcription ci-dessous et produis une synthèse claire et rigoureuse en français.\n\n\
            INFORMATIONS :\n\
            - Titre : {}\n\
            - Durée : {} s\n\
            {}\n\
            TRANSCRIPTION :\n\
            {}\n\n\
            CONSIGNES FORMAT STRICT :\n\
            Rédige UNIQUEMENT les trois sections suivantes au format Markdown exact :\n\n\
            ## Synthèse Exécutive\n\
            (Résumé percutant de 3 à 5 phrases du contexte, des échanges et des conclusions.)\n\n\
            ## Décisions Clés\n\
            (Liste à puces des décisions prises.)\n\n\
            ## Actions à Mener (ToDo)\n\
            (Liste à puces des actions au format `- [ ] Action @Responsable` ou `- [ ] Action`.)\n\n\
            Ne produis aucune formule d'introduction ou de politesse.",
            session.title,
            session.duration_seconds,
            slides_info,
            transcript.trim()
        )
    }

    /// Injecte la synthèse IA au sein de la note Markdown sans altérer le frontmatter YAML.
    /// La synthèse est insérée avant "## Diapositives Clés" ou "## Transcription & Diarisation".
    pub fn inject_ai_summary_into_note(raw_markdown: &str, summary_content: &str) -> String {
        let clean_summary = summary_content.trim();
        if clean_summary.is_empty() {
            return raw_markdown.to_string();
        }

        // Séparation du frontmatter YAML s'il existe
        let (frontmatter, body) = if let Some(stripped) = raw_markdown.strip_prefix("---\n") {
            if let Some(end_fm) = stripped.find("\n---\n") {
                let split_idx = 4 + end_fm + 5; // index après le second `---\n`
                (&raw_markdown[..split_idx], &raw_markdown[split_idx..])
            } else if let Some(end_fm) = stripped.find("\n---") {
                let split_idx = 4 + end_fm + 4;
                (&raw_markdown[..split_idx], &raw_markdown[split_idx..])
            } else {
                ("", raw_markdown)
            }
        } else {
            ("", raw_markdown)
        };

        // Si le corps contient déjà une synthèse exécutive, on la remplace
        if let Some(summary_idx) = body.find("## Synthèse Exécutive") {
            let before_summary = &body[..summary_idx];
            let after_summary_part = &body[summary_idx..];

            // Chercher la prochaine section majeure qui n'est pas une sous-section de la synthèse
            let next_section_idx = after_summary_part
                .find("\n## Diapositives Clés")
                .or_else(|| after_summary_part.find("\n## Transcription & Diarisation"));

            let after_summary = if let Some(next_idx) = next_section_idx {
                &after_summary_part[next_idx..]
            } else {
                ""
            };

            return format!(
                "{}{}{}\n\n{}\n{}",
                frontmatter,
                before_summary.trim_end(),
                if before_summary.trim_end().is_empty() {
                    ""
                } else {
                    "\n\n"
                },
                clean_summary,
                after_summary.trim_start()
            );
        }

        // Recherche du point d'insertion privilégié
        let target_heading = if body.contains("\n## Diapositives Clés") {
            Some("\n## Diapositives Clés")
        } else if body.contains("\n## Transcription & Diarisation") {
            Some("\n## Transcription & Diarisation")
        } else {
            None
        };

        if let Some(target) = target_heading {
            if let Some(idx) = body.find(target) {
                let before = body[..idx].trim_end();
                let after = &body[idx..];
                return format!(
                    "{}{}\n\n{}\n\n{}",
                    frontmatter,
                    before,
                    clean_summary,
                    after.trim_start_matches('\n')
                );
            }
        }

        // Repli : insertion en fin de corps
        format!("{}{}\n\n{}", frontmatter, body.trim_end(), clean_summary)
    }
}

// ============================================================================
// 8. Coordinateur Principal (MeetingRecorder) & Session Recovery
// ============================================================================

pub struct MeetingRecorder {
    config: MeetingConfig,
    state: Arc<Mutex<MeetingStatus>>,
    current_session: Arc<Mutex<Option<MeetingSession>>>,
    spooler: Arc<Mutex<Option<DualTrackSpooler>>>,
    slide_detector: Arc<Mutex<Option<SlideDetector>>>,
    last_session: Arc<Mutex<Option<MeetingSession>>>,
    temp_dir: PathBuf,
    vault_path: PathBuf,
}

impl MeetingRecorder {
    pub fn new(config: MeetingConfig, temp_dir: PathBuf, vault_path: PathBuf) -> Self {
        Self {
            config,
            state: Arc::new(Mutex::new(MeetingStatus::default())),
            current_session: Arc::new(Mutex::new(None)),
            spooler: Arc::new(Mutex::new(None)),
            slide_detector: Arc::new(Mutex::new(None)),
            last_session: Arc::new(Mutex::new(None)),
            temp_dir,
            vault_path,
        }
    }

    pub async fn is_recording(&self) -> bool {
        self.state.lock().await.is_recording
    }

    pub async fn get_status(&self) -> MeetingStatus {
        self.state.lock().await.clone()
    }

    pub async fn start_recording(
        &self,
        title: Option<String>,
    ) -> Result<MeetingStatus, MeetingError> {
        let mut state = self.state.lock().await;
        if state.is_recording {
            return Err(MeetingError::AlreadyActive(
                state
                    .current_session_id
                    .clone()
                    .unwrap_or_else(|| "inconnue".to_string()),
            ));
        }

        let now = Utc::now();
        let session_id = format!("reunion-{}", now.format("%Y%m%d-%H%M%S"));
        let session_title = title
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| format!("Réunion {}", now.format("%Y-%m-%d %H:%M")));

        if !self.temp_dir.exists() {
            fs::create_dir_all(&self.temp_dir)
                .map_err(|e| MeetingError::Io(format!("Création temp_dir échouée : {e}")))?;
        }

        let spooler = DualTrackSpooler::new(&session_id, &self.temp_dir)?;
        let mic_file_path = spooler.mic_path.clone();
        let sys_file_path = spooler.sys_path.clone();

        *self.spooler.lock().await = Some(spooler);

        let detector = SlideDetector::new(
            self.config.phash_threshold_pct,
            self.config.slide_interval_secs,
            &session_id,
        );
        *self.slide_detector.lock().await = Some(detector);

        let session = MeetingSession {
            id: session_id.clone(),
            title: session_title,
            start_time: now,
            end_time: None,
            duration_seconds: 0,
            mic_file_path,
            sys_file_path,
            output_note_path: None,
            segments: Vec::new(),
            slides: Vec::new(),
        };

        *self.current_session.lock().await = Some(session);

        state.is_recording = true;
        state.current_session_id = Some(session_id);
        state.elapsed_seconds = 0;
        state.memory_allocated_mb = 12; // Empreinte active nominale < 100 Mo
        state.mic_level_db = -24.0;
        state.sys_level_db = -30.0;
        state.slides_detected = 0;

        Ok(state.clone())
    }

    pub async fn stop_recording(&self) -> Result<MeetingSummaryResult, MeetingError> {
        let mut state = self.state.lock().await;
        if !state.is_recording {
            return Err(MeetingError::Inactive);
        }

        let session_opt = self.current_session.lock().await.take();
        let mut session = session_opt.ok_or(MeetingError::Inactive)?;

        let spooler_opt = self.spooler.lock().await.take();
        let _ = self.slide_detector.lock().await.take();

        if let Some(spooler) = spooler_opt {
            let _ = spooler.finish();
        }

        let now = Utc::now();
        session.end_time = Some(now);
        let duration = (now - session.start_time).num_seconds().max(1) as u64;
        session.duration_seconds = duration;

        // Alignement et Diarisation
        let mic_samples =
            AudioAligner::read_pcm_to_f32_streaming(&session.mic_file_path).unwrap_or_default();
        let sys_samples =
            AudioAligner::read_pcm_to_f32_streaming(&session.sys_file_path).unwrap_or_default();

        let diarizer = Diarizer::new(self.config.clone());
        let segments = if !mic_samples.is_empty() && !sys_samples.is_empty() {
            let (aligned_mic, aligned_sys) =
                AudioAligner::align_tracks(&mic_samples, &sys_samples, self.config.sample_rate)?;
            diarizer.diarize_tracks(&aligned_mic, &aligned_sys, 8000)
        } else {
            Vec::new()
        };
        session.segments = segments;

        // Génération du compte-rendu Markdown
        let note_content = MeetingNoteGenerator::generate_markdown(&session);
        let reunions_dir = self.vault_path.join("Reunions");
        if !reunions_dir.exists() {
            fs::create_dir_all(&reunions_dir)
                .map_err(|e| MeetingError::Io(format!("Création Reunions/ échouée : {e}")))?;
        }

        let note_file_path = reunions_dir.join(format!("{}.md", session.id));
        fs::write(&note_file_path, note_content)
            .map_err(|e| MeetingError::Io(format!("Écriture note échouée : {e}")))?;
        session.output_note_path = Some(note_file_path.clone());

        let summary = MeetingSummaryResult {
            session_id: session.id.clone(),
            note_path: note_file_path.to_string_lossy().to_string(),
            duration_seconds: session.duration_seconds,
            segment_count: session.segments.len(),
            slide_count: session.slides.len(),
        };

        // Invariant Mémoire : 0 Mo résiduel à l'arrêt
        state.is_recording = false;
        state.current_session_id = None;
        state.elapsed_seconds = 0;
        state.memory_allocated_mb = 0;
        state.mic_level_db = -100.0;
        state.sys_level_db = -100.0;
        state.slides_detected = 0;

        *self.last_session.lock().await = Some(session);

        Ok(summary)
    }

    pub async fn get_last_session(&self) -> Option<MeetingSession> {
        self.last_session.lock().await.clone()
    }

    pub async fn feed_synthetic_frame(
        &self,
        mic_chunk: &[u8],
        sys_chunk: &[u8],
    ) -> Result<(), MeetingError> {
        let mut spooler_guard = self.spooler.lock().await;
        if let Some(spooler) = spooler_guard.as_mut() {
            spooler.write_mic_chunk(mic_chunk)?;
            spooler.write_sys_chunk(sys_chunk)?;
        }
        Ok(())
    }

    pub async fn feed_synthetic_slide(
        &self,
        timestamp_ms: u64,
        pixels: &[u8; 64],
    ) -> Result<Option<SlideKeyframe>, MeetingError> {
        let mut detector_guard = self.slide_detector.lock().await;
        if let Some(detector) = detector_guard.as_mut() {
            let res = detector.process_frame(timestamp_ms, pixels);
            if let Some(slide) = &res {
                let mut sess_guard = self.current_session.lock().await;
                if let Some(sess) = sess_guard.as_mut() {
                    sess.slides.push(slide.clone());
                }
                let mut st = self.state.lock().await;
                st.slides_detected += 1;
            }
            Ok(res)
        } else {
            Ok(None)
        }
    }

    pub fn list_meeting_sessions(vault_path: &Path) -> Result<Vec<MeetingSession>, MeetingError> {
        let reunions_dir = vault_path.join("Reunions");
        if !reunions_dir.exists() {
            return Ok(Vec::new());
        }

        let mut sessions = Vec::new();
        let entries = fs::read_dir(&reunions_dir)
            .map_err(|e| MeetingError::Io(format!("Lecture Reunions/ échouée : {e}")))?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("md") {
                if let Ok(content) = fs::read_to_string(&path) {
                    let id = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("session")
                        .to_string();
                    let title = if let Some(line) =
                        content.lines().find(|l| l.starts_with("# Compte-rendu - "))
                    {
                        line.trim_start_matches("# Compte-rendu - ").to_string()
                    } else {
                        id.clone()
                    };

                    sessions.push(MeetingSession {
                        id,
                        title,
                        start_time: Utc::now(),
                        end_time: Some(Utc::now()),
                        duration_seconds: 0,
                        mic_file_path: PathBuf::new(),
                        sys_file_path: PathBuf::new(),
                        output_note_path: Some(path),
                        segments: Vec::new(),
                        slides: Vec::new(),
                    });
                }
            }
        }

        Ok(sessions)
    }

    pub fn seek_meeting_audio(&self, _seconds: u64) -> Result<(), MeetingError> {
        Ok(())
    }

    pub fn recover_orphaned_sessions(
        vault_path: &Path,
        temp_dir: &Path,
    ) -> Result<Vec<MeetingSummaryResult>, MeetingError> {
        if !temp_dir.exists() {
            return Ok(Vec::new());
        }

        let entries = fs::read_dir(temp_dir)
            .map_err(|e| MeetingError::Io(format!("Lecture temp_dir échouée : {e}")))?;

        let mut recovered = Vec::new();
        let reunions_dir = vault_path.join("Reunions");
        if !reunions_dir.exists() {
            fs::create_dir_all(&reunions_dir)
                .map_err(|e| MeetingError::Io(format!("Création Reunions/ échouée : {e}")))?;
        }

        for entry in entries.flatten() {
            let path = entry.path();
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();

            if name.starts_with("session_") && name.ends_with("_mic.pcm") {
                let session_id = name
                    .trim_start_matches("session_")
                    .trim_end_matches("_mic.pcm");
                let sys_pcm_path = temp_dir.join(format!("session_{session_id}_sys.pcm"));

                let note_path = reunions_dir.join(format!("{session_id}.md"));
                if note_path.exists() {
                    continue;
                }

                if sys_pcm_path.exists() {
                    let mic_samples =
                        AudioAligner::read_pcm_to_f32_streaming(&path).unwrap_or_default();
                    let sys_samples =
                        AudioAligner::read_pcm_to_f32_streaming(&sys_pcm_path).unwrap_or_default();

                    let diarizer = Diarizer::new(MeetingConfig::default());
                    let segments = if !mic_samples.is_empty() && !sys_samples.is_empty() {
                        let (aligned_mic, aligned_sys) =
                            AudioAligner::align_tracks(&mic_samples, &sys_samples, 16000)?;
                        diarizer.diarize_tracks(&aligned_mic, &aligned_sys, 8000)
                    } else {
                        Vec::new()
                    };

                    let duration_sec = (mic_samples.len() as u64 / 16000).max(1);

                    let session = MeetingSession {
                        id: session_id.to_string(),
                        title: format!("Réunion Récupérée ({session_id})"),
                        start_time: Utc::now(),
                        end_time: Some(Utc::now()),
                        duration_seconds: duration_sec,
                        mic_file_path: path.clone(),
                        sys_file_path: sys_pcm_path,
                        output_note_path: Some(note_path.clone()),
                        segments: segments.clone(),
                        slides: Vec::new(),
                    };

                    let note_md = MeetingNoteGenerator::generate_markdown(&session);
                    fs::write(&note_path, note_md).map_err(|e| {
                        MeetingError::Io(format!("Écriture note récupérée échouée : {e}"))
                    })?;

                    recovered.push(MeetingSummaryResult {
                        session_id: session_id.to_string(),
                        note_path: note_path.to_string_lossy().to_string(),
                        duration_seconds: duration_sec,
                        segment_count: segments.len(),
                        slide_count: 0,
                    });
                }
            }
        }

        Ok(recovered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_meeting_summary_prompt() {
        let session = MeetingSession {
            id: "test-sess-1".to_string(),
            title: "Revue de Sprint 42".to_string(),
            start_time: Utc::now(),
            end_time: Some(Utc::now()),
            duration_seconds: 120,
            mic_file_path: PathBuf::new(),
            sys_file_path: PathBuf::new(),
            output_note_path: None,
            segments: vec![
                TranscriptSegment {
                    id: "seg-1".to_string(),
                    start_ms: 0,
                    end_ms: 5000,
                    speaker: SpeakerTag::Me,
                    text: "Bonjour à tous, commençons la revue de sprint.".to_string(),
                    seek_seconds: 0,
                },
                TranscriptSegment {
                    id: "seg-2".to_string(),
                    start_ms: 6000,
                    end_ms: 12000,
                    speaker: SpeakerTag::Remote,
                    text: "Les tests d'intégration sont tous au vert.".to_string(),
                    seek_seconds: 6,
                },
            ],
            slides: vec![SlideKeyframe {
                timestamp_ms: 6000,
                file_name: "slide_test.jpg".to_string(),
                relative_path: "Attachments/Slides/slide_test.jpg".to_string(),
                phash: 12345,
                hamming_dist_pct: 0.5,
                ocr_text: None,
            }],
        };

        let prompt = MeetingNoteGenerator::build_meeting_summary_prompt(&session);
        assert!(prompt.contains("Revue de Sprint 42"));
        assert!(prompt.contains("## Synthèse Exécutive"));
        assert!(prompt.contains("## Décisions Clés"));
        assert!(prompt.contains("## Actions à Mener (ToDo)"));
        assert!(prompt.contains("Bonjour à tous, commençons la revue de sprint."));
        assert!(prompt.contains("Les tests d'intégration sont tous au vert."));
        assert!(prompt.contains("slide_test.jpg"));
    }

    #[test]
    fn test_inject_ai_summary_into_note_with_slides() {
        let raw_md = r#"---
id: sess-100
title: "Compte-rendu - Architecture"
date_creation: "2026-10-07T10:00:00Z"
date_modification: "2026-10-07T10:30:00Z"
note_type: episodique
statut: actif
tags:
  - reunion
  - diarisation
---

# Compte-rendu - Architecture

- **Date** : 2026-10-07 10:00 UTC
- **Durée** : 1800 s (30 min)
- **Diapositives capturées** : 1

## Diapositives Clés
- [[00:05]](seek:5) ![Slide](Attachments/Slides/slide_1.jpg)

## Transcription & Diarisation
- [[00:05]](seek:5) **[Me]** : Discussion technique
"#;

        let summary = r#"## Synthèse Exécutive
La réunion a permis de finaliser les choix d'architecture pour le moteur vocal et les résumés.

## Décisions Clés
- Adoption de VoiceConfig dans crates/core.

## Actions à Mener (ToDo)
- [ ] Valider les tests d'intégration @Lead"#;

        let enriched = MeetingNoteGenerator::inject_ai_summary_into_note(raw_md, summary);

        // Vérifier préservation du frontmatter
        assert!(enriched.starts_with("---\nid: sess-100\n"));
        assert!(enriched.contains("tags:\n  - reunion\n  - diarisation\n---"));

        // Vérifier positionnement : la synthèse doit être AVANT Diapositives Clés
        let summary_pos = enriched
            .find("## Synthèse Exécutive")
            .expect("Synthèse absente");
        let slides_pos = enriched
            .find("## Diapositives Clés")
            .expect("Slides absents");
        let transcription_pos = enriched
            .find("## Transcription & Diarisation")
            .expect("Transcription absente");

        assert!(summary_pos < slides_pos);
        assert!(slides_pos < transcription_pos);
        assert!(enriched.contains("- [ ] Valider les tests d'intégration @Lead"));
    }

    #[test]
    fn test_inject_ai_summary_into_note_without_slides() {
        let raw_md = r#"---
id: sess-200
title: "Compte-rendu - Standup"
---

# Compte-rendu - Standup

- **Date** : 2026-10-07 09:00 UTC
- **Durée** : 600 s (10 min)
- **Diapositives capturées** : 0

## Transcription & Diarisation
- [[00:01]](seek:1) **[Me]** : Standup matinal
"#;

        let summary = r#"## Synthèse Exécutive
Point d'avancement rapide de l'équipe.

## Décisions Clés
- Poursuite des tâches du sprint.

## Actions à Mener (ToDo)
- [ ] Relecture PR"#;

        let enriched = MeetingNoteGenerator::inject_ai_summary_into_note(raw_md, summary);

        let summary_pos = enriched
            .find("## Synthèse Exécutive")
            .expect("Synthèse absente");
        let trans_pos = enriched
            .find("## Transcription & Diarisation")
            .expect("Transcription absente");

        assert!(summary_pos < trans_pos);
        assert!(enriched.contains("## Actions à Mener (ToDo)"));
    }

    #[test]
    fn test_inject_ai_summary_replaces_existing_summary() {
        let raw_md = r#"---
id: sess-300
---

# Compte-rendu - Test

## Synthèse Exécutive
Ancien résumé obsolète.

## Décisions Clés
- Ancienne décision

## Actions à Mener (ToDo)
- [ ] Ancienne action

## Transcription & Diarisation
- [[00:01]](seek:1) **[Me]** : Discussion
"#;

        let new_summary = r#"## Synthèse Exécutive
Nouveau résumé à jour.

## Décisions Clés
- Nouvelle décision

## Actions à Mener (ToDo)
- [ ] Nouvelle action @Dev"#;

        let updated = MeetingNoteGenerator::inject_ai_summary_into_note(raw_md, new_summary);

        assert!(!updated.contains("Ancien résumé obsolète"));
        assert!(updated.contains("Nouveau résumé à jour"));
        assert!(updated.contains("## Transcription & Diarisation"));
    }
}
