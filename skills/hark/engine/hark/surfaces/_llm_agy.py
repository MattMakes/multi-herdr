"""The agy command-line client as hark's only audio-model provider.

agy runs under the operator's own sign-in. hark hands it an environment with
every key-like variable removed and never reads their values.
"""

from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import tempfile
import time
from pathlib import Path
from typing import Any

from hark.surfaces._flow import raise_if

AGY_DEFAULT_MODEL = "gemini-4-argon-high"  # used when --model and HARK_LLM_MODEL are unset


AGY_TIMEOUT_S = 240


MODEL_ENV = "HARK_LLM_MODEL"


ERR_CAP = 300


RETRY_WAIT_S = 4.0


_KEYISH = re.compile(r"API_?KEY|SECRET", re.IGNORECASE)


_SECRET_TEXT = re.compile(r"\b(sk-[A-Za-z0-9_\-]{12,}|AIza[0-9A-Za-z_\-]{20,})")


_RATE_LIMIT = re.compile(r"rate.?limit|RESOURCE_EXHAUSTED|\b429\b|quota", re.IGNORECASE)


_sleep = time.sleep


class ProviderError(RuntimeError):
    """The agy call failed."""


class AgyCannotHear(ProviderError):
    """The probe verdict says agy does not hear audio files; nothing is sent."""


def child_env() -> dict[str, str]:
    """The environment for agy: every variable except key-like names, whose values are never read."""
    return {name: os.environ[name] for name in list(os.environ) if not _KEYISH.search(name)}


def scrub(text: str) -> str:
    """Replace anything that looks like a secret value with ***."""
    return _SECRET_TEXT.sub("***", str(text))


def _agy_usage(usage: Any) -> dict[str, int]:
    if not isinstance(usage, dict):
        return {}
    pairs = (("promptTokenCount", ("promptTokenCount", "input_tokens", "prompt_tokens")),
             ("candidatesTokenCount", ("candidatesTokenCount", "output_tokens", "completion_tokens")),
             ("thoughtsTokenCount", ("thoughtsTokenCount", "thoughts_tokens", "reasoning_tokens")),
             ("totalTokenCount", ("totalTokenCount", "total_tokens")))
    out: dict[str, int] = {}
    for key, names in pairs:
        val = next((usage[n] for n in names if isinstance(usage.get(n), int)), None)
        if val is not None:
            out[key] = val
    return out


def _agy_prompt(wav: str, prompt: str) -> str:
    return (f"Open the audio file {wav} and listen to it. Do not run shell commands or scripts; "
            "use only your own hearing of the audio.\n\n" + prompt)


class AgyProvider:
    """The Antigravity CLI ``agy -p`` as the audio-model provider."""

    name = "agy"
    default_model = AGY_DEFAULT_MODEL

    def executable(self) -> str | None:
        return shutil.which("agy")

    def ready(self) -> bool:
        return self.executable() is not None

    def model(self, model: str | None = None) -> str:
        chosen = model or os.environ.get(MODEL_ENV) or self.default_model
        return chosen[len("models/"):] if chosen.startswith("models/") else chosen

    def how_to_enable(self) -> list[str]:
        return ["Install the agy CLI and sign in with your own account, so `agy` is on PATH.",
                "Run again with --yes-upload (CLI) or consent=True (Python, MCP) only when the user "
                "agrees that this audio leaves the machine."]

    def argv(self, prompt: str, folder: str, model: str) -> list[str]:
        return [self.executable() or "agy", "-p", prompt, "--add-dir", folder, "--model", model,
                "--output-format", "json", "--print-timeout", f"{AGY_TIMEOUT_S}s"]

    def _run_once(self, argv: list[str]) -> subprocess.CompletedProcess:
        try:
            return subprocess.run(argv, capture_output=True, text=True, env=child_env(),
                                  stdin=subprocess.DEVNULL, timeout=AGY_TIMEOUT_S + 30)
        except subprocess.TimeoutExpired:
            raise ProviderError("agy did not answer in time; try a shorter clip with --start / --end") from None
        except OSError as exc:
            raise ProviderError(f"agy could not start: {type(exc).__name__}: {exc}") from None

    def _run(self, argv: list[str]) -> subprocess.CompletedProcess:
        proc = self._run_once(argv)
        if proc.returncode != 0 and _RATE_LIMIT.search((proc.stdout or "") + (proc.stderr or "")):
            _sleep(RETRY_WAIT_S)
            proc = self._run_once(argv)
        return proc

    def generate(self, audio_bytes: bytes, mime: str, prompt: str, model: str) -> dict[str, Any]:
        folder = tempfile.mkdtemp(prefix="hark-llm-")
        try:
            wav = os.path.join(folder, "clip.wav")
            Path(wav).write_bytes(audio_bytes)
            proc = self._run(self.argv(_agy_prompt(wav, prompt), folder, model))
            return self._answer(proc, model)
        finally:
            shutil.rmtree(folder, ignore_errors=True)

    def _answer(self, proc: subprocess.CompletedProcess, model: str) -> dict[str, Any]:
        data = _json_or_none(proc.stdout or "")
        _check_agy_json(data, proc)
        return {"text": data["response"], "finish_reason": data.get("finish_reason") or data.get("stop_reason"),
                "method": "agy", "files_api_deleted": None,
                "model_version": data.get("model") or model or "agy-default",
                "usage": _agy_usage(data.get("usage"))}


def _stopped(data: dict) -> bool:
    return bool(data.get("finished_reason")) or data.get("status") not in (None, "SUCCESS")


def _blank(text: Any) -> bool:
    return not isinstance(text, str) or not text.strip()


def _check_agy_json(data: Any, proc: subprocess.CompletedProcess) -> None:
    """ProviderError when agy gave no JSON, stopped early, or gave an empty answer."""
    err = (proc.stderr or "")[:ERR_CAP]
    code = proc.returncode
    raise_if(not isinstance(data, dict),
             ProviderError(f"agy gave no JSON answer (exit {code}): {err or (proc.stdout or '')[:ERR_CAP]}"))
    raise_if(_stopped(data), ProviderError(f"agy stopped: status {data.get('status')}, "
                                           f"finished_reason {data.get('finished_reason')} (exit {code})"))
    raise_if(_blank(data.get("response")), ProviderError(f"agy gave an empty answer (exit {code}): {err}"))


PROVIDERS: dict[str, type] = {"agy": AgyProvider}


def _json_or_none(text: str) -> Any:
    try:
        return json.loads(text)
    except ValueError:
        return None


def get_provider(provider: Any = "agy") -> Any:
    """A provider object from a name or an object; ValueError for an unknown name."""
    if not isinstance(provider, str):
        return provider
    if provider not in PROVIDERS:
        raise ValueError(f"unknown LLM provider {provider!r}; providers: {', '.join(PROVIDERS)}")
    return PROVIDERS[provider]()
