"""Shared building blocks of the UI recipes (click layers and family boxes).

click, toggle, keypress, typing and popin build on these blocks. The module name
starts with ``_``, so the recipe registry does not list it as a recipe.
"""

from __future__ import annotations

import math
import os
from typing import Any

import numpy as np

from hark.forge.engine import dsp, fx

SR = 48000
Params = dict[str, Any]


# ----------------------------------------------------------------- small tools

def n_of(seconds: float) -> int:
    """Sample count of ``seconds``: at least 1."""
    return max(1, int(round(seconds * SR)))


def t_of(n: int) -> np.ndarray:
    return np.arange(n) / SR


def peak(x: np.ndarray) -> float:
    return float(np.max(np.abs(x))) if len(x) else 0.0


def unit(x: np.ndarray) -> np.ndarray:
    """Divide by the peak (plus a small guard against silence)."""
    return x / (peak(x) + 1e-12)


def rms(x: np.ndarray) -> float:
    return float(np.sqrt(np.mean(np.square(x)))) if len(x) else 0.0


def merged(defaults: Params, p: Params | None) -> Params:
    """The defaults with ``p`` laid over them."""
    out = dict(defaults)
    out.update(p or {})
    return out


def add_at(dst: np.ndarray, x: np.ndarray, start: int, gain: float = 1.0) -> np.ndarray:
    """Add ``gain * x`` into ``dst`` from sample ``start``; cut at the end of ``dst``."""
    if start >= len(dst) or start < 0:
        return dst
    k = min(len(x), len(dst) - start)
    dst[start:start + k] += gain * x[:k]
    return dst


def fit(x: np.ndarray, n: int) -> np.ndarray:
    """Cut or zero-pad ``x`` to ``n`` samples along axis 0."""
    if len(x) >= n:
        return x[:n]
    pad = np.zeros((n - len(x),) + x.shape[1:])
    return np.concatenate([x, pad])


def onset_ramp(x: np.ndarray, rise: float) -> np.ndarray:
    """Multiply the first max(2, N(rise)) samples by sin^2 of a 0..pi/2 ramp."""
    k = min(len(x), max(2, n_of(rise)))
    x[:k] *= np.sin(np.linspace(0, np.pi / 2, k)) ** 2
    return x


def db(v: float) -> float:
    return 10 ** (v / 20)


def ref_root() -> str:
    """The reference root: ``HARK_REFS`` (default ``refs``), with ``/`` separators."""
    root = os.environ.get("HARK_REFS", "refs").replace("\\", "/")
    return root.rstrip("/") or root


# ------------------------------------------------------------ the layer blocks

def band_burst(seconds: float, r: np.random.Generator, f_lo: float, f_hi: float, rise: float = 0.0003,
               order: int = 2) -> np.ndarray:
    """L1: a noise burst in a band with a squared exponential decay, peak 1."""
    n = n_of(max(6 * seconds, 0.004))
    lo = max(40.0, f_lo)
    hi = max(1.05 * lo, min(21500.0, f_hi))
    w = dsp.butter(r.standard_normal(n), "band", (lo, hi), order)
    w = w * np.exp(-t_of(n) / max(seconds, 5e-5)) ** 2
    return unit(onset_ramp(w, rise))


def partials(f0: float, ratios, amps, tau: float, seconds: float, r: np.random.Generator, glide: float = 0.0,
             rise: float = 0.0002) -> np.ndarray:
    """L2: damped partials at f0 x ratio, each with decay tau x ratio^-0.6, peak 1."""
    freqs = [f0 * q for q in ratios]
    taus = [tau * q ** -0.6 for q in ratios]
    y = dsp.modal(freqs, taus, list(amps), seconds, r, jitter=0.004, glide=glide)
    return unit(onset_ramp(np.asarray(y, dtype=float), rise))


def designed_thump(f: float, seconds: float, rise: float, hold_s: float, rel_s: float, drop: float = 0.1,
                   harm: float = 0.1, r: np.random.Generator | None = None) -> np.ndarray:
    """L3: a sine thump with a short downward glide, raised-cosine rise, hold and release, peak 1."""
    n = n_of(seconds)
    t = t_of(n)
    fi = f * (1 + drop * np.exp(-t / max(rise + 0.5 * hold_s, 0.002)))
    start = r.uniform(0, 2 * np.pi) if r is not None else 0.0
    ph = np.cumsum(2 * np.pi * fi / SR) + start
    y = np.sin(ph) + harm * np.sin(2 * ph + 0.3)
    a = max(2, n_of(rise))
    e = np.ones(n)
    k = min(n, a)
    e[:k] = 0.5 - 0.5 * np.cos(np.pi * np.arange(k) / a)
    th = a + n_of(hold_s)
    if th < n:
        e[th:] = np.exp(-(t[th:] - t[th]) / max(rel_s, 0.001))
    return unit(y * e)


def end_gate(x: np.ndarray, length: float, release: float, start: float = 0.0) -> np.ndarray:
    """L4: a raised-cosine end gate that ends at start + length; the signal is cut there."""
    n = len(x)
    end = min(n, n_of(start + length))
    rl = max(8, min(end - 1, n_of(release)))
    g = np.ones(end)
    k0 = max(0, end - rl)
    g[k0:] = 0.5 + 0.5 * np.cos(np.linspace(0, np.pi, end - k0))
    y = np.array(x[:end], dtype=float)
    return y * (g if y.ndim == 1 else g[:, None])


def finish_chain(x: np.ndarray, hp: float = 120, deharsh: float = 0, deharsh_f: float = 3200, sat: float = 0,
                 lp: float | None = None) -> np.ndarray:
    """L5: high-pass, de-harsh dip, low-pass, light tanh saturation."""
    y = dsp.butter(x, "high", hp, 4 if hp > 800 else 2)
    if deharsh > 0.05:
        y = dsp.peq(y, deharsh_f, -deharsh, 1.2)
    if lp is not None:
        y = dsp.butter(y, "low", lp, 2)
    if sat > 0.05:
        y = fx.saturate(y, drive_db=sat, mode="tanh", level="peak")
    return np.asarray(y, dtype=float)


def tiny_space(x: np.ndarray, r: np.random.Generator, wet: float, width: float = 0.0, size: float = 0.12,
               damp: float = 0.7) -> np.ndarray:
    """L6: a tiny dark early-reflection space on a mono signal; stereo out, same length."""
    st = np.column_stack([x, x])
    if wet <= 0.002:
        return st
    er = np.asarray(fx.early(x, r, size=size, taps=14, damp=damp, width=1.0), dtype=float)[: len(x)]
    er = fit(dsp.butter(er, "low", 7000, 1), len(x))
    mid = er.mean(axis=1, keepdims=True)
    er = mid + 4 * width * (er - mid)
    er = er * rms(x) * wet / (rms(er) + 1e-12)
    return st + er


def click_layers(p: Params, r: np.random.Generator, pitch_scale: float = 1.0, level: float = 1.0,
                 seconds: float = 0.16, send: bool = False):
    """L7: transient, tink partials, body and grit of one click; mono, onset at sample 0."""
    n = n_of(seconds)
    main = np.zeros(n)
    snd = np.zeros(n)
    pitch = p["pitch"] * pitch_scale * math.exp(0.01 * r.standard_normal())
    att = p["attack_ms"] / 1000
    hi = pitch * 2 ** (0.5 + 1.6 * p["bright"])
    tr = band_burst(p["snap_ms"] / 1000, r, pitch / 2 ** 0.6, hi, rise=att)
    add_at(main, tr, 0, 1 - 0.5 * p["tone"])
    add_at(snd, tr, 0, 1 - 0.5 * p["tone"])
    _tink(main, p, r, pitch, att, seconds)
    _body(main, p, r, pitch_scale, att, seconds)
    _grit(main, snd, p, r, pitch)
    g = level / (peak(main) + 1e-12)
    return (main * g, snd * g) if send else main * g


def _tink(main: np.ndarray, p: Params, r: np.random.Generator, pitch: float, att: float, seconds: float) -> None:
    k = int(p["partials"])
    inh = p["inharm"]
    ratios = (1.0, inh, inh ** 1.6)[:k]
    amps = (1.0, 0.55, 0.3)[:k]
    tk = partials(pitch, ratios, amps, p["tink_ms"] / 1000, seconds, r, glide=0.004, rise=max(0.0001, att))
    add_at(main, tk, 0, 1.4 * p["tone"])


def _body(main: np.ndarray, p: Params, r: np.random.Generator, pitch_scale: float, att: float,
          seconds: float) -> None:
    if p["body"] <= 0.004:
        return
    th = designed_thump(p["body_f"] * pitch_scale ** 0.5, seconds, max(0.0004, att), 0.0, p["body_ms"] / 1000,
                        drop=p["body_drop"], harm=0.15, r=r)
    add_at(main, th, 0, 1.5 * p["body"])


def _grit(main: np.ndarray, snd: np.ndarray, p: Params, r: np.random.Generator, pitch: float) -> None:
    if p["grit"] <= 0.005:
        return
    gs = p["grit_ms"] / 1000
    g = band_burst(gs, r, pitch * 0.5, min(16000.0, 2.5 * pitch), rise=0.0008, order=1)
    g = unit(g * np.sqrt(np.exp(-t_of(len(g)) / gs)))
    k = n_of(0.0004)
    add_at(main, g, k, 0.35 * p["grit"])
    add_at(snd, g, k, 0.35 * p["grit"])


def room_tail(st: np.ndarray, snd: np.ndarray, r: np.random.Generator, p: Params, size: float = 0.12,
              lowcut: float = 300.0) -> np.ndarray:
    """L8: a small dark room tail fed from the broadband send bus."""
    if p["verb"] <= 0.003:
        return st
    es = float(np.mean(np.square(snd)))
    g = math.sqrt(float(np.mean(np.square(st))) / (es + 1e-20)) if es > 0 else 0.0
    src = np.column_stack([snd, snd]) * g
    wet = fx.reverb(src, r, rt60=p["rt"], size=size, predelay=0.003, damp=0.75, wet=p["verb"], dry=0.0,
                    early_db=-12.0, width=min(1.0, 4 * p["width"]), stereo=True, lowcut=lowcut)
    wet = np.asarray(wet, dtype=float)
    n = max(len(st), len(wet))
    return fit(st, n) + fit(wet, n)


# ------------------------------------------------------- family boxes (L9)

def _box_spec(space: Params, key: str, v: Any) -> Any:
    if not isinstance(v, tuple):
        return [v]
    if len(v) == 3:
        return v
    full = space.get(key)
    is_int = isinstance(full, tuple) and len(full) == 3 and full[2] == "int"
    return (v[0], v[1], "int") if is_int else (v[0], v[1])


def narrow_space(space: Params, box: Params) -> Params:
    """L9 narrowed space: the full space with the box's fixed values and ranges laid over it."""
    out = dict(space)
    for key, v in box.items():
        out[key] = _box_spec(space, key, v)
    return out


def _centre(v: tuple) -> float:
    lo, hi = v[0], v[1]
    if len(v) == 3 and v[2] == "log":
        return math.sqrt(lo * hi)
    return (lo + hi) / 2


def box_centre(defaults: Params, box: Params) -> Params:
    """L9 family centre: the defaults with the box's fixed values and range centres."""
    out = dict(defaults)
    for key, v in box.items():
        if not isinstance(v, tuple):
            out[key] = v
            continue
        c = _centre(v)
        out[key] = int(round(c)) if isinstance(defaults.get(key), int) else c
    return out


def member_of(table: dict[str, Any], cat: str, name: str) -> Any:
    """One member of a category table; KeyError names the category and the member."""
    if cat not in table:
        raise KeyError(f"unknown category {cat!r}")
    if name not in table[cat]:
        raise KeyError(f"unknown member {name!r} of category {cat!r}")
    return table[cat][name]
