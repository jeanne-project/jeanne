# Plugin : PDF Structured Markdown Parser (`pdf-parser`)

## 1. Contexte & Rôle Métier
Ce plugin extrait le texte brut, les tableaux et les métadonnées (titre, auteur, date, nombre de pages) d'un document PDF pour générer une note au format Markdown propre et directement indexable par le coffre Jeanne.

Il s'exécute en mode `on_demand` : il démarre, analyse le fichier spécifié par son chemin d'accès absolu sur le disque, émet le résultat JSON-RPC sur `stdout` et s'arrête immédiatement.

---

## 2. Contrat d'Interface JSON-RPC 2.0

### 2.1 `parse_document` — Extraction d'un fichier PDF

#### Requête reçue sur `stdin` :
```json
{
  "jsonrpc": "2.0",
  "method": "parse_document",
  "params": {
    "file_path": "/chemin/vers/rapport.pdf",
    "options": {
      "extract_tables": true,
      "extract_images": false
    }
  },
  "id": 1
}
```

#### Réponse émise sur `stdout` :
```json
{
  "jsonrpc": "2.0",
  "result": {
    "title": "Rapport Financier Annuel 2026",
    "content_markdown": "# Rapport Financier Annuel 2026\n\n## 1. Synthèse\nLa croissance a atteint...",
    "metadata": {
      "author": "Direction Financière",
      "date": "2026-04-12",
      "page_count": 14
    },
    "attachments": []
  },
  "id": 1
}
```

---

## 3. Contraintes & Exigences Techniques
1. **Zéro Injection Binaire** : Seul le chemin absolu du fichier est passé sur `stdin` ; aucun contenu PDF binaire n'est sérialisé en JSON.
2. **Watchdog 120s** : Si le document contient des boucles complexes ou est corrompu, le sous-processus est automatiquement détruit au bout de 120 secondes par Jeanne.
3. **Tolérance aux erreurs** : Retourne des codes d'erreur JSON-RPC normalisés (`-32001` fichier introuvable, `-32002` fichier corrompu ou protégé par mot de passe).
