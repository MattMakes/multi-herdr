"""riser, downlifter, darkriser and revcymbal (recipes-motion part 2): AT5, AT6, AT8 and the spec's shapes."""

from __future__ import annotations

import numpy as np
import pytest

from hark.forge.recipes.risers import darkriser, downlifter, revcymbal, riser


def _block_db(x: np.ndarray) -> np.ndarray:
    m = x.mean(axis=1)
    k = len(m) // 480
    return 10 * np.log10(np.mean(m[:k * 480].reshape(k, 480) ** 2, axis=1) + 1e-20)


def test_at_5_riser_build_ends_on_the_hit(render_of, p1):
    x = render_of(riser, riser.DEFAULTS)
    assert len(x) == p1(riser.DEFAULTS["length"])
    e = _block_db(x)
    assert np.argmax(e) >= 0.75 * len(e)


@pytest.mark.parametrize("name", list(riser.R6))
def test_riser_round6_build_ends_on_the_hit(render_of, p1, name):
    p = riser.R6[name]
    x = render_of(riser, p)
    assert len(x) == p1(p["length"])
    e = _block_db(x)
    assert np.argmax(e) >= 0.75 * len(e)


@pytest.mark.parametrize("cat", ["riser", "pro_riser"])
def test_riser_family_members_render(render_of, p1, cat):
    for name, p in riser.FAMILIES_BY_CAT[cat].items():
        x = render_of(riser, p)
        assert len(x) == p1(p["length"]), name
        assert np.all(np.isfinite(x)) and np.max(np.abs(x)) < 0.999, name


def test_riser_families_tables():
    assert list(riser.FAMILIES) == ["air", "tension", "stutter", "shepard", "rush", "cymbal"]
    assert riser.DEFAULTS == riser.FAMILIES["air"]
    assert riser.FAMILIES["cymbal"]["metal_db"] == 0 and riser.FAMILIES["air"]["drone_oct"] == 0.08
    rc = riser.FAMILIES_BY_CAT["riser"]["cymbal"]
    assert rc["metal_glide"] == 0.6 and rc["metal_db"] == -6 and rc["width"] == 0.75 and rc["length"] == 2.531
    assert riser.FAMILIES_BY_CAT["pro_riser"]["roll"]["drone_noise"] == 1.0
    assert len(riser.R4_EDITS["riser"]) == 5 and riser.R4_EDITS["riser"][1]["detune"] == 24
    assert riser.R6["swell"]["rswell_db"] == 0 and riser.R6["shepard"]["wave"] == "tri"
    assert all(p["r6"] == 1.0 for p in riser.R6.values())


def test_riser_dense_metal_renders(render_of, p1):
    p = {**riser.DEFAULTS, "metal_db": 0.0, "metal_n": 30, "metal_glide": 0.5}
    x = render_of(riser, p)
    assert len(x) == p1(p["length"]) and np.all(np.isfinite(x))


def test_riser_gate_is_open_on_the_last_sample():
    """P15 lays the phase out from the end; P16 at end phase duty / 2 is fully open."""
    n = 4800
    rate = np.full(n, 30.0)
    ph = riser.phase_from_end(rate, 0.25)
    assert ph[-1] == pytest.approx(0.25 - 30.0 / 48000)
    g = riser.gate_shape(np.array([0.25]), 1 / 30, duty=0.5, sharp=1.0)
    assert g[0] == pytest.approx(1.0)


def test_riser_unison_shape_and_level():
    f = np.full(4800, 440.0)
    y = riser.unison(f, "sine", 0.0, 0.0, np.zeros(5))
    assert y.shape == (4800, 2)
    assert np.allclose(y[:, 0], y[:, 1])
    assert np.max(np.abs(y)) == pytest.approx(np.sqrt(2) / 5 * 5 * np.cos(np.pi / 4), rel=1e-3)


def test_riser_verb_ir_fixed_and_cached():
    a = riser.verb_ir(1.04)
    b = riser.verb_ir(0.96)
    assert a.shape[1:] == (2, 2)
    assert np.array_equal(a, b)


def test_riser_measure_and_target_check(render_of):
    x = render_of(riser, riser.DEFAULTS)
    m = riser.measure(x)
    assert set(m) == {"length_s", "last_third", "end_db", "hf_rise_db", "cent_oct", "side_db", "low_db"}
    assert m["end_db"] > -6 and m["hf_rise_db"] > 10
    assert -3.0 <= riser.target_check(x) <= 0.0


def test_riser_side_db_of_mono_is_minus_99():
    assert riser.side_db(np.ones(100)) == -99.0


def test_at_6_downlifter_falls_from_the_start(render_of, p1):
    p = downlifter.DEFAULTS
    x = render_of(downlifter, p)
    assert len(x) <= p1(p["length"]) + p1(min(1.5, 0.6 * p["rt60"]))
    e = _block_db(x)
    assert np.argmax(e) < 0.4 * len(e)


@pytest.mark.parametrize("name", list(downlifter.FAMILIES))
def test_downlifter_members(render_of, p1, name):
    p = downlifter.FAMILIES[name]
    x = render_of(downlifter, p)
    assert len(x) <= p1(p["length"]) + p1(min(1.5, 0.6 * p["rt60"]))
    assert np.all(np.isfinite(x)) and np.max(np.abs(x)) < 0.999
    assert x[0].tolist() == [0.0, 0.0]


def test_downlifter_comb():
    x = np.zeros(100)
    x[10] = 1.0
    y = downlifter.comb(x, np.full(100, 5 / 48000), 0.5)
    assert y[10] == pytest.approx(1 / 1.5)
    assert y[15] == pytest.approx(0.5 / 1.5)


def test_downlifter_measure(render_of):
    x = render_of(downlifter, downlifter.DEFAULTS)
    m = downlifter.measure(x)
    assert m["peak_frac"] < 0.15 and m["cent_oct"] < 0
    assert -3.0 <= downlifter.target_check(x) <= 0.0


def test_at_8_darkriser_first_member(render_of, p1):
    name, p = darkriser.FAMILIES["riser"][0]
    assert name == "dark"
    x = render_of(darkriser, p)
    assert len(x) == p1(p["length"] + p["tail_tau"] * 60 / 8.686 + 0.02)
    assert np.max(np.abs(x)) == pytest.approx(0.89, abs=1e-9)


def test_darkriser_curves():
    p = darkriser.DEFAULTS
    T = p["length"]
    t = np.array([0.0, T, T + p["tail_tau"]])
    assert np.allclose(darkriser.level_db(p, t), [-p["start_db"], 0.0, -8.686])
    g = darkriser.gate(p, np.array([0.0, T - 0.0001, T + 0.01]))
    assert g[0] == 1.0 and g[2] == 1.0 and g[1] > 0.99


@pytest.mark.parametrize("name", [n for n, _ in darkriser.FAMILIES["riser"]])
def test_darkriser_members(render_of, name):
    p = dict(darkriser.FAMILIES["riser"])[name]
    x = render_of(darkriser, p)
    assert np.max(np.abs(x)) == pytest.approx(0.89, abs=1e-9)


def test_revcymbal_level_curve():
    p = revcymbal.DEFAULTS
    T, hd = p["length"], p["hold"]
    db, tp = revcymbal.level_db(p, np.array([0.0, T, T + hd, T + hd + p["tail_tau"]]))
    assert tp == T
    assert np.allclose(db, [-p["swell_db"], 0.0, p["hold_db"], p["hold_db"] - 8.686])


@pytest.mark.parametrize("name", [n for n, _ in revcymbal.FAMILIES["reverse_swell"]])
def test_revcymbal_members(render_of, p1, name):
    p = dict(revcymbal.FAMILIES["reverse_swell"])[name]
    x = render_of(revcymbal, p)
    tail = p["length"] + p["hold"] + p["tail_tau"] * 60 / 8.686 + 0.02
    assert len(x) <= p1(min(4.0, tail + 0.3 * p["rt60"] * min(1.0, 4 * p["room"])))
    assert np.max(np.abs(x)) == pytest.approx(0.89, abs=1e-9)
    peak_t = np.argmax(np.abs(x).max(axis=1)) / 48000
    assert peak_t == pytest.approx(p["length"], abs=0.12)


def test_revcymbal_width_sets_correlation(render_of):
    wide = render_of(revcymbal, dict(revcymbal.FAMILIES["reverse_swell"])["cymbal"])
    dark = render_of(revcymbal, dict(revcymbal.FAMILIES["reverse_swell"])["dark"])
    assert np.corrcoef(wide[:, 0], wide[:, 1])[0, 1] < 0.4
    assert np.corrcoef(dark[:, 0], dark[:, 1])[0, 1] > 0.8
