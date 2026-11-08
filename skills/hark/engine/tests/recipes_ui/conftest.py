"""Test setup of the UI recipes area.

The UI recipes import ``hark.forge.engine.dsp``, ``hark.forge.engine.fx`` and
``hark.forge.recipes.tonal``. Another unit (T-16) builds them. When one of them
is absent, this file installs a small stand-in for it, written from the
forge-core and recipes-impacts-tonal specs. A stand-in is used only when the
real module is absent. It is never part of the package.
"""

from __future__ import annotations

import importlib
import math
import sys
import types

import numpy as np
from scipy import signal

SR = 48000
STANDINS: list[str] = []


def _absent(name: str) -> bool:
    try:
        importlib.import_module(name)
    except ModuleNotFoundError:
        return True
    return False


def _install(name: str, module: types.ModuleType) -> None:
    parent_name, _, leaf = name.rpartition(".")
    if _absent(parent_name):
        _install(parent_name, types.ModuleType(parent_name))
        sys.modules[parent_name].__path__ = []
    sys.modules[name] = module
    setattr(sys.modules[parent_name], leaf, module)
    STANDINS.append(name)


# ---------------------------------------------------------------- dsp stand-in

def _dsp() -> types.ModuleType:
    m = types.ModuleType("hark.forge.engine.dsp")
    m.SR = SR

    def rng(seed):
        return np.random.default_rng(int(seed) & 0xFFFFFFFF)

    def n_of(seconds):
        return max(1, int(round(seconds * SR)))

    def t_of(n):
        return np.arange(n) / SR

    def noise(seconds, r, color="white"):
        n = n_of(seconds)
        w = r.standard_normal(n)
        if color != "white":
            spec = np.fft.rfft(w)
            f = np.fft.rfftfreq(n, 1 / SR)
            if len(f) > 1:
                f[0] = f[1]
            else:
                f[0] = 1.0
            spec = spec / (f ** (0.5 if color == "pink" else 1.0))
            w = np.fft.irfft(spec, n)
        return w / (np.sqrt(np.mean(w ** 2)) + 1e-12)

    def contact(seconds, r, bright=0.7):
        n = max(2, n_of(seconds))
        pulse = 0.5 - 0.5 * np.cos(np.linspace(0, 2 * np.pi, n))
        return pulse + r.standard_normal(n) * pulse * bright

    def modal(freqs, decays, amps, seconds, r, jitter=0.003, glide=0.0):
        n = n_of(seconds)
        t = t_of(n)
        y = np.zeros(n)
        for f, d, a in zip(freqs, decays, amps):
            if f >= 0.95 * SR / 2 or a == 0:
                continue
            f1 = f * (1 + jitter * r.standard_normal())
            inst = f1 * (1 - glide * (1 - np.exp(-t / max(d, 1e-4))))
            ph = 2 * np.pi * np.cumsum(inst) / SR + r.uniform(0, 2 * np.pi)
            y += a * np.exp(-t / max(d, 1e-5)) * np.sin(ph)
        return y

    def excite(resonance, excitation):
        return signal.fftconvolve(excitation, resonance)[: len(resonance)]

    def butter(x, kind, f, order=2):
        lim = 0.95 * SR / 2
        f = np.clip(np.asarray(f, dtype=float), 10.0, lim)
        btype = {"low": "lowpass", "high": "highpass", "band": "bandpass", "bandstop": "bandstop"}[kind]
        sos = signal.butter(order, f, btype=btype, fs=SR, output="sos")
        return signal.sosfilt(sos, np.asarray(x, dtype=float), axis=0)

    def peq(x, f0, gain_db, q=0.9):
        a_ = 10 ** (gain_db / 40)
        w0 = 2 * np.pi * min(f0, 0.95 * SR / 2) / SR
        al = np.sin(w0) / (2 * q)
        b = np.array([1 + al * a_, -2 * np.cos(w0), 1 - al * a_])
        a = np.array([1 + al / a_, -2 * np.cos(w0), 1 - al / a_])
        return signal.lfilter(b / a[0], a / a[0], np.asarray(x, dtype=float), axis=0)

    def svf(x, cutoff, q=0.7, mode="band"):
        x = np.asarray(x, dtype=float)
        n = len(x)
        fc = np.clip(np.broadcast_to(np.asarray(cutoff, dtype=float), (n,)), 20, 0.45 * SR)
        qq = np.broadcast_to(np.asarray(q, dtype=float), (n,))
        g = np.tan(np.pi * fc / SR)
        k = 1 / np.maximum(qq, 0.05)
        a1 = 1 / (1 + g * (g + k))
        a2 = g * a1
        a3 = g * a2
        out = np.empty(n)
        ic1 = ic2 = 0.0
        for i in range(n):
            v3 = x[i] - ic2
            v1 = a1[i] * ic1 + a2[i] * v3
            v2 = ic2 + a2[i] * ic1 + a3[i] * v3
            ic1 = 2 * v1 - ic1
            ic2 = 2 * v2 - ic2
            if mode == "low":
                out[i] = v2
            elif mode == "band":
                out[i] = v1
            elif mode == "bandn":
                out[i] = k[i] * v1
            else:
                out[i] = x[i] - k[i] * v1 - v2
        return out

    def room_ir(r, rt60=0.25, size=0.5, damp=0.6, seconds=None):
        seconds = seconds if seconds is not None else min(2.0, 1.2 * rt60 + 0.05)
        n = n_of(seconds)
        t = t_of(n)
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
        y = peq(y, 10000, air_db, 0.6)
        if color is not None:
            y = peq(y, color[0], color[1], 0.8)
        pk = np.max(np.abs(y)) + 1e-12
        if y.ndim == 1:
            return y + noise(len(y) / SR, r, "pink")[: len(y)] * pk * 10 ** (floor_db / 20)
        cols = [y[:, c] + noise(len(y) / SR, r, "pink")[: len(y)] * pk * 10 ** (floor_db / 20)
                for c in range(y.shape[1])]
        return np.column_stack(cols)

    def fade(x, fin=0.001, fout=0.02):
        y = np.array(x, dtype=float, copy=True)
        n = len(y)
        a = min(n, n_of(fin))
        b = min(n, n_of(fout))
        shape = (-1,) + (1,) * (y.ndim - 1)
        if a > 1:
            y[:a] *= (0.5 - 0.5 * np.cos(np.linspace(0, np.pi, a))).reshape(shape)
        if b > 1:
            y[n - b:] *= (0.5 + 0.5 * np.cos(np.linspace(0, np.pi, b))).reshape(shape)
        return y

    def place(dst, x, t, gain=1.0):
        i = n_of(t) if t > 0 else 0
        if i >= len(dst):
            return dst
        k = min(len(x), len(dst) - i)
        dst[i:i + k] += gain * x[:k]
        return dst

    def trim_tail(x, floor_db=-70.0, keep=0.01):
        m = np.abs(x if x.ndim == 1 else np.max(x, axis=1))
        loud = np.nonzero(m >= np.max(m) * 10 ** (floor_db / 20))[0]
        end = len(x) if len(loud) == 0 else min(len(x), loud[-1] + n_of(keep))
        return x[:end]

    def normalize(x, peak_db=-1.0):
        return x * 10 ** (peak_db / 20) / (np.max(np.abs(x)) + 1e-12)

    def stereo_of(x):
        x = np.asarray(x)
        return np.column_stack([x, x]) if x.ndim == 1 else x

    for fn in (rng, n_of, t_of, noise, contact, modal, excite, butter, peq, svf, room_ir, room, mic, fade,
               place, trim_tail, normalize, stereo_of):
        setattr(m, fn.__name__, fn)
    return m


# ----------------------------------------------------------------- fx stand-in

def _fx(dsp: types.ModuleType) -> types.ModuleType:
    m = types.ModuleType("hark.forge.engine.fx")

    def early(x, r, size=0.5, taps=18, damp=0.5, width=1.0):
        x = np.asarray(x, dtype=float)
        mono = x if x.ndim == 1 else x.mean(axis=1)
        t_hi = 0.006 + 0.05 * size
        mlen = dsp.n_of(t_hi) + 2
        chans = []
        for c in range(2):
            ts = np.sort(np.exp(r.uniform(math.log(0.003), math.log(t_hi), taps)))
            if c == 1:
                ts = ts * (1 + width * r.uniform(-0.12, 0.12, taps))
            ir = np.zeros(mlen)
            for t in ts:
                ir[min(mlen - 1, dsp.n_of(t))] += r.choice([-1, 1]) * 0.003 / t
            parts = np.zeros(mlen)
            edges = np.linspace(0, mlen, 4).astype(int)
            for j in range(3):
                seg = np.zeros(mlen)
                seg[edges[j]:edges[j + 1]] = ir[edges[j]:edges[j + 1]]
                parts += dsp.butter(seg, "low", 14000 - damp * 4000 * (j + 1), 1)
            parts /= np.sqrt(np.sum(parts ** 2)) + 1e-12
            chans.append(signal.fftconvolve(mono, parts))
        return np.column_stack(chans)

    def reverb(x, r, rt60=1.6, size=0.6, predelay=0.01, damp=0.5, wet=0.3, dry=1.0, width=1.0,
               stereo=None, lowcut=0.0, early_db=-6.0, **_):
        x = np.asarray(x, dtype=float)
        stereo = (x.ndim == 2) if stereo is None else stereo
        n = max(dsp.n_of(1.2 * rt60), 64)
        t = dsp.t_of(n)
        ir = r.standard_normal((n, 2)) * np.exp(-6.91 * t / max(rt60, 0.02))[:, None]
        ir = dsp.butter(ir, "low", 2000 + (1 - damp) * 12000, 1)
        mid = ir.mean(axis=1, keepdims=True)
        ir = mid + width * (ir - mid)
        if lowcut > 0:
            ir = dsp.butter(ir, "high", lowcut, 2)
        ir /= np.sqrt(np.sum(ir ** 2) / 2) + 1e-12
        pd = dsp.n_of(predelay) if predelay > 0 else 0
        ir = np.concatenate([np.zeros((pd, 2)), ir])
        mono = x if x.ndim == 1 else x.mean(axis=1)
        wetsig = np.column_stack([signal.fftconvolve(mono, ir[:, c]) for c in range(2)])
        if not stereo:
            out = wet * wetsig.mean(axis=1) * math.sqrt(2)
            out[: len(x)] += dry * mono
            return out
        out = wet * wetsig
        out[: len(x)] += dry * (dsp.stereo_of(x))
        return out

    def saturate(x, drive_db=6.0, mode="tanh", mix=1.0, bias=0.0, os=None, level="rms"):
        x = np.asarray(x, dtype=float)
        g = 10 ** (drive_db / 20)
        y = np.tanh(g * x + bias) - np.tanh(bias)
        if level == "rms":
            y = y * (np.sqrt(np.mean(x ** 2)) + 1e-12) / (np.sqrt(np.mean(y ** 2)) + 1e-12)
        elif level == "peak":
            y = y * (np.max(np.abs(x)) + 1e-12) / (np.max(np.abs(y)) + 1e-12)
        return mix * y + (1 - mix) * x

    def limit(x, ceiling_db=-1.0, lookahead=0.0025, release=0.08, true_peak=True):
        from scipy.ndimage import minimum_filter1d, uniform_filter1d
        x = np.asarray(x, dtype=float)
        c = 10 ** (ceiling_db / 20)
        pk = np.abs(x) if x.ndim == 1 else np.max(np.abs(x), axis=1)
        need = np.minimum(1.0, c / np.maximum(pk, 1e-12))
        w = max(1, dsp.n_of(lookahead))
        g = minimum_filter1d(need, 2 * w + 1, mode="nearest")
        coef = math.exp(-1 / max(1, release * SR))
        s = 1.0
        out = np.empty_like(g)
        for i, gi in enumerate(g):
            s = 1 - (1 - s) * coef
            s = min(s, gi)
            out[i] = s
        out = np.minimum(uniform_filter1d(out, 2 * w + 1, mode="nearest"), need)
        y = x * (out if x.ndim == 1 else out[:, None])
        return np.clip(y, -c, c)

    def _allpass(x, d, g):
        b = np.zeros(d + 1)
        a = np.zeros(d + 1)
        b[0], b[d] = -g, 1.0
        a[0], a[d] = 1.0, -g
        return signal.lfilter(b, a, x)

    def decorrelate(x, amount=1.0, cut=250.0):
        x = np.asarray(x, dtype=float)
        mono = x if x.ndim == 1 else x.mean(axis=1)
        side = mono
        for d, g in ((149, 0.6), (263, -0.55), (397, 0.5)):
            side = _allpass(side, d, g)
        side = dsp.butter(side, "high", cut, 2)
        side = side * 0.7 * np.sqrt(np.mean(mono ** 2)) / (np.sqrt(np.mean(side ** 2)) + 1e-12)
        norm = math.sqrt(1 + 0.49 * amount ** 2)
        return np.column_stack([(mono + amount * side) / norm, (mono - amount * side) / norm])

    for fn in (early, reverb, saturate, limit, decorrelate):
        setattr(m, fn.__name__, fn)
    return m


# -------------------------------------------------------------- tonal stand-in

def _tonal(dsp: types.ModuleType, fx: types.ModuleType) -> types.ModuleType:
    m = types.ModuleType("hark.forge.recipes.tonal")
    m.SR = SR

    def _rise(t, onset, curve):
        u = np.clip(t / max(onset, 1e-4), 0, 1) ** curve
        return 0.5 - 0.5 * np.cos(np.pi * u)

    def env(t, onset, tau, hold=0, curve=1, dcurve=1):
        x = np.maximum(t - onset - hold, 0) / max(tau, 1e-4)
        return _rise(t, onset, curve) * (np.exp(-x) if dcurve == 1 else np.exp(-x ** dcurve))

    def two_rate(t, onset, fast, tf, ts, curve=1):
        tm = np.maximum(t - onset, 0)
        return _rise(t, onset, curve) * (fast * np.exp(-tm / max(tf, 1e-4)) + (1 - fast) * np.exp(-tm / max(ts, 1e-4)))

    def glide(t, f0, bend_st=0, ptau=0.02):
        return f0 * 2 ** ((bend_st / 12) * np.exp(-t / max(ptau, 1e-4)))

    def rise_track(t, f0, rise_st, rtau):
        return f0 * 2 ** ((rise_st / 12) * (1 - np.exp(-t / max(rtau, 1e-4))))

    def nyquist_fade(f, q=1.0):
        return np.clip((0.45 * SR - q * np.asarray(f)) / 2400, 0, 1)

    def tone(f, r, parts=(), e=None):
        f = np.asarray(f, dtype=float)
        e = np.ones(len(f)) if e is None else e
        ph = 2 * np.pi * np.cumsum(f) / SR + r.uniform(0, 2 * np.pi)
        y = np.sin(ph) * e
        for q, a, d in parts:
            if a <= 0:
                continue
            y = y + a * nyquist_fade(f, q) * e ** d * np.sin(q * ph + r.uniform(0, 2 * np.pi))
        return y

    def follow_noise(n, r, fc, q, e):
        fc = np.broadcast_to(np.minimum(np.asarray(fc, dtype=float), 16000), (n,))
        b = dsp.svf(r.standard_normal(n), fc, q, "band")
        live = e > 0.1 * (np.max(e) + 1e-12)
        rms = np.sqrt(np.mean(b[live] ** 2)) if np.any(live) else 0.0
        return b * math.sqrt(0.5) / (rms + 1e-12) * e

    def burst(n, r, lo, hi, tau, rise=0.0003, order=2):
        k = min(n, dsp.n_of(max(6 * tau + rise, 0.002)))
        w = dsp.butter(r.standard_normal(k), "band", (max(lo, 20), min(hi, 21600)), order)
        t = dsp.t_of(k)
        w = w * (1 - np.exp(-t / max(rise, 1e-5))) * np.exp(-t / max(tau, 1e-5))
        out = np.zeros(n)
        out[:k] = w / (np.max(np.abs(w)) + 1e-12)
        return out

    def thump(n, f, tau, rise=0.002):
        t = dsp.t_of(n)
        y = np.sin(2 * np.pi * f * t) * (1 - np.exp(-t / max(rise, 1e-4))) * np.exp(-t / max(tau, 1e-4))
        return y / (np.max(np.abs(y)) + 1e-12)

    def modes(n, r, freqs, amps, taus, jitter=0, bend=0, bend_tau=0.004):
        t = dsp.t_of(n)
        b = 1 + bend * np.exp(-t / max(bend_tau, 1e-4))
        y = np.zeros(n)
        for f, a, tau in zip(freqs, amps, taus):
            if a <= 0 or f >= 21600:
                continue
            f1 = f * (1 + jitter * r.standard_normal())
            ph = 2 * np.pi * np.cumsum(f1 * b) / SR + r.uniform(0, 2 * np.pi)
            y += a * np.exp(-t / max(tau, 1e-5)) * np.sin(ph)
        return y

    def strike(y, r, contact_s, bright=0.5):
        return dsp.excite(y, dsp.contact(contact_s, r, bright))

    def room(y, r, wet, rt60, size=0.15, width=0.3):
        if wet <= 0.005:
            return y
        return fx.reverb(y, r, rt60=rt60, size=size, predelay=0.004, damp=0.6, wet=wet, stereo=True, width=width)

    def _cos2_tail(y, k):
        if k > 0:
            y[len(y) - k:] *= (np.cos(np.linspace(0, np.pi / 2, k)) ** 2).reshape((-1,) + (1,) * (y.ndim - 1))
        return y

    def finish(y, r, hp=80, lp=12000, room_wet=0, rt60=0.15, width=0, cut=500, end_db=-60, fade=0.006, pan=0,
               size=0.15):
        y = np.array(y, dtype=float)
        _cos2_tail(y, min(len(y), dsp.n_of(0.02), int(0.15 * len(y))))
        y = dsp.butter(y, "high", hp, 4)
        if lp < 17500:
            y = dsp.butter(y, "low", lp, 2)
        st = fx.decorrelate(y, width, cut) if width > 0.005 else np.column_stack([y, y])
        if abs(pan) > 0.005:
            th = (np.clip(pan, -1, 1) + 1) * np.pi / 4
            st = st * np.array([math.sqrt(2) * math.cos(th), math.sqrt(2) * math.sin(th)])
        st = room(st, r, room_wet, rt60, size, 0.3 + width)
        st = dsp.trim_tail(st, end_db, 0.01).copy()
        _cos2_tail(st, min(len(st), dsp.n_of(fade)))
        k = min(len(st), dsp.n_of(0.0003))
        st[:k] *= (0.5 - 0.5 * np.cos(np.linspace(0, np.pi, k)))[:, None]
        return st

    def measure(x):
        x = np.asarray(x, dtype=float)
        st = np.column_stack([x, x]) if x.ndim == 1 else x
        mm = st.mean(axis=1)
        n = len(mm)
        sq = mm ** 2
        cs = np.concatenate([[0], np.cumsum(sq)])
        centres = np.arange(0, n, 48)
        lo = np.clip(centres - 120, 0, n)
        hi = np.clip(centres + 120, 0, n)
        lev = (cs[hi] - cs[lo]) / np.maximum(hi - lo, 1)
        lev = np.convolve(lev, np.ones(10) / 10, mode="same")
        ip = int(np.argmax(lev))
        pk = lev[ip]
        last = np.nonzero(lev >= pk * 10 ** -4.5)[0]
        first = np.nonzero(lev >= pk * 10 ** -3)[0]
        end_ms = last[-1] if len(last) else n / 48
        out = {"dur_s": round(end_ms * 0.001 - first[0] * 0.001, 3)}
        pre = np.nonzero(lev[: ip + 1] >= pk * 1e-2)[0]
        out["attack_ms"] = float(ip - pre[0]) if len(pre) else None
        for key, thr in (("t20_ms", 1e-2), ("t40_ms", 1e-4)):
            below = np.nonzero(lev[ip:] < pk * thr)[0]
            out[key] = float(below[0]) if len(below) else None
        pad = np.concatenate([mm, np.zeros(max(0, 2048 - n))])
        f, _, z = signal.stft(pad, SR, window="hann", nperseg=2048, noverlap=2048 - 240, boundary=None,
                              padded=True)
        pw = np.abs(z) ** 2
        fe = pw.sum(axis=0)
        act = fe >= 0.01 * np.max(fe)
        spec = pw[:, act].mean(axis=1)
        tot = spec.sum() + 1e-30
        out["centroid_hz"] = int(round(float(np.sum(f * spec) / tot)))
        out["lo150"] = round(float(spec[f < 150].sum() / tot), 3)
        out["hi4k"] = round(float(spec[f >= 4000].sum() / tot), 3)
        band = (f >= 80) & (f <= 12000)
        shares, ridge = [], []
        for j in np.nonzero(act)[0]:
            col = pw[band, j]
            if col.max() <= 0:
                continue
            fr = f[band][int(np.argmax(col))]
            sel = (f[band] >= fr / 2 ** 0.25) & (f[band] <= fr * 2 ** 0.25)
            shares.append(col[sel].sum() / col.sum())
            ridge.append(fr)
        out["tonal"] = round(float(np.mean(shares)), 2) if len(shares) >= 2 else None
        if len(ridge) >= 2:
            k3 = max(1, min(3, len(ridge) // 3))
            out["glide_st"] = round(float(12 * np.log2(np.median(ridge[-k3:]) / np.median(ridge[:k3]))), 1)
        else:
            out["glide_st"] = None
        left, right = st[:, 0], st[:, 1]
        out["corr"] = round(float(np.sum(left * right) / (np.sqrt(np.sum(left ** 2) * np.sum(right ** 2)) + 1e-30)), 2)
        return out

    def outside(mm, targets):
        d = 0.0
        for key, (lo, hi, scale) in targets.items():
            v = mm.get(key)
            if v is None:
                d += 1
            elif v < lo:
                d += min(2, (lo - v) / scale)
            elif v > hi:
                d += min(2, (v - hi) / scale)
        return d

    def family_space(space, params, fine):
        return {k: (spec if k in fine else [params[k]]) for k, spec in space.items() if k in params}

    for fn in (env, two_rate, glide, rise_track, nyquist_fade, tone, follow_noise, burst, thump, modes, strike,
               room, finish, measure, outside, family_space):
        setattr(m, fn.__name__, fn)
    return m


def _setup() -> None:
    if _absent("hark.forge.engine.dsp"):
        _install("hark.forge.engine.dsp", _dsp())
    dsp = importlib.import_module("hark.forge.engine.dsp")
    if _absent("hark.forge.engine.fx"):
        _install("hark.forge.engine.fx", _fx(dsp))
    fx = importlib.import_module("hark.forge.engine.fx")
    if _absent("hark.forge.recipes.tonal"):
        _install("hark.forge.recipes.tonal", _tonal(dsp, fx))


_setup()


def pytest_report_header(config) -> str:
    return "recipes_ui stand-ins: " + (", ".join(STANDINS) if STANDINS else "none (real forge engine)")
