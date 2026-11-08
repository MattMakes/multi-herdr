"""Signal helpers shared by the timeline modules: log-mel, robust z, runs, filters, envelopes."""

from __future__ import annotations

import numpy as np
from scipy import ndimage, signal

SR = 48000
_BLOCK = 4096


def mono_of(x: np.ndarray) -> np.ndarray:
    """Plain channel mean as float64 (the core mono fold)."""
    a = np.asarray(x, dtype=np.float64)
    return a.mean(axis=1) if a.ndim == 2 else a


def mel_bank(n_fft: int, n_mels: int, fmin: float, fmax: float, sr: int = SR) -> np.ndarray:
    """Triangular HTK mel filters, peak 1, no area normalisation. Shape (n_mels, n_fft//2 + 1)."""
    lo, hi = 2595 * np.log10(1 + fmin / 700), 2595 * np.log10(1 + fmax / 700)
    edges = 700 * (10 ** (np.linspace(lo, hi, n_mels + 2) / 2595) - 1)
    f = np.arange(n_fft // 2 + 1) * sr / n_fft
    bank = np.zeros((n_mels, f.size))
    for k in range(n_mels):
        a, b, c = edges[k], edges[k + 1], edges[k + 2]
        up = (f - a) / max(b - a, 1e-12)
        down = (c - f) / max(c - b, 1e-12)
        bank[k] = np.clip(np.minimum(up, down), 0, None)
    return bank


def _padded(m: np.ndarray, n_fft: int) -> np.ndarray:
    half = n_fft // 2
    mode = "reflect" if m.size > n_fft else "constant"
    return np.pad(m, half, mode=mode)


def frame_count(n: int, hop: int) -> int:
    return 1 + n // hop


def power_frames(m: np.ndarray, n_fft: int, hop: int):
    """Yield (first frame index, power spectra block) for centred Hann frames."""
    y = _padded(np.asarray(m, dtype=np.float64), n_fft)
    n_frames = 1 + max(0, (y.size - n_fft)) // hop
    win = np.hanning(n_fft + 1)[:-1] if n_fft > 1 else np.ones(1)
    for i0 in range(0, n_frames, _BLOCK):
        i1 = min(n_frames, i0 + _BLOCK)
        idx = (np.arange(i0, i1) * hop)[:, None] + np.arange(n_fft)[None, :]
        spec = np.fft.rfft(y[idx] * win, axis=1)
        yield i0, (spec.real ** 2 + spec.imag ** 2)


def logmel(m: np.ndarray, n_fft: int, hop: int, n_mels: int, fmin: float, fmax: float) -> np.ndarray:
    """Log-mel spectrogram in dB, shape (n_mels, frames)."""
    bank = mel_bank(n_fft, n_mels, fmin, fmax)
    blocks = [bank @ p.T for _, p in power_frames(m, n_fft, hop)]
    out = np.concatenate(blocks, axis=1) if blocks else np.zeros((n_mels, 0))
    return 10 * np.log10(out + 1e-12)


def robust_z(v: np.ndarray, size: int) -> np.ndarray:
    v = np.asarray(v, dtype=np.float64)
    if v.size == 0:
        return v.copy()
    med = ndimage.median_filter(v, size=size, mode="nearest")
    mad = ndimage.median_filter(np.abs(v - med), size=size, mode="nearest") * 1.4826
    pos = v[v > 0]
    floor = 0.02 * np.percentile(pos, 99) if pos.size else 1e-6
    return (v - med) / (mad + floor + 1e-9)


def runs(mask: np.ndarray, min_len: int) -> list[tuple[int, int]]:
    m = np.concatenate([[False], np.asarray(mask, dtype=bool), [False]])
    d = np.diff(m.astype(np.int8))
    starts, ends = np.flatnonzero(d == 1), np.flatnonzero(d == -1)
    return [(int(s), int(e)) for s, e in zip(starts, ends) if e - s >= min_len]


def band_filter(m: np.ndarray, kind: str, freq: float) -> np.ndarray:
    """4th-order Butterworth (sos, causal) with a mirrored lead-in that is dropped afterwards."""
    sos = signal.butter(4, freq, btype=kind, fs=SR, output="sos")
    p = min(m.size - 1, 4800)
    lead = m[p:0:-1] if p > 0 else m[:0]
    y = signal.sosfilt(sos, np.concatenate([lead, m]))
    return y[lead.size:]


def ms_env(a: np.ndarray) -> np.ndarray:
    """Max-abs per millisecond (48 samples) plus 1e-7. Accepts (n,) or (n, k)."""
    a = np.abs(np.asarray(a, dtype=np.float64))
    if a.ndim == 2:
        a = a.max(axis=1)
    n = int(np.ceil(a.size / 48))
    pad = np.zeros(n * 48)
    pad[: a.size] = a
    return pad.reshape(n, 48).max(axis=1) + 1e-7


def db(v, floor: float = 1e-12):
    return 20 * np.log10(np.maximum(np.abs(v), floor))


def ascii_safe(text: str) -> str:
    """Non-ASCII characters become Python backslash escapes."""
    return text.encode("ascii", "backslashreplace").decode("ascii")


def nearest(sorted_times: np.ndarray, t: float) -> int | None:
    """Index of the time nearest to t among the insertion neighbours, or None for an empty array."""
    if sorted_times.size == 0:
        return None
    k = int(np.searchsorted(sorted_times, t))
    best = None
    for i in (k - 1, k, k + 1):
        if 0 <= i < sorted_times.size and (best is None or abs(sorted_times[i] - t) < abs(sorted_times[best] - t)):
            best = i
    return best


class TimelineError(ValueError):
    """An input problem of the timeline unit (bad cue, bad detail, bad window, missing stream)."""
