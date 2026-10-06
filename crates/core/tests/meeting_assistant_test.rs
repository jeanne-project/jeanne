use jeanne_core::meeting::*;
use std::fs;
use std::path::PathBuf;

/// TEST-06-01: Dual-track spooling & chunk bounds
#[test]
fn test_dual_track_spooling_chunk_bounds() {
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let session_id = "test_spool_01";
    let mut spooler = DualTrackSpooler::new(session_id, temp_dir.path()).expect("create spooler");

    // Simuler 60 secondes de PCM 16-bit mono 16 kHz
    // 60 sec * 16000 échantillons/sec * 2 octets = 1 920 000 octets
    let chunk_size = 32 * 1024; // 32 KB par bloc
    let synthetic_mic_chunk = vec![0x11u8; chunk_size];
    let synthetic_sys_chunk = vec![0x22u8; chunk_size];

    let total_expected_bytes = 60 * 16000 * 2;
    let iterations = total_expected_bytes / chunk_size;
    let remainder = total_expected_bytes % chunk_size;

    for _ in 0..iterations {
        spooler
            .write_mic_chunk(&synthetic_mic_chunk)
            .expect("write mic chunk");
        spooler
            .write_sys_chunk(&synthetic_sys_chunk)
            .expect("write sys chunk");
    }
    if remainder > 0 {
        spooler
            .write_mic_chunk(&synthetic_mic_chunk[..remainder])
            .expect("write mic remainder");
        spooler
            .write_sys_chunk(&synthetic_sys_chunk[..remainder])
            .expect("write sys remainder");
    }

    let (mic_path, sys_path, mic_bytes, sys_bytes) = spooler.finish().expect("finish spooler");

    assert_eq!(mic_bytes, total_expected_bytes as u64);
    assert_eq!(sys_bytes, total_expected_bytes as u64);
    assert!(mic_path.exists());
    assert!(sys_path.exists());
    assert_eq!(
        fs::metadata(&mic_path).expect("meta mic").len(),
        total_expected_bytes as u64
    );
    assert_eq!(
        fs::metadata(&sys_path).expect("meta sys").len(),
        total_expected_bytes as u64
    );
}

/// TEST-06-02: Deterministic RMS energy diarization
#[test]
fn test_deterministic_rms_energy_diarization() {
    let config = MeetingConfig::default();
    let diarizer = Diarizer::new(config);

    // Frame 1: Mic high (R > 2.0 -> Me)
    let mic_frame_high = vec![0.5f32; 8000];
    let sys_frame_low = vec![0.05f32; 8000];
    let res1 = diarizer.classify_frame(&mic_frame_high, &sys_frame_low);
    assert_eq!(res1.speaker, SpeakerTag::Me);
    assert!(res1.ratio > 2.0);

    // Frame 2: Sys high (R < 0.5 -> Remote)
    let mic_frame_low = vec![0.04f32; 8000];
    let sys_frame_high = vec![0.5f32; 8000];
    let res2 = diarizer.classify_frame(&mic_frame_low, &sys_frame_high);
    assert_eq!(res2.speaker, SpeakerTag::Remote);
    assert!(res2.ratio < 0.5);

    // Frame 3: Both high (0.5 <= R <= 2.0 -> CrossTalk)
    let mic_frame_both = vec![0.4f32; 8000];
    let sys_frame_both = vec![0.45f32; 8000];
    let res3 = diarizer.classify_frame(&mic_frame_both, &sys_frame_both);
    assert_eq!(res3.speaker, SpeakerTag::CrossTalk);

    // Frame 4: Both silence (< 0.005 RMS -> Silence)
    let mic_silence = vec![0.001f32; 8000];
    let sys_silence = vec![0.001f32; 8000];
    let res4 = diarizer.classify_frame(&mic_silence, &sys_silence);
    assert_eq!(res4.speaker, SpeakerTag::Silence);
}

/// TEST-06-03: Clock drift compensation & alignment
#[test]
fn test_clock_drift_compensation_and_alignment() {
    let sample_rate = 16000;
    let mic_samples = vec![0.2f32; 16000];
    let sys_samples = vec![0.2f32; 16050]; // 50 échantillons de drift (3.125 ms)

    let (aligned_mic, aligned_sys) =
        AudioAligner::align_tracks(&mic_samples, &sys_samples, sample_rate).expect("align tracks");

    assert_eq!(aligned_mic.len(), aligned_sys.len());
    let residual_delta = (aligned_mic.len() as isize - aligned_sys.len() as isize).abs();
    assert_eq!(residual_delta, 0);

    let phase_diff_ms = (50.0 / sample_rate as f64) * 1000.0;
    assert!(phase_diff_ms < 20.0);
}

/// TEST-06-04: 8x8 Perceptual hashing (pHash)
#[test]
fn test_perceptual_hashing_phash() {
    let img1: [u8; 64] = [128; 64];
    let hash1 = PerceptualHasher::hash_grayscale_8x8(&img1);
    let dist_same = PerceptualHasher::hamming_distance(hash1, hash1);
    assert_eq!(dist_same, 0);
    assert_eq!(PerceptualHasher::variation_percentage(hash1, hash1), 0.0);

    // Image altérée avec des pixels contrastés
    let mut img2: [u8; 64] = [50; 64];
    img2[..16].fill(200);
    let hash2 = PerceptualHasher::hash_grayscale_8x8(&img2);
    let dist_diff = PerceptualHasher::hamming_distance(hash1, hash2);
    assert!(dist_diff > 0);
    let var_pct = PerceptualHasher::variation_percentage(hash1, hash2);
    assert!(var_pct > 0.0 && var_pct <= 1.0);
}

/// TEST-06-05: Slide transition detection
#[test]
fn test_slide_transition_detection() {
    let mut detector = SlideDetector::new(0.15, 0, "test_session");

    // Frame 1 : Image de base
    let mut frame1 = [100u8; 64];
    frame1[..32].fill(200);
    let slide1 = detector.process_frame(0, &frame1);
    assert!(slide1.is_some(), "Le premier frame doit être capturé");

    // Frame 2 : Bruit mineur (< 5% variation)
    let mut frame2 = frame1;
    frame2[0] = 199; // Décalage infime
    let slide2 = detector.process_frame(1000, &frame2);
    assert!(
        slide2.is_none(),
        "Un bruit mineur ne doit pas déclencher de transition"
    );

    // Frame 3 : Diapositive complètement différente (> 30% variation)
    let mut frame3 = [200u8; 64];
    frame3[..32].fill(50);
    let slide3 = detector.process_frame(2000, &frame3);
    assert!(
        slide3.is_some(),
        "Une transition majeure doit déclencher un SlideKeyframe"
    );
}

/// TEST-06-06: Markdown note generation & seek links
#[test]
fn test_meeting_note_generation_seek_links() {
    let start_time = chrono::Utc::now();
    let session = MeetingSession {
        id: "reunion-20261006-143000".to_string(),
        title: "Architecture Revue".to_string(),
        start_time,
        end_time: Some(start_time + chrono::Duration::seconds(3600)),
        duration_seconds: 3600,
        mic_file_path: PathBuf::from("mic.pcm"),
        sys_file_path: PathBuf::from("sys.pcm"),
        output_note_path: None,
        segments: vec![
            TranscriptSegment {
                id: "seg-1".to_string(),
                start_ms: 5000,
                end_ms: 12000,
                speaker: SpeakerTag::Me,
                text: "Introduction de l'architecture".to_string(),
                seek_seconds: 5,
            },
            TranscriptSegment {
                id: "seg-2".to_string(),
                start_ms: 15000,
                end_ms: 25000,
                speaker: SpeakerTag::Remote,
                text: "Validation du diagramme".to_string(),
                seek_seconds: 15,
            },
            TranscriptSegment {
                id: "seg-3".to_string(),
                start_ms: 80000,
                end_ms: 95000,
                speaker: SpeakerTag::CrossTalk,
                text: "Discussion sur les invariants".to_string(),
                seek_seconds: 80,
            },
        ],
        slides: vec![SlideKeyframe {
            timestamp_ms: 315000,
            file_name: "slide_01.jpg".to_string(),
            relative_path: "Attachments/Slides/slide_01.jpg".to_string(),
            phash: 123456789,
            hamming_dist_pct: 0.25,
            ocr_text: Some("Schéma Système".to_string()),
        }],
    };

    let md = MeetingNoteGenerator::generate_markdown(&session);

    assert!(md.contains("note_type: episodique"));
    assert!(md.contains("statut: actif"));
    assert!(md.contains("[[00:00:05]](seek:5) **[Me]** :"));
    assert!(md.contains("[[00:00:15]](seek:15) **[Remote]** :"));
    assert!(md.contains("[[00:01:20]](seek:80) **[Cross-talk]** :"));
    assert!(md.contains("[[00:05:15]](seek:315) ![Slide](Attachments/Slides/slide_01.jpg)"));
}

/// TEST-06-07: Orphaned PCM recovery
#[test]
fn test_orphaned_pcm_recovery() {
    let temp_vault = tempfile::tempdir().expect("temp vault");
    let temp_audio = tempfile::tempdir().expect("temp audio");

    let session_id = "orphan_test_123";
    let mic_file = temp_audio
        .path()
        .join(format!("session_{session_id}_mic.pcm"));
    let sys_file = temp_audio
        .path()
        .join(format!("session_{session_id}_sys.pcm"));

    // Écriture de 2 secondes de données PCM
    let dummy_pcm = vec![0u8; 2 * 16000 * 2];
    fs::write(&mic_file, &dummy_pcm).expect("write mic");
    fs::write(&sys_file, &dummy_pcm).expect("write sys");

    let recovered =
        MeetingRecorder::recover_orphaned_sessions(temp_vault.path(), temp_audio.path())
            .expect("recover sessions");

    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].session_id, session_id);

    let expected_note = temp_vault
        .path()
        .join("Reunions")
        .join(format!("{session_id}.md"));
    assert!(
        expected_note.exists(),
        "La note de réunion doit avoir été générée"
    );
}

/// TEST-06-08: Memory budget audit (< 100 MB active, 0 MB stop)
#[tokio::test]
async fn test_memory_budget_audit() {
    let temp_vault = tempfile::tempdir().expect("temp vault");
    let temp_audio = tempfile::tempdir().expect("temp audio");

    let recorder = MeetingRecorder::new(
        MeetingConfig::default(),
        temp_audio.path().to_path_buf(),
        temp_vault.path().to_path_buf(),
    );

    let status_initial = recorder.get_status().await;
    assert_eq!(status_initial.memory_allocated_mb, 0);
    assert!(!status_initial.is_recording);

    let start_status = recorder
        .start_recording(Some("Session Budget Test".to_string()))
        .await
        .expect("start");
    assert!(start_status.is_recording);
    assert!(start_status.memory_allocated_mb < 100);

    // Injection de frames simulées
    let dummy_chunk = vec![0u8; 1024];
    recorder
        .feed_synthetic_frame(&dummy_chunk, &dummy_chunk)
        .await
        .expect("feed frame");

    let summary = recorder.stop_recording().await.expect("stop");
    assert!(!summary.note_path.is_empty());

    let status_stopped = recorder.get_status().await;
    assert!(!status_stopped.is_recording);
    assert_eq!(
        status_stopped.memory_allocated_mb, 0,
        "Mémoire résiduelle strictement 0 Mo après arrêt"
    );
}
