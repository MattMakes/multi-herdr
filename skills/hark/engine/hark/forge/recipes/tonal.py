"""Shared tonal building blocks for the UI and tonal recipes.

A soft onset with a natural ring-down, pitch tracks, partials, pitch-following
noise, contact bursts, struck modes, a small room and a finishing chain. Film UI
sounds start over 20-60 ms, ring down within 0.2-0.4 s and have some stereo
width, so these blocks default to that shape. Every random draw uses the
generator that the caller passes.
"""

from __future__ import annotations

from collections.abc import Sequence
from typing import Any

import numpy as np

from hark.forge.engine import dsp, fx

SR = 48000
_NYQ_EDGE = 0.45 * SR
_TWO_PI = 2.0 * np.pi


def _rise(t: np.ndarray, onset: float, curve: float) -> np.ndarray:
    u = np.clip(t / max(onset, 1e-4), 0.0, 1.0) ** curve
    return 0.5 - 0.5 * np.cos(np.pi * u)


def env(t: np.ndarray, onset: float, tau: float, hold: float = 0, curve: float = 1,
        dcurve: float = 1) -> np.ndarray:
    """Raised-cosine rise over `onset` s, then an exponential-like decay."""
    x = np.maximum(t - onset - hold, 0.0) / max(tau, 1e-4)
    decay = np.exp(-x) if dcurve == 1 else np.exp(-(x ** dcurve))
    return _rise(t, onset, curve) * decay


def two_rate(t: np.ndarray, onset: float, fast: float, tf: float, ts: float, curve: float = 1) -> np.ndarray:
    """Rise, then a fast decay share and a slow ring share."""
    tm = np.maximum(t - onset, 0.0)
    mix = fast * np.exp(-tm / max(tf, 1e-4)) + (1 - fast) * np.exp(-tm / max(ts, 1e-4))
    return _rise(t, onset, curve) * mix


def glide(t: np.ndarray, f0: float, bend_st: float = 0, ptau: float = 0.02) -> np.ndarray:
    """Pitch track that starts bend_st semitones off f0 and settles on f0."""
    return f0 * 2.0 ** ((bend_st / 12.0) * np.exp(-t / max(ptau, 1e-4)))


def rise_track(t: np.ndarray, f0: float, rise_st: float, rtau: float) -> np.ndarray:
    """Pitch track that rises rise_st semitones from f0 (a bubble)."""
    return f0 * 2.0 ** ((rise_st / 12.0) * (1.0 - np.exp(-t / max(rtau, 1e-4))))


def nyquist_fade(f: Any, q: float = 1.0) -> Any:
    """Gain of a partial at ratio q on track f: fades out over 2.4 kHz below 0.45 SR."""
    return np.clip((_NYQ_EDGE - q * np.asarray(f, dtype=float)) / 2400.0, 0.0, 1.0)


def tone(f: Any, r: np.random.Generator, parts: Sequence[tuple[float, float, float]] = (),
         e: np.ndarray | None = None) -> np.ndarray:
    """Sine on track f with optional partials (ratio, amp, decay power)."""
    f = np.asarray(f, dtype=float)
    e = np.ones(len(f)) if e is None else np.asarray(e, dtype=float)
    phase = _TWO_PI * np.cumsum(f) / SR + r.uniform(0, _TWO_PI)
    y = np.sin(phase) * e
    for q, a, d in parts:
        if a <= 0:
            continue
        y = y + a * nyquist_fade(f, q) * e ** d * np.sin(q * phase + r.uniform(0, _TWO_PI))
    return y


def follow_noise(n: int, r: np.random.Generator, fc: Any, q: float, e: np.ndarray) -> np.ndarray:
    """Band-passed noise around fc, as strong as a unit sine where e is live."""
    fc = np.broadcast_to(np.minimum(np.asarray(fc, dtype=float), 16000.0), (n,)).copy()
    b = dsp.svf(r.standard_normal(n), fc, q, "band")
    e = np.asarray(e, dtype=float)
    live = e > 0.1 * (np.max(e) + 1e-12)
    rms = np.sqrt(np.mean(b[live] ** 2)) if np.any(live) else 0.0
    return b * (np.sqrt(0.5) / (rms + 1e-12)) * e


def burst(n: int, r: np.random.Generator, lo: float, hi: float, tau: float, rise: float = 0.0003,
          order: int = 2) -> np.ndarray:
    """Short band-passed noise contact, peak 1, zero-padded to n."""
    k = min(n, dsp.n_of(max(6 * tau + rise, 0.002)))
    w = dsp.butter(r.standard_normal(k), "band", (max(lo, 20.0), min(hi, _NYQ_EDGE)), order)
    t = dsp.t_of(k)
    w = w * (1 - np.exp(-t / max(rise, 1e-5))) * np.exp(-t / max(tau, 1e-5))
    out = np.zeros(n)
    out[:k] = w / (np.max(np.abs(w)) + 1e-12)
    return out


def thump(n: int, f: float, tau: float, rise: float = 0.002) -> np.ndarray:
    """Soft low sine thump, peak 1, no random draw."""
    t = dsp.t_of(n)
    y = np.sin(_TWO_PI * f * t) * (1 - np.exp(-t / max(rise, 1e-4))) * np.exp(-t / max(tau, 1e-4))
    return y / (np.max(np.abs(y)) + 1e-12)


def modes(n: int, r: np.random.Generator, freqs: Sequence[float], amps: Sequence[float], taus: Sequence[float],
          jitter: float = 0, bend: float = 0, bend_tau: float = 0.004) -> np.ndarray:
    """Sum of damped sines; a hard strike starts slightly sharp (bend)."""
    t = dsp.t_of(n)
    b = 1.0 + bend * np.exp(-t / max(bend_tau, 1e-4))
    y = np.zeros(n)
    for f, a, tau in zip(freqs, amps, taus):
        if a <= 0 or f >= _NYQ_EDGE:
            continue
        fj = f * (1 + jitter * r.standard_normal())
        phase = _TWO_PI * np.cumsum(fj * b) / SR + r.uniform(0, _TWO_PI)
        y += a * np.exp(-t / max(tau, 1e-5)) * np.sin(phase)
    return y


def strike(y: np.ndarray, r: np.random.Generator, contact_s: float, bright: float = 0.5) -> np.ndarray:
    """Drive a resonator response with a contact pulse of contact_s seconds."""
    return dsp.excite(y, dsp.contact(contact_s, r, bright))


def room(y: np.ndarray, r: np.random.Generator, wet: float, rt60: float, size: float = 0.15,
         width: float = 0.3) -> np.ndarray:
    """Small stereo room; a wet level at or under 0.005 is a bypass with no draw."""
    if wet <= 0.005:
        return y
    return fx.reverb(y, r, rt60=rt60, size=size, predelay=0.004, damp=0.6, wet=wet, stereo=True, width=width)


def _cos2_tail(y: np.ndarray, k: int) -> np.ndarray:
    if k <= 0:
        return y
    g = np.cos(np.linspace(0, np.pi / 2, k)) ** 2
    g[-1] = 0.0  # cos(pi/2) is not exactly 0 in floating point
    y = y.copy()
    y[-k:] = y[-k:] * (g[:, None] if y.ndim == 2 else g)
    return y


def _start_rise(y: np.ndarray) -> np.ndarray:
    k = min(len(y), dsp.n_of(0.0003))
    if k <= 0:
        return y
    g = 0.5 - 0.5 * np.cos(np.pi * np.linspace(0, 1, k))
    y = y.copy()
    y[:k] = y[:k] * (g[:, None] if y.ndim == 2 else g)
    return y


def _to_stereo(y: np.ndarray, width: float, cut: float, pan: float) -> np.ndarray:
    s = fx.decorrelate(y, width, cut) if width > 0.005 else np.column_stack([y, y])
    if abs(pan) > 0.005:
        th = (np.clip(pan, -1, 1) + 1) * np.pi / 4
        s = s * np.array([np.cos(th), np.sin(th)]) * np.sqrt(2)
    return s


def finish(y: np.ndarray, r: np.random.Generator, hp: float = 80, lp: float = 12000, room_wet: float = 0,
           rt60: float = 0.15, width: float = 0, cut: float = 500, end_db: float = -60, fade: float = 0.006,
           pan: float = 0, size: float = 0.15) -> np.ndarray:
    """Mono in, stereo out: dry pre-fade, filters, width, pan, room, trim, fades."""
    y = np.asarray(y, dtype=float)
    y = _cos2_tail(y, min(len(y), dsp.n_of(0.02), int(0.15 * len(y))))
    y = dsp.butter(y, "high", hp, 4)
    if lp < 17500:
        y = dsp.butter(y, "low", lp, 2)
    s = _to_stereo(y, width, cut, pan)
    s = room(s, r, room_wet, rt60, size, 0.3 + width)
    s = dsp.trim_tail(s, end_db, 0.01)
    s = _cos2_tail(s, min(len(s), dsp.n_of(fade)))
    return _start_rise(s)


# ---------------------------------------------------------------- measure

def _level_track(m: np.ndarray) -> np.ndarray:
    n = len(m)
    cs = np.concatenate([[0.0], np.cumsum(m * m)])
    c = np.arange(0, n, 48)
    a = np.clip(c - 120, 0, n)
    b = np.clip(c + 120, 0, n)
    p = (cs[b] - cs[a]) / np.maximum(b - a, 1)
    return np.convolve(p, np.ones(10) / 10, mode="same")


def _first(cond: np.ndarray) -> int | None:
    idx = np.flatnonzero(cond)
    return int(idx[0]) if len(idx) else None


def _level_measures(m: np.ndarray) -> dict:
    tr = _level_track(m)
    ip = int(np.argmax(tr))
    pk = float(tr[ip])
    before = np.flatnonzero(tr[: ip + 1] >= pk * 1e-2)
    on = _first(tr >= pk * 1e-3) or 0
    ends = np.flatnonzero(tr >= pk * 10 ** -4.5)
    end = int(ends[-1]) if len(ends) else len(m) / 48
    t20 = _first(tr[ip:] < pk * 1e-2)
    t40 = _first(tr[ip:] < pk * 1e-4)
    return {"dur_s": round(end * 0.001 - on * 0.001, 3),
            "attack_ms": ip - int(before[0]) if len(before) else None,
            "t20_ms": t20, "t40_ms": t40}


def _frames(m: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    if len(m) < 2048:
        m = np.concatenate([m, np.zeros(2048 - len(m))])
    k = 1 + (len(m) - 2048) // 240
    idx = np.arange(2048)[None, :] + 240 * np.arange(k)[:, None]
    P = np.abs(np.fft.rfft(m[idx] * np.hanning(2048), axis=1)) ** 2
    return P, np.fft.rfftfreq(2048, 1 / SR)


def _ridge(P: np.ndarray, f: np.ndarray) -> tuple[list[float], list[float]]:
    band = (f >= 80) & (f <= 12000)
    fb = f[band]
    ridge, share = [], []
    for row in P[:, band]:
        if row.max() <= 0:
            continue
        fr = fb[int(np.argmax(row))]
        near = (fb >= fr / 2 ** 0.25) & (fb <= fr * 2 ** 0.25)
        ridge.append(float(fr))
        share.append(float(row[near].sum() / (row.sum() + 1e-30)))
    return ridge, share


def _active(P: np.ndarray) -> np.ndarray:
    e = P.sum(axis=1)
    return P[e >= 0.01 * e.max()] if e.max() > 0 else P[:0]


def _band_measures(act: np.ndarray, f: np.ndarray) -> dict:
    spec = act.mean(axis=0)
    tot = spec.sum() + 1e-30
    return {"centroid_hz": int(round(float((spec * f).sum() / tot))),
            "lo150": round(float(spec[f < 150].sum() / tot), 3),
            "hi4k": round(float(spec[f >= 4000].sum() / tot), 3)}


def _tonal_measures(act: np.ndarray, f: np.ndarray) -> dict:
    ridge, share = _ridge(act, f)
    res: dict = {}
    if len(share) >= 2:
        k3 = max(1, min(3, len(ridge) // 3))
        res = {"tonal": round(float(np.mean(share)), 2),
               "glide_st": round(float(12 * np.log2(np.median(ridge[-k3:]) / np.median(ridge[:k3]))), 1)}
    return res


def _spectral_measures(m: np.ndarray) -> dict:
    P, f = _frames(m)
    act = _active(P)
    out: dict = {"centroid_hz": None, "lo150": None, "hi4k": None, "tonal": None, "glide_st": None}
    if len(act):
        out.update(_band_measures(act, f))
        out.update(_tonal_measures(act, f))
    return out


def measure(x: np.ndarray) -> dict:
    """Catalog measures of one render on silence (no model)."""
    s = dsp.stereo_of(np.asarray(x, dtype=float))
    m = s.mean(axis=1)
    out = _level_measures(m)
    out.update(_spectral_measures(m))
    L, R = s[:, 0], s[:, 1]
    out["corr"] = round(float(np.sum(L * R) / (np.sqrt(np.sum(L * L) * np.sum(R * R)) + 1e-30)), 2)
    return out


def outside(m: dict, targets: dict) -> float:
    """Distance of measures m from target ranges {key: (lo, hi, scale)}; 0 when all met."""
    total = 0.0
    for key, (lo, hi, scale) in targets.items():
        v = m.get(key)
        if v is None:
            total += 1.0
        elif v < lo:
            total += min(2.0, (lo - v) / scale)
        elif v > hi:
            total += min(2.0, (v - hi) / scale)
    return float(total)


def family_space(space: dict, params: dict, fine: Sequence[str]) -> dict:
    """Keep the fine keys' ranges; pin every other key to the member's value."""
    return {k: (spec if k in fine else [params[k]]) for k, spec in space.items() if k in params}
