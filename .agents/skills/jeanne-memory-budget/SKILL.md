---
name: "jeanne-memory-budget"
description: "Garde-fous stricts d'allocation mémoire pour PC 16 Go avec iGPU partagé."
---

# Jeanne Memory Budget Guardrails

## 1. Plafonds d'Allocation RAM
* Veille / Arrière-plan : **< 80 Mo**
* Client distant (API OpenAI) : **< 150 Mo**
* Inférence locale active (3B Q4) : **< 4,5 Go**

## 2. Règles d'Implémentation
1. Interdiction de charger des fichiers médias, audio ou documents complets en mémoire (`std::fs::read` proscrit). Utiliser `BufReader` et des flux I/O par blocs $\le 64$ Ko.
2. Écriture immédiate des buffers audio sur disque : ne jamais conserver plus de 5 secondes de PCM en RAM.
3. Déchargement obligatoire (`unload()`) des contextes lourds après exécution.
