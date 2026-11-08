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

from hark.fit.analysis._common import (SR, as_stereo, check_kind, merge_knobs, db, is_mono_pair,
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
    ev = warp(freqs)
    ecv = warp(cents)
    wmat = np.zeros((len(ecv), len(ev)))
    for bv in range(len(ecv)):
        _fill_weight(wmat[bv], ev, ecv, bv)
    return wmat


def _fill_weight(rw: np.ndarray, ev: np.ndarray, ecv: np.ndarray, bv: int) -> None:
    low_i = ecv[bv - 1] if bv > 0 else ecv[0] - (ecv[1] - ecv[0])
    _rise_part(rw, ev, low_i, ecv[bv])
    if bv == len(ecv) - 1:
        rw[ev > ecv[bv]] = 1.0
    else:
        _fall_part(rw, ev, ecv[bv], ecv[bv + 1])


def _rise_part(rw: np.ndarray, ev: np.ndarray, low_i: float, cv: float) -> None:
    rise = (ev >= low_i) & (ev <= cv)
    rw[rise] = np.sin(0.5 * np.pi * (ev[rise] - low_i) / (cv - low_i)) ** 2


def _fall_part(rw: np.ndarray, ev: np.ndarray, cv: float, high_i: float) -> None:
    fall = (ev > cv) & (ev < high_i)
    rw[fall] = np.cos(0.5 * np.pi * (ev[fall] - cv) / (high_i - cv)) ** 2


@functools.lru_cache(maxsize=8)
def _bandwidths(kname: tuple) -> np.ndarray:
    fq = np.linspace(0.0, SR / 2, 96001)
    return np.trapezoid(weights(fq, np.asarray(kname)), fq, axis=1)


def bandwidths(cents: Any) -> np.ndarray:
    """Integral of each band weight over 0 .. 24 kHz, Hz."""
    return _bandwidths(tuple(float(cv) for cv in np.asarray(cents)))


def _band_sizes(cent_v) -> np.ndarray:
    bwv = bandwidths(cent_v)
    return np.clip(2 ** np.round(np.log2(2 * SR / bwv)), 512, 4096).astype(int)


def n_frames(n: int) -> int:
    """Frame count of the 5 ms analysis grid."""
    return int(n) // HOP + 1


def _group_of(cent_v) -> np.ndarray:
    return np.clip(np.searchsorted(GROUPS, cent_v, side="right") - 1, 0, len(GROUPS) - 2)


def _group_centres() -> np.ndarray:
    gv = np.array([np.sqrt(max(low_i, 50.0) * high_i) for low_i, high_i in zip(GROUPS[:-1], GROUPS[1:])])
    gv[0] = 150.0
    return gv


def _group_blend(cent_v) -> np.ndarray:
    """(B, G) weights that interpolate a group parameter to each band in log2 frequency."""
    lgv = np.log2(_group_centres())
    eye = np.eye(len(lgv))
    lc = np.log2(np.asarray(cent_v, dtype=float))
    return np.array([[np.interp(vv, lgv, eye[gv]) for gv in range(len(lgv))] for vv in lc])


# ================================================================ analysis

def _hann(nn: int) -> np.ndarray:
    wv = 0.5 - 0.5 * np.cos(2 * np.pi * np.arange(nn) / nn)
    return wv / np.sqrt(np.sum(wv ** 2))


def _frames_fft(x: np.ndarray, nn: int, T: int) -> np.ndarray:
    xp = np.concatenate([np.zeros(nn // 2), x, np.zeros(nn + HOP * T)])
    frv = np.lib.stride_tricks.sliding_window_view(xp, nn)[::HOP][:T]
    return rfft(frv * _hann(nn), axis=1, workers=threads())


def analyse(x: Any, cents: Any = None, stereo: bool = True) -> dict:
    """Band power density (B, T), group stereo powers or None, and the frame count."""
    av = x if (isinstance(x, np.ndarray) and x.ndim == 1) else as_stereo(x)
    cents = centers() if cents is None else np.asarray(cents)
    T = n_frames(len(av))
    dmat = _density(av, cents, T)
    stv = _stereo_powers(av, T) if (av.ndim == 2 and stereo and not is_mono_pair(av)) else None
    return {"D": dmat, "stereo": stv, "T": T}


def _density(av: np.ndarray, cent_v: np.ndarray, T: int) -> np.ndarray:
    chan_l = [av[:, 0], av[:, 1]] if av.ndim == 2 else [av]
    size_l = _band_sizes(cent_v)
    dmat = np.zeros((len(cent_v), T))
    for nn in np.unique(size_l):
        pick = np.flatnonzero(size_l == nn)
        dmat[pick] = _size_density(chan_l, nn, T, cent_v, pick)
    return dmat


def _size_density(chan_l: list, nn: int, T: int, cent_v: np.ndarray, pick: np.ndarray) -> np.ndarray:
    pmat = np.mean([np.abs(_frames_fft(cv, nn, T)) ** 2 for cv in chan_l], axis=0)
    wmat = weights(rfftfreq(nn, 1 / SR), cent_v)
    Wn = wmat[pick] / np.maximum(wmat[pick].sum(axis=1, keepdims=True), 1e-30)
    return Wn @ pmat.T


def _stereo_powers(av: np.ndarray, T: int) -> dict:
    XL, XR = _frames_fft(av[:, 0], 1024, T), _frames_fft(av[:, 1], 1024, T)
    fq = rfftfreq(1024, 1 / SR)
    acc = {"L": [], "R": [], "C": []}
    for low_i, high_i in zip(GROUPS[:-1], GROUPS[1:]):
        sv = (fq >= low_i) & (fq < high_i)
        acc["L"].append(np.sum(np.abs(XL[:, sv]) ** 2, axis=1))
        acc["R"].append(np.sum(np.abs(XR[:, sv]) ** 2, axis=1))
        acc["C"].append(np.sum(np.real(XL[:, sv] * np.conj(XR[:, sv])), axis=1))
    return {ix: np.array(vv) for ix, vv in acc.items()}


# ================================================================ ridges

def _ridge_floor(P_db: np.ndarray) -> np.ndarray:
    medv = median_filter(P_db, size=(8, 1), mode="nearest")
    low_i = np.roll(medv, 6, axis=0)
    high_i = np.roll(medv, -6, axis=0)
    low_i[:6] = medv[:6]
    high_i[-6:] = medv[-6:]
    return np.maximum(low_i, high_i) + 1.6


def _frame_peaks(cl, floor, fq, bin_lo, bin_hi, rdb) -> list:
    exv = cl - floor
    return [_peak_point(cl, exv, fq, ii) for ii in _peak_cands(cl, exv, bin_lo, bin_hi, rdb)]


def _peak_cands(cl, exv, bin_lo, bin_hi, rdb) -> list[int]:
    """The 12 strongest local maxima above the floor by ridge_db."""
    cand = [ii for ii in range(max(bin_lo, 2), min(bin_hi, len(cl) - 2))
            if exv[ii] > rdb and cl[ii] == cl[ii - 2:ii + 3].max()]
    return sorted(cand, key=lambda ii: -exv[ii])[:12]


def _peak_point(cl, exv, fq, ii: int) -> tuple:
    """(Hz, dB, excess, bin) of the peak at bin i, refined by a parabola."""
    av, bv, cv = cl[ii - 1], cl[ii], cl[ii + 1]
    curv = av - 2 * bv + cv
    dv = 0.5 * (av - cv) / curv if curv < 0 else 0.0
    return (fq[ii] + dv * (fq[1] - fq[0]), bv - 0.25 * (av - cv) * dv, exv[ii], ii)


def _track_ridges(pdbm, floor, is_on, fq, cfg) -> list:
    bin_lo = int(np.searchsorted(fq, cfg["ridge_lo"]))
    bin_hi = int(np.searchsorted(fq, 16000.0))
    alive, done = [], []
    for ix in range(pdbm.shape[1]):
        pk_l = _frame_peaks(pdbm[:, ix], floor[:, ix], fq, bin_lo, bin_hi, cfg["ridge_db"]) if is_on[ix] else []
        _step_ridges(alive, pk_l, ix, floor)
        done += [trk for trk in alive if trk["miss"] > 2]
        alive = [trk for trk in alive if trk["miss"] <= 2]
    return done + alive


def _step_ridges(alive: list, pk_l: list, ix: int, floor) -> None:
    """Extend each live ridge with its nearest free peak. Free peaks start new ridges."""
    usedv = set()
    for trk in alive:
        winner = _nearest_peak(trk, pk_l, usedv)
        if winner is None:
            trk["miss"] += 1
        else:
            usedv.add(winner[1])
            _extend_ridge(trk, pk_l[winner[1]], ix, floor)
    for jj, pv in enumerate(pk_l):
        if jj not in usedv:
            alive.append({"k": [], "f": [], "db": [], "ex": [], "miss": 0})
            _extend_ridge(alive[-1], pv, ix, floor)


def _nearest_peak(trk: dict, pk_l: list, usedv: set):
    winner = None
    for jj in (jj for jj in range(len(pk_l)) if jj not in usedv):
        dv = abs(np.log2(pk_l[jj][0] / trk["f"][-1]))
        if dv < 0.04 and (winner is None or dv < winner[0]):
            winner = (dv, jj)
    return winner


def _extend_ridge(trk, pv, ix, floor) -> None:
    trk["k"].append(ix)
    trk["f"].append(pv[0])
    trk["db"].append(pv[1])
    trk["ex"].append(pv[2])
    trk["fl"] = trk.get("fl", []) + [floor[pv[3], ix]]
    trk["miss"] = 0


def _ridges(mv: np.ndarray, cfg: dict) -> list:
    pdbm, floor, is_on = _ridge_spectrum(mv)
    trk_l = _track_ridges(pdbm, floor, is_on, rfftfreq(2048, 1 / SR), cfg)
    return [_ridge_model(tv) for tv in _best_ridges(trk_l, cfg)]


def _ridge_spectrum(mv: np.ndarray):
    """2048-point power (dB), its ridge floor, and the frames within 50 dB of the loudest."""
    xmat = _frames_fft(mv, 2048, n_frames(len(mv)))
    pmat = np.abs(xmat.T) ** 2
    pdbm = db(pmat)
    totv = db(pmat.sum(axis=0))
    return pdbm, _ridge_floor(pdbm), totv >= totv.max() - 50


def _best_ridges(trk_l: list, cfg: dict) -> list:
    min_len = max(2, round(cfg["ridge_ms"] / 5))
    good = [tv for tv in trk_l if len(tv["k"]) >= min_len and np.median(tv["ex"]) >= cfg["ridge_db"] + 2]
    good.sort(key=lambda tv: -float(np.sum(10 ** (np.array(tv["db"]) / 10) - 10 ** (np.array(tv["fl"]) / 10))))
    return good[:cfg["max_ridges"]]


def _ridge_model(trk: dict) -> dict:
    tv = np.array(trk["k"]) * HOP / SR
    fq = np.array(trk["f"])
    adb = _ridge_db(trk)
    kept = _greedy_keys(tv, np.column_stack([np.log2(fq) / 0.006, adb / 1.5]), None, 1.0, 60, [])
    return {"t": [round(float(tv[ii]), 4) for ii in kept], "f": [round(float(fq[ii]), 1) for ii in kept],
            "db": [round(float(adb[ii]), 1) for ii in kept], "jitter": _jitter(fq)}


def _ridge_db(trk: dict) -> np.ndarray:
    """Sine amplitude (dB) of each ridge point above its floor."""
    pv = np.maximum(10 ** (np.array(trk["db"]) / 10) - 10 ** (np.array(trk["fl"]) / 10), 1e-30)
    return 20 * np.log10(np.sqrt(6 * pv / 2048))


def _jitter(fq: np.ndarray) -> float:
    lf = np.log2(fq)
    sm = uniform_filter1d(lf, 5, mode="nearest")
    return round(float(np.clip(np.std(lf - sm) * np.log(2), 0.0005, 0.02)), 5)


def _remove_ridges(dmat: np.ndarray, rdg_l: list, cent_v) -> None:
    bwv = bandwidths(cent_v)
    tgv = np.arange(dmat.shape[1]) * HOP / SR
    for rv in rdg_l:
        rtv = np.asarray(rv["t"])
        within = (tgv >= rtv[0]) & (tgv <= rtv[-1])
        if not within.any():
            continue
        fq = np.interp(tgv[within], rtv, rv["f"])
        A2 = 10 ** (np.interp(tgv[within], rtv, rv["db"]) / 10)
        Wf = weights(fq, cent_v)
        sub = Wf * A2 * SR / (4 * bwv[:, None])
        dmat[:, within] = np.maximum(dmat[:, within] - sub, 0.05 * dmat[:, within])


# ================================================================ gesture smoothing

def _onset_frame(dmat: np.ndarray) -> int:
    totv = db(dmat.sum(axis=0))
    return int(np.argmax(totv >= totv.max() - 30))


def _smooth(dmat: np.ndarray, ix: int, start_i: int) -> np.ndarray:
    if ix < 1:
        return dmat.copy()
    wv = np.hanning(ix + 2)[1:-1]
    wv = wv / wv.sum()
    acc = dmat.copy()
    if start_i > 0:
        acc[:, :start_i] = convolve1d(dmat[:, :start_i], wv, axis=1, mode="nearest")
    acc[:, start_i:] = convolve1d(dmat[:, start_i:], wv, axis=1, mode="nearest")
    return acc


def _wander(smat: np.ndarray, ix: int, start_i: int) -> float | None:
    from scipy.signal import savgol_filter
    totv = db(smat.sum(axis=0))
    T = len(totv)
    winv = min(T if T % 2 else T - 1, 4 * ix + 1)
    if winv < 5:
        return None
    fit = savgol_filter(totv, winv, 2, mode="nearest")
    pick = (totv >= totv.max() - 30) & (np.abs(np.arange(T) - start_i) > ix)
    if pick.sum() <= 4:
        return None
    return float(np.sqrt(np.mean((totv - fit)[pick] ** 2)))


def _gesture(dmat: np.ndarray, cfg: dict) -> tuple[np.ndarray, int, float]:
    ix = max(0, round(cfg["gesture_ms"] / 5))
    start_i = _onset_frame(dmat)
    wandr = 0.0
    if cfg["adapt"]:
        ix, wandr = _adapt_k(dmat, ix, start_i, cfg["tol_db"])
    smat = db(_smooth(dmat, ix, start_i))
    smat = np.maximum(smat, smat.max() - 90)
    return smat, ix, wandr


def _adapt_k(dmat: np.ndarray, ix: int, start_i: int, tol_db: float) -> tuple[int, float]:
    """Widen the smoothing until the level wander is within 2 tol_db."""
    k_max = max(ix, min(32, dmat.shape[1] // 6))
    while True:
        wv = _wander(_smooth(dmat, ix, start_i), max(ix, 1), start_i)
        wandr = wv or 0.0
        if wv is None or wv <= 2 * tol_db or ix >= k_max:
            break
        ix = min(k_max, int(np.ceil(1.5 * max(ix, 1))))
    return ix, wandr


# ================================================================ denoise

def _cell_weights(smat: np.ndarray, bwv: np.ndarray, span: float = 50.0) -> np.ndarray:
    gmat = smat + 10 * np.log10(bwv)[:, None]
    return np.clip((gmat - (gmat.max() - span)) / 25, 0.0, 1.0)


def _noise_level(cnt: int, ix: int, cent_v, Wc: np.ndarray) -> float:
    wv = np.random.default_rng(WHITE_SEED).standard_normal(cnt)
    dmat = analyse(wv, cent_v, False)["D"]
    smat = db(_smooth(dmat, ix, 0))
    size_l = _band_sizes(cent_v)
    sdv = np.zeros(len(cent_v))
    for bv in range(len(cent_v)):
        skipv = int(size_l[bv] // 2 // HOP)
        rw = smat[bv, skipv:smat.shape[1] - skipv] if smat.shape[1] - 2 * skipv > 8 else smat[bv]
        sdv[bv] = np.std(rw)
    wb = Wc.mean(axis=1)
    return float(np.sqrt(np.sum(wb * sdv ** 2) / max(wb.sum(), 1e-12)))


def _low_rank(smat: np.ndarray, wmat: np.ndarray, K: int) -> np.ndarray:
    estv = smat.copy()
    for _ in range(10):
        Z = wmat * smat + (1 - wmat) * estv
        muv = Z.mean(axis=1, keepdims=True)
        U, sv, Vt = np.linalg.svd(Z - muv, full_matrices=False)
        estv = muv + (U[:, :K] * sv[:K]) @ Vt[:K]
    return estv


def _denoise(smat: np.ndarray, cnt: int, ix: int, cent_v) -> tuple[np.ndarray, int]:
    bwv = bandwidths(cent_v)
    wmat = _cell_weights(smat, bwv)
    noise = _noise_level(cnt, ix, cent_v, wmat)
    winner, rnk = smat, 0
    for K in range(1, min(16, min(smat.shape)) + 1):
        estv = _low_rank(smat, wmat, K)
        winner, rnk = estv, K
        if np.sqrt(np.sum(wmat * (estv - smat) ** 2) / max(wmat.sum(), 1e-12)) <= noise:
            break
    acc = wmat * winner + (1 - wmat) * smat
    return np.maximum(acc, smat.max() - 90), rnk


# ================================================================ synthesis core

def _gens(seed: int) -> list:
    ss = np.random.SeedSequence(abs(int(seed)) % 2 ** 32)
    return [np.random.default_rng(sv) for sv in ss.spawn(5)]


def _clicks(cnt: int, ratev: float, spread: float, nrm_ms: float, rv) -> np.ndarray:
    x = np.zeros(cnt)
    cntv = rv.poisson(max(ratev, 0.0) * cnt / SR)
    posv = rv.integers(0, cnt, cntv)
    ampv = np.exp(spread * rv.standard_normal(cntv)) * np.where(rv.uniform(size=cntv) < 0.5, -1, 1)
    np.add.at(x, posv, ampv)
    x = _even_clicks(x, ratev, nrm_ms)
    rms = np.sqrt(np.mean(x ** 2))
    return x / rms if rms > 0 else x


def _even_clicks(x: np.ndarray, ratev: float, nrm_ms: float) -> np.ndarray:
    """Level the click train over a window of norm_ms (at least 20 clicks)."""
    winv = int(max(nrm_ms, 20 * 1000.0 / ratev) * SR / 1000) | 1 if nrm_ms > 0 and ratev > 0 else len(x)
    if winv < len(x) / 2:
        pv = uniform_filter1d(x ** 2, winv, mode="nearest")
        x = x * uniform_filter1d(1 / np.sqrt(np.maximum(pv, 1e-3 * pv.mean())), winv, mode="nearest")
    return x


def _gp(mv: int, rate_l, specv, rv, uhi: float | None = None) -> np.ndarray:
    """A unit-std Gaussian process at 1 kHz with a rate spectrum (variance per half-octave band)."""
    fft_n = next_fast_len(max(2 * mv, 64))
    fq = rfftfreq(fft_n, 1e-3)
    if uhi is not None:
        dens = (fq <= uhi).astype(float)
    else:
        rate_l = np.asarray(rate_l, dtype=float)
        bwr = rate_l * (2 ** 0.25 - 2 ** -0.25)
        dv = np.asarray(specv, dtype=float) / bwr
        dens = np.where(fq >= rate_l[0] * 2 ** -0.25,
                        np.interp(np.log2(np.maximum(fq, 1e-9)), np.log2(rate_l), dv), 0.0)
    zv = irfft(rfft(rv.standard_normal(fft_n)) * np.sqrt(dens), fft_n)[:mv]
    sv = np.std(zv)
    return zv / sv if sv > 0 else zv


def _modulation(cnt: int, amv: dict | None, rscale: float, rough: float, rv, gbm) -> np.ndarray | None:
    if not amv or rough <= 0 or max(amv["depth"]) <= 0:
        return None
    mv = cnt // 48 + 2
    rate_l = np.asarray(amv["rates"]) * rscale
    zc = _gp(mv, rate_l, amv["spec"], rv)
    coh = float(amv["coh"])
    mods = []
    for dv in amv["depth"]:
        zg = _gp(mv, rate_l, amv["spec"], rv)
        zv = np.sqrt(coh) * zc + np.sqrt(1 - coh) * zg
        sg = dv * rough
        mods.append(np.exp(sg * zv - sg ** 2))
    tc = np.arange(mv) * 48
    gmat = np.array([np.interp(np.arange(cnt), tc, mo) for mo in mods])
    return gbm @ gmat


def _stereo_ctrl(stv: dict, key_t, cnt: int, gbm) -> tuple:
    iccv = np.clip(np.asarray(stv["icc"], dtype=float), -1, 1)
    ildv = np.asarray(stv["ild"], dtype=float)
    th = 0.5 * np.arccos(iccv)
    rv = 10 ** (ildv / 10)
    par = [np.cos(th), np.sin(th), np.sqrt(2 * rv / (1 + rv)), np.sqrt(2 / (1 + rv))]
    tv = np.arange(cnt) / SR
    return tuple(gbm @ np.array([np.interp(tv, key_t, pv[:, gv]) for gv in range(pv.shape[1])]) for pv in par)


def _fine_gain(finev: dict | None, shift: float, fq: np.ndarray) -> np.ndarray | float:
    if not finev:
        return 1.0
    gridv = np.arange(len(finev["db"])) * finev["step"]
    posv = np.log2(np.maximum(fq, 1e-9) / 2 ** shift / finev["f0"])
    gv = np.where((posv >= gridv[0]) & (posv <= gridv[-1]), np.interp(posv, gridv, finev["db"]), 0.0)
    return 10 ** (gv / 20)


def _excitation(cnt, fft_n, texv, rough, rscale, rv) -> np.ndarray:
    grv = (texv or {}).get("grains")
    if grv and rough > 0:
        x = _clicks(fft_n, grv["density"] / rough ** 2 * rscale, grv.get("spread", 1.0),
                    grv.get("norm_ms", 0.0), rv)
    else:
        x = rv.standard_normal(fft_n)
    return rfft(x)


SYNTH_DEFAULTS = {"stereo": None, "texture": None, "ridges": None, "mono": True, "rate_scale": 1.0,
                  "roughness": 1.0, "fine": None, "fine_shift": 0.0, "power": False, "ridge_sig": None}


def _synth(cnt, key_t, key_db, cent_v, seed, **opt):
    """The synthesis core: band noise on keyframes, with optional stereo, texture and ridges.

    Options and defaults: SYNTH_DEFAULTS. With power=True it also returns the expected power."""
    ov = merge_knobs(SYNTH_DEFAULTS, opt, "synth")
    gv = _gens(seed)
    key_db = np.asarray(key_db, dtype=float)
    bed = _noise_bed(cnt, key_t, key_db, cent_v, ov, gv)
    lwin, rmat = bed["L"], bed["R"]
    _ridge_layer(lwin, rmat, ov["ridges"], ov["ridge_sig"], gv[4], bed["ctrl"], cent_v)
    yv = lwin if ov["mono"] else np.column_stack([lwin, rmat if rmat is not None else lwin])
    return (yv, _expected(cnt, key_t, key_db, cent_v, bed["nfft"], bed["W"], ov["ridges"])) if ov["power"] else yv


def _noise_bed(cnt, key_t, key_db, cent_v, ov: dict, gv: list) -> dict:
    """The band-noise layer: L (and R for stereo), with the weights, FFT size and stereo control."""
    texv = ov["texture"] or {}
    pair2 = not ov["mono"] and ov["stereo"] is not None
    fft_n = next_fast_len(max(cnt, 64))
    wmat, sa_m, sb_m = _spectra(cnt, fft_n, cent_v, texv, gv[:3], pair2, (ov["fine"], ov["fine_shift"]),
                         (ov["roughness"], ov["rate_scale"]))
    gbm = _group_blend(cent_v)
    modv = _modulation(cnt, texv.get("am"), ov["rate_scale"], ov["roughness"], gv[3], gbm)
    ctl = _stereo_ctrl(ov["stereo"], key_t, cnt, gbm) if pair2 else None
    lwin, rmat = _band_mix(cnt, key_t, key_db, (sa_m, sb_m, wmat, fft_n), modv, ctl)
    return {"L": lwin, "R": rmat, "W": wmat, "nfft": fft_n, "ctrl": ctl}


def _spectra(cnt, fft_n, cent_v, texv, gens, pair2, fine_args, rough_args):
    """Band weights and the excitation spectra: A always, B only for stereo."""
    gA, gB, gC = gens
    rough, rscale = rough_args
    fq = rfftfreq(fft_n, 1 / SR)
    wmat = weights(fq, cent_v)
    fgv = _fine_gain(*fine_args, fq)
    sa_m = _excitation(cnt, fft_n, texv, rough, rscale, gC if texv.get("grains") else gA) * fgv
    sb_m = rfft(gB.standard_normal(fft_n)) * fgv if pair2 else None
    return wmat, sa_m, sb_m


def _ridge_layer(lwin, rmat, rdg_l, rsig, rv, ctl, cent_v) -> None:
    rsv = rsig if rsig is not None else _ridge_wave(len(lwin), rdg_l, rv)
    own = rsv is not None and rmat is not None and rsig is None
    _add_ridge_sig(lwin, rmat, rsv, _ridge_gains(rdg_l, ctl, cent_v) if own else (1.0, 1.0))


def _add_ridge_sig(lwin: np.ndarray, rmat, rsv, gains) -> None:
    if rsv is not None:
        lwin += rsv * gains[0]
        if rmat is not None:
            rmat += rsv * gains[1]


def _band_mix(cnt, key_t, key_db, specv, modv, ctl):
    """Sum of the band noises: L only, or L and R from 2 excitations and the stereo control.

    spec is (SA, SB, W, nfft): the excitation spectra, band weights and FFT size."""
    tv = np.arange(cnt) / SR
    lwin = np.zeros(cnt)
    rmat = np.zeros(cnt) if ctl is not None else None
    for bv in range(key_db.shape[1]):
        ampv = 10 ** (np.interp(tv, key_t, key_db[:, bv]) / 20)
        if modv is not None:
            ampv = ampv * modv[bv]
        _mix_band(lwin, rmat, ampv, _band_noise(specv, bv, cnt, ctl is not None),
                  None if ctl is None else [pv[bv] for pv in ctl])
    return lwin, rmat


def _band_noise(specv, bv: int, cnt: int, pair2: bool):
    sa_m, sb_m, wmat, fft_n = specv
    na_v = irfft(sa_m * wmat[bv], fft_n)[:cnt]
    return na_v, (irfft(sb_m * wmat[bv], fft_n)[:cnt] if pair2 else None)


def _mix_band(lwin, rmat, ampv, noises, cb) -> None:
    na_v, nB = noises
    if cb is None:
        lwin += ampv * na_v
    else:
        cv, sv, gl, grv = cb
        lwin += ampv * gl * (cv * na_v + sv * nB)
        rmat += ampv * grv * (cv * na_v - sv * nB)


def _ridge_wave(cnt: int, rdg_l: list | None, rv) -> np.ndarray | None:
    if not rdg_l:
        return None
    tv = np.arange(cnt) / SR
    acc = np.zeros(cnt)
    for rdv in rdg_l:
        _add_ridge(acc, tv, rdv, rv)
    return acc


def _add_ridge(acc: np.ndarray, tv: np.ndarray, rdv: dict, rv) -> None:
    """Add one jittered ridge sine. The random draws happen even when the ridge is outside t."""
    rtv = np.asarray(rdv["t"], dtype=float)
    within = (tv >= rtv[0]) & (tv <= rtv[-1])
    zv = _gp(len(tv) // 48 + 2, None, None, rv, uhi=16.0)
    begin = rv.uniform(0, 2 * np.pi)
    if within.any():
        zi = np.interp(tv[within], np.arange(len(zv)) * 48 / SR, zv)
        fq = np.minimum(np.interp(tv[within], rtv, rdv["f"]) * (1 + rdv["jitter"] * zi), 0.45 * SR)
        ph = begin + 2 * np.pi * np.cumsum(fq) / SR
        av = 10 ** (np.interp(tv[within], rtv, rdv["db"]) / 20) * _edge_fade(int(within.sum()))
        acc[within] += av * np.cos(ph)


def _edge_fade(mv: int) -> np.ndarray:
    ix = min(int(0.005 * SR), mv // 2)
    wv = np.ones(mv)
    if ix > 0:
        ramp = np.arange(ix) / ix
        wv[:ix] = ramp
        wv[mv - ix:] = ramp[::-1]
    return wv


def _ridge_gains(rdg_l, ctl, cent_v):
    gv = _group_of(np.array([np.median(rv["f"]) for rv in rdg_l]))
    bv = np.array([int(np.argmin(np.abs(cent_v - cv))) for cv in _group_centres()[gv]])
    return np.mean(ctl[2][bv], axis=0), np.mean(ctl[3][bv], axis=0)


@functools.lru_cache(maxsize=4)
def _kgain2(fft_n: int) -> np.ndarray:
    from hark.core.features import k_weight
    imp = np.zeros(fft_n)
    imp[0] = 1.0
    return np.abs(rfft(np.asarray(k_weight(imp, SR), dtype=float))) ** 2


def _expected(cnt, key_t, key_db, cent_v, fft_n, wmat, rdg_l) -> dict:
    """Expected power on the 5 ms grid, plain and K-weighted (channel mean)."""
    tgv = np.arange(n_frames(cnt)) * HOP / SR
    ampv = np.array([10 ** (np.interp(tgv, key_t, key_db[:, bv]) / 20) for bv in range(len(cent_v))])
    ix2 = _kgain2(fft_n)
    acc = {}
    for kname, gv in (("p", 1.0), ("k", ix2)):
        self_w = 2 * np.sum(wmat ** 2 * gv, axis=1) / fft_n
        over = 2 * 2 * np.sum(wmat[1:] * wmat[:-1] * gv, axis=1) / fft_n
        pmat = (ampv ** 2 * self_w[:, None]).sum(axis=0) + (ampv[1:] * ampv[:-1] * over[:, None]).sum(axis=0)
        acc[kname] = pmat + _ridge_power(tgv, rdg_l, ix2 if kname == "k" else None, fft_n)
    return acc


def _ridge_power(tgv, rdg_l, ix2, fft_n) -> np.ndarray:
    pmat = np.zeros(len(tgv))
    for rdv in rdg_l or []:
        rtv = np.asarray(rdv["t"], dtype=float)
        within = (tgv >= rtv[0]) & (tgv <= rtv[-1])
        gv = 1.0
        if ix2 is not None:
            gv = ix2[min(int(round(np.median(rdv["f"]) * fft_n / SR)), len(ix2) - 1)]
        pmat[within] += 0.5 * 10 ** (np.interp(tgv[within], rtv, rdv["db"]) / 10) * gv
    return pmat


# ================================================================ gain correction

def _gain_iter(smat, ix, start_i, cnt, cent_v, is_on, cfg) -> np.ndarray:
    gn = smat.copy()
    topv = smat.max()
    key_t = np.arange(smat.shape[1]) * HOP / SR
    for it in range(cfg["iters"]):
        have = []
        for sdv in cfg["seeds"]:
            yv = _synth(cnt, key_t, gn.T, cent_v, 7919 * (it + 1) + sdv)
            have.append(db(_smooth(analyse(yv, cent_v, False)["D"], ix, start_i)))
        diffv = np.where(is_on, smat - np.mean(have, axis=0), 0.0)
        diffv = uniform_filter(diffv, size=3, mode="nearest")
        gn = np.maximum(gn + 0.85 * np.clip(diffv, -12, 12), topv - 90)
    return gn


# ================================================================ stereo trajectory

def _stereo_traj(stv: dict | None) -> dict | None:
    if stv is None:
        return None
    pl_m, pr_m, C = (uniform_filter1d(stv[ix], 7, axis=1, mode="nearest") for ix in ("L", "R", "C"))
    ildv = np.clip(db(pl_m) - db(pr_m), -20, 20)
    iccv = np.clip(C / np.sqrt(np.maximum(pl_m * pr_m, 1e-300)), -1, 1)
    pgm = pl_m + pr_m
    _fill_quiet(ildv, iccv, pgm)
    frac = pgm.sum(axis=1) / max(pgm.sum(), 1e-300)
    return {"ild": ildv, "icc": iccv, "share": frac, "level": db(pgm.sum(axis=0))}


def _fill_quiet(ildv: np.ndarray, iccv: np.ndarray, pgm: np.ndarray) -> None:
    """Quiet frames take the power-weighted mean of the group's active frames."""
    act_v = pgm >= pgm.max() * 10 ** -4.5
    for gv in range(len(pgm)):
        wv = pgm[gv] * act_v[gv]
        if wv.sum() <= 0:
            ildv[gv], iccv[gv] = 0.0, 1.0
        else:
            ildv[gv, ~act_v[gv]] = np.sum(wv * ildv[gv]) / wv.sum()
            iccv[gv, ~act_v[gv]] = np.sum(wv * iccv[gv]) / wv.sum()


# ================================================================ keyframes

def _greedy_keys(tv, ymat, wmat, tolv, max_keys, required) -> list[int]:
    T = len(tv)
    if T <= 2:
        return list(range(T))
    wmat = np.ones_like(ymat) if wmat is None else wmat
    swv = wmat.sum(axis=1)
    denom = np.maximum(swv, 0.25 * swv.max()) if swv.max() > 0 else np.ones(T)
    key_l = sorted(set([0, T - 1] + [int(rv) for rv in required]))
    while len(key_l) < max_keys:
        ii = _worst_frame(tv, ymat, wmat, denom, key_l, tolv)
        if ii is None:
            break
        key_l = sorted(key_l + [ii])
    return key_l


def _worst_frame(tv, ymat, wmat, denom, key_l, tolv) -> int | None:
    """The frame worst fit by interpolation between keys, or None when every error is within tol."""
    kset = np.array(key_l)
    Yh = np.array([np.interp(tv, tv[kset], ymat[kset, cv]) for cv in range(ymat.shape[1])]).T
    errv = np.sqrt(np.sum(wmat * (Yh - ymat) ** 2, axis=1) / denom)
    errv[kset] = 0.0
    ii = int(np.argmax(errv))
    return None if errv[ii] <= tolv else ii


def _keyframes(gn, stv, bwv, cfg, wandr) -> tuple[list[int], np.ndarray]:
    tv = np.arange(gn.shape[1]) * HOP / SR
    rows, Ws = [gn.T], [_cell_weights(gn, bwv, cfg["quiet_db"]).T]
    if stv is not None:
        rows += [stv["ild"].T / 2, stv["icc"].T * 10]
        Ws += [_stereo_key_weight(stv)] * 2
    totv = db(np.sum(10 ** (gn / 10), axis=0))
    pkv = int(np.argmax(totv))
    start_i = int(np.argmax(totv >= totv[pkv] - 30))
    return _greedy_keys(tv, np.hstack(rows), np.hstack(Ws), max(cfg["tol_db"], wandr), cfg["max_keys"],
                        [pkv, start_i]), tv


def _stereo_key_weight(stv: dict) -> np.ndarray:
    lvl = stv["level"]
    lw = np.clip((lvl - (lvl.max() - 40)) / 20, 0, 1)
    return 0.5 * lw[:, None] * stv["share"][None, :] * 7


# ================================================================ fine curve

def _psd_grid(x: np.ndarray, gridv: np.ndarray) -> np.ndarray:
    from scipy.signal import welch
    nn = min(4096, 2 ** int(np.floor(np.log2(max(len(x), 256)))))
    fq, pv = welch(x, SR, nperseg=nn, noverlap=3 * nn // 4, window="hann")
    acc = np.zeros(len(gridv))
    for ii, gv in enumerate(gridv):
        sv = (fq >= gv * 2 ** (-1 / 48)) & (fq <= gv * 2 ** (1 / 48))
        acc[ii] = pv[sv].mean() if sv.any() else pv[int(np.argmin(np.abs(fq - gv)))]
    return acc


def _fine(mv: np.ndarray, model: dict, cent_v) -> dict | None:
    gridv = 40.0 * 2 ** (np.arange(0, int(24 * np.log2(20000 / 40)) + 1) / 24)
    pov = _psd_grid(mv, gridv)
    pr = np.mean([_psd_grid(_render_mono(model, sv), gridv) for sv in (41, 42)], axis=0)
    dv = db(pov) - db(pr)
    dv = dv - uniform_filter1d(dv, 9, mode="nearest")
    dv = np.clip(dv, -12, 12)
    dv[db(pov) < db(pov).max() - 60] = 0.0
    dv *= np.clip((gridv - 150) / 150, 0, 1)
    if np.sqrt(np.mean(dv ** 2)) < 0.4:
        return None
    return {"f0": 40.0, "step": 1 / 24, "db": [round(float(vv), 2) for vv in dv]}


def _render_mono(model: dict, seed: int, txt_v=True, rsig=None) -> np.ndarray:
    envl = model["env"]
    cnt = max(64, int(round(model["dur"] * SR)))
    return _synth(cnt, np.asarray(envl["t"]), envl["db"], centers(model["bands"]["n"]), seed,
                  texture=model.get("texture") if txt_v else None,
                  ridges=model.get("ridges") if rsig is None else None,
                  fine=model.get("fine"), ridge_sig=rsig)


# ================================================================ texture statistics

def _group_env(model: dict, cnt: int) -> np.ndarray:
    cent_v = centers(model["bands"]["n"])
    bwv = bandwidths(cent_v)
    tv = np.arange(cnt // 48 + 1) * 48 / SR
    envl = model["env"]
    dv = np.array([np.interp(tv, envl["t"], np.asarray(envl["db"])[:, bv]) for bv in range(len(cent_v))])
    gv = _group_of(cent_v)
    acc = np.zeros((len(GROUPS) - 1, len(tv)))
    for ix in range(len(acc)):
        sv = gv == ix
        if sv.any():
            acc[ix] = np.sqrt(np.sum(10 ** (dv[sv] / 10) * bwv[sv, None], axis=0) / (SR / 2))
    return acc


def _rate_spectrum(rv: np.ndarray, wv: np.ndarray) -> np.ndarray:
    muv = np.sum(wv * rv) / max(wv.sum(), 1e-12)
    zv = (rv - muv) * wv
    varv = np.sum(wv * (rv - muv) ** 2) / max(wv.sum(), 1e-12)
    pmat = np.abs(np.fft.rfft(zv)) ** 2
    fq = np.fft.rfftfreq(len(zv), 48 / SR)
    specv = np.array([pmat[(fq >= cv * 2 ** -0.25) & (fq < cv * 2 ** 0.25)].sum() for cv in RATES])
    totv = pmat[1:].sum()
    return specv / totv * varv if totv > 0 else specv


def _group_stats(yv: np.ndarray, g_env: np.ndarray, ons_s: float) -> list:
    cnt = len(yv)
    fft_n = next_fast_len(cnt)
    ymat = rfft(yv, fft_n)
    fq = rfftfreq(fft_n, 1 / SR)
    acc = []
    frac = (g_env ** 2).sum(axis=1)
    frac = frac / max(frac.sum(), 1e-300)
    for gv, (low_i, high_i) in enumerate(zip(GROUPS[:-1], GROUPS[1:])):
        acc.append(_one_group(ymat, fq, fft_n, cnt, low_i, high_i, g_env[gv], ons_s) if frac[gv] >= 0.004 else None)
    return acc


def _one_group(ymat, fq, fft_n, cnt, low_i, high_i, envl, ons_s) -> dict | None:
    ybv = irfft(ymat * ((fq >= low_i) & (fq < high_i)), fft_n)[:cnt]
    mv = min(len(envl), cnt // 48)
    av = np.sqrt(np.mean(ybv[:mv * 48].reshape(mv, 48) ** 2, axis=1))
    ev = envl[:mv]
    tv = np.arange(mv) * 48 / SR
    act_v = (ev >= ev.max() * 10 ** -1.5) & (tv > ons_s + 0.015) & (av > 0) & (ev > 0)
    return _active_stats(ybv, av, ev, tv, act_v) if act_v.sum() >= 24 else None


def _active_stats(ybv, av, ev, tv, act_v) -> dict:
    rv = np.where(act_v, np.log(np.maximum(av, 1e-30)) - np.log(np.maximum(ev, 1e-30)), 0.0)
    wv = uniform_filter1d(act_v.astype(float), 5) * act_v
    return {"spec": _rate_spectrum(rv, wv), "halves": _halves(rv, wv, act_v), "lenv": _lenv(ybv, tv, ev, act_v),
            "low": _low_part(rv, wv) * act_v, "act": act_v}


def _halves(rv: np.ndarray, wv: np.ndarray, act_v: np.ndarray) -> list:
    mv = len(act_v)
    sel_i = np.flatnonzero(act_v)
    half = sel_i[len(sel_i) // 2] if len(sel_i) else 0
    acc = []
    for pick in (np.arange(mv) < half, np.arange(mv) >= half):
        acc.append(_rate_spectrum(rv, wv * pick) if (act_v & pick).sum() >= 12 else None)
    return acc


def _lenv(ybv: np.ndarray, tv: np.ndarray, ev: np.ndarray, act_v: np.ndarray) -> float:
    """Std of the log Hilbert envelope over the band envelope, on active frames."""
    from scipy.signal import hilbert
    cnt = len(ybv)
    ae = np.abs(hilbert(ybv))
    eu = np.interp(np.arange(cnt), tv, ev)
    start_s = np.repeat(act_v, 48)[:cnt]
    start_s = np.pad(start_s, (0, cnt - len(start_s)))
    return float(np.std(np.log(np.maximum(ae[start_s], 1e-30)) - np.log(np.maximum(eu[start_s], 1e-30))))


def _low_part(rv: np.ndarray, wv: np.ndarray) -> np.ndarray:
    return uniform_filter1d(rv - np.sum(wv * rv) / max(wv.sum(), 1e-12), 15) if len(rv) > 15 else rv


def _fast(stv: dict) -> float:
    return float(np.sum(stv["spec"][np.asarray(RATES) >= 128]))


def _texture_stats(model, y_list, ons_s, cnt):
    g_env = _group_env(model, cnt)
    return [_group_stats(yv, g_env, ons_s) for yv in y_list]


def _nanmean(vals) -> float:
    vv = [x for x in vals if x is not None and np.isfinite(x)]
    return float(np.mean(vv)) if vv else float("nan")


# ================================================================ texture fit

def _tex_renders(model, txt_v, ridge_sigs, cnt) -> list:
    mv = dict(model, texture=txt_v)
    return [_render_mono(mv, sv, True, rsv) for sv, rsv in zip((31, 32, 33), ridge_sigs)]


def _texture(mv: np.ndarray, model: dict, cfg: dict) -> dict | None:
    begin = _tex_context(mv, model, cfg)
    return None if begin is None else _tex_fit(*begin)


def _tex_context(mv: np.ndarray, model: dict, cfg: dict):
    """(context, plain-noise stats) for the texture fit, or None when no group has stats."""
    cnt = len(mv)
    start_i = _onset_frame(analyse(mv, centers(), False)["D"]) * HOP / SR
    rsv = [_ridge_wave(cnt, model.get("ridges"), _gens(sv)[4]) for sv in (31, 32, 33)]
    srm = _texture_stats(model, [mv], start_i, cnt)[0]
    smat0 = _texture_stats(model, _tex_renders(model, None, rsv, cnt), start_i, cnt)
    okmask = [gv for gv in range(len(srm)) if srm[gv] is not None and all(sv[gv] is not None for sv in smat0)]
    if not okmask:
        return None
    frac = (_group_env(model, cnt) ** 2).sum(axis=1)[okmask]
    gwv = dict(zip(okmask, frac / max(frac.sum(), 1e-300)))
    return {"m": mv, "model": model, "rs": rsv, "on": start_i, "n": cnt, "gw": gwv, "Sr": srm, "opts": cfg}, smat0


def _tex_fit(state: dict, smat0: list) -> dict | None:
    grain_l, stat_d = _grain_fit(state, smat0)
    basev = smat0 if grain_l is None else _tex_stats(state, {"grains": grain_l})
    amv = _am_fit(state, basev)
    if amv is not None and grain_l is not None:
        grain_l, _ = _grain_fit(state, smat0, amv)
        basev = _tex_stats(state, {"grains": grain_l})
        amv = _am_fit(state, basev) or amv
    if amv is not None:
        amv = _depth_fix(state, amv, grain_l, basev)
    return None if grain_l is None and amv is None else {"am": amv, "grains": grain_l, "stats": stat_d}


def _tex_stats(state: dict, txt_v: dict) -> list:
    return _texture_stats(state["model"], _tex_renders(state["model"], txt_v, state["rs"], state["n"]), state["on"], state["n"])


def _grain_fit(state, smat0, amv=None):
    stat_d, grain_q = _grain_stats(state, smat0)
    return (_grain_density(state, amv) if grain_q else None), stat_d


def _grain_stats(state, smat0) -> tuple[dict, bool]:
    """Fast-fluctuation stats of the recording against plain noise, and whether it is grainy."""
    frv = _fast_mix(state["gw"], state["Sr"])
    fg_seeds = [_fast_mix(state["gw"], sv) for sv in smat0]
    fgv = float(np.mean(fg_seeds))
    hratio = frv / max(fgv, 1e-12)
    stat_d = _lenv_stats(state, smat0)
    stat_d.update({"fast_var_real": round(frv, 5), "fast_var_gauss": round(fgv, 5), "fast_ratio": round(hratio, 3)})
    halves_ok, stat_d["fast_ratio_halves"] = _halves_ratio(state, smat0)
    grain_q = frv - fgv > max(0.03, 3 * float(np.std(fg_seeds))) and hratio > 1.5
    return stat_d, bool(grain_q and (halves_ok or not state["opts"]["grain_halves"]))


def _fast_mix(gwv: dict, stat_d: list) -> float:
    return sum(wv * _fast(stat_d[gv]) for gv, wv in gwv.items())


def _lenv_stats(state, smat0) -> dict:
    gwv, srm = state["gw"], state["Sr"]
    return {"lenv_real": _nanmean([srm[gv]["lenv"] for gv in gwv]),
            "lenv_gauss": _nanmean([sv[gv]["lenv"] for sv in smat0 for gv in gwv])}


def _halves_ratio(state, smat0):
    acc = [_half_ratio(state, smat0, hv) for hv in (0, 1)]
    return all(vv is not None and vv > 1.5 for vv in acc), acc


def _half_ratio(state, smat0, hv: int) -> float | None:
    num = sum(wv * _hfast(state["Sr"][gv]["halves"][hv]) for gv, wv in state["gw"].items())
    denom = sum(wv * _nanmean([_hfast(sv[gv]["halves"][hv]) for sv in smat0]) for gv, wv in state["gw"].items())
    okv = denom and np.isfinite(denom) and denom > 0
    return round(num / denom, 3) if okv else None


def _hfast(specv) -> float | None:
    if specv is None:
        return None
    return float(np.sum(specv[np.asarray(RATES) >= 128]))


def _grain_loss(state, density, amv) -> float:
    gv = {"density": float(density), "spread": 1.0}
    if state["opts"]["grain_norm_ms"] > 0:
        gv["norm_ms"] = state["opts"]["grain_norm_ms"]
    smat = _texture_stats(state["model"], _tex_renders(state["model"], {"grains": gv, "am": amv}, state["rs"], state["n"]),
                       state["on"], state["n"])
    loss = 0.0
    for giv, wv in state["gw"].items():
        have = _nanmean([sv[giv]["lenv"] if sv[giv] else None for sv in smat])
        if np.isfinite(have):
            loss += wv * (have - state["Sr"][giv]["lenv"]) ** 2
    return loss


def _grain_density(state, amv) -> dict:
    loss_l = {dv: _grain_loss(state, dv, amv) for dv in GRAIN_GRID}
    winner = min(loss_l, key=loss_l.get)
    ii = GRAIN_GRID.index(winner)
    for jj in (ii - 1, ii + 1):
        if 0 <= jj < len(GRAIN_GRID):
            dv = float(np.sqrt(winner * GRAIN_GRID[jj]))
            loss_l[dv] = _grain_loss(state, dv, amv)
    winner = min(loss_l, key=loss_l.get)
    gv = {"density": round(float(winner), 1), "spread": 1.0}
    if state["opts"]["grain_norm_ms"] > 0:
        gv["norm_ms"] = state["opts"]["grain_norm_ms"]
    return gv


def _am_fit(state, basev) -> dict | None:
    rate_l = np.asarray(RATES)
    dep, shape = _am_depths(state, basev, (rate_l >= 6) & (rate_l <= 64))
    big = [dv for gv, dv in enumerate(dep) if state["gw"].get(gv, 0) > 0.03]
    if not big or max(big) <= 0.08 or shape.sum() <= 0:
        return None
    return {"rates": [float(rv) for rv in rate_l], "spec": [round(float(vv), 6) for vv in shape / shape.sum()],
            "depth": [round(dv, 4) for dv in dep], "coh": round(_coherence(state, basev), 3)}


def _am_depths(state, basev, lowv) -> tuple[list[float], np.ndarray]:
    """AM depth per group and the weighted shape of the excess rate spectrum."""
    dep, shape = [], np.zeros(len(lowv))
    for gv in range(len(GROUPS) - 1):
        if gv in state["gw"]:
            exv = _am_excess(state["Sr"][gv], [bv[gv] for bv in basev], lowv)
            dep.append(float(min(np.sqrt(exv.sum()), 0.6)))
            shape += state["gw"][gv] * exv
        else:
            dep.append(0.0)
    return dep, shape


def _am_excess(real, base_l, lowv) -> np.ndarray:
    bsv = [bv for bv in base_l if bv is not None]
    exv = _halves_cap(_spec_excess(real, bsv, lowv), real, bsv) if bsv else None
    return np.zeros(len(lowv)) if exv is None else np.maximum(exv, 0.0)


def _spec_excess(real, bsv, lowv) -> np.ndarray:
    sb = np.mean([bv["spec"] for bv in bsv], axis=0)
    sdv = np.std([bv["spec"] for bv in bsv], axis=0)
    return np.where((real["spec"] > 1.25 * sb + 2 * sdv) & lowv, real["spec"] - sb, 0.0)


def _halves_cap(exv, real, bsv):
    """ex capped by the excess of each half, or None when a half has no stats."""
    for hv in (0, 1):
        hr = real["halves"][hv]
        hb = [bv["halves"][hv] for bv in bsv if bv["halves"][hv] is not None]
        if hr is None or not hb:
            return None
        exv = np.minimum(exv, np.maximum(hr - 1.25 * np.mean(hb, axis=0), 0.0))
    return exv


def _coherence(state, basev) -> float:
    topv = sorted(state["gw"], key=lambda gv: -state["gw"][gv])[:2]
    return 1.0 if len(topv) < 2 else _pair_coherence(state, basev, topv)


def _pair_coherence(state, basev, topv) -> float:
    av, bv = (state["Sr"][gv] for gv in topv)
    bothv = av["act"] & bv["act"]
    la = _low_excess(av, basev, topv[0])
    lb = _low_excess(bv, basev, topv[1])
    okv = bothv.sum() >= 24 and np.std(la[bothv]) != 0 and np.std(lb[bothv]) != 0
    return float(np.clip(np.corrcoef(la[bothv], lb[bothv])[0, 1], 0, 1)) if okv else 0.0


def _low_excess(stv: dict, basev: list, gv: int) -> np.ndarray:
    return stv["low"] - _nanmean_arr([bb[gv]["low"] for bb in basev if bb[gv]], len(stv["low"]))


def _nanmean_arr(arrs, mv) -> np.ndarray:
    return np.mean(arrs, axis=0)[:mv] if arrs else np.zeros(mv)


def _depth_fix(state, amv, grain_l, basev) -> dict:
    smat = _tex_stats(state, {"am": amv, "grains": grain_l})
    lowv = (np.asarray(RATES) >= 6) & (np.asarray(RATES) <= 64)
    acc = dict(amv)
    dep = list(amv["depth"])
    for gv in state["gw"]:
        dep[gv] = _fixed_depth(state["Sr"][gv], dep[gv], [bv[gv] for bv in basev], [sv[gv] for sv in smat], lowv)
    acc["depth"] = dep
    return acc


def _fixed_depth(real: dict, dv: float, base_l: list, rendered: list, lowv: np.ndarray) -> float:
    """Depth scaled so the rendered low-rate variance matches the recording's."""
    bvar = _nanmean([np.sum(bv["spec"][lowv]) for bv in base_l if bv])
    target = np.sum(real["spec"][lowv]) - bvar
    have = max(_nanmean([np.sum(sv["spec"][lowv]) for sv in rendered if sv]) - bvar, 1e-6)
    if np.isfinite(target) and np.isfinite(have):
        dv = round(float(min(dv * np.sqrt(np.clip(target / have, 0.25, 4)), 0.6)), 4)
    return dv


# ================================================================ fit

def _opts(**kw) -> dict:
    ov = {"n_bands": 48, "tol_db": 1.0, "max_keys": 160, "gesture_ms": 25, "max_rank": 16, "iters": 3,
         "diff_frames": 3, "ridges": True, "fine": False, "texture": True, "stereo": True,
         "seeds": (1, 2), "max_ridges": 4, "ridge_db": 8, "ridge_ms": 50, "ridge_lo": 400,
         "denoise": True, "adapt": True, "quiet_db": 50, "grain_norm_ms": 0, "grain_halves": False}
    ov.update(kw)
    return ov


def _level_db(av: np.ndarray) -> float:
    pv = np.mean(av ** 2, axis=1)
    wv = min(len(pv), int(0.05 * SR))
    if wv < 1:
        return -300.0
    return float(db(uniform_filter1d(pv, wv, mode="constant").max()))


def fit(x: Any, name: str | None = None, verbose: bool = False, sr: int = SR, **kw) -> dict:
    """A spectral noise model of a path or a 48 kHz array, (n,) or (n, 2)."""
    tm0 = time.time()
    cfg = _opts(**kw)
    av = as_stereo(x) if isinstance(x, str) else as_stereo(resample(np.asarray(x, float), sr))
    model = _fit_model(av, cfg, lambda: name or source_name(x))
    model["fit"]["seconds"] = round(time.time() - tm0, 3)
    if verbose:
        print(summary(model))
    return model


def _fit_model(av: np.ndarray, cfg: dict, name) -> dict:
    mv = av.mean(axis=1)
    cent_v = centers(cfg["n_bands"])
    shape = _fit_shape(av, mv, cent_v, cfg)
    model = _store(av, name(), len(mv), cent_v, shape, cfg)
    if cfg["fine"]:
        model["fine"] = _fine(mv, model, cent_v)
    if cfg["texture"]:
        model["texture"] = _texture(mv, model, cfg)
    return model


def _fit_shape(av: np.ndarray, mv: np.ndarray, cent_v, cfg: dict) -> dict:
    """Ridges, the gain keyframes and the stereo trajectory of the recording."""
    anv = analyse(av, cent_v, cfg["stereo"] and not is_mono_pair(av))
    rdg_l = _ridges(mv, cfg) if cfg["ridges"] and len(mv) >= 2048 else []
    _remove_ridges(anv["D"], rdg_l, cent_v)
    gv = _gain_fit(anv["D"], len(mv), cent_v, cfg)
    stv = _stereo_traj(anv["stereo"])
    key_l, tv = _keyframes(gv["gain"], stv, bandwidths(cent_v), cfg, gv["wander"])
    return dict(gv, st=stv, keys=key_l, t=tv, ridges=rdg_l)


def _gain_fit(dmat: np.ndarray, cnt: int, cent_v, cfg: dict) -> dict:
    smat, ix, wandr = _gesture(dmat, cfg)
    smat, rnk = _denoise(smat, cnt, ix, cent_v) if cfg["denoise"] else (smat, 0)
    gn = _gain_iter(smat, ix, _onset_frame(dmat), cnt, cent_v, smat > smat.max() - 70, cfg)
    return {"gain": gn, "k": ix, "rank": rnk, "wander": wandr}


def _store(av, name, cnt, cent_v, shv: dict, cfg) -> dict:
    key_l = shv["keys"]
    stereo = _stereo_keys(shv["st"], key_l)
    return {"kind": KIND, "version": VERSION, "source": name, "sr": SR, "dur": round(cnt / SR, 4),
            "level_db": round(_level_db(av), 2),
            "bands": {"n": len(cent_v), "f_lo": F_LO, "f_hi": F_HI, "scale": "ln(1+f/80)"},
            "fit": {"gesture_ms": 5 * shv["k"], "rank": shv["rank"], "tol_db": cfg["tol_db"],
                    "fluct_db": round(shv["wander"], 3)},
            "env": {"t": [round(float(shv["t"][ii]), 4) for ii in key_l],
                    "db": np.round(shv["gain"][:, key_l].T, 1).tolist()},
            "stereo": stereo, "fine": None, "texture": None, "ridges": shv["ridges"], "finish": True,
            "channels": 2 if stereo else 1}


def _stereo_keys(stv: dict | None, key_l) -> dict | None:
    if stv is None:
        return None
    return {"groups": list(GROUPS), "ild": np.round(stv["ild"][:, key_l].T, 2).tolist(),
            "icc": np.round(stv["icc"][:, key_l].T, 3).tolist()}


# ================================================================ render

def _centroids(key_db: np.ndarray, cent_v) -> tuple[np.ndarray, float]:
    pv = 10 ** (key_db / 10) * bandwidths(cent_v)[None, :]
    wv = (pv / np.maximum(pv.max(axis=1, keepdims=True), 1e-300)) ** 0.3
    cv = np.sum(wv * np.log2(cent_v)[None, :], axis=1) / np.maximum(wv.sum(axis=1), 1e-300)
    kp = pv.sum(axis=1)
    return cv, float(np.sum(kp * cv) / max(kp.sum(), 1e-300))


def _shift_keys(key_db, cent_v, ov: np.ndarray) -> np.ndarray:
    ecv = warp(cent_v)
    acc = key_db.copy()
    for ii, oi in enumerate(ov):
        if oi != 0:
            acc[ii] = np.interp(warp(cent_v / 2 ** oi), ecv, key_db[ii])
    return acc


def _edit_keys(model, key_db, cent_v, octv, swp, tilt, proximity):
    cv, mean = _centroids(key_db, cent_v)
    ov = octv + (swp - 1) * (cv - mean)
    if np.any(ov != 0):
        key_db = _shift_keys(key_db, cent_v, ov)
    key_db = key_db + tilt * np.log2(cent_v / 1000)[None, :]
    if proximity:
        lvl = db(np.sum(10 ** (key_db / 10) * bandwidths(cent_v), axis=1))
        dv = np.clip(lvl - lvl.max(), -40, 0)
        key_db = key_db + proximity * dv[:, None] * np.maximum(0.5 + 0.15 * np.log2(cent_v / 1000), 0)[None, :]
    return key_db, cv, mean


def _edit_ridges(rdg_l, sv, octv, swp, cv, mean, key_t, gn):
    if not rdg_l or gn == 0:
        return []
    acc = []
    for rv in rdg_l:
        rtv = np.asarray(rv["t"]) * sv
        civ = np.interp(rtv, key_t, cv)
        fq = np.minimum(np.asarray(rv["f"]) * 2 ** (octv + (swp - 1) * (civ - mean)), 0.45 * SR)
        acc.append({"t": rtv.tolist(), "f": fq.tolist(), "db": (np.asarray(rv["db"]) + 20 * np.log10(gn)).tolist(),
                    "jitter": rv["jitter"]})
    return acc


def _edit_stereo(stv, width, panv):
    if stv is None:
        return None
    iccv = np.clip(1 - width * (1 - np.asarray(stv["icc"])), -1, 1)
    return {"icc": iccv, "ild": np.asarray(stv["ild"]) * width * panv}


def _peak50(pmat: np.ndarray) -> float:
    wv = min(len(pmat), 10)
    return float(db(uniform_filter1d(pmat, wv, mode="constant").max())) if len(pmat) else -300.0


def _take_gain(yv: np.ndarray, Pk: np.ndarray, cnt: int) -> float:
    from hark.core.features import k_weight
    yyv = yv if yv.ndim == 2 else yv[:, None]
    have = float(np.mean([np.sum(np.asarray(k_weight(yyv[:, cv], SR)) ** 2) for cv in range(yyv.shape[1])]))
    target = float(np.sum(np.interp(np.arange(cnt), np.arange(len(Pk)) * HOP, Pk)))
    return float(np.sqrt(target / have)) if have > 0 else 1.0


RENDER_DEFAULTS = {"length": 1, "speed": 1, "sweep": 1, "shift": 0, "tilt": 0, "roughness": 1, "width": 1,
                   "ridges": 1, "gain_db": 0, "doppler": 0, "proximity": 0, "pan": 1, "motion": 0,
                   "level": True, "limit": True, "steady": True}


def render(model: dict, seed: int = 0, **knobs) -> np.ndarray:
    """A take of the model: (n, 2) float32 at 48 kHz. Knobs and defaults: RENDER_DEFAULTS."""
    check(model)
    ix = merge_knobs(RENDER_DEFAULTS, knobs, "spectral render")
    return _render_knobs(model, seed, ix).astype(np.float32)


def _render_knobs(model: dict, seed: int, ix: dict) -> np.ndarray:
    sv, cnt, key_t, octv = _take_time(model, ix)
    cent_v = centers(model["bands"]["n"])
    key_db, rdv, stv = _edited(model, ix, sv, key_t, cent_v, octv)
    yv, pmat = _synth(cnt, key_t, key_db, cent_v, seed, stereo=stv, texture=model.get("texture"), ridges=rdv, mono=False,
                  rate_scale=1 / sv, roughness=ix["roughness"], fine=model.get("fine"), fine_shift=octv,
                  power=True)
    yv = _level(yv, pmat, model, ix["gain_db"], ix["level"], ix["steady"], cnt)
    if ix["doppler"] or ix["motion"]:
        yv = _motion(yv, model, sv, ix["doppler"], ix["motion"])
    return _peak_cap(yv) if ix["limit"] else yv


def _take_time(model: dict, ix: dict) -> tuple[float, int, np.ndarray, float]:
    """Time stretch, sample count, key times and pitch shift (octaves) of a take."""
    speed = max(ix["speed"], 0.001)
    sv = max(ix["length"], 0.001) / speed
    cnt = max(64, int(round(model["dur"] * sv * SR)))
    key_t = np.asarray(model["env"]["t"], dtype=float) * sv
    return sv, cnt, key_t, ix["shift"] + 0.5 * np.log2(speed)


def _edited(model: dict, ix: dict, sv: float, key_t: np.ndarray, cent_v: np.ndarray, octv: float):
    """Edited keyframes, ridges and stereo for a take."""
    key_db, cv, mean = _edit_keys(model, np.asarray(model["env"]["db"], float), cent_v, octv, ix["sweep"], ix["tilt"],
                              ix["proximity"])
    rdv = _edit_ridges(model.get("ridges"), sv, octv, ix["sweep"], cv, mean, key_t, ix["ridges"])
    return key_db, rdv, _edit_stereo(model.get("stereo"), ix["width"], ix["pan"])


def _peak_cap(yv: np.ndarray) -> np.ndarray:
    """Scale y down so its peak is at most -0.5 dBFS."""
    pkv = float(np.max(np.abs(yv))) if yv.size else 0.0
    cap = 10 ** (-0.5 / 20)
    return yv * (cap / pkv) if pkv > cap else yv


def _level(yv, pmat, model, gain_db, level, steady, cnt):
    if not level:
        return yv
    if steady:
        gv = 10 ** ((model["level_db"] + gain_db - _peak50(pmat["p"])) / 20) * _take_gain(yv, pmat["k"], cnt)
        return yv * gv
    pv = np.mean(yv ** 2, axis=1)
    wv = min(len(pv), int(0.05 * SR))
    pkv = float(db(uniform_filter1d(pv, wv, mode="constant").max()))
    return yv * 10 ** ((model["level_db"] + gain_db - pkv) / 20)


def _motion(yv, model, sv, doppler, motion):
    trk = trajectory(model)
    tpk = trk["peak_s"] * sv
    tauv = max(0.015, 0.25 * (trk["rise_s"] + trk["fall_s"]) * sv)
    if doppler:
        yv = bend(yv, tpk, tauv, doppler)
    if motion:
        yv = pass_pan(yv, tpk, tauv, motion)
    return yv


def bend(y: np.ndarray, t_pk: float, tau: float, semitones: float) -> np.ndarray:
    """A pass-by pitch bend as a time warp: higher while it approaches, lower as it recedes."""
    y = np.asarray(y, dtype=float)
    posv = _bend_pos(len(y), t_pk, tau, semitones)
    src = np.arange(len(y))
    if y.ndim == 1:
        return np.interp(posv, src, y, left=0.0, right=0.0)
    return np.column_stack([np.interp(posv, src, y[:, cv], left=0.0, right=0.0) for cv in range(y.shape[1])])


def _bend_pos(cnt: int, tpk: float, tauv: float, semitones: float) -> np.ndarray:
    """Source read positions of the warp, aligned at the peak and cut at the source end."""
    mv = int(np.ceil(cnt * 2 ** (abs(semitones) / 12))) + 2
    tv = np.arange(mv) / SR
    ratev = 2 ** (semitones * np.tanh((tpk - tv) / max(tauv, 1e-9)) / 12)
    posv = np.concatenate([[0.0], np.cumsum(ratev)[:-1]])
    ix = min(int(round(tpk * SR)), mv - 1)
    posv = posv - posv[ix] + tpk * SR
    stop = np.flatnonzero(posv > cnt - 1)
    return posv[:int(stop[0]) if len(stop) else mv]


def pass_pan(y: np.ndarray, t_pk: float, tau: float, amount: float) -> np.ndarray:
    """A constant-power pass-by pan, unity in the centre."""
    y = np.asarray(y, dtype=float)
    yyv = np.column_stack([y, y]) if y.ndim == 1 else y
    tv = np.arange(len(yyv)) / SR
    posv = np.clip(amount * np.tanh((tv - t_pk) / max(tau, 1e-9)), -1, 1)
    ang = (posv + 1) * np.pi / 4
    return np.column_stack([yyv[:, 0] * np.sqrt(2) * np.cos(ang), yyv[:, 1] * np.sqrt(2) * np.sin(ang)])


# ================================================================ trajectory, summary

def trajectory(model: dict) -> dict:
    """The gesture's motion per keyframe."""
    cent_v = centers(model["bands"]["n"])
    key_db = np.asarray(model["env"]["db"], dtype=float)
    acc = _traj_core(np.asarray(model["env"]["t"], dtype=float), key_db, cent_v)
    acc.update(_traj_stereo(model.get("stereo"), cent_v, key_db))
    return acc


def _traj_core(tv: np.ndarray, key_db: np.ndarray, cent_v) -> dict:
    lvl = db((10 ** (key_db / 10) * bandwidths(cent_v)[None, :]).sum(axis=1))
    cv, _ = _centroids(key_db, cent_v)
    pkv, first, last = _peak_span(lvl)
    return {"t": tv.tolist(), "level_db": np.round(lvl, 2).tolist(), "centroid_hz": np.round(2 ** cv, 1).tolist(),
            "tilt_db_oct": [round(_tilt(key_db[ii], cent_v), 2) for ii in range(len(tv))],
            "peak_s": float(tv[pkv]), "rise_s": float(tv[pkv] - tv[first]), "fall_s": float(tv[last] - tv[pkv])}


def _peak_span(lvl: np.ndarray) -> tuple[int, int, int]:
    """Peak key and the first and last keys within 20 dB of it."""
    pkv = int(np.argmax(lvl))
    nearv = lvl >= lvl[pkv] - 20
    return pkv, int(np.argmax(nearv)), pkv + int(np.flatnonzero(nearv[pkv:])[-1])


def _tilt(rw: np.ndarray, cent_v) -> float:
    pick = rw >= rw.max() - 40
    if pick.sum() < 2:
        return 0.0
    wv = 10 ** ((rw[pick] - rw.max()) / 10)
    return float(np.polyfit(np.log2(cent_v[pick] / 1000), rw[pick], 1, w=np.sqrt(wv))[0])


def _traj_stereo(stv, cent_v, key_db) -> dict:
    if not stv:
        return {"pan_db": [0.0] * len(key_db), "width": [0.0] * len(key_db)}
    gv = _group_of(cent_v)
    pv = 10 ** (key_db / 10) * bandwidths(cent_v)[None, :]
    gp = np.array([pv[:, gv == ix].sum(axis=1) for ix in range(len(GROUPS) - 1)]).T
    wv = gp / np.maximum(gp.sum(axis=1, keepdims=True), 1e-300)
    ildv, iccv = np.asarray(stv["ild"]), np.asarray(stv["icc"])
    return {"pan_db": np.round((wv * ildv).sum(axis=1), 2).tolist(),
            "width": np.round(1 - (wv * iccv).sum(axis=1), 3).tolist()}


def summary(model: dict) -> str:
    """One paragraph of words."""
    trk = trajectory(model)
    lvl = np.asarray(trk["level_db"])
    pkv = int(np.argmax(lvl))
    nearv = np.asarray(trk["t"])[lvl >= lvl[pkv] - 20]
    cenv = trk["centroid_hz"]
    pieces = [f"{model.get('source') or 'array'}: {model['dur'] * 1000:.0f} ms noise gesture, "
             f"{len(trk['t'])} keyframes, rises {trk['rise_s'] * 1000:.0f} ms to its peak and stays within "
             f"20 dB for {(nearv[-1] - nearv[0]) * 1000:.0f} ms; centroid {cenv[0]:.0f} Hz at onset, "
             f"{cenv[pkv]:.0f} Hz at peak, {cenv[-1]:.0f} Hz at end"]
    pieces.append(_stereo_words(model.get("stereo"), trk))
    pieces.append(_texture_words(model.get("texture")))
    for rv in (model.get("ridges") or [])[:4]:
        pieces.append(f"ridge {rv['f'][0]:.0f} to {rv['f'][-1]:.0f} Hz")
    import json
    pieces.append(f"{len(json.dumps(model, separators=(',', ':'))) / 1024:.1f} kB")
    return "; ".join(pv for pv in pieces if pv) + "."


def _stereo_words(stv, trk) -> str:
    if not stv:
        return "mono"
    iccv = np.asarray(stv["icc"])
    panv = trk["pan_db"]
    return (f"stereo (correlation median {np.median(iccv):.2f}, minimum {iccv.min():.2f}, pan "
            f"{min(panv):+.1f} to {max(panv):+.1f} dB)")


def _texture_words(texv) -> str:
    if not texv:
        return ""
    acc = []
    amv = texv.get("am")
    if amv:
        ratev = amv["rates"][int(np.argmax(amv["spec"]))]
        acc.append(f"flutter {ratev:.0f} Hz, depth {8.686 * max(amv['depth']):.1f} dB, coherence {amv['coh']:.2f}")
    if texv.get("grains"):
        acc.append(f"crackle {texv['grains']['density']:.0f} per second")
    return ", ".join(acc)


# ================================================================ face

def check(model: Any) -> None:
    """ValueError when the model is not a spectral_noise model of a supported version."""
    check_kind(model, KIND, VERSION)


def save(model: dict, path: str) -> None:
    """Write compact JSON."""
    save_json(model, path)


def load(path: str) -> dict:
    """Read a model and check its kind."""
    mv = load_json(path)
    check(mv)
    return mv


def to_code(model: dict, var: str = "MODEL") -> str:
    """Readable Python that holds the model and renders it through hark.fit.analysis.spectral."""
    import pprint
    lite = copy.deepcopy({ix: vv for ix, vv in model.items() if ix != "fit_engine"})
    return ('"""Spectral noise model: renders through hark.fit.analysis.spectral."""\n\n'
            "import numpy as np\n\nfrom hark.fit.analysis import spectral\n\n"
            f"{var} = " + pprint.pformat(lite, width=110, compact=True) + "\n\n\n"
            f"def render(seed=0, **knobs):\n    return spectral.render({var}, seed, **knobs)\n")


# ================================================================ compare

def _trim(x: np.ndarray) -> np.ndarray:
    ev = db(np.convolve(x ** 2, np.ones(48) / 48, mode="same")[::48])
    ix = int(np.argmax(ev >= ev.max() - 30)) if len(ev) else 0
    return x[max(0, ix * 48 - 240):]


def _lev50(x: np.ndarray) -> np.ndarray:
    wv = min(len(x), 2400)
    pv = uniform_filter1d(x ** 2, max(wv, 1), mode="constant").max() if len(x) else 0.0
    return x / np.sqrt(pv) if pv > 0 else x


def _tone_profile(x: np.ndarray) -> np.ndarray:
    cv = 1000 * 2 ** (np.arange(-15, 13) / 3)
    xmat = np.abs(rfft(x)) ** 2
    fq = rfftfreq(len(x), 1 / SR)
    pv = np.array([xmat[(fq >= civ * 2 ** (-1 / 6)) & (fq < civ * 2 ** (1 / 6))].sum() for civ in cv])
    return np.maximum(db(pv / max(pv.sum(), 1e-300)), -60)


def _attack_ms(x: np.ndarray) -> float:
    ev = db(uniform_filter1d(np.convolve(x ** 2, np.ones(96) / 96, mode="same")[::96], 3))
    pkv = int(np.argmax(ev))
    return float((pkv - int(np.argmax(ev >= ev[pkv] - 20))) * 2.0)


def compare(a: Any, b: Any, clap: bool = False) -> dict:
    """The fitter's own distance set: mel, envelope correlation, attack, tone, lengths, optional CLAP."""
    sig_a, sig_b, tma, tmb = _compare_inputs(a, b)
    acc = {"mel_db": round(mel_db_distance(tma, tmb)[0], 3), "env_corr": round(_env_corr5(tma, tmb), 4),
           "attack_ms": [_attack_ms(tma), _attack_ms(tmb)], "tone_db": _tone_db(sig_a, sig_b),
           "len_s": [round(len(sig_a) / SR, 4), round(len(sig_b) / SR, 4)]}
    if clap:
        acc["clap"] = _clap_sim(sig_a, sig_b)
    return acc


def _compare_inputs(av: Any, bv: Any):
    """Channel means of a and b, and their trimmed, level-matched copies padded to one length."""
    sig_a, sig_b = (np.asarray(vv, float).mean(axis=1) if np.ndim(vv) == 2 else np.asarray(vv, float) for vv in (av, bv))
    tma, tmb = _lev50(_trim(sig_a)), _lev50(_trim(sig_b))
    cnt = max(len(tma), len(tmb))
    return sig_a, sig_b, np.pad(tma, (0, cnt - len(tma))), np.pad(tmb, (0, cnt - len(tmb)))


def _tone_db(sig_a: np.ndarray, sig_b: np.ndarray) -> float:
    return round(float(np.sqrt(np.mean((_tone_profile(sig_a) - _tone_profile(sig_b)) ** 2))), 3)


def _env_corr5(tma: np.ndarray, tmb: np.ndarray) -> float:
    """Correlation of the 5 ms level envelopes over frames within 60 dB of a's peak."""
    env_a = db(np.convolve(tma ** 2, np.ones(240) / 240, mode="same")[::240])
    env_b = db(np.convolve(tmb ** 2, np.ones(240) / 240, mode="same")[::240])
    flv = env_a.max() - 60
    act_v = (env_a > flv) | (env_b > flv)
    ecv = float(np.corrcoef(np.maximum(env_a, flv)[act_v], np.maximum(env_b, flv)[act_v])[0, 1]) if act_v.sum() > 3 else 0.0
    return ecv if np.isfinite(ecv) else 0.0


def _clap_sim(sig_a: np.ndarray, sig_b: np.ndarray) -> float:
    from hark.models.clap import get
    emat = get().embed_audio([sig_a.astype(np.float32), sig_b.astype(np.float32)])
    return round(float(emat[0] @ emat[1] / (np.linalg.norm(emat[0]) * np.linalg.norm(emat[1]))), 4)


def fit_report(x: Any, clap: bool = False, seeds=(11, 12), **fit_kw) -> dict:
    """Fit plus 2 renders: the model, the renders, the fit distance and the floor."""
    tm0 = time.time()
    av = as_stereo(x)
    model = fit(av, **fit_kw)
    rv = [render(model, sv) for sv in seeds]
    return {"model": model, "renders": rv, "orig": av, "fit": compare(av, rv[0], clap),
            "floor": compare(rv[0], rv[1], clap), "seconds": round(time.time() - tm0, 3)}
