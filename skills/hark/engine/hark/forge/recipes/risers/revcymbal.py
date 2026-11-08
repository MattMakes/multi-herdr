"""revcymbal: a reverse-cymbal swell built forward in time, so the stop after the peak can be shaped freely."""

from __future__ import annotations

import types

import numpy as np

from hark.forge.engine import dsp, fx
from hark.forge.recipes.risers._common import bw, member_recipe, n_of, svf, t_of

TARGET = "a reverse cymbal swell"
ALTS = ["a jet engine", "a wind gust", "rain", "a sword being sharpened"]
REFSET = "reverse_swell"
TARGETS = {"reverse_swell": TARGET}
MODULE = "hark.forge.recipes.risers.revcymbal"

SPACE = {
    "length": (0.3, 2.0), "swell_db": (12.0, 60.0), "curve": (0.5, 3.5, "log"), "hold": (0.0, 0.45),
    "hold_db": (-12.0, 0.0), "tail_tau": (0.008, 0.6, "log"), "cym_f": (300.0, 7000.0, "log"), "bw": (1.0, 6.0),
    "metal": (0.0, 1.0), "partials": (12, 160, "int"), "shimmer": (0.0, 1.0), "open": (0.0, 4.0),
    "hp": (80.0, 2500.0, "log"), "low_db": (-50.0, 0.0), "flut_depth": (0.0, 1.0), "flut_f": (6.0, 60.0, "log"),
    "width": (0.0, 1.0), "room": (0.0, 0.5), "rt60": (0.2, 2.0, "log"),
}

_KEYS = list(SPACE)
_COLS = {
    "cymbal": (0.5037, 19.55, 1.831, 0.22, -8.039, 0.03096, 2600, 3.469, 0.6317, 78, 0.5093, 0.0, 118.7, -50,
               0.6266, 18.8, 0.98, 0.0, 1.386),
    "ring": (0.808, 40.86, 2.308, 0.0, -2.062, 0.1542, 2070, 5.575, 0.7482, 79, 0.8912, 3.093, 658.9, -50, 0.4417,
             21.97, 1.0, 0.129, 1.209),
    "dark": (0.5391, 30.0, 0.5, 0.03684, -0.4249, 0.03842, 346, 3.456, 0.259, 117, 0.3166, 2.554, 80.0, 0.0,
             0.004984, 28.16, 0.35, 0.08302, 0.5783),
    "short": (0.33, 24.0, 0.8282, 0.08, -6.0, 0.025, 3200, 3.2, 0.6, 70, 0.04221, 0.5, 300, -50, 0.8351, 24.0,
              0.95, 0.05, 0.8),
    "breath": (0.7, 30.0, 1.3, 0.094, 0.0, 0.06, 1200, 1.499, 0.12, 60, 0.3, 1.5, 150, -24, 0.366, 14.0, 0.5818,
               0.2272, 0.5846),
}
FAMILIES = {"reverse_swell": [(label, {kn: float(v) for kn, v in zip(_KEYS, cols)}) for label, cols in _COLS.items()]}
DEFAULTS = dict(FAMILIES["reverse_swell"][0][1])


def level_db(cfg: dict, tax: np.ndarray) -> tuple[np.ndarray, float]:
    """Level in dB relative to the peak (9.5), and the peak time."""
    dur, hd = cfg["length"], cfg["hold"]
    rise = -cfg["swell_db"] * (1 - np.clip(tax / dur, 0, 1)) ** cfg["curve"]
    plateau = cfg["hold_db"] * (tax - dur) / hd if hd > 0 else np.zeros_like(tax)
    fall = cfg["hold_db"] - 8.686 * (tax - dur - hd) / cfg["tail_tau"]
    res = np.where(tax <= dur, rise, np.where(tax <= dur + hd, plateau, fall))
    return res, dur


def _rms(wav: np.ndarray) -> float:
    return float(np.sqrt(np.mean(wav ** 2))) + 1e-12


def _control(ns: int, fc: float, gen: np.random.Generator) -> np.ndarray:
    """P6: white noise low-passed (order 2) at fc, unit standard deviation."""
    sig = bw(gen.standard_normal(ns), "low", fc, 2)
    return sig / (np.std(sig) + 1e-12)


def _metal(cfg: dict, ns: int, lo: float, hiv: float, gen: np.random.Generator) -> np.ndarray:
    kn = int(round(cfg["partials"]))
    freq = np.exp(gen.uniform(np.log(lo), np.log(hiv), kn))
    sigma = max(0.3, cfg["bw"] / 3)
    wts = np.exp(-0.5 * (np.log2(freq / cfg["cym_f"]) / sigma) ** 2) * gen.uniform(0.4, 1.0, kn)
    ctrl = _control(ns, 3.0, gen)
    shifts = gen.integers(0, ns, kn)
    phases = gen.uniform(0, 2 * np.pi, kn)
    tax = t_of(ns)
    sig = np.zeros(ns)
    for nth in range(kn):
        am = 1 + 0.35 * cfg["shimmer"] * np.roll(ctrl, int(shifts[nth]))
        sig += wts[nth] * am * np.sin(2 * np.pi * freq[nth] * tax + phases[nth])
    return sig / _rms(sig)


def texture(cfg: dict, ns: int, gen: np.random.Generator) -> tuple[np.ndarray, float]:
    """One channel of metallic partials blended with band noise (9.6); returns (x, hi)."""
    lo = max(60.0, cfg["cym_f"] / 2 ** (cfg["bw"] / 2))
    hiv = min(18000.0, cfg["cym_f"] * 2 ** (cfg["bw"] / 2))
    metal = _metal(cfg, ns, lo, hiv, gen)
    noise = bw(gen.standard_normal(ns), "band", (lo, hiv), 2)
    noise = dsp.peq(noise, cfg["cym_f"], 6.0, 1.0)
    noise = noise / _rms(noise)
    wav = cfg["metal"] * metal + (1 - cfg["metal"]) * noise
    return wav / _rms(wav), hiv


def _flutter(cfg: dict, ns: int, gen: np.random.Generator) -> np.ndarray:
    dv = cfg["flut_depth"]
    return np.exp(dv * _control(ns, cfg["flut_f"], gen) - 0.5 * dv * dv)


def _channels(cfg: dict, ns: int, gen: np.random.Generator):
    """Left and right (texture, low body, flutter) with correlation about sqrt(1 - width^2)."""
    xl, hiv = texture(cfg, ns, gen)
    xr, _ = texture(cfg, ns, gen)
    lows = [bw(gen.standard_normal(ns), "band", (100.0, 450.0), 2) for _ in range(2)]
    fls = [_flutter(cfg, ns, gen) for _ in range(2)]
    wv = cfg["width"]
    kn = np.sqrt(max(0.0, 1 - wv * wv))
    left = (xl, lows[0], fls[0])
    right = (kn * xl + wv * xr, kn * lows[0] + wv * lows[1], kn * fls[0] + wv * fls[1])
    return (left, right), hiv


def _edit_end(sig: np.ndarray, cfg: dict) -> int:
    """Cut 20 ms after the first 5 ms block, at or after the loudest, that is 60 dB under it (9.7 step 9)."""
    mv = sig.mean(axis=1)
    hv = 240
    kn = max(1, len(mv) // hv)
    env = 20 * np.log10(np.sqrt(np.mean(mv[:kn * hv].reshape(kn, hv) ** 2, axis=1)) + 1e-12) if len(mv) >= hv else \
        np.array([0.0])
    ix = int(np.argmax(env))
    quiet = np.nonzero(env[ix:] < env[ix] - 60)[0]
    block = ix + int(quiet[0]) if len(quiet) else kn
    return min(len(sig), block * hv + n_of(0.02))


def _sizes(cfg: dict) -> int:
    """9.7 step 1: the peak, the plateau, the fall to -60 dB and 20 ms, plus some room tail; at most 4 s."""
    tail = cfg["length"] + cfg["hold"] + cfg["tail_tau"] * 60 / 8.686 + 0.02
    return n_of(min(4.0, tail + 0.3 * cfg["rt60"] * min(1.0, 4 * cfg["room"])))


def _shaped(cfg: dict, pair, hiv: float, amp: np.ndarray) -> np.ndarray:
    """9.7 steps 5 and 6: a low-pass that opens as the level grows, the low body, the level and flutter."""
    cut = np.clip(1.3 * hiv * 2.0 ** (-cfg["open"] * (1 - np.sqrt(amp))), 300, 20000)
    res = []
    for wav, low, flut in pair:
        sig = svf(wav, cut, 0.7, "low")
        if cfg["low_db"] > -49.5:
            sig = sig + 10 ** (cfg["low_db"] / 20) * low / _rms(low)
        res.append(sig * amp * flut)
    return bw(np.column_stack(res), "high", cfg["hp"], 2)


def _room(cfg: dict, sig: np.ndarray, gen) -> np.ndarray:
    """The forge-core reverb, cut to the dry length; a non-finite result keeps the dry signal."""
    wetv = fx.reverb(sig, gen, rt60=cfg["rt60"], size=0.5, predelay=0.008, damp=0.4, wet=cfg["room"], width=1.0,
                    lines=16, stereo=True, tail=max(0.2, cfg["rt60"]))[:len(sig)]
    return wetv if np.all(np.isfinite(wetv)) and len(wetv) == len(sig) else sig


def render(cfg: dict, gen: np.random.Generator) -> np.ndarray:
    """One take, (n, 2), normalised to an absolute peak of 0.89."""
    cfg = {**DEFAULTS, **(cfg or {})}
    ns = _sizes(cfg)
    amp = 10 ** (np.maximum(level_db(cfg, t_of(ns))[0], -120) / 20)
    pair, hiv = _channels(cfg, ns, gen)
    sig = _shaped(cfg, pair, hiv, amp)
    if cfg["room"] > 0.01:
        sig = _room(cfg, sig, gen)
    sig = bw(sig, "low", 19000, 4)
    sig = dsp.fade(sig[:_edit_end(sig, cfg)], 0.004, 0.02)
    return 0.89 * sig / (np.max(np.abs(sig)) + 1e-12)


_RECIPE = (MODULE, FAMILIES, SPACE, TARGETS, ALTS, REFSET)


def recipe_for(cat: str = "reverse_swell", preset=0) -> types.SimpleNamespace:
    """A recipe object around one member, by index or by name."""
    return member_recipe(*_RECIPE, render, cat, preset)
