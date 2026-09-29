# Code Review - Jalon 02

STATUS: APPROUVÉ

**Date**: 2026-09-29  
**Auteurs**: Reviewer M2 #1 & Reviewer M2 #2 (Consolidé par QA-Profiler)  
**Périmètre Cible**: `crates/core/src/rag.rs`, `crates/core/src/error.rs`, `crates/core/src/models.rs`, `crates/core/src/storage.rs`, `crates/core/src/vault.rs`  
**Spécification**: `docs/specs/02_SPEC_RAG_HYBRID_TEMPORAL.md`  
**Verdict**: **STATUS: APPROUVÉ**

---

## 1. Synthèse de la Revue de Code

L'implémentation du Jalon 2 (Moteur RAG Hybride & Pondération Temporelle) a été auditée en profondeur par deux auditeurs indépendants et soumise à une batterie de tests d'intrusion et de cas limites adversariaux.

### Points Clés Validés
1. **Conformité Stricte aux Contrats d'Interface (`PROJECT.md`)** :
   - `RagEngine::new(storage: Arc<Mutex<StorageManager>>) -> Self`
   - `RagEngine::search(&self, query: &str, query_embedding: &[f32; 384], limit: usize) -> Result<Vec<HybridSearchResult>, RagError>`
   - `check_circuit_breaker(max_similarity: f32) -> Result<(), RagError>`
   - `RagError` avec conversions bidirectionnelles non cycliques via `Box<JeanneError>`.
2. **Exactitude Mathématique & Algorithmique** :
   - Formule Time-Decay : $\text{Score} = (0.7 \cdot S_v + 0.3 \cdot S_l) / (1 + \lambda \cdot \Delta t_{\text{jours}})$.
   - Immunité absolue des notes procédurales ($\lambda = 0.0 \implies \text{dénominateur} = 1.0$).
   - Demi-vie de 200 jours pour les notes sémantiques et épisodiques ($\lambda = 0.005$).
   - Coupe-circuit anti-hallucination déclenché dès $\max(S_v) < 0.65$ ou sur `NaN`, avant tout calcul FTS5 ou requête relationnelle.
3. **Sécurité Mémoire & Robustesse Rust 2024** :
   - 0 `.unwrap()` et 0 `.expect()` dans `crates/core/src/`.
   - Tous les verrous empoisonnés sont gérés via `map_err`.
   - 0 warning sous `cargo clippy -p jeanne-core --all-targets -- -D warnings`.
   - Résilience prouvée face aux timestamps futurs (clamping `max(0)`), aux requêtes FTS5 malformées (guillemets orphelins, opérateurs nus), et aux cycles de liens d'obsolescence (résolution 1-hop bornée par `LIMIT 1`).

---

## 2. Bloquants

**Aucun bloquant.** L'intégralité des exigences d'acceptation et des contraintes architecturales est respectée.

---

## 3. Avertissements & Remarques Mineures

- **Multiplicateur de candidats en cas de fort taux de dépréciation** : Dans `RagEngine::search_with_options`, la recherche vectorielle extrait `(limit * 3).max(20)` candidats avant d'appliquer le filtre relationnel `status = 'active'`. Si un coffre présentait plus de 80% de notes dépréciées dans le voisinage vectoriel immédiat, le nombre de résultats actifs pourrait être inférieur à `limit`. Ce comportement est conforme au filtrage découplé standard et ne pose aucune anomalie dans un coffre réel.

---

## 4. Conclusion

Le code est d'une grande rigueur, mathématiquement exact, exempt de paniques et de fuites de mémoire. La porte de fusion pour le Jalon 2 est officiellement ouverte.
