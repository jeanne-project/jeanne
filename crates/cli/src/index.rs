use anyhow::{Context, Result};
use jeanne_core::models::{FileLink, IndexedChunk, NoteFrontmatter};
use jeanne_core::parser::parse_markdown;
use jeanne_core::storage::StorageManager;
use jeanne_core::vault::compute_sha256;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Instant;
use walkdir::WalkDir;

use crate::utils::normalize_relative_path;

/// Résumé d'une session d'indexation de coffre Jeanne.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IndexSummary {
    pub vault_path: String,
    pub indexed_files: usize,
    pub indexed_chunks: usize,
    pub links_created: usize,
    pub skipped_files: usize,
    pub duration_ms: u128,
}

/// Indexe récursivement les notes Markdown d'un coffre Jeanne dans sa base SQLite locale.
pub fn index_vault(vault_path: &Path, json_output: bool) -> Result<IndexSummary> {
    let start_time = Instant::now();

    // 1. Ouvrir ou initialiser la base SQLite dans <vault_path>/.jeanne/database.db
    let db_path = vault_path.join(".jeanne").join("database.db");
    let storage = StorageManager::open(&db_path)
        .with_context(|| format!("Impossible d'ouvrir la base SQLite : {}", db_path.display()))?;
    storage
        .init_schema()
        .with_context(|| "Échec de l'initialisation du schéma SQLite")?;

    // 2. Parcourir récursivement les fichiers Markdown en ignorant .jeanne et dossiers cachés
    let mut indexed_files = 0usize;
    let mut indexed_chunks = 0usize;
    let mut links_created = 0usize;
    let mut skipped_files = 0usize;

    let entries = WalkDir::new(vault_path).into_iter().filter_entry(|entry| {
        // Ignorer les répertoires cachés (ex: .jeanne, .git, .obsidian)
        if entry.depth() > 0 {
            let name = entry.file_name().to_string_lossy();
            if name.starts_with('.') {
                return false;
            }
        }
        true
    });

    for entry_res in entries {
        let entry = match entry_res {
            Ok(e) => e,
            Err(err) => {
                tracing::warn!("Erreur lors du parcours d'un fichier : {err}");
                skipped_files += 1;
                continue;
            }
        };

        if !entry.file_type().is_file() {
            continue;
        }

        let file_path = entry.path();
        let is_md = file_path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("md"))
            .unwrap_or(false);

        if !is_md {
            continue;
        }

        let Some(rel_path) = normalize_relative_path(vault_path, file_path) else {
            skipped_files += 1;
            continue;
        };

        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(err) => {
                tracing::warn!(
                    "Échec de lecture du fichier {} : {err}",
                    file_path.display()
                );
                skipped_files += 1;
                continue;
            }
        };

        let file_hash = compute_sha256(content.as_bytes());
        let (frontmatter, body) = match parse_markdown(&content) {
            Ok(parsed) => parsed,
            Err(err) => {
                tracing::warn!(
                    "Frontmatter invalide ou absent pour {rel_path} ({err}), repli par défaut"
                );
                (NoteFrontmatter::default(), content.clone())
            }
        };

        let frontmatter_json = serde_json::to_string(&frontmatter).ok();
        let now = chrono::Local::now().timestamp();

        let conn = storage.raw_connection();
        conn.execute_batch("BEGIN IMMEDIATE;")
            .with_context(|| "Impossible d'initier la transaction SQLite pour le fichier")?;

        let mut file_links_added = 0usize;
        let tx_result: Result<()> = (|| {
            storage
                .upsert_file(&rel_path, &file_hash, now, frontmatter_json.as_deref())
                .with_context(|| format!("Échec upsert_file pour {rel_path}"))?;

            let token_count = body.split_whitespace().count();
            let date_creation = frontmatter.parse_date_creation().unwrap_or(now);
            let superseded_by = frontmatter.clean_superseded_by();
            let deprecated_at = frontmatter.deprecated_at;

            let chunk = IndexedChunk {
                id: None,
                chunk_id: format!("{rel_path}:0"),
                file_path: rel_path.clone(),
                chunk_index: 0,
                content: body,
                token_count,
                coala_type: frontmatter.coala_type(),
                status: frontmatter.status(),
                superseded_by: superseded_by.clone(),
                deprecated_at,
                date_creation,
            };

            storage
                .index_chunk(&chunk)
                .with_context(|| format!("Échec index_chunk pour {rel_path}"))?;

            if let Some(target) = superseded_by {
                storage
                    .insert_file_link(&rel_path, &target, FileLink::TYPE_SUPERSEDES, now)
                    .with_context(|| format!("Échec insert_file_link pour {rel_path}"))?;
                file_links_added += 1;
            }

            Ok(())
        })();

        match tx_result {
            Ok(()) => {
                conn.execute_batch("COMMIT;")
                    .with_context(|| "Échec du commit SQLite")?;
                indexed_files += 1;
                indexed_chunks += 1;
                links_created += file_links_added;
            }
            Err(err) => {
                let _ = conn.execute_batch("ROLLBACK;");
                return Err(err);
            }
        }
    }

    let duration_ms = start_time.elapsed().as_millis();
    let summary = IndexSummary {
        vault_path: vault_path.to_string_lossy().to_string(),
        indexed_files,
        indexed_chunks,
        links_created,
        skipped_files,
        duration_ms,
    };

    if json_output {
        let json_str = serde_json::to_string_pretty(&summary)
            .with_context(|| "Erreur lors de la sérialisation du résumé en JSON")?;
        println!("{json_str}");
    } else {
        println!("✓ Coffre indexé avec succès : {}", summary.vault_path);
        println!("  • Fichiers indexés    : {}", summary.indexed_files);
        println!("  • Fragments générés   : {}", summary.indexed_chunks);
        println!("  • Liens de précédence : {}", summary.links_created);
        if summary.skipped_files > 0 {
            println!("  • Fichiers ignorés    : {}", summary.skipped_files);
        }
        println!("  • Durée               : {} ms", summary.duration_ms);
    }

    Ok(summary)
}
