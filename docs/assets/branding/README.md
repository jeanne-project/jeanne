# Charte Graphique & Actifs de Marque Jeanne

Ce répertoire regroupe les actifs graphiques officiels de Jeanne, validés selon une double déclinaison d'usage.

## 1. Vue d'Ensemble des Identités

| Actif | Fichiers | Usage Cible | Description |
| :--- | :--- | :--- | :--- |
| **Option 1 : Le Sceau du Savoir** | `jeanne_symbol.png`, `jeanne_symbol.jpg` | **Documentation & Web** : En-têtes Markdown, icône de site web / favicon, bannières, documentation technique. | Plume calligraphique stylisée en « J » sur feuille pliée origami avec trame Markdown subtile. Fond bleu nuit profond (#0d1322) avec accents or doux (#d4af37). |
| **Option 2 : L'Assistante Moderne** | `jeanne_avatar.png`, `jeanne_avatar.jpg` | **Application & OS** : Icône native de l'application (Tauri v2), barre des tâches, Dock macOS, lanceurs mobiles et icône Systray. | Portrait épuré et moderne d'une assistante/secrétaire professionnelle bienveillante avec lunettes élégantes et micro-casque discret. |

## 2. Déclinaisons Tauri v2 de l'Application

Les déclinaisons natives de l'Option 2 sont générées et synchronisées sous `apps/desktop/src-tauri/icons/` :
- `icon.png` (512×512)
- `128x128@2x.png` (256×256)
- `128x128.png` (128×128)
- `64x64.png` (64×64)
- `32x32.png` (32×32)
- `icon.ico` (Windows multi-taille)
- `icon.icns` (macOS multi-taille)
- Formats spécifiques Windows Store / Android / iOS

---

## 3. Retouches Graphiques à Réaliser (Dette Graphique & Backlog)

> **Statut** : En attente de traitement manuel (outils vectoriels / retouche graphique type Figma, Photoshop, Affinity ou Illustrator).
> **Contexte** : Les images initiales générées par IA comportent un canevas externe de mise en scène (fond blanc avec ombre portée pour l'avatar, fond gris-ardoise pour le symbole) qu'il convient de retirer pour obtenir des fichiers transparents nets.

### Spécifications des Retouches

1. **Option 1 : Le Sceau du Savoir (`jeanne_symbol.png`)** :
   - **Problème actuel** : Présence d'un fond gris-ardoise externe autour du cadre carré.
   - **Action attendue** :
     - Supprimer le canevas externe.
     - Recadrer au plus près de l'emblème avec canal alpha transparent.
     - Veiller à préserver la translucidité des halos lumineux (lueurs cyan et éclats dorés) sans créer de frange noire (*fringing*).
   - **Fichier cible** : `docs/assets/branding/jeanne_symbol.png` (carré, 1024×1024, PNG-32 avec transparence).

2. **Option 2 : L'Assistante Moderne (`jeanne_avatar.png`)** :
   - **Problème actuel** : Présence d'un fond blanc rectangulaire et d'une ombre portée asymétrique autour de la tuile arrondie (*squircle*), ce qui produit des coins blancs visibles dans le systray ou sur fond sombre.
   - **Action attendue** :
     - Supprimer le fond blanc et l'ombre portée externe.
     - Recadrer la tuile de manière parfaitement centrée et symétrique.
     - Rendre les 4 coins extérieurs de l'arrondi (*squircle*) transparents (canal alpha).
     - *(Variante possible)* : Détourage complet du buste si l'on souhaite un avatar sans tuile d'arrière-plan.
   - **Fichier cible** : `docs/assets/branding/jeanne_avatar.png` (carré, 1024×1024, PNG-32 avec transparence).

### Procédure de Déploiement Après Retouche

Une fois les fichiers PNG transparents ajustés et sauvegardés dans `docs/assets/branding/` :
1. Régénérer l'ensemble des formats applicatifs natifs (desktop, systray, mobiles, ICO, ICNS) via la CLI Tauri :
   ```bash
   cd apps/desktop
   npx @tauri-apps/cli icon --output src-tauri/icons ../../docs/assets/branding/jeanne_avatar.png
   ```
2. Valider l'affichage sur la documentation et l'interface :
   ```bash
   cargo check --workspace
   npm run build
   ```
3. Commiter les nouveaux actifs graphiques.
