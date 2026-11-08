"""profile: a numeric sound profile of reference films (median and spread of each metric).

Every film is measured the same way: loudness, loudness arc, ending, music and speech presence, onset
and hit density, stop-downs, picture sync and spectrum. The profile holds numbers only, no file names.
The timeline critique compares a new soundtrack with such a profile.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import sys
import time
from collections.abc import Mapping, Sequence
from pathlib import Path
from typing import Any

import numpy

from hark.core import audio, features, report
from hark.listen import vocabulary as voc

SR = 48000
ONSET_SR = 22050
HOP = 256
VERSION = 2
AUDIO_PASS_VERSION = 1
AST_VERSION = 1
MIN_FILM_S = 10.0
MIN_SHAPE_S = 3.0
SILENT_LUFS = -69.5
AST_WIN_S = 5.0
AST_MIN_REST_S = 1.0
MUSIC_P = 0.3
SPEECH_P = 0.5
MONO_DB = -40.0
E2_S = 0.002
FAST = (0.1, 0.02)
MOMENTARY = (0.4, 0.1)
SHORT_TERM = (3.0, 0.1)
CHANCE_OFFSETS = (0.23, 0.31, 0.43, 0.57, 0.71, 0.87, 1.13, 1.37, -0.29, -0.41, -0.53, -0.67, -0.79, -0.97,
                  -1.19, -1.43)
PROFILE_PATH = Path(__file__).resolve().parent.parent / "data" / "timeline" / "launch_profile.json"

# key, group, unit, decimals, label (table order = output order)
METRICS: tuple[tuple[str, str, str, int, str], ...] = (
    ("lufs_i", "loudness", "LUFS", 1, "integrated loudness"),
    ("true_peak", "loudness", "dBTP", 1, "true peak"),
    ("lra", "loudness", "LU", 1, "loudness range"),
    ("crest_lu", "loudness", "LU", 1, "loudest 3 s over integrated"),
    ("intro_lu", "arc", "LU", 1, "opening level"),
    ("end_lu", "arc", "LU", 1, "closing level"),
    ("peak_pos", "arc", "0-1", 2, "position of the loudest 3 s"),
    ("first_sound_s", "arc", "s", 2, "time to first sound"),
    ("max_gap_s", "arc", "s", 2, "longest near-silent gap"),
    ("end_silence_s", "ending", "s", 2, "dead air at the end"),
    ("fade_s", "ending", "s", 2, "fade-out length"),
    ("final_lu", "ending", "LU", 1, "final hit level"),
    ("tail_s", "ending", "s", 2, "final hit ring-out"),
    ("music_frac", "music", "share", 2, "music presence"),
    ("speech_frac", "music", "share", 2, "speech presence"),
    ("tempo", "music", "BPM", 0, "tempo"),
    ("onset_rate", "density", "/s", 2, "onsets per second"),
    ("hit_rate", "density", "/s", 2, "salient hits per second"),
    ("stopdowns_min", "density", "/min", 1, "stop-downs per minute"),
    ("cut_rate", "sync", "/s", 2, "visual events per second"),
    ("cuts_with_sound", "sync", "share", 2, "visual events with an onset (100 ms)"),
    ("cut_hit_frac", "sync", "share", 2, "visual events with a salient hit (100 ms)"),
    ("cut_frame_sync", "sync", "share", 2, "visual events with an onset (1 frame)"),
    ("cuts_on_beat", "sync", "share", 2, "visual events on a beat (70 ms)"),
    ("band_sub", "spectrum", "dB", 1, "energy 20-60 Hz"),
    ("band_low", "spectrum", "dB", 1, "energy 60-250 Hz"),
    ("band_lowmid", "spectrum", "dB", 1, "energy 250 Hz-1 kHz"),
    ("band_mid", "spectrum", "dB", 1, "energy 1-4 kHz"),
    ("band_presence", "spectrum", "dB", 1, "energy 4-8 kHz"),
    ("band_air", "spectrum", "dB", 1, "energy 8-16 kHz"),
    ("brightness_hz", "spectrum", "Hz", 0, "brightness"),
    ("side_to_mid_db", "stereo", "dB", 1, "stereo width"),
    ("correlation", "stereo", "r", 2, "L/R correlation"),
)
METRIC_KEYS = tuple(m[0] for m in METRICS)
DECIMALS = {m[0]: m[3] for m in METRICS}
UNITS = {m[0]: m[2] for m in METRICS}
SHAPE_METRICS = ("lra", "crest_lu", "intro_lu", "end_lu", "final_lu")
SYNC_KEYS = ("cuts_with_sound", "cut_hit_frac", "cut_frame_sync", "cuts_on_beat")


class ProfileError(ValueError):
    """Bad profile input (no usable films, bad styles)."""


# ================================================================ small helpers


def energy_mean(levels: Sequence[float]) -> float:
    """10 log10 of the mean power of dB levels."""
    vec = numpy.asarray(levels, numpy.float64)
    return float(10 * numpy.log10(numpy.mean(10 ** (vec / 10))))


def fmt_value(kname: str, vec: float | None) -> str:
    """A metric value as text with its unit."""
    if vec is None:
        return "n.a."
    unit = UNITS.get(kname, "")
    if unit == "share":
        return f"{int(round(100 * vec))}%"
    text = f"{vec:.{DECIMALS.get(kname, 2)}f}"
    return text if unit in ("0-1", "r") else f"{text} {unit}"


def _finite(vec: Any) -> bool:
    return isinstance(vec, (int, float)) and math.isfinite(vec)


def _round_metrics(mat: Mapping[str, Any]) -> dict:
    result = {}
    for kk in METRIC_KEYS:
        vec = mat.get(kk)
        if not _finite(vec):
            result[kk] = None
        elif DECIMALS[kk] == 0:
            result[kk] = int(round(vec))
        else:
            result[kk] = round(float(vec), DECIMALS[kk])
    return result


# ================================================================ media checks


def _no_streams(info: Mapping[str, Any]) -> bool:
    """ffprobe found neither an audio nor a picture stream: the file is not media."""
    return not info.get("codec") and not info.get("has_video") and not info.get("duration")


def _probe_problem(info: Mapping[str, Any]) -> str | None:
    if info.get("probe_error"):
        return f"not a readable audio or video file ({str(info['probe_error'])[:160]})"
    if _no_streams(info):
        return "not a readable audio or video file (no audio or video stream)"
    return None if info.get("codec") else "no audio track"


def media_problem(fpath: str) -> str | None:
    """Why a path cannot be profiled (not found, folder, unreadable, no audio), or None."""
    if not os.path.exists(fpath):
        return "not found"
    return "a folder, not a file" if os.path.isdir(fpath) else _probe_problem(audio.probe(fpath))


def unscorable(metrics: Mapping[str, Any]) -> str | None:
    """'shorter than 3 s' or 'silent', or None (takes duration and lufs or lufs_i)."""
    dd = metrics.get("duration")
    if dd is not None and dd < MIN_SHAPE_S:
        return f"shorter than {MIN_SHAPE_S:g} s ({dd:.2f} s)"
    lufs = metrics.get("lufs_i", metrics.get("lufs"))
    if lufs is None or lufs <= SILENT_LUFS:
        return "silent"
    return None


# ================================================================ P1 audio pass


def _curve(sig: numpy.ndarray, block: tuple[float, float]) -> list[list[float]]:
    tx, lv = features.loudness_curve(sig, win=block[0], hop=block[1])
    lv = numpy.asarray(lv, numpy.float64)
    return [numpy.asarray(tx, numpy.float64).tolist(), numpy.where(numpy.isfinite(lv), lv, -120.0).tolist()]


def _onset_pass(mono: numpy.ndarray) -> dict:
    import librosa

    yv = numpy.asarray(audio.resample(mono.astype(numpy.float32), SR, ONSET_SR), numpy.float32)
    if len(yv) < 2048:
        return {"env": [], "onsets": [], "tempo": None, "beats": []}
    envl = librosa.onset.onset_strength(y=yv, sr=ONSET_SR, hop_length=HOP)
    on = librosa.onset.onset_detect(onset_envelope=envl, sr=ONSET_SR, hop_length=HOP, units="time")
    tempo, beats = librosa.beat.beat_track(onset_envelope=envl, sr=ONSET_SR, hop_length=HOP, units="time")
    return {"env": numpy.asarray(envl, numpy.float64).tolist(), "onsets": [float(vec) for vec in on],
            "tempo": float(numpy.atleast_1d(tempo)[0]) if numpy.size(tempo) else None, "beats": [float(rhs) for rhs in beats]}


def to_attack(tx: float, e_two: numpy.ndarray) -> float:
    """Move a time to the steepest 2 ms rise from 60 ms before to 20 ms after it (P5)."""
    lhs = max(0, int(math.floor((tx - 0.06) / E2_S)))
    rhs = min(len(e_two), int(math.floor((tx + 0.02) / E2_S)) + 1)
    if rhs - lhs < 2:
        return round(tx, 4)
    rise = numpy.diff(e_two[lhs:rhs])
    return round((lhs + 1 + int(numpy.argmax(rise))) * E2_S, 4)


def _e2(mono: numpy.ndarray) -> numpy.ndarray:
    return 20 * numpy.log10(numpy.asarray(features.frame_rms(mono, int(E2_S * SR)), numpy.float64) + 1e-9)


def audio_pass(fpath: str) -> dict:
    """Measurements, onsets, beats, loudness curves and 2 ms levels of a file (cached, P1)."""
    cpath = voc.cache_dir() / "profile" / f"a_{voc.key_of('profile-audio', audio.file_id(fpath), AUDIO_PASS_VERSION)}.json"
    hit = voc.load_json(cpath)
    if isinstance(hit, dict) and "duration" in hit:
        return hit
    sig = numpy.asarray(audio.load(fpath), numpy.float64)
    mono = sig.mean(axis=1) if sig.ndim == 2 else sig
    meas = features.measure(sig, light=True)
    e_two = _e2(mono)
    op = _onset_pass(mono)
    op["onsets"] = [to_attack(tx, e_two) for tx in op["onsets"]]
    ap = {"duration": len(mono) / SR, "measure": report.clean(meas), **op, "e2": e_two.tolist(),
          "fast": _curve(sig, FAST), "momentary": _curve(sig, MOMENTARY), "short": _curve(sig, SHORT_TERM)}
    voc.save_json(cpath, ap)
    return ap


def ast_pass(fpath: str, dd: float) -> tuple[list[float], numpy.ndarray, Any]:
    """5 s window starts, their AudioSet probabilities and the tagger (cached, P2)."""
    from hark.models import ast

    model = ast.get()
    starts = [sv for sv in numpy.arange(0.0, max(dd, 1e-9), AST_WIN_S) if sv == 0 or dd - sv >= AST_MIN_REST_S]
    cpath = voc.cache_dir("ast") / f"pw_{voc.key_of('profile-ast', audio.file_id(fpath), AST_WIN_S, AST_VERSION)}.npy"
    P = voc.load_array(cpath, (len(starts), 527))
    if P is None:
        mono = audio.to_mono(numpy.asarray(audio.load(fpath), numpy.float32))
        segs = [mono[int(sv * SR):int((sv + AST_WIN_S) * SR)] for sv in starts]
        P = numpy.asarray(model.probs(segs), numpy.float16)
        voc.save_array(cpath, P)
    return [float(sv) for sv in starts], numpy.asarray(P, numpy.float32), model


# ================================================================ P3 hits, P4 stop-downs


def _runs(idx: Sequence[int], gap: int = 2) -> list[list[int]]:
    runs: list[list[int]] = []
    for ix in idx:
        if runs and ix - runs[-1][-1] <= gap:
            runs[-1].append(ix)
        else:
            runs.append([ix])
    return runs


def _hit_time(run: list[int], tx: numpy.ndarray, envl: numpy.ndarray) -> float:
    lhs, rhs = tx[run[0]] - 0.05, tx[run[-1]] + 0.1
    ft = numpy.arange(len(envl)) * HOP / ONSET_SR
    sel = numpy.nonzero((ft >= lhs) & (ft <= rhs))[0]
    return float(ft[sel[numpy.argmax(envl[sel])]]) if len(sel) else float(tx[run[0]] + 0.03)


def _jumps(lv: numpy.ndarray, lufs_i: float) -> numpy.ndarray:
    """Frames where the max of the next 4 levels is >= 6 dB over the min of the 10 before, near the level I."""
    padded = numpy.concatenate([numpy.full(10, lv[0]), lv, numpy.full(3, lv[-1])])
    pre = numpy.array([padded[ix:ix + 10].min() for ix in range(len(lv))])
    post = numpy.array([padded[ix + 10:ix + 14].max() for ix in range(len(lv))])
    return numpy.nonzero((post - pre >= 6) & (post >= lufs_i - 8))[0]


def _spaced(times: Sequence[float], gap: float = 0.1) -> list[float]:
    result: list[float] = []
    for h in times:
        if not result or h - result[-1] > gap:
            result.append(h)
    return result


def salient_hits(fast: Sequence[Sequence[float]], lufs_i: float, envl: Sequence[float], e_two: numpy.ndarray) -> list[float]:
    """Times where the 100 ms loudness jumps >= 6 dB within 80 ms and lands near the integrated level (P3)."""
    tx, lv = numpy.asarray(fast[0]), numpy.asarray(fast[1])
    if len(lv) < 20:
        return []
    env_a = numpy.asarray(envl, numpy.float64)
    found = _spaced([_hit_time(run, tx, env_a) for run in _runs(_jumps(lv, lufs_i).tolist())])
    return [round(to_attack(h, e_two), 3) for h in found]


def _stop_at(lv: numpy.ndarray, ix: int, win_n: int) -> tuple[int, float, float] | None:
    ref = float(numpy.median(lv[ix - win_n:ix]))
    if lv[ix] >= ref - 10:
        return None
    jj = ix
    while jj < len(lv) and lv[jj] < ref - 10:
        jj += 1
    return jj, ref, float(lv[ix:jj].min())


def _stop_record(tx: numpy.ndarray, lv: numpy.ndarray, ix: int, found: tuple[int, float, float]) -> list[float] | None:
    """A stop-down that starts at frame i, or None when its length or recovery is out of range."""
    jj, ref, low = found
    length = (jj - ix) * FAST[1]
    if 0.15 <= length <= 1.5 and jj < len(lv) and lv[jj:jj + 16].max() >= ref - 3:
        return [round(float(tx[ix]) - 0.05, 3), round(length, 3), round(ref - low, 1)]
    return None


def stopdowns(fast: Sequence[Sequence[float]]) -> list[list[float]]:
    """Short drops >= 10 dB under the 2 s median, 0.15 to 1.5 s long, back within 3 dB in 0.3 s (P4)."""
    tx, lv = numpy.asarray(fast[0]), numpy.asarray(fast[1])
    win_n = 100
    result: list[list[float]] = []
    ix = win_n
    while len(lv) >= 3 and ix < len(lv):
        found = _stop_at(lv, ix, win_n)
        rec = _stop_record(tx, lv, ix, found) if found else None
        if rec:
            result.append(rec)
            ix = found[0] + 25
        else:
            ix += 1
    return result


# ================================================================ P6 metrics, P9 edges


def _edge_index(ok: numpy.ndarray, lhs: int, t_from: bool) -> float:
    return (lhs + int(ok[0])) * E2_S if t_from else (lhs + int(ok[-1]) + 1) * E2_S


def _edge(e_two: numpy.ndarray, tx: float, t_from: bool) -> float:
    lhs = max(0, int(math.floor((tx - 0.05) / E2_S)))
    rhs = min(len(e_two), int(math.floor((tx + 0.05) / E2_S)) + 1)
    if rhs <= lhs:
        return round(max(0.0, tx), 3)
    span = int(0.15 / E2_S)
    ref = float(e_two[lhs:lhs + span].max()) if t_from else float(e_two[max(0, rhs - span):rhs].max())
    ok = numpy.nonzero(e_two[lhs:rhs] >= ref - 20)[0]
    return round(_edge_index(ok, lhs, t_from), 3) if len(ok) else round(max(0.0, tx), 3)


def _arc(mt: numpy.ndarray, ml: numpy.ndarray, dd: float, lufs_i: float) -> list[float | None]:
    half = max(1.5, 0.05 * dd)
    result: list[float | None] = []
    for kk in range(11):
        sel = numpy.abs(mt - kk / 10 * dd) <= half
        result.append(round(energy_mean(ml[sel]) - lufs_i, 1) if sel.any() else None)
    return result


def _series(ap: dict, name: str) -> tuple[numpy.ndarray, numpy.ndarray]:
    return numpy.asarray(ap[name][0]), numpy.asarray(ap[name][1])


def _mean_where(ml: numpy.ndarray, sel: numpy.ndarray, lufs_i: float) -> float | None:
    return energy_mean(ml[sel]) - lufs_i if sel.any() else None


def _intro_end(ap: dict, lufs_i: float, dd: float) -> dict:
    mt, ml = _series(ap, "momentary")
    if len(mt) <= 2:
        return {}
    span = max(0.1 * dd, 0.5)
    return {"intro_lu": _mean_where(ml, mt <= span, lufs_i), "end_lu": _mean_where(ml, mt >= dd - span, lufs_i)}


def _peak_pos(ap: dict, dd: float) -> dict:
    stt, sl = _series(ap, "short")
    return {"peak_pos": float(stt[int(numpy.argmax(sl))] / dd)} if len(stt) >= 2 and dd >= 6 else {}


def _crest(mat: Mapping[str, Any], lufs_i: float) -> dict:
    if mat.get("short_term_max_lufs") is None or mat.get("lufs") is None:
        return {}
    return {"crest_lu": mat["short_term_max_lufs"] - lufs_i}


def _loudness_metrics(ap: dict, lufs_i: float, dd: float) -> dict:
    mat = ap["measure"]
    bands = mat.get("bands_db") or {}
    return {"lufs_i": mat.get("lufs"), "true_peak": mat.get("true_peak_dbtp"), "lra": mat.get("lra_lu"),
            "correlation": mat.get("correlation"), "brightness_hz": mat.get("brightness_hz"), **_crest(mat, lufs_i),
            **_intro_end(ap, lufs_i, dd), **_peak_pos(ap, dd),
            **{f"band_{rhs}": bands.get(rhs) for rhs in ("sub", "low", "lowmid", "mid", "presence", "air")}}


def _tail(ap: dict, t_stop: float) -> float | None:
    on, envl = ap["onsets"], numpy.asarray(ap["env"], numpy.float64)
    if not on or not len(envl):
        return None
    strength = numpy.array([envl[min(len(envl) - 1, int(round(tx * ONSET_SR / HOP)))] for tx in on])
    sel = [ix for ix, tx in enumerate(on) if t_stop - 4 <= tx < t_stop]
    if not sel:
        return None
    winner = max(sel, key=lambda ix: strength[ix])
    return t_stop - on[winner] if strength[winner] >= numpy.percentile(strength, 75) else None


def _longest_run(flags: numpy.ndarray) -> int:
    run = winner = 0
    for grp in flags:
        run = run + 1 if grp else 0
        winner = max(winner, run)
    return winner


def _max_gap(ft: numpy.ndarray, fl: numpy.ndarray, first: float, t_stop: float, lufs_i: float) -> float:
    rng = (ft >= first) & (ft <= t_stop)
    return _longest_run(fl[rng] < lufs_i - 30) * FAST[1] if rng.sum() > 1 else 0.0


def _fade_final(ap: dict, lufs_i: float, t_stop: float) -> dict:
    mt, ml = _series(ap, "momentary")
    full = mt[(ml >= lufs_i - 6) & (mt <= t_stop)]
    fin = ml[(mt >= t_stop - 3) & (mt <= t_stop)]
    result = {"final_lu": float(fin.max()) - lufs_i if len(fin) else None}
    if len(full):
        result["fade_s"] = max(0.0, t_stop - float(full[-1]) - 0.2)
    return result


def _ending_metrics(ap: dict, lufs_i: float, dd: float) -> dict:
    ft, fl = _series(ap, "fast")
    e_two = numpy.asarray(ap["e2"])
    result: dict[str, Any] = {}
    loud, aud = numpy.nonzero(fl > lufs_i - 20)[0], numpy.nonzero(fl > lufs_i - 30)[0]
    if len(loud):
        result["first_sound_s"] = _edge(e_two, float(ft[loud[0]]), True)
    if not len(aud):
        return result
    t_stop = min(dd, _edge(e_two, float(ft[aud[-1]]), False))
    result.update(end_silence_s=max(0.0, dd - t_stop), **_fade_final(ap, lufs_i, t_stop),
               max_gap_s=_max_gap(ft, fl, result.get("first_sound_s") or 0.0, t_stop, lufs_i), tail_s=_tail(ap, t_stop))
    return result


def _per_s(count: int, dd: float) -> float | None:
    return count / dd if dd else None


def _rates(ap: dict, found: list[float], stops: list, dd: float) -> dict:
    return {"onset_rate": _per_s(len(ap["onsets"]), dd), "hit_rate": _per_s(len(found), dd),
            "stopdowns_min": len(stops) / dd * 60 if dd >= 6 else None,
            "tempo": ap["tempo"] if len(ap["beats"]) > 4 else None}


def integrated(mat: Mapping[str, Any]) -> float:
    """The integrated loudness, or -70 when unknown."""
    return mat.get("lufs") if mat.get("lufs") is not None else -70.0


def is_mono(mat: Mapping[str, Any], lufs_i: float) -> bool:
    return mat.get("side_to_mid_db") is None and lufs_i > SILENT_LUFS


def film_core(ap: dict, found: list[float], stops: list, dd: float) -> tuple[dict, list[float] | None]:
    """The audio metrics and the arc of one film (P6)."""
    mat = ap["measure"]
    lufs_i = integrated(mat)
    result = {**_loudness_metrics(ap, lufs_i, dd), **_ending_metrics(ap, lufs_i, dd), **_rates(ap, found, stops, dd),
           "side_to_mid_db": MONO_DB if is_mono(mat, lufs_i) else mat.get("side_to_mid_db")}
    if dd < MIN_FILM_S:
        result.update({kk: None for kk in SHAPE_METRICS})
    mt, ml = _series(ap, "momentary")
    return result, _arc(mt, ml, dd, lufs_i) if len(mt) > 2 else None


# ================================================================ P7 sync


def near(V: Sequence[float], targets: Sequence[float], tol: float) -> float | None:
    """Share of events with a target no more than tol away (None without events, 0 without targets)."""
    if not len(V):
        return None
    if not len(targets):
        return 0.0
    T = numpy.asarray(targets, numpy.float64)
    return float(numpy.mean([numpy.min(numpy.abs(T - vec)) <= tol + 1e-9 for vec in V]))


def chance(V: Sequence[float], targets: Sequence[float], tol: float, dd: float) -> float | None:
    """Mean of near() over the 16 fixed offsets, wrapped modulo the duration."""
    if not len(V):
        return None
    vals = [near([(vec + o) % dd for vec in V], targets, tol) for o in CHANCE_OFFSETS]
    return float(numpy.mean(vals))


def sync_metrics(V: Sequence[float], ap: dict, found: Sequence[float], fps: float, dd: float) -> tuple[dict, dict]:
    """cut_rate, the 4 sync shares and their chance values."""
    result: dict[str, Any] = {"cut_rate": len(V) / dd if dd else None}
    chances: dict[str, Any] = {kk: None for kk in SYNC_KEYS}
    if len(V) < 3:
        result.update({kk: None for kk in SYNC_KEYS})
        return result, chances
    plan = {"cuts_with_sound": (ap["onsets"], 0.1), "cut_hit_frac": (found, 0.1),
            "cut_frame_sync": (ap["onsets"], 1 / fps + 0.006), "cuts_on_beat": (ap["beats"], 0.07)}
    for kk, (tg, tol) in plan.items():
        result[kk] = near(V, tg, tol)
        cand = chance(V, tg, tol, dd)
        chances[kk] = None if cand is None else round(cand, 2)
    return result, chances


def picture_events(src: str, dd: float) -> tuple[float | None, list[float], list[float]]:
    """(fps, visual event times, their strengths) of a picture source."""
    from hark.timeline import video

    fps, diff = video.frame_diff(src)
    diff = numpy.asarray(diff, numpy.float64)
    idx = [ix for ix in video.diff_events(diff, fps) if ix / fps < dd]
    return float(fps), [round(ix / fps, 3) for ix in idx], [round(float(diff[ix]), 3) for ix in idx]


# ================================================================ film_metrics


def _picture_source(fpath: str, video: str | None) -> str | None:
    if video:
        return video
    from hark.timeline import video as vid

    return fpath if vid.has_video(fpath) else None


def _music(fpath: str, dd: float, use_ast: bool) -> tuple[dict, list | None, float]:
    if not use_ast:
        return {"music_frac": None, "speech_frac": None}, None, 0.0
    t0 = time.perf_counter()
    starts, P, model = ast_pass(fpath, dd)
    pm = [model.prob_of(pth, "Music") for pth in P]
    ps = [model.prob_of(pth, "Speech") for pth in P]
    frame_w = [[round(sv, 3), round(float(lhs), 3), round(float(rhs), 3)] for sv, lhs, rhs in zip(starts, pm, ps)]
    shares = {"music_frac": float(numpy.mean(numpy.array(pm) >= MUSIC_P)) if pm else None,
              "speech_frac": float(numpy.mean(numpy.array(ps) >= SPEECH_P)) if ps else None}
    return shares, frame_w, time.perf_counter() - t0


def _film_summary(fpath: str, row_: dict) -> str:
    mat = row_["metrics"]
    pieces = [f"{os.path.basename(fpath)}: {row_['duration']:.1f} s", f"{fmt_value('lufs_i', mat['lufs_i'])}",
             f"true peak {fmt_value('true_peak', mat['true_peak'])}"]
    if mat["lra"] is not None:
        pieces.append(f"LRA {fmt_value('lra', mat['lra'])}")
    if mat["music_frac"] is not None:
        pieces.append(f"music {fmt_value('music_frac', mat['music_frac'])}")
    pieces += [f"onsets {fmt_value('onset_rate', mat['onset_rate'])}", f"hits {fmt_value('hit_rate', mat['hit_rate'])}"]
    if mat["cuts_with_sound"] is not None:
        pieces.append(f"{len(row_['events']['visual'])} visual events, {fmt_value('cuts_with_sound', mat['cuts_with_sound'])}"
                     " with an onset within 100 ms")
    return ", ".join(pieces) + "."


def _picture(fpath: str, video: str | None, ap: dict, found: list[float], dd: float) -> dict:
    """The picture source, its events, the sync metrics and their chance values."""
    t1 = time.perf_counter()
    src = _picture_source(fpath, video)
    if not src:
        return {"src": None, "fps": None, "V": [], "strength": [], "chances": {kk: None for kk in SYNC_KEYS},
                "sync": {kk: None for kk in ("cut_rate",) + SYNC_KEYS}, "seconds": time.perf_counter() - t1}
    fps, V, strength = picture_events(src, dd)
    sync, chances = sync_metrics(V, ap, found, fps, dd)
    return {"src": src, "fps": fps, "V": V, "strength": strength, "sync": sync, "chances": chances,
            "seconds": time.perf_counter() - t1}


def _events(ap: dict, found: list[float], stops: list, pic: dict, windows: list | None) -> dict:
    return {"hits": found, "visual": pic["V"], "visual_strength": pic["strength"], "stopdowns": stops,
            "beats": [round(rhs, 3) for rhs in ap["beats"]], "music_windows": windows, "onsets_n": len(ap["onsets"])}


def _media_info(fpath: str, ap: dict) -> dict:
    info = audio.probe(fpath)
    mat = ap["measure"]
    return {"codec": info.get("codec"), "lossy": bool(info.get("lossy")), "mono": is_mono(mat, integrated(mat))}


def _audio_part(fpath: str) -> dict:
    """The audio pass, hits, stop-downs, audio metrics and arc, with the seconds it took."""
    t0 = time.perf_counter()
    ap = audio_pass(fpath)
    dd = ap["duration"]
    found = salient_hits(ap["fast"], integrated(ap["measure"]), ap["env"], numpy.asarray(ap["e2"]))
    stops = stopdowns(ap["fast"])
    metrics, arc = film_core(ap, found, stops, dd)
    return {"ap": ap, "d": dd, "hits": found, "stops": stops, "metrics": metrics, "arc": arc,
            "seconds": time.perf_counter() - t0}


def film_metrics(fpath: str, video: str | None = None, use_ast: bool = True) -> dict:
    """Every profile metric of one film or soundtrack."""
    t0 = time.perf_counter()
    au = _audio_part(fpath)
    ap, dd, found = au["ap"], au["d"], au["hits"]
    shares, windows, t_ast = _music(fpath, dd, use_ast)
    pic = _picture(fpath, video, ap, found, dd)
    metrics = {**au["metrics"], **shares, **pic["sync"]}
    row_ = {"path": fpath, "video": pic["src"], "duration": round(dd, 3), "fps": pic["fps"], **_media_info(fpath, ap),
         "short": dd < MIN_FILM_S, "metrics": _round_metrics(metrics), "chance": pic["chances"], "arc": au["arc"],
         "events": _events(ap, found, au["stops"], pic, windows),
         "timing": {"audio": round(au["seconds"], 2), "audioset": round(t_ast, 2), "picture": round(pic["seconds"], 2)}}
    row_["seconds"] = round(time.perf_counter() - t0, 2)
    row_["summary"] = _film_summary(fpath, row_)
    return row_


# ================================================================ P8 aggregate and profile


def _stats(values: Sequence[float | None]) -> dict:
    vec = [float(sig) for sig in values if _finite(sig)]
    if not vec:
        return {"n": 0}
    p10, med, p90 = numpy.percentile(vec, [10, 50, 90])
    return {"n": len(vec), "median": round(float(med), 3), "p10": round(float(p10), 3), "p90": round(float(p90), 3),
            "min": round(min(vec), 3), "max": round(max(vec), 3),
            "values": [round(float(sig), 3) if _finite(sig) else None for sig in values]}


def _arc_stats(arcs: Sequence[Sequence[float | None] | None]) -> dict | None:
    records = [lhs for lhs in arcs if lhs]
    if not records:
        return None
    result: dict[str, list] = {"median": [], "p10": [], "p90": []}
    for kk in range(11):
        vals = [row_[kk] for row_ in records if row_[kk] is not None]
        for name, qtext in (("median", 50), ("p10", 10), ("p90", 90)):
            result[name].append(round(float(numpy.percentile(vals, qtext)), 1) if vals else None)
    return result


def aggregate(reels: Sequence[Mapping]) -> dict:
    """Per-metric stats, sync chance stats and arc percentiles over the films (P8)."""
    return {"metrics": {kk: _stats([fv["metrics"].get(kk) for fv in reels]) for kk in METRIC_KEYS},
            "chance": {kk: _stats([fv["chance"].get(kk) for fv in reels]) for kk in SYNC_KEYS},
            "arc": _arc_stats([fv.get("arc") for fv in reels])}


def _styles_file(pth: Path) -> Any:
    if not pth.exists():
        raise ProfileError(f"styles file {pth} does not exist")
    try:
        return json.loads(pth.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        raise ProfileError(f"styles file {pth} is not valid JSON: {exc}") from exc


def read_styles(looks: Any) -> Any:
    """A styles option: None, a list, a dict, or a JSON file path holding one of them."""
    if looks is None or isinstance(looks, (list, dict)):
        return looks
    data = _styles_file(Path(str(looks)))
    if not isinstance(data, (list, dict)):
        raise ProfileError("styles must be a JSON list or object")
    return data


def style_of(looks: Any, fpath: str, ix: int) -> str:
    """The style of the i-th listed file."""
    if isinstance(looks, dict):
        return str(looks.get(os.path.basename(fpath), ""))
    if isinstance(looks, list) and ix < len(looks):
        return str(looks[ix])
    return ""


def _warn(text: str) -> None:
    print(f"hark profile: {text}", file=sys.stderr)


def _skip_reason(fpath: str) -> str | None:
    problem = media_problem(fpath)
    if problem:
        return problem
    try:
        sig = audio.load(fpath)
    except Exception as exc:  # a decode error skips the file
        return f"cannot decode ({voc.first_line(exc, 160)})"
    dd = len(sig) / SR
    lufs = features.integrated_lufs(numpy.asarray(sig, numpy.float64)) if dd >= MIN_SHAPE_S else None
    return unscorable({"duration": dd, "lufs": lufs})


def _collect(paths: Sequence[str], looks: Any, use_ast: bool) -> tuple[list[dict], list[str]]:
    reels, skips = [], []
    listed: list[str] = []
    for pth in paths:
        found = audio.list_media([pth]) if os.path.exists(pth) or any(cand in pth for cand in "*?[") else []
        if not found:
            skips.append(f"skipped {pth}: not found (or no media files in it)")
        listed += found
    for ix, pth in enumerate(listed):
        reason = _skip_reason(pth)
        if reason:
            skips.append(f"skipped {os.path.basename(pth)}: {reason}")
            _warn(skips[-1])
            continue
        fv = film_metrics(pth, use_ast=use_ast)
        fv["id"], fv["style"] = f"ref_{len(reels) + 1}", style_of(looks, pth, ix)
        _warn(f"{fv['id']}: {fv['duration']:.1f} s, analysed in {fv['seconds']:.1f} s")
        reels.append(fv)
    return reels, skips


def _profile_summary(count: int, agg: dict, warnings: Sequence[str]) -> str:
    mat = agg["metrics"]

    def med(kk: str) -> str:
        return fmt_value(kk, mat[kk]["median"]) if mat[kk].get("n") else "n.a."

    pieces = [f"Profile of {count} film(s): {med('lufs_i')}"]
    if mat["lufs_i"].get("n"):
        pieces[0] += f" (p10-p90 {mat['lufs_i']['p10']:.1f} to {mat['lufs_i']['p90']:.1f})"
    pieces += [f"LRA {med('lra')}", f"onsets {med('onset_rate')}", f"salient hits {med('hit_rate')}"]
    if mat["music_frac"].get("n"):
        pieces.append(f"music {med('music_frac')}")
    if mat["cut_rate"].get("n"):
        pieces.append(f"visual events {med('cut_rate')}")
    return ", ".join(pieces) + "." + "".join(f" {win_n}" for win_n in warnings)


def _film_entry(fv: Mapping) -> dict:
    return {"id": fv["id"], "style": fv["style"], "duration": fv["duration"], "has_video": fv["video"] is not None,
            "metrics": fv["metrics"], "chance": fv["chance"], "arc": fv["arc"]}


def _profile_warnings(count: int, skips: list[str]) -> list[str]:
    warnings = [f"{len(skips)} file(s) skipped"] if skips else []
    if count < 3:
        warnings.append(f"only {count} film(s): the ranges are the min-max of so few values; use 5 or more references")
        _warn(warnings[-1])
    return warnings


def _write_profile(result: str, prof: dict) -> None:
    Path(result).parent.mkdir(parents=True, exist_ok=True)
    Path(result).write_text(json.dumps(report.clean(prof), indent=1), encoding="utf-8")


def profile(paths: str | Sequence[str], styles: Any = None, use_ast: bool = True, out: str | None = None) -> dict:
    """Measure reference films and store the spread of every metric."""
    paths = [paths] if isinstance(paths, str) else list(paths)
    reels, skips = _collect(paths, read_styles(styles), use_ast)
    if not reels:
        raise ProfileError("no usable reference films: " + "; ".join(skips) if skips else "no media files given")
    warnings = _profile_warnings(len(reels), skips)
    agg = aggregate(reels)
    prof = {"version": VERSION, "n": len(reels), **agg, "films": [_film_entry(fv) for fv in reels],
            "warnings": warnings, "summary": _profile_summary(len(reels), agg, warnings)}
    if out:
        _write_profile(out, prof)
    return prof


def load_profile(fpath: str | None = None) -> dict:
    """A profile file (default: the bundled launch profile of the timeline unit)."""
    return json.loads(Path(fpath or PROFILE_PATH).read_text(encoding="utf-8"))


# ================================================================ text and CLI


def profile_text(prof: dict) -> str:
    """Text output of profile."""
    lines = [prof["summary"]]
    lines += [f"  {fv['id']}  {fv['duration']:.1f} s  {fv['style'] or '-'}" for fv in prof["films"]]
    if prof.get("arc"):
        lines.append("arc (LU vs integrated): " + " ".join("n.a." if vec is None else f"{vec:+.1f}"
                                                          for vec in prof["arc"]["median"]))
    for kname, _, _, _, label in METRICS:
        sv = prof["metrics"][kname]
        if sv.get("n"):
            lines.append(f"  {label:<42} {kname:<16} {fmt_value(kname, sv['median']):>10}  "
                         f"{fmt_value(kname, sv['p10'])} .. {fmt_value(kname, sv['p90'])}  "
                         f"{fmt_value(kname, sv['min'])} .. {fmt_value(kname, sv['max'])}")
    return "\n".join(lines)


def _run(args: argparse.Namespace) -> int | None:
    import hark.models

    try:
        looks = read_styles(args.styles)
        if not args.no_ast:
            hark.models.require_ml("profile")
        prof = profile(args.refs, styles=looks, use_ast=not args.no_ast, out=args.o or args.out)
    except (ProfileError, audio.AudioError, json.JSONDecodeError) as exc:
        report.error_line("profile", str(exc))
        return 1
    args.out = None
    report.emit(prof, args, text=profile_text(prof))
    return 0


def add_cli(sub: argparse._SubParsersAction) -> None:
    """Register ``profile``."""
    pth = sub.add_parser("profile", help="A numeric sound profile of reference films.")
    pth.add_argument("refs", nargs="+", help="reference videos, soundtracks or folders")
    pth.add_argument("-o", default=None, help="write the profile JSON here (same as --out)")
    pth.add_argument("--styles", default=None, help="JSON: a list of styles in file order, or {basename: style}")
    pth.add_argument("--no-ast", action="store_true", help="skip the AudioSet pass (no music and speech presence)")
    report.add_common(pth)
    pth.set_defaults(func=_run)
