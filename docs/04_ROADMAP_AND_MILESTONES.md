# 04 - Roadmap & Jalons de Développement (v1)

Ce document consigne la séquence de développement obligatoire de **Jeanne**. Chaque jalon constitue une tranche verticale testable.

> **Règle d'avancement** : Le jalon $N+1$ ne peut débuter que si l'ensemble des critères d'acceptation du jalon $N$ sont vérifiés par des tests automatisés et conformes au budget mémoire (< 80 Mo au repos sur machine 16 Go).

---

## Vue d'Ensemble des Jalons

| Jalon | Intitulé | Cœur fonctionnel | Budget RAM max |
| :--- | :--- | :--- | :--- |
| **Jalon 1** | **MVP Socle & Palette** | Watcher Markdown local + FTS5 BM25 + Overlay `Alt + Espace` | < 80 Mo |
| **Jalon 2** | **RAG Hybride & Temps** | Vectorisation `sqlite-vec` + Formule Time-Decay + Filtre d'obsolescence | < 200 Mo |
| **Jalon 3** | **Inférence Distante** | Client compatible OpenAI (`/v1`) en streaming SSE + Masquage PII local | < 150 Mo |
| **Jalon 4** | **Inférence Locale** | Bindings `llama.cpp` Vulkan pour modèle GGUF 3B + Déchargement RAM | < 4,5 Go |
| **Jalon 5** | **Pipeline Vocal** | Interaction bidirectionnelle STT (Whisper) et TTS (Piper) débrayable | Modulaire |
| **Jalon 6** | **Capture de Réunion** | Diarisation matérielle stéréo (Micro/WASAPI) + OCR diapositives + Seeking | < 100 Mo (capture) |
| **Jalon 7** | **Système de Plugins** | Runner IPC JSON-RPC sur stdio + Plugin parser PDF autonome (Go) | Éphémère |
| **Jalon 8** | **Hardening & v1** | Évaluation sur Golden Dataset + Résolution de conflits + Packaging OS | < 100 Mo |

---

### Détail des Exigences par Jalon

#### Jalon 1 : MVP Socle "File-over-App" & Palette d'Accès Rapide
* Initialisation SQLite `database.db` avec tables `files`, `chunks` et table virtuelle `fts_notes USING fts5`.
* Watcher FS local (`notify-rs`) synchronisant le dossier Markdown en temps réel.
* Raccourci global `Alt + Espace` pilotant la fenêtre transparente `quick-access` (affichage en < 50 ms).
* Recherche lexicale BM25 instantanée (< 15 ms) et capture rapide `/note ` horodatée.

#### Jalon 2 : RAG Hybride & Arbitrage Temporel
* Intégration `sqlite-vec` (`vec_chunks USING vec0`) et modèle dense CPU léger (dim: 384).
* Formule : $\text{Score} = (0.7 \cdot S_{\text{vector}} + 0.3 \cdot S_{\text{BM25}}) \times \frac{1}{1 + \lambda \times \Delta t_{\text{jours}}}$.
* Filtrage strict `WHERE statut = 'actif'` et seuil de repli $\max(S_{\text{vector}}) \ge 0.65$.

#### Jalon 3 : Inférence Hybride Distante & Filtre PII
* Trait `LlmProvider` et implémentation client HTTP vers tout endpoint OpenAI (`/v1`).
* Masquage déterministe par Regex des données sensibles (e-mails, téléphones) avant émission réseau.
* Citations de sources systématiques `[source: nom_note.md]` et affichage streaming.

#### Jalon 4 : Inférence Locale Embarquée (Vulkan)
* Bindings `llama.cpp` Vulkan ciblant des modèles 3B quantifiés (Q4_K_M).
* Contexte KV bridé à 4 096 tokens et bouton explicite de déchargement de la mémoire vive.

#### Jalon 5 : Sous-système Audio & Interaction Vocale
* Capture `cpal` 16 kHz, transcription Whisper à la demande, synthèse vocale Piper TTS.
* Isolation stricte : désactivable pour conserver une empreinte mémoire nulle hors usage.

#### Jalon 6 : Assistant de Réunion (Diarisation Stéréo & Replay)
* Enregistrement stéréo direct sur disque : Canal Gauche = Micro, Canal Droit = WASAPI Loopback.
* Détection de diapositives par pHash (> 15 %) et OCR local.
* Worker asynchrone post-réunion générant le compte-rendu avec liens `seek` interactifs.

#### Jalon 7 : Extensibilité Polyglotte (Plugins JSON-RPC)
* Plugin Manager découvrant les manifestes `plugin.json` et pilotant des sous-processus éphémères sur `stdio`.
* Développement du dépôt officiel `plugin-pdf` en Go.

#### Jalon 8 : Durcissement, Golden Dataset & Publication
* Banc de test d'évaluation automatisé (Recall > 90 %, zéro hallucination sur questions hors-base).
* Packaging des binaires finaux pour Windows et Linux.
