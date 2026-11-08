"""Tone measures pinned at the first benchmark run.

The benchmark reads its tone numbers from here, not from the live tone module, so later work on that module cannot
shift old results. Leave this file alone: any edit changes every benchmark figure. A new measure belongs in bench.py,
with a line in the benchmark change log.
"""

from __future__ import annotations

import os
from collections.abc import Sequence
from typing import Any

from numpy import abs as np_abs
from numpy import argmax as peak_at
from numpy import (
    array,
    asarray,
    delete,
    flatnonzero,
    log10,
    maximum,
    median,
    sqrt,
    zeros,
)
from numpy import mean as average
from numpy.fft import rfft, rfftfreq

SR = 48000
CEN = [1000 * 2 ** (step / 3) for step in range(-15, 13)]
REGIONS = {"<150": (0, 150), "150-1k": (150, 1000), "1-4k": (1000, 4000), "4-12k": (4000, 12000),
           ">12k": (12000, 24000)}

_HALF_BAND = 2 ** (1 / 6)
_TINY = 1e-20


def _flatten_channels(sound: Any):
    """Load a path if needed, then average the channels of a 2-D array."""
    if isinstance(sound, (str, os.PathLike)):
        from hark.core.audio import load

        sound = load(str(sound), sr=SR)
    samples = asarray(sound, dtype=float)
    if samples.ndim != 2:
        return samples
    return samples.mean(axis=1)


def _spectrum(signal_1d):
    """Power per FFT bin and the bin frequencies."""
    return np_abs(rfft(signal_1d)) ** 2, rfftfreq(len(signal_1d), 1 / SR)


def _root_mean_square(values) -> float:
    return float(sqrt(average(asarray(values) ** 2)))


def label(c: float) -> str:
    """Band name: three significant kHz digits from 1 kHz up, whole hertz below."""
    if c < 1000:
        return "%.0f Hz" % c
    return "%.3g kHz" % (c / 1000)


def profile(x: Any) -> "Any":
    """Per-band level in dB relative to the sum of all 28 bands, clipped at -60 dB."""
    power, freqs = _spectrum(_flatten_channels(x))
    sums = []
    for centre in CEN:
        inside = (freqs >= centre / _HALF_BAND) & (freqs < centre * _HALF_BAND)
        sums.append(power[inside].sum() + _TINY)
    per_band = array(sums)
    return maximum(10 * log10(per_band / per_band.sum()), -60.0)


def shares(x: Any) -> dict:
    """Fraction of the energy in each named region, as dB rounded to 0.1."""
    power, freqs = _spectrum(_flatten_channels(x))
    whole = power.sum()
    result = {}
    for region, edges in REGIONS.items():
        bottom, top = edges
        upper_ok = (freqs <= top) if top >= 24000 else (freqs < top)
        picked = power[(freqs >= bottom) & upper_ok].sum()
        result[region] = round(float(10 * log10(picked / (whole + _TINY) + 1e-12)), 1)
    return result


def t40_ms(x: Any, db: float = 40, ms: float = 5) -> float:
    """Milliseconds from the peak frame until the level first falls more than `db` below it."""
    samples = _flatten_channels(x)
    width = int(SR * ms / 1000)
    frames = len(samples) // width
    if frames < 2:
        return 0.0
    blocks = samples[: frames * width].reshape(frames, width)
    level = 10 * log10(average(blocks ** 2, axis=1) + _TINY)
    top = int(peak_at(level))
    quiet = flatnonzero(level[top:] < level[top] - db)
    steps = quiet[0] if len(quiet) else frames - top
    return float(steps * ms)


def _profiles_of(signals: list) -> "Any":
    if not signals:
        return zeros((0, len(CEN)))
    return array([profile(one) for one in signals])


def _leave_one_out(table) -> list[float]:
    count = len(table)
    if count < 3:
        return []
    return [_root_mean_square(table[row] - median(delete(table, row, 0), axis=0)) for row in range(count)]


def _relative_distance(distance: float, spreads: list[float]) -> float | None:
    if len(spreads) < 2:
        return None
    return round(distance / max(float(median(spreads)), 0.5), 2)


def _sentence_bands(result: dict) -> str:
    worst = sorted(result["outside"], key=lambda item: -abs(item[1]))[:3]
    if not worst:
        return ""
    parts = ["%+.0f dB at %s (%s every ref)" % (gap, band, side) for band, gap, side in worst]
    return ": " + ", ".join(parts)


def _sentence_tail(result: dict) -> str:
    shortest, longest = result["t40_ref"]
    tail = result["t40_ms"]
    if tail > 1.5 * longest + 10 or tail < 0.6 * shortest - 5:
        return "; tail t40 %.0f ms (refs %.0f-%.0f)" % (tail, shortest, longest)
    return ""


def _sentence_shares(result: dict) -> str:
    notes = []
    for region, ref_level in result["shares_ref_med"].items():
        gap = result["shares"][region] - ref_level
        if abs(gap) > 9:
            notes.append("%s %+.0f dB" % (region, gap))
    if not notes:
        return ""
    return "; energy share vs the refs: " + ", ".join(notes)


def _sentence_length(result: dict) -> str:
    short_ref, long_ref = result["length_ref"]
    if result["length_s"] > 1.3 * long_ref:
        return "; %.2f s long (refs %s-%s)" % (result["length_s"], short_ref, long_ref)
    return ""


def _per_signal_facts(signals: list) -> tuple[list, list, list]:
    """Tail time, region shares and duration in seconds of each signal."""
    tails, splits, seconds = [], [], []
    for one in signals:
        tails.append(t40_ms(one))
        splits.append(shares(one))
        seconds.append(len(one) / SR)
    return tails, splits, seconds


def _band_facts(offset, flagged: list, summary: dict) -> dict:
    """Distance, ratio and per-band facts; the ref summary rides along under `_summary`."""
    distance = _root_mean_square(offset)
    spreads = summary["spreads"]
    return {"tone_db": round(distance, 2), "tone_ratio": _relative_distance(distance, spreads),
            "tone_ref": [round(one, 2) for one in spreads], "band_dev": [round(float(gap), 1) for gap in offset],
            "outside": flagged, "_summary": summary}


def _sound_facts(samples, summary: dict) -> dict:
    return {"t40_ms": t40_ms(samples), "t40_ref": summary["tails"], "length_s": round(len(samples) / SR, 3),
            "shares": shares(samples), "shares_ref_med": summary["typical"], "length_ref": summary["lengths"]}


class ToneRef:
    """Band statistics of a set of reference sounds, used to judge how far a new sound sits from them."""

    def __init__(self, refs: Sequence[Any], margin: float = 3.0):
        signals = [_flatten_channels(item) for item in refs]
        table = _profiles_of(signals)
        self.margin = margin
        self.P, self.ref_dist = table, _leave_one_out(table)
        self.t40, self.shares, self.length = _per_signal_facts(signals)
        if len(table):
            self.med, self.lo, self.hi = median(table, axis=0), table.min(axis=0), table.max(axis=0)

    @property
    def spread(self) -> tuple[float, float] | None:
        """Median and largest leave-one-out distance; None when there are under 3 refs."""
        gaps = self.ref_dist
        return (float(median(gaps)), float(max(gaps))) if gaps else None

    def _flagged_bands(self, level, offset, floor, ceiling) -> list:
        excess = maximum(level - ceiling - self.margin, 0) - maximum(floor - level - self.margin, 0)
        flagged = []
        for centre, gap, amount in zip(CEN, offset, excess):
            if amount != 0:
                flagged.append((label(centre), round(float(gap), 1), "above" if amount > 0 else "below"))
        return flagged

    def _summary_of(self, chosen: list[int]) -> dict:
        lengths = [self.length[idx] for idx in chosen]
        tails = [self.t40[idx] for idx in chosen]
        typical = {}
        for region in REGIONS:
            typical[region] = round(float(median([self.shares[idx][region] for idx in chosen])), 1)
        return {
            "spreads": [self.ref_dist[idx] for idx in chosen] if self.ref_dist else [],
            "tails": (min(tails), max(tails)),
            "typical": typical,
            "lengths": (round(min(lengths), 3), round(max(lengths), 3)),
        }

    def _measure(self, x: Any, chosen: list[int]) -> dict:
        samples = _flatten_channels(x)
        level = profile(samples)
        subset = self.P[chosen]
        offset = level - median(subset, axis=0)
        flagged = self._flagged_bands(level, offset, subset.min(axis=0), subset.max(axis=0))
        result = _band_facts(offset, flagged, self._summary_of(chosen))
        result.update(_sound_facts(samples, result.pop("_summary")))
        result["words"] = self.words(result)
        return result

    def check(self, x: Any, exclude: int | None = None) -> dict:
        """Compare x with every ref except `exclude`; empty dict when there are no refs."""
        count = len(self.P)
        return self._measure(x, [idx for idx in range(count) if idx != exclude]) if count else {}

    def words(self, m: dict) -> str:
        """Plain-language summary of a check result."""
        report = m
        pieces = ["tone %.1f dB from the refs' median" % report["tone_db"]]
        spreads = report["tone_ref"]
        if spreads:
            pieces.append(" (the refs sit %s-%s apart)" % (min(spreads), max(spreads)))
        pieces.extend(make(report) for make in (_sentence_bands, _sentence_tail, _sentence_shares, _sentence_length))
        return "".join(pieces)

    def region_penalty(self, m: dict, regions: Sequence[str] = ("<150", "4-12k", ">12k"), margin: float = 6,
                       scale: float = 6) -> float:
        """Cost for regions whose share sits well above the refs' typical share."""
        report = m
        if not report:
            return 0.0
        own, typical = report["shares"], report["shares_ref_med"]
        return float(sum(max(0.0, own[name] - typical[name] - margin) / scale for name in regions))

    def penalty(self, m: dict) -> float:
        """How far the tone ratio exceeds 1, plus a cost for a tail longer than any ref."""
        report = m
        ratio = report.get("tone_ratio") if report else None
        if ratio is None:
            return 0.0
        cost = max(0.0, ratio - 1)
        longest, tail = report["t40_ref"][1], report["t40_ms"]
        if longest > 0 and tail > longest:
            cost += 0.5 * min(2.0, (tail - longest) / longest)
        return float(cost)
