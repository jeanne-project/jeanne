# Plugin : Speech-to-Text Whisper Engine (`voice-whisper`)

## 1. Contexte & Rôle Métier
Ce plugin remplace le stub actuel de `WhisperSttEngine` (qui renvoyait la phrase en dur *"Transcription vocale réussie"*) par un **véritable moteur de reconnaissance automatique de la parole (STT)**.

Il convertit un flux d'échantillons audio capturé via `cpal` et rééchantillonné en 16 000 Hz mono par `AudioResampler` en texte naturel écrit avec accents et ponctuation.

---

## 2. Modèles Supportés
* **Modèle par Défaut** : `whisper-tiny` (taille : ~75 Mo, optimisé pour inférence ultra-rapide sur CPU).
* **Modèle Alternatif** : `whisper-base` (taille : ~145 Mo, meilleure précision sur vocabulaire technique et acronymes).
* **Format Audio Exigé** : Échantillons mono flottants 32-bit normalisés entre -1.0 et 1.0 à **16 000 Hz**.

---

## 3. Contrat d'Interface JSON-RPC 2.0

### 3.1 `transcribe_pcm` — Transcription d'un tampon audio

Conformément à la règle de **zéro buffer bloat**, l'audio peut être transmis soit par un fichier temporaire sur disque (`audio_file_path`), soit par des blocs d'échantillons flottants pour des requêtes courtes (< 10 secondes) :

#### Requête reçue sur `stdin` :
```json
{
  "jsonrpc": "2.0",
  "method": "transcribe_pcm",
  "params": {
    "audio_file_path": "/tmp/jeanne_vad_utterance_01.wav",
    "language": "fr",
    "temperature": 0.0
  },
  "id": 1
}
```

#### Réponse émise sur `stdout` :
```json
{
  "jsonrpc": "2.0",
  "result": {
    "text": "De quelle couleur est ma voiture ?",
    "language": "fr",
    "confidence": 0.96,
    "duration_seconds": 2.4,
    "elapsed_ms": 380
  },
  "id": 1
}
```

---

## 4. Contraintes & Exigences Techniques
1. **Latence** : Moins de **500 ms** de temps de traitement pour un énoncé vocal de 3 secondes sur CPU standard.
2. **Empreinte Mémoire** : Inférieure à **150 Mo** de RAM pour `whisper-tiny`.
3. **Absence de bruit de quantification** : Suppression automatique des hallucinations de silence (gating sur segments vides).
