use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

/// Résout le chemin effectif du coffre Jeanne.
/// Si un chemin personnalisé est fourni, vérifie son existence sur le disque.
/// Sinon, utilise la résolution automatique standard de `jeanne-core`.
pub fn resolve_effective_vault_path(custom_path: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = custom_path {
        if !p.exists() {
            bail!(
                "Le dossier de coffre spécifié est introuvable : {}",
                p.display()
            );
        }
        if !p.is_dir() {
            bail!(
                "Le chemin spécifié n'est pas un répertoire : {}",
                p.display()
            );
        }
        Ok(p.to_path_buf())
    } else {
        jeanne_core::resolve_vault_path()
            .with_context(|| "Impossible de résoudre le chemin par défaut du coffre Jeanne")
    }
}

/// Normalise le chemin relatif d'un fichier par rapport à la racine du coffre
/// en utilisant des séparateurs `/` compatibles multi-plateforme.
pub fn normalize_relative_path(vault_path: &Path, file_path: &Path) -> Option<String> {
    if let Ok(rel) = file_path.strip_prefix(vault_path) {
        let normalized = rel.to_string_lossy().replace('\\', "/");
        let trimmed = normalized.trim_start_matches('/').to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }

    // Repli en cas de symlinks ou de chemins absolus non normalisés
    if let (Ok(canon_vault), Ok(canon_file)) = (vault_path.canonicalize(), file_path.canonicalize())
    {
        if let Ok(rel) = canon_file.strip_prefix(&canon_vault) {
            let normalized = rel.to_string_lossy().replace('\\', "/");
            let trimmed = normalized.trim_start_matches('/').to_string();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }

    None
}

/// Formate un timestamp Unix en chaîne de date lisible UTC.
pub fn format_timestamp(ts: i64) -> String {
    if ts == 0 {
        return "N/A".to_string();
    }
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "Date invalide".to_string())
}
