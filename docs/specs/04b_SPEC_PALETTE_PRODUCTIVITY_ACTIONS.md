# Spécification Technique : Suite de Productivité & Actions Rapides de la Palette (04b_SPEC_PALETTE_PRODUCTIVITY_ACTIONS.md)

Ce document formalise les extensions fonctionnelles de la palette d'accès rapide (`Alt + Espace`) de **Jeanne** pour le quotidien, conformément aux principes "File-over-App" et aux invariants de `AGENTS.md`.

---

## 1. Vue d'Ensemble & Objectif
* **Identifiant & Titre** : Tranche 04b — Suite de Productivité & Commandes Rapides de la Palette.
* **Problématique résolue** : Transformer la palette d'accès rapide d'une simple recherche/prise de note (`/note`) en un véritable centre de commande quotidien tout-en-un sans latence (< 50 ms) :
  1. **Tâches & Organisation** : `/todo`, `/tasks`, `/log`, `/meeting`.
  2. **Utilitaires Instantanés (0 ms, sans LLM)** : Calculatrice arithmétique inline, `/timer`, `/clip` (presse-papier), `/snip` (snippets).
  3. **Micro-Assistants IA Presse-Papier** : `/corrige`, `/rephrase`, `/tldr`, `/trad` (exploitant le modèle local 3B ou distant).
  4. **Connaissances & Capture** : `/bookmark`, `/scratch`, `/ask` (RAG direct).
* **Plafond Matériel** :
  - Opérations utilitaires et calcul : 0 ms de délai perçu, < 5 Mo d'allocation mémoire vive.
  - Actions IA : respect strict des quotas existants (Jalon 3 distant et Jalon 4 local).
* **Philosophie "File-over-App"** :
  - Tout ajout ou modification s'effectue directement dans des fichiers Markdown en clair (`Inbox.md`, `Journal/YYYY-MM-DD.md`, `Reunions/`, `Ressources/Bookmarks.md`, `Templates/Snippets.md`).
  - Zéro base propriétaire fermée.

---

## 2. Modèles de Données & Contrats d'Interface

### 2.1 Structures du Domaine Rust (`crates/core/src/productivity.rs`)

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskItem {
    pub file_path: String,
    pub line_number: usize,
    pub content: String,
    pub checked: bool,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnippetItem {
    pub key: String,
    pub title: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MathEvaluationResult {
    pub expression: String,
    pub result: f64,
    pub formatted: String,
}

pub enum AiClipAction {
    Corrige,
    Rephrase(String),
    Tldr,
    Trad(String),
}
```

### 2.2 Contrats IPC Tauri v2 (`apps/desktop/src-tauri/src/lib.rs`)

```rust
// Tâches
#[tauri::command]
async fn execute_todo(state: tauri::State<'_, AppState>, content: String) -> Result<String, String>;

#[tauri::command]
async fn get_vault_tasks(state: tauri::State<'_, AppState>, limit: Option<usize>) -> Result<Vec<TaskItem>, String>;

#[tauri::command]
async fn toggle_vault_task(state: tauri::State<'_, AppState>, file_path: String, line_number: usize, checked: bool) -> Result<(), String>;

// Journal & Réunions
#[tauri::command]
async fn execute_log(state: tauri::State<'_, AppState>, content: String) -> Result<String, String>;

#[tauri::command]
async fn execute_meeting(state: tauri::State<'_, AppState>, title: String) -> Result<String, String>;

// Signets & Snippets
#[tauri::command]
async fn execute_bookmark(state: tauri::State<'_, AppState>, url: String, comment: Option<String>) -> Result<String, String>;

#[tauri::command]
async fn get_snippets(state: tauri::State<'_, AppState>) -> Result<Vec<SnippetItem>, String>;

// Calculatrice
#[tauri::command]
fn evaluate_math(expression: String) -> Result<f64, String>;

// IA Presse-papier
#[tauri::command]
async fn ai_process_clipboard(
    state: tauri::State<'_, AppState>,
    action: String,
    text: String,
    param: Option<String>
) -> Result<String, String>;

// RAG Direct
#[tauri::command]
async fn ask_vault(
    state: tauri::State<'_, AppState>,
    question: String
) -> Result<String, String>;
```

### 2.3 Contrats TypeScript (`apps/desktop/src/lib/types/ipc.ts`)

```typescript
export interface TaskItem {
  file_path: string;
  line_number: number;
  content: string;
  checked: boolean;
  created_at?: string;
}

export interface SnippetItem {
  key: string;
  title: string;
  content: string;
}
```

---

## 3. Scénarios & Cas Limites

1. **Calculatrice Inline** : Détecte automatiquement si la requête de l'utilisateur correspond à une expression arithmétique basique (`+`, `-`, `*`, `/`, `^`, `%`, `()`, nombres à virgule avec point `.` ou virgule `,`). Nettoie systématiquement les artefacts de précision binaire IEEE-754 (ex: `1.2 * 56.4` donne exactement `67.68` et non `67.67999999999999`). En cas d'erreur de syntaxe ou division par zéro, la calculatrice n'affiche rien et laisse la recherche classique s'exécuter sans bloquer.
2. **Gestion des Fichiers Manquants** : Si `Inbox.md`, `Journal/`, `Reunions/` ou `Ressources/Bookmarks.md` n'existent pas lors d'une action `/todo`, `/log` ou `/bookmark`, ils sont créés automatiquement avec un en-tête Markdown et frontmatter YAML standardisé.
3. **Absence de Modèle Local pour les Actions IA** : Si l'utilisateur invoque `/corrige` ou `/ask` sans que le modèle local 3B ne soit chargé et sans clé API distante, une erreur explicite est renvoyée invitant à charger le modèle local dans le dashboard.

---

## 4. Matrice de Tests d'Acceptation (TDD)

| ID Test | Composant | Action | Résultat Attendu |
| :--- | :--- | :--- | :--- |
| **TEST-PROD-01** | `evaluate_math_expression` | Évaluer `"12 * 4.5"` | Retourne `Ok(54.0)` |
| **TEST-PROD-01b** | `evaluate_math_expression` | Évaluer `"1.2 * 56.4"` et `"1,2 * 56,4"` | Retourne `Ok(67.68)` sans dérive float |
| **TEST-PROD-02** | `evaluate_math_expression` | Évaluer `"((10 + 20) * 3) / 2"` | Retourne `Ok(45.0)` |
| **TEST-PROD-03** | `evaluate_math_expression` | Évaluer du texte non mathématique | Retourne `Err(...)` |
| **TEST-PROD-04** | `append_todo` | Ajouter `"Acheter des câbles"` dans `Inbox.md` | Ligne `- [ ] [HH:MM] Acheter des câbles` ajoutée avec frontmatter |
| **TEST-PROD-05** | `parse_and_toggle_tasks` | Trouver `- [ ]` et le basculer en `- [x]` | Fichier mis à jour avec `- [x]` préservant le reste du contenu |
| **TEST-PROD-06** | `append_log_entry` | Ajouter `"Appel client"` au journal du jour | Ligne `- **HH:MM** : Appel client` ajoutée dans `Journal/YYYY-MM-DD.md` |
| **TEST-PROD-07** | `create_meeting_note` | Créer note de réunion `"Sprint Review"` | Fichier `Reunions/YYYY-MM-DD - Sprint Review.md` créé avec template |
| **TEST-PROD-08** | `append_bookmark` | Ajouter `"https://rust-lang.org"` avec commentaire | Ajouté dans `Ressources/Bookmarks.md` formaté en markdown |

---

## 5. Critères de Validation & Clôture
- Zéro `unwrap()` / `expect()` dans le code Rust de production (`crates/core`).
- Tests unitaires et d'intégration verts sous `crates/core/tests/productivity_actions_test.rs`.
- `cargo clippy --workspace --all-targets -- -D warnings` sans erreur.
- `npm run build` frontend sans erreur TypeScript.
