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
from hark.fit.analysis._common import (SR, as_stereo, check_kind, db, is_mono_pair, load_json,
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
    n = len(x)
    pad = 4096
    nfft = n + 2 * pad
    spec = np.fft.rfft(np.pad(x, pad), nfft)
    masks = band_masks(centres, nfft, sr)
    step = int(round(step_s * sr))
    idx = np.arange(0, n, step)
    out = []
    for b, c in enumerate(centres):
        y = np.fft.irfft(spec * masks[b], nfft)[pad:pad + n]
        bw = c * (2 ** (1 / 3) - 2 ** (-1 / 3))
        L = max(2, int(np.clip(4.0 / bw, 0.0005, max_smooth) * sr))
        out.append(10 * np.log10(np.maximum(_min_mean(y * y, L)[idx], 1e-24)))
    return idx / sr, np.array(out)


def _min_mean(p: np.ndarray, L: int) -> np.ndarray:
    c = np.concatenate([[0.0], np.cumsum(p)])
    n = len(p)
    i = np.arange(n)
    lo = np.maximum(i - L + 1, 0)
    trail = (c[i + 1] - c[lo]) / (i + 1 - lo)
    hi = np.minimum(i + L, n)
    lead = (c[hi] - c[i]) / (hi - i)
    return np.minimum(trail, lead)


# ================================================================ strikes

def onsets(x: np.ndarray, sr: int = SR) -> list[dict]:
    """Strikes ``[{'t', 'rise', 'strength'}]`` of a mono signal."""
    x = np.asarray(x, dtype=float)
    if x.ndim == 2:
        x = x.mean(axis=1)
    if len(x) < 64 or not np.any(x):
        return []
    _, env = band_env(x, STRIKE_BANDS, sr, 0.001, 0.008)
    func, total = _onset_function(env)
    cands = [(int(k), float(func[k])) for k in find_peaks(func, height=6.0, distance=25)[0]]
    first = int(np.argmax(total > total.max() - 25))
    cands.append((first, None))
    pe = _peak_env(x, sr)
    found = [s for s in (_time_strike(pe, k, v, sr) for k, v in sorted(cands)) if s]
    return _merge_strikes(found)


def _onset_function(env: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    top = env.max()
    env = np.maximum(env, top - 70)
    total = 10 * np.log10(np.sum(10 ** (env / 10), axis=0))
    padded = np.concatenate([np.repeat(env[:, :1], 4, axis=1), env], axis=1)
    mins = np.min(np.stack([padded[:, i:i + env.shape[1]] for i in range(5)]), axis=0)
    rise = np.maximum(env - mins, 0.0)
    loud = env > top - 60
    cnt = loud.sum(axis=0)
    func = np.where(cnt > 0, (rise * loud).sum(axis=0) / np.maximum(cnt, 1), 0.0)
    func[total < total.max() - 45] = 0.0
    func[(rise > 6).sum(axis=0) < 3] = 0.0
    return func, total


def _peak_env(x: np.ndarray, sr: int) -> np.ndarray:
    size = int(round(0.0025 * sr))
    m = maximum_filter1d(np.abs(x), size, origin=size // 2 - 1, mode="constant")
    return 20 * np.log10(np.maximum(m, 1e-12))


def _time_strike(pe: np.ndarray, k: int, value: float | None, sr: int) -> dict | None:
    c = int(k * sr / 1000)
    a = max(0, c - int(0.025 * sr))
    b = min(len(pe), c + int(0.030 * sr))
    p0 = max(a, c - int(0.008 * sr))
    if b <= p0 + 1:
        return None
    ip = p0 + int(np.argmax(pe[p0:b]))
    peak = pe[ip]
    imin = a + int(np.argmin(pe[a:ip + 1]))
    base = max(float(pe[imin]), peak - 70)
    if value is not None and peak - base < 3:
        return None
    out = _strike_rise(pe, imin, ip, base, sr)
    out["strength"] = float(value if value is not None else peak - base)
    return out


def _strike_rise(pe: np.ndarray, imin: int, ip: int, base: float, sr: int) -> dict:
    """Start time and rise time of the strike that peaks at ip."""
    peak = pe[ip]
    lev = max(base + 0.25 * (peak - base), peak - 30)
    start = imin + int(np.argmax(pe[imin:ip + 1] >= lev))
    rend = start + int(np.argmax(pe[start:ip + 1] >= peak - 1))
    return {"t": start / sr, "rise": max(0.0003, (rend - start) / sr)}


def _merge_strikes(found: list[dict]) -> list[dict]:
    out: list[dict] = []
    for s in sorted(found, key=lambda s: s["t"]):
        if out and s["t"] - out[-1]["t"] < 0.025:
            if s["strength"] > out[-1]["strength"]:
                out[-1] = s
            continue
        out.append(s)
    return out


# ================================================================ framing and peaks

def _stft_frames(x: np.ndarray, W: int) -> dict:
    hop = max(32, W // 8)
    nf = len(x) // hop + 2
    buf = np.concatenate([np.zeros(W // 2), x, np.zeros(W // 2 + 2 * hop)])
    win = get_window("blackman", W)
    idx = np.arange(nf)[:, None] * hop + np.arange(W)[None, :]
    fr = buf[idx] * win
    z = np.zeros((nf, 2 * W))
    z[:, :W // 2] = fr[:, W // 2:]
    z[:, -(W // 2):] = fr[:, :W // 2]
    X = np.fft.rfft(z, axis=1)
    return {"X": X, "hop": hop, "N": 2 * W, "wsum": float(np.sum(win)), "W": W}


def _frame_peaks(mag_db: np.ndarray, phase: np.ndarray, gref: float, fr: dict, opts: dict,
                 sr: int) -> np.ndarray:
    """Peaks of one frame: rows (freq, amp, phase, dB, contrast)."""
    b = _peak_bins(mag_db, gref, fr["N"], opts, sr)
    if len(b) == 0:
        return np.zeros((0, 5))
    return _interp_peaks(b, mag_db, phase, _block_floor(mag_db), fr, sr)


def _peak_bins(d: np.ndarray, gref: float, N: int, opts: dict, sr: int) -> np.ndarray:
    """Bins of the strongest prominent local maxima."""
    pw = 10 ** (d / 10)
    local = 10 * np.log10(np.maximum(uniform_filter1d(pw, 33, mode="nearest"), 1e-30))
    pk = np.zeros(len(d), bool)
    pk[1:-1] = (d[1:-1] > d[:-2]) & (d[1:-1] >= d[2:])
    pk &= (d > gref - opts["dyn_db"]) & (d > d.max() - opts["dyn_db"]) & (d > local + opts["prom_db"])
    pk[:int(np.ceil(30 * N / sr))] = False
    pk[-2:] = False
    b = np.nonzero(pk)[0]
    if len(b) > opts["max_peaks"]:
        b = np.sort(b[np.argsort(d[b])[-opts["max_peaks"]:]])
    return b


def _block_floor(d: np.ndarray) -> np.ndarray:
    nb = max(1, len(d) // 32)
    meds = np.array([np.median(d[i * 32:(i + 1) * 32]) for i in range(nb)])
    centres = np.arange(nb) * 32 + 15.5
    return np.interp(np.arange(len(d)), centres, meds)


def _interp_peaks(b, d, phase, floor, fr, sr):
    a, c, e = d[b - 1], d[b], d[b + 1]
    den = a - 2 * c + e
    p = np.where(np.abs(den) < 1e-12, 0.0, 0.5 * (a - e) / np.where(den == 0, 1, den))
    p = np.clip(p, -0.5, 0.5)
    freq = (b + p) * sr / fr["N"]
    amp = 10 ** ((c - 0.25 * (a - e) * p) / 20) * 2 / fr["wsum"]
    nb = np.where(p >= 0, b + 1, b - 1)
    step = np.angle(np.exp(1j * (phase[nb] - phase[b])))
    ph = phase[b] + np.abs(p) * step
    return np.column_stack([freq, amp, ph, c, c - floor[b]])


# ================================================================ tracking

def _track(peaks: list[np.ndarray], W: int, sr: int) -> list[dict]:
    min_tol = 1.5 * sr / W
    active: list[dict] = []
    done: list[dict] = []
    for k, P in enumerate(peaks):
        used = _assign(active, P, k, min_tol)
        still = []
        for tr in active:
            (still if k - tr["k"][-1] <= 2 else done).append(tr)
        active = still
        for j in range(len(P)):
            if j not in used:
                active.append({"k": [k], "rows": [P[j]], "slope": 0.0})
    done.extend(active)
    return [_fill(tr) for tr in done]


def _assign(active: list[dict], P: np.ndarray, k: int, min_tol: float) -> set:
    if not active or not len(P):
        return set()
    return _greedy_match(active, P, k, _match_cost(active, P, k, min_tol))


def _match_cost(active: list[dict], P: np.ndarray, k: int, min_tol: float) -> np.ndarray:
    """Track-to-peak cost: frequency error over tolerance plus a small level term. inf past tolerance."""
    pred = np.array([tr["rows"][-1][0] + tr["slope"] * (k - tr["k"][-1]) for tr in active])
    last_db = np.array([tr["rows"][-1][3] for tr in active])
    tol = np.maximum(min_tol, 0.025 * pred)
    df = np.abs(P[None, :, 0] - pred[:, None])
    cost = df / tol[:, None] + 0.01 * np.abs(P[None, :, 3] - last_db[:, None])
    cost[df > tol[:, None]] = np.inf
    return cost


def _greedy_match(active: list[dict], P: np.ndarray, k: int, cost: np.ndarray) -> set:
    used_t, used_p = set(), set()
    for flat in np.argsort(cost, axis=None):
        i, j = divmod(int(flat), len(P))
        if not np.isfinite(cost[i, j]):
            break
        if i not in used_t and j not in used_p:
            used_t.add(i)
            used_p.add(j)
            _extend(active[i], P[j], k)
    return used_p


def _extend(tr: dict, row: np.ndarray, k: int) -> None:
    gap = k - tr["k"][-1]
    new = (row[0] - tr["rows"][-1][0]) / gap
    if len(tr["k"]) >= 2:
        tr["slope"] = 0.5 * (tr["slope"] + new)
    elif gap > 1:
        tr["slope"] = new
    else:
        tr["slope"] = new
    tr["k"].append(k)
    tr["rows"].append(row)


def _fill(tr: dict) -> dict:
    ks = np.asarray(tr["k"])
    rows = np.asarray(tr["rows"])
    full = np.arange(ks[0], ks[-1] + 1)
    out = {"k": full, "t0": None}
    for col, name in ((0, "f"), (1, "a"), (4, "c")):
        out[name] = np.interp(full, ks, rows[:, col])
    ph = np.interp(full, ks, rows[:, 2])
    have = np.isin(full, ks)
    out["ph"] = ph
    out["_have"] = have
    out["ph_raw"] = dict(zip(ks.tolist(), rows[:, 2].tolist()))
    return out


def _fill_phase(tr: dict, hop: int, sr: int) -> None:
    ph = tr["ph"].copy()
    for i in range(1, len(ph)):
        k = int(tr["k"][i])
        if k in tr["ph_raw"]:
            ph[i] = tr["ph_raw"][k]
        else:
            ph[i] = ph[i - 1] + 2 * np.pi * 0.5 * (tr["f"][i - 1] + tr["f"][i]) * hop / sr
    tr["ph"] = ph


def _filter_tracks(tracks: list[dict], fr: dict, opts: dict, sr: int, room: int,
                   ref_peak: float | None = None) -> list[dict]:
    hop, W = fr["hop"], fr["W"]
    min_len = max(3, math.ceil(max(opts["min_ms"] / 1000, 1.25 * W / sr) * sr / hop))
    tracks = [t for t in tracks if len(t["k"]) >= min_len]
    keep = _loud_clear(tracks, opts, ref_peak) if tracks else []
    keep.sort(key=lambda t: -float(np.sum(t["a"] ** 2)))
    return keep[:room]


def _loud_clear(tracks: list[dict], opts: dict, ref_peak: float | None) -> list[dict]:
    """Tracks within keep_db of the top peak and with enough contrast over the floor."""
    peaks = np.array([20 * np.log10(max(t["a"].max(), 1e-30)) for t in tracks])
    top = peaks.max() if ref_peak is None else ref_peak
    return [t for t, p in zip(tracks, peaks) if p > top - opts["keep_db"] and _contrast(t) >= opts["snr_db"]]


def _contrast(t: dict) -> float:
    e = t["a"] ** 2
    return float(np.sum(e * t["c"]) / max(np.sum(e), 1e-30))


# ================================================================ onset fixes

def _fix_onsets(tracks: list[dict], strikes: list[dict], fr: dict, sr: int) -> list[dict]:
    hop = fr["hop"]
    half = fr["W"] / (2 * sr)
    tracks = _split_tracks(tracks, strikes, hop, half, sr)
    tracks = [t for t in tracks if not _pre_echo(t, strikes, hop, half, sr)]
    for t in tracks:
        _fix_head(t, strikes, hop, half, sr)
    return [t for t in tracks if len(t["k"])]


def _times(t: dict, hop: int, sr: int) -> np.ndarray:
    return t["k"] * hop / sr


def _slice(t: dict, sel: np.ndarray) -> dict:
    out = dict(t)
    for key in ("k", "f", "a", "c", "ph"):
        out[key] = t[key][sel]
    return out


def _split_tracks(tracks, strikes, hop, half, sr):
    out = []
    for t in tracks:
        parts = [t]
        for s in strikes:
            cut = _split_at(parts[-1], s["t"], hop, half, sr)
            if cut is not None:
                parts = parts[:-1] + cut
        out.extend(parts)
    return out


def _split_at(cur: dict, ts: float, hop: int, half: float, sr: int) -> list | None:
    """[head, tail] (head only when 3+ frames) when cur jumps over 10 dB at strike ts, else None."""
    tt = _times(cur, hop, sr)
    if not _jumps(cur, tt, ts, half):
        return None
    cut = tt < ts
    head, tail = _slice(cur, cut), _slice(cur, ~cut)
    return ([head] if len(head["k"]) >= 3 else []) + [tail]


def _jumps(cur: dict, tt: np.ndarray, ts: float, half: float) -> bool:
    span = tt[0] < ts - 0.5 * half and tt[-1] > ts + 0.55 * half
    after = (tt >= ts + 0.55 * half) & (tt < ts + 0.55 * half + 2 * half)
    before = (tt >= ts - 2 * half) & (tt < ts - 0.5 * half)
    ok = span and after.any() and before.any()
    return bool(ok and db(cur["a"][after].max() ** 2) - db(cur["a"][before].max() ** 2) > 10)


def _pre_echo(t, strikes, hop, half, sr) -> bool:
    tt = _times(t, hop, sr)
    e = t["a"] ** 2
    tot = max(float(e.sum()), 1e-30)
    for s in strikes:
        lo, hi = s["t"] - 1.1 * half, s["t"] + 0.55 * half
        inside = (tt >= lo) & (tt <= hi)
        if e[inside].sum() >= 0.9 * tot and lo <= tt[-1] <= hi:
            return True
    return False


def _fix_head(t, strikes, hop, half, sr) -> None:
    tt = _times(t, hop, sr)
    born = [s for s in strikes if s["t"] - 0.25 * half <= tt[0] <= s["t"] + 1.05 * half]
    if born:
        _rebuild_head(t, tt, max(born, key=lambda s: s["strength"]), hop, half, sr)


def _rebuild_head(t: dict, tt: np.ndarray, s: dict, hop: int, half: float, sr: int) -> None:
    """Start the track at strike s, with its smeared head replaced by the clean decay line."""
    ts = s["t"]
    ad = 20 * np.log10(np.maximum(t["a"], 1e-30))
    a0 = _unsmear(ad, tt, ts, hop, half, sr)
    t["a"] = 10 ** (ad / 20)
    after = _after(tt, ts)
    for key in ("k", "f", "a", "c", "ph"):
        t[key] = t[key][after]
    t["t0"], t["rise"], t["a0"] = ts, s["rise"], 10 ** (a0 / 20)


def _unsmear(ad: np.ndarray, tt: np.ndarray, ts: float, hop: int, half: float, sr: int) -> float:
    """Rewrite the head of ad (dB, in place) from the clean frames. Returns the onset level (dB)."""
    mx = ad.max()
    clean0 = ts + 0.55 * half
    sel = (tt >= clean0) & (tt <= clean0 + max(4 * hop / sr, half))
    smear = tt < clean0
    if sel.sum() >= 2:
        slope, icpt = np.polyfit(tt[sel] - ts, ad[sel], 1)
        a0 = float(np.clip(icpt, mx - 40, mx + 6))
        ad[smear] = np.minimum(icpt + slope * (tt[smear] - ts), mx + 6)
    elif sel.sum() == 1:
        a0 = float(ad[sel][0])
        ad[smear] = a0
    else:
        a0 = float(mx)
        ad[:] = mx
    return a0


def _after(tt: np.ndarray, ts: float) -> np.ndarray:
    after = tt > ts
    if not after.any():
        after = np.zeros(len(tt), bool)
        after[-1] = True
    return after


# ================================================================ phase-exact synthesis

def _synth_exact(tracks: list[dict], n: int, hop: int, sr: int) -> np.ndarray:
    y = np.zeros(n)
    for t in tracks:
        _add_exact(y, t, hop, sr)
    return y


def _add_exact(y, t, hop, sr):
    n = len(y)
    k, f, a, ph = t["k"], t["f"], t["a"], t["ph"]
    w = 2 * np.pi * f / sr
    s0 = int(k[0] * hop)
    for i in range(len(k) - 1):
        seg = _cubic(ph[i], ph[i + 1], w[i], w[i + 1], a[i], a[i + 1], hop)
        _put(y, int(k[i] * hop), seg)
    _head(y, t, s0, w[0], a[0], ph[0], hop, sr)
    m = np.arange(hop)
    tail = a[-1] * (1 - m / hop) * np.cos(ph[-1] + w[-1] * m)
    _put(y, int(k[-1] * hop), tail)
    del n


def _cubic(th0, th1, w0, w1, a0, a1, H):
    M = round(((th0 + w0 * H - th1) + (w1 - w0) * H / 2) / (2 * np.pi))
    e = th1 - th0 - w0 * H + 2 * np.pi * M
    alpha = 3 * e / H ** 2 - (w1 - w0) / H
    beta = -2 * e / H ** 3 + (w1 - w0) / H ** 2
    m = np.arange(H)
    theta = th0 + w0 * m + alpha * m ** 2 + beta * m ** 3
    return (a0 + (a1 - a0) * m / H) * np.cos(theta)


def _head(y, t, s0, w0, a_first, ph0, hop, sr):
    if t.get("t0") is not None:
        i0 = int(round(t["t0"] * sr))
        if i0 >= s0:
            return
        m = np.arange(i0, s0)
        r_end = min(s0, i0 + max(1, int(round(t["rise"] * sr))))
        amp = np.interp(m, [i0, r_end, s0], [0.0, t["a0"], a_first])
    else:
        m = np.arange(s0 - hop, s0)
        amp = a_first * (m - (s0 - hop)) / hop
    _put(y, int(m[0]), amp * np.cos(ph0 + w0 * (m - s0)))


def _put(y, start, seg):
    a = max(start, 0)
    b = min(len(y), start + len(seg))
    if b > a:
        y[a:b] += seg[a - start:b - start]


# ================================================================ atoms

def _pencil(seg: np.ndarray, max_atoms: int, max_t60: float, ridge: float, sr: float) -> list:
    z = _pencil_poles(seg, max_atoms) if len(seg) >= 16 and np.any(seg) else None
    return [] if z is None else _atom_ls(seg, _pole_limits(z, max_t60, sr), ridge, sr)[:max_atoms]


def _pencil_poles(seg: np.ndarray, max_atoms: int):
    """Matrix-pencil poles of seg, or None when the SVD fails or no order is left."""
    P = len(seg) // 3
    H = np.lib.stride_tricks.sliding_window_view(seg, P + 1)
    try:
        _, s, vh = np.linalg.svd(H, full_matrices=False)
    except np.linalg.LinAlgError:
        return None
    e = np.cumsum(s ** 2) / np.sum(s ** 2)
    order = min(int(np.searchsorted(e, 0.998)) + 2, 2 * max_atoms, len(s) - 1)
    if order < 1:
        return None
    V = vh[:order].T
    return np.linalg.eigvals(np.linalg.pinv(V[:-1]) @ V[1:])


def _pole_limits(z, max_t60, sr):
    z = z[np.angle(z) >= 0]
    rmax = 10 ** (-3 / (max_t60 * sr))
    if max_t60 > 0.05:
        z = z[np.abs(z) <= rmax]
    else:
        big = np.abs(z) > rmax
        z = np.where(big, z / np.maximum(np.abs(z), 1e-30) * rmax, z)
    return _distinct_poles(z[np.abs(z) >= 0.001])


def _distinct_poles(z) -> np.ndarray:
    out = []
    for p in z:
        if not any(abs(np.angle(p) - np.angle(q)) < 1e-4 and abs(abs(p) - abs(q)) < 1e-3 for q in out):
            out.append(p)
    return np.asarray(out)


def _atom_ls(seg, z, ridge, sr):
    if not len(z):
        return []
    B, owner = _damped_basis(z, len(seg))
    lam = ridge * np.trace(B.T @ B) / B.shape[1]
    coef = np.linalg.solve(B.T @ B + lam * np.eye(B.shape[1]), B.T @ seg)
    return _atoms_of(z, owner, coef, sr)


def _damped_basis(z, L: int):
    """Cosine (and sine, when not at 0 Hz) columns per pole, with (pole, part) owners."""
    k = np.arange(L)
    cols, owner = [], []
    for i, p in enumerate(z):
        r, w = abs(p), np.angle(p)
        cols.append(r ** k * np.cos(w * k))
        owner.append((i, 0))
        if w > 1e-9:
            cols.append(r ** k * np.sin(w * k))
            owner.append((i, 1))
    return np.array(cols).T, owner


def _atoms_of(z, owner, coef, sr) -> list:
    """Atoms [Hz, tau s, amp, phase], strongest first."""
    cs = np.zeros((len(z), 2))
    for (i, j), v in zip(owner, coef):
        cs[i, j] = v
    atoms = []
    for p, (c, s) in zip(z, cs):
        r, w = abs(p), np.angle(p)
        amp, ph = math.hypot(c, s), math.atan2(-s, c)
        atoms.append([w * sr / (2 * np.pi), -1 / (sr * np.log(r)), amp, ph])
    atoms.sort(key=lambda a: -(a[2] ** 2 * a[1]))
    return atoms


def _strike_atoms(res: np.ndarray, strikes: list[dict], sr: int) -> list[dict]:
    sos = butter(4, 400, "low", fs=sr, output="sos")
    lo = sosfiltfilt(sos, res)
    hi = res - lo
    sig_peak = max(float(np.max(np.abs(res))), 1e-30) if len(res) else 1e-30
    out = []
    for s in sorted(strikes, key=lambda s: -s["strength"])[:16]:
        e = _one_strike(hi, lo, s, sig_peak, sr)
        if e is not None:
            out.append(e)
    return sorted(out, key=lambda e: e["t"])


def _one_strike(hi, lo, s, sig_peak, sr) -> dict | None:
    a0 = max(0, int(round((s["t"] - 0.0005) * sr)))
    atoms = (_band_atoms(hi, a0, int(0.008 * sr), 1, 24, 0.02, 1e-4, (240, sr), sig_peak, sr)
             + _band_atoms(lo, a0, int(0.064 * sr), 8, 6, 0.096, 1e-3, (40, 600), sig_peak, sr))
    if not atoms:
        return None
    return {"t": round(a0 / sr, 6), "atoms": [[round(f, 2), round(t, 7), round(a, 7), round(p, 4)]
                                              for f, t, a, p in atoms]}


def _band_atoms(band, a0, length, dec, max_atoms, t60, ridge, frange, sig_peak, sr):
    seg = band[a0:a0 + length]
    loud = len(seg) >= 32 and np.max(np.abs(seg)) >= sig_peak * 10 ** (-40 / 20)
    atoms = []
    if loud:
        atoms = [a for a in _pencil(seg[::dec], max_atoms, t60, ridge, sr / dec) if frange[0] <= a[0] <= frange[1]]
    return atoms if atoms and _atoms_help(band, a0, length, atoms, sr) else []


def _atoms_help(band, a0, length, atoms, sr) -> bool:
    """True when the atoms remove over 30 % of the band energy after a0."""
    span = min(len(band) - a0, 2 * length + int(0.02 * sr))
    ref = band[a0:a0 + span]
    syn = synth_atoms([{"t": 0.0, "atoms": atoms}], span, sr)
    return bool(np.sum((ref - syn) ** 2) < 0.7 * np.sum(ref ** 2))


# ================================================================ residual model

_BIAS: dict[tuple, np.ndarray] = {}


def _bias(bands: tuple) -> np.ndarray:
    if bands not in _BIAS:
        w = np.random.default_rng(12345).standard_normal(2 * SR)
        _, env = band_env(w, bands, SR)
        env = median_filter(env, size=(1, 5), mode="nearest")
        nfft = len(w) + 2 * 4096
        true = 10 * np.log10(np.mean(band_masks(bands, nfft) ** 2, axis=1))
        T = env.shape[1]
        mid = env[:, int(0.1 * T):int(0.9 * T)].mean(axis=1)
        _BIAS[bands] = true - mid
    return _BIAS[bands]


def _noise_share(res: np.ndarray, bands, times: np.ndarray, strikes, sr: int) -> np.ndarray:
    _, _, Z = stft(res, nperseg=2048, noverlap=2048 - 240, boundary="zeros")
    P = np.abs(Z) ** 2
    share = median_filter(_band_flatness(P, bands, sr), size=(1, 5), mode="nearest")
    ft = np.arange(P.shape[1]) * 240 / sr
    out = np.array([np.interp(times, ft, s) for s in share])
    for s in strikes:
        out[:, (times >= s["t"] - 0.002) & (times <= s["t"] + 0.03)] = 0.0
    return out


def _band_flatness(P: np.ndarray, bands, sr: int) -> np.ndarray:
    """Noise share (dB) per band and frame from the median-to-mean ratio of whitened power."""
    Pw = P / np.maximum(median_filter(P, size=(31, 1), mode="nearest"), 1e-30)
    share = np.zeros((len(bands), P.shape[1]))
    for b, sel in enumerate(_band_bins(bands, sr)):
        if sel.sum() >= 5:
            q = np.median(Pw[sel], axis=0) / np.log(2) / np.maximum(Pw[sel].mean(axis=0), 1e-30)
            share[b] = 10 * np.log10(np.clip(q, 1e-4, 1))
    return share


def _band_bins(bands, sr: int) -> list[np.ndarray]:
    """Bin masks of a 2048-point spectrum, split halfway between band centres in log frequency."""
    lf = np.log2(np.maximum(np.fft.rfftfreq(2048, 1 / sr), 1e-9))
    lb = np.log2(np.asarray(bands))
    edges = np.concatenate([[-np.inf], (lb[:-1] + lb[1:]) / 2, [np.inf]])
    return [(lf >= edges[b]) & (lf < edges[b + 1]) for b in range(len(bands))]


def _residual_model(res: np.ndarray, strikes, peak_db: float, opts: dict, sr: int) -> dict:
    times, env = band_env(res, RES_BANDS, sr)
    env = env + _noise_share(res, RES_BANDS, times, strikes, sr)
    env = median_filter(env, size=(1, 5), mode="nearest") + _bias(RES_BANDS)[:, None]
    floor = peak_db - opts["res_range_db"]
    env = np.maximum(env, floor)
    live = env.max(axis=1) > floor + 1
    keep = _res_keys(times, env, live)
    lev = np.round(env[:, keep], 1)
    lev[np.abs(lev - floor) <= 0.05] = -150.0
    lev[~live] = -150.0
    return {"bands": [round(b, 2) for b in RES_BANDS], "times": [round(float(times[i]), 5) for i in keep],
            "db": lev.tolist(), "hf": hf_rolloff(res, sr)}


def _res_keys(times: np.ndarray, env: np.ndarray, live: np.ndarray) -> list[int]:
    if live.any() and len(times) > 2:
        return rdp_rows(times, env[live], 2.0, 96)
    return [0, len(times) - 1] if len(times) > 1 else [0]


def hf_rolloff(x: np.ndarray, sr: int = 48000, points=HF_POINTS) -> list:
    """Roll-off of x above 16 kHz: [[Hz, dB re 16 kHz], ...], monotone falling."""
    x = np.asarray(x, dtype=float)
    seg = min(4096, 2 ** int(np.floor(np.log2(max(len(x), 256)))))
    f, p = welch(np.pad(x, (0, max(0, seg - len(x)))), fs=sr, nperseg=seg)
    vals = _point_levels(f, p, points, sr)
    rel = np.minimum.accumulate(np.minimum(np.asarray(vals) - vals[0], 0.0))
    rel = np.maximum(rel, -80.0)
    return [[float(c), round(float(v), 1)] for c, v in zip(points, rel)]


def _point_levels(f: np.ndarray, p: np.ndarray, points, sr: int) -> list[float]:
    vals = []
    for c in points:
        sel = (f >= c * 2 ** (-1 / 12)) & (f <= min(c * 2 ** (1 / 12), sr / 2))
        vals.append(10 * np.log10(max(p[sel].mean(), 1e-300)) if sel.any() else -300.0)
    return vals


# ================================================================ breakpoints

def _breakpoints(t: dict, hop: int, sr: int, opts: dict) -> dict:
    tt = t["k"] * hop / sr
    ad = 20 * np.log10(np.maximum(t["a"], 1e-30))
    f = t["f"] if len(t["f"]) < 5 else median_filter(t["f"], 5, mode="nearest")
    t0, rise, a0, ph = _start_point(t, tt, ad, f, hop, sr)
    at, av, fv = np.concatenate([[t0], tt]), np.concatenate([[a0], ad]), np.concatenate([[f[0]], f])
    return _breakpoint_dict(t0, rise, tt[-1], hop / sr, ph, (at, av, fv), opts)


def _breakpoint_dict(t0, rise, t1, rel, ph, knots, opts) -> dict:
    at, av, fv = knots
    ia = rdp(at, av, opts["amp_tol_db"], 64)
    iff = _freq_keys(at, av, fv, opts)
    return {"t0": round(float(t0), 5), "rise": round(float(rise), 5), "t1": round(float(t1), 5),
            "rel": round(rel, 5), "ph": round(float(np.angle(np.exp(1j * ph))), 3),
            "freq": [[round(float(at[i]), 5), round(float(fv[i]), 2)] for i in iff],
            "amp": [[round(float(at[i]), 5), round(float(av[i]), 1)] for i in ia]}


def _start_point(t: dict, tt, ad, f, hop: int, sr: int):
    """(t0, rise, level dB, phase) at the start: the fixed strike onset, or one hop before the track."""
    if t.get("t0") is not None:
        t0, rise = t["t0"], max(t["rise"], 0.0002)
        return t0, rise, 20 * np.log10(max(t["a0"], 1e-30)), t["ph"][0] - 2 * np.pi * f[0] * (tt[0] - t0)
    return tt[0] - hop / sr, max(hop / sr, 0.0002), ad[0], t["ph"][0] - 2 * np.pi * f[0] * hop / sr


def _freq_keys(at, av, fv, opts) -> list[int]:
    cents = 1200 * np.log2(np.maximum(fv, 1))
    ftol = opts["freq_tol_cents"] * 2 ** np.clip((av.max() - av) / 20, 0, 4)
    return rdp(at, cents, ftol, 48)


def _peak_of(p: dict) -> float:
    return max(a[1] for a in p["amp"])


# ================================================================ analysis per window

def _default_opts(**kw) -> dict:
    opts = {"max_partials": 160, "dyn_db": 80, "keep_db": 60, "prom_db": 4, "max_peaks": 100,
            "min_ms": 15, "amp_tol_db": 1, "freq_tol_cents": 1, "res_range_db": 140,
            "transients": True, "passes": 2, "snr_db": 10}
    opts.update({k: v for k, v in kw.items() if k in opts})
    return opts


def _tracks_of(x: np.ndarray, W: int, strikes, opts: dict, sr: int, room: int,
               gref: float | None = None, ref_peak: float | None = None) -> tuple[list, dict]:
    fr = _stft_frames(x, W)
    mag = 20 * np.log10(np.maximum(np.abs(fr["X"]), 1e-30))
    phase = np.angle(fr["X"])
    g = float(mag.max()) if gref is None else gref
    peaks = [_frame_peaks(mag[k], phase[k], g, fr, opts, sr) for k in range(len(mag))]
    tracks = _track(peaks, W, sr)
    for t in tracks:
        _fill_phase(t, fr["hop"], sr)
    tracks = _filter_tracks(tracks, fr, opts, sr, room, ref_peak)
    return _fix_onsets(tracks, strikes, fr, sr), fr | {"gref": g}


def analyze(x: np.ndarray, sr: int, W: int, onsets: list[dict], width: float, opts: dict) -> dict:
    """One window size: {'model', 'det', 'res', 'atoms', 'tracks'}."""
    opts = _default_opts(**(opts or {}))
    x = np.asarray(x, dtype=float)
    n = len(x)
    tracks, fr = _tracks_of(x, W, onsets, opts, sr, opts["max_partials"])
    det = _synth_exact(tracks, n, fr["hop"], sr)
    if opts["passes"] >= 2 and opts["max_partials"] - len(tracks) > 0:
        tracks, det = _second_pass(x, det, tracks, W, onsets, opts, sr, fr)
    res = x - det
    trans = _strike_atoms(res, onsets, sr) if opts["transients"] and onsets else []
    atoms = synth_atoms(trans, n, sr)
    model = _sines_model(x, sr, onsets, width, tracks, fr["hop"], trans, res - atoms, opts)
    model["analysis"] = _analysis_info(x, det, res - atoms, W, fr["hop"])
    return {"model": model, "det": det, "res": res, "atoms": atoms, "tracks": tracks}


def _analysis_info(x, det, rest, W: int, hop: int) -> dict:
    ex = max(float(np.sum(x ** 2)), 1e-30)
    return {"window": W, "hop": hop, "sines_share_db": round(float(db(np.sum(det ** 2) / ex)), 2),
            "residual_db": round(float(db(np.sum(rest ** 2) / ex)), 2)}


def _sines_model(x, sr, onsets, width, tracks, hop, trans, rest, opts) -> dict:
    n = len(x)
    peak_db = float(20 * np.log10(max(np.max(np.abs(x)), 1e-30))) if n else -300.0
    partials = sorted((_breakpoints(t, hop, sr, opts) for t in tracks), key=lambda p: -_peak_of(p))
    return {"kind": KIND, "version": VERSION, "sr": SR, "duration": round(n / sr, 5),
            "onset": round(onsets[0]["t"], 5) if onsets else 0.0,
            "onsets": [round(s["t"], 5) for s in onsets], "peak_db": round(peak_db, 2),
            "width": round(float(width), 1), "partials": partials, "transients": trans,
            "residual": _residual_model(rest, onsets, peak_db, opts, sr), "coherent": True}


def _second_pass(x, det, tracks, W, strikes, opts, sr, fr):
    room = opts["max_partials"] - len(tracks)
    res = x - det
    ref_peak = max((20 * np.log10(max(t["a"].max(), 1e-30)) for t in tracks), default=None)
    more, _ = _tracks_of(res, W, strikes, opts, sr, room, fr["gref"], ref_peak)
    if not more:
        return tracks, det
    add = _synth_exact(more, len(x), fr["hop"], sr)
    if np.sum((res - add) ** 2) < 0.98 * np.sum(res ** 2):
        return tracks + more, det + add
    return tracks, det


# ================================================================ fit

def _lsd(a: np.ndarray, b: np.ndarray) -> float:
    n = max(len(a), len(b))
    a, b = np.pad(a, (0, n - len(a))), np.pad(b, (0, n - len(b)))
    out = [v for v in (_lsd_at(a, b, size) for size in (256, 1024, 4096) if n >= size) if v is not None]
    return float(np.mean(out)) if out else 0.0


def _lsd_at(a: np.ndarray, b: np.ndarray, size: int) -> float | None:
    A = 20 * np.log10(np.maximum(np.abs(stft(a, nperseg=size, noverlap=size - size // 4)[2]), 1e-12))
    B = 20 * np.log10(np.maximum(np.abs(stft(b, nperseg=size, noverlap=size - size // 4)[2]), 1e-12))
    fl = max(A.max(), B.max()) - 60
    act = (A > fl) | (B > fl)
    if not act.any():
        return None
    return float(np.mean(np.minimum(np.abs(np.maximum(A, fl) - np.maximum(B, fl)), 40)[act]))


def fit(x: Any, sr: int = 48000, windows=WINDOWS, max_partials: int = 160, dyn_db: float = 80,
        keep_db: float = 60, prom_db: float = 4, max_peaks: int = 100, min_ms: float = 15,
        amp_tol_db: float = 1, freq_tol_cents: float = 1, res_range_db: float = 140,
        transients: bool = True, passes: int = 2, snr_db: float = 10,
        source: str | None = None) -> dict:
    """A sines model of a path or an array at ``sr`` (mono or (n, 2))."""
    name = source or source_name(x)
    a = as_stereo(x) if isinstance(x, str) else as_stereo(resample(np.asarray(x, float), sr))
    opts = _default_opts(max_partials=max_partials, dyn_db=dyn_db, keep_db=keep_db, prom_db=prom_db,
                         max_peaks=max_peaks, min_ms=min_ms, amp_tol_db=amp_tol_db,
                         freq_tol_cents=freq_tol_cents, res_range_db=res_range_db,
                         transients=transients, passes=passes, snr_db=snr_db)
    model = _fit_sines(a, windows, opts)
    model["source"] = name
    model["summary"] = summary(model)
    return model


def _fit_sines(a: np.ndarray, windows, opts: dict) -> dict:
    stereo = not is_mono_pair(a)
    mono = a.mean(axis=1)
    width = _width_db(a) if stereo else -120.0
    strikes = onsets(mono, SR)
    d, model, tried = _best_window(mono, windows, strikes, width, opts)
    if stereo and width > -60:
        model["side"] = side_model(a, model["duration"])
    model["channels"] = 2 if stereo else 1
    model["analysis"].update({"lsd_db": round(d, 3), "tried": tried,
                              "onset_rise_ms": round(strikes[0]["rise"] * 1000, 3) if strikes else None})
    return model


def _best_window(mono: np.ndarray, windows, strikes, width: float, opts: dict):
    """(LSD, model, tried) of the window size whose render is closest to mono."""
    best, tried = None, {}
    for W in windows:
        if best is None or W <= 2 * len(mono):
            m = analyze(mono, SR, W, strikes, width, opts)["model"]
            d = _lsd(render(m, 0), mono)
            tried[str(W)] = round(d, 3)
            if best is None or d < best[0]:
                best = (d, m)
    return best[0], best[1], tried


def _width_db(a: np.ndarray) -> float:
    s = (a[:, 0] - a[:, 1]) / 2
    m = (a[:, 0] + a[:, 1]) / 2
    em = float(np.sum(m ** 2))
    if em <= 0:
        return -120.0
    return max(-120.0, float(10 * np.log10(max(np.sum(s ** 2), 1e-300) / em)))


# ================================================================ side model

def _key_times(onset: float, dur: float) -> np.ndarray:
    ts = [0.0, onset - 0.01, onset]
    t = onset
    for step, until in ((0.01, 0.1), (0.02, 0.3), (0.04, 0.7)):
        while t + step <= onset + until + 1e-9:
            t += step
            ts.append(t)
    while t + 0.08 < dur:
        t += 0.08
        ts.append(t)
    ts.append(dur)
    return np.unique(np.round(np.clip(ts, 0.0, dur), 6))


def side_model(st: np.ndarray, duration: float | None = None, bands=SIDE_BANDS,
               tol: float = 0.04) -> dict:
    """The fitted side of a stereo source: pan and diffuse gains per band and keyframe."""
    a = as_stereo(st)
    dur = len(a) / SR if duration is None else float(duration)
    P = space.side_powers(a, bands)
    onset = space.active_span(a)[0] / SR
    keys = _key_times(onset, dur)
    PM, PS, C = (_tent_sum(P["t"], P[k], keys) for k in ("PM", "PS", "C"))
    pan, dif = _gains(PM, PS, C)
    pan, dif = _quiet_fix(PM, PS, C, pan, dif)
    keep = rdp_rows(keys, np.vstack([pan, dif]), tol) if len(keys) > 2 else list(range(len(keys)))
    return {"bands": [float(b) for b in bands], "times": [round(float(keys[i]), 5) for i in keep],
            "pan": np.round(pan[:, keep], 3).tolist(), "diffuse": np.round(dif[:, keep], 3).tolist()}


def _tent_sum(ft: np.ndarray, Pb: np.ndarray, keys: np.ndarray) -> np.ndarray:
    out = np.zeros((Pb.shape[0], len(keys)))
    for i, k in enumerate(keys):
        lo = keys[i - 1] if i > 0 else k - 0.01
        hi = keys[i + 1] if i + 1 < len(keys) else k + 0.01
        w = np.where(ft <= k, (ft - lo) / max(k - lo, 1e-9), (hi - ft) / max(hi - k, 1e-9))
        w = np.clip(w, 0.0, 1.0)
        if not w.any():
            w = np.zeros(len(ft))
            w[int(np.argmin(np.abs(ft - k)))] = 1.0
        out[:, i] = Pb @ w
    return out


def _gains(PM, PS, C):
    pm = np.maximum(PM, 1e-30)
    pan = np.clip(C / pm, -1.5, 1.5)
    dif = np.clip(np.sqrt(np.maximum(PS - C ** 2 / pm, 0) / pm), 0, 2)
    return pan, dif


def _quiet_fix(PM, PS, C, pan, dif):
    whole_p, whole_d = _gains(PM.sum(axis=1, keepdims=True), PS.sum(axis=1, keepdims=True),
                              C.sum(axis=1, keepdims=True))
    all_p, all_d = _gains(PM.sum(), PS.sum(), C.sum())
    tot = PM.sum(axis=1)
    for b in range(PM.shape[0]):
        if tot[b] < 1e-6 * max(tot.max(), 1e-300):
            pan[b], dif[b] = all_p, all_d
            continue
        quiet = db(PM[b]) < db(PM[b].max()) - 50
        pan[b, quiet], dif[b, quiet] = whole_p[b, 0], whole_d[b, 0]
    return pan, dif


def morph_side(sa: dict | None, sb: dict | None, w: float, map_a: Callable | None = None,
               map_b: Callable | None = None) -> dict | None:
    """The side of a morph: both sides on the union of mapped times, mixed linearly."""
    if sa and sb and list(sa["bands"]) != list(sb["bands"]):
        raise ValueError("side bands differ between the two models")
    return _morph(sa, sb, w, map_a, map_b) if (sa or sb) else None


def _morph(sa, sb, w, map_a, map_b) -> dict:
    ref = sa or sb
    nb = len(ref["bands"])
    ta, tb = _mapped(sa, map_a), _mapped(sb, map_b)
    grid = np.unique(np.round(np.concatenate([ta, tb]), 6))
    out = {"bands": list(ref["bands"]), "times": grid.tolist()}
    for key in ("pan", "diffuse"):
        ga = _on_grid(sa, key, ta, grid, nb)
        gb = _on_grid(sb, key, tb, grid, nb)
        out[key] = np.round((1 - w) * ga + w * gb, 3).tolist()
    return out


def _mapped(s, fn):
    if not s:
        return np.zeros(0)
    t = np.asarray(s["times"], dtype=float)
    return np.asarray([fn(v) for v in t]) if fn else t


def _on_grid(s, key, t, grid, nb):
    if not s:
        return np.zeros((nb, len(grid)))
    return np.array([np.interp(grid, t, row) for row in s[key]])


def vary_field(r: np.random.Generator, parts: int = 6) -> Callable[[Any], np.ndarray]:
    """A smooth random function of frequency with unit variance."""
    c = r.uniform(0.25, 1.25, parts)
    p = r.uniform(0.0, 2 * np.pi, parts)
    scale = np.sqrt(2.0 / parts)

    def field(f):
        lf = np.log2(np.maximum(np.asarray(f, dtype=float), 20.0))
        return scale * np.sum(np.cos(2 * np.pi * np.multiply.outer(lf, c) + p), axis=-1)

    return field


# ================================================================ render

def check(model: Any) -> None:
    check_kind(model, KIND, VERSION)


def fade_in_s(model: dict) -> float:
    return K.sines_fade_in(model)


def partial_freq(p: dict) -> float:
    """Energy-weighted mean frequency of a partial."""
    fr = np.asarray(p["freq"], dtype=float)
    am = np.asarray(p["amp"], dtype=float)
    t = np.linspace(p["t0"], max(p["t1"], p["t0"] + 1e-6), 64)
    f = 2 ** np.interp(t, fr[:, 0], np.log2(np.maximum(fr[:, 1], 1e-6)))
    e = 10 ** (np.interp(t, am[:, 0], am[:, 1]) / 10)
    return float(np.sum(f * e) / max(np.sum(e), 1e-300))


def _knobs(knobs: dict | None, kw: dict) -> dict:
    k = {"transpose": 0.0, "stretch": 1.0, "decay": 1.0, "brightness": 0.0, "vibrato": 0.0,
         "vibrato_rate": 5.5, "drift": 0.0, "residual": 1.0, "transient": 1.0, "vary": 0.0,
         "partials": None, "inharm": 0.0, "phases": "fit", "gain_db": 0.0, "stereo": False,
         "width": 1.0, "ring": None}
    given = dict(knobs or {}, **kw)
    bad = sorted(set(given) - set(k))
    if bad:
        raise ValueError(f"unknown sines knob(s) {bad}; known: {sorted(k)}")
    k.update(given)
    if k["ring"] is None:
        k["ring"] = k["vary"] > 0
    return k


def _is_identity(k: dict) -> bool:
    return (k["stretch"] == 1 and k["decay"] == 1 and k["transpose"] == 0 and k["vary"] == 0
            and k["brightness"] == 0 and k["inharm"] == 0 and k["phases"] == "fit")


def render(model: dict, seed: int = 0, knobs: dict | None = None, **kw) -> np.ndarray:
    """float64 at 48 kHz: mono, or (n, 2) with ``stereo=True``."""
    check(model)
    k = _knobs(knobs, kw)
    ident = _is_identity(k)
    rj = np.random.default_rng((int(seed) * 7919 + 17) & 0xFFFFFFFF)
    ctx = _render_ctx(model, k, rj)
    parts, times, levels, n = _moved(model, k, ctx, rj, ident)
    cents = _bend(n, k, ctx, rj)
    y = synth_partials(parts, n, SR, cents)
    y = y + synth_atoms(_moved_atoms(model, k, ctx, rj, ident), n, SR)
    y = y + _render_noise(model, times, levels, n, seed, k, ctx)
    y = y * 10 ** (k["gain_db"] / 20)
    if k["stereo"]:
        y = _stereo(model, y, seed, k, ctx)
    return K.sines_finish(model, y)


def _render_ctx(model: dict, k: dict, rj) -> dict:
    summ = model.get("summary") or {}
    ctx = {"ratio": 2 ** (k["transpose"] / 12), "f_ref": summ.get("centroid") or 1000.0,
           "f_low": summ.get("f_low") or 100.0, "on": float(model.get("onset", 0.0)),
           "fields": None}
    if model.get("coherent") and k["vary"] > 0:
        ctx["fields"] = [vary_field(rj) for _ in range(3)]
    return ctx


def _moved(model: dict, k: dict, ctx: dict, rj, ident: bool):
    """Edited partials, residual times and levels, and the take length in samples."""
    parts = sorted(model["partials"], key=lambda p: -_peak_of(p))
    if k["partials"] is not None:
        parts = parts[:int(k["partials"])]
    if not ident:
        parts = [_move_partial(p, k, ctx, rj) for p in parts]
    res = model["residual"]
    times, levels = res["times"], res["db"]
    if not ident or k["residual"] != 1:
        times, levels = _move_residual(res, k, ctx, rj)
    n = _length(model, parts, k, ctx)
    if k["ring"]:
        n, times, levels = _ring(model, n, times, levels, k, ctx)
    return parts, times, levels, n


def _moved_atoms(model: dict, k: dict, ctx: dict, rj, ident: bool) -> list:
    trans = model.get("transients", [])
    return _move_atoms(trans, k, ctx, rj) if (not ident or k["transient"] != 1) else trans


def _render_noise(model: dict, times, levels, n: int, seed: int, k: dict, ctx: dict) -> np.ndarray:
    res = model["residual"]
    noise = synth_noise(res["bands"], times, levels, n, seed, SR, hf=res.get("hf"))
    if k["transient"] != 1:
        noise = noise * (1 + (k["transient"] - 1) * _strike_weight(model, n, ctx, k))
    return noise


def _T(t, k, ctx):
    return ctx["on"] + (np.asarray(t, dtype=float) - ctx["on"]) * k["stretch"]


def _move_partial(p: dict, k: dict, ctx: dict, rj) -> dict:
    p = copy.deepcopy(p)
    fmed = partial_freq(p)
    det, lev, dj = _partial_offsets(fmed, k["vary"], ctx, rj)
    if k["phases"] == "random":
        p["ph"] = float(rj.uniform(-np.pi, np.pi))
    mul = ctx["ratio"] * (fmed / ctx["f_low"]) ** k["inharm"] * 2 ** (float(det) / 1200)
    p["freq"] = [[t, f * mul] for t, f in p["freq"]]
    bright = k["brightness"] * np.log2(max(fmed * ctx["ratio"], 1) / ctx["f_ref"])
    _decay_amp(p, [[t, a + float(lev) + bright] for t, a in p["amp"]], k["decay"] * float(dj))
    _stretch_partial(p, k, ctx)
    return p


def _partial_offsets(fmed: float, v: float, ctx: dict, rj):
    """Detune (cents), level (dB) and decay factor of one partial, from the fields or the generator."""
    fields = ctx["fields"]
    if fields:
        return 8 * v * fields[0](fmed), 2 * v * fields[1](fmed), np.exp(0.15 * v * fields[2](fmed))
    return 8 * v * rj.standard_normal(), 2 * v * rj.standard_normal(), np.exp(0.15 * v * rj.standard_normal())


def _decay_amp(p: dict, amp: list, fd: float) -> None:
    """Scale the time after the amplitude peak by fd."""
    tp = max(amp, key=lambda r: r[1])[0]
    p["amp"] = [[tp + (t - tp) * fd if t > tp else t, a] for t, a in amp]
    if p["t1"] > tp:
        p["t1"] = tp + (p["t1"] - tp) * fd


def _stretch_partial(p: dict, k: dict, ctx: dict) -> None:
    p["amp"] = [[float(_T(t, k, ctx)), a] for t, a in p["amp"]]
    p["freq"] = [[float(_T(t, k, ctx)), f] for t, f in p["freq"]]
    p["t0"], p["t1"] = float(_T(p["t0"], k, ctx)), float(_T(p["t1"], k, ctx))
    p["rel"] = p["rel"] * max(1.0, k["stretch"])


def _move_residual(res: dict, k: dict, ctx: dict, rj):
    lev = _moved_levels(res, k, ctx, rj)
    t = np.asarray(res["times"], dtype=float)
    piv = ctx["on"] + 0.01
    t = np.where(t > piv, piv + (t - piv) * k["decay"], t)
    return _T(t, k, ctx).tolist(), lev.tolist()


def _moved_levels(res: dict, k: dict, ctx: dict, rj) -> np.ndarray:
    lev = np.asarray(res["db"], dtype=float)
    off = lev <= -149
    bands = np.asarray(res["bands"], dtype=float)
    lb = np.log2(bands)
    if ctx["ratio"] != 1:
        lev = np.array([np.interp(lb - np.log2(ctx["ratio"]), lb, lev[:, j]) for j in range(lev.shape[1])]).T
    lev = lev + (k["brightness"] * np.log2(bands * ctx["ratio"] / ctx["f_ref"]))[:, None]
    if k["vary"]:
        lev = lev + (1.5 * k["vary"] * rj.standard_normal(len(bands)))[:, None]
    lev = lev + 20 * np.log10(max(k["residual"], 1e-6))
    lev[off] = -150.0
    return lev


def _length(model, parts, k, ctx) -> int:
    n_id = int(round(model["duration"] * SR))
    if k["decay"] > 1 or k["stretch"] != 1 or k["ring"]:
        ends = [p["t1"] + p["rel"] for p in parts] or [0.0]
        return int(np.ceil(max(max(ends), float(_T(model["duration"], k, ctx))) * SR)) + 1
    return n_id


def _ring(model, n, times, levels, k, ctx):
    end = max(float(times[-1]), float(_T(model["duration"], k, ctx))) + 0.06
    n = max(n, int(np.ceil(end * SR)))
    times = list(times) + [end]
    levels = [list(row) + [-150.0] for row in levels]
    return n, times, levels


def _bend(n, k, ctx, rj):
    if not k["vibrato"] and not k["drift"]:
        return None
    t = np.arange(n) / SR
    cents = np.zeros(n)
    if k["vibrato"]:
        fade = np.clip((t - ctx["on"]) / 0.15, 0, 1)
        cents += k["vibrato"] * np.sin(2 * np.pi * k["vibrato_rate"] * t + rj.uniform(0, 2 * np.pi)) * fade
    if k["drift"]:
        rates = 1.5 * rj.uniform(0.2, 1.0, 4)
        phs = rj.uniform(0, 2 * np.pi, 4)
        cents += k["drift"] * np.sum(np.sin(2 * np.pi * np.outer(t, rates) + phs), axis=1) / 2
    return cents


def _move_atoms(trans, k, ctx, rj):
    out = []
    v = k["vary"]
    coherent = bool(ctx["fields"] is not None or v == 0)
    for tr in trans:
        dc, dg = (30 * v * rj.standard_normal(), 2 * v * rj.standard_normal()) if coherent else (0, 0)
        atoms = []
        for f, tau, a, ph in tr["atoms"]:
            if not coherent:
                dc, dg = 30 * v * rj.standard_normal(), 2 * v * rj.standard_normal()
            f2 = f * ctx["ratio"] * 2 ** (dc / 1200)
            g = 10 ** ((dg + k["brightness"] * np.log2(max(f * ctx["ratio"], 20) / ctx["f_ref"])) / 20)
            atoms.append([f2, tau / ctx["ratio"], a * k["transient"] * g, ph])
        out.append({"t": float(_T(tr["t"], k, ctx)), "atoms": atoms})
    return out


def _strike_weight(model, n, ctx, k):
    t = np.arange(n) / SR
    w = np.zeros(n)
    for s in model.get("onsets", []):
        ts = float(_T(s, k, ctx))
        d = t - ts
        tri = np.clip(1 - np.abs(d - 0.0075) / 0.0125, 0, 1) * (d >= -0.002)
        w = np.maximum(w, tri)
    return w


def _stereo(model, y, seed, k, ctx):
    side = model.get("side")
    s = _fitted_side(side, y, seed, k, ctx) if side else _noise_side(model, len(y), seed, k)
    s = s * k["width"]
    return np.column_stack([y + s, y - s])


def _fitted_side(side: dict, y, seed, k, ctx) -> np.ndarray:
    t = np.asarray(side["times"], dtype=float)
    piv = ctx["on"] + 0.01
    t = _T(np.where(t > piv, piv + (t - piv) * k["decay"], t), k, ctx)
    r = np.random.default_rng((int(seed) * 31 + 7) & 0xFFFFFFFF)
    sign = -1.0 if r.uniform() < 0.5 else 1.0
    return space.shaped_side(y, np.asarray(side["bands"]) * ctx["ratio"], t, side["pan"], side["diffuse"], sign)


def _noise_side(model: dict, n: int, seed, k) -> np.ndarray:
    res = model["residual"]
    return synth_noise(res["bands"], res["times"], res["db"], n, int(seed) + 1000003, SR,
                       hf=res.get("hf")) * 10 ** (model.get("width", -120) / 20) * 10 ** (k["gain_db"] / 20)


# ================================================================ summary, io, code

def summary(model: dict) -> dict:
    parts = model.get("partials") or []
    return _summary(parts) if parts else {"partials": 0}


def _summary(parts: list) -> dict:
    peaks = np.array([_peak_of(p) for p in parts])
    fm = np.array([partial_freq(p) for p in parts])
    strong = _strong(peaks, parts)
    f_low = float(fm[strong].min())
    centroid = _centroid(peaks[strong], fm[strong])
    ratios = np.sort(fm[strong] / f_low)[:12]
    inh = _inharmonicity(ratios) if strong.sum() > 1 else 0.0
    glide = [round(1200 * np.log2(p["freq"][-1][1] / p["freq"][0][1]), 1) for p in parts[:8]]
    return {"partials": len(parts), "f_low": round(f_low, 2), "centroid": round(centroid, 2),
            "f_range": [round(float(fm.min()), 1), round(float(fm.max()), 1)], "top": _top(parts, peaks, fm),
            "ratios": [round(float(r), 3) for r in ratios], "inharmonicity": round(inh, 4),
            "glide_cents": glide}


def _centroid(peaks: np.ndarray, fm: np.ndarray) -> float:
    """Power-weighted mean log frequency, in Hz."""
    pw = 10 ** (peaks / 10)
    return float(2 ** (np.sum(pw * np.log2(fm)) / pw.sum()))


def _strong(peaks: np.ndarray, parts: list) -> np.ndarray:
    """Partials within 30 dB of the top that live long enough (or all within 30 dB)."""
    life = np.array([p["t1"] - p["t0"] for p in parts])
    strong = (peaks >= peaks.max() - 30) & (life >= min(0.1, 0.5 * life.max()))
    return strong if strong.any() else peaks >= peaks.max() - 30


def _inharmonicity(ratios: np.ndarray) -> float:
    return float(np.median(np.abs(ratios - np.round(ratios)) / np.maximum(np.round(ratios), 1)))


def _top(parts: list, peaks: np.ndarray, fm: np.ndarray) -> list[dict]:
    return [{"f": round(float(fm[i]), 1), "db": round(float(peaks[i]), 1), "t60": _t60(parts[i])}
            for i in np.argsort(-peaks)[:8]]


def _t60(p: dict) -> float | None:
    am = np.asarray(p["amp"], dtype=float)
    i = int(np.argmax(am[:, 1]))
    seg = am[i:]
    if len(seg) < 2 or seg[-1, 0] - seg[0, 0] < 0.01:
        return None
    slope = float(np.polyfit(seg[:, 0], seg[:, 1], 1)[0])
    return round(-60 / slope, 3) if slope < -0.5 else None


def describe(model: dict) -> str:
    s = model.get("summary") or summary(model)
    on = model.get("onsets", [])
    head = f"strike at {model.get('onset', 0) * 1000:.0f} ms ({len(on)} strikes)"
    an = model.get("analysis", {})
    tail = f"residual {an.get('residual_db', 'n/a')} dB, window {an.get('window')}, LSD {an.get('lsd_db')}"
    body = _partials_text(s) if s.get("partials") else "noise only, no partials"
    return f"{head}; {body}; {tail}"


def _partials_text(s: dict) -> str:
    inh = s["inharmonicity"]
    word = "harmonic" if inh < 0.01 else "slightly inharmonic" if inh < 0.04 else "inharmonic"
    ratios = ", ".join(f"{r:g}" for r in s["ratios"][:5])
    return (f"{s['partials']} partials {s['f_range'][0]:.0f}-{s['f_range'][1]:.0f} Hz, lowest "
            f"strong {s['f_low']:.0f} Hz, {word} (ratios {ratios}), centroid {s['centroid']:.0f} Hz, "
            f"{_t60_text(s)}{_glide_text(s)}")


def _t60_text(s: dict) -> str:
    t60s = [t["t60"] for t in s["top"] if t["t60"]]
    return f"T60 {min(t60s):.2f}-{max(t60s):.2f} s" if t60s else "T60 n/a"


def _glide_text(s: dict) -> str:
    glide = max(s["glide_cents"], key=abs) if s["glide_cents"] else 0
    return f", glide {glide:+.0f} cents" if abs(glide) > 50 else ""


def save(model: dict, path: str) -> None:
    save_json(model, path)


def load(path: str) -> dict:
    m = load_json(path)
    check(m)
    return m


def to_code(model: dict, name: str = "sound") -> str:
    """Standalone numpy + scipy source that renders the model (mono, identity knobs)."""
    import inspect
    import pprint
    lite = {k: model[k] for k in ("kind", "version", "duration", "onset", "partials", "transients",
                                  "residual", "coherent", "layer") if k in model}
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
