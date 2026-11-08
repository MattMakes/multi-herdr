"""Windowing and scoring for the AudioSet tagger, kept apart from model loading.

The tagger listens at 16 kHz in windows of 10.24 s that overlap by half. A
clip's score per class is the maximum (or the mean) over its windows.
The tagger class mixes in ``TagScoring``. They need these attributes on the instance:
``extractor``, ``model``, ``dtype``, ``labels``, ``index``, ``cache_dir`` and ``_torch``.
"""

from __future__ import annotations

import os
from collections.abc import Sequence

import numpy

from hark.core import audio
from hark.models import _common

RATE = 16000
WIN = 163_840
HOP = WIN // 2
MIN_SAMPLES = 1600
N_CLASSES = 527
BATCH = 8


def window_starts(n: int) -> list[int]:
    """Sample offsets of each window; the final window always ends at sample n."""
    last = n - WIN
    if last <= 0:
        return [0]
    offsets = list(range(0, last + 1, HOP))
    if offsets[-1] != last:
        offsets.append(last)
    return offsets


def _segments(x: numpy.ndarray) -> list[numpy.ndarray]:
    """Cut a 48 kHz clip into 16 kHz float32 windows, padding short ones."""
    mono = audio.to_mono(numpy.asarray(x, dtype=numpy.float32))
    low = audio.resample(mono, audio.SR, RATE)
    pieces = []
    for at in window_starts(len(low)):
        chunk = low[at: at + WIN]
        short = MIN_SAMPLES - len(chunk)
        if short > 0:
            chunk = numpy.pad(chunk, (0, short))
        pieces.append(chunk.astype(numpy.float32))
    return pieces


def _empty() -> numpy.ndarray:
    return numpy.zeros((0, N_CLASSES), numpy.float32)


def _pool_rows(table: numpy.ndarray, sizes: list[int], reduce: str) -> numpy.ndarray:
    """Collapse each clip's consecutive window rows to one row by max (or mean)."""
    if not sizes:
        return _empty()
    pool = numpy.max if reduce == "max" else numpy.mean
    bounds = numpy.cumsum([0, *sizes])
    pooled = [pool(table[lo:hi], axis=0) for lo, hi in zip(bounds[:-1], bounds[1:])]
    return numpy.stack(pooled).astype(numpy.float32)


class TagScoring:
    """Turn clips and files into AudioSet class probabilities."""

    def _score_batch(self, chunks: list[numpy.ndarray]) -> numpy.ndarray:
        """Sigmoid class scores for up to one batch of 16 kHz windows."""
        torch = self._torch
        with _common.hush_loaders():
            packed = self.extractor(chunks, sampling_rate=RATE, return_tensors="pt")
        tensors = _common.to_model_inputs(torch, packed, self.dtype)
        with torch.inference_mode():
            raw = self.model(**tensors).logits.float()
        return torch.sigmoid(raw).cpu().numpy().astype(numpy.float32)

    def _score_windows(self, windows: list[numpy.ndarray], batch: int) -> numpy.ndarray:
        scored = [self._score_batch(windows[i: i + batch]) for i in range(0, len(windows), batch)]
        return numpy.concatenate(scored) if scored else _empty()

    def probs(self, arrays: numpy.ndarray | Sequence[numpy.ndarray], reduce: str = "max", batch: int = BATCH) -> numpy.ndarray:
        """Class probabilities, shape (n, 527), for n clips at 48 kHz."""
        single = isinstance(arrays, numpy.ndarray) and arrays.ndim == 1
        per_clip = [_segments(c) for c in ([arrays] if single else list(arrays))]
        table = self._score_windows([w for windows in per_clip for w in windows], batch)
        return _pool_rows(table, [len(windows) for windows in per_clip], reduce)

    def probs_file(self, path: str, start: float | None = None, end: float | None = None,
                   reduce: str = "max") -> numpy.ndarray:
        """Probabilities, shape (527,), for a span of a file; reused by content hash."""
        key = _common.file_key(path, start, end, reduce)
        store = os.path.join(self.cache_dir, f"p_{key}.npy")
        hit = _common.read_npy(store)
        if hit is not None:
            return hit
        clip = audio.load(path, mono=True, start=start, end=end)
        scores = self.probs([clip], reduce=reduce)[0]
        _common.write_npy(store, scores)
        return scores

    def top(self, p: numpy.ndarray, k: int = 10, min_p: float = 0.0) -> list[tuple[str, float]]:
        """Up to k labels by falling probability, dropping any below min_p."""
        ranked = numpy.argsort(-numpy.asarray(p))[:k]
        return [(self.labels[j], round(float(p[j]), 3)) for j in ranked if p[j] >= min_p]

    def top_file(self, path: str, k: int = 10, start: float | None = None,
                 end: float | None = None) -> list[tuple[str, float]]:
        """The top labels for a span of a file."""
        return self.top(self.probs_file(path, start, end), k)

    def prob_of(self, p: numpy.ndarray, label: str) -> float:
        """The probability that p gives one named label."""
        return float(p[self.index[label]])
