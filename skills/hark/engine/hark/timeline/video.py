"""Picture-side helpers built on ffmpeg.

Covers stream facts, a per-frame change signal, change peaks, cut times and
single-frame grabs. Results that cost a decode are kept under ``$HARK_HOME/video``.
"""

from __future__ import annotations

import hashlib
import io
import json
import os
import re
import subprocess

import numpy as np
from scipy import ndimage

from hark.core.audio import AudioError, file_id, probe

# Size of the grey thumbnail that the change signal is computed on.
_THUMB_W = 96
_THUMB_H = 54
_THUMB_PIXELS = _THUMB_W * _THUMB_H

# Bump a version when its cached result format or meaning changes.
DIFF_VERSION = 1
SCENE_VERSION = 1

_FALLBACK_RATE = 30.0
_PTS_RE = re.compile(r"pts_time:\s*([0-9.]+)")

__all__ = ["info", "has_video", "frame_diff", "diff_events", "cuts", "frame", "frames"]


# ---------------------------------------------------------------- cache plumbing

def _store_root() -> str:
    explicit = os.environ.get("HARK_HOME")
    if explicit:
        return explicit
    from hark.core import config

    return str(config.HOME)


def _key(*parts) -> str:
    joined = "|".join(map(str, parts))
    return hashlib.sha1(joined.encode()).hexdigest()[:24]


def _entry(prefix: str, suffix: str, *parts) -> str:
    return os.path.join(_store_root(), "video", f"{prefix}_{_key(*parts)}.{suffix}")


def _save(target: str, dump) -> None:
    """Write via a temp file then rename; a failed write only loses the cache."""
    scratch = f"{target}.{os.getpid()}.tmp"
    try:
        os.makedirs(os.path.dirname(target), exist_ok=True)
        dump(scratch)
        os.replace(scratch, target)
    except OSError:
        return


_SILENT = ("-an",)


def _ffmpeg(source: str, chain: str, sink: tuple, *, level: str = "error", before=(),
            after=_SILENT) -> subprocess.CompletedProcess:
    """One ffmpeg pass: options ``before`` the input, ``after`` it, then the filter chain and the sink."""
    argv = ["ffmpeg", "-nostdin", "-v", level, *before, "-i", source, *after, "-vf", chain, *sink]
    return subprocess.run(argv, capture_output=True, check=False)


_RAW_GREY = ("-f", "rawvideo", "-pix_fmt", "gray", "pipe:1")
_DISCARD = ("-f", "null", "-")
_PNG_PIPE = ("-f", "image2pipe", "-vcodec", "png", "pipe:1")


def _broken(what: str, path: str, why) -> AudioError:
    return AudioError(f"cannot {what} {path}: {why}")


# ---------------------------------------------------------------- stream facts

def info(path: str) -> dict:
    """Duration, rate, size and which streams exist, from the core probe."""
    meta = probe(path)
    pic = meta.get("video") or {}
    out = {"duration": meta.get("duration")}
    for field in ("fps", "width", "height"):
        out[field] = pic.get(field)
    out["has_audio"] = bool(meta.get("codec"))
    out["has_video"] = bool(meta.get("has_video"))
    return out


def has_video(path: str) -> bool:
    return bool(probe(path).get("has_video"))


# ---------------------------------------------------------------- change signal

def _grey_bytes(path: str) -> bytes:
    shrink = f"scale={_THUMB_W}:{_THUMB_H}:flags=area,format=gray"
    try:
        proc = _ffmpeg(path, shrink, _RAW_GREY)
    except OSError as exc:
        raise _broken("read the picture of", path, exc) from None
    if proc.returncode == 0 and proc.stdout:
        return proc.stdout
    why = proc.stderr.decode("utf-8", "replace").strip()[:200]
    raise _broken("read the picture of", path, why or "no frames")


def _change_per_frame(raw: bytes) -> np.ndarray:
    count = len(raw) // _THUMB_PIXELS
    pix = np.frombuffer(raw, dtype=np.uint8, count=count * _THUMB_PIXELS)
    stack = pix.reshape(count, _THUMB_PIXELS).astype(np.float32)
    signal = np.zeros(count)
    if count >= 2:
        step = np.abs(stack[1:] - stack[:-1])
        signal[1:] = step.mean(axis=1) / 255.0
    return signal


def frame_diff(path: str, cache: bool = True) -> tuple[float, np.ndarray]:
    """Return (fps, per-frame mean grey change in 0..1); frame 0 is always 0."""
    rate = (probe(path).get("video") or {}).get("fps") or _FALLBACK_RATE
    where = _entry("diff", "npz", file_id(path), _THUMB_W, _THUMB_H, DIFF_VERSION)
    if cache and os.path.isfile(where):
        try:
            with np.load(where) as stored:
                return float(stored["fps"]), np.asarray(stored["d"])
        except (OSError, ValueError, KeyError):
            pass
    signal = _change_per_frame(_grey_bytes(path))

    def dump(tmp: str) -> None:
        with open(tmp, "wb") as fh:
            np.savez(fh, fps=rate, d=signal)

    _save(where, dump)
    return float(rate), signal


def _merge_close(peaks, signal: np.ndarray, spacing: float) -> list[int]:
    """Collapse peaks nearer than ``spacing`` frames into the strongest one."""
    picked: list[int] = []
    for idx in peaks:
        if picked and idx - picked[-1] < spacing:
            if signal[idx] > signal[picked[-1]]:
                picked[-1] = idx
        else:
            picked.append(idx)
    return picked


def diff_events(d, fps: float, ratio: float = 4.0, floor: float = 0.012, big: float = 0.15,
                gap: float = 0.12) -> list[int]:
    """Frame numbers where the change signal spikes against its local median."""
    signal = np.asarray(d, dtype=float)
    if signal.size < 3:
        return []
    window = int(round(fps)) | 1
    baseline = ndimage.median_filter(signal, size=window, mode="nearest")
    is_top = signal == ndimage.maximum_filter1d(signal, size=3, mode="nearest")
    relative = (signal > floor) & (signal > ratio * baseline + 0.004)
    hits = np.flatnonzero(is_top & (relative | (signal >= big)))
    return _merge_close([int(h) for h in hits if h > 0], signal, gap * fps)


# ---------------------------------------------------------------- cuts

def _scan_scenes(path: str, threshold: float, min_gap: float) -> list[float]:
    chain = f"scale=320:-2,select='gt(scene,{threshold})',showinfo"
    try:
        proc = _ffmpeg(path, chain, _DISCARD, level="info")
    except OSError as exc:
        raise _broken("scan scenes of", path, exc) from None
    log = proc.stderr.decode("utf-8", "replace")
    if proc.returncode != 0:
        tail = [row for row in log.splitlines() if row.strip()]
        last = tail[-1] if tail else "ffmpeg failed"
        raise _broken("scan scenes of", path, last[:200])
    found: list[float] = []
    for hit in _PTS_RE.finditer(log):
        when = round(float(hit.group(1)), 3)
        if not found or when - found[-1] >= min_gap:
            found.append(when)
    return found


def _cached_scenes(path: str, threshold: float, min_gap: float, cache: bool) -> list[float]:
    where = _entry("scene", "json", file_id(path), threshold, min_gap, SCENE_VERSION)
    if cache and os.path.isfile(where):
        try:
            with open(where, encoding="utf-8") as fh:
                return [float(v) for v in json.load(fh)]
        except (OSError, ValueError, TypeError):
            pass
    found = _scan_scenes(path, threshold, min_gap)

    def dump(tmp: str) -> None:
        with open(tmp, "w", encoding="utf-8") as fh:
            json.dump(found, fh)

    _save(where, dump)
    return found


def cuts(path: str, method: str = "diff", threshold: float = 0.3, min_gap: float = 0.2,
         cache: bool = True) -> list[float]:
    """Seconds where the picture cuts or changes a lot."""
    if method == "scene":
        return _cached_scenes(path, threshold, min_gap, cache)
    rate, signal = frame_diff(path, cache=cache)
    return [round(n / rate, 3) for n in diff_events(signal, rate)]


# ---------------------------------------------------------------- stills

def frame(path: str, t: float, width: int = 320):
    """An RGB PIL image of the picture near ``t`` seconds; None on any failure."""
    from PIL import Image

    seek = f"{max(0.0, t):.3f}"
    try:
        proc = _ffmpeg(path, f"scale={int(width)}:-2", _PNG_PIPE, before=("-ss", seek),
                       after=("-frames:v", "1"))
        if proc.returncode != 0 or not proc.stdout:
            return None
        return Image.open(io.BytesIO(proc.stdout)).convert("RGB")
    except (OSError, ValueError):
        return None


def frames(path: str, times, width: int = 320) -> list:
    return [frame(path, when, width) for when in times]
