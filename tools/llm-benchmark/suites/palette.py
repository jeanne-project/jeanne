"""Test cases for Jeanne's Command Palette AI Productivity Actions (Milestone 04b).
Covers clipboard assistants: /rephrase, /tldr, /trad, /todo extraction, and /bookmark annotation.
Specification: docs/specs/04b_SPEC_PALETTE_PRODUCTIVITY_ACTIONS.md
"""

import re
from typing import List
from .base import AssertionResult, TestCase


CATEGORY_PALETTE = "Actions Palette IA (/rephrase, /tldr, /trad)"


def check_no_chitchat(response: str) -> AssertionResult:
    """Verifies that the LLM did not add conversational framing (critical for clipboard replacement)."""
    cleaned = response.strip()
    first_line = cleaned.split("\n")[0].strip().lower()

    banned_prefixes = [
        "voici",
        "here is",
        "certainement",
        "bien sûr",
        "sure",
        "texte reformulé",
        "reformulation :",
        "reformulation:",
        "résumé :",
        "résumé:",
        "traduction :",
        "traduction:",
        "tl;dr",
        "tldr",
        "en résumé",
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
        details="Output is pure generated text without conversational fluff.",
    )


def get_palette_test_cases() -> List[TestCase]:
    cases = []

    # ─────────────────────────────────────────────────────────────────────────
    # Case 1: /rephrase pro (ton professionnel et fluide)
    # ─────────────────────────────────────────────────────────────────────────
    input_text_01 = "salut dis moi tu peux m envoyer le doc stp c urgent on a un souci avec le client"

    def eval_rephrase_01(response: str) -> List[AssertionResult]:
        res = [check_no_chitchat(response)]
        resp_lower = response.lower()

        # Vérification de la disparition du langage familier/SMS
        banned_sms = ["salut", "stp", "dis moi", "c urgent"]
        retained_sms = [w for w in banned_sms if w in resp_lower]

        # Vérification de la présence des éléments professionnels essentiels
        has_doc = any(w in resp_lower for w in ["document", "dossier", "fichier", "doc"])
        has_client = "client" in resp_lower
        has_urgency = any(w in resp_lower for w in ["urgent", "rapidement", "dès que possible", "urgence"])

        score_content = (
            (1.0 if not retained_sms else 0.0) * 0.4
            + (1.0 if has_doc else 0.0) * 0.2
            + (1.0 if has_client else 0.0) * 0.2
            + (1.0 if has_urgency else 0.0) * 0.2
        )

        res.append(
            AssertionResult(
                name="Professional Style & Preservation",
                passed=score_content >= 0.8,
                score=score_content,
                details=(
                    f"SMS purgé: {len(retained_sms)==0}, Document mentionné: {has_doc}, "
                    f"Client mentionné: {has_client}, Urgence transmise: {has_urgency}"
                ),
            )
        )
        return res

    cases.append(
        TestCase(
            id="PALETTE-01-REPHRASE-PRO",
            name="/rephrase Ton Professionnel",
            category=CATEGORY_PALETTE,
            description="Reformulation d'un message SMS/familier en un style professionnel et fluide pour email.",
            messages=[
                {
                    "role": "system",
                    "content": (
                        "Reformule le texte ci-dessous avec un ton professionnel et fluide. "
                        "Sois clair et concis. Renvoie UNIQUEMENT le texte reformulé, sans commentaire ni salutation :\n\n"
                        f"{input_text_01}"
                    ),
                }
            ],
            evaluator=eval_rephrase_01,
        )
    )

    # ─────────────────────────────────────────────────────────────────────────
    # Case 2: /rephrase concis (ton concis et percutant)
    # ─────────────────────────────────────────────────────────────────────────
    input_text_02 = (
        "En fait je voulais juste vous faire part du fait que nous avons remarqué que potentiellement "
        "il pourrait y avoir une petite baisse de performance au niveau de la synchronisation de la "
        "base de données quand il y a beaucoup de requêtes simultanées en même temps."
    )

    def eval_rephrase_02(response: str) -> List[AssertionResult]:
        res = [check_no_chitchat(response)]
        words = response.strip().split()
        word_count = len(words)
        resp_lower = response.lower()

        # Doit être très concis (< 22 mots contre 45 mots en entrée)
        is_concise = word_count <= 22
        concise_score = 1.0 if is_concise else max(0.0, 1.0 - (word_count - 22) * 0.08)

        # Maintien du sens technique
        has_perf = any(w in resp_lower for w in ["performance", "ralentissement", "baisse"])
        has_sync_db = any(w in resp_lower for w in ["base de données", "bdd", "synchronisation", "requête"])

        meaning_score = (1.0 if has_perf else 0.0) * 0.5 + (1.0 if has_sync_db else 0.0) * 0.5
        total_score = concise_score * 0.5 + meaning_score * 0.5

        res.append(
            AssertionResult(
                name="Conciseness & Substance Retention",
                passed=total_score >= 0.8,
                score=total_score,
                details=f"Longueur: {word_count} mots (cible <= 22), Performance: {has_perf}, BDD/Sync: {has_sync_db}",
            )
        )
        return res

    cases.append(
        TestCase(
            id="PALETTE-02-REPHRASE-CONCISE",
            name="/rephrase Ton Concis & Percutant",
            category=CATEGORY_PALETTE,
            description="Élagage d'un paragraphe verbeux pour ne garder que la substance technique en moins de 22 mots.",
            messages=[
                {
                    "role": "system",
                    "content": (
                        "Reformule le texte ci-dessous avec un ton concis et percutant. "
                        "Sois clair et concis. Renvoie UNIQUEMENT le texte reformulé, sans commentaire ni salutation :\n\n"
                        f"{input_text_02}"
                    ),
                }
            ],
            evaluator=eval_rephrase_02,
        )
    )

    # ─────────────────────────────────────────────────────────────────────────
    # Case 3: /tldr (3 puces synthétiques concises)
    # ─────────────────────────────────────────────────────────────────────────
    input_text_03 = (
        "Le projet Jeanne repose sur le paradigme File-over-App, où les fichiers Markdown constituent "
        "la source de vérité immuable stockée localement sur le disque de l'utilisateur. La base SQLite "
        "avec sqlite-vec et fts5 n'est qu'un cache d'indexation jetable qui peut être reconstruit à tout moment "
        "à partir du dossier de notes. Pour garantir la fluidité sur des ordinateurs dotés de seulement 16 Go de RAM "
        "avec iGPU partagé, l'empreinte mémoire vive du cœur applicatif est strictement bornée à moins de 200 Mo hors modèle, "
        "avec un plafond de contexte KV de 4096 tokens."
    )

    def eval_tldr_03(response: str) -> List[AssertionResult]:
        res = [check_no_chitchat(response)]
        lines = [line.strip() for line in response.strip().split("\n") if line.strip()]

        bullet_lines = [l for l in lines if l.startswith("- ") or l.startswith("* ")]
        exact_3_bullets = len(bullet_lines) == 3
        format_score = 1.0 if exact_3_bullets else (0.5 if len(bullet_lines) in (2, 4) else 0.0)

        # Vérification du contenu des 3 piliers
        full_text = response.lower()
        has_file_over_app = any(w in full_text for w in ["markdown", "file-over-app", "source de vérité"])
        has_sqlite_cache = any(w in full_text for w in ["sqlite", "cache", "index"])
        has_memory_limit = any(w in full_text for w in ["200 mo", "ram", "mémoire", "4096"])

        content_score = (
            (1.0 if has_file_over_app else 0.0) * 0.34
            + (1.0 if has_sqlite_cache else 0.0) * 0.33
            + (1.0 if has_memory_limit else 0.0) * 0.33
        )

        total_score = format_score * 0.5 + content_score * 0.5

        res.append(
            AssertionResult(
                name="Strict 3-Bullet TL;DR Structure",
                passed=total_score >= 0.8,
                score=total_score,
                details=(
                    f"Puces détectées: {len(bullet_lines)}/3, Markdown/Vérité: {has_file_over_app}, "
                    f"SQLite Cache: {has_sqlite_cache}, Budget RAM: {has_memory_limit}"
                ),
            )
        )
        return res

    cases.append(
        TestCase(
            id="PALETTE-03-TLDR-BULLETS",
            name="/tldr Synthèse en 3 Puces Clés",
            category=CATEGORY_PALETTE,
            description="Génération stricte de 3 puces clés concises pour résumer un document technique dans le presse-papier.",
            messages=[
                {
                    "role": "system",
                    "content": (
                        "Résume le texte suivant sous forme de 3 puces clés concises commençant par un tiret (-). "
                        "Renvoie UNIQUEMENT les puces :\n\n"
                        f"{input_text_03}"
                    ),
                }
            ],
            evaluator=eval_tldr_03,
        )
    )

    # ─────────────────────────────────────────────────────────────────────────
    # Case 4: /trad anglais (Traduction technique FR -> EN)
    # ─────────────────────────────────────────────────────────────────────────
    input_text_04 = (
        "Le moteur d'inférence local s'exécute directement en mémoire sans serveur externe "
        "afin de préserver la confidentialité des notes."
    )

    def eval_trad_04(response: str) -> List[AssertionResult]:
        res = [check_no_chitchat(response)]
        resp_lower = response.lower()

        has_inference = any(w in resp_lower for w in ["inference", "engine"])
        has_memory = any(w in resp_lower for w in ["memory", "in-memory"])
        has_server = any(w in resp_lower for w in ["external server", "without an external", "without external"])
        has_privacy = any(w in resp_lower for w in ["privacy", "confidentiality", "notes"])

        # Pas de français résiduel majeur
        french_remnants = [w for w in ["le moteur", "s'exécute", "sans serveur", "afin de"] if w in resp_lower]

        score = (
            (1.0 if has_inference else 0.0) * 0.25
            + (1.0 if has_memory else 0.0) * 0.25
            + (1.0 if has_server else 0.0) * 0.25
            + (1.0 if has_privacy else 0.0) * 0.25
        )
        if french_remnants:
            score *= 0.5

        res.append(
            AssertionResult(
                name="Technical Translation Accuracy (FR->EN)",
                passed=score >= 0.75 and not french_remnants,
                score=score,
                details=f"Inference: {has_inference}, Memory: {has_memory}, Server: {has_server}, Privacy: {has_privacy}",
            )
        )
        return res

    cases.append(
        TestCase(
            id="PALETTE-04-TRAD-FR-TO-EN",
            name="/trad Français vers Anglais",
            category=CATEGORY_PALETTE,
            description="Traduction directe d'une phrase technique en anglais avec terminologie exacte et zéro blabla.",
            messages=[
                {
                    "role": "system",
                    "content": (
                        "Traduis fidèlement le texte suivant en anglais. "
                        "Renvoie UNIQUEMENT la traduction sans commentaire :\n\n"
                        f"{input_text_04}"
                    ),
                }
            ],
            evaluator=eval_trad_04,
        )
    )

    # ─────────────────────────────────────────────────────────────────────────
    # Case 5: /trad français (Traduction technique EN -> FR)
    # ─────────────────────────────────────────────────────────────────────────
    input_text_05 = (
        "The floating quick-access palette must open in less than 50 milliseconds to maintain a seamless user experience."
    )

    def eval_trad_05(response: str) -> List[AssertionResult]:
        res = [check_no_chitchat(response)]
        resp_lower = response.lower()

        has_palette = "palette" in resp_lower
        has_50ms = "50" in resp_lower and any(w in resp_lower for w in ["milliseconde", "ms"])
        has_ux = any(w in resp_lower for w in ["expérience utilisateur", "expérience", "fluide", "transparente"])

        score = (
            (1.0 if has_palette else 0.0) * 0.34
            + (1.0 if has_50ms else 0.0) * 0.33
            + (1.0 if has_ux else 0.0) * 0.33
        )

        res.append(
            AssertionResult(
                name="Technical Translation Accuracy (EN->FR)",
                passed=score >= 0.8,
                score=score,
                details=f"Palette: {has_palette}, 50ms: {has_50ms}, UX: {has_ux}",
            )
        )
        return res

    cases.append(
        TestCase(
            id="PALETTE-05-TRAD-EN-TO-FR",
            name="/trad Anglais vers Français",
            category=CATEGORY_PALETTE,
            description="Traduction directe d'une contrainte technique en français fluide.",
            messages=[
                {
                    "role": "system",
                    "content": (
                        "Traduis fidèlement le texte suivant en français. "
                        "Renvoie UNIQUEMENT la traduction sans commentaire :\n\n"
                        f"{input_text_05}"
                    ),
                }
            ],
            evaluator=eval_trad_05,
        )
    )

    # ─────────────────────────────────────────────────────────────────────────
    # Case 6: /todo (Extraction d'action brute pour Inbox.md)
    # ─────────────────────────────────────────────────────────────────────────
    input_text_06 = (
        "Il faudra absolument penser à auditer la consommation mémoire du pipeline audio avant la réunion de vendredi."
    )

    def eval_todo_06(response: str) -> List[AssertionResult]:
        res = [check_no_chitchat(response)]
        cleaned = response.strip()

        # Doit commencer strictement par une case à cocher Markdown
        starts_with_checkbox = cleaned.startswith("- [ ]") or cleaned.startswith("- [ ] ")
        single_line = len(cleaned.splitlines()) == 1

        lower_resp = cleaned.lower()
        has_audit = any(w in lower_resp for w in ["auditer", "audit", "vérifier", "mesurer"])
        has_audio = any(w in lower_resp for w in ["pipeline audio", "audio", "consommation mémoire", "mémoire"])
        has_date = any(w in lower_resp for w in ["vendredi", "réunion"])

        score = (
            (1.0 if starts_with_checkbox else 0.0) * 0.4
            + (1.0 if single_line else 0.0) * 0.2
            + (1.0 if has_audit else 0.0) * 0.15
            + (1.0 if has_audio else 0.0) * 0.15
            + (1.0 if has_date else 0.0) * 0.10
        )

        res.append(
            AssertionResult(
                name="Clean Markdown Task Extraction",
                passed=score >= 0.8,
                score=score,
                details=(
                    f"Checkbox '- [ ]': {starts_with_checkbox}, Ligne unique: {single_line}, "
                    f"Audit: {has_audit}, Audio: {has_audio}, Vendredi: {has_date}"
                ),
            )
        )
        return res

    cases.append(
        TestCase(
            id="PALETTE-06-TODO-EXTRACTION",
            name="/todo Extraction d'Action Nette",
            category=CATEGORY_PALETTE,
            description="Transformation d'une phrase de réflexion en tâche Markdown '- [ ] ...' prête pour Inbox.md.",
            messages=[
                {
                    "role": "system",
                    "content": (
                        "Tu es l'assistant de capture de la palette Jeanne. Extrais l'action à accomplir "
                        "sous la forme exacte d'une case à cocher Markdown '- [ ] Action concise et impérative'. "
                        "Renvoie UNIQUEMENT cette ligne sans texte additionnel :\n\n"
                        f"{input_text_06}"
                    ),
                }
            ],
            evaluator=eval_todo_06,
        )
    )

    # ─────────────────────────────────────────────────────────────────────────
    # Case 7: /bookmark (Annotation synthétique de signet web)
    # ─────────────────────────────────────────────────────────────────────────
    input_text_07 = (
        "URL: https://github.com/asg017/sqlite-vec\n"
        "Titre: sqlite-vec: A vector search SQLite extension written in C"
    )

    def eval_bookmark_07(response: str) -> List[AssertionResult]:
        res = [check_no_chitchat(response)]
        words = response.strip().split()
        word_count = len(words)
        lower_resp = response.lower()

        # Moins de 20 mots
        concise = word_count <= 20
        concise_score = 1.0 if concise else max(0.0, 1.0 - (word_count - 20) * 0.1)

        has_vector = any(w in lower_resp for w in ["vecteur", "vector", "vectorielle", "embeddings"])
        has_sqlite = "sqlite" in lower_resp

        relevance_score = (1.0 if has_vector else 0.0) * 0.5 + (1.0 if has_sqlite else 0.0) * 0.5
        total_score = concise_score * 0.4 + relevance_score * 0.6

        res.append(
            AssertionResult(
                name="Concise Bookmark Annotation",
                passed=total_score >= 0.8,
                score=total_score,
                details=f"Longueur: {word_count} mots (cible <= 20), Vectoriel: {has_vector}, SQLite: {has_sqlite}",
            )
        )
        return res

    cases.append(
        TestCase(
            id="PALETTE-07-BOOKMARK-SUMMARY",
            name="/bookmark Annotation Synthétique",
            category=CATEGORY_PALETTE,
            description="Génération d'une description percutante en une phrase (< 20 mots) pour un signet technique.",
            messages=[
                {
                    "role": "system",
                    "content": (
                        "Génère une description ultra-courte (1 phrase de 15 mots maximum) pour ce signet web technique. "
                        "Renvoie UNIQUEMENT la description sans guillemets ni introduction :\n\n"
                        f"{input_text_07}"
                    ),
                }
            ],
            evaluator=eval_bookmark_07,
        )
    )

    return cases
