"""Decode media to float32 at 48 kHz through ffmpeg, with a decode cache.

Also: probe, windows, media path expansion, path checks, content ids, a quiet
atomic replace and the dB helper.
"""

from __future__ import annotations

import functools
import glob
import hashlib
import json
import operator
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
from hark.core.safe import attempt, outcome

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


def _block(fh, offset: int) -> bytes:
    fh.seek(offset)
    return fh.read(_BLOCK)


def _hash_blocks(path: str, size: int) -> str:
    offsets = _sample_offsets(size)
    with open(path, "rb") as fh:
        data = b"".join(_block(fh, offset) for offset in offsets)
    return hashlib.sha1(data).hexdigest()[:12]


def _sampled_hash(path: str, size: int) -> str:
    """SHA-1 of 16 sampled 4 KiB blocks, 12 hex; `unreadable` when the file cannot be read."""
    return attempt(_hash_blocks, path, size, errors=(OSError,), default="unreadable")


def _size_mtime(full: str) -> tuple[int, int]:
    st = attempt(os.stat, full, errors=(OSError,))
    return (st.st_size, st.st_mtime_ns) if st is not None else (-1, -1)


def _state(path: str) -> str:
    full = os.path.abspath(path)
    size, mtime = _size_mtime(full)
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


def _rename_once(tmp: str, dst: str, attempt_no: int, tries: int) -> bool:
    """One atomic rename; after a failure that is not the last, sleep 0.05 * (attempt + 1) s."""
    _, error = outcome(os.replace, tmp, dst, errors=(OSError,))
    if error is not None and attempt_no + 1 < tries:
        time.sleep(0.05 * (attempt_no + 1))
    return error is None


def replace_quietly(tmp: str, dst: str, tries: int = 5) -> bool:
    """Rename tmp over dst with retries. Never raises; False when all tries fail."""
    done = any(_rename_once(tmp, dst, i, tries) for i in range(max(1, tries)))
    if not done:
        attempt(os.remove, tmp, errors=(OSError,))
    return done


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


def _reason_text(line: str) -> str:
    return _PREFIX.sub("", line.strip()).strip()


def ffmpeg_reason(stderr: str) -> str:
    """The last meaningful ffmpeg stderr line, cut to 200 characters."""
    lines = (_reason_text(line) for line in stderr.splitlines())
    kept = [text for text in lines if text and not text.lower().startswith(_DROP_STARTS)]
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


def _shape(data: np.ndarray, mono: bool, duplicate: bool) -> np.ndarray:
    """(n,) for mono; (n, 2) for stereo, a mono source duplicated at unity gain."""
    if mono:
        return data
    pairs = data[: data.size - data.size % 2].reshape(-1, 2)
    return np.column_stack([data, data]) if duplicate else pairs


def _decode(path: str, sr: int, mono: bool) -> np.ndarray:
    duplicate = not mono and _source_channels(path) == 1
    data, proc = _ffmpeg_decode(path, sr, 1 if (mono or duplicate) else 2)
    if proc.returncode != 0 or data.size == 0:
        raise _decode_error(path, proc)
    return _shape(data, mono, duplicate)


# ---------------------------------------------------------------- cache


def _cache_file(path: str, sr: int, mono: bool) -> Path:
    return Path(config.DEC_CACHE) / f"{_decode_key(path, sr, 1 if mono else 2)}.npy"


def _read_cache(target: Path) -> np.ndarray | None:
    """The cached decode, or None when absent or unreadable."""
    reader = functools.partial(np.load, allow_pickle=False)
    return attempt(reader, str(target), errors=(OSError, ValueError)) if target.is_file() else None


def _save(tmp: Path, x: np.ndarray) -> None:
    config.ensure_dirs()
    np.save(str(tmp), x)


def _write_cache(target: Path, x: np.ndarray) -> None:
    """Save next to the target with the process id, then move it into place; failures only lose the entry."""
    tmp = target.with_name(f"{target.stem}.{os.getpid()}.tmp.npy")
    saved = outcome(_save, tmp, x, errors=(OSError,))[1] is None
    moved = saved and replace_quietly(str(tmp), str(target))
    if not moved:
        attempt(os.remove, str(tmp), errors=(OSError,))


def _bad_frames(x: np.ndarray) -> np.ndarray:
    bad = ~np.isfinite(x)
    return bad.any(axis=1) if bad.ndim == 2 else bad


def _finite_message(path: str, count: int, first_s: float) -> str:
    return (f"{path}: {count} NaN / Inf sample(s), first at {first_s:.3f} s: "
            "the file is corrupt, or the code that made it divided by zero")


def _check_finite(path: str, x: np.ndarray, sr: int) -> None:
    bad = _bad_frames(x)
    count = int(bad.sum())
    if count:
        raise AudioError(_finite_message(path, count, int(np.argmax(bad)) / sr))


def _cut(x: np.ndarray, sr: int, start: float | None, end: float | None) -> np.ndarray:
    if start is None and end is None:
        return x
    n = len(x)
    i0 = max(0, int(round((start or 0.0) * sr)))
    i1 = n if end is None else min(n, int(round(end * sr)))
    return x[i0:i1]


def _cached_or_decoded(path: str, sr: int, mono: bool, cache: bool) -> np.ndarray:
    target = _cache_file(path, sr, mono) if cache else None
    x = _read_cache(target) if target is not None else None
    if x is not None:
        return x
    x = _decode(path, sr, mono)
    if target is not None:
        _write_cache(target, x)
    return x


def _checked(path: str, sr: int, mono: bool, cache: bool) -> np.ndarray:
    x = _cached_or_decoded(path, sr, mono, cache)
    _check_finite(path, x, sr)
    return x


def _existing(path: str) -> str:
    path = str(path)
    if not os.path.exists(path):
        raise AudioError(f"no such file: {path}")
    return path


def load(path: str, sr: int = 48000, mono: bool = False, start: float | None = None,
         end: float | None = None, cache: bool = True) -> np.ndarray:
    """Decode a media file to float32: (n, 2) stereo or (n,) mono."""
    path = _existing(path)
    return _cut(_checked(path, sr, mono, cache), sr, start, end)


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
    frac = attempt(Fraction, rate or "0/0", errors=(ValueError, ZeroDivisionError))
    return None if frac is None else round(float(frac), 3)


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


def _streams(raw: dict) -> tuple[dict, list[dict]]:
    """The first audio stream ({} when none) and every video stream."""
    streams = raw.get("streams") or []
    audio = next((s for s in streams if s.get("codec_type") == "audio"), {})
    return audio, [s for s in streams if s.get("codec_type") == "video"]


def _audio_fields(audio: dict, fmt: dict, has_picture: bool) -> dict:
    codec = audio.get("codec_name")
    return {"codec": codec, "src_rate": int(audio.get("sample_rate") or 0),
            "channels": int(audio.get("channels") or 0), "kbps": _kbps(audio, fmt, has_picture),
            "lossy": codec in LOSSY_CODECS}


def _probe_fields(raw: dict) -> dict:
    fmt = raw.get("format") or {}
    audio, videos = _streams(raw)
    moving = [s for s in videos if not _is_picture(s)]
    out = _audio_fields(audio, fmt, bool(videos))
    out.update(duration=round(float(fmt.get("duration") or 0.0), 3), has_video=bool(moving))
    out.update({"video": _video_info(moving[0])} if moving else {})
    return out


def _probe_once(path: str) -> dict:
    return _probe_fields(_run_ffprobe(path))


def probe(path: str) -> dict:
    """ffprobe facts of the first audio stream and the container."""
    info, error = outcome(_probe_once, str(path), errors=(OSError, subprocess.SubprocessError, ValueError, AudioError))
    return info if error is None else {"probe_error": str(error)[:_REASON_MAX]}


# ---------------------------------------------------------------- windows and paths


def _starts(n: int, w: int, h: int, pad_last: bool) -> list[int]:
    starts = list(range(0, n - w + 1, h))
    tail = [n - w] if pad_last and starts[-1] + w < n else []
    return starts + tail


def windows(x: np.ndarray, win: float, hop: float, sr: int = 48000,
            pad_last: bool = False) -> list[tuple[float, np.ndarray]]:
    """Cut x into (start_seconds, segment) windows."""
    w = int(round(win * sr))
    if len(x) <= w:
        return [(0.0, x)]
    starts = _starts(len(x), w, max(1, int(round(hop * sr))), pad_last)
    return [(s / sr, x[s: s + w]) for s in starts]


def _scan(folder: str) -> list[os.DirEntry]:
    return list(os.scandir(folder))


def _entries(folder: str) -> list[os.DirEntry]:
    found = attempt(_scan, folder, errors=(OSError,), default=[])
    return sorted(found, key=operator.attrgetter("name"))


def _walk(folder: str, recursive: bool, exts: frozenset[str]) -> list[str]:
    """Media files of a folder in sorted order, its own files before its sub-folders."""
    entries = _entries(folder)
    files = [e.path for e in entries if e.is_file() and Path(e.name).suffix.lower() in exts]
    subs = [e.path for e in entries if e.is_dir(follow_symlinks=False)] if recursive else []
    return files + [path for sub in subs for path in _walk(sub, True, exts)]


def _glob_media(pattern: str) -> list[str]:
    hits = sorted(glob.glob(pattern, recursive=True))
    return [h for h in hits if os.path.isfile(h) and Path(h).suffix.lower() in MEDIA_EXTS]


def _expand_one(path: str, recursive: bool, exts: frozenset[str]) -> list[str]:
    if os.path.isfile(path):
        return [path]
    folder = os.path.isdir(path)
    return _walk(path, recursive, exts) if folder else _glob_media(path)


def _norm_key(path: str) -> str:
    return os.path.normcase(os.path.abspath(path)).casefold()


def _as_list(paths: str | os.PathLike | Sequence[str]) -> list:
    return [paths] if isinstance(paths, (str, os.PathLike)) else list(paths)


def _first_per_key(found: list[str]) -> list[str]:
    unique: dict[str, str] = {}
    for path in found:
        unique.setdefault(_norm_key(path), path)
    return list(unique.values())


def list_media(paths: str | Sequence[str], recursive: bool = True,
               exts: Iterable[str] = MEDIA_EXTS) -> list[str]:
    """Expand files, folders and globs to a list of media files without duplicates."""
    wanted = frozenset(exts)
    found = [f for item in _as_list(paths) for f in _expand_one(str(item), recursive, wanted)]
    return _first_per_key(found)


def _missing_part(item: str) -> str | None:
    """The check_paths part of an input that yields no media file, else None."""
    if list_media([item]):
        return None
    exists = os.path.exists(item) or bool(glob.glob(item, recursive=True))
    return f"{item} (no audio or video files in it)" if exists else f"{item} (no such file or folder)"


def check_paths(paths: str | Sequence[str], what: str = "paths") -> None:
    """Raise AudioError naming every given path that yields no media file."""
    parts = [part for part in map(_missing_part, map(str, _as_list(paths))) if part]
    if not parts:
        return
    more = " ..." if len(parts) > _PARTS_LISTED else ""
    raise AudioError(f"{what}: nothing playable in " + ", ".join(parts[:_PARTS_LISTED]) + more)
