"""Version 1 of the whoosh pass engine (recipes-motion part 1, section 1.13).

A version-1 set models a stick that moves fast past a close microphone. These sets
have no key ``rise``. The render stays so that old seed-bank entries still render.
"""

from __future__ import annotations

from typing import Any

import numpy as np

from hark.forge.engine import dsp
from hark.forge.recipes.motion import _util as u

SR = u.SR
LENGTH_S = 0.8
BANDS = 6
BAND_ROOT = 0.45
WAKE_RATIO = 0.9
PINK_SHARE = 0.5

DEFAULTS_V1: dict[str, float] = {
    "tc": 0.0706346, "s_rise": 0.0380857, "s_fall": 0.0578346, "beta": 1.49847,
    "prox_w": 0.0243735, "f_peak": 398.391, "f_track": 0.108498, "q_shed": 7.05767,
    "steep": 0.23086, "a_pow": 2.90371, "doppler": 0.00143736, "wake_db": -14.0966,
    "wake_tau": 0.0532322, "wake_q": 4.45435, "wake_dark": 0.613911, "wake_hiss_db": -16.0,
    "hiss_db": -19.345, "hiss_hp": 1671.88, "hiss_f": 9908.19, "hiss_pow": 2.57151,
    "push_db": -4.56985, "push_w": 0.0251241, "push_f": 125.898, "buffet_db": -9.18542,
    "buffet_f": 95.0096, "buffet_delay": 0.0219106, "buffet_tau": 0.0262978,
    "flut_depth": 0.0178753, "flut_f": 30.2817, "whip_db": -16.8109, "whip_q": 22.6043,
    "whip_ratio": 1.75668, "pan_w": 0.102279, "width": 0.0730709, "room_wet": 0.158579,
    "rt60": 0.138233, "air_db": -12.0, "floor_db": -75.3278, "cut_db": -46.4213,
    "cut_fade": 0.0281062,
}


class _Stick:
    """The shared motion of one version-1 render."""

    def __init__(self, p: dict, rng: np.random.Generator):
        self.p, self.rng = p, rng
        self.n = u.samples(LENGTH_S)
        self.t = u.times(self.n)
        self.d = self.t - p["tc"]
        s = np.where(self.d < 0, p["s_rise"], p["s_fall"])
        self.U = np.exp(-0.5 * np.abs(self.d / s) ** p["beta"])
        x = self.d / p["prox_w"]
        self.rr = np.sqrt(1 + x * x)
        self.dop = 1 - p["doppler"] * (x / self.rr * self.U)
        self.amp = self.U ** p["a_pow"] / self.rr
        fd = p["flut_depth"]
        self.flut = np.exp(fd * u.control(self.n, p["flut_f"], rng) - 0.5 * fd * fd)
        self.f_tip = np.maximum(p["f_peak"] * np.maximum(self.U, 0.001) ** p["f_track"] * self.dop, 30)
        self.on = 0.5 + 0.5 * np.tanh(self.d / max(0.25 * p["s_rise"], 0.003))
        self.after = np.maximum(self.d, 0.0)

    def band(self, fc: Any, q: float, steep: bool = True) -> np.ndarray:
        y = dsp.svf(self.rng.standard_normal(self.n), fc, q, "band") / q
        k = self.p["steep"] if steep else 0.0
        if k > 0:
            y = (1 - k) * y + k * dsp.svf(y, np.asarray(fc) * 1.08, 0.9, "low")
        return y

    def noise(self, color: str) -> np.ndarray:
        return dsp.noise(LENGTH_S, self.rng, color)[: self.n]


def _shedding(s: _Stick) -> np.ndarray:
    y = np.zeros(s.n)
    for k in range(1, BANDS + 1):
        sk = BAND_ROOT + (1 - BAND_ROOT) * k / BANDS
        y += sk**3 * s.band(np.maximum(s.f_tip * sk, 25), s.p["q_shed"])
    return y * s.amp * s.flut


def _wake(s: _Stick) -> np.ndarray:
    p = s.p
    env = s.on * np.exp(-s.after / p["wake_tau"])
    fc = np.maximum(WAKE_RATIO * p["f_peak"] * (1 + s.after / p["wake_tau"]) ** (-p["wake_dark"]), 40)
    return s.band(fc, p["wake_q"]) * env


def _hiss(s: _Stick) -> np.ndarray:
    p = s.p
    src = (1 - PINK_SHARE) * s.rng.standard_normal(s.n) + PINK_SHARE * 3 * s.noise("pink")
    y = dsp.svf(src, np.maximum(p["hiss_f"] * s.U**0.7 * s.dop, 400), 0.6, "low")
    y = dsp.butter(y, "high", p["hiss_hp"], 4)
    return y * s.U ** p["hiss_pow"] / s.rr * s.flut


def _wake_hiss(s: _Stick) -> np.ndarray:
    p = s.p
    y = dsp.butter(dsp.butter(s.noise("pink"), "high", p["hiss_hp"], 4), "low", 9000.0, 2)
    return y * s.on * np.exp(-s.after / (0.4 * p["wake_tau"]))


def _push(s: _Stick) -> np.ndarray:
    p = s.p
    win = np.exp(-0.5 * (s.d / p["push_w"]) ** 2)
    body = win * dsp.butter(s.noise("brown"), "low", p["push_f"], 2)
    w = 0.35 * p["push_w"]
    ricker = (1 - (s.d / w) ** 2) * np.exp(-0.5 * (s.d / w) ** 2)
    ricker = dsp.butter(ricker, "low", 2 * p["push_f"], 2)
    return body / u.block_peak_rms(body) + 0.7 * ricker / u.block_peak_rms(ricker)


def _buffet(s: _Stick) -> np.ndarray:
    p = s.p
    tb = s.t - (p["tc"] + p["buffet_delay"])
    rise = np.exp(-0.5 * (tb / max(0.5 * p["s_rise"], 0.004)) ** 2)
    env = np.where(tb < 0, rise, np.exp(-np.maximum(tb, 0) / p["buffet_tau"]))
    return dsp.butter(s.noise("brown"), "low", p["buffet_f"], 2) * env


def _whip(s: _Stick) -> np.ndarray:
    p = s.p
    return s.band(np.minimum(s.f_tip * p["whip_ratio"], 16000), p["whip_q"], steep=False) * s.U**4 / s.rr


def _mix(s: _Stick) -> np.ndarray:
    p = s.p
    shed = _shedding(s)
    x = shed / u.block_peak_rms(shed)
    layers = ((_wake, "wake_db"), (_hiss, "hiss_db"), (_wake_hiss, "wake_hiss_db"),
              (_push, "push_db"), (_buffet, "buffet_db"), (_whip, "whip_db"))
    for make, key in layers:
        y = make(s)
        x = x + 10 ** (p[key] / 20.0) * y / u.block_peak_rms(y)
    return x


def _stereo(s: _Stick, x: np.ndarray) -> np.ndarray:
    p = s.p
    side_sign = 1.0 if s.rng.random() < 0.5 else -1.0
    pan = side_sign * p["pan_w"] * np.tanh(s.d / p["prox_w"])
    lag = int(s.rng.uniform(0.2, 0.9) * 1e-3 * SR)
    delayed = np.concatenate([np.zeros(lag), x])[: s.n]
    side = dsp.butter(delayed - x, "high", 300.0, 2) * p["width"]
    return np.column_stack([x * (1 + pan) + side, x * (1 - pan) - side])


def _channel(ch: np.ndarray, p: dict, rng: np.random.Generator, keep: int) -> np.ndarray:
    y = dsp.room(ch, rng, rt60=p["rt60"], size=0.4, wet=p["room_wet"])
    y = dsp.mic(y, rng, low_cut=25.0, air_db=p["air_db"], floor_db=p["floor_db"])
    return dsp.butter(y, "low", 19000.0, 4)[:keep]


def _cut(y: np.ndarray, p: dict) -> np.ndarray:
    cut_db = p.get("cut_db")
    if cut_db is None:
        return dsp.fade(dsp.trim_tail(y, -62.0, 0.03), 0.002, 0.06)
    m = u.mono(y)
    k = len(m) // 240
    if k >= 1:
        blocks = m[: k * 240].reshape(k, 240)
        e = 20 * np.log10(np.sqrt(np.mean(blocks * blocks, axis=1)) + u.EPS)
        i = int(np.argmax(e))
        below = np.nonzero(e[i:] < e[i] + cut_db)[0]
        j = i + int(below[0]) if below.size else k
        y = y[: min(len(y), j * 240 + u.samples(p.get("cut_fade", 0.06)))]
    return dsp.fade(y, 0.002, p.get("cut_fade", 0.06))


def render_v1(p: dict, rng: np.random.Generator) -> np.ndarray:
    """The version-1 render of an old parameter set (no ``rise`` key)."""
    base = {k: v for k, v in DEFAULTS_V1.items() if k not in ("cut_db", "cut_fade")}
    q = {**base, **p}
    s = _Stick(q, rng)
    st = _stereo(s, _mix(s))
    keep = s.n + u.samples(0.1)
    chans = [_channel(st[:, c], q, rng, keep) for c in range(2)]
    m = min(len(c) for c in chans)
    y = np.column_stack([c[:m] for c in chans])
    return np.asarray(_cut(y, q), dtype=float)
