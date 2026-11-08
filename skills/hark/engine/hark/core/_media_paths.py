"""Turn user path arguments into media files, and cut arrays into windows.

Folders expand in name order, own files before sub-folders. A glob keeps only
media files. Two inputs that name the same file count once.
"""

from __future__ import annotations

import glob
import os
from collections.abc import Iterable, Sequence
from pathlib import Path

from hark.core._media_probe import AudioError
from hark.core.safe import attempt

AUDIO_EXTS = frozenset({".wav", ".flac", ".ogg", ".oga", ".opus", ".mp3", ".m4a", ".aac", ".aif", ".aiff", ".wma"})
VIDEO_EXTS = frozenset({".mp4", ".mov", ".m4v", ".webm", ".mkv", ".avi"})
MEDIA_EXTS = AUDIO_EXTS | VIDEO_EXTS

_NAMED_AT_MOST = 5


def windows(x, win: float, hop: float, sr: int = 48000,
            pad_last: bool = False) -> list[tuple[float, object]]:
    """Split x into (start in seconds, slice) pairs of win seconds every hop seconds."""
    width = int(round(win * sr))
    total = len(x)
    if total <= width:
        return [(0.0, x)]
    step = max(1, int(round(hop * sr)))
    offsets = list(range(0, total - width + 1, step))
    if pad_last and offsets[-1] + width < total:
        offsets.append(total - width)
    return [(at / sr, x[at: at + width]) for at in offsets]


def _suffix_in(name: str, kinds: frozenset[str]) -> bool:
    return Path(name).suffix.lower() in kinds


def _folder_media(folder: str, deep: bool, kinds: frozenset[str]) -> list[str]:
    listing = attempt(lambda where: list(os.scandir(where)), folder, errors=(OSError,), default=[])
    listing = sorted(listing, key=lambda entry: entry.name)
    here = [e.path for e in listing if e.is_file() and _suffix_in(e.name, kinds)]
    if not deep:
        return here
    for entry in listing:
        if entry.is_dir(follow_symlinks=False):
            here.extend(_folder_media(entry.path, True, kinds))
    return here


def _one_input(item: str, deep: bool, kinds: frozenset[str]) -> list[str]:
    if os.path.isfile(item):
        return [item]
    if os.path.isdir(item):
        return _folder_media(item, deep, kinds)
    matches = sorted(glob.glob(item, recursive=True))
    return [m for m in matches if os.path.isfile(m) and _suffix_in(m, MEDIA_EXTS)]


def _listify(paths) -> list:
    if isinstance(paths, (str, os.PathLike)):
        return [paths]
    return list(paths)


def list_media(paths: str | Sequence[str], recursive: bool = True,
               exts: Iterable[str] = MEDIA_EXTS) -> list[str]:
    """Media files named by files, folders and globs, each file once, first spelling kept."""
    kinds = frozenset(exts)
    seen: dict[str, str] = {}
    for item in _listify(paths):
        for found in _one_input(str(item), recursive, kinds):
            same = os.path.normcase(os.path.abspath(found)).casefold()
            seen.setdefault(same, found)
    return list(seen.values())


def _why_empty(item: str) -> str | None:
    if list_media([item]):
        return None
    if os.path.exists(item) or glob.glob(item, recursive=True):
        return f"{item} (no audio or video files in it)"
    return f"{item} (no such file or folder)"


def check_paths(paths: str | Sequence[str], what: str = "paths") -> None:
    """AudioError that names each input giving no media file; silent when all give some."""
    problems = [p for p in (_why_empty(str(item)) for item in _listify(paths)) if p]
    if not problems:
        return
    extra = " ..." if len(problems) > _NAMED_AT_MOST else ""
    raise AudioError(f"{what}: nothing playable in " + ", ".join(problems[:_NAMED_AT_MOST]) + extra)
