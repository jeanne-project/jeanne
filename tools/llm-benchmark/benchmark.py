#!/usr/bin/env python3
"""
Jeanne LLM Benchmark Runner
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
from reporter import BenchmarkReporter
from suites import get_all_test_cases, get_available_suites
from suites.base import BenchmarkSuiteResult, InferenceParams, TestCase, TestResult


DEFAULT_CONFIG_FILE = os.path.join(current_dir, "config.json")
EXAMPLE_CONFIG_FILE = os.path.join(current_dir, "config.example.json")


def load_config(config_path: str) -> Dict[str, Any]:
    """Loads configuration file or falls back to example config."""
    target = config_path
    if not os.path.exists(target):
        if os.path.exists(EXAMPLE_CONFIG_FILE):
            print(f"⚠️  Fichier {config_path} introuvable. Utilisation de {EXAMPLE_CONFIG_FILE}")
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

    client = LlmClient(
        endpoint=endpoint,
        model=model_id,
        api_key=api_key,
        api_key_env=api_key_env,
        timeout=timeout,
        mock_mode=mock_mode,
    )

    print(f"\n🚀 Lancement du benchmark pour : \033[1m{display_name}\033[0m")
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

        # Allow test case to override params if strictly required, otherwise use profile params
        effective_params = tc.recommended_params or params

        try:
            response_text, ttft_ms, total_latency_ms, tokens_count, tokens_per_sec = client.generate(
                messages=tc.messages,
                params=effective_params,
            )

            # Evaluate assertions deterministically
            assertions = tc.evaluator(response_text)

            # Overall test score = average of assertion scores * 100
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
            )

            if all_passed:
                print(f"\033[32mPASS ({avg_score:.0f}%)\033[0m [TTFT: {ttft_ms:.0f}ms, {tokens_per_sec:.1f} tps]")
            elif avg_score > 0:
                print(f"\033[33mPARTIEL ({avg_score:.0f}%)\033[0m [TTFT: {ttft_ms:.0f}ms]")
            else:
                print(f"\033[31mFAIL (0%)\033[0m [TTFT: {ttft_ms:.0f}ms]")

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
                error=str(e),
            )

        suite_result.test_results.append(test_res)

        if tc.category not in category_scores_map:
            category_scores_map[tc.category] = []
        category_scores_map[tc.category].append(test_res.score)

    # Compute overall score and averages
    if suite_result.test_results:
        suite_result.overall_score = sum(t.score for t in suite_result.test_results) / len(suite_result.test_results)
        valid_ttfts = [t.ttft_ms for t in suite_result.test_results if t.ttft_ms > 0]
        valid_tps = [t.tokens_per_sec for t in suite_result.test_results if t.tokens_per_sec > 0]
        suite_result.avg_ttft_ms = (sum(valid_ttfts) / len(valid_ttfts)) if valid_ttfts else 0.0
        suite_result.avg_tokens_per_sec = (sum(valid_tps) / len(valid_tps)) if valid_tps else 0.0

    for cat, scores in category_scores_map.items():
        suite_result.category_scores[cat] = sum(scores) / len(scores)

    print(
        f"   ⭐ Score Global : \033[1;35m{suite_result.overall_score:.1f} / 100\033[0m "
        f"(TTFT moy: {suite_result.avg_ttft_ms:.0f}ms, TPS: {suite_result.avg_tokens_per_sec:.1f})"
    )

    return suite_result


def main():
    parser = argparse.ArgumentParser(
        description="Jeanne LLM Benchmark - Banc de test déterministe pour modèles de langage."
    )
    parser.add_argument(
        "--config",
        "-c",
        default=DEFAULT_CONFIG_FILE,
        help="Chemin vers le fichier de configuration JSON (défaut: config.json)",
    )
    parser.add_argument(
        "--models",
        "-m",
        help="Liste d'identifiants de modèles à tester, séparés par des virgules (ex: qwen2.5:3b,llama3.2:3b)",
    )
    parser.add_argument(
        "--profiles",
        "-p",
        help="Liste de profils d'inférence à tester, séparés par des virgules (ex: deterministic_strict,balanced)",
    )
    parser.add_argument(
        "--suites",
        "-s",
        help="Suites à exécuter (rag, pii, structured, meeting, conciseness) séparées par des virgules",
    )
    parser.add_argument(
        "--output-dir",
        "-o",
        default=os.path.join(current_dir, "reports"),
        help="Répertoire de sortie des rapports (défaut: reports/)",
    )
    parser.add_argument(
        "--mock",
        action="store_true",
        help="Mode simulation déterministe (aucun serveur LLM actif requis)",
    )
    parser.add_argument(
        "--verbose",
        "-v",
        action="store_true",
        help="Affichage détaillé des assertions en console",
    )

    args = parser.parse_args()

    # 1. Charger la configuration
    try:
        config = load_config(args.config)
    except Exception as e:
        print(f"❌ Erreur de chargement de la configuration: {e}")
        sys.exit(1)

    hardware_profile = config.get("hardware_profile", "PC Personnel (profil non spécifié)")
    inference_profiles_dict = config.get("inference_profiles", {})

    # Default fallback profile if none defined
    if not inference_profiles_dict:
        inference_profiles_dict = {
            "deterministic_strict": {
                "temperature": 0.0,
                "seed": 42,
                "top_p": 1.0,
                "max_tokens": 1024,
            }
        }

    # 2. Filtrer les suites de tests
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

    # 3. Filtrer les modèles
    all_models = config.get("models", [])
    if args.models:
        selected_model_ids = [m.strip() for m in args.models.split(",")]
        models_to_test = [m for m in all_models if m["id"] in selected_model_ids]
    else:
        models_to_test = all_models

    if not models_to_test:
        print("❌ Aucun modèle à tester dans la configuration.")
        sys.exit(1)

    # 4. Filtrer les profils
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

    # 5. Exécution de la matrice (Modèle x Profils)
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

    # 6. Génération des rapports déterministes
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


if __name__ == "__main__":
    main()
