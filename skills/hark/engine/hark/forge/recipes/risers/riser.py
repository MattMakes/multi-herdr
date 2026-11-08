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
    p = {k: (float(v) if not isinstance(v, str) else v) for k, v in zip(_KEYS, values)}
    p.update(_ALWAYS)
    return p


FAMILIES = {name: _member(cols) for name, cols in _FAM_COLS.items()}
DEFAULTS = dict(FAMILIES["air"])

_ST = FAMILIES["stutter"]


def _over(base: dict, **kw) -> dict:
    out = dict(base)
    out.update(kw)
    return out


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


def _by_cat_riser(base: dict, tuned: tuple, fix: dict) -> dict:
    """A round-1 member, then its round-2 tuned values, then its round-3 fix."""
    p = dict(base)
    p.update({k: (float(v) if not isinstance(v, str) else v) for k, v in zip(_R2_KEYS, tuned)})
    p.update(fix)
    return p


_PRO_COMMON = {"metal_n": 9, "metal_glide": 0.0}
FAMILIES_BY_CAT = {
    "riser": {name: _by_cat_riser(FAMILIES[name], _R2[name], _R3_FIX[name])
              for name in ("air", "tension", "shepard", "rush", "cymbal")},
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


def _r6_member(name: str) -> dict:
    p = dict(_B6)
    p.update({k: float(v) for k, v in zip(_R6_KEYS, _R6_COLS[name])})
    p.update(_R6_EXTRA[name])
    return p


R6 = {name: _r6_member(name) for name in _R6_COLS}

# ---------------------------------------------------------------- shared helpers (5.10)


def lvl(x: np.ndarray, frac: float | None = None) -> float:
    """P8 riser variant: peak 10 ms block RMS of the mono fold (optionally of its last frac) + 1e-12."""
    m = x.mean(axis=1) if x.ndim == 2 else x
    if frac is not None:
        m = m[len(m) - max(1, int(round(frac * len(m)))):]
    k = max(1, len(m) // 480)
    blocks = m[:k * 480].reshape(k, -1) if len(m) >= 480 else m.reshape(1, -1)
    return float(np.sqrt(np.mean(blocks ** 2, axis=1)).max()) + 1e-12


def phase_from_end(rate: np.ndarray, end_phase: float) -> np.ndarray:
    """P15: phase in cycles, laid out so the last sample sits at end_phase."""
    c = np.cumsum(np.asarray(rate, float)[::-1])[::-1] / SR
    return end_phase - c


def gate_shape(phase: np.ndarray, period, duty: float = 0.5, sharp: float = 1.0, ramp: float = 0.0012,
               perc: float = 0.0) -> np.ndarray:
    """P16: a tremolo blended with a square gate (or a percussive gate when perc > 0)."""
    fr = np.mod(phase, 1.0)
    trem = 0.5 + 0.5 * np.cos(2 * np.pi * (fr - duty / 2))
    on = np.clip(fr * period / ramp, 0, 1)
    if perc > 0:
        square = on * np.exp(-fr / max(perc, 0.001)) * np.clip((0.98 - fr) * period / ramp, 0, 1)
    else:
        square = np.minimum(on, np.clip((duty - fr) * period / (1.5 * ramp), 0, 1))
    return (1 - sharp) * trem + sharp * square


def unison(freq, wave: str, detune, width: float, phases) -> np.ndarray:
    """P20: 5 detuned voices panned across the stereo field; returns (n, 2)."""
    out = None
    for k, o in enumerate(UNISON):
        f = np.asarray(freq, float) * 2.0 ** (o * np.asarray(detune, float) / 1200.0)
        v = dsp.osc(f, wave, phase=float(phases[k]))
        th = (np.clip(o * width, -1, 1) + 1) * np.pi / 4
        st = np.column_stack([v * np.cos(th), v * np.sin(th)])
        out = st if out is None else out + st
    return out * np.sqrt(2) / 5


@functools.lru_cache(maxsize=32)
def _ir(rt60: float, size: float, predelay: float, damp: float, lowcut: float) -> np.ndarray:
    return fx.reverb_ir(dsp.rng(1234), rt60=rt60, size=size, predelay=predelay, damp=damp, inputs=2,
                        lowcut=lowcut, seed=1234)


def verb_ir(rt60: float, size: float = 0.75, predelay: float = 0.02, damp: float = 0.5,
            lowcut: float = 200.0) -> np.ndarray:
    """P22: the fixed-seed 2-input reverb IR, (m, 2, 2), cached per rounded setting."""
    return _ir(round(float(rt60), 1), round(float(size), 3), round(float(predelay), 4), round(float(damp), 3),
               round(float(lowcut), 1)).copy()


def wet_of(x: np.ndarray, rt60: float, n_out: int | None = None, **kw) -> np.ndarray:
    """P22: the reverb return alone (no dry), each input channel feeding both outputs."""
    x = stereo2(x)
    ir = verb_ir(rt60, **kw)
    n = len(x) + len(ir) - 1
    out = np.zeros((n, 2))
    for c in range(2):
        for o in range(2):
            out[:, o] += fftconvolve(x[:, c], ir[:, c, o])
    return fit(out, n_out if n_out is not None else n)


def stereo_of(x: np.ndarray) -> np.ndarray:
    return stereo2(x)


def env_db(x: np.ndarray, hop_ms: float = 10) -> np.ndarray:
    m = x.mean(axis=1) if x.ndim == 2 else x
    h = max(1, int(SR * hop_ms / 1000))
    k = max(1, len(m) // h)
    seg = m[:k * h].reshape(k, h) if len(m) >= h else m.reshape(1, -1)
    return 20 * np.log10(np.sqrt(np.mean(seg ** 2, axis=1)) + 1e-9)


def band_env_db(x: np.ndarray, lo: float, hi: float, hop_ms: float = 10) -> np.ndarray:
    if lo <= 20:
        y = bw(x, "low", hi, 4)
    else:
        y = bw(x, "band", (lo, min(hi, 23000)), 4)
    return env_db(y, hop_ms)


def side_db(x: np.ndarray) -> float:
    if x.ndim == 1:
        return -99.0
    mid = (x[:, 0] + x[:, 1]) / 2
    side = (x[:, 0] - x[:, 1]) / 2
    return float(10 * np.log10((np.sum(side ** 2) + 1e-20) / (np.sum(mid ** 2) + 1e-20)))


def low_share_db(x: np.ndarray, f: float = 100) -> float:
    m = x.mean(axis=1) if x.ndim == 2 else x
    lo = bw(m, "low", f, 4)
    return float(10 * np.log10((np.sum(lo ** 2) + 1e-20) / (np.sum(m ** 2) + 1e-20)))


def centroid_track(x: np.ndarray, hop_ms: float = 20) -> tuple[np.ndarray, np.ndarray]:
    m = x.mean(axis=1) if x.ndim == 2 else x
    h = max(2, int(SR * hop_ms / 1000))
    k = len(m) // h
    if k < 1:
        return np.zeros(0), np.zeros(0)
    frames = m[:k * h].reshape(k, h) * np.hanning(h)
    P = np.abs(np.fft.rfft(frames, axis=1)) ** 2
    f = np.fft.rfftfreq(h, 1 / SR)
    power = P.sum(axis=1)
    cent = (P * f).sum(axis=1) / (power + 1e-20)
    return cent, power


# ---------------------------------------------------------------- render pieces


def _layer_at(db: float, x: np.ndarray, frac: float | None = 0.25) -> np.ndarray:
    """AT(db, x): the layer at db relative to its own mix level (last quarter by default)."""
    return 10 ** (db / 20) * x / lvl(x, frac)


def _gate_parts(p: dict, t: np.ndarray, u: np.ndarray, power: float):
    """Gate period, phase, value and depth of 5.9 steps 2 and 3."""
    per = p["gate_p0"] * (p["gate_p1"] / p["gate_p0"]) ** (u ** power)
    phase = phase_from_end(1.0 / per, 0.5 * p["gate_duty"])
    g = gate_shape(phase, per, p["gate_duty"], p["gate_sharp"])
    fin = np.clip((p["length"] - t) / FINAL_OPEN, 0, 1)
    depth = p["gate_depth"] * smooth01(np.clip((u - p["gate_on"]) / 0.2, 0, 1)) * smooth01(fin)
    return per, phase, g, depth, fin


def _frame(p: dict, gate_power: float) -> types.SimpleNamespace:
    """Sample count, time, progress u, curve w = u^curve, level lv and the gate of one render."""
    n = n_of(p["length"])
    t = t_of(n)
    u = t / p["length"]
    w = u ** p["curve"]
    per, phase, g, depth, fin = _gate_parts(p, t, u, gate_power)
    return types.SimpleNamespace(n=n, t=t, u=u, w=w, lv=10 ** (-p["rise_db"] * (1 - w) / 20), per=per,
                                 phase=phase, g=g, depth=depth, fin=fin)


def _noise_source(p: dict, n: int, rng: np.random.Generator) -> np.ndarray:
    white = rng.standard_normal(n)
    pink = dsp.noise(n / SR, rng, "pink")[:n]
    return (1 - p["pink"]) * white + p["pink"] * 1.6 * fit(pink, n)


def _spread(x: np.ndarray, amount: float, w) -> np.ndarray:
    """Decorrelate a mono layer, then set its width."""
    return fx.width(fx.decorrelate(x, amount), w)


def _shepard(p: dict, n: int, rng, f_lo: float, up: bool = True) -> np.ndarray:
    sh = fit(dsp.shepard(n / SR, rng, up=up, rate=p["shep_rate"], f_lo=f_lo, f_hi=9000.0), n)
    return _spread(bw(sh, "high", p["hp"], 2), 0.6, 0.6 * p["width"])


def _pitch(p: dict, u: np.ndarray) -> np.ndarray:
    wt = u ** max(1.0, 0.8 * p["curve"])
    return p["tf0"] * 2.0 ** (p["toct"] * wt)


def _saw_lp(x: np.ndarray, f: np.ndarray, p: dict, q: float, mult: float = 1.0) -> np.ndarray:
    return svf(x, np.minimum(mult * np.minimum(f * p["tbright"], 18000), 18000), q, "low")


def _finish_tail(mix: np.ndarray, p: dict, hp_f: float) -> np.ndarray:
    if p["sat"] > 0.1:
        mix = fx.saturate(mix, p["sat"])
    mix = bw(mix, "high", hp_f, 2)
    mix = bw(mix, "low", 18000, 4)
    return headroom(dsp.fade(mix, 0.004, p["end_ms"] / 1000))


def _add_reverb(mix: np.ndarray, send: np.ndarray, p: dict, n: int) -> np.ndarray:
    if p["verb"] <= 0.005:
        return mix
    wet = wet_of(send, p["rt60"], n)
    return mix + p["verb"] * wet / lvl(wet, 0.25) * lvl(send, 0.25)


def _octave_rise(u: np.ndarray) -> np.ndarray:
    """The +12 layer fades in over the second 60 % of the build."""
    return (np.clip((u - 0.4) / 0.6, 0, 1) ** 2)[:, None]


# ---------------------------------------------------------------- the round-6 build (5.9)


def _build_chord(p: dict, u: np.ndarray, w: np.ndarray, rng) -> tuple[np.ndarray, np.ndarray]:
    f = _pitch(p, u)
    ph = rng.random(15)
    det = p["detune"] * (0.6 + 0.8 * w)
    chord = unison(f, p["wave"], det, 0.5 * p["width"], ph[0:5])
    if p["fifth_db"] > -47:
        chord = chord + 10 ** (p["fifth_db"] / 20) * unison(1.5 * f, p["wave"], det, 0.7 * p["width"], ph[5:10])
    octave = unison(2 * f, p["wave"], 0.7 * det, 0.6 * p["width"], ph[10:15]) * _octave_rise(u)
    if p["wave"] == "saw":
        chord = _saw_lp(chord, f, p, 0.7)
        octave = _saw_lp(octave, f, p, 0.7, 2.0)
    return bw(chord, "high", 0.8 * p["hp"], 2), bw(octave, "high", p["hp"], 2)


def _reverse_swell(p: dict, n: int, rng) -> np.ndarray:
    k = n_of(0.03)
    burst = bw(rng.standard_normal(k), "high", 1500, 2) * np.exp(-t_of(k) / 0.008)
    wet = wet_of(burst, p["rswell_rt"], n_of(p["rswell_rt"] + 0.1), predelay=0.0, damp=0.3, lowcut=400.0)[::-1]
    out = np.zeros((n, 2))
    if len(wet) >= n:
        out[:] = wet[len(wet) - n:]
    else:
        out[n - len(wet):] = wet
    return fx.width(bw(out, "high", p["hp"], 2), 0.25 + 0.5 * p["width"])


def _lp24(x: np.ndarray, fc) -> np.ndarray:
    """P17 low-pass: 2 cascaded state-variable filters, q 0.54 then 1.31."""
    return svf(svf(x, fc, 0.54, "low"), fc, 1.31, "low")


def _hp24(x: np.ndarray, fc) -> np.ndarray:
    return svf(svf(x, fc, 0.54, "high"), fc, 1.31, "high")


def _noise_band(p: dict, f: types.SimpleNamespace, rng) -> np.ndarray:
    """The build's noise: an unresonant band nbw octaves wide whose top moves from nf0 to nf1."""
    fc = p["nf0"] * (p["nf1"] / p["nf0"]) ** f.w
    noise = _hp24(_lp24(_noise_source(p, f.n, rng), np.minimum(fc, 18000)), np.maximum(fc / 2 ** p["nbw"], 20))
    return _spread(bw(noise, "high", p["hp"], 4), 1.0, p["width"] * (0.35 + 0.65 * f.w))


def _build_tonal(p: dict, f: types.SimpleNamespace, rng) -> np.ndarray:
    chord, octave = _build_chord(p, f.u, f.w, rng)
    tonal = _layer_at(p["tone_db"], chord) + _layer_at(p["oct_db"], octave)
    if p["shep_db"] > -47:
        tonal = tonal + _layer_at(p["shep_db"], _shepard(p, f.n, rng, 150.0))
    return tonal


def _build_send(p: dict, f: types.SimpleNamespace, rng) -> np.ndarray:
    """5.9 step 11: the gated noise and tonal layers on the level curve, plus the reverse swell."""
    closed = f.depth * (1 - f.g)
    noise_gate = (1 - closed)[:, None]
    tonal = _build_tonal(p, f, rng) * (1 - p["tone_gate"] * closed)[:, None]
    send = (_layer_at(p["noise_db"], _noise_band(p, f, rng)) * noise_gate + tonal) * f.lv[:, None]
    if p["rswell_db"] > -47:
        send = send + _layer_at(p["rswell_db"], _reverse_swell(p, f.n, rng)) * (0.25 + 0.75 * noise_gate)
    return send


def _pre_dip(p: dict, t: np.ndarray) -> np.ndarray:
    d = p["pre_dip_ms"] / 1000
    d0 = p["length"] - FINAL_OPEN - d
    return (1 - 0.5 * np.exp(-0.5 * ((t - d0) / (0.35 * d)) ** 2))[:, None]


def render_build(p: dict, rng: np.random.Generator) -> np.ndarray:
    """The round-6 build: noise band, unison chord, accelerating gate, optional reverse swell, hard end."""
    p = {**DEFAULTS, **R6_OFF, **p}
    f = _frame(p, p["gate_pow"])
    send = _build_send(p, f, rng)
    mix = _add_reverb(send, send, p, f.n)
    if p["pre_dip_ms"] > 0.5:
        mix = mix * _pre_dip(p, f.t)
    return _finish_tail(mix, p, 120.0)


# ---------------------------------------------------------------- rounds 1 to 4 (5.11)


def _is_blip(p: dict) -> bool:
    return p["tmode"] in ("blips", "nblips")


def _held(f: np.ndarray, phase: np.ndarray) -> np.ndarray:
    """Pitch held per gate pulse at its value on the pulse's first sample."""
    idx = np.floor(phase).astype(np.int64)
    change = np.r_[True, idx[1:] != idx[:-1]]
    first = np.maximum.accumulate(np.where(change, np.arange(len(f)), 0))
    return f[first]


def _noise_blips(p: dict, fb: np.ndarray, rng) -> tuple[np.ndarray, np.ndarray]:
    """2 band-passed noises per blip (their spread set by width), and a noise octave."""
    n = len(fb)
    fc = np.minimum(fb, 16000)
    a = svf(rng.standard_normal(n), fc, p["nb_q"], "band")
    b = svf(rng.standard_normal(n), fc, p["nb_q"], "band")
    mid = (a + b) / 2
    k = min(1.0, 0.5 * p["width"] + 0.2)
    octave = svf(rng.standard_normal(n), np.minimum(2 * fb, 16000), 1.5 * p["nb_q"], "band")
    return np.column_stack([mid + (a - mid) * k, mid + (b - mid) * k]), stereo2(octave)


def _tone_blips(p: dict, fb: np.ndarray, ph: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    tone = unison(fb, p["wave"], p["detune"], 0.25 * p["width"], ph[0:5])
    return tone, unison(2 * fb, p["wave"], 0.7 * p["detune"], 0.6 * p["width"], ph[5:10])


def _blip_gate(p: dict, f: types.SimpleNamespace, scale: float) -> np.ndarray:
    """The percussive blip gate, held open over the final 45 ms."""
    pg = gate_shape(f.phase, f.per, p["gate_duty"], 1.0, perc=scale * p["blip_dec"])
    return (pg * f.fin + 1 - f.fin)[:, None]


def _blip_layers(p: dict, pitch: np.ndarray, f: types.SimpleNamespace, rng, ph) -> tuple[np.ndarray, np.ndarray]:
    fb = _held(pitch, f.phase) * 2.0 ** (-0.35 * np.mod(f.phase, 1.0))
    tone, octave = _noise_blips(p, fb, rng) if p["tmode"] == "nblips" else _tone_blips(p, fb, ph)
    return tone * _blip_gate(p, f, 1.0), octave * _blip_gate(p, f, 0.7)


def _glide_layers(p: dict, pitch: np.ndarray, w: np.ndarray, ph) -> tuple[np.ndarray, np.ndarray]:
    det = p["detune"] * (0.6 + 0.8 * w)
    tone = unison(pitch, p["wave"], det, 0.5 * p["width"], ph[0:5])
    return tone, unison(2 * pitch, p["wave"], 0.7 * det, 0.6 * p["width"], ph[5:10])


def _tonal(p: dict, f: types.SimpleNamespace, rng) -> tuple[np.ndarray, np.ndarray]:
    pitch = _pitch(p, f.u)
    ph = rng.random(10)
    if _is_blip(p):
        tone, octave = _blip_layers(p, pitch, f, rng, ph)
    else:
        tone, octave = _glide_layers(p, pitch, f.w, ph)
    octave = octave * _octave_rise(f.u)
    if p["wave"] == "saw":
        tone = _saw_lp(tone, pitch, p, 0.8)
        octave = _saw_lp(octave, pitch, p, 0.8, 2.0)
    return bw(tone, "high", 0.8 * p["hp"], 2), bw(octave, "high", p["hp"], 2)


def _drone_tone(df: np.ndarray, rng) -> np.ndarray:
    """3 detuned saws (-7, 0, +7 cents) low-passed at 500 Hz, plus a sine."""
    saws = sum(dsp.osc(df * 2.0 ** (c / 1200), "saw", phase=float(rng.random())) for c in (-7, 0, 7)) / 3
    sine = np.sin(2 * np.pi * np.cumsum(df) / SR + 2 * np.pi * rng.random())
    return 0.8 * bw(saws, "low", 500, 2) + 0.35 * sine


def _with_rumble(d: np.ndarray, df: np.ndarray, share: float, rng) -> np.ndarray:
    """Blend the tonal drone with a band-passed noise rumble that follows the drone pitch."""
    rumble = svf(rng.standard_normal(len(df)), np.clip(2.2 * df, 60, 2000), 1.2, "band")
    rumble = bw(rumble, "low", 600, 2)
    return (1 - share) * d / (np.std(d) + 1e-12) + share * rumble / (np.std(rumble) + 1e-12)


def _drone(p: dict, f: types.SimpleNamespace, rng) -> np.ndarray:
    df = p["drone_f"] * 2.0 ** (p["drone_oct"] * f.w)
    d = _drone_tone(df, rng)
    if p["drone_noise"] > 0.01:
        d = _with_rumble(d, df, p["drone_noise"], rng)
    d = bw(d, "high", 30, 2) * 10 ** (-0.3 * p["rise_db"] * (1 - f.w) / 20)
    g = gate_shape(f.phase * 2, f.per / 2, p["gate_duty"], p["gate_sharp"]) if _is_blip(p) else f.g
    return d * (1 - p["gate_drone"] * f.depth * (1 - g))


_METAL_RATIOS = (1.0, 1.37, 1.71, 2.13, 2.58, 3.11, 3.66, 4.29, 5.02)


def _metal_ratios(p: dict, rng) -> np.ndarray:
    """The fixed 9 ratios, or metal_n random ratios (sorted, closer together) when metal_n > 9."""
    count = int(round(p["metal_n"]))
    if count > 9:
        return np.sort(np.exp(rng.uniform(0, np.log(6), count)))
    return np.array(_METAL_RATIOS)


def _opening(k: int, count: int, w: np.ndarray, rng) -> np.ndarray:
    """When partial k opens: its own random point (dense), or higher partials later (fixed)."""
    if count > 9:
        u_on = -1.0 if k % 5 == 0 else rng.uniform(0, 0.75)
        return np.clip((w - u_on) / 0.25, 0, 1)
    return np.clip(0.9 * w * count - 0.6 * k + 1, 0, 1)


def _metal_partial(p: dict, fk: float, k: int, count: int, glide: np.ndarray, rng) -> np.ndarray:
    am = np.abs(bw(rng.standard_normal(len(glide)), "low", 220, 2))
    am = am / (am.mean() + 1e-12)
    if p["metal_glide"] > 0.001:
        phs = 2 * np.pi * np.cumsum(fk * glide) / SR + 2 * np.pi * rng.random()
        return np.sin(phs) * am / (1 + 0.15 * k * 9 / count)
    return np.sin(2 * np.pi * fk * t_of(len(glide))) / (1 + 0.15 * k)


def _metal(p: dict, f: types.SimpleNamespace, rng) -> np.ndarray:
    ratios = _metal_ratios(p, rng)
    glide = 2.0 ** (p["metal_glide"] * f.u ** 1.2)
    y = np.zeros(f.n)
    for k, ratio in enumerate(ratios):
        opening = _opening(k, len(ratios), f.w, rng)
        fk = p["metal_f"] * ratio * (1 + 0.004 * rng.standard_normal())
        if fk * 2 ** p["metal_glide"] <= 19000:
            y += _metal_partial(p, fk, k, len(ratios), glide, rng) * opening
    return _spread(bw(y, "high", 1200, 2), 1.0, p["width"])


def _beds(p: dict, f: types.SimpleNamespace, rng) -> np.ndarray:
    """The noise, shepard and metal layers: the layers that the gate always rides."""
    noise = svf(_noise_source(p, f.n, rng), p["nf0"] * (p["nf1"] / p["nf0"]) ** f.w, p["nq"], "low")
    beds = _layer_at(p["noise_db"], _spread(bw(noise, "high", p["hp"], 4), 1.0, p["width"] * (0.35 + 0.65 * f.w)))
    if p["shep_db"] > -47:
        beds = beds + _layer_at(p["shep_db"], _shepard(p, f.n, rng, 60.0))
    if p["metal_db"] > -47:
        beds = beds + _layer_at(p["metal_db"], _metal(p, f, rng))
    return beds


def _send(p: dict, f: types.SimpleNamespace, rng) -> np.ndarray:
    """5.11 step 12: blips carry their own gate; a glide build is gated as a whole."""
    gate = (1 - f.depth * (1 - f.g))[:, None]
    beds = _beds(p, f, rng)
    tone, octave = _tonal(p, f, rng)
    tonal = _layer_at(p["tone_db"], tone) + _layer_at(p["oct_db"], octave)
    if _is_blip(p):
        return (beds * gate + tonal) * f.lv[:, None]
    return (beds + tonal) * gate * f.lv[:, None]


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """One riser take, (n, 2) float64 at 48 kHz; n = samples of length."""
    if float((p or {}).get("r6", 0.0)) > 0.5:
        return render_build(p, rng)
    p = {**DEFAULTS, **(p or {})}
    f = _frame(p, 1.4)
    send = _send(p, f, rng)
    drone = _layer_at(p["drone_db"], stereo2(_drone(p, f, rng))) * (f.lv ** 0.3)[:, None]
    return _finish_tail(_add_reverb(send + drone, send, p, f.n), p, 30.0)


# ---------------------------------------------------------------- measure (5.12)


def _ma5(e: np.ndarray) -> np.ndarray:
    return np.convolve(e, np.ones(5) / 5, mode="same")


def _cent_oct(x: np.ndarray, loud_rel: float) -> float:
    cent, power = centroid_track(x)
    loud = cent[power > loud_rel * (power.max() if power.size else 0)]
    if len(loud) < 6:
        return 0.0
    return float(np.log2(np.median(loud[-3:]) / max(np.median(loud[:3]), 1.0)))


def _last_third(e: np.ndarray) -> float:
    """How much of the rise happens in the final third: 0.5 or more is concave."""
    k = len(e)
    k10 = max(1, k // 10)
    a, b = float(np.median(e[:k10])), float(e[-k10:].max())
    two = float(_ma5(e)[min(k - 1, int(2 * k / 3))])
    return (b - two) / max(b - a, 0.001)


def _hf_rise(x: np.ndarray) -> float:
    """Rise of the 4 to 20 kHz band: max of the last tenth minus the median of the first quarter (dB)."""
    hf = band_env_db(x, 4000, 20000)
    return float(hf[-max(1, len(hf) // 10):].max() - np.median(hf[:max(1, len(hf) // 4)]))


def measure(x: np.ndarray) -> dict:
    """Model-free riser measurements of 5.12."""
    e = env_db(x)
    return {
        "length_s": round(len(x) / SR, 3),
        "last_third": _last_third(e),
        "end_db": float(e[-5:].max() - e.max()),
        "hf_rise_db": _hf_rise(x),
        "cent_oct": _cent_oct(x, 1e-4),
        "side_db": side_db(x),
        "low_db": low_share_db(x),
    }


def target_check(x: np.ndarray, row=None) -> float:
    m = measure(x)
    pen = 4 * max(0.0, 0.45 - m["last_third"])
    pen += max(0.0, -3 - m["end_db"]) / 4
    pen += max(0.0, 20 - m["hf_rise_db"]) / 10
    pen += max(0.0, m["low_db"] + 10) / 6
    pen += max(0.0, m["side_db"] + 6) / 6
    return -min(3.0, pen)
