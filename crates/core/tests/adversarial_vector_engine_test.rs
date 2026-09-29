//! Adversarial Stress Test Suite: sqlite-vec Integration, Vector Serialization & Cosine Geometry
//!
//! Written by Challenger M1 #1 to empirically challenge and stress-test:
//! 1. Little-Endian binary vector serialization (384D f32 -> 1,536 bytes) with extreme float representations.
//! 2. Cosine distance accuracy against mathematical ground truth (identical, orthogonal, anti-parallel, fractional angles).
//! 3. Scale-invariance and non-normalized vector handling.
//! 4. Zero vectors, near-zero magnitude vectors, subnormals, and boundary inputs.
//! 5. Vector upsert/overwrite atomicity and single-rowid churn (50 consecutive updates).
//! 6. k-NN limit boundaries: k=0, k=1, k=exact count, k > total count, k=usize::MAX.
//! 7. Monotonic ordering verification across multiple angular separations.
//! 8. 64-bit integer rowid extremes (including i64::MAX).
//! 9. Parity between in-memory and on-disk database instances.

use jeanne_core::models::{CoalaType, IndexedChunk, NoteStatus};
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

#[allow(dead_code)]
fn make_unnormalized_vector(seed: f32, scale: f32) -> [f32; 384] {
    let mut vec = [0.0f32; 384];
    for (i, item) in vec.iter_mut().enumerate() {
        *item = ((i as f32 + seed) * 0.23).cos() * scale;
    }
    vec
}

// ============================================================================
// 1. SERIALIZATION & EXTREME FLOAT BIT REPRESENTATION TESTS
// ============================================================================

#[test]
fn test_adv_vec_01_serialization_exactness_and_roundtrip_extremes() {
    // 1. All zeros
    let zero_vec = [0.0f32; 384];
    let zero_bytes = StorageManager::serialize_vector(&zero_vec);
    assert_eq!(zero_bytes.len(), 1536);
    assert!(zero_bytes.iter().all(|&b| b == 0));
    let zero_deser = StorageManager::deserialize_vector(&zero_bytes);
    assert_eq!(zero_vec, zero_deser);

    // 2. Negative zeros (-0.0f32)
    let mut neg_zero_vec = [0.0f32; 384];
    neg_zero_vec[0] = -0.0f32;
    neg_zero_vec[187] = -0.0f32;
    let neg_zero_bytes = StorageManager::serialize_vector(&neg_zero_vec);
    assert_eq!(
        neg_zero_bytes[3], 0x80,
        "Sign bit must be preserved in Little-Endian byte 3"
    );
    let neg_zero_deser = StorageManager::deserialize_vector(&neg_zero_bytes);
    assert_eq!(neg_zero_deser[0].to_bits(), (-0.0f32).to_bits());

    // 3. Subnormals and smallest positive normals
    let mut subnormal_vec = [0.0f32; 384];
    subnormal_vec[10] = f32::MIN_POSITIVE; // ~1.175494e-38
    subnormal_vec[20] = 1.4e-45_f32; // Smallest subnormal
    let subnormal_bytes = StorageManager::serialize_vector(&subnormal_vec);
    let subnormal_deser = StorageManager::deserialize_vector(&subnormal_bytes);
    assert_eq!(subnormal_deser[10].to_bits(), f32::MIN_POSITIVE.to_bits());
    assert_eq!(subnormal_deser[20].to_bits(), (1.4e-45_f32).to_bits());

    // 4. Large finite magnitudes
    let mut large_vec = [0.0f32; 384];
    large_vec[50] = 1e30_f32;
    large_vec[51] = -1e30_f32;
    large_vec[383] = f32::MAX;
    let large_bytes = StorageManager::serialize_vector(&large_vec);
    let large_deser = StorageManager::deserialize_vector(&large_bytes);
    assert_eq!(large_deser[50], 1e30_f32);
    assert_eq!(large_deser[51], -1e30_f32);
    assert_eq!(large_deser[383], f32::MAX);

    // 5. Endianness verification: float 1.0f32 in Little-Endian is [0x00, 0x00, 0x80, 0x3F]
    let e0 = make_basis_vector(0);
    let e0_bytes = StorageManager::serialize_vector(&e0);
    assert_eq!(e0_bytes[0], 0x00);
    assert_eq!(e0_bytes[1], 0x00);
    assert_eq!(e0_bytes[2], 0x80);
    assert_eq!(e0_bytes[3], 0x3F);
}

// ============================================================================
// 2. MATHEMATICAL ORACLE TESTS: COSINE GEOMETRY IN SQLITE-VEC
// ============================================================================

#[test]
fn test_adv_vec_02_cosine_distance_mathematical_oracles() {
    let storage = StorageManager::open_in_memory().expect("open storage");
    storage.init_schema().expect("init schema");

    storage
        .upsert_file("Notes/Geometry.md", "hash_geom", 1715000000, None)
        .expect("upsert file");

    let chunk = IndexedChunk::new(
        "geom:0",
        "Notes/Geometry.md",
        0,
        "Cosine geometry test chunk",
        5,
        CoalaType::Semantic,
        NoteStatus::Active,
        1715000000,
    );
    let r1 = storage.index_chunk(&chunk).expect("index chunk");

    // Case 1: Identical vector -> distance = 0.0
    let v_orig = make_synthetic_vector(123.45);
    storage.insert_chunk_vector(r1, &v_orig).expect("insert");
    let res_ident = storage.search_vector(&v_orig, 1).expect("search ident");
    assert_eq!(res_ident.len(), 1);
    assert_eq!(res_ident[0].0, r1);
    assert!(
        res_ident[0].1.abs() < 1e-5,
        "Identical vector distance must be 0.0, got {}",
        res_ident[0].1
    );

    // Case 2: Orthogonal vectors -> distance = 1.0
    let e0 = make_basis_vector(0);
    let e1 = make_basis_vector(1);
    storage.insert_chunk_vector(r1, &e0).expect("insert e0");
    let res_ortho = storage.search_vector(&e1, 1).expect("search e1");
    assert_eq!(res_ortho.len(), 1);
    assert!(
        (res_ortho[0].1 - 1.0).abs() < 1e-4,
        "Orthogonal vector distance must be 1.0, got {}",
        res_ortho[0].1
    );

    // Case 3: Anti-parallel vectors -> distance = 2.0
    let mut neg_e0 = [0.0f32; 384];
    neg_e0[0] = -1.0;
    let res_anti = storage.search_vector(&neg_e0, 1).expect("search neg_e0");
    assert_eq!(res_anti.len(), 1);
    assert!(
        (res_anti[0].1 - 2.0).abs() < 1e-4,
        "Anti-parallel vector distance must be 2.0, got {}",
        res_anti[0].1
    );

    // Case 4: 45-degree angle vector (cos(pi/4) = sqrt(2)/2 ~= 0.7071068)
    // distance = 1.0 - 0.7071068 = 0.2928932
    let mut v_45 = [0.0f32; 384];
    let val_45 = (2.0f32).sqrt() / 2.0;
    v_45[0] = val_45;
    v_45[1] = val_45;
    storage.insert_chunk_vector(r1, &e0).expect("insert e0");
    let res_45 = storage.search_vector(&v_45, 1).expect("search v_45");
    let expected_dist_45 = 1.0 - val_45;
    assert!(
        (res_45[0].1 - expected_dist_45).abs() < 1e-4,
        "45-degree distance expected {}, got {}",
        expected_dist_45,
        res_45[0].1
    );

    // Case 5: 60-degree angle vector (cos(pi/3) = 0.5)
    // distance = 1.0 - 0.5 = 0.5
    let mut v_60 = [0.0f32; 384];
    v_60[0] = 0.5;
    v_60[1] = (3.0f32).sqrt() / 2.0;
    let res_60 = storage.search_vector(&v_60, 1).expect("search v_60");
    assert!(
        (res_60[0].1 - 0.5).abs() < 1e-4,
        "60-degree distance expected 0.5, got {}",
        res_60[0].1
    );
}

// ============================================================================
// 3. SCALE-INVARIANCE & UNNORMALIZED VECTOR TESTS
// ============================================================================

#[test]
fn test_adv_vec_03_scale_invariance_and_unnormalized_vectors() {
    let storage = StorageManager::open_in_memory().expect("open storage");
    storage.init_schema().expect("init schema");

    storage
        .upsert_file("Notes/Scale.md", "hash_scale", 1715000000, None)
        .expect("upsert file");

    let chunk = IndexedChunk::new(
        "scale:0",
        "Notes/Scale.md",
        0,
        "Scale invariance chunk",
        4,
        CoalaType::Semantic,
        NoteStatus::Active,
        1715000000,
    );
    let r1 = storage.index_chunk(&chunk).expect("index chunk");

    // Insert normalized vector
    let v_base = make_synthetic_vector(42.0);
    storage
        .insert_chunk_vector(r1, &v_base)
        .expect("insert base");

    // Search with unnormalized scaled vectors: scale=100.0, scale=0.005, scale=5000.0
    for scale in [0.005f32, 0.1, 10.0, 100.0, 5000.0] {
        let mut scaled_query = [0.0f32; 384];
        for (i, &val) in v_base.iter().enumerate() {
            scaled_query[i] = val * scale;
        }
        let res = storage
            .search_vector(&scaled_query, 1)
            .expect("search scaled");
        assert_eq!(res.len(), 1);
        assert!(
            res[0].1.abs() < 1e-4,
            "Cosine distance must be scale invariant (scale={scale}, got distance {})",
            res[0].1
        );
    }
}

// ============================================================================
// 4. VECTOR OVERWRITES & SINGLE-ROWID CHURN
// ============================================================================

#[test]
fn test_adv_vec_04_rapid_single_rowid_overwrites() {
    let storage = StorageManager::open_in_memory().expect("open storage");
    storage.init_schema().expect("init schema");

    storage
        .upsert_file("Notes/Churn.md", "hash_churn", 1715000000, None)
        .expect("upsert file");

    let chunk = IndexedChunk::new(
        "churn:0",
        "Notes/Churn.md",
        0,
        "Rapid churn chunk",
        4,
        CoalaType::Semantic,
        NoteStatus::Active,
        1715000000,
    );
    let rowid = storage.index_chunk(&chunk).expect("index chunk");

    // Perform 50 consecutive vector updates on the exact same rowid
    for i in 0..50 {
        let v = make_synthetic_vector(i as f32);
        storage.insert_chunk_vector(rowid, &v).expect("insert vec");

        // Verify immediately that the search finds it with distance 0.0
        let res = storage.search_vector(&v, 1).expect("search current");
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].0, rowid);
        assert!(res[0].1.abs() < 1e-4);
    }

    // Verify virtual table row count remains exactly 1
    let count: i64 = storage
        .raw_connection()
        .query_row("SELECT COUNT(*) FROM vec_chunks;", [], |row| row.get(0))
        .expect("count rows");
    assert_eq!(
        count, 1,
        "vec_chunks must never duplicate rows upon overwrite"
    );
}

// ============================================================================
// 5. K-NN SEARCH LIMIT BOUNDARIES
// ============================================================================

#[test]
fn test_adv_vec_05_knn_limit_boundaries() {
    let storage = StorageManager::open_in_memory().expect("open storage");
    storage.init_schema().expect("init schema");

    let file_path = "Notes/KnnBounds.md";
    storage
        .upsert_file(file_path, "hash_knn", 1715000000, None)
        .expect("upsert file");

    let total_chunks = 15;
    let mut rowids = Vec::new();

    for i in 0..total_chunks {
        let chunk = IndexedChunk::new(
            format!("knn:{i}"),
            file_path,
            i,
            format!("Content for chunk {i}"),
            5,
            CoalaType::Semantic,
            NoteStatus::Active,
            1715000000,
        );
        let rid = storage.index_chunk(&chunk).expect("index");
        let v = make_synthetic_vector(i as f32 * 10.0);
        storage.insert_chunk_vector(rid, &v).expect("insert vec");
        rowids.push(rid);
    }

    let query_vec = make_synthetic_vector(0.0);

    // Boundary 1: limit = 0 -> must return empty vec without querying SQL
    let res_0 = storage.search_vector(&query_vec, 0).expect("limit 0");
    assert!(res_0.is_empty(), "limit = 0 must return empty Vec");

    // Boundary 2: limit = 1 -> must return exactly 1 closest match
    let res_1 = storage.search_vector(&query_vec, 1).expect("limit 1");
    assert_eq!(res_1.len(), 1);
    assert_eq!(res_1[0].0, rowids[0]);
    assert!(res_1[0].1.abs() < 1e-5);

    // Boundary 3: limit = 7 (intermediate) -> must return exactly 7 sorted
    let res_7 = storage.search_vector(&query_vec, 7).expect("limit 7");
    assert_eq!(res_7.len(), 7);
    for w in res_7.windows(2) {
        assert!(
            w[0].1 <= w[1].1,
            "Results must be strictly sorted by distance ({} <= {})",
            w[0].1,
            w[1].1
        );
    }

    // Boundary 4: limit = total_chunks (exact match) -> returns all 15
    let res_exact = storage
        .search_vector(&query_vec, total_chunks)
        .expect("limit exact");
    assert_eq!(res_exact.len(), total_chunks);

    // Boundary 5: limit = 100 (limit > total_chunks) -> returns all 15 without padding
    let res_over = storage.search_vector(&query_vec, 100).expect("limit 100");
    assert_eq!(
        res_over.len(),
        total_chunks,
        "k > total count must return all existing rows without error or padding"
    );

    // Boundary 6: limit = usize::MAX (integer overflow protection) -> does not panic
    let res_max = storage.search_vector(&query_vec, usize::MAX);
    assert!(res_max.is_ok(), "usize::MAX must not panic");
    assert_eq!(res_max.unwrap().len(), total_chunks);
}

// ============================================================================
// 6. MULTI-VECTOR ANGULAR MONOTONICITY ORACLE
// ============================================================================

#[test]
fn test_adv_vec_06_angular_distance_monotonicity() {
    let storage = StorageManager::open_in_memory().expect("open storage");
    storage.init_schema().expect("init schema");

    storage
        .upsert_file("Notes/Monotonic.md", "hash_mono", 1715000000, None)
        .expect("upsert file");

    // Insert 6 vectors at increasing angles: 0, 15, 30, 60, 90, 180 degrees
    let angles_deg = [0.0f32, 15.0, 30.0, 60.0, 90.0, 180.0];
    let mut rowids = Vec::new();

    for (i, &deg) in angles_deg.iter().enumerate() {
        let rad = deg.to_radians();
        let mut v = [0.0f32; 384];
        v[0] = rad.cos();
        v[1] = rad.sin();

        let chunk = IndexedChunk::new(
            format!("angle:{i}"),
            "Notes/Monotonic.md",
            i,
            format!("Vector at {deg} deg"),
            4,
            CoalaType::Semantic,
            NoteStatus::Active,
            1715000000,
        );
        let rid = storage.index_chunk(&chunk).expect("index");
        storage.insert_chunk_vector(rid, &v).expect("insert");
        rowids.push(rid);
    }

    // Query with vector at 0 degrees
    let mut q = [0.0f32; 384];
    q[0] = 1.0;

    let results = storage.search_vector(&q, 10).expect("search monotonic");
    assert_eq!(results.len(), angles_deg.len());

    // Verify distances strictly increase monotonically
    for i in 0..results.len() - 1 {
        assert!(
            results[i].1 <= results[i + 1].1,
            "Distance must strictly increase with angle: pos {} ({}) <= pos {} ({})",
            i,
            results[i].1,
            i + 1,
            results[i + 1].1
        );
    }

    // Verify rowid order matches the inserted angle order
    for (i, &rid) in rowids.iter().enumerate() {
        assert_eq!(
            results[i].0, rid,
            "Expected rowid {} at rank {} (angle {} deg)",
            rid, i, angles_deg[i]
        );
    }
}

// ============================================================================
// 7. EXTREME 64-BIT INTEGER ROWIDS (i64::MAX)
// ============================================================================

#[test]
fn test_adv_vec_07_extreme_64bit_integer_rowids() {
    let storage = StorageManager::open_in_memory().expect("open storage");
    storage.init_schema().expect("init schema");

    let extreme_rowids: [i64; 4] = [
        1,
        1_000_000_000,
        4_294_967_296, // 2^32 (overflows 32-bit unsigned/signed)
        i64::MAX,      // 9_223_372_036_854_775_807
    ];

    for (i, &rid) in extreme_rowids.iter().enumerate() {
        let v = make_synthetic_vector(i as f32 + 1.0);
        storage
            .insert_chunk_vector(rid, &v)
            .expect("insert extreme rowid");

        let res = storage.search_vector(&v, 1).expect("search extreme rowid");
        assert_eq!(res.len(), 1);
        assert_eq!(
            res[0].0, rid,
            "Retrieved rowid must match extreme 64-bit integer {rid}"
        );
        assert!(res[0].1.abs() < 1e-4);
    }
}

// ============================================================================
// 8. ON-DISK VS IN-MEMORY ENGINE PARITY
// ============================================================================

#[test]
fn test_adv_vec_08_ondisk_inmemory_parity() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("parity_test.db");

    let disk_storage = StorageManager::open(&db_path).expect("open disk");
    disk_storage.init_schema().expect("init disk");

    let mem_storage = StorageManager::open_in_memory().expect("open mem");
    mem_storage.init_schema().expect("init mem");

    let v1 = make_synthetic_vector(11.0);
    let v2 = make_synthetic_vector(22.0);

    // Insert into disk
    disk_storage.insert_chunk_vector(10, &v1).expect("disk v1");
    disk_storage.insert_chunk_vector(20, &v2).expect("disk v2");

    // Insert into memory
    mem_storage.insert_chunk_vector(10, &v1).expect("mem v1");
    mem_storage.insert_chunk_vector(20, &v2).expect("mem v2");

    let q = make_synthetic_vector(15.0);
    let disk_res = disk_storage.search_vector(&q, 2).expect("disk search");
    let mem_res = mem_storage.search_vector(&q, 2).expect("mem search");

    assert_eq!(disk_res.len(), mem_res.len());
    for i in 0..disk_res.len() {
        assert_eq!(
            disk_res[i].0, mem_res[i].0,
            "Rowids must match between disk and mem"
        );
        assert!(
            (disk_res[i].1 - mem_res[i].1).abs() < 1e-5,
            "Distances must match between disk ({}) and mem ({})",
            disk_res[i].1,
            mem_res[i].1
        );
    }
}
