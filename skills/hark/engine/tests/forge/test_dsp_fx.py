"""Acceptance tests D1-D20 of forge-core part 2 (dsp and fx)."""

import numpy as np
import pytest

from hark.forge.engine import dsp, fx
from hark.forge.engine.dsp import SR


def _spec_db(x):
    w = np.hanning(len(x))
    return np.fft.rfftfreq(len(x), 1 / SR), 20 * np.log10(np.abs(np.fft.rfft(x * w)) + 1e-12)


def _peak_near(f, s, f0, half=20.0):
    sel = (f > f0 - half) & (f < f0 + half)
    return s[sel].max()


def test_d1_pink_noise_repeatable_unit_rms():
    a = dsp.noise(0.1, dsp.rng(7), "pink")
    b = dsp.noise(0.1, dsp.rng(7), "pink")
    assert np.array_equal(a, b)
    assert abs(np.sqrt(np.mean(a * a)) - 1.0) < 1e-6


def test_d2_saw_aliasing_and_triangle_harmonics():
    s = dsp.osc(1000, "saw", 0.1)
    assert 0.9 <= np.max(np.abs(s)) <= 1.15
    f, d = _spec_db(s)
    fund = _peak_near(f, d, 1000)
    harm = np.zeros(len(f), dtype=bool)
    for k in range(1, 25):
        harm |= np.abs(f - 1000 * k) < 60
    assert d[~harm & (f > 100)].max() < fund - 40
    t = dsp.osc(1000, "tri", 0.1)
    f, d = _spec_db(t[480:])
    assert abs((_peak_near(f, d, 1000) - _peak_near(f, d, 3000)) - 19.1) < 2.0


def test_d3_svf_band_gain():
    w = np.random.default_rng(0).standard_normal(SR * 4)

    def gain_at(mode):
        y = dsp.svf(w, 1000, 8, mode)
        f = np.fft.rfftfreq(len(w), 1 / SR)
        ratio = np.abs(np.fft.rfft(y)) / (np.abs(np.fft.rfft(w)) + 1e-12)
        return 20 * np.log10(np.median(ratio[(f > 990) & (f < 1010)]))

    assert abs(gain_at("band") - 18.06) < 1.5
    assert abs(gain_at("bandn")) < 1.5


def test_d4_exp_sweep_midpoint():
    f = dsp.sweep(100, 1600, 1.0, "exp")
    assert abs(f[len(f) // 2] - 400) / 400 < 0.01


def test_d5_sub_drop_level_and_end():
    y = dsp.sub_drop(1.6, dsp.rng(0))
    assert abs(20 * np.log10(np.max(np.abs(y))) + 1.0) < 0.01
    assert y[-1] == 0


def test_d6_bell_bar_partials():
    y = dsp.bell(1000, 1.0, dsp.rng(1), "bar")
    f, d = _spec_db(y)
    top = d.max()
    for p in (1000, 2756, 5404):
        assert _peak_near(f, d, p, 30) > top - 40


def test_d7_stack_stereo_length():
    a, b = np.ones(1000), np.ones(2000)
    y = dsp.stack([(a, 0, 0), (b, 0.1, -6, 0.5)])
    assert y.ndim == 2 and y.shape[1] == 2
    assert len(y) == max(len(a), 4800 + len(b))


def test_d8_xfade_length():
    a, b = np.ones(5000), np.ones(4000)
    assert len(dsp.xfade(a, b, 0.02)) == len(a) + len(b) - 960


def _edc_rt60(y):
    b = dsp.butter(y, "band", (500, 2000), 4)
    e = (b ** 2).sum(axis=1) if b.ndim == 2 else b ** 2
    edc = np.cumsum(e[::-1])[::-1]
    db = 10 * np.log10(edc / edc[0] + 1e-30)
    i5, i35 = int(np.argmax(db < -5)), int(np.argmax(db < -35))
    return (i35 - i5) / SR * 2


def test_d9_reverb_decay_and_decorrelation():
    imp = np.zeros(4800)
    imp[0] = 1.0
    y = fx.reverb(imp, dsp.rng(1), rt60=1.0, wet=1, dry=0, stereo=True)
    assert 0.8 <= _edc_rt60(y) <= 1.2
    tail = y[4800:]
    assert np.corrcoef(tail[:, 0], tail[:, 1])[0, 1] < 0.5


def test_d10_reverb_cache_hit_restores_state():
    x = np.random.default_rng(2).standard_normal(2400) * 0.1
    r = dsp.rng(5)
    a = fx.reverb(x, r, rt60=0.4)
    after_fresh = r.random()
    hits = fx.ir_cache_stats()["hits"]
    r = dsp.rng(5)
    b = fx.reverb(x, r, rt60=0.4)
    assert np.array_equal(a, b)
    assert fx.ir_cache_stats()["hits"] == hits + 1
    assert r.random() == after_fresh


def test_d11_ir_seed_leaves_generator():
    r = dsp.rng(9)
    state = r.bit_generator.state
    fx.reverb(np.ones(100), r, rt60=0.3, ir_seed=3)
    assert r.bit_generator.state == state


def test_d12_limiter_ceiling_and_passthrough():
    t = dsp.t_of(SR // 2)
    loud = 2.0 * np.sin(2 * np.pi * 440 * t)
    assert np.max(np.abs(fx.limit(loud, -1.0))) <= 10 ** (-1 / 20) + 1e-12
    quiet = 0.5 * np.sin(2 * np.pi * 440 * t)
    assert np.max(np.abs(fx.limit(quiet, -1.0) - quiet)) < 1e-9


def test_d13_split_sums_flat():
    # An impulse gives the exact response: no edge effects of a finite noise take.
    x = np.zeros(SR)
    x[0] = 1.0
    bands = fx.split(x, [200, 3000])
    assert len(bands) == 3
    s = np.abs(np.fft.rfft(sum(bands)))
    f = np.fft.rfftfreq(len(x), 1 / SR)
    sel = (f > 20) & (f < 20000)
    assert np.max(np.abs(20 * np.log10(s[sel]))) < 0.1


def test_d14_saturate_alias_and_level():
    t = dsp.t_of(SR // 2)
    x = 0.5 * np.sin(2 * np.pi * 7000 * t)
    y = fx.saturate(x, 18, "tanh")
    assert abs(np.sqrt(np.mean(y * y)) - np.sqrt(np.mean(x * x))) < 1e-9
    f, d = _spec_db(y)
    harm = np.zeros(len(f), dtype=bool)
    for k in range(1, 4):
        harm |= np.abs(f - 7000 * k) < 80
    assert d[~harm & (f > 50)].max() < d[harm].max() - 45


def test_d15_transient_steady_part():
    t = dsp.t_of(SR)
    x = 0.5 * np.sin(2 * np.pi * 300 * t)
    y = fx.transient(x, attack_db=6, sustain_db=6)
    i = int(0.05 * SR)
    ratio = np.max(np.abs(y[i:])) / np.max(np.abs(x[i:]))
    lo = np.min(np.abs(y[i:][np.abs(x[i:]) > 0.45]) / np.abs(x[i:][np.abs(x[i:]) > 0.45]))
    assert abs(20 * np.log10(ratio)) < 0.5 and abs(20 * np.log10(lo)) < 0.5


def test_d16_eq_match_moves_profile():
    from hark.forge.engine import tone

    p = dsp.noise(1.0, dsp.rng(1), "pink")
    target = tone.profile(np.random.default_rng(3).standard_normal(SR))

    def dist(a):
        pr = tone.profile(a)
        v = target - pr
        w = 10 ** (pr / 10)
        v = v - np.sum(w * v) / np.sum(w)
        return np.sqrt(np.mean(v ** 2))

    y = fx.eq_match(p, target)
    assert dist(y) <= 0.5 * dist(p)
    assert abs(np.sqrt(np.mean(y * y)) - np.sqrt(np.mean(p * p))) < 1e-6


def _pitch(seg):
    f = np.fft.rfftfreq(len(seg) * 8, 1 / SR)
    return f[np.argmax(np.abs(np.fft.rfft(seg * np.hanning(len(seg)), len(seg) * 8)))]


def test_d17_doppler_pitch_shift():
    t = dsp.t_of(SR * 3)
    x = np.sin(2 * np.pi * 1000 * t)
    y = fx.doppler(x, speed=40, dist=4, stereo=False)
    a, b = _pitch(y[2400:12000]), _pitch(y[-12000:-2400])
    assert a > 1000 > b
    assert abs(a / b - (343 / (343 - 40)) / (343 / (343 + 40))) < 0.03


def test_d18_crush_one_bit():
    x = np.random.default_rng(0).uniform(-1, 1, 1000)
    assert set(np.unique(fx.crush(x, bits=1))) <= {-1.0, 0.0, 1.0}


def test_d19_glitch_length_and_repeatable():
    x = np.random.default_rng(0).standard_normal(SR)
    a, b = fx.glitch(x, dsp.rng(3)), fx.glitch(x, dsp.rng(3))
    assert len(a) == len(x) and np.array_equal(a, b)


def test_d20_decorrelate_mid_and_correlation():
    x = np.random.default_rng(0).standard_normal(SR)
    y = fx.decorrelate(x, 1.0)
    assert np.allclose((y[:, 0] + y[:, 1]) / 2, x / np.sqrt(1.49))
    assert np.corrcoef(y[:, 0], y[:, 1])[0, 1] < 0.8


@pytest.mark.parametrize("fn", ["reverse_swell", "granular"])
def test_stereo_effects_shapes(fn):
    hit = dsp.bell(800, 0.3, dsp.rng(0))
    if fn == "reverse_swell":
        y = fx.reverse_swell(hit, dsp.rng(0), seconds=0.5, rt60=0.8)
        assert y.shape[1] == 2 and len(y) >= dsp.n_of(0.5)
    else:
        y = fx.granular(hit, 0.5, dsp.rng(0))
        assert y.shape == (dsp.n_of(0.5), 2)
