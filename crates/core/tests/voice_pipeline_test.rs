use jeanne_core::voice::{
    AudioResampler, PiperTtsEngine, SentenceSplitter, VadConfig, VadDecision,
    VoiceActivityDetector, VoiceError, VoicePipeline, VoiceState, WhisperSttEngine,
    get_audio_devices,
};
use std::f32::consts::PI;
use std::sync::Arc;

/// TEST-05-01: Rééchantillonnage 48 kHz vers 16 kHz
#[test]
fn test_05_01_resampling_48k_to_16k() {
    let input_rate = 48000;
    let target_rate = 16000;
    let frequency = 440.0;
    let samples: Vec<f32> = (0..48000)
        .map(|i| (2.0 * PI * frequency * (i as f32) / input_rate as f32).sin())
        .collect();

    let resampled = AudioResampler::resample_buffer_mono(&samples, input_rate, target_rate)
        .expect("resample_buffer_mono 48k->16k failed");

    let diff = (resampled.len() as i64 - target_rate as i64).abs();
    assert!(
        diff <= 15,
        "Attendu 16000 +- 15 échantillons, obtenu {}",
        resampled.len()
    );
    assert!(!resampled.is_empty());
    assert!(resampled.iter().any(|&s| s.abs() > 0.1));
}

/// TEST-05-02: Rééchantillonnage 44.1 kHz vers 16 kHz
#[test]
fn test_05_02_resampling_44_1k_to_16k() {
    let input_rate = 44100;
    let target_rate = 16000;
    let frequency = 440.0;
    let samples: Vec<f32> = (0..44100)
        .map(|i| (2.0 * PI * frequency * (i as f32) / input_rate as f32).sin())
        .collect();

    let resampled = AudioResampler::resample_buffer_mono(&samples, input_rate, target_rate)
        .expect("resample_buffer_mono 44.1k->16k failed");

    let diff = (resampled.len() as i64 - target_rate as i64).abs();
    assert!(
        diff <= 15,
        "Attendu 16000 +- 15 échantillons, obtenu {}",
        resampled.len()
    );
    assert!(!resampled.is_empty());
}

/// TEST-05-03: Downmix stéréo entrelacé vers mono
#[test]
fn test_05_03_interleaved_stereo_downmix() {
    let mut resampler = AudioResampler::new(48000, 2).expect("resampler creation");
    let frames = 48000;
    let mut stereo_interleaved = Vec::with_capacity(frames * 2);
    for _ in 0..frames {
        stereo_interleaved.push(0.6f32);
        stereo_interleaved.push(0.2f32);
    }

    let mono_resampled = resampler
        .process_interleaved_chunk(&stereo_interleaved)
        .expect("process_interleaved_chunk");

    let diff = (mono_resampled.len() as i64 - 16000).abs();
    assert!(
        diff <= 15,
        "Longueur attendue ~16000, obtenu {}",
        mono_resampled.len()
    );

    let avg: f32 = mono_resampled.iter().sum::<f32>() / mono_resampled.len() as f32;
    assert!(
        (avg - 0.4).abs() < 0.05,
        "Downmix moyen attendu ~0.4, obtenu {avg}"
    );
}

/// TEST-05-04: Détection de silence VAD (seuil 700 ms)
#[test]
fn test_05_04_vad_silence_detection() {
    let config = VadConfig {
        energy_threshold: 0.015,
        silence_timeout_ms: 700,
        min_speech_duration_ms: 250,
    };
    let mut vad = VoiceActivityDetector::new(config, 16000);

    let speech_frame = vec![0.1f32; 320];
    let d1 = vad.process_frame(&speech_frame);
    assert_eq!(d1, VadDecision::SpeechOngoing);
    assert!(vad.is_speaking());

    let silence_frame = vec![0.0f32; 320];
    let mut decisions = Vec::new();
    // 35 frames de 20 ms = 700 ms (11200 échantillons). La 35e frame déclenche SpeechEnded.
    for _ in 0..36 {
        decisions.push(vad.process_frame(&silence_frame));
    }

    assert!(decisions.contains(&VadDecision::SpeechEnded));
    assert_eq!(decisions[34], VadDecision::SpeechEnded);
    assert_eq!(*decisions.last().unwrap(), VadDecision::Silence);
    assert!(!vad.is_speaking());
}

/// TEST-05-05: Détection du début de parole VAD (Speech Onset)
#[test]
fn test_05_05_vad_speech_onset_detection() {
    let mut vad = VoiceActivityDetector::new(VadConfig::default(), 16000);
    assert!(!vad.is_speaking());

    let speech_frame = vec![0.08f32; 320];
    let decision = vad.process_frame(&speech_frame);
    assert_eq!(decision, VadDecision::SpeechOngoing);
    assert!(vad.is_speaking());
}

/// TEST-05-06: Découpage par ponctuation SentenceSplitter
#[test]
fn test_05_06_sentence_splitter_punctuation() {
    let mut splitter = SentenceSplitter::new();
    let tokens = ["Hello", " world", ".", " How", " are", " you", "?"];
    let mut emitted = Vec::new();

    for token in tokens {
        emitted.extend(splitter.push_token(token));
    }

    assert_eq!(emitted, vec!["Hello world.", "How are you?"]);
}

/// TEST-05-07: Flush du texte résiduel sans ponctuation finale
#[test]
fn test_05_07_sentence_splitter_trailing_flush() {
    let mut splitter = SentenceSplitter::new();
    let tokens = ["Final", " thought", " without", " dot"];

    for token in tokens {
        let _ = splitter.push_token(token);
    }

    let flushed = splitter.flush();
    assert_eq!(flushed, Some("Final thought without dot".to_string()));
}

/// TEST-05-08: Gestion des abréviations dans SentenceSplitter
#[test]
fn test_05_08_sentence_splitter_abbreviation_handling() {
    let mut splitter = SentenceSplitter::new();
    let tokens = ["e.g.", " this", " is", " a", " test."];
    let mut emitted = Vec::new();

    for token in tokens {
        emitted.extend(splitter.push_token(token));
    }

    assert_eq!(emitted, vec!["e.g. this is a test."]);
}

/// TEST-05-09: Zéro fuite mémoire lors de la désactivation (Voice OFF)
#[tokio::test]
async fn test_05_09_zero_memory_leak_on_voice_off() {
    let stt = Arc::new(WhisperSttEngine::new(None));
    let tts = Arc::new(PiperTtsEngine::new(None));
    let pipeline = VoicePipeline::new(VadConfig::default(), stt, tts);

    pipeline.start().await.expect("start");
    assert!(pipeline.is_active().await);

    pipeline.stop().await.expect("stop");
    let status = pipeline.get_status().await;

    assert!(!status.is_active);
    assert_eq!(status.memory_allocated_mb, 0);
    assert_eq!(status.state, VoiceState::Idle);
}

/// TEST-05-10: Pipeline de transcription STT
#[tokio::test]
async fn test_05_10_stt_transcription_pipeline() {
    let stt = Arc::new(WhisperSttEngine::new(None));
    let tts = Arc::new(PiperTtsEngine::new(None));
    let pipeline = VoicePipeline::new(VadConfig::default(), stt, tts);

    pipeline.start().await.expect("start");
    let synthetic_audio = vec![0.1f32; 16000];
    let transcription = pipeline
        .transcribe_buffer(&synthetic_audio, 16000)
        .await
        .expect("transcribe");

    assert!(!transcription.is_empty());
}

/// TEST-05-11: Latence TTFB TTS incrémental < 800 ms
#[tokio::test]
async fn test_05_11_incremental_tts_ttfb_latency() {
    let stt = Arc::new(WhisperSttEngine::new(None));
    let tts = Arc::new(PiperTtsEngine::new(None));
    let pipeline = VoicePipeline::new(VadConfig::default(), stt, tts);

    pipeline.start().await.expect("start");
    let text = "Première phrase courte. Deuxième phrase de synthèse. Troisième phrase finale.";
    let audio = pipeline.synthesize_speech(text).await.expect("synthesize");

    assert!(!audio.is_empty());
    let status = pipeline.get_status().await;
    assert!(
        status.last_synthesis_ttfb_ms < 800,
        "TTFB attendu < 800 ms, obtenu {} ms",
        status.last_synthesis_ttfb_ms
    );
}

/// TEST-05-12: Découverte des périphériques audio (cpal)
#[test]
fn test_05_12_audio_device_discovery() {
    let report = get_audio_devices();
    assert!(!report.input_devices.is_empty());
    assert!(!report.output_devices.is_empty());
    assert!(report.default_input_name.is_some());
    assert!(report.default_output_name.is_some());
}

/// TEST-05-13: Rejet des requêtes lorsque le pipeline est inactif
#[tokio::test]
async fn test_05_13_inactive_pipeline_rejection() {
    let stt = Arc::new(WhisperSttEngine::new(None));
    let tts = Arc::new(PiperTtsEngine::new(None));
    let pipeline = VoicePipeline::new(VadConfig::default(), stt, tts);

    let transcribe_res = pipeline.transcribe_buffer(&[0.1; 1600], 16000).await;
    match transcribe_res {
        Err(VoiceError::Inactive) => {}
        other => panic!("Attendu VoiceError::Inactive, obtenu {:?}", other),
    }

    let synth_res = pipeline.synthesize_speech("Test").await;
    match synth_res {
        Err(VoiceError::Inactive) => {}
        other => panic!("Attendu VoiceError::Inactive, obtenu {:?}", other),
    }
}

/// TEST-05-14: Idempotence des transitions d'état du pipeline
#[tokio::test]
async fn test_05_14_pipeline_state_idempotence_and_reset() {
    let stt = Arc::new(WhisperSttEngine::new(None));
    let tts = Arc::new(PiperTtsEngine::new(None));
    let pipeline = VoicePipeline::new(VadConfig::default(), stt, tts);

    pipeline.start().await.expect("start 1");
    pipeline.start().await.expect("start 2");
    pipeline.stop().await.expect("stop 1");
    pipeline.stop().await.expect("stop 2");
    pipeline.start().await.expect("start 3");
    pipeline.stop().await.expect("stop 3");

    let status = pipeline.get_status().await;
    assert!(!status.is_active);
    assert_eq!(status.memory_allocated_mb, 0);
}
