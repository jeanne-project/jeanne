# Jeanne (Monorepo)

<p align="center">
  <img src="docs/assets/branding/jeanne_symbol.png" width="128" alt="Jeanne Logo" />
</p>

<!-- README-I18N:START -->

[Français](./README.md) | **English**

<!-- README-I18N:END -->

Application core of the Jeanne project:
* `crates/core`: Pure Rust library (Markdown persistence, SQLite, RAG, and orchestration).
* `apps/desktop`: Tauri v2 desktop application (Svelte UI, floating palette, audio capture).
* `tools/llm-benchmark`: [Deterministic LLM Evaluation Tool](./tools/llm-benchmark/README.md) (hardware benchmarks, in-process GGUF, inference profiles, token metrics).

## Verification
```bash
cargo check --workspace
```
