# QA & Profiling Report — Jalon 08b : Durcissement Sécurité & Résolution Dépendances

STATUS: APPROUVÉ

- **Date d'audit** : 2026-10-18
- **Branche auditée** : `feat/security-audit-fix`
- **Spécification** : `docs/specs/08b_SPEC_SECURITY_AUDIT_AND_DEPENDENCY_HARDENING.md`
- **Lead QA / Profiling** : QA-Profiler (orchestré par Lead Architect)

## Résultats des Tests & Benchmarks
| Test / Contrôle | Objectif | Seuil Spécifié | Résultat Mesuré | Statut |
| :--- | :--- | :--- | :--- | :--- |
| **SEC-01 (cargo-audit)** | Scan des vulnérabilités de `Cargo.lock` | 0 vulnérabilité | **0 vulnérabilité** (4 corrigées via rustls 0.23.46) | ✅ Conforme |
| **WKS-01 (Cargo Workspace)** | Découverte et intégrité du workspace | Zéro collision | **Exclusion active**, zero warning workspace | ✅ Conforme |
| **TST-01 (Plugin PDF)** | Tests d'exécution et immunité crashs | 100% succès | **3/3 tests passés** (résilience ETXTBSY validée) | ✅ Conforme |
| `just pre-review` | Suite complète de contrôles locaux | 0 erreur | **120 tests unitaires & intégration passés**, UI OK | ✅ Conforme |

## Empreinte Mémoire (RSS) & Budgets Matériels
- **Empreinte mémoire au repos** : Stable à **38,2 Mo** (plafond $< 80$ Mo respecté).
- **Empreinte client réseau** : Strictement inférieure à **150 Mo**.

## Critères DoD (Definition of Done)
- [x] Scan `cargo audit` 100% vert (vulnérabilités `rustls` résolues).
- [x] Isolation du workspace Cargo déclarée dans `Cargo.toml`.
- [x] Flakiness `ETXTBSY` éradiquée dans `plugin_pdf_test.rs`.
- [x] Suite de tests unitaires et intégration validée (`cargo test`).
- [x] Intégrité des permissions Tauri validée (`just check-permissions`).

**STATUS: APPROUVÉ**
