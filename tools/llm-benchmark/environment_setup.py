"""Environment preparation helper: detects hardware capabilities, configures isolated venvs (CUDA, Vulkan, CPU), and installs llama-cpp-python."""

import os
import platform
import shutil
import subprocess
import sys
from dataclasses import dataclass
from typing import Any, Dict, List, Optional, Tuple
from hardware_detector import detect_host_hardware


@dataclass
class BackendProfile:
    id: str
    display_name: str
    venv_name: str
    description: str
    pip_install_args: List[str]
    is_recommended: bool = False
    env_vars: Optional[Dict[str, str]] = None


def get_available_backend_profiles() -> List[BackendProfile]:
    """Determines supported backend profiles based on detected hardware."""
    hw = detect_host_hardware()
    gpu_str = hw.get("gpu", "").lower()
    system = platform.system()

    profiles = []

    # 1. NVIDIA CUDA Profile
    if "nvidia" in gpu_str:
        profiles.append(
            BackendProfile(
                id="cuda",
                display_name="NVIDIA CUDA (Accélération maximale)",
                venv_name=".venv-cuda",
                description="Exploite les cœurs CUDA et la VRAM dédiée NVIDIA. Vitesse maximale pour GeForce RTX/GTX.",
                pip_install_args=[
                    "llama-cpp-python",
                    "--extra-index-url",
                    "https://abetlen.github.io/llama-cpp-python/whl/cu124",
                ],
                is_recommended=True,
            )
        )

    # 2. Vulkan Profile (Universal GPU / iGPU)
    vulkan_recommended = ("radeon" in gpu_str or "iris" in gpu_str or "intel" in gpu_str)
    profiles.append(
        BackendProfile(
            id="vulkan",
            display_name="Vulkan (Moteur universel Jeanne)",
            venv_name=".venv-vulkan",
            description="Accélération Vulkan sur shaders de calcul (AMD Radeon, Intel Iris Xe, ou NVIDIA en mode Vulkan).",
            pip_install_args=["llama-cpp-python"],
            env_vars={"CMAKE_ARGS": "-DGGML_VULKAN=on"},
            is_recommended=vulkan_recommended,
        )
    )

    # 3. Apple Silicon Metal Profile
    if system == "Darwin" and "apple" in gpu_str:
        profiles.append(
            BackendProfile(
                id="metal",
                display_name="Apple Silicon Metal",
                venv_name=".venv-metal",
                description="Accélération GPU unifiée Metal sur processeurs Apple Silicon (M1/M2/M3/M4).",
                pip_install_args=["llama-cpp-python"],
                env_vars={"CMAKE_ARGS": "-DGGML_METAL=on"},
                is_recommended=True,
            )
        )

    # 4. CPU Baseline Profile (toujours disponible)
    profiles.append(
        BackendProfile(
            id="cpu",
            display_name="CPU Standard (Baseline de référence)",
            venv_name=".venv-cpu",
            description="Inférence 100% sur processeur (AVX2/AVX-512) sans accélération graphique. Référence de comparaison.",
            pip_install_args=["llama-cpp-python"],
            is_recommended=not any(p.is_recommended for p in profiles),
        )
    )

    return profiles


def get_venv_python_path(venv_dir: str) -> str:
    """Returns the path to the python binary inside the given venv directory."""
    if platform.system() == "Windows":
        return os.path.join(venv_dir, "Scripts", "python.exe")
    return os.path.join(venv_dir, "bin", "python")


def create_and_setup_venv(
    profile: BackendProfile,
    base_dir: str,
    dry_run: bool = False,
) -> Tuple[bool, str]:
    """Creates a virtual environment and installs the specified backend."""
    venv_dir = os.path.join(base_dir, profile.venv_name)
    python_bin = get_venv_python_path(venv_dir)

    print(f"\n📦 Préparation de l'environnement : \033[1;36m{profile.display_name}\033[0m")
    print(f"   Dossier venv : {venv_dir}")
    print(f"   Description  : {profile.description}")

    if dry_run:
        print("   [Mode Simulation] Commandes qui seraient exécutées :")
        print(f"   1. {sys.executable} -m venv {venv_dir}")
        print(f"   2. {python_bin} -m pip install --upgrade pip")
        env_str = " ".join(f"{k}='{v}'" for k, v in (profile.env_vars or {}).items())
        cmd_str = " ".join(profile.pip_install_args)
        print(f"   3. {env_str} {python_bin} -m pip install {cmd_str}")
        return True, venv_dir

    # 1. Création du virtualenv s'il n'existe pas
    if not os.path.exists(python_bin):
        print("   ⏳ Création du virtualenv isolé...")
        try:
            subprocess.run([sys.executable, "-m", "venv", venv_dir], check=True)
        except Exception as e:
            print(f"   ❌ Erreur lors de la création du venv: {e}")
            return False, str(e)
    else:
        print("   ℹ️  Virtualenv existant détecté, mise à jour des paquets...")

    # 2. Mise à niveau de pip
    print("   ⏳ Mise à jour de pip...")
    try:
        subprocess.run(
            [python_bin, "-m", "pip", "install", "--upgrade", "pip"],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    except Exception:
        pass

    # 3. Installation de llama-cpp-python avec les variables d'environnement
    print("   ⏳ Installation de llama-cpp-python...")
    env = os.environ.copy()
    if profile.env_vars:
        env.update(profile.env_vars)

    pip_cmd = [python_bin, "-m", "pip", "install"] + profile.pip_install_args

    try:
        res = subprocess.run(pip_cmd, env=env, capture_output=True, text=True)
        if res.returncode != 0:
            print(f"   ⚠️  Avertissement lors de l'installation : {res.stderr[:300]}")
            # Si la roue précompilée a échoué (ex: CUDA), proposer le fallback
            if profile.id == "cuda" and "--extra-index-url" in profile.pip_install_args:
                print("   🔄 Tentative de repli via compilation directe CUDA...")
                env["CMAKE_ARGS"] = "-DGGML_CUDA=on"
                res_fb = subprocess.run([python_bin, "-m", "pip", "install", "llama-cpp-python"], env=env, capture_output=True, text=True)
                if res_fb.returncode == 0:
                    print("   ✔️ Installation réussie via fallback CUDA !")
                    return True, venv_dir
            return False, res.stderr
    except Exception as e:
        print(f"   ❌ Erreur d'exécution pip : {e}")
        return False, str(e)

    print("   ✔️ Environnement installé et prêt avec succès !")
    return True, venv_dir


def check_and_relaunch_in_venv(venv_identifier: str, base_dir: str) -> None:
    """
    Relaunches the current script inside the specified venv python interpreter
    if not already running inside it.
    """
    target_venv = venv_identifier
    if not target_venv.startswith(".venv-") and not os.path.isabs(target_venv):
        # Allow passing alias like "cuda", "vulkan", "cpu"
        target_venv = f".venv-{target_venv}"

    if not os.path.isabs(target_venv):
        target_venv = os.path.join(base_dir, target_venv)

    venv_python = get_venv_python_path(target_venv)

    if not os.path.exists(venv_python):
        print(f"❌ L'environnement virtuel '{target_venv}' n'existe pas.")
        print("💡 Vous pouvez le créer automatiquement avec :")
        print(f"   python3 tools/llm-benchmark/benchmark.py setup --backend {venv_identifier.replace('.venv-', '')}")
        sys.exit(1)

    # Check if we are already inside this venv
    current_python = os.path.abspath(sys.executable)
    target_python_abs = os.path.abspath(venv_python)

    if current_python != target_python_abs:
        # Filter out the --venv argument to avoid loop
        new_argv = [venv_python]
        skip_next = False
        for arg in sys.argv:
            if skip_next:
                skip_next = False
                continue
            if arg in ("--venv", "-v"):
                skip_next = True
                continue
            if arg.startswith("--venv="):
                continue
            new_argv.append(arg)

        # Replace process with the venv python
        try:
            os.execv(target_python_abs, new_argv)
        except Exception as e:
            print(f"❌ Échec de relance dans le venv : {e}")
            sys.exit(1)
