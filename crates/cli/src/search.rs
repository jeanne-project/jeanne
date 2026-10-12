use anyhow::{Context, Result, bail};
use jeanne_core::models::SearchResult;
use jeanne_core::storage::StorageManager;
use std::path::Path;

/// Recherche des notes dans le coffre Jeanne via FTS5 (avec repli en recherche de questions).
pub fn search_vault(
    vault_path: &Path,
    query: &str,
    limit: usize,
    json_output: bool,
) -> Result<Vec<SearchResult>> {
    let db_path = vault_path.join(".jeanne").join("database.db");
    if !db_path.exists() {
        bail!(
            "Base de données introuvable à {}. Veuillez d'abord indexer le coffre avec 'jeanne vault index'.",
            db_path.display()
        );
    }

    let storage = StorageManager::open(&db_path)
        .with_context(|| format!("Impossible d'ouvrir la base SQLite à {}", db_path.display()))?;

    let trimmed_query = query.trim();
    if trimmed_query.is_empty() {
        if json_output {
            println!("[]");
        } else {
            println!("Requête vide : aucun résultat.");
        }
        return Ok(Vec::new());
    }

    // 1. Recherche FTS5 standard
    let mut results = storage.search_fts(trimmed_query, limit).unwrap_or_default();

    // 2. Repli automatique sur `search_question` si la recherche FTS directe ne renvoie rien
    if results.is_empty() {
        results = storage
            .search_question(trimmed_query, limit)
            .with_context(|| {
                format!("Échec de la recherche par question pour la requête : '{trimmed_query}'")
            })?;
    }

    if json_output {
        let json_str = serde_json::to_string_pretty(&results)
            .with_context(|| "Erreur lors de la sérialisation des résultats en JSON")?;
        println!("{json_str}");
    } else if results.is_empty() {
        println!("Aucun résultat trouvé pour la requête : \"{trimmed_query}\"");
    } else {
        println!(
            "Trouvé {} résultat(s) pour \"{}\" :\n",
            results.len(),
            trimmed_query
        );

        for (idx, res) in results.iter().enumerate() {
            let num = idx + 1;
            // Remplacement des balises <mark> par une surbrillance terminal ANSI (jaune gras)
            let formatted_snippet = res
                .snippet
                .replace("<mark>", "\x1b[1;33m")
                .replace("</mark>", "\x1b[0m");

            println!(
                "[{num}] {} ({})\n    Score  : {:.2} | Statut : {} | Créé : {}\n    Extrait: {}\n",
                res.title,
                res.file_path,
                res.score,
                res.statut,
                res.date_creation,
                formatted_snippet
            );
        }
    }

    Ok(results)
}
