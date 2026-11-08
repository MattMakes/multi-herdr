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
FAMILIES = {"riser": [(label, {kn: float(v) for kn, v in zip(_KEYS, cols)}) for label, cols in _COLS.items()]}
DEFAULTS = dict(FAMILIES["riser"][0][1])


def level_db(cfg: dict, tax: np.ndarray) -> np.ndarray:
    """Level in dB relative to the peak at t = length; an exponential fall after it."""
    dur = cfg["length"]
    before = -cfg["start_db"] * (1 - np.clip(tax / dur, 0, 1)) ** cfg["curve"]
    after = -8.686 * (tax - dur) / cfg["tail_tau"]
    return np.where(tax <= dur, before, after)


def gate(cfg: dict, tax: np.ndarray) -> np.ndarray:
    """The stutter gate (1 = open); fully open over the last 30 ms before the peak and after it."""
    dur = cfg["length"]
    onset = cfg["gate_at"] * dur
    v = np.clip((tax - onset) / max(dur - onset, 0.001), 0, 1)
    rate = cfg["gate_f0"] * (cfg["gate_f1"] / cfg["gate_f0"]) ** v
    phase = np.cumsum(np.where(tax >= onset, rate, 0.0)) / SR
    duty = cfg["duty"]
    frac = np.mod(phase + 0.5 * duty, 1.0)
    opened = np.where(frac < duty, 0.5 - 0.5 * np.cos(2 * np.pi * np.minimum(frac / duty, 1)), 0.0)
    g = np.where(tax >= onset, 1 - cfg["gate_depth"] * (1 - opened), 1.0)
    kn = np.clip((tax - (dur - 0.03)) / 0.03, 0, 1)
    g = g * (1 - kn) + kn
    return np.where(tax > dur, 1.0, g)


def _pink(ns: int, gen) -> np.ndarray:
    """P13b: FFT pink of exactly n samples, unit standard deviation."""
    F = np.fft.rfft(gen.standard_normal(ns))
    freq = np.maximum(np.fft.rfftfreq(ns, 1 / SR), 15.0)
    sig = np.fft.irfft(F / np.sqrt(freq), ns)
    return sig / (np.std(sig) + 1e-12)


def _crackle(cfg: dict, ns: int, gen) -> np.ndarray:
    dur = cfg["length"]
    sig = np.zeros(ns)
    rate = lambda tg: cfg["crackle_rate"] * (0.15 + 0.85 * min(1.0, tg / dur) ** 1.5)  # noqa: E731
    for tg in poisson_times(gen, dur, rate):
        mv = n_of(gen.uniform(0.003, 0.008))
        grain = gen.standard_normal(mv) * hann(mv)
        grain *= gen.uniform(0.05, 1.0) ** 0.6
        add_at(sig, grain, int(round(tg * SR)))
    sig = bw(sig, "high", cfg["crackle_hp"], 2)
    return 0.25 * sig / (np.sqrt(np.mean(sig ** 2)) + 1e-12)


def _bodies(cfg: dict, ns: int, tax: np.ndarray, gen) -> tuple[np.ndarray, np.ndarray]:
    mv = np.clip(tax / cfg["length"], 0, 1)
    cut = cfg["body_f0"] * (cfg["body_f1"] / cfg["body_f0"]) ** (mv ** 1.5)
    res = []
    for _ in range(2):
        bv = svf(_pink(ns, gen), cut, cfg["body_q"], "bandn")
        res.append(bv / (np.sqrt(np.mean(bv ** 2)) + 1e-12))
    return res[0], res[1]


def _sub(cfg: dict, tax: np.ndarray, gen) -> np.ndarray:
    """A constant sub with a little tanh drive."""
    gd = 1 + 2 * cfg["sub_drive"]
    return np.tanh(gd * np.sin(2 * np.pi * cfg["sub_f"] * tax + gen.uniform(0, 2 * np.pi))) / np.tanh(gd) / 0.7


def _channels(cfg: dict, ns: int, tax: np.ndarray, gen) -> tuple[tuple, tuple]:
    """(bodies, crackles) per channel: the lows stay identical, width blends in a second body above 200 Hz."""
    body1, body2 = _bodies(cfg, ns, tax, gen)
    cr1, cr2 = _crackle(cfg, ns, gen), _crackle(cfg, ns, gen)
    wv = cfg["width"]
    extra = np.sqrt(max(0.0, 1 - wv * wv))
    hi1 = bw(body1, "high", 200, 2)
    right = (body1 - hi1) + extra * hi1 + wv * bw(body2, "high", 200, 2)
    return (body1, right), (cr1, extra * cr1 + wv * cr2)


def _mix(cfg: dict, ns: int, gen) -> np.ndarray:
    tax = t_of(ns)
    amp = 10 ** (np.maximum(level_db(cfg, tax), -120) / 20) * gate(cfg, tax)
    sub = 10 ** (cfg["sub_db"] / 20) * _sub(cfg, tax, gen)
    bodies, crackles = _channels(cfg, ns, tax, gen)
    cg = 10 ** (cfg["crackle_db"] / 20) / 0.25
    return np.column_stack([(bodies[cv] + sub + cg * crackles[cv]) * amp for cv in range(2)])


def render(cfg: dict, gen: np.random.Generator) -> np.ndarray:
    """One take, (n, 2), normalised to an absolute peak of 0.89."""
    cfg = {**DEFAULTS, **(cfg or {})}
    tail = cfg["tail_tau"] * 60 / 8.686
    ns = n_of(cfg["length"] + tail + 0.02)
    sig = _mix(cfg, ns, gen)
    if cfg["room"] > 0.01:
        sig = dsp.room(sig, gen, rt60=0.35, size=0.4, damp=0.6, wet=cfg["room"])[:ns]
    sig = dsp.fade(bw(bw(sig, "high", 25, 2), "low", 18000, 4), 0.002, min(0.02, 0.5 * tail))
    return 0.89 * sig / (np.max(np.abs(sig)) + 1e-12)


_RECIPE = (MODULE, FAMILIES, SPACE, TARGETS, ALTS, REFSET)


def recipe_for(cat: str = "riser", preset=0) -> types.SimpleNamespace:
    """A recipe object around one member, by index or by name."""
    return member_recipe(*_RECIPE, render, cat, preset)
