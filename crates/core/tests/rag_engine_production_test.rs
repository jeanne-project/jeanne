//! Production integration test suite for RagEngine, check_circuit_breaker, and RagError.

use jeanne_core::error::{JeanneError, RagError};
use jeanne_core::models::{CoalaType, FileLink, IndexedChunk, NoteStatus};
use jeanne_core::rag::{RagEngine, check_circuit_breaker};
use jeanne_core::storage::StorageManager;
use std::sync::{Arc, Mutex};

fn make_basis_vector(dim: usize) -> [f32; 384] {
    let mut vec = [0.0f32; 384];
    if dim < 384 {
        vec[dim] = 1.0;
    }
    vec
}

fn make_synthetic_vector(s: f32) -> [f32; 384] {
    let mut vec = [0.0f32; 384];
    let clamped = s.clamp(-1.0, 1.0);
    vec[0] = clamped;
    vec[1] = (1.0 - clamped * clamped).max(0.0).sqrt();
    vec
}

fn create_test_storage() -> StorageManager {
    let storage = StorageManager::open_in_memory().expect("open storage");
    storage.init_schema().expect("init schema");
    storage
}

#[test]
fn test_rag_engine_circuit_breaker_unit() {
    assert_eq!(
        check_circuit_breaker(0.6499),
        Err(RagError::InformationNotFound { similarity: 0.6499 })
    );
    assert_eq!(
        check_circuit_breaker(0.0),
        Err(RagError::InformationNotFound { similarity: 0.0 })
    );
    assert_eq!(
        check_circuit_breaker(-1.0),
        Err(RagError::InformationNotFound { similarity: -1.0 })
    );
    assert_eq!(
        check_circuit_breaker(f32::NAN),
        Err(RagError::InformationNotFound { similarity: 0.0 })
    );
    assert!(check_circuit_breaker(0.6500).is_ok());
    assert!(check_circuit_breaker(0.6501).is_ok());
    assert!(check_circuit_breaker(1.0).is_ok());
}

#[test]
fn test_rag_error_conversions_and_partial_eq() {
    let err1 = RagError::InformationNotFound { similarity: 0.62 };
    let err2 = RagError::InformationNotFound {
        similarity: 0.6200001,
    };
    assert_eq!(err1, err2);

    let j_err = JeanneError::Vault("something failed".to_string());
    let r_err: RagError = j_err.into();
    match &r_err {
        RagError::Storage(boxed) => {
            assert!(boxed.to_string().contains("something failed"));
        }
        _ => panic!("Expected RagError::Storage"),
    }

    let back_to_jeanne: JeanneError = r_err.into();
    match back_to_jeanne {
        JeanneError::Rag(inner) => match inner {
            RagError::Storage(boxed) => {
                assert!(boxed.to_string().contains("something failed"));
            }
            _ => panic!("Expected inner RagError::Storage"),
        },
        _ => panic!("Expected JeanneError::Rag"),
    }
}

#[test]
fn test_rag_engine_empty_db_triggers_circuit_breaker() {
    let storage = create_test_storage();
    let engine = RagEngine::new(Arc::new(Mutex::new(storage)));

    let query_vec = make_basis_vector(0);
    let result = engine.search("anything", &query_vec, 10);
    assert_eq!(
        result,
        Err(RagError::InformationNotFound { similarity: 0.0 })
    );
}

#[test]
fn test_rag_engine_identical_vector_search() {
    let storage = create_test_storage();

    storage
        .upsert_file("notes/rust.md", "hash1", 1720000000, None)
        .expect("upsert file");

    let chunk = IndexedChunk::new(
        "chunk_1",
        "notes/rust.md",
        0,
        "Rust ownership model prevents data races and memory leaks.",
        10,
        CoalaType::Semantic,
        NoteStatus::Active,
        1720000000,
    );
    let rowid = storage.index_chunk(&chunk).expect("index chunk");

    let v0 = make_basis_vector(0);
    storage
        .insert_chunk_vector(rowid, &v0)
        .expect("insert vector");

    let engine = RagEngine::new(Arc::new(Mutex::new(storage)));

    let results = engine
        .search("ownership", &v0, 5)
        .expect("search should succeed");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].chunk_id, "chunk_1");
    assert!((results[0].vector_score - 1.0).abs() < 1e-5);
    assert!(results[0].bm25_score > 0.0);
}

#[test]
fn test_rag_engine_circuit_breaker_vetoes_irrelevant_query() {
    let storage = create_test_storage();

    storage
        .upsert_file("notes/rust.md", "hash1", 1720000000, None)
        .expect("upsert file");

    let chunk = IndexedChunk::new(
        "chunk_1",
        "notes/rust.md",
        0,
        "Rust ownership and borrowing rules.",
        6,
        CoalaType::Semantic,
        NoteStatus::Active,
        1720000000,
    );
    let rowid = storage.index_chunk(&chunk).expect("index chunk");

    // Stored vector along basis 0
    let v0 = make_basis_vector(0);
    storage
        .insert_chunk_vector(rowid, &v0)
        .expect("insert vector");

    let engine = RagEngine::new(Arc::new(Mutex::new(storage)));

    // Query vector along basis 1 (orthogonal, cosine similarity = 0.0 < 0.65)
    let v1 = make_basis_vector(1);
    let result = engine.search("ownership", &v1, 5);

    assert_eq!(
        result,
        Err(RagError::InformationNotFound { similarity: 0.0 })
    );
}

#[test]
fn test_rag_engine_obsolescence_filtering_omits_deprecated() {
    let storage = create_test_storage();

    storage
        .upsert_file("notes/adr_old.md", "h1", 1720000000, None)
        .expect("upsert");
    storage
        .upsert_file("notes/adr_new.md", "h2", 1720000000, None)
        .expect("upsert");

    let chunk_old = IndexedChunk::new(
        "c_old",
        "notes/adr_old.md",
        0,
        "Legacy architecture using PostgreSQL.",
        5,
        CoalaType::Semantic,
        NoteStatus::Deprecated,
        1720000000,
    );
    let r_old = storage.index_chunk(&chunk_old).expect("index");
    let v_old = make_basis_vector(0);
    storage
        .insert_chunk_vector(r_old, &v_old)
        .expect("insert vec");

    let chunk_new = IndexedChunk::new(
        "c_new",
        "notes/adr_new.md",
        0,
        "Modern architecture using SQLite and sqlite-vec.",
        6,
        CoalaType::Semantic,
        NoteStatus::Active,
        1720000000,
    );
    let r_new = storage.index_chunk(&chunk_new).expect("index");
    let v_new = make_synthetic_vector(0.85);
    storage
        .insert_chunk_vector(r_new, &v_new)
        .expect("insert vec");

    let engine = RagEngine::new(Arc::new(Mutex::new(storage)));

    let query_vec = make_basis_vector(0);
    let results = engine
        .search("architecture", &query_vec, 10)
        .expect("search success");

    // c_old has similarity 1.0 but is deprecated => must be omitted!
    // c_new has similarity 0.85 and is active => must be returned!
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].chunk_id, "c_new");
}

#[test]
fn test_rag_engine_one_hop_supersedes_resolution() {
    let storage = create_test_storage();

    storage
        .upsert_file("notes/spec_v1.md", "h1", 1710000000, None)
        .expect("upsert");
    storage
        .upsert_file("notes/spec_v2.md", "h2", 1720000000, None)
        .expect("upsert");

    let chunk = IndexedChunk::new(
        "chunk_v1",
        "notes/spec_v1.md",
        0,
        "Spec v1 protocol description.",
        5,
        CoalaType::Semantic,
        NoteStatus::Active,
        1710000000,
    );
    let rid = storage.index_chunk(&chunk).expect("index");
    let vec = make_basis_vector(0);
    storage.insert_chunk_vector(rid, &vec).expect("insert vec");

    // Link: notes/spec_v2.md supersedes notes/spec_v1.md
    storage
        .insert_file_link(
            "notes/spec_v2.md",
            "notes/spec_v1.md",
            FileLink::TYPE_SUPERSEDES,
            1720000000,
        )
        .expect("insert link");

    let engine = RagEngine::new(Arc::new(Mutex::new(storage)));

    let results = engine
        .search("protocol", &vec, 5)
        .expect("search should succeed");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].chunk_id, "chunk_v1");
    assert_eq!(
        results[0].superseded_by,
        Some("notes/spec_v2.md".to_string())
    );
}

#[test]
fn test_rag_engine_time_decay_procedural_immunity() {
    let now = 1720000000i64;
    let old_creation = now - (365 * 86400); // 1 year ago

    let storage = create_test_storage();

    storage
        .upsert_file("notes/proc.md", "h1", old_creation, None)
        .expect("upsert");

    let chunk = IndexedChunk::new(
        "chunk_proc",
        "notes/proc.md",
        0,
        "Standard deployment checklist protocol.",
        5,
        CoalaType::Procedural,
        NoteStatus::Active,
        old_creation,
    );
    let rid = storage.index_chunk(&chunk).expect("index");
    let vec = make_basis_vector(0);
    storage.insert_chunk_vector(rid, &vec).expect("insert vec");

    let engine = RagEngine::new(Arc::new(Mutex::new(storage)));

    let results = engine
        .search_at("deployment", &vec, 5, now)
        .expect("search success");

    assert_eq!(results.len(), 1);
    let r = &results[0];
    assert!((r.age_days - 365.0).abs() < 1e-4);
    // Procedural => lambda = 0.0 => combined_score == raw_blended
    let expected_raw = 0.7 * r.vector_score + 0.3 * r.bm25_score;
    assert!((r.combined_score - expected_raw).abs() < 1e-5);
}

#[test]
fn test_rag_engine_time_decay_semantic_half_life() {
    let now = 1720000000i64;
    let creation_200_days_ago = now - (200 * 86400);

    let storage = create_test_storage();

    storage
        .upsert_file("notes/sem.md", "h1", creation_200_days_ago, None)
        .expect("upsert");

    let chunk = IndexedChunk::new(
        "chunk_sem",
        "notes/sem.md",
        0,
        "Semantic knowledge about database engines.",
        6,
        CoalaType::Semantic,
        NoteStatus::Active,
        creation_200_days_ago,
    );
    let rid = storage.index_chunk(&chunk).expect("index");
    let vec = make_basis_vector(0);
    storage.insert_chunk_vector(rid, &vec).expect("insert vec");

    let engine = RagEngine::new(Arc::new(Mutex::new(storage)));

    let results = engine
        .search_at("database", &vec, 5, now)
        .expect("search success");

    assert_eq!(results.len(), 1);
    let r = &results[0];
    assert!((r.age_days - 200.0).abs() < 1e-4);
    // Half-life: 1 + 0.005 * 200 = 2.0 => score is halved!
    let raw = 0.7 * r.vector_score + 0.3 * r.bm25_score;
    assert!((r.combined_score - raw / 2.0).abs() < 1e-5);
}

#[test]
fn test_rag_engine_hybrid_blending_vector_and_bm25() {
    let storage = create_test_storage();

    storage
        .upsert_file("notes/doc1.md", "h1", 1720000000, None)
        .expect("upsert");
    storage
        .upsert_file("notes/doc2.md", "h2", 1720000000, None)
        .expect("upsert");

    // doc1: matches keywords "algorithm"
    let c1 = IndexedChunk::new(
        "c1",
        "notes/doc1.md",
        0,
        "Fast sorting algorithm and sorting strategies.",
        6,
        CoalaType::Semantic,
        NoteStatus::Active,
        1720000000,
    );
    let r1 = storage.index_chunk(&c1).expect("index");
    let v1 = make_synthetic_vector(0.70);
    storage.insert_chunk_vector(r1, &v1).expect("insert vec");

    // doc2: does not match keyword "algorithm"
    let c2 = IndexedChunk::new(
        "c2",
        "notes/doc2.md",
        0,
        "Networking and packet routing mechanisms.",
        5,
        CoalaType::Semantic,
        NoteStatus::Active,
        1720000000,
    );
    let r2 = storage.index_chunk(&c2).expect("index");
    let v2 = make_synthetic_vector(0.70);
    storage.insert_chunk_vector(r2, &v2).expect("insert vec");

    let engine = RagEngine::new(Arc::new(Mutex::new(storage)));

    let query_vec = make_basis_vector(0);
    let results = engine
        .search("algorithm", &query_vec, 5)
        .expect("search success");

    assert_eq!(results.len(), 2);
    // c1 has BM25 match => higher combined score than c2
    assert_eq!(results[0].chunk_id, "c1");
    assert_eq!(results[1].chunk_id, "c2");
    assert!(results[0].bm25_score > results[1].bm25_score);
    assert_eq!(results[1].bm25_score, 0.0);
}

#[test]
fn test_rag_engine_historical_audit_mode() {
    let storage = create_test_storage();

    storage
        .upsert_file("notes/dep.md", "h1", 1720000000, None)
        .expect("upsert");

    let chunk = IndexedChunk::new(
        "c_dep",
        "notes/dep.md",
        0,
        "Deprecated protocol specification.",
        4,
        CoalaType::Semantic,
        NoteStatus::Deprecated,
        1720000000,
    );
    let rid = storage.index_chunk(&chunk).expect("index");
    let vec = make_basis_vector(0);
    storage.insert_chunk_vector(rid, &vec).expect("insert vec");

    let engine = RagEngine::new(Arc::new(Mutex::new(storage)));

    // Standard search: deprecated note omitted
    let std_results = engine.search("protocol", &vec, 5).expect("search success");
    assert_eq!(std_results.len(), 0);

    // Audit search: include_deprecated = true => deprecated note returned
    let audit_results = engine
        .search_with_options("protocol", &vec, 5, 1720000000, true)
        .expect("audit search success");
    assert_eq!(audit_results.len(), 1);
    assert_eq!(audit_results[0].chunk_id, "c_dep");
    assert_eq!(audit_results[0].status, NoteStatus::Deprecated);
}

#[test]
fn test_rag_engine_limit_zero() {
    let storage = create_test_storage();
    let engine = RagEngine::new(Arc::new(Mutex::new(storage)));

    let vec = make_basis_vector(0);
    let res = engine.search("test", &vec, 0).expect("limit 0 is ok");
    assert!(res.is_empty());
}
