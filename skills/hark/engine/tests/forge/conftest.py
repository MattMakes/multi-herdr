"""Forge test helpers: the test recipe, stub listen models and fixtures."""

from __future__ import annotations

import math
import sys
import types
from pathlib import Path

import numpy as np

SR = 48000


# ---------------------------------------------------------------- test recipe

TINY = "forgetest_tiny"


def _load_tiny():
    import importlib.util

    from hark.forge import recipes as registry

    parent = "hark.forge.recipes.ui"
    if parent not in sys.modules:
        try:
            importlib.import_module(parent)
        except ModuleNotFoundError:
            pkg = types.ModuleType(parent)
            pkg.__path__ = []
            sys.modules[parent] = pkg
    full = f"{parent}.{TINY}"
    if full not in sys.modules:
        spec = importlib.util.spec_from_file_location(full, Path(__file__).with_name("tiny_recipe.py"))
        mod = importlib.util.module_from_spec(spec)
        sys.modules[full] = mod
        spec.loader.exec_module(mod)
        setattr(sys.modules[parent], TINY, mod)
    return registry, sys.modules[full]



import pytest  # noqa: E402


@pytest.fixture
def tiny(monkeypatch):
    """The tiny test recipe, registered in the recipe registry as forgetest_tiny."""
    registry, mod = _load_tiny()
    real = registry._table
    monkeypatch.setattr(registry, "_table", lambda: {**real(), TINY: "ui"})
    return mod


# ---------------------------------------------------------------- model-free test critic

class ToyCritic:
    """A model-free critic: prefers a spectral centroid near 1500 Hz."""

    def __init__(self, flat: bool = False, refs=None):
        self.flat, self.refs, self.calls = flat, list(refs or []), 0
        self.tone_target, self.polish_ref, self.study = None, None, {}
        self.target, self.alts = "a short bright ping", []

    def _row(self, x):
        m = np.asarray(x, dtype=float)
        m = m.mean(axis=1) if m.ndim == 2 else m
        P = np.abs(np.fft.rfft(m)) ** 2
        f = np.fft.rfftfreq(len(m), 1 / SR)
        c = float((P * f).sum() / (P.sum() + 1e-30))
        total = 1.0 if self.flat else 3.0 - abs(math.log2(c / 1500.0))
        return {"total": total, "passes": total > 2.0, "match_z": total, "p_real": 0.9, "ref_typ_z": None,
                "feat_pen": 0.0, "summary": f"centroid {c:.0f} Hz"}

    def score(self, xs, E=None, exclude=None):
        self.calls += 1
        return [dict(self._row(x), i=i) for i, x in enumerate(xs)]


@pytest.fixture
def toy_critic():
    """Factory of model-free critics: toy_critic(flat=False, refs=None)."""
    return ToyCritic


# ---------------------------------------------------------------- stub listen services

class StubServices:
    """Deterministic stand-ins for the listen models (no ML). Counts embeddings."""

    def __init__(self):
        self.embeds = 0
        self.tag_calls = 0

    @staticmethod
    def _vec(x):
        from hark.forge.engine import tone

        p = 10 ** (tone.profile(x) / 20)
        return p / (np.linalg.norm(p) + 1e-12)

    def embed_text(self, texts):
        out = []
        for t in texts:
            v = np.random.default_rng(sum(map(ord, t))).random(28)
            out.append(v / np.linalg.norm(v))
        return np.array(out)

    def calibrate_z(self, E, Q):
        return 3.5 + 10 * (np.asarray(E) @ np.asarray(Q).T - 0.7)

    def top_terms(self, e, k=3):
        return ["ping", "tone", "chime"][:k]

    def embed_files(self, paths):
        from hark.core.audio import load

        self.embeds += 1
        return np.array([self._vec(load(p)) for p in paths])

    def embed_audio(self, arrays):
        self.embeds += 1
        return np.array([self._vec(a) for a in arrays])

    def audible(self, m):
        return float(np.max(np.abs(m))) > 1e-4

    def pure_tone(self, m):
        return False

    def p_real(self, m, e):
        return 0.9

    def feats(self, x):
        m = np.asarray(x, dtype=float).mean(axis=1)
        return {"length_s": len(m) / SR, "attack_ms": 2.0 + float(np.argmax(np.abs(m))) / 48}

    def typicality(self, e, A, k):
        c = np.sort(np.asarray(A) @ e)[::-1][:k]
        return float((c.mean() - 0.9) * 20)

    def tagger_name(self):
        return "stub-tagger"

    def tags(self, m):
        self.tag_calls += 1
        return [("Ping", 0.8), ("Chime", 0.1), ("Tone", 0.05)], 0.02


@pytest.fixture
def stub_listen(monkeypatch):
    """Install stub listen services in the critic; returns the stub."""
    from hark.forge.engine import critic

    stub = StubServices()
    monkeypatch.setattr(critic, "services", lambda: stub)
    return stub
