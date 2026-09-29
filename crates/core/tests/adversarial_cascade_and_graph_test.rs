//! Adversarial Challenge Test Suite: Cascade Deletion, Orphan Vectors & Graph Traversal
//!
//! Written by Challenger M1 #2 to stress-test:
//! 1. Cascade deletion with multi-chunk files (orphan rows in chunks, file_links, fts_notes, vec_chunks).
//! 2. Circular and self-referencing links in 1-hop traversal.
//! 3. Non-existent target links and deleting non-existent files.
//! 4. High-churn insert-delete-reindex cycles.

use jeanne_core::models::{CoalaType, FileLink, IndexedChunk, NoteStatus};
use jeanne_core::storage::StorageManager;
use tempfile::tempdir;

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
fn test_adv_01_cascade_multi_chunk_deletion_and_zero_orphan_rows() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("cascade_adv.db");
    let storage = StorageManager::open(&db_path).expect("open storage");
    storage.init_schema().expect("init schema");

    let file_path = "Notes/DeepKnowledge.md";
    storage
        .upsert_file(file_path, "hash_knowledge_001", 1715000000, None)
        .expect("upsert file");

    let mut rowids = Vec::new();
    let num_chunks = 30;

    for i in 0..num_chunks {
        let chunk = IndexedChunk {
            id: None,
            chunk_id: format!("knowledge_chunk_{i}"),
            file_path: file_path.to_string(),
            chunk_index: i,
            content: format!(
                "Adversarial test content section {i} regarding quantum computing paradigms"
            ),
            token_count: 10,
            coala_type: CoalaType::Semantic,
            status: NoteStatus::Active,
            superseded_by: None,
            deprecated_at: None,
            date_creation: 1715000000,
        };
        let rowid = storage.index_chunk(&chunk).expect("index chunk");
        rowids.push(rowid);

        let vec = make_synthetic_vector(i as f32 * 10.0);
        storage
            .insert_chunk_vector(rowid, &vec)
            .expect("insert vector");
    }

    // Add multiple file links
    storage
        .insert_file_link(
            file_path,
            "Notes/Target1.md",
            FileLink::TYPE_WIKILINK,
            1715000000,
        )
        .expect("link 1");
    storage
        .insert_file_link(
            file_path,
            "Notes/Target2.md",
            FileLink::TYPE_SUPERSEDES,
            1715000000,
        )
        .expect("link 2");
    storage
        .insert_file_link(file_path, "Notes/Target3.md", "relates", 1715000000)
        .expect("link 3");

    // Verify pre-deletion counts
    let conn = storage.raw_connection();

    let chunks_cnt: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM chunks WHERE file_path = ?1",
            [file_path],
            |r| r.get(0),
        )
        .expect("count chunks");
    assert_eq!(
        chunks_cnt, num_chunks as i64,
        "All chunks must exist before deletion"
    );

    let links_cnt: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM file_links WHERE source_path = ?1",
            [file_path],
            |r| r.get(0),
        )
        .expect("count links");
    assert_eq!(links_cnt, 3, "All links must exist before deletion");

    let vecs_cnt: i64 = conn
        .query_row("SELECT COUNT(*) FROM vec_chunks", [], |r| r.get(0))
        .expect("count vec_chunks");
    assert_eq!(
        vecs_cnt, num_chunks as i64,
        "All vectors must exist before deletion"
    );

    let fts_cnt: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM fts_notes WHERE file_path = ?1",
            [file_path],
            |r| r.get(0),
        )
        .expect("count fts_notes");
    assert_eq!(
        fts_cnt, num_chunks as i64,
        "All fts_notes must exist before deletion"
    );

    // FTS search should return matches
    let search_pre = storage.search_fts("quantum", 10).expect("search pre");
    assert!(
        !search_pre.is_empty(),
        "FTS must find quantum before deletion"
    );

    // ACT: Delete the file
    storage.delete_file(file_path).expect("delete_file");

    // ASSERT: Zero orphan rows in ALL tables
    let post_files: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM files WHERE file_path = ?1",
            [file_path],
            |r| r.get(0),
        )
        .expect("count files post");
    assert_eq!(post_files, 0, "File row must be deleted");

    let post_chunks: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM chunks WHERE file_path = ?1",
            [file_path],
            |r| r.get(0),
        )
        .expect("count chunks post");
    assert_eq!(post_chunks, 0, "No chunks may remain for deleted file");

    let post_links: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM file_links WHERE source_path = ?1",
            [file_path],
            |r| r.get(0),
        )
        .expect("count links post");
    assert_eq!(
        post_links, 0,
        "No file_links may remain with source_path of deleted file"
    );

    let post_vecs: i64 = conn
        .query_row("SELECT COUNT(*) FROM vec_chunks", [], |r| r.get(0))
        .expect("count vecs post");
    assert_eq!(post_vecs, 0, "No orphan vectors may remain in vec_chunks");

    let post_fts: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM fts_notes WHERE file_path = ?1",
            [file_path],
            |r| r.get(0),
        )
        .expect("count fts post");
    assert_eq!(
        post_fts, 0,
        "CRITICAL: No orphan entries may remain in fts_notes after file deletion!"
    );

    let search_post = storage.search_fts("quantum", 10).expect("search post");
    assert!(
        search_post.is_empty(),
        "FTS search must return empty post deletion"
    );
}

#[test]
fn test_adv_02_circular_and_self_referencing_supersedes_links() {
    let storage = StorageManager::open_in_memory().expect("open memory");
    storage.init_schema().expect("init schema");

    // 1. Two-way cycle: A supersedes B, B supersedes A
    // (e.g. mutual conflict or edit war in notes)
    storage
        .insert_file_link("NoteA.md", "NoteB.md", FileLink::TYPE_SUPERSEDES, 100)
        .expect("link A supersedes B");
    storage
        .insert_file_link("NoteB.md", "NoteA.md", FileLink::TYPE_SUPERSEDES, 200)
        .expect("link B supersedes A");

    // Query superseding for NoteB: should find NoteA (who supersedes B)
    let b_successor = storage.get_superseding_file("NoteB.md").expect("get B");
    assert_eq!(b_successor, Some("NoteA.md".to_string()));

    // Query superseding for NoteA: should find NoteB (who supersedes A)
    let a_successor = storage.get_superseding_file("NoteA.md").expect("get A");
    assert_eq!(a_successor, Some("NoteB.md".to_string()));

    // 2. Self-referencing supersedes: NoteSelf.md supersedes NoteSelf.md
    storage
        .insert_file_link("NoteSelf.md", "NoteSelf.md", FileLink::TYPE_SUPERSEDES, 300)
        .expect("link self");

    let self_successor = storage
        .get_superseding_file("NoteSelf.md")
        .expect("get self");
    assert_eq!(
        self_successor,
        Some("NoteSelf.md".to_string()),
        "Self-reference terminates cleanly"
    );

    // 3. Three-node cycle: X -> Y -> Z -> X
    storage
        .insert_file_link("NodeX.md", "NodeY.md", FileLink::TYPE_SUPERSEDES, 401)
        .expect("link X supersedes Y");
    storage
        .insert_file_link("NodeY.md", "NodeZ.md", FileLink::TYPE_SUPERSEDES, 402)
        .expect("link Y supersedes Z");
    storage
        .insert_file_link("NodeZ.md", "NodeX.md", FileLink::TYPE_SUPERSEDES, 403)
        .expect("link Z supersedes X");

    // Y's successor is X (X supersedes Y)
    assert_eq!(
        storage.get_superseding_file("NodeY.md").expect("Y"),
        Some("NodeX.md".to_string())
    );
    // Z's successor is Y (Y supersedes Z)
    assert_eq!(
        storage.get_superseding_file("NodeZ.md").expect("Z"),
        Some("NodeY.md".to_string())
    );
    // X's successor is Z (Z supersedes X)
    assert_eq!(
        storage.get_superseding_file("NodeX.md").expect("X"),
        Some("NodeZ.md".to_string())
    );
}

#[test]
fn test_adv_03_non_existent_target_links_and_non_existent_file_deletion() {
    let storage = StorageManager::open_in_memory().expect("open memory");
    storage.init_schema().expect("init schema");

    // 1. Link to non-existent target path
    let ghost_target = "Notes/GhostNeverCreated.md";
    let real_source = "Notes/ExistingNote.md";

    storage
        .insert_file_link(
            real_source,
            ghost_target,
            FileLink::TYPE_SUPERSEDES,
            1716000000,
        )
        .expect("insert link to non-existent target");

    // Query 1-hop for ghost target
    let resolved = storage
        .get_superseding_file(ghost_target)
        .expect("query ghost");
    assert_eq!(
        resolved,
        Some(real_source.to_string()),
        "Should return the source document that claims to supersede the ghost"
    );

    // Query 1-hop for a completely unknown file
    let unknown = storage
        .get_superseding_file("CompletelyRandom.md")
        .expect("query unknown");
    assert_eq!(
        unknown, None,
        "Unknown note should have no superseding file"
    );

    // 2. Delete non-existent file path: must be a safe, idempotent no-op
    let delete_noop = storage.delete_file("NonExistent/File/Path.md");
    assert!(
        delete_noop.is_ok(),
        "Deleting a non-existent file must not error"
    );

    // Ensure database remained intact
    let conn = storage.raw_connection();
    let file_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        file_count, 1,
        "Real source file created during link insertion must still exist"
    );
}

#[test]
fn test_adv_04_churn_and_reindexing_stress() {
    let storage = StorageManager::open_in_memory().expect("open memory");
    storage.init_schema().expect("init schema");

    let file_path = "Notes/Churn.md";

    // 10 cycles of inserting 10 chunks, querying vectors, deleting, and re-inserting
    for cycle in 0..10 {
        storage
            .upsert_file(
                file_path,
                &format!("hash_{cycle}"),
                1710000000 + cycle,
                None,
            )
            .expect("upsert file");

        let mut rowids = Vec::new();
        for i in 0..10 {
            let chunk = IndexedChunk {
                id: None,
                chunk_id: format!("churn_{cycle}_{i}"),
                file_path: file_path.to_string(),
                chunk_index: i,
                content: format!("Stress churn text cycle {cycle} chunk {i}"),
                token_count: 5,
                coala_type: CoalaType::Semantic,
                status: NoteStatus::Active,
                superseded_by: None,
                deprecated_at: None,
                date_creation: 1710000000 + cycle,
            };
            let rowid = storage.index_chunk(&chunk).expect("index");
            rowids.push(rowid);

            let v = make_synthetic_vector((cycle * 10 + i as i64) as f32);
            storage
                .insert_chunk_vector(rowid, &v)
                .expect("insert vector");
        }

        // Verify counts
        let conn = storage.raw_connection();
        let chunks_cnt: i64 = conn
            .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(chunks_cnt, 10);
        let vecs_cnt: i64 = conn
            .query_row("SELECT COUNT(*) FROM vec_chunks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(vecs_cnt, 10);
        let fts_cnt: i64 = conn
            .query_row("SELECT COUNT(*) FROM fts_notes", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fts_cnt, 10);

        // Delete file
        storage.delete_file(file_path).expect("delete file");

        // Verify zero leftovers
        let chunks_post: i64 = conn
            .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(chunks_post, 0, "Cycle {cycle}: chunks must be 0");
        let vecs_post: i64 = conn
            .query_row("SELECT COUNT(*) FROM vec_chunks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(vecs_post, 0, "Cycle {cycle}: vec_chunks must be 0");
        let fts_post: i64 = conn
            .query_row("SELECT COUNT(*) FROM fts_notes", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fts_post, 0, "Cycle {cycle}: fts_notes must be 0");
    }
}

#[test]
fn test_adv_05_multi_file_isolation_cascade_deletion() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("multi_file_adv.db");
    let storage = StorageManager::open(&db_path).expect("open storage");
    storage.init_schema().expect("init schema");

    let file_a = "Notes/FileA.md";
    let file_b = "Notes/FileB.md";
    let file_c = "Notes/FileC.md";

    storage
        .upsert_file(file_a, "hash_a", 1700000000, None)
        .unwrap();
    storage
        .upsert_file(file_b, "hash_b", 1700000000, None)
        .unwrap();
    storage
        .upsert_file(file_c, "hash_c", 1700000000, None)
        .unwrap();

    let populate_file = |storage: &StorageManager, path: &str, count: usize, tag: &str| {
        for i in 0..count {
            let chunk = IndexedChunk {
                id: None,
                chunk_id: format!("{tag}_chunk_{i}"),
                file_path: path.to_string(),
                chunk_index: i,
                content: format!("Content unique to {tag} section {i}"),
                token_count: 5,
                coala_type: CoalaType::Semantic,
                status: NoteStatus::Active,
                superseded_by: None,
                deprecated_at: None,
                date_creation: 1700000000,
            };
            let rowid = storage.index_chunk(&chunk).unwrap();
            let v = make_synthetic_vector((i + 1) as f32);
            storage.insert_chunk_vector(rowid, &v).unwrap();
        }
    };

    populate_file(&storage, file_a, 40, "alpha");
    populate_file(&storage, file_b, 30, "beta");
    populate_file(&storage, file_c, 20, "gamma");

    storage
        .insert_file_link(file_a, file_b, "relates", 100)
        .unwrap();
    storage
        .insert_file_link(file_b, file_c, "supersedes", 200)
        .unwrap();
    storage
        .insert_file_link(file_c, file_a, "wikilink", 300)
        .unwrap();

    let conn = storage.raw_connection();

    // Verify initial global counts: 90 chunks, 90 vectors, 90 fts, 3 links, 3 files
    let total_chunks: i64 = conn
        .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
        .unwrap();
    assert_eq!(total_chunks, 90);
    let total_vecs: i64 = conn
        .query_row("SELECT COUNT(*) FROM vec_chunks", [], |r| r.get(0))
        .unwrap();
    assert_eq!(total_vecs, 90);
    let total_fts: i64 = conn
        .query_row("SELECT COUNT(*) FROM fts_notes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(total_fts, 90);
    let total_links: i64 = conn
        .query_row("SELECT COUNT(*) FROM file_links", [], |r| r.get(0))
        .unwrap();
    assert_eq!(total_links, 3);

    // ACT: Delete ONLY file B (30 chunks)
    storage.delete_file(file_b).expect("delete file B");

    // ASSERT: Total counts should drop by exactly 30 chunks, 30 vecs, 30 fts, 1 link (where B was source)
    let post_chunks: i64 = conn
        .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
        .unwrap();
    assert_eq!(post_chunks, 60, "Must be exactly 40 (A) + 20 (C)");

    let post_vecs: i64 = conn
        .query_row("SELECT COUNT(*) FROM vec_chunks", [], |r| r.get(0))
        .unwrap();
    assert_eq!(post_vecs, 60, "vec_chunks must have 60 rows left");

    let post_fts: i64 = conn
        .query_row("SELECT COUNT(*) FROM fts_notes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(post_fts, 60, "fts_notes must have 60 rows left");

    // File B chunks specifically:
    let b_chunks: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM chunks WHERE file_path = ?1",
            [file_b],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(b_chunks, 0);

    // File A chunks specifically:
    let a_chunks: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM chunks WHERE file_path = ?1",
            [file_a],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(a_chunks, 40);

    // File C chunks specifically:
    let c_chunks: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM chunks WHERE file_path = ?1",
            [file_c],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(c_chunks, 20);

    // FTS isolation:
    let beta_res = storage.search_fts("beta", 10).unwrap();
    assert!(
        beta_res.is_empty(),
        "Deleted file's content must not appear in FTS"
    );

    let alpha_res = storage.search_fts("alpha", 10).unwrap();
    assert_eq!(
        alpha_res.len(),
        10,
        "Alpha file's content must remain searchable"
    );

    let gamma_res = storage.search_fts("gamma", 10).unwrap();
    assert_eq!(
        gamma_res.len(),
        10,
        "Gamma file's content must remain searchable"
    );
}

#[test]
fn test_adv_06_dangling_target_link_retention_and_deletion() {
    let storage = StorageManager::open_in_memory().expect("open memory");
    storage.init_schema().expect("init schema");

    let src = "Notes/Arch_v2.md";
    let tgt = "Notes/Arch_v1.md";

    storage.upsert_file(src, "h2", 1700000000, None).unwrap();
    storage.upsert_file(tgt, "h1", 1600000000, None).unwrap();

    // Arch_v2 supersedes Arch_v1
    storage
        .insert_file_link(src, tgt, FileLink::TYPE_SUPERSEDES, 1700000000)
        .unwrap();

    // 1. Delete target file Arch_v1 (the obsolete file physically deleted)
    storage.delete_file(tgt).expect("delete target file");

    // 1-hop traversal should still find that Arch_v2 is the successor of Arch_v1
    let successor = storage.get_superseding_file(tgt).unwrap();
    assert_eq!(
        successor,
        Some(src.to_string()),
        "Even when target file is physically deleted, successor relation must hold"
    );

    // 2. Delete source file Arch_v2
    storage.delete_file(src).expect("delete source file");

    // The link should now be cascade-purged because source_path was deleted
    let conn = storage.raw_connection();
    let link_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM file_links", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        link_count, 0,
        "Link must be cascade deleted when source is deleted"
    );

    let successor_after = storage.get_superseding_file(tgt).unwrap();
    assert_eq!(
        successor_after, None,
        "No successor after both files deleted"
    );
}

#[test]
fn test_adv_07_chunks_without_vectors_and_unindexed_vectors() {
    let storage = StorageManager::open_in_memory().expect("open memory");
    storage.init_schema().expect("init schema");

    let file_path = "Notes/Partial.md";
    storage
        .upsert_file(file_path, "h_part", 1700000000, None)
        .unwrap();

    // Insert 5 chunks: 3 with vectors, 2 without vectors
    let mut rowids = Vec::new();
    for i in 0..5 {
        let chunk = IndexedChunk {
            id: None,
            chunk_id: format!("part_chunk_{i}"),
            file_path: file_path.to_string(),
            chunk_index: i,
            content: format!("Partial chunk {i}"),
            token_count: 3,
            coala_type: CoalaType::Semantic,
            status: NoteStatus::Active,
            superseded_by: None,
            deprecated_at: None,
            date_creation: 1700000000,
        };
        let rowid = storage.index_chunk(&chunk).unwrap();
        rowids.push(rowid);

        if i < 3 {
            let v = make_synthetic_vector(i as f32);
            storage.insert_chunk_vector(rowid, &v).unwrap();
        }
    }

    // Insert an orphan vector directly into vec_chunks with rowid not matching any chunk
    let orphan_rowid = 888888i64;
    let orphan_vec = make_synthetic_vector(99.0);
    storage
        .insert_chunk_vector(orphan_rowid, &orphan_vec)
        .unwrap();

    let conn = storage.raw_connection();
    let initial_vecs: i64 = conn
        .query_row("SELECT COUNT(*) FROM vec_chunks", [], |r| r.get(0))
        .unwrap();
    assert_eq!(initial_vecs, 4, "3 chunk vectors + 1 orphan vector");

    // ACT: delete_file purges file chunks and runs defensive vector orphan cleanup
    storage.delete_file(file_path).expect("delete_file");

    // ASSERT: All 4 vectors in vec_chunks must be deleted (3 because chunks were deleted, 1 because rowid 888888 is an orphan)
    let post_vecs: i64 = conn
        .query_row("SELECT COUNT(*) FROM vec_chunks", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        post_vecs, 0,
        "All vectors including orphan rowid must be purged"
    );
}

#[test]
fn test_adv_08_complex_graph_1hop_obsolescence_prioritization() {
    let storage = StorageManager::open_in_memory().expect("open memory");
    storage.init_schema().expect("init schema");

    let old_doc = "Notes/Legacy_Spec.md";
    let candidate_v2 = "Notes/Spec_v2.md";
    let candidate_v3 = "Notes/Spec_v3.md";

    storage
        .upsert_file(old_doc, "h0", 1600000000, None)
        .unwrap();
    storage
        .upsert_file(candidate_v2, "h2", 1650000000, None)
        .unwrap();
    storage
        .upsert_file(candidate_v3, "h3", 1700000000, None)
        .unwrap();

    // Two different documents claim to supersede Legacy_Spec.md:
    // v2 at timestamp 1650000000
    // v3 at timestamp 1700000000 (newer)
    storage
        .insert_file_link(candidate_v2, old_doc, FileLink::TYPE_SUPERSEDES, 1650000000)
        .unwrap();
    storage
        .insert_file_link(candidate_v3, old_doc, FileLink::TYPE_SUPERSEDES, 1700000000)
        .unwrap();

    let best_successor = storage.get_superseding_file(old_doc).unwrap();
    assert_eq!(
        best_successor,
        Some(candidate_v3.to_string()),
        "Must pick the newest superseding document by created_at DESC"
    );
}

#[test]
fn test_adv_09_special_characters_sql_injection_resilience() {
    let storage = StorageManager::open_in_memory().expect("open memory");
    storage.init_schema().expect("init schema");

    // File paths with quotes, SQL keywords, symbols, emoji, and spaces
    let tricky_file = "Notes/It's a tricky file -- DROP TABLE chunks; -- \"[2026]\" 🦀.md";
    let tricky_target = "Notes/O'Connor's & Co. \"Special\" -- DELETE FROM files.md";

    storage
        .upsert_file(tricky_file, "hash_tricky", 1720000000, None)
        .unwrap();
    storage
        .upsert_file(tricky_target, "hash_target", 1720000000, None)
        .unwrap();

    let chunk = IndexedChunk {
        id: None,
        chunk_id: format!("{tricky_file}:0"),
        file_path: tricky_file.to_string(),
        chunk_index: 0,
        content: "Special SQL characters '\"-- /* comment */ content".to_string(),
        token_count: 8,
        coala_type: CoalaType::Semantic,
        status: NoteStatus::Active,
        superseded_by: None,
        deprecated_at: None,
        date_creation: 1720000000,
    };
    let rowid = storage.index_chunk(&chunk).unwrap();
    let v = make_synthetic_vector(42.0);
    storage.insert_chunk_vector(rowid, &v).unwrap();

    // Link insertion with tricky names
    storage
        .insert_file_link(
            tricky_file,
            tricky_target,
            FileLink::TYPE_SUPERSEDES,
            1720000000,
        )
        .unwrap();

    // 1-Hop traversal on tricky name
    let successor = storage.get_superseding_file(tricky_target).unwrap();
    assert_eq!(successor, Some(tricky_file.to_string()));

    // Cascade deletion of tricky file
    storage
        .delete_file(tricky_file)
        .expect("delete tricky file");

    let conn = storage.raw_connection();
    let remaining_chunks: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM chunks WHERE file_path = ?1",
            [tricky_file],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(remaining_chunks, 0);

    let remaining_vecs: i64 = conn
        .query_row("SELECT COUNT(*) FROM vec_chunks", [], |r| r.get(0))
        .unwrap();
    assert_eq!(remaining_vecs, 0);

    // Verify foreign key integrity using SQLite's foreign_key_check pragma
    let fk_violations: Vec<String> = conn
        .prepare("PRAGMA foreign_key_check;")
        .unwrap()
        .query_map([], |row| {
            let table: String = row.get(0)?;
            Ok(table)
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(
        fk_violations.is_empty(),
        "Zero foreign key violations must exist: {:?}",
        fk_violations
    );
}

#[test]
fn test_adv_10_large_scale_cascade_and_foreign_key_check() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("large_scale.db");
    let storage = StorageManager::open(&db_path).expect("open storage");
    storage.init_schema().expect("init schema");

    let conn = storage.raw_connection();
    let file_path = "Notes/MassiveDocument.md";
    storage
        .upsert_file(file_path, "hash_massive", 1720000000, None)
        .unwrap();

    // 200 chunks in single file
    let chunk_count = 200;
    for i in 0..chunk_count {
        let chunk = IndexedChunk {
            id: None,
            chunk_id: format!("massive_{i}"),
            file_path: file_path.to_string(),
            chunk_index: i,
            content: format!("Massive chunk {i} of text data"),
            token_count: 5,
            coala_type: CoalaType::Semantic,
            status: NoteStatus::Active,
            superseded_by: None,
            deprecated_at: None,
            date_creation: 1720000000,
        };
        let rowid = storage.index_chunk(&chunk).unwrap();
        let v = make_synthetic_vector((i % 50) as f32);
        storage.insert_chunk_vector(rowid, &v).unwrap();
    }

    // Verify exactly 200 rows in chunks, vec_chunks, fts_notes
    let c_cnt: i64 = conn
        .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
        .unwrap();
    assert_eq!(c_cnt, chunk_count as i64);
    let v_cnt: i64 = conn
        .query_row("SELECT COUNT(*) FROM vec_chunks", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v_cnt, chunk_count as i64);
    let f_cnt: i64 = conn
        .query_row("SELECT COUNT(*) FROM fts_notes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(f_cnt, chunk_count as i64);

    // ACT: Cascade delete massive file
    storage.delete_file(file_path).unwrap();

    // Verify all 200 rows purged from all tables
    let c_post: i64 = conn
        .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
        .unwrap();
    assert_eq!(c_post, 0);
    let v_post: i64 = conn
        .query_row("SELECT COUNT(*) FROM vec_chunks", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v_post, 0);
    let f_post: i64 = conn
        .query_row("SELECT COUNT(*) FROM fts_notes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(f_post, 0);

    // Foreign key integrity check
    let violations: Vec<String> = conn
        .prepare("PRAGMA foreign_key_check;")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(
        violations.is_empty(),
        "Database must pass PRAGMA foreign_key_check"
    );
}
