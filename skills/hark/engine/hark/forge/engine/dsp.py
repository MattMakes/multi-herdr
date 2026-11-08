"""Sound building blocks for forge recipes.

Every function computes its samples. Nothing reads recorded audio. The random
generator is always passed in, and the order of its draws is fixed, so one
seed gives one exact render.
"""

from __future__ import annotations

from collections.abc import Sequence
from typing import Any

import numpy as np
from scipy import signal

SR = 48000
NYQ = SR / 2

BELLS: dict[str, tuple[tuple[float, ...], tuple[float, ...]]] = {
    "church": ((0.5, 1.0, 1.183, 1.506, 2.0, 2.514, 2.662, 3.011, 4.166, 5.433, 6.796, 8.215),
               (0.55, 0.8, 0.7, 0.35, 1.0, 0.45, 0.4, 0.3, 0.25, 0.18, 0.12, 0.08)),
    "tubular": ((1.0, 2.756, 5.404, 8.933, 13.34), (0.35, 1.0, 0.7, 0.4, 0.2)),
    "bar": ((1.0, 2.756, 5.404, 8.933), (1.0, 0.3, 0.12, 0.06)),
    "glass": ((1.0, 2.32, 4.25, 6.63, 9.38), (1.0, 0.45, 0.25, 0.12, 0.06)),
    "bowl": ((1.0, 2.71, 5.13, 8.27), (1.0, 0.6, 0.3, 0.12)),
}


# ---------------------------------------------------------------- basics

def rng(seed: int) -> np.random.Generator:
    """The forge generator for one seed."""
    return np.random.default_rng(int(seed) & 0xFFFFFFFF)


def n_of(seconds: float) -> int:
    """Sample count of a duration, at least 1."""
    return max(1, int(round(float(seconds) * SR)))


def t_of(n: int) -> np.ndarray:
    """Sample times in seconds."""
    return np.arange(int(n)) / SR


def _rms(x: np.ndarray) -> float:
    return float(np.sqrt(np.mean(np.square(x)))) + 1e-12


def _peak(x: np.ndarray) -> float:
    return float(np.max(np.abs(x))) if np.size(x) else 0.0


def _level(x: np.ndarray) -> np.ndarray:
    """Per-sample absolute level, the channel maximum for stereo."""
    a = np.abs(np.asarray(x, dtype=float))
    return a.max(axis=1) if a.ndim == 2 else a


def stereo_of(x: np.ndarray) -> np.ndarray:
    """Mono to 2 equal channels; stereo is returned unchanged."""
    x = np.asarray(x, dtype=float)
    return np.column_stack([x, x]) if x.ndim == 1 else x


def _rise(n: int) -> np.ndarray:
    """Half raised cosine 0..1 over n points."""
    return 0.5 - 0.5 * np.cos(np.linspace(0.0, np.pi, n))


# ---------------------------------------------------------------- noise, envelopes

def _shape_spectrum(x: np.ndarray, power: float) -> np.ndarray:
    spec = np.fft.rfft(x)
    f = np.fft.rfftfreq(len(x), 1.0 / SR)
    if len(f) > 1:
        f[0] = f[1]
    else:
        f[0] = 1.0
    return np.fft.irfft(spec / f ** power, len(x))


def noise(seconds: float, r: np.random.Generator, color: str = "white") -> np.ndarray:
    """Unit-RMS noise; white, pink or brown."""
    x = r.standard_normal(n_of(seconds))
    if color != "white":
        x = _shape_spectrum(x, 0.5 if color == "pink" else 1.0)
    return x / (np.sqrt(np.mean(x * x)) + 1e-12)


def env_exp(seconds: float, tau: float, attack: float = 0.0005) -> np.ndarray:
    """Exponential decay with a short linear attack."""
    n = n_of(seconds)
    e = np.exp(-t_of(n) / max(tau, 1e-5))
    a = min(n_of(attack), n)
    if a > 1:
        e[:a] *= np.linspace(0.0, 1.0, a)
    return e


def env_curve(points: Sequence[tuple[float, float]], seconds: float, shape: str = "lin") -> np.ndarray:
    """Breakpoint envelope; ``exp`` interpolates in dB."""
    pts = sorted((float(t), float(v)) for t, v in points)
    ts = np.array([p[0] for p in pts])
    vs = np.array([p[1] for p in pts])
    t = t_of(n_of(seconds))
    if shape == "exp":
        db = 20 * np.log10(np.maximum(vs, 1e-6))
        return 10 ** (np.interp(t, ts, db) / 20)
    return np.interp(t, ts, vs)


def contact(seconds: float, r: np.random.Generator, bright: float = 0.7) -> np.ndarray:
    """A raised-cosine contact pulse with noisy grit."""
    n = max(2, n_of(seconds))
    pulse = 0.5 - 0.5 * np.cos(np.linspace(0.0, 2 * np.pi, n))
    grit = r.standard_normal(n) * pulse * bright
    return pulse + grit


# ---------------------------------------------------------------- resonators

def modal(freqs: Sequence[float], decays: Sequence[float], amps: Sequence[float], seconds: float,
          r: np.random.Generator, jitter: float = 0.003, glide: float = 0.0) -> np.ndarray:
    """Sum of decaying sine modes."""
    n = n_of(seconds)
    t = t_of(n)
    y = np.zeros(n)
    for f, d, a in zip(freqs, decays, amps):
        if f >= 0.95 * NYQ or a == 0:
            continue
        f2 = f * (1 + jitter * r.standard_normal())
        inst = f2 * (1 - glide * (1 - np.exp(-t / max(d, 1e-4))))
        ph = 2 * np.pi * np.cumsum(inst) / SR + r.uniform(0, 2 * np.pi)
        y += a * np.exp(-t / max(d, 1e-5)) * np.sin(ph)
    return y


def modal_cloud(f_lo: float, f_hi: float, count: int, decay_lo: float, decay_hi: float, seconds: float,
                r: np.random.Generator, tilt: float = -3.0) -> np.ndarray:
    """A dense set of log-spread modes; higher modes take decay_hi."""
    f = np.sort(np.exp(r.uniform(np.log(f_lo), np.log(f_hi), int(count))))
    u = np.log(f / f_lo) / max(np.log(f_hi / f_lo), 1e-9)
    dec = np.exp(np.log(decay_lo) + u * (np.log(decay_hi) - np.log(decay_lo)))
    amp = 10 ** (tilt * np.log2(f / f_lo) / 20) * r.uniform(0.3, 1.0, len(f))
    return modal(f, dec, amp, seconds, r, jitter=0.0)


def excite(resonance: np.ndarray, excitation: np.ndarray) -> np.ndarray:
    """Convolve an excitation with a resonance, cut to the resonance length."""
    y = signal.fftconvolve(np.asarray(excitation, float), np.asarray(resonance, float))
    return y[: len(resonance)]


# ---------------------------------------------------------------- filters

def _clip_f(f: Any) -> Any:
    return np.clip(np.asarray(f, dtype=float), 10.0, 0.95 * NYQ)


def butter(x: np.ndarray, kind: str, f: Any, order: int = 2) -> np.ndarray:
    """Causal Butterworth filter along axis 0 (low, high, band, bandstop)."""
    btype = {"low": "lowpass", "high": "highpass", "band": "bandpass", "bandstop": "bandstop"}[kind]
    fc = _clip_f(f)
    wn = fc.tolist() if fc.ndim else float(fc)
    sos = signal.butter(order, wn, btype=btype, fs=SR, output="sos")
    return signal.sosfilt(sos, np.asarray(x, dtype=float), axis=0)


def peq(x: np.ndarray, f0: float, gain_db: float, q: float = 0.9) -> np.ndarray:
    """RBJ peaking biquad."""
    a_ = 10 ** (gain_db / 40)
    w0 = 2 * np.pi * min(f0, 0.95 * NYQ) / SR
    alpha = np.sin(w0) / (2 * q)
    b = np.array([1 + alpha * a_, -2 * np.cos(w0), 1 - alpha * a_])
    a = np.array([1 + alpha / a_, -2 * np.cos(w0), 1 - alpha / a_])
    return signal.lfilter(b / a[0], a / a[0], np.asarray(x, dtype=float), axis=0)


def _svf_coef(fc: np.ndarray, q: np.ndarray) -> tuple[np.ndarray, ...]:
    g = np.tan(np.pi * fc / SR)
    k = 1.0 / np.maximum(q, 0.05)
    a1 = 1.0 / (1 + g * (g + k))
    a2 = g * a1
    return k, a1, a2, g * a2


def _svf_lti(x: np.ndarray, k: float, a1: float, a2: float, a3: float, mode: str) -> np.ndarray:
    A = np.array([[2 * a1 - 1, -2 * a2], [2 * a2, 1 - 2 * a3]])
    B = np.array([[2 * a2], [2 * a3]])
    rows = {"band": ([a1, -a2], a2), "low": ([a2, 1 - a3], a3)}
    if mode in rows:
        c, d = rows[mode]
    elif mode == "bandn":
        c, d = [k * a1, -k * a2], k * a2
    else:
        c, d = [-k * a1 - a2, k * a2 - (1 - a3)], 1 - k * a2 - a3
    b, a = signal.ss2tf(A, B, np.array([c]), np.array([[d]]))
    return signal.lfilter(b[0], a, x)


def _svf_states(xs: list, b1: list, b2: list, b3: list) -> tuple[np.ndarray, np.ndarray]:
    """Band (v1) and low (v2) outputs of the TPT state-variable filter, sample by sample."""
    v1s, v2s = [], []
    ic1 = ic2 = 0.0
    for i, xi in enumerate(xs):
        v3 = xi - ic2
        v1 = b1[i] * ic1 + b2[i] * v3
        v2 = ic2 + b2[i] * ic1 + b3[i] * v3
        ic1 = 2 * v1 - ic1
        ic2 = 2 * v2 - ic2
        v1s.append(v1)
        v2s.append(v2)
    return np.array(v1s, dtype=float), np.array(v2s, dtype=float)


_SVF_OUT = {"low": lambda x, k, v1, v2: v2, "band": lambda x, k, v1, v2: v1,
            "bandn": lambda x, k, v1, v2: k * v1}


def _svf_high(x: np.ndarray, k: np.ndarray, v1: np.ndarray, v2: np.ndarray) -> np.ndarray:
    return x - k * v1 - v2


def _svf_loop(x: np.ndarray, k: np.ndarray, a1: np.ndarray, a2: np.ndarray, a3: np.ndarray,
              mode: str) -> np.ndarray:
    v1, v2 = _svf_states(x.tolist(), a1.tolist(), a2.tolist(), a3.tolist())
    return _SVF_OUT.get(mode, _svf_high)(x, np.asarray(k, dtype=float), v1, v2)


def svf(x: np.ndarray, cutoff: Any, q: Any = 0.7, mode: str = "band") -> np.ndarray:
    """Time-varying TPT state-variable filter (low, band, bandn, high)."""
    x = np.asarray(x, dtype=float)
    if x.ndim == 2:
        return np.column_stack([svf(x[:, c], cutoff, q, mode) for c in range(x.shape[1])])
    n = len(x)
    fc = np.clip(np.broadcast_to(np.asarray(cutoff, float), (n,)), 20.0, 0.45 * SR)
    qq = np.broadcast_to(np.asarray(q, float), (n,))
    if n and np.ptp(fc) == 0 and np.ptp(qq) == 0:
        k, a1, a2, a3 = (float(v[0]) for v in _svf_coef(fc[:1], qq[:1]))
        return _svf_lti(x, k, a1, a2, a3, mode)
    return _svf_loop(x, *_svf_coef(fc, qq), mode)


# ---------------------------------------------------------------- room, mic, stereo

def _onepole(x: np.ndarray, kind: str, f: float) -> np.ndarray:
    return butter(x, kind, f, order=1)


def room_ir(r: np.random.Generator, rt60: float = 0.25, size: float = 0.5, damp: float = 0.6,
            seconds: float | None = None) -> np.ndarray:
    """A small noise-tail room impulse response with 6 early reflections."""
    sec = seconds if seconds is not None else min(2.0, 1.2 * rt60 + 0.05)
    n = n_of(sec)
    t = t_of(n)
    tail = r.standard_normal(n) * np.exp(-6.91 * t / max(rt60, 0.02))
    lo = _onepole(tail, "low", 1500 + (1 - damp) * 9000)
    mix = (t / sec) ** 0.7
    tail = (1 - mix * damp) * tail + mix * damp * lo
    ir = 0.3 * tail
    ir[0] += 1.0
    for _ in range(6):
        d = int(r.uniform(0.002, 0.004 + 0.02 * size) * SR)
        amp = r.uniform(0.15, 0.45)
        sign = r.choice([-1.0, 1.0])
        if d < n:
            ir[d] += amp * sign
    return ir


def room(x: np.ndarray, r: np.random.Generator, rt60: float = 0.25, size: float = 0.5, damp: float = 0.6,
         wet: float = 0.12) -> np.ndarray:
    """Dry signal plus a normalised small-room wet part."""
    ir = room_ir(r, rt60, size, damp)
    ir[0] = 0.0
    x = np.asarray(x, dtype=float)
    norm = np.sqrt(np.sum(ir * ir)) + 1e-9
    kern = ir if x.ndim == 1 else ir[:, None]
    conv = signal.fftconvolve(x, kern, axes=0) if x.ndim == 2 else signal.fftconvolve(x, ir)
    out = wet * conv / norm
    out[: len(x)] += x
    return out


def mic(x: np.ndarray, r: np.random.Generator, low_cut: float = 35.0, air_db: float = -2.0,
        floor_db: float = -66.0, color: tuple[float, float] | None = None) -> np.ndarray:
    """Microphone colour: low cut, air shelf, optional colour bump, noise floor."""
    y = _onepole(np.asarray(x, float), "high", low_cut)
    y = peq(y, 10000.0, air_db, 0.6)
    if color is not None:
        y = peq(y, color[0], color[1], 0.8)
    pk = _peak(y) + 1e-12
    g = pk * 10 ** (floor_db / 20)
    sec = len(y) / SR
    if y.ndim == 1:
        return y + g * noise(sec, r, "pink")[: len(y)]
    return y + g * np.column_stack([noise(sec, r, "pink")[: len(y)] for _ in range(y.shape[1])])


def _delayed(x: np.ndarray, d: int) -> np.ndarray:
    out = np.zeros_like(x)
    if d < len(x):
        out[d:] = x[: len(x) - d]
    return out


def _pan_gains(pan: Any) -> tuple[Any, Any]:
    theta = (np.clip(pan, -1, 1) + 1) * np.pi / 4
    return np.cos(theta) * np.sqrt(2), np.sin(theta) * np.sqrt(2)


def stereo(x: np.ndarray, r: np.random.Generator, width: float = 0.3, pan: float = 0.0) -> np.ndarray:
    """Mono to stereo with a short-delay side signal and a constant-power pan."""
    x = np.asarray(x, dtype=float)
    d1 = int(r.uniform(0.2, 0.9) * 1e-3 * SR)
    d2 = int(r.uniform(0.2, 0.9) * 1e-3 * SR)
    side = butter(_delayed(x, d1) - _delayed(x, d2), "high", 300.0, 2)
    gl, gr = _pan_gains(pan)
    return np.column_stack([(x + width * side) * gl, (x - width * side) * gr])


# ---------------------------------------------------------------- finishing

def fade(x: np.ndarray, fin: float = 0.001, fout: float = 0.02) -> np.ndarray:
    """Raised-cosine fade in and fade out."""
    y = np.array(x, dtype=float, copy=True)
    n = len(y)
    a, b = min(n, n_of(fin)), min(n, n_of(fout))
    if a > 1:
        w = 0.5 - 0.5 * np.cos(np.linspace(0, np.pi, a))
        y[:a] *= w if y.ndim == 1 else w[:, None]
    if b > 1:
        w = 0.5 + 0.5 * np.cos(np.linspace(0, np.pi, b))
        y[n - b:] *= w if y.ndim == 1 else w[:, None]
    return y


def place(dst: np.ndarray, x: np.ndarray, t: float, gain: float = 1.0) -> np.ndarray:
    """Add x into dst at time t, in place."""
    i = n_of(t) if t > 0 else 0
    if i >= len(dst):
        return dst
    m = min(len(x), len(dst) - i)
    dst[i:i + m] += gain * np.asarray(x[:m])
    return dst


def trim_tail(x: np.ndarray, floor_db: float = -70.0, keep: float = 0.01) -> np.ndarray:
    """Cut the tail after the last sample within floor_db of the peak."""
    x = np.asarray(x)
    if len(x) == 0:
        return x
    m = np.abs(x).max(axis=1) if x.ndim == 2 else np.abs(x)
    loud = np.nonzero(m >= m.max() * 10 ** (floor_db / 20))[0]
    end = len(x) if len(loud) == 0 else min(len(x), int(loud[-1]) + n_of(keep))
    return x[:end]


def normalize(x: np.ndarray, peak_db: float = -1.0) -> np.ndarray:
    """Scale to a peak level in dBFS."""
    x = np.asarray(x, dtype=float)
    return x * 10 ** (peak_db / 20) / (_peak(x) + 1e-12)


# ---------------------------------------------------------------- curves, oscillators

def _resample_curve(a: np.ndarray, n: int) -> np.ndarray:
    return a if len(a) == n else np.interp(np.linspace(0, len(a) - 1, n), np.arange(len(a)), a)


def curve(spec: Any, n: int, shape: str = "lin") -> np.ndarray:
    """A control value per sample from a scalar, breakpoints or an array."""
    if np.isscalar(spec):
        out = np.full(n, float(spec))
    elif isinstance(spec, (list, tuple)) and spec and isinstance(spec[0], (list, tuple)):
        out = env_curve(spec, n / SR, shape)[:n]
    else:
        out = _resample_curve(np.asarray(spec, dtype=float), n)
    return out


def sweep(f0: float, f1: float, seconds: float, curve: str = "exp", power: float = 1.0) -> np.ndarray:
    """A frequency track from f0 to f1."""
    u = np.linspace(0.0, 1.0, n_of(seconds))
    if curve == "lin":
        return f0 + (f1 - f0) * u
    if curve == "ease":
        u = u ** power
    return f0 * (f1 / f0) ** u


def _blep(t: np.ndarray, dt: np.ndarray) -> np.ndarray:
    out = np.zeros_like(t)
    lo = t < dt
    u = t[lo] / dt[lo]
    out[lo] = 2 * u - u * u - 1
    hi = (~lo) & (t > 1 - dt)
    u = (t[hi] - 1) / dt[hi]
    out[hi] = u * u + 2 * u + 1
    return out


def _square(ph: np.ndarray, dt: np.ndarray) -> np.ndarray:
    sq = np.where(ph < 0.5, 1.0, -1.0)
    return sq + _blep(ph, dt) - _blep((ph + 0.5) % 1.0, dt)


def _tri(f: np.ndarray, sq: np.ndarray, p0: float) -> np.ndarray:
    zi = np.array([0.9995 * -0.9995 * (1 - 2 * p0)])
    y, _ = signal.lfilter([1.0], [1.0, -0.9995], 4 * f / SR * sq, zi=zi)
    return y


def osc(freq: Any, shape: str = "sine", seconds: float | None = None, phase: float = 0.0) -> np.ndarray:
    """Band-limited oscillator (sine, saw, square, tri) with PolyBLEP edges."""
    f = np.asarray(freq, dtype=float)
    if f.ndim == 0:
        f = np.full(n_of(seconds or 1.0), float(f))
    dt = np.clip(f / SR, 1e-9, 0.49)
    p0 = phase % 1.0
    ph = (p0 + np.concatenate([[0.0], np.cumsum(dt[:-1])])) % 1.0
    if shape == "sine":
        y = np.sin(2 * np.pi * ph)
    elif shape == "saw":
        y = 2 * ph - 1 - _blep(ph, dt)
    elif shape == "square":
        y = _square(ph, dt)
    else:
        y = _tri(f, _square(ph, dt), p0)
    return y


def _shepard_voice(r: np.random.Generator, n: int, u: np.ndarray, cfg: tuple) -> np.ndarray:
    f_lo, octs, tilt, kind, q = cfg
    f = f_lo * 2 ** (u * octs)
    a = (0.5 - 0.5 * np.cos(2 * np.pi * u)) * 10 ** (tilt * u * octs / 20)
    if kind == "tone":
        return a * np.sin(2 * np.pi * np.cumsum(f) / SR + r.uniform(0, 2 * np.pi))
    return a * svf(r.standard_normal(n), f, q, "bandn") * np.sqrt(q)


def shepard(seconds: float, r: np.random.Generator, up: bool = True, rate: float = 0.5, f_lo: float = 40.0,
            f_hi: float = 10000.0, kind: str = "tone", q: float = 6.0, tilt: float = -1.5) -> np.ndarray:
    """Shepard-Risset glissando of tones or noise bands, unit RMS."""
    n = n_of(seconds)
    t = t_of(n)
    octs = np.log2(f_hi / f_lo)
    v = max(2, int(round(octs)))
    sign = 1.0 if up else -1.0
    cfg = (f_lo, octs, tilt, kind, q)
    y = np.zeros(n)
    for k in range(v):
        y += _shepard_voice(r, n, (k / v + sign * rate * t / octs) % 1.0, cfg)
    return y / _rms(y)


# ---------------------------------------------------------------- air

def wander(seconds: float, r: np.random.Generator, rate: float = 4.0, octaves: int = 3) -> np.ndarray:
    """Smooth random control signal, mean 0 and unit standard deviation."""
    n = n_of(seconds)
    m = max(16, int(1000 * n / SR) + 4)
    acc = np.zeros(m)
    for i in range(octaves):
        fr = float(np.clip(rate / 3 ** i, 0.05, 400))
        pad = int(2000 / fr) + 8
        raw = r.standard_normal(m + 2 * pad)
        sos = signal.butter(2, min(fr / 500, 0.999), "low", output="sos")
        sm = signal.sosfiltfilt(sos, raw)[pad:pad + m]
        acc += sm / np.sqrt(fr) * 0.8 ** i
    y = np.interp(t_of(n) * 1000, np.arange(m), acc)
    y = y - y.mean()
    return y / (y.std() + 1e-12)


def _air_bed(n: int, r: np.random.Generator, center: float, lo_db: float, hi_db: float) -> np.ndarray:
    spec = np.fft.rfft(r.standard_normal(n))
    f = np.fft.rfftfreq(n, 1.0 / SR)
    u = np.log2(np.maximum(f, 5) / center)
    g = -10 * np.log10(2 ** (-lo_db / 3.0103 * u) + 2 ** (hi_db / 3.0103 * u))
    x = np.fft.irfft(spec * 10 ** (g / 20), n)
    return x / _rms(x)


def air(seconds: float, r: np.random.Generator, center: float = 1200.0, lo_db: float = 6.0, hi_db: float = 4.5,
        flutter: float = 0.5, rate: float = 6.0, bright: float = 0.6, whistle: float = 0.0) -> np.ndarray:
    """Moving-air turbulence with gusts, unit RMS."""
    n = n_of(seconds)
    x = _air_bed(n, r, center, lo_db, hi_db)
    c = wander(seconds, r, rate)
    if bright > 0:
        x = svf(x, np.clip(6 * center * 2 ** (1.2 * bright * flutter * c), 200, 20000), 0.6, "low")
    if whistle > 0:
        c2 = 0.6 * c + 0.8 * wander(seconds, r, 0.4 * rate)
        wh = svf(r.standard_normal(n), center * 2 ** (0.5 * c2), 9, "bandn")
        x = x / _rms(x) + whistle * wh / _rms(wh)
    x = x * np.exp(1.1 * flutter * c)
    return x / _rms(x)


# ---------------------------------------------------------------- designed sources

def _sub_click(n: int, r: np.random.Generator) -> np.ndarray:
    k = min(n, n_of(0.012))
    t = t_of(k)
    ck = butter(r.standard_normal(k), "high", 1800.0, 2) * np.exp(-t / 0.0012)
    ck += 0.6 * np.sin(2 * np.pi * 2400 * t) * np.exp(-t / 0.003)
    return ck / (_peak(ck) + 1e-12)


def sub_drop(seconds: float = 1.6, r: np.random.Generator | None = None, f0: float = 120.0, f1: float = 34.0,
             drop: float = 0.25, decay: float = 0.6, harm: float = 0.3, click: float = 0.25,
             attack: float = 0.004) -> np.ndarray:
    """A falling sub tone with an optional click, -1 dBFS."""
    r = rng(0) if r is None else r
    n = n_of(seconds)
    t = t_of(n)
    f = f1 + (f0 - f1) * np.exp(-t / max(drop, 1e-3))
    ph = 2 * np.pi * np.cumsum(f) / SR
    y = np.sin(ph) + harm * (0.55 * np.sin(2 * ph + 0.3) + 0.3 * np.sin(3 * ph + 0.7))
    e = np.exp(-t / max(decay, 1e-3))
    a = min(n, n_of(attack))
    e[:a] *= _rise(a)
    y = y * e
    if click > 0:
        ck = _sub_click(n, r)
        y[: len(ck)] += click * ck
    return normalize(fade(y, 0.0, 0.3 * seconds), -1.0)


def _bell_partials(partials: Any) -> tuple[Sequence[float], Sequence[float]]:
    return BELLS[partials] if isinstance(partials, str) else partials


def _bell_strike(n: int, r: np.random.Generator, f0: float) -> np.ndarray:
    k = min(n, n_of(0.006))
    s = butter(r.standard_normal(k), "band", (2 * f0, min(12 * f0, 18000.0)), 2)
    return s * np.exp(-t_of(k) / 0.0008)


def _bell_partial(r: np.random.Generator, t: np.ndarray, f0: float, qa: tuple, cfg: tuple) -> np.ndarray | None:
    """One beating partial pair; None above 0.45 SR (no random draws then)."""
    q, a = qa
    stretch, bright, beat, decay, slope = cfg
    q2 = q ** (1 + stretch)
    f = f0 * q2
    if f > 0.45 * SR:
        return None
    a2 = a * q2 ** (1.2 * (bright - 0.5))
    b = beat * r.uniform(0.4, 1.6) * (1.0 if q2 >= 1 else 0.5)
    p1, p2 = r.uniform(0, 6.3), r.uniform(0, 6.3)
    e = a2 * np.exp(-t / max(decay * q2 ** -slope, 1e-4))
    return e * (np.sin(2 * np.pi * f * t + p1) + 0.7 * np.sin(2 * np.pi * (f + b) * t + p2)) / 1.7


def _add_strike(y: np.ndarray, r: np.random.Generator, f0: float, strike: float) -> None:
    s = _bell_strike(len(y), r, f0)
    y[: len(s)] += strike * s * _peak(y) / (_peak(s) + 1e-12)


def bell(f0: float, seconds: float, r: np.random.Generator, partials: Any = "bar", decay: float = 1.2,
         slope: float = 0.8, bright: float = 0.5, beat: float = 1.0, stretch: float = 0.0,
         strike: float = 0.3) -> np.ndarray:
    """Additive bell with beating partial pairs, -1 dBFS."""
    n = n_of(seconds)
    t = t_of(n)
    cfg = (stretch, bright, beat, decay, slope)
    y = np.zeros(n)
    for qa in zip(*_bell_partials(partials)):
        part = _bell_partial(r, t, f0, qa, cfg)
        if part is not None:
            y += part
    k = min(n, n_of(0.0015))
    y[:k] *= np.linspace(0, 1, k)
    if strike > 0:
        _add_strike(y, r, f0, strike)
    return normalize(fade(y, 0.0, 0.15 * seconds), -1.0)


def fm(fc: float, ratio: float, index: float, seconds: float, decay: float = 1.0, index_decay: float = 0.3,
       attack: float = 0.002) -> np.ndarray:
    """Two-operator FM tone with a decaying index."""
    n = n_of(seconds)
    t = t_of(n)
    ind = index * np.exp(-t / max(index_decay, 1e-4))
    y = np.sin(2 * np.pi * fc * t + ind * np.sin(2 * np.pi * fc * ratio * t)) * np.exp(-t / max(decay, 1e-4))
    a = min(n, n_of(attack))
    y[:a] *= _rise(a)
    return fade(y, 0.0, 0.15 * seconds)


# ---------------------------------------------------------------- layering

def onset(x: np.ndarray, db: float = -24.0) -> float:
    """Time of the first sample within db of the peak; 0.0 for silence."""
    m = _level(x)
    if len(m) == 0 or m.max() == 0:
        return 0.0
    return int(np.argmax(m >= m.max() * 10 ** (db / 20))) / SR


def align(x: np.ndarray, t: float = 0.0, db: float = -24.0) -> np.ndarray:
    """Shift x so its onset lands on t."""
    x = np.asarray(x, dtype=float)
    d = (n_of(t) if t > 0 else 0) - int(round(onset(x, db) * SR))
    if d >= 0:
        pad = np.zeros((d,) + x.shape[1:])
        return np.concatenate([pad, x])
    return x[-d:]


def pan_to_stereo(x: np.ndarray, pan: float = 0.0) -> np.ndarray:
    """Constant-power pan of mono, or balance of stereo."""
    gl, gr = _pan_gains(pan)
    x = np.asarray(x, dtype=float)
    if x.ndim == 1:
        return np.column_stack([x * gl, x * gr])
    return x * np.array([gl, gr])


def _layer(item: Any) -> dict:
    if isinstance(item, dict):
        return {"x": np.asarray(item["x"], float), "t": float(item.get("t", 0.0)),
                "gain": float(item.get("gain", 0.0)), "pan": item.get("pan")}
    x, t, g = item[0], item[1], item[2]
    return {"x": np.asarray(x, float), "t": float(t), "gain": float(g), "pan": item[3] if len(item) > 3 else None}


def _layer_start(ly: dict, onset_db: float | None) -> dict:
    if onset_db is not None:
        ly["t"] -= onset(ly["x"], onset_db)
    if ly["t"] < 0:
        ly["x"] = ly["x"][n_of(-ly["t"]):]
        ly["t"] = 0.0
    return ly


def _stereo_needed(lys: list[dict]) -> bool:
    return any(ly["x"].ndim == 2 or ly["pan"] is not None for ly in lys)


def _mix_len(lys: list[dict], length: float | None) -> int:
    return n_of(length) if length is not None else max(int(round(ly["t"] * SR)) + len(ly["x"]) for ly in lys)


def _place_layer(out: np.ndarray, ly: dict, st: bool) -> None:
    x = ly["x"]
    if st and (x.ndim == 1 or ly["pan"] is not None):
        x = pan_to_stereo(x, ly["pan"] or 0.0)
    i = int(round(ly["t"] * SR))
    if i < len(out):
        m = min(len(x), len(out) - i)
        out[i:i + m] += 10 ** (ly["gain"] / 20) * x[:m]


def stack(layers: Sequence[Any], length: float | None = None, onset_db: float | None = None,
          peak_db: float | None = None) -> np.ndarray:
    """Mix layers placed in time, with gain in dB and optional pan."""
    lys = [_layer_start(_layer(it), onset_db) for it in layers]
    st = _stereo_needed(lys)
    n = _mix_len(lys, length)
    out = np.zeros((n, 2) if st else n)
    for ly in lys:
        _place_layer(out, ly, st)
    return normalize(out, peak_db) if peak_db is not None else out


def xfade(a: np.ndarray, b: np.ndarray, seconds: float = 0.02, power: bool = True) -> np.ndarray:
    """Crossfade the end of a into the start of b."""
    a, b = np.asarray(a, float), np.asarray(b, float)
    k = min(n_of(seconds), len(a), len(b))
    u = np.linspace(0, 1, k)
    fo, fi = (np.cos(u * np.pi / 2), np.sin(u * np.pi / 2)) if power else (1 - u, u)
    if a.ndim == 2:
        fo, fi = fo[:, None], fi[:, None]
    mid = a[len(a) - k:] * fo + b[:k] * fi
    return np.concatenate([a[: len(a) - k], mid, b[k:]])
