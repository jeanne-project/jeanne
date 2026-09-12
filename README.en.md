# Jeanne (Monorepo)

<!-- README-I18N:START -->

[Français](./README.md) | **English**

<!-- README-I18N:END -->

Application core of the Jeanne project:
* `crates/core`: Pure Rust library (Markdown persistence, SQLite, RAG, and orchestration).
* `apps/desktop`: Tauri v2 desktop application (Svelte UI, floating palette, audio capture).

## Verification
```bash
cargo check --workspace
```
