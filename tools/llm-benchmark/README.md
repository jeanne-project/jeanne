# 🧠 Jeanne LLM Benchmark Tool

Outil indépendant et déterministe d'évaluation des modèles de langage (LLMs) pour l'écosystème **Jeanne**.

Cet outil mesure rigoureusement l'adéquation fonctionnelle et matérielle des modèles de langage (locaux ou distants) face aux exigences réelles de Jeanne : latence premier token (TTFT), débit de génération (TPS), durée d'exécution totale, consommation de tokens, détection du bloat de réflexion (`<think>`), respect des schémas JSON, citations RAG et préservation PII.

> 🛡️ **Isolation Garantie** : Cet outil ne dépend d'aucun code interne de Jeanne et n'est pas embarqué dans son build de production (`Cargo.lock` / Tauri). Il n'altère aucun fichier du coffre utilisateur ni aucun index SQLite.

---

## 🎯 Fonctionnalités Clés

1. **Reproductibilité & Déterminisme Absolu** :
   - Évaluation programmatique stricte sans « LLM-as-a-judge » : expressions régulières, parseurs JSON stricts, analyseurs d'exactitude de citations et vérification de non-hallucination.
   - Tous les hyperparamètres d'inférence (`temperature`, `seed`, `top_p`, `max_tokens`, etc.) sont enregistrés dans la configuration et restitués de manière transparente dans les rapports.
2. **Profils par Défaut GGUF (`default_gguf` & valeurs natives)** :
   - Prise en charge des valeurs `null` pour hériter fidèlement des hyperparamètres définis nativement dans le fichier GGUF ou par le serveur d'inférence (température, top_p, seed).
   - Indication explicite de la valeur retenue avec la mention `(par défaut)` dans les tableaux de reproductibilité Markdown et dans les données JSON (`resolved_params`).
3. **Générateur de Profils d'Inférence CLI (`--profil`)** :
   - Définition succincte et cumulable de profils à la volée lors de l'initialisation : `default`, `deterministic`, `balanced`, `creative`.
   - Tirage aléatoire contrôlé dans les plages recommandées et possibilité de surcharges explicites (ex: `,temperature=0.4,seed=42`).
   - Fixation des graines et paramètres directement dans le `config.json` pour garantir une reproductibilité parfaite.
4. **Attribution Exhaustive des Profils aux Modèles** :
   - Tous les profils configurés sont automatiquement assignés à l'ensemble des modèles éligibles (GGUF locaux, Ollama ou modèles recommandés).
5. **Filtrage Intelligent des Fichiers Modèles** :
   - Exclusion automatique des téléchargements incomplets ou fichiers vides (`0 octet`, `.part`), des encodeurs vision multimodaux (`mmproj`), et des modules de spéculation multi-tokens (`mtp`).
6. **Moteur Embarqué Direct (In-Process GGUF via `llama.cpp`)** :
   - Exécution directe en mémoire des fichiers `.gguf` sans aucun intermédiaire réseau ni démon HTTP actif.
   - Conforme au runtime Jeanne : plafond KV strict à 4096 tokens, accélération matérielle Vulkan/CUDA/Metal et déchargement immédiat de la RAM/VRAM à l'issue des tests.
   - Supporte également le mode HTTP compatible OpenAI `/v1` (Ollama, `llama-server`, LM Studio, OpenAI, etc.).
7. **Gestion Multi-Environnements & Virtualenvs (`venv`)** :
   - Préparation assistée et bascule automatique vers des environnements virtuels isolés pour tester plusieurs backends sans conflits de DLL (CUDA, Vulkan, CPU).
8. **Métriques Physiques, Temporelles & Alerte Surcharge de Réflexion** :
   - **Durée Totale & Latence** : Chronométrage précis par test et durée cumulée par configuration.
   - **Volume de Tokens & Détection `<think>`** : Détection des tokens de réflexion (modèles DeepSeek-R1, QwQ non calibrés). Les modèles générant une réflexion excessive sont alertés car ils saturent le cache KV ($n_{\text{ctx}} \le 4096$) et détruisent la réactivité de la palette (< 50 ms).
   - **TTFT & Débit TPS** : Latence du premier token émis et débit réel en tokens/seconde.
   - **Suite `/corrige`** : Évaluation dédiée à la remédiation orthographique et grammaticale sans bavardage parasite.

---

## 🚀 Démarrage Rapide

### Prérequis
- Python 3.10 ou supérieur.
- Pour exécuter des modèles GGUF directement sans serveur externe :
  ```bash
  # NVIDIA CUDA (roue précompilée, ultra-rapide) :
  pip install llama-cpp-python --extra-index-url https://abetlen.github.io/llama-cpp-python/whl/cu124

  # Vulkan (AMD Radeon, Intel Iris Xe, NVIDIA Vulkan) :
  CMAKE_ARGS="-DGGML_VULKAN=on" pip install llama-cpp-python

  # CPU standard :
  pip install llama-cpp-python
  ```

---

## 🛠️ Guide d'Utilisation & Exemples

### 1. Préparer vos Environnements Isolés (`setup`)

Pour comparer plusieurs accélérations matérielles sur la même machine sans conflit de dépendances :

```bash
# Lister les environnements recommandés pour votre matériel hôte :
python3 tools/llm-benchmark/benchmark.py setup --list

# Installer l'environnement NVIDIA CUDA :
python3 tools/llm-benchmark/benchmark.py setup --backend cuda

# Installer l'environnement Vulkan (conditions Jeanne) :
python3 tools/llm-benchmark/benchmark.py setup --backend vulkan

# Installer la référence CPU :
python3 tools/llm-benchmark/benchmark.py setup --backend cpu

# Tout préparer en une seule commande :
python3 tools/llm-benchmark/benchmark.py setup --backend all

# Mode interactif pas-à-pas :
python3 tools/llm-benchmark/benchmark.py setup
```

---

### 2. Initialiser la Configuration (`init`)

La commande `init` sonde votre matériel, recherche vos modèles et génère un fichier `config.json`.

#### Exemple A : Initialisation standard (profils par défaut)
Configure les 4 profils de référence (`default_gguf`, `deterministic_strict`, `balanced_temp03`, `creative_temp07`) :
```bash
python3 tools/llm-benchmark/benchmark.py init
```

#### Exemple B : Scanner un dossier de modèles locaux GGUF
Scanne récursivement vos fichiers `.gguf` en ignorant automatiquement les fichiers temporaires, `mmproj` et `mtp` :
```bash
python3 tools/llm-benchmark/benchmark.py init --models-dir /chemin/vers/mes/modeles/ -f
```

#### Exemple C : Générer des profils personnalisés avec `--profil`
Vous pouvez passer l'argument `--profil` (ou `--profile`) plusieurs fois pour créer des variantes d'inférence spécifiques :
```bash
python3 tools/llm-benchmark/benchmark.py init \
  --models-dir /chemin/vers/mes/modeles/ \
  --profil default \
  --profil deterministic \
  --profil deterministic,seed=465 \
  --profil balanced,temperature=0.4,seed=42 \
  --profil creative,presence_penalty=0.2,name=creative_custom \
  --force
```

#### 📋 Syntaxe des types de profils `--profil` :

| Type | Comportement par défaut | Paramètres générés / Plages |
| :--- | :--- | :--- |
| `default` | Valeurs natives du GGUF / serveur | `temperature=null`, `seed=null`, `top_p=null` |
| `deterministic` | Mode déterministe strict | `temperature=0.0`, `top_p=1.0`, `seed` aléatoire ou fixé |
| `balanced` | Équilibré pour tâches courantes | `temp` $\in [0.2, 0.4]$, `top_p` $\in [0.85, 0.95]$, `seed` aléatoire |
| `creative` | Créatif / brainstorming | `temp` $\in [0.65, 0.85]$, `top_p` $\in [0.90, 0.98]$, pénalités $\in [0.05, 0.15]$ |

> 💡 **Surcharges Précises** : Séparez les surcharges par des virgules : `,clé=valeur` (ex: `temperature=0.35`, `seed=1234`, `top_p=0.9`, `max_tokens=2048`, `name=mon_profil`). Les graines aléatoires générées sont gravées dans `config.json` pour garantir que les tests ultérieurs restent 100% reproductibles.

---

### 3. Exécuter le Benchmark (`run`)

Le benchmark s'exécute soit via la sous-commande `run`, soit directement à la racine du script.

#### Exemple A : Exécution directe (détection automatique du meilleur venv)
Détecte automatiquement votre GPU et bascule en toute transparence sur l'environnement adapté (ex. CUDA ou Vulkan) s'il existe :
```bash
python3 tools/llm-benchmark/benchmark.py run
```

#### Exemple B : Exécution ciblée sous un venv spécifique
```bash
# Exécution sous CUDA :
python3 tools/llm-benchmark/benchmark.py run --venv cuda

# Exécution sous Vulkan :
python3 tools/llm-benchmark/benchmark.py run --venv vulkan

# Exécution sous CPU :
python3 tools/llm-benchmark/benchmark.py run --venv cpu
```

#### Exemple C : Filtrer par modèle et par profil
```bash
# Ne tester qu'un modèle spécifique sous un profil précis :
python3 tools/llm-benchmark/benchmark.py run \
  --models qwen2.5:3b-instruct-q4_k_m \
  --profiles default_gguf,deterministic_strict
```

#### Exemple D : Filtrer par suites de tests
```bash
# Exécuter uniquement le RAG et la correction orthographique /corrige :
python3 tools/llm-benchmark/benchmark.py run --suites rag,corrige
```
*Suites disponibles* : `rag`, `pii`, `structured`, `meeting`, `conciseness`, `corrige`.

#### Exemple E : Mode simulation sèche (`--mock`)
Permet de vérifier toute la matrice de tests et de générer un rapport complet sans nécessiter de GPU ni de modèle téléchargé :
```bash
python3 tools/llm-benchmark/benchmark.py run --mock
```

#### Exemple F : Mode verbeux avec assertions en direct (`-v`)
Affiche le détail de chaque assertion unitaire validée en console :
```bash
python3 tools/llm-benchmark/benchmark.py run --verbose
```

---

## ⚙️ Structure du Fichier `config.json`

Le fichier de configuration généré par `init` :

```json
{
  "hardware_profile": "AMD Ryzen 7 7840HS (16 threads) | 16.0 Go RAM | GPU: AMD Radeon 780M (Vulkan)",
  "inference_profiles": {
    "default_gguf": {
      "temperature": null,
      "seed": null,
      "top_p": null,
      "max_tokens": 1024,
      "frequency_penalty": 0.0,
      "presence_penalty": 0.0
    },
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
      "id": "qwen2.5-3b-instruct-q4_k_m",
      "display_name": "qwen2.5-3b-instruct-q4_k_m (2.1 Go)",
      "engine": "embedded",
      "endpoint": null,
      "file_path": "/chemin/vers/qwen2.5-3b-instruct-q4_k_m.gguf",
      "size": "2.1 Go",
      "timeout_secs": 60,
      "profiles": [
        "default_gguf",
        "deterministic_strict",
        "balanced_temp03",
        "creative_temp07"
      ]
    }
  ]
}
```

---

## 📊 Rapports & Publication

Les rapports sont automatiquement générés dans `tools/llm-benchmark/reports/` avec horodatage :
- `benchmark_report_YYYY-MM-DD_HHMMSS.md` : Rapport complet GitHub-Flavored Markdown.
- `benchmark_report_YYYY-MM-DD_HHMMSS.json` : Données brutes machine-readable pour archivage et benchmarks comparatifs.

### Extrait du Tableau de Reproductibilité Produit :

| Profil | Temperature | Seed | Top-P | Max Tokens | Penalties (Freq / Pres) |
| :--- | :---: | :---: | :---: | :---: | :---: |
| **`default_gguf`** | `0.8 (par défaut)` | `non fixé (par défaut)` | `0.95 (par défaut)` | `1024` | `0.0 / 0.0` |
| **`deterministic_strict`** | `0.0` | `42` | `1.0` | `1024` | `0.0 / 0.0` |
| **`balanced_temp03`** | `0.3` | `42` | `0.9` | `1024` | `0.0 / 0.0` |

### Critères de Verdict Jeanne :
- 🟢 **Excellent (Idéal pour Jeanne)** : Score $\ge 90\%$, débit $\ge 20$ tps, TTFT $\le 350$ ms, aucun bloat de réflexion.
- 🟡 **Acceptable (Performances viables)** : Score $\ge 80\%$, débit $\ge 15$ tps, TTFT $\le 600$ ms.
- ⚠️ **Non viable (Surcharge de réflexion)** : $\ge 250$ tokens de monologue intérieur ou $>25\%$ du volume total, saturant le contexte KV 4096.
- 🔴 **Non Recommandé** : Échec fonctionnel ou modèle trop lent.

---

## 📖 Récapitulatif des Options CLI

| Commande | Option | Description |
| :--- | :--- | :--- |
| `setup` | `-b, --backend <id>` | Backend à installer (`cuda`, `vulkan`, `cpu`, `metal`, `all`) |
| `setup` | `-l, --list` | Liste les profils d'environnement compatibles avec l'hôte |
| `setup` | `--dry-run` | Affiche les commandes pip/venv sans les exécuter |
| `init` | `-o, --output <path>` | Chemin du fichier de configuration (défaut: `config.json`) |
| `init` | `-f, --force` | Écrase le fichier de configuration existant |
| `init` | `--models-dir <path>` | Dossier local à scanner pour découvrir les fichiers `.gguf` |
| `init` | `--profil <spec>` | Spécifie un profil d'inférence à générer (cumulable) |
| `init` | `--no-probe` | Désactive la détection du serveur local Ollama |
| `run` | `-c, --config <path>` | Chemin du fichier de configuration JSON |
| `run` | `--venv <id>` | Exécute sous un environnement virtuel spécifique (`cuda`, `vulkan`, `cpu`) |
| `run` | `--no-auto-venv` | Désactive la bascule automatique sur un virtualenv détecté |
| `run` | `-m, --models <ids>` | Liste d'identifiants de modèles à tester (séparés par des virgules) |
| `run` | `-p, --profiles <ids>` | Liste de profils à exécuter (séparés par des virgules) |
| `run` | `-s, --suites <names>` | Suites à lancer (`rag`, `pii`, `structured`, `meeting`, `conciseness`, `corrige`) |
| `run` | `--mock` | Exécution en simulation déterministe sans GPU ni modèle |
| `run` | `-v, --verbose` | Affichage détaillé en console de chaque assertion |
| `run` | `--output-dir <path>` | Répertoire de destination des rapports (défaut: `reports/`) |
