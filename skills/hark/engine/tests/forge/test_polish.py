"""Acceptance tests P1-P15 of forge-core part 4 (artifact detectors and polish)."""

import numpy as np
from scipy import signal

from hark.forge.engine import dsp, polish

SR = 48000


def _t(sec: float) -> np.ndarray:
    return np.arange(int(SR * sec)) / SR


def _sine(f: float, sec: float, a: float = 0.5) -> np.ndarray:
    return a * np.sin(2 * np.pi * f * _t(sec))


def _fade(x: np.ndarray, ms: float = 10.0) -> np.ndarray:
    n = int(SR * ms / 1000)
    y = x.copy()
    y[:n] *= np.linspace(0, 1, n)
    y[-n:] *= np.linspace(1, 0, n)
    return y


def _decay_noise(sec: float, tau: float, seed: int = 0) -> np.ndarray:
    t = _t(sec)
    return np.random.default_rng(seed).standard_normal(len(t)) * np.exp(-t / tau) * 0.5


def test_p1_click_found():
    # A slow decay keeps the loudest frame at the start, away from the jump
    # (the detector skips +-3 frames around the loudest frame).
    x = _fade(_sine(1000, 0.5) * np.linspace(1.0, 0.8, SR // 2))
    assert polish.clicks(x)["clicks"] == 0
    y = x.copy()
    y[int(0.25 * SR)] += 0.5
    c = polish.clicks(y)
    assert c["clicks"] == 1
    assert abs(c["click_times"][0] - 0.25) <= 0.002


def test_p2_edge_db():
    t = _t(0.5)
    x = 0.5 * np.cos(2 * np.pi * 1000 * t)  # ends near full amplitude
    x = x * np.minimum(1, t / 0.01)
    assert polish.clicks(x)["edge_db"] > -6
    assert polish.clicks(_fade(x))["edge_db"] < -60


def test_p3_aliasing_naive_saw_vs_polyblep():
    t = _t(1.0)
    naive = 0.5 * (2 * ((440 * t) % 1.0) - 1)
    a = polish.aliasing(naive)
    assert a["fold"] > 0.02 and a["harmonic_frames"] >= 2
    assert polish.aliasing(0.5 * dsp.osc(440, "saw", 1.0))["fold"] <= 0.02
    w = polish.aliasing(np.random.default_rng(0).standard_normal(SR) * 0.3)
    assert w["harmonic_frames"] == 0 and w["fold"] == 0


def test_p4_dc_offset_is_hard():
    x = _fade(_sine(500, 0.5, 0.4)) + 0.1
    d = polish.dc(x)
    assert abs(d["dc_db"] - (-14)) < 1.5
    c = polish.PolishRef().check(x)
    assert c["art"].get("dc_db", 0) > 0
    assert "dc_db" in c["hard"]


def test_p5_resonance_found():
    pink = dsp.noise(1.0, dsp.rng(2), "pink") * 0.05
    f, pw = signal.welch(pink, SR, nperseg=4096)
    local = np.sqrt(pw[np.argmin(np.abs(f - 3000))] * SR / 2)
    x = pink + local * 10 ** (25 / 20) * np.sin(2 * np.pi * 3000 * _t(1.0)) * np.sqrt(2 / 4096) * 30
    r = polish.resonances(x)
    assert r["res_hz"] is not None and abs(r["res_hz"] - 3000) <= 60
    assert r["res_db"] > 15


def test_p6_truncated_tail():
    x = _decay_noise(2.0, 0.1)
    cut = int(SR * np.log(10) * 0.1)  # the file stops where the amplitude is down 20 dB
    v = polish.truncated(x[:cut])["trunc_db"]
    assert v is not None and abs(v - (-20)) <= 3
    full = x[: int(SR * np.log(10) * 0.1 * 3.5)]  # down 70 dB
    assert polish.truncated(full)["trunc_db"] is None


def test_p7_digital_silence():
    x = _fade(_sine(700, 0.5))
    i = int(0.25 * SR)
    x[i:i + 480] = 0.0
    assert polish.silence(x)["silence_ms"] == 10.0
    hard = polish.PolishRef().check(x)
    assert "silence_ms" in hard["hard"]
    soft = polish.PolishRef(designed=True).check(x)
    assert "silence_ms" not in soft["hard"]
    assert abs(soft["art"]["silence_ms"] - hard["art"]["silence_ms"] / 2) < 1e-3


def test_p8_clipped_flat_tops():
    x = np.clip(_sine(300, 0.5, 1.0), -0.8, 0.8) / 0.8
    assert polish.clipping(x)["clip_runs"] > 0


def test_p9_attack_time():
    t = _t(0.5)
    x = np.cos(2 * np.pi * 2000 * t) * np.exp(-t / 0.02)
    x = np.concatenate([np.zeros(480), x])
    assert polish.attack(x)["attack_ms"] < 1
    t = _t(0.5)
    y = _sine(1000, 0.5) * np.minimum(1, t / 0.05)
    assert abs(polish.attack(y)["attack_ms"] - 40) < 4


def test_p10_evolution():
    w = np.random.default_rng(4).standard_normal(2 * SR) * 0.3
    assert polish.evolution(w)["evolution_db"] < 1.5
    n = np.random.default_rng(5).standard_normal(SR) * 0.3
    fc = np.geomspace(200, 8000, SR)
    y = dsp.svf(n, fc, 15, "band")
    e = polish.evolution(y)
    assert e["evolution_db"] > 3 and e["centroid_oct"] > 3


def test_p11_lufs_reference_sine():
    x = np.sin(2 * np.pi * 1000 * _t(1.0))
    assert abs(polish.lufs_max(x) - (-3.0)) < 0.1
    assert abs(polish.lufs_max(np.column_stack([x, x])) - 0.0) < 0.1


def test_p12_loudness_spread():
    x = _sine(1000, 1.0)
    assert abs(polish.loudness_spread([x, 0.5 * x])["spread_lu"] - 6.02) < 0.01


def test_p13_bump_limit_from_refs():
    ref = polish.PolishRef()
    ref.M = [{"bump_db": 4.0} for _ in range(3)]
    ref.n = 3
    clean = polish.measure(_fade(_decay_noise(0.6, 0.05)))
    five = ref.check(None, m={**clean, "bump_db": 5.0})
    assert "bump_db" not in five["polish"]
    assert five["limits"]["bump_db"] == 6.0
    nine = ref.check(None, m={**clean, "bump_db": 9.0})
    assert abs(nine["polish"]["bump_db"] - 0.5) < 1e-9


def test_p14_clean_words():
    x = _fade(dsp.butter(_decay_noise(0.6, 0.08), "low", 12000, 4), 5)
    c = polish.PolishRef().check(x)
    assert c["words"] == "clean (no artifact or polish flag)", c


def _comb_tail(seed: int = 0) -> np.ndarray:
    d = int(0.005 * SR)
    x = np.random.default_rng(seed).standard_normal(int(0.6 * SR))
    a = np.zeros(d + 1)
    a[0], a[d] = 1.0, -0.9
    y = signal.lfilter([1.0], a, x)
    return _fade(y * np.exp(-_t(0.6) / 0.12) / np.max(np.abs(y)) * 0.8, 5)


def test_p15_comb_ring_vs_pitch():
    r = polish.ring(_comb_tail())
    assert r["ring_z"] is not None and r["ring_z"] >= 12
    assert abs(r["ring_ms"] - 5.0) <= 0.1
    buzz = dsp.osc(200, "saw", 0.6) * np.exp(-_t(0.6) / 0.12) * 0.8
    b = polish.ring(_fade(buzz, 5))
    assert b["ring_z"] is None and b.get("ring_src") == "pitch"
