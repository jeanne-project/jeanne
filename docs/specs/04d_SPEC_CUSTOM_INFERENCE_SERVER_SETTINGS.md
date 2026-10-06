# Spécification Technique : Serveur d'Inférence Personnalisé, Découverte Dynamique des Modèles et Découplage GGUF (04d_SPEC_CUSTOM_INFERENCE_SERVER_SETTINGS.md)

Ce document formalise la spécification technique pour la prise en charge complète des serveurs d'inférence personnalisés (Ollama, LM Studio, vLLM, llama-server, proxies d'entreprise compatibles OpenAI), la sélection explicite du modèle distant via menu déroulant, la gestion des clés API d'authentification, et le découplage strict évitant le chargement obligatoire d'un modèle GGUF local.

---

## 1. Vue d'Ensemble & Objectifs

### 1.1 Problème Identifié
1. **Sélection opaque du modèle distant** : La fonction `detect_daemon_model_name()` sélectionnait un modèle arbitraire (nom du GGUF local si présent, sinon recherche de "qwen", sinon premier élément, sinon `"qwen2.5:3b"` codé en dur). L'utilisateur n'avait aucun moyen de choisir son modèle ni de voir ce qui était utilisé.
2. **Blocage arbitraire sans modèle GGUF local** : `generate_stream()` dans `LocalLlmEngine` échouait immédiatement avec `LlmError::ModelNotLoaded` si `is_model_loaded()` était faux, même si un serveur d'inférence personnalisé était configuré et disponible. De même, les commandes IPC `ai_process_clipboard` et `ask_vault` refusaient de générer une réponse sans modèle local en RAM.
3. **Absence de clé d'API (`api_key`)** : `LocalEngineConfig` ne possédait aucun champ pour configurer un jeton d'authentification, empêchant l'utilisation d'endpoints distants ou sécurisés nécessitant un en-tête `Authorization: Bearer <key>`.
4. **Absence de découverte interactive dans l'interface** : Aucun bouton permettant de tester l'URL et d'interroger `/models`, ni de menu déroulant pour sélectionner le modèle distant.

### 1.2 Objectifs de la Tranche
1. **Support complet de l'authentification et de la sélection de modèle distant** :
   - Ajout des champs `daemon_api_key: Option<String>` et `daemon_model: Option<String>` dans `LocalEngineConfig`.
   - Transmission automatique du header `Authorization: Bearer <daemon_api_key>` lors des requêtes HTTP vers le serveur (`/models` et `/chat/completions`).
2. **Découverte dynamique des modèles sur le serveur distant** :
   - Commande IPC Tauri `fetch_remote_server_models(endpoint: String, api_key: Option<String>) -> Result<Vec<String>, String>` interrogeant `{endpoint}/models` conformément au standard OpenAI.
   - Interface utilisateur avec bouton interactif « Récupérer les modèles » et menu déroulant `<select>` alimenté dynamiquement.
3. **Découplage strict de l'obligation de modèle GGUF local** :
   - Introduction de `is_inference_ready()` dans le moteur : l'inférence est prête si un modèle GGUF local est en mémoire OU si un serveur personnalisé valide (`daemon_endpoint`) est configuré.
   - Autorisation de `generate_stream()` sans GGUF local dès lors qu'un serveur personnalisé est défini.
   - Mise à jour de `ask_vault` et `ai_process_clipboard` pour utiliser `is_inference_ready()`.
   - Adaptation du tableau de bord UI : statut vert « Serveur Personnalisé Actif » sans incitation erronée à charger un fichier GGUF.
4. **Plafond matériel & frugalité** :
   - En mode serveur personnalisé distant/externe, l'application fonctionne avec son empreinte minimale résidente (**< 150 Mo RAM**), préservant l'intégralité de la mémoire pour l'OS et les autres tâches.

---

## 2. Modèles de Données & Contrats d'Interface

### 2.1 Structures du Domaine Rust (`crates/core/src/local_llm.rs`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LocalEngineConfig {
    pub model_path: Option<String>,
    pub context_size: u32,
    pub threads: Option<u32>,
    pub use_vulkan: bool,
    pub use_gpu: bool,
    pub gpu_layers: Option<u32>,
    pub generation_timeout_secs: u64,
    pub temperature: f32,
    pub max_tokens: u32,
    pub top_p: Option<f32>,
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

impl LocalLlmEngine {
    /// Indique si le moteur est prêt pour l'inférence (modèle GGUF en RAM OU serveur personnalisé configuré).
    pub async fn is_inference_ready(&self) -> bool;

    /// Récupère la liste des modèles disponibles sur un endpoint OpenAI-compatible distant.
    pub async fn fetch_remote_models(
        endpoint: &str,
        api_key: Option<&str>,
    ) -> Result<Vec<String>, LlmError>;
}
```

### 2.2 Contrats IPC Tauri v2 (`apps/desktop/src-tauri/src/lib.rs`)

```rust
#[tauri::command]
pub async fn fetch_remote_server_models(
    endpoint: String,
    api_key: Option<String>,
) -> Result<Vec<String>, String>;

#[tauri::command]
pub async fn is_inference_ready(
    state: tauri::State<'_, AppState>,
) -> Result<bool, String>;
```

### 2.3 Permissions Tauri (`permissions/autogenerated/` et `capabilities/default.json`)

Fichier `apps/desktop/src-tauri/permissions/autogenerated/commands/fetch_remote_server_models.toml` :
```toml
[[permission]]
identifier = "allow-fetch-remote-server-models"
description = "Permet d'interroger la liste des modèles d'un serveur d'inférence distant."
commands.allow = ["fetch_remote_server_models"]
```

Fichier `apps/desktop/src-tauri/permissions/autogenerated/commands/is_inference_ready.toml` :
```toml
[[permission]]
identifier = "allow-is-inference-ready"
description = "Permet de vérifier si le moteur d'inférence est prêt (local ou serveur personnalisé)."
commands.allow = ["is_inference_ready"]
```

Et ajout dans `apps/desktop/src-tauri/capabilities/default.json` :
- `"allow-fetch-remote-server-models"`
- `"allow-is-inference-ready"`

### 2.4 Contrats Frontend TypeScript (`apps/desktop/src/lib/types/ipc.ts`)

```typescript
export interface LocalEngineConfig {
  model_path?: string | null;
  context_size: number;
  threads?: number | null;
  use_vulkan: boolean;
  use_gpu: boolean;
  gpu_layers?: number | null;
  generation_timeout_secs: number;
  temperature: number;
  top_p?: number | null;
  top_k?: number | null;
  max_tokens: number;
  allow_extended_context?: boolean;
  daemon_endpoint?: string | null;
  daemon_api_key?: string | null;
  daemon_model?: string | null;
  expected_sha256?: string | null;
}
```

---

## 3. Scénarios & Cas Limites

### 3.1 Scénario Nominal : Découverte et Sélection de Modèle Serveur
1. L'utilisateur ouvre les Paramètres (⚙️) et saisit `http://127.0.0.1:11434/v1` (ou l'URL de son serveur vLLM/LM Studio).
2. Si le serveur requiert une clé, l'utilisateur saisit son token dans le champ « Clé d'API ».
3. L'utilisateur clique sur **« 🔄 Récupérer les modèles »**.
4. Le frontend appelle `fetch_remote_server_models(endpoint, api_key)`.
5. Le backend effectue une requête `GET {endpoint}/models` avec l'en-tête `Authorization: Bearer <key>` et un timeout de 3 secondes.
6. La liste des identifiants (`["qwen2.5:7b-instruct", "mistral:latest", ...]`) est renvoyée et alimente un `<select>`.
7. L'utilisateur choisit son modèle dans le menu déroulant.
8. Les paramètres sont enregistrés (`saveEngineConfig`).
9. L'accueil et la barre d'accès rapide utilisent directement ce modèle pour toute action IA.

### 3.2 Scénario Sans Fichier GGUF Local
1. L'utilisateur n'a aucun fichier `.gguf` téléchargé dans `models/`.
2. Un serveur personnalisé est configuré avec un modèle sélectionné.
3. Le tableau de bord affiche le statut d'inférence : `🟢 Serveur Personnalisé Actif (qwen2.5:7b-instruct)`.
4. L'utilisateur déclenche une action rapide (ex: `/corrige` ou `/ask`).
5. `is_inference_ready()` retourne `true`.
6. L'inférence s'exécute avec succès via le serveur distant sans aucune erreur `ModelNotLoaded`.

### 3.3 Scénario d'Erreur Réseau ou d'Authentification
1. L'URL est injoignable ou l'API key est invalide (401 Unauthorized).
2. `fetch_remote_server_models()` retourne un message d'erreur clair et contextualisé à l'UI.
3. L'UI affiche une notification d'erreur sans crasher et permet la saisie manuelle libre du nom de modèle si l'API `/models` n'est pas exposée par le serveur.

---

## 4. Matrice de Tests (Phase Rouge -> Phase Verte)

| ID | Test | Description | Comportement Attendu |
| :--- | :--- | :--- | :--- |
| **TEST-04D-01** | `test_fetch_remote_models_success` | Mock HTTP server répondant sur `/models` avec liste JSON OpenAI | Retourne les identifiants de modèles `["model-a", "model-b"]` |
| **TEST-04D-02** | `test_fetch_remote_models_with_bearer_token` | Mock HTTP server exigeant `Authorization: Bearer test_secret` | Requête acceptée si le token est fourni, rejetée avec `LlmError::Auth` si absent ou invalide |
| **TEST-04D-03** | `test_is_inference_ready_local_vs_daemon` | Test unitaire de `is_inference_ready()` | `false` si aucun GGUF et aucun daemon, `true` si GGUF chargé, `true` si `daemon_endpoint` configuré |
| **TEST-04D-04** | `test_generate_stream_via_daemon_without_gguf` | `generate_stream()` appelé sans aucun modèle GGUF chargé en RAM | Tente le streaming via le daemon mocké au lieu de renvoyer immédiatement `ModelNotLoaded` |
| **TEST-04D-05** | `test_daemon_model_priority` | Définition de `daemon_model: Some("custom-qwen-7b")` | Le payload JSON envoyé à `/chat/completions` contient `"model": "custom-qwen-7b"` |

---

## 5. Budgets Matériels & Invariants
* **Empreinte RAM en mode serveur personnalisé** : **< 150 Mo RAM** (aucun modèle GGUF chargé en mémoire vive locale).
* **Zéro Dépendance Superflue** : Réutilisation de `reqwest` et `serde_json` déjà présents dans `crates/core`.
* **Sécurité & Zéro-Panic** : Pas d'`unwrap()` dans le code de parsing ou de requêtage réseau.
