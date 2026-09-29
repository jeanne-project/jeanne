//! Moteur RAG hybride couplant recherche vectorielle dense (sqlite-vec),
//! recherche lexicale BM25 (fts5), formule d'atténuation temporelle (Time-Decay),
//! filtrage d'obsolescence sémantique (graphe 1-hop) et coupe-circuit anti-hallucination.

use crate::error::{JeanneError, RagError};
use crate::models::{CoalaType, HybridSearchResult, NoteStatus};
use crate::storage::StorageManager;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Seuil minimal de similarité cosinus requis pour franchir le coupe-circuit anti-hallucination.
pub const SIMILARITY_THRESHOLD: f32 = 0.65;

/// Vérifie le coupe-circuit anti-hallucination selon la spécification §3.2.
/// Si `max_similarity < 0.65` (ou si la valeur est NaN), retourne immédiatement
/// `Err(RagError::InformationNotFound { similarity })`.
pub fn check_circuit_breaker(max_similarity: f32) -> Result<(), RagError> {
    if max_similarity.is_nan() || max_similarity < SIMILARITY_THRESHOLD {
        Err(RagError::InformationNotFound {
            similarity: if max_similarity.is_nan() {
                0.0
            } else {
                max_similarity
            },
        })
    } else {
        Ok(())
    }
}

/// Calcule le score hybride pondéré avec atténuation temporelle Time-Decay (SPEC §2.3) :
/// Score = (0.7 * S_vector + 0.3 * S_BM25) / (1 + lambda * delta_t_jours)
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

/// Moteur de recherche RAG hybride et temporel adossé au coffre de notes Jeanne.
pub struct RagEngine {
    storage: Arc<Mutex<StorageManager>>,
}

impl RagEngine {
    /// Crée une nouvelle instance du moteur RAG adossée au gestionnaire de stockage SQLite partagé.
    pub fn new(storage: Arc<Mutex<StorageManager>>) -> Self {
        Self { storage }
    }

    /// Exécute une recherche hybride standard à l'instant présent (`chrono::Utc::now()`).
    pub fn search(
        &self,
        query: &str,
        query_embedding: &[f32; 384],
        limit: usize,
    ) -> Result<Vec<HybridSearchResult>, RagError> {
        let now = chrono::Utc::now().timestamp();
        self.search_at(query, query_embedding, limit, now)
    }

    /// Exécute une recherche hybride standard avec exclusion des notes dépréciées à un timestamp Unix déterministe.
    pub fn search_at(
        &self,
        query: &str,
        query_embedding: &[f32; 384],
        limit: usize,
        now: i64,
    ) -> Result<Vec<HybridSearchResult>, RagError> {
        self.search_with_options(query, query_embedding, limit, now, false)
    }

    /// Exécute une recherche hybride avec option d'audit historique (`include_deprecated: true`).
    pub fn search_with_options(
        &self,
        query: &str,
        query_embedding: &[f32; 384],
        limit: usize,
        now: i64,
        include_deprecated: bool,
    ) -> Result<Vec<HybridSearchResult>, RagError> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let storage_guard = self.storage.lock().map_err(|e| {
            RagError::Storage(Box::new(JeanneError::Vault(format!(
                "Storage lock poisoned: {e}"
            ))))
        })?;

        // 1. Recherche vectorielle dense sur `vec_chunks`
        let candidate_k = (limit * 3).max(20);
        let vec_results = storage_guard.search_vector(query_embedding, candidate_k)?;

        // 2. Coupe-circuit anti-hallucination immédiat
        // Si aucun vecteur n'est indexé dans vec_chunks, similarité 0.0
        let max_similarity = if vec_results.is_empty() {
            0.0f32
        } else {
            vec_results
                .iter()
                .map(|(_, dist)| 1.0f32 - *dist)
                .fold(f32::NEG_INFINITY, f32::max)
        };

        check_circuit_breaker(max_similarity)?;

        // 3. Recherche lexicale BM25 sur FTS5 (sur notes actives)
        let trimmed_query = query.trim();
        let mut bm25_scores: HashMap<String, f64> = HashMap::new();

        if !trimmed_query.is_empty() {
            let conn = storage_guard.raw_connection();
            let sanitized = sanitize_fts_query(trimmed_query);
            if !sanitized.is_empty() {
                let fts_limit = (limit * 3).max(50) as i64;
                let status_clause = if include_deprecated {
                    ""
                } else {
                    "AND c.status = 'active'"
                };
                let fts_sql = format!(
                    r#"
                    SELECT f.chunk_id, -bm25(fts_notes) AS raw_score
                    FROM fts_notes f
                    JOIN chunks c ON c.chunk_id = f.chunk_id
                    WHERE fts_notes MATCH ?1 {status_clause}
                    ORDER BY raw_score DESC
                    LIMIT ?2;
                    "#
                );

                let mut raw_scores: Vec<(String, f64)> = Vec::new();
                let execute_query = |q: &str| -> rusqlite::Result<Vec<(String, f64)>> {
                    let mut stmt = conn.prepare(&fts_sql)?;
                    let rows = stmt.query_map(rusqlite::params![q, fts_limit], |r| {
                        let cid: String = r.get(0)?;
                        let raw: f64 = r.get(1)?;
                        Ok((cid, raw))
                    })?;
                    let mut list = Vec::new();
                    for item in rows {
                        list.push(item?);
                    }
                    Ok(list)
                };

                match execute_query(&sanitized) {
                    Ok(list) => {
                        raw_scores = list;
                    }
                    Err(_) => {
                        // Repli sécurisé en découpant les mots en tokens FTS5 valides
                        let fallback = sanitized
                            .split_whitespace()
                            .map(|w| format!("\"{}\"*", w.replace('"', "")))
                            .collect::<Vec<_>>()
                            .join(" ");
                        if !fallback.is_empty() {
                            if let Ok(list) = execute_query(&fallback) {
                                raw_scores = list;
                            }
                        }
                    }
                }

                // Normalisation par max-scaling sur l'échantillon candidat
                let max_raw = raw_scores.iter().fold(0.0f64, |acc, (_, s)| acc.max(*s));
                if max_raw > 0.0 {
                    for (cid, raw) in raw_scores {
                        let norm = (raw / max_raw).clamp(0.0, 1.0);
                        bm25_scores.insert(cid, norm);
                    }
                }
            }
        }

        // 4. Hydratation des fragments candidats depuis la table `chunks`
        // En appliquant le filtrage strict d'obsolescence (WHERE status = 'active')
        let conn = storage_guard.raw_connection();
        let status_clause = if include_deprecated {
            ""
        } else {
            "AND status = 'active'"
        };

        let sql = format!(
            r#"
            SELECT
                id, chunk_id, file_path, content, coala_type,
                status, superseded_by, deprecated_at, date_creation
            FROM chunks
            WHERE id = ?1 {status_clause};
            "#
        );

        let mut stmt = conn.prepare(&sql).map_err(RagError::Database)?;
        let mut candidate_results = Vec::new();
        let mut link_cache: HashMap<String, Option<String>> = HashMap::new();

        for (rowid, dist) in &vec_results {
            let mut rows = stmt
                .query(rusqlite::params![rowid])
                .map_err(RagError::Database)?;
            if let Some(row) = rows.next().map_err(RagError::Database)? {
                let chunk_id: String = row.get(1).map_err(RagError::Database)?;
                let file_path: String = row.get(2).map_err(RagError::Database)?;
                let content: String = row.get(3).map_err(RagError::Database)?;
                let coala_str: String = row.get(4).map_err(RagError::Database)?;
                let status_str: String = row.get(5).map_err(RagError::Database)?;
                let mut superseded_by: Option<String> = row.get(6).map_err(RagError::Database)?;
                let deprecated_at: Option<i64> = row.get(7).map_err(RagError::Database)?;
                let date_creation: i64 = row.get(8).map_err(RagError::Database)?;

                let coala_type = CoalaType::from_str_lenient(&coala_str);
                let status = NoteStatus::from_str_lenient(&status_str);

                // Similarité vectorielle bornée dans [0.0, 1.0]
                let vector_score = (1.0f64 - *dist as f64).clamp(0.0, 1.0);

                // Score lexical BM25 normalisé
                let bm25_score = bm25_scores.get(&chunk_id).copied().unwrap_or(0.0);

                // 5. Résolution d'obsolescence du graphe à 1-hop
                if superseded_by.as_deref().unwrap_or("").trim().is_empty() {
                    superseded_by = None;
                    let replacement = match link_cache.entry(file_path.clone()) {
                        std::collections::hash_map::Entry::Occupied(e) => e.get().clone(),
                        std::collections::hash_map::Entry::Vacant(e) => {
                            let resolved = storage_guard
                                .get_superseding_file(&file_path)
                                .unwrap_or(None);
                            e.insert(resolved.clone());
                            resolved
                        }
                    };
                    if replacement.is_some() {
                        superseded_by = replacement;
                    }
                }

                // 6. Formule d'atténuation temporelle Time-Decay
                let age_days = ((now - date_creation).max(0) as f64) / 86400.0;
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
        }

        // 7. Tri décroissant par score combiné et troncature à la limite demandée
        candidate_results.sort_by(|a, b| {
            b.combined_score
                .partial_cmp(&a.combined_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        candidate_results.truncate(limit);
        Ok(candidate_results)
    }
}

/// Nettoie la requête textuelle pour éviter les erreurs de syntaxe FTS5.
fn sanitize_fts_query(query: &str) -> String {
    let trimmed = query.trim();
    let quote_count = trimmed.chars().filter(|c| *c == '"').count();
    if quote_count % 2 != 0 {
        trimmed.replace('"', "")
    } else {
        trimmed.to_string()
    }
}
