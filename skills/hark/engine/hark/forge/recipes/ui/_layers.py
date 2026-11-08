"""Parts that the UI recipes share: click layers, small rooms, family boxes and table readers.

Used by click, toggle, keypress, typing, pop and popin. The leading ``_`` in the
module name keeps the recipe registry from treating it as a recipe.
"""

from __future__ import annotations

import math
import os
from typing import Any

import numpy as np

from hark.forge.engine import dsp, fx

SR = 48000
Params = dict[str, Any]


# ------------------------------------------------------------ helpers

def n_of(seconds: float) -> int:
    """Number of samples in ``seconds``, never less than 1."""
    return max(1, int(round(seconds * SR)))


def t_of(n: int) -> np.ndarray:
    return np.arange(n) / SR


def peak(x: np.ndarray) -> float:
    return float(np.max(np.abs(x))) if len(x) else 0.0


def unit(x: np.ndarray) -> np.ndarray:
    """Scale to peak 1; a tiny offset keeps silence finite."""
    return x / (peak(x) + 1e-12)


def rms(x: np.ndarray) -> float:
    return float(np.sqrt(np.mean(np.square(x)))) if len(x) else 0.0


def merged(defaults: Params, p: Params | None) -> Params:
    """Copy of ``defaults`` with the values of ``p`` written over it."""
    out = dict(defaults)
    out.update(p or {})
    return out


def add_at(dst: np.ndarray, x: np.ndarray, start: int, gain: float = 1.0) -> np.ndarray:
    """Mix ``gain * x`` into ``dst`` at ``start``; whatever runs past the end is dropped."""
    if start >= len(dst) or start < 0:
        return dst
    span = min(len(x), len(dst) - start)
    dst[start:start + span] += gain * x[:span]
    return dst


def fit(x: np.ndarray, n: int) -> np.ndarray:
    """Trim ``x`` to ``n`` samples on axis 0, or pad it with zeros."""
    if len(x) >= n:
        return x[:n]
    pad = np.zeros((n - len(x),) + x.shape[1:])
    return np.concatenate([x, pad])


def onset_ramp(x: np.ndarray, rise: float) -> np.ndarray:
    """Fade in over ``rise`` seconds (2 samples at least) along a squared sine."""
    span = min(len(x), max(2, n_of(rise)))
    x[:span] *= np.sin(np.linspace(0, np.pi / 2, span)) ** 2
    return x


def db(v: float) -> float:
    return 10 ** (v / 20)


def ref_root() -> str:
    """Folder of the reference sounds: ``HARK_REFS`` or ``refs``, with forward slashes."""
    root = os.environ.get("HARK_REFS", "refs").replace("\\", "/")
    return root.rstrip("/") or root


# ----------------------------------------------------------- short wrappers

TWO_PI = 2 * np.pi


def decay(t: np.ndarray, tau: float) -> np.ndarray:
    """Exponential fall with time constant ``tau``."""
    return np.exp(-t / tau)


def osc(freq: float, t: np.ndarray, phase: float | None = None) -> np.ndarray:
    """A sine at ``freq``; the phase is added only when one is given."""
    arg = TWO_PI * freq * t
    return np.sin(arg if phase is None else arg + phase)


def lowpass(x: np.ndarray, f: float, order: int = 2) -> np.ndarray:
    return dsp.butter(x, "low", f, order)


def highpass(x: np.ndarray, f: float, order: int = 2) -> np.ndarray:
    return dsp.butter(x, "high", f, order)


def bandpass(x: np.ndarray, lo: float, hi: float, order: int = 2) -> np.ndarray:
    return dsp.butter(x, "band", (lo, hi), order)


# ------------------------------------------------------- parameter tables

def _scalar(word: str) -> Any:
    """Read one table cell: an int, else a float, else the bare word."""
    for kind in (int, float):
        try:
            return kind(word)
        except ValueError:
            pass
    return word


def space_table(text: str) -> Params:
    """Build a search space from rows of ``name lo hi [scale]``.

    A row whose second cell is ``|`` lists choices instead: ``name | a b c``.
    """
    out: Params = {}
    for row in text.strip().splitlines():
        name, *cells = row.split()
        if cells[0] == "|":
            out[name] = [_scalar(c) for c in cells[1:]]
        else:
            out[name] = tuple(_scalar(c) for c in cells)
    return out


def value_table(text: str) -> Params:
    """Build a parameter set from ``name value`` pairs, several pairs per row allowed."""
    words = text.split()
    return {words[i]: _scalar(words[i + 1]) for i in range(0, len(words), 2)}


# ------------------------------------------------------------ sound layers

def band_burst(seconds: float, r: np.random.Generator, f_lo: float, f_hi: float, rise: float = 0.0003,
               order: int = 2) -> np.ndarray:
    """Band-limited noise burst that dies along a squared exponential; peak 1."""
    size = n_of(max(6 * seconds, 0.004))
    low = max(40.0, f_lo)
    high = max(1.05 * low, min(21500.0, f_hi))
    noise = dsp.butter(r.standard_normal(size), "band", (low, high), order)
    noise = noise * np.exp(-t_of(size) / max(seconds, 5e-5)) ** 2
    return unit(onset_ramp(noise, rise))


def partials(f0: float, ratios, amps, tau: float, seconds: float, r: np.random.Generator, glide: float = 0.0,
             rise: float = 0.0002) -> np.ndarray:
    """Decaying partials at f0 times each ratio; higher ratios ring shorter. Peak 1."""
    freqs = [f0 * q for q in ratios]
    taus = [tau * q ** -0.6 for q in ratios]
    out = dsp.modal(freqs, taus, list(amps), seconds, r, jitter=0.004, glide=glide)
    return unit(onset_ramp(np.asarray(out, dtype=float), rise))


def designed_thump(f: float, seconds: float, rise: float, hold_s: float, rel_s: float, drop: float = 0.1,
                   harm: float = 0.1, r: np.random.Generator | None = None) -> np.ndarray:
    """Low sine hit that bends down a little, with a cosine rise, a hold and a release; peak 1."""
    size = n_of(seconds)
    tt = t_of(size)
    inst_hz = f * (1 + drop * np.exp(-tt / max(rise + 0.5 * hold_s, 0.002)))
    start = r.uniform(0, 2 * np.pi) if r is not None else 0.0
    phase = np.cumsum(2 * np.pi * inst_hz / SR) + start
    out = np.sin(phase) + harm * np.sin(2 * phase + 0.3)
    rise_n = max(2, n_of(rise))
    shape = np.ones(size)
    span = min(size, rise_n)
    shape[:span] = 0.5 - 0.5 * np.cos(np.pi * np.arange(span) / rise_n)
    hold_at = rise_n + n_of(hold_s)
    if hold_at < size:
        shape[hold_at:] = np.exp(-(tt[hold_at:] - tt[hold_at]) / max(rel_s, 0.001))
    return unit(out * shape)


def end_gate(x: np.ndarray, length: float, release: float, start: float = 0.0) -> np.ndarray:
    """Close the sound with a cosine fade that reaches zero at start + length, and cut it there."""
    size = len(x)
    end = min(size, n_of(start + length))
    rl = max(8, min(end - 1, n_of(release)))
    gain = np.ones(end)
    k0 = max(0, end - rl)
    gain[k0:] = 0.5 + 0.5 * np.cos(np.linspace(0, np.pi, end - k0))
    out = np.array(x[:end], dtype=float)
    return out * (gain if out.ndim == 1 else gain[:, None])


def _tone_stages(out: np.ndarray, deharsh: float, deharsh_f: float, lp: float | None) -> np.ndarray:
    """Optional presence dip and optional low-pass."""
    if deharsh > 0.05:
        out = dsp.peq(out, deharsh_f, -deharsh, 1.2)
    if lp is not None:
        out = dsp.butter(out, "low", lp, 2)
    return out


def finish_chain(x: np.ndarray, hp: float = 120, deharsh: float = 0, deharsh_f: float = 3200, sat: float = 0,
                 lp: float | None = None) -> np.ndarray:
    """Output chain: high-pass, presence dip, low-pass and a light tanh drive."""
    out = dsp.butter(x, "high", hp, 4 if hp > 800 else 2)
    out = _tone_stages(out, deharsh, deharsh_f, lp)
    if sat > 0.05:
        out = fx.saturate(out, drive_db=sat, mode="tanh", level="peak")
    return np.asarray(out, dtype=float)


def finish_pair(main: np.ndarray, snd: np.ndarray, p: dict) -> tuple[np.ndarray, np.ndarray]:
    """Run the output chain on the dry path and on the room send (send: high-pass of 300 Hz or more, no drive)."""
    main = finish_chain(main, p["hp"], p["deharsh"], p["deharsh_f"], p["sat"], p["lp"])
    snd = finish_chain(snd, max(p["hp"], 300), p["deharsh"], p["deharsh_f"], 0, p["lp"])
    return main, snd


def tiny_space(x: np.ndarray, r: np.random.Generator, wet: float, width: float = 0.0, size: float = 0.12,
               damp: float = 0.7) -> np.ndarray:
    """Small dark space of early reflections around a mono sound; returns stereo of the same length."""
    pair = np.column_stack([x, x])
    if wet <= 0.002:
        return pair
    early = np.asarray(fx.early(x, r, size=size, taps=14, damp=damp, width=1.0), dtype=float)[: len(x)]
    early = fit(dsp.butter(early, "low", 7000, 1), len(x))
    centre = early.mean(axis=1, keepdims=True)
    early = centre + 4 * width * (early - centre)
    early = early * rms(x) * wet / (rms(early) + 1e-12)
    return pair + early


def click_layers(p: Params, r: np.random.Generator, pitch_scale: float = 1.0, level: float = 1.0,
                 seconds: float = 0.16, send: bool = False):
    """One mono click from sample 0: snap, metallic partials, body and grit."""
    size = n_of(seconds)
    main = np.zeros(size)
    snd = np.zeros(size)
    pitch = p["pitch"] * pitch_scale * math.exp(0.01 * r.standard_normal())
    att = p["attack_ms"] / 1000
    high = pitch * 2 ** (0.5 + 1.6 * p["bright"])
    snap = band_burst(p["snap_ms"] / 1000, r, pitch / 2 ** 0.6, high, rise=att)
    add_at(main, snap, 0, 1 - 0.5 * p["tone"])
    add_at(snd, snap, 0, 1 - 0.5 * p["tone"])
    _tink(main, p, r, pitch, att, seconds)
    _body(main, p, r, pitch_scale, att, seconds)
    _grit(main, snd, p, r, pitch)
    gain = level / (peak(main) + 1e-12)
    return (main * gain, snd * gain) if send else main * gain


def _tink(main: np.ndarray, cfg: Params, gen: np.random.Generator, pitch: float, att: float, seconds: float) -> None:
    span = int(cfg["partials"])
    inh = cfg["inharm"]
    ratios = (1.0, inh, inh ** 1.6)[:span]
    amps = (1.0, 0.55, 0.3)[:span]
    tink = partials(pitch, ratios, amps, cfg["tink_ms"] / 1000, seconds, gen, glide=0.004, rise=max(0.0001, att))
    add_at(main, tink, 0, 1.4 * cfg["tone"])


def _body(main: np.ndarray, cfg: Params, gen: np.random.Generator, pitch_scale: float, att: float,
          seconds: float) -> None:
    if cfg["body"] <= 0.004:
        return
    hold_at = designed_thump(cfg["body_f"] * pitch_scale ** 0.5, seconds, max(0.0004, att), 0.0, cfg["body_ms"] / 1000,
                        drop=cfg["body_drop"], harm=0.15, r=gen)
    add_at(main, hold_at, 0, 1.5 * cfg["body"])


def _grit(main: np.ndarray, snd: np.ndarray, cfg: Params, gen: np.random.Generator, pitch: float) -> None:
    if cfg["grit"] <= 0.005:
        return
    grit_s = cfg["grit_ms"] / 1000
    gain = band_burst(grit_s, gen, pitch * 0.5, min(16000.0, 2.5 * pitch), rise=0.0008, order=1)
    gain = unit(gain * np.sqrt(np.exp(-t_of(len(gain)) / grit_s)))
    span = n_of(0.0004)
    add_at(main, gain, span, 0.35 * cfg["grit"])
    add_at(snd, gain, span, 0.35 * cfg["grit"])


def room_tail(st: np.ndarray, snd: np.ndarray, r: np.random.Generator, p: Params, size: float = 0.12,
              lowcut: float = 300.0) -> np.ndarray:
    """Short dark room tail driven by the full-band send."""
    if p["verb"] <= 0.003:
        return st
    send_pow = float(np.mean(np.square(snd)))
    gain = math.sqrt(float(np.mean(np.square(st))) / (send_pow + 1e-20)) if send_pow > 0 else 0.0
    feed = np.column_stack([snd, snd]) * gain
    wet = fx.reverb(feed, r, rt60=p["rt"], size=size, predelay=0.003, damp=0.75, wet=p["verb"], dry=0.0,
                    early_db=-12.0, width=min(1.0, 4 * p["width"]), stereo=True, lowcut=lowcut)
    wet = np.asarray(wet, dtype=float)
    size = max(len(st), len(wet))
    return fit(st, size) + fit(wet, size)


# ------------------------------------------------------------ family boxes

def _box_spec(space: Params, key: str, val: Any) -> Any:
    if not isinstance(val, tuple):
        return [val]
    if len(val) == 3:
        return val
    full = space.get(key)
    is_int = isinstance(full, tuple) and len(full) == 3 and full[2] == "int"
    return (val[0], val[1], "int") if is_int else (val[0], val[1])


def narrow_space(space: Params, box: Params) -> Params:
    """Search space of a family: the full space with the fixed values and ranges of the box on top."""
    out = dict(space)
    for key, val in box.items():
        out[key] = _box_spec(space, key, val)
    return out


def _centre(val: tuple) -> float:
    low, high = val[0], val[1]
    if len(val) == 3 and val[2] == "log":
        return math.sqrt(low * high)
    return (low + high) / 2


def box_centre(defaults: Params, box: Params) -> Params:
    """Centre of a family: the defaults with each box value, or the middle of each box range."""
    out = dict(defaults)
    for key, val in box.items():
        if not isinstance(val, tuple):
            out[key] = val
            continue
        c = _centre(val)
        out[key] = int(round(c)) if isinstance(defaults.get(key), int) else c
    return out


def member_of(table: dict[str, Any], cat: str, name: str) -> Any:
    """Look up one member of a category; the KeyError names what is missing."""
    if cat not in table:
        raise KeyError(f"unknown category {cat!r}")
    if name not in table[cat]:
        raise KeyError(f"unknown member {name!r} of category {cat!r}")
    return table[cat][name]
