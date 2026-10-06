# Écosystème des Plugins Jeanne (IPC JSON-RPC 2.0)

Ce répertoire héberge les plugins officiels et extensions modulaires de **Jeanne**.

Conformément à la règle d'or **"Zero-Core-Bloat"** ([`AGENTS.md`](file:///home/runner/antigravity/jeanne/AGENTS.md)), le noyau applicatif (`crates/core`) n'embarque aucun runtime d'inférence lourd (C++/Python/FFmpeg) au sein du binaire principal. Les traitements d'inférence locale (LLM), de vectorisation (embeddings), de transcription vocale (STT), de synthèse vocale (TTS) et d'ingestion documentaire complexe (PDF) sont déportés dans des **sous-processus indépendants et éphémères**.

---

## 1. Principes Fondamentaux d'Architecture

1. **Isolation de Processus (Process Isolation & Crash Immunity)** :
   - Chaque plugin s'exécute dans un processus distinct du système d'exploitation (compilé en Go, Rust, C++, Zig ou binaire natif autonome).
   - Un plantage (panic, segmentation fault, out-of-memory OOM) d'un plugin ne fait **jamais** crasher le processus central de Jeanne.
2. **Protocole Standardisé (JSON-RPC 2.0 sur `stdio`)** :
   - Requêtes émises sur `stdin` du sous-processus.
   - Réponses ou événements de streaming renvoyés sur `stdout` (format JSON-RPC 2.0 / NDJSON).
   - Journaux et télémétrie non structurés émis sur `stderr` (capturés par `tracing` côté Jeanne).
3. **Zéro Transfert Binaire Massif ("File-over-IPC")** :
   - Le passage de gros tampons binaires (documents PDF multi-Mo, fichiers audio volumineux) dans les charges utiles JSON-RPC est **strictement proscrit**.
   - Seuls les chemins absolus (`file_path`) ou des segments audio minimaux sont transmis ; le plugin lit directement les données depuis le disque.
4. **Gestion du Cycle de Vie & Éradication des Processus Zombies** :
   - **`on_demand`** : Le plugin est lancé à la demande pour exécuter une tâche (ex. parsing d'un PDF, vectorisation d'un lot de notes) et s'arrête dès que la tâche est finie.
   - **`daemon_managed`** : Le plugin peut être maintenu actif pendant une session de travail (ex. inférence locale interactive), mais dispose d'un arrêt automatique après une durée d'inactivité configurable et est systématiquement terminé à la fermeture de Jeanne (`SIGTERM` / `kill`).
   - Un **Watchdog côté Rust** tue tout processus ne répondant pas dans le temps imparti (`timeout_seconds`).

---

## 2. Structure d'un Répertoire de Plugin

Chaque plugin dispose d'un dossier dédié contenant son manifeste racine `plugin.json` et sa documentation technique `README.md` :

```text
plugins/
├── README.md                     # Ce document de cadrage architectural
├── llm-runner/                   # Plugin 1 : Inférence Locale LLM (GGUF 3B / Vulkan)
│   ├── plugin.json
│   └── README.md
├── embeddings/                   # Plugin 2 : Générateur d'Embeddings RAG (all-MiniLM-L6-v2)
│   ├── plugin.json
│   └── README.md
├── voice-whisper/                # Plugin 3 : Transcription Audio STT (Whisper)
│   ├── plugin.json
│   └── README.md
├── voice-piper/                  # Plugin 4 : Synthèse Vocale TTS (Piper)
│   ├── plugin.json
│   └── README.md
└── pdf-parser/                   # Plugin 5 : Ingestion Documentaire PDF (pdfcpu/Go)
    ├── plugin.json
    └── README.md
```

---

## 3. Schéma du Manifeste (`plugin.json`)

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "schema_version": "1.0",
  "id": "org.jeanneproject.plugin.<nom>",
  "name": "Nom Lisible du Plugin",
  "version": "1.0.0",
  "description": "Description succincte du rôle et des modèles embarqués",
  "author": "Jeanne Project",
  "entrypoint": {
    "windows": "bin/plugin-name.exe",
    "linux": "bin/plugin-name"
  },
  "lifecycle": "on_demand",
  "timeout_seconds": 60,
  "capabilities": [
    {
      "type": "llm_runner | embeddings_generator | voice_stt | voice_tts | document_parser",
      "config": {}
    }
  ]
}
```

---

## 4. Codes d'Erreur Normalisés JSON-RPC 2.0

* `-32700` : **Parse error** (Payload JSON corrompu ou illisible sur `stdin`).
* `-32600` : **Invalid Request** (Structure JSON-RPC non conforme).
* `-32601` : **Method not found** (Méthode demandée non supportée par cette capability).
* `-32602` : **Invalid params** (Paramètres obligatoires manquants ou invalides).
* `-32001` : **Resource Not Found** (Fichier, modèle ou ressource introuvable sur le disque).
* `-32002` : **Execution Error** (Erreur interne de traitement tensoriel, audio ou parsing).
* `-32003` : **Timeout Reached** (Dépassement du délai maximal alloué).
* `-32004` : **Memory Budget Exceeded** (Dépassement du budget mémoire alloué au plugin).
