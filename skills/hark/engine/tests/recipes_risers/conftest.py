"""Fixtures of the recipes_risers area.

When a module of another unit is absent (the forge engine of T-16, the impact recipes of 17b), this file installs a
test-only stand-in from standin_engine.py. It never replaces a real module. The terminal summary names every
stand-in in use, so a run that depends on one is visible.
"""

from __future__ import annotations

import importlib
import importlib.util
import pathlib
import sys
import types

import numpy as np
import pytest

_HERE = pathlib.Path(__file__).parent
STANDINS: list[str] = []


def _load_standin_file() -> types.ModuleType:
    spec = importlib.util.spec_from_file_location("_risers_standin_engine", _HERE / "standin_engine.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def _ensure_package(name: str) -> None:
    try:
        importlib.import_module(name)
    except ModuleNotFoundError:
        pkg = types.ModuleType(name)
        pkg.__path__ = []
        sys.modules[name] = pkg


def _install(name: str, builder) -> None:
    try:
        importlib.import_module(name)
        return
    except ModuleNotFoundError as exc:
        if exc.name is None or not name.startswith(exc.name):
            raise
    parent, _, leaf = name.rpartition(".")
    _ensure_package(parent)
    mod = builder()
    sys.modules[name] = mod
    setattr(sys.modules[parent], leaf, mod)
    STANDINS.append(name)


def _install_all() -> None:
    std = _load_standin_file()
    _install("hark.forge.engine.dsp", std.build_dsp)
    _install("hark.forge.engine.fx", std.build_fx)
    _install("hark.forge.engine.search", std.build_search)
    _install("hark.forge.recipes.impacts.impact", std.build_impact)
    _install("hark.forge.recipes.impacts.subdrop", std.build_subdrop)


_install_all()


def pytest_terminal_summary(terminalreporter, exitstatus, config) -> None:
    if STANDINS:
        terminalreporter.write_line(
            "recipes_risers: STAND-INS IN USE (real modules absent): " + ", ".join(STANDINS), yellow=True)


@pytest.fixture(scope="session")
def standins() -> list[str]:
    return list(STANDINS)


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
