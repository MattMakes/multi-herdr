"""hark: listen to, measure, make and review sound effects."""

import importlib
from pathlib import Path

__version__ = "0.1.0"

_API_EXTRA = ("llm_critique",)
_ROOT = Path(__file__).parent


def _exists(dotted: str) -> bool:
    """True when the hark module or package named by ``dotted`` is on disk."""
    base = _ROOT.joinpath(*dotted.split(".")[1:])
    return base.with_suffix(".py").is_file() or (base / "__init__.py").is_file()


def _api():
    return importlib.import_module("hark.core.api") if _exists("hark.core.api") else None


def _api_names(api) -> tuple:
    extra = _API_EXTRA if api is not None else ()
    return tuple(getattr(api, "__all__", ())) + extra


def __getattr__(name: str):
    """Forward the public API names to hark.core.api on first use.

    Nothing heavy is imported by ``import hark``. Any other name is a normal
    submodule import.
    """
    api = _api()
    if name in _api_names(api):
        return getattr(api, name)
    if not _exists(f"{__name__}.{name}"):
        raise AttributeError(f"module 'hark' has no attribute {name!r}")
    return importlib.import_module(f"{__name__}.{name}")
