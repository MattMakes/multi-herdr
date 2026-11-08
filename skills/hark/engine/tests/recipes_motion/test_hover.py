"""hover (recipes-motion-tonal section 14): AT21 to AT23 and the no-merge rule."""

from __future__ import annotations

import math

import numpy as np
import pytest
from scipy import signal


def true_peak_db(x: np.ndarray) -> float:
    """Peak in dBFS after 4x oversampling (a true-peak estimate)."""
    up = signal.resample_poly(x, 4, 1, axis=0)
    return float(20 * np.log10(np.max(np.abs(up)) + 1e-20))


def test_at_21_family_params_and_space(load):
    m = load("hover")
    p = m.family_params("pro_hover_tick", "ping")
    assert p["ping"] == pytest.approx(0.75)
    assert p["ping_f"] == pytest.approx(math.sqrt(8000 * 14000), abs=0.05)
    assert p["ping_f"] == pytest.approx(10583.0, abs=0.05)
    assert p["length"] == pytest.approx(0.035)
    assert p["grains"] == 0.0 and p["detent"] == 0.0
    space = m.family_space("hover_tick", "mid")
    assert space["tick_f"] == (3000, 5000, "log")
    assert space["detent"] == [0.0]
    assert tuple(space["snap"]) == (1.0, 2.0)


def test_at_22_defaults(load, run, p1, band_share_db):
    m = load("hover")
    x = run(m, m.DEFAULTS)
    assert len(x) == p1(0.002 + 0.045) == 2256
    assert true_peak_db(x) <= -1.0 + 0.1
    assert band_share_db(x, 0.0, 500.0) < -30.0


def test_at_23_detent_extends_the_file(load, run, p1):
    m = load("hover")
    x = run(m, {**m.DEFAULTS, "detent": 0.5, "detent_dt": 0.012})
    assert len(x) == p1(0.002 + 0.012 + 5.76e-3 * 6 + 0.01) == 2811


def test_render_does_not_merge_defaults(load):
    m = load("hover")
    p = dict(m.DEFAULTS)
    del p["tick_f"]
    with pytest.raises(KeyError):
        m.render(p, np.random.default_rng(1))


def test_absent_contact_and_grains_mean_off(load, run):
    m = load("hover")
    full = {**m.DEFAULTS, "contact": 0.0, "grains": 0.0}
    bare = {k: v for k, v in full.items() if k not in ("contact", "contact_ms", "grains")}
    assert np.array_equal(run(m, full), run(m, bare))


@pytest.mark.parametrize("cat", ("pro_hover_tick", "hover_tick"))
def test_every_family_renders(cat, load, run):
    m = load("hover")
    for fam in m.FAMILIES[cat]:
        x = run(m, m.family_params(cat, fam))
        assert np.all(np.isfinite(x)) and 1e-4 <= np.max(np.abs(x)) <= 1.0, fam
