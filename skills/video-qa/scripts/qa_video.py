#!/usr/bin/env python3
"""Numeric render QA gate for a video project.

Runs 21 numeric checks on render/master.mp4 against timeline.json, writes
qa/render-check.json, and draws 3 inspection sheets:
qa/contact-sheet-360p.png, qa/phone-sheet-360w.png and qa/hook-strip.png.

Python 3 standard library only. Needs ffmpeg and ffprobe on PATH.
Each check has a state: pass, fail or skip. A skip means the check does not
apply to the input (a 4 s film has no motion contrast). A check that applies
but cannot run (no probe frames for HTML scenes, no raster report for scenes
that draw images, 0 files or 0 samples) is a fail, never a skip.
Exit 0: no check failed. Exit 1: a check failed. Exit 2: bad input; the
report is still written, with passed false and the error.

Usage:
  qa_video.py --video render/master.mp4 --timeline timeline.json
              [--scenes-dir scenes] [--render-dir render]
              [--out qa/render-check.json]
              [--contact-sheet qa/contact-sheet-360p.png]
              [--hook-strip qa/hook-strip.png]
              [--phone-sheet qa/phone-sheet-360w.png]
              [--phone-frames qa/phone-frames]
              [--raster-sizes render/raster-sizes.json]
              [--expect GATE-RESULT ...]
              [--probe-forward DIR --probe-reverse DIR]
  qa_video.py --self-test
"""

import argparse
import json
import math
import operator
import os
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import types
import zlib
from fractions import Fraction
from pathlib import Path

STATES = ("pass", "skip", "fail")
CHECKS = (
    "scene_determinism",
    "render_determinism",
    "stream_integrity",
    "format_fit",
    "frame0_readiness",
    "hook_motion",
    "black_frames",
    "freeze_frames",
    "audio_loudness",
    "audio_boundary_pops",
    "variants_captions",
    "safe_zone_flash",
    "frame_pops",
    "cuts_vs_plan",
    "dead_time",
    "flat_opening",
    "motion_contrast",
    "loop_seam",
    "raster_upscale",
    "cut_flash",
    "gate_results",
)

DURATION_TOL_SEC = 0.05
# Master length range of each meta.format, from docs/specs/video-formats.md.
FORMAT_RANGES_SEC = {"short": (15.0, 90.0), "long": (180.0, 900.0)}
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

# Motion and cut analysis: grey frames, 96 px on the long side, every frame.
# A frame difference is the mean absolute pixel difference in [0, 1].
MOTION_LONG_SIDE = 96
POP_MIN_DIFF = 0.08          # a 1-frame pop jumps away and back by at least this
POP_RETURN_RATIO = 0.5       # frame before vs frame after differ by at most this x the jump
CUT_MIN_DIFF = 0.10
CUT_MEDIAN_RATIO = 3.0       # a cut is this many times the local median difference
CUT_MEDIAN_RADIUS = 8        # frames on each side for the local median
CUT_MATCH_SEC = 0.5          # a cut within this of a scene boundary is planned
DEAD_DIFF = 0.002            # below this a frame step counts as no motion
DEAD_MAX_SEC = 2.0
OPENING_SEC = 5.0
OPENING_MIN_RATIO = 0.5      # opening motion per second vs the rest of the film
OPENING_MIN_REST_SEC = 2.0
CONTRAST_MIN_SECONDS = 6     # whole seconds needed to judge motion contrast
CONTRAST_MIN_RATIO = 1.5     # 90th / 10th percentile of motion per second
LOOP_SEAM_FACTOR = 3.0
LOOP_SEAM_FLOOR = 0.02
RASTER_WARN_SCALE = 1.12
RASTER_WARN_SEC = 0.5
RASTER_FAIL_SCALE = 1.2
RASTER_DEFAULT_STEP_SEC = 0.1
RASTER_INTEGER_TOL = 0.02    # a nearest-neighbour draw within this of a whole-number scale is pixel art
PHONE_TILE_W = 360
CUT_LUMA_DELTA = 0.06
PHONE_FRAME_H = 360
DEFAULT_GATE_RESULTS = (
    "qa/audio-check.json",
    "audio/music-continuity.json",
    "qa/timeline-check.json",
    "qa/plate-check.json",
)

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
RASTER_SOURCE = re.compile(
    r"<img\b|<video\b|\bdrawImage\s*\(|\burl\(\s*[\"']?[^)\"']+\.(?:png|jpe?g|webp|gif|avif)\b"
    r"|[\"'`][^\"'`\n]+\.(?:png|jpe?g|webp|gif|avif|mp4|webm|mov)[\"'`]", re.I)
SEEK_HOOK = re.compile(r"\b(?:window|globalThis)\s*\.\s*__seekToFrame\s*=")


class InputError(Exception):
    """Bad input: the gate cannot run. Exit code 2."""


class Caught:
    """A with-block that keeps 1 exception of the given kinds in .error instead of raising it."""

    def __init__(self, kinds):
        self.kinds, self.error = kinds, None

    def __enter__(self):
        return self

    def __exit__(self, kind, error, _trace):
        self.error = error if kind is not None and issubclass(kind, self.kinds) else None
        return self.error is not None


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


def transition_kind(scene):
    tr = scene.get("transition_out")
    return str(tr if isinstance(tr, str) else (tr or {}).get("type", "")).lower()


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
    lufs, peak = _summary_levels(err)
    return {
        "frames": frames,
        "blacks": blacks,
        "freezes": freezes,
        "lufs": lufs,
        "true_peak": peak,
    }


def sampled_frames(video, rate, width, height, end_sec, pix_fmt):
    """Raw frames at t = 0, 1/rate, 2/rate, ... The fps filter would pick the frame at
    the end of each interval, so select takes the first frame at or after each sample time."""
    pick = f"eq(n\\,0)+gt(floor((t+0.0005)*{rate})\\,floor((prev_selected_t+0.0005)*{rate}))"
    proc = ffmpeg("-i", str(video), "-t", f"{end_sec:.3f}", "-vf", f"select={pick},scale={width}:{height}",
                  "-fps_mode", "passthrough", "-f", "rawvideo", "-pix_fmt", pix_fmt, "-", binary=True)
    size = width * height * (3 if pix_fmt == "rgb24" else 1)
    return [proc.stdout[i:i + size] for i in range(0, len(proc.stdout) - size + 1, size)]


def frame_diff(a, b):
    return sum(map(abs, map(operator.sub, a, b))) / (255.0 * len(a))


def motion_size(meta):
    """Frame size for motion analysis: MOTION_LONG_SIDE px on the long side, at least 2 px."""
    width, height = float(meta["width"]), float(meta["height"])
    scale = MOTION_LONG_SIDE / max(width, height)
    return max(2, round(width * scale)), max(2, round(height * scale))


def grey_frames(video, w, h):
    proc = ffmpeg("-i", str(video), "-map", "0:v:0", "-vf", f"scale={w}:{h}:flags=area", "-fps_mode", "passthrough",
                  "-f", "rawvideo", "-pix_fmt", "gray", "-", binary=True)
    if proc.returncode != 0:
        raise InputError(f"ffmpeg cannot decode grey frames of {video}")
    size = w * h
    return [proc.stdout[i:i + size] for i in range(0, len(proc.stdout) - size + 1, size)]


def motion_frames(video, meta):
    """Every frame as small grey raw bytes, plus diffs[i] = diff(frame i-1, frame i), diffs[0] = 0."""
    frames = grey_frames(video, *motion_size(meta))
    diffs = [0.0] + [frame_diff(a, b) for a, b in zip(frames, frames[1:])]
    return types.SimpleNamespace(frames=frames, diffs=diffs, fps=float(meta["fps"]))


# ---------------------------------------------------------------- checks
# Each check returns (details: dict, errors: list of (message, time_sec or None)).
# A details dict with "skipped": True means the check did not run. With "applies": False
# the check does not apply to this input (state skip). A check that applies but did not
# run also returns an error, so its state is fail.

def skip(reason, **extra):
    """The check does not apply to this input."""
    return {"skipped": True, "applies": False, "reason": reason, **extra}, []


def skip_applicable(reason, **extra):
    """The check applies to this input but cannot run: a fail."""
    return {"skipped": True, "applies": True, "reason": reason, **extra}, \
        [(f"check applies but did not run: {reason}", None)]


def check_state(details, errors):
    return "fail" if errors else "skip" if details.get("skipped") else "pass"


def scene_files(root):
    """Scene source files under root, without vendored and minified code."""
    if root is None or not root.is_dir():
        return []
    return [path for path in sorted(root.rglob("*"))
            if path.suffix in SCAN_SUFFIXES and path.is_file() and not path.name.endswith(".min.js")
            and not SCAN_SKIP_DIRS & set(path.relative_to(root).parts[:-1])]


def has_html_scenes(ctx):
    return any(path.suffix == ".html" for path in scene_files(ctx.scenes_dir))


def draws_rasters(ctx):
    """True when a scene source draws a raster image or video, or the project has capture plates."""
    texts = (strip_comments(path.read_text(errors="replace")) for path in scene_files(ctx.scenes_dir))
    captures = ctx.project / "captures"
    return any(RASTER_SOURCE.search(text) for text in texts) or (
        captures.is_dir() and any(path.is_file() for path in captures.iterdir()))

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


def line_at(text, offset):
    return text.count("\n", 0, offset) + 1


def forbidden_call_errors(rel, text):
    return [(f"{rel}:{line_at(text, m.start())}: scene code calls {name}", None)
            for name, pattern in FORBIDDEN_CALLS for m in re.finditer(pattern, text)]


def css_motions(text, suffix):
    """(offset, match) of each CSS animation or transition that is not none."""
    found = [(offset + m.start(), m) for offset, block in css_blocks(text, suffix) for m in CSS_MOTION.finditer(block)]
    return [(at, m) for at, m in found if m.group(2).strip().lower() not in ("", "none")]


def css_motion_errors(rel, text, suffix):
    return [(f"{rel}:{line_at(text, at)}: free-running CSS {m.group(1)}: {m.group(2).strip()}", None)
            for at, m in css_motions(text, suffix)]


def scan_scene_file(root, path):
    """Return (scene unit, defines the seek hook, errors) for 1 scene source file."""
    rel = path.relative_to(root)
    text = strip_comments(path.read_text(errors="replace"))
    errors = forbidden_call_errors(rel, text) + css_motion_errors(rel, text, path.suffix)
    return rel.parts[0] if len(rel.parts) > 1 else ".", bool(SEEK_HOOK.search(text)), errors


def scene_units(scans):
    units = {}
    for unit, has_hook, _ in scans:
        units[unit] = units.get(unit, False) or has_hook
    return units


def empty_scan_errors(root, scanned):
    message = f"files_scanned: 0; {root} holds no scene source ({', '.join(sorted(SCAN_SUFFIXES))})"
    return [] if scanned else [(message, None)]


def hook_errors(units):
    return [(f"scene {unit} does not define window.__seekToFrame", None)
            for unit, has_hook in sorted(units.items()) if not has_hook]


def scan_errors(root, scans, units):
    return sum((scan[2] for scan in scans), []) + hook_errors(units) + empty_scan_errors(root, len(scans))


def check_scene_determinism(ctx):
    root = ctx.scenes_dir
    if root is None or not root.is_dir():
        return skip("no scenes directory")
    scans = [scan_scene_file(root, path) for path in scene_files(root)]
    units = scene_units(scans)
    return {"files_scanned": len(scans), "scenes": sorted(units)}, scan_errors(root, scans, units)


def psnr_avg(a, b):
    proc = ffmpeg("-i", str(a), "-i", str(b), "-lavfi", "psnr", "-f", "null", "-")
    m = re.search(r"average:(\S+)", proc.stderr)
    if proc.returncode != 0 or not m:
        raise InputError(f"ffmpeg psnr failed for {a} and {b}: {proc.stderr.strip()[-300:]}")
    return math.inf if m.group(1) == "inf" else float(m.group(1))


def missing_probes(ctx):
    reason = "no --probe-forward/--probe-reverse frame sets"
    if has_html_scenes(ctx):
        return skip_applicable(f"{reason} for the HTML scenes in {ctx.scenes_dir}")
    return skip(f"{reason}; no HTML scenes")


def probe_set_errors(fwd_names, rev_names, pairs):
    differ = (f"probe sets differ: forward-only {sorted(fwd_names - rev_names)}, "
              f"reverse-only {sorted(rev_names - fwd_names)}", None)
    few = (f"need >= 3 probe frames (0, K, 2K), found {len(pairs)}", None)
    return [differ] * (fwd_names != rev_names) + [few] * (len(pairs) < 3)


def psnr_error(name, value):
    message = f"{name}: cold-forward vs warm-reverse PSNR {value:.2f} dB < {PSNR_MIN_DB} dB"
    return [(message, None)] if value < PSNR_MIN_DB else []


def png_names(folder):
    return {p.name for p in folder.glob("*.png")}


def psnr_values(fwd, rev, pairs):
    return {name: psnr_avg(fwd / name, rev / name) for name in pairs}


def psnr_errors(psnr):
    return [error for name, value in psnr.items() for error in psnr_error(name, value)]


def rounded_db(psnr):
    return {name: "inf" if math.isinf(value) else round(value, 2) for name, value in psnr.items()}


def check_render_determinism(ctx):
    if ctx.probe_forward is None:
        return missing_probes(ctx)
    fwd_names, rev_names = png_names(ctx.probe_forward), png_names(ctx.probe_reverse)
    pairs = sorted(fwd_names & rev_names)
    psnr = psnr_values(ctx.probe_forward, ctx.probe_reverse, pairs)
    errors = probe_set_errors(fwd_names, rev_names, pairs) + psnr_errors(psnr)
    return {"psnr_avg_db": rounded_db(psnr), "min_db": PSNR_MIN_DB}, errors


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


def format_fit_errors(fmt, length):
    if fmt not in FORMAT_RANGES_SEC:
        return [(f"meta.format is {fmt!r}; it must be 'short' or 'long'", None)]
    low, high = FORMAT_RANGES_SEC[fmt]
    if length is None:
        return [("master length is unknown", None)]
    if not low - DURATION_TOL_SEC <= length <= high + DURATION_TOL_SEC:
        return [(f"master length {length:.2f} s is outside the {fmt} range {low:g}-{high:g} s", None)]
    return []


def check_format_fit(ctx):
    """The encoded master length must fit the range of meta.format."""
    fmt, length = ctx.meta.get("format"), stream_duration(ctx.info, "video")
    details = {"format": fmt, "master_sec": length, "range_sec": FORMAT_RANGES_SEC.get(fmt)}
    return details, format_fit_errors(fmt, length)


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


def read_gate_result(path):
    """Read one gate result. Producers use either `pass` or `passed`."""
    path = Path(path)
    label = f"cannot read gate result {path}"
    object_type = dict
    data = None
    try:
        data = json.loads(path.read_text())
    except (OSError, ValueError) as exc:
        raise InputError(f"{label}: {exc}") from exc
    if not isinstance(data, object_type):
        return {}
    return data


def result_source(ctx, path):
    audio_result = path.name in ("audio-check.json", "music-continuity.json")
    return ctx.mix if audio_result else ctx.video


def result_pass(report):
    key = "pass" if "pass" in report else "passed" if "passed" in report else None
    return key, report.get(key) if key else None


def pass_field_error(path, key, passed):
    missing = f"gate result {path} has no pass or passed field"
    failed = f"gate result {path} {key} is {passed!r}, not true"
    return missing if key is None else failed if passed is not True else None


def freshness_error(path, source):
    missing = f"gate result {path} checks missing source {source}"
    stale = source.is_file() and path.stat().st_mtime < source.stat().st_mtime
    return missing if not source.is_file() else f"gate result {path} is older than {source}" if stale else None


def inspect_gate_result(path, source):
    item = {"result": str(path), "source": str(source)}
    if not path.is_file():
        return item, [f"expected gate result is missing: {path}"]
    state = result_pass(read_gate_result(path))
    item.update({"field": state[0], "pass": state[1]})
    errors = [message for message in (pass_field_error(path, *state), freshness_error(path, source)) if message]
    return item, errors


def check_gate_results(ctx):
    results = [inspect_gate_result(path, result_source(ctx, path)) for path in ctx.expected_gate_results]
    checked = [item for item, _ in results]
    errors = [(message, None) for _, messages in results for message in messages]
    return {"expected": checked}, errors


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


def frame_count(video_stream):
    value = str(video_stream.get("nb_frames", ""))
    return int(value) if value.isdigit() else None


def master_frame_count(ctx):
    count = frame_count(stream(ctx.info, "video") or {})
    return len(ctx.analysis["frames"]) if count is None else count


def size_error(name, v, size):
    got = (v.get("width"), v.get("height"))
    return None if got == size else f"{name} is {got[0]}x{got[1]}, expected {size[0]}x{size[1]}"


def duration_error(name, dur, duration):
    near = dur is not None and abs(dur - duration) <= DURATION_TOL_SEC
    return None if near else f"{name} duration {dur} s differs from {duration} s by > {DURATION_TOL_SEC} s"


def count_error(name, count, master_count):
    return None if count == master_count else f"{name} frame count {count}, expected master.mp4 frame count {master_count}"


def check_variant(path, expected):
    """Return (frame count, [error messages]) for 1 derived video.

    `expected` holds the target "size", the master "duration" and the master "frames".
    """
    info = ffprobe(path)
    v = stream(info, "video") or {}
    count = frame_count(v)
    errors = (size_error(path.name, v, expected["size"]),
              duration_error(path.name, stream_duration(info, "video"), expected["duration"]),
              count_error(path.name, count, expected["frames"]))
    return count, [e for e in errors if e]


def variant_targets(meta):
    short, long_ = sorted((int(meta["width"]), int(meta["height"])))
    return {"16x9.mp4": (long_, short), "9x16.mp4": (short, long_), "1x1.mp4": (short, short)}


CAPTION_FILES = (("captions.srt", False), ("captions.vtt", True))


def present(root, names):
    return [name for name in names if (root / name).is_file()]


def variant_report(ctx):
    """Return ({video name: frame count}, [error messages]) for master.mp4 and each present derived video."""
    expected = {"duration": float(ctx.meta["duration_sec"]), "frames": master_frame_count(ctx)}
    targets = variant_targets(ctx.meta)
    results = {name: check_variant(ctx.render_dir / name, {**expected, "size": targets[name]})
               for name in present(ctx.render_dir, targets)}
    frame_counts = {"master.mp4": expected["frames"], **{name: res[0] for name, res in results.items()}}
    return frame_counts, [msg for res in results.values() for msg in res[1]]


def caption_errors(root, duration):
    return [msg for name, vtt in CAPTION_FILES if (root / name).is_file()
            for msg in check_captions(root / name, vtt, duration)]


def checked_files(root, frame_counts):
    """The derived videos and caption files present in `root`."""
    return [name for name in frame_counts if name != "master.mp4"] + present(root, [n for n, _ in CAPTION_FILES])


def check_variants_captions(ctx):
    frame_counts, errors = variant_report(ctx)
    seen = checked_files(ctx.render_dir, frame_counts)
    if not seen:
        return skip(f"no variants or captions in {ctx.render_dir}")
    errors += caption_errors(ctx.render_dir, float(ctx.meta["duration_sec"]))
    return {"checked": seen, "frame_counts": frame_counts}, [(msg, None) for msg in errors]


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


def find_pops(motion):
    """Frames that jump away for exactly 1 frame and come back."""
    f, d = motion.frames, motion.diffs
    pops = []
    for i in range(1, len(f) - 1):
        jump = min(d[i], d[i + 1])
        if jump >= POP_MIN_DIFF and frame_diff(f[i - 1], f[i + 1]) <= POP_RETURN_RATIO * jump:
            pops.append(i)
    return pops


def mean_luma(frame):
    return sum(frame) / (255.0 * len(frame))


def fade_hold(scene):
    return scene.get("static_hold") is True and "fade" in str(scene.get("static_hold_reason", "")).lower()


def scene_last_frames(scene, motion):
    end_frame = min(len(motion.frames), math.ceil(scene_end(scene) * motion.fps))
    return end_frame - 2, end_frame - 1


def cut_flash_sample(scene, motion):
    pair = scene_last_frames(scene, motion)
    lumas = [mean_luma(motion.frames[i]) for i in pair] if pair[0] >= 0 and pair[1] < len(motion.frames) else []
    return (*pair, lumas, fade_hold(scene)) if lumas else None


def cut_flash_measurement(scene, sample):
    first, last, lumas, exempt = sample
    return {"scene": scene.get("id"), "frames": [first, last],
            "luma": [round(value * 255, 2) for value in lumas], "fade_hold": exempt}


def cut_flash_error(scene, motion, sample):
    delta = abs(sample[2][1] - sample[2][0])
    message = (f"scene {scene.get('id')} frames {sample[0]} and {sample[1]} mean luma "
               f"{sample[2][0] * 255:.2f} and {sample[2][1] * 255:.2f} differ by "
               f"{delta:.1%} (max {CUT_LUMA_DELTA:.0%})")
    return None if sample[3] or delta <= CUT_LUMA_DELTA else (message, sample[0] / motion.fps)


def cut_flash_result(scene, motion):
    sample = cut_flash_sample(scene, motion)
    result = None if sample is None else (cut_flash_measurement(scene, sample),
                                          cut_flash_error(scene, motion, sample))
    return result


def check_cut_flash(ctx):
    results = [result for scene in ctx.scenes if (result := cut_flash_result(scene, ctx.motion))]
    measured = [item for item, _ in results]
    errors = [error for _, error in results if error]
    return {"max_delta": CUT_LUMA_DELTA, "scenes": measured}, errors


def check_frame_pops(ctx):
    m, errors = ctx.motion, []
    pops = find_pops(m)
    for i in pops:
        delta = mean_luma(m.frames[i]) - mean_luma(m.frames[i - 1])
        kind = "flash" if abs(delta) > FLASH_DELTA else "pop"
        t = i / m.fps
        errors.append((f"1-frame {kind} at frame {i} in scene {scene_at(ctx.scenes, t)} "
                       f"(jump {m.diffs[i]:.3f}, mean luma change {delta:+.2f})", t))
    return {"pop_frames": pops, "min_jump": POP_MIN_DIFF}, errors


def find_cuts(motion, skip):
    d, r = motion.diffs, CUT_MEDIAN_RADIUS
    cuts = []
    for i in range(1, len(d)):
        if i in skip or d[i] < CUT_MIN_DIFF:
            continue
        local = sorted(d[max(1, i - r):i] + d[i + 1:i + 1 + r])
        median = local[len(local) // 2] if local else 0.0
        if d[i] >= CUT_MEDIAN_RATIO * median:
            cuts.append(i)
    return cuts


def check_cuts_vs_plan(ctx):
    m = ctx.motion
    pops = find_pops(m)
    skip = set(pops) | {i + 1 for i in pops}
    boundaries = [float(s["start_sec"]) for s in ctx.scenes[1:]]
    cuts, errors = [], []
    for i in find_cuts(m, skip):
        t = i / m.fps
        near = min((abs(t - b) for b in boundaries), default=math.inf)
        planned = near <= CUT_MATCH_SEC
        cuts.append({"time_sec": round(t, 3), "diff": round(m.diffs[i], 3), "planned": planned})
        if not planned:
            errors.append((f"unplanned cut at {stamp(t)} in scene {scene_at(ctx.scenes, t)}: no scene "
                           f"boundary within {CUT_MATCH_SEC} s", t))
    hard = [float(s["start_sec"]) for prev, s in zip(ctx.scenes, ctx.scenes[1:]) if transition_kind(prev) == "cut"]
    missing = [stamp(b) for b in hard if not any(abs(c["time_sec"] - b) <= CUT_MATCH_SEC for c in cuts)]
    return {"cuts": cuts, "match_sec": CUT_MATCH_SEC, "planned_cuts_not_seen": missing}, errors


def still_runs(motion):
    """(start_sec, end_sec) of runs where every frame step is below DEAD_DIFF."""
    runs, start = [], None
    for i, value in enumerate(motion.diffs[1:] + [math.inf], start=1):
        if value < DEAD_DIFF:
            start = i - 1 if start is None else start
        elif start is not None:
            runs.append((start / motion.fps, (i - 1) / motion.fps))
            start = None
    return runs


def dead_runs(ctx):
    """Return (start, end, planned) for each still run of DEAD_MAX_SEC or longer."""
    windows, tol = hold_windows(ctx.scenes), 1.0 / ctx.motion.fps
    runs = [(start, end) for start, end in still_runs(ctx.motion) if end - start >= DEAD_MAX_SEC]
    return [(start, end, inside(start, end, windows, tol)) for start, end in runs]


def dead_message(scenes, start, end):
    return (f"dead time {stamp(start)}-{stamp(end)} ({end - start:.2f} s without motion) "
            f"in scene {scene_at(scenes, start)} outside a static_hold scene")


def dead_intervals(runs):
    return [{"start_sec": round(start, 3), "end_sec": round(end, 3), "planned": planned}
            for start, end, planned in runs]


def check_dead_time(ctx):
    runs = dead_runs(ctx)
    errors = [(dead_message(ctx.scenes, start, end), start) for start, end, planned in runs if not planned]
    return {"max_sec": DEAD_MAX_SEC, "still_diff": DEAD_DIFF, "intervals": dead_intervals(runs)}, errors


def motion_per_second(motion):
    """Sum of frame differences in each whole second of the video."""
    seconds = int(len(motion.diffs) / motion.fps)
    per = [0.0] * seconds
    for i, value in enumerate(motion.diffs):
        s = int(i / motion.fps)
        if s < seconds:
            per[s] += value
    return [round(v, 4) for v in per]


def flat_opening_errors(opening, rest):
    message = (f"flat opening: motion {opening:.3f}/s in 0-{OPENING_SEC:g} s is under "
               f"{OPENING_MIN_RATIO:g} x the rest of the film ({rest:.3f}/s)")
    return [(message, 0.0)] if opening < OPENING_MIN_RATIO * rest else []


def check_flat_opening(ctx):
    per = motion_per_second(ctx.motion)
    head = int(OPENING_SEC)
    if len(per) < head + OPENING_MIN_REST_SEC:
        return skip(f"shorter than {OPENING_SEC + OPENING_MIN_REST_SEC:g} s")
    opening, rest = sum(per[:head]) / head, sum(per[head:]) / len(per[head:])
    details = {"opening_per_sec": round(opening, 4), "rest_per_sec": round(rest, 4), "min_ratio": OPENING_MIN_RATIO}
    return details, flat_opening_errors(opening, rest)


def percentiles_10_90(values):
    ranked = sorted(values)
    return ranked[int(0.1 * (len(ranked) - 1))], ranked[int(math.ceil(0.9 * (len(ranked) - 1)))]


def contrast_errors(low, high):
    message = (f"flat motion: 90th percentile {high:.3f}/s is under {CONTRAST_MIN_RATIO:g} x the "
               f"10th percentile {low:.3f}/s; no calm or busy stretch stands out")
    return [(message, None)] if 0 < high < CONTRAST_MIN_RATIO * low else []


def check_motion_contrast(ctx):
    per = motion_per_second(ctx.motion)
    if len(per) < CONTRAST_MIN_SECONDS:
        return skip(f"shorter than {CONTRAST_MIN_SECONDS} s", per_second=per)
    low, high = percentiles_10_90(per)
    details = {"per_second": per, "p10": low, "p90": high, "min_ratio": CONTRAST_MIN_RATIO}
    return details, contrast_errors(low, high)


def typical_seam_step(motion):
    """The median frame step in the first and last second, around the loop seam."""
    n = max(1, int(motion.fps))
    near = sorted(motion.diffs[1:n + 1] + motion.diffs[-n:])
    return near[len(near) // 2]


def seam_errors(motion, seam, typical, limit):
    message = (f"loop seam: last frame to frame 0 differs by {seam:.3f}, above {limit:.3f} "
               f"({LOOP_SEAM_FACTOR:g} x the typical step {typical:.3f})")
    return [(message, (len(motion.frames) - 1) / motion.fps)] if seam > limit else []


def check_loop_seam(ctx):
    if ctx.meta.get("format") != "short":
        return skip("meta.format is not short")
    if ctx.meta.get("loop") is not True:
        return skip("meta.loop is not true")
    typical = typical_seam_step(ctx.motion)
    seam = frame_diff(ctx.motion.frames[-1], ctx.motion.frames[0])
    limit = max(LOOP_SEAM_FLOOR, LOOP_SEAM_FACTOR * typical)
    details = {"seam_diff": round(seam, 4), "typical_diff": round(typical, 4), "max_diff": round(limit, 4)}
    return details, seam_errors(ctx.motion, seam, typical, limit)


def raster_sample(s):
    """(t_sec, src, natural_px, drawn_px, nearest) of 1 report sample."""
    return (float(s["t_sec"]), str(s["src"]), [float(v) for v in s["natural_px"]],
            [float(v) for v in s["drawn_px"]], s.get("sampling") == "nearest")


def detail_errors(raw):
    """A sample whose pre-upscale detail check threw has no proof of its real resolution: a fail."""
    return [(f"{s['src']}: pre-upscale detail check failed: {s['detail_error']}", float(s["t_sec"]))
            for s in raw if s.get("detail_error")]


def parse_raster_sizes(text):
    data = json.loads(text)
    step = float(data.get("step_sec", RASTER_DEFAULT_STEP_SEC))
    return step, [raster_sample(s) for s in data["samples"]], detail_errors(data["samples"])


def read_raster_sizes(path):
    caught = Caught((OSError, ValueError, KeyError, TypeError, AttributeError))
    with caught:
        return parse_raster_sizes(path.read_text())
    raise InputError(f"cannot read raster sizes {path}: {caught.error}") from caught.error


def load_raster_sizes(path):
    """Return (step_sec, samples, detail check errors) of a raster size report."""
    report = read_raster_sizes(path)
    if any(min(n) <= 0 for _, _, n, _, _ in report[1]):
        raise InputError(f"{path}: natural_px must be positive")
    return report


def longest_run(series, step):
    """Longest span (samples x step) of consecutive samples above the warn scale, and its start time."""
    longest, start, count, first = 0.0, None, 0, None
    for t, scale in series:
        if scale <= RASTER_WARN_SCALE:
            count = 0
            continue
        first = t if count == 0 else first
        count += 1
        if count * step > longest:
            longest, start = count * step, first
    return longest, start


def integer_scale(natural, drawn):
    """The whole-number scale k >= 1 of a draw at k x on both axes, else None."""
    scales = [d / n for d, n in zip(drawn, natural)]
    k = round(scales[0])
    return k if k >= 1 and all(abs(s - k) <= RASTER_INTEGER_TOL for s in scales) else None


def pixel_art(sample):
    """A nearest-neighbour draw at a whole-number scale keeps every source pixel sharp."""
    _, _, natural, drawn, nearest = sample
    return nearest and integer_scale(natural, drawn) is not None


def drawn_scale(sample):
    """The larger axis ratio drawn / natural of 1 sample."""
    natural, drawn = sample[2], sample[3]
    return max(drawn[0] / natural[0], drawn[1] / natural[1])


def raster_series(samples):
    """Map each raster source to its [(time, drawn scale)] samples, in time order.

    Pixel-art samples (nearest-neighbour at a whole-number scale) are left out: they pass.
    """
    by_src = {}
    for sample in sorted(s for s in samples if not pixel_art(s)):
        by_src.setdefault(sample[1], []).append((sample[0], drawn_scale(sample)))
    return sorted(by_src.items())


def raster_error(src, series):
    t_max, peak = max(series, key=operator.itemgetter(1))
    message = f"{src} drawn at {peak:.2f}x its natural size (fail at {RASTER_FAIL_SCALE:g}x)"
    return (message, t_max) if peak >= RASTER_FAIL_SCALE else None


def raster_warning(src, series, step):
    longest, start = longest_run(series, step)
    if max(scale for _, scale in series) >= RASTER_FAIL_SCALE or longest < RASTER_WARN_SEC:
        return None
    return ("raster_upscale", f"{src} drawn above {RASTER_WARN_SCALE:g}x for {longest:.2f} s "
            f"(warn at {RASTER_WARN_SEC:g} s)", start)


def raster_errors(series):
    return [e for e in (raster_error(src, points) for src, points in series) if e]


def raster_warnings(series, step):
    return [w for w in (raster_warning(src, points, step) for src, points in series) if w]


def raster_worst(series):
    return {src: round(max(scale for _, scale in points), 3) for src, points in series}


def raster_details(path, samples, series):
    return {"report": str(path), "samples": len(samples), "max_scale": raster_worst(series),
            "pixel_art_samples": sum(map(pixel_art, samples)),
            "warn_scale": RASTER_WARN_SCALE, "fail_scale": RASTER_FAIL_SCALE}


def missing_raster_report(ctx, path):
    reason = f"no raster size report at {path}"
    if draws_rasters(ctx):
        return skip_applicable(f"{reason}; the scenes draw images, video or capture plates")
    return skip(f"{reason}; no scene draws a raster")


def empty_report_errors(path, samples):
    return [] if samples else [(f"{path} has 0 samples; an empty size report proves nothing", None)]


def check_raster_upscale(ctx):
    path = ctx.raster_sizes
    if path is None or not path.is_file():
        return missing_raster_report(ctx, path)
    step, samples, detail = load_raster_sizes(path)
    series = raster_series(samples)
    ctx.warnings += raster_warnings(series, step)
    errors = empty_report_errors(path, samples) + detail + raster_errors(series)
    return raster_details(path, samples, series), errors


CHECK_FUNCS = {
    "scene_determinism": check_scene_determinism,
    "render_determinism": check_render_determinism,
    "stream_integrity": check_stream_integrity,
    "format_fit": check_format_fit,
    "frame0_readiness": check_frame0,
    "hook_motion": check_hook_motion,
    "black_frames": check_black,
    "freeze_frames": check_freeze,
    "audio_loudness": check_loudness,
    "audio_boundary_pops": check_boundary_pops,
    "gate_results": check_gate_results,
    "variants_captions": check_variants_captions,
    "safe_zone_flash": check_safe_zone_flash,
    "cut_flash": check_cut_flash,
    "frame_pops": check_frame_pops,
    "cuts_vs_plan": check_cuts_vs_plan,
    "dead_time": check_dead_time,
    "flat_opening": check_flat_opening,
    "motion_contrast": check_motion_contrast,
    "loop_seam": check_loop_seam,
    "raster_upscale": check_raster_upscale,
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


def make_sheet(ctx, out, fps, end_sec, max_tiles=None, tile_w=None):
    """Tiles SHEET_TILE_H px high, or tile_w px wide when tile_w is set."""
    w, h = ctx.meta["width"], ctx.meta["height"]
    if tile_w:
        tw, th = tile_w, max(2, 2 * round(tile_w * h / w / 2))
    else:
        th, tw = SHEET_TILE_H, max(2, 2 * round(w * SHEET_TILE_H / h / 2))
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


def hero_frame_time(meta):
    hero = meta.get("hero_sec")
    return sum(map(float, hero)) / 2 if isinstance(hero, list) and len(hero) == 2 else float(meta["duration_sec"]) / 2


def scene_phone_times(scene):
    actions = scene.get("locked_actions_sec") or [float(scene["duration_sec"]) / 2]
    label = re.sub(r"[^A-Za-z0-9_-]+", "-", str(scene.get("id", "scene"))).strip("-") or "scene"
    start = float(scene["start_sec"])
    return [(f"{label}-action-{index:02d}", start + float(relative))
            for index, relative in enumerate(actions, start=1)]


def phone_frame_times(ctx):
    actions = [pick for scene in ctx.scenes for pick in scene_phone_times(scene)]
    picks = [("hero", hero_frame_time(ctx.meta)), *actions]
    limit = float(ctx.meta["duration_sec"]) - 1.0 / float(ctx.meta["fps"])
    return [(label, min(max(t, 0.0), limit)) for label, t in picks]


def extract_phone_frame(video, out, time_sec):
    proc = ffmpeg("-y", "-ss", f"{time_sec:.6f}", "-i", str(video), "-frames:v", "1",
                  "-vf", f"scale=-2:{PHONE_FRAME_H}", str(out))
    if proc.returncode != 0 or not out.is_file():
        raise InputError(f"cannot extract phone frame at {time_sec:.3f} s: {proc.stderr.strip()[-300:]}")


def write_phone_frame(ctx, pick):
    label, time_sec = pick
    out = ctx.phone_frames / f"{label}.png"
    extract_phone_frame(ctx.video, out, time_sec)
    return {"path": str(out), "time_sec": round(time_sec, 3)}


def make_phone_frames(ctx):
    ctx.phone_frames.mkdir(parents=True, exist_ok=True)
    files = [write_phone_frame(ctx, pick) for pick in phone_frame_times(ctx)]
    return {"directory": str(ctx.phone_frames), "height_px": PHONE_FRAME_H, "frames": files}


# ---------------------------------------------------------------- driver

def path_or(value, default):
    return Path(value) if value else default


class Context:
    def __init__(self, args):
        self.video = Path(args.video)
        self.timeline = Path(args.timeline)
        if not self.video.is_file():
            raise InputError(f"video not found: {self.video}")
        project = self.timeline.resolve().parent
        self.load_timelines(project)
        self.set_paths(args, project)
        self.warnings = []
        self.set_probes(args)
        self.analyse_video()

    def load_timelines(self, project):
        """Read meta from timeline.json, and scenes from timeline-master.json when it exists."""
        self.meta, self.scenes = load_timeline(self.timeline)
        master = project / "timeline-master.json"
        self.scene_timeline = master if master.is_file() else self.timeline
        self.scenes = load_timeline(self.scene_timeline)[1]

    def set_paths(self, args, project):
        self.project = project
        self.set_input_paths(args, project)
        self.set_output_paths(args, project / "qa")
        self.set_gate_paths(args, project)

    def set_input_paths(self, args, project):
        self.scenes_dir = path_or(args.scenes_dir, project / "scenes")
        self.render_dir = path_or(args.render_dir, self.video.resolve().parent)
        self.raster_sizes = path_or(args.raster_sizes, self.render_dir / "raster-sizes.json")

    def set_output_paths(self, args, qa):
        self.out = path_or(args.out, qa / "render-check.json")
        self.contact_sheet = path_or(args.contact_sheet, qa / "contact-sheet-360p.png")
        self.hook_strip = path_or(args.hook_strip, qa / "hook-strip.png")
        self.phone_sheet = path_or(args.phone_sheet, qa / "phone-sheet-360w.png")
        self.phone_frames = path_or(args.phone_frames, None)

    def set_gate_paths(self, args, project):
        self.mix = project / "audio" / "mix.wav"
        expected = args.expect if args.expect is not None else DEFAULT_GATE_RESULTS
        self.expected_gate_results = [project / value for value in expected]

    def set_probes(self, args):
        if (args.probe_forward is None) != (args.probe_reverse is None):
            raise InputError("--probe-forward and --probe-reverse go together")
        self.probe_forward = path_or(args.probe_forward, None)
        self.probe_reverse = path_or(args.probe_reverse, None)

    def analyse_video(self):
        self.info = ffprobe(self.video)
        if stream(self.info, "video") is None:
            raise InputError(f"{self.video} has no video stream")
        self.analysis = analyse(self.video, stream(self.info, "audio") is not None)
        self.motion = motion_frames(self.video, self.meta)


def time_fields(t):
    return {} if t is None else {"time_sec": round(t, 3), "timestamp": stamp(t)}


def run_checks(ctx):
    """Return ({check name: details}, [error entries]) for every check in CHECKS."""
    results = {name: CHECK_FUNCS[name](ctx) for name in CHECKS}
    states = {name: check_state(details, found) for name, (details, found) in results.items()}
    checks = {name: {**details, "state": states[name], "passed": states[name] != "fail"}
              for name, (details, _) in results.items()}
    return checks, error_entries(results)


def state_counts(checks):
    states = [c["state"] for c in checks.values()]
    return {state: states.count(state) for state in STATES}


def error_entries(results):
    return [{"check": name, "message": message, **time_fields(t)}
            for name, (_, found) in results.items() for message, t in found]


def warning_entries(warnings):
    return [{"check": name, "message": message, **time_fields(t)} for name, message, t in warnings]


def write_report(out, report):
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, indent=2) + "\n")


def make_sheets(ctx):
    duration = float(ctx.meta["duration_sec"])
    artifacts = {
        "contact_sheet": make_sheet(ctx, ctx.contact_sheet, CONTACT_FPS, duration),
        "hook_strip": make_sheet(ctx, ctx.hook_strip, HOOK_FPS, HOOK_WINDOW_SEC,
                                 max_tiles=int(HOOK_WINDOW_SEC * HOOK_FPS)),
        "phone_sheet": make_sheet(ctx, ctx.phone_sheet, CONTACT_FPS, duration, tile_w=PHONE_TILE_W),
    }
    if ctx.phone_frames is not None:
        artifacts["phone_frames"] = make_phone_frames(ctx)
    return artifacts


def gate_report(ctx, checks, errors):
    counts = state_counts(checks)
    failed = bool(errors) or counts["fail"] > 0
    return {"passed": not failed, "state": "fail" if failed else "pass", "counts": counts,
            "video": str(ctx.video), "timeline": str(ctx.timeline),
            "scene_timeline": str(ctx.scene_timeline), "checks": checks, "errors": errors}


def run_gate(args):
    ctx = Context(args)
    report = gate_report(ctx, *run_checks(ctx))
    report["warnings"] = warning_entries(ctx.warnings)
    report["artifacts"] = make_sheets(ctx)
    write_report(ctx.out, report)
    return report, ctx.out


def require_tools():
    missing = [tool for tool in ("ffmpeg", "ffprobe") if shutil.which(tool) is None]
    if missing:
        raise InputError(f"missing on PATH: {', '.join(missing)}")


# ---------------------------------------------------------------- self-test

SCENE_OK = """<!doctype html><html><head><style>.dot { transform: translateX(0); }</style></head>
<body><div class="dot"></div><script>
// Pure function of (frameIndex, fps). No clocks, no timers, no random numbers.
window.__seekToFrame = function (frameIndex, fps) {
  const wobble = 0.37;
  document.querySelector('.dot').style.transform = 'translateX(' + (frameIndex / fps * 100 + wobble) + 'px)';
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


FIXTURE_SEC = 15.0   # the shortest short film
CALM_FROM_SEC = 8.0  # a plate covers most of the frame from here, so motion_contrast sees a calm stretch


def make_fixture_video(path, vf="null", tone_db=None, clip=False, width=320, height=180, fps=30, dur=FIXTURE_SEC):
    src = f"testsrc2=s={width}x{height}:r={fps}:d={dur}"
    vf = f"{vf},drawbox=x=0:y=0:w=iw:h=ih*0.5:color=0x203040:t=fill:enable='gte(t,{CALM_FROM_SEC:g})'"
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


def fixture_timeline(static_hold):
    return {
        "meta": {"id": "self-test", "title": "Self test", "aspect": "16:9", "fps": 30, "width": 320,
                 "height": 180, "duration_sec": FIXTURE_SEC, "format": "short", "lufs_target": -14.0, "hero_sec": [2.5, 3.5]},
        "scenes": [
            {"id": "s01-hook", "kind": "hook", "start_sec": 0.0, "duration_sec": 2.0,
             "transition_out": {"type": "cut", "duration_sec": 0.0}, "locked_actions_sec": [0.5, 1.5],
             "critical_regions": [[0.1, 0.2, 0.8, 0.4]], "focal_point": [0.5, 0.4]},
            {"id": "s02-demo", "kind": "demo", "start_sec": 2.0, "duration_sec": CALM_FROM_SEC - 2.0,
             "transition_out": {"type": "cut", "duration_sec": 0.0}, "focal_point": [0.5, 0.5]},
            {"id": "s03-cta", "kind": "cta", "start_sec": CALM_FROM_SEC, "duration_sec": FIXTURE_SEC - CALM_FROM_SEC,
             "static_hold": static_hold,
             "critical_regions": [[0.2, 0.3, 0.6, 0.3]], "focal_point": [0.5, 0.45]},
        ],
    }


def make_variants(render, fps):
    sizes = (("16x9.mp4", "320:180"), ("9x16.mp4", "180:320"), ("1x1.mp4", "180:180"))
    codec = ["-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", "-r", str(fps)]
    master = str(render / "master.mp4")
    for name, size in sizes:
        sh("-i", master, "-vf", f"scale={size}", "-an", *codec, str(render / name))


def make_probe_frame(side, frame):
    side.mkdir(parents=True, exist_ok=True)
    sh("-f", "lavfi", "-i", "testsrc2=s=320x180:r=30:d=4", "-vf", f"select=eq(n\\,{frame})",
       "-frames:v", "1", str(side / f"frame-{frame:06d}.png"))


def make_probe_pair(forward, reverse, frame):
    make_probe_frame(forward, frame)
    shutil.copy2(forward / f"frame-{frame:06d}.png", reverse / f"frame-{frame:06d}.png")


def make_probes(probe):
    forward, reverse = probe / "forward", probe / "reverse"
    reverse.mkdir(parents=True, exist_ok=True)
    make_probe_pair(forward, reverse, 0)
    make_probe_pair(forward, reverse, 30)
    make_probe_pair(forward, reverse, 60)


def write_fixture_files(root, opts):
    (root / "timeline.json").write_text(json.dumps(fixture_timeline(opts["static_hold"]), indent=2))
    (root / "scenes" / "s01-hook").mkdir(parents=True)
    (root / "scenes" / "s01-hook" / "index.html").write_text(opts["scene_html"])
    (root / "render" / "captions.srt").write_text(SRT_OK)
    (root / "render" / "captions.vtt").write_text(VTT_OK)


def write_gate_result(root, name, value):
    path = root / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value))


def write_gate_results(root):
    write_gate_result(root, "qa/audio-check.json", {"passed": True})
    write_gate_result(root, "audio/music-continuity.json", {"pass": True})
    write_gate_result(root, "qa/timeline-check.json", {"passed": True})
    write_gate_result(root, "qa/plate-check.json", {"passed": True})


FIXTURE_DEFAULTS = {"video_vf": "null", "clip": False, "scene_html": SCENE_OK, "gain": 0.0, "probes": True,
                    "static_hold": False, "variants": False, "variant_fps": 30}


def write_project_sources(root, render, opts):
    render.mkdir(parents=True)
    make_fixture_video(render / "master.mp4", vf=opts["video_vf"], tone_db=opts["gain"], clip=opts["clip"])
    write_fixture_files(root, opts)
    (root / "audio").mkdir(exist_ok=True)
    (root / "audio" / "mix.wav").write_bytes(b"self-test audio source")
    write_gate_results(root)


def write_optional_fixtures(root, render, opts):
    variants = make_variants(render, opts["variant_fps"]) if opts["variants"] else None
    probes = make_probes(root / "probe") if opts["probes"] else None
    return variants, probes


def make_project(root, **options):
    """Build a self-test project. `options` override FIXTURE_DEFAULTS."""
    opts = {**FIXTURE_DEFAULTS, **options}
    render = root / "render"
    write_project_sources(root, render, opts)
    write_optional_fixtures(root, render, opts)
    return root


def gate_args(root, probes=None):
    """Gate arguments; probes default to on when the project has probe frames."""
    argv = ["--video", str(root / "render" / "master.mp4"), "--timeline", str(root / "timeline.json")]
    if probes or (probes is None and (root / "probe").is_dir()):
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


GATE_CASES = [  # (name, make_project options, failed checks, text that 1 error message must hold)
    ("pass", {}, set(), None),
    ("black-frame", {"video_vf": "drawbox=x=0:y=0:w=iw:h=ih:color=black:t=fill:enable='between(t,1.2,1.7)'"},
     {"black_frames", "cuts_vs_plan"}, None),
    ("dark-background", {"video_vf": "drawbox=x=0:y=0:w=iw:h=ih:color=0x0B1020:t=fill:enable='between(t,1.2,1.7)'"},
     {"cuts_vs_plan"}, None),  # the jump to the dark plate is a cut, but not black
    ("frozen", {"video_vf": "trim=end=2,tpad=stop_mode=clone:stop_duration=13"},
     {"dead_time", "freeze_frames"}, None),
    ("raf-scene", {"scene_html": SCENE_RAF}, {"scene_determinism"}, None),
    ("variant-frame-count", {"variants": True, "variant_fps": 24}, {"variants_captions"}, "frame count"),
    ("clipped-audio", {"clip": True}, {"audio_loudness", "audio_boundary_pops"}, None),
]


def stray_files(skill_dir):
    """Files in the skill that are not text sources."""
    files = [p for p in skill_dir.rglob("*") if p.is_file() and "__pycache__" not in p.parts]
    return [p for p in files if p.suffix not in (".md", ".py", ".mjs", ".html")]


def self_test_static():
    skill_dir = Path(__file__).resolve().parent.parent
    size = (skill_dir / "SKILL.md").stat().st_size
    assert size < SKILL_MD_MAX_BYTES, f"SKILL.md is {size} bytes"
    assert not stray_files(skill_dir), f"non-text files in skill: {stray_files(skill_dir)}"
    assert (parse_captions(SRT_OK, False)[1] == (2.1, 3.8)), "SRT parser"
    for bad in ("1\n00:00:02,000 --> 00:00:01,000\nx\n", "2\n00:00:00,000 --> 00:00:02,000\na\n\n"
                "3\n00:00:01,000 --> 00:00:03,000\nb\n"):
        assert check_captions_text(bad), f"caption error not found in {bad!r}"


def assert_sheets(qa):
    assert_png(qa / "contact-sheet-360p.png", 6 * 640, 5 * 360)
    assert_png(qa / "hook-strip.png", 6 * 640, 2 * 360)
    assert_png(qa / "phone-sheet-360w.png", 6 * PHONE_TILE_W, 5 * 202)


def assert_gate_report(root, case):
    """Check the report of 1 GATE_CASES run. Return the failed check names."""
    name, _, expected, must_say = case
    report, failed = failed_checks(root)
    assert failed == expected, f"{name}: failed checks {sorted(failed)}, expected {sorted(expected)}\n" \
                               f"{json.dumps(report['errors'], indent=2)}"
    assert report["passed"] is (not expected), f"{name}: passed flag wrong"
    assert must_say is None or any(must_say in e["message"] for e in report["errors"]), \
        f"{name}: no {must_say!r} error in {report['errors']}"
    return failed


# The pass fixture is a 15 s short film with no loop and no raster: loop_seam and
# raster_upscale do not apply.
PASS_COUNTS = {"pass": len(CHECKS) - 2, "skip": 2, "fail": 0}


def assert_pass_counts(report, stdout):
    assert report["counts"] == PASS_COUNTS and report["state"] == "pass", f"pass counts: {report['counts']}"
    line = f"{PASS_COUNTS['pass']} pass / {PASS_COUNTS['skip']} skip / 0 fail"
    assert line in stdout, f"headline lacks {line!r}: {stdout}"


def self_test_gate_case(tmp, gain, case):
    root = make_project(tmp / case[0], gain=gain, **case[1])
    code, proc = invoke(gate_args(root))
    failed = assert_gate_report(root, case)
    assert code == (1 if case[2] else 0), f"{case[0]}: exit {code}\n{proc.stderr}"
    if not case[2]:
        assert_pass_counts(json.loads((root / "qa" / "render-check.json").read_text()), proc.stdout)
    assert_sheets(root / "qa")
    print(f"ok {case[0]}: failed={sorted(failed)} exit={code}")


def self_test_timeline_master(root):
    timeline = json.loads((root / "timeline.json").read_text())
    for scene in timeline["scenes"][1:]:
        scene["static_hold"] = True
    (root / "timeline-master.json").write_text(json.dumps(timeline))
    code, _ = invoke(gate_args(root))
    assert code == 0, "timeline-master.json static_hold scene must allow a planned freeze"
    print("ok timeline-master-static-hold: exit=0")


def write_music_report(root, passed):
    report = {"window_ms": 50, "phrase_bars": 4, "max_drop_db": 12,
              "digital_silence": False, "worst": [], "edits": [], "loop_joins": 0, "pass": passed}
    (root / "audio").mkdir(exist_ok=True)
    (root / "audio" / "music-continuity.json").write_text(json.dumps(report))


def self_test_music(root):
    write_music_report(root, False)
    code, _ = invoke(gate_args(root))
    _, failed = failed_checks(root)
    assert code == 1 and failed == {"gate_results"}, f"music fail: {sorted(failed)}"
    print("ok music-continuity-fail: failed=['gate_results'] exit=1")
    write_music_report(root, True)
    code, _ = invoke(gate_args(root))
    assert code == 0, "music-continuity pass must pass the gate"
    print("ok music-continuity-pass: exit=0")


def phone_test_context(root, out):
    meta, scenes = load_timeline(root / "timeline.json")
    return types.SimpleNamespace(video=root / "render" / "master.mp4", phone_frames=out,
                                 meta=meta, scenes=scenes)


def assert_phone_frames(out, result):
    frames = sorted(out.glob("*.png"))
    assert len(frames) == len(result["frames"]) == 5, f"phone frames: found {len(frames)}, expected 5"
    for path in frames:
        assert_png(path, 640, PHONE_FRAME_H)


def self_test_phone_frames(root):
    out = root / "qa" / "phone-frames"
    result = make_phone_frames(phone_test_context(root, out))
    assert_phone_frames(out, result)
    print("ok phone-frames: count=5 size=640x360")


def self_test_probe_mismatch(root):
    make_probes(root / "probe")
    sh("-f", "lavfi", "-i", "color=c=white:s=320x180:d=1", "-frames:v", "1",
       str(root / "probe" / "reverse" / "frame-000030.png"))
    code, _ = invoke(gate_args(root, probes=True))
    _, failed = failed_checks(root)
    assert code == 1 and failed == {"render_determinism"}, f"probe mismatch: {sorted(failed)}"
    print("ok probe-mismatch: failed=['render_determinism'] exit=1")


def self_test_missing_input(tmp, root):
    out = root / "qa" / "render-check.json"
    out.write_text(json.dumps({"passed": True, "state": "pass", "checks": {}, "errors": []}))
    code, _ = invoke(["--video", str(tmp / "missing.mp4"), "--timeline", str(root / "timeline.json")])
    assert code == 2, f"missing video: exit {code}"
    report = json.loads(out.read_text())
    assert report["passed"] is False and "missing.mp4" in report["input_error"], f"stale report kept: {report}"
    print("ok missing-input: exit=2, the old passing report is replaced by passed=false")


def self_test_gate(tmp):
    gain = tone_gain_db(tmp)
    for case in GATE_CASES:
        self_test_gate_case(tmp, gain, case)
    self_test_timeline_master(tmp / "frozen")
    self_test_music(tmp / "pass")
    self_test_phone_frames(tmp / "pass")
    self_test_probe_mismatch(tmp / "pass")
    self_test_missing_input(tmp, tmp / "pass")


def state_ctx(root, html=None):
    scenes = root / "scenes"
    scenes.mkdir(parents=True)
    if html is not None:
        (scenes / "index.html").write_text(html)
    return types.SimpleNamespace(scenes_dir=scenes, project=root, probe_forward=None, probe_reverse=None,
                                 raster_sizes=root / "render" / "raster-sizes.json", warnings=[])


def assert_state(name, result, want):
    details, errors = result
    got = check_state(details, errors)
    assert got == want, f"{name}: state {got}, expected {want}: {details} {errors}"


def self_test_scan_states(tmp):
    assert_state("empty scenes dir", check_scene_determinism(state_ctx(tmp / "st-empty")), "fail")
    assert_state("no scenes dir", check_scene_determinism(types.SimpleNamespace(scenes_dir=tmp / "none")), "skip")
    html = state_ctx(tmp / "st-html", SCENE_OK)
    assert_state("no probes, HTML scenes", check_render_determinism(html), "fail")
    assert_state("no raster report, no raster", check_raster_upscale(html), "skip")
    no_html = types.SimpleNamespace(scenes_dir=tmp / "none", probe_forward=None, probe_reverse=None)
    assert_state("no probes, no HTML scenes", check_render_determinism(no_html), "skip")


def self_test_states(tmp):
    self_test_scan_states(tmp)
    img = state_ctx(tmp / "st-img", SCENE_OK.replace("<body>", '<body><img src="plate.png">'))
    assert_state("no raster report, scene draws an image", check_raster_upscale(img), "fail")
    video = state_ctx(tmp / "st-video", "<canvas></canvas><script>ctx.drawImage(clip, 0, 0);</script>")
    assert_state("no raster report, canvas drawImage", check_raster_upscale(video), "fail")
    print("ok states: files_scanned 0 fails, applicable skips fail, other skips skip")


def self_test_in(tmp):
    self_test_states(tmp)
    self_test_new_checks(tmp)
    self_test_motion(tmp)
    self_test_dead_time_boundary()
    self_test_raster(tmp)
    self_test_gate(tmp)


def cut_flash_test_fixture():
    scenes = [{"id": "s01", "start_sec": 0.0, "duration_sec": 0.4}]
    motion = types.SimpleNamespace(frames=[bytes([80])] * 4, fps=10.0)
    return scenes, motion


def assert_equal_cut_luma(scenes, motion):
    _, errors = check_cut_flash(types.SimpleNamespace(motion=motion, scenes=scenes))
    assert not errors, f"equal cut luma must pass: {errors}"


def assert_dark_cut_frame(scenes, motion):
    motion.frames[-1] = bytes([0])
    _, errors = check_cut_flash(types.SimpleNamespace(motion=motion, scenes=scenes))
    assert len(errors) == 1 and "frames 2 and 3" in errors[0][0], f"dark cut frame must fail: {errors}"


def assert_fade_hold(scenes, motion):
    scenes[0].update({"static_hold": True, "static_hold_reason": "Fade to the end card."})
    _, errors = check_cut_flash(types.SimpleNamespace(motion=motion, scenes=scenes))
    assert not errors, f"declared fade hold must allow a luma change: {errors}"


def self_test_cut_flash():
    scenes, motion = cut_flash_test_fixture()
    assert_equal_cut_luma(scenes, motion)
    assert_dark_cut_frame(scenes, motion)
    assert_fade_hold(scenes, motion)
    print("ok cut-flash: equal luma passes, dark last frame fails")


def gate_result_test_context(tmp):
    gate_root = tmp / "gate-results"
    gate_root.mkdir()
    master, mix = gate_root / "master.mp4", gate_root / "mix.wav"
    master.write_bytes(b"video")
    mix.write_bytes(b"audio")
    expected = gate_root / "audio-check.json"
    gate_ctx = types.SimpleNamespace(expected_gate_results=[expected], video=master, mix=mix)
    return gate_ctx, expected, master


def assert_missing_gate_result(gate_ctx):
    _, errors = check_gate_results(gate_ctx)
    assert len(errors) == 1 and "missing" in errors[0][0], f"missing gate result must fail: {errors}"


def assert_stale_gate_result(gate_ctx, expected, master):
    expected.write_text(json.dumps({"passed": True}))
    os.utime(expected, (master.stat().st_mtime - 2, master.stat().st_mtime - 2))
    _, errors = check_gate_results(gate_ctx)
    assert len(errors) == 1 and "older" in errors[0][0], f"stale gate result must fail: {errors}"


def assert_fieldless_gate_result(gate_ctx, expected):
    expected.write_text(json.dumps({"status": "ok"}))
    _, errors = check_gate_results(gate_ctx)
    assert len(errors) == 1 and "no pass" in errors[0][0], f"missing pass field must fail: {errors}"


def self_test_gate_results(tmp):
    gate_ctx, expected, master = gate_result_test_context(tmp)
    assert_missing_gate_result(gate_ctx)
    assert_stale_gate_result(gate_ctx, expected, master)
    assert_fieldless_gate_result(gate_ctx, expected)
    print("ok gate-results: missing, stale, and fieldless results fail")


def format_clip(path, seconds):
    """A tiny still clip: 64x36 at 5 fps, so a 20 s clip encodes in well under 1 s."""
    sh("-f", "lavfi", "-i", f"color=c=0x203040:s=64x36:r=5:d={seconds}", "-c:v", "libx264",
       "-preset", "ultrafast", "-pix_fmt", "yuv420p", str(path))
    return ffprobe(path)


def format_fit_state(fmt, info):
    meta = {} if fmt is None else {"format": fmt}
    return check_state(*check_format_fit(types.SimpleNamespace(meta=meta, info=info)))


def fake_probe(seconds):
    """ffprobe output of a master of this length; a real 300 s clip would slow the self-test."""
    return {"streams": [{"codec_type": "video", "duration": str(seconds)}], "format": {"duration": str(seconds)}}


def self_test_format_fit(tmp):
    short_pass, short_fail = format_clip(tmp / "format-20s.mp4", 20), format_clip(tmp / "format-10s.mp4", 10)
    # The long cases fake the master length with fake_probe instead of encoding 3 min or more.
    cases = [("short 20 s", "short", short_pass, "pass"), ("short 10 s", "short", short_fail, "fail"),
             ("long 300 s", "long", fake_probe(300.0), "pass"), ("long 74 s", "long", fake_probe(74.0), "fail"),
             ("no format", None, short_pass, "fail"), ("bad format", "medium", short_pass, "fail")]
    for name, fmt, info, want in cases:
        got = format_fit_state(fmt, info)
        assert got == want, f"format_fit {name}: state {got}, expected {want}"
    print("ok format-fit: short 20 s and long 300 s pass; short 10 s, long 74 s, no format, bad format fail")


def self_test_new_checks(tmp):
    self_test_cut_flash()
    self_test_gate_results(tmp)
    self_test_format_fit(tmp)


def self_test():
    require_tools()
    self_test_static()
    with tempfile.TemporaryDirectory(prefix="qa-video-") as tmp_name:
        self_test_in(Path(tmp_name))
    print("self-test passed")
    return 0


MOTION_CHECKS = ("frame_pops", "cuts_vs_plan", "dead_time", "flat_opening", "motion_contrast", "loop_seam")
AMP_BUSY_THEN_CALM = "if(lt(t,5),50,if(lt(t,6),50-38*(t-5),12))"
AMP_CALM_THEN_BUSY = "if(lt(t,5),12,if(lt(t,6),12+38*(t-5),50))"


def motion_clip(path, amp, cut_at=None, flash_frame=None, freeze=None):
    """8 s, 160x90, 30 fps: a white box swings left and right with amplitude amp(t), 1 cycle per second.

    cut_at swaps the background colour (a hard cut), flash_frame paints 1 frame white, freeze=(a, b)
    holds the box still from a to b s (both whole or half seconds, so the box does not jump).
    """
    phase = f"if(between(t,{freeze[0]},{freeze[1]}),{freeze[0]},t)" if freeze else "t"
    x = f"60+({amp})*sin(2*PI*({phase}))".replace(",", "\\,")
    bg = "color=c=0x203040:s=160x90:r=30:d=8"
    if cut_at is not None:
        bg += f",drawbox=x=0:y=0:w=iw:h=ih:color=0x606878:t=fill:enable='gte(t,{cut_at})'"
    graph = f"{bg}[bg];color=c=white:s=40x40:r=30:d=8[box];[bg][box]overlay=x='{x}':y=25"
    if flash_frame is not None:
        graph += f",drawbox=x=0:y=0:w=iw:h=ih:color=white:t=fill:enable='eq(n,{flash_frame})'"
    sh("-f", "lavfi", "-i", graph + "[out0]", "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p",
       str(path))


def motion_failures(path, loop=False, fmt="short"):
    meta = {"fps": 30, "width": 160, "height": 90, "duration_sec": 8.0, "loop": loop, "format": fmt}
    scenes = [{"id": "s01", "start_sec": 0.0, "duration_sec": 4.0, "transition_out": "cut"},
              {"id": "s02", "start_sec": 4.0, "duration_sec": 4.0}]
    ctx = types.SimpleNamespace(meta=meta, scenes=scenes, motion=motion_frames(path, meta))
    results = {name: CHECK_FUNCS[name](ctx) for name in MOTION_CHECKS}
    return {name for name, (_, errors) in results.items() if errors}, results


def self_test_dead_time_boundary():
    motion = types.SimpleNamespace(diffs=[0.0] * 61 + [0.1], fps=30.0)
    scene = {"id": "s01", "start_sec": 0.0, "duration_sec": 2.1}
    ctx = types.SimpleNamespace(motion=motion, scenes=[scene])
    _, errors = check_dead_time(ctx)
    assert errors, "dead time of exactly 2.0 s must fail"
    scene["static_hold"] = True
    _, errors = check_dead_time(ctx)
    assert not errors, "static_hold must allow dead time of exactly 2.0 s"
    print("ok dead-time boundary: fail at 2.0 s, pass with static_hold")


def self_test_motion(tmp):
    cases = [
        ("clean", {"amp": AMP_BUSY_THEN_CALM, "cut_at": 4.0}, False, set()),
        ("flash", {"amp": AMP_BUSY_THEN_CALM, "cut_at": 4.0, "flash_frame": 45}, False, {"frame_pops"}),
        ("freeze-3s", {"amp": AMP_BUSY_THEN_CALM, "cut_at": 4.0, "freeze": (0.5, 3.5)}, False, {"dead_time"}),
        ("unplanned-cut", {"amp": AMP_BUSY_THEN_CALM, "cut_at": 2.0}, False, {"cuts_vs_plan"}),
        ("flat-opening", {"amp": AMP_CALM_THEN_BUSY, "cut_at": 4.0}, False, {"flat_opening"}),
        ("flat-motion", {"amp": "50"}, False, {"motion_contrast"}),
        ("loop-clean", {"amp": AMP_BUSY_THEN_CALM}, True, set()),
        ("loop-seam", {"amp": AMP_BUSY_THEN_CALM, "cut_at": 4.0}, True, {"loop_seam"}),
        ("loop-seam-long", {"amp": AMP_BUSY_THEN_CALM, "cut_at": 4.0}, True, set()),  # loop_seam is short only
    ]
    for name, clip, loop, expected in cases:
        path = tmp / f"motion-{name}.mp4"
        motion_clip(path, **clip)
        failed, results = motion_failures(path, loop, "long" if name.endswith("-long") else "short")
        assert failed == expected, f"motion {name}: failed {sorted(failed)}, expected {sorted(expected)}\n" \
                                   f"{json.dumps({k: v[1] for k, v in results.items()}, indent=1)}"
        print(f"ok motion {name}: failed={sorted(failed)}")


def raster_result(tmp, name, samples):  # samples may set "sampling": "nearest"
    path = tmp / f"raster-{name}.json"
    path.write_text(json.dumps({"step_sec": 0.1, "samples": samples}))
    ctx = types.SimpleNamespace(raster_sizes=path, warnings=[])
    _, errors = check_raster_upscale(ctx)
    return errors, ctx.warnings


def self_test_raster(tmp):
    def series(scale, start, count):
        return [{"t_sec": round(start + 0.1 * i, 3), "src": "materials/shot.png", "natural_px": [800, 600],
                 "drawn_px": [800 * scale, 600 * scale]} for i in range(count)]
    errors, warnings = raster_result(tmp, "clean", series(1.0, 0.0, 10) + series(1.12, 1.0, 10))
    assert not errors and not warnings, f"raster clean: {errors} {warnings}"
    errors, warnings = raster_result(tmp, "brief-warn", series(1.0, 0.0, 10) + series(1.15, 1.0, 4))
    assert not errors and not warnings, f"raster 0.4 s at 1.15x must not warn: {errors} {warnings}"
    errors, warnings = raster_result(tmp, "warn", series(1.0, 0.0, 10) + series(1.15, 1.0, 6))
    assert not errors and len(warnings) == 1 and warnings[0][2] == 1.0, f"raster warn: {errors} {warnings}"
    errors, _ = raster_result(tmp, "fail", series(1.0, 0.0, 10) + series(1.3, 1.0, 1))
    assert len(errors) == 1 and "1.30x" in errors[0][0], f"raster 1.3x must fail: {errors}"
    try:
        raster_result(tmp, "bad", [{"t_sec": 0, "src": "a.png", "natural_px": [0, 10], "drawn_px": [5, 5]}])
        raise AssertionError("raster report with a zero natural size must be an input error")
    except InputError:
        pass
    errors, _ = raster_result(tmp, "empty", [])
    assert len(errors) == 1 and "0 samples" in errors[0][0], f"0 raster samples must fail: {errors}"
    pixel = {"t_sec": 0.0, "src": "sprites/hero.png", "natural_px": [16, 24], "drawn_px": [48, 72]}
    errors, _ = raster_result(tmp, "pixel-art", [{**pixel, "sampling": "nearest"}])
    assert not errors, f"3x nearest-neighbour pixel art must pass: {errors}"
    errors, _ = raster_result(tmp, "smooth-3x", [pixel])
    assert len(errors) == 1, f"3x smoothed draw must fail: {errors}"
    errors, _ = raster_result(tmp, "nearest-2.5x", [{**pixel, "drawn_px": [40, 60], "sampling": "nearest"}])
    assert len(errors) == 1, f"2.5x nearest draw is not a whole-number scale and must fail: {errors}"
    errors, _ = raster_result(tmp, "detail-error", [{**pixel, "drawn_px": [16, 24], "detail_error": "canvas is tainted"}])
    assert len(errors) == 1 and "canvas is tainted" in errors[0][0], f"detail_error must fail: {errors}"
    print("ok raster: clean, brief, warn, fail at 1.3x, bad input, 0 samples fail, 3x nearest passes, "
          "detail_error fails")


def check_captions_text(text):
    with tempfile.TemporaryDirectory(prefix="qa-captions-") as tmp:
        path = Path(tmp) / "captions.srt"
        path.write_text(text)
        return check_captions(path, False, 4.0)


def add_input_args(parser):
    parser.add_argument("--self-test", action="store_true", help="run the built-in tests and exit")
    parser.add_argument("--video", help="render/master.mp4")
    parser.add_argument("--timeline", help="timeline.json")
    parser.add_argument("--scenes-dir", help="scene sources (default: <timeline dir>/scenes)")
    parser.add_argument("--render-dir", help="aspect variants and captions (default: video dir)")


def add_output_args(parser):
    parser.add_argument("--out", help="report path (default: <timeline dir>/qa/render-check.json)")
    parser.add_argument("--contact-sheet", help="default: <timeline dir>/qa/contact-sheet-360p.png")
    parser.add_argument("--hook-strip", help="default: <timeline dir>/qa/hook-strip.png")
    parser.add_argument("--phone-sheet", help="default: <timeline dir>/qa/phone-sheet-360w.png")
    parser.add_argument("--phone-frames", help="write hero and key-action PNGs at 360 px high")


def add_gate_args(parser):
    parser.add_argument("--raster-sizes", help="image natural vs drawn sizes (default: <render dir>/raster-sizes.json)")
    parser.add_argument("--expect", action="append", metavar="GATE-RESULT",
                        help="expected result JSON relative to the project; repeat to override defaults")
    parser.add_argument("--probe-forward", help="cold-forward probe PNG frames (0, K, 2K)")
    parser.add_argument("--probe-reverse", help="warm-reverse probe PNG frames, same file names")


def make_parser():
    parser = argparse.ArgumentParser(description="Numeric render QA gate and inspection sheets.")
    add_input_args(parser)
    add_output_args(parser)
    add_gate_args(parser)
    return parser


def print_finding(prefix, finding):
    where = f"[{finding['timestamp']}] " if "timestamp" in finding else ""
    print(f"{prefix} {finding['check']}: {where}{finding['message']}")


def headline(report, out):
    n = report["counts"]
    return f"{'PASS' if report['passed'] else 'FAIL'}: {n['pass']} pass / {n['skip']} skip / {n['fail']} fail; report {out}"


def print_report(report, out):
    [print_finding("WARN", finding) for finding in report["warnings"]]
    [print(f"SKIP {name}: {c['reason']}") for name, c in report["checks"].items() if c["state"] == "skip"]
    [print_finding("FAIL", finding) for finding in report["errors"]]
    print(headline(report, out))


def report_path(args):
    """The --out path, or None when the command writes no report."""
    no_report = args.self_test or not args.video or not args.timeline
    return None if no_report else path_or(args.out, Path(args.timeline).resolve().parent / "qa" / "render-check.json")


def remove_old_report(out):
    """An old report must never read as the result of this run."""
    return out is not None and out.unlink(missing_ok=True)


def run_command(args, parser):
    if args.self_test:
        return self_test()
    if not args.video or not args.timeline:
        parser.error("--video and --timeline are required")
    require_tools()
    report, out = run_gate(args)
    print_report(report, out)
    return 0 if report["passed"] else 1


def write_input_error(exc, out):
    error = {"check": "input", "message": str(exc)}
    write_report(out, {"passed": False, "state": "fail", "input_error": str(exc), "checks": {},
                       "errors": [error], "warnings": []})
    print(f"FAIL: input error; report {out}", file=sys.stderr)


def report_input_error(exc, out):
    print(f"qa_video: {exc}", file=sys.stderr)
    _ = out is not None and write_input_error(exc, out)
    return 2


def run_safely(args, parser):
    out = report_path(args)
    remove_old_report(out)
    caught = Caught(InputError)
    with caught:
        return run_command(args, parser)
    return report_input_error(caught.error, out)


def main(argv=None):
    parser = make_parser()
    return run_safely(parser.parse_args(argv), parser)


if __name__ == "__main__":
    sys.exit(main())
