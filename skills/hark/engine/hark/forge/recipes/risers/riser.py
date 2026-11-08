"""riser: a designed build into a hit; the last sample is the moment of the cut.

Two render paths: rounds 1 to 4 (``render``) and the round-6 build (``render_build``, chosen by ``r6 > 0.5``).
The module also holds the helpers that downlifter and zip import.
"""

from __future__ import annotations

import functools
import types

import numpy as np
from scipy.signal import fftconvolve

from hark.forge.engine import dsp, fx
from hark.forge.recipes.risers._common import SR, bw, fit, headroom, n_of, smooth01, stereo2, svf, t_of

TARGET = "a cinematic riser building tension into a hit"
ALTS = ["a jet engine", "an air raid siren", "heavy rain on a roof", "a vacuum cleaner"]
REFSET = "riser"
TARGETS = {"riser": TARGET, "pro_riser": "a short stuttering digital riser into a cut"}

UNISON = (-1.0, -0.45, 0.0, 0.45, 1.0)
FINAL_OPEN = 0.045

SPACE = {
    "length": (0.8, 4.0), "curve": (1.0, 4.0), "rise_db": (12.0, 45.0), "noise_db": (-36.0, 0.0),
    "nf0": (150.0, 1500.0, "log"), "nf1": (2500.0, 16000.0, "log"), "nq": (0.7, 4.0, "log"),
    "pink": (0.0, 1.0), "tone_db": (-36.0, 0.0), "tmode": ["glide", "blips", "nblips"],
    "wave": ["sine", "tri", "saw"], "tf0": (60.0, 4000.0, "log"), "toct": (0.5, 3.0),
    "tbright": (1.5, 12.0, "log"), "detune": (0.0, 30.0), "oct_db": (-36.0, -3.0),
    "shep_db": (-48.0, 0.0), "shep_rate": (0.3, 1.6), "drone_db": (-48.0, -3.0),
    "drone_f": (35.0, 110.0, "log"), "metal_db": (-48.0, 0.0), "metal_f": (1500.0, 5000.0, "log"),
    "gate_depth": (0.0, 1.0), "gate_on": (0.0, 0.7), "gate_p0": (0.02, 0.5, "log"),
    "gate_p1": (0.012, 0.08, "log"), "gate_duty": (0.25, 0.8), "gate_sharp": (0.0, 1.0),
    "gate_drone": (0.0, 1.0), "blip_dec": (0.1, 0.5), "verb": (0.0, 0.6), "rt60": (0.6, 3.5, "log"),
    "width": (0.0, 1.0), "sat": (0.0, 12.0), "hp": (60.0, 500.0, "log"), "end_ms": (2.0, 20.0, "log"),
    "drone_oct": (0.0, 1.5), "drone_noise": (0.0, 1.0), "metal_n": (9, 40, "int"),
    "metal_glide": (0.0, 1.0), "nb_q": (2.0, 16.0, "log"),
}

SPACE_R6 = {
    "r6": (0.0, 1.0), "nbw": (0.8, 4.0), "fifth_db": (-48.0, 0.0), "tone_gate": (0.0, 1.0),
    "gate_pow": (0.6, 3.0), "rswell_db": (-48.0, 0.0), "rswell_rt": (1.0, 4.0), "pre_dip_ms": (0.0, 60.0),
}
R6_OFF = {"r6": 0.0, "nbw": 2.0, "fifth_db": -48.0, "tone_gate": 1.0, "gate_pow": 1.4,
          "rswell_db": -48.0, "rswell_rt": 2.0, "pre_dip_ms": 0.0}

# ---------------------------------------------------------------- presets (5.4)

_KEYS = tuple(("length curve rise_db noise_db nf0 nf1 nq pink tone_db tmode wave tf0 toct tbright detune oct_db "
         "shep_db shep_rate drone_db drone_f metal_db metal_f gate_depth gate_on gate_p0 gate_p1 gate_duty "
         "gate_sharp gate_drone blip_dec verb rt60 width sat hp end_ms").split())

_FAM_COLS = {
    "air": (2.6, 2.4, 36, 0, 300, 12000, 1.6, 0.55, -20, "glide", "tri", 400, 1.5, 4, 12, -30, -48, 0.8, -30,
            55, -48, 2600, 0, 0.5, 0.2, 0.04, 0.5, 0, 0, 0.35, 0.3, 2.0, 0.7, 2, 180, 6),
    "tension": (3.2, 2.6, 32, -10, 250, 9000, 1.2, 0.7, 0, "glide", "saw", 110, 2.0, 5, 18, -10, -48, 0.8, -14,
                55, -48, 2600, 0.55, 0.25, 0.25, 0.06, 0.5, 0.2, 0, 0.35, 0.3, 2.4, 0.5, 5, 120, 5),
    "stutter": (0.9, 1.6, 16, -20, 600, 12000, 1.0, 0.3, 0, "blips", "sine", 1700, 2.6, 4, 6, -24, -48, 0.8,
                -12, 95, -48, 2600, 1.0, 0, 0.055, 0.036, 0.5, 1.0, 1.0, 0.2, 0.08, 1.0, 0.3, 4, 250, 4),
    "shepard": (3.8, 1.8, 30, -12, 200, 7000, 2.2, 0.9, -36, "glide", "sine", 300, 1.0, 3, 8, -36, 0, 0.9, -22,
                45, -48, 2600, 0, 0.5, 0.2, 0.04, 0.5, 0, 0, 0.35, 0.45, 3.0, 0.9, 1.5, 140, 8),
    "rush": (1.5, 3.2, 30, 0, 500, 15000, 2.4, 0.4, -10, "glide", "sine", 260, 2.8, 6, 10, -20, -48, 0.8, -22,
             82, -48, 2600, 0.5, 0.1, 0.1, 0.028, 0.5, 0.1, 0, 0.35, 0.22, 1.4, 0.5, 2, 200, 4),
    "cymbal": (2.2, 2.8, 40, -4, 900, 16000, 0.8, 0.25, -36, "glide", "tri", 400, 1.5, 4, 12, -36, -48, 0.8,
               -48, 55, 0, 2400, 0, 0.5, 0.2, 0.04, 0.5, 0, 0, 0.35, 0.35, 1.6, 0.85, 1, 400, 4),
}
_ALWAYS = {"drone_oct": 0.08, "drone_noise": 0.0, "metal_n": 9, "metal_glide": 0.0, "nb_q": 6.0}


def _member(values: tuple) -> dict:
    cfg = {kn: (float(v) if not isinstance(v, str) else v) for kn, v in zip(_KEYS, values)}
    cfg.update(_ALWAYS)
    return cfg


FAMILIES = {label: _member(cols) for label, cols in _FAM_COLS.items()}
DEFAULTS = dict(FAMILIES["air"])

_ST = FAMILIES["stutter"]


def _over(basev: dict, **extra) -> dict:
    res = dict(basev)
    res.update(extra)
    return res


PRO = {
    "blips": dict(_ST),
    "chop": _over(_ST, length=1.0, tmode="glide", wave="tri", tone_db=-14, noise_db=0, nf0=700, nf1=14000, nq=2.0,
                  gate_p0=0.04, gate_p1=0.02, gate_duty=0.45, drone_db=-20, hp=500, rise_db=26, curve=2.2,
                  verb=0.05),
    "gated_saw": _over(_ST, length=1.4, tmode="glide", wave="saw", tf0=300, toct=2.0, tbright=3.0, detune=14,
                       noise_db=-12, gate_p0=0.06, gate_p1=0.022, gate_duty=0.6, drone_db=-14, drone_f=60, hp=150,
                       sat=3, rise_db=26, curve=2.2, verb=0.06),
    "roll": _over(_ST, length=1.3, curve=2.4, tmode="glide", tone_db=-36, oct_db=-36, noise_db=0, nf0=1200,
                  nf1=12000, nq=0.9, pink=0.6, gate_p0=0.085, gate_p1=0.022, gate_duty=0.4, gate_sharp=1.0,
                  drone_db=-24, gate_drone=1.0, hp=300, verb=0.1, rise_db=28),
    "blips_hi": _over(_ST, length=0.8, tf0=2600, toct=2.2, gate_p0=0.045, gate_p1=0.028, drone_db=-18,
                      drone_f=110, noise_db=-14, rise_db=20),
}

# Round-2 tuned values (5.6), one tuple per member in _R2_KEYS order.
_R2_KEYS = _KEYS
_R2 = {
    "air": (2.47, 2.043, 33.76, -2.34, 294.7, 10379, 1.856, 0.646, -22.34, "glide", "tri", 440.8, 1.225, 3.63,
            13.64, -29.46, -43.4, 0.778, -32.06, 50.92, -46.76, 2615.8, 0.039, 0.584, 0.173, 0.041, 0.506, 0.058,
            0.103, 0.35, 0.259, 1.765, 0.586, 0.861, 193.6, 5.46),
    "tension": (2.816, 2.681, 35.24, -9.59, 214.0, 7710.6, 1.253, 0.814, -3.34, "glide", "saw", 76.65, 1.767,
                6.06, 20.45, -10.69, -45.8, 0.765, -16.86, 53.29, -45.95, 2641.2, 0.558, 0.264, 0.254, 0.058,
                0.493, 0.213, 0.052, 0.35, 0.286, 2.495, 0.522, 5.94, 109.6, 5.56),
    "shepard": (3.708, 2.026, 30.53, -8.98, 206.5, 8656.4, 2.572, 0.974, -35.18, "glide", "sine", 260.0, 1.046,
                2.34, 9.42, -32.85, -4.73, 0.887, -22.49, 51.44, -44.93, 2843.6, 0.048, 0.578, 0.173, 0.049,
                0.483, 0.018, 0.098, 0.35, 0.415, 2.889, 0.909, 0.291, 108.6, 10.47),
    "rush": (1.756, 3.117, 32.74, -4.32, 468.2, 12035, 2.54, 0.344, -8.74, "glide", "sine", 397.2, 2.516, 5.63,
             9.84, -19.99, -42.3, 0.889, -24.66, 76.65, -43.25, 2909.5, 0.497, 0.074, 0.146, 0.026, 0.458, 0.188,
             0.092, 0.364, 0.292, 1.687, 0.565, 3.2, 171.7, 3.32),
    "cymbal": (2.531, 2.469, 41.5, -3.21, 730.3, 15030, 0.704, 0.312, -33.79, "glide", "tri", 542.9, 1.24, 3.7,
               11.5, -32.17, -44.76, 0.725, -46.7, 61.19, -4.46, 2153.3, 0.035, 0.514, 0.209, 0.046, 0.508,
               0.035, 0.05, 0.35, 0.396, 1.688, 0.96, 0.901, 403.8, 4.21),
}
_R3_FIX = {
    "air": {"drone_oct": 0.9, "drone_noise": 0.7},
    "tension": {"drone_oct": 1.0, "drone_noise": 0.5, "drone_db": -20.0},
    "shepard": {"drone_oct": 0.7, "drone_noise": 0.8},
    "rush": {"drone_oct": 0.6, "drone_noise": 0.3, "drone_db": -28.0},
    "cymbal": {"metal_n": 9, "metal_glide": 0.6, "metal_db": -6.0, "drone_db": -48.0, "width": 0.75},
}


def _by_cat_riser(basev: dict, tuned: tuple, fix: dict) -> dict:
    """A round-1 member, then its round-2 tuned values, then its round-3 fix."""
    cfg = dict(basev)
    cfg.update({kn: (float(v) if not isinstance(v, str) else v) for kn, v in zip(_R2_KEYS, tuned)})
    cfg.update(fix)
    return cfg


_PRO_COMMON = {"metal_n": 9, "metal_glide": 0.0}
FAMILIES_BY_CAT = {
    "riser": {label: _by_cat_riser(FAMILIES[label], _R2[label], _R3_FIX[label])
              for label in ("air", "tension", "shepard", "rush", "cymbal")},
    "pro_riser": {
        "nblips": _over(_ST, length=1.0, tmode="nblips", nb_q=7, tf0=1700, toct=2.4, tone_db=0, noise_db=-10,
                        nf0=900, nf1=13000, nq=1.2, gate_p0=0.05, gate_p1=0.024, gate_duty=0.5, blip_dec=0.25,
                        drone_db=-16, drone_noise=1.0, drone_oct=0.3, gate_drone=1.0, oct_db=-20, rise_db=20,
                        curve=1.8, verb=0.03, sat=2, **_PRO_COMMON),
        "chop": _over(PRO["chop"], tone_db=-24, drone_noise=1.0, drone_oct=0.3, nq=2.4, gate_p0=0.045,
                      gate_p1=0.02, sat=2, drone_db=-18, nb_q=6, **_PRO_COMMON),
        "roll": _over(PRO["roll"], drone_noise=1.0, drone_oct=0.3, nb_q=6, **_PRO_COMMON),
        "air_gate": _over(_ST, length=1.2, tmode="glide", wave="saw", tf0=400, toct=2.0, tbright=2.0, tone_db=-16,
                          noise_db=0, nf0=500, nf1=12000, nq=3.0, pink=0.5, gate_depth=0.85, gate_on=0.1,
                          gate_p0=0.07, gate_p1=0.022, gate_duty=0.55, gate_sharp=0.6, drone_db=-18,
                          drone_noise=1.0, drone_oct=0.5, gate_drone=0.8, hp=200, sat=2, rise_db=26, curve=2.2,
                          verb=0.12, width=0.4, nb_q=6, **_PRO_COMMON),
        "nblips_hi": _over(_ST, length=0.85, tmode="nblips", nb_q=10, tf0=2600, toct=2.1, tone_db=0, noise_db=-12,
                           nf0=1500, nf1=14000, gate_p0=0.045, gate_p1=0.026, blip_dec=0.22, drone_db=-20,
                           drone_f=110, drone_noise=1.0, drone_oct=0.3, gate_drone=1.0, rise_db=20, verb=0.03,
                           sat=2, **_PRO_COMMON),
    },
}

R4_EDITS = {"riser": [
    {"drone_db": -48.0, "hp": 220.0, "nq": 1.2, "curve": 2.6},
    {"drone_db": -48.0, "tf0": 150.0, "toct": 1.8, "hp": 200.0, "sat": 3.0, "detune": 24.0, "noise_db": -5.0,
     "curve": 2.2},
    {"drone_db": -48.0, "hp": 180.0, "curve": 2.4},
    {"drone_db": -48.0, "hp": 220.0, "nq": 1.6},
    {"drone_db": -48.0, "metal_glide": 0.2},
]}

_B6 = _over(FAMILIES["air"], metal_db=-48.0, metal_f=2600.0, blip_dec=0.35, drone_oct=0.0, drone_noise=0.0,
            metal_n=9, metal_glide=0.0, nb_q=6.0, drone_db=-48.0, drone_f=55.0, shep_db=-48.0, gate_drone=0.0,
            tmode="glide", sat=1.0, nq=0.7)
_B6.update(R6_OFF)
_B6["r6"] = 1.0

_R6_KEYS = ("length curve rise_db noise_db nf0 nf1 nbw pink tone_db fifth_db oct_db gate_depth gate_on gate_p0 "
            "gate_p1 gate_duty gate_sharp gate_pow tone_gate verb rt60 width hp end_ms rswell_db").split()
_R6_COLS = {
    "air": (2.4, 2.6, 34, 0, 900, 13000, 2.2, 0.5, -16, -8, -26, 0.6, 0.15, 0.25, 0.034, 0.55, 0.3, 1.6, 0.6,
            0.3, 2.0, 0.6, 300, 5, -14),
    "tension": (2.8, 2.4, 30, -9, 1200, 11000, 1.8, 0.6, 0, -4, -12, 0.75, 0.1, 0.17, 0.025, 0.5, 0.55, 1.5, 1.0,
                0.25, 2.0, 0.5, 260, 4, -48),
    "shepard": (3.4, 2.2, 30, -8, 700, 9000, 2.6, 0.9, -36, -48, -36, 0.45, 0.2, 0.5, 0.07, 0.5, 0.0, 1.3, 0.8,
                0.45, 3.0, 0.85, 260, 8, -20),
    "rush": (1.3, 2.2, 26, -3, 1500, 15000, 1.6, 0.35, -4, -6, -18, 0.85, 0.0, 0.06, 0.021, 0.5, 0.8, 1.2, 1.0,
             0.15, 1.2, 0.4, 400, 3, -48),
    "swell": (2.2, 2.8, 38, -8, 2000, 16000, 1.4, 0.3, -22, -6, -30, 0.35, 0.5, 0.12, 0.03, 0.5, 0.2, 2.0, 0.5,
              0.2, 1.6, 0.8, 500, 4, 0),
}
_R6_EXTRA = {
    "air": {"wave": "tri", "tf0": 700.0, "toct": 2.0, "detune": 14.0, "rswell_rt": 2.2},
    "tension": {"wave": "saw", "tf0": 300.0, "toct": 2.0, "tbright": 4.0, "detune": 22.0},
    "shepard": {"shep_db": 0.0, "shep_rate": 0.9, "rswell_rt": 3.0},
    "rush": {"wave": "sine", "tf0": 1000.0, "toct": 2.2, "detune": 10.0},
    "swell": {"wave": "sine", "tf0": 1500.0, "toct": 1.6, "detune": 8.0, "rswell_rt": 2.6},
}


def _r6_member(label: str) -> dict:
    cfg = dict(_B6)
    cfg.update({kn: float(v) for kn, v in zip(_R6_KEYS, _R6_COLS[label])})
    cfg.update(_R6_EXTRA[label])
    return cfg


R6 = {label: _r6_member(label) for label in _R6_COLS}

# ---------------------------------------------------------------- shared helpers (5.10)


def lvl(wav: np.ndarray, frac: float | None = None) -> float:
    """P8 riser variant: peak 10 ms block RMS of the mono fold (optionally of its last frac) + 1e-12."""
    mv = wav.mean(axis=1) if wav.ndim == 2 else wav
    if frac is not None:
        mv = mv[len(mv) - max(1, int(round(frac * len(mv)))):]
    kn = max(1, len(mv) // 480)
    blocks = mv[:kn * 480].reshape(kn, -1) if len(mv) >= 480 else mv.reshape(1, -1)
    return float(np.sqrt(np.mean(blocks ** 2, axis=1)).max()) + 1e-12


def phase_from_end(rate: np.ndarray, end_phase: float) -> np.ndarray:
    """P15: phase in cycles, laid out so the last sample sits at end_phase."""
    cv = np.cumsum(np.asarray(rate, float)[::-1])[::-1] / SR
    return end_phase - cv


def gate_shape(phase: np.ndarray, period, duty: float = 0.5, sharp: float = 1.0, ramp: float = 0.0012,
               perc: float = 0.0) -> np.ndarray:
    """P16: a tremolo blended with a square gate (or a percussive gate when perc > 0)."""
    fcurve = np.mod(phase, 1.0)
    trem = 0.5 + 0.5 * np.cos(2 * np.pi * (fcurve - duty / 2))
    onv = np.clip(fcurve * period / ramp, 0, 1)
    if perc > 0:
        square = onv * np.exp(-fcurve / max(perc, 0.001)) * np.clip((0.98 - fcurve) * period / ramp, 0, 1)
    else:
        square = np.minimum(onv, np.clip((duty - fcurve) * period / (1.5 * ramp), 0, 1))
    return (1 - sharp) * trem + sharp * square


def unison(freq, wave: str, detune, wd: float, phases) -> np.ndarray:
    """P20: 5 detuned voices panned across the stereo field; returns (n, 2)."""
    res = None
    for kn, o in enumerate(UNISON):
        f = np.asarray(freq, float) * 2.0 ** (o * np.asarray(detune, float) / 1200.0)
        v = dsp.osc(f, wave, phase=float(phases[kn]))
        th = (np.clip(o * wd, -1, 1) + 1) * np.pi / 4
        stv = np.column_stack([v * np.cos(th), v * np.sin(th)])
        res = stv if res is None else res + stv
    return res * np.sqrt(2) / 5


@functools.lru_cache(maxsize=32)
def _ir(rtime: float, size: float, predelay: float, damp: float, lowcut: float) -> np.ndarray:
    return fx.reverb_ir(dsp.rng(1234), rt60=rtime, size=size, predelay=predelay, damp=damp, inputs=2,
                        lowcut=lowcut, seed=1234)


def verb_ir(rtime: float, size: float = 0.75, predelay: float = 0.02, damp: float = 0.5,
            lowcut: float = 200.0) -> np.ndarray:
    """P22: the fixed-seed 2-input reverb IR, (m, 2, 2), cached per rounded setting."""
    return _ir(round(float(rtime), 1), round(float(size), 3), round(float(predelay), 4), round(float(damp), 3),
               round(float(lowcut), 1)).copy()


def wet_of(wav: np.ndarray, rtime: float, n_out: int | None = None, **extra) -> np.ndarray:
    """P22: the reverb return alone (no dry), each input channel feeding both outputs."""
    wav = stereo2(wav)
    ir = verb_ir(rtime, **extra)
    ns = len(wav) + len(ir) - 1
    res = np.zeros((ns, 2))
    for cv in range(2):
        for o in range(2):
            res[:, o] += fftconvolve(wav[:, cv], ir[:, cv, o])
    return fit(res, n_out if n_out is not None else ns)


def stereo_of(wav: np.ndarray) -> np.ndarray:
    return stereo2(wav)


def env_db(wav: np.ndarray, hop_ms: float = 10) -> np.ndarray:
    mv = wav.mean(axis=1) if wav.ndim == 2 else wav
    hv = max(1, int(SR * hop_ms / 1000))
    kn = max(1, len(mv) // hv)
    seg = mv[:kn * hv].reshape(kn, hv) if len(mv) >= hv else mv.reshape(1, -1)
    return 20 * np.log10(np.sqrt(np.mean(seg ** 2, axis=1)) + 1e-9)


def band_env_db(wav: np.ndarray, lo: float, hiv: float, hop_ms: float = 10) -> np.ndarray:
    if lo <= 20:
        sig = bw(wav, "low", hiv, 4)
    else:
        sig = bw(wav, "band", (lo, min(hiv, 23000)), 4)
    return env_db(sig, hop_ms)


def side_db(wav: np.ndarray) -> float:
    if wav.ndim == 1:
        return -99.0
    mid = (wav[:, 0] + wav[:, 1]) / 2
    sidev = (wav[:, 0] - wav[:, 1]) / 2
    return float(10 * np.log10((np.sum(sidev ** 2) + 1e-20) / (np.sum(mid ** 2) + 1e-20)))


def low_share_db(wav: np.ndarray, f: float = 100) -> float:
    mv = wav.mean(axis=1) if wav.ndim == 2 else wav
    lo = bw(mv, "low", f, 4)
    return float(10 * np.log10((np.sum(lo ** 2) + 1e-20) / (np.sum(mv ** 2) + 1e-20)))


def centroid_track(wav: np.ndarray, hop_ms: float = 20) -> tuple[np.ndarray, np.ndarray]:
    mv = wav.mean(axis=1) if wav.ndim == 2 else wav
    hv = max(2, int(SR * hop_ms / 1000))
    kn = len(mv) // hv
    if kn < 1:
        return np.zeros(0), np.zeros(0)
    frames = mv[:kn * hv].reshape(kn, hv) * np.hanning(hv)
    P = np.abs(np.fft.rfft(frames, axis=1)) ** 2
    f = np.fft.rfftfreq(hv, 1 / SR)
    power = P.sum(axis=1)
    cent = (P * f).sum(axis=1) / (power + 1e-20)
    return cent, power


# ---------------------------------------------------------------- render pieces


def _layer_at(db: float, wav: np.ndarray, frac: float | None = 0.25) -> np.ndarray:
    """AT(db, x): the layer at db relative to its own mix level (last quarter by default)."""
    return 10 ** (db / 20) * wav / lvl(wav, frac)


def _gate_parts(cfg: dict, tax: np.ndarray, prog: np.ndarray, power: float):
    """Gate period, phase, value and depth of 5.9 steps 2 and 3."""
    per = cfg["gate_p0"] * (cfg["gate_p1"] / cfg["gate_p0"]) ** (prog ** power)
    phase = phase_from_end(1.0 / per, 0.5 * cfg["gate_duty"])
    g = gate_shape(phase, per, cfg["gate_duty"], cfg["gate_sharp"])
    fin = np.clip((cfg["length"] - tax) / FINAL_OPEN, 0, 1)
    depth = cfg["gate_depth"] * smooth01(np.clip((prog - cfg["gate_on"]) / 0.2, 0, 1)) * smooth01(fin)
    return per, phase, g, depth, fin


def _frame(cfg: dict, gate_power: float) -> types.SimpleNamespace:
    """Sample count, time, progress u, curve w = u^curve, level lv and the gate of one render."""
    ns = n_of(cfg["length"])
    tax = t_of(ns)
    prog = tax / cfg["length"]
    wv = prog ** cfg["curve"]
    per, phase, g, depth, fin = _gate_parts(cfg, tax, prog, gate_power)
    return types.SimpleNamespace(n=ns, t=tax, u=prog, w=wv, lv=10 ** (-cfg["rise_db"] * (1 - wv) / 20), per=per,
                                 phase=phase, g=g, depth=depth, fin=fin)


def _noise_source(cfg: dict, ns: int, gen: np.random.Generator) -> np.ndarray:
    white = gen.standard_normal(ns)
    pink = dsp.noise(ns / SR, gen, "pink")[:ns]
    return (1 - cfg["pink"]) * white + cfg["pink"] * 1.6 * fit(pink, ns)


def _spread(wav: np.ndarray, amount: float, wv) -> np.ndarray:
    """Decorrelate a mono layer, then set its width."""
    return fx.width(fx.decorrelate(wav, amount), wv)


def _shepard(cfg: dict, ns: int, gen, f_lo: float, up: bool = True) -> np.ndarray:
    shv = fit(dsp.shepard(ns / SR, gen, up=up, rate=cfg["shep_rate"], f_lo=f_lo, f_hi=9000.0), ns)
    return _spread(bw(shv, "high", cfg["hp"], 2), 0.6, 0.6 * cfg["width"])


def _pitch(cfg: dict, prog: np.ndarray) -> np.ndarray:
    wt = prog ** max(1.0, 0.8 * cfg["curve"])
    return cfg["tf0"] * 2.0 ** (cfg["toct"] * wt)


def _saw_lp(wav: np.ndarray, f: np.ndarray, cfg: dict, qv: float, mult: float = 1.0) -> np.ndarray:
    return svf(wav, np.minimum(mult * np.minimum(f * cfg["tbright"], 18000), 18000), qv, "low")


def _finish_tail(mix: np.ndarray, cfg: dict, hp_f: float) -> np.ndarray:
    if cfg["sat"] > 0.1:
        mix = fx.saturate(mix, cfg["sat"])
    mix = bw(mix, "high", hp_f, 2)
    mix = bw(mix, "low", 18000, 4)
    return headroom(dsp.fade(mix, 0.004, cfg["end_ms"] / 1000))


def _add_reverb(mix: np.ndarray, feedv: np.ndarray, cfg: dict, ns: int) -> np.ndarray:
    if cfg["verb"] <= 0.005:
        return mix
    wetv = wet_of(feedv, cfg["rt60"], ns)
    return mix + cfg["verb"] * wetv / lvl(wetv, 0.25) * lvl(feedv, 0.25)


def _octave_rise(prog: np.ndarray) -> np.ndarray:
    """The +12 layer fades in over the second 60 % of the build."""
    return (np.clip((prog - 0.4) / 0.6, 0, 1) ** 2)[:, None]


# ---------------------------------------------------------------- the round-6 build (5.9)


def _build_chord(cfg: dict, prog: np.ndarray, wv: np.ndarray, gen) -> tuple[np.ndarray, np.ndarray]:
    f = _pitch(cfg, prog)
    ph = gen.random(15)
    dtn = cfg["detune"] * (0.6 + 0.8 * wv)
    chord = unison(f, cfg["wave"], dtn, 0.5 * cfg["width"], ph[0:5])
    if cfg["fifth_db"] > -47:
        chord = chord + 10 ** (cfg["fifth_db"] / 20) * unison(1.5 * f, cfg["wave"], dtn, 0.7 * cfg["width"], ph[5:10])
    octave = unison(2 * f, cfg["wave"], 0.7 * dtn, 0.6 * cfg["width"], ph[10:15]) * _octave_rise(prog)
    if cfg["wave"] == "saw":
        chord = _saw_lp(chord, f, cfg, 0.7)
        octave = _saw_lp(octave, f, cfg, 0.7, 2.0)
    return bw(chord, "high", 0.8 * cfg["hp"], 2), bw(octave, "high", cfg["hp"], 2)


def _reverse_swell(cfg: dict, ns: int, gen) -> np.ndarray:
    kn = n_of(0.03)
    burst = bw(gen.standard_normal(kn), "high", 1500, 2) * np.exp(-t_of(kn) / 0.008)
    wetv = wet_of(burst, cfg["rswell_rt"], n_of(cfg["rswell_rt"] + 0.1), predelay=0.0, damp=0.3, lowcut=400.0)[::-1]
    res = np.zeros((ns, 2))
    if len(wetv) >= ns:
        res[:] = wetv[len(wetv) - ns:]
    else:
        res[ns - len(wetv):] = wetv
    return fx.width(bw(res, "high", cfg["hp"], 2), 0.25 + 0.5 * cfg["width"])


def _lp24(wav: np.ndarray, cut) -> np.ndarray:
    """P17 low-pass: 2 cascaded state-variable filters, q 0.54 then 1.31."""
    return svf(svf(wav, cut, 0.54, "low"), cut, 1.31, "low")


def _hp24(wav: np.ndarray, cut) -> np.ndarray:
    return svf(svf(wav, cut, 0.54, "high"), cut, 1.31, "high")


def _noise_band(cfg: dict, f: types.SimpleNamespace, gen) -> np.ndarray:
    """The build's noise: an unresonant band nbw octaves wide whose top moves from nf0 to nf1."""
    cut = cfg["nf0"] * (cfg["nf1"] / cfg["nf0"]) ** f.w
    noise = _hp24(_lp24(_noise_source(cfg, f.n, gen), np.minimum(cut, 18000)), np.maximum(cut / 2 ** cfg["nbw"], 20))
    return _spread(bw(noise, "high", cfg["hp"], 4), 1.0, cfg["width"] * (0.35 + 0.65 * f.w))


def _build_tonal(cfg: dict, f: types.SimpleNamespace, gen) -> np.ndarray:
    chord, octave = _build_chord(cfg, f.u, f.w, gen)
    tonal = _layer_at(cfg["tone_db"], chord) + _layer_at(cfg["oct_db"], octave)
    if cfg["shep_db"] > -47:
        tonal = tonal + _layer_at(cfg["shep_db"], _shepard(cfg, f.n, gen, 150.0))
    return tonal


def _build_send(cfg: dict, f: types.SimpleNamespace, gen) -> np.ndarray:
    """5.9 step 11: the gated noise and tonal layers on the level curve, plus the reverse swell."""
    closed = f.depth * (1 - f.g)
    noise_gate = (1 - closed)[:, None]
    tonal = _build_tonal(cfg, f, gen) * (1 - cfg["tone_gate"] * closed)[:, None]
    feedv = (_layer_at(cfg["noise_db"], _noise_band(cfg, f, gen)) * noise_gate + tonal) * f.lv[:, None]
    if cfg["rswell_db"] > -47:
        feedv = feedv + _layer_at(cfg["rswell_db"], _reverse_swell(cfg, f.n, gen)) * (0.25 + 0.75 * noise_gate)
    return feedv


def _pre_dip(cfg: dict, tax: np.ndarray) -> np.ndarray:
    dv = cfg["pre_dip_ms"] / 1000
    d0 = cfg["length"] - FINAL_OPEN - dv
    return (1 - 0.5 * np.exp(-0.5 * ((tax - d0) / (0.35 * dv)) ** 2))[:, None]


def render_build(cfg: dict, gen: np.random.Generator) -> np.ndarray:
    """The round-6 build: noise band, unison chord, accelerating gate, optional reverse swell, hard end."""
    cfg = {**DEFAULTS, **R6_OFF, **cfg}
    f = _frame(cfg, cfg["gate_pow"])
    feedv = _build_send(cfg, f, gen)
    mix = _add_reverb(feedv, feedv, cfg, f.n)
    if cfg["pre_dip_ms"] > 0.5:
        mix = mix * _pre_dip(cfg, f.t)
    return _finish_tail(mix, cfg, 120.0)


# ---------------------------------------------------------------- rounds 1 to 4 (5.11)


def _is_blip(cfg: dict) -> bool:
    return cfg["tmode"] in ("blips", "nblips")


def _held(f: np.ndarray, phase: np.ndarray) -> np.ndarray:
    """Pitch held per gate pulse at its value on the pulse's first sample."""
    idx = np.floor(phase).astype(np.int64)
    change = np.r_[True, idx[1:] != idx[:-1]]
    first = np.maximum.accumulate(np.where(change, np.arange(len(f)), 0))
    return f[first]


def _noise_blips(cfg: dict, fb: np.ndarray, gen) -> tuple[np.ndarray, np.ndarray]:
    """2 band-passed noises per blip (their spread set by width), and a noise octave."""
    ns = len(fb)
    cut = np.minimum(fb, 16000)
    amp = svf(gen.standard_normal(ns), cut, cfg["nb_q"], "band")
    bv = svf(gen.standard_normal(ns), cut, cfg["nb_q"], "band")
    mid = (amp + bv) / 2
    kn = min(1.0, 0.5 * cfg["width"] + 0.2)
    octave = svf(gen.standard_normal(ns), np.minimum(2 * fb, 16000), 1.5 * cfg["nb_q"], "band")
    return np.column_stack([mid + (amp - mid) * kn, mid + (bv - mid) * kn]), stereo2(octave)


def _tone_blips(cfg: dict, fb: np.ndarray, ph: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    tone = unison(fb, cfg["wave"], cfg["detune"], 0.25 * cfg["width"], ph[0:5])
    return tone, unison(2 * fb, cfg["wave"], 0.7 * cfg["detune"], 0.6 * cfg["width"], ph[5:10])


def _blip_gate(cfg: dict, f: types.SimpleNamespace, scale: float) -> np.ndarray:
    """The percussive blip gate, held open over the final 45 ms."""
    pg = gate_shape(f.phase, f.per, cfg["gate_duty"], 1.0, perc=scale * cfg["blip_dec"])
    return (pg * f.fin + 1 - f.fin)[:, None]


def _blip_layers(cfg: dict, pitch: np.ndarray, f: types.SimpleNamespace, gen, ph) -> tuple[np.ndarray, np.ndarray]:
    fb = _held(pitch, f.phase) * 2.0 ** (-0.35 * np.mod(f.phase, 1.0))
    tone, octave = _noise_blips(cfg, fb, gen) if cfg["tmode"] == "nblips" else _tone_blips(cfg, fb, ph)
    return tone * _blip_gate(cfg, f, 1.0), octave * _blip_gate(cfg, f, 0.7)


def _glide_layers(cfg: dict, pitch: np.ndarray, wv: np.ndarray, ph) -> tuple[np.ndarray, np.ndarray]:
    dtn = cfg["detune"] * (0.6 + 0.8 * wv)
    tone = unison(pitch, cfg["wave"], dtn, 0.5 * cfg["width"], ph[0:5])
    return tone, unison(2 * pitch, cfg["wave"], 0.7 * dtn, 0.6 * cfg["width"], ph[5:10])


def _tonal(cfg: dict, f: types.SimpleNamespace, gen) -> tuple[np.ndarray, np.ndarray]:
    pitch = _pitch(cfg, f.u)
    ph = gen.random(10)
    if _is_blip(cfg):
        tone, octave = _blip_layers(cfg, pitch, f, gen, ph)
    else:
        tone, octave = _glide_layers(cfg, pitch, f.w, ph)
    octave = octave * _octave_rise(f.u)
    if cfg["wave"] == "saw":
        tone = _saw_lp(tone, pitch, cfg, 0.8)
        octave = _saw_lp(octave, pitch, cfg, 0.8, 2.0)
    return bw(tone, "high", 0.8 * cfg["hp"], 2), bw(octave, "high", cfg["hp"], 2)


def _drone_tone(df: np.ndarray, gen) -> np.ndarray:
    """3 detuned saws (-7, 0, +7 cents) low-passed at 500 Hz, plus a sine."""
    saws = sum(dsp.osc(df * 2.0 ** (cv / 1200), "saw", phase=float(gen.random())) for cv in (-7, 0, 7)) / 3
    sine = np.sin(2 * np.pi * np.cumsum(df) / SR + 2 * np.pi * gen.random())
    return 0.8 * bw(saws, "low", 500, 2) + 0.35 * sine


def _with_rumble(dv: np.ndarray, df: np.ndarray, share: float, gen) -> np.ndarray:
    """Blend the tonal drone with a band-passed noise rumble that follows the drone pitch."""
    rumble = svf(gen.standard_normal(len(df)), np.clip(2.2 * df, 60, 2000), 1.2, "band")
    rumble = bw(rumble, "low", 600, 2)
    return (1 - share) * dv / (np.std(dv) + 1e-12) + share * rumble / (np.std(rumble) + 1e-12)


def _drone(cfg: dict, f: types.SimpleNamespace, gen) -> np.ndarray:
    df = cfg["drone_f"] * 2.0 ** (cfg["drone_oct"] * f.w)
    dv = _drone_tone(df, gen)
    if cfg["drone_noise"] > 0.01:
        dv = _with_rumble(dv, df, cfg["drone_noise"], gen)
    dv = bw(dv, "high", 30, 2) * 10 ** (-0.3 * cfg["rise_db"] * (1 - f.w) / 20)
    g = gate_shape(f.phase * 2, f.per / 2, cfg["gate_duty"], cfg["gate_sharp"]) if _is_blip(cfg) else f.g
    return dv * (1 - cfg["gate_drone"] * f.depth * (1 - g))


_METAL_RATIOS = (1.0, 1.37, 1.71, 2.13, 2.58, 3.11, 3.66, 4.29, 5.02)


def _metal_ratios(cfg: dict, gen) -> np.ndarray:
    """The fixed 9 ratios, or metal_n random ratios (sorted, closer together) when metal_n > 9."""
    count = int(round(cfg["metal_n"]))
    if count > 9:
        return np.sort(np.exp(gen.uniform(0, np.log(6), count)))
    return np.array(_METAL_RATIOS)


def _opening(kn: int, count: int, wv: np.ndarray, gen) -> np.ndarray:
    """When partial k opens: its own random point (dense), or higher partials later (fixed)."""
    if count > 9:
        u_on = -1.0 if kn % 5 == 0 else gen.uniform(0, 0.75)
        return np.clip((wv - u_on) / 0.25, 0, 1)
    return np.clip(0.9 * wv * count - 0.6 * kn + 1, 0, 1)


def _metal_partial(cfg: dict, fk: float, kn: int, count: int, glide: np.ndarray, gen) -> np.ndarray:
    am = np.abs(bw(gen.standard_normal(len(glide)), "low", 220, 2))
    am = am / (am.mean() + 1e-12)
    if cfg["metal_glide"] > 0.001:
        phs = 2 * np.pi * np.cumsum(fk * glide) / SR + 2 * np.pi * gen.random()
        return np.sin(phs) * am / (1 + 0.15 * kn * 9 / count)
    return np.sin(2 * np.pi * fk * t_of(len(glide))) / (1 + 0.15 * kn)


def _metal(cfg: dict, f: types.SimpleNamespace, gen) -> np.ndarray:
    ratios = _metal_ratios(cfg, gen)
    glide = 2.0 ** (cfg["metal_glide"] * f.u ** 1.2)
    sig = np.zeros(f.n)
    for kn, ratio in enumerate(ratios):
        opening = _opening(kn, len(ratios), f.w, gen)
        fk = cfg["metal_f"] * ratio * (1 + 0.004 * gen.standard_normal())
        if fk * 2 ** cfg["metal_glide"] <= 19000:
            sig += _metal_partial(cfg, fk, kn, len(ratios), glide, gen) * opening
    return _spread(bw(sig, "high", 1200, 2), 1.0, cfg["width"])


def _beds(cfg: dict, f: types.SimpleNamespace, gen) -> np.ndarray:
    """The noise, shepard and metal layers: the layers that the gate always rides."""
    noise = svf(_noise_source(cfg, f.n, gen), cfg["nf0"] * (cfg["nf1"] / cfg["nf0"]) ** f.w, cfg["nq"], "low")
    beds = _layer_at(cfg["noise_db"], _spread(bw(noise, "high", cfg["hp"], 4), 1.0, cfg["width"] * (0.35 + 0.65 * f.w)))
    if cfg["shep_db"] > -47:
        beds = beds + _layer_at(cfg["shep_db"], _shepard(cfg, f.n, gen, 60.0))
    if cfg["metal_db"] > -47:
        beds = beds + _layer_at(cfg["metal_db"], _metal(cfg, f, gen))
    return beds


def _send(cfg: dict, f: types.SimpleNamespace, gen) -> np.ndarray:
    """5.11 step 12: blips carry their own gate; a glide build is gated as a whole."""
    gate = (1 - f.depth * (1 - f.g))[:, None]
    beds = _beds(cfg, f, gen)
    tone, octave = _tonal(cfg, f, gen)
    tonal = _layer_at(cfg["tone_db"], tone) + _layer_at(cfg["oct_db"], octave)
    if _is_blip(cfg):
        return (beds * gate + tonal) * f.lv[:, None]
    return (beds + tonal) * gate * f.lv[:, None]


def render(cfg: dict, gen: np.random.Generator) -> np.ndarray:
    """One riser take, (n, 2) float64 at 48 kHz; n = samples of length."""
    if float((cfg or {}).get("r6", 0.0)) > 0.5:
        return render_build(cfg, gen)
    cfg = {**DEFAULTS, **(cfg or {})}
    f = _frame(cfg, 1.4)
    feedv = _send(cfg, f, gen)
    drone = _layer_at(cfg["drone_db"], stereo2(_drone(cfg, f, gen))) * (f.lv ** 0.3)[:, None]
    return _finish_tail(_add_reverb(feedv + drone, feedv, cfg, f.n), cfg, 30.0)


# ---------------------------------------------------------------- measure (5.12)


def _ma5(env: np.ndarray) -> np.ndarray:
    return np.convolve(env, np.ones(5) / 5, mode="same")


def _cent_oct(wav: np.ndarray, loud_rel: float) -> float:
    cent, power = centroid_track(wav)
    loud = cent[power > loud_rel * (power.max() if power.size else 0)]
    if len(loud) < 6:
        return 0.0
    return float(np.log2(np.median(loud[-3:]) / max(np.median(loud[:3]), 1.0)))


def _last_third(env: np.ndarray) -> float:
    """How much of the rise happens in the final third: 0.5 or more is concave."""
    kn = len(env)
    k10 = max(1, kn // 10)
    amp, bv = float(np.median(env[:k10])), float(env[-k10:].max())
    two = float(_ma5(env)[min(kn - 1, int(2 * kn / 3))])
    return (bv - two) / max(bv - amp, 0.001)


def _hf_rise(wav: np.ndarray) -> float:
    """Rise of the 4 to 20 kHz band: max of the last tenth minus the median of the first quarter (dB)."""
    hf = band_env_db(wav, 4000, 20000)
    return float(hf[-max(1, len(hf) // 10):].max() - np.median(hf[:max(1, len(hf) // 4)]))


def measure(wav: np.ndarray) -> dict:
    """Model-free riser measurements of 5.12."""
    env = env_db(wav)
    return {
        "length_s": round(len(wav) / SR, 3),
        "last_third": _last_third(env),
        "end_db": float(env[-5:].max() - env.max()),
        "hf_rise_db": _hf_rise(wav),
        "cent_oct": _cent_oct(wav, 1e-4),
        "side_db": side_db(wav),
        "low_db": low_share_db(wav),
    }


def target_check(wav: np.ndarray, row=None) -> float:
    mv = measure(wav)
    pen = 4 * max(0.0, 0.45 - mv["last_third"])
    pen += max(0.0, -3 - mv["end_db"]) / 4
    pen += max(0.0, 20 - mv["hf_rise_db"]) / 10
    pen += max(0.0, mv["low_db"] + 10) / 6
    pen += max(0.0, mv["side_db"] + 6) / 6
    return -min(3.0, pen)
