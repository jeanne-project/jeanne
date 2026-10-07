# QA & Profiling Report — Jalon 07 : Système de Plugins (JSON-RPC) & Parser PDF

STATUS: APPROUVÉ

- **Date d'audit** : 2026-10-07
- **Branche auditée** : `feat/m07-plugins`
- **Spécification** : `docs/specs/07_SPEC_EXTENSIBILITY_PLUGINS.md`, `07b_SPEC_PLUGIN_ECOSYSTEM_AND_CORE_RUNNERS.md`
- **Note** : l'exécution des mesures a été réalisée par l'Architecte, le sous-agent `qa-profiler` n'ayant pas d'outil d'exécution dans cette session.

## Résultats des tests
| Test | Résultat |
| :--- | :--- |
| `just pre-review` (permissions, fmt, clippy `-D warnings`, build-plugins, `cargo test -p jeanne-core`, `npm run build`) | ✅ vert |
| TEST-07-01 `test_07_01_pdf_parser_roundtrip` (titre, `##`, tableau MD, métadonnées, codes -32001/-32002) | ✅ |
| TEST-07-02 `test_07_02_watchdog_leaves_no_zombie` (`/proc/<pid>` absent après timeout) | ✅ |
| TEST-07-03 `test_07_03_crash_immunity` (SIGSEGV → `ProcessFailed`, cœur opérationnel) | ✅ |
| `plugin_ecosystem_test` (5 tests : découverte, watchdog, embeddings, llm-runner) | ✅ |
| `go test ./...` dans `plugins/pdf-parser` (parse, codes d'erreur, protocole NDJSON) | ✅ |
| `ps` après tests : aucun zombie ni `pdf-parser` résiduel | ✅ |

## Empreinte Mémoire (RSS)
- Plugin `pdf-parser` (parse réel d'un PDF) : **8,6 Mo** pic RSS ; processus éphémère, 0 Mo au repos.
- Cœur Jeanne : aucun changement résident (plugin lancé à la demande) ; budget < 200 Mo respecté.
- Aucun contenu binaire sur stdio : seul `file_path` est transmis ; lecture disque via `ReaderAt`.

## Critères DoD
- [x] Manifestes `plugin.json` découverts (dont `document_parser`).
- [x] Watchdog sans zombie ; crash isolé.
- [x] Plugin PDF Go autonome, intégré à `just build-plugins`.

## Avertissements (non bloquants)
- Heuristique de titre : la taille « corps » est déterminée par volume de caractères ; un document très court dont le titre domine peut ne pas produire de `#`. À affiner au Jalon 8 (Golden Dataset).
- PDF chiffrés : renvoient -32002 (pas de saisie de mot de passe).

**STATUS: APPROUVÉ**
