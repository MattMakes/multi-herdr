"""Recipe thump: a low whoosh that lands on the edit with a sliding sine thump.

Spec: docs/specs/hark/recipes-impacts-hits.md section 3. The whoosh pass engine is the motion recipe `whoosh`
(recipes-motion-pass 1.2); ARCHITECTURE.md 8.5 allows this import.
"""

from __future__ import annotations

import types
from typing import Any

import numpy as np

from hark.forge.engine import dsp
from hark.forge.recipes.motion import whoosh

SR = 48000
PEAK = 0.89

TARGET = "a deep low whoosh that lands with a thump"
TARGETS = {"whoosh_big": TARGET}
ALTS = ["a strong wind", "a bass drum", "a car passing", "thunder"]
REFSET = "whoosh_big"

THUMP_KEYS: tuple[str, ...] = ("thump_db", "thump_f0", "thump_f1", "thump_tau", "thump_lead")
THUMP_SPACE: dict[str, Any] = {
    "thump_db": (-40.0, 6.0),
    "thump_f0": (60.0, 220.0, "log"),
    "thump_f1": (32.0, 90.0, "log"),
    "thump_tau": (0.03, 0.4, "log"),
    "thump_lead": (-0.04, 0.03),
}
SPACE: dict[str, Any] = {**whoosh.SPACE, **THUMP_SPACE}

_TUNED: dict[str, float] = {
    "rise": 0.13, "rise_k": 0.8, "hold": 0.01, "fall": 0.55, "fall_k": 1.6, "start_db": -34,
    "track": 0.7, "f_peak": 200, "up_oct": 1.0, "down_oct": 1.6, "dop_st": 2.0, "dop_w": 0.06,
    "q_air": 1.0, "lp_ratio": 4.0, "flut_depth": 0.2, "flut_f": 10, "low_db": -2, "low_f": 110,
    "low_smooth": 0.03, "sub_db": 0, "sub_f": 52, "sub_glide": 0.4, "sub_drive": 1.0, "hiss_db": -26,
    "hiss_hp": 1500, "hiss_open": 0.8, "hiss_pow": 1.3, "tone_db": -50, "grain_db": -50, "room_wet": 0.15,
    "rt60": 0.7, "room_damp": 0.45, "pan": 0.05, "width": 0.08, "drive_db": 2.5, "end_db": -64,
    "end_fade": 0.08, "thump_db": -6, "thump_f0": 120, "thump_f1": 50, "thump_tau": 0.12, "thump_lead": 0.01,
}
BASE: dict[str, Any] = {**whoosh.BASES["big"], **whoosh.OPTIONAL, **_TUNED}
DEFAULTS: dict[str, Any] = dict(BASE)

PRESETS: dict[str, dict[str, dict[str, float]]] = {
    "whoosh_big": {
        "thump": {
            "rise_k": 1.266, "track": 0.871, "up_oct": 2.118, "down_oct": 1.68, "lp_ratio": 4.211,
            "flut_depth": 0.26, "flut_f": 10.37, "low_db": -22.24, "low_f": 283.6, "sub_db": -14.85,
            "sub_f": 59.26, "sub_glide": 0.8055, "sub_drive": 1.462, "hiss_db": -15.21, "hiss_hp": 3308,
            "rt60": 0.3, "width": 0.4326, "thump_f1": 70.85,
        },
        "thump_deep": {
            "rise": 0.1401, "rise_k": 0.8922, "fall": 0.75, "fall_k": 1.761, "track": 0.9656, "f_peak": 150,
            "up_oct": 2.965, "down_oct": 1.4, "q_air": 1.343, "lp_ratio": 1.959, "flut_depth": 0.06493,
            "flut_f": 22.93, "low_db": -17.48, "low_f": 241.1, "sub_db": -20.64, "sub_f": 44.0,
            "sub_glide": 0.8703, "sub_drive": 0.9437, "hiss_db": -15.95, "hiss_hp": 4168, "rt60": 0.8878,
            "room_wet": 0.2214, "width": 0.2907, "drive_db": 5.561, "thump_db": -9.0, "thump_f0": 92.21,
            "thump_f1": 41.61, "thump_tau": 0.1913, "thump_lead": 0.00001,
        },
        "thump_airy": {
            "rise": 0.11, "fall": 0.45, "track": 0.7214, "f_peak": 320, "up_oct": 2.596, "down_oct": 2.257,
            "q_air": 1.52, "lp_ratio": 7.464, "flut_depth": 0.4371, "flut_f": 9.589, "low_db": -4.037,
            "low_f": 50.94, "sub_db": -4.72, "sub_f": 57.99, "hiss_db": -20.0, "hiss_hp": 5687, "width": 0.2,
            "drive_db": 6.444, "thump_db": -12.0, "thump_f0": 115.4, "thump_f1": 45.69, "thump_tau": 0.06806,
        },
    },
}
FAMILIES: dict[str, list[tuple[str, dict]]] = {
    cat: [(name, {**BASE, **over}) for name, over in members.items()] for cat, members in PRESETS.items()
}


def preset(name: str, cat: str = "whoosh_big") -> dict[str, Any]:
    """The base merged with 1 member's overrides."""
    members = PRESETS[cat]
    if name not in members:
        raise KeyError(f"thump: no member {name!r} in {cat!r}")
    return {**BASE, **members[name]}


def preset_full(cat: str, params: dict) -> dict[str, Any]:
    """The base merged with params."""
    return {**BASE, **params}


_RECIPE_DATA = (SPACE, TARGETS, ALTS)


def recipe(cat: str = "whoosh_big", preset_name: str = "thump") -> types.SimpleNamespace:
    """A recipe object for 1 member."""
    space, targets, alts = _RECIPE_DATA
    return types.SimpleNamespace(
        MODULE=__name__, SPACE=dict(space), DEFAULTS=preset(preset_name, cat), render=render,
        TARGET=targets[cat], ALTS=list(alts), REFSET="whoosh_big", PRESET=preset_name,
    )


def peak_time(q: dict) -> float:
    """The pass peak time, as the engine's level curve defines it."""
    return q["rise"] * (max(-q["start_db"], 1) / 30) ** (1 / q["rise_k"])


def thump_wave(q: dict, m: int, rng: np.random.Generator) -> np.ndarray:
    """The sliding sine thump, m samples, t = 0 at its strike."""
    if m <= 16:
        return np.zeros(max(m, 0))
    t = np.arange(m) / SR
    f = q["thump_f1"] + (q["thump_f0"] - q["thump_f1"]) * np.exp(-t / 0.025)
    phase = 2 * np.pi * np.cumsum(f) / SR + rng.uniform(-0.3, 0.3)
    return np.sin(phase) * np.exp(-t / q["thump_tau"]) * (1 - np.exp(-t / 0.0006))


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """One take: the whoosh pass engine, then the thump; stereo float64, peak 0.89."""
    q = {**BASE, **p}
    x = np.array(whoosh.render({k: v for k, v in q.items() if k not in THUMP_KEYS}, rng), dtype=np.float64)
    n = len(x)
    i0 = int(np.clip(dsp.n_of(max(0.0, peak_time(q) - q["thump_lead"])), 0, n - 1))
    th = thump_wave(q, n - i0, rng)
    if q["thump_db"] > -39.5 and len(th) and np.any(th):
        th = dsp.fade(th, 0.0, min(0.05, 0.5 * n / SR))
        lvl = float(np.max(np.abs(x)))
        x[i0:] += (10 ** (q["thump_db"] / 20) * lvl * th)[:, None]
    return x * PEAK / (np.max(np.abs(x)) + 1e-12)
