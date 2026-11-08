"""Sines fitter: tracked partials, strike atoms and third-octave residual noise.

Analysis by synthesis in the sinusoidal-modelling tradition: per-frame peak
picking, frame-to-frame partial tracking, phase-exact resynthesis for the
residual, damped-sinusoid atoms at strikes, and a band-noise residual model.
"""

from __future__ import annotations

import copy
import math
from typing import Any, Callable

import numpy as np
from scipy.ndimage import maximum_filter1d, median_filter, uniform_filter1d
from scipy.signal import butter, find_peaks, get_window, sosfiltfilt, stft, welch

from hark.fit.analysis import _kernels as K
from hark.fit.analysis import space
from hark.fit.analysis._common import (SR, as_stereo, check_kind, merge_knobs, db, is_mono_pair, load_json,
                                       rdp, rdp_rows, resample, save_json, source_name)

KIND = "sines"
NAME = "sines"
VERSION = 1

KNOBS: dict[str, tuple] = {
    "transpose": ((-3.0, 3.0), 0.0, "Semitones, partials and residual together."),
    "stretch": ((0.85, 1.18, "log"), 1.0, "Time factor about the first strike."),
    "decay": ((0.7, 1.43, "log"), 1.0, "Factor on every decay after its peak."),
    "brightness": ((-3.0, 3.0), 0.0, "dB per octave about the partial centroid."),
    "vibrato": ((0.0, 20.0), 0.0, "Cents."),
    "drift": ((0.0, 10.0), 0.0, "Cents of slow wander."),
    "residual": ((0.5, 2.0, "log"), 1.0, "Gain on the noise."),
    "transient": ((0.5, 2.0, "log"), 1.0, "Gain on the strike noise (first 15 ms)."),
    "vary": ((0.0, 0.6), 0.0, "Seeded jitter."),
    "inharm": ((-0.03, 0.03), 0.0, "Partial ratio stretch."),
    "width": ((0.5, 1.5), 1.0, "Factor on the fitted side."),
}

RENDER_KNOBS = ("transpose", "stretch", "decay", "brightness", "vibrato", "vibrato_rate", "drift",
                "residual", "transient", "vary", "partials", "inharm", "phases", "gain_db",
                "stereo", "width", "ring")

WINDOWS = (512, 1024, 2048, 4096)
RES_BANDS = tuple(50.0 * 2 ** (k / 3) for k in range(26))
SIDE_BANDS = (125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0)
STRIKE_BANDS = tuple(150.0 * 2 ** k for k in range(7))
HF_POINTS = tuple(float(16000 + 1000 * k) for k in range(9))

band_masks = K.tri_masks
synth_partials = K.synth_partials
synth_atoms = K.synth_atoms
synth_noise = K.synth_noise
finish = K.finish

__all__ = ["KIND", "NAME", "VERSION", "KNOBS", "fit", "render", "check", "summary", "save", "load",
           "analyze", "onsets", "side_model", "morph_side", "vary_field", "finish", "hf_rolloff",
           "synth_partials", "synth_atoms", "synth_noise", "partial_freq", "fade_in_s",
           "band_masks", "describe", "to_code"]


# ================================================================ band envelopes

def band_env(x: np.ndarray, centres, sr: int = SR, step_s: float = 0.001,
             max_smooth: float = 0.03) -> tuple[np.ndarray, np.ndarray]:
    """Band power in dB at ``step_s`` steps: (times, (B, T))."""
    x = np.asarray(x, dtype=float)
    count = len(x)
    pad = 4096
    n_fft = count + 2 * pad
    spec = np.fft.rfft(np.pad(x, pad), n_fft)
    masks = band_masks(centres, n_fft, sr)
    stride = int(round(step_s * sr))
    sel_i = np.arange(0, count, stride)
    acc = []
    for bb, cc in enumerate(centres):
        out_y = np.fft.irfft(spec * masks[bb], n_fft)[pad:pad + count]
        bw = cc * (2 ** (1 / 3) - 2 ** (-1 / 3))
        lwin = max(2, int(np.clip(4.0 / bw, 0.0005, max_smooth) * sr))
        acc.append(10 * np.log10(np.maximum(_lesser_mean(out_y * out_y, lwin)[sel_i], 1e-24)))
    return sel_i / sr, np.array(acc)


def _lesser_mean(pwr: np.ndarray, lwin: int) -> np.ndarray:
    cc = np.concatenate([[0.0], np.cumsum(pwr)])
    count = len(pwr)
    ii = np.arange(count)
    low_i = np.maximum(ii - lwin + 1, 0)
    trail = (cc[ii + 1] - cc[low_i]) / (ii + 1 - low_i)
    high_i = np.minimum(ii + lwin, count)
    lead = (cc[high_i] - cc[ii]) / (high_i - ii)
    return np.minimum(trail, lead)


# ================================================================ strikes

def onsets(x: np.ndarray, sr: int = SR) -> list[dict]:
    """Strikes ``[{'t', 'rise', 'strength'}]`` of a mono signal."""
    x = np.asarray(x, dtype=float)
    if x.ndim == 2:
        x = x.mean(axis=1)
    if len(x) < 64 or not np.any(x):
        return []
    _, envl = band_env(x, STRIKE_BANDS, sr, 0.001, 0.008)
    novel, tot_db = _novelty_curve(envl)
    cand_l = [(int(ix), float(novel[ix])) for ix in find_peaks(novel, height=6.0, distance=25)[0]]
    first = int(np.argmax(tot_db > tot_db.max() - 25))
    cand_l.append((first, None))
    penv = _env_of_peaks(x, sr)
    got = [sv for sv in (_locate_strike(penv, ix, val, sr) for ix, val in sorted(cand_l)) if sv]
    return _join_strikes(got)


def _novelty_curve(envl: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    top_v = envl.max()
    envl = np.maximum(envl, top_v - 70)
    tot_db = 10 * np.log10(np.sum(10 ** (envl / 10), axis=0))
    padded = np.concatenate([np.repeat(envl[:, :1], 4, axis=1), envl], axis=1)
    mins = np.min(np.stack([padded[:, ii:ii + envl.shape[1]] for ii in range(5)]), axis=0)
    rise_v = np.maximum(envl - mins, 0.0)
    loud_v = envl > top_v - 60
    cntv = loud_v.sum(axis=0)
    novel = np.where(cntv > 0, (rise_v * loud_v).sum(axis=0) / np.maximum(cntv, 1), 0.0)
    novel[tot_db < tot_db.max() - 45] = 0.0
    novel[(rise_v > 6).sum(axis=0) < 3] = 0.0
    return novel, tot_db


def _env_of_peaks(sig: np.ndarray, rate: int) -> np.ndarray:
    size = int(round(0.0025 * rate))
    mm = maximum_filter1d(np.abs(sig), size, origin=size // 2 - 1, mode="constant")
    return 20 * np.log10(np.maximum(mm, 1e-12))


def _locate_strike(penv: np.ndarray, ix: int, valv: float | None, rate: int) -> dict | None:
    cc = int(ix * rate / 1000)
    aa = max(0, cc - int(0.025 * rate))
    bb = min(len(penv), cc + int(0.030 * rate))
    pp0 = max(aa, cc - int(0.008 * rate))
    if bb <= pp0 + 1:
        return None
    ipk = pp0 + int(np.argmax(penv[pp0:bb]))
    pk_v = penv[ipk]
    i_min = aa + int(np.argmin(penv[aa:ipk + 1]))
    basev = max(float(penv[i_min]), pk_v - 70)
    if valv is not None and pk_v - basev < 3:
        return None
    acc = _rise_of_strike(penv, i_min, ipk, basev, rate)
    acc["strength"] = float(valv if valv is not None else pk_v - basev)
    return acc


def _rise_of_strike(penv: np.ndarray, i_min: int, ipk: int, basev: float, rate: int) -> dict:
    """Start time and rise time of the strike that peaks at ip."""
    pk_v = penv[ipk]
    level = max(basev + 0.25 * (pk_v - basev), pk_v - 30)
    begin = i_min + int(np.argmax(penv[i_min:ipk + 1] >= level))
    rend = begin + int(np.argmax(penv[begin:ipk + 1] >= pk_v - 1))
    return {"t": begin / rate, "rise": max(0.0003, (rend - begin) / rate)}


def _join_strikes(got: list[dict]) -> list[dict]:
    acc: list[dict] = []
    for sv in sorted(got, key=lambda sv: sv["t"]):
        if acc and sv["t"] - acc[-1]["t"] < 0.025:
            if sv["strength"] > acc[-1]["strength"]:
                acc[-1] = sv
            continue
        acc.append(sv)
    return acc


# ================================================================ framing and peaks

def _frames_stft(sig: np.ndarray, win_n: int) -> dict:
    step_n = max(32, win_n // 8)
    nfr = len(sig) // step_n + 2
    buf = np.concatenate([np.zeros(win_n // 2), sig, np.zeros(win_n // 2 + 2 * step_n)])
    winv = get_window("blackman", win_n)
    sel_i = np.arange(nfr)[:, None] * step_n + np.arange(win_n)[None, :]
    frame = buf[sel_i] * winv
    cplx = np.zeros((nfr, 2 * win_n))
    cplx[:, :win_n // 2] = frame[:, win_n // 2:]
    cplx[:, -(win_n // 2):] = frame[:, :win_n // 2]
    X = np.fft.rfft(cplx, axis=1)
    return {"X": X, "hop": step_n, "N": 2 * win_n, "wsum": float(np.sum(winv)), "W": win_n}


def _peaks_in_frame(magdb: np.ndarray, phs0: np.ndarray, g_ref: float, frame: dict, cfg: dict,
                 rate: int) -> np.ndarray:
    """Peaks of one frame: rows (freq, amp, phase, dB, contrast)."""
    bb = _bins_of_peaks(magdb, g_ref, frame["N"], cfg, rate)
    if len(bb) == 0:
        return np.zeros((0, 5))
    return _refine_peaks(bb, magdb, phs0, _floor_per_block(magdb), frame, rate)


def _bins_of_peaks(dlt: np.ndarray, g_ref: float, N: int, cfg: dict, rate: int) -> np.ndarray:
    """Bins of the strongest prominent local maxima."""
    powv = 10 ** (dlt / 10)
    local = 10 * np.log10(np.maximum(uniform_filter1d(powv, 33, mode="nearest"), 1e-30))
    pk = np.zeros(len(dlt), bool)
    pk[1:-1] = (dlt[1:-1] > dlt[:-2]) & (dlt[1:-1] >= dlt[2:])
    pk &= (dlt > g_ref - cfg["dyn_db"]) & (dlt > dlt.max() - cfg["dyn_db"]) & (dlt > local + cfg["prom_db"])
    pk[:int(np.ceil(30 * N / rate))] = False
    pk[-2:] = False
    bb = np.nonzero(pk)[0]
    if len(bb) > cfg["max_peaks"]:
        bb = np.sort(bb[np.argsort(dlt[bb])[-cfg["max_peaks"]:]])
    return bb


def _floor_per_block(dlt: np.ndarray) -> np.ndarray:
    nbin = max(1, len(dlt) // 32)
    meds = np.array([np.median(dlt[ii * 32:(ii + 1) * 32]) for ii in range(nbin)])
    centres = np.arange(nbin) * 32 + 15.5
    return np.interp(np.arange(len(dlt)), centres, meds)


def _refine_peaks(bb, dlt, phs0, floor, frame, rate):
    aa, cc, err = dlt[bb - 1], dlt[bb], dlt[bb + 1]
    denom = aa - 2 * cc + err
    pwr = np.where(np.abs(denom) < 1e-12, 0.0, 0.5 * (aa - err) / np.where(denom == 0, 1, denom))
    pwr = np.clip(pwr, -0.5, 0.5)
    freq = (bb + pwr) * rate / frame["N"]
    ampl = 10 ** ((cc - 0.25 * (aa - err) * pwr) / 20) * 2 / frame["wsum"]
    nbin = np.where(pwr >= 0, bb + 1, bb - 1)
    stride = np.angle(np.exp(1j * (phs0[nbin] - phs0[bb])))
    phz = phs0[bb] + np.abs(pwr) * stride
    return np.column_stack([freq, ampl, phz, cc, cc - floor[bb]])


# ================================================================ tracking

def _track_one(pk_list: list[np.ndarray], win_n: int, rate: int) -> list[dict]:
    tol_min = 1.5 * rate / win_n
    is_on: list[dict] = []
    finished: list[dict] = []
    for ix, pmat in enumerate(pk_list):
        used = _link_peaks(is_on, pmat, ix, tol_min)
        remaining = []
        for trk in is_on:
            (remaining if ix - trk["k"][-1] <= 2 else finished).append(trk)
        is_on = remaining
        for jj in range(len(pmat)):
            if jj not in used:
                is_on.append({"k": [ix], "rows": [pmat[jj]], "slope": 0.0})
    finished.extend(is_on)
    return [_gap_fill(trk) for trk in finished]


def _link_peaks(is_on: list[dict], pmat: np.ndarray, ix: int, tol_min: float) -> set:
    if not is_on or not len(pmat):
        return set()
    return _match_greedy(is_on, pmat, ix, _link_cost(is_on, pmat, ix, tol_min))


def _link_cost(is_on: list[dict], pmat: np.ndarray, ix: int, tol_min: float) -> np.ndarray:
    """Track-to-peak cost: frequency error over tolerance plus a small level term. inf past tolerance."""
    guess = np.array([trk["rows"][-1][0] + trk["slope"] * (ix - trk["k"][-1]) for trk in is_on])
    last_db = np.array([trk["rows"][-1][3] for trk in is_on])
    tolv = np.maximum(tol_min, 0.025 * guess)
    dfv = np.abs(pmat[None, :, 0] - guess[:, None])
    price = dfv / tolv[:, None] + 0.01 * np.abs(pmat[None, :, 3] - last_db[:, None])
    price[dfv > tolv[:, None]] = np.inf
    return price


def _match_greedy(is_on: list[dict], pmat: np.ndarray, ix: int, price: np.ndarray) -> set:
    usedt, usedp = set(), set()
    for flat in np.argsort(price, axis=None):
        ii, jj = divmod(int(flat), len(pmat))
        if not np.isfinite(price[ii, jj]):
            break
        if ii not in usedt and jj not in usedp:
            usedt.add(ii)
            usedp.add(jj)
            _grow_track(is_on[ii], pmat[jj], ix)
    return usedp


def _grow_track(trk: dict, rw: np.ndarray, ix: int) -> None:
    gapv = ix - trk["k"][-1]
    fresh = (rw[0] - trk["rows"][-1][0]) / gapv
    if len(trk["k"]) >= 2:
        trk["slope"] = 0.5 * (trk["slope"] + fresh)
    elif gapv > 1:
        trk["slope"] = fresh
    else:
        trk["slope"] = fresh
    trk["k"].append(ix)
    trk["rows"].append(rw)


def _gap_fill(trk: dict) -> dict:
    kset = np.asarray(trk["k"])
    rws = np.asarray(trk["rows"])
    whole = np.arange(kset[0], kset[-1] + 1)
    acc = {"k": whole, "t0": None}
    for col, name in ((0, "f"), (1, "a"), (4, "c")):
        acc[name] = np.interp(whole, kset, rws[:, col])
    phz = np.interp(whole, kset, rws[:, 2])
    have = np.isin(whole, kset)
    acc["ph"] = phz
    acc["_have"] = have
    acc["ph_raw"] = dict(zip(kset.tolist(), rws[:, 2].tolist()))
    return acc


def _phase_fill(trk: dict, step_n: int, rate: int) -> None:
    phz = trk["ph"].copy()
    for ii in range(1, len(phz)):
        ix = int(trk["k"][ii])
        if ix in trk["ph_raw"]:
            phz[ii] = trk["ph_raw"][ix]
        else:
            phz[ii] = phz[ii - 1] + 2 * np.pi * 0.5 * (trk["f"][ii - 1] + trk["f"][ii]) * step_n / rate
    trk["ph"] = phz


def _prune_tracks(lines_: list[dict], frame: dict, cfg: dict, rate: int, space_left: int,
                   refpk: float | None = None) -> list[dict]:
    step_n, win_n = frame["hop"], frame["W"]
    min_len = max(3, math.ceil(max(cfg["min_ms"] / 1000, 1.25 * win_n / rate) * rate / step_n))
    lines_ = [tm for tm in lines_ if len(tm["k"]) >= min_len]
    kept = _clear_loud(lines_, cfg, refpk) if lines_ else []
    kept.sort(key=lambda tm: -float(np.sum(tm["a"] ** 2)))
    return kept[:space_left]


def _clear_loud(lines_: list[dict], cfg: dict, refpk: float | None) -> list[dict]:
    """Tracks within keep_db of the top peak and with enough contrast over the floor."""
    pk_list = np.array([20 * np.log10(max(tm["a"].max(), 1e-30)) for tm in lines_])
    top_v = pk_list.max() if refpk is None else refpk
    return [tm for tm, pwr in zip(lines_, pk_list) if pwr > top_v - cfg["keep_db"] and _peak_contrast(tm) >= cfg["snr_db"]]


def _peak_contrast(tm: dict) -> float:
    err = tm["a"] ** 2
    return float(np.sum(err * tm["c"]) / max(np.sum(err), 1e-30))


# ================================================================ onset fixes

def _repair_onsets(lines_: list[dict], hits: list[dict], frame: dict, rate: int) -> list[dict]:
    step_n = frame["hop"]
    halfw = frame["W"] / (2 * rate)
    lines_ = _divide_tracks(lines_, hits, step_n, halfw, rate)
    lines_ = [tm for tm in lines_ if not _echo_before(tm, hits, step_n, halfw, rate)]
    for tm in lines_:
        _repair_head(tm, hits, step_n, halfw, rate)
    return [tm for tm in lines_ if len(tm["k"])]


def _time_list(tm: dict, step_n: int, rate: int) -> np.ndarray:
    return tm["k"] * step_n / rate


def _cut(tm: dict, pick: np.ndarray) -> dict:
    acc = dict(tm)
    for kname in ("k", "f", "a", "c", "ph"):
        acc[kname] = tm[kname][pick]
    return acc


def _divide_tracks(lines_, hits, step_n, halfw, rate):
    acc = []
    for tm in lines_:
        pieces = [tm]
        for sv in hits:
            cut_v = _divide_at(pieces[-1], sv["t"], step_n, halfw, rate)
            if cut_v is not None:
                pieces = pieces[:-1] + cut_v
        acc.extend(pieces)
    return acc


def _divide_at(now_: dict, tstamp: float, step_n: int, halfw: float, rate: int) -> list | None:
    """[head, tail] (head only when 3+ frames) when cur jumps over 10 dB at strike ts, else None."""
    tgrid = _time_list(now_, step_n, rate)
    if not _jump_list(now_, tgrid, tstamp, halfw):
        return None
    cut_v = tgrid < tstamp
    head_v, tail_v = _cut(now_, cut_v), _cut(now_, ~cut_v)
    return ([head_v] if len(head_v["k"]) >= 3 else []) + [tail_v]


def _jump_list(now_: dict, tgrid: np.ndarray, tstamp: float, halfw: float) -> bool:
    spn = tgrid[0] < tstamp - 0.5 * halfw and tgrid[-1] > tstamp + 0.55 * halfw
    later = (tgrid >= tstamp + 0.55 * halfw) & (tgrid < tstamp + 0.55 * halfw + 2 * halfw)
    earlier = (tgrid >= tstamp - 2 * halfw) & (tgrid < tstamp - 0.5 * halfw)
    ok = spn and later.any() and earlier.any()
    return bool(ok and db(now_["a"][later].max() ** 2) - db(now_["a"][earlier].max() ** 2) > 10)


def _echo_before(tm, hits, step_n, halfw, rate) -> bool:
    tgrid = _time_list(tm, step_n, rate)
    err = tm["a"] ** 2
    totv = max(float(err.sum()), 1e-30)
    for sv in hits:
        low_i, high_i = sv["t"] - 1.1 * halfw, sv["t"] + 0.55 * halfw
        inside = (tgrid >= low_i) & (tgrid <= high_i)
        if err[inside].sum() >= 0.9 * totv and low_i <= tgrid[-1] <= high_i:
            return True
    return False


def _repair_head(tm, hits, step_n, halfw, rate) -> None:
    tgrid = _time_list(tm, step_n, rate)
    birth = [sv for sv in hits if sv["t"] - 0.25 * halfw <= tgrid[0] <= sv["t"] + 1.05 * halfw]
    if birth:
        _remake_head(tm, tgrid, max(birth, key=lambda sv: sv["strength"]), step_n, halfw, rate)


def _remake_head(tm: dict, tgrid: np.ndarray, sv: dict, step_n: int, halfw: float, rate: int) -> None:
    """Start the track at strike s, with its smeared head replaced by the clean decay line."""
    tstamp = sv["t"]
    adj = 20 * np.log10(np.maximum(tm["a"], 1e-30))
    amp0 = _sharpen(adj, tgrid, tstamp, step_n, halfw, rate)
    tm["a"] = 10 ** (adj / 20)
    later = _post_strike(tgrid, tstamp)
    for kname in ("k", "f", "a", "c", "ph"):
        tm[kname] = tm[kname][later]
    tm["t0"], tm["rise"], tm["a0"] = tstamp, sv["rise"], 10 ** (amp0 / 20)


def _sharpen(adj: np.ndarray, tgrid: np.ndarray, tstamp: float, step_n: int, halfw: float, rate: int) -> float:
    """Rewrite the head of ad (dB, in place) from the clean frames. Returns the onset level (dB)."""
    maxv = adj.max()
    cln0 = tstamp + 0.55 * halfw
    pick = (tgrid >= cln0) & (tgrid <= cln0 + max(4 * step_n / rate, halfw))
    smr = tgrid < cln0
    if pick.sum() >= 2:
        slp, intercept = np.polyfit(tgrid[pick] - tstamp, adj[pick], 1)
        amp0 = float(np.clip(intercept, maxv - 40, maxv + 6))
        adj[smr] = np.minimum(intercept + slp * (tgrid[smr] - tstamp), maxv + 6)
    elif pick.sum() == 1:
        amp0 = float(adj[pick][0])
        adj[smr] = amp0
    else:
        amp0 = float(maxv)
        adj[:] = maxv
    return amp0


def _post_strike(tgrid: np.ndarray, tstamp: float) -> np.ndarray:
    later = tgrid > tstamp
    if not later.any():
        later = np.zeros(len(tgrid), bool)
        later[-1] = True
    return later


# ================================================================ phase-exact synthesis

def _exact_synth(lines_: list[dict], count: int, step_n: int, rate: int) -> np.ndarray:
    out_y = np.zeros(count)
    for tm in lines_:
        _mix_exact_into(out_y, tm, step_n, rate)
    return out_y


def _mix_exact_into(out_y, tm, step_n, rate):
    count = len(out_y)
    ix, fq, aa, phz = tm["k"], tm["f"], tm["a"], tm["ph"]
    wt = 2 * np.pi * fq / rate
    st0 = int(ix[0] * step_n)
    for ii in range(len(ix) - 1):
        chunk = _cubic_interp(phz[ii], phz[ii + 1], wt[ii], wt[ii + 1], aa[ii], aa[ii + 1], step_n)
        _place(out_y, int(ix[ii] * step_n), chunk)
    _attack_part(out_y, tm, st0, wt[0], aa[0], phz[0], step_n, rate)
    mm = np.arange(step_n)
    tail_v = aa[-1] * (1 - mm / step_n) * np.cos(phz[-1] + wt[-1] * mm)
    _place(out_y, int(ix[-1] * step_n), tail_v)
    del count


def _cubic_interp(thr0, thr1, wt0, wt1, amp0, a1, hmat):
    M = round(((thr0 + wt0 * hmat - thr1) + (wt1 - wt0) * hmat / 2) / (2 * np.pi))
    err = thr1 - thr0 - wt0 * hmat + 2 * np.pi * M
    alpha = 3 * err / hmat ** 2 - (wt1 - wt0) / hmat
    beta = -2 * err / hmat ** 3 + (wt1 - wt0) / hmat ** 2
    mm = np.arange(hmat)
    theta = thr0 + wt0 * mm + alpha * mm ** 2 + beta * mm ** 3
    return (amp0 + (a1 - amp0) * mm / hmat) * np.cos(theta)


def _attack_part(out_y, tm, st0, wt0, first_a, ph0, step_n, rate):
    if tm.get("t0") is not None:
        ii0 = int(round(tm["t0"] * rate))
        if ii0 >= st0:
            return
        mm = np.arange(ii0, st0)
        r_end = min(st0, ii0 + max(1, int(round(tm["rise"] * rate))))
        ampl = np.interp(mm, [ii0, r_end, st0], [0.0, tm["a0"], first_a])
    else:
        mm = np.arange(st0 - step_n, st0)
        ampl = first_a * (mm - (st0 - step_n)) / step_n
    _place(out_y, int(mm[0]), ampl * np.cos(ph0 + wt0 * (mm - st0)))


def _place(out_y, begin, chunk):
    aa = max(begin, 0)
    bb = min(len(out_y), begin + len(chunk))
    if bb > aa:
        out_y[aa:bb] += chunk[aa - begin:bb - begin]


# ================================================================ atoms

def _matrix_pencil(chunk: np.ndarray, max_atoms: int, max_t60: float, rdg: float, rate: float) -> list:
    cplx = _poles_by_pencil(chunk, max_atoms) if len(chunk) >= 16 and np.any(chunk) else None
    return [] if cplx is None else _atom_lstsq(chunk, _limit_poles(cplx, max_t60, rate), rdg, rate)[:max_atoms]


def _poles_by_pencil(chunk: np.ndarray, max_atoms: int):
    """Matrix-pencil poles of seg, or None when the SVD fails or no order is left."""
    pmat = len(chunk) // 3
    hmat = np.lib.stride_tricks.sliding_window_view(chunk, pmat + 1)
    try:
        _, sv, vh = np.linalg.svd(hmat, full_matrices=False)
    except np.linalg.LinAlgError:
        return None
    err = np.cumsum(sv ** 2) / np.sum(sv ** 2)
    ordv = min(int(np.searchsorted(err, 0.998)) + 2, 2 * max_atoms, len(sv) - 1)
    if ordv < 1:
        return None
    vmat = vh[:ordv].T
    return np.linalg.eigvals(np.linalg.pinv(vmat[:-1]) @ vmat[1:])


def _limit_poles(cplx, max_t60, rate):
    cplx = cplx[np.angle(cplx) >= 0]
    r_max = 10 ** (-3 / (max_t60 * rate))
    if max_t60 > 0.05:
        cplx = cplx[np.abs(cplx) <= r_max]
    else:
        big = np.abs(cplx) > r_max
        cplx = np.where(big, cplx / np.maximum(np.abs(cplx), 1e-30) * r_max, cplx)
    return _unique_poles(cplx[np.abs(cplx) >= 0.001])


def _unique_poles(cplx) -> np.ndarray:
    acc = []
    for pwr in cplx:
        if not any(abs(np.angle(pwr) - np.angle(qq)) < 1e-4 and abs(abs(pwr) - abs(qq)) < 1e-3 for qq in acc):
            acc.append(pwr)
    return np.asarray(acc)


def _atom_lstsq(chunk, cplx, rdg, rate):
    if not len(cplx):
        return []
    nband, holder = _decay_basis(cplx, len(chunk))
    lam = rdg * np.trace(nband.T @ nband) / nband.shape[1]
    coefs = np.linalg.solve(nband.T @ nband + lam * np.eye(nband.shape[1]), nband.T @ chunk)
    return _atom_set(cplx, holder, coefs, rate)


def _decay_basis(cplx, lwin: int):
    """Cosine (and sine, when not at 0 Hz) columns per pole, with (pole, part) owners."""
    ix = np.arange(lwin)
    cls, holder = [], []
    for ii, pwr in enumerate(cplx):
        rr, wt = abs(pwr), np.angle(pwr)
        cls.append(rr ** ix * np.cos(wt * ix))
        holder.append((ii, 0))
        if wt > 1e-9:
            cls.append(rr ** ix * np.sin(wt * ix))
            holder.append((ii, 1))
    return np.array(cls).T, holder


def _atom_set(cplx, holder, coefs, rate) -> list:
    """Atoms [Hz, tau s, amp, phase], strongest first."""
    csum = np.zeros((len(cplx), 2))
    for (ii, jj), val in zip(holder, coefs):
        csum[ii, jj] = val
    atom_l = []
    for pwr, (cc, sv) in zip(cplx, csum):
        rr, wt = abs(pwr), np.angle(pwr)
        ampl, phz = math.hypot(cc, sv), math.atan2(-sv, cc)
        atom_l.append([wt * rate / (2 * np.pi), -1 / (rate * np.log(rr)), ampl, phz])
    atom_l.sort(key=lambda aa: -(aa[2] ** 2 * aa[1]))
    return atom_l


def _atoms_at_strike(resid: np.ndarray, hits: list[dict], rate: int) -> list[dict]:
    sos = butter(4, 400, "low", fs=rate, output="sos")
    low_i = sosfiltfilt(sos, resid)
    high_i = resid - low_i
    sigpk = max(float(np.max(np.abs(resid))), 1e-30) if len(resid) else 1e-30
    acc = []
    for sv in sorted(hits, key=lambda sv: -sv["strength"])[:16]:
        err = _single_strike(high_i, low_i, sv, sigpk, rate)
        if err is not None:
            acc.append(err)
    return sorted(acc, key=lambda err: err["t"])


def _single_strike(high_i, low_i, sv, sigpk, rate) -> dict | None:
    amp0 = max(0, int(round((sv["t"] - 0.0005) * rate)))
    atom_l = (_atoms_per_band(high_i, amp0, int(0.008 * rate), 1, 24, 0.02, 1e-4, (240, rate), sigpk, rate)
             + _atoms_per_band(low_i, amp0, int(0.064 * rate), 8, 6, 0.096, 1e-3, (40, 600), sigpk, rate))
    if not atom_l:
        return None
    return {"t": round(amp0 / rate, 6), "atoms": [[round(fq, 2), round(tm, 7), round(aa, 7), round(pwr, 4)]
                                              for fq, tm, aa, pwr in atom_l]}


def _atoms_per_band(bnd, amp0, len_n, decv, max_atoms, t60, rdg, f_rng, sigpk, rate):
    chunk = bnd[amp0:amp0 + len_n]
    loud_v = len(chunk) >= 32 and np.max(np.abs(chunk)) >= sigpk * 10 ** (-40 / 20)
    atom_l = []
    if loud_v:
        atom_l = [aa for aa in _matrix_pencil(chunk[::decv], max_atoms, t60, rdg, rate / decv) if f_rng[0] <= aa[0] <= f_rng[1]]
    helps = bool(atom_l) and _atoms_line(bnd, amp0, len_n, atom_l, rate)
    return atom_l if helps else []


def _atoms_line(bnd, amp0, len_n, atom_l, rate) -> bool:
    """True when the atoms remove over 30 % of the band energy after a0."""
    spn = min(len(bnd) - amp0, 2 * len_n + int(0.02 * rate))
    refv = bnd[amp0:amp0 + spn]
    syn = synth_atoms([{"t": 0.0, "atoms": atom_l}], spn, rate)
    return bool(np.sum((refv - syn) ** 2) < 0.7 * np.sum(refv ** 2))


# ================================================================ residual model

_BIAS: dict[tuple, np.ndarray] = {}


def _offset_bias(band_l: tuple) -> np.ndarray:
    if band_l not in _BIAS:
        wt = np.random.default_rng(12345).standard_normal(2 * SR)
        _, envl = band_env(wt, band_l, SR)
        envl = median_filter(envl, size=(1, 5), mode="nearest")
        n_fft = len(wt) + 2 * 4096
        true = 10 * np.log10(np.mean(band_masks(band_l, n_fft) ** 2, axis=1))
        T = envl.shape[1]
        mid = envl[:, int(0.1 * T):int(0.9 * T)].mean(axis=1)
        _BIAS[band_l] = true - mid
    return _BIAS[band_l]


def _noise_part(resid: np.ndarray, band_l, tvals: np.ndarray, hits, rate: int) -> np.ndarray:
    _, _, Z = stft(resid, nperseg=2048, noverlap=2048 - 240, boundary="zeros")
    pmat = np.abs(Z) ** 2
    frac = median_filter(_flatness_by_band(pmat, band_l, rate), size=(1, 5), mode="nearest")
    fhat = np.arange(pmat.shape[1]) * 240 / rate
    acc = np.array([np.interp(tvals, fhat, sv) for sv in frac])
    for sv in hits:
        acc[:, (tvals >= sv["t"] - 0.002) & (tvals <= sv["t"] + 0.03)] = 0.0
    return acc


def _flatness_by_band(pmat: np.ndarray, band_l, rate: int) -> np.ndarray:
    """Noise share (dB) per band and frame from the median-to-mean ratio of whitened power."""
    pwm = pmat / np.maximum(median_filter(pmat, size=(31, 1), mode="nearest"), 1e-30)
    frac = np.zeros((len(band_l), pmat.shape[1]))
    for bb, pick in enumerate(_bins_in_band(band_l, rate)):
        if pick.sum() >= 5:
            qq = np.median(pwm[pick], axis=0) / np.log(2) / np.maximum(pwm[pick].mean(axis=0), 1e-30)
            frac[bb] = 10 * np.log10(np.clip(qq, 1e-4, 1))
    return frac


def _bins_in_band(band_l, rate: int) -> list[np.ndarray]:
    """Bin masks of a 2048-point spectrum, split halfway between band centres in log frequency."""
    lfreq = np.log2(np.maximum(np.fft.rfftfreq(2048, 1 / rate), 1e-9))
    lbin = np.log2(np.asarray(band_l))
    edg = np.concatenate([[-np.inf], (lbin[:-1] + lbin[1:]) / 2, [np.inf]])
    return [(lfreq >= edg[bb]) & (lfreq < edg[bb + 1]) for bb in range(len(band_l))]


def _model_residual(resid: np.ndarray, hits, pkdb: float, cfg: dict, rate: int) -> dict:
    tvals, envl = band_env(resid, RES_BANDS, rate)
    envl = envl + _noise_part(resid, RES_BANDS, tvals, hits, rate)
    envl = median_filter(envl, size=(1, 5), mode="nearest") + _offset_bias(RES_BANDS)[:, None]
    floor = pkdb - cfg["res_range_db"]
    envl = np.maximum(envl, floor)
    alive = envl.max(axis=1) > floor + 1
    kept = _residual_points(tvals, envl, alive)
    level = np.round(envl[:, kept], 1)
    level[np.abs(level - floor) <= 0.05] = -150.0
    level[~alive] = -150.0
    return {"bands": [round(bb, 2) for bb in RES_BANDS], "times": [round(float(tvals[ii]), 5) for ii in kept],
            "db": level.tolist(), "hf": hf_rolloff(resid, rate)}


def _residual_points(tvals: np.ndarray, envl: np.ndarray, alive: np.ndarray) -> list[int]:
    if alive.any() and len(tvals) > 2:
        return rdp_rows(tvals, envl[alive], 2.0, 96)
    return [0, len(tvals) - 1] if len(tvals) > 1 else [0]


def hf_rolloff(x: np.ndarray, sr: int = 48000, points=HF_POINTS) -> list:
    """Roll-off of x above 16 kHz: [[Hz, dB re 16 kHz], ...], monotone falling."""
    x = np.asarray(x, dtype=float)
    chunk = min(4096, 2 ** int(np.floor(np.log2(max(len(x), 256)))))
    fq, pwr = welch(np.pad(x, (0, max(0, chunk - len(x)))), fs=sr, nperseg=chunk)
    values_ = _levels_at_points(fq, pwr, points, sr)
    relv = np.minimum.accumulate(np.minimum(np.asarray(values_) - values_[0], 0.0))
    relv = np.maximum(relv, -80.0)
    return [[float(cc), round(float(val), 1)] for cc, val in zip(points, relv)]


def _levels_at_points(fq: np.ndarray, pwr: np.ndarray, pts, rate: int) -> list[float]:
    values_ = []
    for cc in pts:
        pick = (fq >= cc * 2 ** (-1 / 12)) & (fq <= min(cc * 2 ** (1 / 12), rate / 2))
        values_.append(10 * np.log10(max(pwr[pick].mean(), 1e-300)) if pick.any() else -300.0)
    return values_


# ================================================================ breakpoints

def _bp_list(tm: dict, step_n: int, rate: int, cfg: dict) -> dict:
    tgrid = tm["k"] * step_n / rate
    adj = 20 * np.log10(np.maximum(tm["a"], 1e-30))
    fq = tm["f"] if len(tm["f"]) < 5 else median_filter(tm["f"], 5, mode="nearest")
    tm0, rise_v, amp0, phz = _first_point(tm, tgrid, adj, fq, step_n, rate)
    where_, avg, fval = np.concatenate([[tm0], tgrid]), np.concatenate([[amp0], adj]), np.concatenate([[fq[0]], fq])
    return _bp_record(tm0, rise_v, tgrid[-1], step_n / rate, phz, (where_, avg, fval), cfg)


def _bp_record(tm0, rise_v, t1, relv, phz, knots, cfg) -> dict:
    where_, avg, fval = knots
    ia = rdp(where_, avg, cfg["amp_tol_db"], 64)
    iff = _freq_points(where_, avg, fval, cfg)
    return {"t0": round(float(tm0), 5), "rise": round(float(rise_v), 5), "t1": round(float(t1), 5),
            "rel": round(relv, 5), "ph": round(float(np.angle(np.exp(1j * phz))), 3),
            "freq": [[round(float(where_[ii]), 5), round(float(fval[ii]), 2)] for ii in iff],
            "amp": [[round(float(where_[ii]), 5), round(float(avg[ii]), 1)] for ii in ia]}


def _first_point(tm: dict, tgrid, adj, fq, step_n: int, rate: int):
    """(t0, rise, level dB, phase) at the start: the fixed strike onset, or one hop before the track."""
    if tm.get("t0") is not None:
        tm0, rise_v = tm["t0"], max(tm["rise"], 0.0002)
        return tm0, rise_v, 20 * np.log10(max(tm["a0"], 1e-30)), tm["ph"][0] - 2 * np.pi * fq[0] * (tgrid[0] - tm0)
    return tgrid[0] - step_n / rate, max(step_n / rate, 0.0002), adj[0], tm["ph"][0] - 2 * np.pi * fq[0] * step_n / rate


def _freq_points(where_, avg, fval, cfg) -> list[int]:
    cent_v = 1200 * np.log2(np.maximum(fval, 1))
    ftol = cfg["freq_tol_cents"] * 2 ** np.clip((avg.max() - avg) / 20, 0, 4)
    return rdp(where_, cent_v, ftol, 48)


def _top_of(pwr: dict) -> float:
    return max(aa[1] for aa in pwr["amp"])


# ================================================================ analysis per window

def _base_options(**kw) -> dict:
    cfg = {"max_partials": 160, "dyn_db": 80, "keep_db": 60, "prom_db": 4, "max_peaks": 100,
            "min_ms": 15, "amp_tol_db": 1, "freq_tol_cents": 1, "res_range_db": 140,
            "transients": True, "passes": 2, "snr_db": 10}
    cfg.update({ix: val for ix, val in kw.items() if ix in cfg})
    return cfg


def _track_set(sig: np.ndarray, win_n: int, hits, cfg: dict, rate: int, space_left: int,
               g_ref: float | None = None, refpk: float | None = None) -> tuple[list, dict]:
    frame = _frames_stft(sig, win_n)
    magv = 20 * np.log10(np.maximum(np.abs(frame["X"]), 1e-30))
    phs0 = np.angle(frame["X"])
    gg = float(magv.max()) if g_ref is None else g_ref
    pk_list = [_peaks_in_frame(magv[ix], phs0[ix], gg, frame, cfg, rate) for ix in range(len(magv))]
    lines_ = _track_one(pk_list, win_n, rate)
    for tm in lines_:
        _phase_fill(tm, frame["hop"], rate)
    lines_ = _prune_tracks(lines_, frame, cfg, rate, space_left, refpk)
    return _repair_onsets(lines_, hits, frame, rate), frame | {"gref": gg}


def analyze(x: np.ndarray, sr: int, W: int, onsets: list[dict], width: float, opts: dict) -> dict:
    """One window size: {'model', 'det', 'res', 'atoms', 'tracks'}."""
    opts = _base_options(**(opts or {}))
    x = np.asarray(x, dtype=float)
    count = len(x)
    lines_, frame = _track_set(x, W, onsets, opts, sr, opts["max_partials"])
    detail = _exact_synth(lines_, count, frame["hop"], sr)
    if opts["passes"] >= 2 and opts["max_partials"] - len(lines_) > 0:
        lines_, detail = _refit_pass(x, detail, lines_, W, onsets, opts, sr, frame)
    resid = x - detail
    transp = _atoms_at_strike(resid, onsets, sr) if opts["transients"] and onsets else []
    atom_l = synth_atoms(transp, count, sr)
    mdl = _model_of_sines(x, sr, onsets, width, lines_, frame["hop"], transp, resid - atom_l, opts)
    mdl["analysis"] = _analysis_meta(x, detail, resid - atom_l, W, frame["hop"])
    return {"model": mdl, "det": detail, "res": resid, "atoms": atom_l, "tracks": lines_}


def _analysis_meta(sig, detail, remain, win_n: int, step_n: int) -> dict:
    exv = max(float(np.sum(sig ** 2)), 1e-30)
    return {"window": win_n, "hop": step_n, "sines_share_db": round(float(db(np.sum(detail ** 2) / exv)), 2),
            "residual_db": round(float(db(np.sum(remain ** 2) / exv)), 2)}


def _model_of_sines(sig, rate, onsets, wdt, lines_, step_n, transp, remain, cfg) -> dict:
    count = len(sig)
    pkdb = float(20 * np.log10(max(np.max(np.abs(sig)), 1e-30))) if count else -300.0
    partials = sorted((_bp_list(tm, step_n, rate, cfg) for tm in lines_), key=lambda pwr: -_top_of(pwr))
    return {"kind": KIND, "version": VERSION, "sr": SR, "duration": round(count / rate, 5),
            "onset": round(onsets[0]["t"], 5) if onsets else 0.0,
            "onsets": [round(sv["t"], 5) for sv in onsets], "peak_db": round(pkdb, 2),
            "width": round(float(wdt), 1), "partials": partials, "transients": transp,
            "residual": _model_residual(remain, onsets, pkdb, cfg, rate), "coherent": True}


def _refit_pass(sig, detail, lines_, win_n, hits, cfg, rate, frame):
    space_left = cfg["max_partials"] - len(lines_)
    resid = sig - detail
    refpk = max((20 * np.log10(max(tm["a"].max(), 1e-30)) for tm in lines_), default=None)
    extra, _ = _track_set(resid, win_n, hits, cfg, rate, space_left, frame["gref"], refpk)
    if not extra:
        return lines_, detail
    add = _exact_synth(extra, len(sig), frame["hop"], rate)
    if np.sum((resid - add) ** 2) < 0.98 * np.sum(resid ** 2):
        return lines_ + extra, detail + add
    return lines_, detail


# ================================================================ fit

def _log_spec_dist(aa: np.ndarray, bb: np.ndarray) -> float:
    count = max(len(aa), len(bb))
    aa, bb = np.pad(aa, (0, count - len(aa))), np.pad(bb, (0, count - len(bb)))
    acc = [val for val in (_log_spec_dist_at(aa, bb, size) for size in (256, 1024, 4096) if count >= size) if val is not None]
    return float(np.mean(acc)) if acc else 0.0


def _log_spec_dist_at(aa: np.ndarray, bb: np.ndarray, size: int) -> float | None:
    amat = 20 * np.log10(np.maximum(np.abs(stft(aa, nperseg=size, noverlap=size - size // 4)[2]), 1e-12))
    nband = 20 * np.log10(np.maximum(np.abs(stft(bb, nperseg=size, noverlap=size - size // 4)[2]), 1e-12))
    flv = max(amat.max(), nband.max()) - 60
    actv = (amat > flv) | (nband > flv)
    if not actv.any():
        return None
    return float(np.mean(np.minimum(np.abs(np.maximum(amat, flv) - np.maximum(nband, flv)), 40)[actv]))


FIT_KNOBS = {"sr": 48000, "windows": WINDOWS, "max_partials": 160, "dyn_db": 80, "keep_db": 60, "prom_db": 4,
             "max_peaks": 100, "min_ms": 15, "amp_tol_db": 1, "freq_tol_cents": 1, "res_range_db": 140,
             "transients": True, "passes": 2, "snr_db": 10, "source": None}


def fit(x: Any, **kw) -> dict:
    """A sines model of a path or an array at ``sr`` (mono or (n, 2)). Knobs and defaults: FIT_KNOBS."""
    ix = merge_knobs(FIT_KNOBS, kw, "sines fit")
    name = ix["source"] or source_name(x)
    aa = as_stereo(x) if isinstance(x, str) else as_stereo(resample(np.asarray(x, float), ix["sr"]))
    mdl = _sines_fit_core(aa, ix["windows"], _base_options(**ix))
    mdl["source"] = name
    mdl["summary"] = summary(mdl)
    return mdl


def _sines_fit_core(aa: np.ndarray, windows, cfg: dict) -> dict:
    stereo = not is_mono_pair(aa)
    mono_v = aa.mean(axis=1)
    wdt = _width_db(aa) if stereo else -120.0
    hits = onsets(mono_v, SR)
    dlt, mdl, attempts = _pick_window(mono_v, windows, hits, wdt, cfg)
    if stereo and wdt > -60:
        mdl["side"] = side_model(aa, mdl["duration"])
    mdl["channels"] = 2 if stereo else 1
    mdl["analysis"].update({"lsd_db": round(dlt, 3), "tried": attempts,
                              "onset_rise_ms": round(hits[0]["rise"] * 1000, 3) if hits else None})
    return mdl


def _pick_window(mono_v: np.ndarray, windows, hits, wdt: float, cfg: dict):
    """(LSD, model, tried) of the window size whose render is closest to mono."""
    winner, attempts = None, {}
    for win_n in windows:
        if winner is None or win_n <= 2 * len(mono_v):
            mm = analyze(mono_v, SR, win_n, hits, wdt, cfg)["model"]
            dlt = _log_spec_dist(render(mm, 0), mono_v)
            attempts[str(win_n)] = round(dlt, 3)
            if winner is None or dlt < winner[0]:
                winner = (dlt, mm)
    return winner[0], winner[1], attempts


def _width_db(aa: np.ndarray) -> float:
    sv = (aa[:, 0] - aa[:, 1]) / 2
    mm = (aa[:, 0] + aa[:, 1]) / 2
    emv = float(np.sum(mm ** 2))
    if emv <= 0:
        return -120.0
    return max(-120.0, float(10 * np.log10(max(np.sum(sv ** 2), 1e-300) / emv)))


# ================================================================ side model

def _key_moments(ons: float, dur_s: float) -> np.ndarray:
    tstamp = [0.0, ons - 0.01, ons]
    tm = ons
    for stride, until in ((0.01, 0.1), (0.02, 0.3), (0.04, 0.7)):
        while tm + stride <= ons + until + 1e-9:
            tm += stride
            tstamp.append(tm)
    while tm + 0.08 < dur_s:
        tm += 0.08
        tstamp.append(tm)
    tstamp.append(dur_s)
    return np.unique(np.round(np.clip(tstamp, 0.0, dur_s), 6))


def side_model(st: np.ndarray, duration: float | None = None, bands=SIDE_BANDS,
               tol: float = 0.04) -> dict:
    """The fitted side of a stereo source: pan and diffuse gains per band and keyframe."""
    aa = as_stereo(st)
    dur_s = len(aa) / SR if duration is None else float(duration)
    knames = _key_moments(space.active_span(aa)[0] / SR, dur_s)
    panpos, diff_v = _gains_of_side(space.side_powers(aa, bands), knames)
    return _side_record(bands, knames, panpos, diff_v, tol)


def _gains_of_side(pmat: dict, knames: np.ndarray):
    pmag, pspec, cmat = (_tri_sum(pmat["t"], pmat[ix], knames) for ix in ("PM", "PS", "C"))
    panpos, diff_v = _gain_list(pmag, pspec, cmat)
    return _hush_fix(pmag, pspec, cmat, panpos, diff_v)


def _side_record(band_l, knames: np.ndarray, panpos: np.ndarray, diff_v: np.ndarray, tolv: float) -> dict:
    kept = rdp_rows(knames, np.vstack([panpos, diff_v]), tolv) if len(knames) > 2 else list(range(len(knames)))
    return {"bands": [float(bb) for bb in band_l], "times": [round(float(knames[ii]), 5) for ii in kept],
            "pan": np.round(panpos[:, kept], 3).tolist(), "diffuse": np.round(diff_v[:, kept], 3).tolist()}


def _tri_sum(fhat: np.ndarray, pbm: np.ndarray, knames: np.ndarray) -> np.ndarray:
    acc = np.zeros((pbm.shape[0], len(knames)))
    for ii, ix in enumerate(knames):
        low_i = knames[ii - 1] if ii > 0 else ix - 0.01
        high_i = knames[ii + 1] if ii + 1 < len(knames) else ix + 0.01
        wt = np.where(fhat <= ix, (fhat - low_i) / max(ix - low_i, 1e-9), (high_i - fhat) / max(high_i - ix, 1e-9))
        wt = np.clip(wt, 0.0, 1.0)
        if not wt.any():
            wt = np.zeros(len(fhat))
            wt[int(np.argmin(np.abs(fhat - ix)))] = 1.0
        acc[:, ii] = pbm @ wt
    return acc


def _gain_list(pmag, pspec, cmat):
    pmv = np.maximum(pmag, 1e-30)
    panpos = np.clip(cmat / pmv, -1.5, 1.5)
    diff_v = np.clip(np.sqrt(np.maximum(pspec - cmat ** 2 / pmv, 0) / pmv), 0, 2)
    return panpos, diff_v


def _hush_fix(pmag, pspec, cmat, panpos, diff_v):
    whole_p, whole_d = _gain_list(pmag.sum(axis=1, keepdims=True), pspec.sum(axis=1, keepdims=True),
                              cmat.sum(axis=1, keepdims=True))
    all_p, all_d = _gain_list(pmag.sum(), pspec.sum(), cmat.sum())
    totv = pmag.sum(axis=1)
    for bb in range(pmag.shape[0]):
        if totv[bb] < 1e-6 * max(totv.max(), 1e-300):
            panpos[bb], diff_v[bb] = all_p, all_d
            continue
        hush = db(pmag[bb]) < db(pmag[bb].max()) - 50
        panpos[bb, hush], diff_v[bb, hush] = whole_p[bb, 0], whole_d[bb, 0]
    return panpos, diff_v


def morph_side(sa: dict | None, sb: dict | None, w: float, map_a: Callable | None = None,
               map_b: Callable | None = None) -> dict | None:
    """The side of a morph: both sides on the union of mapped times, mixed linearly."""
    if sa and sb and list(sa["bands"]) != list(sb["bands"]):
        raise ValueError("side bands differ between the two models")
    return _blend(sa, sb, w, map_a, map_b) if (sa or sb) else None


def _blend(seg_a, seg_b, wt, amap, bmap) -> dict:
    refv = seg_a or seg_b
    nbin = len(refv["bands"])
    tma, tmb = _map_time(seg_a, amap), _map_time(seg_b, bmap)
    grid_v = np.unique(np.round(np.concatenate([tma, tmb]), 6))
    acc = {"bands": list(refv["bands"]), "times": grid_v.tolist()}
    for kname in ("pan", "diffuse"):
        ga = _to_grid(seg_a, kname, tma, grid_v, nbin)
        gb = _to_grid(seg_b, kname, tmb, grid_v, nbin)
        acc[kname] = np.round((1 - wt) * ga + wt * gb, 3).tolist()
    return acc


def _map_time(sv, fnc):
    if not sv:
        return np.zeros(0)
    tm = np.asarray(sv["times"], dtype=float)
    return np.asarray([fnc(val) for val in tm]) if fnc else tm


def _to_grid(sv, kname, tm, grid_v, nbin):
    if not sv:
        return np.zeros((nbin, len(grid_v)))
    return np.array([np.interp(grid_v, tm, rw) for rw in sv[kname]])


def vary_field(r: np.random.Generator, parts: int = 6) -> Callable[[Any], np.ndarray]:
    """A smooth random function of frequency with unit variance."""
    cc = r.uniform(0.25, 1.25, parts)
    pwr = r.uniform(0.0, 2 * np.pi, parts)
    scale = np.sqrt(2.0 / parts)

    def field(fq):
        lfreq = np.log2(np.maximum(np.asarray(fq, dtype=float), 20.0))
        return scale * np.sum(np.cos(2 * np.pi * np.multiply.outer(lfreq, cc) + pwr), axis=-1)

    return field


# ================================================================ render

def check(model: Any) -> None:
    check_kind(model, KIND, VERSION)


def fade_in_s(model: dict) -> float:
    return K.sines_fade_in(model)


def partial_freq(p: dict) -> float:
    """Energy-weighted mean frequency of a partial."""
    frame = np.asarray(p["freq"], dtype=float)
    amv = np.asarray(p["amp"], dtype=float)
    tm = np.linspace(p["t0"], max(p["t1"], p["t0"] + 1e-6), 64)
    fq = 2 ** np.interp(tm, frame[:, 0], np.log2(np.maximum(frame[:, 1], 1e-6)))
    err = 10 ** (np.interp(tm, amv[:, 0], amv[:, 1]) / 10)
    return float(np.sum(fq * err) / max(np.sum(err), 1e-300))


def _knob_values(knobs: dict | None, kw: dict) -> dict:
    ix = {"transpose": 0.0, "stretch": 1.0, "decay": 1.0, "brightness": 0.0, "vibrato": 0.0,
         "vibrato_rate": 5.5, "drift": 0.0, "residual": 1.0, "transient": 1.0, "vary": 0.0,
         "partials": None, "inharm": 0.0, "phases": "fit", "gain_db": 0.0, "stereo": False,
         "width": 1.0, "ring": None}
    supplied = dict(knobs or {}, **kw)
    badv = sorted(set(supplied) - set(ix))
    if badv:
        raise ValueError(f"unknown sines knob(s) {badv}; known: {sorted(ix)}")
    ix.update(supplied)
    if ix["ring"] is None:
        ix["ring"] = ix["vary"] > 0
    return ix


def _knobs_neutral(ix: dict) -> bool:
    return (ix["stretch"] == 1 and ix["decay"] == 1 and ix["transpose"] == 0 and ix["vary"] == 0
            and ix["brightness"] == 0 and ix["inharm"] == 0 and ix["phases"] == "fit")


def render(model: dict, seed: int = 0, knobs: dict | None = None, **kw) -> np.ndarray:
    """float64 at 48 kHz: mono, or (n, 2) with ``stereo=True``."""
    check(model)
    ix = _knob_values(knobs, kw)
    ident_v = _knobs_neutral(ix)
    jit = np.random.default_rng((int(seed) * 7919 + 17) & 0xFFFFFFFF)
    state = _render_state(model, ix, jit)
    pieces, tvals, lvls, count = _shifted(model, ix, state, jit, ident_v)
    cent_v = _pitch_bend(count, ix, state, jit)
    out_y = synth_partials(pieces, count, SR, cent_v)
    out_y = out_y + synth_atoms(_shifted_atoms(model, ix, state, jit, ident_v), count, SR)
    out_y = out_y + _noise_render(model, tvals, lvls, count, seed, ix, state)
    out_y = out_y * 10 ** (ix["gain_db"] / 20)
    if ix["stereo"]:
        out_y = _to_stereo(model, out_y, seed, ix, state)
    return K.sines_finish(model, out_y)


def _render_state(mdl: dict, ix: dict, jit) -> dict:
    sumv = mdl.get("summary") or {}
    state = {"ratio": 2 ** (ix["transpose"] / 12), "f_ref": sumv.get("centroid") or 1000.0,
           "f_low": sumv.get("f_low") or 100.0, "on": float(mdl.get("onset", 0.0)),
           "fields": None}
    if mdl.get("coherent") and ix["vary"] > 0:
        state["fields"] = [vary_field(jit) for _ in range(3)]
    return state


def _shifted(mdl: dict, ix: dict, state: dict, jit, ident_v: bool):
    """Edited partials, residual times and levels, and the take length in samples."""
    pieces = sorted(mdl["partials"], key=lambda pwr: -_top_of(pwr))
    if ix["partials"] is not None:
        pieces = pieces[:int(ix["partials"])]
    if not ident_v:
        pieces = [_shift_partial(pwr, ix, state, jit) for pwr in pieces]
    resid = mdl["residual"]
    tvals, lvls = resid["times"], resid["db"]
    if not ident_v or ix["residual"] != 1:
        tvals, lvls = _shift_residual(resid, ix, state, jit)
    count = _out_length(mdl, pieces, ix, state)
    if ix["ring"]:
        count, tvals, lvls = _ringing(mdl, count, tvals, lvls, ix, state)
    return pieces, tvals, lvls, count


def _shifted_atoms(mdl: dict, ix: dict, state: dict, jit, ident_v: bool) -> list:
    transp = mdl.get("transients", [])
    return _shift_atoms(transp, ix, state, jit) if (not ident_v or ix["transient"] != 1) else transp


def _noise_render(mdl: dict, tvals, lvls, count: int, seed: int, ix: dict, state: dict) -> np.ndarray:
    resid = mdl["residual"]
    nz = synth_noise(resid["bands"], tvals, lvls, count, seed, SR, hf=resid.get("hf"))
    if ix["transient"] != 1:
        nz = nz * (1 + (ix["transient"] - 1) * _weight_of_strike(mdl, count, state, ix))
    return nz


def _TimeAxis(tm, ix, state):
    return state["on"] + (np.asarray(tm, dtype=float) - state["on"]) * ix["stretch"]


def _shift_partial(pwr: dict, ix: dict, state: dict, jit) -> dict:
    pwr = copy.deepcopy(pwr)
    fmid = partial_freq(pwr)
    detail, level, dj = _offsets_of_partials(fmid, ix["vary"], state, jit)
    if ix["phases"] == "random":
        pwr["ph"] = float(jit.uniform(-np.pi, np.pi))
    mul = state["ratio"] * (fmid / state["f_low"]) ** ix["inharm"] * 2 ** (float(detail) / 1200)
    pwr["freq"] = [[tm, fq * mul] for tm, fq in pwr["freq"]]
    bright = ix["brightness"] * np.log2(max(fmid * state["ratio"], 1) / state["f_ref"])
    _amp_decay(pwr, [[tm, aa + float(level) + bright] for tm, aa in pwr["amp"]], ix["decay"] * float(dj))
    _partial_stretch(pwr, ix, state)
    return pwr


def _offsets_of_partials(fmid: float, val: float, state: dict, jit):
    """Detune (cents), level (dB) and decay factor of one partial, from the fields or the generator."""
    fld_l = state["fields"]
    if fld_l:
        return 8 * val * fld_l[0](fmid), 2 * val * fld_l[1](fmid), np.exp(0.15 * val * fld_l[2](fmid))
    return 8 * val * jit.standard_normal(), 2 * val * jit.standard_normal(), np.exp(0.15 * val * jit.standard_normal())


def _amp_decay(pwr: dict, ampl: list, fdv: float) -> None:
    """Scale the time after the amplitude peak by fd."""
    tpk = max(ampl, key=lambda rr: rr[1])[0]
    pwr["amp"] = [[tpk + (tm - tpk) * fdv if tm > tpk else tm, aa] for tm, aa in ampl]
    if pwr["t1"] > tpk:
        pwr["t1"] = tpk + (pwr["t1"] - tpk) * fdv


def _partial_stretch(pwr: dict, ix: dict, state: dict) -> None:
    pwr["amp"] = [[float(_TimeAxis(tm, ix, state)), aa] for tm, aa in pwr["amp"]]
    pwr["freq"] = [[float(_TimeAxis(tm, ix, state)), fq] for tm, fq in pwr["freq"]]
    pwr["t0"], pwr["t1"] = float(_TimeAxis(pwr["t0"], ix, state)), float(_TimeAxis(pwr["t1"], ix, state))
    pwr["rel"] = pwr["rel"] * max(1.0, ix["stretch"])


def _shift_residual(resid: dict, ix: dict, state: dict, jit):
    level = _shifted_levels(resid, ix, state, jit)
    tm = np.asarray(resid["times"], dtype=float)
    pivot = state["on"] + 0.01
    tm = np.where(tm > pivot, pivot + (tm - pivot) * ix["decay"], tm)
    return _TimeAxis(tm, ix, state).tolist(), level.tolist()


def _shifted_levels(resid: dict, ix: dict, state: dict, jit) -> np.ndarray:
    level = np.asarray(resid["db"], dtype=float)
    off = level <= -149
    band_l = np.asarray(resid["bands"], dtype=float)
    lbin = np.log2(band_l)
    if state["ratio"] != 1:
        level = np.array([np.interp(lbin - np.log2(state["ratio"]), lbin, level[:, jj]) for jj in range(level.shape[1])]).T
    level = level + (ix["brightness"] * np.log2(band_l * state["ratio"] / state["f_ref"]))[:, None]
    if ix["vary"]:
        level = level + (1.5 * ix["vary"] * jit.standard_normal(len(band_l)))[:, None]
    level = level + 20 * np.log10(max(ix["residual"], 1e-6))
    level[off] = -150.0
    return level


def _out_length(mdl, pieces, ix, state) -> int:
    n_id = int(round(mdl["duration"] * SR))
    if ix["decay"] > 1 or ix["stretch"] != 1 or ix["ring"]:
        ends = [pwr["t1"] + pwr["rel"] for pwr in pieces] or [0.0]
        return int(np.ceil(max(max(ends), float(_TimeAxis(mdl["duration"], ix, state))) * SR)) + 1
    return n_id


def _ringing(mdl, count, tvals, lvls, ix, state):
    stop_ = max(float(tvals[-1]), float(_TimeAxis(mdl["duration"], ix, state))) + 0.06
    count = max(count, int(np.ceil(stop_ * SR)))
    tvals = list(tvals) + [stop_]
    lvls = [list(rw) + [-150.0] for rw in lvls]
    return count, tvals, lvls


def _pitch_bend(count, ix, state, jit):
    if not ix["vibrato"] and not ix["drift"]:
        return None
    tm = np.arange(count) / SR
    cent_v = np.zeros(count)
    if ix["vibrato"]:
        fade = np.clip((tm - state["on"]) / 0.15, 0, 1)
        cent_v += ix["vibrato"] * np.sin(2 * np.pi * ix["vibrato_rate"] * tm + jit.uniform(0, 2 * np.pi)) * fade
    if ix["drift"]:
        rates = 1.5 * jit.uniform(0.2, 1.0, 4)
        phs = jit.uniform(0, 2 * np.pi, 4)
        cent_v += ix["drift"] * np.sum(np.sin(2 * np.pi * np.outer(tm, rates) + phs), axis=1) / 2
    return cent_v


def _shift_atoms(transp, ix, state, jit):
    cohere = bool(state["fields"] is not None or ix["vary"] == 0)
    return [{"t": float(_TimeAxis(trk["t"], ix, state)), "atoms": _atoms_as_list(trk, ix, state, jit, cohere)} for trk in transp]


def _atoms_as_list(trk: dict, ix: dict, state: dict, jit, cohere: bool) -> list:
    """Moved atoms of one transient. Coherent: one random offset per transient, else one per atom."""
    val = ix["vary"]
    dcv, dgv = (30 * val * jit.standard_normal(), 2 * val * jit.standard_normal()) if cohere else (0, 0)
    atom_l = []
    for fq, tau_v, aa, phz in trk["atoms"]:
        if not cohere:
            dcv, dgv = 30 * val * jit.standard_normal(), 2 * val * jit.standard_normal()
        atom_l.append(_shifted_atom((fq, tau_v, aa, phz), dcv, dgv, ix, state))
    return atom_l


def _shifted_atom(atom, dcv: float, dgv: float, ix: dict, state: dict) -> list:
    fq, tau_v, aa, phz = atom
    f2 = fq * state["ratio"] * 2 ** (dcv / 1200)
    gg = 10 ** ((dgv + ix["brightness"] * np.log2(max(fq * state["ratio"], 20) / state["f_ref"])) / 20)
    return [f2, tau_v / state["ratio"], aa * ix["transient"] * gg, phz]


def _weight_of_strike(mdl, count, state, ix):
    tm = np.arange(count) / SR
    wt = np.zeros(count)
    for sv in mdl.get("onsets", []):
        tstamp = float(_TimeAxis(sv, ix, state))
        dlt = tm - tstamp
        tri = np.clip(1 - np.abs(dlt - 0.0075) / 0.0125, 0, 1) * (dlt >= -0.002)
        wt = np.maximum(wt, tri)
    return wt


def _to_stereo(mdl, out_y, seed, ix, state):
    side_v = mdl.get("side")
    sv = _side_of_fit(side_v, out_y, seed, ix, state) if side_v else _side_noise(mdl, len(out_y), seed, ix)
    sv = sv * ix["width"]
    return np.column_stack([out_y + sv, out_y - sv])


def _side_of_fit(side_v: dict, out_y, seed, ix, state) -> np.ndarray:
    tm = np.asarray(side_v["times"], dtype=float)
    pivot = state["on"] + 0.01
    tm = _TimeAxis(np.where(tm > pivot, pivot + (tm - pivot) * ix["decay"], tm), ix, state)
    rr = np.random.default_rng((int(seed) * 31 + 7) & 0xFFFFFFFF)
    sign = -1.0 if rr.uniform() < 0.5 else 1.0
    return space.shaped_side(out_y, np.asarray(side_v["bands"]) * state["ratio"], tm, side_v["pan"], side_v["diffuse"], sign)


def _side_noise(mdl: dict, count: int, seed, ix) -> np.ndarray:
    resid = mdl["residual"]
    return synth_noise(resid["bands"], resid["times"], resid["db"], count, int(seed) + 1000003, SR,
                       hf=resid.get("hf")) * 10 ** (mdl.get("width", -120) / 20) * 10 ** (ix["gain_db"] / 20)


# ================================================================ summary, io, code

def summary(model: dict) -> dict:
    pieces = model.get("partials") or []
    return _summary_text(pieces) if pieces else {"partials": 0}


def _summary_text(pieces: list) -> dict:
    pk_list = np.array([_top_of(pwr) for pwr in pieces])
    fmean = np.array([partial_freq(pwr) for pwr in pieces])
    loudest = _strongest(pk_list, pieces)
    hh = _harmonic_info(pk_list[loudest], fmean[loudest])
    glide_v = [round(1200 * np.log2(pwr["freq"][-1][1] / pwr["freq"][0][1]), 1) for pwr in pieces[:8]]
    return {"partials": len(pieces), "f_low": hh["f_low"], "centroid": hh["centroid"],
            "f_range": [round(float(fmean.min()), 1), round(float(fmean.max()), 1)], "top": _highest(pieces, pk_list, fmean),
            "ratios": hh["ratios"], "inharmonicity": hh["inh"], "glide_cents": glide_v}


def _harmonic_info(pk: np.ndarray, fq: np.ndarray) -> dict:
    """Lowest frequency, centroid, ratios and inharmonicity of the strong partials."""
    flow_ = float(fq.min())
    ratio_l = np.sort(fq / flow_)[:12]
    inhv = _inharm_value(ratio_l) if len(fq) > 1 else 0.0
    return {"f_low": round(flow_, 2), "centroid": round(_spec_centre(pk, fq), 2),
            "ratios": [round(float(rr), 3) for rr in ratio_l], "inh": round(inhv, 4)}


def _spec_centre(pk_list: np.ndarray, fmean: np.ndarray) -> float:
    """Power-weighted mean log frequency, in Hz."""
    powv = 10 ** (pk_list / 10)
    return float(2 ** (np.sum(powv * np.log2(fmean)) / powv.sum()))


def _strongest(pk_list: np.ndarray, pieces: list) -> np.ndarray:
    """Partials within 30 dB of the top that live long enough (or all within 30 dB)."""
    lifev = np.array([pwr["t1"] - pwr["t0"] for pwr in pieces])
    loudest = (pk_list >= pk_list.max() - 30) & (lifev >= min(0.1, 0.5 * lifev.max()))
    return loudest if loudest.any() else pk_list >= pk_list.max() - 30


def _inharm_value(ratio_l: np.ndarray) -> float:
    return float(np.median(np.abs(ratio_l - np.round(ratio_l)) / np.maximum(np.round(ratio_l), 1)))


def _highest(pieces: list, pk_list: np.ndarray, fmean: np.ndarray) -> list[dict]:
    return [{"f": round(float(fmean[ii]), 1), "db": round(float(pk_list[ii]), 1), "t60": _decay_t60(pieces[ii])}
            for ii in np.argsort(-pk_list)[:8]]


def _decay_t60(pwr: dict) -> float | None:
    amv = np.asarray(pwr["amp"], dtype=float)
    ii = int(np.argmax(amv[:, 1]))
    chunk = amv[ii:]
    if len(chunk) < 2 or chunk[-1, 0] - chunk[0, 0] < 0.01:
        return None
    slp = float(np.polyfit(chunk[:, 0], chunk[:, 1], 1)[0])
    return round(-60 / slp, 3) if slp < -0.5 else None


def describe(model: dict) -> str:
    sv = model.get("summary") or summary(model)
    on = model.get("onsets", [])
    head_v = f"strike at {model.get('onset', 0) * 1000:.0f} ms ({len(on)} strikes)"
    anv = model.get("analysis", {})
    tail_v = f"residual {anv.get('residual_db', 'n/a')} dB, window {anv.get('window')}, LSD {anv.get('lsd_db')}"
    body = _partials_words(sv) if sv.get("partials") else "noise only, no partials"
    return f"{head_v}; {body}; {tail_v}"


def _partials_words(sv: dict) -> str:
    inhv = sv["inharmonicity"]
    word = "harmonic" if inhv < 0.01 else "slightly inharmonic" if inhv < 0.04 else "inharmonic"
    ratio_l = ", ".join(f"{rr:g}" for rr in sv["ratios"][:5])
    return (f"{sv['partials']} partials {sv['f_range'][0]:.0f}-{sv['f_range'][1]:.0f} Hz, lowest "
            f"strong {sv['f_low']:.0f} Hz, {word} (ratios {ratio_l}), centroid {sv['centroid']:.0f} Hz, "
            f"{_t60_words(sv)}{_glide_words(sv)}")


def _t60_words(sv: dict) -> str:
    t60_l = [tm["t60"] for tm in sv["top"] if tm["t60"]]
    return f"T60 {min(t60_l):.2f}-{max(t60_l):.2f} s" if t60_l else "T60 n/a"


def _glide_words(sv: dict) -> str:
    glide_v = max(sv["glide_cents"], key=abs) if sv["glide_cents"] else 0
    return f", glide {glide_v:+.0f} cents" if abs(glide_v) > 50 else ""


def save(model: dict, path: str) -> None:
    save_json(model, path)


def load(path: str) -> dict:
    mm = load_json(path)
    check(mm)
    return mm


def to_code(model: dict, name: str = "sound") -> str:
    """Standalone numpy + scipy source that renders the model (mono, identity knobs)."""
    import inspect
    import pprint
    lite = {ix: model[ix] for ix in ("kind", "version", "duration", "onset", "partials", "transients",
                                  "residual", "coherent", "layer") if ix in model}
    return (f'"""Sines model {name!r}: standalone render at the identity knobs."""\n\n'
            + inspect.getsource(K) + "\n\nMODEL = " + pprint.pformat(lite, width=110, compact=True)
            + "\n\n\ndef render(seed=0):\n    return sines_identity(MODEL, seed)\n\n\n"
            + _MAIN_16BIT.format(arg_path=1, arg_seed=2))


_MAIN_16BIT = '''def _write_wav16(path, y):
    import wave
    pcm = (np.clip(y, -1.0, 1.0) * 32767.0).astype("<i2")
    with wave.open(path, "wb") as fh:
        fh.setnchannels(1 if y.ndim == 1 else y.shape[1])
        fh.setsampwidth(2)
        fh.setframerate(SR)
        fh.writeframes(pcm.tobytes())


if __name__ == "__main__":
    import sys
    out = sys.argv[{arg_path}] if len(sys.argv) > {arg_path} else "sound.wav"
    seed = int(sys.argv[{arg_seed}]) if len(sys.argv) > {arg_seed} else 0
    _write_wav16(out, render(seed))
'''
