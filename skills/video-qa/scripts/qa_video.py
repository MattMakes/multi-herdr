#!/usr/bin/env python3
"""Numeric render QA gate for a video project.

Runs 11 numeric checks on render/master.mp4 against timeline.json, writes
qa/render-check.json, and draws 2 inspection sheets:
qa/contact-sheet-360p.png and qa/hook-strip.png.

Python 3 standard library only. Needs ffmpeg and ffprobe on PATH.
Exit 0: every check passed. Exit 1: a check failed. Exit 2: bad input.

Usage:
  qa_video.py --video render/master.mp4 --timeline timeline.json
              [--scenes-dir scenes] [--render-dir render]
              [--out qa/render-check.json]
              [--contact-sheet qa/contact-sheet-360p.png]
              [--hook-strip qa/hook-strip.png]
              [--probe-forward DIR --probe-reverse DIR]
  qa_video.py --self-test
"""

import argparse
import json
import math
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import zlib
from fractions import Fraction
from pathlib import Path

CHECKS = (
    "scene_determinism",
    "render_determinism",
    "stream_integrity",
    "frame0_readiness",
    "hook_motion",
    "black_frames",
    "freeze_frames",
    "audio_loudness",
    "audio_boundary_pops",
    "variants_captions",
    "safe_zone_flash",
)

DURATION_TOL_SEC = 0.05
PSNR_MIN_DB = 45.0
FRAME0_YAVG_RANGE = (18.0, 240.0)
HOOK_WINDOW_SEC = 3.0
HOOK_STEP_SEC = 0.5
HOOK_MIN_DIFF = 0.03
BLACK_FILTER = "blackdetect=d=0.05:pic_th=0.98:pix_th=0.03"
FREEZE_FILTER = "freezedetect=n=-60dB:d=1.5"
FREEZE_MAX_SEC = 1.5
LUFS_TOL = 1.0
TRUE_PEAK_MAX_DBTP = -1.0
POP_WINDOW_SEC = 0.020
POP_PEAK_MAX_DBFS = -3.0
FLASH_DELTA = 0.20
FLASH_MAX_PER_SEC = 3
DEFAULT_FADE_SEC = 0.4
CAPTION_ZONE_BOTTOM = 0.82
VERTICAL_UI_RIGHT = 0.85
SHEET_TILE_H = 360
SHEET_COLUMNS = 6
CONTACT_FPS = 2
HOOK_FPS = 4
SKILL_MD_MAX_BYTES = 12288

SCAN_SUFFIXES = {".html", ".js", ".mjs", ".ts"}
SCAN_SKIP_DIRS = {"node_modules", "vendor", "lib"}
FORBIDDEN_CALLS = (
    ("requestAnimationFrame", r"\brequestAnimationFrame\s*\("),
    ("setInterval", r"\bsetInterval\s*\("),
    ("setTimeout", r"\bsetTimeout\s*\("),
    ("Date.now", r"\bDate\s*\.\s*now\s*\("),
    ("performance.now", r"\bperformance\s*\.\s*now\s*\("),
    ("Math.random", r"\bMath\s*\.\s*random\s*\("),
)
COMMENT_PATTERNS = (r"/\*.*?\*/", r"<!--.*?-->", r"(?<![:\w\"'\\])//[^\n]*")
CSS_MOTION = re.compile(r"(?<![-\w])(animation(?:-name)?|transition)\s*:\s*([^;}\"']*)", re.I)
SEEK_HOOK = re.compile(r"\b(?:window|globalThis)\s*\.\s*__seekToFrame\s*=")


class InputError(Exception):
    """Bad input: the gate cannot run. Exit code 2."""


# ---------------------------------------------------------------- helpers

def run(cmd, binary=False):
    return subprocess.run(cmd, capture_output=True, text=not binary)


def ffmpeg(*args, binary=False):
    return run(["ffmpeg", "-hide_banner", "-nostats", *args], binary=binary)


def ffprobe(path):
    proc = run(["ffprobe", "-v", "error", "-show_streams", "-show_format", "-of", "json", str(path)])
    if proc.returncode != 0:
        raise InputError(f"ffprobe cannot read {path}: {proc.stderr.strip()}")
    return json.loads(proc.stdout)


def stream(info, kind):
    return next((s for s in info.get("streams", []) if s.get("codec_type") == kind), None)


def stream_duration(info, kind):
    s = stream(info, kind)
    value = (s or {}).get("duration") or info.get("format", {}).get("duration")
    return float(value) if value is not None else None


def stamp(t):
    ms = int(round(max(t, 0.0) * 1000))
    return f"{ms // 60000:02d}:{ms // 1000 % 60:02d}.{ms % 1000:03d}"


def scene_end(scene):
    return float(scene["start_sec"]) + float(scene["duration_sec"])


def scene_at(scenes, t):
    for scene in scenes:
        if float(scene["start_sec"]) <= t < scene_end(scene):
            return scene.get("id", "?")
    return scenes[-1].get("id", "?") if scenes else "-"


def fade_windows(scenes):
    """Time windows where a declared fade transition may show black frames."""
    windows = []
    for scene in scenes:
        tr = scene.get("transition_out")
        kind = tr if isinstance(tr, str) else (tr or {}).get("type", "")
        if "fade" not in str(kind).lower():
            continue
        dur = DEFAULT_FADE_SEC if isinstance(tr, str) else float(tr.get("duration_sec", DEFAULT_FADE_SEC))
        edge = scene_end(scene)
        windows.append((edge - dur, edge + dur))
    return windows


def hold_windows(scenes):
    return [(float(s["start_sec"]), scene_end(s)) for s in scenes if s.get("static_hold") is True]


def inside(start, end, windows, tol):
    return any(start >= lo - tol and end <= hi + tol for lo, hi in windows)


def load_timeline(path):
    try:
        data = json.loads(Path(path).read_text())
    except (OSError, ValueError) as exc:
        raise InputError(f"cannot read timeline {path}: {exc}") from exc
    meta = data.get("meta") if isinstance(data, dict) else None
    if not isinstance(meta, dict):
        raise InputError("timeline.json has no meta object")
    for key in ("fps", "width", "height", "duration_sec"):
        if not isinstance(meta.get(key), (int, float)) or isinstance(meta.get(key), bool):
            raise InputError(f"timeline.json meta.{key} must be a number")
    scenes = data.get("scenes")
    if not isinstance(scenes, list) or not scenes:
        raise InputError("timeline.json scenes must be a non-empty list")
    for scene in scenes:
        if not isinstance(scene.get("start_sec"), (int, float)) or not isinstance(
            scene.get("duration_sec"), (int, float)
        ):
            raise InputError(f"scene {scene.get('id')!r} needs numeric start_sec and duration_sec")
    return meta, scenes


# ---------------------------------------------------------------- analysis pass

def analyse(video, has_audio):
    """One decode pass: per-frame signalstats, black, freeze and loudness."""
    args = ["-i", str(video), "-map", "0:v:0",
            "-vf", f"signalstats,metadata=mode=print:file=-,{BLACK_FILTER},{FREEZE_FILTER}"]
    if has_audio:
        args += ["-map", "0:a:0", "-af", "ebur128=peak=true:framelog=verbose"]
    proc = ffmpeg(*args, "-f", "null", "-")
    if proc.returncode != 0:
        raise InputError(f"ffmpeg cannot decode {video}: {proc.stderr.strip()[-400:]}")
    frames = []
    for line in proc.stdout.splitlines():
        if line.startswith("frame:"):
            m = re.search(r"pts_time:(\S+)", line)
            frames.append({"t": float(m.group(1)) if m else len(frames)})
        elif line.startswith("lavfi.signalstats.") and frames:
            key, _, value = line[len("lavfi.signalstats."):].partition("=")
            frames[-1][key] = float(value)
    err = proc.stderr
    blacks = [(float(a), float(b)) for a, b in re.findall(r"black_start:(\S+) black_end:(\S+)", err)]
    starts = [float(v) for v in re.findall(r"freeze_start: (\S+)", err)]
    ends = [float(v) for v in re.findall(r"freeze_end: (\S+)", err)]
    freezes = [(s, ends[i] if i < len(ends) else None) for i, s in enumerate(starts)]
    loud = re.search(r"I:\s+(-?[\d.]+|-inf) LUFS", err)
    peak = re.search(r"True peak:\s+Peak:\s+(-?[\d.]+|-inf) dBFS", err)
    return {
        "frames": frames,
        "blacks": blacks,
        "freezes": freezes,
        "lufs": float(loud.group(1)) if loud else None,
        "true_peak": float(peak.group(1)) if peak else None,
    }


def sampled_frames(video, rate, width, height, end_sec, pix_fmt):
    """Raw frames at t = 0, 1/rate, 2/rate, ... The fps filter would pick the frame at
    the end of each interval, so select takes the first frame at or after each sample time."""
    pick = f"eq(n\\,0)+gt(floor((t+0.0005)*{rate})\\,floor((prev_selected_t+0.0005)*{rate}))"
    proc = ffmpeg("-i", str(video), "-t", f"{end_sec:.3f}", "-vf", f"select={pick},scale={width}:{height}",
                  "-fps_mode", "passthrough", "-f", "rawvideo", "-pix_fmt", pix_fmt, "-", binary=True)
    size = width * height * (3 if pix_fmt == "rgb24" else 1)
    return [proc.stdout[i:i + size] for i in range(0, len(proc.stdout) - size + 1, size)]


# ---------------------------------------------------------------- checks
# Each check returns (details: dict, errors: list of (message, time_sec or None)).
# A details dict with "skipped": True means the input for the check is absent.

def strip_comments(text):
    for pattern in COMMENT_PATTERNS:
        text = re.sub(pattern, lambda m: re.sub(r"[^\n]", " ", m.group()), text, flags=re.S)
    return text


def css_blocks(text, suffix):
    if suffix != ".html":
        return []
    blocks = [(m.start(1), m.group(1)) for m in re.finditer(r"<style[^>]*>(.*?)</style>", text, re.S | re.I)]
    blocks += [(m.start(1), m.group(1)) for m in re.finditer(r"\bstyle\s*=\s*\"([^\"]*)\"", text, re.I)]
    return blocks


def check_scene_determinism(ctx):
    root = ctx.scenes_dir
    if root is None or not root.is_dir():
        return {"skipped": True, "reason": "no scenes directory"}, []
    errors, units, scanned = [], {}, 0
    for path in sorted(root.rglob("*")):
        rel = path.relative_to(root)
        if (path.suffix not in SCAN_SUFFIXES or not path.is_file() or path.name.endswith(".min.js")
                or SCAN_SKIP_DIRS & set(rel.parts[:-1])):
            continue
        scanned += 1
        unit = rel.parts[0] if len(rel.parts) > 1 else "."
        text = strip_comments(path.read_text(errors="replace"))
        units[unit] = units.get(unit, False) or bool(SEEK_HOOK.search(text))
        for name, pattern in FORBIDDEN_CALLS:
            for m in re.finditer(pattern, text):
                line = text.count("\n", 0, m.start()) + 1
                errors.append((f"{rel}:{line}: scene code calls {name}", None))
        for offset, block in css_blocks(text, path.suffix):
            for m in CSS_MOTION.finditer(block):
                if m.group(2).strip().lower() not in ("", "none"):
                    line = text.count("\n", 0, offset + m.start()) + 1
                    errors.append((f"{rel}:{line}: free-running CSS {m.group(1)}: {m.group(2).strip()}", None))
    for unit, has_hook in sorted(units.items()):
        if not has_hook:
            errors.append((f"scene {unit} does not define window.__seekToFrame", None))
    return {"files_scanned": scanned, "scenes": sorted(units)}, errors


def psnr_avg(a, b):
    proc = ffmpeg("-i", str(a), "-i", str(b), "-lavfi", "psnr", "-f", "null", "-")
    m = re.search(r"average:(\S+)", proc.stderr)
    if proc.returncode != 0 or not m:
        raise InputError(f"ffmpeg psnr failed for {a} and {b}: {proc.stderr.strip()[-300:]}")
    return math.inf if m.group(1) == "inf" else float(m.group(1))


def check_render_determinism(ctx):
    fwd, rev = ctx.probe_forward, ctx.probe_reverse
    if fwd is None:
        return {"skipped": True, "reason": "no --probe-forward/--probe-reverse frame sets"}, []
    fwd_names = {p.name for p in fwd.glob("*.png")}
    rev_names = {p.name for p in rev.glob("*.png")}
    errors = []
    if fwd_names != rev_names:
        errors.append((f"probe sets differ: forward-only {sorted(fwd_names - rev_names)}, "
                       f"reverse-only {sorted(rev_names - fwd_names)}", None))
    pairs = sorted(fwd_names & rev_names)
    if len(pairs) < 3:
        errors.append((f"need >= 3 probe frames (0, K, 2K), found {len(pairs)}", None))
    values = {}
    for name in pairs:
        value = psnr_avg(fwd / name, rev / name)
        values[name] = "inf" if math.isinf(value) else round(value, 2)
        if value < PSNR_MIN_DB:
            errors.append((f"{name}: cold-forward vs warm-reverse PSNR {value:.2f} dB < {PSNR_MIN_DB} dB", None))
    return {"psnr_avg_db": values, "min_db": PSNR_MIN_DB}, errors


def check_stream_integrity(ctx):
    meta, info, errors = ctx.meta, ctx.info, []
    v, a = stream(info, "video"), stream(info, "audio")
    if v is None:
        return {}, [("no video stream", None)]
    if a is None:
        errors.append(("no audio stream", None))
    fps = Fraction(str(meta["fps"])).limit_denominator(1001)
    r_rate, avg_rate = Fraction(v.get("r_frame_rate", "0/1")), Fraction(v.get("avg_frame_rate", "0/1"))
    if not (r_rate == avg_rate == fps):
        errors.append((f"frame rate r={r_rate} avg={avg_rate}, expected constant {fps}", None))
    if v.get("pix_fmt") != "yuv420p":
        errors.append((f"pix_fmt {v.get('pix_fmt')}, expected yuv420p", None))
    if (v.get("width"), v.get("height")) != (meta["width"], meta["height"]):
        errors.append((f"resolution {v.get('width')}x{v.get('height')}, expected "
                       f"{meta['width']}x{meta['height']}", None))
    target = float(meta["duration_sec"])
    durations = {"video": stream_duration(info, "video")}
    if a is not None:
        durations["audio"] = stream_duration(info, "audio")
    for kind, dur in durations.items():
        if dur is None or abs(dur - target) > DURATION_TOL_SEC:
            errors.append((f"{kind} duration {dur} s differs from meta.duration_sec {target} s "
                           f"by > {DURATION_TOL_SEC} s", None))
    expected = round(target * float(fps))
    nb = int(v["nb_frames"]) if str(v.get("nb_frames", "")).isdigit() else len(ctx.analysis["frames"])
    if abs(nb - expected) > 1:
        errors.append((f"nb_frames {nb}, expected {expected} +-1", None))
    details = {"r_frame_rate": str(r_rate), "avg_frame_rate": str(avg_rate), "pix_fmt": v.get("pix_fmt"),
               "width": v.get("width"), "height": v.get("height"), "nb_frames": nb,
               "durations_sec": durations}
    return details, errors


def check_frame0(ctx):
    frames = ctx.analysis["frames"]
    if not frames:
        return {}, [("no decoded frames", 0.0)]
    f0 = frames[0]
    yavg, ymin, ymax = f0.get("YAVG"), f0.get("YMIN"), f0.get("YMAX")
    errors = []
    dark_ok = ctx.meta.get("allow_dark_intro") is True
    lo, hi = FRAME0_YAVG_RANGE
    if not dark_ok and not (lo <= yavg <= hi):
        errors.append((f"frame 0 YAVG {yavg} outside [{lo:g}, {hi:g}] (blank frame)", 0.0))
    if ymin == ymax:
        errors.append((f"frame 0 is flat: YMIN == YMAX == {ymin}", 0.0))
    return {"YAVG": yavg, "YMIN": ymin, "YMAX": ymax, "allow_dark_intro": dark_ok}, errors


def check_hook_motion(ctx):
    w, h = 160, max(2, round(160 * ctx.meta["height"] / ctx.meta["width"]))
    frames = sampled_frames(ctx.video, 1 / HOOK_STEP_SEC, w, h, HOOK_WINDOW_SEC + HOOK_STEP_SEC / 2, "gray")
    diffs = []
    for a, b in zip(frames, frames[1:]):
        diffs.append(round(sum(abs(x - y) for x, y in zip(a, b)) / (255.0 * len(a)), 4))
    best = max(diffs, default=0.0)
    errors = []
    if best < HOOK_MIN_DIFF:
        errors.append((f"no visible motion in 0.0-{HOOK_WINDOW_SEC} s: max adjacent "
                       f"{HOOK_STEP_SEC} s frame diff {best:.4f} < {HOOK_MIN_DIFF}", 0.0))
    return {"pair_diffs": diffs, "max_diff": best, "min_required": HOOK_MIN_DIFF}, errors


def check_black(ctx):
    tol = 1.0 / float(ctx.meta["fps"])
    windows = fade_windows(ctx.scenes)
    found, errors = [], []
    for start, end in ctx.analysis["blacks"]:
        planned = inside(start, end, windows, tol)
        found.append({"start_sec": start, "end_sec": end, "planned": planned})
        if not planned:
            errors.append((f"unplanned black interval {stamp(start)}-{stamp(end)} "
                           f"in scene {scene_at(ctx.scenes, start)}", start))
    return {"filter": BLACK_FILTER, "intervals": found}, errors


def check_freeze(ctx):
    tol = 1.0 / float(ctx.meta["fps"])
    duration = stream_duration(ctx.info, "video") or float(ctx.meta["duration_sec"])
    windows = hold_windows(ctx.scenes)
    found, errors = [], []
    for start, end in ctx.analysis["freezes"]:
        end = duration if end is None else end
        if end - start <= FREEZE_MAX_SEC:
            continue
        planned = inside(start, end, windows, tol)
        found.append({"start_sec": start, "end_sec": end, "planned": planned})
        if not planned:
            errors.append((f"unplanned freeze {stamp(start)}-{stamp(end)} ({end - start:.2f} s) "
                           f"in scene {scene_at(ctx.scenes, start)}", start))
    return {"filter": FREEZE_FILTER, "intervals": found}, errors


def check_loudness(ctx):
    if stream(ctx.info, "audio") is None:
        return {}, [("no audio stream to measure", None)]
    target = float(ctx.meta.get("lufs_target", -14.0))
    lufs, peak = ctx.analysis["lufs"], ctx.analysis["true_peak"]
    errors = []
    if lufs is None or abs(lufs - target) > LUFS_TOL:
        errors.append((f"integrated loudness {lufs} LUFS outside {target} +-{LUFS_TOL} LU", None))
    if peak is None or peak > TRUE_PEAK_MAX_DBTP:
        errors.append((f"true peak {peak} dBTP > {TRUE_PEAK_MAX_DBTP} dBTP", None))
    return {"integrated_lufs": lufs, "true_peak_dbtp": peak, "target_lufs": target}, errors


def window_peak_dbfs(video, start, length):
    proc = ffmpeg("-ss", f"{max(start, 0.0):.3f}", "-t", f"{length:.3f}", "-i", str(video),
                  "-map", "0:a:0", "-af", "astats", "-f", "null", "-")
    values = [float(v) for v in re.findall(r"Peak level dB: (-?[\d.]+|-inf)", proc.stderr)]
    return max(values) if values else None


def check_boundary_pops(ctx):
    if stream(ctx.info, "audio") is None:
        return {}, [("no audio stream to measure", None)]
    peaks, errors = {}, []
    for scene in ctx.scenes[1:]:
        cut = float(scene["start_sec"])
        peak = window_peak_dbfs(ctx.video, cut - POP_WINDOW_SEC, 2 * POP_WINDOW_SEC)
        peaks[stamp(cut)] = peak
        if peak is not None and peak > POP_PEAK_MAX_DBFS:
            errors.append((f"audio peak {peak:.1f} dBFS > {POP_PEAK_MAX_DBFS} dBFS within "
                           f"{POP_WINDOW_SEC * 1000:.0f} ms of cut into scene {scene.get('id')}", cut))
    return {"cut_peaks_dbfs": peaks, "max_dbfs": POP_PEAK_MAX_DBFS}, errors


def caption_time(text):
    m = re.fullmatch(r"(?:(\d+):)?(\d{2}):(\d{2})[,.](\d{3})", text.strip())
    if not m:
        raise ValueError(f"bad timestamp {text.strip()!r}")
    hours, minutes, seconds, ms = (int(g or 0) for g in m.groups())
    return hours * 3600 + minutes * 60 + seconds + ms / 1000


def parse_captions(text, vtt):
    """Return [(start, end)] cues. Raise ValueError on a syntax error."""
    lines = text.replace("\r\n", "\n").lstrip("\ufeff").split("\n")
    if vtt and not lines[0].startswith("WEBVTT"):
        raise ValueError("missing WEBVTT header")
    cues = []
    for i, line in enumerate(lines):
        if "-->" not in line:
            continue
        left, _, right = line.partition("-->")
        cues.append((caption_time(left), caption_time(right.split()[0] if right.split() else "")))
        if not vtt and not (i > 0 and lines[i - 1].strip().isdigit()):
            raise ValueError(f"line {i + 1}: cue without a numeric index")
    if not cues:
        raise ValueError("no cues")
    return cues


def check_captions(path, vtt, duration):
    try:
        cues = parse_captions(path.read_text(errors="replace"), vtt)
    except ValueError as exc:
        return [f"{path.name}: {exc}"]
    errors, prev_end = [], 0.0
    for start, end in cues:
        if not (0.0 <= start < end <= duration + DURATION_TOL_SEC):
            errors.append(f"{path.name}: cue {stamp(start)}-{stamp(end)} outside [0, {duration}] s or empty")
        if start < prev_end:
            errors.append(f"{path.name}: cue at {stamp(start)} overlaps previous cue ending {stamp(prev_end)}")
        prev_end = max(prev_end, end)
    return errors


def check_variants_captions(ctx):
    root = ctx.render_dir
    short, long_ = sorted((int(ctx.meta["width"]), int(ctx.meta["height"])))
    targets = {"16x9.mp4": (long_, short), "9x16.mp4": (short, long_), "1x1.mp4": (short, short)}
    duration = float(ctx.meta["duration_sec"])
    seen, errors = [], []
    for name, (tw, th) in targets.items():
        path = root / name
        if not path.is_file():
            continue
        seen.append(name)
        info = ffprobe(path)
        v = stream(info, "video") or {}
        if (v.get("width"), v.get("height")) != (tw, th):
            errors.append((f"{name} is {v.get('width')}x{v.get('height')}, expected {tw}x{th}", None))
        dur = stream_duration(info, "video")
        if dur is None or abs(dur - duration) > DURATION_TOL_SEC:
            errors.append((f"{name} duration {dur} s differs from {duration} s by > {DURATION_TOL_SEC} s", None))
    for name, vtt in (("captions.srt", False), ("captions.vtt", True)):
        path = root / name
        if path.is_file():
            seen.append(name)
            errors += [(msg, None) for msg in check_captions(path, vtt, duration)]
    if not seen:
        return {"skipped": True, "reason": f"no variants or captions in {root}"}, []
    return {"checked": seen}, errors


def check_safe_zone_flash(ctx):
    errors = []
    vertical = ctx.meta.get("aspect") == "9:16" or ctx.meta["height"] > ctx.meta["width"]
    for scene in ctx.scenes:
        for box in scene.get("critical_regions") or []:
            x, y, w, h = (float(v) for v in box)
            bad = min(x, y, w, h) < 0 or x + w > 1 or y + h > CAPTION_ZONE_BOTTOM
            if vertical and x + w > VERTICAL_UI_RIGHT:
                bad = True
            if bad:
                errors.append((f"scene {scene.get('id')}: critical region {box} leaves the safe zone",
                               float(scene["start_sec"])))
    frames = ctx.analysis["frames"]
    flips = [b["t"] for a, b in zip(frames, frames[1:])
             if abs(b.get("YAVG", 0) - a.get("YAVG", 0)) / 255.0 > FLASH_DELTA]
    worst, worst_t = 0, None
    for i, t in enumerate(flips):
        count = sum(1 for u in flips[i:] if u < t + 1.0)
        if count > worst:
            worst, worst_t = count, t
    if worst > FLASH_MAX_PER_SEC:
        errors.append((f"{worst} full-frame luminance transitions > {FLASH_DELTA:.0%} within 1 s "
                       f"from {stamp(worst_t)} (max {FLASH_MAX_PER_SEC})", worst_t))
    return {"luma_transitions": len(flips), "max_per_second": worst}, errors


CHECK_FUNCS = {
    "scene_determinism": check_scene_determinism,
    "render_determinism": check_render_determinism,
    "stream_integrity": check_stream_integrity,
    "frame0_readiness": check_frame0,
    "hook_motion": check_hook_motion,
    "black_frames": check_black,
    "freeze_frames": check_freeze,
    "audio_loudness": check_loudness,
    "audio_boundary_pops": check_boundary_pops,
    "variants_captions": check_variants_captions,
    "safe_zone_flash": check_safe_zone_flash,
}


# ---------------------------------------------------------------- sheets

FONT = {  # 5x7 glyphs, one hex byte per row, bit 4 is the left column.
    "0": "0E11131519110E", "1": "040C040404040E", "2": "0E11010204081F", "3": "1F02040201110E",
    "4": "02060A121F0202", "5": "1F101E0101110E", "6": "0608101E11110E", "7": "1F010204080808",
    "8": "0E11110E11110E", "9": "0E11110F01020C", "A": "0E11111F111111", "B": "1E11111E11111E",
    "C": "0E11101010110E", "D": "1E11111111111E", "E": "1F10101E10101F", "F": "1F10101E101010",
    "G": "0E11101711110F", "H": "1111111F111111", "I": "0E04040404040E",
    "J": "0702020202120C", "K": "11121418141211", "L": "1010101010101F", "M": "111B1515111111",
    "N": "11111915131111", "O": "0E11111111110E", "P": "1E11111E101010", "Q": "0E11111115120D",
    "R": "1E11111E141211", "S": "0F10100E01011E", "T": "1F040404040404", "U": "1111111111110E",
    "V": "11111111110A04", "W": "1111111515150A", "X": "11110A040A1111", "Y": "11110A04040404",
    "Z": "1F01020408101F", "-": "0000001F000000", "_": "0000000000001F", ".": "00000000000C0C",
    ":": "000C0C000C0C00", " ": "00000000000000", "?": "0E110102040004", "/": "01010204081010",
}
GLYPH_SCALE = 2
GLYPH_ADVANCE = 6 * GLYPH_SCALE
LINE_HEIGHT = 9 * GLYPH_SCALE


def draw_label(canvas, cw, x0, y0, lines, max_w):
    """Burn text lines on a dark bar at (x0, y0) of an RGB canvas of width cw."""
    pad = 4
    bar_w = min(max_w, max(len(s) for s in lines) * GLYPH_ADVANCE + 2 * pad)
    bar_h = len(lines) * LINE_HEIGHT + pad
    for y in range(y0, y0 + bar_h):
        row = (y * cw + x0) * 3
        canvas[row:row + bar_w * 3] = b"\x10\x10\x10" * bar_w
    for li, text in enumerate(lines):
        for ci, ch in enumerate(text.upper()):
            gx = x0 + pad + ci * GLYPH_ADVANCE
            if gx + GLYPH_ADVANCE > x0 + bar_w:
                break
            rows = FONT.get(ch, FONT["?"])
            for ry in range(7):
                bits = int(rows[ry * 2:ry * 2 + 2], 16)
                for rx in range(5):
                    if not bits & (0x10 >> rx):
                        continue
                    for sy in range(GLYPH_SCALE):
                        py = y0 + pad + li * LINE_HEIGHT + ry * GLYPH_SCALE + sy
                        px = gx + rx * GLYPH_SCALE
                        off = (py * cw + px) * 3
                        canvas[off:off + 3 * GLYPH_SCALE] = b"\xff\xff\xff" * GLYPH_SCALE


def write_png(path, width, height, rgb):
    raw = b"".join(b"\x00" + rgb[y * width * 3:(y + 1) * width * 3] for y in range(height))

    def chunk(tag, data):
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data))

    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
                     + chunk(b"IDAT", zlib.compress(raw, 6)) + chunk(b"IEND", b""))


def make_sheet(ctx, out, fps, end_sec, max_tiles=None):
    th = SHEET_TILE_H
    tw = max(2, 2 * round(ctx.meta["width"] * th / ctx.meta["height"] / 2))
    frames = sampled_frames(ctx.video, fps, tw, th, end_sec, "rgb24")[:max_tiles]
    if not frames:
        raise InputError(f"no frames decoded for {out}")
    cols = min(SHEET_COLUMNS, len(frames))
    rows = math.ceil(len(frames) / cols)
    cw, ch = cols * tw, rows * th
    canvas = bytearray(cw * ch * 3)
    for i, frame in enumerate(frames):
        x0, y0 = (i % cols) * tw, (i // cols) * th
        for y in range(th):
            off = ((y0 + y) * cw + x0) * 3
            canvas[off:off + tw * 3] = frame[y * tw * 3:(y + 1) * tw * 3]
        t = i / fps
        draw_label(canvas, cw, x0, y0, [stamp(t), scene_at(ctx.scenes, t)], tw)
    write_png(out, cw, ch, bytes(canvas))
    return {"path": str(out), "tiles": len(frames), "tile_px": [tw, th], "fps": fps, "size_px": [cw, ch]}


# ---------------------------------------------------------------- driver

class Context:
    def __init__(self, args):
        self.video = Path(args.video)
        self.timeline = Path(args.timeline)
        if not self.video.is_file():
            raise InputError(f"video not found: {self.video}")
        self.meta, self.scenes = load_timeline(self.timeline)
        project = self.timeline.resolve().parent
        self.scenes_dir = Path(args.scenes_dir) if args.scenes_dir else project / "scenes"
        self.render_dir = Path(args.render_dir) if args.render_dir else self.video.resolve().parent
        self.out = Path(args.out) if args.out else project / "qa" / "render-check.json"
        self.contact_sheet = Path(args.contact_sheet) if args.contact_sheet else project / "qa" / "contact-sheet-360p.png"
        self.hook_strip = Path(args.hook_strip) if args.hook_strip else project / "qa" / "hook-strip.png"
        if (args.probe_forward is None) != (args.probe_reverse is None):
            raise InputError("--probe-forward and --probe-reverse go together")
        self.probe_forward = Path(args.probe_forward) if args.probe_forward else None
        self.probe_reverse = Path(args.probe_reverse) if args.probe_reverse else None
        self.info = ffprobe(self.video)
        if stream(self.info, "video") is None:
            raise InputError(f"{self.video} has no video stream")
        self.analysis = analyse(self.video, stream(self.info, "audio") is not None)


def run_gate(args):
    ctx = Context(args)
    report = {"passed": True, "video": str(ctx.video), "timeline": str(ctx.timeline), "checks": {}, "errors": []}
    for name in CHECKS:
        details, errors = CHECK_FUNCS[name](ctx)
        details["passed"] = not errors
        report["checks"][name] = details
        for message, t in errors:
            entry = {"check": name, "message": message}
            if t is not None:
                entry["time_sec"] = round(t, 3)
                entry["timestamp"] = stamp(t)
            report["errors"].append(entry)
    report["passed"] = not report["errors"]
    duration = float(ctx.meta["duration_sec"])
    report["artifacts"] = {
        "contact_sheet": make_sheet(ctx, ctx.contact_sheet, CONTACT_FPS, duration),
        "hook_strip": make_sheet(ctx, ctx.hook_strip, HOOK_FPS, HOOK_WINDOW_SEC,
                                 max_tiles=int(HOOK_WINDOW_SEC * HOOK_FPS)),
    }
    ctx.out.parent.mkdir(parents=True, exist_ok=True)
    ctx.out.write_text(json.dumps(report, indent=2) + "\n")
    return report, ctx.out


def require_tools():
    missing = [tool for tool in ("ffmpeg", "ffprobe") if shutil.which(tool) is None]
    if missing:
        raise InputError(f"missing on PATH: {', '.join(missing)}")


# ---------------------------------------------------------------- self-test

SCENE_OK = """<!doctype html><html><head><style>.dot { transform: translateX(0); }</style></head>
<body><div class="dot"></div><script>
// Pure function of (frameIndex, fps). No clocks, no timers.
function mulberry32(a) { return function () { a |= 0; a = a + 0x6D2B79F5 | 0;
  let t = Math.imul(a ^ a >>> 15, 1 | a); t = t + Math.imul(t ^ t >>> 7, 61 | t) ^ t;
  return ((t ^ t >>> 14) >>> 0) / 4294967296; }; }
window.__seekToFrame = function (frameIndex, fps) {
  const rand = mulberry32(7);
  document.querySelector('.dot').style.transform = 'translateX(' + (frameIndex / fps * 100 + rand()) + 'px)';
};
</script></body></html>
"""
SCENE_RAF = """<!doctype html><html><body><canvas></canvas><script>
let x = 0;
function tick() { x += Math.random(); requestAnimationFrame(tick); }
requestAnimationFrame(tick);
</script></body></html>
"""
SRT_OK = "1\n00:00:00,100 --> 00:00:01,900\nKill flaky tests.\n\n2\n00:00:02,100 --> 00:00:03,800\nTry it now.\n"
VTT_OK = "WEBVTT\n\n00:00.100 --> 00:01.900\nKill flaky tests.\n\n00:02.100 --> 00:03.800\nTry it now.\n"


def sh(*args):
    proc = ffmpeg("-y", "-loglevel", "error", *args)
    if proc.returncode != 0:
        raise RuntimeError(f"ffmpeg fixture failed: {' '.join(args)}\n{proc.stderr}")


def make_fixture_video(path, vf="null", tone_db=None, clip=False, width=320, height=180, fps=30, dur=4.0):
    src = f"testsrc2=s={width}x{height}:r={fps}:d={dur}"
    audio = f"aevalsrc=sin(440*2*PI*t)|sin(440*2*PI*t):s=48000:d={dur}" if clip else f"sine=f=440:sample_rate=48000:d={dur}"
    afilter = "anull" if clip else f"volume={tone_db}dB"
    sh("-f", "lavfi", "-i", src, "-f", "lavfi", "-i", audio, "-vf", vf, "-af", afilter, "-ac", "2",
       "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", "-r", str(fps),
       "-c:a", "aac", "-b:a", "192k", "-t", str(dur), str(path))


def tone_gain_db(tmp):
    """Gain that brings the 440 Hz test tone to -14 LUFS."""
    probe = tmp / "tone.wav"
    sh("-f", "lavfi", "-i", "sine=f=440:sample_rate=48000:d=4", "-ac", "2", str(probe))
    proc = ffmpeg("-i", str(probe), "-af", "ebur128=framelog=verbose", "-f", "null", "-")
    lufs = float(re.search(r"I:\s+(-?[\d.]+) LUFS", proc.stderr).group(1))
    return -14.0 - lufs


def make_project(root, video_vf="null", clip=False, scene_html=SCENE_OK, gain=0.0, probes=True, static_hold=False):
    root.mkdir(parents=True)
    (root / "render").mkdir()
    make_fixture_video(root / "render" / "master.mp4", vf=video_vf, tone_db=gain, clip=clip)
    timeline = {
        "meta": {"id": "self-test", "title": "Self test", "aspect": "16:9", "fps": 30, "width": 320,
                 "height": 180, "duration_sec": 4.0, "lufs_target": -14.0},
        "scenes": [
            {"id": "s01-hook", "kind": "hook", "start_sec": 0.0, "duration_sec": 2.0,
             "transition_out": {"type": "cut", "duration_sec": 0.0},
             "critical_regions": [[0.1, 0.2, 0.8, 0.4]], "focal_point": [0.5, 0.4]},
            {"id": "s02-cta", "kind": "cta", "start_sec": 2.0, "duration_sec": 2.0, "static_hold": static_hold,
             "critical_regions": [[0.2, 0.3, 0.6, 0.3]], "focal_point": [0.5, 0.45]},
        ],
    }
    (root / "timeline.json").write_text(json.dumps(timeline, indent=2))
    scene_dir = root / "scenes" / "s01-hook"
    scene_dir.mkdir(parents=True)
    (scene_dir / "index.html").write_text(scene_html)
    (root / "render" / "captions.srt").write_text(SRT_OK)
    (root / "render" / "captions.vtt").write_text(VTT_OK)
    for name, size in (("16x9.mp4", "320:180"), ("9x16.mp4", "180:320"), ("1x1.mp4", "180:180")):
        sh("-i", str(root / "render" / "master.mp4"), "-vf", f"scale={size}", "-an", "-c:v", "libx264",
           "-preset", "ultrafast", "-pix_fmt", "yuv420p", str(root / "render" / name))
    if probes:
        for side in ("forward", "reverse"):
            (root / "probe" / side).mkdir(parents=True)
            for frame in (0, 30, 60):
                sh("-f", "lavfi", "-i", "testsrc2=s=320x180:r=30:d=4", "-vf", f"select=eq(n\\,{frame})",
                   "-frames:v", "1", str(root / "probe" / side / f"frame-{frame:06d}.png"))
    return root


def gate_args(root, probes=True):
    argv = ["--video", str(root / "render" / "master.mp4"), "--timeline", str(root / "timeline.json")]
    if probes:
        argv += ["--probe-forward", str(root / "probe" / "forward"), "--probe-reverse", str(root / "probe" / "reverse")]
    return argv


def invoke(argv):
    proc = run([sys.executable, str(Path(__file__).resolve()), *argv])
    return proc.returncode, proc


def failed_checks(root):
    report = json.loads((root / "qa" / "render-check.json").read_text())
    return report, {name for name, c in report["checks"].items() if not c["passed"]}


def assert_png(path, width, height):
    data = path.read_bytes()
    assert data[:8] == b"\x89PNG\r\n\x1a\n", f"{path} is not a PNG"
    w, h = struct.unpack(">II", data[16:24])
    assert (w, h) == (width, height), f"{path} is {w}x{h}, expected {width}x{height}"


def self_test():
    require_tools()
    skill_dir = Path(__file__).resolve().parent.parent
    skill_md = skill_dir / "SKILL.md"
    assert skill_md.stat().st_size < SKILL_MD_MAX_BYTES, f"SKILL.md is {skill_md.stat().st_size} bytes"
    stray = [p for p in skill_dir.rglob("*") if p.is_file() and p.suffix not in (".md", ".py", ".mjs", ".html")
             and "__pycache__" not in p.parts]
    assert not stray, f"non-text files in skill: {stray}"
    assert (parse_captions(SRT_OK, False)[1] == (2.1, 3.8)), "SRT parser"
    for bad in ("1\n00:00:02,000 --> 00:00:01,000\nx\n", "2\n00:00:00,000 --> 00:00:02,000\na\n\n"
                "3\n00:00:01,000 --> 00:00:03,000\nb\n"):
        assert check_captions_text(bad), f"caption error not found in {bad!r}"

    with tempfile.TemporaryDirectory(prefix="qa-video-") as tmp_name:
        tmp = Path(tmp_name)
        gain = tone_gain_db(tmp)
        cases = [
            ("pass", {}, set()),
            ("black-frame", {"video_vf": "drawbox=x=0:y=0:w=iw:h=ih:color=black:t=fill:enable='between(t,1.2,1.7)'"},
             {"black_frames"}),
            ("dark-background", {"video_vf": "drawbox=x=0:y=0:w=iw:h=ih:color=0x0B1020:t=fill:enable='between(t,1.2,1.7)'"},
             set()),
            ("frozen", {"video_vf": "trim=end=2,tpad=stop_mode=clone:stop_duration=2"}, {"freeze_frames"}),
            ("raf-scene", {"scene_html": SCENE_RAF}, {"scene_determinism"}),
            ("clipped-audio", {"clip": True}, {"audio_loudness", "audio_boundary_pops"}),
        ]
        for name, kwargs, expected in cases:
            root = make_project(tmp / name, gain=gain, **kwargs)
            code, proc = invoke(gate_args(root))
            report, failed = failed_checks(root)
            assert failed == expected, f"{name}: failed checks {sorted(failed)}, expected {sorted(expected)}\n" \
                                       f"{json.dumps(report['errors'], indent=2)}"
            assert code == (1 if expected else 0), f"{name}: exit {code}\n{proc.stderr}"
            assert report["passed"] is (not expected), f"{name}: passed flag wrong"
            assert_png(root / "qa" / "contact-sheet-360p.png", 6 * 640, 2 * 360)
            assert_png(root / "qa" / "hook-strip.png", 6 * 640, 2 * 360)
            print(f"ok {name}: failed={sorted(failed)} exit={code}")

        root = tmp / "frozen"
        timeline = json.loads((root / "timeline.json").read_text())
        timeline["scenes"][1]["static_hold"] = True
        (root / "timeline.json").write_text(json.dumps(timeline))
        code, _ = invoke(gate_args(root))
        assert code == 0, "static_hold scene must allow a planned freeze"
        print("ok frozen-with-static-hold: exit=0")

        root = tmp / "pass"
        sh("-f", "lavfi", "-i", "color=c=white:s=320x180:d=1", "-frames:v", "1",
           str(root / "probe" / "reverse" / "frame-000030.png"))
        code, _ = invoke(gate_args(root))
        _, failed = failed_checks(root)
        assert code == 1 and failed == {"render_determinism"}, f"probe mismatch: {sorted(failed)}"
        print("ok probe-mismatch: failed=['render_determinism'] exit=1")

        code, proc = invoke(["--video", str(tmp / "missing.mp4"), "--timeline", str(root / "timeline.json")])
        assert code == 2, f"missing video: exit {code}"
        print("ok missing-input: exit=2")
    print("self-test passed")
    return 0


def check_captions_text(text):
    with tempfile.TemporaryDirectory(prefix="qa-captions-") as tmp:
        path = Path(tmp) / "captions.srt"
        path.write_text(text)
        return check_captions(path, False, 4.0)


def main(argv=None):
    parser = argparse.ArgumentParser(description="Numeric render QA gate and inspection sheets.")
    parser.add_argument("--self-test", action="store_true", help="run the built-in tests and exit")
    parser.add_argument("--video", help="render/master.mp4")
    parser.add_argument("--timeline", help="timeline.json")
    parser.add_argument("--scenes-dir", help="scene sources (default: <timeline dir>/scenes)")
    parser.add_argument("--render-dir", help="aspect variants and captions (default: video dir)")
    parser.add_argument("--out", help="report path (default: <timeline dir>/qa/render-check.json)")
    parser.add_argument("--contact-sheet", help="default: <timeline dir>/qa/contact-sheet-360p.png")
    parser.add_argument("--hook-strip", help="default: <timeline dir>/qa/hook-strip.png")
    parser.add_argument("--probe-forward", help="cold-forward probe PNG frames (0, K, 2K)")
    parser.add_argument("--probe-reverse", help="warm-reverse probe PNG frames, same file names")
    args = parser.parse_args(argv)
    try:
        if args.self_test:
            return self_test()
        if not args.video or not args.timeline:
            parser.error("--video and --timeline are required")
        require_tools()
        report, out = run_gate(args)
    except InputError as exc:
        print(f"qa_video: {exc}", file=sys.stderr)
        return 2
    for err in report["errors"]:
        where = f"[{err['timestamp']}] " if "timestamp" in err else ""
        print(f"FAIL {err['check']}: {where}{err['message']}")
    print(f"{'PASS' if report['passed'] else 'FAIL'}: {sum(c['passed'] for c in report['checks'].values())}"
          f"/{len(CHECKS)} checks passed; report {out}")
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
