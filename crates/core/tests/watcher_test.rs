use std::sync::{Arc, Mutex};
use std::time::Duration;
use jeanne_core::storage::StorageManager;
use jeanne_core::vault::VaultWatcher;
use tempfile::tempdir;

/// TEST-01-03: Dé-rebond du Watcher FS (300 ms)
/// Objectif : Vérifier que 10 modifications rapides en moins de 100 ms sont regroupées
/// et ne déclenchent exactement qu'une seule transaction d'indexation après la fenêtre de calme de 300 ms.
#[tokio::test]
async fn test_01_03_watcher_debouncing_window() {
    let temp_vault = tempdir().expect("Création répertoire temporaire");
    let vault_path = temp_vault.path().to_path_buf();

    let db_path = vault_path.join(".jeanne").join("database.db");
    let storage = StorageManager::open(&db_path).expect("Ouverture StorageManager");
    storage.init_schema().expect("Initialisation schéma SQLite");
    let storage = Arc::new(Mutex::new(storage));

    let mut watcher = VaultWatcher::new(&vault_path, storage.clone())
        .expect("Initialisation VaultWatcher")
        .with_debounce_duration(Duration::from_millis(300));

    watcher.start().expect("Démarrage du watcher");

    // 1. Écrire rapidement 10 modifications dans un fichier Markdown en < 100 ms
    let note_path = vault_path.join("RapidNote.md");
    for i in 1..=10 {
        let content = format!(
            "---\nid: note-rapid\ntitle: Note Rapide\ndate_creation: \"2026-09-13T10:00:00Z\"\ndate_modification: \"2026-09-13T10:00:00Z\"\nnote_type: semantique\nstatut: actif\ntags:\n  - test\n---\n\nContenu itération numéro {}\n",
            i
        );
        std::fs::write(&note_path, content).expect("Écriture fichier note");
        tokio::time::sleep(Duration::from_millis(8)).await;
    }

    // 2. Attendre l'expiration de la fenêtre de dé-rebond glissante (300 ms + marge de 150 ms)
    tokio::time::sleep(Duration::from_millis(450)).await;

    // 3. Assertions contractuelles de la matrice TEST-01-03
    // Assertion 1 : Exactement 1 transaction d'indexation doit avoir été exécutée
    assert_eq!(
        watcher.reconciliation_count(),
        1,
        "Le dé-rebond doit agréger les événements et exécuter exactement 1 transaction d'indexation"
    );

    // Assertion 2 : Le contenu final (itération 10) doit être présent et indexé dans FTS5
    let s = storage.lock().unwrap();
    let results = s.search_fts("numéro 10", 10).expect("Recherche FTS5");
    assert_eq!(
        results.len(),
        1,
        "L'index FTS5 doit contenir le contenu de la dernière itération enregistrée"
    );

    watcher.stop();
}

/// TEST-01-04: Réconciliation et suppression en cascade lors de la suppression d'un fichier source
/// Objectif : Vérifier que lorsqu'un fichier Markdown est supprimé du disque, le watcher le détecte,
/// supprime l'entrée dans 'files', ce qui cascade sur 'chunks' et purge 'fts_notes' via le trigger chunks_ad.
#[tokio::test]
async fn test_01_04_cascade_deletion_on_file_removal() {
    let temp_vault = tempdir().expect("Création répertoire temporaire");
    let vault_path = temp_vault.path().to_path_buf();

    let db_path = vault_path.join(".jeanne").join("database.db");
    let storage = StorageManager::open(&db_path).expect("Ouverture StorageManager");
    storage.init_schema().expect("Initialisation schéma SQLite");
    let storage = Arc::new(Mutex::new(storage));

    let mut watcher = VaultWatcher::new(&vault_path, storage.clone())
        .expect("Initialisation VaultWatcher")
        .with_debounce_duration(Duration::from_millis(300));

    watcher.start().expect("Démarrage du watcher");

    // 1. Créer un fichier dans le coffre
    let note_path = vault_path.join("ToDelete.md");
    let initial_content = r#"---
id: note-to-delete
title: Note Éphémère
date_creation: "2026-09-13T10:00:00Z"
date_modification: "2026-09-13T10:00:00Z"
note_type: semantique
statut: actif
tags:
  - suppression
---

Ce motclefspecifique doit disparaître complètement lors de la suppression du fichier source.
"#;
    std::fs::write(&note_path, initial_content).expect("Création fichier initial");

    // Attendre l'indexation initiale par le watcher
    tokio::time::sleep(Duration::from_millis(450)).await;

    // Vérifier la présence initiale dans FTS5
    {
        let s = storage.lock().unwrap();
        let initial_results = s
            .search_fts("motclefspecifique", 5)
            .expect("Recherche initiale FTS5");
        assert_eq!(
            initial_results.len(),
            1,
            "Le fichier créé doit être indexé et trouvable dans FTS5"
        );
    }

    // 2. Supprimer le fichier source du disque
    std::fs::remove_file(&note_path).expect("Suppression du fichier source");

    // Attendre la détection et réconciliation par le watcher
    tokio::time::sleep(Duration::from_millis(450)).await;

    // 3. Assertions contractuelles de la matrice TEST-01-04
    {
        let s = storage.lock().unwrap();
        let conn = s.raw_connection();

        // Vérification de la purge dans la table `files`
        let file_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM files WHERE file_path = 'ToDelete.md'",
                [],
                |row| row.get(0),
            )
            .expect("Comptage dans table files");
        assert_eq!(
            file_count, 0,
            "L'entrée dans la table 'files' doit être supprimée lors de la réconciliation"
        );

        // Vérification de la cascade sur `chunks`
        let chunk_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE file_path = 'ToDelete.md'",
                [],
                |row| row.get(0),
            )
            .expect("Comptage dans table chunks");
        assert_eq!(
            chunk_count, 0,
            "Tous les fragments associés dans 'chunks' doivent être purgés par la clé étrangère ON DELETE CASCADE"
        );

        // Vérification de la purge dans l'index FTS5 (trigger chunks_ad)
        let fts_results = s
            .search_fts("motclefspecifique", 5)
            .expect("Recherche après suppression");
        assert!(
            fts_results.is_empty(),
            "La table virtuelle FTS5 ne doit plus retourner de résultat pour le document supprimé"
        );
    }

    watcher.stop();
}
