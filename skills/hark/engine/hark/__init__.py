"""hark: listen to, measure, make and review sound effects."""

import importlib

__version__ = "0.1.0"

_API_EXTRA = ("llm_critique",)


def _import_or_none(module: str, absent: tuple):
    """Import a module, or return None when the module itself is absent."""
    try:
        return importlib.import_module(module)
    except ModuleNotFoundError as exc:
        if exc.name not in absent:
            raise
        return None


def _api_names(api) -> tuple:
    return tuple(getattr(api, "__all__", ())) + _API_EXTRA


def __getattr__(name: str):
    """Forward the public API names to hark.core.api on first use.

    Nothing heavy is imported by ``import hark``. Any other name is a normal
    submodule import.
    """
    api = _import_or_none("hark.core.api", ("hark.core", "hark.core.api"))
    if api is not None and name in _api_names(api):
        return getattr(api, name)
    full = f"{__name__}.{name}"
    module = _import_or_none(full, (full,))
    if module is None:
        raise AttributeError(f"module 'hark' has no attribute {name!r}")
    return module
