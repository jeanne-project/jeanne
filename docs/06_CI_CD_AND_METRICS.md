# 06 - Stratégie CI/CD & Métriques de Performance

## 1. Vision DevOps & Invariants de Qualité

Dans le cadre du développement souverain et local-first de **Jeanne**, l'infrastructure d'intégration et de livraison continues (CI/CD) applique strictement les principes de **Spec-Driven Development (SDD)** et la protection des budgets matériels (16 Go de RAM unifiée / iGPU partagé 8 Go max).

Chaque pipeline agit comme un garde-fou déterministe pour interdire toute dérive architecturale :
* **Tolérance Zéro Dette Statique** : Aucune alerte de formatage (`rustfmt`) ou d'analyse (`clippy -D warnings`), zéro appel résiduel à `.unwrap()` ou `.expect()` dans le domaine central (`crates/core`).
* **Conformité & Intégrité** : Audit systématique des licences permissives (`cargo-deny`) et détection précoce des fuites de clés API ou secrets (`gitleaks`).
* **Packaging Multiplateforme Strict** : Validation systématique de la compilation native sans régression sur Linux, Windows et macOS pour Tauri v2.

---

## 2. Pipelines GitHub Actions Actifs (Phase Immédiate)

Ces workflows sont exécutés sur l'infrastructure GitHub Actions via `.github/workflows/` et exploitent le cache Rust partagé (`Swatinem/rust-cache`) pour minimiser les temps de cycle (< 3 minutes).

### 2.1 Fast Feedback CI (`ci.yml`)
* **Déclencheurs** : `push` sur `main` et toutes les `pull_request`.
* **Rôle** : Validation rapide du code Rust et de l'interface Svelte 5.
* **Jobs** :
  1. `rust-static-and-lint` :
     - Vérification du formattage via `cargo fmt --check`.
     - Contrôle strict par expression régulière de l'absence de panics (`.unwrap()` et `.expect()`) dans `crates/core/src/`.
     - Analyse linter stricte via `cargo clippy --workspace --all-targets -- -D warnings`.
  2. `rust-test` :
     - Exécution intégrale de la suite de tests unitaires et d'intégration (`cargo test --workspace`).
     - Validation du moteur SQLite WAL, de l'indexation FTS5 BM25, des triggers et de la détection de frontmatter.
  3. `frontend` :
     - Installation hermétique des dépendances UI (`npm ci`).
     - Compilation TypeScript et Svelte 5 via `npm run build` dans `apps/desktop`.

### 2.2 Sécurité, Dépendances & Licences (`security.yml`)
* **Déclencheurs** : `push` sur `main`, `pull_request`, et exécution planifiée hebdomadaire (lundi à 03:00 UTC).
* **Rôle** : Sécurisation de la chaîne d'approvisionnement logicielle et intégrité légale.
* **Jobs** :
  1. `gitleaks` : Scan complet de l'historique Git via le binaire officiel autonome open-source `gitleaks` (évitant les restrictions de licence commerciale de l'Action v2 en contexte organisation GitHub).
  2. `cargo-audit` : Détection des vulnérabilités connues dans l'arbre des dépendances Rust via la base Advisory de RustSec.
  3. `cargo-deny` : Contrôle de la conformité des licences (autorisant strictement MIT, Apache-2.0, BSD-3-Clause, ISC, Unicode, CC0) et détection des doublons de crates non autorisés (`deny.toml`).

### 2.3 Validation de Compilation Tauri v2 (`build-check.yml`)
* **Déclencheurs** : `pull_request` ciblant `main` avec modifications dans `apps/desktop/**`, `crates/**`, `Cargo.toml` ou `Cargo.lock`.
* **Matrice d'OS** : `windows-latest`, `ubuntu-22.04`, `macos-latest`.
* **Rôle** : Garantir la compilabilité native de la coquille Tauri v2 sans encourir le coût temporel du packaging d'installateurs (`--no-bundle`).
* **Dépendances système Linux** : `libwebkit2gtk-4.1-dev`, `libappindicator3-dev`, `librsvg2-dev`, `patchelf`, `libssl-dev`, `libxdo-dev`.

### 2.4 Packaging & Publication Automatisée (`release.yml`)
* **Déclencheur** : `push` de tags Git respectant le gabarit de version `v*` (ex: `v0.1.0-m1`, `v0.2.0-m2`).
* **Matrice d'OS** : `windows-latest`, `ubuntu-22.04`, `macos-latest`.
* **Rôle** : Génération des artefacts finaux de distribution via `tauri-apps/tauri-action` :
  - **Windows** : Installateur exécutable NSIS (`.exe`) et paquetage Windows Installer (`.msi`).
  - **Linux** : Paquet Debian (`.deb`) et paquet portable universel (`.AppImage`).
  - **macOS** : Image disque universelle (`.dmg`).
* **Publication** : Création d'une release GitHub au statut *brouillon* (*draft release*), téléversement des binaires signés et génération des sommes de contrôle cryptographiques `SHA256SUMS`.
* **Signature de Code (Feuille de route Jalon 8)** : L'injection des certificats d'autorité (Apple Developer ID avec notarisation `altool`/`notarytool`, Windows Authenticode) et de la clé privée de mise à jour Tauri (`TAURI_SIGNING_PRIVATE_KEY`) est planifiée pour le Jalon 8 avant publication grand public.

---

## 3. Catalogue des Pipelines Différés & Feuille de Route d'Activation

Pour ne pas surcharger la CI avant que les composants correspondants ne soient implémentés, les pipelines lourds ou spécialisés sont planifiés selon les jalons de la roadmap (`docs/04_ROADMAP_AND_MILESTONES.md`).

```
Milestone 1 & 2 (Actuel)       ──> Fast CI, Security, Tauri Build Matrix, Tagged Release
Milestone 3 & 4 (Inférence)     ──> Continuous Benchmarking, Memory Gatekeeper, Proxmox Self-Hosted Runner
Milestone 8 (Hardening final)  ──> Eval-Driven Maintenance RAG (Golden Dataset, Faithfulness)
```

### 3.1 Jalons 3 & 4 : Inférence Distante & Locale

#### A. Continuous Benchmarking (`Criterion.rs` / `CodSpeed`)
* **Objectif** : Détecter automatiquement toute dégradation de performance sur les chemins critiques :
  - Découpage de texte sémantique (*Semantic Chunking*).
  - Vitesse de vectorisation locale (modèle `fastembed` BGE-Small sur CPU).
  - Latence de recherche hybride ($S_{\text{vector}} + S_{\text{BM25}} \times \text{TimeDecay}$) devant rester strictly $< 30\text{ ms}$ pour 10 000 chunks.
  - Débit de génération de tokens en inférence locale Vulkan 3B Q4 ($\ge 25\text{ tok/s}$).
* **Action CI** : Pipeline activé sur PR déclenchant `cargo bench` avec comparaison de baseline et alerte en cas de régression $> 10\%$.

#### B. Memory Budget Gatekeeper (Audit RSS Automatisé)
* **Objectif** : Empêcher le dépassement des seuils de mémoire vive définis dans `AGENTS.md` :
  - Veille / arrière-plan : RAM résidente $\text{RSS} < 80\text{ Mo}$.
  - Mode client distant : $\text{RSS} < 150\text{ Mo}$.
  - Mode inférence locale (3B quantifié) : $\text{RSS} < 4,5\text{ Go}$.
* **Action CI** : Script de test d'endurance exécutant une simulation d'ingestion massive et mesurant les pics de `VmRSS` via `/proc/$PID/status` sous Linux. Blocage de la PR si la mémoire résiduelle dépasse le plafond.

#### C. Runner Auto-hébergé Proxmox (Ryzen AI MAX+ 395)
* **Objectif** :
  - Fournir un environnement matériel identique à la cible de production (architecture AMD x86_64, iGPU Radeon 8060S / NPU).
  - Accélérer drastiquement les temps de compilation C++ et Rust liés à `llama.cpp` et `sqlite-vec`.
  - Économiser les quotas de minutes GitHub Actions sur les tests lourds d'inférence.
* **Mise en œuvre** : Enregistrement d'un GitHub Actions Runner auto-hébergé sous Proxmox VE dans un conteneur LXC Debian optimisé avec accès direct aux périphériques GPU `/dev/kfd` et `/dev/dri`.

### 3.2 Jalon 8 : Hardening, Évaluation & Packaging Final

#### A. Eval-Driven Maintenance RAG (Banc Golden Dataset)
* **Objectif** : Évaluer scientifiquement la qualité des réponses générées et l'exactitude contextuelle sans intervention humaine.
* **Outils d'évaluation** : Intégration de frameworks d'évaluation RAG (Ragas / DeepEval / Golden Dataset natif).
* **Métriques sous surveillance stricte** :
  - **Fidélité contextuelle (*Faithfulness*)** : $\ge 95\%$ (zéro hallucination sur faits inventés).
  - **Rappel documentaire (*Context Recall*)** : $\ge 90\%$ (pertinence des chunks remontés).
  - **Filtrage d'Obsolescence** : $100\%$ de conformité sur l'omission des notes marquées `status: deprecated`.
  - **Résolution 1-hop** : Vérification systématique du remplacement des concepts dépréciés par leur successeur via `superseded_by`.
* **Action CI** : Job nocturne (*nightly*) exécutant le banc complet de 50 cas d'usage réels et publiant un rapport synthétique de scoring dans les artefacts de build.

---

## 4. Matrice Récapitulative des Pipelines

| Pipeline | Fréquence / Déclencheur | Environnement / Runner | Livrables / Objectif | Statut |
| :--- | :--- | :--- | :--- | :--- |
| **Fast CI** | `push main`, `pull_request` | GitHub Hosted `ubuntu-22.04` | Format, Clippy strict, Tests, Build UI | **Actif** |
| **Security & Licenses** | PR, push, Hebdo (Lundi 3h) | GitHub Hosted `ubuntu-22.04` | Gitleaks, cargo-audit, cargo-deny | **Actif** |
| **Tauri Matrix Build** | PR touchant `apps/` ou `crates/` | Windows, Ubuntu, macOS | Validation compilation cross-platform | **Actif** |
| **Release Packaging** | Tag `v*` | Windows, Ubuntu, macOS | NSIS, MSI, DEB, AppImage, DMG, SHA-256 | **Actif** |
| **Criterion Benchmarks** | PR de jalon RAG / Inférence | Proxmox Runner auto-hébergé | Suivi de latence et débits tok/s | *Jalon 3-4* |
| **RSS Gatekeeper** | PR de jalon RAG / Inférence | Linux bare-metal | Validation budget RSS < 80 Mo / 4,5 Go | *Jalon 3-4* |
| **RAG Golden Dataset** | Nightly & Pré-release Jalon 8 | Proxmox Runner auto-hébergé | Scores Faithfulness & Recall $\ge 90\%$ | *Jalon 8* |
