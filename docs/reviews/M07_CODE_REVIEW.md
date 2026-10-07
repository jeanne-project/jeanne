# Code Review - Jalon 07 : Système de Plugins JSON-RPC & Parser PDF

STATUS: APPROUVÉ

- **Branche** : `feat/m07-plugins`
- **Rédaction** : audit du sous-agent `reviewer` (sans outil d'exécution), bloquants corrigés et contrôles exécutés par l'Architecte.

## Contrôles
- `just pre-review` (permissions, fmt, clippy `-D warnings`, build-plugins, `cargo test -p jeanne-core`, `npm run build`) : ✅ vert après correctifs.
- `go test ./...` (plugins/pdf-parser) : ✅

## Bloquants (tous résolus)
- [x] `runner.rs` : `kill_on_drop(true)` ajouté — plus d'orphelin sur erreur d'écriture stdin.
- [x] `runner.rs` : lecture stdout bornée (8 Mo/ligne) via `take` + `read_until`.
- [x] `plugin_pdf_test.rs` : plus de SKIP silencieux, le test échoue si le binaire est indisponible.
- [x] Écart spec/test (`SubprocessCrashed` vs `ProcessFailed`, -32002) : spec 07 amendée (§3.3).

## Avertissements & Dette
- `manager.rs` : découverte remontant jusqu'à 2 parents du cwd et `CARGO_MANIFEST_DIR` ; à restreindre en production (Jalon 8).
- `manager.rs` : `find_by_capability` non déterministe si doublons ; erreurs de manifeste non journalisées.
- `main.go` : `trimSpace` redondant avec `bytes.TrimSpace`.
- Heuristique de titre PDF à affiner (Golden Dataset).

**STATUS: APPROUVÉ**
