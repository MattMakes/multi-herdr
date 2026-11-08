"""CLAP and AST wrappers: windows, padding, caches, precision, the ML gate (core-io 3.8, 4.6-4.8, 4.12).

Tests without the ml marker run without torch. AT-35 .. AT-38 need the real models (marked ml).
"""

from __future__ import annotations

import sys
import types

import numpy as np
import pytest

from hark.models import MLMissing, _common, ast, clap

SR = 48000


@pytest.fixture
def no_ml(monkeypatch):
    from hark import models

    monkeypatch.setattr(models, "_absent", lambda names: tuple(names))


def test_get_without_ml_raises_before_loading(no_ml):
    with pytest.raises(MLMissing) as err:
        clap.get()
    assert err.value.feature == "CLAP"
    with pytest.raises(MLMissing) as err:
        ast.get()
    assert err.value.feature == "AudioSet tagger"


def test_e_m13_clap_window_count():
    assert len(clap._segments(np.zeros(480_000, np.float32))) == 1
    assert len(clap._segments(np.zeros(480_001, np.float32))) == 2


def test_at_35_clap_window_starts_without_model():
    x = np.zeros(25 * SR, np.float32)
    starts = [t for t, _ in __import__("hark.core.audio", fromlist=["windows"]).windows(x, 10.0, 5.0, SR, True)]
    assert starts == [0.0, 5.0, 10.0, 15.0]
    assert all(len(s) == clap.WIN for s in clap._segments(x))


def test_e_m12_short_input_is_padded():
    seg = clap._prepare(np.ones(10, np.float32), "repeatpad")
    assert len(seg) == 480 and seg[10:].sum() == 0
    assert len(clap._prepare(np.ones(1000, np.float32), "zero")) == clap.WIN
    assert len(clap._prepare(np.ones(1000, np.float32), "repeatpad")) == 1000


def test_stereo_input_is_made_mono():
    x = np.column_stack([np.ones(1000), -np.ones(1000)]).astype(np.float32)
    assert np.allclose(clap._segments(x)[0], 0.0)


def test_at_38_ast_window_starts_without_model():
    n = 30 * 16000
    starts = [s / 16000 for s in ast.window_starts(n)]
    assert starts == [0.0, 5.12, 10.24, 15.36, 19.76]
    assert ast.window_starts(1000) == [0]
    assert ast.window_starts(ast.WIN + ast.HOP) == [0, ast.HOP]


def test_ast_segments_pad_to_minimum():
    segs = ast._segments(np.zeros(100, np.float32))
    assert len(segs) == 1 and len(segs[0]) == ast.MIN_SAMPLES


def test_group_means_are_unit_rows():
    rows = _common.unit_rows(np.random.default_rng(0).standard_normal((5, 512)))
    out = clap._group_means(rows, [2, 3])
    assert out.shape == (2, 512)
    assert np.allclose(np.linalg.norm(out, axis=1), 1.0, atol=1e-5)
    assert clap._group_means(rows[:0], []).shape == (0, 512)


def test_cache_keys():
    assert len(_common.key24("a")) == 24
    assert _common.wanted(None, "cfg/model", "rev") == ("cfg/model", "rev")
    assert _common.wanted("other/model", "cfg/model", "rev") == ("other/model", None)


def test_file_key_depends_on_range(tmp_path):
    path = tmp_path / "x.wav"
    path.write_bytes(b"data")
    keys = {_common.file_key(str(path), None, None, "repeatpad"), _common.file_key(str(path), 0.5, None, "repeatpad"),
            _common.file_key(str(path), None, None, "zero")}
    assert len(keys) == 3


def _fake_torch(cap="AVX512", avx512_bf16=False, amx=False, fail_probe=False):
    def probe(value):
        def run():
            if fail_probe:
                raise RuntimeError("probe")
            return value

        return run

    cpu = types.SimpleNamespace(_is_avx512_bf16_supported=probe(avx512_bf16), _is_amx_tile_supported=probe(amx))
    backends = types.SimpleNamespace(cpu=types.SimpleNamespace(get_cpu_capability=lambda: cap))
    return types.SimpleNamespace(cpu=cpu, backends=backends)


@pytest.mark.parametrize("torch, onednn, expected", [
    (_fake_torch(cap="AVX2", avx512_bf16=True), "", False),
    (_fake_torch(cap="AVX512", avx512_bf16=True), "AVX2", False),
    (_fake_torch(cap="AVX512", avx512_bf16=True), "", True),
    (_fake_torch(cap="AVX512", amx=True), "", True),
    (_fake_torch(cap="AVX512"), "", False),
    (_fake_torch(cap="AVX512", fail_probe=True), "", False),
    (types.SimpleNamespace(cpu=types.SimpleNamespace(), backends=None), "", False),
])
def test_native_bf16(monkeypatch, torch, onednn, expected):
    monkeypatch.setenv("ONEDNN_MAX_CPU_ISA", onednn)
    assert clap.native_bf16(torch) is expected


class _FakeModel:
    def __init__(self, fail=False):
        self.fail, self.dtype = fail, "fp32"

    def to(self, dtype):
        if self.fail:
            raise RuntimeError("no bf16")
        self.dtype = "bf16"
        return self

    def float(self):
        self.dtype = "fp32"
        return self


def test_choose_precision(cfg, monkeypatch):
    import contextlib

    torch = _fake_torch(cap="AVX512", amx=True)
    torch.bfloat16 = "bf16"
    torch.inference_mode = contextlib.nullcontext
    torch.ones = lambda shape, dtype=None: np.ones(shape)
    model, precision = _common.choose_precision(torch, _FakeModel())
    assert precision == "bf16" and model.dtype == "bf16"
    model, precision = _common.choose_precision(torch, _FakeModel(fail=True))
    assert precision == "fp32" and model.dtype == "fp32"
    monkeypatch.setattr(cfg, "PRECISION", "fp32")
    assert _common.choose_precision(torch, _FakeModel())[1] == "fp32"


def test_load_error_advice(cfg):
    from hark.models import hub

    cached = hub.Spec("org/m", "main", "cached", "/cache/snap", "model.safetensors")
    folder = hub.Spec("/my/folder", "local", "cached", "/my/folder", "model.safetensors", folder=True)
    assert "delete that folder" in str(_common.load_error(cached, "ClapModel", OSError("bad")))
    text = str(_common.load_error(folder, "ClapModel", OSError("bad")))
    assert "delete" not in text and "ClapModel failed: OSError: bad" in text


def test_npy_cache_roundtrip(tmp_path):
    target = str(tmp_path / "a_x.npy")
    assert _common.read_npy(target) is None
    _common.write_npy(target, np.arange(3))
    assert np.array_equal(_common.read_npy(target), np.arange(3, dtype=np.float32))
    (tmp_path / "bad.npy").write_bytes(b"junk")
    assert _common.read_npy(str(tmp_path / "bad.npy")) is None


def test_models_init_imports_only_stdlib():
    import ast as pyast
    from pathlib import Path

    import hark.models as pkg

    tree = pyast.parse(Path(pkg.__file__).read_text(encoding="utf-8"))
    names = {a.name.split(".")[0] for n in pyast.walk(tree) if isinstance(n, pyast.Import) for a in n.names}
    names |= {n.module.split(".")[0] for n in pyast.walk(tree) if isinstance(n, pyast.ImportFrom) and n.module}
    assert names <= set(sys.stdlib_module_names) | {"__future__"}


# ---------------------------------------------------------------- real models (ML tier, pre-filled cache)


# The user's offline flag and hub cache, read before the autouse cfg fixture clears them.
_REAL_OFFLINE = __import__("hark.core.config", fromlist=["offline"]).offline()
_REAL_HF_CACHE = __import__("hark.core.config", fromlist=["HF_CACHE"]).HF_CACHE


@pytest.fixture
def real_models(cfg, monkeypatch):
    """The user's pre-filled hub cache, offline; embedding caches stay in tmp_path."""
    if _REAL_OFFLINE is None:
        pytest.skip("needs-data: a pre-filled model cache with an offline flag (no test downloads)")
    monkeypatch.setenv(_REAL_OFFLINE, "1")
    cfg.HF_CACHE = _REAL_HF_CACHE
    monkeypatch.setattr(clap, "_INSTANCE", None)
    monkeypatch.setattr(ast, "_INSTANCE", None)


@pytest.fixture
def real_clap(real_models):
    return clap.get()


@pytest.mark.ml
def test_at_35_clap_per_window(real_clap):
    x = 0.1 * np.sin(np.arange(25 * SR) / 20.0).astype(np.float32)
    per = real_clap.embed_audio(x, per_window=True)
    assert per[0].shape == (4, 512)
    mean = real_clap.embed_audio(x)
    assert abs(np.linalg.norm(mean[0]) - 1.0) < 1e-5


@pytest.mark.ml
def test_at_36_embed_file_cache(real_clap, tmp_path, monkeypatch):
    import soundfile as sf

    path = tmp_path / "s.wav"
    sf.write(str(path), (0.2 * np.sin(np.arange(SR) / 9.0)).astype(np.float32), SR)
    first = real_clap.embed_file(str(path))
    monkeypatch.setattr(real_clap, "embed_audio", lambda *a, **k: pytest.fail("model called"))
    assert np.array_equal(real_clap.embed_file(str(path)), first)


@pytest.mark.ml
def test_at_37_embed_text_dedup(real_clap):
    import os

    rows = real_clap.embed_text(["hark test a", "hark test b", "hark test a"])
    assert rows.shape == (3, 512) and np.array_equal(rows[0], rows[2])
    with np.load(os.path.join(real_clap.cache_dir, "text.npz")) as data:
        assert {_common.key24("hark test a"), _common.key24("hark test b")} <= set(data.files)


@pytest.mark.ml
def test_at_38_ast_probs(real_models):
    tagger = ast.get()
    p = tagger.probs(0.1 * np.sin(np.arange(30 * SR) / 20.0).astype(np.float32))[0]
    assert p.shape == (527,) and p.min() >= 0 and p.max() <= 1
    top = tagger.top(p, 5)
    assert [v for _, v in top] == sorted((v for _, v in top), reverse=True)
