"""Recipe pop: a small bubble pop (version 1) and designed motion-design pops (version 2).

A parameter set with the key ``style`` renders with version 2 (chirp, blip,
bubble). Any other set renders with version 1, the physical bubble model. The
version 2 recipe object is ``V2``.
"""

from __future__ import annotations

import math
import types

import numpy as np

from hark.forge.engine import dsp, fx
from hark.forge.recipes.ui import _layers as L

TARGET = "a small bubble pop"
V2_TARGET = "a short tonal pop"
TARGETS = {"pop": TARGET, "pro_pop": V2_TARGET}
ALTS = ["a cork pop", "a water drop", "a finger snap", "a mouse click"]
V2_ALTS = ["a mouse click", "a laser zap", "a synth beep", "a finger snap"]
REFSET = "pop"
REFS = [f"{L.ref_root()}/ui/ui_pop_{i:02d}.wav" for i in range(1, 5)]

LOG = "log"

# ---------------------------------------------------------------- version 1

SPACE = {
    "f0": (400, 3500, LOG), "q": (4, 40, LOG), "rise": (-0.1, 1.4), "rise_tau": (0.002, 0.08, LOG),
    "att": (0.0001, 0.03, LOG), "h2": (0.0, 0.3), "wobble": (0.0, 0.05), "breath_amp": (0.0, 1.0),
    "breath_tau": (0.001, 0.03, LOG), "f2_ratio": (1.6, 4.5), "f2_amp": (0.0, 0.8), "f2_q": (3, 25, LOG),
    "f2_follow": (0.5, 1.0), "sub_amp": (0.0, 0.6), "sub_ratio": (1.2, 2.6), "sub_delay": (0.002, 0.03, LOG),
    "click_amp": (0.0, 1.5), "click_w": (5e-5, 6e-4, LOG), "tear": (1e-4, 3e-3, LOG), "jerks": (1, 6, "int"),
    "click_lp": (2000, 9000, LOG), "shred_amp": (0.0, 0.5), "shred_tau": (2e-4, 3e-3, LOG),
    "shred_hp": (500, 5000, LOG), "hiss_amp": (0.0, 0.6), "hiss_f": (1500, 6000, LOG), "hiss_q": (0.5, 3.0),
    "hiss_tau": (0.001, 0.03, LOG), "puff_amp": (0.0, 0.35), "puff_f": (120, 400, LOG), "ring_delay": (0.0, 0.003),
    "rt60": (0.05, 0.45, LOG), "wet": (0.0, 0.45), "damp": (0.3, 0.95), "size": (0.05, 0.8),
    "air_db": (-12.0, 2.0), "floor_db": (-86.0, -64.0), "swell_amp": (0.0, 0.6), "swell_t": (5e-4, 0.012, LOG),
    "pre": (0.004, 0.035), "hp": (40, 160, LOG), "lp": (4000, 14000, LOG), "clip": (0.14, 0.38),
    "cut_fade": (0.004, 0.03, LOG), "cut_db": (-56.0, -42.0),
}

DEFAULTS = {
    "f0": 440.159, "q": 22.1207, "rise": 0.991892, "rise_tau": 0.0159722, "att": 0.000357098, "h2": 0.00847939,
    "wobble": 0.0112906, "breath_amp": 0.0, "breath_tau": 0.00848874, "f2_ratio": 3.82514, "f2_amp": 0.8,
    "f2_q": 25.0, "f2_follow": 0.948223, "sub_amp": 0.00328062, "sub_ratio": 2.08913, "sub_delay": 0.0111736,
    "click_amp": 0.685048, "click_w": 7.72837e-05, "tear": 0.000171419, "jerks": 1, "click_lp": 4881.31,
    "shred_amp": 0.200779, "shred_tau": 0.00153019, "shred_hp": 5000.0, "hiss_amp": 0.0659931, "hiss_f": 2418.88,
    "hiss_q": 2.86992, "hiss_tau": 0.00113073, "puff_amp": 0.0, "puff_f": 121.063, "ring_delay": 0.00105542,
    "rt60": 0.249181, "wet": 0.291252, "damp": 0.490701, "size": 0.05, "air_db": -0.476997, "floor_db": -64.4367,
    "swell_amp": 0.160452, "swell_t": 0.00252584, "pre": 0.0269276, "hp": 92.6866, "lp": 10181.4,
    "clip": 0.274425, "cut_fade": 0.0153059,
}

V1_BOXES = {
    "main": {"swell_amp": (0.15, 0.6), "swell_t": (0.002, 0.012, LOG), "click_amp": (0, 0.7)},
    "bloop": {"f0": (350, 900, LOG), "rise": (0.7, 1.4), "rise_tau": (0.025, 0.08, LOG), "att": (0.006, 0.04, LOG),
              "q": (18, 40, LOG), "click_amp": (0, 0.35), "shred_amp": (0, 0.2), "ring_delay": (0, 0.025)},
    "small": {"swell_amp": (0.15, 0.6), "swell_t": (0.002, 0.012, LOG), "f0": (1400, 3500, LOG), "rise": (0, 1.0),
              "click_amp": (0, 0.8), "shred_amp": (0, 0.45)},
    "lip": {"f0": (450, 1300, LOG), "rise": (-0.1, 0.35), "rise_tau": (0.005, 0.05, LOG), "q": (4, 14, LOG),
            "breath_amp": (0.2, 1.0), "att": (0.0003, 0.004, LOG), "click_amp": (0, 0.6), "sub_amp": (0, 0.2)},
}

V1_N_S = 0.35


def v1_family_space(fam: str) -> dict:
    """The version 1 space with one family's ranges merged over it."""
    if fam not in V1_BOXES:
        raise KeyError(f"unknown pop version 1 family {fam!r}")
    return L.narrow_space(SPACE, V1_BOXES[fam])


def rupture(w: float, n: int) -> np.ndarray:
    """V1: a positive raised-cosine pulse, then a wider negative lobe of equal area."""
    a, b = max(2, L.n_of(w)), max(3, L.n_of(2.5 * w))
    pos = 0.5 - 0.5 * np.cos(np.linspace(0, 2 * np.pi, a))
    neg = 0.5 - 0.5 * np.cos(np.linspace(0, 2 * np.pi, b))
    neg = neg * -pos.sum() / neg.sum()
    return L.fit(np.concatenate([pos, neg]), n)


def tear_crackle(w: float, tear: float, jerks: int, n: int, r: np.random.Generator) -> np.ndarray:
    """V2: a few rupture pulses spread over the tear time."""
    times = np.sort(np.concatenate([[0.0], r.uniform(0, tear, int(jerks) - 1)]))
    y = np.zeros(n)
    pulse = rupture(w, n)
    for k, t in enumerate(times):
        amp = 1.0
        if k >= 1:
            amp = r.uniform(0.25, 0.8) * 0.8 ** k * (-1.0 if r.uniform() < 0.25 else 1.0)
        L.add_at(y, pulse, L.n_of(t) if t > 0 else 0, amp)
    return y


def wobble(depth: float, n: int, r: np.random.Generator) -> np.ndarray:
    """V3: slow random pitch wander of RMS ``depth``."""
    if depth <= 0:
        return np.zeros(n)
    w = dsp.butter(r.standard_normal(n), "low", 60, 2)
    return w * depth / (L.rms(w) + 1e-12)


def pitch_track(f0: float, rise: float, rise_tau: float, n: int, wob: np.ndarray) -> np.ndarray:
    """V4: the rising cavity pitch."""
    t = L.t_of(n)
    return f0 * (1 + rise * (1 - np.exp(-t / rise_tau))) * (1 + wob)


def gliding_ring(f0: float, q: float, rise: float, rise_tau: float, att: float, h2: float, n: int,
                 r: np.random.Generator, wob: np.ndarray) -> np.ndarray:
    """V5: the cavity ring with its rising pitch and a weak 2nd harmonic."""
    t = L.t_of(n)
    ph = np.cumsum(2 * np.pi * pitch_track(f0, rise, rise_tau, n, wob) / L.SR) + r.uniform(-0.3, 0.3)
    env = (1 - np.exp(-t / att)) * np.exp(-t / (q / (np.pi * f0)))
    return env * np.sin(ph) + h2 * env ** 2 * np.sin(2 * ph + 0.5)


def _svf_noise(r: np.random.Generator, n: int, fc, q: float) -> np.ndarray:
    return np.asarray(dsp.svf(r.standard_normal(n), fc, q, "band"), dtype=float)


def _v1_body(p: dict, r: np.random.Generator, n: int) -> np.ndarray:
    t = L.t_of(n)
    wob = wobble(p["wobble"], n, r)
    body = gliding_ring(p["f0"], p["q"], p["rise"], p["rise_tau"], p["att"], p["h2"], n, r, wob)
    body += p["f2_amp"] * gliding_ring(p["f0"] * p["f2_ratio"], p["f2_q"], p["rise"] * p["f2_follow"],
                                       p["rise_tau"], p["att"], 0.0, n, r, wob)
    if p["breath_amp"] > 0.01:
        fc = pitch_track(p["f0"], p["rise"], p["rise_tau"], n, wob)
        br = _svf_noise(r, n, fc, max(2.0, p["q"] / 2))
        br *= (1 - np.exp(-t / max(p["att"], 0.0003))) * np.exp(-t / p["breath_tau"])
        body += p["breath_amp"] * L.unit(br)
    if p["sub_amp"] > 0.01:
        sub = gliding_ring(p["f0"] * p["sub_ratio"], 0.7 * p["q"], 0.4 * p["rise"], 0.6 * p["rise_tau"],
                           max(p["att"], 0.0002), p["h2"], n, r, np.zeros(n))
        L.add_at(body, sub, L.n_of(p["sub_delay"]), p["sub_amp"])
    return body


def _v1_event(p: dict, r: np.random.Generator, n: int) -> np.ndarray:
    t = L.t_of(n)
    tick = tear_crackle(p["click_w"], p["tear"], p["jerks"], n, r)
    if p["click_lp"] < 19000:
        tick = dsp.butter(tick, "low", p["click_lp"], 2)
    x = p["click_amp"] * tick
    shred = dsp.butter(r.standard_normal(n), "high", p["shred_hp"], 2) * np.exp(-t / p["shred_tau"])
    x = x + 0.5 * p["shred_amp"] * shred
    L.add_at(x, _v1_body(p, r, n), L.n_of(p["ring_delay"]) if p["ring_delay"] > 0 else 0)
    hiss = _svf_noise(r, n, p["hiss_f"], p["hiss_q"]) * (1 - np.exp(-t / 0.0003)) * np.exp(-t / p["hiss_tau"])
    x = x + p["hiss_amp"] * L.unit(hiss)
    puff = np.sin(2 * np.pi * p["puff_f"] * t) * np.exp(-2.5 * p["puff_f"] * t) * (1 - np.exp(-t / 0.0004))
    return x + p["puff_amp"] * puff


def _leak(p: dict, r: np.random.Generator) -> np.ndarray:
    m = L.n_of(p["swell_t"])
    w = r.standard_normal(m)
    leak = _svf_noise_of(w, p["f0"], 3) + 0.6 * _svf_noise_of(w, p["f0"] * p["f2_ratio"], 2)
    leak = leak + 0.9 * dsp.butter(w, "high", 3000, 2)
    leak = leak * np.exp(5 * (np.arange(m) / m - 1))
    return p["swell_amp"] * L.unit(leak)


def _svf_noise_of(w: np.ndarray, fc: float, q: float) -> np.ndarray:
    return np.asarray(dsp.svf(w, fc, q, "band"), dtype=float)


def _cut_point(x: np.ndarray, p: dict, end: int, head: int) -> int:
    """V1 step 13: wait until the ring is cut_db below its loudest 5 ms frame."""
    mono = L.fit(x.mean(axis=1) if x.ndim == 2 else x, max(240, len(x)))
    k = len(mono) // 240
    e = 20 * np.log10(np.sqrt(np.square(mono[: k * 240]).reshape(k, 240).mean(axis=1)) + 1e-12)
    i = int(np.argmax(e))
    quiet = np.nonzero(e[i:] < e[i] + p["cut_db"])[0]
    j = int(quiet[0]) if len(quiet) else k - i
    died = (i + j) * 240 + L.n_of(p["cut_fade"])
    return min(len(x), max(end, died), head + L.n_of(0.42))


def render_v1(p: dict, rng: np.random.Generator) -> np.ndarray:
    """Version 1: the bubble pop, mono float64 at 48 kHz (about 0.2-0.45 s with the edit)."""
    p = L.merged(DEFAULTS, p)
    x = _v1_event(p, rng, L.n_of(V1_N_S))
    lead = 0
    if p["swell_amp"] > 0.01:
        leak = _leak(p, rng)
        lead = len(leak)
        x = np.concatenate([leak, x])
    x = np.concatenate([np.zeros(L.n_of(p["pre"])), x])
    x = np.asarray(dsp.room(x, rng, rt60=p["rt60"], size=p["size"], damp=p["damp"], wet=p["wet"]), dtype=float)
    if "clip" not in p:
        x = dsp.mic(x, rng, air_db=p["air_db"], floor_db=p["floor_db"])
        x = dsp.trim_tail(np.asarray(x, dtype=float), -62.0, 0.02)
        return L.headroom(np.asarray(dsp.fade(x, 0.001, 0.02), dtype=np.float64))
    x = dsp.butter(dsp.butter(x, "high", p["hp"], 4), "low", p["lp"], 2)
    x = np.asarray(dsp.mic(x, rng, air_db=p["air_db"], floor_db=p["floor_db"]), dtype=float)
    head = L.n_of(p["pre"]) + lead
    end = head + L.n_of(p["clip"])
    if "cut_db" in p:
        end = _cut_point(x, p, end, head)
    return L.headroom(np.asarray(dsp.fade(x[:end], 0.001, p["cut_fade"]), dtype=np.float64))


# ---------------------------------------------------------------- version 2

V2_SPACE = {
    "style": ["chirp", "blip", "bubble"], "f_hi": (3000, 11500, LOG), "f_lo": (90, 900, LOG),
    "sweep": (0.025, 0.14), "ease": (0.5, 2.0), "steps": (0.0, 1.0), "step_ms": (4.0, 13.0), "duty": (0.45, 1.0),
    "pre_db": (-40.0, 0.0), "pre_hz": (2500, 11000, LOG), "pre_t": (0.008, 0.04), "peak_at": (0.1, 0.8),
    "start_db": (-12.0, 0.0), "tail_db": (-20.0, -3.0), "tail_hold": (0.0, 0.035), "tail_tau": (0.006, 0.05, LOG),
    "harm": (0.0, 0.7), "odd": (0.0, 1.0), "f_blip": (700, 2200, LOG), "box": (0.003, 0.07, LOG),
    "b_att": (0.0004, 0.004, LOG), "b_rel": (0.002, 0.014, LOG), "b_drop": (0.0, 5.0), "b_tau": (0.006, 0.3, LOG),
    "f0": (280, 1400, LOG), "rise": (0.1, 1.6), "rise_tau": (0.003, 0.04, LOG), "q_tau": (0.01, 0.09, LOG),
    "att": (0.0004, 0.004, LOG), "h2": (0.0, 0.3), "daughter": (0.0, 0.6), "d_ratio": (1.3, 2.4),
    "d_delay": (0.004, 0.016), "tick_db": (-40.0, 6.0), "tick_hz": (1800, 9000, LOG), "tick_ms": (1, 12, LOG),
    "breath": (0.0, 0.5), "room": (0.0, 0.25), "floor_db": (-110.0, -62.0), "hp": (60, 400, LOG),
    "lp": (4500, 18000, LOG), "drive": (0.0, 8.0), "length": (0.03, 0.4), "fade": (0.002, 0.03, LOG),
    "width": (0.0, 0.4), "gdepth": (-1.0, 1.0), "c_breath": (-60.0, -6.0), "c_bq": (1.5, 8, LOG),
    "land_drop": (0.0, 7.0), "s_att": (0.0, 0.5), "pre_ring": (0.0, 8.0), "c_in": (0.0, 1.0),
    "c_air": (-60.0, -10.0), "c_nat": (0.0, 1.0), "c_ptau": (0.002, 0.03, LOG), "c_tau": (0.006, 0.05, LOG),
    "c_h2": (0.0, 0.3), "c_puff": (-60.0, -6.0), "c_puff_hz": (800, 6000, LOG), "c_puff_ms": (0.6, 6, LOG),
    "c_thump": (-60.0, -6.0), "c_room": (0.0, 0.3), "c_pre": (-60.0, -3.0), "c_pre_hz": (3000, 11000, LOG),
    "c_pre_t": (0.01, 0.04), "n6": (0.0, 1.0), "n_f0": (150, 12000, LOG), "n_f1": (100, 6000, LOG),
    "n_ptau": (0.002, 0.06, LOG), "n_sweep": (0.0, 0.14), "n_ease": (0.4, 2.0), "n_att": (0.0003, 0.04, LOG),
    "n_tau": (0.003, 0.06, LOG), "n_h2": (0.0, 0.4), "n_h3": (0.0, 0.2), "n_m2": (0.0, 0.6), "n_m2r": (1.3, 4.5),
    "n_m2d": (1.0, 4.0), "n_breath": (-60.0, -8.0), "n_dbl": (-60.0, 0.0), "n_dgap": (0.02, 0.12),
    "n_dratio": (0.6, 2.0), "n_dtau": (0.4, 1.6),
}

V2_DEFAULTS = {
    "style": "chirp", "f_hi": 9800.0, "f_lo": 360.0, "sweep": 0.055, "ease": 1.0, "steps": 1.0, "step_ms": 9.0,
    "duty": 0.62, "pre_db": -2.0, "pre_hz": 9800.0, "pre_t": 0.032, "peak_at": 0.3, "start_db": -2.0,
    "tail_db": -11.0, "tail_hold": 0.006, "tail_tau": 0.009, "harm": 0.35, "odd": 0.6, "f_blip": 1220.0,
    "box": 0.005, "b_att": 0.0008, "b_rel": 0.003, "b_drop": 0.0, "b_tau": 0.3, "f0": 520.0, "rise": 0.9,
    "rise_tau": 0.012, "q_tau": 0.035, "att": 0.0008, "h2": 0.08, "daughter": 0.15, "d_ratio": 1.8,
    "d_delay": 0.008, "tick_db": -30.0, "tick_hz": 3500.0, "tick_ms": 1.5, "breath": 0.12, "room": 0.06,
    "floor_db": -110.0, "hp": 150.0, "lp": 16000.0, "drive": 1.5, "length": 0.16, "fade": 0.004, "width": 0.15,
    "gdepth": -1.0, "c_breath": -60.0, "c_bq": 3.0, "land_drop": 0.0, "s_att": 0.0, "pre_ring": 0.0, "c_in": 0.0,
    "c_air": -60.0, "c_nat": 0.0, "c_ptau": 0.008, "c_tau": 0.018, "c_h2": 0.1, "c_puff": -60.0,
    "c_puff_hz": 2500.0, "c_puff_ms": 2.0, "c_thump": -60.0, "c_room": 0.0, "c_pre": -60.0, "c_pre_hz": 9000.0,
    "c_pre_t": 0.025, "n6": 0.0, "n_f0": 1500.0, "n_f1": 600.0, "n_ptau": 0.01, "n_sweep": 0.0, "n_ease": 1.0,
    "n_att": 0.001, "n_tau": 0.015, "n_h2": 0.1, "n_h3": 0.0, "n_m2": 0.0, "n_m2r": 2.3, "n_m2d": 2.0,
    "n_breath": -60.0, "n_dbl": -60.0, "n_dgap": 0.05, "n_dratio": 1.33, "n_dtau": 1.0,
}

FINE = {
    "chirp": ["ease", "duty", "start_db", "tail_tau", "harm", "odd", "tick_db", "tick_hz", "hp", "lp", "drive",
              "fade", "peak_at", "c_breath", "land_drop"],
    "blip": ["b_att", "b_rel", "b_tau", "harm", "odd", "tick_db", "tick_hz", "hp", "lp", "drive", "fade"],
    "bubble": ["rise_tau", "att", "h2", "d_ratio", "d_delay", "tick_db", "tick_hz", "tick_ms", "breath", "room",
               "hp", "lp", "drive", "fade", "width", "floor_db"],
}


def family_space(params: dict, style: str | None = None) -> dict:
    """The version 2 space with every non-fine key pinned to the member's value (or the default)."""
    style = style or params.get("style") or "chirp"
    if style not in FINE:
        raise ValueError(f"unknown pop style {style!r}")
    full = dict(V2_DEFAULTS, **params)
    return {k: (spec if k in FINE[style] else [full[k]]) for k, spec in V2_SPACE.items()}


def _fade_hi(f, k: float):
    return np.clip((18000 - k * np.asarray(f)) / 3000, 0, 1)


def harmonics(ph: np.ndarray, f: np.ndarray, harm: float, odd: float) -> np.ndarray:
    """W1: a sine with a 2nd and a 3rd harmonic that fade out near 18 kHz."""
    y = np.sin(ph)
    for k, a in ((2, harm * (1 - odd)), (3, harm * odd)):
        if a > 0:
            y = y + a * _fade_hi(f, k) * np.sin(k * ph)
    return y


def click(n: int, db: float, hz: float, r: np.random.Generator, ms: float = 1.5) -> np.ndarray:
    """W2: a short band click at the onset (ms above 1.5: the bright 1-4 kHz snap)."""
    if ms <= 1.5:
        k = min(n, L.n_of(0.003))
        w = dsp.butter(r.standard_normal(k), "band", (hz / 1.5, min(1.5 * hz, 20000.0)), 2)
        w = w * np.exp(-L.t_of(k) / (ms / 1000 / 2.5))
    else:
        k = min(n, L.n_of(0.004))
        t = L.t_of(k)
        w = dsp.butter(r.standard_normal(k), "band", (hz / 2, min(1.7 * hz, 20000.0)), 4)
        w = w * (1 - np.exp(-t / 0.0003)) * np.exp(-t / (ms / 1000)) ** 2
        w = dsp.butter(w, "low", min(4000.0, 2 * hz), 4)
    out = np.zeros(n)
    out[:k] = 10 ** (db / 20) * L.unit(np.asarray(w, dtype=float))
    return out


def _puff(y: np.ndarray, p: dict, r: np.random.Generator, rise_tau: float) -> None:
    if p["c_puff"] <= -59:
        return
    ms = p["c_puff_ms"] / 1000
    hz = p["c_puff_hz"]
    k = min(len(y), L.n_of(max(0.004, 5 * ms)))
    t = L.t_of(k)
    w = dsp.butter(r.standard_normal(k), "band", (hz / 2.2, min(2.2 * hz, 16000.0)), 2)
    w = w * (1 - np.exp(-t / rise_tau)) * np.exp(-t / ms)
    y[:k] += 10 ** (p["c_puff"] / 20) * L.unit(w)


def _thump(y: np.ndarray, p: dict, fl: float) -> None:
    if p["c_thump"] <= -59:
        return
    k = min(len(y), L.n_of(1.5 / fl))
    t = L.t_of(k)
    th = np.sin(2 * np.pi * fl * t) * np.exp(-2 * fl * t) * (1 - np.exp(-t / 0.0006))
    y[:k] += 10 ** (p["c_thump"] / 20) * L.unit(th)


def _natural_tail(y: np.ndarray, p: dict, r: np.random.Generator) -> np.ndarray:
    """W3 steps 7-9: edge fades, the soft pre-tick and the tiny room."""
    a = min(len(y), L.n_of(0.0003))
    y[:a] *= 0.5 - 0.5 * np.cos(np.linspace(0, np.pi, a))
    b = min(len(y), L.n_of(0.004))
    y[len(y) - b:] *= np.cos(np.linspace(0, np.pi / 2, b)) ** 2
    if p["c_pre"] > -59:
        m = L.n_of(p["c_pre_t"])
        t = L.t_of(m)
        pre = np.sin(2 * np.pi * p["c_pre_hz"] * t) * np.exp(-np.maximum(t - 0.00125, 0) / 0.002)
        kp = min(m, L.n_of(0.0025))
        pre[:kp] *= np.hanning(kp)
        y = np.concatenate([10 ** (p["c_pre"] / 20) * pre, y])
    if p["c_room"] > 0.005:
        y = fx.reverb(y, r, rt60=0.1 + 0.4 * p["c_room"], size=0.12, predelay=0.003, damp=0.6, wet=p["c_room"],
                      stereo=True, width=0.25)
        y = dsp.trim_tail(np.asarray(y, dtype=float), -56.0, 0.012)
    return np.asarray(y, dtype=float)


def natural_chirp(p: dict, r: np.random.Generator) -> np.ndarray:
    """W3: a rounded sine body whose pitch falls fast and settles."""
    n = L.n_of(min(0.4, max(0.03, 7 * p["c_tau"] + 0.006)))
    t = L.t_of(n)
    f = p["f_lo"] + (p["f_hi"] - p["f_lo"]) * np.exp(-t / p["c_ptau"])
    ph = np.cumsum(2 * np.pi * f / L.SR) + r.uniform(0, 2 * np.pi)
    env = (1 - np.exp(-t / 0.0004)) * np.exp(-t / p["c_tau"])
    h2 = p["c_h2"] * _fade_hi(f, 2) * np.exp(-2 * t / p["c_tau"]) * np.sin(2 * ph + 0.6)
    y = (np.sin(ph) + h2) * env
    _puff(y, p, r, 0.0008)
    _thump(y, p, max(80.0, 0.35 * p["f_lo"]))
    return _natural_tail(y, p, r)


def _moving_pitch(p: dict, n: int, f0: float, f1: float) -> np.ndarray:
    t = L.t_of(n)
    if p["n_sweep"] > 0:
        gl = (1 - np.clip(t / p["n_sweep"], 0, 1)) ** p["n_ease"]
    else:
        gl = np.exp(-t / p["n_ptau"])
    return np.exp(math.log(f1) + (math.log(f0) - math.log(f1)) * gl)


def moving_pop(p: dict, r: np.random.Generator, n: int, f0: float, f1: float, tau: float,
               att: float) -> np.ndarray:
    """W4: one round body whose log pitch travels from f0 to f1 during the decay."""
    t = L.t_of(n)
    f = _moving_pitch(p, n, f0, f1)
    ph = np.cumsum(2 * np.pi * f / L.SR) + r.uniform(0, 2 * np.pi)
    env_r = 0.5 - 0.5 * np.cos(np.pi * np.clip(t / att, 0, 1))
    dec = np.exp(-np.maximum(t - att, 0) / tau)
    y = np.sin(ph)
    if p["n_h2"] > 0:
        y = y + p["n_h2"] * _fade_hi(f, 2) * dec * np.sin(2 * ph + 0.5)
    if p["n_h3"] > 0:
        y = y + p["n_h3"] * _fade_hi(f, 3) * dec ** 2 * np.sin(3 * ph + 1.1)
    if p["n_m2"] > 0:
        ph2 = np.cumsum(2 * np.pi * f * p["n_m2r"] / L.SR) + r.uniform(0, 2 * np.pi)
        y = y + p["n_m2"] * _fade_hi(f, p["n_m2r"]) * dec ** p["n_m2d"] * np.sin(ph2)
    env = env_r * dec
    y = y * env
    if p["n_breath"] > -59:
        b = _svf_noise(r, n, np.minimum(f, 16000.0), p["c_bq"])
        k = min(n, max(16, L.n_of(att + tau)))
        b = b * math.sqrt(0.5) / (L.rms(b[:k]) + 1e-12)
        y = y + 10 ** (p["n_breath"] / 20) * b * env
    return y


def natural_pop(p: dict, r: np.random.Generator) -> np.ndarray:
    """W5: the moving-pitch pop, or a double pop."""
    double = p["n_dbl"] > -59
    gap = p["n_dgap"] if double else 0.0
    dur = p["n_att"] + 6.5 * p["n_tau"] * (max(1.0, p["n_dtau"]) if double else 1.0) + gap + 0.006
    n = L.n_of(min(0.4, max(0.03, dur)))
    y = moving_pop(p, r, n, p["n_f0"], p["n_f1"], p["n_tau"], p["n_att"])
    if double:
        k = L.n_of(gap)
        if k < n:
            y[k:] += 10 ** (p["n_dbl"] / 20) * moving_pop(p, r, n - k, p["n_f0"] * p["n_dratio"],
                                                         p["n_f1"] * p["n_dratio"], p["n_tau"] * p["n_dtau"],
                                                         p["n_att"])
    _puff(y, p, r, 0.0006)
    _thump(y, p, max(70.0, 0.35 * min(p["n_f0"], p["n_f1"])))
    return _natural_tail(y, p, r)


def _step_gate(p: dict, s: np.ndarray, lp: np.ndarray, log_at) -> tuple[np.ndarray, np.ndarray]:
    """W6 step 4: the held log pitch inside each block and the shaped block gate."""
    B = p["step_ms"] / 1000
    sp = np.maximum(s, 0)
    held = log_at(np.clip(np.floor(sp / B) * B / p["sweep"], 0, 1))
    lp = np.where(s < p["sweep"], (1 - p["steps"]) * lp + p["steps"] * held, lp)
    t_land = math.ceil(p["sweep"] / B - 1e-9) * B
    x = np.mod(sp, B) / B
    e_f = 0.0007 / B
    e_r = max(e_f, p["s_att"] * p["duty"])
    raw = np.where(s < t_land, np.clip(np.minimum(x / e_r, (p["duty"] - x) / e_f), 0, 1),
                   np.clip((s - t_land) / 0.0007, 0, 1))
    depth = p["steps"] if p["gdepth"] < 0 else p["gdepth"]
    g = np.where(s >= 0, 1 - depth * (1 - (0.5 - 0.5 * np.cos(np.pi * raw))), 1.0)
    return lp, g


def _chirp_level(p: dict, s: np.ndarray, u: np.ndarray) -> np.ndarray:
    pa = p["peak_at"]
    db = np.where(u < pa, p["start_db"] * (1 - u / max(pa, 1e-3)), p["tail_db"] * (u - pa) / max(1 - pa, 1e-3))
    db = db - 8.686 * np.maximum(s - p["sweep"] - p["tail_hold"], 0) / p["tail_tau"]
    return 10 ** (db / 20)


def _air(y: np.ndarray, p: dict, r: np.random.Generator, env: np.ndarray) -> np.ndarray:
    an = dsp.butter(r.standard_normal(len(y)), "band", (2500, 14000), 2)
    an = an * math.sqrt(0.5) / (L.rms(an) + 1e-12)
    k = math.exp(-1 / (L.SR * 2.5 * p["tail_tau"]))
    ea = np.empty(len(env))
    last = 0.0
    for i, v in enumerate(env):
        last = max(v, last * k)
        ea[i] = last
    return y + 10 ** (p["c_air"] / 20) * an * ea


def _pre_blip(y: np.ndarray, p: dict, t0: float) -> None:
    g = 10 ** (p["pre_db"] / 20)
    k = min(len(y), L.n_of(0.003))
    t = L.t_of(len(y))
    y[:k] += g * np.hanning(k) * np.sin(2 * np.pi * p["pre_hz"] * t[:k])
    if p["pre_ring"] > 0.05:
        kk = min(len(y), L.n_of(t0 + 0.002))
        tt = t[:kk]
        ring = np.sin(2 * np.pi * p["pre_hz"] * tt) * np.exp(-tt / (p["pre_ring"] / 1000))
        y[:kk] += 0.5 * g * ring * np.clip(tt / 0.0015, 0, 1)


def designed_chirp(p: dict, r: np.random.Generator) -> np.ndarray:
    """W6: the stepped or smooth designed chirp (or W3 / W5 when switched on)."""
    if p["n6"] > 0.5:
        return natural_pop(p, r)
    if p["c_nat"] > 0.5:
        return natural_chirp(p, r)
    pre_on = p["pre_db"] > -39
    t0 = p["pre_t"] if pre_on else 0.0
    n = L.n_of(t0 + p["sweep"] + p["tail_hold"] + 5.5 * p["tail_tau"] + p["fade"] + 0.002)
    s = L.t_of(n) - t0
    u = np.clip(s / p["sweep"], 0, 1)
    lo, hi = math.log(p["f_lo"]), math.log(p["f_hi"])

    def log_at(v):
        return hi + (lo - hi) * v ** p["ease"]

    lp, gate = log_at(u), np.ones(n)
    if p["steps"] > 0:
        lp, gate = _step_gate(p, s, lp, log_at)
    if p["land_drop"] > 0:
        lp = lp - math.log(2) * p["land_drop"] / 12 * (1 - np.exp(-np.maximum(s - p["sweep"], 0) / 0.02))
    f = np.exp(lp)
    y = harmonics(np.cumsum(2 * np.pi * f / L.SR) + r.uniform(0, 2 * np.pi), f, p["harm"], p["odd"])
    env = np.where(s < 0, 0.0, _chirp_level(p, s, u) * gate)
    i0 = L.n_of(t0) if t0 > 0 else 0
    a = max(0, min(L.n_of(0.0008), n - i0))
    env[i0:i0 + a] *= 0.5 - 0.5 * np.cos(np.linspace(0, np.pi, a))
    return _chirp_layers(y * env, p, r, f, env, t0, i0)


def _chirp_layers(y: np.ndarray, p: dict, r: np.random.Generator, f: np.ndarray, env: np.ndarray, t0: float,
                  i0: int) -> np.ndarray:
    """W6 steps 10-14: breath, air, pre-blip, onset click and the fade-in."""
    n = len(y)
    if p["c_breath"] > -59:
        b = _svf_noise(r, n, np.minimum(f, 16000.0), p["c_bq"])
        y = y + 10 ** (p["c_breath"] / 20) * b * math.sqrt(0.5) / (L.rms(b) + 1e-12) * env
    if p["c_air"] > -59:
        y = _air(y, p, r, env)
    if p["pre_db"] > -39:
        _pre_blip(y, p, t0)
    if p["tick_db"] > -39:
        y = y + np.roll(click(n, p["tick_db"], p["tick_hz"], r, p["tick_ms"]), i0)
    if p["c_in"] > 0.01:
        k = min(n, L.n_of(p["c_in"] / 1000))
        y[:k] *= 0.5 - 0.5 * np.cos(np.linspace(0, np.pi, k))
    return y


def blip(p: dict, r: np.random.Generator) -> np.ndarray:
    """W7: a gated tone burst with harmonics and an optional pitch drop."""
    n = L.n_of(p["box"] + p["b_rel"] + p["fade"] + 0.004)
    t = L.t_of(n)
    f = p["f_blip"] * 2 ** (-(p["b_drop"] / 12) * np.clip(t / max(p["box"], 1e-3), 0, 1))
    y = harmonics(np.cumsum(2 * np.pi * f / L.SR) + r.uniform(0, 2 * np.pi), f, p["harm"], p["odd"])
    a = np.clip(t / p["b_att"], 0, 1)
    rl = np.clip((t - p["box"]) / p["b_rel"], 0, 1)
    y = y * (0.5 - 0.5 * np.cos(np.pi * a)) * (0.5 + 0.5 * np.cos(np.pi * rl)) * np.exp(-t / p["b_tau"])
    if p["tick_db"] > -39:
        y = y + click(n, p["tick_db"], p["tick_hz"], r, p["tick_ms"])
    return y


def bubble(p: dict, r: np.random.Generator) -> np.ndarray:
    """W8: a pure rising sine with a tick, an optional daughter ring and pitch-tracking breath."""
    n = L.n_of(max(p["length"], 0.06) + 0.05)
    t = L.t_of(n)
    trk = p["f0"] * (1 + p["rise"] * (1 - np.exp(-t / p["rise_tau"])))
    ph = np.cumsum(2 * np.pi * trk / L.SR) + r.uniform(-0.3, 0.3)
    env = (1 - np.exp(-t / p["att"])) * np.exp(-t / p["q_tau"])
    y = env * np.sin(ph) + p["h2"] * env ** 2 * np.sin(2 * ph + 0.4)
    if p["daughter"] > 0.01:
        d = p["f0"] * p["d_ratio"] * (1 + 0.4 * p["rise"] * (1 - np.exp(-t / (0.6 * p["rise_tau"]))))
        de = (1 - np.exp(-t / max(p["att"], 0.0003))) * np.exp(-t / (0.6 * p["q_tau"]))
        L.add_at(y, p["daughter"] * de * np.sin(np.cumsum(2 * np.pi * d / L.SR)), L.n_of(p["d_delay"]))
    if p["breath"] > 0.01:
        b = _svf_noise(r, n, trk, 4) * (1 - np.exp(-t / 0.0006)) * np.exp(-t / 0.006)
        y = y + p["breath"] * L.unit(b)
    if p["tick_db"] > -39:
        y = y + click(n, p["tick_db"], p["tick_hz"], r, p["tick_ms"])
    return y


def _bubble_out(y: np.ndarray, p: dict, r: np.random.Generator) -> np.ndarray:
    if p["room"] > 0.005:
        y = fx.reverb(y, r, rt60=0.12 + 0.5 * p["room"], size=0.15, predelay=0.004, damp=0.6, wet=p["room"],
                      stereo=True, width=0.3 + p["width"])
    else:
        y = np.column_stack([y, y])
    if p["floor_db"] > -100:
        y = dsp.mic(np.asarray(y, dtype=float), r, floor_db=p["floor_db"])
    return np.asarray(y, dtype=float)[: L.n_of(p["length"])]


STYLES = {"chirp": designed_chirp, "blip": blip, "bubble": bubble}


def render_v2(p: dict, rng: np.random.Generator) -> np.ndarray:
    """Version 2: a designed pop. Mono for chirp (stereo with its room) and blip, stereo for bubble."""
    p = dict(V2_DEFAULTS, **(p or {}))
    if p["style"] not in STYLES:
        raise ValueError(f"unknown pop style {p['style']!r}")
    y = np.asarray(STYLES[p["style"]](p, rng), dtype=float)
    if p["drive"] > 0.2:
        y = np.asarray(fx.saturate(y, drive_db=p["drive"], mode="tanh", mix=0.5, level="rms"), dtype=float)
    y = dsp.butter(y, "high", p["hp"], 4)
    if p["lp"] < 17500:
        y = dsp.butter(y, "low", p["lp"], 2)
    y = np.asarray(y, dtype=float)
    if p["style"] == "bubble":
        y = _bubble_out(y, p, rng)
    elif p["style"] == "blip":
        y = y[: min(len(y), max(L.n_of(p["length"]), L.n_of(p["box"] + p["b_rel"] + 0.002)))]
    k = min(len(y), L.n_of(p["fade"]))
    w = np.cos(np.linspace(0, np.pi / 2, k)) ** 2
    y = np.array(y, dtype=float)
    y[len(y) - k:] *= w if y.ndim == 1 else w[:, None]
    return L.headroom(y.astype(np.float64))


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """A pop take: version 2 when ``p`` has ``style``, else version 1."""
    return render_v2(p, rng) if "style" in (p or {}) else render_v1(p, rng)


# --------------------------------------------------------------- v2 palettes

def _members(base: dict, table: dict[str, dict]) -> list[tuple[str, dict]]:
    return [(name, {**V2_DEFAULTS, **base, **edits}) for name, edits in table.items()]


ROUND6_BASE = {"n6": 1, "drive": 0, "steps": 0, "pre_db": -40, "tick_db": -40, "harm": 0, "c_breath": -60,
               "hp": 120, "lp": 16000, "fade": 0.004}
ROUND6 = {
    "chirp": {"n_f0": 9800, "n_f1": 350, "n_ptau": 0.03, "n_att": 0.012, "n_tau": 0.018, "n_h2": 0.05,
              "n_breath": -12, "c_bq": 2.0, "c_puff": -22, "c_puff_hz": 7000, "c_puff_ms": 0.6, "c_pre": -10,
              "c_pre_hz": 9500, "c_pre_t": 0.026, "c_room": 0.08},
    "boop": {"n_f0": 1200, "n_f1": 210, "n_ptau": 0.014, "n_att": 0.0008, "n_tau": 0.016, "n_h2": 0.05,
             "c_puff": -12, "c_puff_hz": 4500, "c_puff_ms": 0.6, "hp": 90, "c_room": 0.06},
    "bloop": {"n_f0": 260, "n_f1": 540, "n_ptau": 0.022, "n_att": 0.004, "n_tau": 0.016, "n_h2": 0.15,
              "n_breath": -24, "c_bq": 3.0, "c_room": 0.1, "hp": 100},
    "double": {"n_f0": 1700, "n_f1": 950, "n_ptau": 0.005, "n_att": 0.0008, "n_tau": 0.008, "n_h2": 0.1,
               "n_m2": 0.2, "n_m2r": 2.76, "n_m2d": 2.5, "n_dbl": -4, "n_dgap": 0.07, "n_dratio": 0.75,
               "n_dtau": 1.2, "c_puff": -16, "c_puff_hz": 3500, "c_puff_ms": 0.6, "c_room": 0.08},
    "tink": {"n_f0": 2300, "n_f1": 1900, "n_ptau": 0.004, "n_att": 0.0005, "n_tau": 0.012, "n_h2": 0.03,
             "n_m2": 0.45, "n_m2r": 2.76, "n_m2d": 1.5, "c_puff": -20, "c_puff_hz": 6000, "c_puff_ms": 0.5,
             "c_room": 0.08},
    "thup": {"n_f0": 200, "n_f1": 300, "n_ptau": 0.01, "n_att": 0.0015, "n_tau": 0.012, "n_h2": 0.1, "c_puff": -2,
             "c_puff_hz": 900, "c_puff_ms": 2.5, "c_thump": -6, "c_room": 0.08, "hp": 80},
}

# Q-POP-1: the kit pops keep numbered names.
BUBBLES = {
    "pop_01": {"f0": 520, "rise": 0.9, "q_tau": 0.014, "att": 0.002, "room": 0.15, "length": 0.2, "tick_db": 0,
               "tick_ms": 6, "tick_hz": 1700},
    "pop_02": {"f0": 1100, "rise": 0.55, "rise_tau": 0.006, "q_tau": 0.009, "tick_db": -6, "tick_ms": 4,
               "tick_hz": 1700, "daughter": 0.3, "room": 0.1, "length": 0.15},
    "pop_03": {"f0": 330, "rise": 1.3, "rise_tau": 0.03, "q_tau": 0.03, "att": 0.005, "tick_db": 0, "tick_ms": 7,
               "tick_hz": 1700, "daughter": 0, "room": 0.08, "length": 0.25},
    "pop_04": {"f0": 720, "rise": 0.25, "q_tau": 0.008, "breath": 0.45, "tick_db": -2, "tick_ms": 5,
               "tick_hz": 1700, "daughter": 0, "room": 0.12, "length": 0.16},
    "pop_05": {"f0": 450, "rise": 0.15, "q_tau": 0.01, "tick_db": 2, "tick_ms": 6, "tick_hz": 1700, "room": 0.22,
               "length": 0.22},
}

ROUND5_BASE = {"c_nat": 1, "drive": 0, "steps": 0, "pre_db": -40, "tick_db": -40, "harm": 0, "c_breath": -60,
               "hp": 150, "lp": 15000, "fade": 0.004}
ROUND5 = {
    "chirp": {"f_hi": 6500, "f_lo": 420, "c_ptau": 0.009, "c_tau": 0.02, "c_h2": 0.08, "c_puff": -16,
              "c_puff_hz": 3500, "c_puff_ms": 1.2, "c_room": 0.08, "c_pre": -10, "c_pre_hz": 9500, "c_pre_t": 0.028},
    "mouth": {"f_hi": 1500, "f_lo": 500, "c_ptau": 0.005, "c_tau": 0.014, "c_h2": 0.12, "c_puff": -9,
              "c_puff_hz": 1800, "c_puff_ms": 1.5, "c_thump": -12, "c_room": 0.12},
    "blip": {"f_hi": 1350, "f_lo": 1150, "c_ptau": 0.006, "c_tau": 0.009, "c_h2": 0.15, "c_puff": -22,
             "c_puff_hz": 4000, "c_puff_ms": 0.8, "c_room": 0.06},
    "drop": {"f_hi": 1150, "f_lo": 230, "c_ptau": 0.016, "c_tau": 0.018, "c_h2": 0.1, "c_puff": -16,
             "c_puff_hz": 2500, "c_room": 0.1, "hp": 120},
    "plip": {"f_hi": 2000, "f_lo": 1350, "c_ptau": 0.007, "c_tau": 0.015, "c_h2": 0.06, "c_puff": -14,
             "c_puff_hz": 5000, "c_puff_ms": 1.0, "c_room": 0.2},
    "droplet": {"f_hi": 3200, "f_lo": 850, "c_ptau": 0.004, "c_tau": 0.011, "c_h2": 0.05, "c_puff": -12,
                "c_puff_hz": 6000, "c_puff_ms": 0.8, "c_room": 0.1},
}

ROUND3 = {
    "m0": {"f_hi": 9800, "f_lo": 360, "sweep": 0.055, "ease": 0.74, "steps": 0.6, "gdepth": 0.5, "step_ms": 7,
           "duty": 0.72, "s_att": 0.3, "pre_db": -3, "pre_ring": 5, "peak_at": 0.55, "start_db": -2.2,
           "tail_db": -22, "tail_hold": 0.002, "tail_tau": 0.0075, "harm": 0.22, "odd": 0.2, "tick_db": -18,
           "tick_hz": 4000, "hp": 110, "lp": 14000, "drive": 2.5, "fade": 0.008, "c_breath": -15, "c_bq": 3.0,
           "land_drop": 3, "c_in": 0.3},
    "m1": {"f_hi": 5200, "f_lo": 300, "sweep": 0.045, "ease": 1.4, "steps": 0, "pre_db": -40, "peak_at": 0.03,
           "start_db": -0.5, "tail_db": -22, "tail_hold": 0.002, "tail_tau": 0.006, "harm": 0.05, "odd": 0,
           "tick_db": -34, "tick_hz": 7000, "hp": 260, "lp": 11000, "drive": 1.0, "fade": 0.004, "c_breath": -13,
           "c_bq": 2.5, "land_drop": 2, "c_in": 0.3},
    "m2": {"style": "blip", "f_blip": 1220, "box": 0.005, "harm": 0.5102, "odd": 0.06914, "step_ms": 7,
           "b_att": 0.0018, "b_rel": 0.00202, "tick_db": -32.96618, "tick_hz": 2533.60892, "hp": 81.01064,
           "lp": 4820.49678, "drive": 5.31339, "length": 0.03, "fade": 0.02436},
    "m3": {"f_hi": 1150, "f_lo": 140, "sweep": 0.06, "ease": 1.16, "steps": 0, "pre_db": -40, "peak_at": 0.1,
           "start_db": -4, "tail_db": -12, "tail_hold": 0.003, "tail_tau": 0.0119, "harm": 0.11, "odd": 0.7,
           "tick_db": -22, "tick_hz": 6000, "hp": 300, "lp": 13800, "drive": 3.0, "fade": 0.02, "c_breath": -18,
           "c_bq": 3.0, "land_drop": 2, "c_in": 0.3},
    "m4": {"style": "blip", "f_blip": 1600, "box": 0.045, "b_drop": 4, "harm": 0.3305, "odd": 0, "step_ms": 7,
           "b_att": 0.00143, "b_rel": 0.002, "tick_db": -7.33112, "tick_hz": 5329.17327, "hp": 78.94849,
           "lp": 13005.45302, "drive": 0.61736, "length": 0.07, "fade": 0.02784},
    "m5": {"f_hi": 3200, "f_lo": 700, "sweep": 0.03, "ease": 0.8, "steps": 0, "pre_db": -40, "peak_at": 0.45,
           "start_db": -6, "tail_db": -18, "tail_hold": 0.002, "tail_tau": 0.009, "harm": 0.08, "odd": 0,
           "tick_db": -24, "tick_hz": 6500, "hp": 200, "lp": 12000, "drive": 1.5, "fade": 0.006, "c_breath": -17,
           "c_bq": 4.0, "land_drop": 1.5, "c_in": 0.3},
}
ROUND4_EDITS = {"m0": {}, "m1": {"harm": 0.08, "odd": 0, "c_breath": -10, "c_bq": 2.0, "c_air": -20}, "m2": {},
                "m3": {}, "m4": {}, "m5": {}}

FAMILIES = {
    "pro_pop": _members(ROUND6_BASE, ROUND6),
    "pop": _members({"style": "bubble"}, BUBBLES),
}
ROUND5_FAMILY = _members(ROUND5_BASE, ROUND5)
ROUND4_FAMILY = [(name, {**V2_DEFAULTS, **ROUND3[name], **ROUND4_EDITS[name]}) for name in ROUND3]


# ------------------------------------------------------- v2 targets, penalty

V2_TARGETS = {
    "pro_pop": [("length_s", 0.025, 0.16, 0.05), ("t40_ms", 4, 70, 25), ("low_db", -200, -10, 6),
                ("corr", 0.95, 1.0, 0.1)],
    "pop": [("length_s", 0.12, 0.42, 0.1), ("t40_ms", 25, 160, 40), ("low_db", -200, -18, 6)],
}


def _distance(v, lo: float, hi: float, scale: float) -> float:
    if v is None:
        return 1.0
    return (lo - v) / scale if v < lo else (v - hi) / scale if v > hi else 0.0


def _first_share(shares) -> float:
    if isinstance(shares, dict):
        shares = list(shares.values())
    return float(shares[0])


def pro_penalty(x: np.ndarray, cat: str) -> float:
    """The version 2 profile penalty of a category (the notify measurement); 0 without targets."""
    targets = V2_TARGETS.get(cat)
    if not targets:
        return 0.0
    from hark.forge.recipes.risers import notify  # same layer; imported only when a penalty is asked for

    m = dict(notify.measure(x))
    m["low_db"] = 10 * math.log10(10 ** (_first_share(m["shares"]) / 10) + 1e-12)
    return float(sum(min(2.0, _distance(m.get(k), lo, hi, s)) for k, lo, hi, s in targets))


def extra_for(cat: str, weight: float = 1.0):
    """The critic extra function: minus weight x the version 2 penalty."""
    return lambda x, row=None: -weight * pro_penalty(x, cat)


V2 = types.SimpleNamespace(
    MODULE=__name__, TARGET=V2_TARGET, TARGETS={"pro_pop": V2_TARGET, "pop": TARGET}, ALTS=V2_ALTS,
    REFSET="pro_pop", SPACE=V2_SPACE, DEFAULTS=V2_DEFAULTS, render=render_v2, FAMILIES=FAMILIES,
    family_space=family_space, extra_for=extra_for, pro_penalty=pro_penalty,
)
