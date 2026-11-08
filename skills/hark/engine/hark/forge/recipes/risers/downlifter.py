"""downlifter: a sinking filtered sweep for the moment after an impact (the riser in reverse)."""

from __future__ import annotations

import numpy as np

from hark.forge.engine import dsp, fx
from hark.forge.recipes.risers._common import SR, bw, headroom, n_of, svf, t_of
from hark.forge.recipes.risers.riser import (
    centroid_track,
    env_db,
    low_share_db,
    lvl,
    side_db,
    stereo_of,
    unison,
    wet_of,
)

TARGET = "a downlifter sound effect: a sinking filtered sweep"
ALTS = ["a jet engine", "an air raid siren", "a plane flying overhead", "a gust of wind"]
REFSET = "downlifter"

SPACE = {
    "length": (1.0, 4.0), "curve": (1.0, 4.0), "drop_db": (24.0, 50.0), "att_ms": (2.0, 300.0, "log"),
    "noise_db": (-36.0, 0.0), "nf0": (5000.0, 16000.0, "log"), "nf1": (120.0, 800.0, "log"),
    "nq": (0.7, 4.0, "log"), "pink": (0.0, 1.0), "tone_db": (-36.0, 0.0), "tmode": ["fall", "tape"],
    "wave": ["sine", "tri", "saw", "square"], "tf0": (150.0, 2500.0, "log"), "toct": (0.7, 3.0),
    "tbright": (1.5, 10.0, "log"), "detune": (0.0, 30.0), "tape_curve": (0.7, 2.5), "sub_db": (-48.0, 0.0),
    "sub_f0": (60.0, 140.0, "log"), "sub_f1": (28.0, 50.0, "log"), "shep_db": (-48.0, 0.0),
    "shep_rate": (0.3, 1.6), "flange": (0.0, 0.8), "fl_rate": (0.3, 4.0, "log"), "verb": (0.0, 0.7),
    "rt60": (0.8, 4.0, "log"), "width": (0.0, 1.0), "sat": (0.0, 10.0), "hp": (40.0, 300.0, "log"),
}

_KEYS = ("length curve drop_db att_ms noise_db nf0 nf1 nq pink tone_db tmode wave tf0 toct tbright detune "
         "tape_curve sub_db sub_f0 sub_f1 shep_db shep_rate flange fl_rate verb rt60 width sat hp").split()
_COLS = {
    "air_fall": (2.2, 2.2, 40, 90, 0, 14000, 250, 1.4, 0.45, -36, "fall", "tri", 800, 1.5, 4, 10, 1.4, -48, 90,
                 38, -48, 0.8, 0, 1.0, 0.3, 1.8, 0.8, 1, 80),
    "synth_fall": (2.8, 2.0, 36, 120, -30, 6000, 300, 1.2, 0.7, 0, "fall", "saw", 880, 2.0, 4, 16, 1.4, -30, 90,
                   38, -48, 0.8, 0.35, 1.2, 0.35, 2.4, 0.6, 3, 60),
    "sub_sink": (1.6, 2.6, 38, 45, -16, 6000, 150, 1.0, 0.9, 0, "fall", "square", 400, 2.5, 2.5, 6, 1.4, -6, 85,
                 34, -48, 0.8, 0, 1.0, 0.2, 1.4, 0.3, 4, 40),
    "tape": (1.3, 1.6, 30, 40, -20, 9000, 400, 1.0, 0.5, 0, "tape", "saw", 330, 1.0, 5, 10, 1.5, -24, 82, 36,
             -48, 0.8, 0, 1.0, 0.15, 1.2, 0.4, 3, 50),
    "shepard_fall": (3.4, 1.5, 34, 200, -10, 9000, 300, 2.0, 0.8, -36, "fall", "sine", 800, 1.0, 3, 8, 1.4, -36,
                     80, 40, 0, 0.9, 0, 1.0, 0.45, 3.0, 0.85, 1, 70),
    "flange_air": (3.0, 1.8, 38, 150, 0, 12000, 400, 0.9, 0.6, -24, "fall", "tri", 1200, 2.0, 4, 10, 1.4, -40, 90,
                   38, -48, 0.8, 0.7, 0.8, 0.3, 2.2, 0.7, 1, 90),
}
FAMILIES = {name: {k: (v if isinstance(v, str) else float(v)) for k, v in zip(_KEYS, cols)}
            for name, cols in _COLS.items()}
DEFAULTS = dict(FAMILIES["air_fall"])


def comb(x: np.ndarray, delay_s: np.ndarray, mix: float) -> np.ndarray:
    """y = (x + mix x(t - d(t))) / (1 + mix); the delayed read is linearly interpolated, 0 before the start."""
    n = len(x)
    pos = np.arange(n) - np.asarray(delay_s, float) * SR
    i0 = np.floor(pos).astype(np.int64)
    fr = pos - i0
    ok0 = (i0 >= 0) & (i0 < n)
    ok1 = (i0 + 1 >= 0) & (i0 + 1 < n)
    a = np.where(ok0, x[np.clip(i0, 0, n - 1)], 0.0)
    b = np.where(ok1, x[np.clip(i0 + 1, 0, n - 1)], 0.0)
    return (x + mix * ((1 - fr) * a + fr * b)) / (1 + mix)


def _noise(p: dict, n: int, v: np.ndarray, rng) -> tuple[np.ndarray, np.ndarray]:
    pink = dsp.noise(n / SR, rng, "pink")[:n]
    src = (1 - p["pink"]) * rng.standard_normal(n) + p["pink"] * 1.6 * pink
    fc = p["nf0"] * (p["nf1"] / p["nf0"]) ** v
    y = svf(src, fc, p["nq"], "low")
    y = bw(bw(y, "high", p["hp"], 2), "low", 17000, 4)
    if p["flange"] > 0.01:
        rate = p["fl_rate"] * (1 - 0.66 * v)
        phase = np.cumsum(rate) / SR
        d = 0.0004 + 0.0086 * (0.5 - 0.5 * np.cos(2 * np.pi * phase))
        y = comb(y, d, p["flange"])
    return fx.width(fx.decorrelate(y, 1.0), p["width"]), fc


def _tape_tone(p: dict, n: int, u: np.ndarray, ph: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    m = n + n_of(0.05)
    chord = np.zeros((m, 2))
    for k, (ratio, gain) in enumerate(((1.0, 1.0), (1.5, 0.6), (2.0, 0.5))):
        f = np.full(n, p["tf0"] * ratio)
        chord[:n] += gain * unison(f, p["wave"], p["detune"], 0.5 * p["width"], np.roll(ph, k))
    sp = np.maximum((1 - u) ** p["tape_curve"], 0)
    pos = np.concatenate([[0.0], np.cumsum(sp)[:-1]])
    tone = np.column_stack([np.interp(pos, np.arange(m), chord[:, c]) for c in range(2)])
    return tone, p["tf0"] * np.maximum(sp, 0.02)


def _tone(p: dict, n: int, u: np.ndarray, v: np.ndarray, rng) -> np.ndarray:
    ph = rng.random(5)
    if p["tmode"] == "tape":
        tone, ftrack = _tape_tone(p, n, u, ph)
    else:
        ftrack = p["tf0"] * 2.0 ** (-p["toct"] * v)
        tone = unison(ftrack, p["wave"], p["detune"], 0.5 * p["width"], ph)
    if p["wave"] in ("saw", "square"):
        tone = svf(tone, np.clip(ftrack * p["tbright"], 200, 18000), 0.9, "low")
    return bw(tone, "high", p["hp"], 2)


def _sub(p: dict, n: int, t: np.ndarray, u: np.ndarray, att: np.ndarray) -> np.ndarray:
    fs = p["sub_f0"] * (p["sub_f1"] / p["sub_f0"]) ** (1 - (1 - u) ** 2.2)
    s = dsp.osc(fs, "sine") * att * np.exp(-u / 0.22) * np.clip((1 - u) / 0.2, 0, 1) ** 2
    s = s + 0.15 * np.tanh(3 * s)
    return bw(s, "high", 25, 2)


def _at(db: float, x: np.ndarray) -> np.ndarray:
    return 10 ** (db / 20) * x / lvl(x)


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """One downlifter take, (n, 2): the dry fall, then its reverb tail, trimmed at -62 dB."""
    q = dict(DEFAULTS)
    q.update(p or {})
    p = q
    L = p["length"]
    n = n_of(L)
    t = t_of(n)
    u = t / L
    v = 1 - (1 - u) ** p["curve"]
    att = np.sin(0.5 * np.pi * np.clip(t / (p["att_ms"] / 1000), 0, 1)) ** 2
    lv = att * 10 ** (-p["drop_db"] * v / 20) * np.clip((1 - u) / 0.12, 0, 1) ** 1.5
    noise, fc = _noise(p, n, v, rng)
    tone = _tone(p, n, u, v, rng)
    dry = _at(p["noise_db"], noise) + _at(p["tone_db"], tone)
    if p["shep_db"] > -47:
        sh = dsp.shepard(L, rng, up=False, rate=p["shep_rate"], f_lo=60.0, f_hi=9000.0)[:n]
        sh = svf(bw(sh, "high", p["hp"], 2), np.minimum(1.3 * fc, 18000), 0.7, "low")
        dry = dry + _at(p["shep_db"], fx.width(fx.decorrelate(sh, 0.6), 0.6 * p["width"]))
    dry = dry * lv[:, None]
    tail = n_of(min(1.5, 0.6 * p["rt60"]))
    out = np.zeros((n + tail, 2))
    out[:n] = dry
    if p["sub_db"] > -47:
        out[:n] += _at(p["sub_db"], stereo_of(_sub(p, n, t, u, att)))
    if p["verb"] > 0.005:
        wet = wet_of(dry, p["rt60"], n + tail, size=0.8, predelay=0.015, lowcut=150.0)
        out = out + p["verb"] * wet / lvl(wet) * lvl(dry)
    if p["sat"] > 0.1:
        out = fx.saturate(out, p["sat"])
    out = bw(out, "high", 25, 2)
    out = dsp.trim_tail(out, -62.0, 0.02)
    return headroom(dsp.fade(out, 0.0015, min(0.12, 0.25 * len(out) / SR)))


def measure(x: np.ndarray) -> dict:
    """Model-free downlifter measurements (6.6)."""
    e = env_db(x)
    k = len(e)
    ip = int(np.argmax(e))
    pk = float(e[ip])
    active = np.nonzero(e > pk - 45)[0]
    end = int(active[-1]) if len(active) else k - 1
    third = ip + max(1, (end - ip) // 3)
    sm = np.convolve(e, np.ones(5) / 5, mode="same")
    fall = max(pk - float(sm[end]), 0.001)
    cent, power = centroid_track(x)
    loud = cent[power > 1e-3 * (power.max() if power.size else 0)]
    cent_oct = 0.0 if len(loud) < 6 else float(np.log2(np.median(loud[-3:]) / max(np.median(loud[:3]), 1.0)))
    m = x.mean(axis=1) if x.ndim == 2 else x
    return {
        "length_s": round(len(x) / SR, 3),
        "peak_frac": ip / k,
        "sag": (pk - float(sm[min(third, k - 1)])) / fall,
        "cent_oct": cent_oct,
        "tail_db": 20 * np.log10(np.sqrt(np.mean(m[-480:] ** 2)) + 1e-12) - pk,
        "side_db": side_db(x),
        "low_db": low_share_db(x),
    }


def target_check(x: np.ndarray, row=None) -> float:
    m = measure(x)
    pen = 6 * max(0.0, m["peak_frac"] - 0.15)
    pen += max(0.0, -1 + m["cent_oct"]) if m["cent_oct"] > -1 else 0.0
    pen += 3 * max(0.0, 0.3 - m["sag"])
    pen += max(0.0, m["tail_db"] + 45) / 8
    pen += max(0.0, m["side_db"] + 4) / 6
    return -min(3.0, pen)
