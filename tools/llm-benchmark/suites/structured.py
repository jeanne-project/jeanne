"""Test cases for Structured JSON generation & Quick Palette action parsing (Milestones 1 & 4b)."""

from typing import List
from .base import (
    AssertionResult,
    TestCase,
    check_json_strict,
)


def get_structured_test_cases() -> List[TestCase]:
    cases = []

    # Case 1: Palette Action Dispatch (Strict JSON)
    def eval_struct_01(response: str) -> List[AssertionResult]:
        res = []
        # Check strict JSON and schema
        json_check = check_json_strict(
            response,
            required_keys=["action", "title", "priority", "due_date"],
            allowed_values={"priority": ["low", "normal", "high", "urgent"]},
        )
        res.append(json_check)

        # Check for conversational fluff (must be avoided)
        first_line = response.strip().split("\n")[0].strip()
        has_chitchat = any(
            phrase in first_line.lower()
            for phrase in ["voici", "here is", "certainement", "bien sûr", "sure"]
        )
        res.append(
            AssertionResult(
                name="Zero Chitchat / Pure Data",
                passed=not has_chitchat,
                score=1.0 if not has_chitchat else 0.0,
                details="Output was pure JSON without conversational introduction"
                if not has_chitchat
                else "Model included polite conversational filler before or after the JSON.",
            )
        )
        return res

    cases.append(
        TestCase(
            id="STRUCT-01-PALETTE-ACTION",
            name="Palette Quick Action JSON Dispatch",
            category="Structured Output",
            description="Verifies that the LLM transforms a natural language prompt into a strict JSON payload without chat banter.",
            messages=[
                {
                    "role": "system",
                    "content": (
                        "Tu es le parseur d'action de la palette rapide de Jeanne. "
                        "Transforme la commande utilisateur en un objet JSON STRICT respectant ce schéma :\n"
                        "{\n"
                        '  "action": "create_task" | "create_note" | "search",\n'
                        '  "title": string,\n'
                        '  "priority": "low" | "normal" | "high" | "urgent",\n'
                        '  "due_date": string (format YYYY-MM-DD ou "none")\n'
                        "}\n"
                        "RÉPONDS UNIQUEMENT AVEC LE JSON SANS AUCUN TEXTE AUTOUR."
                    ),
                },
                {
                    "role": "user",
                    "content": "Rappelle-moi urgemment de réviser les index fts5 le 2026-10-15 pour la release.",
                },
            ],
            evaluator=eval_struct_01,
        )
    )

    # Case 2: Document Frontmatter Metadata Extraction
    def eval_struct_02(response: str) -> List[AssertionResult]:
        return [
            check_json_strict(
                response,
                required_keys=["category", "tags", "summary"],
                allowed_values={"category": ["Projets", "Ressources", "Archives", "Casquettes"]},
            )
        ]

    cases.append(
        TestCase(
            id="STRUCT-02-VAULT-METADATA",
            name="Vault Category & Tags Metadata JSON",
            category="Structured Output",
            description="Verifies classification into Jeanne's 4-Memory Stratification categories.",
            messages=[
                {
                    "role": "system",
                    "content": (
                        "Tu es le classificateur de coffre Jeanne. "
                        "Classe la note fournie selon la taxonomie officielle et retourne un JSON strict :\n"
                        "{\n"
                        '  "category": "Projets" | "Ressources" | "Archives" | "Casquettes",\n'
                        '  "tags": [string],\n'
                        '  "summary": string\n'
                        "}\n"
                        "Ne réponds qu'avec le JSON."
                    ),
                },
                {
                    "role": "user",
                    "content": (
                        "Spécification technique de l'algorithme BM25 avec k1=1.2 et b=0.75 "
                        "pour la recherche plein texte sur les fichiers Markdown du coffre."
                    ),
                },
            ],
            evaluator=eval_struct_02,
        )
    )

    return cases
