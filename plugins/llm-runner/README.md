# Plugin : Local LLM Inference Runner (`llm-runner`)

## 1. Contexte & Rôle Métier
Ce plugin assure l'inférence neuronale locale de **modèles de langage 2B/3B quantifiés** au format GGUF (`Qwen3.5-2B-Q4_K_M.gguf`, `Llama-3.2-3B`, etc.). Il permet à l'application Jeanne d'exécuter des requêtes de synthèse, réécriture, correction orthographique, traduction et extraction d'informations de manière 100% souveraine et hors-ligne, sans dépendre d'une connexion Internet.

Conformément à l'invariant architectural de Jeanne, ce plugin tourne dans un **sous-processus dédié** afin de protéger le processus central contre tout plantage mémoire ou instabilité du pilote graphique.

---

## 2. Choix d'Implémentation & Performance : `llama-server` vs Runner Natif
* **Performances** : Identiques entre un wrapper `llama-server` et une bibliothèque Rust FFI, car les calculs de tenseurs sont exécutés par le même moteur C++/Vulkan sous-jacent.
* **Stratégie Recommandée** :
  - Le plugin peut encapsuler le binaire officiel hautement optimisé `llama-server` (ou utiliser les bindings C `llama.cpp` compilés de façon autonome).
  - Il expose une interface standardisée **JSON-RPC 2.0 sur `stdio`** pour Jeanne, masquant la complexité des arguments CLI et du réseau.
  - Il gère le continuous batching, le streaming de jetons et le déchargement immédiat de la RAM.

---

## 3. Contrat d'Interface JSON-RPC 2.0

### 3.1 `load_model` — Chargement d'un modèle GGUF en mémoire vive

#### Requête reçue sur `stdin` :
```json
{
  "jsonrpc": "2.0",
  "method": "load_model",
  "params": {
    "model_path": "/chemin/vers/Qwen3.5-2B-Q4_K_M.gguf",
    "use_gpu": true,
    "gpu_layers": 99,
    "context_size": 4096,
    "threads": 4
  },
  "id": 1
}
```

#### Réponse émise sur `stdout` :
```json
{
  "jsonrpc": "2.0",
  "result": {
    "status": "loaded",
    "model_name": "Qwen3.5-2B-Q4_K_M.gguf",
    "architecture": "qwen2",
    "context_size": 4096,
    "vram_allocated_mb": 1850,
    "ram_allocated_mb": 420,
    "backend": "vulkan"
  },
  "id": 1
}
```

---

### 3.2 `generate_stream` — Inférence et émission de tokens en flux continu

#### Requête reçue sur `stdin` :
```json
{
  "jsonrpc": "2.0",
  "method": "generate_stream",
  "params": {
    "prompt": "Corrige ce texte : salut, commen sa va ?",
    "max_tokens": 512,
    "temperature": 0.3,
    "top_p": 0.8,
    "top_k": 20,
    "stop_tokens": ["<|im_end|>", "\n\nHuman:"]
  },
  "id": 2
}
```

#### Événements intermédiaires émis sur `stdout` (lignes NDJSON) :
```json
{"jsonrpc": "2.0", "method": "token_chunk", "params": {"request_id": 2, "delta": "Salut", "done": false}}
{"jsonrpc": "2.0", "method": "token_chunk", "params": {"request_id": 2, "delta": ",", "done": false}}
{"jsonrpc": "2.0", "method": "token_chunk", "params": {"request_id": 2, "delta": " comment", "done": false}}
{"jsonrpc": "2.0", "method": "token_chunk", "params": {"request_id": 2, "delta": " ça", "done": false}}
{"jsonrpc": "2.0", "method": "token_chunk", "params": {"request_id": 2, "delta": " va", "done": false}}
{"jsonrpc": "2.0", "method": "token_chunk", "params": {"request_id": 2, "delta": " ?", "done": false}}
```

#### Réponse finale émise sur `stdout` :
```json
{
  "jsonrpc": "2.0",
  "result": {
    "request_id": 2,
    "done": true,
    "generated_tokens": 7,
    "prompt_tokens": 12,
    "tokens_per_second": 28.4,
    "finish_reason": "stop"
  },
  "id": 2
}
```

---

### 3.3 `unload_model` — Déchargement immédiat de la RAM/VRAM

#### Requête reçue sur `stdin` :
```json
{
  "jsonrpc": "2.0",
  "method": "unload_model",
  "params": {},
  "id": 3
}
```

#### Réponse émise sur `stdout` :
```json
{
  "jsonrpc": "2.0",
  "result": {
    "status": "unloaded",
    "freed_mb": 2270
  },
  "id": 3
}
```

---

## 4. Contraintes & Exigences Techniques
1. **Plafond RAM/VRAM** : L'empreinte résiduelle totale du sous-processus ne doit jamais dépasser **4,5 Go**. Lors de l'appel à `unload_model`, l'empreinte doit redescendre sous les **50 Mo** en moins de 2 secondes.
2. **Support de l'interchangeabilité de modèle** : L'utilisateur peut spécifier n'importe quel fichier GGUF présent sur sa machine ; le plugin valide l'en-tête binaire GGUF et applique la configuration de contexte appropriée.
3. **Arrêt propre sur SIGTERM / fermeture de `stdin`** : Si le tube `stdin` se ferme, le sous-processus doit immédiatement libérer les ressources allouées et quitter proprement (zéro processus fantôme).
