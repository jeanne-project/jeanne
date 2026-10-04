import random
from typing import Any, Dict, List, Optional, Tuple


DEFAULT_INFERENCE_PROFILES: Dict[str, Dict[str, Any]] = {
    "default_gguf": {
        "temperature": None,
        "seed": None,
        "top_p": None,
        "max_tokens": 1024,
        "frequency_penalty": 0.0,
        "presence_penalty": 0.0,
    },
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


def _parse_scalar_value(val_str: str) -> Any:
    val_clean = val_str.strip()
    if val_clean.lower() in ("null", "none"):
        return None
    if val_clean.lower() in ("true", "yes"):
        return True
    if val_clean.lower() in ("false", "no"):
        return False
    try:
        if "." in val_clean:
            return float(val_clean)
        return int(val_clean)
    except ValueError:
        return val_clean


def generate_profile_from_spec(spec: str) -> Tuple[str, Dict[str, Any]]:
    """
    Génère une configuration de profil d'inférence à partir d'une spécification concise.
    Format :
        <type>[,clé1=valeur1,clé2=valeur2,...]
    Types :
        - default : paramètres par défaut du GGUF/modèle (temperature=None, top_p=None, seed=None)
        - deterministic : strict déterministe (temperature=0.0, top_p=1.0, seed aléatoire ou fixé)
        - balanced : équilibré (temp dans [0.2, 0.4], top_p dans [0.85, 0.95], seed aléatoire)
        - creative : créatif (temp dans [0.65, 0.85], top_p dans [0.90, 0.98], pénalités, seed aléatoire)
    """
    parts = [p.strip() for p in spec.split(",") if p.strip()]
    if not parts:
        raise ValueError("Spécification de profil vide.")

    ptype = parts[0].lower()
    overrides: Dict[str, Any] = {}
    custom_name: Optional[str] = None

    for param_part in parts[1:]:
        if "=" not in param_part:
            raise ValueError(
                f"Paramètre invalide '{param_part}' dans la spécification '{spec}'. Format attendu : clé=valeur."
            )
        k, v = param_part.split("=", 1)
        k = k.strip().lower()
        parsed_v = _parse_scalar_value(v)
        if k == "name":
            custom_name = str(parsed_v)
        else:
            overrides[k] = parsed_v

    if ptype in ("default", "def", "gguf"):
        profile: Dict[str, Any] = {
            "temperature": None,
            "seed": None,
            "top_p": None,
            "max_tokens": 1024,
            "frequency_penalty": 0.0,
            "presence_penalty": 0.0,
        }
    elif ptype in ("deterministic", "det", "strict"):
        profile = {
            "temperature": 0.0,
            "seed": random.randint(1000, 99999),
            "top_p": 1.0,
            "max_tokens": 1024,
            "frequency_penalty": 0.0,
            "presence_penalty": 0.0,
        }
    elif ptype in ("balanced", "bal", "balance"):
        profile = {
            "temperature": round(random.uniform(0.2, 0.4), 2),
            "seed": random.randint(1000, 99999),
            "top_p": round(random.uniform(0.85, 0.95), 2),
            "max_tokens": 1024,
            "frequency_penalty": 0.0,
            "presence_penalty": 0.0,
        }
    elif ptype in ("creative", "crea"):
        profile = {
            "temperature": round(random.uniform(0.65, 0.85), 2),
            "seed": random.randint(1000, 99999),
            "top_p": round(random.uniform(0.90, 0.98), 2),
            "max_tokens": 1024,
            "frequency_penalty": round(random.uniform(0.05, 0.15), 2),
            "presence_penalty": round(random.uniform(0.05, 0.15), 2),
        }
    else:
        raise ValueError(
            f"Type de profil inconnu '{ptype}'. Types supportés : 'default', 'deterministic', 'balanced', 'creative'."
        )

    # Application des surcharges
    for k, v in overrides.items():
        profile[k] = v

    # Détermination du nom unique de clé
    if custom_name:
        generated_name = custom_name
    else:
        if ptype in ("default", "def", "gguf"):
            generated_name = "default_gguf"
        elif ptype in ("deterministic", "det", "strict"):
            s_val = profile.get("seed")
            generated_name = f"deterministic_seed{s_val}" if s_val is not None else "deterministic"
        elif ptype in ("balanced", "bal", "balance"):
            t_val = profile.get("temperature")
            s_val = profile.get("seed")
            t_str = f"temp{int(round(t_val*100)):02d}" if t_val is not None else "tempdef"
            s_str = f"seed{s_val}" if s_val is not None else "noseed"
            generated_name = f"balanced_{t_str}_{s_str}"
        elif ptype in ("creative", "crea"):
            t_val = profile.get("temperature")
            s_val = profile.get("seed")
            t_str = f"temp{int(round(t_val*100)):02d}" if t_val is not None else "tempdef"
            s_str = f"seed{s_val}" if s_val is not None else "noseed"
            generated_name = f"creative_{t_str}_{s_str}"
        else:
            generated_name = ptype

    return generated_name, profile


def build_profiles_from_specs(specs: List[str]) -> Dict[str, Dict[str, Any]]:
    """
    Parse une liste de spécifications CLI (--profil) et assemble un dictionnaire de profils uniques.
    Garantit des noms de profils uniques en ajoutant un suffixe numérique si nécessaire.
    """
    results: Dict[str, Dict[str, Any]] = {}
    for spec in specs:
        name, prof = generate_profile_from_spec(spec)
        final_name = name
        counter = 2
        while final_name in results:
            final_name = f"{name}_{counter}"
            counter += 1
        results[final_name] = prof
    return results


def build_benchmark_config(
    hardware_summary: str,
    models: List[Dict[str, Any]],
    custom_profiles: Optional[Dict[str, Dict[str, Any]]] = None,
) -> Dict[str, Any]:
    """Assembles a complete configuration payload."""
    profiles = custom_profiles if custom_profiles is not None else DEFAULT_INFERENCE_PROFILES
    return {
        "hardware_profile": hardware_summary,
        "inference_profiles": profiles,
        "models": models,
    }
