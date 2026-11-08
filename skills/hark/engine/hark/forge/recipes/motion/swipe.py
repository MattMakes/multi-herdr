"""swipe: cloth or paper moved past the listener (recipes-motion part 1, section 3).

A parameter set on the whoosh pass engine for UI slides: most energy in the low layer,
a rough noise texture (rustle and grains), no whistle, slow smooth envelopes, wide.
"""

from __future__ import annotations

import types
from typing import Any

import numpy as np

from hark.forge.recipes.motion import whoosh

MODULE = "hark.forge.recipes.motion.swipe"
TARGET = "a soft cloth swipe"
ALTS = ["a gust of wind", "a broom on a floor", "paper tearing", "a zipper pulled"]
REFSET = "swipe"
KIND = "swipe"

SPACE = whoosh.SPACE
CHARACTER = tuple(whoosh.CHARACTER) + ("rustle", "grain_rate", "hiss_rustle", "low_lag")

_STEP_A = {**whoosh.R2, **{
    "rise": 0.12, "rise_k": 0.9, "hold": 0.03, "fall": 0.14, "fall_k": 0.8, "start_db": -34.0,
    "track": 0.7, "f_peak": 350.0, "up_oct": 1.2, "down_oct": 2.0, "dop_st": 1.0, "dop_w": 0.05,
    "q_air": 0.9, "lp_ratio": 3.5, "flut_depth": 0.3, "flut_f": 14.0, "rustle": 0.55,
    "rustle_f": 90.0, "low_db": 4.0, "low_f": 110.0, "low_smooth": 0.05, "sub_db": -50.0,
    "hiss_db": -24.0, "hiss_hp": 2500.0, "hiss_open": 0.8, "hiss_pow": 1.5, "tone_db": -50.0,
    "grain_db": -24.0, "grain_rate": 600.0, "grain_f": 2600.0, "room_wet": 0.12, "rt60": 0.35,
    "pan": 0.15, "width": 0.35, "drive_db": 1.5, "end_db": -66.0, "end_fade": 0.05,
}}
_STEP_B = {**_STEP_A, **{
    "rise": 0.16, "rise_k": 1.0, "hold": 0.03, "fall": 0.17, "fall_k": 0.65, "track": 0.7,
    "f_peak": 380.0, "q_air": 0.8, "lp_ratio": 4.0, "rustle": 0.5, "rustle_f": 110.0,
    "low_db": -3.0, "low_f": 120.0, "low_smooth": 0.08, "low_lag": 0.05, "eq_lo_db": -2.0,
    "eq_lo_f": 120.0, "hiss_db": -15.0, "hiss_lo": 1800.0, "hiss_hp": 3000.0, "hiss_open": 1.2,
    "hiss_pow": 1.0, "hiss_rustle": 0.7, "grain_db": -18.0, "grain_rate": 1500.0,
    "grain_f": 5500.0, "width": 0.4, "eq_hi_db": -4.0, "eq_hi_f": 9000.0,
}}
DEFAULTS: dict[str, Any] = {**_STEP_B, **{
    "hiss_pre": 0.4, "hiss_tail": 0.5, "hiss_mfloor": 0.6, "hiss_width": 0.7, "room_damp": 0.3,
    "low_lag": 0.02, "low_smooth": 0.05, "low_db": -5.0, "low_width": 0.75, "hiss_db": -20.0,
}}

PRESETS: dict[str, dict[str, dict[str, float]]] = {"swipe": {
    "cloth": {},
    "paper": {"rise": 0.12, "fall": 0.13, "f_peak": 520, "q_air": 1.1, "low_db": -6, "low_f": 150,
              "grain_db": -16, "grain_rate": 2500, "grain_f": 6000, "hiss_db": -19,
              "hiss_lo": 2000, "hiss_rustle": 0.5, "rustle_f": 150, "width": 0.3},
    "velvet": {"rise": 0.2, "fall": 0.22, "fall_k": 0.6, "hold": 0.05, "f_peak": 260,
               "low_db": -1, "low_f": 95, "low_smooth": 0.07, "low_lag": 0.06, "rustle": 0.35,
               "rustle_f": 70, "grain_db": -24, "hiss_db": -19, "flut_depth": 0.4},
    "flick": {"rise": 0.1, "rise_k": 0.8, "fall": 0.13, "f_peak": 450, "flut_depth": 0.55,
              "flut_f": 32, "low_db": -7, "low_smooth": 0.06, "low_lag": 0.03, "grain_db": -18,
              "grain_rate": 900, "hiss_db": -17, "pan": 0.3},
    "glide": {"rise": 0.18, "hold": 0.1, "fall": 0.16, "fall_k": 0.9, "f_peak": 450,
              "up_oct": 1.6, "down_oct": 1.6, "q_air": 1.0, "rustle": 0.2, "hiss_rustle": 0.35,
              "low_db": -5, "grain_db": -32, "hiss_db": -19, "dop_st": 2, "dop_w": 0.1,
              "width": 0.45, "room_wet": 0.18, "rt60": 0.5},
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
    """A recipe object that uses the swipe seed bank."""
    return types.SimpleNamespace(
        TARGET=TARGET, ALTS=list(ALTS), REFSET=REFSET, SPACE=SPACE,
        DEFAULTS=dict(defaults if defaults is not None else DEFAULTS), render=render, KIND=kind,
        MODULE=MODULE)


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """DEFAULTS, then the whoosh OPTIONAL off values, then p, on the whoosh engine."""
    return whoosh.render({**DEFAULTS, **whoosh.OPTIONAL, **p}, rng)
