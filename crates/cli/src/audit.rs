use anyhow::{Context, Result, bail};
use jeanne_core::models::{FileLink, VaultStats};
use jeanne_core::parser::parse_markdown;
use jeanne_core::storage::StorageManager;
use jeanne_core::vault::compute_sha256;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use walkdir::WalkDir;

use crate::utils::{format_timestamp, normalize_relative_path};

/// Statut de santé global du coffre.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AuditHealthStatus {
    Ok,
    Warning,
    Error,
}

impl std::fmt::Display for AuditHealthStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ok => write!(f, "OK"),
            Self::Warning => write!(f, "ALERTES"),
            Self::Error => write!(f, "ERREURS"),
        }
    }
}

/// Anomalie de frontmatter détectée lors de l'audit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InvalidFrontmatterIssue {
    pub file_path: String,
    pub error: String,
}

/// Lien `superseded_by` brisé (cible introuvable sur disque).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrokenLinkIssue {
    pub source_path: String,
    pub target_path: String,
    pub reason: String,
}

/// Rapport complet d'audit du coffre Jeanne.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditReport {
    pub vault_path: String,
    pub status: AuditHealthStatus,
    pub sqlite_integrity: String,
    pub stats: Option<VaultStats>,
    pub total_disk_files: usize,
    pub unindexed_files: Vec<String>,
    pub orphan_files: Vec<String>,
    pub outdated_files: Vec<String>,
    pub invalid_frontmatters: Vec<InvalidFrontmatterIssue>,
    pub broken_links: Vec<BrokenLinkIssue>,
    pub errors_count: usize,
    pub warnings_count: usize,
}

/// Exécute l'audit complet du coffre Jeanne et affiche le rapport.
pub fn audit_vault(vault_path: &Path, json_output: bool) -> Result<AuditReport> {
    if !vault_path.exists() {
        bail!(
            "Le dossier du coffre spécifié est introuvable : {}",
            vault_path.display()
        );
    }

    let db_path = vault_path.join(".jeanne").join("database.db");
    let has_db = db_path.exists();

    let mut sqlite_integrity = if has_db {
        "En attente de contrôle".to_string()
    } else {
        "Base SQLite absente (.jeanne/database.db manquant)".to_string()
    };

    let mut db_files: HashMap<String, String> = HashMap::new();
    let mut db_links: Vec<(String, String)> = Vec::new();
    let mut stats: Option<VaultStats> = None;

    let storage_opt = if has_db {
        match StorageManager::open(&db_path) {
            Ok(storage) => Some(storage),
            Err(err) => {
                sqlite_integrity = format!("Erreur d'ouverture SQLite : {err}");
                None
            }
        }
    } else {
        None
    };

    if let Some(ref storage) = storage_opt {
        // 1. Contrôle d'intégrité SQLite PRAGMA
        let raw_conn = storage.raw_connection();
        match raw_conn.query_row("PRAGMA integrity_check;", [], |row| row.get::<_, String>(0)) {
            Ok(integrity) => sqlite_integrity = integrity,
            Err(err) => sqlite_integrity = format!("Échec PRAGMA integrity_check : {err}"),
        }

        // 2. Statistiques globales
        stats = storage.get_stats().ok();

        // 3. Inventaire des fichiers enregistrés dans la table files
        if let Ok(mut stmt) = raw_conn.prepare("SELECT file_path, file_hash FROM files;") {
            if let Ok(rows) = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            }) {
                for item in rows.flatten() {
                    db_files.insert(item.0, item.1);
                }
            }
        }

        // 4. Inventaire des liens de précédence dans SQLite
        if let Ok(mut stmt) = raw_conn
            .prepare("SELECT source_path, target_path FROM file_links WHERE link_type = ?1;")
        {
            if let Ok(rows) = stmt.query_map([FileLink::TYPE_SUPERSEDES], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            }) {
                for item in rows.flatten() {
                    db_links.push(item);
                }
            }
        }
    }

    // 5. Inventaire des fichiers présents sur le disque et validation des frontmatters
    let mut disk_files: HashMap<String, String> = HashMap::new();
    let mut invalid_frontmatters: Vec<InvalidFrontmatterIssue> = Vec::new();
    let mut disk_superseded_links: Vec<(String, String)> = Vec::new();

    let walker = WalkDir::new(vault_path).into_iter().filter_entry(|entry| {
        if entry.depth() > 0 {
            let name = entry.file_name().to_string_lossy();
            if name.starts_with('.') {
                return false;
            }
        }
        true
    });

    for entry_res in walker {
        let entry = match entry_res {
            Ok(e) => e,
            Err(err) => {
                tracing::warn!("Erreur lors de l'exploration du coffre : {err}");
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
            continue;
        };

        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(err) => {
                invalid_frontmatters.push(InvalidFrontmatterIssue {
                    file_path: rel_path.clone(),
                    error: format!("Impossible de lire le fichier UTF-8 : {err}"),
                });
                continue;
            }
        };

        let hash = compute_sha256(content.as_bytes());
        disk_files.insert(rel_path.clone(), hash);

        // Validation du frontmatter
        match parse_markdown(&content) {
            Ok((frontmatter, _)) => {
                // Vérifier la cohérence de date_creation si présente
                let date_str = frontmatter.date_creation.trim();
                if !date_str.is_empty() && frontmatter.parse_date_creation().is_none() {
                    invalid_frontmatters.push(InvalidFrontmatterIssue {
                        file_path: rel_path.clone(),
                        error: format!("Format de date_creation invalide : '{date_str}'"),
                    });
                }

                if let Some(target) = frontmatter.clean_superseded_by() {
                    disk_superseded_links.push((rel_path.clone(), target));
                }
            }
            Err(err) => {
                invalid_frontmatters.push(InvalidFrontmatterIssue {
                    file_path: rel_path.clone(),
                    error: err.to_string(),
                });
            }
        }
    }

    // 6. Comparaison Disque vs Base SQLite
    let mut unindexed_files: Vec<String> = Vec::new();
    let mut outdated_files: Vec<String> = Vec::new();

    for (rel_path, disk_hash) in &disk_files {
        match db_files.get(rel_path) {
            None => unindexed_files.push(rel_path.clone()),
            Some(db_hash) => {
                if db_hash != disk_hash {
                    outdated_files.push(rel_path.clone());
                }
            }
        }
    }

    let mut orphan_files: Vec<String> = Vec::new();
    for db_path_key in db_files.keys() {
        if !disk_files.contains_key(db_path_key) {
            orphan_files.push(db_path_key.clone());
        }
    }

    unindexed_files.sort();
    outdated_files.sort();
    orphan_files.sort();

    // 7. Détection des liens superseded_by brisés
    let mut broken_links: Vec<BrokenLinkIssue> = Vec::new();
    let mut seen_broken: HashSet<(String, String)> = HashSet::new();

    let all_links = db_links
        .into_iter()
        .chain(disk_superseded_links)
        .collect::<Vec<_>>();

    for (source, target) in all_links {
        if seen_broken.contains(&(source.clone(), target.clone())) {
            continue;
        }

        let target_clean = target.trim().replace('\\', "/");
        let trimmed_target = target_clean.trim_start_matches('/');
        let target_with_md = if trimmed_target.ends_with(".md") {
            trimmed_target.to_string()
        } else {
            format!("{trimmed_target}.md")
        };

        let exists_on_disk = disk_files.contains_key(trimmed_target)
            || disk_files.contains_key(&target_with_md)
            || vault_path.join(trimmed_target).exists()
            || vault_path.join(&target_with_md).exists();

        if !exists_on_disk {
            seen_broken.insert((source.clone(), target.clone()));
            broken_links.push(BrokenLinkIssue {
                source_path: source,
                target_path: target,
                reason: "La note cible n'existe pas dans le coffre".to_string(),
            });
        }
    }

    // 8. Calcul de la sévérité et du statut de santé
    let is_sqlite_clean = sqlite_integrity.trim().eq_ignore_ascii_case("ok");
    let mut errors_count = 0usize;
    let mut warnings_count = 0usize;

    if !is_sqlite_clean {
        errors_count += 1;
    }
    errors_count += invalid_frontmatters.len();
    errors_count += broken_links.len();

    warnings_count += unindexed_files.len();
    warnings_count += orphan_files.len();
    warnings_count += outdated_files.len();

    let status = if errors_count > 0 {
        AuditHealthStatus::Error
    } else if warnings_count > 0 {
        AuditHealthStatus::Warning
    } else {
        AuditHealthStatus::Ok
    };

    let report = AuditReport {
        vault_path: vault_path.to_string_lossy().to_string(),
        status,
        sqlite_integrity: sqlite_integrity.clone(),
        stats,
        total_disk_files: disk_files.len(),
        unindexed_files,
        orphan_files,
        outdated_files,
        invalid_frontmatters,
        broken_links,
        errors_count,
        warnings_count,
    };

    if json_output {
        let json_str = serde_json::to_string_pretty(&report)
            .with_context(|| "Erreur lors de la sérialisation du rapport d'audit en JSON")?;
        println!("{json_str}");
    } else {
        println!("══════════════════════════════════════════════════════════════════════");
        println!(
            "Diagnostic d'intégrité du coffre Jeanne : {}",
            report.vault_path
        );
        println!("══════════════════════════════════════════════════════════════════════");
        println!("Statut global : [{}]\n", report.status);

        println!("[1] Base de données SQLite");
        println!("    • Intégrité PRAGMA integrity_check : {sqlite_integrity}");
        if let Some(ref st) = report.stats {
            println!(
                "    • Total fichiers indexés           : {}",
                st.total_files
            );
            println!(
                "    • Total fragments (chunks)         : {}",
                st.total_chunks
            );
            println!(
                "    • Dernier scan enregistré          : {}",
                format_timestamp(st.last_scan_timestamp)
            );
        } else {
            println!("    • Statistiques indisponibles (base inaccessible)");
        }
        println!();

        println!("[2] Synchronisation Disque <-> Index");
        println!(
            "    • Fichiers Markdown sur le disque  : {}",
            report.total_disk_files
        );
        println!(
            "    • Fichiers non indexés             : {}",
            report.unindexed_files.len()
        );
        for f in &report.unindexed_files {
            println!("      + {f}");
        }
        println!(
            "    • Fichiers orphelins dans l'index  : {}",
            report.orphan_files.len()
        );
        for f in &report.orphan_files {
            println!("      - {f}");
        }
        println!(
            "    • Fichiers modifiés hors-sync      : {}",
            report.outdated_files.len()
        );
        for f in &report.outdated_files {
            println!("      ~ {f}");
        }
        println!();

        println!("[3] Frontmatters & Métadonnées");
        println!(
            "    • Frontmatters invalides           : {}",
            report.invalid_frontmatters.len()
        );
        for issue in &report.invalid_frontmatters {
            println!("      ! {} -> {}", issue.file_path, issue.error);
        }
        println!();

        println!("[4] Liens d'Obsolescence (superseded_by)");
        println!(
            "    • Liens brisés                     : {}",
            report.broken_links.len()
        );
        for link in &report.broken_links {
            println!(
                "      x {} -> {} ({})",
                link.source_path, link.target_path, link.reason
            );
        }
        println!("══════════════════════════════════════════════════════════════════════");
        println!(
            "Bilan : {} erreur(s), {} avertissement(s).",
            report.errors_count, report.warnings_count
        );
        println!("══════════════════════════════════════════════════════════════════════");
    }

    Ok(report)
}
