"""Cinematic boom and designed sub hit.

Spec: docs/specs/hark/recipes-impacts-impact.md. Two generations live here. A
parameter set that holds ``pre_len`` uses generation 2, the physical trailer
boom. Any other set uses generation 3. ``swell`` is a separate air riser that
other recipes can call.
"""

from __future__ import annotations

import os
import types
from collections.abc import Callable
from typing import Any

import numpy as np
from scipy import signal

from hark.forge.engine import dsp
from hark.forge.recipes.impacts import _common as cm

SR = cm.SR
_BASE_RATE = 6000              # the low layers are built at this rate, then upsampled
_UPSAMPLE = SR // _BASE_RATE
_OUT_PEAK = 0.89
_GEN2_LEN_S = 5.0
_LAYER_GAIN = 1.25
_DRUM_RATIOS = (1.0, 1.594, 2.136, 2.296, 2.653, 2.918)   # circular membrane overtones

TARGET = "a deep cinematic boom impact"
TARGETS = {
    "impact": "a deep cinematic boom impact",
    "pro_impact": "a deep designed sub impact with a granular rumble tail, gated",
}
ALTS = ["a kick drum", "an explosion", "a door slam", "thunder"]
REFSET = "impact"

_REF_DIR = os.environ.get("HARK_REFS", "refs").replace("\\", "/").rstrip("/")
REFS = [_REF_DIR + "/impact/impact_%02d.wav" % k for k in (1, 2, 3)]


# ---------------------------------------------------------------- basic tools

def _samples(seconds: float) -> int:
    return max(1, int(round(seconds * SR)))


def _unit(sig: np.ndarray) -> np.ndarray:
    return sig / (np.max(np.abs(sig)) + 1e-12)


def _unit_rms(sig: np.ndarray) -> np.ndarray:
    return sig / (np.sqrt(np.mean(sig ** 2)) + 1e-12)


_BTYPE = {"low": "lowpass", "high": "highpass", "band": "bandpass"}


def _sos(sig: np.ndarray, kind: str, cutoff: Any, order: int = 2, rate: int = SR) -> np.ndarray:
    """Butterworth filter at ``rate``; every cutoff is held inside [10 Hz, 0.45 rate]."""
    edges = np.clip(np.asarray(cutoff, dtype=float), 10.0, 0.45 * rate)
    sections = signal.butter(order, edges, btype=_BTYPE[kind], fs=rate, output="sos")
    return signal.sosfilt(sections, sig, axis=0)


def _lp(sig: np.ndarray, cutoff: float, order: int = 2, rate: int = SR) -> np.ndarray:
    return _sos(sig, "low", cutoff, order, rate)


def _hp(sig: np.ndarray, cutoff: float, order: int = 2, rate: int = SR) -> np.ndarray:
    return _sos(sig, "high", cutoff, order, rate)


def _bp(sig: np.ndarray, lo: float, hi: float, order: int = 2) -> np.ndarray:
    return _sos(sig, "band", (lo, hi), order)


# Coefficients of a cheap 3-pole pink filter.
_PINK_NUM = np.array([0.049922035, -0.095993537, 0.050612699, -0.004408786])
_PINK_DEN = np.array([1.0, -2.494956002, 2.017265875, -0.522189400])


def _noise(n: int, rng: np.random.Generator, colour: str = "white") -> np.ndarray:
    """Unit-RMS noise: white, pink (IIR filtered) or brown (leaky sum)."""
    raw = rng.standard_normal(n)
    if colour == "pink":
        raw = signal.lfilter(_PINK_NUM, _PINK_DEN, raw)
    elif colour == "brown":
        raw = signal.lfilter([1.0], [1.0, -0.997], raw)
    return _unit_rms(raw)


def _mic(st: np.ndarray, rng: np.random.Generator, floor_db: float, low_cut: float = 20.0,
         air_db: float = -1.0) -> np.ndarray:
    """Microphone: high-pass, a 10 kHz air shelf, and its own pink hiss in each channel."""
    st = dsp.peq(_hp(st, low_cut, 1), 10000.0, air_db, 0.6)
    hiss_gain = np.max(np.abs(st)) * 10 ** (floor_db / 20)
    return np.column_stack([st[:, ch] + _noise(len(st), rng, "pink") * hiss_gain for ch in range(st.shape[1])])


def _slide(tt: np.ndarray, f_from: float, f_to: float, tau: float, rng: np.random.Generator) -> np.ndarray:
    """Sine whose pitch moves from f_from to f_to along an exponential, phase in closed form."""
    tau = max(tau, 1e-4)
    cycles = f_to * tt + (f_from - f_to) * tau * (1 - cm.decay(tt, tau))
    return np.sin(cm.TWO_PI * cycles + rng.uniform(-0.3, 0.3))


def _swell_fall(tt: np.ndarray, attack: float, tau: float) -> np.ndarray:
    """Exponential fall that opens with a raised-cosine fade-in of ``attack`` seconds."""
    shape = cm.decay(tt, max(tau, 1e-4))
    early = tt < attack
    shape[early] *= 0.5 - 0.5 * np.cos(np.pi * tt[early] / attack)
    return shape


def _fade_in(tt: np.ndarray, attack: float) -> np.ndarray:
    """All ones, except a raised-cosine ramp over the first ``attack`` seconds."""
    shape = np.ones_like(tt)
    if attack > 0:
        early = tt < attack
        shape[early] = 0.5 - 0.5 * np.cos(np.pi * tt[early] / attack)
    return shape


def _to_size(sig: np.ndarray, n: int) -> np.ndarray:
    if len(sig) < n:
        return np.concatenate([sig, np.zeros((n - len(sig),) + sig.shape[1:])])
    return sig[:n]


def _raise_rate(low: np.ndarray, n: int) -> np.ndarray:
    """Polyphase upsampling from the base rate to 48 kHz, cut or padded to n."""
    return _to_size(signal.resample_poly(low, _UPSAMPLE, 1), n)


def _mix_in(dst: np.ndarray, sig: np.ndarray, at: int) -> None:
    if at < len(dst):
        span = min(len(sig), len(dst) - at)
        dst[at:at + span] += sig[:span]


# ---------------------------------------------------------------- the hall

def _band_decays(noise: np.ndarray, tt: np.ndarray, rt60: float, damp: float, size: float) -> np.ndarray:
    """Split noise into 4 bands; higher bands die sooner as ``damp`` grows."""
    times = (1.25 * rt60, rt60, rt60 * (1 - 0.5 * damp), rt60 * (1 - 0.8 * damp))
    left, acc = noise, np.zeros_like(noise)
    for idx, edge in enumerate((250.0, 1500.0, 6000.0)):
        part = _lp(left, edge, 2)
        left = left - part
        acc += part * np.exp(-6.91 * tt / max(times[idx], 0.05))
    acc += left * np.exp(-6.91 * tt / max(times[3], 0.05))
    return acc * (1 - cm.decay(tt, 0.008 + 0.03 * size))


def _early_taps(n: int, size: float, rng: np.random.Generator) -> np.ndarray:
    taps = np.zeros(n)
    for _ in range(40):
        at = int(np.floor(rng.uniform(0.003, 0.02 + 0.08 * size) * SR))
        gain = rng.uniform(0.05, 0.2) * rng.choice([-1.0, 1.0])
        if at < n:
            taps[at] += gain * np.exp(-at / SR / 0.06)
    return taps


def hall_ir(rt60: float, damp: float, size: float = 0.8, predelay: float = 0.02, seconds: float | None = None,
            *, rng: np.random.Generator | None = None) -> np.ndarray:
    """Mono hall impulse response with unit energy.

    Pass the render generator as ``rng``. Without one the hall uses seed 0, so
    two calls give the same response.
    """
    gen = np.random.default_rng(0) if rng is None else rng
    if seconds is None:
        seconds = min(5.0, 1.1 * rt60 + 0.1)
    n = _samples(seconds)
    tt = cm.times(n)
    late = _band_decays(gen.standard_normal(n), tt, rt60, damp, size)
    early = _early_taps(n, size, gen)
    resp = 0.25 * late + _lp(early, 7000.0, 1)
    resp = resp / (np.sqrt(np.sum(resp ** 2)) + 1e-12)
    if predelay <= 0:
        return resp
    return np.concatenate([np.zeros(_samples(predelay)), resp])


# ---------------------------------------------------------------- tone shaping

def _desk_drive(sig: np.ndarray, drive: float, tone: float) -> np.ndarray:
    """Soft tanh drive scaled to the peak, followed by a low-pass at ``tone``."""
    if drive <= 0.001:
        return sig
    push = 1 + 7 * drive
    top = np.max(np.abs(sig)) + 1e-12
    shaped = top * np.tanh(push * sig / top) / push * (1 + 0.15 * drive)
    return _lp(shaped, tone, 2)


def _feedback_comb(sig: np.ndarray, delay: np.ndarray, fb: float) -> np.ndarray:
    """Comb with a fractional, time-varying delay, run block by block."""
    out = np.array(sig, dtype=float, copy=True)
    total = len(sig)
    block = max(1, int(np.floor(delay.min())) - 1)
    for first in range(0, total, block):
        idx = np.arange(first, min(total, first + block))
        back = idx - delay[idx]
        valid = back >= 1
        whole = np.floor(back[valid]).astype(int)
        frac = back[valid] - whole
        out[idx[valid]] += fb * ((1 - frac) * out[whole] + frac * out[whole + 1])
    return out


def _flange(sig: np.ndarray, p: dict) -> np.ndarray:
    """Blend in a feedback comb whose delay creeps up over the tail."""
    blend = p["comb_mix"]
    if blend < 0.01:
        return sig
    tt = cm.times(len(sig))
    delay = p["comb_d0"] / 1000 * SR * (1 + (p["comb_ratio"] - 1) * (1 - cm.decay(tt, 1.2)))
    wet = _feedback_comb(sig, delay, p["comb_fb"]) * (1 - p["comb_fb"])
    return sig + blend * (wet - sig)


def _shell_modes(tt: np.ndarray, lo: float, span: float, tau: float, rng: np.random.Generator) -> np.ndarray:
    """28 random low-mid modes; the higher a mode, the shorter it rings."""
    count = 28
    hi = min(lo * span, 2520.0)
    hz = np.sort(np.exp(rng.uniform(np.log(lo), np.log(hi), count)))
    pos = np.log(hz / lo) / max(np.log(hi / lo), 1e-9)
    falls = tau * np.exp(-1.6 * pos)
    gains = 10 ** (-4 * np.log2(hz / lo) / 20) * rng.uniform(0.3, 1.0, count)
    starts = rng.uniform(0, cm.TWO_PI, count)
    acc = np.zeros_like(tt)
    for f_k, g_k, d_k, s_k in zip(hz, gains, falls, starts):
        acc += g_k * cm.osc(f_k, tt, s_k) * cm.decay(tt, d_k)
    return _unit(acc * (1 - cm.decay(tt, 0.0015)))


def _slow_wander(m: int, rate: float, rng: np.random.Generator) -> np.ndarray:
    warm = max(2, int(np.floor(_BASE_RATE / max(rate, 0.05))))
    return _unit_rms(_lp(_noise(m + warm, rng), rate, 2, _BASE_RATE)[warm:])


def _rumble_roll(n: int, p: dict, rng: np.random.Generator, newer: bool) -> np.ndarray:
    """Low rolling rumble built at the base rate; ``newer`` picks the steeper generation-3 filters."""
    m = int(np.floor(n * _BASE_RATE / SR)) + 8
    tt = np.arange(m) / _BASE_RATE
    order, pink_k, hp_hz, build = (4, 1.4, 26.0, 0.08) if newer else (2, 1.8, 24.0, 0.15)
    cut = p["roll_cut"]
    body = _lp(_noise(m, rng, "brown"), cut, order, _BASE_RATE)
    body = body + 0.5 * _lp(_noise(m, rng, "pink"), pink_k * cut, order, _BASE_RATE)
    body = _hp(body, hp_hz, 2, _BASE_RATE)
    wander = _slow_wander(m, p["roll_rate"], rng)
    hold, fall = p["roll_hold"], p["roll_fall"]
    shape = (1 - cm.decay(tt, build)) * cm.decay(tt, 2.5 * hold)
    shape = np.where(tt > hold, shape * cm.decay(tt - hold, fall), shape)
    depth = p["roll_depth"]
    return _unit(_raise_rate(body * shape * np.exp(depth * wander - 0.5 * depth ** 2), n))


# ---------------------------------------------------------------- generation 2

V2_SPACE, V2_DEFAULTS = cm.spec_rows("""
    pre_len 0.08 1.6 log = 0.7875
    pre_level 0.02 1.0 log = 0.133
    pre_bright 200.0 16000.0 log = 600
    pre_low 0.0 1.0 = 0.5775
    riser_level 0.0 0.6 = 0.0
    riser_f 800.0 12000.0 log = 3063
    crack_level 0.01 0.8 log = 0.72
    crack_ms 0.2 3.0 log = 1.907
    crack_f 600.0 9000.0 log = 1133
    contact_ms 0.15 3.0 log = 0.6308
    crackle 0.0 1.5 = 1.035
    debris_ms 60.0 500.0 log = 66.93
    thump_f0 70.0 400.0 log = 141.9
    thump_f1 22.0 70.0 log = 30.16
    drop_tau 0.006 0.3 log = 0.03454
    thump_tau 0.12 2.0 log = 0.2087
    modes_level 0.0 0.6 = 0.2633
    shell_level 0.0 0.8 = 0.2035
    shell_lo 70.0 250.0 log = 121.7
    shell_span 2.0 10.0 log = 3.57
    shell_tau 0.05 1.2 log = 0.3507
    sub_f 24.0 62.0 log = 60.13
    sub_tau 0.15 3.0 log = 0.8391
    sub_level 0.0 1.0 = 0.0
    rumble_level 0.02 1.2 log = 0.1937
    rumble_cut 80.0 1500.0 log = 103.9
    rumble_tau 0.3 3.5 log = 1.658
    sweep_level 0.005 0.5 log = 0.1123
    sweep_hi 250.0 8000.0 log = 365.9
    sweep_tau 0.08 1.2 log = 0.5692
    echo_level 0.0 0.6 = 0.485
    echo_delay 0.08 0.6 log = 0.1192
    comb_mix 0.0 0.8 = 0.1677
    comb_d0 0.15 2.0 log = 0.2453
    comb_ratio 1.2 6.0 log = 3.23
    comb_fb 0.2 0.85 = 0.7521
    hall_wet 0.05 1.2 log = 0.8017
    hall_rt 0.5 8.0 log = 3.897
    hall_damp 0.0 1.0 = 0.2887
    predelay 0.0 0.06 = 0.0
    drive 0.0 1.0 = 0.3529
    sat_tone 1500.0 14000.0 log = 8003
    width 0.0 1.0 = 0.3363
    floor_db -95.0 -66.0 = -88.67
    roll_level 0.0 1.5 = 0.4072
    roll_cut 60.0 400.0 log = 141.7
    roll_hold 0.8 3.2 = 2.26
    roll_fall 0.25 1.5 log = 0.5904
    roll_rate 0.3 3.0 log = 0.6466
    roll_depth 0.0 0.8 = 0.5458
""")


def _g2_drum(tt: np.ndarray, p: dict, rng: np.random.Generator) -> np.ndarray:
    hi, lo, glide = p["thump_f0"], p["thump_f1"], p["drop_tau"]
    out = _slide(tt, hi, lo, glide, rng) * _swell_fall(tt, 0.0015, p["thump_tau"])
    level = p["modes_level"]
    if level <= 0.005:
        return out
    for j, ratio in enumerate(_DRUM_RATIOS[1:]):
        if hi * ratio > 2700:
            continue
        gain = level * 0.75 ** j * rng.uniform(0.6, 1.0)
        tone = _slide(tt, hi * ratio, lo * ratio, glide, rng)
        out = out + gain * tone * _swell_fall(tt, 0.001, p["thump_tau"] / (1.5 + 0.8 * j))
    return out


def _g2_rumble(tt: np.ndarray, p: dict, rng: np.random.Generator) -> np.ndarray:
    brown = _noise(len(tt), rng, "brown")
    cut = min(p["rumble_cut"], 2520.0)
    bright = _lp(brown, cut, 2, _BASE_RATE)
    dark = _lp(brown, 0.3 * cut, 2, _BASE_RATE)
    blend = cm.decay(tt, max(0.5 * p["rumble_tau"], 0.05))
    body = _hp(blend * bright + 1.4 * (1 - blend) * dark, 22.0, 2, _BASE_RATE)
    wobble = np.ones_like(tt)
    for _ in range(3):
        wobble += 0.12 * cm.osc(rng.uniform(1.5, 6.0), tt, rng.uniform(0, cm.TWO_PI))
    return _unit(body * wobble * _swell_fall(tt, 0.012, p["rumble_tau"]))


def _scatter_hits(m: int, life: float, count: int, spread: float, cut_s: float, rng: np.random.Generator) -> np.ndarray:
    """Random signed clicks at exponential times, each scaled by its own age."""
    when = rng.exponential(spread, count)
    sign = rng.standard_normal(count)
    alive = when < cut_s
    out = np.zeros(m)
    np.add.at(out, np.minimum((when[alive] * SR).astype(int), m - 1), sign[alive] * np.exp(-when[alive] / life))
    return out


def _g2_debris(m: int, p: dict, rng: np.random.Generator) -> np.ndarray:
    life = p["debris_ms"] / 1000
    hits = _scatter_hits(m, life, int(np.floor(60 + 3000 * life)), 0.5 * life, 0.58, rng)
    centre = p["crack_f"]
    return _unit(_bp(hits, 0.45 * centre, min(4 * centre, 21600.0)))


def _g2_crack(n: int, p: dict, rng: np.random.Generator) -> np.ndarray:
    m = min(n, _samples(0.6))
    tt = cm.times(m)
    touch = min(m, max(2, _samples(p["contact_ms"] / 1000)))
    out = np.zeros(m)
    out[:touch] += 0.5 - 0.5 * np.cos(np.linspace(0, cm.TWO_PI, touch))
    out += 0.8 * cm.osc(p["crack_f"], tt) * cm.decay(tt, p["crack_ms"] / 1000) * (1 - cm.decay(tt, 0.0002))
    out = _hp(out, 30.0, 1)
    if p["crackle"] > 0.01:
        out = out + p["crackle"] * _g2_debris(m, p, rng)
    return _to_size(_unit(out), n)


def _g2_sweep(n: int, p: dict, rng: np.random.Generator) -> np.ndarray:
    tt = cm.times(n)
    fall = p["sweep_tau"]
    centre = 70 + (p["sweep_hi"] - 70) * cm.decay(tt, fall)
    return _unit(dsp.svf(_noise(n, rng, "pink"), centre, 2.5, "band") * _swell_fall(tt, 0.004, 1.8 * fall))


def _air_rise(n: int, f_end: float, rng: np.random.Generator) -> np.ndarray:
    pos = np.arange(n) / max(n - 1, 1)
    centre = 150 * (f_end / 150) ** (pos ** 1.5)
    band = dsp.svf(_noise(n, rng, "pink"), centre, 1.2, "band")
    return _unit(band * (np.exp(3.5 * pos) - 1) / (np.exp(3.5) - 1))


def _repeats(dry: np.ndarray, p: dict, rng: np.random.Generator) -> np.ndarray:
    """Up to 3 darker, quieter copies of the dry hit."""
    out = np.zeros_like(dry)
    if p["echo_level"] < 0.01:
        return out
    lag = 0.0
    for k in range(3):
        lag += p["echo_delay"] * rng.uniform(0.8, 1.3)
        start = _samples(lag)
        if start >= len(dry):
            break
        out[start:] += p["echo_level"] * 0.6 ** k * _lp(dry, 4000 * 0.5 ** k, 2)[: len(dry) - start]
    return out


def _g2_side(ch: int, parts: dict, p: dict, rng: np.random.Generator) -> tuple[np.ndarray, np.ndarray]:
    spread = p["width"]
    rumble, sweep = parts["rumble"], parts["sweep"]
    if spread > 0.01:
        rumble = (1 - spread) * rumble + spread * _raise_rate(_g2_rumble(parts["tt"], p, rng), parts["n"])
        sweep = (1 - spread) * sweep + spread * _g2_sweep(parts["n"], p, rng)
    dry = parts["direct"] + p["rumble_level"] * rumble + p["sweep_level"] * sweep
    dry = dsp.fade(dry + _repeats(dry, p, rng), 0.0, 0.4)
    wet = _flange(signal.fftconvolve(dry, parts["halls"][ch]), p)
    return dry, wet


def _g2_reverse_swell(dst: np.ndarray, head: np.ndarray, top: float, p: dict, rng: np.random.Generator) -> None:
    """Fill the lead-in of one channel with a reversed hall swell and, when set, the air riser."""
    lead_s, lead_n = p["pre_len"], len(dst)
    resp = hall_ir(max(0.3, 1.6 * lead_s), 0.4, 0.6, 0.0, seconds=1.2 * lead_s + 0.25, rng=rng)
    back = _lp(_to_size(signal.fftconvolve(head, resp), lead_n)[::-1], p["pre_bright"], 2)
    back = dsp.fade(back, min(0.25 * lead_s, 0.1), 0.003)
    dst += back * p["pre_level"] * top / (np.max(np.abs(back)) + 1e-12)
    if p["riser_level"] > 0.005 and lead_n > 64:
        dst += p["riser_level"] * top * dsp.fade(_air_rise(lead_n, p["riser_f"], rng), 0.01, 0.002)


def _g2_lead_in(out: np.ndarray, parts: dict, p: dict, rng: np.random.Generator) -> None:
    span = _samples(0.2)
    crack = parts["crack"]
    head = crack[:span] / (np.max(np.abs(crack)) + 1e-12) + p["pre_low"] * _unit(parts["low"])[:span]
    top = np.max(np.abs(out))
    for ch in (0, 1):
        _g2_reverse_swell(out[:parts["lead"], ch], head, top, p, rng)


def _g2_tail_roll(out: np.ndarray, lead: int, p: dict, rng: np.random.Generator) -> None:
    if p["roll_level"] <= 0.005:
        return
    top = np.max(np.abs(out))
    span = min(len(out) - lead, _samples(p["roll_hold"] + 6 * p["roll_fall"] + 0.5))
    shared = _rumble_roll(span, p, rng, newer=False)
    spread = p["width"]
    for ch in (0, 1):
        roll = (1 - spread) * shared + spread * _rumble_roll(span, p, rng, newer=False) if spread > 0.01 else shared
        _mix_in(out[:, ch], p["roll_level"] * top * dsp.fade(roll, 0.0, 0.3), lead)


def _g2_low(tt: np.ndarray, n: int, p: dict, rng: np.random.Generator) -> np.ndarray:
    """Drum, sub sine and shell at the base rate, then upsampled and driven."""
    low = _g2_drum(tt, p, rng)
    low = low + p["sub_level"] * cm.osc(p["sub_f"], tt, rng.uniform(0, 1)) * _swell_fall(tt, 0.02, p["sub_tau"])
    low = low + p["shell_level"] * _shell_modes(tt, p["shell_lo"], p["shell_span"], p["shell_tau"], rng)
    return _desk_drive(_raise_rate(low, n), p["drive"], p["sat_tone"])


def _g2_parts(p: dict, rng: np.random.Generator) -> dict:
    n = _samples(_GEN2_LEN_S)
    tt = np.arange(int(_BASE_RATE * _GEN2_LEN_S)) / _BASE_RATE
    low = _g2_low(tt, n, p, rng)
    crack = _g2_crack(n, p, rng)
    parts = {"n": n, "tt": tt, "low": low, "crack": crack, "direct": low + p["crack_level"] * crack}
    parts["rumble"] = _raise_rate(_g2_rumble(tt, p, rng), n)
    parts["sweep"] = _g2_sweep(n, p, rng)
    rt, damp, pre = p["hall_rt"], p["hall_damp"], p["predelay"]
    first = hall_ir(rt, damp, 0.8, pre, rng=rng)
    second = hall_ir(rt, damp, 0.8, pre + rng.uniform(0.001, 0.006), rng=rng)
    parts["halls"] = (first, second)
    return parts


def render_v2(p: dict, rng: np.random.Generator) -> np.ndarray:
    """Generation 2, the physical trailer boom. The hit lands after ``pre_len`` seconds."""
    p = {**V2_DEFAULTS, **p}
    parts = _g2_parts(p, rng)
    lead = _samples(p["pre_len"])
    parts["lead"] = lead
    out = np.zeros((lead + parts["n"] + max(len(h) for h in parts["halls"]), 2))
    for ch in (0, 1):
        dry, wet = _g2_side(ch, parts, p, rng)
        _mix_in(out[:, ch], dry, lead)
        _mix_in(out[:, ch], p["hall_wet"] * wet, lead)
    _g2_lead_in(out, parts, p, rng)
    _g2_tail_roll(out, lead, p, rng)
    out = dsp.trim_tail(_mic(out, rng, p["floor_db"]), -66.0, 0.05)
    return dsp.fade(out, 0.002, 0.25)


# ---------------------------------------------------------------- the air riser

_SWELL_OPTS: dict[str, float] = cm.value_table("""
    f_start 150  f_end 6000  q 1.2  curve 1.5  grow 3.5  width 0.6  room_wet 0.25
    tube 0.5  tube_lo 90  tube_hi 600  flutter 0.25  rev 0.5
""")


def _pipe(noise: np.ndarray, pos: np.ndarray, lo: float, hi: float, curve: float) -> np.ndarray:
    """Six harmonic notch-band resonances that track a rising pitch."""
    pitch = lo * (hi / lo) ** (pos ** curve)
    acc = np.zeros_like(noise)
    for k in range(1, 7):
        acc += dsp.svf(noise, np.minimum(k * pitch, 19200.0), 9.0, "bandn") / k
    return _unit_rms(acc)


def _flutter(n: int, rng: np.random.Generator) -> np.ndarray:
    warm = SR // 12
    return _unit_rms(_lp(rng.standard_normal(n + warm), 12.0, 2)[warm:])


def _riser_layer(n: int, pos: np.ndarray, rng: np.random.Generator, opt: dict) -> np.ndarray:
    pink = _noise(n, rng, "pink")
    start, end, curve = opt["f_start"], opt["f_end"], opt["curve"]
    out = _unit_rms(dsp.svf(pink, start * (end / start) ** (pos ** curve), opt["q"], "band"))
    if opt["tube"] > 0:
        out = out + opt["tube"] * _pipe(pink, pos, opt["tube_lo"], opt["tube_hi"], curve)
    depth = opt["flutter"]
    if depth > 0:
        out = out * np.exp(depth * _flutter(n, rng) - 0.5 * depth ** 2)
    grow = opt["grow"]
    return out * (np.exp(grow * pos) - 1) / (np.exp(grow) - 1)


def _reverse_wash(n: int, pos: np.ndarray, seconds: float, f_end: float, rng: np.random.Generator) -> np.ndarray:
    tap = np.zeros(_samples(0.02))
    burst = _samples(0.004)
    tap[:burst] = _noise(burst, rng)
    resp = hall_ir(1.4 * seconds, 0.45, 0.7, 0.0, seconds=seconds + 0.1, rng=rng)
    backwards = _to_size(signal.fftconvolve(tap, resp), n)[::-1]
    return _unit(_lp(backwards, 0.8 * f_end, 2)) * (0.3 + 0.7 * pos)


def _riser_pair(n: int, pos: np.ndarray, seconds: float, opt: dict, rng: np.random.Generator) -> np.ndarray:
    common = _riser_layer(n, pos, rng, opt)
    spread = opt["width"]
    sides = []
    for _side in (0, 1):
        one = _unit((1 - spread) * common + spread * _riser_layer(n, pos, rng, opt))
        if opt["rev"] > 0:
            one = one + opt["rev"] * _reverse_wash(n, pos, seconds, opt["f_end"], rng)
        sides.append(one)
    return np.column_stack(sides)


def swell(seconds: float, rng: np.random.Generator, **opts: float) -> np.ndarray:
    """Air riser on its own, loudest at its last sample. Stereo with peak 1.

    Options (with defaults in ``_SWELL_OPTS``): f_start, f_end, q, curve, grow,
    width, room_wet, tube, tube_lo, tube_hi, flutter, rev.
    """
    extra = set(opts) - set(_SWELL_OPTS)
    if extra:
        raise TypeError(f"swell: unknown options {sorted(extra)}")
    opt = {**_SWELL_OPTS, **opts}
    n = _samples(seconds)
    pos = np.arange(n) / max(n - 1, 1)
    out = _riser_pair(n, pos, seconds, opt, rng)
    resp = hall_ir(0.9, 0.5, 0.6, 0.004, rng=rng)
    room = np.column_stack([signal.fftconvolve(out[:, ch], resp)[:n] for ch in (0, 1)])
    out = out + opt["room_wet"] * room / np.sqrt(np.sum(resp ** 2))
    return _unit(dsp.fade(_mic(out, rng, -80.0), 0.03, 0.006))


# ---------------------------------------------------------------- generation 3

SPACE, DEFAULTS = cm.spec_rows("""
    pre 0.0 1.2 = 0.35
    pre_level 0.02 0.7 log = 0.12
    crack 0.03 2.0 log = 0.35
    crack_f 900.0 7000.0 log = 2200
    crack_ms 2.0 18.0 log = 6.0
    snap 0.0 1.5 = 0.0
    sheen 0.0 2.0 = 0.0
    sheen_ms 4.0 16.0 log = 9.0
    sheen_f 4500.0 9000.0 log = 6000
    pre_shape | 0 1 = 0
    debris 0.0 1.0 = 0.4
    blast 0.0 1.0 = 0.5
    blast_f 250.0 3000.0 log = 900
    blast_tau 0.03 0.8 log = 0.25
    body_f0 70.0 230.0 log = 140
    body_f1 34.0 75.0 log = 46
    drop_tau 0.015 0.14 log = 0.05
    body_tau 0.08 0.7 log = 0.32
    body_attack 0.001 0.3 log = 0.004
    modes 0.0 0.6 = 0.25
    body 0.1 1.2 log = 1.0
    drive 0.0 1.0 = 0.35
    sub 0.0 1.2 = 0.7
    sub_f 28.0 60.0 log = 40
    sub_semis 0.0 24.0 = 12.0
    sub_tau 0.2 2.2 log = 1.1
    shell 0.0 0.8 = 0.3
    shell_lo 90.0 260.0 log = 130
    roll 0.0 1.5 = 0.6
    roll_cut 55.0 320.0 log = 140
    roll_hold 0.2 3.0 = 1.6
    roll_fall 0.12 1.6 log = 0.6
    roll_rate 0.3 3.0 log = 0.7
    roll_depth 0.0 0.8 = 0.45
    grain_rate 18.0 48.0 log = 30.0
    grain_depth 0.0 1.0 = 0.0
    air 0.0 1.0 = 0.0
    air_f 2500.0 11000.0 log = 7000
    air_div 0.25 1.0 = 1.0
    grain_sharp 2.0 6.0 = 3.0
    air_q 0.5 3.0 = 0.8
    hall_wet 0.03 1.0 log = 0.45
    hall_rt 0.4 6.0 log = 3.2
    hall_damp 0.0 1.0 = 0.4
    hall_tone 3000.0 12000.0 log = 5000
    width 0.0 1.0 = 0.55
    gate 0.0 1.6 = 0.0
    gate_ms 6.0 40.0 = 20.0
    gate_fall 0.08 0.6 log = 0.25
    gate_db 0.0 30.0 = 0.0
    floor_db -110.0 -70.0 = -90.0
    sheen_top 7000.0 20000.0 log = 20000
    top 6000.0 20000.0 log = 20000
    plate 0.0 1.0 = 0.0
    plate_f 150.0 900.0 log = 400
    plate_tau 0.2 2.0 log = 0.8
    scatter 0.0 1.0 = 0.0
    scatter_s 0.3 2.5 = 1.2
    grain_jit 0.0 1.0 = 0.0
    boom 0.0 2.5 = 0.0
    boom_f0 80.0 260.0 log = 150
    boom_f1 34.0 70.0 log = 48
    boom_drop 0.015 0.15 log = 0.045
    boom_tau 0.08 0.9 log = 0.3
    boom_att 0.002 0.12 log = 0.01
    boom_drive 0.0 1.0 = 0.3
    roll_in 0.0 0.4 = 0.0
""")


def _g3_debris(m: int, p: dict, rng: np.random.Generator) -> np.ndarray:
    life = 0.04 + 0.2 * p["debris"]
    hits = _scatter_hits(m, life, int(np.floor(80 + 2500 * life)), 0.45 * life, 0.38, rng)
    centre = p["crack_f"]
    return _unit(_bp(hits, 0.35 * centre, min(4 * centre, 21600.0)))


def _crack_burst(size: int, p: dict, rng: np.random.Generator) -> np.ndarray:
    idx = np.arange(size)
    burst = rng.standard_normal(size) * (1 - idx / size) ** 3 * (1 - np.exp(-idx / (SR * 0.0003)))
    centre = p["crack_f"]
    return _bp(burst, 0.5 * centre, min(3 * centre, 21600.0))


def _g3_crack(n: int, p: dict, rng: np.random.Generator) -> np.ndarray:
    m = min(n, _samples(0.4))
    out = np.zeros(m)
    touch = min(m, max(2, _samples(0.0006)))
    out[:touch] += 0.5 - 0.5 * np.cos(np.linspace(0, cm.TWO_PI, touch))
    burst_n = min(m, max(8, _samples(p["crack_ms"] / 1000)))
    out[:burst_n] += 1.4 * _unit(_crack_burst(burst_n, p, rng))
    if p["debris"] > 0.01:
        out = out + 0.5 * p["debris"] * _g3_debris(m, p, rng)
    return _to_size(_unit(_hp(out, 40.0, 1)), n)


def _g3_snap(n: int, rng: np.random.Generator) -> np.ndarray:
    size = min(n, _samples(0.0015))
    pos = np.arange(size) / size
    return _to_size(_unit(_hp(rng.standard_normal(size) * (1 - pos) ** 4, 5000.0, 2)), n)


def _g3_sheen(n: int, p: dict, rng: np.random.Generator) -> np.ndarray:
    size = min(n, max(16, _samples(p["sheen_ms"] / 1000)))
    idx = np.arange(size)
    shape = (1 - idx / size) ** 2 * (1 - np.exp(-idx / (SR * 0.0002)))
    common = rng.standard_normal(size)
    sides = []
    for _side in (0, 1):
        one = _hp((0.7 * common + 0.3 * rng.standard_normal(size)) * shape, p["sheen_f"], 4)
        if p["sheen_top"] < 19000:
            one = _lp(one, p["sheen_top"], 2)
        sides.append(one)
    return _to_size(_unit(np.column_stack(sides)), n)


def _plate_modes(p: dict, rng: np.random.Generator) -> tuple[np.ndarray, ...]:
    """Draw 40 plate modes: frequency, level, decay and a start phase per channel."""
    base = p["plate_f"]
    hz = np.sort(base * np.exp(rng.uniform(0, np.log(min(16.0, 9000.0 / base)), 40)))
    gains = (hz / base) ** -0.6 * rng.uniform(0.3, 1.0, 40)
    falls = p["plate_tau"] * (hz / base) ** -0.55
    starts = rng.uniform(0, cm.TWO_PI, (40, 2))
    return hz, gains, falls, starts


def _g3_plate(n: int, p: dict, rng: np.random.Generator) -> np.ndarray:
    """40 plate modes with a slight pitch sag; each channel gets its own phases."""
    m = min(n, _samples(min(4.0, 5 * p["plate_tau"] + 0.1)))
    hz, gains, falls, starts = _plate_modes(p, rng)
    tt = cm.times(m)
    bent = tt + 0.0012 * (1 - cm.decay(tt, 0.1))
    onset = 1 - cm.decay(tt, 0.0008)
    out = np.zeros((m, 2))
    for k in range(40):
        level = gains[k] * cm.decay(tt, falls[k]) * onset
        arg = cm.TWO_PI * hz[k] * bent
        for ch in (0, 1):
            out[:, ch] += level * np.sin(arg + starts[k, ch])
    return _to_size(_unit(out), n)


def _gravel_side(size: int, span_s: float, rng: np.random.Generator) -> np.ndarray:
    count = int(np.floor(60 + 260 * span_s))
    when = 0.03 + rng.exponential(0.4 * span_s, count)
    sign = rng.standard_normal(count)
    level = rng.uniform(0.2, 1.0, count) ** 2
    alive = when < span_s
    hits = np.zeros(size)
    np.add.at(hits, np.minimum((when[alive] * SR).astype(int), size - 1),
              (sign * np.exp(-when / (0.5 * span_s)) * level)[alive])
    grain_n = _samples(0.004)
    grain = rng.standard_normal(grain_n) * np.exp(-np.arange(grain_n) / (SR * 0.0012))
    out = signal.fftconvolve(hits, grain)[:size]
    return _unit(_bp(out, 700.0, 6000.0)) + 0.6 * _unit(_bp(out, 250.0, 1400.0))


def _g3_scatter(n: int, p: dict, rng: np.random.Generator) -> np.ndarray:
    span_s = p["scatter_s"]
    size = min(n, _samples(span_s + 0.25))
    return _to_size(_unit(np.column_stack([_gravel_side(size, span_s, rng) for _ in range(2)])), n)


def _g3_overtones(out: np.ndarray, tt: np.ndarray, late: np.ndarray, p: dict, rng: np.random.Generator) -> np.ndarray:
    """Add the drum overtones; they glide with the body and die faster."""
    for j, ratio in enumerate(_DRUM_RATIOS[1:]):
        gain = p["modes"] * 0.75 ** j * rng.uniform(0.6, 1.0)
        tone = _slide(tt, p["body_f0"] * ratio, p["body_f1"] * ratio, p["drop_tau"], rng)
        out = out + gain * tone * cm.decay(late, p["body_tau"] / (1.5 + 0.8 * j))
    return out


def _g3_body(tt: np.ndarray, p: dict, rng: np.random.Generator) -> np.ndarray:
    attack = p["body_attack"]
    late = np.maximum(tt - attack, 0)
    out = _slide(tt, p["body_f0"], p["body_f1"], p["drop_tau"], rng) * cm.decay(late, p["body_tau"])
    if p["modes"] > 0.005:
        out = _g3_overtones(out, tt, late, p, rng)
    out = out * _fade_in(tt, attack)
    drive = p["drive"]
    if drive > 0.01:
        push = 1 + 6 * drive
        top = np.max(np.abs(out)) + 1e-12
        out = top * np.tanh(push * out / top) / np.tanh(push)
        out = _lp(out, min(2700.0, 900 + 2500 * (1 - drive)), 2, _BASE_RATE)
    return _unit(out)


def _g3_sub(tt: np.ndarray, p: dict, rng: np.random.Generator) -> np.ndarray:
    end_hz = p["sub_f"]
    attack = max(p["body_attack"], 0.008)
    tone = _slide(tt, end_hz * 2 ** (p["sub_semis"] / 12), end_hz, 0.09, rng) * _fade_in(tt, attack)
    return tone * cm.decay(np.maximum(tt - attack, 0), p["sub_tau"])


def _one_grain(curve: np.ndarray, at: float, step: float, jit: float, rng: np.random.Generator) -> None:
    start = max(0, int(np.floor((at + rng.uniform(-0.12, 0.12) * step) * SR)))
    size = min(len(curve) - start, int(np.floor(1.6 * step * SR)))
    if size <= 8:
        return
    age = np.arange(size) / SR
    level = rng.uniform(0.55 - 0.4 * jit, 1.0)
    curve[start:start + size] += level * (1 - cm.decay(age, 0.0025)) * cm.decay(age, 0.32 * step)


def _grain_gain(n: int, rate: float, depth: float, rng: np.random.Generator, slow: float = 0.18,
                jit: float = 0.0) -> np.ndarray:
    """Gain curve near 1 that pulses in grains at ``rate`` per second, slowing over time."""
    if depth <= 0.01:
        return np.ones(n)
    curve = np.zeros(n)
    now, last = 0.0, (n - 1) / SR
    while now < last:
        step = (1 + slow * min(now / 1.2, 1.5)) / rate
        _one_grain(curve, now, step, jit, rng)
        if jit > 0:
            now += step * (1 + jit * rng.uniform(-0.6, 0.9))
        else:
            now += step
    curve = curve / (np.percentile(curve, 95) + 1e-9)
    return (1 - depth) + depth * np.clip(curve, 0, 1.6)


def _g3_roll(n: int, p: dict, rng: np.random.Generator) -> np.ndarray:
    roll = _rumble_roll(n, p, rng, newer=True)
    return _unit(roll * _grain_gain(n, p["grain_rate"], p["grain_depth"], rng, jit=p["grain_jit"]))


def _g3_air(n: int, attack: float, p: dict, rng: np.random.Generator) -> np.ndarray:
    tt = cm.times(n)
    band = dsp.svf(_noise(n, rng, "pink"), p["air_f"], p["air_q"], "bandn")
    hold = max(0.25, 0.5 * p["roll_hold"] + p["roll_fall"])
    shape = _fade_in(tt, max(attack, 0.02)) * cm.decay(np.maximum(tt - attack, 0), hold)
    depth = p["grain_depth"]
    if depth > 0.01:
        pulses = _grain_gain(n, p["grain_rate"] * p["air_div"], min(1.0, 1.5 * depth), rng, jit=p["grain_jit"])
        shape = shape * pulses ** p["grain_sharp"]
    return _unit(band * shape)


def _g3_boom(tt: np.ndarray, p: dict) -> np.ndarray:
    glide = max(p["boom_drop"], 0.001)
    lo, hi, attack = p["boom_f1"], p["boom_f0"], p["boom_att"]
    arg = cm.TWO_PI * (lo * tt + (hi - lo) * glide * (1 - cm.decay(tt, glide)))
    out = np.sin(arg) * _fade_in(tt, attack) * cm.decay(np.maximum(tt - attack, 0), p["boom_tau"])
    if p["boom_drive"] > 0.01:
        push = 1 + 8 * p["boom_drive"]
        out = _lp(np.tanh(push * out) / np.tanh(push), 1100.0, 2, _BASE_RATE)
    return _unit(out)


def _g3_blast(n: int, p: dict, rng: np.random.Generator) -> np.ndarray:
    tt = cm.times(n)
    fall = p["blast_tau"]
    centre = p["blast_f"] * (0.5 + 0.5 * cm.decay(tt, max(fall, 0.02)))
    band = dsp.svf(_noise(n, rng, "pink"), centre, 0.55, "bandn")
    return _unit(band * (1 - cm.decay(tt, 0.002)) * cm.decay(tt, fall))


def _gate_shape(n: int, at: float, ms: float) -> np.ndarray:
    """Ones until ``at`` seconds, a raised-cosine close over ``ms``, then zeros."""
    shape = np.ones(n)
    start = _samples(at)
    if start >= n:
        return shape
    width = max(8, _samples(ms / 1000))
    close = 0.5 + 0.5 * np.cos(np.linspace(0, np.pi, width))
    span = min(width, n - start)
    shape[start:start + span] = close[:span]
    shape[start + span:] = 0.0
    return shape


def _g3_seconds(p: dict) -> float:
    ring = p["roll_hold"] + 5 * p["roll_fall"] + 0.4
    length = min(7.0, max(ring, 3 * p["sub_tau"] + 0.3, 1.2 * p["hall_rt"] + 0.3))
    if p["gate"] > 0:
        length = min(length, p["gate"] + p["gate_ms"] / 1000 + 0.01)
    return length


def _g3_direct(parts: dict, p: dict, rng: np.random.Generator) -> None:
    n, tt = parts["n"], parts["tt"]
    parts["body"] = _raise_rate(_g3_body(tt, p, rng), n)
    parts["sub"] = _raise_rate(_g3_sub(tt, p, rng), n)
    shell = np.zeros(n)
    if p["shell"] > 0.01:
        shell = _raise_rate(_shell_modes(tt, p["shell_lo"], 4.5, 0.3, rng) * _fade_in(tt, p["body_attack"]), n)
    parts["crack"] = _g3_crack(n, p, rng)
    direct = p["body"] * parts["body"] + 0.6 * p["shell"] * shell + p["crack"] * parts["crack"]
    if p["boom"] > 0.005:
        direct = direct + p["boom"] * _raise_rate(_g3_boom(tt, p), n)
    if p["snap"] > 0.005:
        direct = direct + p["snap"] * _g3_snap(n, rng)
    parts["direct"] = direct


def _g3_layers(parts: dict, p: dict, rng: np.random.Generator) -> None:
    n = parts["n"]
    parts["sheen"] = _g3_sheen(n, p, rng) if p["sheen"] > 0.005 else None
    parts["plate"] = _g3_plate(n, p, rng) if p["plate"] > 0.005 else None
    parts["scatter"] = _g3_scatter(n, p, rng) if p["scatter"] > 0.005 else None
    parts["roll"] = _g3_roll(n, p, rng) if p["roll"] > 0.01 else None
    parts["air"] = _g3_air(n, p["body_attack"], p, rng) if p["air"] > 0.005 else None
    parts["blast"] = _g3_blast(n, p, rng) if p["blast"] > 0.01 else None
    parts["halls"] = None
    if p["hall_wet"] > 0.031:
        first = hall_ir(p["hall_rt"], p["hall_damp"], 0.8, 0.012, rng=rng)
        second = hall_ir(p["hall_rt"], p["hall_damp"], 0.8, 0.012 + rng.uniform(0.001, 0.005), rng=rng)
        parts["halls"] = (first, second)


def _widen(common: np.ndarray | None, fresh: Callable[[], np.ndarray], spread: float) -> np.ndarray | None:
    """Blend a shared layer with a fresh take of it; no fresh take is made for a narrow image."""
    if common is None:
        return None
    if spread > 0.01:
        return (1 - spread) * common + spread * fresh()
    return common


def _g3_dry(ch: int, parts: dict, p: dict, rng: np.random.Generator) -> np.ndarray:
    n, spread = parts["n"], p["width"]
    dry = np.array(parts["direct"], copy=True)
    for name in ("sheen", "plate"):
        if parts[name] is not None:
            dry += p[name] * parts[name][:, ch]
    if parts["scatter"] is not None:
        dry += p["scatter"] * _LAYER_GAIN * parts["scatter"][:, ch]
    roll = _widen(parts["roll"], lambda: _g3_roll(n, p, rng), spread)
    if roll is not None:
        if p["roll_in"] > 0.005:
            roll = roll * _fade_in(cm.times(n), p["roll_in"])
        dry += p["roll"] * _LAYER_GAIN * roll
    air = _widen(parts["air"], lambda: _g3_air(n, p["body_attack"], p, rng), spread)
    if air is not None:
        dry += p["air"] * _LAYER_GAIN * air
    blast = _widen(parts["blast"], lambda: _g3_blast(n, p, rng), spread)
    if blast is not None:
        dry += p["blast"] * _LAYER_GAIN * blast
    return dry


def _g3_side(out: np.ndarray, ch: int, parts: dict, p: dict, rng: np.random.Generator) -> None:
    lead = parts["lead"]
    dry = _g3_dry(ch, parts, p, rng)
    main = dry + p["sub"] * _LAYER_GAIN * parts["sub"]
    if p["gate"] <= 0:
        main = dsp.fade(main, 0.0, 0.3 * parts["seconds"])
    _mix_in(out[:, ch], main, lead)
    if parts["halls"] is None:
        return
    send = parts["direct"] + 0.5 * (dry - parts["direct"])
    send = _hp(_lp(send, p["hall_tone"], 1), 110.0, 2)
    _mix_in(out[:, ch], p["hall_wet"] * signal.fftconvolve(send, parts["halls"][ch]), lead)


def _even_db_rise(back: np.ndarray) -> np.ndarray:
    """Reshape a reversed swell so its level climbs 34 dB along a fixed curve."""
    level = np.sqrt(np.convolve(back ** 2, np.ones(480) / 480, mode="same") + 1e-9)
    pos = np.linspace(0, 1, len(back))
    back = back * 10 ** ((-34 + 34 * pos ** 1.3) / 20) / (level / np.max(level))
    return _lp(back, 4000.0, 2)


def _g3_lead_in(out: np.ndarray, parts: dict, p: dict, rng: np.random.Generator) -> None:
    lead = parts["lead"]
    if lead <= 64:
        return
    span = _samples(0.15)
    head = parts["crack"][:span] + 0.6 * parts["body"][:span]
    top = np.max(np.abs(out))
    lead_s = p["pre"]
    for ch in (0, 1):
        resp = hall_ir(max(0.4, 1.8 * lead_s), 0.4, 0.6, 0.0, seconds=1.2 * lead_s + 0.2, rng=rng)
        back = _lp(_to_size(signal.fftconvolve(head, resp), lead)[::-1], 2500.0, 2)
        if int(p["pre_shape"]) == 1:
            back = _even_db_rise(back)
        else:
            back = back * np.linspace(0, 1, lead) ** 1.5
        back = dsp.fade(back, 0.01, 0.002)
        out[:lead, ch] += back * p["pre_level"] * top / (np.max(np.abs(back)) + 1e-12)


def _g3_gate(st: np.ndarray, lead: int, p: dict) -> np.ndarray:
    size = len(st) - lead
    tt = cm.times(size)
    shape = _gate_shape(size, p["gate"], p["gate_ms"])
    if p["gate_db"] > 0.1:
        ramp = max(0.02, min(p["gate_fall"], 0.8 * p["gate"]))
        shape = shape * 10 ** (-p["gate_db"] * np.clip((tt - (p["gate"] - ramp)) / ramp, 0, 1) / 20)
    st = np.array(st, copy=True)
    st[lead:] *= shape[:, None]
    return dsp.fade(np.concatenate([st, np.zeros((_samples(0.02), 2))]), 0.001, 0.0)


def _g3_master(out: np.ndarray, parts: dict, p: dict, rng: np.random.Generator) -> np.ndarray:
    st = _hp(out, 24.0, 2)
    if p["top"] < 19000:
        st = _lp(st, p["top"], 2)
    if p["floor_db"] > -105:
        st = _mic(st, rng, p["floor_db"], 22.0, -0.5)
    if p["gate"] > 0:
        return _g3_gate(st, parts["lead"], p)
    st = dsp.trim_tail(st, -70.0, 0.05)
    return dsp.fade(st, 0.001, min(0.35, 0.1 * len(st) / SR))


def render_v3(p: dict, rng: np.random.Generator) -> np.ndarray:
    """Generation 3, the cinematic boom and the designed hit. Stereo, peak 0.89."""
    p = {**DEFAULTS, **p}
    seconds = _g3_seconds(p)
    n = _samples(seconds)
    parts: dict[str, Any] = {"seconds": seconds, "n": n,
                             "tt": np.arange(int(np.floor(_BASE_RATE * seconds)) + 8) / _BASE_RATE}
    _g3_direct(parts, p, rng)
    _g3_layers(parts, p, rng)
    parts["lead"] = _samples(p["pre"]) if p["pre"] > 0.02 else 0
    halls = parts["halls"]
    hall_n = max(len(h) for h in halls) if halls is not None and p["gate"] <= 0 else 0
    out = np.zeros((parts["lead"] + n + hall_n, 2))
    for ch in (0, 1):
        _g3_side(out, ch, parts, p, rng)
    _g3_lead_in(out, parts, p, rng)
    st = _g3_master(out, parts, p, rng)
    return st * _OUT_PEAK / (np.max(np.abs(st)) + 1e-12)


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """Render one take: generation 2 when ``pre_len`` is present, otherwise generation 3."""
    return render_v2(p, rng) if "pre_len" in p else render_v3(p, rng)


# ---------------------------------------------------------------- families

def _stack(*layers: dict | str) -> dict:
    """Merge parameter sets left to right; a string layer is read as ``name value`` pairs."""
    out: dict = {}
    for layer in layers:
        out.update(cm.value_table(layer) if isinstance(layer, str) else layer)
    return out


def _members(base: dict, edits: dict[str, str]) -> list[tuple[str, dict]]:
    return [(name, _stack(base, edit)) for name, edit in edits.items()]


_PRO_BASE = _stack("""
    pre 0  pre_level 0.1  crack 0.3  crack_f 3000  crack_ms 4  debris 0.1  blast 0.05  blast_f 300
    blast_tau 0.15  body_f0 120  body_f1 75  drop_tau 0.05  body_tau 0.18  body_attack 0.1  modes 0.1
    body 0.35  drive 0.15  sub 0.15  sub_f 48  sub_semis 7  sub_tau 0.5  shell 0.1  shell_lo 110  roll 1.5
    roll_cut 200  roll_hold 0.7  roll_fall 0.5  roll_rate 1.5  roll_depth 0.4  grain_rate 36
    grain_depth 1.0  air 0.2  air_f 4500  hall_wet 0.04  hall_rt 0.6  hall_damp 0.4  width 1.0  gate 1.1
    gate_ms 15  floor_db -110
""")
_CINE_BASE = _stack(DEFAULTS, """
    sub 0.35  sub_f 42  sub_tau 0.7  body_f0 160  body_f1 62  body_tau 0.25  shell 0.8
    shell_lo 110  crack 0.8  crack_f 1500  crack_ms 10  debris 0.7  blast 0.8  blast_f 600  blast_tau 0.3
    hall_wet 0.8  roll 0.45  roll_cut 200  pre 0  snap 0.6  hall_tone 9000  roll_depth 0.22  roll_rate 0.5
""")
_CINE_BRIGHT = _stack(_CINE_BASE, """
    blast 1.0  blast_f 1200  sub 0.2  body 0.7  crack 1.4  crack_f 2500  roll 0.35  sheen 1.4
    sheen_ms 7  sheen_f 6000
""")
_PRO_AIRY = _stack(_PRO_BASE, """
    air 0.7  air_f 8500  air_q 1.6  gate_db 15  gate_fall 0.25  gate_ms 8  air_div 0.5  grain_sharp 4.0
""")
_CINE_SOFT = _stack(_CINE_BRIGHT, "sheen 0.3  sheen_ms 14  sheen_top 11000  top 13000  crack_f 1600  crack 0.95")
_PRO_JITTER = _stack(DEFAULTS, _PRO_AIRY, "grain_jit 0.7  crack_f 1800  snap 0  sheen 0")

_IMPACT_V2 = _members(_CINE_BASE, {
    "classic": "",
    "deep": """body_f0 110  body_f1 44  sub 0.6  sub_f 34  sub_tau 1.0  roll 0.6  roll_cut 140  roll_hold 2.4
               crack 0.4  crack_f 1100  blast_f 400  blast_tau 0.45  hall_rt 4.5  hall_damp 0.6""",
    "punch": """body_f0 210  body_f1 58  drop_tau 0.03  body_tau 0.18  crack 1.0  crack_f 2600  crack_ms 7
                blast_tau 0.15  roll_hold 0.9  roll_fall 0.4  hall_rt 2.0  hall_wet 0.6  sub_tau 0.45""",
    "metal": """shell 0.8  shell_lo 210  modes 0.5  crack_f 4000  debris 1.0  air 0.08  air_f 6500
                hall_damp 0.1  hall_rt 3.6  blast_f 1200""",
    "swell": "pre 0.9  pre_level 0.35  body_attack 0.03  roll_hold 2.0  roll 0.55  hall_rt 3.8",
})
_PRO_V2 = _members(_PRO_BASE, {
    "rumble": "",
    "air": "gate 0.6  body_attack 0.25  air 0.5  air_f 9000  grain_depth 0.5  roll 1.0  roll_hold 0.5  sub 0.35",
    "short": "gate 0.65  body_attack 0.05  crack 0.5  grain_rate 42  roll_hold 0.4  roll_fall 0.3  body 0.5",
    "deep": "gate 1.3  sub 0.6  sub_f 36  body_f1 62  grain_rate 28  roll_cut 150  roll_hold 0.9  air 0.06",
    "swell": "pre 0.18  pre_level 0.3  body_attack 0.2  gate 0.9  grain_rate 32",
})
_IMPACT_V3 = _members(_CINE_BRIGHT, {
    "classic": "",
    "deep": """body_f0 110  body_f1 44  body 0.8  sub 0.45  sub_f 34  sub_tau 1.0  roll 0.5  roll_cut 140
               roll_hold 2.2  crack 1.3  crack_f 1600  blast_f 700  blast_tau 0.45  hall_rt 4.5  hall_damp 0.6
               sheen 1.5  sheen_ms 10""",
    "punch": """body_f0 210  body_f1 58  drop_tau 0.03  body_tau 0.18  crack 1.35  crack_f 3000  crack_ms 7
                blast_tau 0.12  roll_hold 0.9  roll_fall 0.4  hall_rt 2.0  hall_wet 0.6  sub 0.15  sub_tau 0.45
                sheen 1.6  sheen_ms 5""",
    "metal": """shell 0.8  shell_lo 260  modes 0.5  crack_f 3500  debris 0.6  air 0  air_f 6500  hall_damp 0.35
                hall_rt 3.6  blast_f 2000  sheen 1.8  sheen_ms 9  sheen_f 7000  crack 2.0  body_attack 0.001
                hall_tone 5500""",
    "swell": """pre 0.5  pre_level 0.35  pre_shape 1  body_attack 0.03  roll_hold 2.0  sheen 1.0  roll 0.45
                hall_rt 3.8""",
    "thunder": """body 0.5  body_f0 130  body_f1 48  debris 1.0  crack_f 1800  blast 1.0  blast_f 900
                  blast_tau 0.5  roll 0.7  roll_hold 1.9  roll_depth 0.35  roll_rate 1.2  roll_cut 260  sheen 1.6
                  sheen_ms 9  crack 1.7  hall_rt 3.4""",
})
_PRO_V3 = _members(_PRO_AIRY, {
    "rumble": "",
    "air": """gate 0.6  body_attack 0.25  air 0.9  air_f 9000  grain_depth 0.5  roll 1.0  roll_hold 0.5  sub 0.35
              gate_db 18  gate_fall 0.15  air_div 1.0  grain_sharp 3.0""",
    "short": """gate 0.65  body_attack 0.05  crack 0.5  grain_rate 42  roll_hold 0.4  roll_fall 0.3  body 0.5
                gate_db 14  gate_fall 0.2""",
    "deep": """gate 1.3  sub 0.6  sub_f 36  body_f1 62  grain_rate 28  roll_cut 150  roll_hold 0.9  air 0.6
               gate_db 16  gate_fall 0.25""",
    "swell": "pre 0.18  pre_level 0.3  body_attack 0.2  gate 0.9  grain_rate 32",
})

# Tuned values of the fourth pro round, one row per member, in the order of _TUNED_KEYS.
_TUNED_KEYS = ("crack body_f1 body_attack body sub roll roll_cut roll_hold roll_fall grain_rate grain_depth air "
               "gate gate_db").split()
_TUNED_ROWS = {
    "rumble": "0.5122 39.5286 0.1351 0.1221 0.0082 1.264 101.7411 0.6747 0.3832 22.7054 0.9699 0.4949 0.955 20.3501",
    "air": "0.3841 42.8117 0.0444 0.3126 0.6875 1.4018 255.8402 0.7123 0.4128 22.3646 0.8043 0.6265 0.8096 15.4599",
    "short": "0.4808 75.0 0.0464 0.5303 0.1124 1.4651 194.2516 0.4826 0.2785 39.6209 1.0 0.6444 0.6288 14.1698",
    "deep": "0.1025 50.8261 0.0534 0.2825 0.4785 1.3857 109.0885 0.3807 0.601 18.6677 0.6976 0.7944 1.2768 16.8876",
    "swell": "0.3 75.0 0.2 0.35 0.15 1.5 200 0.7 0.5 32 1.0 0.7 0.9 15",
}
_TUNED_SHARED = "pre_shape 1  hall_tone 6000  sheen_ms 8  sheen_f 6363.961  crack_f 1800"
_PRO_V4 = [(name, _stack(_PRO_JITTER, edit, dict(zip(_TUNED_KEYS, map(cm.scalar, _TUNED_ROWS[name].split()))),
                         _TUNED_SHARED))
           for name, edit in _PRO_V3]


def _grainy(rate: float) -> dict:
    """Grain settings of the sixth round: the given rate, jitter 0.5, boom drive 0.1."""
    return {"grain_rate": rate, "grain_jit": 0.5, "boom_drive": 0.1}


_BOOM_EDITS = {
    "rumble": _stack("boom 0.6  boom_f0 120  boom_f1 42  boom_drop 0.07  boom_tau 0.3  boom_att 0.07  roll_in 0.03"
                     "  crack 0.7", _grainy(40)),
    "air": _stack("boom 0.55  boom_f0 130  boom_f1 46  boom_drop 0.06  boom_tau 0.25  boom_att 0.08  roll_in 0.03",
                  _grainy(38)),
    "short": _stack("boom 0.55  boom_f0 140  boom_f1 52  boom_drop 0.05  boom_tau 0.2  boom_att 0.06  roll_in 0.03",
                    _grainy(42)),
    "deep": _stack("boom 0.65  boom_f0 95  boom_f1 36  boom_drop 0.1  boom_tau 0.3  boom_att 0.08  roll_in 0.04"
                   "  gate_db 12  air 0.45", _grainy(34)),
    "swell": _stack("boom 0.5  boom_f0 110  boom_f1 44  boom_drop 0.08  boom_tau 0.35  boom_att 0.12  roll_in 0.06",
                    _grainy(36)),
}
_PRO_V6 = [(name, _stack(prev, _BOOM_EDITS[name])) for name, prev in _PRO_V4]
_IMPACT_V4 = _members(_CINE_SOFT, {
    "classic": "",
    "deep": """body_f0 95  body_f1 38  body 0.8  sub 0.55  sub_f 34  sub_tau 1.0  roll 0.55  roll_cut 110
               roll_hold 2.1  crack 1.1  crack_f 1500  blast 0.9  blast_f 600  blast_tau 0.45  shell 0.7
               hall_rt 4.5  hall_damp 0.6  hall_tone 4000  sheen 0.45  sheen_ms 16  top 11000""",
    "punch": """body_f0 230  body_f1 66  drop_tau 0.03  body_tau 0.22  crack 0.95  modes 0.5  shell 0.5  roll 0.25
                crack_f 1800  crack_ms 7  blast_tau 0.12  roll_hold 0.9  roll_fall 0.4  hall_rt 2.1  hall_wet 0.45
                sub 0.15  sub_tau 0.45  sheen 0.35  sheen_ms 10""",
    "metal": """shell 0.8  shell_lo 260  modes 0.5  crack_f 2200  debris 0.4  air 0  air_f 6500  hall_damp 0.35
                hall_rt 3.6  blast_f 2000  sheen 0.35  sheen_ms 12  sheen_f 7000  crack 0.95  body_attack 0.001
                hall_tone 5500  plate 0.7  plate_f 380  plate_tau 0.9  top 11000""",
    "swell": """pre 0.5  pre_level 0.35  pre_shape 1  body_attack 0.03  roll_hold 2.0  sheen 0.3  roll 0.45
                hall_rt 3.8""",
    "blast": """body 0.5  body_f0 130  body_f1 48  debris 0.6  crack_f 1400  blast 1.0  blast_f 900  blast_tau 0.5
                roll 0.7  roll_hold 1.9  roll_depth 0.35  roll_rate 1.2  roll_cut 260  sheen 0.3  sheen_ms 12
                crack 1.1  hall_rt 3.4  scatter 0.5  scatter_s 2.0""",
})

FAMILIES: dict[str, list[tuple[str, dict]]] = {"impact": _IMPACT_V4, "pro_impact": _PRO_V6}
FAMILIES_R2 = {"impact": _IMPACT_V2, "pro_impact": _PRO_V2}
FAMILIES_R3 = {"impact": _IMPACT_V3, "pro_impact": _PRO_V3}
FAMILIES_R4 = {"impact": _IMPACT_V4, "pro_impact": _PRO_V4}

SEARCH_KEYS: dict[str, list[str]] = {
    "impact": ("crack crack_f sheen sheen_ms debris blast blast_f blast_tau body_f0 body_f1 body_tau body sub "
               "shell roll roll_cut roll_hold roll_fall roll_depth hall_wet hall_rt hall_tone width").split(),
    "pro_impact": ("crack body body_f1 body_attack sub roll roll_cut roll_hold roll_fall grain_rate grain_depth "
                   "air gate gate_db").split(),
}
LIMITS: dict[str, dict[str, tuple]] = {
    "pro_impact": cm.space_table("""
        gate 0.55 1.35
        body_attack 0.04 0.3 log
        roll_cut 100.0 260.0 log
        roll_hold 0.2 1.2
        roll_fall 0.15 0.8 log
        sub 0.0 0.7
        air 0.2 1.0
        gate_db 12.0 22.0
    """),
    "impact": cm.space_table("""
        roll_hold 0.8 3.0
        hall_rt 1.6 6.0 log
        roll_fall 0.25 1.6 log
    """),
}


def space_for(cat: str) -> dict[str, Any]:
    """Search space of one category: its search keys, each at its narrower limit when it has one."""
    tighter = LIMITS.get(cat, {})
    return {key: tighter.get(key, SPACE[key]) for key in SEARCH_KEYS[cat]}


def _pick(cat: str, preset: int | str) -> tuple[str, dict]:
    group = FAMILIES[cat]
    if not isinstance(preset, str):
        name, params = group[preset]
        return name, dict(params)
    by_name = dict(group)
    if preset not in by_name:
        raise KeyError(f"impact: no member {preset!r} in {cat!r}")
    return preset, dict(by_name[preset])


def recipe_for(cat: str = "impact", preset: int | str = 0) -> types.SimpleNamespace:
    """Recipe object for one family member, chosen by index or by name."""
    name, params = _pick(cat, preset)
    return types.SimpleNamespace(
        MODULE=__name__, SPACE=space_for(cat), DEFAULTS=params, render=render, TARGET=TARGETS[cat],
        ALTS=list(ALTS), REFSET=cat, PRESET=name,
    )


# ---------------------------------------------------------------- measurement

def _first_true(mask: np.ndarray, fallback: int) -> int:
    hits = np.nonzero(mask)[0]
    return int(hits[0]) if len(hits) else fallback


def _ms_levels(mono: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """RMS of each 1 ms frame, and the same in dB."""
    count = max(1, len(mono) // 48)
    frames = np.resize(mono, count * 48).reshape(count, 48) if len(mono) >= 48 else mono.reshape(1, -1)
    rms = np.sqrt(np.mean(frames ** 2, axis=1) + 1e-20)
    return rms, 20 * np.log10(rms + 1e-12)


def _fall_stats(smooth_db: np.ndarray) -> dict:
    """t20, t40 and the fitted decay slope, all counted from the loudest smoothed frame."""
    top_at = int(np.argmax(smooth_db))
    top = smooth_db[top_at]
    after = smooth_db[top_at:]
    stats: dict[str, Any] = {"t20": _first_true(after < top - 20, len(after)),
                             "t40": _first_true(after < top - 40, len(after))}
    lo = top_at + _first_true(after < top - 6, 0)
    hi = top_at + _first_true(after < top - 40, len(after))
    slope = float("nan")
    if hi - lo > 5:
        slope = round(float(np.polyfit(np.arange(lo, hi) / 1000, smooth_db[lo:hi], 1)[0]), 1)
    stats["slope_dBs"] = slope
    return stats


def _end_stats(smooth_db: np.ndarray) -> dict:
    """len50, plus how far the level falls over the closing 40 ms and sits 60 ms before the end."""
    top = smooth_db[int(np.argmax(smooth_db))]
    loud = np.nonzero(smooth_db > top - 50)[0]
    last = int(loud[-1])
    drop40 = round(float(smooth_db[last - 40] - smooth_db[last]), 1) if last > 40 else 0.0
    before = round(float(smooth_db[last - 60] - top), 1) if last > 60 else 0.0
    return {"len50": round((last - int(loud[0])) / 1000, 3), "gate_db40ms": drop40, "pre_gate": before}


def _level_stats(mono: np.ndarray) -> dict:
    rms, level = _ms_levels(mono)
    top_at = int(np.argmax(rms))
    stats: dict[str, Any] = {"attack_ms": top_at - _first_true(level >= level[top_at] - 20, top_at),
                             "peak_ms": top_at}
    smooth = 10 * np.log10(np.convolve(rms ** 2, np.ones(10) / 10, mode="same") + 1e-20)
    stats.update(_fall_stats(smooth))
    stats.update(_end_stats(smooth))
    return stats


def _share_db(hz: np.ndarray, power: np.ndarray, lo: float, hi: float, total: float) -> float:
    part = np.sum(power[(hz >= lo) & (hz < hi)])
    if part > 0:
        return float(10 * np.log10(part / total))
    return -120.0


_OCTAVE_CENTRES = (31.5, 63, 125, 250, 500, 1000, 2000, 4000, 8000, 16000)
_THIRD_CENTRES = 1000 * 2 ** (np.arange(-17, 14) / 3)


def _balance_slope(hz: np.ndarray, power: np.ndarray, total: float) -> float:
    """Fitted slope of third-octave level (dB) against log2 of the band centre, over 100 Hz to 10 kHz."""
    step = 2 ** (1 / 6)
    levels = np.array([_share_db(hz, power, c / step, c * step, total) for c in _THIRD_CENTRES])
    used = (_THIRD_CENTRES >= 100) & (_THIRD_CENTRES <= 10000)
    return round(float(np.polyfit(np.log2(_THIRD_CENTRES[used]), levels[used], 1)[0]), 2)


def _spectrum_stats(mono: np.ndarray) -> dict:
    hz, power = signal.welch(mono, SR, nperseg=min(len(mono), 16384))
    total = np.sum(power) + 1e-30
    half = np.sqrt(2)
    return {
        "sh150": round(float(np.sum(power[hz < 150]) / total), 3),
        "tilt": _balance_slope(hz, power, total),
        "oct": {int(c): round(_share_db(hz, power, c / half, c * half, total), 1) for c in _OCTAVE_CENTRES},
        "cen": int(round(np.sum(hz * power) / total)),
    }


def _loud_frames(loudest: np.ndarray) -> np.ndarray:
    """Mask of frames within 25 dB of the loudest frame; all false for silence."""
    if loudest.max() > 0:
        return loudest >= loudest.max() * 10 ** (-25 / 20)
    return np.zeros_like(loudest, bool)


def _pitch_track(mono: np.ndarray) -> tuple[int, int, int]:
    """Start, median and end of the strongest 25 Hz - 3 kHz bin over the loud frames."""
    seg = min(8192, len(mono))
    hz, _tt, spec = signal.stft(mono, SR, nperseg=seg, noverlap=max(0, seg - 960), boundary=None, padded=False)
    band = (hz >= 25) & (hz <= 3000)
    if spec.shape[1] == 0 or not np.any(band):
        return (0, 0, 0)
    mag = np.abs(spec[band])
    loud = _loud_frames(mag.max(axis=0))
    if not np.any(loud):
        return (0, 0, 0)
    track = hz[band][np.argmax(mag, axis=0)][loud]
    return (int(round(track[0])), int(round(np.median(track))), int(round(track[-1])))


def _stereo_stats(left: np.ndarray, right: np.ndarray) -> dict:
    """Correlation of the two channels and the side level against the mid, in dB."""
    side = (left - right) / 2
    mid = (left + right) / 2
    corr = np.sum(left * right) / (np.sqrt(np.sum(left ** 2) * np.sum(right ** 2)) + 1e-30)
    ratio = (np.sum(side ** 2) + 1e-30) / (np.sum(mid ** 2) + 1e-30)
    return {"corr": round(float(corr), 2), "side_db": round(float(10 * np.log10(ratio)), 1)}


def measure(x: np.ndarray) -> dict:
    """Measure a render (mono or stereo) for the study checks."""
    st = np.asarray(x, dtype=float)
    if st.ndim != 2:
        st = np.column_stack([st, st])
    mono = st.mean(axis=1)
    out: dict[str, Any] = {"dur": round(len(mono) / SR, 3)}
    out.update(_level_stats(mono))
    out.update(_spectrum_stats(mono))
    out.update(_stereo_stats(st[:, 0], st[:, 1]))
    out["ridge"] = _pitch_track(mono)
    return out


STUDY: dict[str, dict[str, tuple[float, float]]] = {
    "impact": cm.space_table("""
        t40 1300 2600
        slope_dBs -20 -10
        tilt -6.5 -3.5
        sh150 0.65 0.93
    """),
    "pro_impact": cm.space_table("""
        len50 0.6 1.3
        attack_ms 50 290
        t40 250 1100
        gate_db40ms 8 40
        sh150 0.75 0.97
        tilt -6.0 -2.0
        corr -0.1 0.5
        pre_gate -34 -22
    """),
}
# Keys whose distance is measured on a straight scale, not in octaves.
_STRAIGHT = {"tilt", "slope_dBs", "corr", "gate_db40ms", "sh150"}
_TRACK_SLOT = {"ridge0": 0, "ridge": 1, "ridge1": 2}


def _lookup(stats: dict, key: str) -> float | None:
    if key not in _TRACK_SLOT:
        return stats.get(key)
    track = stats.get("ridge")
    return None if track is None else track[_TRACK_SLOT[key]]


def study_penalty(m: dict, targets: dict[str, tuple[float, float]]) -> float:
    """Sum of how far each measured value sits outside its study range (in octaves or range widths)."""
    total = 0.0
    for key, (lo, hi) in targets.items():
        val = _lookup(m, key)
        if val is None or not np.isfinite(val) or lo <= val <= hi:
            continue
        edge = lo if val < lo else hi
        if lo > 0 and val > 0 and key not in _STRAIGHT:
            total += abs(np.log2(val / edge))
        else:
            total += abs(val - edge) / max(1e-6, hi - lo)
    return float(total)


def study_extra(targets: dict[str, tuple[float, float]], weight: float = 1.0) -> Callable[[np.ndarray], float]:
    """Critic extra term: minus ``weight`` times the study penalty of the render."""

    def extra(x: np.ndarray, row: dict | None = None) -> float:
        return -weight * study_penalty(measure(x), targets)

    return extra
