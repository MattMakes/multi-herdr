"""Checks on the arguments an MCP client passes to hark tools.

Each check returns the cleaned value or raises ``ToolError`` with a message the
agent can act on. Paths are made absolute against the server's working folder.
"""

from __future__ import annotations

import os
from typing import Any

from hark.surfaces._flow import raise_if

VIDEO_EXTS = (".mp4", ".mov", ".m4v", ".webm", ".mkv", ".avi")
CUE_TEXT_START = set("[{0123456789.-")


class ToolError(Exception):
    """A failed tool call with one readable line; the client receives an MCP error result."""


def _hint() -> str:
    return f"pass an absolute path (the server's working folder is {os.getcwd()})"


def absolute(raw: str) -> str:
    """Strip spaces and quotes, expand ~ and make the path absolute."""
    return os.path.abspath(os.path.expanduser(raw.strip().strip('"')))


def file_arg(value: Any, what: str = "path") -> str:
    """The absolute path of an existing file, or ToolError."""
    given = isinstance(value, str) and bool(value.strip().strip('"'))
    raise_if(not given, ToolError(f"{what}: a file path is required"))
    where = absolute(value)
    raise_if(os.path.isdir(where), ToolError(f"{what}: {where} is a folder; a file is expected"))
    raise_if(not os.path.exists(where), ToolError(f"{what}: no such file {where}; {_hint()}"))
    return where


def optional_file(value: Any, what: str) -> str | None:
    if value is None or value == "":
        return None
    return file_arg(value, what)


def out_arg(value: Any, what: str) -> str:
    if isinstance(value, str) and value.strip():
        return absolute(value)
    raise ToolError(f"{what}: an output path is required")


def _expand(item: str) -> list[str]:
    from hark.core import audio

    try:
        return list(audio.list_media([item]))
    except Exception:
        return []


def paths_arg(value: Any, what: str) -> list[str]:
    """One flat media list from files, folders and globs; each input must give media."""
    inputs = [value] if isinstance(value, str) else list(value or [])
    if not inputs:
        raise ToolError(f"{what}: give at least 1 file, folder or glob")
    media: list[str] = []
    barren: list[str] = []
    for raw in inputs:
        where = absolute(str(raw))
        hits = _expand(where)
        if hits:
            media.extend(hits)
        else:
            barren.append(where)
    if barren:
        raise ToolError(f"{what}: no media files in {', '.join(barren[:5])}; {_hint()}")
    return media


def choice_arg(value: str, allowed: tuple[str, ...], what: str) -> str:
    if value in allowed:
        return value
    raise ToolError(f"{what} must be one of {', '.join(allowed)} (got {value!r})")


def cues_arg(value: Any) -> Any:
    """None for no cues, a checked file path for a cue file, else the inline cue text or list."""
    if value is None or value == "" or value == []:
        return None
    inline = not isinstance(value, str) or value.strip()[:1] in CUE_TEXT_START
    return value if inline else file_arg(value, "cues")


def video_default(path: str, video: Any) -> str | None:
    """The video file to show: the one given, else the input itself when it is a video."""
    if video is not None and video != "":
        return file_arg(video, "video")
    if os.path.splitext(path)[1].lower() in VIDEO_EXTS:
        return path
    return None


def range_arg(value: Any, what: str, lo: float, hi: float | None = None) -> Any:
    if value is None:
        return None
    too_low = value < lo
    too_high = hi is not None and value > hi
    if not (too_low or too_high):
        return value
    upper = "inf" if hi is None else hi
    raise ToolError(f"{what} must be in {lo}..{upper} (got {value})")
