"""Cut hit: a short sub drop with a kick, made to sit on a picture edit.

Spec: docs/specs/hark/recipes-impacts-hits.md section 1.
"""

from __future__ import annotations

import types
from typing import Any

import numpy as np

from hark.forge.engine import dsp
from hark.forge.recipes.impacts import _common as cm

SR = cm.SR
_OUT_PEAK = 0.89

TARGET = "a short deep sub bass hit with a kick drum"
TARGETS = {"cut_hit": TARGET}
ALTS = ["an explosion", "a door slam", "thunder", "a low hum"]
REFSET = "cut_hit"

SPACE: dict[str, Any] = cm.space_table("""
    length 0.12 1.8
    fade 0.004 0.25 log
    sub_f0 50.0 260.0 log
    sub_f1 36.0 130.0 log
    sub_glide 0.008 0.4 log
    sub_attack 0.002 0.35 log
    sub_t20 0.03 0.8 log
    sub_drive 0.0 3.0
    sub_phase 0.0 1.0
    kick 0.0 1.5
    kick_f0 120.0 700.0 log
    kick_f1 40.0 130.0 log
    kick_tau 0.003 0.06 log
    kick_decay 0.015 0.25 log
    click 0.0 1.0
    click_f 1200.0 9000.0 log
    click_ms 1.0 15.0 log
    punch 0.0 1.2
    punch_f 140.0 1000.0 log
    punch_decay 0.008 0.15 log
    rumble 0.0 1.2
    rumble_t20 0.1 1.5 log
    grain_rate 12.0 70.0 log
    grain_tick 0.0 1.0
    crackle 0.0 1.5
    air 0.0 1.0
    air_f 3000.0 12000.0 log
    air_len 0.03 0.8 log
    width 0.0 1.0
    room 0.0 0.4
""")
_KEYS = list(SPACE)

# One row of values per member, in the key order of SPACE.
_COLUMNS: dict[str, str] = {
    "sub": (
        "0.2858 0.1949 224.8 45.69 0.04827 0.01613 0.04364 0.6188 0.0 0.6151 174.9 92.57 0.008307 "
        "0.2089 0.1667 1226 2.506 1.069 374.4 0.1464 0.0 0.1565 12.92 0.002876 0.0 0.0 9115 0.03 "
        "0.3875 0.05584"
    ),
    "kick": (
        "1.022 0.0589 176.6 37.04 0.1106 0.007655 0.05602 0.3078 0.0 1.025 363.2 108.2 0.009914 "
        "0.03626 0.1688 2440 5.914 1.077 437.1 0.04711 0.0 0.1437 29.02 0.0 0.0 0.0 6018 0.07952 "
        "0.0 0.01595"
    ),
    "punch": (
        "0.2823 0.01307 144.2 47.8 0.06248 0.06125 0.03285 2.878 0.09153 0.6973 284.4 84.94 "
        "0.0141 0.04286 0.4873 9000 5.467 0.9001 1000 0.07649 0.0 0.1443 15.95 0.04938 0.0 0.0 "
        "5858 0.1071 0.346 0.1467"
    ),
    "rumble": (
        "1.4 0.04147 125.0 40.27 0.15 0.07542 0.09859 0.863 1.0 0.3807 197.6 69.6 0.004458 0.0623 "
        "0.0 2568 2.467 0.03031 140 0.01199 1.2 1.5 21.45 0.6988 0.0 0.3205 3809 0.166 0.6293 "
        "0.3665"
    ),
    "air": (
        "0.5493 0.2087 203.3 50.96 0.02 0.05396 0.03 3.0 1.0 1.084 193.6 50.09 0.006564 0.03479 "
        "0.4183 3710 1.241 0.1212 190.7 0.1313 0.9522 0.4372 14.47 0.4375 0.7462 0.824 10810 "
        "0.6526 1.0 0.4"
    ),
}
FAMILIES: dict[str, list[tuple[str, dict]]] = {
    "cut_hit": [(name, dict(zip(_KEYS, map(cm.scalar, row.split())))) for name, row in _COLUMNS.items()],
}
DEFAULTS: dict[str, Any] = dict(FAMILIES["cut_hit"][0][1])


def space_for(cat: str = "cut_hit") -> dict[str, Any]:
    """Copy of SPACE; this category searches every key."""
    return dict(SPACE)


def _pick(cat: str, preset: int | str) -> tuple[str, dict]:
    members = FAMILIES[cat]
    if isinstance(preset, str):
        found = dict(members)
        if preset not in found:
            raise KeyError(f"cuthit: no member {preset!r} in {cat!r}")
        return preset, dict(found[preset])
    name, params = members[preset]
    return name, dict(params)


def recipe_for(cat: str = "cut_hit", preset: int | str = 0) -> types.SimpleNamespace:
    """Recipe object for one family member, chosen by index or by name."""
    name, params = _pick(cat, preset)
    return types.SimpleNamespace(
        MODULE=__name__, SPACE=space_for(cat), DEFAULTS=params, render=render, TARGET=TARGETS[cat],
        ALTS=list(ALTS), REFSET="cut_hit", PRESET=name,
    )


def _norm(sig: np.ndarray) -> np.ndarray:
    return sig / (np.max(np.abs(sig)) + 1e-12)


def _bp(sig: np.ndarray, lower: float, upper: float) -> np.ndarray:
    return dsp.butter(sig, "band", (lower, upper), 2)


def bloom(t: np.ndarray, attack: float, t20: float) -> np.ndarray:
    """Cosine rise until ``attack``, then a fall of 20 dB per ``t20`` seconds."""
    return np.where(t < attack, 0.5 - 0.5 * np.cos(np.pi * np.minimum(t / attack, 1.0)),
                    10 ** (-(t - attack) / t20))


def _sub_pair(cfg: dict, secs: np.ndarray, gen: np.random.Generator) -> tuple[np.ndarray, np.ndarray]:
    hz = cfg["sub_f1"] + (cfg["sub_f0"] - cfg["sub_f1"]) * np.exp(-secs / cfg["sub_glide"])
    phase = 2 * np.pi * np.cumsum(hz) / SR + gen.uniform(0, 2 * np.pi)
    shape = bloom(secs, cfg["sub_attack"], cfg["sub_t20"])
    lift = 1 + 2 * cfg["sub_drive"]
    sides = []
    for offset in (0.0, 0.5 * np.pi * cfg["sub_phase"]):
        bank = np.tanh(lift * np.sin(phase + offset)) / np.tanh(lift)
        sides.append(dsp.butter(bank, "low", 900.0, 2) * shape)
    return sides[0], sides[1]


def _kick_drop(cfg: dict, secs: np.ndarray, gen: np.random.Generator) -> np.ndarray:
    hz = cfg["kick_f1"] + (cfg["kick_f0"] - cfg["kick_f1"]) * np.exp(-secs / cfg["kick_tau"])
    phase = 2 * np.pi * np.cumsum(hz) / SR + gen.uniform(-0.2, 0.2)
    return np.sin(phase) * np.exp(-secs / cfg["kick_decay"]) * (1 - np.exp(-secs / 0.0004))


def _snap(cfg: dict, size: int, gen: np.random.Generator) -> np.ndarray:
    length = min(size, max(16, dsp.n_of(cfg["click_ms"] / 1000)))
    pos = np.arange(length) / length
    noise = _norm(_bp(gen.standard_normal(length) * (1 - pos) ** 3, 0.5 * cfg["click_f"], min(2.2 * cfg["click_f"], 19000)))
    span = min(size, dsp.n_of(0.012))
    ptime = np.arange(span) / SR
    ping = np.sin(2 * np.pi * cfg["click_f"] * ptime) * np.exp(-ptime / 0.003) * (1 - np.exp(-ptime / 0.0002))
    out = np.zeros(size)
    out[:length] += noise
    out[:span] += 0.5 * ping
    return _norm(out)


def _mid_punch(cfg: dict, secs: np.ndarray, gen: np.random.Generator) -> np.ndarray:
    noise = _bp(gen.standard_normal(len(secs)), cfg["punch_f"] / 1.7, min(1.7 * cfg["punch_f"], 19000))
    return _norm(noise * np.exp(-secs / cfg["punch_decay"]) * (1 - np.exp(-secs / 0.0008)))


def _air_pair(cfg: dict, secs: np.ndarray, gen: np.random.Generator) -> tuple[np.ndarray, np.ndarray]:
    shape = (1 - np.exp(-secs / 0.005)) * np.exp(-secs / cfg["air_len"])
    out = []
    for _ in range(2):
        noise = dsp.butter(gen.standard_normal(len(secs)), "high", cfg["air_f"], 4)
        out.append(dsp.butter(noise, "low", min(2.5 * cfg["air_f"], 19000), 2) * shape)
    return out[0], out[1]


def _mix_at(dst: np.ndarray, sig: np.ndarray, at: int) -> None:
    piece = min(len(sig), len(dst) - at)
    if piece > 0:
        dst[at:at + piece] += sig[:piece]


def _low_grain(piece: int, gen: np.random.Generator) -> np.ndarray:
    walk = np.cumsum(gen.standard_normal(piece))
    return _bp(walk - walk.mean(), 35.0, 220.0) * np.hanning(piece)


def _grain_click(span: int, gen: np.random.Generator) -> np.ndarray:
    tt = np.arange(span) / SR
    noise = _bp(gen.standard_normal(span), 800.0, 5000.0)
    return noise * np.sin(0.5 * np.pi * np.minimum(tt / 0.0025, 1.0)) ** 2 * np.exp(-tt / 0.003)


def _grit(span: int, gen: np.random.Generator) -> np.ndarray:
    return dsp.butter(gen.standard_normal(span), "high", 500.0, 2) * np.hanning(span)


def _grain_bits(out: np.ndarray, at: int, amp: float, lift: float, cfg: dict, gen: np.random.Generator) -> None:
    left = len(out) - at
    if cfg["grain_tick"] > 0.01:
        span = min(dsp.n_of(0.014), left)
        if span > 8:
            _mix_at(out, 0.25 * cfg["grain_tick"] * amp * _norm(_grain_click(span, gen)), at)
    if cfg["crackle"] > 0.01:
        span = min(dsp.n_of(gen.uniform(0.001, 0.003)), left)
        if span > 8:
            _mix_at(out, cfg["crackle"] * amp * np.exp(0.5 * lift) * _norm(_grit(span, gen)), at)


def _rumble_side(cfg: dict, size: int, secs: np.ndarray, gen: np.random.Generator) -> np.ndarray:
    out = np.zeros(size)
    span = size / SR
    when = 0.01
    while when < span:
        pos = when / span
        piece = dsp.n_of(gen.uniform(0.025, 0.06))
        grain = _low_grain(piece, gen)
        lift = gen.standard_normal()
        amp = 10 ** (-when / cfg["rumble_t20"]) * np.exp(min(0.35 * lift, 0.3))
        at = dsp.n_of(when)
        _mix_at(out, amp * _norm(grain), at)
        if at < size:
            _grain_bits(out, at, amp, lift, cfg, gen)
        when += gen.exponential(1.0 / (cfg["grain_rate"] * (1 - 0.5 * pos)))
    if cfg["rumble"] > 0.01:
        walk = np.cumsum(gen.standard_normal(size))
        out += _norm(_bp(walk - walk.mean(), 35.0, 220.0)) * 0.06 * 10 ** (-secs / cfg["rumble_t20"])
    return out


def end_cut(y: np.ndarray, end_db: float = -60.0) -> np.ndarray:
    """Trim to 30 ms past the last 5 ms frame that is still within ``end_db`` of the loudest."""
    piece = y.mean(axis=1) if y.ndim == 2 else y
    frames = len(piece) // 240
    if frames < 4:
        return y
    rms = np.sqrt(np.mean(piece[: frames * 240].reshape(frames, 240) ** 2, axis=1))
    db = 20 * np.log10(rms + 1e-12)
    last = int(np.nonzero(db >= db.max() + end_db)[0][-1])
    return y[: min(len(y), (last + 1) * 240 + dsp.n_of(0.03))]


def _roomed(cfg: dict, top: np.ndarray, size: int, gen: np.random.Generator) -> np.ndarray:
    if cfg["room"] > 0.01:
        return dsp.room(top, gen, rt60=0.3, size=0.35, damp=0.7, wet=cfg["room"])[:size]
    dsp.room_ir(gen, rt60=0.3, size=0.35, damp=0.7)
    return top


def _spread_top(top: np.ndarray, noise: float, keep: float) -> tuple[np.ndarray, np.ndarray]:
    size = len(top)
    upper = dsp.butter(top, "high", 150.0, 2)
    lower = top - upper
    lag = dsp.n_of(0.0007)
    side = np.zeros(size)
    if lag < size:
        side[lag:] = upper[: size - lag]
    side = dsp.butter(side, "high", 150.0, 2)
    return lower + upper, lower + keep * upper + noise * side


def _common_peak(amp: np.ndarray, walk: np.ndarray) -> float:
    return float(max(np.max(np.abs(amp)), np.max(np.abs(walk)))) + 1e-12


def _make_layers(cfg: dict, size: int, secs: np.ndarray, gen: np.random.Generator) -> dict:
    """Make every layer in a fixed order, even silent ones, so one seed always gives one take."""
    bank = {}
    bank["sub"] = _sub_pair(cfg, secs, gen)
    bank["kick"] = _kick_drop(cfg, secs, gen)
    bank["click"] = _snap(cfg, size, gen)
    bank["punch"] = _mid_punch(cfg, secs, gen)
    bank["air"] = _air_pair(cfg, secs, gen)
    bank["rumble"] = (_rumble_side(cfg, size, secs, gen), _rumble_side(cfg, size, secs, gen))
    return bank


def _narrow(left: np.ndarray, right: np.ndarray, keep: float, noise: float) -> tuple[np.ndarray, np.ndarray, float]:
    """Pull the right side toward the left by the width; return both sides and their common peak."""
    right = keep * left + noise * right
    return left, right, _common_peak(left, right)


def _combine(cfg: dict, bank: dict, size: int, gen: np.random.Generator) -> np.ndarray:
    top = 0.6 * cfg["kick"] * bank["kick"] + 0.5 * cfg["click"] * bank["click"] + 0.6 * cfg["punch"] * bank["punch"]
    top = _roomed(cfg, top, size, gen)
    noise = cfg["width"]
    keep = np.sqrt(max(0.0, 1 - noise * noise))
    top_l, top_r = _spread_top(top, noise, keep)
    rum_l, rum_r, rum_top = _narrow(*bank["rumble"], keep, noise)
    air_l, air_r, air_top = _narrow(*bank["air"], keep, noise)
    left = _side_out(cfg, bank["sub"][0] + top_l, (rum_l, rum_top), (air_l, air_top))
    right = _side_out(cfg, bank["sub"][1] + top_r, (rum_r, rum_top), (air_r, air_top))
    return np.column_stack([left, right])


def _side_out(cfg: dict, base: np.ndarray, rum: tuple, air: tuple) -> np.ndarray:
    """One output side: sub plus top, with rumble and air each scaled by their common peak."""
    return base + 0.7 * cfg["rumble"] * rum[0] / rum[1] + 0.35 * cfg["air"] * air[0] / air[1]


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """Render one hit: stereo float64, no longer than ``length`` seconds, peak 0.89."""
    p = {**DEFAULTS, **p}
    size = dsp.n_of(p["length"])
    secs = np.arange(size) / SR
    out = _combine(p, _make_layers(p, size, secs, rng), size, rng)
    out = end_cut(dsp.butter(out, "high", 24.0, 2), -60.0)
    out = dsp.fade(out, 0.0004, min(p["fade"], 0.9 * len(out) / SR))
    return out * _OUT_PEAK / (np.max(np.abs(out)) + 1e-12)
