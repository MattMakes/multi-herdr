"""Camera shutter: a release click, a short gap, then the heavier landing of the mechanism."""

from __future__ import annotations

import math

import numpy as np

from hark.forge.engine import dsp
from hark.forge.recipes import tonal
from hark.forge.recipes.ui import _layers as kit

TARGET = "a camera shutter click"
TARGETS = {"shutter": TARGET}
ALTS = ["a door latch", "a stapler", "a mouse click", "a light switch", "a gun being cocked"]

SPACE = kit.space_table("""
    gap 0.015 0.14 log
    a_db -24.0 6.0
    a_hz 1500 9000 log
    a_ms 0.2 3 log
    a_modes 0.0 1.0
    whirr -60.0 -6.0
    whirr_hz 600 5000 log
    b_hz 300 3000 log
    b_decay 0.004 0.08 log
    b_noise -40.0 0.0
    b_noise_hz 600 6000 log
    b_contact 0.0002 0.003 log
    body -60.0 0.0
    body_f 70 300 log
    body_ms 4 50 log
    rattle -60.0 -6.0
    rattle_hz 2000 7000 log
    hp 60 500 log
    lp 3000 16000 log
    room 0.0 0.3
    rt 0.05 0.4 log
    width 0.0 0.5
""")

DEFAULTS = kit.value_table("""
    gap 0.045  a_db -6.0  a_hz 3500.0  a_ms 0.8  a_modes 0.5  whirr -30.0  whirr_hz 1800.0  b_hz 900.0
    b_decay 0.025  b_noise -10.0  b_noise_hz 2000.0  b_contact 0.0008  body -16.0  body_f 150.0
    body_ms 14.0  rattle -30.0  rattle_hz 3500.0  hp 90.0  lp 9000.0  room 0.1  rt 0.14  width 0.06
""")

FINE = ["a_db", "a_hz", "a_ms", "a_modes", "whirr", "whirr_hz", "b_decay", "b_noise", "b_noise_hz", "b_contact",
        "body", "body_f", "body_ms", "rattle", "rattle_hz", "hp", "lp", "room", "rt", "width"]

BOX_MODES = [(1.0, 1.0, 1.0), (1.62, 0.7, 0.7), (2.31, 0.5, 0.5), (3.17, 0.35, 0.35), (4.4, 0.2, 0.25)]
LATCH_MODES = [(1.0, 1.0), (1.71, 0.6), (2.63, 0.35)]

STUDY = kit.space_table("""
    dur_s 0.16 0.38 0.08
    attack_ms 16 170 20
    t20_ms 20 123 30
    centroid_hz 624 2187 300
    lo150 0.01 0.24 0.1
    tonal 0.3 0.52 0.1
    corr 0.96 0.99 0.1
""")

_EDITS = {
    "dslr": kit.value_table("""
        gap 0.0391  a_db -23.5  a_hz 5537  a_ms 0.5005  a_modes 0.3966  whirr -9.218  whirr_hz 1085
        b_hz 1433  b_decay 0.006  b_noise -1.243  b_noise_hz 1355  b_contact 0.00221  body -20.61
        body_f 238.2  body_ms 19.41  rattle -26.17  rattle_hz 5575  hp 92.91  lp 5091  room 0.2557
        rt 0.2552  width 0.1069
    """),
    "snap": kit.value_table("""
        gap 0.05821  a_db -22.88  a_hz 3300  a_ms 2.964  a_modes 0.1957  whirr -15.59  whirr_hz 5000
        b_hz 2037  b_decay 0.004  b_noise -2.783  b_noise_hz 2530  b_contact 0.00031  body -10.33
        body_f 156.8  body_ms 5.495  rattle -48.36  rattle_hz 2581  hp 173.8  lp 9437  room 0.01662
        rt 0.0526  width 0.234
    """),
    "mirror": kit.value_table("""
        gap 0.06462  a_db -18.51  a_hz 8113  a_ms 0.5578  a_modes 0.1115  whirr -6.0  whirr_hz 688.7
        b_hz 1511  b_decay 0.00622  b_noise 0.0  b_noise_hz 1468  b_contact 0.003  body -33.33  body_f 300
        body_ms 22.0  rattle -26.59  rattle_hz 7000  hp 128.8  lp 6009  room 0.3  rt 0.374  width 0.09233
    """),
    "film": kit.value_table("""
        gap 0.095  a_db -12.29  a_hz 4200  a_ms 0.9  whirr -14.0  whirr_hz 900  b_hz 720  b_decay 0.01103
        b_noise -7.79  b_noise_hz 600  b_contact 0.001728  body -12.0  body_f 112.7  body_ms 26.32
        rattle -24.0  rattle_hz 4200  hp 84.56  lp 6500  room 0.3  rt 0.2  width 0.08
    """),
    "quick": kit.value_table("""
        gap 0.021  a_db -4.0  a_hz 3800  a_ms 0.2281  a_modes 0.4098  whirr -40.0  b_hz 1250  b_decay 0.008
        b_noise -6.0  b_noise_hz 2200  b_contact 0.001477  body -17.09  body_f 199.6  body_ms 8.0
        rattle -50.0  hp 150  room 0.09189  rt 0.2138  width 0.1
    """),
}

FAMILY_NAMES = {"shutter": list(_EDITS)}
FAMILY = {"shutter": [dict(DEFAULTS, **_EDITS[name]) for name in _EDITS]}
FAMILIES = {"shutter": list(zip(FAMILY_NAMES["shutter"], FAMILY["shutter"]))}


def family_space(params: dict) -> dict:
    """Space of a member: the fine keys stay free, gap and b_hz keep the member values (T15)."""
    return tonal.family_space(SPACE, _full(params), FINE)


def _full(params: dict) -> dict:
    """Lay the member values over the defaults."""
    return dict(DEFAULTS, **params)


def measure(x: np.ndarray) -> dict:
    """Catalog measurement of a render (T14)."""
    return tonal.measure(x)


def study_penalty(x: np.ndarray) -> float:
    """How far a render sits from the film-shutter study ranges (T15)."""
    return float(tonal.outside(tonal.measure(x), STUDY))


def study_extra(x: np.ndarray, row: dict | None = None) -> float:
    """Critic extra: the study distance with its sign flipped."""
    return -study_penalty(x)


def _count(seconds: float) -> int:
    return max(1, int(round(seconds * tonal.SR)))


def _norm(sig: np.ndarray) -> np.ndarray:
    return sig / (float(np.max(np.abs(sig))) + 1e-12)


def _release(cfg: dict, gen: np.random.Generator, a_hz: float) -> np.ndarray:
    latch_n = _count(0.03)
    latch = tonal.burst(latch_n, gen, a_hz / 1.8, min(1.8 * a_hz, 18000.0), cfg["a_ms"] / 1000, rise=0.0002)
    if cfg["a_modes"] > 0.005:
        ratios = [ratio for ratio, _ in LATCH_MODES]
        latch_ring = tonal.modes(latch_n, gen, [a_hz * ratio for ratio in ratios], [gain for _, gain in LATCH_MODES], [0.004 / ratio for ratio in ratios],
                         jitter=0.01)
        latch = latch + cfg["a_modes"] * _norm(latch_ring)
    return _norm(latch)


def _landing(cfg: dict, gen: np.random.Generator, box_n: int, b_hz: float) -> np.ndarray:
    freqs = [b_hz * ratio for ratio, _, _ in BOX_MODES]
    taus = [max(0.0008, cfg["b_decay"] * fall) for _, _, fall in BOX_MODES]
    box_ring = tonal.modes(box_n, gen, freqs, [latch for _, latch, _ in BOX_MODES], taus, jitter=0.01)
    box = _norm(np.asarray(tonal.strike(box_ring, gen, cfg["b_contact"], bright=0.6))[:box_n])
    if cfg["b_noise"] > -39:
        centre = cfg["b_noise_hz"]
        box = box + 10 ** (cfg["b_noise"] / 20) * tonal.burst(box_n, gen, centre / 2, 2 * centre, 0.001, rise=0.0002)
    if cfg["body"] > -59:
        box = box + 10 ** (cfg["body"] / 20) * tonal.thump(box_n, cfg["body_f"], cfg["body_ms"] / 1000, rise=0.0008)
    if cfg["rattle"] > -59:
        box = box + _buzz(cfg, gen, box_n)
    return box


def _buzz(cfg: dict, gen: np.random.Generator, box_n: int) -> np.ndarray:
    """Loose-part buzz at ``rattle_hz``, delayed by 4 ms after the landing."""
    centre = cfg["rattle_hz"]
    buzz = tonal.burst(box_n, gen, centre / 1.6, 1.6 * centre, 0.006, rise=0.002)
    lag = min(box_n, _count(0.004))
    return 10 ** (cfg["rattle"] / 20) * np.concatenate([np.zeros(lag), buzz])[:box_n]


def _spin(cfg: dict, gen: np.random.Generator, gap: float) -> np.ndarray:
    whirr_n = _count(gap)
    centre = cfg["whirr_hz"]
    noise = dsp.butter(gen.standard_normal(whirr_n), "band", (centre / 1.7, 1.7 * centre), 2)
    noise = noise * np.sin(np.pi * np.linspace(0, 1, whirr_n)) ** 0.7
    return 10 ** (cfg["whirr"] / 20) * _norm(noise)


def _place_stages(cfg: dict, rng: np.random.Generator, gap: float, a_hz: float, b_hz: float) -> np.ndarray:
    """Place the release click (at ``a_db``) and, ``gap`` seconds on, the landing; at most 1 s long."""
    tail = 7 * max(cfg["b_decay"], cfg["body_ms"] / 1000 if cfg["body"] > -59 else 0.0)
    size = _count(min(1.0, gap + tail + 0.03))
    latch = _release(cfg, rng, a_hz)
    box_n = max(1, size - _count(gap))
    box = _landing(cfg, rng, box_n, b_hz)
    out = np.zeros(size)
    latch_n = min(len(latch), size)
    out[:latch_n] += 10 ** (cfg["a_db"] / 20) * latch[:latch_n]
    out[size - box_n:] += box
    return out


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """Render one shutter: stereo float64 at 48 kHz, about 1 s at most."""
    p = dict(DEFAULTS, **(p or {}))
    gap = p["gap"] * math.exp(0.03 * rng.standard_normal())
    b_hz = p["b_hz"] * math.exp(0.03 * rng.standard_normal())
    a_hz = p["a_hz"] * math.exp(0.03 * rng.standard_normal())
    out = _place_stages(p, rng, gap, a_hz, b_hz)
    size = len(out)
    if p["whirr"] > -59:
        noise = _spin(p, rng, gap)
        out[: min(size, len(noise))] += noise[: min(size, len(noise))]
    pair = tonal.finish(out, rng, hp=p["hp"], lp=p["lp"], room_wet=p["room"], rt60=p["rt"], width=p["width"], cut=600)
    return np.asarray(pair, dtype=np.float64)
