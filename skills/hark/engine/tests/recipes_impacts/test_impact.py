"""Tests of part impact: families, search spaces, hall_ir, swell, measurement and study penalty."""

from __future__ import annotations

import math

import numpy as np
import pytest

from hark.forge.recipes.impacts import impact

SR = 48000


def test_families_are_version_3_with_punch():
    assert [n for n, _ in impact.FAMILIES["impact"]] == ["classic", "deep", "punch", "metal", "swell", "blast"]
    assert [n for n, _ in impact.FAMILIES["pro_impact"]] == ["rumble", "air", "short", "deep", "swell"]
    punch = dict(impact.FAMILIES["impact"])["punch"]
    assert set(punch) >= set(impact.SPACE)
    assert "pre_len" not in punch


def test_space_for_keys_and_limits():
    imp = impact.space_for("impact")
    assert list(imp) == impact.SEARCH_KEYS["impact"] and len(imp) == 23
    assert imp["roll_hold"] == (0.8, 3.0) and imp["hall_rt"] == (1.6, 6.0, "log")
    assert imp["crack"] == impact.SPACE["crack"]
    pro = impact.space_for("pro_impact")
    assert len(pro) == 14 and "width" not in pro and "air_f" not in pro
    assert pro["gate"] == (0.55, 1.35) and pro["gate_db"] == (12.0, 22.0)


def test_recipe_for():
    r = impact.recipe_for("pro_impact", "air")
    assert r.PRESET == "air" and r.REFSET == "pro_impact" and r.TARGET == impact.TARGETS["pro_impact"]
    assert r.SPACE == impact.space_for("pro_impact") and r.DEFAULTS == dict(impact.FAMILIES["pro_impact"])["air"]
    assert impact.recipe_for().PRESET == "classic"
    with pytest.raises(KeyError):
        impact.recipe_for("impact", "nope")
    with pytest.raises(IndexError):
        impact.recipe_for("impact", 99)


def test_hall_ir_contract():
    a = impact.hall_ir(2.0, 0.4, 0.8, 0.02)
    b = impact.hall_ir(2.0, 0.4, 0.8, 0.02)
    assert np.array_equal(a, b), "rng None uses a fixed seed"
    pre = round(0.02 * SR)
    assert np.all(a[:pre] == 0)
    assert len(a) == pre + round(min(5.0, 1.1 * 2.0 + 0.1) * SR)
    assert np.sum(a[pre:] ** 2) == pytest.approx(1.0, rel=1e-6)
    c = impact.hall_ir(2.0, 0.4, 0.8, 0.0, 0.5, rng=np.random.default_rng(7))
    assert len(c) == round(0.5 * SR)
    assert not np.array_equal(c, impact.hall_ir(2.0, 0.4, 0.8, 0.0, 0.5, rng=np.random.default_rng(8)))


def test_swell_ends_on_its_loudest_part():
    y = impact.swell(1.5, np.random.default_rng(1))
    assert y.shape == (round(1.5 * SR), 2)
    assert np.max(np.abs(y)) == pytest.approx(1.0)
    env = np.abs(y).max(axis=1)
    assert env[-round(0.3 * SR):].max() > 3 * env[: round(0.3 * SR)].max()


def test_version_2_set_renders(take):
    """A set with pre_len renders with version 2: the contact lands at pre_len, the level is left to the caller."""
    p = {**impact.V2_DEFAULTS, "pre_len": 0.6}
    y = take(impact.render, p)
    assert y.ndim == 2 and np.all(np.isfinite(y)) and np.max(np.abs(y)) > 1e-7
    env = np.abs(y).max(axis=1)
    assert np.argmax(env) >= round(0.6 * SR) - round(0.005 * SR)


def test_measure_keys(take):
    m = impact.measure(take(impact.render, dict(impact.FAMILIES["pro_impact"])["rumble"]))
    for key in ("dur", "attack_ms", "peak_ms", "t20", "t40", "slope_dBs", "len50", "gate_db40ms", "sh150",
                "tilt", "cen", "oct", "corr", "side_db", "ridge", "pre_gate"):
        assert key in m, key
    assert len(m["ridge"]) == 3
    mono = impact.measure(np.sin(2 * np.pi * 60 * np.arange(SR) / SR) * np.exp(-np.arange(SR) / SR / 0.2))
    assert mono["corr"] == 1.0 and mono["sh150"] > 0.9


def test_study_penalty_rules():
    assert impact.study_penalty({"t40": 1500}, {"t40": (1300, 2600)}) == 0.0
    assert impact.study_penalty({"t40": 650}, {"t40": (1300, 2600)}) == pytest.approx(1.0)
    assert impact.study_penalty({"tilt": -8.0}, {"tilt": (-6.5, -3.5)}) == pytest.approx(0.5)
    assert impact.study_penalty({"sh150": 0.5}, {"sh150": (0.65, 0.93)}) == pytest.approx(0.15 / 0.28)
    assert impact.study_penalty({"ridge": (80, 60, 40)}, {"ridge0": (55, 75), "ridge1": (38, 50)}) \
        == pytest.approx(math.log2(80 / 75))
    assert impact.study_penalty({"t40": None, "len50": float("nan")}, {"t40": (1, 2), "len50": (1, 2)}) == 0.0


def test_study_extra(take):
    x = take(impact.render, dict(impact.FAMILIES["pro_impact"])["rumble"])
    extra = impact.study_extra(impact.STUDY["pro_impact"], weight=2.0)
    assert extra(x) == pytest.approx(-2.0 * impact.study_penalty(impact.measure(x), impact.STUDY["pro_impact"]))


def test_pro_impact_members_near_the_pro_study(take):
    """The delivered pro_impact members stay close to the pro-study targets (penalty under 2 octaves/widths)."""
    for name, p in impact.FAMILIES["pro_impact"]:
        m = impact.measure(take(impact.render, p))
        assert impact.study_penalty(m, impact.STUDY["pro_impact"]) < 2.0, name
