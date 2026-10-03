@echo off
setlocal

:: Activation du mode DEBUG par défaut pour un diagnostic complet
set RUST_LOG=debug,jeanne_core=debug,jeanne_desktop=debug,tauri=info
set RUST_BACKTRACE=1
set VITE_DEBUG=true

cd /d "%~dp0apps\desktop"
echo ==========================================
echo     Jeanne - Assistant IA Souverain
echo ==========================================
echo [MODE DEBUG ACTIF] RUST_LOG=%RUST_LOG%
echo Lancement en mode developpement (Vite + Tauri v2)...
echo [Raccourci palette : Alt + Espace]
echo.
call npx tauri dev
pause
