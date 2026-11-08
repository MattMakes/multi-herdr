"""Contract, determinism, peak and range tests for the 8 impact recipes (recipes-impacts index 8.1-8.3)."""

from __future__ import annotations

import numpy as np
import pytest

RECIPES = ["impact", "cuthit", "thump", "knock", "mallet", "subdrop", "drip", "pluck"]
NORMALISED = {"impact", "cuthit", "thump", "subdrop"}
PEAK = 0.89
MINUS_1_DBFS = 10 ** (-1 / 20)


def _defaults(mod, name: str) -> dict:
    p = dict(mod.DEFAULTS)
    if name == "pluck":
        p["f0"] = 523.25
    return p


def _corner(space: dict, end: int) -> dict:
    """Every key at its low (end 0) or high (end 1) bound; choices take the first or last value."""
    out = {}
    for key, spec in space.items():
        if isinstance(spec, list):
            out[key] = spec[0] if end == 0 else spec[-1]
        else:
            out[key] = spec[end]
    return out


def _check_audio(y: np.ndarray) -> None:
    assert y.dtype == np.float64
    assert y.ndim == 1 or (y.ndim == 2 and y.shape[1] == 2)
    assert len(y) > 0
    assert np.all(np.isfinite(y))
    assert np.max(np.abs(y)) > 1e-7


@pytest.mark.parametrize("name", RECIPES)
def test_8_1_contract(name, recipe_module, take):
    mod = recipe_module(name)
    for attr in ("SPACE", "DEFAULTS", "TARGET", "ALTS", "render"):
        assert hasattr(mod, attr), attr
    y = take(mod.render, _defaults(mod, name), 1)
    _check_audio(y)
    if name in NORMALISED:
        assert abs(np.max(np.abs(y)) - PEAK) < 1e-9


@pytest.mark.parametrize("name", RECIPES)
def test_8_2_determinism(name, recipe_module, take):
    mod = recipe_module(name)
    p = _defaults(mod, name)
    a, b = take(mod.render, p, 1), take(mod.render, p, 1)
    assert a.shape == b.shape and np.array_equal(a, b)
    c = take(mod.render, p, 2)
    assert a.shape != c.shape or not np.array_equal(a, c)


@pytest.mark.parametrize("name", ["cuthit", "subdrop", "thump"])
def test_8_3_peak_of_every_member(name, recipe_module, take):
    mod = recipe_module(name)
    for cat, members in mod.FAMILIES.items():
        for member, params in members:
            y = take(mod.render, params, 1)
            assert abs(np.max(np.abs(y)) - PEAK) < 1e-9, (cat, member)


@pytest.mark.parametrize("name", RECIPES)
@pytest.mark.parametrize("end", [0, 1], ids=["low", "high"])
def test_space_extremes_render(name, end, recipe_module, take, forge_render):
    """Every key at a bound of SPACE renders finite audio; the forge wrapper output stays at or below -1 dBFS."""
    mod = recipe_module(name)
    p = {**_defaults(mod, name), **_corner(mod.SPACE, end)}
    y = take(mod.render, p, 3)
    _check_audio(y)
    if name in NORMALISED:
        assert np.max(np.abs(y)) <= PEAK + 1e-9
    w = forge_render(mod, p, 3)
    assert w is not None
    assert np.max(np.abs(w)) <= MINUS_1_DBFS + 1e-6


@pytest.mark.parametrize("name", RECIPES)
def test_forge_wrapper_level(name, recipe_module, forge_render):
    """Raw renders keep the spec level; the forge wrapper output peaks at -1 dBFS."""
    mod = recipe_module(name)
    w = forge_render(mod, _defaults(mod, name), 1)
    assert w is not None
    assert abs(float(np.max(np.abs(w))) - MINUS_1_DBFS) < 1e-4


@pytest.mark.parametrize("name", RECIPES)
def test_family_members_render(name, recipe_module, take):
    mod = recipe_module(name)
    sets = [p for members in getattr(mod, "FAMILIES", {}).values() for _, p in members]
    sets += [p for members in getattr(mod, "FAMILY", {}).values() for p in members]
    for p in sets:
        _check_audio(take(mod.render, p, 1))
