"""Forge test helpers.

The forge engine calls a few core functions (load, write_wav, k_weight, report
helpers). When the core unit is not built yet, this file installs small
stand-ins for exactly those names. It installs nothing when the real modules
exist, so the integration run uses the real core.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import math
import sys
import types
from pathlib import Path

import numpy as np
from scipy import signal

SR = 48000


def _absent(name: str) -> bool:
    try:
        return importlib.util.find_spec(name) is None
    except ModuleNotFoundError:
        return True


def _audio_stand_in() -> types.ModuleType:
    import soundfile as sf

    mod = types.ModuleType("hark.core.audio")
    mod.SR = SR
    mod.Audio = np.ndarray

    class AudioError(RuntimeError):
        pass

    def to_mono(x):
        x = np.asarray(x)
        return x.mean(axis=1) if x.ndim == 2 else x

    def load(path, sr=SR, mono=False, start=None, end=None, cache=True):
        if not Path(path).is_file():
            raise AudioError(f"no such file: {path}")
        x, rate = sf.read(str(path), dtype="float32", always_2d=True)
        if rate != sr:
            g = math.gcd(rate, sr)
            x = signal.resample_poly(x, sr // g, rate // g, axis=0).astype(np.float32)
        i0 = int(round((start or 0) * sr))
        i1 = len(x) if end is None else int(round(end * sr))
        x = x[i0:i1]
        if x.shape[1] == 1:
            x = np.column_stack([x[:, 0], x[:, 0]])
        return to_mono(x).astype(np.float32) if mono else x[:, :2].astype(np.float32)

    def write_wav(path, x, sr=SR, subtype="PCM_24"):
        p = Path(path)
        p.parent.mkdir(parents=True, exist_ok=True)
        sf.write(str(p), np.asarray(x, dtype=np.float64), sr, subtype=subtype)
        return str(p)

    def list_media(paths, recursive=True, exts=None):
        exts = exts or mod.MEDIA_EXTS
        out = []
        for p in [paths] if isinstance(paths, (str, Path)) else paths:
            p = Path(p)
            if p.is_dir():
                out += sorted(str(f) for f in (p.rglob("*") if recursive else p.iterdir())
                              if f.is_file() and f.suffix.lower() in exts)
            elif p.is_file():
                out.append(str(p))
        return out

    mod.AudioError, mod.to_mono, mod.load, mod.write_wav = AudioError, to_mono, load, write_wav
    mod.list_media = list_media
    mod.MEDIA_EXTS = frozenset({".wav", ".flac", ".aif", ".aiff", ".ogg", ".mp3"})
    return mod


_K = np.array([[1.53512485958697, -2.69169618940638, 1.19839281085285, 1.0, -1.69065929318241, 0.73248077421585],
               [1.0, -2.0, 1.0, 1.0, -1.99004745483398, 0.99007225036621]])


def _features_stand_in() -> types.ModuleType:
    mod = types.ModuleType("hark.core.features")

    def k_weight(x, sr=SR):
        return signal.sosfilt(_K, np.asarray(x, dtype=float), axis=0)

    mod.k_weight = k_weight
    return mod


def _clean(obj):
    if isinstance(obj, dict):
        return {str(k): _clean(v) for k, v in obj.items()}
    if isinstance(obj, (list, tuple)):
        return [_clean(v) for v in obj]
    if isinstance(obj, np.ndarray):
        return _clean(obj.tolist())
    if isinstance(obj, (np.floating, float)):
        v = float(obj)
        return None if not math.isfinite(v) else v
    if isinstance(obj, np.integer):
        return int(obj)
    if obj is None or isinstance(obj, (bool, int, str)):
        return obj
    return str(obj)


def _report_stand_in() -> types.ModuleType:
    mod = types.ModuleType("hark.core.report")

    def add_common(parser: argparse.ArgumentParser) -> None:
        parser.add_argument("--json", action="store_true")
        parser.add_argument("--out", default=None)
        parser.add_argument("--threads", type=int, default=0)

    def to_json(result, indent=1):
        return json.dumps(_clean(result), indent=indent)

    def emit(result, args, text=None):
        if getattr(args, "out", None):
            Path(args.out).parent.mkdir(parents=True, exist_ok=True)
            Path(args.out).write_text(to_json(result))
        print(to_json(result) if getattr(args, "json", False) else (text or result.get("text") or
                                                                    result.get("summary", "")))
        return result

    mod.add_common, mod.to_json, mod.emit, mod.clean = add_common, to_json, emit, _clean
    return mod


def _install() -> None:
    import hark

    if _absent("hark.core"):
        pkg = types.ModuleType("hark.core")
        pkg.__path__ = []
        sys.modules["hark.core"] = pkg
        hark.core = pkg
    makers = {"audio": _audio_stand_in, "features": _features_stand_in, "report": _report_stand_in}
    for name, make in makers.items():
        full = f"hark.core.{name}"
        if full not in sys.modules and _absent(full):
            mod = make()
            sys.modules[full] = mod
            setattr(sys.modules["hark.core"], name, mod)


_install()


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


import importlib  # noqa: E402

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
