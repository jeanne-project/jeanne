use jeanne_core::eval::{
    GoldenDatasetEvaluator, GoldenDatasetItem, OOD_FALLBACK_STRING, compute_deterministic_embedding,
};
use jeanne_core::models::{IndexedChunk, NoteFrontmatter};
use jeanne_core::storage::StorageManager;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Helper: charge le dataset golden depuis le fichier JSON
fn load_golden_dataset() -> Vec<GoldenDatasetItem> {
    let candidates = [
        "tests/data/eval_golden_dataset.json",
        "crates/core/tests/data/eval_golden_dataset.json",
        "../../tests/data/eval_golden_dataset.json",
    ];

    for path in &candidates {
        if Path::new(path).exists() {
            let data = std::fs::read_to_string(path).expect("lecture golden dataset");
            return serde_json::from_str(&data).expect("désérialisation golden dataset");
        }
    }

    panic!("Impossible de trouver tests/data/eval_golden_dataset.json");
}

/// Helper: initialise le coffre de test avec les notes architecturales et procédurales requises
fn setup_test_knowledge_vault(storage: &StorageManager) {
    let notes = [
        (
            "note_arch_sqlite_vec_01",
            "notes/arch/sqlite_vec.md",
            "SQLite-Vec Constraints and Architecture",
            "Must be a 64-bit signed integer rowid. TEXT or UUID primary keys cause runtime SQL errors in sqlite-vec vec0 tables.",
            "semantique",
        ),
        (
            "note_arch_memory_budgets_02",
            "notes/arch/memory_budgets.md",
            "Memory Footprint and Hardware Budgets",
            "Release build idle RAM must remain strictly below 80 MB. Application resident footprint excluding LLM weights stays below 200 MB. Active 3B quantized model footprint must not exceed 4.5 GB RAM on shared iGPU 16 GB machine. Defines four memory strata: Procedural templates and protocols, Semantic verified facts and specifications, Episodic chronological logs and meeting transcripts, Working bounded active conversation window.",
            "semantique",
        ),
        (
            "note_arch_timedecay_03",
            "notes/arch/timedecay.md",
            "Hybrid Search and Time Decay Formulation",
            "Combines 70% vector similarity and 30% BM25 lexical score. Raw score is divided by 1 plus lambda times delta time in days. Procedural notes have half life of infinity with lambda equal to 0.0. Semantic notes have half life of 365 days. Episodic notes have half life of 30 days. Standard search filters out deprecated notes with WHERE statut = 'actif'. Superseded links are resolved to target note in graph.",
            "semantique",
        ),
        (
            "note_arch_circuit_breaker_04",
            "notes/arch/circuit_breaker.md",
            "Anti-Hallucination Circuit Breaker",
            "Circuit breaker threshold is strictly 0.65 cosine similarity. Queries below 0.65 trigger InformationNotFound error. Ensures zero hallucinations for out-of-domain queries.",
            "semantique",
        ),
        (
            "note_arch_plugins_05",
            "notes/arch/plugins.md",
            "Subprocess Plugins and JSON-RPC Architecture",
            "Subprocesses communicate exclusively via JSON-RPC 2.0 over standard I/O stdio. Binary payloads are not streamed over stdio. Subprocess timeout kills hung plugins after 30 seconds. Bounded stdout line reading up to 8 MB to prevent buffer overflow attacks.",
            "semantique",
        ),
        (
            "note_arch_hardware_06",
            "notes/arch/hardware.md",
            "Local Inference and Hardware Detection",
            "Context window is bounded to 4096 tokens to conserve RAM. Preserves at least 512 MB to 1024 MB VRAM headroom for host compositor. Qwen 2.5 or 3.5 2B Q4_K_M was selected as default lightweight local model for Vulkan acceleration.",
            "semantique",
        ),
        (
            "note_arch_meeting_07",
            "notes/arch/meeting.md",
            "Meeting Recorder and Diarization",
            "Left channel records local microphone. Right channel records system loopback WASAPI audio. Ratio of mic RMS to sys RMS greater than 2.0 classifies speaker as Me. Ratio of mic RMS to sys RMS less than 0.5 classifies speaker as Remote. Scans temp directory for unfinalized pcm files without matching markdown note to reconstruct note and align spooled audio.",
            "semantique",
        ),
        (
            "note_arch_slide_08",
            "notes/arch/slide.md",
            "Slide Keyframe Extraction and Perceptual Hashing",
            "Perceptual hash threshold is 15 percent Hamming distance variation. Linked with interactive seek format [[timestamp]](seek:seconds) accompanied by markdown image tag referencing relative path.",
            "semantique",
        ),
        (
            "note_arch_pii_09",
            "notes/arch/pii.md",
            "PII Redaction and Source Citation Invariant",
            "Deterministic regex redacts emails phones and credit cards before remote transmission. PII is demasked locally upon receiving streaming tokens. Every factual claim must cite source note using format [source: filename.md].",
            "semantique",
        ),
        (
            "note_arch_audio_10",
            "notes/arch/audio.md",
            "Audio Pipeline and Hot Unplug Handling",
            "Standard audio sampling rate is 16000 Hz 16 kHz mono. CPAL DeviceNotAvailable error is caught without panic. Spooler flushes and finalizes audio files up to interruption. Emits toast notification that audio device disconnected and meeting was saved.",
            "semantique",
        ),
        (
            "note_arch_conventions_11",
            "notes/arch/conventions.md",
            "Toolchain Conventions and Release Flags",
            "Quick-access overlay must appear and be interactive in less than 50 ms. Strict zero unwrap and zero expect rule in crates/core. Mandates Rust Edition 2024 with MSRV 1.85.0. Release compiler flags in Cargo.toml require opt-level = 3 lto = true codegen-units = 1 panic = abort strip = true.",
            "procedurale",
        ),
        (
            "note_proc_git_hygiene_12",
            "notes/proc/git_hygiene.md",
            "Git Workflow and Spec Driven Development",
            "Sole entry point for user requests and specification author is Architect agent. Orchestrates milestone branches reviews and merge gates. Spec-Driven Development is mandatory with zero lines of production code without validated spec in docs/specs. Review report docs/reviews/M{ID}_CODE_REVIEW.md must explicitly state STATUS: APPROUVÉ before milestone merge.",
            "procedurale",
        ),
        (
            "note_episodic_adr01_13",
            "notes/episodic/adr01.md",
            "ADR-01 Architecture Decisions",
            "Local vault of plain Markdown files is the primary immutable source of truth. SQLite database is strictly a disposable derived cache index. Strict zero Python runtime and zero Pydantic dependencies in crates/core. All document schemas are validated natively in Rust.",
            "episodique",
        ),
        (
            "note_episodic_sync_14",
            "notes/episodic/sync_q3.md",
            "Sprint Retrospective and Sync Q3",
            "Implement Svelte 5 runes migration for floating palette overlay latency reduction was assigned to frontend lead.",
            "episodique",
        ),
        (
            "note_episodic_incident_15",
            "notes/episodic/incident_20260928.md",
            "Incident Report Memory Soak",
            "Unbounded memory growth caused by reading full WAV audio files directly into memory buffer. Fixed by enforcing streaming chunked spooling with DualTrackSpooler.",
            "episodique",
        ),
        (
            "note_arch_vault_indexing_16",
            "notes/arch/vault_indexing.md",
            "Concurrent Vault Indexing Protection",
            "Computes SHA-256 hash before SQLite transaction. Verifies disk file hash and timestamp after parsing. Discards stale transaction and reenqueues updated file if hash changed.",
            "semantique",
        ),
    ];

    let now = chrono::Utc::now().timestamp();

    for (id, _original_path, title, content, coala) in notes {
        let file_path = format!("notes/{id}.md");
        let fm = NoteFrontmatter {
            id: id.to_string(),
            title: title.to_string(),
            note_type: coala.to_string(),
            statut: "actif".to_string(),
            ..Default::default()
        };
        let fm_json = serde_json::to_string(&fm).ok();
        storage
            .upsert_file(&file_path, "sha256_mock", now, fm_json.as_deref())
            .expect("upsert file");

        let chunk = IndexedChunk {
            id: None,
            chunk_id: format!("{file_path}:0"),
            file_path: file_path.clone(),
            chunk_index: 0,
            content: content.to_string(),
            token_count: content.split_whitespace().count(),
            coala_type: fm.coala_type(),
            status: fm.status(),
            superseded_by: None,
            deprecated_at: None,
            date_creation: now,
        };
        let rowid = storage.index_chunk(&chunk).expect("index chunk");

        // Calcul d'embedding déterministe pour chaque note
        let emb = compute_deterministic_embedding(content);
        storage
            .insert_chunk_vector(rowid, &emb)
            .expect("insert chunk vector");
    }
}

/// TEST-08-01: Validation du Golden Dataset (50 requêtes, Recall >= 90%, Faithfulness >= 95%, Hallucinations = 0)
#[test]
fn test_08_01_golden_dataset_metrics_and_scoring() {
    let items = load_golden_dataset();
    assert_eq!(
        items.len(),
        50,
        "Le Golden Dataset doit contenir exactement 50 requêtes curées"
    );

    let storage = StorageManager::open_in_memory().expect("open test storage");
    storage.init_schema().expect("init schema");
    setup_test_knowledge_vault(&storage);

    let storage_arc = Arc::new(Mutex::new(storage));
    let evaluator = GoldenDatasetEvaluator::new(storage_arc);

    let metrics = evaluator.evaluate_dataset(&items);

    println!("=== RÉSULTATS DU BENCHMARK GOLDEN DATASET ===");
    println!("Total requêtes: {}", metrics.total_queries);
    println!("Requêtes In-Domain: {}", metrics.in_domain_queries);
    println!("Requêtes Out-of-Domain: {}", metrics.out_of_domain_queries);
    println!(
        "Retrieval Recall @ 3: {:.2}% (Seuil requis >= 90%)",
        metrics.recall_at_3 * 100.0
    );
    println!(
        "Context Faithfulness: {:.2}% (Seuil requis >= 95%)",
        metrics.context_faithfulness * 100.0
    );
    println!(
        "Out-of-Domain Hallucination Rate: {:.2}% (Seuil strict = 0%)",
        metrics.ood_hallucination_rate * 100.0
    );
    println!(
        "Out-of-Domain Fallback Exact Match: {:.2}% (Attendu = 100%)",
        metrics.ood_fallback_exact_match_rate * 100.0
    );

    // Assertions strictes de la spécification (§2.2 et §4)
    assert!(
        metrics.recall_at_3 >= 0.90,
        "Retrieval Recall ({:.2}%) doit être >= 90%",
        metrics.recall_at_3 * 100.0
    );

    assert!(
        metrics.context_faithfulness >= 0.95,
        "Context Faithfulness ({:.2}%) doit être >= 95%",
        metrics.context_faithfulness * 100.0
    );

    assert_eq!(
        metrics.ood_hallucination_rate, 0.0,
        "Out-of-Domain Hallucination Rate doit être strictement 0%"
    );

    assert_eq!(
        metrics.ood_fallback_exact_match_rate, 1.0,
        "Out-of-Domain Fallback Exact Match doit être strictement 100%"
    );
}

#[test]
fn test_08_01_ood_exact_fallback_string() {
    let items = load_golden_dataset();
    let ood_items: Vec<_> = items.into_iter().filter(|i| i.is_out_of_domain).collect();
    assert_eq!(
        ood_items.len(),
        12,
        "Il doit y avoir exactement 12 requêtes hors domaine"
    );

    let storage = StorageManager::open_in_memory().expect("open test storage");
    storage.init_schema().expect("init schema");
    let storage_arc = Arc::new(Mutex::new(storage));
    let evaluator = GoldenDatasetEvaluator::new(storage_arc);

    for ood in ood_items {
        let res = evaluator.evaluate_query(&ood);
        assert!(res.is_out_of_domain);
        assert_eq!(
            res.generated_response, OOD_FALLBACK_STRING,
            "Requête OOD '{}' doit renvoyer la chaîne exacte de fallback",
            ood.query
        );
        assert!(!res.is_hallucination);
    }
}
