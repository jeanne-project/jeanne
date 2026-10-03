"""Test cases for Meeting diarization notes & Action checklist extraction (Milestones 5 & 6)."""

from typing import List
from .base import (
    AssertionResult,
    TestCase,
    check_markdown_checklists,
)

SAMPLE_MEETING_TRANSCRIPT = """
Alice [00:02]: Bonjour à tous. Pour le Jalon 4, nous devons valider les shaders Vulkan sur le laptop de test.
Bob [00:15]: J'ai testé hier soir, l'allocation VRAM reste sous 2.1 Go avec Qwen 2.5 3B Q4_K_M. Par contre le temps de déchargement dépasse parfois 2 secondes.
Alice [00:34]: D'accord. Bob, peux-tu optimiser la libération du buffer mmap d'ici vendredi ?
Bob [00:41]: Oui, je m'en occupe d'ici vendredi 17h.
Claire [00:48]: De mon côté, les tests d'intégration du Jalon 3 sur le masquage PII sont tous verts. Je prépare le rapport d'audit pour lundi.
Alice [01:05]: Parfait. Décision adoptée : on clôture la relecture du code ce soir et Claire soumet le rapport lundi.
"""


def get_meeting_test_cases() -> List[TestCase]:
    cases = []

    def eval_meeting_01(response: str) -> List[AssertionResult]:
        res = []
        # Checklist verification: must have at least 2 checkbox items
        res.append(check_markdown_checklists(response, min_items=2))

        # Structure verification: decisions & actions headers
        lower = response.lower()
        has_decisions = "décision" in lower or "decision" in lower
        has_actions = "action" in lower or "tâche" in lower or "todo" in lower
        res.append(
            AssertionResult(
                name="Meeting Sections Structure",
                passed=has_decisions and has_actions,
                score=1.0 if (has_decisions and has_actions) else 0.5,
                details="Included both Decisions and Actions sections"
                if (has_decisions and has_actions)
                else "Missing required section header (Decisions or Actions).",
            )
        )

        # Attribution check: names mentioned
        has_people = "bob" in lower and "claire" in lower
        res.append(
            AssertionResult(
                name="Action Item Attribution",
                passed=has_people,
                score=1.0 if has_people else 0.0,
                details="Action items correctly attributed to owners (Bob / Claire)"
                if has_people
                else "Failed to attribute tasks to Bob and Claire.",
            )
        )
        return res

    cases.append(
        TestCase(
            id="MEET-01-ACTION-EXTRACTION",
            name="Meeting Summary & Action Checklist",
            category="Meeting Assistant",
            description="Extracts structured decisions and assigned markdown task checkboxes from a raw audio transcript.",
            messages=[
                {
                    "role": "system",
                    "content": (
                        "Tu es l'assistant de réunion de Jeanne. À partir de la transcription brute, "
                        "génère un compte-rendu synthétique avec deux sections obligatoires :\n"
                        "### Décisions\n"
                        "(Liste des décisions prises)\n\n"
                        "### Actions à mener\n"
                        "(Chaque action doit impérativement être une case à cocher Markdown "
                        "avec l'assignation de la personne : - [ ] @Nom: action)"
                    ),
                },
                {
                    "role": "user",
                    "content": f"Voici la transcription de la réunion :\n{SAMPLE_MEETING_TRANSCRIPT}",
                },
            ],
            evaluator=eval_meeting_01,
        )
    )

    return cases
