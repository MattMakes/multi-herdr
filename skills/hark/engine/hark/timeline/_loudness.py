"""Loudness picture of a window: curves, integrated, LRA, true peak, gaps, clipping (analysis 2.4)."""

from __future__ import annotations

import numpy as np
from scipy import ndimage

from hark.timeline._dsp import SR, runs


def _curve(x: np.ndarray, win: float, hop: float) -> tuple[np.ndarray, np.ndarray]:
    from hark.core.features import loudness_curve

    t, L = loudness_curve(x, win, hop)
    return np.asarray(t, dtype=float), np.maximum(np.asarray(L, dtype=float), -70.0)


def _abs_max(x: np.ndarray) -> np.ndarray:
    a = np.abs(np.asarray(x))
    return a.max(axis=1) if a.ndim == 2 else a


def clipping(x: np.ndarray) -> dict:
    c = _abs_max(x) >= 0.999
    r = runs(c, 3)
    return {"samples": int(c.sum()), "runs": len(r), "times": [round(s / SR, 3) for s, _ in r[:20]]}


def _threshold(integrated: float | None) -> float:
    if integrated is None or integrated <= -60:
        return -70.0
    return max(-60.0, integrated - 20)


def _low_mask(F: np.ndarray, integrated: float | None, thr: float) -> np.ndarray:
    low = F <= thr if integrated is None else F < thr
    padded = np.pad(low, 3, mode="edge")
    closed = ndimage.binary_closing(padded, structure=np.ones(7, dtype=bool))[3:-3]
    return closed & (F < thr + 6)


def _where(start: float, end: float, dur: float, at_start: bool, at_end: bool) -> str | None:
    s = start <= 0.05 and at_start
    e = end >= dur - 0.05 and at_end
    if s and e:
        return "whole file"
    return "lead-in" if s else "tail" if e else None


def gaps(x: np.ndarray, integrated: float | None, at_start: bool = True, at_end: bool = True) -> list[dict]:
    dur = len(x) / SR
    if dur < 0.5:
        return []
    t, F = _curve(x, 0.1, 0.05)
    thr = _threshold(integrated)
    amax = _abs_max(x)
    out = []
    for s, e in runs(_low_mask(F, integrated, thr), 5):
        start = 0.0 if s == 0 else max(0.0, t[s] - 0.05)
        end = dur if e == F.size else min(dur, t[e - 1] + 0.05)
        seg = amax[int(start * SR): max(int(start * SR) + 1, int(end * SR))]
        kind = ("digital silence" if seg.size and seg.max() < 1e-5
                else "near silence" if F[s:e].max() < -60 else "drop")
        gap = {"start": round(float(start), 2), "end": round(float(end), 2), "dur": round(float(end - start), 2),
               "kind": kind, "level_lufs": round(float(F[s:e].mean()), 1)}
        where = _where(start, end, dur, at_start, at_end)
        if where:
            gap["where"] = where
        out.append(gap)
    return out


def _momentary(x: np.ndarray, out: dict) -> float | None:
    from hark.core.features import integrated_lufs

    t, L = _curve(x, 0.4, 0.1)
    integrated = None
    if L.max() > -70:
        val = float(integrated_lufs(x))
        integrated = round(val, 1) if val > -70 else None
        k = int(np.argmax(L))
        out["max_momentary"] = {"t": round(float(t[k]), 3), "lufs": round(float(L[k]), 1)}
    out["momentary"] = {"t0": round(float(t[0]), 3), "dt": 0.1, "lufs": [round(float(v), 1) for v in L]}
    return integrated


def _short_term(x: np.ndarray, out: dict) -> None:
    t, L = _curve(x, 3.0, 0.5)
    if L.max() > -70:
        k = int(np.argmax(L))
        out["max_short_term"] = {"start": round(float(t[k] - 1.5), 3), "end": round(float(t[k] + 1.5), 3),
                                 "lufs": round(float(L[k]), 1)}
    out["short_term"] = {"t0": round(float(t[0]), 3), "dt": 0.5, "lufs": [round(float(v), 1) for v in L]}


def loudness(x: np.ndarray, at_start: bool = True, at_end: bool = True) -> dict:
    """The loudness picture of x (times relative to x)."""
    from hark.core.features import lra, true_peak_dbtp

    x = np.asarray(x)
    dur = len(x) / SR
    silent = not np.any(x)
    out: dict = {"integrated_lufs": None,
                 "true_peak_dbtp": None if silent else round(float(true_peak_dbtp(x)), 1),
                 "digital_silence": bool(silent)}
    integrated = _momentary(x, out) if dur >= 0.4 else None
    out["integrated_lufs"] = integrated
    if dur >= 3.0:
        _short_term(x, out)
    value = lra(x) if dur >= 6.0 else None
    out["lra_lu"] = round(float(value), 1) if value is not None else None
    out["silences"] = gaps(x, integrated, at_start, at_end)
    out["clipping"] = clipping(x)
    return out
