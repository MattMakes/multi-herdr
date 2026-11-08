"""zip: a designed tonal zip, one fast exponential pitch sweep (recipes-motion part 2, section 7).

A sine with harmonics 2, 3 and 5, optional FM sheen, a filtered saw body, an air band
that follows the pitch, a wide noise band and zipper teeth. Near mono.
"""

from __future__ import annotations

from typing import Any

import numpy as np

from hark.forge.engine import dsp, fx
from hark.forge.recipes.motion import _util as u
from hark.forge.recipes.risers.riser import lvl, side_db, stereo_of, wet_of

SR = u.SR
TARGET = "a quick tonal zip, a fast synth pitch sweep"
ALTS = ["a bird chirp", "a squeaky door hinge", "a car horn", "a phone ringing"]
REFSET = "pro_zip"

SPACE: dict[str, Any] = {
    "shape": ["up", "down", "arc"], "length": (0.06, 0.3), "f0": (80.0, 8000.0, "log"),
    "f1": (80.0, 12000.0, "log"), "fpk": (500.0, 6000.0, "log"), "pk_at": (0.3, 0.7),
    "arc_back": (0.5, 1.0), "bend": (-0.5, 0.6), "h2_db": (-40.0, -6.0), "h3_db": (-40.0, -6.0),
    "h5_db": (-48.0, -12.0), "sub_db": (-48.0, -6.0), "fm": (0.0, 1.5), "fm_ratio": [1.0, 2.0],
    "saw_db": (-48.0, 0.0), "saw_lp": (1.5, 6.0, "log"), "air_db": (-40.0, -10.0),
    "air_q": (2.0, 8.0, "log"), "detune": (0.0, 15.0), "att_ms": (3.0, 40.0, "log"),
    "rel_ms": (4.0, 40.0, "log"), "tilt_db": (-9.0, 9.0), "verb": (0.0, 0.3),
    "width": (0.0, 0.35), "sat": (0.0, 9.0), "noise_db": (-60.0, -4.0),
    "noise_ratio": (0.7, 3.0, "log"), "noise_q": (0.5, 2.0, "log"), "teeth": (0.0, 0.7),
    "teeth_hz": (40.0, 260.0, "log"),
}

_KEYS = ("shape", "length", "f0", "f1", "fpk", "pk_at", "arc_back", "bend", "h2_db", "h3_db",
         "h5_db", "sub_db", "fm", "fm_ratio", "saw_db", "saw_lp", "air_db", "air_q", "detune",
         "att_ms", "rel_ms", "tilt_db", "verb", "width", "sat")
_COLUMNS = {
    "up": ("up", 0.19, 100, 1600, 2000, 0.5, 1.0, 0.05, -14, -24, -30, -48, 0.3, 1, -6, 3.0,
           -28, 4, 6, 28, 10, 6, 0.06, 0.12, 3),
    "up_hi": ("up", 0.24, 130, 10000, 2000, 0.5, 1.0, 0.25, -16, -24, -36, -48, 0.15, 2, -10,
              2.5, -24, 5, 7, 30, 6, 4, 0.05, 0.1, 2),
    "down": ("down", 0.075, 7200, 420, 2000, 0.5, 1.0, -0.2, -18, -26, -40, -48, 0.2, 1, -8,
             3.5, -26, 4, 3, 6, 12, -6, 0.08, 0.1, 2),
    "arc": ("arc", 0.25, 1000, 1000, 3000, 0.45, 0.9, 0.0, -16, -22, -36, -48, 0.25, 1, -14,
            2.0, -30, 5, 5, 22, 18, -3, 0.07, 0.12, 2),
    "down_warm": ("down", 0.2, 2400, 160, 2000, 0.5, 1.0, -0.3, -10, -16, -28, -20, 0.5, 1, -2,
                  2.2, -32, 3, 8, 10, 22, -8, 0.1, 0.15, 4),
}
_AIR_OFF = {"noise_db": -60.0, "noise_ratio": 1.5, "noise_q": 1.0, "teeth": 0.0, "teeth_hz": 120.0}


def _member(values: tuple) -> dict[str, Any]:
    d = {k: (v if isinstance(v, str) else float(v)) for k, v in zip(_KEYS, values)}
    return {**d, **_AIR_OFF}


FAMILIES: dict[str, dict[str, Any]] = {name: _member(col) for name, col in _COLUMNS.items()}
DEFAULTS: dict[str, Any] = dict(FAMILIES["up"])

_R2_KEYS = ("length", "f0", "f1", "fpk", "pk_at", "arc_back", "bend", "h2_db", "h3_db", "h5_db",
            "sub_db", "fm", "fm_ratio", "saw_db", "saw_lp", "air_db", "air_q", "detune", "att_ms",
            "rel_ms", "tilt_db", "verb", "width", "sat")
_R2 = {
    "up": (0.178, 126.06, 1707.5, 2405.7, 0.506, 0.957, 0.027, -11.4, -23.0, -26.0, -46.1, 0.319,
           1, -1.99, 2.67, -28.7, 4.59, 4.36, 34.2, 9.54, 7.43, 0.025, 0.109, 2.09),
    "up_hi": (0.236, 176.5, 8869.0, 1800.9, 0.521, 0.969, 0.382, -13.75, -21.3, -38.1, -47.2,
              0.066, 2, -9.86, 2.26, -22.0, 5.65, 6.34, 29.8, 6.32, 4.96, 0.025, 0.082, 2.5),
    "down": (0.103, 6706.8, 477.6, 2160.3, 0.486, 0.948, -0.15, -17.95, -27.9, -40.3, -43.7,
             0.362, 1, -7.4, 3.29, -25.5, 3.82, 2.62, 7.96, 10.07, -5.4, 0.051, 0.127, 2.67),
    "arc": (0.277, 623.3, 1000.0, 3470.1, 0.419, 0.887, -0.115, -14.16, -25.4, -36.9, -43.6, 0.24,
            1, -10.94, 2.3, -32.7, 4.34, 3.45, 27.7, 19.4, -3.0, 0.046, 0.135, 1.61),
    "down_warm": (0.187, 2505.5, 149.4, 2586.2, 0.456, 0.984, -0.27, -13.85, -14.2, -32.2, -17.4,
                  0.505, 1, -7.25, 2.47, -35.1, 2.85, 7.75, 13.4, 22.8, -8.2, 0.081, 0.183, 3.41),
}
_R3 = {
    "up": (-8, 1.6, 0.9, 0.3, 90), "up_hi": (-10, 1.3, 0.8, 0.2, 120),
    "down": (-12, 1.4, 0.9, 0.15, 150), "arc": (-8, 1.6, 0.9, 0.35, 70),
    "down_warm": (-12, 2.2, 0.8, 0.3, 60),
}


def _round2(name: str) -> dict[str, float]:
    return {k: float(v) for k, v in zip(_R2_KEYS, _R2[name])}


def _round3(name: str) -> dict[str, float]:
    return {k: float(v) for k, v in zip(tuple(_AIR_OFF), _R3[name])}


def _pro(name: str) -> dict[str, Any]:
    return {**FAMILIES[name], **_round2(name), **_round3(name)}


_PRO = {name: _pro(name) for name in _COLUMNS}
_PRO["air_up"] = {**FAMILIES["up"], **{k: float(v) for k, v in zip(_R2_KEYS, _R2["up"])}, **{
    "length": 0.2, "f0": 300.0, "f1": 4500.0, "h2_db": -20.0, "h3_db": -30.0, "saw_db": -12.0,
    "noise_db": -4.0, "noise_ratio": 1.2, "noise_q": 1.4, "teeth": 0.35, "teeth_hz": 110.0,
    "att_ms": 25.0, "tilt_db": 6.0}}
FAMILIES_BY_CAT: dict[str, dict[str, dict[str, Any]]] = {"pro_zip": _PRO}


def trajectory(p: dict, n: int) -> np.ndarray:
    """The pitch per sample (section 7.6); a straight line on a log axis, or an arc."""
    t = np.linspace(0.0, 1.0, n)
    if p["shape"] == "arc":
        return _arc(p, t)
    f0 = float(p["f0"])
    return f0 * (_end_pitch(p, f0) / f0) ** (t ** (2 ** p["bend"]))


def _end_pitch(p: dict, f0: float) -> float:
    """f1, or 4 f0 (up) or f0 / 4 (down) when the sweep is under 1.5 times."""
    f1 = float(p["f1"])
    if p["shape"] == "up" and f1 < 1.5 * f0:
        return 4 * f0
    if p["shape"] == "down" and f1 > f0 / 1.5:
        return f0 / 4
    return f1


def _arc(p: dict, u_: np.ndarray) -> np.ndarray:
    """The arc of 7.6: up to fpk on a sine, back to f0 x arc_back on a cosine (log2 Hz)."""
    a, f0 = float(p["pk_at"]), float(p["f0"])
    lo, pk, back = np.log2(f0), np.log2(p["fpk"]), np.log2(f0 * p["arc_back"])
    rise = lo + (pk - lo) * np.sin(0.5 * np.pi * np.clip(u_ / a, 0, 1))
    fall = pk + (back - pk) * (1 - np.cos(0.5 * np.pi * np.clip((u_ - a) / (1 - a), 0, 1)))
    return 2 ** np.where(u_ <= a, rise, fall)


def _voice(p: dict, f: np.ndarray, cents: float, rng: np.random.Generator) -> np.ndarray:
    fk = f * 2 ** (cents / 1200.0)
    phi = 2 * np.pi * np.cumsum(fk) / SR + rng.uniform(0, 2 * np.pi)
    ratio = float(p["fm_ratio"])
    m = 0.0
    if p["fm"] > 0.01:
        fmi = p["fm"] * np.clip((14000 - fk * (1 + 2 * ratio)) / 4000.0, 0, 1)
        m = fmi * np.sin(ratio * phi)
    v = np.sin(phi + m)
    for h, key in ((2, "h2_db"), (3, "h3_db"), (5, "h5_db")):
        fade = np.clip((18000 - h * fk) / 3000.0, 0, 1)
        v = v + 10 ** (p[key] / 20.0) * fade * np.sin(h * phi + 0.5 * h * m)
    if p["sub_db"] > -47:
        v = v + 10 ** (p["sub_db"] / 20.0) * np.sin(0.5 * phi)
    return v


def _voices(p: dict, f: np.ndarray, rng: np.random.Generator) -> np.ndarray:
    out = np.zeros((len(f), 2))
    side_gain = 0.5 * min(1.0, p["detune"] / 4.0)
    for cents, pan, gain in ((0.0, 0.0, 1.0), (-p["detune"], -1.0, side_gain),
                             (p["detune"], 1.0, side_gain)):
        v = _voice(p, f, cents, rng)
        gl, gr = u.pan_gains(pan * p["width"])
        out[:, 0] += gain * v * gl
        out[:, 1] += gain * v * gr
    return out


def _saw(p: dict, f: np.ndarray, rng: np.random.Generator) -> np.ndarray:
    saw = np.zeros(len(f))
    for c in (-p["detune"], 0.0, p["detune"]):
        saw += dsp.osc(f * 2 ** (c / 1200.0), "saw", phase=rng.uniform(0, 1))[: len(f)]
    saw = dsp.svf(saw, np.clip(f * p["saw_lp"], 150, 16000), 1.1, "low")
    saw = dsp.svf(saw, np.clip(1.05 * f * p["saw_lp"], 150, 16000), 0.7, "low")
    return dsp.butter(saw, "low", 15000.0, 6)


def _tone(p: dict, f: np.ndarray, rng: np.random.Generator) -> np.ndarray:
    out = _voices(p, f, rng)
    if p["saw_db"] > -47:
        saw = _saw(p, f, rng)
        out = out / lvl(out) + 10 ** (p["saw_db"] / 20.0) * stereo_of(saw) / lvl(saw)
    return out / lvl(out)


def _noises(p: dict, f: np.ndarray, rng: np.random.Generator) -> np.ndarray:
    n = len(f)
    air = dsp.svf(rng.standard_normal(n), np.clip(1.5 * f, 200, 16000), p["air_q"], "bandn")
    y = 10 ** (p["air_db"] / 20.0) * stereo_of(air) / lvl(air)
    if p["noise_db"] > -59:
        wide = dsp.svf(rng.standard_normal(n), np.clip(f * p["noise_ratio"], 250, 15000),
                       p["noise_q"], "band")
        wide = dsp.butter(wide, "low", 16000.0, 4)
        y = y + 10 ** (p["noise_db"] / 20.0) * stereo_of(wide) / lvl(wide)
    return y


def _envelope(p: dict, f: np.ndarray) -> np.ndarray:
    length = float(p["length"])
    t = u.times(len(f))
    a = np.clip(t / (p["att_ms"] / 1000.0), 0, 1)
    rl = np.clip((length - t) / (p["rel_ms"] / 1000.0), 0, 1)
    env = (0.5 - 0.5 * np.cos(np.pi * a)) * (0.5 - 0.5 * np.cos(np.pi * rl))
    env = env * 10 ** (p["tilt_db"] * (t / length) / 20.0)
    if p["teeth"] > 0.01:
        rate = p["teeth_hz"] * np.sqrt(f / np.exp(np.mean(np.log(f))))
        env = env * (1 - p["teeth"] * (0.5 - 0.5 * np.cos(2 * np.pi * np.cumsum(rate) / SR)))
    return env


def _room(y: np.ndarray, p: dict) -> np.ndarray:
    n = len(y)
    tail = u.samples(0.3)
    wet = wet_of(y, 0.3, n + tail, size=0.3, predelay=0.004, lowcut=300)
    y = np.vstack([y, np.zeros((tail, 2))]) + p["verb"] * wet / lvl(wet) * lvl(y)
    y = dsp.trim_tail(y, -60.0, 0.004)
    return dsp.fade(y, 0.0, min(0.02, 0.1 * p["length"]))


def _remove_dc(y: np.ndarray) -> np.ndarray:
    h = np.hanning(len(y)) if len(y) > 1 else np.ones(1)
    return y - (y.mean(axis=0) / h.mean())[None, :] * h[:, None]


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """One take of the zip (section 7.7), float64 stereo at 48 kHz."""
    q = {**DEFAULTS, **p}
    n = u.samples(q["length"])
    f = trajectory(q, n)
    y = _tone(q, f, rng) + _noises(q, f, rng)
    y = dsp.butter(y * _envelope(q, f)[:, None], "high", 40.0, 2)
    if q["sat"] > 0.1:
        y = dsp.butter(fx.saturate(y, q["sat"], mode="tanh"), "low", 18000.0, 4)
    if q["verb"] > 0.005:
        y = _room(y, q)
    y = dsp.fade(y, 0.0005, 0.002)
    return u.below_full_scale(_remove_dc(np.asarray(y, dtype=float)))


def ridge(x: np.ndarray, hop_ms: float = 2.5, win: int = 1024) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    """The strongest-bin frequency per frame: (frame times s, Hz, level dB re the loudest frame)."""
    m = np.concatenate([np.zeros(win // 2), u.mono(x), np.zeros(win // 2)])
    hop = max(1, int(round(hop_ms * SR / 1000.0)))
    count = 1 + max(0, (len(m) - win) // hop)
    w = np.hanning(win)
    frames = np.stack([m[i * hop : i * hop + win] * w for i in range(count)])
    mag = np.abs(np.fft.rfft(frames, axis=1))
    mag[:, :2] = 0.0
    k = np.argmax(mag, axis=1)
    level = 20 * np.log10(mag[np.arange(count), k] + u.EPS)
    return np.arange(count) * hop / SR, k * SR / win, level - level.max()


def _slope(t: np.ndarray, y: np.ndarray) -> float:
    return float(np.polyfit(t, y, 1)[0]) if len(t) >= 2 else 0.0


def _sweep(x: np.ndarray) -> tuple[float, float, bool]:
    t, f, level = ridge(x)
    keep = (level >= -20) & (f > 0)
    t, lf = t[keep], np.log2(np.maximum(f[keep], 1.0))
    if len(lf) < 4:
        return 0.0, 0.0, False
    half = _arc_rate(t, lf)
    rate, arc = (_slope(t, lf), False) if half is None else (half, True)
    return float(lf.max() - lf.min()), rate, arc


def _arc_rate(t: np.ndarray, lf: np.ndarray) -> float | None:
    """The steeper half slope when the 2 halves sweep opposite ways above 3 oct/s, else None."""
    h = len(lf) // 2
    if h < 3 or len(lf) - h < 3:
        return None
    a, b = _slope(t[:h], lf[:h]), _slope(t[h:], lf[h:])
    if a * b >= 0 or min(abs(a), abs(b)) <= 3:
        return None
    return a if abs(a) >= abs(b) else b


def measure(x: np.ndarray) -> dict[str, Any]:
    """Length, attack, active span, end level, width and the sweep rate of a zip (section 7.8)."""
    m = u.mono(x)
    k = max(1, len(m) // 240)
    blocks = m[: k * 240].reshape(k, 240) if len(m) >= 240 else m[None, :]
    e = 20 * np.log10(np.sqrt(np.mean(blocks * blocks, axis=1)) + u.EPS)
    top = int(np.argmax(e))
    live = np.nonzero(e >= e[top] - 30)[0]
    span, rate, arc = _sweep(x)
    return {"length_s": len(m) / SR, "attack_ms": 5.0 * top,
            "active_s": (live[-1] - live[0] + 1) * 0.005, "end_db": float(e[-1] - e[top]),
            "side_db": side_db(x), "oct_span": span, "oct_per_s": rate, "arc": arc}


def target_check(x: np.ndarray, row: Any = None) -> float:
    """A penalty score in [-3, 0] against the measured pro zips (section 7.8)."""
    m = measure(x)
    r = abs(m["oct_per_s"])
    lo, span = (8.0, 1.0) if m["arc"] else (12.0, 1.5)
    penalty = (max(0.0, lo - r) / 6 + max(0.0, r - 75) / 20 + 1.5 * max(0.0, span - m["oct_span"])
               + 10 * max(0.0, m["active_s"] - 0.33) + max(0.0, m["side_db"] + 10) / 6)
    return -min(3.0, penalty)
