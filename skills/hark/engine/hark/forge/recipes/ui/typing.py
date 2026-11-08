"""Recipe typing: a designed, high-passed digital typing tick, and a typed run in one file.

The single key is the module recipe. The typed run is the recipe object ``RUN``.
Both write to the same seed file; their critic contexts differ by target text.
"""

from __future__ import annotations

import math
import types

import numpy as np

from hark.forge.engine import dsp
from hark.forge.recipes.ui import _layers as L

TARGET = "a short crisp high-passed digital typing tick for a motion graphics title"
RUN_TARGET = "a quick run of crisp high-passed digital typing ticks for an animated title"
TARGETS = {"pro_typing_key": TARGET, "pro_typing_run": RUN_TARGET}
ALTS = ["a mouse click", "a drop of water", "a hi-hat", "a spray can hiss"]
RUN_ALTS = ["rain on a window", "a hi-hat pattern", "a clock ticking", "popcorn popping"]
REFSET = "pro_typing_key"

SPACE = {
    "ridge": (6500, 13500, "log"), "ridge_db": (0.0, 12.0), "ridge_q": (0.5, 2.5, "log"),
    "lowcut": (2500, 5000, "log"), "hicut": (12000, 20000, "log"), "att": (0.0015, 0.005, "log"),
    "att_curve": (0.6, 3.0, "log"), "tau1": (0.0012, 0.006, "log"), "tail_db": (-40.0, -12.0),
    "tau2": (0.008, 0.05, "log"), "touch_db": (-30.0, -4.0), "touch_dt": (0.0015, 0.008, "log"),
    "rattle_db": (-40.0, -18.0), "tink_db": (-40.0, 2.0), "rel_db": (-40.0, -14.0),
    "rel_dt": (0.035, 0.075, "log"), "length": (0.05, 0.1), "gate": (0.004, 0.02, "log"), "key": (0, 999, "int"),
    "grain": (0.0, 1.0), "grain_rate": (300, 4000, "log"), "tink_glide": (-12.0, 12.0),
}

DEFAULTS = {
    "ridge": 9600.0, "ridge_db": 6.0, "ridge_q": 1.0, "lowcut": 3400.0, "hicut": 16000.0, "att": 0.003,
    "att_curve": 1.2, "tau1": 0.0025, "tail_db": -24.0, "tau2": 0.016, "touch_db": -12.0, "touch_dt": 0.003,
    "rattle_db": -30.0, "tink_db": -32.0, "rel_db": -40.0, "rel_dt": 0.05, "length": 0.075, "gate": 0.008,
    "key": 7, "grain": 0.0, "grain_rate": 1500.0, "tink_glide": 0.0,
}

RUN_SPACE = dict(SPACE, rate=(8.5, 14.0), jit=(0.12, 0.35), lvl_jit=(1.0, 4.5), accent=(0.0, 0.35),
                 gap=(0.0, 0.2), var=(0.01, 0.08), dur=(1.1, 1.9), swell_db=(-60.0, -26.0), swell_len=(0.2, 0.5))
RUN_DEFAULTS = dict(DEFAULTS, rate=11.5, jit=0.22, lvl_jit=2.5, accent=0.15, gap=0.08, var=0.04, dur=1.5,
                    swell_db=-42.0, swell_len=0.3)

OFF_DB = -39.5
LAYOUTS = 5


def key_rng(key: int, part: int = 0) -> np.random.Generator:
    """The fixed per-key generator (only part 0 is used)."""
    return np.random.default_rng(int(key) * 7919 + part * 104729 + 31)


# ------------------------------------------------------------ building blocks

def band(w: np.ndarray, p: dict, ridge: float | None = None) -> np.ndarray:
    """Y1: order-6 high-pass, ridge boost, gentle low-pass, and a steep cut at 18.5 kHz."""
    y = dsp.butter(w, "high", p["lowcut"], 6)
    y = dsp.peq(y, p["ridge"] if ridge is None else ridge, p["ridge_db"], p["ridge_q"])
    y = dsp.butter(y, "low", p["hicut"], 2)
    return np.asarray(dsp.butter(y, "low", 18500, 8), dtype=float)


def burst_env(n: int, att: float, curve: float, tau1: float, tail: float, tau2: float) -> np.ndarray:
    """Y2: a power-curve attack, then a fast drop into a faint long tail."""
    n = max(0, n)
    a = max(2, L.n_of(att))
    k = min(a, n)
    e = np.empty(n)
    e[:k] = (np.arange(k) / a) ** curve
    td = (np.arange(k, n) - (k - 1)) / L.SR
    e[k:] = (1 - tail) * np.exp(-td / tau1) + tail * np.exp(-td / tau2)
    return e


def _velvet(n: int, r: np.random.Generator, rate: float) -> np.ndarray:
    blk = max(1, int(L.SR / rate))
    starts = np.arange(0, n, blk)
    pos = starts + r.integers(0, blk, len(starts))
    sign = np.where(r.uniform(size=len(starts)) < 0.5, -1.0, 1.0)
    amp = r.uniform(0.3, 1.0, len(starts))
    imp = np.zeros(n)
    keep = pos < n
    imp[pos[keep]] = (sign * amp)[keep]
    return imp / (L.rms(imp) + 1e-12)


def _rattle(env: np.ndarray, p: dict, r: np.random.Generator, kr: np.random.Generator, k_hit: int) -> None:
    n = len(env)
    for _ in range(int(kr.integers(1, 4))):
        tj = k_hit / L.SR + kr.uniform(0.012, 0.045) * (1 + 0.1 * r.standard_normal())
        lvl_db = p["rattle_db"] + kr.uniform(-4.0, 2.0)
        tau = kr.uniform(0.001, 0.003)
        i = L.n_of(tj)
        if i < n - 8:
            t = L.t_of(n - i)
            env[i:] += 10 ** (lvl_db / 20) * np.exp(-t / tau) * np.minimum(1.0, t / 0.0004)


def _tink(y: np.ndarray, p: dict, r: np.random.Generator, kr: np.random.Generator, k_hit: int,
          ridge_k: float) -> None:
    fs = np.sort(kr.uniform(5000, 12500, 3)) * ridge_k
    taus = kr.uniform(0.002, 0.006, 3)
    m = len(y) - k_hit
    if m <= 0:
        return
    t = L.t_of(m)
    tk = np.zeros(m)
    gl = p["tink_glide"]
    for f, a, tau in zip(fs, (1.0, 0.6, 0.4), taus):
        f1 = f * (1 + 0.003 * r.standard_normal())
        if gl != 0:
            fi = np.minimum(f1 * 2 ** ((gl / 12) * (1 - np.exp(-t / 0.005))), 17000.0)
            ph = 2 * np.pi * np.cumsum(fi) / L.SR
        else:
            ph = 2 * np.pi * f1 * t
        tk += a * np.exp(-t / tau) * np.sin(ph + r.uniform(0, 6.28))
    ramp = min(24, m)
    tk[:ramp] *= np.arange(ramp) / 24
    w = L.n_of(0.01)
    y[k_hit:] += 10 ** (p["tink_db"] / 20) * tk * L.rms(y[:w]) / (L.rms(tk[:w]) + 1e-12)


def _release(y: np.ndarray, p: dict, r: np.random.Generator, t_rel: float, ridge: float, tail: float) -> None:
    i = L.n_of(t_rel)
    m = len(y) - i
    if m <= 0:
        return
    er = burst_env(m, 0.0008, 1.0, 0.6 * p["tau1"], 0.5 * tail, 0.4 * p["tau2"])
    rr = band(r.standard_normal(m) * er, p, ridge=min(1.15 * ridge, 16000.0))
    y[i:] += 10 ** (p["rel_db"] / 20) * L.unit(rr) * L.peak(y)


def _edit(y: np.ndarray, gate: float, end: int) -> np.ndarray:
    g = max(1, min(len(y) - 2, L.n_of(gate)))
    end = min(len(y), end)
    out = np.array(y[:end], dtype=float)
    k0 = max(0, end - g)
    out[k0:] *= 0.5 + 0.5 * np.cos(np.pi * np.arange(1, end - k0 + 1) / (end - k0))
    return out


def one_key(p: dict, r: np.random.Generator, force: float = 1.0, ridge_k: float = 1.0) -> np.ndarray:
    """One mono key tick: touch, hit, optional rattle, tink and release, then the edit."""
    kr = key_rng(p["key"])
    length = p["length"] * (1 + 0.06 * r.standard_normal())
    rel_on = p["rel_db"] > OFF_DB
    t_rel = p["touch_dt"] + p["rel_dt"] * (1 + 0.08 * r.standard_normal())
    n = L.n_of(max(length, t_rel + 0.025 if rel_on else 0.0))
    ridge = p["ridge"] * ridge_k * (1 + 0.015 * r.standard_normal())
    w = r.standard_normal(n)
    if p["grain"] > 0:
        w = (1 - p["grain"]) * w + p["grain"] * _velvet(n, r, p["grain_rate"])
    k_hit = min(n, L.n_of(p["touch_dt"] * (1 + 0.1 * r.standard_normal())))
    tail = 10 ** ((p["tail_db"] + 1.5 * r.standard_normal()) / 20)
    hit = np.zeros(n)
    hit[k_hit:] = burst_env(n - k_hit, 0.6 * p["att"], p["att_curve"], p["tau1"], tail, p["tau2"])
    touch = 10 ** (p["touch_db"] / 20) * burst_env(n, p["att"], p["att_curve"], 0.7 * p["tau1"], 0.3 * tail,
                                                    0.5 * p["tau2"])
    env = np.maximum(hit, touch)
    if p["rattle_db"] > OFF_DB:
        _rattle(env, p, r, kr, k_hit)
    y = band(w * env, p, ridge=min(ridge, 16000.0))
    if p["tink_db"] > OFF_DB:
        _tink(y, p, r, kr, k_hit, ridge_k)
    if rel_on:
        _release(y, p, r, t_rel, ridge, tail)
    y = _edit(y, p["gate"], n if rel_on else L.n_of(length))
    return force * L.unit(y)


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """One key: stereo (2 equal channels) float64 at 48 kHz, 50-100 ms; peak about 1 x force."""
    p = L.merged(DEFAULTS, p)
    force = math.exp(0.08 * rng.standard_normal())
    y = dsp.fade(one_key(p, rng, force), 0.0, 0.0015)
    return L.headroom(np.column_stack([y, y]).astype(np.float64))


# ------------------------------------------------------------------ typed run

def key_times(p: dict, r: np.random.Generator) -> list[float]:
    """Jittered key onsets with word gaps, from 0 until dur - 0.07 s."""
    med = 1 / p["rate"]
    times, t = [], 0.0
    while t < p["dur"] - 0.07:
        times.append(t)
        d = med * math.exp(p["jit"] * r.standard_normal())
        if r.uniform() < p["gap"]:
            d *= r.uniform(1.4, 1.7)
        t += min(max(d, 0.62 * med), 1.75 * med)
    return times


def _pick_layout(r: np.random.Generator, prev: int) -> int:
    k = int(r.integers(0, LAYOUTS))
    if k == prev:
        k = (k + 1 + int(r.integers(0, LAYOUTS - 1))) % LAYOUTS
    return k


def _place_keys(y: np.ndarray, p: dict, r: np.random.Generator, times: list[float]) -> None:
    prev, lvl = -1, 0.0
    for t in times:
        k = _pick_layout(r, prev)
        prev = k
        lvl = 0.6 * lvl + p["lvl_jit"] * r.standard_normal()
        gain_db = lvl + (r.uniform(3, 5) if r.uniform() < p["accent"] else 0.0)
        ridge_k = math.exp(p["var"] * (k - 2) / 2 + 0.3 * p["var"] * r.standard_normal())
        key = one_key(dict(p, key=int(p["key"]) + 101 * k), r, 1.0, ridge_k)
        L.add_at(y, key, L.n_of(t) if t > 0 else 0, 10 ** (min(gain_db, 6.0) / 20))


def _swell(y: np.ndarray, p: dict, r: np.random.Generator, last: float) -> None:
    m = min(len(y), L.n_of(min(p["swell_len"], last)))
    s = dsp.butter(dsp.noise(m / L.SR, r, "brown")[:m], "band", (90, 900), 2)
    s = L.unit(s * np.linspace(0, 1, m) ** 2.5)
    y[len(y) - m:] += 10 ** (p["swell_db"] / 20) * L.peak(y) * s


def render_run(p: dict, rng: np.random.Generator) -> np.ndarray:
    """A typed run as one file: stereo (2 equal channels) float64 at 48 kHz, about dur s."""
    p = L.merged(RUN_DEFAULTS, p)
    times = key_times(p, rng)
    n = L.n_of(times[-1] + p["length"] + 0.06)
    y = np.zeros(n)
    _place_keys(y, p, rng, times)
    if p["swell_db"] > -59:
        _swell(y, p, rng, times[-1])
    bed = dsp.butter(rng.standard_normal(n), "band", (p["lowcut"], 16000), 2)
    y = y + bed / (np.std(bed) + 1e-12) * L.peak(y) * 10 ** (-68 / 20)
    y = dsp.fade(y, 0.002, 0.004)
    return L.headroom(np.column_stack([y, y]).astype(np.float64))


# --------------------------------------------------------------- study checks

def env_db(x: np.ndarray) -> np.ndarray:
    """Y3: 1 ms frames of mean square in dB, relative to the loudest frame."""
    m = L.fit(_mono(x), max(48, len(_mono(x))))
    k = len(m) // 48
    fr = np.square(m[: k * 48]).reshape(k, 48).mean(axis=1)
    e = 10 * np.log10(fr + 1e-14)
    return e - e.max()


def _mono(x: np.ndarray) -> np.ndarray:
    x = np.asarray(x, dtype=float)
    return x.mean(axis=1) if x.ndim == 2 else x


def _fall_ms(m: np.ndarray, db: float) -> float | None:
    sm = np.convolve(np.square(m), np.ones(480) / 480, mode="same")[::48]
    e = 10 * np.log10(sm + 1e-14)
    e = e - e.max()
    ip = int(np.argmax(e))
    below = np.nonzero(e[ip:] <= db)[0]
    return float(below[0]) if len(below) else None


def _lo2k_db(m: np.ndarray) -> float:
    pw = np.abs(np.fft.rfft(m * np.hanning(len(m)))) ** 2
    f = np.fft.rfftfreq(len(m), 1 / L.SR)
    return round(10 * math.log10(pw[f < 2000].sum() / (pw.sum() + 1e-30) + 1e-14), 1)


def key_stats(x: np.ndarray) -> dict:
    """attack_ms, t20_ms, t40_ms, lo2k_db and length_s of one key render."""
    m = _mono(x)
    e = env_db(m)
    ip = int(np.argmax(e))
    i20 = int(np.nonzero(e[: ip + 1] >= -20)[0][0])
    return {"attack_ms": float(ip - i20), "t20_ms": _fall_ms(m, -20), "t40_ms": _fall_ms(m, -40),
            "lo2k_db": _lo2k_db(m), "length_s": round(len(m) / L.SR, 3)}


def outside(v: float | None, lo: float, hi: float, scale: float) -> float:
    """Distance of v from [lo, hi] in units of scale; 1 when v is missing."""
    if v is None:
        return 1.0
    return max(0.0, lo - v, v - hi) / scale


def key_penalty(x: np.ndarray) -> float:
    """The key extra term, 0 to -3: film attack, decay, top-only band and length."""
    s = key_stats(x)
    terms = (outside(s["attack_ms"], 1.5, 5.5, 3), outside(s["t20_ms"], 10, 19, 6), outside(s["t40_ms"], 42, 72, 25),
             max(0.0, s["lo2k_db"] + 30) / 10, outside(s["length_s"], 0.05, 0.1, 0.03))
    return -min(3.0, 0.75 * sum(min(1.0, v) for v in terms))


def run_stats(x: np.ndarray) -> dict:
    """keys, med_ms, min_ms, max_ms and lo2k_db of a typed run."""
    m = _mono(x)
    e = env_db(m)
    kept: list[int] = []
    for i in np.nonzero((e[1:] > -32) & (e[:-1] <= -32))[0] + 1:
        if not kept or i - kept[-1] >= 45:
            kept.append(int(i))
    d = np.diff(kept) if len(kept) >= 2 else np.array([0])
    return {"keys": len(kept), "med_ms": float(np.median(d)), "min_ms": float(d.min()), "max_ms": float(d.max()),
            "lo2k_db": _lo2k_db(m)}


def run_penalty(x: np.ndarray) -> float:
    """The run extra term, 0 to -3: key spacing, top-only band and at least 10 keys."""
    s = run_stats(x)
    total = min(1.0, outside(s["med_ms"], 72, 100, 20)) + min(1.0, max(0.0, s["lo2k_db"] + 28) / 10)
    total += 0.5 if s["keys"] < 10 else 0.0
    return -min(3.0, 0.75 * total)


def extra_for(cat: str, weight: float = 1.0):
    """The critic extra function of ``pro_typing_key`` or ``pro_typing_run``, else None."""
    fn = {"pro_typing_key": key_penalty, "pro_typing_run": run_penalty}.get(cat)
    if fn is None:
        return None
    return lambda x, row=None: weight * fn(x)


RUN = types.SimpleNamespace(
    MODULE=__name__, TARGET=RUN_TARGET, TARGETS={"pro_typing_run": RUN_TARGET}, ALTS=RUN_ALTS,
    REFSET="pro_typing_run", SPACE=RUN_SPACE, DEFAULTS=RUN_DEFAULTS, render=render_run, extra_for=extra_for,
)
