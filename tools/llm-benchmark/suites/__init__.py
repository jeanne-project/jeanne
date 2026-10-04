"""Suites aggregator for Jeanne LLM Benchmark."""

from typing import Dict, List
from .base import TestCase
from .rag import get_rag_test_cases
from .pii import get_pii_test_cases
from .structured import get_structured_test_cases
from .meeting import get_meeting_test_cases
from .conciseness import get_conciseness_test_cases
from .correction import get_correction_test_cases
from .palette import get_palette_test_cases


def get_all_test_cases() -> List[TestCase]:
    cases = []
    cases.extend(get_rag_test_cases())
    cases.extend(get_pii_test_cases())
    cases.extend(get_structured_test_cases())
    cases.extend(get_meeting_test_cases())
    cases.extend(get_conciseness_test_cases())
    cases.extend(get_correction_test_cases())
    cases.extend(get_palette_test_cases())
    return cases


def get_available_suites() -> Dict[str, List[TestCase]]:
    return {
        "rag": get_rag_test_cases(),
        "pii": get_pii_test_cases(),
        "structured": get_structured_test_cases(),
        "meeting": get_meeting_test_cases(),
        "conciseness": get_conciseness_test_cases(),
        "correction": get_correction_test_cases(),
        "corrige": get_correction_test_cases(),
        "palette": get_palette_test_cases(),
        "actions": get_palette_test_cases(),
    }
