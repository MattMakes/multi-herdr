"""notify: short notification cues in 4 styles (ding, success, bell, chime).

The ding is the two-stage pro ding: a bright head, a gate that drops it by about 20 dB, then a quiet tremolo body.
``measure`` and ``outside`` are shared: shimmer and the pop recipe import them.
"""

from __future__ import annotations

from typing import Any, Callable

import numpy as np

from hark.forge.engine import fx
from hark.forge.recipes.risers._common import bw, headroom, n_of, t_of

SR = 48000
TARGET = "a short bright notification ding"
ALTS = ["a phone ringtone", "an alarm clock beep", "a wine glass clink", "a xylophone note", "a typewriter bell"]
REFSET = "pro_notify"
SEMI = 2.0 ** (1 / 12)
STYLES = ["ding", "success", "bell", "chime"]

# key, SPACE spec, default
_ROWS: list[tuple[str, Any, Any]] = [
    ("style", STYLES, "ding"), ("f0", (1100.0, 2600.0, "log"), 1822.0), ("att", (0.0008, 0.012, "log"), 0.003),
    ("step_db", (-36.0, -18.0), -30.0), ("hold", (0.018, 0.07), 0.032), ("hold_tau", (0.04, 0.4, "log"), 0.15),
    ("drop_db", (-26.0, -12.0), -24.0), ("drop_ms", (2.0, 14.0), 5.0), ("ring_tau", (0.02, 0.15, "log"), 0.05),
    ("bright", (0.05, 0.9), 0.35), ("p2", (3.2, 3.6), 3.42), ("oct", (0.0, 0.3), 0.06), ("detune", (0.0, 3.0), 1.2),
    ("tick_db", (-40.0, -18.0), -30.0), ("tick_f", (4000.0, 11000.0, "log"), 7000.0),
    ("body_db", (-26.0, -12.0), -23.0), ("body_ratio", [0.5, 1.0, 1.5], 0.5), ("body_in", (0.004, 0.03), 0.012),
    ("body_tau", (0.08, 0.6, "log"), 0.3), ("body_p3", (0.0, 0.8), 0.45), ("body_p7", (0.0, 0.6), 0.6),
    ("trem_hz", (12.0, 34.0), 24.0), ("trem", (0.0, 0.85), 0.55), ("drive", (0.0, 9.0), 3.0),
    ("verb", (0.0, 0.35), 0.12), ("rt60", (0.15, 0.7, "log"), 0.3), ("width", (0.0, 1.0), 0.8),
    ("head_width", (0.0, 0.35), 0.15), ("length", (0.2, 0.42), 0.26), ("fade", (0.02, 0.09), 0.07),
    ("hp", (450.0, 900.0, "log"), 550.0), ("interval", [3, 4, 5, 7], 7), ("gap", (0.05, 0.13), 0.085),
    ("second_db", (-6.0, 1.0), -2.0), ("partials", ["celesta", "glass", "bar", "bowl", "tine"], "celesta"),
    ("decay", (0.2, 1.6, "log"), 0.8), ("slope", (0.4, 1.2), 0.8), ("hard", (0.0, 1.0), 0.4),
    ("beat", (0.3, 4.0, "log"), 1.0), ("room", (0.0, 0.3), 0.12), ("early_db", (0.0, 18.0), 0.0),
    ("early_tau", (0.02, 0.12, "log"), 0.05), ("head_drive", (-1.0, 9.0), -1.0), ("air", (-60.0, -10.0), -60.0),
    ("air_tau", (0.02, 0.2, "log"), 0.06), ("air_lo", (0.0, 12000.0), 0.0), ("p3", (0.0, 0.5), 0.0),
    ("p3_tau", (0.008, 0.06, "log"), 0.02), ("strike", (-60.0, -4.0), -60.0),
    ("strike_tau", (0.006, 0.04, "log"), 0.015), ("strike_max", (6.0, 20.0), 20.0),
    ("mat", ["film", "glass", "bowl", "tine", "wood", "fm"], "film"), ("c_fast", (0.5, 0.97), 0.9),
    ("c_tf", (0.004, 0.03, "log"), 0.012), ("c_ts", (0.06, 0.4, "log"), 0.15), ("c_mallet", (-60.0, -6.0), -30.0),
    ("c_hard", (0.0, 1.0), 0.5), ("c_bend", (0.0, 60.0), 0.0), ("c_sub", (0.0, 0.6), 0.0),
    ("c_motif", ["single", "up", "flam"], "single"), ("c_index", (0.5, 6.0), 2.5),
    ("c_itau", (0.006, 0.06, "log"), 0.02), ("ring", (0.1, 4.0, "log"), 1.0), ("soft", (0.0, 0.08), 0.0),
    ("bell_len", (0.0, 2.6), 0.0),
]
SPACE: dict[str, Any] = {k: spec for k, spec, _ in _ROWS}
DEFAULTS: dict[str, Any] = {k: d for k, _, d in _ROWS}

PARTIALS = {
    "celesta": ((1, 2, 3.98, 6.9, 10.1), (1, 0.06, 0.22, 0.05, 0.03)),
    "glass": ((1, 2.32, 4.25, 6.63, 9.38), (1, 0.45, 0.25, 0.12, 0.06)),
    "bar": ((1, 2.756, 5.404, 8.933), (1, 0.35, 0.14, 0.06)),
    "bowl": ((1, 2.71, 5.13, 8.27), (1, 0.6, 0.3, 0.12)),
    "tine": ((1, 2, 5.8, 6.27), (1, 0.12, 0.12, 0.25)),
}
MATS = {
    "film": ((1, 1, 1), (3.42, 0.3, 0.45)),
    "glass": ((1, 1, 1), (2.32, 0.3, 0.5), (3.4, 0.28, 0.35), (4.25, 0.12, 0.25), (6.63, 0.05, 0.15)),
    "bowl": ((1, 1, 1), (2.71, 0.4, 0.6), (3.38, 0.22, 0.4), (5.13, 0.14, 0.3)),
    "tine": ((1, 1, 1), (3.44, 0.12, 0.3), (5.8, 0.08, 0.25), (6.27, 0.1, 0.2)),
    "wood": ((1, 1, 1), (3.4, 0.18, 0.3), (3.98, 0.12, 0.22), (9.2, 0.03, 0.1)),
    "fm": ((1, 1, 1),),
}

# ------------------------------------------------------------------------------------------------ families (12.6)

LADDER = [1822.0 * SEMI ** k for k in range(5)]


def _over(base: dict, **kw: Any) -> dict:
    out = dict(DEFAULTS)
    out.update(base)
    out.update(kw)
    return out


_CLEAN = dict(style="ding", step_db=-30, hold=0.034, hold_tau=0.25, drop_db=-20, drop_ms=4, ring_tau=0.05,
              bright=0.25, p2=3.42, oct=0, detune=0.8, tick_db=-30, tick_f=9000, body_db=-22, body_ratio=0.5,
              body_in=0.03, body_tau=0.13, body_p3=0.25, body_p7=0.2, trem=0.35, trem_hz=24, drive=0,
              head_drive=0, air=-60, p3=0, verb=0.15, rt60=0.3, width=0.7, head_width=0.15, length=0.3, fade=0.05,
              hp=500)
_R5 = [
    ("ding_1822", dict(f0=LADDER[0], att=0.0015, length=0.26)),
    ("ding_1930", dict(f0=LADDER[1], att=0.012, step_db=-26, hold=0.04, body_db=-21, width=0.9, head_width=0.25,
                       length=0.27)),
    ("ding_2045", dict(f0=LADDER[2], att=0.003, hold=0.036, body_tau=0.2, body_db=-20, length=0.36, trem=0.45)),
    ("ding_2167", dict(f0=LADDER[3], att=0.011, step_db=-26, hold=0.04, bright=0.32, width=0.9, head_width=0.25,
                       length=0.3)),
    ("ding_2296", dict(f0=LADDER[4], att=0.004, hold=0.03, bright=0.2, body_ratio=1.0, body_p3=0.15, body_p7=0,
                       body_db=-19, body_tau=0.16, trem=0.25, length=0.26)),
]

_R3_KEYS = ("style f0 interval att step_db hold hold_tau drop_db drop_ms ring_tau bright p2 oct detune tick_db "
            "tick_f body_db body_ratio body_in body_tau body_p3 body_p7 trem trem_hz drive head_drive air air_tau "
            "p3 p3_tau verb rt60 width head_width length fade hp gap second_db").split()
_R3_COLS = [
    ("success", LADDER[0], 7, 0.002, -28, 0.026, 0.05, -12, 12, 0.07, 0.3, 3.4, 0.12, 0.6, -26, 9000, -20, 0.5,
     0.02, 0.14, 0.5, 0.3, 0.45, 24, 2, 0, -19, 0.05, 0.3, 0.02, 0.22, 0.3, 0.8, 0.25, 0.4, 0.05, 500, 0.06, -1.5),
    ("ding", LADDER[2], 7, 0.002, -30, 0.036, 0.1, -14, 8, 0.06, 0.2, 3.55, 0.15, 2.0, -24, 9000, -18, 0.5, 0.015,
     0.16, 0.7, 0.45, 0.5, 18, 3, 0, -18, 0.06, 0.25, 0.025, 0.22, 0.3, 1.0, 0.3, 0.34, 0.06, 450, 0.085, -2),
    ("ding", LADDER[0], 7, 0.0015, -22, 0.03, 0.1, -13, 6, 0.05, 0.35, 3.44, 0.05, 1.0, -22, 7000, -19, 0.5, 0.006,
     0.13, 0.45, 0.3, 0.55, 24, 3, 0, -17, 0.04, 0.35, 0.018, 0.12, 0.35, 0.8, 0.2, 0.28, 0.05, 600, 0.085, -2),
    ("ding", LADDER[1], 7, 0.002, -34, 0.022, 0.2, -12, 8, 0.045, 0.5, 3.46, 0.05, 0.5, -25, 9500, -20, 1.0, 0.02,
     0.12, 0.7, 0.2, 0.25, 24, 3, 0, -19, 0.05, 0.2, 0.015, 0.3, 0.3, 0.8, 0.2, 0.26, 0.06, 520, 0.085, -2),
    ("ding", LADDER[3], 7, 0.003, -20, 0.018, 0.05, -12, 5, 0.04, 0.7, 3.3, 0.08, 0.8, -26, 8000, -20, 1.5, 0.011,
     0.12, 0.6, 0.4, 0.6, 30, 3, 0, -18, 0.035, 0.25, 0.012, 0.16, 0.5, 0.8, 0.25, 0.24, 0.05, 450, 0.085, -2),
]

_BELL_KEYS = ("f0 partials decay slope hard early_db early_tau strike strike_tau beat length room hp tick_db drive "
              "width").split()
_BELL_COLS = [
    ("celesta C5", (523.25, "celesta", 0.5, 1.0, 0.7, 8, 0.05, -14, 0.015, 0.8, 2.3, 0.12, 180, -26, 1.0, 0.2)),
    ("glass A5", (880, "glass", 0.42, 1.1, 0.75, 7, 0.04, -12, 0.012, 1.6, 2.0, 0.15, 300, -24, 1.0, 0.2)),
    ("bar G5", (783.99, "bar", 0.38, 1.0, 0.85, 8, 0.035, -10, 0.01, 1.0, 1.8, 0.1, 300, -22, 1.5, 0.2)),
    ("small bell A4", (440, "bowl", 0.6, 0.9, 0.6, 10, 0.07, -16, 0.02, 1.2, 2.5, 0.15, 150, -30, 1.0, 0.25)),
    ("tine D5", (587.33, "tine", 0.36, 1.1, 0.65, 5, 0.04, -18, 0.012, 0.6, 1.8, 0.1, 150, -26, 1.0, 0.15)),
    ("celesta E6", (1318.5, "celesta", 0.34, 1.0, 0.8, 7, 0.03, -12, 0.01, 0.9, 1.7, 0.12, 300, -22, 1.0, 0.2)),
]
_R4_PAIRS = [(0.55, -20), (0.6, -18), (0.65, -17), (0.48, -22), (0.5, -24), (0.62, -18)]

FAMILY = {
    "pro_notify": [_over(_CLEAN, **kw) for _, kw in _R5],
    "notify": [_over({"style": "bell"}, **dict(zip(_BELL_KEYS, cols))) for _, cols in _BELL_COLS],
}
FAMILY_NAMES = {"pro_notify": [name for name, _ in _R5], "notify": [name for name, _ in _BELL_COLS]}
FAMILY_R5 = {"pro_notify": [dict(p) for p in FAMILY["pro_notify"]]}
FAMILY_NAMES_R5 = {"pro_notify": list(FAMILY_NAMES["pro_notify"])}
FAMILY_R3 = {"pro_notify": [_over({}, **dict(zip(_R3_KEYS, cols))) for cols in _R3_COLS]}
R4_EDITS = {
    "notify": [{"strike_max": 12, "slope": 1.1, "hard": h, "strike": s} for h, s in _R4_PAIRS],
    "pro_notify": [{"body_db": -13} for _ in range(5)],
}

_CHIME = dict(style="chime", drive=0, head_drive=0, hp=500, verb=0.14, rt60=0.3, width=0.7, fade=0.05, detune=0.8)
_R6 = [
    ("film_ding", dict(FAMILY["pro_notify"][0])),
    ("fm_bell", _over(_CHIME, f0=LADDER[1], mat="fm", att=0.005, c_fast=0.9, c_tf=0.011, c_ts=0.16, c_index=1.8,
                      c_itau=0.015, c_mallet=-40, c_hard=0.4, length=0.3, detune=0)),
    ("wood", _over(_CHIME, f0=LADDER[4], mat="wood", att=0.002, c_fast=0.95, c_tf=0.007, c_ts=0.09, c_mallet=-14,
                   c_hard=0.1, length=0.22, c_bend=40, detune=0, verb=0.1)),
    ("ding_dong", _over(_CHIME, f0=LADDER[3], mat="glass", att=0.0015, c_fast=0.9, c_tf=0.012, c_ts=0.12,
                        c_mallet=-26, c_hard=0.7, length=0.4, c_motif="up", interval=-4, gap=0.11, second_db=-2)),
    ("octave_body", _over(_CHIME, f0=LADDER[2], mat="film", att=0.0015, c_fast=0.9, c_tf=0.013, c_ts=0.1,
                          c_mallet=-30, c_hard=0.7, length=0.3, c_sub=0.1)),
]
FAMILY_R6 = {"pro_notify": [p for _, p in _R6]}
FAMILY_NAMES_R6 = {"pro_notify": [name for name, _ in _R6]}

_MID_KEYS = ("f0 mat att detune verb rt60 width length fade hp c_fast c_tf c_ts c_mallet c_hard c_bend c_sub "
             "c_index c_itau ring soft").split()
_MID_COLS = [
    ("tine", (830.61, "tine", 0.00316, 0.9, 0.255, 0.358, 0.313, 0.425, 0.0435, 135.6, 0.623, 0.0187, 0.0623, -59,
              1.0, 18.8, 0.45, 1.655, 0.0097, 0.82, 0.049)),
    ("wood", (880, "wood", 0.00814, 1.68, 0.35, 0.363, 0.229, 0.317, 0.0613, 254.7, 0.596, 0.0291, 0.077, -25.3,
              0.058, 22.9, 0.45, 4.38, 0.0064, 0.65, 0.047)),
]
FAMILY_MID = {"mid_ding": [_over({"style": "chime", "drive": 0, "c_motif": "single"}, **dict(zip(_MID_KEYS, c)))
                           for _, c in _MID_COLS]}
FAMILY_NAMES_MID = {"mid_ding": [name for name, _ in _MID_COLS]}


_TABLES = (FAMILY, FAMILY_NAMES, R4_EDITS)


def delivered(cat: str) -> list[tuple[str, dict]]:
    """The delivered members of a category as (name, full params): pro_notify round 5, notify with R4_EDITS."""
    family, names, edits = _TABLES
    if cat == "notify":
        return [(name, {**p, **edit}) for name, p, edit in zip(names["notify"], family["notify"], edits["notify"])]
    return list(zip(names[cat], (dict(p) for p in family[cat])))


# ------------------------------------------------------------------------------------------------ knobs (12.7, 12.8)

_DING_FINE = ["att", "step_db", "head_width", "hold_tau", "drop_db", "drop_ms", "ring_tau", "p2", "oct", "detune",
              "tick_db", "tick_f", "body_in", "body_tau", "body_p3", "body_p7", "drive", "verb", "rt60", "fade",
              "hp", "air", "air_tau", "p3", "p3_tau"]
FINE = {
    "ding": _DING_FINE,
    "success": _DING_FINE + ["gap", "second_db"],
    "bell": ["slope", "hard", "beat", "room", "tick_db", "tick_f", "drive", "fade", "att", "early_db", "early_tau",
             "strike", "strike_tau"],
    "chime": ["att", "c_fast", "c_tf", "c_ts", "c_mallet", "c_hard", "c_bend", "detune", "verb", "rt60", "fade",
              "hp"],
}
RING_KEYS = {
    "bell": ["decay", "early_tau", "strike_tau"],
    "ding": ["hold_tau", "ring_tau", "body_tau", "air_tau", "p3_tau"],
    "success": ["hold_tau", "ring_tau", "body_tau", "air_tau", "p3_tau"],
    "chime": ["c_tf", "c_ts", "c_itau"],
}


_SPACE_TABLES = (FINE, SPACE, DEFAULTS)


def family_space(params: dict, style: str | None = None) -> dict:
    """SPACE with every key pinned to the member's value except the FINE keys of its style."""
    fine, space, defaults = _SPACE_TABLES
    free = set(fine.get(style or params.get("style") or "ding", ()))
    return {k: spec if k in free else [params.get(k, defaults[k])] for k, spec in space.items()}


def ring_scaled(p: dict, ring: float | None = None) -> dict:
    """The decay knob: multiply the style's time constants by ring."""
    ring = ring if ring is not None else p.get("ring", 1.0)
    out = dict(p)
    if ring == 1:
        return out
    for k in RING_KEYS.get(p.get("style"), ()):
        if k in out:
            out[k] = out[k] * ring
    return out


# ------------------------------------------------------------------------------------------------ common pieces


def _unit_peak(x: np.ndarray) -> np.ndarray:
    return x / (np.max(np.abs(x)) + 1e-12)


def _rc_rise(x: np.ndarray, k: int) -> np.ndarray:
    k = min(len(x), k)
    if k > 1:
        w = 0.5 - 0.5 * np.cos(np.pi * np.arange(k) / (k - 1))
        x[:k] *= w if x.ndim == 1 else w[:, None]
    return x


def _cos2_fade(x: np.ndarray, k: int) -> np.ndarray:
    k = min(len(x), k)
    if k > 1:
        w = np.cos(0.5 * np.pi * np.arange(k) / (k - 1)) ** 2
        x[len(x) - k:] *= w if x.ndim == 1 else w[:, None]
    return x


def _partial(t: np.ndarray, f: float, amp: float, d: float, rng: np.random.Generator) -> np.ndarray:
    phi = rng.uniform(0, 2 * np.pi)
    y = np.sin(2 * np.pi * f * t + phi)
    if d > 0:
        y = (y + 0.6 * np.sin(2 * np.pi * (f + d * rng.uniform(0.6, 1.4)) * t + phi)) / 1.6
    return amp * y


def _tick(p: dict, n: int, rng: np.random.Generator) -> np.ndarray:
    k = min(n, n_of(0.004))
    w = bw(rng.standard_normal(k), "band", (p["tick_f"] / 1.6, min(1.6 * p["tick_f"], 20000)), 2)
    out = np.zeros(n)
    out[:k] = _unit_peak(w * np.exp(-t_of(k) / 0.0007)) * 10 ** (p["tick_db"] / 20)
    return out


def _finish(p: dict, x: np.ndarray, override: float = -1.0) -> np.ndarray:
    drive = override if override >= 0 else p["drive"]
    if drive > 0.2:
        x = fx.saturate(x, drive, mix=0.6)
    return bw(x, "high", p["hp"], 4)


def _end(p: dict, y: np.ndarray, n_len: int, fade: float | None = None) -> np.ndarray:
    if len(y) >= n_len:
        y = y[:n_len].copy()
    else:
        y = np.concatenate([y, np.zeros((n_len - len(y),) + y.shape[1:])])
    y = _cos2_fade(y, n_of(p["fade"] if fade is None else fade))
    return _rc_rise(y, n_of(0.0003))


def _soft(p: dict, y: np.ndarray) -> np.ndarray:
    if p["soft"] > 0:
        y = _rc_rise(y, n_of(p["soft"]))
    return y


def _plate(y: np.ndarray, send: np.ndarray, p: dict, rng: np.random.Generator) -> np.ndarray:
    """Add verb x the wet return of send; y is zero-padded to the wet length."""
    wet = fx.reverb(send, rng, rt60=p["rt60"], size=0.25, predelay=0.006, damp=0.45, wet=1.0, dry=0.0, stereo=True,
                    width=0.4 + p["width"])
    if len(wet) > len(y):
        y = np.concatenate([y, np.zeros((len(wet) - len(y), 2))])
    y = y.copy()
    y[:len(wet)] += p["verb"] * wet
    return y


# ------------------------------------------------------------------------------------------------ ding (12.10)


def _head_tone(p: dict, t: np.ndarray, rng) -> np.ndarray:
    f0 = p["f0"]
    head = _partial(t, f0, 1, p["detune"], rng)
    if f0 * p["p2"] < 0.45 * SR:
        head += p["bright"] * np.exp(-t / 0.09) * _partial(t, f0 * p["p2"], 1, 0, rng)
    if p["oct"] > 0:
        head += p["oct"] * _partial(t, 2 * f0, 1, 0, rng)
    if p["p3"] > 0:
        for q, a in ((2.756, 1.0), (5.404, 0.6)):
            if f0 * q < 0.4 * SR:
                head += p["p3"] * a * np.exp(-t / (p["p3_tau"] * 2.756 / q)) * _partial(t, f0 * q, 1, 1.5, rng)
    return head


def head_env(p: dict, t: np.ndarray) -> np.ndarray:
    """Onset step and rise, sag, the gated drop, and the ring after it."""
    s = 10 ** (p["step_db"] / 20)
    rise = s + (1 - s) * (0.5 - 0.5 * np.cos(np.pi * np.clip(t / p["att"], 0, 1)))
    sag = np.exp(-t / p["hold_tau"])
    dt = max(p["drop_ms"] / 1000, 0.001)
    fl = 10 ** (p["drop_db"] / 20)
    gate = 1 - (1 - fl) * (0.5 - 0.5 * np.cos(np.pi * np.clip((t - p["hold"]) / dt, 0, 1)))
    after = np.where(t > p["hold"] + dt, np.exp(-(t - p["hold"] - dt) / p["ring_tau"]), 1.0)
    e = rise * sag * gate * after
    e[0] = 0.0
    return e


def _air(p: dict, t: np.ndarray, rng) -> np.ndarray:
    lo = p["air_lo"] if p["air_lo"] > 0 else min(3000.0, 1.4 * p["f0"])
    w = bw(rng.standard_normal(len(t)), "band", (lo, 16000), 2 if lo < 6000 else 4)
    w = w / (np.sqrt(np.mean(w ** 2)) + 1e-12)
    ea = np.exp(-t / p["air_tau"])
    k = min(len(t), n_of(0.001))
    ea[:k] *= np.linspace(0, 1, k)
    return 0.7 * 10 ** (p["air"] / 20) * w * ea


def _body_tone(p: dict, t: np.ndarray, rng) -> np.ndarray:
    """The body partial at F = f0 x body_ratio, with partials at 3 x and 6.965 x, normalised by their sum."""
    F = p["f0"] * p["body_ratio"]
    body = _partial(t, F, 1, 0.8, rng)
    norm = 1.0
    for m, a in ((3, p["body_p3"]), (6.965, p["body_p7"])):
        if a > 0 and F * m < 0.45 * SR:
            body = body + a * _partial(t, F * m, 1, 0, rng)
            norm += a
    return body / norm


def _body(p: dict, t: np.ndarray, rng) -> np.ndarray:
    t0 = 0.6 * p["hold"]
    ein = np.clip((t - t0) / max(p["body_in"], 0.001), 0, 1)
    env = (0.5 - 0.5 * np.cos(np.pi * ein)) * np.exp(-np.maximum(t - t0, 0) / p["body_tau"])
    trem = 1 - p["trem"] * (0.5 - 0.5 * np.cos(2 * np.pi * p["trem_hz"] * t + rng.uniform(0, 1)))
    return _body_tone(p, t, rng) * env * trem * 10 ** (p["body_db"] / 20)


def ding(p: dict, n: int, rng: np.random.Generator) -> tuple[np.ndarray, np.ndarray]:
    """(head, body) of one ding, both mono, both starting at t = 0."""
    t = t_of(n)
    head = _head_tone(p, t, rng) * head_env(p, t)
    if p["air"] > -59:
        head = head + _air(p, t, rng)
    head = head + _tick(p, n, rng)
    return head, _body(p, t, rng)


def _delayed(x: np.ndarray, off: int, n: int) -> np.ndarray:
    out = np.zeros(n)
    if off < n:
        out[off:] = x[:n - off]
    return out


def _render_ding(p: dict, n: int, n_len: int, rng) -> np.ndarray:
    head, body = ding(p, n, rng)
    if p["style"] == "success":
        h2, b2 = ding({**p, "f0": p["f0"] * SEMI ** p["interval"]}, n, rng)
        g, off = 10 ** (p["second_db"] / 20), n_of(p["gap"])
        body = body * np.clip(1 - t_of(n) / (p["gap"] + 0.03), 0.25, 1) + g * _delayed(b2, off, n)
        head = head + g * _delayed(h2, off, n)
    head = _finish(p, head, p["head_drive"])
    body = _finish(p, body)
    S = fx.decorrelate(body, p["width"], 600)
    y = fx.decorrelate(head, p["head_width"], 1500) + S
    if p["verb"] > 0.005:
        y = _plate(y, 0.3 * np.column_stack([head, head]) + S, p, rng)
    return _end(p, _soft(p, y), n_len)


# ------------------------------------------------------------------------------------------------ bell (12.10)


def _bell_partial(p: dict, q: float, ge: float, t: np.ndarray, rng) -> np.ndarray:
    """One bell partial of ratio q: a slow beat, a decay that is faster for higher partials, the early drop."""
    tau = max(p["decay"] * q ** -p["slope"], 0.004)
    am = 1 - 0.35 * (0.5 - 0.5 * np.cos(2 * np.pi * p["beat"] * rng.uniform(0.5, 1.5) * t))
    env = np.exp(-t / tau)
    if ge > 0:
        env = env * (1 + ge * min(1.0, 0.5 + 0.25 * q) * np.exp(-t / p["early_tau"]))
    return env * am * np.sin(2 * np.pi * p["f0"] * q * t + rng.uniform(0, 6.3))


def _bell_partials(p: dict, t: np.ndarray, rng) -> np.ndarray:
    ratios, amps = PARTIALS[p["partials"]]
    ge = 10 ** (p["early_db"] / 20) - 1
    y = np.zeros(len(t))
    for q, a in zip(ratios, amps):
        if p["f0"] * q <= 0.45 * SR:
            y += a * q ** (1.5 * (p["hard"] - 0.5)) * _bell_partial(p, q, ge, t, rng)
    return y / (1 + 0.75 * ge) if ge > 0 else y


def _strike_modes(p: dict, t: np.ndarray, rng) -> np.ndarray:
    ys = np.zeros(len(t))
    for q in (6.1, 8.7, 11.9, 15.3, 19.6):
        f = p["f0"] * q * rng.uniform(0.98, 1.02)
        if f > 17000 or q > p["strike_max"]:
            continue
        tau = p["strike_tau"] * np.sqrt(6.1 / q)
        ys += rng.uniform(0.5, 1) * np.exp(-t / tau) * np.sin(2 * np.pi * f * t + rng.uniform(0, 6.3))
    return 10 ** (p["strike"] / 20) * ys


def bell(p: dict, n: int, rng: np.random.Generator) -> np.ndarray:
    """A modal struck tone with a free ring-out (mono)."""
    t = t_of(n)
    y = _bell_partials(p, t, rng)
    if p["strike"] > -59:
        y = y + _strike_modes(p, t, rng)
    y = _rc_rise(y, n_of(max(0.5 * p["att"], 0.0005)))
    k = min(n, n_of(0.006))
    m = bw(rng.standard_normal(k), "low", 2000 + 14000 * p["hard"], 2) * np.exp(-t_of(k) / 0.001)
    y[:k] += (0.05 + 0.25 * p["hard"]) * _unit_peak(m) * np.max(np.abs(y))
    return y + _tick(p, n, rng) * np.max(np.abs(y))


def _render_bell(p: dict, n: int, n_len: int, rng) -> np.ndarray:
    y = _finish(p, _soft(p, bell(p, n, rng)))
    y = fx.decorrelate(y, 0.5 * p["width"], 600)
    if p["room"] > 0:
        y = fx.reverb(y, rng, rt60=0.35 + 1.5 * p["room"], size=0.35, predelay=0.008, damp=0.55, wet=p["room"],
                      stereo=True, width=0.8)
    if p["early_db"] > 0 or p["strike"] > -59:
        return _end(p, y, n_len, max(p["fade"], 0.25))
    return _end(p, y, n_len, max(p["fade"], 0.4 * n_len / SR))


# ------------------------------------------------------------------------------------------------ chime (12.10)


def _two_rate(p: dict, tm: np.ndarray, dm: float = 1.0) -> np.ndarray:
    return p["c_fast"] * np.exp(-tm / (p["c_tf"] * dm)) + (1 - p["c_fast"]) * np.exp(-tm / (p["c_ts"] * dm))


def _phase(f: np.ndarray, rng) -> np.ndarray:
    return 2 * np.pi * np.cumsum(f) / SR + rng.uniform(0, 6.3)


def _mat_tone(p: dict, f0: float, t, tm, bend, rng) -> np.ndarray:
    if p["mat"] == "fm":
        idx = p["c_index"] * np.exp(-tm / p["c_itau"]) + 0.25
        mod = _phase(2.4 * f0 * bend, rng)
        car = _phase(f0 * bend, rng)
        return np.sin(car + idx * np.sin(mod)) * _two_rate(p, tm)
    y = np.zeros(len(t))
    for q, a, dm in MATS[p["mat"]]:
        f = f0 * q
        if f > 0.42 * SR:
            continue
        ph = _phase(f * bend, rng)
        tone = np.sin(ph)
        if p["detune"] > 0:
            tone = (tone + 0.5 * np.sin(ph + 2 * np.pi * p["detune"] * rng.uniform(0.6, 1.4) * t)) / 1.5
        y += a * _two_rate(p, tm, dm) * tone
    return y


def _chime_mallet(p: dict, n: int, rng) -> np.ndarray:
    """A short band of noise: felt (c_hard 0) is low and soft, metal (1) is high and short."""
    k, h = min(n, n_of(0.008)), p["c_hard"]
    w = bw(rng.standard_normal(k), "band", (1500 + 4000 * h, min(4000 + 12000 * h, 18000)), 2)
    tk = t_of(k)
    return 10 ** (p["c_mallet"] / 20) * _unit_peak(w * (1 - np.exp(-tk / 0.0003)) * np.exp(-tk / (0.0035 - 0.0027 * h)))


def _sub_body(p: dict, f0: float, t: np.ndarray, tm: np.ndarray, bend: np.ndarray, rng) -> np.ndarray:
    ramp = 0.5 - 0.5 * np.cos(np.pi * np.clip(t / 0.02, 0, 1))
    return p["c_sub"] * ramp * np.exp(-tm / p["c_ts"]) * np.sin(_phase(0.5 * f0 * bend, rng))


def strike(p: dict, n: int, f0: float, rng: np.random.Generator) -> np.ndarray:
    """One chime strike of the chosen material (mono)."""
    t = t_of(n)
    bend = 2.0 ** ((p["c_bend"] / 1200) * np.exp(-t / 0.01))
    a = max(p["att"], 0.0006)
    tm = np.maximum(t - a, 0)
    y = _mat_tone(p, f0, t, tm, bend, rng)
    if p["c_sub"] > 0:
        y = y + _sub_body(p, f0, t, tm, bend, rng)
    y = y * (0.5 - 0.5 * np.cos(np.pi * np.clip(t / a, 0, 1)))
    if p["c_mallet"] > -59:
        mallet = _chime_mallet(p, n, rng)
        y[:len(mallet)] += mallet
    return y


def chime(p: dict, n: int, rng: np.random.Generator) -> np.ndarray:
    y = strike(p, n, p["f0"], rng)
    if p["c_motif"] in ("up", "flam"):
        off = min(n, n_of(p["gap"]))
        f2 = p["f0"] * SEMI ** p["interval"] if p["c_motif"] == "up" else p["f0"]
        y2 = strike(p, n - off, f2, rng) if n - off > 0 else np.zeros(0)
        if p["c_motif"] == "up":
            y = y * np.clip(1 - t_of(n) / (p["gap"] + 0.04), 0.3, 1)
        y[off:] += 10 ** (p["second_db"] / 20) * y2
    return y


def _render_chime(p: dict, n: int, n_len: int, rng) -> np.ndarray:
    y = _finish(p, _soft(p, chime(p, n, rng)))
    y = fx.decorrelate(y, 0.5 * p["width"], 1200)
    if p["verb"] > 0.005:
        y = _plate(y, y, p, rng)
    return _end(p, y, n_len)


# ------------------------------------------------------------------------------------------------ render


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """One notify take, (n, 2) float64; n = samples of length (bell: bell_len, else at least 0.5 s)."""
    q = dict(DEFAULTS)
    q.update(p or {})
    if q["ring"] != 1:
        q = ring_scaled(q)
    p = q
    L = p["length"]
    if p["style"] == "bell":
        L = p["bell_len"] if p["bell_len"] > 0 else max(p["length"], 0.5)
    n_len = n_of(L)
    n = n_len + n_of(0.05)
    if p["style"] == "bell":
        return headroom(_render_bell(p, n, n_len, rng))
    if p["style"] == "chime":
        return headroom(_render_chime(p, n, n_len, rng))
    if p["style"] != "success":
        p = {**p, "style": "ding"}
    return headroom(_render_ding(p, n, n_len, rng))


# ------------------------------------------------------------------------------------------------ measure (12.11)

REGIONS = ((0, 150), (150, 500), (500, 1000), (1000, 4000), (4000, 12000), (12000, 24000))


def _hann(m: int) -> np.ndarray:
    return 0.5 - 0.5 * np.cos(2 * np.pi * np.arange(m) / max(1, m - 1))


def _env_db(m: np.ndarray, ms: float) -> np.ndarray:
    h = max(1, int(SR * ms / 1000))
    k = len(m) // h
    if k < 1:
        return np.array([-120.0])
    return 20 * np.log10(np.sqrt(np.mean(m[:k * h].reshape(k, h) ** 2, axis=1)) + 1e-12)


def _fall_ms(e2: np.ndarray, drop: float) -> float | None:
    p = int(np.argmax(e2))
    below = np.nonzero(e2[p:] < e2[p] - drop)[0]
    return float(below[0] * 2) if len(below) else None


def _spectrum_fields(m: np.ndarray) -> dict:
    P = np.abs(np.fft.rfft(m * _hann(len(m)))) ** 2
    f = np.fft.rfftfreq(len(m), 1 / SR)
    tot = P.sum() + 1e-20
    shares = [round(float(10 * np.log10(P[(f >= a) & (f < b)].sum() / tot + 1e-12)), 1) for a, b in REGIONS]
    cs = 1000 * 2.0 ** (np.arange(-10, 11) / 3)
    lv = [max(-60.0, float(10 * np.log10(P[(f >= c / 2 ** (1 / 6)) & (f < c * 2 ** (1 / 6))].sum() / tot + 1e-12)))
          for c in cs]
    tilt = round(float(np.polyfit(np.log2(cs), lv, 1)[0]), 2)
    return {"shares": shares, "tilt": tilt}


def _stereo_fields(x: np.ndarray) -> dict:
    L, R = x[:, 0], x[:, 1]
    corr = float(np.corrcoef(L, R)[0, 1]) if np.std(L) > 0 and np.std(R) > 0 else 1.0
    M, S = (L + R) / 2, (L - R) / 2
    side = 10 * np.log10(np.mean(S ** 2) / (np.mean(M ** 2) + 1e-20) + 1e-12)
    return {"corr": round(corr, 3), "side_mid_db": round(float(side), 1)}


def _frame_peaks(m: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Level (dB) and peak frequency of 512-sample Hann frames every 240 samples."""
    w = _hann(512)
    f = np.fft.rfftfreq(4096, 1 / SR)
    frames = [m[i:i + 512] * w for i in range(0, len(m) - 512, 240)]
    lv = np.array([10 * np.log10(np.mean(fr ** 2) + 1e-20) for fr in frames])
    pk = np.array([f[np.argmax(np.abs(np.fft.rfft(fr, 4096)))] for fr in frames])
    return lv, pk


def _ridge(m: np.ndarray) -> dict | None:
    lv, pk = _frame_peaks(m)
    if not len(lv):
        return None
    kept = pk[lv >= lv.max() - 30]
    return {"start_hz": round(float(kept[0])), "end_hz": round(float(kept[-1])), "max_hz": round(float(kept.max())),
            "min_hz": round(float(kept.min())),
            "sweep_oct": round(float(np.log2(max(kept[-1], 1) / max(kept[0], 1))), 2)}


def _time_fields(m: np.ndarray) -> tuple[dict, int]:
    """Attack, t20, t40 and peak time, and the first 1 ms block within 20 dB of the peak."""
    e1 = _env_db(m, 1)
    pk = int(np.argmax(e1))
    first = int(np.nonzero(e1 >= e1[pk] - 20)[0][0])
    e2 = np.convolve(_env_db(m, 2), np.ones(5) / 5, mode="same")
    return {"attack_ms": float(pk - first), "t20_ms": _fall_ms(e2, 20), "t40_ms": _fall_ms(e2, 40),
            "peak_t": round(pk / 1000, 4)}, first


def _head_hz(m: np.ndarray, first: int) -> dict:
    """The strongest frequency of the 40 ms from the onset; absent for a very short signal."""
    a = int(first * SR / 1000)
    seg = m[a:a + 1920]
    if len(seg) <= 256:
        return {}
    spec = np.abs(np.fft.rfft(seg * _hann(len(seg)), 8192))
    return {"head_hz": round(float(np.fft.rfftfreq(8192, 1 / SR)[np.argmax(spec)]))}


def measure(x: np.ndarray) -> dict:
    """Model-free cue measurements (12.11): attack, decay times, spectrum shares, tilt, stereo, head pitch, ridge."""
    x = np.asarray(x, float)
    x = x if x.ndim == 2 else np.column_stack([x, x])
    m = x.mean(axis=1)
    times, first = _time_fields(m)
    out: dict[str, Any] = {"length_s": round(len(m) / SR, 3), **times, **_spectrum_fields(m), **_stereo_fields(x),
                           **_head_hz(m, first)}
    ridge = _ridge(m)
    if ridge is not None:
        out["ridge"] = ridge
    return out


# ------------------------------------------------------------------------------------------------ penalties (12.12)


def outside(v: float | None, lo: float, hi: float, s: float) -> float:
    """Distance of v outside [lo, hi] in units of s; 1.0 when v is missing."""
    if v is None:
        return 1.0
    return max(lo - v, v - hi, 0.0) / s


TARGETS = {
    "pro_notify": [("length_s", 0.2, 0.42, 0.1), ("attack_ms", 0, 15, 10), ("t20_ms", 20, 45, 20),
                   ("t40_ms", 150, 340, 80), ("head_hz", 1700, 2400, 400), ("low_db", -200, -38, 8),
                   ("corr", 0.6, 0.97, 0.2)],
    "notify": [("length_s", 0.5, 2.6, 0.4), ("attack_ms", 0, 10, 10), ("sub_db", -200, -20, 8)],
}


def penalty_fields(x: np.ndarray) -> dict:
    """measure(x) plus low_db (energy under 500 Hz), sub_db and sweep_oct."""
    m = measure(x)
    sh = m["shares"]
    m["low_db"] = float(10 * np.log10(10 ** (sh[0] / 10) + 10 ** (sh[1] / 10) + 1e-12))
    m["sub_db"] = sh[0]
    if "ridge" in m:
        m["sweep_oct"] = m["ridge"]["sweep_oct"]
    return m


def pro_penalty(x: np.ndarray, cat: str) -> float:
    if cat not in TARGETS:
        return 0.0
    m = penalty_fields(x)
    return float(sum(min(2.0, outside(m.get(k), lo, hi, s)) for k, lo, hi, s in TARGETS[cat]))


def extra_for(cat: str, weight: float = 1.0) -> Callable[[np.ndarray, Any], float]:
    """A critic extra check: -weight x pro_penalty(x, cat)."""
    def extra(x: np.ndarray, row: Any = None) -> float:
        return -weight * pro_penalty(x, cat)
    return extra
