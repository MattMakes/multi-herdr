"""Sub drop: a sine that slides down into the sub range, under a bright click.

Spec: docs/specs/hark/recipes-impacts-hits.md section 2.
"""

from __future__ import annotations

import types
from typing import Any

import numpy as np

from hark.forge.engine import dsp
from hark.forge.recipes.impacts import _common as cm
from hark.forge.recipes.impacts.impact import measure, study_extra, study_penalty

SR = cm.SR
_OUT_PEAK = 0.89

TARGET = "a deep sub bass drop"
TARGETS = {
    "sub_drop": TARGET,
    "pro_sub_drop": "a designed sub drop: a low sine falling under a bright click",
}
ALTS = ["a kick drum", "an explosion", "thunder", "a low hum"]
REFSET = "sub_drop"

SPACE: dict[str, Any] = cm.space_table("""
    f0 45.0 220.0 log
    f1 28.0 62.0 log
    glide 0.06 1.6 log
    glide_curve 0.3 3.0 log
    bloom 0.004 0.45 log
    hold 0.0 0.5
    decay_db 15.0 90.0
    length 0.6 3.0
    harm 0.0 0.8
    harm_lp 150.0 650.0 log
    click 0.0 1.0
    click_f 1200.0 8000.0 log
    click_ms 1.0 40.0 log
    thump 0.0 1.0
    grit 0.0 1.0
    width 0.0 0.3
    room 0.0 0.4
""")
_KEYS = list(SPACE)


def _members(rows: dict[str, str]) -> list[tuple[str, dict]]:
    """Read one value row per member; the values follow the key order of SPACE."""
    return [(name, dict(zip(_KEYS, map(cm.scalar, row.split())))) for name, row in rows.items()]


FAMILIES: dict[str, list[tuple[str, dict]]] = {
    "sub_drop": _members({
        "808": "120 40 0.12 0.6 0.004 0.05 30 2.0 0.35 300 0.4 3000 3 0.5 0.0 0.05 0.1",
        "pro": "64 44 0.75 1.0 0.38 0.05 42 1.4 0.3 300 0.6 3500 30 0.0 0.0 0.05 0.1",
        "long": "90 30 1.4 1.0 0.15 0.2 22 3.0 0.25 250 0.15 2500 8 0.0 0.15 0.08 0.0",
        "punch": "160 50 0.2 0.5 0.004 0.0 70 0.8 0.5 400 0.7 4000 2 0.8 0.0 0.05 0.15",
        "gritty": "110 36 0.5 0.8 0.05 0.1 35 1.8 0.7 450 0.3 2200 6 0.2 0.6 0.12 0.05",
    }),
    "pro_sub_drop": _members({
        "ref": "64 44 0.75 1.0 0.38 0.04 42 1.3 0.25 300 0.6 3500 35 0.0 0.0 0.04 0.08",
        "quick": "72 46 0.5 1.0 0.24 0.02 52 1.0 0.3 320 0.8 4200 25 0.1 0.0 0.04 0.08",
        "low": "58 37 0.85 1.0 0.42 0.06 38 1.5 0.4 260 0.45 2800 40 0.0 0.1 0.04 0.06",
        "bright": "66 44 0.7 1.0 0.3 0.04 42 1.25 0.6 420 0.9 5500 30 0.0 0.0 0.06 0.12",
        "soft": "62 44 0.75 1.3 0.43 0.08 34 1.5 0.2 260 0.2 2600 40 0.0 0.05 0.03 0.05",
    }),
}
DEFAULTS: dict[str, Any] = dict(dict(FAMILIES["sub_drop"])["pro"])

FINE: dict[str, list[str]] = {
    "sub_drop": "harm harm_lp click click_f length".split(),
    "pro_sub_drop": "harm harm_lp click click_f click_ms length".split(),
}
LIMITS: dict[str, dict[str, tuple]] = {
    "pro_sub_drop": cm.space_table("""
        f0 54.0 80.0 log
        f1 34.0 50.0 log
        glide 0.4 1.0 log
        bloom 0.2 0.45 log
        decay_db 32.0 55.0
        length 0.9 1.6
        click_ms 15.0 45.0 log
    """),
}
STUDY: dict[str, dict[str, tuple[float, float]]] = {
    "pro_sub_drop": cm.space_table("""
        len50 0.8 1.6
        attack_ms 250 460
        slope_dBs -50 -32
        sh150 0.85 1.0
        corr 0.8 1.0
        ridge0 55 75
        ridge1 38 50
    """),
    "sub_drop": cm.space_table("""
        len50 0.6 3.0
        sh150 0.8 1.0
        corr 0.8 1.0
        slope_dBs -90 -15
    """),
}

__all__ = [
    "SR", "TARGET", "TARGETS", "ALTS", "REFSET", "SPACE", "FAMILIES", "DEFAULTS", "FINE", "LIMITS", "STUDY",
    "render", "space_for", "recipe_for", "seconds_of", "measure", "study_penalty", "study_extra",
]


_SEARCH_PARTS = (SPACE, FINE, LIMITS)


def space_for(cat: str) -> dict[str, Any]:
    """Search space of one category: its fine keys, each at its narrower limit when it has one."""
    space, fine, limits = _SEARCH_PARTS
    narrow = limits.get(cat, {})
    return {order_k: narrow.get(order_k, space[order_k]) for order_k in fine[cat]}


def _pick(cat: str, preset: int | str) -> tuple[str, dict]:
    members = FAMILIES[cat]
    if isinstance(preset, str):
        found = dict(members)
        if preset not in found:
            raise KeyError(f"subdrop: no member {preset!r} in {cat!r}")
        return preset, dict(found[preset])
    name, params = members[preset]
    return name, dict(params)


def recipe_for(cat: str = "sub_drop", preset: int | str = 0) -> types.SimpleNamespace:
    """Recipe object for one family member, chosen by index or by name."""
    name, params = _pick(cat, preset)
    return types.SimpleNamespace(
        MODULE=__name__, SPACE=space_for(cat), DEFAULTS=params, render=render, TARGET=TARGETS[cat],
        ALTS=list(ALTS), REFSET=cat, PRESET=name,
    )


def seconds_of(p: dict) -> float:
    """Seconds to render: ``length``, shortened to where the decay reaches -70 dB, never under 0.3 s."""
    life = p["bloom"] + p["hold"] + 70.0 / p["decay_db"]
    return max(0.3, min(p["length"], life))


def _slide_hz(cfg: dict, secs: np.ndarray) -> np.ndarray:
    prog = np.clip(secs / max(cfg["glide"], 0.001), 0, 1) ** cfg["glide_curve"]
    log_hi, log_lo = np.log(cfg["f0"]), np.log(cfg["f1"])
    return np.exp(log_hi + (log_lo - log_hi) * prog)


def _level(cfg: dict, secs: np.ndarray) -> np.ndarray:
    bloom, hold = cfg["bloom"], cfg["hold"]
    shape = np.ones_like(secs)
    rising = secs < bloom
    shape[rising] = 0.5 - 0.5 * np.cos(np.pi * secs[rising] / bloom)
    late = secs > bloom + hold
    shape[late] = 10 ** (-cfg["decay_db"] * (secs[late] - bloom - hold) / 20)
    return shape


def _norm(sig: np.ndarray) -> np.ndarray:
    return sig / (np.max(np.abs(sig)) + 1e-12)


def _overtones(cfg: dict, hz: np.ndarray, phase: np.ndarray, shape_t: np.ndarray) -> np.ndarray:
    if cfg["harm"] <= 0.01:
        return np.zeros_like(hz)
    out = np.zeros_like(hz)
    for order_k in range(2, 7):
        hk = order_k * hz
        taper = np.clip((300 - hk) / 80, 0, 1)
        out += (0.6 / order_k) / (1 + (hk / cfg["harm_lp"]) ** 4) * np.sin(order_k * phase + 0.4 * order_k) * taper
    return cfg["harm"] * _norm(out * shape_t)


def _rumble_band(size: int, gen: np.random.Generator, lo: float, upper: float) -> np.ndarray:
    walk = np.cumsum(gen.standard_normal(size))
    return _norm(dsp.butter(walk - walk.mean(), "band", (lo, upper), 2))


def _dirt(cfg: dict, size: int, shape_t: np.ndarray, gen: np.random.Generator) -> np.ndarray:
    if cfg["grit"] <= 0.01:
        return np.zeros(size)
    return _rumble_band(size, gen, 35.0, 160.0) * 0.3 * cfg["grit"] * shape_t


def _snap(cfg: dict, size: int, secs: np.ndarray, gen: np.random.Generator) -> np.ndarray:
    length = min(size, max(16, dsp.n_of(cfg["click_ms"] / 1000)))
    pos = np.arange(length) / length
    noise = gen.standard_normal(length) * (1 - pos) ** 3 * (1 - np.exp(-secs[:length] / 0.0002))
    noise = _norm(dsp.butter(noise, "high", cfg["click_f"] / 2, 2))
    order_k = min(size, dsp.n_of(0.02))
    ptime = secs[:order_k]
    ping = np.sin(2 * np.pi * cfg["click_f"] * ptime) * np.exp(-ptime / 0.003) * (1 - np.exp(-ptime / 0.0003))
    out = np.zeros(size)
    out[:length] += noise
    out[:order_k] += 0.6 * ping
    return _norm(out)


def _kick_body(secs: np.ndarray, gen: np.random.Generator) -> np.ndarray:
    hz = 90 + 60 * np.exp(-secs / 0.03)
    out = np.sin(2 * np.pi * np.cumsum(hz) / SR + gen.uniform(0, 0.3))
    out = out * np.exp(-secs / 0.06) * (1 - np.exp(-secs / 0.001))
    return dsp.fade(out, 0.0, 0.01)


def _upper_layers(cfg: dict, size: int, secs: np.ndarray, gen: np.random.Generator) -> np.ndarray:
    top = cfg["click"] * _snap(cfg, size, secs, gen) + 0.7 * cfg["thump"] * _kick_body(secs, gen)
    if cfg["room"] > 0.01 and np.any(top):
        top = dsp.room(top, gen, rt60=0.35, size=0.4, damp=0.7, wet=cfg["room"])[:size]
    return top


def _hiss_floor(size: int, shape_t: np.ndarray, gen: np.random.Generator) -> np.ndarray:
    noise = _norm(dsp.butter(gen.standard_normal(size), "band", (300.0, 12000.0), 2))
    return noise * 1e-4 * np.maximum(shape_t, 0.01)


def _stereo(cfg: dict, mono: np.ndarray, upper: np.ndarray) -> np.ndarray:
    size = len(mono)
    bright = cm.highpass(upper, 150.0)
    lag = dsp.n_of(0.0045)
    side = np.zeros(size)
    if lag < size:
        side[lag:] = cfg["width"] * bright[: size - lag]
    return np.column_stack([mono + side, mono - side])


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """Render one drop: stereo float64 of ``seconds_of(p)`` seconds, peak 0.89."""
    p = {**DEFAULTS, **p}
    seconds = seconds_of(p)
    size = dsp.n_of(seconds)
    secs = np.arange(size) / SR
    hz = _slide_hz(p, secs)
    phase = 2 * np.pi * np.cumsum(hz) / SR + rng.uniform(0, 0.2)
    shape_t = _level(p, secs)
    harm = _overtones(p, hz, phase, shape_t)
    grit = _dirt(p, size, shape_t, rng)
    top = _upper_layers(p, size, secs, rng)
    floor = _hiss_floor(size, shape_t, rng)
    mono = np.sin(phase) * shape_t + harm + grit + top + floor
    out = dsp.butter(_stereo(p, mono, harm + top), "high", 22.0, 2)
    out = dsp.fade(out, 0.0015, max(0.05, 0.15 * seconds))
    return out * _OUT_PEAK / (np.max(np.abs(out)) + 1e-12)
