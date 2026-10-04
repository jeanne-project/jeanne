"""Scans local directories for model files (.gguf, .bin) and probes local inference daemons."""

import json
import os
import re
import urllib.request
from typing import Any, Dict, List, Optional, Tuple


SUPPORTED_EXTENSIONS = {".gguf", ".bin", ".safetensors"}


def format_file_size(size_bytes: int) -> str:
    """Formats bytes into human readable MB or GB string."""
    gb = size_bytes / (1024**3)
    if gb >= 1.0:
        return f"{gb:.2f} Go"
    mb = size_bytes / (1024**2)
    return f"{mb:.0f} Mo"


def should_ignore_model_file(fname: str, size_bytes: int) -> Tuple[bool, Optional[str]]:
    """
    Détermine si un fichier modèle doit être ignoré lors de l'initialisation du benchmark :
    - Fichiers vides (taille 0 ou < 1 Ko, stubs ou téléchargements en cours)
    - Projecteurs multimodaux pour la vision (mmproj)
    - Modules de prédiction multi-tokens auxiliaires (mtp)
    """
    fname_lower = fname.lower()

    # 1. Fichiers vides ou en cours de téléchargement
    if size_bytes == 0:
        return True, "Fichier vide (0 octet, téléchargement en cours ou annulé)"

    # Extensions temporaires fréquentes lors d'un téléchargement
    incomplete_exts = (".part", ".crdownload", ".tmp", ".aria2", ".download", ".incomplete")
    if any(fname_lower.endswith(ext) for ext in incomplete_exts) or ".part." in fname_lower:
        return True, "Téléchargement en cours (fichier temporaire)"

    # Fichiers anormalement petits pour un modèle de langage (< 1 Ko, stub Git LFS ou corrompu)
    if size_bytes < 1024:
        return True, "Fichier anormalement petit (< 1 Ko, stub ou pointeur LFS)"

    # 2. Projecteurs multimodaux pour la vision (mmproj)
    # Servent uniquement d'encodeurs visuels pour LLaVA / vision models, non exécutables comme LLM texte
    if "mmproj" in fname_lower:
        return True, "Projecteur multimodal vision (mmproj, non autonome)"

    # 3. Modules Multi-Token Prediction (MTP)
    # Modules auxiliaires de spéculation (DeepSeek-V3, etc.), non exécutables de manière autonome
    if "mtp" in fname_lower:
        return True, "Module Multi-Token Prediction auxiliaire (MTP, non autonome)"

    return False, None


def scan_models_directory(
    models_dir: str,
    default_endpoint: Optional[str] = None,
    default_profiles: Optional[List[str]] = None,
    use_embedded_engine: bool = True,
    log_ignored: bool = True,
) -> List[Dict[str, Any]]:
    """
    Recursively scans the given directory for GGUF/model files.
    Filters out empty files, vision mmproj files, and MTP auxiliary adapters.
    Returns a list of model configuration dictionaries.
    """
    profiles = default_profiles or ["deterministic_strict", "balanced_temp03"]
    discovered_models = []

    if not os.path.exists(models_dir):
        print(f"⚠️  Dossier de modèles introuvable : {models_dir}")
        return []

    for root, _, files in os.walk(models_dir):
        for fname in sorted(files):
            _, ext = os.path.splitext(fname)
            if ext.lower() in SUPPORTED_EXTENSIONS:
                full_path = os.path.join(root, fname)
                try:
                    size_bytes = os.path.getsize(full_path)
                except Exception:
                    size_bytes = 0

                # Filtrage des fichiers non éligibles au benchmark
                ignore, reason = should_ignore_model_file(fname, size_bytes)
                if ignore:
                    if log_ignored:
                        print(f"   ⏭️  Ignoré : \033[2m{fname}\033[0m ({reason})")
                    continue

                size_str = format_file_size(size_bytes)
                model_id = os.path.splitext(fname)[0]
                display_name = f"{model_id} ({size_str})"
                engine_type = "embedded" if use_embedded_engine else "http"

                discovered_models.append(
                    {
                        "id": model_id,
                        "display_name": display_name,
                        "engine": engine_type,
                        "endpoint": default_endpoint if not use_embedded_engine else None,
                        "file_path": full_path,
                        "size": size_str,
                        "timeout_secs": 60,
                        "profiles": list(profiles),
                    }
                )

    return discovered_models


def probe_ollama_models(
    endpoint: str = "http://localhost:11434",
    default_profiles: Optional[List[str]] = None,
) -> List[Dict[str, Any]]:
    """
    Attempts to probe an active local Ollama daemon for installed models.
    """
    profiles = default_profiles or ["deterministic_strict", "balanced_temp03"]
    url = f"{endpoint.rstrip('/')}/api/tags"
    discovered = []

    try:
        req = urllib.request.Request(url, headers={"Accept": "application/json"})
        with urllib.request.urlopen(req, timeout=1.5) as resp:
            data = json.loads(resp.read().decode("utf-8"))
            models_list = data.get("models", [])
            for m in models_list:
                name = m.get("name", "")
                if name:
                    size = m.get("size", 0)
                    ignore, _ = should_ignore_model_file(name, size)
                    if ignore:
                        continue
                    size_str = format_file_size(size) if size else "Inconnue"
                    discovered.append(
                        {
                            "id": name,
                            "display_name": f"{name} ({size_str})",
                            "endpoint": f"{endpoint.rstrip('/')}/v1",
                            "size": size_str,
                            "timeout_secs": 60,
                            "profiles": list(profiles),
                        }
                    )
    except Exception:
        # Ollama is not running or unreachable, ignore quietly
        pass

    return discovered


def get_default_recommended_models(
    default_endpoint: str = "http://localhost:11434/v1",
    default_profiles: Optional[List[str]] = None,
) -> List[Dict[str, Any]]:
    """Fallback models list recommended for the Jeanne project."""
    profiles = default_profiles or ["deterministic_strict", "balanced_temp03"]
    return [
        {
            "id": "qwen2.5:3b-instruct-q4_k_m",
            "display_name": "Qwen 2.5 3B Instruct (Q4_K_M)",
            "endpoint": default_endpoint,
            "timeout_secs": 60,
            "profiles": list(profiles),
        },
        {
            "id": "llama3.2:3b-instruct-q4_k_m",
            "display_name": "Llama 3.2 3B Instruct (Q4_K_M)",
            "endpoint": default_endpoint,
            "timeout_secs": 60,
            "profiles": ["deterministic_strict"],
        },
        {
            "id": "gpt-4o-mini",
            "display_name": "OpenAI GPT-4o-mini (Cloud / Comparatif)",
            "endpoint": "https://api.openai.com/v1",
            "api_key_env": "OPENAI_API_KEY",
            "timeout_secs": 30,
            "profiles": ["deterministic_strict"],
        },
    ]
