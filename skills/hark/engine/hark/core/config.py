"""Cache folders, model ids and pins, offline flags, threads and precision.

Standard library only. Paths are computed at import; ``refresh()`` computes
them again (tests use it after they change ``HARK_HOME``). Other modules read
the attributes through the module (``config.HOME``), never by value copy.
"""

from __future__ import annotations

import os
from pathlib import Path

PKG_DIR = str(Path(__file__).resolve().parent.parent)
DATA_DIR = str(Path(PKG_DIR) / "data")
VENV_PYTHON_DEFAULT = str(Path("~/.local/state/hark/venv/bin/python").expanduser())

DEFAULT_CLAP = "laion/larger_clap_general"
DEFAULT_AST = "MIT/ast-finetuned-audioset-10-10-0.4593"

OFFLINE_FLAGS = ("HARK_OFFLINE", "HF_HUB_OFFLINE", "TRANSFORMERS_OFFLINE")
_TRUE_WORDS = frozenset({"1", "TRUE", "YES", "ON"})

_CLAP_FILES = (
    "config.json",
    "preprocessor_config.json",
    "merges.txt",
    "special_tokens_map.json",
    "tokenizer.json",
    "tokenizer_config.json",
    "vocab.json",
)

PINS: dict[str, dict] = {
    DEFAULT_CLAP: {
        "revision": "16c8cc3159a3c8a31e8ff5ef1f66b0d9ab3667db",
        "weights": "model.safetensors",
        "files": _CLAP_FILES,
    },
    DEFAULT_AST: {
        "revision": "f826b80d28226b62986cc218e5cec390b1096902",
        "weights": "model.safetensors",
        "files": ("config.json", "preprocessor_config.json"),
    },
}

CUSTOM_ALLOW = (
    "config.json",
    "preprocessor_config.json",
    "feature_extractor_config.json",
    "processor_config.json",
    "tokenizer.json",
    "tokenizer_config.json",
    "special_tokens_map.json",
    "added_tokens.json",
    "vocab.json",
    "vocab.txt",
    "merges.txt",
    "tokenizer.model",
    "spiece.model",
    "sentencepiece.bpe.model",
    "model.safetensors",
    "model.safetensors.index.json",
    "model-?????-of-?????.safetensors",
)


def _env(name: str) -> str | None:
    value = os.environ.get(name, "")
    return value if value.strip() else None


def _home() -> str:
    """HARK_HOME made absolute, else ~/.cache/hark. Never a folder in the repo."""
    given = _env("HARK_HOME")
    root = Path(given).expanduser() if given else Path("~/.cache/hark").expanduser()
    return str(root.resolve())


def refresh() -> None:
    """Read the environment again and set every path and model attribute."""
    global HOME, HF_CACHE, EMB_CACHE, DEC_CACHE
    global CLAP_MODEL, AST_MODEL, CLAP_REVISION, AST_REVISION, PRECISION
    HOME = _home()
    HF_CACHE = str(Path(HOME) / "hf")
    EMB_CACHE = str(Path(HOME) / "emb")
    DEC_CACHE = str(Path(HOME) / "decoded")
    CLAP_MODEL = _env("HARK_CLAP") or DEFAULT_CLAP
    AST_MODEL = _env("HARK_AST") or DEFAULT_AST
    CLAP_REVISION = _env("HARK_CLAP_REVISION")
    AST_REVISION = _env("HARK_AST_REVISION")
    PRECISION = (os.environ.get("HARK_PRECISION") or "bf16").strip().lower()


HOME = HF_CACHE = EMB_CACHE = DEC_CACHE = ""
CLAP_MODEL = AST_MODEL = ""
CLAP_REVISION: str | None = None
AST_REVISION: str | None = None
PRECISION = "bf16"
refresh()


def offline() -> str | None:
    """Name of the first offline flag that is set now, else None."""
    for flag in OFFLINE_FLAGS:
        if os.environ.get(flag, "").strip().upper() in _TRUE_WORDS:
            return flag
    return None


def default_threads() -> int:
    """max(1, min(4, cpu // 2)), with cpu 2 when the OS does not say."""
    cpu = os.cpu_count() or 2
    return max(1, min(4, cpu // 2))


def threads() -> int:
    """HARK_THREADS (at least 1) when set, else the default count."""
    value = os.environ.get("HARK_THREADS", "").strip()
    if not value:
        return default_threads()
    return max(1, int(value))


def ensure_dirs() -> None:
    """Make the cache root and its hf, emb and decoded folders."""
    for folder in (HOME, HF_CACHE, EMB_CACHE, DEC_CACHE):
        Path(folder).mkdir(parents=True, exist_ok=True)
