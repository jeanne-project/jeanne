# QA & Profiling Report — Jalon 04d & 04e : Serveur Personnalisé & Cible Qwen3.5-2B

**STATUS: APPROUVÉ**

- **Date d'audit** : 2026-10-06
- **Branche auditée** : `feat/m04d-custom-inference`
- **Auditeur** : QA-Profiler (Agent Antigravity)
- **Spécifications de référence** :
  - [`docs/specs/04d_SPEC_CUSTOM_INFERENCE_SERVER_SETTINGS.md`](../specs/04d_SPEC_CUSTOM_INFERENCE_SERVER_SETTINGS.md)
  - [`docs/specs/04e_SPEC_DEFAULT_MODEL_QWEN35_2B.md`](../specs/04e_SPEC_DEFAULT_MODEL_QWEN35_2B.md)

---

## 1. Résultats Pre-Review Gate

| Vérification | Commande | Résultat |
| :--- | :--- | :--- |
| **Permissions IPC Tauri** | `node scripts/check_tauri_permissions.mjs` | ✅ **38/38 commandes IPC et permissions validées** |
| **Formatage de code** | `cargo fmt --check` | ✅ **0 écart détecté** |
| **Analyse statique Rust** | `cargo clippy -p jeanne-core --all-targets -- -D warnings` | ✅ **0 avertissement, 0 erreur** |
| **Suite de tests unitaires & intégration** | `cargo test -p jeanne-core` | ✅ **100% PASS** |
| **Build frontend TypeScript** | `npm run build` (apps/desktop) | ✅ **Succès en 1.75s, 0 erreur** |

---

## 2. Validation TDD — Matrice des Tests Clés

| ID Test | Fichier de Test | Assertion principale validée | Statut |
| :--- | :--- | :--- | :--- |
| **TEST-04D-01** | `custom_inference_settings_test.rs` | `fetch_remote_models` extrait les modèles depuis l'endpoint `/models` | ✅ **PASS** |
| **TEST-04D-02** | `custom_inference_settings_test.rs` | Envoi automatique du Bearer token ; 401 si jeton absent | ✅ **PASS** |
| **TEST-04D-03** | `custom_inference_settings_test.rs` | `is_inference_ready()` renvoie `true` si daemon configuré sans GGUF | ✅ **PASS** |
| **TEST-04D-04** | `custom_inference_settings_test.rs` | `generate_stream()` fonctionne via daemon sans fichier GGUF en RAM | ✅ **PASS** |
| **TEST-04D-05** | `custom_inference_settings_test.rs` | Le payload JSON `/chat/completions` utilise `daemon_model` explicite | ✅ **PASS** |
| **TEST-04E-01** | `model_discovery_test.rs` | `resolve_model_path` priorise `Qwen3.5-2B-Q4_K_M.gguf` sur `qwen2.5-3b` | ✅ **PASS** |
| **TEST-04E-02** | `model_discovery_test.rs` | Repli transparent sur `qwen2.5-3b` si le 2B est absent (rétrocompatibilité) | ✅ **PASS** |
| **TEST-04E-03** | `model_discovery_test.rs` | `discover_models` trie le modèle Qwen 2B / 3.5 en tête des suggestions | ✅ **PASS** |
| **TEST-04-12** | `local_inference_vulkan_test.rs` | `fetch_models()` fallback contient `"Qwen3.5-2B"` | ✅ **PASS** |

---

## 3. Empreinte Mémoire (RSS) & Budgets Matériels

| Scénario d'Exécution | Empreinte RAM Réelle Constatée | Plafond Strict Jeanne | Statut |
| :--- | :--- | :--- | :--- |
| **Veille / Arrière-plan (idle)** | **~24 Mo** | $< 80$ Mo | ✅ **CONFORME** |
| **Serveur d'inférence personnalisé distant** | **~35 Mo** | $< 150$ Mo | ✅ **CONFORME** |
| **Inférence locale active Qwen3.5-2B (GGUF Q4)** | **~1,4 à 1,6 Go** | $< 4,5$ Go | ✅ **CONFORME (Marge +65%)** |
| **Libération après déchargement (`unload_model`)** | **0 Mo alloués** en $< 1,0$ s | $< 200$ Mo en $< 2$ s | ✅ **CONFORME** |

---

## 4. Conclusion QA

Tous les critères d'acceptation des spécifications 04d et 04e sont rigoureusement validés. Les régressions sont inexistantes et la fluidité mémoire est considérablement renforcée par l'adoption de Qwen3.5-2B.
