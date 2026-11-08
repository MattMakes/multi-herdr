"""Decode media to float32 at 48 kHz through ffmpeg, with a decode cache.

Also: probe, windows, media path expansion, path checks, content ids, a quiet
atomic replace and the dB helper.
"""

from __future__ import annotations

import glob
import hashlib
import json
import os
import re
import shutil
import subprocess
import time
from collections.abc import Iterable, Sequence
from fractions import Fraction
from pathlib import Path

import numpy as np

from hark.core import config

Audio = np.ndarray

SR = 48000
DECODE_VERSION = 2
AUDIO_EXTS = frozenset({".wav", ".flac", ".ogg", ".oga", ".opus", ".mp3", ".m4a", ".aac", ".aif", ".aiff", ".wma"})
VIDEO_EXTS = frozenset({".mp4", ".mov", ".m4v", ".webm", ".mkv", ".avi"})
MEDIA_EXTS = AUDIO_EXTS | VIDEO_EXTS
LOSSY_CODECS = frozenset({"aac", "mp3", "mp2", "opus", "vorbis", "wmav2", "ac3", "eac3"})

_BLOCK = 4096
_BLOCKS = 16
_PROBE_TIMEOUT_S = 60
_REASON_MAX = 200
_KBPS_LIMIT = 10_000_000
_PARTS_LISTED = 5
_PREFIX = re.compile(r"^\[[^\]]* @ (0x)?[0-9a-fA-F]+\]\s*")
_DROP_STARTS = ("error opening output", "conversion failed")


class AudioError(RuntimeError):
    """A decode, probe or path problem. The message is one line."""


# ---------------------------------------------------------------- ids


def _sample_offsets(size: int) -> list[int]:
    span = max(0, size - _BLOCK)
    return sorted({(i * span) // max(1, _BLOCKS - 1) for i in range(_BLOCKS)})


def _sampled_hash(path: str, size: int) -> str:
    try:
        digest = hashlib.sha1()
        with open(path, "rb") as fh:
            for offset in _sample_offsets(size):
                fh.seek(offset)
                digest.update(fh.read(_BLOCK))
        return digest.hexdigest()[:12]
    except OSError:
        return "unreadable"


def _state(path: str) -> str:
    full = os.path.abspath(path)
    try:
        st = os.stat(full)
        size, mtime = st.st_size, st.st_mtime_ns
    except OSError:
        size, mtime = -1, -1
    return f"{full}|{size}|{mtime}|{_sampled_hash(full, max(size, 0))}"


def _sha20(text: str) -> str:
    return hashlib.sha1(text.encode("utf-8")).hexdigest()[:20]


def file_id(path: str) -> str:
    """Stable 20-hex id of a file: path, size, mtime and sampled content."""
    return _sha20(_state(path))


def array_id(x: np.ndarray) -> str:
    """Stable 20-hex id of an array: float32 bytes and shape."""
    arr = np.ascontiguousarray(np.asarray(x, dtype=np.float32))
    digest = hashlib.sha1(arr.tobytes())
    digest.update(str(arr.shape).encode("ascii"))
    return digest.hexdigest()[:20]


def _decode_key(path: str, sr: int, channels: int) -> str:
    return _sha20(f"{_state(path)}|{sr}|{channels}|v{DECODE_VERSION}")


# ---------------------------------------------------------------- small helpers


def db(v: float, floor: float = 1e-12) -> float:
    """20 log10(max(v, floor)); silence gives -240 dB."""
    return float(20.0 * np.log10(max(float(v), floor)))


def replace_quietly(tmp: str, dst: str, tries: int = 5) -> bool:
    """Rename tmp over dst with retries. Never raises; False when all tries fail."""
    for attempt in range(max(1, tries)):
        try:
            os.replace(tmp, dst)
            return True
        except OSError:
            if attempt + 1 < tries:
                time.sleep(0.05 * (attempt + 1))
    try:
        os.remove(tmp)
    except OSError:
        pass
    return False


def to_mono(x: np.ndarray) -> np.ndarray:
    """Channel mean of 2-D input; 1-D input as is."""
    return x.mean(axis=1) if x.ndim == 2 else x


def resample(x: np.ndarray, sr_from: int, sr_to: int) -> np.ndarray:
    """soxr HQ resample to float32; unchanged when the rates are equal."""
    if sr_from == sr_to:
        return x
    import soxr

    return soxr.resample(np.asarray(x, dtype=np.float32), sr_from, sr_to, quality="HQ").astype(np.float32)


def write_wav(path: str, x: np.ndarray, sr: int = 48000, subtype: str = "PCM_24") -> str:
    """Write a WAV file, making parent folders. Returns the path."""
    import soundfile as sf

    Path(path).parent.mkdir(parents=True, exist_ok=True)
    sf.write(str(path), np.asarray(x), sr, subtype=subtype)
    return str(path)


# ---------------------------------------------------------------- ffmpeg


def _tool(name: str) -> str:
    found = shutil.which(name)
    if found is None:
        raise AudioError(f"{name} not found on PATH")
    return found


def ffmpeg_reason(stderr: str) -> str:
    """The last meaningful ffmpeg stderr line, cut to 200 characters."""
    kept = []
    for line in stderr.splitlines():
        text = _PREFIX.sub("", line.strip()).strip()
        if text and not text.lower().startswith(_DROP_STARTS):
            kept.append(text)
    return (kept[-1] if kept else "ffmpeg failed")[:_REASON_MAX]


def _source_channels(path: str) -> int:
    info = probe(path)
    return int(info.get("channels") or 0)


def _ffmpeg_decode(path: str, sr: int, channels: int) -> tuple[np.ndarray, subprocess.CompletedProcess]:
    cmd = [_tool("ffmpeg"), "-nostdin", "-v", "error", "-i", path, "-vn",
           "-f", "f32le", "-acodec", "pcm_f32le", "-ar", str(sr), "-ac", str(channels), "-"]
    proc = subprocess.run(cmd, stdin=subprocess.DEVNULL, capture_output=True)
    data = np.frombuffer(proc.stdout, dtype="<f4").astype(np.float32)
    return data, proc


def _decode_error(path: str, proc: subprocess.CompletedProcess) -> AudioError:
    stderr = proc.stderr.decode("utf-8", "replace")
    info = probe(path)
    if "probe_error" not in info and info.get("has_video") and not info.get("codec"):
        return AudioError(f"{path} has no audio track (a silent render? mux the soundtrack in first)")
    if proc.returncode == 0 or "does not contain any stream" in stderr:
        return AudioError(f"{path}: no audio stream")
    return AudioError(f"cannot decode {path}: {ffmpeg_reason(stderr)}")


def _decode(path: str, sr: int, mono: bool) -> np.ndarray:
    duplicate = not mono and _source_channels(path) == 1
    channels = 1 if (mono or duplicate) else 2
    data, proc = _ffmpeg_decode(path, sr, channels)
    if proc.returncode != 0 or data.size == 0:
        raise _decode_error(path, proc)
    if mono:
        return data
    if duplicate:
        return np.column_stack([data, data])
    return data[: data.size - data.size % 2].reshape(-1, 2)


# ---------------------------------------------------------------- cache


def _cache_file(path: str, sr: int, mono: bool) -> Path:
    return Path(config.DEC_CACHE) / f"{_decode_key(path, sr, 1 if mono else 2)}.npy"


def _read_cache(target: Path) -> np.ndarray | None:
    try:
        return np.load(str(target), allow_pickle=False) if target.is_file() else None
    except (OSError, ValueError):
        return None


def _write_cache(target: Path, x: np.ndarray) -> None:
    tmp = target.with_name(f"{target.stem}.{os.getpid()}.tmp.npy")
    try:
        config.ensure_dirs()
        np.save(str(tmp), x)
    except OSError:
        _remove(tmp)
        return
    replace_quietly(str(tmp), str(target))


def _remove(path: Path) -> None:
    try:
        path.unlink()
    except OSError:
        pass


def _check_finite(path: str, x: np.ndarray, sr: int) -> None:
    bad = ~np.isfinite(x)
    if bad.ndim == 2:
        bad = bad.any(axis=1)
    count = int(bad.sum())
    if count:
        first = int(np.argmax(bad)) / sr
        raise AudioError(f"{path}: {count} NaN / Inf sample(s), first at {first:.3f} s: "
                         "the file is corrupt, or the code that made it divided by zero")


def _cut(x: np.ndarray, sr: int, start: float | None, end: float | None) -> np.ndarray:
    if start is None and end is None:
        return x
    n = len(x)
    i0 = max(0, int(round((start or 0.0) * sr)))
    i1 = n if end is None else min(n, int(round(end * sr)))
    return x[i0:i1]


def load(path: str, sr: int = 48000, mono: bool = False, start: float | None = None,
         end: float | None = None, cache: bool = True) -> np.ndarray:
    """Decode a media file to float32: (n, 2) stereo or (n,) mono."""
    path = str(path)
    if not os.path.exists(path):
        raise AudioError(f"no such file: {path}")
    target = _cache_file(path, sr, mono) if cache else None
    x = _read_cache(target) if target is not None else None
    if x is None:
        x = _decode(path, sr, mono)
        if target is not None:
            _write_cache(target, x)
    _check_finite(path, x, sr)
    return _cut(x, sr, start, end)


def duration(path: str) -> float:
    """Length of the mono decode in seconds."""
    return len(load(path, mono=True)) / SR


# ---------------------------------------------------------------- probe


def _run_ffprobe(path: str) -> dict:
    cmd = [_tool("ffprobe"), "-v", "error", "-print_format", "json",
           "-show_entries",
           "stream=codec_type,codec_name,sample_rate,channels,bit_rate,width,height,r_frame_rate"
           ":stream_disposition=attached_pic:format=duration,bit_rate", path]
    proc = subprocess.run(cmd, stdin=subprocess.DEVNULL, capture_output=True, timeout=_PROBE_TIMEOUT_S)
    text = proc.stdout.decode("utf-8", "replace").strip()
    return json.loads(text) if text else {}


def _is_picture(stream: dict) -> bool:
    return bool((stream.get("disposition") or {}).get("attached_pic"))


def _fps(rate: str | None) -> float | None:
    try:
        frac = Fraction(rate or "0/0")
    except (ValueError, ZeroDivisionError):
        return None
    return round(float(frac), 3)


def _kbps(audio: dict, fmt: dict, has_picture: bool) -> int | None:
    rate = str(audio.get("bit_rate") or "")
    if rate.isdigit() and int(rate) < _KBPS_LIMIT:
        return int(round(int(rate) / 1000))
    whole = str(fmt.get("bit_rate") or "")
    if whole.isdigit() and not has_picture:
        return int(round(int(whole) / 1000))
    return None


def _video_info(stream: dict) -> dict:
    return {"width": stream.get("width"), "height": stream.get("height"), "fps": _fps(stream.get("r_frame_rate"))}


def _probe_fields(raw: dict) -> dict:
    streams = raw.get("streams") or []
    fmt = raw.get("format") or {}
    audio = next((s for s in streams if s.get("codec_type") == "audio"), {})
    videos = [s for s in streams if s.get("codec_type") == "video"]
    moving = [s for s in videos if not _is_picture(s)]
    codec = audio.get("codec_name")
    out = {
        "codec": codec,
        "src_rate": int(audio.get("sample_rate") or 0),
        "channels": int(audio.get("channels") or 0),
        "kbps": _kbps(audio, fmt, bool(videos)),
        "lossy": codec in LOSSY_CODECS,
        "duration": round(float(fmt.get("duration") or 0.0), 3),
        "has_video": bool(moving),
    }
    if moving:
        out["video"] = _video_info(moving[0])
    return out


def probe(path: str) -> dict:
    """ffprobe facts of the first audio stream and the container."""
    try:
        return _probe_fields(_run_ffprobe(str(path)))
    except (OSError, subprocess.SubprocessError, ValueError, AudioError) as exc:
        return {"probe_error": str(exc)[:_REASON_MAX]}


# ---------------------------------------------------------------- windows and paths


def windows(x: np.ndarray, win: float, hop: float, sr: int = 48000,
            pad_last: bool = False) -> list[tuple[float, np.ndarray]]:
    """Cut x into (start_seconds, segment) windows."""
    w, h, n = int(round(win * sr)), max(1, int(round(hop * sr))), len(x)
    if n <= w:
        return [(0.0, x)]
    starts = list(range(0, n - w + 1, h))
    if pad_last and starts[-1] + w < n:
        starts.append(n - w)
    return [(s / sr, x[s: s + w]) for s in starts]


def _walk(folder: str, recursive: bool, exts: Iterable[str]) -> list[str]:
    found = []
    for root, dirs, files in os.walk(folder):
        dirs.sort()
        found += [os.path.join(root, f) for f in sorted(files) if Path(f).suffix.lower() in exts]
        if not recursive:
            break
    return found


def _expand_one(path: str, recursive: bool, exts: Iterable[str]) -> list[str]:
    if os.path.isdir(path):
        return _walk(path, recursive, exts)
    if os.path.isfile(path):
        return [path]
    hits = sorted(glob.glob(path, recursive=True))
    return [h for h in hits if os.path.isfile(h) and Path(h).suffix.lower() in MEDIA_EXTS]


def _norm_key(path: str) -> str:
    return os.path.normcase(os.path.abspath(path)).casefold()


def list_media(paths: str | Sequence[str], recursive: bool = True,
               exts: Iterable[str] = MEDIA_EXTS) -> list[str]:
    """Expand files, folders and globs to a list of media files without duplicates."""
    items = [paths] if isinstance(paths, (str, os.PathLike)) else list(paths)
    seen, out = set(), []
    for item in items:
        for found in _expand_one(str(item), recursive, frozenset(exts)):
            key = _norm_key(found)
            if key not in seen:
                seen.add(key)
                out.append(found)
    return out


def check_paths(paths: str | Sequence[str], what: str = "paths") -> None:
    """Raise AudioError naming every given path that yields no media file."""
    items = [paths] if isinstance(paths, (str, os.PathLike)) else list(paths)
    parts = []
    for item in map(str, items):
        if list_media([item]):
            continue
        exists = os.path.exists(item) or bool(glob.glob(item, recursive=True))
        parts.append(f"{item} (no audio or video files in it)" if exists else f"{item} (no such file or folder)")
    if parts:
        more = " ..." if len(parts) > _PARTS_LISTED else ""
        raise AudioError(f"{what}: nothing playable in " + ", ".join(parts[:_PARTS_LISTED]) + more)
