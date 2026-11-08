"""Recipe mallet: gated mallet notes, a soft pluck and runs of chime notes.

Spec: docs/specs/hark/recipes-impacts-tuned.md section 1.
"""

from __future__ import annotations

import math
import types
from typing import Any

import numpy as np

from hark.forge.engine import dsp
from hark.forge.recipes import tonal

SR = 48000

TARGET = "a short glockenspiel note"
ALTS = ["a wine glass clink", "a phone notification", "a xylophone note", "a triangle", "a music box"]

# Partial tables: (ratio to f0, amplitude).
BARS: dict[str, tuple[tuple[float, float], ...]] = {
    "glock": ((1.0, 1.0), (2.756, 0.35), (5.404, 0.12), (8.933, 0.05)),
    "celesta": ((1.0, 1.0), (2.0, 0.05), (3.98, 0.22), (6.9, 0.05)),
    "metal": ((1.0, 1.0), (3.9, 0.25), (9.9, 0.08)),
    "marimba": ((1.0, 1.0), (3.92, 0.3), (9.8, 0.08)),
    "tine": ((1.0, 1.0), (5.8, 0.12), (6.27, 0.2)),
}

# key: (range, default)
_TABLE: dict[str, tuple[Any, Any]] = {
    "style": (["note", "pluck"], "note"),
    "bar": (["glock", "celesta", "metal", "marimba", "tine"], "glock"),
    "f0": ((150.0, 3200.0, "log"), 1046.5),
    "decay": ((0.05, 1.6, "log"), 0.6),
    "slope": ((0.3, 1.6), 0.9),
    "hard": ((0.0, 1.0), 0.5),
    "strike": ((-60.0, -6.0), -26.0),
    "beat": ((0.0, 6.0), 1.2),
    "onset": ((0.0005, 0.06, "log"), 0.004),
    "gate": ((0.05, 1.0, "log"), 0.2),
    "gate_tau": ((0.005, 0.08, "log"), 0.03),
    "tilt": ((0.6, 2.5), 1.4),
    "pos": ((0.05, 0.5), 0.18),
    "contact": ((0.0003, 0.01, "log"), 0.003),
    "body": ((-60.0, 0.0), -14.0),
    "body_f": ((90.0, 400.0, "log"), 180.0),
    "hp": ((60.0, 800.0, "log"), 300.0),
    "lp": ((3000.0, 16000.0, "log"), 12000.0),
    "width": ((0.0, 1.0), 0.5),
    "pan": ((-1.0, 1.0), 0.0),
    "room": ((0.0, 0.35), 0.1),
    "rt": ((0.1, 0.8, "log"), 0.3),
    "oct": ((-60.0, 0.0), -60.0),
    "oct_ratio": ([0.5, 2.0], 0.5),
    "jit": ((0.0, 1.0), 0.5),
}
SPACE: dict[str, Any] = {k: v[0] for k, v in _TABLE.items()}
DEFAULTS: dict[str, Any] = {k: v[1] for k, v in _TABLE.items()}

FINE: list[str] = [
    "decay", "slope", "hard", "strike", "beat", "onset", "gate", "gate_tau", "tilt", "pos", "contact", "body",
    "body_f", "hp", "lp", "width", "room", "rt", "oct",
]

_ROWS: dict[str, tuple] = {
    "style": ("note", "pluck", "note", "note"),
    "bar": ("glock", "glock", "celesta", "marimba"),
    "f0": (1274, 284.2, 784, 392),
    "decay": (0.5027, 0.1234, 0.7148, 1.6),
    "slope": (0.8703, 0.3789, 0.4249, 0.3),
    "hard": (0.6343, 0.1259, 0.5432, 0.45),
    "strike": (-14.89, -49.8, -49.98, -16.89),
    "beat": (4.807, 2.786, 1.934, 1.747),
    "onset": (0.00162, 0.06, 0.00548, 0.004282),
    "gate": (0.08478, 0.1256, 0.08034, 0.06156),
    "gate_tau": (0.0422, 0.01994, 0.04521, 0.03),
    "tilt": (0.6, 0.7913, 1.4, 1.4),
    "pos": (0.4314, 0.07761, 0.18, 0.18),
    "contact": (0.00436, 0.00563, 0.003, 0.003),
    "body": (-21.52, -40.86, -14, -14),
    "body_f": (360.3, 125.1, 180, 180),
    "hp": (194.3, 372.5, 678.6, 120),
    "lp": (6499, 4270, 8593, 16000),
    "width": (0.3488, 0.08427, 0.8211, 0.3335),
    "pan": (0, 0, 0, 0),
    "room": (0.2474, 0.07512, 0.1831, 0.1626),
    "rt": (0.4467, 0.6068, 0.4, 0.6887),
    "oct": (-2.483, -60, -60, -60),
    "oct_ratio": (0.5, 0.5, 0.5, 0.5),
    "jit": (0.4495, 0.02373, 0.4, 0.4),
}
FAMILY_NAMES: dict[str, list[str]] = {"mallet": ["glock", "pluck", "celesta", "marimba"]}
FAMILY: dict[str, list[dict]] = {
    "mallet": [{**DEFAULTS, **{k: row[i] for k, row in _ROWS.items()}} for i in range(4)],
}

# T15 targets: key -> (lo, hi, scale), measured with tonal.measure.
STUDY: dict[str, tuple[float, float, float]] = {
    "dur_s": (0.28, 0.44, 0.1),
    "attack_ms": (16, 45, 12),
    "t20_ms": (38, 256, 60),
    "centroid_hz": (550, 1505, 300),
    "lo150": (0.0, 0.04, 0.05),
    "tonal": (0.55, 0.68, 0.1),
    "corr": (0.33, 0.99, 0.2),
}
STUDY_PLUCK: dict[str, tuple[float, float, float]] = {
    "dur_s": (0.25, 0.35, 0.08),
    "attack_ms": (15, 131, 20),
    "t20_ms": (104, 150, 30),
    "centroid_hz": (335, 809, 100),
    "lo150": (0.02, 0.17, 0.08),
    "tonal": (0.4, 0.62, 0.1),
    "corr": (0.95, 0.99, 0.1),
}


_FAMILY_BOX = (SPACE, DEFAULTS, FINE)


def family_space(params: dict) -> dict[str, Any]:
    """The pinned family space around 1 member: fine keys keep their full range."""
    space, defaults, fine = _FAMILY_BOX
    return tonal.family_space(space, {**defaults, **params}, fine)


def _n(seconds: float) -> int:
    return max(1, int(round(seconds * SR)))


def _on(db: float) -> bool:
    return db > -59


def damper(t: np.ndarray, gate: float, gate_tau: float) -> np.ndarray:
    """1 until the damper engages at `gate`, then an exponential decay with `gate_tau`."""
    return np.exp(-np.maximum(t - gate, 0) / max(gate_tau, 1e-4))


def _rise(t: np.ndarray, onset: float) -> np.ndarray:
    return 0.5 - 0.5 * np.cos(np.pi * np.clip(t / max(onset, 1e-4), 0, 1))


def spread(p: dict, rng: np.random.Generator) -> dict:
    """The take spread: decay, gate and hardness move with jit; the pitch never moves."""
    j = p["jit"] * rng.standard_normal(3)
    return {**p, "decay": p["decay"] * math.exp(0.08 * j[0]), "gate": p["gate"] * math.exp(0.05 * j[1]),
            "hard": float(np.clip(p["hard"] + 0.04 * j[2], 0, 1))}


def _bar(name: str) -> tuple[tuple[float, float], ...]:
    if name not in BARS:
        raise ValueError(f"mallet: unknown bar {name!r}")
    return BARS[name]


def _partial(p: dict, t: np.ndarray, f: float, q: float, a: float, rng: np.random.Generator) -> np.ndarray:
    amp = a * q ** (1.5 * (p["hard"] - 0.5))
    tau = max(p["decay"] * q ** -p["slope"], 0.003)
    b = p["beat"] * rng.uniform(0.6, 1.4)
    am = 1 - 0.3 * (0.5 - 0.5 * np.cos(2 * np.pi * b * t)) if b > 0.01 else 1.0
    return amp * np.exp(-t / tau) * am * np.sin(2 * np.pi * f * t + rng.uniform(0, 2 * np.pi))


def _note(p: dict, rng: np.random.Generator, n: int, f0: float) -> np.ndarray:
    t = np.arange(n) / SR
    y = np.zeros(n)
    for q, a in _bar(p["bar"]):
        f = f0 * q
        if f < 0.42 * SR:
            y += _partial(p, t, f, q, a, rng)
    y *= damper(t, p["gate"], p["gate_tau"]) * _rise(t, p["onset"])
    if _on(p["strike"]):
        pk = float(np.max(np.abs(y)))
        lo = 800 + 3000 * p["hard"]
        tau = 0.0006 + 0.002 * (1 - p["hard"])
        y += pk * 10 ** (p["strike"] / 20) * tonal.burst(n, rng, lo, min(4 * lo, 16000), tau,
                                                         max(0.0002, 0.3 * p["onset"]))
    return y


def _harmonic_level(p: dict, k: int, f: float) -> float:
    """String harmonic k: tilt, pluck-point comb and the finger-contact low-pass."""
    return k ** -p["tilt"] * abs(math.sin(math.pi * k * p["pos"])) / (1 + (2 * math.pi * f * p["contact"] / 6) ** 2)


def _with_body(y: np.ndarray, p: dict) -> np.ndarray:
    if not _on(p["body"]):
        return y
    bd = _body_res(y, p["body_f"])
    return y + 10 ** (p["body"] / 20) * bd / (np.max(np.abs(bd)) + 1e-12) * np.max(np.abs(y))


def _pluck(p: dict, rng: np.random.Generator, n: int, f0: float) -> np.ndarray:
    t = np.arange(n) / SR
    y = np.zeros(n)
    for k in range(1, 13):
        f = f0 * k * (1 + 0.0004 * k * k)
        if f >= 0.4 * SR:
            break
        tau = max(p["decay"] * k ** -p["slope"], 0.004)
        y += _harmonic_level(p, k, f) * np.exp(-t / tau) * np.sin(2 * np.pi * f * t + rng.uniform(0, 0.3))
    y *= damper(t, p["gate"], p["gate_tau"]) * _rise(t, p["onset"])
    return _with_body(y, p)


def _body_res(y: np.ndarray, f: float) -> np.ndarray:
    return dsp.svf(y, f, 3.0, "bandn")


def voice(p: dict, rng: np.random.Generator, n: int, f0: float) -> np.ndarray:
    """One played note (struck bar or plucked string), with the optional octave double."""
    make = _pluck if p["style"] == "pluck" else _note
    if p["style"] not in ("note", "pluck"):
        raise ValueError(f"mallet: unknown style {p['style']!r}")
    y = make(p, rng, n, f0)
    if _on(p["oct"]):
        y = y + 10 ** (p["oct"] / 20) * make(p, rng, n, f0 * p["oct_ratio"])
    return y


def length_of(p: dict) -> float:
    """Note seconds: the damper end, the free ring end, or 2 s, whichever is first."""
    return min(2.0, p["gate"] + 7 * p["gate_tau"] + 0.02, max(p["onset"] + 7 * p["decay"], 0.06) + 0.02)


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """One note: stereo float64 after the T13 finish."""
    p = spread({**DEFAULTS, **p}, rng)
    n = _n(length_of(p))
    y = voice(p, rng, n, p["f0"])
    return tonal.finish(y, rng, hp=p["hp"], lp=p["lp"], room_wet=p["room"], rt60=p["rt"], width=p["width"],
                        cut=500, pan=p["pan"], size=0.3)


# ---- run builder ----

SCALES: dict[str, tuple[int, ...]] = {
    "penta": (0, 2, 4, 7, 9),
    "major": (0, 2, 4, 5, 7, 9, 11),
    "lydian": (0, 2, 4, 6, 7, 9, 11),
    "sus": (0, 2, 5, 7, 10),
}
_RUN_TABLE: dict[str, tuple[Any, Any]] = {
    "notes": ((2, 8, "int"), 4),
    "step": ((0.05, 0.18), 0.11),
    "swing": ((0.0, 0.3), 0.0),
    "scale": (["penta", "major", "lydian", "sus"], "penta"),
    "pattern": (["up", "down", "arc"], "up"),
    "span": ((1, 3, "int"), 1),
    "spread": ((0.0, 1.0), 0.6),
    "accent": ((-6.0, 6.0), 2.0),
    "last_gate": ((1.0, 4.0), 2.0),
    "ramp": ((-6.0, 6.0), 0.0),
}
RUN_SPACE: dict[str, Any] = {**SPACE, **{k: v[0] for k, v in _RUN_TABLE.items()}}
RUN_DEFAULTS: dict[str, Any] = {**DEFAULTS, **{k: v[1] for k, v in _RUN_TABLE.items()}}


def _arc(idx: list[int]) -> list[int]:
    k = len(idx)
    h = (k + 1) // 2
    return idx[:h] + idx[: k - h][::-1]


_CONTOURS = {"up": list, "down": lambda idx: idx[::-1], "arc": _arc}


def _degrees(k: int, span: int, pattern: str) -> list[int]:
    if pattern not in _CONTOURS:
        raise ValueError(f"mallet: unknown pattern {pattern!r}")
    return _CONTOURS[pattern]([i * span for i in range(k)])


def run_semitones(p: dict) -> list[int]:
    """The semitone offsets of a run from f0."""
    if p["scale"] not in SCALES:
        raise ValueError(f"mallet: unknown scale {p['scale']!r}")
    scale = SCALES[p["scale"]]
    m = len(scale)
    return [12 * (i // m) + scale[i % m] for i in _degrees(int(p["notes"]), int(p["span"]), p["pattern"])]


def run_freqs(p: dict) -> list[float]:
    """The note frequencies of a run (Hz)."""
    p = {**RUN_DEFAULTS, **p}
    return [p["f0"] * 2 ** (s / 12) for s in run_semitones(p)]


def _onsets(p: dict, k: int) -> list[float]:
    return [i * p["step"] * ((1 + p["swing"]) if i % 2 else 1) for i in range(k)]


def _run_note(p: dict, last: dict, i: int, k: int, f: float, rng: np.random.Generator) -> tuple[float, np.ndarray]:
    final = i == k - 1
    q = spread(last if final else p, rng)
    g = 10 ** ((p["ramp"] * i / max(1, k - 1) + (p["accent"] if final else 0)) / 20)
    side = 1 if i % 2 else -1
    pan = p["pan"] + p["spread"] * (side * (0.4 + 0.6 * i / max(1, k - 1)))
    y = voice(q, rng, _n(length_of(q)), f)
    st = tonal.finish(y, rng, hp=q["hp"], lp=q["lp"], room_wet=0, width=q["width"], cut=500,
                      pan=float(np.clip(pan, -1, 1)), end_db=-70, fade=0.004)
    return g, st


def _tail_fade(x: np.ndarray, seconds: float) -> np.ndarray:
    k = min(len(x), _n(seconds))
    x = x.copy()
    x[len(x) - k:] *= (np.cos(np.linspace(0, np.pi / 2, k)) ** 2)[:, None]
    return x


def render_run(p: dict, rng: np.random.Generator) -> np.ndarray:
    """A run of gated notes on a scale: stereo float64."""
    p = {**RUN_DEFAULTS, **p}
    fs = run_freqs(p)
    k = len(fs)
    last = {**p, "gate": p["gate"] * p["last_gate"]}
    ts = _onsets(p, k)
    n = _n(ts[-1] + length_of(last) + 0.05)
    out = np.zeros((n, 2))
    for i, f in enumerate(fs):
        g, st = _run_note(p, last, i, k, f, rng)
        at = _n(ts[i]) if ts[i] > 0 else 0
        m = min(len(st), n - at)
        if m > 0:
            out[at:at + m] += g * st[:m]
    out = tonal.room(out, rng, p["room"], p["rt"], 0.3, 0.3 + p["width"])
    out = dsp.trim_tail(out, -60.0, 0.01)
    return _tail_fade(out, 0.006)


RUN = types.SimpleNamespace(
    MODULE=__name__, SPACE=RUN_SPACE, DEFAULTS=dict(RUN_DEFAULTS), render=render_run,
    TARGET="a quick run of chime notes",
    ALTS=["a music box melody", "a harp glissando", "a phone ringtone", "a xylophone scale"],
)

_FLOURISH = {
    "f0": 802.3, "decay": 0.06476, "slope": 0.4859, "hard": 0.3246, "strike": -51.63, "beat": 2.417,
    "onset": 0.00098, "gate": 0.2162, "gate_tau": 0.04794, "tilt": 0.7552, "pos": 0.3692, "contact": 0.00257,
    "body": -19.32, "body_f": 400, "hp": 142.8, "lp": 3000, "width": 0.01723, "room": 0.09026, "rt": 0.2059,
    "jit": 0.5908, "notes": 3, "step": 0.04412, "swing": 0.06969, "scale": "lydian", "pattern": "arc", "span": 2,
    "spread": 0.1809, "accent": -4.474, "last_gate": 3.365,
}
_UP4 = {"f0": 1046.5, "notes": 4, "step": 0.11, "span": 2, "gate": 0.12, "gate_tau": 0.03}
RUN_NAMES: dict[str, list[str]] = {"chime_run": ["flourish", "up4"]}
RUNS: dict[str, list[dict]] = {"chime_run": [{**RUN_DEFAULTS, **_FLOURISH}, {**RUN_DEFAULTS, **_UP4}]}
