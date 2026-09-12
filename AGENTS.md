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
3. **Strict 4-Memory Stratification (KOALA Framework)**:
   - *Procedural*: Prompts, templates, and rules (injected directly into prompts).
   - *Semantic*: Current verified facts and specifications (enforcing strict obsolescence validation).
   - *Episodic*: Chronological logs, meeting transcripts, and journal entries (ranked with Time-Decay).
   - *Working*: Active conversation window bounded strictly by a token budget.
4. **Zero-Core-Bloat**:
   - `crates/core` must never depend on UI frameworks, FFmpeg, or heavy format decoders.
   - Any document format other than plain text and Markdown must be processed exclusively via external plugins through IPC JSON-RPC 2.0.

## 2. Toolchain & Coding Standards
* **Backend**: Rust **Edition 2024** (`rust-version = "1.85.0"`).
  - Error Handling: `anyhow` for top-level application orchestration; `thiserror` for library domain crates.
  - Concurrency: `tokio 1.43+` (asynchronous, non-blocking runtime).
  - Telemetry: `tracing` crate (raw `println!` and `eprintln!` are forbidden in production).
* **Frontend**: Tauri v2, Svelte 5 (using `$state`, `$derived`, `$effect` runes), TypeScript strict.
  - Quick-access overlay (`Alt + Space`) must appear and be interactive in **less than 50 ms**.
* **Subprocess Plugins**: Polyglot (Go or Rust). Standard I/O communication via JSON-RPC 2.0.

## 3. Pre-Commit Verification Checklist
Before completing any assignment, an agent must execute and pass:
1. `cargo check --workspace` -> 0 warnings, 0 errors.
2. `cargo test --workspace` -> All tests pass.
3. Ensure no zombie subprocesses or leaked file descriptors exist during plugin invocation.

## 4. Cadre Opérationnel d'Avancement

1. **Ordre Séquentiel Strict** : Se référer impérativement à `docs/04_ROADMAP_AND_MILESTONES.md`. Il est formellement interdit de développer des briques d'un jalon ultérieur tant que le jalon courant n'a pas validé tous ses critères d'acceptation.
2. **Consultation de la Documentation** : Utiliser la commande CLI `ctx7` ou le skill global `find-docs` pour vérifier les APIs officielles (Tauri v2, Svelte 5, Rust 2024) en cas de doute.
3. **Absence de Daemons MCP** : Ne créer aucun fichier de configuration MCP résident.

## 5. Rôles et Personas d'Exécution (.agents/personas/)

Pour toute intervention, adopte explicitement le rôle correspondant à la tâche en cours :

| Persona | Rôle & Responsabilité principale | Périmètre cible |
| :--- | :--- | :--- |
| **`Architect`** | Rédaction et audit de complétude des specs (SDD), budgets matériels. | `docs/specs/`, `docs/`, `AGENTS.md` |
| **`Rust-Core`** | Implémentation du moteur Rust 2024, persistance SQLite, audio et IPC. | `crates/core/**`, `src-tauri/**` |
| **`Frontend`** | Interface Svelte 5 (Runes), palette flottante et événements Tauri v2. | `apps/desktop/src/**` |
| **`Reviewer`** | Audit de code statique, sécurité mémoire, absence de `unwrap`, propreté. | Lecture globale, diffs Git |
| **`QA-Profiler`** | Contrôle des budgets RAM, tests aux limites, validation Definition of Done. | `tests/**`, benchmarks |
| **`Plugin-Dev`** | Sous-processus isolés en JSON-RPC 2.0 (Go/Rust). | `plugins/**` |

### Règles d'Interaction
1. **Déclaration de Rôle** : En début de session ou de tâche, déclare explicitement ton persona actif (ex. : « *J'agis en tant que Rust-Core pour implémenter le schéma FTS5 du Jalon 1* »).
2. **Respect des Cloisonnements** : Un persona de développement (`Rust-Core`, `Frontend`) ne modifie jamais une spécification technique ; seul l'`Architect` en a la prérogative.
3. **Revue Obligatoire** : Aucun commit n'est poussé sur `main` sans validation préalable par le `Reviewer`.
