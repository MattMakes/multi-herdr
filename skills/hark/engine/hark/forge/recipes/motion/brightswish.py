"""brightswish: bright, wide air flicks and a designed whip (recipes-motion part 1, section 4).

The whoosh pass engine with a high air band and no low or sub layer, plus one extra
layer for the whip: a feedback comb whose fundamental falls over the file.
"""

from __future__ import annotations

import types
from typing import Any

import numpy as np

from hark.forge.engine import dsp
from hark.forge.recipes.motion import _util as u
from hark.forge.recipes.motion import whoosh

SR = u.SR
MODULE = "hark.forge.recipes.motion.brightswish"
PEAK = 0.89

COMB_SPACE: dict[str, tuple] = {
    "comb_mix": (0.0, 1.0), "comb_f0": (150.0, 4000.0, "log"),
    "comb_f1": (40.0, 1500.0, "log"), "comb_fb": (0.0, 0.92),
}
COMB_KEYS: tuple[str, ...] = tuple(COMB_SPACE)
COMB_OFF: dict[str, float] = {"comb_mix": 0.0, "comb_f0": 1000.0, "comb_f1": 200.0, "comb_fb": 0.5}

SPACE: dict[str, tuple] = {**whoosh.SPACE, **COMB_SPACE}

SWISH: dict[str, Any] = {**whoosh.FAST, **whoosh.OPTIONAL, **COMB_OFF, **{
    "rise": 0.13, "rise_k": 0.8, "hold": 0.0, "fall": 0.07, "fall_k": 1.1, "start_db": -32.0,
    "track": 0.7, "f_peak": 2300.0, "up_oct": 1.0, "down_oct": 1.3, "dop_st": 3.0,
    "dop_w": 0.03, "q_air": 1.4, "lp_ratio": 2.6, "flut_depth": 0.2, "flut_f": 30.0,
    "low_db": -40.0, "sub_db": -50.0, "hiss_db": -6.0, "hiss_hp": 3000.0, "hiss_open": 1.2,
    "hiss_pow": 1.3, "hiss_tail": None, "hiss_pre": None, "hiss_mfloor": 0.0,
    "tail_sweep": 0.0, "hiss_q": 0.8, "tone_db": -50.0, "grain_db": -50.0, "room_wet": 0.08,
    "rt60": 0.35, "room_damp": 0.3, "pan": 0.15, "width": 0.35, "hiss_width": 0.5,
    "drive_db": 1.0, "end_db": -62.0, "end_fade": 0.03, "eq_lo_db": -12.0, "eq_lo_f": 300.0,
    "eq_hi_db": 0.0, "eq_hi_f": 6000.0,
}}

WHIP: dict[str, Any] = {**SWISH, **{
    "rise": 0.02, "rise_k": 1.0, "hold": 0.0, "fall": 0.5, "fall_k": 0.75, "start_db": -30.0,
    "track": 1.2, "f_peak": 4200.0, "up_oct": 0.5, "down_oct": 3.5, "dop_st": 6.0,
    "dop_w": 0.05, "q_air": 2.5, "lp_ratio": 2.2, "flut_depth": 0.6, "flut_f": 32.0,
    "hiss_db": -12.0, "hiss_hp": 2500.0, "hiss_tail": 1.0, "room_wet": 0.15, "rt60": 0.6,
    "width": 0.6, "hiss_width": 1.0, "pan": 0.1, "end_db": -60.0, "end_fade": 0.06,
    "comb_mix": 0.5, "comb_f0": 2200.0, "comb_f1": 160.0, "comb_fb": 0.7,
}}

BASES: dict[str, dict[str, Any]] = {"swish": SWISH, "whoosh_fast": WHIP}
DEFAULTS: dict[str, Any] = dict(SWISH)
TARGETS: dict[str, str] = {"swish": "a bright quick swish", "whoosh_fast": "a bright whip whoosh"}
TARGET = TARGETS["swish"]
ALTS = ["a short burst of steam", "a page being flipped", "a sword being sharpened", "rain"]
REFSET = "swish"

FAMILIES: dict[str, list[tuple[str, dict[str, float]]]] = {
    "swish": [
        ("bright", {
            "rise_k": 0.9287, "hold": 0.05657, "fall_k": 1.127, "track": 0.6361, "up_oct": 1.004,
            "down_oct": 2.161, "q_air": 2.813, "flut_depth": 0.3016, "flut_f": 33.74,
            "hiss_db": -3.588, "hiss_hp": 2054, "hiss_open": 1.037, "hiss_pow": 1.567,
            "grain_rate": 2789, "grain_f": 2753, "room_wet": 0.0, "rt60": 0.4884, "pan": 0.1457,
            "width": 0.4008, "eq_lo_db": -10.06, "eq_hi_db": -12.75, "hiss_width": 0.7457,
            "comb_f0": 327.0, "comb_f1": 66.57, "comb_fb": 0.5578}),
        ("flick", {
            "rise": 0.09435, "rise_k": 0.45, "fall": 0.03, "fall_k": 1.335, "track": 0.7737,
            "f_peak": 800.3, "up_oct": 1.338, "down_oct": 3.046, "dop_st": 2.201, "q_air": 0.6,
            "lp_ratio": 2.779, "flut_depth": 0.524, "flut_f": 44.69, "hiss_db": -2.058,
            "hiss_hp": 3720, "hiss_open": 0.7442, "hiss_pow": 1.669, "grain_rate": 381.1,
            "grain_f": 3588, "room_wet": 0.06281, "rt60": 0.307, "pan": 0.05213,
            "width": 0.5124, "end_fade": 0.02, "eq_lo_db": -12.61, "eq_hi_db": 0.8779,
            "hiss_width": 1.0, "comb_mix": 0.3188, "comb_f0": 4000, "comb_f1": 99.3,
            "comb_fb": 0.4513}),
    ],
    "whoosh_fast": [
        ("whip", {
            "rise": 0.02, "rise_k": 0.7391, "hold": 0.05657, "fall_k": 1.127, "track": 1.071,
            "f_peak": 4000, "up_oct": 0.2647, "dop_st": 10.95, "q_air": 3.505, "lp_ratio": 1.2,
            "flut_f": 25.21, "hiss_db": 0.0, "hiss_hp": 1807, "hiss_open": 0.0,
            "hiss_pow": 1.51, "grain_db": -46.8, "grain_rate": 50, "grain_f": 5843,
            "room_wet": 0.03593, "rt60": 0.6411, "pan": 0.2634, "eq_lo_db": -15.23,
            "eq_hi_db": 1.169, "comb_mix": 1.0, "comb_f0": 1464, "comb_f1": 40,
            "comb_fb": 0.7323}),
    ],
}


def preset(cat: str, name: str) -> dict[str, Any]:
    """The base of cat with the member's overrides on top."""
    return {**BASES[cat], **dict(FAMILIES[cat])[name]}


def preset_full(cat: str, params: dict) -> dict[str, Any]:
    """The base of cat with params on top (a member stored as overrides becomes a full set)."""
    return {**BASES[cat], **params}


def recipe(cat: str = "swish", name: str | None = None) -> types.SimpleNamespace:
    """A recipe object for one category; it uses the brightswish module name."""
    member = name if name is not None else FAMILIES[cat][0][0]
    return types.SimpleNamespace(
        TARGET=TARGETS[cat], ALTS=list(ALTS), REFSET=cat, SPACE=SPACE,
        DEFAULTS=preset(cat, member), render=render, MODULE=MODULE, PRESET=member)


def falling_comb(x: np.ndarray, f0: float, f1: float, fb: float, mix: float) -> np.ndarray:
    """P4 on one channel: a feedback comb whose fundamental falls from f0 to f1."""
    n = len(x)
    pos = np.arange(n) - SR / (f0 * (f1 / f0) ** (np.arange(n) / max(1, n - 1)))
    z = np.array(x, dtype=float)
    for i in range(n):
        p = pos[i]
        if p >= 1.0:
            j = int(p)
            fr = p - j
            z[i] += fb * ((1.0 - fr) * z[j] + fr * z[j + 1] if j + 1 < i else z[j])
    return x + mix * ((1.0 - fb) * z - x)


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """One take (section 4.6): the whoosh render, the falling comb, a fade, peak 0.89."""
    q = {**SWISH, **p}
    y = whoosh.render({k: v for k, v in q.items() if k not in COMB_KEYS}, rng)
    if q["comb_mix"] >= 0.01:
        y = np.column_stack([
            falling_comb(y[:, c], q["comb_f0"], q["comb_f1"], q["comb_fb"], q["comb_mix"])
            for c in range(y.shape[1])])
    y = dsp.fade(y, 0.0, min(q["end_fade"], 0.3 * len(y) / SR))
    return PEAK * y / (np.max(np.abs(y)) + u.EPS)
