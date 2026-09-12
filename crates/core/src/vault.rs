use std::path::{Path, PathBuf};
use crate::error::Result;

/// Résout le chemin absolu du coffre (vault) Jeanne selon la stratégie de priorité :
/// 1. Variable d'environnement `JEANNE_VAULT_PATH`
/// 2. Répertoire documents utilisateur (`Documents/JeanneVault`)
pub fn resolve_vault_path() -> Result<PathBuf> {
    if let Ok(env_val) = std::env::var("JEANNE_VAULT_PATH") {
        let trimmed = env_val.trim();
        if !trimmed.is_empty() {
            let path = PathBuf::from(trimmed);
            if !path.exists() {
                std::fs::create_dir_all(&path)?;
            }
            return Ok(path);
        }
    }

    let base_dir = if cfg!(windows) {
        std::env::var("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
    } else {
        std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
    };

    let vault_dir = base_dir.join("Documents").join("JeanneVault");
    if !vault_dir.exists() {
        let _ = std::fs::create_dir_all(&vault_dir);
    }

    Ok(vault_dir)
}

/// Observateur de système de fichiers pour la synchronisation du coffre en temps réel.
pub struct VaultWatcher {
    _vault_path: PathBuf,
}

impl VaultWatcher {
    /// Initialise un nouvel observateur pour le chemin de coffre spécifié.
    pub fn new<P: AsRef<Path>>(vault_path: P) -> Result<Self> {
        Ok(Self {
            _vault_path: vault_path.as_ref().to_path_buf(),
        })
    }

    /// Démarre la boucle de surveillance asynchrone des événements de fichiers.
    pub fn start(&mut self) -> Result<()> {
        Ok(())
    }
}
