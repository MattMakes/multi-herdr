"""Fit analysis: the sines, modal, spectral and gesture fitters and the space layer.

This package imports nothing at module level. ``module_for`` imports a kind
module on first use.
"""

from __future__ import annotations

import importlib
from types import ModuleType

KINDS: dict[str, str] = {
    "sines": "hark.fit.analysis.sinusoidal",
    "modal": "hark.fit.analysis.modal",
    "spectral_noise": "hark.fit.analysis.spectral",
    "gesture": "hark.fit.analysis.gesture",
}


def module_for(kind: str) -> ModuleType:
    """The analysis module of a model kind value; ValueError for an unknown kind."""
    if kind not in KINDS:
        raise ValueError(f"unknown model kind {kind!r}; known: {sorted(KINDS)}")
    return importlib.import_module(KINDS[kind])
