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
SPACE: dict[str, Any] = {kn: spec for kn, spec, _ in _ROWS}
DEFAULTS: dict[str, Any] = {kn: dv for kn, _, dv in _ROWS}

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

LADDER = [1822.0 * SEMI ** kn for kn in range(5)]


def _over(basev: dict, **kw: Any) -> dict:
    res = dict(DEFAULTS)
    res.update(basev)
    res.update(kw)
    return res


_CLEAN = {"style": "ding", "step_db": -30, "hold": 0.034, "hold_tau": 0.25, "drop_db": -20, "drop_ms": 4,
          "ring_tau": 0.05,
              "bright": 0.25, "p2": 3.42, "oct": 0, "detune": 0.8, "tick_db": -30, "tick_f": 9000, "body_db": -22,
              "body_ratio": 0.5,
              "body_in": 0.03, "body_tau": 0.13, "body_p3": 0.25, "body_p7": 0.2, "trem": 0.35, "trem_hz": 24,
              "drive": 0,
              "head_drive": 0, "air": -60, "p3": 0, "verb": 0.15, "rt60": 0.3, "width": 0.7, "head_width": 0.15,
              "length": 0.3, "fade": 0.05,
              "hp": 500}
_R5 = [
    ("ding_1822", {"f0": LADDER[0], "att": 0.0015, "length": 0.26}),
    ("ding_1930", {"f0": LADDER[1], "att": 0.012, "step_db": -26, "hold": 0.04, "body_db": -21, "width": 0.9,
                   "head_width": 0.25,
                       "length": 0.27}),
    ("ding_2045", {"f0": LADDER[2], "att": 0.003, "hold": 0.036, "body_tau": 0.2, "body_db": -20, "length": 0.36,
                   "trem": 0.45}),
    ("ding_2167", {"f0": LADDER[3], "att": 0.011, "step_db": -26, "hold": 0.04, "bright": 0.32, "width": 0.9,
                   "head_width": 0.25,
                       "length": 0.3}),
    ("ding_2296", {"f0": LADDER[4], "att": 0.004, "hold": 0.03, "bright": 0.2, "body_ratio": 1.0, "body_p3": 0.15,
                   "body_p7": 0,
                       "body_db": -19, "body_tau": 0.16, "trem": 0.25, "length": 0.26}),
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
FAMILY_NAMES = {"pro_notify": [label for label, _ in _R5], "notify": [label for label, _ in _BELL_COLS]}
FAMILY_R5 = {"pro_notify": [dict(cfg) for cfg in FAMILY["pro_notify"]]}
FAMILY_NAMES_R5 = {"pro_notify": list(FAMILY_NAMES["pro_notify"])}
FAMILY_R3 = {"pro_notify": [_over({}, **dict(zip(_R3_KEYS, cols))) for cols in _R3_COLS]}
R4_EDITS = {
    "notify": [{"strike_max": 12, "slope": 1.1, "hard": hv, "strike": sv} for hv, sv in _R4_PAIRS],
    "pro_notify": [{"body_db": -13} for _ in range(5)],
}

_CHIME = {"style": "chime", "drive": 0, "head_drive": 0, "hp": 500, "verb": 0.14, "rt60": 0.3, "width": 0.7,
          "fade": 0.05, "detune": 0.8}
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
FAMILY_R6 = {"pro_notify": [cfg for _, cfg in _R6]}
FAMILY_NAMES_R6 = {"pro_notify": [label for label, _ in _R6]}

_MID_KEYS = ("f0 mat att detune verb rt60 width length fade hp c_fast c_tf c_ts c_mallet c_hard c_bend c_sub "
             "c_index c_itau ring soft").split()
_MID_COLS = [
    ("tine", (830.61, "tine", 0.00316, 0.9, 0.255, 0.358, 0.313, 0.425, 0.0435, 135.6, 0.623, 0.0187, 0.0623, -59,
              1.0, 18.8, 0.45, 1.655, 0.0097, 0.82, 0.049)),
    ("wood", (880, "wood", 0.00814, 1.68, 0.35, 0.363, 0.229, 0.317, 0.0613, 254.7, 0.596, 0.0291, 0.077, -25.3,
              0.058, 22.9, 0.45, 4.38, 0.0064, 0.65, 0.047)),
]
FAMILY_MID = {"mid_ding": [_over({"style": "chime", "drive": 0, "c_motif": "single"}, **dict(zip(_MID_KEYS, cv)))
                           for _, cv in _MID_COLS]}
FAMILY_NAMES_MID = {"mid_ding": [label for label, _ in _MID_COLS]}


_TABLES = (FAMILY, FAMILY_NAMES, R4_EDITS)


def delivered(cat: str) -> list[tuple[str, dict]]:
    """The delivered members of a category as (name, full params): pro_notify round 5, notify with R4_EDITS."""
    family, names, edits = _TABLES
    if cat == "notify":
        return [(label, {**cfg, **edit}) for label, cfg, edit in zip(names["notify"], family["notify"],
                                                                     edits["notify"])]
    return list(zip(names[cat], (dict(cfg) for cfg in family[cat])))


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


def family_space(pset: dict, style: str | None = None) -> dict:
    """SPACE with every key pinned to the member's value except the FINE keys of its style."""
    fine, space, dflt = _SPACE_TABLES
    free = set(fine.get(style or pset.get("style") or "ding", ()))
    return {kn: spec if kn in free else [pset.get(kn, dflt[kn])] for kn, spec in space.items()}


def ring_scaled(cfg: dict, ring: float | None = None) -> dict:
    """The decay knob: multiply the style's time constants by ring."""
    ring = ring if ring is not None else cfg.get("ring", 1.0)
    res = dict(cfg)
    if ring == 1:
        return res
    for kn in RING_KEYS.get(cfg.get("style"), ()):
        if kn in res:
            res[kn] = res[kn] * ring
    return res


# ------------------------------------------------------------------------------------------------ common pieces


def _unit_peak(wav: np.ndarray) -> np.ndarray:
    return wav / (np.max(np.abs(wav)) + 1e-12)


def _rc_rise(wav: np.ndarray, kn: int) -> np.ndarray:
    kn = min(len(wav), kn)
    if kn > 1:
        wv = 0.5 - 0.5 * np.cos(np.pi * np.arange(kn) / (kn - 1))
        wav[:kn] *= wv if wav.ndim == 1 else wv[:, None]
    return wav


def _cos2_fade(wav: np.ndarray, kn: int) -> np.ndarray:
    kn = min(len(wav), kn)
    if kn > 1:
        wv = np.cos(0.5 * np.pi * np.arange(kn) / (kn - 1)) ** 2
        wav[len(wav) - kn:] *= wv if wav.ndim == 1 else wv[:, None]
    return wav


def _partial(tax: np.ndarray, freq: float, amp: float, dv: float, gen: np.random.Generator) -> np.ndarray:
    phs0 = gen.uniform(0, 2 * np.pi)
    sig = np.sin(2 * np.pi * freq * tax + phs0)
    if dv > 0:
        sig = (sig + 0.6 * np.sin(2 * np.pi * (freq + dv * gen.uniform(0.6, 1.4)) * tax + phs0)) / 1.6
    return amp * sig


def _tick(cfg: dict, ns: int, gen: np.random.Generator) -> np.ndarray:
    kn = min(ns, n_of(0.004))
    wv = bw(gen.standard_normal(kn), "band", (cfg["tick_f"] / 1.6, min(1.6 * cfg["tick_f"], 20000)), 2)
    res = np.zeros(ns)
    res[:kn] = _unit_peak(wv * np.exp(-t_of(kn) / 0.0007)) * 10 ** (cfg["tick_db"] / 20)
    return res


def _finish(cfg: dict, wav: np.ndarray, override: float = -1.0) -> np.ndarray:
    drive = override if override >= 0 else cfg["drive"]
    if drive > 0.2:
        wav = fx.saturate(wav, drive, mix=0.6)
    return bw(wav, "high", cfg["hp"], 4)


def _end(cfg: dict, sig: np.ndarray, n_len: int, fade: float | None = None) -> np.ndarray:
    if len(sig) >= n_len:
        sig = sig[:n_len].copy()
    else:
        sig = np.concatenate([sig, np.zeros((n_len - len(sig),) + sig.shape[1:])])
    sig = _cos2_fade(sig, n_of(cfg["fade"] if fade is None else fade))
    return _rc_rise(sig, n_of(0.0003))


def _soft(cfg: dict, sig: np.ndarray) -> np.ndarray:
    if cfg["soft"] > 0:
        sig = _rc_rise(sig, n_of(cfg["soft"]))
    return sig


def _plate(sig: np.ndarray, feedv: np.ndarray, cfg: dict, gen: np.random.Generator) -> np.ndarray:
    """Add verb x the wet return of send; y is zero-padded to the wet length."""
    wetv = fx.reverb(feedv, gen, rt60=cfg["rt60"], size=0.25, predelay=0.006, damp=0.45, wet=1.0, dry=0.0, stereo=True,
                    width=0.4 + cfg["width"])
    if len(wetv) > len(sig):
        sig = np.concatenate([sig, np.zeros((len(wetv) - len(sig), 2))])
    sig = sig.copy()
    sig[:len(wetv)] += cfg["verb"] * wetv
    return sig


# ------------------------------------------------------------------------------------------------ ding (12.10)


def _head_tone(cfg: dict, tax: np.ndarray, gen) -> np.ndarray:
    fa = cfg["f0"]
    head = _partial(tax, fa, 1, cfg["detune"], gen)
    if fa * cfg["p2"] < 0.45 * SR:
        head += cfg["bright"] * np.exp(-tax / 0.09) * _partial(tax, fa * cfg["p2"], 1, 0, gen)
    if cfg["oct"] > 0:
        head += cfg["oct"] * _partial(tax, 2 * fa, 1, 0, gen)
    if cfg["p3"] > 0:
        for qv, a in ((2.756, 1.0), (5.404, 0.6)):
            if fa * qv < 0.4 * SR:
                head += cfg["p3"] * a * np.exp(-tax / (cfg["p3_tau"] * 2.756 / qv)) * _partial(tax, fa * qv, 1, 1.5,
                                                                                               gen)
    return head


def head_env(cfg: dict, tax: np.ndarray) -> np.ndarray:
    """Onset step and rise, sag, the gated drop, and the ring after it."""
    sv = 10 ** (cfg["step_db"] / 20)
    rise = sv + (1 - sv) * (0.5 - 0.5 * np.cos(np.pi * np.clip(tax / cfg["att"], 0, 1)))
    sag = np.exp(-tax / cfg["hold_tau"])
    dt = max(cfg["drop_ms"] / 1000, 0.001)
    fl = 10 ** (cfg["drop_db"] / 20)
    gate = 1 - (1 - fl) * (0.5 - 0.5 * np.cos(np.pi * np.clip((tax - cfg["hold"]) / dt, 0, 1)))
    after = np.where(tax > cfg["hold"] + dt, np.exp(-(tax - cfg["hold"] - dt) / cfg["ring_tau"]), 1.0)
    e = rise * sag * gate * after
    e[0] = 0.0
    return e


def _air(cfg: dict, tax: np.ndarray, gen) -> np.ndarray:
    lo = cfg["air_lo"] if cfg["air_lo"] > 0 else min(3000.0, 1.4 * cfg["f0"])
    wv = bw(gen.standard_normal(len(tax)), "band", (lo, 16000), 2 if lo < 6000 else 4)
    wv = wv / (np.sqrt(np.mean(wv ** 2)) + 1e-12)
    ea = np.exp(-tax / cfg["air_tau"])
    kn = min(len(tax), n_of(0.001))
    ea[:kn] *= np.linspace(0, 1, kn)
    return 0.7 * 10 ** (cfg["air"] / 20) * wv * ea


def _body_tone(cfg: dict, tax: np.ndarray, gen) -> np.ndarray:
    """The body partial at F = f0 x body_ratio, with partials at 3 x and 6.965 x, normalised by their sum."""
    F = cfg["f0"] * cfg["body_ratio"]
    bodyv = _partial(tax, F, 1, 0.8, gen)
    norm = 1.0
    for mv, a in ((3, cfg["body_p3"]), (6.965, cfg["body_p7"])):
        if a > 0 and F * mv < 0.45 * SR:
            bodyv = bodyv + a * _partial(tax, F * mv, 1, 0, gen)
            norm += a
    return bodyv / norm


def _body(cfg: dict, tax: np.ndarray, gen) -> np.ndarray:
    onset = 0.6 * cfg["hold"]
    ein = np.clip((tax - onset) / max(cfg["body_in"], 0.001), 0, 1)
    env = (0.5 - 0.5 * np.cos(np.pi * ein)) * np.exp(-np.maximum(tax - onset, 0) / cfg["body_tau"])
    trem = 1 - cfg["trem"] * (0.5 - 0.5 * np.cos(2 * np.pi * cfg["trem_hz"] * tax + gen.uniform(0, 1)))
    return _body_tone(cfg, tax, gen) * env * trem * 10 ** (cfg["body_db"] / 20)


def ding(cfg: dict, ns: int, gen: np.random.Generator) -> tuple[np.ndarray, np.ndarray]:
    """(head, body) of one ding, both mono, both starting at t = 0."""
    tax = t_of(ns)
    head = _head_tone(cfg, tax, gen) * head_env(cfg, tax)
    if cfg["air"] > -59:
        head = head + _air(cfg, tax, gen)
    head = head + _tick(cfg, ns, gen)
    return head, _body(cfg, tax, gen)


def _delayed(wav: np.ndarray, off: int, ns: int) -> np.ndarray:
    res = np.zeros(ns)
    if off < ns:
        res[off:] = wav[:ns - off]
    return res


def _render_ding(cfg: dict, ns: int, n_len: int, gen) -> np.ndarray:
    head, bodyv = ding(cfg, ns, gen)
    if cfg["style"] == "success":
        h2, b2 = ding({**cfg, "f0": cfg["f0"] * SEMI ** cfg["interval"]}, ns, gen)
        g, off = 10 ** (cfg["second_db"] / 20), n_of(cfg["gap"])
        bodyv = bodyv * np.clip(1 - t_of(ns) / (cfg["gap"] + 0.03), 0.25, 1) + g * _delayed(b2, off, ns)
        head = head + g * _delayed(h2, off, ns)
    head = _finish(cfg, head, cfg["head_drive"])
    bodyv = _finish(cfg, bodyv)
    S = fx.decorrelate(bodyv, cfg["width"], 600)
    sig = fx.decorrelate(head, cfg["head_width"], 1500) + S
    if cfg["verb"] > 0.005:
        sig = _plate(sig, 0.3 * np.column_stack([head, head]) + S, cfg, gen)
    return _end(cfg, _soft(cfg, sig), n_len)


# ------------------------------------------------------------------------------------------------ bell (12.10)


def _bell_partial(cfg: dict, qv: float, ge: float, tax: np.ndarray, gen) -> np.ndarray:
    """One bell partial of ratio q: a slow beat, a decay that is faster for higher partials, the early drop."""
    tc = max(cfg["decay"] * qv ** -cfg["slope"], 0.004)
    am = 1 - 0.35 * (0.5 - 0.5 * np.cos(2 * np.pi * cfg["beat"] * gen.uniform(0.5, 1.5) * tax))
    env = np.exp(-tax / tc)
    if ge > 0:
        env = env * (1 + ge * min(1.0, 0.5 + 0.25 * qv) * np.exp(-tax / cfg["early_tau"]))
    return env * am * np.sin(2 * np.pi * cfg["f0"] * qv * tax + gen.uniform(0, 6.3))


def _bell_partials(cfg: dict, tax: np.ndarray, gen) -> np.ndarray:
    ratios, amps = PARTIALS[cfg["partials"]]
    ge = 10 ** (cfg["early_db"] / 20) - 1
    sig = np.zeros(len(tax))
    for qv, a in zip(ratios, amps):
        if cfg["f0"] * qv <= 0.45 * SR:
            sig += a * qv ** (1.5 * (cfg["hard"] - 0.5)) * _bell_partial(cfg, qv, ge, tax, gen)
    return sig / (1 + 0.75 * ge) if ge > 0 else sig


def _strike_modes(cfg: dict, tax: np.ndarray, gen) -> np.ndarray:
    ys = np.zeros(len(tax))
    for qv in (6.1, 8.7, 11.9, 15.3, 19.6):
        freq = cfg["f0"] * qv * gen.uniform(0.98, 1.02)
        if freq > 17000 or qv > cfg["strike_max"]:
            continue
        tc = cfg["strike_tau"] * np.sqrt(6.1 / qv)
        ys += gen.uniform(0.5, 1) * np.exp(-tax / tc) * np.sin(2 * np.pi * freq * tax + gen.uniform(0, 6.3))
    return 10 ** (cfg["strike"] / 20) * ys


def bell(cfg: dict, ns: int, gen: np.random.Generator) -> np.ndarray:
    """A modal struck tone with a free ring-out (mono)."""
    tax = t_of(ns)
    sig = _bell_partials(cfg, tax, gen)
    if cfg["strike"] > -59:
        sig = sig + _strike_modes(cfg, tax, gen)
    sig = _rc_rise(sig, n_of(max(0.5 * cfg["att"], 0.0005)))
    kn = min(ns, n_of(0.006))
    mv = bw(gen.standard_normal(kn), "low", 2000 + 14000 * cfg["hard"], 2) * np.exp(-t_of(kn) / 0.001)
    sig[:kn] += (0.05 + 0.25 * cfg["hard"]) * _unit_peak(mv) * np.max(np.abs(sig))
    return sig + _tick(cfg, ns, gen) * np.max(np.abs(sig))


def _render_bell(cfg: dict, ns: int, n_len: int, gen) -> np.ndarray:
    sig = _finish(cfg, _soft(cfg, bell(cfg, ns, gen)))
    sig = fx.decorrelate(sig, 0.5 * cfg["width"], 600)
    if cfg["room"] > 0:
        sig = fx.reverb(sig, gen, rt60=0.35 + 1.5 * cfg["room"], size=0.35, predelay=0.008, damp=0.55, wet=cfg["room"],
                      stereo=True, width=0.8)
    if cfg["early_db"] > 0 or cfg["strike"] > -59:
        return _end(cfg, sig, n_len, max(cfg["fade"], 0.25))
    return _end(cfg, sig, n_len, max(cfg["fade"], 0.4 * n_len / SR))


# ------------------------------------------------------------------------------------------------ chime (12.10)


def _two_rate(cfg: dict, tm: np.ndarray, dm: float = 1.0) -> np.ndarray:
    return cfg["c_fast"] * np.exp(-tm / (cfg["c_tf"] * dm)) + (1 - cfg["c_fast"]) * np.exp(-tm / (cfg["c_ts"] * dm))


def _phase(freq: np.ndarray, gen) -> np.ndarray:
    return 2 * np.pi * np.cumsum(freq) / SR + gen.uniform(0, 6.3)


def _mat_tone(cfg: dict, fa: float, tax, tm, bend, gen) -> np.ndarray:
    if cfg["mat"] == "fm":
        idx = cfg["c_index"] * np.exp(-tm / cfg["c_itau"]) + 0.25
        mod = _phase(2.4 * fa * bend, gen)
        car = _phase(fa * bend, gen)
        return np.sin(car + idx * np.sin(mod)) * _two_rate(cfg, tm)
    sig = np.zeros(len(tax))
    for qv, a, dm in MATS[cfg["mat"]]:
        freq = fa * qv
        if freq > 0.42 * SR:
            continue
        phase = _phase(freq * bend, gen)
        tone = np.sin(phase)
        if cfg["detune"] > 0:
            tone = (tone + 0.5 * np.sin(phase + 2 * np.pi * cfg["detune"] * gen.uniform(0.6, 1.4) * tax)) / 1.5
        sig += a * _two_rate(cfg, tm, dm) * tone
    return sig


def _chime_mallet(cfg: dict, ns: int, gen) -> np.ndarray:
    """A short band of noise: felt (c_hard 0) is low and soft, metal (1) is high and short."""
    kn, hv = min(ns, n_of(0.008)), cfg["c_hard"]
    wv = bw(gen.standard_normal(kn), "band", (1500 + 4000 * hv, min(4000 + 12000 * hv, 18000)), 2)
    tkv = t_of(kn)
    return 10 ** (cfg["c_mallet"] / 20) * _unit_peak(
        wv * (1 - np.exp(-tkv / 0.0003)) * np.exp(-tkv / (0.0035 - 0.0027 * hv)))


def _sub_body(cfg: dict, fa: float, tax: np.ndarray, tm: np.ndarray, bend: np.ndarray, gen) -> np.ndarray:
    ramp = 0.5 - 0.5 * np.cos(np.pi * np.clip(tax / 0.02, 0, 1))
    return cfg["c_sub"] * ramp * np.exp(-tm / cfg["c_ts"]) * np.sin(_phase(0.5 * fa * bend, gen))


def strike(cfg: dict, ns: int, fa: float, gen: np.random.Generator) -> np.ndarray:
    """One chime strike of the chosen material (mono)."""
    tax = t_of(ns)
    bend = 2.0 ** ((cfg["c_bend"] / 1200) * np.exp(-tax / 0.01))
    a = max(cfg["att"], 0.0006)
    tm = np.maximum(tax - a, 0)
    sig = _mat_tone(cfg, fa, tax, tm, bend, gen)
    if cfg["c_sub"] > 0:
        sig = sig + _sub_body(cfg, fa, tax, tm, bend, gen)
    sig = sig * (0.5 - 0.5 * np.cos(np.pi * np.clip(tax / a, 0, 1)))
    if cfg["c_mallet"] > -59:
        mallet = _chime_mallet(cfg, ns, gen)
        sig[:len(mallet)] += mallet
    return sig


def chime(cfg: dict, ns: int, gen: np.random.Generator) -> np.ndarray:
    sig = strike(cfg, ns, cfg["f0"], gen)
    if cfg["c_motif"] in ("up", "flam"):
        off = min(ns, n_of(cfg["gap"]))
        f2 = cfg["f0"] * SEMI ** cfg["interval"] if cfg["c_motif"] == "up" else cfg["f0"]
        y2 = strike(cfg, ns - off, f2, gen) if ns - off > 0 else np.zeros(0)
        if cfg["c_motif"] == "up":
            sig = sig * np.clip(1 - t_of(ns) / (cfg["gap"] + 0.04), 0.3, 1)
        sig[off:] += 10 ** (cfg["second_db"] / 20) * y2
    return sig


def _render_chime(cfg: dict, ns: int, n_len: int, gen) -> np.ndarray:
    sig = _finish(cfg, _soft(cfg, chime(cfg, ns, gen)))
    sig = fx.decorrelate(sig, 0.5 * cfg["width"], 1200)
    if cfg["verb"] > 0.005:
        sig = _plate(sig, sig, cfg, gen)
    return _end(cfg, sig, n_len)


# ------------------------------------------------------------------------------------------------ render


def render(cfg: dict, gen: np.random.Generator) -> np.ndarray:
    """One notify take, (n, 2) float64; n = samples of length (bell: bell_len, else at least 0.5 s)."""
    qv = dict(DEFAULTS)
    qv.update(cfg or {})
    if qv["ring"] != 1:
        qv = ring_scaled(qv)
    cfg = qv
    span = cfg["length"]
    if cfg["style"] == "bell":
        span = cfg["bell_len"] if cfg["bell_len"] > 0 else max(cfg["length"], 0.5)
    n_len = n_of(span)
    ns = n_len + n_of(0.05)
    if cfg["style"] == "bell":
        return headroom(_render_bell(cfg, ns, n_len, gen))
    if cfg["style"] == "chime":
        return headroom(_render_chime(cfg, ns, n_len, gen))
    if cfg["style"] != "success":
        cfg = {**cfg, "style": "ding"}
    return headroom(_render_ding(cfg, ns, n_len, gen))


# ------------------------------------------------------------------------------------------------ measure (12.11)

REGIONS = ((0, 150), (150, 500), (500, 1000), (1000, 4000), (4000, 12000), (12000, 24000))


def _hann(mv: int) -> np.ndarray:
    return 0.5 - 0.5 * np.cos(2 * np.pi * np.arange(mv) / max(1, mv - 1))


def _env_db(mv: np.ndarray, msec: float) -> np.ndarray:
    hv = max(1, int(SR * msec / 1000))
    kn = len(mv) // hv
    if kn < 1:
        return np.array([-120.0])
    return 20 * np.log10(np.sqrt(np.mean(mv[:kn * hv].reshape(kn, hv) ** 2, axis=1)) + 1e-12)


def _fall_ms(e2: np.ndarray, drop: float) -> float | None:
    cfg = int(np.argmax(e2))
    below = np.nonzero(e2[cfg:] < e2[cfg] - drop)[0]
    return float(below[0] * 2) if len(below) else None


def _spectrum_fields(mv: np.ndarray) -> dict:
    P = np.abs(np.fft.rfft(mv * _hann(len(mv)))) ** 2
    freq = np.fft.rfftfreq(len(mv), 1 / SR)
    tot = P.sum() + 1e-20
    shares = [round(float(10 * np.log10(P[(freq >= a) & (freq < bv)].sum() / tot + 1e-12)), 1) for a, bv in REGIONS]
    cs = 1000 * 2.0 ** (np.arange(-10, 11) / 3)
    lvl0 = [max(-60.0,
                float(10 * np.log10(P[(freq >= cv / 2 ** (1 / 6)) & (freq < cv * 2 ** (1 / 6))].sum() / tot + 1e-12)))
          for cv in cs]
    tilt = round(float(np.polyfit(np.log2(cs), lvl0, 1)[0]), 2)
    return {"shares": shares, "tilt": tilt}


def _stereo_fields(wav: np.ndarray) -> dict:
    span, R = wav[:, 0], wav[:, 1]
    corr = float(np.corrcoef(span, R)[0, 1]) if np.std(span) > 0 and np.std(R) > 0 else 1.0
    M, S = (span + R) / 2, (span - R) / 2
    sidev = 10 * np.log10(np.mean(S ** 2) / (np.mean(M ** 2) + 1e-20) + 1e-12)
    return {"corr": round(corr, 3), "side_mid_db": round(float(sidev), 1)}


def _frame_peaks(mv: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Level (dB) and peak frequency of 512-sample Hann frames every 240 samples."""
    wv = _hann(512)
    freq = np.fft.rfftfreq(4096, 1 / SR)
    frames = [mv[ix:ix + 512] * wv for ix in range(0, len(mv) - 512, 240)]
    lvl0 = np.array([10 * np.log10(np.mean(fcurve ** 2) + 1e-20) for fcurve in frames])
    peak = np.array([freq[np.argmax(np.abs(np.fft.rfft(fcurve, 4096)))] for fcurve in frames])
    return lvl0, peak


def _ridge(mv: np.ndarray) -> dict | None:
    lvl0, peak = _frame_peaks(mv)
    if not len(lvl0):
        return None
    kept = peak[lvl0 >= lvl0.max() - 30]
    return {"start_hz": round(float(kept[0])), "end_hz": round(float(kept[-1])), "max_hz": round(float(kept.max())),
            "min_hz": round(float(kept.min())),
            "sweep_oct": round(float(np.log2(max(kept[-1], 1) / max(kept[0], 1))), 2)}


def _time_fields(mv: np.ndarray) -> tuple[dict, int]:
    """Attack, t20, t40 and peak time, and the first 1 ms block within 20 dB of the peak."""
    e1 = _env_db(mv, 1)
    peak = int(np.argmax(e1))
    first = int(np.nonzero(e1 >= e1[peak] - 20)[0][0])
    e2 = np.convolve(_env_db(mv, 2), np.ones(5) / 5, mode="same")
    return {"attack_ms": float(peak - first), "t20_ms": _fall_ms(e2, 20), "t40_ms": _fall_ms(e2, 40),
            "peak_t": round(peak / 1000, 4)}, first


def _head_hz(mv: np.ndarray, first: int) -> dict:
    """The strongest frequency of the 40 ms from the onset; absent for a very short signal."""
    a = int(first * SR / 1000)
    seg = mv[a:a + 1920]
    if len(seg) <= 256:
        return {}
    spec = np.abs(np.fft.rfft(seg * _hann(len(seg)), 8192))
    return {"head_hz": round(float(np.fft.rfftfreq(8192, 1 / SR)[np.argmax(spec)]))}


def measure(wav: np.ndarray) -> dict:
    """Model-free cue measurements (12.11): attack, decay times, spectrum shares, tilt, stereo, head pitch, ridge."""
    wav = np.asarray(wav, float)
    wav = wav if wav.ndim == 2 else np.column_stack([wav, wav])
    mv = wav.mean(axis=1)
    times, first = _time_fields(mv)
    res: dict[str, Any] = {"length_s": round(len(mv) / SR, 3), **times, **_spectrum_fields(mv), **_stereo_fields(wav),
                           **_head_hz(mv, first)}
    ridge = _ridge(mv)
    if ridge is not None:
        res["ridge"] = ridge
    return res


# ------------------------------------------------------------------------------------------------ penalties (12.12)


def outside(v: float | None, lo: float, hiv: float, sv: float) -> float:
    """Distance of v outside [lo, hi] in units of s; 1.0 when v is missing."""
    if v is None:
        return 1.0
    return max(lo - v, v - hiv, 0.0) / sv


TARGETS = {
    "pro_notify": [("length_s", 0.2, 0.42, 0.1), ("attack_ms", 0, 15, 10), ("t20_ms", 20, 45, 20),
                   ("t40_ms", 150, 340, 80), ("head_hz", 1700, 2400, 400), ("low_db", -200, -38, 8),
                   ("corr", 0.6, 0.97, 0.2)],
    "notify": [("length_s", 0.5, 2.6, 0.4), ("attack_ms", 0, 10, 10), ("sub_db", -200, -20, 8)],
}


def penalty_fields(wav: np.ndarray) -> dict:
    """measure(x) plus low_db (energy under 500 Hz), sub_db and sweep_oct."""
    mv = measure(wav)
    shv = mv["shares"]
    mv["low_db"] = float(10 * np.log10(10 ** (shv[0] / 10) + 10 ** (shv[1] / 10) + 1e-12))
    mv["sub_db"] = shv[0]
    if "ridge" in mv:
        mv["sweep_oct"] = mv["ridge"]["sweep_oct"]
    return mv


def pro_penalty(wav: np.ndarray, cat: str) -> float:
    if cat not in TARGETS:
        return 0.0
    mv = penalty_fields(wav)
    return float(sum(min(2.0, outside(mv.get(kn), lo, hiv, sv)) for kn, lo, hiv, sv in TARGETS[cat]))


def extra_for(cat: str, weight: float = 1.0) -> Callable[[np.ndarray, Any], float]:
    """A critic extra check: -weight x pro_penalty(x, cat)."""
    def extra(wav: np.ndarray, row: Any = None) -> float:
        return -weight * pro_penalty(wav, cat)
    return extra
