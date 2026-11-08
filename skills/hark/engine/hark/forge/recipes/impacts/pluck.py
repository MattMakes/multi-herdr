"""Recipe pluck: one plucked kalimba tine over a wooden box, modelled from physics.

Spec: docs/specs/hark/recipes-impacts-tuned.md section 3.
Decisions (ARCHITECTURE.md 15): the render merges DEFAULTS (Q-PLUCK-1), and the 2nd bending mode is skipped at or
above 0.45 x 48000 Hz, as the 3rd and 4th are (Q-PLUCK-2).
"""

from __future__ import annotations

import math
import os
from typing import Any

import numpy as np

from hark.forge.engine import dsp

SR = 48000
LEN = 3.0
NYQ_GUARD = 0.45 * SR

TARGET = "a kalimba note, a thumb piano pluck"
ALTS = ["a music box", "a xylophone note", "a glockenspiel", "an electric piano note", "a synthesizer pluck"]


def _ref_root() -> str:
    root = os.environ.get("HARK_REFS", "refs").replace("\\", "/")
    return root.rstrip("/") or root


REFS: list[str] = [f"{_ref_root()}/kalimba/kalimba_{i:02d}.ogg" for i in range(1, 6)]
REF_F0: tuple[float, ...] = (392.0, 440.0, 493.9, 587.3, 659.3)

_MAJOR = (0, 2, 4, 5, 7, 9, 11)
TINES: list[float] = [261.63 * 2 ** ((12 * o + s) / 12) for o in range(3) for s in _MAJOR][:17]

# key: (range, default)
_TABLE: dict[str, tuple[Any, float]] = {
    "p2_ratio": ((5.0, 7.0), 5.0),
    "p2_lvl": ((0.02, 0.6, "log"), 0.386),
    "p3_lvl": ((0.0, 0.25), 0.2038),
    "tau1": ((0.35, 2.4, "log"), 0.3529),
    "tau_exp": ((0.5, 1.4), 1.309),
    "beat_hz": ((0.2, 4.0, "log"), 2.759),
    "beat_lvl": ((0.0, 0.6), 0.01985),
    "h2_lvl": ((0.0, 0.25), 0.1751),
    "thumb_ms": ((0.1, 4.0, "log"), 0.3572),
    "zing_lvl": ((0.0, 0.4), 0.09041),
    "zing_f": ((2500.0, 12000.0, "log"), 6908.0),
    "zing_tau": ((0.004, 0.06, "log"), 0.006356),
    "skin_lvl": ((0.0, 0.25), 0.2027),
    "skin_f": ((1500.0, 9000.0, "log"), 1500.0),
    "helm_f": ((160.0, 420.0, "log"), 197.2),
    "helm_tau": ((0.008, 0.06, "log"), 0.01108),
    "box_lvl": ((0.0, 0.8), 0.1924),
    "box_lo": ((150.0, 600.0, "log"), 545.6),
    "box_tau": ((0.003, 0.03, "log"), 0.01542),
    "knock_lvl": ((0.0, 0.8), 0.5643),
    "knock_f": ((40.0, 160.0, "log"), 77.94),
    "knock_tau": ((0.008, 0.08, "log"), 0.03731),
    "symp_lvl": ((0.0, 0.25), 0.02239),
    "symp_rise": ((0.005, 0.08, "log"), 0.07959),
    "room_wet": ((0.0, 0.35), 0.2094),
    "rt60": ((0.15, 0.9, "log"), 0.4881),
    "width": ((0.0, 0.5), 0.0),
    "air_db": ((-9.0, 0.0), -7.11),
    "floor_db": ((-88.0, -58.0), -64.71),
    "rumble": ((0.0, 0.8), 0.3376),
}
SPACE: dict[str, Any] = {k: v[0] for k, v in _TABLE.items()}
DEFAULTS: dict[str, Any] = {k: v[1] for k, v in _TABLE.items()}

_LETTERS = {"C": 0, "D": 2, "E": 4, "F": 5, "G": 7, "A": 9, "B": 11}


def note(name: str, a4: float = 440.0) -> float:
    """A note name such as 'C5', 'F#4' or 'Bb3' to Hz."""
    if not name or name[0].upper() not in _LETTERS:
        raise ValueError(f"pluck: unknown note name {name!r}")
    s, i = _LETTERS[name[0].upper()], 1
    while i < len(name) and name[i] in "#b":
        s += 1 if name[i] == "#" else -1
        i += 1
    try:
        octave = int(name[i:])
    except ValueError:
        raise ValueError(f"pluck: note name {name!r} has no octave") from None
    return a4 * 2 ** ((s + 12 * (octave + 1) - 69) / 12)


def _n(seconds: float) -> int:
    return max(1, int(round(seconds * SR)))


def _peak(x: np.ndarray) -> float:
    return float(np.max(np.abs(x))) + 1e-12


def _at_start(y: np.ndarray, n: int) -> np.ndarray:
    out = np.zeros(n)
    out[: len(y)] = y[:n]
    return out


def mode(t: np.ndarray, f: float, tau: float, a: float, attack: float, rng: np.random.Generator) -> np.ndarray:
    """One decaying tine mode, released from rest, with a raised-cosine attack."""
    ph = rng.uniform(0, 2 * np.pi)
    y = a * np.exp(-t / tau) * np.sin(2 * np.pi * f * t + 0.15 * ph)
    k = max(2, _n(attack))
    if k < len(y):
        y[:k] *= 0.5 - 0.5 * np.cos(np.linspace(0, np.pi, k))
    return y


def _tine_modes(p: dict, f0: float, tau1: float, lp: Any) -> list[tuple[float, float, float]]:
    """(frequency, tau, level) of the bending modes 2-4 that pass the guard."""
    f2 = p["p2_ratio"] * f0
    out = []
    if f2 < NYQ_GUARD:
        out.append((f2, p["p2_lvl"]))
    if p["p3_lvl"] > 0.005:
        out += [(f, lvl) for f, lvl in ((2.8 * f2, p["p3_lvl"]), (5.5 * f2, 0.5 * p["p3_lvl"])) if f < NYQ_GUARD]
    return [(f, tau1 * (f / f0) ** -p["tau_exp"], lvl * lp(f) / lp(f0)) for f, lvl in out]


def tine(p: dict, f0: float, n: int, rng: np.random.Generator) -> np.ndarray:
    """The tine: fundamental, polarisation beat, bending modes and the nonlinear 2nd harmonic."""
    t = np.arange(n) / SR
    big_t = p["thumb_ms"] / 1000

    def lp(f: float) -> float:
        return 1 / (1 + 0.5 * f * big_t)

    tau1 = p["tau1"] * (f0 / 523.25) ** -0.5
    att = max(0.35 * big_t, 0.0002)
    y = mode(t, f0, tau1, 1.0, att, rng)
    if p["beat_lvl"] > 0.005:
        y += mode(t, f0 + p["beat_hz"], 0.85 * tau1, p["beat_lvl"], att, rng)
    for f, tau, lvl in _tine_modes(p, f0, tau1, lp):
        y += mode(t, f, tau, lvl, att, rng)
    if p["h2_lvl"] > 0.005:
        y += mode(t, 2 * f0, tau1 / 2, p["h2_lvl"], att, rng)
    return y


def box_ir(p: dict, rng: np.random.Generator) -> np.ndarray:
    """The wooden box: a Helmholtz resonance plus 18 short plate modes."""
    n = _n(0.25)
    t = np.arange(n) / SR
    ir = np.zeros(n)
    ir[0] = 1.0
    hf = p["helm_f"]
    ir += 0.9 * np.sin(2 * np.pi * hf * t) * np.exp(-t / p["helm_tau"]) * 2 * np.pi * hf / SR * 6
    for fi in np.exp(rng.uniform(np.log(p["box_lo"]), np.log(2500.0), 18)):
        tau = p["box_tau"] * (fi / p["box_lo"]) ** -0.5
        a = rng.uniform(0.2, 1.0)
        ir += a * np.sin(2 * np.pi * fi * t + rng.uniform(0, 6.3)) * np.exp(-t / tau) * 2 * np.pi * fi / SR * 2
    return ir


def _cents(ft: float, f0: float) -> float:
    return min(abs(1200 * math.log2(ft / (h * f0))) for h in (0.5, 1, 1.5, 2, 3, 4))


def sympathetic(p: dict, f0: float, n: int, rng: np.random.Generator) -> np.ndarray:
    """The other tines, driven through the bridge; strongest on the played note's partials."""
    y = np.zeros(n)
    if p["symp_lvl"] < 0.003:
        return y
    t = np.arange(n) / SR
    rise = 1 - np.exp(-t / p["symp_rise"])
    for ft in TINES:
        if abs(ft / f0 - 1) < 0.03:
            continue
        a = p["symp_lvl"] * (0.15 + math.exp(-(_cents(ft, f0) / 30) ** 2)) * rng.uniform(0.5, 1.0)
        tau = 1.2 * p["tau1"] * (ft / 523.25) ** -0.5
        f = ft * (1 + 0.0015 * rng.standard_normal())
        y += a * np.sin(2 * np.pi * f * t + rng.uniform(0, 6.3)) * np.exp(-t / tau) * rise
    return y


def skin(p: dict, n: int, rng: np.random.Generator) -> np.ndarray:
    """Skin or nail slip noise at the release."""
    m = _n(0.02)
    t = np.arange(m) / SR
    w = rng.standard_normal(m) * np.exp(-t / 0.003)
    w = dsp.butter(w, "band", (p["skin_f"] / 2, min(2 * p["skin_f"], NYQ_GUARD)), 2)
    return _at_start(w / _peak(w), n)


def thud(p: dict, n: int, rng: np.random.Generator) -> np.ndarray:
    """The low thud of the instrument pushed against its rest."""
    m = min(n, _n(0.4))
    t = np.arange(m) / SR
    f = p["knock_f"] * (1 + 0.05 * rng.standard_normal())
    tau = p["knock_tau"]
    y = np.sin(2 * np.pi * f * t) * np.exp(-t / tau) * (1 - np.exp(-t / 0.004))
    y += 0.5 * np.sin(2 * np.pi * 2.3 * f * t) * np.exp(-t / (0.5 * tau)) * (1 - np.exp(-t / 0.002))
    return _at_start(y / _peak(y), n)


def zing(p: dict, f0: float, n: int, rng: np.random.Generator) -> np.ndarray:
    """The bridge rattle: band noise gated at the period of the note."""
    m = min(n, _n(0.4))
    t = np.arange(m) / SR
    w = dsp.butter(rng.standard_normal(m), "band", (0.5 * p["zing_f"], min(1.8 * p["zing_f"], NYQ_GUARD)), 2)
    gate = np.maximum(0, np.sin(2 * np.pi * f0 * t)) ** 4
    y = w * (0.35 + gate) * np.exp(-t / p["zing_tau"])
    return _at_start(y / _peak(y), n)


def room_tone(n: int, rumble: float, rng: np.random.Generator) -> np.ndarray:
    """Pink room tone with a share of low rumble."""
    w = dsp.noise(n / SR, rng, "pink")[:n]
    lo = dsp.butter(dsp.noise(n / SR, rng, "brown")[:n], "low", 120.0, 2)
    lo = lo / (np.sqrt(np.mean(lo ** 2)) + 1e-12)
    return (1 - rumble) * w + 2 * rumble * lo


def _pitch(p: dict, rng: np.random.Generator) -> float:
    f0 = p.get("f0")
    if f0:
        return float(f0)
    return float(REF_F0[int(rng.integers(len(REF_F0)))])


def _dry(p: dict, f0: float, m: int, rng: np.random.Generator) -> np.ndarray:
    y = tine(p, f0, m, rng)
    pk = _peak(y)
    body = np.convolve(y, box_ir(p, rng))[:m]
    body = body * pk / _peak(body)
    x = (1 - 0.5 * p["box_lvl"]) * y + p["box_lvl"] * body
    x += pk * sympathetic(p, f0, m, rng)
    x += p["skin_lvl"] * pk * skin(p, m, rng)
    x += p.get("knock_lvl", 0.0) * pk * thud(p, m, rng)
    if p.get("zing_lvl", 0.0) > 0.003:
        x += p["zing_lvl"] * pk * zing(p, f0, m, rng)
    return x


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """One take: stereo float64, up to LEN s; the level is left to the caller."""
    p = {**DEFAULTS, **p}
    f0 = _pitch(p, rng)
    n = _n(LEN)
    pre = _n(0.004)
    x = np.concatenate([np.zeros(pre), _dry(p, f0, n - pre, rng)])
    x = dsp.stereo(x, rng, width=p["width"], pan=0.0)
    x = dsp.room(x, rng, rt60=p["rt60"], size=0.3, damp=0.6, wet=p["room_wet"])
    x = dsp.mic(x, rng, low_cut=20.0, air_db=p["air_db"], floor_db=-140.0)[:n]
    pk = _peak(x)
    floor = pk * 10 ** (p["floor_db"] / 20)
    for c in range(x.shape[1]):
        x[:, c] += room_tone(n, p.get("rumble", 0.0), rng) * floor
    x = dsp.trim_tail(x, p["floor_db"] - 2, 0.05)
    return dsp.fade(x, 0.0005, min(0.4, 0.3 * len(x) / SR))
