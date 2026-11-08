"""Spectral fitter: a time-varying 48-band noise gesture with stereo, texture and tonal ridges.

Every render draws fresh noise. The band axis is equally spaced on ln(1 + f/80).
"""

from __future__ import annotations

import copy
import functools
import time
from typing import Any

import numpy as np
from scipy.fft import irfft, next_fast_len, rfft, rfftfreq
from scipy.ndimage import convolve1d, median_filter, uniform_filter, uniform_filter1d

from hark.fit.analysis._common import (SR, as_stereo, check_kind, db, is_mono_pair,
                                       load_json, mel_db_distance, resample, save_json,
                                       source_name, threads)

KIND = "spectral_noise"
NAME = "spectral"
VERSION = 1

KNOBS: dict[str, tuple] = {
    "length": ((0.8, 1.25, "log"), 1.0, "Time-stretch."),
    "speed": ((0.8, 1.25, "log"), 1.0, "Shorter and higher above 1."),
    "sweep": ((0.7, 1.43, "log"), 1.0, "Factor on the centroid excursion."),
    "shift": ((-0.4, 0.4), 0.0, "Octaves."),
    "tilt": ((-2.5, 2.5), 0.0, "dB per octave about 1 kHz."),
    "roughness": ((0.5, 1.5), 1.0, "Texture depth."),
    "width": ((0.5, 1.5), 1.0, "Side and pan amount."),
    "ridges": ((0.5, 1.5), 1.0, "Tonal ridge gain."),
}

RENDER_KNOBS = ("length", "speed", "sweep", "shift", "tilt", "roughness", "width", "ridges",
                "gain_db", "doppler", "proximity", "pan", "motion", "level", "limit", "steady")

DETAIL = {"gesture_ms": 10, "tol_db": 0.5, "max_keys": 600, "denoise": False, "adapt": False,
          "max_ridges": 100, "ridge_db": 5, "ridge_ms": 20, "ridge_lo": 60}

HOP = 240
N_BANDS = 48
F_LO, F_HI = 20.0, 20000.0
GROUPS = (0.0, 300.0, 1000.0, 2500.0, 5000.0, 10000.0, 24000.0)
RATES = tuple(2.0 ** (k / 2) for k in range(19))
GRAIN_GRID = (25, 60, 150, 400, 1000, 2500, 6000, 15000)
WHITE_SEED = 12345

__all__ = ["KIND", "NAME", "VERSION", "KNOBS", "DETAIL", "fit", "render", "check", "summary",
           "save", "load", "trajectory", "bend", "pass_pan", "analyse", "centers", "warp", "unwarp",
           "weights", "bandwidths", "n_frames", "compare", "fit_report", "to_code"]


# ================================================================ band axis

def warp(f: Any) -> np.ndarray:
    """The band axis: ln(1 + f / 80)."""
    return np.log1p(np.asarray(f, dtype=float) / 80.0)


def unwarp(e: Any) -> np.ndarray:
    """Hz of a warp value."""
    return 80.0 * np.expm1(np.asarray(e, dtype=float))


def centers(n: int = N_BANDS) -> np.ndarray:
    """Band centres, equally spaced on the warp from 20 Hz to 20 kHz."""
    return unwarp(np.linspace(warp(F_LO), warp(F_HI), n))


def weights(freqs: Any, cents: Any) -> np.ndarray:
    """Raised-cosine band weights (B, F) on the warp axis."""
    e = warp(freqs)
    ec = warp(cents)
    W = np.zeros((len(ec), len(e)))
    for b in range(len(ec)):
        _fill_weight(W[b], e, ec, b)
    return W


def _fill_weight(row: np.ndarray, e: np.ndarray, ec: np.ndarray, b: int) -> None:
    lo = ec[b - 1] if b > 0 else ec[0] - (ec[1] - ec[0])
    rise = (e >= lo) & (e <= ec[b])
    row[rise] = np.sin(0.5 * np.pi * (e[rise] - lo) / (ec[b] - lo)) ** 2
    if b == len(ec) - 1:
        row[e > ec[b]] = 1.0
    else:
        fall = (e > ec[b]) & (e < ec[b + 1])
        row[fall] = np.cos(0.5 * np.pi * (e[fall] - ec[b]) / (ec[b + 1] - ec[b])) ** 2


@functools.lru_cache(maxsize=8)
def _bandwidths(key: tuple) -> np.ndarray:
    f = np.linspace(0.0, SR / 2, 96001)
    return np.trapezoid(weights(f, np.asarray(key)), f, axis=1)


def bandwidths(cents: Any) -> np.ndarray:
    """Integral of each band weight over 0 .. 24 kHz, Hz."""
    return _bandwidths(tuple(float(c) for c in np.asarray(cents)))


def _band_sizes(cents) -> np.ndarray:
    bw = bandwidths(cents)
    return np.clip(2 ** np.round(np.log2(2 * SR / bw)), 512, 4096).astype(int)


def n_frames(n: int) -> int:
    """Frame count of the 5 ms analysis grid."""
    return int(n) // HOP + 1


def _group_of(cents) -> np.ndarray:
    return np.clip(np.searchsorted(GROUPS, cents, side="right") - 1, 0, len(GROUPS) - 2)


def _group_centres() -> np.ndarray:
    g = np.array([np.sqrt(max(lo, 50.0) * hi) for lo, hi in zip(GROUPS[:-1], GROUPS[1:])])
    g[0] = 150.0
    return g


def _group_blend(cents) -> np.ndarray:
    """(B, G) weights that interpolate a group parameter to each band in log2 frequency."""
    lg = np.log2(_group_centres())
    eye = np.eye(len(lg))
    lc = np.log2(np.asarray(cents, dtype=float))
    return np.array([[np.interp(v, lg, eye[g]) for g in range(len(lg))] for v in lc])


# ================================================================ analysis

def _hann(N: int) -> np.ndarray:
    w = 0.5 - 0.5 * np.cos(2 * np.pi * np.arange(N) / N)
    return w / np.sqrt(np.sum(w ** 2))


def _frames_fft(x: np.ndarray, N: int, T: int) -> np.ndarray:
    xp = np.concatenate([np.zeros(N // 2), x, np.zeros(N + HOP * T)])
    fr = np.lib.stride_tricks.sliding_window_view(xp, N)[::HOP][:T]
    return rfft(fr * _hann(N), axis=1, workers=threads())


def analyse(x: Any, cents: Any = None, stereo: bool = True) -> dict:
    """Band power density (B, T), group stereo powers or None, and the frame count."""
    a = x if (isinstance(x, np.ndarray) and x.ndim == 1) else as_stereo(x)
    cents = centers() if cents is None else np.asarray(cents)
    T = n_frames(len(a))
    D = _density(a, cents, T)
    st = _stereo_powers(a, T) if (a.ndim == 2 and stereo and not is_mono_pair(a)) else None
    return {"D": D, "stereo": st, "T": T}


def _density(a: np.ndarray, cents: np.ndarray, T: int) -> np.ndarray:
    chans = [a[:, 0], a[:, 1]] if a.ndim == 2 else [a]
    sizes = _band_sizes(cents)
    D = np.zeros((len(cents), T))
    for N in np.unique(sizes):
        sel = np.flatnonzero(sizes == N)
        D[sel] = _size_density(chans, N, T, cents, sel)
    return D


def _size_density(chans: list, N: int, T: int, cents: np.ndarray, sel: np.ndarray) -> np.ndarray:
    P = np.mean([np.abs(_frames_fft(c, N, T)) ** 2 for c in chans], axis=0)
    W = weights(rfftfreq(N, 1 / SR), cents)
    Wn = W[sel] / np.maximum(W[sel].sum(axis=1, keepdims=True), 1e-30)
    return Wn @ P.T


def _stereo_powers(a: np.ndarray, T: int) -> dict:
    XL, XR = _frames_fft(a[:, 0], 1024, T), _frames_fft(a[:, 1], 1024, T)
    f = rfftfreq(1024, 1 / SR)
    out = {"L": [], "R": [], "C": []}
    for lo, hi in zip(GROUPS[:-1], GROUPS[1:]):
        s = (f >= lo) & (f < hi)
        out["L"].append(np.sum(np.abs(XL[:, s]) ** 2, axis=1))
        out["R"].append(np.sum(np.abs(XR[:, s]) ** 2, axis=1))
        out["C"].append(np.sum(np.real(XL[:, s] * np.conj(XR[:, s])), axis=1))
    return {k: np.array(v) for k, v in out.items()}


# ================================================================ ridges

def _ridge_floor(P_db: np.ndarray) -> np.ndarray:
    med = median_filter(P_db, size=(8, 1), mode="nearest")
    lo = np.roll(med, 6, axis=0)
    hi = np.roll(med, -6, axis=0)
    lo[:6] = med[:6]
    hi[-6:] = med[-6:]
    return np.maximum(lo, hi) + 1.6


def _frame_peaks(col, floor, f, lo_bin, hi_bin, ridge_db) -> list:
    ex = col - floor
    cand = []
    for i in range(max(lo_bin, 2), min(hi_bin, len(col) - 2)):
        if ex[i] > ridge_db and col[i] == col[i - 2:i + 3].max():
            cand.append(i)
    cand = sorted(cand, key=lambda i: -ex[i])[:12]
    out = []
    for i in cand:
        a, b, c = col[i - 1], col[i], col[i + 1]
        curv = a - 2 * b + c
        d = 0.5 * (a - c) / curv if curv < 0 else 0.0
        out.append((f[i] + d * (f[1] - f[0]), b - 0.25 * (a - c) * d, ex[i], i))
    return out


def _track_ridges(Pdb, floor, active, f, opts) -> list:
    lo_bin = int(np.searchsorted(f, opts["ridge_lo"]))
    hi_bin = int(np.searchsorted(f, 16000.0))
    live, done = [], []
    for k in range(Pdb.shape[1]):
        peaks = _frame_peaks(Pdb[:, k], floor[:, k], f, lo_bin, hi_bin, opts["ridge_db"]) if active[k] else []
        _step_ridges(live, peaks, k, floor)
        done += [tr for tr in live if tr["miss"] > 2]
        live = [tr for tr in live if tr["miss"] <= 2]
    return done + live


def _step_ridges(live: list, peaks: list, k: int, floor) -> None:
    """Extend each live ridge with its nearest free peak. Free peaks start new ridges."""
    used = set()
    for tr in live:
        best = _nearest_peak(tr, peaks, used)
        if best is None:
            tr["miss"] += 1
        else:
            used.add(best[1])
            _extend_ridge(tr, peaks[best[1]], k, floor)
    for j, p in enumerate(peaks):
        if j not in used:
            live.append({"k": [], "f": [], "db": [], "ex": [], "miss": 0})
            _extend_ridge(live[-1], p, k, floor)


def _nearest_peak(tr: dict, peaks: list, used: set):
    best = None
    for j in (j for j in range(len(peaks)) if j not in used):
        d = abs(np.log2(peaks[j][0] / tr["f"][-1]))
        if d < 0.04 and (best is None or d < best[0]):
            best = (d, j)
    return best


def _extend_ridge(tr, p, k, floor) -> None:
    tr["k"].append(k)
    tr["f"].append(p[0])
    tr["db"].append(p[1])
    tr["ex"].append(p[2])
    tr["fl"] = tr.get("fl", []) + [floor[p[3], k]]
    tr["miss"] = 0


def _ridges(m: np.ndarray, opts: dict) -> list:
    T = n_frames(len(m))
    X = _frames_fft(m, 2048, T)
    P = np.abs(X.T) ** 2
    Pdb = db(P)
    floor = _ridge_floor(Pdb)
    tot = db(P.sum(axis=0))
    active = tot >= tot.max() - 50
    f = rfftfreq(2048, 1 / SR)
    tracks = _track_ridges(Pdb, floor, active, f, opts)
    min_len = max(2, round(opts["ridge_ms"] / 5))
    good = [t for t in tracks if len(t["k"]) >= min_len and np.median(t["ex"]) >= opts["ridge_db"] + 2]
    good.sort(key=lambda t: -float(np.sum(10 ** (np.array(t["db"]) / 10) - 10 ** (np.array(t["fl"]) / 10))))
    return [_ridge_model(t) for t in good[:opts["max_ridges"]]]


def _ridge_model(tr: dict) -> dict:
    t = np.array(tr["k"]) * HOP / SR
    f = np.array(tr["f"])
    adb = _ridge_db(tr)
    keep = _greedy_keys(t, np.column_stack([np.log2(f) / 0.006, adb / 1.5]), None, 1.0, 60, [])
    return {"t": [round(float(t[i]), 4) for i in keep], "f": [round(float(f[i]), 1) for i in keep],
            "db": [round(float(adb[i]), 1) for i in keep], "jitter": _jitter(f)}


def _ridge_db(tr: dict) -> np.ndarray:
    """Sine amplitude (dB) of each ridge point above its floor."""
    p = np.maximum(10 ** (np.array(tr["db"]) / 10) - 10 ** (np.array(tr["fl"]) / 10), 1e-30)
    return 20 * np.log10(np.sqrt(6 * p / 2048))


def _jitter(f: np.ndarray) -> float:
    lf = np.log2(f)
    sm = uniform_filter1d(lf, 5, mode="nearest")
    return round(float(np.clip(np.std(lf - sm) * np.log(2), 0.0005, 0.02)), 5)


def _remove_ridges(D: np.ndarray, ridges: list, cents) -> None:
    bw = bandwidths(cents)
    tg = np.arange(D.shape[1]) * HOP / SR
    for r in ridges:
        rt = np.asarray(r["t"])
        inside = (tg >= rt[0]) & (tg <= rt[-1])
        if not inside.any():
            continue
        f = np.interp(tg[inside], rt, r["f"])
        A2 = 10 ** (np.interp(tg[inside], rt, r["db"]) / 10)
        Wf = weights(f, cents)
        sub = Wf * A2 * SR / (4 * bw[:, None])
        D[:, inside] = np.maximum(D[:, inside] - sub, 0.05 * D[:, inside])


# ================================================================ gesture smoothing

def _onset_frame(D: np.ndarray) -> int:
    tot = db(D.sum(axis=0))
    return int(np.argmax(tot >= tot.max() - 30))


def _smooth(D: np.ndarray, k: int, on: int) -> np.ndarray:
    if k < 1:
        return D.copy()
    w = np.hanning(k + 2)[1:-1]
    w = w / w.sum()
    out = D.copy()
    if on > 0:
        out[:, :on] = convolve1d(D[:, :on], w, axis=1, mode="nearest")
    out[:, on:] = convolve1d(D[:, on:], w, axis=1, mode="nearest")
    return out


def _wander(S: np.ndarray, k: int, on: int) -> float | None:
    from scipy.signal import savgol_filter
    tot = db(S.sum(axis=0))
    T = len(tot)
    win = min(T if T % 2 else T - 1, 4 * k + 1)
    if win < 5:
        return None
    fit = savgol_filter(tot, win, 2, mode="nearest")
    sel = (tot >= tot.max() - 30) & (np.abs(np.arange(T) - on) > k)
    if sel.sum() <= 4:
        return None
    return float(np.sqrt(np.mean((tot - fit)[sel] ** 2)))


def _gesture(D: np.ndarray, opts: dict) -> tuple[np.ndarray, int, float]:
    k = max(0, round(opts["gesture_ms"] / 5))
    on = _onset_frame(D)
    wander = 0.0
    if opts["adapt"]:
        k, wander = _adapt_k(D, k, on, opts["tol_db"])
    S = db(_smooth(D, k, on))
    S = np.maximum(S, S.max() - 90)
    return S, k, wander


def _adapt_k(D: np.ndarray, k: int, on: int, tol_db: float) -> tuple[int, float]:
    """Widen the smoothing until the level wander is within 2 tol_db."""
    k_max = max(k, min(32, D.shape[1] // 6))
    while True:
        w = _wander(_smooth(D, k, on), max(k, 1), on)
        wander = w or 0.0
        if w is None or w <= 2 * tol_db or k >= k_max:
            break
        k = min(k_max, int(np.ceil(1.5 * max(k, 1))))
    return k, wander


# ================================================================ denoise

def _cell_weights(S: np.ndarray, bw: np.ndarray, span: float = 50.0) -> np.ndarray:
    G = S + 10 * np.log10(bw)[:, None]
    return np.clip((G - (G.max() - span)) / 25, 0.0, 1.0)


def _noise_level(n: int, k: int, cents, Wc: np.ndarray) -> float:
    w = np.random.default_rng(WHITE_SEED).standard_normal(n)
    D = analyse(w, cents, False)["D"]
    S = db(_smooth(D, k, 0))
    sizes = _band_sizes(cents)
    sd = np.zeros(len(cents))
    for b in range(len(cents)):
        skip = int(sizes[b] // 2 // HOP)
        row = S[b, skip:S.shape[1] - skip] if S.shape[1] - 2 * skip > 8 else S[b]
        sd[b] = np.std(row)
    wb = Wc.mean(axis=1)
    return float(np.sqrt(np.sum(wb * sd ** 2) / max(wb.sum(), 1e-12)))


def _low_rank(S: np.ndarray, W: np.ndarray, K: int) -> np.ndarray:
    est = S.copy()
    for _ in range(10):
        Z = W * S + (1 - W) * est
        mu = Z.mean(axis=1, keepdims=True)
        U, s, Vt = np.linalg.svd(Z - mu, full_matrices=False)
        est = mu + (U[:, :K] * s[:K]) @ Vt[:K]
    return est


def _denoise(S: np.ndarray, n: int, k: int, cents) -> tuple[np.ndarray, int]:
    bw = bandwidths(cents)
    W = _cell_weights(S, bw)
    noise = _noise_level(n, k, cents, W)
    best, rank = S, 0
    for K in range(1, min(16, min(S.shape)) + 1):
        est = _low_rank(S, W, K)
        best, rank = est, K
        if np.sqrt(np.sum(W * (est - S) ** 2) / max(W.sum(), 1e-12)) <= noise:
            break
    out = W * best + (1 - W) * S
    return np.maximum(out, S.max() - 90), rank


# ================================================================ synthesis core

def _gens(seed: int) -> list:
    ss = np.random.SeedSequence(abs(int(seed)) % 2 ** 32)
    return [np.random.default_rng(s) for s in ss.spawn(5)]


def _clicks(n: int, rate: float, spread: float, norm_ms: float, r) -> np.ndarray:
    x = np.zeros(n)
    count = r.poisson(max(rate, 0.0) * n / SR)
    pos = r.integers(0, n, count)
    amp = np.exp(spread * r.standard_normal(count)) * np.where(r.uniform(size=count) < 0.5, -1, 1)
    np.add.at(x, pos, amp)
    x = _even_clicks(x, rate, norm_ms)
    rms = np.sqrt(np.mean(x ** 2))
    return x / rms if rms > 0 else x


def _even_clicks(x: np.ndarray, rate: float, norm_ms: float) -> np.ndarray:
    """Level the click train over a window of norm_ms (at least 20 clicks)."""
    win = int(max(norm_ms, 20 * 1000.0 / rate) * SR / 1000) | 1 if norm_ms > 0 and rate > 0 else len(x)
    if win < len(x) / 2:
        p = uniform_filter1d(x ** 2, win, mode="nearest")
        x = x * uniform_filter1d(1 / np.sqrt(np.maximum(p, 1e-3 * p.mean())), win, mode="nearest")
    return x


def _gp(m: int, rates, spec, r, unit_hi: float | None = None) -> np.ndarray:
    """A unit-std Gaussian process at 1 kHz with a rate spectrum (variance per half-octave band)."""
    nfft = next_fast_len(max(2 * m, 64))
    f = rfftfreq(nfft, 1e-3)
    if unit_hi is not None:
        dens = (f <= unit_hi).astype(float)
    else:
        rates = np.asarray(rates, dtype=float)
        bwr = rates * (2 ** 0.25 - 2 ** -0.25)
        d = np.asarray(spec, dtype=float) / bwr
        dens = np.where(f >= rates[0] * 2 ** -0.25,
                        np.interp(np.log2(np.maximum(f, 1e-9)), np.log2(rates), d), 0.0)
    z = irfft(rfft(r.standard_normal(nfft)) * np.sqrt(dens), nfft)[:m]
    s = np.std(z)
    return z / s if s > 0 else z


def _modulation(n: int, am: dict | None, rate_scale: float, roughness: float, r, Gb) -> np.ndarray | None:
    if not am or roughness <= 0 or max(am["depth"]) <= 0:
        return None
    m = n // 48 + 2
    rates = np.asarray(am["rates"]) * rate_scale
    zc = _gp(m, rates, am["spec"], r)
    coh = float(am["coh"])
    mods = []
    for d in am["depth"]:
        zg = _gp(m, rates, am["spec"], r)
        z = np.sqrt(coh) * zc + np.sqrt(1 - coh) * zg
        sg = d * roughness
        mods.append(np.exp(sg * z - sg ** 2))
    tc = np.arange(m) * 48
    G = np.array([np.interp(np.arange(n), tc, mo) for mo in mods])
    return Gb @ G


def _stereo_ctrl(st: dict, kt, n: int, Gb) -> tuple:
    icc = np.clip(np.asarray(st["icc"], dtype=float), -1, 1)
    ild = np.asarray(st["ild"], dtype=float)
    th = 0.5 * np.arccos(icc)
    r = 10 ** (ild / 10)
    par = [np.cos(th), np.sin(th), np.sqrt(2 * r / (1 + r)), np.sqrt(2 / (1 + r))]
    t = np.arange(n) / SR
    return tuple(Gb @ np.array([np.interp(t, kt, p[:, g]) for g in range(p.shape[1])]) for p in par)


def _fine_gain(fine: dict | None, shift: float, f: np.ndarray) -> np.ndarray | float:
    if not fine:
        return 1.0
    grid = np.arange(len(fine["db"])) * fine["step"]
    pos = np.log2(np.maximum(f, 1e-9) / 2 ** shift / fine["f0"])
    g = np.where((pos >= grid[0]) & (pos <= grid[-1]), np.interp(pos, grid, fine["db"]), 0.0)
    return 10 ** (g / 20)


def _excitation(n, nfft, tex, roughness, rate_scale, r) -> np.ndarray:
    gr = (tex or {}).get("grains")
    if gr and roughness > 0:
        x = _clicks(nfft, gr["density"] / roughness ** 2 * rate_scale, gr.get("spread", 1.0),
                    gr.get("norm_ms", 0.0), r)
    else:
        x = r.standard_normal(nfft)
    return rfft(x)


def _synth(n, kt, kdb, cents, seed, stereo=None, texture=None, ridges=None, mono=True,
           rate_scale=1.0, roughness=1.0, fine=None, fine_shift=0.0, power=False, ridge_sig=None):
    """The synthesis core: band noise on keyframes, with optional stereo, texture and ridges."""
    gA, gB, gC, gM, gR = _gens(seed)
    tex = texture or {}
    two = not mono and stereo is not None
    nfft = next_fast_len(max(n, 64))
    W, SA, SB = _spectra(n, nfft, cents, tex, (gA, gB, gC), two, (fine, fine_shift), (roughness, rate_scale))
    Gb = _group_blend(cents)
    mod = _modulation(n, tex.get("am"), rate_scale, roughness, gM, Gb)
    kdb = np.asarray(kdb, dtype=float)
    ctrl = _stereo_ctrl(stereo, kt, n, Gb) if two else None
    L, R = _band_mix(n, kt, kdb, SA, SB, W, nfft, mod, ctrl)
    _ridge_layer(L, R, ridges, ridge_sig, gR, ctrl, cents)
    y = L if mono else np.column_stack([L, R if two else L])
    return (y, _expected(n, kt, kdb, cents, nfft, W, ridges)) if power else y


def _spectra(n, nfft, cents, tex, gens, two, fine_args, rough_args):
    """Band weights and the excitation spectra: A always, B only for stereo."""
    gA, gB, gC = gens
    roughness, rate_scale = rough_args
    f = rfftfreq(nfft, 1 / SR)
    W = weights(f, cents)
    fg = _fine_gain(*fine_args, f)
    SA = _excitation(n, nfft, tex, roughness, rate_scale, gC if tex.get("grains") else gA) * fg
    SB = rfft(gB.standard_normal(nfft)) * fg if two else None
    return W, SA, SB


def _ridge_layer(L, R, ridges, ridge_sig, r, ctrl, cents) -> None:
    rs = ridge_sig if ridge_sig is not None else _ridge_wave(len(L), ridges, r)
    own = rs is not None and R is not None and ridge_sig is None
    _add_ridge_sig(L, R, rs, _ridge_gains(ridges, ctrl, cents) if own else (1.0, 1.0))


def _add_ridge_sig(L: np.ndarray, R, rs, gains) -> None:
    if rs is not None:
        L += rs * gains[0]
        if R is not None:
            R += rs * gains[1]


def _band_mix(n, kt, kdb, SA, SB, W, nfft, mod, ctrl):
    """Sum of the band noises: L only, or L and R from 2 excitations and the stereo control."""
    t = np.arange(n) / SR
    L = np.zeros(n)
    R = np.zeros(n) if ctrl is not None else None
    for b in range(kdb.shape[1]):
        amp = 10 ** (np.interp(t, kt, kdb[:, b]) / 20)
        if mod is not None:
            amp = amp * mod[b]
        nA = irfft(SA * W[b], nfft)[:n]
        if ctrl is None:
            L += amp * nA
            continue
        nB = irfft(SB * W[b], nfft)[:n]
        c, s, gl, gr = (p[b] for p in ctrl)
        L += amp * gl * (c * nA + s * nB)
        R += amp * gr * (c * nA - s * nB)
    return L, R


def _ridge_wave(n: int, ridges: list | None, r) -> np.ndarray | None:
    if not ridges:
        return None
    t = np.arange(n) / SR
    out = np.zeros(n)
    for rd in ridges:
        _add_ridge(out, t, rd, r)
    return out


def _add_ridge(out: np.ndarray, t: np.ndarray, rd: dict, r) -> None:
    """Add one jittered ridge sine. The random draws happen even when the ridge is outside t."""
    rt = np.asarray(rd["t"], dtype=float)
    inside = (t >= rt[0]) & (t <= rt[-1])
    z = _gp(len(t) // 48 + 2, None, None, r, unit_hi=16.0)
    start = r.uniform(0, 2 * np.pi)
    if inside.any():
        zi = np.interp(t[inside], np.arange(len(z)) * 48 / SR, z)
        f = np.minimum(np.interp(t[inside], rt, rd["f"]) * (1 + rd["jitter"] * zi), 0.45 * SR)
        ph = start + 2 * np.pi * np.cumsum(f) / SR
        a = 10 ** (np.interp(t[inside], rt, rd["db"]) / 20) * _edge_fade(int(inside.sum()))
        out[inside] += a * np.cos(ph)


def _edge_fade(m: int) -> np.ndarray:
    k = min(int(0.005 * SR), m // 2)
    w = np.ones(m)
    if k > 0:
        ramp = np.arange(k) / k
        w[:k] = ramp
        w[m - k:] = ramp[::-1]
    return w


def _ridge_gains(ridges, ctrl, cents):
    g = _group_of(np.array([np.median(r["f"]) for r in ridges]))
    b = np.array([int(np.argmin(np.abs(cents - c))) for c in _group_centres()[g]])
    return np.mean(ctrl[2][b], axis=0), np.mean(ctrl[3][b], axis=0)


@functools.lru_cache(maxsize=4)
def _kgain2(nfft: int) -> np.ndarray:
    from hark.core.features import k_weight
    imp = np.zeros(nfft)
    imp[0] = 1.0
    return np.abs(rfft(np.asarray(k_weight(imp, SR), dtype=float))) ** 2


def _expected(n, kt, kdb, cents, nfft, W, ridges) -> dict:
    """Expected power on the 5 ms grid, plain and K-weighted (channel mean)."""
    tg = np.arange(n_frames(n)) * HOP / SR
    amp = np.array([10 ** (np.interp(tg, kt, kdb[:, b]) / 20) for b in range(len(cents))])
    k2 = _kgain2(nfft)
    out = {}
    for key, g in (("p", 1.0), ("k", k2)):
        self_w = 2 * np.sum(W ** 2 * g, axis=1) / nfft
        over = 2 * 2 * np.sum(W[1:] * W[:-1] * g, axis=1) / nfft
        P = (amp ** 2 * self_w[:, None]).sum(axis=0) + (amp[1:] * amp[:-1] * over[:, None]).sum(axis=0)
        out[key] = P + _ridge_power(tg, ridges, k2 if key == "k" else None, nfft)
    return out


def _ridge_power(tg, ridges, k2, nfft) -> np.ndarray:
    P = np.zeros(len(tg))
    for rd in ridges or []:
        rt = np.asarray(rd["t"], dtype=float)
        inside = (tg >= rt[0]) & (tg <= rt[-1])
        g = 1.0
        if k2 is not None:
            g = k2[min(int(round(np.median(rd["f"]) * nfft / SR)), len(k2) - 1)]
        P[inside] += 0.5 * 10 ** (np.interp(tg[inside], rt, rd["db"]) / 10) * g
    return P


# ================================================================ gain correction

def _gain_iter(S, k, on, n, cents, active, opts) -> np.ndarray:
    gain = S.copy()
    top = S.max()
    kt = np.arange(S.shape[1]) * HOP / SR
    for it in range(opts["iters"]):
        got = []
        for sd in opts["seeds"]:
            y = _synth(n, kt, gain.T, cents, 7919 * (it + 1) + sd)
            got.append(db(_smooth(analyse(y, cents, False)["D"], k, on)))
        diff = np.where(active, S - np.mean(got, axis=0), 0.0)
        diff = uniform_filter(diff, size=3, mode="nearest")
        gain = np.maximum(gain + 0.85 * np.clip(diff, -12, 12), top - 90)
    return gain


# ================================================================ stereo trajectory

def _stereo_traj(st: dict | None) -> dict | None:
    if st is None:
        return None
    PL, PR, C = (uniform_filter1d(st[k], 7, axis=1, mode="nearest") for k in ("L", "R", "C"))
    ild = np.clip(db(PL) - db(PR), -20, 20)
    icc = np.clip(C / np.sqrt(np.maximum(PL * PR, 1e-300)), -1, 1)
    Pg = PL + PR
    _fill_quiet(ild, icc, Pg)
    share = Pg.sum(axis=1) / max(Pg.sum(), 1e-300)
    return {"ild": ild, "icc": icc, "share": share, "level": db(Pg.sum(axis=0))}


def _fill_quiet(ild: np.ndarray, icc: np.ndarray, Pg: np.ndarray) -> None:
    """Quiet frames take the power-weighted mean of the group's active frames."""
    act = Pg >= Pg.max() * 10 ** -4.5
    for g in range(len(Pg)):
        w = Pg[g] * act[g]
        if w.sum() <= 0:
            ild[g], icc[g] = 0.0, 1.0
        else:
            ild[g, ~act[g]] = np.sum(w * ild[g]) / w.sum()
            icc[g, ~act[g]] = np.sum(w * icc[g]) / w.sum()


# ================================================================ keyframes

def _greedy_keys(t, Y, W, tol, max_keys, required) -> list[int]:
    T = len(t)
    if T <= 2:
        return list(range(T))
    W = np.ones_like(Y) if W is None else W
    sw = W.sum(axis=1)
    den = np.maximum(sw, 0.25 * sw.max()) if sw.max() > 0 else np.ones(T)
    keys = sorted(set([0, T - 1] + [int(r) for r in required]))
    while len(keys) < max_keys:
        i = _worst_frame(t, Y, W, den, keys, tol)
        if i is None:
            break
        keys = sorted(keys + [i])
    return keys


def _worst_frame(t, Y, W, den, keys, tol) -> int | None:
    """The frame worst fit by interpolation between keys, or None when every error is within tol."""
    ks = np.array(keys)
    Yh = np.array([np.interp(t, t[ks], Y[ks, c]) for c in range(Y.shape[1])]).T
    err = np.sqrt(np.sum(W * (Yh - Y) ** 2, axis=1) / den)
    err[ks] = 0.0
    i = int(np.argmax(err))
    return None if err[i] <= tol else i


def _keyframes(gain, st, bw, opts, wander) -> tuple[list[int], np.ndarray]:
    t = np.arange(gain.shape[1]) * HOP / SR
    rows, Ws = [gain.T], [_cell_weights(gain, bw, opts["quiet_db"]).T]
    if st is not None:
        rows += [st["ild"].T / 2, st["icc"].T * 10]
        Ws += [_stereo_key_weight(st)] * 2
    tot = db(np.sum(10 ** (gain / 10), axis=0))
    pk = int(np.argmax(tot))
    on = int(np.argmax(tot >= tot[pk] - 30))
    return _greedy_keys(t, np.hstack(rows), np.hstack(Ws), max(opts["tol_db"], wander), opts["max_keys"],
                        [pk, on]), t


def _stereo_key_weight(st: dict) -> np.ndarray:
    lev = st["level"]
    lw = np.clip((lev - (lev.max() - 40)) / 20, 0, 1)
    return 0.5 * lw[:, None] * st["share"][None, :] * 7


# ================================================================ fine curve

def _psd_grid(x: np.ndarray, grid: np.ndarray) -> np.ndarray:
    from scipy.signal import welch
    N = min(4096, 2 ** int(np.floor(np.log2(max(len(x), 256)))))
    f, p = welch(x, SR, nperseg=N, noverlap=3 * N // 4, window="hann")
    out = np.zeros(len(grid))
    for i, g in enumerate(grid):
        s = (f >= g * 2 ** (-1 / 48)) & (f <= g * 2 ** (1 / 48))
        out[i] = p[s].mean() if s.any() else p[int(np.argmin(np.abs(f - g)))]
    return out


def _fine(m: np.ndarray, model: dict, cents) -> dict | None:
    grid = 40.0 * 2 ** (np.arange(0, int(24 * np.log2(20000 / 40)) + 1) / 24)
    po = _psd_grid(m, grid)
    pr = np.mean([_psd_grid(_render_mono(model, s), grid) for s in (41, 42)], axis=0)
    d = db(po) - db(pr)
    d = d - uniform_filter1d(d, 9, mode="nearest")
    d = np.clip(d, -12, 12)
    d[db(po) < db(po).max() - 60] = 0.0
    d *= np.clip((grid - 150) / 150, 0, 1)
    if np.sqrt(np.mean(d ** 2)) < 0.4:
        return None
    return {"f0": 40.0, "step": 1 / 24, "db": [round(float(v), 2) for v in d]}


def _render_mono(model: dict, seed: int, texture=True, ridge_sig=None) -> np.ndarray:
    env = model["env"]
    n = max(64, int(round(model["dur"] * SR)))
    return _synth(n, np.asarray(env["t"]), env["db"], centers(model["bands"]["n"]), seed,
                  texture=model.get("texture") if texture else None,
                  ridges=model.get("ridges") if ridge_sig is None else None,
                  fine=model.get("fine"), ridge_sig=ridge_sig)


# ================================================================ texture statistics

def _group_env(model: dict, n: int) -> np.ndarray:
    cents = centers(model["bands"]["n"])
    bw = bandwidths(cents)
    t = np.arange(n // 48 + 1) * 48 / SR
    env = model["env"]
    d = np.array([np.interp(t, env["t"], np.asarray(env["db"])[:, b]) for b in range(len(cents))])
    g = _group_of(cents)
    out = np.zeros((len(GROUPS) - 1, len(t)))
    for k in range(len(out)):
        s = g == k
        if s.any():
            out[k] = np.sqrt(np.sum(10 ** (d[s] / 10) * bw[s, None], axis=0) / (SR / 2))
    return out


def _rate_spectrum(r: np.ndarray, w: np.ndarray) -> np.ndarray:
    mu = np.sum(w * r) / max(w.sum(), 1e-12)
    z = (r - mu) * w
    var = np.sum(w * (r - mu) ** 2) / max(w.sum(), 1e-12)
    P = np.abs(np.fft.rfft(z)) ** 2
    f = np.fft.rfftfreq(len(z), 48 / SR)
    spec = np.array([P[(f >= c * 2 ** -0.25) & (f < c * 2 ** 0.25)].sum() for c in RATES])
    tot = P[1:].sum()
    return spec / tot * var if tot > 0 else spec


def _group_stats(y: np.ndarray, genv: np.ndarray, onset_s: float) -> list:
    n = len(y)
    nfft = next_fast_len(n)
    Y = rfft(y, nfft)
    f = rfftfreq(nfft, 1 / SR)
    out = []
    share = (genv ** 2).sum(axis=1)
    share = share / max(share.sum(), 1e-300)
    for g, (lo, hi) in enumerate(zip(GROUPS[:-1], GROUPS[1:])):
        out.append(_one_group(Y, f, nfft, n, lo, hi, genv[g], onset_s) if share[g] >= 0.004 else None)
    return out


def _one_group(Y, f, nfft, n, lo, hi, env, onset_s) -> dict | None:
    yb = irfft(Y * ((f >= lo) & (f < hi)), nfft)[:n]
    m = min(len(env), n // 48)
    a = np.sqrt(np.mean(yb[:m * 48].reshape(m, 48) ** 2, axis=1))
    e = env[:m]
    t = np.arange(m) * 48 / SR
    act = (e >= e.max() * 10 ** -1.5) & (t > onset_s + 0.015) & (a > 0) & (e > 0)
    return _active_stats(yb, a, e, t, act) if act.sum() >= 24 else None


def _active_stats(yb, a, e, t, act) -> dict:
    r = np.where(act, np.log(np.maximum(a, 1e-30)) - np.log(np.maximum(e, 1e-30)), 0.0)
    w = uniform_filter1d(act.astype(float), 5) * act
    return {"spec": _rate_spectrum(r, w), "halves": _halves(r, w, act), "lenv": _lenv(yb, t, e, act),
            "low": _low_part(r, w) * act, "act": act}


def _halves(r: np.ndarray, w: np.ndarray, act: np.ndarray) -> list:
    m = len(act)
    idx = np.flatnonzero(act)
    half = idx[len(idx) // 2] if len(idx) else 0
    out = []
    for sel in (np.arange(m) < half, np.arange(m) >= half):
        out.append(_rate_spectrum(r, w * sel) if (act & sel).sum() >= 12 else None)
    return out


def _lenv(yb: np.ndarray, t: np.ndarray, e: np.ndarray, act: np.ndarray) -> float:
    """Std of the log Hilbert envelope over the band envelope, on active frames."""
    from scipy.signal import hilbert
    n = len(yb)
    ae = np.abs(hilbert(yb))
    eu = np.interp(np.arange(n), t, e)
    on_s = np.repeat(act, 48)[:n]
    on_s = np.pad(on_s, (0, n - len(on_s)))
    return float(np.std(np.log(np.maximum(ae[on_s], 1e-30)) - np.log(np.maximum(eu[on_s], 1e-30))))


def _low_part(r: np.ndarray, w: np.ndarray) -> np.ndarray:
    return uniform_filter1d(r - np.sum(w * r) / max(w.sum(), 1e-12), 15) if len(r) > 15 else r


def _fast(st: dict) -> float:
    return float(np.sum(st["spec"][np.asarray(RATES) >= 128]))


def _texture_stats(model, y_list, onset_s, n):
    genv = _group_env(model, n)
    return [_group_stats(y, genv, onset_s) for y in y_list]


def _nanmean(vals) -> float:
    v = [x for x in vals if x is not None and np.isfinite(x)]
    return float(np.mean(v)) if v else float("nan")


# ================================================================ texture fit

def _tex_renders(model, texture, ridge_sigs, n) -> list:
    m = dict(model, texture=texture)
    return [_render_mono(m, s, True, rs) for s, rs in zip((31, 32, 33), ridge_sigs)]


def _texture(m: np.ndarray, model: dict, opts: dict) -> dict | None:
    start = _tex_context(m, model, opts)
    return None if start is None else _tex_fit(*start)


def _tex_context(m: np.ndarray, model: dict, opts: dict):
    """(context, plain-noise stats) for the texture fit, or None when no group has stats."""
    n = len(m)
    on = _onset_frame(analyse(m, centers(), False)["D"]) * HOP / SR
    rs = [_ridge_wave(n, model.get("ridges"), _gens(s)[4]) for s in (31, 32, 33)]
    Sr = _texture_stats(model, [m], on, n)[0]
    S0 = _texture_stats(model, _tex_renders(model, None, rs, n), on, n)
    valid = [g for g in range(len(Sr)) if Sr[g] is not None and all(s[g] is not None for s in S0)]
    if not valid:
        return None
    share = (_group_env(model, n) ** 2).sum(axis=1)[valid]
    gw = dict(zip(valid, share / max(share.sum(), 1e-300)))
    return {"m": m, "model": model, "rs": rs, "on": on, "n": n, "gw": gw, "Sr": Sr, "opts": opts}, S0


def _tex_fit(ctx: dict, S0: list) -> dict | None:
    grains, stats = _grain_fit(ctx, S0)
    base = S0 if grains is None else _tex_stats(ctx, {"grains": grains})
    am = _am_fit(ctx, base)
    if am is not None and grains is not None:
        grains, _ = _grain_fit(ctx, S0, am)
        base = _tex_stats(ctx, {"grains": grains})
        am = _am_fit(ctx, base) or am
    if am is not None:
        am = _depth_fix(ctx, am, grains, base)
    return None if grains is None and am is None else {"am": am, "grains": grains, "stats": stats}


def _tex_stats(ctx: dict, texture: dict) -> list:
    return _texture_stats(ctx["model"], _tex_renders(ctx["model"], texture, ctx["rs"], ctx["n"]), ctx["on"], ctx["n"])


def _grain_fit(ctx, S0, am=None):
    stats, grainy = _grain_stats(ctx, S0)
    return (_grain_density(ctx, am) if grainy else None), stats


def _grain_stats(ctx, S0) -> tuple[dict, bool]:
    """Fast-fluctuation stats of the recording against plain noise, and whether it is grainy."""
    fr = _fast_mix(ctx["gw"], ctx["Sr"])
    fg_seeds = [_fast_mix(ctx["gw"], s) for s in S0]
    fg = float(np.mean(fg_seeds))
    hratio = fr / max(fg, 1e-12)
    stats = _lenv_stats(ctx, S0)
    stats.update({"fast_var_real": round(fr, 5), "fast_var_gauss": round(fg, 5), "fast_ratio": round(hratio, 3)})
    halves_ok, stats["fast_ratio_halves"] = _halves_ratio(ctx, S0)
    grainy = fr - fg > max(0.03, 3 * float(np.std(fg_seeds))) and hratio > 1.5
    return stats, bool(grainy and (halves_ok or not ctx["opts"]["grain_halves"]))


def _fast_mix(gw: dict, stats: list) -> float:
    return sum(w * _fast(stats[g]) for g, w in gw.items())


def _lenv_stats(ctx, S0) -> dict:
    gw, Sr = ctx["gw"], ctx["Sr"]
    return {"lenv_real": _nanmean([Sr[g]["lenv"] for g in gw]),
            "lenv_gauss": _nanmean([s[g]["lenv"] for s in S0 for g in gw])}


def _halves_ratio(ctx, S0):
    out = [_half_ratio(ctx, S0, h) for h in (0, 1)]
    return all(v is not None and v > 1.5 for v in out), out


def _half_ratio(ctx, S0, h: int) -> float | None:
    num = sum(w * _hfast(ctx["Sr"][g]["halves"][h]) for g, w in ctx["gw"].items())
    den = sum(w * _nanmean([_hfast(s[g]["halves"][h]) for s in S0]) for g, w in ctx["gw"].items())
    ok = den and np.isfinite(den) and den > 0
    return round(num / den, 3) if ok else None


def _hfast(spec) -> float | None:
    if spec is None:
        return None
    return float(np.sum(spec[np.asarray(RATES) >= 128]))


def _grain_loss(ctx, density, am) -> float:
    g = {"density": float(density), "spread": 1.0}
    if ctx["opts"]["grain_norm_ms"] > 0:
        g["norm_ms"] = ctx["opts"]["grain_norm_ms"]
    S = _texture_stats(ctx["model"], _tex_renders(ctx["model"], {"grains": g, "am": am}, ctx["rs"], ctx["n"]),
                       ctx["on"], ctx["n"])
    loss = 0.0
    for gi, w in ctx["gw"].items():
        got = _nanmean([s[gi]["lenv"] if s[gi] else None for s in S])
        if np.isfinite(got):
            loss += w * (got - ctx["Sr"][gi]["lenv"]) ** 2
    return loss


def _grain_density(ctx, am) -> dict:
    losses = {d: _grain_loss(ctx, d, am) for d in GRAIN_GRID}
    best = min(losses, key=losses.get)
    i = GRAIN_GRID.index(best)
    for j in (i - 1, i + 1):
        if 0 <= j < len(GRAIN_GRID):
            d = float(np.sqrt(best * GRAIN_GRID[j]))
            losses[d] = _grain_loss(ctx, d, am)
    best = min(losses, key=losses.get)
    g = {"density": round(float(best), 1), "spread": 1.0}
    if ctx["opts"]["grain_norm_ms"] > 0:
        g["norm_ms"] = ctx["opts"]["grain_norm_ms"]
    return g


def _am_fit(ctx, base) -> dict | None:
    rates = np.asarray(RATES)
    depth, shape = _am_depths(ctx, base, (rates >= 6) & (rates <= 64))
    big = [d for g, d in enumerate(depth) if ctx["gw"].get(g, 0) > 0.03]
    if not big or max(big) <= 0.08 or shape.sum() <= 0:
        return None
    return {"rates": [float(r) for r in rates], "spec": [round(float(v), 6) for v in shape / shape.sum()],
            "depth": [round(d, 4) for d in depth], "coh": round(_coherence(ctx, base), 3)}


def _am_depths(ctx, base, low) -> tuple[list[float], np.ndarray]:
    """AM depth per group and the weighted shape of the excess rate spectrum."""
    depth, shape = [], np.zeros(len(low))
    for g in range(len(GROUPS) - 1):
        if g in ctx["gw"]:
            ex = _am_excess(ctx["Sr"][g], [b[g] for b in base], low)
            depth.append(float(min(np.sqrt(ex.sum()), 0.6)))
            shape += ctx["gw"][g] * ex
        else:
            depth.append(0.0)
    return depth, shape


def _am_excess(real, bases, low) -> np.ndarray:
    bs = [b for b in bases if b is not None]
    ex = _halves_cap(_spec_excess(real, bs, low), real, bs) if bs else None
    return np.zeros(len(low)) if ex is None else np.maximum(ex, 0.0)


def _spec_excess(real, bs, low) -> np.ndarray:
    sb = np.mean([b["spec"] for b in bs], axis=0)
    sd = np.std([b["spec"] for b in bs], axis=0)
    return np.where((real["spec"] > 1.25 * sb + 2 * sd) & low, real["spec"] - sb, 0.0)


def _halves_cap(ex, real, bs):
    """ex capped by the excess of each half, or None when a half has no stats."""
    for h in (0, 1):
        hr = real["halves"][h]
        hb = [b["halves"][h] for b in bs if b["halves"][h] is not None]
        if hr is None or not hb:
            return None
        ex = np.minimum(ex, np.maximum(hr - 1.25 * np.mean(hb, axis=0), 0.0))
    return ex


def _coherence(ctx, base) -> float:
    top = sorted(ctx["gw"], key=lambda g: -ctx["gw"][g])[:2]
    return 1.0 if len(top) < 2 else _pair_coherence(ctx, base, top)


def _pair_coherence(ctx, base, top) -> float:
    a, b = (ctx["Sr"][g] for g in top)
    both = a["act"] & b["act"]
    la = _low_excess(a, base, top[0])
    lb = _low_excess(b, base, top[1])
    ok = both.sum() >= 24 and np.std(la[both]) != 0 and np.std(lb[both]) != 0
    return float(np.clip(np.corrcoef(la[both], lb[both])[0, 1], 0, 1)) if ok else 0.0


def _low_excess(st: dict, base: list, g: int) -> np.ndarray:
    return st["low"] - _nanmean_arr([bb[g]["low"] for bb in base if bb[g]], len(st["low"]))


def _nanmean_arr(arrs, m) -> np.ndarray:
    return np.mean(arrs, axis=0)[:m] if arrs else np.zeros(m)


def _depth_fix(ctx, am, grains, base) -> dict:
    S = _tex_stats(ctx, {"am": am, "grains": grains})
    low = (np.asarray(RATES) >= 6) & (np.asarray(RATES) <= 64)
    out = dict(am)
    depth = list(am["depth"])
    for g in ctx["gw"]:
        depth[g] = _fixed_depth(ctx["Sr"][g], depth[g], [b[g] for b in base], [s[g] for s in S], low)
    out["depth"] = depth
    return out


def _fixed_depth(real: dict, d: float, bases: list, rendered: list, low: np.ndarray) -> float:
    """Depth scaled so the rendered low-rate variance matches the recording's."""
    bvar = _nanmean([np.sum(b["spec"][low]) for b in bases if b])
    want = np.sum(real["spec"][low]) - bvar
    got = max(_nanmean([np.sum(s["spec"][low]) for s in rendered if s]) - bvar, 1e-6)
    if np.isfinite(want) and np.isfinite(got):
        d = round(float(min(d * np.sqrt(np.clip(want / got, 0.25, 4)), 0.6)), 4)
    return d


# ================================================================ fit

def _opts(**kw) -> dict:
    o = {"n_bands": 48, "tol_db": 1.0, "max_keys": 160, "gesture_ms": 25, "max_rank": 16, "iters": 3,
         "diff_frames": 3, "ridges": True, "fine": False, "texture": True, "stereo": True,
         "seeds": (1, 2), "max_ridges": 4, "ridge_db": 8, "ridge_ms": 50, "ridge_lo": 400,
         "denoise": True, "adapt": True, "quiet_db": 50, "grain_norm_ms": 0, "grain_halves": False}
    o.update(kw)
    return o


def _level_db(a: np.ndarray) -> float:
    p = np.mean(a ** 2, axis=1)
    w = min(len(p), int(0.05 * SR))
    if w < 1:
        return -300.0
    return float(db(uniform_filter1d(p, w, mode="constant").max()))


def fit(x: Any, name: str | None = None, verbose: bool = False, sr: int = SR, **kw) -> dict:
    """A spectral noise model of a path or a 48 kHz array, (n,) or (n, 2)."""
    t0 = time.time()
    opts = _opts(**kw)
    a = as_stereo(x) if isinstance(x, str) else as_stereo(resample(np.asarray(x, float), sr))
    model = _fit_model(a, opts, lambda: name or source_name(x))
    model["fit"]["seconds"] = round(time.time() - t0, 3)
    if verbose:
        print(summary(model))
    return model


def _fit_model(a: np.ndarray, opts: dict, name) -> dict:
    m = a.mean(axis=1)
    n = len(m)
    cents = centers(opts["n_bands"])
    an = analyse(a, cents, opts["stereo"] and not is_mono_pair(a))
    D = an["D"]
    ridges = _ridges(m, opts) if opts["ridges"] and n >= 2048 else []
    _remove_ridges(D, ridges, cents)
    S, k, wander = _gesture(D, opts)
    S, rank = _denoise(S, n, k, cents) if opts["denoise"] else (S, 0)
    gain = _gain_iter(S, k, _onset_frame(D), n, cents, S > S.max() - 70, opts)
    st = _stereo_traj(an["stereo"])
    keys, t = _keyframes(gain, st, bandwidths(cents), opts, wander)
    model = _store(a, name(), n, cents, gain, st, keys, t, ridges, opts, k, rank, wander)
    if opts["fine"]:
        model["fine"] = _fine(m, model, cents)
    if opts["texture"]:
        model["texture"] = _texture(m, model, opts)
    return model


def _store(a, name, n, cents, gain, st, keys, t, ridges, opts, k, rank, wander) -> dict:
    stereo = None
    if st is not None:
        stereo = {"groups": list(GROUPS), "ild": np.round(st["ild"][:, keys].T, 2).tolist(),
                  "icc": np.round(st["icc"][:, keys].T, 3).tolist()}
    return {"kind": KIND, "version": VERSION, "source": name, "sr": SR, "dur": round(n / SR, 4),
            "level_db": round(_level_db(a), 2),
            "bands": {"n": len(cents), "f_lo": F_LO, "f_hi": F_HI, "scale": "ln(1+f/80)"},
            "fit": {"gesture_ms": 5 * k, "rank": rank, "tol_db": opts["tol_db"], "fluct_db": round(wander, 3)},
            "env": {"t": [round(float(t[i]), 4) for i in keys],
                    "db": np.round(gain[:, keys].T, 1).tolist()},
            "stereo": stereo, "fine": None, "texture": None, "ridges": ridges, "finish": True,
            "channels": 2 if stereo else 1}


# ================================================================ render

def _centroids(kdb: np.ndarray, cents) -> tuple[np.ndarray, float]:
    p = 10 ** (kdb / 10) * bandwidths(cents)[None, :]
    w = (p / np.maximum(p.max(axis=1, keepdims=True), 1e-300)) ** 0.3
    c = np.sum(w * np.log2(cents)[None, :], axis=1) / np.maximum(w.sum(axis=1), 1e-300)
    kp = p.sum(axis=1)
    return c, float(np.sum(kp * c) / max(kp.sum(), 1e-300))


def _shift_keys(kdb, cents, o: np.ndarray) -> np.ndarray:
    ec = warp(cents)
    out = kdb.copy()
    for i, oi in enumerate(o):
        if oi != 0:
            out[i] = np.interp(warp(cents / 2 ** oi), ec, kdb[i])
    return out


def _edit_keys(model, kdb, cents, oct_, sweep, tilt, proximity):
    c, mean = _centroids(kdb, cents)
    o = oct_ + (sweep - 1) * (c - mean)
    if np.any(o != 0):
        kdb = _shift_keys(kdb, cents, o)
    kdb = kdb + tilt * np.log2(cents / 1000)[None, :]
    if proximity:
        lev = db(np.sum(10 ** (kdb / 10) * bandwidths(cents), axis=1))
        d = np.clip(lev - lev.max(), -40, 0)
        kdb = kdb + proximity * d[:, None] * np.maximum(0.5 + 0.15 * np.log2(cents / 1000), 0)[None, :]
    return kdb, c, mean


def _edit_ridges(ridges, s, oct_, sweep, c, mean, kt, gain):
    if not ridges or gain == 0:
        return []
    out = []
    for r in ridges:
        rt = np.asarray(r["t"]) * s
        ci = np.interp(rt, kt, c)
        f = np.minimum(np.asarray(r["f"]) * 2 ** (oct_ + (sweep - 1) * (ci - mean)), 0.45 * SR)
        out.append({"t": rt.tolist(), "f": f.tolist(), "db": (np.asarray(r["db"]) + 20 * np.log10(gain)).tolist(),
                    "jitter": r["jitter"]})
    return out


def _edit_stereo(st, width, pan):
    if st is None:
        return None
    icc = np.clip(1 - width * (1 - np.asarray(st["icc"])), -1, 1)
    return {"icc": icc, "ild": np.asarray(st["ild"]) * width * pan}


def _peak50(P: np.ndarray) -> float:
    w = min(len(P), 10)
    return float(db(uniform_filter1d(P, w, mode="constant").max())) if len(P) else -300.0


def _take_gain(y: np.ndarray, Pk: np.ndarray, n: int) -> float:
    from hark.core.features import k_weight
    yy = y if y.ndim == 2 else y[:, None]
    got = float(np.mean([np.sum(np.asarray(k_weight(yy[:, c], SR)) ** 2) for c in range(yy.shape[1])]))
    want = float(np.sum(np.interp(np.arange(n), np.arange(len(Pk)) * HOP, Pk)))
    return float(np.sqrt(want / got)) if got > 0 else 1.0


def render(model: dict, seed: int = 0, length: float = 1, speed: float = 1, sweep: float = 1,
           shift: float = 0, tilt: float = 0, roughness: float = 1, width: float = 1, ridges: float = 1,
           gain_db: float = 0, doppler: float = 0, proximity: float = 0, pan: float = 1,
           motion: float = 0, level: bool = True, limit: bool = True, steady: bool = True) -> np.ndarray:
    """A take of the model: (n, 2) float32 at 48 kHz."""
    check(model)
    k = {"length": length, "speed": speed, "sweep": sweep, "shift": shift, "tilt": tilt,
         "roughness": roughness, "width": width, "ridges": ridges, "gain_db": gain_db, "doppler": doppler,
         "proximity": proximity, "pan": pan, "motion": motion, "level": level, "limit": limit,
         "steady": steady}
    return _render_knobs(model, seed, k).astype(np.float32)


def _render_knobs(model: dict, seed: int, k: dict) -> np.ndarray:
    s, n, kt, oct_ = _take_time(model, k)
    cents = centers(model["bands"]["n"])
    kdb, rd, st = _edited(model, k, s, kt, cents, oct_)
    y, P = _synth(n, kt, kdb, cents, seed, st, model.get("texture"), rd, mono=False, rate_scale=1 / s,
                  roughness=k["roughness"], fine=model.get("fine"), fine_shift=oct_, power=True)
    y = _level(y, P, model, k["gain_db"], k["level"], k["steady"], n)
    if k["doppler"] or k["motion"]:
        y = _motion(y, model, s, k["doppler"], k["motion"])
    return _peak_cap(y) if k["limit"] else y


def _take_time(model: dict, k: dict) -> tuple[float, int, np.ndarray, float]:
    """Time stretch, sample count, key times and pitch shift (octaves) of a take."""
    speed = max(k["speed"], 0.001)
    s = max(k["length"], 0.001) / speed
    n = max(64, int(round(model["dur"] * s * SR)))
    kt = np.asarray(model["env"]["t"], dtype=float) * s
    return s, n, kt, k["shift"] + 0.5 * np.log2(speed)


def _edited(model: dict, k: dict, s: float, kt: np.ndarray, cents: np.ndarray, oct_: float):
    """Edited keyframes, ridges and stereo for a take."""
    kdb, c, mean = _edit_keys(model, np.asarray(model["env"]["db"], float), cents, oct_, k["sweep"], k["tilt"],
                              k["proximity"])
    rd = _edit_ridges(model.get("ridges"), s, oct_, k["sweep"], c, mean, kt, k["ridges"])
    return kdb, rd, _edit_stereo(model.get("stereo"), k["width"], k["pan"])


def _peak_cap(y: np.ndarray) -> np.ndarray:
    """Scale y down so its peak is at most -0.5 dBFS."""
    pk = float(np.max(np.abs(y))) if y.size else 0.0
    cap = 10 ** (-0.5 / 20)
    return y * (cap / pk) if pk > cap else y


def _level(y, P, model, gain_db, level, steady, n):
    if not level:
        return y
    if steady:
        g = 10 ** ((model["level_db"] + gain_db - _peak50(P["p"])) / 20) * _take_gain(y, P["k"], n)
        return y * g
    p = np.mean(y ** 2, axis=1)
    w = min(len(p), int(0.05 * SR))
    pk = float(db(uniform_filter1d(p, w, mode="constant").max()))
    return y * 10 ** ((model["level_db"] + gain_db - pk) / 20)


def _motion(y, model, s, doppler, motion):
    tr = trajectory(model)
    t_pk = tr["peak_s"] * s
    tau = max(0.015, 0.25 * (tr["rise_s"] + tr["fall_s"]) * s)
    if doppler:
        y = bend(y, t_pk, tau, doppler)
    if motion:
        y = pass_pan(y, t_pk, tau, motion)
    return y


def bend(y: np.ndarray, t_pk: float, tau: float, semitones: float) -> np.ndarray:
    """A pass-by pitch bend as a time warp: higher while it approaches, lower as it recedes."""
    y = np.asarray(y, dtype=float)
    pos = _bend_pos(len(y), t_pk, tau, semitones)
    src = np.arange(len(y))
    if y.ndim == 1:
        return np.interp(pos, src, y, left=0.0, right=0.0)
    return np.column_stack([np.interp(pos, src, y[:, c], left=0.0, right=0.0) for c in range(y.shape[1])])


def _bend_pos(n: int, t_pk: float, tau: float, semitones: float) -> np.ndarray:
    """Source read positions of the warp, aligned at the peak and cut at the source end."""
    m = int(np.ceil(n * 2 ** (abs(semitones) / 12))) + 2
    t = np.arange(m) / SR
    rate = 2 ** (semitones * np.tanh((t_pk - t) / max(tau, 1e-9)) / 12)
    pos = np.concatenate([[0.0], np.cumsum(rate)[:-1]])
    k = min(int(round(t_pk * SR)), m - 1)
    pos = pos - pos[k] + t_pk * SR
    stop = np.flatnonzero(pos > n - 1)
    return pos[:int(stop[0]) if len(stop) else m]


def pass_pan(y: np.ndarray, t_pk: float, tau: float, amount: float) -> np.ndarray:
    """A constant-power pass-by pan, unity in the centre."""
    y = np.asarray(y, dtype=float)
    yy = np.column_stack([y, y]) if y.ndim == 1 else y
    t = np.arange(len(yy)) / SR
    pos = np.clip(amount * np.tanh((t - t_pk) / max(tau, 1e-9)), -1, 1)
    ang = (pos + 1) * np.pi / 4
    return np.column_stack([yy[:, 0] * np.sqrt(2) * np.cos(ang), yy[:, 1] * np.sqrt(2) * np.sin(ang)])


# ================================================================ trajectory, summary

def trajectory(model: dict) -> dict:
    """The gesture's motion per keyframe."""
    cents = centers(model["bands"]["n"])
    kdb = np.asarray(model["env"]["db"], dtype=float)
    t = np.asarray(model["env"]["t"], dtype=float)
    lev = db((10 ** (kdb / 10) * bandwidths(cents)[None, :]).sum(axis=1))
    c, _ = _centroids(kdb, cents)
    pk, first, last = _peak_span(lev)
    out = {"t": t.tolist(), "level_db": np.round(lev, 2).tolist(), "centroid_hz": np.round(2 ** c, 1).tolist(),
           "tilt_db_oct": [round(_tilt(kdb[i], cents), 2) for i in range(len(t))],
           "peak_s": float(t[pk]), "rise_s": float(t[pk] - t[first]), "fall_s": float(t[last] - t[pk])}
    out.update(_traj_stereo(model.get("stereo"), cents, kdb))
    return out


def _peak_span(lev: np.ndarray) -> tuple[int, int, int]:
    """Peak key and the first and last keys within 20 dB of it."""
    pk = int(np.argmax(lev))
    near = lev >= lev[pk] - 20
    return pk, int(np.argmax(near)), pk + int(np.flatnonzero(near[pk:])[-1])


def _tilt(row: np.ndarray, cents) -> float:
    sel = row >= row.max() - 40
    if sel.sum() < 2:
        return 0.0
    w = 10 ** ((row[sel] - row.max()) / 10)
    return float(np.polyfit(np.log2(cents[sel] / 1000), row[sel], 1, w=np.sqrt(w))[0])


def _traj_stereo(st, cents, kdb) -> dict:
    if not st:
        return {"pan_db": [0.0] * len(kdb), "width": [0.0] * len(kdb)}
    g = _group_of(cents)
    p = 10 ** (kdb / 10) * bandwidths(cents)[None, :]
    gp = np.array([p[:, g == k].sum(axis=1) for k in range(len(GROUPS) - 1)]).T
    w = gp / np.maximum(gp.sum(axis=1, keepdims=True), 1e-300)
    ild, icc = np.asarray(st["ild"]), np.asarray(st["icc"])
    return {"pan_db": np.round((w * ild).sum(axis=1), 2).tolist(),
            "width": np.round(1 - (w * icc).sum(axis=1), 3).tolist()}


def summary(model: dict) -> str:
    """One paragraph of words."""
    tr = trajectory(model)
    lev = np.asarray(tr["level_db"])
    pk = int(np.argmax(lev))
    near = np.asarray(tr["t"])[lev >= lev[pk] - 20]
    cen = tr["centroid_hz"]
    parts = [f"{model.get('source') or 'array'}: {model['dur'] * 1000:.0f} ms noise gesture, "
             f"{len(tr['t'])} keyframes, rises {tr['rise_s'] * 1000:.0f} ms to its peak and stays within "
             f"20 dB for {(near[-1] - near[0]) * 1000:.0f} ms; centroid {cen[0]:.0f} Hz at onset, "
             f"{cen[pk]:.0f} Hz at peak, {cen[-1]:.0f} Hz at end"]
    parts.append(_stereo_words(model.get("stereo"), tr))
    parts.append(_texture_words(model.get("texture")))
    for r in (model.get("ridges") or [])[:4]:
        parts.append(f"ridge {r['f'][0]:.0f} to {r['f'][-1]:.0f} Hz")
    import json
    parts.append(f"{len(json.dumps(model, separators=(',', ':'))) / 1024:.1f} kB")
    return "; ".join(p for p in parts if p) + "."


def _stereo_words(st, tr) -> str:
    if not st:
        return "mono"
    icc = np.asarray(st["icc"])
    pan = tr["pan_db"]
    return (f"stereo (correlation median {np.median(icc):.2f}, minimum {icc.min():.2f}, pan "
            f"{min(pan):+.1f} to {max(pan):+.1f} dB)")


def _texture_words(tex) -> str:
    if not tex:
        return ""
    out = []
    am = tex.get("am")
    if am:
        rate = am["rates"][int(np.argmax(am["spec"]))]
        out.append(f"flutter {rate:.0f} Hz, depth {8.686 * max(am['depth']):.1f} dB, coherence {am['coh']:.2f}")
    if tex.get("grains"):
        out.append(f"crackle {tex['grains']['density']:.0f} per second")
    return ", ".join(out)


# ================================================================ face

def check(model: Any) -> None:
    """ValueError when the model is not a spectral_noise model of a supported version."""
    check_kind(model, KIND, VERSION)


def save(model: dict, path: str) -> None:
    """Write compact JSON."""
    save_json(model, path)


def load(path: str) -> dict:
    """Read a model and check its kind."""
    m = load_json(path)
    check(m)
    return m


def to_code(model: dict, var: str = "MODEL") -> str:
    """Readable Python that holds the model and renders it through hark.fit.analysis.spectral."""
    import pprint
    lite = copy.deepcopy({k: v for k, v in model.items() if k != "fit_engine"})
    return ('"""Spectral noise model: renders through hark.fit.analysis.spectral."""\n\n'
            "import numpy as np\n\nfrom hark.fit.analysis import spectral\n\n"
            f"{var} = " + pprint.pformat(lite, width=110, compact=True) + "\n\n\n"
            f"def render(seed=0, **knobs):\n    return spectral.render({var}, seed, **knobs)\n")


# ================================================================ compare

def _trim(x: np.ndarray) -> np.ndarray:
    e = db(np.convolve(x ** 2, np.ones(48) / 48, mode="same")[::48])
    k = int(np.argmax(e >= e.max() - 30)) if len(e) else 0
    return x[max(0, k * 48 - 240):]


def _lev50(x: np.ndarray) -> np.ndarray:
    w = min(len(x), 2400)
    p = uniform_filter1d(x ** 2, max(w, 1), mode="constant").max() if len(x) else 0.0
    return x / np.sqrt(p) if p > 0 else x


def _tone_profile(x: np.ndarray) -> np.ndarray:
    c = 1000 * 2 ** (np.arange(-15, 13) / 3)
    X = np.abs(rfft(x)) ** 2
    f = rfftfreq(len(x), 1 / SR)
    p = np.array([X[(f >= ci * 2 ** (-1 / 6)) & (f < ci * 2 ** (1 / 6))].sum() for ci in c])
    return np.maximum(db(p / max(p.sum(), 1e-300)), -60)


def _attack_ms(x: np.ndarray) -> float:
    e = db(uniform_filter1d(np.convolve(x ** 2, np.ones(96) / 96, mode="same")[::96], 3))
    pk = int(np.argmax(e))
    return float((pk - int(np.argmax(e >= e[pk] - 20))) * 2.0)


def compare(a: Any, b: Any, clap: bool = False) -> dict:
    """The fitter's own distance set: mel, envelope correlation, attack, tone, lengths, optional CLAP."""
    xa, xb = (np.asarray(v, float).mean(axis=1) if np.ndim(v) == 2 else np.asarray(v, float) for v in (a, b))
    ta, tb = _lev50(_trim(xa)), _lev50(_trim(xb))
    n = max(len(ta), len(tb))
    ta, tb = np.pad(ta, (0, n - len(ta))), np.pad(tb, (0, n - len(tb)))
    out = {"mel_db": round(mel_db_distance(ta, tb)[0], 3), "env_corr": round(_env_corr5(ta, tb), 4),
           "attack_ms": [_attack_ms(ta), _attack_ms(tb)],
           "tone_db": round(float(np.sqrt(np.mean((_tone_profile(xa) - _tone_profile(xb)) ** 2))), 3),
           "len_s": [round(len(xa) / SR, 4), round(len(xb) / SR, 4)]}
    if clap:
        out["clap"] = _clap_sim(xa, xb)
    return out


def _env_corr5(ta: np.ndarray, tb: np.ndarray) -> float:
    """Correlation of the 5 ms level envelopes over frames within 60 dB of a's peak."""
    ea = db(np.convolve(ta ** 2, np.ones(240) / 240, mode="same")[::240])
    eb = db(np.convolve(tb ** 2, np.ones(240) / 240, mode="same")[::240])
    fl = ea.max() - 60
    act = (ea > fl) | (eb > fl)
    ec = float(np.corrcoef(np.maximum(ea, fl)[act], np.maximum(eb, fl)[act])[0, 1]) if act.sum() > 3 else 0.0
    return ec if np.isfinite(ec) else 0.0


def _clap_sim(xa: np.ndarray, xb: np.ndarray) -> float:
    from hark.models.clap import get
    E = get().embed_audio([xa.astype(np.float32), xb.astype(np.float32)])
    return round(float(E[0] @ E[1] / (np.linalg.norm(E[0]) * np.linalg.norm(E[1]))), 4)


def fit_report(x: Any, clap: bool = False, seeds=(11, 12), **fit_kw) -> dict:
    """Fit plus 2 renders: the model, the renders, the fit distance and the floor."""
    t0 = time.time()
    a = as_stereo(x)
    model = fit(a, **fit_kw)
    r = [render(model, s) for s in seeds]
    return {"model": model, "renders": r, "orig": a, "fit": compare(a, r[0], clap),
            "floor": compare(r[0], r[1], clap), "seconds": round(time.time() - t0, 3)}
