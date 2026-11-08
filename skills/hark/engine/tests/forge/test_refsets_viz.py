"""Acceptance tests RS1-RS5 and V1-V3 of forge-core part 9."""

import json

import numpy as np
import pytest

from hark.forge.engine import refsets, viz

SR = 48000


def _touch(p):
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_bytes(b"x")
    return p


def test_rs1_scan(tmp_path):
    _touch(tmp_path / "click/forge/a.wav")
    _touch(tmp_path / "click/bench/b.WAV")
    _touch(tmp_path / "_x/forge/c.wav")
    reg = refsets.scan(tmp_path)
    assert list(reg["categories"]) == ["click"]
    c = reg["categories"]["click"]
    assert len(c["forge"]) == 1 and len(c["bench"]) == 1 and c["note"] == ""


def test_rs2_write_refuses_both_splits(tmp_path):
    reg = {"version": 1, "categories": {"click": {"forge": ["/a.wav"], "bench": ["/a.wav"], "note": ""}}}
    with pytest.raises(ValueError):
        refsets.write(reg, tmp_path / "r.json")
    assert not (tmp_path / "r.json").exists()


def _registry(tmp_path):
    f = _touch(tmp_path / "refs/click/forge/a.wav")
    b = _touch(tmp_path / "refs/click/bench/b.wav")
    path = refsets.write(refsets.scan(tmp_path / "refs"), tmp_path / "refsets.json")
    return path, f, b


def test_rs3_refset_errors(tmp_path):
    path, f, _ = _registry(tmp_path)
    assert refsets.refset("click", path=path) == [str(f).replace("\\", "/")]
    with pytest.raises(ValueError):
        refsets.refset("click", "train", path=path)
    with pytest.raises(KeyError):
        refsets.refset("nope", path=path)
    f.unlink()
    with pytest.raises(refsets.RefsetsMissing):
        refsets.refset("click", path=path)


def test_rs4_held_out(tmp_path, monkeypatch):
    path, f, b = _registry(tmp_path)
    monkeypatch.setenv("HARK_REFSETS", str(path))
    assert refsets.held_out([str(b), str(f)]) == [str(b)]


def test_rs5_missing_registry(tmp_path, monkeypatch):
    monkeypatch.setenv("HARK_HOME", str(tmp_path))
    monkeypatch.delenv("HARK_REFSETS", raising=False)
    with pytest.raises(refsets.RefsetsMissing) as e:
        refsets.load()
    assert "<root>/<category>/{forge,bench}" in str(e.value)


def test_check_split_sources():
    reg = {"categories": {"a": {"forge": ["/x.wav"], "bench": []}, "b": {"forge": [], "bench": ["/y.wav"]}}}
    probs = refsets.check_split(reg, {"/x.wav": "s1", "/y.wav": "s1"})
    assert probs == ["source s1 is forge in ['a'] and bench in ['b']"]


def _ping(f, sec=0.4, lead=0.0):
    t = np.arange(int(sec * SR)) / SR
    y = np.sin(2 * np.pi * f * t) * np.exp(-t / 0.08)
    return np.concatenate([np.zeros(int(lead * SR)), y])


def test_v1_png_with_ref(tmp_path):
    out = viz.compare_png(tmp_path / "c.png", [("m", _ping(800))], [("r", _ping(900))])
    assert (tmp_path / "c.png").stat().st_size > 0
    assert out["rows"] == 1 and out["pairs"] == ["r"]


def test_v2_png_without_refs(tmp_path):
    out = viz.compare_png(tmp_path / "c.png", [("m", _ping(800))])
    assert (tmp_path / "c.png").is_file() and out["pairs"] == [None]


def test_v3_onset_align():
    x = np.zeros(int(0.1 * SR) + 100)
    x[int(0.1 * SR)] = 1.0
    y = viz.onset_align(x)
    assert len(y) == len(x) - int(0.1 * SR) + 240
    assert y[240] == 1.0
