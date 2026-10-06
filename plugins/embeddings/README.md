# Plugin : Text Embeddings Generator (`embeddings`)

## 1. Contexte & Rôle Métier
Ce plugin génère des **vecteurs d'embedding denses** (représentations sémantiques continues) à partir de fragments textuels de notes Markdown ou de requêtes utilisateurs.

Il alimente directement la table virtuelle `vec_chunks USING vec0(embedding FLOAT[384])` gérée par l'extension `sqlite-vec` dans le stockage SQLite de Jeanne. C'est la brique indispensable qui active la recherche hybride dense + BM25 avec filtre temporel *Time-Decay* (`RagEngine`).

---

## 2. Modèle par Défaut & Interchangeabilité

* **Modèle par Défaut** : `all-MiniLM-L6-v2` (dimension de sortie : **384**, taille sur disque : ~**80 Mo**).
* **Interchangeabilité Requise** :
  - L'utilisateur peut sélectionner un autre modèle via `config.json` ou par la commande JSON-RPC `switch_model`.
  - Modèles alternatifs supportés : `bge-small-en-v1.5`, `multilingual-e5-small`.
  - La dimension par défaut est alignée sur **384** pour correspondre au schéma `FLOAT[384]` de `vec_chunks`.

---

## 3. Contrat d'Interface JSON-RPC 2.0

### 3.1 `get_model_info` — Consultation des caractéristiques du modèle actif

#### Requête reçue sur `stdin` :
```json
{
  "jsonrpc": "2.0",
  "method": "get_model_info",
  "params": {},
  "id": 1
}
```

#### Réponse émise sur `stdout` :
```json
{
  "jsonrpc": "2.0",
  "result": {
    "model_id": "all-MiniLM-L6-v2",
    "dimension": 384,
    "max_seq_length": 512,
    "normalized": true
  },
  "id": 1
}
```

---

### 3.2 `embed_text` — Vectorisation d'un texte unique (requête de recherche ou extrait)

#### Requête reçue sur `stdin` :
```json
{
  "jsonrpc": "2.0",
  "method": "embed_text",
  "params": {
    "text": "De quelle couleur est ma voiture ?",
    "prompt_type": "query"
  },
  "id": 2
}
```

#### Réponse émise sur `stdout` :
```json
{
  "jsonrpc": "2.0",
  "result": {
    "embedding": [0.0241, -0.0512, 0.0891, "...(384 composantes f32 normalisées L2)"],
    "token_count": 8,
    "elapsed_ms": 12
  },
  "id": 2
}
```

---

### 3.3 `embed_batch` — Vectorisation d'un lot de fragments lors de la synchronisation du coffre

#### Requête reçue sur `stdin` :
```json
{
  "jsonrpc": "2.0",
  "method": "embed_batch",
  "params": {
    "texts": [
      "Ma voiture est bleue. Je l'ai achetée en 2024.",
      "Réunion projet Jeanne : validation du jalon d'inférence locale."
    ]
  },
  "id": 3
}
```

#### Réponse émise sur `stdout` :
```json
{
  "jsonrpc": "2.0",
  "result": {
    "embeddings": [
      [0.0125, -0.0341, "...(384 composantes f32)"],
      [-0.0412, 0.0911, "...(384 composantes f32)"]
    ],
    "count": 2,
    "total_tokens": 28,
    "elapsed_ms": 25
  },
  "id": 3
}
```

---

### 3.4 `switch_model` — Changement dynamique de modèle d'embedding

#### Requête reçue sur `stdin` :
```json
{
  "jsonrpc": "2.0",
  "method": "switch_model",
  "params": {
    "model_id": "multilingual-e5-small"
  },
  "id": 4
}
```

#### Réponse émise sur `stdout` :
```json
{
  "jsonrpc": "2.0",
  "result": {
    "status": "switched",
    "model_id": "multilingual-e5-small",
    "dimension": 384
  },
  "id": 4
}
```

---

## 4. Contraintes & Exigences Techniques
1. **Normalisation L2 Impérative** : Tous les vecteurs émis doivent avoir une norme euclidienne $\|v\|_2 = 1.0 \pm 10^{-5}$, car le calcul de similarité cosinus sous `sqlite-vec` (distance `cosine`) présuppose des vecteurs normalisés pour une vitesse maximale.
2. **Latence d'inférence** : Inférieure à **30 ms** pour un texte individuel et inférieure à **100 ms** pour un lot de 10 notes sur CPU standard (AVX2/NEON).
3. **Empreinte mémoire** : Strictement inférieure à **150 Mo** de RAM résidente.
