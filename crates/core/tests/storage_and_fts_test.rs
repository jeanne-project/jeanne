use tempfile::tempdir;
use jeanne_core::models::IndexedChunk;
use jeanne_core::parser::parse_markdown;
use jeanne_core::storage::StorageManager;
use jeanne_core::vault::resolve_vault_path;

#[test]
fn test_01_01_sqlite_wal_init_and_schema() {
    let dir = tempdir().expect("Impossible de créer le dossier temporaire");
    let db_path = dir.path().join("test_database.db");

    let storage = StorageManager::open(&db_path).expect("Échec d'ouverture de la base");
    storage.init_schema().expect("Échec d'initialisation du schéma");

    let conn = storage.raw_connection();

    // 1. Vérification des PRAGMAs
    let journal_mode: String = conn
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .expect("Échec lecture journal_mode");
    assert_eq!(journal_mode.to_lowercase(), "wal", "Le mode journal doit être WAL");

    let foreign_keys: i32 = conn
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .expect("Échec lecture foreign_keys");
    assert_eq!(foreign_keys, 1, "Les clés étrangères doivent être activées (foreign_keys = ON)");

    // 2. Vérification des tables
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .expect("Échec préparation statement tables");
    let tables: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .expect("Échec requête tables")
        .collect::<Result<Vec<String>, _>>()
        .expect("Échec collecte tables");

    assert!(tables.contains(&"files".to_string()), "La table 'files' doit exister");
    assert!(tables.contains(&"chunks".to_string()), "La table 'chunks' doit exister");
    assert!(tables.contains(&"fts_notes".to_string()), "La table virtuelle 'fts_notes' doit exister");

    // 3. Vérification des triggers
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='trigger' ORDER BY name")
        .expect("Échec préparation statement triggers");
    let triggers: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .expect("Échec requête triggers")
        .collect::<Result<Vec<String>, _>>()
        .expect("Échec collecte triggers");

    assert!(triggers.contains(&"chunks_ai".to_string()), "Le trigger 'chunks_ai' doit exister");
    assert!(triggers.contains(&"chunks_ad".to_string()), "Le trigger 'chunks_ad' doit exister");
    assert!(triggers.contains(&"chunks_au".to_string()), "Le trigger 'chunks_au' doit exister");
}

#[test]
fn test_01_02_frontmatter_extraction_gray_matter() {
    let markdown_content = r#"---
id: "note_20260912_103000"
title: "Technical Architecture Review"
date_creation: "2026-09-12T10:30:00Z"
date_modification: "2026-09-12T10:30:00Z"
type: "semantique"
statut: "actif"
tags:
  - "architecture"
  - "review"
source_media: "Attachments/meeting_20260912.wav"
---

# Introduction

Ceci est le corps de la note markdown.
Il contient des concepts clés sur l'architecture File-over-App.
"#;

    let (frontmatter, body) = parse_markdown(markdown_content).expect("Échec de parsing markdown");

    assert_eq!(frontmatter.id, "note_20260912_103000");
    assert_eq!(frontmatter.title, "Technical Architecture Review");
    assert_eq!(frontmatter.date_creation, "2026-09-12T10:30:00Z");
    assert_eq!(frontmatter.date_modification, "2026-09-12T10:30:00Z");
    assert_eq!(frontmatter.note_type, "semantique");
    assert_eq!(frontmatter.statut, "actif");
    assert_eq!(frontmatter.tags, vec!["architecture".to_string(), "review".to_string()]);
    assert_eq!(frontmatter.source_media, Some("Attachments/meeting_20260912.wav".to_string()));

    assert!(body.starts_with("# Introduction"), "Le corps doit commencer par le titre sans le frontmatter");
    assert!(body.contains("Ceci est le corps de la note markdown."));

    // Test avec des champs optionnels manquants pour valider les valeurs par défaut
    let minimal_content = r#"---
title: "Minimal Note"
---
Corps minimal.
"#;
    let (minimal_frontmatter, minimal_body) = parse_markdown(minimal_content).expect("Échec parsing minimal");
    assert_eq!(minimal_frontmatter.title, "Minimal Note");
    assert_eq!(minimal_frontmatter.note_type, "semantique");
    assert_eq!(minimal_frontmatter.statut, "actif");
    assert!(minimal_frontmatter.tags.is_empty());
    assert_eq!(minimal_frontmatter.source_media, None);
    assert_eq!(minimal_body.trim(), "Corps minimal.");
}

#[test]
fn test_01_03_indexing_bm25_search_and_cascade_delete() {
    let dir = tempdir().expect("Impossible de créer le dossier temporaire");
    let db_path = dir.path().join("test_fts.db");

    let storage = StorageManager::open(&db_path).expect("Échec d'ouverture");
    storage.init_schema().expect("Échec init schéma");

    let conn = storage.raw_connection();

    // Insertion d'un fichier parent dans 'files' pour respecter la clé étrangère
    conn.execute(
        "INSERT INTO files (file_path, file_hash, last_modified, frontmatter_json) VALUES (?1, ?2, ?3, ?4)",
        (
            "Notes/Architecture.md",
            "hash_123456",
            1710000000i64,
            "{}",
        ),
    ).expect("Échec insertion fichier");

    let chunk = IndexedChunk {
        chunk_id: "chunk_arch_01".to_string(),
        file_path: "Notes/Architecture.md".to_string(),
        chunk_index: 0,
        content: "Jeanne implémente une architecture souveraine File-over-App garantissant la pérennité des données.".to_string(),
        token_count: 14,
        note_type: "semantique".to_string(),
        statut: "actif".to_string(),
        date_creation: 1710000000,
    };

    storage.index_chunk(&chunk).expect("Échec indexation chunk");

    // 1. Recherche FTS5 BM25
    let results = storage.search_fts("souveraine", 10).expect("Échec recherche FTS");
    assert_eq!(results.len(), 1, "La recherche doit trouver 1 résultat");
    assert_eq!(results[0].chunk_id, "chunk_arch_01");
    assert!(results[0].snippet.contains("souveraine") || results[0].snippet.contains("<mark>"), "Le snippet doit être généré");

    // 2. Suppression du chunk et vérification du trigger chunks_ad
    storage.delete_chunk("chunk_arch_01").expect("Échec suppression chunk");

    let fts_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM fts_notes WHERE chunk_id = ?1", ["chunk_arch_01"], |row| row.get(0))
        .expect("Échec comptage fts_notes");
    assert_eq!(fts_count, 0, "Le trigger chunks_ad doit avoir supprimé le chunk de fts_notes");

    let post_delete_results = storage.search_fts("souveraine", 10).expect("Échec recherche post suppression");
    assert!(post_delete_results.is_empty(), "La recherche FTS ne doit plus retourner de résultat après suppression");
}

#[test]
fn test_01_04_vault_path_resolution_priority() {
    let temp_vault = tempdir().expect("Impossible de créer le vault temporaire");
    let custom_path = temp_vault.path().to_path_buf();

    // 1. Priorité à JEANNE_VAULT_PATH si définie
    unsafe {
        std::env::set_var("JEANNE_VAULT_PATH", custom_path.to_str().unwrap());
    }

    let resolved = resolve_vault_path().expect("Échec résolution vault avec variable");
    assert_eq!(resolved, custom_path, "La priorité doit être accordée à JEANNE_VAULT_PATH");

    // 2. Fallback vers le dossier utilisateur si non définie
    unsafe {
        std::env::remove_var("JEANNE_VAULT_PATH");
    }

    let fallback_resolved = resolve_vault_path().expect("Échec résolution fallback");
    assert!(
        fallback_resolved.ends_with("JeanneVault") || fallback_resolved.ends_with("JeanneVault/"),
        "Le fallback doit se terminer par JeanneVault : {:?}", fallback_resolved
    );
}
