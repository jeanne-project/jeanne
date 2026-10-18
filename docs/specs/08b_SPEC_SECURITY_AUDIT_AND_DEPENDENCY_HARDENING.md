# Spécification Technique : Durcissement Sécurité & Résolution Dépendances (08b_SPEC_SECURITY_AUDIT_AND_DEPENDENCY_HARDENING.md)

Ce document formalise la spécification technique pour la résolution des vulnérabilités de sécurité RustSec, la correction des conflits de workspace pour Renovate et la fiabilisation des tests d'exécution de plugins sous Linux.

---

## 1. Vue d'Ensemble & Objectifs

### 1.1 Contexte & Justification
1. **Vulnérabilité critique RustSec (rustls v0.23.45)** :
   Le 14 octobre 2026, 4 avis de sécurité ont été publiés contre `rustls` v0.23.45 (advisories RUSTSEC-2026-0346, RUSTSEC-2026-0347, RUSTSEC-2026-0348, RUSTSEC-2026-0349), bloquant l'ensemble des pipelines CI de sécurité (`cargo-audit`) tant sur la branche `main` que sur les pull requests dépendantes.
2. **Conflit de Workspace Cargo (Renovate Artifacts)** :
   Le paquet d'évaluation `.agent/skills/rust-skills/checks/Cargo.toml` n'était ni membre du workspace racine ni formellement exclu (`workspace.exclude`), provoquant l'échec de synchronisation de `Cargo.lock` dans l'automatisation Mend Renovate (`error: current package believes it's in a workspace when it's not`).
3. **Flakiness I/O sous Linux (`ETXTBSY`)** :
   Dans `crates/core/tests/plugin_pdf_test.rs`, la création et l'exécution immédiate de scripts de test sans temporisation pouvait déclencher par intermittence l'erreur OS 26 (`ExecutableFileBusy` / `ETXTBSY`) en environnement CI hautement parallèle.

### 1.2 Objectifs de la Tranche
1. **Mise à jour de sécurité de `rustls`** vers `>=0.23.46` dans `Cargo.lock` pour éradiquer les 4 CVEs/RUSTSEC.
2. **Déclaration d'exclusion du sous-dossier de checks** dans le manifest racine (`Cargo.toml`) et ajout de `[workspace]` autonome dans `.agent/skills/rust-skills/checks/Cargo.toml`.
3. **Mécanisme de retry sur `ETXTBSY`** dans les tests de plugins pour assurer 100% de déterminisme en CI.
4. **Validation intégrale de la chaîne CI locale** : `cargo audit`, `just pre-review` (permissions, formattage, clippy, tests, build).

---

## 2. Contrats d'Interface & Modifications

### 2.1 Workspace Cargo Racine (`Cargo.toml`)
Ajout de la section d'exclusion :
```toml
[workspace]
resolver = "2"
members = [
    "crates/core",
    "crates/cli",
    "apps/desktop/src-tauri"
]
exclude = [
    ".agent/skills/rust-skills/checks"
]
```

### 2.2 Manifest Autonome Checks (`.agent/skills/rust-skills/checks/Cargo.toml`)
Ajout explicite de la table `[workspace]` pour isolation hermétique.

### 2.3 Résilience des Tests de Plugins (`crates/core/tests/plugin_pdf_test.rs`)
Gestion du code d'erreur OS 26 (`ETXTBSY`) par boucle d'attente courte (50 ms, jusqu'à 5 tentatives) lors des tests de processus éphémères.

---

## 3. Matrice de Tests & Critères d'Acceptation

| ID | Test / Contrôle | Description | Comportement Attendu |
| :--- | :--- | :--- | :--- |
| **SEC-01** | `cargo audit` | Audit des vulnérabilités de `Cargo.lock` | 0 vulnérabilité détectée (Code retour 0) |
| **WKS-01** | `cargo check --workspace` | Contrôle d'intégrité du workspace Cargo | Découverte propre sans avertissement de workspace orphelin |
| **TST-01** | `plugin_pdf_test` | Tests d'exécution JSON-RPC et immunité crashs | 3/3 tests verts sans panique `ETXTBSY` |
| **REV-01** | `just pre-review` | Contrôles déterministes obligatoires | Clippy 0 warnings, tests 100% verts, build Svelte 5 OK |
