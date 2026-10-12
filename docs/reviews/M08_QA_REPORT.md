# QA & Profiling Report — Jalon 08 : Durcissement, Golden Dataset QA & Production Packaging

STATUS: APPROUVÉ

- **Date d'audit** : 2026-10-12
- **Branche auditée** : `feat/m08-hardening`
- **Spécification** : `docs/specs/08_SPEC_HARDENING_EVAL_PACKAGING.md`
- **Lead QA / Profiling** : QA-Profiler (orchestré par Lead Architect)

## Résultats des Tests & Benchmarks
| Test | Objectif | Seuil Spécifié | Résultat Mesuré | Statut |
| :--- | :--- | :--- | :--- | :--- |
| **TEST-08-01 (Recall)** | Rappel documentaire Golden Dataset | $\ge 90\%$ | **100,00%** (38/38) | ✅ Conforme |
| **TEST-08-01 (Faithfulness)** | Fidélité des citations aux sources | $\ge 95\%$ | **97,37%** | ✅ Conforme |
| **TEST-08-01 (Hallucination)** | Rejet des requêtes hors-domaine (OOD) | **0,00%** | **0,00%** (0 hallucination) | ✅ Conforme |
| **TEST-08-01 (Exact Fallback)** | Émission de la chaîne exacte de repli | **100%** | **100,00%** (12/12) | ✅ Conforme |
| **TEST-08-02 (Hot-Unplug)** | Débranchement micro brutal en cours de réunion | Zéro panique, WAV valide | **WAV 16 kHz mono 16-bit valide**, Toast émis, 0 Mo résiduel | ✅ Conforme |
| **TEST-08-03 (Concurrency)** | Écriture externe concurrente pendant indexation | Rechargement auto V2 | **Détection SHA-256, transaction obsolète annulée, V2 indexée** | ✅ Conforme |
| **TEST-08-04 (Soak Test)** | Variance mémoire après simulation d'endurance | $< 5,0$ Mo | **0,00 Mo** mesuré | ✅ Conforme |
| `just pre-review` | Permissions, format, clippy, Go plugins, tests, build Svelte | 0 erreur, 0 warning | **121 tests passés**, build UI 145 Ko dist | ✅ Conforme |

## Empreinte Mémoire (RSS) & Budgets Matériels
- **Empreinte au repos (Release Build Idle RAM)** : **38,2 Mo** mesuré (Plafond strict $< 80$ Mo respecté).
- **Empreinte en pic durant l'évaluation (50 requêtes)** : **44,1 Mo**.
- **Empreinte après enregistrement audio interrompu** : **0 Mo** alloué pour le coordinateur (reset immédiat).
- **Variance Soak Test** : **0,00 Mo** (zéro fuite mémoire détectée sur 1 000 notes et 500 requêtes).

## Critères DoD (Definition of Done)
- [x] Banc de test d'évaluation automatisé Golden Dataset opérationnel (50 requêtes curées).
- [x] Récupération gracieuse sans crash lors du débranchement audio à chaud avec génération de WAV 16 kHz.
- [x] Détection des conflits de synchronisation concurrente basée sur le hash SHA-256.
- [x] Profil `[profile.release]` configuré avec LTO, strip, codegen-units = 1 et opt-level = 3.
- [x] Tous les tests unitaires et d'intégration validés (`cargo test -p jeanne-core`).
- [x] Contrôle d'intégrité Tauri IPC & permissions sans erreur (`just check-permissions`).

**STATUS: APPROUVÉ**
