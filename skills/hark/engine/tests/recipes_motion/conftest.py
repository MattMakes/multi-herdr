"""Fixtures for the motion recipe tests (recipes-motion AT1-AT4, AT7, AT17-AT23, AT29)."""

from __future__ import annotations

import importlib
import types
from collections.abc import Callable

import numpy as np
import pytest

SR = 48000
RECIPES = ("whoosh", "swish", "swipe", "brightswish", "zip", "hover", "shimmer")


def load_recipe(name: str) -> types.ModuleType:
    return importlib.import_module(f"hark.forge.recipes.motion.{name}")


@pytest.fixture
def load() -> Callable[[str], types.ModuleType]:
    """load(name) -> the recipe module."""
    return load_recipe


@pytest.fixture
def p1() -> Callable[[float], int]:
    """P1: max(1, round(seconds x 48000))."""
    return lambda s: max(1, int(round(float(s) * SR)))


@pytest.fixture
def default_params() -> Callable[[types.ModuleType], dict]:
    """The AT1 parameter set: DEFAULTS, or family_params('hover_tick', 'high') for hover."""

    def make(module: types.ModuleType) -> dict:
        if module.__name__.endswith(".hover"):
            return module.family_params("hover_tick", "high")
        return dict(module.DEFAULTS)

    return make


@pytest.fixture
def band_share_db() -> Callable[[np.ndarray, float, float], float]:
    """band_share_db(x, lo, hi): energy of the mono fold in [lo, hi) Hz re the total, in dB."""

    def share(x: np.ndarray, lo: float, hi: float) -> float:
        m = x.mean(axis=1) if x.ndim == 2 else x
        power = np.abs(np.fft.rfft(m)) ** 2
        f = np.fft.rfftfreq(len(m), 1.0 / SR)
        band = power[(f >= lo) & (f < hi)].sum()
        return float(10 * np.log10(band / (power.sum() + 1e-20) + 1e-20))

    return share


def render(module: types.ModuleType, params: dict, seed: int = 1) -> np.ndarray:
    """The spec's "render X": module.render(p, default_rng(seed)) as a float array."""
    return np.asarray(module.render(dict(params), np.random.default_rng(seed)))


@pytest.fixture
def run() -> Callable[..., np.ndarray]:
    """run(module, params, seed=1) -> the raw recipe output."""
    return render
