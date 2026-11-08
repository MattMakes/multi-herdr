"""Audio measures, the shared pair trim, the level match and the cleanliness check of the blind test.

Both clips of a pair get the same trim rule, both play equally loud at their loudest moment, and no clip plays
background noise before or after its sound. Levels are dB relative to each sound's own loudest envelope frame.
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
LEAD, GAP, BETWEEN, TAIL = 1.0, 1.2, 2.5, 1.0
COMPAT = 1.3
ON_DB, END_DB = -50.0, -50.0
REACH_MARGIN, FLOOR_MARGIN, HEAD_MARGIN = 3.0, 10.0, 1.0
LEVEL_CAP = -6.0
FADE_IN_S = 0.004
FADE_DB, FADE_MIN_S = 6.0, 0.005
REF_FADE_DEFAULT, MADE_FADE_GUARD = 0.03, 0.01
ENV = {"short": (0.0025, 3), "long": (0.005, 6)}
CLEAN_HEAD, CLEAN_REACH, CLEAN_FLOOR = -30.0, -30.0, -50.0
SHORT_S = 1.0
WIN = {"short": 0.1, "long": 0.4}
TARGET = {"short": -15.0, "long": -16.0}
TP_MAX = -1.0
FAIL = {"loud_lu": 0.5, "true_peak": TP_MAX + 0.05, "floor_margin": 6.0, "preroll_ms": 5.0, "edge_db": 10.0,
        "edge_loud": -20.0, "exposed_ms": 100.0, "exposed_floor": -70.0}
WARN = {"length_ratio": 1.6, "tp_diff": 6.0, "other_lu": 2.0, "floor_dbfs": -60.0, "mono": 0.995, "wide": 0.9,
        "exposed_ms": 30.0}
MANIFEST: Path | None = None  # the refset manifest; set by blind_v2


# ---------------------------------------------------------------- loading

def load_audio(path: Any, start: float | None = None, stop: float | None = None) -> np.ndarray:
    """Float64 stereo at 48 kHz (mono doubled, first 2 channels kept), optionally only [start, stop)."""
    import soundfile as sf
    from scipy.signal import resample_poly

    info = sf.info(str(path))
    i0 = int(round((start or 0.0) * info.samplerate))
    i1 = None if stop is None else int(round(stop * info.samplerate))
    x, rate = sf.read(str(path), dtype="float64", always_2d=True, start=i0, stop=i1)
    if rate != SR:
        g = math.gcd(rate, SR)
        x = resample_poly(x, SR // g, rate // g, axis=0)
    return np.column_stack([x[:, 0], x[:, 0]]) if x.shape[1] == 1 else x[:, :2]


# ---------------------------------------------------------------- audio measures

def frames_db(x: np.ndarray, ms: float) -> np.ndarray:
    """Channel-mean power per whole frame in dB; one value for a file shorter than a frame."""
    p = np.mean(np.asarray(x, dtype=float).reshape(len(x), -1) ** 2, axis=1)
    n = max(1, int(round(SR * ms / 1000)))
    k = len(p) // n
    if k < 1:
        return np.array([10 * np.log10(float(np.mean(p)) + 1e-20)])
    return 10 * np.log10(p[: k * n].reshape(k, n).mean(axis=1) + 1e-20)


def onset(x: np.ndarray, below: float = 40) -> int:
    """Sample of the first 1 ms frame within `below` dB of the loudest."""
    e = frames_db(x, 1)
    return int(np.argmax(e >= e.max() - below)) * int(0.001 * SR)


def end_of(x: np.ndarray, below: float = 50) -> int:
    """End sample of the last 5 ms frame within `below` dB of the loudest."""
    e = frames_db(x, 5)
    return min(len(x), (int(np.flatnonzero(e >= e.max() - below)[-1]) + 1) * int(0.005 * SR))


def extent_s(x: np.ndarray) -> float:
    """The audible extent in seconds."""
    return max(0.0, (end_of(x) - onset(x)) / SR)


def raised(n: int) -> np.ndarray:
    """Raised-cosine ramp 0 -> 1 over n samples."""
    return 0.5 - 0.5 * np.cos(np.pi * np.arange(n) / max(1, n))


def power_frames(x: np.ndarray, n: int, smooth: int = 1) -> np.ndarray:
    """Channel-summed power, mean per n-sample frame (last frame zero-padded), optionally smoothed."""
    p = np.sum(np.asarray(x, dtype=float).reshape(len(x), -1) ** 2, axis=1)
    k = max(1, math.ceil(len(p) / n))
    p = np.pad(p, (0, k * n - len(p))).reshape(k, n).mean(axis=1)
    if smooth > 1 and k > 1:
        p = np.convolve(p, np.ones(smooth) / smooth, mode="same")
    return p


def loud_max(x: np.ndarray, win: float) -> float:
    """K-weighted loudness of the loudest win-second window, at every sample position."""
    from hark.core.features import k_weight

    pad = np.zeros((int(0.05 * SR), 2))
    y = np.concatenate([pad, np.asarray(x, dtype=float).reshape(len(x), -1)[:, :2] * np.ones((1, 2)), pad])
    w = int(round(win * SR))
    if len(y) < w:
        y = np.pad(y, ((0, w - len(y)), (0, 0)))
    s = np.sum(np.asarray(k_weight(y, SR)) ** 2, axis=1)
    c = np.concatenate([[0.0], np.cumsum(s)])
    return float(-0.691 + 10 * np.log10(np.max((c[w:] - c[:-w]) / w) + 1e-20))


def meter_m400(x: np.ndarray) -> float:
    """Max of the momentary loudness curve (400 ms window, 10 ms hop); a cross-check."""
    from hark.core.features import loudness_curve

    n = int(0.4 * SR)
    y = np.pad(x, ((0, max(0, n - len(x))), (0, 0)))
    return float(np.max(loudness_curve(y, win=0.4, hop=0.01)[1]))


def true_peak(x: np.ndarray) -> float:
    """True peak in dBTP (4x oversampled); never under the sample peak."""
    from hark.core.features import true_peak_dbtp

    return max(float(true_peak_dbtp(x)), sample_peak(x))


def sample_peak(x: np.ndarray) -> float:
    return float(20 * np.log10(np.max(np.abs(x)) + 1e-20)) if len(x) else -200.0


def rms_dbfs(x: np.ndarray) -> float:
    return float(10 * np.log10(np.mean(np.asarray(x) ** 2) + 1e-20)) if len(x) else -200.0


def stereo_corr(x: np.ndarray) -> float:
    L, R = x[:, 0], x[:, 1]
    return float(np.sum(L * R) / math.sqrt(float(np.sum(L ** 2) * np.sum(R ** 2)) + 1e-30))


# ---------------------------------------------------------------- file facts

def _key(p: Any) -> str:
    return os.path.normcase(os.path.normpath(str(p).replace("\\", "/")))


@functools.lru_cache(maxsize=1)
def _manifest_index(path: str) -> dict:
    if not path or not os.path.isfile(path):
        return {}
    m = json.loads(Path(path).read_text())
    return {_key(r["file"]): r for c in m.values() for s in ("forge", "bench") for r in c.get(s) or []}


def manifest_entry(p: Any) -> dict | None:
    """The refset manifest record of a ref file, or None (a made final has none)."""
    return _manifest_index(str(MANIFEST or "")).get(_key(p))


@functools.lru_cache(maxsize=None)
def source_floor(p: str) -> float | None:
    """The recording floor power of a bench ref, measured in its source around the trim."""
    m = manifest_entry(p)
    if not m or not m.get("source_path") or not os.path.isfile(m["source_path"]):
        return None
    t0, t1 = m["trim_s"]
    x = load_audio(m["source_path"], max(0.0, t0 - 20), t1 + 20)
    pw = power_frames(x, int(0.05 * SR)) / 2
    pw = pw[pw > 2e-10]
    if not len(pw):
        return None
    return float(np.percentile(pw, 5) * 10 ** (float(m.get("gain_db") or 0) / 10))


def end_fade_s(p: Any) -> float:
    m = manifest_entry(p)
    if m is None:
        return MADE_FADE_GUARD
    return float(m.get("fade_out_s") or REF_FADE_DEFAULT)


def _db(p: np.ndarray, top: float) -> np.ndarray:
    return 10 * np.log10(p + 1e-20) - top


def analyse(x: np.ndarray, cls: str, fade_s: float, floor_pow: float | None = None, cut_file: bool = True) -> dict:
    """Envelope facts for the trim: reach, head, peak_late, strike and floor."""
    hop_s, sm = ENV[cls]
    n = int(round(hop_s * SR))
    p = power_frames(x, n, sm)
    top = float(10 * np.log10(p.max() + 1e-20))
    e, e1 = _db(p, top), _db(power_frames(x, int(0.001 * SR)), top)
    ip = int(np.argmax(e))
    stop = max(ip + 1, len(e) - math.ceil((fade_s + 0.005) / hop_s))
    reach = float(np.min(e[max(ip, stop - max(1, math.ceil(0.015 / hop_s))):stop]))
    head = (float(np.max(e1[2:5])) if len(e1) > 5 else float(np.max(e1))) if cut_file else None
    floor = float(10 * np.log10(floor_pow + 1e-30) - top) if floor_pow is not None else None
    return {"e": e, "e1": e1, "top": top, "ip": ip, "stop": stop, "hop": n, "reach": reach, "head": head,
            "peak_late": ip * n >= 0.85 * len(x), "strike": float(np.max(e1[:15])) >= -6, "floor": floor,
            "fade_s": fade_s}


def _cls(*extents: float) -> str:
    return "long" if max(extents) >= SHORT_S else "short"


@functools.lru_cache(maxsize=None)
def facts(p: str, is_ref: bool) -> tuple[float, bool]:
    """(audible extent, trims cleanly) of a file."""
    x = load_audio(p)
    ext = extent_s(x)
    info = analyse(x, _cls(ext), end_fade_s(p) if is_ref else MADE_FADE_GUARD,
                   source_floor(p) if is_ref else None, cut_file=is_ref)
    clean = ((info["head"] is None or info["head"] <= CLEAN_HEAD or info["strike"])
             and (info["reach"] <= CLEAN_REACH or info["peak_late"])
             and (info["floor"] is None or info["floor"] <= CLEAN_FLOOR) and stereo_corr(x) > -0.5)
    return ext, bool(clean)


# ---------------------------------------------------------------- the pair trim

def pair_levels(a: dict, b: dict) -> tuple[float, float]:
    """(L_on, L_end): levels both sounds reach naturally, above any floor, capped at -6 dB."""
    floors = [i["floor"] + FLOOR_MARGIN for i in (a, b) if i.get("floor") is not None]
    L_end = max([END_DB] + [i["reach"] + REACH_MARGIN for i in (a, b)] + floors)
    L_on = max([ON_DB] + [i["head"] + HEAD_MARGIN for i in (a, b) if i.get("head") is not None] + floors)
    return min(LEVEL_CAP, L_on), min(LEVEL_CAP, L_end)


def _bounds(info: dict, n_x: int, L_on: float, L_end: float) -> tuple[int, int, int, int]:
    n1, hop, e = int(0.001 * SR), info["hop"], info["e"]
    hits = np.flatnonzero(info["e1"] >= L_on)
    on = int(hits[0]) * n1 if len(hits) else 0
    js = np.flatnonzero(e[: info["stop"]] >= L_end)
    j = int(js[-1]) if len(js) else info["ip"]
    end = min(n_x, (j + 1) * hop)
    fs = np.flatnonzero(e[: j + 1] >= L_end + FADE_DB)
    f0 = (int(fs[-1]) + 1) * hop if len(fs) else (info["ip"] + 1) * hop
    f0 = min(max(f0, on), end - int(FADE_MIN_S * SR))
    return on, j, end, f0


def _edge_facts(info: dict, start: int, on: int, j: int, f0: int) -> dict:
    n1, hop, e = int(0.001 * SR), info["hop"], info["e"]
    s, h = max(0, start), start + int(FADE_IN_S * SR) // 2
    cut_in = float(np.max(info["e1"][s // n1: max(s // n1 + 1, math.ceil(h / n1))])) if h > s else -200.0
    after = e[j + 1: j + 1 + math.ceil(0.005 * SR / hop)]
    cut_out = float(np.max(after)) if len(after) else float(e[j])
    exposed = None
    if info.get("floor") is not None:
        seg = e[on // hop: f0 // hop]
        exposed = float(np.sum(seg < info["floor"] + 6) * hop / SR * 1000)
    return {"cut_in_db": round(cut_in, 2), "cut_out_db": round(cut_out, 2), "floor_exposed_ms": exposed}


def cut(x: np.ndarray, info: dict, L_on: float, L_end: float, keep_end: bool) -> tuple[np.ndarray, dict]:
    """Trim one clip with a 4 ms raised-cosine fade in and a fade over its last 6 dB; return (clip, facts)."""
    on, j, end, f0 = _bounds(info, len(x), L_on, L_end)
    start = on - int(FADE_IN_S * SR)
    kept = bool(keep_end and info["e"][info["stop"] - 1] >= L_end)
    if kept:
        end, j = len(x), len(info["e"]) - 1
        f0 = end - int(0.002 * SR)
    clip = np.array(x[max(0, start):end], dtype=float)
    if start < 0:
        clip = np.concatenate([np.zeros((-start, clip.shape[1])), clip])
    nf = int(FADE_IN_S * SR)
    clip[:nf] *= raised(min(nf, len(clip)))[:, None][: len(clip[:nf])]
    nout = end - f0
    if nout > 0:
        clip[-nout:] *= raised(nout)[::-1][:, None]
    f = {"onset_s": round(on / SR, 4), "start_s": round(start / SR, 4), "end_s": round(end / SR, 4),
         "fade_out_ms": round(nout / SR * 1000, 2), **_edge_facts(info, start, on, j, f0),
         "reach_db": round(info["reach"], 2), "head_db": None if info["head"] is None else round(info["head"], 2),
         "floor_db": None if info["floor"] is None else round(info["floor"], 2), "kept_file_end": kept,
         "end_fade_s": info["fade_s"], "top_db": round(info["top"], 2)}
    return clip, f


# ---------------------------------------------------------------- level match and cleanliness

def match_pair(real: np.ndarray, made: np.ndarray, cls: str) -> tuple[float, float, float]:
    """(made gain, common gain, peak-cap drop) in dB: equal loudest moments, true peaks under -1 dBTP."""
    w = WIN[cls]
    g = loud_max(real, w) - loud_max(made, w)
    gc = TARGET[cls] - loud_max(real, w)
    tp = max(true_peak(real), true_peak(made) + g) + gc
    over = max(0.0, tp - TP_MAX)
    return g, gc - over, over


def measure_clip(x: np.ndarray, cls: str, onset_i: int) -> dict:
    """Level facts of one clip as played."""
    other = "long" if cls == "short" else "short"
    return {"loud_pair_measure": loud_max(x, WIN[cls]), "loud_other_window": loud_max(x, WIN[other]),
            "meter_m400": meter_m400(x), "true_peak_dbtp": true_peak(x), "sample_peak_dbfs": sample_peak(x),
            "preroll_dbfs": rms_dbfs(x[:onset_i]), "length_s": round(len(x) / SR, 4), "stereo_corr": stereo_corr(x)}


def _pair_numbers(m: tuple[dict, dict], f: tuple[dict, dict], lv: tuple[float, float], gc: float) -> dict:
    fr = f[0]
    lens = sorted(c["length_s"] for c in m)
    floor_played = fr["floor_db"] + fr["top_db"] + gc - 10 * math.log10(2) if fr["floor_db"] is not None else None
    return {"loud_diff": abs(m[0]["loud_pair_measure"] - m[1]["loud_pair_measure"]),
            "other_diff": abs(m[0]["loud_other_window"] - m[1]["loud_other_window"]),
            "meter_diff": abs(m[0]["meter_m400"] - m[1]["meter_m400"]),
            "tp_diff": abs(m[0]["true_peak_dbtp"] - m[1]["true_peak_dbtp"]),
            "tp_max": max(c["true_peak_dbtp"] for c in m), "preroll_max_dbfs": max(c["preroll_dbfs"] for c in m),
            "preroll_ms": max((c["onset_s"] - c["start_s"]) * 1000 for c in f),
            "floor_played_dbfs": floor_played,
            "floor_margin_db": min(lv) - fr["floor_db"] if fr["floor_db"] is not None else None,
            "floor_exposed_ms": fr.get("floor_exposed_ms"), "lengths_s": lens,
            "length_ratio": lens[1] / max(lens[0], 1e-9)}


def _edge_fails(f: tuple[dict, dict], fails: list) -> None:
    for key, what in (("cut_in_db", "onset"), ("cut_out_db", "end")):
        a, b = f[0][key], f[1][key]
        if abs(a - b) > FAIL["edge_db"] and max(a, b) > FAIL["edge_loud"]:
            fails.append(f"the {what} trims differ by {abs(a - b):.1f} dB while the louder is above -20 dB")


def _floor_rules(p: dict, fails: list, warns: list) -> None:
    if p["floor_margin_db"] is not None and p["floor_margin_db"] < FAIL["floor_margin"]:
        fails.append(f"floor margin {p['floor_margin_db']:.1f} dB under 6 dB")
    ex, fp = p["floor_exposed_ms"], p["floor_played_dbfs"]
    if ex is not None and ex > FAIL["exposed_ms"] and fp is not None and fp > FAIL["exposed_floor"]:
        fails.append(f"the floor is heard for {ex:.0f} ms inside a clip")
    elif ex is not None and ex > WARN["exposed_ms"]:
        warns.append(f"the floor is heard for {ex:.0f} ms inside a clip")
    if fp is not None and fp > WARN["floor_dbfs"]:
        warns.append(f"floor as played {fp:.1f} dBFS")


def _warn_rules(m: tuple[dict, dict], p: dict, warns: list) -> None:
    if p["length_ratio"] > WARN["length_ratio"]:
        warns.append(f"length ratio {p['length_ratio']:.2f}")
    if p["tp_diff"] > WARN["tp_diff"]:
        warns.append(f"true peaks differ by {p['tp_diff']:.1f} dB")
    if p["other_diff"] > WARN["other_lu"]:
        warns.append(f"the other loudness window differs by {p['other_diff']:.1f} LU")
    corr = sorted(c["stereo_corr"] for c in m)
    if corr[1] >= WARN["mono"] and corr[0] < WARN["wide"]:
        warns.append("one clip is mono and the other is wide")
    if corr[0] < 0:
        warns.append("a clip has anti-phase stereo")


def check_pair(m: tuple[dict, dict], f: tuple[dict, dict], lv: tuple[float, float], cls: str,
               gc: float) -> tuple[list[str], list[str], dict]:
    """(fails, warnings, symmetric row) of one pair as played; real first in m and f."""
    p = _pair_numbers(m, f, lv, gc)
    fails: list[str] = []
    warns: list[str] = []
    if p["loud_diff"] > FAIL["loud_lu"]:
        fails.append(f"the loudest moments differ by {p['loud_diff']:.2f} LU")
    if p["tp_max"] > FAIL["true_peak"]:
        fails.append(f"true peak {p['tp_max']:.2f} dBTP above -1")
    if p["preroll_ms"] > FAIL["preroll_ms"] + 1e-6:
        fails.append(f"pre-roll {p['preroll_ms']:.1f} ms")
    if any(c["cut_in_db"] >= lv[0] for c in f):
        fails.append("the pre-roll holds sound")
    _floor_rules(p, fails, warns)
    _edge_fails(f, fails)
    _warn_rules(m, p, warns)
    return fails, warns, _symmetric(cls, p, f, lv, m)


def _symmetric(cls: str, p: dict, f: tuple[dict, dict], lv: tuple[float, float], m: tuple[dict, dict]) -> dict:
    r = {"class": cls, "matched_on": f"K-weighted loudest {WIN[cls] * 1000:.0f} ms"}
    for k in ("loud_diff", "other_diff", "meter_diff", "tp_diff", "tp_max", "preroll_max_dbfs", "preroll_ms",
              "floor_played_dbfs", "floor_margin_db", "floor_exposed_ms", "length_ratio"):
        r[k] = None if p[k] is None else round(p[k], 3)
    r.update(L_on=round(lv[0], 2), L_end=round(lv[1], 2), lengths_s=p["lengths_s"],
             cut_in_db=sorted(c["cut_in_db"] for c in f), cut_out_db=sorted(c["cut_out_db"] for c in f),
             fade_out_ms=sorted(c["fade_out_ms"] for c in f),
             stereo_corr=sorted(round(c["stereo_corr"], 3) for c in m))
    return r
