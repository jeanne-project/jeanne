use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::Manager;
use tauri_plugin_global_shortcut::GlobalShortcutExt;
use tokio::io::AsyncWriteExt;

use jeanne_core::{IndexedChunk, NoteFrontmatter, SearchResult, StorageManager, VaultStats};

/// État applicatif partagé contenant l'accès sécurisé au moteur SQLite et le chemin racine du coffre.
pub struct AppState {
    pub storage: Mutex<StorageManager>,
    pub vault_path: PathBuf,
}

#[tauri::command]
fn get_core_version() -> String {
    jeanne_core::version().to_string()
}

#[tauri::command]
async fn search_notes(
    state: tauri::State<'_, AppState>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<SearchResult>, String> {
    let storage = state
        .storage
        .lock()
        .map_err(|e| format!("Erreur d'accès à la base de données : {e}"))?;
    let limit = limit.unwrap_or(20);
    storage.search_fts(&query, limit).map_err(|e| e.to_string())
}

#[tauri::command]
async fn capture_quick_note(
    state: tauri::State<'_, AppState>,
    content: String,
) -> Result<String, String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Err("Le contenu de la note ne peut pas être vide".to_string());
    }

    let body_text = if let Some(stripped) = trimmed.strip_prefix("/note") {
        stripped.trim()
    } else {
        trimmed
    };

    if body_text.is_empty() {
        return Err("Le contenu de la note ne peut pas être vide".to_string());
    }

    let now = chrono::Local::now();
    let date_str = now.format("%Y-%m-%d").to_string();
    let time_str = now.format("%H:%M:%S").to_string();
    let now_iso = now.to_rfc3339();

    let journal_dir = state.vault_path.join("Journal");
    if !journal_dir.exists() {
        tokio::fs::create_dir_all(&journal_dir)
            .await
            .map_err(|e| format!("Impossible de créer le dossier Journal : {e}"))?;
    }

    let note_file = journal_dir.join(format!("{date_str}.md"));
    let is_new = !note_file.exists();

    if is_new {
        let initial_content = format!(
            "---\nid: journal-{date_str}\ntitle: Journal {date_str}\ndate_creation: \"{now_iso}\"\ndate_modification: \"{now_iso}\"\nnote_type: episodique\nstatut: actif\ntags:\n  - journal\n---\n\n# Journal - {date_str}\n\n## {time_str}\n{body_text}\n"
        );
        tokio::fs::write(&note_file, initial_content)
            .await
            .map_err(|e| format!("Impossible d'écrire la note : {e}"))?;
    } else {
        let mut file = tokio::fs::OpenOptions::new()
            .append(true)
            .open(&note_file)
            .await
            .map_err(|e| format!("Impossible d'ouvrir le fichier journal : {e}"))?;
        file.write_all(format!("\n## {time_str}\n{body_text}\n").as_bytes())
            .await
            .map_err(|e| format!("Impossible d'ajouter le contenu à la note : {e}"))?;
    }

    // Indexation FTS5 en temps réel
    let file_content = tokio::fs::read_to_string(&note_file)
        .await
        .map_err(|e| format!("Impossible de lire la note pour indexation : {e}"))?;
    let rel_path = format!("Journal/{date_str}.md");

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    file_content.hash(&mut hasher);
    let file_hash = format!("{:016x}", hasher.finish());

    let storage = state
        .storage
        .lock()
        .map_err(|e| format!("Erreur d'accès à la base de données : {e}"))?;

    let (frontmatter, body) = jeanne_core::parse_markdown(&file_content)
        .unwrap_or_else(|_| (NoteFrontmatter::default(), file_content.clone()));

    let frontmatter_json = serde_json::to_string(&frontmatter).ok();
    storage
        .upsert_file(
            &rel_path,
            &file_hash,
            now.timestamp(),
            frontmatter_json.as_deref(),
        )
        .map_err(|e| e.to_string())?;

    let chunk = IndexedChunk {
        chunk_id: format!("{rel_path}:0"),
        file_path: rel_path.clone(),
        chunk_index: 0,
        content: body,
        token_count: body_text.split_whitespace().count(),
        note_type: frontmatter.note_type,
        statut: frontmatter.statut,
        date_creation: now.timestamp(),
    };
    storage.index_chunk(&chunk).map_err(|e| e.to_string())?;

    Ok(note_file.to_string_lossy().to_string())
}

/// Valide le confinement strict et l'extension autorisée pour l'ouverture d'une note.
fn validate_and_resolve_note_path(vault_path: &Path, file_path: &str) -> Result<PathBuf, String> {
    let path = Path::new(file_path);
    let target_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        vault_path.join(path)
    };

    if !target_path.exists() {
        return Err(format!(
            "Le fichier n'existe pas : {}",
            target_path.display()
        ));
    }

    // 1. Protection stricte contre le Path Traversal via canonicalisation
    let canonical_target = target_path
        .canonicalize()
        .map_err(|e| format!("Chemin cible invalide : {e}"))?;
    let canonical_vault = vault_path
        .canonicalize()
        .map_err(|e| format!("Chemin du coffre invalide : {e}"))?;

    if !canonical_target.starts_with(&canonical_vault) {
        return Err("Accès refusé : le fichier doit résider à l'intérieur du coffre".to_string());
    }

    // 2. Restriction stricte aux extensions Markdown
    let extension = canonical_target
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    if extension != "md" && extension != "markdown" {
        return Err(
            "Format non autorisé : seules les notes Markdown peuvent être ouvertes".to_string(),
        );
    }

    Ok(canonical_target)
}

#[tauri::command]
async fn open_note_in_editor(
    state: tauri::State<'_, AppState>,
    file_path: String,
) -> Result<(), String> {
    let canonical_target = validate_and_resolve_note_path(&state.vault_path, &file_path)?;
    let path_str = canonical_target.to_string_lossy().to_string();

    #[cfg(target_os = "windows")]
    {
        // Utilisation directe de rundll32 avec ShellExecuteEx via FileProtocolHandler
        // sans passer par cmd.exe pour éliminer tout risque d'injection de commande shell
        std::process::Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", &path_str])
            .spawn()
            .map_err(|e| {
                format!("Impossible d'ouvrir le fichier avec l'application système : {e}")
            })?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path_str)
            .spawn()
            .map_err(|e| {
                format!("Impossible d'ouvrir le fichier avec l'application système : {e}")
            })?;
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(&path_str)
            .spawn()
            .map_err(|e| {
                format!("Impossible d'ouvrir le fichier avec l'application système : {e}")
            })?;
    }

    Ok(())
}

#[tauri::command]
async fn get_vault_stats(state: tauri::State<'_, AppState>) -> Result<VaultStats, String> {
    let storage = state
        .storage
        .lock()
        .map_err(|e| format!("Erreur d'accès à la base de données : {e}"))?;
    storage.get_stats().map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_quick_access_height(app: tauri::AppHandle, height: u32) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("quick-access") {
        window
            .set_size(tauri::LogicalSize::new(720.0, height as f64))
            .map_err(|e| format!("Erreur redimensionnement fenêtre : {e}"))?;
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt::init();
    tracing::info!("Démarrage du client Jeanne Desktop...");

    if let Err(err) = tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        if let Some(window) = app.get_webview_window("quick-access") {
                            let is_visible = window.is_visible().unwrap_or(false);
                            if is_visible {
                                let _ = window.hide();
                            } else {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            get_core_version,
            search_notes,
            capture_quick_note,
            open_note_in_editor,
            get_vault_stats,
            set_quick_access_height
        ])
        .setup(|app| {
            tracing::info!("Initialisation des sous-systèmes Jeanne Desktop...");

            let vault_path = match jeanne_core::resolve_vault_path() {
                Ok(path) => path,
                Err(err) => {
                    tracing::error!("Impossible de résoudre le chemin du coffre : {}", err);
                    return Err(Box::new(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("Erreur chemin de coffre : {err}"),
                    )));
                }
            };

            let db_path = vault_path.join(".jeanne").join("database.db");
            let storage = match StorageManager::open(&db_path) {
                Ok(s) => s,
                Err(err) => {
                    tracing::error!("Impossible d'ouvrir la base SQLite : {}", err);
                    return Err(Box::new(std::io::Error::other(format!(
                        "Erreur ouverture DB : {err}"
                    ))));
                }
            };

            if let Err(err) = storage.init_schema() {
                tracing::error!("Impossible d'initialiser le schéma SQLite : {}", err);
                return Err(Box::new(std::io::Error::other(format!(
                    "Erreur initialisation schéma : {err}"
                ))));
            }

            app.manage(AppState {
                storage: Mutex::new(storage),
                vault_path,
            });

            // Enregistrement du raccourci global avec repli en cascade
            let global_shortcut = app.global_shortcut();
            let candidates = ["Alt+Space", "Alt+Shift+Space", "Ctrl+Shift+Space"];
            let mut registered = false;

            for candidate in &candidates {
                match global_shortcut.register(*candidate) {
                    Ok(_) => {
                        tracing::info!("Raccourci global enregistré avec succès : {}", candidate);
                        registered = true;
                        break;
                    }
                    Err(err) => {
                        tracing::warn!(
                            "Échec d'enregistrement du raccourci global '{}' : {}",
                            candidate,
                            err
                        );
                    }
                }
            }

            if !registered {
                tracing::error!(
                    "Impossible d'enregistrer un raccourci global valide pour l'overlay."
                );
            }

            tracing::info!("Sous-système bureau Tauri initialisé.");
            Ok(())
        })
        .on_window_event(|window, event| {
            // Masquage automatique de la fenêtre d'accès rapide lors de la perte de focus
            if let tauri::WindowEvent::Focused(false) = event {
                if window.label() == "quick-access" {
                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
    {
        tracing::error!("Erreur critique lors de l'exécution de Jeanne Desktop : {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_core_version_command() {
        let version = get_core_version();
        assert!(!version.is_empty());
    }

    #[tokio::test]
    async fn test_storage_commands_in_memory() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let db_path = temp_dir.path().join("test.db");
        let storage = StorageManager::open(&db_path).expect("open storage");
        storage.init_schema().expect("init schema");

        let app_state = AppState {
            storage: Mutex::new(storage),
            vault_path: temp_dir.path().to_path_buf(),
        };

        // Test insertion manuelle et recherche
        {
            let s = app_state.storage.lock().unwrap();
            s.upsert_file("Journal/2026-09-12.md", "hash1", 1710000000, None)
                .expect("upsert file");
            s.index_chunk(&IndexedChunk {
                chunk_id: "Journal/2026-09-12.md:0".to_string(),
                file_path: "Journal/2026-09-12.md".to_string(),
                chunk_index: 0,
                content: "Note rapide de réunion sur l'architecture Jeanne".to_string(),
                token_count: 8,
                note_type: "episodique".to_string(),
                statut: "actif".to_string(),
                date_creation: 1710000000,
            })
            .expect("index chunk");
        }

        let s = app_state.storage.lock().unwrap();
        let results = s.search_fts("architecture", 10).expect("search");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].chunk_id, "Journal/2026-09-12.md:0");

        let stats = s.get_stats().expect("stats");
        assert_eq!(stats.total_files, 1);
        assert_eq!(stats.total_chunks, 1);
    }

    #[test]
    fn test_validate_and_resolve_note_path_valid() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let vault_path = temp_dir.path();
        let note_path = vault_path.join("test.md");
        std::fs::write(&note_path, "# Test").expect("write test file");

        let resolved = validate_and_resolve_note_path(vault_path, "test.md");
        assert!(resolved.is_ok());
        assert_eq!(resolved.unwrap(), note_path.canonicalize().unwrap());
    }

    #[test]
    fn test_validate_and_resolve_note_path_outside_vault() {
        let vault_dir = tempfile::tempdir().expect("vault dir");
        let outside_dir = tempfile::tempdir().expect("outside dir");
        let outside_file = outside_dir.path().join("evil.md");
        std::fs::write(&outside_file, "# Evil").expect("write file");

        let result =
            validate_and_resolve_note_path(vault_dir.path(), outside_file.to_str().unwrap());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Accès refusé"));
    }

    #[test]
    fn test_validate_and_resolve_note_path_forbidden_extension() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let vault_path = temp_dir.path();
        let exe_file = vault_path.join("script.bat");
        std::fs::write(&exe_file, "echo hello").expect("write file");

        let result = validate_and_resolve_note_path(vault_path, "script.bat");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Format non autorisé"));
    }

    #[test]
    fn test_validate_and_resolve_note_path_nonexistent() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let vault_path = temp_dir.path();

        let result = validate_and_resolve_note_path(vault_path, "nonexistent.md");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("n'existe pas"));
    }
}
