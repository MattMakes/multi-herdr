"""recipes-impacts part keys: keypress and typing (one key and the typed run)."""

from __future__ import annotations

import numpy as np
import pytest

from hark.forge.recipes.ui import keypress, typing

SR = 48000


def rng(seed: int = 1) -> np.random.Generator:
    return np.random.default_rng(seed)


def corr(y: np.ndarray) -> float:
    return float(np.corrcoef(y[:, 0], y[:, 1])[0, 1])


# -------------------------------------------------------------- keypress

def test_keypress_metadata():
    assert keypress.REFSET == "typing_key"
    names = [r.rsplit("/", 1)[1] for r in keypress.REFS]
    assert names == [f"kb_thock_0{i}.wav" for i in range(1, 5)] + ["kb_enter_01.wav", "kb_space_01.wav"]
    assert not hasattr(keypress, "FAMILIES")
    assert len(keypress.SPACE) == 51


def test_keypress_defaults_are_v1_with_v2_edits():
    edits = {"click_lvl": 0.6, "thock_tau": 0.05, "clip": 0.24, "top_lvl": 0.8, "top_lo": 2500.0,
             "top_tau": 0.0015, "jacket_lvl": 0.0, "jacket_dt": 0.008, "pre_gap": 0.002, "pre_len": 0.006,
             "width": 0.2}
    assert keypress.DEFAULTS == dict(keypress.V1_DEFAULTS, **edits)
    assert "top_lvl" not in keypress.V1_DEFAULTS and len(keypress.V1_DEFAULTS) == 44


def test_keypress_v1_set_renders_v1_path():
    v1 = keypress.render(dict(keypress.V1_DEFAULTS), rng())
    v2 = keypress.render(dict(keypress.DEFAULTS), rng())
    assert np.isfinite(v1).all() and v1.shape[1] == 2
    assert v1.shape != v2.shape or not np.array_equal(v1, v2)
    assert len(v1) / SR > 0.3


def test_keypress_key_rng_formula():
    a = keypress.key_rng(12, 5).uniform()
    b = np.random.default_rng(12 * 7919 + 5 * 104729 + 17).uniform()
    assert a == b


def test_keypress_layout_is_fixed_per_key():
    n = 2400
    a = keypress.cloud(2000, 8000, 10, 0.003, 0.002, n, keypress.key_rng(5, 0), rng(1), detune=0.0)
    b = keypress.cloud(2000, 8000, 10, 0.003, 0.002, n, keypress.key_rng(5, 0), rng(2), detune=0.0)
    c = keypress.cloud(2000, 8000, 10, 0.003, 0.002, n, keypress.key_rng(6, 0), rng(1), detune=0.0)
    spec = [np.abs(np.fft.rfft(x)) for x in (a, b, c)]
    same = np.corrcoef(spec[0], spec[1])[0, 1]
    other = np.corrcoef(spec[0], spec[2])[0, 1]
    assert same > other


def test_keypress_blocks():
    p = keypress.pulse(1e-4)
    assert len(p) == round(1e-4 * SR) and p.sum() == pytest.approx(1.0)
    assert len(keypress.hiss(1000, 4000, 0.001, 4800, rng())) == 4800
    ring = keypress.ring([1000, 0.48 * SR], [0.001, 0.001], [1, 1], 4800, rng())
    assert np.all(ring[int(9 * 0.001 * SR) + 2:] == 0)
    x = np.ones(1000)
    assert keypress.energy(x)[0] == pytest.approx(1.0)


def test_keypress_width_narrows_the_pair():
    wide = keypress.render(dict(keypress.DEFAULTS, width=0.6, floor_db=-78), rng())
    narrow = keypress.render(dict(keypress.DEFAULTS, width=0.0, floor_db=-78), rng())
    assert corr(narrow) > corr(wide)
    assert corr(narrow) > 0.95


def test_keypress_room_models():
    y = keypress.render(dict(keypress.DEFAULTS, room_model="diffuse"), rng())
    assert np.isfinite(y).all()
    with pytest.raises(ValueError):
        keypress.render(dict(keypress.DEFAULTS, room_model="hall"), rng())


def test_keypress_jacket_adds_lead():
    base = keypress.render(dict(keypress.DEFAULTS, jacket_lvl=0.0), rng())
    clicky = keypress.render(dict(keypress.DEFAULTS, jacket_lvl=0.6, jacket_dt=0.012), rng())
    assert len(clicky) > len(base)


def test_keypress_key_changes_the_board():
    a = keypress.render(dict(keypress.DEFAULTS, key=1), rng())
    b = keypress.render(dict(keypress.DEFAULTS, key=2), rng())
    assert a.shape != b.shape or not np.allclose(a, b)


def test_keypress_v2_penalty():
    y = keypress.render(dict(keypress.DEFAULTS), rng())
    s = keypress.key_stats(y)
    assert set(s) == {"onset_ms", "top_db", "t40_ms"}
    assert -1.5 <= keypress.key_penalty(y) <= 0
    assert keypress.extra_for("pro_click") is None
    assert keypress.extra_for("typing_key", 2.0)(y) == pytest.approx(2 * keypress.key_penalty(y))


def test_keypress_bright_top():
    y = keypress.render(dict(keypress.DEFAULTS), rng())
    assert keypress.key_stats(y)["top_db"] > -8


# ---------------------------------------------------------------- typing

def test_typing_metadata():
    assert typing.REFSET == "pro_typing_key"
    assert typing.RUN.REFSET == "pro_typing_run"
    assert typing.RUN.MODULE.rsplit(".", 1)[1] == "typing"
    assert set(typing.RUN_SPACE) - set(typing.SPACE) == {"rate", "jit", "lvl_jit", "accent", "gap", "var", "dur",
                                                         "swell_db", "swell_len"}
    assert len(typing.DEFAULTS) == 22


# Recorded once from key_rng(7) on numpy 2 and reviewed. It pins the per-key seed across versions:
# a changed seed formula changes every typed run that users already tuned.
KEY_7_FIRST_UNIFORM = 0.7605958890389746


def test_typing_key_rng_is_stable_per_key():
    assert np.array_equal(typing.key_rng(7).uniform(size=4), typing.key_rng(7).uniform(size=4))


def test_typing_key_rng_differs_between_keys():
    assert not np.array_equal(typing.key_rng(7).uniform(size=4), typing.key_rng(8).uniform(size=4))


def test_typing_key_rng_seed_is_pinned():
    assert typing.key_rng(7).uniform() == KEY_7_FIRST_UNIFORM


def test_typing_key_is_mono_and_high_passed():
    y = typing.render(dict(typing.DEFAULTS), rng())
    assert np.array_equal(y[:, 0], y[:, 1])
    s = typing.key_stats(y)
    assert s["lo2k_db"] <= -28
    assert 1.5 <= s["attack_ms"] <= 5.5
    assert typing.key_penalty(y) > -0.75


def test_typing_burst_envelope():
    e = typing.burst_env(480, 0.003, 1.0, 0.0025, 0.1, 0.016)
    a = round(0.003 * SR)
    assert e[0] == 0 and e[a - 1] == pytest.approx((a - 1) / a)
    assert e[a] == pytest.approx(0.9 * np.exp(-1 / SR / 0.0025) + 0.1 * np.exp(-1 / SR / 0.016))


def test_typing_release_extends_the_key():
    p = dict(typing.DEFAULTS, rel_db=-20, rel_dt=0.075, length=0.05)
    y = typing.render(p, rng())
    assert len(y) / SR >= 0.003 * 0.7 + 0.075 * 0.7 + 0.025


@pytest.mark.parametrize("edit", [{"grain": 1.0, "grain_rate": 300}, {"tink_db": 2, "tink_glide": 12},
                                  {"tink_db": 0, "tink_glide": -12}, {"rattle_db": -18}])
def test_typing_round3_keys(edit):
    y = typing.render(dict(typing.DEFAULTS, **edit), rng())
    assert np.isfinite(y).all() and np.max(np.abs(y)) > 1e-7


def test_typing_key_times_spacing():
    p = dict(typing.RUN_DEFAULTS)
    times = typing.key_times(p, rng())
    d = np.diff(times)
    med = 1 / p["rate"]
    assert np.all(d >= 0.62 * med - 1e-12) and np.all(d <= 1.75 * med + 1e-12)
    assert times[0] == 0 and times[-1] < p["dur"] - 0.07


def test_typing_layout_never_repeats():
    r = rng()
    prev = -1
    for _ in range(500):
        k = typing._pick_layout(r, prev)
        assert k != prev and 0 <= k < 5
        prev = k


def test_typing_run_statistics():
    y = typing.RUN.render(dict(typing.RUN_DEFAULTS), rng())
    assert np.array_equal(y[:, 0], y[:, 1])
    s = typing.run_stats(y)
    assert s["keys"] >= 10
    assert 72 <= s["med_ms"] <= 100
    assert s["lo2k_db"] <= -28
    assert typing.run_penalty(y) == pytest.approx(0.0)


def test_typing_extra_for():
    y = typing.render(dict(typing.DEFAULTS), rng())
    assert typing.extra_for("pro_typing_key")(y) == pytest.approx(typing.key_penalty(y))
    assert typing.extra_for("typing_key") is None
    assert typing.outside(None, 0, 1, 1) == 1.0 and typing.outside(3, 0, 1, 2) == 1.0
