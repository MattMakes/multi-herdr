"""CPU tagging against the 527 AudioSet classes, with results kept on disk.

This module loads the model and holds the shared instance; the window and
scoring code is in ``hark.models._ast_scoring``.
"""

from __future__ import annotations

import threading

from hark.core import config
from hark.models import _common, hub
from hark.models._ast_scoring import (  # noqa: F401  (re-exported names)
    HOP,
    MIN_SAMPLES,
    WIN,
    TagScoring,
    _segments,
    window_starts,
)

_LOCK = threading.RLock()
_INSTANCE: AST | None = None


def _label_list(id2label: dict) -> list[str]:
    """Class names in class-index order."""
    return [str(id2label[k]) for k in range(len(id2label))]


class AST(TagScoring):
    """A loaded AudioSet tagger, bound to one model snapshot and one cache folder."""

    def __init__(self, model_id: str | None = None) -> None:
        torch = _common.torch_ready("AudioSet tagger")
        from transformers import ASTFeatureExtractor, ASTForAudioClassification

        config.ensure_dirs()
        self.model_id, self.revision = _common.wanted(model_id, config.AST_MODEL, config.AST_REVISION)
        self.spec = spec = hub.ensure(self.model_id, self.revision)
        self.weights, self.dir = spec.weights, spec.snapshot
        self.extractor = _common.load_pretrained(ASTFeatureExtractor, spec)
        raw = _common.load_pretrained(ASTForAudioClassification, spec, **_common.load_options(spec))
        self.model, self.precision = _common.choose_precision(torch, raw.eval())
        self.dtype = torch.bfloat16 if self.precision == "bf16" else torch.float32
        self.labels = _label_list(self.model.config.id2label)
        self.index = dict(zip(self.labels, range(len(self.labels))))
        self.cache_dir = _common.embed_store("ast", spec, self.precision)
        self._torch = torch


def get(model_id: str | None = None) -> AST:
    """Shared tagger for the process; built on first call or when the model id changes."""
    global _INSTANCE
    _common.require_ml("AudioSet tagger")
    target = model_id or config.AST_MODEL
    with _LOCK:
        if _INSTANCE is None or _INSTANCE.model_id != target:
            _INSTANCE = AST(model_id)
        return _INSTANCE
