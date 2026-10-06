# Spécification Technique : Cohérence du Pipeline RAG, Prompt Unifié & Découplage Matériel Serveur (04g_SPEC_RAG_PIPELINE_AND_ENGINE_COHERENCE.md)

Ce document formalise les exigences techniques et d'architecture pour la résolution des incohérences de Priorité 2 (INC-04, INC-05, INC-06) identifiées lors de l'audit architectural.

---

## 1. Contexte & Objectifs

L'audit architectural de Jeanne a mis en évidence trois incohérences majeures concernant les moteurs d'inférence et le pipeline RAG :
1. **INC-04 (Plafonds Matériels Indûment Appliqués)** :
   - `calculate_max_allowed_context` bride arbitrairement le contexte à 4 096 tokens sur les machines $\le 16$ Go de RAM, même lorsque l'inférence est entièrement déportée sur un serveur distant (vLLM, OpenAI).
   - Les champs spécifiques à llama.cpp (`num_gpu`, `num_thread` dans l'objet `options`) sont envoyés à tout serveur HTTP distant, polluant les requêtes envoyées aux API strictes.
2. **INC-05 (Déconnexion de `fetch_models` dans `LlmProvider`)** :
   - L'implémentation de `LlmProvider::fetch_models` pour `LocalLlmEngine` n'interroge que les fichiers `.gguf` sur disque et ignore totalement `daemon_endpoint`.
3. **INC-06 (Rupture RAG & Prompt Ad-Hoc dans `ask_vault`)** :
   - `ask_vault` effectue un appel FTS5 redondant (`search_question` suivi de `search_fts` identique en cas de résultat vide).
   - `storage.search_question` n'utilise pas l'extraction de mots-clés existante (`extract_search_keywords`).
   - Le prompt envoyé au LLM dans `ask_vault` est formaté via une chaîne ad-hoc au lieu d'utiliser la fonction certifiée `jeanne_core::llm::build_rag_prompt()` qui garantit la citation stricte des sources `[source: filename.md]`.

---

## 2. Spécification Détaillée

### 2.1 INC-04 : Découplage du Contexte KV et Assainissement des Options HTTP (`crates/core/src/local_llm.rs`)

1. **Débridage Contexte pour Serveur Distant** :
   - Dans `LocalLlmEngine::new()` et `update_config()` :
     Si `config.daemon_endpoint` est renseigné et non vide, la taille de contexte `context_size` n'est pas bridée par la RAM physique locale (`total_system_ram_mb`). L'utilisateur peut ainsi exploiter des contextes jusqu'à 32 768 tokens (ou la valeur configurée) sans limitation artificielle.
2. **Assainissement du Payload HTTP Distant** :
   - Dans `try_stream_from_local_daemon()` :
     Si `is_remote_endpoint(base_url)` est vrai :
     Ne pas injecter l'objet `"options": { "num_gpu": ..., "num_thread": ... }` dans le payload JSON, pour respecter strictement la spécification standard OpenAI `/v1/chat/completions`.
     Conserver `"options"` uniquement pour les daemons locaux (Ollama / llama-server sur `127.0.0.1`).

---

### 2.2 INC-05 : Unification de `LlmProvider::fetch_models` (`crates/core/src/local_llm.rs`)

1. **Prise en compte du Serveur Distant dans `fetch_models`** :
   - Dans `impl LlmProvider for LocalLlmEngine` :
     La méthode `fetch_models(&self)` vérifie si `daemon_endpoint` est configuré.
     Si oui : appelle `Self::fetch_remote_models(&endpoint, api_key)`. Si des modèles distants sont retournés, les renvoie en priorité.
     Si aucun serveur n'est configuré ou si l'appel échoue, repli sur la découverte des modèles GGUF locaux via `crate::model_discovery::discover_models()`.

---

### 2.3 INC-06 : Pipeline RAG Structuré & Citations de Sources dans `ask_vault` (`crates/core` & `src-tauri`)

1. **Recherche de Question Intelligente (`crates/core/src/storage.rs`)** :
   - `storage.search_question(question: &str, limit: usize)` :
     Utilise `extract_search_keywords(question)` pour extraire les termes significatifs (élimination de la ponctuation et des stop-words français et anglais).
     Construit une requête FTS5 ciblée sur les mots-clés. Si aucun mot-clé n'est extrait, repli gracieux sur la requête brute.
2. **Prompt RAG Structuré (`apps/desktop/src-tauri/src/lib.rs`)** :
   - Dans `ask_vault` :
     Convertit les extraits de notes trouvés en `HybridSearchResult` structurés.
     Appelle `jeanne_core::llm::build_rag_prompt(clean_q, &hybrid_results, None)` pour construire les messages avec injection stricte du contexte et obligation de citation `[source: filename.md]`.
     Formate le prompt pour le flux de génération de manière unifiée.

---

## 3. Matrice de Tests & Critères d'Acceptation

| ID Test | Composant | Description | Résultat Attendu |
| :--- | :--- | :--- | :--- |
| **TEST-04G-01** | `crates/core` | Contexte étendu débridé sur serveur distant | `context_size` de 8192 ou 16384 préservé même sur PC simulé avec $\le 16$ Go RAM si `daemon_endpoint` est actif |
| **TEST-04G-02** | `crates/core` | Absence d'`options` non standard vers serveur distant | La requête envoyée à un serveur distant ne contient pas la clé `"options"` |
| **TEST-04G-03** | `crates/core` | `fetch_models` interroge le serveur distant | `engine.fetch_models()` retourne les modèles du serveur distant si configuré |
| **TEST-04G-04** | `crates/core` | `search_question` filtre les mots vides | Requête "Où est le compte-rendu de réunion ?" transformée en recherche ciblée sur "compte rendu reunion" |
| **TEST-04G-05** | `desktop` | `ask_vault` génère un prompt RAG avec `[source: ...]` | Le prompt construit respecte le format strict et cite les noms de fichiers |
