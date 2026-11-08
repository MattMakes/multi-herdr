"""The CLAP text-audio embedding model on CPU, with text and audio embedding caches."""

from __future__ import annotations

import os
import threading
from collections.abc import Sequence
from typing import Any

import numpy as np

from hark.core import audio, config
from hark.models import _common, hub

WIN_S = 10.0
HOP_S = 5.0
WIN = 480_000
MIN_SAMPLES = 480
DIM = 512
TEXT_BATCH = 64
AUDIO_BATCH = 8
FALLBACK_LOGIT_SCALE = 100.0

_LOCK = threading.RLock()
_INSTANCE: Clap | None = None


def _as_list(arrays: np.ndarray | Sequence[np.ndarray]) -> list[np.ndarray]:
    single = isinstance(arrays, np.ndarray) and arrays.ndim == 1
    return [arrays] if single else list(arrays)


def _split(rows: np.ndarray, counts: list[int]) -> list[np.ndarray]:
    bounds = np.cumsum([0] + counts)
    return [rows[a:b] for a, b in zip(bounds[:-1], bounds[1:])]


def native_bf16(torch: Any) -> bool:
    """True only when the CPU has native bf16 matrix instructions."""
    return _common.native_bf16(torch)


def _segments(x: np.ndarray) -> list[np.ndarray]:
    m = audio.to_mono(np.asarray(x, dtype=np.float32))
    if len(m) <= WIN:
        return [m]
    return [seg for _, seg in audio.windows(m, WIN_S, HOP_S, sr=audio.SR, pad_last=True)]


def _prepare(seg: np.ndarray, pad: str) -> np.ndarray:
    if len(seg) < MIN_SAMPLES:
        seg = np.pad(seg, (0, MIN_SAMPLES - len(seg)))
    if pad == "zero" and len(seg) < WIN:
        seg = np.pad(seg, (0, WIN - len(seg)))
    return np.ascontiguousarray(seg[:WIN], dtype=np.float32)


def _group_means(rows: np.ndarray, counts: list[int]) -> np.ndarray:
    bounds = np.cumsum([0] + counts)
    means = [rows[a:b].mean(axis=0) for a, b in zip(bounds[:-1], bounds[1:])]
    return _common.unit_rows(np.stack(means)) if means else np.zeros((0, DIM), np.float32)


class Clap:
    """A loaded CLAP model with its caches."""

    def __init__(self, model_id: str | None = None) -> None:
        torch = _common.torch_ready("CLAP")
        from transformers import ClapModel, ClapProcessor

        config.ensure_dirs()
        self.model_id, self.revision = _common.wanted(model_id, config.CLAP_MODEL, config.CLAP_REVISION)
        self.spec = hub.ensure(self.model_id, self.revision)
        self.weights = self.spec.weights
        self.dir = self.spec.snapshot
        self.processor = _common.load_pretrained(ClapProcessor, self.spec)
        model = _common.load_pretrained(ClapModel, self.spec, **_common.load_options(self.spec))
        self.model, self.precision = _common.choose_precision(torch, model.eval())
        self.dtype = torch.bfloat16 if self.precision == "bf16" else torch.float32
        self.logit_scale = self._logit_scale()
        self.cache_dir = _common.embed_store("clap", self.spec, self.precision)
        self._torch = torch
        self._text: dict[str, np.ndarray] = self._read_text_cache()

    def _logit_scale(self) -> float:
        try:
            return float(self.model.logit_scale_a.exp().item())
        except Exception:  # noqa: BLE001 - a missing parameter uses the fallback
            return FALLBACK_LOGIT_SCALE

    # ------------------------------------------------------------ text

    @property
    def _text_path(self) -> str:
        return os.path.join(self.cache_dir, "text.npz")

    def _read_text_cache(self) -> dict[str, np.ndarray]:
        try:
            with np.load(self._text_path, allow_pickle=False) as data:
                return {k: np.asarray(data[k], dtype=np.float32) for k in data.files}
        except (OSError, ValueError):
            return {}

    def _save_text_cache(self) -> None:
        with _LOCK:
            merged = {**self._read_text_cache(), **self._text}
            tmp = os.path.join(self.cache_dir, f"text.{os.getpid()}.tmp.npz")
            try:
                np.savez(tmp, **merged)
            except OSError:
                return
            audio.replace_quietly(tmp, self._text_path)

    def _embed_text_batch(self, texts: list[str]) -> np.ndarray:
        inputs = self.processor.tokenizer(texts, padding=True, return_tensors="pt")
        with self._torch.inference_mode():
            out = _common.pool_output(self.model.get_text_features(**inputs))
        return _common.unit_rows(out.float().cpu().numpy())

    def embed_text(self, texts: str | Sequence[str], batch: int = TEXT_BATCH) -> np.ndarray:
        """(n, 512) unit text embeddings, cached by text."""
        items = [texts] if isinstance(texts, str) else list(texts)
        if not items:
            return np.zeros((0, DIM), np.float32)
        keys = [_common.key24(t) for t in items]
        self._fill_text(list(dict.fromkeys(t for t, k in zip(items, keys) if k not in self._text)), batch)
        return np.stack([self._text[k] for k in keys]).astype(np.float32)

    def _fill_text(self, todo: list[str], batch: int) -> None:
        for part in (todo[i: i + batch] for i in range(0, len(todo), batch)):
            self._text.update(zip(map(_common.key24, part), self._embed_text_batch(part)))
        if todo:
            self._save_text_cache()

    # ------------------------------------------------------------ audio

    def _embed_segments(self, segs: list[np.ndarray], batch: int) -> np.ndarray:
        rows = []
        for i in range(0, len(segs), batch):
            feats = self.processor.feature_extractor(segs[i: i + batch], sampling_rate=audio.SR,
                                                     return_tensors="pt")
            inputs = _common.to_model_inputs(self._torch, feats, self.dtype)
            with self._torch.inference_mode():
                out = _common.pool_output(self.model.get_audio_features(**inputs))
            rows.append(_common.unit_rows(out.float().cpu().numpy()))
        return np.concatenate(rows) if rows else np.zeros((0, DIM), np.float32)

    def embed_audio(self, arrays: np.ndarray | Sequence[np.ndarray], pad: str = "repeatpad", batch: int = AUDIO_BATCH,
                    per_window: bool = False) -> np.ndarray | list[np.ndarray]:
        """Embed 48 kHz arrays: (n, 512) means, or one (k_i, 512) array per input."""
        groups = [[_prepare(s, pad) for s in _segments(x)] for x in _as_list(arrays)]
        counts = [len(g) for g in groups]
        rows = self._embed_segments([s for g in groups for s in g], batch)
        return _split(rows, counts) if per_window else _group_means(rows, counts)

    def _audio_path(self, path: str, start: float | None, end: float | None, pad: str) -> str:
        return os.path.join(self.cache_dir, f"a_{_common.file_key(path, start, end, pad)}.npy")

    def embed_file(self, path: str, start: float | None = None, end: float | None = None,
                   pad: str = "repeatpad") -> np.ndarray:
        """(512,) embedding of a file range, cached by content."""
        target = self._audio_path(path, start, end, pad)
        cached = _common.read_npy(target)
        if cached is not None:
            return cached
        vec = self.embed_audio([audio.load(path, mono=True, start=start, end=end)], pad=pad)[0]
        _common.write_npy(target, vec)
        return vec

    def embed_files(self, paths: Sequence[str], pad: str = "repeatpad", batch: int = AUDIO_BATCH) -> np.ndarray:
        """(n, 512) embeddings of whole files, in input order."""
        targets = [self._audio_path(p, None, None, pad) for p in paths]
        found = [_common.read_npy(t) for t in targets]
        todo = [i for i, v in enumerate(found) if v is None]
        for i in range(0, len(todo), batch):
            part = todo[i: i + batch]
            vecs = self.embed_audio([audio.load(paths[j], mono=True) for j in part], pad=pad)
            for j, vec in zip(part, vecs):
                found[j] = vec
                _common.write_npy(targets[j], vec)
        return np.stack(found).astype(np.float32) if found else np.zeros((0, DIM), np.float32)


def get(model_id: str | None = None) -> Clap:
    """The process-wide CLAP model; loads on first use or for another model id."""
    global _INSTANCE
    _common.require_ml("CLAP")
    with _LOCK:
        wanted = model_id or config.CLAP_MODEL
        if _INSTANCE is None or _INSTANCE.model_id != wanted:
            _INSTANCE = Clap(model_id)
        return _INSTANCE
