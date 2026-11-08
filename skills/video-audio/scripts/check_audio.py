#!/usr/bin/env python3
"""Phase 3 audio gate for a video project: checks audio/mix.wav before render.

Usage:
  check_audio.py <audio/mix.wav> [--timeline timeline.json] [--beats audio/beats.json]
                 [--sync-sheet audio/sync-sheet.json] [--sfx-cues audio/sfx-cues.json]
                 [--sfx-bus audio/sfx-bus.wav] [--bed audio/bed.wav] [--music audio/music.wav]
                 [--bed-beats audio/bed.wav]
                 [--out qa/audio-check.json]
  check_audio.py --measure-sfx audio/sfx
  check_audio.py --self-test

Exit codes: 0 every gate passes, 1 a gate fails, 2 usage or tool error.
Needs ffmpeg and ffprobe on PATH. Python standard library only.
"""

import argparse
import array
import functools
import json
import math
import os
import re
import shutil
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True
from beat_grid import BeatError, measure as measure_beats

SAMPLE_RATE = 48000
CHANNELS = 2
DURATION_TOLERANCE_SEC = 0.05
LUFS_MIN = -15.0
LUFS_MAX = -13.0
TRUE_PEAK_MAX_DBTP = -2.0
CUT_PEAK_MAX_DBFS = -3.0
CUT_PEAK_RADIUS_SEC = 0.020
BED_BEAT_TOLERANCE_SEC = 0.020
CHECK_INPUTS = ("timeline_path", "beats_path", "sync_path", "cues_path", "bus", "bed", "music",
                "bed_beats")
EDGE_SILENCE_MAX_SEC = 0.20
DROPOUT_MAX_SEC = 0.80
ALLOWED_SILENCE_SLACK_SEC = 0.05
EDGE_EPSILON_SEC = 0.01
SILENCE_FLOOR_DB = -50.0
SILENCE_FILTER = f"ebur128=peak=true:framelog=info,silencedetect=n={SILENCE_FLOOR_DB:g}dB:d=0.2"
SYNC_EPSILON_SEC = 0.01
SFX_SYNC_TOLERANCE_SEC = 0.04
SFX_BAND_MARGIN_DB = 3.0
SYNC_FLOOR_DB = 40.0
ENVELOPE_WINDOW_SEC = 0.005
HERO_LOUDNESS_SLACK_LU = 0.5
MOMENTARY_WINDOW_SEC = 0.4
LEVEL_WINDOW_SEC = 0.1
BREAKDOWN_MAX_DIP_DB = 12.0
REMOVED_SILENCE_KEY = "payoff_silence_sec"
REMOVED_FIELD_MESSAGE = (f"meta.{REMOVED_SILENCE_KEY} is removed: do not plan silence before the payoff. "
                         f"Delete it, or use the optional meta.payoff_breakdown_sec "
                         f"(a dip of at most {BREAKDOWN_MAX_DIP_DB:g} dB)")
MUSIC_GRID_MESSAGE = "--music needs --beats or --sync-sheet for the bar grid"
DROP_MIN_JUMP_DB = 6.0
ANALYSIS_RATE = 16000
BANDS = {"low": "lowpass=f=150", "mid": "highpass=f=150,lowpass=f=2000",
         "high": "highpass=f=2000"}
SYNC_POINTS = ("onset", "peak", "end")
SKILL_MD_MAX_BYTES = 12288
REFERENCE_MAX_BYTES = 24576
TEXT_SUFFIXES = (".md", ".py", ".txt")


class ToolError(Exception):
    """ffmpeg, ffprobe or an input file cannot be used."""


def _run(cmd):
    try:
        return subprocess.run(cmd, capture_output=True, text=True, check=False)
    except FileNotFoundError as exc:
        raise ToolError(f"{cmd[0]} is not on PATH") from exc


def probe(path):
    """Return sample rate, channels and duration of the first audio stream."""
    proc = _run([
        "ffprobe", "-v", "error", "-select_streams", "a:0",
        "-show_entries", "stream=sample_rate,channels:format=duration",
        "-of", "json", path,
    ])
    if proc.returncode != 0:
        raise ToolError(f"ffprobe cannot read {path}: {proc.stderr.strip()}")
    data = json.loads(proc.stdout or "{}")
    streams = data.get("streams") or []
    if not streams:
        raise ToolError(f"{path} has no audio stream")
    return {
        "sample_rate": int(streams[0].get("sample_rate", 0)),
        "channels": int(streams[0].get("channels", 0)),
        "duration_sec": float(data.get("format", {}).get("duration", 0.0)),
    }


def _db(text):
    return -math.inf if text == "-inf" else float(text)


def _summary_levels(text):
    """Read integrated loudness and true peak from ebur128 summary lines.

    Returns (lufs, peak) as floats; a value the text does not hold is None.
    """
    lufs = peak = None
    heading = ""
    for raw in text.splitlines():
        label, _, rest = raw.strip().partition(":")
        fields = rest.split()
        if not fields:
            heading = label
        elif label == "I" and fields[1:] == ["LUFS"]:
            lufs = float(fields[0])
        elif label == "Peak" and heading == "True peak" and fields[1:] == ["dBFS"]:
            peak = float(fields[0])
    return lufs, peak


def measure(path):
    """Run ebur128 and silencedetect. Return loudness, true peak and silences."""
    proc = _run([
        "ffmpeg", "-hide_banner", "-nostats", "-i", path,
        "-af", SILENCE_FILTER, "-f", "null", "-",
    ])
    if proc.returncode != 0:
        raise ToolError(f"ffmpeg cannot analyse {path}: {proc.stderr.strip()[-400:]}")
    log = proc.stderr
    _, found, block = log.rpartition("Summary:")
    lufs, peak = _summary_levels(block if found else "")
    if lufs is None or peak is None:
        raise ToolError("ebur128 summary is missing from the ffmpeg log")
    silences = []
    start = None
    for line in log.splitlines():
        found = re.search(r"silence_start: (-?[\d.]+)", line)
        if found:
            start = max(0.0, float(found.group(1)))
            continue
        found = re.search(r"silence_end: (-?[\d.]+)", line)
        if found and start is not None:
            silences.append([start, float(found.group(1))])
            start = None
    momentary = [(float(t), _db(m)) for t, m in re.findall(
        r"\] t: ([\d.]+)\s+TARGET:\S+ LUFS\s+M:\s*(-?[\d.]+|-inf)", log)]
    return {
        "integrated_lufs": lufs,
        "true_peak_dbtp": peak,
        "silences": silences,
        "momentary": momentary,
    }


def _err(errors, rule, message):
    errors.append({"rule": rule, "message": message})


def _err_if(errors, condition, rule, message):
    """Add the error only when condition is true."""
    errors.extend([{"rule": rule, "message": message}] if condition else [])


def _load_json(path):
    try:
        with open(path, encoding="utf-8") as handle:
            return json.load(handle)
    except (OSError, ValueError) as exc:
        raise ToolError(f"cannot read JSON {path}: {exc}") from exc


def _allowed(timeline):
    meta = timeline.get("meta", {}) if isinstance(timeline, dict) else {}
    ranges = meta.get("allowed_silences", []) if isinstance(meta, dict) else []
    out = []
    for item in ranges if isinstance(ranges, list) else []:
        if isinstance(item, list) and len(item) == 2 and all(_is_num(v) for v in item):
            out.append((float(item[0]), float(item[1])))
    return out


def _is_num(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value)


def _meta_range(timeline, key):
    """Return (start, end) of meta.<key> as [start_sec, end_sec], None when absent, or "bad"."""
    meta = timeline.get("meta", {}) if isinstance(timeline, dict) else {}
    value = meta.get(key) if isinstance(meta, dict) else None
    if value is None:
        return None
    if (isinstance(value, list) and len(value) == 2 and all(_is_num(v) for v in value)
            and value[0] < value[1]):
        return float(value[0]), float(value[1])
    return "bad"


def check_silences(silences, duration, allowed, errors):
    for start, end in silences:
        length = end - start
        if start <= EDGE_EPSILON_SEC:
            if length > EDGE_SILENCE_MAX_SEC:
                _err(errors, "silence.leading",
                     f"leading silence is {length:.3f} s; the limit is {EDGE_SILENCE_MAX_SEC} s")
        elif end >= duration - EDGE_EPSILON_SEC:
            if length > EDGE_SILENCE_MAX_SEC:
                _err(errors, "silence.trailing",
                     f"trailing silence is {length:.3f} s; the limit is {EDGE_SILENCE_MAX_SEC} s")
        elif length > DROPOUT_MAX_SEC and not any(
            start >= lo - ALLOWED_SILENCE_SLACK_SEC and end <= hi + ALLOWED_SILENCE_SLACK_SEC
            for lo, hi in allowed
        ):
            _err(errors, "silence.dropout",
                 f"silence below -50 dBFS from {start:.3f} s to {end:.3f} s "
                 f"({length:.3f} s) is not in meta.allowed_silences")


def check_beats(beats, duration, errors):
    if not isinstance(beats, dict):
        _err(errors, "beats.schema", "beats.json must be a JSON object")
        return
    bpm = beats.get("bpm")
    if not _is_num(bpm) or bpm <= 0:
        _err(errors, "beats.bpm", f"bpm must be a number > 0, got {bpm!r}")
    for key in ("beats_sec", "downbeats_sec"):
        values = beats.get(key)
        if not isinstance(values, list) or not values or not all(_is_num(v) for v in values):
            _err(errors, "beats.schema", f"{key} must be a non-empty list of numbers")
            continue
        if any(b <= a for a, b in zip(values, values[1:])):
            _err(errors, "beats.order", f"{key} is not strictly increasing")
        if values[0] < 0 or values[-1] > duration + DURATION_TOLERANCE_SEC:
            _err(errors, "beats.range", f"{key} leaves [0, {duration:.3f}] s")


# ---------------------------------------------------------------- sync sheet

def _narration_text(scene):
    value = scene.get("narration")
    if isinstance(value, dict):
        value = value.get("text", "")
    return value.strip() if isinstance(value, str) else ""


def _is_word(item):
    return (isinstance(item, dict) and isinstance(item.get("word"), str) and item["word"]
            and _is_num(item.get("start_sec")) and _is_num(item.get("end_sec")))


def _check_words(seg, errors):
    words = seg.get("words")
    if not isinstance(words, list) or not all(_is_word(w) for w in words):
        _err(errors, "sync.schema",
             f"segment {seg['scene_id']}: words must be a list of {{word, start_sec, end_sec}}")
        return
    floor = -math.inf
    for word in words:
        if word["start_sec"] > word["end_sec"] or word["start_sec"] < floor - SYNC_EPSILON_SEC:
            _err(errors, "sync.word_order",
                 f"segment {seg['scene_id']}: word {word['word']!r} at {word['start_sec']} s "
                 "is out of order")
        if (word["start_sec"] < seg["start_sec"] - SYNC_EPSILON_SEC
                or word["end_sec"] > seg["end_sec"] + SYNC_EPSILON_SEC):
            _err(errors, "sync.word_in_segment",
                 f"segment {seg['scene_id']}: word {word['word']!r} leaves "
                 f"[{seg['start_sec']}, {seg['end_sec']}] s")
        floor = max(floor, word["end_sec"])


def _check_segment(seg, scenes, errors):
    if not isinstance(seg, dict) or seg.get("scene_id") not in scenes:
        _err(errors, "sync.schema", "each vo_segments entry needs a scene_id from timeline.json")
        return None
    if not (_is_num(seg.get("start_sec")) and _is_num(seg.get("end_sec"))
            and seg["start_sec"] < seg["end_sec"]):
        _err(errors, "sync.schema", f"segment {seg['scene_id']}: start_sec < end_sec is required")
        return None
    scene = scenes[seg["scene_id"]]
    lo = scene.get("start_sec", 0.0)
    hi = lo + scene.get("duration_sec", 0.0) if _is_num(lo) else None
    if hi is None or not _is_num(scene.get("duration_sec")):
        _err(errors, "sync.schema", f"scene {seg['scene_id']} has no start_sec and duration_sec")
    elif seg["start_sec"] < lo - SYNC_EPSILON_SEC or seg["end_sec"] > hi + SYNC_EPSILON_SEC:
        _err(errors, "sync.segment_in_scene",
             f"segment {seg['scene_id']} [{seg['start_sec']}, {seg['end_sec']}] s leaves "
             f"its scene [{lo}, {hi}] s")
    _check_words(seg, errors)
    return seg["scene_id"]


def check_sync_sheet(sheet, timeline, duration, errors):
    """Validate audio/sync-sheet.json against the reconciled timeline."""
    if not isinstance(sheet, dict):
        _err(errors, "sync.schema", "sync-sheet.json must be a JSON object")
        return
    if sheet.get("timing") not in ("estimated", "measured"):
        _err(errors, "sync.schema", f"timing must be estimated or measured, got {sheet.get('timing')!r}")
    beat_errors = []
    check_beats(sheet, duration, beat_errors)
    errors.extend({"rule": "sync." + e["rule"], "message": e["message"]} for e in beat_errors)
    scene_list = timeline.get("scenes", []) if isinstance(timeline, dict) else []
    scenes = {s.get("id"): s for s in scene_list if isinstance(s, dict)}
    segments = sheet.get("vo_segments")
    if not isinstance(segments, list):
        _err(errors, "sync.schema", "vo_segments must be a list")
        return
    covered = {_check_segment(seg, scenes, errors) for seg in segments}
    for scene_id, scene in scenes.items():
        if _narration_text(scene) and scene_id not in covered:
            _err(errors, "sync.missing", f"scene {scene_id} has narration and no vo_segments entry")


# ---------------------------------------------------------------- sample analysis

def _decode_cmd(path, audio_filter, rate):
    """Return the ffmpeg command that decodes path to mono s16le at rate."""
    if not shutil.which("ffmpeg"):
        raise ToolError("ffmpeg is not on PATH")
    filters = ["-af", audio_filter] if audio_filter else []
    return (["ffmpeg", "-hide_banner", "-v", "error", "-i", path] + filters
            + ["-ac", "1", "-ar", str(rate), "-f", "s16le", "-"])


def _s16(raw):
    """Return signed 16-bit little-endian samples from raw bytes."""
    samples = array.array("h")
    samples.frombytes(raw[:len(raw) // 2 * 2])
    if sys.byteorder == "big":
        samples.byteswap()
    return samples


def _pcm(path, audio_filter=None, rate=ANALYSIS_RATE):
    """Decode path to mono signed 16-bit samples at the selected rate."""
    proc = subprocess.run(_decode_cmd(path, audio_filter, rate), capture_output=True, check=False)
    tail = proc.stderr.decode(errors="replace")[-400:]
    if proc.returncode != 0:
        raise ToolError(f"ffmpeg cannot decode {path}: {tail}")
    return _s16(proc.stdout)


def _rms_db(samples, start_sec, end_sec):
    lo = max(0, int(start_sec * ANALYSIS_RATE))
    hi = len(samples) if math.isinf(end_sec) else min(len(samples), math.ceil(end_sec * ANALYSIS_RATE))
    if hi <= lo:
        return -math.inf
    power = sum(v * v for v in samples[lo:hi]) / (hi - lo)
    return 10 * math.log10(power / (32768.0 * 32768.0)) if power > 0 else -math.inf


def _levels(samples, window_sec):
    count = int(len(samples) / (window_sec * ANALYSIS_RATE))
    return [_rms_db(samples, k * window_sec, (k + 1) * window_sec) for k in range(count)]


def _cut_times(timeline):
    """Return the sorted scene start times after 0 s."""
    scenes = timeline.get("scenes", []) if isinstance(timeline, dict) else []
    starts = (scene.get("start_sec") for scene in scenes if isinstance(scene, dict))
    return sorted({float(start) for start in starts if _is_num(start) and start > 0})


def _peak_dbfs(samples, sec):
    """Return the sample peak in dBFS within CUT_PEAK_RADIUS_SEC of sec."""
    lo = max(0, int((sec - CUT_PEAK_RADIUS_SEC) * SAMPLE_RATE))
    hi = math.ceil((sec + CUT_PEAK_RADIUS_SEC) * SAMPLE_RATE)
    peak = max((abs(value) for value in samples[lo:hi]), default=0) / 32768.0
    return 20 * math.log10(peak) if peak else -math.inf


def _cut_result(samples, cut, errors):
    """Check the peak at 1 cut and return its metric."""
    peak = _peak_dbfs(samples, cut)
    message = (f"peak within {CUT_PEAK_RADIUS_SEC * 1000:.0f} ms of the cut at {cut:.3f} s is "
               f"{peak:.2f} dBFS; it must be <= {CUT_PEAK_MAX_DBFS} dBFS")
    if peak > CUT_PEAK_MAX_DBFS:
        _err(errors, "mix.cut_peak", message)
    return {"sec": cut, "peak_dbfs": None if math.isinf(peak) else round(peak, 2)}


def check_cut_peaks(mix, timeline, errors):
    """Check the sample peak within 20 ms on each scene cut."""
    cuts = _cut_times(timeline)
    samples = _pcm(mix, rate=SAMPLE_RATE) if cuts else []
    return [_cut_result(samples, cut, errors) for cut in cuts]


def check_bed_beats(path, timeline, errors):
    """Measure path and check every timeline cut against its nearest beat."""
    measured = measure_beats(path)
    entries = [_bed_beat_entry(cut, measured["beats_sec"]) for cut in _cut_times(timeline)]
    worst = max(entries, key=lambda entry: entry["drift_sec"], default=None)
    _check_worst_bed_beat(worst, errors)
    return {"tempo_bpm": measured["tempo_bpm"], "cuts": entries,
            "worst_drift_sec": worst["drift_sec"] if worst else 0.0,
            "worst_cut_sec": worst["cut_sec"] if worst else None}


def _bed_beat_entry(cut, beats):
    """Return the nearest measured beat and drift for 1 cut."""
    nearest = min(beats, key=functools.partial(_beat_distance, cut))
    return {"cut_sec": cut, "nearest_beat_sec": nearest,
            "drift_sec": round(abs(cut - nearest), 6)}


def _beat_distance(cut, beat):
    """Return the absolute distance between a cut and a beat."""
    return abs(beat - cut)


def _check_worst_bed_beat(worst, errors):
    """Record an error when the worst cut exceeds the tolerance."""
    if not worst or worst["drift_sec"] <= BED_BEAT_TOLERANCE_SEC:
        return
    _err(errors, "bed.beat_alignment",
         f"worst cut drift is {worst['drift_sec']:.3f} s at {worst['cut_sec']:.3f} s; "
         f"the nearest measured bed beat is {worst['nearest_beat_sec']:.3f} s "
         f"(tolerance {BED_BEAT_TOLERANCE_SEC:.3f} s)")


def sync_points(path):
    """Return onset, peak and end of a rendered effect, in seconds from its first sample."""
    levels = _levels(_pcm(path), ENVELOPE_WINDOW_SEC)
    if not levels or max(levels) == -math.inf:
        raise ToolError(f"{path} is silent")
    peak = max(range(len(levels)), key=levels.__getitem__)
    loud = [k for k, v in enumerate(levels) if v >= levels[peak] - SYNC_FLOOR_DB]
    return {"onset_sec": round(loud[0] * ENVELOPE_WINDOW_SEC, 3),
            "peak_sec": round((peak + 0.5) * ENVELOPE_WINDOW_SEC, 3),
            "end_sec": round((loud[-1] + 1) * ENVELOPE_WINDOW_SEC, 3)}


def measure_sfx(folder):
    """Return {file name: sync points} for every .wav in folder."""
    names = sorted(n for n in os.listdir(folder) if n.endswith(".wav"))
    return {name: sync_points(os.path.join(folder, name)) for name in names}


# ---------------------------------------------------------------- effects

def _cue_list(cues):
    items = cues.get("cues") if isinstance(cues, dict) else cues
    return items if isinstance(items, list) else None


def _cue_file(cue, root):
    path = cue.get("file") or os.path.join("audio", "sfx", f"{cue.get('kind')}.wav")
    return path if os.path.isabs(path) else os.path.join(root, path)


def _dominant_band(path):
    return max(BANDS, key=lambda band: _rms_db(_pcm(path, BANDS[band]), 0, math.inf))


class _BandCache:
    """Band-filtered samples of the SFX bus and the bed, decoded once per band."""

    def __init__(self, bus, bed):
        self.paths = {"bus": bus, "bed": bed}
        self.cache = {}

    def level(self, which, band, start, end):
        key = (which, band)
        if key not in self.cache:
            self.cache[key] = _pcm(self.paths[which], BANDS[band])
        return _rms_db(self.cache[key], start, end)


def _check_cue(cue, root, points, bands, errors):
    """Check 1 cue. Return its metrics entry, or None when the cue is malformed."""
    if not (isinstance(cue, dict) and _is_num(cue.get("time_sec")) and _is_num(cue.get("start_sec"))):
        _err(errors, "sfx.schema", f"cue {cue!r} needs time_sec and start_sec in seconds")
        return None
    sync = cue.get("sync") or ("end" if cue.get("kind") == "riser" else "peak")
    path = _cue_file(cue, root)
    if sync not in SYNC_POINTS or not os.path.isfile(path):
        _err(errors, "sfx.schema", f"cue at {cue['time_sec']} s: sync must be onset, peak or "
                                   f"end, and {path} must exist")
        return None
    if path not in points:
        points[path] = dict(sync_points(path), band=_dominant_band(path) if bands else None)
    p = points[path]
    landed = cue["start_sec"] + p[f"{sync}_sec"]
    entry = {"time_sec": cue["time_sec"], "kind": cue.get("kind"), "sync": sync,
             "landed_sec": round(landed, 3)}
    if abs(landed - cue["time_sec"]) > SFX_SYNC_TOLERANCE_SEC:
        _err(errors, "sfx.sync", f"{cue.get('kind')} {sync} lands at {landed:.3f} s; the cue is "
                                 f"at {cue['time_sec']:.3f} s (tolerance {SFX_SYNC_TOLERANCE_SEC} s)")
    if bands:
        lo, hi = cue["start_sec"] + p["onset_sec"], cue["start_sec"] + p["end_sec"]
        margin = bands.level("bus", p["band"], lo, hi) - bands.level("bed", p["band"], lo, hi)
        entry.update(band=p["band"], margin_db=None if math.isinf(margin) else round(margin, 2))
        if margin < SFX_BAND_MARGIN_DB:
            _err(errors, "sfx.margin", f"{cue.get('kind')} at {cue['time_sec']:.3f} s is "
                                       f"{margin:.1f} dB over the bed in the {p['band']} band; "
                                       f"it must be >= {SFX_BAND_MARGIN_DB} dB")
    return entry


def check_sfx(cues_path, bus, bed, errors):
    """Check placement by measured sync point, and the margin over the bed. Return metrics."""
    cues = _cue_list(_load_json(cues_path))
    if cues is None:
        _err(errors, "sfx.schema", "sfx-cues.json must be a list of cues or {\"cues\": [...]}")
        return []
    root = os.path.dirname(os.path.dirname(os.path.abspath(cues_path)))
    bands = _BandCache(bus, bed) if bus and bed else None
    points = {}
    entries = (_check_cue(cue, root, points, bands, errors) for cue in cues)
    return [e for e in entries if e]


# ---------------------------------------------------------------- arc

def check_hero(momentary, hero, errors):
    """The loudest momentary loudness of the mix lands on meta.hero_sec."""
    values = [m for _, m in momentary if m != -math.inf]
    inside = [m for t, m in momentary if hero[0] + 0.1 <= t <= hero[1] + MOMENTARY_WINDOW_SEC]
    loudest = max(values) if values else -math.inf
    best = max(inside) if inside else -math.inf
    if best < loudest - HERO_LOUDNESS_SLACK_LU:
        _err(errors, "mix.hero_loudest",
             f"the loudest moment is {loudest:.1f} LUFS; on the hero {list(hero)} s it is only "
             f"{best:.1f} LUFS (slack {HERO_LOUDNESS_SLACK_LU} LU)")
    return {"loudest_lufs": round(loudest, 2), "hero_lufs": None if math.isinf(best) else round(best, 2)}


def _median(values):
    ordered = sorted(values)
    return ordered[len(ordered) // 2] if ordered else -math.inf


def _outside_window(levels, planned):
    """Return the finite 0.1 s levels that lie fully outside the planned window."""
    def outside(k):
        return (k + 1) * LEVEL_WINDOW_SEC <= planned[0] or k * LEVEL_WINDOW_SEC >= planned[1]
    return [v for k, v in enumerate(levels) if v != -math.inf and outside(k)]


def _silent_runs(silences, planned):
    """Return the silencedetect runs that overlap the planned window, rounded."""
    return [[round(a, 3), round(b, 3)] for a, b in silences if a < planned[1] and b > planned[0]]


def _silent_message(planned, silent):
    return (f"the breakdown {list(planned)} s is silent (under {SILENCE_FLOOR_DB:g} dBFS) at "
            f"{silent} s; a breakdown dips, it never stops")


def _dip_message(planned, dip):
    return (f"the breakdown {list(planned)} s dips {dip:.1f} dB under the rest of the mix; "
            f"the limit is {BREAKDOWN_MAX_DIP_DB:g} dB")


def _breakdown_problem(planned, silent, levels):
    """Return the error message for a breakdown, or None when it passes."""
    dip = levels["dip_db"]
    is_silent = bool(silent) or levels["breakdown_dbfs"] is None
    too_deep = dip is not None and dip > BREAKDOWN_MAX_DIP_DB
    deep = _dip_message(planned, dip) if too_deep else None
    return _silent_message(planned, silent) if is_silent else deep


def check_payoff_breakdown(mix, planned, silences, errors):
    """meta.payoff_breakdown_sec is a dip of <= 12 dB under the rest of the mix, never silent.

    The dip is the median 0.1 s level outside the window minus the RMS level of the window.
    Silent means a silencedetect run (under SILENCE_FLOOR_DB) overlaps the window.
    """
    samples = _pcm(mix)
    reference = _median(_outside_window(_levels(samples, LEVEL_WINDOW_SEC), planned))
    inside = _rms_db(samples, planned[0], planned[1])
    levels = {"reference_dbfs": _round_db(reference), "breakdown_dbfs": _round_db(inside),
              "dip_db": _round_db(reference - inside)}
    problem = _breakdown_problem(planned, _silent_runs(silences, planned), levels)
    if problem:
        _err(errors, "mix.payoff_breakdown", problem)
    return levels


def _round_db(value):
    return None if math.isinf(value) or math.isnan(value) else round(value, 2)


def check_drop(music, beats, hero, errors):
    """Find the drop: the bar with the largest rise in low-band energy. It lands on the hero."""
    downbeats = beats.get("downbeats_sec") if isinstance(beats, dict) else None
    bpm = beats.get("bpm") if isinstance(beats, dict) else None
    if not (isinstance(downbeats, list) and len(downbeats) >= 2
            and all(_is_num(v) for v in downbeats) and _is_num(bpm) and bpm > 0):
        _err(errors, "music.drop", "the drop check needs bpm and at least 2 downbeats_sec")
        return None
    low = _pcm(music, BANDS["low"])
    edges = list(downbeats) + [2 * downbeats[-1] - downbeats[-2]]
    bars = [_rms_db(low, a, b) for a, b in zip(edges, edges[1:])]
    jumps = [(b - a if a != -math.inf else (math.inf if b != -math.inf else 0.0), k + 1)
             for k, (a, b) in enumerate(zip(bars, bars[1:]))]
    jump, bar = max(jumps)
    out = {"drop_sec": downbeats[bar], "jump_db": None if math.isinf(jump) else round(jump, 2),
           "bar_low_db": [None if math.isinf(v) else round(v, 2) for v in bars]}
    if hero and jump >= DROP_MIN_JUMP_DB and abs(downbeats[bar] - hero[0]) > 60.0 / bpm + 1e-6:
        _err(errors, "music.drop",
             f"the drop of the bed is at {downbeats[bar]:.3f} s; the hero starts at "
             f"{hero[0]:.3f} s (tolerance 1 beat, {60.0 / bpm:.3f} s)")
    return out


# ---------------------------------------------------------------- gate

def _check_format(info, timeline, has_timeline, errors):
    if info["sample_rate"] != SAMPLE_RATE:
        _err(errors, "format.sample_rate",
             f"sample rate is {info['sample_rate']} Hz; it must be {SAMPLE_RATE} Hz")
    if info["channels"] != CHANNELS:
        _err(errors, "format.channels", f"channels is {info['channels']}; it must be {CHANNELS}")
    if has_timeline:
        target = (timeline.get("meta") or {}).get("duration_sec") if isinstance(timeline, dict) else None
        if not _is_num(target):
            _err(errors, "format.duration", "timeline.json meta.duration_sec is missing")
        elif abs(info["duration_sec"] - target) > DURATION_TOLERANCE_SEC:
            _err(errors, "format.duration",
                 f"mix is {info['duration_sec']:.3f} s; timeline is {target:.3f} s "
                 f"(tolerance {DURATION_TOLERANCE_SEC} s)")


def _check_loudness(metrics, errors):
    lufs = metrics["integrated_lufs"]
    if not LUFS_MIN <= lufs <= LUFS_MAX:
        _err(errors, "loudness.integrated",
             f"integrated loudness is {lufs} LUFS; it must be in [{LUFS_MIN}, {LUFS_MAX}] LUFS")
    peak = metrics["true_peak_dbtp"]
    if peak > TRUE_PEAK_MAX_DBTP:
        _err(errors, "loudness.true_peak",
             f"true peak is {peak} dBTP; it must be <= {TRUE_PEAK_MAX_DBTP} dBTP")


def _has_music(timeline, errors):
    """meta.bpm (optional, a number > 0) says that the film has music."""
    meta = timeline.get("meta", {}) if isinstance(timeline, dict) else {}
    bpm = meta.get("bpm") if isinstance(meta, dict) else None
    if bpm is None:
        return False
    if not _is_num(bpm) or bpm <= 0:
        _err(errors, "plan.bpm", f"meta.bpm must be a number > 0, got {bpm!r}")
        return False
    return True


def _range_problem(key, value, required):
    """Return the error message for meta.<key>, or None when it is valid."""
    bad = f"meta.{key} must be [start_sec, end_sec] with start_sec < end_sec"
    missing = f"meta.{key} is required when meta.bpm says the film has music" if required else None
    return bad if value == "bad" else (missing if value is None else None)


def _plan_range(timeline, key, rule, required, errors):
    """Return meta.<key> as (start, end) or None. Report a bad or a missing required range."""
    value = _meta_range(timeline, key)
    message = _range_problem(key, value, required)
    _err_if(errors, message is not None, rule, message)
    return None if value == "bad" else value


def _reject_removed_field(timeline, errors):
    """Reject meta.payoff_silence_sec: no gate plans silence before the payoff."""
    present = _meta_range(timeline, REMOVED_SILENCE_KEY) is not None
    _err_if(errors, present, "plan.removed_field", REMOVED_FIELD_MESSAGE)


def _plan_ranges(timeline, errors):
    """Return (hero_sec, payoff_breakdown_sec), each (start, end) or None."""
    music = _has_music(timeline, errors)
    _reject_removed_field(timeline, errors)
    return (_plan_range(timeline, "hero_sec", "mix.hero_loudest", music, errors),
            _plan_range(timeline, "payoff_breakdown_sec", "mix.payoff_breakdown", False, errors))


def _music_drop_metrics(beats, music, hero, errors):
    """With --music, return {"music_drop": ...} for the drop of the bed on the bar grid."""
    _err_if(errors, bool(music) and beats is None, "music.drop", MUSIC_GRID_MESSAGE)
    can_measure = bool(music) and beats is not None
    return {"music_drop": check_drop(music, beats, hero, errors)} if can_measure else {}


def _hero_metrics(metrics, hero, errors):
    """Return {"hero": ...} when the plan has a hero, else {}."""
    return {"hero": check_hero(metrics["momentary"], hero, errors)} if hero else {}


def _breakdown_metrics(mix, metrics, breakdown, errors):
    """Return {"payoff_breakdown": ...} when the plan has a breakdown, else {}."""
    found = check_payoff_breakdown(mix, breakdown, metrics["silences"], errors) if breakdown else None
    return {"payoff_breakdown": found} if breakdown else {}


def _check_plan(mix, metrics, timeline, errors):
    """Check the hero and the breakdown of the plan. Return the hero range."""
    hero, breakdown = _plan_ranges(timeline, errors)
    metrics.update(_hero_metrics(metrics, hero, errors))
    metrics.update(_breakdown_metrics(mix, metrics, breakdown, errors))
    return hero


def _check_arc(mix, metrics, timeline, beats, music, errors):
    """Check the hero, the breakdown and the music drop."""
    metrics.update(_music_drop_metrics(beats, music, _check_plan(mix, metrics, timeline, errors), errors))


def _check_mix(mix, timeline_path, errors):
    """Check the format, the loudness and the cut peaks of the mix."""
    info = probe(mix)
    metrics = {**info, **measure(mix)}
    timeline = _load_json(timeline_path) if timeline_path else {}
    _check_format(info, timeline, bool(timeline_path), errors)
    _check_loudness(metrics, errors)
    metrics["cut_peaks"] = check_cut_peaks(mix, timeline, errors)
    return metrics, timeline


def _check_sheet(sync_path, timeline, duration, errors):
    """Check the sync sheet and return it, or None without a path."""
    sheet = _load_json(sync_path) if sync_path else None
    if sync_path:
        check_sync_sheet(sheet, timeline, duration, errors)
    return sheet


def _check_grid(inputs, timeline, duration, errors):
    """Check beats.json and the sync sheet, and return the bar grid."""
    beats = _load_json(inputs["beats_path"]) if inputs["beats_path"] else None
    if inputs["beats_path"]:
        check_beats(beats, duration, errors)
    sheet = _check_sheet(inputs["sync_path"], timeline, duration, errors)
    return beats if beats is not None else sheet


def _cue_metrics(inputs, errors):
    """Check the SFX cues and return their metric, or {} without a cue file."""
    cues = inputs["cues_path"]
    sfx = check_sfx(cues, inputs["bus"], inputs["bed"], errors) if cues else None
    return {"sfx": sfx} if cues else {}


def _check_grid_and_cues(inputs, metrics, timeline, errors):
    """Check the bar grid, then the SFX cues, and return the bar grid."""
    beats = _check_grid(inputs, timeline, metrics["duration_sec"], errors)
    metrics.update(_cue_metrics(inputs, errors))
    if inputs["bed_beats"]:
        metrics["bed_beats"] = check_bed_beats(inputs["bed_beats"], timeline, errors)
    return beats


def _check_all(mix, inputs):
    """Run every check that inputs select and return the report dict."""
    errors = []
    metrics, timeline = _check_mix(mix, inputs["timeline_path"], errors)
    _check_arc(mix, metrics, timeline, _check_grid_and_cues(inputs, metrics, timeline, errors),
               inputs["music"], errors)
    check_silences(metrics["silences"], metrics["duration_sec"], _allowed(timeline), errors)
    return _report(mix, metrics, errors)


def check(mix, *paths, **named):
    """Return the report dict. Raises ToolError when an input cannot be read.

    paths and named give the CHECK_INPUTS, in that order or by name.
    """
    inputs = dict.fromkeys(CHECK_INPUTS)
    inputs.update(zip(CHECK_INPUTS, paths), **named)
    return _check_all(mix, inputs)


def _finite(value):
    """Return value, or None when it is infinite."""
    return None if math.isinf(value) else value


def _report(mix, metrics, errors):
    """Return the report dict with JSON-safe loudness metrics."""
    del metrics["momentary"]
    metrics.update({key: _finite(metrics[key]) for key in ("integrated_lufs", "true_peak_dbtp")})
    return {"passed": not errors, "file": mix, "errors": errors, "metrics": metrics,
            "notes": ["Keep the WAV at or below -2.0 dBTP because AAC adds about +1 dB.",
                      "video-render measures the encoded MP4 at or below -1.0 dBTP."]}


def write_report(report, out):
    if not out:
        return
    parent = os.path.dirname(out)
    if parent:
        os.makedirs(parent, exist_ok=True)
    with open(out, "w", encoding="utf-8") as handle:
        json.dump(report, handle, indent=2)
        handle.write("\n")


def _parser():
    """Return the command-line parser."""
    parser = argparse.ArgumentParser(description="Check audio/mix.wav against the Phase 3 gates.")
    _add_gate_arguments(parser)
    _add_mode_arguments(parser)
    return parser


def _add_gate_arguments(parser):
    """Add audio-gate input arguments."""
    parser.add_argument("mix", nargs="?", help="path to audio/mix.wav")
    parser.add_argument("--timeline", help="timeline.json with meta.duration_sec")
    parser.add_argument("--beats", help="audio/beats.json with bpm, beats_sec, downbeats_sec")
    parser.add_argument("--sync-sheet", help="audio/sync-sheet.json to validate (needs --timeline)")
    parser.add_argument("--sfx-cues", help="audio/sfx-cues.json: check each sync point")
    parser.add_argument("--sfx-bus", help="audio/sfx-bus.wav: with --bed, check the margin")
    parser.add_argument("--bed", help="audio/bed.wav: the ducked music bed alone")
    parser.add_argument("--music", help="audio/music.wav: find the drop by low-band energy")
    parser.add_argument("--bed-beats", metavar="BED_WAV",
                        help="measure final bed beats and check every timeline cut within 20 ms")


def _add_mode_arguments(parser):
    """Add output and alternate-mode arguments."""
    parser.add_argument("--measure-sfx", help="print the sync points of every .wav in a folder")
    parser.add_argument("--out", help="write the JSON report here, e.g. qa/audio-check.json")
    parser.add_argument("--self-test", action="store_true", help="run the built-in tests")


def _validate_cli(parser, args):
    """Reject incomplete combinations of command-line options."""
    _require_mix_argument(parser, args)
    _require_sync_timeline(parser, args)
    _require_bed_timeline(parser, args)


def _require_mix_argument(parser, args):
    """Require the mix path in gate mode."""
    missing = not args.mix
    if missing:
        parser.error("give a mix path, --measure-sfx or --self-test")
    return args.mix


def _require_sync_timeline(parser, args):
    """Require a timeline when checking a sync sheet."""
    missing = args.sync_sheet and not args.timeline
    if missing:
        parser.error("--sync-sheet needs --timeline")
    return args.timeline


def _require_bed_timeline(parser, args):
    """Require a timeline when checking final-bed beats."""
    missing = args.bed_beats and not args.timeline
    if missing:
        parser.error("--bed-beats needs --timeline")
    return args.timeline


def _print_sfx(folder):
    """Print measured effect sync points and return success."""
    print(json.dumps(measure_sfx(folder), indent=2))
    return 0


class _InputErrorReport:
    """On an input error inside the block, write a passed: false report to out."""

    def __init__(self, out, mix):
        self.out = out
        self.mix = mix

    def __enter__(self):
        return self

    def __exit__(self, kind, exc, trace):
        failed = isinstance(exc, (BeatError, ToolError, OSError))
        if failed:
            write_report(_input_report(exc, self.mix), self.out)
        return False


def _gate_report(args):
    """Run check() on the CLI inputs. An input error writes passed: false to --out."""
    inputs = (args.mix, args.timeline, args.beats, args.sync_sheet, args.sfx_cues,
              args.sfx_bus, args.bed, args.music, args.bed_beats)
    with _InputErrorReport(args.out, args.mix):
        return check(*inputs)


def _run_gate(args):
    """Run the audio gate, write its report, and return its exit code.

    An input error writes a passed: false report to --out and returns 2.
    """
    report = _gate_report(args)
    write_report(report, args.out)
    print(json.dumps(report, indent=2))
    return 0 if report["passed"] else 1


def _run_cli(parser, args):
    """Run the command selected by parsed arguments."""
    self_test_requested = args.self_test
    sfx_folder = args.measure_sfx
    if self_test_requested:
        return self_test()
    if sfx_folder:
        return _print_sfx(sfx_folder)
    _remove_stale_report(args.out)
    _validate_cli(parser, args)
    return _run_gate(args)


def main(argv=None):
    parser = _parser()
    return _run_cli(parser, parser.parse_args(argv))


def _remove_stale_report(out):
    """Delete an earlier --out report, so a failed run never leaves an old pass."""
    path = out or ""
    stale = bool(path) and os.path.lexists(path)
    if stale:
        os.remove(path)


def _input_report(exc, mix=None):
    """Return the passed: false report for an input error."""
    return {"passed": False, "file": mix, "errors": [{"rule": "input", "message": str(exc)}]}


def _input_error(exc):
    """Print an input error and return exit code 2."""
    print(json.dumps(_input_report(exc)))
    return 2


# ---------------------------------------------------------------- self-test

def _synth(path, expr, seconds, target_lufs, rate=SAMPLE_RATE, channels=CHANNELS):
    """Render an aevalsrc expression, then set its integrated loudness to target_lufs."""
    raw = path + ".raw.wav"
    exprs = "|".join([expr] * channels)
    proc = _run([
        "ffmpeg", "-hide_banner", "-v", "error", "-y", "-f", "lavfi",
        "-i", f"aevalsrc=exprs='{exprs}':s={rate}:d={seconds}",
        "-c:a", "pcm_s16le", raw,
    ])
    if proc.returncode != 0:
        raise ToolError(proc.stderr)
    gain = target_lufs - measure(raw)["integrated_lufs"]
    proc = _run([
        "ffmpeg", "-hide_banner", "-v", "error", "-y", "-i", raw,
        "-af", f"volume={gain:.3f}dB", "-c:a", "pcm_s16le", path,
    ])
    os.remove(raw)
    if proc.returncode != 0:
        raise ToolError(proc.stderr)


def _write(path, data):
    with open(path, "w", encoding="utf-8") as handle:
        json.dump(data, handle)


def _rules(report):
    return sorted({e["rule"] for e in report["errors"]})


def _render(path, expr, seconds):
    """Render an aevalsrc expression as 48 kHz stereo, with no loudness change."""
    os.makedirs(os.path.dirname(path), exist_ok=True)
    proc = _run(["ffmpeg", "-hide_banner", "-v", "error", "-y", "-f", "lavfi", "-i",
                 f"aevalsrc=exprs='{expr}|{expr}':s={SAMPLE_RATE}:d={seconds}",
                 "-c:a", "pcm_s16le", path])
    if proc.returncode != 0:
        raise ToolError(proc.stderr)


def _sheet():
    def words(items):
        return [{"word": w, "start_sec": a, "end_sec": b} for w, a, b in items]
    return {"version": 1, "timing": "estimated", "bpm": 120,
            "beats_sec": [0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5], "downbeats_sec": [0.0, 2.0],
            "vo_segments": [
                {"scene_id": "s01", "start_sec": 0.15, "end_sec": 1.5, "timing": "estimated",
                 "words": words([("Ship", 0.2, 0.5), ("every", 0.5, 0.9), ("frame.", 0.9, 1.4)])},
                {"scene_id": "s02", "start_sec": 2.15, "end_sec": 3.5, "timing": "estimated",
                 "words": words([("Try", 2.2, 2.5), ("it", 2.5, 2.7), ("today.", 2.7, 3.3)])}]}


def _sync_tests(p, expect, failures):
    _write(p("sync-timeline.json"), {"meta": {"duration_sec": 4.0}, "scenes": [
        {"id": "s01", "start_sec": 0.0, "duration_sec": 2.0, "narration": "Ship every frame."},
        {"id": "s02", "start_sec": 2.0, "duration_sec": 2.0, "narration": "Try it today."}]})
    _write(p("sheet.json"), _sheet())
    expect("pass a valid sync sheet",
           check(p("good.wav"), p("sync-timeline.json"), sync_path=p("sheet.json")), [])
    cases = {
        "sync.word_order": lambda s: s["vo_segments"][0]["words"].reverse(),
        "sync.word_in_segment": lambda s: s["vo_segments"][1]["words"][2].update(end_sec=3.8),
        "sync.segment_in_scene": lambda s: s["vo_segments"][0].update(end_sec=2.4),
        "sync.schema": lambda s: s.update(timing="guessed"),
        "sync.missing": lambda s: s["vo_segments"].pop(),
        "sync.beats.order": lambda s: s.update(beats_sec=[0.0, 1.0, 0.5]),
    }
    timeline = _load_json(p("sync-timeline.json"))
    for rule, change in cases.items():
        sheet = _sheet()
        change(sheet)
        errors = []
        check_sync_sheet(sheet, timeline, 4.0, errors)
        got = _rules({"errors": errors})
        print(f"{'ok  ' if got == [rule] else 'FAIL'} sync sheet fails only {rule}: {got}")
        if got != [rule]:
            failures.append(f"sync sheet: want [{rule}], got {got}")


def _sfx_tests(p, expect):
    sfx = p("proj/audio/sfx")
    # riser: 1.0 s rising sweep with a hard stop, so its end is at 1.0 s.
    _render(os.path.join(sfx, "riser.wav"), "0.5*t*sin(2*PI*(200+400*t)*t)", 1.0)
    # tick: silent, then a burst at 0.20 to 0.22 s, so its peak is near 0.21 s.
    _render(os.path.join(sfx, "tick.wav"), "if(between(t,0.2,0.22),0.5*sin(2*PI*2000*t),0)", 0.3)
    good = [{"time_sec": 2.0, "kind": "riser", "start_sec": 1.0},
            {"time_sec": 3.0, "kind": "tick", "start_sec": 2.79}]
    _write(p("proj/audio/sfx-cues.json"), good)
    expect("pass effects placed by measured sync point",
           check(p("good.wav"), cues_path=p("proj/audio/sfx-cues.json")), [])
    _write(p("proj/audio/sfx-cues.json"), [dict(good[0], start_sec=2.0), good[1]])
    expect("fail a riser placed by its first sample",
           check(p("good.wav"), cues_path=p("proj/audio/sfx-cues.json")), ["sfx.sync"])

    _render(p("proj/bed.wav"), "0.05*sin(2*PI*1000*t)", 4)
    for name, amp in (("loud", 0.3), ("quiet", 0.02)):
        _render(os.path.join(sfx, f"chime-{name}.wav"), f"{amp}*sin(2*PI*1320*t)", 0.5)
        _render(p(f"proj/bus-{name}.wav"),
                f"if(between(t,1.0,1.5),{amp}*sin(2*PI*1320*t),0)", 4)
        _write(p("proj/audio/sfx-cues.json"), [{"time_sec": 1.0, "kind": f"chime-{name}",
                                                 "start_sec": 1.0, "sync": "onset"}])
        report = check(p("good.wav"), cues_path=p("proj/audio/sfx-cues.json"),
                       bus=p(f"proj/bus-{name}.wav"), bed=p("proj/bed.wav"))
        if name == "loud":
            expect("pass an effect 15 dB over the bed", report, [])
        else:
            expect("fail an effect under the bed in its band", report, ["sfx.margin"])


def _arc_tests(p, expect):
    tone = "0.2*sin(2*PI*440*t)"
    _synth(p("hero.wav"), f"{tone}*if(between(t,2.5,3.0),2,1)", 4, -14.0)
    _write(p("hero-ok.json"), {"meta": {"duration_sec": 4.0, "hero_sec": [2.4, 3.2]}})
    expect("pass the loudest moment on the hero", check(p("hero.wav"), p("hero-ok.json")), [])
    _write(p("hero-bad.json"), {"meta": {"duration_sec": 4.0, "hero_sec": [0.5, 1.0]}})
    expect("fail the loudest moment off the hero",
           check(p("hero.wav"), p("hero-bad.json")), ["mix.hero_loudest"])

    _breakdown_dip_tests(p, expect, tone)
    _breakdown_plan_tests(p, expect, tone)
    _drop_tests(p, expect, tone)


def _drop_tests(p, expect, tone):
    """The drop of the bed lands on the hero start."""
    _synth(p("good8.wav"), tone, 8, -14.0)
    _render(p("music.wav"), "if(lt(t,4),0.02,0.5)*sin(2*PI*60*t)", 8)
    _write(p("beats8.json"), {"bpm": 120, "beats_sec": [k * 0.5 for k in range(16)],
                              "downbeats_sec": [0.0, 2.0, 4.0, 6.0]})
    for name, hero, rules in (("on", [4.0, 5.0], []), ("off", [6.0, 7.0], ["music.drop"])):
        _drop_case(p, expect, name, hero, rules)


def _drop_case(p, expect, name, hero, rules):
    """Check the drop of music.wav against 1 hero range."""
    _write(p(f"drop-{name}.json"), {"meta": {"duration_sec": 8.0, "hero_sec": hero}})
    report = check(p("good8.wav"), p(f"drop-{name}.json"), p("beats8.json"), music=p("music.wav"))
    expect(f"{'pass' if not rules else 'fail'} the drop {name} the hero", report, rules)


def _breakdown_dip_tests(p, expect, tone):
    """A breakdown dips <= 12 dB and is never silent."""
    _write(p("breakdown.json"), {"meta": {"duration_sec": 5.0, "payoff_breakdown_sec": [2.0, 2.6]}})
    # Gains 0.5, 0.1 and 0 are dips of 6 dB, 20 dB and silence.
    cases = (("6db", 0.5, []), ("20db", 0.1, ["mix.payoff_breakdown"]),
             ("silent", 0, ["mix.payoff_breakdown"]))
    for name, gain, rules in cases:
        _dip_case(p, expect, tone, (name, gain, rules))


def _dip_case(p, expect, tone, case):
    """Render a 5 s tone with the breakdown at gain, and check it."""
    name, gain, rules = case
    _synth(p(f"breakdown-{name}.wav"), f"{tone}*if(between(t,2.0,2.6),{gain},1)", 5, -14.0)
    report = check(p(f"breakdown-{name}.wav"), p("breakdown.json"))
    expect(f"{'pass' if not rules else 'fail'} a breakdown dip of {name}", report, rules)


def _breakdown_plan_tests(p, expect, tone):
    """No payoff field is required, the old silence field fails, and bpm needs a hero."""
    _write(p("music.json"), {"meta": {"duration_sec": 4.0, "bpm": 120, "hero_sec": [1.5, 2.5]}})
    _synth(p("music-hero.wav"), f"{tone}*if(between(t,1.6,2.4),2,1)", 4, -14.0)
    expect("pass meta.bpm with no payoff field", check(p("music-hero.wav"), p("music.json")), [])
    _removed_field_test(p, expect)


def _removed_field_test(p, expect):
    """The old payoff silence field fails, and meta.bpm without a hero fails."""
    _write(p("old-silence.json"), {"meta": {"duration_sec": 4.0, REMOVED_SILENCE_KEY: [1.0, 1.5]}})
    expect("reject the removed payoff silence field",
           check(p("good.wav"), p("old-silence.json")), ["plan.removed_field"])
    _write(p("music-nohero.json"), {"meta": {"duration_sec": 4.0, "bpm": 120}})
    expect("fail meta.bpm without hero_sec",
           check(p("good.wav"), p("music-nohero.json")), ["mix.hero_loudest"])


def _cli_report_tests(p, failures):
    """--out never keeps a stale pass: exit 1 writes false, an input error writes false."""
    out = p("qa/audio-check.json")
    os.makedirs(os.path.dirname(out), exist_ok=True)
    _cli_case(out, ("CLI exit 1 and --out report", [p("quiet.wav")], 1), failures)
    _cli_case(out, ("CLI input error over a passing report writes passed: false",
                    [p("good.wav"), "--timeline", p("missing.json")], 2), failures)


def _cli_over_stale(out, args):
    """Run the CLI with --out over a stale passing report. Return (exit code, report)."""
    _write(out, {"passed": True})
    cmd = [sys.executable, os.path.abspath(__file__), *args, "--out", out]
    code = subprocess.run(cmd, capture_output=True, check=False).returncode
    return code, (_load_json(out) if os.path.isfile(out) else {})


def _cli_case(out, case, failures):
    """The CLI exits with the wanted code and leaves passed: false with an error."""
    name, args, want = case
    code, report = _cli_over_stale(out, args)
    ok = code == want and report.get("passed") is False and bool(report.get("errors"))
    print(f"{'ok  ' if ok else 'FAIL'} {name}")
    failures.extend([] if ok else [f"{name}: exit {code}, report {report}"])


def _bed_beat_tests(p, expect, failures):
    """Check aligned cuts and a cut list that drifts 0.15 s by its end."""
    click = "if(lt(mod(t,0.5),0.01),0.8*sin(2*PI*1000*t),0)"
    bed = p("bed-click.wav")
    _render(bed, click, 4)
    _aligned_bed_case(p, expect, bed)
    _drifting_bed_case(p, expect, failures, bed)


def _bed_timeline(p, name, cuts):
    """Write a 4 s timeline with the selected cut times."""
    scenes = [{"id": f"s{k}", "start_sec": cut} for k, cut in enumerate([0.0, *cuts])]
    path = p(name)
    _write(path, {"meta": {"duration_sec": 4.0}, "scenes": scenes})
    return path


def _aligned_bed_case(p, expect, bed):
    """Check cuts aligned to measured final-bed beats."""
    timeline = _bed_timeline(p, "bed-aligned.json", [1.0, 2.0, 3.0])
    expect("pass cuts aligned to measured final-bed beats",
           check(p("good.wav"), timeline, bed_beats=bed), [])


def _drifting_bed_case(p, expect, failures, bed):
    """Check cuts that drift 0.15 s by the final cut."""
    timeline = _bed_timeline(p, "bed-drift.json", [1.05, 2.10, 3.15])
    report = check(p("good.wav"), timeline, bed_beats=bed)
    expect("fail a cut list that drifts 0.15 s by the end", report, ["bed.beat_alignment"])
    worst = report["metrics"]["bed_beats"]
    ok = worst["worst_drift_sec"] >= 0.149 and worst["worst_cut_sec"] == 3.15
    print(f"{'ok  ' if ok else 'FAIL'} report worst bed-beat drift and location: {worst}")
    if not ok:
        failures.append(f"wrong worst bed-beat drift: {worst}")


def self_test():
    here = os.path.dirname(os.path.abspath(__file__))
    skill_dir = os.path.dirname(here)
    failures = []

    def expect(name, report, rules):
        got = _rules(report)
        ok = report["passed"] if not rules else set(rules) <= set(got) and not report["passed"]
        print(f"{'ok  ' if ok else 'FAIL'} {name}: rules={got}")
        if not ok:
            failures.append(f"{name}: want {rules}, got {got} {report['metrics']}")

    if not shutil.which("ffmpeg") or not shutil.which("ffprobe"):
        print("FAIL ffmpeg and ffprobe must be on PATH")
        return 1

    tone = "0.2*sin(2*PI*440*t)"
    with tempfile.TemporaryDirectory(prefix="check-audio-") as tmp:
        def p(name):
            return os.path.join(tmp, name)

        _synth(p("good.wav"), tone, 4, -14.0)
        _write(p("timeline.json"), {"meta": {"duration_sec": 4.0}})
        _write(p("beats.json"), {"bpm": 120, "beats_sec": [0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5],
                                  "downbeats_sec": [0.0, 2.0]})
        expect("pass -14 LUFS stereo", check(p("good.wav"), p("timeline.json"), p("beats.json")), [])

        _synth(p("quiet.wav"), tone, 4, -20.0)
        expect("fail -20 LUFS", check(p("quiet.wav")), ["loudness.integrated"])

        burst = f"{tone}+if(between(t,1.5,1.52),0.8*sin(2*PI*3000*t),0)"
        _synth(p("hot.wav"), burst, 4, -14.0)
        expect("fail true peak > -2.0 dBTP", check(p("hot.wav")), ["loudness.true_peak"])

        _render(p("cut-hot.wav"),
                "0.12*sin(2*PI*440*t)+if(between(t,1.99,2.01),0.85*sin(2*PI*3000*t),0)", 4)
        _write(p("cuts.json"), {"meta": {"duration_sec": 4.0}, "scenes": [
            {"id": "a", "start_sec": 0.0}, {"id": "b", "start_sec": 2.0}]})
        expect("fail a cut peak above -3.0 dBFS", check(p("cut-hot.wav"), p("cuts.json")),
               ["mix.cut_peak"])
        expect("pass cut peaks below -3.0 dBFS", check(p("good.wav"), p("cuts.json")), [])

        gap = f"if(between(t,1.8,3.0),0,{tone})"
        _synth(p("gap.wav"), gap, 5, -14.0)
        expect("fail 1.2 s internal dropout", check(p("gap.wav")), ["silence.dropout"])
        _write(p("allowed.json"), {"meta": {"duration_sec": 5.0, "allowed_silences": [[1.8, 3.0]]}})
        expect("pass allowed silence range", check(p("gap.wav"), p("allowed.json")), [])

        _synth(p("lead.wav"), f"if(lt(t,0.5),0,{tone})", 4, -14.0)
        expect("fail 0.5 s leading silence", check(p("lead.wav")), ["silence.leading"])
        _synth(p("tail.wav"), f"if(gt(t,3.5),0,{tone})", 4, -14.0)
        expect("fail 0.5 s trailing silence", check(p("tail.wav")), ["silence.trailing"])

        _synth(p("mono.wav"), tone, 4, -14.0, rate=44100, channels=1)
        expect("fail mono 44.1 kHz", check(p("mono.wav")), ["format.channels", "format.sample_rate"])

        _write(p("long.json"), {"meta": {"duration_sec": 4.2}})
        expect("fail duration mismatch", check(p("good.wav"), p("long.json")), ["format.duration"])

        _write(p("bad-beats.json"), {"bpm": 0, "beats_sec": [0.0, 1.0, 0.5],
                                      "downbeats_sec": [0.0, 9.0]})
        expect("fail invalid beats.json", check(p("good.wav"), None, p("bad-beats.json")),
               ["beats.bpm", "beats.order", "beats.range"])

        _sync_tests(p, expect, failures)
        _sfx_tests(p, expect)
        _arc_tests(p, expect)
        _bed_beat_tests(p, expect, failures)

        _cli_report_tests(p, failures)

    skill_md = os.path.join(skill_dir, "SKILL.md")
    size = os.path.getsize(skill_md) if os.path.isfile(skill_md) else SKILL_MD_MAX_BYTES
    print(f"{'ok  ' if size < SKILL_MD_MAX_BYTES else 'FAIL'} SKILL.md is {size} bytes")
    if size >= SKILL_MD_MAX_BYTES:
        failures.append(f"SKILL.md is {size} bytes; the limit is < {SKILL_MD_MAX_BYTES}")
    ref = os.path.join(skill_dir, "references", "lavfi-sfx-recipes.md")
    ref_size = os.path.getsize(ref) if os.path.isfile(ref) else REFERENCE_MAX_BYTES
    print(f"{'ok  ' if ref_size < REFERENCE_MAX_BYTES else 'FAIL'} reference is {ref_size} bytes")
    if ref_size >= REFERENCE_MAX_BYTES:
        failures.append(f"references/lavfi-sfx-recipes.md is missing or >= {REFERENCE_MAX_BYTES} bytes")
    binaries = [os.path.join(root, name) for root, _, names in os.walk(skill_dir)
                for name in names if not name.endswith(TEXT_SUFFIXES)
                and "__pycache__" not in root]
    print(f"{'ok  ' if not binaries else 'FAIL'} no binary files in the skill: {binaries}")
    if binaries:
        failures.append(f"binary files in the skill: {binaries}")

    print("self-test PASSED" if not failures else "self-test FAILED:\n  " + "\n  ".join(failures))
    return 0 if not failures else 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (BeatError, ToolError, OSError) as exc:
        sys.exit(_input_error(exc))
