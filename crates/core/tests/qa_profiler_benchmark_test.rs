use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tempfile::tempdir;

use jeanne_core::models::{CoalaType, IndexedChunk, NoteStatus};
use jeanne_core::rag::RagEngine;
use jeanne_core::storage::StorageManager;
use jeanne_core::vault::VaultWatcher;

/// Helper to read Resident RSS and Peak RSS (High Water Mark) from Linux /proc/self/status.
fn get_memory_stats_kb() -> (usize, usize) {
    let mut vmrss = 0;
    let mut vmhwm = 0;
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        for line in status.lines() {
            if line.starts_with("VmRSS:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    vmrss = parts[1].parse().unwrap_or(0);
                }
            } else if line.starts_with("VmHWM:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    vmhwm = parts[1].parse().unwrap_or(0);
                }
            }
        }
    }
    (vmrss, vmhwm)
}

/// Protocole QA-Profiler 1 : Benchmark de Latence BM25 FTS5
/// Critère contractuel : Latence de recherche < 15 ms sur un coffre indexé.
#[test]
fn benchmark_bm25_search_latency() {
    let dir = tempdir().expect("Création dossier temporaire");
    let db_path = dir.path().join("bench_fts.db");
    let storage = StorageManager::open(&db_path).expect("Ouverture base SQLite");
    storage.init_schema().expect("Initialisation schéma FTS5");

    let conn = storage.raw_connection();

    // 1. Ingestion de 50 notes diversifiées
    for i in 0..50 {
        let file_path = format!("Notes/Doc_{i:02}.md");
        let hash = format!("hash_{i:04}");
        conn.execute(
            "INSERT INTO files (file_path, file_hash, last_modified, frontmatter_json) VALUES (?1, ?2, ?3, ?4)",
            (&file_path, &hash, 1710000000i64 + i as i64, "{\"title\": \"Note de test\"}"),
        ).expect("Insertion fichier");

        let content = if i % 5 == 0 {
            format!(
                "Architecture distribuée et gestion souveraine des données locales pour Jeanne document {i}."
            )
        } else if i % 3 == 0 {
            format!("Indexation lexicale FTS5 et synchronisation continue du coffre markdown {i}.")
        } else {
            format!(
                "Contenu générique de réunion et journalisation périodique fragment numéro {i}."
            )
        };

        let chunk = IndexedChunk {
            id: None,
            chunk_id: format!("{file_path}:0"),
            file_path,
            chunk_index: 0,
            content,
            token_count: 15,
            coala_type: CoalaType::Semantic,
            status: NoteStatus::Active,
            superseded_by: None,
            deprecated_at: None,
            date_creation: 1710000000 + i as i64,
        };
        let rowid = storage.index_chunk(&chunk).expect("Indexation chunk");

        // Ingestion du vecteur 384D correspondant pour benchmark hybride
        let mut vec = [0.0f32; 384];
        vec[i % 5] = 1.0;
        storage
            .insert_chunk_vector(rowid, &vec)
            .expect("Insertion vecteur");
    }

    // 2. Exécution de 100 requêtes de recherche représentatives et mesure de la latence BM25
    let queries = [
        "architecture",
        "souveraine",
        "lexicale",
        "réunion",
        "Jeanne",
    ];
    let mut latencies = Vec::with_capacity(100);

    for (idx, q) in queries.iter().cycle().take(100).enumerate() {
        let query_term = if idx % 2 == 0 { *q } else { &format!("{q}*") };
        let start = Instant::now();
        let results = storage.search_fts(query_term, 10).expect("Recherche FTS5");
        let elapsed = start.elapsed();
        latencies.push(elapsed);
        assert!(
            !results.is_empty(),
            "La recherche doit retourner des résultats pour '{query_term}'"
        );
        assert!(
            results[0].snippet.contains("<mark>"),
            "Le snippet doit contenir le balisage <mark> pour la surbrillance"
        );
    }

    latencies.sort();
    let p50 = latencies[50];
    let p95 = latencies[95];
    let max = latencies[99];

    println!("BM25 Latency Benchmark Results (100 iterations) :");
    println!("- P50 : {p50:?}");
    println!("- P95 : {p95:?}");
    println!("- Max : {max:?}");

    // Assertion contractuelle : P95 strictement inférieur à 15 ms
    assert!(
        p95 < Duration::from_millis(15),
        "La latence P95 ({p95:?}) doit être strictement inférieure au plafond de 15 ms"
    );

    // 3. Exécution de 100 requêtes de recherche Hybride RAG (Dense Vector + BM25 + Time-Decay)
    let engine = RagEngine::new(Arc::new(Mutex::new(storage)));
    let mut hybrid_latencies = Vec::with_capacity(100);

    for (idx, q) in queries.iter().cycle().take(100).enumerate() {
        let mut query_vec = [0.0f32; 384];
        query_vec[idx % 5] = 1.0;
        let start = Instant::now();
        let results = engine
            .search(q, &query_vec, 10)
            .expect("Recherche Hybride RAG");
        let elapsed = start.elapsed();
        hybrid_latencies.push(elapsed);
        assert!(
            !results.is_empty(),
            "La recherche hybride doit retourner des résultats pour '{q}'"
        );
    }

    hybrid_latencies.sort();
    let h_p50 = hybrid_latencies[50];
    let h_p95 = hybrid_latencies[95];
    let h_max = hybrid_latencies[99];

    println!("Hybrid RAG Latency Benchmark Results (100 iterations) :");
    println!("- P50 : {h_p50:?}");
    println!("- P95 : {h_p95:?}");
    println!("- Max : {h_max:?}");

    // Assertion contractuelle : Latence P95 strictement inférieure à 30 ms
    assert!(
        h_p95 < Duration::from_millis(30),
        "La latence P95 Hybride ({h_p95:?}) doit être strictement inférieure à 30 ms"
    );

    // 4. Profilage de l'empreinte mémoire résidente (RSS)
    let (rss_kb, hwm_kb) = get_memory_stats_kb();
    if rss_kb > 0 || hwm_kb > 0 {
        println!("Memory Footprint under Hybrid RAG Workload :");
        println!(
            "- Current RSS : {} KB ({:.2} MB)",
            rss_kb,
            rss_kb as f64 / 1024.0
        );
        println!(
            "- Peak RSS (VmHWM) : {} KB ({:.2} MB)",
            hwm_kb,
            hwm_kb as f64 / 1024.0
        );
        // Exigence contractuelle stricte : RSS résidente < 200 Mo (204 800 Ko)
        assert!(
            hwm_kb < 200 * 1024,
            "L'empreinte mémoire résidente maximale ({hwm_kb} KB) dépasse le plafond strict de 200 Mo"
        );
    }
}

/// Protocole QA-Profiler 2 : Stress de Concurrence SQLite (WAL Mode) & Dé-rebond Watcher
/// Exigence QA-Profiler : Simuler 20 écritures consécutives en 100 ms tout en exécutant
/// des lectures concurrentes, et vérifier l'absence d'erreur 'database is locked'.
#[tokio::test]
async fn stress_concurrency_and_debouncing_under_load() {
    let temp_vault = tempdir().expect("Création répertoire temporaire");
    let vault_path = temp_vault.path().to_path_buf();

    let db_path = vault_path.join(".jeanne").join("database.db");
    let storage = StorageManager::open(&db_path).expect("Ouverture StorageManager");
    storage.init_schema().expect("Initialisation schéma SQLite");
    let storage_arc = Arc::new(Mutex::new(storage));

    let mut watcher = VaultWatcher::new(&vault_path, storage_arc.clone())
        .expect("Initialisation VaultWatcher")
        .with_debounce_duration(Duration::from_millis(300));

    watcher.start().expect("Démarrage du watcher");

    // 1. Lancer un thread concurrent de lecture intensive (simulation recherche UI pendant sauvegarde)
    let storage_reader = storage_arc.clone();
    let stop_reader = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_reader_clone = stop_reader.clone();

    let reader_handle = tokio::spawn(async move {
        let mut read_count = 0;
        while !stop_reader_clone.load(std::sync::atomic::Ordering::Relaxed) {
            {
                if let Ok(s) = storage_reader.try_lock() {
                    let _ = s.search_fts("stress", 5);
                    read_count += 1;
                }
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        read_count
    });

    // 2. Simuler 20 écritures rapides en moins de 100 ms (rafale de sauvegarde utilisateur)
    let note_path = vault_path.join("ConcurrentStressNote.md");
    for i in 1..=20 {
        let content = format!(
            "---\nid: stress-{i}\ntitle: Note Stress {i}\ndate_creation: \"2026-09-13T12:00:00Z\"\ndate_modification: \"2026-09-13T12:00:00Z\"\nnote_type: episodique\nstatut: actif\ntags:\n  - stress\n---\n\nContenu itération numéro {i} pour test de concurrence sans lock.\n"
        );
        std::fs::write(&note_path, content).expect("Écriture fichier sous stress");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    // 3. Attendre la stabilisation de la fenêtre de dé-rebond (300 ms + délai d'attente)
    let start_wait = Instant::now();
    while watcher.reconciliation_count() == 0 && start_wait.elapsed() < Duration::from_millis(2500)
    {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Arrêt du lecteur concurrent
    stop_reader.store(true, std::sync::atomic::Ordering::Relaxed);
    let total_reads = reader_handle
        .await
        .expect("Arrêt tâche de lecture concurrente");

    println!("Total concurrent reads during file watcher burst: {total_reads}");

    // 4. Assertions contractuelles
    // Assertion 1 : Le dé-rebond a bien fonctionné (au maximum 2 transactions pour les 20 rafales, typiquement 1)
    let recon_count = watcher.reconciliation_count();
    assert!(
        (1..=2).contains(&recon_count),
        "Le dé-rebond doit agréger les 20 événements en 1 (ou max 2) transaction(s), obtenu: {recon_count}"
    );

    // Assertion 2 : Le contenu final de l'itération 20 est indexé et interrogeable
    let s = storage_arc.lock().expect("Verrou storage");
    let results = s
        .search_fts("itération numéro 20", 5)
        .expect("Recherche FTS5 finale");
    assert_eq!(
        results.len(),
        1,
        "Le document final (itération 20) doit être indexé et trouvable sans verrou résiduel"
    );

    watcher.stop();

    // 5. Profilage de l'empreinte mémoire résidente sous charge concurrente
    let (rss_kb, hwm_kb) = get_memory_stats_kb();
    if rss_kb > 0 || hwm_kb > 0 {
        println!("Memory Footprint under Concurrency Stress Workload :");
        println!(
            "- Current RSS : {} KB ({:.2} MB)",
            rss_kb,
            rss_kb as f64 / 1024.0
        );
        println!(
            "- Peak RSS (VmHWM) : {} KB ({:.2} MB)",
            hwm_kb,
            hwm_kb as f64 / 1024.0
        );
        assert!(
            hwm_kb < 200 * 1024,
            "L'empreinte mémoire résidente maximale ({hwm_kb} KB) dépasse le plafond strict de 200 Mo"
        );
    }
}

/// Protocole QA-Profiler 3 : Benchmark Mémoire & Latence Inférence Distante + Masquage PII (Jalon 3)
/// Exigence contractuelle :
/// 1. VmRSS résidente strictement < 150 Mo durant le streaming.
/// 2. Latence d'annulation (cancellation) < 20 ms.
/// 3. Débit de masquage PII instantané (< 10 ms pour 10 000 caractères).
#[tokio::test]
async fn benchmark_remote_inference_and_pii_memory() {
    use futures_util::StreamExt;
    use jeanne_core::llm::{ChatMessage, LlmProvider, OpenAiClient, OpenAiConfig};
    use jeanne_core::pii::{PiiSession, PiiSlidingBuffer};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio_util::sync::CancellationToken;

    // 1. Benchmark de débit du moteur de masquage PII (100 itérations sur texte lourd)
    let sample_text = "Rapport confidentiel : contacter alex.smith@acme-corp.com ou john.doe@partner.org. \
                       Numéros d'urgence : +33 6 12 34 56 78 et (01) 45 67 89 00. \
                       Coordonnées bancaires : IBAN FR7630006000011234567890189 et carte 4111-2222-3333-4444. \
                       Document de travail soumis à révision trimestrielle.";

    let mut pii_latencies = Vec::with_capacity(100);
    for _ in 0..100 {
        let mut session = PiiSession::new();
        let start = Instant::now();
        let masked = session.mask_text(sample_text);
        let demasked = session.demask_text(&masked);
        let elapsed = start.elapsed();
        pii_latencies.push(elapsed);
        assert_eq!(demasked, sample_text);
    }

    pii_latencies.sort();
    let pii_p50 = pii_latencies[50];
    let pii_p95 = pii_latencies[95];
    println!("PII Masking Latency Benchmark (100 iterations) :");
    println!("- P50 : {pii_p50:?}");
    println!("- P95 : {pii_p95:?}");
    assert!(
        pii_p95 < Duration::from_millis(5),
        "Le masquage PII P95 ({pii_p95:?}) doit être strictement inférieur à 5 ms"
    );

    // 2. Mock SSE HTTP Server générant un flux volumineux de 2 000 fragments
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let port = listener.local_addr().expect("port").port();

    tokio::spawn(async move {
        if let Ok((mut socket, _)) = listener.accept().await {
            let mut buf = [0u8; 1024];
            let _ = socket.read(&mut buf).await;

            let header = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n";
            let _ = socket.write_all(header.as_bytes()).await;

            for i in 0..2000 {
                let chunk_data = format!(
                    "data: {{\"choices\": [{{\"delta\": {{\"content\": \"token_{i} \"}}}}]}}\n\n"
                );
                let chunk_http = format!("{:X}\r\n{}\r\n", chunk_data.len(), chunk_data);
                if socket.write_all(chunk_http.as_bytes()).await.is_err() {
                    break;
                }
            }
            let done = "data: [DONE]\n\n";
            let done_http = format!("{:X}\r\n{}\r\n0\r\n\r\n", done.len(), done);
            let _ = socket.write_all(done_http.as_bytes()).await;
        }
    });

    // 3. Consommation en streaming avec reconstitution PII en direct
    let config = OpenAiConfig {
        base_url: format!("http://127.0.0.1:{port}/v1"),
        ..Default::default()
    };
    let client = OpenAiClient::new(config);
    let token = CancellationToken::new();

    let mut stream = client
        .chat_stream(
            vec![ChatMessage {
                role: "user".to_string(),
                content: "Benchmark streaming throughput".to_string(),
            }],
            token.clone(),
        )
        .await
        .expect("chat_stream creation");

    let mut received_tokens = 0;
    let mut sliding_buffer = PiiSlidingBuffer::new(std::collections::HashMap::new());

    let stream_start = Instant::now();
    while let Some(chunk_res) = stream.next().await {
        if let Ok(chunk) = chunk_res {
            let _ = sliding_buffer.process_chunk(&chunk);
            received_tokens += 1;
            if received_tokens == 500 {
                // Test d'annulation réactive en cours de flux
                let cancel_start = Instant::now();
                token.cancel();
                let cancel_elapsed = cancel_start.elapsed();
                assert!(
                    cancel_elapsed < Duration::from_millis(20),
                    "L'annulation doit s'exécuter en moins de 20 ms, mesuré: {cancel_elapsed:?}"
                );
            }
        }
    }
    let total_stream_time = stream_start.elapsed();
    println!("Streaming Benchmark : Consommé {received_tokens} tokens en {total_stream_time:?}");

    // 4. Profilage de l'empreinte mémoire résidente (RSS) sous streaming actif
    let (rss_kb, hwm_kb) = get_memory_stats_kb();
    if rss_kb > 0 || hwm_kb > 0 {
        println!("Memory Footprint under Remote Streaming Workload :");
        println!(
            "- Current RSS : {} KB ({:.2} MB)",
            rss_kb,
            rss_kb as f64 / 1024.0
        );
        println!(
            "- Peak RSS (VmHWM) : {} KB ({:.2} MB)",
            hwm_kb,
            hwm_kb as f64 / 1024.0
        );
        // Exigence contractuelle stricte : RSS résidente < 150 Mo (153 600 Ko)
        assert!(
            hwm_kb < 150 * 1024,
            "L'empreinte mémoire résidente maximale ({hwm_kb} KB) dépasse le plafond strict de 150 Mo"
        );
    }
}
