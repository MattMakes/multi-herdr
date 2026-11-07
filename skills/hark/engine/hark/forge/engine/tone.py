"""Model-free tone and envelope checks against real references.

ToneRef compares the 1/3-octave balance of a sound with its references.
EnvRef compares band energy over time (a bandgram) with them.
Every measurement uses the mono mix of the whole file at 48 kHz.
"""

from __future__ import annotations

import math
import os
from collections.abc import Sequence
from typing import Any

import numpy as np
from scipy import signal

SR = 48000
CEN = np.array([1000.0 * 2 ** (k / 3) for k in range(-15, 13)])
EDGE = 2 ** (1 / 6)
REGIONS = {"<150": (0, 150), "150-1k": (150, 1000), "1-4k": (1000, 4000), "4-12k": (4000, 12000),
           ">12k": (12000, 24000)}
FLOOR = -70.0
HOP_MS = 10.0
STRETCH = (0.85, 0.92, 1.0, 1.08, 1.18)
SHIFT = (-3, -2, -1, 0, 1, 2, 3)
SEGMENTS = (("pre", -math.inf, -2), ("attack", -2, 3), ("body", 3, 20), ("tail", 20, 100), ("late", 100, math.inf))
GROUPS = (("31-80 Hz", 0, 90), ("100-200 Hz", 90, 224), ("250-500 Hz", 224, 560), ("630-1250 Hz", 560, 1400),
          ("1.6-3.2 kHz", 1400, 3550), ("4-6.3 kHz", 3550, 7100), ("8-10 kHz", 7100, 11200),
          ("12.5-16 kHz", 11200, math.inf))
OUT_MARGIN = 6.0

_BANK: list | None = None


# ---------------------------------------------------------------- input

def as_mono(x: Any) -> np.ndarray:
    """Mono float64 signal from a path or an array (48 kHz)."""
    if isinstance(x, (str, os.PathLike)):
        from hark.core import audio

        x = audio.load(str(x), sr=SR)
    a = np.asarray(x, dtype=float)
    return a.mean(axis=1) if a.ndim == 2 else a


def label(c: float) -> str:
    """Band label, for example '1.00 kHz' or '250 Hz'."""
    return f"{c / 1000:.3g} kHz" if c >= 1000 else f"{round(c)} Hz"


def _power(m: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    return np.fft.rfftfreq(len(m), 1.0 / SR), np.abs(np.fft.rfft(m)) ** 2


# ---------------------------------------------------------------- spectrum

def profile(x: Any) -> np.ndarray:
    """28 band levels in dB relative to the total power of the bands, floored at -60."""
    f, p = _power(as_mono(x))
    bands = np.array([p[(f >= c / EDGE) & (f < c * EDGE)].sum() for c in CEN]) + 1e-20
    return np.maximum(10 * np.log10(bands / bands.sum()), -60.0)


def shares(x: Any) -> dict:
    """Energy share per region in dB."""
    f, p = _power(as_mono(x))
    tot = p.sum() + 1e-20
    return {k: round(float(10 * np.log10(p[(f >= a) & (f < b)].sum() / tot + 1e-12)), 1)
            for k, (a, b) in REGIONS.items()}


def t40_ms(x: Any, db: float = 40.0, ms: float = 5.0) -> float:
    """Time from the loudest frame to the first frame db under it."""
    m = as_mono(x)
    n = int(SR * ms / 1000)
    k = len(m) // n
    if k < 2:
        return 0.0
    fr = m[: k * n].reshape(k, n)
    e = 20 * np.log10(np.sqrt(np.mean(fr * fr, axis=1)) + 1e-12)
    i = int(np.argmax(e))
    under = np.nonzero(e[i:] < e[i] - db)[0]
    j = int(under[0]) if len(under) else k - i
    return float(j * ms)


# ---------------------------------------------------------------- ToneRef

class ToneRef:
    """The 1/3-octave balance of a set of references."""

    def __init__(self, refs: Sequence[Any], margin: float = 3.0):
        mono = [as_mono(r) for r in refs]
        self.n = len(mono)
        self.margin = margin
        self.P = np.array([profile(m) for m in mono]) if mono else np.zeros((0, len(CEN)))
        self.med = np.median(self.P, axis=0) if self.n else None
        self.lo = self.P.min(axis=0) if self.n else None
        self.hi = self.P.max(axis=0) if self.n else None
        self.t40 = np.array([t40_ms(m) for m in mono])
        self.S = [shares(m) for m in mono]
        self.len_s = np.array([len(m) / SR for m in mono])
        self.ref_dist = self._loo() if self.n >= 3 else np.array([])

    def _loo(self) -> np.ndarray:
        out = []
        for i in range(self.n):
            others = np.delete(self.P, i, axis=0)
            out.append(float(np.sqrt(np.mean((self.P[i] - np.median(others, axis=0)) ** 2))))
        return np.array(out)

    @property
    def spread(self) -> tuple[float, float] | None:
        if len(self.ref_dist) == 0:
            return None
        return float(np.median(self.ref_dist)), float(np.max(self.ref_dist))

    def _keep(self, exclude: Any) -> list[int]:
        drop = set() if exclude is None else ({exclude} if np.isscalar(exclude) else set(exclude))
        return [i for i in range(self.n) if i not in drop]

    def check(self, x: Any, exclude: Any = None) -> dict:
        """Distance of x from the refs' median balance, with outside bands and words."""
        keep = self._keep(exclude)
        if not keep:
            return {}
        P = self.P[keep]
        med, lo, hi = np.median(P, axis=0), P.min(axis=0), P.max(axis=0)
        m = as_mono(x)
        p = profile(m)
        dev = p - med
        sp = self.ref_dist[keep] if len(self.ref_dist) else np.array([])
        ratio = float(np.sqrt(np.mean(dev ** 2))) / max(float(np.median(sp)), 0.5) if len(sp) >= 2 else None
        out = _outside_bands(p, dev, lo, hi, self.margin)
        res = {"tone_db": round(float(np.sqrt(np.mean(dev ** 2))), 2),
               "tone_ratio": None if ratio is None else round(ratio, 2),
               "tone_ref": [round(float(v), 2) for v in sp], "band_dev": [round(float(v), 1) for v in dev],
               "outside": out, "t40_ms": t40_ms(m),
               "t40_ref": (float(self.t40[keep].min()), float(self.t40[keep].max())),
               "length_s": round(len(m) / SR, 3), "shares": shares(m),
               "shares_ref_med": {k: round(float(np.median([self.S[i][k] for i in keep])), 1) for k in REGIONS},
               "length_ref": (round(float(self.len_s[keep].min()), 3), round(float(self.len_s[keep].max()), 3))}
        res["words"] = ToneRef.words(res)
        return res

    @staticmethod
    def words(m: dict) -> str:
        """One plain line about the tone check."""
        s = f"tone {m['tone_db']:.1f} dB from the refs' median"
        if m.get("tone_ref"):
            s += f" (the refs sit {min(m['tone_ref']):.1f}-{max(m['tone_ref']):.1f} apart)"
        top = sorted(m.get("outside", []), key=lambda o: -abs(o[1]))[:3]
        if top:
            s += ": " + ", ".join(f"{d:+.0f} dB at {b} ({w} every ref)" for b, d, w in top)
        return s + _tone_tail_words(m)

    @staticmethod
    def region_penalty(m: dict, regions: Sequence[str] = ("<150", "4-12k", ">12k"), margin: float = 6.0,
                       scale: float = 6.0) -> float:
        """1 per scale dB of region share beyond the refs' median plus margin."""
        if not m:
            return 0.0
        sh, ref = m["shares"], m["shares_ref_med"]
        return float(sum(max(0.0, sh[r] - ref[r] - margin) / scale for r in regions))

    def penalty(self, m: dict) -> float:
        """Tone distance beyond a typical real recording, plus a long-tail term."""
        if not m or m.get("tone_ratio") is None:
            return 0.0
        pen = max(0.0, m["tone_ratio"] - 1)
        b = m["t40_ref"][1]
        if b > 0 and m["t40_ms"] > b:
            pen += 0.5 * min(2.0, (m["t40_ms"] - b) / b)
        return float(pen)


def _outside_bands(p: np.ndarray, dev: np.ndarray, lo: np.ndarray, hi: np.ndarray, margin: float) -> list:
    out = []
    for i, c in enumerate(CEN):
        over = max(p[i] - hi[i] - margin, 0) - max(lo[i] - p[i] - margin, 0)
        if over != 0:
            out.append((label(c), round(float(dev[i]), 1), "above" if over > 0 else "below"))
    return out


def _tone_tail_words(m: dict) -> str:
    s = ""
    a, b = m.get("t40_ref", (0, 0))
    t = m.get("t40_ms", 0.0)
    if t > 1.5 * b + 10 or t < 0.6 * a - 5:
        s += f"; tail t40 {t:.0f} ms (refs {a:.0f}-{b:.0f})"
    ref = m.get("shares_ref_med", {})
    diffs = [(k, v - ref[k]) for k, v in m.get("shares", {}).items() if k in ref and abs(v - ref[k]) > 9]
    if diffs:
        s += "; energy share vs the refs: " + ", ".join(f"{k} {d:+.0f} dB" for k, d in diffs)
    lo, hi = m.get("length_ref", (0, 0))
    if hi and m.get("length_s", 0) > 1.3 * hi:
        s += f"; {m['length_s']:.2f} s long (refs {lo:.2f}-{hi:.2f})"
    return s


# ---------------------------------------------------------------- bandgram

def _filters() -> list:
    global _BANK
    if _BANK is None:
        _BANK = [signal.butter(3, [c / EDGE, min(c * EDGE, 0.49 * SR)], "bandpass", fs=SR, output="sos")
                 for c in CEN]
    return _BANK


def bandgram(x: Any, hop_ms: float = HOP_MS) -> np.ndarray:
    """Band energy over time: (frames, 28) dB relative to the loudest frame, floored at -70."""
    m = as_mono(x)
    n = int(round(SR * hop_ms / 1000))
    k = max(1, math.ceil(len(m) / n))
    mp = np.zeros(k * n)
    mp[: len(m)] = m
    G = np.column_stack([np.mean(signal.sosfilt(s, mp).reshape(k, n) ** 2, axis=1) for s in _filters()])
    return np.maximum(10 * np.log10(G / (G.sum(axis=1).max() + 1e-30) + 1e-30), FLOOR)


def _anchors(G: np.ndarray) -> dict:
    tot = 10 * np.log10(np.sum(10 ** (G / 10), axis=1) + 1e-30)
    pk = int(np.argmax(tot))
    act = np.nonzero(tot >= tot[pk] - 50)[0]
    return {"peak": pk, "onset": int(np.argmax(tot >= tot[pk] - 20)), "lo": int(act[0]), "hi": int(act[-1])}


def _rows(G: np.ndarray, t: np.ndarray) -> np.ndarray:
    i = np.floor(t).astype(int)
    fr = (t - i)[:, None]

    def row(j: np.ndarray) -> np.ndarray:
        ok = (j >= 0) & (j < len(G))
        out = np.full((len(j), G.shape[1]), FLOOR)
        out[ok] = G[j[ok]]
        return out

    return row(i) * (1 - fr) + row(i + 1) * fr


def _cost(Av: np.ndarray, Bv: np.ndarray, s: float) -> tuple[float, np.ndarray]:
    w = np.clip((np.maximum(Av, Bv) - FLOOR) / 40, 0, 1)
    d = float(np.sum(w * np.minimum(np.abs(Av - Bv), 30)) / (np.sum(w) + 1e-9))
    return d * (1 + 0.5 * abs(math.log(s))), w


def _frames(a: dict, b: dict, name: str) -> np.ndarray:
    a0, b0 = a[name], b[name]
    lo = max(-300, min(a["lo"] - a0, b["lo"] - b0)) - 3
    hi = min(600, max(a["hi"] - a0, b["hi"] - b0)) + 4
    return np.arange(lo, hi)


def _search(A: np.ndarray, B: np.ndarray, stretch: Sequence[float], shift: Sequence[int]) -> dict:
    a, b = _anchors(A), _anchors(B)
    best = None
    for name in ("onset", "peak"):
        tt = _frames(a, b, name)
        Bv = _rows(B, b[name] + tt)
        for s in stretch:
            c, w = _cost(_rows(A, a[name] + tt / s), Bv, s)
            if best is None or c < best["cost"]:
                best = {"cost": c, "anchor": name, "stretch": s, "shift": 0, "tt": tt, "Bv": Bv}
    tt, a0 = best["tt"], a[best["anchor"]]
    for sh in shift:
        if sh == 0:
            continue
        c, _ = _cost(_rows(A, a0 + tt / best["stretch"] + sh), best["Bv"], best["stretch"])
        if c < best["cost"]:
            best.update(cost=c, shift=sh)
    best["Av"] = _rows(A, a0 + tt / best["stretch"] + best["shift"])
    return best


def _cells(best: dict) -> dict:
    Av, Bv, tt = best["Av"], best["Bv"], best["tt"]
    w = np.clip((np.maximum(Av, Bv) - FLOOR) / 40, 0, 1)
    W = w.sum() + 1e-9
    cells = {}
    for seg, s0, s1 in SEGMENTS:
        rows = (tt >= s0) & (tt < s1)
        for grp, g0, g1 in GROUPS:
            cols = (CEN >= g0) & (CEN < g1)
            ww = w[np.ix_(rows, cols)]
            if ww.sum() < 0.01 * W:
                continue
            diff = float(np.sum(ww * (Av - Bv)[np.ix_(rows, cols)]) / ww.sum())
            cells[f"{seg} {grp}"] = (round(diff, 1), round(float(ww.sum() / W), 3))
    return cells


def env_dist(A: np.ndarray, B: np.ndarray, stretch: Sequence[float] = STRETCH, shift: Sequence[int] = SHIFT,
             detail: bool = False) -> Any:
    """Envelope distance in dB of bandgram A (judged) from bandgram B (reference)."""
    best = _search(np.asarray(A), np.asarray(B), stretch, shift)
    if not detail:
        return best["cost"]
    info = {"anchor": best["anchor"], "stretch": best["stretch"], "shift_ms": best["shift"] * 10,
            "cells": _cells(best)}
    return best["cost"], info


# ---------------------------------------------------------------- EnvRef

def _name(ref: Any, i: int) -> str:
    return str(ref) if isinstance(ref, (str, os.PathLike)) else f"ref{i}"


class EnvRef:
    """Band-energy shapes of a set of references."""

    def __init__(self, refs: Sequence[Any], names: Sequence[str] | None = None, margin: float = OUT_MARGIN):
        refs = list(refs)
        self.names = list(names) if names is not None else [_name(r, i) for i, r in enumerate(refs)]
        self.margin = margin
        self.G = [bandgram(r) for r in refs]
        self.n = len(self.G)
        self.D = np.full((self.n, self.n), np.inf)
        self._detail: dict = {}
        for i in range(self.n):
            for j in range(self.n):
                if i != j:
                    self.D[i, j], self._detail[(i, j)] = env_dist(self.G[i], self.G[j], detail=True)
        self.cell_spread = self._cell_spread()
        self.out_ref = np.array([self._outside([self._detail[(i, j)] for j in range(self.n) if j != i])[1]
                                 for i in range(self.n)]) if self.n >= 2 else np.array([])

    def _cell_spread(self) -> dict:
        pool: dict[str, list[float]] = {}
        for info in self._detail.values():
            for key, (diff, _) in info["cells"].items():
                pool.setdefault(key, []).append(abs(diff))
        return {k: float(np.median(v)) for k, v in pool.items() if len(v) >= 2}

    def _outside(self, infos: list[dict]) -> tuple[list, float]:
        if len(infos) < 2:
            return [], 0.0
        out, excess = [], 0.0
        for key in infos[0]["cells"]:
            if not all(key in i["cells"] for i in infos):
                continue
            diffs = [i["cells"][key][0] for i in infos]
            share = float(np.mean([i["cells"][key][1] for i in infos]))
            lo = min(diffs) if all(d > 0 for d in diffs) else (-max(diffs) if all(d < 0 for d in diffs) else 0.0)
            tol = max(self.margin, self.cell_spread.get(key, self.margin))
            if lo > tol and share >= 0.02:
                out.append((key, round(math.copysign(lo, diffs[0]), 1), round(share, 3)))
                excess += (lo - tol) * min(1.0, share / 0.05)
        out.sort(key=lambda o: -abs(o[1]) * o[2])
        return out, excess

    def _keep(self, exclude: Any) -> list[int]:
        drop = set() if exclude is None else ({exclude} if np.isscalar(exclude) else set(exclude))
        return [i for i in range(self.n) if i not in drop]

    def spread(self, exclude: Any = None) -> list[float]:
        """Nearest-neighbour distance of each kept ref, with 3 or more kept."""
        keep = self._keep(exclude)
        if len(keep) < 3:
            return []
        return [float(min(self.D[i, j] for j in keep if j != i)) for i in keep]

    def check(self, x: Any, exclude: Any = None, G: np.ndarray | None = None) -> dict:
        """Envelope distance to the nearest kept ref, outside cells and words."""
        keep = self._keep(exclude)
        if not keep:
            return {}
        g = bandgram(x) if G is None else G
        res = [env_dist(g, self.G[i], detail=True) for i in keep]
        k = int(np.argmin([r[0] for r in res]))
        d, info = res[k]
        sp = self.spread(exclude)
        outside, excess = self._outside([r[1] for r in res])
        oref = self.out_ref[keep] if self.n >= 3 else np.array([])
        m = {"env_db": round(d, 2), "env_ratio": round(d / max(float(np.median(sp)), 1.0), 2) if sp else None,
             "env_ref": [round(v, 2) for v in sp], "nearest": os.path.basename(self.names[keep[k]].replace("\\", "/")),
             "nearest_i": keep[k], "anchor": info["anchor"], "stretch": info["stretch"],
             "shift_ms": info["shift_ms"], "cells": info["cells"], "outside": outside, "env_out": round(excess, 2),
             "env_out_ref": round(float(np.median(oref)), 2) if len(oref) else None}
        m["words"] = EnvRef.words(m)
        return m

    @staticmethod
    def words(m: dict) -> str:
        """One plain line about the envelope check."""
        s = f"envelope {m['env_db']:.1f} dB from the nearest ref {m['nearest']}"
        if m.get("env_ref"):
            s += f" (the refs sit {min(m['env_ref']):.1f}-{max(m['env_ref']):.1f} apart)"
        if m.get("outside"):
            return s + "; beyond every ref: " + ", ".join(f"{k} {v:+.0f} dB" for k, v, _ in m["outside"][:3])
        big = [(k, v, sh) for k, (v, sh) in m.get("cells", {}).items() if abs(v) >= 8]
        big.sort(key=lambda c: -abs(c[1]) * math.sqrt(c[2]))
        if big:
            s += ": vs it " + ", ".join(f"{k} {v:+.0f} dB" for k, v, _ in big[:2])
        return s

    @staticmethod
    def penalty(m: dict, cap: float = 2.0) -> float:
        """Distance beyond typical refs plus a gap no ref shares."""
        if not m:
            return 0.0
        near = max(0.0, m["env_ratio"] - 1) if m.get("env_ratio") is not None else 0.0
        tol = max(3.0, m.get("env_out_ref") or 0.0)
        return float(min(cap, near + max(0.0, m.get("env_out", 0.0) - tol) / 6))
