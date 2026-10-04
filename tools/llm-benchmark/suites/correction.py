"""Test cases for Jeanne's /corrige clipboard assistant (spelling, grammar, and syntax correction)."""

import re
from typing import List
from .base import AssertionResult, TestCase


SYSTEM_CORRIGE_PROMPT = (
    "Tu es un relecteur professionnel. Corrige l'orthographe, la grammaire, la syntaxe et la ponctuation "
    "du texte ci-dessous. Conserve le ton et le format exacts. "
    "Renvoie UNIQUEMENT le texte corrigé, sans salutation ni explication ni guillemets."
)


def check_no_chitchat(response: str) -> AssertionResult:
    """Verifies that the LLM did not add conversational framing (critical for clipboard replacement)."""
    cleaned = response.strip()
    first_line = cleaned.split("\n")[0].strip().lower()

    banned_prefixes = [
        "voici",
        "here is",
        "certainement",
        "bien sûr",
        "texte corrigé",
        "correction :",
        "correction:",
        "j'ai corrigé",
        "après correction",
        "voilà",
    ]

    for prefix in banned_prefixes:
        if first_line.startswith(prefix) or first_line == prefix:
            return AssertionResult(
                name="Zero Chitchat / Pure Text",
                passed=False,
                score=0.0,
                details=f"Response includes conversational framing: '{first_line[:40]}...'",
            )

    return AssertionResult(
        name="Zero Chitchat / Pure Text",
        passed=True,
        score=1.0,
        details="Output is pure corrected text without conversational fluff.",
    )


def check_correction_terms(
    response: str,
    required_terms: List[str],
    forbidden_errors: List[str],
) -> AssertionResult:
    """Verifies that errors were fixed and corrected target words are present."""
    lower_resp = response.lower()

    missing_corrections = [t for t in required_terms if t.lower() not in lower_resp]
    unfixed_errors = []
    for err in forbidden_errors:
        pattern = rf"(?<!\w){re.escape(err.lower())}(?!\w)"
        if re.search(pattern, lower_resp):
            unfixed_errors.append(err)

    total_checks = len(required_terms) + len(forbidden_errors)
    passed_checks = (len(required_terms) - len(missing_corrections)) + (len(forbidden_errors) - len(unfixed_errors))
    score = passed_checks / max(1, total_checks)

    if not missing_corrections and not unfixed_errors:
        return AssertionResult(
            name="Error Remediation Accuracy",
            passed=True,
            score=1.0,
            details=f"All {len(required_terms)} corrections verified. Zero original errors retained.",
        )

    details_parts = []
    if missing_corrections:
        details_parts.append(f"Mots corrigés manquants : {missing_corrections}")
    if unfixed_errors:
        details_parts.append(f"Fautes non corrigées : {unfixed_errors}")

    return AssertionResult(
        name="Error Remediation Accuracy",
        passed=False,
        score=max(0.0, score),
        details="; ".join(details_parts),
    )


def get_correction_test_cases() -> List[TestCase]:
    cases = []

    # ─────────────────────────────────────────────────────────────────────────
    # Case 1: Salutation et question courante (/corrige de base)
    # ─────────────────────────────────────────────────────────────────────────
    input_text_01 = "Bonjor, coment sa va ?"
    # Attendu : "Bonjour, comment ça va ?"

    def eval_corr_01(response: str) -> List[AssertionResult]:
        res = [check_no_chitchat(response)]
        res.append(
            check_correction_terms(
                response=response,
                required_terms=["bonjour", "comment", "ça va"],
                forbidden_errors=["bonjor", "coment", "sa va"],
            )
        )
        return res

    cases.append(
        TestCase(
            id="CORR-01-COURTOISIE-BASIQUE",
            name="/corrige Salutation et Orthographe Courante",
            category="Relecture & Correction (/corrige)",
            description="Vérifie la correction de fautes usuelles ('Bonjor, coment sa va ?' -> 'Bonjour, comment ça va ?') sans bavardage.",
            messages=[
                {"role": "system", "content": SYSTEM_CORRIGE_PROMPT},
                {"role": "user", "content": input_text_01},
            ],
            evaluator=eval_corr_01,
        )
    )

    # ─────────────────────────────────────────────────────────────────────────
    # Case 2: Accents, homophones et ponctuation
    # ─────────────────────────────────────────────────────────────────────────
    input_text_02 = "aparament sa marche pas, ou est le probleme ?"
    # Attendu : "Apparemment ça ne marche pas, où est le problème ?" ou "Apparemment ça marche pas, où est le problème ?"

    def eval_corr_02(response: str) -> List[AssertionResult]:
        res = [check_no_chitchat(response)]
        res.append(
            check_correction_terms(
                response=response,
                required_terms=["apparemment", "ça", "où", "problème"],
                forbidden_errors=["aparament", "ou est", "probleme"],
            )
        )
        return res

    cases.append(
        TestCase(
            id="CORR-02-ACCENTS-ET-HOMOPHONES",
            name="/corrige Accents et Homophones Grammaticaux",
            category="Relecture & Correction (/corrige)",
            description="Corrige les accents manquants et homophones ('aparament', 'sa', 'ou est', 'probleme').",
            messages=[
                {"role": "system", "content": SYSTEM_CORRIGE_PROMPT},
                {"role": "user", "content": input_text_02},
            ],
            evaluator=eval_corr_02,
        )
    )

    # ─────────────────────────────────────────────────────────────────────────
    # Case 3: Email professionnel, accords et participes passés
    # ─────────────────────────────────────────────────────────────────────────
    input_text_03 = "Je vous contact car nous avon reçut votre devis mes il y a des érreur de calcule."
    # Attendu : "Je vous contacte car nous avons reçu votre devis mais il y a des erreurs de calcul."

    def eval_corr_03(response: str) -> List[AssertionResult]:
        res = [check_no_chitchat(response)]
        res.append(
            check_correction_terms(
                response=response,
                required_terms=["contacte", "avons", "reçu", "mais", "erreurs", "calcul"],
                forbidden_errors=["contact car", "avon", "reçut", "mes il", "érreur", "calcule"],
            )
        )
        return res

    cases.append(
        TestCase(
            id="CORR-03-EMAIL-PRO-ACCORDS",
            name="/corrige Email Professionnel et Participes",
            category="Relecture & Correction (/corrige)",
            description="Corrige les accords complexes dans un contexte professionnel ('avon reçut', 'mes', 'érreur de calcule').",
            messages=[
                {"role": "system", "content": SYSTEM_CORRIGE_PROMPT},
                {"role": "user", "content": input_text_03},
            ],
            evaluator=eval_corr_03,
        )
    )

    # ─────────────────────────────────────────────────────────────────────────
    # Case 4: Termes techniques et syntaxe développeur
    # ─────────────────────────────────────────────────────────────────────────
    input_text_04 = "je suis developpeur et j'ai implementer une requete sqlite sans index fts5."
    # Attendu : Majuscule initiale, "développeur", "implémenté", "requête", préservation intacte de "sqlite" et "fts5"

    def eval_corr_04(response: str) -> List[AssertionResult]:
        res = [check_no_chitchat(response)]
        res.append(
            check_correction_terms(
                response=response,
                required_terms=["développeur", "implémenté", "requête", "sqlite", "fts5"],
                forbidden_errors=["developpeur", "implementer", "requete"],
            )
        )
        # Vérification majuscule initiale
        has_initial_cap = response.strip().startswith("Je ")
        res.append(
            AssertionResult(
                name="Initial Capitalization",
                passed=has_initial_cap,
                score=1.0 if has_initial_cap else 0.5,
                details="Sentence starts with capital 'Je'" if has_initial_cap else "Missing initial capital letter.",
            )
        )
        return res

    cases.append(
        TestCase(
            id="CORR-04-TEXTE-TECHNIQUE-DEVELOPPEUR",
            name="/corrige Préservation Technique et Participe Passé",
            category="Relecture & Correction (/corrige)",
            description="Corrige le participe passé et les accents tout en préservant le jargon technique (sqlite, fts5).",
            messages=[
                {"role": "system", "content": SYSTEM_CORRIGE_PROMPT},
                {"role": "user", "content": input_text_04},
            ],
            evaluator=eval_corr_04,
        )
    )

    return cases
