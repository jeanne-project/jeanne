//! Modèles de données typés et protocole JSON-RPC 2.0 pour l'écosystème de plugins Jeanne.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

/// Cycle de vie d'un plugin.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum PluginLifecycle {
    /// Lancé à la demande, exécute la tâche et se termine immédiatement.
    #[serde(rename = "on_demand")]
    #[default]
    OnDemand,
    /// Maintenu actif pendant la session avec arrêt après inactivité.
    #[serde(rename = "daemon_managed")]
    DaemonManaged,
}

/// Capacités opérationnelles déclarées par un plugin dans son manifeste.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum PluginCapability {
    /// Inférence locale de modèles de langage (GGUF).
    #[serde(rename = "llm_runner")]
    LlmRunner {
        #[serde(default)]
        default_model: String,
        #[serde(default)]
        supported_formats: Vec<String>,
        #[serde(default = "default_context_size")]
        max_context_size: u32,
        #[serde(default)]
        supports_streaming: bool,
    },
    /// Génération d'embeddings denses pour recherche hybride vectorielle.
    #[serde(rename = "embeddings_generator")]
    EmbeddingsGenerator {
        #[serde(default)]
        default_model: String,
        #[serde(default = "default_dimension")]
        default_dimension: usize,
        #[serde(default = "default_seq_length")]
        max_sequence_length: usize,
        #[serde(default)]
        supports_batching: bool,
    },
    /// Reconnaissance vocale (Speech-to-Text).
    #[serde(rename = "voice_stt")]
    VoiceStt {
        #[serde(default)]
        default_model: String,
        #[serde(default)]
        input_format: String,
        #[serde(default)]
        supported_languages: Vec<String>,
    },
    /// Synthèse vocale (Text-to-Speech).
    #[serde(rename = "voice_tts")]
    VoiceTts {
        #[serde(default)]
        default_voice: String,
        #[serde(default)]
        output_format: String,
        #[serde(default)]
        streaming_sentence_level: bool,
    },
    /// Ingestion et parsing de formats documentaires complexes.
    #[serde(rename = "document_parser")]
    DocumentParser {
        #[serde(default)]
        supported_extensions: Vec<String>,
        #[serde(default)]
        supported_mimetypes: Vec<String>,
    },
}

fn default_context_size() -> u32 {
    4096
}

fn default_dimension() -> usize {
    384
}

fn default_seq_length() -> usize {
    512
}

/// Point d'entrée de l'exécutable selon la plateforme cible.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginEntrypoint {
    #[serde(default)]
    pub windows: String,
    #[serde(default)]
    pub linux: String,
}

impl PluginEntrypoint {
    /// Résout le chemin relatif approprié selon le système d'exploitation hôte.
    pub fn resolve_for_current_os(&self) -> &str {
        if cfg!(target_os = "windows") {
            &self.windows
        } else {
            &self.linux
        }
    }
}

/// Manifeste complet d'un plugin (`plugin.json`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginManifest {
    pub schema_version: String,
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    pub entrypoint: PluginEntrypoint,
    #[serde(default)]
    pub lifecycle: PluginLifecycle,
    #[serde(default = "default_timeout_seconds")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub idle_timeout_seconds: Option<u64>,
    #[serde(default)]
    pub capabilities: Vec<PluginCapability>,
    #[serde(skip)]
    pub root_dir: PathBuf,
}

fn default_timeout_seconds() -> u64 {
    60
}

/// Requête standard JSON-RPC 2.0 transmise sur `stdin`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
    pub id: u64,
}

impl JsonRpcRequest {
    pub fn new(method: &str, params: serde_json::Value, id: u64) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params,
            id,
        }
    }
}

/// Réponse standard JSON-RPC 2.0 reçue sur `stdout`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(default)]
    pub result: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<JsonRpcError>,
    pub id: u64,
}

/// Détail d'erreur typée JSON-RPC 2.0.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(default)]
    pub data: Option<serde_json::Value>,
}

/// Notification streaming unilatérale (ex: `token_chunk` pour LLM).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    pub params: serde_json::Value,
}

/// Erreurs de gestion et d'exécution des plugins.
#[derive(Debug, Error)]
pub enum PluginError {
    #[error("Plugin introuvable : {0}")]
    NotFound(String),
    #[error("Manifeste de plugin invalide dans {0} : {1}")]
    InvalidManifest(String, String),
    #[error("Exécutable introuvable pour le plugin {0} : {1}")]
    ExecutableNotFound(String, String),
    #[error("Erreur I/O lors de l'exécution du plugin : {0}")]
    Io(#[from] std::io::Error),
    #[error("Erreur de sérialisation JSON-RPC : {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Timeout d'exécution dépassé ({0}s) pour le plugin {1}")]
    Timeout(u64, String),
    #[error("Le sous-processus du plugin a échoué (code de sortie : {0:?}) : {1}")]
    ProcessFailed(Option<i32>, String),
    #[error("Erreur JSON-RPC retournée par le plugin (code {code}) : {message}")]
    JsonRpc { code: i64, message: String },
    #[error("Capability {0} non disponible pour le plugin {1}")]
    CapabilityUnsupported(String, String),
}
