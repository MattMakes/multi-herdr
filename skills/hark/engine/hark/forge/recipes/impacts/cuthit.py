"""Recipe cuthit: a short sub hit with a kick on top, placed on an edit.

Spec: docs/specs/hark/recipes-impacts-hits.md section 1.
"""

from __future__ import annotations

import types
from typing import Any

import numpy as np

from hark.forge.engine import dsp

SR = 48000
PEAK = 0.89

TARGET = "a short deep sub bass hit with a kick drum"
TARGETS = {"cut_hit": TARGET}
ALTS = ["an explosion", "a door slam", "thunder", "a low hum"]
REFSET = "cut_hit"

SPACE: dict[str, Any] = {
    "length": (0.12, 1.8),
    "fade": (0.004, 0.25, "log"),
    "sub_f0": (50.0, 260.0, "log"),
    "sub_f1": (36.0, 130.0, "log"),
    "sub_glide": (0.008, 0.4, "log"),
    "sub_attack": (0.002, 0.35, "log"),
    "sub_t20": (0.03, 0.8, "log"),
    "sub_drive": (0.0, 3.0),
    "sub_phase": (0.0, 1.0),
    "kick": (0.0, 1.5),
    "kick_f0": (120.0, 700.0, "log"),
    "kick_f1": (40.0, 130.0, "log"),
    "kick_tau": (0.003, 0.06, "log"),
    "kick_decay": (0.015, 0.25, "log"),
    "click": (0.0, 1.0),
    "click_f": (1200.0, 9000.0, "log"),
    "click_ms": (1.0, 15.0, "log"),
    "punch": (0.0, 1.2),
    "punch_f": (140.0, 1000.0, "log"),
    "punch_decay": (0.008, 0.15, "log"),
    "rumble": (0.0, 1.2),
    "rumble_t20": (0.1, 1.5, "log"),
    "grain_rate": (12.0, 70.0, "log"),
    "grain_tick": (0.0, 1.0),
    "crackle": (0.0, 1.5),
    "air": (0.0, 1.0),
    "air_f": (3000.0, 12000.0, "log"),
    "air_len": (0.03, 0.8, "log"),
    "width": (0.0, 1.0),
    "room": (0.0, 0.4),
}
_KEYS = list(SPACE)

# One column per member, values in SPACE key order.
_COLUMNS: dict[str, list[float]] = {
    "sub": [0.2858, 0.1949, 224.8, 45.69, 0.04827, 0.01613, 0.04364, 0.6188, 0.0, 0.6151, 174.9, 92.57,
            0.008307, 0.2089, 0.1667, 1226, 2.506, 1.069, 374.4, 0.1464, 0.0, 0.1565, 12.92, 0.002876, 0.0,
            0.0, 9115, 0.03, 0.3875, 0.05584],
    "kick": [1.022, 0.0589, 176.6, 37.04, 0.1106, 0.007655, 0.05602, 0.3078, 0.0, 1.025, 363.2, 108.2,
             0.009914, 0.03626, 0.1688, 2440, 5.914, 1.077, 437.1, 0.04711, 0.0, 0.1437, 29.02, 0.0, 0.0,
             0.0, 6018, 0.07952, 0.0, 0.01595],
    "punch": [0.2823, 0.01307, 144.2, 47.8, 0.06248, 0.06125, 0.03285, 2.878, 0.09153, 0.6973, 284.4, 84.94,
              0.0141, 0.04286, 0.4873, 9000, 5.467, 0.9001, 1000, 0.07649, 0.0, 0.1443, 15.95, 0.04938, 0.0,
              0.0, 5858, 0.1071, 0.346, 0.1467],
    "rumble": [1.4, 0.04147, 125.0, 40.27, 0.15, 0.07542, 0.09859, 0.863, 1.0, 0.3807, 197.6, 69.6,
               0.004458, 0.0623, 0.0, 2568, 2.467, 0.03031, 140, 0.01199, 1.2, 1.5, 21.45, 0.6988, 0.0,
               0.3205, 3809, 0.166, 0.6293, 0.3665],
    "air": [0.5493, 0.2087, 203.3, 50.96, 0.02, 0.05396, 0.03, 3.0, 1.0, 1.084, 193.6, 50.09,
            0.006564, 0.03479, 0.4183, 3710, 1.241, 0.1212, 190.7, 0.1313, 0.9522, 0.4372, 14.47, 0.4375,
            0.7462, 0.824, 10810, 0.6526, 1.0, 0.4],
}
FAMILIES: dict[str, list[tuple[str, dict]]] = {
    "cut_hit": [(name, dict(zip(_KEYS, values))) for name, values in _COLUMNS.items()],
}
DEFAULTS: dict[str, Any] = dict(FAMILIES["cut_hit"][0][1])


def space_for(cat: str = "cut_hit") -> dict[str, Any]:
    """A copy of the full space (every category searches all keys)."""
    return dict(SPACE)


def _member(cat: str, preset: int | str) -> tuple[str, dict]:
    members = FAMILIES[cat]
    if isinstance(preset, str):
        found = dict(members)
        if preset not in found:
            raise KeyError(f"cuthit: no member {preset!r} in {cat!r}")
        return preset, dict(found[preset])
    name, params = members[preset]
    return name, dict(params)


def recipe_for(cat: str = "cut_hit", preset: int | str = 0) -> types.SimpleNamespace:
    """A recipe object for 1 family member (index or name)."""
    name, params = _member(cat, preset)
    return types.SimpleNamespace(
        MODULE=__name__, SPACE=space_for(cat), DEFAULTS=params, render=render, TARGET=TARGETS[cat],
        ALTS=list(ALTS), REFSET="cut_hit", PRESET=name,
    )


def _peak1(x: np.ndarray) -> np.ndarray:
    return x / (np.max(np.abs(x)) + 1e-12)


def _band(x: np.ndarray, lo: float, hi: float) -> np.ndarray:
    return dsp.butter(x, "band", (lo, hi), 2)


def bloom(t: np.ndarray, attack: float, t20: float) -> np.ndarray:
    """Raised-cosine rise to the peak, then -20 dB every t20 seconds."""
    return np.where(t < attack, 0.5 - 0.5 * np.cos(np.pi * np.minimum(t / attack, 1.0)),
                    10 ** (-(t - attack) / t20))


def _sub(p: dict, t: np.ndarray, rng: np.random.Generator) -> tuple[np.ndarray, np.ndarray]:
    f = p["sub_f1"] + (p["sub_f0"] - p["sub_f1"]) * np.exp(-t / p["sub_glide"])
    phase = 2 * np.pi * np.cumsum(f) / SR + rng.uniform(0, 2 * np.pi)
    env = bloom(t, p["sub_attack"], p["sub_t20"])
    g = 1 + 2 * p["sub_drive"]
    sides = []
    for offset in (0.0, 0.5 * np.pi * p["sub_phase"]):
        s = np.tanh(g * np.sin(phase + offset)) / np.tanh(g)
        sides.append(dsp.butter(s, "low", 900.0, 2) * env)
    return sides[0], sides[1]


def _kick(p: dict, t: np.ndarray, rng: np.random.Generator) -> np.ndarray:
    f = p["kick_f1"] + (p["kick_f0"] - p["kick_f1"]) * np.exp(-t / p["kick_tau"])
    phase = 2 * np.pi * np.cumsum(f) / SR + rng.uniform(-0.2, 0.2)
    return np.sin(phase) * np.exp(-t / p["kick_decay"]) * (1 - np.exp(-t / 0.0004))


def _click(p: dict, n: int, rng: np.random.Generator) -> np.ndarray:
    length = min(n, max(16, dsp.n_of(p["click_ms"] / 1000)))
    u = np.arange(length) / length
    w = _peak1(_band(rng.standard_normal(length) * (1 - u) ** 3, 0.5 * p["click_f"], min(2.2 * p["click_f"], 19000)))
    k = min(n, dsp.n_of(0.012))
    tk = np.arange(k) / SR
    ping = np.sin(2 * np.pi * p["click_f"] * tk) * np.exp(-tk / 0.003) * (1 - np.exp(-tk / 0.0002))
    y = np.zeros(n)
    y[:length] += w
    y[:k] += 0.5 * ping
    return _peak1(y)


def _punch(p: dict, t: np.ndarray, rng: np.random.Generator) -> np.ndarray:
    w = _band(rng.standard_normal(len(t)), p["punch_f"] / 1.7, min(1.7 * p["punch_f"], 19000))
    return _peak1(w * np.exp(-t / p["punch_decay"]) * (1 - np.exp(-t / 0.0008)))


def _air(p: dict, t: np.ndarray, rng: np.random.Generator) -> tuple[np.ndarray, np.ndarray]:
    env = (1 - np.exp(-t / 0.005)) * np.exp(-t / p["air_len"])
    out = []
    for _ in range(2):
        w = dsp.butter(rng.standard_normal(len(t)), "high", p["air_f"], 4)
        out.append(dsp.butter(w, "low", min(2.5 * p["air_f"], 19000), 2) * env)
    return out[0], out[1]


def _add_at(dst: np.ndarray, x: np.ndarray, at: int) -> None:
    m = min(len(x), len(dst) - at)
    if m > 0:
        dst[at:at + m] += x[:m]


def _brown_grain(m: int, rng: np.random.Generator) -> np.ndarray:
    b = np.cumsum(rng.standard_normal(m))
    return _band(b - b.mean(), 35.0, 220.0) * np.hanning(m)


def _tick(k: int, rng: np.random.Generator) -> np.ndarray:
    tt = np.arange(k) / SR
    w = _band(rng.standard_normal(k), 800.0, 5000.0)
    return w * np.sin(0.5 * np.pi * np.minimum(tt / 0.0025, 1.0)) ** 2 * np.exp(-tt / 0.003)


def _crackle(k: int, rng: np.random.Generator) -> np.ndarray:
    return dsp.butter(rng.standard_normal(k), "high", 500.0, 2) * np.hanning(k)


def _grain_extras(y: np.ndarray, at: int, a: float, g: float, p: dict, rng: np.random.Generator) -> None:
    left = len(y) - at
    if p["grain_tick"] > 0.01:
        k = min(dsp.n_of(0.014), left)
        if k > 8:
            _add_at(y, 0.25 * p["grain_tick"] * a * _peak1(_tick(k, rng)), at)
    if p["crackle"] > 0.01:
        k = min(dsp.n_of(rng.uniform(0.001, 0.003)), left)
        if k > 8:
            _add_at(y, p["crackle"] * a * np.exp(0.5 * g) * _peak1(_crackle(k, rng)), at)


def _rumble_channel(p: dict, n: int, t: np.ndarray, rng: np.random.Generator) -> np.ndarray:
    y = np.zeros(n)
    span = n / SR
    tg = 0.01
    while tg < span:
        u = tg / span
        m = dsp.n_of(rng.uniform(0.025, 0.06))
        grain = _brown_grain(m, rng)
        g = rng.standard_normal()
        a = 10 ** (-tg / p["rumble_t20"]) * np.exp(min(0.35 * g, 0.3))
        at = dsp.n_of(tg)
        _add_at(y, a * _peak1(grain), at)
        if at < n:
            _grain_extras(y, at, a, g, p, rng)
        tg += rng.exponential(1.0 / (p["grain_rate"] * (1 - 0.5 * u)))
    if p["rumble"] > 0.01:
        b = np.cumsum(rng.standard_normal(n))
        y += _peak1(_band(b - b.mean(), 35.0, 220.0)) * 0.06 * 10 ** (-t / p["rumble_t20"])
    return y


def end_cut(y: np.ndarray, end_db: float = -60.0) -> np.ndarray:
    """Cut 30 ms after the last 5 ms frame within end_db of the loudest frame."""
    m = y.mean(axis=1) if y.ndim == 2 else y
    frames = len(m) // 240
    if frames < 4:
        return y
    rms = np.sqrt(np.mean(m[: frames * 240].reshape(frames, 240) ** 2, axis=1))
    db = 20 * np.log10(rms + 1e-12)
    last = int(np.nonzero(db >= db.max() + end_db)[0][-1])
    return y[: min(len(y), (last + 1) * 240 + dsp.n_of(0.03))]


def _room_top(p: dict, top: np.ndarray, n: int, rng: np.random.Generator) -> np.ndarray:
    if p["room"] > 0.01:
        return dsp.room(top, rng, rt60=0.3, size=0.35, damp=0.7, wet=p["room"])[:n]
    dsp.room_ir(rng, rt60=0.3, size=0.35, damp=0.7)
    return top


def _width_top(top: np.ndarray, w: float, rk: float) -> tuple[np.ndarray, np.ndarray]:
    n = len(top)
    hi = dsp.butter(top, "high", 150.0, 2)
    lo = top - hi
    d = dsp.n_of(0.0007)
    side = np.zeros(n)
    if d < n:
        side[d:] = hi[: n - d]
    side = dsp.butter(side, "high", 150.0, 2)
    return lo + hi, lo + rk * hi + w * side


def _joint_peak(a: np.ndarray, b: np.ndarray) -> float:
    return float(max(np.max(np.abs(a)), np.max(np.abs(b)))) + 1e-12


def _draw(p: dict, n: int, t: np.ndarray, rng: np.random.Generator) -> dict:
    """Every layer, in the fixed draw order, whatever its level (so a seed is 1 take)."""
    s = {}
    s["sub"] = _sub(p, t, rng)
    s["kick"] = _kick(p, t, rng)
    s["click"] = _click(p, n, rng)
    s["punch"] = _punch(p, t, rng)
    s["air"] = _air(p, t, rng)
    s["rumble"] = (_rumble_channel(p, n, t, rng), _rumble_channel(p, n, t, rng))
    return s


def _pair(left: np.ndarray, right: np.ndarray, rk: float, w: float) -> tuple[np.ndarray, np.ndarray, float]:
    """Mix the right channel toward the left by width; return both and their joint peak."""
    right = rk * left + w * right
    return left, right, _joint_peak(left, right)


def _mix(p: dict, s: dict, n: int, rng: np.random.Generator) -> np.ndarray:
    top = 0.6 * p["kick"] * s["kick"] + 0.5 * p["click"] * s["click"] + 0.6 * p["punch"] * s["punch"]
    top = _room_top(p, top, n, rng)
    w = p["width"]
    rk = np.sqrt(max(0.0, 1 - w * w))
    top_l, top_r = _width_top(top, w, rk)
    rum_l, rum_r, rl = _pair(*s["rumble"], rk, w)
    air_l, air_r, al = _pair(*s["air"], rk, w)
    left = _channel(p, s["sub"][0] + top_l, (rum_l, rl), (air_l, al))
    right = _channel(p, s["sub"][1] + top_r, (rum_r, rl), (air_r, al))
    return np.column_stack([left, right])


def _channel(p: dict, base: np.ndarray, rum: tuple, air: tuple) -> np.ndarray:
    """One output channel: sub and top, plus the rumble and air layers over their joint peaks."""
    return base + 0.7 * p["rumble"] * rum[0] / rum[1] + 0.35 * p["air"] * air[0] / air[1]


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """One take: stereo float64, at most `length` s, peak 0.89."""
    p = {**DEFAULTS, **p}
    n = dsp.n_of(p["length"])
    t = np.arange(n) / SR
    y = _mix(p, _draw(p, n, t, rng), n, rng)
    y = end_cut(dsp.butter(y, "high", 24.0, 2), -60.0)
    y = dsp.fade(y, 0.0004, min(p["fade"], 0.9 * len(y) / SR))
    return y * PEAK / (np.max(np.abs(y)) + 1e-12)
