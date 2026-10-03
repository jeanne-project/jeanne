"""Test cases for Conciseness, Instruction following and Bounded KV Context limits (Milestone 4)."""

from typing import List
from .base import (
    AssertionResult,
    TestCase,
    check_word_count,
)

# A dense context simulating several notes injected into the prompt
DENSE_CONTEXT = """
# Fiche Synthétique : Module Hardware Jeanne
Le module hardware détecte automatiquement la présence d'un GPU compatible Vulkan.
Sur les architectures APU AMD Ryzen avec Radeon 780M, la mémoire vive allouée dynamiquement
peut atteindre jusqu'à la moitié de la mémoire physique totale, soit 8 Go sur une machine de 16 Go.
La couche d'isolation single-tenant utilise un verrou d'accès exclusif (Mutex) qui interdit toute allocation concurrente.
Le contexte KV par défaut est strictement borné à 4096 tokens pour éviter toute dérive OOM.
Lorsqu'un déchargement est déclenché via l'action unload, le buffer mmap est détaché et la commande
munmap libère les pages physiques de la RAM en moins de 2.0 secondes.
La télémétrie locale mesure le débit TPS et le Time-to-first-token en temps réel.
"""


def get_conciseness_test_cases() -> List[TestCase]:
    cases = []

    def eval_concise_01(response: str) -> List[AssertionResult]:
        res = []
        # Word count check: must not exceed 40 words
        res.append(check_word_count(response, max_words=40))

        # Content relevance check: must mention 4096 tokens limit
        lower = response.lower()
        has_limit = "4096" in lower or "4 096" in lower
        res.append(
            AssertionResult(
                name="Target Fact Accuracy",
                passed=has_limit,
                score=1.0 if has_limit else 0.0,
                details="Accurately stated the 4096 KV limit"
                if has_limit
                else "Did not mention the exact 4096 limit.",
            )
        )
        return res

    cases.append(
        TestCase(
            id="CTX-01-STRESS-CONCISE",
            name="Dense Context & Strict Word Ceiling",
            category="Conciseness & KV Limit",
            description="Tests the model's ability to remain strictly concise (< 40 words) despite dense context.",
            messages=[
                {
                    "role": "system",
                    "content": (
                        "Tu es l'assistant de Jeanne. Réponds à la question en te basant sur le contexte. "
                        "CONSIGNE STRICTE : Ta réponse doit faire MOINS DE 40 MOTS au total. "
                        "Sois direct, précis et sans fioritures."
                    ),
                },
                {
                    "role": "user",
                    "content": (
                        f"Contexte :\n{DENSE_CONTEXT}\n\n"
                        "Question: Quelle est la limite stricte de la fenêtre de contexte KV et pourquoi ?"
                    ),
                },
            ],
            evaluator=eval_concise_01,
        )
    )

    return cases
