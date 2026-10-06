# Spécification Technique : Modèle Local par Défaut Qwen3.5-2B (04e_SPEC_DEFAULT_MODEL_QWEN35_2B.md)

Ce document formalise la spécification technique pour la transition du modèle d'inférence locale par défaut de **Jeanne** vers **Qwen3.5-2B** (quantification `Q4_K_M` issue du dépôt Unsloth), en remplacement de Qwen2.5-3B.

---

## 1. Vue d'Ensemble & Objectifs

### 1.1 Contexte & Justification
Les benchmarks récents démontrent que **Qwen3.5-2B** surpasse Qwen2.5-3B en compréhension contextuelle, fidélité d'extraction et vitesse de génération tout en réduisant la taille binaire de **~2.1 Go à ~1.28 Go** (-39% d'empreinte disque et mémoire). Cette réduction drastique renforce l'invariant de frugalité de Jeanne (< 4.5 Go de RAM en inférence active, idéale pour machines 8 Go / 16 Go avec iGPU partagé).

### 1.2 Objectifs de la Tranche
1. **Cible par défaut standardisée** :
   - Fichier cible : `Qwen3.5-2B-Q4_K_M.gguf`
   - Dépôt Hugging Face : `https://huggingface.co/unsloth/Qwen3.5-2B-GGUF`
   - URL de téléchargement direct : `https://huggingface.co/unsloth/Qwen3.5-2B-GGUF/resolve/main/Qwen3.5-2B-Q4_K_M.gguf`
   - Taille binaire : ~1.28 Go (1 280 835 840 octets).
2. **Priorisation algorithmique de découverte** :
   - `resolve_model_path_in_dirs(None, ...)` privilégie en priorité absolue tout fichier GGUF contenant `qwen` et (`2b` ou `3.5`).
   - Rétrocompatibilité totale : si un utilisateur possède déjà `qwen2.5-3b-instruct-q4_k_m.gguf`, il est résolu en priorité secondaire avant les modèles tiers.
   - Le tri dans `discover_models()` place les modèles Qwen 2B / 3.5 en tête des suggestions.
3. **Mise à jour des fallbacks noyau et IPC** :
   - `LocalLlmEngine::fetch_models()` renvoie `"Qwen3.5-2B-Q4_K_M.gguf"` en cas d'absence de modèle physique.
   - `get_default_model_path()` dans Tauri Desktop pointe vers `models/Qwen3.5-2B-Q4_K_M.gguf`.
   - `detect_daemon_model_name()` utilise `"qwen3.5:2b"` comme repli d'inférence distante par défaut.
4. **Mise à jour de l'UI Svelte 5 et des guides** :
   - Liens directs de téléchargement Hugging Face mis à jour vers le dépôt Unsloth.
   - Textes de configuration, estimations de taille (~1.28 Go) et mentions d'aide adaptés.
   - Plugin runner (`plugins/llm-runner`) synchronisé sur `Qwen3.5-2B-Q4_K_M.gguf`.

---

## 2. Contrats d'Interface & Modèles de Données

### 2.1 Résolution et Découverte (`crates/core/src/model_discovery.rs`)

```rust
// Ordre de priorité de résolution lorsque requested = None :
// 1. Modèle contenant "qwen" ET ("2b" OU "3.5") (Priorité absolue)
// 2. Modèle contenant "qwen" ET "3b" (Rétrocompatibilité Qwen 2.5)
// 3. Tout modèle contenant "qwen"
// 4. Premier modèle .gguf disponible dans les dossiers candidats
```

### 2.2 Hyperparamètres Recommandés (`crates/core/src/local_llm.rs`)

```rust
ModelRecommendedParams {
    context_size: Some(32768), // Natif jusqu'à 32k tokens
    temperature: Some(0.7),
    top_p: Some(0.8),
    top_k: Some(20),
}
```

---

## 3. Matrice de Tests TDD (Phase Rouge -> Phase Verte)

| ID | Test | Description | Comportement Attendu |
| :--- | :--- | :--- | :--- |
| **TEST-04E-01** | `test_resolve_model_path_prioritizes_qwen35_2b` | Dossier avec `Qwen3.5-2B-Q4_K_M.gguf` et `qwen2.5-3b-instruct-q4_k_m.gguf` | `resolve_model_path(None)` sélectionne impérativement le 2B |
| **TEST-04E-02** | `test_resolve_model_path_fallback_qwen25_compat` | Dossier uniquement avec `qwen2.5-3b-instruct-q4_k_m.gguf` | Résout le 3B sans régression |
| **TEST-04E-03** | `test_discover_models_orders_qwen2b_first` | Découverte avec modèles multiples (Llama, Qwen 3B, Qwen 2B) | Qwen 2B / 3.5 est positionné en premier |
| **TEST-04E-04** | `test_fetch_models_default_qwen35` | `LocalLlmEngine::fetch_models()` sans fichier sur disque | Retourne `["Qwen3.5-2B-Q4_K_M.gguf"]` |
| **TEST-04E-05** | `test_default_model_path_resolution` | `get_default_model_path` fallback | Finit par `Qwen3.5-2B-Q4_K_M.gguf` |

---

## 4. Budgets Matériels & Invariants

* **Taille fichier** : ~1.28 Go (vs ~2.1 Go pour Qwen2.5-3B).
* **RAM Inférence active** : $\le 1.8$ Go sur iGPU Vulkan / CPU.
* **RAM Veille** : $< 80$ Mo après déchargement (`unload_model()`).
* **Vitesse cible** : $\ge 25$ tokens/seconde sur processeur moderne avec Vulkan.
