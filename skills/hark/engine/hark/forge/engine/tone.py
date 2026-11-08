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
from numpy import (
    arange,
    argmax,
    argmin,
    array,
    asarray,
    clip,
    column_stack,
    delete,
    floor,
    full,
    isscalar,
    ix_,
    log10,
    maximum,
    mean,
    median,
    minimum,
    ndarray,
    nonzero,
    sqrt,
    zeros,
)
from scipy.signal import butter, sosfilt

SR = 48000
CEN = array([1000.0 * 2 ** (k / 3) for k in range(-15, 13)])
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

_SOS_CACHE: list | None = None


# ---------------------------------------------------------------- input

def as_mono(x: Any) -> ndarray:
    """Load a path (resampled to 48 kHz) or take an array, and average stereo down to one channel."""
    src = x
    if isinstance(src, (str, os.PathLike)):
        from hark.core import audio

        src = audio.load(str(src), sr=SR)
    arr = asarray(src, float)
    return arr if arr.ndim != 2 else arr.mean(axis=1)


def label(c: float) -> str:
    """Band label, for example '1.00 kHz' or '250 Hz'."""
    return f"{c / 1000:.3g} kHz" if c >= 1000 else f"{round(c)} Hz"


def _freqs_and_power(sig: ndarray) -> tuple[ndarray, ndarray]:
    spec = np.fft.rfft(sig)
    return np.fft.rfftfreq(len(sig), 1.0 / SR), np.abs(spec) ** 2


# ---------------------------------------------------------------- spectrum

def profile(x: Any) -> ndarray:
    """Level of each of the 28 third-octave bands in dB re their summed power, never below -60."""
    hz, pw = _freqs_and_power(as_mono(x))
    per_band = [pw[(hz >= fc / EDGE) & (hz < fc * EDGE)].sum() for fc in CEN]
    levels = array(per_band) + 1e-20
    return maximum(10 * log10(levels / levels.sum()), -60.0)


def shares(x: Any) -> dict:
    """Fraction of total power that falls in each named region, in dB rounded to 0.1."""
    hz, pw = _freqs_and_power(as_mono(x))
    whole = pw.sum() + 1e-20
    result = {}
    for region, (f_lo, f_hi) in REGIONS.items():
        inside = pw[(hz >= f_lo) & (hz < f_hi)].sum()
        result[region] = _fround(10 * log10(inside / whole + 1e-12), 1)
    return result


def t40_ms(x: Any, db: float = 40.0, ms: float = 5.0) -> float:
    """Milliseconds from the loudest ms-long block until a block falls db below it (or the end)."""
    sig = as_mono(x)
    hop = int(SR * ms / 1000)
    count = len(sig) // hop
    if count < 2:
        return 0.0
    blocks = sig[: count * hop].reshape(count, hop)
    lev = 20 * log10(sqrt(mean(blocks * blocks, axis=1)) + 1e-12)
    loud_at = int(argmax(lev))
    quiet = nonzero(lev[loud_at:] < lev[loud_at] - db)[0]
    after = int(quiet[0]) if len(quiet) else count - loud_at
    return float(after * ms)


# ---------------------------------------------------------------- ToneRef

def _fround(v: Any, digits: int) -> float:
    return round(float(v), digits)


def _rms_of(dev_v: ndarray) -> float:
    return float(sqrt(mean(dev_v ** 2)))


def _kept(total_refs: int, exclude: Any) -> list[int]:
    """Indices 0..count-1 minus the excluded one(s); exclude is None, an index or a collection."""
    if exclude is None:
        dropped: set = set()
    else:
        dropped = {exclude} if isscalar(exclude) else set(exclude)
    return [row_i for row_i in range(total_refs) if row_i not in dropped]


def _envelope_of(kept_profs: ndarray) -> tuple:
    if len(kept_profs):
        return median(kept_profs, axis=0), kept_profs.min(axis=0), kept_profs.max(axis=0)
    return None, None, None


def _loo_distances(kept_profs: ndarray) -> ndarray:
    """RMS distance of each profile from the median of all the others."""
    return array([_rms_of(kept_profs[row_i] - median(delete(kept_profs, row_i, axis=0), axis=0)) for row_i in range(len(kept_profs))])


def _measure_ref(sig: ndarray) -> tuple:
    """Profile, t40, region shares and duration of one reference."""
    return profile(sig), t40_ms(sig), shares(sig), len(sig) / SR


class ToneRef:
    """Third-octave balance of a reference set, and how far a new sound sits from it."""

    def __init__(self, refs: Sequence[Any], margin: float = 3.0):
        measured = [_measure_ref(as_mono(ref_in)) for ref_in in refs]
        cols = list(zip(*measured)) if measured else [(), (), (), ()]
        profs = array(cols[0]) if measured else zeros((0, len(CEN)))
        self.margin, self.n, self.P = margin, len(measured), profs
        self.med, self.lo, self.hi = _envelope_of(profs)
        self.t40, self.S, self.len_s = array(cols[1]), list(cols[2]), array(cols[3])
        self.ref_dist = array([]) if len(measured) < 3 else _loo_distances(profs)

    @property
    def spread(self) -> tuple[float, float] | None:
        dists = self.ref_dist
        return (float(median(dists)), float(np.max(dists))) if len(dists) else None

    def check(self, x: Any, exclude: Any = None) -> dict:
        """Compare x with the kept references: balance, outside bands, decay, shares, length, words."""
        kept = _kept(self.n, exclude)
        if len(kept) == 0:
            return {}
        sig = as_mono(x)
        report = {**self._spectral_terms(profile(sig), kept), **self._temporal_terms(sig, kept)}
        report["words"] = ToneRef.words(report)
        return report

    def _spectral_terms(self, prof: ndarray, kept: list[int]) -> dict:
        kept_profs = self.P[kept]
        offset = prof - median(kept_profs, axis=0)
        ref_gaps = self.ref_dist[kept] if len(self.ref_dist) else array([])
        over_ratio = None if len(ref_gaps) < 2 else _rms_of(offset) / max(float(median(ref_gaps)), 0.5)
        return {"tone_db": round(_rms_of(offset), 2),
                "tone_ratio": over_ratio if over_ratio is None else round(over_ratio, 2),
                "tone_ref": [_fround(gap_v, 2) for gap_v in ref_gaps],
                "band_dev": [_fround(dev_v, 1) for dev_v in offset],
                "outside": _bands_out_of_range(prof, offset, kept_profs.min(axis=0), kept_profs.max(axis=0), self.margin)}

    def _temporal_terms(self, sig: ndarray, kept: list[int]) -> dict:
        kept_t40, kept_len = self.t40[kept], self.len_s[kept]
        ref_med_sh = {cell_name: _fround(median([self.S[row_i][cell_name] for row_i in kept]), 1) for cell_name in REGIONS}
        return {"t40_ms": t40_ms(sig), "t40_ref": (float(kept_t40.min()), float(kept_t40.max())),
                "length_s": round(len(sig) / SR, 3), "shares": shares(sig), "shares_ref_med": ref_med_sh,
                "length_ref": (_fround(kept_len.min(), 3), _fround(kept_len.max(), 3))}

    @staticmethod
    def words(m: dict) -> str:
        """Summarise a tone check result in one sentence."""
        chunks = [f"tone {m['tone_db']:.1f} dB from the refs' median"]
        ref_gaps = m.get("tone_ref")
        if ref_gaps:
            chunks.append(f" (the refs sit {min(ref_gaps):.1f}-{max(ref_gaps):.1f} apart)")
        top3 = sorted(m.get("outside", []), key=lambda entry: -abs(entry[1]))[:3]
        if top3:
            chunks.append(": " + ", ".join(f"{lean_db:+.0f} dB at {band_lbl} ({where} every ref)" for band_lbl, lean_db, where in top3))
        chunks.append(_extra_notes(m))
        return "".join(chunks)

    @staticmethod
    def region_penalty(m: dict, regions: Sequence[str] = ("<150", "4-12k", ">12k"), margin: float = 6.0,
                       scale: float = 6.0) -> float:
        """Sum, over the named regions, of (share above the refs' median minus margin) / scale."""
        if not m:
            return 0.0
        own_sh, ref_sh = m["shares"], m["shares_ref_med"]
        return float(sum(max(0.0, own_sh[region] - ref_sh[region] - margin) / scale for region in regions))

    def penalty(self, m: dict) -> float:
        """How much the balance exceeds a normal ref-to-ref gap, plus a cost for a tail longer than every ref."""
        over_ratio = m.get("tone_ratio") if m else None
        if over_ratio is None:
            return 0.0
        cost = max(0.0, over_ratio - 1)
        ref_max, t40_now = m["t40_ref"][1], m["t40_ms"]
        if ref_max > 0 and t40_now > ref_max:
            cost += 0.5 * min(2.0, (t40_now - ref_max) / ref_max)
        return float(cost)


def _bands_out_of_range(prof: ndarray, offset: ndarray, lo_env: ndarray, hi_env: ndarray, margin: float) -> list:
    acc = []
    for idx, fc in enumerate(CEN):
        miss = max(prof[idx] - hi_env[idx] - margin, 0) - max(lo_env[idx] - prof[idx] - margin, 0)
        if miss != 0:
            acc.append((label(fc), _fround(offset[idx], 1), "above" if miss > 0 else "below"))
    return acc


def _decay_note(rep: dict) -> str:
    ref_lo, ref_hi = rep.get("t40_ref", (0, 0))
    pos = rep.get("t40_ms", 0.0)
    return f"; tail t40 {pos:.0f} ms (refs {ref_lo:.0f}-{ref_hi:.0f})" if pos > 1.5 * ref_hi + 10 or pos < 0.6 * ref_lo - 5 else ""


def _region_note(rep: dict) -> str:
    ref_sh = rep.get("shares_ref_med", {})
    ref_gaps = [(region, val - ref_sh[region]) for region, val in rep.get("shares", {}).items() if region in ref_sh and abs(val - ref_sh[region]) > 9]
    return "; energy share vs the refs: " + ", ".join(f"{region} {gap_db:+.0f} dB" for region, gap_db in ref_gaps) if ref_gaps else ""


def _duration_note(rep: dict) -> str:
    ref_short, ref_long = rep.get("length_ref", (0, 0))
    return f"; {rep['length_s']:.2f} s long (refs {ref_short:.2f}-{ref_long:.2f})" if ref_long and rep.get("length_s", 0) > 1.3 * ref_long else ""


def _extra_notes(rep: dict) -> str:
    return _decay_note(rep) + _region_note(rep) + _duration_note(rep)


# ---------------------------------------------------------------- bandgram

def _third_octave_bank() -> list:
    global _SOS_CACHE
    if _SOS_CACHE is None:
        _SOS_CACHE = [butter(3, [fc / EDGE, min(fc * EDGE, 0.49 * SR)], "bandpass", fs=SR, output="sos")
                 for fc in CEN]
    return _SOS_CACHE


def bandgram(x: Any, hop_ms: float = HOP_MS) -> ndarray:
    """Per-frame energy of the 28 bands, in dB below the loudest frame total, clipped at FLOOR."""
    sig = as_mono(x)
    hop = int(round(SR * hop_ms / 1000))
    nframes = max(1, math.ceil(len(sig) / hop))
    padded = zeros(nframes * hop)
    padded[: len(sig)] = sig
    energy = column_stack([mean(sosfilt(sos, padded).reshape(nframes, hop) ** 2, axis=1)
                           for sos in _third_octave_bank()])
    loudest = energy.sum(axis=1).max()
    return maximum(10 * log10(energy / (loudest + 1e-30) + 1e-30), FLOOR)


def _landmarks(gram: ndarray) -> dict:
    level = 10 * log10(np.sum(10 ** (gram / 10), axis=1) + 1e-30)
    loud_at = int(argmax(level))
    top = level[loud_at]
    alive = nonzero(level >= top - 50)[0]
    return {"peak": loud_at, "onset": int(argmax(level >= top - 20)), "lo": int(alive[0]), "hi": int(alive[-1])}


def _weights(cand: ndarray, ref_rows: ndarray) -> ndarray:
    """0..1 weight per cell: 0 at FLOOR, 1 when the louder of the two is 40 dB above it."""
    return clip((maximum(cand, ref_rows) - FLOOR) / 40, 0, 1)


def _frame_distance(cand: ndarray, ref_rows: ndarray, factor: float) -> float:
    wts = _weights(cand, ref_rows)
    err = minimum(np.abs(cand - ref_rows), 30)
    dist = float(np.sum(wts * err) / (np.sum(wts) + 1e-9))
    return dist * (1 + 0.5 * abs(math.log(factor)))


def _coarse_pick(judged: ndarray, target: ndarray, factors: Sequence[float]) -> tuple:
    """Anchor and stretch with the lowest distance: (cost, anchor, stretch, offsets, ref rows, start)."""
    marks_j, marks_t = _landmarks(judged), _landmarks(target)
    low, pick = None, None
    for mark in ("onset", "peak"):
        offs = _offset_window(marks_j, marks_t, mark)
        ref_rows = _sample_frames(target, marks_t[mark] + offs)
        for factor in factors:
            dist = _frame_distance(_sample_frames(judged, marks_j[mark] + offs / factor), ref_rows, factor)
            if low is None or dist < low:
                low, pick = dist, (mark, factor, offs, ref_rows, marks_j[mark] + offs / factor)
    return (low, *pick)


def _nudge_pick(judged: ndarray, ref_rows: ndarray, factor: float, start: ndarray, low: float,
                offsets: Sequence[int]) -> tuple[float, int]:
    """Best whole-frame shift of the coarse pick: (cost, shift); shift 0 keeps the coarse pick."""
    moved = 0
    for nudge in offsets:
        if nudge == 0:
            continue
        dist = _frame_distance(_sample_frames(judged, start + nudge), ref_rows, factor)
        if dist < low:
            low, moved = dist, nudge
    return low, moved


def _align_search(judged: ndarray, target: ndarray, factors: Sequence[float], offsets: Sequence[int]) -> dict:
    """Pick anchor and stretch with the lowest distance, then try whole-frame nudges of the winner."""
    low, mark, factor, offs, ref_rows, start = _coarse_pick(judged, target, factors)
    low, moved = _nudge_pick(judged, ref_rows, factor, start, low, offsets)
    return {"cost": low, "anchor": mark, "stretch": factor, "shift": moved, "offs": offs,
            "ref_rows": ref_rows, "cand": _sample_frames(judged, start + moved)}


def _cell_table(cand: ndarray, ref_rows: ndarray, offs: ndarray) -> dict:
    """Weighted mean dB difference and weight share for each (time segment, band group) cell."""
    wts = _weights(cand, ref_rows)
    whole = wts.sum() + 1e-9
    delta = cand - ref_rows
    table = {}
    for seg_name, t_from, t_to in SEGMENTS:
        in_seg = (offs >= t_from) & (offs < t_to)
        for grp_name, f_from, f_to in GROUPS:
            sel = ix_(in_seg, (CEN >= f_from) & (CEN < f_to))
            cell_w = wts[sel]
            mass = cell_w.sum()
            if mass < 0.01 * whole:
                continue
            table[f"{seg_name} {grp_name}"] = (_fround(np.sum(cell_w * delta[sel]) / mass, 1),
                                              _fround(mass / whole, 3))
    return table


def env_dist(A: ndarray, B: ndarray, stretch: Sequence[float] = STRETCH, shift: Sequence[int] = SHIFT,
             detail: bool = False) -> Any:
    """dB distance of bandgram A from reference bandgram B after the best time alignment."""
    found = _align_search(asarray(A), asarray(B), stretch, shift)
    if not detail:
        return found["cost"]
    info = {"anchor": found["anchor"], "stretch": found["stretch"], "shift_ms": found["shift"] * 10,
            "cells": _cell_table(found["cand"], found["ref_rows"], found["offs"])}
    return found["cost"], info


def _sample_frames(judged_gram: ndarray, pos: ndarray) -> ndarray:
    base = floor(pos).astype(int)
    frac = (pos - base)[:, None]

    def take(jdx: ndarray) -> ndarray:
        inside = (jdx >= 0) & (jdx < len(judged_gram))
        acc = full((len(jdx), judged_gram.shape[1]), FLOOR)
        acc[inside] = judged_gram[jdx[inside]]
        return acc

    return take(base) * (1 - frac) + take(base + 1) * frac


def _offset_window(marks_a: dict, marks_b: dict, mark: str) -> ndarray:
    anchor_a, anchor_b = marks_a[mark], marks_b[mark]
    start = max(-300, min(marks_a["lo"] - anchor_a, marks_b["lo"] - anchor_b)) - 3
    stop = min(600, max(marks_a["hi"] - anchor_a, marks_b["hi"] - anchor_b)) + 4
    return arange(start, stop)


# ---------------------------------------------------------------- EnvRef

def _ref_label(ref_sh: Any, idx: int) -> str:
    return str(ref_sh) if isinstance(ref_sh, (str, os.PathLike)) else f"ref{idx}"


def _shared_sign_gap(ref_gaps: list[float]) -> float:
    """The smallest offset when every diff has the same sign (negative side as -max), else 0."""
    if all(gap_db > 0 for gap_db in ref_gaps):
        return min(ref_gaps)
    return -max(ref_gaps) if all(gap_db < 0 for gap_db in ref_gaps) else 0.0


def _env_vs_spread(gap_db: float, ref_gaps: list[float]) -> float | None:
    return round(gap_db / max(float(median(ref_gaps)), 1.0), 2) if ref_gaps else None


def _pairwise(grams: list) -> tuple[ndarray, dict]:
    total_refs = len(grams)
    near_db = full((total_refs, total_refs), np.inf)
    details: dict = {}
    for src_i in range(total_refs):
        for dst_i in range(total_refs):
            if src_i == dst_i:
                continue
            near_db[src_i, dst_i], details[(src_i, dst_i)] = env_dist(grams[src_i], grams[dst_i], detail=True)
    return near_db, details


def _median_abs_cells(details: Any) -> dict:
    by_cell: dict[str, list[float]] = {}
    for pair_det in details:
        for cell_name, (lean, _) in pair_det["cells"].items():
            by_cell.setdefault(cell_name, []).append(abs(lean))
    return {cell_name: float(median(dev_v)) for cell_name, dev_v in by_cell.items() if len(dev_v) >= 2}


def _cells_in_all(pair_dets: list[dict]) -> list[str]:
    """Cell names of the first comparison that every comparison has."""
    return [name for name in pair_dets[0]["cells"] if all(name in det["cells"] for det in pair_dets)]


def _rank_hits(found: list[tuple]) -> tuple[list, float]:
    """Outside entries sorted by weighted size, and the summed excess in input order."""
    surplus = 0.0
    for _, extra in found:
        surplus += extra
    return sorted((hit[0] for hit in found), key=lambda entry: -abs(entry[1]) * entry[2]), surplus


class EnvRef:
    """Bandgrams of a reference set, their pairwise envelope distances and typical cell gaps."""

    def __init__(self, refs: Sequence[Any], names: Sequence[str] | None = None, margin: float = OUT_MARGIN):
        ref_list = list(refs)
        grams = [bandgram(ref_in) for ref_in in ref_list]
        if names is None:
            names = [_ref_label(ref_in, row_i) for row_i, ref_in in enumerate(ref_list)]
        dists, pair_info = _pairwise(grams)
        self.names, self.margin, self.G, self.n = list(names), margin, grams, len(grams)
        self.D, self._pair_info = dists, pair_info
        self.cell_spread = _median_abs_cells(pair_info.values())
        self.out_ref = self._ref_outside_scores()

    def _ref_outside_scores(self) -> ndarray:
        if self.n < 2:
            return array([])
        per_ref = []
        for row_i in range(self.n):
            pair_rows = [self._pair_info[(row_i, col_j)] for col_j in range(self.n) if col_j != row_i]
            per_ref.append(self._beyond_all(pair_rows)[1])
        return array(per_ref)

    def _cell_overshoot(self, cell_name: str, pair_dets: list[dict]) -> tuple | None:
        """For a cell where every comparison leans the same way past the tolerance: (entry, weighted excess)."""
        leans = [dists["cells"][cell_name][0] for dists in pair_dets]
        avg_share = float(mean([dists["cells"][cell_name][1] for dists in pair_dets]))
        lean = _shared_sign_gap(leans)
        tolerance = max(self.margin, self.cell_spread.get(cell_name, self.margin))
        if not (lean > tolerance and avg_share >= 0.02):
            return None
        row = (cell_name, round(math.copysign(lean, leans[0]), 1), round(avg_share, 3))
        return row, (lean - tolerance) * min(1.0, avg_share / 0.05)

    def _beyond_all(self, pair_dets: list[dict]) -> tuple[list, float]:
        if len(pair_dets) < 2:
            return [], 0.0
        hits = [self._cell_overshoot(name, pair_dets) for name in _cells_in_all(pair_dets)]
        return _rank_hits([hit for hit in hits if hit is not None])

    def spread(self, exclude: Any = None) -> list[float]:
        """For each kept ref, the distance to its closest kept neighbour; empty below 3 kept refs."""
        kept = _kept(self.n, exclude)
        if len(kept) < 3:
            return []
        return [float(min(self.D[row_i, col_j] for col_j in kept if col_j != row_i)) for row_i in kept]

    def check(self, x: Any, exclude: Any = None, G: ndarray | None = None) -> dict:
        """Compare x with the kept references: nearest distance, cells beyond every ref, words."""
        kept = _kept(self.n, exclude)
        if not kept:
            return {}
        judged_gram = G if G is not None else bandgram(x)
        trials = [env_dist(judged_gram, self.G[row_i], detail=True) for row_i in kept]
        report = self._envelope_terms(trials, kept, exclude)
        report["words"] = EnvRef.words(report)
        return report

    def _median_ref_outside(self, kept: list[int]) -> float | None:
        if self.n < 3:
            return None
        picked = self.out_ref[kept]
        return _fround(median(picked), 2) if len(picked) else None

    def _envelope_terms(self, trials: list, kept: list[int], exclude: Any) -> dict:
        best_k = int(argmin([run[0] for run in trials]))
        near_db, pair_det = trials[best_k]
        ref_gaps = self.spread(exclude)
        outside, surplus = self._beyond_all([run[1] for run in trials])
        winner = kept[best_k]
        return {"env_db": round(near_db, 2), "env_ratio": _env_vs_spread(near_db, ref_gaps),
                "env_ref": [round(gap_v, 2) for gap_v in ref_gaps],
                "nearest": os.path.basename(self.names[winner].replace("\\", "/")), "nearest_i": winner,
                **{region: pair_det[region] for region in ("anchor", "stretch", "shift_ms", "cells")},
                "outside": outside, "env_out": round(surplus, 2), "env_out_ref": self._median_ref_outside(kept)}

    @staticmethod
    def words(m: dict) -> str:
        """Summarise an envelope check result in one sentence."""
        line = f"envelope {m['env_db']:.1f} dB from the nearest ref {m['nearest']}"
        ref_gaps = m.get("env_ref")
        if ref_gaps:
            line += f" (the refs sit {min(ref_gaps):.1f}-{max(ref_gaps):.1f} apart)"
        if m.get("outside"):
            shown = ", ".join(f"{cell_lbl} {lean_db:+.0f} dB" for cell_lbl, lean_db, _ in m["outside"][:3])
            return f"{line}; beyond every ref: {shown}"
        loud = [(cell_lbl, lean_db, wshare) for cell_lbl, (lean_db, wshare) in m.get("cells", {}).items() if abs(lean_db) >= 8]
        loud.sort(key=lambda entry: -abs(entry[1]) * math.sqrt(entry[2]))
        if loud:
            line += ": vs it " + ", ".join(f"{cell_lbl} {lean_db:+.0f} dB" for cell_lbl, lean_db, _ in loud[:2])
        return line

    @staticmethod
    def penalty(m: dict, cap: float = 2.0) -> float:
        """Excess over the typical ref spread plus excess of cells no ref shares, capped at cap."""
        if not m:
            return 0.0
        over_ratio = m.get("env_ratio")
        over_typ = 0.0 if over_ratio is None else max(0.0, over_ratio - 1)
        tolerance = max(3.0, m.get("env_out_ref") or 0.0)
        unshared = max(0.0, m.get("env_out", 0.0) - tolerance) / 6
        return float(min(cap, over_typ + unshared))
