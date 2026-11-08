"""hover: a short, quiet, high tick for a cursor hover (recipes-motion part 4, section 14).

A ping (one decaying sine, an optional partner above it), a noise tick whose band falls
during the decay, an optional repeat tick, and a contact impulse with micro-grains.
``render`` does not merge DEFAULTS: pass a full set (``family_params`` gives one).
"""

from __future__ import annotations

from typing import Any

import numpy as np

from hark.forge.engine import dsp, fx
from hark.forge.recipes.motion import _util as u

SR = u.SR
PRE_S = 0.002
BUFFER_S = 0.14
DECAY_50DB = 5.76e-3

TARGETS: dict[str, str] = {
    "hover_tick": "a tiny high soft tick when the cursor hovers over a button",
    "pro_hover_tick": "a tiny airy digital tick for an interface animation",
}
TARGET = TARGETS["hover_tick"]
ALTS = ["a clock ticking", "a drop of water", "a mouse click", "a cricket chirp"]
REFSET = "hover_tick"

SPACE: dict[str, tuple] = {
    "ping": (0.0, 1.0), "ping_f": (3000.0, 15000.0, "log"), "ping_ms": (1.0, 8.0, "log"),
    "ping_up": (0.0, 0.5), "ping_ratio": (1.5, 2.4), "tick_f": (2500.0, 11000.0, "log"),
    "tick_q": (0.8, 4.0, "log"), "tick_ms": (1.5, 20.0, "log"), "snap": (1.0, 3.0),
    "fall": (0.0, 1.5), "detent": (0.0, 0.8), "detent_dt": (0.004, 0.016),
    "attack_ms": (0.05, 3.0, "log"), "hp": (500.0, 2500.0, "log"), "lp": (8000.0, 20000.0, "log"),
    "sat": (0.0, 6.0), "length": (0.02, 0.11), "release_ms": (3.0, 25.0, "log"),
    "width": (0.0, 1.0), "verb": (0.0, 0.35), "rt": (0.04, 0.3, "log"), "contact": (0.0, 40.0),
    "contact_ms": (0.08, 1.0, "log"), "grains": (0.0, 0.5),
}

DEFAULTS: dict[str, float] = {
    "ping": 0.5, "ping_f": 8000.0, "ping_ms": 6.0, "ping_up": 0.15, "ping_ratio": 1.9,
    "tick_f": 6000.0, "tick_q": 1.6, "tick_ms": 6.0, "snap": 1.5, "fall": 0.6, "detent": 0.0,
    "detent_dt": 0.008, "attack_ms": 0.3, "hp": 900.0, "lp": 16000.0, "sat": 1.0,
    "length": 0.045, "release_ms": 10.0, "width": 0.3, "verb": 0.0, "rt": 0.1, "contact": 0.0,
    "contact_ms": 0.2, "grains": 0.0,
}

_LP = (13000, 17000, "log")
_VERB = (0, 0.06)
_ROOM = {"verb": (0.05, 0.3), "rt": (0.05, 0.25, "log")}

FAMILIES: dict[str, dict[str, dict[str, Any]]] = {
    "pro_hover_tick": {
        "ping": {"detent": 0, "ping": (0.6, 0.9), "ping_f": (8000, 14000, "log"),
                 "ping_ms": (1.5, 3, "log"), "snap": (2, 3), "contact": (3, 7), "grains": 0,
                 "contact_ms": (0.25, 0.6, "log"), "width": (0.6, 1), "verb": _VERB, "lp": _LP,
                 "length": (0.025, 0.045)},
        "air": {"detent": 0, "ping": (0, 0.15), "tick_f": (7000, 11000, "log"), "fall": (0.4, 1.2),
                "tick_ms": (2, 6, "log"), "snap": (2, 3), "contact": (12, 30),
                "grains": (0.05, 0.35), "contact_ms": (0.3, 0.8, "log"), "width": (0.8, 1),
                "verb": _VERB, "lp": _LP, "length": (0.03, 0.055)},
        "glass": {"detent": 0, "ping": (0.3, 0.6), "ping_f": (7000, 11000, "log"),
                  "ping_ms": (1.5, 3, "log"), "snap": (2, 3), "tick_f": (7000, 11000, "log"),
                  "ping_up": (0.15, 0.4), "contact": (6, 12), "grains": (0, 0.15),
                  "hp": (1000, 2200, "log"), "contact_ms": (0.25, 0.6, "log"), "width": (0.6, 1),
                  "verb": _VERB, "lp": _LP, "length": (0.03, 0.05)},
        "soft": {"detent": 0, "ping": (0, 0.3), "ping_ms": (1.5, 3, "log"),
                 "tick_f": (5000, 8000, "log"), "tick_ms": (2, 6, "log"), "snap": (1.5, 2.5),
                 "attack_ms": (0.3, 1.2, "log"), "contact": (1.5, 4), "grains": 0,
                 "contact_ms": (0.4, 1, "log"), "width": (0.6, 1), "hp": (800, 1500, "log"),
                 "verb": (0, 0.08), "lp": (12000, 16000, "log"), "length": (0.03, 0.06)},
        "detent": {"ping": (0, 0.35), "ping_ms": (1, 3, "log"), "tick_f": (7000, 11000, "log"),
                   "snap": (2, 3), "tick_ms": (1.5, 4, "log"), "detent": (0.35, 0.8),
                   "detent_dt": (0.006, 0.012), "contact": (12, 30), "grains": (0.05, 0.35),
                   "contact_ms": (0.2, 0.5, "log"), "width": (0.7, 1), "verb": _VERB, "lp": _LP,
                   "length": (0.025, 0.045)},
    },
    "hover_tick": {
        "high": {"detent": 0, "ping": (0, 0.3), "ping_ms": (1, 3, "log"),
                 "tick_f": (8000, 11000, "log"), "tick_ms": (1.5, 5, "log"), "snap": (1.5, 3),
                 "hp": (1500, 2500, "log"), **_ROOM, "length": (0.04, 0.07)},
        "mid": {"detent": 0, "ping": (0, 0.3), "ping_ms": (1, 4, "log"),
                "tick_f": (3000, 5000, "log"), "tick_ms": (4, 14, "log"), "snap": (1, 2),
                "hp": (700, 1400, "log"), **_ROOM, "length": (0.065, 0.11)},
        "ping": {"detent": 0, "ping": (0.75, 1), "ping_f": (5000, 9000, "log"),
                 "ping_ms": (2, 7, "log"), "snap": (1.5, 3), **_ROOM, "length": (0.045, 0.085)},
        "air": {"detent": 0, "ping": (0, 0.12), "tick_f": (5000, 9000, "log"), "fall": (0.6, 1.4),
                "tick_ms": (3, 12, "log"), **_ROOM, "length": (0.05, 0.095)},
        "detent": {"ping": (0, 0.3), "ping_ms": (1, 3, "log"), "tick_f": (4500, 8000, "log"),
                   "snap": (1.5, 3), "detent": (0.35, 0.8), "detent_dt": (0.006, 0.014), **_ROOM,
                   "length": (0.05, 0.095)},
    },
}


def narrow(space: dict, fam: dict) -> dict:
    """The space with each family range as the key's range and each fixed value pinned."""
    out = dict(space)
    for k, v in fam.items():
        if isinstance(v, tuple):
            kind = v[2] if len(v) > 2 else ("int" if len(space.get(k, ())) > 2
                                            and space[k][2] == "int" else None)
            out[k] = (v[0], v[1], kind) if kind else (v[0], v[1])
        else:
            out[k] = [v]
    return out


def centre(defaults: dict, fam: dict) -> dict:
    """The defaults with each family range replaced by its centre."""
    out = dict(defaults)
    for k, v in fam.items():
        if not isinstance(v, tuple):
            out[k] = v
            continue
        c = float(np.sqrt(v[0] * v[1])) if len(v) > 2 and v[2] == "log" else (v[0] + v[1]) / 2.0
        out[k] = int(round(c)) if isinstance(defaults.get(k), int) else c
    return out


def family_space(cat: str, fam: str) -> dict:
    """The search space of one family."""
    return narrow(SPACE, FAMILIES[cat][fam])


def family_params(cat: str, fam: str) -> dict:
    """A full parameter set at the centre of one family."""
    return centre(DEFAULTS, FAMILIES[cat][fam])


def partials(f0: float, ratios, amps, tau: float, seconds: float, att_s: float,
             rng: np.random.Generator) -> np.ndarray:
    """A sum of damped sines with a sin^2 attack, unit peak (the click recipe helper)."""
    n = u.samples(seconds)
    t = u.times(n)
    y = np.zeros(n)
    for ratio, amp in zip(ratios, amps):
        f = f0 * ratio
        if f >= 0.95 * SR / 2 or amp == 0:
            continue
        fj = f * (1 + 0.004 * rng.standard_normal())
        phase = rng.uniform(0, 2 * np.pi)
        y += amp * np.exp(-t / max(tau * ratio**-0.6, 1e-5)) * np.sin(2 * np.pi * fj * t + phase)
    a = min(n, max(2, u.samples(att_s)))
    y[:a] *= u.sin2_in(a)
    return u.unit_peak(y)


def burst(seconds: float, f_lo: float, f_hi: float, att_s: float, order: int,
          rng: np.random.Generator) -> np.ndarray:
    """A decaying band of noise, unit peak (the click recipe helper)."""
    n = u.samples(max(6 * seconds, 0.004))
    e = np.exp(-u.times(n) / max(seconds, 5e-5)) ** 2
    a = min(n, max(2, u.samples(att_s)))
    e[:a] *= u.sin2_in(a)
    lo, hi = max(40.0, f_lo), min(21500.0, f_hi)
    y = dsp.butter(rng.standard_normal(n), "band", (lo, max(1.05 * lo, hi)), order)
    return u.unit_peak(y * e)


def gate(x: np.ndarray, length: float, release: float, start: float) -> np.ndarray:
    """Cut x at start + length with a raised-cosine release before the cut."""
    end = min(len(x), u.samples(start + length))
    rl = max(8, min(end - 1, u.samples(release)))
    g = np.ones(end)
    if rl > 1:
        g[end - rl :] = 0.5 + 0.5 * np.cos(np.pi * np.arange(rl) / (rl - 1))
    return x[:end] * (g[:, None] if x.ndim == 2 else g)


def _tick(p: dict, f_scale: float, rng: np.random.Generator) -> np.ndarray:
    tau = p["tick_ms"] / 1000.0
    m = u.samples(max(0.012, 6 * tau))
    t = u.times(m)
    fc = p["tick_f"] * f_scale * 2 ** (-p["fall"] * (1 - np.exp(-t / (1.5 * tau))))
    y = dsp.svf(rng.standard_normal(m), np.minimum(fc, 20000.0), p["tick_q"], "band")
    e = np.exp(-t / tau) ** p["snap"]
    a = min(m, max(2, u.samples(p["attack_ms"] / 1000.0)))
    e[:a] *= u.sin2_in(a)
    return u.unit_peak(y * e)


def _contact(p: dict, peak: float, size: int, rng: np.random.Generator) -> np.ndarray:
    ct = np.zeros((size, 2))
    amount = p.get("contact", 0.0)
    if amount <= 0.01:
        return ct
    level = amount * peak
    theta = 0.5 * np.pi * min(1.0, p["width"])
    events = [(0.0, 1.0, p["contact_ms"])]
    if p.get("grains", 0.0) > 0.005:
        for _ in range(4):
            sign = 1.0 if rng.random() < 0.5 else -1.0
            events.append((rng.uniform(0.002, 0.025),
                           p["grains"] * 10 ** (rng.uniform(-20, 0) / 20.0) * sign, 0.1))
    for when, amp, decay in events:
        c1 = burst(decay / 1000.0, 2000, 15000, 0.00005, 4, rng)
        c2 = burst(decay / 1000.0, 2000, 15000, 0.00005, 4, rng)
        pair = np.column_stack([c1, np.cos(theta) * c1 + np.sin(theta) * c2])
        u.add_at(ct, level * amp * pair, u.samples(when))
    return ct


def one(p: dict, f_scale: float, rng: np.random.Generator) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    """One hit: (ping share x ping, tick, stereo contact), all of one length."""
    k = 2 if p["ping_up"] > 0.01 else 1
    pg = partials(p["ping_f"] * f_scale, [1.0, p["ping_ratio"]][:k], [1.0, p["ping_up"]][:k],
                  p["ping_ms"] / 1000.0, 0.1, max(0.0001, p["attack_ms"] / 1000.0), rng)
    tk = _tick(p, f_scale, rng)
    size = max(len(pg), len(tk))
    pg = np.pad(pg, (0, size - len(pg)))
    tk = (1 - p["ping"]) * np.pad(tk, (0, size - len(tk)))
    peak = max(float(np.max(np.abs(tk))), float(np.max(np.abs(p["ping"] * pg))))
    return p["ping"] * pg, tk, _contact(p, peak, size, rng)


def _place(bufs: list[np.ndarray], parts: tuple, gain: float, at: float) -> None:
    start = u.samples(at) if at > 0 else 0
    for buf, part in zip(bufs, parts):
        u.add_at(buf, gain * part, start)


def _hits(p: dict, rng: np.random.Generator) -> np.ndarray:
    lvl = 10 ** (0.03 * rng.standard_normal())
    fs = float(np.exp(0.01 * rng.standard_normal()))
    n = u.samples(BUFFER_S)
    bufs = [np.zeros(n), np.zeros(n), np.zeros((n, 2))]
    _place(bufs, one(p, fs, rng), lvl, PRE_S)
    if p["detent"] > 0.02:
        _place(bufs, one(p, 1.06 * fs, rng), p["detent"] * lvl, PRE_S + p["detent_dt"])
    pg, tk, ct = bufs
    return np.column_stack([pg, pg]) + fx.decorrelate(tk, p["width"], cut=1500.0)[:n] + ct


def _ring_s(p: dict) -> float:
    ping = p["ping_ms"] if p["ping"] > 0.05 else 0.0
    tick = p["tick_ms"] / p["snap"] if p["ping"] < 0.95 else 0.0
    return DECAY_50DB * max(ping, tick)


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """One take of the hover tick (section 14.6), float64 stereo at 48 kHz."""
    st = _hits(p, rng)
    st = dsp.butter(dsp.butter(st, "high", p["hp"], 4), "low", p["lp"], 2)
    if p.get("contact", 0.0) > 0.01:
        st = u.unit_peak(st)
    if p["sat"] > 0.05:
        st = fx.saturate(st, p["sat"], mode="tanh", level="peak")
    if p["verb"] > 0.003:
        st = fx.reverb(st, rng, rt60=p["rt"], size=0.1, predelay=0.002, damp=0.5, wet=p["verb"],
                       early_db=-10.0, width=1.0, stereo=True, lowcut=1500.0)
    last = p["detent_dt"] if p["detent"] > 0.02 else 0.0
    release = p["release_ms"] / 1000.0
    st = gate(np.asarray(st, dtype=float), max(p["length"], last + _ring_s(p) + release),
              release, PRE_S)
    st = fx.limit(st, -1.0, true_peak=True)
    return np.asarray(dsp.fade(st, 0.0005, 0.002), dtype=float)
