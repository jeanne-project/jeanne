@echo off
setlocal
cd /d "%~dp0apps\desktop"
echo ==========================================
echo     Jeanne - Assistant IA Souverain
echo ==========================================
echo Lancement en mode developpement (Vite + Tauri v2)...
echo [Raccourci palette : Alt + Espace]
call npx tauri dev
pause
