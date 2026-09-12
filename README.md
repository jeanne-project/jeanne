# Jeanne (Monorepo)

<!-- README-I18N:START -->

**Français** | [English](./README.en.md)

<!-- README-I18N:END -->

Cœur applicatif du projet Jeanne :
* `crates/core` : Bibliothèque Rust pure (persistance Markdown, SQLite, RAG et orchestration).
* `apps/desktop` : Application de bureau Tauri v2 (UI Svelte, palette flottante, capture audio).

## Vérification
```bash
cargo check --workspace
```
