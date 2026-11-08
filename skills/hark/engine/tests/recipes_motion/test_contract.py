"""The recipe contract for the 7 motion recipes: AT1, AT2, and renders at the SPACE extremes."""

from __future__ import annotations

import numpy as np
import pytest

RECIPES = ("whoosh", "swish", "swipe", "brightswish", "zip", "hover", "shimmer")


def at_extreme(base: dict, space: dict, side: int) -> dict:
    """base with every range key at its low (side 0) or high (side 1) end."""
    out = dict(base)
    for key, spec in space.items():
        if isinstance(spec, tuple):
            out[key] = spec[side]
    return out


def choice_sets(base: dict, space: dict) -> list[dict]:
    """One set per value of each multi-value choice key, on the low and the high extreme."""
    sets = []
    for key, spec in space.items():
        if isinstance(spec, list) and len(spec) > 1:
            sets += [{**at_extreme(base, space, side), key: v} for v in spec for side in (0, 1)]
    return sets


def assert_sound(x: np.ndarray) -> None:
    assert x.ndim == 2 and x.shape[1] == 2
    assert np.all(np.isfinite(x))
    peak = float(np.max(np.abs(x)))
    assert 1e-4 <= peak <= 1.0


@pytest.mark.parametrize("name", RECIPES)
def test_contract_attributes(name, load):
    m = load(name)
    for attr in ("SPACE", "DEFAULTS", "TARGET", "ALTS", "render"):
        assert hasattr(m, attr), attr
    assert isinstance(m.TARGET, str) and m.TARGET
    assert isinstance(m.ALTS, list) and m.ALTS


@pytest.mark.parametrize("name", RECIPES)
def test_at_1_defaults_render(name, load, run, default_params):
    m = load(name)
    x = run(m, default_params(m))
    assert x.dtype == np.float64
    assert x.shape[0] >= 480
    assert_sound(x)


@pytest.mark.parametrize("name", RECIPES)
def test_at_2_deterministic_per_seed(name, load, run, default_params):
    m = load(name)
    p = default_params(m)
    a, b, c = run(m, p, 7), run(m, p, 7), run(m, p, 8)
    assert np.array_equal(a, b)
    assert a.shape != c.shape or not np.array_equal(a, c)


@pytest.mark.parametrize("name", RECIPES)
def test_space_extremes_render(name, load, run, default_params):
    m = load(name)
    base = default_params(m)
    sets = [at_extreme(base, m.SPACE, 0), at_extreme(base, m.SPACE, 1)] + choice_sets(base, m.SPACE)
    for p in sets:
        assert_sound(run(m, p))
