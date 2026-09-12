---
name: "sdd-workflow"
description: "Protocole opérationnel Spec-Driven Development (SDD) couplé au TDD pour l'implémentation par tranches verticales."
---

# Spec-Driven Development (SDD) & TDD Execution Runbook

Ce runbook régit le cycle de développement de chaque jalon dans le projet **Jeanne**. Le code n'est qu'un sous-produit dérivé de la spécification technique.

---

## 1. Le Cycle en 4 Phases

```
┌────────────────────────────────────────────────────────┐
│  Phase 1 : Formalisation de la Spécification           │
│  - Rédaction ou audit du contrat dans docs/specs/      │
│  - Validation des types Rust, SQL DDL et IPC           │
└───────────────────────────┬────────────────────────────┘
                            │
                            ▼
┌────────────────────────────────────────────────────────┐
│  Phase 2 : Phase Rouge (TDD)                           │
│  - Écriture des tests d'intégration et unitaires       │
│  - Échec attendu et vérifié (Red State)                │
└───────────────────────────┬────────────────────────────┘
                            │
                            ▼
┌────────────────────────────────────────────────────────┐
│  Phase 3 : Phase Verte                                 │
│  - Implémentation minimale stricte                     │
│  - Validation du passage des tests (Green State)       │
└───────────────────────────┬────────────────────────────┘
                            │
                            ▼
┌────────────────────────────────────────────────────────┐
│  Phase 4 : Refactoring & Audit de Clôture              │
│  - Contrôle du budget mémoire (RAM < 80 Mo / 4,5 Go)   │
│  - cargo clippy --workspace && cargo test --workspace  │
│  - Validation de la matrice d'acceptation de la spec   │
└────────────────────────────────────────────────────────┘
```

---

## 2. Directives Opérationnelles par Phase

### Phase 1 : Cadrage & Contrats (`docs/specs/<id>_SPEC_<nom>.md`)
Avant d'écrire du code applicatif sous `crates/` ou `apps/`, la spec doit définir :
1. **Structures et Énumérations** : Définition formelle des types Rust avec attributs `serde`.
2. **Schéma SQL & DDL** : Tables réelles, tables virtuelles (`FTS5`, `sqlite-vec`), triggers et pragmas.
3. **Contrats IPC Tauri v2** : Noms exacts des commandes, arguments typés et permissions de capabilities (`capabilities/default.json`).
4. **Matrice de Tests** : Liste exhaustive des cas nominaux, limites et erreurs.
5. **Budgets Matériels** : Plafonds RAM et latences cibles.

### Phase 2 : Phase Rouge (Test-First)
* Tout nouveau comportement doit posséder un test unitaire ou d'intégration écrit **avant** son implémentation métier.
* Les tests de base de données doivent s'exécuter sur une base SQLite temporaire isolée.
* `cargo test` doit compiler et échouer pour la raison exacte documentée dans la spec.

### Phase 3 : Phase Verte (Minimal Implementation)
* Implémenter uniquement le code nécessaire pour faire passer les tests.
* Interdiction d'ajouter des abstractions anticipatives non prévues dans la spec (*YAGNI*).
* Respecter l'interdiction de `unwrap()` ou `expect()` dans le code métier (`crates/core`).

### Phase 4 : Audit de Clôture
* Vérifier le non-dépassement des plafonds de RAM alloués.
* Exécuter `cargo clippy --workspace --all-targets -- -D warnings`.
* Valider les critères de la spécification.

---

## 3. Table d'Anti-Rationalisation

| Excuse fréquente de l'Agent IA | Réfutation formelle (Comportement Requis) |
| :--- | :--- |
| *« Je vais d'abord coder la fonctionnalité, puis j'écrirai les tests pour gagner du temps. »* | **Rejet immédiat**. La spec dicte les tests ; les tests dictent le code. La Phase Rouge est un prérequis absolu. |
| *« C'est une petite fonction utilitaire, pas besoin de mettre à jour la spec. »* | **Rejet**. Toute signature publique ou modification de schéma doit être consignée dans le contrat de spec. |
| *« La spec ne mentionne pas la gestion de cette erreur, j'utilise un `unwrap()`. »* | **Rejet**. Le code de production doit mapper toute erreur vers un type `thiserror` documenté. |
