# AI Agent Development Guidelines (AGENTS.md)

This document establishes the non-negotiable architectural invariants, memory budgets, and engineering conventions for **Jeanne**. Every AI agent contributing to this repository must strictly adhere to these rules.

## 1. Architectural Invariants (Golden Rules)

1. **"File-over-App" Philosophy**:
   - The primary, immutable source of truth is **the local vault of plain Markdown (`.md`) files**.
   - The SQLite database (`sqlite-vec` + FTS5) is strictly a **disposable, derived cache index**. The system must remain capable of rebuilding the entire database from disk files at any time without data loss.
2. **Hardware Constraints (16 GB RAM / Shared iGPU Max 8 GB)**:
   - The application resident footprint (excluding LLM weights) must stay below **200 MB RAM**.
   - In standalone local inference mode (active 3B quantized model), total process footprint must not exceed **4.5 GB RAM**.
   - In remote OpenAI-compatible mode, client footprint must remain under **150 MB RAM**.
   - **Zero Buffer Bloat**: Loading entire multi-megabyte audio files, video containers, or large PDFs directly into memory buffers is prohibited. Streaming I/O and disk-backed chunking are mandatory.
3. **Strict 4-Memory Stratification (CoALA & IPCRA Framework)**:
   - *Procedural*: Prompts, templates, and rules (`Casquettes/`, protocols, injected directly into prompts).
   - *Semantic*: Current verified facts and specifications (`Ressources/`, `Zettelkasten/`, enforcing strict obsolescence validation).
   - *Episodic*: Chronological logs, meeting transcripts, and journal entries (`Projets/`, `Archives/`, ranked with Time-Decay).
   - *Working*: Active conversation window bounded strictly by a token budget.
4. **Zero-Core-Bloat & Zero-Python Runtime**:
   - `crates/core` must never depend on UI frameworks, FFmpeg, or heavy format decoders.
   - All document schema validation is executed natively in Rust (`serde`, `validator`). Zero Python/Pydantic runtime is permitted.
   - Any document format other than plain text and Markdown must be processed exclusively via external plugins through IPC JSON-RPC 2.0.
5. **Spec-Driven Development (SDD) Obligatoire** :
   - Il est formellement interdit d'implémenter du code applicatif sans une spécification technique validée dans `docs/specs/<id>_SPEC_<nom>.md`.
   - Tout développement suit obligatoirement la compétence `.agent/skills/sdd-workflow/SKILL.md` (Phase 1: Spec -> Phase 2: Rouge TDD -> Phase 3: Verte -> Phase 4: Audit de clôture).
   - Aucun commit de code de production n'est accepté sans son jeu de tests unitaires et d'intégration validé.

## 2. Toolchain & Coding Standards
* **Automation**: `Justfile` à la racine pour standardiser les flux de branches, les audits et les portes de fusion.
* **Backend**: Rust **Edition 2024** (`rust-version = "1.85.0"`).
  - Error Handling: `anyhow` for top-level application orchestration; `thiserror` for library domain crates.
  - Concurrency: `tokio 1.43+` (asynchronous, non-blocking runtime).
  - Telemetry: `tracing` crate (raw `println!` and `eprintln!` are forbidden in production).
* **Frontend**: Tauri v2, Svelte 5 (using `$state`, `$derived`, `$effect` runes), TypeScript strict.
  - Quick-access overlay (`Alt + Space`) must appear and be interactive in **less than 50 ms**.
* **Subprocess Plugins**: Polyglot (Go or Rust). Standard I/O communication via JSON-RPC 2.0.

## 3. Pre-Commit & Quality Verification Checklist
Before completing any assignment, an agent must execute and pass:
1. `just pre-review` (or `cargo clippy --workspace --all-targets -- -D warnings` && `cargo test --workspace` && `cd apps/desktop && npm run build`).
2. Verify zero warnings, zero errors, all tests green.
3. Ensure no zombie subprocesses or leaked file descriptors exist during plugin invocation.

## 4. Cadre Opérationnel d'Avancement & Portes de Fusion

1. **Ordre Séquentiel Strict** : Se référer impérativement à `docs/04_ROADMAP_AND_MILESTONES.md`. Il est formellement interdit de développer des briques d'un jalon ultérieur tant que le jalon courant n'a pas validé tous ses critères d'acceptation.
2. **Cycle de Branches Éphémères & Worktrees (`Justfile`)** :
   - Chaque jalon est développé sur une branche isolée créée via `just start-milestone <id> <name>` (branche `feat/m{{id}}-{{name}}`).
   - Pour les tâches d'audit ou de test parallèles sans perturber l'espace de travail principal, utiliser les worktrees isolés dans `.worktrees/` via `just worktree-add <name> <branch>` et `just worktree-clean <name>`.
3. **Traçabilité des Audits sous `docs/reviews/`** :
   - Tout jalon requiert obligatoirement deux artefacts d'audit écrits avant d'envisager la fusion :
     - `docs/reviews/M{ID}_CODE_REVIEW.md` : Rédigé et maintenu par le `Reviewer` (initialisé avec `just init-review {ID}`). Doit afficher impérativement `STATUS: APPROUVÉ`.
     - `docs/reviews/M{ID}_QA_REPORT.md` : Rédigé et maintenu par le `QA-Profiler` (initialisé avec `just init-qa {ID}`). Doit valider la DoD et les budgets RSS.
   - La fusion sur `main` est orchestrée exclusivement par l'Architecte via `just merge-milestone <id> <name>`, qui bloque si le statut n'est pas approuvé.
4. **Consultation de la Documentation** : Utiliser la commande CLI `ctx7` ou le skill global `find-docs` pour vérifier les APIs officielles (Tauri v2, Svelte 5, Rust 2024) en cas de doute.
5. **Absence de Daemons MCP** : Ne créer aucun fichier de configuration MCP résident.

## 5. Agents et Sous-Agents d'Exécution (.agent/agents/)

Les agents spécialisés sont définis dans `.agent/agents/` sous forme de fichiers Markdown dotés d'un frontmatter YAML conforme au standard Google Antigravity (`subagent: true`, `mainAgent: true`). Ils peuvent être adoptés comme **rôle contextuel direct** par l'agent principal ou invoqués de manière autonome en arrière-plan via `invoke_subagent`.

| Agent | Identifiant Antigravity | Rôle & Responsabilité principale | Périmètre cible |
| :--- | :--- | :--- | :--- |
| **`Architect`** | `architect` | **Ordonnanceur Unique & Lead Architect**. Point de contact central, qualification des demandes (spec vs implémentation), pilotage des branches et validation des portes de fusion. | `docs/specs/`, `docs/`, `AGENTS.md`, `Justfile`, gestion Git |
| **`Rust-Core`** | `rust-core` | Implémentation du moteur Rust 2024, persistance SQLite, audio et IPC. | `crates/core/**`, `src-tauri/**` |
| **`Frontend`** | `frontend` | Interface Svelte 5 (Runes), palette flottante et événements Tauri v2. | `apps/desktop/src/**` |
| **`Reviewer`** | `reviewer` | Audit de code statique, sécurité mémoire, absence de `unwrap`, rédaction obligatoire de `docs/reviews/M{ID}_CODE_REVIEW.md`. | Lecture globale, diffs Git, `docs/reviews/` |
| **`QA-Profiler`** | `qa-profiler` | Contrôle des budgets RAM (RSS), tests aux limites, validation DoD, rédaction obligatoire de `docs/reviews/M{ID}_QA_REPORT.md`. | `tests/**`, benchmarks, `docs/reviews/` |
| **`Plugin-Dev`** | `plugin-dev` | Sous-processus isolés en JSON-RPC 2.0 (Go/Rust). | `plugins/**` |

### Règles d'Interaction
1. **Interlocuteur Unique** : L'`architect` est le point d'entrée unique de toute commande, signalement de bogue ou évolution. Il qualifie le besoin et ordonnance le travail des agents spécialisés.
2. **Déclaration de Rôle** : En début de session ou de tâche interactive, déclare explicitement ton rôle ou agent actif (ex. : « *J'agis en tant que Lead Architect et Ordonnanceur Système pour...* »).
3. **Délégation et Parallélisme** : L'ordonnanceur peut instancier directement les sous-agents en tâche de fond via `invoke_subagent` (ex. `reviewer` ou `qa-profiler` dans un worktree dédié) pour paralléliser les validations sans saturer la fenêtre de contexte.
4. **Respect des Cloisonnements** : Un agent de développement (`rust-core`, `frontend`) ne modifie jamais une spécification technique ; seul l'`architect` en a la prérogative.
5. **Revue Obligatoire & Porte de Fusion** : Aucun commit n'est poussé sur `main` sans validation préalable par le `reviewer` (`STATUS: APPROUVÉ`) et le `qa-profiler` dans leurs rapports respectifs sous `docs/reviews/`.
