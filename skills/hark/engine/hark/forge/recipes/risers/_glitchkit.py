"""Building blocks shared by glitch and burstglitch (a helper module: the name starts with ``_``)."""

from __future__ import annotations

from typing import Callable

import numpy as np

from hark.forge.recipes.risers._common import SR, box, hann, n_of, poisson_times, speed_read, t_of, tile


def pattern_rng(pattern: float, offset: int) -> np.random.Generator:
    """The layout generator: seeded from the integer pattern key, never from the render generator."""
    return np.random.default_rng((int(round(pattern)) * 7919 + offset) & 0xFFFFFFFF)


def grid(t_st: float, dur: float, g0: float, g1: float, accel: float) -> list[tuple[float, float]]:
    """Accelerating slots (start, length) from t_st to the cut at dur (10.4 step 3)."""
    span = max(0.001, dur - t_st)
    slots: list[tuple[float, float]] = []
    t = t_st
    while True:
        u = min(1.0, (t - t_st) / span) ** accel
        d = g0 + (g1 - g0) * u
        if t + d > dur + 0.25 * d:
            break
        slots.append((t, d))
        t += d
    return slots or [(t_st, span)]


def start_of(t: float) -> int:
    return int(round(t * SR))


def peak(x: np.ndarray) -> float:
    return float(np.max(np.abs(x))) if x.size else 0.0


def held_track(n: int, prng: np.random.Generator, value: Callable[[], float], lo_s: float, hi_s: float) -> np.ndarray:
    """A pitch track that holds a drawn value for a drawn time, segment after segment, over n samples."""
    out = np.empty(n)
    i = 0
    while i < n:
        m = max(1, n_of(prng.uniform(lo_s, hi_s)))
        out[i:i + m] = value()
        i += m
    return out


def stepped_track(n: int, base: float, steps: list[int]) -> np.ndarray:
    """Pitch steps base x 2^(k / 6) splitting n samples into equal parts."""
    edges = np.linspace(0, n, len(steps) + 1).astype(int)
    out = np.empty(n)
    for k, a, b in zip(steps, edges[:-1], edges[1:]):
        out[a:b] = base * 2.0 ** (k / 6)
    return out


def read_slice(buf: np.ndarray, reverse: bool, speed: float) -> np.ndarray:
    x = buf[::-1] if reverse else buf
    return speed_read(x, speed)


def body_segment(buf: np.ndarray, m: int) -> np.ndarray:
    """The first m samples of a buffer, tiled when m is longer."""
    return tile(buf, m)


def decay_env(m: int, tau: float, edge: float) -> np.ndarray:
    """A box window with raised-cosine edges times an exponential decay."""
    return box(m, edge) * np.exp(-t_of(m) / max(tau, 1e-6))


def ramp_in(env: np.ndarray, seconds: float = 0.0004) -> np.ndarray:
    k = min(len(env), n_of(seconds))
    if k > 1:
        env[:k] *= np.linspace(0.0, 1.0, k)
    return env


def last_env(hold_n: int, hold_tau: float, tail_n: int, end_tail: float) -> np.ndarray:
    """The final burst: an exponential hold, then a fast tail past the cut; the start ramps in over 0.4 ms."""
    hold = np.exp(-t_of(hold_n) / max(hold_tau, 1e-6))
    end = hold[-1] if hold_n else 1.0
    tail = end * np.exp(-t_of(tail_n) / (end_tail / 1.3))
    return ramp_in(np.concatenate([hold, tail]))


def add_clip(dst: np.ndarray, x: np.ndarray, i: int) -> None:
    """Add x into dst from sample i, cut at the end of dst."""
    if 0 <= i < len(dst):
        m = min(len(x), len(dst) - i)
        dst[i:i + m] += x[:m]


def grain_train(n: int, t_end: float, rate: Callable[[float], float], rng: np.random.Generator,
                grain: Callable[[float], np.ndarray]) -> np.ndarray:
    """Poisson grains from 0 to t_end; grain(u) returns one grain at progress u = t / t_end."""
    y = np.zeros(n)
    for tg in poisson_times(rng, t_end, lambda tt: rate(min(1.0, tt / t_end))):
        add_clip(y, grain(tg / t_end), start_of(tg))
    return y


def hann_grain(rng: np.random.Generator, lo_s: float = 0.0015, hi_s: float = 0.006) -> np.ndarray:
    m = max(2, n_of(rng.uniform(lo_s, hi_s)))
    return rng.standard_normal(m) * hann(m)


def accel_rate(base: float) -> Callable[[float], float]:
    """Crackle rate that grows over the lead-in: base x (0.25 + 0.75 u^1.5)."""
    return lambda u: base * (0.25 + 0.75 * u ** 1.5)
