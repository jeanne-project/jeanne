# 07b - Spécification Technique : Écosystème de Plugins Jeanne & Runners Asynchrones

## 1. Vue d'Ensemble & Objectifs Architecturaux
* **Identifiant & Titre** : Spécification 07b — Écosystème de Plugins Déportés (Inférence Locale, Embeddings RAG, STT, TTS & Parsers).
* **Problématique** :
  Le noyau de Jeanne (`crates/core`) doit impérativement respecter la règle **Zero-Core-Bloat** ([`AGENTS.md`](file:///home/runner/antigravity/jeanne/AGENTS.md)) et l'invariant de mémoire vive (< 200 Mo au repos sur PC 16 Go).
  L'intégration directe de bibliothèques lourdes C++ ou de moteurs neuronaux dans le binaire principal présente un risque majeur de crash mémoire et de lourdeur de compilation.
  Cette spécification formalise le cadre complet permettant d'exécuter l'inférence locale (LLM), la vectorisation sémantique (`sqlite-vec`), la transcription vocale (STT) et la synthèse vocale (TTS) sous forme de **sous-processus indépendants et cloisonnés communiquant en JSON-RPC 2.0 sur `stdio`**.
* **Plafonds Matériels & Contraintes Mémoire** :
  * Processus principal Jeanne : strictly $\le$ **200 Mo RAM**.
  * Sous-processus d'inférence LLM local (`llm-runner`) : strictly $\le$ **4,5 Go RAM** en charge, $< 50$ Mo déchargé.
  * Sous-processus d'embeddings (`embeddings`) : strictly $\le$ **150 Mo RAM**.
  * Sous-processus vocaux (`whisper` / `piper`) : strictly $\le$ **250 Mo RAM** cumulés en écoute active, **0 Mo** à l'arrêt.
  * Zéro processus zombie (`kill()` + `wait()` systématique via RAII et Watchdog Tokio).

---

## 2. Modèles de Données & Contrats Typés Côté Rust (`crates/core`)

### 2.1 Manifeste de Plugin (`PluginManifest`)
```rust
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PluginLifecycle {
    #[serde(rename = "on_demand")]
    OnDemand,
    #[serde(rename = "daemon_managed")]
    DaemonManaged,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum PluginCapability {
    #[serde(rename = "llm_runner")]
    LlmRunner {
        default_model: String,
        supported_formats: Vec<String>,
        max_context_size: u32,
        supports_streaming: bool,
    },
    #[serde(rename = "embeddings_generator")]
    EmbeddingsGenerator {
        default_model: String,
        default_dimension: usize,
        max_sequence_length: usize,
        supports_batching: bool,
    },
    #[serde(rename = "voice_stt")]
    VoiceStt {
        default_model: String,
        input_format: String,
        supported_languages: Vec<String>,
    },
    #[serde(rename = "voice_tts")]
    VoiceTts {
        default_voice: String,
        output_format: String,
        streaming_sentence_level: bool,
    },
    #[serde(rename = "document_parser")]
    DocumentParser {
        supported_extensions: Vec<String>,
        supported_mimetypes: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginEntrypoint {
    pub windows: String,
    pub linux: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginManifest {
    pub schema_version: String,
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub entrypoint: PluginEntrypoint,
    pub lifecycle: PluginLifecycle,
    pub timeout_seconds: u64,
    pub capabilities: Vec<PluginCapability>,
    #[serde(skip)]
    pub root_dir: PathBuf,
}
```

---

### 2.2 Protocole JSON-RPC 2.0 Typé
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    pub params: serde_json::Value,
    pub id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub result: Option<serde_json::Value>,
    pub error: Option<JsonRpcError>,
    pub id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    pub params: serde_json::Value,
}
```

---

## 3. Architecture du Gestionnaire & Supervision des Sous-Processus

### 3.1 `PluginManager` (Découverte & Résolution)
Le `PluginManager` scanne les dossiers candidats ordonnés :
1. `./plugins/<id>/plugin.json` (environnement de développement et plugins embarqués)
2. `%APPDATA%/Jeanne/plugins/<id>/` (Windows) ou `~/.local/share/jeanne/plugins/<id>/` (Linux)
3. Variable d'environnement `JEANNE_PLUGINS_DIR`

Il indexe les plugins par `id` et par `capability`.

### 3.2 `PluginProcessRunner` (Exécution Asynchrone & Watchdog)
Pour chaque invocation :
1. `tokio::process::Command` instancie le binaire du plugin avec redirection stricte :
   - `stdin` : `piped` (transmission de la charge JSON-RPC)
   - `stdout` : `piped` (lecture asynchrone des lignes NDJSON)
   - `stderr` : `piped` (télémétrie redirigée vers `tracing::debug!`)
2. Un **Watchdog `tokio::time::timeout`** garantit qu'aucun sous-processus ne dépasse `timeout_seconds`.
3. En cas de dépassement ou d'erreur, `child.kill().await` et `child.wait().await` éliminent impérativement tout processus zombie dans la table des processus du système d'exploitation.

---

## 4. Intégration dans les Sous-Systèmes Jeanne

### 4.1 Inférence Locale LLM (`LocalLlmEngine`)
* Lorsqu'une inférence est demandée (`ai_process_clipboard`, `/corrige`, `/trad`, `/ask`), le moteur délègue la génération à `plugin-llm-runner`.
* Le streaming est consommé ligne par ligne via les notifications `token_chunk` et alimente le canal `mpsc::channel` vers l'interface utilisateur.

### 4.2 Recherche Vectorielle RAG (`RagEngine` & `StorageManager`)
* Lors de la modification ou création d'une note Markdown (captée par `VaultWatcher`), le texte de la note est envoyé à `plugin-embeddings` via `embed_batch`.
* Les vecteurs de dimension 384 retournés sont insérés dans `vec_chunks` via `storage.insert_chunk_vector(rowid, &vector)`.
* Lors d'une question `/ask`, la requête est vectorisée via `embed_text`, permettant à `RagEngine` d'exécuter la recherche hybride dense (cosinus) + BM25 avec le coupe-circuit anti-hallucination.

### 4.3 Sous-Système Audio (`VoicePipeline`)
* `PluginSttEngine` implémente le trait `SttEngine` et délègue à `voice-whisper` (`transcribe_pcm`).
* `PluginTtsEngine` implémente le trait `TtsEngine` et délègue à `voice-piper` (`synthesize_sentence`).

---

## 5. Matrice des Tests d'Acceptation (TDD - Phase 2 Rouge)

| Test ID | Composant cible | Scénario d'entrée | Résultat attendu |
| :--- | :--- | :--- | :--- |
| **TEST-07B-01** | `PluginManager` | Découverte des 5 manifestes `plugin.json` dans `plugins/` | Détection valide des 5 plugins avec typage exact de leurs capabilities respectives |
| **TEST-07B-02** | `PluginProcessRunner` | Exécution d'un ping JSON-RPC vers un sous-processus mock | Réception correcte de la réponse sur `stdout`, code retour 0, descripteurs fermés |
| **TEST-07B-03** | Watchdog & Zombies | Exécution d'un script mock qui boucle indéfiniment | Déclenchement du timeout, envoi SIGTERM/SIGKILL, zéro processus zombie résiduel |
| **TEST-07B-04** | Embeddings RAG | Envoi de 2 phrases à `plugin-embeddings` | Réception de 2 vecteurs normalisés de dimension 384, insertion réussie dans `vec_chunks` |
| **TEST-07B-05** | Streaming LLM | Envoi d'une invite à `plugin-llm-runner` | Réception asynchrone des jetons unitaires jusqu'à `done: true` |
| **TEST-07B-06** | Voix STT / TTS | Envoi d'un extrait audio et d'une phrase textuelle | Transcription fidèle et génération d'un flux audio PCM valide |

---

## 6. Fiches de Tâches & Plan de Travail pour les Sous-Agents

### Agent 1 : `rust-core`
* **Périmètre** : `crates/core/src/plugins/` (nouveau module `plugins`)
* **Tâches** :
  1. Définir les structures `PluginManifest`, `PluginCapability`, `JsonRpcRequest`, `JsonRpcResponse` dans `crates/core/src/plugins/models.rs`.
  2. Implémenter le scanner de manifestes `PluginManager` dans `crates/core/src/plugins/manager.rs`.
  3. Implémenter `PluginProcessRunner` avec Watchdog Tokio et éradication des zombies dans `crates/core/src/plugins/runner.rs`.
  4. Implémenter les adaptateurs de traits :
     - `PluginLlmProvider` implémentant `LlmProvider`
     - `PluginEmbeddingsEngine` connectant `VaultWatcher` à `storage.insert_chunk_vector`
     - `PluginSttEngine` implémentant `SttEngine`
     - `PluginTtsEngine` implémentant `TtsEngine`
  5. Écrire la suite de tests d'intégration `crates/core/tests/plugin_system_test.rs`.

### Agent 2 : `plugin-dev`
* **Périmètre** : `plugins/` (sous-processus autonomes)
* **Tâches** :
  1. `plugins/embeddings` : Implémenter l'exécutable d'embedding (modèle ONNX `all-MiniLM-L6-v2`, support de `config.json` pour interchangeabilité).
  2. `plugins/llm-runner` : Implémenter le runner d'inférence GGUF (encapsulant `llama-server` ou runner C/C++ autonome avec support Vulkan/GPU).
  3. `plugins/voice-whisper` : Implémenter le sous-processus Whisper STT (transcription de flux PCM 16 kHz).
  4. `plugins/voice-piper` : Implémenter le sous-processus Piper TTS (synthèse vocale phrase par phrase).
  5. `plugins/pdf-parser` : Implémenter le parser PDF autonome en Go.

### Agent 3 : `reviewer` & `qa-profiler`
* **Périmètre** : Contrôle qualité, audit statique et profiler mémoire.
* **Tâches** :
  1. Vérification zéro panic (`unwrap`/`expect`) sur le parsing JSON-RPC.
  2. Contrôle de l'absence de fuites de descripteurs de fichiers (`pipe` fermés proprement).
  3. Validation des budgets mémoire RSS (< 200 Mo core, < 4.5 Go LLM, < 150 Mo embeddings).
  4. Rédaction de `docs/reviews/M07_CODE_REVIEW.md` et `docs/reviews/M07_QA_REPORT.md`.
