"""ffprobe facts about a media file, and readable ffmpeg failure text.

``probe`` never raises: a failure becomes ``{"probe_error": <text>}``.
"""

from __future__ import annotations

import json
import re
import shutil
import subprocess
from fractions import Fraction

from hark.core.safe import attempt, outcome

LOSSY_CODECS = frozenset({"aac", "mp3", "mp2", "opus", "vorbis", "wmav2", "ac3", "eac3"})

REASON_MAX = 200
_PROBE_LIMIT_S = 60
_STREAM_RATE_CAP = 10_000_000
_LOG_TAG = re.compile(r"^\[[^\]]* @ (0x)?[0-9a-fA-F]+\]\s*")
_NOISE_HEADS = ("error opening output", "conversion failed")
_ENTRIES = ("stream=codec_type,codec_name,sample_rate,channels,bit_rate,width,height,r_frame_rate"
            ":stream_disposition=attached_pic:format=duration,bit_rate")


class AudioError(RuntimeError):
    """One-line message about a decode, probe or path failure."""


def find_tool(name: str) -> str:
    """Absolute path of an executable on PATH, or AudioError."""
    where = shutil.which(name)
    if where is None:
        raise AudioError(f"{name} not found on PATH")
    return where


def ffmpeg_reason(stderr: str) -> str:
    """The last useful line of ffmpeg's error output, at most 200 characters."""
    useful = "ffmpeg failed"
    for raw in stderr.splitlines():
        text = _LOG_TAG.sub("", raw.strip()).strip()
        if text and not text.lower().startswith(_NOISE_HEADS):
            useful = text
    return useful[:REASON_MAX]


def _ask_ffprobe(path: str) -> dict:
    argv = [find_tool("ffprobe"), "-v", "error", "-print_format", "json", "-show_entries", _ENTRIES, path]
    done = subprocess.run(argv, stdin=subprocess.DEVNULL, capture_output=True, timeout=_PROBE_LIMIT_S)
    body = done.stdout.decode("utf-8", "replace").strip()
    if not body:
        return {}
    return json.loads(body)


def _frame_rate(text: str | None) -> float | None:
    ratio = attempt(Fraction, text or "0/0", errors=(ValueError, ZeroDivisionError))
    if ratio is None:
        return None
    return round(float(ratio), 3)


def _as_kbps(bits: object) -> int:
    return int(round(int(str(bits)) / 1000))


def _bitrate(track: dict, container: dict, has_picture: bool) -> int | None:
    """Stream bit rate in kb/s; the container rate stands in only without any picture stream."""
    own = str(track.get("bit_rate") or "")
    if own.isdigit() and int(own) < _STREAM_RATE_CAP:
        return _as_kbps(own)
    total = str(container.get("bit_rate") or "")
    if total.isdigit() and not has_picture:
        return _as_kbps(total)
    return None


def _probe_fields(raw: dict) -> dict:
    """Flatten raw ffprobe JSON into hark's probe record."""
    container = raw.get("format") or {}
    every = raw.get("streams") or []
    track = {}
    for stream in every:
        if stream.get("codec_type") == "audio":
            track = stream
            break
    visual = [s for s in every if s.get("codec_type") == "video"]
    motion = [s for s in visual if not (s.get("disposition") or {}).get("attached_pic")]
    name = track.get("codec_name")
    record = {
        "codec": name,
        "src_rate": int(track.get("sample_rate") or 0),
        "channels": int(track.get("channels") or 0),
        "kbps": _bitrate(track, container, bool(visual)),
        "lossy": name in LOSSY_CODECS,
        "duration": round(float(container.get("duration") or 0.0), 3),
        "has_video": bool(motion),
    }
    if motion:
        first = motion[0]
        record["video"] = {"width": first.get("width"), "height": first.get("height"),
                           "fps": _frame_rate(first.get("r_frame_rate"))}
    return record


def probe(path: str) -> dict:
    """Facts about the first audio stream and the container, from ffprobe."""
    caught = (OSError, subprocess.SubprocessError, ValueError, AudioError)
    record, failure = outcome(lambda where: _probe_fields(_ask_ffprobe(where)), str(path), errors=caught)
    if failure is not None:
        return {"probe_error": str(failure)[:REASON_MAX]}
    return record
