"""Model-free artifact detectors, polish measures and their penalty rules.

Each detector returns plain data. PolishRef turns the measures into critic
penalties, relative to real references where it has them. K-weighting comes
from hark.core.features.k_weight (one BS.1770 filter for all of hark).
"""

from __future__ import annotations

import math
import os
import sys
from collections.abc import Sequence
from types import SimpleNamespace
from typing import Any

import numpy as np
from scipy import fft as sp_fft
from scipy import ndimage, signal

from hark.forge.engine import tone
from hark.forge.engine.tone import CEN

SR = 48000
ALIAS_PEAK_DB = 10.0
ALIAS_FAMILY = 0.5
ALIAS_LOUDEST = 4.0
RING_SPAN_HZ = 2000.0
RING_PITCH_Z = 100.0
RING_SMOOTH_DB = 2.0
HARD_AT = 0.5
GATE_DB = {"designed": -30.0, "gated": -12.0}

# key: (bad, abs, margin, scale, kind, cap)
RULES: dict[str, tuple] = {
    "clicks": (1, 0.0, 0.0, 1.0, "hard", 2.0),
    "edge_db": (1, -40.0, 6.0, 10.0, "hard", 2.0),
    "fold": (1, 0.02, 0.02, 0.03, "hard", 2.0),
    "nyq_db": (1, -35.0, 6.0, 8.0, "soft", 1.0),
    "dc_db": (1, -40.0, 6.0, 10.0, "hard", 2.0),
    "infra_db": (1, -20.0, 6.0, 8.0, "soft", 1.5),
    "res_db": (1, 15.0, 3.0, 5.0, "soft", 2.0),
    "ring_z": (1, 12.0, 3.0, 6.0, "soft", 2.0),
    "trunc_db": (1, -45.0, 6.0, 10.0, "hard", 2.0),
    "silence_ms": (1, 2.0, 2.0, 10.0, "hard", 2.0),
    "clip_runs": (1, 0.0, 1.0, 4.0, "hard", 2.0),
    "evolution_db": (-1, None, 0.35, None, "soft", 2.0),
}

# key: (bad, kind, margin, scale)
POLISH: dict[str, tuple] = {
    "attack_ms": (1, "log", 1.5, 1.0),
    "onset_hf_db": (-1, "db", 4.0, 6.0),
    "bump_db": (1, "db", 2.0, 6.0),
    "t40_ms": (-1, "log", 0.6, 1.0),
    "crest_db": (1, "db", 2.0, 4.0),
}

PRINT_KEYS = ("clicks edge_db fold nyq_db dc_db res_db res_hz ring_z trunc_db silence_ms clip_runs evolution_db "
              "attack_ms onset_hf_db bump_db t40_ms crest_db").split()


# ---------------------------------------------------------------- helpers

def _raw(x: Any) -> np.ndarray:
    if isinstance(x, (str, os.PathLike)):
        from hark.core import audio

        x = audio.load(str(x), sr=SR)
    return np.asarray(x, dtype=float)


def _mono(x: Any) -> np.ndarray:
    a = _raw(x)
    return a.mean(axis=1) if a.ndim == 2 else a


def _r(v: Any, d: int) -> Any:
    return None if v is None else round(float(v), d)


def frame_db(x: np.ndarray, n: int) -> np.ndarray:
    """Power in dB of consecutive n-sample frames."""
    k = len(x) // n
    if k < 1:
        return np.array([-240.0])
    return 10 * np.log10(np.mean(x[: k * n].reshape(k, n) ** 2, axis=1) + 1e-24)


def power_stft(x: np.ndarray, nfft: int, hop: int) -> np.ndarray:
    """Hann-windowed power STFT, shape (frames, bins)."""
    if len(x) < nfft:
        x = np.concatenate([x, np.zeros(nfft - len(x))])
    k = 1 + (len(x) - nfft) // hop
    idx = np.arange(nfft)[None, :] + hop * np.arange(k)[:, None]
    return np.abs(np.fft.rfft(x[idx] * np.hanning(nfft), axis=1)) ** 2


def _k_weight(x: np.ndarray) -> np.ndarray:
    from hark.core.features import k_weight

    return np.asarray(k_weight(np.asarray(x, dtype=float), SR), dtype=float)


def _lsq_slope(y: np.ndarray, dt: float) -> float:
    t = np.arange(len(y)) * dt
    return float(np.polyfit(t, y, 1)[0])


# ---------------------------------------------------------------- clicks

def _impulsive(d2: np.ndarray, k: int) -> np.ndarray:
    fr = d2[: k * 48].reshape(k, 48) ** 2
    top2 = np.sort(fr, axis=1)[:, -2:].sum(axis=1)
    return top2 / (fr.sum(axis=1) + 1e-30)


def _edge_db(m: np.ndarray) -> float:
    if not len(m):
        return -240.0
    peak = float(np.max(np.abs(m))) + 1e-12
    return 20 * math.log10(max(abs(m[0]), abs(m[-1])) / peak + 1e-12)


def _click_times(m: np.ndarray, thr: float) -> list[float]:
    d2 = np.diff(m, 2) if len(m) > 2 else np.zeros(0)
    e, lvl = frame_db(d2, 48), frame_db(m, 48)
    k = min(len(e), len(lvl))
    ok = k >= 24 and len(d2) >= 48 * k
    return _click_runs(e[:k], lvl[:k], _impulsive(d2, k), thr) if ok else []


def clicks(x: Any, thr: float = 18.0) -> dict:
    """Isolated discontinuities inside the sound."""
    m = _mono(x)
    times = _click_times(m, thr)
    return {"clicks": len(times), "click_times": times[:8], "edge_db": round(_edge_db(m), 1)}


def _click_hits(e: np.ndarray, lvl: np.ndarray, imp: np.ndarray, thr: float) -> np.ndarray:
    k = len(e)
    ep = np.pad(e, 12, mode="edge")
    win = np.lib.stride_tricks.sliding_window_view(ep, 25)[:k]
    pre = np.median(win[:, 2:10], axis=1)
    post = np.median(win[:, 15:23], axis=1)
    far = np.abs(np.arange(k) - int(np.argmax(lvl))) > 3
    return (e - np.maximum(pre, post) > thr) & (np.abs(post - pre) < 8) & (lvl > lvl.max() - 70) & far & (imp > 0.6)


def _run_mids(hit: np.ndarray) -> list[float]:
    """Mid times (s, 1 ms frames) of the runs of True."""
    out, i, k = [], 0, len(hit)
    while i < k:
        j = i
        while hit[i] and j + 1 < k and hit[j + 1]:
            j += 1
        if hit[i]:
            out.append(round(((i + j + 1) / 2) / 1000, 3))
        i = j + 1
    return out


def _click_runs(e: np.ndarray, lvl: np.ndarray, imp: np.ndarray, thr: float) -> list[float]:
    return _run_mids(_click_hits(e, lvl, imp, thr))


# ---------------------------------------------------------------- aliasing

class _Alias:
    """Shared state of the aliasing detector."""

    def __init__(self, nfft: int, hop: int):
        self.nfft, self.hop = nfft, hop
        self.df = SR / nfft
        self.freqs = np.arange(nfft // 2 + 1) * self.df
        self.c0 = 150.0 * 2 ** (np.arange(0, int(96 * math.log2(3000 / 150)) + 1) / 96)
        self.c0 = self.c0[self.c0 < 3000]
        self.H = np.arange(1, int(4000 / 150) + 1)

    def fit(self, s: np.ndarray, pw: np.ndarray) -> tuple[float, float] | None:
        band = (self.freqs >= 60) & (self.freqs < 4000)
        tl = s[band].sum()
        if tl <= 0:
            return None
        best, bi = -1e9, -1
        for i, c in enumerate(self.c0):
            g = c * self.H
            g = g[(g >= 60) & (g < 4000)]
            b = np.unique(np.round(g / self.df).astype(int))
            share = pw[b].sum() / tl - 0.02 * len(b)
            if share > best:
                best, bi = share, i
        if best < 0.7:
            return None
        return self._refine(s, self.c0[bi]), best

    def _refine(self, s: np.ndarray, c0: float) -> float:
        num = den = 0.0
        for h in range(1, int(4000 / c0) + 1):
            got = self._harmonic(s, h, c0)
            if got is not None:
                f, w = got
                num += w * h * f
                den += w * h * h
        return num / den if den > 0 else c0

    def _harmonic(self, s: np.ndarray, h: int, c0: float) -> tuple[float, float] | None:
        k0 = int(round(h * c0 / self.df))
        lo, hi = max(1, k0 - 3), min(len(s) - 2, k0 + 3)
        if hi < lo:
            return None
        k = lo + int(np.argmax(s[lo:hi + 1]))
        return self._peak_hz(s, k), s[k]

    def _peak_hz(self, s: np.ndarray, k: int) -> float:
        a, b, c = np.log(s[k - 1:k + 2] + 1e-30)
        den_p = a - 2 * b + c
        off = 0.5 * (a - c) / den_p if den_p != 0 else 0.0
        return (k + float(np.clip(off, -1, 1))) * self.df


def _alias_band(al: _Alias, r: float, smear: float) -> tuple | None:
    """(harmonics, half widths, top Hz, first bin, last bin) of the judged band, or None."""
    df = al.df
    hs = np.arange(math.ceil(4000 / r), int(20000 / r) + 1)
    hw = 2 + hs * smear
    ok = 2 * hw * df < 0.5 * r
    if len(hs) == 0 or not ok[0]:
        return None
    last = int(np.argmin(ok)) if not ok.all() else len(ok)
    hs, hw = hs[:last], hw[:last]
    top = min(20000.0, (hs[-1] + 0.5) * r)
    return hs, hw, top, int(math.ceil(4000 / df)), int(top / df)


def _band_power(s: np.ndarray, band: tuple | None) -> float | None:
    if band is None:
        return None
    b0, b1 = band[3], band[4]
    th = s[b0:b1 + 1].sum()
    return None if b1 - b0 < 20 or th < 1e-3 * s.sum() else th


def _harm_windows(hs: np.ndarray, hw: np.ndarray, r: float, df: float) -> list[tuple[int, int]]:
    return [(int(round(h * r / df - w)), int(round(h * r / df + w))) for h, w in zip(hs, hw)]


def _alias_frame(al: _Alias, s: np.ndarray, r: float, smear: float, legacy: bool) -> tuple[float, float] | None:
    band = _alias_band(al, r, smear)
    th = _band_power(s, band)
    if th is None:
        return None
    fr = SimpleNamespace(s=s, r=r, smear=smear, df=al.df, th=th, hs=band[0], top=band[2], b0=band[3], b1=band[4])
    fr.win = _harm_windows(band[0], band[1], r, al.df)
    if legacy:
        on = sum(s[max(a, fr.b0):min(b, fr.b1) + 1].sum() for a, b in fr.win)
        res = ((1 - min(1.0, on / th)) * th, th)
    else:
        res = _alias_excess(fr)
    return res


def _alias_lines(r: float, smear: float, top: float, df: float) -> tuple:
    """Alias lines in the band that sit clear of the nearest harmonic: ka, fa, hn, hwn, gap."""
    ka = np.arange(int(SR / 2 / r) + 1, int(4 * SR / r) + 1)
    fa = np.abs(((ka * r + SR / 2) % SR) - SR / 2)
    keep = (fa >= 4000) & (fa <= top)
    ka, fa = ka[keep], fa[keep]
    hn = np.maximum(1, np.round(fa / r))
    hwn = 2 + hn * smear
    gap = np.abs(fa - hn * r) / df
    judge = gap >= hwn + 1
    return ka[judge], fa[judge], hn[judge], hwn[judge], gap[judge]


def _href(fr: SimpleNamespace) -> float:
    return max([fr.s[a:b + 1].max() for (a, b), h in zip(fr.win, fr.hs) if h * fr.r >= 0.5 * fr.top and b >= a]
               or [0.0])


def _line_thr(fr: SimpleNamespace, c: int, hn: float, hwn: float, gap: float) -> float:
    """Level a line must pass: over the local floor and over the leak of the nearest harmonic."""
    hc = int(round(hn * fr.r / fr.df))
    hpk = fr.s[max(0, int(hc - hwn)):int(hc + hwn) + 1].max()
    d = max(2.5, gap - hwn + 2)
    leak = hpk * 10 ** ((-31.5 - 18 * math.log2(d / 2.5)) / 10)
    return 10 ** (ALIAS_PEAK_DB / 10) * max(fr.floor[c], leak)


def _alias_line(fr: SimpleNamespace, f: float, aw: float, hn: float, hwn: float, gap: float) -> tuple:
    """(judged, hit, excess power or None) of one alias line."""
    c = int(round(f / fr.df))
    a, b = max(fr.b0, int(round(c - aw))), min(fr.b1, int(round(c + aw)))
    if b < a:
        return False, False, None
    w = fr.s[a:b + 1]
    hit = _line_thr(fr, c, hn, hwn, gap) < w.max() <= ALIAS_LOUDEST * fr.href
    return True, hit, (max(0.0, w.sum() - fr.floor[c] * len(w)) if hit else None)


def _alias_count(fr: SimpleNamespace, cols: np.ndarray, first: np.ndarray) -> tuple[int, int, float]:
    nf = hits = 0
    excess = 0.0
    for row, fst in zip(cols, first):
        judged, hit, add = _alias_line(fr, *row)
        nf += int(judged and bool(fst))
        hits += int(judged and bool(fst) and bool(hit))
        excess = excess + add if add is not None else excess
    return nf, hits, excess


def _alias_excess(fr: SimpleNamespace) -> tuple[float, float] | None:
    ka, fa, hn, hwn, gap = _alias_lines(fr.r, fr.smear, fr.top, fr.df)
    first = ka < SR / fr.r
    if first.sum() < 4:
        return None
    aw = np.minimum(1 + ka * fr.smear, np.maximum(0.5, gap - hwn - 0.5))
    fr.floor = ndimage.median_filter(fr.s, size=65, mode="nearest")
    fr.href = _href(fr)
    nf, hits, excess = _alias_count(fr, np.column_stack([fa, aw, hn, hwn, gap]), first)
    return min(fr.th, excess) * min(1.0, (hits / max(1, nf)) / ALIAS_FAMILY), fr.th


def _alias_f0(S: np.ndarray, al: _Alias, frames: int) -> dict:
    """Fitted f0 of each active frame with a harmonic fit."""
    lv = 10 * np.log10(S.sum(axis=1) + 1e-30)
    act = np.nonzero(lv > lv.max() - 30)[0][:frames]
    pw = sum(np.roll(S, k, axis=1) for k in range(-2, 3))
    fits = {int(i): al.fit(S[i], pw[i]) for i in act}
    return {i: v[0] for i, v in fits.items() if v is not None}


def _f0_rate(f0: dict, i: int, r: float, hop: int) -> float:
    return max([abs(r - f0[j]) for j in (i - 1, i + 1) if j in f0] or [0.0]) / (hop / SR)


def _alias_sum(al: _Alias, S: np.ndarray, f0: dict, hop: int, legacy: bool) -> tuple[float, float, int]:
    num = den = 0.0
    harm = 0
    for i, r in f0.items():
        smear = 0.5 * _f0_rate(f0, i, r, hop) * (al.nfft / SR) / al.df
        res = _alias_frame(al, S[i], r, smear, legacy)
        if res is not None:
            num += res[0]
            den += res[1]
            harm += 1
    return num, den, harm


def aliasing(x: Any, nfft: int = 4096, hop: int = 1024, frames: int = 64, legacy: bool = False) -> dict:
    """Foldover of moving harmonic partials; inharmonic sounds are not judged."""
    S = power_stft(_mono(x), nfft, hop)
    al = _Alias(nfft, hop)
    tot = S.sum() + 1e-30
    nyq = 10 * math.log10(S[:, al.freqs >= 20000].sum() / tot + 1e-12)
    num, den, harm = _alias_sum(al, S, _alias_f0(S, al, frames), hop, legacy)
    fold = num / den if den > 0 and harm >= 2 else 0.0
    return {"fold": round(fold, 3), "harmonic_frames": harm, "nyq_db": round(nyq, 1)}


# ---------------------------------------------------------------- dc, resonances, evolution

def dc(x: Any) -> dict:
    """DC offset and infrasound share."""
    m = _mono(x)
    peak = float(np.max(np.abs(m))) + 1e-12 if len(m) else 1e-12
    n = sp_fft.next_fast_len(max(len(m), 1), real=True)
    p = np.abs(np.fft.rfft(m, n)) ** 2
    f = np.fft.rfftfreq(n, 1 / SR)
    infra = 10 * math.log10(p[f < 15].sum() / (p.sum() + 1e-30) + 1e-12)
    return {"dc_db": round(20 * math.log10(abs(float(np.mean(m))) / peak + 1e-12), 1), "infra_db": round(infra, 1)}


def _ear(f: np.ndarray) -> np.ndarray:
    w = np.full(len(f), 0.5)
    w[(f >= 1000) & (f < 9000)] = 0.7
    w[(f >= 2000) & (f < 6000)] = 1.0
    return w


def _res_nfft(n: int) -> int:
    return 4096 if n >= 4096 else max(1024, 1 << max(0, int(math.ceil(math.log2(max(n, 1))))))


def _res_grid(m: np.ndarray) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    """1/48-octave grid from 100 Hz, the cell maxima and their prominence."""
    nfft = _res_nfft(len(m))
    L = 10 * np.log10(power_stft(m, nfft, nfft // 4).sum(axis=0) + 1e-30)
    f = np.fft.rfftfreq(nfft, 1 / SR)
    g = 100.0 * 2 ** (np.arange(0, 48 * 8) / 48)
    g = g[g < 16000]
    lg = np.array([_cell_max(L, f, c) for c in g])
    return g, lg, lg - ndimage.median_filter(lg, size=33, mode="nearest")


def _local_max(prom: np.ndarray) -> np.ndarray:
    loc = np.zeros(len(prom), dtype=bool)
    loc[1:-1] = (prom[1:-1] >= prom[:-2]) & (prom[1:-1] >= prom[2:])
    return loc


def resonances(x: Any, top: int = 3) -> dict:
    """The harshest narrow spectral peak."""
    g, lg, prom = _res_grid(_mono(x))
    cand = np.nonzero((lg > lg.max() - 30) & (prom > 0) & _local_max(prom))[0]
    if len(cand) == 0:
        return {"res_db": 0.0, "res_hz": None, "res_peaks": []}
    score = prom[cand] * _ear(g[cand])
    order = cand[np.argsort(-score)]
    return {"res_db": round(float(score.max()), 1), "res_hz": int(round(g[order[0]])),
            "res_peaks": [(int(round(g[i])), round(float(prom[i]), 1)) for i in order[:top]]}


def _cell_max(L: np.ndarray, f: np.ndarray, c: float) -> float:
    sel = (f >= c / 2 ** (1 / 96)) & (f < c * 2 ** (1 / 96))
    if not sel.any():
        sel = np.zeros(len(f), dtype=bool)
        sel[int(np.argmin(np.abs(f - c)))] = True
    return float(L[sel].max())


_OCT = (0, 112, 224, 447, 894, 1789, 3578, 7155, math.inf)


def _oct_power(G: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    P = 10 ** (G / 10)
    Q = np.column_stack([P[:, (CEN >= _OCT[j]) & (CEN < _OCT[j + 1])].sum(axis=1) for j in range(8)]) + 1e-12
    return P, ndimage.uniform_filter1d(Q, 3, axis=0, mode="constant")


def _evo_db(Q: np.ndarray, tot: np.ndarray, act: np.ndarray) -> float | None:
    R = np.maximum(10 * np.log10(Q / tot[:, None]), -40)
    rows = [i for i in range(5, len(Q)) if act[i] and act[i - 5]]
    if len(rows) < 3:
        return None
    sh_all = Q / tot[:, None]
    ch = np.array([math.sqrt(np.sum((s := (sh_all[i] + sh_all[i - 5]) / 2) * (R[i] - R[i - 5]) ** 2) / np.sum(s))
                   for i in rows])
    return float(np.sum(ch * tot[rows]) / np.sum(tot[rows]))


def _cent_spread(P: np.ndarray, act: np.ndarray) -> float:
    cent = np.log2((P[act] * CEN).sum(axis=1) / P[act].sum(axis=1))
    return round(float(np.percentile(cent, 90) - np.percentile(cent, 10)), 2)


def evolution(x: Any, G: np.ndarray | None = None) -> dict:
    """Change of spectral shape over 50 ms, independent of level."""
    G = tone.bandgram(_mono(x)) if G is None else G
    P, Q = _oct_power(G)
    tot = Q.sum(axis=1)
    act = 10 * np.log10(tot) > 10 * np.log10(tot.max()) - 30
    evo = _evo_db(Q, tot, act) if act.sum() >= 8 else None
    if evo is None:
        return {"evolution_db": None, "centroid_oct": None}
    return {"evolution_db": round(evo, 2), "centroid_oct": _cent_spread(P, act)}


# ---------------------------------------------------------------- ring

def _log_spec(seg: np.ndarray, N: int) -> tuple[np.ndarray, np.ndarray]:
    w = seg * np.hanning(len(seg))
    p = np.abs(np.fft.rfft(w, N)) ** 2
    return np.fft.rfftfreq(N, 1 / SR), 10 * np.log10(p + 1e-30)


def _cepstrum(L: np.ndarray, f: np.ndarray, df: float, nb: int) -> tuple | None:
    """Cepstrum values and quefrencies (s) between 2 and 40 ms of the 300 Hz - 12 kHz ripple."""
    sel = (f >= 300) & (f <= 12000)
    Ls = np.maximum(L[sel], L[sel].max() - 60)
    res = Ls - ndimage.uniform_filter1d(Ls, nb, mode="nearest")
    cep = np.abs(np.fft.rfft(res * np.hanning(len(res))))
    q = np.arange(len(cep)) / (len(res) * df)
    keep = (q >= 0.002) & (q <= 0.040)
    return (cep[keep], q[keep]) if keep.sum() >= 8 else None


def _robust_z(c: np.ndarray) -> tuple[float, int]:
    i = int(np.argmax(c))
    med = np.median(c)
    return (c[i] - med) / (1.4826 * np.median(np.abs(c - med)) + 1e-12), i


def _bandwidth(L: np.ndarray, df: float, nb: int) -> float:
    smooth_full = ndimage.uniform_filter1d(L, nb, mode="nearest")
    return float(np.sum(smooth_full >= smooth_full.max() - 40) * df)


def _cep_peak(seg: np.ndarray, want_bw: bool = False) -> tuple | None:
    N = 8192 if len(seg) <= 8192 else 16384
    f, L = _log_spec(seg, N)
    df = SR / N
    nb = max(3, int(round(2000 / df)))
    got = _cepstrum(L, f, df, nb)
    if got is None:
        return None
    c, qq = got
    z, i = _robust_z(c)
    out = (float(z), float(qq[i] * 1000))
    return out + (_bandwidth(L, df, nb),) if want_bw else out


def _teeth(seg: np.ndarray, ms: float) -> float:
    f0 = 1000 / ms
    f, L = _log_spec(seg, 16384)
    df = SR / 16384
    lv = []
    for k in range(math.ceil(300 / f0), int(12000 / f0) + 1):
        b = int(round(k * f0 / df))
        lv.append(L[max(0, b - 2):b + 3].max())
    lv = np.array([v for v in lv if v >= L.max() - 50])
    if len(lv) < 8:
        return math.inf
    return float(np.median(np.abs(np.diff(lv, 2))))


def _body_period(m: np.ndarray, ip: int) -> tuple | None:
    """The cepstral peak of the body around the loudest frame, when it is strong."""
    body = m[max(0, (ip - 2) * 480):(ip + 6) * 480]
    cb = _cep_peak(body) if len(body) >= 1920 else None
    return cb if cb is not None and cb[0] >= RING_PITCH_Z else None


def _period_match(ms: float, period: float) -> bool:
    ratio = ms / period
    return abs(ratio - 1) <= 0.3 or any(abs(ratio / k - 1) <= 0.1 for k in (2, 3, 0.5, 1 / 3))


def _is_pitch(m: np.ndarray, ip: int, tailseg: np.ndarray, ms: float) -> bool:
    if _teeth(tailseg, ms) <= RING_SMOOTH_DB:
        return True
    cb = _body_period(m, ip)
    return cb is not None and _period_match(ms, cb[1])


def _tail_seg(m: np.ndarray) -> tuple[int, np.ndarray | None]:
    """The loudest 10 ms frame and the tail from 30 ms after it down to -50 dB (None when short)."""
    e = frame_db(m, 480)
    ip = int(np.argmax(e))
    s = ip + 3
    below = np.nonzero(e[s:] < e[ip] - 50)[0]
    end = s + int(below[0]) if len(below) else len(e) - 1
    return ip, (m[s * 480:(end + 1) * 480][:16384] if end - s >= 6 else None)


def _ring_src(m: np.ndarray, ip: int, seg: np.ndarray, ms: float, bw: float) -> str | None:
    """Why a strong cepstral peak is not a ring: narrow band or pitch; None when it is a ring."""
    narrow = bw < RING_SPAN_HZ
    pitch = not narrow and _is_pitch(m, ip, seg, ms)
    return "narrow" if narrow else "pitch" if pitch else None


def ring(x: Any, legacy: bool = False) -> dict:
    """Comb-like metallic ringing in the tail."""
    m = _mono(x)
    ip, seg = _tail_seg(m)
    cp = _cep_peak(seg, want_bw=True) if seg is not None else None
    if cp is None:
        return {"ring_z": None, "ring_ms": None}
    z, ms, bw = cp
    src = None if legacy or z < 8 else _ring_src(m, ip, seg, ms, bw)
    if src is None:
        return {"ring_z": round(z, 1), "ring_ms": round(ms, 2)}
    return {"ring_z": None, "ring_ms": round(ms, 2), "ring_raw_z": round(z, 1), "ring_src": src}


# ---------------------------------------------------------------- truncated, silence, clipping

def _cut_level(e: np.ndarray, ip: int, end: float) -> float | None:
    before = e[max(ip, len(e) - 38):len(e) - 8]
    slope = _lsq_slope(before, 0.005) if len(before) >= 4 else 0.0
    fade_from = float(np.mean(e[len(e) - 10:len(e) - 7]))
    expect = max(3.0, -slope * 0.04)
    cut = None
    if fade_from - end > 3 * expect + 6 and fade_from > -50:
        cut = fade_from
    elif slope < -20 and end > -50:
        cut = end
    return cut


def truncated(x: Any) -> dict:
    """A tail cut while it is still decaying."""
    e = frame_db(_mono(x), 240)
    if len(e) < 12:
        return {"trunc_db": None, "end_db": None}
    ip = int(np.argmax(e))
    e = e - e[ip]
    end = float(np.mean(e[-2:]))
    cut = None if len(e) - ip - 1 < 12 or end < -55 else _cut_level(e, ip, end)
    return {"trunc_db": _r(cut, 1), "end_db": round(end, 1)}


def silence(x: Any) -> dict:
    """Runs of exact zeros inside the sound."""
    m = np.abs(_mono(x))
    peak = (m.max() if len(m) else 0.0) + 1e-12
    aud = np.nonzero(m > 1e-3 * peak)[0]
    if len(aud) < 2:
        return {"silence_ms": 0.0, "silence_max_ms": 0.0}
    z = m[aud[0]:aud[-1] + 1] < 1e-6
    runs = _run_lengths(z)
    runs = [r for r in runs if r >= 96]
    return {"silence_ms": round(sum(runs) / 48, 1), "silence_max_ms": round(max(runs, default=0) / 48, 1)}


def _run_lengths(flags: np.ndarray) -> list[int]:
    d = np.diff(np.concatenate([[0], flags.astype(int), [0]]))
    return list(np.nonzero(d == -1)[0] - np.nonzero(d == 1)[0])


def clipping(x: Any) -> dict:
    """Flat tops and samples over full scale."""
    a = _raw(x)
    chans = [a] if a.ndim == 1 else [a[:, c] for c in range(a.shape[1])]
    runs = overs = 0
    for c in chans:
        pk = np.max(np.abs(c)) if len(c) else 0.0
        if pk <= 0:
            continue
        overs += int(np.sum(np.abs(c) > 1.0))
        flag = (np.abs(c[1:]) >= 0.99 * pk) & (np.abs(np.diff(c)) < 2e-6)
        runs += sum(1 for r in _run_lengths(flag) if r >= 2)
    return {"clip_runs": runs, "overs": overs}


# ---------------------------------------------------------------- polish measures

def _attack_idx(m: np.ndarray, n: int) -> tuple[int, int]:
    """The 10 % and 90 % frames (0.25 ms) of the onset."""
    pw = np.mean(m[: n * 12].reshape(n, 12) ** 2, axis=1)
    env = np.sqrt(np.convolve(pw, np.ones(2) / 2, mode="same"))
    i90 = int(np.argmax(env >= 0.9 * env.max()))
    low = np.nonzero(env[: i90 + 1] < 0.1 * env.max())[0]
    return (int(low[-1]) if len(low) else 0), i90


def _onset_hf(seg: np.ndarray) -> float | None:
    if len(seg) < 32:
        return None
    p = np.abs(np.fft.rfft(seg * np.hanning(len(seg)), 1024)) ** 2
    f = np.fft.rfftfreq(1024, 1 / SR)
    return 10 * math.log10(p[f >= 6000].sum() / (p.sum() + 1e-30) + 1e-12)


def attack(x: Any) -> dict:
    """10 %-90 % attack time and the high-frequency share of the onset."""
    m = _mono(x)
    n = len(m) // 12
    if n < 4:
        return {"attack_ms": None, "onset_hf_db": None}
    i10, i90 = _attack_idx(m, n)
    hf = _onset_hf(m[12 * i10:12 * i10 + 480])
    return {"attack_ms": round((i90 - i10) * 0.25, 2), "onset_hf_db": _r(hf, 1)}


def _tail_curve(m: np.ndarray, k: int) -> np.ndarray:
    """10 ms levels from the loudest frame on, re that frame."""
    pw = np.mean(m[: k * 480].reshape(k, 480) ** 2, axis=1)
    e = 10 * np.log10(np.convolve(pw, np.ones(3) / 3, mode="same") + 1e-24)
    ip = int(np.argmax(e))
    return e[ip:] - e[ip]


def _bump(after: np.ndarray) -> float:
    live = after > -45
    live[:2] = False
    rise = after - np.minimum.accumulate(after)
    return float(rise[live].max()) if live.any() else 0.0


def _decay(after: np.ndarray) -> float | None:
    fit = (after >= -45) & (after <= -6)
    if fit.sum() < 3:
        return None
    t = np.nonzero(fit)[0] * 0.01
    return float(np.polyfit(t, after[fit], 1)[0])


def tail(x: Any) -> dict:
    """Tail bump, decay rate and t40."""
    m = _mono(x)
    k = len(m) // 480
    if k < 4:
        return {"bump_db": 0.0, "decay_db_s": None, "t40_ms": None}
    after = _tail_curve(m, k)
    under = np.nonzero(after < -40)[0]
    t40 = 10.0 * (int(under[0]) if len(under) else len(after))
    return {"bump_db": round(_bump(after), 1), "decay_db_s": _r(_decay(after), 1), "t40_ms": t40}


def body(x: Any) -> dict:
    """Crest factor of the loudest 50 ms, K-weighted."""
    m = _mono(x)
    km = _k_weight(m)
    sq = km ** 2
    if len(sq) >= 2400:
        loud = float(np.max(np.convolve(sq, np.ones(2400) / 2400, mode="valid")))
    else:
        loud = float(sq.sum() / 2400)
    peak = float(np.max(np.abs(m))) if len(m) else 0.0
    return {"crest_db": round(20 * math.log10(peak + 1e-12) - 10 * math.log10(loud + 1e-24), 1)}


def lufs_max(x: Any) -> float:
    """Momentary maximum loudness (400 ms) in LUFS."""
    a = _raw(x)
    chans = [a] if a.ndim == 1 else [a[:, c] for c in range(a.shape[1])]
    sq = sum(_k_weight(c) ** 2 for c in chans)
    if len(sq) >= 19200:
        v = float(np.max(np.convolve(sq, np.ones(19200) / 19200, mode="valid")))
    else:
        v = float(np.sum(sq) / 19200)
    return -0.691 + 10 * math.log10(v + 1e-24)


def loudness_spread(xs: Sequence[Any]) -> dict:
    """Loudness of a set of variants: values, range and standard deviation."""
    v = [lufs_max(x) for x in xs]
    if not v:
        return {"lufs": [], "spread_lu": None, "std_lu": None}
    return {"lufs": [round(u, 2) for u in v], "spread_lu": round(max(v) - min(v), 2),
            "std_lu": round(float(np.std(v)), 2)}


def measure(x: Any, G: np.ndarray | None = None) -> dict:
    """Every detector and polish measure of one sound."""
    a = _raw(x)
    out: dict = {}
    for fn in (clicks, aliasing, dc, resonances, ring, truncated, silence, clipping, attack, tail, body):
        out.update(fn(a))
    out.update(evolution(a, G))
    return out


# ---------------------------------------------------------------- PolishRef

def _low_penalty(v: float, rv: list, margin: float, cap: float, kind: str) -> tuple[float, float | None, str]:
    """Penalty of a measure where low is bad, against the refs' median."""
    if len(rv) < 2 or np.median(rv) < 1.5:
        return 0.0, None, kind
    limit = margin * float(np.median(rv))
    return min(cap, max(0.0, 2 * (limit - v) / max(limit, 1e-6))), limit, kind


def _designed(key: str, v: float, pen: float, limit: float, kind: str, gated: bool) -> tuple[float, float, str]:
    """Designed sounds: clicks and silence count half; a tail cut is judged against the gate level."""
    scale, cap = RULES[key][3], RULES[key][5]
    if key in ("clicks", "silence_ms"):
        kind, pen = "soft", pen / 2
    elif key == "trunc_db":
        limit = max(limit, GATE_DB["gated"] if gated else GATE_DB["designed"])
        pen = min(cap, max(0.0, (v - limit) / scale))
        kind = "soft" if gated else kind
    return pen, limit, kind


def _art_penalty(key: str, v: float, rv: list, designed: bool, gated: bool) -> tuple[float, float | None, str]:
    bad, ab, margin, scale, kind, cap = RULES[key]
    if bad < 0:
        return _low_penalty(v, rv, margin, cap, kind)
    limit = max(ab, max(rv) + margin) if rv else ab
    pen = min(cap, max(0.0, (v - limit) / scale))
    if designed:
        pen, limit, kind = _designed(key, v, pen, limit, kind, gated)
    return pen, limit, kind


def _log_penalty(v: float, rv: list, bad: float, margin: float, scale: float) -> tuple[float, float]:
    if bad > 0:
        limit = margin * max(rv) + 1
        return math.log2(max(v, 1e-3) / limit) / scale, limit
    limit = margin * min(rv)
    return math.log2(limit / max(v, 1e-3)) / scale, limit


def _lin_penalty(v: float, rv: list, bad: float, margin: float, scale: float) -> tuple[float, float]:
    mg = max(margin, 0.25 * (max(rv) - min(rv)))
    if bad > 0:
        limit = max(rv) + mg
        return (v - limit) / scale, limit
    limit = min(rv) - mg
    return (limit - v) / scale, limit


def _polish_penalty(key: str, v: float, rv: list) -> tuple[float, float] | None:
    bad, kind, margin, scale = POLISH[key]
    if key == "bump_db":
        limit = max(4.0, max(rv) + margin) if rv else 4.0
        res = ((v - limit) / scale, limit)
    elif len(rv) < 2:
        res = None
    elif kind == "log":
        res = _log_penalty(v, rv, bad, margin, scale)
    else:
        res = _lin_penalty(v, rv, bad, margin, scale)
    return res


class PolishRef:
    """Artifact and polish penalties of a sound against real references."""

    def __init__(self, refs: Sequence[Any] = (), designed: bool = False, gated: bool = False,
                 targets: dict | None = None):
        self.M = [measure(r) for r in refs]
        self.n = len(self.M)
        self.designed = designed
        self.gated = gated
        self.targets = dict(targets or {})

    def _rv(self, key: str, keep: list[int]) -> list:
        return [self.M[i][key] for i in keep if self.M[i].get(key) is not None]

    def check(self, x: Any, exclude: Any = None, G: np.ndarray | None = None, m: dict | None = None) -> dict:
        """Penalties, limits, hard faults and words for one sound."""
        drop = set() if exclude is None else ({exclude} if np.isscalar(exclude) else set(exclude))
        keep = [i for i in range(self.n) if i not in drop]
        m = measure(x, G) if m is None else m
        c = {"measures": m, "limits": {}, "art": {}, "polish": {}, "hard": []}
        self._artifacts(c, m, keep)
        self._polish(c, m, keep)
        c["art_pen"] = round(sum(c["art"].values()), 3)
        c["polish_pen"] = round(sum(c["polish"].values()), 3)
        c["words"] = self.words(c, keep)
        return c

    def _artifacts(self, c: dict, m: dict, keep: list[int]) -> None:
        for key in RULES:
            v = m.get(key)
            if v is None:
                continue
            pen, limit, kind = _art_penalty(key, float(v), self._rv(key, keep), self.designed, self.gated)
            if limit is None:
                continue
            c["limits"][key] = round(limit, 2)
            if pen > 0:
                c["art"][key] = round(pen, 3)
            if kind == "hard" and pen >= HARD_AT:
                c["hard"].append(key)

    def _polish(self, c: dict, m: dict, keep: list[int]) -> None:
        for key in POLISH:
            v = m.get(key)
            if v is None:
                continue
            rv = self._rv(key, keep)
            if key != "bump_db" and len(rv) < 2 and key in self.targets:
                rv = list(self.targets[key])
            res = _polish_penalty(key, float(v), rv)
            if res is None:
                continue
            p, limit = res
            c["limits"][key] = round(limit, 2)
            if p > 0:
                c["polish"][key] = round(min(2.0, p), 3)

    def words(self, c: dict, keep: list[int] | None = None) -> str:
        """Plain text of the flagged artifacts and polish items."""
        keep = list(range(self.n)) if keep is None else keep
        m = c["measures"]
        art = [_word(k, m, self._rv(k, keep)) for k in sorted(c["art"], key=lambda k: -c["art"][k])]
        pol = [_word(k, m, self._rv(k, keep)) for k in sorted(c["polish"], key=lambda k: -c["polish"][k])]
        parts = []
        if art:
            parts.append("artifacts: " + "; ".join(art))
        if pol:
            parts.append("polish: " + "; ".join(pol))
        return "; ".join(parts) if parts else "clean (no artifact or polish flag)"


def _rng_txt(rv: list, d: int | None = None) -> str:
    if not rv:
        return ""
    lo, hi = min(rv), max(rv)
    return f" (refs {lo:.{d}f}-{hi:.{d}f})" if d is not None else f" (refs {lo}-{hi})"


_WORDS = {
    "clicks": lambda v, m, rv: f"{v} click(s) ({', '.join(f'{t:.3f} s' for t in m.get('click_times', [])[:3])})",
    "edge_db": lambda v, m, rv: f"starts / ends off zero ({v} dB re peak)",
    "fold": lambda v, m, rv: f"aliasing: {v * 100:.0f}% of the moving partials fold back",
    "nyq_db": lambda v, m, rv: f"energy above 20 kHz {v} dB" + _rng_txt(rv),
    "dc_db": lambda v, m, rv: f"DC offset {v} dB re peak",
    "infra_db": lambda v, m, rv: f"infrasound below 15 Hz {v} dB" + _rng_txt(rv),
    "res_db": lambda v, m, rv: f"narrow resonance at {m.get('res_hz')} Hz +{v} dB" + _rng_txt(rv),
    "ring_z": lambda v, m, rv: f"metallic comb ring every {m.get('ring_ms') or 0:.1f} ms (z {v})" + _rng_txt(rv),
    "trunc_db": lambda v, m, rv: f"tail cut at {v} dB",
    "silence_ms": lambda v, m, rv: f"{v} ms of digital silence inside",
    "clip_runs": lambda v, m, rv: f"clipped ({v} flat tops)",
    "evolution_db": lambda v, m, rv: f"frozen spectrum ({v:.1f} dB of change, refs median "
                                     f"{float(np.median(rv)) if rv else 0:.1f})",
    "attack_ms": lambda v, m, rv: f"attack {v} ms" + _rng_txt(rv, 1),
    "onset_hf_db": lambda v, m, rv: f"dull onset (>6 kHz share {v} dB" + _rng_txt(rv) + ")",
    "bump_db": lambda v, m, rv: f"bumpy tail (+{v} dB re-rise)",
    "t40_ms": lambda v, m, rv: f"tail dies fast (t40 {v} ms" + _rng_txt(rv) + ")",
    "crest_db": lambda v, m, rv: f"thin body (crest {v:.1f} dB" + _rng_txt(rv, 1) + ")",
}


def _word(key: str, m: dict, rv: list) -> str:
    return _WORDS[key](m.get(key), m, rv)


def _files(paths: Sequence[str]) -> list[str]:
    exts = (".wav", ".flac", ".aif", ".aiff", ".ogg", ".mp3")
    out = []
    for p in paths:
        if os.path.isdir(p):
            out += sorted(os.path.join(d, f) for d, _, fs in os.walk(p) for f in fs if f.lower().endswith(exts))
        else:
            out.append(p)
    return out


def main(argv: Sequence[str] | None = None) -> int:
    """Print the detector values of each audio file."""
    for path in _files(list(sys.argv[1:] if argv is None else argv)):
        m = measure(path)
        print(os.path.basename(path), " ".join(f"{k}={m.get(k)}" for k in PRINT_KEYS))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
