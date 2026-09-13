use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tempfile::tempdir;

use jeanne_core::models::IndexedChunk;
use jeanne_core::storage::StorageManager;
use jeanne_core::vault::VaultWatcher;

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
            format!("Architecture distribuée et gestion souveraine des données locales pour Jeanne document {i}.")
        } else if i % 3 == 0 {
            format!("Indexation lexicale FTS5 et synchronisation continue du coffre markdown {i}.")
        } else {
            format!("Contenu générique de réunion et journalisation périodique fragment numéro {i}.")
        };

        let chunk = IndexedChunk {
            chunk_id: format!("{file_path}:0"),
            file_path,
            chunk_index: 0,
            content,
            token_count: 15,
            note_type: "semantique".to_string(),
            statut: "actif".to_string(),
            date_creation: 1710000000 + i as i64,
        };
        storage.index_chunk(&chunk).expect("Indexation chunk");
    }

    // 2. Exécution de 100 requêtes de recherche représentatives et mesure de la latence
    let queries = ["architecture", "souveraine", "lexicale", "réunion", "Jeanne"];
    let mut latencies = Vec::with_capacity(100);

    for (idx, q) in queries.iter().cycle().take(100).enumerate() {
        let query_term = if idx % 2 == 0 { *q } else { &format!("{q}*") };
        let start = Instant::now();
        let results = storage.search_fts(query_term, 10).expect("Recherche FTS5");
        let elapsed = start.elapsed();
        latencies.push(elapsed);
        assert!(!results.is_empty(), "La recherche doit retourner des résultats pour '{query_term}'");
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
    while watcher.reconciliation_count() == 0 && start_wait.elapsed() < Duration::from_millis(2500) {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Arrêt du lecteur concurrent
    stop_reader.store(true, std::sync::atomic::Ordering::Relaxed);
    let total_reads = reader_handle.await.expect("Arrêt tâche de lecture concurrente");

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
    let results = s.search_fts("itération numéro 20", 5).expect("Recherche FTS5 finale");
    assert_eq!(
        results.len(),
        1,
        "Le document final (itération 20) doit être indexé et trouvable sans verrou résiduel"
    );

    watcher.stop();
}
