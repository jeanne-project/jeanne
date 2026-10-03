# 🧠 Jeanne LLM Benchmark Tool

Outil indépendant et déterministe d'évaluation des modèles de langage (LLMs) pour le projet **Jeanne**.

Cet outil permet de mesurer rigoureusement l'adéquation, la fiabilité et les performances (TTFT, débit TPS, respect des formats, citations RAG, préservation PII) de différents LLMs (locaux ou distants) en fonction de votre matériel.

> 🛡️ **Isolation Garantie** : Cet outil ne dépend d'aucun code interne de Jeanne et n'est pas embarqué dans son build (`Cargo.lock` / Tauri). Il ne modifie aucun fichier de configuration ni aucun coffre utilisateur.

---

## 🎯 Fonctionnalités Clés

1. **Reproductibilité & Déterminisme Absolu** :
   - Évaluation programmatique stricte sans « LLM-as-a-judge » : expressions régulières, validateurs JSON, analyseurs de citations et détection de mots-clés de rejet.
   - Tous les hyperparamètres d'inférence (`temperature`, `seed`, `top_p`, `max_tokens`, etc.) sont appliqués de manière contrôlée et explicitement consignés dans chaque rapport.
2. **Support Multi-Profils d'Inférence** :
   - Permet d'exécuter des séries de tests avec différents profils (ex. `deterministic_strict` à $T=0.0$, `balanced` à $T=0.3$, etc.) sur un même modèle ou plusieurs modèles.
3. **Cas d'Usage Réels de Jeanne** :
   - **RAG & Citations** : Synthèse sous contrainte avec obligation de citer `[source: nom_note.md]` et refus propre sans hallucination pour les questions hors-domaine.
   - **Préservation PII** : Rétention stricte des tokens masqués (`[PERSON_1]`, `[EMAIL_1]`, etc.) sans corruption.
   - **Sortie Structurée Palette** : Génération de JSON strict sans texte d'enrobage pour les commandes rapides.
   - **Synthèse de Réunion** : Extraction de résumés exécutifs et de cases à cocher `- [ ] @Nom: action`.
   - **Concision & Limite KV** : Respect de limites de mots sous contexte chargé (simulation du plafond 4096 tokens).
4. **Métriques Physiques & Fonctionnelles** :
   - **TTFT (Time-To-First-Token)** : Latence du premier token émis via flux SSE.
   - **TPS (Tokens/sec)** : Débit de génération effectif.
   - **Scores par catégorie** et **Jeanne Suitability Score** global.
5. **Rapports Prêts pour GitHub** :
   - Génération simultanée d'un rapport en **Markdown** (tableaux synthétiques, badges et sections repliables) et en **JSON** (données brutes pour archivage ou CI).

---

## 🚀 Démarrage Rapide

### Prérequis
- Python 3.10 ou supérieur.
- Zéro bibliothèque externe requise (fonctionne à 100% avec la bibliothèque standard Python).
- Un serveur d'inférence OpenAI-compatible actif (ex: [Ollama](https://ollama.com/), `llama-server`, [LM Studio](https://lmstudio.ai/)) OU une clé API pour un service distant.

### 1. Tester l'outil en mode simulation (Dry-Run)
Pour vérifier le bon fonctionnement de la suite et observer la génération des rapports sans serveur actif :

```bash
python3 tools/llm-benchmark/benchmark.py --mock
```

Les rapports seront générés dans le dossier `tools/llm-benchmark/reports/`.

---

### 1. Initialisation Automatique (`init`)
L'outil propose une commande `init` qui sonde automatiquement votre matériel (CPU, RAM, GPU/Vulkan) et prépare la configuration de base avec les profils d'inférence recommandés :

```bash
# Initialisation simple (détection du hardware + modèles recommandés ou Ollama local) :
python3 tools/llm-benchmark/benchmark.py init

# Initialisation en scannant un dossier local contenant vos modèles GGUF :
python3 tools/llm-benchmark/benchmark.py init --models-dir /chemin/vers/mes/modeles/

# Options disponibles pour init :
#   --models-dir, -d : Dossier à scanner récursivement (.gguf, .bin, .safetensors)
#   --endpoint, -e   : URL d'inférence par défaut (défaut: http://localhost:11434/v1)
#   --output, -o     : Fichier de sortie (défaut: config.json)
#   --force, -f      : Écraser la configuration existante
#   --no-probe       : Ne pas sonder le serveur Ollama local
```

### 2. Tester en mode simulation (Dry-Run)
Pour vérifier le bon fonctionnement de la suite et observer la génération des rapports sans serveur actif :

```bash
python3 tools/llm-benchmark/benchmark.py --mock
```

Les rapports seront générés dans le dossier `tools/llm-benchmark/reports/`.

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
