"""Tests of part tuned: mallet, drip, pluck (recipes-impacts index 8.10-8.16 and part tuned)."""

from __future__ import annotations

import numpy as np
import pytest

from hark.forge.recipes import tonal
from hark.forge.recipes.impacts import drip, mallet, pluck

SR = 48000


def _median_measure(mod, params: dict, takes: int = 8) -> dict:
    ms = [tonal.measure(mod.render(dict(params), np.random.default_rng(s))) for s in range(1, takes + 1)]
    out = {}
    for key in ms[0]:
        vals = [m[key] for m in ms if m[key] is not None]
        out[key] = float(np.median(vals)) if vals else None
    return out


def _inside(m: dict, study: dict) -> int:
    return sum(1 for k, (lo, hi, _) in study.items() if m.get(k) is not None and lo <= m[k] <= hi)


def _member(mod, cat: str, name: str) -> dict:
    return dict(mod.FAMILY[cat][mod.FAMILY_NAMES[cat].index(name)])


def _envelope_db(y: np.ndarray) -> np.ndarray:
    m = y.mean(axis=1) if y.ndim == 2 else y
    frames = len(m) // 48
    e = np.sqrt(np.mean(m[: frames * 48].reshape(frames, 48) ** 2, axis=1))
    return 20 * np.log10(e / e.max() + 1e-12)


# ---- mallet

def test_8_10_mallet_gate(take):
    glock = {**_member(mallet, "mallet", "glock"), "jit": 0, "room": 0}
    # The spec prints 0.39018 s, but its own formula gives 0.08478 + 0.2954 + 0.02 = 0.40018 s.
    assert mallet.length_of(glock) == pytest.approx(min(2, 0.08478 + 7 * 0.0422 + 0.02, 0.00162 + 7 * 0.5027 + 0.02))
    assert mallet.length_of(glock) == pytest.approx(0.40018, abs=1e-9)
    db = _envelope_db(take(mallet.render, glock))
    assert np.all(db[500:] < -40)
    assert len(db) < 500 or db[:500].max() == 0


def test_8_11_run_frequencies():
    up = {"f0": 1046.5, "scale": "penta", "notes": 4, "span": 2, "pattern": "up"}
    assert mallet.run_semitones({**mallet.RUN_DEFAULTS, **up}) == [0, 4, 9, 14]
    fs = mallet.run_freqs(up)
    assert fs == pytest.approx([1046.5 * 2 ** (s / 12) for s in (0, 4, 9, 14)])
    arc = {"f0": 1046.5, "scale": "lydian", "notes": 3, "span": 2, "pattern": "arc"}
    assert mallet.run_semitones({**mallet.RUN_DEFAULTS, **arc}) == [0, 4, 0]


def test_run_down_and_even_arc():
    base = {**mallet.RUN_DEFAULTS, "scale": "penta", "span": 1}
    assert mallet.run_semitones({**base, "notes": 3, "pattern": "down"}) == [4, 2, 0]
    assert mallet.run_semitones({**base, "notes": 4, "pattern": "arc"}) == [0, 2, 2, 0]
    with pytest.raises(ValueError):
        mallet.run_semitones({**base, "scale": "dorian"})


def test_mallet_family_and_space():
    assert mallet.FAMILY_NAMES["mallet"] == ["glock", "pluck", "celesta", "marimba"]
    glock = mallet.FAMILY["mallet"][0]
    space = mallet.family_space(glock)
    for key in ("style", "bar", "f0", "pan", "oct_ratio", "jit"):
        assert space[key] == [glock[key]]
    for key in mallet.FINE:
        assert space[key] == mallet.SPACE[key]
    assert set(mallet.STUDY) == set(mallet.STUDY_PLUCK)


def test_mallet_members_meet_their_study(take):
    """glock lands inside at least 6 of 7 bell-note ranges; the pluck member at least 5 of the pluck ranges."""
    assert _inside(_median_measure(mallet, _member(mallet, "mallet", "glock")), mallet.STUDY) >= 6
    assert _inside(_median_measure(mallet, _member(mallet, "mallet", "pluck")), mallet.STUDY_PLUCK) >= 5


def test_mallet_pitch_never_moves(take):
    p = {**mallet.DEFAULTS, "bar": "tine", "jit": 1.0, "oct": -60, "strike": -60, "room": 0, "width": 0}
    for seed in (1, 2, 3):
        y = take(mallet.render, p, seed).mean(axis=1)
        spec = np.abs(np.fft.rfft(y, 8 * len(y)))
        f = np.fft.rfftfreq(8 * len(y), 1 / SR)
        assert f[np.argmax(spec)] == pytest.approx(1046.5, rel=0.01)


def test_mallet_errors(take):
    with pytest.raises(ValueError):
        take(mallet.render, {"bar": "bell"})
    with pytest.raises(ValueError):
        take(mallet.render, {"style": "bowed"})


def test_mallet_high_partials_skipped(take):
    y = take(mallet.render, {"f0": 3200, "bar": "glock", "lp": 16000, "room": 0}).mean(axis=1)
    assert np.all(np.isfinite(y))


def test_chime_runs(take):
    assert mallet.RUN_NAMES["chime_run"] == ["flourish", "up4"]
    assert mallet.RUN.render is mallet.render_run and mallet.RUN.render.__name__ == "render_run"
    assert mallet.RUN.MODULE == mallet.__name__ and mallet.RUN.TARGET == "a quick run of chime notes"
    for p in mallet.RUNS["chime_run"]:
        y = take(mallet.render_run, p)
        assert y.ndim == 2 and np.all(np.isfinite(y)) and np.max(np.abs(y)) > 1e-7
        assert np.max(np.abs(y[-4:])) < 1e-2
    a = take(mallet.render_run, mallet.RUN_DEFAULTS, 1)
    assert np.array_equal(a, take(mallet.render_run, mallet.RUN_DEFAULTS, 1))


# ---- drip

def test_8_13_drip_allowance():
    assert drip.allows({}) and drip.allows(None)
    assert drip.allows({"rise_st": 20, "from_s": None, "to_s": 0.2})
    assert drip.allows({"rise_st": 5, "from_s": 0.01, "to_s": 0.15})
    assert not drip.allows({"rise_st": 15, "from_s": 0.01, "to_s": 0.15})
    assert not drip.allows({"rise_st": 5, "from_s": 0.08, "to_s": 0.2})
    assert not drip.allows({"rise_st": 5, "from_s": 0.0, "to_s": 0.3})
    assert drip.RISES is True and drip.RISE_MAX_ST == 14.0 and drip.RISE_MAX_S == 0.25 and drip.RISE_START_S == 0.06


def test_8_14_drip_profile():
    assert _inside(_median_measure(drip, _member(drip, "drip", "drop")), drip.STUDY) >= 4


def test_drip_family_space():
    drop = drip.FAMILY["drip"][0]
    space = drip.family_space(drop)
    for key in ("f0", "rise", "daughter", "jit"):
        assert space[key] == [drop[key]]
    for key in drip.FINE:
        assert space[key] == drip.SPACE[key]
    assert drip.FAMILY_NAMES["drip"] == ["drop", "bubble", "plink", "bloop", "tiny"]


def test_drip_length_cap():
    assert drip.length_of({**drip.DEFAULTS, "tau": 0.5, "daughter": -3}) == 1.0
    assert drip.length_of(drip.DEFAULTS) == pytest.approx(0.02 + 7 * 0.05 + 0.02)


def test_drip_pitch_rises(take):
    """The bubble pitch climbs from f0 toward f0 x 2^(rise/12)."""
    p = {**drip.DEFAULTS, "jit": 0, "breath": -60, "click": -60, "thump": -60, "h2": 0, "room": 0,
         "width": 0, "rise": 12, "rtau": 0.02, "tau": 0.1}
    y = take(drip.render, p).mean(axis=1)
    early = y[: round(0.01 * SR)]
    late = y[round(0.12 * SR): round(0.2 * SR)]

    def peak_hz(seg):
        spec = np.abs(np.fft.rfft(seg * np.hanning(len(seg)), 1 << 16))
        return np.fft.rfftfreq(1 << 16, 1 / SR)[np.argmax(spec)]

    assert peak_hz(late) > 1.4 * peak_hz(early)


# ---- pluck

def test_8_12_note_names():
    assert round(pluck.note("C5"), 4) == 523.2511
    assert round(pluck.note("A4"), 4) == 440.0
    assert round(pluck.note("F#4"), 4) == 369.9944
    assert round(pluck.note("Bb3"), 4) == 233.0819
    assert round(pluck.note("c5"), 4) == 523.2511
    with pytest.raises(ValueError):
        pluck.note("H4")
    with pytest.raises(ValueError):
        pluck.note("C")


def _strongest(y: np.ndarray, lo: float = 300, hi: float = 1000) -> float:
    m = y.mean(axis=1)[: 2 * SR]
    spec = np.abs(np.fft.rfft(m, 1 << 18))
    f = np.fft.rfftfreq(1 << 18, 1 / SR)
    band = (f >= lo) & (f <= hi)
    return float(f[band][np.argmax(spec[band])])


def test_8_15_pluck_pitch(take):
    for seed in range(1, 9):
        f = _strongest(take(pluck.render, pluck.DEFAULTS, seed))
        assert min(abs(f / r - 1) for r in pluck.REF_F0) <= 0.01
    assert _strongest(take(pluck.render, {**pluck.DEFAULTS, "f0": pluck.note("C5")})) == pytest.approx(523.25, rel=0.01)


def bank_members() -> dict[str, tuple[dict, int]]:
    """The 3 members of bank/pluck.json, built from the defaults by the edits of part seeds S3.3."""
    c5 = 440 * 2 ** (3 / 12)
    d = {**pluck.DEFAULTS, "f0": c5}
    soft = {**d, "p2_lvl": d["p2_lvl"] * 0.75, "thumb_ms": 1.4, "zing_lvl": d["zing_lvl"] * 0.5,
            "skin_lvl": d["skin_lvl"] * 0.4, "knock_lvl": d["knock_lvl"] * 0.5}
    ring = {**d, "tau1": d["tau1"] * 1.5}
    return {"kalimba": (d, 8001), "kalimba_soft": (soft, 8009), "kalimba_ring": (ring, 8006)}


def test_8_16_pluck_bank(take):
    members = bank_members()
    soft, seed = members["kalimba_soft"]
    assert seed == 8009 and soft["p2_lvl"] == pytest.approx(0.2895) and soft["knock_lvl"] == pytest.approx(0.28215)
    ring, seed = members["kalimba_ring"]
    assert seed == 8006 and ring["tau1"] == pytest.approx(0.52935)
    plain, seed = members["kalimba"]
    assert seed == 8001 and plain["f0"] == 523.2511306011972
    assert all(set(p) == set(pluck.SPACE) | {"f0"} for p, _ in members.values())
    for p, seed in members.values():
        y = take(pluck.render, p, seed)
        assert np.all(np.isfinite(y)) and len(y) <= 3 * SR


def test_pluck_space_and_tines():
    assert len(pluck.SPACE) == 30 and "f0" not in pluck.SPACE
    assert len(pluck.TINES) == 17
    assert pluck.TINES[0] == pytest.approx(261.63) and pluck.TINES[-1] == pytest.approx(261.63 * 2 ** (28 / 12))
    assert len(pluck.REFS) == 5 and all("/kalimba/" in r for r in pluck.REFS)


def test_pluck_merges_defaults_and_guards_nyquist(take):
    """Q-PLUCK-1: a partial dict renders. Q-PLUCK-2: a 2nd mode at or above 0.45 x 48000 Hz is skipped."""
    y = take(pluck.render, {"f0": 440.0})
    assert y.shape == (3 * SR, 2) or len(y) <= 3 * SR
    modes = pluck._tine_modes({**pluck.DEFAULTS, "p2_ratio": 7.0}, 3500.0, 0.3, lambda f: 1.0)
    assert all(f < 0.45 * SR for f, _, _ in modes)
    assert np.all(np.isfinite(take(pluck.render, {"f0": 3500.0, "p2_ratio": 7.0})))


def test_pluck_level_left_to_caller(take):
    y = take(pluck.render, {"f0": 523.25})
    assert len(y) <= 3 * SR
    assert np.max(np.abs(y[-4:])) < 1e-2 * np.max(np.abs(y))
