"""Fixtures of the recipes_impacts area.

When a forge module of another unit (dsp, fx, tonal) is absent, this file
installs a test stand-in for it from forge_standins.py. A real module always
wins: the stand-in goes in only when the import fails.
"""

from __future__ import annotations

import importlib
import importlib.util
import pathlib
import sys
import types

import numpy as np
import pytest

HERE = pathlib.Path(__file__).parent
STANDIN_NAMES = ("hark.forge.engine.dsp", "hark.forge.engine.fx", "hark.forge.recipes.tonal")


def _load_standins() -> types.ModuleType:
    spec = importlib.util.spec_from_file_location("_impacts_forge_standins", HERE / "forge_standins.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def _absent(name: str) -> bool:
    try:
        importlib.import_module(name)
    except ModuleNotFoundError:
        return True
    return False


def _ensure_engine_package() -> None:
    if not _absent("hark.forge.engine"):
        return
    pkg = types.ModuleType("hark.forge.engine", "test stand-in package")
    pkg.__path__ = []
    sys.modules["hark.forge.engine"] = pkg
    importlib.import_module("hark.forge").engine = pkg


def _install() -> list[str]:
    missing = [n for n in STANDIN_NAMES if _absent(n)]
    if not missing:
        return []
    _ensure_engine_package()
    mods = _load_standins().build_modules()
    for name in missing:
        sys.modules[name] = mods[name]
        parent, _, leaf = name.rpartition(".")
        setattr(sys.modules[parent], leaf, mods[name])
    return missing


INSTALLED = _install()


def pytest_report_header(config):
    if INSTALLED:
        return "recipes_impacts stand-ins: " + ", ".join(INSTALLED)
    return "recipes_impacts stand-ins: none (real forge modules)"


SR = 48000


@pytest.fixture
def take():
    """take(recipe_render, params, seed) -> float64 array from a generator of that seed."""

    def run(render, params, seed=1):
        return render(dict(params), np.random.default_rng(seed & 0xFFFFFFFF))

    return run


def _search_render():
    try:
        from hark.forge.engine import search
    except ImportError:
        return _load_standins().search_render
    return search.render


@pytest.fixture
def forge_render():
    """forge_render(recipe, params, seed): the forge wrapper (real when present, else the stand-in)."""
    return _search_render()


def load_recipe(name: str):
    """Import an impact recipe; thump skips while its provider motion/whoosh is absent."""
    if name == "thump":
        pytest.importorskip("hark.forge.recipes.motion.whoosh")
    return importlib.import_module(f"hark.forge.recipes.impacts.{name}")


@pytest.fixture
def recipe_module():
    """recipe_module(name) -> the recipe module (see load_recipe)."""
    return load_recipe
