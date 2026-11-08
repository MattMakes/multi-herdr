"""Recipe impact: a cinematic boom and a designed sub impact.

Spec: docs/specs/hark/recipes-impacts-impact.md. A parameter set with the key
`pre_len` renders with version 2 (the physical trailer boom). Every other set
renders with version 3. `swell` is the stand-alone air riser.
"""

from __future__ import annotations

import types
from collections.abc import Callable
from typing import Any

import numpy as np
from scipy import signal

from hark.forge.engine import dsp

SR = 48000
LO = 6000
UP = SR // LO
PEAK = 0.89
TAIL = 5.0
P_REF = 1.25
MEMBRANE = (1.0, 1.594, 2.136, 2.296, 2.653, 2.918)

TARGET = "a deep cinematic boom impact"
TARGETS = {
    "impact": "a deep cinematic boom impact",
    "pro_impact": "a deep designed sub impact with a granular rumble tail, gated",
}
ALTS = ["a kick drum", "an explosion", "a door slam", "thunder"]
REFSET = "impact"


def _ref_root() -> str:
    import os

    return os.environ.get("HARK_REFS", "refs").replace("\\", "/").rstrip("/")


REFS = [f"{_ref_root()}/impact/impact_{i:02d}.wav" for i in (1, 2, 3)]


# ------------------------------------------------------------ shared blocks

def _n(seconds: float) -> int:
    return max(1, int(round(seconds * SR)))


def _peak1(x: np.ndarray) -> np.ndarray:
    return x / (np.max(np.abs(x)) + 1e-12)


def _filt(x: np.ndarray, kind: str, f: Any, order: int = 2, rate: int = SR) -> np.ndarray:
    """I5: Butterworth at `rate`, cutoff clipped to [10, 0.45 rate]."""
    f = np.clip(np.asarray(f, dtype=float), 10.0, 0.45 * rate)
    btype = {"low": "lowpass", "high": "highpass", "band": "bandpass"}[kind]
    sos = signal.butter(order, f, btype=btype, fs=rate, output="sos")
    return signal.sosfilt(sos, x, axis=0)


_PINK_B = np.array([0.049922035, -0.095993537, 0.050612699, -0.004408786])
_PINK_A = np.array([1.0, -2.494956002, 2.017265875, -0.522189400])


def _noise(n: int, rng: np.random.Generator, colour: str = "white") -> np.ndarray:
    """I1: white, pink (economy IIR) or brown (leaky integrator), unit RMS."""
    w = rng.standard_normal(n)
    if colour == "pink":
        w = signal.lfilter(_PINK_B, _PINK_A, w)
    elif colour == "brown":
        w = signal.lfilter([1.0], [1.0, -0.997], w)
    return w / (np.sqrt(np.mean(w ** 2)) + 1e-12)


def _mic(y: np.ndarray, rng: np.random.Generator, floor_db: float, low_cut: float = 20.0,
         air_db: float = -1.0) -> np.ndarray:
    """I2: high-pass, air shelf at 10 kHz, independent pink floor per channel."""
    y = dsp.peq(_filt(y, "high", low_cut, 1), 10000.0, air_db, 0.6)
    pk = np.max(np.abs(y))
    g = pk * 10 ** (floor_db / 20)
    cols = [y[:, c] + _noise(len(y), rng, "pink") * g for c in range(y.shape[1])]
    return np.column_stack(cols)


def _glide(t: np.ndarray, f_start: float, f_end: float, tau: float, rng: np.random.Generator) -> np.ndarray:
    """I3: closed-form phase of an exponential pitch slide."""
    tau = max(tau, 1e-4)
    ph = f_end * t + (f_start - f_end) * tau * (1 - np.exp(-t / tau))
    return np.sin(2 * np.pi * ph + rng.uniform(-0.3, 0.3))


def _env(t: np.ndarray, attack: float, tau: float) -> np.ndarray:
    """I4: exponential decay with a raised-cosine rise over `attack`."""
    e = np.exp(-t / max(tau, 1e-4))
    rising = t < attack
    e[rising] *= 0.5 - 0.5 * np.cos(np.pi * t[rising] / attack)
    return e


def _rise(t: np.ndarray, a: float) -> np.ndarray:
    """J1: 1, with a raised-cosine rise before `a`."""
    e = np.ones_like(t)
    if a > 0:
        rising = t < a
        e[rising] = 0.5 - 0.5 * np.cos(np.pi * t[rising] / a)
    return e


def _up(x: np.ndarray, n: int) -> np.ndarray:
    """I6: polyphase up-sampling 6 kHz -> 48 kHz, cut or padded to n."""
    y = signal.resample_poly(x, UP, 1)
    return _fit(y, n)


def _fit(y: np.ndarray, n: int) -> np.ndarray:
    if len(y) >= n:
        return y[:n]
    return np.concatenate([y, np.zeros((n - len(y),) + y.shape[1:])])


def _add(dst: np.ndarray, x: np.ndarray, at: int) -> None:
    if at >= len(dst):
        return
    m = min(len(x), len(dst) - at)
    dst[at:at + m] += x[:m]


def _hall_tail(w: np.ndarray, t: np.ndarray, rt60: float, damp: float, size: float) -> np.ndarray:
    rts = (1.25 * rt60, rt60, rt60 * (1 - 0.5 * damp), rt60 * (1 - 0.8 * damp))
    rest, tail = w, np.zeros_like(w)
    for k, fc in enumerate((250.0, 1500.0, 6000.0)):
        band = _filt(rest, "low", fc, 2)
        rest = rest - band
        tail += band * np.exp(-6.91 * t / max(rts[k], 0.05))
    tail += rest * np.exp(-6.91 * t / max(rts[3], 0.05))
    return tail * (1 - np.exp(-t / (0.008 + 0.03 * size)))


def _reflections(n: int, size: float, rng: np.random.Generator) -> np.ndarray:
    refl = np.zeros(n)
    for _ in range(40):
        d = int(np.floor(rng.uniform(0.003, 0.02 + 0.08 * size) * SR))
        a = rng.uniform(0.05, 0.2) * rng.choice([-1.0, 1.0])
        if d < n:
            refl[d] += a * np.exp(-d / SR / 0.06)
    return refl


def hall_ir(rt60: float, damp: float, size: float = 0.8, predelay: float = 0.02, seconds: float | None = None,
            *, rng: np.random.Generator | None = None) -> np.ndarray:
    """I7: the computed hall impulse response (unit energy, mono).

    rng is the render generator. With rng None a fixed seed (0) is used, so the
    result is deterministic.
    """
    rng = np.random.default_rng(0) if rng is None else rng
    seconds = min(5.0, 1.1 * rt60 + 0.1) if seconds is None else seconds
    n = _n(seconds)
    t = np.arange(n) / SR
    tail = _hall_tail(rng.standard_normal(n), t, rt60, damp, size)
    refl = _reflections(n, size, rng)
    ir = 0.25 * tail + _filt(refl, "low", 7000.0, 1)
    ir = ir / (np.sqrt(np.sum(ir ** 2)) + 1e-12)
    if predelay > 0:
        ir = np.concatenate([np.zeros(_n(predelay)), ir])
    return ir


def _console(x: np.ndarray, drive: float, tone: float) -> np.ndarray:
    """I8: console saturation on the loud section, then a roll-off."""
    if drive <= 0.001:
        return x
    g = 1 + 7 * drive
    pk = np.max(np.abs(x)) + 1e-12
    y = pk * np.tanh(g * x / pk) / g * (1 + 0.15 * drive)
    return _filt(y, "low", tone, 2)


def _comb_block(x: np.ndarray, d: np.ndarray, fb: float) -> np.ndarray:
    y = np.array(x, dtype=float, copy=True)
    n = len(x)
    step = max(1, int(np.floor(d.min())) - 1)
    for s in range(0, n, step):
        i = np.arange(s, min(n, s + step))
        pos = i - d[i]
        ok = pos >= 1
        lo = np.floor(pos[ok]).astype(int)
        fr = pos[ok] - lo
        y[i[ok]] += fb * ((1 - fr) * y[lo] + fr * y[lo + 1])
    return y


def _flange(x: np.ndarray, p: dict) -> np.ndarray:
    """I9: feedback comb whose delay drifts upward, mixed in by comb_mix."""
    if p["comb_mix"] < 0.01:
        return x
    t = np.arange(len(x)) / SR
    d = p["comb_d0"] / 1000 * SR * (1 + (p["comb_ratio"] - 1) * (1 - np.exp(-t / 1.2)))
    y = _comb_block(x, d, p["comb_fb"]) * (1 - p["comb_fb"])
    return x + p["comb_mix"] * (y - x)


def _shell(tl: np.ndarray, lo: float, span: float, tau: float, rng: np.random.Generator) -> np.ndarray:
    """Shell cloud: 28 low-mid modes, the top ones ringing shorter."""
    hi = min(lo * span, 2520.0)
    f = np.sort(np.exp(rng.uniform(np.log(lo), np.log(hi), 28)))
    u = np.log(f / lo) / max(np.log(hi / lo), 1e-9)
    taus = tau * np.exp(-1.6 * u)
    amps = 10 ** (-4 * np.log2(f / lo) / 20) * rng.uniform(0.3, 1.0, 28)
    phases = rng.uniform(0, 2 * np.pi, 28)
    x = np.zeros_like(tl)
    for fk, ak, tk, pk in zip(f, amps, taus, phases):
        x += ak * np.sin(2 * np.pi * fk * tl + pk) * np.exp(-tl / tk)
    return _peak1(x * (1 - np.exp(-tl / 0.0015)))


def _lf_wobble(m: int, rate: float, rng: np.random.Generator) -> np.ndarray:
    k = max(2, int(np.floor(LO / max(rate, 0.05))))
    lf = _filt(_noise(m + k, rng), "low", rate, 2, LO)[k:]
    return lf / (np.sqrt(np.mean(lf ** 2)) + 1e-12)


def _roll(n: int, p: dict, rng: np.random.Generator, v3: bool) -> np.ndarray:
    """v2 roll; with v3 the order-4 variant (J10 without the grains)."""
    m = int(np.floor(n * LO / SR)) + 8
    t = np.arange(m) / LO
    order, pink_k, hp, build = (4, 1.4, 26.0, 0.08) if v3 else (2, 1.8, 24.0, 0.15)
    w = _filt(_noise(m, rng, "brown"), "low", p["roll_cut"], order, LO)
    w = w + 0.5 * _filt(_noise(m, rng, "pink"), "low", pink_k * p["roll_cut"], order, LO)
    w = _filt(w, "high", hp, 2, LO)
    lf = _lf_wobble(m, p["roll_rate"], rng)
    hold, fall = p["roll_hold"], p["roll_fall"]
    e = (1 - np.exp(-t / build)) * np.exp(-t / (2.5 * hold))
    e = np.where(t > hold, e * np.exp(-(t - hold) / fall), e)
    depth = p["roll_depth"]
    y = w * e * np.exp(depth * lf - 0.5 * depth ** 2)
    return _peak1(_up(y, n))


# ------------------------------------------------------------ version 2

V2_SPACE: dict[str, Any] = {
    "pre_len": (0.08, 1.6, "log"), "pre_level": (0.02, 1.0, "log"), "pre_bright": (200.0, 16000.0, "log"),
    "pre_low": (0.0, 1.0), "riser_level": (0.0, 0.6), "riser_f": (800.0, 12000.0, "log"),
    "crack_level": (0.01, 0.8, "log"), "crack_ms": (0.2, 3.0, "log"), "crack_f": (600.0, 9000.0, "log"),
    "contact_ms": (0.15, 3.0, "log"), "crackle": (0.0, 1.5), "debris_ms": (60.0, 500.0, "log"),
    "thump_f0": (70.0, 400.0, "log"), "thump_f1": (22.0, 70.0, "log"), "drop_tau": (0.006, 0.3, "log"),
    "thump_tau": (0.12, 2.0, "log"), "modes_level": (0.0, 0.6), "shell_level": (0.0, 0.8),
    "shell_lo": (70.0, 250.0, "log"), "shell_span": (2.0, 10.0, "log"), "shell_tau": (0.05, 1.2, "log"),
    "sub_f": (24.0, 62.0, "log"), "sub_tau": (0.15, 3.0, "log"), "sub_level": (0.0, 1.0),
    "rumble_level": (0.02, 1.2, "log"), "rumble_cut": (80.0, 1500.0, "log"), "rumble_tau": (0.3, 3.5, "log"),
    "sweep_level": (0.005, 0.5, "log"), "sweep_hi": (250.0, 8000.0, "log"), "sweep_tau": (0.08, 1.2, "log"),
    "echo_level": (0.0, 0.6), "echo_delay": (0.08, 0.6, "log"), "comb_mix": (0.0, 0.8),
    "comb_d0": (0.15, 2.0, "log"), "comb_ratio": (1.2, 6.0, "log"), "comb_fb": (0.2, 0.85),
    "hall_wet": (0.05, 1.2, "log"), "hall_rt": (0.5, 8.0, "log"), "hall_damp": (0.0, 1.0),
    "predelay": (0.0, 0.06), "drive": (0.0, 1.0), "sat_tone": (1500.0, 14000.0, "log"), "width": (0.0, 1.0),
    "floor_db": (-95.0, -66.0), "roll_level": (0.0, 1.5), "roll_cut": (60.0, 400.0, "log"),
    "roll_hold": (0.8, 3.2), "roll_fall": (0.25, 1.5, "log"), "roll_rate": (0.3, 3.0, "log"),
    "roll_depth": (0.0, 0.8),
}
V2_DEFAULTS: dict[str, float] = dict(zip(V2_SPACE, [
    0.7875, 0.133, 600, 0.5775, 0.0, 3063, 0.72, 1.907, 1133, 0.6308, 1.035, 66.93, 141.9, 30.16, 0.03454,
    0.2087, 0.2633, 0.2035, 121.7, 3.57, 0.3507, 60.13, 0.8391, 0.0, 0.1937, 103.9, 1.658, 0.1123, 365.9,
    0.5692, 0.485, 0.1192, 0.1677, 0.2453, 3.23, 0.7521, 0.8017, 3.897, 0.2887, 0.0, 0.3529, 8003, 0.3363,
    -88.67, 0.4072, 141.7, 2.26, 0.5904, 0.6466, 0.5458,
]))
V2_NEUTRAL = {"comb_mix": 0.0, "riser_level": 0.0, "roll_level": 0.0}


def _v2_body(tl: np.ndarray, p: dict, rng: np.random.Generator) -> np.ndarray:
    x = _glide(tl, p["thump_f0"], p["thump_f1"], p["drop_tau"], rng) * _env(tl, 0.0015, p["thump_tau"])
    if p["modes_level"] <= 0.005:
        return x
    for j, ratio in enumerate(MEMBRANE[1:]):
        if p["thump_f0"] * ratio > 2700:
            continue
        a = p["modes_level"] * 0.75 ** j * rng.uniform(0.6, 1.0)
        g = _glide(tl, p["thump_f0"] * ratio, p["thump_f1"] * ratio, p["drop_tau"], rng)
        x = x + a * g * _env(tl, 0.001, p["thump_tau"] / (1.5 + 0.8 * j))
    return x


def _v2_rumble(tl: np.ndarray, p: dict, rng: np.random.Generator) -> np.ndarray:
    w = _noise(len(tl), rng, "brown")
    c = min(p["rumble_cut"], 2520.0)
    a, b = _filt(w, "low", c, 2, LO), _filt(w, "low", 0.3 * c, 2, LO)
    k = np.exp(-tl / max(0.5 * p["rumble_tau"], 0.05))
    y = _filt(k * a + 1.4 * (1 - k) * b, "high", 22.0, 2, LO)
    mod = np.ones_like(tl)
    for _ in range(3):
        mod += 0.12 * np.sin(2 * np.pi * rng.uniform(1.5, 6.0) * tl + rng.uniform(0, 2 * np.pi))
    return _peak1(y * mod * _env(tl, 0.012, p["rumble_tau"]))


def _v2_debris(m: int, p: dict, rng: np.random.Generator) -> np.ndarray:
    td = p["debris_ms"] / 1000
    count = int(np.floor(60 + 3000 * td))
    times = rng.exponential(0.5 * td, count)
    g = rng.standard_normal(count)
    keep = times < 0.58
    d = np.zeros(m)
    np.add.at(d, np.minimum((times[keep] * SR).astype(int), m - 1), g[keep] * np.exp(-times[keep] / td))
    return _peak1(_filt(d, "band", (0.45 * p["crack_f"], min(4 * p["crack_f"], 21600.0)), 2))


def _v2_crack(n: int, p: dict, rng: np.random.Generator) -> np.ndarray:
    m = min(n, _n(0.6))
    t = np.arange(m) / SR
    k = min(m, max(2, _n(p["contact_ms"] / 1000)))
    y = np.zeros(m)
    y[:k] += 0.5 - 0.5 * np.cos(np.linspace(0, 2 * np.pi, k))
    y += 0.8 * np.sin(2 * np.pi * p["crack_f"] * t) * np.exp(-t / (p["crack_ms"] / 1000)) * (1 - np.exp(-t / 0.0002))
    y = _filt(y, "high", 30.0, 1)
    if p["crackle"] > 0.01:
        y = y + p["crackle"] * _v2_debris(m, p, rng)
    return _fit(_peak1(y), n)


def _v2_sweep(n: int, p: dict, rng: np.random.Generator) -> np.ndarray:
    t = np.arange(n) / SR
    fc = 70 + (p["sweep_hi"] - 70) * np.exp(-t / p["sweep_tau"])
    y = dsp.svf(_noise(n, rng, "pink"), fc, 2.5, "band") * _env(t, 0.004, 1.8 * p["sweep_tau"])
    return _peak1(y)


def _air_riser(n: int, f_end: float, rng: np.random.Generator) -> np.ndarray:
    u = np.arange(n) / max(n - 1, 1)
    fc = 150 * (f_end / 150) ** (u ** 1.5)
    y = dsp.svf(_noise(n, rng, "pink"), fc, 1.2, "band") * (np.exp(3.5 * u) - 1) / (np.exp(3.5) - 1)
    return _peak1(y)


def _echoes(x: np.ndarray, p: dict, rng: np.random.Generator) -> np.ndarray:
    out = np.zeros_like(x)
    if p["echo_level"] < 0.01:
        return out
    d = 0.0
    for k in range(3):
        d += p["echo_delay"] * rng.uniform(0.8, 1.3)
        i = _n(d)
        if i >= len(x):
            break
        out[i:] += p["echo_level"] * 0.6 ** k * _filt(x, "low", 4000 * 0.5 ** k, 2)[: len(x) - i]
    return out


def _v2_channel(c: int, s: dict, p: dict, rng: np.random.Generator) -> tuple[np.ndarray, np.ndarray]:
    w = p["width"]
    rum, swp = s["rum"], s["swp"]
    if w > 0.01:
        rum = (1 - w) * rum + w * _up(_v2_rumble(s["tl"], p, rng), s["K"])
        swp = (1 - w) * swp + w * _v2_sweep(s["K"], p, rng)
    dry = s["direct"] + p["rumble_level"] * rum + p["sweep_level"] * swp
    dry = dsp.fade(dry + _echoes(dry, p, rng), 0.0, 0.4)
    wet = _flange(signal.fftconvolve(dry, s["irs"][c]), p)
    return dry, wet


def _v2_swell(out: np.ndarray, s: dict, p: dict, rng: np.random.Generator) -> None:
    pre, kp = p["pre_len"], s["kp"]
    m = _n(0.2)
    head = s["crack"][:m] / (np.max(np.abs(s["crack"])) + 1e-12) + p["pre_low"] * _peak1(s["low"])[:m]
    pk = np.max(np.abs(out))
    for c in range(2):
        ir = hall_ir(max(0.3, 1.6 * pre), 0.4, 0.6, 0.0, seconds=1.2 * pre + 0.25, rng=rng)
        sw = _filt(_fit(signal.fftconvolve(head, ir), kp)[::-1], "low", p["pre_bright"], 2)
        sw = dsp.fade(sw, min(0.25 * pre, 0.1), 0.003)
        out[:kp, c] += sw * p["pre_level"] * pk / (np.max(np.abs(sw)) + 1e-12)
        if p["riser_level"] > 0.005 and kp > 64:
            out[:kp, c] += p["riser_level"] * pk * dsp.fade(_air_riser(kp, p["riser_f"], rng), 0.01, 0.002)


def _v2_roll(out: np.ndarray, kp: int, p: dict, rng: np.random.Generator) -> None:
    if p["roll_level"] <= 0.005:
        return
    pk = np.max(np.abs(out))
    k_roll = min(len(out) - kp, _n(p["roll_hold"] + 6 * p["roll_fall"] + 0.5))
    rc = _roll(k_roll, p, rng, v3=False)
    w = p["width"]
    for c in range(2):
        rl = (1 - w) * rc + w * _roll(k_roll, p, rng, v3=False) if w > 0.01 else rc
        _add(out[:, c], p["roll_level"] * pk * dsp.fade(rl, 0.0, 0.3), kp)


def _v2_sources(p: dict, rng: np.random.Generator) -> dict:
    K = _n(TAIL)
    tl = np.arange(int(LO * TAIL)) / LO
    low = _v2_body(tl, p, rng)
    low = low + p["sub_level"] * np.sin(2 * np.pi * p["sub_f"] * tl + rng.uniform(0, 1)) * _env(tl, 0.02, p["sub_tau"])
    low = low + p["shell_level"] * _shell(tl, p["shell_lo"], p["shell_span"], p["shell_tau"], rng)
    low = _console(_up(low, K), p["drive"], p["sat_tone"])
    crack = _v2_crack(K, p, rng)
    s = {"K": K, "tl": tl, "low": low, "crack": crack, "direct": low + p["crack_level"] * crack}
    s["rum"] = _up(_v2_rumble(tl, p, rng), K)
    s["swp"] = _v2_sweep(K, p, rng)
    ir1 = hall_ir(p["hall_rt"], p["hall_damp"], 0.8, p["predelay"], rng=rng)
    ir2 = hall_ir(p["hall_rt"], p["hall_damp"], 0.8, p["predelay"] + rng.uniform(0.001, 0.006), rng=rng)
    s["irs"] = (ir1, ir2)
    return s


def render_v2(p: dict, rng: np.random.Generator) -> np.ndarray:
    """Version 2: the physical trailer boom. The contact lands at pre_len."""
    p = {**V2_DEFAULTS, **p}
    s = _v2_sources(p, rng)
    kp = _n(p["pre_len"])
    s["kp"] = kp
    out = np.zeros((kp + s["K"] + max(len(ir) for ir in s["irs"]), 2))
    for c in range(2):
        dry, wet = _v2_channel(c, s, p, rng)
        _add(out[:, c], dry, kp)
        _add(out[:, c], p["hall_wet"] * wet, kp)
    _v2_swell(out, s, p, rng)
    _v2_roll(out, kp, p, rng)
    y = _mic(out, rng, p["floor_db"])
    y = dsp.trim_tail(y, -66.0, 0.05)
    return dsp.fade(y, 0.002, 0.25)


# ------------------------------------------------------------ swell (stand-alone riser)

def _swell_tube(w: np.ndarray, u: np.ndarray, lo: float, hi: float, curve: float) -> np.ndarray:
    ft = lo * (hi / lo) ** (u ** curve)
    pipe = np.zeros_like(w)
    for k in range(1, 7):
        pipe += dsp.svf(w, np.minimum(k * ft, 19200.0), 9.0, "bandn") / k
    return pipe / (np.sqrt(np.mean(pipe ** 2)) + 1e-12)


def _swell_layer(n: int, u: np.ndarray, rng: np.random.Generator, k: dict) -> np.ndarray:
    w = _noise(n, rng, "pink")
    fc = k["f_start"] * (k["f_end"] / k["f_start"]) ** (u ** k["curve"])
    y = dsp.svf(w, fc, k["q"], "band")
    y = y / (np.sqrt(np.mean(y ** 2)) + 1e-12)
    if k["tube"] > 0:
        y = y + k["tube"] * _swell_tube(w, u, k["tube_lo"], k["tube_hi"], k["curve"])
    if k["flutter"] > 0:
        y = y * np.exp(k["flutter"] * _lf_flutter(n, rng) - 0.5 * k["flutter"] ** 2)
    return y * (np.exp(k["grow"] * u) - 1) / (np.exp(k["grow"]) - 1)


def _lf_flutter(n: int, rng: np.random.Generator) -> np.ndarray:
    k = SR // 12
    lf = _filt(rng.standard_normal(n + k), "low", 12.0, 2)[k:]
    return lf / (np.sqrt(np.mean(lf ** 2)) + 1e-12)


def _swell_wash(n: int, u: np.ndarray, seconds: float, f_end: float, rng: np.random.Generator) -> np.ndarray:
    strike = np.zeros(_n(0.02))
    k = _n(0.004)
    strike[:k] = _noise(k, rng)
    ir = hall_ir(1.4 * seconds, 0.45, 0.7, 0.0, seconds=seconds + 0.1, rng=rng)
    tail = _fit(signal.fftconvolve(strike, ir), n)[::-1]
    return _peak1(_filt(tail, "low", 0.8 * f_end, 2)) * (0.3 + 0.7 * u)


def swell(seconds: float, rng: np.random.Generator, f_start: float = 150, f_end: float = 6000, q: float = 1.2,
          curve: float = 1.5, grow: float = 3.5, width: float = 0.6, room_wet: float = 0.25, tube: float = 0.5,
          tube_lo: float = 90, tube_hi: float = 600, flutter: float = 0.25, rev: float = 0.5) -> np.ndarray:
    """A stand-alone air riser whose last sample is its loudest. Stereo, peak 1."""
    n = _n(seconds)
    u = np.arange(n) / max(n - 1, 1)
    k = dict(f_start=f_start, f_end=f_end, q=q, curve=curve, grow=grow, tube=tube, tube_lo=tube_lo,
             tube_hi=tube_hi, flutter=flutter)
    shared = _swell_layer(n, u, rng, k)
    chans = []
    for _c in range(2):
        y = _peak1((1 - width) * shared + width * _swell_layer(n, u, rng, k))
        if rev > 0:
            y = y + rev * _swell_wash(n, u, seconds, f_end, rng)
        chans.append(y)
    y = np.column_stack(chans)
    ir = hall_ir(0.9, 0.5, 0.6, 0.004, rng=rng)
    wet = np.column_stack([signal.fftconvolve(y[:, c], ir)[:n] for c in range(2)])
    y = y + room_wet * wet / np.sqrt(np.sum(ir ** 2))
    y = dsp.fade(_mic(y, rng, -80.0), 0.03, 0.006)
    return _peak1(y)


# ------------------------------------------------------------ version 3

_V3 = [
    ("pre", (0.0, 1.2), 0.35), ("pre_level", (0.02, 0.7, "log"), 0.12), ("crack", (0.03, 2.0, "log"), 0.35),
    ("crack_f", (900.0, 7000.0, "log"), 2200), ("crack_ms", (2.0, 18.0, "log"), 6.0), ("snap", (0.0, 1.5), 0.0),
    ("sheen", (0.0, 2.0), 0.0), ("sheen_ms", (4.0, 16.0, "log"), 9.0), ("sheen_f", (4500.0, 9000.0, "log"), 6000),
    ("pre_shape", [0, 1], 0), ("debris", (0.0, 1.0), 0.4), ("blast", (0.0, 1.0), 0.5),
    ("blast_f", (250.0, 3000.0, "log"), 900), ("blast_tau", (0.03, 0.8, "log"), 0.25),
    ("body_f0", (70.0, 230.0, "log"), 140), ("body_f1", (34.0, 75.0, "log"), 46),
    ("drop_tau", (0.015, 0.14, "log"), 0.05), ("body_tau", (0.08, 0.7, "log"), 0.32),
    ("body_attack", (0.001, 0.3, "log"), 0.004), ("modes", (0.0, 0.6), 0.25), ("body", (0.1, 1.2, "log"), 1.0),
    ("drive", (0.0, 1.0), 0.35), ("sub", (0.0, 1.2), 0.7), ("sub_f", (28.0, 60.0, "log"), 40),
    ("sub_semis", (0.0, 24.0), 12.0), ("sub_tau", (0.2, 2.2, "log"), 1.1), ("shell", (0.0, 0.8), 0.3),
    ("shell_lo", (90.0, 260.0, "log"), 130), ("roll", (0.0, 1.5), 0.6), ("roll_cut", (55.0, 320.0, "log"), 140),
    ("roll_hold", (0.2, 3.0), 1.6), ("roll_fall", (0.12, 1.6, "log"), 0.6), ("roll_rate", (0.3, 3.0, "log"), 0.7),
    ("roll_depth", (0.0, 0.8), 0.45), ("grain_rate", (18.0, 48.0, "log"), 30.0), ("grain_depth", (0.0, 1.0), 0.0),
    ("air", (0.0, 1.0), 0.0), ("air_f", (2500.0, 11000.0, "log"), 7000), ("air_div", (0.25, 1.0), 1.0),
    ("grain_sharp", (2.0, 6.0), 3.0), ("air_q", (0.5, 3.0), 0.8), ("hall_wet", (0.03, 1.0, "log"), 0.45),
    ("hall_rt", (0.4, 6.0, "log"), 3.2), ("hall_damp", (0.0, 1.0), 0.4), ("hall_tone", (3000.0, 12000.0, "log"), 5000),
    ("width", (0.0, 1.0), 0.55), ("gate", (0.0, 1.6), 0.0), ("gate_ms", (6.0, 40.0), 20.0),
    ("gate_fall", (0.08, 0.6, "log"), 0.25), ("gate_db", (0.0, 30.0), 0.0), ("floor_db", (-110.0, -70.0), -90.0),
    ("sheen_top", (7000.0, 20000.0, "log"), 20000), ("top", (6000.0, 20000.0, "log"), 20000),
    ("plate", (0.0, 1.0), 0.0), ("plate_f", (150.0, 900.0, "log"), 400), ("plate_tau", (0.2, 2.0, "log"), 0.8),
    ("scatter", (0.0, 1.0), 0.0), ("scatter_s", (0.3, 2.5), 1.2), ("grain_jit", (0.0, 1.0), 0.0),
    ("boom", (0.0, 2.5), 0.0), ("boom_f0", (80.0, 260.0, "log"), 150), ("boom_f1", (34.0, 70.0, "log"), 48),
    ("boom_drop", (0.015, 0.15, "log"), 0.045), ("boom_tau", (0.08, 0.9, "log"), 0.3),
    ("boom_att", (0.002, 0.12, "log"), 0.01), ("boom_drive", (0.0, 1.0), 0.3), ("roll_in", (0.0, 0.4), 0.0),
]
SPACE: dict[str, Any] = {k: r for k, r, _ in _V3}
DEFAULTS: dict[str, Any] = {k: d for k, _, d in _V3}


def _j2_debris(m: int, p: dict, rng: np.random.Generator) -> np.ndarray:
    td = 0.04 + 0.2 * p["debris"]
    count = int(np.floor(80 + 2500 * td))
    times = rng.exponential(0.45 * td, count)
    g = rng.standard_normal(count)
    keep = times < 0.38
    d = np.zeros(m)
    np.add.at(d, np.minimum((times[keep] * SR).astype(int), m - 1), g[keep] * np.exp(-times[keep] / td))
    return _peak1(_filt(d, "band", (0.35 * p["crack_f"], min(4 * p["crack_f"], 21600.0)), 2))


def _j2_crack(n: int, p: dict, rng: np.random.Generator) -> np.ndarray:
    m = min(n, _n(0.4))
    y = np.zeros(m)
    k = min(m, max(2, _n(0.0006)))
    y[:k] += 0.5 - 0.5 * np.cos(np.linspace(0, 2 * np.pi, k))
    L = min(m, max(8, _n(p["crack_ms"] / 1000)))
    i = np.arange(L)
    w = rng.standard_normal(L) * (1 - i / L) ** 3 * (1 - np.exp(-i / (SR * 0.0003)))
    w = _filt(w, "band", (0.5 * p["crack_f"], min(3 * p["crack_f"], 21600.0)), 2)
    y[:L] += 1.4 * _peak1(w)
    if p["debris"] > 0.01:
        y = y + 0.5 * p["debris"] * _j2_debris(m, p, rng)
    return _fit(_peak1(_filt(y, "high", 40.0, 1)), n)


def _j3_snap(n: int, rng: np.random.Generator) -> np.ndarray:
    L = min(n, _n(0.0015))
    u = np.arange(L) / L
    return _fit(_peak1(_filt(rng.standard_normal(L) * (1 - u) ** 4, "high", 5000.0, 2)), n)


def _j4_sheen(n: int, p: dict, rng: np.random.Generator) -> np.ndarray:
    L = min(n, max(16, _n(p["sheen_ms"] / 1000)))
    i = np.arange(L)
    e = (1 - i / L) ** 2 * (1 - np.exp(-i / (SR * 0.0002)))
    c = rng.standard_normal(L)
    chans = []
    for _c in range(2):
        y = _filt((0.7 * c + 0.3 * rng.standard_normal(L)) * e, "high", p["sheen_f"], 4)
        if p["sheen_top"] < 19000:
            y = _filt(y, "low", p["sheen_top"], 2)
        chans.append(y)
    return _fit(_peak1(np.column_stack(chans)), n)


def _j5_plate(n: int, p: dict, rng: np.random.Generator) -> np.ndarray:
    pf = p["plate_f"]
    m = min(n, _n(min(4.0, 5 * p["plate_tau"] + 0.1)))
    f = np.sort(pf * np.exp(rng.uniform(0, np.log(min(16.0, 9000.0 / pf)), 40)))
    a = (f / pf) ** -0.6 * rng.uniform(0.3, 1.0, 40)
    tau = p["plate_tau"] * (f / pf) ** -0.55
    ph = rng.uniform(0, 2 * np.pi, (40, 2))
    t = np.arange(m) / SR
    tg = t + 0.0012 * (1 - np.exp(-t / 0.1))
    on = 1 - np.exp(-t / 0.0008)
    y = np.zeros((m, 2))
    for k in range(40):
        amp = a[k] * np.exp(-t / tau[k]) * on
        y[:, 0] += amp * np.sin(2 * np.pi * f[k] * tg + ph[k, 0])
        y[:, 1] += amp * np.sin(2 * np.pi * f[k] * tg + ph[k, 1])
    return _fit(_peak1(y), n)


def _j6_channel(L: int, T: float, rng: np.random.Generator) -> np.ndarray:
    count = int(np.floor(60 + 260 * T))
    times = 0.03 + rng.exponential(0.4 * T, count)
    g = rng.standard_normal(count)
    lv = rng.uniform(0.2, 1.0, count) ** 2
    keep = times < T
    d = np.zeros(L)
    np.add.at(d, np.minimum((times[keep] * SR).astype(int), L - 1), (g * np.exp(-times / (0.5 * T)) * lv)[keep])
    k = _n(0.004)
    grain = rng.standard_normal(k) * np.exp(-np.arange(k) / (SR * 0.0012))
    y = signal.fftconvolve(d, grain)[:L]
    return _peak1(_filt(y, "band", (700.0, 6000.0), 2)) + 0.6 * _peak1(_filt(y, "band", (250.0, 1400.0), 2))


def _j6_scatter(n: int, p: dict, rng: np.random.Generator) -> np.ndarray:
    T = p["scatter_s"]
    L = min(n, _n(T + 0.25))
    return _fit(_peak1(np.column_stack([_j6_channel(L, T, rng) for _ in range(2)])), n)


def _j7_body(tl: np.ndarray, p: dict, rng: np.random.Generator) -> np.ndarray:
    a = p["body_attack"]
    late = np.maximum(tl - a, 0)
    x = _glide(tl, p["body_f0"], p["body_f1"], p["drop_tau"], rng) * np.exp(-late / p["body_tau"])
    if p["modes"] > 0.005:
        for j, ratio in enumerate(MEMBRANE[1:]):
            amp = p["modes"] * 0.75 ** j * rng.uniform(0.6, 1.0)
            g = _glide(tl, p["body_f0"] * ratio, p["body_f1"] * ratio, p["drop_tau"], rng)
            x = x + amp * g * np.exp(-late / (p["body_tau"] / (1.5 + 0.8 * j)))
    x = x * _rise(tl, a)
    if p["drive"] > 0.01:
        g = 1 + 6 * p["drive"]
        pk = np.max(np.abs(x)) + 1e-12
        x = pk * np.tanh(g * x / pk) / np.tanh(g)
        x = _filt(x, "low", min(2700.0, 900 + 2500 * (1 - p["drive"])), 2, LO)
    return _peak1(x)


def _j8_sub(tl: np.ndarray, p: dict, rng: np.random.Generator) -> np.ndarray:
    f1 = p["sub_f"]
    a = max(p["body_attack"], 0.008)
    return _glide(tl, f1 * 2 ** (p["sub_semis"] / 12), f1, 0.09, rng) * _rise(tl, a) * \
        np.exp(-np.maximum(tl - a, 0) / p["sub_tau"])


def _grain_once(g: np.ndarray, tk: float, sp: float, jit: float, rng: np.random.Generator) -> None:
    n = len(g)
    i0 = max(0, int(np.floor((tk + rng.uniform(-0.12, 0.12) * sp) * SR)))
    L = min(n - i0, int(np.floor(1.6 * sp * SR)))
    if L > 8:
        u = np.arange(L) / SR
        g[i0:i0 + L] += rng.uniform(0.55 - 0.4 * jit, 1.0) * (1 - np.exp(-u / 0.0025)) * np.exp(-u / (0.32 * sp))


def _grains(n: int, rate: float, depth: float, rng: np.random.Generator, slow: float = 0.18,
           jit: float = 0.0) -> np.ndarray:
    """J9: a granular gain curve around 1 over n samples."""
    if depth <= 0.01:
        return np.ones(n)
    g = np.zeros(n)
    tk, last = 0.0, (n - 1) / SR
    while tk < last:
        sp = (1 + slow * min(tk / 1.2, 1.5)) / rate
        _grain_once(g, tk, sp, jit, rng)
        tk += sp * (1 + jit * rng.uniform(-0.6, 0.9)) if jit > 0 else sp
    g = g / (np.percentile(g, 95) + 1e-9)
    return (1 - depth) + depth * np.clip(g, 0, 1.6)


def _j10_roll(n: int, p: dict, rng: np.random.Generator) -> np.ndarray:
    y = _roll(n, p, rng, v3=True)
    return _peak1(y * _grains(n, p["grain_rate"], p["grain_depth"], rng, jit=p["grain_jit"]))


def _j11_air(n: int, a: float, p: dict, rng: np.random.Generator) -> np.ndarray:
    t = np.arange(n) / SR
    y = dsp.svf(_noise(n, rng, "pink"), p["air_f"], p["air_q"], "bandn")
    e = _rise(t, max(a, 0.02)) * np.exp(-np.maximum(t - a, 0) / max(0.25, 0.5 * p["roll_hold"] + p["roll_fall"]))
    if p["grain_depth"] > 0.01:
        gc = _grains(n, p["grain_rate"] * p["air_div"], min(1.0, 1.5 * p["grain_depth"]), rng, jit=p["grain_jit"])
        e = e * gc ** p["grain_sharp"]
    return _peak1(y * e)


def _j12_boom(tl: np.ndarray, p: dict) -> np.ndarray:
    td = max(p["boom_drop"], 0.001)
    ph = 2 * np.pi * (p["boom_f1"] * tl + (p["boom_f0"] - p["boom_f1"]) * td * (1 - np.exp(-tl / td)))
    x = np.sin(ph) * _rise(tl, p["boom_att"]) * np.exp(-np.maximum(tl - p["boom_att"], 0) / p["boom_tau"])
    if p["boom_drive"] > 0.01:
        g = 1 + 8 * p["boom_drive"]
        x = _filt(np.tanh(g * x) / np.tanh(g), "low", 1100.0, 2, LO)
    return _peak1(x)


def _j13_blast(n: int, p: dict, rng: np.random.Generator) -> np.ndarray:
    t = np.arange(n) / SR
    tau = p["blast_tau"]
    fc = p["blast_f"] * (0.5 + 0.5 * np.exp(-t / max(tau, 0.02)))
    y = dsp.svf(_noise(n, rng, "pink"), fc, 0.55, "bandn") * (1 - np.exp(-t / 0.002)) * np.exp(-t / tau)
    return _peak1(y)


def _gate_curve(n: int, at: float, ms: float) -> np.ndarray:
    """J14: ones, then a raised-cosine fall to 0 at `at` s, then zeros."""
    g = np.ones(n)
    i = _n(at)
    if i < n:
        k = max(8, _n(ms / 1000))
        fall = 0.5 + 0.5 * np.cos(np.linspace(0, np.pi, k))
        m = min(k, n - i)
        g[i:i + m] = fall[:m]
        g[i + m:] = 0.0
    return g


def _hit_length(p: dict) -> float:
    ring = p["roll_hold"] + 5 * p["roll_fall"] + 0.4
    T = min(7.0, max(ring, 3 * p["sub_tau"] + 0.3, 1.2 * p["hall_rt"] + 0.3))
    if p["gate"] > 0:
        T = min(T, p["gate"] + p["gate_ms"] / 1000 + 0.01)
    return T


def _v3_direct(s: dict, p: dict, rng: np.random.Generator) -> None:
    n, tl, a = s["n"], s["tl"], p["body_attack"]
    s["body"] = _up(_j7_body(tl, p, rng), n)
    s["sub"] = _up(_j8_sub(tl, p, rng), n)
    shell = np.zeros(n)
    if p["shell"] > 0.01:
        shell = _up(_shell(tl, p["shell_lo"], 4.5, 0.3, rng) * _rise(tl, a), n)
    s["crack"] = _j2_crack(n, p, rng)
    d = p["body"] * s["body"] + 0.6 * p["shell"] * shell + p["crack"] * s["crack"]
    if p["boom"] > 0.005:
        d = d + p["boom"] * _up(_j12_boom(tl, p), n)
    if p["snap"] > 0.005:
        d = d + p["snap"] * _j3_snap(n, rng)
    s["direct"] = d


def _v3_layers(s: dict, p: dict, rng: np.random.Generator) -> None:
    n = s["n"]
    s["sheen"] = _j4_sheen(n, p, rng) if p["sheen"] > 0.005 else None
    s["plate"] = _j5_plate(n, p, rng) if p["plate"] > 0.005 else None
    s["scatter"] = _j6_scatter(n, p, rng) if p["scatter"] > 0.005 else None
    s["rc"] = _j10_roll(n, p, rng) if p["roll"] > 0.01 else None
    s["ac"] = _j11_air(n, p["body_attack"], p, rng) if p["air"] > 0.005 else None
    s["bc"] = _j13_blast(n, p, rng) if p["blast"] > 0.01 else None
    s["irs"] = None
    if p["hall_wet"] > 0.031:
        ir1 = hall_ir(p["hall_rt"], p["hall_damp"], 0.8, 0.012, rng=rng)
        ir2 = hall_ir(p["hall_rt"], p["hall_damp"], 0.8, 0.012 + rng.uniform(0.001, 0.005), rng=rng)
        s["irs"] = (ir1, ir2)


def _diffuse(shared: np.ndarray | None, make: Callable[[], np.ndarray], w: float) -> np.ndarray | None:
    if shared is None:
        return None
    return (1 - w) * shared + w * make() if w > 0.01 else shared


def _v3_dry(c: int, s: dict, p: dict, rng: np.random.Generator) -> np.ndarray:
    n, w = s["n"], p["width"]
    dry = np.array(s["direct"], copy=True)
    for key in ("sheen", "plate"):
        if s[key] is not None:
            dry += p[key] * s[key][:, c]
    if s["scatter"] is not None:
        dry += p["scatter"] * P_REF * s["scatter"][:, c]
    rl = _diffuse(s["rc"], lambda: _j10_roll(n, p, rng), w)
    if rl is not None:
        if p["roll_in"] > 0.005:
            rl = rl * _rise(np.arange(n) / SR, p["roll_in"])
        dry += p["roll"] * P_REF * rl
    al = _diffuse(s["ac"], lambda: _j11_air(n, p["body_attack"], p, rng), w)
    if al is not None:
        dry += p["air"] * P_REF * al
    bl = _diffuse(s["bc"], lambda: _j13_blast(n, p, rng), w)
    if bl is not None:
        dry += p["blast"] * P_REF * bl
    return dry


def _v3_channel(out: np.ndarray, c: int, s: dict, p: dict, rng: np.random.Generator) -> None:
    kl = s["kl"]
    dry = _v3_dry(c, s, p, rng)
    main = dry + p["sub"] * P_REF * s["sub"]
    if p["gate"] <= 0:
        main = dsp.fade(main, 0.0, 0.3 * s["T"])
    _add(out[:, c], main, kl)
    if s["irs"] is not None:
        send = s["direct"] + 0.5 * (dry - s["direct"])
        send = _filt(_filt(send, "low", p["hall_tone"], 1), "high", 110.0, 2)
        _add(out[:, c], p["hall_wet"] * signal.fftconvolve(send, s["irs"][c]), kl)


def _straight_db(sw: np.ndarray) -> np.ndarray:
    ev = np.sqrt(np.convolve(sw ** 2, np.ones(480) / 480, mode="same") + 1e-9)
    v = np.linspace(0, 1, len(sw))
    sw = sw * 10 ** ((-34 + 34 * v ** 1.3) / 20) / (ev / np.max(ev))
    return _filt(sw, "low", 4000.0, 2)


def _v3_swell(out: np.ndarray, s: dict, p: dict, rng: np.random.Generator) -> None:
    kl = s["kl"]
    if kl <= 64:
        return
    m = _n(0.15)
    head = s["crack"][:m] + 0.6 * s["body"][:m]
    pk = np.max(np.abs(out))
    pre = p["pre"]
    for c in range(2):
        ir = hall_ir(max(0.4, 1.8 * pre), 0.4, 0.6, 0.0, seconds=1.2 * pre + 0.2, rng=rng)
        sw = _filt(_fit(signal.fftconvolve(head, ir), kl)[::-1], "low", 2500.0, 2)
        if int(p["pre_shape"]) == 1:
            sw = _straight_db(sw)
        else:
            sw = sw * np.linspace(0, 1, kl) ** 1.5
        sw = dsp.fade(sw, 0.01, 0.002)
        out[:kl, c] += sw * p["pre_level"] * pk / (np.max(np.abs(sw)) + 1e-12)


def _v3_gate(y: np.ndarray, kl: int, p: dict) -> np.ndarray:
    n = len(y) - kl
    t = np.arange(n) / SR
    g = _gate_curve(n, p["gate"], p["gate_ms"])
    if p["gate_db"] > 0.1:
        gf = max(0.02, min(p["gate_fall"], 0.8 * p["gate"]))
        g = g * 10 ** (-p["gate_db"] * np.clip((t - (p["gate"] - gf)) / gf, 0, 1) / 20)
    y = np.array(y, copy=True)
    y[kl:] *= g[:, None]
    y = np.concatenate([y, np.zeros((_n(0.02), 2))])
    return dsp.fade(y, 0.001, 0.0)


def _v3_finish(out: np.ndarray, s: dict, p: dict, rng: np.random.Generator) -> np.ndarray:
    y = _filt(out, "high", 24.0, 2)
    if p["top"] < 19000:
        y = _filt(y, "low", p["top"], 2)
    if p["floor_db"] > -105:
        y = _mic(y, rng, p["floor_db"], 22.0, -0.5)
    if p["gate"] > 0:
        return _v3_gate(y, s["kl"], p)
    y = dsp.trim_tail(y, -70.0, 0.05)
    return dsp.fade(y, 0.001, min(0.35, 0.1 * len(y) / SR))


def render_v3(p: dict, rng: np.random.Generator) -> np.ndarray:
    """Version 3: the cinematic boom and the designed hit. Stereo, peak 0.89."""
    p = {**DEFAULTS, **p}
    T = _hit_length(p)
    n = _n(T)
    s: dict[str, Any] = {"T": T, "n": n, "tl": np.arange(int(np.floor(LO * T)) + 8) / LO}
    _v3_direct(s, p, rng)
    _v3_layers(s, p, rng)
    s["kl"] = _n(p["pre"]) if p["pre"] > 0.02 else 0
    hall = max(len(ir) for ir in s["irs"]) if s["irs"] is not None and p["gate"] <= 0 else 0
    out = np.zeros((s["kl"] + n + hall, 2))
    for c in range(2):
        _v3_channel(out, c, s, p, rng)
    _v3_swell(out, s, p, rng)
    y = _v3_finish(out, s, p, rng)
    return y * PEAK / (np.max(np.abs(y)) + 1e-12)


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """One take. A set with `pre_len` renders with version 2, else version 3."""
    if "pre_len" in p:
        return render_v2(p, rng)
    return render_v3(p, rng)


# ------------------------------------------------------------ families

def _m(*sets: dict, **over: Any) -> dict:
    out: dict = {}
    for s in sets:
        out.update(s)
    out.update(over)
    return out


BP1 = dict(pre=0, pre_level=0.1, crack=0.3, crack_f=3000, crack_ms=4, debris=0.1, blast=0.05, blast_f=300,
           blast_tau=0.15, body_f0=120, body_f1=75, drop_tau=0.05, body_tau=0.18, body_attack=0.1, modes=0.1,
           body=0.35, drive=0.15, sub=0.15, sub_f=48, sub_semis=7, sub_tau=0.5, shell=0.1, shell_lo=110, roll=1.5,
           roll_cut=200, roll_hold=0.7, roll_fall=0.5, roll_rate=1.5, roll_depth=0.4, grain_rate=36,
           grain_depth=1.0, air=0.2, air_f=4500, hall_wet=0.04, hall_rt=0.6, hall_damp=0.4, width=1.0, gate=1.1,
           gate_ms=15, floor_db=-110)
BC1 = _m(DEFAULTS, sub=0.35, sub_f=42, sub_tau=0.7, body_f0=160, body_f1=62, body_tau=0.25, shell=0.8,
         shell_lo=110, crack=0.8, crack_f=1500, crack_ms=10, debris=0.7, blast=0.8, blast_f=600, blast_tau=0.3,
         hall_wet=0.8, roll=0.45, roll_cut=200, pre=0, snap=0.6, hall_tone=9000, roll_depth=0.22, roll_rate=0.5)
BC3 = _m(BC1, blast=1.0, blast_f=1200, sub=0.2, body=0.7, crack=1.4, crack_f=2500, roll=0.35, sheen=1.4,
         sheen_ms=7, sheen_f=6000)
BP3 = _m(BP1, air=0.7, air_f=8500, air_q=1.6, gate_db=15, gate_fall=0.25, gate_ms=8, air_div=0.5, grain_sharp=4.0)
BC4 = _m(BC3, sheen=0.3, sheen_ms=14, sheen_top=11000, top=13000, crack_f=1600, crack=0.95)
BP4 = _m(DEFAULTS, BP3, grain_jit=0.7, crack_f=1800, snap=0, sheen=0)


def BG(rate: float) -> dict:
    """Round 6 grain set: rate, jitter 0.5, boom drive 0.1."""
    return dict(grain_rate=rate, grain_jit=0.5, boom_drive=0.1)


def _family(base: dict, overrides: dict[str, dict]) -> list[tuple[str, dict]]:
    return [(name, _m(base, over)) for name, over in overrides.items()]


IMPACT_R2 = _family(BC1, {
    "classic": {},
    "deep": dict(body_f0=110, body_f1=44, sub=0.6, sub_f=34, sub_tau=1.0, roll=0.6, roll_cut=140, roll_hold=2.4,
                 crack=0.4, crack_f=1100, blast_f=400, blast_tau=0.45, hall_rt=4.5, hall_damp=0.6),
    "punch": dict(body_f0=210, body_f1=58, drop_tau=0.03, body_tau=0.18, crack=1.0, crack_f=2600, crack_ms=7,
                  blast_tau=0.15, roll_hold=0.9, roll_fall=0.4, hall_rt=2.0, hall_wet=0.6, sub_tau=0.45),
    "metal": dict(shell=0.8, shell_lo=210, modes=0.5, crack_f=4000, debris=1.0, air=0.08, air_f=6500,
                  hall_damp=0.1, hall_rt=3.6, blast_f=1200),
    "swell": dict(pre=0.9, pre_level=0.35, body_attack=0.03, roll_hold=2.0, roll=0.55, hall_rt=3.8),
})
_PRO_R2_OVER = {
    "rumble": {},
    "air": dict(gate=0.6, body_attack=0.25, air=0.5, air_f=9000, grain_depth=0.5, roll=1.0, roll_hold=0.5, sub=0.35),
    "short": dict(gate=0.65, body_attack=0.05, crack=0.5, grain_rate=42, roll_hold=0.4, roll_fall=0.3, body=0.5),
    "deep": dict(gate=1.3, sub=0.6, sub_f=36, body_f1=62, grain_rate=28, roll_cut=150, roll_hold=0.9, air=0.06),
    "swell": dict(pre=0.18, pre_level=0.3, body_attack=0.2, gate=0.9, grain_rate=32),
}
PRO_IMPACT_R2 = _family(BP1, _PRO_R2_OVER)
IMPACT_R3 = _family(BC3, {
    "classic": {},
    "deep": dict(body_f0=110, body_f1=44, body=0.8, sub=0.45, sub_f=34, sub_tau=1.0, roll=0.5, roll_cut=140,
                 roll_hold=2.2, crack=1.3, crack_f=1600, blast_f=700, blast_tau=0.45, hall_rt=4.5, hall_damp=0.6,
                 sheen=1.5, sheen_ms=10),
    "punch": dict(body_f0=210, body_f1=58, drop_tau=0.03, body_tau=0.18, crack=1.35, crack_f=3000, crack_ms=7,
                  blast_tau=0.12, roll_hold=0.9, roll_fall=0.4, hall_rt=2.0, hall_wet=0.6, sub=0.15, sub_tau=0.45,
                  sheen=1.6, sheen_ms=5),
    "metal": dict(shell=0.8, shell_lo=260, modes=0.5, crack_f=3500, debris=0.6, air=0, air_f=6500, hall_damp=0.35,
                  hall_rt=3.6, blast_f=2000, sheen=1.8, sheen_ms=9, sheen_f=7000, crack=2.0, body_attack=0.001,
                  hall_tone=5500),
    "swell": dict(pre=0.5, pre_level=0.35, pre_shape=1, body_attack=0.03, roll_hold=2.0, sheen=1.0, roll=0.45,
                  hall_rt=3.8),
    "thunder": dict(body=0.5, body_f0=130, body_f1=48, debris=1.0, crack_f=1800, blast=1.0, blast_f=900,
                    blast_tau=0.5, roll=0.7, roll_hold=1.9, roll_depth=0.35, roll_rate=1.2, roll_cut=260, sheen=1.6,
                    sheen_ms=9, crack=1.7, hall_rt=3.4),
})
PRO_IMPACT_R3 = _family(BP3, {
    "rumble": {},
    "air": dict(gate=0.6, body_attack=0.25, air=0.9, air_f=9000, grain_depth=0.5, roll=1.0, roll_hold=0.5, sub=0.35,
                gate_db=18, gate_fall=0.15, air_div=1.0, grain_sharp=3.0),
    "short": dict(gate=0.65, body_attack=0.05, crack=0.5, grain_rate=42, roll_hold=0.4, roll_fall=0.3, body=0.5,
                  gate_db=14, gate_fall=0.2),
    "deep": dict(gate=1.3, sub=0.6, sub_f=36, body_f1=62, grain_rate=28, roll_cut=150, roll_hold=0.9, air=0.6,
                 gate_db=16, gate_fall=0.25),
    "swell": dict(pre=0.18, pre_level=0.3, body_attack=0.2, gate=0.9, grain_rate=32),
})
_R4_KEYS = ("crack", "body_f1", "body_attack", "body", "sub", "roll", "roll_cut", "roll_hold", "roll_fall",
            "grain_rate", "grain_depth", "air", "gate", "gate_db")
_R4_VALUES = {
    "rumble": (0.5122, 39.5286, 0.1351, 0.1221, 0.0082, 1.264, 101.7411, 0.6747, 0.3832, 22.7054, 0.9699, 0.4949,
               0.955, 20.3501),
    "air": (0.3841, 42.8117, 0.0444, 0.3126, 0.6875, 1.4018, 255.8402, 0.7123, 0.4128, 22.3646, 0.8043, 0.6265,
            0.8096, 15.4599),
    "short": (0.4808, 75.0, 0.0464, 0.5303, 0.1124, 1.4651, 194.2516, 0.4826, 0.2785, 39.6209, 1.0, 0.6444, 0.6288,
              14.1698),
    "deep": (0.1025, 50.8261, 0.0534, 0.2825, 0.4785, 1.3857, 109.0885, 0.3807, 0.601, 18.6677, 0.6976, 0.7944,
             1.2768, 16.8876),
    "swell": (0.3, 75.0, 0.2, 0.35, 0.15, 1.5, 200, 0.7, 0.5, 32, 1.0, 0.7, 0.9, 15),
}
_R4_COMMON = dict(pre_shape=1, hall_tone=6000, sheen_ms=8, sheen_f=6363.961, crack_f=1800)
PRO_IMPACT_R4 = [(name, _m(BP4, r3, dict(zip(_R4_KEYS, _R4_VALUES[name])), _R4_COMMON))
                 for name, r3 in PRO_IMPACT_R3]
_R6 = {
    "rumble": _m(dict(boom=0.6, boom_f0=120, boom_f1=42, boom_drop=0.07, boom_tau=0.3, boom_att=0.07, roll_in=0.03,
                      crack=0.7), BG(40)),
    "air": _m(dict(boom=0.55, boom_f0=130, boom_f1=46, boom_drop=0.06, boom_tau=0.25, boom_att=0.08, roll_in=0.03),
              BG(38)),
    "short": _m(dict(boom=0.55, boom_f0=140, boom_f1=52, boom_drop=0.05, boom_tau=0.2, boom_att=0.06, roll_in=0.03),
                BG(42)),
    "deep": _m(dict(boom=0.65, boom_f0=95, boom_f1=36, boom_drop=0.1, boom_tau=0.3, boom_att=0.08, roll_in=0.04,
                    gate_db=12, air=0.45), BG(34)),
    "swell": _m(dict(boom=0.5, boom_f0=110, boom_f1=44, boom_drop=0.08, boom_tau=0.35, boom_att=0.12, roll_in=0.06),
                BG(36)),
}
PRO_IMPACT_R6 = [(name, _m(r4, _R6[name])) for name, r4 in PRO_IMPACT_R4]
IMPACT_R4 = _family(BC4, {
    "classic": {},
    "deep": dict(body_f0=95, body_f1=38, body=0.8, sub=0.55, sub_f=34, sub_tau=1.0, roll=0.55, roll_cut=110,
                 roll_hold=2.1, crack=1.1, crack_f=1500, blast=0.9, blast_f=600, blast_tau=0.45, shell=0.7,
                 hall_rt=4.5, hall_damp=0.6, hall_tone=4000, sheen=0.45, sheen_ms=16, top=11000),
    "punch": dict(body_f0=230, body_f1=66, drop_tau=0.03, body_tau=0.22, crack=0.95, modes=0.5, shell=0.5, roll=0.25,
                  crack_f=1800, crack_ms=7, blast_tau=0.12, roll_hold=0.9, roll_fall=0.4, hall_rt=2.1, hall_wet=0.45,
                  sub=0.15, sub_tau=0.45, sheen=0.35, sheen_ms=10),
    "metal": dict(shell=0.8, shell_lo=260, modes=0.5, crack_f=2200, debris=0.4, air=0, air_f=6500, hall_damp=0.35,
                  hall_rt=3.6, blast_f=2000, sheen=0.35, sheen_ms=12, sheen_f=7000, crack=0.95, body_attack=0.001,
                  hall_tone=5500, plate=0.7, plate_f=380, plate_tau=0.9, top=11000),
    "swell": dict(pre=0.5, pre_level=0.35, pre_shape=1, body_attack=0.03, roll_hold=2.0, sheen=0.3, roll=0.45,
                  hall_rt=3.8),
    "blast": dict(body=0.5, body_f0=130, body_f1=48, debris=0.6, crack_f=1400, blast=1.0, blast_f=900, blast_tau=0.5,
                  roll=0.7, roll_hold=1.9, roll_depth=0.35, roll_rate=1.2, roll_cut=260, sheen=0.3, sheen_ms=12,
                  crack=1.1, hall_rt=3.4, scatter=0.5, scatter_s=2.0),
})

FAMILIES: dict[str, list[tuple[str, dict]]] = {"impact": IMPACT_R4, "pro_impact": PRO_IMPACT_R6}
FAMILIES_R2 = {"impact": IMPACT_R2, "pro_impact": PRO_IMPACT_R2}
FAMILIES_R3 = {"impact": IMPACT_R3, "pro_impact": PRO_IMPACT_R3}
FAMILIES_R4 = {"impact": IMPACT_R4, "pro_impact": PRO_IMPACT_R4}

SEARCH_KEYS: dict[str, list[str]] = {
    "impact": ["crack", "crack_f", "sheen", "sheen_ms", "debris", "blast", "blast_f", "blast_tau", "body_f0",
               "body_f1", "body_tau", "body", "sub", "shell", "roll", "roll_cut", "roll_hold", "roll_fall",
               "roll_depth", "hall_wet", "hall_rt", "hall_tone", "width"],
    "pro_impact": ["crack", "body", "body_f1", "body_attack", "sub", "roll", "roll_cut", "roll_hold", "roll_fall",
                   "grain_rate", "grain_depth", "air", "gate", "gate_db"],
}
LIMITS: dict[str, dict[str, tuple]] = {
    "pro_impact": {"gate": (0.55, 1.35), "body_attack": (0.04, 0.3, "log"), "roll_cut": (100.0, 260.0, "log"),
                   "roll_hold": (0.2, 1.2), "roll_fall": (0.15, 0.8, "log"), "sub": (0.0, 0.7),
                   "air": (0.2, 1.0), "gate_db": (12.0, 22.0)},
    "impact": {"roll_hold": (0.8, 3.0), "hall_rt": (1.6, 6.0, "log"), "roll_fall": (0.25, 1.6, "log")},
}


def space_for(cat: str) -> dict[str, Any]:
    """The search space of a category: its search keys, at the limit range when one exists."""
    limits = LIMITS.get(cat, {})
    return {k: limits.get(k, SPACE[k]) for k in SEARCH_KEYS[cat]}


def _member(cat: str, preset: int | str) -> tuple[str, dict]:
    members = FAMILIES[cat]
    if isinstance(preset, str):
        found = dict(members)
        if preset not in found:
            raise KeyError(f"impact: no member {preset!r} in {cat!r}")
        return preset, dict(found[preset])
    name, params = members[preset]
    return name, dict(params)


def recipe_for(cat: str = "impact", preset: int | str = 0) -> types.SimpleNamespace:
    """A recipe object for 1 family member (index or name)."""
    name, params = _member(cat, preset)
    return types.SimpleNamespace(
        MODULE=__name__, SPACE=space_for(cat), DEFAULTS=params, render=render, TARGET=TARGETS[cat],
        ALTS=list(ALTS), REFSET=cat, PRESET=name,
    )


# ------------------------------------------------------------ pro-study measurement

def _frames_db(m: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    nf = max(1, len(m) // 48)
    fr = np.resize(m, nf * 48).reshape(nf, 48) if len(m) >= 48 else m.reshape(1, -1)
    e = np.sqrt(np.mean(fr ** 2, axis=1) + 1e-20)
    return e, 20 * np.log10(e + 1e-12)


def _first(mask: np.ndarray, default: int) -> int:
    idx = np.nonzero(mask)[0]
    return int(idx[0]) if len(idx) else default


def _envelope_keys(m: np.ndarray) -> dict:
    e, edb = _frames_db(m)
    pk = int(np.argmax(e))
    out: dict[str, Any] = {"attack_ms": pk - _first(edb >= edb[pk] - 20, pk), "peak_ms": pk}
    sdb = 10 * np.log10(np.convolve(e ** 2, np.ones(10) / 10, mode="same") + 1e-20)
    spk = int(np.argmax(sdb))
    top = sdb[spk]
    rest = sdb[spk:]
    out["t20"] = _first(rest < top - 20, len(rest))
    out["t40"] = _first(rest < top - 40, len(rest))
    a = spk + _first(rest < top - 6, 0)
    b = spk + _first(rest < top - 40, len(rest))
    if b - a > 5:
        out["slope_dBs"] = round(float(np.polyfit(np.arange(a, b) / 1000, sdb[a:b], 1)[0]), 1)
    else:
        out["slope_dBs"] = float("nan")
    act = np.nonzero(sdb > top - 50)[0]
    end = int(act[-1])
    out["len50"] = round((end - int(act[0])) / 1000, 3)
    out["gate_db40ms"] = round(float(sdb[end - 40] - sdb[end]), 1) if end > 40 else 0.0
    out["pre_gate"] = round(float(sdb[end - 60] - top), 1) if end > 60 else 0.0
    return out


def _band_db(f: np.ndarray, pxx: np.ndarray, lo: float, hi: float, total: float) -> float:
    s = np.sum(pxx[(f >= lo) & (f < hi)])
    return float(10 * np.log10(s / total)) if s > 0 else -120.0


def _spectral_keys(m: np.ndarray) -> dict:
    f, pxx = signal.welch(m, SR, nperseg=min(len(m), 16384))
    total = np.sum(pxx) + 1e-30
    centres = 1000 * 2 ** (np.arange(-17, 14) / 3)
    bal = np.array([_band_db(f, pxx, c / 2 ** (1 / 6), c * 2 ** (1 / 6), total) for c in centres])
    sel = (centres >= 100) & (centres <= 10000)
    octs = (31.5, 63, 125, 250, 500, 1000, 2000, 4000, 8000, 16000)
    return {
        "sh150": round(float(np.sum(pxx[f < 150]) / total), 3),
        "tilt": round(float(np.polyfit(np.log2(centres[sel]), bal[sel], 1)[0]), 2),
        "oct": {int(c): round(_band_db(f, pxx, c / np.sqrt(2), c * np.sqrt(2), total), 1) for c in octs},
        "cen": int(round(np.sum(f * pxx) / total)),
    }


def _ridge(m: np.ndarray) -> tuple[int, int, int]:
    seg = min(8192, len(m))
    f, _t, z = signal.stft(m, SR, nperseg=seg, noverlap=max(0, seg - 960), boundary=None, padded=False)
    band = (f >= 25) & (f <= 3000)
    if z.shape[1] == 0 or not np.any(band):
        return (0, 0, 0)
    mag = np.abs(z[band])
    peaks = mag.max(axis=0)
    keep = peaks >= peaks.max() * 10 ** (-25 / 20) if peaks.max() > 0 else np.zeros_like(peaks, bool)
    if not np.any(keep):
        return (0, 0, 0)
    freqs = f[band][np.argmax(mag, axis=0)][keep]
    return (int(round(freqs[0])), int(round(np.median(freqs))), int(round(freqs[-1])))


def measure(x: np.ndarray) -> dict:
    """Pro-study measurement of a render (mono or stereo)."""
    x = np.asarray(x, dtype=float)
    x = x if x.ndim == 2 else np.column_stack([x, x])
    m = x.mean(axis=1)
    out: dict[str, Any] = {"dur": round(len(m) / SR, 3)}
    out.update(_envelope_keys(m))
    out.update(_spectral_keys(m))
    left, right = x[:, 0], x[:, 1]
    out["corr"] = round(float(np.sum(left * right) / (np.sqrt(np.sum(left ** 2) * np.sum(right ** 2)) + 1e-30)), 2)
    side, mid = (left - right) / 2, (left + right) / 2
    out["side_db"] = round(float(10 * np.log10((np.sum(side ** 2) + 1e-30) / (np.sum(mid ** 2) + 1e-30))), 1)
    out["ridge"] = _ridge(m)
    return out


STUDY: dict[str, dict[str, tuple[float, float]]] = {
    "impact": {"t40": (1300, 2600), "slope_dBs": (-20, -10), "tilt": (-6.5, -3.5), "sh150": (0.65, 0.93)},
    "pro_impact": {"len50": (0.6, 1.3), "attack_ms": (50, 290), "t40": (250, 1100), "gate_db40ms": (8, 40),
                   "sh150": (0.75, 0.97), "tilt": (-6.0, -2.0), "corr": (-0.1, 0.5), "pre_gate": (-34, -22)},
}
_LINEAR_KEYS = {"tilt", "slope_dBs", "corr", "gate_db40ms", "sh150"}
_RIDGE_INDEX = {"ridge0": 0, "ridge": 1, "ridge1": 2}


def _value(m: dict, key: str) -> float | None:
    if key in _RIDGE_INDEX:
        r = m.get("ridge")
        return None if r is None else r[_RIDGE_INDEX[key]]
    return m.get(key)


def study_penalty(m: dict, targets: dict[str, tuple[float, float]]) -> float:
    """Distance of a measurement from study ranges (octaves or range widths)."""
    total = 0.0
    for key, (lo, hi) in targets.items():
        v = _value(m, key)
        if v is None or not np.isfinite(v) or lo <= v <= hi:
            continue
        bound = lo if v < lo else hi
        if lo > 0 and v > 0 and key not in _LINEAR_KEYS:
            total += abs(np.log2(v / bound))
        else:
            total += abs(v - bound) / max(1e-6, hi - lo)
    return float(total)


def study_extra(targets: dict[str, tuple[float, float]], weight: float = 1.0) -> Callable[[np.ndarray], float]:
    """A critic extra term: -weight x the study penalty of the render."""

    def extra(x: np.ndarray, row: dict | None = None) -> float:
        return -weight * study_penalty(measure(x), targets)

    return extra
