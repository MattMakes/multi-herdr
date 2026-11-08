"""The AudioSet tagger (Audio Spectrogram Transformer) on CPU: 527 class probabilities, cached."""

from __future__ import annotations

import os
import threading
from collections.abc import Sequence

import numpy as np

from hark.core import audio, config
from hark.models import _common, hub

RATE = 16000
WIN = 163_840
HOP = 81_920
MIN_SAMPLES = 1600
N_CLASSES = 527
BATCH = 8

_LOCK = threading.RLock()
_INSTANCE: AST | None = None


def window_starts(n: int) -> list[int]:
    """Window starts in samples at 16 kHz: hop half a window, one flush with the end."""
    if n <= WIN:
        return [0]
    starts = list(range(0, n - WIN + 1, HOP))
    if (n - WIN) % HOP:
        starts.append(n - WIN)
    return starts


def _segments(x: np.ndarray) -> list[np.ndarray]:
    m = audio.resample(audio.to_mono(np.asarray(x, dtype=np.float32)), audio.SR, RATE)
    segs = [m[s: s + WIN] for s in window_starts(len(m))]
    return [np.pad(s, (0, max(0, MIN_SAMPLES - len(s)))).astype(np.float32) for s in segs]


def _reduce(rows: np.ndarray, how: str) -> np.ndarray:
    return rows.max(axis=0) if how == "max" else rows.mean(axis=0)


class AST:
    """A loaded AudioSet tagger with its cache."""

    def __init__(self, model_id: str | None = None) -> None:
        torch = _common.torch_ready("AudioSet tagger")
        from transformers import ASTFeatureExtractor, ASTForAudioClassification

        config.ensure_dirs()
        self.model_id, self.revision = _common.wanted(model_id, config.AST_MODEL, config.AST_REVISION)
        self.spec = hub.ensure(self.model_id, self.revision)
        self.weights = self.spec.weights
        self.dir = self.spec.snapshot
        self.extractor = _common.from_pretrained(ASTFeatureExtractor, self.spec)
        model = _common.from_pretrained(ASTForAudioClassification, self.spec, **_common.model_kwargs(self.spec))
        self.model, self.precision = _common.choose_precision(torch, model.eval())
        self.dtype = torch.bfloat16 if self.precision == "bf16" else torch.float32
        id2label = self.model.config.id2label
        self.labels = [str(id2label[i]) for i in range(len(id2label))]
        self.index = {label: i for i, label in enumerate(self.labels)}
        self.cache_dir = _common.cache_folder("ast", self.spec, self.precision)
        self._torch = torch

    def _run(self, segs: list[np.ndarray]) -> np.ndarray:
        with _common.quiet():
            feats = self.extractor(segs, sampling_rate=RATE, return_tensors="pt")
        inputs = _common.cast_inputs(self._torch, feats, self.dtype)
        with self._torch.inference_mode():
            logits = self.model(**inputs).logits.float()
        return self._torch.sigmoid(logits).cpu().numpy().astype(np.float32)

    def _window_probs(self, segs: list[np.ndarray], batch: int) -> np.ndarray:
        rows = [self._run(segs[i: i + batch]) for i in range(0, len(segs), batch)]
        return np.concatenate(rows) if rows else np.zeros((0, N_CLASSES), np.float32)

    def probs(self, arrays: np.ndarray | Sequence[np.ndarray], reduce: str = "max", batch: int = BATCH) -> np.ndarray:
        """(n, 527) class probabilities of 48 kHz arrays."""
        items = [arrays] if isinstance(arrays, np.ndarray) and arrays.ndim == 1 else list(arrays)
        groups = [_segments(x) for x in items]
        rows = self._window_probs([s for g in groups for s in g], batch)
        bounds = np.cumsum([0] + [len(g) for g in groups])
        out = [_reduce(rows[a:b], reduce) for a, b in zip(bounds[:-1], bounds[1:])]
        return np.stack(out).astype(np.float32) if out else np.zeros((0, N_CLASSES), np.float32)

    def probs_file(self, path: str, start: float | None = None, end: float | None = None,
                   reduce: str = "max") -> np.ndarray:
        """(527,) probabilities of a file range, cached by content."""
        target = os.path.join(self.cache_dir, f"p_{_common.file_key(path, start, end, reduce)}.npy")
        cached = _common.read_npy(target)
        if cached is not None:
            return cached
        p = self.probs([audio.load(path, mono=True, start=start, end=end)], reduce=reduce)[0]
        _common.write_npy(target, p)
        return p

    def top(self, p: np.ndarray, k: int = 10, min_p: float = 0.0) -> list[tuple[str, float]]:
        """The k highest classes, descending, those with p >= min_p."""
        order = np.argsort(-np.asarray(p))[:k]
        return [(self.labels[i], round(float(p[i]), 3)) for i in order if p[i] >= min_p]

    def top_file(self, path: str, k: int = 10, start: float | None = None,
                 end: float | None = None) -> list[tuple[str, float]]:
        """top() of a file range."""
        return self.top(self.probs_file(path, start, end), k)

    def prob_of(self, p: np.ndarray, label: str) -> float:
        """Probability of one label."""
        return float(p[self.index[label]])


def get(model_id: str | None = None) -> AST:
    """The process-wide AudioSet tagger; loads on first use or for another model id."""
    global _INSTANCE
    _common.require_ml("AudioSet tagger")
    with _LOCK:
        wanted = model_id or config.AST_MODEL
        if _INSTANCE is None or _INSTANCE.model_id != wanted:
            _INSTANCE = AST(model_id)
        return _INSTANCE
