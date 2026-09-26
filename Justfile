set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

# Affiche les commandes disponibles
default:
    @just --list

# Démarre une branche pour un jalon (ex: just start-milestone 02 rag)
start-milestone id name:
    git checkout main
    git pull origin main
    git checkout -b feat/m{{id}}-{{name}}
    @echo "Branche feat/m{{id}}-{{name}} créée."

# Contrôles locaux déterministes obligatoires avant appel au Reviewer
pre-review:
    @echo "=== [Pre-Review] Exécution des contrôles qualité ==="
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    cd apps/desktop && npm run build
    @echo "✅ Contrôles statiques et tests validés."

# Initialise le fichier de revue pour le Reviewer
init-review milestone:
    @New-Item -ItemType Directory -Force -Path docs/reviews | Out-Null
    @Set-Content -Path docs/reviews/M{{milestone}}_CODE_REVIEW.md -Value "# Code Review - Jalon {{milestone}}`n`nSTATUS: EN_ATTENTE`n`n## Bloquants`n`n## Avertissements`n"
    @echo "docs/reviews/M{{milestone}}_CODE_REVIEW.md initialisé."

# Initialise le rapport de QA pour le QA-Profiler
init-qa milestone:
    @New-Item -ItemType Directory -Force -Path docs/reviews | Out-Null
    @Set-Content -Path docs/reviews/M{{milestone}}_QA_REPORT.md -Value "# QA & Profiling Report - Jalon {{milestone}}`n`nSTATUS: EN_ATTENTE`n`n## Empreinte Mémoire (RSS)`n`n## Critères DoD`n"
    @echo "docs/reviews/M{{milestone}}_QA_REPORT.md initialisé."

# Fusionne la branche de jalon sur main après approbation stricte
merge-milestone id name:
    @echo "Vérification de l'approbation de la revue..."
    @Select-String -Path docs/reviews/M{{id}}_CODE_REVIEW.md -Pattern "STATUS: APPROUVÉ" -Quiet | ForEach-Object { if (-not $_) { echo "❌ Erreur : La revue n'est pas marquée 'STATUS: APPROUVÉ'"; exit 1 } }
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
