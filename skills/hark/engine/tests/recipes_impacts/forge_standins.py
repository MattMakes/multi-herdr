"""Test-only stand-ins for forge modules of another unit (T-16).

conftest.py installs a stand-in only when the real module cannot be imported.
Each stand-in follows the public contract of ARCHITECTURE.md 8.6 and 8.7 and the
algorithms of forge-core-dsp and recipes-impacts-tonal, for the names that the
impact recipes call. The package never ships these.
"""

from __future__ import annotations

import math
import types

import numpy as np
from scipy import signal

SR = 48000

try:  # the real svf may use numba; the stand-in uses it when present
    from numba import njit
except ImportError:  # pragma: no cover
    def njit(*a, **k):
        def wrap(fn):
            return fn
        return wrap if not (a and callable(a[0])) else a[0]


# ---------------------------------------------------------------- dsp

def rng(seed):
    return np.random.default_rng(int(seed) & 0xFFFFFFFF)


def n_of(seconds):
    return max(1, int(round(seconds * SR)))


def t_of(n):
    return np.arange(n) / SR


def noise(seconds, r, color="white"):
    n = n_of(seconds)
    x = r.standard_normal(n)
    if color != "white":
        spec = np.fft.rfft(x)
        f = np.fft.rfftfreq(n, 1 / SR)
        if len(f) > 1:
            f[0] = f[1]
        else:
            f[0] = 1.0
        spec = spec / (f ** 0.5 if color == "pink" else f)
        x = np.fft.irfft(spec, n)
    return x / (np.sqrt(np.mean(x ** 2)) + 1e-12)


def contact(seconds, r, bright=0.7):
    n = max(2, n_of(seconds))
    pulse = 0.5 - 0.5 * np.cos(np.linspace(0, 2 * np.pi, n))
    return pulse + r.standard_normal(n) * pulse * bright


def excite(resonance, excitation):
    return signal.fftconvolve(excitation, resonance)[: len(resonance)]


def butter(x, kind, f, order=2):
    hi = 0.95 * SR / 2
    f = np.clip(np.asarray(f, dtype=float), 10.0, hi)
    btype = {"low": "lowpass", "high": "highpass", "band": "bandpass", "bandstop": "bandstop"}[kind]
    sos = signal.butter(order, f, btype=btype, fs=SR, output="sos")
    return signal.sosfilt(sos, np.asarray(x, dtype=float), axis=0)


def peq(x, f0, gain_db, q=0.9):
    a_ = 10 ** (gain_db / 40)
    w0 = 2 * np.pi * min(f0, 0.95 * SR / 2) / SR
    alpha = np.sin(w0) / (2 * q)
    b = np.array([1 + alpha * a_, -2 * np.cos(w0), 1 - alpha * a_])
    a = np.array([1 + alpha / a_, -2 * np.cos(w0), 1 - alpha / a_])
    return signal.lfilter(b / a[0], a / a[0], np.asarray(x, dtype=float), axis=0)


@njit(cache=False)
def _svf_pick(mode, xi, v1, v2, k):
    """The output of 1 SVF step: low (0), band (1), normalised band (2) or high (3)."""
    return (v2, v1, k * v1, xi - k * v1 - v2)[mode]


@njit(cache=False)
def _svf_kernel(x, fc, q, mode):
    out = np.empty_like(x)
    ic1 = 0.0
    ic2 = 0.0
    for i in range(x.shape[0]):
        g = math.tan(math.pi * fc[i] / 48000.0)
        k = 1.0 / max(q[i], 0.05)
        a1 = 1.0 / (1.0 + g * (g + k))
        a2 = g * a1
        a3 = g * a2
        v3 = x[i] - ic2
        v1 = a1 * ic1 + a2 * v3
        v2 = ic2 + a2 * ic1 + a3 * v3
        ic1 = 2 * v1 - ic1
        ic2 = 2 * v2 - ic2
        out[i] = _svf_pick(mode, x[i], v1, v2, k)
    return out


def svf(x, cutoff, q=0.7, mode="band"):
    x = np.asarray(x, dtype=float)
    n = x.shape[0]
    fc = np.clip(np.broadcast_to(np.asarray(cutoff, dtype=float), (n,)), 20.0, 0.45 * SR).copy()
    qq = np.broadcast_to(np.asarray(q, dtype=float), (n,)).copy()
    m = {"low": 0, "band": 1, "bandn": 2, "high": 3}[mode]
    if x.ndim == 1:
        return _svf_kernel(x, fc, qq, m)
    return np.column_stack([_svf_kernel(np.ascontiguousarray(x[:, c]), fc, qq, m) for c in range(x.shape[1])])


def room_ir(r, rt60=0.25, size=0.5, damp=0.6, seconds=None):
    seconds = seconds if seconds is not None else min(2.0, 1.2 * rt60 + 0.05)
    n = n_of(seconds)
    t = np.arange(n) / SR
    tail = r.standard_normal(n) * np.exp(-6.91 * t / max(rt60, 0.02))
    lo = butter(tail, "low", 1500 + (1 - damp) * 9000, 1)
    mix = (t / seconds) ** 0.7
    tail = (1 - mix * damp) * tail + mix * damp * lo
    ir = 0.3 * tail
    ir[0] += 1
    for _ in range(6):
        d = int(r.uniform(0.002, 0.004 + 0.02 * size) * SR)
        a = r.uniform(0.15, 0.45)
        s = r.choice([-1, 1])
        if d < n:
            ir[d] += a * s
    return ir


def room(x, r, rt60=0.25, size=0.5, damp=0.6, wet=0.12):
    ir = room_ir(r, rt60, size, damp)
    ir[0] = 0
    x = np.asarray(x, dtype=float)
    norm = np.sqrt(np.sum(ir ** 2)) + 1e-9
    if x.ndim == 1:
        conv = signal.fftconvolve(x, ir)
    else:
        conv = np.column_stack([signal.fftconvolve(x[:, c], ir) for c in range(x.shape[1])])
    out = wet * conv / norm
    out[: len(x)] += x
    return out


def mic(x, r, low_cut=35.0, air_db=-2.0, floor_db=-66.0, color=None):
    y = butter(x, "high", low_cut, 1)
    y = peq(y, 10000.0, air_db, 0.6)
    if color is not None:
        y = peq(y, color[0], color[1], 0.8)
    pk = np.max(np.abs(y)) + 1e-12
    g = pk * 10 ** (floor_db / 20)
    if y.ndim == 1:
        return y + noise(len(y) / SR, r, "pink")[: len(y)] * g
    cols = [y[:, c] + noise(len(y) / SR, r, "pink")[: len(y)] * g for c in range(y.shape[1])]
    return np.column_stack(cols)


def stereo(x, r, width=0.3, pan=0.0):
    x = np.asarray(x, dtype=float)
    d1 = int(r.uniform(0.2, 0.9) * 1e-3 * SR)
    d2 = int(r.uniform(0.2, 0.9) * 1e-3 * SR)
    side = _delay(x, d1) - _delay(x, d2)
    side = butter(side, "high", 300.0, 2)
    left, right = x + width * side, x - width * side
    th = (np.clip(pan, -1, 1) + 1) * np.pi / 4
    return np.column_stack([left * np.cos(th) * np.sqrt(2), right * np.sin(th) * np.sqrt(2)])


def _delay(x, d):
    y = np.zeros_like(x)
    if d < len(x):
        y[d:] = x[: len(x) - d]
    return y


def fade(x, fin=0.001, fout=0.02):
    y = np.array(x, dtype=float, copy=True)
    n = len(y)
    a, b = min(n, n_of(fin)), min(n, n_of(fout))
    if a > 1:
        ramp = 0.5 - 0.5 * np.cos(np.linspace(0, np.pi, a))
        y[:a] *= ramp if y.ndim == 1 else ramp[:, None]
    if b > 1:
        ramp = 0.5 + 0.5 * np.cos(np.linspace(0, np.pi, b))
        y[n - b:] *= ramp if y.ndim == 1 else ramp[:, None]
    return y


def place(dst, x, t, gain=1.0):
    i = n_of(t) if t > 0 else 0
    if i >= len(dst):
        return dst
    m = min(len(x), len(dst) - i)
    dst[i:i + m] += gain * x[:m]
    return dst


def trim_tail(x, floor_db=-70.0, keep=0.01):
    m = np.abs(x if x.ndim == 1 else np.max(x, axis=1))
    loud = np.nonzero(m >= np.max(m) * 10 ** (floor_db / 20))[0]
    if len(loud) == 0:
        return x
    return x[: min(len(x), loud[-1] + n_of(keep))]


def normalize(x, peak_db=-1.0):
    return x * 10 ** (peak_db / 20) / (np.max(np.abs(x)) + 1e-12)


def stereo_of(x):
    x = np.asarray(x)
    return x if x.ndim == 2 else np.column_stack([x, x])


# ---------------------------------------------------------------- fx

def decorrelate(x, amount=1.0, cut=250.0):
    x = np.asarray(x, dtype=float)
    m = x if x.ndim == 1 else x.mean(axis=1)
    side = m
    for d, g in ((149, 0.6), (263, -0.55), (397, 0.5)):
        b = np.zeros(d + 1)
        a = np.zeros(d + 1)
        b[0], b[d] = -g, 1.0
        a[0], a[d] = 1.0, -g
        side = signal.lfilter(b, a, side)
    side = butter(side, "high", cut, 2)
    rms = np.sqrt(np.mean(m ** 2)) + 1e-12
    side = side * 0.7 * rms / (np.sqrt(np.mean(side ** 2)) + 1e-12)
    norm = np.sqrt(1 + 0.49 * amount ** 2)
    return np.column_stack([(m + amount * side) / norm, (m - amount * side) / norm])


def reverb(x, r, rt60=1.6, size=0.6, predelay=0.01, damp=0.5, wet=0.3, dry=1.0, width=1.0, stereo=None, **_):
    """A small noise-tail reverb: unit-energy IR per channel, dry added."""
    x = np.asarray(x, dtype=float)
    st = (x.ndim == 2) if stereo is None else stereo
    n = n_of(min(4.0, 1.1 * rt60 + 0.05))
    t = np.arange(n) / SR
    pre = n_of(predelay) if predelay > 0 else 0
    mono = x if x.ndim == 1 else x.mean(axis=1)
    irs = _reverb_irs(r, t, pre, rt60, damp, width)
    w = np.column_stack([signal.fftconvolve(mono, ir) for ir in irs])
    if not st:
        out = w.mean(axis=1) * np.sqrt(2) * wet
        out[: len(x)] += dry * mono
        return out
    out = w * wet
    out[: len(x)] += dry * stereo_of(x)
    return out


def _reverb_irs(r, t, pre, rt60, damp, width):
    """2 unit-energy noise-tail impulse responses, pulled together when width < 1."""
    irs = []
    for _c in range(2):
        tail = r.standard_normal(len(t)) * np.exp(-6.91 * t / max(rt60, 0.05))
        tail = butter(tail, "low", 2000 + (1 - damp) * 12000, 1)
        ir = np.concatenate([np.zeros(pre), tail])
        irs.append(ir / (np.sqrt(np.sum(ir ** 2)) + 1e-12))
    shared = 0.5 * (irs[0] + irs[1])
    return [(1 - width) * shared + width * ir for ir in irs] if width < 1 else irs


# ---------------------------------------------------------------- tonal

def _rise(t, onset, curve=1):
    u = np.clip(t / max(onset, 1e-4), 0, 1) ** curve
    return 0.5 - 0.5 * np.cos(np.pi * u)


def env(t, onset, tau, hold=0, curve=1, dcurve=1):
    x = np.maximum(t - onset - hold, 0) / max(tau, 1e-4)
    return _rise(t, onset, curve) * np.exp(-(x if dcurve == 1 else x ** dcurve))


def two_rate(t, onset, fast, tf, ts, curve=1):
    tm = np.maximum(t - onset, 0)
    return _rise(t, onset, curve) * (fast * np.exp(-tm / max(tf, 1e-4)) + (1 - fast) * np.exp(-tm / max(ts, 1e-4)))


def glide(t, f0, bend_st=0, ptau=0.02):
    return f0 * 2 ** ((bend_st / 12) * np.exp(-t / max(ptau, 1e-4)))


def rise_track(t, f0, rise_st, rtau):
    return f0 * 2 ** ((rise_st / 12) * (1 - np.exp(-t / max(rtau, 1e-4))))


def nyquist_fade(f):
    return np.clip((0.45 * SR - f) / 2400, 0, 1)


def tone(f, r, parts=(), e=None):
    f = np.asarray(f, dtype=float)
    e = np.ones_like(f) if e is None else e
    phase = 2 * np.pi * np.cumsum(f) / SR + r.uniform(0, 2 * np.pi)
    y = np.sin(phase) * e
    for q, a, d in parts:
        if a <= 0:
            continue
        y = y + a * nyquist_fade(q * f) * e ** d * np.sin(q * phase + r.uniform(0, 2 * np.pi))
    return y


def follow_noise(n, r, fc, q, e):
    fc = np.broadcast_to(np.minimum(np.asarray(fc, dtype=float), 16000.0), (n,))
    b = svf(r.standard_normal(n), fc, q, "band")
    live = e > 0.1 * (np.max(e) + 1e-12)
    rms = np.sqrt(np.mean(b[live] ** 2)) if np.any(live) else 1.0
    return b * np.sqrt(0.5) / (rms + 1e-12) * e


def burst(n, r, lo, hi, tau, rise=0.0003, order=2):
    k = min(n, n_of(max(6 * tau + rise, 0.002)))
    w = butter(r.standard_normal(k), "band", (max(lo, 20), min(hi, 21600)), order)
    t = np.arange(k) / SR
    w = w * (1 - np.exp(-t / max(rise, 1e-5))) * np.exp(-t / max(tau, 1e-5))
    out = np.zeros(n)
    out[:k] = w / (np.max(np.abs(w)) + 1e-12)
    return out


def thump(n, f, tau, rise=0.002):
    t = np.arange(n) / SR
    y = np.sin(2 * np.pi * f * t) * (1 - np.exp(-t / max(rise, 1e-4))) * np.exp(-t / max(tau, 1e-4))
    return y / (np.max(np.abs(y)) + 1e-12)


def modes(n, r, freqs, amps, taus, jitter=0, bend=0, bend_tau=0.004):
    t = np.arange(n) / SR
    b = 1 + bend * np.exp(-t / max(bend_tau, 1e-4))
    y = np.zeros(n)
    for f, a, tau in zip(freqs, amps, taus):
        if a <= 0 or f >= 21600:
            continue
        f2 = f * (1 + jitter * r.standard_normal())
        ph = 2 * np.pi * np.cumsum(f2 * b) / SR + r.uniform(0, 2 * np.pi)
        y += a * np.exp(-t / max(tau, 1e-5)) * np.sin(ph)
    return y


def strike(y, r, contact_s, bright=0.5):
    return excite(y, contact(contact_s, r, bright))


def room_t(y, r, wet, rt60, size=0.15, width=0.3):
    if wet <= 0.005:
        return y
    return reverb(y, r, rt60=rt60, size=size, predelay=0.004, damp=0.6, wet=wet, stereo=True, width=width)


def _cos2_tail(y, k):
    if k > 0:
        ramp = np.cos(np.linspace(0, np.pi / 2, k)) ** 2
        y[len(y) - k:] *= ramp if y.ndim == 1 else ramp[:, None]
    return y


def finish(y, r, hp=80, lp=12000, room_wet=0, rt60=0.15, width=0, cut=500, end_db=-60, fade=0.006, pan=0,
           size=0.15):
    y = _finish_band(y, hp, lp)
    y = _finish_image(y, width, cut, pan)
    y = room_t(y, r, room_wet, rt60, size, 0.3 + width)
    return _finish_edges(trim_tail(y, end_db, 0.01), fade)


def _finish_band(y, hp, lp):
    y = np.array(y, dtype=float, copy=True)
    _cos2_tail(y, min(len(y), n_of(0.02), int(0.15 * len(y))))
    y = butter(y, "high", hp, 4)
    return butter(y, "low", lp, 2) if lp < 17500 else y


def _finish_image(y, width, cut, pan):
    y = decorrelate(y, width, cut) if width > 0.005 else np.column_stack([y, y])
    if abs(pan) > 0.005:
        th = (np.clip(pan, -1, 1) + 1) * np.pi / 4
        y = y * np.array([np.sqrt(2) * np.cos(th), np.sqrt(2) * np.sin(th)])
    return y


def _finish_edges(y, fade):
    y = _cos2_tail(np.array(y), min(len(y), n_of(fade)))
    k = min(len(y), n_of(0.0003))
    if k > 1:
        y[:k] *= (0.5 - 0.5 * np.cos(np.linspace(0, np.pi, k)))[:, None]
    return y


def _level_track(m):
    n = len(m)
    sq = np.concatenate([[0.0], np.cumsum(m ** 2)])
    centres = np.arange(0, n, 48)
    lo = np.clip(centres - 120, 0, n)
    hi = np.clip(centres + 120, 0, n)
    lev = (sq[hi] - sq[lo]) / np.maximum(hi - lo, 1)
    return np.convolve(lev, np.ones(10) / 10, mode="same")


def _spectrum(m):
    if len(m) < 2048:
        m = np.concatenate([m, np.zeros(2048 - len(m))])
    frames = [m[i:i + 2048] for i in range(0, len(m) - 2048 + 1, 240)]
    win = np.hanning(2048)
    p = np.array([np.abs(np.fft.rfft(fr * win)) ** 2 for fr in frames])
    return p, np.fft.rfftfreq(2048, 1 / SR)


def _level_keys(lev, n):
    ip = int(np.argmax(lev))
    pk = lev[ip]
    on = np.nonzero(lev >= pk * 10 ** -3)[0]
    last = np.nonzero(lev >= pk * 10 ** -4.5)[0]
    end = last[-1] if len(last) else n / 48
    out = {"dur_s": round((end - on[0]) * 0.001, 3)}
    before = np.nonzero(lev[: ip + 1] >= pk * 1e-2)[0]
    out["attack_ms"] = int(ip - before[0]) if len(before) else None
    for key, thr in (("t20_ms", 1e-2), ("t40_ms", 1e-4)):
        below = np.nonzero(lev[ip:] < pk * thr)[0]
        out[key] = int(below[0]) if len(below) else None
    return out


def _balance_keys(spec, f):
    tot = np.sum(spec) + 1e-30
    return {
        "centroid_hz": int(round(np.sum(f * spec) / (np.sum(spec) + 1e-30))),
        "lo150": round(float(np.sum(spec[f < 150]) / tot), 3),
        "hi4k": round(float(np.sum(spec[f >= 4000]) / tot), 3),
    }


def _frame_ridges(rows, f):
    """Per active frame: the strongest-bin frequency in 80-12000 Hz and its quarter-octave energy share."""
    band = (f >= 80) & (f <= 12000)
    shares, ridge = [], []
    for row in rows:
        sub = row[band]
        if sub.max() <= 0:
            continue
        fr = f[band][int(np.argmax(sub))]
        sel = band & (f >= fr / 2 ** 0.25) & (f <= fr * 2 ** 0.25)
        shares.append(np.sum(row[sel]) / (np.sum(sub) + 1e-30))
        ridge.append(fr)
    return shares, ridge


def _tonal_keys(rows, f):
    shares, ridge = _frame_ridges(rows, f)
    out = {"tonal": round(float(np.mean(shares)), 2) if len(shares) >= 2 else None, "glide_st": None}
    if len(ridge) >= 2:
        k3 = max(1, min(3, len(ridge) // 3))
        out["glide_st"] = round(float(12 * np.log2(np.median(ridge[-k3:]) / np.median(ridge[:k3]))), 1)
    return out


def _corr(left, right):
    return round(float(np.sum(left * right) / (np.sqrt(np.sum(left ** 2) * np.sum(right ** 2)) + 1e-30)), 2)


def measure(x):
    x = np.asarray(x, dtype=float)
    x = x if x.ndim == 2 else np.column_stack([x, x])
    m = x.mean(axis=1)
    out = _level_keys(_level_track(m), len(m))
    p, f = _spectrum(m)
    fe = p.sum(axis=1)
    act = fe >= 0.01 * fe.max()
    out.update(_balance_keys(p[act].mean(axis=0), f))
    out.update(_tonal_keys(p[act], f))
    out["corr"] = _corr(x[:, 0], x[:, 1])
    return out


def outside(m, targets):
    d = 0.0
    for key, (lo, hi, scale) in targets.items():
        v = m.get(key)
        if v is None:
            d += 1
        elif v < lo:
            d += min(2, (lo - v) / scale)
        elif v > hi:
            d += min(2, (v - hi) / scale)
    return d


def family_space(space, params, fine):
    return {k: (space[k] if k in fine else [params[k]]) for k in space if k in params}


def build_modules() -> dict[str, types.ModuleType]:
    """Module objects keyed by dotted name: dsp, fx and tonal."""
    g = globals()
    dsp_names = ["rng", "n_of", "t_of", "noise", "contact", "excite", "butter", "peq", "svf", "room_ir", "room",
                 "mic", "stereo", "fade", "place", "trim_tail", "normalize", "stereo_of"]
    tonal_names = ["env", "two_rate", "glide", "rise_track", "nyquist_fade", "tone", "follow_noise", "burst",
                   "thump", "modes", "strike", "finish", "measure", "outside", "family_space"]
    mods = {}
    for name, names in (("hark.forge.engine.dsp", dsp_names), ("hark.forge.engine.fx", ["decorrelate", "reverb"]),
                        ("hark.forge.recipes.tonal", tonal_names)):
        mod = types.ModuleType(name, "test stand-in")
        for n in names:
            setattr(mod, n, g[n])
        mod.SR = SR
        mod.STANDIN = True
        mods[name] = mod
    mods["hark.forge.recipes.tonal"].room = room_t
    return mods


def search_render(recipe, params, seed):
    """Stand-in for hark.forge.engine.search.render (ARCHITECTURE.md 8.2): -1 dBFS float32, or None."""
    try:
        y = np.asarray(recipe.render(dict(params), rng(seed)), dtype=np.float64)
    except Exception:
        return None
    peak = float(np.max(np.abs(y))) if y.size and np.all(np.isfinite(y)) else 0.0
    return (y * (10 ** (-1 / 20) / peak)).astype(np.float32) if peak >= 1e-7 else None
