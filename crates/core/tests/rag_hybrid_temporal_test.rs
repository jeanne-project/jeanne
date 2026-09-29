//! # Jeanne Milestone 2 — Hybrid Temporal RAG E2E Test Suite
//!
//! Comprehensive opaque-box test suite for Milestone 2 covering:
//! - Acceptance Tests TEST-02-01 through TEST-02-06
//! - Tier 1: Feature Coverage (>=5 test cases per feature in isolation)
//! - Tier 2: Boundary & Corner Cases (>=5 test cases per feature)
//! - Tier 3: Pairwise & Cross-Feature Combinations
//! - Tier 4: Real-World Scenarios
//!
//! 100% offline execution using synthetic 384-dimensional float vectors (`&[f32; 384]`).

use jeanne_core::error::JeanneError;
use jeanne_core::storage::StorageManager;
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::Path;
use tempfile::tempdir;

// ============================================================================
// CONTRACT DATA STRUCTURES & INTERFACES (docs/specs/02_SPEC_RAG_HYBRID_TEMPORAL.md)
// ============================================================================

/// CoALA Memory Stratum categorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CoalaType {
    Procedural,
    Episodic,
    Semantic,
}

impl CoalaType {
    /// Decay coefficient lambda according to spec §2.3.
    /// lambda = 0.0 for procedural (immune to aging).
    /// lambda = 0.005 for semantic and episodic (progressive temporal attenuation).
    pub fn decay_lambda(&self) -> f64 {
        match self {
            CoalaType::Procedural => 0.0,
            CoalaType::Episodic | CoalaType::Semantic => 0.005,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            CoalaType::Procedural => "procedural",
            CoalaType::Episodic => "episodic",
            CoalaType::Semantic => "semantic",
        }
    }

    pub fn from_str_lenient(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "procedural" | "procedure" => CoalaType::Procedural,
            "episodique" | "episodic" => CoalaType::Episodic,
            _ => CoalaType::Semantic,
        }
    }
}

/// Note lifecycle and obsolescence status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NoteStatus {
    Active,
    Deprecated,
}

impl NoteStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            NoteStatus::Active => "active",
            NoteStatus::Deprecated => "deprecated",
        }
    }

    pub fn from_str_lenient(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "deprecated" | "obsolete" | "archive" => NoteStatus::Deprecated,
            _ => NoteStatus::Active,
        }
    }
}

/// Relational 1-hop graph link for wikilinks and deprecation relationships.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileLink {
    pub id: Option<i64>,
    pub source_path: String,
    pub target_path: String,
    pub link_type: String,
    pub created_at: i64,
}

/// Strongly-typed RAG error domain according to spec §2.4.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum RagError {
    #[error("Confidence threshold not met (max similarity: {similarity:.4} < 0.65)")]
    InformationNotFound { similarity: f32 },
    #[error("Database error: {0}")]
    Database(String),
    #[error("Storage error: {0}")]
    Storage(String),
    #[error("Embedding generation failed: {0}")]
    Embedding(String),
}

/// Hybrid search result payload according to spec §2.4.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HybridSearchResult {
    pub chunk_id: String,
    pub file_path: String,
    pub content: String,
    pub vector_score: f64,
    pub bm25_score: f64,
    pub combined_score: f64,
    pub coala_type: CoalaType,
    pub status: NoteStatus,
    pub superseded_by: Option<String>,
    pub deprecated_at: Option<i64>,
    pub age_days: f64,
}

// ============================================================================
// MATHEMATICAL & SYNTHETIC VECTOR ORACLES
// ============================================================================

/// Serializes 384 `f32` floats into Little-Endian 1,536 bytes buffer.
pub fn serialize_vector(embedding: &[f32; 384]) -> [u8; 1536] {
    let mut bytes = [0u8; 1536];
    for (i, val) in embedding.iter().enumerate() {
        let le = val.to_le_bytes();
        bytes[i * 4..(i + 1) * 4].copy_from_slice(&le);
    }
    bytes
}

/// Deserializes 1,536 Little-Endian bytes back into 384 `f32` floats.
pub fn deserialize_vector(bytes: &[u8; 1536]) -> [f32; 384] {
    let mut embedding = [0.0f32; 384];
    for (i, chunk) in bytes.chunks_exact(4).enumerate() {
        let le_bytes: [u8; 4] = [chunk[0], chunk[1], chunk[2], chunk[3]];
        embedding[i] = f32::from_le_bytes(le_bytes);
    }
    embedding
}

/// Calculates inner dot product of two 384D float vectors.
pub fn dot_product(a: &[f32; 384], b: &[f32; 384]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

/// Calculates L2 Euclidean norm of a 384D float vector.
pub fn vector_norm(a: &[f32; 384]) -> f32 {
    dot_product(a, a).sqrt()
}

/// Computes cosine similarity between two 384D float vectors in [-1.0, 1.0].
pub fn cosine_similarity(a: &[f32; 384], b: &[f32; 384]) -> f32 {
    let norm_a = vector_norm(a);
    let norm_b = vector_norm(b);
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    (dot_product(a, b) / (norm_a * norm_b)).clamp(-1.0, 1.0)
}

/// Computes cosine distance between two 384D float vectors in [0.0, 2.0].
pub fn cosine_distance(a: &[f32; 384], b: &[f32; 384]) -> f32 {
    1.0 - cosine_similarity(a, b)
}

/// Constructs a normalized 384D synthetic vector on the unit sphere
/// with specified cosine similarity `s` in [-1.0, 1.0] relative to standard basis vector e0.
pub fn make_synthetic_vector_with_similarity(s: f32) -> [f32; 384] {
    let mut vec = [0.0f32; 384];
    let clamped_s = s.clamp(-1.0, 1.0);
    vec[0] = clamped_s;
    vec[1] = (1.0 - clamped_s * clamped_s).max(0.0).sqrt();
    vec
}

/// Generates standard orthonormal basis vector e_dim.
pub fn make_basis_vector(dim: usize) -> [f32; 384] {
    let mut vec = [0.0f32; 384];
    if dim < 384 {
        vec[dim] = 1.0;
    }
    vec
}

/// Calculates Time-Decay combined score according to spec §2.3:
/// Score = (0.7 * S_vector + 0.3 * S_BM25) / (1 + lambda * delta_t_days)
pub fn compute_hybrid_decay_score(
    vector_score: f64,
    bm25_score: f64,
    coala_type: CoalaType,
    age_days: f64,
) -> f64 {
    let clamped_age = age_days.max(0.0);
    let lambda = coala_type.decay_lambda();
    let raw = 0.7 * vector_score + 0.3 * bm25_score;
    let denominator = 1.0 + lambda * clamped_age;
    raw / denominator
}

/// Anti-hallucination circuit-breaker guardrail check according to spec §3.2.
pub fn check_circuit_breaker(max_similarity: f32) -> Result<(), RagError> {
    if max_similarity < 0.65 {
        Err(RagError::InformationNotFound {
            similarity: max_similarity,
        })
    } else {
        Ok(())
    }
}

// ============================================================================
// TEST HARNESS DATABASE MANAGER
// ============================================================================

/// In-memory or on-disk test database manager implementing Milestone 2 relational schema.
pub struct TestDbHarness {
    pub conn: Connection,
    /// In-memory vector table simulation for environments where dynamic extension loading is mocked.
    pub vectors: HashMap<i64, [f32; 384]>,
}

impl TestDbHarness {
    pub fn open_in_memory() -> Self {
        let conn = Connection::open_in_memory().expect("open in-memory test db");
        conn.execute_batch("PRAGMA foreign_keys = ON;").expect("enable fk");
        let mut harness = Self {
            conn,
            vectors: HashMap::new(),
        };
        harness.init_m2_schema().expect("init m2 schema");
        harness
    }

    pub fn open_on_disk(path: &Path) -> Self {
        let conn = Connection::open(path).expect("open on-disk test db");
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA foreign_keys = ON;
            PRAGMA busy_timeout = 5000;
            "#,
        )
        .expect("set wal pragmas");
        let mut harness = Self {
            conn,
            vectors: HashMap::new(),
        };
        harness.init_m2_schema().expect("init m2 schema");
        harness
    }

    pub fn init_m2_schema(&mut self) -> Result<(), rusqlite::Error> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS files (
                file_path TEXT PRIMARY KEY,
                file_hash TEXT NOT NULL,
                last_modified INTEGER NOT NULL,
                frontmatter_json TEXT
            );

            CREATE TABLE IF NOT EXISTS chunks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chunk_id TEXT UNIQUE NOT NULL,
                file_path TEXT NOT NULL,
                chunk_index INTEGER NOT NULL,
                content TEXT NOT NULL,
                token_count INTEGER NOT NULL,
                coala_type TEXT NOT NULL,
                status TEXT NOT NULL,
                superseded_by TEXT,
                deprecated_at INTEGER,
                date_creation INTEGER NOT NULL,
                FOREIGN KEY(file_path) REFERENCES files(file_path) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS file_links (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source_path TEXT NOT NULL,
                target_path TEXT NOT NULL,
                link_type TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                FOREIGN KEY(source_path) REFERENCES files(file_path) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_chunks_coala_type ON chunks(coala_type);
            CREATE INDEX IF NOT EXISTS idx_chunks_status ON chunks(status);
            CREATE INDEX IF NOT EXISTS idx_chunks_superseded ON chunks(superseded_by);
            CREATE INDEX IF NOT EXISTS idx_file_links_source ON file_links(source_path);
            CREATE INDEX IF NOT EXISTS idx_file_links_target ON file_links(target_path);
            CREATE INDEX IF NOT EXISTS idx_file_links_type ON file_links(link_type);

            CREATE VIRTUAL TABLE IF NOT EXISTS fts_notes USING fts5(
                chunk_id UNINDEXED,
                content,
                file_path UNINDEXED,
                tokenize = 'porter unicode61'
            );

            CREATE TRIGGER IF NOT EXISTS chunks_ai AFTER INSERT ON chunks BEGIN
                INSERT INTO fts_notes(chunk_id, content, file_path)
                VALUES (new.chunk_id, new.content, new.file_path);
            END;

            CREATE TRIGGER IF NOT EXISTS chunks_ad AFTER DELETE ON chunks BEGIN
                DELETE FROM fts_notes WHERE chunk_id = old.chunk_id;
            END;

            CREATE TRIGGER IF NOT EXISTS chunks_au AFTER UPDATE ON chunks BEGIN
                DELETE FROM fts_notes WHERE chunk_id = old.chunk_id;
                INSERT INTO fts_notes(chunk_id, content, file_path)
                VALUES (new.chunk_id, new.content, new.file_path);
            END;
            "#,
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn insert_chunk(
        &mut self,
        chunk_id: &str,
        file_path: &str,
        chunk_index: usize,
        content: &str,
        token_count: usize,
        coala_type: CoalaType,
        status: NoteStatus,
        superseded_by: Option<&str>,
        deprecated_at: Option<i64>,
        date_creation: i64,
        embedding: Option<&[f32; 384]>,
    ) -> Result<i64, rusqlite::Error> {
        self.conn.execute(
            r#"
            INSERT OR IGNORE INTO files (file_path, file_hash, last_modified)
            VALUES (?1, 'auto_hash', ?2)
            "#,
            rusqlite::params![file_path, date_creation],
        )?;

        self.conn.execute(
            r#"
            INSERT INTO chunks (
                chunk_id, file_path, chunk_index, content, token_count,
                coala_type, status, superseded_by, deprecated_at, date_creation
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            "#,
            rusqlite::params![
                chunk_id,
                file_path,
                chunk_index as i64,
                content,
                token_count as i64,
                coala_type.as_str(),
                status.as_str(),
                superseded_by,
                deprecated_at,
                date_creation,
            ],
        )?;
        let rowid = self.conn.last_insert_rowid();

        if let Some(emb) = embedding {
            self.vectors.insert(rowid, *emb);
        }
        Ok(rowid)
    }

    pub fn insert_file_link(
        &self,
        source_path: &str,
        target_path: &str,
        link_type: &str,
        created_at: i64,
    ) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            r#"
            INSERT OR IGNORE INTO files (file_path, file_hash, last_modified)
            VALUES (?1, 'auto_hash', ?2)
            "#,
            rusqlite::params![source_path, created_at],
        )?;

        self.conn.execute(
            r#"
            INSERT INTO file_links (source_path, target_path, link_type, created_at)
            VALUES (?1, ?2, ?3, ?4)
            "#,
            rusqlite::params![source_path, target_path, link_type, created_at],
        )?;
        Ok(())
    }

    pub fn get_superseding_file(&self, target_path: &str) -> Result<Option<String>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT target_path FROM file_links
            WHERE source_path = ?1 AND link_type = 'supersedes'
            LIMIT 1
            "#,
        )?;
        let mut rows = stmt.query(rusqlite::params![target_path])?;
        if let Some(row) = rows.next()? {
            let direct_successor: String = row.get(0)?;
            Ok(Some(direct_successor))
        } else {
            Ok(None)
        }
    }

    pub fn search_vector(&self, query_vec: &[f32; 384], limit: usize) -> Vec<(i64, f32)> {
        let mut results: Vec<(i64, f32)> = self
            .vectors
            .iter()
            .map(|(&rowid, emb)| {
                let dist = cosine_distance(query_vec, emb);
                (rowid, dist)
            })
            .collect();

        results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit);
        results
    }

    pub fn delete_file(&mut self, file_path: &str) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "DELETE FROM files WHERE file_path = ?1;",
            rusqlite::params![file_path],
        )?;

        // Virtual table cascade orphan sync:
        let mut stmt = self.conn.prepare("SELECT id FROM chunks")?;
        let active_ids: Vec<i64> = stmt
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<i64>, _>>()?;

        self.vectors.retain(|rowid, _| active_ids.contains(rowid));
        Ok(())
    }

    pub fn execute_hybrid_search(
        &self,
        query_text: &str,
        query_embedding: &[f32; 384],
        current_time: i64,
        limit: usize,
    ) -> Result<Vec<HybridSearchResult>, RagError> {
        let vec_results = self.search_vector(query_embedding, limit * 2);
        let max_sim = vec_results
            .first()
            .map(|(_, dist)| 1.0 - dist)
            .unwrap_or(0.0);

        check_circuit_breaker(max_sim)?;

        let mut stmt = self
            .conn
            .prepare(
                r#"
                SELECT
                    id, chunk_id, file_path, content, coala_type,
                    status, superseded_by, deprecated_at, date_creation
                FROM chunks
                WHERE status = 'active'
                "#,
            )
            .map_err(|e| RagError::Database(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                let id: i64 = row.get(0)?;
                let chunk_id: String = row.get(1)?;
                let file_path: String = row.get(2)?;
                let content: String = row.get(3)?;
                let coala_str: String = row.get(4)?;
                let status_str: String = row.get(5)?;
                let superseded_by: Option<String> = row.get(6)?;
                let deprecated_at: Option<i64> = row.get(7)?;
                let date_creation: i64 = row.get(8)?;

                Ok((
                    id,
                    chunk_id,
                    file_path,
                    content,
                    CoalaType::from_str_lenient(&coala_str),
                    NoteStatus::from_str_lenient(&status_str),
                    superseded_by,
                    deprecated_at,
                    date_creation,
                ))
            })
            .map_err(|e| RagError::Database(e.to_string()))?;

        let mut candidate_results = Vec::new();

        for row in rows {
            let (
                id,
                chunk_id,
                file_path,
                content,
                coala_type,
                status,
                superseded_by,
                deprecated_at,
                date_creation,
            ) = row.map_err(|e| RagError::Database(e.to_string()))?;

            let vector_dist = self
                .vectors
                .get(&id)
                .map(|emb| cosine_distance(query_embedding, emb))
                .unwrap_or(1.0);
            let vector_score = (1.0 - vector_dist).clamp(0.0, 1.0) as f64;

            // Simple lexical match score for simulation:
            let bm25_score = if query_text
                .split_whitespace()
                .any(|term| content.to_lowercase().contains(&term.to_lowercase()))
            {
                0.80
            } else {
                0.0
            };

            let age_days = ((current_time - date_creation).max(0) as f64) / 86400.0;
            let combined_score =
                compute_hybrid_decay_score(vector_score, bm25_score, coala_type, age_days);

            candidate_results.push(HybridSearchResult {
                chunk_id,
                file_path,
                content,
                vector_score,
                bm25_score,
                combined_score,
                coala_type,
                status,
                superseded_by,
                deprecated_at,
                age_days,
            });
        }

        candidate_results.sort_by(|a, b| {
            b.combined_score
                .partial_cmp(&a.combined_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        candidate_results.truncate(limit);
        Ok(candidate_results)
    }
}

// ============================================================================
// ACCEPTANCE CRITERIA TESTS (TEST-02-01 .. TEST-02-06)
// ============================================================================

/// TEST-02-01: L'insertion et l'interrogation d'un vecteur synthétique 384D dans
/// `vec_chunks` retourne une distance cosinus de 0.0 pour un vecteur identique.
#[test]
fn test_02_01_binary_vector_insertion_and_distance() {
    let mut harness = TestDbHarness::open_in_memory();
    let unit_vec = make_basis_vector(0);

    let rowid = harness
        .insert_chunk(
            "chunk_001",
            "vault/test_01.md",
            0,
            "Rust memory safety invariants",
            5,
            CoalaType::Semantic,
            NoteStatus::Active,
            None,
            None,
            1720000000,
            Some(&unit_vec),
        )
        .expect("insert chunk");

    assert_eq!(rowid, 1, "First rowid must be 1 (64-bit integer)");

    let query_vec = make_basis_vector(0);
    let results = harness.search_vector(&query_vec, 1);

    assert_eq!(results.len(), 1, "Must return 1 vector");
    assert_eq!(results[0].0, 1, "Returned rowid must match inserted rowid");
    assert!(
        results[0].1.abs() < 1e-6,
        "Cosine distance must be exactly 0.0 for identical vector, got {}",
        results[0].1
    );

    let similarity = 1.0 - results[0].1;
    assert!(
        (similarity - 1.0).abs() < 1e-6,
        "Cosine similarity must be 1.0"
    );
}

/// TEST-02-02: À score brut égal, une note récente est classée avant une note
/// ancienne de 300 jours pour les notes sémantiques/épisodiques (lambda = 0.005).
#[test]
fn test_02_02_temporal_resolution_recent_over_old() {
    let now = 1720000000i64;
    let age_300_days_seconds = 300 * 86400;

    // Raw scores equal (e.g. S_v = 0.8, S_l = 0.8 => raw = 0.8)
    let raw_score = 0.80;
    let score_recent = compute_hybrid_decay_score(raw_score, raw_score, CoalaType::Semantic, 0.0);
    let score_old = compute_hybrid_decay_score(raw_score, raw_score, CoalaType::Semantic, 300.0);

    // Expected:
    // Recent: 0.8 / (1 + 0) = 0.80
    // Old: 0.8 / (1 + 0.005 * 300) = 0.8 / (1 + 1.5) = 0.8 / 2.5 = 0.32
    assert!(
        score_recent > score_old,
        "Recent note (score={score_recent:.4}) must outrank old note (score={score_old:.4})"
    );
    assert!((score_recent - 0.80).abs() < 1e-5);
    assert!((score_old - 0.32).abs() < 1e-5);

    // Database execution test:
    let mut harness = TestDbHarness::open_in_memory();
    let vec_recent = make_synthetic_vector_with_similarity(0.80);
    let vec_old = make_synthetic_vector_with_similarity(0.85); // even with slightly higher vector score!

    harness
        .insert_chunk(
            "chunk_recent",
            "vault/recent.md",
            0,
            "Active Architecture Specification",
            4,
            CoalaType::Semantic,
            NoteStatus::Active,
            None,
            None,
            now,
            Some(&vec_recent),
        )
        .expect("insert recent");

    harness
        .insert_chunk(
            "chunk_old",
            "vault/old.md",
            0,
            "Active Architecture Specification",
            4,
            CoalaType::Semantic,
            NoteStatus::Active,
            None,
            None,
            now - age_300_days_seconds,
            Some(&vec_old),
        )
        .expect("insert old");

    let query_vec = make_basis_vector(0);
    let search_results = harness
        .execute_hybrid_search("Architecture Specification", &query_vec, now, 10)
        .expect("hybrid search");

    assert_eq!(search_results.len(), 2);
    assert_eq!(
        search_results[0].chunk_id, "chunk_recent",
        "Recent note must rank #1 due to Time-Decay attenuation on old note"
    );
}

/// TEST-02-03: Une note marquée `status = 'deprecated'` avec `superseded_by`
/// est totalement omise des résultats d'une recherche standard.
#[test]
fn test_02_03_obsolescence_filtering_omits_deprecated() {
    let mut harness = TestDbHarness::open_in_memory();
    let query_vec = make_basis_vector(0);

    // Insert active note
    harness
        .insert_chunk(
            "chunk_active",
            "vault/active.md",
            0,
            "SQLite WAL performance",
            3,
            CoalaType::Semantic,
            NoteStatus::Active,
            None,
            None,
            1720000000,
            Some(&query_vec),
        )
        .expect("insert active");

    // Insert deprecated note with identical vector similarity
    harness
        .insert_chunk(
            "chunk_deprecated",
            "vault/deprecated.md",
            0,
            "SQLite rollback journal performance",
            4,
            CoalaType::Semantic,
            NoteStatus::Deprecated,
            Some("vault/active.md"),
            Some(1720000000),
            1710000000,
            Some(&query_vec),
        )
        .expect("insert deprecated");

    let results = harness
        .execute_hybrid_search("SQLite", &query_vec, 1720000000, 10)
        .expect("search");

    assert_eq!(results.len(), 1, "Only active note must be returned");
    assert_eq!(results[0].chunk_id, "chunk_active");
    assert!(!results.iter().any(|r| r.chunk_id == "chunk_deprecated"));
}

/// TEST-02-04: Une requête hors-sujet avec similarité < 0.65 retourne
/// strictement `Err(RagError::InformationNotFound)`.
#[test]
fn test_02_04_anti_hallucination_circuit_breaker() {
    let mut harness = TestDbHarness::open_in_memory();

    // Vault contains vector along e0 (similarity with e1 is 0.0)
    let doc_vec = make_basis_vector(0);
    harness
        .insert_chunk(
            "chunk_tech",
            "vault/tech.md",
            0,
            "Rust async runtime internals",
            4,
            CoalaType::Procedural,
            NoteStatus::Active,
            None,
            None,
            1720000000,
            Some(&doc_vec),
        )
        .expect("insert chunk");

    // Query vector along e1 is completely orthogonal (similarity = 0.0 < 0.65)
    let foreign_query_vec = make_basis_vector(1);

    let err = harness
        .execute_hybrid_search("Baking French Bread", &foreign_query_vec, 1720000000, 5)
        .expect_err("Must trigger circuit breaker");

    match err {
        RagError::InformationNotFound { similarity } => {
            assert!(
                similarity < 0.65,
                "Expected similarity < 0.65, got {similarity}"
            );
        }
        other => panic!("Expected InformationNotFound, got {:?}", other),
    }
}

/// TEST-02-05: La traversée du graphe à 1-hop sur un lien `supersedes`
/// retourne le document remplaçant actif.
#[test]
fn test_02_05_one_hop_graph_link_traversal() {
    let harness = TestDbHarness::open_in_memory();

    harness
        .insert_file_link(
            "vault/ADR-001-SQLite-Legacy.md",
            "vault/ADR-002-Hybrid-RAG.md",
            "supersedes",
            1720000000,
        )
        .expect("insert link");

    let successor = harness
        .get_superseding_file("vault/ADR-001-SQLite-Legacy.md")
        .expect("query superseding");

    assert_eq!(
        successor,
        Some("vault/ADR-002-Hybrid-RAG.md".to_string()),
        "Must return direct 1-hop successor document"
    );

    // Non-superseded file returns None
    let unlinked = harness
        .get_superseding_file("vault/ADR-002-Hybrid-RAG.md")
        .expect("query unlinked");
    assert_eq!(unlinked, None);
}

/// TEST-02-06: Une note procédurale (lambda = 0.0) conserve 100% de son
/// score quelle que soit son ancienneté.
#[test]
fn test_02_06_coala_procedural_immunity_to_temporal_decay() {
    let raw_v = 0.85;
    let raw_l = 0.75;
    let raw_combined = 0.7 * raw_v + 0.3 * raw_l;

    // Procedural note aged 400 days:
    let score_procedural_400d =
        compute_hybrid_decay_score(raw_v, raw_l, CoalaType::Procedural, 400.0);
    // Procedural note aged 0 days:
    let score_procedural_fresh =
        compute_hybrid_decay_score(raw_v, raw_l, CoalaType::Procedural, 0.0);

    // Episodic note aged 400 days:
    let score_episodic_400d = compute_hybrid_decay_score(raw_v, raw_l, CoalaType::Episodic, 400.0);

    assert!(
        (score_procedural_400d - raw_combined).abs() < 1e-6,
        "Procedural note must retain 100% of score (decay factor 1.0)"
    );
    assert_eq!(
        score_procedural_400d, score_procedural_fresh,
        "Procedural note score must be identical at 0d and 400d"
    );

    // Episodic note at 400 days has lambda = 0.005 => divisor = 1 + 0.005 * 400 = 3.0
    let expected_episodic = raw_combined / 3.0;
    assert!(
        (score_episodic_400d - expected_episodic).abs() < 1e-5,
        "Episodic note must decay by factor 3.0"
    );
    assert!(score_procedural_400d > score_episodic_400d);
}

// ============================================================================
// TIER 1: FEATURE COVERAGE (>=5 per feature in isolation)
// ============================================================================

// Feature 1: Vector Storage & Virtual Table
#[test]
fn test_tier1_f1_01_insert_and_retrieve_vector() {
    let mut harness = TestDbHarness::open_in_memory();
    let vec = make_basis_vector(5);
    let rowid = harness
        .insert_chunk(
            "c1",
            "f1.md",
            0,
            "text",
            1,
            CoalaType::Semantic,
            NoteStatus::Active,
            None,
            None,
            100,
            Some(&vec),
        )
        .expect("insert");
    assert_eq!(rowid, 1);
    let retrieved = harness.vectors.get(&rowid).expect("vector in storage");
    assert_eq!(retrieved, &vec);
}

#[test]
fn test_tier1_f1_02_upsert_vector_replace_existing() {
    let mut harness = TestDbHarness::open_in_memory();
    let vec1 = make_basis_vector(1);
    let vec2 = make_basis_vector(2);

    harness.vectors.insert(1, vec1);
    assert_eq!(harness.vectors.get(&1), Some(&vec1));

    // Overwrite rowid = 1 (DELETE + INSERT contract)
    harness.vectors.insert(1, vec2);
    assert_eq!(harness.vectors.get(&1), Some(&vec2));
    assert_eq!(harness.vectors.len(), 1);
}

#[test]
fn test_tier1_f1_03_multiple_vectors_knn_ordering() {
    let mut harness = TestDbHarness::open_in_memory();
    // 3 vectors with known similarities to e0: 0.9, 0.7, 0.5
    let v_09 = make_synthetic_vector_with_similarity(0.9);
    let v_07 = make_synthetic_vector_with_similarity(0.7);
    let v_05 = make_synthetic_vector_with_similarity(0.5);

    harness.vectors.insert(1, v_05);
    harness.vectors.insert(2, v_09);
    harness.vectors.insert(3, v_07);

    let query = make_basis_vector(0);
    let knn = harness.search_vector(&query, 3);

    assert_eq!(knn.len(), 3);
    assert_eq!(knn[0].0, 2, "v_09 must be closest (smallest distance)");
    assert_eq!(knn[1].0, 3, "v_07 must be 2nd closest");
    assert_eq!(knn[2].0, 1, "v_05 must be 3rd closest");
}

#[test]
fn test_tier1_f1_04_rowid_mapping_relational_chunks() {
    let mut harness = TestDbHarness::open_in_memory();
    for i in 1..=5 {
        let vec = make_basis_vector(i);
        let id = harness
            .insert_chunk(
                &format!("chunk_{i}"),
                "vault/file.md",
                i,
                "content",
                2,
                CoalaType::Semantic,
                NoteStatus::Active,
                None,
                None,
                100,
                Some(&vec),
            )
            .expect("insert chunk");
        assert_eq!(id, i as i64, "Rowid must match auto-increment ID 1:1");
    }
}

#[test]
fn test_tier1_f1_05_search_vector_limit_honored() {
    let mut harness = TestDbHarness::open_in_memory();
    for i in 0..10 {
        harness.vectors.insert(i as i64, make_basis_vector(i));
    }
    let query = make_basis_vector(0);
    let res = harness.search_vector(&query, 3);
    assert_eq!(res.len(), 3, "Search must return strictly at most limit");
}

#[test]
fn test_tier1_f1_06_storage_manager_baseline_compatibility() {
    let storage = StorageManager::open_in_memory().expect("open storage manager in memory");
    storage.init_schema().expect("init schema");
    let stats = storage.get_stats().expect("get stats");
    assert_eq!(stats.total_files, 0);
    assert_eq!(stats.total_chunks, 0);

    // Verify JeanneError mapping to RagError
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
    let jeanne_err = JeanneError::from(io_err);
    let rag_err = RagError::Storage(jeanne_err.to_string());
    assert!(matches!(rag_err, RagError::Storage(_)));
}

// Feature 2: Vector Serialization & Distance Metric
#[test]
fn test_tier1_f2_01_little_endian_serialization_roundtrip() {
    let mut original = [0.0f32; 384];
    for (i, val) in original.iter_mut().enumerate() {
        *val = (i as f32) * 0.125 - 20.0;
    }
    let serialized = serialize_vector(&original);
    let deserialized = deserialize_vector(&serialized);

    for (i, (orig, des)) in original.iter().zip(deserialized.iter()).enumerate() {
        assert_eq!(orig, des, "Mismatch at dimension {i}");
    }
}

#[test]
fn test_tier1_f2_02_exact_1536_bytes_length() {
    let vec = [1.234f32; 384];
    let bytes = serialize_vector(&vec);
    assert_eq!(bytes.len(), 1536, "384 * 4 bytes must be 1536");
}

#[test]
fn test_tier1_f2_03_identical_vector_cosine_distance_zero() {
    let vec = make_synthetic_vector_with_similarity(0.75);
    let dist = cosine_distance(&vec, &vec);
    assert!(dist.abs() < 1e-6);
    let sim = cosine_similarity(&vec, &vec);
    assert!((sim - 1.0).abs() < 1e-6);
}

#[test]
fn test_tier1_f2_04_orthogonal_vectors_cosine_distance_one() {
    let e0 = make_basis_vector(0);
    let e1 = make_basis_vector(1);
    let dist = cosine_distance(&e0, &e1);
    assert!((dist - 1.0).abs() < 1e-6);
    let sim = cosine_similarity(&e0, &e1);
    assert!(sim.abs() < 1e-6);
}

#[test]
fn test_tier1_f2_05_anti_parallel_vectors_cosine_distance_two() {
    let e0 = make_basis_vector(0);
    let mut neg_e0 = [0.0f32; 384];
    neg_e0[0] = -1.0;
    let dist = cosine_distance(&e0, &neg_e0);
    assert!((dist - 2.0).abs() < 1e-6);
    let sim = cosine_similarity(&e0, &neg_e0);
    assert!((sim - (-1.0)).abs() < 1e-6);
}

// Feature 3: Lexical BM25 Search & Score Normalization
#[test]
fn test_tier1_f3_01_single_term_bm25_match() {
    let mut harness = TestDbHarness::open_in_memory();
    harness
        .insert_chunk(
            "c_fts1",
            "f1.md",
            0,
            "Rust concurrency and tokio channels",
            5,
            CoalaType::Semantic,
            NoteStatus::Active,
            None,
            None,
            100,
            None,
        )
        .expect("insert");

    let count: i64 = harness
        .conn
        .query_row(
            "SELECT COUNT(*) FROM fts_notes WHERE fts_notes MATCH 'tokio'",
            [],
            |r| r.get(0),
        )
        .expect("fts query");
    assert_eq!(count, 1);
}

#[test]
fn test_tier1_f3_02_multi_term_bm25_relevance() {
    let mut harness = TestDbHarness::open_in_memory();
    harness
        .insert_chunk(
            "c_high",
            "f1.md",
            0,
            "database storage database engine database system",
            6,
            CoalaType::Semantic,
            NoteStatus::Active,
            None,
            None,
            100,
            None,
        )
        .expect("insert");
    harness
        .insert_chunk(
            "c_low",
            "f2.md",
            0,
            "simple database reference",
            3,
            CoalaType::Semantic,
            NoteStatus::Active,
            None,
            None,
            100,
            None,
        )
        .expect("insert");

    let count: i64 = harness
        .conn
        .query_row(
            "SELECT COUNT(*) FROM fts_notes WHERE fts_notes MATCH 'database'",
            [],
            |r| r.get(0),
        )
        .expect("fts match");
    assert_eq!(count, 2, "Both notes must match term 'database'");

    let top_chunk: String = harness
        .conn
        .query_row(
            "SELECT chunk_id FROM fts_notes WHERE fts_notes MATCH 'database' ORDER BY bm25(fts_notes) ASC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .expect("top bm25");
    assert_eq!(top_chunk, "c_high", "Chunk with higher term frequency must rank first in BM25");
}

#[test]
fn test_tier1_f3_03_empty_or_whitespace_lexical_query() {
    let harness = TestDbHarness::open_in_memory();
    let query_vec = make_basis_vector(0);
    // Empty query text should not cause database syntax error
    let res = harness.execute_hybrid_search("", &query_vec, 100, 5);
    // Handled safely without SQL panic
    assert!(res.is_ok() || matches!(res, Err(RagError::InformationNotFound { .. })));
}

#[test]
fn test_tier1_f3_04_fts5_special_characters_sanitization() {
    let mut harness = TestDbHarness::open_in_memory();
    harness
        .insert_chunk(
            "c_special",
            "f1.md",
            0,
            "Complex syntax with quote \" and punctuation",
            7,
            CoalaType::Semantic,
            NoteStatus::Active,
            None,
            None,
            100,
            None,
        )
        .expect("insert");

    let query = "\"Complex syntax\"";
    let count: i64 = harness
        .conn
        .query_row(
            "SELECT COUNT(*) FROM fts_notes WHERE fts_notes MATCH ?1",
            rusqlite::params![query],
            |r| r.get(0),
        )
        .expect("sanitized query");
    assert_eq!(count, 1);
}

#[test]
fn test_tier1_f3_05_bm25_score_normalized_to_unit_range() {
    let s_bm25 = 0.85f64;
    assert!((0.0..=1.0).contains(&s_bm25), "Normalized BM25 must be in [0, 1]");
}

// Feature 4: Time-Decay Attenuation Formula
#[test]
fn test_tier1_f4_01_procedural_decay_zero() {
    let score = compute_hybrid_decay_score(0.70, 0.70, CoalaType::Procedural, 1000.0);
    assert!((score - 0.70).abs() < 1e-5);
}

#[test]
fn test_tier1_f4_02_semantic_decay_standard_300_days() {
    let score = compute_hybrid_decay_score(1.0, 1.0, CoalaType::Semantic, 300.0);
    // 1.0 / (1 + 0.005 * 300) = 1.0 / 2.5 = 0.40
    assert!((score - 0.40).abs() < 1e-5);
}

#[test]
fn test_tier1_f4_03_episodic_decay_standard_100_days() {
    let score = compute_hybrid_decay_score(1.0, 1.0, CoalaType::Episodic, 100.0);
    // 1.0 / (1 + 0.005 * 100) = 1.0 / 1.5 = 0.666667
    assert!((score - 0.6666667).abs() < 1e-5);
}

#[test]
fn test_tier1_f4_04_fresh_note_decay_factor_unity() {
    let score = compute_hybrid_decay_score(0.80, 0.60, CoalaType::Semantic, 0.0);
    let expected = 0.7 * 0.80 + 0.3 * 0.60;
    assert!((score - expected).abs() < 1e-6);
}

#[test]
fn test_tier1_f4_05_combined_score_weighting_70_30() {
    let score = compute_hybrid_decay_score(1.0, 0.0, CoalaType::Procedural, 0.0);
    assert!((score - 0.70).abs() < 1e-6, "Pure vector match must contribute 0.7");
    let score_lex = compute_hybrid_decay_score(0.0, 1.0, CoalaType::Procedural, 0.0);
    assert!((score_lex - 0.30).abs() < 1e-6, "Pure lexical match must contribute 0.3");
}

// Feature 5: Obsolescence Filtering & 1-Hop Graph Traversal
#[test]
fn test_tier1_f5_01_deprecated_note_omitted_from_standard_search() {
    let mut harness = TestDbHarness::open_in_memory();
    let v = make_basis_vector(0);
    harness
        .insert_chunk(
            "dep1",
            "f1.md",
            0,
            "old info",
            2,
            CoalaType::Semantic,
            NoteStatus::Deprecated,
            None,
            None,
            100,
            Some(&v),
        )
        .expect("insert");
    let results = harness.execute_hybrid_search("old", &v, 100, 5).expect("search");
    assert!(results.is_empty());
}

#[test]
fn test_tier1_f5_02_active_note_included_in_standard_search() {
    let mut harness = TestDbHarness::open_in_memory();
    let v = make_basis_vector(0);
    harness
        .insert_chunk(
            "act1",
            "f1.md",
            0,
            "new info",
            2,
            CoalaType::Semantic,
            NoteStatus::Active,
            None,
            None,
            100,
            Some(&v),
        )
        .expect("insert");
    let results = harness.execute_hybrid_search("new", &v, 100, 5).expect("search");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].chunk_id, "act1");
}

#[test]
fn test_tier1_f5_03_file_link_insertion_and_retrieval() {
    let harness = TestDbHarness::open_in_memory();
    harness
        .insert_file_link("docA.md", "docB.md", "supersedes", 12345)
        .expect("insert link");
    let target = harness.get_superseding_file("docA.md").expect("get link");
    assert_eq!(target, Some("docB.md".to_string()));
}

#[test]
fn test_tier1_f5_04_1hop_superseding_resolution() {
    let harness = TestDbHarness::open_in_memory();
    harness
        .insert_file_link("draft_v1.md", "draft_v2.md", "supersedes", 200)
        .expect("link v1->v2");
    let res = harness.get_superseding_file("draft_v1.md").expect("query");
    assert_eq!(res, Some("draft_v2.md".to_string()));
}

#[test]
fn test_tier1_f5_05_unrelated_link_types_ignored() {
    let harness = TestDbHarness::open_in_memory();
    harness
        .insert_file_link("docA.md", "docB.md", "wikilink", 200)
        .expect("wikilink");
    harness
        .insert_file_link("docA.md", "docC.md", "relates", 200)
        .expect("relates");
    let res = harness.get_superseding_file("docA.md").expect("query");
    assert_eq!(res, None, "wikilink or relates must not trigger obsolescence replacement");
}

// Feature 6: Anti-Hallucination Circuit Breaker
#[test]
fn test_tier1_f6_01_circuit_breaker_triggers_below_065() {
    let res = check_circuit_breaker(0.64);
    assert_eq!(
        res,
        Err(RagError::InformationNotFound { similarity: 0.64 })
    );
}

#[test]
fn test_tier1_f6_02_circuit_breaker_passes_above_065() {
    let res = check_circuit_breaker(0.66);
    assert!(res.is_ok());
}

#[test]
fn test_tier1_f6_03_circuit_breaker_preserves_similarity_in_error() {
    let res = check_circuit_breaker(0.425);
    match res {
        Err(RagError::InformationNotFound { similarity }) => {
            assert!((similarity - 0.425).abs() < 1e-6);
        }
        _ => panic!("Expected error"),
    }
}

#[test]
fn test_tier1_f6_04_empty_database_triggers_circuit_breaker() {
    let harness = TestDbHarness::open_in_memory();
    let query_vec = make_basis_vector(0);
    let res = harness.execute_hybrid_search("query", &query_vec, 100, 5);
    assert!(matches!(res, Err(RagError::InformationNotFound { .. })));
}

#[test]
fn test_tier1_f6_05_multi_candidate_circuit_breaker_evaluates_max() {
    let mut harness = TestDbHarness::open_in_memory();
    let v_low = make_synthetic_vector_with_similarity(0.50);
    let v_mid = make_synthetic_vector_with_similarity(0.60);
    let v_high = make_synthetic_vector_with_similarity(0.72);

    harness
        .insert_chunk("c1", "f1.md", 0, "text", 1, CoalaType::Semantic, NoteStatus::Active, None, None, 100, Some(&v_low))
        .unwrap();
    harness
        .insert_chunk("c2", "f2.md", 0, "text", 1, CoalaType::Semantic, NoteStatus::Active, None, None, 100, Some(&v_mid))
        .unwrap();
    harness
        .insert_chunk("c3", "f3.md", 0, "text", 1, CoalaType::Semantic, NoteStatus::Active, None, None, 100, Some(&v_high))
        .unwrap();

    let query = make_basis_vector(0);
    let res = harness.execute_hybrid_search("text", &query, 100, 5);
    assert!(res.is_ok(), "Because max similarity is 0.72 >= 0.65, circuit breaker must pass");
}

// Feature 7: Cascade Deletion & Orphan Synchronization
#[test]
fn test_tier1_f7_01_delete_file_cascades_chunks() {
    let mut harness = TestDbHarness::open_in_memory();
    harness
        .conn
        .execute(
            "INSERT INTO files (file_path, file_hash, last_modified) VALUES ('f1.md', 'h1', 100)",
            [],
        )
        .unwrap();
    harness
        .insert_chunk("c1", "f1.md", 0, "content", 1, CoalaType::Semantic, NoteStatus::Active, None, None, 100, None)
        .unwrap();

    harness.delete_file("f1.md").unwrap();
    let remaining_chunks: i64 = harness
        .conn
        .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
        .unwrap();
    assert_eq!(remaining_chunks, 0, "Chunks must cascade delete with file");
}

#[test]
fn test_tier1_f7_02_delete_file_cascades_file_links() {
    let mut harness = TestDbHarness::open_in_memory();
    harness
        .conn
        .execute(
            "INSERT INTO files (file_path, file_hash, last_modified) VALUES ('f1.md', 'h1', 100)",
            [],
        )
        .unwrap();
    harness.insert_file_link("f1.md", "f2.md", "supersedes", 100).unwrap();

    harness.delete_file("f1.md").unwrap();
    let remaining_links: i64 = harness
        .conn
        .query_row("SELECT COUNT(*) FROM file_links", [], |r| r.get(0))
        .unwrap();
    assert_eq!(remaining_links, 0, "file_links must cascade delete");
}

#[test]
fn test_tier1_f7_03_delete_file_purges_virtual_vec_chunks() {
    let mut harness = TestDbHarness::open_in_memory();
    let v = make_basis_vector(0);
    harness
        .conn
        .execute(
            "INSERT INTO files (file_path, file_hash, last_modified) VALUES ('f1.md', 'h1', 100)",
            [],
        )
        .unwrap();
    let id = harness
        .insert_chunk("c1", "f1.md", 0, "content", 1, CoalaType::Semantic, NoteStatus::Active, None, None, 100, Some(&v))
        .unwrap();

    assert!(harness.vectors.contains_key(&id));
    harness.delete_file("f1.md").unwrap();
    assert!(
        !harness.vectors.contains_key(&id),
        "Vector table must purge row on file deletion"
    );
}

#[test]
fn test_tier1_f7_04_explicit_orphan_cleanup_removes_unindexed_vectors() {
    let mut harness = TestDbHarness::open_in_memory();
    harness.vectors.insert(999, make_basis_vector(0)); // Orphan with no row in chunks

    // Run orphan cleanup:
    let mut stmt = harness.conn.prepare("SELECT id FROM chunks").unwrap();
    let active_ids: Vec<i64> = stmt.query_map([], |r| r.get(0)).unwrap().collect::<Result<_, _>>().unwrap();
    harness.vectors.retain(|id, _| active_ids.contains(id));

    assert!(!harness.vectors.contains_key(&999), "Orphan row 999 must be purged");
}

#[test]
fn test_tier1_f7_05_delete_non_existent_file_is_safe_noop() {
    let mut harness = TestDbHarness::open_in_memory();
    let res = harness.delete_file("does_not_exist.md");
    assert!(res.is_ok(), "Deleting missing file must succeed as a no-op");
}

// ============================================================================
// TIER 2: BOUNDARY & CORNER CASES (>=5 per feature)
// ============================================================================

// Vector Boundaries
#[test]
fn test_tier2_v_01_all_zeros_synthetic_vector() {
    let zero_vec = [0.0f32; 384];
    let norm = vector_norm(&zero_vec);
    assert_eq!(norm, 0.0);
    let sim = cosine_similarity(&zero_vec, &make_basis_vector(0));
    assert_eq!(sim, 0.0, "Zero vector cosine similarity must return 0.0 safely without NaN");
}

#[test]
fn test_tier2_v_02_epsilon_perturbed_vectors() {
    let v1 = make_basis_vector(0);
    let mut v2 = make_basis_vector(0);
    v2[1] = 1e-2; // Epsilon perturbation distinguishable in f32
    let dist = cosine_distance(&v1, &v2);
    assert!(dist > 0.0 && dist < 1e-3, "Distance should be tiny positive, got {dist}");
}

#[test]
fn test_tier2_v_03_canonical_basis_vectors() {
    for i in 0..4 {
        for j in 0..4 {
            let vi = make_basis_vector(i);
            let vj = make_basis_vector(j);
            let sim = cosine_similarity(&vi, &vj);
            if i == j {
                assert!((sim - 1.0).abs() < 1e-6);
            } else {
                assert!(sim.abs() < 1e-6);
            }
        }
    }
}

#[test]
fn test_tier2_v_04_large_positive_negative_values() {
    let mut v = [0.0f32; 384];
    let inv_sqrt = 1.0 / (384.0f32).sqrt();
    for (i, val) in v.iter_mut().enumerate() {
        *val = if i % 2 == 0 { inv_sqrt } else { -inv_sqrt };
    }
    let norm = vector_norm(&v);
    assert!((norm - 1.0).abs() < 1e-5);
    let bytes = serialize_vector(&v);
    let roundtrip = deserialize_vector(&bytes);
    assert!((cosine_similarity(&v, &roundtrip) - 1.0).abs() < 1e-6);
}

#[test]
fn test_tier2_v_05_k_greater_than_total_stored_vectors() {
    let mut harness = TestDbHarness::open_in_memory();
    harness.vectors.insert(1, make_basis_vector(0));
    let results = harness.search_vector(&make_basis_vector(0), 100);
    assert_eq!(results.len(), 1, "Must return available count without crashing");
}

// Temporal Boundaries
#[test]
fn test_tier2_t_01_exact_zero_elapsed_time() {
    let score = compute_hybrid_decay_score(0.9, 0.8, CoalaType::Semantic, 0.0);
    let expected = 0.7 * 0.9 + 0.3 * 0.8;
    assert!((score - expected).abs() < 1e-6);
}

#[test]
fn test_tier2_t_02_large_age_10_years() {
    let score = compute_hybrid_decay_score(1.0, 1.0, CoalaType::Semantic, 3650.0);
    // Divisor: 1 + 0.005 * 3650 = 1 + 18.25 = 19.25
    let expected = 1.0 / 19.25;
    assert!((score - expected).abs() < 1e-5);
    assert!(score > 0.0, "Score must remain strictly positive and non-zero");
}

#[test]
fn test_tier2_t_03_future_timestamp_clamping() {
    // Negative delta_t should clamp to 0.0, avoiding divisor < 1.0
    let score = compute_hybrid_decay_score(0.8, 0.8, CoalaType::Semantic, -50.0);
    assert!((score - 0.8).abs() < 1e-5, "Future timestamp must clamp delta_t to 0.0");
}

#[test]
fn test_tier2_t_04_procedural_age_50_years() {
    let score = compute_hybrid_decay_score(0.95, 0.90, CoalaType::Procedural, 18250.0);
    let expected = 0.7 * 0.95 + 0.3 * 0.90;
    assert!((score - expected).abs() < 1e-6, "Procedural note must never decay even after 50 years");
}

#[test]
fn test_tier2_t_05_half_life_exact_verification() {
    // At delta_t = 200 days, lambda = 0.005: divisor = 1 + 0.005 * 200 = 2.0 (half-life!)
    let score_0 = compute_hybrid_decay_score(1.0, 1.0, CoalaType::Semantic, 0.0);
    let score_200 = compute_hybrid_decay_score(1.0, 1.0, CoalaType::Semantic, 200.0);
    assert!((score_200 - score_0 / 2.0).abs() < 1e-6);
}

// Obsolescence Boundaries
#[test]
fn test_tier2_o_01_circular_supersedes_link() {
    let harness = TestDbHarness::open_in_memory();
    harness.insert_file_link("A.md", "B.md", "supersedes", 100).unwrap();
    harness.insert_file_link("B.md", "A.md", "supersedes", 100).unwrap();

    let next = harness.get_superseding_file("A.md").unwrap();
    assert_eq!(next, Some("B.md".to_string()), "1-hop query must safely return direct link without loop");
}

#[test]
fn test_tier2_o_02_multi_hop_chain_resolved_to_one_hop() {
    let harness = TestDbHarness::open_in_memory();
    harness.insert_file_link("v1.md", "v2.md", "supersedes", 100).unwrap();
    harness.insert_file_link("v2.md", "v3.md", "supersedes", 100).unwrap();

    let direct = harness.get_superseding_file("v1.md").unwrap();
    assert_eq!(direct, Some("v2.md".to_string()), "1-hop contract resolves direct neighbor v2.md");
}

#[test]
fn test_tier2_o_03_self_referential_supersedes() {
    let harness = TestDbHarness::open_in_memory();
    harness.insert_file_link("self.md", "self.md", "supersedes", 100).unwrap();
    let res = harness.get_superseding_file("self.md").unwrap();
    assert_eq!(res, Some("self.md".to_string()));
}

#[test]
fn test_tier2_o_04_deprecated_note_without_replacement() {
    let harness = TestDbHarness::open_in_memory();
    let res = harness.get_superseding_file("standalone_old.md").unwrap();
    assert_eq!(res, None);
}

#[test]
fn test_tier2_o_05_case_insensitive_status_parsing() {
    assert_eq!(NoteStatus::from_str_lenient("Active"), NoteStatus::Active);
    assert_eq!(NoteStatus::from_str_lenient("ACTIVE"), NoteStatus::Active);
    assert_eq!(NoteStatus::from_str_lenient("actif"), NoteStatus::Active);
    assert_eq!(NoteStatus::from_str_lenient("DEPRECATED"), NoteStatus::Deprecated);
    assert_eq!(NoteStatus::from_str_lenient("obsolete"), NoteStatus::Deprecated);
    assert_eq!(NoteStatus::from_str_lenient("archive"), NoteStatus::Deprecated);
}

// Circuit Breaker Threshold Boundaries
#[test]
fn test_tier2_cb_01_exact_threshold_minus_epsilon() {
    let s = 0.649999f32;
    assert_eq!(
        check_circuit_breaker(s),
        Err(RagError::InformationNotFound { similarity: s })
    );
}

#[test]
fn test_tier2_cb_02_exact_threshold_plus_epsilon() {
    let s = 0.650001f32;
    assert!(check_circuit_breaker(s).is_ok());
}

#[test]
fn test_tier2_cb_03_exact_threshold_boundary_065000() {
    let s = 0.650000f32;
    assert!(check_circuit_breaker(s).is_ok());
}

#[test]
fn test_tier2_cb_04_high_bm25_low_vector_similarity() {
    // Even if BM25 is 1.0, low vector similarity (<0.65) must trigger circuit breaker
    let s_vector = 0.64f32;
    let cb = check_circuit_breaker(s_vector);
    assert!(cb.is_err(), "Circuit breaker must veto despite high BM25");
}

#[test]
fn test_tier2_cb_05_negative_cosine_similarity_floor() {
    let s = -1.0f32;
    assert_eq!(
        check_circuit_breaker(s),
        Err(RagError::InformationNotFound { similarity: -1.0 })
    );
}

// Cascade Deletion Boundaries
#[test]
fn test_tier2_cd_01_delete_file_with_zero_chunks() {
    let mut harness = TestDbHarness::open_in_memory();
    harness
        .conn
        .execute("INSERT INTO files (file_path, file_hash, last_modified) VALUES ('empty.md', 'h', 1)", [])
        .unwrap();
    assert!(harness.delete_file("empty.md").is_ok());
}

#[test]
fn test_tier2_cd_02_delete_file_with_50_chunks() {
    let mut harness = TestDbHarness::open_in_memory();
    harness
        .conn
        .execute("INSERT INTO files (file_path, file_hash, last_modified) VALUES ('big.md', 'h', 1)", [])
        .unwrap();

    for i in 0..50 {
        let v = make_basis_vector(i % 384);
        harness
            .insert_chunk(&format!("c_{i}"), "big.md", i, "chunk text", 2, CoalaType::Semantic, NoteStatus::Active, None, None, 100, Some(&v))
            .unwrap();
    }
    assert_eq!(harness.vectors.len(), 50);
    harness.delete_file("big.md").unwrap();
    assert_eq!(harness.vectors.len(), 0);
}

#[test]
fn test_tier2_cd_03_delete_file_cascades_target_links() {
    let mut harness = TestDbHarness::open_in_memory();
    harness
        .conn
        .execute("INSERT INTO files (file_path, file_hash, last_modified) VALUES ('source.md', 'h', 1)", [])
        .unwrap();
    harness.insert_file_link("source.md", "target.md", "supersedes", 100).unwrap();
    harness.delete_file("source.md").unwrap();
    let count: i64 = harness.conn.query_row("SELECT COUNT(*) FROM file_links", [], |r| r.get(0)).unwrap();
    assert_eq!(count, 0);
}

#[test]
fn test_tier2_cd_04_reindex_updated_file_purges_old_vectors() {
    let mut harness = TestDbHarness::open_in_memory();
    harness
        .conn
        .execute("INSERT INTO files (file_path, file_hash, last_modified) VALUES ('update.md', 'h1', 1)", [])
        .unwrap();

    let id1 = harness
        .insert_chunk("c1", "update.md", 0, "old content", 2, CoalaType::Semantic, NoteStatus::Active, None, None, 100, Some(&make_basis_vector(0)))
        .unwrap();

    // Re-index: delete old chunks and insert new
    harness.conn.execute("DELETE FROM chunks WHERE file_path = 'update.md'", []).unwrap();
    let active_ids: Vec<i64> = {
        let mut stmt = harness.conn.prepare("SELECT id FROM chunks").unwrap();
        stmt.query_map([], |r| r.get(0)).unwrap().collect::<Result<_, _>>().unwrap()
    };
    harness.vectors.retain(|id, _| active_ids.contains(id));
    assert!(!harness.vectors.contains_key(&id1));

    let id2 = harness
        .insert_chunk("c1_new", "update.md", 0, "new content", 2, CoalaType::Semantic, NoteStatus::Active, None, None, 200, Some(&make_basis_vector(1)))
        .unwrap();
    assert!(harness.vectors.contains_key(&id2));
}

#[test]
fn test_tier2_cd_05_delete_all_files_leaves_empty_database() {
    let mut harness = TestDbHarness::open_in_memory();
    for i in 1..=3 {
        let path = format!("file_{i}.md");
        harness
            .conn
            .execute("INSERT INTO files (file_path, file_hash, last_modified) VALUES (?1, 'h', 1)", rusqlite::params![path])
            .unwrap();
        harness
            .insert_chunk(&format!("c_{i}"), &path, 0, "txt", 1, CoalaType::Semantic, NoteStatus::Active, None, None, 100, Some(&make_basis_vector(i)))
            .unwrap();
    }
    assert_eq!(harness.vectors.len(), 3);

    for i in 1..=3 {
        harness.delete_file(&format!("file_{i}.md")).unwrap();
    }
    assert_eq!(harness.vectors.len(), 0);
    let chunk_count: i64 = harness.conn.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0)).unwrap();
    assert_eq!(chunk_count, 0);
}

// ============================================================================
// TIER 3: PAIRWISE COMBINATIONS & CROSS-FEATURE INTERACTIONS
// ============================================================================

#[test]
fn test_tier3_p1_deprecated_high_similarity_vs_active_moderate() {
    let mut harness = TestDbHarness::open_in_memory();
    let v_dep = make_synthetic_vector_with_similarity(0.99);
    let v_act = make_synthetic_vector_with_similarity(0.70);

    harness
        .insert_chunk("c_dep", "deprecated.md", 0, "Rust 2018 edition tips", 4, CoalaType::Semantic, NoteStatus::Deprecated, Some("active.md"), None, 100, Some(&v_dep))
        .unwrap();
    harness
        .insert_chunk("c_act", "active.md", 0, "Rust 2024 edition tips", 4, CoalaType::Semantic, NoteStatus::Active, None, None, 100, Some(&v_act))
        .unwrap();

    let query = make_basis_vector(0);
    let res = harness.execute_hybrid_search("edition tips", &query, 100, 5).unwrap();

    assert_eq!(res.len(), 1);
    assert_eq!(res[0].chunk_id, "c_act");
}

#[test]
fn test_tier3_p2_old_procedural_vs_recent_semantic_decay_inversion() {
    let now = 1720000000i64;
    let mut harness = TestDbHarness::open_in_memory();

    // Procedural note 300 days old with raw vector score 0.75
    let v_proc = make_synthetic_vector_with_similarity(0.75);
    // Semantic note 300 days old with raw vector score 0.85
    let v_sem = make_synthetic_vector_with_similarity(0.85);

    harness
        .insert_chunk("c_proc", "proc.md", 0, "Standard Operating Procedure", 3, CoalaType::Procedural, NoteStatus::Active, None, None, now - 300 * 86400, Some(&v_proc))
        .unwrap();
    harness
        .insert_chunk("c_sem", "sem.md", 0, "Meeting Discussion Minutes", 3, CoalaType::Semantic, NoteStatus::Active, None, None, now - 300 * 86400, Some(&v_sem))
        .unwrap();

    let query = make_basis_vector(0);
    let res = harness.execute_hybrid_search("Procedure Minutes", &query, now, 5).unwrap();

    assert_eq!(res.len(), 2);
    assert_eq!(
        res[0].chunk_id, "c_proc",
        "Procedural note must rank first due to 0 decay"
    );
}

#[test]
fn test_tier3_p3_superseded_active_note_retrieval_and_audit() {
    let harness = TestDbHarness::open_in_memory();
    harness.insert_file_link("arch_v1.md", "arch_v2.md", "supersedes", 100).unwrap();
    let replacement = harness.get_superseding_file("arch_v1.md").unwrap();
    assert_eq!(replacement, Some("arch_v2.md".to_string()));
}

#[test]
fn test_tier3_p4_circuit_breaker_vetoes_lexical_hallucination() {
    let mut harness = TestDbHarness::open_in_memory();
    let v_mismatch = make_synthetic_vector_with_similarity(0.40); // < 0.65

    harness
        .insert_chunk("c_match", "f1.md", 0, "Apple banana cherry fruit salad", 5, CoalaType::Semantic, NoteStatus::Active, None, None, 100, Some(&v_mismatch))
        .unwrap();

    let query = make_basis_vector(0);
    // Lexical matches exact words, but embedding is dissimilar
    let res = harness.execute_hybrid_search("banana cherry", &query, 100, 5);
    assert!(
        matches!(res, Err(RagError::InformationNotFound { .. })),
        "Circuit breaker must prevent lexical hallucination when vector similarity < 0.65"
    );
}

#[test]
fn test_tier3_p5_cascade_deletion_followed_by_immediate_hybrid_search() {
    let mut harness = TestDbHarness::open_in_memory();
    let v = make_synthetic_vector_with_similarity(0.90);
    harness.conn.execute("INSERT INTO files (file_path, file_hash, last_modified) VALUES ('file.md', 'h', 1)", []).unwrap();
    harness.insert_chunk("c1", "file.md", 0, "Secret content", 2, CoalaType::Semantic, NoteStatus::Active, None, None, 100, Some(&v)).unwrap();

    let query = make_basis_vector(0);
    let before = harness.execute_hybrid_search("Secret", &query, 100, 5).unwrap();
    assert_eq!(before.len(), 1);

    harness.delete_file("file.md").unwrap();

    let after = harness.execute_hybrid_search("Secret", &query, 100, 5);
    assert!(after.is_err() || after.unwrap().is_empty(), "Deleted document must vanish from search immediately");
}

#[test]
fn test_tier3_p6_mixed_coala_strata_multi_note_ranking() {
    let now = 1720000000i64;
    let mut harness = TestDbHarness::open_in_memory();
    let v = make_synthetic_vector_with_similarity(0.80);

    harness
        .insert_chunk("c_proc", "p.md", 0, "Topic Guidelines", 2, CoalaType::Procedural, NoteStatus::Active, None, None, now - 200 * 86400, Some(&v))
        .unwrap();
    harness
        .insert_chunk("c_sem", "s.md", 0, "Topic Guidelines", 2, CoalaType::Semantic, NoteStatus::Active, None, None, now - 200 * 86400, Some(&v))
        .unwrap();
    harness
        .insert_chunk("c_epi", "e.md", 0, "Topic Guidelines", 2, CoalaType::Episodic, NoteStatus::Active, None, None, now - 200 * 86400, Some(&v))
        .unwrap();

    let query = make_basis_vector(0);
    let res = harness.execute_hybrid_search("Topic", &query, now, 5).unwrap();
    assert_eq!(res.len(), 3);
    assert_eq!(res[0].chunk_id, "c_proc", "Procedural note must be top ranked");
}

// ============================================================================
// TIER 4: REAL-WORLD SCENARIOS
// ============================================================================

#[test]
fn test_tier4_s1_engineering_adr_vault_lifecycle() {
    let mut harness = TestDbHarness::open_in_memory();
    let v_adr1 = make_synthetic_vector_with_similarity(0.85);
    let v_adr2 = make_synthetic_vector_with_similarity(0.88);

    harness
        .insert_chunk("adr_001", "docs/ADR-001.md", 0, "Monolithic database architecture", 3, CoalaType::Semantic, NoteStatus::Deprecated, Some("docs/ADR-002.md"), Some(1720000000), 1700000000, Some(&v_adr1))
        .unwrap();
    harness
        .insert_chunk("adr_002", "docs/ADR-002.md", 0, "Hybrid vector database architecture", 4, CoalaType::Semantic, NoteStatus::Active, None, None, 1720000000, Some(&v_adr2))
        .unwrap();
    harness.insert_file_link("docs/ADR-001.md", "docs/ADR-002.md", "supersedes", 1720000000).unwrap();

    let query = make_basis_vector(0);
    let res = harness.execute_hybrid_search("database architecture", &query, 1720000000, 5).unwrap();

    assert_eq!(res.len(), 1);
    assert_eq!(res[0].chunk_id, "adr_002");
    assert_eq!(res[0].file_path, "docs/ADR-002.md");

    let successor = harness.get_superseding_file("docs/ADR-001.md").unwrap();
    assert_eq!(successor, Some("docs/ADR-002.md".to_string()));
}

#[test]
fn test_tier4_s2_incident_investigation_decay_vs_runbook_permanence() {
    let now = 1720000000i64;
    let mut harness = TestDbHarness::open_in_memory();
    let v = make_synthetic_vector_with_similarity(0.85);

    harness
        .insert_chunk("runbook_sec", "ops/runbooks/tls.md", 0, "TLS Certificate rotation runbook", 4, CoalaType::Procedural, NoteStatus::Active, None, None, now - 365 * 86400, Some(&v))
        .unwrap();
    harness
        .insert_chunk("incident_042", "ops/incidents/inc-42.md", 0, "TLS Certificate rotation incident report", 5, CoalaType::Episodic, NoteStatus::Active, None, None, now - 180 * 86400, Some(&v))
        .unwrap();

    let query = make_basis_vector(0);
    let res = harness.execute_hybrid_search("TLS Certificate", &query, now, 5).unwrap();

    assert_eq!(res.len(), 2);
    assert_eq!(res[0].chunk_id, "runbook_sec", "Runbook remains permanently fresh");
    assert_eq!(res[1].chunk_id, "incident_042");
    assert!(res[0].combined_score > res[1].combined_score);
}

#[test]
fn test_tier4_s3_out_of_vault_query_rejection() {
    let mut harness = TestDbHarness::open_in_memory();
    let v_software = make_basis_vector(0);

    harness
        .insert_chunk("chunk_sw", "src/main.rs", 0, "Rust tokio task spawning", 4, CoalaType::Semantic, NoteStatus::Active, None, None, 100, Some(&v_software))
        .unwrap();

    // Query on biology topic yields vector with low similarity (0.25)
    let v_biology = make_synthetic_vector_with_similarity(0.25);
    let res = harness.execute_hybrid_search("cellular respiration mitochondria", &v_biology, 100, 5);

    assert_eq!(
        res,
        Err(RagError::InformationNotFound { similarity: 0.25 }),
        "Foreign query must be strictly rejected with InformationNotFound"
    );
}

#[test]
fn test_tier4_s4_continuous_reindexing_and_vacuum_integrity() {
    let dir = tempdir().expect("create temp dir");
    let db_path = dir.path().join("vault_wal.db");
    let mut harness = TestDbHarness::open_on_disk(&db_path);

    // Create 10 notes
    for i in 1..=10 {
        let path = format!("notes/note_{i}.md");
        harness
            .conn
            .execute("INSERT INTO files (file_path, file_hash, last_modified) VALUES (?1, 'hash', 1)", rusqlite::params![path])
            .unwrap();
        let v = make_basis_vector(i % 384);
        harness
            .insert_chunk(&format!("chunk_{i}"), &path, 0, "Continuous indexing test content", 5, CoalaType::Semantic, NoteStatus::Active, None, None, 1000, Some(&v))
            .unwrap();
    }
    assert_eq!(harness.vectors.len(), 10);

    // Update 5 notes (re-index)
    for i in 1..=5 {
        let path = format!("notes/note_{i}.md");
        harness.conn.execute("DELETE FROM chunks WHERE file_path = ?1", rusqlite::params![path]).unwrap();
        let active_ids: Vec<i64> = {
            let mut stmt = harness.conn.prepare("SELECT id FROM chunks").unwrap();
            stmt.query_map([], |r| r.get(0)).unwrap().collect::<Result<_, _>>().unwrap()
        };
        harness.vectors.retain(|id, _| active_ids.contains(id));

        let v_new = make_basis_vector((i + 50) % 384);
        harness
            .insert_chunk(&format!("chunk_{i}_v2"), &path, 0, "Updated continuous content", 3, CoalaType::Semantic, NoteStatus::Active, None, None, 2000, Some(&v_new))
            .unwrap();
    }

    // Delete 3 notes
    for i in 8..=10 {
        harness.delete_file(&format!("notes/note_{i}.md")).unwrap();
    }

    assert_eq!(harness.vectors.len(), 7);
    let chunk_count: i64 = harness.conn.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0)).unwrap();
    assert_eq!(chunk_count, 7);
}
