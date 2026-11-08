"""How long the CLAP and AudioSet models take to load and to score one window."""

from __future__ import annotations

import time
from typing import Any


def _stopwatch(action) -> tuple[Any, float]:
    began = time.perf_counter()
    value = action()
    return value, time.perf_counter() - began


def time_both() -> dict:
    """Load both models, time the load and one silent window each; MLMissing or RuntimeError propagate."""
    import numpy as np

    from hark.models import ast, clap

    silence = [np.zeros(48000, dtype=np.float32)]
    tagger_clap, clap_load = _stopwatch(clap.get)
    _, clap_window = _stopwatch(lambda: tagger_clap.embed_audio(silence))
    timings: dict[str, Any] = {"clap_load_s": round(clap_load, 1), "clap_one_window_s": round(clap_window, 2),
                               "clap_precision": tagger_clap.precision,
                               "clap_revision": tagger_clap.spec.revision, "clap_weights": tagger_clap.weights}
    tagger_ast, ast_load = _stopwatch(ast.get)
    _, ast_window = _stopwatch(lambda: tagger_ast.probs(silence))
    timings.update({"ast_load_s": round(ast_load, 1), "ast_one_window_s": round(ast_window, 2),
                    "ast_revision": tagger_ast.spec.revision, "ast_weights": tagger_ast.weights})
    import torch

    timings["native_bf16"] = clap.native_bf16(torch)
    return timings
