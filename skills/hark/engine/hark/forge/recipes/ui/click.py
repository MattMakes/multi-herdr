"""Recipe click: a clean, bright UI click and the pro product-video tick."""

from __future__ import annotations

import math

import numpy as np

from hark.forge.engine import dsp, fx
from hark.forge.recipes.ui import _layers as L

TARGET = "a clean bright user interface click"
TARGETS = {
    "click": "a clean bright user interface click",
    "pro_click": "a crisp high digital interface tick for a product video",
}
ALTS = ["a coin dropped on a table", "a mechanical keyboard key press", "a camera shutter", "a woodblock hit"]
REFSET = "click"
REFS = [f"{L.ref_root()}/ui/ui_click_{i:02d}.wav" for i in range(1, 5)]

# The full space. thock narrows body_f and hp below 150 and 80 (Q-RANGE-1: widened).
SPACE = {
    "pitch": (1200, 9500, "log"),
    "bright": (0.0, 1.0),
    "snap_ms": (0.15, 1.5, "log"),
    "tone": (0.0, 1.0),
    "partials": (1, 3, "int"),
    "inharm": (1.25, 2.8),
    "tink_ms": (1.5, 25, "log"),
    "body": (0.005, 0.8, "log"),
    "body_f": (120, 1200, "log"),
    "body_ms": (1.5, 12, "log"),
    "body_drop": (0.0, 0.5),
    "grit": (0.0, 0.6),
    "grit_ms": (2, 30, "log"),
    "rel": (0.0, 0.9),
    "rel_dt": (0.008, 0.09),
    "room": (0.0, 0.3),
    "verb": (0.0, 0.35),
    "rt": (0.04, 0.35, "log"),
    "hp": (50, 2500, "log"),
    "lp": (5000, 19000, "log"),
    "deharsh": (0.0, 8.0),
    "deharsh_f": (2400, 4200, "log"),
    "sat": (0.0, 9.0),
    "attack_ms": (0.05, 2.5, "log"),
    "length": (0.025, 0.14),
    "release_ms": (2, 30, "log"),
    "width": (0.0, 0.25),
}

DEFAULTS = {
    "pitch": 6000.0, "bright": 0.5, "snap_ms": 0.4, "tone": 0.6, "partials": 2, "inharm": 1.62,
    "tink_ms": 5.0, "body": 0.08, "body_f": 600.0, "body_ms": 5.0, "body_drop": 0.15, "grit": 0.15,
    "grit_ms": 6.0, "rel": 0.0, "rel_dt": 0.05, "room": 0.06, "verb": 0.0, "rt": 0.12, "hp": 200.0,
    "lp": 17000.0, "deharsh": 3.0, "deharsh_f": 3200.0, "sat": 2.0, "attack_ms": 0.3, "length": 0.06,
    "release_ms": 10.0, "width": 0.05,
}

LOG = "log"
BOXES = {
    "pro_click": {
        "tink": {"verb": 0, "pitch": (5000, 8000, LOG), "tone": (0.4, 0.8), "partials": (2, 3),
                 "tink_ms": (1.5, 4, LOG), "rel": (0.35, 0.8), "rel_dt": (0.009, 0.015),
                 "body": (0.06, 0.18, LOG), "body_f": (600, 1000, LOG), "body_ms": (2, 5, LOG),
                 "hp": (350, 800, LOG), "length": (0.025, 0.04), "release_ms": (2, 8, LOG)},
        "snap": {"verb": 0, "hp": (250, 600, LOG), "pitch": (4000, 7000, LOG), "tone": (0, 0.3),
                 "tink_ms": (1.5, 3, LOG), "bright": (0.5, 1.0), "snap_ms": (0.3, 1.2, LOG), "rel": (0.3, 0.7),
                 "rel_dt": (0.009, 0.016), "body": (0.12, 0.3, LOG), "body_f": (500, 900, LOG),
                 "body_ms": (2, 5, LOG), "length": (0.025, 0.045), "release_ms": (2, 8, LOG)},
        "mouse": {"verb": 0, "pitch": (2500, 4500, LOG), "tone": (0.1, 0.4), "tink_ms": (1.5, 3, LOG),
                  "body": (0.35, 0.8, LOG), "body_f": (300, 600, LOG), "body_ms": (3, 7, LOG),
                  "hp": (100, 200, LOG), "attack_ms": (0.3, 1.5, LOG), "rel": 0, "deharsh": (0, 3),
                  "lp": (11000, 16000, LOG), "length": (0.04, 0.06), "release_ms": (4, 12, LOG)},
        "glass": {"verb": 0, "pitch": (7000, 9500, LOG), "tone": (0.6, 0.9), "partials": 3,
                  "tink_ms": (2, 5, LOG), "rel": 0, "body": (0.03, 0.1, LOG), "body_f": (700, 1200, LOG),
                  "body_ms": (2, 4, LOG), "hp": (500, 1000, LOG), "length": (0.03, 0.06),
                  "release_ms": (4, 12, LOG)},
        "soft": {"verb": 0, "hp": (150, 350, LOG), "pitch": (3500, 5500, LOG), "tone": (0.3, 0.7),
                 "tink_ms": (1.5, 4, LOG), "rel": 0, "body": (0.2, 0.5, LOG), "body_f": (400, 900, LOG),
                 "body_ms": (2, 6, LOG), "deharsh": (2, 6), "length": (0.035, 0.06), "release_ms": (4, 12, LOG)},
    },
    "click": {
        "soft": {"pitch": (1800, 3200, LOG), "tone": (0.05, 0.3), "tink_ms": (1.5, 3, LOG), "bright": (0.1, 0.5),
                 "body": (0.1, 0.35, LOG), "body_f": (300, 700, LOG), "body_ms": (2, 5, LOG), "rel": 0,
                 "deharsh": (0, 2), "grit": (0.15, 0.45), "verb": (0.08, 0.3), "rt": (0.08, 0.25, LOG),
                 "lp": (8000, 12000, LOG), "length": (0.08, 0.12)},
        "thock": {"pitch": (1200, 2200, LOG), "tone": (0, 0.2), "tink_ms": (1.5, 3, LOG), "bright": (0, 0.3),
                  "body": (0.5, 0.8, LOG), "body_f": (120, 260, LOG), "body_ms": (3, 6, LOG), "rel": (0, 0.08),
                  "rel_dt": (0.025, 0.04), "hp": (50, 100, LOG), "deharsh": (0, 2), "grit": (0.1, 0.35),
                  "verb": (0.05, 0.25), "rt": (0.08, 0.25, LOG), "lp": (5000, 8000, LOG), "length": (0.09, 0.13)},
        "tink": {"pitch": (3800, 6000, LOG), "tone": (0.25, 0.5), "partials": 2, "tink_ms": (1.5, 3, LOG),
                 "bright": (0.4, 0.8), "body": (0.01, 0.05, LOG), "body_ms": (1.5, 3, LOG), "rel": 0,
                 "hp": (400, 900, LOG), "deharsh": (0, 2), "grit": (0.1, 0.4), "verb": (0.05, 0.25),
                 "lp": (13000, 17000, LOG), "length": (0.06, 0.1)},
        "snap": {"pitch": (2200, 4000, LOG), "tone": (0, 0.2), "bright": (0.3, 0.7), "grit": (0.2, 0.5),
                 "rel": (0.2, 0.6), "rel_dt": (0.008, 0.016), "body": (0.03, 0.2, LOG), "body_ms": (1.5, 5, LOG),
                 "deharsh": (0, 2), "verb": (0.05, 0.3), "lp": (10000, 15000, LOG), "length": (0.06, 0.1)},
        "double": {"pitch": (2000, 3800, LOG), "tone": (0.05, 0.3), "tink_ms": (1.5, 3, LOG), "rel": (0.05, 0.15),
                   "rel_dt": (0.03, 0.07), "body": (0.05, 0.3, LOG), "body_ms": (2, 6, LOG), "deharsh": (0, 2),
                   "grit": (0.1, 0.4), "verb": (0.05, 0.25), "lp": (9000, 13000, LOG), "length": (0.09, 0.13)},
    },
}


def family_space(cat: str, fam: str) -> dict:
    """The full space narrowed by one family box (L9)."""
    return L.narrow_space(SPACE, L.member_of(BOXES, cat, fam))


def family_params(cat: str, fam: str) -> dict:
    """The centre of one family box over the defaults (L9)."""
    return L.box_centre(DEFAULTS, L.member_of(BOXES, cat, fam))


FAMILIES = {cat: [(name, family_params(cat, name)) for name in boxes] for cat, boxes in BOXES.items()}


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """One click take: stereo float64 at 48 kHz, 2 ms lead, about 27-160 ms long."""
    p = L.merged(DEFAULTS, p)
    lvl = 10 ** (0.04 * rng.standard_normal())
    x, snd = L.click_layers(p, rng, level=lvl, send=True)
    rel_on = p["rel"] > 0.004
    if rel_on:
        q = dict(p, body=p["body"] * 0.5, tone=p["tone"] * 0.8)
        y, ys = L.click_layers(q, rng, pitch_scale=1.18, level=p["rel"] * lvl, send=True)
        k = L.n_of(p["rel_dt"] * math.exp(0.05 * rng.standard_normal()))
        L.add_at(x, y, k)
        L.add_at(snd, ys, k)
    x = L.finish_chain(x, p["hp"], p["deharsh"], p["deharsh_f"], p["sat"], p["lp"])
    snd = L.finish_chain(snd, max(p["hp"], 300), p["deharsh"], p["deharsh_f"], 0, p["lp"])
    lead = np.zeros(L.n_of(0.002))
    x = np.concatenate([lead, x])
    snd = np.concatenate([lead, snd])
    st = L.tiny_space(x, rng, p["room"], p["width"])
    st = L.room_tail(st, snd, rng, p)
    end = max(p["length"], p["rel_dt"] + 0.015 if rel_on else 0.0)
    st = L.end_gate(st, end, p["release_ms"] / 1000, start=0.002)
    st = fx.limit(st, ceiling_db=-1.0)
    return np.asarray(dsp.fade(st, 0.0005, 0.002), dtype=np.float64)
