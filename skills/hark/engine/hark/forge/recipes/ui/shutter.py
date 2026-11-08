"""Recipe shutter: a camera shutter in 2 mechanical stages (release, then landing)."""

from __future__ import annotations

import math

import numpy as np

from hark.forge.engine import dsp
from hark.forge.recipes import tonal

TARGET = "a camera shutter click"
TARGETS = {"shutter": TARGET}
ALTS = ["a door latch", "a stapler", "a mouse click", "a light switch", "a gun being cocked"]

SPACE = {
    "gap": (0.015, 0.14, "log"), "a_db": (-24.0, 6.0), "a_hz": (1500, 9000, "log"), "a_ms": (0.2, 3, "log"),
    "a_modes": (0.0, 1.0), "whirr": (-60.0, -6.0), "whirr_hz": (600, 5000, "log"), "b_hz": (300, 3000, "log"),
    "b_decay": (0.004, 0.08, "log"), "b_noise": (-40.0, 0.0), "b_noise_hz": (600, 6000, "log"),
    "b_contact": (0.0002, 0.003, "log"), "body": (-60.0, 0.0), "body_f": (70, 300, "log"),
    "body_ms": (4, 50, "log"), "rattle": (-60.0, -6.0), "rattle_hz": (2000, 7000, "log"), "hp": (60, 500, "log"),
    "lp": (3000, 16000, "log"), "room": (0.0, 0.3), "rt": (0.05, 0.4, "log"), "width": (0.0, 0.5),
}

DEFAULTS = {
    "gap": 0.045, "a_db": -6.0, "a_hz": 3500.0, "a_ms": 0.8, "a_modes": 0.5, "whirr": -30.0, "whirr_hz": 1800.0,
    "b_hz": 900.0, "b_decay": 0.025, "b_noise": -10.0, "b_noise_hz": 2000.0, "b_contact": 0.0008, "body": -16.0,
    "body_f": 150.0, "body_ms": 14.0, "rattle": -30.0, "rattle_hz": 3500.0, "hp": 90.0, "lp": 9000.0,
    "room": 0.1, "rt": 0.14, "width": 0.06,
}

FINE = ["a_db", "a_hz", "a_ms", "a_modes", "whirr", "whirr_hz", "b_decay", "b_noise", "b_noise_hz", "b_contact",
        "body", "body_f", "body_ms", "rattle", "rattle_hz", "hp", "lp", "room", "rt", "width"]

BOX_MODES = [(1.0, 1.0, 1.0), (1.62, 0.7, 0.7), (2.31, 0.5, 0.5), (3.17, 0.35, 0.35), (4.4, 0.2, 0.25)]
LATCH_MODES = [(1.0, 1.0), (1.71, 0.6), (2.63, 0.35)]

STUDY = {
    "dur_s": (0.16, 0.38, 0.08), "attack_ms": (16, 170, 20), "t20_ms": (20, 123, 30),
    "centroid_hz": (624, 2187, 300), "lo150": (0.01, 0.24, 0.1), "tonal": (0.3, 0.52, 0.1),
    "corr": (0.96, 0.99, 0.1),
}

_EDITS = {
    "dslr": {"gap": 0.0391, "a_db": -23.5, "a_hz": 5537, "a_ms": 0.5005, "a_modes": 0.3966, "whirr": -9.218,
             "whirr_hz": 1085, "b_hz": 1433, "b_decay": 0.006, "b_noise": -1.243, "b_noise_hz": 1355,
             "b_contact": 0.00221, "body": -20.61, "body_f": 238.2, "body_ms": 19.41, "rattle": -26.17,
             "rattle_hz": 5575, "hp": 92.91, "lp": 5091, "room": 0.2557, "rt": 0.2552, "width": 0.1069},
    "snap": {"gap": 0.05821, "a_db": -22.88, "a_hz": 3300, "a_ms": 2.964, "a_modes": 0.1957, "whirr": -15.59,
             "whirr_hz": 5000, "b_hz": 2037, "b_decay": 0.004, "b_noise": -2.783, "b_noise_hz": 2530,
             "b_contact": 0.00031, "body": -10.33, "body_f": 156.8, "body_ms": 5.495, "rattle": -48.36,
             "rattle_hz": 2581, "hp": 173.8, "lp": 9437, "room": 0.01662, "rt": 0.0526, "width": 0.234},
    "mirror": {"gap": 0.06462, "a_db": -18.51, "a_hz": 8113, "a_ms": 0.5578, "a_modes": 0.1115, "whirr": -6.0,
               "whirr_hz": 688.7, "b_hz": 1511, "b_decay": 0.00622, "b_noise": 0.0, "b_noise_hz": 1468,
               "b_contact": 0.003, "body": -33.33, "body_f": 300, "body_ms": 22.0, "rattle": -26.59,
               "rattle_hz": 7000, "hp": 128.8, "lp": 6009, "room": 0.3, "rt": 0.374, "width": 0.09233},
    "film": {"gap": 0.095, "a_db": -12.29, "a_hz": 4200, "a_ms": 0.9, "whirr": -14.0, "whirr_hz": 900, "b_hz": 720,
             "b_decay": 0.01103, "b_noise": -7.79, "b_noise_hz": 600, "b_contact": 0.001728, "body": -12.0,
             "body_f": 112.7, "body_ms": 26.32, "rattle": -24.0, "rattle_hz": 4200, "hp": 84.56, "lp": 6500,
             "room": 0.3, "rt": 0.2, "width": 0.08},
    "quick": {"gap": 0.021, "a_db": -4.0, "a_hz": 3800, "a_ms": 0.2281, "a_modes": 0.4098, "whirr": -40.0,
              "b_hz": 1250, "b_decay": 0.008, "b_noise": -6.0, "b_noise_hz": 2200, "b_contact": 0.001477,
              "body": -17.09, "body_f": 199.6, "body_ms": 8.0, "rattle": -50.0, "hp": 150, "room": 0.09189,
              "rt": 0.2138, "width": 0.1},
}

FAMILY_NAMES = {"shutter": list(_EDITS)}
FAMILY = {"shutter": [dict(DEFAULTS, **_EDITS[name]) for name in _EDITS]}
FAMILIES = {"shutter": list(zip(FAMILY_NAMES["shutter"], FAMILY["shutter"]))}


def family_space(params: dict) -> dict:
    """The full space with gap and b_hz pinned to the member's values (T15)."""
    return tonal.family_space(SPACE, _member(params), FINE)


def _member(params: dict) -> dict:
    """A full parameter set: the defaults under the member's values."""
    return dict(DEFAULTS, **params)


def measure(x: np.ndarray) -> dict:
    """The catalog measurement (T14)."""
    return tonal.measure(x)


def study_penalty(x: np.ndarray) -> float:
    """Distance of a render from the film shutter targets (T15)."""
    return float(tonal.outside(tonal.measure(x), STUDY))


def study_extra(x: np.ndarray, row: dict | None = None) -> float:
    """Critic extra term: minus the study distance."""
    return -study_penalty(x)


def _n(seconds: float) -> int:
    return max(1, int(round(seconds * tonal.SR)))


def _peak1(x: np.ndarray) -> np.ndarray:
    return x / (float(np.max(np.abs(x))) + 1e-12)


def _stage_a(p: dict, r: np.random.Generator, a_hz: float) -> np.ndarray:
    ka = _n(0.03)
    a = tonal.burst(ka, r, a_hz / 1.8, min(1.8 * a_hz, 18000.0), p["a_ms"] / 1000, rise=0.0002)
    if p["a_modes"] > 0.005:
        ratios = [q for q, _ in LATCH_MODES]
        am = tonal.modes(ka, r, [a_hz * q for q in ratios], [g for _, g in LATCH_MODES], [0.004 / q for q in ratios],
                         jitter=0.01)
        a = a + p["a_modes"] * _peak1(am)
    return _peak1(a)


def _stage_b(p: dict, r: np.random.Generator, kb: int, b_hz: float) -> np.ndarray:
    freqs = [b_hz * q for q, _, _ in BOX_MODES]
    taus = [max(0.0008, p["b_decay"] * d) for _, _, d in BOX_MODES]
    bm = tonal.modes(kb, r, freqs, [a for _, a, _ in BOX_MODES], taus, jitter=0.01)
    b = _peak1(np.asarray(tonal.strike(bm, r, p["b_contact"], bright=0.6))[:kb])
    if p["b_noise"] > -39:
        hz = p["b_noise_hz"]
        b = b + 10 ** (p["b_noise"] / 20) * tonal.burst(kb, r, hz / 2, 2 * hz, 0.001, rise=0.0002)
    if p["body"] > -59:
        b = b + 10 ** (p["body"] / 20) * tonal.thump(kb, p["body_f"], p["body_ms"] / 1000, rise=0.0008)
    if p["rattle"] > -59:
        b = b + _rattle(p, r, kb)
    return b


def _rattle(p: dict, r: np.random.Generator, kb: int) -> np.ndarray:
    """Stage B rattle: a band burst at rattle_hz that starts 4 ms late."""
    hz = p["rattle_hz"]
    rr = tonal.burst(kb, r, hz / 1.6, 1.6 * hz, 0.006, rise=0.002)
    s = min(kb, _n(0.004))
    return 10 ** (p["rattle"] / 20) * np.concatenate([np.zeros(s), rr])[:kb]


def _whirr(p: dict, r: np.random.Generator, gap: float) -> np.ndarray:
    kw = _n(gap)
    hz = p["whirr_hz"]
    w = dsp.butter(r.standard_normal(kw), "band", (hz / 1.7, 1.7 * hz), 2)
    w = w * np.sin(np.pi * np.linspace(0, 1, kw)) ** 0.7
    return 10 ** (p["whirr"] / 20) * _peak1(w)


def _stages(p: dict, rng: np.random.Generator, gap: float, a_hz: float, b_hz: float) -> np.ndarray:
    """Stage A at a_db, then stage B gap seconds later, on a buffer of at most 1 s."""
    tail = 7 * max(p["b_decay"], p["body_ms"] / 1000 if p["body"] > -59 else 0.0)
    n = _n(min(1.0, gap + tail + 0.03))
    a = _stage_a(p, rng, a_hz)
    kb = max(1, n - _n(gap))
    b = _stage_b(p, rng, kb, b_hz)
    y = np.zeros(n)
    ka = min(len(a), n)
    y[:ka] += 10 ** (p["a_db"] / 20) * a[:ka]
    y[n - kb:] += b
    return y


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """One shutter take: stereo float64 at 48 kHz, at most about 1 s."""
    p = dict(DEFAULTS, **(p or {}))
    gap = p["gap"] * math.exp(0.03 * rng.standard_normal())
    b_hz = p["b_hz"] * math.exp(0.03 * rng.standard_normal())
    a_hz = p["a_hz"] * math.exp(0.03 * rng.standard_normal())
    y = _stages(p, rng, gap, a_hz, b_hz)
    n = len(y)
    if p["whirr"] > -59:
        w = _whirr(p, rng, gap)
        y[: min(n, len(w))] += w[: min(n, len(w))]
    st = tonal.finish(y, rng, hp=p["hp"], lp=p["lp"], room_wet=p["room"], rt60=p["rt"], width=p["width"], cut=600)
    return np.asarray(st, dtype=np.float64)
