"""Small signal helpers that the motion recipes share.

This module is a helper (its name starts with ``_``), so the recipe registry does
not list it. The helpers follow the recipe-level primitives of
docs/specs/hark/recipes-motion.md (P5 to P8, P12, P26).
"""

from __future__ import annotations

import numpy as np
from scipy import fft as sfft
from scipy import signal

from hark.forge.engine import dsp

SR = 48000
EPS = 1e-12


def samples(seconds: float) -> int:
    """P1: max(1, round(seconds x SR))."""
    return max(1, int(round(float(seconds) * SR)))


def times(n: int) -> np.ndarray:
    """The time of each of n samples, in seconds."""
    return np.arange(n) / SR


def mono(x: np.ndarray) -> np.ndarray:
    """The channel mean of a stereo buffer; a mono buffer unchanged."""
    x = np.asarray(x, dtype=float)
    return x.mean(axis=1) if x.ndim == 2 else x


def rms(x: np.ndarray) -> float:
    """Root mean square of all samples."""
    x = np.asarray(x, dtype=float)
    return float(np.sqrt(np.mean(x * x))) if x.size else 0.0


def unit_peak(x: np.ndarray) -> np.ndarray:
    """x divided by (max |x| + 1e-12)."""
    return x / (np.max(np.abs(x)) + EPS) if x.size else x


def hann_smooth(x: np.ndarray, seconds: float) -> np.ndarray:
    """P5: zero-phase smoothing with a normalised symmetric Hann window."""
    k = max(3, int(seconds * SR))
    if k % 2 == 0:
        k += 1
    w = 0.5 - 0.5 * np.cos(2 * np.pi * np.arange(k) / (k - 1))
    w /= w.sum()
    half = k // 2
    padded = np.concatenate([np.full(half, x[0]), x, np.full(half, x[-1])])
    return np.convolve(padded, w, mode="valid")[: len(x)]


def control(n: int, fc: float, rng: np.random.Generator) -> np.ndarray:
    """P6: white noise low-passed at fc (order 2), scaled to unit standard deviation."""
    y = dsp.butter(rng.standard_normal(n), "low", fc, 2)
    return y / (np.std(y) + EPS)


def colored(n: int, rng: np.random.Generator, power: float) -> np.ndarray:
    """P7: noise shaped by f^-power in the FFT (0.5 pink, 1 brown), unit std."""
    m = sfft.next_fast_len(n + 256)
    spec = np.fft.rfft(rng.standard_normal(m))
    f = np.maximum(np.fft.rfftfreq(m, 1.0 / SR), 15.0)
    y = np.fft.irfft(spec / f**power, m)[128 : 128 + n]
    return y / (np.std(y) + EPS)


def block_peak_rms(x: np.ndarray, block: int = 480) -> float:
    """P8, whoosh variant: the peak 10 ms block RMS (+1e-12); full RMS when no whole block."""
    y = mono(x)
    k = len(y) // block
    if k < 1:
        return rms(y) + EPS
    blocks = y[: k * block].reshape(k, block)
    return float(np.sqrt(np.mean(blocks * blocks, axis=1)).max()) + EPS


def shelf(x: np.ndarray, f0: float, gain_db: float, high: bool = True, s: float = 0.8) -> np.ndarray:
    """P12: an RBJ cookbook shelving biquad, applied along axis 0."""
    a_ = 10 ** (gain_db / 40.0)
    w0 = 2 * np.pi * min(f0, 0.45 * SR) / SR
    c = np.cos(w0)
    alpha = np.sin(w0) / 2 * np.sqrt((a_ + 1 / a_) * (1 / s - 1) + 2)
    q = 2 * np.sqrt(a_) * alpha
    b, a = _shelf_ba(a_, c, q, 1.0 if high else -1.0)
    return signal.lfilter(b / a[0], a / a[0], x, axis=0)


def _shelf_ba(a_: float, c: float, q: float, sign: float) -> tuple[np.ndarray, np.ndarray]:
    """The P12 numerator and denominator; sign +1 for the high shelf, -1 for the low shelf."""
    plus, minus = (a_ + 1) + sign * (a_ - 1) * c, (a_ + 1) - sign * (a_ - 1) * c
    mid_b = -2 * sign * a_ * ((a_ - 1) + sign * (a_ + 1) * c)
    mid_a = 2 * sign * ((a_ - 1) - sign * (a_ + 1) * c)
    return np.array([a_ * (plus + q), mid_b, a_ * (plus - q)]), np.array([minus + q, mid_a, minus - q])


def raised_cos_in(k: int) -> np.ndarray:
    """0.5 - 0.5 cos(pi i / (k - 1)) for i = 0 .. k - 1 (ones for k < 2)."""
    if k < 2:
        return np.ones(max(k, 0))
    return 0.5 - 0.5 * np.cos(np.pi * np.arange(k) / (k - 1))


def sin2_in(k: int) -> np.ndarray:
    """sin^2 of a phase from 0 to pi/2 over k samples (ones for k < 2)."""
    if k < 2:
        return np.ones(max(k, 0))
    return np.sin(0.5 * np.pi * np.arange(k) / (k - 1)) ** 2


def pan_gains(pos: float) -> tuple[float, float]:
    """Constant-power pan gains (sqrt 2 cos, sqrt 2 sin) of a position in [-1, 1]."""
    theta = (float(np.clip(pos, -1.0, 1.0)) + 1.0) * np.pi / 4
    return float(np.sqrt(2) * np.cos(theta)), float(np.sqrt(2) * np.sin(theta))


def add_at(dst: np.ndarray, x: np.ndarray, start: int) -> None:
    """Add x into dst from index start, in place; the part past the end is dropped."""
    if start >= len(dst) or start < 0:
        return
    m = min(len(x), len(dst) - start)
    dst[start : start + m] += x[:m]


def stereo(x: np.ndarray) -> np.ndarray:
    """A mono buffer duplicated to 2 channels; a stereo buffer unchanged."""
    x = np.asarray(x, dtype=float)
    return x if x.ndim == 2 else np.column_stack([x, x])


def below_full_scale(x: np.ndarray) -> np.ndarray:
    """x scaled to peak 0.89 when its peak exceeds 1.0, else x unchanged.

    The spec leaves the raw level open (the forge wrapper normalises). This guard keeps
    a raw render under 0 dBFS. It is linear, so it changes no shape and no ratio.
    """
    peak = float(np.max(np.abs(x))) if x.size else 0.0
    return x * (0.89 / peak) if peak > 1.0 else x
