"""Universal client supporting both in-process embedded llama.cpp execution and remote HTTP endpoints."""

import json
import os
import re
import time
import urllib.error
import urllib.request
from typing import Any, Dict, List, Optional, Tuple
from embedded_engine import EmbeddedGgufEngine
from suites.base import InferenceParams


class LlmClient:
    """
    Unified LLM Client capable of:
    1. Direct embedded GGUF inference (using in-process llama.cpp Vulkan / CPU bindings).
    2. Remote / local daemon streaming inference via OpenAI-compatible HTTP (/v1).
    """

    def __init__(
        self,
        endpoint: Optional[str] = None,
        model: str = "default",
        file_path: Optional[str] = None,
        engine: str = "auto",  # "embedded", "http", "auto"
        api_key: Optional[str] = None,
        api_key_env: Optional[str] = None,
        timeout: float = 60.0,
        mock_mode: bool = False,
        use_vulkan: bool = True,
    ):
        self.endpoint = endpoint.rstrip("/") if endpoint else None
        self.model = model
        self.file_path = file_path
        self.timeout = timeout
        self.mock_mode = mock_mode
        self.use_vulkan = use_vulkan

        if api_key:
            self.api_key = api_key
        elif api_key_env and api_key_env in os.environ:
            self.api_key = os.environ[api_key_env]
        else:
            self.api_key = None

        # Decide whether to use embedded engine or HTTP
        self.is_embedded = False
        self.embedded_engine: Optional[EmbeddedGgufEngine] = None

        if not self.mock_mode:
            if engine == "embedded" or (
                engine == "auto"
                and self.file_path
                and os.path.exists(self.file_path)
                and (not self.endpoint or self.endpoint.startswith("embedded"))
            ):
                self.is_embedded = True
                self.embedded_engine = EmbeddedGgufEngine(
                    model_path=self.file_path,
                    n_ctx=4096,
                    use_vulkan=self.use_vulkan,
                )

    @staticmethod
    def normalize_messages(messages: List[Dict[str, str]]) -> List[Dict[str, str]]:
        """Ensures messages contain at least one 'user' message for strict chat templates."""
        if not messages:
            return [{"role": "user", "content": ""}]
        if any(m.get("role") == "user" for m in messages):
            return messages
        normalized = []
        for i, m in enumerate(messages):
            if i == len(messages) - 1 and m.get("role") == "system":
                normalized.append({"role": "user", "content": m.get("content", "")})
            else:
                normalized.append(dict(m))
        return normalized

    def generate(
        self,
        messages: List[Dict[str, str]],
        params: InferenceParams,
    ) -> Tuple[str, float, float, int, float]:
        """
        Executes generation either via in-process embedded llama.cpp or HTTP SSE.
        Returns:
            (response_text, ttft_ms, total_latency_ms, tokens_count, tokens_per_sec)
        """
        norm_messages = self.normalize_messages(messages)

        if self.mock_mode:
            return self._mock_generate(norm_messages, params)

        if self.is_embedded and self.embedded_engine:
            return self.embedded_engine.generate(norm_messages, params)

        if not self.endpoint:
            raise ValueError(
                f"Aucun endpoint HTTP ni fichier GGUF valide spécifié pour le modèle '{self.model}'."
            )

        return self._http_generate(norm_messages, params)

    def _http_generate(
        self,
        messages: List[Dict[str, str]],
        params: InferenceParams,
    ) -> Tuple[str, float, float, int, float]:
        """Streaming chat completion over HTTP."""
        url = f"{self.endpoint}/chat/completions"
        payload: Dict[str, Any] = {
            "model": self.model,
            "messages": messages,
            "stream": True,
            "max_tokens": params.max_tokens,
        }
        if params.temperature is not None:
            payload["temperature"] = params.temperature
        if params.top_p is not None:
            payload["top_p"] = params.top_p
        if params.seed is not None:
            payload["seed"] = params.seed
        if params.frequency_penalty is not None and params.frequency_penalty != 0.0:
            payload["frequency_penalty"] = params.frequency_penalty
        if params.presence_penalty is not None and params.presence_penalty != 0.0:
            payload["presence_penalty"] = params.presence_penalty

        headers = {
            "Content-Type": "application/json",
            "Accept": "text/event-stream",
        }
        if self.api_key:
            headers["Authorization"] = f"Bearer {self.api_key}"

        req_data = json.dumps(payload).encode("utf-8")
        req = urllib.request.Request(url, data=req_data, headers=headers, method="POST")

        t_start = time.perf_counter()
        t_first_token: Optional[float] = None
        collected_chunks: List[str] = []
        reasoning_chunks: List[str] = []

        try:
            with urllib.request.urlopen(req, timeout=self.timeout) as response:
                for line in response:
                    line_str = line.decode("utf-8", errors="replace").strip()
                    if not line_str or line_str.startswith(":"):
                        continue
                    if line_str.startswith("data:"):
                        data_part = line_str[5:].strip()
                        if data_part == "[DONE]":
                            break
                        try:
                            chunk_json = json.loads(data_part)
                            choices = chunk_json.get("choices", [])
                            if choices:
                                delta = choices[0].get("delta", {})
                                reasoning = delta.get("reasoning_content", "")
                                content = delta.get("content", "")
                                if reasoning:
                                    if t_first_token is None:
                                        t_first_token = time.perf_counter()
                                    reasoning_chunks.append(reasoning)
                                if content:
                                    if t_first_token is None:
                                        t_first_token = time.perf_counter()
                                    collected_chunks.append(content)
                        except json.JSONDecodeError:
                            continue
        except urllib.error.HTTPError as e:
            err_body = e.read().decode("utf-8", errors="replace")
            raise RuntimeError(f"HTTP {e.code} Error from {url}: {err_body}")
        except Exception as e:
            raise RuntimeError(f"Connection error to {url}: {str(e)}")

        t_end = time.perf_counter()

        if reasoning_chunks:
            response_text = f"<think>\n{''.join(reasoning_chunks)}\n</think>\n" + "".join(collected_chunks)
        else:
            response_text = "".join(collected_chunks)

        ttft_ms = ((t_first_token - t_start) * 1000.0) if t_first_token else ((t_end - t_start) * 1000.0)
        total_latency_ms = (t_end - t_start) * 1000.0

        total_chunks_count = len(collected_chunks) + len(reasoning_chunks)
        tokens_count = max(total_chunks_count, int(len(response_text.split()) * 1.33))
        if tokens_count == 0:
            tokens_count = 1

        generation_duration_sec = max(0.001, (t_end - (t_first_token or t_start)))
        tokens_per_sec = tokens_count / generation_duration_sec

        return response_text, ttft_ms, total_latency_ms, tokens_count, tokens_per_sec

    def _mock_generate(
        self,
        messages: List[Dict[str, str]],
        params: InferenceParams,
    ) -> Tuple[str, float, float, int, float]:
        """Generates realistic deterministic responses for dry-run verification."""
        last_msg = messages[-1]["content"].lower() if messages else ""
        all_content = " ".join(m.get("content", "") for m in messages).lower()
        is_reasoning = any(k in self.model.lower() for k in ["r1", "qwq", "think", "reasoning"])

        time.sleep(0.05)
        eff_temp = params.temperature if params.temperature is not None else 0.8
        ttft_ms = 45.0 + (eff_temp * 10.0)

        if "quel est le rôle de la base de données sqlite" in all_content:
            resp = (
                "La base de données SQLite (avec sqlite-vec et fts5) n'est qu'un cache d'indexation "
                "jetable et dérivé dans Jeanne. La source de vérité absolue et immuable reste le dossier "
                "de fichiers Markdown locaux selon la philosophie File-over-App. [source: note_architecture_v1.md]"
            )
        elif "satellite" in all_content:
            resp = "Information non trouvée dans les documents fournis."
        elif "confidentialité des données et où sont stockées les clés" in all_content:
            resp = (
                "Jeanne garantit la confidentialité grâce au stockage dans le trousseau sécurisé (keyring natif) "
                "et au masquage local des données sensibles [source: specs_securite_local.md]. "
                "De plus, les notes Markdown restent sous contrôle local selon la philosophie File-over-App [source: note_architecture_v1.md]."
            )
        elif "[person_1]" in all_content:
            resp = (
                "Bonjour [PERSON_1],\n\n"
                "Je vous confirme notre rendez-vous du [DATE_1]. Pour toute question, vous pouvez écrire à "
                "[EMAIL_1] ou me contacter au [PHONE_1].\n\nBien cordialement."
            )
        elif "[client_a]" in all_content:
            resp = (
                "Comparatif pour le projet [PROJECT_X] :\n"
                "- [CLIENT_A] propose le budget [BUDGET_A] avec un délai court de 3 mois.\n"
                "- [CLIENT_B] propose le budget [BUDGET_B] avec un délai plus long de 6 mois."
            )
        elif "rappelle-moi urgemment de réviser les index fts5" in all_content:
            resp = json.dumps(
                {
                    "action": "create_task",
                    "title": "Réviser les index fts5 pour la release",
                    "priority": "urgent",
                    "due_date": "2026-10-15",
                },
                indent=2,
            )
        elif "bm25 avec k1=1.2" in all_content:
            resp = json.dumps(
                {
                    "category": "Ressources",
                    "tags": ["bm25", "recherche", "fts5", "algorithme"],
                    "summary": "Paramétrage BM25 pour la recherche plein texte sur les notes Markdown.",
                },
                indent=2,
            )
        elif "transcription de la réunion" in all_content:
            resp = (
                "### Décisions\n"
                "- Clôture de la relecture du code ce soir.\n"
                "- Soumission du rapport d'audit par Claire lundi.\n\n"
                "### Actions à mener\n"
                "- [ ] @Bob: Optimiser la libération du buffer mmap d'ici vendredi 17h\n"
                "- [ ] @Claire: Préparer le rapport d'audit du Jalon 3 pour lundi\n"
            )
        elif "fenêtre de contexte kv" in all_content:
            resp = "La limite stricte est de 4096 tokens pour prévenir les débordements de mémoire vive (OOM)."
        elif "bonjor, coment sa va" in all_content:
            resp = "Bonjour, comment ça va ?"
        elif "aparament sa marche pas" in all_content:
            resp = "Apparemment ça ne marche pas, où est le problème ?"
        elif "reçut votre devis mes il y a des érreur" in all_content:
            resp = "Je vous contacte car nous avons reçu votre devis mais il y a des erreurs de calcul."
        elif "implementer une requete sqlite" in all_content:
            resp = "Je suis développeur et j'ai implémenté une requête sqlite sans index fts5."
        elif "tu peux m envoyer le doc stp c urgent on a un souci avec le client" in all_content:
            resp = "Pourriez-vous s'il vous plaît me transmettre le document dès que possible ? Nous rencontrons une urgence concernant le dossier client."
        elif "baisse de performance au niveau de la synchronisation" in all_content:
            resp = "Une baisse de performance affecte la synchronisation de la base de données lors de fortes charges simultanées."
        elif "3 puces clés concises commençant par un tiret (-)" in all_content or "le projet jeanne repose sur le paradigme file-over-app" in all_content:
            resp = (
                "- Les fichiers Markdown constituent la source de vérité immuable locale selon la philosophie File-over-App.\n"
                "- La base de données SQLite avec sqlite-vec et fts5 sert exclusivement de cache d'indexation jetable.\n"
                "- L'empreinte mémoire vive applicative est plafonnée à 200 Mo avec un contexte KV limité à 4096 tokens."
            )
        elif "traduis fidèlement le texte suivant en anglais" in all_content or "sans serveur externe afin de préserver la confidentialité" in all_content:
            resp = "The local inference engine executes directly in-memory without an external server to preserve the privacy of the notes."
        elif "traduis fidèlement le texte suivant en français" in all_content or "floating quick-access palette" in all_content:
            resp = "La palette d'accès rapide flottante doit s'ouvrir en moins de 50 millisecondes pour préserver une expérience utilisateur fluide."
        elif "tu es l'assistant de capture de la palette jeanne" in all_content or "auditer la consommation mémoire du pipeline audio" in all_content:
            resp = "- [ ] Auditer la consommation mémoire du pipeline audio d'ici la réunion de vendredi"
        elif "pour ce signet web technique" in all_content or "sqlite-vec: a vector search sqlite extension" in all_content:
            resp = "Extension SQLite écrite en C pour la recherche vectorielle rapide et locale."
        else:
            resp = "Réponse simulée de test pour l'assistant Jeanne."

        if is_reasoning:
            think_prefix = (
                "<think>\n"
                "Analyse minutieuse de la requête de l'utilisateur...\n"
                "Considération des contraintes imposées par l'assistant Jeanne et respect strict des règles.\n"
                "Élaboration d'une réponse optimale étape par étape afin de respecter la syntaxe requise.\n"
                "</think>\n"
            )
            resp = think_prefix + resp

        tokens_count = int(len(resp.split()) * 1.33) if is_reasoning else len(resp.split())
        total_latency_ms = ttft_ms + (tokens_count * 15.0)
        tokens_per_sec = tokens_count / max(0.01, (total_latency_ms - ttft_ms) / 1000.0)

        return resp, ttft_ms, total_latency_ms, tokens_count, tokens_per_sec

    def unload(self) -> None:
        """Frees model from RAM/VRAM if running embedded engine."""
        if self.embedded_engine:
            self.embedded_engine.unload()
