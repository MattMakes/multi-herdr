"""Recipe ui_pop: the soft, rounded mid-register pop of music-led product films."""

from __future__ import annotations

import math

import numpy as np

from hark.forge.recipes import tonal

TARGET = "a soft rounded pop, a small bubble popping in a user interface"
TARGETS = {"ui_pop": TARGET}
ALTS = ["a mouse click", "a water drop", "a wooden knock", "a synth beep", "a finger snap"]

SPACE = {
    "f0": (250, 1200, "log"), "bend": (-3.0, 72.0), "ptau": (0.004, 0.08, "log"), "sweep": (0.0, 0.15),
    "ease": (0.4, 2.5), "onset": (0.004, 0.07, "log"), "curve": (0.6, 2.5), "hold": (0.0, 0.03),
    "tau": (0.004, 0.09, "log"), "dcurve": (1.0, 2.5), "h2": (0.0, 0.8), "h3": (0.0, 0.5), "hdec": (1.0, 3.0),
    "inh": (0.0, 0.08), "m2": (0.0, 0.8), "m2r": (1.4, 4.5), "m2dec": (1.0, 4.0), "breath": (-60.0, -2.0),
    "bq": (1.0, 8.0, "log"), "bdec": (0.4, 2.0), "puff": (-60.0, -6.0), "puff_hz": (600, 5000, "log"),
    "puff_ms": (0.5, 8, "log"), "thump": (-60.0, -6.0), "thump_f": (70, 200, "log"), "click": (-60.0, -10.0),
    "click_hz": (1500, 8000, "log"), "dbl": (-60.0, 0.0), "dgap": (0.03, 0.12), "dratio": (0.6, 1.6),
    "hp": (60, 300, "log"), "lp": (2500, 12000, "log"), "room": (0.0, 0.35), "rt": (0.06, 0.35, "log"),
    "width": (0.0, 1.5), "jit": (0.0, 1.0),
}

DEFAULTS = {
    "f0": 370.0, "bend": 0.7, "ptau": 0.0075, "sweep": 0.0, "ease": 1.0, "onset": 0.0236, "curve": 2.45,
    "hold": 0.0147, "tau": 0.041, "dcurve": 2.12, "h2": 0.57, "h3": 0.35, "hdec": 1.0, "inh": 0.0, "m2": 0.66,
    "m2r": 1.72, "m2dec": 1.43, "breath": -14.0, "bq": 3.6, "bdec": 0.6, "puff": -40.0, "puff_hz": 4270.0,
    "puff_ms": 2.7, "thump": -15.7, "thump_f": 129.0, "click": -60.0, "click_hz": 3500.0, "dbl": -60.0,
    "dgap": 0.07, "dratio": 0.8, "hp": 112.0, "lp": 3530.0, "room": 0.15, "rt": 0.1, "width": 0.1, "jit": 0.5,
}

FINE = ["ptau", "ease", "onset", "curve", "hold", "tau", "dcurve", "h2", "h3", "hdec", "inh", "m2dec", "breath",
        "bq", "bdec", "puff", "puff_hz", "puff_ms", "thump", "thump_f", "click", "click_hz", "hp", "lp", "room",
        "rt", "width"]

STUDY = {
    "dur_s": (0.10, 0.24, 0.08), "attack_ms": (18, 62, 15), "t20_ms": (57, 129, 30),
    "centroid_hz": (475, 830, 200), "lo150": (0.0, 0.14, 0.1), "tonal": (0.48, 0.63, 0.1), "corr": (0.93, 1.0, 0.1),
}

_EDITS = {
    "soft": {},
    "step": {"f0": 466.16, "bend": 3.0, "ptau": 0.03},
    "boop": {"f0": 357.1, "bend": 9.0, "ptau": 0.03, "onset": 0.025, "curve": 2.0, "hold": 0.0062, "tau": 0.026,
             "dcurve": 1.4, "h2": 0.61, "h3": 0.17, "hdec": 1.63, "m2": 0.79, "m2r": 3.69, "m2dec": 2.68,
             "breath": -13.8, "bq": 7.0, "bdec": 0.4, "puff": -51.0, "puff_hz": 4600, "puff_ms": 1.9, "thump": -8.0,
             "thump_f": 76, "click": -54.0, "hp": 95, "lp": 12000, "room": 0.115, "rt": 0.093, "width": 0.2},
    "blip": {"f0": 889.1, "bend": -3.0, "ptau": 0.08, "ease": 1.576, "onset": 0.01359, "curve": 2.5,
             "hold": 0.00311, "tau": 0.01052, "dcurve": 2.007, "h2": 0.3, "h3": 0.09244, "hdec": 1.107,
             "inh": 0.01188, "m2": 0.2913, "m2r": 4.115, "m2dec": 1.032, "breath": -21.12, "bq": 2.26,
             "bdec": 1.076, "puff": -46.31, "puff_hz": 4264, "puff_ms": 0.5636, "thump": -36.45, "thump_f": 108.6,
             "click": -45.55, "click_hz": 4013, "dbl": -55.33, "dgap": 0.05156, "dratio": 0.7141, "hp": 60,
             "lp": 12000, "room": 0.0, "rt": 0.2241, "width": 1.341, "jit": 0.6717},
    "fall": {"f0": 345.76, "bend": 7.36, "ptau": 0.0058, "sweep": 0.0894, "ease": 1.123, "onset": 0.0177,
             "curve": 1.658, "hold": 0.0006, "tau": 0.0225, "dcurve": 2.138, "h2": 0.671, "h3": 0.49, "hdec": 1.424,
             "m2": 0.53, "m2r": 4.5, "m2dec": 1.0, "breath": -8.81, "bq": 2.61, "bdec": 1.89, "puff": -47.5,
             "puff_hz": 5000, "puff_ms": 6.13, "thump": -26.1, "thump_f": 81, "click": -33.5, "click_hz": 4083,
             "dbl": -29.6, "dgap": 0.113, "dratio": 1.155, "hp": 79.7, "lp": 11076, "room": 0.0, "rt": 0.076,
             "width": 0.64, "jit": 0.83},
    "wide": {"f0": 622.5, "bend": -0.9783, "ptau": 0.08, "sweep": 0.00291, "ease": 1.822, "onset": 0.04115,
             "curve": 2.5, "hold": 0.00244, "tau": 0.03267, "dcurve": 1.844, "h2": 0.5426, "h3": 0.1129,
             "hdec": 1.054, "inh": 0.02561, "m2": 0.2683, "m2r": 4.5, "m2dec": 1.0, "breath": -9.013, "bq": 3.179,
             "bdec": 0.5391, "puff": -13.68, "puff_hz": 5000, "puff_ms": 1.019, "thump": -46.85, "thump_f": 107.4,
             "click": -10.1, "click_hz": 6070, "dbl": -47.17, "dgap": 0.07519, "dratio": 0.6551, "hp": 67.89,
             "lp": 11620, "room": 0.00401, "rt": 0.1363, "width": 0.8105, "jit": 0.3896},
}

FAMILY_NAMES = {"ui_pop": list(_EDITS)}
FAMILY = {"ui_pop": [dict(DEFAULTS, **_EDITS[name]) for name in _EDITS]}
FAMILIES = {"ui_pop": list(zip(FAMILY_NAMES["ui_pop"], FAMILY["ui_pop"]))}


def family_space(params: dict) -> dict:
    """The full space with every character key pinned to the member's value (T15)."""
    return tonal.family_space(SPACE, dict(DEFAULTS, **params), FINE)


def measure(x: np.ndarray) -> dict:
    """The catalog measurement (T14)."""
    return tonal.measure(x)


def study_penalty(x: np.ndarray) -> float:
    """Distance of a render from the film pop targets (T15); 0 inside every range."""
    return float(tonal.outside(tonal.measure(x), STUDY))


def study_extra(x: np.ndarray, row: dict | None = None) -> float:
    """Critic extra term: minus the study distance."""
    return -study_penalty(x)


def _pitch(p: dict, t: np.ndarray, f0: float) -> np.ndarray:
    if p["sweep"] > 0.001:
        u = np.clip(t / p["sweep"], 0, 1)
        return f0 * 2 ** ((p["bend"] / 12) * (1 - u) ** p["ease"])
    return tonal.glide(t, f0, p["bend"], p["ptau"])


def _one_pop(p: dict, r: np.random.Generator, n: int, f0: float, tau: float, gain: float = 1.0) -> np.ndarray:
    t = np.arange(n) / tonal.SR
    e = tonal.env(t, p["onset"], tau, p["hold"], p["curve"], p["dcurve"])
    f = _pitch(p, t, f0)
    inh = p["inh"]
    parts = [(2 * (1 + inh), p["h2"], p["hdec"]), (3 * (1 + 2 * inh), p["h3"], 1.5 * p["hdec"])]
    if p["m2"] > 0:
        parts.append((p["m2r"], p["m2"], p["m2dec"]))
    y = tonal.tone(f, r, parts, e)
    if p["breath"] > -59:
        y = y + 10 ** (p["breath"] / 20) * tonal.follow_noise(n, r, f, p["bq"], e ** p["bdec"])
    return gain * y


def _layers(y: np.ndarray, p: dict, r: np.random.Generator, n: int, tau: float, onset: float) -> np.ndarray:
    pk = float(np.max(np.abs(y)))
    if p["puff"] > -59:
        hz = p["puff_hz"]
        y = y + pk * 10 ** (p["puff"] / 20) * tonal.burst(n, r, hz / 2.2, 2.2 * hz, p["puff_ms"] / 1000,
                                                         rise=max(0.0006, 0.3 * onset))
    if p["thump"] > -59:
        y = y + pk * 10 ** (p["thump"] / 20) * tonal.thump(n, p["thump_f"], max(0.012, 0.6 * tau),
                                                          rise=max(0.002, 0.5 * onset))
    if p["click"] > -59:
        hz = p["click_hz"]
        y = y + pk * 10 ** (p["click"] / 20) * tonal.burst(n, r, hz / 1.6, 1.6 * hz, 0.0008)
    return y


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """One pop take: stereo float64 at 48 kHz, at most about 0.8 s."""
    p = dict(DEFAULTS, **(p or {}))
    j = p["jit"] * rng.standard_normal(3)
    f0 = p["f0"] * math.exp(0.02 * j[0])
    tau = p["tau"] * math.exp(0.08 * j[1])
    onset = p["onset"] * math.exp(0.1 * j[2])
    q = dict(p, onset=onset)
    double = p["dbl"] > -59
    gap = p["dgap"] if double else 0.0
    n = max(1, int(round(min(0.8, onset + p["hold"] + 7 * tau + gap + 0.01) * tonal.SR)))
    y = _one_pop(q, rng, n, f0, tau)
    if double:
        k = max(1, int(round(gap * tonal.SR)))
        if k < n:
            y[k:] += _one_pop(q, rng, n - k, f0 * p["dratio"], 0.9 * tau, 10 ** (p["dbl"] / 20))
    y = _layers(y, p, rng, n, tau, onset)
    st = tonal.finish(y, rng, hp=p["hp"], lp=p["lp"], room_wet=p["room"], rt60=p["rt"], width=p["width"], cut=400)
    return np.asarray(st, dtype=np.float64)
