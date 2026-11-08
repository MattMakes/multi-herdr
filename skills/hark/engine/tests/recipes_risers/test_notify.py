"""notify (recipes-motion part 4, section 12): AT11 to AT16, the families, the knobs and the measures."""

from __future__ import annotations

import numpy as np
import pytest

from hark.forge.recipes.risers import notify


def test_at_11_pro_ding(render_of):
    p = notify.FAMILY["pro_notify"][0]
    assert notify.FAMILY_NAMES["pro_notify"][0] == "ding_1822"
    x = render_of(notify, p)
    assert len(x) == 12480
    m = notify.penalty_fields(x)
    assert 1772 <= m["head_hz"] <= 1872
    assert m["attack_ms"] <= 15
    assert m["low_db"] < -38
    assert x[0].tolist() == [0.0, 0.0]


def _has_peak(x: np.ndarray, f: float) -> bool:
    """A local spectral peak within 2 % of f that stands 10 dB over the median of its +-15 % neighbourhood."""
    m = x.mean(axis=1)
    P = np.abs(np.fft.rfft(m * np.hanning(len(m)))) ** 2
    freqs = np.fft.rfftfreq(len(m), 1 / 48000)
    near = (freqs >= 0.98 * f) & (freqs <= 1.02 * f)
    hood = (freqs >= 0.85 * f) & (freqs <= 1.15 * f)
    return 10 * np.log10(P[near].max() / np.median(P[hood])) > 10


def test_at_12_success_two_dings(render_of):
    p = notify.FAMILY_R3["pro_notify"][0]
    assert p["style"] == "success" and p["interval"] == 7
    x = render_of(notify, p)
    assert _has_peak(x, 1822.0)
    assert _has_peak(x, 1822.0 * 2 ** (7 / 12))


def test_at_13_round3_bell_with_edit(render_of):
    p = {**notify.FAMILY["notify"][0], **notify.R4_EDITS["notify"][0]}
    x = render_of(notify, p)
    assert len(x) == 110400
    m = notify.penalty_fields(x)
    assert m["sub_db"] <= -20
    assert notify.outside(m["sub_db"], -200, -20, 8) == 0
    assert m["attack_ms"] <= 10


def _without_knobs(p: dict) -> dict:
    return {k: v for k, v in p.items() if k not in ("ring", "soft", "bell_len")}


@pytest.mark.parametrize("p", [notify.FAMILY["pro_notify"][1], notify.FAMILY["notify"][2],
                               notify.FAMILY_R6["pro_notify"][3], notify.FAMILY_R3["pro_notify"][0]],
                         ids=["ding", "bell", "chime_up", "success"])
def test_at_14_neutral_knobs_change_nothing(render_of, p):
    bare = _without_knobs(p)
    with_knobs = {**bare, "ring": 1, "soft": 0, "bell_len": 0}
    assert np.array_equal(render_of(notify, bare), render_of(notify, with_knobs))


def test_at_15_ring_scaled():
    out = notify.ring_scaled({"style": "ding", "hold_tau": 0.1, "body_tau": 0.2, "decay": 0.5}, 2)
    assert out["hold_tau"] == pytest.approx(0.2)
    assert out["body_tau"] == pytest.approx(0.4)
    assert out["decay"] == 0.5
    same = {"style": "ding", "hold_tau": 0.1}
    assert notify.ring_scaled(same) == same and notify.ring_scaled(same) is not same
    assert notify.ring_scaled({"style": "other", "hold_tau": 0.1}, 3) == {"style": "other", "hold_tau": 0.1}


def test_at_16_family_space():
    s = notify.family_space(notify.FAMILY["pro_notify"][0])
    assert s["f0"] == [1822.0]
    assert s["style"] == ["ding"]
    assert s["att"] == (0.0008, 0.012, "log")
    assert s["interval"] == [7]
    assert set(s) == set(notify.SPACE)
    pinned = notify.family_space({"style": "unknown"})
    assert all(isinstance(v, list) and len(v) == 1 for v in pinned.values())


def test_ladder_and_family_tables():
    assert [round(f, 2) for f in notify.LADDER] == [1822.0, 1930.34, 2045.13, 2166.74, 2295.58]
    assert notify.FAMILY_NAMES["notify"][0] == "celesta C5"
    assert len(notify.R4_EDITS["notify"]) == 6 and notify.R4_EDITS["notify"][3] == {
        "strike_max": 12, "slope": 1.1, "hard": 0.48, "strike": -22}
    assert notify.FAMILY_R6["pro_notify"][0] == notify.FAMILY["pro_notify"][0]
    assert notify.FAMILY_R6["pro_notify"][3]["interval"] == -4
    assert notify.FAMILY_MID["mid_ding"][0]["c_mallet"] == -59
    assert notify.FAMILY_NAMES_R6["pro_notify"] == ["film_ding", "fm_bell", "wood", "ding_dong", "octave_body"]


@pytest.mark.parametrize("p", notify.FAMILY_R6["pro_notify"] + notify.FAMILY_MID["mid_ding"],
                         ids=notify.FAMILY_NAMES_R6["pro_notify"] + ["mid_tine", "mid_wood"])
def test_chime_members(render_of, p1, p):
    x = render_of(notify, p)
    assert len(x) == p1(p["length"])
    assert np.all(np.isfinite(x)) and np.max(np.abs(x)) < 0.999


def test_unknown_style_renders_the_ding(render_of):
    p = notify.FAMILY["pro_notify"][0]
    assert np.array_equal(render_of(notify, {**p, "style": "zzz"}), render_of(notify, {**p, "style": "ding"}))


def test_soft_onset_slows_the_attack(render_of):
    p = notify.FAMILY["pro_notify"][0]
    hard = notify.measure(render_of(notify, p))["attack_ms"]
    soft = notify.measure(render_of(notify, {**p, "soft": 0.06}))["attack_ms"]
    assert soft > hard


def test_bell_len_sets_the_bell_length(render_of, p1):
    p = {**notify.FAMILY["notify"][1], "bell_len": 1.2}
    assert len(render_of(notify, p)) == p1(1.2)
    short = {**notify.FAMILY["notify"][1], "length": 0.3}
    assert len(render_of(notify, short)) == p1(0.5)


def test_measure_fields(render_of):
    m = notify.measure(render_of(notify, notify.DEFAULTS))
    for key in ("length_s", "attack_ms", "t20_ms", "t40_ms", "peak_t", "shares", "tilt", "corr", "side_mid_db",
                "head_hz", "ridge"):
        assert key in m, key
    assert len(m["shares"]) == 6
    assert set(m["ridge"]) == {"start_hz", "end_hz", "max_hz", "min_hz", "sweep_oct"}
    tiny = notify.measure(np.zeros(100))
    assert tiny["attack_ms"] == 0 and "head_hz" not in tiny and "ridge" not in tiny


def test_outside_and_penalties(render_of):
    assert notify.outside(None, 0, 1, 1) == 1.0
    assert notify.outside(-1, 0, 1, 2) == 0.5
    assert notify.outside(3, 0, 1, 2) == 1.0
    assert notify.outside(0.5, 0, 1, 2) == 0.0
    x = render_of(notify, notify.FAMILY["pro_notify"][0])
    assert notify.pro_penalty(x, "unknown") == 0.0
    pen = notify.pro_penalty(x, "pro_notify")
    assert 0.0 <= pen <= 2.0 * len(notify.TARGETS["pro_notify"])
    assert notify.extra_for("pro_notify", 2.0)(x, None) == pytest.approx(-2.0 * pen)
