"""darkriser: a short dark riser of noise and sub through a speeding stutter gate, up to a cut. No rising tone."""

from __future__ import annotations

import types

import numpy as np

from hark.forge.engine import dsp
from hark.forge.recipes.risers._common import SR, add_at, bw, hann, member_recipe, n_of, poisson_times, svf, t_of

TARGET = "a short dark riser building into a hit"
ALTS = ["a jet engine", "a vacuum cleaner", "heavy rain on a roof", "a helicopter"]
REFSET = "riser"
TARGETS = {"riser": TARGET}
MODULE = "hark.forge.recipes.risers.darkriser"

SPACE = {
    "length": (0.3, 1.8), "start_db": (12.0, 48.0), "curve": (0.5, 3.5, "log"), "tail_tau": (0.01, 0.12, "log"),
    "body_f0": (60.0, 600.0, "log"), "body_f1": (150.0, 3000.0, "log"), "body_q": (0.5, 2.0),
    "sub_f": (32.0, 90.0, "log"), "sub_db": (-40.0, 12.0), "sub_drive": (0.0, 2.0), "crackle_db": (-50.0, -6.0),
    "crackle_rate": (20.0, 600.0, "log"), "crackle_hp": (1500.0, 9000.0, "log"), "gate_at": (0.2, 0.95),
    "gate_f0": (3.0, 16.0, "log"), "gate_f1": (10.0, 45.0, "log"), "gate_depth": (0.0, 1.0),
    "duty": (0.25, 0.85), "width": (0.0, 1.0), "room": (0.0, 0.4),
}

_KEYS = list(SPACE)
_COLS = {
    "dark": (0.74, 41.86, 1.378, 0.02767, 148.4, 502.3, 1.136, 47.85, -10.93, 0.7114, -50, 134.9, 3039, 0.7039,
             7.817, 19.91, 0.8088, 0.3958, 0.2022, 0.2256),
    "build": (1.2, 44.04, 0.8445, 0.07787, 94.27, 150.0, 2.0, 33.58, 3.405, 1.116, -6, 112.1, 9000, 0.2, 6.657,
              12.49, 0.75, 0.5742, 0.0, 0.1399),
    "snatch": (0.45, 24.0, 3.108, 0.02393, 279.2, 373.3, 2.0, 37.65, -13.57, 1.628, -22.68, 20.44, 4463, 0.337,
               7.732, 45.0, 0.2926, 0.3132, 0.0431, 0.2851),
}
FAMILIES = {"riser": [(name, {k: float(v) for k, v in zip(_KEYS, cols)}) for name, cols in _COLS.items()]}
DEFAULTS = dict(FAMILIES["riser"][0][1])


def level_db(p: dict, t: np.ndarray) -> np.ndarray:
    """Level in dB relative to the peak at t = length; an exponential fall after it."""
    T = p["length"]
    before = -p["start_db"] * (1 - np.clip(t / T, 0, 1)) ** p["curve"]
    after = -8.686 * (t - T) / p["tail_tau"]
    return np.where(t <= T, before, after)


def gate(p: dict, t: np.ndarray) -> np.ndarray:
    """The stutter gate (1 = open); fully open over the last 30 ms before the peak and after it."""
    T = p["length"]
    t0 = p["gate_at"] * T
    v = np.clip((t - t0) / max(T - t0, 0.001), 0, 1)
    rate = p["gate_f0"] * (p["gate_f1"] / p["gate_f0"]) ** v
    phase = np.cumsum(np.where(t >= t0, rate, 0.0)) / SR
    duty = p["duty"]
    frac = np.mod(phase + 0.5 * duty, 1.0)
    opened = np.where(frac < duty, 0.5 - 0.5 * np.cos(2 * np.pi * np.minimum(frac / duty, 1)), 0.0)
    g = np.where(t >= t0, 1 - p["gate_depth"] * (1 - opened), 1.0)
    k = np.clip((t - (T - 0.03)) / 0.03, 0, 1)
    g = g * (1 - k) + k
    return np.where(t > T, 1.0, g)


def _pink(n: int, rng) -> np.ndarray:
    """P13b: FFT pink of exactly n samples, unit standard deviation."""
    F = np.fft.rfft(rng.standard_normal(n))
    f = np.maximum(np.fft.rfftfreq(n, 1 / SR), 15.0)
    y = np.fft.irfft(F / np.sqrt(f), n)
    return y / (np.std(y) + 1e-12)


def _crackle(p: dict, n: int, rng) -> np.ndarray:
    T = p["length"]
    y = np.zeros(n)
    rate = lambda tg: p["crackle_rate"] * (0.15 + 0.85 * min(1.0, tg / T) ** 1.5)  # noqa: E731
    for tg in poisson_times(rng, T, rate):
        m = n_of(rng.uniform(0.003, 0.008))
        grain = rng.standard_normal(m) * hann(m)
        grain *= rng.uniform(0.05, 1.0) ** 0.6
        add_at(y, grain, int(round(tg * SR)))
    y = bw(y, "high", p["crackle_hp"], 2)
    return 0.25 * y / (np.sqrt(np.mean(y ** 2)) + 1e-12)


def _bodies(p: dict, n: int, t: np.ndarray, rng) -> tuple[np.ndarray, np.ndarray]:
    m = np.clip(t / p["length"], 0, 1)
    fc = p["body_f0"] * (p["body_f1"] / p["body_f0"]) ** (m ** 1.5)
    out = []
    for _ in range(2):
        b = svf(_pink(n, rng), fc, p["body_q"], "bandn")
        out.append(b / (np.sqrt(np.mean(b ** 2)) + 1e-12))
    return out[0], out[1]


def _sub(p: dict, t: np.ndarray, rng) -> np.ndarray:
    """A constant sub with a little tanh drive."""
    gd = 1 + 2 * p["sub_drive"]
    return np.tanh(gd * np.sin(2 * np.pi * p["sub_f"] * t + rng.uniform(0, 2 * np.pi))) / np.tanh(gd) / 0.7


def _channels(p: dict, n: int, t: np.ndarray, rng) -> tuple[tuple, tuple]:
    """(bodies, crackles) per channel: the lows stay identical, width blends in a second body above 200 Hz."""
    body1, body2 = _bodies(p, n, t, rng)
    cr1, cr2 = _crackle(p, n, rng), _crackle(p, n, rng)
    w = p["width"]
    kw = np.sqrt(max(0.0, 1 - w * w))
    hi1 = bw(body1, "high", 200, 2)
    right = (body1 - hi1) + kw * hi1 + w * bw(body2, "high", 200, 2)
    return (body1, right), (cr1, kw * cr1 + w * cr2)


def _mix(p: dict, n: int, rng) -> np.ndarray:
    t = t_of(n)
    a = 10 ** (np.maximum(level_db(p, t), -120) / 20) * gate(p, t)
    sub = 10 ** (p["sub_db"] / 20) * _sub(p, t, rng)
    bodies, crackles = _channels(p, n, t, rng)
    cg = 10 ** (p["crackle_db"] / 20) / 0.25
    return np.column_stack([(bodies[c] + sub + cg * crackles[c]) * a for c in range(2)])


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """One take, (n, 2), normalised to an absolute peak of 0.89."""
    p = {**DEFAULTS, **(p or {})}
    tail = p["tail_tau"] * 60 / 8.686
    n = n_of(p["length"] + tail + 0.02)
    y = _mix(p, n, rng)
    if p["room"] > 0.01:
        y = dsp.room(y, rng, rt60=0.35, size=0.4, damp=0.6, wet=p["room"])[:n]
    y = dsp.fade(bw(bw(y, "high", 25, 2), "low", 18000, 4), 0.002, min(0.02, 0.5 * tail))
    return 0.89 * y / (np.max(np.abs(y)) + 1e-12)


_RECIPE = (MODULE, FAMILIES, SPACE, TARGETS, ALTS, REFSET)


def recipe_for(cat: str = "riser", preset=0) -> types.SimpleNamespace:
    """A recipe object around one member, by index or by name."""
    return member_recipe(*_RECIPE, render, cat, preset)
