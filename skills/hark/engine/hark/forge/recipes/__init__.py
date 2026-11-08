"""Recipe registry: finds recipe modules in the recipe folders.

A recipe lives at hark/forge/recipes/<folder>/<name>.py. Names are flat and
unique across folders. Scanning imports nothing; load() imports one recipe.
Modules whose name starts with "_" are helpers, not recipes.

The module is also callable: hark.forge.recipes() returns names().
"""

from __future__ import annotations

import functools
import importlib
import pkgutil
import sys
import types
from pathlib import Path

FOLDERS: tuple[str, ...] = ("ui", "impacts", "motion", "risers")

_ROOT = Path(__file__).resolve().parent
_PREFIX = __name__
_SEEDS = _ROOT.parent / "seeds"


class UnknownRecipe(KeyError):
    """No recipe of that name."""


def _modules(path: Path) -> list[str]:
    if not path.is_dir():
        return []
    return [i.name for i in pkgutil.iter_modules([str(path)]) if not (i.name.startswith("_") or i.ispkg)]


def _scan(root: Path) -> dict[str, str]:
    found: dict[str, str] = {}
    for folder in FOLDERS:
        for name in _modules(root / folder):
            if name in found:
                raise RuntimeError(f"recipe name {name!r} is in both {found[name]!r} and {folder!r}")
            found[name] = folder
    return found


def _table() -> dict[str, str]:
    return _scan(_ROOT)


def names() -> list[str]:
    """Sorted flat recipe names of every folder."""
    return sorted(_table())


def folder_of(name: str) -> str:
    """The folder that holds a recipe, for example "motion" for "whoosh"."""
    table = _table()
    if name not in table:
        raise UnknownRecipe(name)
    return table[name]


def module_path(name: str) -> str:
    """Dotted module path of a recipe."""
    return f"{_PREFIX}.{folder_of(name)}.{name}"


def load(name: str) -> types.ModuleType:
    """Import and return a recipe module."""
    return importlib.import_module(module_path(name))


def _recipe_module_name(dotted: str | None) -> str | None:
    if not dotted or not dotted.startswith(_PREFIX + "."):
        return None
    parts = dotted[len(_PREFIX) + 1:].split(".")
    if len(parts) == 2 and parts[0] in FOLDERS:
        return parts[1]
    return None


def short_name(obj: object) -> str | None:
    """Flat recipe name of a recipe module, recipe object or render function."""
    if isinstance(obj, functools.partial):
        obj = obj.func
    if isinstance(obj, types.ModuleType):
        return obj.__name__.rsplit(".", 1)[-1]
    if callable(obj) and hasattr(obj, "__module__") and not hasattr(obj, "SPACE"):
        return _recipe_module_name(getattr(obj, "__module__", None))
    label = getattr(obj, "__name__", None) or getattr(obj, "MODULE", None)
    return str(label).rsplit(".", 1)[-1] if label else None


def seed_path(name: str) -> Path:
    """Path of a recipe's seed bank (the file may not exist)."""
    return _SEEDS / folder_of(name) / f"{name}.json"


class _CallableModule(types.ModuleType):
    def __call__(self) -> list[str]:
        return names()


sys.modules[__name__].__class__ = _CallableModule
