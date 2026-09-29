//! Adversarial Challenge Test Suite: Hybrid Scoring Formula & Time-Decay Attenuation
//!
//! Written by Challenger M2 #1 to stress-test:
//! 1. Extreme timestamps: Delta t = 0, Delta t = 10 years, Delta t = 50 years, negative Delta t (future clamping).
//! 2. Mathematical boundaries: pure vector match, pure lexical match, all zeros, threshold 0.65 boundary.
//! 3. Anti-hallucination circuit-breaker veto on pure lexical matches (S_vec < 0.65).
//! 4. Ranking inversions: old procedural vs recent semantic notes.
//! 5. Strict temporal monotonicity and half-life exactness.
//! 6. Resilience against NaN, infinities, and extreme time horizons.

use jeanne_core::error::RagError;
use jeanne_core::models::{CoalaType, IndexedChunk, NoteStatus};
use jeanne_core::rag::{RagEngine, check_circuit_breaker, compute_hybrid_decay_score};
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

// ============================================================================
// 1. EXTREME TIMESTAMPS & PROCEDURAL IMMUNITY
// ============================================================================

#[test]
fn test_adv_score_01_extreme_timestamps_delta_t_zero() {
    let v_score = 0.85;
    let b_score = 0.60;
    let expected_raw = 0.7 * v_score + 0.3 * b_score;

    for coala_type in [
        CoalaType::Procedural,
        CoalaType::Semantic,
        CoalaType::Episodic,
    ] {
        let score = compute_hybrid_decay_score(v_score, b_score, coala_type, 0.0);
        assert!(
            (score - expected_raw).abs() < 1e-9,
            "At Delta t = 0, score for {:?} must be identical to raw score",
            coala_type
        );
    }
}

#[test]
fn test_adv_score_02_extreme_timestamps_10_years_decay() {
    let age_10_years_days = 3650.0;
    let v_score = 0.90;
    let b_score = 0.50;
    let raw = 0.7 * v_score + 0.3 * b_score; // 0.63 + 0.15 = 0.78

    // Procedural: lambda = 0.0 => denominator = 1.0 => 100% score retention
    let proc_score =
        compute_hybrid_decay_score(v_score, b_score, CoalaType::Procedural, age_10_years_days);
    assert!(
        (proc_score - raw).abs() < 1e-9,
        "Procedural note must retain 100% of score after 10 years"
    );

    // Semantic & Episodic: lambda = 0.005 => denominator = 1 + 0.005 * 3650 = 1 + 18.25 = 19.25
    let expected_attenuation = 1.0 / 19.25;
    let sem_score =
        compute_hybrid_decay_score(v_score, b_score, CoalaType::Semantic, age_10_years_days);
    let epi_score =
        compute_hybrid_decay_score(v_score, b_score, CoalaType::Episodic, age_10_years_days);

    assert!(
        (sem_score - (raw * expected_attenuation)).abs() < 1e-9,
        "Semantic score after 10 years must be attenuated by factor 1/19.25"
    );
    assert!(
        (epi_score - (raw * expected_attenuation)).abs() < 1e-9,
        "Episodic score after 10 years must match semantic attenuation"
    );
    assert!(
        proc_score > sem_score * 19.0,
        "Procedural score must be over 19 times higher than semantic note of same age"
    );
}

#[test]
fn test_adv_score_03_extreme_timestamps_50_years_procedural_immunity() {
    let age_50_years_days = 18250.0;
    let v_score = 0.80;
    let b_score = 0.40;
    let raw = 0.7 * v_score + 0.3 * b_score;

    let proc_score =
        compute_hybrid_decay_score(v_score, b_score, CoalaType::Procedural, age_50_years_days);
    assert!(
        (proc_score - raw).abs() < 1e-9,
        "Procedural note must retain 100% score even after 50 years"
    );

    // Semantic denominator: 1 + 0.005 * 18250 = 1 + 91.25 = 92.25
    let sem_score =
        compute_hybrid_decay_score(v_score, b_score, CoalaType::Semantic, age_50_years_days);
    assert!((sem_score - (raw / 92.25)).abs() < 1e-9);
    assert!(
        (proc_score / sem_score - 92.25).abs() < 1e-5,
        "Ratio of procedural to semantic score after 50 years must equal 92.25"
    );
}

#[test]
fn test_adv_score_04_negative_delta_t_future_clamping() {
    let v_score = 0.80;
    let b_score = 0.50;
    let raw = 0.7 * v_score + 0.3 * b_score;

    for negative_age in [-1.0, -100.0, -10000.0] {
        for coala_type in [
            CoalaType::Procedural,
            CoalaType::Semantic,
            CoalaType::Episodic,
        ] {
            let score = compute_hybrid_decay_score(v_score, b_score, coala_type, negative_age);
            assert!(
                (score - raw).abs() < 1e-9,
                "Negative age ({}) must be defensively clamped to 0.0, preventing denominator < 1.0 or division by zero",
                negative_age
            );
        }
    }
}

#[test]
fn test_adv_score_05_half_life_exactness() {
    let v_score = 1.0;
    let b_score = 1.0;
    let _raw = 1.0;

    // Half-life: at delta_t = 200 days, denominator = 1 + 0.005 * 200 = 2.0 => score = 0.5
    let score_200 = compute_hybrid_decay_score(v_score, b_score, CoalaType::Semantic, 200.0);
    assert!(
        (score_200 - 0.50).abs() < 1e-9,
        "Semantic score at exactly 200 days must be halved (0.50)"
    );

    // 400 days: denominator = 3.0 => score = 1/3
    let score_400 = compute_hybrid_decay_score(v_score, b_score, CoalaType::Semantic, 400.0);
    assert!(
        (score_400 - (1.0 / 3.0)).abs() < 1e-9,
        "Semantic score at 400 days must be 1/3"
    );

    // 600 days: denominator = 4.0 => score = 1/4
    let score_600 = compute_hybrid_decay_score(v_score, b_score, CoalaType::Semantic, 600.0);
    assert!(
        (score_600 - 0.25).abs() < 1e-9,
        "Semantic score at 600 days must be 1/4"
    );

    // 1000 days: denominator = 6.0 => score = 1/6
    let score_1000 = compute_hybrid_decay_score(v_score, b_score, CoalaType::Semantic, 1000.0);
    assert!(
        (score_1000 - (1.0 / 6.0)).abs() < 1e-9,
        "Semantic score at 1000 days must be 1/6"
    );
}

// ============================================================================
// 2. SCORE BOUNDARIES & CIRCUIT BREAKER COUPLING
// ============================================================================

#[test]
fn test_adv_score_06_score_boundaries_extremes() {
    // Pure vector match (S_v = 1.0, S_b = 0.0)
    let s_pure_vec = compute_hybrid_decay_score(1.0, 0.0, CoalaType::Procedural, 0.0);
    assert!((s_pure_vec - 0.70).abs() < 1e-9);

    // Pure lexical match (S_v = 0.0, S_b = 1.0)
    let s_pure_lex = compute_hybrid_decay_score(0.0, 1.0, CoalaType::Procedural, 0.0);
    assert!((s_pure_lex - 0.30).abs() < 1e-9);

    // Both maximum (S_v = 1.0, S_b = 1.0)
    let s_both_max = compute_hybrid_decay_score(1.0, 1.0, CoalaType::Procedural, 0.0);
    assert!((s_both_max - 1.00).abs() < 1e-9);

    // Both zero (S_v = 0.0, S_b = 0.0)
    let s_both_zero = compute_hybrid_decay_score(0.0, 0.0, CoalaType::Procedural, 0.0);
    assert!((s_both_zero - 0.00).abs() < 1e-9);

    // Threshold boundary (S_v = 0.65, S_b = 0.0)
    let s_thresh = compute_hybrid_decay_score(0.65, 0.0, CoalaType::Procedural, 0.0);
    assert!((s_thresh - 0.455).abs() < 1e-9);

    // Sub-threshold boundary (S_v = 0.6499, S_b = 1.0)
    let s_sub = compute_hybrid_decay_score(0.6499, 1.0, CoalaType::Procedural, 0.0);
    assert!((s_sub - (0.7 * 0.6499 + 0.3)).abs() < 1e-9);
}

#[test]
fn test_adv_score_07_circuit_breaker_veto_on_pure_lexical_in_rag_engine() {
    let storage = create_test_storage();

    storage
        .upsert_file("notes/lexical_only.md", "hash1", 1720000000, None)
        .expect("upsert");

    let chunk = IndexedChunk::new(
        "c_lex",
        "notes/lexical_only.md",
        0,
        "SpecificKeyword appeared multiple times in this document.",
        7,
        CoalaType::Semantic,
        NoteStatus::Active,
        1720000000,
    );
    let rowid = storage.index_chunk(&chunk).expect("index chunk");

    // Vector is stored along basis 0
    let v0 = make_basis_vector(0);
    storage
        .insert_chunk_vector(rowid, &v0)
        .expect("insert vector");

    let engine = RagEngine::new(Arc::new(Mutex::new(storage)));

    // 1. Query vector along basis 1 (orthogonal => similarity = 0.0)
    // Even though keyword matches 100% ("SpecificKeyword"), circuit breaker must veto!
    let v_orthogonal = make_basis_vector(1);
    let result = engine.search("SpecificKeyword", &v_orthogonal, 5);

    assert_eq!(
        result,
        Err(RagError::InformationNotFound { similarity: 0.0 }),
        "Pure lexical match with orthogonal vector must be vetoed by circuit breaker"
    );

    // 2. Query with sub-threshold similarity vector (similarity = 0.649 < 0.65)
    let v_sub = make_synthetic_vector(0.649);
    let result_sub = engine.search("SpecificKeyword", &v_sub, 5);

    match result_sub {
        Err(RagError::InformationNotFound { similarity }) => {
            assert!(
                similarity < 0.65,
                "Expected similarity < 0.65, got {similarity}"
            );
        }
        Ok(_) => panic!("Circuit breaker must veto query with similarity 0.649 < 0.65"),
        Err(other) => panic!("Expected InformationNotFound, got {:?}", other),
    }

    // 3. Query with supra-threshold similarity vector (similarity = 0.651 >= 0.65)
    let v_sup = make_synthetic_vector(0.651);
    let result_sup = engine.search("SpecificKeyword", &v_sup, 5);
    assert!(
        result_sup.is_ok(),
        "Query with similarity >= 0.65 must pass circuit breaker"
    );
    let hits = result_sup.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].chunk_id, "c_lex");
    assert!(hits[0].bm25_score > 0.0);
}

// ============================================================================
// 3. RANKING INVERSION TESTS
// ============================================================================

#[test]
fn test_adv_score_08_ranking_inversion_old_procedural_vs_recent_semantic() {
    let now = 1720000000i64;
    let storage = create_test_storage();

    // 1. Note A: 10-year-old Procedural Note (3,650 days old)
    // Moderate vector similarity 0.75, no lexical keyword
    let age_10y = 3650 * 86400;
    storage
        .upsert_file("notes/sop_deploy.md", "h1", now - age_10y, None)
        .expect("upsert");
    let c_proc = IndexedChunk::new(
        "chunk_sop",
        "notes/sop_deploy.md",
        0,
        "Standard operating procedure for server deployment and maintenance.",
        9,
        CoalaType::Procedural,
        NoteStatus::Active,
        now - age_10y,
    );
    let r_proc = storage.index_chunk(&c_proc).expect("index");
    let v_proc = make_synthetic_vector(0.75);
    storage
        .insert_chunk_vector(r_proc, &v_proc)
        .expect("insert vec");

    // 2. Note B: 100-day-old Semantic Note
    // Higher vector similarity 0.85, no lexical keyword
    let age_100d = 100 * 86400;
    storage
        .upsert_file("notes/meeting_notes.md", "h2", now - age_100d, None)
        .expect("upsert");
    let c_sem = IndexedChunk::new(
        "chunk_sem_100d",
        "notes/meeting_notes.md",
        0,
        "Deployment discussion notes from Q2 architecture sync.",
        7,
        CoalaType::Semantic,
        NoteStatus::Active,
        now - age_100d,
    );
    let r_sem = storage.index_chunk(&c_sem).expect("index");
    let v_sem = make_synthetic_vector(0.85);
    storage
        .insert_chunk_vector(r_sem, &v_sem)
        .expect("insert vec");

    // 3. Note C: Fresh Semantic Note (0 days old)
    // Moderate vector similarity 0.78
    storage
        .upsert_file("notes/fresh_update.md", "h3", now, None)
        .expect("upsert");
    let c_fresh = IndexedChunk::new(
        "chunk_fresh",
        "notes/fresh_update.md",
        0,
        "Fresh deployment guidelines finalized today.",
        5,
        CoalaType::Semantic,
        NoteStatus::Active,
        now,
    );
    let r_fresh = storage.index_chunk(&c_fresh).expect("index");
    let v_fresh = make_synthetic_vector(0.78);
    storage
        .insert_chunk_vector(r_fresh, &v_fresh)
        .expect("insert vec");

    let engine = RagEngine::new(Arc::new(Mutex::new(storage)));

    // Query along e0 (dot product = s)
    let q_vec = make_basis_vector(0);
    let results = engine
        .search_at("deployment", &q_vec, 10, now)
        .expect("search success");

    assert_eq!(results.len(), 3);

    // Theoretical Combined Scores:
    // Fresh Semantic (0 days):
    // raw = 0.7 * 0.78 + 0.3 * 1.0 (or lexical factor)
    // denom = 1.0
    //
    // Old Procedural (3650 days):
    // raw = 0.7 * 0.75 + 0.3 * lexical
    // denom = 1.0 (immune to decay!)
    //
    // 100-day Semantic:
    // raw = 0.7 * 0.85 + 0.3 * lexical
    // denom = 1 + 0.005 * 100 = 1.5 (attenuated by 33%!)

    // Verify ranking order:
    // Note C (Fresh) ranks higher than Note A (Procedural)
    // Note A (10-year Procedural) ranks HIGHER than Note B (100-day Semantic) despite Note B having higher vector similarity!
    let pos_proc = results
        .iter()
        .position(|r| r.chunk_id == "chunk_sop")
        .unwrap();
    let pos_sem_100d = results
        .iter()
        .position(|r| r.chunk_id == "chunk_sem_100d")
        .unwrap();

    assert!(
        pos_proc < pos_sem_100d,
        "10-year-old Procedural Note (pos={pos_proc}) must outrank 100-day Semantic Note (pos={pos_sem_100d}) due to temporal decay attenuation"
    );
}

// ============================================================================
// 4. MATHEMATICAL MONOTONICITY & SOUNDNESS
// ============================================================================

#[test]
fn test_adv_score_09_strict_temporal_monotonicity_oracle() {
    let raw_v = 0.90;
    let raw_b = 0.70;

    let time_steps: Vec<f64> = vec![
        0.0, 1.0, 5.0, 15.0, 30.0, 60.0, 100.0, 200.0, 365.0, 500.0, 1000.0, 3650.0, 10000.0,
    ];

    // For Semantic & Episodic: strictly decreasing
    for coala_type in [CoalaType::Semantic, CoalaType::Episodic] {
        let mut prev_score = f64::INFINITY;
        for &t in &time_steps {
            let score = compute_hybrid_decay_score(raw_v, raw_b, coala_type, t);
            assert!(
                score < prev_score,
                "Score at t={t} ({score}) must be strictly less than previous ({prev_score}) for {:?}",
                coala_type
            );
            assert!(score > 0.0, "Score must remain strictly positive");
            prev_score = score;
        }
    }

    // For Procedural: strictly constant
    let reference_proc_score = compute_hybrid_decay_score(raw_v, raw_b, CoalaType::Procedural, 0.0);
    for &t in &time_steps {
        let score = compute_hybrid_decay_score(raw_v, raw_b, CoalaType::Procedural, t);
        assert!(
            (score - reference_proc_score).abs() < 1e-9,
            "Procedural score at t={t} ({score}) must be strictly identical to t=0 ({reference_proc_score})"
        );
    }
}

#[test]
fn test_adv_score_10_extreme_float_and_nan_resilience() {
    // 1. Circuit breaker with NaN
    let cb_nan = check_circuit_breaker(f32::NAN);
    assert_eq!(
        cb_nan,
        Err(RagError::InformationNotFound { similarity: 0.0 }),
        "Circuit breaker with NaN must fail and return similarity 0.0"
    );

    // 2. Circuit breaker with negative infinity
    let cb_neginf = check_circuit_breaker(f32::NEG_INFINITY);
    assert_eq!(
        cb_neginf,
        Err(RagError::InformationNotFound {
            similarity: f32::NEG_INFINITY
        }),
        "Circuit breaker with -inf must fail"
    );

    // 3. Compute score with astronomical age (1 billion days)
    let astro_age = 1e9f64;
    let score_astro = compute_hybrid_decay_score(1.0, 1.0, CoalaType::Semantic, astro_age);
    assert!(score_astro > 0.0, "Score must be positive");
    assert!(score_astro < 1e-6, "Score must be infinitesimal (< 1e-6)");
    assert!(!score_astro.is_nan(), "Score must not be NaN");
    assert!(!score_astro.is_infinite(), "Score must not be infinite");

    // 4. Compute score with NaN age
    let score_nan_age = compute_hybrid_decay_score(0.8, 0.5, CoalaType::Semantic, f64::NAN);
    let expected_raw = 0.7 * 0.8 + 0.3 * 0.5;
    assert!(
        (score_nan_age - expected_raw).abs() < 1e-9,
        "NaN age must be clamped to 0.0 via f64::max, yielding raw score"
    );
}
