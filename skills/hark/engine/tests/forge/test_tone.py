"""Acceptance tests T1-T10 of forge-core part 3 (tone and envelope checks)."""

import numpy as np

from hark.forge.engine import dsp, tone

SR = 48000


def _white(seed: int, sec: float = 1.0) -> np.ndarray:
    return np.random.default_rng(seed).standard_normal(int(SR * sec))


def _pink(seed: int, sec: float = 1.0) -> np.ndarray:
    return dsp.noise(sec, dsp.rng(seed), "pink")


def _decay(tau: float, sec: float = 1.0, seed: int = 0, lead: float = 0.0) -> np.ndarray:
    t = np.arange(int(SR * sec)) / SR
    y = _white(seed, sec) * np.exp(-np.maximum(t - lead, 0) / tau)
    y[t < lead] = 0.0
    return y


def test_t1_white_profile_slope():
    p = tone.profile(_white(1))
    assert np.all(p >= -60)
    cen = np.asarray(tone.CEN)
    sel = cen > 100
    k = np.arange(len(cen))[sel]
    fit = np.polyval(np.polyfit(k, p[sel], 1), k)
    slope = np.polyfit(k, p[sel], 1)[0]
    assert 0.8 < slope < 1.2
    assert np.max(np.abs(p[sel] - fit)) < 1.5


def test_t2_profile_level_independent():
    x = _pink(3)
    assert np.allclose(tone.profile(x), tone.profile(0.01 * x), atol=1e-9)


def test_t3_t40_of_exponential_decay():
    t = np.arange(SR) / SR
    x = np.sin(2 * np.pi * 1000 * t) * np.exp(-t / 0.1)
    v = tone.t40_ms(x)
    assert abs(v - 460) <= 5 + 0.02 * 460


def test_t4_toneref_white_and_pink():
    ref = tone.ToneRef([_white(s) for s in (10, 11, 12, 13)])
    m = ref.check(_white(20))
    assert m["tone_db"] < 1.0
    assert 0.3 <= m["tone_ratio"] <= 3
    assert m["outside"] == []
    mp = ref.check(_pink(21))
    assert mp["tone_ratio"] > 5
    low = [o for o in mp["outside"] if o[0].endswith(" Hz")]
    high = [o for o in mp["outside"] if o[0].endswith("kHz") and float(o[0].split()[0]) >= 4]
    assert low and all(o[2] == "above" for o in low)
    assert high and all(o[2] == "below" for o in high)


def test_t5_tone_penalty():
    ref = tone.ToneRef([_white(s) for s in (10, 11, 12, 13)])
    b = float(np.max(ref.t40))
    assert ref.penalty({"tone_ratio": 0.9, "t40_ms": b * 0.5, "t40_ref": (b * 0.4, b)}) == 0
    assert abs(ref.penalty({"tone_ratio": 1.5, "t40_ms": b * 0.5, "t40_ref": (b * 0.4, b)}) - 0.5) < 1e-12
    assert tone.ToneRef([]).penalty({}) == 0


def test_t6_bandgram_of_tone_burst():
    t = np.arange(int(0.3 * SR)) / SR
    x = np.sin(2 * np.pi * 1000 * t) * (t < 0.2)
    G = tone.bandgram(x)
    assert G.shape[1] == 28
    f, b = np.unravel_index(np.argmax(G), G.shape)
    assert int(np.argmin(np.abs(np.asarray(tone.CEN) - 1000))) == b
    assert abs(G.max()) < 0.5
    assert G.min() >= tone.FLOOR


def test_t7_env_dist_self_and_shift():
    x = _decay(0.08, 0.8, lead=0.05)
    G = tone.bandgram(x)
    assert tone.env_dist(G, G) == 0
    shifted = np.concatenate([np.zeros(int(0.02 * SR)), x])[: len(x)]
    assert tone.env_dist(tone.bandgram(shifted), G) < 1.0


def test_t8_half_tail_shows():
    ref = tone.bandgram(_decay(0.2, 1.5, lead=0.02))
    short = tone.bandgram(_decay(0.1, 1.5, lead=0.02))
    assert tone.env_dist(short, ref) >= 3.0


def test_t9_envref_extra_tail_outside():
    refs = [_decay(0.05 * (1 + 0.05 * i), 1.2, seed=i, lead=0.02) for i in range(4)]
    er = tone.EnvRef(refs)
    x = _decay(0.05, 1.2, seed=9, lead=0.02)
    t = np.arange(len(x)) / SR
    tail = (t > 0.22) & (t < 1.02)
    x = x + tail * _white(30, 1.2) * 10 ** (-15 / 20) * 0.3
    m = er.check(x)
    assert m["outside"], m
    assert any(k.startswith("tail") for k, _, _ in m["outside"])
    assert m["env_out"] > 0
    assert tone.EnvRef.penalty(m) > 0


def test_t10_region_penalty():
    m = {"shares": {"<150": -3.0}, "shares_ref_med": {"<150": -15.0}}
    assert abs(tone.ToneRef.region_penalty(m, regions=("<150",)) - 1.0) < 1e-12
