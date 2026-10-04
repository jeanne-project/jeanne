#!/usr/bin/env python3
"""
Jeanne LLM Benchmark Runner & Config Initializer
A standalone, deterministic benchmarking tool to evaluate and compare LLMs
for the Jeanne project across realistic local productivity scenarios.
"""

import argparse
import json
import os
import sys
from typing import Any, Dict, List, Optional

# Ensure local imports work regardless of current working directory
current_dir = os.path.dirname(os.path.abspath(__file__))
if current_dir not in sys.path:
    sys.path.insert(0, current_dir)

from client import LlmClient
from config_template import DEFAULT_INFERENCE_PROFILES, build_benchmark_config
from embedded_engine import EmbeddedGgufEngine
from environment_setup import (
    check_and_relaunch_in_venv,
    create_and_setup_venv,
    find_venvs_with_llama_cpp,
    get_available_backend_profiles,
    get_best_matching_venv,
    get_venv_python_path,
)
from hardware_detector import detect_host_hardware
from model_scanner import (
    get_default_recommended_models,
    probe_ollama_models,
    scan_models_directory,
)
from reporter import BenchmarkReporter
from suites import get_all_test_cases, get_available_suites
from suites.base import (
    BenchmarkSuiteResult,
    InferenceParams,
    TestCase,
    TestResult,
    extract_thinking_stats,
)


DEFAULT_CONFIG_FILE = os.path.join(current_dir, "config.json")
EXAMPLE_CONFIG_FILE = os.path.join(current_dir, "config.example.json")


def load_config(config_path: str) -> Dict[str, Any]:
    """Loads configuration file or falls back to example config."""
    target = config_path
    if not os.path.exists(target):
        if os.path.exists(EXAMPLE_CONFIG_FILE):
            print(f"⚠️  Fichier {config_path} introuvable. Utilisation du template {EXAMPLE_CONFIG_FILE}")
            target = EXAMPLE_CONFIG_FILE
        else:
            raise FileNotFoundError(f"Configuration file not found: {config_path}")

    with open(target, "r", encoding="utf-8") as f:
        return json.load(f)


def parse_inference_profile(profile_dict: Dict[str, Any]) -> InferenceParams:
    """Parses a dictionary into an InferenceParams dataclass."""
    return InferenceParams(
        temperature=float(profile_dict.get("temperature", 0.0)),
        seed=profile_dict.get("seed", 42),
        top_p=float(profile_dict.get("top_p", 1.0)),
        max_tokens=int(profile_dict.get("max_tokens", 1024)),
        frequency_penalty=float(profile_dict.get("frequency_penalty", 0.0)),
        presence_penalty=float(profile_dict.get("presence_penalty", 0.0)),
    )


# ==============================================================================
# SUB-COMMAND: INIT (Génération automatique de configuration)
# ==============================================================================

def execute_init(args: argparse.Namespace) -> None:
    """Detects host hardware, scans for model files, and generates config.json."""
    # Basculer sur un virtualenv spécifique si demandé
    target_venv = getattr(args, "venv", None)
    if target_venv:
        check_and_relaunch_in_venv(target_venv, current_dir, auto_detect=False)

    output_path = args.output
    if not os.path.isabs(output_path):
        output_path = os.path.abspath(os.path.join(current_dir, output_path))

    if os.path.exists(output_path) and not args.force:
        print(f"⚠️  Le fichier de configuration '{output_path}' existe déjà.")
        print("   Utilisez l'option '--force' (ou '-f') pour l'écraser.")
        sys.exit(1)

    print("\n" + "=" * 70)
    print("   🔍 INITIALISATION AUTOMATIQUE DU BENCHMARK JEANNE")
    print("=" * 70)

    # 1. Détection matérielle
    print("🖥️  Sondage des spécifications matérielles de la machine hôte...")
    hw = detect_host_hardware()
    hw_summary = hw["summary"]
    print(f"   • Système : {hw['os']}")
    print(f"   • Processeur : {hw['cpu']} ({hw['cpu_threads']} threads logiques)")
    print(f"   • Mémoire vive (RAM) : {hw['ram_gb']} Go")
    print(f"   • Graphismes / GPU : {hw['gpu']}")
    print(f"   • Accélération matérielle : {'Oui (Vulkan/Metal)' if hw['hardware_acceleration'] else 'CPU uniquement'}")
    has_embedded, embedded_desc = EmbeddedGgufEngine.is_available()
    print(f"   • Moteur GGUF embarqué (In-Process) : {'✔️ ' + embedded_desc if has_embedded else '⚪ Non installé (pip install llama-cpp-python)'}")

    # 2. Découverte des modèles
    models: List[Dict[str, Any]] = []

    if args.models_dir:
        print(f"\n📂 Recherche de modèles dans le dossier : {args.models_dir}")
        discovered = scan_models_directory(
            models_dir=args.models_dir,
            default_endpoint=args.endpoint,
        )
        if discovered:
            print(f"   ✔️ {len(discovered)} modèle(s) éligible(s) retenu(s) pour le benchmark :")
            for m in discovered:
                print(f"      - {m['id']} [{m.get('size', '')}]")
            models.extend(discovered)
        else:
            print("   ℹ️  Aucun fichier de modèle éligible (.gguf, .bin, .safetensors) trouvé dans ce dossier.")

    # 3. Sonde Ollama si aucun dossier ou pas de modèles trouvés
    if not models and not args.no_probe:
        print("\n🦙 Vérification de la présence d'un serveur Ollama local actif...")
        ollama_models = probe_ollama_models(default_profiles=["deterministic_strict", "balanced_temp03"])
        if ollama_models:
            print(f"   ✔️ {len(ollama_models)} modèle(s) détecté(s) sur Ollama :")
            for m in ollama_models:
                print(f"      - {m['id']} [{m.get('size', '')}]")
            models.extend(ollama_models)
        else:
            print("   ℹ️  Serveur Ollama non détecté ou aucun modèle téléchargé.")

    # 4. Modèles de référence recommandés si rien n'a été découvert
    if not models:
        print("\n📦 Inclusion des modèles de référence recommandés pour Jeanne...")
        models = get_default_recommended_models(default_endpoint=args.endpoint)
        for m in models:
            print(f"   - {m['display_name']} (Endpoint: {m['endpoint']})")

    # 5. Construction de la configuration finale
    config_dict = build_benchmark_config(
        hardware_summary=hw_summary,
        models=models,
        custom_profiles=DEFAULT_INFERENCE_PROFILES,
    )

    # Création du dossier parent si nécessaire
    os.makedirs(os.path.dirname(output_path), exist_ok=True)

    with open(output_path, "w", encoding="utf-8") as f:
        json.dump(config_dict, f, indent=2, ensure_ascii=False)

    print("\n" + "=" * 70)
    print("✅ CONFIGURATION INITIALISÉE AVEC SUCCÈS !")
    print("=" * 70)
    print(f"📄 Fichier généré : \033[1;32m{output_path}\033[0m")
    print(f"⚙️  Profils d'inférence configurés : {list(DEFAULT_INFERENCE_PROFILES.keys())}")
    print(f"🤖 Modèles configurés : {len(models)}")
    print("\n👉 Vous pouvez dès à présent lancer le benchmark avec :")
    print("   python3 tools/llm-benchmark/benchmark.py")

    try:
        detected_venvs = find_venvs_with_llama_cpp([current_dir])
        if detected_venvs:
            best_v = get_best_matching_venv(detected_venvs)
            if best_v:
                v_alias = best_v["name"].replace(".venv-", "")
                print(f"\n💡 Environnement optimisé détecté (\033[1;36m{best_v['name']}\033[0m, backend {best_v['backend'].upper()}) :")
                print(f"   python3 tools/llm-benchmark/benchmark.py run --venv {v_alias}")
    except Exception:
        pass
    print("=" * 70 + "\n")


# ==============================================================================
# SUB-COMMAND: SETUP (Préparation d'environnements virtuels CUDA / Vulkan / CPU)
# ==============================================================================

def execute_setup(args: argparse.Namespace) -> None:
    """Prepares isolated Python virtual environments with specialized llama.cpp builds."""
    print("\n" + "=" * 70)
    print("   🛠️  PRÉPARATION DE L'ENVIRONNEMENT D'INFÉRENCE LOCAL")
    print("=" * 70)

    hw = detect_host_hardware()
    print(f"🖥️  Matériel détecté : {hw['summary']}")
    print(f"🎮 Accélération graphique : {hw['gpu']}")

    available_profiles = get_available_backend_profiles()

    print("\n📋 Configurations d'environnements disponibles pour votre matériel :")
    for idx, p in enumerate(available_profiles, start=1):
        rec_tag = " \033[1;32m(Recommandé)\033[0m" if p.is_recommended else ""
        print(f"   [{idx}] \033[1m{p.id.upper()}\033[0m - {p.display_name}{rec_tag}")
        print(f"       Dossier venv : {p.venv_name}")
        print(f"       Description  : {p.description}")

    if getattr(args, "list", False):
        print("\nOption --list activée. Fin de la commande setup.")
        return

    chosen_backend = getattr(args, "backend", None)

    # Mode interactif si aucun backend spécifié et stdin est un terminal tty
    if not chosen_backend and sys.stdin.isatty():
        print("\n👉 Quel environnement souhaitez-vous préparer ?")
        print("   Entrez un numéro, l'identifiant (ex: cuda, vulkan, cpu) ou 'all' pour tous : ", end="", flush=True)
        try:
            user_input = sys.stdin.readline().strip().lower()
            if user_input in ("all", "tous", "*"):
                chosen_backend = "all"
            elif user_input.isdigit() and 1 <= int(user_input) <= len(available_profiles):
                chosen_backend = available_profiles[int(user_input) - 1].id
            elif any(p.id == user_input for p in available_profiles):
                chosen_backend = user_input
            else:
                print("❌ Choix invalide.")
                sys.exit(1)
        except Exception:
            chosen_backend = None

    if not chosen_backend:
        print("\nℹ️  Aucun backend spécifié. Vous pouvez préparer un environnement avec :")
        for p in available_profiles:
            print(f"   python3 tools/llm-benchmark/benchmark.py setup --backend {p.id}")
        print("   python3 tools/llm-benchmark/benchmark.py setup --backend all")
        return

    if chosen_backend.lower() == "all":
        profiles_to_install = available_profiles
    else:
        matched = [p for p in available_profiles if p.id.lower() == chosen_backend.lower()]
        if not matched:
            print(f"❌ Backend inconnu '{chosen_backend}'. Disponibles : {[p.id for p in available_profiles]}")
            sys.exit(1)
        profiles_to_install = matched

    dry_run = getattr(args, "dry_run", False)
    successful_venvs = []

    for prof in profiles_to_install:
        ok, venv_res = create_and_setup_venv(
            profile=prof,
            base_dir=current_dir,
            dry_run=dry_run,
        )
        if ok:
            successful_venvs.append((prof, venv_res))

    print("\n" + "=" * 70)
    print("🎉 PRÉPARATION TERMINÉE !")
    print("=" * 70)
    if not dry_run and successful_venvs:
        print("💡 Pour exécuter votre benchmark dans un environnement spécifique :")
        for prof, venv_dir in successful_venvs:
            print(f"\n👉 Backend \033[1m{prof.id.upper()}\033[0m :")
            print(f"   • En une seule commande :")
            print(f"     python3 tools/llm-benchmark/benchmark.py run --venv {prof.id}")
            print(f"   • Ou en activant manuellement le venv :")
            if platform.system() == "Windows":
                print(f"     {venv_dir}\\Scripts\\activate")
            else:
                print(f"     source {venv_dir}/bin/activate")
            print(f"     python3 tools/llm-benchmark/benchmark.py")
    print("=" * 70 + "\n")


# ==============================================================================
# SUB-COMMAND: RUN (Exécution du benchmark)
# ==============================================================================

def run_benchmark_for_model_profile(
    model_cfg: Dict[str, Any],
    profile_name: str,
    params: InferenceParams,
    test_cases: List[TestCase],
    mock_mode: bool = False,
    verbose: bool = False,
) -> BenchmarkSuiteResult:
    """Runs all test cases for a specific model under a specific inference profile."""
    model_id = model_cfg["id"]
    display_name = model_cfg.get("display_name", model_id)
    endpoint = model_cfg.get("endpoint", "http://localhost:11434/v1")
    api_key = model_cfg.get("api_key")
    api_key_env = model_cfg.get("api_key_env")
    timeout = float(model_cfg.get("timeout_secs", 60.0))

    file_path = model_cfg.get("file_path")
    engine = model_cfg.get("engine", "auto")

    client = LlmClient(
        endpoint=endpoint,
        model=model_id,
        file_path=file_path,
        engine=engine,
        api_key=api_key,
        api_key_env=api_key_env,
        timeout=timeout,
        mock_mode=mock_mode,
    )

    engine_tag = " [Moteur Embarqué GGUF]" if client.is_embedded else f" [Endpoint: {endpoint}]"
    print(f"\n🚀 Lancement du benchmark pour : \033[1m{display_name}\033[0m{engine_tag}")
    print(f"   ⚙️  Profil: \033[36m{profile_name}\033[0m ({params.summary_str()})")
    print(f"   🌐 Endpoint: {endpoint}")

    suite_result = BenchmarkSuiteResult(
        model_id=model_id,
        model_display_name=display_name,
        profile_name=profile_name,
        inference_params=params.to_dict(),
    )

    category_scores_map: Dict[str, List[float]] = {}

    for idx, tc in enumerate(test_cases, start=1):
        print(f"   [{idx}/{len(test_cases)}] Test '{tc.name}' ({tc.category})... ", end="", flush=True)

        effective_params = tc.recommended_params or params

        try:
            response_text, ttft_ms, total_latency_ms, tokens_count, tokens_per_sec = client.generate(
                messages=tc.messages,
                params=effective_params,
            )

            # Extraction et décompte des tokens de réflexion (<think>...</think>)
            thinking_tokens, clean_text = extract_thinking_stats(response_text)

            assertions = tc.evaluator(clean_text)

            if assertions:
                avg_score = (sum(a.score for a in assertions) / len(assertions)) * 100.0
                all_passed = all(a.passed for a in assertions)
            else:
                avg_score = 100.0
                all_passed = True

            test_res = TestResult(
                test_id=tc.id,
                test_name=tc.name,
                category=tc.category,
                model_id=model_id,
                profile_name=profile_name,
                inference_params=effective_params.to_dict(),
                passed=all_passed,
                score=avg_score,
                ttft_ms=ttft_ms,
                total_latency_ms=total_latency_ms,
                tokens_generated=tokens_count,
                tokens_per_sec=tokens_per_sec,
                response_text=response_text,
                assertions=assertions,
                thinking_tokens=thinking_tokens,
            )

            duration_s = total_latency_ms / 1000.0
            think_tag = f" (🧠 {thinking_tokens} think)" if thinking_tokens > 0 else ""
            metrics_str = f"[{duration_s:.2f}s | {tokens_count} tok{think_tag} | TTFT: {ttft_ms:.0f}ms | {tokens_per_sec:.1f} tps]"

            if all_passed:
                print(f"\033[32mPASS ({avg_score:.0f}%)\033[0m {metrics_str}")
            elif avg_score > 0:
                print(f"\033[33mPARTIEL ({avg_score:.0f}%)\033[0m {metrics_str}")
            else:
                print(f"\033[31mFAIL (0%)\033[0m {metrics_str}")

            if verbose:
                for ass in assertions:
                    icon = "  ✔️" if ass.passed else "  ❌"
                    print(f"     {icon} {ass.name}: {ass.details}")

        except Exception as e:
            print(f"\033[31mERROR: {str(e)}\033[0m")
            test_res = TestResult(
                test_id=tc.id,
                test_name=tc.name,
                category=tc.category,
                model_id=model_id,
                profile_name=profile_name,
                inference_params=effective_params.to_dict(),
                passed=False,
                score=0.0,
                ttft_ms=0.0,
                total_latency_ms=0.0,
                tokens_generated=0,
                tokens_per_sec=0.0,
                response_text="",
                assertions=[],
                thinking_tokens=0,
                error=str(e),
            )

        suite_result.test_results.append(test_res)

        if tc.category not in category_scores_map:
            category_scores_map[tc.category] = []
        category_scores_map[tc.category].append(test_res.score)

    if suite_result.test_results:
        suite_result.overall_score = sum(t.score for t in suite_result.test_results) / len(suite_result.test_results)
        valid_ttfts = [t.ttft_ms for t in suite_result.test_results if t.ttft_ms > 0]
        valid_tps = [t.tokens_per_sec for t in suite_result.test_results if t.tokens_per_sec > 0]
        suite_result.avg_ttft_ms = (sum(valid_ttfts) / len(valid_ttfts)) if valid_ttfts else 0.0
        suite_result.avg_tokens_per_sec = (sum(valid_tps) / len(valid_tps)) if valid_tps else 0.0

        suite_result.total_duration_sec = sum(t.total_latency_ms for t in suite_result.test_results) / 1000.0
        suite_result.total_tokens_generated = sum(t.tokens_generated for t in suite_result.test_results)
        suite_result.total_thinking_tokens = sum(t.thinking_tokens for t in suite_result.test_results)

    for cat, scores in category_scores_map.items():
        suite_result.category_scores[cat] = sum(scores) / len(scores)

    think_summary = f" (🧠 {suite_result.total_thinking_tokens} pensée)" if suite_result.total_thinking_tokens > 0 else ""
    print(
        f"   ⭐ Score Global : \033[1;35m{suite_result.overall_score:.1f} / 100\033[0m "
        f"[Durée: {suite_result.total_duration_sec:.1f}s | Tokens: {suite_result.total_tokens_generated}{think_summary} | "
        f"TTFT moy: {suite_result.avg_ttft_ms:.0f}ms, TPS: {suite_result.avg_tokens_per_sec:.1f}]"
    )

    client.unload()
    return suite_result


def execute_run(args: argparse.Namespace) -> None:
    """Executes the benchmark matrix and generates reports."""
    # Basculer automatiquement sur le virtualenv spécifié ou détecté
    target_venv = getattr(args, "venv", None)
    no_auto_venv = getattr(args, "no_auto_venv", False)
    check_and_relaunch_in_venv(target_venv, current_dir, auto_detect=not no_auto_venv)

    try:
        config = load_config(args.config)
    except Exception as e:
        print(f"❌ Erreur de chargement de la configuration: {e}")
        print("💡 Vous pouvez initialiser un fichier de configuration avec :")
        print("   python3 tools/llm-benchmark/benchmark.py init")
        sys.exit(1)

    hardware_profile = config.get("hardware_profile", "PC Personnel")
    inference_profiles_dict = config.get("inference_profiles", {})

    if not inference_profiles_dict:
        inference_profiles_dict = DEFAULT_INFERENCE_PROFILES

    available_suites = get_available_suites()
    if args.suites:
        selected_suite_names = [s.strip().lower() for s in args.suites.split(",")]
        test_cases = []
        for sname in selected_suite_names:
            if sname in available_suites:
                test_cases.extend(available_suites[sname])
            else:
                print(f"⚠️  Suite inconnue : '{sname}'. Suites disponibles : {list(available_suites.keys())}")
    else:
        test_cases = get_all_test_cases()

    if not test_cases:
        print("❌ Aucun test sélectionné.")
        sys.exit(1)

    all_models = config.get("models", [])
    if args.models:
        selected_model_ids = [m.strip() for m in args.models.split(",")]
        models_to_test = [m for m in all_models if m["id"] in selected_model_ids]
    else:
        models_to_test = all_models

    if not models_to_test:
        print("❌ Aucun modèle à tester dans la configuration.")
        sys.exit(1)

    allowed_profiles = [p.strip() for p in args.profiles.split(",")] if args.profiles else None

    print("\n" + "=" * 70)
    print("   🧠 JEANNE LLM BENCHMARK SUITE - TEST D'ADÉQUATION PROJET")
    print("=" * 70)
    print(f"📍 Machine hôte : {hardware_profile}")
    print(f"🧪 Nombre de cas de tests chargés : {len(test_cases)}")
    print(f"🤖 Modèles sélectionnés : {len(models_to_test)}")
    if args.mock:
        print("💡 Mode simulation (--mock) actif.")
    print("=" * 70)

    all_results: List[BenchmarkSuiteResult] = []

    for model_cfg in models_to_test:
        model_profiles = model_cfg.get("profiles", list(inference_profiles_dict.keys()))

        for p_name in model_profiles:
            if allowed_profiles and p_name not in allowed_profiles:
                continue

            if p_name not in inference_profiles_dict:
                print(f"⚠️  Profil '{p_name}' non trouvé dans inference_profiles, ignoré.")
                continue

            p_params = parse_inference_profile(inference_profiles_dict[p_name])
            res = run_benchmark_for_model_profile(
                model_cfg=model_cfg,
                profile_name=p_name,
                params=p_params,
                test_cases=test_cases,
                mock_mode=args.mock,
                verbose=args.verbose,
            )
            all_results.append(res)

    reporter = BenchmarkReporter(
        hardware_profile=hardware_profile,
        suites_results=all_results,
        output_dir=args.output_dir,
    )

    report_files = reporter.generate()

    print("\n" + "=" * 70)
    print("🎉 BENCHMARK TERMINÉ AVEC SUCCÈS !")
    print("=" * 70)
    print(f"📄 Rapport Markdown généré (prêt pour GitHub) :\n   👉 \033[1;32m{report_files['markdown']}\033[0m")
    print(f"💾 Données brutes JSON générées :\n   👉 \033[1;34m{report_files['json']}\033[0m")
    print("=" * 70 + "\n")


# ==============================================================================
# MAIN CLI DISPATCHER
# ==============================================================================

def main():
    parser = argparse.ArgumentParser(
        description="Jeanne LLM Benchmark - Banc de test déterministe pour modèles de langage."
    )

    subparsers = parser.add_subparsers(dest="command", help="Commande à exécuter")

    # Sub-command: init
    init_parser = subparsers.add_parser("init", help="Initialiser la configuration automatiquement")
    init_parser.add_argument(
        "--models-dir",
        "-d",
        help="Chemin vers un dossier local contenant les fichiers modèles GGUF à tester",
    )
    init_parser.add_argument(
        "--endpoint",
        "-e",
        default="http://localhost:11434/v1",
        help="Endpoint d'inférence par défaut (défaut: http://localhost:11434/v1)",
    )
    init_parser.add_argument(
        "--output",
        "-o",
        default=DEFAULT_CONFIG_FILE,
        help="Chemin de sortie du fichier de configuration (défaut: config.json)",
    )
    init_parser.add_argument(
        "--force",
        "-f",
        action="store_true",
        help="Écraser le fichier de configuration s'il existe déjà",
    )
    init_parser.add_argument(
        "--no-probe",
        action="store_true",
        help="Ne pas sonder le serveur Ollama local",
    )
    init_parser.add_argument(
        "--venv",
        help="Sonder ou initialiser sous un environnement virtuel spécifique (ex: cuda, vulkan, cpu, .venv-cuda)",
    )

    # Sub-command: setup
    setup_parser = subparsers.add_parser("setup", help="Préparer les environnements virtuels isolés (CUDA, Vulkan, CPU)")
    setup_parser.add_argument(
        "--backend",
        "-b",
        choices=["cuda", "vulkan", "cpu", "metal", "all"],
        help="Backend spécifique à configurer (ex: cuda, vulkan, cpu, all)",
    )
    setup_parser.add_argument(
        "--list",
        "-l",
        action="store_true",
        help="Lister les profils d'environnement disponibles pour la machine hôte",
    )
    setup_parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Afficher les commandes sans créer les environnements virtuels ni installer de paquets",
    )

    # Sub-command: run
    run_parser = subparsers.add_parser("run", help="Exécuter les benchmarks")
    for p in [parser, run_parser]:
        # Support both top-level arguments (python3 benchmark.py --mock) and run sub-command
        if p is parser:
            # We add top-level arguments directly to parser as fallback
            pass
        p.add_argument(
            "--config",
            "-c",
            default=DEFAULT_CONFIG_FILE,
            help="Chemin vers le fichier de configuration JSON (défaut: config.json)",
        )
        p.add_argument(
            "--venv",
            help="Exécuter le benchmark dans un environnement virtuel spécifique (ex: cuda, vulkan, cpu, .venv-cuda)",
        )
        p.add_argument(
            "--no-auto-venv",
            action="store_true",
            help="Désactiver la bascule automatique sur un virtualenv détecté",
        )
        p.add_argument(
            "--models",
            "-m",
            help="Liste d'identifiants de modèles à tester, séparés par des virgules",
        )
        p.add_argument(
            "--profiles",
            "-p",
            help="Liste de profils d'inférence à tester, séparés par des virgules",
        )
        p.add_argument(
            "--suites",
            "-s",
            help="Suites à exécuter (rag, pii, structured, meeting, conciseness) séparées par des virgules",
        )
        p.add_argument(
            "--output-dir",
            default=os.path.join(current_dir, "reports"),
            help="Répertoire de sortie des rapports (défaut: reports/)",
        )
        p.add_argument(
            "--mock",
            action="store_true",
            help="Mode simulation déterministe (aucun serveur LLM actif requis)",
        )
        p.add_argument(
            "--verbose",
            "-v",
            action="store_true",
            help="Affichage détaillé des assertions en console",
        )

    # Command dispatching
    if len(sys.argv) > 1 and sys.argv[1] == "init":
        args = parser.parse_args()
        execute_init(args)
    elif len(sys.argv) > 1 and sys.argv[1] == "setup":
        args = parser.parse_args()
        execute_setup(args)
    elif len(sys.argv) > 1 and sys.argv[1] == "run":
        args = parser.parse_args()
        execute_run(args)
    else:
        # Default fallback to run with top-level flags (e.g. `benchmark.py --mock`)
        args = parser.parse_args()
        execute_run(args)


if __name__ == "__main__":
    main()
