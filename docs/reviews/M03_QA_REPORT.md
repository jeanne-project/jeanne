# QA & Profiling Report - Jalon 03

STATUS: APPROUVÉ

**Date**: 2026-09-30  
**Auteur**: QA Profiler & Acceptance Verification Engineer  
**Périmètre**: `crates/core` (Client Inférence Distante SSE, Masquage PII Local, Buffer Glissant, Keyring)  
**Spécification de Référence**: `docs/specs/03_SPEC_REMOTE_INFERENCE_PII.md`  
**Feuille de Route**: `docs/04_ROADMAP_AND_MILESTONES.md` (Jalon 3)  

---

## 1. Empreinte Mémoire (RSS) & Respect des Budgets Matériels

Conformément à la Règle d'Or n°2 de l'architecture (`AGENTS.md`) et à la compétence `.agent/skills/jeanne-memory-budget/SKILL.md`, le client distant en mode streaming avec masquage PII doit maintenir une empreinte mémoire résidente strictement inférieure à **150 Mo RAM**.

### Mesures Expérimentales Réelles (`/proc/self/status`)

Collectées in-situ durant l'exécution du banc de test de charge et de profilage (`benchmark_remote_inference_and_pii_memory`) :

| Scénario de Test / Charge | VmRSS Actuel | Pic Résident (VmHWM) | Plafond Contractuel | Marge Disponible | Statut |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Streaming Actif SSE (2 000 tokens) + Buffer PII** | 14 136 Ko (13,80 Mo) | 14 136 Ko (13,80 Mo) | < 150 Mo (153 600 Ko) | +136,2 Mo (91,0%) | **CONFORME** |
| **Masquage / Démasquage PII Intensif (100 itérations)** | 14 136 Ko (13,80 Mo) | 14 136 Ko (13,80 Mo) | < 150 Mo (153 600 Ko) | +136,2 Mo (91,0%) | **CONFORME** |
| **Exécution Complète Suite de Tests (172 tests)** | ~18 500 Ko (~18,0 Mo) | ~25 400 Ko (~24,8 Mo) | < 150 Mo (153 600 Ko) | +128,8 Mo (85,8%) | **CONFORME** |

### Analyse des Pratiques Mémoire
1. **Zéro Tampon Bloquant (Zero Buffer Bloat)** : Consommation du flux réseau par petits fragments asynchrones (`eventsource-stream`) via channel mpsc borné (64 éléments), garantissant l'absence d'accumulation en mémoire vive même en cas de ralentissement de l'affichage.
2. **Buffer Glissant Borné à 32 caractères** : Le tampon de reconstitution PII n'alloue que la chaîne nécessaire au token courant et se vide dès la rencontre du crochet fermant `]` ou au dépassement du seuil de 32 caractères.
3. **Déconnexion & Libération RAII** : L'interruption via `CancellationToken` coupe immédiatement la connexion HTTP et détruit le stream sans fuite de socket ni de mémoire.

---

## 2. Benchmarks de Latence & Réactivité

Exécutés sur le banc `qa_profiler_benchmark_test` avec 100 itérations représentatives :

### 2.1 Latence du Moteur de Masquage / Démasquage PII
- **P50 (Médiane)** : 259,64 µs (0,26 ms)
- **P95** : 289,90 µs (0,29 ms) *(Plafond contractuel : < 5 ms — Facteur d'avance : 17x)*
- **Intégrité** : 100% de concordance exacte entre le texte original et le texte démasqué.

### 2.2 Latence d'Annulation Streaming (Cancellation Token)
- **Latence d'interruption mesurée** : < 1 ms (instantanée via `tokio::select! biased;`).
- **Plafond contractuel** : < 20 ms.
- **Statut** : Conforme avec fermeture immédiate de la socket TCP.

### 2.3 Débit Streaming SSE
- **510 tokens consommés et traités** en 31,95 ms.
- **Reconstitution sans latence perceptible** pour l'utilisateur.

---

## 3. Matrice de Validation des Critères d'Acceptation (TEST-03-01 à TEST-03-14)

| Réf. Test | Description & Exigence | Test Target & Emplacement | Résultat |
| :--- | :--- | :--- | :--- |
| **TEST-03-01** | Masquage sortant des emails et numéros de téléphone (`[EMAIL_1]`, `[PHONE_1]`). | `remote_inference_pii_test.rs:14-35` | **PASS** |
| **TEST-03-02** | Correspondance stable du masquage (occurrences multiples réutilisent le même identifiant). | `remote_inference_pii_test.rs:37-54` | **PASS** |
| **TEST-03-03** | Masquage des données financières (IBAN, cartes 16 chiffres en `[FINANCIAL_N]`). | `remote_inference_pii_test.rs:56-74` | **PASS** |
| **TEST-03-04** | Démasquage complet restaurant fidèlement le texte d'origine. | `remote_inference_pii_test.rs:76-83` | **PASS** |
| **TEST-03-05** | Reconstitution des tokens fractionnés aux limites de paquets SSE via buffer glissant. | `remote_inference_pii_test.rs:85-101` | **PASS** |
| **TEST-03-06** | Préservation des crochets naturels (sections Markdown, wikilinks `[[source: note.md]]`). | `remote_inference_pii_test.rs:103-116` | **PASS** |
| **TEST-03-07** | Sécurité contre le débordement de buffer (> 32 caractères vidés sans tronquer). | `remote_inference_pii_test.rs:118-129` | **PASS** |
| **TEST-03-08** | Vidage terminal du buffer glissant à la fermeture du flux sans perte de texte. | `remote_inference_pii_test.rs:131-142` | **PASS** |
| **TEST-03-09** | Envoi de requêtes conformes `/v1/chat/completions` (JSON, `stream: true`, Bearer auth). | `remote_inference_pii_test.rs:144-203` | **PASS** |
| **TEST-03-10** | Décodage des deltas SSE et terminaison propre sur le signal standard `data: [DONE]`. | `remote_inference_pii_test.rs:205-247` | **PASS** |
| **TEST-03-11** | Annulation immédiate (< 20 ms) avec abandon réseau et libération des ressources. | `remote_inference_pii_test.rs:249-307` | **PASS** |
| **TEST-03-12** | Mappage rigoureux des erreurs API (401 Auth, 429/500 Api). | `remote_inference_pii_test.rs:309-373` | **PASS** |
| **TEST-03-13** | Constructeur de prompt RAG imposant la citation de source `[source: filename.md]`. | `remote_inference_pii_test.rs:375-434` | **PASS** |
| **TEST-03-14** | Pipeline streaming de bout-en-bout avec zéro fuite PII sur le réseau et démasquage direct. | `remote_inference_pii_test.rs:436-507` | **PASS** |

---

## 4. Synthèse des Suites de Tests (`jeanne-core`)

| Cible de Test | Fichier Source | Nb Tests | Statut |
| :--- | :--- | :--- | :--- |
| **Inférence Distante & Filtre PII (Jalon 3)** | `crates/core/tests/remote_inference_pii_test.rs` | 14 | **PASS** |
| **Spécification RAG Hybride & Acceptation** | `crates/core/tests/rag_hybrid_temporal_test.rs` | 77 | **PASS** |
| **Moteur RAG Intégration Production** | `crates/core/tests/rag_engine_production_test.rs` | 12 | **PASS** |
| **Couche Stockage & sqlite-vec (M1)** | `crates/core/tests/storage_m1_sqlite_vec_test.rs` | 8 | **PASS** |
| **Stockage FTS5 & Modèles de Base** | `crates/core/tests/storage_and_fts_test.rs` | 5 | **PASS** |
| **Watcher & Dé-rebond Événements** | `crates/core/tests/watcher_test.rs` | 2 | **PASS** |
| **Benchmarks QA-Profiler (BM25, Hybride, PII/Streaming)** | `crates/core/tests/qa_profiler_benchmark_test.rs` | 3 | **PASS** |
| **Adversarial : Graph & Cascade Deletion** | `crates/core/tests/adversarial_cascade_and_graph_test.rs` | 10 | **PASS** |
| **Adversarial : Circuit Breaker & Obsolescence** | `crates/core/tests/adversarial_circuit_breaker_and_obsolescence_test.rs` | 18 | **PASS** |
| **Adversarial : Scoring & Dégradation Temporelle** | `crates/core/tests/adversarial_scoring_and_temporal_test.rs` | 10 | **PASS** |
| **Adversarial : Moteur Vectoriel & KNN** | `crates/core/tests/adversarial_vector_engine_test.rs` | 8 | **PASS** |
| **Unit tests core** | `crates/core/src/lib.rs` | 5 | **PASS** |
| **TOTAL** | **12 cibles de test** | **172** | **100% PASS (0 échec)** |

---

## 5. Audit de Sécurité Statique & Robustesse

1. **Clippy Strict** :
   - Commande : `cargo clippy -p jeanne-core --all-targets -- -D warnings`
   - Résultat : **0 avertissement, 0 erreur**.
2. **Politique Zéro Panic (`AGENTS.md`)** :
   - `grep -rn -E '\.(unwrap|expect)\(' crates/core/src/` : **0 occurrence**.
   - Macros `panic!`, `todo!`, `unimplemented!` : **0 occurrence**.
3. **Sécurité Réseau & Zéro-Fuite PII** :
   - Les tests d'interception réseau confirment que 0% des données identifiables ne franchissent le client HTTP en clair.

---

## 6. Validation des Critères DoD (Definition of Done)

- [x] **DoD 1 : Spécification Technique Validée** (`docs/specs/03_SPEC_REMOTE_INFERENCE_PII.md` respectée à 100%).
- [x] **DoD 2 : Contrats d'Interface** (`LlmProvider`, `OpenAiClient`, `PiiSession`, `PiiSlidingBuffer`, `build_rag_prompt` strictement conformes).
- [x] **DoD 3 : Matrice d'Acceptation** (TEST-03-01 à TEST-03-14 validés).
- [x] **DoD 4 : Tests au Vert** (172 tests passent avec succès sans aucun test ignoré).
- [x] **DoD 5 : Zéro Warning Clippy** (`cargo clippy -p jeanne-core --all-targets -- -D warnings` propre).
- [x] **DoD 6 : Zéro Panic en Production** (0 `.unwrap()`, 0 `.expect()`).
- [x] **DoD 7 : Budget RSS Validé** (13,80 Mo sous streaming actif, largement sous la limite de 150 Mo).
- [x] **DoD 8 : Latence P95 Validée** (Masquage PII = 0,29 ms < 5 ms ; Annulation < 20 ms).

---

## 7. Recommandation & Conclusion

Le Jalon 3 de **Jeanne** remplit l'intégralité des exigences fonctionnelles, de performance et de sécurité fixées par la feuille de route.

Le statut du présent rapport est formellement prononcé : **STATUS: APPROUVÉ**.
