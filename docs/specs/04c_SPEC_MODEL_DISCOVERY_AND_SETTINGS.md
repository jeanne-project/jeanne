# Spécification Technique : Détection Multi-Modèles & Menu Paramètres (04c_SPEC_MODEL_DISCOVERY_AND_SETTINGS.md)

Ce document formalise la spécification technique pour la détection dynamique des modèles GGUF locaux, la résolution multi-dossiers et l'interface de gestion des paramètres de **Jeanne**, conformément aux principes "File-over-App" et aux invariants de `AGENTS.md`.

---

## 1. Vue d'Ensemble & Analyse du Problème

### 1.1 Problème Identifié
Lorsqu'un utilisateur télécharge un modèle GGUF (ex. `qwen2.5-3b-instruct-q4_k_m.gguf`) et le dépose dans le dossier `models` du projet (`D:\sources\Jeanne\jeanne\models\`), le modèle n'est pas détecté ni chargé pour deux raisons majeures :
1. **Résolution rigide du répertoire** : `resolve_default_model_dir()` ciblait uniquement le chemin système (`%APPDATA%\Jeanne\models` sous Windows ou `~/.local/share/jeanne/models` sous Linux), ignorant le dossier local `./models` du répertoire de travail ou de la racine du dépôt.
2. **Sensibilité à la casse & nommage strict** : Le code cherchait exclusivement le fichier avec la casse exacte `"Qwen2.5-3B-Instruct-Q4_K_M.gguf"`. Or le lien direct Hugging Face télécharge un fichier nommé en minuscules `"qwen2.5-3b-instruct-q4_k_m.gguf"`.
3. **Absence de multi-modèles et de paramétrage** : Le système ne proposait aucune découverte automatique des fichiers `.gguf` présents, ni d'interface utilisateur pour basculer entre différents modèles selon la puissance de la machine de l'utilisateur (ex. 1.5B, 3B, 7B).

### 1.2 Objectifs de la Tranche
1. **Résolution multi-dossiers intelligente** : Scanner dans l'ordre de priorité :
   - Variable d'environnement `JEANNE_MODELS_DIR` (si définie)
   - Dossier `./models` relatif au répertoire d'exécution (dépôt / binaire)
   - Dossier système OS (`%APPDATA%\Jeanne\models` ou `~/.local/share/jeanne/models`)
2. **Détection dynamique des modèles GGUF** : Scanner tous les fichiers `.gguf` (insensible à la casse), extraire leurs métadonnées (taille sur disque, architecture, compatibilité RAM) et déterminer si un modèle est actuellement chargé.
3. **Résolution tolérante du modèle par défaut** : Si aucun modèle spécifique n'est demandé, chercher en priorité tout modèle Qwen 3B (quelle que soit la casse) ou le premier modèle `.gguf` valide présent dans les dossiers candidats.
4. **Menu Paramètres (UI Svelte 5 Runes)** :
   - Accessible depuis l'en-tête (icône engrenage ⚙️) et la carte d'inférence locale.
   - Liste interactive de tous les modèles `.gguf` détectés dans le dossier `models`.
   - Affichage des caractéristiques de chaque modèle (nom, taille formatée, architecture, statut mémoire).
   - Bouton de chargement immédiat du modèle sélectionné.
   - Bouton de rafraîchissement (rescan) et copie du chemin du dossier `models`.
   - Recommandations matérielles adaptées (RAM disponible, accélération Vulkan).

---

## 2. Modèles de Données & Contrats d'Interface

### 2.1 Structures du Domaine Rust (`crates/core/src/model_discovery.rs`)

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveredModel {
    /// Nom du fichier (ex: "qwen2.5-3b-instruct-q4_k_m.gguf")
    pub name: String,
    /// Chemin absolu complet
    pub path: String,
    /// Taille en octets
    pub size_bytes: u64,
    /// Taille formatée lisible (ex: "2.10 Go")
    pub size_formatted: String,
    /// Architecture GGUF détectée (ex: "qwen2", "llama", etc.)
    pub architecture: Option<String>,
    /// Indique si ce modèle est actuellement chargé en mémoire
    pub is_loaded: bool,
    /// Indique si le modèle est compatible avec le budget mémoire recommandé (ex: <= 4.5 Go)
    pub fits_ram: bool,
}

/// Résout les dossiers candidats où chercher des modèles GGUF.
pub fn get_candidate_model_dirs() -> Vec<PathBuf>;

/// Découvre tous les modèles GGUF présents dans les répertoires candidats.
pub fn discover_models(currently_loaded_path: Option<&str>) -> Vec<DiscoveredModel>;

/// Résout le chemin d'un modèle demandé ou cherche le meilleur modèle disponible.
pub fn resolve_model_path(requested: Option<&str>) -> Option<PathBuf>;
```

### 2.2 Contrats IPC Tauri v2 (`apps/desktop/src-tauri/src/lib.rs`)

```rust
#[tauri::command]
async fn list_available_models(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<DiscoveredModel>, String>;

#[tauri::command]
fn get_models_directory() -> String;

#[tauri::command]
async fn load_local_model(
    state: tauri::State<'_, AppState>,
    model_path: Option<String>,
) -> Result<(), String>;
```

### 2.3 Contrat Frontend TypeScript (`apps/desktop/src/lib/types/ipc.ts`)

```typescript
export interface DiscoveredModel {
  name: string;
  path: string;
  size_bytes: number;
  size_formatted: string;
  architecture?: string;
  is_loaded: boolean;
  fits_ram: bool;
}

export interface IpcCommands {
  // ... existantes
  list_available_models(): Promise<DiscoveredModel[]>;
  get_models_directory(): Promise<string>;
}
```

---

## 3. Matrice de Tests (Phase Rouge -> Phase Verte)

| ID | Test | Description | Comportement Attendu |
| :--- | :--- | :--- | :--- |
| **TEST-04C-01** | `test_candidate_model_dirs` | Vérifie que `./models` et le chemin OS sont dans les candidats | Au moins un chemin valide retourné |
| **TEST-04C-02** | `test_discover_models_finds_gguf` | Dossier temporaire avec fichier `.gguf` et fichier `.txt` | Seul le `.gguf` est détecté avec taille et nom |
| **TEST-04C-03** | `test_discover_models_case_insensitivity` | Fichiers `.gguf` et `.GGUF` en minuscules/majuscules | Tous les modèles GGUF sont détectés |
| **TEST-04C-04** | `test_resolve_model_path_fallback_qwen` | Dossier temporaire avec `qwen2.5-3b-instruct-q4_k_m.gguf` | Trouve le fichier même si `requested` est `None` |
| **TEST-04C-05** | `test_resolve_model_path_by_name` | Recherche par nom exact ou nom sans chemin | Chemin complet résolu avec succès |
| **TEST-04C-06** | `test_load_local_model_with_discovered_model` | Chargement dans `LocalLlmEngine` via chemin résolu | Modèle chargé avec succès, `is_model_loaded() == true` |

---

## 4. Budgets Matériels & Invariants
* **Empreinte mémoire** : Le scan de répertoires utilise exclusivement des itérateurs légers `std::fs::read_dir`, sans charger les fichiers en mémoire.
* **Sécurité & Zéro-Panic** : Toute erreur de lecture d'un fichier défectueux est interceptée gracieusement sans paniquer.
* **Temps de réponse UI** : `list_available_models` doit s'exécuter en moins de 15 ms pour un dossier de modèles standard.
