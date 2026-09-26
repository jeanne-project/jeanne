# 01 - Vision & System Architecture

## 1. Context & Problem Statement
Personal AI Knowledge Assistants ("Second Brains") consistently fail in production due to three architectural flaws:
1. **Context Rot**: As vaults expand, naive vector search injects noisy chunks into the inference window, exhausting the LLM attention budget and triggering confident hallucinations.
2. **Lack of Temporal Arbitration & Obsolescence**: In flat vector indexes, an outdated decision from six months ago carries identical semantic weight to a corrective decision made yesterday. Outdated resources contaminate responses unless explicitly invalidated.
3. **Infrastructure Bloat & Runtime Fragility**: Running multiple Docker containers (vector databases, message queues) or scripting runtime dependencies (Python/Pydantic daemons) overwhelms standard desktop machines (16 GB RAM, shared iGPU) and destroys cold-start performance.

**Jeanne (J.E.A.N.N.E.)** solves this through memory stratification (CoALA / IPCRA), native temporal decay ranking, deterministic hardware diarization, native Rust validation, and a zero-daemon native architecture.

---

## 2. Unified Knowledge Architecture: CoALA & IPCRA Framework

Jeanne organizes knowledge according to the **CoALA (Cognitive Architectures for Language Agents)** framework, mapped onto an **IPCRA** vault folder topology:

```
Vault (`~/SecondBrain/`)
├── Casquettes/     ──> [Mémoire Procédurale]      (Rôles, protocoles, guides, checklists)
├── Projets/        ──> [Mémoire Épisodique]       (Journaux de bord actifs, comptes-rendus datés)
├── Archives/       ──> [Mémoire Épisodique]       (Projets clos, logs append-only historiques)
├── Ressources/     ──> [Mémoire Sémantique Fact.] (Fiches techniques, tarifs, specs, hardware)
├── Zettelkasten/   ──> [Mémoire Sémantique Conc.] (Notes atomiques 1 note = 1 concept, evergreen)
└── Inbox/          ──> [Sas d'Ingestion]          (Captures brutes non classées, idées volantes)
```

### 2.1 The 4 Memory Strata
1. **Mémoire de travail (*Working Memory*)** :
   - Contexte immédiat de la tâche et historique court de la conversation active.
   - Éphémère, volatile, strictement bornée par le budget de tokens alloué au modèle.
2. **Mémoire procédurale (*Procedural Memory*)** :
   - Hébergée dans `Casquettes/` et dans les règles opérationnelles.
   - Contient les gabarits, guides opératoires, protocoles de décision et checklists de contrôle.
   - Invariant : Injectée directement dans les prompts systèmes selon le rôle actif, sans atténuation temporelle ($\lambda = 0.0$).
3. **Mémoire épisodique (*Episodic Memory*)** :
   - Hébergée dans `Projets/` et `Archives/`.
   - Journaux de bord datés, transcripts de réunions, décisions ponctuelles et historiques d'incidents.
   - Modèle immuable (*append-only* temporel) soumis à la formule d'atténuation temporelle Time-Decay ($\lambda = 0.005$).
4. **Mémoire sémantique factuelle (*Semantic Factual Memory*)** :
   - Hébergée dans `Ressources/`.
   - Fiches techniques, tarifs, spécifications d'infrastructures, manuels de référence.
   - Soumise à une gestion stricte de l'obsolescence et à une invalidation locale à un degré ($1\text{-hop}$).
5. **Mémoire sémantique conceptuelle (*Semantic Conceptual Memory*)** :
   - Hébergée dans `Zettelkasten/`.
   - Notes atomiques interconnectées par wikilinks `[[lien]]` respectant le principe fondamental : **$1\text{ note} = 1\text{ concept}$**.

### 2.2 Cycle de Vie Zettelkasten & Maturation des Connaissances
Les notes conceptuelles traversent trois stades de maturation :
* **`Inbox/`** : Sas de capture brute (notes vocales transcrites, extraits de lecture, idées non formalisées).
* **`seedlings/` (pousses)** : Notes atomiques embryonnaires en cours de clarification, annotées avec des wikilinks préliminaires.
* **`evergreen/` (permanentes)** : Notes conceptuelles denses, stables, interconnectées dans le graphe de connaissances, révisées et validées.

---

## 3. System Topology

```
┌────────────────────────────────────────────────────────────────────────┐
│               Local Source of Truth (Markdown Vault)                   │
│                     `~/SecondBrain/**/*.md`                            │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │ File System Events (`notify-rs`)
                                    ▼
┌────────────────────────────────────────────────────────────────────────┐
│                        Rust Core (`jeanne-core`)                       │
│  - Semantic Chunker                  - Hybrid RAG Engine (BM25 + Vec)  │
│  - Time-Decay Relevance Ranker       - 1-Hop Graph Invalidation        │
│  - Native Rust Frontmatter Validator - Local Regex PII Masker          │
└──────────────┬──────────────────────────────────────────┬──────────────┘
               │ In-process binding                       │ IPC stdio (JSON-RPC)
               ▼                                          ▼
┌──────────────────────────────┐          ┌──────────────────────────────┐
│   Derived SQLite Index       │          │     Polyglot Plugins         │
│   - `vec_chunks` (sqlite-vec)│          │     - Parsers (PDF in Go...) │
│   - `fts_notes` (BM25 FTS5)  │          │     - Meeting Replay (FFmpeg)│
│   - `file_links` (1-Hop Graph│          │     - Voice (Piper / Whisper)│
│   - Validity & CoALA metadata│          └──────────────────────────────┘
└──────────────┴───────────────┘
               ▲
               │ Direct In-Process / Crate Dependency
      ┌────────┴────────────────────────────────────────┐
      │                                                 │
┌─────┴────────────────────────────┐      ┌─────────────┴────────────────────────────┐
│ Tauri v2 Desktop Client          │      │ Headless CLI (`crates/cli`)              │
│ (`jeanne-desktop`)               │      │ `jeanne vault <cmd>`                     │
│ - Floating Overlay (`Alt+Space`) │      │ - Indexation & Sync en tâche de fond     │
│ - Instant FTS5 Search (<50ms)    │      │ - Requêtes RAG & Diagnostics sans IHM    │
│ - Deterministic Stereo Capture   │      │ - Pipeline CI/CD et scripts d'audit      │
└──────────────────────────────────┘      └──────────────────────────────────────────┘
```

---

## 4. Native Rust Validation (Zero-Python Invariant)

Contrairement aux approches conventionnelles reposant sur des scripts Python et des validateurs Pydantic :
* **Zéro Dépendance Runtime Python** : L'environnement d'exécution n'impose ni Python, ni uv/poetry, ni daemons externes.
* **Validation Native en Rust** :
  - Extraction du frontmatter YAML via `gray_matter`.
  - Désérialisation et validation structurelle et logique assurées directement par `serde` et la crate Rust `validator`.
  - Contrats stricts validés dès l'ingestion : statut (`active`, `deprecated`), horodatages RFC 3339 / epoch, métadonnées CoALA (`coala_type`), et validité des cibles de wikilinks.
  - Performance déterministe et empreinte mémoire résiduelle nulle.

---

## 5. Headless & CLI Architecture (`crates/cli`)

Pour garantir l'interopérabilité et la testabilité sans IHM :
* Le crate `crates/cli` expose l'exécutable `jeanne` (`jeanne vault index`, `jeanne vault search`, `jeanne vault audit`).
* Il consomme directement `jeanne-core` sans lier Tauri, WebKit ou des bibliothèques d'affichage.
* Permet l'automatisation en ligne de commande, les scripts d'administration et la validation de performance en environnement serveur ou conteneurisé.

---

## 6. Engineering Trade-Offs & Rationales

* **Rust Edition 2024 for Core & CLI**: Élimine les pauses Garbage Collector, garantit un nettoyage mémoire déterministe (RAII) sous la contrainte stricte des 16 Go de RAM, et offre un FFI C/C++ sans surcoût pour `sqlite-vec` et `llama.cpp`.
* **Embedded SQLite + `sqlite-vec`**: Élimine les daemons serveurs (Qdrant, Milvus, PostgreSQL). Indexation locale dans un unique fichier `.db` avec mode WAL et RAM résidente au repos **< 80 Mo**.
* **Graphe à 1 degré (`1-hop`) pour l'Obsolescence** : Évite la complexité et le surcoût mémoire d'une base de graphes lourde (Neo4j). Une simple table relationnelle indexée `file_links` suffit pour invalider les contradictions directes et propager les avertissements d'obsolescence.
* **Go pour les Plugins Documentaires Lourdes** : Standalone binaries compilés, appelés à la demande via JSON-RPC 2.0 sur stdio, isolés du processus principal.
