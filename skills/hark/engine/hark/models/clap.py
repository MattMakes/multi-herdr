"""The CLAP text-audio model on CPU: loading and the shared instance.

The embedding code and its caches are in ``hark.models._clap_embedding``.
"""

from __future__ import annotations

from typing import Any

from hark.core import config
from hark.models import _common, hub
from hark.models._clap_embedding import (  # noqa: F401  (re-exported names)
    MODEL_LOCK,
    WIN,
    ClapEmbedding,
    _group_means,
    _prepare,
    _segments,
)

FALLBACK_LOGIT_SCALE = 100.0

_LOCK = MODEL_LOCK
_INSTANCE: Clap | None = None


def native_bf16(torch: Any) -> bool:
    """True only when the CPU has native bf16 matrix instructions."""
    return _common.native_bf16(torch)


class Clap(ClapEmbedding):
    """A loaded CLAP model, bound to one snapshot and one cache folder."""

    def __init__(self, model_id: str | None = None) -> None:
        torch = _common.torch_ready("CLAP")
        from transformers import ClapModel, ClapProcessor

        config.ensure_dirs()
        self.model_id, self.revision = _common.wanted(model_id, config.CLAP_MODEL, config.CLAP_REVISION)
        spec = hub.ensure(self.model_id, self.revision)
        self.spec = spec
        self.weights = spec.weights
        self.dir = spec.snapshot
        self.processor = _common.load_pretrained(ClapProcessor, spec)
        raw = _common.load_pretrained(ClapModel, spec, **_common.load_options(spec))
        self.model, self.precision = _common.choose_precision(torch, raw.eval())
        self.dtype = torch.bfloat16 if self.precision == "bf16" else torch.float32
        try:
            self.logit_scale = float(self.model.logit_scale_a.exp().item())
        except Exception:  # noqa: BLE001 - without the parameter, use the fixed fallback
            self.logit_scale = FALLBACK_LOGIT_SCALE
        self.cache_dir = _common.embed_store("clap", spec, self.precision)
        self._torch = torch
        self._text = self._load_phrases()


def get(model_id: str | None = None) -> Clap:
    """Shared CLAP model for the process; built on first call or when the model id changes."""
    global _INSTANCE
    _common.require_ml("CLAP")
    target = model_id or config.CLAP_MODEL
    with _LOCK:
        if _INSTANCE is None or _INSTANCE.model_id != target:
            _INSTANCE = Clap(model_id)
        return _INSTANCE
