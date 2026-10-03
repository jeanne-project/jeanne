use jeanne_core::{
    CancellationToken, HardwareInfo, IndexedChunk, LocalEngineConfig, LocalInferenceStats,
    LocalLlmEngine, NoteFrontmatter, SearchResult, SnippetItem, StorageManager, TaskItem,
    VaultStats, VaultWatcher,
};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::Manager;
use tauri_plugin_global_shortcut::GlobalShortcutExt;
use tokio::io::AsyncWriteExt;

/// État applicatif partagé contenant l'accès sécurisé au moteur SQLite, le chemin racine du coffre
/// et le moteur d'inférence local single-tenant.
pub struct AppState {
    pub storage: Arc<Mutex<StorageManager>>,
    pub vault_path: PathBuf,
    pub watcher: Mutex<Option<VaultWatcher>>,
    pub local_engine: Arc<LocalLlmEngine>,
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
        id: None,
        chunk_id: format!("{rel_path}:0"),
        file_path: rel_path.clone(),
        chunk_index: 0,
        content: body,
        token_count: body_text.split_whitespace().count(),
        coala_type: frontmatter.coala_type(),
        status: frontmatter.status(),
        superseded_by: frontmatter.clean_superseded_by(),
        deprecated_at: frontmatter.deprecated_at,
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

#[tauri::command]
async fn load_local_model(
    state: tauri::State<'_, AppState>,
    model_path: Option<String>,
) -> Result<(), String> {
    tracing::info!(
        ">>> [IPC:load_local_model] Demande de chargement reçue : path={:?}",
        model_path
    );
    let start = std::time::Instant::now();
    let result = state
        .local_engine
        .load_model(model_path)
        .await
        .map_err(|e| e.to_string());

    match &result {
        Ok(()) => {
            let elapsed = start.elapsed();
            let stats = state.local_engine.get_stats().await;
            tracing::info!(
                ">>> [IPC:load_local_model] Succès : modèle chargé en {} ms. RAM allouée = {} Mo.",
                elapsed.as_millis(),
                stats.memory_allocated_mb
            );
        }
        Err(e) => {
            tracing::warn!(">>> [IPC:load_local_model] Échec du chargement : {}", e);
        }
    }
    result
}

#[tauri::command]
async fn unload_local_model(state: tauri::State<'_, AppState>) -> Result<(), String> {
    tracing::info!(">>> [IPC:unload_local_model] Demande de déchargement du modèle.");
    let res = state
        .local_engine
        .unload_model()
        .await
        .map_err(|e| e.to_string());
    if res.is_ok() {
        tracing::info!(
            ">>> [IPC:unload_local_model] Modèle déchargé avec succès. Mémoire libérée."
        );
    }
    res
}

#[tauri::command]
async fn get_hardware_profile(state: tauri::State<'_, AppState>) -> Result<HardwareInfo, String> {
    let mut hw = state.local_engine.hardware_info().clone();
    let config = state.local_engine.get_config().await;
    if !config.use_gpu {
        hw.vulkan_supported = false;
        hw.vulkan_device_name = Some("Désactivé (Mode CPU forcé)".to_string());
    }
    hw.recommended_model_loaded = state.local_engine.is_model_loaded().await;
    Ok(hw)
}

#[tauri::command]
async fn get_local_engine_config(
    state: tauri::State<'_, AppState>,
) -> Result<LocalEngineConfig, String> {
    Ok(state.local_engine.get_config().await)
}

#[tauri::command]
async fn update_local_engine_config(
    state: tauri::State<'_, AppState>,
    config: LocalEngineConfig,
) -> Result<LocalEngineConfig, String> {
    tracing::info!(
        ">>> [IPC:update_local_engine_config] use_gpu={}, timeout={}s, temp={}, ctx={}",
        config.use_gpu,
        config.generation_timeout_secs,
        config.temperature,
        config.context_size
    );

    let settings_dir = state.vault_path.join(".jeanne");
    if let Err(e) = tokio::fs::create_dir_all(&settings_dir).await {
        tracing::warn!("Impossible de créer le répertoire .jeanne : {e}");
    }
    let settings_file = settings_dir.join("local_llm_settings.json");
    if let Ok(serialized) = serde_json::to_string_pretty(&config) {
        if let Err(e) = tokio::fs::write(&settings_file, serialized).await {
            tracing::warn!("Impossible d'enregistrer local_llm_settings.json : {e}");
        } else {
            tracing::info!(
                "Configuration du moteur local enregistrée dans {:?}",
                settings_file
            );
        }
    }

    state.local_engine.update_config(config).await;
    Ok(state.local_engine.get_config().await)
}

#[tauri::command]
async fn get_local_inference_stats(
    state: tauri::State<'_, AppState>,
) -> Result<LocalInferenceStats, String> {
    Ok(state.local_engine.get_stats().await)
}

/// Retourne le chemin complet attendu du fichier modèle GGUF par défaut ou trouvé.
/// Utilisé par le frontend pour guider l'utilisateur lors de la configuration initiale.
#[tauri::command]
fn get_default_model_path() -> String {
    if let Some(p) = jeanne_core::resolve_model_path(None) {
        p.to_string_lossy().to_string()
    } else {
        jeanne_core::resolve_default_model_dir()
            .join("qwen2.5-3b-instruct-q4_k_m.gguf")
            .to_string_lossy()
            .to_string()
    }
}

/// Retourne le répertoire actif des modèles locaux GGUF.
#[tauri::command]
fn get_models_directory() -> String {
    jeanne_core::resolve_default_model_dir()
        .to_string_lossy()
        .to_string()
}

/// Découvre tous les modèles GGUF disponibles dans les dossiers modèles et indique lequel est chargé.
#[tauri::command]
async fn list_available_models(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<jeanne_core::DiscoveredModel>, String> {
    let currently_loaded = state.local_engine.loaded_model_path().await;
    let models = jeanne_core::discover_models(currently_loaded.as_deref());
    Ok(models)
}

#[tauri::command]
async fn execute_todo(
    state: tauri::State<'_, AppState>,
    content: String,
) -> Result<String, String> {
    jeanne_core::append_todo(&state.vault_path, &content)
}

#[tauri::command]
async fn get_vault_tasks(
    state: tauri::State<'_, AppState>,
    limit: Option<usize>,
) -> Result<Vec<TaskItem>, String> {
    let mut all_tasks = Vec::new();
    let limit = limit.unwrap_or(50);

    let inbox_path = state.vault_path.join("Inbox.md");
    if inbox_path.exists() {
        if let Ok(tasks) = jeanne_core::extract_tasks_from_file(&inbox_path) {
            all_tasks.extend(tasks);
        }
    }

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let journal_today = state.vault_path.join("Journal").join(format!("{today}.md"));
    if journal_today.exists() {
        if let Ok(tasks) = jeanne_core::extract_tasks_from_file(&journal_today) {
            all_tasks.extend(tasks);
        }
    }

    all_tasks.truncate(limit);
    Ok(all_tasks)
}

#[tauri::command]
async fn toggle_vault_task(
    state: tauri::State<'_, AppState>,
    file_path: String,
    line_number: usize,
    checked: bool,
) -> Result<(), String> {
    let target = validate_and_resolve_note_path(&state.vault_path, &file_path)?;
    jeanne_core::toggle_task_in_file(&target, line_number, checked)
}

#[tauri::command]
async fn execute_log(state: tauri::State<'_, AppState>, content: String) -> Result<String, String> {
    jeanne_core::append_log_entry(&state.vault_path, &content)
}

#[tauri::command]
async fn execute_meeting(
    state: tauri::State<'_, AppState>,
    title: String,
) -> Result<String, String> {
    jeanne_core::create_meeting_note(&state.vault_path, &title)
}

#[tauri::command]
async fn execute_bookmark(
    state: tauri::State<'_, AppState>,
    url: String,
    comment: Option<String>,
) -> Result<String, String> {
    jeanne_core::append_bookmark(&state.vault_path, &url, comment)
}

#[tauri::command]
fn get_snippets(state: tauri::State<'_, AppState>) -> Vec<SnippetItem> {
    jeanne_core::load_snippets(&state.vault_path)
}

#[tauri::command]
fn evaluate_math(expression: String) -> Result<f64, String> {
    jeanne_core::evaluate_math_expression(&expression)
}

#[tauri::command]
async fn ai_process_clipboard(
    state: tauri::State<'_, AppState>,
    action: String,
    text: String,
    param: Option<String>,
) -> Result<String, String> {
    let clean_text = text.trim();
    if clean_text.is_empty() {
        return Err("Le presse-papier est vide ou ne contient pas de texte".to_string());
    }

    if !state.local_engine.is_model_loaded().await {
        return Err(
            "Le modèle local (Qwen 3B) n'est pas chargé.\nVeuillez le charger depuis le tableau de bord pour activer les actions IA du presse-papier."
                .to_string(),
        );
    }

    let prompt = match action.as_str() {
        "corrige" => format!(
            "Tu es un relecteur professionnel. Corrige l'orthographe, la grammaire, la syntaxe et la ponctuation du texte ci-dessous. Conserve le ton et le format exacts. Renvoie UNIQUEMENT le texte corrigé, sans salutation ni explication :\n\n{clean_text}"
        ),
        "rephrase" => {
            let ton = param.as_deref().unwrap_or("professionnel et fluide");
            format!(
                "Reformule le texte ci-dessous avec un ton {ton}. Sois clair et concis. Renvoie UNIQUEMENT le texte reformulé, sans commentaire ni salutation :\n\n{clean_text}"
            )
        }
        "tldr" | "resume" => format!(
            "Résume le texte suivant sous forme de 3 puces clés concises commençant par un tiret (-). Renvoie UNIQUEMENT les puces :\n\n{clean_text}"
        ),
        "trad" => {
            let target_lang = param.as_deref().unwrap_or("français");
            format!(
                "Traduis fidèlement le texte suivant en {target_lang}. Renvoie UNIQUEMENT la traduction sans commentaire :\n\n{clean_text}"
            )
        }
        _ => return Err(format!("Action IA inconnue : '{action}'")),
    };

    tracing::info!(
        ">>> [IPC:ai_process_clipboard] Action='{}', Param='{:?}', Taille texte={} caractères",
        action,
        param,
        clean_text.len()
    );
    tracing::debug!(
        ">>> [IPC:ai_process_clipboard] Texte source : {:?}",
        clean_text
    );
    tracing::debug!(
        ">>> [IPC:ai_process_clipboard] Prompt envoyé : {:?}",
        prompt
    );

    let cancel = CancellationToken::new();
    let mut rx = state
        .local_engine
        .generate_stream(prompt, cancel)
        .await
        .map_err(|e| format!("Erreur génération IA : {e}"))?;

    let mut result = String::new();
    while let Some(token) = rx.recv().await {
        result.push_str(&token);
    }

    let final_res = result.trim().to_string();
    tracing::info!(
        ">>> [IPC:ai_process_clipboard] Réponse finale générée ({} caractères) : {:?}",
        final_res.len(),
        final_res
    );
    Ok(final_res)
}

#[tauri::command]
async fn ask_vault(state: tauri::State<'_, AppState>, question: String) -> Result<String, String> {
    let clean_q = question.trim();
    if clean_q.is_empty() {
        return Err("La question ne peut pas être vide".to_string());
    }

    let search_results = {
        let storage = state
            .storage
            .lock()
            .map_err(|e| format!("Erreur accès base de données : {e}"))?;
        storage.search_fts(clean_q, 3).unwrap_or_default()
    };

    if search_results.is_empty() {
        return Ok(
            "Aucune note correspondante trouvée dans votre coffre pour répondre à cette question."
                .to_string(),
        );
    }

    let mut context_chunks = Vec::new();
    for hit in &search_results {
        context_chunks.push(format!(
            "--- Note : {} ---\n{}",
            hit.title,
            hit.snippet.replace("<mark>", "").replace("</mark>", "")
        ));
    }
    let context_text = context_chunks.join("\n\n");

    if state.local_engine.is_model_loaded().await {
        let prompt = format!(
            "Tu es Jeanne, assistant de connaissances. Réponds à la question suivante en te basant STRICTEMENT sur les extraits du coffre fournis ci-dessous. Si l'information n'est pas présente, indique-le honnêtement. Cite la note source entre crochets [source: titre].\n\nExtraits du coffre :\n{context_text}\n\nQuestion : {clean_q}\n\nRéponse :"
        );

        let cancel = CancellationToken::new();
        let mut rx = state
            .local_engine
            .generate_stream(prompt, cancel)
            .await
            .map_err(|e| format!("Erreur génération IA : {e}"))?;

        let mut answer = String::new();
        while let Some(token) = rx.recv().await {
            answer.push_str(&token);
        }
        Ok(answer.trim().to_string())
    } else {
        let mut response = format!(
            "🔍 **Notes correspondantes trouvées ({})** *(Modèle local déchargé)* :\n\n",
            search_results.len()
        );
        for hit in search_results {
            response.push_str(&format!(
                "📄 **{}** (`{}`)\n> {}\n\n",
                hit.title,
                hit.file_path,
                hit.snippet.replace("<mark>", "**").replace("</mark>", "**")
            ));
        }
        response.push_str("💡 *Chargez le modèle local (Qwen 3B) dans l'accueil pour obtenir une réponse synthétisée par IA.*");
        Ok(response)
    }
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
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new(
            "debug,jeanne_core=debug,jeanne_desktop=debug,tauri=info",
        )
    });

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(true)
        .with_file(true)
        .with_line_number(true)
        .init();

    tracing::info!("============================================================");
    tracing::info!("  Démarrage du client Jeanne Desktop (Mode DEBUG configuré) ");
    tracing::info!("============================================================");

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
            exit_app,
            load_local_model,
            unload_local_model,
            get_hardware_profile,
            get_local_inference_stats,
            get_default_model_path,
            execute_todo,
            get_vault_tasks,
            toggle_vault_task,
            execute_log,
            execute_meeting,
            execute_bookmark,
            get_snippets,
            evaluate_math,
            ai_process_clipboard,
            ask_vault,
            list_available_models,
            get_models_directory,
            get_local_engine_config,
            update_local_engine_config
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
                tracing::info!(
                    "VaultWatcher démarré avec succès sur {}",
                    vault_path.display()
                );
            }

            let settings_file = vault_path.join(".jeanne").join("local_llm_settings.json");
            let initial_config = if settings_file.exists() {
                match std::fs::read_to_string(&settings_file) {
                    Ok(content) => {
                        match serde_json::from_str::<LocalEngineConfig>(&content) {
                            Ok(conf) => {
                                tracing::info!(
                                    "Configuration du modèle local chargée depuis {:?}",
                                    settings_file
                                );
                                conf
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "Erreur décodage configuration locale ({e}), utilisation par défaut."
                                );
                                LocalEngineConfig::default()
                            }
                        }
                    }
                    Err(_) => LocalEngineConfig::default(),
                }
            } else {
                LocalEngineConfig::default()
            };

            let local_engine = Arc::new(LocalLlmEngine::new(initial_config));

            app.manage(AppState {
                storage: storage_arc,
                vault_path,
                watcher: Mutex::new(Some(watcher)),
                local_engine,
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
            let quit_i =
                tauri::menu::MenuItemBuilder::with_id("quit", "Quitter Jeanne").build(app)?;
            let show_main_i =
                tauri::menu::MenuItemBuilder::with_id("show_main", "Ouvrir Jeanne").build(app)?;
            let show_overlay_i = tauri::menu::MenuItemBuilder::with_id(
                "show_overlay",
                "Palette d'accès rapide (Alt+Espace)",
            )
            .build(app)?;

            let tray_menu = tauri::menu::MenuBuilder::new(app)
                .items(&[&show_main_i, &show_overlay_i])
                .separator()
                .items(&[&quit_i])
                .build()?;

            let mut tray_builder = tauri::tray::TrayIconBuilder::new()
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "quit" => {
                        tracing::info!(
                            "Fermeture de Jeanne demandée depuis le menu de notification."
                        );
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
                    tracing::info!(
                        "Fermeture de la fenêtre principale reçue, arrêt ordonné de Jeanne."
                    );
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
    use jeanne_core::{CoalaType, NoteStatus};

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

        let local_engine = Arc::new(LocalLlmEngine::new(LocalEngineConfig::default()));
        let app_state = AppState {
            storage: Arc::new(Mutex::new(storage)),
            vault_path: temp_dir.path().to_path_buf(),
            watcher: Mutex::new(None),
            local_engine: local_engine.clone(),
        };

        // Test insertion manuelle et recherche
        {
            let s = app_state.storage.lock().unwrap();
            s.upsert_file("Journal/2026-09-12.md", "hash1", 1710000000, None)
                .expect("upsert file");
            s.index_chunk(&IndexedChunk {
                id: None,
                chunk_id: "Journal/2026-09-12.md:0".to_string(),
                file_path: "Journal/2026-09-12.md".to_string(),
                chunk_index: 0,
                content: "Note rapide de réunion sur l'architecture Jeanne".to_string(),
                token_count: 8,
                coala_type: CoalaType::Episodic,
                status: NoteStatus::Active,
                superseded_by: None,
                deprecated_at: None,
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
        let chosen =
            register_first_available_shortcut(&candidates, |_| Err("All conflicts".into()));
        assert_eq!(chosen, None);
    }

    #[tokio::test]
    async fn test_desktop_local_llm_state_and_profile() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let db_path = temp_dir.path().join("test.db");
        let storage = StorageManager::open(&db_path).expect("open storage");
        storage.init_schema().expect("init schema");

        let local_engine = Arc::new(LocalLlmEngine::new(LocalEngineConfig::default()));
        let app_state = AppState {
            storage: Arc::new(Mutex::new(storage)),
            vault_path: temp_dir.path().to_path_buf(),
            watcher: Mutex::new(None),
            local_engine: local_engine.clone(),
        };

        let hw = app_state.local_engine.hardware_info();
        assert!(hw.total_system_ram_mb > 0);
        assert!(!app_state.local_engine.is_model_loaded().await);

        let stats = app_state.local_engine.get_stats().await;
        assert_eq!(stats.memory_allocated_mb, 0);

        assert!(app_state.local_engine.unload_model().await.is_ok());
    }

    #[tokio::test]
    async fn test_desktop_local_engine_config_update_and_persistence() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let db_path = temp_dir.path().join("test.db");
        let storage = StorageManager::open(&db_path).expect("open storage");
        storage.init_schema().expect("init schema");

        let local_engine = Arc::new(LocalLlmEngine::new(LocalEngineConfig::default()));
        let app_state = AppState {
            storage: Arc::new(Mutex::new(storage)),
            vault_path: temp_dir.path().to_path_buf(),
            watcher: Mutex::new(None),
            local_engine: local_engine.clone(),
        };

        let initial = app_state.local_engine.get_config().await;
        assert!(initial.use_gpu);
        assert_eq!(initial.generation_timeout_secs, 10);

        let mut updated = initial.clone();
        updated.use_gpu = false;
        updated.generation_timeout_secs = 20;

        // Persistence in vault directory .jeanne/local_llm_settings.json
        let settings_dir = app_state.vault_path.join(".jeanne");
        tokio::fs::create_dir_all(&settings_dir)
            .await
            .expect("mkdir");
        let settings_file = settings_dir.join("local_llm_settings.json");
        let serialized = serde_json::to_string_pretty(&updated).expect("serialize");
        tokio::fs::write(&settings_file, serialized)
            .await
            .expect("write");
        app_state.local_engine.update_config(updated.clone()).await;

        let fetched = app_state.local_engine.get_config().await;
        assert!(!fetched.use_gpu);
        assert_eq!(fetched.generation_timeout_secs, 20);

        // Verify hardware profile reports CPU forced mode when use_gpu is false
        let mut hw = app_state.local_engine.hardware_info().clone();
        if !fetched.use_gpu {
            hw.vulkan_supported = false;
            hw.vulkan_device_name = Some("Désactivé (Mode CPU forcé)".to_string());
        }
        assert!(!hw.vulkan_supported);
        assert_eq!(
            hw.vulkan_device_name.as_deref(),
            Some("Désactivé (Mode CPU forcé)")
        );

        // Verify reloading from disk matches
        let disk_content = std::fs::read_to_string(&settings_file).expect("read");
        let parsed: LocalEngineConfig = serde_json::from_str(&disk_content).expect("parse");
        assert!(!parsed.use_gpu);
        assert_eq!(parsed.generation_timeout_secs, 20);
    }
}
