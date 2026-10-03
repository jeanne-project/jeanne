//! Module de découverte dynamique, résolution multi-dossiers et gestion des modèles GGUF.
//!
//! Conforme à la spécification `docs/specs/04c_SPEC_MODEL_DISCOVERY_AND_SETTINGS.md`.
//! Garantit une recherche tolérante à la casse et multi-dossiers (local, projet, OS).

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::local_llm::validate_gguf_header;

/// Informations synthétiques sur un modèle GGUF découvert sur le disque.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveredModel {
    /// Nom du fichier (ex: "qwen2.5-3b-instruct-q4_k_m.gguf")
    pub name: String,
    /// Chemin absolu complet du fichier
    pub path: String,
    /// Taille brute en octets
    pub size_bytes: u64,
    /// Taille formatée lisible (ex: "2.10 Go" ou "850.4 Mo")
    pub size_formatted: String,
    /// Architecture GGUF détectée (ex: "Qwen2", "Llama", etc.)
    pub architecture: Option<String>,
    /// Indique si ce modèle est actuellement chargé en mémoire vive
    pub is_loaded: bool,
    /// Indique si le modèle respecte le budget mémoire frugal (< 4.5 Go)
    pub fits_ram: bool,
}

/// Formate une taille en octets en chaîne lisible (Go ou Mo).
pub fn format_file_size(bytes: u64) -> String {
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;

    if bytes as f64 >= GIB {
        format!("{:.2} Go", (bytes as f64) / GIB)
    } else {
        format!("{:.1} Mo", (bytes as f64) / MIB)
    }
}

/// Déduit l'architecture du modèle à partir de son nom de fichier.
pub fn guess_architecture_from_name(name: &str) -> Option<String> {
    let lower = name.to_lowercase();
    if lower.contains("qwen") {
        Some("Qwen2".to_string())
    } else if lower.contains("llama") {
        Some("Llama".to_string())
    } else if lower.contains("mistral") {
        Some("Mistral".to_string())
    } else if lower.contains("phi") {
        Some("Phi".to_string())
    } else if lower.contains("gemma") {
        Some("Gemma".to_string())
    } else if lower.contains("deepseek") {
        Some("DeepSeek".to_string())
    } else {
        None
    }
}

/// Résout l'ensemble des répertoires candidats où des modèles GGUF peuvent être stockés.
///
/// Ordre de priorité :
/// 1. Variable d'environnement `JEANNE_MODELS_DIR`
/// 2. Répertoire `./models` relatif au dossier courant d'exécution
/// 3. Répertoire `models` relatif à l'exécutable et ses parents directs
/// 4. Dossier système standard OS (%APPDATA%\Jeanne\models ou ~/.local/share/jeanne/models)
/// 5. Fallback `./.jeanne/models`
pub fn get_candidate_model_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();

    // 1. Variable d'environnement explicite
    if let Ok(env_dir) = std::env::var("JEANNE_MODELS_DIR") {
        let p = PathBuf::from(env_dir);
        if !dirs.contains(&p) {
            dirs.push(p);
        }
    }

    // 2. Dossier ./models dans le répertoire de travail courant
    if let Ok(cwd) = std::env::current_dir() {
        let cwd_models = cwd.join("models");
        if !dirs.contains(&cwd_models) {
            dirs.push(cwd_models);
        }
    }

    // 3. Dossier relatif à l'exécutable
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let exe_models = exe_dir.join("models");
            if !dirs.contains(&exe_models) {
                dirs.push(exe_models);
            }
            if let Some(parent) = exe_dir.parent() {
                let parent_models = parent.join("models");
                if !dirs.contains(&parent_models) {
                    dirs.push(parent_models);
                }
                if let Some(grandparent) = parent.parent() {
                    let gp_models = grandparent.join("models");
                    if !dirs.contains(&gp_models) {
                        dirs.push(gp_models);
                    }
                }
            }
        }
    }

    // 4. Dossier standard OS
    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            let p = PathBuf::from(appdata).join("Jeanne").join("models");
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
                .join("models");
            if !dirs.contains(&p) {
                dirs.push(p);
            }
        }
    }

    // 5. Fallback local
    let default_dot = PathBuf::from(".jeanne").join("models");
    if !dirs.contains(&default_dot) {
        dirs.push(default_dot);
    }

    dirs
}

/// Découvre tous les modèles GGUF présents dans les répertoires spécifiés.
pub fn discover_models_in_dirs(
    dirs: &[PathBuf],
    currently_loaded_path: Option<&str>,
) -> Vec<DiscoveredModel> {
    let mut discovered: Vec<DiscoveredModel> = Vec::new();
    let loaded_canon = currently_loaded_path.and_then(|p| fs::canonicalize(p).ok());

    for dir in dirs {
        if !dir.exists() || !dir.is_dir() {
            continue;
        }

        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let is_gguf = path
                .extension()
                .map(|ext| ext.to_string_lossy().eq_ignore_ascii_case("gguf"))
                .unwrap_or(false);

            if !is_gguf {
                continue;
            }

            let file_name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            let metadata = match fs::metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let size_bytes = metadata.len();
            let size_formatted = format_file_size(size_bytes);

            // Vérification si le modèle est actuellement chargé
            let is_loaded = if let Some(ref loaded) = loaded_canon {
                fs::canonicalize(&path)
                    .map(|p| &p == loaded)
                    .unwrap_or(false)
            } else if let Some(loaded_str) = currently_loaded_path {
                path.to_string_lossy() == loaded_str
            } else {
                false
            };

            // Architecture : tentative de lecture d'en-tête GGUF puis fallback nom
            let architecture = validate_gguf_header(&path)
                .ok()
                .and_then(|meta| meta.architecture)
                .or_else(|| guess_architecture_from_name(&file_name));

            // Budget RAM : considéré compatible si taille <= 4.5 Go (recommandation 16 Go RAM)
            const MAX_RECOMMENDED_BYTES: u64 = 4_831_838_208; // 4.5 Go
            let fits_ram = size_bytes <= MAX_RECOMMENDED_BYTES;

            let full_path_str = path.to_string_lossy().to_string();

            // Éviter les doublons si le même chemin canonique a déjà été inspecté
            if !discovered.iter().any(|m| m.path == full_path_str) {
                discovered.push(DiscoveredModel {
                    name: file_name,
                    path: full_path_str,
                    size_bytes,
                    size_formatted,
                    architecture,
                    is_loaded,
                    fits_ram,
                });
            }
        }
    }

    // Tri : Modèle chargé en premier, puis Qwen 3B, puis par ordre alphabétique
    discovered.sort_by(|a, b| {
        if a.is_loaded != b.is_loaded {
            return b.is_loaded.cmp(&a.is_loaded);
        }
        let a_is_qwen = a.name.to_lowercase().contains("qwen");
        let b_is_qwen = b.name.to_lowercase().contains("qwen");
        if a_is_qwen != b_is_qwen {
            return b_is_qwen.cmp(&a_is_qwen);
        }
        a.name.cmp(&b.name)
    });

    discovered
}

/// Découvre tous les modèles GGUF présents dans l'ensemble des répertoires candidats.
pub fn discover_models(currently_loaded_path: Option<&str>) -> Vec<DiscoveredModel> {
    let dirs = get_candidate_model_dirs();
    discover_models_in_dirs(&dirs, currently_loaded_path)
}

/// Résout le chemin d'un modèle GGUF dans les répertoires spécifiés.
///
/// Si `requested` est fourni :
/// - Vérifie s'il s'agit d'un chemin de fichier existant
/// - Sinon, cherche par nom de fichier (insensible à la casse) dans chaque répertoire candidat
///
/// Si `requested` est None :
/// - Cherche en priorité un fichier contenant "qwen" et "3b" (insensible à la casse)
/// - Sinon le premier fichier `.gguf` valide trouvé
pub fn resolve_model_path_in_dirs(requested: Option<&str>, dirs: &[PathBuf]) -> Option<PathBuf> {
    if let Some(req) = requested {
        let trimmed = req.trim();
        if trimmed.is_empty() {
            return resolve_model_path_in_dirs(None, dirs);
        }

        let p = Path::new(trimmed);
        if p.is_file() {
            return Some(p.to_path_buf());
        }

        // Recherche par nom de fichier dans les dossiers candidats
        let search_filename = p
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| trimmed.to_string());

        for dir in dirs {
            if !dir.exists() || !dir.is_dir() {
                continue;
            }
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let entry_path = entry.path();
                    if entry_path.is_file() {
                        if let Some(name) = entry_path.file_name() {
                            if name
                                .to_string_lossy()
                                .eq_ignore_ascii_case(&search_filename)
                            {
                                return Some(entry_path);
                            }
                        }
                    }
                }
            }
        }
    } else {
        let mut first_gguf: Option<PathBuf> = None;

        for dir in dirs {
            if !dir.exists() || !dir.is_dir() {
                continue;
            }
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let entry_path = entry.path();
                    if !entry_path.is_file() {
                        continue;
                    }
                    let is_gguf = entry_path
                        .extension()
                        .map(|ext| ext.to_string_lossy().eq_ignore_ascii_case("gguf"))
                        .unwrap_or(false);

                    if !is_gguf {
                        continue;
                    }

                    let file_name = entry_path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_lowercase())
                        .unwrap_or_default();

                    // Priorité absolue : modèle Qwen 3B recommandé
                    if file_name.contains("qwen") && file_name.contains("3b") {
                        return Some(entry_path);
                    }

                    if first_gguf.is_none() {
                        first_gguf = Some(entry_path);
                    }
                }
            }
        }

        if let Some(any_gguf) = first_gguf {
            return Some(any_gguf);
        }
    }

    None
}

/// Résout le chemin d'un modèle GGUF dans les répertoires candidats standards.
pub fn resolve_model_path(requested: Option<&str>) -> Option<PathBuf> {
    let dirs = get_candidate_model_dirs();
    resolve_model_path_in_dirs(requested, &dirs)
}
