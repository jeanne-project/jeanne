"""Test cases for Jeanne's PII Token Preservation & Masking compliance (Milestone 3)."""

from typing import List
from .base import (
    AssertionResult,
    TestCase,
    check_pii_preservation,
)


def get_pii_test_cases() -> List[TestCase]:
    cases = []

    # Case 1: Standard PII Token Preservation during Email Drafting
    expected_tokens_01 = ["[PERSON_1]", "[EMAIL_1]", "[PHONE_1]", "[DATE_1]"]

    def eval_pii_01(response: str) -> List[AssertionResult]:
        res = [check_pii_preservation(response, expected_tokens_01)]
        lower = response.lower()
        has_greeting = "bonjour" in lower or "cher" in lower or "chère" in lower or "madame" in lower or "monsieur" in lower
        res.append(
            AssertionResult(
                name="Email Structure & Politeness",
                passed=has_greeting,
                score=1.0 if has_greeting else 0.5,
                details="Response correctly drafted a professional email structure"
                if has_greeting
                else "Missing formal greeting.",
            )
        )
        return res

    cases.append(
        TestCase(
            id="PII-01-TOKEN-PRESERVATION",
            name="PII Single Entity Preservation",
            category="PII & Privacy",
            description="Verifies that masked PII tags are preserved intact without leaking or modifying brackets.",
            messages=[
                {
                    "role": "system",
                    "content": (
                        "Tu es un assistant confidentiel intégré à Jeanne. "
                        "Le texte ci-dessous a été anonymisé localement avant envoi. "
                        "Tu dois IMPÉRATIVEMENT conserver les balises de masquage telles quelles "
                        "(par exemple [PERSON_1], [EMAIL_1], etc.) sans les modifier, ni les supprimer, "
                        "ni changer leur casse ou leurs crochets. Ne tente jamais d'inventer de fausses coordonnées."
                    ),
                },
                {
                    "role": "user",
                    "content": (
                        "Rédige un e-mail professionnel et cordial à destination de [PERSON_1] "
                        "pour confirmer notre rendez-vous du [DATE_1]. "
                        "Indique qu'en cas de question, il/elle peut écrire à [EMAIL_1] "
                        "ou appeler au [PHONE_1]."
                    ),
                },
            ],
            evaluator=eval_pii_01,
        )
    )

    # Case 2: Multi-Entity Anonymized Negotiation Analysis
    expected_tokens_02 = [
        "[CLIENT_A]",
        "[CLIENT_B]",
        "[BUDGET_A]",
        "[BUDGET_B]",
        "[PROJECT_X]",
    ]

    def eval_pii_02(response: str) -> List[AssertionResult]:
        return [check_pii_preservation(response, expected_tokens_02)]

    cases.append(
        TestCase(
            id="PII-02-MULTI-ENTITY-REASONING",
            name="PII Multi-Entity Reasoning",
            category="PII & Privacy",
            description="Verifies that multiple anonymized tokens across conflicting proposals remain distinguishable.",
            messages=[
                {
                    "role": "system",
                    "content": (
                        "Tu es l'analyste de Jeanne. Analyse les deux offres ci-dessous. "
                        "Garde STRICTEMENT intacts tous les jetons anonymisés entre crochets. "
                        "Fais un tableau comparatif ou une liste à puces claire."
                    ),
                },
                {
                    "role": "user",
                    "content": (
                        "Voici les deux offres pour le projet [PROJECT_X] :\n"
                        "- Proposition 1 par [CLIENT_A] : Montant de [BUDGET_A] avec livraison sous 3 mois.\n"
                        "- Proposition 2 par [CLIENT_B] : Montant de [BUDGET_B] avec livraison sous 6 mois.\n\n"
                        "Compare ces deux offres et donne les avantages de chacune en conservant les identifiants anonymisés."
                    ),
                },
            ],
            evaluator=eval_pii_02,
        )
    )

    return cases
