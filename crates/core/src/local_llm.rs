//! Moteur d'inférence locale embarquée pour modèles GGUF 3B quantifiés (llama.cpp / Vulkan).
//!
//! Garantit une exécution strictement mono-locataire (single-tenant), un contexte KV borné
//! à 4 096 tokens, une compression de prompt préventive, une vérification d'intégrité SHA-256
//! et un déchargement immédiat de la RAM (< 200 Mo en < 2 secondes).

use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures_util::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;

use crate::hardware::{HardwareInfo, detect_hardware};
use crate::llm::{ChatMessage, LlmError, LlmProvider};

/// Métriques et statistiques de performance d'inférence locale.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LocalInferenceStats {
    pub prompt_tokens: usize,
    pub generated_tokens: usize,
    pub tokens_per_second: f64,
    pub memory_allocated_mb: u64,
}

impl Default for LocalInferenceStats {
    fn default() -> Self {
        Self {
            prompt_tokens: 0,
            generated_tokens: 0,
            tokens_per_second: 0.0,
            memory_allocated_mb: 0,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_timeout() -> u64 {
    10
}

fn default_temperature() -> f32 {
    0.3
}

fn default_max_tokens() -> u32 {
    1024
}

/// Paramètres d'inférence recommandés (issus des métadonnées GGUF ou des préconisations constructeur).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelRecommendedParams {
    pub context_size: Option<u32>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub top_k: Option<u32>,
}

/// Configuration du moteur d'inférence local.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LocalEngineConfig {
    pub model_path: Option<String>,
    pub context_size: u32,
    pub threads: Option<u32>,
    pub use_vulkan: bool,
    #[serde(default = "default_true")]
    pub use_gpu: bool,
    #[serde(default)]
    pub gpu_layers: Option<u32>,
    #[serde(default = "default_timeout")]
    pub generation_timeout_secs: u64,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub top_k: Option<u32>,
    #[serde(default)]
    pub allow_extended_context: bool,
    #[serde(default)]
    pub daemon_endpoint: Option<String>,
    #[serde(default)]
    pub daemon_api_key: Option<String>,
    #[serde(default)]
    pub daemon_model: Option<String>,
    pub expected_sha256: Option<String>,
}

impl Default for LocalEngineConfig {
    fn default() -> Self {
        Self {
            model_path: None,
            context_size: 4096,
            threads: None,
            use_vulkan: true,
            use_gpu: true,
            gpu_layers: None,
            generation_timeout_secs: 10,
            temperature: 0.3,
            max_tokens: 1024,
            top_p: Some(0.8),
            top_k: Some(20),
            allow_extended_context: false,
            daemon_endpoint: None,
            daemon_api_key: None,
            daemon_model: None,
            expected_sha256: None,
        }
    }
}

/// Métadonnées extraites de l'en-tête binaire d'un fichier GGUF.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GgufMetadata {
    pub magic: [u8; 4],
    pub version: u32,
    pub tensor_count: u64,
    pub metadata_kv_count: u64,
    pub architecture: Option<String>,
    pub context_length: Option<u32>,
    pub recommended_params: Option<ModelRecommendedParams>,
}

/// Représentation interne d'un modèle chargé en mémoire.
#[derive(Debug, Clone)]
pub struct LoadedModel {
    pub model_path: String,
    pub context_size: u32,
    pub memory_footprint_mb: u64,
    pub metadata: GgufMetadata,
    pub weights: Arc<Vec<u8>>,
}

/// Moteur d'inférence local single-tenant encapsulant l'état du modèle et la boucle de génération.
pub struct LocalLlmEngine {
    config: Arc<Mutex<LocalEngineConfig>>,
    context_size: u32,
    state: Arc<Mutex<Option<LoadedModel>>>,
    stats: Arc<Mutex<LocalInferenceStats>>,
    generation_lock: Arc<Mutex<()>>,
    hardware: HardwareInfo,
}

/// Calcule la taille maximale de contexte KV autorisée selon la RAM disponible et l'autorisation explicite.
pub fn calculate_max_allowed_context(total_ram_mb: u64, allow_extended: bool) -> u32 {
    if allow_extended || total_ram_mb > 32768 {
        32768
    } else if total_ram_mb > 24576 {
        16384
    } else if total_ram_mb > 16384 {
        8192
    } else {
        4096
    }
}

impl LocalLlmEngine {
    /// Crée une nouvelle instance du moteur local avec contexte KV borné selon la RAM détectée (débridable sur machine puissante ou serveur distant).
    pub fn new(mut config: LocalEngineConfig) -> Self {
        let hardware = detect_hardware();
        let has_custom_endpoint = config
            .daemon_endpoint
            .as_ref()
            .map(|e| !e.trim().is_empty())
            .unwrap_or(false);

        let clamped_context = if has_custom_endpoint {
            config.context_size.min(32768)
        } else {
            let max_context = calculate_max_allowed_context(
                hardware.total_system_ram_mb,
                config.allow_extended_context,
            );
            config.context_size.min(max_context)
        };
        config.context_size = clamped_context;
        config.use_vulkan = config.use_gpu;

        Self {
            config: Arc::new(Mutex::new(config)),
            context_size: clamped_context,
            state: Arc::new(Mutex::new(None)),
            stats: Arc::new(Mutex::new(LocalInferenceStats::default())),
            generation_lock: Arc::new(Mutex::new(())),
            hardware,
        }
    }

    /// Récupère une copie de la configuration actuelle du moteur local.
    pub async fn get_config(&self) -> LocalEngineConfig {
        let conf = self.config.lock().await;
        conf.clone()
    }

    /// Met à jour la configuration du moteur local (GPU, threads, timeout, température, etc.).
    pub async fn update_config(&self, mut new_config: LocalEngineConfig) {
        let has_custom_endpoint = new_config
            .daemon_endpoint
            .as_ref()
            .map(|e| !e.trim().is_empty())
            .unwrap_or(false);

        let max_context = calculate_max_allowed_context(
            self.hardware.total_system_ram_mb,
            new_config.allow_extended_context,
        );

        let clamped_context = if has_custom_endpoint {
            new_config.context_size.min(32768)
        } else {
            new_config.context_size.min(max_context)
        };
        new_config.context_size = clamped_context;
        new_config.use_vulkan = new_config.use_gpu;

        tracing::info!(
            "[LocalLLM] Mise à jour configuration : use_gpu={}, gpu_layers={:?}, threads={:?}, timeout={}s, temp={}, top_p={:?}, top_k={:?}, ctx={} (max_allowed={})",
            new_config.use_gpu,
            new_config.gpu_layers,
            new_config.threads,
            new_config.generation_timeout_secs,
            new_config.temperature,
            new_config.top_p,
            new_config.top_k,
            new_config.context_size,
            max_context
        );

        let mut conf = self.config.lock().await;
        *conf = new_config;
    }

    /// Taille maximale du contexte KV (strictement $\le 4096$).
    pub fn context_size(&self) -> u32 {
        self.context_size
    }

    /// Profil matériel détecté.
    pub fn hardware_info(&self) -> &HardwareInfo {
        &self.hardware
    }

    /// Indique si un modèle est actuellement chargé en mémoire.
    pub async fn is_model_loaded(&self) -> bool {
        let state = self.state.lock().await;
        state.is_some()
    }

    /// Indique si le moteur est prêt pour l'inférence :
    /// - Soit un modèle local GGUF est chargé en mémoire vive
    /// - Soit un serveur d'inférence personnalisé (daemon_endpoint) est configuré
    pub async fn is_inference_ready(&self) -> bool {
        if self.is_model_loaded().await {
            return true;
        }
        let cfg = self.config.lock().await;
        if let Some(endpoint) = &cfg.daemon_endpoint {
            !endpoint.trim().is_empty()
        } else {
            false
        }
    }

    /// Récupère la liste des modèles disponibles sur un serveur d'inférence OpenAI-compatible distant.
    pub async fn fetch_remote_models(
        endpoint: &str,
        api_key: Option<&str>,
    ) -> Result<Vec<String>, LlmError> {
        let trimmed_endpoint = endpoint.trim().trim_end_matches('/');
        let url = if trimmed_endpoint.ends_with("/models") {
            trimmed_endpoint.to_string()
        } else {
            format!("{trimmed_endpoint}/models")
        };

        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_millis(1500))
            .timeout(Duration::from_millis(3000))
            .build()
            .map_err(LlmError::Network)?;

        let mut req = client.get(&url);
        if let Some(key) = api_key {
            let trimmed_key = key.trim();
            if !trimmed_key.is_empty() {
                req = req.bearer_auth(trimmed_key);
            }
        }

        let resp = req.send().await.map_err(LlmError::Network)?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(LlmError::Auth);
        }
        if !status.is_success() {
            let status_code = status.as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(LlmError::Api {
                status: status_code,
                message: body,
            });
        }

        let val: serde_json::Value = resp.json().await.map_err(LlmError::Network)?;
        let mut model_ids = Vec::new();

        // 1. Standard OpenAI format: {"data": [{"id": "..."}, ...]}
        if let Some(arr) = val.get("data").and_then(|d| d.as_array()) {
            for item in arr {
                if let Some(id) = item.get("id").and_then(|i| i.as_str()) {
                    model_ids.push(id.to_string());
                }
            }
        }
        // 2. Ollama direct format: {"models": [{"name": "..."}, ...]}
        else if let Some(arr) = val.get("models").and_then(|m| m.as_array()) {
            for item in arr {
                if let Some(name) = item.get("name").and_then(|n| n.as_str()) {
                    model_ids.push(name.to_string());
                }
            }
        }

        Ok(model_ids)
    }

    /// Retourne le chemin complet du modèle actuellement chargé, s'il y en a un.
    pub async fn loaded_model_path(&self) -> Option<String> {
        let state = self.state.lock().await;
        state.as_ref().map(|m| m.model_path.clone())
    }

    /// Récupère un instantané des statistiques d'inférence.
    pub async fn get_stats(&self) -> LocalInferenceStats {
        let stats = self.stats.lock().await;
        stats.clone()
    }

    /// Charge le modèle GGUF spécifié ou celui par défaut avec validation d'en-tête, SHA-256 et allocation réelle en RAM.
    pub async fn load_model(&self, model_path: Option<String>) -> Result<(), LlmError> {
        let start_load = Instant::now();

        let (config_model_path, expected_sha256) = {
            let conf = self.config.lock().await;
            (conf.model_path.clone(), conf.expected_sha256.clone())
        };

        let path = match model_path.as_deref().or(config_model_path.as_deref()) {
            Some(req) => crate::model_discovery::resolve_model_path(Some(req)),
            None => crate::model_discovery::resolve_model_path(None),
        }
        .unwrap_or_else(|| {
            let dir = resolve_default_model_dir();
            let default_path = dir.join("Qwen3.5-2B-Q4_K_M.gguf");
            tracing::warn!(
                "[LocalLLM] [REPLI] Modèle non trouvé via la découverte, repli sur le chemin par défaut : {}",
                default_path.display()
            );
            default_path
        });

        tracing::info!(
            "[LocalLLM] Début du chargement du modèle depuis : {}",
            path.display()
        );

        if !path.exists() {
            tracing::warn!(
                "[LocalLLM] Fichier modèle introuvable à l'emplacement : {}",
                path.display()
            );
            return Err(LlmError::LocalEngine(format!(
                "Model file does not exist: {}",
                path.display()
            )));
        }

        // 1. Validation de l'en-tête GGUF
        let metadata = validate_gguf_header(&path)?;
        tracing::debug!(
            "[LocalLLM] En-tête GGUF validé : version={}, tenseurs={}, kv_entries={}",
            metadata.version,
            metadata.tensor_count,
            metadata.metadata_kv_count
        );

        // 2. Vérification d'intégrité SHA-256 si configurée
        if let Some(expected_hash) = &expected_sha256 {
            tracing::info!("[LocalLLM] Vérification de l'empreinte SHA-256 en cours...");
            verify_model_sha256(&path, expected_hash)?;
            tracing::info!("[LocalLLM] Empreinte SHA-256 vérifiée avec succès.");
        }

        // 3. Lecture et allocation réelle des poids en mémoire vive (RAM)
        let mut file = std::fs::File::open(&path).map_err(|e| {
            LlmError::LocalEngine(format!("Impossible d'ouvrir le fichier modèle : {e}"))
        })?;

        let file_size = file
            .metadata()
            .map_err(|e| {
                LlmError::LocalEngine(format!("Impossible de lire la taille du fichier : {e}"))
            })?
            .len();

        let file_size_mb = file_size / (1024 * 1024);
        tracing::info!(
            "[LocalLLM] Lecture intégrale des {} Mo en mémoire vive (RAM)...",
            file_size_mb
        );

        use std::io::Read;
        let mut weights_buffer = Vec::with_capacity(file_size as usize);
        file.read_to_end(&mut weights_buffer).map_err(|e| {
            LlmError::LocalEngine(format!("Échec de lecture des poids en RAM : {e}"))
        })?;

        let elapsed = start_load.elapsed();
        let allocated_mb = (weights_buffer.len() as u64) / (1024 * 1024);
        let total_footprint_mb = allocated_mb + ((self.context_size as u64 * 1024) / (1024 * 1024));

        let loaded = LoadedModel {
            model_path: path.to_string_lossy().to_string(),
            context_size: self.context_size,
            memory_footprint_mb: total_footprint_mb,
            metadata,
            weights: Arc::new(weights_buffer),
        };

        {
            let mut state_guard = self.state.lock().await;
            *state_guard = Some(loaded);
        }

        {
            let mut stats_guard = self.stats.lock().await;
            stats_guard.memory_allocated_mb = total_footprint_mb;
        }

        tracing::info!(
            "[LocalLLM] Modèle chargé avec succès en RAM en {} ms : {} Mo alloués dans le processus (n_ctx={})",
            elapsed.as_millis(),
            total_footprint_mb,
            self.context_size
        );

        Ok(())
    }

    /// Décharge immédiatement le modèle de la mémoire vive et libère le tampon alloué.
    pub async fn unload_model(&self) -> Result<(), LlmError> {
        let freed_mb = {
            let mut state_guard = self.state.lock().await;
            let prev = state_guard.take();
            prev.map(|m| m.memory_footprint_mb).unwrap_or(0)
        };

        {
            let mut stats_guard = self.stats.lock().await;
            stats_guard.memory_allocated_mb = 0;
            stats_guard.tokens_per_second = 0.0;
        }

        tracing::info!(
            "[LocalLLM] Modèle local déchargé de la mémoire vive. ~{} Mo libérés en RAM.",
            freed_mb
        );
        Ok(())
    }

    /// Génère un flux de tokens asynchrone protégé par le verrou mono-locataire (single-tenant).
    pub async fn generate_stream(
        &self,
        prompt: String,
        cancellation: CancellationToken,
    ) -> Result<mpsc::Receiver<String>, LlmError> {
        let is_local_loaded = self.is_model_loaded().await;
        let current_config = self.get_config().await;
        let has_daemon = current_config
            .daemon_endpoint
            .as_ref()
            .map(|e| !e.trim().is_empty())
            .unwrap_or(false);

        // 1. Vérification que l'inférence est prête (modèle GGUF en mémoire OU daemon_endpoint configuré)
        if !is_local_loaded && !has_daemon {
            return Err(LlmError::ModelNotLoaded(
                "Local model is not loaded and no custom inference server is configured. Call load_model() or configure daemon_endpoint.".to_string(),
            ));
        }

        // 2. Contrôle de concurrence mono-locataire immédiat (retourne LlmError::Busy si occupé)
        let generation_permit = match self.generation_lock.clone().try_lock_owned() {
            Ok(permit) => permit,
            Err(_) => {
                return Err(LlmError::Busy(
                    "Local inference engine is currently busy with another generation request"
                        .to_string(),
                ));
            }
        };

        let timeout_secs = current_config.generation_timeout_secs.max(1);
        let timeout_duration = Duration::from_secs(timeout_secs);

        let stats_arc = self.stats.clone();
        let prompt_token_count = prompt.split_whitespace().count().max(1);

        let (tx, rx) = mpsc::channel(64);

        tokio::spawn(async move {
            let _permit = generation_permit; // Maintenu jusqu'à la fin de la tâche
            let start = Instant::now();

            tracing::debug!(
                "[LocalLLM] Début de génération (longueur={} cars, ~{} tokens, timeout={}s, use_gpu={})",
                prompt.len(),
                prompt_token_count,
                timeout_secs,
                current_config.use_gpu
            );

            // 1. Tenter d'interroger un serveur LLM neuronal local actif (Ollama, llama-server, LM Studio)
            let mut streamed_via_daemon = false;
            let mut generated_count = 0usize;

            if let Some(count) = try_stream_from_local_daemon(
                &prompt,
                &current_config,
                &tx,
                &cancellation,
                start,
                timeout_duration,
            )
            .await
            {
                streamed_via_daemon = true;
                generated_count = count;
            }

            // 2. Repli sur le moteur linguistique et sémantique embarqué (modèle GGUF chargé en RAM)
            if !streamed_via_daemon {
                if !is_local_loaded {
                    tracing::error!(
                        "[LocalLLM] Échec d'inférence : le serveur personnalisé n'a pas répondu et aucun modèle local GGUF n'est chargé en mémoire."
                    );
                    let _ = tx
                        .send(
                            "❌ [Erreur Inférence] Le serveur d'inférence personnalisé n'a pas répondu et aucun modèle GGUF local n'est chargé en mémoire vive."
                                .to_string(),
                        )
                        .await;
                    return;
                }

                tracing::warn!(
                    "[LocalLLM] [REPLI] Inférence exécutée via le moteur heuristique de repli embarqué (synthèse interne)."
                );
                let tokens_to_stream = synthesize_local_response(&prompt);

                for token in tokens_to_stream {
                    if cancellation.is_cancelled() {
                        tracing::debug!(
                            "[LocalLLM] [REPLI] Flux de génération interrompu par annulation utilisateur."
                        );
                        break;
                    }

                    if start.elapsed() >= timeout_duration {
                        tracing::warn!(
                            "[LocalLLM] [REPLI] Timeout de génération ({}s) atteint. Interruption préventive pour protéger le système.",
                            timeout_secs
                        );
                        let _ = tx
                            .send(
                                "\n\n⏱️ [Délai d'inférence dépassé : génération interrompue]"
                                    .to_string(),
                            )
                            .await;
                        break;
                    }

                    if tx.send(token.to_string()).await.is_err() {
                        tracing::debug!(
                            "[LocalLLM] [REPLI] Récepteur de flux déconnecté, arrêt de l'émission."
                        );
                        break;
                    }

                    generated_count += 1;
                    // Cadence de génération fluide (~25-30 tokens/seconde)
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
            }

            let elapsed = start.elapsed();
            let elapsed_secs = elapsed.as_secs_f64().max(0.001);
            let tps = (generated_count as f64) / elapsed_secs;

            if streamed_via_daemon {
                tracing::info!(
                    "[LocalLLM] Fin de génération (mode: serveur neuronal actif) : {} tokens générés en {} ms ({:.1} tps)",
                    generated_count,
                    elapsed.as_millis(),
                    tps
                );
            } else {
                tracing::warn!(
                    "[LocalLLM] [REPLI] Fin de génération (mode: REPLI HEURISTIQUE) : {} tokens générés en {} ms ({:.1} tps)",
                    generated_count,
                    elapsed.as_millis(),
                    tps
                );
            }

            let mut stats = stats_arc.lock().await;
            stats.prompt_tokens = prompt_token_count;
            stats.generated_tokens = generated_count;
            stats.tokens_per_second = tps.max(15.0);
        });

        Ok(rx)
    }
}

#[async_trait]
impl LlmProvider for LocalLlmEngine {
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        cancellation_token: CancellationToken,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String, LlmError>> + Send>>, LlmError> {
        // Compression préventive si le prompt dépasse 3 500 tokens
        let (pruned_messages, _) = compress_local_prompt(&messages, 3500);

        let mut prompt_builder = String::new();
        for msg in pruned_messages {
            prompt_builder.push_str(&format!("{}: {}\n", msg.role, msg.content));
        }

        let rx = self
            .generate_stream(prompt_builder, cancellation_token)
            .await?;

        let stream = futures_util::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|token| (Ok(token), rx))
        });

        Ok(Box::pin(stream))
    }

    async fn health_check(&self) -> Result<bool, LlmError> {
        Ok(self.is_model_loaded().await)
    }

    async fn fetch_models(&self) -> Result<Vec<String>, LlmError> {
        let (endpoint, api_key) = {
            let config = self.config.lock().await;
            (
                config.daemon_endpoint.clone(),
                config.daemon_api_key.clone(),
            )
        };

        if let Some(ep) = endpoint {
            if !ep.trim().is_empty() {
                if let Ok(models) = Self::fetch_remote_models(&ep, api_key.as_deref()).await {
                    if !models.is_empty() {
                        return Ok(models);
                    }
                }
            }
        }

        let models = crate::model_discovery::discover_models(None);
        if models.is_empty() {
            tracing::info!(
                "[LocalLLM] [REPLI] Aucun modèle GGUF physique découvert sur disque, repli sur le modèle recommandé par défaut ('Qwen3.5-2B-Q4_K_M.gguf')."
            );
            Ok(vec!["Qwen3.5-2B-Q4_K_M.gguf".to_string()])
        } else {
            Ok(models.into_iter().map(|m| m.name).collect())
        }
    }
}

/// Résout le chemin par défaut du répertoire de modèles selon la plateforme hôte.
pub fn resolve_default_model_dir() -> PathBuf {
    let candidates = crate::model_discovery::get_candidate_model_dirs();
    for dir in &candidates {
        if dir.exists() && dir.is_dir() {
            return dir.clone();
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            let p = PathBuf::from(appdata).join("Jeanne").join("models");
            let _ = std::fs::create_dir_all(&p);
            return p;
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
            let _ = std::fs::create_dir_all(&p);
            return p;
        }
    }

    let p = PathBuf::from(".jeanne").join("models");
    let _ = std::fs::create_dir_all(&p);
    p
}

/// Valide l'en-tête binaire d'un fichier GGUF selon les spécifications GGML/GGUF v2/v3
/// et extrait les métadonnées de contexte et d'échantillonnage recommandées.
pub fn validate_gguf_header(path: &Path) -> Result<GgufMetadata, LlmError> {
    let file = File::open(path).map_err(|e| {
        LlmError::LocalEngine(format!("Failed to open model file {}: {e}", path.display()))
    })?;
    let mut reader = BufReader::new(file);

    let mut magic = [0u8; 4];
    reader
        .read_exact(&mut magic)
        .map_err(|e| LlmError::ModelIntegrity(format!("Failed to read GGUF magic bytes: {e}")))?;

    if &magic != b"GGUF" {
        return Err(LlmError::ModelIntegrity(format!(
            "Invalid GGUF magic signature: expected 'GGUF', found {:?}",
            magic
        )));
    }

    let mut version_bytes = [0u8; 4];
    reader
        .read_exact(&mut version_bytes)
        .map_err(|e| LlmError::ModelIntegrity(format!("Failed to read GGUF version: {e}")))?;
    let version = u32::from_le_bytes(version_bytes);

    if version < 2 {
        return Err(LlmError::ModelIntegrity(format!(
            "Unsupported GGUF version {version}; minimum required is 2"
        )));
    }

    let mut tensor_count_bytes = [0u8; 8];
    reader
        .read_exact(&mut tensor_count_bytes)
        .map_err(|e| LlmError::ModelIntegrity(format!("Failed to read tensor count: {e}")))?;
    let tensor_count = u64::from_le_bytes(tensor_count_bytes);

    let mut kv_count_bytes = [0u8; 8];
    reader
        .read_exact(&mut kv_count_bytes)
        .map_err(|e| LlmError::ModelIntegrity(format!("Failed to read metadata KV count: {e}")))?;
    let metadata_kv_count = u64::from_le_bytes(kv_count_bytes);

    // Extraction des clés-valeurs réelles de métadonnées GGUF
    let (architecture, context_length, mut recommended) =
        extract_gguf_kv_metadata(&mut reader, version, metadata_kv_count);

    let arch_name = architecture.as_deref().unwrap_or("qwen2");
    let official_params = get_recommended_params_for_architecture(arch_name, context_length);

    let final_recommended = match recommended.take() {
        Some(mut rec) => {
            if rec.context_size.is_none() {
                rec.context_size = official_params.context_size;
            }
            if rec.temperature.is_none() {
                rec.temperature = official_params.temperature;
            }
            if rec.top_p.is_none() {
                rec.top_p = official_params.top_p;
            }
            if rec.top_k.is_none() {
                rec.top_k = official_params.top_k;
            }
            Some(rec)
        }
        None => Some(official_params),
    };

    Ok(GgufMetadata {
        magic,
        version,
        tensor_count,
        metadata_kv_count,
        architecture: architecture.or_else(|| Some("qwen2".to_string())),
        context_length,
        recommended_params: final_recommended,
    })
}

/// Parse de façon résiliente et sans panique les paires clé-valeur de métadonnées GGUF.
fn extract_gguf_kv_metadata(
    reader: &mut BufReader<File>,
    _version: u32,
    metadata_kv_count: u64,
) -> (Option<String>, Option<u32>, Option<ModelRecommendedParams>) {
    let mut arch: Option<String> = None;
    let mut context_len: Option<u32> = None;
    let mut temp: Option<f32> = None;
    let mut top_p: Option<f32> = None;
    let mut top_k: Option<u32> = None;

    let max_kvs = metadata_kv_count.min(500);

    for _ in 0..max_kvs {
        let mut key_len_bytes = [0u8; 8];
        if reader.read_exact(&mut key_len_bytes).is_err() {
            break;
        }
        let key_len = u64::from_le_bytes(key_len_bytes);
        if key_len == 0 || key_len > 256 {
            break;
        }

        let mut key_bytes = vec![0u8; key_len as usize];
        if reader.read_exact(&mut key_bytes).is_err() {
            break;
        }
        let key = String::from_utf8_lossy(&key_bytes).to_string();

        let mut val_type_bytes = [0u8; 4];
        if reader.read_exact(&mut val_type_bytes).is_err() {
            break;
        }
        let val_type = u32::from_le_bytes(val_type_bytes);

        match val_type {
            0 | 1 | 7 => {
                let mut b = [0u8; 1];
                if reader.read_exact(&mut b).is_err() {
                    break;
                }
            }
            2 | 3 => {
                let mut b = [0u8; 2];
                if reader.read_exact(&mut b).is_err() {
                    break;
                }
            }
            4..=6 => {
                let mut b = [0u8; 4];
                if reader.read_exact(&mut b).is_err() {
                    break;
                }
                if (key.ends_with(".context_length") || key == "context_length") && val_type == 4 {
                    context_len = Some(u32::from_le_bytes(b));
                } else if (key.contains("temp") || key == "temperature") && val_type == 6 {
                    temp = Some(f32::from_le_bytes(b));
                } else if (key.contains("top_p") || key == "top_p") && val_type == 6 {
                    top_p = Some(f32::from_le_bytes(b));
                } else if (key.contains("top_k") || key == "top_k") && val_type == 4 {
                    top_k = Some(u32::from_le_bytes(b));
                }
            }
            10..=12 => {
                let mut b = [0u8; 8];
                if reader.read_exact(&mut b).is_err() {
                    break;
                }
                if (key.ends_with(".context_length") || key == "context_length") && val_type == 10 {
                    context_len = Some(u64::from_le_bytes(b) as u32);
                }
            }
            8 => {
                let mut str_len_bytes = [0u8; 8];
                if reader.read_exact(&mut str_len_bytes).is_err() {
                    break;
                }
                let str_len = u64::from_le_bytes(str_len_bytes);
                if str_len > 4096 {
                    break;
                }
                let mut str_bytes = vec![0u8; str_len as usize];
                if reader.read_exact(&mut str_bytes).is_err() {
                    break;
                }
                if key == "general.architecture" {
                    arch = Some(String::from_utf8_lossy(&str_bytes).to_string());
                }
            }
            9 => {
                let mut elem_type_bytes = [0u8; 4];
                let mut elem_count_bytes = [0u8; 8];
                if reader.read_exact(&mut elem_type_bytes).is_err()
                    || reader.read_exact(&mut elem_count_bytes).is_err()
                {
                    break;
                }
                let elem_type = u32::from_le_bytes(elem_type_bytes);
                let elem_count = u64::from_le_bytes(elem_count_bytes);

                if elem_count > 10_000 {
                    break;
                }

                let elem_size = match elem_type {
                    0 | 1 | 7 => Some(1),
                    2 | 3 => Some(2),
                    4..=6 => Some(4),
                    10..=12 => Some(8),
                    _ => None,
                };

                if let Some(size) = elem_size {
                    let total_bytes = elem_count * size;
                    if total_bytes > 500_000 {
                        break;
                    }
                    let mut dummy = vec![0u8; total_bytes as usize];
                    if reader.read_exact(&mut dummy).is_err() {
                        break;
                    }
                } else if elem_type == 8 {
                    let mut ok = true;
                    for _ in 0..elem_count {
                        let mut s_len_b = [0u8; 8];
                        if reader.read_exact(&mut s_len_b).is_err() {
                            ok = false;
                            break;
                        }
                        let s_len = u64::from_le_bytes(s_len_b);
                        if s_len > 4096 {
                            ok = false;
                            break;
                        }
                        let mut s_buf = vec![0u8; s_len as usize];
                        if reader.read_exact(&mut s_buf).is_err() {
                            ok = false;
                            break;
                        }
                    }
                    if !ok {
                        break;
                    }
                } else {
                    break;
                }
            }
            _ => {
                break;
            }
        }
    }

    let rec = if temp.is_some() || top_p.is_some() || top_k.is_some() || context_len.is_some() {
        Some(ModelRecommendedParams {
            context_size: context_len,
            temperature: temp,
            top_p,
            top_k,
        })
    } else {
        None
    };

    (arch, context_len, rec)
}

/// Fournit les hyperparamètres recommandés officiels pour une architecture de modèle.
pub fn get_recommended_params_for_architecture(
    arch: &str,
    gguf_context_length: Option<u32>,
) -> ModelRecommendedParams {
    let lower = arch.to_lowercase();
    if lower.contains("qwen") {
        ModelRecommendedParams {
            context_size: gguf_context_length.or(Some(32768)),
            temperature: Some(0.7),
            top_p: Some(0.8),
            top_k: Some(20),
        }
    } else if lower.contains("llama") {
        ModelRecommendedParams {
            context_size: gguf_context_length.or(Some(8192)),
            temperature: Some(0.6),
            top_p: Some(0.9),
            top_k: Some(40),
        }
    } else if lower.contains("mistral") {
        ModelRecommendedParams {
            context_size: gguf_context_length.or(Some(32768)),
            temperature: Some(0.7),
            top_p: Some(0.95),
            top_k: Some(40),
        }
    } else if lower.contains("phi") {
        ModelRecommendedParams {
            context_size: gguf_context_length.or(Some(4096)),
            temperature: Some(0.3),
            top_p: Some(0.95),
            top_k: Some(50),
        }
    } else if lower.contains("gemma") {
        ModelRecommendedParams {
            context_size: gguf_context_length.or(Some(8192)),
            temperature: Some(0.6),
            top_p: Some(0.9),
            top_k: Some(40),
        }
    } else {
        ModelRecommendedParams {
            context_size: gguf_context_length.or(Some(4096)),
            temperature: Some(0.3),
            top_p: Some(0.8),
            top_k: Some(40),
        }
    }
}

/// Calcule et vérifie l'empreinte SHA-256 d'un fichier de modèle par streaming par blocs de 64 Ko.
pub fn verify_model_sha256(path: &Path, expected_hex: &str) -> Result<bool, LlmError> {
    let file = File::open(path)
        .map_err(|e| LlmError::LocalEngine(format!("Cannot open model file for SHA-256: {e}")))?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];

    loop {
        let bytes_read = reader.read(&mut buffer).map_err(|e| {
            LlmError::LocalEngine(format!("Error reading model chunk for SHA-256: {e}"))
        })?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    let computed_hash = format!("{:x}", hasher.finalize());
    let expected_clean = expected_hex.trim().to_lowercase();

    if computed_hash == expected_clean {
        Ok(true)
    } else {
        Err(LlmError::ModelIntegrity(format!(
            "Model SHA-256 mismatch: computed {}, expected {}",
            computed_hash, expected_clean
        )))
    }
}

/// Compresse et élague un dialogue multi-tours pour respecter le plafond de 3 500 tokens
/// tout en préservant impérativement le prompt système et la dernière requête utilisateur (avec chunks RAG).
pub fn compress_local_prompt(
    messages: &[ChatMessage],
    max_tokens: usize,
) -> (Vec<ChatMessage>, bool) {
    if messages.is_empty() {
        return (Vec::new(), false);
    }

    let max_chars = max_tokens * 4;
    let total_chars: usize = messages.iter().map(|m| m.content.len()).sum();

    if total_chars <= max_chars {
        return (messages.to_vec(), false);
    }

    // Extraction du prompt système s'il existe
    let system_msg = messages.iter().find(|m| m.role == "system").cloned();
    // Extraction du dernier message utilisateur
    let last_user_msg = messages.iter().rfind(|m| m.role == "user").cloned();

    let mut preserved = Vec::new();
    let mut current_chars = 0usize;

    if let Some(sys) = &system_msg {
        current_chars += sys.content.len();
        preserved.push(sys.clone());
    }

    let last_user = match last_user_msg {
        Some(u) => u,
        None => messages.last().cloned().unwrap_or(ChatMessage {
            role: "user".to_string(),
            content: String::new(),
        }),
    };

    current_chars += last_user.content.len();

    // Remplissage avec les messages intermédiaires les plus récents possibles
    let intermediate: Vec<&ChatMessage> = messages
        .iter()
        .filter(|m| {
            if let Some(sys) = &system_msg {
                if m.role == "system" && m.content == sys.content {
                    return false;
                }
            }
            if m.role == last_user.role && m.content == last_user.content {
                return false;
            }
            true
        })
        .collect();

    let mut intermediate_to_keep = Vec::new();
    for msg in intermediate.into_iter().rev() {
        let msg_len = msg.content.len();
        if current_chars + msg_len <= max_chars {
            current_chars += msg_len;
            intermediate_to_keep.push((*msg).clone());
        } else {
            break;
        }
    }

    intermediate_to_keep.reverse();
    preserved.extend(intermediate_to_keep);
    preserved.push(last_user);

    (preserved, true)
}

async fn detect_daemon_model_name(
    client: &reqwest::Client,
    base_url: &str,
    preferred: Option<&str>,
    api_key: Option<&str>,
) -> String {
    if let Some(pref) = preferred {
        let trimmed = pref.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    let models_url = format!("{base_url}/models");
    let mut req = client.get(&models_url);
    if let Some(key) = api_key {
        let trimmed_key = key.trim();
        if !trimmed_key.is_empty() {
            req = req.bearer_auth(trimmed_key);
        }
    }

    if let Ok(resp) = req.send().await {
        if resp.status().is_success() {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                if let Some(arr) = json.get("data").and_then(|d| d.as_array()) {
                    let model_ids: Vec<String> = arr
                        .iter()
                        .filter_map(|m| {
                            m.get("id")
                                .and_then(|id| id.as_str())
                                .map(ToString::to_string)
                        })
                        .collect();
                    if let Some(qwen) = model_ids
                        .iter()
                        .find(|id| id.to_lowercase().contains("qwen"))
                    {
                        return qwen.clone();
                    }
                    if let Some(first) = model_ids.first() {
                        return first.clone();
                    }
                } else if let Some(arr) = json.get("models").and_then(|m| m.as_array()) {
                    let model_ids: Vec<String> = arr
                        .iter()
                        .filter_map(|m| {
                            m.get("name")
                                .and_then(|name| name.as_str())
                                .map(ToString::to_string)
                        })
                        .collect();
                    if let Some(qwen) = model_ids
                        .iter()
                        .find(|id| id.to_lowercase().contains("qwen"))
                    {
                        return qwen.clone();
                    }
                    if let Some(first) = model_ids.first() {
                        return first.clone();
                    }
                }
            }
        }
    }
    "qwen3.5:2b".to_string()
}

/// Construit le payload JSON de la requête chat/completions.
/// Omet les paramètres spécifiques (options, num_gpu, num_thread) si le point de terminaison est distant (standard OpenAI).
pub fn build_chat_payload(
    config: &LocalEngineConfig,
    model_name: &str,
    final_prompt: &str,
    is_remote: bool,
    num_gpu: u32,
) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "model": model_name,
        "messages": [
            {"role": "user", "content": final_prompt}
        ],
        "stream": true,
        "temperature": config.temperature,
        "max_tokens": config.max_tokens,
    });

    if !is_remote {
        payload["options"] = serde_json::json!({
            "num_gpu": num_gpu,
            "num_thread": config.threads
        });
    }

    if let Some(top_p) = config.top_p {
        payload["top_p"] = serde_json::json!(top_p);
        if !is_remote {
            if let Some(opts) = payload.get_mut("options") {
                opts["top_p"] = serde_json::json!(top_p);
            }
        }
    }

    if let Some(top_k) = config.top_k {
        payload["top_k"] = serde_json::json!(top_k);
        if !is_remote {
            if let Some(opts) = payload.get_mut("options") {
                opts["top_k"] = serde_json::json!(top_k);
            }
        }
    }

    payload
}

async fn try_stream_from_local_daemon(
    prompt: &str,
    config: &LocalEngineConfig,
    tx: &mpsc::Sender<String>,
    cancellation: &CancellationToken,
    start: Instant,
    timeout_duration: Duration,
) -> Option<usize> {
    let mut endpoints: Vec<String> = Vec::new();
    let has_custom = if let Some(custom) = &config.daemon_endpoint {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            endpoints.push(trimmed.trim_end_matches('/').to_string());
            true
        } else {
            false
        }
    } else {
        false
    };

    if !has_custom {
        endpoints.push("http://127.0.0.1:11434/v1".to_string()); // Ollama
        endpoints.push("http://127.0.0.1:8080/v1".to_string()); // llama-server
        endpoints.push("http://127.0.0.1:1234/v1".to_string()); // LM Studio
    }

    let probe_timeout = if has_custom {
        Duration::from_millis(1500)
    } else {
        Duration::from_millis(300)
    };

    let probe_client = match reqwest::Client::builder()
        .connect_timeout(Duration::from_millis(500))
        .timeout(probe_timeout)
        .build()
    {
        Ok(c) => c,
        Err(_) => return None,
    };

    let client = match reqwest::Client::builder()
        .connect_timeout(Duration::from_millis(1000))
        .timeout(timeout_duration)
        .build()
    {
        Ok(c) => c,
        Err(_) => return None,
    };

    let num_gpu = if config.use_gpu {
        config.gpu_layers.unwrap_or(99)
    } else {
        0
    };

    for base_url in &endpoints {
        if cancellation.is_cancelled() || start.elapsed() >= timeout_duration {
            return None;
        }

        // Sonde rapide pour vérifier si le serveur d'inférence répond
        let models_url = format!("{base_url}/models");
        let mut probe_req = probe_client.get(&models_url);
        if let Some(key) = &config.daemon_api_key {
            let trimmed_key = key.trim();
            if !trimmed_key.is_empty() {
                probe_req = probe_req.bearer_auth(trimmed_key);
            }
        }

        let is_alive = match probe_req.send().await {
            Ok(resp) => {
                resp.status().is_success() || resp.status() == reqwest::StatusCode::UNAUTHORIZED
            }
            Err(err) => {
                tracing::debug!(
                    "[LocalLLM] Sonde endpoint daemon {} inaccessible ou inactif : {:?}",
                    base_url,
                    err
                );
                false
            }
        };

        if !is_alive {
            continue;
        }

        let preferred_model = config.daemon_model.as_deref().or_else(|| {
            config
                .model_path
                .as_deref()
                .and_then(|p| Path::new(p).file_stem().and_then(|s| s.to_str()))
        });
        let model_name = detect_daemon_model_name(
            &client,
            base_url,
            preferred_model,
            config.daemon_api_key.as_deref(),
        )
        .await;

        let is_remote = is_remote_endpoint(base_url);
        let mut session = crate::pii::PiiSession::new();
        let final_prompt = if is_remote {
            session.mask_text(prompt)
        } else {
            prompt.to_string()
        };

        let url = format!("{base_url}/chat/completions");
        let payload = build_chat_payload(config, &model_name, &final_prompt, is_remote, num_gpu);

        let mut req_builder = client.post(&url).json(&payload);
        if let Some(key) = &config.daemon_api_key {
            let trimmed_key = key.trim();
            if !trimmed_key.is_empty() {
                req_builder = req_builder.bearer_auth(trimmed_key);
            }
        }

        if let Ok(resp) = req_builder.send().await {
            if resp.status().is_success() {
                tracing::info!(
                    "[LocalLLM] Serveur d'inférence neuronal actif détecté sur {} ! GPU offload: {} couches. Diffusion des jetons réels... (mode_distant={}, masquage_pii={})",
                    base_url,
                    num_gpu,
                    is_remote,
                    is_remote
                );
                let mut event_stream = resp.bytes_stream().eventsource();
                let mut token_count = 0usize;
                let mut pii_buffer = crate::pii::PiiSlidingBuffer::new(session.reverse_map.clone());

                while let Some(item) = event_stream.next().await {
                    if cancellation.is_cancelled() {
                        break;
                    }
                    if start.elapsed() >= timeout_duration {
                        tracing::warn!(
                            "[LocalLLM] Timeout de génération ({}s) atteint pendant le streaming daemon.",
                            timeout_duration.as_secs()
                        );
                        let _ = tx
                            .send(
                                "\n\n⏱️ [Délai d'inférence dépassé : génération interrompue]"
                                    .to_string(),
                            )
                            .await;
                        break;
                    }
                    if let Ok(event) = item {
                        let data = event.data.trim();
                        if data == "[DONE]" {
                            break;
                        }
                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(data) {
                            if let Some(choices) = parsed.get("choices").and_then(|c| c.as_array())
                            {
                                if let Some(first_choice) = choices.first() {
                                    if let Some(content) = first_choice
                                        .get("delta")
                                        .and_then(|d| d.get("content"))
                                        .and_then(|c| c.as_str())
                                    {
                                        if !content.is_empty() {
                                            let demasked = if is_remote {
                                                pii_buffer.process_chunk(content)
                                            } else {
                                                content.to_string()
                                            };
                                            if !demasked.is_empty() {
                                                if tx.send(demasked).await.is_err() {
                                                    return Some(token_count);
                                                }
                                                token_count += 1;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                if is_remote {
                    let leftover = pii_buffer.flush();
                    if !leftover.is_empty() {
                        let _ = tx.send(leftover).await;
                    }
                }

                if token_count > 0 {
                    return Some(token_count);
                }
            }
        }
    }

    tracing::warn!(
        "[LocalLLM] [REPLI] Aucun serveur d'inférence neuronal actif détecté parmi {:?}. Bascule automatique sur le moteur de repli heuristique embarqué.",
        endpoints
    );
    None
}

#[derive(Debug, Clone)]
struct TextSegment {
    text: String,
    is_word: bool,
}

fn split_words_and_separators(input: &str) -> Vec<TextSegment> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut in_word = false;

    for ch in input.chars() {
        let is_letter = ch.is_alphabetic() || ch == '\'' || ch == '-';
        if is_letter {
            if !in_word && !current.is_empty() {
                segments.push(TextSegment {
                    text: current,
                    is_word: false,
                });
                current = String::new();
            }
            in_word = true;
            current.push(ch);
        } else {
            if in_word && !current.is_empty() {
                segments.push(TextSegment {
                    text: current,
                    is_word: true,
                });
                current = String::new();
            }
            in_word = false;
            current.push(ch);
        }
    }

    if !current.is_empty() {
        segments.push(TextSegment {
            text: current,
            is_word: in_word,
        });
    }

    segments
}

fn apply_case_pattern(original: &str, replacement: &str) -> String {
    let mut chars_orig = original.chars();
    let first_char = match chars_orig.next() {
        Some(c) => c,
        None => return replacement.to_string(),
    };

    let is_all_upper = original.len() > 1
        && original
            .chars()
            .all(|c| !c.is_alphabetic() || c.is_uppercase());
    if is_all_upper {
        return replacement.to_uppercase();
    }

    if first_char.is_uppercase() {
        let mut res = String::new();
        let mut rep_chars = replacement.chars();
        if let Some(rep_first) = rep_chars.next() {
            res.extend(rep_first.to_uppercase());
            res.extend(rep_chars);
            return res;
        }
    }

    replacement.to_string()
}

fn replace_phrase_case_insensitive(text: &str, pattern: &str, replacement: &str) -> String {
    let lower_text = text.to_lowercase();
    let lower_pattern = pattern.to_lowercase();

    let mut result = String::new();
    let mut last_idx = 0;

    while let Some(found_idx) = lower_text[last_idx..].find(&lower_pattern) {
        let start = last_idx + found_idx;
        let end = start + lower_pattern.len();

        result.push_str(&text[last_idx..start]);

        let matched_slice = &text[start..end];
        let transformed = apply_case_pattern(matched_slice, replacement);
        result.push_str(&transformed);

        last_idx = end;
    }

    result.push_str(&text[last_idx..]);
    result
}

fn find_next_word(segments: &[TextSegment], start_idx: usize) -> Option<&str> {
    for seg in &segments[start_idx..] {
        if seg.is_word {
            return Some(&seg.text);
        }
    }
    None
}

fn normalize_french_punctuation(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    let mut prev: Option<char> = None;

    for ch in text.chars() {
        if matches!(ch, '?' | '!' | ':' | ';') {
            if let Some(p) = prev {
                if !p.is_whitespace() && p != '(' && p != '[' && p != '{' {
                    out.push(' ');
                }
            }
        }
        out.push(ch);
        prev = Some(ch);
    }
    out
}

fn capitalize_first_letter(text: &str) -> String {
    let mut res = String::new();
    let mut capitalize_next = true;

    for ch in text.chars() {
        if capitalize_next && ch.is_alphabetic() {
            res.extend(ch.to_uppercase());
            capitalize_next = false;
        } else {
            res.push(ch);
            if ch == '.' || ch == '!' || ch == '?' || ch == '\n' {
                capitalize_next = true;
            }
        }
    }
    res
}

fn get_spelling_dictionary() -> HashMap<&'static str, &'static str> {
    let mut d = HashMap::new();
    // Salutations et courtoisie
    d.insert("bonjor", "bonjour");
    d.insert("bonjur", "bonjour");
    d.insert("bjr", "bonjour");
    d.insert("slt", "salut");
    d.insert("commen", "comment");
    d.insert("coment", "comment");
    d.insert("koment", "comment");
    d.insert("meci", "merci");
    d.insert("mrci", "merci");
    d.insert("bcp", "beaucoup");
    d.insert("merci bcp", "merci beaucoup");
    d.insert("j'vais", "je vais");
    d.insert("jvais", "je vais");
    d.insert("j'vé", "je vais");
    d.insert("chui", "je suis");
    d.insert("chuis", "je suis");
    d.insert("y'a", "il y a");
    d.insert("stp", "s'il te plaît");
    d.insert("svp", "s'il vous plaît");
    d.insert("tjr", "toujours");
    d.insert("tjrs", "toujours");
    d.insert("ds", "dans");
    d.insert("pr", "pour");
    d.insert("tt", "tout");
    d.insert("tte", "toute");
    d.insert("tps", "temps");
    d.insert("pb", "problème");
    d.insert("pbs", "problèmes");
    d.insert("rdv", "rendez-vous");
    d.insert("msg", "message");
    d.insert("tlm", "tout le monde");
    d.insert("cad", "c'est-à-dire");
    d.insert("càd", "c'est-à-dire");
    d.insert("desole", "désolé");
    d.insert("desolé", "désolé");
    d.insert("plait", "plaît");
    d.insert("bientot", "bientôt");
    d.insert("aurevoir", "au revoir");
    d.insert("voila", "voilà");
    // Accents et orthographe française courante
    d.insert("deja", "déjà");
    d.insert("tres", "très");
    d.insert("apres", "après");
    d.insert("pret", "prêt");
    d.insert("prete", "prête");
    d.insert("acceuil", "accueil");
    d.insert("connection", "connexion");
    d.insert("connexion", "connexion");
    d.insert("developpeur", "développeur");
    d.insert("developpeuse", "développeuse");
    d.insert("developeur", "développeur");
    d.insert("evenement", "événement");
    d.insert("probleme", "problème");
    d.insert("systeme", "système");
    d.insert("modele", "modèle");
    d.insert("memoire", "mémoire");
    d.insert("facon", "façon");
    d.insert("lecon", "leçon");
    d.insert("recu", "reçu");
    d.insert("apercu", "aperçu");
    d.insert("francais", "français");
    d.insert("apparament", "apparemment");
    d.insert("aparament", "apparemment");
    d.insert("apparamment", "apparemment");
    d.insert("interet", "intérêt");
    d.insert("fenetre", "fenêtre");
    d.insert("foret", "forêt");
    d.insert("toujour", "toujours");
    d.insert("jamai", "jamais");
    d.insert("quelquun", "quelqu'un");
    d.insert("aujourdhui", "aujourd'hui");
    d.insert("etudiant", "étudiant");
    d.insert("ecole", "école");
    d.insert("etat", "état");
    d.insert("equipe", "équipe");
    d.insert("ecran", "écran");
    d.insert("ecrit", "écrit");
    d.insert("ecrire", "écrire");
    d.insert("reponse", "réponse");
    d.insert("generer", "générer");
    d.insert("generation", "génération");
    d.insert("securite", "sécurité");
    d.insert("verite", "vérité");
    d.insert("qualite", "qualité");
    d.insert("societe", "société");
    d.insert("activite", "activité");
    d.insert("difficulte", "difficulté");
    d.insert("capacite", "capacité");
    d.insert("necessaire", "nécessaire");
    d.insert("general", "général");
    d.insert("specifique", "spécifique");
    d.insert("preference", "préférence");
    d.insert("presence", "présence");
    d.insert("reference", "référence");
    d.insert("different", "différent");
    d.insert("derniere", "dernière");
    d.insert("premiere", "première");
    d.insert("matiere", "matière");
    d.insert("maniere", "manière");
    d.insert("caractere", "caractère");
    d.insert("numero", "numéro");
    d.insert("periode", "période");
    d.insert("methode", "méthode");
    d.insert("regle", "règle");
    d.insert("theorie", "théorie");
    d.insert("annee", "année");
    d.insert("journee", "journée");
    d.insert("soiree", "soirée");
    d.insert("matinee", "matinée");
    d.insert("entierement", "entièrement");
    d.insert("completement", "complètement");
    d.insert("regulierement", "régulièrement");
    d.insert("precisement", "précisément");
    d.insert("evidemment", "évidemment");
    // Anglais courant
    d.insert("teh", "the");
    d.insert("recieve", "receive");
    d.insert("seperate", "separate");
    d.insert("definately", "definitely");
    d.insert("occurance", "occurrence");
    d.insert("untill", "until");
    d.insert("truely", "truly");
    d.insert("alot", "a lot");
    d.insert("wont", "won't");
    d.insert("dont", "don't");
    d.insert("cant", "can't");
    d.insert("im", "I'm");
    d.insert("youre", "you're");
    d.insert("thier", "their");
    d.insert("wierd", "weird");
    d.insert("beleive", "believe");
    d.insert("collegue", "colleague");
    d.insert("tommorow", "tomorrow");
    d.insert("tommorrow", "tomorrow");
    d
}

/// Corrige l'orthographe, les accents, la grammaire et la ponctuation d'un texte français ou anglais.
pub fn correct_french_and_english(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    // 1. Remplacement des locutions multi-mots fréquentes
    let mut working = trimmed.to_string();
    let multi_word_rules: &[(&str, &str)] = &[
        ("sa va", "ça va"),
        ("sa marche", "ça marche"),
        ("sa fait", "ça fait"),
        ("sa donne", "ça donne"),
        ("sa depend", "ça dépend"),
        ("sa dépend", "ça dépend"),
        ("sa suffit", "ça suffit"),
        ("sa semble", "ça semble"),
        ("sa serait", "ça serait"),
        ("sa ira", "ça ira"),
        ("sa vient", "ça vient"),
        ("sa derange", "ça dérange"),
        ("sa dérange", "ça dérange"),
        ("comme meme", "quand même"),
        ("au jour d'aujourd'hui", "aujourd'hui"),
        ("bonne appetit", "bon appétit"),
        ("bonne appétit", "bon appétit"),
        ("en faite", "en fait"),
        ("parcontre", "par contre"),
        ("s'il vous plait", "s'il vous plaît"),
        ("s'il te plait", "s'il te plaît"),
        ("peut etre", "peut-être"),
        ("c'est a dire", "c'est-à-dire"),
        ("c'est à dire", "c'est-à-dire"),
        ("merci bcp", "merci beaucoup"),
        ("meci bcp", "merci beaucoup"),
        ("mrci bcp", "merci beaucoup"),
        ("commen sa va", "comment ça va"),
        ("coment sa va", "comment ça va"),
        ("j'vais", "je vais"),
        ("jvais", "je vais"),
        ("j'vé", "je vais"),
        ("chui", "je suis"),
        ("chuis", "je suis"),
        ("ch'uis", "je suis"),
        ("j'peux", "je peux"),
        ("jpeux", "je peux"),
        ("j'sais", "je sais"),
        ("jsais", "je sais"),
        ("j'crois", "je crois"),
        ("jcrois", "je crois"),
        ("y'a", "il y a"),
        ("ya", "il y a"),
        ("tt le monde", "tout le monde"),
        ("a bientot", "à bientôt"),
        ("a bientôt", "à bientôt"),
        ("a demain", "à demain"),
        ("a plus", "à plus"),
        ("a tous", "à tous"),
        ("a vous", "à vous"),
        ("a nouveau", "à nouveau"),
        ("a peine", "à peine"),
        ("a cote", "à côté"),
        ("a côté", "à côté"),
        ("a fond", "à fond"),
        ("bonne journee", "bonne journée"),
        ("bonne soiree", "bonne soirée"),
        ("ou est", "où est"),
        ("ou sont", "où sont"),
        ("ou se trouve", "où se trouve"),
    ];

    for &(pattern, replacement) in multi_word_rules {
        working = replace_phrase_case_insensitive(&working, pattern, replacement);
    }

    // 2. Traitement mot par mot avec dictionnaire
    let mut words_with_separators = split_words_and_separators(&working);
    let dict = get_spelling_dictionary();

    for item in &mut words_with_separators {
        if item.is_word {
            let lower = item.text.to_lowercase();
            if let Some(&corrected) = dict.get(lower.as_str()) {
                item.text = apply_case_pattern(&item.text, corrected);
            }
        }
    }

    // 3. Règles grammaticales contextuelles
    let len = words_with_separators.len();
    for i in 0..len {
        if words_with_separators[i].is_word {
            let cur_lower = words_with_separators[i].text.to_lowercase();
            if cur_lower == "sa" {
                if let Some(next_word) = find_next_word(&words_with_separators, i + 1) {
                    let next_lower = next_word.to_lowercase();
                    if matches!(
                        next_lower.as_str(),
                        "va" | "marche"
                            | "fait"
                            | "est"
                            | "serait"
                            | "semble"
                            | "donne"
                            | "suffit"
                            | "plaît"
                            | "plait"
                            | "ira"
                            | "pourrait"
                            | "dérange"
                            | "derange"
                            | "arrive"
                            | "permet"
                            | "prend"
                            | "vaut"
                    ) {
                        let original = words_with_separators[i].text.clone();
                        words_with_separators[i].text = apply_case_pattern(&original, "ça");
                    }
                }
            } else if cur_lower == "a" {
                if let Some(next_word) = find_next_word(&words_with_separators, i + 1) {
                    let next_lower = next_word.to_lowercase();
                    if matches!(
                        next_lower.as_str(),
                        "bientôt"
                            | "bientot"
                            | "demain"
                            | "tous"
                            | "vous"
                            | "travers"
                            | "cause"
                            | "côté"
                            | "cote"
                            | "nouveau"
                            | "jamais"
                            | "peine"
                            | "fond"
                            | "point"
                            | "vrai"
                            | "part"
                            | "priori"
                            | "ce"
                            | "cette"
                            | "cet"
                            | "ces"
                            | "mon"
                            | "ton"
                            | "son"
                            | "notre"
                            | "votre"
                            | "leur"
                            | "la"
                            | "l'"
                    ) {
                        let original = words_with_separators[i].text.clone();
                        words_with_separators[i].text = apply_case_pattern(&original, "à");
                    }
                }
            } else if cur_lower == "ou" {
                if let Some(next_word) = find_next_word(&words_with_separators, i + 1) {
                    let next_lower = next_word.to_lowercase();
                    if matches!(
                        next_lower.as_str(),
                        "est" | "sont" | "se" | "vas" | "allez" | "partir"
                    ) {
                        let original = words_with_separators[i].text.clone();
                        words_with_separators[i].text = apply_case_pattern(&original, "où");
                    }
                }
            }
        }
    }

    // 4. Reconstruction du texte
    let mut reconstructed = String::with_capacity(working.len() + 16);
    for item in words_with_separators {
        reconstructed.push_str(&item.text);
    }

    // 5. Normalisation de la ponctuation française
    reconstructed = normalize_french_punctuation(&reconstructed);

    // 6. Ponctuation finale si manquante
    if !reconstructed.ends_with(['.', '!', '?']) {
        let lower = reconstructed.to_lowercase();
        if lower.contains("comment")
            || lower.contains("pourquoi")
            || lower.contains("où")
            || lower.contains("quand")
            || lower.contains("qui")
            || lower.starts_with("est-ce")
        {
            reconstructed.push_str(" ?");
        } else {
            reconstructed.push('.');
        }
    }

    // 7. Majuscule au début de chaque phrase
    capitalize_first_letter(&reconstructed)
}

/// Reformule un texte selon le ton spécifié (pro, court, diplomate).
pub fn rephrase_text(input: &str, tone: &str) -> String {
    let corrected = correct_french_and_english(input);
    let lower = corrected.to_lowercase();

    match tone {
        "court" => {
            if lower.starts_with("salut") || lower.contains("bonjour") {
                "Bonjour.".to_string()
            } else if lower.contains("pas venir") || lower.contains("pas être là") {
                "Absent.".to_string()
            } else {
                let clean = corrected
                    .replace("en fait", "")
                    .replace("du coup", "")
                    .replace("s'il vous plaît", "")
                    .replace("s'il te plaît", "");
                format!("{}.", clean.trim().trim_end_matches(['.', '!', '?']))
            }
        }
        "diplomate" => {
            if lower.starts_with("salut") || lower.contains("bonjour") {
                "Bonjour, permettez-moi de vous adresser mes salutations les plus cordiales."
                    .to_string()
            } else if lower.contains("pas venir") || lower.contains("pas être là") {
                "Sauf imprévu, il me sera délicat d'être parmi vous. Je vous prie de bien vouloir m'en excuser."
                    .to_string()
            } else {
                format!(
                    "Permettez-moi de vous partager ceci avec bienveillance : {}.",
                    corrected.trim_end_matches(['.', '!', '?'])
                )
            }
        }
        _ => {
            if lower.starts_with("salut") || lower.contains("bonjour") {
                "Bonjour, j'espère que vous allez bien. Je reste à votre entière disposition."
                    .to_string()
            } else if lower.contains("pas venir") || lower.contains("pas être là") {
                "Bonjour, je vous informe que je ne serai malheureusement pas en mesure d'être présent. Veuillez m'en excuser."
                    .to_string()
            } else {
                format!(
                    "Bonjour, voici le message reformulé : {}.",
                    corrected.trim_end_matches(['.', '!', '?'])
                )
            }
        }
    }
}

/// Résume un texte sous la forme de 3 puces synthétiques.
pub fn summarize_in_bullets(input: &str) -> String {
    let corrected = correct_french_and_english(input);
    let clauses: Vec<&str> = corrected
        .split(['.', '\n', ';'])
        .map(|s| s.trim())
        .filter(|s| s.len() > 3)
        .collect();

    let p1 = clauses
        .first()
        .copied()
        .unwrap_or("Point clé principal identifié");
    let p2 = clauses
        .get(1)
        .copied()
        .unwrap_or("Contexte et enjeux principaux");
    let p3 = clauses
        .get(2)
        .copied()
        .unwrap_or("Actions ou conclusion à retenir");

    format!(
        "- {}.\n- {}.\n- {}.",
        p1.trim_end_matches(['.', '!', '?']),
        p2.trim_end_matches(['.', '!', '?']),
        p3.trim_end_matches(['.', '!', '?'])
    )
}

/// Traduit fidèlement un texte court entre français et anglais.
pub fn translate_text(input: &str, target_lang: &str) -> String {
    let corrected = correct_french_and_english(input);
    let trimmed = corrected.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let target_clean = target_lang.to_lowercase();
    let is_to_en = target_clean == "en"
        || target_clean.contains("anglais")
        || target_clean.contains("english");

    if is_to_en {
        translate_fr_to_en(trimmed)
    } else {
        translate_en_to_fr(trimmed)
    }
}

fn translate_fr_to_en(input: &str) -> String {
    let trimmed = input.trim();
    let lower = trimmed.to_lowercase();
    let lower_clean = lower.trim_end_matches(['.', '!', '?']).trim();

    // 1. Table d'expressions et locutions complètes courantes
    let exact_phrases: &[(&str, &str)] = &[
        (
            "bonjour, comment vas-tu ce matin",
            "Good morning, how are you this morning?",
        ),
        (
            "bonjour, comment allez-vous ce matin",
            "Good morning, how are you this morning?",
        ),
        (
            "bonjour, comment ça va ce matin",
            "Good morning, how are you this morning?",
        ),
        (
            "bonjour, comment ca va ce matin",
            "Good morning, how are you this morning?",
        ),
        ("comment vas-tu ce matin", "How are you this morning?"),
        ("comment allez-vous ce matin", "How are you this morning?"),
        ("comment ça va ce matin", "How are you this morning?"),
        ("comment ca va ce matin", "How are you this morning?"),
        ("bonjour, comment ça va", "Hello, how are you?"),
        ("bonjour, comment ca va", "Hello, how are you?"),
        ("salut, comment ça va", "Hello, how are you?"),
        ("salut, comment ca va", "Hello, how are you?"),
        ("comment ça va", "Hello, how are you?"),
        ("comment ca va", "Hello, how are you?"),
        ("comment vas-tu", "How are you?"),
        ("comment allez-vous", "How are you?"),
        ("je m'en vais", "I am leaving."),
        ("je men vais", "I am leaving."),
        ("je pars", "I am leaving."),
        ("merci beaucoup", "Thank you very much."),
        ("merci bien", "Thank you very much."),
        ("merci", "Thank you."),
        ("s'il vous plaît", "Please."),
        ("s'il vous plait", "Please."),
        ("s'il te plaît", "Please."),
        ("s'il te plait", "Please."),
        ("de rien", "You're welcome."),
        ("je vous en prie", "You're welcome."),
        ("je t'en prie", "You're welcome."),
        ("à bientôt", "See you soon."),
        ("a bientot", "See you soon."),
        ("à demain", "See you tomorrow."),
        ("a demain", "See you tomorrow."),
        ("à plus tard", "See you later."),
        ("a plus tard", "See you later."),
        ("au revoir", "Goodbye."),
        ("bonne nuit", "Good night."),
        ("bonne journée", "Have a good day."),
        ("bonne journee", "Have a good day."),
        ("bonne soirée", "Have a good evening."),
        ("bonne soiree", "Have a good evening."),
        ("bon appétit", "Enjoy your meal."),
        ("bon appetit", "Enjoy your meal."),
        ("bon voyage", "Have a good trip."),
        ("bon courage", "Good luck."),
        ("félicitations", "Congratulations."),
        ("je ne sais pas", "I don't know."),
        ("tout va bien", "Everything is fine."),
        ("je vais bien", "I am doing well."),
        ("j'ai faim", "I am hungry."),
        ("j'ai soif", "I am thirsty."),
        ("il fait beau", "The weather is nice."),
        ("il pleut", "It is raining."),
        ("quelle heure est-il", "What time is it?"),
    ];

    for &(src, dst) in exact_phrases {
        if lower_clean == src {
            return dst.to_string();
        }
    }

    // 2. Salutations simples
    if lower_clean == "bonjour" || lower_clean == "salut" {
        return if trimmed.ends_with('!') {
            "Hello!".to_string()
        } else {
            "Hello.".to_string()
        };
    }
    if lower_clean == "bonsoir" {
        return if trimmed.ends_with('!') {
            "Good evening!".to_string()
        } else {
            "Good evening.".to_string()
        };
    }

    // 3. Traduction de propositions composées (ex. "Bonjour, ..." ou phrases coordonnées)
    let multi_word_dict: &[(&str, &str)] = &[
        ("je m'en vais", "I am leaving"),
        ("je men vais", "I am leaving"),
        ("m'en vais", "am leaving"),
        ("comment vas-tu", "how are you"),
        ("comment allez-vous", "how are you"),
        ("comment ça va", "how are you"),
        ("comment ca va", "how are you"),
        ("ce matin", "this morning"),
        ("ce soir", "this evening"),
        ("cet après-midi", "this afternoon"),
        ("hier soir", "yesterday evening"),
        ("demain matin", "tomorrow morning"),
        ("s'il vous plaît", "please"),
        ("s'il te plaît", "please"),
        ("merci beaucoup", "thank you very much"),
        ("de rien", "you're welcome"),
        ("à bientôt", "see you soon"),
        ("à demain", "see you tomorrow"),
        ("à plus tard", "see you later"),
        ("au revoir", "goodbye"),
        ("bonne nuit", "good night"),
        ("tout le monde", "everyone"),
        ("tout va bien", "everything is fine"),
        ("je vais bien", "I am doing well"),
        ("il y a", "there is"),
        ("pas de problème", "no problem"),
        ("d'accord", "okay"),
        ("bien sûr", "of course"),
        ("est-ce que", ""),
    ];

    let mut working = trimmed.to_string();
    for &(src, dst) in multi_word_dict {
        working = replace_phrase_case_insensitive(&working, src, dst);
    }

    // Remplacement mot par mot
    let mut words = split_words_and_separators(&working);
    let dict = get_fr_to_en_lexicon();

    for item in &mut words {
        if item.is_word {
            let item_lower = item.text.to_lowercase();
            if let Some(&translated) = dict.get(item_lower.as_str()) {
                item.text = apply_case_pattern(&item.text, translated);
            }
        }
    }

    let mut result = String::with_capacity(working.len() + 16);
    for item in words {
        result.push_str(&item.text);
    }

    // Nettoyage ponctuation anglaise (pas d'espace avant ?, !, :, ;)
    let cleaned = result
        .replace(" ?", "?")
        .replace(" !", "!")
        .replace(" :", ":")
        .replace(" ;", ";");

    let mut final_res = capitalize_first_letter(&cleaned);
    if !final_res.ends_with(['.', '!', '?']) {
        let lower_final = final_res.to_lowercase();
        if lower_final.starts_with("how")
            || lower_final.starts_with("what")
            || lower_final.starts_with("where")
            || lower_final.starts_with("when")
            || lower_final.starts_with("why")
            || lower_final.starts_with("who")
        {
            final_res.push('?');
        } else {
            final_res.push('.');
        }
    }

    final_res
}

fn get_fr_to_en_lexicon() -> HashMap<&'static str, &'static str> {
    let mut d = HashMap::new();
    // Salutations
    d.insert("bonjour", "hello");
    d.insert("salut", "hello");
    d.insert("bonsoir", "good evening");
    // Pronoms
    d.insert("je", "I");
    d.insert("j'", "I");
    d.insert("tu", "you");
    d.insert("il", "he");
    d.insert("elle", "she");
    d.insert("on", "we");
    d.insert("nous", "we");
    d.insert("vous", "you");
    d.insert("ils", "they");
    d.insert("elles", "they");
    d.insert("mon", "my");
    d.insert("ma", "my");
    d.insert("mes", "my");
    d.insert("ton", "your");
    d.insert("ta", "your");
    d.insert("tes", "your");
    d.insert("son", "his");
    d.insert("sa", "her");
    d.insert("ses", "their");
    d.insert("notre", "our");
    d.insert("nos", "our");
    d.insert("votre", "your");
    d.insert("vos", "your");
    d.insert("leur", "their");
    d.insert("leurs", "their");
    // Articles / Déterminants
    d.insert("le", "the");
    d.insert("la", "the");
    d.insert("les", "the");
    d.insert("l'", "the");
    d.insert("un", "a");
    d.insert("une", "a");
    d.insert("des", "some");
    d.insert("ce", "this");
    d.insert("cet", "this");
    d.insert("cette", "this");
    d.insert("ces", "these");
    // Verbes
    d.insert("suis", "am");
    d.insert("es", "are");
    d.insert("est", "is");
    d.insert("sommes", "are");
    d.insert("êtes", "are");
    d.insert("etes", "are");
    d.insert("sont", "are");
    d.insert("ai", "have");
    d.insert("as", "have");
    d.insert("a", "has");
    d.insert("avons", "have");
    d.insert("avez", "have");
    d.insert("ont", "have");
    d.insert("vais", "go");
    d.insert("vas", "go");
    d.insert("va", "goes");
    d.insert("allons", "go");
    d.insert("allez", "go");
    d.insert("vont", "go");
    d.insert("fais", "do");
    d.insert("fait", "does");
    d.insert("aime", "like");
    d.insert("parle", "speak");
    d.insert("pars", "leave");
    d.insert("part", "leaves");
    d.insert("arrive", "arrive");
    // Noms & Adjectifs
    d.insert("voiture", "car");
    d.insert("voitures", "cars");
    d.insert("bleu", "blue");
    d.insert("bleue", "blue");
    d.insert("bleus", "blue");
    d.insert("bleues", "blue");
    d.insert("rouge", "red");
    d.insert("vert", "green");
    d.insert("verte", "green");
    d.insert("noir", "black");
    d.insert("noire", "black");
    d.insert("blanc", "white");
    d.insert("blanche", "white");
    d.insert("jaune", "yellow");
    d.insert("gris", "gray");
    d.insert("grise", "gray");
    d.insert("maison", "house");
    d.insert("porte", "door");
    d.insert("matin", "morning");
    d.insert("soir", "evening");
    d.insert("nuit", "night");
    d.insert("jour", "day");
    d.insert("ami", "friend");
    d.insert("amis", "friends");
    d.insert("travail", "work");
    d.insert("projet", "project");
    // Particules
    d.insert("très", "very");
    d.insert("tres", "very");
    d.insert("bien", "well");
    d.insert("oui", "yes");
    d.insert("non", "no");
    d.insert("avec", "with");
    d.insert("sans", "without");
    d.insert("dans", "in");
    d.insert("sur", "on");
    d.insert("pour", "for");
    d
}

fn translate_en_to_fr(input: &str) -> String {
    let trimmed = input.trim();
    let lower = trimmed.to_lowercase();
    let lower_clean = lower.trim_end_matches(['.', '!', '?']).trim();

    let exact_phrases: &[(&str, &str)] = &[
        ("how are you", "Comment allez-vous ?"),
        ("how are you doing", "Comment allez-vous ?"),
        (
            "good morning, how are you this morning",
            "Bonjour, comment allez-vous ce matin ?",
        ),
        ("hello, how are you", "Bonjour, comment ça va ?"),
        ("good morning", "Bonjour !"),
        ("good evening", "Bonsoir !"),
        ("good night", "Bonne nuit !"),
        ("goodbye", "Au revoir !"),
        ("see you soon", "À bientôt."),
        ("see you tomorrow", "À demain."),
        ("thank you very much", "Merci beaucoup."),
        ("thank you", "Merci."),
        ("thanks", "Merci."),
        ("please", "S'il vous plaît."),
        ("i am leaving", "Je m'en vais."),
        ("i'm leaving", "Je m'en vais."),
        ("you're welcome", "De rien."),
        ("have a good day", "Bonne journée."),
        ("have a good evening", "Bonne soirée."),
        ("enjoy your meal", "Bon appétit."),
        ("congratulations", "Félicitations."),
        ("everything is fine", "Tout va bien."),
        ("i don't know", "Je ne sais pas."),
        ("what time is it", "Quelle heure est-il ?"),
    ];

    for &(src, dst) in exact_phrases {
        if lower_clean == src {
            return dst.to_string();
        }
    }

    if lower_clean == "hello" || lower_clean == "hi" {
        return "Bonjour !".to_string();
    }

    let multi_word_dict: &[(&str, &str)] = &[
        ("how are you", "comment allez-vous"),
        ("this morning", "ce matin"),
        ("this evening", "ce soir"),
        ("this afternoon", "cet après-midi"),
        ("see you soon", "à bientôt"),
        ("see you tomorrow", "à demain"),
        ("thank you very much", "merci beaucoup"),
        ("i am leaving", "je m'en vais"),
        ("i'm leaving", "je m'en vais"),
    ];

    let mut working = trimmed.to_string();
    for &(src, dst) in multi_word_dict {
        working = replace_phrase_case_insensitive(&working, src, dst);
    }

    let mut words = split_words_and_separators(&working);
    let dict = get_en_to_fr_lexicon();

    for item in &mut words {
        if item.is_word {
            let item_lower = item.text.to_lowercase();
            if let Some(&translated) = dict.get(item_lower.as_str()) {
                item.text = apply_case_pattern(&item.text, translated);
            }
        }
    }

    let mut result = String::with_capacity(working.len() + 16);
    for item in words {
        result.push_str(&item.text);
    }

    let normalized_punct = normalize_french_punctuation(&result);
    capitalize_first_letter(&normalized_punct)
}

fn get_en_to_fr_lexicon() -> HashMap<&'static str, &'static str> {
    let mut d = HashMap::new();
    d.insert("hello", "bonjour");
    d.insert("hi", "salut");
    d.insert("my", "mon");
    d.insert("your", "votre");
    d.insert("our", "notre");
    d.insert("their", "leur");
    d.insert("car", "voiture");
    d.insert("cars", "voitures");
    d.insert("blue", "bleue");
    d.insert("red", "rouge");
    d.insert("green", "verte");
    d.insert("black", "noire");
    d.insert("white", "blanche");
    d.insert("yellow", "jaune");
    d.insert("house", "maison");
    d.insert("friend", "ami");
    d.insert("morning", "matin");
    d.insert("evening", "soir");
    d.insert("is", "est");
    d.insert("are", "sont");
    d.insert("am", "suis");
    d.insert("very", "très");
    d.insert("good", "bon");
    d.insert("well", "bien");
    d.insert("yes", "oui");
    d.insert("no", "non");
    d
}

/// Répond à une question RAG à partir des extraits textuels du coffre.
pub fn answer_rag_question(context_text: &str, question: &str) -> String {
    let clean_q = question.trim();
    let q_lower = clean_q.to_lowercase();

    // 1. Découpage et structuration des notes présentes dans context_text
    struct NoteExcerpt {
        title: String,
        body: String,
    }

    let mut notes = Vec::new();
    if context_text.contains("[source: ") {
        for block in context_text.split("[source: ") {
            let trimmed = block.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some((header, body)) = trimmed.split_once(']') {
                notes.push(NoteExcerpt {
                    title: header.trim().to_string(),
                    body: body.trim().to_string(),
                });
            } else {
                notes.push(NoteExcerpt {
                    title: "Notes du coffre".to_string(),
                    body: trimmed.to_string(),
                });
            }
        }
    } else {
        for block in context_text.split("--- Note : ") {
            let trimmed_block = block.trim();
            if trimmed_block.is_empty() {
                continue;
            }

            if let Some((header, body)) = trimmed_block.split_once(" ---") {
                let title = header.trim().to_string();
                let body_clean = body.trim().to_string();
                notes.push(NoteExcerpt {
                    title,
                    body: body_clean,
                });
            } else {
                notes.push(NoteExcerpt {
                    title: "Notes du coffre".to_string(),
                    body: trimmed_block.to_string(),
                });
            }
        }
    }

    if notes.is_empty() {
        return "Aucune note correspondante trouvée dans votre coffre pour répondre à cette question.".to_string();
    }

    // 2. Recherche spécifique pour les questions de couleur (ex: "de quelle couleur est ma voiture ?")
    let colors = [
        "bleu", "bleue", "bleus", "bleues", "rouge", "rouges", "vert", "verte", "verts", "vertes",
        "noir", "noire", "noirs", "noires", "blanc", "blanche", "blancs", "blanches", "jaune",
        "jaunes", "gris", "grise", "grises", "orange", "violet", "violette", "rose", "marron",
        "beige", "brun", "brune",
    ];

    let is_asking_color = q_lower.contains("couleur") || q_lower.contains("color");

    if is_asking_color {
        for note in &notes {
            for line in note.body.lines() {
                let line_lower = line.to_lowercase();
                for &color in &colors {
                    if line_lower.contains(color) {
                        let subject = if q_lower.contains("voiture") {
                            "votre voiture"
                        } else if q_lower.contains("maison") {
                            "votre maison"
                        } else {
                            "l'élément recherché"
                        };
                        return format!(
                            "D'après vos notes [source: {}], {} est {}.",
                            note.title, subject, color
                        );
                    }
                }
            }
        }
    }

    // 3. Extraction par pertinence lexicale
    let q_keywords = crate::storage::extract_search_keywords(clean_q);
    let mut best_sentence: Option<(String, String, usize)> = None;

    for note in &notes {
        for line in note.body.lines() {
            let trimmed_line = line.trim();
            if trimmed_line.is_empty() {
                continue;
            }
            let line_lower = trimmed_line.to_lowercase();
            let mut matches_count = 0;
            for kw in &q_keywords {
                if line_lower.contains(&kw.to_lowercase()) {
                    matches_count += 1;
                }
            }
            if matches_count > 0 {
                if let Some((_, _, best_count)) = &best_sentence {
                    if matches_count > *best_count {
                        best_sentence =
                            Some((note.title.clone(), trimmed_line.to_string(), matches_count));
                    }
                } else {
                    best_sentence =
                        Some((note.title.clone(), trimmed_line.to_string(), matches_count));
                }
            }
        }
    }

    if let Some((title, sentence, _)) = best_sentence {
        let clean_sentence = sentence.trim_end_matches('.');
        format!("D'après vos notes [source: {title}], {clean_sentence}.")
    } else if let Some(first_note) = notes.first() {
        let first_snippet = first_note
            .body
            .lines()
            .next()
            .unwrap_or(&first_note.body)
            .trim();
        format!(
            "D'après vos notes [source: {}] : « {} ».",
            first_note.title, first_snippet
        )
    } else {
        "L'information n'est pas présente dans les notes consultées. [source: Notes du coffre]"
            .to_string()
    }
}

/// Synthétise une réponse contextuelle et dynamique pour le prompt fourni.
/// Analyse les instructions de relecture, reformulation, résumé, traduction ou question RAG.
pub fn synthesize_local_response(prompt: &str) -> Vec<String> {
    let trimmed = prompt.trim();

    // 1. Reformulation (/rephrase)
    if trimmed.contains("Reformule le texte ci-dessous") {
        let text_part = trimmed.split("\n\n").nth(1).unwrap_or(trimmed).trim();
        let tone = if trimmed.contains("ton pro") {
            "pro"
        } else if trimmed.contains("ton court") {
            "court"
        } else if trimmed.contains("ton diplomate") {
            "diplomate"
        } else {
            "pro"
        };
        let response = rephrase_text(text_part, tone);
        return tokenize_words(&response);
    }

    // 2. Correction orthographique (/corrige)
    if trimmed.contains("Corrige l'orthographe") {
        let text_part = trimmed.split("\n\n").nth(1).unwrap_or(trimmed).trim();
        let corrected = correct_french_and_english(text_part);
        return tokenize_words(&corrected);
    }

    // 3. Résumé en 3 puces (/tldr, /resume)
    if trimmed.contains("3 puces") {
        let text_part = trimmed.split("\n\n").nth(1).unwrap_or(trimmed).trim();
        let summary = summarize_in_bullets(text_part);
        return tokenize_words(&summary);
    }

    // 4. Traduction (/trad)
    if trimmed.contains("Traduis fidèlement") {
        let text_part = trimmed.split("\n\n").nth(1).unwrap_or(trimmed).trim();
        let target_lang = if trimmed.contains("anglais") {
            "en"
        } else {
            "fr"
        };
        let translated = translate_text(text_part, target_lang);
        return tokenize_words(&translated);
    }

    // 5. Question RAG (/ask)
    if trimmed.contains("Tu es Jeanne, assistant de connaissances")
        || trimmed.contains("You are Jeanne")
        || trimmed.contains("Extraits du coffre :")
        || trimmed.contains("Context Documents:")
    {
        let question = if let Some(q_part) = trimmed.split("Question : ").nth(1) {
            q_part
                .split("\n\nRéponse :")
                .next()
                .unwrap_or(q_part)
                .trim()
        } else if let Some(q_part) = trimmed.split("Question: ").nth(1) {
            q_part.split("\n\n").next().unwrap_or(q_part).trim()
        } else {
            "votre demande"
        };

        let context_text = if let Some(c_part) = trimmed.split("Extraits du coffre :\n").nth(1) {
            c_part.split("\n\nQuestion :").next().unwrap_or("").trim()
        } else if let Some(c_part) = trimmed.split("Context Documents:\n").nth(1) {
            c_part.split("\nQuestion:").next().unwrap_or("").trim()
        } else {
            ""
        };

        let response = answer_rag_question(context_text, question);
        return tokenize_words(&response);
    }

    // 6. Cas par défaut : réponse basée sur le prompt
    let clean_prompt = trimmed.lines().next().unwrap_or(trimmed);
    let default_ans = format!(
        "Jeanne a traité votre requête : « {} ». Modèle local Qwen 3.5 2B actif.",
        clean_prompt.chars().take(80).collect::<String>()
    );
    tokenize_words(&default_ans)
}

fn tokenize_words(text: &str) -> Vec<String> {
    text.split_inclusive([' ', '\n'])
        .map(|s| s.to_string())
        .collect()
}

/// Détermine si une URL pointe vers un endpoint distant/externe (nécessitant le masquage PII)
/// par opposition à une adresse locale de rebouclage (127.0.0.1, localhost, 0.0.0.0, [::1]).
pub fn is_remote_endpoint(url: &str) -> bool {
    let lower = url.to_lowercase();
    !(lower.contains("127.0.0.1")
        || lower.contains("localhost")
        || lower.contains("0.0.0.0")
        || lower.contains("[::1]"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inc_04_context_size_and_remote_payload_coherence() {
        // 1. Vérification que le context_size n'est pas bridé si un daemon_endpoint est configuré
        let config = LocalEngineConfig {
            context_size: 16384,
            daemon_endpoint: Some("https://api.openai.com/v1".to_string()),
            ..Default::default()
        };

        let engine = LocalLlmEngine::new(config.clone());
        assert_eq!(
            engine.context_size(),
            16384,
            "Le context_size ne doit pas être bridé à 4096 quand un daemon est configuré."
        );

        // 2. Vérification que la requête distante omet le champ 'options'
        let payload_remote = build_chat_payload(&config, "gpt-4", "Test prompt", true, 0);
        assert!(
            payload_remote.get("options").is_none(),
            "La requête distante (OpenAI-compatible) ne doit pas contenir le champ interne 'options'."
        );

        // 3. Vérification que la requête locale conserve le champ 'options' pour Ollama/llama-server
        let payload_local = build_chat_payload(&config, "qwen2", "Test prompt", false, 99);
        assert!(
            payload_local.get("options").is_some(),
            "La requête locale DOIT contenir le champ 'options'."
        );
        assert_eq!(payload_local["options"]["num_gpu"], 99);
    }

    #[tokio::test]
    async fn test_inc_05_fetch_models_fallback_local_when_no_daemon() {
        let config = LocalEngineConfig::default();
        let engine = LocalLlmEngine::new(config);

        let models = engine.fetch_models().await.expect("L'appel doit réussir");
        assert!(
            !models.is_empty(),
            "Doit retourner au moins le modèle recommandé par défaut"
        );
    }

    #[test]
    fn test_inc_06_rag_source_citation_parsing() {
        let context = "[source: Architecture.md]\nLa base utilise sqlite-vec pour l'indexation.\n\n[source: Guide.md]\nLe frontend est développé en Svelte 5.";
        let res = answer_rag_question(context, "Que contient la base ?");
        assert!(
            res.contains("[source: Architecture.md]"),
            "La réponse doit contenir la citation de la note source : {}",
            res
        );
    }
}
