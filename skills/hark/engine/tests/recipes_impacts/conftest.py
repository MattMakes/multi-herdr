"""Fixtures of the recipes_impacts area."""

from __future__ import annotations

import importlib

import numpy as np
import pytest

SR = 48000


@pytest.fixture
def take():
    """take(recipe_render, params, seed) -> float64 array from a generator of that seed."""

    def run(render, params, seed=1):
        return render(dict(params), np.random.default_rng(seed & 0xFFFFFFFF))

    return run


@pytest.fixture
def forge_render():
    """forge_render(recipe, params, seed): the forge search wrapper."""
    from hark.forge.engine import search

    return search.render


def load_recipe(name: str):
    """Import an impact recipe."""
    return importlib.import_module(f"hark.forge.recipes.impacts.{name}")


@pytest.fixture
def recipe_module():
    """recipe_module(name) -> the recipe module (see load_recipe)."""
    return load_recipe
