use crate::error::Result;
use std::path::{Path, PathBuf};

/// Résout le chemin absolu du coffre (vault) Jeanne selon la stratégie de priorité :
/// 1. Variable d'environnement `JEANNE_VAULT_PATH`
/// 2. Répertoire documents utilisateur (`Documents/JeanneVault`)
/// 3. Crée le dossier et le fichier d'introduction `Welcome.md` s'ils n'existent pas.
pub fn resolve_vault_path() -> Result<PathBuf> {
    let path = if let Ok(env_val) = std::env::var("JEANNE_VAULT_PATH") {
        let trimmed = env_val.trim();
        if !trimmed.is_empty() {
            let p = PathBuf::from(trimmed);
            if !p.exists() {
                std::fs::create_dir_all(&p)?;
            }
            p
        } else {
            fallback_vault_path()?
        }
    } else {
        fallback_vault_path()?
    };

    ensure_welcome_note(&path)?;
    Ok(path)
}

fn fallback_vault_path() -> Result<PathBuf> {
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
        std::fs::create_dir_all(&vault_dir)?;
    }
    Ok(vault_dir)
}

fn ensure_welcome_note(vault_dir: &Path) -> Result<()> {
    let welcome_path = vault_dir.join("Welcome.md");
    if !welcome_path.exists() {
        let content = r#"---
id: welcome-note
title: Bienvenue dans Jeanne
date_creation: "2026-09-12T00:00:00Z"
date_modification: "2026-09-12T00:00:00Z"
type: semantique
statut: actif
tags:
  - guide
  - bienvenue
---

# Bienvenue dans Jeanne

Jeanne est votre assistant de connaissances souverain et local-first ("File-over-App").

## Principes clés
- **Vos fichiers vous appartiennent** : toutes les notes sont au format Markdown standard sur votre disque.
- **Accès Rapide** : appuyez sur `Alt + Espace` pour invoquer la palette flottante.
- **Recherche Instantanée** : indexation plein-texte SQLite FTS5 ultra-rapide.
- **Prise de note rapide** : tapez `/note Votre pensée` pour consigner directement dans le journal du jour.
"#;
        std::fs::write(&welcome_path, content)?;
    }
    Ok(())
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
