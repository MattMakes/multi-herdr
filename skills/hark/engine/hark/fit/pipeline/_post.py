"""The render-side signal steps: post stage, gate, bed layer, take high-pass, finish."""

from __future__ import annotations

import math
from typing import Callable

import numpy as np
from scipy import signal

SR = 48000
FADE_IN_S = 0.002
FADE_MIN_S = 0.00025
PEAK_DB = -1
TAKE_DC_HZ = 8
GATE_SMOOTH_S = 0.001
ATTACK_HOP = 12          # 0.25 ms frames
ATTACK_FRAME_S = 0.00025

Points = list  # [[t s, dB], ...]


# ---------------------------------------------------------------- small helpers


def _per_channel(y: np.ndarray, fn: Callable[[np.ndarray], np.ndarray]) -> np.ndarray:
    if y.ndim == 1:
        return fn(y)
    return np.column_stack([fn(y[:, c]) for c in range(y.shape[1])])


def mono(y: np.ndarray) -> np.ndarray:
    return y if y.ndim == 1 else y.mean(axis=1)


def _scale(y: np.ndarray, g: np.ndarray) -> np.ndarray:
    return y * (g if y.ndim == 1 else g[:, None])


def frame_db(m: np.ndarray, hop: int, n: int | None = None) -> np.ndarray:
    k = len(m) // hop if n is None else n
    p = np.mean(m[: k * hop].reshape(k, hop) ** 2, axis=1) if k else np.zeros(0)
    return 10 * np.log10(np.maximum(p, 1e-20))


def smooth121(x: np.ndarray) -> np.ndarray:
    if len(x) < 2:
        return x.copy()
    p = np.pad(x, 1, mode="edge")
    return 0.25 * p[:-2] + 0.5 * p[1:-1] + 0.25 * p[2:]


# ---------------------------------------------------------------- EQ and gain


def eq(y: np.ndarray, gains_db: list) -> np.ndarray:
    """The forge minimum-phase third-octave EQ with 28 band gains."""
    from hark.forge.engine import fx

    g = np.asarray(gains_db, dtype=np.float64)
    return _per_channel(y, lambda c: np.asarray(fx.eq_bands(c, g, phase="min"), dtype=np.float64))


# ---------------------------------------------------------------- attack clamp


def _pre_frames(t_on: float, scale: float, anchor: float) -> int:
    if anchor and scale != 1:
        return int(round((anchor + (t_on - anchor) * scale) / ATTACK_FRAME_S))
    return int(round(t_on * scale / ATTACK_FRAME_S))


def _orig_time(t: np.ndarray, scale: float, anchor: float) -> np.ndarray:
    if scale == 1:
        return t
    return anchor + (t - anchor) / scale if anchor else t / scale


def _pre_gains(e: np.ndarray, db: np.ndarray, k: int, scale: float, anchor: float) -> np.ndarray:
    """Cuts that hold the pre-onset frames 1.5 dB over the recording's pre-roll."""
    if not (k and len(db)):
        return np.zeros(k)
    tc = _orig_time((np.arange(k) + 0.5) * ATTACK_FRAME_S, scale, anchor)
    want = np.interp(tc / ATTACK_FRAME_S - 0.5, np.arange(len(db)), db)
    return np.minimum(0.0, want + 1.5 - e[:k])


def _attack_want(att: list, na: int) -> np.ndarray:
    if len(att) > 1:
        return np.interp(np.linspace(0, len(att) - 1, na), np.arange(len(att)), att)
    return np.full(na, att[0])


def _attack_gains(e: np.ndarray, att: list, na: int) -> np.ndarray:
    """Gains (+-9 dB) that follow the recording's attack, faded out over 8 frames."""
    m = len(e)
    if not (na and m):
        return np.zeros(m)
    ga = np.clip(_attack_want(att, na)[:m] - e, -9, 9)
    fade = np.minimum(1.0, (m - 1 - np.arange(m)) / 8.0)
    return ga * np.clip(fade, 0, 1)


def _clamp_frames(n: int, pre: dict, scale: float, anchor: float) -> tuple:
    """(pre-onset frames, attack frames, all frames) after time scaling, capped at n samples."""
    np_ = _pre_frames(float(pre["t_on"]), scale, anchor)
    n_att = len(pre.get("attack_db") or [])
    na = int(round(n_att * scale)) if scale != 1 else n_att
    nb = min(n // ATTACK_HOP, np_ + na)
    return min(np_, nb), na, nb


def _clamp_gains(y: np.ndarray, pre: dict, scale: float, anchor: float) -> np.ndarray | None:
    k, na, nb = _clamp_frames(len(y), pre, scale, anchor)
    if nb < 2:
        return None
    e = frame_db(mono(y), ATTACK_HOP, nb)
    db = np.asarray(pre.get("db") or [], dtype=np.float64)
    att = list(pre.get("attack_db") or [])
    g = np.concatenate([_pre_gains(e, db, k, scale, anchor), _attack_gains(e[k:nb], att, na)])
    if pre.get("v") == 2:
        return np.concatenate([_smooth_part(g[:k]), _smooth_part(g[k:])])
    return _smooth_part(g)


def _smooth_part(g: np.ndarray) -> np.ndarray:
    if len(g) == 0:
        return g
    neg = np.minimum(g, 0.0)
    p = np.pad(neg, 1, mode="edge")
    wide = np.minimum(np.minimum(p[:-2], p[1:-1]), p[2:])
    return smooth121(np.where(wide < 0, np.minimum(wide, g), g))


def _clamp_curve(g: np.ndarray) -> np.ndarray:
    """Per-sample linear gain from the 0.25 ms frame gains, back to 0 dB at the end."""
    span = len(g) * ATTACK_HOP
    gs = np.interp(np.arange(span), np.append((np.arange(len(g)) + 0.5) * ATTACK_HOP, span),
                   np.append(g, 0.0))
    return 10 ** (gs / 20)


def _hp_clamp(y: np.ndarray, lin: np.ndarray, hp: float) -> np.ndarray:
    """Apply the clamp only above hp Hz: add the high-passed gain change to the render."""
    span = len(lin)
    end = min(len(y), span + int(round(0.02 * SR)))
    full = np.ones(end)
    full[:span] = lin
    corr = _scale(y[:end], full - 1.0)
    if len(corr) > 30:
        sos = signal.butter(4, float(hp), "highpass", fs=SR, output="sos")
        corr = signal.sosfiltfilt(sos, corr, axis=0)
    out = y.copy()
    out[:end] = out[:end] + corr
    return out


def attack_clamp(y: np.ndarray, pre: dict, scale: float = 1, anchor: float = 0) -> np.ndarray:
    """Restore the recording's attack shape over the first frames of a render."""
    g = _clamp_gains(y, pre, scale, anchor)
    if g is None:
        return y
    lin = _clamp_curve(g)
    hp = pre.get("hp_hz")
    if hp:
        return _hp_clamp(y, lin, hp)
    out = y.copy()
    out[: len(lin)] = _scale(y[: len(lin)], lin)
    return out


# ---------------------------------------------------------------- envelope gain and gate


def env_gain(y: np.ndarray, env: dict, scale: float = 1, anchor: float = 0) -> np.ndarray:
    """The optional envelope calibration curve, time-scaled about the anchor."""
    db = np.asarray(env.get("db") or [], dtype=np.float64)
    if not len(db):
        return y
    hop = float(env.get("hop", 0.005))
    tk = anchor + ((np.arange(len(db)) + 0.5) * hop - anchor) * scale
    g = np.interp(np.arange(len(y)) / SR, tk, db)
    return _scale(y, 10 ** (g / 20))


def gate_curve(pts: Points, n: int) -> np.ndarray:
    """Per-sample linear gate gain, Hann smoothed over 1 ms."""
    p = np.asarray(pts, dtype=np.float64).reshape(-1, 2)
    t = np.arange(n) / SR
    db = np.interp(t, p[:, 0], p[:, 1], left=0.0, right=p[-1, 1])
    lin = 10 ** (db / 20)
    w = round(GATE_SMOOTH_S * SR) | 1
    h = np.hanning(w + 2)[1:-1]
    h /= h.sum()
    pad = np.pad(lin, w // 2, mode="edge")
    return np.convolve(pad, h, mode="valid")[:n]


def apply_gate(y: np.ndarray, gate: dict, scale: float = 1,
               tmap: Callable[[Points], Points] | None = None) -> np.ndarray:
    pts = [list(p) for p in gate["pts"]]
    if tmap is not None:
        pts = tmap(pts)
    elif scale != 1:
        pts = [[t * scale, d] for t, d in pts]
    if not pts:
        return y
    return _scale(y, gate_curve(pts, len(y)))


def apply_post(y: np.ndarray, post: dict | None, scale: float = 1, tail: float = 1,
               tmap: Callable[[Points], Points] | None = None, anchor: float = 0) -> np.ndarray:
    """A render through a post stage: EQ, gain, attack clamp, envelope gain, gate."""
    y = np.asarray(y, dtype=np.float64)
    if not post:
        return y
    gains = post.get("eq_db") or []
    if any(float(v) != 0 for v in gains):
        y = eq(y, gains)
    y = y * 10 ** (float(post.get("gain_db", 0.0)) / 20)
    if post.get("pre"):
        y = attack_clamp(y, post["pre"], scale, anchor)
    if post.get("env"):
        y = env_gain(y, post["env"], scale * tail, anchor)
    if post.get("gate"):
        y = apply_gate(y, post["gate"], scale, tmap)
    return y


# ---------------------------------------------------------------- bed layer


def _noise_c() -> float:
    w = signal.get_window("hann", 1024)
    return float(np.sum(w ** 2) / np.sum(w) ** 2 * math.log(2))


def _is_line(db: np.ndarray, med: np.ndarray, k: int) -> bool:
    return db[k] >= med[k] + 10 and db[k] >= db[k - 1] and db[k] >= db[k + 1]


def _line(db: np.ndarray, power: np.ndarray, k: int) -> tuple:
    """(frequency Hz, amplitude) of a spectral line at bin k, parabolic refined."""
    a, b, c = db[k - 1], db[k], db[k + 1]
    curv = a - 2 * b + c
    off = 0.5 * (a - c) / curv if curv < 0 else 0.0
    amp = math.sqrt(4 * float(power[k - 1: k + 2].sum()) / 1.5)
    return (k + off) * SR / 1024, amp


def _bed_lines(power: np.ndarray) -> tuple[list, np.ndarray]:
    db = 10 * np.log10(np.maximum(power, 1e-30))
    med = signal.medfilt(db, 15) if len(db) >= 15 else np.full_like(db, np.median(db))
    ks = [k for k in range(1, len(db) - 1) if _is_line(db, med, k)]
    out = power.copy()
    for k in ks:
        out[k - 1: k + 2] = 10 ** (med[k - 1: k + 2] / 10)
    return [_line(db, power, k) for k in ks], out


def _shaped_noise(r: np.random.Generator, power: np.ndarray, nb: int) -> np.ndarray:
    white = r.standard_normal(nb + 1024)
    _, _, z = signal.stft(white, nperseg=1024, noverlap=768, window="hann")
    z = z * np.sqrt(power / _noise_c())[:, None]
    _, x = signal.istft(z, nperseg=1024, noverlap=768, window="hann")
    return np.pad(x, (0, max(0, nb - len(x))))[:nb]


def _line_tones(lines: list, phases: np.ndarray, nb: int) -> np.ndarray:
    t = np.arange(nb) / SR
    x = np.zeros(nb)
    for (f, a), ph in zip(lines, phases):
        x = x + a * np.cos(2 * np.pi * f * t + ph)
    return x


def _bed_fades(x: np.ndarray) -> np.ndarray:
    """A 2 ms linear fade-in and a 10 ms linear fade-out."""
    nb = len(x)
    fi, fo = min(nb, int(0.002 * SR)), min(nb, int(0.010 * SR))
    x[:fi] *= np.arange(fi) / max(fi, 1)
    x[nb - fo:] *= np.arange(fo, 0, -1) / max(fo, 1)
    return x


def bed_noise(bed: dict, n: int, seed: int = 0) -> np.ndarray:
    """The bed layer: steady lines plus shaped noise, length n (zero after the bed's length)."""
    nb = min(n, int(bed["n"]))
    y = np.zeros(n)
    if nb <= 0:
        return y
    lines, power = _bed_lines(10 ** (np.asarray(bed["psd_db"], dtype=np.float64) / 10))
    r = np.random.default_rng([seed & 0xFFFFFFFF, 77])
    phases = r.uniform(0, 2 * np.pi, len(lines))
    x = _shaped_noise(r, power, nb)
    y[:nb] = _bed_fades(x + _line_tones(lines, phases, nb))
    return y


def add_bed(y: np.ndarray, bed: dict, seed: int) -> np.ndarray:
    b = bed_noise(bed, len(y), seed)
    return y + (b if y.ndim == 1 else b[:, None])


# ---------------------------------------------------------------- take and finish


def take_highpass(y: np.ndarray) -> np.ndarray:
    """Causal 2nd-order Butterworth high-pass at 8 Hz per channel."""
    sos = signal.butter(2, TAKE_DC_HZ, "highpass", fs=SR, output="sos")
    return signal.sosfilt(sos, y, axis=0)


def _onset40(m: np.ndarray) -> int:
    k = len(m) // 48
    if k < 2:
        return 0
    e = frame_db(m, 48, k)
    return int(np.argmax(e > e.max() - 40)) * 48


def finish(y: np.ndarray) -> np.ndarray:
    """Fade-in from zero (0.25 .. 2 ms) and a gain down to -1 dBFS when the peak is over it."""
    y = np.array(y, dtype=np.float64)
    if not len(y):
        return y
    fade = min(max(_onset40(mono(y)) / SR - 0.0005, FADE_MIN_S), FADE_IN_S)
    k = int(round(fade * SR))
    if k > 1:
        ramp = np.sin(np.pi * np.arange(k) / (2 * k)) ** 2
        y[:k] = _scale(y[:k], ramp)
    peak = float(np.max(np.abs(y)))
    lim = 10 ** (PEAK_DB / 20)
    if peak > lim:
        y = y * (lim / peak)
    return y
