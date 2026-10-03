"""Default inference profiles and configuration generation template."""

from typing import Any, Dict, List, Optional


DEFAULT_INFERENCE_PROFILES: Dict[str, Dict[str, Any]] = {
    "deterministic_strict": {
        "temperature": 0.0,
        "seed": 42,
        "top_p": 1.0,
        "max_tokens": 1024,
        "frequency_penalty": 0.0,
        "presence_penalty": 0.0,
    },
    "balanced_temp03": {
        "temperature": 0.3,
        "seed": 42,
        "top_p": 0.9,
        "max_tokens": 1024,
        "frequency_penalty": 0.0,
        "presence_penalty": 0.0,
    },
    "creative_temp07": {
        "temperature": 0.7,
        "seed": 123,
        "top_p": 0.95,
        "max_tokens": 1024,
        "frequency_penalty": 0.1,
        "presence_penalty": 0.1,
    },
}


def build_benchmark_config(
    hardware_summary: str,
    models: List[Dict[str, Any]],
    custom_profiles: Optional[Dict[str, Dict[str, Any]]] = None,
) -> Dict[str, Any]:
    """Assembles a complete configuration payload."""
    profiles = custom_profiles or DEFAULT_INFERENCE_PROFILES
    return {
        "hardware_profile": hardware_summary,
        "inference_profiles": profiles,
        "models": models,
    }
