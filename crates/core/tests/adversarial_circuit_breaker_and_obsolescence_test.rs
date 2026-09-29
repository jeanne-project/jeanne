//! Adversarial Challenge Test Suite: Anti-Hallucination Circuit Breaker & Obsolescence Filtering
//!
//! Authored by Challenger M2 #2 for Jeanne Milestone 2.
//!
//! Empirically challenges and stress-tests:
//! 1. Circuit breaker threshold boundary precision: 0.64999 vs 0.65000 vs 0.65001.
//! 2. Negative vector similarities (including diametrically opposed vectors S_v = -1.0).
//! 3. Empty vault and empty vector index handling.
//! 4. High BM25 lexical match with low vector similarity (circuit breaker veto against hallucination).
//! 5. Strict omission of deprecated notes even under near-perfect vector similarity (0.999).
//! 6. Deprecated note with `superseded_by` pointer vs 1-hop active replacement resolution.
//! 7. Candidate-k window saturation with multiple deprecated notes.
//! 8. Adversarial lexical input resilience (FTS5 operator injection, SQL injection syntax, unclosed quotes).

use jeanne_core::error::RagError;
use jeanne_core::models::{CoalaType, FileLink, IndexedChunk, NoteStatus};
use jeanne_core::rag::{check_circuit_breaker, RagEngine, SIMILARITY_THRESHOLD};
use jeanne_core::storage::StorageManager;
use std::sync::{Arc, Mutex};

// ============================================================================
// HELPER GENERATORS & ORACLES
// ============================================================================

/// Generates a standard basis vector with 1.0 at index `dim` and 0.0 elsewhere.
fn make_basis_vector(dim: usize) -> [f32; 384] {
    let mut vec = [0.0f32; 384];
    if dim < 384 {
        vec[dim] = 1.0;
    }
    vec
}

/// Generates a normalized unit 384D vector whose cosine similarity with `make_basis_vector(0)`
/// is exactly `target_sim` (clamped to [-1.0, 1.0]).
///
/// v = [target_sim, sqrt(1 - target_sim^2), 0, ..., 0]
/// dot(v, basis_0) = target_sim
/// norm(v) = sqrt(target_sim^2 + 1 - target_sim^2) = 1.0
fn make_unit_vector_at_similarity(target_sim: f32) -> [f32; 384] {
    let mut vec = [0.0f32; 384];
    let clamped = target_sim.clamp(-1.0, 1.0);
    vec[0] = clamped;
    let rem_sq = (1.0f32 - clamped * clamped).max(0.0);
    vec[1] = rem_sq.sqrt();
    vec
}

/// Creates an isolated in-memory StorageManager with schema initialized.
fn create_test_storage() -> StorageManager {
    let storage = StorageManager::open_in_memory().expect("open in-memory storage");
    storage.init_schema().expect("init schema");
    storage
}

// ============================================================================
// 1. CIRCUIT BREAKER THRESHOLD BOUNDARY TESTS (0.64999 vs 0.65000 vs 0.65001)
// ============================================================================

#[test]
fn test_adv_cb_01_unit_threshold_fine_boundaries() {
    assert_eq!(SIMILARITY_THRESHOLD, 0.65f32);

    // Strictly below 0.65 -> Must return Err(RagError::InformationNotFound)
    assert_eq!(
        check_circuit_breaker(0.64999),
        Err(RagError::InformationNotFound { similarity: 0.64999 })
    );
    assert_eq!(
        check_circuit_breaker(0.6499999),
        Err(RagError::InformationNotFound { similarity: 0.6499999 })
    );
    assert_eq!(
        check_circuit_breaker(0.64000),
        Err(RagError::InformationNotFound { similarity: 0.64000 })
    );

    // Exactly at threshold or above -> Must return Ok(())
    assert!(check_circuit_breaker(0.65000).is_ok());
    assert!(check_circuit_breaker(0.6500001).is_ok());
    assert!(check_circuit_breaker(0.65001).is_ok());
    assert!(check_circuit_breaker(0.70000).is_ok());
    assert!(check_circuit_breaker(1.00000).is_ok());

    // Non-finite values
    assert_eq!(
        check_circuit_breaker(f32::NAN),
        Err(RagError::InformationNotFound { similarity: 0.0 })
    );
    assert_eq!(
        check_circuit_breaker(f32::NEG_INFINITY),
        Err(RagError::InformationNotFound { similarity: f32::NEG_INFINITY })
    );
    assert!(check_circuit_breaker(f32::INFINITY).is_ok());
}

#[test]
fn test_adv_cb_02_sqlite_vec_threshold_boundaries_end_to_end() {
    let storage = create_test_storage();
    storage
        .upsert_file("notes/boundary.md", "hash_boundary", 1720000000, None)
        .expect("upsert file");

    let chunk = IndexedChunk::new(
        "chunk_boundary_1",
        "notes/boundary.md",
        0,
        "Circuit breaker boundary verification note content.",
        8,
        CoalaType::Semantic,
        NoteStatus::Active,
        1720000000,
    );
    let rowid = storage.index_chunk(&chunk).expect("index chunk");

    // Stored vector along basis 0
    let v_doc = make_basis_vector(0);
    storage.insert_chunk_vector(rowid, &v_doc).expect("insert vector");

    let engine = RagEngine::new(Arc::new(Mutex::new(storage)));

    // Case A: Query vector with theoretical similarity 0.64999 (< 0.65)
    let q_below = make_unit_vector_at_similarity(0.64999);
    let res_below = engine.search("boundary", &q_below, 5);
    match res_below {
        Err(RagError::InformationNotFound { similarity }) => {
            assert!(
                similarity < 0.65,
                "Expected similarity < 0.65, got {similarity}"
            );
            assert!(
                (similarity - 0.64999).abs() < 1e-4,
                "Similarity should match query target within float tolerance, got {similarity}"
            );
        }
        Ok(results) => panic!("Expected circuit breaker trigger, but search succeeded: {results:?}"),
        Err(e) => panic!("Unexpected error variant: {e:?}"),
    }

    // Case B: Query vector with theoretical similarity 0.65001 (>= 0.65)
    let q_above = make_unit_vector_at_similarity(0.65001);
    let res_above = engine.search("boundary", &q_above, 5);
    match res_above {
        Ok(results) => {
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].chunk_id, "chunk_boundary_1");
            assert!(
                results[0].vector_score >= 0.65,
                "Expected vector score >= 0.65, got {}",
                results[0].vector_score
            );
        }
        Err(e) => panic!("Expected search success for similarity >= 0.65, got error: {e:?}"),
    }
}

// ============================================================================
// 2. NEGATIVE VECTOR SIMILARITIES & DIAMETRIC OPPOSITION
// ============================================================================

#[test]
fn test_adv_cb_03_negative_similarities_unit_and_extremes() {
    assert_eq!(
        check_circuit_breaker(-1.0),
        Err(RagError::InformationNotFound { similarity: -1.0 })
    );
    assert_eq!(
        check_circuit_breaker(-0.5),
        Err(RagError::InformationNotFound { similarity: -0.5 })
    );
    assert_eq!(
        check_circuit_breaker(-0.0001),
        Err(RagError::InformationNotFound { similarity: -0.0001 })
    );
    assert_eq!(
        check_circuit_breaker(-1e20),
        Err(RagError::InformationNotFound { similarity: -1e20 })
    );
}

#[test]
fn test_adv_cb_04_diametrically_opposed_vectors_sqlite_vec_end_to_end() {
    let storage = create_test_storage();
    storage
        .upsert_file("notes/positive.md", "h_pos", 1720000000, None)
        .expect("upsert");

    let chunk = IndexedChunk::new(
        "chunk_pos_1",
        "notes/positive.md",
        0,
        "Positive pole vector note.",
        5,
        CoalaType::Semantic,
        NoteStatus::Active,
        1720000000,
    );
    let rowid = storage.index_chunk(&chunk).expect("index chunk");

    // Stored vector = +basis_0
    let v_pos = make_basis_vector(0);
    storage.insert_chunk_vector(rowid, &v_pos).expect("insert vector");

    let engine = RagEngine::new(Arc::new(Mutex::new(storage)));

    // Query vector = -basis_0 (diametrically opposed: dot = -1.0, cosine dist = 2.0, sim = -1.0)
    let mut v_neg = [0.0f32; 384];
    v_neg[0] = -1.0;

    let res = engine.search("pole", &v_neg, 5);
    match res {
        Err(RagError::InformationNotFound { similarity }) => {
            assert!(
                (similarity - (-1.0)).abs() < 1e-4,
                "Expected similarity == -1.0 for diametric vector, got {similarity}"
            );
        }
        Ok(results) => panic!("Expected circuit breaker error, got results: {results:?}"),
        Err(e) => panic!("Unexpected error variant: {e:?}"),
    }
}

#[test]
fn test_adv_cb_05_all_candidates_negative_similarities_evaluation() {
    let storage = create_test_storage();
    storage
        .upsert_file("notes/negatives.md", "h_neg", 1720000000, None)
        .expect("upsert");

    // Insert 3 chunks with different negative alignments against basis_0
    let sims = [-0.95f32, -0.60f32, -0.20f32];
    for (i, &sim) in sims.iter().enumerate() {
        let chunk = IndexedChunk::new(
            format!("chunk_neg_{i}"),
            "notes/negatives.md",
            i,
            format!("Negative vector chunk number {i}"),
            5,
            CoalaType::Semantic,
            NoteStatus::Active,
            1720000000,
        );
        let rid = storage.index_chunk(&chunk).expect("index");
        let v = make_unit_vector_at_similarity(sim);
        storage.insert_chunk_vector(rid, &v).expect("insert vec");
    }

    let engine = RagEngine::new(Arc::new(Mutex::new(storage)));

    // Query with +basis_0
    let query_vec = make_basis_vector(0);
    let res = engine.search("negative", &query_vec, 10);

    match res {
        Err(RagError::InformationNotFound { similarity }) => {
            // Maximum similarity among [-0.95, -0.60, -0.20] is -0.20
            assert!(
                (similarity - (-0.20)).abs() < 1e-4,
                "Expected max similarity to be approximately -0.20, got {similarity}"
            );
        }
        Ok(_) => panic!("Should have triggered circuit breaker"),
        Err(e) => panic!("Unexpected error: {e:?}"),
    }
}

// ============================================================================
// 3. EMPTY VAULT, PURGED VAULT, AND ZERO CANDIDATE BEHAVIOR
// ============================================================================

#[test]
fn test_adv_cb_06_empty_vault_fresh_database() {
    let storage = create_test_storage();
    let engine = RagEngine::new(Arc::new(Mutex::new(storage)));

    let query_vec = make_basis_vector(0);
    let res = engine.search("any query", &query_vec, 10);

    assert_eq!(
        res,
        Err(RagError::InformationNotFound { similarity: 0.0 })
    );
}

#[test]
fn test_adv_cb_07_chunks_exist_but_vec_chunks_empty() {
    let storage = create_test_storage();
    storage
        .upsert_file("notes/unembedded.md", "h_unemb", 1720000000, None)
        .expect("upsert");

    let chunk = IndexedChunk::new(
        "chunk_unemb_1",
        "notes/unembedded.md",
        0,
        "Content indexed in relational table but never embedded.",
        7,
        CoalaType::Semantic,
        NoteStatus::Active,
        1720000000,
    );
    let _rowid = storage.index_chunk(&chunk).expect("index chunk");
    // Notice: we do NOT call storage.insert_chunk_vector()

    let engine = RagEngine::new(Arc::new(Mutex::new(storage)));
    let query_vec = make_basis_vector(0);
    let res = engine.search("unembedded", &query_vec, 10);

    assert_eq!(
        res,
        Err(RagError::InformationNotFound { similarity: 0.0 }),
        "Empty vec_chunks must trigger circuit breaker with similarity 0.0"
    );
}

#[test]
fn test_adv_cb_08_purged_vault_after_cascade_delete() {
    let storage = Arc::new(Mutex::new(create_test_storage()));
    let file_path = "notes/ephemeral.md";
    {
        let guard = storage.lock().expect("lock storage");
        guard
            .upsert_file(file_path, "h_eph", 1720000000, None)
            .expect("upsert");

        let chunk = IndexedChunk::new(
            "chunk_eph_1",
            file_path,
            0,
            "Ephemeral content to be deleted.",
            5,
            CoalaType::Semantic,
            NoteStatus::Active,
            1720000000,
        );
        let rowid = guard.index_chunk(&chunk).expect("index");
        let vec = make_basis_vector(0);
        guard.insert_chunk_vector(rowid, &vec).expect("insert vector");
    }

    let engine = RagEngine::new(storage.clone());
    let vec = make_basis_vector(0);

    // Search succeeds before deletion
    assert!(engine.search("ephemeral", &vec, 5).is_ok());

    // Cascade delete file
    {
        let guard = storage.lock().expect("lock storage");
        guard.delete_file(file_path).expect("delete file");
    }

    // After cascade delete, vec_chunks is empty -> Circuit breaker must trigger
    let res_after = engine.search("ephemeral", &vec, 5);
    assert_eq!(
        res_after,
        Err(RagError::InformationNotFound { similarity: 0.0 })
    );
}

#[test]
fn test_adv_cb_09_limit_zero_boundary() {
    let storage = create_test_storage();
    storage
        .upsert_file("notes/test.md", "h1", 1720000000, None)
        .expect("upsert");
    let chunk = IndexedChunk::new(
        "c1",
        "notes/test.md",
        0,
        "Zero limit test content.",
        5,
        CoalaType::Semantic,
        NoteStatus::Active,
        1720000000,
    );
    let rowid = storage.index_chunk(&chunk).expect("index");
    let vec = make_basis_vector(0);
    storage.insert_chunk_vector(rowid, &vec).expect("insert vec");

    let engine = RagEngine::new(Arc::new(Mutex::new(storage)));

    // limit = 0 must return Ok(empty) immediately without error
    let res = engine.search("zero", &vec, 0).expect("limit 0 success");
    assert!(res.is_empty());
}

// ============================================================================
// 4. HIGH BM25 WITH LOW VECTOR SIMILARITY (ANTI-HALLUCINATION VETO)
// ============================================================================

#[test]
fn test_adv_cb_10_high_bm25_orthogonal_vector_vetoed() {
    let storage = create_test_storage();
    storage
        .upsert_file("notes/lexical.md", "h_lex", 1720000000, None)
        .expect("upsert");

    let chunk = IndexedChunk::new(
        "chunk_lex_1",
        "notes/lexical.md",
        0,
        "Quantum computing qubits entanglement superposition decoherence cryptography.",
        10,
        CoalaType::Semantic,
        NoteStatus::Active,
        1720000000,
    );
    let rowid = storage.index_chunk(&chunk).expect("index");

    // Stored vector along basis 0
    let v_stored = make_basis_vector(0);
    storage.insert_chunk_vector(rowid, &v_stored).expect("insert vector");

    let engine = RagEngine::new(Arc::new(Mutex::new(storage)));

    // Query text has 100% keyword match against FTS5
    let lexical_query = "quantum computing qubits entanglement";

    // Query vector is orthogonal (basis 1, cosine similarity = 0.0)
    let v_orthogonal = make_basis_vector(1);

    let res = engine.search(lexical_query, &v_orthogonal, 5);

    // Circuit breaker MUST veto despite perfect BM25 lexical match
    match res {
        Err(RagError::InformationNotFound { similarity }) => {
            assert!(
                similarity.abs() < 1e-4,
                "Expected similarity ~0.0, got {similarity}"
            );
        }
        Ok(results) => panic!(
            "CRITICAL: Circuit breaker failed to veto lexical hallucination! Returned: {results:?}"
        ),
        Err(e) => panic!("Unexpected error: {e:?}"),
    }
}

#[test]
fn test_adv_cb_11_high_bm25_low_vector_similarity_050_vetoed() {
    let storage = create_test_storage();
    storage
        .upsert_file("notes/lexical2.md", "h_lex2", 1720000000, None)
        .expect("upsert");

    let chunk = IndexedChunk::new(
        "chunk_lex_2",
        "notes/lexical2.md",
        0,
        "Distributed database replication consensus raft paxos linearizability.",
        9,
        CoalaType::Semantic,
        NoteStatus::Active,
        1720000000,
    );
    let rowid = storage.index_chunk(&chunk).expect("index");
    let v_stored = make_basis_vector(0);
    storage.insert_chunk_vector(rowid, &v_stored).expect("insert vec");

    let engine = RagEngine::new(Arc::new(Mutex::new(storage)));

    // Query text matches perfectly
    let lexical_query = "distributed database consensus raft";

    // Query vector has similarity 0.50 (< 0.65 threshold)
    let v_050 = make_unit_vector_at_similarity(0.50);

    let res = engine.search(lexical_query, &v_050, 5);

    match res {
        Err(RagError::InformationNotFound { similarity }) => {
            assert!(
                (similarity - 0.50).abs() < 1e-4,
                "Expected similarity ~0.50, got {similarity}"
            );
        }
        Ok(results) => panic!(
            "Circuit breaker failed! Vector similarity 0.50 should not pass, returned: {results:?}"
        ),
        Err(e) => panic!("Unexpected error: {e:?}"),
    }
}

#[test]
fn test_adv_cb_12_high_bm25_near_threshold_06499_vetoed() {
    let storage = create_test_storage();
    storage
        .upsert_file("notes/lexical3.md", "h_lex3", 1720000000, None)
        .expect("upsert");

    let chunk = IndexedChunk::new(
        "chunk_lex_3",
        "notes/lexical3.md",
        0,
        "Rust asynchronous programming tokio channels pin futures stream.",
        9,
        CoalaType::Semantic,
        NoteStatus::Active,
        1720000000,
    );
    let rowid = storage.index_chunk(&chunk).expect("index");
    let v_stored = make_basis_vector(0);
    storage.insert_chunk_vector(rowid, &v_stored).expect("insert vec");

    let engine = RagEngine::new(Arc::new(Mutex::new(storage)));

    let lexical_query = "asynchronous programming tokio channels";
    let v_06499 = make_unit_vector_at_similarity(0.6499);

    let res = engine.search(lexical_query, &v_06499, 5);

    match res {
        Err(RagError::InformationNotFound { similarity }) => {
            assert!(
                similarity < 0.65,
                "Expected similarity < 0.65, got {similarity}"
            );
        }
        Ok(results) => panic!("Expected circuit breaker trigger, got results: {results:?}"),
        Err(e) => panic!("Unexpected error: {e:?}"),
    }
}

// ============================================================================
// 5. DEPRECATED NOTE EXCLUSION UNDER HIGH VECTOR SIMILARITY
// ============================================================================

#[test]
fn test_adv_obs_13_single_deprecated_note_near_perfect_similarity_omitted() {
    let storage = create_test_storage();
    let file_path = "notes/deprecated_only.md";
    storage
        .upsert_file(file_path, "h_dep_only", 1720000000, None)
        .expect("upsert");

    let chunk = IndexedChunk::new(
        "chunk_dep_only_1",
        file_path,
        0,
        "Legacy system documentation that has been deprecated.",
        8,
        CoalaType::Semantic,
        NoteStatus::Deprecated,
        1720000000,
    );
    let rowid = storage.index_chunk(&chunk).expect("index chunk");

    // Vector similarity near-perfect: 0.999
    let v_near_perfect = make_unit_vector_at_similarity(0.999);
    storage.insert_chunk_vector(rowid, &v_near_perfect).expect("insert vector");

    let engine = RagEngine::new(Arc::new(Mutex::new(storage)));

    // Standard search: query vector is basis 0 (similarity 0.999 >= 0.65)
    let query_vec = make_basis_vector(0);
    let res = engine.search("system documentation", &query_vec, 10);

    // Circuit breaker passes (0.999 >= 0.65), but SQL status filter drops deprecated note!
    // Resulting list must be completely empty!
    let results = res.expect("search should succeed without circuit breaker error");
    assert!(
        results.is_empty(),
        "Deprecated note MUST NOT be returned in standard search, got: {results:?}"
    );

    // Historical audit search: must include the deprecated note
    let audit_results = engine
        .search_with_options("system documentation", &query_vec, 10, 1720000000, true)
        .expect("audit search");
    assert_eq!(audit_results.len(), 1);
    assert_eq!(audit_results[0].chunk_id, "chunk_dep_only_1");
    assert_eq!(audit_results[0].status, NoteStatus::Deprecated);
}

#[test]
fn test_adv_obs_14_deprecated_near_perfect_vs_active_moderate_strict_omission() {
    let storage = create_test_storage();

    // Deprecated note with near-perfect similarity (0.99) and identical lexical match
    storage
        .upsert_file("notes/v1_deprecated.md", "h_v1", 1710000000, None)
        .expect("upsert");
    let c_dep = IndexedChunk::new(
        "c_dep",
        "notes/v1_deprecated.md",
        0,
        "Authentication protocol OAuth1 implementation with HMAC signatures.",
        8,
        CoalaType::Semantic,
        NoteStatus::Deprecated,
        1710000000,
    );
    let r_dep = storage.index_chunk(&c_dep).expect("index");
    let v_dep = make_unit_vector_at_similarity(0.99);
    storage.insert_chunk_vector(r_dep, &v_dep).expect("insert vec");

    // Active note with moderate similarity (0.75)
    storage
        .upsert_file("notes/v2_active.md", "h_v2", 1720000000, None)
        .expect("upsert");
    let c_act = IndexedChunk::new(
        "c_act",
        "notes/v2_active.md",
        0,
        "Modern authentication protocol OAuth2 implementation with PKCE and JWT.",
        9,
        CoalaType::Semantic,
        NoteStatus::Active,
        1720000000,
    );
    let r_act = storage.index_chunk(&c_act).expect("index");
    let v_act = make_unit_vector_at_similarity(0.75);
    storage.insert_chunk_vector(r_act, &v_act).expect("insert vec");

    let engine = RagEngine::new(Arc::new(Mutex::new(storage)));

    // Standard search with basis 0
    let query_vec = make_basis_vector(0);
    let results = engine
        .search("authentication protocol OAuth", &query_vec, 10)
        .expect("standard search");

    // ONLY the active note must be returned! Deprecated note must be 100% excluded!
    assert_eq!(
        results.len(),
        1,
        "Standard search must return exactly 1 active note"
    );
    assert_eq!(results[0].chunk_id, "c_act");
    assert_eq!(results[0].file_path, "notes/v2_active.md");
    assert_eq!(results[0].status, NoteStatus::Active);
    assert!(!results.iter().any(|r| r.chunk_id == "c_dep"));
}

#[test]
fn test_adv_obs_15_deprecated_note_with_superseded_by_metadata() {
    let storage = create_test_storage();
    storage
        .upsert_file("notes/arch_v1.md", "h_v1", 1710000000, None)
        .expect("upsert");

    let mut chunk = IndexedChunk::new(
        "c_arch_v1",
        "notes/arch_v1.md",
        0,
        "Monolithic architecture deployment on bare metal servers.",
        8,
        CoalaType::Semantic,
        NoteStatus::Deprecated,
        1710000000,
    );
    chunk.superseded_by = Some("notes/arch_v2.md".to_string());
    chunk.deprecated_at = Some(1720000000);

    let rid = storage.index_chunk(&chunk).expect("index");
    let vec = make_basis_vector(0);
    storage.insert_chunk_vector(rid, &vec).expect("insert vec");

    let engine = RagEngine::new(Arc::new(Mutex::new(storage)));
    let results = engine.search("architecture deployment", &vec, 10).expect("search");

    // Standard search: deprecated note must be omitted even if superseded_by is populated
    assert!(
        results.is_empty(),
        "Deprecated note must be completely omitted from standard results"
    );
}

#[test]
fn test_adv_obs_16_active_note_with_1hop_supersedes_link_resolution() {
    let storage = create_test_storage();
    storage
        .upsert_file("notes/spec_old.md", "h1", 1710000000, None)
        .expect("upsert");
    storage
        .upsert_file("notes/spec_new.md", "h2", 1720000000, None)
        .expect("upsert");

    // Note is Active, but replaced by spec_new.md
    let chunk = IndexedChunk::new(
        "c_spec_old",
        "notes/spec_old.md",
        0,
        "Specification version 1.0 details and interface contracts.",
        8,
        CoalaType::Semantic,
        NoteStatus::Active,
        1710000000,
    );
    let rid = storage.index_chunk(&chunk).expect("index");
    let vec = make_basis_vector(0);
    storage.insert_chunk_vector(rid, &vec).expect("insert vec");

    // Link: notes/spec_new.md supersedes notes/spec_old.md
    storage
        .insert_file_link(
            "notes/spec_new.md",
            "notes/spec_old.md",
            FileLink::TYPE_SUPERSEDES,
            1720000000,
        )
        .expect("insert link");

    let engine = RagEngine::new(Arc::new(Mutex::new(storage)));
    let results = engine
        .search("specification contracts", &vec, 5)
        .expect("search");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].chunk_id, "c_spec_old");
    assert_eq!(
        results[0].superseded_by,
        Some("notes/spec_new.md".to_string()),
        "1-hop graph link must resolve superseding document"
    );
}

// ============================================================================
// 6. ADVERSARIAL LEXICAL SANITIZATION & FTS5 RESILIENCE IN RAG
// ============================================================================

#[test]
fn test_adv_fts_17_adversarial_queries_in_rag_search() {
    let storage = create_test_storage();
    storage
        .upsert_file("notes/resilient.md", "h_res", 1720000000, None)
        .expect("upsert");

    let chunk = IndexedChunk::new(
        "chunk_res_1",
        "notes/resilient.md",
        0,
        "System resilience under adversarial query input conditions.",
        8,
        CoalaType::Semantic,
        NoteStatus::Active,
        1720000000,
    );
    let rowid = storage.index_chunk(&chunk).expect("index");
    let vec = make_basis_vector(0);
    storage.insert_chunk_vector(rowid, &vec).expect("insert vec");

    let engine = RagEngine::new(Arc::new(Mutex::new(storage)));

    let adversarial_inputs = [
        r#""unbalanced single quote"#,
        r#"multiple """ quotes """" in " input"#,
        "AND OR NOT NEAR/5",
        "SELECT * FROM chunks WHERE '1'='1';",
        "'; DROP TABLE chunks; --",
        "🦀 🚀 ⚠️ 💡 Unicode & Emojis",
        "   \t\n   ", // Only whitespace
        "***???---+++",
        "column:value prefix:query",
        "NEAR(foo bar, 10)",
    ];

    for &input in &adversarial_inputs {
        let res = engine.search(input, &vec, 5);
        // None of these inputs may cause a SQL syntax error, panic, or unexpected crash
        assert!(
            res.is_ok(),
            "Adversarial input {input:?} caused search failure: {res:?}"
        );
    }
}


#[test]
fn test_adv_obs_18_candidate_k_window_saturation_by_deprecated_notes() {
    let storage = create_test_storage();

    // 1. Insert 22 deprecated notes with higher similarity (0.95)
    for i in 0..22 {
        let path = format!("notes/deprecated_{i}.md");
        storage
            .upsert_file(&path, &format!("h_dep_{i}"), 1710000000, None)
            .expect("upsert");
        let chunk = IndexedChunk::new(
            format!("c_dep_{i}"),
            &path,
            0,
            format!("Deprecated content version {i}"),
            5,
            CoalaType::Semantic,
            NoteStatus::Deprecated,
            1710000000,
        );
        let rid = storage.index_chunk(&chunk).expect("index");
        let vec = make_unit_vector_at_similarity(0.95);
        storage.insert_chunk_vector(rid, &vec).expect("insert vec");
    }

    // 2. Insert 2 active notes with lower similarity (0.80)
    for i in 0..2 {
        let path = format!("notes/active_{i}.md");
        storage
            .upsert_file(&path, &format!("h_act_{i}"), 1720000000, None)
            .expect("upsert");
        let chunk = IndexedChunk::new(
            format!("c_act_{i}"),
            &path,
            0,
            format!("Active valid content version {i}"),
            5,
            CoalaType::Semantic,
            NoteStatus::Active,
            1720000000,
        );
        let rid = storage.index_chunk(&chunk).expect("index");
        let vec = make_unit_vector_at_similarity(0.80);
        storage.insert_chunk_vector(rid, &vec).expect("insert vec");
    }

    let engine = RagEngine::new(Arc::new(Mutex::new(storage)));
    let query_vec = make_basis_vector(0);

    // Case A: With limit=1, candidate_k is (1*3).max(20) = 20.
    // The top 20 vectors in sqlite-vec are ALL deprecated (22 exist at 0.95).
    // The SQL post-filter (WHERE status = 'active') drops all 20, yielding empty results.
    let res_small_k = engine.search("content", &query_vec, 1).expect("search");
    assert!(
        res_small_k.is_empty(),
        "Small candidate_k window saturated by 20+ deprecated notes yields empty results"
    );

    // Case B: With limit=10, candidate_k is (10*3).max(20) = 30.
    // The top 30 vectors encompass both the 22 deprecated and the 2 active notes.
    // The 2 active notes survive the SQL filter and are successfully returned!
    let res_large_k = engine.search("content", &query_vec, 10).expect("search");
    assert_eq!(
        res_large_k.len(),
        2,
        "Larger candidate_k window allows active notes to surface after deprecated notes are filtered"
    );
    assert!(res_large_k.iter().all(|r| r.status == NoteStatus::Active));
}

