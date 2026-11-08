"""Gallery measurements and drawings: decode, loudness, onset, card span and spectrogram thumbnails."""

from __future__ import annotations

import hashlib
import math
import os
from math import gcd
from typing import Any

import numpy as np

RATE = 48000
THUMB_W, THUMB_H = 520, 132
THUMB_VERSION = 6
DB_RANGE = 72.0
F_LO, F_HI = 30.0, 20000.0
ONSET_DB, PREROLL_S = -60.0, 0.005
MOMENTARY_S = 0.4
LUFS_OFFSET = -0.691
GUIDES_HZ = (100.0, 1000.0, 10000.0)
GUIDE_TINT = np.array([0.80, 0.83, 0.87])
CMAP_STOPS = ((0.00, "#fbfbfa"), (0.18, "#e6eaef"), (0.42, "#a9b8c9"),
              (0.62, "#5f7793"), (0.80, "#283b55"), (1.00, "#070c14"))


class Decoded:
    """1 decoded sound file: frames x channels float32 at its own rate, plus its SHA-1."""

    def __init__(self, path: str, x: np.ndarray, rate: int, sha1: str) -> None:
        self.path, self.x, self.rate, self.sha1 = path, x, rate, sha1


_CACHE: dict[str, Decoded] = {}


def clear_cache() -> None:
    _CACHE.clear()


def decode(path: str) -> Decoded:
    """Decode a file once per build (cached by path)."""
    if path not in _CACHE:
        import soundfile as sf
        with open(path, "rb") as fh:
            sha1 = hashlib.sha1(fh.read()).hexdigest()
        x, rate = sf.read(path, dtype="float32", always_2d=True)
        _CACHE[path] = Decoded(path, x, int(rate), sha1)
    return _CACHE[path]


def to_48k(x: np.ndarray, rate: int) -> np.ndarray:
    """Polyphase resample to 48 kHz (axis 0)."""
    if rate == RATE:
        return x
    from scipy.signal import resample_poly
    g = gcd(int(rate), RATE)
    return resample_poly(x, RATE // g, int(rate) // g, axis=0)


def _k_weight(ch: np.ndarray) -> np.ndarray:
    from hark.core.features import k_weight
    return np.asarray(k_weight(ch, sr=RATE), dtype=np.float64)


def lufs_max(x: np.ndarray, rate: int) -> float | None:
    """Momentary-max loudness (0.4 s window, K-weighted), 1 decimal; None for 0 frames."""
    x = np.asarray(x)
    x = x[:, None] if x.ndim == 1 else x
    y = to_48k(x, rate).astype(np.float64)
    if y.shape[0] == 0:
        return None
    p = sum(_k_weight(y[:, c]) ** 2 for c in range(y.shape[1]))
    n = int(MOMENTARY_S * RATE)
    if len(p) < n:
        ms = float(np.sum(p)) / n
    else:
        cs = np.concatenate([[0.0], np.cumsum(p)])
        ms = float(np.max(cs[n:] - cs[:-n])) / n
    return round(LUFS_OFFSET + 10 * math.log10(ms + 1e-24), 1)


def measure(path: str) -> dict[str, Any]:
    """dur_s, peak_dbfs, lufs, channels and sha1 of 1 sound file."""
    d = decode(path)
    peak = float(np.max(np.abs(d.x))) if d.x.size else 0.0
    return {"dur_s": round(d.x.shape[0] / d.rate, 3),
            "peak_dbfs": round(20 * math.log10(peak), 1) if peak > 0 else None,
            "lufs": lufs_max(d.x, d.rate) if d.x.shape[0] else None,
            "channels": int(d.x.shape[1]), "sha1": d.sha1}


def onset_index(m: np.ndarray, rate: int) -> int:
    """Start index for drawing: 5 ms before the first sample above -60 dB re peak."""
    if m.size == 0:
        return 0
    peak = float(np.max(np.abs(m)))
    if peak <= 0:
        return 0
    first = int(np.argmax(np.abs(m) > peak * 10 ** (ONSET_DB / 20)))
    return max(0, first - int(round(PREROLL_S * rate)))


def card_span(durations: list[float | None]) -> float:
    """1 time span per card: max(0.05, min(max d, 2.5 x median d)); 1.0 s without durations."""
    d = [float(v) for v in durations if v]
    if not d:
        return 1.0
    return max(0.05, min(max(d), 2.5 * float(np.median(d))))


def _nfft(span: float, n: int) -> int:
    base = 256 if span < 0.15 else 512 if span < 0.6 else 1024 if span < 2.5 else 2048
    return min(base, max(64, 2 ** int(math.floor(math.log2(max(n, 1))))))


def _bin_mean(P: np.ndarray, pos: np.ndarray, edges: np.ndarray) -> np.ndarray:
    """Mean of rows of P whose position lies in each [edge_i, edge_i+1); empty slots interpolate."""
    cs = np.concatenate([np.zeros((1,) + P.shape[1:]), np.cumsum(P, axis=0)], axis=0)
    idx = np.searchsorted(pos, edges, side="left")
    counts = np.diff(idx)
    sums = cs[idx[1:]] - cs[idx[:-1]]
    out = sums / np.maximum(counts, 1).reshape((-1,) + (1,) * (P.ndim - 1))
    empty = np.where(counts == 0)[0]
    if empty.size:
        centres = np.sqrt(edges[:-1] * edges[1:]) if edges[0] > 0 else (edges[:-1] + edges[1:]) / 2
        out[empty] = _interp_rows(P, pos, centres[empty])
    return out


def _interp_rows(P: np.ndarray, pos: np.ndarray, at: np.ndarray) -> np.ndarray:
    j = np.clip(np.searchsorted(pos, at), 1, len(pos) - 1)
    lo, hi = pos[j - 1], pos[j]
    w = np.clip((at - lo) / np.where(hi > lo, hi - lo, 1.0), 0.0, 1.0)
    w = w.reshape((-1,) + (1,) * (P.ndim - 1))
    return P[j - 1] * (1 - w) + P[j] * w


def _smooth_streaks(S: np.ndarray, edges: np.ndarray, rate: int, nfft: int, dt_col: float) -> np.ndarray:
    from scipy.signal.windows import bartlett
    bin_hz = rate / nfft
    for k in range(S.shape[0]):
        nb = max((edges[k + 1] - edges[k]) / bin_hz, 1.0)
        if nb >= 4:
            continue
        L = int(math.floor(min(THUMB_W / 16, round(4 * (nfft / rate) / nb / dt_col))))
        if L >= 3:
            w = bartlett(L + 2)[1:-1]
            S[k] = np.convolve(S[k], w / w.sum(), mode="same")
    return S


def _hex_rgb(h: str) -> np.ndarray:
    return np.array([int(h[i:i + 2], 16) / 255 for i in (1, 3, 5)])


def colour(v: np.ndarray) -> np.ndarray:
    """The 6-stop white-to-ink colour map on clip(v, 0, 1); shape (..., 3)."""
    pos = np.array([p for p, _ in CMAP_STOPS])
    rgb = np.array([_hex_rgb(h) for _, h in CMAP_STOPS])
    v = np.clip(v, 0, 1)
    return np.stack([np.interp(v, pos, rgb[:, c]) for c in range(3)], axis=-1)


def _guides(img: np.ndarray, v: np.ndarray, centres: np.ndarray) -> np.ndarray:
    for f in GUIDES_HZ:
        if not centres[0] <= f <= centres[-1]:
            continue
        row = THUMB_H - 1 - int(np.argmin(np.abs(centres - f)))
        mask = v[row] < 0.35
        img[row, mask] = img[row, mask] * 0.9 + GUIDE_TINT * 0.1 - 0.035
    return img


def _power(m: np.ndarray, rate: int, nfft: int, hop: int) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    from scipy.signal import stft
    f, t, Z = stft(m, fs=rate, window="hann", nperseg=nfft, noverlap=nfft - hop,
                   boundary="zeros", padded=True)
    return f, t, np.abs(Z) ** 2


def spectrogram(x: np.ndarray, rate: int, span: float) -> np.ndarray:
    """A 132 x 520 RGB uint8 picture of the sound from its onset over ``span`` seconds."""
    m = np.asarray(x, dtype=np.float64)
    m = m.mean(axis=1) if m.ndim == 2 else m
    m = m[onset_index(m, rate):]
    n_span = max(int(math.floor(span * rate)), 64)
    m = np.pad(m[:n_span], (0, max(0, n_span - len(m[:n_span]))))
    nfft = _nfft(span, len(m))
    hop = int(np.clip(n_span // (2 * THUMB_W), 8, nfft // 2))
    f, t, P = _power(m, rate, nfft, hop)
    edges = np.geomspace(F_LO, min(F_HI, rate / 2), THUMB_H + 1)
    rows = _bin_mean(P, f, edges)
    dt_col = n_span / rate / THUMB_W
    S = _bin_mean(rows.T, t, np.linspace(0, n_span / rate, THUMB_W + 1)).T
    S = _smooth_streaks(S, edges, rate, nfft, dt_col)
    db = 10 * np.log10(S + 1e-20)
    db = np.maximum(db - db.max(), -DB_RANGE)
    v = ((db + DB_RANGE) / DB_RANGE)[::-1]
    img = _guides(colour(v), v, np.sqrt(edges[:-1] * edges[1:]))
    return np.round(np.clip(img, 0, 1) * 255).astype(np.uint8)


def thumb_name(label: str, category: str, path: str, span: float) -> str:
    """<label>_<category>_<stem>_<h8>.png with unsafe characters replaced."""
    import re
    st = os.stat(path)
    norm = os.path.abspath(path).replace("\\", "/")
    key = f"{norm}|{st.st_size}|{st.st_mtime_ns}|{span:.4f}|{THUMB_VERSION}"
    h8 = hashlib.sha1(key.encode()).hexdigest()[:8]
    stem = os.path.splitext(os.path.basename(path))[0]
    safe = re.sub(r"[^A-Za-z0-9_.-]", "-", f"{label}_{category}_{stem}").strip("-")
    return f"{safe}_{h8}.png"


def write_thumb(path: str, out_png: str, span: float) -> bool:
    """Write the thumbnail; return True when it was made, False when it was reused."""
    if os.path.isfile(out_png):
        return False
    from PIL import Image
    d = decode(path)
    tmp = out_png + ".part"
    Image.fromarray(spectrogram(d.x, d.rate, span), "RGB").save(tmp, format="PNG")
    os.replace(tmp, out_png)
    return True
