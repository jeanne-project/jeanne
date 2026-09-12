---
name: "jeanne-sqlite-hybrid"
description: "Pragmas SQLite, sérialisation vectorielle et formule Time-Decay."
---

# Jeanne SQLite Hybrid Engine Patterns

## 1. Configuration Initiale
À l'ouverture de `database.db` via `rusqlite` :
```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
```

## 2. Vecteurs & Pertinence Temporelle
* Sérialisation Little-Endian (vecteurs 384 flottants `f32`) pour `vec_chunks`.
* Filtrage d'obsolescence : `WHERE statut = 'actif'`.
* Formule : $\text{Score} = (0.7 \cdot S_{\text{vector}} + 0.3 \cdot S_{\text{BM25}}) \times \frac{1}{1 + \lambda \times \Delta t_{\text{jours}}}$.
