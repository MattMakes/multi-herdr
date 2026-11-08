"""whoosh: the pass engine (recipes-motion part 1, section 1).

An air pass with one shared level curve: the level rises to one peak and falls into a
smooth tail, the filters open with the level and sink after the peak, and a Doppler
glide moves the pitch from high to low. Parameter sets without ``rise`` are version-1
sets and render on ``render_v1``.
"""

from __future__ import annotations

import types
from typing import Any

import numpy as np

from hark.forge.engine import dsp, fx
from hark.forge.recipes.motion import _util as u
from hark.forge.recipes.motion._whoosh_v1 import DEFAULTS_V1, render_v1

SR = u.SR
MODULE = "hark.forge.recipes.motion.whoosh"

KINDS: dict[str, dict[str, Any]] = {
    "fast": {"target": "a fast crisp air whoosh",
             "alts": ["a wind gust", "rain", "a sword swing", "a jet engine"],
             "refset": "whoosh_fast"},
    "big": {"target": "a big deep whoosh transition",
            "alts": ["a strong wind", "a jet plane flying over", "waves", "a car passing"],
            "refset": "whoosh_big"},
    "pro_big": {"target": "a deep low designed whoosh swell",
                "alts": ["a strong wind", "a jet plane flying over", "a car passing", "a bass drum"],
                "refset": "pro_whoosh_big"},
}

TARGET = KINDS["fast"]["target"]
ALTS = list(KINDS["fast"]["alts"])
REFSET = KINDS["fast"]["refset"]
REFS = [f"whoosh/whoosh_fast_{i:02d}.wav" for i in range(1, 5)]

SPACE: dict[str, tuple] = {
    "rise": (0.02, 0.6, "log"), "rise_k": (0.45, 1.6), "hold": (0.0, 0.25),
    "fall": (0.03, 0.8, "log"), "fall_k": (0.5, 2.2), "start_db": (-44.0, -22.0),
    "track": (0.3, 1.2), "f_peak": (60.0, 4000.0, "log"), "up_oct": (0.0, 3.0),
    "down_oct": (0.0, 3.5), "dop_st": (0.0, 12.0), "dop_w": (0.005, 0.15, "log"),
    "q_air": (0.6, 5.0, "log"), "lp_ratio": (1.2, 8.0, "log"), "flut_depth": (0.0, 0.6),
    "flut_f": (6.0, 60.0, "log"), "rustle": (0.0, 0.9), "rustle_f": (40.0, 260.0, "log"),
    "low_db": (-40.0, 6.0), "low_f": (50.0, 400.0, "log"), "low_smooth": (0.002, 0.08, "log"),
    "sub_db": (-50.0, 10.0), "sub_f": (32.0, 110.0, "log"), "sub_glide": (0.0, 1.5),
    "sub_drive": (0.0, 3.0), "hiss_db": (-45.0, 0.0), "hiss_hp": (800.0, 8000.0, "log"),
    "hiss_open": (0.0, 2.5), "hiss_pow": (1.0, 4.0), "tone_db": (-50.0, -3.0),
    "tone_q": (6.0, 60.0, "log"), "tone_ratio": (0.5, 4.0, "log"), "tone_pure": (0.0, 1.0),
    "grain_db": (-50.0, -3.0), "grain_rate": (50.0, 3000.0, "log"),
    "grain_f": (800.0, 9000.0, "log"), "room_wet": (0.0, 0.5), "rt60": (0.15, 2.0, "log"),
    "pan": (0.0, 0.5), "width": (0.0, 0.6), "drive_db": (0.0, 9.0), "end_db": (-75.0, -50.0),
    "end_fade": (0.02, 0.15, "log"), "hiss_rustle": (0.0, 1.0), "hiss_lo": (600.0, 4000.0, "log"),
    "eq_lo_db": (-18.0, 3.0), "eq_lo_f": (80.0, 300.0, "log"), "eq_hi_db": (-15.0, 12.0),
    "eq_hi_f": (1500.0, 9000.0, "log"), "low_lag": (0.0, 0.08), "hiss_pre": (0.3, 3.0),
    "hiss_tail": (0.3, 3.0), "hiss_mfloor": (0.0, 1.0), "hiss_width": (0.0, 1.0),
    "room_damp": (0.1, 0.8), "low_width": (0.0, 1.0), "tail_sweep": (0.0, 3.0),
    "tail_tau": (0.05, 0.6, "log"), "hiss_q": (0.6, 3.0), "tail_gust": (0.0, 0.8),
}

OPTIONAL: dict[str, float | None] = {
    "low_lag": 0.0, "hiss_rustle": 0.0, "hiss_lo": None, "eq_lo_db": 0.0, "eq_lo_f": 150.0,
    "eq_hi_db": 0.0, "eq_hi_f": 4000.0, "hiss_pre": None, "hiss_tail": None,
    "hiss_mfloor": 0.0, "hiss_width": 0.0, "room_damp": 0.55, "low_width": 0.0,
    "tail_sweep": 0.0, "tail_tau": 0.25, "hiss_q": 0.6, "tail_gust": 0.0,
}

R2: dict[str, float] = {
    "rise": 0.065, "rise_k": 0.9, "hold": 0.0, "fall": 0.075, "fall_k": 0.75, "start_db": -34.0,
    "track": 0.45, "f_peak": 420.0, "up_oct": 1.5, "down_oct": 1.8, "dop_st": 2.0, "dop_w": 0.03,
    "q_air": 1.1, "lp_ratio": 3.0, "flut_depth": 0.18, "flut_f": 24.0, "rustle": 0.0,
    "rustle_f": 90.0, "low_db": -9.0, "low_f": 130.0, "low_smooth": 0.03, "sub_db": -40.0,
    "sub_f": 60.0, "sub_glide": 0.3, "sub_drive": 0.5, "hiss_db": -13.0, "hiss_hp": 2500.0,
    "hiss_open": 1.2, "hiss_pow": 1.8, "tone_db": -40.0, "tone_q": 20.0, "tone_ratio": 1.7,
    "tone_pure": 0.0, "grain_db": -50.0, "grain_rate": 300.0, "grain_f": 3000.0,
    "room_wet": 0.12, "rt60": 0.35, "pan": 0.15, "width": 0.12, "drive_db": 2.0,
    "end_db": -66.0, "end_fade": 0.04,
}

FAST: dict[str, float] = {**R2, **{
    "rise": 0.13, "rise_k": 0.7, "hold": 0.005, "fall": 0.2, "fall_k": 0.58, "track": 0.5,
    "down_oct": 2.0, "q_air": 1.0, "lp_ratio": 4.0, "low_db": -15.0, "low_f": 170.0,
    "low_smooth": 0.04, "hiss_db": -24.0, "hiss_hp": 2000.0, "hiss_open": 0.9, "hiss_pow": 1.2,
    "room_wet": 0.1, "rt60": 0.58, "end_fade": 0.06, "eq_lo_db": -7.0, "eq_lo_f": 145.0,
    "eq_hi_db": -4.0, "eq_hi_f": 6000.0, "room_damp": 0.3, "hiss_tail": 0.75,
    "hiss_mfloor": 0.35, "hiss_pre": 0.8, "tail_sweep": 0.6, "tail_tau": 0.25, "hiss_q": 1.4,
}}

BIG: dict[str, float] = {**R2, **{
    "rise": 0.36, "rise_k": 0.8, "fall": 0.36, "fall_k": 0.85, "start_db": -36.0, "track": 0.8,
    "f_peak": 380.0, "up_oct": 1.4, "down_oct": 2.3, "dop_st": 3.0, "dop_w": 0.08, "q_air": 1.2,
    "lp_ratio": 4.5, "flut_depth": 0.25, "flut_f": 12.0, "low_db": -2.0, "low_f": 120.0,
    "low_smooth": 0.04, "sub_db": -6.0, "sub_f": 55.0, "sub_glide": 0.5, "sub_drive": 1.0,
    "hiss_db": -16.0, "hiss_hp": 2000.0, "hiss_open": 0.8, "hiss_pow": 1.3, "room_wet": 0.22,
    "rt60": 0.9, "pan": 0.12, "width": 0.2, "drive_db": 3.0, "end_fade": 0.08,
    "hiss_tail": 0.75, "hiss_mfloor": 0.35, "room_damp": 0.35,
}}

PRO_BIG: dict[str, float] = {
    **{k: v for k, v in BIG.items() if k not in ("hiss_tail", "hiss_mfloor", "room_damp")},
    **{"rise": 0.3, "rise_k": 1.2, "hold": 0.02, "fall": 0.165, "fall_k": 0.8, "start_db": -34.0,
       "f_peak": 90.0, "up_oct": 0.8, "down_oct": 1.5, "dop_st": 2.0, "low_db": 0.0,
       "low_f": 90.0, "low_smooth": 0.03, "sub_db": 2.0, "sub_f": 46.0, "sub_glide": 0.5,
       "sub_drive": 0.8, "q_air": 0.8, "lp_ratio": 5.0, "hiss_db": -30.0, "hiss_hp": 1000.0,
       "hiss_open": 1.4, "hiss_pow": 1.2, "room_wet": 0.1, "rt60": 0.5, "pan": 0.0,
       "width": 0.03, "drive_db": 2.0, "end_db": -64.0, "end_fade": 0.06},
}

BASES: dict[str, dict[str, float]] = {"fast": FAST, "big": BIG, "pro_big": PRO_BIG}
DEFAULTS: dict[str, float] = dict(FAST)

PRESETS: dict[str, dict[str, dict[str, float]]] = {
    "fast": {
        "crisp": {"grain_db": -17, "grain_rate": 1400, "grain_f": 4800, "hiss_db": -27},
        "sleeve": {"hiss_rustle": 0.5, "hiss_pre": 1.3, "hiss_db": -20, "hiss_lo": 1200,
                   "eq_hi_db": -10},
        "whistle": {"tone_db": -3, "tone_q": 50, "tone_pure": 1.0, "tone_ratio": 1.5, "dop_st": 12,
                    "dop_w": 0.04, "tail_tau": 0.4, "hiss_q": 2.2},
        "heavy": {"fall": 0.19, "f_peak": 300, "flut_depth": 0.35, "flut_f": 20, "room_wet": 0.14,
                  "rt60": 0.55, "tail_tau": 0.4, "hiss_q": 2.2},
        "reverse": {"rise": 0.25, "rise_k": 0.55, "hold": 0.0, "fall": 0.08, "fall_k": 1.0,
                    "room_wet": 0.15},
        "tight": {"fall": 0.1, "fall_k": 0.75, "f_peak": 750, "q_air": 2.5, "room_wet": 0.12,
                  "rt60": 0.35, "grain_db": -20, "grain_rate": 2000, "grain_f": 6000,
                  "tail_sweep": 0.0, "hiss_q": 0.6},
    },
    "big": {
        "swell": {},
        "cape": {"rise": 0.26, "rise_k": 0.9, "fall": 0.32, "hold": 0.05, "f_peak": 330, "q_air": 0.9,
                 "flut_depth": 0.6, "flut_f": 13, "rustle": 0.6, "rustle_f": 70, "hiss_db": -14,
                 "hiss_lo": 1400, "hiss_rustle": 0.6, "grain_db": -24, "grain_rate": 700,
                 "grain_f": 3500, "low_db": -4, "sub_db": -24, "width": 0.45, "pan": 0.25,
                 "room_wet": 0.18},
        "jet": {"tone_db": -3, "tone_q": 50, "tone_ratio": 1.4, "tone_pure": 0.9, "dop_st": 12,
                "dop_w": 0.1, "f_peak": 700, "up_oct": 1.0, "down_oct": 2.6, "q_air": 1.4,
                "sub_db": -18, "low_db": -8, "hiss_db": -20, "rise": 0.42, "fall": 0.36, "pan": 0.3},
        "deep": {"rise": 0.5, "fall": 0.45, "f_peak": 260, "up_oct": 0.9, "down_oct": 1.8,
                 "low_db": -1, "low_f": 90, "sub_db": -2, "sub_f": 46, "sub_glide": 0.8,
                 "sub_drive": 1.4, "flut_depth": 0.55, "flut_f": 6, "drive_db": 7, "grain_db": -26,
                 "grain_rate": 200, "grain_f": 1200, "hiss_db": -22, "rt60": 1.3, "room_wet": 0.28,
                 "width": 0.08, "pan": 0.0},
        "gust": {"rise": 0.45, "rise_k": 1.1, "fall": 0.45, "f_peak": 700, "up_oct": 1.8,
                 "down_oct": 2.4, "q_air": 0.8, "lp_ratio": 3.5, "low_db": -12, "sub_db": -50,
                 "hiss_db": -14, "hiss_hp": 2200, "flut_depth": 0.6, "flut_f": 6, "width": 0.5,
                 "room_wet": 0.35, "rt60": 1.0},
        "reverse": {"rise": 0.6, "rise_k": 0.55, "hold": 0.0, "fall": 0.06, "fall_k": 1.4,
                    "room_wet": 0.2, "rt60": 0.6},
    },
    "pro_big": {
        "swell": {},
        "reverse": {"rise": 0.5, "rise_k": 0.55, "hold": 0.0, "fall": 0.05, "fall_k": 1.4},
        "long": {"rise": 0.42, "fall": 0.24, "hold": 0.05, "sub_f": 42, "sub_glide": 0.7, "rt60": 0.8,
                 "flut_depth": 0.6, "flut_f": 6},
        "windy": {"f_peak": 200, "q_air": 0.7, "lp_ratio": 3.0, "hiss_db": -27, "hiss_rustle": 0.5,
                  "hiss_lo": 1200, "rustle": 0.5, "rustle_f": 50, "sub_db": -2},
        "tonal": {"sub_db": 8, "low_db": -10, "sub_f": 70, "sub_glide": 1.5, "sub_drive": 0.3,
                  "tone_db": -6, "tone_q": 50, "tone_ratio": 4.0, "tone_pure": 1.0, "dop_st": 12},
        "gritty": {"drive_db": 9, "sub_drive": 3.0, "grain_db": -20, "grain_rate": 300,
                   "grain_f": 1500},
    },
}

CHARACTER: tuple[str, ...] = (
    "rise", "fall", "hold", "f_peak", "up_oct", "down_oct", "dop_st", "low_db", "sub_db", "sub_f",
    "hiss_db", "tone_db", "tone_ratio", "grain_db", "grain_f", "width", "pan", "fall_k",
    "flut_depth", "rustle", "hiss_rustle", "tone_pure", "q_air",
)

shelf = u.shelf

__all__ = [
    "SPACE", "OPTIONAL", "DEFAULTS", "BASES", "PRESETS", "KINDS", "CHARACTER", "TARGET", "ALTS",
    "REFSET", "REFS", "preset", "local_space", "recipe", "envelope", "render", "shelf",
    "render_v1", "DEFAULTS_V1",
]


def preset(kind: str, member: str) -> dict[str, Any]:
    """The base set of a kind with the member's overrides on top."""
    return {**BASES[kind], **PRESETS[kind][member]}


def _box(spec: tuple, v: float, w: float) -> tuple | None:
    lo, hi = float(spec[0]), float(spec[1])
    log = len(spec) > 2 and spec[2] == "log"
    v = float(np.clip(v, lo, hi))
    if log:
        lo, hi, v = np.log(lo), np.log(hi), np.log(v)
    a, b = max(lo, v - w * (hi - lo)), min(hi, v + w * (hi - lo))
    if b - a < 1e-9:
        return None
    return (float(np.exp(a)), float(np.exp(b)), "log") if log else (float(a), float(b))


def local_space(params: dict, space: dict | None = None, keys=None, char_w: float = 0.12,
                free_w: float = 0.3, character=None) -> dict:
    """A narrow search box around one parameter set (section 1.9)."""
    space = SPACE if space is None else space
    character = CHARACTER if character is None else character
    out = {}
    for k in _box_keys(params, space, keys):
        box = _box(space[k], params[k], char_w if k in character else free_w)
        if box is not None:
            out[k] = box
    return out


def _box_keys(params: dict, space: dict, keys) -> list[str]:
    """The range keys of space that params sets, limited to keys when given."""
    wanted = space if keys is None else [k for k in space if k in keys]
    return [k for k in wanted if not isinstance(space[k], list) and params.get(k) is not None]


def recipe(kind: str = "fast", defaults: dict | None = None,
           module_name: str = MODULE) -> types.SimpleNamespace:
    """A recipe object for one kind; its module name picks the seed bank."""
    return types.SimpleNamespace(SPACE=SPACE, render=render, KIND=kind, MODULE=module_name,
                                 **_kind_fields(kind, defaults))


def _kind_fields(kind: str, defaults: dict | None) -> dict[str, Any]:
    info = KINDS[kind]
    return {"TARGET": info["target"], "ALTS": list(info["alts"]), "REFSET": info["refset"],
            "DEFAULTS": dict(defaults if defaults is not None else BASES[kind])}


def envelope(p: dict) -> tuple[np.ndarray, np.ndarray, float, int]:
    """The shared level curve (section 1.10): (t, a, tp, n)."""
    tp = float(float(p["rise"]) * (max(-float(p["start_db"]), 1.0) / 30.0) ** (1.0 / p["rise_k"]))
    n = _env_samples(p, tp)
    t = u.times(n)
    a = _level(p, t - tp)
    return t, a / a.max(), tp, n


def _env_samples(p: dict, tp: float) -> int:
    """Steps 2 and 3 of 1.10: the end estimate, at most 4.5 s."""
    room = 0.9 * p["rt60"] * min(1.0, 4 * p["room_wet"] + 0.2)
    t_end = tp + p["hold"] + p["fall"] * (95.0 / 30.0) ** (1.0 / p["fall_k"]) + room + 0.05
    return u.samples(min(tp + max(t_end - tp, 0.1), 4.5))


def _level(p: dict, d: np.ndarray) -> np.ndarray:
    """Steps 4 to 6 of 1.10: the level in dB as amplitude, with a rounded peak."""
    rise, fall, hold = float(p["rise"]), float(p["fall"]), float(p["hold"])
    pre = d < 0
    x = np.where(pre, -d / rise, np.maximum(d - hold, 0.0) / fall)
    k = np.where(pre, p["rise_k"], p["fall_k"])
    a = 10 ** (np.maximum(-30.0 * x**k, -140.0) / 20.0)
    return u.hann_smooth(a, float(np.clip(0.3 * min(rise, fall + hold), 0.004, 0.1)))


def _opt(p: dict, key: str) -> Any:
    v = p.get(key, OPTIONAL[key])
    return OPTIONAL[key] if v is None and OPTIONAL[key] is not None else v


class _Pass:
    """The shared motion state of one version-2 render."""

    def __init__(self, p: dict, rng: np.random.Generator):
        self.p, self.rng = p, rng
        self.t, self.a, self.tp, self.n = envelope(p)
        self.d = self.t - self.tp
        self.m = self.a ** p["track"]
        self.dop = 2 ** (-(p["dop_st"] / 12.0) * 0.5 * np.tanh(self.d / p["dop_w"]))
        self.sweep = _opt(p, "tail_sweep")
        self.g = 1 - np.exp(-np.maximum(self.d, 0) / _opt(p, "tail_tau"))
        self.f_air = _air_centre(p, self.d, self.m, self.dop, 0.5 * self.sweep * self.g)
        fd = p["flut_depth"]
        self.flut = np.exp(fd * u.control(self.n, p["flut_f"], rng) - 0.5 * fd * fd)

    def white(self) -> np.ndarray:
        return self.rng.standard_normal(self.n)

    def am(self, depth: float, fc: float) -> np.ndarray:
        v = np.exp(depth * u.control(self.n, fc, self.rng))
        return v / (u.rms(v) + u.EPS)


def _air_centre(p: dict, d: np.ndarray, m: np.ndarray, dop: np.ndarray,
                sink: np.ndarray) -> np.ndarray:
    """Steps 3 and 4 of 1.11: the air band centre, sunk by the tail sweep."""
    span = np.where(d < 0, p["up_oct"], p["down_oct"])
    f_air = 2 ** (np.log2(p["f_peak"]) - span * (1 - m)) * dop
    return f_air * 2 ** (-sink) if _opt(p, "tail_sweep") > 0 else f_air


def _air_body(s: _Pass) -> np.ndarray:
    p = s.p
    body = np.zeros(s.n)
    for mult, gain in ((1.0, 1.0), (1.6, 0.55), (2.5, 0.3)):
        fc = np.clip(s.f_air * mult, 30, 20000)
        body += gain * dsp.svf(u.colored(s.n, s.rng, 0.5), fc, p["q_air"], "bandn")
    body = dsp.svf(body, np.clip(s.f_air * p["lp_ratio"], 60, 20000), 0.7, "low")
    if p["rustle"] > 0:
        body = body * s.am(1.2 * p["rustle"], p["rustle_f"])
    return body * s.a * s.flut


def _low_band(s: _Pass, env: np.ndarray) -> np.ndarray:
    p = s.p
    y = dsp.svf(u.colored(s.n, s.rng, 1.0), np.clip(p["low_f"] * (0.6 + 0.4 * s.m), 25, 2000),
                0.75, "low")
    return dsp.butter(y, "high", max(28.0, p["low_f"] / 2.5), 2) * env


def _low_layers(s: _Pass) -> tuple[np.ndarray, np.ndarray | None]:
    env = u.hann_smooth(s.a, s.p["low_smooth"])
    env = env / env.max()
    lag = int(round(_opt(s.p, "low_lag") * SR))
    if lag > 0:
        env = np.concatenate([np.zeros(lag), env])[: s.n]
    first = _low_band(s, env)
    second = _low_band(s, env) if _opt(s.p, "low_width") > 0 else None
    return first, second


def _sub(s: _Pass) -> np.ndarray:
    p = s.p
    env = u.hann_smooth(s.a, max(p["low_smooth"], 0.02))
    env = env / env.max()
    glide = 0.5 + 0.5 * np.tanh(s.d / (0.5 * p["fall"] + 0.02))
    f_sub = p["sub_f"] * 2 ** (-p["sub_glide"] * glide)
    y = np.sin(2 * np.pi * np.cumsum(f_sub) / SR + s.rng.uniform(0, 2 * np.pi))
    if p["sub_drive"] > 0:
        g, b = 1 + 2 * p["sub_drive"], 0.15 * p["sub_drive"]
        y = np.tanh(g * y + b) - np.tanh(b)
    return dsp.butter(y, "low", 600.0, 2) * env


def _hiss_filters(s: _Pass) -> tuple[Any, np.ndarray, bool]:
    p = s.p
    fabric = _opt(p, "hiss_rustle") > 0
    hiss_lo = p.get("hiss_lo")
    h_lo = (hiss_lo if hiss_lo is not None else p["hiss_hp"]) if fabric else p["hiss_hp"]
    floor = _opt(p, "hiss_mfloor")
    mh = s.m if floor <= 0 else floor + (1 - floor) * s.m
    h_lp = np.clip(p["hiss_hp"] * 2 ** (p["hiss_open"] * mh) * 1.5 * s.dop, 500, 20000)
    if s.sweep > 0:
        sh = 2 ** (-s.sweep * s.g)
        return np.clip(h_lo * sh, 150, 16000), np.clip(h_lp * sh, 300, 20000), fabric
    return float(h_lo), h_lp, fabric


def _hiss_env(s: _Pass) -> np.ndarray:
    p = s.p
    pre, post = p.get("hiss_pre"), p.get("hiss_tail")
    if pre is None and post is None:
        e = s.a ** p["hiss_pow"]
    else:
        w = 0.5 + 0.5 * np.tanh(s.d / 0.005)
        pre = p["hiss_pow"] if pre is None else pre
        post = p["hiss_pow"] if post is None else post
        e = (1 - w) * s.a**pre + w * s.a**post
    gust = _opt(p, "tail_gust")
    if gust > 0:
        gw = np.clip(s.d / 0.04, 0, 1)
        gu = np.exp(gust * u.control(s.n, 5.5, s.rng) - 0.5 * gust * gust)
        e = e * (1 - gw + gw * gu)
    return e * s.flut


def _one_hiss(s: _Pass, h_hp: Any, h_lp: np.ndarray, fabric: bool) -> np.ndarray:
    p = s.p
    if np.ndim(h_hp):
        y = dsp.svf(dsp.svf(s.white(), h_hp, 0.54, "high"), h_hp, 1.31, "high")
    else:
        y = dsp.butter(s.white(), "high", h_hp, 4)
    y = dsp.svf(y, h_lp, _opt(p, "hiss_q"), "low")
    if fabric:
        y = y * s.am(1.4 * _opt(p, "hiss_rustle"), 1.8 * p["rustle_f"])
    return y


def _hiss_layers(s: _Pass) -> tuple[np.ndarray, np.ndarray | None]:
    h_hp, h_lp, fabric = _hiss_filters(s)
    e = _hiss_env(s)
    first = _one_hiss(s, h_hp, h_lp, fabric) * e
    second = None
    if _opt(s.p, "hiss_width") > 0:
        second = _one_hiss(s, h_hp, h_lp, fabric) * e
    return first, second


def _whistle(s: _Pass) -> np.ndarray:
    p = s.p
    ft = np.clip(s.f_air * p["tone_ratio"], 40, 16000)
    band = dsp.svf(s.white(), ft, p["tone_q"], "bandn")
    band = band / (np.std(band) + u.EPS)
    sine = np.sqrt(2) * np.sin(2 * np.pi * np.cumsum(ft) / SR)
    return ((1 - p["tone_pure"]) * band + p["tone_pure"] * sine) * s.a**1.5


def _grains(s: _Pass) -> np.ndarray:
    p = s.p
    hits = s.rng.random(s.n) < p["grain_rate"] * s.a / SR
    weights = s.rng.lognormal(0.0, 0.5, s.n)
    bumps = u.hann_smooth(hits * weights, 0.003)
    band = dsp.svf(s.white(), np.clip(p["grain_f"] * s.dop, 200, 18000), 1.2, "bandn")
    return band * bumps * s.a


def _layer(y: np.ndarray, db: float) -> np.ndarray:
    return 10 ** (db / 20.0) * y / u.block_peak_rms(y)


def _mix(s: _Pass) -> tuple[np.ndarray, dict]:
    p = s.p
    x = _layer(_air_body(s), 0.0)
    sides: dict[str, Any] = {}
    if p["low_db"] > -39.5:
        low, sides["low"] = _low_layers(s)
        sides["low_ref"] = low
        x = x + _layer(low, p["low_db"])
    if p["sub_db"] > -49.5:
        x = x + _layer(_sub(s), p["sub_db"])
    if p["hiss_db"] > -44.5:
        hiss, sides["hiss"] = _hiss_layers(s)
        sides["hiss_ref"] = hiss
        x = x + _layer(hiss, p["hiss_db"])
    if p["tone_db"] > -49.5:
        x = x + _layer(_whistle(s), p["tone_db"])
    if p["grain_db"] > -49.5:
        x = x + _layer(_grains(s), p["grain_db"])
    return x, sides


def _side(s: _Pass, x: np.ndarray, sides: dict) -> np.ndarray:
    p = s.p
    side = np.zeros(s.n)
    if p["width"] > 0:
        air = dsp.svf(s.white(), np.clip(2 * s.f_air, 200, 18000), 0.8, "bandn") * s.a * s.flut
        air = dsp.butter(air, "high", 300.0, 2)
        side += p["width"] * 0.5 * u.block_peak_rms(x) / u.block_peak_rms(air) * air
    if sides.get("hiss") is not None:
        side += (_opt(p, "hiss_width") * 10 ** (p["hiss_db"] / 20.0) * sides["hiss"]
                 / u.block_peak_rms(sides["hiss_ref"]))
    if sides.get("low") is not None:
        side += (_opt(p, "low_width") * 10 ** (p["low_db"] / 20.0) * sides["low"]
                 / u.block_peak_rms(sides["low_ref"]))
    return side


def _stereo(s: _Pass, x: np.ndarray, sides: dict) -> np.ndarray:
    p = s.p
    direction = 1.0 if s.rng.random() < 0.5 else -1.0
    pan = direction * p["pan"] * np.tanh(s.d / max(p["dop_w"], 0.01))
    side = _side(s, x, sides)
    return np.column_stack([x * (1 + pan) + side, x * (1 - pan) - side])


def _space(y: np.ndarray, p: dict, rng: np.random.Generator) -> np.ndarray:
    if p["room_wet"] <= 0.005:
        return y
    kw = dict(rt60=p["rt60"], size=0.45, predelay=0.006, damp=_opt(p, "room_damp"),
              wet=p["room_wet"], width=min(1.0, 0.3 + 1.5 * p["width"]), lowcut=120.0,
              stereo=True, tail=max(0.2, 1.1 * p["rt60"]))
    wet = fx.reverb(y, rng, lines=8, **kw)
    if not np.all(np.isfinite(wet)):
        wet = fx.reverb(y, rng, lines=16, **kw)
    return np.asarray(wet, dtype=float)[: len(y)]


def _finish(y: np.ndarray, p: dict) -> np.ndarray:
    if p["drive_db"] > 0.05:
        y = fx.saturate(y / (np.max(np.abs(y)) + u.EPS), p["drive_db"], mode="tape", os=2)
    if abs(_opt(p, "eq_lo_db")) > 0.05:
        y = u.shelf(y, _opt(p, "eq_lo_f"), _opt(p, "eq_lo_db"), high=False, s=0.8)
    if abs(_opt(p, "eq_hi_db")) > 0.05:
        y = u.shelf(y, _opt(p, "eq_hi_f"), _opt(p, "eq_hi_db"), high=True, s=0.8)
    y = dsp.butter(y, "high", 25.0, 4)
    return dsp.butter(y, "low", 19000.0, 4)


def edit(y: np.ndarray, end_db: float, end_fade: float) -> np.ndarray:
    """The edit of section 1.12: cut where the level falls end_db under the peak, then fade."""
    end = _edit_end(u.mono(y), end_db)
    if end is not None:
        y = y[: min(len(y), end + u.samples(end_fade / 2))]
    return _edit_fades(np.array(y, dtype=float), end_fade)


def _edit_end(m: np.ndarray, end_db: float) -> int | None:
    """Steps 1 to 3 of 1.12: the first sample of the block under the end level (None: no block)."""
    k = len(m) // 240
    if k < 1:
        return None
    blocks = m[: k * 240].reshape(k, 240)
    e = 20 * np.log10(np.sqrt(np.mean(blocks * blocks, axis=1)) + u.EPS)
    e = np.convolve(e, np.ones(4) / 4.0, mode="same")
    i = int(np.argmax(e))
    below = np.nonzero(e[i:] < e[i] + end_db)[0]
    return 240 * (i + int(below[0]) if below.size else k)


def _edit_fades(y: np.ndarray, end_fade: float) -> np.ndarray:
    """Steps 5 and 6 of 1.12: the raised-cosine end fade and the sin² start ramp."""
    out = min(u.samples(end_fade), len(y) - 1)
    if out > 1:
        y[-out:] *= (0.5 + 0.5 * np.cos(np.pi * np.arange(out) / (out - 1)))[:, None]
    fin = min(u.samples(0.0015), len(y))
    y[:fin] *= u.sin2_in(fin)[:, None]
    return y


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """One take of the pass engine, float64 stereo at 48 kHz."""
    if "rise" not in p:
        return u.below_full_scale(render_v1(p, rng))
    q = {**OPTIONAL, **p}
    s = _Pass(q, rng)
    x, sides = _mix(s)
    y = _stereo(s, x, sides)
    y = _finish(_space(y, q, rng), q)
    return u.below_full_scale(edit(y, q["end_db"], q["end_fade"]))
