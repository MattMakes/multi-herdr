"""swish: a thin, fast object cutting the air (recipes-motion part 1, section 2).

A parameter set on the whoosh pass engine: short and steep, thin, with an optional
aeolian tone (a narrow band that follows the speed and the Doppler shift).
"""

from __future__ import annotations

import types
from typing import Any

import numpy as np

from hark.forge.recipes.motion import whoosh

MODULE = "hark.forge.recipes.motion.swish"
TARGET = "a thin quick swish of a cane through the air"
ALTS = ["a sword being sharpened", "a short burst of steam", "a page being flipped",
        "a bird wing flap"]
REFSET = "swish"
KIND = "swish"

SPACE = whoosh.SPACE
CHARACTER = whoosh.CHARACTER

_STEP_A = {**whoosh.R2, **{
    "rise": 0.045, "rise_k": 0.8, "hold": 0.0, "fall": 0.08, "fall_k": 0.7, "start_db": -34.0,
    "track": 0.6, "f_peak": 520.0, "up_oct": 1.4, "down_oct": 1.8, "dop_st": 4.0, "dop_w": 0.02,
    "q_air": 1.8, "lp_ratio": 2.2, "flut_depth": 0.15, "flut_f": 30.0, "low_db": -16.0,
    "low_f": 180.0, "low_smooth": 0.02, "sub_db": -50.0, "hiss_db": -14.0, "hiss_hp": 3800.0,
    "hiss_open": 1.2, "hiss_pow": 1.8, "tone_db": -16.0, "tone_q": 28.0, "tone_ratio": 1.0,
    "tone_pure": 0.25, "grain_db": -50.0, "room_wet": 0.15, "rt60": 0.35, "pan": 0.2,
    "width": 0.1, "drive_db": 1.5, "end_db": -66.0, "end_fade": 0.03,
}}
_STEP_B = {**_STEP_A, **{
    "rise": 0.07, "rise_k": 1.0, "hold": 0.004, "fall": 0.14, "fall_k": 0.58, "lp_ratio": 3.0,
    "low_db": -24.0, "low_f": 220.0, "hiss_db": -24.0, "hiss_hp": 2400.0, "hiss_pow": 1.2,
    "room_wet": 0.22, "rt60": 0.5, "end_fade": 0.05, "eq_lo_db": -10.0, "eq_lo_f": 150.0,
    "eq_hi_db": -4.0, "eq_hi_f": 6000.0,
}}
DEFAULTS: dict[str, Any] = {**_STEP_B, **{
    "fall": 0.21, "room_wet": 0.1, "room_damp": 0.3, "end_db": -62.0, "hiss_tail": 0.85,
    "hiss_mfloor": 0.3, "hiss_pre": 0.8,
}}

PRESETS: dict[str, dict[str, dict[str, float]]] = {"swish": {
    "cane": {},
    "blade": {"rise": 0.055, "fall": 0.085, "fall_k": 0.8, "f_peak": 900, "q_air": 2.8,
              "tone_db": -22, "tone_ratio": 1.3, "hiss_db": -19, "hiss_hp": 3200, "low_db": -32,
              "dop_st": 5, "room_wet": 0.1, "rt60": 0.3},
    "stick": {"rise": 0.08, "fall": 0.17, "fall_k": 0.55, "end_db": -60, "f_peak": 380,
              "q_air": 1.3, "low_db": -16, "low_f": 180, "flut_depth": 0.5, "flut_f": 9,
              "tone_db": -30, "hiss_db": -27, "room_wet": 0.14, "rt60": 0.5},
    "zing": {"rise": 0.06, "rise_k": 0.8, "fall": 0.13, "f_peak": 700, "tone_db": -3,
             "tone_q": 50, "tone_ratio": 1.2, "tone_pure": 0.9, "dop_st": 9, "dop_w": 0.015,
             "hiss_db": -25},
    "soft": {"rise": 0.08, "f_peak": 560, "q_air": 1.0, "tone_db": -40, "rustle": 0.6,
             "rustle_f": 120, "hiss_rustle": 0.6, "hiss_lo": 1500, "hiss_db": -21,
             "grain_db": -26, "grain_rate": 900, "grain_f": 3500, "width": 0.35,
             "room_wet": 0.14, "rt60": 0.5, "pan": 0.3},
}}


def preset(kind: str, member: str) -> dict[str, Any]:
    """DEFAULTS with the member's overrides on top."""
    return {**DEFAULTS, **PRESETS[kind][member]}


def local_space(params: dict, space: dict | None = None, keys=None, char_w: float = 0.12,
                free_w: float = 0.3) -> dict:
    """The whoosh local search box with this module's space and character."""
    return whoosh.local_space(params, SPACE if space is None else space, keys, char_w, free_w,
                              CHARACTER)


def recipe(kind: str = KIND, defaults: dict | None = None) -> types.SimpleNamespace:
    """A recipe object that uses the swish seed bank."""
    return types.SimpleNamespace(
        TARGET=TARGET, ALTS=list(ALTS), REFSET=REFSET, SPACE=SPACE,
        DEFAULTS=dict(defaults if defaults is not None else DEFAULTS), render=render, KIND=kind,
        MODULE=MODULE)


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """DEFAULTS, then the whoosh OPTIONAL off values, then p, on the whoosh engine."""
    return whoosh.render({**DEFAULTS, **whoosh.OPTIONAL, **p}, rng)
