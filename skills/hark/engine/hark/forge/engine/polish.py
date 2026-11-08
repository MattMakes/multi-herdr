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
from scipy import ndimage

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

def _as_array(sig: Any) -> np.ndarray:
    if isinstance(sig, (str, os.PathLike)):
        from hark.core import audio

        sig = audio.load(str(sig), sr=SR)
    return np.asarray(sig, dtype=float)


def _one_channel(sig: Any) -> np.ndarray:
    lhs = _as_array(sig)
    return lhs.mean(axis=1) if lhs.ndim == 2 else lhs


def _rounded(val: Any, dd: int) -> Any:
    return None if val is None else round(float(val), dd)


def frame_db(x: np.ndarray, n: int) -> np.ndarray:
    """Power in dB of consecutive n-sample frames."""
    ky = len(x) // n
    if ky < 1:
        return np.array([-240.0])
    return 10 * np.log10(np.mean(x[: ky * n].reshape(ky, n) ** 2, axis=1) + 1e-24)


def power_stft(x: np.ndarray, nfft: int, hop: int) -> np.ndarray:
    """Hann-windowed power STFT, shape (frames, bins)."""
    if len(x) < nfft:
        x = np.concatenate([x, np.zeros(nfft - len(x))])
    ky = 1 + (len(x) - nfft) // hop
    grid_ix = np.arange(nfft)[None, :] + hop * np.arange(ky)[:, None]
    return np.abs(np.fft.rfft(x[grid_ix] * np.hanning(nfft), axis=1)) ** 2


def _kw_filter(sig: np.ndarray) -> np.ndarray:
    from hark.core.features import k_weight

    return np.asarray(k_weight(np.asarray(sig, dtype=float), SR), dtype=float)


def _fit_slope(vals_y: np.ndarray, step: float) -> float:
    tm = np.arange(len(vals_y)) * step
    return float(np.polyfit(tm, vals_y, 1)[0])


# ---------------------------------------------------------------- clicks

def _spike_share(curv: np.ndarray, ky: int) -> np.ndarray:
    fs = curv[: ky * 48].reshape(ky, 48) ** 2
    two_top = np.sort(fs, axis=1)[:, -2:].sum(axis=1)
    return two_top / (fs.sum(axis=1) + 1e-30)


def _boundary_level(mono: np.ndarray) -> float:
    if not len(mono):
        return -240.0
    pk_v = float(np.max(np.abs(mono))) + 1e-12
    return 20 * math.log10(max(abs(mono[0]), abs(mono[-1])) / pk_v + 1e-12)


def _find_clicks(mono: np.ndarray, gate_db: float) -> list[float]:
    curv = np.diff(mono, 2) if len(mono) > 2 else np.zeros(0)
    lev, lev2 = frame_db(curv, 48), frame_db(mono, 48)
    ky = min(len(lev), len(lev2))
    usable = ky >= 24 and len(curv) >= 48 * ky
    return _click_centres(lev[:ky], lev2[:ky], _spike_share(curv, ky), gate_db) if usable else []


def clicks(x: Any, thr: float = 18.0) -> dict:
    """Isolated discontinuities inside the sound."""
    mono = _one_channel(x)
    tlist = _find_clicks(mono, thr)
    return {"clicks": len(tlist), "click_times": tlist[:8], "edge_db": round(_boundary_level(mono), 1)}


def _click_mask(lev: np.ndarray, lev2: np.ndarray, spiky: np.ndarray, gate_db: float) -> np.ndarray:
    ky = len(lev)
    padded = np.pad(lev, 12, mode="edge")
    windows = np.lib.stride_tricks.sliding_window_view(padded, 25)[:ky]
    before_m = np.median(windows[:, 2:10], axis=1)
    after_m = np.median(windows[:, 15:23], axis=1)
    distant = np.abs(np.arange(ky) - int(np.argmax(lev2))) > 3
    return (lev - np.maximum(before_m, after_m) > gate_db) & (np.abs(after_m - before_m) < 8) & (lev2 > lev2.max() - 70) & distant & (spiky > 0.6)


def _streak_centres(flagged: np.ndarray) -> list[float]:
    """Mid times (s, 1 ms frames) of the runs of True."""
    acc, ix, ky = [], 0, len(flagged)
    while ix < ky:
        jx = ix
        while flagged[ix] and jx + 1 < ky and flagged[jx + 1]:
            jx += 1
        if flagged[ix]:
            acc.append(round(((ix + jx + 1) / 2) / 1000, 3))
        ix = jx + 1
    return acc


def _click_centres(lev: np.ndarray, lev2: np.ndarray, spiky: np.ndarray, gate_db: float) -> list[float]:
    return _streak_centres(_click_mask(lev, lev2, spiky, gate_db))


# ---------------------------------------------------------------- aliasing

class _FoldScan:
    """Shared state of the aliasing detector."""

    def __init__(self, size: int, stride: int):
        self.size, self.stride = size, stride
        self.bin_hz = SR / size
        self.bin_f = np.arange(size // 2 + 1) * self.bin_hz
        self.cands_hz = 150.0 * 2 ** (np.arange(0, int(96 * math.log2(3000 / 150)) + 1) / 96)
        self.cands_hz = self.cands_hz[self.cands_hz < 3000]
        self.mults = np.arange(1, int(4000 / 150) + 1)

    def harmonic_fit(self, s: np.ndarray, pw: np.ndarray) -> tuple[float, float] | None:
        rng_b = (self.bin_f >= 60) & (self.bin_f < 4000)
        band_sum = s[rng_b].sum()
        if band_sum <= 0:
            return None
        top_share, best_ix = -1e9, -1
        for ix, cc in enumerate(self.cands_hz):
            grid = cc * self.mults
            grid = grid[(grid >= 60) & (grid < 4000)]
            rhs = np.unique(np.round(grid / self.bin_hz).astype(int))
            frac = pw[rhs].sum() / band_sum - 0.02 * len(rhs)
            if frac > top_share:
                top_share, best_ix = frac, ix
        if top_share < 0.7:
            return None
        return self._sharpen(s, self.cands_hz[best_ix]), top_share

    def _sharpen(self, sv: np.ndarray, start_hz: float) -> float:
        upper = lower = 0.0
        for order_n in range(1, int(4000 / start_hz) + 1):
            hit_v = self._partial_near(sv, order_n, start_hz)
            if hit_v is not None:
                hz, wv = hit_v
                upper += wv * order_n * hz
                lower += wv * order_n * order_n
        return upper / lower if lower > 0 else start_hz

    def _partial_near(self, sv: np.ndarray, order_n: int, start_hz: float) -> tuple[float, float] | None:
        centre_k = int(round(order_n * start_hz / self.bin_hz))
        low, high = max(1, centre_k - 3), min(len(sv) - 2, centre_k + 3)
        if high < low:
            return None
        ky = low + int(np.argmax(sv[low:high + 1]))
        return self._interp_hz(sv, ky), sv[ky]

    def _interp_hz(self, sv: np.ndarray, ky: int) -> float:
        lhs, rhs, cc = np.log(sv[ky - 1:ky + 2] + 1e-30)
        curvature = lhs - 2 * rhs + cc
        shift = 0.5 * (lhs - cc) / curvature if curvature != 0 else 0.0
        return (ky + float(np.clip(shift, -1, 1))) * self.bin_hz


def _judged_range(scan: _FoldScan, f0v: float, blur: float) -> tuple | None:
    """(harmonics, half widths, top Hz, first bin, last bin) of the judged band, or None."""
    bin_hz = scan.bin_hz
    orders = np.arange(math.ceil(4000 / f0v), int(20000 / f0v) + 1)
    halfw = 2 + orders * blur
    usable = 2 * halfw * bin_hz < 0.5 * f0v
    if len(orders) == 0 or not usable[0]:
        return None
    n_ok = int(np.argmin(usable)) if not usable.all() else len(usable)
    orders, halfw = orders[:n_ok], halfw[:n_ok]
    ceiling = min(20000.0, (orders[-1] + 0.5) * f0v)
    return orders, halfw, ceiling, int(math.ceil(4000 / bin_hz)), int(ceiling / bin_hz)


def _range_energy(sv: np.ndarray, rng_b: tuple | None) -> float | None:
    if rng_b is None:
        return None
    lo_b, hi_b = rng_b[3], rng_b[4]
    band_e = sv[lo_b:hi_b + 1].sum()
    return None if hi_b - lo_b < 20 or band_e < 1e-3 * sv.sum() else band_e


def _partial_slots(orders: np.ndarray, halfw: np.ndarray, f0v: float, bin_hz: float) -> list[tuple[int, int]]:
    return [(int(round(order_n * f0v / bin_hz - wv)), int(round(order_n * f0v / bin_hz + wv))) for order_n, wv in zip(orders, halfw)]


def _fold_frame(scan: _FoldScan, sv: np.ndarray, f0v: float, blur: float, old_mode: bool) -> tuple[float, float] | None:
    rng_b = _judged_range(scan, f0v, blur)
    band_e = _range_energy(sv, rng_b)
    if band_e is None:
        return None
    fs = SimpleNamespace(spec=sv, f0v=f0v, blur=blur, bin_hz=scan.bin_hz, band_pw=band_e, orders=rng_b[0],
                         ceil_hz=rng_b[2], lo_bin=rng_b[3], hi_bin=rng_b[4])
    fs.slots = _partial_slots(rng_b[0], rng_b[1], f0v, scan.bin_hz)
    if old_mode:
        in_slots = sum(sv[max(lhs, fs.lo_bin):min(rhs, fs.hi_bin) + 1].sum() for lhs, rhs in fs.slots)
        outcome = ((1 - min(1.0, in_slots / band_e)) * band_e, band_e)
    else:
        outcome = _fold_energy(fs)
    return outcome


def _mirror_lines(f0v: float, blur: float, ceiling: float, bin_hz: float) -> tuple:
    """Alias lines in the band that sit clear of the nearest harmonic: ka, fa, hn, hwn, gap."""
    alias_k = np.arange(int(SR / 2 / f0v) + 1, int(4 * SR / f0v) + 1)
    fold_hz = np.abs(((alias_k * f0v + SR / 2) % SR) - SR / 2)
    kept = (fold_hz >= 4000) & (fold_hz <= ceiling)
    alias_k, fold_hz = alias_k[kept], fold_hz[kept]
    near_n = np.maximum(1, np.round(fold_hz / f0v))
    half_w = 2 + near_n * blur
    dist = np.abs(fold_hz - near_n * f0v) / bin_hz
    clear = dist >= half_w + 1
    return alias_k[clear], fold_hz[clear], near_n[clear], half_w[clear], dist[clear]


def _top_partial(fs: SimpleNamespace) -> float:
    return max([fs.spec[lhs:rhs + 1].max() for (lhs, rhs), order_n in zip(fs.slots, fs.orders) if order_n * fs.f0v >= 0.5 * fs.ceil_hz and rhs >= lhs]
               or [0.0])


def _line_gate(fs: SimpleNamespace, cc: int, near_n: float, half_w: float, dist: float) -> float:
    """Level a line must pass: over the local floor and over the leak of the nearest harmonic."""
    centre_b = int(round(near_n * fs.f0v / fs.bin_hz))
    near_pk = fs.spec[max(0, int(centre_b - half_w)):int(centre_b + half_w) + 1].max()
    dd = max(2.5, dist - half_w + 2)
    leak_v = near_pk * 10 ** ((-31.5 - 18 * math.log2(dd / 2.5)) / 10)
    return 10 ** (ALIAS_PEAK_DB / 10) * max(fs.bed[cc], leak_v)


def _judge_line(fs: SimpleNamespace, hz: float, aw_v: float, near_n: float, half_w: float, dist: float) -> tuple:
    """(judged, hit, excess power or None) of one alias line."""
    cc = int(round(hz / fs.bin_hz))
    lhs, rhs = max(fs.lo_bin, int(round(cc - aw_v))), min(fs.hi_bin, int(round(cc + aw_v)))
    if rhs < lhs:
        return False, False, None
    wv = fs.spec[lhs:rhs + 1]
    flagged = _line_gate(fs, cc, near_n, half_w, dist) < wv.max() <= ALIAS_LOUDEST * fs.ref_peak
    return True, flagged, (max(0.0, wv.sum() - fs.bed[cc] * len(wv)) if flagged else None)


def _tally_lines(fs: SimpleNamespace, table: np.ndarray, is_first: np.ndarray) -> tuple[int, int, float]:
    n_first = n_hit = 0
    extra = 0.0
    for line, first_f in zip(table, is_first):
        was_judged, flagged, gain = _judge_line(fs, *line)
        n_first += int(was_judged and bool(first_f))
        n_hit += int(was_judged and bool(first_f) and bool(flagged))
        extra = extra + gain if gain is not None else extra
    return n_first, n_hit, extra


def _fold_energy(fs: SimpleNamespace) -> tuple[float, float] | None:
    alias_k, fold_hz, near_n, half_w, dist = _mirror_lines(fs.f0v, fs.blur, fs.ceil_hz, fs.bin_hz)
    is_first = alias_k < SR / fs.f0v
    if is_first.sum() < 4:
        return None
    aw_v = np.minimum(1 + alias_k * fs.blur, np.maximum(0.5, dist - half_w - 0.5))
    fs.bed = ndimage.median_filter(fs.spec, size=65, mode="nearest")
    fs.ref_peak = _top_partial(fs)
    n_first, n_hit, extra = _tally_lines(fs, np.column_stack([fold_hz, aw_v, near_n, half_w, dist]), is_first)
    return min(fs.band_pw, extra) * min(1.0, (n_hit / max(1, n_first)) / ALIAS_FAMILY), fs.band_pw


def _frame_pitches(spg: np.ndarray, scan: _FoldScan, max_frames: int) -> dict:
    """Fitted f0 of each active frame with a harmonic fit."""
    lev_list = 10 * np.log10(spg.sum(axis=1) + 1e-30)
    busy = np.nonzero(lev_list > lev_list.max() - 30)[0][:max_frames]
    pow2 = sum(np.roll(spg, ky, axis=1) for ky in range(-2, 3))
    fit_map = {int(ix): scan.harmonic_fit(spg[ix], pow2[ix]) for ix in busy}
    return {ix: val[0] for ix, val in fit_map.items() if val is not None}


def _pitch_slew(pitch: dict, ix: int, f0v: float, stride: int) -> float:
    return max([abs(f0v - pitch[jx]) for jx in (ix - 1, ix + 1) if jx in pitch] or [0.0]) / (stride / SR)


def _fold_totals(scan: _FoldScan, spg: np.ndarray, pitch: dict, stride: int, old_mode: bool) -> tuple[float, float, int]:
    upper = lower = 0.0
    n_harm = 0
    for ix, f0v in pitch.items():
        blur = 0.5 * _pitch_slew(pitch, ix, f0v, stride) * (scan.size / SR) / scan.bin_hz
        outcome = _fold_frame(scan, spg[ix], f0v, blur, old_mode)
        if outcome is not None:
            upper += outcome[0]
            lower += outcome[1]
            n_harm += 1
    return upper, lower, n_harm


def aliasing(x: Any, nfft: int = 4096, hop: int = 1024, frames: int = 64, legacy: bool = False) -> dict:
    """Foldover of moving harmonic partials; inharmonic sounds are not judged."""
    spg = power_stft(_one_channel(x), nfft, hop)
    scan = _FoldScan(nfft, hop)
    total = spg.sum() + 1e-30
    air_db = 10 * math.log10(spg[:, scan.bin_f >= 20000].sum() / total + 1e-12)
    upper, lower, n_harm = _fold_totals(scan, spg, _frame_pitches(spg, scan, frames), hop, legacy)
    fold_v = upper / lower if lower > 0 and n_harm >= 2 else 0.0
    return {"fold": round(fold_v, 3), "harmonic_frames": n_harm, "nyq_db": round(air_db, 1)}


# ---------------------------------------------------------------- dc, resonances, evolution

def dc(x: Any) -> dict:
    """DC offset and infrasound share."""
    mono = _one_channel(x)
    pk_v = float(np.max(np.abs(mono))) + 1e-12 if len(mono) else 1e-12
    cnt = sp_fft.next_fast_len(max(len(mono), 1), real=True)
    pwr = np.abs(np.fft.rfft(mono, cnt)) ** 2
    hz = np.fft.rfftfreq(cnt, 1 / SR)
    sub_db = 10 * math.log10(pwr[hz < 15].sum() / (pwr.sum() + 1e-30) + 1e-12)
    return {"dc_db": round(20 * math.log10(abs(float(np.mean(mono))) / pk_v + 1e-12), 1), "infra_db": round(sub_db, 1)}


def _hearing_weight(hz: np.ndarray) -> np.ndarray:
    wv = np.full(len(hz), 0.5)
    wv[(hz >= 1000) & (hz < 9000)] = 0.7
    wv[(hz >= 2000) & (hz < 6000)] = 1.0
    return wv


def _peak_fft_size(cnt: int) -> int:
    return 4096 if cnt >= 4096 else max(1024, 1 << max(0, int(math.ceil(math.log2(max(cnt, 1))))))


def _fine_grid(mono: np.ndarray) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    """1/48-octave grid from 100 Hz, the cell maxima and their prominence."""
    size = _peak_fft_size(len(mono))
    lvl_db = 10 * np.log10(power_stft(mono, size, size // 4).sum(axis=0) + 1e-30)
    hz = np.fft.rfftfreq(size, 1 / SR)
    grid = 100.0 * 2 ** (np.arange(0, 48 * 8) / 48)
    grid = grid[grid < 16000]
    grid_db = np.array([_grid_cell_top(lvl_db, hz, cc) for cc in grid])
    return grid, grid_db, grid_db - ndimage.median_filter(grid_db, size=33, mode="nearest")


def _is_summit(stick_out: np.ndarray) -> np.ndarray:
    summit = np.zeros(len(stick_out), dtype=bool)
    summit[1:-1] = (stick_out[1:-1] >= stick_out[:-2]) & (stick_out[1:-1] >= stick_out[2:])
    return summit


def resonances(x: Any, top: int = 3) -> dict:
    """The harshest narrow spectral peak."""
    grid, grid_db, stick_out = _fine_grid(_one_channel(x))
    picks = np.nonzero((grid_db > grid_db.max() - 30) & (stick_out > 0) & _is_summit(stick_out))[0]
    if len(picks) == 0:
        return {"res_db": 0.0, "res_hz": None, "res_peaks": []}
    weighted = stick_out[picks] * _hearing_weight(grid[picks])
    ranked = picks[np.argsort(-weighted)]
    return {"res_db": round(float(weighted.max()), 1), "res_hz": int(round(grid[ranked[0]])),
            "res_peaks": [(int(round(grid[ix])), round(float(stick_out[ix]), 1)) for ix in ranked[:top]]}


def _grid_cell_top(lvl_db: np.ndarray, hz: np.ndarray, cc: float) -> float:
    mask = (hz >= cc / 2 ** (1 / 96)) & (hz < cc * 2 ** (1 / 96))
    if not mask.any():
        mask = np.zeros(len(hz), dtype=bool)
        mask[int(np.argmin(np.abs(hz - cc)))] = True
    return float(lvl_db[mask].max())


_BAND_EDGES = (0, 112, 224, 447, 894, 1789, 3578, 7155, math.inf)


def _band_energy(bandg: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    lin = 10 ** (bandg / 10)
    bands = np.column_stack([lin[:, (CEN >= _BAND_EDGES[jx]) & (CEN < _BAND_EDGES[jx + 1])].sum(axis=1) for jx in range(8)]) + 1e-12
    return lin, ndimage.uniform_filter1d(bands, 3, axis=0, mode="constant")


def _shape_change(bands: np.ndarray, total: np.ndarray, busy: np.ndarray) -> float | None:
    rel_db = np.maximum(10 * np.log10(bands / total[:, None]), -40)
    pairs = [ix for ix in range(5, len(bands)) if busy[ix] and busy[ix - 5]]
    if len(pairs) < 3:
        return None
    shares = bands / total[:, None]
    change = np.array([math.sqrt(np.sum((sv := (shares[ix] + shares[ix - 5]) / 2) * (rel_db[ix] - rel_db[ix - 5]) ** 2) / np.sum(sv))
                   for ix in pairs])
    return float(np.sum(change * total[pairs]) / np.sum(total[pairs]))


def _centre_range(lin: np.ndarray, busy: np.ndarray) -> float:
    cent_oct = np.log2((lin[busy] * CEN).sum(axis=1) / lin[busy].sum(axis=1))
    return round(float(np.percentile(cent_oct, 90) - np.percentile(cent_oct, 10)), 2)


def evolution(x: Any, G: np.ndarray | None = None) -> dict:
    """Change of spectral shape over 50 ms, independent of level."""
    G = tone.bandgram(_one_channel(x)) if G is None else G
    lin, bands = _band_energy(G)
    total = bands.sum(axis=1)
    busy = 10 * np.log10(total) > 10 * np.log10(total.max()) - 30
    evo_v = _shape_change(bands, total, busy) if busy.sum() >= 8 else None
    if evo_v is None:
        return {"evolution_db": None, "centroid_oct": None}
    return {"evolution_db": round(evo_v, 2), "centroid_oct": _centre_range(lin, busy)}


# ---------------------------------------------------------------- ring

def _db_spectrum(chunk: np.ndarray, fft_n: int) -> tuple[np.ndarray, np.ndarray]:
    wv = chunk * np.hanning(len(chunk))
    pwr = np.abs(np.fft.rfft(wv, fft_n)) ** 2
    return np.fft.rfftfreq(fft_n, 1 / SR), 10 * np.log10(pwr + 1e-30)


def _ripple_cep(lvl_db: np.ndarray, hz: np.ndarray, bin_hz: float, width_n: int) -> tuple | None:
    """Cepstrum values and quefrencies (s) between 2 and 40 ms of the 300 Hz - 12 kHz ripple."""
    mask = (hz >= 300) & (hz <= 12000)
    lvl_c = np.maximum(lvl_db[mask], lvl_db[mask].max() - 60)
    outcome = lvl_c - ndimage.uniform_filter1d(lvl_c, width_n, mode="nearest")
    cepv = np.abs(np.fft.rfft(outcome * np.hanning(len(outcome))))
    quef = np.arange(len(cepv)) / (len(outcome) * bin_hz)
    kept = (quef >= 0.002) & (quef <= 0.040)
    return (cepv[kept], quef[kept]) if kept.sum() >= 8 else None


def _mad_score(cc: np.ndarray) -> tuple[float, int]:
    ix = int(np.argmax(cc))
    middle = np.median(cc)
    return (cc[ix] - middle) / (1.4826 * np.median(np.abs(cc - middle)) + 1e-12), ix


def _live_width(lvl_db: np.ndarray, bin_hz: float, width_n: int) -> float:
    smooth_l = ndimage.uniform_filter1d(lvl_db, width_n, mode="nearest")
    return float(np.sum(smooth_l >= smooth_l.max() - 40) * bin_hz)


def _cep_top(chunk: np.ndarray, with_bw: bool = False) -> tuple | None:
    fft_n = 8192 if len(chunk) <= 8192 else 16384
    hz, lvl_db = _db_spectrum(chunk, fft_n)
    bin_hz = SR / fft_n
    width_n = max(3, int(round(2000 / bin_hz)))
    hit_v = _ripple_cep(lvl_db, hz, bin_hz, width_n)
    if hit_v is None:
        return None
    cc, quefs = hit_v
    zv, ix = _mad_score(cc)
    acc = (float(zv), float(quefs[ix] * 1000))
    return acc + (_live_width(lvl_db, bin_hz, width_n),) if with_bw else acc


def _comb_rough(chunk: np.ndarray, per_ms: float) -> float:
    pitch = 1000 / per_ms
    hz, lvl_db = _db_spectrum(chunk, 16384)
    bin_hz = SR / 16384
    lev_list = []
    for ky in range(math.ceil(300 / pitch), int(12000 / pitch) + 1):
        rhs = int(round(ky * pitch / bin_hz))
        lev_list.append(lvl_db[max(0, rhs - 2):rhs + 3].max())
    lev_list = np.array([val for val in lev_list if val >= lvl_db.max() - 50])
    if len(lev_list) < 8:
        return math.inf
    return float(np.median(np.abs(np.diff(lev_list, 2))))


def _body_cep(mono: np.ndarray, pk_ix: int) -> tuple | None:
    """The cepstral peak of the body around the loudest frame, when it is strong."""
    body_seg = mono[max(0, (pk_ix - 2) * 480):(pk_ix + 6) * 480]
    bodyc = _cep_top(body_seg) if len(body_seg) >= 1920 else None
    return bodyc if bodyc is not None and bodyc[0] >= RING_PITCH_Z else None


def _same_period(per_ms: float, per: float) -> bool:
    rel = per_ms / per
    return abs(rel - 1) <= 0.3 or any(abs(rel / ky - 1) <= 0.1 for ky in (2, 3, 0.5, 1 / 3))


def _pitched(mono: np.ndarray, pk_ix: int, tail_chunk: np.ndarray, per_ms: float) -> bool:
    if _comb_rough(tail_chunk, per_ms) <= RING_SMOOTH_DB:
        return True
    bodyc = _body_cep(mono, pk_ix)
    return bodyc is not None and _same_period(per_ms, bodyc[1])


def _tail_slice(mono: np.ndarray) -> tuple[int, np.ndarray | None]:
    """The loudest 10 ms frame and the tail from 30 ms after it down to -50 dB (None when short)."""
    lev = frame_db(mono, 480)
    pk_ix = int(np.argmax(lev))
    sv = pk_ix + 3
    under_ix = np.nonzero(lev[sv:] < lev[pk_ix] - 50)[0]
    stop = sv + int(under_ix[0]) if len(under_ix) else len(lev) - 1
    return pk_ix, (mono[sv * 480:(stop + 1) * 480][:16384] if stop - sv >= 6 else None)


def _ring_excuse(mono: np.ndarray, pk_ix: int, chunk: np.ndarray, per_ms: float, width_hz: float) -> str | None:
    """Why a strong cepstral peak is not a ring: narrow band or pitch; None when it is a ring."""
    is_narrow = width_hz < RING_SPAN_HZ
    is_pitch = not is_narrow and _pitched(mono, pk_ix, chunk, per_ms)
    return "narrow" if is_narrow else "pitch" if is_pitch else None


def ring(x: Any, legacy: bool = False) -> dict:
    """Comb-like metallic ringing in the tail."""
    mono = _one_channel(x)
    pk_ix, chunk = _tail_slice(mono)
    cep_info = _cep_top(chunk, with_bw=True) if chunk is not None else None
    if cep_info is None:
        return {"ring_z": None, "ring_ms": None}
    zv, per_ms, width_hz = cep_info
    reason = None if legacy or zv < 8 else _ring_excuse(mono, pk_ix, chunk, per_ms, width_hz)
    if reason is None:
        return {"ring_z": round(zv, 1), "ring_ms": round(per_ms, 2)}
    return {"ring_z": None, "ring_ms": round(per_ms, 2), "ring_raw_z": round(zv, 1), "ring_src": reason}


# ---------------------------------------------------------------- truncated, silence, clipping

def _cut_db(lev: np.ndarray, pk_ix: int, stop: float) -> float | None:
    tail_part = lev[max(pk_ix, len(lev) - 38):len(lev) - 8]
    rate = _fit_slope(tail_part, 0.005) if len(tail_part) >= 4 else 0.0
    fade_lvl = float(np.mean(lev[len(lev) - 10:len(lev) - 7]))
    expect_db = max(3.0, -rate * 0.04)
    cut_v = None
    if fade_lvl - stop > 3 * expect_db + 6 and fade_lvl > -50:
        cut_v = fade_lvl
    elif rate < -20 and stop > -50:
        cut_v = stop
    return cut_v


def truncated(x: Any) -> dict:
    """A tail cut while it is still decaying."""
    lev = frame_db(_one_channel(x), 240)
    if len(lev) < 12:
        return {"trunc_db": None, "end_db": None}
    pk_ix = int(np.argmax(lev))
    lev = lev - lev[pk_ix]
    stop = float(np.mean(lev[-2:]))
    cut_v = None if len(lev) - pk_ix - 1 < 12 or stop < -55 else _cut_db(lev, pk_ix, stop)
    return {"trunc_db": _rounded(cut_v, 1), "end_db": round(stop, 1)}


def silence(x: Any) -> dict:
    """Runs of exact zeros inside the sound."""
    mono = np.abs(_one_channel(x))
    pk_v = (mono.max() if len(mono) else 0.0) + 1e-12
    audible = np.nonzero(mono > 1e-3 * pk_v)[0]
    if len(audible) < 2:
        return {"silence_ms": 0.0, "silence_max_ms": 0.0}
    zv = mono[audible[0]:audible[-1] + 1] < 1e-6
    streaks = _streaks(zv)
    streaks = [f0v for f0v in streaks if f0v >= 96]
    return {"silence_ms": round(sum(streaks) / 48, 1), "silence_max_ms": round(max(streaks, default=0) / 48, 1)}


def _streaks(mask_b: np.ndarray) -> list[int]:
    dd = np.diff(np.concatenate([[0], mask_b.astype(int), [0]]))
    return list(np.nonzero(dd == -1)[0] - np.nonzero(dd == 1)[0])


def clipping(x: Any) -> dict:
    """Flat tops and samples over full scale."""
    lhs = _as_array(x)
    channels = [lhs] if lhs.ndim == 1 else [lhs[:, cc] for cc in range(lhs.shape[1])]
    streaks = n_over = 0
    for cc in channels:
        chan_pk = np.max(np.abs(cc)) if len(cc) else 0.0
        if chan_pk <= 0:
            continue
        n_over += int(np.sum(np.abs(cc) > 1.0))
        flat_mask = (np.abs(cc[1:]) >= 0.99 * chan_pk) & (np.abs(np.diff(cc)) < 2e-6)
        streaks += sum(1 for f0v in _streaks(flat_mask) if f0v >= 2)
    return {"clip_runs": streaks, "overs": n_over}


# ---------------------------------------------------------------- polish measures

def _rise_frames(mono: np.ndarray, cnt: int) -> tuple[int, int]:
    """The 10 % and 90 % frames (0.25 ms) of the onset."""
    pow2 = np.mean(mono[: cnt * 12].reshape(cnt, 12) ** 2, axis=1)
    envv = np.sqrt(np.convolve(pow2, np.ones(2) / 2, mode="same"))
    top_ix = int(np.argmax(envv >= 0.9 * envv.max()))
    quiet = np.nonzero(envv[: top_ix + 1] < 0.1 * envv.max())[0]
    return (int(quiet[-1]) if len(quiet) else 0), top_ix


def _onset_air(chunk: np.ndarray) -> float | None:
    if len(chunk) < 32:
        return None
    pwr = np.abs(np.fft.rfft(chunk * np.hanning(len(chunk)), 1024)) ** 2
    hz = np.fft.rfftfreq(1024, 1 / SR)
    return 10 * math.log10(pwr[hz >= 6000].sum() / (pwr.sum() + 1e-30) + 1e-12)


def attack(x: Any) -> dict:
    """10 %-90 % attack time and the high-frequency share of the onset."""
    mono = _one_channel(x)
    cnt = len(mono) // 12
    if cnt < 4:
        return {"attack_ms": None, "onset_hf_db": None}
    low_ix, top_ix = _rise_frames(mono, cnt)
    air = _onset_air(mono[12 * low_ix:12 * low_ix + 480])
    return {"attack_ms": round((top_ix - low_ix) * 0.25, 2), "onset_hf_db": _rounded(air, 1)}


def _decay_curve(mono: np.ndarray, ky: int) -> np.ndarray:
    """10 ms levels from the loudest frame on, re that frame."""
    pow2 = np.mean(mono[: ky * 480].reshape(ky, 480) ** 2, axis=1)
    lev = 10 * np.log10(np.convolve(pow2, np.ones(3) / 3, mode="same") + 1e-24)
    pk_ix = int(np.argmax(lev))
    return lev[pk_ix:] - lev[pk_ix]


def _rerise(curve: np.ndarray) -> float:
    active = curve > -45
    active[:2] = False
    lift = curve - np.minimum.accumulate(curve)
    return float(lift[active].max()) if active.any() else 0.0


def _fall_rate(curve: np.ndarray) -> float | None:
    use = (curve >= -45) & (curve <= -6)
    if use.sum() < 3:
        return None
    tm = np.nonzero(use)[0] * 0.01
    return float(np.polyfit(tm, curve[use], 1)[0])


def tail(x: Any) -> dict:
    """Tail bump, decay rate and t40."""
    mono = _one_channel(x)
    ky = len(mono) // 480
    if ky < 4:
        return {"bump_db": 0.0, "decay_db_s": None, "t40_ms": None}
    curve = _decay_curve(mono, ky)
    gone = np.nonzero(curve < -40)[0]
    t40_v = 10.0 * (int(gone[0]) if len(gone) else len(curve))
    return {"bump_db": round(_rerise(curve), 1), "decay_db_s": _rounded(_fall_rate(curve), 1), "t40_ms": t40_v}


def body(x: Any) -> dict:
    """Crest factor of the loudest 50 ms, K-weighted."""
    mono = _one_channel(x)
    kmono = _kw_filter(mono)
    sq_k = kmono ** 2
    if len(sq_k) >= 2400:
        loud_v = float(np.max(np.convolve(sq_k, np.ones(2400) / 2400, mode="valid")))
    else:
        loud_v = float(sq_k.sum() / 2400)
    pk_v = float(np.max(np.abs(mono))) if len(mono) else 0.0
    return {"crest_db": round(20 * math.log10(pk_v + 1e-12) - 10 * math.log10(loud_v + 1e-24), 1)}


def lufs_max(x: Any) -> float:
    """Momentary maximum loudness (400 ms) in LUFS."""
    lhs = _as_array(x)
    channels = [lhs] if lhs.ndim == 1 else [lhs[:, cc] for cc in range(lhs.shape[1])]
    sq_k = sum(_kw_filter(cc) ** 2 for cc in channels)
    if len(sq_k) >= 19200:
        val = float(np.max(np.convolve(sq_k, np.ones(19200) / 19200, mode="valid")))
    else:
        val = float(np.sum(sq_k) / 19200)
    return -0.691 + 10 * math.log10(val + 1e-24)


def loudness_spread(xs: Sequence[Any]) -> dict:
    """Loudness of a set of variants: values, range and standard deviation."""
    val = [lufs_max(sig) for sig in xs]
    if not val:
        return {"lufs": [], "spread_lu": None, "std_lu": None}
    return {"lufs": [round(lv_u, 2) for lv_u in val], "spread_lu": round(max(val) - min(val), 2),
            "std_lu": round(float(np.std(val)), 2)}


def measure(x: Any, G: np.ndarray | None = None) -> dict:
    """Every detector and polish measure of one sound."""
    lhs = _as_array(x)
    acc: dict = {}
    for meas in (clicks, aliasing, dc, resonances, ring, truncated, silence, clipping, attack, tail, body):
        acc.update(meas(lhs))
    acc.update(evolution(lhs, G))
    return acc


# ---------------------------------------------------------------- PolishRef

def _under_cost(val: float, refv: list, slack: float, maxc: float, sev: str) -> tuple[float, float | None, str]:
    """Penalty of a measure where low is bad, against the refs' median."""
    if len(refv) < 2 or np.median(refv) < 1.5:
        return 0.0, None, sev
    bound = slack * float(np.median(refv))
    return min(maxc, max(0.0, 2 * (bound - val) / max(bound, 1e-6))), bound, sev


def _designed_adjust(name: str, val: float, cost: float, bound: float, sev: str, is_gated: bool) -> tuple[float, float, str]:
    """Designed sounds: clicks and silence count half; a tail cut is judged against the gate level."""
    unit, maxc = RULES[name][3], RULES[name][5]
    if name in ("clicks", "silence_ms"):
        sev, cost = "soft", cost / 2
    elif name == "trunc_db":
        bound = max(bound, GATE_DB["gated"] if is_gated else GATE_DB["designed"])
        cost = min(maxc, max(0.0, (val - bound) / unit))
        sev = "soft" if is_gated else sev
    return cost, bound, sev


def _artifact_cost(name: str, val: float, refv: list, is_designed: bool, is_gated: bool) -> tuple[float, float | None, str]:
    sign, floor_v, slack, unit, sev, maxc = RULES[name]
    if sign < 0:
        return _under_cost(val, refv, slack, maxc, sev)
    bound = max(floor_v, max(refv) + slack) if refv else floor_v
    cost = min(maxc, max(0.0, (val - bound) / unit))
    if is_designed:
        cost, bound, sev = _designed_adjust(name, val, cost, bound, sev, is_gated)
    return cost, bound, sev


def _ratio_cost(val: float, refv: list, sign: float, slack: float, unit: float) -> tuple[float, float]:
    if sign > 0:
        bound = slack * max(refv) + 1
        return math.log2(max(val, 1e-3) / bound) / unit, bound
    bound = slack * min(refv)
    return math.log2(bound / max(val, 1e-3)) / unit, bound


def _diff_cost(val: float, refv: list, sign: float, slack: float, unit: float) -> tuple[float, float]:
    pad = max(slack, 0.25 * (max(refv) - min(refv)))
    if sign > 0:
        bound = max(refv) + pad
        return (val - bound) / unit, bound
    bound = min(refv) - pad
    return (bound - val) / unit, bound


def _finish_cost(name: str, val: float, refv: list) -> tuple[float, float] | None:
    sign, sev, slack, unit = POLISH[name]
    if name == "bump_db":
        bound = max(4.0, max(refv) + slack) if refv else 4.0
        outcome = ((val - bound) / unit, bound)
    elif len(refv) < 2:
        outcome = None
    elif sev == "log":
        outcome = _ratio_cost(val, refv, sign, slack, unit)
    else:
        outcome = _diff_cost(val, refv, sign, slack, unit)
    return outcome


class PolishRef:
    """Artifact and polish penalties of a sound against real references."""

    def __init__(self, refs: Sequence[Any] = (), designed: bool = False, gated: bool = False,
                 targets: dict | None = None):
        self.M = [measure(f0v) for f0v in refs]
        self.n = len(self.M)
        self.is_designed = designed
        self.is_gated = gated
        self.fallback = dict(targets or {})

    def _ref_vals(self, name: str, kept: list[int]) -> list:
        return [self.M[ix][name] for ix in kept if self.M[ix].get(name) is not None]

    def check(self, x: Any, exclude: Any = None, G: np.ndarray | None = None, m: dict | None = None) -> dict:
        """Penalties, limits, hard faults and words for one sound."""
        skip = set() if exclude is None else ({exclude} if np.isscalar(exclude) else set(exclude))
        kept = [ix for ix in range(self.n) if ix not in skip]
        m = measure(x, G) if m is None else m
        cc = {"measures": m, "limits": {}, "art": {}, "polish": {}, "hard": []}
        self._artifact_terms(cc, m, kept)
        self._finish_terms(cc, m, kept)
        cc["art_pen"] = round(sum(cc["art"].values()), 3)
        cc["polish_pen"] = round(sum(cc["polish"].values()), 3)
        cc["words"] = self.words(cc, kept)
        return cc

    def _artifact_terms(self, cc: dict, mono: dict, kept: list[int]) -> None:
        for name in RULES:
            val = mono.get(name)
            if val is None:
                continue
            cost, bound, sev = _artifact_cost(name, float(val), self._ref_vals(name, kept), self.is_designed, self.is_gated)
            if bound is None:
                continue
            cc["limits"][name] = round(bound, 2)
            if cost > 0:
                cc["art"][name] = round(cost, 3)
            if sev == "hard" and cost >= HARD_AT:
                cc["hard"].append(name)

    def _finish_terms(self, cc: dict, mono: dict, kept: list[int]) -> None:
        for name in POLISH:
            val = mono.get(name)
            if val is None:
                continue
            refv = self._ref_vals(name, kept)
            if name != "bump_db" and len(refv) < 2 and name in self.fallback:
                refv = list(self.fallback[name])
            outcome = _finish_cost(name, float(val), refv)
            if outcome is None:
                continue
            pwr, bound = outcome
            cc["limits"][name] = round(bound, 2)
            if pwr > 0:
                cc["polish"][name] = round(min(2.0, pwr), 3)

    def words(self, c: dict, keep: list[int] | None = None) -> str:
        """Plain text of the flagged artifacts and polish items."""
        keep = list(range(self.n)) if keep is None else keep
        mono = c["measures"]
        art_w = [_phrase(ky, mono, self._ref_vals(ky, keep)) for ky in sorted(c["art"], key=lambda ky: -c["art"][ky])]
        pol_w = [_phrase(ky, mono, self._ref_vals(ky, keep)) for ky in sorted(c["polish"], key=lambda ky: -c["polish"][ky])]
        bits = []
        if art_w:
            bits.append("artifacts: " + "; ".join(art_w))
        if pol_w:
            bits.append("polish: " + "; ".join(pol_w))
        return "; ".join(bits) if bits else "clean (no artifact or polish flag)"


def _ref_range(refv: list, dd: int | None = None) -> str:
    if not refv:
        return ""
    low, high = min(refv), max(refv)
    return f" (refs {low:.{dd}f}-{high:.{dd}f})" if dd is not None else f" (refs {low}-{high})"


_PHRASES = {
    "clicks": lambda val, mono, refv: f"{val} click(s) ({', '.join(f'{tm:.3f} s' for tm in mono.get('click_times', [])[:3])})",
    "edge_db": lambda val, mono, refv: f"starts / ends off zero ({val} dB re peak)",
    "fold": lambda val, mono, refv: f"aliasing: {val * 100:.0f}% of the moving partials fold back",
    "nyq_db": lambda val, mono, refv: f"energy above 20 kHz {val} dB" + _ref_range(refv),
    "dc_db": lambda val, mono, refv: f"DC offset {val} dB re peak",
    "infra_db": lambda val, mono, refv: f"infrasound below 15 Hz {val} dB" + _ref_range(refv),
    "res_db": lambda val, mono, refv: f"narrow resonance at {mono.get('res_hz')} Hz +{val} dB" + _ref_range(refv),
    "ring_z": lambda val, mono, refv: f"metallic comb ring every {mono.get('ring_ms') or 0:.1f} ms (z {val})" + _ref_range(refv),
    "trunc_db": lambda val, mono, refv: f"tail cut at {val} dB",
    "silence_ms": lambda val, mono, refv: f"{val} ms of digital silence inside",
    "clip_runs": lambda val, mono, refv: f"clipped ({val} flat tops)",
    "evolution_db": lambda val, mono, refv: f"frozen spectrum ({val:.1f} dB of change, refs median "
                                     f"{float(np.median(refv)) if refv else 0:.1f})",
    "attack_ms": lambda val, mono, refv: f"attack {val} ms" + _ref_range(refv, 1),
    "onset_hf_db": lambda val, mono, refv: f"dull onset (>6 kHz share {val} dB" + _ref_range(refv) + ")",
    "bump_db": lambda val, mono, refv: f"bumpy tail (+{val} dB re-rise)",
    "t40_ms": lambda val, mono, refv: f"tail dies fast (t40 {val} ms" + _ref_range(refv) + ")",
    "crest_db": lambda val, mono, refv: f"thin body (crest {val:.1f} dB" + _ref_range(refv, 1) + ")",
}


def _phrase(name: str, mono: dict, refv: list) -> str:
    return _PHRASES[name](mono.get(name), mono, refv)


def _audio_list(plist: Sequence[str]) -> list[str]:
    suffixes = (".wav", ".flac", ".aif", ".aiff", ".ogg", ".mp3")
    acc = []
    for pwr in plist:
        if os.path.isdir(pwr):
            acc += sorted(os.path.join(dd, hz) for dd, _, fnames in os.walk(pwr) for hz in fnames if hz.lower().endswith(suffixes))
        else:
            acc.append(pwr)
    return acc


def main(argv: Sequence[str] | None = None) -> int:
    """Print the detector values of each audio file."""
    for fpath in _audio_list(list(sys.argv[1:] if argv is None else argv)):
        mono = measure(fpath)
        print(os.path.basename(fpath), " ".join(f"{ky}={mono.get(ky)}" for ky in PRINT_KEYS))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
