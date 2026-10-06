"""Embedded GGUF inference engine executing llama.cpp directly in-process without any HTTP daemon."""

import gc
import json
import os
import shutil
import subprocess
import time
from typing import Any, Dict, List, Optional, Tuple
from suites.base import InferenceParams


class EmbeddedGgufEngine:
    """
    Direct in-process GGUF engine using llama-cpp-python bindings or standalone llama-cli.
    Mirrors Jeanne's embedded execution model (4096 context ceiling, Vulkan acceleration, clean unload).
    """

    def __init__(
        self,
        model_path: str,
        n_ctx: int = 4096,
        n_gpu_layers: int = -1,
        use_gpu: bool = True,
        use_vulkan: Optional[bool] = None,
        verbose: bool = False,
    ):
        self.model_path = os.path.abspath(model_path)
        self.n_ctx = min(n_ctx, 4096)  # Strict Jeanne KV context ceiling
        self.n_gpu_layers = n_gpu_layers
        self.use_gpu = use_vulkan if use_vulkan is not None else use_gpu
        self.use_vulkan = self.use_gpu
        self.verbose = verbose
        self._llm = None
        self._load_duration_ms: float = 0.0

        if not os.path.exists(self.model_path):
            raise FileNotFoundError(f"Modèle GGUF introuvable au chemin : {self.model_path}")

    @staticmethod
    def is_available() -> Tuple[bool, str]:
        """Checks if llama-cpp-python bindings or standalone llama-cli are installed."""
        # 1. Vérification dans le processus Python en cours
        try:
            import llama_cpp
            return True, f"llama-cpp-python (v{llama_cpp.__version__}) [environnement actif]"
        except ImportError:
            pass

        # 2. Vérification dans les virtualenvs existants
        try:
            from environment_setup import find_venvs_with_llama_cpp
            venvs = find_venvs_with_llama_cpp()
            if venvs:
                if len(venvs) == 1:
                    v = venvs[0]
                    v_name = v["name"]
                    ver_str = f"v{v['version']}" if v.get("version") and v["version"] != "inconnue" else "prêt"
                    return True, f"llama-cpp-python ({ver_str}) [détecté dans le venv '{v_name}']"
                else:
                    details = []
                    for v in venvs:
                        ver = f"v{v['version']}" if v.get("version") and v["version"] != "inconnue" else "prêt"
                        details.append(f"{v['name']} ({ver})")
                    return True, f"llama-cpp-python [détecté dans {len(venvs)} venvs : {', '.join(details)}]"
        except Exception:
            pass

        # 3. Check for standalone llama-cli binary in PATH
        cli_path = shutil.which("llama-cli") or shutil.which("llama-simple")
        if cli_path:
            return True, f"binaire llama-cli ({cli_path})"

        return False, "Non installé (pip install llama-cpp-python ou 'python3 tools/llm-benchmark/benchmark.py setup')"

    def load(self) -> float:
        """Loads the model into memory. Returns load duration in ms."""
        if self._llm is not None:
            return self._load_duration_ms

        t0 = time.perf_counter()
        try:
            from llama_cpp import Llama
        except ImportError:
            # Tenter d'injecter in-process le site-packages d'un venv disponible
            try:
                from environment_setup import try_activate_venv_in_process
                if try_activate_venv_in_process():
                    from llama_cpp import Llama
                else:
                    raise ImportError()
            except Exception:
                # Fallback to standalone llama-cli if present
                cli_path = shutil.which("llama-cli")
                if cli_path:
                    self._llm = "cli_fallback"
                    self._load_duration_ms = 0.0
                    return 0.0

                raise RuntimeError(
                    "Le moteur d'inférence embarqué requiert 'llama-cpp-python'.\n"
                    "Pour l'installer sur votre PC avec accélération matérielle :\n"
                    "  • NVIDIA (CUDA précompilé - recommandé) :\n"
                    "      pip install llama-cpp-python --extra-index-url https://abetlen.github.io/llama-cpp-python/whl/cu124\n"
                    "  • NVIDIA (Compilation CUDA) : CMAKE_ARGS=\"-DGGML_CUDA=on\" pip install llama-cpp-python\n"
                    "  • Vulkan (comme Jeanne sur AMD/Intel/NVIDIA) : CMAKE_ARGS=\"-DGGML_VULKAN=on\" pip install llama-cpp-python\n"
                    "  • Apple Silicon (Metal) : CMAKE_ARGS=\"-DGGML_METAL=on\" pip install llama-cpp-python\n"
                    "  • CPU uniquement : pip install llama-cpp-python"
                )

        # In llama.cpp, n_gpu_layers=-1 offloads all layers to Vulkan/GPU
        gpu_layers = self.n_gpu_layers if self.use_vulkan else 0
        self._llm = Llama(
            model_path=self.model_path,
            n_ctx=self.n_ctx,
            n_gpu_layers=gpu_layers,
            verbose=self.verbose,
        )
        t_end = time.perf_counter()
        self._load_duration_ms = (t_end - t0) * 1000.0
        return self._load_duration_ms

    @staticmethod
    def _normalize_messages(messages: List[Dict[str, str]]) -> List[Dict[str, str]]:
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
        Executes in-process streaming generation directly on the GGUF file.
        Returns: (response_text, ttft_ms, total_latency_ms, tokens_count, tokens_per_sec)
        """
        self.load()

        norm_messages = self._normalize_messages(messages)

        if self._llm == "cli_fallback":
            return self._generate_via_cli(norm_messages, params)

        t_start = time.perf_counter()
        t_first_token: Optional[float] = None
        collected_chunks: List[str] = []

        completion_kwargs: Dict[str, Any] = {
            "messages": norm_messages,
            "max_tokens": params.max_tokens,
            "stream": True,
        }
        if params.temperature is not None:
            completion_kwargs["temperature"] = params.temperature
        if params.top_p is not None:
            completion_kwargs["top_p"] = params.top_p
        if params.seed is not None:
            completion_kwargs["seed"] = params.seed
        if params.frequency_penalty is not None and params.frequency_penalty != 0.0:
            completion_kwargs["frequency_penalty"] = params.frequency_penalty
        if params.presence_penalty is not None and params.presence_penalty != 0.0:
            completion_kwargs["presence_penalty"] = params.presence_penalty

        stream = self._llm.create_chat_completion(**completion_kwargs)

        for chunk in stream:
            choices = chunk.get("choices", [])
            if choices:
                delta = choices[0].get("delta", {})
                content = delta.get("content", "")
                if content:
                    if t_first_token is None:
                        t_first_token = time.perf_counter()
                    collected_chunks.append(content)

        t_end = time.perf_counter()

        response_text = "".join(collected_chunks)
        ttft_ms = ((t_first_token - t_start) * 1000.0) if t_first_token else ((t_end - t_start) * 1000.0)
        total_latency_ms = (t_end - t_start) * 1000.0

        tokens_count = len(collected_chunks)
        if tokens_count == 0:
            tokens_count = max(1, int(len(response_text.split()) * 1.33))

        generation_duration_sec = max(0.001, (t_end - (t_first_token or t_start)))
        tokens_per_sec = tokens_count / generation_duration_sec

        return response_text, ttft_ms, total_latency_ms, tokens_count, tokens_per_sec

    def _generate_via_cli(
        self,
        messages: List[Dict[str, str]],
        params: InferenceParams,
    ) -> Tuple[str, float, float, int, float]:
        """Execution fallback using standalone llama-cli subprocess."""
        # Simple prompt serialization
        prompt_parts = []
        for m in messages:
            role = m["role"]
            content = m["content"]
            prompt_parts.append(f"<|im_start|>{role}\n{content}<|im_end|>")
        prompt_parts.append("<|im_start|>assistant\n")
        full_prompt = "\n".join(prompt_parts)

        cmd = [
            "llama-cli",
            "-m", self.model_path,
            "-p", full_prompt,
            "-n", str(params.max_tokens),
            "-c", str(self.n_ctx),
        ]
        if params.temperature is not None:
            cmd.extend(["--temp", str(params.temperature)])
        if params.top_p is not None:
            cmd.extend(["--top-p", str(params.top_p)])
        if params.seed is not None:
            cmd.extend(["-s", str(params.seed)])
        if self.use_vulkan:
            cmd.extend(["-ngl", str(self.n_gpu_layers)])

        t_start = time.perf_counter()
        proc = subprocess.run(cmd, capture_output=True, text=True)
        t_end = time.perf_counter()

        response_text = proc.stdout.strip()
        tokens_count = max(1, int(len(response_text.split()) * 1.33))
        total_latency_ms = (t_end - t_start) * 1000.0
        ttft_ms = total_latency_ms * 0.2  # Approximate for CLI batch mode
        tokens_per_sec = tokens_count / max(0.001, (total_latency_ms / 1000.0))

        return response_text, ttft_ms, total_latency_ms, tokens_count, tokens_per_sec

    def unload(self) -> None:
        """Unloads the model from RAM/VRAM to free resources immediately."""
        self._llm = None
        gc.collect()
