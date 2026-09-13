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

use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};

use crate::error::JeanneError;
use crate::storage::StorageManager;

/// Observateur de système de fichiers pour la synchronisation du coffre en temps réel.
pub struct VaultWatcher {
    vault_path: PathBuf,
    storage: Arc<Mutex<StorageManager>>,
    debounce_duration: Duration,
    reconciliation_count: Arc<AtomicUsize>,
    watcher: Option<RecommendedWatcher>,
    task_handle: Option<tokio::task::JoinHandle<()>>,
    stop_tx: Option<tokio::sync::oneshot::Sender<()>>,
    runtime: Option<tokio::runtime::Runtime>,
}

impl VaultWatcher {
    /// Initialise un nouvel observateur pour le chemin de coffre spécifié et le stockage SQLite.
    pub fn new<P: AsRef<Path>>(
        vault_path: P,
        storage: Arc<Mutex<StorageManager>>,
    ) -> Result<Self> {
        Ok(Self {
            vault_path: vault_path.as_ref().to_path_buf(),
            storage,
            debounce_duration: Duration::from_millis(300),
            reconciliation_count: Arc::new(AtomicUsize::new(0)),
            watcher: None,
            task_handle: None,
            stop_tx: None,
            runtime: None,
        })
    }

    /// Retourne le chemin racine du coffre surveillé.
    pub fn vault_path(&self) -> &Path {
        &self.vault_path
    }

    /// Retourne une référence vers le gestionnaire de stockage partagé.
    pub fn storage(&self) -> &Arc<Mutex<StorageManager>> {
        &self.storage
    }

    /// Configure la durée de la fenêtre glissante de dé-rebond.
    pub fn with_debounce_duration(mut self, duration: Duration) -> Self {
        self.debounce_duration = duration;
        self
    }

    /// Retourne la durée de la fenêtre glissante de dé-rebond.
    pub fn debounce_duration(&self) -> Duration {
        self.debounce_duration
    }

    /// Retourne le nombre de transactions de réconciliation exécutées.
    pub fn reconciliation_count(&self) -> usize {
        self.reconciliation_count.load(Ordering::SeqCst)
    }

    /// Démarre la boucle de surveillance asynchrone des événements de fichiers.
    pub fn start(&mut self) -> Result<()> {
        if self.task_handle.is_some() {
            return Ok(());
        }

        let (event_tx, mut event_rx) =
            tokio::sync::mpsc::unbounded_channel::<notify::Result<notify::Event>>();
        let mut watcher = RecommendedWatcher::new(
            move |res| {
                let _ = event_tx.send(res);
            },
            Config::default(),
        )?;
        watcher.watch(&self.vault_path, RecursiveMode::Recursive)?;

        let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel::<()>();
        let vault_path = self.vault_path.clone();
        let storage = self.storage.clone();
        let debounce_duration = self.debounce_duration;
        let reconciliation_count = self.reconciliation_count.clone();

        let debouncer_task = async move {
            let mut pending_paths: HashSet<PathBuf> = HashSet::new();
            let sleep = tokio::time::sleep(debounce_duration);
            tokio::pin!(sleep);
            let mut timer_active = false;

            loop {
                tokio::select! {
                    _ = &mut stop_rx => {
                        break;
                    }
                    maybe_event = event_rx.recv() => {
                        match maybe_event {
                            Some(Ok(event)) => {
                                let mut has_relevant = false;
                                for path in event.paths {
                                    // Ignorer les dossiers ou fichiers cachés internes au coffre (.jeanne, etc.)
                                    let is_hidden = if let Ok(rel) = path.strip_prefix(&vault_path) {
                                        rel.components().any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
                                    } else {
                                        path.file_name().map(|n| n.to_string_lossy().starts_with('.')).unwrap_or(false)
                                    };
                                    if is_hidden {
                                        continue;
                                    }

                                    let is_md = path.extension()
                                        .and_then(|e| e.to_str())
                                        .map(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"))
                                        .unwrap_or(false);
                                    if is_md {
                                        pending_paths.insert(path);
                                        has_relevant = true;
                                    }
                                }
                                if has_relevant {
                                    sleep.as_mut().reset(tokio::time::Instant::now() + debounce_duration);
                                    timer_active = true;
                                }
                            }
                            Some(Err(err)) => {
                                tracing::warn!("Erreur notify dans VaultWatcher : {}", err);
                            }
                            None => {
                                break;
                            }
                        }
                    }
                    _ = &mut sleep, if timer_active => {
                        timer_active = false;
                        if !pending_paths.is_empty() {
                            let batch = std::mem::take(&mut pending_paths);
                            if let Err(err) = reconcile_batch(&vault_path, &storage, batch).await {
                                tracing::error!("Erreur lors de la réconciliation VaultWatcher : {}", err);
                            }
                            reconciliation_count.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                }
            }
        };

        let (task_handle, runtime) = if let Ok(handle) = tokio::runtime::Handle::try_current() {
            (handle.spawn(debouncer_task), None)
        } else {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()?;
            let handle = rt.spawn(debouncer_task);
            (handle, Some(rt))
        };

        self.watcher = Some(watcher);
        self.task_handle = Some(task_handle);
        self.stop_tx = Some(stop_tx);
        self.runtime = runtime;

        Ok(())
    }

    /// Arrête la surveillance asynchrone des événements de fichiers.
    pub fn stop(&mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.task_handle.take() {
            handle.abort();
        }
        self.watcher = None;
        if let Some(rt) = self.runtime.take() {
            rt.shutdown_background();
        }
    }
}

impl Drop for VaultWatcher {
    fn drop(&mut self) {
        self.stop();
    }
}

async fn reconcile_batch(
    vault_path: &Path,
    storage_mutex: &Arc<Mutex<StorageManager>>,
    paths: HashSet<PathBuf>,
) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }

    struct FileItem {
        rel_path: String,
        content: String,
        hash: String,
        now: i64,
    }

    let mut to_upsert = Vec::new();
    let mut to_delete = Vec::new();

    for path in paths {
        let Some(rel_path) = normalize_rel_path(vault_path, &path) else {
            continue;
        };

        if path.is_file() {
            if let Ok(content) = tokio::fs::read_to_string(&path).await {
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                content.hash(&mut hasher);
                let hash = format!("{:016x}", hasher.finish());
                let now = chrono::Local::now().timestamp();
                to_upsert.push(FileItem {
                    rel_path,
                    content,
                    hash,
                    now,
                });
            }
        } else if !path.exists() {
            to_delete.push(rel_path);
        }
    }

    if to_upsert.is_empty() && to_delete.is_empty() {
        return Ok(());
    }

    let storage = storage_mutex
        .lock()
        .map_err(|e| JeanneError::Vault(format!("Verrouillage storage échoué: {e}")))?;

    let conn = storage.raw_connection();
    conn.execute_batch("BEGIN IMMEDIATE;")?;

    let mut tx_result: Result<()> = Ok(());

    for del in to_delete {
        if let Err(e) = storage.delete_file(&del) {
            tx_result = Err(e);
            break;
        }
    }

    if tx_result.is_ok() {
        for item in to_upsert {
            let (frontmatter, body) = crate::parser::parse_markdown(&item.content)
                .unwrap_or_else(|_| (crate::models::NoteFrontmatter::default(), item.content.clone()));
            let frontmatter_json = serde_json::to_string(&frontmatter).ok();

            if let Err(e) = storage.upsert_file(
                &item.rel_path,
                &item.hash,
                item.now,
                frontmatter_json.as_deref(),
            ) {
                tx_result = Err(e);
                break;
            }

            let token_count = body.split_whitespace().count();
            let chunk = crate::models::IndexedChunk {
                chunk_id: format!("{}:0", item.rel_path),
                file_path: item.rel_path.clone(),
                chunk_index: 0,
                content: body,
                token_count,
                note_type: frontmatter.note_type,
                statut: frontmatter.statut,
                date_creation: item.now,
            };

            if let Err(e) = storage.index_chunk(&chunk) {
                tx_result = Err(e);
                break;
            }
        }
    }

    if tx_result.is_ok() {
        conn.execute_batch("COMMIT;")?;
    } else {
        let _ = conn.execute_batch("ROLLBACK;");
    }

    tx_result
}

fn normalize_rel_path(vault_path: &Path, file_path: &Path) -> Option<String> {
    if let Ok(rel) = file_path.strip_prefix(vault_path) {
        let s = rel.to_string_lossy().replace('\\', "/");
        let trimmed = s.trim_start_matches('/').to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }

    let canon_vault = vault_path.canonicalize().ok();
    let canon_file = file_path.canonicalize().ok().or_else(|| {
        let parent = file_path.parent()?;
        let canon_parent = parent.canonicalize().ok()?;
        let file_name = file_path.file_name()?;
        Some(canon_parent.join(file_name))
    });

    if let (Some(v), Some(f)) = (canon_vault, canon_file) {
        if let Ok(rel) = f.strip_prefix(&v) {
            let s = rel.to_string_lossy().replace('\\', "/");
            let trimmed = s.trim_start_matches('/').to_string();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }

    let vault_comps: Vec<_> = vault_path.components().collect();
    let file_comps: Vec<_> = file_path.components().collect();

    if file_comps.len() > vault_comps.len() {
        let matches = file_comps.iter().zip(vault_comps.iter()).all(|(fc, vc)| {
            fc.as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case(&vc.as_os_str().to_string_lossy())
        });
        if matches {
            let rel_parts: Vec<_> = file_comps[vault_comps.len()..]
                .iter()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect();
            let joined = rel_parts.join("/");
            if !joined.is_empty() {
                return Some(joined);
            }
        }
    }

    file_path.file_name().map(|n| n.to_string_lossy().to_string())
}


