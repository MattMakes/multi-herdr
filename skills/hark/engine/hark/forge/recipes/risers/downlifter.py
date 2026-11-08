"""downlifter: a sinking filtered sweep for the moment after an impact (the riser in reverse)."""

from __future__ import annotations

import types

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
FAMILIES = {label: {kn: (v if isinstance(v, str) else float(v)) for kn, v in zip(_KEYS, cols)}
            for label, cols in _COLS.items()}
DEFAULTS = dict(FAMILIES["air_fall"])


def comb(wav: np.ndarray, delay_s: np.ndarray, mix: float) -> np.ndarray:
    """y = (x + mix x(t - d(t))) / (1 + mix); the delayed read is linearly interpolated, 0 before the start."""
    ns = len(wav)
    pos = np.arange(ns) - np.asarray(delay_s, float) * SR
    at = np.floor(pos).astype(np.int64)
    fcurve = pos - at
    ok0 = (at >= 0) & (at < ns)
    ok1 = (at + 1 >= 0) & (at + 1 < ns)
    amp = np.where(ok0, wav[np.clip(at, 0, ns - 1)], 0.0)
    bv = np.where(ok1, wav[np.clip(at + 1, 0, ns - 1)], 0.0)
    return (wav + mix * ((1 - fcurve) * amp + fcurve * bv)) / (1 + mix)


def _noise(cfg: dict, ns: int, v: np.ndarray, gen) -> tuple[np.ndarray, np.ndarray]:
    pink = dsp.noise(ns / SR, gen, "pink")[:ns]
    srcv = (1 - cfg["pink"]) * gen.standard_normal(ns) + cfg["pink"] * 1.6 * pink
    cut = cfg["nf0"] * (cfg["nf1"] / cfg["nf0"]) ** v
    sig = svf(srcv, cut, cfg["nq"], "low")
    sig = bw(bw(sig, "high", cfg["hp"], 2), "low", 17000, 4)
    if cfg["flange"] > 0.01:
        rate = cfg["fl_rate"] * (1 - 0.66 * v)
        phase = np.cumsum(rate) / SR
        dv = 0.0004 + 0.0086 * (0.5 - 0.5 * np.cos(2 * np.pi * phase))
        sig = comb(sig, dv, cfg["flange"])
    return fx.width(fx.decorrelate(sig, 1.0), cfg["width"]), cut


def _tape_tone(cfg: dict, ns: int, prog: np.ndarray, ph: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    mv = ns + n_of(0.05)
    chord = np.zeros((mv, 2))
    for kn, (ratio, gain) in enumerate(((1.0, 1.0), (1.5, 0.6), (2.0, 0.5))):
        freq = np.full(ns, cfg["tf0"] * ratio)
        chord[:ns] += gain * unison(freq, cfg["wave"], cfg["detune"], 0.5 * cfg["width"], np.roll(ph, kn))
    sp = np.maximum((1 - prog) ** cfg["tape_curve"], 0)
    pos = np.concatenate([[0.0], np.cumsum(sp)[:-1]])
    tone = np.column_stack([np.interp(pos, np.arange(mv), chord[:, cv]) for cv in range(2)])
    return tone, cfg["tf0"] * np.maximum(sp, 0.02)


def _tone(cfg: dict, ns: int, prog: np.ndarray, v: np.ndarray, gen) -> np.ndarray:
    ph = gen.random(5)
    if cfg["tmode"] == "tape":
        tone, ftrack = _tape_tone(cfg, ns, prog, ph)
    else:
        ftrack = cfg["tf0"] * 2.0 ** (-cfg["toct"] * v)
        tone = unison(ftrack, cfg["wave"], cfg["detune"], 0.5 * cfg["width"], ph)
    if cfg["wave"] in ("saw", "square"):
        tone = svf(tone, np.clip(ftrack * cfg["tbright"], 200, 18000), 0.9, "low")
    return bw(tone, "high", cfg["hp"], 2)


def _sub(cfg: dict, ns: int, tax: np.ndarray, prog: np.ndarray, atk: np.ndarray) -> np.ndarray:
    fs = cfg["sub_f0"] * (cfg["sub_f1"] / cfg["sub_f0"]) ** (1 - (1 - prog) ** 2.2)
    sv = dsp.osc(fs, "sine") * atk * np.exp(-prog / 0.22) * np.clip((1 - prog) / 0.2, 0, 1) ** 2
    sv = sv + 0.15 * np.tanh(3 * sv)
    return bw(sv, "high", 25, 2)


def _at(db: float, wav: np.ndarray) -> np.ndarray:
    return 10 ** (db / 20) * wav / lvl(wav)


def _curves(cfg: dict) -> types.SimpleNamespace:
    """Sample count, time, progress u, the slowing curve v, the attack and the dry level of 6.5 steps 1 to 3."""
    ns = n_of(cfg["length"])
    tax = t_of(ns)
    prog = tax / cfg["length"]
    atk = np.sin(0.5 * np.pi * np.clip(tax / (cfg["att_ms"] / 1000), 0, 1)) ** 2
    v = 1 - (1 - prog) ** cfg["curve"]
    lvl0 = atk * 10 ** (-cfg["drop_db"] * v / 20) * np.clip((1 - prog) / 0.12, 0, 1) ** 1.5
    return types.SimpleNamespace(n=ns, t=tax, u=prog, v=v, att=atk, lv=lvl0)


def _shepard_fall(cfg: dict, cv: types.SimpleNamespace, cut: np.ndarray, gen) -> np.ndarray:
    shv = dsp.shepard(cfg["length"], gen, up=False, rate=cfg["shep_rate"], f_lo=60.0, f_hi=9000.0)[:cv.n]
    shv = svf(bw(shv, "high", cfg["hp"], 2), np.minimum(1.3 * cut, 18000), 0.7, "low")
    return _at(cfg["shep_db"], fx.width(fx.decorrelate(shv, 0.6), 0.6 * cfg["width"]))


def _dry(cfg: dict, cv: types.SimpleNamespace, gen) -> np.ndarray:
    """6.5 step 11: noise, tone and shepard layers on the dry level curve."""
    noise, cut = _noise(cfg, cv.n, cv.v, gen)
    drysig = _at(cfg["noise_db"], noise) + _at(cfg["tone_db"], _tone(cfg, cv.n, cv.u, cv.v, gen))
    if cfg["shep_db"] > -47:
        drysig = drysig + _shepard_fall(cfg, cv, cut, gen)
    return drysig * cv.lv[:, None]


def _tail_room(cfg: dict, cv: types.SimpleNamespace, drysig: np.ndarray) -> np.ndarray:
    """The dry fall and the sub in a buffer with room for the tail, plus the reverb of the dry fall."""
    res = np.zeros((cv.n + n_of(min(1.5, 0.6 * cfg["rt60"])), 2))
    res[:cv.n] = drysig
    if cfg["sub_db"] > -47:
        res[:cv.n] += _at(cfg["sub_db"], stereo_of(_sub(cfg, cv.n, cv.t, cv.u, cv.att)))
    if cfg["verb"] > 0.005:
        wetv = wet_of(drysig, cfg["rt60"], len(res), size=0.8, predelay=0.015, lowcut=150.0)
        res = res + cfg["verb"] * wetv / lvl(wetv) * lvl(drysig)
    return res


def render(cfg: dict, gen: np.random.Generator) -> np.ndarray:
    """One downlifter take, (n, 2): the dry fall, then its reverb tail, trimmed at -62 dB."""
    cfg = {**DEFAULTS, **(cfg or {})}
    cv = _curves(cfg)
    res = _tail_room(cfg, cv, _dry(cfg, cv, gen))
    if cfg["sat"] > 0.1:
        res = fx.saturate(res, cfg["sat"])
    res = dsp.trim_tail(bw(res, "high", 25, 2), -62.0, 0.02)
    return headroom(dsp.fade(res, 0.0015, min(0.12, 0.25 * len(res) / SR)))


def _shape(env: np.ndarray) -> tuple[float, float]:
    """(peak_frac, sag) of 6.6: how early the peak is, and how much of the fall is done after a third."""
    kn = len(env)
    ip = int(np.argmax(env))
    active = np.nonzero(env > env[ip] - 45)[0]
    end = int(active[-1]) if len(active) else kn - 1
    sm = np.convolve(env, np.ones(5) / 5, mode="same")
    third = min(ip + max(1, (end - ip) // 3), kn - 1)
    return ip / kn, float((env[ip] - sm[third]) / max(env[ip] - sm[end], 0.001))


def _cent_oct(wav: np.ndarray) -> float:
    cent, power = centroid_track(wav)
    loud = cent[power > 1e-3 * (power.max() if power.size else 0)]
    return 0.0 if len(loud) < 6 else float(np.log2(np.median(loud[-3:]) / max(np.median(loud[:3]), 1.0)))


def _tail_db(wav: np.ndarray, peak: float) -> float:
    mv = wav.mean(axis=1) if wav.ndim == 2 else wav
    return float(20 * np.log10(np.sqrt(np.mean(mv[-480:] ** 2)) + 1e-12) - peak)


def measure(wav: np.ndarray) -> dict:
    """Model-free downlifter measurements (6.6)."""
    env = env_db(wav)
    peak_frac, sag = _shape(env)
    return {"length_s": round(len(wav) / SR, 3), "peak_frac": peak_frac, "sag": sag, "cent_oct": _cent_oct(wav),
            "tail_db": _tail_db(wav, float(env.max())), "side_db": side_db(wav), "low_db": low_share_db(wav)}


def target_check(wav: np.ndarray, row=None) -> float:
    mv = measure(wav)
    pen = 6 * max(0.0, mv["peak_frac"] - 0.15)
    pen += max(0.0, -1 + mv["cent_oct"]) if mv["cent_oct"] > -1 else 0.0
    pen += 3 * max(0.0, 0.3 - mv["sag"])
    pen += max(0.0, mv["tail_db"] + 45) / 8
    pen += max(0.0, mv["side_db"] + 4) / 6
    return -min(3.0, pen)
