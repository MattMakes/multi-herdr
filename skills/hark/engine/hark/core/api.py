"""One flat, lazy namespace with the public function of every hark unit.

Each name imports its target module on first access.
"""

from __future__ import annotations

import importlib
from typing import Any

_TARGETS: dict[str, tuple[str, str]] = {
    "describe": ("hark.listen.describe", "describe"),
    "match": ("hark.listen.match", "match"),
    "rank": ("hark.listen.search", "rank"),
    "find": ("hark.listen.search", "find"),
    "compare": ("hark.listen.compare", "compare"),
    "ab": ("hark.listen.compare", "ab"),
    "realness": ("hark.listen.realness", "realness"),
    "realness_many": ("hark.listen.realness", "realness_many"),
    "profile": ("hark.listen.profile", "profile"),
    "timeline": ("hark.timeline.timeline", "timeline"),
    "sheet": ("hark.timeline.sheet", "sheet"),
    "critique": ("hark.timeline.critique", "critique"),
    "measure": ("hark.core.features", "measure"),
    "doctor": ("hark.doctor", "check"),
    "llm_critique": ("hark.surfaces.llm", "critique"),
    "sfx_list": ("hark.forge.bank.sfx", "sfx_list"),
    "sfx_render": ("hark.forge.bank.sfx", "sfx_render"),
    "sfx_check": ("hark.forge.bank.sfx", "sfx_check"),
    "kit_init": ("hark.forge.bank.kit", "init"),
    "kit_render": ("hark.forge.bank.kit", "render"),
    "kit_check": ("hark.forge.bank.kit", "check"),
    "kit_audition": ("hark.forge.bank.kit", "audition"),
}

__all__ = sorted(_TARGETS)


def target(name: str) -> tuple[str, str]:
    """(module, function) behind an API name; KeyError for an unknown name."""
    return _TARGETS[name]


def __getattr__(name: str) -> Any:
    if name not in _TARGETS:
        raise AttributeError(f"module 'hark.api' has no attribute {name!r}")
    module, attr = _TARGETS[name]
    return getattr(importlib.import_module(module), attr)


def __dir__() -> list[str]:
    return sorted(set(globals()) | set(__all__))
