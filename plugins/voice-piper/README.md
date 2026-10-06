# Plugin : Text-to-Speech Piper Engine (`voice-piper`)

## 1. Contexte & Rôle Métier
Ce plugin remplace le stub actuel de `PiperTtsEngine` (qui générait une simple onde sinusoïdale pure à 440 Hz / un bip) par une **véritable synthèse vocale neuronale locale à voix humaine naturelle**.

Il s'intègre avec le découpeur incrémental `SentenceSplitter` de Jeanne : dès qu'une phrase complète est produite par le LLM, elle est transmise au plugin Piper pour commencer la restitution vocale sans attendre la fin du paragraphe.

---

## 2. Voix Supportées & Format
* **Voix Française par Défaut** : `fr_FR-siwis-medium` (ONNX compact ~60 Mo, voix féminine claire et posée).
* **Voix Anglaise** : `en_US-lessac-medium` (~65 Mo).
* **Format de Sortie** : Flux audio PCM mono flottant 32-bit à **16 000 Hz** ou **22 050 Hz** (automatiquement rééchantillonné vers les enceintes de l'hôte).

---

## 3. Contrat d'Interface JSON-RPC 2.0

### 3.1 `synthesize_sentence` — Synthèse vocale d'une phrase

#### Requête reçue sur `stdin` :
```json
{
  "jsonrpc": "2.0",
  "method": "synthesize_sentence",
  "params": {
    "sentence": "Bonjour, votre voiture est bien de couleur bleue selon votre journal.",
    "voice_id": "fr_FR-siwis-medium",
    "speed": 1.0
  },
  "id": 1
}
```

#### Réponse émise sur `stdout` :
```json
{
  "jsonrpc": "2.0",
  "result": {
    "audio_file_path": "/tmp/jeanne_tts_chunk_01.wav",
    "sample_rate": 16000,
    "duration_seconds": 3.8,
    "sample_count": 60800,
    "elapsed_ms": 195
  },
  "id": 1
}
```

---

## 4. Contraintes & Exigences Techniques
1. **Latence Premier Octet (TTFB)** : Inférieure à **300 ms** entre la réception de la phrase et la disponibilité du fichier audio, afin de garantir une sensation de dialogue temps réel fluide (< 800 ms total avec VAD et LLM).
2. **Empreinte Mémoire** : Inférieure à **100 Mo** de RAM par voix chargée.
3. **Zéro Artéfact** : Phonémisation correcte des abréviations et acronymes français usuels via `espeak-ng`.
