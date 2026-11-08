"""Media in hark: decode anything ffmpeg reads to float32 at 48 kHz, with a disk cache.

This module holds decoding, the decode cache and a few signal helpers. It also
re-exports the content ids (``_media_id``), ffprobe access (``_media_probe``)
and path expansion and windowing (``_media_paths``), so callers import one name.
"""

from __future__ import annotations

import os
import subprocess
import time
from pathlib import Path

import numpy as np

from hark.core import config
from hark.core._media_id import (  # noqa: F401  (re-exported names)
    DECODE_VERSION,
    _sample_offsets,
    _sampled_hash,
    array_id,
    decode_key,
    file_id,
)
from hark.core._media_paths import (  # noqa: F401  (re-exported names)
    AUDIO_EXTS,
    MEDIA_EXTS,
    VIDEO_EXTS,
    check_paths,
    list_media,
    windows,
)
from hark.core._media_probe import (  # noqa: F401  (re-exported names)
    LOSSY_CODECS,
    AudioError,
    _probe_fields,
    ffmpeg_reason,
    find_tool,
    probe,
)
from hark.core.safe import attempt, outcome

Audio = np.ndarray

SR = 48000


def db(v: float, floor: float = 1e-12) -> float:
    """Level in dB, 20 log10 of v clamped at floor; a zero input gives -240."""
    return float(20.0 * np.log10(max(float(v), floor)))


def replace_quietly(tmp: str, dst: str, tries: int = 5) -> bool:
    """Move tmp onto dst, retrying with a short growing pause; never raises.

    When every try fails, tmp is removed and the result is False.
    """
    rounds = max(1, tries)
    for n in range(rounds):
        if outcome(os.replace, tmp, dst, errors=(OSError,))[1] is None:
            return True
        if n + 1 < tries:
            time.sleep(0.05 * (n + 1))
    attempt(os.remove, tmp, errors=(OSError,))
    return False


def to_mono(x: np.ndarray) -> np.ndarray:
    """Average the channels of a 2-D array; a 1-D array passes through."""
    if x.ndim != 2:
        return x
    return x.mean(axis=1)


def resample(x: np.ndarray, sr_from: int, sr_to: int) -> np.ndarray:
    """High-quality soxr rate change to float32; the input itself when the rates match."""
    if sr_from == sr_to:
        return x
    import soxr

    changed = soxr.resample(np.asarray(x, dtype=np.float32), sr_from, sr_to, quality="HQ")
    return changed.astype(np.float32)


def write_wav(path: str, x: np.ndarray, sr: int = 48000, subtype: str = "PCM_24") -> str:
    """Write x as a WAV file, creating missing parent folders; returns the path as text."""
    import soundfile

    target = str(path)
    Path(target).parent.mkdir(parents=True, exist_ok=True)
    soundfile.write(target, np.asarray(x), sr, subtype=subtype)
    return target


def _run_ffmpeg(path: str, sr: int, channels: int):
    argv = [find_tool("ffmpeg"), "-nostdin", "-v", "error", "-i", path, "-vn",
            "-f", "f32le", "-acodec", "pcm_f32le", "-ar", str(sr), "-ac", str(channels), "-"]
    done = subprocess.run(argv, stdin=subprocess.DEVNULL, capture_output=True)
    return np.frombuffer(done.stdout, dtype="<f4").astype(np.float32), done


def _explain_failure(path: str, done: subprocess.CompletedProcess) -> AudioError:
    log = done.stderr.decode("utf-8", "replace")
    facts = probe(path)
    silent_video = "probe_error" not in facts and facts.get("has_video") and not facts.get("codec")
    if silent_video:
        return AudioError(f"{path} has no audio track (a silent render? mux the soundtrack in first)")
    if done.returncode == 0 or "does not contain any stream" in log:
        return AudioError(f"{path}: no audio stream")
    return AudioError(f"cannot decode {path}: {ffmpeg_reason(log)}")


def _arrange(samples: np.ndarray, mono: bool, copy_up: bool) -> np.ndarray:
    """Shape interleaved ffmpeg output: (n,) for mono, (n, 2) for stereo."""
    if mono:
        return samples
    if copy_up:
        return np.column_stack([samples, samples])
    even = samples.size - samples.size % 2
    return samples[:even].reshape(-1, 2)


def _fresh_decode(path: str, sr: int, mono: bool) -> np.ndarray:
    """Decode with ffmpeg; a mono source asked for in stereo is copied to both sides."""
    copy_up = not mono and int(probe(path).get("channels") or 0) == 1
    samples, done = _run_ffmpeg(path, sr, 1 if mono or copy_up else 2)
    if done.returncode != 0 or samples.size == 0:
        raise _explain_failure(path, done)
    return _arrange(samples, mono, copy_up)


def _cache_path(path: str, sr: int, mono: bool) -> Path:
    return Path(config.DEC_CACHE) / f"{decode_key(path, sr, 1 if mono else 2)}.npy"


def _from_cache(where: Path) -> np.ndarray | None:
    if not where.is_file():
        return None
    return attempt(lambda name: np.load(name, allow_pickle=False), str(where), errors=(OSError, ValueError))


def _into_cache(where: Path, x: np.ndarray) -> None:
    """Write beside the entry under a per-process name, then move it in; errors cost the entry only."""
    scratch = where.with_name(f"{where.stem}.{os.getpid()}.tmp.npy")

    def save() -> None:
        config.ensure_dirs()
        np.save(str(scratch), x)

    stored = outcome(save, errors=(OSError,))[1] is None
    if not (stored and replace_quietly(str(scratch), str(where))):
        attempt(os.remove, str(scratch), errors=(OSError,))


def _refuse_non_finite(path: str, x: np.ndarray, sr: int) -> None:
    broken = ~np.isfinite(x)
    if broken.ndim == 2:
        broken = broken.any(axis=1)
    count = int(broken.sum())
    if count:
        when = int(np.argmax(broken)) / sr
        raise AudioError(f"{path}: {count} NaN / Inf sample(s), first at {when:.3f} s: "
                         "the file is corrupt, or the code that made it divided by zero")


def _decoded(path: str, sr: int, mono: bool, cache: bool) -> np.ndarray:
    """The decode from the cache when present; otherwise a fresh one, stored when caching."""
    if not cache:
        return _fresh_decode(path, sr, mono)
    where = _cache_path(path, sr, mono)
    hit = _from_cache(where)
    if hit is not None:
        return hit
    fresh = _fresh_decode(path, sr, mono)
    _into_cache(where, fresh)
    return fresh


def _span(x: np.ndarray, sr: int, start: float | None, end: float | None) -> np.ndarray:
    if start is None and end is None:
        return x
    first = max(0, int(round((start or 0.0) * sr)))
    stop = len(x) if end is None else min(len(x), int(round(end * sr)))
    return x[first:stop]


def load(path: str, sr: int = 48000, mono: bool = False, start: float | None = None,
         end: float | None = None, cache: bool = True) -> np.ndarray:
    """Read a media file as float32 samples: shape (n, 2) for stereo, (n,) for mono."""
    path = str(path)
    if not os.path.exists(path):
        raise AudioError(f"no such file: {path}")
    x = _decoded(path, sr, mono, cache)
    _refuse_non_finite(path, x, sr)
    return _span(x, sr, start, end)


def duration(path: str) -> float:
    """Seconds of audio in the file, from its mono decode."""
    return len(load(path, mono=True)) / SR
