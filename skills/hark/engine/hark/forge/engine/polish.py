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
from typing import Any

import numpy as np
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


def clicks(x: Any, thr: float = 18.0) -> dict:
    """Isolated discontinuities inside the sound."""
    m = _mono(x)
    peak = float(np.max(np.abs(m))) + 1e-12 if len(m) else 1e-12
    edge = 20 * math.log10(max(abs(m[0]), abs(m[-1])) / peak + 1e-12) if len(m) else -240.0
    d2 = np.diff(m, 2) if len(m) > 2 else np.zeros(0)
    e, lvl = frame_db(d2, 48), frame_db(m, 48)
    k = min(len(e), len(lvl))
    times: list[float] = []
    if k >= 24 and len(d2) >= 48 * k:
        times = _click_runs(e[:k], lvl[:k], _impulsive(d2, k), thr)
    return {"clicks": len(times), "click_times": times[:8], "edge_db": round(edge, 1)}


def _click_runs(e: np.ndarray, lvl: np.ndarray, imp: np.ndarray, thr: float) -> list[float]:
    k = len(e)
    ep = np.pad(e, 12, mode="edge")
    win = np.lib.stride_tricks.sliding_window_view(ep, 25)[:k]
    pre = np.median(win[:, 2:10], axis=1)
    post = np.median(win[:, 15:23], axis=1)
    loud = int(np.argmax(lvl))
    far = np.abs(np.arange(k) - loud) > 3
    hit = (e - np.maximum(pre, post) > thr) & (np.abs(post - pre) < 8) & (lvl > lvl.max() - 70) & far & (imp > 0.6)
    out, i = [], 0
    while i < k:
        if hit[i]:
            j = i
            while j + 1 < k and hit[j + 1]:
                j += 1
            out.append(round(((i + j + 1) / 2) / 1000, 3))
            i = j + 1
        else:
            i += 1
    return out


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
            k0 = int(round(h * c0 / self.df))
            lo, hi = max(1, k0 - 3), min(len(s) - 2, k0 + 3)
            if hi < lo:
                continue
            k = lo + int(np.argmax(s[lo:hi + 1]))
            a, b, c = np.log(s[k - 1:k + 2] + 1e-30)
            den_p = a - 2 * b + c
            off = 0.5 * (a - c) / den_p if den_p != 0 else 0.0
            f, w = (k + float(np.clip(off, -1, 1))) * self.df, s[k]
            num += w * h * f
            den += w * h * h
        return num / den if den > 0 else c0


def _alias_frame(al: _Alias, s: np.ndarray, r: float, smear: float, legacy: bool) -> tuple[float, float] | None:
    df = al.df
    hs = np.arange(math.ceil(4000 / r), int(20000 / r) + 1)
    hw = 2 + hs * smear
    ok = 2 * hw * df < 0.5 * r
    if len(hs) == 0 or not ok[0]:
        return None
    last = int(np.argmin(ok)) if not ok.all() else len(ok)
    hs, hw = hs[:last], hw[:last]
    top = min(20000.0, (hs[-1] + 0.5) * r)
    b0, b1 = int(math.ceil(4000 / df)), int(top / df)
    th = s[b0:b1 + 1].sum()
    if b1 - b0 < 20 or th < 1e-3 * s.sum():
        return None
    win = [(int(round(h * r / df - w)), int(round(h * r / df + w))) for h, w in zip(hs, hw)]
    if legacy:
        on = sum(s[max(a, b0):min(b, b1) + 1].sum() for a, b in win)
        return (1 - min(1.0, on / th)) * th, th
    return _alias_excess(s, r, smear, top, th, (b0, b1), win, hs, df)


def _alias_excess(s: np.ndarray, r: float, smear: float, top: float, th: float, band: tuple, win: list,
                  hs: np.ndarray, df: float) -> tuple[float, float] | None:
    ka = np.arange(int(SR / 2 / r) + 1, int(4 * SR / r) + 1)
    fa = np.abs(((ka * r + SR / 2) % SR) - SR / 2)
    keep = (fa >= 4000) & (fa <= top)
    ka, fa = ka[keep], fa[keep]
    hn = np.maximum(1, np.round(fa / r))
    hwn = 2 + hn * smear
    gap = np.abs(fa - hn * r) / df
    judge = gap >= hwn + 1
    ka, fa, hn, hwn, gap = ka[judge], fa[judge], hn[judge], hwn[judge], gap[judge]
    first = ka < SR / r
    if first.sum() < 4:
        return None
    aw = np.minimum(1 + ka * smear, np.maximum(0.5, gap - hwn - 0.5))
    b0, b1 = band
    floor = ndimage.median_filter(s, size=65, mode="nearest")
    href = max([s[a:b + 1].max() for (a, b), h in zip(win, hs) if h * r >= 0.5 * top and b >= a] or [0.0])
    nf = hits = 0
    excess = 0.0
    for j in range(len(fa)):
        c = int(round(fa[j] / df))
        a, b = max(b0, int(round(c - aw[j]))), min(b1, int(round(c + aw[j])))
        if b < a:
            continue
        w = s[a:b + 1]
        hc = int(round(hn[j] * r / df))
        hpk = s[max(0, int(hc - hwn[j])):int(hc + hwn[j]) + 1].max()
        d = max(2.5, gap[j] - hwn[j] + 2)
        leak = hpk * 10 ** ((-31.5 - 18 * math.log2(d / 2.5)) / 10)
        thr = 10 ** (ALIAS_PEAK_DB / 10) * max(floor[c], leak)
        hit = thr < w.max() <= ALIAS_LOUDEST * href
        if first[j]:
            nf += 1
            hits += int(hit)
        if hit:
            excess += max(0.0, w.sum() - floor[c] * len(w))
    return min(th, excess) * min(1.0, (hits / max(1, nf)) / ALIAS_FAMILY), th


def aliasing(x: Any, nfft: int = 4096, hop: int = 1024, frames: int = 64, legacy: bool = False) -> dict:
    """Foldover of moving harmonic partials; inharmonic sounds are not judged."""
    m = _mono(x)
    S = power_stft(m, nfft, hop)
    al = _Alias(nfft, hop)
    tot = S.sum() + 1e-30
    nyq = 10 * math.log10(S[:, al.freqs >= 20000].sum() / tot + 1e-12)
    lv = 10 * np.log10(S.sum(axis=1) + 1e-30)
    act = np.nonzero(lv > lv.max() - 30)[0][:frames]
    pw = sum(np.roll(S, k, axis=1) for k in range(-2, 3))
    fits = {int(i): al.fit(S[i], pw[i]) for i in act}
    f0 = {i: v[0] for i, v in fits.items() if v is not None}
    num = den = 0.0
    harm = 0
    for i, r in f0.items():
        rate = max([abs(r - f0[j]) for j in (i - 1, i + 1) if j in f0] or [0.0]) / (hop / SR)
        smear = 0.5 * rate * (nfft / SR) / al.df
        res = _alias_frame(al, S[i], r, smear, legacy)
        if res is None:
            continue
        num += res[0]
        den += res[1]
        harm += 1
    fold = num / den if den > 0 and harm >= 2 else 0.0
    return {"fold": round(fold, 3), "harmonic_frames": harm, "nyq_db": round(nyq, 1)}


# ---------------------------------------------------------------- dc, resonances, evolution

def dc(x: Any) -> dict:
    """DC offset and infrasound share."""
    m = _mono(x)
    peak = float(np.max(np.abs(m))) + 1e-12 if len(m) else 1e-12
    n = signal.next_fast_len(max(len(m), 1), real=True)
    p = np.abs(np.fft.rfft(m, n)) ** 2
    f = np.fft.rfftfreq(n, 1 / SR)
    infra = 10 * math.log10(p[f < 15].sum() / (p.sum() + 1e-30) + 1e-12)
    return {"dc_db": round(20 * math.log10(abs(float(np.mean(m))) / peak + 1e-12), 1), "infra_db": round(infra, 1)}


def _ear(f: np.ndarray) -> np.ndarray:
    w = np.full(len(f), 0.5)
    w[(f >= 1000) & (f < 9000)] = 0.7
    w[(f >= 2000) & (f < 6000)] = 1.0
    return w


def resonances(x: Any, top: int = 3) -> dict:
    """The harshest narrow spectral peak."""
    m = _mono(x)
    nfft = 4096 if len(m) >= 4096 else max(1024, 1 << max(0, int(math.ceil(math.log2(max(len(m), 1))))))
    L = 10 * np.log10(power_stft(m, nfft, nfft // 4).sum(axis=0) + 1e-30)
    f = np.fft.rfftfreq(nfft, 1 / SR)
    g = 100.0 * 2 ** (np.arange(0, 48 * 8) / 48)
    g = g[g < 16000]
    lg = np.array([_cell_max(L, f, c) for c in g])
    prom = lg - ndimage.median_filter(lg, size=33, mode="nearest")
    loc = np.zeros(len(g), dtype=bool)
    loc[1:-1] = (prom[1:-1] >= prom[:-2]) & (prom[1:-1] >= prom[2:])
    cand = np.nonzero((lg > lg.max() - 30) & (prom > 0) & loc)[0]
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


def evolution(x: Any, G: np.ndarray | None = None) -> dict:
    """Change of spectral shape over 50 ms, independent of level."""
    G = tone.bandgram(_mono(x)) if G is None else G
    P = 10 ** (G / 10)
    Q = np.column_stack([P[:, (CEN >= _OCT[j]) & (CEN < _OCT[j + 1])].sum(axis=1) for j in range(8)]) + 1e-12
    Q = ndimage.uniform_filter1d(Q, 3, axis=0, mode="constant")
    tot = Q.sum(axis=1)
    act = 10 * np.log10(tot) > 10 * np.log10(tot.max()) - 30
    none = {"evolution_db": None, "centroid_oct": None}
    if act.sum() < 8:
        return none
    R = np.maximum(10 * np.log10(Q / tot[:, None]), -40)
    rows = [i for i in range(5, len(Q)) if act[i] and act[i - 5]]
    if len(rows) < 3:
        return none
    sh_all = Q / tot[:, None]
    ch = np.array([math.sqrt(np.sum((s := (sh_all[i] + sh_all[i - 5]) / 2) * (R[i] - R[i - 5]) ** 2) / np.sum(s))
                   for i in rows])
    evo = float(np.sum(ch * tot[rows]) / np.sum(tot[rows]))
    cent = np.log2((P[act] * CEN).sum(axis=1) / P[act].sum(axis=1))
    return {"evolution_db": round(evo, 2), "centroid_oct": round(float(np.percentile(cent, 90) -
                                                                         np.percentile(cent, 10)), 2)}


# ---------------------------------------------------------------- ring

def _log_spec(seg: np.ndarray, N: int) -> tuple[np.ndarray, np.ndarray]:
    w = seg * np.hanning(len(seg))
    p = np.abs(np.fft.rfft(w, N)) ** 2
    return np.fft.rfftfreq(N, 1 / SR), 10 * np.log10(p + 1e-30)


def _cep_peak(seg: np.ndarray, want_bw: bool = False) -> tuple | None:
    N = 8192 if len(seg) <= 8192 else 16384
    f, L = _log_spec(seg, N)
    df = SR / N
    nb = max(3, int(round(2000 / df)))
    smooth_full = ndimage.uniform_filter1d(L, nb, mode="nearest")
    sel = (f >= 300) & (f <= 12000)
    Ls = np.maximum(L[sel], L[sel].max() - 60)
    res = Ls - ndimage.uniform_filter1d(Ls, nb, mode="nearest")
    cep = np.abs(np.fft.rfft(res * np.hanning(len(res))))
    q = np.arange(len(cep)) / (len(res) * df)
    keep = (q >= 0.002) & (q <= 0.040)
    if keep.sum() < 8:
        return None
    c, qq = cep[keep], q[keep]
    i = int(np.argmax(c))
    med = np.median(c)
    z = (c[i] - med) / (1.4826 * np.median(np.abs(c - med)) + 1e-12)
    if not want_bw:
        return float(z), float(qq[i] * 1000)
    bw = float(np.sum(smooth_full >= smooth_full.max() - 40) * df)
    return float(z), float(qq[i] * 1000), bw


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


def _is_pitch(m: np.ndarray, ip: int, tailseg: np.ndarray, ms: float) -> bool:
    if _teeth(tailseg, ms) <= RING_SMOOTH_DB:
        return True
    body = m[max(0, (ip - 2) * 480):(ip + 6) * 480]
    if len(body) < 1920:
        return False
    cb = _cep_peak(body)
    if cb is None or cb[0] < RING_PITCH_Z:
        return False
    ratio = ms / cb[1]
    return abs(ratio - 1) <= 0.3 or any(abs(ratio / k - 1) <= 0.1 for k in (2, 3, 0.5, 1 / 3))


def ring(x: Any, legacy: bool = False) -> dict:
    """Comb-like metallic ringing in the tail."""
    m = _mono(x)
    e = frame_db(m, 480)
    ip = int(np.argmax(e))
    s = ip + 3
    below = np.nonzero(e[s:] < e[ip] - 50)[0]
    end = s + int(below[0]) if len(below) else len(e) - 1
    if end - s < 6:
        return {"ring_z": None, "ring_ms": None}
    seg = m[s * 480:(end + 1) * 480][:16384]
    cp = _cep_peak(seg, want_bw=True)
    if cp is None:
        return {"ring_z": None, "ring_ms": None}
    z, ms, bw = cp
    out = {"ring_z": round(z, 1), "ring_ms": round(ms, 2)}
    if legacy or z < 8:
        return out
    if bw < RING_SPAN_HZ:
        return {"ring_z": None, "ring_ms": round(ms, 2), "ring_raw_z": round(z, 1), "ring_src": "narrow"}
    if _is_pitch(m, ip, seg, ms):
        return {"ring_z": None, "ring_ms": round(ms, 2), "ring_raw_z": round(z, 1), "ring_src": "pitch"}
    return out


# ---------------------------------------------------------------- truncated, silence, clipping

def truncated(x: Any) -> dict:
    """A tail cut while it is still decaying."""
    e = frame_db(_mono(x), 240)
    if len(e) < 12:
        return {"trunc_db": None, "end_db": None}
    ip = int(np.argmax(e))
    e = e - e[ip]
    end = float(np.mean(e[-2:]))
    if len(e) - ip - 1 < 12 or end < -55:
        return {"trunc_db": None, "end_db": round(end, 1)}
    before = e[max(ip, len(e) - 38):len(e) - 8]
    slope = _lsq_slope(before, 0.005) if len(before) >= 4 else 0.0
    fade_from = float(np.mean(e[len(e) - 10:len(e) - 7]))
    expect = max(3.0, -slope * 0.04)
    cut = None
    if fade_from - end > 3 * expect + 6 and fade_from > -50:
        cut = fade_from
    elif slope < -20 and end > -50:
        cut = end
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

def attack(x: Any) -> dict:
    """10 %-90 % attack time and the high-frequency share of the onset."""
    m = _mono(x)
    n = len(m) // 12
    if n < 4:
        return {"attack_ms": None, "onset_hf_db": None}
    pw = np.mean(m[: n * 12].reshape(n, 12) ** 2, axis=1)
    env = np.sqrt(np.convolve(pw, np.ones(2) / 2, mode="same"))
    i90 = int(np.argmax(env >= 0.9 * env.max()))
    low = np.nonzero(env[: i90 + 1] < 0.1 * env.max())[0]
    i10 = int(low[-1]) if len(low) else 0
    seg = m[12 * i10:12 * i10 + 480]
    hf = None
    if len(seg) >= 32:
        p = np.abs(np.fft.rfft(seg * np.hanning(len(seg)), 1024)) ** 2
        f = np.fft.rfftfreq(1024, 1 / SR)
        hf = 10 * math.log10(p[f >= 6000].sum() / (p.sum() + 1e-30) + 1e-12)
    return {"attack_ms": round((i90 - i10) * 0.25, 2), "onset_hf_db": _r(hf, 1)}


def tail(x: Any) -> dict:
    """Tail bump, decay rate and t40."""
    m = _mono(x)
    k = len(m) // 480
    if k < 4:
        return {"bump_db": 0.0, "decay_db_s": None, "t40_ms": None}
    pw = np.mean(m[: k * 480].reshape(k, 480) ** 2, axis=1)
    e = 10 * np.log10(np.convolve(pw, np.ones(3) / 3, mode="same") + 1e-24)
    ip = int(np.argmax(e))
    after = e[ip:] - e[ip]
    under = np.nonzero(after < -40)[0]
    t40 = 10.0 * (int(under[0]) if len(under) else len(after))
    live = after > -45
    live[:2] = False
    rise = after - np.minimum.accumulate(after)
    bump = float(rise[live].max()) if live.any() else 0.0
    fit = (after >= -45) & (after <= -6)
    dec = None
    if fit.sum() >= 3:
        t = np.nonzero(fit)[0] * 0.01
        dec = float(np.polyfit(t, after[fit], 1)[0])
    return {"bump_db": round(bump, 1), "decay_db_s": _r(dec, 1), "t40_ms": t40}


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

def _art_penalty(key: str, v: float, rv: list, designed: bool, gated: bool) -> tuple[float, float | None, str]:
    bad, ab, margin, scale, kind, cap = RULES[key]
    if bad < 0:
        if len(rv) < 2 or np.median(rv) < 1.5:
            return 0.0, None, kind
        limit = margin * float(np.median(rv))
        return min(cap, max(0.0, 2 * (limit - v) / max(limit, 1e-6))), limit, kind
    limit = max(ab, max(rv) + margin) if rv else ab
    pen = min(cap, max(0.0, (v - limit) / scale))
    if designed and key in ("clicks", "silence_ms"):
        kind, pen = "soft", pen / 2
    if designed and key == "trunc_db":
        limit = max(limit, GATE_DB["gated"] if gated else GATE_DB["designed"])
        pen = min(cap, max(0.0, (v - limit) / scale))
        kind = "soft" if gated else kind
    return pen, limit, kind


def _polish_penalty(key: str, v: float, rv: list) -> tuple[float, float] | None:
    bad, kind, margin, scale = POLISH[key]
    if key == "bump_db":
        limit = max(4.0, max(rv) + margin) if rv else 4.0
        return (v - limit) / scale, limit
    if len(rv) < 2:
        return None
    if kind == "log":
        if bad > 0:
            limit = margin * max(rv) + 1
            return math.log2(max(v, 1e-3) / limit) / scale, limit
        limit = margin * min(rv)
        return math.log2(limit / max(v, 1e-3)) / scale, limit
    mg = max(margin, 0.25 * (max(rv) - min(rv)))
    if bad > 0:
        limit = max(rv) + mg
        return (v - limit) / scale, limit
    limit = min(rv) - mg
    return (limit - v) / scale, limit


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


def _word(key: str, m: dict, rv: list) -> str:
    v = m.get(key)
    texts = {
        "clicks": lambda: f"{v} click(s) ({', '.join(f'{t:.3f} s' for t in m.get('click_times', [])[:3])})",
        "edge_db": lambda: f"starts / ends off zero ({v} dB re peak)",
        "fold": lambda: f"aliasing: {v * 100:.0f}% of the moving partials fold back",
        "nyq_db": lambda: f"energy above 20 kHz {v} dB" + _rng_txt(rv),
        "dc_db": lambda: f"DC offset {v} dB re peak",
        "infra_db": lambda: f"infrasound below 15 Hz {v} dB" + _rng_txt(rv),
        "res_db": lambda: f"narrow resonance at {m.get('res_hz')} Hz +{v} dB" + _rng_txt(rv),
        "ring_z": lambda: f"metallic comb ring every {m.get('ring_ms') or 0:.1f} ms (z {v})" + _rng_txt(rv),
        "trunc_db": lambda: f"tail cut at {v} dB",
        "silence_ms": lambda: f"{v} ms of digital silence inside",
        "clip_runs": lambda: f"clipped ({v} flat tops)",
        "evolution_db": lambda: f"frozen spectrum ({v:.1f} dB of change, refs median "
                                f"{float(np.median(rv)) if rv else 0:.1f})",
        "attack_ms": lambda: f"attack {v} ms" + _rng_txt(rv, 1),
        "onset_hf_db": lambda: f"dull onset (>6 kHz share {v} dB" + _rng_txt(rv) + ")",
        "bump_db": lambda: f"bumpy tail (+{v} dB re-rise)",
        "t40_ms": lambda: f"tail dies fast (t40 {v} ms" + _rng_txt(rv) + ")",
        "crest_db": lambda: f"thin body (crest {v:.1f} dB" + _rng_txt(rv, 1) + ")",
    }
    return texts[key]()


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
