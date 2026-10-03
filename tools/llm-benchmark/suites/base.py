"""Base classes, data models, and deterministic evaluation primitives for the Jeanne LLM Benchmark."""

import json
import re
import time
from dataclasses import dataclass, field
from typing import Any, Callable, Dict, List, Optional, Tuple


@dataclass
class InferenceParams:
    """Hyperparameters used for LLM generation. Recorded to guarantee determinism and reproducibility."""
    temperature: float = 0.0
    seed: Optional[int] = 42
    top_p: float = 1.0
    max_tokens: int = 1024
    frequency_penalty: float = 0.0
    presence_penalty: float = 0.0

    def to_dict(self) -> Dict[str, Any]:
        data = {
            "temperature": self.temperature,
            "seed": self.seed,
            "top_p": self.top_p,
            "max_tokens": self.max_tokens,
            "frequency_penalty": self.frequency_penalty,
            "presence_penalty": self.presence_penalty,
        }
        return {k: v for k, v in data if v is not None} if False else data

    def summary_str(self) -> str:
        s = f"temp={self.temperature}"
        if self.seed is not None:
            s += f", seed={self.seed}"
        s += f", top_p={self.top_p}, max_tokens={self.max_tokens}"
        return s


@dataclass
class AssertionResult:
    """Result of a single deterministic assertion check."""
    name: str
    passed: bool
    score: float  # 0.0 to 1.0
    details: str


@dataclass
class TestCase:
    """Specification of an individual test case."""
    id: str
    name: str
    category: str
    description: str
    messages: List[Dict[str, str]]
    evaluator: Callable[[str], List[AssertionResult]]
    recommended_params: Optional[InferenceParams] = None


@dataclass
class TestResult:
    """Outcome of running a test case against an LLM endpoint."""
    test_id: str
    test_name: str
    category: str
    model_id: str
    profile_name: str
    inference_params: Dict[str, Any]
    passed: bool
    score: float  # 0.0 to 100.0
    ttft_ms: float
    total_latency_ms: float
    tokens_generated: int
    tokens_per_sec: float
    response_text: str
    assertions: List[AssertionResult]
    error: Optional[str] = None


@dataclass
class BenchmarkSuiteResult:
    """Aggregated results for a model run under a specific inference profile."""
    model_id: str
    model_display_name: str
    profile_name: str
    inference_params: Dict[str, Any]
    test_results: List[TestResult] = field(default_factory=list)
    overall_score: float = 0.0
    avg_ttft_ms: float = 0.0
    avg_tokens_per_sec: float = 0.0
    category_scores: Dict[str, float] = field(default_factory=dict)


# ==============================================================================
# Deterministic Assertion Helpers (Zero LLM-as-a-judge)
# ==============================================================================

def check_citations(
    response: str,
    required_sources: List[str],
    forbidden_sources: Optional[List[str]] = None,
) -> AssertionResult:
    """Verifies faithful markdown source citations in format [source: filename.md]."""
    lower_resp = response.lower()
    citation_regex = re.compile(r"\[source:\s*([^\]]+)\]", re.IGNORECASE)
    found_citations = [m.strip().lower() for m in citation_regex.findall(response)]

    missing = []
    for req in required_sources:
        req_clean = req.strip().lower()
        # Direct check in found citations or in response text as fallback
        if not any(req_clean in cit for cit in found_citations) and f"[source: {req_clean}]" not in lower_resp:
            missing.append(req)

    hallucinated = []
    if forbidden_sources:
        for f in forbidden_sources:
            f_clean = f.strip().lower()
            if any(f_clean in cit for cit in found_citations) or f_clean in lower_resp:
                hallucinated.append(f)

    if missing and hallucinated:
        return AssertionResult(
            name="Citations Validation",
            passed=False,
            score=0.0,
            details=f"Missing sources: {missing}; Hallucinated/forbidden: {hallucinated}",
        )
    if missing:
        score = max(0.0, (len(required_sources) - len(missing)) / len(required_sources))
        return AssertionResult(
            name="Citations Validation",
            passed=False,
            score=score,
            details=f"Missing required sources: {missing}. Found citations: {found_citations}",
        )
    if hallucinated:
        return AssertionResult(
            name="Citations Validation",
            passed=False,
            score=0.5,
            details=f"Contains forbidden or hallucinated sources: {hallucinated}",
        )

    return AssertionResult(
        name="Citations Validation",
        passed=True,
        score=1.0,
        details=f"All {len(required_sources)} source citations verified: {found_citations}",
    )


def check_refusal_on_missing_info(response: str) -> AssertionResult:
    """Verifies that the LLM explicitly refuses when knowledge is absent (anti-hallucination)."""
    lower = response.lower()
    refusal_keywords = [
        "information non trouvée",
        "introuvable",
        "ne mentionne pas",
        "pas mention",
        "aucun élément",
        "documents ne contiennent pas",
        "not found",
        "not mentioned",
        "no information",
        "cannot find",
        "not in the provided",
        "ne contiennent aucune information",
    ]

    matched = [kw for kw in refusal_keywords if kw in lower]
    if matched:
        return AssertionResult(
            name="Refusal on Missing Data",
            passed=True,
            score=1.0,
            details=f"Properly refused with keyword(s): {matched}",
        )

    return AssertionResult(
        name="Refusal on Missing Data",
        passed=False,
        score=0.0,
        details="Model attempted to answer without the required information (likely hallucination).",
    )


def check_pii_preservation(response: str, expected_tokens: List[str]) -> AssertionResult:
    """Verifies that all anonymized PII tokens ([PERSON_1], [EMAIL_1], etc.) remain unmodified."""
    missing_tokens = []
    for token in expected_tokens:
        if token not in response:
            missing_tokens.append(token)

    if not missing_tokens:
        return AssertionResult(
            name="PII Token Integrity",
            passed=True,
            score=1.0,
            details=f"All {len(expected_tokens)} PII tokens perfectly preserved without corruption.",
        )

    score = max(0.0, (len(expected_tokens) - len(missing_tokens)) / len(expected_tokens))
    return AssertionResult(
        name="PII Token Integrity",
        passed=False,
        score=score,
        details=f"Missing or corrupted PII tokens: {missing_tokens}",
    )


def check_json_strict(
    response: str,
    required_keys: List[str],
    allowed_values: Optional[Dict[str, List[Any]]] = None,
) -> AssertionResult:
    """Verifies that the output is strictly valid JSON containing expected schema properties."""
    cleaned = response.strip()
    # Strip markdown code blocks if wrapped
    if cleaned.startswith("```"):
        lines = cleaned.split("\n")
        if lines[0].startswith("```"):
            lines = lines[1:]
        if lines and lines[-1].startswith("```"):
            lines = lines[:-1]
        cleaned = "\n".join(lines).strip()

    try:
        data = json.loads(cleaned)
    except json.JSONDecodeError as e:
        return AssertionResult(
            name="JSON Syntax Validation",
            passed=False,
            score=0.0,
            details=f"Invalid JSON: {str(e)}",
        )

    if not isinstance(data, dict):
        return AssertionResult(
            name="JSON Schema Validation",
            passed=False,
            score=0.2,
            details=f"Expected JSON object, got {type(data).__name__}",
        )

    missing_keys = [k for k in required_keys if k not in data]
    if missing_keys:
        score = max(0.0, (len(required_keys) - len(missing_keys)) / len(required_keys))
        return AssertionResult(
            name="JSON Schema Validation",
            passed=False,
            score=score,
            details=f"Missing required keys: {missing_keys}. Found: {list(data.keys())}",
        )

    if allowed_values:
        for key, valid_vals in allowed_values.items():
            if key in data and data[key] not in valid_vals:
                return AssertionResult(
                    name="JSON Values Validation",
                    passed=False,
                    score=0.7,
                    details=f"Key '{key}' value '{data[key]}' not in allowed {valid_vals}",
                )

    return AssertionResult(
        name="JSON Syntax & Schema Validation",
        passed=True,
        score=1.0,
        details=f"Valid JSON with all required keys: {required_keys}",
    )


def check_markdown_checklists(response: str, min_items: int = 1) -> AssertionResult:
    """Verifies that the response extracts Jeanne task checklists (- [ ] ...)."""
    task_regex = re.compile(r"^\s*-\s*\[\s*\]\s+(.+)$", re.MULTILINE)
    matches = task_regex.findall(response)

    if len(matches) >= min_items:
        return AssertionResult(
            name="Task Checklist Extraction",
            passed=True,
            score=1.0,
            details=f"Extracted {len(matches)} checklist item(s) (min required: {min_items}).",
        )

    score = len(matches) / max(1, min_items)
    return AssertionResult(
        name="Task Checklist Extraction",
        passed=False,
        score=score,
        details=f"Extracted only {len(matches)} checklist item(s), expected at least {min_items}.",
    )


def check_word_count(response: str, max_words: int) -> AssertionResult:
    """Verifies conciseness and adherence to bounded word count limits."""
    words = response.split()
    count = len(words)
    if count <= max_words:
        return AssertionResult(
            name="Conciseness Limit",
            passed=True,
            score=1.0,
            details=f"Word count {count} is within limit ({max_words}).",
        )

    # Partial score with linear penalty
    over = count - max_words
    score = max(0.0, 1.0 - (over / max_words))
    return AssertionResult(
        name="Conciseness Limit",
        passed=False,
        score=score,
        details=f"Word count {count} exceeds limit of {max_words} words.",
    )
