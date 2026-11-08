"""revcymbal: a reverse-cymbal swell built forward in time, so the stop after the peak can be shaped freely."""

from __future__ import annotations

import types

import numpy as np

from hark.forge.engine import dsp, fx
from hark.forge.recipes.risers._common import bw, member_recipe, n_of, svf, t_of

TARGET = "a reverse cymbal swell"
ALTS = ["a jet engine", "a wind gust", "rain", "a sword being sharpened"]
REFSET = "reverse_swell"
TARGETS = {"reverse_swell": TARGET}
MODULE = "hark.forge.recipes.risers.revcymbal"

SPACE = {
    "length": (0.3, 2.0), "swell_db": (12.0, 60.0), "curve": (0.5, 3.5, "log"), "hold": (0.0, 0.45),
    "hold_db": (-12.0, 0.0), "tail_tau": (0.008, 0.6, "log"), "cym_f": (300.0, 7000.0, "log"), "bw": (1.0, 6.0),
    "metal": (0.0, 1.0), "partials": (12, 160, "int"), "shimmer": (0.0, 1.0), "open": (0.0, 4.0),
    "hp": (80.0, 2500.0, "log"), "low_db": (-50.0, 0.0), "flut_depth": (0.0, 1.0), "flut_f": (6.0, 60.0, "log"),
    "width": (0.0, 1.0), "room": (0.0, 0.5), "rt60": (0.2, 2.0, "log"),
}

_KEYS = list(SPACE)
_COLS = {
    "cymbal": (0.5037, 19.55, 1.831, 0.22, -8.039, 0.03096, 2600, 3.469, 0.6317, 78, 0.5093, 0.0, 118.7, -50,
               0.6266, 18.8, 0.98, 0.0, 1.386),
    "ring": (0.808, 40.86, 2.308, 0.0, -2.062, 0.1542, 2070, 5.575, 0.7482, 79, 0.8912, 3.093, 658.9, -50, 0.4417,
             21.97, 1.0, 0.129, 1.209),
    "dark": (0.5391, 30.0, 0.5, 0.03684, -0.4249, 0.03842, 346, 3.456, 0.259, 117, 0.3166, 2.554, 80.0, 0.0,
             0.004984, 28.16, 0.35, 0.08302, 0.5783),
    "short": (0.33, 24.0, 0.8282, 0.08, -6.0, 0.025, 3200, 3.2, 0.6, 70, 0.04221, 0.5, 300, -50, 0.8351, 24.0,
              0.95, 0.05, 0.8),
    "breath": (0.7, 30.0, 1.3, 0.094, 0.0, 0.06, 1200, 1.499, 0.12, 60, 0.3, 1.5, 150, -24, 0.366, 14.0, 0.5818,
               0.2272, 0.5846),
}
FAMILIES = {"reverse_swell": [(name, {k: float(v) for k, v in zip(_KEYS, cols)}) for name, cols in _COLS.items()]}
DEFAULTS = dict(FAMILIES["reverse_swell"][0][1])


def level_db(p: dict, t: np.ndarray) -> tuple[np.ndarray, float]:
    """Level in dB relative to the peak (9.5), and the peak time."""
    T, hd = p["length"], p["hold"]
    rise = -p["swell_db"] * (1 - np.clip(t / T, 0, 1)) ** p["curve"]
    plateau = p["hold_db"] * (t - T) / hd if hd > 0 else np.zeros_like(t)
    fall = p["hold_db"] - 8.686 * (t - T - hd) / p["tail_tau"]
    out = np.where(t <= T, rise, np.where(t <= T + hd, plateau, fall))
    return out, T


def _rms(x: np.ndarray) -> float:
    return float(np.sqrt(np.mean(x ** 2))) + 1e-12


def _control(n: int, fc: float, rng: np.random.Generator) -> np.ndarray:
    """P6: white noise low-passed (order 2) at fc, unit standard deviation."""
    y = bw(rng.standard_normal(n), "low", fc, 2)
    return y / (np.std(y) + 1e-12)


def _metal(p: dict, n: int, lo: float, hi: float, rng: np.random.Generator) -> np.ndarray:
    k = int(round(p["partials"]))
    f = np.exp(rng.uniform(np.log(lo), np.log(hi), k))
    sigma = max(0.3, p["bw"] / 3)
    wts = np.exp(-0.5 * (np.log2(f / p["cym_f"]) / sigma) ** 2) * rng.uniform(0.4, 1.0, k)
    ctrl = _control(n, 3.0, rng)
    shifts = rng.integers(0, n, k)
    phases = rng.uniform(0, 2 * np.pi, k)
    t = t_of(n)
    y = np.zeros(n)
    for j in range(k):
        am = 1 + 0.35 * p["shimmer"] * np.roll(ctrl, int(shifts[j]))
        y += wts[j] * am * np.sin(2 * np.pi * f[j] * t + phases[j])
    return y / _rms(y)


def texture(p: dict, n: int, rng: np.random.Generator) -> tuple[np.ndarray, float]:
    """One channel of metallic partials blended with band noise (9.6); returns (x, hi)."""
    lo = max(60.0, p["cym_f"] / 2 ** (p["bw"] / 2))
    hi = min(18000.0, p["cym_f"] * 2 ** (p["bw"] / 2))
    metal = _metal(p, n, lo, hi, rng)
    noise = bw(rng.standard_normal(n), "band", (lo, hi), 2)
    noise = dsp.peq(noise, p["cym_f"], 6.0, 1.0)
    noise = noise / _rms(noise)
    x = p["metal"] * metal + (1 - p["metal"]) * noise
    return x / _rms(x), hi


def _flutter(p: dict, n: int, rng: np.random.Generator) -> np.ndarray:
    d = p["flut_depth"]
    return np.exp(d * _control(n, p["flut_f"], rng) - 0.5 * d * d)


def _channels(p: dict, n: int, rng: np.random.Generator):
    """Left and right (texture, low body, flutter) with correlation about sqrt(1 - width^2)."""
    xl, hi = texture(p, n, rng)
    xr, _ = texture(p, n, rng)
    lows = [bw(rng.standard_normal(n), "band", (100.0, 450.0), 2) for _ in range(2)]
    fls = [_flutter(p, n, rng) for _ in range(2)]
    w = p["width"]
    k = np.sqrt(max(0.0, 1 - w * w))
    left = (xl, lows[0], fls[0])
    right = (k * xl + w * xr, k * lows[0] + w * lows[1], k * fls[0] + w * fls[1])
    return (left, right), hi


def _edit_end(y: np.ndarray, p: dict) -> int:
    """Cut 20 ms after the first 5 ms block, at or after the loudest, that is 60 dB under it (9.7 step 9)."""
    m = y.mean(axis=1)
    h = 240
    k = max(1, len(m) // h)
    e = 20 * np.log10(np.sqrt(np.mean(m[:k * h].reshape(k, h) ** 2, axis=1)) + 1e-12) if len(m) >= h else \
        np.array([0.0])
    i = int(np.argmax(e))
    quiet = np.nonzero(e[i:] < e[i] - 60)[0]
    block = i + int(quiet[0]) if len(quiet) else k
    return min(len(y), block * h + n_of(0.02))


def _sizes(p: dict) -> int:
    """9.7 step 1: the peak, the plateau, the fall to -60 dB and 20 ms, plus some room tail; at most 4 s."""
    tail = p["length"] + p["hold"] + p["tail_tau"] * 60 / 8.686 + 0.02
    return n_of(min(4.0, tail + 0.3 * p["rt60"] * min(1.0, 4 * p["room"])))


def _shaped(p: dict, chans, hi: float, a: np.ndarray) -> np.ndarray:
    """9.7 steps 5 and 6: a low-pass that opens as the level grows, the low body, the level and flutter."""
    cut = np.clip(1.3 * hi * 2.0 ** (-p["open"] * (1 - np.sqrt(a))), 300, 20000)
    out = []
    for x, low, flut in chans:
        y = svf(x, cut, 0.7, "low")
        if p["low_db"] > -49.5:
            y = y + 10 ** (p["low_db"] / 20) * low / _rms(low)
        out.append(y * a * flut)
    return bw(np.column_stack(out), "high", p["hp"], 2)


def _room(p: dict, y: np.ndarray, rng) -> np.ndarray:
    """The forge-core reverb, cut to the dry length; a non-finite result keeps the dry signal."""
    wet = fx.reverb(y, rng, rt60=p["rt60"], size=0.5, predelay=0.008, damp=0.4, wet=p["room"], width=1.0,
                    lines=16, stereo=True, tail=max(0.2, p["rt60"]))[:len(y)]
    return wet if np.all(np.isfinite(wet)) and len(wet) == len(y) else y


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """One take, (n, 2), normalised to an absolute peak of 0.89."""
    p = {**DEFAULTS, **(p or {})}
    n = _sizes(p)
    a = 10 ** (np.maximum(level_db(p, t_of(n))[0], -120) / 20)
    chans, hi = _channels(p, n, rng)
    y = _shaped(p, chans, hi, a)
    if p["room"] > 0.01:
        y = _room(p, y, rng)
    y = bw(y, "low", 19000, 4)
    y = dsp.fade(y[:_edit_end(y, p)], 0.004, 0.02)
    return 0.89 * y / (np.max(np.abs(y)) + 1e-12)


_RECIPE = (MODULE, FAMILIES, SPACE, TARGETS, ALTS, REFSET)


def recipe_for(cat: str = "reverse_swell", preset=0) -> types.SimpleNamespace:
    """A recipe object around one member, by index or by name."""
    return member_recipe(*_RECIPE, render, cat, preset)
