"""burstglitch: a digital glitch that hits first, then accelerates into the cut. Chirps step down, never up."""

from __future__ import annotations

import types

import numpy as np

from hark.forge.engine import dsp
from hark.forge.recipes.risers import _glitchkit as gk
from hark.forge.recipes.risers._common import bw, crush, member_recipe, n_of, t_of

TARGET = "a short digital glitch burst"
ALTS = ["radio static", "a woodpecker", "rain on a tin roof", "a camera shutter"]
REFSET = "glitch"
TARGETS = {"glitch": TARGET}
MODULE = "hark.forge.recipes.risers.burstglitch"

SPACE = {
    "dur": (0.1, 0.8), "crackle": (0.0, 0.6), "crackle_db": (-45.0, -20.0), "crackle_rate": (40.0, 600.0, "log"),
    "crackle_hp": (1500.0, 9000.0, "log"), "grid0": (0.018, 0.07, "log"), "grid1": (0.008, 0.04, "log"),
    "accel": (0.5, 2.5, "log"), "gate": (0.25, 0.9), "decay": (0.15, 2.0, "log"), "accent_db": (0.0, 12.0),
    "fall_db": (-12.0, 6.0), "lvl_jit": (0.0, 8.0), "bits": (3.0, 10.0), "srate": (1500.0, 20000.0, "log"),
    "tonal": (0.0, 1.0), "tone_f": (300.0, 6000.0, "log"), "repeat": (0.0, 0.9), "lo_f": (150.0, 7000.0, "log"),
    "hi_f": (1500.0, 18000.0, "log"), "width": (0.0, 1.0), "low_db": (-50.0, 0.0), "low_f": (60.0, 500.0, "log"),
    "end_tail": (0.004, 0.06, "log"), "pattern": (0, 999, "int"),
}

_KEYS = list(SPACE)
_COLS = {
    "burst": (0.726, 0.4906, -35.95, 579.1, 7237, 0.0229, 0.02146, 1.588, 0.6991, 0.4964, 3.442, -12.0, 7.164,
              7.138, 5507, 0.02252, 3311, 0.5548, 5000, 16990, 1.0, -40.98, 156.6, 0.02849, 17),
    "mid": (0.2159, 0.1444, -32.74, 40.0, 3000, 0.03, 0.01798, 0.8936, 0.6, 0.7133, 3.068, -4.756, 2.992, 6.225,
            8000, 0.2935, 636.8, 0.371, 150, 2764, 0.1, -50.0, 150, 0.005826, 5),
    "chirp": (0.2788, 0.2608, -45.0, 94.3, 1734, 0.03896, 0.009084, 0.7294, 0.7683, 1.285, 6.973, -9.256, 2.413,
              4.329, 6357, 0.85, 446.1, 0.2995, 459.1, 6342, 0.7961, -50.0, 150, 0.02571, 29),
}
FAMILIES = {"glitch": [(name, {k: float(v) for k, v in zip(_KEYS, cols)}) for name, cols in _COLS.items()]}
DEFAULTS = dict(FAMILIES["glitch"][0][1])
NBUF = 3


def step_track(p: dict, size: int, prng: np.random.Generator) -> np.ndarray:
    """The pitch track of a tonal buffer: 1 to 3 steps of f x 2^(k / 6) that only step DOWN."""
    f = p["tone_f"] * 2.0 ** prng.uniform(-0.5, 0.5)
    steps = sorted((int(k) for k in prng.integers(-4, 5, int(prng.integers(1, 4)))), reverse=True)
    return gk.stepped_track(size, f, steps)


def _buffer(p: dict, size: int, rng, prng) -> np.ndarray:
    x = rng.standard_normal(size)
    if p["tonal"] > 0.02:
        sq = dsp.osc(step_track(p, size, prng), "square")
        x = (1 - 0.7 * p["tonal"]) * x / 3 + p["tonal"] * sq
    return crush(x / (gk.peak(x) + 1e-12), p["bits"], p["srate"])


def _level_db(p: dict, k: int, nb: int, prng) -> float:
    jit = abs(p["lvl_jit"] * prng.standard_normal())
    if k == 0:
        return 0.0
    return -p["accent_db"] + p["fall_db"] * (k - 1) / max(1, nb - 2) - jit


def _own_choice(p: dict, prev_idx: int, offset: int, prng) -> tuple[int, int]:
    idx = prev_idx if prng.random() < p["repeat"] else int(prng.integers(NBUF))
    return idx, offset


def _choices(p: dict, b: types.SimpleNamespace, prev: list[int], prng) -> tuple[tuple, tuple]:
    """(set, buffer, offset) for left and right; a right channel that is not its own reads the left choice."""
    left = (0, *_own_choice(p, prev[0], 0, prng))
    if prng.random() > p["width"]:
        return left, left
    off = n_of(prng.uniform(0, 0.002)) if b.inner else 0
    return left, (1, *_own_choice(p, prev[1], off, prng))


def _env(p: dict, m: int, d: float, last: bool, tail: int) -> np.ndarray:
    body = gk.decay_env(m, p["decay"] * d, 0.0004)
    if not last:
        return body
    end = body[-1] if m else 1.0
    return np.concatenate([body, end * np.exp(-t_of(tail) / (p["end_tail"] / 1.3))])


def _burst(p: dict, k: int, slot: tuple, ctx: types.SimpleNamespace, prng) -> types.SimpleNamespace:
    """Start, gain and envelope of burst k; the last burst runs to the cut and on into the tail."""
    t0, d = slot
    last = k == ctx.nb - 1
    start = gk.start_of(t0)
    g = max(16, ctx.n_end - start) if last else max(16, n_of(d * p["gate"]))
    m = g if last else min(g, ctx.size)
    return types.SimpleNamespace(start=start, inner=not last and k > 0, env=_env(p, m, d, last, ctx.tail),
                                 gain=10 ** (_level_db(p, k, ctx.nb, prng) / 20))


def _write(y: np.ndarray, b: types.SimpleNamespace, sets, choices, rng) -> None:
    for c, (s, idx, off) in enumerate(choices):
        seg = gk.body_segment(sets[s][idx], len(b.env))
        gk.add_clip(y[:, c], seg * b.env * b.gain * np.exp(0.1 * rng.standard_normal()), b.start + off)


def _bursts(p: dict, ctx: types.SimpleNamespace, sets, rng, prng) -> np.ndarray:
    y = np.zeros((ctx.n, 2))
    prev = [0, 0]
    for k, slot in enumerate(ctx.slots):
        b = _burst(p, k, slot, ctx, prng)
        choices = _choices(p, b, prev, prng)
        prev = [choices[0][1], choices[1][1]]
        _write(y, b, sets, choices, rng)
    return y


def _low_pulses(p: dict, slots, n: int, rng, prng) -> np.ndarray:
    low = np.zeros(n)
    for k, (t0, d) in enumerate(slots):
        start = gk.start_of(t0)
        m = min(n_of(d * p["gate"] + 0.01), n - start)
        f0 = p["low_f"] * 2.0 ** prng.uniform(-0.4, 0.4)
        ph = rng.uniform(0, 2 * np.pi)
        if m < 16:
            continue
        t = t_of(m)
        pulse = np.sin(2 * np.pi * f0 * t + ph) * np.exp(-t / max(0.004, 0.5 * d)) * gk.box(m, 0.001)
        gk.add_clip(low, pulse * (1.0 if k == 0 else 10 ** (-p["accent_db"] / 20)), start)
    return low


def _crackle(p: dict, n: int, t_st: float, rng) -> np.ndarray:
    def grain(u: float) -> np.ndarray:
        return gk.hann_grain(rng) * 10 ** ((18 * np.log10(rng.uniform(0.05, 1)) + 6 * u) / 20)
    chans = [bw(gk.grain_train(n, t_st, gk.accel_rate(p["crackle_rate"]), rng, grain), "high", p["crackle_hp"], 2)
             for _ in range(2)]
    return np.column_stack(chans)


def _layout(p: dict) -> types.SimpleNamespace:
    """Sizes, the crackle lead-in, the slot grid and the buffer length of one render."""
    n_end, tail = n_of(p["dur"]), n_of(5 * p["end_tail"])
    t_st = p["dur"] * p["crackle"]
    slots = gk.grid(t_st, p["dur"], p["grid0"], p["grid1"], p["accel"])
    return types.SimpleNamespace(n_end=n_end, tail=tail, n=n_end + tail, t_st=t_st, slots=slots, nb=len(slots),
                                 size=n_of(max(d for _, d in slots) + 0.004))


def _extras(p: dict, ctx: types.SimpleNamespace, pk: float, rng, prng) -> np.ndarray:
    """The low pulse under each burst and the crackle lead-in, both scaled to the bursts' peak pk."""
    out = np.zeros((ctx.n, 2))
    if p.get("low_db", -50.0) > -49.5:
        low = _low_pulses(p, ctx.slots, ctx.n, rng, prng)
        out += (pk * 10 ** (p["low_db"] / 20) * low / (gk.peak(low) + 1e-12))[:, None]
    if p["crackle"] > 0.02 and ctx.t_st > 0.01:
        cr = _crackle(p, ctx.n, ctx.t_st, rng)
        out += cr / (gk.peak(cr) + 1e-12) * pk * 10 ** (p["crackle_db"] / 20)
    return out


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """One take, (n, 2); n = samples of dur + samples of 5 end_tail; absolute peak 0.89."""
    p = {**DEFAULTS, **(p or {})}
    prng = gk.pattern_rng(p["pattern"], 101)
    ctx = _layout(p)
    sets = [[_buffer(p, ctx.size, rng, prng) for _ in range(NBUF)] for _ in range(2)]
    y = bw(_bursts(p, ctx, sets, rng, prng), "band", (p["lo_f"], max(p["hi_f"], 1.5 * p["lo_f"])), 2)
    y = bw(y + _extras(p, ctx, gk.peak(y), rng, prng), "low", 18500, 6)
    y[:2] = 0.0
    return dsp.fade(0.89 * y / (gk.peak(y) + 1e-12), 0.0005, 0.003)


_RECIPE = (MODULE, FAMILIES, SPACE, TARGETS, ALTS, REFSET)


def recipe_for(cat: str = "glitch", preset=0) -> types.SimpleNamespace:
    """A recipe object around one member, by index or by name."""
    return member_recipe(*_RECIPE, render, cat, preset)
