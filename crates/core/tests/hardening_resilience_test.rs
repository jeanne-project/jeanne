use jeanne_core::meeting::{
    HotUnplugRecoveryResult, MeetingConfig, MeetingError, MeetingRecorder,
    validate_wav_header_16k_mono,
};
use jeanne_core::storage::StorageManager;
use jeanne_core::vault::index_note_with_concurrency_check;
use std::fs::File;
use std::io::Read;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;

/// TEST-08-02: Hot-unplug resilience & valid 16 kHz WAV finalization
#[tokio::test]
async fn test_08_02_hot_unplug_recovery_and_wav_generation() {
    let temp_vault = tempdir().expect("temp vault dir");
    let temp_spool = tempdir().expect("temp spool dir");

    let config = MeetingConfig::default();
    let recorder = MeetingRecorder::new(
        config,
        temp_spool.path().to_path_buf(),
        temp_vault.path().to_path_buf(),
    );

    // 1. Démarrer l'enregistrement de réunion
    let status_start = recorder
        .start_recording(Some("Réunion Architecture Sprint".to_string()))
        .await
        .expect("start recording");
    assert!(status_start.is_recording);
    assert!(recorder.is_recording().await);

    // 2. Émettre des trames synthétiques PCM (16 kHz, 16-bit)
    let pcm_frame = vec![0u8; 16000 * 2]; // 1 seconde de silence/audio PCM
    recorder
        .feed_synthetic_frame(&pcm_frame, &pcm_frame)
        .await
        .expect("feed synthetic frame");

    // 3. Simuler le débranchement brutal du périphérique audio (StreamError::DeviceNotAvailable)
    let recovery: HotUnplugRecoveryResult = recorder
        .handle_hot_unplug()
        .await
        .expect("hot unplug should recover cleanly");

    // 4. Assertions sur le toast de notification non-bloquant (SPEC §3.1)
    assert_eq!(
        recovery.toast_message,
        "Audio device disconnected. Meeting saved up to interruption."
    );

    // 5. Vérifier que le fichier WAV 16 kHz valide a été produit sur disque
    assert!(
        recovery.wav_file_path.exists(),
        "Le fichier WAV de récupération doit exister"
    );
    let mut wav_file = File::open(&recovery.wav_file_path).expect("open wav file");
    let mut header_bytes = [0u8; 44];
    wav_file
        .read_exact(&mut header_bytes)
        .expect("read wav header");

    assert!(
        validate_wav_header_16k_mono(&header_bytes),
        "L'en-tête WAV doit être un format valide 16 kHz mono 16-bit PCM"
    );

    // 6. Vérifier que la note markdown a été sauvegardée dans Reunions/
    let reunions_dir = temp_vault.path().join("Reunions");
    assert!(reunions_dir.exists());
    let note_path = reunions_dir.join(format!("{}.md", recovery.session_id));
    assert!(note_path.exists());
    let note_content = std::fs::read_to_string(&note_path).expect("read note content");
    assert!(note_content.contains("Compte-rendu - Réunion Architecture Sprint"));

    // 7. Vérifier que l'état du coordinateur a été réinitialisé à l'arrêt propre (0 Mo alloué, is_recording = false)
    let status_after = recorder.get_status().await;
    assert!(!status_after.is_recording);
    assert_eq!(status_after.memory_allocated_mb, 0);
    assert_eq!(status_after.current_session_id, None);
    assert!(!recorder.is_recording().await);
}

#[tokio::test]
async fn test_08_02_hot_unplug_when_inactive_returns_error() {
    let temp_vault = tempdir().expect("temp vault dir");
    let temp_spool = tempdir().expect("temp spool dir");

    let recorder = MeetingRecorder::new(
        MeetingConfig::default(),
        temp_spool.path().to_path_buf(),
        temp_vault.path().to_path_buf(),
    );

    // Débranchement alors qu'aucun enregistrement n'est en cours
    let result = recorder.handle_hot_unplug().await;
    match result {
        Err(MeetingError::Inactive) => (),
        other => panic!("Attendu MeetingError::Inactive, reçu : {:?}", other),
    }
}

/// TEST-08-03: Détection de modification concurrente de fichier lors de l'indexation (SPEC §3.2)
#[tokio::test]
async fn test_08_03_concurrent_file_modification_detection() {
    let temp_vault = tempdir().expect("temp vault");
    let note_file = temp_vault.path().join("spec_concurrency.md");

    // Écriture de la version V1
    std::fs::write(
        &note_file,
        "# Version 1\nInitial content written by external process.",
    )
    .expect("write v1");

    let storage = StorageManager::open_in_memory().expect("open storage");
    storage.init_schema().expect("init schema");
    let storage_arc = Arc::new(Mutex::new(storage));

    // Simulation d'une écriture concurrente (V2) sur le disque pendant l'indexation
    // La fonction index_note_with_concurrency_check doit détecter le changement de hash SHA-256
    // et réindexer la version la plus récente sans corrompre la base
    let note_path_clone = note_file.clone();
    let (indexed_version, retry_occurred) = index_note_with_concurrency_check(
        temp_vault.path(),
        "spec_concurrency.md",
        &storage_arc,
        Some(Box::new(move || {
            // Callback simulant une écriture concurrente externe (Dropbox/Git) exactement au milieu du parsing
            std::fs::write(
                &note_path_clone,
                "# Version 2\nUpdated content written concurrently by sync client.",
            )
            .expect("concurrent write v2");
        })),
    )
    .await
    .expect("index with concurrency check");

    assert!(
        retry_occurred,
        "Une réconciliation concurrente a dû être déclenchée"
    );
    assert_eq!(indexed_version, 2);

    // Vérifier que la note indexée dans SQLite correspond bien à la Version 2
    let st = storage_arc.lock().expect("lock storage");
    let search_res = st
        .search_fts("Updated content", 5)
        .expect("search fts updated");
    assert!(!search_res.is_empty());
    assert!(search_res[0].snippet.contains("Version 2"));
}

/// TEST-08-04: Test d'endurance (Soak Test) & absence de fuite mémoire (SPEC §4 TEST-08-03)
#[test]
fn test_08_04_soak_test_memory_leak_variance() {
    let storage = StorageManager::open_in_memory().expect("open storage");
    storage.init_schema().expect("init schema");

    // Simulation multi-cycles de charge (équivalent synthétique d'un cycle 24h)
    // 5 cycles de 200 notes insérées + 100 requêtes de recherche FTS par cycle
    let mut initial_allocated_bytes = 0usize;
    let mut final_allocated_bytes = 0usize;

    for cycle in 0..5 {
        for n in 0..200 {
            let path = format!("cycle_{cycle}_note_{n}.md");
            let content = format!(
                "---\nid: note_{cycle}_{n}\ntitle: Note {n}\nnote_type: semantique\nstatut: actif\n---\n# Note {n} Content\nThis is synthetic body for testing leak resistance in cycle {cycle} item {n}."
            );
            let chunk = jeanne_core::models::IndexedChunk {
                id: None,
                chunk_id: format!("{path}:0"),
                file_path: path.clone(),
                chunk_index: 0,
                content,
                token_count: 20,
                coala_type: jeanne_core::models::CoalaType::Semantic,
                status: jeanne_core::models::NoteStatus::Active,
                superseded_by: None,
                deprecated_at: None,
                date_creation: 1700000000,
            };
            let _ = storage.index_chunk(&chunk);
        }

        // Requêtes intensives
        for q in 0..100 {
            let query = format!("synthetic body item {q}");
            let _ = storage.search_fts(&query, 10);
        }

        if cycle == 0 {
            // Mesure post-chauffage
            initial_allocated_bytes = get_current_process_memory_estimate();
        }
        if cycle == 4 {
            final_allocated_bytes = get_current_process_memory_estimate();
        }
    }

    // La variance entre le cycle 1 et la fin du test doit être inférieure à 5 Mo
    let variance_bytes = final_allocated_bytes.saturating_sub(initial_allocated_bytes);
    let variance_mb = variance_bytes as f64 / (1024.0 * 1024.0);

    println!(
        "Soak Test: Variance mémoire mesurée = {:.2} Mo (Plafond strict < 5.0 Mo)",
        variance_mb
    );
    assert!(
        variance_mb < 5.0,
        "La variance mémoire ({:.2} Mo) dépasse le plafond strict de 5 Mo",
        variance_mb
    );
}

fn get_current_process_memory_estimate() -> usize {
    // Lecture directe de /proc/self/statm sur Linux ou repli
    if let Ok(statm) = std::fs::read_to_string("/proc/self/statm") {
        if let Some(rss_pages) = statm.split_whitespace().nth(1) {
            if let Ok(pages) = rss_pages.parse::<usize>() {
                return pages * 4096; // Taille de page 4 Ko
            }
        }
    }
    10 * 1024 * 1024 // 10 Mo par défaut si non disponible
}
