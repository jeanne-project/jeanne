"""Test cases for Jeanne's Hybrid RAG & Knowledge Vault functionality (Milestones 2 & 3)."""

from typing import List
from .base import (
    AssertionResult,
    TestCase,
    check_citations,
    check_refusal_on_missing_info,
)


DOC_ARCHITECTURE = """# note_architecture_v1.md
## Principes Fondamentaux de Jeanne
Jeanne adopte une philosophie stricte "File-over-App".
La source de vérité absolue et immuable est le dossier de fichiers Markdown (.md) locaux.
La base de données SQLite (avec sqlite-vec et fts5) n'est qu'un cache d'indexation jetable et dérivé.
Le système est capable de reconstruire l'intégralité de la base de données à partir des fichiers du disque à tout moment sans aucune perte de données.
Le budget RAM résident en fonctionnement normal ne doit jamais dépasser 200 Mo.
"""

DOC_SECURITY = """# specs_securite_local.md
## Chiffrement et Confidentialité
Toutes les données sensibles et jetons d'API sont stockés dans le trousseau sécurisé de l'OS (keyring natif).
Avant tout appel à un modèle distant tiers, les identifiants personnels (adresses e-mail, téléphones) sont masqués localement par des balises déterministes.
Le modèle local 3B fonctionne en offline total via Vulkan compute shaders sans aucune télémétrie externe.
"""

DOC_PROJECT_ALPHA = """# projet_alpha_2026.md
## Statut du Projet Alpha
Le projet Alpha a franchi le jalon 4 le 2 octobre 2026.
L'objectif est d'atteindre un débit de 15 tokens par seconde sur iGPU Radeon 780M.
L'équipe est composée de 3 ingénieurs : Thomas, Sarah et Yassine.
Budget alloué : 45 000 euros.
"""


def get_rag_test_cases() -> List[TestCase]:
    cases = []

    # Case 1: Factoid with single source citation
    def eval_rag_01(response: str) -> List[AssertionResult]:
        res = []
        res.append(check_citations(response, required_sources=["note_architecture_v1.md"]))
        # Content verification: must mention file-over-app or markdown
        lower = response.lower()
        has_key_facts = "file-over-app" in lower or "markdown" in lower
        res.append(
            AssertionResult(
                name="Key Architectural Facts",
                passed=has_key_facts,
                score=1.0 if has_key_facts else 0.0,
                details="Response accurately identified the File-over-App philosophy"
                if has_key_facts
                else "Response failed to mention File-over-App or Markdown source of truth.",
            )
        )
        return res

    cases.append(
        TestCase(
            id="RAG-01-FAITHFUL-CITATION",
            name="RAG Single-Source Citation",
            category="RAG & Knowledge",
            description="Verifies that the LLM cites the exact document and relies strictly on provided context.",
            messages=[
                {
                    "role": "system",
                    "content": (
                        "Tu es l'assistant de Jeanne. Réponds à la question de l'utilisateur "
                        "en te basant STRICTEMENT sur les documents fournis ci-dessous. "
                        "Tu dois obligatoirement citer ta source sous la forme exacte: [source: nom_du_fichier.md]. "
                        "N'invente aucune information."
                    ),
                },
                {
                    "role": "user",
                    "content": (
                        f"Voici mes documents :\n\n{DOC_ARCHITECTURE}\n\n"
                        "Question: Quel est le rôle de la base de données SQLite dans Jeanne et quelle est la source de vérité ?"
                    ),
                },
            ],
            evaluator=eval_rag_01,
        )
    )

    # Case 2: Multi-source synthesis with multiple citations
    def eval_rag_02(response: str) -> List[AssertionResult]:
        res = []
        res.append(
            check_citations(
                response,
                required_sources=["note_architecture_v1.md", "specs_securite_local.md"],
            )
        )
        lower = response.lower()
        mentions_keyring = "keyring" in lower or "trousseau" in lower or "masqué" in lower or "vulkan" in lower
        res.append(
            AssertionResult(
                name="Cross-Document Integration",
                passed=mentions_keyring,
                score=1.0 if mentions_keyring else 0.0,
                details="Successfully synthesized security aspects with architecture"
                if mentions_keyring
                else "Failed to integrate security specifics from the second note.",
            )
        )
        return res

    cases.append(
        TestCase(
            id="RAG-02-MULTI-SOURCE",
            name="RAG Multi-Source Cross Citation",
            category="RAG & Knowledge",
            description="Verifies multi-document synthesis with multiple required citations.",
            messages=[
                {
                    "role": "system",
                    "content": (
                        "Tu es l'assistant de Jeanne. Réponds en synthétisant les documents ci-dessous. "
                        "Pour chaque affirmation, cite obligatoirement sa source sous le format [source: nom_du_fichier.md]."
                    ),
                },
                {
                    "role": "user",
                    "content": (
                        f"Documents :\n\n{DOC_ARCHITECTURE}\n\n{DOC_SECURITY}\n\n"
                        "Question: Comment Jeanne garantit-elle la confidentialité des données et où sont stockées les clés d'API ?"
                    ),
                },
            ],
            evaluator=eval_rag_02,
        )
    )

    # Case 3: Out-of-domain query (Must refuse cleanly without hallucination)
    def eval_rag_03(response: str) -> List[AssertionResult]:
        return [check_refusal_on_missing_info(response)]

    cases.append(
        TestCase(
            id="RAG-03-ANTI-HALLUCINATION",
            name="RAG Out-Of-Domain Refusal",
            category="RAG & Knowledge",
            description="Verifies that the LLM rejects answering questions when the information is missing from documents.",
            messages=[
                {
                    "role": "system",
                    "content": (
                        "Tu es l'assistant de Jeanne. Réponds EXCLUSIVEMENT à l'aide des documents fournis. "
                        "Si l'information n'est pas présente dans les documents, réponds impérativement et uniquement : "
                        "'Information non trouvée dans les documents fournis.' N'invente jamais de réponse."
                    ),
                },
                {
                    "role": "user",
                    "content": (
                        f"Documents :\n\n{DOC_ARCHITECTURE}\n\n{DOC_PROJECT_ALPHA}\n\n"
                        "Question: Quel est le nom du satellite lancé par l'agence spatiale européenne en 2024 ?"
                    ),
                },
            ],
            evaluator=eval_rag_03,
        )
    )

    return cases
