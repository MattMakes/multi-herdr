"""The recipe contract for the 8 riser-folder recipes: AT1, AT2, and renders at the parameter extremes."""

from __future__ import annotations

import numpy as np
import pytest

RISERS = ("riser", "darkriser", "downlifter", "revcymbal", "glitch", "burstglitch", "notify", "sting")


def _space_value(spec, end: int):
    """The low (end 0) or high (end 1) edge of a SPACE spec."""
    if isinstance(spec, list):
        return spec[-1] if end else spec[0]
    lo, hi = spec[0], spec[1]
    v = hi if end else lo
    return int(v) if len(spec) > 2 and spec[2] == "int" else float(v)


def _corner(module, end: int) -> dict:
    p = dict(module.DEFAULTS)
    p.update({k: _space_value(spec, end) for k, spec in module.SPACE.items()})
    return p


@pytest.mark.parametrize("name", RISERS)
def test_contract_attributes(risers, name):
    m = risers[name]
    for attr in ("SPACE", "DEFAULTS", "TARGET", "ALTS", "render"):
        assert hasattr(m, attr), attr
    assert isinstance(m.TARGET, str) and m.TARGET
    assert isinstance(m.ALTS, list) and all(isinstance(a, str) for a in m.ALTS)


@pytest.mark.parametrize("name", RISERS)
def test_at_1_defaults_render(risers, name, render_of):
    x = render_of(risers[name], risers[name].DEFAULTS)
    assert x.dtype == np.float64
    assert x.ndim == 2 and x.shape[1] == 2
    assert x.shape[0] >= 480
    assert np.all(np.isfinite(x))
    assert np.max(np.abs(x)) >= 1e-4


@pytest.mark.parametrize("name", RISERS)
def test_at_2_deterministic_per_seed(risers, name, render_of):
    m = risers[name]
    a = render_of(m, m.DEFAULTS, seed=7)
    b = render_of(m, m.DEFAULTS, seed=7)
    c = render_of(m, m.DEFAULTS, seed=8)
    assert np.array_equal(a, b)
    assert a.shape != c.shape or not np.array_equal(a, c)


def _expected_n(name: str, p: dict, p1) -> tuple[int, int]:
    """(lowest, highest) sample count the spec allows for a parameter set of one recipe."""
    if name == "riser":
        return p1(p["length"]), p1(p["length"])
    if name == "darkriser":
        n = p1(p["length"] + p["tail_tau"] * 60 / 8.686 + 0.02)
        return n, n
    if name == "downlifter":
        return 1, p1(p["length"]) + p1(min(1.5, 0.6 * p["rt60"]))
    if name == "revcymbal":
        tail = p["length"] + p["hold"] + p["tail_tau"] * 60 / 8.686 + 0.02
        return 1, p1(min(4.0, tail + 0.3 * p["rt60"] * min(1.0, 4 * p["room"])))
    if name == "glitch":
        n = p1(p["dur"]) + p1(4 * p["end_tail"])
        return n, n
    if name == "burstglitch":
        n = p1(p["dur"]) + p1(5 * p["end_tail"])
        return n, n
    if name == "notify":
        n = p1(p["bell_len"] if p["style"] == "bell" and p["bell_len"] > 0 else
               max(p["length"], 0.5) if p["style"] == "bell" else p["length"])
        return n, n
    return 480, p1(12.0)


@pytest.mark.parametrize("end", [0, 1], ids=["low", "high"])
@pytest.mark.parametrize("name", RISERS)
def test_extremes_render_clean(risers, name, end, render_of, p1):
    """Every SPACE key at its low (or high) edge: finite, no clipping, the spec's sample count."""
    m = risers[name]
    p = _corner(m, end)
    x = render_of(m, p)
    assert np.all(np.isfinite(x))
    assert np.max(np.abs(x)) < 0.999
    lo, hi = _expected_n(name, p, p1)
    assert lo <= len(x) <= hi


@pytest.mark.parametrize("name", RISERS)
def test_defaults_do_not_clip(risers, name, render_of, p1):
    m = risers[name]
    x = render_of(m, m.DEFAULTS)
    assert np.max(np.abs(x)) < 0.999
    lo, hi = _expected_n(name, {**m.DEFAULTS}, p1) if name != "sting" else (480, p1(12.0))
    assert lo <= len(x) <= hi


@pytest.mark.parametrize("name", ["darkriser", "revcymbal", "burstglitch"])
def test_recipe_for_objects(risers, name):
    m = risers[name]
    cat = next(iter(m.FAMILIES))
    first, last = m.FAMILIES[cat][0], m.FAMILIES[cat][-1]
    a = m.recipe_for(cat, 0)
    b = m.recipe_for(cat, last[0])
    assert a.PRESET == first[0] and a.DEFAULTS == first[1]
    assert b.PRESET == last[0] and b.DEFAULTS == last[1]
    assert a.MODULE == f"hark.forge.recipes.risers.{name}"
    assert a.render is m.render and a.SPACE == m.SPACE and a.REFSET == m.REFSET


def test_registry_names_unique(risers):
    """The flat names of this folder do not clash with each other (the registry requires unique names)."""
    assert len({m.__name__.rsplit(".", 1)[-1] for m in risers.values()}) == 8
