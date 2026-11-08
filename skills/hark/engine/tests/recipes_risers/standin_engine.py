"""Test-only stand-ins for the forge engine (dsp, fx) and the impact recipes.

conftest.py installs a stand-in only when the real module cannot be imported. These are simplified versions of the
ARCHITECTURE.md signatures, good enough to exercise the riser-folder recipes. They never ship in the package.
"""

from __future__ import annotations

import types

import numpy as np
from scipy.signal import butter as _sp_butter
from scipy.signal import fftconvolve, lfilter, sosfilt

SR = 48000


# ------------------------------------------------------------------ dsp

def rng(seed):
    return np.random.default_rng(int(seed) & 0xFFFFFFFF)


def n_of(seconds):
    return max(1, int(round(float(seconds) * SR)))


def t_of(n):
    return np.arange(int(n)) / SR


def noise(seconds, r, color="white"):
    n = n_of(seconds)
    x = r.standard_normal(n)
    if color != "white":
        F = np.fft.rfft(x)
        f = np.fft.rfftfreq(n, 1 / SR)
        if len(f) > 1:
            f[0] = f[1]
        else:
            f[0] = 1.0
        F = F / (f ** (0.5 if color == "pink" else 1.0))
        x = np.fft.irfft(F, n)
    return x / (np.sqrt(np.mean(x ** 2)) + 1e-12)


def butter(x, kind, f, order=2):
    ny = SR / 2
    if kind in ("band", "bandstop"):
        lo, hi = (min(max(v, 10), 0.95 * ny) for v in f)
        if hi <= lo:
            hi = min(lo * 1.01, 0.95 * ny)
            lo = hi / 1.01
        sos = _sp_butter(order, [lo / ny, hi / ny], btype="bandpass" if kind == "band" else "bandstop",
                         output="sos")
    else:
        fc = min(max(float(f), 10), 0.95 * ny)
        sos = _sp_butter(order, fc / ny, btype=kind, output="sos")
    return sosfilt(sos, np.asarray(x, float), axis=0)


def peq(x, f0, gain_db, q=0.9):
    A = 10 ** (gain_db / 40)
    w0 = 2 * np.pi * min(f0, 0.95 * SR / 2) / SR
    al = np.sin(w0) / (2 * q)
    b = np.array([1 + al * A, -2 * np.cos(w0), 1 - al * A])
    a = np.array([1 + al / A, -2 * np.cos(w0), 1 - al / A])
    return lfilter(b / a[0], a / a[0], x, axis=0)


def _svf_coefs(n, cutoff, q):
    fc = np.clip(np.broadcast_to(np.asarray(cutoff, float), (n,)), 20, 0.45 * SR)
    g = np.tan(np.pi * fc / SR)
    k = 1 / np.maximum(np.broadcast_to(np.asarray(q, float), (n,)), 0.05)
    a1 = 1 / (1 + g * (g + k))
    return a1, g * a1, g * g * a1, k


def _svf_run(xs, A1, A2, A3):
    """The band (v1) and low (v2) outputs of the trapezoidal state variable filter."""
    v1s, v2s = [], []
    ic1 = ic2 = 0.0
    for x, a1, a2, a3 in zip(xs, A1, A2, A3):
        v3 = x - ic2
        v1 = a1 * ic1 + a2 * v3
        v2 = ic2 + a2 * ic1 + a3 * v3
        ic1, ic2 = 2 * v1 - ic1, 2 * v2 - ic2
        v1s.append(v1)
        v2s.append(v2)
    return np.array(v1s), np.array(v2s)


def svf(x, cutoff, q=0.7, mode="band"):
    x = np.asarray(x, float)
    a1, a2, a3, k = _svf_coefs(len(x), cutoff, q)
    v1, v2 = _svf_run(x.tolist(), a1.tolist(), a2.tolist(), a3.tolist())
    outs = {"low": lambda: v2, "band": lambda: v1, "bandn": lambda: k * v1}
    return outs.get(mode, lambda: x - k * v1 - v2)()


def fade(x, fin=0.001, fout=0.02):
    y = np.array(x, float, copy=True)
    n = len(y)
    a = min(n, n_of(fin))
    b = min(n, n_of(fout))
    if a > 1:
        r = 0.5 - 0.5 * np.cos(np.linspace(0, np.pi, a))
        y[:a] *= r if y.ndim == 1 else r[:, None]
    if b > 1:
        r = 0.5 + 0.5 * np.cos(np.linspace(0, np.pi, b))
        y[n - b:] *= r if y.ndim == 1 else r[:, None]
    return y


def trim_tail(x, floor_db=-70.0, keep=0.01):
    m = np.abs(x if x.ndim == 1 else x.max(axis=1))
    loud = np.nonzero(m >= m.max() * 10 ** (floor_db / 20))[0]
    end = len(x) if not len(loud) else min(len(x), loud[-1] + n_of(keep))
    return x[:end]


def _blep(t, dt):
    out = np.zeros_like(t)
    a = t < dt
    u = t[a] / dt[a]
    out[a] = 2 * u - u * u - 1
    b = t > 1 - dt
    u = (t[b] - 1) / dt[b]
    out[b] = u * u + 2 * u + 1
    return out


def _saw(ph, dt, f, phase):
    return 2 * ph - 1 - _blep(ph, dt)


def _square(ph, dt, f, phase):
    return np.where(ph < 0.5, 1.0, -1.0) + _blep(ph, dt) - _blep(np.mod(ph + 0.5, 1.0), dt)


def _triangle(ph, dt, f, phase):
    sq = _square(ph, dt, f, phase)
    return lfilter([1.0], [1.0, -0.9995], 4 * f / SR * sq, zi=[-0.9995 * (1 - 2 * (phase % 1))])[0]


_SHAPES = {"sine": lambda ph, dt, f, phase: np.sin(2 * np.pi * ph), "saw": _saw, "square": _square}


def osc(freq, shape="sine", seconds=None, phase=0.0):
    f = np.asarray(freq, float)
    if f.ndim == 0:
        f = np.full(n_of(seconds or 1.0), float(f))
    dt = np.clip(f / SR, 1e-9, 0.49)
    ph = np.mod(phase + np.concatenate([[0.0], np.cumsum(dt)[:-1]]), 1.0)
    return _SHAPES.get(shape, _triangle)(ph, dt, f, phase)


def shepard(seconds, r, up=True, rate=0.5, f_lo=40.0, f_hi=10000.0, kind="tone", q=6.0, tilt=-1.5):
    n = n_of(seconds)
    t = t_of(n)
    octs = np.log2(f_hi / f_lo)
    v = max(2, int(round(octs)))
    sign = 1 if up else -1
    y = np.zeros(n)
    for k in range(v):
        u = np.mod(k / v + sign * rate * t / octs, 1.0)
        f = f_lo * 2 ** (u * octs)
        a = (0.5 - 0.5 * np.cos(2 * np.pi * u)) * 10 ** (tilt * u * octs / 20)
        y += a * np.sin(2 * np.pi * np.cumsum(f) / SR + r.uniform(0, 2 * np.pi))
    return y / (np.sqrt(np.mean(y ** 2)) + 1e-12)


def air(seconds, r, center=1200.0, lo_db=6.0, hi_db=4.5, flutter=0.5, rate=6.0, bright=0.6, whistle=0.0):
    n = n_of(seconds)
    x = butter(r.standard_normal(n), "band", (center / 3, min(center * 4, 20000)), 2)
    c = butter(r.standard_normal(n), "low", max(rate, 10.5), 2)
    c = c / (np.std(c) + 1e-12)
    x = x * np.exp(0.5 * flutter * c)
    return x / (np.sqrt(np.mean(x ** 2)) + 1e-12)


BELLS = {
    "tubular": ([1.0, 2.756, 5.404, 8.933, 13.34], [0.35, 1.0, 0.7, 0.4, 0.2]),
    "bar": ([1.0, 2.756, 5.404, 8.933], [1.0, 0.3, 0.12, 0.06]),
    "glass": ([1.0, 2.32, 4.25, 6.63, 9.38], [1.0, 0.45, 0.25, 0.12, 0.06]),
    "bowl": ([1.0, 2.71, 5.13, 8.27], [1.0, 0.6, 0.3, 0.12]),
}


def _bell_partial(f, a, q, t, r, b):
    """One partial and its beating twin; b holds f0, decay, slope, bright and beat."""
    beat = b.beat * r.uniform(0.4, 1.6)
    env = a * q ** (1.2 * (b.bright - 0.5)) * np.exp(-t / max(b.decay * q ** -b.slope, 1e-4))
    pair = np.sin(2 * np.pi * f * t + r.uniform(0, 6.3)) + 0.7 * np.sin(2 * np.pi * (f + beat) * t + r.uniform(0, 6.3))
    return env * pair / 1.7


def bell(f0, seconds, r, partials="bar", decay=1.2, slope=0.8, bright=0.5, beat=1.0, stretch=0.0, strike=0.3):
    ratios, amps = BELLS[partials] if isinstance(partials, str) else partials
    b = types.SimpleNamespace(decay=decay, slope=slope, bright=bright, beat=beat)
    n = n_of(seconds)
    t = t_of(n)
    y = np.zeros(n)
    for q, a in zip(ratios, amps):
        if f0 * q <= 0.45 * SR:
            y += _bell_partial(f0 * q, a, q, t, r, b)
    k = min(n, n_of(0.0015))
    y[:k] *= np.linspace(0, 1, k)
    y = fade(y, 0, 0.15 * seconds)
    return y * 10 ** (-1 / 20) / (np.max(np.abs(y)) + 1e-12)


def fm(fc, ratio, index, seconds, decay=1.0, index_decay=0.3, attack=0.002):
    n = n_of(seconds)
    t = t_of(n)
    idx = index * np.exp(-t / max(index_decay, 1e-4))
    y = np.sin(2 * np.pi * fc * t + idx * np.sin(2 * np.pi * fc * ratio * t)) * np.exp(-t / max(decay, 1e-4))
    a = min(n, n_of(attack))
    y[:a] *= 0.5 - 0.5 * np.cos(np.linspace(0, np.pi, a))
    return fade(y, 0, 0.15 * seconds)


def room(x, r, rt60=0.25, size=0.5, damp=0.6, wet=0.12):
    m = n_of(min(2.0, 1.2 * rt60 + 0.05))
    ir = r.standard_normal(m) * np.exp(-6.91 * t_of(m) / rt60)
    ir[0] = 0
    conv = fftconvolve(x, ir[:, None] if x.ndim == 2 else ir, axes=0) if x.ndim == 2 else fftconvolve(x, ir)
    out = conv * wet / (np.sqrt(np.sum(ir ** 2)) + 1e-9)
    out[:len(x)] += x
    return out


def stereo_of(x):
    return x if x.ndim == 2 else np.column_stack([x, x])


# ------------------------------------------------------------------ fx

def _ir_len(tail, rt60, bass):
    return max(n_of(tail if tail is not None else 1.2 * rt60 * max(1.0, bass)), 64)


def _ir_gen(seed, r):
    return rng(seed) if seed is not None else r


def _ir_lowcut(ir, lowcut):
    return butter(ir, "high", lowcut, 2) if lowcut > 0 else ir


def _ir_pad(ir, predelay, inputs):
    pd = n_of(predelay) if predelay > 0 else 0
    return np.concatenate([np.zeros((pd, inputs, 2)), ir])


def reverb_ir(r, rt60=1.6, size=0.6, predelay=0.01, damp=0.5, bass=1.2, mod=1.0, diffuse=0.7, early_db=-6.0,
              width=1.0, late="noise", lines=16, tail=None, inputs=1, hf_hz=4000.0, lf_hz=250.0, air=1.0,
              lowcut=0.0, seed=None, cache=True):
    n = _ir_len(tail, rt60, bass)
    t = t_of(n)
    ir = _ir_gen(seed, r).standard_normal((n, inputs, 2)) * np.exp(-6.91 * t / max(rt60, 0.02))[:, None, None]
    ir /= np.sqrt(np.sum(ir ** 2, axis=0, keepdims=True)) + 1e-12
    return _ir_pad(_ir_lowcut(ir, lowcut), predelay, inputs)


def _wet(xs, ir):
    w = np.zeros((len(xs) + len(ir) - 1, 2))
    for c in range(xs.shape[1]):
        for o in range(2):
            w[:, o] += fftconvolve(xs[:, c], ir[:, c, o])
    return w


def _mono_out(x, w, wet, dry):
    out = w.mean(axis=1) * np.sqrt(2) * wet
    out[:len(x)] += dry * (x if x.ndim == 1 else x.mean(axis=1))
    return out


def _stereo_out(x, xs, w, wet, dry):
    out = w * wet
    out[:len(x)] += dry * (xs if xs.shape[1] == 2 else np.column_stack([x, x]))
    return out


def reverb(x, r, rt60=1.6, size=0.6, predelay=0.01, damp=0.5, bass=1.2, mod=1.0, diffuse=0.7, early_db=-6.0,
           width=1.0, wet=0.3, dry=1.0, late="noise", lines=16, tail=None, stereo=None, air=1.0, lowcut=0.0,
           ir_seed=None, grid=False):
    xs = x[:, None] if x.ndim == 1 else x
    ir = reverb_ir(r, rt60=rt60, predelay=predelay, tail=tail, inputs=xs.shape[1], lowcut=lowcut, seed=ir_seed)
    w = _wet(xs, ir)
    two = x.ndim == 2 if stereo is None else stereo
    return _stereo_out(x, xs, w, wet, dry) if two else _mono_out(x, w, wet, dry)


def saturate(x, drive_db=6.0, mode="tanh", mix=1.0, bias=0.0, os=None, level="rms"):
    g = 10 ** (drive_db / 20)
    y = np.tanh(g * x + bias) - np.tanh(bias)
    if level == "rms":
        y = y * np.sqrt(np.mean(x ** 2)) / (np.sqrt(np.mean(y ** 2)) + 1e-12)
    elif level == "peak":
        y = y * np.max(np.abs(x)) / (np.max(np.abs(y)) + 1e-12)
    return mix * y + (1 - mix) * x


def _allpass(x, d, g):
    b = np.zeros(d + 1)
    a = np.zeros(d + 1)
    b[0], b[d] = -g, 1.0
    a[0], a[d] = 1.0, -g
    return lfilter(b, a, x)


def decorrelate(x, amount=1.0, cut=250.0):
    m = x.mean(axis=1) if x.ndim == 2 else x
    s = m
    for d, g in ((149, 0.6), (263, -0.55), (397, 0.5)):
        s = _allpass(s, d, g)
    s = butter(s, "high", cut, 2)
    s = s * 0.7 * np.sqrt(np.mean(m ** 2)) / (np.sqrt(np.mean(s ** 2)) + 1e-12)
    k = np.sqrt(1 + 0.49 * amount ** 2)
    return np.column_stack([(m + amount * s) / k, (m - amount * s) / k])


def width(x, w):
    if x.ndim == 1:
        x = decorrelate(x, 1.0)
    k = np.clip(np.broadcast_to(np.asarray(w, float), (len(x),)), 0, 2)
    mid = (x[:, 0] + x[:, 1]) / 2
    side = (x[:, 0] - x[:, 1]) / 2 * k
    return np.column_stack([mid + side, mid - side])


def crush(x, bits=8, rate=None, mix=1.0):
    y = np.asarray(x, float)
    if rate:
        i = np.arange(len(y))
        y = y[np.minimum(np.floor(np.floor(i * rate / SR) * SR / rate).astype(int), len(y) - 1)]
    if bits:
        q = 2.0 ** (bits - 1)
        y = np.round(y * q) / q
    return mix * y + (1 - mix) * x


def _grain(s, i, n, gl, r, g):
    """One windowed grain read from s for output sample i; g holds pos, pos_jitter, pitch and pitch_jitter."""
    rate = 2 ** ((g.pitch + g.pitch_jitter * r.standard_normal()) / 12)
    p0 = (g.pos[0] + (g.pos[1] - g.pos[0]) * i / n + g.pos_jitter * r.standard_normal()) * (len(s) - gl * rate)
    start = np.clip(p0, 0, max(0, len(s) - gl * rate - 2))
    idx = np.clip((start + np.arange(gl) * rate).astype(int), 0, len(s) - 1)
    return s[idx] * np.hanning(gl)


def _place(out, grain, i, th):
    m = min(len(grain), len(out) - i)
    out[i:i + m, 0] += grain[:m] * np.cos(th) * np.sqrt(2)
    out[i:i + m, 1] += grain[:m] * np.sin(th) * np.sqrt(2)


def _grain_starts(seconds, density, n):
    period = 1 / max(density, 1e-3)
    starts = []
    t = 0.0
    while t < seconds:
        starts.append(n_of(t) if t > 0 else 0)
        t += period
    return [i for i in starts if i < n]


def granular(src, seconds, r, grain=0.06, density=40.0, pos=(0.0, 1.0), pos_jitter=0.02, pitch=0.0,
             pitch_jitter=0.0, time_jitter=0.5, spread=0.6, reverse=0.0):
    s = src.mean(axis=1) if src.ndim == 2 else src
    g = types.SimpleNamespace(pos=pos, pos_jitter=pos_jitter, pitch=pitch, pitch_jitter=pitch_jitter)
    n = n_of(seconds)
    out = np.zeros((n, 2))
    gl = n_of(grain)
    for i in _grain_starts(seconds, density, n):
        gr = _grain(s, i, n, gl, r, g)
        _place(out, gr, i, (np.clip(spread * r.uniform(-1, 1), -1, 1) + 1) * np.pi / 4)
    return out / np.sqrt(max(1, density * grain * 0.375))


def build_dsp() -> types.ModuleType:
    mod = types.ModuleType("hark.forge.engine.dsp")
    for name in ("rng n_of t_of noise butter peq svf fade trim_tail osc shepard air bell fm room stereo_of "
                 "BELLS").split():
        setattr(mod, name, globals()[name])
    mod.STANDIN = True
    return mod


def build_fx() -> types.ModuleType:
    mod = types.ModuleType("hark.forge.engine.fx")
    for name in "reverb_ir reverb saturate decorrelate width crush granular".split():
        setattr(mod, name, globals()[name])
    mod.STANDIN = True
    return mod


# ------------------------------------------------------------------ impact and subdrop recipes

def hall_ir(rt60, damp, size, predelay, seconds=None, *, rng=None):
    g = rng if rng is not None else np.random.default_rng(0)
    seconds = seconds if seconds is not None else min(5.0, 1.1 * rt60 + 0.1)
    n = n_of(seconds)
    t = t_of(n)
    ir = g.standard_normal(n) * np.exp(-6.91 * t / max(rt60, 0.05)) * (1 - np.exp(-t / (0.008 + 0.03 * size)))
    ir /= np.sqrt(np.sum(ir ** 2)) + 1e-12
    if predelay > 0:
        ir = np.concatenate([np.zeros(n_of(predelay)), ir])
    return ir


def _impact_render(p, r):
    n = n_of(1.0)
    t = t_of(n)
    y = np.sin(2 * np.pi * 80 * t) * np.exp(-t / 0.15) + 0.3 * r.standard_normal(n) * np.exp(-t / 0.01)
    return np.column_stack([y, y])


def _subdrop_render(p, r):
    n = n_of(p.get("length", 1.4))
    t = t_of(n)
    f = p.get("f1", 40) + (p.get("f0", 64) - p.get("f1", 40)) * np.exp(-t / 0.3)
    y = np.sin(2 * np.pi * np.cumsum(f) / SR) * np.exp(-t * p.get("decay_db", 40) / 8.686)
    return np.column_stack([y, y])


def build_impact() -> types.ModuleType:
    mod = types.ModuleType("hark.forge.recipes.impacts.impact")
    mod.hall_ir = hall_ir
    mod.render = _impact_render
    mod.FAMILIES = {"impact": [("classic", {}), ("deep", {}), ("punch", {"f": 1.0})]}
    mod.measure = lambda x: {}
    mod.study_extra = lambda x, row=None: 0.0
    mod.study_penalty = lambda x, cat=None: 0.0
    mod.STANDIN = True
    return mod


def build_subdrop() -> types.ModuleType:
    mod = types.ModuleType("hark.forge.recipes.impacts.subdrop")
    mod.render = _subdrop_render
    mod.FAMILIES = {"sub_drop": [("808", {"f0": 120, "f1": 40, "length": 2.0})],
                    "pro_sub_drop": [("ref", {"f0": 64, "f1": 44, "length": 1.3, "decay_db": 42})]}
    mod.STANDIN = True
    return mod


# ------------------------------------------------------------------ forge-core render wrapper (ARCHITECTURE 8.2)

def _wrapper_render(recipe, params, seed):
    y = recipe.render(dict(params), np.random.default_rng(int(seed) & 0xFFFFFFFF))
    y = np.asarray(y, float)
    if y.size == 0 or not np.all(np.isfinite(y)) or np.max(np.abs(y)) < 1e-7:
        return None
    return y * 10 ** (-1 / 20) / np.max(np.abs(y))


def build_search() -> types.ModuleType:
    mod = types.ModuleType("hark.forge.engine.search")
    mod.render = _wrapper_render
    mod.STANDIN = True
    return mod
