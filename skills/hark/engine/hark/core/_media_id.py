"""Content ids for files and arrays, used as cache keys across hark.

A file id mixes the absolute path, the size, the modification time and a hash
of 16 evenly spaced 4 KiB blocks, so that a large file is never read whole.
"""

from __future__ import annotations

import hashlib
import os

import numpy

from hark.core.safe import attempt

DECODE_VERSION = 2

_CHUNK = 4096
_CHUNK_COUNT = 16


def _sample_offsets(size: int) -> list[int]:
    """Byte offsets of the sampled blocks, spread from 0 to the last full block."""
    room = max(0, size - _CHUNK)
    steps = max(1, _CHUNK_COUNT - 1)
    return sorted({(room * k) // steps for k in range(_CHUNK_COUNT)})


def _read_chunks(path: str, size: int) -> str:
    pieces = []
    with open(path, "rb") as handle:
        for where in _sample_offsets(size):
            handle.seek(where)
            pieces.append(handle.read(_CHUNK))
    return hashlib.sha1(b"".join(pieces)).hexdigest()[:12]


def _sampled_hash(path: str, size: int) -> str:
    """12 hex of SHA-1 over the sampled blocks; the word `unreadable` on a read error."""
    return attempt(_read_chunks, path, size, errors=(OSError,), default="unreadable")


def _fingerprint(path: str) -> str:
    """Absolute path, size, mtime in ns and block hash, joined by bars."""
    where = os.path.abspath(path)
    info = attempt(os.stat, where, errors=(OSError,))
    size, stamp = (-1, -1) if info is None else (info.st_size, info.st_mtime_ns)
    return "|".join((where, str(size), str(stamp), _sampled_hash(where, max(size, 0))))


def _hex20(text: str) -> str:
    return hashlib.sha1(text.encode("utf-8")).hexdigest()[:20]


def file_id(path: str) -> str:
    """A 20-hex id that changes when the file's path, size, mtime or sampled bytes change."""
    return _hex20(_fingerprint(path))


def array_id(x: numpy.ndarray) -> str:
    """A 20-hex id of an array's float32 bytes plus its shape."""
    flat = numpy.ascontiguousarray(numpy.asarray(x, dtype=numpy.float32))
    hasher = hashlib.sha1(flat.tobytes())
    hasher.update(str(flat.shape).encode("ascii"))
    return hasher.hexdigest()[:20]


def decode_key(path: str, sr: int, channels: int) -> str:
    """Cache key of one decode: the file fingerprint, rate, channel count and decoder version."""
    return _hex20(f"{_fingerprint(path)}|{sr}|{channels}|v{DECODE_VERSION}")
