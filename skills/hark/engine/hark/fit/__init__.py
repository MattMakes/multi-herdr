"""hark fit: from a recording to a recipe module.

The names below load their module on first use, so ``import hark.fit`` stays
cheap and never loads the analysis or the pipeline.
"""

from __future__ import annotations

import importlib
from typing import Any

_API = {
    "fit": "hark.fit.pipeline.auto",
    "render": "hark.fit.pipeline.auto",
    "morph": "hark.fit.pipeline.auto",
    "kind_of": "hark.fit.pipeline.auto",
    "load": "hark.fit.pipeline.auto",
    "save": "hark.fit.pipeline.auto",
    "to_recipe": "hark.fit.pipeline.codegen",
    "run_fit": "hark.fit.pipeline.cli",
    "run_morph": "hark.fit.pipeline.cli",
    "add_cli": "hark.fit.pipeline.cli",
}

__all__ = sorted(_API)


def __getattr__(name: str) -> Any:
    target = _API.get(name)
    if target is None:
        raise AttributeError(f"module 'hark.fit' has no attribute {name!r}")
    return getattr(importlib.import_module(target), name)
