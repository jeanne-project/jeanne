//! Gestionnaire de découverte, d'enregistrement et d'indexation des plugins Jeanne.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::debug;

use super::models::{PluginCapability, PluginError, PluginManifest};

/// Gestionnaire de plugins scannant l'écosystème modulaire de Jeanne.
#[derive(Debug, Default, Clone)]
pub struct PluginManager {
    plugins: HashMap<String, PluginManifest>,
}

impl PluginManager {
    /// Initialise un nouveau gestionnaire et découvre les plugins disponibles.
    pub fn new() -> Self {
        let mut manager = Self::default();
        manager.discover_plugins();
        manager
    }

    /// Résout la liste ordonnée des répertoires où chercher des plugins.
    pub fn candidate_plugin_dirs() -> Vec<PathBuf> {
        let mut dirs = Vec::new();

        // 1. Variable d'environnement explicite
        if let Ok(env_path) = std::env::var("JEANNE_PLUGINS_DIR") {
            let p = PathBuf::from(env_path);
            if !dirs.contains(&p) {
                dirs.push(p);
            }
        }

        // 2. Variable d'environnement CARGO_MANIFEST_DIR (en environnement de test)
        if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
            let m_path = PathBuf::from(manifest_dir);
            if let Some(parent) = m_path.parent() {
                let p = parent.join("plugins");
                if !dirs.contains(&p) {
                    dirs.push(p);
                }
                if let Some(gp) = parent.parent() {
                    let p2 = gp.join("plugins");
                    if !dirs.contains(&p2) {
                        dirs.push(p2);
                    }
                }
            }
        }

        // 3. Répertoire ./plugins relatif au dossier de travail courant et ses parents
        if let Ok(cwd) = std::env::current_dir() {
            let p = cwd.join("plugins");
            if !dirs.contains(&p) {
                dirs.push(p);
            }
            if let Some(parent) = cwd.parent() {
                let p2 = parent.join("plugins");
                if !dirs.contains(&p2) {
                    dirs.push(p2);
                }
                if let Some(gp) = parent.parent() {
                    let p3 = gp.join("plugins");
                    if !dirs.contains(&p3) {
                        dirs.push(p3);
                    }
                }
            }
        }

        // 4. Répertoire plugins relatif à l'exécutable
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(parent) = exe_path.parent() {
                let p = parent.join("plugins");
                if !dirs.contains(&p) {
                    dirs.push(p);
                }
                if let Some(grandparent) = parent.parent() {
                    let gp = grandparent.join("plugins");
                    if !dirs.contains(&gp) {
                        dirs.push(gp);
                    }
                }
            }
        }

        // 4. Dossier standard de l'OS
        #[cfg(target_os = "windows")]
        {
            if let Ok(appdata) = std::env::var("APPDATA") {
                let p = PathBuf::from(appdata).join("Jeanne").join("plugins");
                if !dirs.contains(&p) {
                    dirs.push(p);
                }
            }
        }

        #[cfg(target_os = "linux")]
        {
            if let Ok(home) = std::env::var("HOME") {
                let p = PathBuf::from(home)
                    .join(".local")
                    .join("share")
                    .join("jeanne")
                    .join("plugins");
                if !dirs.contains(&p) {
                    dirs.push(p);
                }
            }
        }

        dirs
    }

    /// Scanne l'ensemble des répertoires candidats et indexe les manifestes `plugin.json`.
    pub fn discover_plugins(&mut self) {
        let candidate_dirs = Self::candidate_plugin_dirs();

        for root in candidate_dirs {
            if !root.exists() || !root.is_dir() {
                continue;
            }

            debug!("Recherche de plugins sous : {}", root.display());
            if let Ok(entries) = std::fs::read_dir(&root) {
                for entry in entries.flatten() {
                    let sub_dir = entry.path();
                    if sub_dir.is_dir() {
                        let manifest_path = sub_dir.join("plugin.json");
                        if manifest_path.exists() && manifest_path.is_file() {
                            if let Ok(manifest) = Self::load_manifest(&manifest_path) {
                                debug!(
                                    "Plugin découvert : id={}, nom='{}', version={}",
                                    manifest.id, manifest.name, manifest.version
                                );
                                self.plugins.insert(manifest.id.clone(), manifest);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Charge et valide un fichier `plugin.json` donné.
    pub fn load_manifest(manifest_path: &Path) -> Result<PluginManifest, PluginError> {
        let content = std::fs::read_to_string(manifest_path).map_err(|e| {
            PluginError::InvalidManifest(
                manifest_path.display().to_string(),
                format!("Impossible de lire le fichier : {e}"),
            )
        })?;

        let mut manifest: PluginManifest = serde_json::from_str(&content).map_err(|e| {
            PluginError::InvalidManifest(
                manifest_path.display().to_string(),
                format!("Erreur de désérialisation JSON : {e}"),
            )
        })?;

        if let Some(parent) = manifest_path.parent() {
            manifest.root_dir = parent.to_path_buf();
        }

        Ok(manifest)
    }

    /// Retourne un plugin par son identifiant unique (ex: "org.jeanneproject.embeddings.generator").
    pub fn get_plugin(&self, id: &str) -> Option<&PluginManifest> {
        self.plugins.get(id)
    }

    /// Recherche le premier plugin déclarant une capability donnée (ex: "embeddings_generator", "llm_runner").
    pub fn find_by_capability(&self, capability_type: &str) -> Option<&PluginManifest> {
        self.plugins.values().find(|m| {
            m.capabilities.iter().any(|cap| match cap {
                PluginCapability::LlmRunner { .. } => capability_type == "llm_runner",
                PluginCapability::EmbeddingsGenerator { .. } => {
                    capability_type == "embeddings_generator"
                }
                PluginCapability::VoiceStt { .. } => capability_type == "voice_stt",
                PluginCapability::VoiceTts { .. } => capability_type == "voice_tts",
                PluginCapability::DocumentParser { .. } => capability_type == "document_parser",
            })
        })
    }

    /// Liste l'ensemble des manifestes de plugins découverts.
    pub fn list_plugins(&self) -> Vec<&PluginManifest> {
        self.plugins.values().collect()
    }

    /// Résout le chemin absolu de l'exécutable pour un manifeste donné.
    pub fn resolve_executable(manifest: &PluginManifest) -> Result<PathBuf, PluginError> {
        let rel_entrypoint = manifest.entrypoint.resolve_for_current_os();
        let target = manifest.root_dir.join(rel_entrypoint);

        if target.exists() {
            Ok(target)
        } else {
            Err(PluginError::ExecutableNotFound(
                manifest.id.clone(),
                target.display().to_string(),
            ))
        }
    }
}
