"""Video helpers on ffmpeg: stream info, frame difference, visual events, cuts and frames (sheet-video part 1)."""

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

W, H = 96, 54
DIFF_VERSION = 1
SCENE_VERSION = 1
DEFAULT_FPS = 30.0

__all__ = ["info", "has_video", "frame_diff", "diff_events", "cuts", "frame", "frames"]


def _home() -> str:
    home = os.environ.get("HARK_HOME")
    if home:
        return home
    from hark.core import config

    return str(config.HOME)


def _cache_dir() -> str:
    return os.path.join(_home(), "video")


def _key(*parts) -> str:
    return hashlib.sha1("|".join(str(p) for p in parts).encode()).hexdigest()[:24]


def _atomic_write(path: str, write) -> None:
    try:
        os.makedirs(os.path.dirname(path), exist_ok=True)
        tmp = f"{path}.{os.getpid()}.tmp"
        write(tmp)
        os.replace(tmp, path)
    except OSError:
        pass


def info(path: str) -> dict:
    """Stream info from the core probe."""
    p = probe(path)
    v = p.get("video") or {}
    return {"duration": p.get("duration"), "fps": v.get("fps"), "width": v.get("width"), "height": v.get("height"),
            "has_audio": bool(p.get("codec")), "has_video": bool(p.get("has_video"))}


def has_video(path: str) -> bool:
    return bool(probe(path).get("has_video"))


# ---------------------------------------------------------------- frame difference

def _decode_grey(path: str) -> bytes:
    cmd = ["ffmpeg", "-nostdin", "-v", "error", "-i", path, "-an", "-vf", f"scale={W}:{H}:flags=area,format=gray",
           "-f", "rawvideo", "-pix_fmt", "gray", "pipe:1"]
    try:
        run = subprocess.run(cmd, capture_output=True, check=False)
    except OSError as exc:
        raise AudioError(f"cannot read the picture of {path}: {exc}") from None
    if run.returncode != 0 or not run.stdout:
        reason = run.stderr.decode("utf-8", "replace").strip()[:200]
        raise AudioError(f"cannot read the picture of {path}: {reason or 'no frames'}")
    return run.stdout


def _diff_of(raw: bytes) -> np.ndarray:
    n = len(raw) // (W * H)
    fr = np.frombuffer(raw[: n * W * H], dtype=np.uint8).reshape(n, W * H).astype(np.float32)
    d = np.zeros(n)
    if n > 1:
        d[1:] = np.abs(np.diff(fr, axis=0)).mean(axis=1) / 255.0
    return d


def frame_diff(path: str, cache: bool = True) -> tuple[float, np.ndarray]:
    """(fps, mean absolute grey difference per frame, 0..1)."""
    fps = (probe(path).get("video") or {}).get("fps") or DEFAULT_FPS
    cpath = os.path.join(_cache_dir(), f"diff_{_key(file_id(path), W, H, DIFF_VERSION)}.npz")
    if cache and os.path.isfile(cpath):
        try:
            with np.load(cpath) as z:
                return float(z["fps"]), np.asarray(z["d"])
        except (OSError, ValueError, KeyError):
            pass
    d = _diff_of(_decode_grey(path))
    def write(tmp):
        with open(tmp, "wb") as fh:
            np.savez(fh, fps=fps, d=d)

    _atomic_write(cpath, write)
    return float(fps), d


def diff_events(d, fps: float, ratio: float = 4.0, floor: float = 0.012, big: float = 0.15,
                gap: float = 0.12) -> list[int]:
    """Frame indices of visual events in a frame difference signal."""
    d = np.asarray(d, dtype=float)
    if d.size < 3:
        return []
    size = int(round(fps)) | 1
    med = ndimage.median_filter(d, size=size, mode="nearest")
    loc = ndimage.maximum_filter1d(d, size=3, mode="nearest")
    cand = np.flatnonzero((d == loc) & (((d > floor) & (d > ratio * med + 0.004)) | (d >= big)))
    kept: list[int] = []
    for i in (int(c) for c in cand if c > 0):
        if kept and i - kept[-1] < gap * fps:
            if d[i] > d[kept[-1]]:
                kept[-1] = i
            continue
        kept.append(i)
    return kept


# ---------------------------------------------------------------- cuts

def _scene_times(path: str, threshold: float, min_gap: float) -> list[float]:
    vf = f"scale=320:-2,select='gt(scene,{threshold})',showinfo"
    cmd = ["ffmpeg", "-nostdin", "-v", "info", "-i", path, "-an", "-vf", vf, "-f", "null", "-"]
    try:
        run = subprocess.run(cmd, capture_output=True, check=False)
    except OSError as exc:
        raise AudioError(f"cannot scan scenes of {path}: {exc}") from None
    err = run.stderr.decode("utf-8", "replace")
    if run.returncode != 0:
        lines = [ln for ln in err.splitlines() if ln.strip()]
        raise AudioError(f"cannot scan scenes of {path}: {(lines[-1] if lines else 'ffmpeg failed')[:200]}")
    kept: list[float] = []
    for m in re.finditer(r"pts_time:\s*([0-9.]+)", err):
        t = round(float(m.group(1)), 3)
        if not kept or t - kept[-1] >= min_gap:
            kept.append(t)
    return kept


def _scene_cuts(path: str, threshold: float, min_gap: float, cache: bool) -> list[float]:
    cpath = os.path.join(_cache_dir(), f"scene_{_key(file_id(path), threshold, min_gap, SCENE_VERSION)}.json")
    if cache and os.path.isfile(cpath):
        try:
            with open(cpath, encoding="utf-8") as fh:
                return [float(t) for t in json.load(fh)]
        except (OSError, ValueError, TypeError):
            pass
    times = _scene_times(path, threshold, min_gap)

    def write(tmp):
        with open(tmp, "w", encoding="utf-8") as fh:
            json.dump(times, fh)

    _atomic_write(cpath, write)
    return times


def cuts(path: str, method: str = "diff", threshold: float = 0.3, min_gap: float = 0.2,
         cache: bool = True) -> list[float]:
    """Picture cuts and big visual changes, in seconds."""
    if method == "scene":
        return _scene_cuts(path, threshold, min_gap, cache)
    fps, d = frame_diff(path, cache=cache)
    return [round(i / fps, 3) for i in diff_events(d, fps)]


# ---------------------------------------------------------------- frames

def frame(path: str, t: float, width: int = 320):
    """One RGB frame near time t as a PIL image, or None."""
    from PIL import Image

    cmd = ["ffmpeg", "-nostdin", "-v", "error", "-ss", f"{max(0.0, t):.3f}", "-i", path, "-frames:v", "1",
           "-vf", f"scale={int(width)}:-2", "-f", "image2pipe", "-vcodec", "png", "pipe:1"]
    try:
        run = subprocess.run(cmd, capture_output=True, check=False)
        if run.returncode != 0 or not run.stdout:
            return None
        return Image.open(io.BytesIO(run.stdout)).convert("RGB")
    except (OSError, ValueError):
        return None


def frames(path: str, times, width: int = 320) -> list:
    return [frame(path, t, width) for t in times]
