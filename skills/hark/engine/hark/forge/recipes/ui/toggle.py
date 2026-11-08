"""Recipe toggle: a UI switch flipped with 2 quick contacts."""

from __future__ import annotations

import math

import numpy as np

from hark.forge.engine import dsp, fx
from hark.forge.recipes.ui import _layers as L

TARGET = "a small switch flipped on with two quick clicks"
TARGETS = {"toggle": TARGET}
ALTS = ["a gun being cocked", "a pen cap snapping shut", "a mechanical keyboard key press", "a coin dropped"]
REFSET = "toggle"

SPACE = {
    "gap": (0.025, 0.16, "log"),
    "dir": (0.7, 1.45, "log"),
    "a_lvl": (0.12, 2.5, "log"),
    "pitch": (1600, 8000, "log"),
    "bright": (0.0, 1.0),
    "snap_ms": (0.15, 1.2, "log"),
    "tone": (0.0, 1.0),
    "partials": (1, 3, "int"),
    "inharm": (1.25, 2.8),
    "tink_ms": (1.5, 8, "log"),
    "body": (0.01, 0.8, "log"),
    "body_f": (150, 1200, "log"),
    "body_ms": (2, 12, "log"),
    "body_drop": (0.0, 0.5),
    "grit": (0.0, 0.6),
    "grit_ms": (2, 30, "log"),
    "slide": (0.0, 0.12),
    "slide_f": (1500, 7000, "log"),
    "spring": (0.0, 0.08),
    "spring_f": (2000, 6000, "log"),
    "room": (0.0, 0.3),
    "verb": (0.0, 0.35),
    "rt": (0.06, 0.4, "log"),
    "hp_steep": [0, 1],
    "hp": (80, 500, "log"),
    "lp": (7000, 19000, "log"),
    "deharsh": (0.0, 8.0),
    "deharsh_f": (2400, 4200, "log"),
    "sat": (0.0, 8.0),
    "attack_ms": (0.05, 2.0, "log"),
    "tail": (0.03, 0.2),
    "release_ms": (5, 35, "log"),
    "width": (0.0, 0.25),
}

DEFAULTS = {
    "gap": 0.06, "dir": 1.15, "a_lvl": 0.45, "pitch": 4500.0, "bright": 0.5, "snap_ms": 0.4, "tone": 0.5,
    "partials": 2, "inharm": 1.62, "tink_ms": 5.0, "body": 0.15, "body_f": 500.0, "body_ms": 6.0,
    "body_drop": 0.15, "grit": 0.15, "grit_ms": 6.0, "slide": 0.05, "slide_f": 3500.0, "spring": 0.0,
    "spring_f": 3500.0, "room": 0.08, "verb": 0.12, "rt": 0.15, "hp_steep": 0, "hp": 150.0, "lp": 17000.0,
    "deharsh": 3.0, "deharsh_f": 3200.0, "sat": 2.0, "attack_ms": 0.3, "tail": 0.06, "release_ms": 12.0,
    "width": 0.05,
}

LOG = "log"
_SWITCH = {"gap": (0.045, 0.08, LOG), "a_lvl": (1.7, 2.5, LOG), "body": (0.015, 0.07, LOG),
           "body_f": (450, 900, LOG), "body_ms": (2, 4, LOG), "tink_ms": (1.5, 3, LOG), "spring": (0, 0.04),
           "hp_steep": 1, "tone": (0.2, 0.45), "room": (0, 0.12), "slide": (0, 0.06), "hp": (180, 400, LOG),
           "verb": (0.06, 0.22), "rt": (0.08, 0.2, LOG), "deharsh": (0, 2), "lp": (9000, 14000, LOG),
           "tail": (0.07, 0.12)}
BOXES = {
    "toggle": {
        "on": dict(_SWITCH, dir=(1.12, 1.35, LOG), pitch=(2400, 4200, LOG)),
        "off": dict(_SWITCH, dir=(0.72, 0.88, LOG), pitch=(2800, 4500, LOG)),
        "latch": {"gap": (0.08, 0.12, LOG), "a_lvl": (1.15, 1.6, LOG), "body": (0.03, 0.08, LOG),
                  "slide": (0.03, 0.08), "tone": (0.1, 0.4), "room": (0, 0.12), "body_f": (320, 600, LOG),
                  "body_ms": (2, 4, LOG), "pitch": (2000, 3500, LOG), "tink_ms": (1.5, 3, LOG), "hp_steep": 1,
                  "verb": (0.08, 0.2), "rt": (0.08, 0.18, LOG), "hp": (150, 300, LOG), "spring": (0.02, 0.06),
                  "deharsh": (0, 2), "lp": (8000, 12000, LOG), "tail": (0.09, 0.13)},
        "micro": {"gap": (0.025, 0.045, LOG), "a_lvl": (1.5, 2.3, LOG), "pitch": (3200, 4800, LOG),
                  "body": (0.01, 0.04, LOG), "body_f": (600, 1100, LOG), "body_ms": (2, 4, LOG),
                  "hp": (250, 500, LOG), "room": (0, 0.12), "tink_ms": (1.5, 3, LOG), "slide": 0, "spring": 0,
                  "hp_steep": 1, "verb": (0.06, 0.2), "rt": (0.07, 0.15, LOG), "lp": (10000, 13000, LOG),
                  "tail": (0.07, 0.11)},
        "rocker": {"gap": (0.06, 0.1, LOG), "a_lvl": (0.45, 0.8, LOG), "pitch": (2000, 3500, LOG),
                   "tone": (0.1, 0.35), "body": (0.03, 0.09, LOG), "body_f": (400, 800, LOG),
                   "tink_ms": (1.5, 3, LOG), "spring": (0, 0.03), "hp_steep": 1, "hp": (150, 350, LOG),
                   "room": (0, 0.12), "verb": (0.05, 0.2), "rt": (0.08, 0.2, LOG), "body_ms": (2, 4, LOG),
                   "deharsh": (0, 2), "slide": (0.02, 0.08), "lp": (8000, 12000, LOG), "tail": (0.08, 0.13)},
    },
}

BUFFER_S = 0.45
PRE = 0.002


def family_space(cat: str, fam: str) -> dict:
    """The full space narrowed by one family box (L9)."""
    return L.narrow_space(SPACE, L.member_of(BOXES, cat, fam))


def family_params(cat: str, fam: str) -> dict:
    """The centre of one family box over the defaults (L9)."""
    return L.box_centre(DEFAULTS, L.member_of(BOXES, cat, fam))


FAMILIES = {cat: [(name, family_params(cat, name)) for name in boxes] for cat, boxes in BOXES.items()}


def _slide(p: dict, r: np.random.Generator, gap: float, lvl: float) -> np.ndarray:
    m = L.n_of(gap)
    f = p["slide_f"]
    w = dsp.butter(r.standard_normal(m), "band", (f / 1.6, min(16000.0, 1.6 * f)), 2)
    w = w * (np.arange(m) / max(m - 1, 1)) ** 2
    return L.unit(w) * p["slide"] * lvl


def _spring(p: dict, r: np.random.Generator) -> np.ndarray:
    ratios, amps = (1.0, 1.37, 2.11), (1.0, 0.6, 0.35)
    if p["hp_steep"] == 1:
        fs = p["spring_f"] * p["dir"]
        sp = L.partials(fs, ratios, amps, 0.005, 0.06, r, rise=0.001)
        nz = L.band_burst(0.006, r, fs / 1.8, min(16000.0, 1.8 * fs), rise=0.001, order=1)
        return L.unit(0.4 * sp + L.fit(nz, len(sp)))
    return L.partials(p["spring_f"], ratios, amps, 0.018, 0.12, r, rise=0.001)


def _contacts(p: dict, r: np.random.Generator, gap: float, lvl: float):
    n = L.n_of(BUFFER_S)
    main, snd = np.zeros(n), np.zeros(n)
    i0 = L.n_of(PRE)
    a, a_s = L.click_layers(p, r, level=p["a_lvl"] * lvl, send=True)
    L.add_at(main, a, i0)
    L.add_at(snd, a_s, i0)
    if p["slide"] > 0.004:
        sl = _slide(p, r, gap, lvl)
        L.add_at(main, sl, i0)
        L.add_at(snd, sl, i0)
    q = dict(p, body_f=p["body_f"] * p["dir"] ** 0.5)
    b, b_s = L.click_layers(q, r, pitch_scale=p["dir"], level=lvl, send=True)
    ib = L.n_of(PRE + gap)
    L.add_at(main, b, ib)
    L.add_at(snd, b_s, ib)
    if p["spring"] > 0.004:
        L.add_at(main, _spring(p, r), L.n_of(PRE + gap + 0.002), p["spring"] * lvl)
    return main, snd


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """One toggle take: stereo float64 at 48 kHz, peak -1 dBFS or below."""
    p = L.merged(DEFAULTS, p)
    lvl = 10 ** (0.03 * rng.standard_normal())
    gap = p["gap"] * math.exp(0.04 * rng.standard_normal())
    main, snd = _contacts(p, rng, gap, lvl)
    if p["hp_steep"] == 1:
        main = dsp.butter(main, "high", p["hp"], 2)
    main = L.finish_chain(main, p["hp"], p["deharsh"], p["deharsh_f"], p["sat"], p["lp"])
    snd = L.finish_chain(snd, max(p["hp"], 300), p["deharsh"], p["deharsh_f"], 0, p["lp"])
    st = L.tiny_space(main, rng, p["room"], p["width"])
    st = L.room_tail(st, snd, rng, p, size=0.15)
    st = L.end_gate(st, gap + p["tail"], p["release_ms"] / 1000, start=PRE)
    st = st * 10 ** (-1.5 / 20) / (L.peak(st) + 1e-12)
    st = fx.limit(st, ceiling_db=-1.0)
    return np.asarray(dsp.fade(st, 0.0005, 0.002), dtype=np.float64)
