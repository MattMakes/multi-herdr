"""Fixtures of the listen tests: stub models and isolated caches."""

from __future__ import annotations

import argparse
import hashlib
import types

import numpy as np
import pytest

SR = 48000


# ---------------------------------------------------------------- stub models


def _seeded_unit(text: str, dim: int = 512) -> np.ndarray:
    seed = int(hashlib.sha1(text.encode("utf-8")).hexdigest()[:8], 16)
    v = np.random.default_rng(seed).standard_normal(dim)
    return (v / np.linalg.norm(v)).astype(np.float32)


_PROJ = np.random.default_rng(1234).standard_normal((48, 512))


def _audio_vector(x: np.ndarray) -> np.ndarray:
    """A level-free spectral fingerprint projected to 512 dimensions."""
    m = np.asarray(x, np.float64)
    m = m.mean(axis=1) if m.ndim == 2 else m
    spec = np.abs(np.fft.rfft(m, n=1 << 15)) ** 2 + 1e-20
    edges = np.geomspace(1, len(spec) - 1, 49).astype(int)
    bands = np.array([spec[a:max(a + 1, b)].sum() for a, b in zip(edges[:-1], edges[1:])])
    f = np.log10(bands / bands.sum() + 1e-12)
    v = (f - f.mean()) @ _PROJ
    return (v / (np.linalg.norm(v) + 1e-12)).astype(np.float32)


class FakeClap:
    """Stand-in CLAP: hashed text vectors, spectral audio vectors, call counters."""

    model_id = "laion/larger_clap_general"
    precision = "bf16"
    logit_scale = 38.75

    def __init__(self):
        self.text_calls = 0
        self.texts = 0
        self.audio_windows = 0

    def embed_text(self, texts, batch=64):
        texts = [texts] if isinstance(texts, str) else list(texts)
        self.text_calls += 1
        self.texts += len(texts)
        return np.stack([_seeded_unit(t) for t in texts]) if texts else np.zeros((0, 512), np.float32)

    def embed_audio(self, arrays, pad="repeatpad", batch=8, per_window=False):
        arrays = [arrays] if isinstance(arrays, np.ndarray) and arrays.ndim == 1 else list(arrays)
        self.audio_windows += len(arrays)
        return np.stack([_audio_vector(a) for a in arrays])


class FakeAST:
    """Stand-in AudioSet tagger with fixed probabilities."""

    def __init__(self, music: float = 0.02, speech: float = 0.01):
        named = ["Speech", "Music", "Sound effect", "Silence", "Synthesizer", "Sine wave", "Beep, bleep",
                 "Click", "Rain", "White noise"]
        self.labels = named + [f"class {i}" for i in range(527 - len(named))]
        self.index = {lab: i for i, lab in enumerate(self.labels)}
        self.calls = 0
        self.base = np.full(527, 0.001, np.float32)
        self.base[self.index["Music"]] = music
        self.base[self.index["Speech"]] = speech
        self.base[self.index["Sound effect"]] = 0.4
        self.base[self.index["Click"]] = 0.3

    def probs(self, arrays, reduce="max", batch=8):
        arrays = [arrays] if isinstance(arrays, np.ndarray) and arrays.ndim == 1 else list(arrays)
        self.calls += 1
        return np.stack([self.base.copy() for _ in arrays]) if arrays else np.zeros((0, 527), np.float32)

    def probs_file(self, path, start=None, end=None, reduce="max"):
        self.calls += 1
        return self.base.copy()

    def top(self, p, k=10, min_p=0.0):
        order = np.argsort(-np.asarray(p), kind="stable")[:k]
        return [(self.labels[i], round(float(p[i]), 3)) for i in order if p[i] >= min_p]

    def top_file(self, path, k=10, start=None, end=None):
        return self.top(self.probs_file(path, start, end), k)

    def prob_of(self, p, label):
        i = self.index.get(label)
        return 0.0 if i is None else float(p[i])


class Loads:
    """Counts model loads (calls of the patched get functions)."""

    def __init__(self):
        self.clap = 0
        self.ast = 0


@pytest.fixture(autouse=True)
def listen_cache(tmp_path, monkeypatch):
    """Every test gets its own embedding cache and a fresh vocabulary."""
    from hark.core import config
    from hark.listen import vocabulary

    monkeypatch.setattr(config, "EMB_CACHE", str(tmp_path / "emb"), raising=False)
    vocabulary.reset()
    yield tmp_path / "emb"
    vocabulary.reset()


@pytest.fixture
def loads() -> Loads:
    return Loads()


@pytest.fixture
def fake_clap(monkeypatch, loads) -> FakeClap:
    """Replace the CLAP model with the stub; ``loads.clap`` counts get() calls."""
    from hark.models import clap

    model = FakeClap()

    def get(model_id=None):
        loads.clap += 1
        return model

    monkeypatch.setattr(clap, "get", get)
    return model


@pytest.fixture
def fake_ast(monkeypatch, loads) -> FakeAST:
    """Replace the AudioSet tagger with the stub; ``loads.ast`` counts get() calls."""
    from hark.models import ast

    model = FakeAST()

    def get(model_id=None):
        loads.ast += 1
        return model

    monkeypatch.setattr(ast, "get", get)
    return model


@pytest.fixture
def no_models(monkeypatch):
    """Fail the test when any model loads."""
    from hark.models import ast, clap

    def boom(model_id=None):
        raise AssertionError("a model was loaded")

    monkeypatch.setattr(clap, "get", boom)
    monkeypatch.setattr(ast, "get", boom)


@pytest.fixture
def no_require(monkeypatch):
    """Let CLI handlers pass the ML check (the stub models stand in)."""
    import hark.models

    monkeypatch.setattr(hark.models, "require_ml", lambda feature: None)


@pytest.fixture
def run_cli(capsys):
    """run_cli(module, argv) -> (exit code, stdout, stderr) through the module's add_cli."""

    def run(module, argv):
        parser = argparse.ArgumentParser(prog="hark")
        sub = parser.add_subparsers(dest="command")
        module.add_cli(sub)
        args = parser.parse_args(argv)
        code = args.func(args)
        out = capsys.readouterr()
        return (code or 0), out.out, out.err

    return run


@pytest.fixture
def write(tmp_path):
    """write(name, x) -> str path of a 24-bit WAV under tmp_path (float32 for exact zeros kept)."""
    import soundfile as sf

    def make(name, x, subtype="FLOAT"):
        p = tmp_path / name
        p.parent.mkdir(parents=True, exist_ok=True)
        sf.write(str(p), np.asarray(x, np.float32), SR, subtype=subtype)
        return str(p)

    return make


def click_signal(dur=1.0, at=0.2, amp=0.8, decay=0.012, freq=3000.0):
    """One short click: instant attack, exponential decay."""
    x = np.zeros(int(round(dur * SR)))
    t = np.arange(int(0.3 * SR)) / SR
    burst = amp * np.sin(2 * np.pi * freq * t) * np.exp(-t / decay)
    i = int(round(at * SR))
    j = min(len(x), i + len(burst))
    x[i:j] += burst[:j - i]
    return x


def sine_signal(dur=2.0, freq=440.0, dbfs=-12.0):
    t = np.arange(int(round(dur * SR))) / SR
    return 10 ** (dbfs / 20) * np.sin(2 * np.pi * freq * t)


@pytest.fixture
def sig():
    """Signal builders shared by the listen tests."""
    return types.SimpleNamespace(click=click_signal, sine=sine_signal)
