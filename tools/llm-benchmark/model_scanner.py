"""Scans local directories for model files (.gguf, .bin) and probes local inference daemons."""

import json
import os
import urllib.request
from typing import Any, Dict, List, Optional


SUPPORTED_EXTENSIONS = {".gguf", ".bin", ".safetensors"}


def format_file_size(size_bytes: int) -> str:
    """Formats bytes into human readable MB or GB string."""
    gb = size_bytes / (1024**3)
    if gb >= 1.0:
        return f"{gb:.2f} Go"
    mb = size_bytes / (1024**2)
    return f"{mb:.0f} Mo"


def scan_models_directory(
    models_dir: str,
    default_endpoint: str = "http://localhost:11434/v1",
    default_profiles: Optional[List[str]] = None,
) -> List[Dict[str, Any]]:
    """
    Recursively scans the given directory for GGUF/model files.
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
                    size_str = format_file_size(size_bytes)
                except Exception:
                    size_str = "Taille inconnue"

                model_id = os.path.splitext(fname)[0]
                display_name = f"{model_id} ({size_str})"

                discovered_models.append(
                    {
                        "id": model_id,
                        "display_name": display_name,
                        "endpoint": default_endpoint,
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
