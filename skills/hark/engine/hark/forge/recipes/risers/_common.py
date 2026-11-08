"""Small helpers shared by the riser-folder recipes (not a recipe: the name starts with ``_``)."""

from __future__ import annotations

import types
from typing import Callable

import numpy as np

from hark.forge.engine import dsp

SR = 48000


def n_of(seconds: float) -> int:
    """Samples of a duration: at least 1."""
    return max(1, int(round(float(seconds) * SR)))


def t_of(n: int) -> np.ndarray:
    return np.arange(int(n)) / SR


def unit_peak(x: np.ndarray) -> np.ndarray:
    return x / (np.max(np.abs(x)) + 1e-12) if x.size else x


def smooth01(x: np.ndarray) -> np.ndarray:
    """Cubic smooth step x^2 (3 - 2x) on [0, 1]."""
    return x * x * (3.0 - 2.0 * x)


def per_channel(fn: Callable[[np.ndarray], np.ndarray], x: np.ndarray) -> np.ndarray:
    """Apply a mono function to each channel of a stereo buffer."""
    if x.ndim == 1:
        return fn(x)
    return np.column_stack([fn(x[:, c]) for c in range(x.shape[1])])


def svf(x: np.ndarray, cutoff, q, mode: str) -> np.ndarray:
    """The forge-core state-variable filter, on mono or on each stereo channel."""
    return per_channel(lambda ch: dsp.svf(ch, cutoff, q, mode), x)


def bw(x: np.ndarray, kind: str, f, order: int) -> np.ndarray:
    return dsp.butter(x, kind, f, order)


def stereo2(x: np.ndarray) -> np.ndarray:
    return x if x.ndim == 2 else np.column_stack([x, x])


def mono(x: np.ndarray) -> np.ndarray:
    return x.mean(axis=1) if x.ndim == 2 else x


def fit(x: np.ndarray, n: int) -> np.ndarray:
    """Cut or zero-pad a buffer to n samples along time."""
    if len(x) >= n:
        return x[:n]
    pad = [(0, n - len(x))] + [(0, 0)] * (x.ndim - 1)
    return np.pad(x, pad)


def delayed(x: np.ndarray, d: int, n: int) -> np.ndarray:
    """x delayed by d samples, in an n-sample buffer."""
    out = np.zeros((n,) + x.shape[1:])
    if d < n:
        m = min(len(x), n - d)
        out[d:d + m] = x[:m]
    return out


def poisson_times(rng: np.random.Generator, t_end: float, rate_at: Callable[[float], float]) -> list[float]:
    """Event times of a Poisson process with a time-varying rate, from 0 up to t_end."""
    times: list[float] = []
    tg = 0.0
    while True:
        tg += rng.exponential(1.0 / max(rate_at(tg), 1e-9))
        if tg >= t_end:
            return times
        times.append(tg)


def hann(m: int) -> np.ndarray:
    """Symmetric Hann window of m points."""
    if m < 2:
        return np.ones(max(m, 0))
    return 0.5 - 0.5 * np.cos(2 * np.pi * np.arange(m) / (m - 1))


def add_at(dst: np.ndarray, x: np.ndarray, i: int) -> None:
    """Add x into dst at sample i, cut at the end of dst."""
    if i >= len(dst) or i < 0:
        return
    m = min(len(x), len(dst) - i)
    dst[i:i + m] += x[:m]


def box(n: int, edge: float = 0.0004) -> np.ndarray:
    """P26 box window: ones with raised-cosine edges."""
    w = np.ones(n)
    k = min(n // 2, max(2, n_of(edge)))
    if k > 1:
        r = 0.5 - 0.5 * np.cos(np.pi * np.arange(k) / (k - 1))
        w[:k] *= r
        w[n - k:] *= r[::-1]
    return w


def crush(x: np.ndarray, bits: float, rate: float) -> np.ndarray:
    """P25 sample-and-hold to rate, then quantise to bits."""
    n = len(x)
    i = np.arange(n)
    idx = np.minimum((np.floor(np.floor(i * rate / SR) * SR / rate)).astype(int), n - 1)
    y = x[idx]
    q = 2.0 ** (bits - 1)
    return np.round(y * q) / q


def tile(x: np.ndarray, m: int) -> np.ndarray:
    """Repeat x to m samples (or take its first m)."""
    if m <= len(x):
        return x[:m].copy()
    reps = int(np.ceil(m / max(1, len(x))))
    return np.tile(x, reps)[:m]


def speed_read(x: np.ndarray, factor: float) -> np.ndarray:
    """Resample by index i x factor (truncated, clamped to the last sample)."""
    if factor == 1.0:
        return x
    idx = np.minimum((np.arange(len(x)) * factor).astype(int), len(x) - 1)
    return x[idx]


def merged(defaults: dict, p: dict) -> dict:
    out = dict(defaults)
    out.update(p or {})
    return out


def headroom(x: np.ndarray) -> np.ndarray:
    """Scale a render whose peak would clip (0.999 or more) down to a peak of 0.89; quieter renders pass unchanged."""
    pk = float(np.max(np.abs(x))) if x.size else 0.0
    return x * (0.89 / pk) if pk >= 0.999 else x


def member_recipe(module: str, families: dict, space: dict, targets: dict, alts: list, refset: str,
                  render: Callable, cat: str, preset) -> types.SimpleNamespace:
    """A recipe object around one family member, chosen by list index or by name (recipes-motion 8.2)."""
    members = families[cat]
    name, params = members[preset] if isinstance(preset, int) else (preset, dict(members)[preset])
    return types.SimpleNamespace(MODULE=module, SPACE=dict(space), DEFAULTS=dict(params), render=render,
                                 TARGET=targets[cat], ALTS=list(alts), REFSET=refset, PRESET=name)
