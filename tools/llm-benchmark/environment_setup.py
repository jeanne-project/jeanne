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
    """Returns the path to the python binary inside the given venv directory (cross-platform)."""
    candidates = [
        os.path.join(venv_dir, "Scripts", "python.exe"),
        os.path.join(venv_dir, "Scripts", "python"),
        os.path.join(venv_dir, "bin", "python"),
        os.path.join(venv_dir, "bin", "python3"),
        os.path.join(venv_dir, "bin", "python.exe"),
    ]
    for c in candidates:
        if os.path.isfile(c):
            return c

    # Fallback standard path according to OS
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


def get_candidate_venv_dirs(base_dirs: Optional[List[str]] = None) -> List[str]:
    """
    Scans common project locations to discover virtual environments.
    Checks:
      - os.environ['VIRTUAL_ENV']
      - tools/llm-benchmark/
      - tools/llm-benchmark/venvs/
      - Current working directory (os.getcwd())
      - Project root directory (repo root)
    """
    search_dirs: List[str] = []
    if base_dirs:
        for d in base_dirs:
            if d and os.path.isdir(d) and d not in search_dirs:
                search_dirs.append(os.path.abspath(d))

    pkg_dir = os.path.dirname(os.path.abspath(__file__))
    if pkg_dir not in search_dirs:
        search_dirs.append(pkg_dir)

    venvs_sub = os.path.join(pkg_dir, "venvs")
    if os.path.isdir(venvs_sub) and venvs_sub not in search_dirs:
        search_dirs.append(venvs_sub)

    cwd = os.getcwd()
    if os.path.isdir(cwd) and cwd not in search_dirs:
        search_dirs.append(os.path.abspath(cwd))

    repo_root = os.path.dirname(os.path.dirname(pkg_dir))
    if os.path.isdir(repo_root) and repo_root not in search_dirs:
        search_dirs.append(repo_root)

    candidates: List[str] = []
    seen = set()

    def add_candidate(path: str):
        norm = os.path.normcase(os.path.abspath(path))
        if norm not in seen and os.path.isdir(path):
            seen.add(norm)
            candidates.append(os.path.abspath(path))

    # Active virtualenv in environment
    active_env = os.environ.get("VIRTUAL_ENV")
    if active_env:
        add_candidate(active_env)

    # Standard names to probe directly
    standard_names = [
        ".venv-cuda",
        ".venv-vulkan",
        ".venv-cpu",
        ".venv-metal",
        ".venv",
        "venv",
        "env",
        ".env",
        "cuda",
        "vulkan",
        "cpu",
    ]

    for s_dir in search_dirs:
        for name in standard_names:
            add_candidate(os.path.join(s_dir, name))

        # Dynamic discovery of folders starting with .venv or containing pyvenv.cfg
        try:
            for entry in os.listdir(s_dir):
                entry_path = os.path.join(s_dir, entry)
                if os.path.isdir(entry_path) and (
                    entry.startswith(".venv")
                    or entry.startswith("venv")
                    or os.path.isfile(os.path.join(entry_path, "pyvenv.cfg"))
                ):
                    add_candidate(entry_path)
        except Exception:
            pass

    return candidates


def get_site_packages_dir(venv_dir: str) -> Optional[str]:
    """Finds site-packages directory inside a virtualenv."""
    # Windows: Lib/site-packages
    win_sp = os.path.join(venv_dir, "Lib", "site-packages")
    if os.path.isdir(win_sp):
        return win_sp

    # Linux / macOS: lib/pythonX.Y/site-packages
    lib_dir = os.path.join(venv_dir, "lib")
    if os.path.isdir(lib_dir):
        try:
            for entry in os.listdir(lib_dir):
                if entry.startswith("python"):
                    sp = os.path.join(lib_dir, entry, "site-packages")
                    if os.path.isdir(sp):
                        return sp
        except Exception:
            pass
    return None


def inspect_venv(venv_dir: str) -> Optional[Dict[str, Any]]:
    """
    Inspects a directory to check if it's a valid venv and whether llama-cpp-python is installed.
    Returns: dict with details or None if not a valid venv.
    """
    python_bin = get_venv_python_path(venv_dir)
    if not os.path.isfile(python_bin):
        return None

    venv_name = os.path.basename(os.path.normpath(venv_dir))
    site_packages = get_site_packages_dir(venv_dir)

    has_llama_cpp = False
    version = None

    # 1. Quick check via site-packages
    if site_packages and os.path.isdir(site_packages):
        llama_dir = os.path.join(site_packages, "llama_cpp")
        if os.path.isdir(llama_dir):
            has_llama_cpp = True
            try:
                for item in os.listdir(site_packages):
                    if item.startswith("llama_cpp_python-") and item.endswith(".dist-info"):
                        parts = item.replace(".dist-info", "").split("-")
                        if len(parts) >= 2:
                            version = parts[1]
                            break
            except Exception:
                pass

    # 2. Verification via subprocess
    if not version:
        try:
            res = subprocess.run(
                [python_bin, "-c", "import llama_cpp; print(llama_cpp.__version__)"],
                capture_output=True,
                text=True,
                timeout=2.0,
            )
            if res.returncode == 0 and res.stdout.strip():
                has_llama_cpp = True
                version = res.stdout.strip()
        except Exception:
            pass

    if not has_llama_cpp:
        return {
            "path": venv_dir,
            "name": venv_name,
            "python_path": python_bin,
            "site_packages": site_packages,
            "has_llama_cpp": False,
            "version": None,
            "backend": "unknown",
        }

    backend_hint = "cpu"
    name_lower = venv_name.lower()
    if "cuda" in name_lower:
        backend_hint = "cuda"
    elif "vulkan" in name_lower:
        backend_hint = "vulkan"
    elif "metal" in name_lower:
        backend_hint = "metal"

    return {
        "path": venv_dir,
        "name": venv_name,
        "python_path": python_bin,
        "site_packages": site_packages,
        "has_llama_cpp": True,
        "version": version or "inconnue",
        "backend": backend_hint,
    }


def find_venvs_with_llama_cpp(base_dirs: Optional[List[str]] = None) -> List[Dict[str, Any]]:
    """Returns a list of all discovered virtual environments containing llama-cpp-python."""
    candidates = get_candidate_venv_dirs(base_dirs)
    venvs_found = []
    seen = set()

    for c in candidates:
        info = inspect_venv(c)
        if info and info["has_llama_cpp"]:
            norm = os.path.normcase(os.path.abspath(info["path"]))
            if norm not in seen:
                seen.add(norm)
                venvs_found.append(info)

    return venvs_found


def get_best_matching_venv(venvs: List[Dict[str, Any]]) -> Optional[Dict[str, Any]]:
    """
    Chooses the most optimal venv for the host machine hardware.
    E.g. NVIDIA -> prefers CUDA venv; AMD/Intel -> prefers Vulkan venv.
    """
    if not venvs:
        return None

    hw = detect_host_hardware()
    gpu_str = hw.get("gpu", "").lower()

    if "nvidia" in gpu_str:
        for v in venvs:
            if v["backend"] == "cuda" or "cuda" in v["name"].lower():
                return v

    if any(k in gpu_str for k in ["radeon", "amd", "iris", "intel", "vulkan"]):
        for v in venvs:
            if v["backend"] == "vulkan" or "vulkan" in v["name"].lower():
                return v

    return venvs[0]


def try_activate_venv_in_process(venv_info: Optional[Dict[str, Any]] = None) -> bool:
    """
    Attempts to inject a venv's site-packages and DLL directories into the current running process
    so that `import llama_cpp` succeeds without restarting Python.
    """
    target = venv_info
    if not target:
        venvs = find_venvs_with_llama_cpp()
        target = get_best_matching_venv(venvs) if venvs else None

    if not target or not target.get("site_packages"):
        return False

    sp = target["site_packages"]
    if sp not in sys.path:
        sys.path.insert(0, sp)

    # Windows DLL directory handling
    if platform.system() == "Windows":
        v_dir = target["path"]
        scripts_dir = os.path.join(v_dir, "Scripts")
        llama_pkg_dir = os.path.join(sp, "llama_cpp")

        for dll_dir in [scripts_dir, llama_pkg_dir]:
            if os.path.isdir(dll_dir):
                if hasattr(os, "add_dll_directory"):
                    try:
                        os.add_dll_directory(dll_dir)
                    except Exception:
                        pass
                if dll_dir not in os.environ.get("PATH", ""):
                    os.environ["PATH"] = dll_dir + os.pathsep + os.environ.get("PATH", "")

    try:
        import llama_cpp
        return True
    except Exception:
        return False


def check_and_relaunch_in_venv(
    venv_identifier: Optional[str] = None,
    base_dir: Optional[str] = None,
    auto_detect: bool = True,
) -> bool:
    """
    Relaunches the current script inside the specified venv python interpreter
    or automatically detects an available venv with llama-cpp-python if needed.
    """
    # Prevent infinite relaunch loops
    if os.environ.get("JEANNE_BENCHMARK_RELAUNCHED") == "1":
        return False

    pkg_dir = base_dir or os.path.dirname(os.path.abspath(__file__))
    target_python: Optional[str] = None
    target_venv_name: str = ""

    if venv_identifier:
        # User explicitly specified a venv alias or path
        target_venv = venv_identifier
        if not target_venv.startswith(".venv-") and not os.path.isabs(target_venv):
            candidate_prefixed = f".venv-{target_venv}"
            if os.path.isdir(os.path.join(pkg_dir, candidate_prefixed)):
                target_venv = candidate_prefixed

        if not os.path.isabs(target_venv):
            target_venv = os.path.join(pkg_dir, target_venv)

        target_python = get_venv_python_path(target_venv)
        target_venv_name = os.path.basename(target_venv)

        if not os.path.exists(target_python):
            print(f"❌ L'environnement virtuel '{target_venv}' n'existe pas.")
            print("💡 Vous pouvez le créer automatiquement avec :")
            clean_id = venv_identifier.replace(".venv-", "")
            print(f"   python3 tools/llm-benchmark/benchmark.py setup --backend {clean_id}")
            sys.exit(1)
    elif auto_detect:
        # Check if current Python already has llama_cpp
        try:
            import llama_cpp
            return False  # Already in an environment with llama_cpp
        except ImportError:
            pass

        # Try to find existing venvs with llama_cpp
        venvs = find_venvs_with_llama_cpp([pkg_dir])
        best = get_best_matching_venv(venvs)
        if best and os.path.isfile(best["python_path"]):
            target_python = best["python_path"]
            target_venv_name = best["name"]
            print(f"\n💡 \033[1;36mMoteur llama-cpp-python détecté dans le venv '{target_venv_name}'.\033[0m")
            print(f"   Bascule automatique sur cet environnement pour l'exécution...")

    if not target_python or not os.path.isfile(target_python):
        return False

    current_python = os.path.abspath(sys.executable)
    target_python_abs = os.path.abspath(target_python)

    if os.path.normcase(current_python) == os.path.normcase(target_python_abs):
        return False

    # Build new argv, filtering out --venv argument to prevent loop
    new_argv = [target_python_abs]
    skip_next = False
    for arg in sys.argv[1:]:
        if skip_next:
            skip_next = False
            continue
        if arg in ("--venv", "-v"):
            skip_next = True
            continue
        if arg.startswith("--venv="):
            continue
        new_argv.append(arg)

    os.environ["JEANNE_BENCHMARK_RELAUNCHED"] = "1"

    if platform.system() == "Windows":
        # Windows compatibility: subprocess.call + exit ensures reliable execution
        try:
            ret = subprocess.call([target_python_abs] + new_argv[1:])
            sys.exit(ret)
        except Exception as e:
            print(f"❌ Échec de relance dans le venv sous Windows : {e}")
            sys.exit(1)
    else:
        try:
            os.execv(target_python_abs, new_argv)
        except Exception as e:
            print(f"❌ Échec de relance dans le venv : {e}")
            sys.exit(1)

    return True
