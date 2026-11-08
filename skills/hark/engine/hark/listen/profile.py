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

import numpy as np

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
    v = np.asarray(levels, np.float64)
    return float(10 * np.log10(np.mean(10 ** (v / 10))))


def fmt_value(key: str, v: float | None) -> str:
    """A metric value as text with its unit."""
    if v is None:
        return "n.a."
    unit = UNITS.get(key, "")
    if unit == "share":
        return f"{int(round(100 * v))}%"
    text = f"{v:.{DECIMALS.get(key, 2)}f}"
    return text if unit in ("0-1", "r") else f"{text} {unit}"


def _finite(v: Any) -> bool:
    return isinstance(v, (int, float)) and math.isfinite(v)


def _round_metrics(m: Mapping[str, Any]) -> dict:
    out = {}
    for k in METRIC_KEYS:
        v = m.get(k)
        if not _finite(v):
            out[k] = None
        elif DECIMALS[k] == 0:
            out[k] = int(round(v))
        else:
            out[k] = round(float(v), DECIMALS[k])
    return out


# ================================================================ media checks


def _probe_problem(info: Mapping[str, Any]) -> str | None:
    if info.get("probe_error"):
        return f"not a readable audio or video file ({str(info['probe_error'])[:160]})"
    return None if info.get("codec") else "no audio track"


def media_problem(path: str) -> str | None:
    """Why a path cannot be profiled (not found, folder, unreadable, no audio), or None."""
    if not os.path.exists(path):
        return "not found"
    return "a folder, not a file" if os.path.isdir(path) else _probe_problem(audio.probe(path))


def unscorable(metrics: Mapping[str, Any]) -> str | None:
    """'shorter than 3 s' or 'silent', or None (takes duration and lufs or lufs_i)."""
    d = metrics.get("duration")
    if d is not None and d < MIN_SHAPE_S:
        return f"shorter than {MIN_SHAPE_S:g} s ({d:.2f} s)"
    lufs = metrics.get("lufs_i", metrics.get("lufs"))
    if lufs is None or lufs <= SILENT_LUFS:
        return "silent"
    return None


# ================================================================ P1 audio pass


def _curve(x: np.ndarray, block: tuple[float, float]) -> list[list[float]]:
    t, lv = features.loudness_curve(x, win=block[0], hop=block[1])
    lv = np.asarray(lv, np.float64)
    return [np.asarray(t, np.float64).tolist(), np.where(np.isfinite(lv), lv, -120.0).tolist()]


def _onset_pass(mono: np.ndarray) -> dict:
    import librosa

    y = np.asarray(audio.resample(mono.astype(np.float32), SR, ONSET_SR), np.float32)
    if len(y) < 2048:
        return {"env": [], "onsets": [], "tempo": None, "beats": []}
    env = librosa.onset.onset_strength(y=y, sr=ONSET_SR, hop_length=HOP)
    on = librosa.onset.onset_detect(onset_envelope=env, sr=ONSET_SR, hop_length=HOP, units="time")
    tempo, beats = librosa.beat.beat_track(onset_envelope=env, sr=ONSET_SR, hop_length=HOP, units="time")
    return {"env": np.asarray(env, np.float64).tolist(), "onsets": [float(v) for v in on],
            "tempo": float(np.atleast_1d(tempo)[0]) if np.size(tempo) else None, "beats": [float(b) for b in beats]}


def to_attack(t: float, e2: np.ndarray) -> float:
    """Move a time to the steepest 2 ms rise from 60 ms before to 20 ms after it (P5)."""
    a = max(0, int(math.floor((t - 0.06) / E2_S)))
    b = min(len(e2), int(math.floor((t + 0.02) / E2_S)) + 1)
    if b - a < 2:
        return round(t, 4)
    rise = np.diff(e2[a:b])
    return round((a + 1 + int(np.argmax(rise))) * E2_S, 4)


def _e2(mono: np.ndarray) -> np.ndarray:
    return 20 * np.log10(np.asarray(features.frame_rms(mono, int(E2_S * SR)), np.float64) + 1e-9)


def audio_pass(path: str) -> dict:
    """Measurements, onsets, beats, loudness curves and 2 ms levels of a file (cached, P1)."""
    cpath = voc.cache_dir() / "profile" / f"a_{voc.key_of('profile-audio', audio.file_id(path), AUDIO_PASS_VERSION)}.json"
    hit = voc.load_json(cpath)
    if isinstance(hit, dict) and "duration" in hit:
        return hit
    x = np.asarray(audio.load(path), np.float64)
    mono = x.mean(axis=1) if x.ndim == 2 else x
    meas = features.measure(x, light=True)
    e2 = _e2(mono)
    op = _onset_pass(mono)
    op["onsets"] = [to_attack(t, e2) for t in op["onsets"]]
    ap = {"duration": len(mono) / SR, "measure": report.clean(meas), **op, "e2": e2.tolist(),
          "fast": _curve(x, FAST), "momentary": _curve(x, MOMENTARY), "short": _curve(x, SHORT_TERM)}
    voc.save_json(cpath, ap)
    return ap


def ast_pass(path: str, d: float) -> tuple[list[float], np.ndarray, Any]:
    """5 s window starts, their AudioSet probabilities and the tagger (cached, P2)."""
    from hark.models import ast

    model = ast.get()
    starts = [s for s in np.arange(0.0, max(d, 1e-9), AST_WIN_S) if s == 0 or d - s >= AST_MIN_REST_S]
    cpath = voc.cache_dir("ast") / f"pw_{voc.key_of('profile-ast', audio.file_id(path), AST_WIN_S, AST_VERSION)}.npy"
    P = voc.load_array(cpath, (len(starts), 527))
    if P is None:
        mono = audio.to_mono(np.asarray(audio.load(path), np.float32))
        segs = [mono[int(s * SR):int((s + AST_WIN_S) * SR)] for s in starts]
        P = np.asarray(model.probs(segs), np.float16)
        voc.save_array(cpath, P)
    return [float(s) for s in starts], np.asarray(P, np.float32), model


# ================================================================ P3 hits, P4 stop-downs


def _runs(idx: Sequence[int], gap: int = 2) -> list[list[int]]:
    runs: list[list[int]] = []
    for i in idx:
        if runs and i - runs[-1][-1] <= gap:
            runs[-1].append(i)
        else:
            runs.append([i])
    return runs


def _hit_time(run: list[int], t: np.ndarray, env: np.ndarray) -> float:
    a, b = t[run[0]] - 0.05, t[run[-1]] + 0.1
    ft = np.arange(len(env)) * HOP / ONSET_SR
    sel = np.nonzero((ft >= a) & (ft <= b))[0]
    return float(ft[sel[np.argmax(env[sel])]]) if len(sel) else float(t[run[0]] + 0.03)


def salient_hits(fast: Sequence[Sequence[float]], I: float, env: Sequence[float], e2: np.ndarray) -> list[float]:
    """Times where the 100 ms loudness jumps >= 6 dB within 80 ms and lands near the integrated level (P3)."""
    t, lv = np.asarray(fast[0]), np.asarray(fast[1])
    if len(lv) < 20:
        return []
    padded = np.concatenate([np.full(10, lv[0]), lv, np.full(3, lv[-1])])
    pre = np.array([padded[i:i + 10].min() for i in range(len(lv))])
    post = np.array([padded[i + 10:i + 14].max() for i in range(len(lv))])
    cand = np.nonzero((post - pre >= 6) & (post >= I - 8))[0]
    hits: list[float] = []
    for run in _runs(cand.tolist()):
        h = _hit_time(run, t, np.asarray(env, np.float64))
        if not hits or h - hits[-1] > 0.1:
            hits.append(h)
    return [round(to_attack(h, e2), 3) for h in hits]


def _stop_at(lv: np.ndarray, i: int, w: int) -> tuple[int, float, float] | None:
    ref = float(np.median(lv[i - w:i]))
    if lv[i] >= ref - 10:
        return None
    j = i
    while j < len(lv) and lv[j] < ref - 10:
        j += 1
    return j, ref, float(lv[i:j].min())


def stopdowns(fast: Sequence[Sequence[float]]) -> list[list[float]]:
    """Short drops >= 10 dB under the 2 s median, 0.15 to 1.5 s long, back within 3 dB in 0.3 s (P4)."""
    t, lv = np.asarray(fast[0]), np.asarray(fast[1])
    h, w = FAST[1], 100
    out: list[list[float]] = []
    if len(lv) < 3 or len(lv) <= w:
        return out
    i = w
    while i < len(lv):
        found = _stop_at(lv, i, w)
        if found:
            j, ref, low = found
            length = (j - i) * h
            if 0.15 <= length <= 1.5 and j < len(lv) and lv[j:j + 16].max() >= ref - 3:
                out.append([round(float(t[i]) - 0.05, 3), round(length, 3), round(ref - low, 1)])
                i = j + 25
                continue
        i += 1
    return out


# ================================================================ P6 metrics, P9 edges


def _edge(e2: np.ndarray, t: float, start: bool) -> float:
    a = max(0, int(math.floor((t - 0.05) / E2_S)))
    b = min(len(e2), int(math.floor((t + 0.05) / E2_S)) + 1)
    if b <= a:
        return round(max(0.0, t), 3)
    span = int(0.15 / E2_S)
    ref = float(e2[a:a + span].max()) if start else float(e2[max(0, b - span):b].max())
    ok = np.nonzero(e2[a:b] >= ref - 20)[0]
    if not len(ok):
        return round(max(0.0, t), 3)
    return round((a + int(ok[0])) * E2_S if start else (a + int(ok[-1]) + 1) * E2_S, 3)


def _arc(mt: np.ndarray, ml: np.ndarray, d: float, I: float) -> list[float | None]:
    half = max(1.5, 0.05 * d)
    out: list[float | None] = []
    for k in range(11):
        sel = np.abs(mt - k / 10 * d) <= half
        out.append(round(energy_mean(ml[sel]) - I, 1) if sel.any() else None)
    return out


def _series(ap: dict, name: str) -> tuple[np.ndarray, np.ndarray]:
    return np.asarray(ap[name][0]), np.asarray(ap[name][1])


def _mean_where(ml: np.ndarray, sel: np.ndarray, I: float) -> float | None:
    return energy_mean(ml[sel]) - I if sel.any() else None


def _intro_end(ap: dict, I: float, d: float) -> dict:
    mt, ml = _series(ap, "momentary")
    if len(mt) <= 2:
        return {}
    span = max(0.1 * d, 0.5)
    return {"intro_lu": _mean_where(ml, mt <= span, I), "end_lu": _mean_where(ml, mt >= d - span, I)}


def _peak_pos(ap: dict, d: float) -> dict:
    st, sl = _series(ap, "short")
    return {"peak_pos": float(st[int(np.argmax(sl))] / d)} if len(st) >= 2 and d >= 6 else {}


def _crest(m: Mapping[str, Any], I: float) -> dict:
    if m.get("short_term_max_lufs") is None or m.get("lufs") is None:
        return {}
    return {"crest_lu": m["short_term_max_lufs"] - I}


def _loudness_metrics(ap: dict, I: float, d: float) -> dict:
    m = ap["measure"]
    bands = m.get("bands_db") or {}
    return {"lufs_i": m.get("lufs"), "true_peak": m.get("true_peak_dbtp"), "lra": m.get("lra_lu"),
            "correlation": m.get("correlation"), "brightness_hz": m.get("brightness_hz"), **_crest(m, I),
            **_intro_end(ap, I, d), **_peak_pos(ap, d),
            **{f"band_{b}": bands.get(b) for b in ("sub", "low", "lowmid", "mid", "presence", "air")}}


def _tail(ap: dict, t_end: float) -> float | None:
    on, env = ap["onsets"], np.asarray(ap["env"], np.float64)
    if not on or not len(env):
        return None
    strength = np.array([env[min(len(env) - 1, int(round(t * ONSET_SR / HOP)))] for t in on])
    sel = [i for i, t in enumerate(on) if t_end - 4 <= t < t_end]
    if not sel:
        return None
    best = max(sel, key=lambda i: strength[i])
    return t_end - on[best] if strength[best] >= np.percentile(strength, 75) else None


def _longest_run(flags: np.ndarray) -> int:
    run = best = 0
    for g in flags:
        run = run + 1 if g else 0
        best = max(best, run)
    return best


def _max_gap(ft: np.ndarray, fl: np.ndarray, first: float, t_end: float, I: float) -> float:
    rng = (ft >= first) & (ft <= t_end)
    return _longest_run(fl[rng] < I - 30) * FAST[1] if rng.sum() > 1 else 0.0


def _fade_final(ap: dict, I: float, t_end: float) -> dict:
    mt, ml = _series(ap, "momentary")
    full = mt[(ml >= I - 6) & (mt <= t_end)]
    fin = ml[(mt >= t_end - 3) & (mt <= t_end)]
    out = {"final_lu": float(fin.max()) - I if len(fin) else None}
    if len(full):
        out["fade_s"] = max(0.0, t_end - float(full[-1]) - 0.2)
    return out


def _ending_metrics(ap: dict, I: float, d: float) -> dict:
    ft, fl = _series(ap, "fast")
    e2 = np.asarray(ap["e2"])
    out: dict[str, Any] = {}
    loud, aud = np.nonzero(fl > I - 20)[0], np.nonzero(fl > I - 30)[0]
    if len(loud):
        out["first_sound_s"] = _edge(e2, float(ft[loud[0]]), True)
    if not len(aud):
        return out
    t_end = min(d, _edge(e2, float(ft[aud[-1]]), False))
    out.update(end_silence_s=max(0.0, d - t_end), **_fade_final(ap, I, t_end),
               max_gap_s=_max_gap(ft, fl, out.get("first_sound_s") or 0.0, t_end, I), tail_s=_tail(ap, t_end))
    return out


def _per_s(n: int, d: float) -> float | None:
    return n / d if d else None


def _rates(ap: dict, hits: list[float], stops: list, d: float) -> dict:
    return {"onset_rate": _per_s(len(ap["onsets"]), d), "hit_rate": _per_s(len(hits), d),
            "stopdowns_min": len(stops) / d * 60 if d >= 6 else None,
            "tempo": ap["tempo"] if len(ap["beats"]) > 4 else None}


def integrated(m: Mapping[str, Any]) -> float:
    """The integrated loudness, or -70 when unknown."""
    return m.get("lufs") if m.get("lufs") is not None else -70.0


def is_mono(m: Mapping[str, Any], I: float) -> bool:
    return m.get("side_to_mid_db") is None and I > SILENT_LUFS


def film_core(ap: dict, hits: list[float], stops: list, d: float) -> tuple[dict, list[float] | None]:
    """The audio metrics and the arc of one film (P6)."""
    m = ap["measure"]
    I = integrated(m)
    out = {**_loudness_metrics(ap, I, d), **_ending_metrics(ap, I, d), **_rates(ap, hits, stops, d),
           "side_to_mid_db": MONO_DB if is_mono(m, I) else m.get("side_to_mid_db")}
    if d < MIN_FILM_S:
        out.update({k: None for k in SHAPE_METRICS})
    mt, ml = _series(ap, "momentary")
    return out, _arc(mt, ml, d, I) if len(mt) > 2 else None


# ================================================================ P7 sync


def near(V: Sequence[float], targets: Sequence[float], tol: float) -> float | None:
    """Share of events with a target no more than tol away (None without events, 0 without targets)."""
    if not len(V):
        return None
    if not len(targets):
        return 0.0
    T = np.asarray(targets, np.float64)
    return float(np.mean([np.min(np.abs(T - v)) <= tol + 1e-9 for v in V]))


def chance(V: Sequence[float], targets: Sequence[float], tol: float, d: float) -> float | None:
    """Mean of near() over the 16 fixed offsets, wrapped modulo the duration."""
    if not len(V):
        return None
    vals = [near([(v + o) % d for v in V], targets, tol) for o in CHANCE_OFFSETS]
    return float(np.mean(vals))


def sync_metrics(V: Sequence[float], ap: dict, hits: Sequence[float], fps: float, d: float) -> tuple[dict, dict]:
    """cut_rate, the 4 sync shares and their chance values."""
    out: dict[str, Any] = {"cut_rate": len(V) / d if d else None}
    chances: dict[str, Any] = {k: None for k in SYNC_KEYS}
    if len(V) < 3:
        out.update({k: None for k in SYNC_KEYS})
        return out, chances
    plan = {"cuts_with_sound": (ap["onsets"], 0.1), "cut_hit_frac": (hits, 0.1),
            "cut_frame_sync": (ap["onsets"], 1 / fps + 0.006), "cuts_on_beat": (ap["beats"], 0.07)}
    for k, (tg, tol) in plan.items():
        out[k] = near(V, tg, tol)
        c = chance(V, tg, tol, d)
        chances[k] = None if c is None else round(c, 2)
    return out, chances


def picture_events(src: str, d: float) -> tuple[float | None, list[float], list[float]]:
    """(fps, visual event times, their strengths) of a picture source."""
    from hark.timeline import video

    fps, diff = video.frame_diff(src)
    diff = np.asarray(diff, np.float64)
    idx = [i for i in video.diff_events(diff, fps) if i / fps < d]
    return float(fps), [round(i / fps, 3) for i in idx], [round(float(diff[i]), 3) for i in idx]


# ================================================================ film_metrics


def _picture_source(path: str, video: str | None) -> str | None:
    if video:
        return video
    from hark.timeline import video as vid

    return path if vid.has_video(path) else None


def _music(path: str, d: float, use_ast: bool) -> tuple[dict, list | None, float]:
    if not use_ast:
        return {"music_frac": None, "speech_frac": None}, None, 0.0
    t0 = time.perf_counter()
    starts, P, model = ast_pass(path, d)
    pm = [model.prob_of(p, "Music") for p in P]
    ps = [model.prob_of(p, "Speech") for p in P]
    win = [[round(s, 3), round(float(a), 3), round(float(b), 3)] for s, a, b in zip(starts, pm, ps)]
    shares = {"music_frac": float(np.mean(np.array(pm) >= MUSIC_P)) if pm else None,
              "speech_frac": float(np.mean(np.array(ps) >= SPEECH_P)) if ps else None}
    return shares, win, time.perf_counter() - t0


def _film_summary(path: str, r: dict) -> str:
    m = r["metrics"]
    parts = [f"{os.path.basename(path)}: {r['duration']:.1f} s", f"{fmt_value('lufs_i', m['lufs_i'])}",
             f"true peak {fmt_value('true_peak', m['true_peak'])}"]
    if m["lra"] is not None:
        parts.append(f"LRA {fmt_value('lra', m['lra'])}")
    if m["music_frac"] is not None:
        parts.append(f"music {fmt_value('music_frac', m['music_frac'])}")
    parts += [f"onsets {fmt_value('onset_rate', m['onset_rate'])}", f"hits {fmt_value('hit_rate', m['hit_rate'])}"]
    if m["cuts_with_sound"] is not None:
        parts.append(f"{len(r['events']['visual'])} visual events, {fmt_value('cuts_with_sound', m['cuts_with_sound'])}"
                     " with an onset within 100 ms")
    return ", ".join(parts) + "."


def _picture(path: str, video: str | None, ap: dict, hits: list[float], d: float) -> dict:
    """The picture source, its events, the sync metrics and their chance values."""
    t1 = time.perf_counter()
    src = _picture_source(path, video)
    if not src:
        return {"src": None, "fps": None, "V": [], "strength": [], "chances": {k: None for k in SYNC_KEYS},
                "sync": {k: None for k in ("cut_rate",) + SYNC_KEYS}, "seconds": time.perf_counter() - t1}
    fps, V, strength = picture_events(src, d)
    sync, chances = sync_metrics(V, ap, hits, fps, d)
    return {"src": src, "fps": fps, "V": V, "strength": strength, "sync": sync, "chances": chances,
            "seconds": time.perf_counter() - t1}


def _events(ap: dict, hits: list[float], stops: list, pic: dict, windows: list | None) -> dict:
    return {"hits": hits, "visual": pic["V"], "visual_strength": pic["strength"], "stopdowns": stops,
            "beats": [round(b, 3) for b in ap["beats"]], "music_windows": windows, "onsets_n": len(ap["onsets"])}


def _media_info(path: str, ap: dict) -> dict:
    info = audio.probe(path)
    m = ap["measure"]
    return {"codec": info.get("codec"), "lossy": bool(info.get("lossy")), "mono": is_mono(m, integrated(m))}


def film_metrics(path: str, video: str | None = None, use_ast: bool = True) -> dict:
    """Every profile metric of one film or soundtrack."""
    t0 = time.perf_counter()
    ap = audio_pass(path)
    d = ap["duration"]
    hits = salient_hits(ap["fast"], integrated(ap["measure"]), ap["env"], np.asarray(ap["e2"]))
    stops = stopdowns(ap["fast"])
    metrics, arc = film_core(ap, hits, stops, d)
    t_audio = time.perf_counter() - t0
    shares, windows, t_ast = _music(path, d, use_ast)
    pic = _picture(path, video, ap, hits, d)
    metrics.update(**shares, **pic["sync"])
    r = {"path": path, "video": pic["src"], "duration": round(d, 3), "fps": pic["fps"], **_media_info(path, ap),
         "short": d < MIN_FILM_S, "metrics": _round_metrics(metrics), "chance": pic["chances"], "arc": arc,
         "events": _events(ap, hits, stops, pic, windows),
         "timing": {"audio": round(t_audio, 2), "audioset": round(t_ast, 2), "picture": round(pic["seconds"], 2)}}
    r["seconds"] = round(time.perf_counter() - t0, 2)
    r["summary"] = _film_summary(path, r)
    return r


# ================================================================ P8 aggregate and profile


def _stats(values: Sequence[float | None]) -> dict:
    v = [float(x) for x in values if _finite(x)]
    if not v:
        return {"n": 0}
    p10, med, p90 = np.percentile(v, [10, 50, 90])
    return {"n": len(v), "median": round(float(med), 3), "p10": round(float(p10), 3), "p90": round(float(p90), 3),
            "min": round(min(v), 3), "max": round(max(v), 3),
            "values": [round(float(x), 3) if _finite(x) else None for x in values]}


def _arc_stats(arcs: Sequence[Sequence[float | None] | None]) -> dict | None:
    rows = [a for a in arcs if a]
    if not rows:
        return None
    out: dict[str, list] = {"median": [], "p10": [], "p90": []}
    for k in range(11):
        vals = [r[k] for r in rows if r[k] is not None]
        for name, q in (("median", 50), ("p10", 10), ("p90", 90)):
            out[name].append(round(float(np.percentile(vals, q)), 1) if vals else None)
    return out


def aggregate(films: Sequence[Mapping]) -> dict:
    """Per-metric stats, sync chance stats and arc percentiles over the films (P8)."""
    return {"metrics": {k: _stats([f["metrics"].get(k) for f in films]) for k in METRIC_KEYS},
            "chance": {k: _stats([f["chance"].get(k) for f in films]) for k in SYNC_KEYS},
            "arc": _arc_stats([f.get("arc") for f in films])}


def _styles_file(p: Path) -> Any:
    if not p.exists():
        raise ProfileError(f"styles file {p} does not exist")
    try:
        return json.loads(p.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        raise ProfileError(f"styles file {p} is not valid JSON: {exc}") from exc


def read_styles(styles: Any) -> Any:
    """A styles option: None, a list, a dict, or a JSON file path holding one of them."""
    if styles is None or isinstance(styles, (list, dict)):
        return styles
    data = _styles_file(Path(str(styles)))
    if not isinstance(data, (list, dict)):
        raise ProfileError("styles must be a JSON list or object")
    return data


def style_of(styles: Any, path: str, i: int) -> str:
    """The style of the i-th listed file."""
    if isinstance(styles, dict):
        return str(styles.get(os.path.basename(path), ""))
    if isinstance(styles, list) and i < len(styles):
        return str(styles[i])
    return ""


def _warn(text: str) -> None:
    print(f"hark profile: {text}", file=sys.stderr)


def _skip_reason(path: str) -> str | None:
    problem = media_problem(path)
    if problem:
        return problem
    try:
        x = audio.load(path)
    except Exception as exc:  # a decode error skips the file
        return f"cannot decode ({voc.first_line(exc, 160)})"
    d = len(x) / SR
    lufs = features.integrated_lufs(np.asarray(x, np.float64)) if d >= MIN_SHAPE_S else None
    return unscorable({"duration": d, "lufs": lufs})


def _collect(paths: Sequence[str], styles: Any, use_ast: bool) -> tuple[list[dict], list[str]]:
    films, skips = [], []
    listed: list[str] = []
    for p in paths:
        found = audio.list_media([p]) if os.path.exists(p) or any(c in p for c in "*?[") else []
        if not found:
            skips.append(f"skipped {p}: not found (or no media files in it)")
        listed += found
    for i, p in enumerate(listed):
        reason = _skip_reason(p)
        if reason:
            skips.append(f"skipped {os.path.basename(p)}: {reason}")
            _warn(skips[-1])
            continue
        f = film_metrics(p, use_ast=use_ast)
        f["id"], f["style"] = f"ref_{len(films) + 1}", style_of(styles, p, i)
        _warn(f"{f['id']}: {f['duration']:.1f} s, analysed in {f['seconds']:.1f} s")
        films.append(f)
    return films, skips


def _profile_summary(n: int, agg: dict, warnings: Sequence[str]) -> str:
    m = agg["metrics"]

    def med(k: str) -> str:
        return fmt_value(k, m[k]["median"]) if m[k].get("n") else "n.a."

    parts = [f"Profile of {n} film(s): {med('lufs_i')}"]
    if m["lufs_i"].get("n"):
        parts[0] += f" (p10-p90 {m['lufs_i']['p10']:.1f} to {m['lufs_i']['p90']:.1f})"
    parts += [f"LRA {med('lra')}", f"onsets {med('onset_rate')}", f"salient hits {med('hit_rate')}"]
    if m["music_frac"].get("n"):
        parts.append(f"music {med('music_frac')}")
    if m["cut_rate"].get("n"):
        parts.append(f"visual events {med('cut_rate')}")
    return ", ".join(parts) + "." + "".join(f" {w}" for w in warnings)


def profile(paths: str | Sequence[str], styles: Any = None, use_ast: bool = True, out: str | None = None) -> dict:
    """Measure reference films and store the spread of every metric."""
    paths = [paths] if isinstance(paths, str) else list(paths)
    styles = read_styles(styles)
    films, skips = _collect(paths, styles, use_ast)
    if not films:
        raise ProfileError("no usable reference films: " + "; ".join(skips) if skips else "no media files given")
    warnings = [f"{len(skips)} file(s) skipped"] if skips else []
    if len(films) < 3:
        warnings.append(f"only {len(films)} film(s): the ranges are the min-max of so few values; use 5 or more "
                        "references")
        _warn(warnings[-1])
    agg = aggregate(films)
    prof = {"version": VERSION, "n": len(films), **agg,
            "films": [{"id": f["id"], "style": f["style"], "duration": f["duration"], "has_video": f["video"] is not None,
                       "metrics": f["metrics"], "chance": f["chance"], "arc": f["arc"]} for f in films],
            "warnings": warnings, "summary": _profile_summary(len(films), agg, warnings)}
    if out:
        Path(out).parent.mkdir(parents=True, exist_ok=True)
        Path(out).write_text(json.dumps(report.clean(prof), indent=1), encoding="utf-8")
    return prof


def load_profile(path: str | None = None) -> dict:
    """A profile file (default: the bundled launch profile of the timeline unit)."""
    return json.loads(Path(path or PROFILE_PATH).read_text(encoding="utf-8"))


# ================================================================ text and CLI


def profile_text(prof: dict) -> str:
    """Text output of profile."""
    lines = [prof["summary"]]
    lines += [f"  {f['id']}  {f['duration']:.1f} s  {f['style'] or '-'}" for f in prof["films"]]
    if prof.get("arc"):
        lines.append("arc (LU vs integrated): " + " ".join("n.a." if v is None else f"{v:+.1f}"
                                                          for v in prof["arc"]["median"]))
    for key, _, _, _, label in METRICS:
        s = prof["metrics"][key]
        if s.get("n"):
            lines.append(f"  {label:<42} {key:<16} {fmt_value(key, s['median']):>10}  "
                         f"{fmt_value(key, s['p10'])} .. {fmt_value(key, s['p90'])}  "
                         f"{fmt_value(key, s['min'])} .. {fmt_value(key, s['max'])}")
    return "\n".join(lines)


def _run(args: argparse.Namespace) -> int | None:
    import hark.models

    try:
        styles = read_styles(args.styles)
        if not args.no_ast:
            hark.models.require_ml("profile")
        prof = profile(args.refs, styles=styles, use_ast=not args.no_ast, out=args.o or args.out)
    except (ProfileError, audio.AudioError, json.JSONDecodeError) as exc:
        report.error_line("profile", str(exc))
        return 1
    args.out = None
    report.emit(prof, args, text=profile_text(prof))
    return 0


def add_cli(sub: argparse._SubParsersAction) -> None:
    """Register ``profile``."""
    p = sub.add_parser("profile", help="A numeric sound profile of reference films.")
    p.add_argument("refs", nargs="+", help="reference videos, soundtracks or folders")
    p.add_argument("-o", default=None, help="write the profile JSON here (same as --out)")
    p.add_argument("--styles", default=None, help="JSON: a list of styles in file order, or {basename: style}")
    p.add_argument("--no-ast", action="store_true", help="skip the AudioSet pass (no music and speech presence)")
    report.add_common(p)
    p.set_defaults(func=_run)
