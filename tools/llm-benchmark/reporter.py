"""Deterministic report generator creating GitHub-flavored Markdown and machine-readable JSON."""

import json
import os
from datetime import datetime
from typing import Any, Dict, List
from suites.base import BenchmarkSuiteResult, TestResult


def format_param_report(param_key: str, val: Any) -> str:
    """Formate une valeur d'hyperparamètre, en annotant explicitement les valeurs par défaut avec '(par défaut)'."""
    if val is None:
        if param_key == "temperature":
            return "0.8 (par défaut)"
        elif param_key == "top_p":
            return "0.95 (par défaut)"
        elif param_key == "seed":
            return "non fixé (par défaut)"
        elif param_key in ("frequency_penalty", "presence_penalty"):
            return "0.0 (par défaut)"
        return "défaut (par défaut)"
    if isinstance(val, float):
        return f"{val}"
    return str(val)


def compute_verdict(suite: BenchmarkSuiteResult) -> str:
    """Computes a hardware & project suitability verdict based on Jeanne's criteria."""
    score = suite.overall_score
    tps = suite.avg_tokens_per_sec
    ttft = suite.avg_ttft_ms
    total_tokens = suite.total_tokens_generated
    think_tokens = suite.total_thinking_tokens

    # Surcharge de tokens de réflexion (e.g. DeepSeek-R1, QwQ non calibrés) :
    # Non viable pour Jeanne car sature la fenêtre KV (4096 tokens max) et détruit la réactivité de la palette (< 50ms)
    if think_tokens >= 250 or (total_tokens > 0 and (think_tokens / total_tokens) >= 0.25):
        return f"⚠️ **Non viable pour Jeanne** (Surcharge de réflexion : {think_tokens} tokens de pensée, risque de saturation KV)"

    # Alerte hyper-verbosité sur des requêtes courtes
    if total_tokens > 2000:
        return f"⚠️ **Non recommandé** (Hyper-verbeux : {total_tokens} tokens générés)"

    if score >= 90.0 and tps >= 20.0 and ttft <= 350.0:
        return "🟢 **Excellent** (Idéal pour Jeanne)"
    elif score >= 80.0 and tps >= 15.0 and ttft <= 600.0:
        return "🟡 **Acceptable** (Performances viables)"
    elif score >= 70.0:
        return "🟠 **Mitigé** (Quelques hallucinations ou lenteurs)"
    else:
        return "🔴 **Non Recommandé** (Échec fonctionnel ou trop lent)"


class BenchmarkReporter:
    """Generates deterministic benchmark reports in Markdown and JSON."""

    def __init__(
        self,
        hardware_profile: str,
        suites_results: List[BenchmarkSuiteResult],
        output_dir: str = "reports",
    ):
        self.hardware_profile = hardware_profile
        self.suites_results = suites_results
        self.output_dir = output_dir
        os.makedirs(self.output_dir, exist_ok=True)

    def generate(self, run_tag: str = "") -> Dict[str, str]:
        """Generates both Markdown and JSON reports and writes them to disk."""
        now = datetime.now()
        timestamp_str = now.strftime("%Y-%m-%d_%H%M%S")
        suffix = f"_{run_tag}" if run_tag else ""

        md_filename = f"benchmark_report_{timestamp_str}{suffix}.md"
        json_filename = f"benchmark_report_{timestamp_str}{suffix}.json"

        md_path = os.path.join(self.output_dir, md_filename)
        json_path = os.path.join(self.output_dir, json_filename)

        md_content = self._build_markdown(now)
        with open(md_path, "w", encoding="utf-8") as f:
            f.write(md_content)

        json_data = self._build_json(now)
        with open(json_path, "w", encoding="utf-8") as f:
            json.dump(json_data, f, indent=2, ensure_ascii=False)

        return {"markdown": md_path, "json": json_path}

    def _build_markdown(self, run_time: datetime) -> str:
        lines = []
        lines.append("# 📊 Jeanne LLM Benchmark Report")
        lines.append("")
        lines.append(f"- **Date d'exécution** : {run_time.strftime('%Y-%m-%d %H:%M:%S')}")
        lines.append(f"- **Profil Matériel Hôte** : `{self.hardware_profile}`")
        lines.append(f"- **Nombre de configurations testées** : {len(self.suites_results)}")
        lines.append("- **Objectif** : Mesurer l'adéquation fonctionnelle et matérielle pour l'écosystème Jeanne (File-over-App, RAG hybride, PII, Actions Palette, Réunions).")
        lines.append("")

        # 1. Tableau des Paramètres d'Inférence (Reproductibilité)
        lines.append("## ⚙️ Paramètres d'Inférence Utilisés (Reproductibilité)")
        lines.append("")
        lines.append("| Profil | Temperature | Seed | Top-P | Max Tokens | Penalties (Freq / Pres) |")
        lines.append("| :--- | :---: | :---: | :---: | :---: | :---: |")

        # Collect unique profiles
        seen_profiles = set()
        for suite in self.suites_results:
            p_name = suite.profile_name
            if p_name not in seen_profiles:
                seen_profiles.add(p_name)
                p = suite.inference_params
                temp = format_param_report("temperature", p.get("temperature"))
                seed = format_param_report("seed", p.get("seed"))
                top_p = format_param_report("top_p", p.get("top_p"))
                max_t = p.get("max_tokens", 1024)
                freq = format_param_report("frequency_penalty", p.get("frequency_penalty", 0.0))
                pres = format_param_report("presence_penalty", p.get("presence_penalty", 0.0))
                lines.append(f"| **`{p_name}`** | `{temp}` | `{seed}` | `{top_p}` | `{max_t}` | `{freq} / {pres}` |")
        lines.append("")

        # 2. Tableau Récapitulatif Global
        lines.append("## 🏆 Classement & Adéquation Jeanne")
        lines.append("")
        lines.append("| Modèle | Profil | Score Global | Durée Tot. | Tokens (Pensée) | TTFT Moy. | Débit TPS | RAG | PII | JSON | Réunion | Concision | /corrige | Palette IA | Verdict Matériel |")
        lines.append("| :--- | :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :--- |")

        # Sort suites by overall score descending
        sorted_suites = sorted(self.suites_results, key=lambda s: s.overall_score, reverse=True)

        for suite in sorted_suites:
            rag_sc = f"{suite.category_scores.get('RAG & Knowledge', 0.0):.0f}%"
            pii_sc = f"{suite.category_scores.get('PII & Privacy', 0.0):.0f}%"
            json_sc = f"{suite.category_scores.get('Structured Output', 0.0):.0f}%"
            meet_sc = f"{suite.category_scores.get('Meeting Assistant', 0.0):.0f}%"
            conc_sc = f"{suite.category_scores.get('Conciseness & KV Limit', 0.0):.0f}%"
            corr_sc = f"{suite.category_scores.get('Relecture & Correction (/corrige)', 0.0):.0f}%"
            pal_sc = f"{suite.category_scores.get('Actions Palette IA (/rephrase, /tldr, /trad)', 0.0):.0f}%"
            verdict = compute_verdict(suite)

            dur_str = f"{suite.total_duration_sec:.1f} s"
            tok_str = (
                f"{suite.total_tokens_generated} ({suite.total_thinking_tokens})"
                if suite.total_thinking_tokens > 0
                else f"{suite.total_tokens_generated}"
            )

            lines.append(
                f"| **{suite.model_display_name}** | `{suite.profile_name}` | **{suite.overall_score:.1f} / 100** | "
                f"{dur_str} | {tok_str} | {suite.avg_ttft_ms:.0f} ms | {suite.avg_tokens_per_sec:.1f} tps | "
                f"{rag_sc} | {pii_sc} | {json_sc} | {meet_sc} | {conc_sc} | {corr_sc} | {pal_sc} | {verdict} |"
            )
        lines.append("")

        # 3. Détail par Modèle
        lines.append("## 🔍 Détails par Modèle et Assertions")
        lines.append("")

        for suite in sorted_suites:
            lines.append(f"<details>")
            lines.append(f"<summary><b>{suite.model_display_name}</b> (Profil: <code>{suite.profile_name}</code>) - Score : <b>{suite.overall_score:.1f}%</b></summary>")
            lines.append("")
            lines.append(f"- **ID Modèle** : `{suite.model_id}`")
            p = suite.inference_params
            p_desc = (
                f"temperature: {format_param_report('temperature', p.get('temperature'))}, "
                f"seed: {format_param_report('seed', p.get('seed'))}, "
                f"top_p: {format_param_report('top_p', p.get('top_p'))}, "
                f"max_tokens: {p.get('max_tokens', 1024)}"
            )
            lines.append(f"- **Paramètres d'inférence** : `{p_desc}`")
            lines.append(f"- **Durée totale cumulée** : `{suite.total_duration_sec:.2f} s`")
            think_info = f" (dont `{suite.total_thinking_tokens}` tokens de réflexion)" if suite.total_thinking_tokens > 0 else ""
            lines.append(f"- **Volume total généré** : `{suite.total_tokens_generated} tokens`{think_info}")
            lines.append(f"- **Latence Premier Token (TTFT)** : `{suite.avg_ttft_ms:.1f} ms`")
            lines.append(f"- **Débit moyen** : `{suite.avg_tokens_per_sec:.1f} tokens/s`")
            lines.append("")
            lines.append("### Résultats des Tests :")
            lines.append("")
            lines.append("| ID Test | Nom du Test | Catégorie | Score | Durée | Tokens (Pensée) | TTFT | Débit | Statut |")
            lines.append("| :--- | :--- | :--- | :---: | :---: | :---: | :---: | :---: | :---: |")

            for t in suite.test_results:
                badge = "✅ Réussi" if t.passed else ("⚠️ Partiel" if t.score > 0 else "❌ Échec")
                dur_test = f"{t.total_latency_ms / 1000.0:.2f} s"
                tok_test = f"{t.tokens_generated} ({t.thinking_tokens})" if t.thinking_tokens > 0 else f"{t.tokens_generated}"
                lines.append(
                    f"| `{t.test_id}` | {t.test_name} | {t.category} | {t.score:.0f}% | {dur_test} | {tok_test} | {t.ttft_ms:.0f} ms | {t.tokens_per_sec:.1f} tps | {badge} |"
                )

            lines.append("")
            lines.append("#### Assertions et Réponses :")
            lines.append("")

            for t in suite.test_results:
                lines.append(f"##### `{t.test_id}` : {t.test_name}")
                if t.error:
                    lines.append(f"- 🚨 **Erreur d'exécution** : `{t.error}`")
                    engine_label = t.engine or "auto"
                    target_label = t.endpoint or "inconnu"
                    lines.append(f"- 🎯 **Contexte d'exécution** : Moteur `{engine_label}` | Cible `{target_label}`")
                    if t.error_traceback:
                        lines.append("- <details><summary>Traceback complet d'erreur</summary>\n")
                        lines.append("```python")
                        lines.append(t.error_traceback.strip())
                        lines.append("```\n</details>")
                else:
                    lines.append(f"- **Assertions Vérifiées** :")
                    for ass in t.assertions:
                        check_icon = "✔️" if ass.passed else "❌"
                        lines.append(f"  - {check_icon} **{ass.name}** (Score: {ass.score*100:.0f}%) : _{ass.details}_")

                if t.messages:
                    lines.append("- <details><summary>Prompt & Messages envoyés</summary>\n")
                    lines.append("```json")
                    lines.append(json.dumps(t.messages, indent=2, ensure_ascii=False))
                    lines.append("```\n</details>")

                lines.append("")
                lines.append("```text")
                clean_preview = t.response_text.strip()
                if len(clean_preview) > 500:
                    clean_preview = clean_preview[:500] + "... [tronqué pour lisibilité]"
                lines.append(clean_preview if clean_preview else "(Réponse vide)")
                lines.append("```")
                lines.append("")

            lines.append("</details>")
            lines.append("")

        lines.append("---")
        lines.append("*Rapport généré automatiquement par l'outil indépendant de benchmark de Jeanne.*")
        return "\n".join(lines)

    def _build_json(self, run_time: datetime) -> Dict[str, Any]:
        data = {
            "version": "1.0.0",
            "timestamp": run_time.isoformat(),
            "hardware_profile": self.hardware_profile,
            "suites": [],
        }

        for s in self.suites_results:
            suite_dict = {
                "model_id": s.model_id,
                "model_display_name": s.model_display_name,
                "profile_name": s.profile_name,
                "inference_params": s.inference_params,
                "resolved_params": {
                    "temperature": format_param_report("temperature", s.inference_params.get("temperature")),
                    "seed": format_param_report("seed", s.inference_params.get("seed")),
                    "top_p": format_param_report("top_p", s.inference_params.get("top_p")),
                    "max_tokens": s.inference_params.get("max_tokens", 1024),
                    "frequency_penalty": format_param_report("frequency_penalty", s.inference_params.get("frequency_penalty", 0.0)),
                    "presence_penalty": format_param_report("presence_penalty", s.inference_params.get("presence_penalty", 0.0)),
                },
                "overall_score": round(s.overall_score, 2),
                "total_duration_sec": round(s.total_duration_sec, 2),
                "total_tokens_generated": s.total_tokens_generated,
                "total_thinking_tokens": s.total_thinking_tokens,
                "avg_ttft_ms": round(s.avg_ttft_ms, 2),
                "avg_tokens_per_sec": round(s.avg_tokens_per_sec, 2),
                "category_scores": s.category_scores,
                "tests": [],
            }

            for t in s.test_results:
                suite_dict["tests"].append(
                    {
                        "test_id": t.test_id,
                        "test_name": t.test_name,
                        "category": t.category,
                        "passed": t.passed,
                        "score": round(t.score, 2),
                        "duration_sec": round(t.total_latency_ms / 1000.0, 3),
                        "total_latency_ms": round(t.total_latency_ms, 2),
                        "tokens_generated": t.tokens_generated,
                        "thinking_tokens": t.thinking_tokens,
                        "content_tokens": max(0, t.tokens_generated - t.thinking_tokens),
                        "ttft_ms": round(t.ttft_ms, 2),
                        "tokens_per_sec": round(t.tokens_per_sec, 2),
                        "response_text": t.response_text,
                        "error": t.error,
                        "error_traceback": t.error_traceback,
                        "engine": t.engine,
                        "endpoint": t.endpoint,
                        "messages": t.messages,
                        "assertions": [
                            {
                                "name": a.name,
                                "passed": a.passed,
                                "score": round(a.score, 2),
                                "details": a.details,
                            }
                            for a in t.assertions
                        ],
                    }
                )

            data["suites"].append(suite_dict)

        return data
