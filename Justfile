set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

export PATH := "/home/runner/.cargo/bin:" + env_var_or_default("PATH", "")
export PKG_CONFIG_PATH := "/home/runner/.local/usr/lib/x86_64-linux-gnu/pkgconfig:" + env_var_or_default("PKG_CONFIG_PATH", "")
export C_INCLUDE_PATH := "/home/runner/.local/usr/include:" + env_var_or_default("C_INCLUDE_PATH", "")
export CPLUS_INCLUDE_PATH := "/home/runner/.local/usr/include:" + env_var_or_default("CPLUS_INCLUDE_PATH", "")
export RUSTFLAGS := "-L native=/home/runner/.local/usr/lib/x86_64-linux-gnu " + env_var_or_default("RUSTFLAGS", "")
export LD_LIBRARY_PATH := "/home/runner/.local/usr/lib/x86_64-linux-gnu:" + env_var_or_default("LD_LIBRARY_PATH", "")


# Affiche les commandes disponibles
default:
    @just --list

# Démarre une branche pour un jalon (ex: just start-milestone 02 rag)
start-milestone id name:
    git checkout main
    git pull origin main
    git checkout -b feat/m{{id}}-{{name}}
    @echo "Branche feat/m{{id}}-{{name}} créée."

# Contrôle la synchronisation des commandes IPC, capabilities et fichiers TOML de permissions
check-permissions:
    @node scripts/check_tauri_permissions.mjs

# Vérifie qu'aucun fichier non suivi (untracked) ou modifié n'a été oublié
[unix]
check-git-clean:
    @if [ -n "$$(git status --porcelain)" ]; then echo "❌ Erreur : Fichiers non commités ou non suivis détectés dans l'arbre de travail :"; git status -s; exit 1; fi
    @echo "✅ Arbre Git propre (zéro fichier non suivi ou non commité)."

[windows]
check-git-clean:
    @if ($$(git status --porcelain)) { Write-Error "❌ Erreur : Fichiers non commités ou non suivis détectés dans l'arbre de travail."; git status -s; exit 1 }
    @echo "✅ Arbre Git propre (zéro fichier non suivi ou non commité)."

# Contrôles locaux déterministes obligatoires avant appel au Reviewer
pre-review:
    @echo "=== [Pre-Review] Exécution des contrôles qualité ==="
    just check-permissions
    cargo clippy -p jeanne-core --all-targets -- -D warnings
    cargo test -p jeanne-core
    cd apps/desktop && npm run build
    @echo "✅ Contrôles statiques, tests et permissions validés."

# Initialise le fichier de revue pour le Reviewer
[windows]
init-review milestone:
    @New-Item -ItemType Directory -Force -Path docs/reviews | Out-Null
    @Set-Content -Path docs/reviews/M{{milestone}}_CODE_REVIEW.md -Value "# Code Review - Jalon {{milestone}}`n`nSTATUS: EN_ATTENTE`n`n## Bloquants`n`n## Avertissements`n"
    @echo "docs/reviews/M{{milestone}}_CODE_REVIEW.md initialisé."

[unix]
init-review milestone:
    @mkdir -p docs/reviews
    @printf "# Code Review - Jalon %s\n\nSTATUS: EN_ATTENTE\n\n## Bloquants\n\n## Avertissements\n" "{{milestone}}" > docs/reviews/M{{milestone}}_CODE_REVIEW.md
    @echo "docs/reviews/M{{milestone}}_CODE_REVIEW.md initialisé."

# Initialise le rapport de QA pour le QA-Profiler
[windows]
init-qa milestone:
    @New-Item -ItemType Directory -Force -Path docs/reviews | Out-Null
    @Set-Content -Path docs/reviews/M{{milestone}}_QA_REPORT.md -Value "# QA & Profiling Report - Jalon {{milestone}}`n`nSTATUS: EN_ATTENTE`n`n## Empreinte Mémoire (RSS)`n`n## Critères DoD`n"
    @echo "docs/reviews/M{{milestone}}_QA_REPORT.md initialisé."

[unix]
init-qa milestone:
    @mkdir -p docs/reviews
    @printf "# QA & Profiling Report - Jalon %s\n\nSTATUS: EN_ATTENTE\n\n## Empreinte Mémoire (RSS)\n\n## Critères DoD\n" "{{milestone}}" > docs/reviews/M{{milestone}}_QA_REPORT.md
    @echo "docs/reviews/M{{milestone}}_QA_REPORT.md initialisé."

# Fusionne la branche de jalon sur main après approbation stricte
[windows]
merge-milestone id name:
    @echo "Vérification des permissions et de l'arbre Git..."
    just check-permissions
    just check-git-clean
    @echo "Vérification de l'approbation de la revue..."
    @Select-String -Path docs/reviews/M{{id}}_CODE_REVIEW.md -Pattern "STATUS: APPROUVÉ" -Quiet | ForEach-Object { if (-not $_) { echo "❌ Erreur : La revue n'est pas marquée 'STATUS: APPROUVÉ'"; exit 1 } }
    git checkout main
    git merge --no-ff feat/m{{id}}-{{name}} -m "feat(milestone-{{id}}): merge validated slice {{name}}"
    @echo "✅ Branche feat/m{{id}}-{{name}} fusionnée avec succès sur main."

[unix]
merge-milestone id name:
    @echo "Vérification des permissions et de l'arbre Git..."
    just check-permissions
    just check-git-clean
    @echo "Vérification de l'approbation de la revue..."
    @if ! grep -q "STATUS: APPROUVÉ" docs/reviews/M{{id}}_CODE_REVIEW.md; then echo "❌ Erreur : La revue n'est pas marquée 'STATUS: APPROUVÉ'"; exit 1; fi
    git checkout main
    git merge --no-ff feat/m{{id}}-{{name}} -m "feat(milestone-{{id}}): merge validated slice {{name}}"
    @echo "✅ Branche feat/m{{id}}-{{name}} fusionnée avec succès sur main."

# Ajoute un worktree pour isoler une tâche d'audit ou de test
worktree-add name branch:
    git worktree add .worktrees/{{name}} {{branch}}
    @echo "Worktree .worktrees/{{name}} créé sur {{branch}}."

# Nettoie un worktree
worktree-clean name:
    git worktree remove .worktrees/{{name}}
    @echo "Worktree .worktrees/{{name}} supprimé."
