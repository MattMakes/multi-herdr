"""hark.forge: make sounds in code, search them against a critic, write and report them.

The names below load lazily from hark.forge.engine.loop on first access, so
`import hark.forge.engine.dsp` never loads the critic or the search.
"""

import importlib
from pathlib import Path

_LOOP = ("forge", "family_box", "palette", "palette_md", "variant_recipe", "pick_refs", "resolve_refs",
         "render_block", "render_params", "audition", "report_md", "SCORE_KEYS", "load_recipe")
_ROOT = Path(__file__).parent

__all__ = list(_LOOP) + ["recipes"]


def _is_sub(name: str) -> bool:
    return (_ROOT / f"{name}.py").is_file() or (_ROOT / name / "__init__.py").is_file()


def __getattr__(name: str):
    if name in _LOOP:
        return getattr(importlib.import_module("hark.forge.engine.loop"), name)
    if _is_sub(name):
        # hark.forge.recipes is a callable module: hark.forge.recipes() lists the recipes.
        return importlib.import_module(f"hark.forge.{name}")
    raise AttributeError(f"module 'hark.forge' has no attribute {name!r}")


def __dir__():
    return sorted(set(globals()) | set(__all__))
