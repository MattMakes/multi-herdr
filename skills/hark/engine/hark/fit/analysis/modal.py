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
    X = np.abs(stft(m, nperseg=nper, noverlap=nper - hop, boundary="zeros", padded=True)[2])
    S = np.log1p(X / (0.001 * X.max() + 1e-12))
    flux = np.zeros(S.shape[1])
    if S.shape[1] > 2:
        flux[2:] = np.sum(np.minimum(np.maximum(S[:, 2:] - S[:, :-2], 0.0), cap), axis=0)
    energy = db(np.sum(X ** 2, axis=0))
    return flux, energy, np.arange(S.shape[1]) * hop


def onsets(x: np.ndarray, rel: float = 0.2, floor_db: float = -36, min_gap_ms: float = 12,
           max_n: int = 10) -> list[int]:
    """Strike onsets in samples."""
    x = np.asarray(x, dtype=float)
    out = [0]
    if x.size and np.any(x):
        out = _found_onsets(x, rel, floor_db, min_gap_ms, max_n)
    return out


def _found_onsets(x: np.ndarray, rel: float, floor_db: float, min_gap_ms: float, max_n: int) -> list[int]:
    flux, e, centre = onset_strength(x)
    found = [[_refine(x, int(centre[k])), float(flux[k])]
             for k in _onset_candidates(flux, e, rel, floor_db) if not _skip_candidate(x, int(centre[k]))]
    found = _strongest(_gap_filter(found, min_gap_ms), max_n)
    if found:
        return [int(f[0]) for f in found]
    return [int(np.argmax(np.abs(x) >= 0.1 * np.max(np.abs(x))))]


def _strongest(found: list, max_n: int) -> list:
    if len(found) <= max_n:
        return found
    keep = sorted(np.argsort([-f[1] for f in found], kind="stable")[:max_n])
    return [found[i] for i in keep]


def _onset_candidates(flux: np.ndarray, e: np.ndarray, rel: float, floor_db: float) -> list[int]:
    from scipy.ndimage import maximum_filter1d, minimum_filter1d
    fmax = float(flux.max())
    if fmax <= 0:
        return []
    local = flux >= maximum_filter1d(flux, 13, mode="constant")
    ahead = maximum_filter1d(e, 9, origin=-4, mode="nearest")
    before = minimum_filter1d(e, 9, origin=4, mode="nearest")
    need = np.where(flux >= 0.5 * fmax, 1.0, 3.0)
    ok = local & (flux >= rel * fmax) & (ahead >= e.max() + floor_db) & (ahead - before >= need)
    return [int(k) for k in np.flatnonzero(ok) if k >= 2]


def _skip_candidate(x: np.ndarray, c: int) -> bool:
    if c > len(x) - 192:
        return True
    if c < 128:
        return not (np.max(np.abs(x[:240])) >= 0.25 * np.max(np.abs(x)))
    return False


def _refine(x: np.ndarray, c: int) -> int:
    a, b = max(0, c - 240), min(len(x), c + 240)
    w = x[a:b]
    pk = float(np.max(np.abs(w))) if len(w) else 0.0
    if len(w) < 100 or pk <= 0:
        return c
    return a + _rise_index(w, pk)


def _rise_index(w: np.ndarray, pk: float) -> int:
    """Index of the strike start in w: the first loud sample after silence, else the first big step."""
    if np.sqrt(np.mean(w[:96] ** 2)) < 0.05 * pk:
        return int(np.argmax(np.abs(w) >= 0.1 * pk))
    d = np.abs(np.diff(w))
    thr = min(max(0.3 * d.max(), 4 * np.median(d[:96])), d.max())
    return int(np.argmax(d >= thr))


def _gap_filter(found: list, min_gap_ms: float) -> list:
    gap = min_gap_ms * SR / 1000.0
    out: list = []
    for t, fl in sorted(found, key=lambda f: f[0]):
        if out and t - out[-1][0] < gap:
            if fl > 2.5 * out[-1][1]:
                out[-1][1] = fl
            continue
        out.append([t, fl])
    return out


# ================================================================ fade-out

def fade_out(x: np.ndarray, cands=(0.005, 0.01, 0.02, 0.03, 0.05, 0.1, 0.25),
             gain_db: float = 0.5) -> float:
    """Length (s) of a linear fade at the end of x, or 0 when there is none."""
    x = np.asarray(x, dtype=float)
    pts = _tail_points(x)
    return 0.0 if pts is None else _best_fade(*pts, cands, gain_db)


def _tail_points(x: np.ndarray):
    """Tail envelope points (t, env dB, length s, window s), or None when too short."""
    hop = SR // 1000
    nh = len(x) // hop
    if nh < 20:
        return None
    from scipy.ndimage import convolve1d
    from hark.fit.analysis._common import hann_norm
    p = convolve1d(x ** 2, hann_norm(8 * hop), mode="nearest")
    t = (np.arange(nh) * hop + hop // 2) / SR
    env = db(p[(t * SR).astype(int)])
    T = len(x) / SR
    W = min(0.45 * T, 0.6)
    sel = (t >= T - W) & (env >= env.max() - 80)
    return (t[sel], env[sel], T, W) if sel.sum() >= 10 else None


def _best_fade(ts, es, T, W, cands, gain_db) -> float:
    base = _ramp_error(ts, es, T, 0.0)
    best, best_err = 0.0, base
    for L in (c for c in cands if c <= W / 2):
        err = _ramp_error(ts, es, T, L)
        if err < best_err:
            best, best_err = L, err
    return float(best) if best and best_err < base - gain_db else 0.0


def _ramp_error(t: np.ndarray, env: np.ndarray, T: float, L: float) -> float:
    r = 20 * np.log10(np.clip((T - t) / L, 0.001, 1.0)) if L > 0 else np.zeros_like(t)
    v = env - r
    coef = np.polyfit(t, v, 1)
    return float(np.mean(np.abs(v - np.polyval(coef, t))))


def _unfade(m: np.ndarray, fo: float) -> tuple[np.ndarray, int]:
    n = len(m)
    nf = int(round(fo * SR))
    y = m.copy()
    if nf <= 0:
        return y, n
    nf = min(nf, n)
    ramp = np.linspace(1.0, 0.0, nf)
    y[n - nf:] = y[n - nf:] / np.maximum(ramp, 0.25)
    return y, n - int(0.25 * nf)


# ================================================================ ESPRIT

def esprit(s: np.ndarray, kmax: int = 16, dyn_db: float = 60) -> tuple[np.ndarray, int]:
    """Poles of a sum of damped exponentials, model order by MDL."""
    s = np.asarray(s)
    out = (np.zeros(0, dtype=complex), 0)
    if len(s) >= 12:
        out = _esprit_poles(s, kmax, dyn_db)
    return out


def _esprit_poles(s: np.ndarray, kmax: int, dyn_db: float) -> tuple[np.ndarray, int]:
    N = len(s)
    L = max(4, N // 3)
    M = N - L + 1
    H = np.lib.stride_tricks.sliding_window_view(s, M)[:L]
    try:
        U, sv, _ = np.linalg.svd(H, full_matrices=False)
    except np.linalg.LinAlgError:
        sv = np.zeros(0)
    k = _mdl_order(sv, L, M, kmax, dyn_db)
    if k == 0:
        return np.zeros(0, dtype=complex), 0
    Uk = U[:, :k]
    phi = np.linalg.lstsq(Uk[:-1], Uk[1:], rcond=None)[0]
    return np.linalg.eigvals(phi), k


def _mdl_order(sv: np.ndarray, p: int, M: int, kmax: int, dyn_db: float) -> int:
    lam = sv ** 2
    if not len(lam) or lam[0] <= 0:
        return 0
    cap = min(kmax, p - 2, int(np.sum(sv > sv[0] * 10 ** (-dyn_db / 20))))
    best, best_k = np.inf, 0
    for k in range(max(cap, 0) + 1):
        rest = np.maximum(lam[k:], 1e-300)
        g = np.exp(np.mean(np.log(rest)))
        a = np.mean(rest)
        v = -M * (p - k) * np.log(g / a) + 0.5 * k * (2 * p - k) * np.log(M)
        if v < best:
            best, best_k = v, k
    return best_k


# ================================================================ mode candidates

def _band_candidates(seg: np.ndarray, kmax: int, nmax: int) -> list[tuple[float, float]]:
    Ns = len(seg)
    if Ns < 32:
        return []
    nfft = next_fast_len(2 * Ns)
    X = fft(seg, nfft)
    f = np.fft.fftfreq(nfft, 1.0 / SR)
    out = []
    for lo, hi in zip(MODE_BANDS[:-1], MODE_BANDS[1:]):
        out += _one_band(X, f, Ns, nfft, lo, hi, kmax, nmax)
    return out


def _band_mask(f: np.ndarray, lo: float, hi: float, tw: float) -> np.ndarray:
    up = np.clip((f - (lo - 2 * tw)) / tw, 0.0, 1.0)
    down = np.clip(((hi + 2 * tw) - f) / tw, 0.0, 1.0)
    m = (0.5 - 0.5 * np.cos(np.pi * up)) * (0.5 - 0.5 * np.cos(np.pi * down))
    return np.where(f > 0, m, 0.0)


def _one_band(X, f, Ns, nfft, lo, hi, kmax, nmax) -> list[tuple[float, float]]:
    base = _baseband(X, f, Ns, lo, hi)
    out = []
    if base is not None:
        d, fc, fs_d = base
        for w in _band_windows(d, nmax):
            out += _poles_to_modes(esprit(w, kmax)[0], fc, fs_d, lo, hi)
    return out


def _baseband(X, f, Ns, lo, hi):
    """Band lo..hi shifted to 0 Hz and decimated: (signal, centre, rate), or None when too short."""
    B = hi - lo
    tw = max(0.15 * B, 8.0)
    D = max(1, min(int(SR / (1.15 * (B + 4 * tw))), Ns // 48))
    if Ns // D < 16:
        return None
    fc = 0.5 * (lo + hi)
    a = ifft(X * _band_mask(f, lo, hi, tw))[:Ns]
    return (a * np.exp(-2j * np.pi * fc * np.arange(Ns) / SR))[::D], fc, SR / D


def _band_windows(d: np.ndarray, nmax: int) -> list[np.ndarray]:
    wins = [d[:nmax]]
    s2 = 3 * nmax // 4
    if len(d) > s2 + nmax // 2 and np.mean(np.abs(d[s2:s2 + nmax]) ** 2) > 1e-4 * np.mean(np.abs(d[:nmax]) ** 2):
        wins.append(d[s2:s2 + nmax])
    return wins


def _poles_to_modes(z, fc, fs_d, lo, hi) -> list[tuple[float, float]]:
    r = np.abs(z)
    z = z[(r > 1e-6) & (r < 0.99999)]
    out = []
    for p in z:
        fr = fc + np.angle(p) * fs_d / (2 * np.pi)
        tau = -1.0 / (np.log(abs(p)) * fs_d)
        if lo <= fr < hi and 0.0003 <= tau < 30 and np.pi * fr * tau >= 2:
            out.append((float(fr), float(tau)))
    return out


def _is_dup(f: float, tau: float, kept: list) -> bool:
    for fe, te in kept:
        tol = max(0.3, 0.0015 * f, 0.25 / (np.pi * np.sqrt(tau * te)))
        if abs(f - fe) < tol and abs(np.log(tau / te)) < 1.2:
            return True
    return False


def _dedupe(cands: list, kept: list) -> list:
    pool = list(kept)
    out = []
    for f, tau in cands:
        if not _is_dup(f, tau, pool):
            out.append((f, tau))
            pool.append((f, tau))
    return out


# ================================================================ least squares

def _poles(freqs, taus) -> np.ndarray:
    f = np.asarray(freqs, dtype=float)
    t = np.asarray(taus, dtype=float)
    return np.exp(-1.0 / (t * SR) + 2j * np.pi * f / SR)


def _geo(w: np.ndarray, L: np.ndarray) -> np.ndarray:
    near = np.abs(1 - w) < 1e-12
    safe = np.where(near, 0.5, 1 - w)
    return np.where(near, L, (1 - w ** L) / safe)


def _gram(z: np.ndarray, starts: np.ndarray, end: int) -> np.ndarray:
    s = np.asarray(starts)
    late = np.maximum.outer(s, s)
    L = np.maximum(end - late, 0).astype(float)
    pi = z[:, None] ** (late - s[:, None])
    pj = z[None, :] ** (late - s[None, :])
    S1 = pi * pj * _geo(np.multiply.outer(z, z), L)
    S2 = pi * np.conj(pj) * _geo(np.multiply.outer(z, np.conj(z)), L)
    RR, II = np.real(S1 + S2) / 2, np.real(S2 - S1) / 2
    RI, IR = np.imag(S1 - S2) / 2, np.imag(S1 + S2) / 2
    return np.block([[RR, RI], [IR, II]])


def _project(m: np.ndarray, z: np.ndarray, starts: np.ndarray, taus, end: int) -> np.ndarray:
    P = np.zeros(len(z), dtype=complex)
    taus = np.asarray(taus, dtype=float)
    for s in np.unique(starts):
        idx = np.flatnonzero(starts == s)
        stop = min(end, int(s + 16 * taus[idx].max() * SR) + 1, len(m))
        lz = np.log(z[idx])
        for c0 in range(int(s), stop, 8192):
            c1 = min(stop, c0 + 8192)
            k = np.arange(c0 - s, c1 - s)
            P[idx] += m[c0:c1] @ np.exp(np.multiply.outer(k, lz))
    return np.concatenate([P.real, P.imag])


def _solve(G: np.ndarray, b: np.ndarray, lam: float) -> np.ndarray:
    A = G + lam * np.diag(np.diag(G))
    try:
        return np.linalg.solve(A, b)
    except np.linalg.LinAlgError:
        return np.linalg.lstsq(A, b, rcond=None)[0]


def _quad(G: np.ndarray, x: np.ndarray, idx: np.ndarray, k: int) -> float:
    j = np.concatenate([idx, idx + k])
    return float(x[j] @ G[np.ix_(j, j)] @ x[j])


def _cancels(G: np.ndarray, x: np.ndarray, starts: np.ndarray, cross: float) -> bool:
    k = len(starts)
    total = float(x @ G @ x)
    strike_sum = 0.0
    for s in np.unique(starts):
        idx = np.flatnonzero(starts == s)
        joint = _quad(G, x, idx, k)
        sep = sum(_quad(G, x, np.array([i]), k) for i in idx)
        if sep > 2 * max(joint, 1e-300):
            return True
        strike_sum += joint
    return len(np.unique(starts)) > 1 and strike_sum > cross * max(total, 1e-300)


def _ls(m, starts, freqs, taus, end, ridge, cross):
    starts = np.asarray(starts, dtype=int)
    z = _poles(freqs, taus)
    G = _gram(z, starts, end)
    b = _project(m, z, starts, taus, end)
    lam = ridge
    x = _solve(G, b, lam)
    for _ in range(5):
        if not _cancels(G, x, starts, cross):
            break
        lam *= 10
        x = _solve(G, b, lam)
    k = len(starts)
    return x[:k] - 1j * x[k:], G, b, x


def ls_modes(m: np.ndarray, starts, freqs, taus, end: int | None = None, ridge: float = 1e-5,
             cross: float = 10) -> np.ndarray:
    """Complex amplitudes of damped modes by regularised least squares."""
    if not len(freqs):
        return np.zeros(0, dtype=complex)
    m = np.asarray(m, dtype=float)
    end = len(m) if end is None else int(end)
    return _ls(m, starts, freqs, taus, end, ridge, cross)[0]


def _energy(c: np.ndarray, taus) -> np.ndarray:
    return 0.5 * np.abs(c) ** 2 / (1 - np.exp(-2.0 / (np.asarray(taus, dtype=float) * SR)))


def _mode_signal(n: int, starts, freqs, taus, c, end: int | None = None) -> np.ndarray:
    y = np.zeros(n)
    if len(freqs):
        _add_modes(y, np.asarray(starts, dtype=int), freqs, np.asarray(taus, dtype=float), c,
                   n if end is None else min(int(end), n))
    return y


def _add_modes(y: np.ndarray, starts: np.ndarray, freqs, taus: np.ndarray, c, end: int) -> None:
    z = _poles(freqs, taus)
    for s in np.unique(starts):
        idx = np.flatnonzero(starts == s)
        stop = min(end, int(s + 16 * taus[idx].max() * SR) + 1)
        lz = np.log(z[idx])
        for c0 in range(int(s), stop, 8192):
            c1 = min(stop, c0 + 8192)
            k = np.arange(c0 - s, c1 - s)
            y[c0:c1] += np.real(np.exp(np.multiply.outer(k, lz)) @ c[idx])


# ================================================================ the passes

def _strike_ends(ons: list[int], active_end: int) -> list[int]:
    return [ons[i + 1] if i + 1 < len(ons) else active_end for i in range(len(ons))]


def _active_end(m: np.ndarray, n_fit: int) -> int:
    nfr = max(1, (len(m) - 24) // 12 + 1)
    e = np.array([np.sum(m[i * 12:i * 12 + 24] ** 2) for i in range(nfr)])
    if e.max() <= 0:
        return n_fit
    last = int(np.flatnonzero(e >= e.max() * 10 ** -7.5)[-1])
    return min(last * 12 + 24 + 12, n_fit)


def _new_modes(work, st, end, opts) -> list:
    on, skip = st["on"], opts["skip"]
    c0 = on + skip + int(round(0.0005 * SR))
    seg = work[c0:min(end, c0 + int(opts["analysis_s"] * SR))]
    cands = _dedupe(_band_candidates(seg, opts["kmax"], opts["nmax"]), st["carry"] + st["own"])
    room = opts["max_modes"] - len(st["own"])
    return _keep_new(work, cands, on + skip, end, room, opts) if cands and room > 0 else []


def _keep_new(work, cands, start, end, room, opts) -> list:
    """Keep the strongest new modes and subtract them from work."""
    f, t = zip(*cands)
    s = np.full(len(f), start)
    c = ls_modes(work[:end], s, f, t, end, opts["ridge"], opts["cross"])
    e = _energy(c, t)
    keep = [i for i in np.argsort(-e)[:room] if e[i] >= e.sum() * 10 ** (opts["prune_db"] / 10)]
    work[:] -= _mode_signal(len(work), s[keep], np.array(f)[keep], np.array(t)[keep], c[keep], end)
    return [cands[i] for i in keep]


def _carry(strikes: list, i: int) -> list:
    on = strikes[i]["on"]
    pool = []
    for st in strikes[:i]:
        for f, tau in st["own"]:
            if np.exp(-(on - st["on"]) / (tau * SR)) > 0.01:
                pool.append((f, tau))
    pool.sort(key=lambda p: -p[1])
    return _dedupe(pool[:40], [])


def _flat(strikes: list, skip: int):
    rows = [(i, f, t) for i, st in enumerate(strikes) for f, t in st["carry"] + st["own"]]
    if not rows:
        return np.zeros(0, int), np.zeros(0), np.zeros(0), np.zeros(0, int)
    idx, f, t = (np.array(v) for v in zip(*rows))
    starts = np.array([strikes[i]["on"] + skip for i in idx])
    return idx.astype(int), f.astype(float), t.astype(float), starts.astype(int)


def _set_modes(strikes: list, idx, f, t, keep) -> None:
    for i, st in enumerate(strikes):
        sel = [j for j in np.flatnonzero(idx == i) if keep[j]]
        n_carry = len(st["carry"])
        rows = list(zip(f[sel], t[sel]))
        pos = [j - np.flatnonzero(idx == i)[0] for j in sel]
        st["carry"] = [r for r, p in zip(rows, pos) if p < n_carry]
        st["own"] = [r for r, p in zip(rows, pos) if p >= n_carry]


def _joint(y, strikes, opts):
    """Joint least squares of every strike's modes, with prune and cluster rounds."""
    fitted = _prune_rounds(y, strikes, opts)
    return None if fitted is None else _cluster_rounds(y, strikes, opts, fitted)


def _fit_flat(y, strikes, opts):
    """(idx, f, t, starts, c, G, b, x) of every strike's modes, or None when there are none."""
    idx, f, t, s = _flat(strikes, opts["skip"])
    if not len(f):
        return None
    return (idx, f, t, s) + tuple(_ls(y, s, f, t, opts["n_fit"], opts["ridge"], opts["cross"]))


def _prune_rounds(y, strikes, opts):
    fitted = None
    for _ in range(2):
        fitted = _fit_flat(y, strikes, opts)
        if fitted is None or _prune(strikes, fitted, opts):
            break
    return fitted


def _prune(strikes, fitted, opts) -> bool:
    """Drop weak modes per strike. True when every mode stays."""
    idx, f, t, _, c = fitted[:5]
    e = _energy(c, t)
    keep = np.ones(len(f), bool)
    for i in range(len(strikes)):
        sel = idx == i
        if sel.any():
            keep[sel] = e[sel] >= e[sel].sum() * 10 ** (opts["prune_db"] / 10)
    done = bool(keep.all())
    if not done:
        _set_modes(strikes, idx, f, t, keep)
    return done


def _cluster_rounds(y, strikes, opts, fitted):
    for _ in range(3):
        idx, f, t, _, _, G, b, x = fitted
        drop = _cluster_drops(idx, f, t, G, b, x)
        if not drop.any():
            break
        _set_modes(strikes, idx, f, t, ~drop)
        fitted = _fit_flat(y, strikes, opts)
        if fitted is None:
            break
    return None if fitted is None else fitted[:5]


def _cluster_drops(idx, f, t, G, b, x) -> np.ndarray:
    k = len(f)
    drop = np.zeros(k, bool)
    for i in np.unique(idx):
        mi = np.flatnonzero(idx == i)
        mi = mi[np.argsort(f[mi], kind="stable")]
        for cl in _clusters(mi, f, t):
            sep = sum(_quad(G, x, np.array([j]), k) for j in cl)
            if sep > 2 * max(_quad(G, x, np.array(cl), k), 1e-300):
                best = max(cl, key=lambda j: _explained(G, b, j, k))
                drop[[j for j in cl if j != best]] = True
    return drop


def _clusters(mi, f, t) -> list[list[int]]:
    out, cur = [], [mi[0]] if len(mi) else []
    for a, b2 in zip(mi[:-1], mi[1:]):
        bw = 0.5 * (1 / (np.pi * t[a]) + 1 / (np.pi * t[b2]))
        if f[b2] - f[a] < max(2.0, bw):
            cur.append(b2)
        else:
            out.append(cur)
            cur = [b2]
    if cur:
        out.append(cur)
    return [c for c in out if len(c) >= 2]


def _explained(G, b, j, k) -> float:
    jj = [j, j + k]
    Gk = G[np.ix_(jj, jj)]
    try:
        return float(b[jj] @ np.linalg.solve(Gk, b[jj]))
    except np.linalg.LinAlgError:
        return 0.0


def _passes(y: np.ndarray, ons: list[int], opts: dict):
    strikes = [{"on": o, "own": [], "carry": []} for o in ons]
    ends = _strike_ends(ons, opts["active_end"])
    work = y.copy()
    result = None
    for _ in range(opts["passes"]):
        _grow(work, strikes, ends, opts)
        result = _joint(y, strikes, opts)
        if result is None:
            break
        idx, f, t, s, c = result
        work = y - _mode_signal(len(y), s, f, t, c, opts["n_fit"])
    return strikes, result


def _grow(work, strikes, ends, opts) -> None:
    """One pass of new modes for every strike."""
    for i, st in enumerate(strikes):
        st["carry"] = _carry(strikes, i)
        st["own"] = _dedupe(st["own"], st["carry"])
        end = opts["n_fit"] if i == len(strikes) - 1 else ends[i]
        if ends[i] - (st["on"] + opts["skip"]) >= 48:
            st["own"] += _new_modes(work, st, end, opts)


# ================================================================ strike rows

def _rows(idx, f, t, c, i, skip) -> list:
    sel = np.flatnonzero(idx == i)
    rows = []
    for j in sel[np.argsort(f[sel], kind="stable")]:
        a = abs(c[j]) * np.exp(skip / (t[j] * SR))
        ph = np.angle(c[j]) - 2 * np.pi * f[j] * skip / SR
        ph = (ph + np.pi) % (2 * np.pi) - np.pi
        row = [round(float(f[j]), 2), sig(t[j]), sig(a), round(float(ph), 4)]
        if row[1] > 0 and row[2] > 0:
            rows.append(row)
    return rows


def _rows_wave(rows: list, n: int) -> np.ndarray:
    return K.mode_wave(np.asarray(rows, dtype=float).reshape(-1, 4), n)


def _choose_ramps(y, strikes_rows, ons, skip):
    """Attack ramp per strike, and the mode signal with the chosen ramps."""
    n = len(y)
    modesig = np.zeros(n)
    ramps = []
    for i, (o, rows) in enumerate(zip(ons, strikes_rows)):
        w = _strike_wave(rows, n - o, i > 0)
        stop = min(n - o, skip + int(round(0.0015 * SR)))
        best = _best_ramp(w[:stop], (y - modesig)[o:o + stop])
        ramps.append(best if rows else 0.0)
        modesig[o:] += _ramped(w, best)
    return ramps, modesig


def _strike_wave(rows: list, n: int, later: bool) -> np.ndarray:
    w = _rows_wave(rows, n) if rows else np.zeros(n)
    if later:
        k = min(K.LATER_RAMP, len(w))
        w[:k] *= K.raised_cos(K.LATER_RAMP)[:k]
    return w


def _best_ramp(head: np.ndarray, target: np.ndarray) -> float:
    best, best_err = 0.0, None
    for r_ms in RAMPS_MS:
        err = float(np.sum((target - _ramped(head, r_ms)) ** 2))
        if best_err is None or err < best_err * (1 - 1e-9):
            best, best_err = r_ms, err
    return best


def _ramped(w: np.ndarray, r_ms: float) -> np.ndarray:
    r = int(round(r_ms * SR / 1000.0))
    if r <= 0:
        return w
    out = w.copy()
    k = min(r, len(out))
    out[:k] *= K.raised_cos(r)[:k]
    return out


def _sub_pulse(resid: np.ndarray, y: np.ndarray):
    n = len(y)
    if n <= 4 * K.SUB_DEC:
        return None, np.zeros(n)
    lp = sosfiltfilt(butter(4, 20, fs=SR, output="sos"), resid)
    if np.sum(lp ** 2) <= 0.001 * np.sum(y ** 2):
        return None, np.zeros(n)
    xs = [sig(v, 4) for v in resample_poly(lp, 1, K.SUB_DEC)]
    up = resample_poly(np.asarray(xs), K.SUB_DEC, 1)
    w = np.zeros(n)
    w[:min(n, len(up))] = up[:n]
    return {"rate": SR // K.SUB_DEC, "x": xs}, w


def _contact(resid: np.ndarray, ons: list[int], attack_ms: float):
    n = len(resid)
    r = resid.copy()
    out, total = [], np.zeros(n)
    for i, o in enumerate(ons):
        stop = min(o + int(round(attack_ms * SR / 1000)), ons[i + 1] if i + 1 < len(ons) else n, n)
        rows = _contact_rows(r, o, stop) if stop - o >= 24 else []
        out.append(rows)
        if rows:
            w = _rows_wave(rows, n - o)
            r[o:] -= w
            total[o:] += w
    return out, total


def _contact_rows(r: np.ndarray, o: int, stop: int) -> list:
    f, tau = _contact_poles(r[o:stop])
    rows = []
    if len(f):
        c = ls_modes(r, np.full(len(f), o), f, tau, min(len(r), o + int(0.006 * SR)))
        rows = [[round(float(fi), 2), sig(ti), sig(abs(ci)), round(float(np.angle(ci)), 4)]
                for fi, ti, ci in zip(f, tau, c)]
    return sorted([rw for rw in rows if rw[1] > 0 and rw[2] > 0], key=lambda rw: rw[0])


def _contact_poles(seg: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Frequencies and decays of the short contact modes in seg."""
    z = esprit(seg, 24, 50)[0]
    rad = np.abs(z)
    z = z[(rad > 1e-4) & (rad < 0.9999) & (np.angle(z) >= 0)]
    f = np.angle(z) * SR / (2 * np.pi)
    tau = -1.0 / (np.log(np.abs(z)) * SR)
    ok = (tau >= 1e-5) & (tau <= 0.002) & (f < 0.47 * SR)
    return f[ok], tau[ok]


# ================================================================ noise envelopes

def _band_power(x: np.ndarray, masks: np.ndarray, nfft: int) -> np.ndarray:
    n = len(x)
    X = rfft(x, nfft)
    out = np.zeros((len(masks), n))
    for b, w in enumerate(masks):
        full = np.zeros(nfft, dtype=complex)
        half = X * w
        full[:len(half)] = half
        full[1:(nfft + 1) // 2] *= 2
        out[b] = 0.5 * np.abs(ifft(full)[:n]) ** 2
    return out


def _smoothed(P: np.ndarray, ms: float, grid: np.ndarray) -> np.ndarray:
    from hark.fit.analysis._common import hann_norm
    w = hann_norm(max(1, int(round(ms * SR / 1000))))
    s = fftconvolve(P, w[None, :], mode="same", axes=1)
    return np.maximum(s[:, grid], 0.0)


def _weights(tg: np.ndarray, ons_s: list[float]) -> tuple[np.ndarray, np.ndarray]:
    near = np.zeros(len(tg))
    for o in ons_s:
        d = tg - o
        w = np.where(d < -0.008, 0.0, np.where(d <= 0.015, 1.0, np.clip((0.03 - d) / 0.015, 0, 1)))
        near = np.maximum(near, w)
    far = np.clip((tg - max(ons_s) - 0.15) / 0.15, 0.0, 1.0)
    return near, far


def _noise_power(x: np.ndarray, masks, nfft, grid, near, far) -> np.ndarray:
    P = _band_power(x, masks, nfft)
    s, m, l = (_smoothed(P, ms, grid) for ms in (0.5, 6.0, 30.0))
    return near * s + (1 - near) * ((1 - far) * m + far * l)


def _noise_env(m: np.ndarray, det: np.ndarray, ons: list[int], tol_db: float, pre_cap_s: float) -> dict:
    tg, far, E = _noise_db(m, det, ons, pre_cap_s)
    env = []
    for b in range(len(E)):
        keep = rdp(tg, E[b], tol_db * (1 + far))
        env.append([[round(float(tg[i]), 5), round(float(E[b, i]), 2)] for i in keep])
    return {"edges": [float(e) for e in NOISE_EDGES], "h_erb": 0.4, "env": env}


def _noise_db(m: np.ndarray, det: np.ndarray, ons: list[int], pre_cap_s: float):
    """Grid times, far weight and residual noise power (dB) per ERB band."""
    n = len(m)
    nfft = next_fast_len(n + 16384)
    masks = K.erb_masks(nfft)
    grid = np.arange(0, n, GRID)
    tg = grid / SR
    near, far = _weights(tg, [o / SR for o in ons])
    Po = _noise_power(m, masks, nfft, grid, near, far)
    Pd = _noise_power(det, masks, nfft, grid, near, far)
    P = np.maximum(Po - Pd, 1e-6 * Po)
    _pre_cap(P, ons, pre_cap_s)
    E = db(P)
    return tg, far, np.maximum(E, E.max() - 100.0)


def _pre_cap(P: np.ndarray, ons: list[int], pre_cap_s: float) -> None:
    for o in ons:
        k = o // GRID
        a = k - int(round(pre_cap_s / K.GRID_S))
        if a < 1:
            continue
        cap = np.min(P[:, max(0, a - 2):a + 1], axis=1, keepdims=True)
        P[:, a:k + 1] = np.minimum(P[:, a:k + 1], cap)


def _side_ratio(a: np.ndarray) -> list[float]:
    n = len(a)
    nfft = next_fast_len(n)
    masks = K.erb_masks(nfft)
    S = np.abs(rfft((a[:, 0] - a[:, 1]) / 2, nfft)) ** 2
    M = np.abs(rfft((a[:, 0] + a[:, 1]) / 2, nfft)) ** 2
    W2 = masks ** 2
    rho = np.sqrt((W2 @ S) / np.maximum(W2 @ M, 1e-300))
    return [round(float(v), 4) for v in np.minimum(rho, 1.5)]


# ================================================================ fit

FIT_KNOBS = {"max_strikes": 10, "kmax": 16, "nmax": 384, "passes": 2, "skip_ms": 0.5, "prune_db": -50,
             "max_modes": 80, "onset_rel": 0.2, "noise_tol_db": 1, "analysis_s": 3, "attack_ms": 2.5,
             "verbose": False, "ramps": RAMPS_MS, "pre_cap_s": 0.002, "side_t": True, "sr": SR}


def fit(x: Any, **kw) -> dict:
    """A modal model of a path or a 48 kHz array, (n,) or (n, 2). Knobs and defaults: FIT_KNOBS."""
    k = merge_knobs(FIT_KNOBS, kw, "modal fit")
    a = as_stereo(x) if isinstance(x, str) else as_stereo(resample(np.asarray(x, float), k["sr"]))
    model = _fit(a, k)
    model["source"] = source_name(x)
    if k["verbose"]:
        print(summary(model))
    return model


def _fit(a: np.ndarray, k: dict) -> dict:
    stereo = not is_mono_pair(a)
    m = a.mean(axis=1)
    ons, fo, y, opts = _fit_setup(m, k)
    det, strikes, sub, n_modes, modesig = _deterministic(y, ons, opts, k["attack_ms"])
    _fade_tail(det, fo)
    model = {"kind": KIND, "version": VERSION, "sr": SR, "length_s": round(len(m) / SR, 6),
             "channels": 2 if stereo else 1, "fade_out_s": float(fo), "strikes": strikes, "sub": sub,
             "noise": _noise_env(m, det, ons, k["noise_tol_db"], k["pre_cap_s"]), "side": None,
             "coherent": True, "fit": _diag(m, modesig, det, k["skip_ms"], n_modes)}
    if stereo:
        _add_side(model, a, k["side_t"])
    return model


def _fit_setup(m: np.ndarray, k: dict):
    """Onsets, fade-out length, the unfaded signal and the pass options."""
    ons = onsets(m, rel=k["onset_rel"], max_n=k["max_strikes"])
    fo = fade_out(m)
    y, n_fit = _unfade(m, fo)
    return ons, fo, y, _fit_opts(k, n_fit, _active_end(m, n_fit))


def _fit_opts(k: dict, n_fit: int, active_end: int) -> dict:
    return {"skip": int(round(k["skip_ms"] * SR / 1000)), "kmax": k["kmax"], "nmax": k["nmax"],
            "passes": k["passes"], "prune_db": k["prune_db"], "max_modes": k["max_modes"],
            "analysis_s": k["analysis_s"], "ridge": 1e-5, "cross": 10, "n_fit": n_fit,
            "active_end": active_end}


def _fade_tail(det: np.ndarray, fo: float) -> None:
    if fo > 0:
        n = len(det)
        nf = min(int(round(fo * SR)), n)
        det[n - nf:] *= np.linspace(1.0, 0.0, nf)


def _add_side(model: dict, a: np.ndarray, side_t: bool) -> None:
    model["side"] = _side_ratio(a)
    if side_t:
        from hark.fit.analysis import sinusoidal
        model["side_t"] = sinusoidal.side_model(a, len(a) / SR)


def _deterministic(y: np.ndarray, ons: list[int], opts: dict, attack_ms: float):
    n = len(y)
    _, res = _passes(y, ons, opts)
    rows = [[] for _ in ons]
    if res is not None:
        idx, f, t, s, c = res
        rows = [_rows(idx, f, t, c, i, opts["skip"]) for i in range(len(ons))]
    ramps, modesig = _choose_ramps(y, rows, ons, opts["skip"])
    sub, subw = _sub_pulse(y - modesig, y)
    attack, contact = _contact(y - modesig - subw, ons, attack_ms)
    strikes = [{"t": round(o / SR, 6), "ramp_ms": float(rp), "modes": rw, "attack": at}
               for o, rp, rw, at in zip(ons, ramps, rows, attack)]
    return modesig + subw + contact, strikes, sub, sum(len(r) for r in rows), modesig


def _diag(m, modesig, det, skip_ms, n_modes) -> dict:
    e = float(np.sum(m ** 2))
    share = float(db(np.sum(modesig ** 2) / e)) if e > 0 else -300.0
    resid = float(db(np.sum((m - det) ** 2) / e)) if e > 0 else -300.0
    return {"skip_ms": skip_ms, "modes": int(n_modes), "mode_share_db": round(share, 2),
            "resid_db": round(resid, 2)}


# ================================================================ face

def check(model: Any) -> None:
    """ValueError when the model is not a modal model of a supported version."""
    check_kind(model, KIND, VERSION)


def render(model: dict, seed: int = 0, **knobs) -> np.ndarray:
    """Validate, then render: (n, 2) for a stereo model or stereo=True, else (n,)."""
    check(model)
    if "gain_db" in knobs:
        knobs["gain"] = knobs.pop("gain_db")
    bad = sorted(set(knobs) - set(RENDER_KNOBS))
    if bad:
        raise ValueError(f"unknown modal knobs {bad}; known: {sorted(RENDER_KNOBS)}")
    return K.render_model(model, seed, **knobs)


def save(model: dict, path: str) -> None:
    """Write compact JSON."""
    save_json(model, path)


def load(path: str) -> dict:
    """Read a model and check its kind."""
    m = load_json(path)
    check(m)
    return m


def summary(model: dict) -> str:
    """A few lines of text."""
    ft = model.get("fit", {})
    st = model.get("strikes", [])
    lines = [f"modal v{model.get('version')}: {model['length_s'] * 1000:.0f} ms, {len(st)} strikes, "
             f"{ft.get('modes', 0)} modes, mode share {ft.get('mode_share_db')} dB, "
             f"residual {ft.get('resid_db')} dB"]
    for s in st:
        top = sorted(s["modes"], key=lambda r: -(r[2] ** 2 * r[1]))[:5]
        modes = ", ".join(f"{r[0]:.0f} Hz/{r[1] * 1000:.0f} ms" for r in top) or "none"
        lines.append(f"  strike {s['t'] * 1000:.1f} ms, ramp {s['ramp_ms']} ms, "
                     f"{len(s['modes'])} modes: {modes}")
    return "\n".join(lines)


# ================================================================ quick checks

def mel_distance(x: np.ndarray, y: np.ndarray) -> tuple[float, list[float]]:
    """Mean absolute mel dB at 3 FFT sizes, and the value per size."""
    return mel_db_distance(_mono(x), _mono(y))


def _mono(x: np.ndarray) -> np.ndarray:
    x = np.asarray(x, dtype=float)
    return x.mean(axis=1) if x.ndim == 2 else x


def _rms_env_db(x: np.ndarray, win: int, hop: int) -> np.ndarray:
    p = np.convolve(x ** 2, np.ones(win) / win, mode="same")[::hop]
    return db(p)


def env_corr(x: np.ndarray, y: np.ndarray) -> float:
    """Pearson correlation of 2 ms RMS dB envelopes, floored 60 dB under x's peak."""
    x, y = _mono(x), _mono(y)
    n = max(len(x), len(y))
    x, y = np.pad(x, (0, n - len(x))), np.pad(y, (0, n - len(y)))
    ex, ey = _rms_env_db(x, 96, 96), _rms_env_db(y, 96, 96)
    fl = ex.max() - 60
    ex, ey = np.maximum(ex, fl), np.maximum(ey, fl)
    if np.std(ex) == 0 or np.std(ey) == 0:
        return 0.0
    return float(np.corrcoef(ex, ey)[0, 1])


def rise_ms(x: np.ndarray) -> float:
    """10 % to 90 % rise time of a 1 ms RMS envelope, ms."""
    e = np.sqrt(np.convolve(_mono(x) ** 2, np.ones(48) / 48, mode="same")[::6])
    if not len(e) or e.max() <= 0:
        return 0.0
    k = int(np.argmax(e))
    a = max(0, k - int(0.03 * SR / 6))
    seg = e[a:k + 1]
    i10 = int(np.argmax(seg >= 0.1 * e[k]))
    i90 = int(np.argmax(seg >= 0.9 * e[k]))
    return float((i90 - i10) * 6 / SR * 1000)


# ================================================================ standalone code

def to_code(model: dict, name: str = "sound") -> str:
    """A standalone numpy + scipy script that renders the same samples as ``render``."""
    import inspect
    lite = copy.deepcopy({k: v for k, v in model.items() if k != "fit_engine"})
    return (f'"""Modal model {name!r}: standalone render (numpy and scipy only)."""\n\n'
            + inspect.getsource(K) + "\n\nMODEL = " + _literal(lite) + "\n\n\n"
            + "def render(seed=0, **knobs):\n    return render_model(MODEL, seed, **knobs)\n\n\n"
            + _MAIN_F32)


def _literal(model: dict) -> str:
    """The model as a Python literal, one mode row per line."""
    lines = ["{"]
    for k, v in model.items():
        if k == "strikes":
            lines.append("    'strikes': [")
            for s in v:
                lines.append("        {" + f"'t': {s['t']!r}, 'ramp_ms': {s['ramp_ms']!r},")
                lines.append("         'modes': [")
                lines += [f"             {r!r}," for r in s["modes"]]
                lines.append(f"         ], 'attack': {s['attack']!r}" + "},")
            lines.append("    ],")
        else:
            lines.append(f"    {k!r}: {v!r},")
    lines.append("}")
    return "\n".join(lines)


_MAIN_F32 = '''if __name__ == "__main__":
    import sys
    from scipy.io import wavfile
    seed = int(sys.argv[1]) if len(sys.argv) > 1 else 0
    out = sys.argv[2] if len(sys.argv) > 2 else "sound.wav"
    wavfile.write(out, SR, render(seed).astype(np.float32))
'''
