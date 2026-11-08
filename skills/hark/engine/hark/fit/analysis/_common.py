"""Helpers shared by the fit analysis modules: input loading, simplifiers, JSON."""

from __future__ import annotations

import json
import os
from typing import Any

import numpy as np

SR = 48000


def as_stereo(x: Any) -> np.ndarray:
    """Any input as an (n, 2) float64 array: mono duplicated, extra columns dropped."""
    if isinstance(x, (str, os.PathLike)):
        x = load_audio(str(x))
    a = np.asarray(x, dtype=float)
    if a.ndim == 1:
        return np.column_stack([a, a])
    if a.shape[1] == 1:
        return np.column_stack([a[:, 0], a[:, 0]])
    return a[:, :2].copy()


def load_audio(path: str, sr: int = SR) -> np.ndarray:
    """Decode a file at 48 kHz through the core loader; stereo stays (n, 2)."""
    from hark.core.audio import load
    return np.asarray(load(path, sr=sr, mono=False), dtype=float)


def resample(x: np.ndarray, sr: int) -> np.ndarray:
    """Polyphase resampling to 48 kHz along axis 0."""
    if sr == SR:
        return np.asarray(x, dtype=float)
    from math import gcd

    from scipy.signal import resample_poly
    g = gcd(int(sr), SR)
    return resample_poly(np.asarray(x, dtype=float), SR // g, int(sr) // g, axis=0)


def is_mono_pair(x2: np.ndarray) -> bool:
    """True when both channels of an (n, 2) array are equal."""
    return bool(np.allclose(x2[:, 0], x2[:, 1]))


def source_name(x: Any) -> str | None:
    if isinstance(x, (str, os.PathLike)):
        return os.path.basename(str(x))
    return None


def db(p: Any, floor: float = 1e-30) -> np.ndarray:
    return 10.0 * np.log10(np.maximum(p, floor))


def hann_norm(n: int) -> np.ndarray:
    """A normalised Hann window of n points without its zero ends."""
    n = max(int(n), 1)
    w = np.hanning(n + 2)[1:-1]
    return w / np.sum(w)


def smooth(x: np.ndarray, n: int, axis: int = -1) -> np.ndarray:
    """Convolve with a normalised Hann of n points (same length)."""
    from scipy.ndimage import convolve1d
    if n <= 1:
        return np.asarray(x, dtype=float)
    return convolve1d(np.asarray(x, dtype=float), hann_norm(n), axis=axis, mode="nearest")


def rdp(t: np.ndarray, v: np.ndarray, tol: Any, max_pts: int | None = None,
        grow: float = 1.4) -> list[int]:
    """Ramer-Douglas-Peucker on the value axis; ``tol`` scalar or per point."""
    t = np.asarray(t, dtype=float)
    v = np.asarray(v, dtype=float)
    tol = np.broadcast_to(np.asarray(tol, dtype=float), v.shape).copy()
    if len(t) <= 2:
        return list(range(len(t)))
    while True:
        keep = _rdp_pass(t, v[None, :], tol[None, :])
        if max_pts is None or len(keep) <= max_pts:
            return keep
        tol = tol * grow


def rdp_rows(t: np.ndarray, rows: np.ndarray, tol: float, max_pts: int | None = None,
             grow: float = 1.3) -> list[int]:
    """Multi-row simplifier: one time grid, the largest error over all rows."""
    t = np.asarray(t, dtype=float)
    rows = np.atleast_2d(np.asarray(rows, dtype=float))
    if len(t) <= 2:
        return list(range(len(t)))
    tol = float(tol)
    while True:
        keep = _rdp_pass(t, rows, np.full(rows.shape, tol))
        if max_pts is None or len(keep) <= max_pts:
            return keep
        tol *= grow


def _rdp_pass(t: np.ndarray, rows: np.ndarray, tol: np.ndarray) -> list[int]:
    keep = {0, len(t) - 1}
    stack = [(0, len(t) - 1)]
    while stack:
        i, j = stack.pop()
        if j - i < 2:
            continue
        k = _worst(t, rows, tol, i, j)
        if k is not None:
            keep.add(k)
            stack.append((i, k))
            stack.append((k, j))
    return sorted(keep)


def _worst(t, rows, tol, i, j):
    seg = slice(i + 1, j)
    span = t[j] - t[i]
    frac = (t[seg] - t[i]) / span if span > 0 else np.zeros(j - i - 1)
    line = rows[:, i:i + 1] + (rows[:, j:j + 1] - rows[:, i:i + 1]) * frac
    err = np.max(np.abs(rows[:, seg] - line) / np.maximum(tol[:, seg], 1e-12), axis=0)
    k = int(np.argmax(err))
    return i + 1 + k if err[k] > 1.0 else None


def r6(x: float, d: int) -> float:
    return float(round(float(x), d))


def sig(x: float, n: int = 5) -> float:
    """Round to n significant digits."""
    x = float(x)
    if x == 0 or not np.isfinite(x):
        return 0.0
    return float(f"{x:.{n - 1}e}")


def save_json(model: dict, path: str) -> None:
    d = os.path.dirname(os.path.abspath(path))
    os.makedirs(d, exist_ok=True)
    with open(path, "w", encoding="utf-8") as fh:
        json.dump(model, fh, separators=(",", ":"))


def load_json(path: str) -> dict:
    with open(path, encoding="utf-8") as fh:
        return json.load(fh)


def check_kind(model: Any, kind: str, version: int = 1) -> None:
    """ValueError unless ``model`` is a dict of ``kind`` with a supported version."""
    if not isinstance(model, dict):
        raise ValueError(f"not a {kind} model: {type(model).__name__}")
    if model.get("kind") != kind:
        raise ValueError(f"not a {kind} model: kind {model.get('kind')!r}")
    if int(model.get("version", 1)) > version:
        raise ValueError(f"{kind} model version {model.get('version')} is newer than {version}")


def threads() -> int:
    """FFT worker count from HARK_THREADS (default 2)."""
    try:
        v = int(os.environ.get("HARK_THREADS", "2"))
    except ValueError:
        return 2
    return v if v > 0 else 2


def mel_filters(nfft: int, n_mels: int, sr: int = SR, fmin: float = 30.0,
                fmax: float = 20000.0) -> np.ndarray:
    """Triangular mel filters (HTK mel scale); empty filters removed."""
    def mel(f):
        return 2595.0 * np.log10(1.0 + np.asarray(f) / 700.0)

    def imel(m):
        return 700.0 * (10.0 ** (np.asarray(m) / 2595.0) - 1.0)

    f = np.fft.rfftfreq(nfft, 1.0 / sr)
    pts = imel(np.linspace(mel(fmin), mel(fmax), n_mels + 2))
    fb = []
    for i in range(n_mels):
        lo, c, hi = pts[i:i + 3]
        w = np.maximum(0.0, np.minimum((f - lo) / (c - lo), (hi - f) / (hi - c)))
        if np.any(w > 0):
            fb.append(w)
    return np.array(fb)


def mel_db_distance(x: np.ndarray, y: np.ndarray, sizes=(256, 1024, 4096),
                    mels=(32, 64, 96)) -> tuple[float, list[float]]:
    """Mean absolute mel dB over cells above 80 dB under x's loudest cell."""
    from scipy.signal import stft
    x = np.asarray(x, dtype=float)
    y = np.asarray(y, dtype=float)
    n = max(len(x), len(y))
    x = np.pad(x, (0, n - len(x)))
    y = np.pad(y, (0, n - len(y)))
    per = []
    for size, nm in zip(sizes, mels):
        if n < size:
            continue
        fb = mel_filters(size, nm)
        sx = stft(x, nperseg=size, noverlap=size - size // 4)[2]
        sy = stft(y, nperseg=size, noverlap=size - size // 4)[2]
        mx = db(fb @ np.abs(sx) ** 2)
        my = db(fb @ np.abs(sy) ** 2)
        fl = mx.max() - 80.0
        act = (mx > fl) | (my > fl)
        if np.any(act):
            per.append(float(np.mean(np.abs(np.maximum(mx, fl) - np.maximum(my, fl))[act])))
    return (float(np.mean(per)) if per else 0.0), per
