"""glitch: a digital stutter that opens with crackle and accelerates into the cut.

Two paths: version 2 (``render``) and version 6 (``render6``), chosen by the presence of ``roll0``. A pattern
generator, seeded from ``pattern``, lays out the gesture; the render generator draws every noise sample.
"""

from __future__ import annotations

import types

import numpy as np

from hark.forge.engine import dsp, fx
from hark.forge.recipes.risers import _glitchkit as gk
from hark.forge.recipes.risers._common import SR, bw, crush, hann, headroom, n_of, t_of

TARGET = "a short digital glitch stutter with bit-crushed noise bursts that speed up and cut off"
ALTS = ["radio static", "a helicopter", "a woodpecker", "rain on a tin roof"]
REFSET = "pro_glitch"
TARGETS = {"pro_glitch": TARGET, "glitch": TARGET}
PRO_SHARES = (-21.7, -24.8, -17.0, -1.2, -6.6)
FM_RATIOS = (0.5, 1.0, 1.41, 2.0, 3.0, 7.13)

_V2 = [
    ("dur", 0.35, 0.85, None, 0.65), ("crackle", 0.12, 0.5, None, 0.35),
    ("crackle_rate", 40.0, 300.0, "log", 200.0), ("crackle_hp", 1500.0, 6000.0, "log", 3500.0),
    ("crackle_db", -30.0, -6.0, None, -14.0), ("grid0", 0.026, 0.046, None, 0.032),
    ("grid1", 0.015, 0.028, None, 0.022), ("accel", 0.6, 2.5, "log", 1.2), ("gate", 0.35, 0.95, None, 0.7),
    ("swell", 0.0, 10.0, None, 4.0), ("bits", 3.0, 10.0, None, 5.0), ("srate", 1500.0, 16000.0, "log", 6000.0),
    ("tonal", 0.0, 1.0, None, 0.35), ("tone_f", 500.0, 5000.0, "log", 1800.0), ("fm", 0.0, 1.0, None, 0.3),
    ("slices", 1, 5, "int", 3), ("repeat", 0.0, 0.9, None, 0.5), ("rev", 0.0, 0.5, None, 0.15),
    ("pitch_step", -2.0, 2.0, None, 0.0), ("low_db", -50.0, -14.0, None, -34.0),
    ("decay", 0.15, 3.0, "log", 0.6), ("lvl_jit", 0.0, 8.0, None, 4.0), ("floor_db", -72.0, -36.0, None, -52.0),
    ("width", 0.0, 1.0, None, 0.9), ("hp", 300.0, 7000.0, "log", 4500.0), ("bright", -6.0, 9.0, None, 3.0),
    ("drive", 0.0, 12.0, None, 4.0), ("end_tail", 0.004, 0.025, "log", 0.012), ("pattern", 0, 999, "int", 11),
    ("skip", 0.0, 0.5, None, 0.0), ("accent_db", 0.0, 12.0, None, 0.0),
]
_V6 = [
    ("dur", 0.22, 0.85, None, 0.55), ("crackle", 0.0, 0.35, None, 0.12), ("crackle_db", -30.0, -6.0, None, -18.0),
    ("grid0", 0.03, 0.11, None, 0.06), ("grid1", 0.012, 0.045, None, 0.022), ("accel", 0.6, 2.5, "log", 1.2),
    ("gate", 0.45, 1.0, None, 0.8), ("roll0", 0.006, 0.07, "log", 0.03), ("roll1", 0.0025, 0.03, "log", 0.006),
    ("srate0", 2500.0, 24000.0, "log", 16000.0), ("srate1", 700.0, 12000.0, "log", 3000.0),
    ("sdrop", 0.0, 0.8, None, 0.3), ("bits0", 4.0, 12.0, None, 8.0), ("bits1", 2.0, 8.0, None, 4.0),
    ("tone", 0.0, 1.0, None, 0.6), ("tone_f", 110.0, 1400.0, "log", 330.0), ("notes", 1, 3, "int", 2),
    ("chirp", 0.0, 1.0, None, 0.3), ("fm", 0.0, 1.0, None, 0.3), ("noise", 0.0, 1.0, None, 0.3),
    ("jump", 0.0, 1.0, None, 0.5), ("rev", 0.0, 0.5, None, 0.15), ("pitch_step", -2.0, 2.0, None, 0.0),
    ("drop", 0.0, 0.4, None, 0.12), ("width", 0.0, 1.0, None, 0.8), ("pan_jump", 0.0, 1.0, None, 0.3),
    ("decay", 0.3, 6.0, "log", 2.0), ("swell", 0.0, 10.0, None, 4.0), ("lvl_jit", 0.0, 8.0, None, 3.0),
    ("low_db", -60.0, -10.0, None, -30.0), ("hp", 60.0, 2000.0, "log", 150.0),
    ("lp", 5000.0, 18000.0, "log", 14000.0), ("bright", -6.0, 6.0, None, 0.0), ("drive", 0.0, 12.0, None, 4.0),
    ("end_tail", 0.004, 0.03, "log", 0.01), ("floor_db", -72.0, -40.0, None, -56.0),
    ("pattern", 0, 999, "int", 601),
]


def _space(rows: list) -> dict:
    return {k: ((lo, hi) if scale is None else (lo, hi, scale)) for k, lo, hi, scale, _ in rows}


SPACE = _space(_V2)
DEFAULTS = {k: d for k, _, _, _, d in _V2 if k not in ("skip", "accent_db")}
SPACE6 = _space(_V6)
DEFAULTS6 = {k: d for k, _, _, _, d in _V6}

FAMILY6 = [
    ("stutter", dict(roll0=0.035, roll1=0.004, grid0=0.07, grid1=0.02, gate=0.9, srate0=16000, srate1=2500,
                     bits0=8, bits1=4, tone=0.5, tone_f=280, chirp=0.2, fm=0.3, noise=0.6, jump=0.25, drop=0.08,
                     swell=8, lvl_jit=2, hp=250, bright=3, dur=0.6, pattern=611)),
    ("crush", dict(roll0=0.02, roll1=0.011, grid0=0.055, grid1=0.025, gate=0.75, srate0=12000, srate1=1500,
                   sdrop=0.6, bits0=6, bits1=3, tone=0.25, chirp=0.1, fm=0.4, noise=0.9, jump=0.8, pan_jump=0.6,
                   swell=8, lvl_jit=2, hp=300, bright=3, dur=0.5, pattern=612)),
    ("data", dict(roll0=0.012, roll1=0.005, grid0=0.045, grid1=0.018, gate=0.6, srate0=20000, srate1=6000,
                  bits0=7, bits1=5, tone=0.0, chirp=0.8, fm=0.6, noise=0.5, jump=0.9, drop=0.05, width=1.0,
                  pan_jump=0.5, swell=9, lvl_jit=2, hp=500, crackle=0.0, dur=0.45, pattern=613)),
    ("dropout", dict(roll0=0.06, roll1=0.012, grid0=0.09, grid1=0.03, gate=0.7, srate0=24000, srate1=1200,
                     sdrop=0.15, bits0=10, bits1=3, tone=0.35, tone_f=180, notes=2, chirp=0.0, fm=0.5, noise=0.7,
                     jump=0.1, drop=0.15, swell=8, lvl_jit=2, low_db=-24, hp=120, crackle=0.0, dur=0.7,
                     pattern=614)),
    ("scramble", dict(roll0=0.025, roll1=0.006, grid0=0.05, grid1=0.02, gate=0.85, srate0=8000, srate1=2000,
                      sdrop=0.5, bits0=6, bits1=4, tone=0.3, tone_f=600, chirp=0.3, fm=0.8, noise=0.6, jump=1.0,
                      rev=0.45, pitch_step=1.0, drop=0.05, pan_jump=0.4, swell=9, lvl_jit=2, dur=0.55,
                      pattern=615)),
    ("hit", dict(roll0=0.015, roll1=0.003, grid0=0.035, grid1=0.014, gate=0.75, srate0=10000, srate1=2000,
                 bits0=6, bits1=3, tone=0.3, tone_f=220, chirp=0.2, fm=0.3, noise=0.8, jump=0.5, drop=0.0, swell=6,
                 low_db=-22, crackle=0.0, dur=0.3, pattern=616)),
]


def full6(name: str) -> dict:
    """A FAMILY6 member as a full parameter set (DEFAULTS6 plus its overrides)."""
    p = dict(DEFAULTS6)
    p.update(dict(FAMILY6)[name])
    return {k: (float(v) if not isinstance(v, str) else v) for k, v in p.items()}


# ------------------------------------------------------------------------------------------------ version 2


def _fm_tone(prng: np.random.Generator, n: int, fc_lo: float, fc_hi: float, ix_lo: float,
             ix_hi: float) -> np.ndarray:
    fc = prng.uniform(fc_lo, fc_hi)
    ratio = FM_RATIOS[int(prng.integers(len(FM_RATIOS)))]
    index = prng.uniform(ix_lo, ix_hi)
    t = t_of(n)
    return np.sin(2 * np.pi * fc * t + index * np.sin(2 * np.pi * fc * ratio * t))


def _buffer_v2(p: dict, size: int, rng, prng) -> np.ndarray:
    noise = crush(rng.standard_normal(size), p["bits"] + 2, p["srate"])
    x = noise / (np.std(noise) + 1e-12) * (1 - 0.6 * max(p["tonal"], p["fm"]))
    if p["tonal"] > 0.02:
        base = p["tone_f"] * 2.0 ** prng.uniform(-1, 1)
        steps = [int(k) for k in prng.integers(-5, 6, int(prng.integers(1, 4)))]
        shape = "square" if prng.random() < 0.7 else "saw"
        x = x + p["tonal"] * 1.4 * dsp.osc(gk.stepped_track(size, base, steps), shape)
    if p["fm"] > 0.02:
        x = x + p["fm"] * 1.2 * _fm_tone(prng, size, 800, 6000, 1.5, 8)
    return crush(x / (gk.peak(x) + 1e-12), p["bits"], p["srate"])


def _choices_v2(p: dict, b: types.SimpleNamespace, prev: list[int], nbuf: int, prng) -> tuple[tuple, tuple]:
    """(buffer, reversed, offset) for left and right; with chance 1 - width the right copies the left."""
    left = _pick_v2(p, prev[0], nbuf, 0, prng)
    if prng.random() > p["width"]:
        return left, left
    off = 0 if b.last else n_of(prng.uniform(0, 0.003))
    return left, _pick_v2(p, prev[1], nbuf, off, prng)


def _pick_v2(p: dict, prev_idx: int, nbuf: int, offset: int, prng) -> tuple[int, bool, int]:
    idx = prev_idx if prng.random() < p["repeat"] else int(prng.integers(nbuf))
    return idx, bool(prng.random() < p["rev"]), offset


def _burst_env_v2(p: dict, m: int, d: float, last: bool, tail_n: int) -> np.ndarray:
    if not last:
        return gk.decay_env(m, p["decay"] * d, 0.0004)
    hold_tau = max(p["decay"] * d, 0.5 * (m - 1) / SR)
    return gk.last_env(m - 1, hold_tau, tail_n + 1, p["end_tail"])[:m + tail_n]


def _level_v2(p: dict, k: int, nb: int, last: bool, prng) -> float:
    jit = abs(p["lvl_jit"] * prng.standard_normal())
    lv = 10 ** ((p["swell"] * (k / max(1, nb - 1) - 1) - jit * (not last)) / 20)
    if "accent_db" in p and not last and prng.random() > 0.35:
        lv *= 10 ** (-p["accent_db"] / 20)
    return lv


def _low_pulse(m: int, f0: float, tau: float, gain: float) -> np.ndarray:
    t = t_of(m)
    return np.sin(2 * np.pi * f0 * t) * np.exp(-t / tau) * gk.box(m, 0.001) * gain


def _layout(p: dict, t_st: float) -> types.SimpleNamespace:
    """Sizes, the slot grid from t_st to the cut, and the buffer length (largest slot + 4 ms)."""
    n_end, tail_n = n_of(p["dur"]), n_of(4 * p["end_tail"])
    slots = gk.grid(t_st, p["dur"], p["grid0"], p["grid1"], p["accel"])
    return types.SimpleNamespace(n_end=n_end, tail_n=tail_n, n=n_end + tail_n, t_st=t_st, slots=slots,
                                 nb=len(slots), size=n_of(max(d for _, d in slots) + 0.004))


def _burst_v2(p: dict, k: int, slot: tuple, ctx: types.SimpleNamespace, prng) -> types.SimpleNamespace:
    """Start, level, speed and envelope of burst k; the last burst runs to the cut."""
    t0, d = slot
    last = k == ctx.nb - 1
    start = gk.start_of(t0)
    g = max(16, ctx.n_end - start) if last else max(16, n_of(d * p["gate"]))
    m = g if last else min(g, ctx.size)
    return types.SimpleNamespace(k=k, d=d, last=last, start=start, speed=2.0 ** (k * p["pitch_step"] / 12),
                                 level=_level_v2(p, k, ctx.nb, last, prng),
                                 env=_burst_env_v2(p, m, d, last, ctx.tail_n))


def _skipped(p: dict, b: types.SimpleNamespace, prng) -> bool:
    """Round 3: with chance skip, a slot that is not the first or the last stays empty."""
    return "skip" in p and 0 < b.k and not b.last and prng.random() < p["skip"]


def _write_v2(y: np.ndarray, b: types.SimpleNamespace, bufs, choices, rng) -> None:
    for c, (idx, rev, off) in enumerate(choices):
        seg = gk.body_segment(gk.read_slice(bufs[idx], rev, b.speed), len(b.env))
        gk.add_clip(y[:, c], seg * b.env * b.level * np.exp(0.12 * rng.standard_normal()), b.start + off)


def _low_v2(p: dict, low: np.ndarray, b: types.SimpleNamespace, prng) -> None:
    m = min(n_of(b.d * p["gate"] + 0.006), len(low) - b.start)
    f0 = prng.uniform(55, 110)
    if m > 1:
        gk.add_clip(low, _low_pulse(m, f0, 0.012, 10 ** (p["low_db"] / 20) * b.level), b.start)


def _bursts_v2(p: dict, ctx: types.SimpleNamespace, bufs, rng, prng) -> tuple[np.ndarray, np.ndarray]:
    y, low = np.zeros((ctx.n, 2)), np.zeros(ctx.n)
    prev = [0, 0]
    for k, slot in enumerate(ctx.slots):
        b = _burst_v2(p, k, slot, ctx, prng)
        if not _skipped(p, b, prng):
            choices = _choices_v2(p, b, prev, len(bufs), prng)
            prev = [choices[0][0], choices[1][0]]
            _write_v2(y, b, bufs, choices, rng)
            if p["low_db"] > -39.5:
                _low_v2(p, low, b, prng)
    return y, low


def _crackle_grain_v2(p: dict, rng, round3: bool):
    def grain(u: float) -> np.ndarray:
        m = max(2, n_of(rng.uniform(0.0015, 0.006)))
        t = t_of(m)
        if rng.random() < 0.2:
            g = 1.5 * np.sin(2 * np.pi * rng.uniform(150, 900) * t) * hann(m)
        else:
            w = hann(m) if round3 else np.sqrt(hann(m))
            g = rng.standard_normal(m) * w * np.exp(-t / (0.5 * m / SR))
        return g * 10 ** ((p["crackle_db"] + 18 * np.log10(rng.uniform(0.05, 1)) + 6 * u) / 20)
    return grain


def _crackle_v2(p: dict, n: int, t_st: float, rng) -> np.ndarray:
    grain = _crackle_grain_v2(p, rng, "skip" in p)
    chans = []
    for _ in range(2):
        x = gk.grain_train(n, t_st, gk.accel_rate(p["crackle_rate"]), rng, grain)
        chans.append(0.85 * bw(x, "high", p["crackle_hp"], 2) + 0.15 * bw(x, "high", 120, 2))
    return np.column_stack(chans)


def _bed(n: int, n_end: int, hp: float, rate: float, rng) -> np.ndarray:
    bed = bw(rng.standard_normal((n, 2)), "high", hp, 2)
    bed = np.column_stack([crush(bed[:, c], 6, rate) for c in range(2)])
    bed = bed / (4 * np.std(bed) + 1e-12)
    bed[n_end:] = 0.0
    return bed


def _finish(y: np.ndarray, drive: float) -> np.ndarray:
    if drive > 0.2:
        y = fx.saturate(y / (gk.peak(y) + 1e-12), drive, os=2)
    y = bw(y, "low", 18000, 8)
    y[:2] = 0.0
    return dsp.fade(y, 0.0005, 0.003)


def _merged_v2(p: dict) -> dict:
    q = dict(DEFAULTS)
    q.update(p or {})
    return q


def _texture_v2(p: dict, ctx: types.SimpleNamespace, y: np.ndarray, rng) -> np.ndarray:
    """The crackle lead-in and the crushed bed under the stutter, both scaled to the current peak."""
    if p["crackle"] > 0.02 and ctx.t_st > 0.02:
        y = y + _crackle_v2(p, ctx.n, ctx.t_st, rng) * gk.peak(y)
    if p["floor_db"] > -71.5:
        ramp = np.minimum(1.0, t_of(ctx.n) / max(0.02, ctx.t_st + 0.02))[:, None]
        y = y + 10 ** (p["floor_db"] / 20) * gk.peak(y) * _bed(ctx.n, ctx.n_end, 3000, p["srate"], rng) * ramp
    return y


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """One take. A set with roll0 renders with version 6; else version 2 (not peak-normalised below 0 dBFS)."""
    if "roll0" in (p or {}):
        return render6(p, rng)
    p = _merged_v2(p)
    prng = gk.pattern_rng(p["pattern"], 13)
    ctx = _layout(p, p["dur"] * p["crackle"])
    bufs = [_buffer_v2(p, ctx.size, rng, prng) for _ in range(max(1, int(round(p["slices"]))))]
    y, low = _bursts_v2(p, ctx, bufs, rng, prng)
    y = bw(y, "high", p["hp"], 4)
    y = _texture_v2(p, ctx, y + (low * gk.peak(y))[:, None], rng)
    return headroom(_finish(dsp.peq(y, 6000, p["bright"], 0.5), p["drive"]))


# ------------------------------------------------------------------------------------------------ version 6

_NS = n_of(0.6)
_INTERVALS = (0, 7, 12, 3, 10)


def _chord6(p: dict, prng) -> np.ndarray:
    track = gk.held_track(_NS, prng, lambda: p["tone_f"] * 2.0 ** (int(prng.integers(-7, 8)) / 12), 0.04, 0.12)
    shape = "square" if prng.random() < 0.5 else "saw"
    notes = _INTERVALS[:max(1, int(round(p["notes"])))]
    return sum(dsp.osc(track * 2.0 ** (iv / 12) * (1 + 0.004 * j), shape) for j, iv in enumerate(notes)) / len(notes)


def source6(p: dict, rng, prng) -> np.ndarray:
    """The 0.6 s source the bursts cut from: chord, chirps, FM and noise, peak 1."""
    s = np.zeros(_NS)
    if p["tone"] > 0.01:
        s += p["tone"] * _chord6(p, prng)
    if p["chirp"] > 0.01:
        track = gk.held_track(_NS, prng, lambda: 1000 * 2.0 ** prng.uniform(0, 2), 0.005, 0.03)
        s += p["chirp"] * 0.8 * dsp.osc(track, "square")
    if p["fm"] > 0.01:
        s += p["fm"] * _fm_tone(prng, _NS, 500, 3000, 2, 9)
    if p["noise"] > 0.01:
        s += p["noise"] * 0.5 * rng.standard_normal(_NS)
    return s / (gk.peak(s) + 1e-12)


def _crackle6(p: dict, n: int, t_end: float, rng) -> np.ndarray:
    def grain(u: float) -> np.ndarray:
        m = max(2, n_of(rng.uniform(0.0015, 0.006)))
        g = crush(rng.standard_normal(m), 4, (4000, 8000, 16000)[int(rng.integers(3))]) * hann(m)
        return g * 10 ** ((p["crackle_db"] + 14 * np.log10(rng.uniform(0.05, 1)) + 6 * u) / 20)
    chans = [bw(gk.grain_train(n, t_end, gk.accel_rate(220.0), rng, grain), "high", 1500, 2) for _ in range(2)]
    return np.column_stack(chans)


def _rate_drop(p: dict, prng) -> float:
    """With chance sdrop the sample rate drops 1 or 2 more octaves."""
    if prng.random() < p["sdrop"]:
        return 2.0 if prng.random() < 0.5 else 4.0
    return 1.0


def _draw6(p: dict, b: types.SimpleNamespace, pos: list[int], c: int, prng) -> tuple[int, bool, float, int]:
    """Read start, reverse flag, rate and bits of one channel's segment; a jump moves the read position."""
    if prng.random() < p["jump"]:
        pos[c] = int(prng.integers(0, max(1, _NS - b.ls - 1)))
    rev = bool(prng.random() < p["rev"])
    srk = b.sr0 / _rate_drop(p, prng)
    bk = int(round(b.b0 + prng.uniform(-1, 1)))
    return max(0, min(pos[c], _NS - b.ls - 1)), rev, srk, bk


def _segment6(p: dict, src, pos: list[int], c: int, b: types.SimpleNamespace, prng) -> np.ndarray:
    a, rev, srk, bk = _draw6(p, b, pos, c, prng)
    sl = gk.read_slice(src[a:a + b.ls], rev, b.speed)
    pos[c] = (pos[c] + n_of(b.d)) % max(1, _NS - b.ls - 1)
    seg = gk.body_segment(sl, b.m) / (gk.peak(sl) + 1e-12)
    return crush(seg, max(2, bk), min(srk, SR / 2))


def _env6(p: dict, g: int, d: float, last: bool, tail_n: int) -> np.ndarray:
    if not last:
        return gk.decay_env(g, p["decay"] * d, 0.0003)
    return gk.last_env(g, g / SR / 3.45, tail_n, p["end_tail"])


def _drop_run(state: dict, want: bool) -> bool:
    """A drop-out happens when wanted and fewer than 2 drop-outs came in a row; else the run resets."""
    dropped = want and state["run"] < 2
    state["run"] = state["run"] + 1 if dropped else 0
    return dropped


def _level6(p: dict, u: float, last: bool, prng) -> float:
    jit = abs(p["lvl_jit"] * prng.standard_normal())
    return 10 ** ((p["swell"] * (u - 1) - jit * (not last)) / 20)


def _sides6(p: dict, last: bool, prng) -> tuple[float, float]:
    """With chance pan_jump (never the last burst) the burst plays hard left or hard right."""
    if prng.random() < p["pan_jump"] and not last:
        return (1.0, 0.12) if prng.random() < 0.5 else (0.12, 1.0)
    return 1.0, 1.0


def _slice6(p: dict, k: int, u: float) -> dict:
    """Slice length, speed, sample rate and bits of burst k at progress u."""
    return {"ls": max(24, n_of(p["roll0"] * (p["roll1"] / p["roll0"]) ** u)),
            "speed": 2.0 ** (k * p["pitch_step"] / 12), "sr0": p["srate0"] * (p["srate1"] / p["srate0"]) ** u,
            "b0": p["bits0"] + (p["bits1"] - p["bits0"]) * u}


def _burst6(p: dict, k: int, slot: tuple, ctx: types.SimpleNamespace, prng) -> types.SimpleNamespace:
    t0, d = slot
    u = k / max(1, ctx.nb - 1)
    last = k == ctx.nb - 1
    start = gk.start_of(t0)
    g = max(32, ctx.n_end - start) if last else max(32, n_of(d * p["gate"]))
    return types.SimpleNamespace(d=d, last=last, inner=k > 0 and not last, start=start, g=g,
                                 m=g + (ctx.tail_n if last else 0), level=_level6(p, u, last, prng),
                                 sides=_sides6(p, last, prng), **_slice6(p, k, u))


def _thump6(p: dict, low: np.ndarray, b: types.SimpleNamespace, prng) -> None:
    m = min(n_of(min(0.05, b.d)), len(low) - b.start)
    f = prng.uniform(50, 90)
    if m > 1:
        gk.add_clip(low, _low_pulse(m, f, 0.015, 10 ** (p["low_db"] / 20) * b.level), b.start)


def _write6(p: dict, y, b: types.SimpleNamespace, src, pos: list[int], prng) -> None:
    env = _env6(p, b.g, b.d, b.last, b.m - b.g) * b.level
    own = prng.random() < p["width"]
    seg_l = _segment6(p, src, pos, 0, b, prng)
    seg_r = _segment6(p, src, pos, 1, b, prng) if own else seg_l
    off = n_of(prng.uniform(0, 0.002)) if (own and not b.last) else 0
    gk.add_clip(y[:, 0], seg_l * env * b.sides[0], b.start)
    gk.add_clip(y[:, 1], seg_r * env * b.sides[1], b.start + off)


def _bursts6(p: dict, ctx: types.SimpleNamespace, src, prng) -> tuple[np.ndarray, np.ndarray]:
    pos = [int(v) for v in prng.integers(0, _NS // 2, 2)]
    state = {"run": 0}
    y, low = np.zeros((ctx.n, 2)), np.zeros(ctx.n)
    for k, slot in enumerate(ctx.slots):
        b = _burst6(p, k, slot, ctx, prng)
        if not _drop_run(state, b.inner and prng.random() < p["drop"]):
            _write6(p, y, b, src, pos, prng)
            if p["low_db"] > -55 and b.level > 0.5:
                _thump6(p, low, b, prng)
    return y, low


def _post6(p: dict, ctx: types.SimpleNamespace, y: np.ndarray, low: np.ndarray, rng) -> np.ndarray:
    """10.6 steps 5 to 10: high-pass, low thumps, crackle lead-in, bed, EQ, drive, the final low-pass and peak."""
    y = bw(y, "high", p["hp"], 2)
    pk = gk.peak(y)
    y = y + (low * 3 * pk)[:, None]
    if ctx.t_st > 0.02:
        y = y + _crackle6(p, ctx.n, ctx.t_st, rng) * 2 * pk
    if p["floor_db"] > -71.5:
        y = y + 10 ** (p["floor_db"] / 20) * pk * _bed(ctx.n, ctx.n_end, 2000, 8000, rng)
    y = _finish(dsp.peq(bw(bw(y, "high", 25, 2), "low", p["lp"], 4), 6000, p["bright"], 0.5), p["drive"])
    return 0.89 * y / (gk.peak(y) + 1e-12)


def render6(p: dict, rng: np.random.Generator) -> np.ndarray:
    """Version 6: a beat-repeat roll cut from a pitched source, normalised to an absolute peak of 0.89."""
    p = {**DEFAULTS6, **(p or {})}
    prng = gk.pattern_rng(p["pattern"], 61)
    ctx = _layout(p, p["dur"] * p["crackle"] if p["crackle"] > 0.02 else 0.0)
    src = source6(p, rng, prng)
    y, low = _bursts6(p, ctx, src, prng)
    return _post6(p, ctx, y, low, rng)


# ------------------------------------------------------------------------------------------------ measures


def _stereo(x: np.ndarray) -> np.ndarray:
    return x if x.ndim == 2 else np.column_stack([x, x])


def _block_db(m: np.ndarray, h: int) -> np.ndarray:
    k = max(1, len(m) // h)
    seg = m[:k * h].reshape(k, h) if len(m) >= h else m.reshape(1, -1)
    return 10 * np.log10(np.mean(seg ** 2, axis=1) + 1e-14)


def _shares(m: np.ndarray) -> tuple[float, float, list[float]]:
    P = np.abs(np.fft.rfft(m)) ** 2
    f = np.fft.rfftfreq(len(m), 1 / SR)
    tot = P.sum() + 1e-14
    cen = float((P * f).sum() / tot)
    s412 = float(10 * np.log10(P[(f >= 4000) & (f < 12000)].sum() / tot + 1e-14))
    edges = (0, 150, 1000, 4000, 12000, 24001)
    shares = [round(float(10 * np.log10(P[(f >= a) & (f < b)].sum() / tot + 1e-14)), 1)
              for a, b in zip(edges[:-1], edges[1:])]
    return cen, s412, shares


def _onsets(m: np.ndarray) -> list[int]:
    e = _block_db(m, 48)
    e = e - e.max()
    above = e > -14
    kept: list[int] = []
    for i in range(1, len(e)):
        if above[i] and not above[i - 1] and (not kept or i - kept[-1] >= 12):
            kept.append(i)
    return kept


def _gap_medians(onsets: list[int]) -> tuple[float | None, float | None]:
    gaps = np.diff(onsets).astype(float)
    if len(gaps) < 4:
        return None, None
    half = len(gaps) // 2
    return float(np.median(gaps[:half])), float(np.median(gaps[half:]))


def glitch_stats(x: np.ndarray) -> dict:
    """Model-free glitch measurements (10.7)."""
    x = _stereo(np.asarray(x, float))
    m = x.mean(axis=1)
    diff = x[:, 0] - x[:, 1]
    corr = 1.0 if np.std(diff) <= 1e-9 else float(np.corrcoef(x[:, 0], x[:, 1])[0, 1])
    cen, s412, shares = _shares(m)
    end_db = float(10 * np.log10(np.mean(m[-480:] ** 2) + 1e-14) - _block_db(m, 240).max())
    on = _onsets(m)
    early, late = _gap_medians(on)
    return {"corr": corr, "centroid_hz": cen, "s412_db": s412, "shares": shares, "end_db": end_db,
            "bursts": len(on), "early_ms": early, "late_ms": late,
            "med2_db": float(np.median(_block_db(m, 96)) - _block_db(m, 96).max())}


def _accel_term(s: dict) -> float:
    if s["early_ms"] is None:
        return 0.5
    return min(1.0, max(0.0, s["late_ms"] - s["early_ms"] + 2) / 8)


def study_extra(x: np.ndarray, row=None) -> float:
    """The pro glitch box: a penalty against the measured pro example, returned as -min(3, 0.6 sum)."""
    s = glitch_stats(x)
    share_err = sum(max(0.0, abs(a - b) - 6) / 12 for a, b in zip(s["shares"], PRO_SHARES))
    terms = [
        min(1.0, max(0.0, s["corr"] - 0.3) / 0.4),
        min(1.0, max(0.0, 6500 - s["centroid_hz"], s["centroid_hz"] - 11000) / 2500),
        min(1.0, max(0.0, -4 - s["s412_db"]) / 6),
        min(1.0, max(0.0, s["end_db"] + 30) / 10),
        0.5 if s["bursts"] < 6 else 0.0,
        _accel_term(s),
        min(1.5, share_err),
        min(1.0, max(0.0, s["med2_db"] + 29) / 6),
    ]
    return -min(3.0, 0.6 * sum(terms))


def generic_extra(x: np.ndarray, row=None) -> float:
    """Gesture rules only (no references): width, a stop on the cut, enough bursts, acceleration."""
    s = glitch_stats(x)
    total = (min(1.0, max(0.0, s["corr"] - 0.5) / 0.4) + min(1.0, max(0.0, s["end_db"] + 30) / 10)
             + (0.5 if s["bursts"] < 6 else 0.0) + _accel_term(s))
    return -min(3.0, 0.6 * total)
