# 🧠 Jeanne LLM Benchmark Tool

Outil indépendant et déterministe d'évaluation des modèles de langage (LLMs) pour le projet **Jeanne**.

Cet outil permet de mesurer rigoureusement l'adéquation, la fiabilité et les performances (TTFT, débit TPS, respect des formats, citations RAG, préservation PII) de différents LLMs (locaux ou distants) en fonction de votre matériel.

> 🛡️ **Isolation Garantie** : Cet outil ne dépend d'aucun code interne de Jeanne et n'est pas embarqué dans son build (`Cargo.lock` / Tauri). Il ne modifie aucun fichier de configuration ni aucun coffre utilisateur.

---

## 🎯 Fonctionnalités Clés

1. **Reproductibilité & Déterminisme Absolu** :
   - Évaluation programmatique stricte sans « LLM-as-a-judge » : expressions régulières, validateurs JSON, analyseurs de citations et détection de mots-clés de rejet.
   - Tous les hyperparamètres d'inférence (`temperature`, `seed`, `top_p`, `max_tokens`, etc.) sont appliqués de manière contrôlée et explicitement consignés dans chaque rapport.
2. **Moteur d'Inférence Embarqué Direct (In-Process GGUF)** :
   - Capable d'exécuter des fichiers `.gguf` directement en mémoire **sans aucun serveur externe ni intermédiaire réseau** grâce aux bindings natifs `llama.cpp`.
   - Conforme au runtime de Jeanne : contexte KV borné à 4096 tokens, accélération matérielle Vulkan/Metal/CUDA et libération immédiate de la RAM à la fin du test.
   - Supporte également le mode distant ou démon (Ollama, `llama-server`, LM Studio, OpenAI) via l'interface `/v1`.
3. **Support Multi-Profils d'Inférence** :
   - Permet d'exécuter des séries de tests avec différents profils (ex. `deterministic_strict` à $T=0.0$, `balanced` à $T=0.3$, etc.) sur un même modèle ou plusieurs modèles.
4. **Cas d'Usage Réels de Jeanne** :
   - **RAG & Citations** : Synthèse sous contrainte avec obligation de citer `[source: nom_note.md]` et refus propre sans hallucination pour les questions hors-domaine.
   - **Préservation PII** : Rétention stricte des tokens masqués (`[PERSON_1]`, `[EMAIL_1]`, etc.) sans corruption.
   - **Sortie Structurée Palette** : Génération de JSON strict sans texte d'enrobage pour les commandes rapides.
   - **Relecture & Correction (`/corrige`)** : Remédiation orthographique, grammaticale et syntaxique de phrases erronées en préservant le ton et sans aucun bavardage parasite.
   - **Synthèse de Réunion** : Extraction de résumés exécutifs et de cases à cocher `- [ ] @Nom: action`.
   - **Concision & Limite KV** : Respect de limites de mots sous contexte chargé (simulation du plafond 4096 tokens).
5. **Métriques Physiques, Temporelles & Gestion du Cache KV** :
   - **Durée Totale & Latence** : Temps d'exécution précis pour chaque test individuel et durée totale cumulée de la suite.
   - **Tokens Générés & Détection du Bloat de Réflexion (`<think>`)** : Décompte précis des tokens générés totaux et isolement des tokens de monologue intérieur (`thinking_tokens` des modèles de type DeepSeek-R1 ou QwQ). Les modèles générant des centaines de tokens de réflexion pour des requêtes simples sont immédiatement pénalisés dans le verdict car ils saturent le cache KV ($n_{\text{ctx}} \le 4096$) et détruisent la réactivité de la palette flottante (< 50 ms), même avec 100% de score fonctionnel.
   - **TTFT (Time-To-First-Token)** : Latence du premier token émis via flux SSE / in-process.
   - **TPS (Tokens/sec)** : Débit de génération effectif.
   - **Scores par catégorie** et **Jeanne Suitability Score** global avec verdict matériel explicite.
6. **Rapports Prêts pour GitHub** :
   - Génération simultanée d'un rapport en **Markdown** (tableaux synthétiques, badges et sections repliables) et en **JSON** (données brutes pour archivage ou CI).

---

## 🚀 Démarrage Rapide

### Prérequis
- Python 3.10 ou supérieur.
- Pour exécuter des fichiers GGUF sans aucun serveur externe (mode embarqué) :
  ```bash
  # Option 1 - NVIDIA GPU (CUDA précompilé sans compilation - ultra-rapide) :
  pip install llama-cpp-python --extra-index-url https://abetlen.github.io/llama-cpp-python/whl/cu124
  # (ou cu121 / cu122 selon votre version CUDA installée)

  # Option 2 - Vulkan (recommandé pour Jeanne sur iGPU AMD Radeon / Intel Iris Xe ou NVIDIA Vulkan) :
  CMAKE_ARGS="-DGGML_VULKAN=on" pip install llama-cpp-python

  # Option 3 - CPU standard :
  pip install llama-cpp-python
  ```
- Si vous préférez tester via un serveur externe ou une API : Ollama, `llama-server` ou clé API distante.

### 1. Préparer vos Environnements Isolés (`setup`)
Pour tester plusieurs backends matériels sur la même machine (par exemple comparer **CUDA vs Vulkan vs CPU** sur NVIDIA, ou **Vulkan vs CPU** sur iGPU) sans conflit de dépendances, l'outil gère des virtualenvs (`venv`) dédiés :

```bash
# Voir les environnements disponibles pour votre matériel :
python3 tools/llm-benchmark/benchmark.py setup --list

# Préparer un environnement spécifique (crée le venv et installe llama-cpp-python avec les bons flags) :
python3 tools/llm-benchmark/benchmark.py setup --backend cuda     # NVIDIA CUDA
python3 tools/llm-benchmark/benchmark.py setup --backend vulkan   # Vulkan (AMD/Intel/NVIDIA)
python3 tools/llm-benchmark/benchmark.py setup --backend cpu      # CPU standard

# Tout préparer en une seule commande :
python3 tools/llm-benchmark/benchmark.py setup --backend all

# Mode interactif (menu de sélection) :
python3 tools/llm-benchmark/benchmark.py setup
```

### 2. Initialisation Automatique de la Configuration (`init`)
Sonde votre matériel et prépare la configuration de test à partir de vos modèles :

```bash
# Initialisation simple (détection du hardware + modèles recommandés ou Ollama local) :
python3 tools/llm-benchmark/benchmark.py init

# Initialisation en scannant un dossier local contenant vos fichiers .gguf :
python3 tools/llm-benchmark/benchmark.py init --models-dir /chemin/vers/mes/modeles/
```

### 3. Exécuter le Benchmark avec l'Environnement de votre Choix
Vous pouvez exécuter le benchmark directement dans un environnement isolé sans même avoir à l'activer manuellement :

```bash
# Exécution sous l'environnement CUDA :
python3 tools/llm-benchmark/benchmark.py run --venv cuda

# Exécution sous l'environnement Vulkan (conditions Jeanne) :
python3 tools/llm-benchmark/benchmark.py run --venv vulkan

# Exécution sous l'environnement CPU de référence :
python3 tools/llm-benchmark/benchmark.py run --venv cpu

# Tester en mode simulation (sans modèle ni GPU) :
python3 tools/llm-benchmark/benchmark.py run --mock
```

---

## ⚙️ Structure de Configuration (`config.json`)

Le fichier `config.json` produit automatiquement ressemble à ceci :

```json
{
  "hardware_profile": "AMD Ryzen 7 7840HS (16 threads) | 16.0 Go RAM | GPU: AMD Radeon 780M (Vulkan)",
  "inference_profiles": {
    "deterministic_strict": {
      "temperature": 0.0,
      "seed": 42,
      "top_p": 1.0,
      "max_tokens": 1024,
      "frequency_penalty": 0.0,
      "presence_penalty": 0.0
    },
    "balanced_temp03": {
      "temperature": 0.3,
      "seed": 42,
      "top_p": 0.9,
      "max_tokens": 1024,
      "frequency_penalty": 0.0,
      "presence_penalty": 0.0
    },
    "creative_temp07": {
      "temperature": 0.7,
      "seed": 123,
      "top_p": 0.95,
      "max_tokens": 1024,
      "frequency_penalty": 0.1,
      "presence_penalty": 0.1
    }
  },
  "models": [
    {
      "id": "qwen2.5:3b-instruct-q4_k_m",
      "display_name": "Qwen 2.5 3B (Q4_K_M)",
      "endpoint": "http://localhost:11434/v1",
      "profiles": ["deterministic_strict", "balanced_temp03"]
    }
  ]
}
```


---

## 📖 Commandes et Options CLI

Lancer le benchmark complet :
```bash
python3 tools/llm-benchmark/benchmark.py
```

Filtrer sur un ou plusieurs modèles spécifiques :
```bash
python3 tools/llm-benchmark/benchmark.py --models qwen2.5:3b-instruct-q4_k_m
```

Filtrer sur un profil d'inférence précis :
```bash
python3 tools/llm-benchmark/benchmark.py --profiles deterministic_strict
```

Filtrer sur des suites de tests particulières :
```bash
python3 tools/llm-benchmark/benchmark.py --suites rag,pii
```

Activer l'affichage verbeux des assertions en direct :
```bash
python3 tools/llm-benchmark/benchmark.py --verbose
```

---

## 📤 Publication des Résultats sur GitHub

Les rapports sont automatiquement enregistrés sous `tools/llm-benchmark/reports/` avec un nom horodaté :
- `benchmark_report_YYYY-MM-DD_HHMMSS.md`
- `benchmark_report_YYYY-MM-DD_HHMMSS.json`

Pour partager vos résultats avec la communauté ou les intégrer au dépôt Jeanne :
1. Renommez ou déplacez le fichier `.md` (par exemple vers `docs/benchmarks/benchmark_mon_pc.md`).
2. Faites un commit :
   ```bash
   git add tools/llm-benchmark/reports/
   git commit -m "docs(benchmark): add LLM evaluation report for Ryzen 7840HS"
   git push origin main
   ```

Le rapport Markdown est directement lisible sur l'interface GitHub avec ses tableaux formatés, ses statuts et ses détails repliables.
