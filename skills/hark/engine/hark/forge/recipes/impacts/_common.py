"""Small helpers that several impact recipes share.

The leading ``_`` keeps the recipe registry from listing this module as a recipe.
"""

from __future__ import annotations

import os
from typing import Any

import numpy as np

from hark.forge.engine import dsp

SR = 48000
TWO_PI = 2 * np.pi


def ref_root() -> str:
    """Folder of the reference sounds: ``HARK_REFS`` or ``refs``, with forward slashes."""
    root = os.environ.get("HARK_REFS", "refs").replace("\\", "/")
    return root.rstrip("/") or root


def _scalar(word: str) -> Any:
    """Read one table cell: an int, else a float, else the bare word."""
    for kind in (int, float):
        try:
            return kind(word)
        except ValueError:
            pass
    return word


def space_table(text: str) -> dict[str, Any]:
    """Build a search space from rows of ``name lo hi [scale]``.

    A row whose second cell is ``|`` lists choices instead: ``name | a b c``.
    """
    out: dict[str, Any] = {}
    for row in text.strip().splitlines():
        name, *cells = row.split()
        if cells[0] == "|":
            out[name] = [_scalar(c) for c in cells[1:]]
        else:
            out[name] = tuple(_scalar(c) for c in cells)
    return out


def value_table(text: str) -> dict[str, Any]:
    """Build a parameter set from ``name value`` pairs, several pairs per row allowed."""
    words = text.split()
    return {words[i]: _scalar(words[i + 1]) for i in range(0, len(words), 2)}


def decay(t: np.ndarray, tau: float) -> np.ndarray:
    """Exponential fall with time constant ``tau``."""
    return np.exp(-t / tau)


def osc(freq: float, t: np.ndarray, phase: float | None = None) -> np.ndarray:
    """A sine at ``freq``; the phase is added only when one is given."""
    arg = TWO_PI * freq * t
    return np.sin(arg if phase is None else arg + phase)


def times(n: int) -> np.ndarray:
    """Sample times in seconds of an ``n``-sample buffer."""
    return np.arange(n) / SR


def samples(seconds: float) -> int:
    """Sample count of ``seconds``, at least 1."""
    return max(1, int(round(seconds * SR)))


def lowpass(x: np.ndarray, f: float, order: int = 2) -> np.ndarray:
    return dsp.butter(x, "low", f, order)


def highpass(x: np.ndarray, f: float, order: int = 2) -> np.ndarray:
    return dsp.butter(x, "high", f, order)


def bandpass(x: np.ndarray, lo: float, hi: float, order: int = 2) -> np.ndarray:
    return dsp.butter(x, "band", (lo, hi), order)
