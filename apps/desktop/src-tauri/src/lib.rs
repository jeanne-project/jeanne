use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::Manager;
use tauri_plugin_global_shortcut::GlobalShortcutExt;
use tokio::io::AsyncWriteExt;

use jeanne_core::{IndexedChunk, NoteFrontmatter, SearchResult, StorageManager, VaultStats, VaultWatcher};

/// État applicatif partagé contenant l'accès sécurisé au moteur SQLite et le chemin racine du coffre.
pub struct AppState {
    pub storage: Arc<Mutex<StorageManager>>,
    pub vault_path: PathBuf,
    pub watcher: Mutex<Option<VaultWatcher>>,
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
    capture_quick_note_core(&state.vault_path, &state.storage, &content).await
}

async fn capture_quick_note_core(
    vault_path: &Path,
    storage_mutex: &Mutex<StorageManager>,
    content: &str,
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

    let journal_dir = vault_path.join("Journal");
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

    let storage = storage_mutex
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

fn clean_exit(app: &tauri::AppHandle) {
    tracing::info!("Arrêt ordonné de Jeanne et libération des ressources...");
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut watcher_guard) = state.watcher.lock() {
            if let Some(mut watcher) = watcher_guard.take() {
                watcher.stop();
            }
        }
    }
    if let Some(quick_window) = app.get_webview_window("quick-access") {
        let _ = quick_window.destroy();
    }
    if let Some(main_window) = app.get_webview_window("main") {
        let _ = main_window.destroy();
    }
    app.exit(0);
}

#[tauri::command]
fn exit_app(app: tauri::AppHandle) {
    tracing::info!("Fermeture de Jeanne demandée depuis l'application.");
    clean_exit(&app);
}

/// Tente d'enregistrer séquentiellement une liste ordonnée de raccourcis candidats.
/// Retourne le premier raccourci ayant réussi son enregistrement auprès du système.
pub fn register_first_available_shortcut<F>(
    candidates: &[&str],
    mut register_fn: F,
) -> Option<String>
where
    F: FnMut(&str) -> Result<(), String>,
{
    for candidate in candidates {
        match register_fn(candidate) {
            Ok(()) => return Some(candidate.to_string()),
            Err(err) => {
                tracing::warn!(
                    "Échec d'enregistrement du raccourci global '{}' : {}",
                    candidate,
                    err
                );
            }
        }
    }
    None
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
            set_quick_access_height,
            exit_app
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

            let storage_arc = Arc::new(Mutex::new(storage));

            let mut watcher = match VaultWatcher::new(&vault_path, storage_arc.clone()) {
                Ok(w) => w,
                Err(err) => {
                    tracing::error!("Impossible d'initialiser le VaultWatcher : {}", err);
                    return Err(Box::new(std::io::Error::other(format!(
                        "Erreur création watcher: {err}"
                    ))));
                }
            };

            if let Err(err) = watcher.start() {
                tracing::warn!("Avertissement lors du démarrage du VaultWatcher : {}", err);
            } else {
                tracing::info!("VaultWatcher démarré avec succès sur {}", vault_path.display());
            }

            app.manage(AppState {
                storage: storage_arc,
                vault_path,
                watcher: Mutex::new(Some(watcher)),
            });

            // Enregistrement du raccourci global avec repli en cascade
            let global_shortcut = app.global_shortcut();
            let candidates = ["Alt+Space", "Alt+Shift+Space", "Ctrl+Shift+Space"];
            let registered = register_first_available_shortcut(&candidates, |candidate| {
                global_shortcut
                    .register(candidate)
                    .map_err(|e| e.to_string())
            });

            match registered {
                Some(shortcut) => {
                    tracing::info!("Raccourci global enregistré avec succès : {}", shortcut);
                }
                None => {
                    tracing::error!(
                        "Impossible d'enregistrer un raccourci global valide pour l'overlay."
                    );
                }
            }

            // Enregistrement de l'icône de zone de notification système (System Tray)
            let quit_i = tauri::menu::MenuItemBuilder::with_id("quit", "Quitter Jeanne").build(app)?;
            let show_main_i = tauri::menu::MenuItemBuilder::with_id("show_main", "Ouvrir Jeanne").build(app)?;
            let show_overlay_i = tauri::menu::MenuItemBuilder::with_id("show_overlay", "Palette d'accès rapide (Alt+Espace)").build(app)?;

            let tray_menu = tauri::menu::MenuBuilder::new(app)
                .items(&[&show_main_i, &show_overlay_i])
                .separator()
                .items(&[&quit_i])
                .build()?;

            let mut tray_builder = tauri::tray::TrayIconBuilder::new()
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    match event.id().as_ref() {
                        "quit" => {
                            tracing::info!("Fermeture de Jeanne demandée depuis le menu de notification.");
                            clean_exit(app);
                        }
                        "show_main" => {
                            if let Some(win) = app.get_webview_window("main") {
                                let _ = win.unminimize();
                                let _ = win.show();
                                let _ = win.set_focus();
                            }
                        }
                        "show_overlay" => {
                            if let Some(win) = app.get_webview_window("quick-access") {
                                let _ = win.show();
                                let _ = win.set_focus();
                            }
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        button_state: tauri::tray::MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.unminimize();
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                    }
                });

            if let Some(icon) = app.default_window_icon() {
                tray_builder = tray_builder.icon(icon.clone());
            }

            if let Err(err) = tray_builder.build(app) {
                tracing::warn!("Impossible d'initialiser le tray icon : {}", err);
            } else {
                tracing::info!("System Tray initialisé avec succès.");
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
            // Fermeture complète et propre de l'application si la fenêtre principale est fermée
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    tracing::info!("Fermeture de la fenêtre principale reçue, arrêt ordonné de Jeanne.");
                    clean_exit(window.app_handle());
                } else if window.label() == "quick-access" {
                    api.prevent_close();
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
            storage: Arc::new(Mutex::new(storage)),
            vault_path: temp_dir.path().to_path_buf(),
            watcher: Mutex::new(None),
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

    #[tokio::test]
    async fn test_capture_quick_note_creates_and_indexes_journal() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let vault_path = temp_dir.path();
        let db_path = vault_path.join(".jeanne").join("database.db");
        let storage = StorageManager::open(&db_path).expect("open storage");
        storage.init_schema().expect("init schema");
        let storage_mutex = Mutex::new(storage);

        let note_path_str = capture_quick_note_core(
            vault_path,
            &storage_mutex,
            "/note Déploiement réussi du socle Jeanne",
        )
        .await
        .expect("capture note");

        let note_file = std::path::PathBuf::from(&note_path_str);
        assert!(note_file.exists(), "Le fichier journal doit avoir été créé");

        let content = std::fs::read_to_string(&note_file).expect("read journal");
        assert!(content.contains("Déploiement réussi du socle Jeanne"));
        assert!(content.contains("note_type: episodique"));

        // Vérifier l'indexation FTS5 immédiate
        let s = storage_mutex.lock().unwrap();
        let results = s.search_fts("Déploiement", 5).expect("search fts");
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].title,
            format!("Journal {}", chrono::Local::now().format("%Y-%m-%d"))
        );
    }

    #[test]
    fn test_shortcut_fallback_cascade_primary_success() {
        let candidates = ["Alt+Space", "Alt+Shift+Space", "Ctrl+Shift+Space"];
        let chosen = register_first_available_shortcut(&candidates, |cand| {
            if cand == "Alt+Space" {
                Ok(())
            } else {
                Err("Should not be called".into())
            }
        });
        assert_eq!(chosen, Some("Alt+Space".to_string()));
    }

    #[test]
    fn test_shortcut_fallback_cascade_secondary_success() {
        let candidates = ["Alt+Space", "Alt+Shift+Space", "Ctrl+Shift+Space"];
        let chosen = register_first_available_shortcut(&candidates, |cand| {
            if cand == "Alt+Space" {
                Err("Conflict with OS window menu".into())
            } else if cand == "Alt+Shift+Space" {
                Ok(())
            } else {
                Err("Should not be called".into())
            }
        });
        assert_eq!(chosen, Some("Alt+Shift+Space".to_string()));
    }

    #[test]
    fn test_shortcut_fallback_cascade_tertiary_success() {
        let candidates = ["Alt+Space", "Alt+Shift+Space", "Ctrl+Shift+Space"];
        let chosen = register_first_available_shortcut(&candidates, |cand| {
            if cand == "Ctrl+Shift+Space" {
                Ok(())
            } else {
                Err("Conflict".into())
            }
        });
        assert_eq!(chosen, Some("Ctrl+Shift+Space".to_string()));
    }

    #[test]
    fn test_shortcut_fallback_cascade_all_failed() {
        let candidates = ["Alt+Space", "Alt+Shift+Space", "Ctrl+Shift+Space"];
        let chosen = register_first_available_shortcut(&candidates, |_| {
            Err("All conflicts".into())
        });
        assert_eq!(chosen, None);
    }
}
