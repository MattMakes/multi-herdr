"""Recipe popin: a soft round pop-in of a UI element, and a soft finger tap.

A low designed thump under a thin tick, with optional air, a second body, a
surface mode, a contact burst and a long quiet ring.
"""

from __future__ import annotations

import math

import numpy as np

from hark.forge.engine import dsp, fx
from hark.forge.recipes.ui import _layers as L

TARGET = "a soft round low pop as an icon appears on screen"
_TAP = "a soft low thump of a finger touching a screen"
TARGETS = {"pro_pop_in": TARGET, "pro_tap": _TAP, "tap": _TAP}
ALTS = ["a door slam", "a balloon popping", "a wooden knock", "a cork popping"]
REFSET = "pro_pop_in"

# Q-RANGE-1: att_ms starts at 0.8 and hp reaches 80, so the tap families fit inside the space.
SPACE = {
    "body_f": (70, 220, "log"), "drop": (0.0, 1.0), "att_ms": (0.8, 22, "log"), "hold_ms": (0.5, 45, "log"),
    "sag_db": (0.0, 12.0), "rel_ms": (3, 40, "log"), "harm": (0.0, 0.35), "sub": (0.0, 0.7), "sat": (0.0, 8.0),
    "tick_db": (-34.0, -2.0), "tick_f": (1200, 8000, "log"), "tick_bw": (1.0, 3.0), "tick_ms": (2, 40, "log"),
    "glass": (0.0, 1.0), "air_db": (-60.0, -24.0), "air_tilt": (-1.0, 1.0), "glass_f": (1800, 7000, "log"),
    "dbl": (0.0, 0.9), "dbl_dt": (0.05, 0.08), "dbl_ratio": (0.85, 1.25), "width": (0.0, 1.2),
    "knock": (0.0, 0.5), "knock_f": (280, 900, "log"), "tick_width": (0.0, 1.0), "hp": (35, 80, "log"),
    "lp": (2500, 16000, "log"), "length": (0.08, 0.3), "release_ms": (6, 40, "log"),
    "contact_db": (-60.0, 6.0), "contact_ms": (0.2, 3, "log"), "contact_f": (800, 6000, "log"),
    "ring_db": (-60.0, -10.0), "ring_ms": (10, 90, "log"), "contact_lp_x": (0.0, 2.5),
}

DEFAULTS = {
    "body_f": 105.0, "drop": 0.35, "att_ms": 12.0, "hold_ms": 6.0, "sag_db": 0.0, "rel_ms": 8.0, "harm": 0.12,
    "sub": 0.2, "sat": 2.0, "tick_db": -12.0, "tick_f": 3500.0, "tick_bw": 2.5, "tick_ms": 12.0, "glass": 0.0,
    "air_db": -60.0, "air_tilt": 0.0, "glass_f": 3200.0, "dbl": 0.0, "dbl_dt": 0.065, "dbl_ratio": 1.0,
    "width": 0.6, "knock": 0.0, "knock_f": 500.0, "tick_width": 0.6, "hp": 45.0, "lp": 9000.0, "length": 0.18,
    "release_ms": 15.0, "contact_db": -60.0, "contact_ms": 0.8, "contact_f": 2500.0, "ring_db": -60.0,
    "ring_ms": 40.0, "contact_lp_x": 0.0,
}

LOG = "log"
_TAP_BASE = {"drop": (0, 0.15), "sub": (0, 0.2), "width": (0.7, 1.1), "knock": 0, "dbl": 0}
_SURF = {"width": (0, 0.3), "dbl": 0, "contact_lp_x": (1.1, 1.8)}
BOXES = {
    "pro_pop_in": {
        "round": {"width": (0.6, 1.1), "knock": 0, "body_f": (95, 120, LOG), "att_ms": (12, 20, LOG),
                  "hold_ms": (5, 14, LOG), "sag_db": (4, 10), "rel_ms": (3, 7, LOG), "glass": 0,
                  "air_db": (-50, -38), "dbl": 0, "length": (0.12, 0.18)},
        "double": {"width": (0.6, 1.1), "knock": 0, "body_f": (95, 125, LOG), "att_ms": (6, 14, LOG),
                   "hold_ms": (4, 12, LOG), "sag_db": (3, 9), "rel_ms": (3, 7, LOG), "glass": 0,
                   "air_db": (-50, -38), "dbl": (0.5, 0.85), "dbl_dt": (0.06, 0.07), "dbl_ratio": (1.1, 1.25),
                   "length": (0.16, 0.22)},
        "knock": {"width": (0.5, 0.9), "knock": 0, "body_f": (115, 135, LOG), "harm": (0.25, 0.35), "sat": (4, 8),
                  "att_ms": (5, 10, LOG), "hold_ms": (4, 10, LOG), "sag_db": (3, 8), "rel_ms": (3, 6, LOG),
                  "glass": 0, "tick_db": (-10, -4), "tick_f": (1500, 3000, LOG), "contact_db": (-16, -8),
                  "contact_ms": (0.5, 1.5, LOG), "contact_f": (1500, 3000, LOG), "air_db": (-48, -38), "dbl": 0,
                  "length": (0.1, 0.15)},
        "deep": {"width": (0.7, 1.1), "knock": 0, "body_f": (75, 92, LOG), "att_ms": (14, 22, LOG),
                 "hold_ms": (8, 20, LOG), "sag_db": (5, 12), "rel_ms": (4, 9, LOG), "sub": (0.3, 0.7),
                 "harm": (0, 0.08), "sat": (0, 2), "glass": 0, "air_db": (-58, -46), "dbl": 0, "tick_db": (-30, -20),
                 "lp": (4000, 7000, LOG), "length": (0.14, 0.22)},
        "airy": {"width": (0.6, 1.1), "knock": 0, "body_f": (100, 130, LOG), "att_ms": (8, 16, LOG),
                 "hold_ms": (5, 14, LOG), "sag_db": (3, 9), "rel_ms": (3, 7, LOG), "glass": 0, "air_db": (-32, -24),
                 "air_tilt": (0.3, 1.0), "tick_db": (-12, -6), "tick_f": (4000, 8000, LOG),
                 "tick_ms": (15, 35, LOG), "tick_width": (0.8, 1.0), "lp": (12000, 16000, LOG), "dbl": 0,
                 "length": (0.12, 0.18)},
    },
    "pro_tap": {
        "finger": dict(_TAP_BASE, glass=0, body_f=(135, 165, LOG), att_ms=(8, 13, LOG), hold_ms=(5, 12, LOG),
                       sag_db=(3, 9), rel_ms=(2.5, 5, LOG), air_db=(-40, -30), air_tilt=(0, 0.8),
                       contact_db=(-11, -3), contact_ms=(0.4, 1.2, LOG), contact_f=(1500, 3500, LOG),
                       lp=(9000, 16000, LOG), length=(0.07, 0.1)),
        "glass": dict(_TAP_BASE, body_f=(140, 170, LOG), att_ms=(6, 11, LOG), glass=(0.4, 0.8),
                      glass_f=(2500, 5000, LOG), tick_db=(-14, -6), hold_ms=(4, 11, LOG), sag_db=(3, 9),
                      rel_ms=(2.5, 5, LOG), air_db=(-44, -32), air_tilt=(0, 1.0), contact_db=(-9, -1),
                      contact_ms=(0.3, 0.8, LOG), contact_f=(3000, 6000, LOG), lp=(11000, 16000, LOG),
                      length=(0.07, 0.1)),
        "soft": dict(_TAP_BASE, glass=0, body_f=(110, 140, LOG), att_ms=(10, 18, LOG), tick_db=(-24, -14),
                     hold_ms=(7, 15, LOG), sag_db=(4, 10), rel_ms=(3, 6, LOG), air_db=(-46, -36),
                     contact_db=(-17, -9), contact_ms=(0.8, 2.0, LOG), contact_f=(1200, 2500, LOG),
                     length=(0.08, 0.11)),
        "knock": dict(_TAP_BASE, glass=0, body_f=(150, 210, LOG), att_ms=(3, 7, LOG), tick_db=(-10, -2),
                      hold_ms=(3, 9, LOG), sag_db=(3, 9), rel_ms=(2.5, 5, LOG), air_db=(-38, -28),
                      air_tilt=(0, 1.0), contact_db=(-5, 3), contact_ms=(0.4, 1.0, LOG),
                      contact_f=(2000, 4500, LOG), lp=(11000, 16000, LOG), length=(0.06, 0.09)),
        "double": dict(_TAP_BASE, glass=0, body_f=(130, 165, LOG), att_ms=(6, 12, LOG), dbl=(0.3, 0.6),
                       dbl_dt=(0.05, 0.065), hold_ms=(4, 10, LOG), sag_db=(3, 9), rel_ms=(2.5, 5, LOG),
                       air_db=(-42, -32), contact_db=(-11, -3), contact_ms=(0.4, 1.2, LOG),
                       contact_f=(1500, 3500, LOG), length=(0.11, 0.14)),
    },
    "tap": {
        "finger": dict(_SURF, knock=(0.05, 0.3), glass=0, body_f=(110, 160, LOG), att_ms=(1.0, 2.0, LOG),
                       hold_ms=(0.5, 2, LOG), rel_ms=(2.5, 5, LOG), ring_db=(-30, -21), ring_ms=(17, 32, LOG),
                       contact_db=(-6, 2), contact_ms=(0.4, 1.2, LOG), contact_f=(1200, 2800, LOG),
                       tick_db=(-24, -14), air_db=(-60, -42), lp=(7000, 12000, LOG), hp=(50, 75, LOG),
                       sub=(0, 0.15), length=(0.08, 0.12)),
        "glass": dict(_SURF, knock=(0.05, 0.25), body_f=(120, 180, LOG), att_ms=(1.0, 2.0, LOG), glass=(0.3, 0.6),
                      glass_f=(1800, 3200, LOG), tick_db=(-18, -10), tick_f=(1500, 3500, LOG),
                      hold_ms=(0.5, 2, LOG), tick_ms=(2, 8, LOG), rel_ms=(2.5, 5, LOG), ring_db=(-30, -22),
                      ring_ms=(18, 35, LOG), contact_db=(-2, 4), contact_ms=(0.3, 0.8, LOG),
                      contact_f=(2000, 4000, LOG), air_db=(-60, -40), lp=(8000, 12000, LOG), hp=(50, 75, LOG),
                      sub=(0, 0.15), length=(0.08, 0.12)),
        "table": dict(_SURF, knock=(0.05, 0.4), glass=0, body_f=(80, 110, LOG), att_ms=(1.2, 3, LOG),
                      hold_ms=(0.5, 3, LOG), sub=0, rel_ms=(3, 6, LOG), ring_db=(-29, -20), ring_ms=(15, 30, LOG),
                      contact_db=(-8, 0), contact_ms=(0.8, 2.0, LOG), contact_f=(900, 2000, LOG),
                      tick_db=(-26, -16), air_db=(-60, -46), lp=(6000, 10000, LOG), hp=(55, 80, LOG),
                      length=(0.09, 0.14)),
        "knuckle": dict(_SURF, knock=(0.1, 0.5), glass=0, body_f=(150, 220, LOG), att_ms=(0.8, 1.6, LOG),
                        tick_db=(-16, -8), tick_f=(1500, 3500, LOG), hold_ms=(0.5, 1.5, LOG), tick_ms=(2, 8, LOG),
                        rel_ms=(2.5, 4.5, LOG), ring_db=(-27, -19), ring_ms=(18, 35, LOG), contact_db=(-2, 4),
                        contact_ms=(0.4, 1.0, LOG), contact_f=(1500, 3200, LOG), air_db=(-60, -46),
                        lp=(7000, 11000, LOG), hp=(55, 80, LOG), sub=(0, 0.1), length=(0.08, 0.12)),
        "soft": dict(_SURF, knock=(0.05, 0.25), glass=0, body_f=(95, 130, LOG), att_ms=(1.5, 4, LOG),
                     tick_db=(-28, -18), hold_ms=(1, 4, LOG), rel_ms=(3.5, 7, LOG), ring_db=(-28, -19),
                     ring_ms=(15, 30, LOG), contact_db=(-12, -4), contact_ms=(0.8, 2.0, LOG),
                     contact_f=(900, 2200, LOG), air_db=(-60, -44), lp=(6000, 10000, LOG), hp=(50, 75, LOG),
                     sub=(0, 0.15), length=(0.09, 0.14)),
    },
}

BUFFER_S = 0.34
PRE = 0.002
BODY_S = 0.32
OFF_DB = -59.5


def family_space(cat: str, fam: str) -> dict:
    """The full space narrowed by one family box (L9)."""
    return L.narrow_space(SPACE, L.member_of(BOXES, cat, fam))


def family_params(cat: str, fam: str) -> dict:
    """The centre of one family box over the defaults (L9)."""
    return L.box_centre(DEFAULTS, L.member_of(BOXES, cat, fam))


FAMILIES = {cat: [(name, family_params(cat, name)) for name in boxes] for cat, boxes in BOXES.items()}


# ----------------------------------------------------------------- the layers

def _ring(p: dict, r: np.random.Generator, f: float, att: float) -> np.ndarray:
    rg = L.designed_thump(f, BODY_S, att, 0.0, p["ring_ms"] / 1000, drop=0.5 * p["drop"], harm=p["harm"], r=r)
    nz = L.unit(dsp.butter(r.standard_normal(len(rg)), "band", (0.5 * f, 3 * f), 2))
    nz = nz * np.abs(rg) / (L.peak(rg) + 1e-12)
    return L.unit(0.6 * L.unit(rg) + 0.8 * nz)


def body(p: dict, r: np.random.Generator, f: float, level: float) -> np.ndarray:
    """One body hit: designed thump, sag, octave-down partner, quiet ring, light drive."""
    att, hold, rel = p["att_ms"] / 1000, p["hold_ms"] / 1000, p["rel_ms"] / 1000
    y = L.designed_thump(f, BODY_S, att, hold, rel, drop=p["drop"], harm=p["harm"], r=r)
    if p["sag_db"] > 0.05:
        u = np.clip((L.t_of(len(y)) - att) / max(0.001, hold), 0, 1)
        y = y * 10 ** (-p["sag_db"] * u / 20)
    if p["sub"] > 0.01:
        y = y + p["sub"] * L.designed_thump(0.5 * f, BODY_S, 1.3 * att, hold, 1.2 * rel, drop=0.5 * p["drop"],
                                            harm=0.0, r=r)
    if p.get("ring_db", -60.0) > OFF_DB:
        y = L.unit(y) + 10 ** (p["ring_db"] / 20) * _ring(p, r, f, att)
    if p["sat"] > 0.05:
        y = np.asarray(fx.saturate(y, drive_db=p["sat"], mode="tanh", level="peak"), dtype=float)
    return level * L.unit(y)


def _knock(bus: np.ndarray, p: dict, r: np.random.Generator, lvl: float) -> None:
    kf = p["knock_f"] * math.exp(0.02 * r.standard_normal())
    tau = 0.004 + (0.006 if p["knock_f"] < 500 else 0.0)
    att = max(0.0005, 0.3 * p["att_ms"] / 1000)
    kn = L.partials(kf, (1.0, 2.7), (1.0, 0.25), tau, 0.08, r, rise=att)
    L.add_at(bus, kn, L.n_of(PRE + 0.3 * p["att_ms"] / 1000), p["knock"] * lvl)


def _contact(p: dict, r: np.random.Generator, n: int, ref_peak: float) -> np.ndarray:
    cf = p.get("contact_f", 2500.0)
    c = L.band_burst(p.get("contact_ms", 0.8) / 1000, r, cf / 2.5, min(18000.0, 2.5 * cf), rise=0.0002)
    lpx = p.get("contact_lp_x", 0.0)
    if lpx > 0.05:
        c = L.unit(dsp.butter(c, "low", min(18000.0, cf * max(1.0, lpx)), 4))
    bus = np.zeros(n)
    L.add_at(bus, c, L.n_of(PRE), ref_peak * 10 ** (p["contact_db"] / 20))
    return bus


def _tick(p: dict, r: np.random.Generator) -> np.ndarray:
    half = 2 ** (p["tick_bw"] / 2)
    nz = L.band_burst(p["tick_ms"] / 1000, r, p["tick_f"] / half, min(16000.0, p["tick_f"] * half), rise=0.0004)
    tau = min(0.012, max(0.002, 0.6 * p["tick_ms"] / 1000))
    gl = L.partials(p["glass_f"], (1.0, 2.32), (1.0, 0.3), tau, 0.08, r, rise=0.0003)
    return L.unit((1 - p["glass"]) * L.fit(nz, len(gl)) + p["glass"] * gl)


def _air(p: dict, r: np.random.Generator, bus: np.ndarray, pk: float) -> np.ndarray:
    env = np.sqrt(np.convolve(np.square(bus), np.ones(96) / 96, mode="same"))
    nz = dsp.butter(r.standard_normal(len(bus)), "band", (1000, 14000), 2)
    a = abs(p["air_tilt"])
    shaped = dsp.butter(nz, "high" if p["air_tilt"] > 0 else "low", 4000, 1)
    nz = a * shaped + (1 - a) * nz
    nz = nz / (L.rms(nz) + 1e-12)
    return nz * env / (env.max() + 1e-12) * pk * 10 ** (p["air_db"] / 20)


def _buses(p: dict, r: np.random.Generator, f: float, lvl: float):
    n = L.n_of(BUFFER_S)
    bus = np.zeros(n)
    L.add_at(bus, body(p, r, f, lvl), L.n_of(PRE))
    if p["dbl"] > 0.02:
        L.add_at(bus, body(p, r, f * p["dbl_ratio"], p["dbl"] * lvl), L.n_of(PRE + p["dbl_dt"]))
    if p["knock"] > 0.005:
        _knock(bus, p, r, lvl)
    contact_on = p.get("contact_db", -60.0) > OFF_DB
    cbus = _contact(p, r, n, L.peak(bus)) if contact_on else np.zeros(n)
    bus = np.asarray(dsp.butter(bus, "high", p["hp"], 2), dtype=float)
    tk = _tick(p, r)
    pk = L.peak(bus)
    tbus = np.zeros(n)
    t_tick = PRE + 0.6 * p["att_ms"] / 1000
    L.add_at(tbus, tk, L.n_of(t_tick), pk * 10 ** (p["tick_db"] / 20))
    if p["dbl"] > 0.02:
        L.add_at(tbus, tk, L.n_of(t_tick + p["dbl_dt"]), pk * p["dbl"] * 10 ** ((p["tick_db"] - 3) / 20))
    if p["air_db"] > OFF_DB:
        tbus = tbus + _air(p, r, bus, pk)
    return bus, cbus, tbus, contact_on


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """One pop-in or tap: stereo float64 at 48 kHz, 2 ms + ``length`` long, peak -1 dBFS or below."""
    p = L.merged(DEFAULTS, p)
    f = p["body_f"] * math.exp(0.012 * rng.standard_normal())
    lvl = 10 ** (0.02 * rng.standard_normal())
    bus, cbus, tbus, contact_on = _buses(p, rng, f, lvl)
    d = L.n_of(p["width"] / (6 * f)) if p["width"] > 0.02 else 0
    right = np.concatenate([np.zeros(d), bus])[: len(bus)]
    st = np.column_stack([bus + cbus, right + cbus])
    st = st + L.fit(np.asarray(fx.decorrelate(tbus, p["tick_width"], 800.0), dtype=float), len(st))
    st = np.asarray(dsp.butter(st, "low", p["lp"], 2), dtype=float)
    st = L.end_gate(st, p["length"], p["release_ms"] / 1000, start=PRE)
    if contact_on:
        st = st * 10 ** (-1.5 / 20) / (L.peak(st) + 1e-12)
    st = fx.limit(st, ceiling_db=-1.0)
    return np.asarray(dsp.fade(st, 0.0005, 0.002), dtype=np.float64)
