"""Modal fitter: strikes of damped modes, a contact transient, a sub pulse and band noise.

The renderer lives in ``_kernels`` so that ``to_code`` can copy it into a
standalone script that renders the same samples.
"""

from __future__ import annotations

import copy
from typing import Any

import numpy as np
from scipy.fft import fft, ifft, next_fast_len, rfft
from scipy.signal import butter, fftconvolve, resample_poly, sosfiltfilt, stft

from hark.fit.analysis import _kernels as K
from hark.fit.analysis._common import (SR, as_stereo, check_kind, merge_knobs, db, is_mono_pair, load_json,
                                       mel_db_distance, rdp, resample, save_json, sig, source_name)

KIND = "modal"
NAME = "modal"
VERSION = 1

KNOBS: dict[str, tuple] = {
    "size": ((0.8, 1.25, "log"), 1.0, "Object size: mode frequencies / size, decays x size."),
    "damping": ((0.7, 1.43, "log"), 1.0, "Decays / damping."),
    "brightness": ((-3.0, 3.0), 0.0, "dB per octave about 1 kHz on modes and noise."),
    "hardness": ((0.7, 1.43, "log"), 1.0, "Contact: shorter and brighter attack above 1."),
    "noise": ((-6.0, 4.0), 0.0, "dB on the noise bands."),
    "width": ((0.5, 1.5), 1.0, "Factor on the side."),
    "room": ((0.0, 0.25), 0.0, "Wet of a small stochastic room."),
    "vary": ((0.0, 0.6), 0.0, "Seeded jitter of modes and noise."),
    "spacing": ((0.8, 1.25, "log"), 1.0, "Factor on the time between strikes."),
}

RENDER_KNOBS = ("size", "damping", "brightness", "hardness", "noise", "width", "room", "vary",
                "spacing", "gain", "stereo", "ring")

MODE_BANDS = (20, 60, 120, 200, 320, 500, 750, 1100, 1600, 2300, 3300, 4700, 6600, 9300, 13000,
              21500)
RAMPS_MS = (0.0, 0.05, 0.1, 0.2, 0.35, 0.5, 0.8, 1.2)
NOISE_EDGES = K.NOISE_EDGES
GRID = int(K.GRID_S * SR)

render_model = K.render_model

__all__ = ["KIND", "NAME", "VERSION", "KNOBS", "fit", "render", "check", "summary", "save", "load",
           "render_model", "onsets", "onset_strength", "fade_out", "esprit", "ls_modes",
           "band_masks", "band_centres", "to_code", "mel_distance", "env_corr", "rise_ms"]


def band_masks(n_fft: int, edges=NOISE_EDGES, h: float = 0.4) -> np.ndarray:
    """Power-complementary ERB-rate noise band masks on the rfft bins."""
    return K.erb_masks(n_fft, edges, h)


def band_centres(edges=NOISE_EDGES, top: float = 22000) -> np.ndarray:
    """Centres of the noise bands, Hz."""
    return K.erb_centres(edges, top)


# ================================================================ onsets

def onset_strength(m: np.ndarray, nper: int = 256, hop: int = 32,
                   cap: float = 1.5) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    """Capped log-spectral flux at lag 2, frame energy (dB) and frame centre samples."""
    m = np.asarray(m, dtype=float)
    xmat = np.abs(stft(m, nperseg=nper, noverlap=nper - hop, boundary="zeros", padded=True)[2])
    smat = np.log1p(xmat / (0.001 * xmat.max() + 1e-12))
    fluxv = np.zeros(smat.shape[1])
    if smat.shape[1] > 2:
        fluxv[2:] = np.sum(np.minimum(np.maximum(smat[:, 2:] - smat[:, :-2], 0.0), cap), axis=0)
    energy = db(np.sum(xmat ** 2, axis=0))
    return fluxv, energy, np.arange(smat.shape[1]) * hop


def onsets(x: np.ndarray, rel: float = 0.2, floor_db: float = -36, min_gap_ms: float = 12,
           max_n: int = 10) -> list[int]:
    """Strike onsets in samples."""
    x = np.asarray(x, dtype=float)
    acc = [0]
    if x.size and np.any(x):
        acc = _onsets_found(x, rel, floor_db, min_gap_ms, max_n)
    return acc


def _onsets_found(x: np.ndarray, relv: float, floor_db: float, min_gap_ms: float, n_cap: int) -> list[int]:
    fluxv, ev, cen = onset_strength(x)
    got = [[_refine_onset(x, int(cen[ix])), float(fluxv[ix])]
             for ix in _onset_cands(fluxv, ev, relv, floor_db) if not _drop_cand(x, int(cen[ix]))]
    got = _loudest(_filter_gaps(got, min_gap_ms), n_cap)
    if got:
        return [int(fq[0]) for fq in got]
    return [int(np.argmax(np.abs(x) >= 0.1 * np.max(np.abs(x))))]


def _loudest(got: list, n_cap: int) -> list:
    if len(got) <= n_cap:
        return got
    keep_m = sorted(np.argsort([-fq[1] for fq in got], kind="stable")[:n_cap])
    return [got[ii] for ii in keep_m]


def _onset_cands(fluxv: np.ndarray, ev: np.ndarray, relv: float, floor_db: float) -> list[int]:
    from scipy.ndimage import maximum_filter1d, minimum_filter1d
    f_max = float(fluxv.max())
    if f_max <= 0:
        return []
    loc = fluxv >= maximum_filter1d(fluxv, 13, mode="constant")
    aheadv = maximum_filter1d(ev, 9, origin=-4, mode="nearest")
    earlier = minimum_filter1d(ev, 9, origin=4, mode="nearest")
    want = np.where(fluxv >= 0.5 * f_max, 1.0, 3.0)
    good = loc & (fluxv >= relv * f_max) & (aheadv >= ev.max() + floor_db) & (aheadv - earlier >= want)
    return [int(ix) for ix in np.flatnonzero(good) if ix >= 2]


def _drop_cand(x: np.ndarray, cv: int) -> bool:
    if cv > len(x) - 192:
        return True
    if cv < 128:
        return not (np.max(np.abs(x[:240])) >= 0.25 * np.max(np.abs(x)))
    return False


def _refine_onset(x: np.ndarray, cv: int) -> int:
    av, bv = max(0, cv - 240), min(len(x), cv + 240)
    wv = x[av:bv]
    pkv = float(np.max(np.abs(wv))) if len(wv) else 0.0
    if len(wv) < 100 or pkv <= 0:
        return cv
    return av + _rise_at(wv, pkv)


def _rise_at(wv: np.ndarray, pkv: float) -> int:
    """Index of the strike start in w: the first loud sample after silence, else the first big step."""
    if np.sqrt(np.mean(wv[:96] ** 2)) < 0.05 * pkv:
        return int(np.argmax(np.abs(wv) >= 0.1 * pkv))
    dv = np.abs(np.diff(wv))
    thrv = min(max(0.3 * dv.max(), 4 * np.median(dv[:96])), dv.max())
    return int(np.argmax(dv >= thrv))


def _filter_gaps(got: list, min_gap_ms: float) -> list:
    gapv = min_gap_ms * SR / 1000.0
    acc: list = []
    for tv, flv in sorted(got, key=lambda fq: fq[0]):
        if acc and tv - acc[-1][0] < gapv:
            if flv > 2.5 * acc[-1][1]:
                acc[-1][1] = flv
            continue
        acc.append([tv, flv])
    return acc


# ================================================================ fade-out

def fade_out(x: np.ndarray, cands=(0.005, 0.01, 0.02, 0.03, 0.05, 0.1, 0.25),
             gain_db: float = 0.5) -> float:
    """Length (s) of a linear fade at the end of x, or 0 when there is none."""
    x = np.asarray(x, dtype=float)
    points = _tail_pts(x)
    return 0.0 if points is None else _fade_pick(*points, cands, gain_db)


def _tail_pts(x: np.ndarray):
    """Tail envelope points (t, env dB, length s, window s), or None when too short."""
    step_n = SR // 1000
    nhv = len(x) // step_n
    if nhv < 20:
        return None
    from scipy.ndimage import convolve1d
    from hark.fit.analysis._common import hann_norm
    pv = convolve1d(x ** 2, hann_norm(8 * step_n), mode="nearest")
    tv = (np.arange(nhv) * step_n + step_n // 2) / SR
    envl = db(pv[(tv * SR).astype(int)])
    tmat = len(x) / SR
    wmat = min(0.45 * tmat, 0.6)
    pick = (tv >= tmat - wmat) & (envl >= envl.max() - 80)
    return (tv[pick], envl[pick], tmat, wmat) if pick.sum() >= 10 else None


def _fade_pick(tsv, esv, tmat, wmat, cand_l, gain_db) -> float:
    basev = _ramp_err(tsv, esv, tmat, 0.0)
    winner, err_best = 0.0, basev
    for lrow in (cv for cv in cand_l if cv <= wmat / 2):
        errv = _ramp_err(tsv, esv, tmat, lrow)
        if errv < err_best:
            winner, err_best = lrow, errv
    return float(winner) if winner and err_best < basev - gain_db else 0.0


def _ramp_err(tv: np.ndarray, envl: np.ndarray, tmat: float, lrow: float) -> float:
    rv = 20 * np.log10(np.clip((tmat - tv) / lrow, 0.001, 1.0)) if lrow > 0 else np.zeros_like(tv)
    vv = envl - rv
    coefs = np.polyfit(tv, vv, 1)
    return float(np.mean(np.abs(vv - np.polyval(coefs, tv))))


def _undo_fade(mv: np.ndarray, fov: float) -> tuple[np.ndarray, int]:
    cnt = len(mv)
    n_frq = int(round(fov * SR))
    yv = mv.copy()
    if n_frq <= 0:
        return yv, cnt
    n_frq = min(n_frq, cnt)
    rmp = np.linspace(1.0, 0.0, n_frq)
    yv[cnt - n_frq:] = yv[cnt - n_frq:] / np.maximum(rmp, 0.25)
    return yv, cnt - int(0.25 * n_frq)


# ================================================================ ESPRIT

def esprit(s: np.ndarray, kmax: int = 16, dyn_db: float = 60) -> tuple[np.ndarray, int]:
    """Poles of a sum of damped exponentials, model order by MDL."""
    s = np.asarray(s)
    acc = (np.zeros(0, dtype=complex), 0)
    if len(s) >= 12:
        acc = _poles_esprit(s, kmax, dyn_db)
    return acc


def _poles_esprit(sgl: np.ndarray, k_max: int, dyn_db: float) -> tuple[np.ndarray, int]:
    nlen = len(sgl)
    lrow = max(4, nlen // 3)
    mcol = nlen - lrow + 1
    hank = np.lib.stride_tricks.sliding_window_view(sgl, mcol)[:lrow]
    try:
        umat, svals, _ = np.linalg.svd(hank, full_matrices=False)
    except np.linalg.LinAlgError:
        svals = np.zeros(0)
    ix = _order_mdl(svals, lrow, mcol, k_max, dyn_db)
    if ix == 0:
        return np.zeros(0, dtype=complex), 0
    uk_m = umat[:, :ix]
    phiv = np.linalg.lstsq(uk_m[:-1], uk_m[1:], rcond=None)[0]
    return np.linalg.eigvals(phiv), ix


def _order_mdl(svals: np.ndarray, pv: int, mcol: int, k_max: int, dyn_db: float) -> int:
    lamv = svals ** 2
    if not len(lamv) or lamv[0] <= 0:
        return 0
    capv = min(k_max, pv - 2, int(np.sum(svals > svals[0] * 10 ** (-dyn_db / 20))))
    winner, k_best = np.inf, 0
    for ix in range(max(capv, 0) + 1):
        remain = np.maximum(lamv[ix:], 1e-300)
        gv = np.exp(np.mean(np.log(remain)))
        av = np.mean(remain)
        vv = -mcol * (pv - ix) * np.log(gv / av) + 0.5 * ix * (2 * pv - ix) * np.log(mcol)
        if vv < winner:
            winner, k_best = vv, ix
    return k_best


# ================================================================ mode candidates

def _cands_per_band(chunk: np.ndarray, k_max: int, n_max: int) -> list[tuple[float, float]]:
    ns_v = len(chunk)
    if ns_v < 32:
        return []
    fft_n = next_fast_len(2 * ns_v)
    xmat = fft(chunk, fft_n)
    fq = np.fft.fftfreq(fft_n, 1.0 / SR)
    acc = []
    for low_i, high_i in zip(MODE_BANDS[:-1], MODE_BANDS[1:]):
        acc += _single_band(xmat, fq, ns_v, fft_n, low_i, high_i, k_max, n_max)
    return acc


def _mask_band(fq: np.ndarray, low_i: float, high_i: float, twv: float) -> np.ndarray:
    upv = np.clip((fq - (low_i - 2 * twv)) / twv, 0.0, 1.0)
    downv = np.clip(((high_i + 2 * twv) - fq) / twv, 0.0, 1.0)
    mv = (0.5 - 0.5 * np.cos(np.pi * upv)) * (0.5 - 0.5 * np.cos(np.pi * downv))
    return np.where(fq > 0, mv, 0.0)


def _single_band(xmat, fq, ns_v, fft_n, low_i, high_i, k_max, n_max) -> list[tuple[float, float]]:
    basev = _to_baseband(xmat, fq, ns_v, low_i, high_i)
    acc = []
    if basev is not None:
        dv, fcv, fsd = basev
        for wv in _windows_per_band(dv, n_max):
            acc += _modes_from_poles(esprit(wv, k_max)[0], fcv, fsd, low_i, high_i)
    return acc


def _to_baseband(xmat, fq, ns_v, low_i, high_i):
    """Band lo..hi shifted to 0 Hz and decimated: (signal, centre, rate), or None when too short."""
    bmat = high_i - low_i
    twv = max(0.15 * bmat, 8.0)
    dmat = max(1, min(int(SR / (1.15 * (bmat + 4 * twv))), ns_v // 48))
    if ns_v // dmat < 16:
        return None
    fcv = 0.5 * (low_i + high_i)
    av = ifft(xmat * _mask_band(fq, low_i, high_i, twv))[:ns_v]
    return (av * np.exp(-2j * np.pi * fcv * np.arange(ns_v) / SR))[::dmat], fcv, SR / dmat


def _windows_per_band(dv: np.ndarray, n_max: int) -> list[np.ndarray]:
    win_l = [dv[:n_max]]
    sv2 = 3 * n_max // 4
    if len(dv) > sv2 + n_max // 2 and np.mean(np.abs(dv[sv2:sv2 + n_max]) ** 2) > 1e-4 * np.mean(np.abs(dv[:n_max]) ** 2):
        win_l.append(dv[sv2:sv2 + n_max])
    return win_l


def _modes_from_poles(zv, fcv, fsd, low_i, high_i) -> list[tuple[float, float]]:
    rv = np.abs(zv)
    zv = zv[(rv > 1e-6) & (rv < 0.99999)]
    acc = []
    for pv in zv:
        frv = fcv + np.angle(pv) * fsd / (2 * np.pi)
        tauv = -1.0 / (np.log(abs(pv)) * fsd)
        if low_i <= frv < high_i and 0.0003 <= tauv < 30 and np.pi * frv * tauv >= 2:
            acc.append((float(frv), float(tauv)))
    return acc


def _duplicate(fq: float, tauv: float, kept_l: list) -> bool:
    for fev, tev in kept_l:
        tolv = max(0.3, 0.0015 * fq, 0.25 / (np.pi * np.sqrt(tauv * tev)))
        if abs(fq - fev) < tolv and abs(np.log(tauv / tev)) < 1.2:
            return True
    return False


def _unique_modes(cand_l: list, kept_l: list) -> list:
    poolv = list(kept_l)
    acc = []
    for fq, tauv in cand_l:
        if not _duplicate(fq, tauv, poolv):
            acc.append((fq, tauv))
            poolv.append((fq, tauv))
    return acc


# ================================================================ least squares

def _pole_set(freq_l, tau_l) -> np.ndarray:
    fq = np.asarray(freq_l, dtype=float)
    tv = np.asarray(tau_l, dtype=float)
    return np.exp(-1.0 / (tv * SR) + 2j * np.pi * fq / SR)


def _geo_mean(wv: np.ndarray, lrow: np.ndarray) -> np.ndarray:
    nearv = np.abs(1 - wv) < 1e-12
    safev = np.where(nearv, 0.5, 1 - wv)
    return np.where(nearv, lrow, (1 - wv ** lrow) / safev)


def _gram_mat(zv: np.ndarray, begins: np.ndarray, stop_i: int) -> np.ndarray:
    sgl = np.asarray(begins)
    latev = np.maximum.outer(sgl, sgl)
    lrow = np.maximum(stop_i - latev, 0).astype(float)
    pi = zv[:, None] ** (latev - sgl[:, None])
    pjv = zv[None, :] ** (latev - sgl[None, :])
    smat1 = pi * pjv * _geo_mean(np.multiply.outer(zv, zv), lrow)
    smat2 = pi * np.conj(pjv) * _geo_mean(np.multiply.outer(zv, np.conj(zv)), lrow)
    rr_m, ii_m = np.real(smat1 + smat2) / 2, np.real(smat2 - smat1) / 2
    ri_m, ir_m = np.imag(smat1 - smat2) / 2, np.imag(smat1 + smat2) / 2
    return np.block([[rr_m, ri_m], [ir_m, ii_m]])


def _projection(mv: np.ndarray, zv: np.ndarray, begins: np.ndarray, tau_l, stop_i: int) -> np.ndarray:
    pmat = np.zeros(len(zv), dtype=complex)
    tau_l = np.asarray(tau_l, dtype=float)
    for sgl in np.unique(begins):
        sel_i = np.flatnonzero(begins == sgl)
        stop_v = min(stop_i, int(sgl + 16 * tau_l[sel_i].max() * SR) + 1, len(mv))
        lzv = np.log(zv[sel_i])
        for cv0 in range(int(sgl), stop_v, 8192):
            cv1 = min(stop_v, cv0 + 8192)
            ix = np.arange(cv0 - sgl, cv1 - sgl)
            pmat[sel_i] += mv[cv0:cv1] @ np.exp(np.multiply.outer(ix, lzv))
    return np.concatenate([pmat.real, pmat.imag])


def _lstsq_solve(gmat: np.ndarray, bv: np.ndarray, lamv: float) -> np.ndarray:
    amat = gmat + lamv * np.diag(np.diag(gmat))
    try:
        return np.linalg.solve(amat, bv)
    except np.linalg.LinAlgError:
        return np.linalg.lstsq(amat, bv, rcond=None)[0]


def _quad_form(gmat: np.ndarray, x: np.ndarray, sel_i: np.ndarray, ix: int) -> float:
    jx = np.concatenate([sel_i, sel_i + ix])
    return float(x[jx] @ gmat[np.ix_(jx, jx)] @ x[jx])


def _cancelling(gmat: np.ndarray, x: np.ndarray, begins: np.ndarray, crs: float) -> bool:
    ix = len(begins)
    tot = float(x @ gmat @ x)
    hit_sum = 0.0
    for sgl in np.unique(begins):
        sel_i = np.flatnonzero(begins == sgl)
        jnt = _quad_form(gmat, x, sel_i, ix)
        sepv = sum(_quad_form(gmat, x, np.array([ii]), ix) for ii in sel_i)
        if sepv > 2 * max(jnt, 1e-300):
            return True
        hit_sum += jnt
    return len(np.unique(begins)) > 1 and hit_sum > crs * max(tot, 1e-300)


def _lsq(mv, begins, freq_l, tau_l, stop_i, rdg, crs):
    begins = np.asarray(begins, dtype=int)
    zv = _pole_set(freq_l, tau_l)
    gmat = _gram_mat(zv, begins, stop_i)
    bv = _projection(mv, zv, begins, tau_l, stop_i)
    lamv = rdg
    x = _lstsq_solve(gmat, bv, lamv)
    for _ in range(5):
        if not _cancelling(gmat, x, begins, crs):
            break
        lamv *= 10
        x = _lstsq_solve(gmat, bv, lamv)
    ix = len(begins)
    return x[:ix] - 1j * x[ix:], gmat, bv, x


def ls_modes(m: np.ndarray, starts, freqs, taus, end: int | None = None, ridge: float = 1e-5,
             cross: float = 10) -> np.ndarray:
    """Complex amplitudes of damped modes by regularised least squares."""
    if not len(freqs):
        return np.zeros(0, dtype=complex)
    m = np.asarray(m, dtype=float)
    end = len(m) if end is None else int(end)
    return _lsq(m, starts, freqs, taus, end, ridge, cross)[0]


def _energy_of(cv: np.ndarray, tau_l) -> np.ndarray:
    return 0.5 * np.abs(cv) ** 2 / (1 - np.exp(-2.0 / (np.asarray(tau_l, dtype=float) * SR)))


def _mode_wave(cnt: int, begins, freq_l, tau_l, cv, stop_i: int | None = None) -> np.ndarray:
    yv = np.zeros(cnt)
    if len(freq_l):
        _append_modes(yv, np.asarray(begins, dtype=int), freq_l, np.asarray(tau_l, dtype=float), cv,
                   cnt if stop_i is None else min(int(stop_i), cnt))
    return yv


def _append_modes(yv: np.ndarray, begins: np.ndarray, freq_l, tau_l: np.ndarray, cv, stop_i: int) -> None:
    zv = _pole_set(freq_l, tau_l)
    for sgl in np.unique(begins):
        sel_i = np.flatnonzero(begins == sgl)
        stop_v = min(stop_i, int(sgl + 16 * tau_l[sel_i].max() * SR) + 1)
        lzv = np.log(zv[sel_i])
        for cv0 in range(int(sgl), stop_v, 8192):
            cv1 = min(stop_v, cv0 + 8192)
            ix = np.arange(cv0 - sgl, cv1 - sgl)
            yv[cv0:cv1] += np.real(np.exp(np.multiply.outer(ix, lzv)) @ cv[sel_i])


# ================================================================ the passes

def _ends_of_strikes(ons_l: list[int], act_end: int) -> list[int]:
    return [ons_l[ii + 1] if ii + 1 < len(ons_l) else act_end for ii in range(len(ons_l))]


def _end_of_active(mv: np.ndarray, fit_n: int) -> int:
    nfrm = max(1, (len(mv) - 24) // 12 + 1)
    ev = np.array([np.sum(mv[ii * 12:ii * 12 + 24] ** 2) for ii in range(nfrm)])
    if ev.max() <= 0:
        return fit_n
    last_i = int(np.flatnonzero(ev >= ev.max() * 10 ** -7.5)[-1])
    return min(last_i * 12 + 24 + 12, fit_n)


def _fresh_modes(workv, stv, stop_i, cfg) -> list:
    start_i, skipv = stv["on"], cfg["skip"]
    cv0 = start_i + skipv + int(round(0.0005 * SR))
    chunk = workv[cv0:min(stop_i, cv0 + int(cfg["analysis_s"] * SR))]
    cand_l = _unique_modes(_cands_per_band(chunk, cfg["kmax"], cfg["nmax"]), stv["carry"] + stv["own"])
    roomv = cfg["max_modes"] - len(stv["own"])
    return _keep_fresh(workv, cand_l, start_i + skipv, stop_i, roomv, cfg) if cand_l and roomv > 0 else []


def _keep_fresh(workv, cand_l, begin, stop_i, roomv, cfg) -> list:
    """Keep the strongest new modes and subtract them from work."""
    fq, tv = zip(*cand_l)
    sgl = np.full(len(fq), begin)
    cv = ls_modes(workv[:stop_i], sgl, fq, tv, stop_i, cfg["ridge"], cfg["cross"])
    ev = _energy_of(cv, tv)
    keep_m = [ii for ii in np.argsort(-ev)[:roomv] if ev[ii] >= ev.sum() * 10 ** (cfg["prune_db"] / 10)]
    workv[:] -= _mode_wave(len(workv), sgl[keep_m], np.array(fq)[keep_m], np.array(tv)[keep_m], cv[keep_m], stop_i)
    return [cand_l[ii] for ii in keep_m]


def _carry_over(hits: list, ii: int) -> list:
    start_i = hits[ii]["on"]
    poolv = []
    for stv in hits[:ii]:
        for fq, tauv in stv["own"]:
            if np.exp(-(start_i - stv["on"]) / (tauv * SR)) > 0.01:
                poolv.append((fq, tauv))
    poolv.sort(key=lambda pv: -pv[1])
    return _unique_modes(poolv[:40], [])


def _flat_fit(hits: list, skipv: int):
    rws = [(ii, fq, tv) for ii, stv in enumerate(hits) for fq, tv in stv["carry"] + stv["own"]]
    if not rws:
        return np.zeros(0, int), np.zeros(0), np.zeros(0), np.zeros(0, int)
    sel_i, fq, tv = (np.array(vv) for vv in zip(*rws))
    begins = np.array([hits[ii]["on"] + skipv for ii in sel_i])
    return sel_i.astype(int), fq.astype(float), tv.astype(float), begins.astype(int)


def _mode_set(hits: list, sel_i, fq, tv, keep_m) -> None:
    for ii, stv in enumerate(hits):
        pick = [jx for jx in np.flatnonzero(sel_i == ii) if keep_m[jx]]
        carry_n = len(stv["carry"])
        rws = list(zip(fq[pick], tv[pick]))
        posv = [jx - np.flatnonzero(sel_i == ii)[0] for jx in pick]
        stv["carry"] = [rv for rv, pv in zip(rws, posv) if pv < carry_n]
        stv["own"] = [rv for rv, pv in zip(rws, posv) if pv >= carry_n]


def _joint_fit(yv, hits, cfg):
    """Joint least squares of every strike's modes, with prune and cluster rounds."""
    fit_l = _prune_loop(yv, hits, cfg)
    return None if fit_l is None else _cluster_loop(yv, hits, cfg, fit_l)


def _flat_model(yv, hits, cfg):
    """(idx, f, t, starts, c, G, b, x) of every strike's modes, or None when there are none."""
    sel_i, fq, tv, sgl = _flat_fit(hits, cfg["skip"])
    if not len(fq):
        return None
    return (sel_i, fq, tv, sgl) + tuple(_lsq(yv, sgl, fq, tv, cfg["n_fit"], cfg["ridge"], cfg["cross"]))


def _prune_loop(yv, hits, cfg):
    fit_l = None
    for _ in range(2):
        fit_l = _flat_model(yv, hits, cfg)
        if fit_l is None or _prune_once(hits, fit_l, cfg):
            break
    return fit_l


def _prune_once(hits, fit_l, cfg) -> bool:
    """Drop weak modes per strike. True when every mode stays."""
    sel_i, fq, tv, _, cv = fit_l[:5]
    ev = _energy_of(cv, tv)
    keep_m = np.ones(len(fq), bool)
    for ii in range(len(hits)):
        pick = sel_i == ii
        if pick.any():
            keep_m[pick] = ev[pick] >= ev[pick].sum() * 10 ** (cfg["prune_db"] / 10)
    finished = bool(keep_m.all())
    if not finished:
        _mode_set(hits, sel_i, fq, tv, keep_m)
    return finished


def _cluster_loop(yv, hits, cfg, fit_l):
    for _ in range(3):
        sel_i, fq, tv, _, _, gmat, bv, x = fit_l
        dropv = _drops_by_cluster(sel_i, fq, tv, gmat, bv, x)
        if not dropv.any():
            break
        _mode_set(hits, sel_i, fq, tv, ~dropv)
        fit_l = _flat_model(yv, hits, cfg)
        if fit_l is None:
            break
    return None if fit_l is None else fit_l[:5]


def _drops_by_cluster(sel_i, fq, tv, gmat, bv, x) -> np.ndarray:
    ix = len(fq)
    dropv = np.zeros(ix, bool)
    for ii in np.unique(sel_i):
        miv = np.flatnonzero(sel_i == ii)
        miv = miv[np.argsort(fq[miv], kind="stable")]
        for clv in _cluster_list(miv, fq, tv):
            sepv = sum(_quad_form(gmat, x, np.array([jx]), ix) for jx in clv)
            if sepv > 2 * max(_quad_form(gmat, x, np.array(clv), ix), 1e-300):
                winner = max(clv, key=lambda jx: _explained_share(gmat, bv, jx, ix))
                dropv[[jx for jx in clv if jx != winner]] = True
    return dropv


def _cluster_list(miv, fq, tv) -> list[list[int]]:
    acc, now_v = [], [miv[0]] if len(miv) else []
    for av, bv2 in zip(miv[:-1], miv[1:]):
        bwv = 0.5 * (1 / (np.pi * tv[av]) + 1 / (np.pi * tv[bv2]))
        if fq[bv2] - fq[av] < max(2.0, bwv):
            now_v.append(bv2)
        else:
            acc.append(now_v)
            now_v = [bv2]
    if now_v:
        acc.append(now_v)
    return [cv for cv in acc if len(cv) >= 2]


def _explained_share(gmat, bv, jx, ix) -> float:
    jj2 = [jx, jx + ix]
    gk_m = gmat[np.ix_(jj2, jj2)]
    try:
        return float(bv[jj2] @ np.linalg.solve(gk_m, bv[jj2]))
    except np.linalg.LinAlgError:
        return 0.0


def _pass_list(yv: np.ndarray, ons_l: list[int], cfg: dict):
    hits = [{"on": ov, "own": [], "carry": []} for ov in ons_l]
    end_l = _ends_of_strikes(ons_l, cfg["active_end"])
    workv = yv.copy()
    resv = None
    for _ in range(cfg["passes"]):
        _grow_modes(workv, hits, end_l, cfg)
        resv = _joint_fit(yv, hits, cfg)
        if resv is None:
            break
        sel_i, fq, tv, sgl, cv = resv
        workv = yv - _mode_wave(len(yv), sgl, fq, tv, cv, cfg["n_fit"])
    return hits, resv


def _grow_modes(workv, hits, end_l, cfg) -> None:
    """One pass of new modes for every strike."""
    for ii, stv in enumerate(hits):
        stv["carry"] = _carry_over(hits, ii)
        stv["own"] = _unique_modes(stv["own"], stv["carry"])
        stop_i = cfg["n_fit"] if ii == len(hits) - 1 else end_l[ii]
        if end_l[ii] - (stv["on"] + cfg["skip"]) >= 48:
            stv["own"] += _fresh_modes(workv, stv, stop_i, cfg)


# ================================================================ strike rows

def _row_set(sel_i, fq, tv, cv, ii, skipv) -> list:
    pick = np.flatnonzero(sel_i == ii)
    rws = []
    for jx in pick[np.argsort(fq[pick], kind="stable")]:
        av = abs(cv[jx]) * np.exp(skipv / (tv[jx] * SR))
        phz = np.angle(cv[jx]) - 2 * np.pi * fq[jx] * skipv / SR
        phz = (phz + np.pi) % (2 * np.pi) - np.pi
        rw1 = [round(float(fq[jx]), 2), sig(tv[jx]), sig(av), round(float(phz), 4)]
        if rw1[1] > 0 and rw1[2] > 0:
            rws.append(rw1)
    return rws


def _wave_of_rows(rws: list, cnt: int) -> np.ndarray:
    return K.mode_wave(np.asarray(rws, dtype=float).reshape(-1, 4), cnt)


def _pick_ramps(yv, hit_rows, ons_l, skipv):
    """Attack ramp per strike, and the mode signal with the chosen ramps."""
    cnt = len(yv)
    mode_sig = np.zeros(cnt)
    ramp_l = []
    for ii, (ov, rws) in enumerate(zip(ons_l, hit_rows)):
        wv = _wave_of_strike(rws, cnt - ov, ii > 0)
        stop_v = min(cnt - ov, skipv + int(round(0.0015 * SR)))
        winner = _ramp_pick(wv[:stop_v], (yv - mode_sig)[ov:ov + stop_v])
        ramp_l.append(winner if rws else 0.0)
        mode_sig[ov:] += _with_ramp(wv, winner)
    return ramp_l, mode_sig


def _wave_of_strike(rws: list, cnt: int, laterv: bool) -> np.ndarray:
    wv = _wave_of_rows(rws, cnt) if rws else np.zeros(cnt)
    if laterv:
        ix = min(K.LATER_RAMP, len(wv))
        wv[:ix] *= K.raised_cos(K.LATER_RAMP)[:ix]
    return wv


def _ramp_pick(headv: np.ndarray, tgt: np.ndarray) -> float:
    winner, err_best = 0.0, None
    for rms_v in RAMPS_MS:
        errv = float(np.sum((tgt - _with_ramp(headv, rms_v)) ** 2))
        if err_best is None or errv < err_best * (1 - 1e-9):
            winner, err_best = rms_v, errv
    return winner


def _with_ramp(wv: np.ndarray, rms_v: float) -> np.ndarray:
    rv = int(round(rms_v * SR / 1000.0))
    if rv <= 0:
        return wv
    acc = wv.copy()
    ix = min(rv, len(acc))
    acc[:ix] *= K.raised_cos(rv)[:ix]
    return acc


def _subpulse(residv: np.ndarray, yv: np.ndarray):
    cnt = len(yv)
    if cnt <= 4 * K.SUB_DEC:
        return None, np.zeros(cnt)
    lpv = sosfiltfilt(butter(4, 20, fs=SR, output="sos"), residv)
    if np.sum(lpv ** 2) <= 0.001 * np.sum(yv ** 2):
        return None, np.zeros(cnt)
    xsv = [sig(vv, 4) for vv in resample_poly(lpv, 1, K.SUB_DEC)]
    upv = resample_poly(np.asarray(xsv), K.SUB_DEC, 1)
    wv = np.zeros(cnt)
    wv[:min(cnt, len(upv))] = upv[:cnt]
    return {"rate": SR // K.SUB_DEC, "x": xsv}, wv


def _contact_fit(residv: np.ndarray, ons_l: list[int], attack_ms: float):
    cnt = len(residv)
    rv = residv.copy()
    acc, tot = [], np.zeros(cnt)
    for ii, ov in enumerate(ons_l):
        stop_v = min(ov + int(round(attack_ms * SR / 1000)), ons_l[ii + 1] if ii + 1 < len(ons_l) else cnt, cnt)
        rws = _rows_of_contact(rv, ov, stop_v) if stop_v - ov >= 24 else []
        acc.append(rws)
        if rws:
            wv = _wave_of_rows(rws, cnt - ov)
            rv[ov:] -= wv
            tot[ov:] += wv
    return acc, tot


def _rows_of_contact(rv: np.ndarray, ov: int, stop_v: int) -> list:
    fq, tauv = _poles_of_contact(rv[ov:stop_v])
    rws = []
    if len(fq):
        cv = ls_modes(rv, np.full(len(fq), ov), fq, tauv, min(len(rv), ov + int(0.006 * SR)))
        rws = [[round(float(fiv), 2), sig(tiv), sig(abs(civ)), round(float(np.angle(civ)), 4)]
                for fiv, tiv, civ in zip(fq, tauv, cv)]
    return sorted([rwv for rwv in rws if rwv[1] > 0 and rwv[2] > 0], key=lambda rwv: rwv[0])


def _poles_of_contact(chunk: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Frequencies and decays of the short contact modes in seg."""
    zv = esprit(chunk, 24, 50)[0]
    radv = np.abs(zv)
    zv = zv[(radv > 1e-4) & (radv < 0.9999) & (np.angle(zv) >= 0)]
    fq = np.angle(zv) * SR / (2 * np.pi)
    tauv = -1.0 / (np.log(np.abs(zv)) * SR)
    good = (tauv >= 1e-5) & (tauv <= 0.002) & (fq < 0.47 * SR)
    return fq[good], tauv[good]


# ================================================================ noise envelopes

def _power_per_band(x: np.ndarray, mask_l: np.ndarray, fft_n: int) -> np.ndarray:
    cnt = len(x)
    xmat = rfft(x, fft_n)
    acc = np.zeros((len(mask_l), cnt))
    for bv, wv in enumerate(mask_l):
        full = np.zeros(fft_n, dtype=complex)
        halfv = xmat * wv
        full[:len(halfv)] = halfv
        full[1:(fft_n + 1) // 2] *= 2
        acc[bv] = 0.5 * np.abs(ifft(full)[:cnt]) ** 2
    return acc


def _smooth_curve(pmat: np.ndarray, msv: float, gridv: np.ndarray) -> np.ndarray:
    from hark.fit.analysis._common import hann_norm
    wv = hann_norm(max(1, int(round(msv * SR / 1000))))
    sgl = fftconvolve(pmat, wv[None, :], mode="same", axes=1)
    return np.maximum(sgl[:, gridv], 0.0)


def _weight_vec(tgv: np.ndarray, ons_sec: list[float]) -> tuple[np.ndarray, np.ndarray]:
    nearv = np.zeros(len(tgv))
    for ov in ons_sec:
        dv = tgv - ov
        wv = np.where(dv < -0.008, 0.0, np.where(dv <= 0.015, 1.0, np.clip((0.03 - dv) / 0.015, 0, 1)))
        nearv = np.maximum(nearv, wv)
    farv = np.clip((tgv - max(ons_sec) - 0.15) / 0.15, 0.0, 1.0)
    return nearv, farv


def _power_of_noise(x: np.ndarray, mask_l, fft_n, gridv, nearv, farv) -> np.ndarray:
    pmat = _power_per_band(x, mask_l, fft_n)
    sgl, mv, lv = (_smooth_curve(pmat, msv, gridv) for msv in (0.5, 6.0, 30.0))
    return nearv * sgl + (1 - nearv) * ((1 - farv) * mv + farv * lv)


def _env_of_noise(mv: np.ndarray, tonal: np.ndarray, ons_l: list[int], tol_db: float, pre_cap_s: float) -> dict:
    tgv, farv, emat = _noise_level_db(mv, tonal, ons_l, pre_cap_s)
    envl = []
    for bv in range(len(emat)):
        keep_m = rdp(tgv, emat[bv], tol_db * (1 + farv))
        envl.append([[round(float(tgv[ii]), 5), round(float(emat[bv, ii]), 2)] for ii in keep_m])
    return {"edges": [float(ev) for ev in NOISE_EDGES], "h_erb": 0.4, "env": envl}


def _noise_level_db(mv: np.ndarray, tonal: np.ndarray, ons_l: list[int], pre_cap_s: float):
    """Grid times, far weight and residual noise power (dB) per ERB band."""
    cnt = len(mv)
    fft_n = next_fast_len(cnt + 16384)
    mask_l = K.erb_masks(fft_n)
    gridv = np.arange(0, cnt, GRID)
    tgv = gridv / SR
    nearv, farv = _weight_vec(tgv, [ov / SR for ov in ons_l])
    pov = _power_of_noise(mv, mask_l, fft_n, gridv, nearv, farv)
    pdm = _power_of_noise(tonal, mask_l, fft_n, gridv, nearv, farv)
    pmat = np.maximum(pov - pdm, 1e-6 * pov)
    _cap_before(pmat, ons_l, pre_cap_s)
    emat = db(pmat)
    return tgv, farv, np.maximum(emat, emat.max() - 100.0)


def _cap_before(pmat: np.ndarray, ons_l: list[int], pre_cap_s: float) -> None:
    for ov in ons_l:
        ix = ov // GRID
        av = ix - int(round(pre_cap_s / K.GRID_S))
        if av < 1:
            continue
        capv = np.min(pmat[:, max(0, av - 2):av + 1], axis=1, keepdims=True)
        pmat[:, av:ix + 1] = np.minimum(pmat[:, av:ix + 1], capv)


def _ratio_side(av: np.ndarray) -> list[float]:
    cnt = len(av)
    fft_n = next_fast_len(cnt)
    mask_l = K.erb_masks(fft_n)
    smat = np.abs(rfft((av[:, 0] - av[:, 1]) / 2, fft_n)) ** 2
    mcol = np.abs(rfft((av[:, 0] + av[:, 1]) / 2, fft_n)) ** 2
    wmat2 = mask_l ** 2
    rhov = np.sqrt((wmat2 @ smat) / np.maximum(wmat2 @ mcol, 1e-300))
    return [round(float(vv), 4) for vv in np.minimum(rhov, 1.5)]


# ================================================================ fit

FIT_KNOBS = {"max_strikes": 10, "kmax": 16, "nmax": 384, "passes": 2, "skip_ms": 0.5, "prune_db": -50,
             "max_modes": 80, "onset_rel": 0.2, "noise_tol_db": 1, "analysis_s": 3, "attack_ms": 2.5,
             "verbose": False, "ramps": RAMPS_MS, "pre_cap_s": 0.002, "side_t": True, "sr": SR}


def fit(x: Any, **kw) -> dict:
    """A modal model of a path or a 48 kHz array, (n,) or (n, 2). Knobs and defaults: FIT_KNOBS."""
    ix = merge_knobs(FIT_KNOBS, kw, "modal fit")
    av = as_stereo(x) if isinstance(x, str) else as_stereo(resample(np.asarray(x, float), ix["sr"]))
    model = _fit_core(av, ix)
    model["source"] = source_name(x)
    if ix["verbose"]:
        print(summary(model))
    return model


def _fit_core(av: np.ndarray, ix: dict) -> dict:
    stereo = not is_mono_pair(av)
    mv = av.mean(axis=1)
    ons_l, fov, yv, cfg = _setup_fit(mv, ix)
    tonal, hits, subv, mode_n, mode_sig = _tonal_part(yv, ons_l, cfg, ix["attack_ms"])
    _tail_fade(tonal, fov)
    model = {"kind": KIND, "version": VERSION, "sr": SR, "length_s": round(len(mv) / SR, 6),
             "channels": 2 if stereo else 1, "fade_out_s": float(fov), "strikes": hits, "sub": subv,
             "noise": _env_of_noise(mv, tonal, ons_l, ix["noise_tol_db"], ix["pre_cap_s"]), "side": None,
             "coherent": True, "fit": _diagnostics(mv, mode_sig, tonal, ix["skip_ms"], mode_n)}
    if stereo:
        _attach_side(model, av, ix["side_t"])
    return model


def _setup_fit(mv: np.ndarray, ix: dict):
    """Onsets, fade-out length, the unfaded signal and the pass options."""
    ons_l = onsets(mv, rel=ix["onset_rel"], max_n=ix["max_strikes"])
    fov = fade_out(mv)
    yv, fit_n = _undo_fade(mv, fov)
    return ons_l, fov, yv, _opts_of_fit(ix, fit_n, _end_of_active(mv, fit_n))


def _opts_of_fit(ix: dict, fit_n: int, act_end: int) -> dict:
    return {"skip": int(round(ix["skip_ms"] * SR / 1000)), "kmax": ix["kmax"], "nmax": ix["nmax"],
            "passes": ix["passes"], "prune_db": ix["prune_db"], "max_modes": ix["max_modes"],
            "analysis_s": ix["analysis_s"], "ridge": 1e-5, "cross": 10, "n_fit": fit_n,
            "active_end": act_end}


def _tail_fade(tonal: np.ndarray, fov: float) -> None:
    if fov > 0:
        cnt = len(tonal)
        n_frq = min(int(round(fov * SR)), cnt)
        tonal[cnt - n_frq:] *= np.linspace(1.0, 0.0, n_frq)


def _attach_side(model: dict, av: np.ndarray, side_t: bool) -> None:
    model["side"] = _ratio_side(av)
    if side_t:
        from hark.fit.analysis import sinusoidal
        model["side_t"] = sinusoidal.side_model(av, len(av) / SR)


def _tonal_part(yv: np.ndarray, ons_l: list[int], cfg: dict, attack_ms: float):
    cnt = len(yv)
    _, rsv = _pass_list(yv, ons_l, cfg)
    rws = [[] for _ in ons_l]
    if rsv is not None:
        sel_i, fq, tv, sgl, cv = rsv
        rws = [_row_set(sel_i, fq, tv, cv, ii, cfg["skip"]) for ii in range(len(ons_l))]
    ramp_l, mode_sig = _pick_ramps(yv, rws, ons_l, cfg["skip"])
    subv, sub_w = _subpulse(yv - mode_sig, yv)
    attack, contact = _contact_fit(yv - mode_sig - sub_w, ons_l, attack_ms)
    hits = [{"t": round(ov / SR, 6), "ramp_ms": float(rpv), "modes": rwv, "attack": atv}
               for ov, rpv, rwv, atv in zip(ons_l, ramp_l, rws, attack)]
    return mode_sig + sub_w + contact, hits, subv, sum(len(rv) for rv in rws), mode_sig


def _diagnostics(mv, mode_sig, tonal, skip_ms, mode_n) -> dict:
    ev = float(np.sum(mv ** 2))
    frac = float(db(np.sum(mode_sig ** 2) / ev)) if ev > 0 else -300.0
    residv = float(db(np.sum((mv - tonal) ** 2) / ev)) if ev > 0 else -300.0
    return {"skip_ms": skip_ms, "modes": int(mode_n), "mode_share_db": round(frac, 2),
            "resid_db": round(residv, 2)}


# ================================================================ face

def check(model: Any) -> None:
    """ValueError when the model is not a modal model of a supported version."""
    check_kind(model, KIND, VERSION)


def render(model: dict, seed: int = 0, **knobs) -> np.ndarray:
    """Validate, then render: (n, 2) for a stereo model or stereo=True, else (n,)."""
    check(model)
    if "gain_db" in knobs:
        knobs["gain"] = knobs.pop("gain_db")
    badv = sorted(set(knobs) - set(RENDER_KNOBS))
    if badv:
        raise ValueError(f"unknown modal knobs {badv}; known: {sorted(RENDER_KNOBS)}")
    return K.render_model(model, seed, **knobs)


def save(model: dict, path: str) -> None:
    """Write compact JSON."""
    save_json(model, path)


def load(path: str) -> dict:
    """Read a model and check its kind."""
    mv = load_json(path)
    check(mv)
    return mv


def summary(model: dict) -> str:
    """A few lines of text."""
    ftv = model.get("fit", {})
    stv = model.get("strikes", [])
    line_l = [f"modal v{model.get('version')}: {model['length_s'] * 1000:.0f} ms, {len(stv)} strikes, "
             f"{ftv.get('modes', 0)} modes, mode share {ftv.get('mode_share_db')} dB, "
             f"residual {ftv.get('resid_db')} dB"]
    for sgl in stv:
        topv = sorted(sgl["modes"], key=lambda rv: -(rv[2] ** 2 * rv[1]))[:5]
        modes = ", ".join(f"{rv[0]:.0f} Hz/{rv[1] * 1000:.0f} ms" for rv in topv) or "none"
        line_l.append(f"  strike {sgl['t'] * 1000:.1f} ms, ramp {sgl['ramp_ms']} ms, "
                     f"{len(sgl['modes'])} modes: {modes}")
    return "\n".join(line_l)


# ================================================================ quick checks

def mel_distance(x: np.ndarray, y: np.ndarray) -> tuple[float, list[float]]:
    """Mean absolute mel dB at 3 FFT sizes, and the value per size."""
    return mel_db_distance(_to_mono(x), _to_mono(y))


def _to_mono(x: np.ndarray) -> np.ndarray:
    x = np.asarray(x, dtype=float)
    return x.mean(axis=1) if x.ndim == 2 else x


def _env_rms_db(x: np.ndarray, winv: int, step_n: int) -> np.ndarray:
    pv = np.convolve(x ** 2, np.ones(winv) / winv, mode="same")[::step_n]
    return db(pv)


def env_corr(x: np.ndarray, y: np.ndarray) -> float:
    """Pearson correlation of 2 ms RMS dB envelopes, floored 60 dB under x's peak."""
    x, y = _to_mono(x), _to_mono(y)
    cnt = max(len(x), len(y))
    x, y = np.pad(x, (0, cnt - len(x))), np.pad(y, (0, cnt - len(y)))
    exv, eyv = _env_rms_db(x, 96, 96), _env_rms_db(y, 96, 96)
    flv = exv.max() - 60
    exv, eyv = np.maximum(exv, flv), np.maximum(eyv, flv)
    if np.std(exv) == 0 or np.std(eyv) == 0:
        return 0.0
    return float(np.corrcoef(exv, eyv)[0, 1])


def rise_ms(x: np.ndarray) -> float:
    """10 % to 90 % rise time of a 1 ms RMS envelope, ms."""
    ev = np.sqrt(np.convolve(_to_mono(x) ** 2, np.ones(48) / 48, mode="same")[::6])
    if not len(ev) or ev.max() <= 0:
        return 0.0
    ix = int(np.argmax(ev))
    av = max(0, ix - int(0.03 * SR / 6))
    chunk = ev[av:ix + 1]
    ix10 = int(np.argmax(chunk >= 0.1 * ev[ix]))
    ix90 = int(np.argmax(chunk >= 0.9 * ev[ix]))
    return float((ix90 - ix10) * 6 / SR * 1000)


# ================================================================ standalone code

def to_code(model: dict, name: str = "sound") -> str:
    """A standalone numpy + scipy script that renders the same samples as ``render``."""
    import inspect
    slim = copy.deepcopy({ix: vv for ix, vv in model.items() if ix != "fit_engine"})
    return (f'"""Modal model {name!r}: standalone render (numpy and scipy only)."""\n\n'
            + inspect.getsource(K) + "\n\nMODEL = " + _as_literal(slim) + "\n\n\n"
            + "def render(seed=0, **knobs):\n    return render_model(MODEL, seed, **knobs)\n\n\n"
            + _MAIN_F32)


def _as_literal(model: dict) -> str:
    """The model as a Python literal, one mode row per line."""
    line_l = ["{"]
    for ix, vv in model.items():
        if ix == "strikes":
            line_l.append("    'strikes': [")
            for sgl in vv:
                line_l.append("        {" + f"'t': {sgl['t']!r}, 'ramp_ms': {sgl['ramp_ms']!r},")
                line_l.append("         'modes': [")
                line_l += [f"             {rv!r}," for rv in sgl["modes"]]
                line_l.append(f"         ], 'attack': {sgl['attack']!r}" + "},")
            line_l.append("    ],")
        else:
            line_l.append(f"    {ix!r}: {vv!r},")
    line_l.append("}")
    return "\n".join(line_l)


_MAIN_F32 = '''if __name__ == "__main__":
    import sys
    from scipy.io import wavfile
    seed = int(sys.argv[1]) if len(sys.argv) > 1 else 0
    out = sys.argv[2] if len(sys.argv) > 2 else "sound.wav"
    wavfile.write(out, SR, render(seed).astype(np.float32))
'''
