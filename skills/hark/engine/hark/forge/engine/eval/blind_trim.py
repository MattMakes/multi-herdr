"""Blind-test audio helpers: measurements, the common trim of a pair, loudness matching and the clean-cut checks.

The real and the made clip of a pair are trimmed by one rule and played equally loud at their loudest point, and
neither clip lets room noise through before or after the sound. Envelope levels are in dB below the clip's own
loudest frame.
"""

from __future__ import annotations

import functools
import json
import math
import os
from pathlib import Path
from typing import Any

import numpy as np

SR = 48000

# Playback layout of a trial, in seconds.
LEAD = 1.0
GAP = 1.2
BETWEEN = 2.5
TAIL = 1.0
COMPAT = 1.3

# Trim levels and the margins added to them.
END_DB = -50.0
ON_DB = -50.0
HEAD_MARGIN = 1.0
REACH_MARGIN = 3.0
FLOOR_MARGIN = 10.0
LEVEL_CAP = -6.0

# Fades.
FADE_MIN_S = 0.005
FADE_DB = 6.0
FADE_IN_S = 0.004
MADE_FADE_GUARD = 0.01
REF_FADE_DEFAULT = 0.03

ENV = {"short": (0.0025, 3), "long": (0.005, 6)}
CLEAN_FLOOR = -50.0
CLEAN_REACH = -30.0
CLEAN_HEAD = -30.0
SHORT_S = 1.0

# Loudness match.
WIN = {"short": 0.1, "long": 0.4}
TARGET = {"short": -15.0, "long": -16.0}
TP_MAX = -1.0
FAIL = {"loud_lu": 0.5, "true_peak": TP_MAX + 0.05, "floor_margin": 6.0, "preroll_ms": 5.0, "edge_db": 10.0,
        "edge_loud": -20.0, "exposed_ms": 100.0, "exposed_floor": -70.0}
WARN = {"length_ratio": 1.6, "tp_diff": 6.0, "other_lu": 2.0, "floor_dbfs": -60.0, "mono": 0.995, "wide": 0.9,
        "exposed_ms": 30.0}
MANIFEST: Path | None = None  # path of the refset manifest; blind_v2 sets it

_MS = int(0.001 * SR)


def _channels(x: Any) -> np.ndarray:
    """2-D float view: one row per sample."""
    return np.asarray(x, dtype=float).reshape(len(x), -1)


def _to_db(power: Any, guard: float = 1e-20) -> Any:
    return 10 * np.log10(power + guard)


# ---------------------------------------------------------------- reading files

def load_audio(path: Any, start: float | None = None, stop: float | None = None) -> np.ndarray:
    """Two float64 channels at 48 kHz (a mono file is duplicated, extra channels dropped), optionally [start, stop)."""
    import soundfile as sf
    from scipy.signal import resample_poly

    native = sf.info(str(path)).samplerate
    first = int(round((start or 0.0) * native))
    last = None if stop is None else int(round(stop * native))
    data, rate = sf.read(str(path), dtype="float64", always_2d=True, start=first, stop=last)
    if rate != SR:
        common = math.gcd(rate, SR)
        data = resample_poly(data, SR // common, rate // common, axis=0)
    if data.shape[1] != 1:
        return data[:, :2]
    return np.column_stack([data[:, 0], data[:, 0]])


# ---------------------------------------------------------------- measurements

def frames_db(x: np.ndarray, ms: float) -> np.ndarray:
    """dB of the channel-averaged power in each complete frame; a clip shorter than one frame gives one value."""
    per_sample = np.mean(_channels(x) ** 2, axis=1)
    hop = max(1, int(round(SR * ms / 1000)))
    whole = len(per_sample) // hop
    if whole < 1:
        return np.array([_to_db(float(np.mean(per_sample)))])
    return _to_db(per_sample[: whole * hop].reshape(whole, hop).mean(axis=1))


def onset(x: np.ndarray, below: float = 40) -> int:
    """Start sample of the first 1 ms frame no more than `below` dB under the peak frame."""
    levels = frames_db(x, 1)
    return int(np.argmax(levels >= levels.max() - below)) * _MS


def end_of(x: np.ndarray, below: float = 50) -> int:
    """Sample just after the last 5 ms frame no more than `below` dB under the peak frame."""
    levels = frames_db(x, 5)
    final = int(np.flatnonzero(levels >= levels.max() - below)[-1])
    return min(len(x), (final + 1) * int(0.005 * SR))


def extent_s(x: np.ndarray) -> float:
    """Seconds between the onset and the end of the audible part."""
    return max(0.0, (end_of(x) - onset(x)) / SR)


def raised(n: int) -> np.ndarray:
    """Raised-cosine rise from 0 towards 1 in n steps."""
    return 0.5 - 0.5 * np.cos(np.pi * np.arange(n) / max(1, n))


def power_frames(x: np.ndarray, n: int, smooth: int = 1) -> np.ndarray:
    """Mean of the channel-summed power over n-sample frames (zero-padded at the end), with optional smoothing."""
    summed = np.sum(_channels(x) ** 2, axis=1)
    count = max(1, math.ceil(len(summed) / n))
    framed = np.pad(summed, (0, count * n - len(summed))).reshape(count, n).mean(axis=1)
    if smooth <= 1 or count <= 1:
        return framed
    return np.convolve(framed, np.ones(smooth) / smooth, mode="same")


def loud_max(x: np.ndarray, win: float) -> float:
    """K-weighted loudness of the loudest win-second span, tested at every sample offset."""
    from hark.core.features import k_weight

    margin = np.zeros((int(0.05 * SR), 2))
    stereo = _channels(x)[:, :2] * np.ones((1, 2))
    padded = np.concatenate([margin, stereo, margin])
    span = int(round(win * SR))
    if len(padded) < span:
        padded = np.pad(padded, ((0, span - len(padded)), (0, 0)))
    energy = np.sum(np.asarray(k_weight(padded, SR)) ** 2, axis=1)
    running = np.concatenate([[0.0], np.cumsum(energy)])
    best = np.max((running[span:] - running[:-span]) / span)
    return float(-0.691 + _to_db(best))


def meter_m400(x: np.ndarray) -> float:
    """Peak of the momentary loudness curve (400 ms window, 10 ms step), used as a second opinion."""
    from hark.core.features import loudness_curve

    shortfall = max(0, int(0.4 * SR) - len(x))
    padded = np.pad(x, ((0, shortfall), (0, 0)))
    return float(np.max(loudness_curve(padded, win=0.4, hop=0.01)[1]))


def true_peak(x: np.ndarray) -> float:
    """dBTP from 4x oversampling, never lower than the sample peak."""
    from hark.core.features import true_peak_dbtp

    return max(float(true_peak_dbtp(x)), sample_peak(x))


def sample_peak(x: np.ndarray) -> float:
    if not len(x):
        return -200.0
    return float(20 * np.log10(np.max(np.abs(x)) + 1e-20))


def rms_dbfs(x: np.ndarray) -> float:
    if not len(x):
        return -200.0
    return float(_to_db(np.mean(np.asarray(x) ** 2)))


def stereo_corr(x: np.ndarray) -> float:
    left, right = x[:, 0], x[:, 1]
    scale = math.sqrt(float(np.sum(left ** 2) * np.sum(right ** 2)) + 1e-30)
    return float(np.sum(left * right) / scale)


# ---------------------------------------------------------------- per-file facts

def _path_key(p: Any) -> str:
    return os.path.normcase(os.path.normpath(str(p).replace("\\", "/")))


@functools.lru_cache(maxsize=1)
def _manifest_index(path: str) -> dict:
    if not path or not os.path.isfile(path):
        return {}
    by_key = {}
    for category in json.loads(Path(path).read_text()).values():
        for split in ("forge", "bench"):
            for record in category.get(split) or []:
                by_key[_path_key(record["file"])] = record
    return by_key


def manifest_entry(p: Any) -> dict | None:
    """Manifest record of a reference file; None for a made final, which has no record."""
    return _manifest_index(str(MANIFEST or "")).get(_path_key(p))


@functools.lru_cache(maxsize=None)
def source_floor(p: str) -> float | None:
    """Noise-floor power of a bench ref, measured in its source recording around the trimmed section."""
    record = manifest_entry(p)
    if not record or not record.get("source_path") or not os.path.isfile(record["source_path"]):
        return None
    begin, finish = record["trim_s"]
    around = load_audio(record["source_path"], max(0.0, begin - 20), finish + 20)
    blocks = power_frames(around, int(0.05 * SR)) / 2
    blocks = blocks[blocks > 2e-10]
    if not len(blocks):
        return None
    gain = 10 ** (float(record.get("gain_db") or 0) / 10)
    return float(np.percentile(blocks, 5) * gain)


def end_fade_s(p: Any) -> float:
    record = manifest_entry(p)
    if record is None:
        return MADE_FADE_GUARD
    return float(record.get("fade_out_s") or REF_FADE_DEFAULT)


def _head_level(fine: np.ndarray, cut_file: bool) -> float | None:
    """Loudest of 1 ms frames 2-4 (or of all frames in a very short clip); None for an uncut file."""
    if not cut_file:
        return None
    return float(np.max(fine[2:5] if len(fine) > 5 else fine))


def _envelopes(x: np.ndarray, cls: str) -> tuple[np.ndarray, np.ndarray, float, int]:
    """(coarse envelope, 1 ms envelope, top level in dB, coarse hop in samples), levels relative to the top."""
    hop_s, smooth = ENV[cls]
    hop = int(round(hop_s * SR))
    coarse_power = power_frames(x, hop, smooth)
    top = float(_to_db(coarse_power.max()))
    return _to_db(coarse_power) - top, _to_db(power_frames(x, _MS)) - top, top, hop


def _reach_level(env: np.ndarray, peak: int, stop: int, hop_s: float) -> float:
    """Lowest level in the last 15 ms before the fade region, never earlier than the peak."""
    window = max(1, math.ceil(0.015 / hop_s))
    return float(np.min(env[max(peak, stop - window):stop]))


def analyse(x: np.ndarray, cls: str, fade_s: float, floor_pow: float | None = None, cut_file: bool = True) -> dict:
    """Envelope facts the trim needs: reach, head, peak_late, strike and floor."""
    hop_s = ENV[cls][0]
    env, fine, top, hop = _envelopes(x, cls)
    peak = int(np.argmax(env))
    stop = max(peak + 1, len(env) - math.ceil((fade_s + 0.005) / hop_s))
    reach = _reach_level(env, peak, stop, hop_s)
    floor = None if floor_pow is None else float(10 * np.log10(floor_pow + 1e-30) - top)
    return {"e": env, "e1": fine, "top": top, "ip": peak, "stop": stop, "hop": hop, "reach": reach,
            "head": _head_level(fine, cut_file), "peak_late": peak * hop >= 0.85 * len(x),
            "strike": float(np.max(fine[:15])) >= -6, "floor": floor, "fade_s": fade_s}


def _cls(*extents: float) -> str:
    return "short" if max(extents) < SHORT_S else "long"


def _trims_cleanly(info: dict, x: np.ndarray) -> bool:
    head = info["head"]
    if not (head is None or head <= CLEAN_HEAD or info["strike"]):
        return False
    if not (info["reach"] <= CLEAN_REACH or info["peak_late"]):
        return False
    if info["floor"] is not None and info["floor"] > CLEAN_FLOOR:
        return False
    return stereo_corr(x) > -0.5


@functools.lru_cache(maxsize=None)
def facts(p: str, is_ref: bool) -> tuple[float, bool]:
    """(audible extent, whether it trims cleanly) of one file."""
    audio = load_audio(p)
    extent = extent_s(audio)
    if is_ref:
        fade, floor = end_fade_s(p), source_floor(p)
    else:
        fade, floor = MADE_FADE_GUARD, None
    info = analyse(audio, _cls(extent), fade, floor, cut_file=is_ref)
    return extent, bool(_trims_cleanly(info, audio))


# ---------------------------------------------------------------- trimming a pair

def pair_levels(a: dict, b: dict) -> tuple[float, float]:
    """(L_on, L_end): levels both clips reach on their own, kept above any floor and capped at -6 dB."""
    both = (a, b)
    floor_terms = [side["floor"] + FLOOR_MARGIN for side in both if side.get("floor") is not None]
    end_level = max([END_DB] + [side["reach"] + REACH_MARGIN for side in both] + floor_terms)
    head_terms = [side["head"] + HEAD_MARGIN for side in both if side.get("head") is not None]
    on_level = max([ON_DB] + head_terms + floor_terms)
    return min(LEVEL_CAP, on_level), min(LEVEL_CAP, end_level)


def _last_index(mask: np.ndarray, fallback: int) -> int:
    hits = np.flatnonzero(mask)
    return int(hits[-1]) if len(hits) else fallback


def _cut_points(info: dict, total: int, L_on: float, L_end: float) -> tuple[int, int, int, int]:
    """(onset sample, last kept frame, end sample, fade-out start sample)."""
    hop, env = info["hop"], info["e"]
    above = np.flatnonzero(info["e1"] >= L_on)
    begin = int(above[0]) * _MS if len(above) else 0
    last = _last_index(env[: info["stop"]] >= L_end, info["ip"])
    finish = min(total, (last + 1) * hop)
    fade_frame = _last_index(env[: last + 1] >= L_end + FADE_DB, info["ip"])
    fade_from = min(max((fade_frame + 1) * hop, begin), finish - int(FADE_MIN_S * SR))
    return begin, last, finish, fade_from


def _edge_levels(info: dict, start: int, begin: int, last: int, fade_from: int) -> dict:
    hop, env = info["hop"], info["e"]
    lo = max(0, start)
    half_fade = start + int(FADE_IN_S * SR) // 2
    if half_fade > lo:
        first = lo // _MS
        level_in = float(np.max(info["e1"][first: max(first + 1, math.ceil(half_fade / _MS))]))
    else:
        level_in = -200.0
    beyond = env[last + 1: last + 1 + math.ceil(0.005 * SR / hop)]
    level_out = float(np.max(beyond)) if len(beyond) else float(env[last])
    heard = None
    if info.get("floor") is not None:
        inside = env[begin // hop: fade_from // hop]
        heard = float(np.sum(inside < info["floor"] + 6) * hop / SR * 1000)
    return {"cut_in_db": round(level_in, 2), "cut_out_db": round(level_out, 2), "floor_exposed_ms": heard}


def _apply_fades(x: np.ndarray, start: int, finish: int, fade_from: int) -> tuple[np.ndarray, int]:
    piece = np.array(x[max(0, start):finish], dtype=float)
    if start < 0:
        piece = np.concatenate([np.zeros((-start, piece.shape[1])), piece])
    rise = int(FADE_IN_S * SR)
    piece[:rise] *= raised(min(rise, len(piece)))[:, None][: len(piece[:rise])]
    tail = finish - fade_from
    if tail > 0:
        piece[-tail:] *= raised(tail)[::-1][:, None]
    return piece, tail


def _maybe_round(value: float | None) -> float | None:
    return value if value is None else round(value, 2)


def cut(x: np.ndarray, info: dict, L_on: float, L_end: float, keep_end: bool) -> tuple[np.ndarray, dict]:
    """Trim one clip: 4 ms raised-cosine fade-in, fade-out across its final 6 dB. Returns (clip, facts)."""
    begin, last, finish, fade_from = _cut_points(info, len(x), L_on, L_end)
    start = begin - int(FADE_IN_S * SR)
    kept = bool(keep_end and info["e"][info["stop"] - 1] >= L_end)
    if kept:
        finish, last = len(x), len(info["e"]) - 1
        fade_from = finish - int(0.002 * SR)
    piece, tail = _apply_fades(x, start, finish, fade_from)
    report = {"onset_s": round(begin / SR, 4), "start_s": round(start / SR, 4), "end_s": round(finish / SR, 4),
              "fade_out_ms": round(tail / SR * 1000, 2)}
    report.update(_edge_levels(info, start, begin, last, fade_from))
    report.update(reach_db=round(info["reach"], 2), head_db=_maybe_round(info["head"]),
                  floor_db=_maybe_round(info["floor"]), kept_file_end=kept, end_fade_s=info["fade_s"],
                  top_db=round(info["top"], 2))
    return piece, report


# ---------------------------------------------------------------- loudness match and checks

def match_pair(real: np.ndarray, made: np.ndarray, cls: str) -> tuple[float, float, float]:
    """(made gain, shared gain, peak-cap reduction) in dB: equal loudest spans, true peaks at most -1 dBTP."""
    span = WIN[cls]
    real_loud = loud_max(real, span)
    made_gain = real_loud - loud_max(made, span)
    shared = TARGET[cls] - real_loud
    peak = max(true_peak(real), true_peak(made) + made_gain) + shared
    excess = max(0.0, peak - TP_MAX)
    return made_gain, shared - excess, excess


def measure_clip(x: np.ndarray, cls: str, onset_i: int) -> dict:
    """Level facts of one clip as the listener hears it."""
    alt = "short" if cls == "long" else "long"
    return {"loud_pair_measure": loud_max(x, WIN[cls]), "loud_other_window": loud_max(x, WIN[alt]),
            "meter_m400": meter_m400(x), "true_peak_dbtp": true_peak(x), "sample_peak_dbfs": sample_peak(x),
            "preroll_dbfs": rms_dbfs(x[:onset_i]), "length_s": round(len(x) / SR, 4), "stereo_corr": stereo_corr(x)}


def _gap(m: tuple[dict, dict], key: str) -> float:
    return abs(m[0][key] - m[1][key])


def _pair_numbers(m: tuple[dict, dict], f: tuple[dict, dict], lv: tuple[float, float], gc: float) -> dict:
    ref_facts = f[0]
    floor = ref_facts["floor_db"]
    durations = sorted(clip["length_s"] for clip in m)
    played = None if floor is None else floor + ref_facts["top_db"] + gc - 10 * math.log10(2)
    return {"loud_diff": _gap(m, "loud_pair_measure"), "other_diff": _gap(m, "loud_other_window"),
            "meter_diff": _gap(m, "meter_m400"), "tp_diff": _gap(m, "true_peak_dbtp"),
            "tp_max": max(clip["true_peak_dbtp"] for clip in m),
            "preroll_max_dbfs": max(clip["preroll_dbfs"] for clip in m),
            "preroll_ms": max((side["onset_s"] - side["start_s"]) * 1000 for side in f),
            "floor_played_dbfs": played,
            "floor_margin_db": None if floor is None else min(lv) - floor,
            "floor_exposed_ms": ref_facts.get("floor_exposed_ms"), "lengths_s": durations,
            "length_ratio": durations[1] / max(durations[0], 1e-9)}


def _floor_messages(nums: dict, fails: list, warns: list) -> None:
    margin = nums["floor_margin_db"]
    if margin is not None and margin < FAIL["floor_margin"]:
        fails.append("floor margin %.1f dB under 6 dB" % margin)
    played, heard = nums["floor_played_dbfs"], nums["floor_exposed_ms"]
    if heard is not None:
        text = "the floor is heard for %.0f ms inside a clip" % heard
        if heard > FAIL["exposed_ms"] and played is not None and played > FAIL["exposed_floor"]:
            fails.append(text)
        elif heard > WARN["exposed_ms"]:
            warns.append(text)
    if played is not None and played > WARN["floor_dbfs"]:
        warns.append("floor as played %.1f dBFS" % played)


def _edge_messages(f: tuple[dict, dict], fails: list) -> None:
    for key, where in (("cut_in_db", "onset"), ("cut_out_db", "end")):
        one, two = f[0][key], f[1][key]
        if abs(one - two) > FAIL["edge_db"] and max(one, two) > FAIL["edge_loud"]:
            fails.append("the %s trims differ by %.1f dB while the louder is above -20 dB" % (where, abs(one - two)))


def _soft_messages(m: tuple[dict, dict], nums: dict, warns: list) -> None:
    if nums["length_ratio"] > WARN["length_ratio"]:
        warns.append("length ratio %.2f" % nums["length_ratio"])
    if nums["tp_diff"] > WARN["tp_diff"]:
        warns.append("true peaks differ by %.1f dB" % nums["tp_diff"])
    if nums["other_diff"] > WARN["other_lu"]:
        warns.append("the other loudness window differs by %.1f LU" % nums["other_diff"])
    low, high = sorted(clip["stereo_corr"] for clip in m)
    if high >= WARN["mono"] and low < WARN["wide"]:
        warns.append("one clip is mono and the other is wide")
    if low < 0:
        warns.append("a clip has anti-phase stereo")


def check_pair(m: tuple[dict, dict], f: tuple[dict, dict], lv: tuple[float, float], cls: str,
               gc: float) -> tuple[list[str], list[str], dict]:
    """(fails, warnings, order-free row) of one pair as played; the real clip comes first in m and f."""
    nums = _pair_numbers(m, f, lv, gc)
    fails: list[str] = []
    warns: list[str] = []
    if nums["loud_diff"] > FAIL["loud_lu"]:
        fails.append("the loudest moments differ by %.2f LU" % nums["loud_diff"])
    if nums["tp_max"] > FAIL["true_peak"]:
        fails.append("true peak %.2f dBTP above -1" % nums["tp_max"])
    if nums["preroll_ms"] > FAIL["preroll_ms"] + 1e-6:
        fails.append("pre-roll %.1f ms" % nums["preroll_ms"])
    if any(side["cut_in_db"] >= lv[0] for side in f):
        fails.append("the pre-roll holds sound")
    _floor_messages(nums, fails, warns)
    _edge_messages(f, fails)
    _soft_messages(m, nums, warns)
    return fails, warns, _row(cls, nums, f, lv, m)


_ROW_NUMBERS = ("loud_diff", "other_diff", "meter_diff", "tp_diff", "tp_max", "preroll_max_dbfs", "preroll_ms",
                "floor_played_dbfs", "floor_margin_db", "floor_exposed_ms", "length_ratio")


def _row(cls: str, nums: dict, f: tuple[dict, dict], lv: tuple[float, float], m: tuple[dict, dict]) -> dict:
    row = {"class": cls, "matched_on": "K-weighted loudest %.0f ms" % (WIN[cls] * 1000)}
    for key in _ROW_NUMBERS:
        row[key] = None if nums[key] is None else round(nums[key], 3)
    row["L_on"], row["L_end"] = round(lv[0], 2), round(lv[1], 2)
    row["lengths_s"] = nums["lengths_s"]
    for key in ("cut_in_db", "cut_out_db", "fade_out_ms"):
        row[key] = sorted(side[key] for side in f)
    row["stereo_corr"] = sorted(round(clip["stereo_corr"], 3) for clip in m)
    return row
