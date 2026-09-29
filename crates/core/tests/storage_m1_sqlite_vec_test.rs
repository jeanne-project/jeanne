//! Milestone 1 Integration Tests: Storage Layer & sqlite-vec Integration
//!
//! Tests real SQLite database interactions using `StorageManager`, `sqlite-vec` virtual table `vec0`,
//! vector serialization, 1-hop link graph traversal, and cascade synchronization.

use jeanne_core::models::{CoalaType, FileLink, IndexedChunk, NoteStatus};
use jeanne_core::storage::StorageManager;
use tempfile::tempdir;

fn make_basis_vector(dim: usize) -> [f32; 384] {
    let mut vec = [0.0f32; 384];
    if dim < 384 {
        vec[dim] = 1.0;
    }
    vec
}

fn make_synthetic_vector(seed: f32) -> [f32; 384] {
    let mut vec = [0.0f32; 384];
    let mut norm_sq = 0.0f32;
    for (i, item) in vec.iter_mut().enumerate() {
        let val = ((i as f32 + seed) * 0.17).sin();
        *item = val;
        norm_sq += val * val;
    }
    let norm = norm_sq.sqrt();
    if norm > 0.0 {
        for val in &mut vec {
            *val /= norm;
        }
    }
    vec
}

#[test]
fn test_m1_01_vector_serialization_and_deserialization_roundtrip() {
    let original = make_synthetic_vector(42.0);
    let bytes = StorageManager::serialize_vector(&original);
    assert_eq!(
        bytes.len(),
        1536,
        "Serialized buffer must be exactly 1,536 bytes"
    );

    let deserialized = StorageManager::deserialize_vector(&bytes);
    for (i, (&orig, &deser)) in original.iter().zip(deserialized.iter()).enumerate() {
        assert_eq!(orig, deser, "Mismatch at dimension {i}: {orig} != {deser}");
    }
}

#[test]
fn test_m1_02_sqlite_vec_identical_vector_cosine_distance_zero() {
    // TEST-02-01: L'insertion et l'interrogation d'un vecteur synthétique 384D dans
    // vec_chunks retourne une distance cosinus de 0.0 pour un vecteur identique.
    let storage = StorageManager::open_in_memory().expect("open in-memory db");
    storage.init_schema().expect("init schema");

    storage
        .upsert_file("Notes/Test.md", "hash_001", 1710000000, None)
        .expect("upsert file");

    let chunk = IndexedChunk {
        id: None,
        chunk_id: "Notes/Test.md:0".to_string(),
        file_path: "Notes/Test.md".to_string(),
        chunk_index: 0,
        content: "Testing sqlite-vec vector integration in Jeanne core.".to_string(),
        token_count: 8,
        coala_type: CoalaType::Semantic,
        status: NoteStatus::Active,
        superseded_by: None,
        deprecated_at: None,
        date_creation: 1710000000,
    };
    let rowid = storage.index_chunk(&chunk).expect("index chunk");
    assert!(rowid > 0, "rowid must be positive");

    let vec_query = make_synthetic_vector(1.23);
    storage
        .insert_chunk_vector(rowid, &vec_query)
        .expect("insert vector");

    let results = storage.search_vector(&vec_query, 5).expect("search vector");
    assert_eq!(results.len(), 1, "Expected 1 nearest neighbor");
    let (found_rowid, distance) = results[0];
    assert_eq!(found_rowid, rowid, "Result rowid must match inserted rowid");
    assert!(
        distance.abs() < 1e-5,
        "Identical vector must have cosine distance 0.0 (got {distance})"
    );
    let cosine_sim = 1.0 - distance;
    assert!(
        (cosine_sim - 1.0).abs() < 1e-5,
        "Cosine similarity must be 1.0 (got {cosine_sim})"
    );
}

#[test]
fn test_m1_03_sqlite_vec_orthogonal_and_antiparallel_distances() {
    let storage = StorageManager::open_in_memory().expect("open in-memory db");
    storage.init_schema().expect("init schema");

    storage
        .upsert_file("Notes/Math.md", "hash_math", 1710000000, None)
        .expect("upsert file");

    let e0 = make_basis_vector(0);
    let e1 = make_basis_vector(1);

    let mut neg_e0 = [0.0f32; 384];
    neg_e0[0] = -1.0;

    let c1 = IndexedChunk::new(
        "math:0",
        "Notes/Math.md",
        0,
        "Basis vector 0",
        3,
        CoalaType::Semantic,
        NoteStatus::Active,
        1710000000,
    );
    let c2 = IndexedChunk::new(
        "math:1",
        "Notes/Math.md",
        1,
        "Basis vector 1",
        3,
        CoalaType::Semantic,
        NoteStatus::Active,
        1710000000,
    );
    let c3 = IndexedChunk::new(
        "math:2",
        "Notes/Math.md",
        2,
        "Negative basis vector 0",
        4,
        CoalaType::Semantic,
        NoteStatus::Active,
        1710000000,
    );

    let r1 = storage.index_chunk(&c1).expect("c1");
    let r2 = storage.index_chunk(&c2).expect("c2");
    let r3 = storage.index_chunk(&c3).expect("c3");

    storage.insert_chunk_vector(r1, &e0).expect("insert e0");
    storage.insert_chunk_vector(r2, &e1).expect("insert e1");
    storage
        .insert_chunk_vector(r3, &neg_e0)
        .expect("insert neg_e0");

    let results = storage.search_vector(&e0, 10).expect("search e0");
    assert_eq!(results.len(), 3);

    // Closest: e0 (distance 0.0)
    assert_eq!(results[0].0, r1);
    assert!(results[0].1.abs() < 1e-5);

    // Next: orthogonal e1 (distance 1.0)
    assert_eq!(results[1].0, r2);
    assert!((results[1].1 - 1.0).abs() < 1e-4);

    // Furthest: anti-parallel -e0 (distance 2.0)
    assert_eq!(results[2].0, r3);
    assert!((results[2].1 - 2.0).abs() < 1e-4);
}

#[test]
fn test_m1_04_vector_upsert_replaces_existing_atomically() {
    let storage = StorageManager::open_in_memory().expect("open in-memory db");
    storage.init_schema().expect("init schema");

    storage
        .upsert_file("Notes/Upsert.md", "hash_up", 1710000000, None)
        .expect("upsert file");

    let chunk = IndexedChunk::new(
        "up:0",
        "Notes/Upsert.md",
        0,
        "Initial content",
        2,
        CoalaType::Semantic,
        NoteStatus::Active,
        1710000000,
    );
    let rowid = storage.index_chunk(&chunk).expect("index chunk");

    let v1 = make_basis_vector(0);
    let v2 = make_basis_vector(5);

    // First insertion
    storage.insert_chunk_vector(rowid, &v1).expect("insert v1");
    let res1 = storage.search_vector(&v1, 5).expect("search v1");
    assert_eq!(res1[0].0, rowid);
    assert!(res1[0].1.abs() < 1e-5);

    // Second insertion on same rowid (upsert replacing prior vector)
    storage.insert_chunk_vector(rowid, &v2).expect("insert v2");
    let res2 = storage.search_vector(&v2, 5).expect("search v2");
    assert_eq!(res2[0].0, rowid);
    assert!(res2[0].1.abs() < 1e-5);

    // Old vector v1 should now be orthogonal to the updated entry
    let res_old = storage.search_vector(&v1, 5).expect("search old v1");
    assert_eq!(res_old[0].0, rowid);
    assert!((res_old[0].1 - 1.0).abs() < 1e-4);

    // Total count in vec_chunks virtual table must remain 1
    let count: i64 = storage
        .raw_connection()
        .query_row("SELECT COUNT(*) FROM vec_chunks;", [], |row| row.get(0))
        .expect("count vec_chunks");
    assert_eq!(count, 1, "Must not create duplicate rows in vec_chunks");
}

#[test]
fn test_m1_05_file_links_and_1hop_supersedes_resolution() {
    // TEST-02-05: La traversée du graphe à 1-hop sur un lien supersedes retourne le document remplaçant actif.
    let storage = StorageManager::open_in_memory().expect("open in-memory db");
    storage.init_schema().expect("init schema");

    let old_file = "Notes/API_v1.md";
    let new_file = "Notes/API_v2.md";

    storage
        .upsert_file(old_file, "h1", 1700000000, None)
        .expect("upsert old");
    storage
        .upsert_file(new_file, "h2", 1710000000, None)
        .expect("upsert new");

    // Insert obsolescence link: new_file supersedes old_file
    storage
        .insert_file_link(new_file, old_file, FileLink::TYPE_SUPERSEDES, 1710000000)
        .expect("insert link");

    // Query 1-hop superseding file for old_file
    let superseding = storage
        .get_superseding_file(old_file)
        .expect("get superseding");
    assert_eq!(
        superseding,
        Some(new_file.to_string()),
        "1-hop traversal must return the successor note"
    );

    // Query superseding for new_file (not superseded)
    let none_superseding = storage
        .get_superseding_file(new_file)
        .expect("get superseding");
    assert_eq!(
        none_superseding, None,
        "Active successor must not have a superseding note"
    );
}

#[test]
fn test_m1_06_cascade_delete_purges_chunks_links_and_vecs() {
    let storage = StorageManager::open_in_memory().expect("open in-memory db");
    storage.init_schema().expect("init schema");

    let file_path = "Notes/DeprecateMe.md";
    storage
        .upsert_file(file_path, "h_del", 1710000000, None)
        .expect("upsert");

    let c1 = IndexedChunk::new(
        "del:0",
        file_path,
        0,
        "Chunk 0 content",
        3,
        CoalaType::Semantic,
        NoteStatus::Active,
        1710000000,
    );
    let c2 = IndexedChunk::new(
        "del:1",
        file_path,
        1,
        "Chunk 1 content",
        3,
        CoalaType::Semantic,
        NoteStatus::Active,
        1710000000,
    );

    let r1 = storage.index_chunk(&c1).expect("c1");
    let r2 = storage.index_chunk(&c2).expect("c2");

    let v1 = make_basis_vector(10);
    let v2 = make_basis_vector(20);
    storage.insert_chunk_vector(r1, &v1).expect("v1");
    storage.insert_chunk_vector(r2, &v2).expect("v2");

    storage
        .insert_file_link(file_path, "Notes/Target.md", "relates", 1710000000)
        .expect("link");

    // Verify initial states
    let conn = storage.raw_connection();
    let chunks_cnt: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM chunks WHERE file_path = ?1;",
            [file_path],
            |r| r.get(0),
        )
        .expect("count chunks");
    assert_eq!(chunks_cnt, 2);

    let links_cnt: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM file_links WHERE source_path = ?1;",
            [file_path],
            |r| r.get(0),
        )
        .expect("count links");
    assert_eq!(links_cnt, 1);

    let vecs_cnt: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM vec_chunks WHERE rowid IN (?1, ?2);",
            rusqlite::params![r1, r2],
            |r| r.get(0),
        )
        .expect("count vecs");
    assert_eq!(vecs_cnt, 2);

    // DELETE FILE
    storage.delete_file(file_path).expect("delete file");

    // Assert chunks CASCADE purged
    let post_chunks: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM chunks WHERE file_path = ?1;",
            [file_path],
            |r| r.get(0),
        )
        .expect("count chunks post");
    assert_eq!(post_chunks, 0, "Chunks must be deleted by CASCADE");

    // Assert file_links CASCADE purged
    let post_links: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM file_links WHERE source_path = ?1;",
            [file_path],
            |r| r.get(0),
        )
        .expect("count links post");
    assert_eq!(post_links, 0, "file_links must be deleted by CASCADE");

    // Assert vec_chunks purged via trigger chunks_vd and defensive query
    let post_vecs: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM vec_chunks WHERE rowid IN (?1, ?2);",
            rusqlite::params![r1, r2],
            |r| r.get(0),
        )
        .expect("count vecs post");
    assert_eq!(post_vecs, 0, "vec_chunks must be purged on file deletion");
}

#[test]
fn test_m1_07_on_disk_persistence_and_reopen() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("persist_test.db");

    let v_stored = make_synthetic_vector(7.89);
    let stored_rowid: i64;

    // Scope 1: Write and close
    {
        let storage = StorageManager::open(&db_path).expect("open on-disk");
        storage.init_schema().expect("init");

        storage
            .upsert_file("Notes/Persistent.md", "hash_p", 1710000000, None)
            .expect("upsert");
        let chunk = IndexedChunk::new(
            "p:0",
            "Notes/Persistent.md",
            0,
            "Persistent note content",
            3,
            CoalaType::Procedural,
            NoteStatus::Active,
            1710000000,
        );
        stored_rowid = storage.index_chunk(&chunk).expect("index");
        storage
            .insert_chunk_vector(stored_rowid, &v_stored)
            .expect("insert vec");
    }

    // Scope 2: Reopen and verify retrieval
    {
        let storage = StorageManager::open(&db_path).expect("reopen on-disk");
        let results = storage
            .search_vector(&v_stored, 5)
            .expect("search reopened");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, stored_rowid);
        assert!(
            results[0].1.abs() < 1e-5,
            "Persisted vector cosine distance must be 0.0"
        );
    }
}

#[test]
fn test_m1_08_schema_migration_backward_compatibility() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("migration_test.db");

    // Create legacy database with old schema (without 'id' and with old column names)
    {
        let conn = rusqlite::Connection::open(&db_path).expect("open raw");
        conn.execute_batch(
            r#"
            CREATE TABLE files (
                file_path TEXT PRIMARY KEY,
                file_hash TEXT NOT NULL,
                last_modified INTEGER NOT NULL,
                frontmatter_json TEXT
            );

            CREATE TABLE chunks (
                chunk_id TEXT PRIMARY KEY,
                file_path TEXT NOT NULL,
                chunk_index INTEGER NOT NULL,
                content TEXT NOT NULL,
                token_count INTEGER NOT NULL,
                note_type TEXT NOT NULL,
                statut TEXT NOT NULL,
                date_creation INTEGER NOT NULL,
                FOREIGN KEY(file_path) REFERENCES files(file_path) ON DELETE CASCADE
            );

            INSERT INTO files VALUES ('Notes/Old.md', 'hash_old', 1700000000, NULL);
            INSERT INTO chunks VALUES ('Notes/Old.md:0', 'Notes/Old.md', 0, 'Legacy content', 2, 'semantique', 'actif', 1700000000);
            "#,
        )
        .expect("init legacy schema");
    }

    // Open via StorageManager which will run migrate_schema_if_needed()
    let storage = StorageManager::open(&db_path).expect("open via StorageManager");
    storage.init_schema().expect("init schema runs migration");

    let conn = storage.raw_connection();

    // Verify 'id' exists and is populated
    let row: (i64, String, String, String) = conn
        .query_row(
            "SELECT id, chunk_id, coala_type, status FROM chunks WHERE chunk_id = 'Notes/Old.md:0';",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .expect("query migrated row");

    assert!(row.0 > 0, "Migrated row must have positive integer id");
    assert_eq!(row.1, "Notes/Old.md:0");
    assert_eq!(row.2, "semantique");
    assert_eq!(row.3, "actif");
}
