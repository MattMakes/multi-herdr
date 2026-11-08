"""Recipe subdrop: a sine that glides down into the sub region, with a click on top.

Spec: docs/specs/hark/recipes-impacts-hits.md section 2.
"""

from __future__ import annotations

import types
from typing import Any

import numpy as np

from hark.forge.engine import dsp
from hark.forge.recipes.impacts.impact import measure, study_extra, study_penalty

SR = 48000
PEAK = 0.89

TARGET = "a deep sub bass drop"
TARGETS = {
    "sub_drop": TARGET,
    "pro_sub_drop": "a designed sub drop: a low sine falling under a bright click",
}
ALTS = ["a kick drum", "an explosion", "thunder", "a low hum"]
REFSET = "sub_drop"

SPACE: dict[str, Any] = {
    "f0": (45.0, 220.0, "log"),
    "f1": (28.0, 62.0, "log"),
    "glide": (0.06, 1.6, "log"),
    "glide_curve": (0.3, 3.0, "log"),
    "bloom": (0.004, 0.45, "log"),
    "hold": (0.0, 0.5),
    "decay_db": (15.0, 90.0),
    "length": (0.6, 3.0),
    "harm": (0.0, 0.8),
    "harm_lp": (150.0, 650.0, "log"),
    "click": (0.0, 1.0),
    "click_f": (1200.0, 8000.0, "log"),
    "click_ms": (1.0, 40.0, "log"),
    "thump": (0.0, 1.0),
    "grit": (0.0, 1.0),
    "width": (0.0, 0.3),
    "room": (0.0, 0.4),
}
_KEYS = list(SPACE)


def _members(columns: dict[str, list[float]]) -> list[tuple[str, dict]]:
    return [(name, dict(zip(_KEYS, values))) for name, values in columns.items()]


FAMILIES: dict[str, list[tuple[str, dict]]] = {
    "sub_drop": _members({
        "808": [120, 40, 0.12, 0.6, 0.004, 0.05, 30, 2.0, 0.35, 300, 0.4, 3000, 3, 0.5, 0.0, 0.05, 0.1],
        "pro": [64, 44, 0.75, 1.0, 0.38, 0.05, 42, 1.4, 0.3, 300, 0.6, 3500, 30, 0.0, 0.0, 0.05, 0.1],
        "long": [90, 30, 1.4, 1.0, 0.15, 0.2, 22, 3.0, 0.25, 250, 0.15, 2500, 8, 0.0, 0.15, 0.08, 0.0],
        "punch": [160, 50, 0.2, 0.5, 0.004, 0.0, 70, 0.8, 0.5, 400, 0.7, 4000, 2, 0.8, 0.0, 0.05, 0.15],
        "gritty": [110, 36, 0.5, 0.8, 0.05, 0.1, 35, 1.8, 0.7, 450, 0.3, 2200, 6, 0.2, 0.6, 0.12, 0.05],
    }),
    "pro_sub_drop": _members({
        "ref": [64, 44, 0.75, 1.0, 0.38, 0.04, 42, 1.3, 0.25, 300, 0.6, 3500, 35, 0.0, 0.0, 0.04, 0.08],
        "quick": [72, 46, 0.5, 1.0, 0.24, 0.02, 52, 1.0, 0.3, 320, 0.8, 4200, 25, 0.1, 0.0, 0.04, 0.08],
        "low": [58, 37, 0.85, 1.0, 0.42, 0.06, 38, 1.5, 0.4, 260, 0.45, 2800, 40, 0.0, 0.1, 0.04, 0.06],
        "bright": [66, 44, 0.7, 1.0, 0.3, 0.04, 42, 1.25, 0.6, 420, 0.9, 5500, 30, 0.0, 0.0, 0.06, 0.12],
        "soft": [62, 44, 0.75, 1.3, 0.43, 0.08, 34, 1.5, 0.2, 260, 0.2, 2600, 40, 0.0, 0.05, 0.03, 0.05],
    }),
}
DEFAULTS: dict[str, Any] = dict(dict(FAMILIES["sub_drop"])["pro"])

FINE: dict[str, list[str]] = {
    "sub_drop": ["harm", "harm_lp", "click", "click_f", "length"],
    "pro_sub_drop": ["harm", "harm_lp", "click", "click_f", "click_ms", "length"],
}
LIMITS: dict[str, dict[str, tuple]] = {
    "pro_sub_drop": {
        "f0": (54.0, 80.0, "log"), "f1": (34.0, 50.0, "log"), "glide": (0.4, 1.0, "log"),
        "bloom": (0.2, 0.45, "log"), "decay_db": (32.0, 55.0), "length": (0.9, 1.6),
        "click_ms": (15.0, 45.0, "log"),
    },
}
STUDY: dict[str, dict[str, tuple[float, float]]] = {
    "pro_sub_drop": {
        "len50": (0.8, 1.6), "attack_ms": (250, 460), "slope_dBs": (-50, -32), "sh150": (0.85, 1.0),
        "corr": (0.8, 1.0), "ridge0": (55, 75), "ridge1": (38, 50),
    },
    "sub_drop": {"len50": (0.6, 3.0), "sh150": (0.8, 1.0), "corr": (0.8, 1.0), "slope_dBs": (-90, -15)},
}

__all__ = [
    "SR", "TARGET", "TARGETS", "ALTS", "REFSET", "SPACE", "FAMILIES", "DEFAULTS", "FINE", "LIMITS", "STUDY",
    "render", "space_for", "recipe_for", "seconds_of", "measure", "study_penalty", "study_extra",
]


def space_for(cat: str) -> dict[str, Any]:
    """The search space of a category: its fine keys, at the limit range when one exists."""
    limits = LIMITS.get(cat, {})
    return {k: limits.get(k, SPACE[k]) for k in FINE[cat]}


def _member(cat: str, preset: int | str) -> tuple[str, dict]:
    members = FAMILIES[cat]
    if isinstance(preset, str):
        found = dict(members)
        if preset not in found:
            raise KeyError(f"subdrop: no member {preset!r} in {cat!r}")
        return preset, dict(found[preset])
    name, params = members[preset]
    return name, dict(params)


def recipe_for(cat: str = "sub_drop", preset: int | str = 0) -> types.SimpleNamespace:
    """A recipe object for 1 family member (index or name)."""
    name, params = _member(cat, preset)
    return types.SimpleNamespace(
        MODULE=__name__, SPACE=space_for(cat), DEFAULTS=params, render=render, TARGET=TARGETS[cat],
        ALTS=list(ALTS), REFSET=cat, PRESET=name,
    )


def seconds_of(p: dict) -> float:
    """File length: the asked length, cut at the -70 dB point of the decay, at least 0.3 s."""
    life = p["bloom"] + p["hold"] + 70.0 / p["decay_db"]
    return max(0.3, min(p["length"], life))


def _pitch(p: dict, t: np.ndarray) -> np.ndarray:
    s = np.clip(t / max(p["glide"], 0.001), 0, 1) ** p["glide_curve"]
    lf0, lf1 = np.log(p["f0"]), np.log(p["f1"])
    return np.exp(lf0 + (lf1 - lf0) * s)


def _envelope(p: dict, t: np.ndarray) -> np.ndarray:
    bloom, hold = p["bloom"], p["hold"]
    e = np.ones_like(t)
    rising = t < bloom
    e[rising] = 0.5 - 0.5 * np.cos(np.pi * t[rising] / bloom)
    late = t > bloom + hold
    e[late] = 10 ** (-p["decay_db"] * (t[late] - bloom - hold) / 20)
    return e


def _peak1(x: np.ndarray) -> np.ndarray:
    return x / (np.max(np.abs(x)) + 1e-12)


def _harmonics(p: dict, f: np.ndarray, phase: np.ndarray, env: np.ndarray) -> np.ndarray:
    if p["harm"] <= 0.01:
        return np.zeros_like(f)
    y = np.zeros_like(f)
    for k in range(2, 7):
        fk = k * f
        taper = np.clip((300 - fk) / 80, 0, 1)
        y += (0.6 / k) / (1 + (fk / p["harm_lp"]) ** 4) * np.sin(k * phase + 0.4 * k) * taper
    return p["harm"] * _peak1(y * env)


def _brown_band(n: int, rng: np.random.Generator, lo: float, hi: float) -> np.ndarray:
    b = np.cumsum(rng.standard_normal(n))
    return _peak1(dsp.butter(b - b.mean(), "band", (lo, hi), 2))


def _grit(p: dict, n: int, env: np.ndarray, rng: np.random.Generator) -> np.ndarray:
    if p["grit"] <= 0.01:
        return np.zeros(n)
    return _brown_band(n, rng, 35.0, 160.0) * 0.3 * p["grit"] * env


def _click(p: dict, n: int, t: np.ndarray, rng: np.random.Generator) -> np.ndarray:
    length = min(n, max(16, dsp.n_of(p["click_ms"] / 1000)))
    u = np.arange(length) / length
    w = rng.standard_normal(length) * (1 - u) ** 3 * (1 - np.exp(-t[:length] / 0.0002))
    w = _peak1(dsp.butter(w, "high", p["click_f"] / 2, 2))
    k = min(n, dsp.n_of(0.02))
    tk = t[:k]
    ping = np.sin(2 * np.pi * p["click_f"] * tk) * np.exp(-tk / 0.003) * (1 - np.exp(-tk / 0.0003))
    y = np.zeros(n)
    y[:length] += w
    y[:k] += 0.6 * ping
    return _peak1(y)


def _thump(t: np.ndarray, rng: np.random.Generator) -> np.ndarray:
    f = 90 + 60 * np.exp(-t / 0.03)
    y = np.sin(2 * np.pi * np.cumsum(f) / SR + rng.uniform(0, 0.3))
    y = y * np.exp(-t / 0.06) * (1 - np.exp(-t / 0.001))
    return dsp.fade(y, 0.0, 0.01)


def _top(p: dict, n: int, t: np.ndarray, rng: np.random.Generator) -> np.ndarray:
    top = p["click"] * _click(p, n, t, rng) + 0.7 * p["thump"] * _thump(t, rng)
    if p["room"] > 0.01 and np.any(top):
        top = dsp.room(top, rng, rt60=0.35, size=0.4, damp=0.7, wet=p["room"])[:n]
    return top


def _floor(n: int, env: np.ndarray, rng: np.random.Generator) -> np.ndarray:
    w = _peak1(dsp.butter(rng.standard_normal(n), "band", (300.0, 12000.0), 2))
    return w * 1e-4 * np.maximum(env, 0.01)


def _widen(p: dict, mono: np.ndarray, upper: np.ndarray) -> np.ndarray:
    n = len(mono)
    hi = dsp.butter(upper, "high", 150.0, 2)
    d = dsp.n_of(0.0045)
    side = np.zeros(n)
    if d < n:
        side[d:] = p["width"] * hi[: n - d]
    return np.column_stack([mono + side, mono - side])


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """One take: stereo float64, `seconds_of(p)` long, peak 0.89."""
    p = {**DEFAULTS, **p}
    seconds = seconds_of(p)
    n = dsp.n_of(seconds)
    t = np.arange(n) / SR
    f = _pitch(p, t)
    phase = 2 * np.pi * np.cumsum(f) / SR + rng.uniform(0, 0.2)
    env = _envelope(p, t)
    harm = _harmonics(p, f, phase, env)
    grit = _grit(p, n, env, rng)
    top = _top(p, n, t, rng)
    floor = _floor(n, env, rng)
    mono = np.sin(phase) * env + harm + grit + top + floor
    y = dsp.butter(_widen(p, mono, harm + top), "high", 22.0, 2)
    y = dsp.fade(y, 0.0015, max(0.05, 0.15 * seconds))
    return y * PEAK / (np.max(np.abs(y)) + 1e-12)
