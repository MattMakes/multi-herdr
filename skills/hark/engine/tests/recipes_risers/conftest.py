"""Fixtures of the recipes_risers area."""

from __future__ import annotations

import importlib
import importlib.util

import numpy as np
import pytest

RISERS = ("riser", "darkriser", "downlifter", "revcymbal", "glitch", "burstglitch", "notify", "sting")


@pytest.fixture(scope="session")
def risers() -> dict:
    """The 8 recipe modules of the folder, by flat name."""
    return {name: importlib.import_module(f"hark.forge.recipes.risers.{name}") for name in RISERS}


@pytest.fixture(scope="session")
def p1():
    """P1: samples of a duration, at least 1."""
    return lambda s: max(1, int(round(s * 48000)))


@pytest.fixture(scope="session")
def wrapper():
    """The forge-core render wrapper: render(recipe, params, seed) -> -1 dBFS array or None."""
    return importlib.import_module("hark.forge.engine.search").render


@pytest.fixture
def render_of():
    """render_of(module, p, seed=1) -> the raw recipe output for a parameter set."""

    def run(module, p, seed: int = 1):
        return module.render(dict(p), np.random.default_rng(seed))

    return run
