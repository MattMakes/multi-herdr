"""Fixtures of the timeline tests.

Model stubs (CLAP, AST, vocabulary) replace the real models through ``monkeypatch`` in the ``stub_models`` fixture.
"""

from __future__ import annotations

import argparse
import hashlib
import subprocess
import types

import numpy as np
import pytest

SR = 48000


def parser_with(add_cli) -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="hark")
    add_cli(p.add_subparsers(dest="command"))
    return p


# ---------------------------------------------------------------- fixtures



class StubClap:
    """Fixed unit vectors: an excerpt maps to a vector from its RMS level, a text to a vector from its hash."""

    dim = 32

    def _unit(self, seed: int) -> np.ndarray:
        v = np.random.default_rng(seed).normal(size=self.dim)
        return v / np.linalg.norm(v)

    def embed_text(self, texts, batch=64):
        return np.vstack([self._unit(int(hashlib.sha1(t.encode()).hexdigest()[:8], 16)) for t in texts])

    def embed_audio(self, arrays, pad="repeatpad", batch=8, per_window=False):
        self.calls = getattr(self, "calls", 0) + len(arrays)
        out = []
        for a in arrays:
            level = int(np.clip(20 * np.log10(np.sqrt(np.mean(np.square(a))) + 1e-9) + 100, 0, 200)) // 6
            out.append(self._unit(level))
        return np.vstack(out).astype(np.float32)


class StubAST:
    labels = ["Music", "Speech", "Silence", "Clicking", "Noise", "Tap"]
    index = {lab: k for k, lab in enumerate(labels)}

    def probs(self, arrays, reduce="max", batch=8):
        return np.tile(np.array([0.1, 0.02, 0.0, 0.6, 0.3, 0.07], dtype=np.float32), (len(arrays), 1))

    def top(self, p, k=10, min_p=0.0):
        order = np.argsort(-np.asarray(p))
        return [(self.labels[i], round(float(p[i]), 3)) for i in order[:k] if p[i] >= min_p]

    def prob_of(self, p, label):
        return float(p[self.index[label]])


class StubVocab:
    """Two terms per section category; calibrate() gives z from fixed cosines."""

    def __init__(self):
        cats = ("music_genre", "music_mood", "instruments", "ambience", "voice")
        self.terms = [f"{c} term {k}" for c in cats for k in (1, 2)]
        self.term_category = [c for c in cats for _ in (1, 2)]
        self.categories = list(cats)
        self.templates = ["{}"]

    def calibrate(self, A, Q=None, method="both"):
        n, T = len(A), len(self.terms)
        C = np.tile(np.linspace(0.3, 0.1, T), (n, 1))
        Z = np.tile(np.linspace(3.0, 0.5, T), (n, 1))
        return types.SimpleNamespace(C=C, Z=Z, U=C, mu=np.zeros(n), sigma=np.ones(n), Cq=None, z_q=None,
                                     percentile=None, rank=None)


@pytest.fixture
def stub_models(monkeypatch):
    """Replace CLAP, AST and the vocabulary with fixed stubs. Returns the CLAP stub (it counts audio calls).

    The stubs need no ML extras, so the CLI handlers' require_ml check is a no-op here.
    """
    import hark.listen.vocabulary as vocabulary
    import hark.models
    import hark.models.ast as ast_mod
    import hark.models.clap as clap_mod

    monkeypatch.setattr(hark.models, "require_ml", lambda feature: None)
    clap, ast, vocab = StubClap(), StubAST(), StubVocab()
    monkeypatch.setattr(clap_mod, "get", lambda model_id=None: clap)
    monkeypatch.setattr(ast_mod, "get", lambda model_id=None: ast)
    monkeypatch.setattr(vocabulary, "get", lambda: vocab)
    return clap


def make_click_track(times, dur, amp=0.5, bed=0.01, seed=0, length=0.05, tau=0.008):
    """Decaying noise bursts at the given times over a noise bed (analysis tests helper)."""
    r = np.random.default_rng(seed)
    x = bed * r.uniform(-1, 1, int(round(dur * SR)))
    n = int(round(length * SR))
    env = np.exp(-np.arange(n) / SR / tau)
    for t in times:
        i = int(round(t * SR))
        j = min(x.size, i + n)
        x[i:j] += amp * r.uniform(-1, 1, j - i) * env[: j - i]
    return x.astype(np.float32)


@pytest.fixture
def click_track():
    return make_click_track


@pytest.fixture
def cli_parser():
    """parser(add_cli) -> argparse parser with that command registered."""
    return parser_with


def make_mp4(path, dur=3.0, split=1.5, fps=30, audio=None, colours=None, size="96x54"):
    """Write an mp4 with ffmpeg: colour clips (default black then white at ``split``), optional audio array."""
    import soundfile as sf

    clips = colours or [("black", split), ("white", dur - split)]
    cmd = ["ffmpeg", "-nostdin", "-v", "error", "-y"]
    for colour, d in clips:
        cmd += ["-f", "lavfi", "-i", f"color=c={colour}:s={size}:r={fps}:d={d}"]
    n = len(clips)
    filt = "".join(f"[{k}:v]" for k in range(n)) + f"concat=n={n}:v=1:a=0[v]"
    maps = ["-map", "[v]"]
    if audio is not None:
        wav_path = str(path) + ".wav"
        sf.write(wav_path, np.asarray(audio, dtype=np.float32), SR)
        cmd += ["-i", wav_path]
        maps += ["-map", f"{n}:a", "-c:a", "pcm_s16le"] if str(path).endswith(".mov") else \
            ["-map", f"{n}:a", "-c:a", "aac", "-b:a", "192k"]
    cmd += ["-filter_complex", filt, *maps, "-c:v", "libx264", "-pix_fmt", "yuv420p", "-r", str(fps), str(path)]
    subprocess.run(cmd, check=True, capture_output=True)
    return str(path)


@pytest.fixture
def mp4():
    return make_mp4
