<#
.SYNOPSIS
    Lance l'application Jeanne Desktop en mode développement.
.DESCRIPTION
    Vérifie les prérequis (Rust/Cargo, Node.js/npm), installe les dépendances si nécessaire et démarre le serveur Vite + Tauri v2.
#>
[CmdletBinding()]
param (
    [switch]$BuildOnly
)

$ErrorActionPreference = "Stop"

$ProjectRoot = $PSScriptRoot
$DesktopDir = Join-Path $ProjectRoot "apps\desktop"

Write-Host "==========================================" -ForegroundColor Cyan
Write-Host "    Jeanne — Assistant IA Souverain       " -ForegroundColor Cyan
Write-Host "==========================================" -ForegroundColor Cyan

# 1. Vérification des prérequis
if (-not (Get-Command npm -ErrorAction SilentlyContinue)) {
    Write-Error "Node.js / npm n'est pas installé ou absent du PATH. Installez Node.js v18+."
    exit 1
}

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Error "Cargo / Rust n'est pas installé ou absent du PATH. Installez Rust via rustup."
    exit 1
}

# 2. Vérification des dépendances npm
if (-not (Test-Path (Join-Path $DesktopDir "node_modules"))) {
    Write-Host "[1/2] Installation des dépendances npm..." -ForegroundColor Yellow
    Push-Location $DesktopDir
    npm install
    Pop-Location
} else {
    Write-Host "[1/2] Dépendances npm prêtes." -ForegroundColor Green
}

# 3. Lancement
if ($BuildOnly) {
    Write-Host "[2/2] Construction du binaire de production..." -ForegroundColor Yellow
    Push-Location $DesktopDir
    npx tauri build
    Pop-Location
} else {
    Write-Host "[2/2] Démarrage de Jeanne en mode développement..." -ForegroundColor Yellow
    Write-Host "      • Dashboard principal : fenêtre bureau" -ForegroundColor DarkGray
    Write-Host "      • Palette flottante    : Alt + Espace" -ForegroundColor DarkGray
    Write-Host "      • Arrêt               : Ctrl + C" -ForegroundColor DarkGray
    Push-Location $DesktopDir
    npx tauri dev
    Pop-Location
}
