"""hark: listen to, measure, make and review sound effects."""

__version__ = "0.1.0"

_API_EXTRA = ("llm_critique",)


def __getattr__(name: str):
    """Forward the public API names to hark.core.api on first use.

    Nothing heavy is imported by ``import hark``. Any other name is a normal
    submodule import.
    """
    if name.startswith("_"):
        raise AttributeError(name)
    import importlib

    try:
        api = importlib.import_module("hark.core.api")
    except ModuleNotFoundError as exc:
        if exc.name not in ("hark.core", "hark.core.api"):
            raise
        api = None
    if api is not None and (name in getattr(api, "__all__", ()) or name in _API_EXTRA):
        return getattr(api, name)
    try:
        return importlib.import_module(f"{__name__}.{name}")
    except ModuleNotFoundError as exc:
        if exc.name != f"{__name__}.{name}":
            raise
        raise AttributeError(f"module 'hark' has no attribute {name!r}") from None
