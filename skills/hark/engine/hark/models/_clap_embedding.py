"""Text and audio embedding for the CLAP wrapper, with both disk caches.

``ClapEmbedding`` is mixed into the CLAP class. It relies on these instance
attributes: ``processor``, ``model``, ``dtype``, ``cache_dir``, ``_torch`` and
``_text`` (key24 of a text -> its unit vector).

Audio goes in at 48 kHz. A clip longer than 10 s is cut into 10 s windows
every 5 s; its embedding is the renormalised mean of the window vectors.
"""

from __future__ import annotations

import os
import threading
from collections.abc import Sequence

import numpy

from hark.core import audio
from hark.models import _common

WIN_S = 10.0
HOP_S = 5.0
WIN = 480_000
MIN_SAMPLES = 480
DIM = 512
TEXT_BATCH = 64
AUDIO_BATCH = 8

MODEL_LOCK = threading.RLock()


def _nothing() -> numpy.ndarray:
    return numpy.zeros((0, DIM), numpy.float32)


def _slices(counts: list[int]):
    """Start and stop row of each group, given group sizes in order."""
    stop = 0
    for size in counts:
        yield stop, stop + size
        stop += size


def _segments(x: numpy.ndarray) -> list[numpy.ndarray]:
    """One mono window for a short clip; overlapping padded windows for a long one."""
    mono = audio.to_mono(numpy.asarray(x, dtype=numpy.float32))
    if len(mono) <= WIN:
        return [mono]
    framed = audio.windows(mono, WIN_S, HOP_S, sr=audio.SR, pad_last=True)
    return [piece for _, piece in framed]


def _prepare(seg: numpy.ndarray, pad: str) -> numpy.ndarray:
    """Pad to the model minimum (and to a full window for "zero"), then trim to a window."""
    need = MIN_SAMPLES
    if pad == "zero":
        need = max(need, WIN)
    if len(seg) < need:
        seg = numpy.pad(seg, (0, need - len(seg)))
    return numpy.ascontiguousarray(seg[:WIN], dtype=numpy.float32)


def _group_means(rows: numpy.ndarray, counts: list[int]) -> numpy.ndarray:
    """Unit-length mean vector of each group of window rows."""
    if not counts:
        return _nothing()
    averaged = numpy.stack([rows[lo:hi].mean(axis=0) for lo, hi in _slices(counts)])
    return _common.unit_rows(averaged)


def _clips(arrays: numpy.ndarray | Sequence[numpy.ndarray]) -> list[numpy.ndarray]:
    if isinstance(arrays, numpy.ndarray) and arrays.ndim == 1:
        return [arrays]
    return list(arrays)


class ClapEmbedding:
    """Embedding methods shared by every loaded CLAP model."""

    # text side ---------------------------------------------------------

    def _phrase_file(self) -> str:
        return os.path.join(self.cache_dir, "text.npz")

    def _load_phrases(self) -> dict[str, numpy.ndarray]:
        try:
            with numpy.load(self._phrase_file(), allow_pickle=False) as stored:
                return {name: numpy.asarray(stored[name], dtype=numpy.float32) for name in stored.files}
        except (OSError, ValueError):
            return {}

    def _store_phrases(self) -> None:
        with MODEL_LOCK:
            everything = self._load_phrases()
            everything.update(self._text)
            scratch = os.path.join(self.cache_dir, f"text.{os.getpid()}.tmp.npz")
            try:
                numpy.savez(scratch, **everything)
            except OSError:
                return
            audio.replace_quietly(scratch, self._phrase_file())

    def _encode_phrases(self, phrases: list[str]) -> numpy.ndarray:
        tokens = self.processor.tokenizer(phrases, padding=True, return_tensors="pt")
        with self._torch.inference_mode():
            feats = _common.pool_output(self.model.get_text_features(**tokens))
        return _common.unit_rows(feats.float().cpu().numpy())

    def embed_text(self, texts: str | Sequence[str], batch: int = TEXT_BATCH) -> numpy.ndarray:
        """Unit text vectors, shape (n, 512); each text is encoded once and kept on disk."""
        phrases = [texts] if isinstance(texts, str) else list(texts)
        if not phrases:
            return _nothing()
        hashes = [_common.key24(phrase) for phrase in phrases]
        fresh = {}
        for phrase, digest in zip(phrases, hashes):
            if digest not in self._text:
                fresh.setdefault(phrase, None)
        pending = list(fresh)
        for lo in range(0, len(pending), batch):
            group = pending[lo: lo + batch]
            vectors = self._encode_phrases(group)
            for phrase, vector in zip(group, vectors):
                self._text[_common.key24(phrase)] = vector
        if pending:
            self._store_phrases()
        return numpy.stack([self._text[digest] for digest in hashes]).astype(numpy.float32)

    # audio side --------------------------------------------------------

    def _encode_windows(self, windows: list[numpy.ndarray], batch: int) -> numpy.ndarray:
        torch = self._torch
        blocks = []
        for lo in range(0, len(windows), batch):
            packed = self.processor.feature_extractor(windows[lo: lo + batch], sampling_rate=audio.SR,
                                                      return_tensors="pt")
            tensors = _common.to_model_inputs(torch, packed, self.dtype)
            with torch.inference_mode():
                feats = _common.pool_output(self.model.get_audio_features(**tensors))
            blocks.append(_common.unit_rows(feats.float().cpu().numpy()))
        if not blocks:
            return _nothing()
        return numpy.concatenate(blocks)

    def embed_audio(self, arrays: numpy.ndarray | Sequence[numpy.ndarray], pad: str = "repeatpad",
                    batch: int = AUDIO_BATCH, per_window: bool = False) -> numpy.ndarray | list[numpy.ndarray]:
        """Vectors for 48 kHz clips: (n, 512) clip means, or a (k_i, 512) array per clip."""
        per_clip = [[_prepare(piece, pad) for piece in _segments(clip)] for clip in _clips(arrays)]
        sizes = [len(windows) for windows in per_clip]
        table = self._encode_windows([w for windows in per_clip for w in windows], batch)
        if per_window:
            return [table[lo:hi] for lo, hi in _slices(sizes)]
        return _group_means(table, sizes)

    def _vector_file(self, path: str, start: float | None, end: float | None, pad: str) -> str:
        return os.path.join(self.cache_dir, f"a_{_common.file_key(path, start, end, pad)}.npy")

    def embed_file(self, path: str, start: float | None = None, end: float | None = None,
                   pad: str = "repeatpad") -> numpy.ndarray:
        """The (512,) vector of a span of a file, reused by content hash."""
        store = self._vector_file(path, start, end, pad)
        hit = _common.read_npy(store)
        if hit is not None:
            return hit
        clip = audio.load(path, mono=True, start=start, end=end)
        vector = self.embed_audio([clip], pad=pad)[0]
        _common.write_npy(store, vector)
        return vector

    def embed_files(self, paths: Sequence[str], pad: str = "repeatpad", batch: int = AUDIO_BATCH) -> numpy.ndarray:
        """Vectors of whole files, shape (n, 512), in the order given."""
        stores = [self._vector_file(p, None, None, pad) for p in paths]
        vectors = [_common.read_npy(s) for s in stores]
        absent = [n for n, v in enumerate(vectors) if v is None]
        for lo in range(0, len(absent), batch):
            chosen = absent[lo: lo + batch]
            made = self.embed_audio([audio.load(paths[n], mono=True) for n in chosen], pad=pad)
            for n, vector in zip(chosen, made):
                vectors[n] = vector
                _common.write_npy(stores[n], vector)
        if not vectors:
            return _nothing()
        return numpy.stack(vectors).astype(numpy.float32)
