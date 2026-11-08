"""A frozen copy of the tone checks, so benchmark numbers do not move when the live tone module changes.

Never edit this file. A change moves every benchmark number. Add new measures in bench.py instead and record
them in the benchmark change log.
"""

from __future__ import annotations

import os
from collections.abc import Sequence
from typing import Any

import numpy as np

SR = 48000
CEN = [1000 * 2 ** (k / 3) for k in range(-15, 13)]
REGIONS = {"<150": (0, 150), "150-1k": (150, 1000), "1-4k": (1000, 4000), "4-12k": (4000, 12000),
           ">12k": (12000, 24000)}


def _mono(x: Any) -> np.ndarray:
    if isinstance(x, (str, os.PathLike)):
        from hark.core.audio import load

        x = load(str(x), sr=SR)
    a = np.asarray(x, dtype=float)
    return a.mean(axis=1) if a.ndim == 2 else a


def label(c: float) -> str:
    """'1.00 kHz' style label at or above 1 kHz, else '250 Hz'."""
    return f"{c / 1000:.3g} kHz" if c >= 1000 else f"{c:.0f} Hz"


def _power(m: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    return np.abs(np.fft.rfft(m)) ** 2, np.fft.rfftfreq(len(m), 1 / SR)


def profile(x: Any) -> np.ndarray:
    """28 band levels in dB re the total, floor -60."""
    P, f = _power(_mono(x))
    b = np.array([P[(f >= c / 2 ** (1 / 6)) & (f < c * 2 ** (1 / 6))].sum() + 1e-20 for c in CEN])
    return np.maximum(10 * np.log10(b / b.sum()), -60.0)


def shares(x: Any) -> dict:
    """Energy share per region in dB, 1 decimal."""
    P, f = _power(_mono(x))
    tot = P.sum()
    out = {}
    for name, (lo, hi) in REGIONS.items():
        sel = (f >= lo) & ((f < hi) if hi < 24000 else (f <= hi))
        out[name] = round(float(10 * np.log10(P[sel].sum() / (tot + 1e-20) + 1e-12)), 1)
    return out


def t40_ms(x: Any, db: float = 40, ms: float = 5) -> float:
    """Time from the loudest 5 ms frame to the first frame more than db under it."""
    m = _mono(x)
    n = int(SR * ms / 1000)
    k = len(m) // n
    if k < 2:
        return 0.0
    e = 10 * np.log10(np.mean(m[: k * n].reshape(k, n) ** 2, axis=1) + 1e-20)
    i = int(np.argmax(e))
    after = np.flatnonzero(e[i:] < e[i] - db)
    return float((after[0] if len(after) else k - i) * ms)


def _rms(v: np.ndarray) -> float:
    return float(np.sqrt(np.mean(np.asarray(v) ** 2)))


class ToneRef:
    """Band profile statistics of a reference set."""

    def __init__(self, refs: Sequence[Any], margin: float = 3.0):
        ms = [_mono(r) for r in refs]
        self.margin = margin
        self.P = np.array([profile(m) for m in ms]) if ms else np.zeros((0, len(CEN)))
        self.t40 = [t40_ms(m) for m in ms]
        self.shares = [shares(m) for m in ms]
        self.length = [len(m) / SR for m in ms]
        n = len(self.P)
        self.ref_dist = [_rms(self.P[i] - np.median(np.delete(self.P, i, 0), axis=0)) for i in range(n)] \
            if n >= 3 else []
        if n:
            self.med, self.lo, self.hi = np.median(self.P, axis=0), self.P.min(axis=0), self.P.max(axis=0)

    @property
    def spread(self) -> tuple[float, float] | None:
        """(median, max) of the leave-one-out distances, or None with fewer than 3 refs."""
        return (float(np.median(self.ref_dist)), float(max(self.ref_dist))) if self.ref_dist else None

    def _outside(self, p: np.ndarray, dev: np.ndarray, lo: np.ndarray, hi: np.ndarray) -> list:
        over = np.maximum(p - hi - self.margin, 0) - np.maximum(lo - p - self.margin, 0)
        return [(label(c), round(float(d), 1), "above" if o > 0 else "below")
                for c, d, o in zip(CEN, dev, over) if o != 0]

    def check(self, x: Any, exclude: int | None = None) -> dict:
        """Tone distance of x to the kept refs, with band, tail, share and length facts."""
        if not len(self.P):
            return {}
        keep = [i for i in range(len(self.P)) if i != exclude]
        P = self.P[keep]
        med, lo, hi = np.median(P, axis=0), P.min(axis=0), P.max(axis=0)
        m = _mono(x)
        p = profile(m)
        dev = p - med
        sp = [self.ref_dist[i] for i in keep] if self.ref_dist else []
        dist = _rms(dev)
        sh = shares(m)
        t40s = [self.t40[i] for i in keep]
        lens = [self.length[i] for i in keep]
        out = {"tone_db": round(dist, 2), "tone_ratio": round(dist / max(float(np.median(sp)), 0.5), 2)
               if len(sp) >= 2 else None, "tone_ref": [round(s, 2) for s in sp],
               "band_dev": [round(float(d), 1) for d in dev], "outside": self._outside(p, dev, lo, hi),
               "t40_ms": t40_ms(m), "t40_ref": (min(t40s), max(t40s)), "length_s": round(len(m) / SR, 3),
               "shares": sh, "shares_ref_med": {k: round(float(np.median([self.shares[i][k] for i in keep])), 1)
                                                for k in REGIONS},
               "length_ref": (round(min(lens), 3), round(max(lens), 3))}
        out["words"] = self.words(out)
        return out

    def words(self, m: dict) -> str:
        """One plain sentence about the check result."""
        s = f"tone {m['tone_db']:.1f} dB from the refs' median"
        if m["tone_ref"]:
            s += f" (the refs sit {min(m['tone_ref'])}-{max(m['tone_ref'])} apart)"
        top = sorted(m["outside"], key=lambda o: -abs(o[1]))[:3]
        if top:
            s += ": " + ", ".join(f"{d:+.0f} dB at {b} ({w} every ref)" for b, d, w in top)
        a, b = m["t40_ref"]
        if m["t40_ms"] > 1.5 * b + 10 or m["t40_ms"] < 0.6 * a - 5:
            s += f"; tail t40 {m['t40_ms']:.0f} ms (refs {a:.0f}-{b:.0f})"
        far = [f"{k} {m['shares'][k] - v:+.0f} dB" for k, v in m["shares_ref_med"].items()
               if abs(m["shares"][k] - v) > 9]
        if far:
            s += "; energy share vs the refs: " + ", ".join(far)
        if m["length_s"] > 1.3 * m["length_ref"][1]:
            s += f"; {m['length_s']:.2f} s long (refs {m['length_ref'][0]}-{m['length_ref'][1]})"
        return s

    def region_penalty(self, m: dict, regions: Sequence[str] = ("<150", "4-12k", ">12k"), margin: float = 6,
                       scale: float = 6) -> float:
        """Penalty for energy shares well above the refs' median share."""
        if not m:
            return 0.0
        return float(sum(max(0.0, m["shares"][r] - m["shares_ref_med"][r] - margin) / scale for r in regions))

    def penalty(self, m: dict) -> float:
        """Tone ratio above 1, plus a long-tail term."""
        if not m or m.get("tone_ratio") is None:
            return 0.0
        pen = max(0.0, m["tone_ratio"] - 1)
        b = m["t40_ref"][1]
        if b > 0 and m["t40_ms"] > b:
            pen += 0.5 * min(2.0, (m["t40_ms"] - b) / b)
        return float(pen)
