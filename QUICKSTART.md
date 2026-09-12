# 🚀 Guide de Démarrage Rapide — Jeanne

Ce guide détaille comment lancer **Jeanne** en environnement de développement local.

---

## 1. Méthode la Plus Rapide (Script en 1 clic)

À la racine du dépôt, exécutez le script PowerShell :

```powershell
.\run.ps1
```

> **Alternative Windows (CMD / Double-clic) :**
> Lancez directement le fichier `run.bat`.

Le script s'assure que les dépendances Node.js sont installées et démarre automatiquement le serveur Vite ainsi que l'application de bureau Tauri v2.

---

## 2. Méthode Manuelle (Ligne de commande)

Si vous préférez lancer les commandes manuellement :

```powershell
# 1. Naviguer dans le projet bureau
cd .\apps\desktop

# 2. Installer les dépendances frontend (si pas déjà fait)
npm install

# 3. Lancer l'application en mode développement
npx tauri dev
# ou
npm run tauri dev
```

---

## 💡 Pourquoi `cargo tauri dev` n'a pas fonctionné ?

La commande `cargo tauri` nécessite l'outil global Cargo `cargo-tauri`. Si vous préférez utiliser `cargo tauri` plutôt que `npx tauri dev`, vous pouvez installer la CLI Tauri globale en exécutant :

```powershell
cargo install tauri-cli --version "^2.0.0" --locked
```

Une fois installé, `cargo tauri dev` sera disponible dans `apps/desktop`.

---

## 3. Utilisation de la Palette Flottante (Jalon 1)

Une fois l'application démarrée :

| Action | Raccourci / Déclencheur | Comportement |
| :--- | :--- | :--- |
| **Afficher / Masquer la palette** | `Alt + Espace` | Fait apparaître la fenêtre d'accès rapide au premier plan (ou repli automatique sur `Alt + Maj + Espace` si réservé par Windows). |
| **Recherche instantanée** | Taper du texte | Recherche lexicale FTS5 BM25 avec surlignage dynamique (`<mark>`). |
| **Navigation dans les résultats** | <kbd>↑</kbd> / <kbd>↓</kbd> | Déplace la sélection dans la liste des résultats. |
| **Ouvrir une note** | <kbd>Entrée</kbd> | Ouvre la note sélectionnée dans l'éditeur par défaut du système et ferme la palette. |
| **Consigner une note rapide** | `/note [votre texte]` + <kbd>Entrée</kbd> | Ajoute le contenu horodaté dans `Journal/YYYY-MM-DD.md` et l'indexe instantanément. |
| **Fermer la palette** | <kbd>Échap</kbd> ou clic extérieur | Masquage instantané (perte de focus `blur`). |

---

## 4. Commandes de Validation & Tests

Pour valider l'intégrité du code avant commit :

```powershell
# Vérification des types Rust (0 erreur, 0 avertissement exigé)
cargo check --workspace

# Exécution de la suite de tests automatisés (moteur + bureau)
cargo test --workspace

# Compilation du frontend Svelte 5
cd apps/desktop
npm run build
```
