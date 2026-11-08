"""zip (recipes-motion-sweep section 7): AT7, the trajectory and the pro_zip members."""

from __future__ import annotations

import numpy as np
import pytest


def test_at_7_zip_dc_removed(load, run):
    m = load("zip")
    x = run(m, m.DEFAULTS)
    peak = np.max(np.abs(x))
    assert np.all(np.abs(x.mean(axis=0)) <= 1e-9 * peak)


def test_trajectory_up_is_a_straight_log_line(load):
    m = load("zip")
    p = {**m.DEFAULTS, "shape": "up", "f0": 100.0, "f1": 1600.0, "bend": 0.0}
    f = m.trajectory(p, 101)
    assert f[0] == pytest.approx(100.0) and f[-1] == pytest.approx(1600.0)
    assert np.allclose(np.diff(np.log2(f)), np.log2(16) / 100)


def test_trajectory_fixes_a_too_small_sweep(load):
    m = load("zip")
    up = m.trajectory({**m.DEFAULTS, "shape": "up", "f0": 500.0, "f1": 600.0}, 50)
    down = m.trajectory({**m.DEFAULTS, "shape": "down", "f0": 2000.0, "f1": 1800.0}, 50)
    assert up[-1] == pytest.approx(2000.0)
    assert down[-1] == pytest.approx(500.0)


def test_trajectory_arc_peaks_and_returns(load):
    m = load("zip")
    p = {**m.DEFAULTS, "shape": "arc", "f0": 1000.0, "fpk": 3000.0, "pk_at": 0.45, "arc_back": 0.9}
    f = m.trajectory(p, 201)
    assert f.max() == pytest.approx(3000.0, rel=1e-3)
    assert f[-1] == pytest.approx(900.0)


def test_pro_zip_members_sweep(load, run):
    m = load("zip")
    for name, p in m.FAMILIES_BY_CAT["pro_zip"].items():
        x = run(m, p)
        assert np.all(np.isfinite(x)) and np.max(np.abs(x)) <= 1.0, name
        meas = m.measure(x)
        assert meas["oct_span"] > 0.5, name
        assert -3.0 <= m.target_check(x) <= 0.0, name
