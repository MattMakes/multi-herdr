"""Mallet: damped bar notes, a soft string pluck, and short runs of chime notes.

Spec: docs/specs/hark/recipes-impacts-tuned.md section 1.
"""

from __future__ import annotations

import math
import types
from typing import Any

import numpy as np

from hark.forge.engine import dsp
from hark.forge.recipes import tonal
from hark.forge.recipes.impacts import _common as cm

SR = cm.SR

TARGET = "a short glockenspiel note"
ALTS = ["a wine glass clink", "a phone notification", "a xylophone note", "a triangle", "a music box"]

# Each bar: pairs of (frequency over f0, level).
BARS: dict[str, tuple[tuple[float, float], ...]] = {
    "glock": ((1.0, 1.0), (2.756, 0.35), (5.404, 0.12), (8.933, 0.05)),
    "celesta": ((1.0, 1.0), (2.0, 0.05), (3.98, 0.22), (6.9, 0.05)),
    "metal": ((1.0, 1.0), (3.9, 0.25), (9.9, 0.08)),
    "marimba": ((1.0, 1.0), (3.92, 0.3), (9.8, 0.08)),
    "tine": ((1.0, 1.0), (5.8, 0.12), (6.27, 0.2)),
}

SPACE, DEFAULTS = cm.spec_rows("""
    style | note pluck = note
    bar | glock celesta metal marimba tine = glock
    f0 150.0 3200.0 log = 1046.5
    decay 0.05 1.6 log = 0.6
    slope 0.3 1.6 = 0.9
    hard 0.0 1.0 = 0.5
    strike -60.0 -6.0 = -26.0
    beat 0.0 6.0 = 1.2
    onset 0.0005 0.06 log = 0.004
    gate 0.05 1.0 log = 0.2
    gate_tau 0.005 0.08 log = 0.03
    tilt 0.6 2.5 = 1.4
    pos 0.05 0.5 = 0.18
    contact 0.0003 0.01 log = 0.003
    body -60.0 0.0 = -14.0
    body_f 90.0 400.0 log = 180.0
    hp 60.0 800.0 log = 300.0
    lp 3000.0 16000.0 log = 12000.0
    width 0.0 1.0 = 0.5
    pan -1.0 1.0 = 0.0
    room 0.0 0.35 = 0.1
    rt 0.1 0.8 log = 0.3
    oct -60.0 0.0 = -60.0
    oct_ratio | 0.5 2.0 = 0.5
    jit 0.0 1.0 = 0.5
""")

FINE: list[str] = [
    "decay", "slope", "hard", "strike", "beat", "onset", "gate", "gate_tau", "tilt", "pos", "contact", "body",
    "body_f", "hp", "lp", "width", "room", "rt", "oct",
]

_MEMBER_ROWS: dict[str, tuple] = cm.space_table("""
    style note pluck note note
    bar glock glock celesta marimba
    f0 1274 284.2 784 392
    decay 0.5027 0.1234 0.7148 1.6
    slope 0.8703 0.3789 0.4249 0.3
    hard 0.6343 0.1259 0.5432 0.45
    strike -14.89 -49.8 -49.98 -16.89
    beat 4.807 2.786 1.934 1.747
    onset 0.00162 0.06 0.00548 0.004282
    gate 0.08478 0.1256 0.08034 0.06156
    gate_tau 0.0422 0.01994 0.04521 0.03
    tilt 0.6 0.7913 1.4 1.4
    pos 0.4314 0.07761 0.18 0.18
    contact 0.00436 0.00563 0.003 0.003
    body -21.52 -40.86 -14 -14
    body_f 360.3 125.1 180 180
    hp 194.3 372.5 678.6 120
    lp 6499 4270 8593 16000
    width 0.3488 0.08427 0.8211 0.3335
    pan 0 0 0 0
    room 0.2474 0.07512 0.1831 0.1626
    rt 0.4467 0.6068 0.4 0.6887
    oct -2.483 -60 -60 -60
    oct_ratio 0.5 0.5 0.5 0.5
    jit 0.4495 0.02373 0.4 0.4
""")
FAMILY_NAMES: dict[str, list[str]] = {"mallet": ["glock", "pluck", "celesta", "marimba"]}
FAMILY: dict[str, list[dict]] = {
    "mallet": [{**DEFAULTS, **{k: row[i] for k, row in _MEMBER_ROWS.items()}} for i in range(4)],
}

# Study ranges (T15) as lo, hi and scale per key of tonal.measure.
STUDY: dict[str, tuple[float, float, float]] = cm.space_table("""
    dur_s 0.28 0.44 0.1
    attack_ms 16 45 12
    t20_ms 38 256 60
    centroid_hz 550 1505 300
    lo150 0.0 0.04 0.05
    tonal 0.55 0.68 0.1
    corr 0.33 0.99 0.2
""")
STUDY_PLUCK: dict[str, tuple[float, float, float]] = cm.space_table("""
    dur_s 0.25 0.35 0.08
    attack_ms 15 131 20
    t20_ms 104 150 30
    centroid_hz 335 809 100
    lo150 0.02 0.17 0.08
    tonal 0.4 0.62 0.1
    corr 0.95 0.99 0.1
""")


_PIN_PARTS = (SPACE, DEFAULTS, FINE)


def family_space(params: dict) -> dict[str, Any]:
    """Space around one member: only the fine keys stay free, the rest are pinned."""
    space, defaults, fine = _PIN_PARTS
    return tonal.family_space(space, {**defaults, **params}, fine)


def _count(seconds: float) -> int:
    return max(1, int(round(seconds * SR)))


def _audible(db: float) -> bool:
    return db > -59


def damper(t: np.ndarray, gate: float, gate_tau: float) -> np.ndarray:
    """Gain of the damper: 1 before ``gate``, then a fall with time constant ``gate_tau``."""
    return np.exp(-np.maximum(t - gate, 0) / max(gate_tau, 1e-4))


def _fade_in(secs: np.ndarray, onset: float) -> np.ndarray:
    return 0.5 - 0.5 * np.cos(np.pi * np.clip(secs / max(onset, 1e-4), 0, 1))


def spread(p: dict, rng: np.random.Generator) -> dict:
    """Vary one take: decay, gate and hardness move by ``jit``; the pitch stays put."""
    jolt = p["jit"] * rng.standard_normal(3)
    return {**p, "decay": p["decay"] * math.exp(0.08 * jolt[0]), "gate": p["gate"] * math.exp(0.05 * jolt[1]),
            "hard": float(np.clip(p["hard"] + 0.04 * jolt[2], 0, 1))}


def _bar_modes(name: str) -> tuple[tuple[float, float], ...]:
    if name not in BARS:
        raise ValueError(f"mallet: unknown bar {name!r}")
    return BARS[name]


def _bar_partial(cfg: dict, secs: np.ndarray, hz: float, ratio: float, gain: float, gen: np.random.Generator) -> np.ndarray:
    amp = gain * ratio ** (1.5 * (cfg["hard"] - 0.5))
    tau = max(cfg["decay"] * ratio ** -cfg["slope"], 0.003)
    beat_hz = cfg["beat"] * gen.uniform(0.6, 1.4)
    wobble = 1 - 0.3 * (0.5 - 0.5 * np.cos(2 * np.pi * beat_hz * secs)) if beat_hz > 0.01 else 1.0
    return amp * np.exp(-secs / tau) * wobble * np.sin(2 * np.pi * hz * secs + gen.uniform(0, 2 * np.pi))


def _struck(cfg: dict, gen: np.random.Generator, size: int, f0: float) -> np.ndarray:
    secs = np.arange(size) / SR
    out = np.zeros(size)
    for ratio, gain in _bar_modes(cfg["bar"]):
        hz = f0 * ratio
        if hz < 0.42 * SR:
            out += _bar_partial(cfg, secs, hz, ratio, gain, gen)
    out *= damper(secs, cfg["gate"], cfg["gate_tau"]) * _fade_in(secs, cfg["onset"])
    if _audible(cfg["strike"]):
        top = float(np.max(np.abs(out)))
        low_hz = 800 + 3000 * cfg["hard"]
        tau = 0.0006 + 0.002 * (1 - cfg["hard"])
        out += top * 10 ** (cfg["strike"] / 20) * tonal.burst(size, gen, low_hz, min(4 * low_hz, 16000), tau,
                                                         max(0.0002, 0.3 * cfg["onset"]))
    return out


def _string_level(cfg: dict, count: int, hz: float) -> float:
    """Level of string harmonic k: spectral tilt, pluck position notches and the soft finger."""
    return count ** -cfg["tilt"] * abs(math.sin(math.pi * count * cfg["pos"])) / (1 + (2 * math.pi * hz * cfg["contact"] / 6) ** 2)


def _add_box(out: np.ndarray, cfg: dict) -> np.ndarray:
    if not _audible(cfg["body"]):
        return out
    boxed = _box_band(out, cfg["body_f"])
    return out + 10 ** (cfg["body"] / 20) * boxed / (np.max(np.abs(boxed)) + 1e-12) * np.max(np.abs(out))


def _plucked(cfg: dict, gen: np.random.Generator, size: int, f0: float) -> np.ndarray:
    secs = np.arange(size) / SR
    out = np.zeros(size)
    for count in range(1, 13):
        hz = f0 * count * (1 + 0.0004 * count * count)
        if hz >= 0.4 * SR:
            break
        tau = max(cfg["decay"] * count ** -cfg["slope"], 0.004)
        out += _string_level(cfg, count, hz) * np.exp(-secs / tau) * np.sin(2 * np.pi * hz * secs + gen.uniform(0, 0.3))
    out *= damper(secs, cfg["gate"], cfg["gate_tau"]) * _fade_in(secs, cfg["onset"])
    return _add_box(out, cfg)


def _box_band(out: np.ndarray, hz: float) -> np.ndarray:
    return dsp.svf(out, hz, 3.0, "bandn")


def voice(p: dict, rng: np.random.Generator, n: int, f0: float) -> np.ndarray:
    """Play one note (bar or string) and add the octave copy when ``oct`` is on."""
    make = _plucked if p["style"] == "pluck" else _struck
    if p["style"] not in ("note", "pluck"):
        raise ValueError(f"mallet: unknown style {p['style']!r}")
    out = make(p, rng, n, f0)
    if _audible(p["oct"]):
        out = out + 10 ** (p["oct"] / 20) * make(p, rng, n, f0 * p["oct_ratio"])
    return out


def length_of(p: dict) -> float:
    """Length of a note in seconds: the earliest of the damped end, the natural end and 2 s."""
    return min(2.0, p["gate"] + 7 * p["gate_tau"] + 0.02, max(p["onset"] + 7 * p["decay"], 0.06) + 0.02)


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """Render one note: stereo float64 through the shared T13 finish."""
    p = spread({**DEFAULTS, **p}, rng)
    size = _count(length_of(p))
    out = voice(p, rng, size, p["f0"])
    return tonal.finish(out, rng, hp=p["hp"], lp=p["lp"], room_wet=p["room"], rt60=p["rt"], width=p["width"],
                        cut=500, pan=p["pan"], size=0.3)


# ------------------------------------------------------------ chime runs

SCALES: dict[str, tuple[int, ...]] = {
    "penta": (0, 2, 4, 7, 9),
    "major": (0, 2, 4, 5, 7, 9, 11),
    "lydian": (0, 2, 4, 6, 7, 9, 11),
    "sus": (0, 2, 5, 7, 10),
}
_RUN_SPACE, _RUN_DEFAULTS = cm.spec_rows("""
    notes 2 8 int = 4
    step 0.05 0.18 = 0.11
    swing 0.0 0.3 = 0.0
    scale | penta major lydian sus = penta
    pattern | up down arc = up
    span 1 3 int = 1
    spread 0.0 1.0 = 0.6
    accent -6.0 6.0 = 2.0
    last_gate 1.0 4.0 = 2.0
    ramp -6.0 6.0 = 0.0
""")
RUN_SPACE: dict[str, Any] = {**SPACE, **_RUN_SPACE}
RUN_DEFAULTS: dict[str, Any] = {**DEFAULTS, **_RUN_DEFAULTS}


def _up_down(idx: list[int]) -> list[int]:
    count = len(idx)
    half = (count + 1) // 2
    return idx[:half] + idx[: count - half][::-1]


_SHAPES = {"up": list, "down": lambda idx: idx[::-1], "arc": _up_down}


def _steps(count: int, span: int, pattern: str) -> list[int]:
    if pattern not in _SHAPES:
        raise ValueError(f"mallet: unknown pattern {pattern!r}")
    return _SHAPES[pattern]([idx * span for idx in range(count)])


def run_semitones(p: dict) -> list[int]:
    """Semitones above f0 of each note in a run."""
    if p["scale"] not in SCALES:
        raise ValueError(f"mallet: unknown scale {p['scale']!r}")
    scale = SCALES[p["scale"]]
    width_n = len(scale)
    return [12 * (idx // width_n) + scale[idx % width_n] for idx in _steps(int(p["notes"]), int(p["span"]), p["pattern"])]


def run_freqs(p: dict) -> list[float]:
    """Frequency in Hz of each note in a run."""
    p = {**RUN_DEFAULTS, **p}
    return [p["f0"] * 2 ** (semi / 12) for semi in run_semitones(p)]


def _start_times(cfg: dict, count: int) -> list[float]:
    return [idx * cfg["step"] * ((1 + cfg["swing"]) if idx % 2 else 1) for idx in range(count)]


def _one_run_note(cfg: dict, last: dict, idx: int, count: int, hz: float, gen: np.random.Generator) -> tuple[float, np.ndarray]:
    final = idx == count - 1
    ratio = spread(last if final else cfg, gen)
    level = 10 ** ((cfg["ramp"] * idx / max(1, count - 1) + (cfg["accent"] if final else 0)) / 20)
    side = 1 if idx % 2 else -1
    pan = cfg["pan"] + cfg["spread"] * (side * (0.4 + 0.6 * idx / max(1, count - 1)))
    out = voice(ratio, gen, _count(length_of(ratio)), hz)
    pair = tonal.finish(out, gen, hp=ratio["hp"], lp=ratio["lp"], room_wet=0, width=ratio["width"], cut=500,
                      pan=float(np.clip(pan, -1, 1)), end_db=-70, fade=0.004)
    return level, pair


def _fade_tail(sig: np.ndarray, seconds: float) -> np.ndarray:
    count = min(len(sig), _count(seconds))
    sig = sig.copy()
    sig[len(sig) - count:] *= (np.cos(np.linspace(0, np.pi / 2, count)) ** 2)[:, None]
    return sig


def render_run(p: dict, rng: np.random.Generator) -> np.ndarray:
    """Render a run of damped notes along a scale: stereo float64."""
    p = {**RUN_DEFAULTS, **p}
    freqs = run_freqs(p)
    count = len(freqs)
    last = {**p, "gate": p["gate"] * p["last_gate"]}
    starts = _start_times(p, count)
    size = _count(starts[-1] + length_of(last) + 0.05)
    out = np.zeros((size, 2))
    for idx, hz in enumerate(freqs):
        level, pair = _one_run_note(p, last, idx, count, hz, rng)
        at = _count(starts[idx]) if starts[idx] > 0 else 0
        width_n = min(len(pair), size - at)
        if width_n > 0:
            out[at:at + width_n] += level * pair[:width_n]
    out = tonal.room(out, rng, p["room"], p["rt"], 0.3, 0.3 + p["width"])
    out = dsp.trim_tail(out, -60.0, 0.01)
    return _fade_tail(out, 0.006)


RUN = types.SimpleNamespace(
    MODULE=__name__, SPACE=RUN_SPACE, DEFAULTS=dict(RUN_DEFAULTS), render=render_run,
    TARGET="a quick run of chime notes",
    ALTS=["a music box melody", "a harp glissando", "a phone ringtone", "a xylophone scale"],
)

_RUN_A = cm.value_table("""
    f0 802.3  decay 0.06476  slope 0.4859  hard 0.3246  strike -51.63  beat 2.417  onset 0.00098
    gate 0.2162  gate_tau 0.04794  tilt 0.7552  pos 0.3692  contact 0.00257  body -19.32  body_f 400
    hp 142.8  lp 3000  width 0.01723  room 0.09026  rt 0.2059  jit 0.5908  notes 3  step 0.04412
    swing 0.06969  scale lydian  pattern arc  span 2  spread 0.1809  accent -4.474  last_gate 3.365
""")
_RUN_B = cm.value_table("""
    f0 1046.5  notes 4  step 0.11  span 2  gate 0.12  gate_tau 0.03
""")
RUN_NAMES: dict[str, list[str]] = {"chime_run": ["flourish", "up4"]}
RUNS: dict[str, list[dict]] = {"chime_run": [{**RUN_DEFAULTS, **_RUN_A}, {**RUN_DEFAULTS, **_RUN_B}]}
