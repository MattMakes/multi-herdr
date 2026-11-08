"""hark doctor: packages, tools, models, bank, problems, notes, summary (core-io 2.7, 3.9)."""

from __future__ import annotations

import json
import sys
import types

import pytest

from hark import doctor
from hark.models import MLMissing

CLAP = "laion/larger_clap_general"
AST = "MIT/ast-finetuned-audioset-10-10-0.4593"


@pytest.fixture
def clap_cached(cfg, snapshot, pin_files):
    return snapshot(CLAP, cfg.PINS[CLAP]["revision"], pin_files(CLAP))


@pytest.fixture
def fake_bank(monkeypatch, tmp_path):
    """Put a fake hark.forge.bank.bank in sys.modules for one test (doctor imports it by name)."""

    def install(ids=(), version="t1", broken=None, files=()):
        mod = types.ModuleType("hark.forge.bank.bank")
        mod.__file__ = str(tmp_path / "bank" / "bank.py")
        mod.VERSION = version
        entries = {i: ({"type": "fit"} if i.startswith("fit/") else {}) for i in ids}

        def _ids():
            if broken:
                raise ValueError(broken)
            return sorted(entries)

        mod.ids = _ids
        mod.get = lambda i: dict(entries[i])
        mod.bank_files = lambda: tuple(files)
        monkeypatch.setitem(sys.modules, "hark.forge.bank.bank", mod)
        return mod

    return install


def test_at_25_cached_and_missing_models(cfg, clap_cached, run_hark):
    r = doctor.check()
    assert r["models"][CLAP]["cached"] == "cached" and r["models"][AST]["cached"] == "missing"
    assert r["models"][CLAP]["loads"] == cfg.PINS[CLAP]["revision"]
    assert r["models_cached"] == {CLAP: True, AST: False}
    assert any("not cached yet" in n for n in r["notes"])
    assert not any(AST in p or CLAP in p for p in r["problems"])
    assert run_hark("doctor", "--json").returncode == 0


def test_at_25_offline_problem(cfg, clap_cached, monkeypatch):
    monkeypatch.setenv("HARK_OFFLINE", "1")
    r = doctor.check()
    assert any("HARK_OFFLINE" in p and AST in p for p in r["problems"])
    assert not any("not cached yet" in n for n in r["notes"])


def test_at_26_load_failure_is_a_problem(cfg, clap_cached, monkeypatch):
    def boom():
        raise RuntimeError("boom")

    monkeypatch.setattr(doctor, "_load_models", boom)
    r = doctor.check(load=True)
    assert "model load failed: boom" in r["problems"]


def test_at_27_fetch_missing(cfg, clap_cached, fake_download):
    r = doctor.check(fetch_missing=True)
    assert r["fetched"][CLAP] == "already cached"
    assert r["fetched"][AST] == "downloaded " + cfg.PINS[AST]["revision"][:7]
    assert len(fake_download) == 1
    assert r["models_cached"] == {CLAP: True, AST: True}


def test_e_d6_fetch_offline(cfg, monkeypatch, fake_download):
    monkeypatch.setenv("HF_HUB_OFFLINE", "1")
    r = doctor.check(fetch_missing=True)
    assert r["fetched"][AST].startswith("failed: ") and "HF_HUB_OFFLINE" in r["fetched"][AST]
    assert any(p.startswith(f"{AST}: failed:") for p in r["problems"])
    assert fake_download == []


def test_e_d4_load_offline_uncached_no_attempt(cfg, monkeypatch):
    monkeypatch.setenv("HARK_OFFLINE", "1")
    monkeypatch.setattr(doctor, "_load_models", lambda: pytest.fail("must not load"))
    r = doctor.check(load=True)
    assert any("HARK_OFFLINE" in p for p in r["problems"])


def test_e_d7_load_without_torch(cfg, clap_cached, monkeypatch):
    from hark import models

    monkeypatch.setattr(models, "_absent", lambda names: tuple(names))
    r = doctor.check(load=True)
    hits = [p for p in r["problems"] if p.startswith("model load failed:")]
    assert hits and models.install_command("ml") in hits[0]


def test_q2_ml_absent_is_a_note(cfg, monkeypatch):
    from hark import models

    monkeypatch.setattr(models, "_absent", lambda names: tuple(names))
    monkeypatch.setattr(doctor, "missing_ml", lambda: ("torch", "transformers"))
    r = doctor.check()
    assert any(models.install_command("ml") in n for n in r["notes"])
    assert not any("torch" in p for p in r["problems"])


def test_packages_without_import(cfg, monkeypatch):
    pkgs = doctor.packages()
    assert set(pkgs) == set(doctor.PACKAGES)
    assert pkgs["numpy"][0].isdigit()
    monkeypatch.setitem(doctor.PACKAGES, "no_such_pkg_hark", "no-such")
    assert doctor.packages()["no_such_pkg_hark"] == "MISSING (ModuleNotFoundError)"


def test_e_d3_tools_missing(cfg, monkeypatch):
    real = doctor.shutil.which
    monkeypatch.setattr(doctor.shutil, "which", lambda n: None if n == "ffprobe" else real(n))
    r = doctor.check()
    assert r["ffprobe"].startswith("MISSING")
    assert "ffprobe missing" in r["problems"]


def test_basic_fields(cfg):
    r = doctor.check()
    for key in ("python", "hark_home", "threads", "precision", "clap_model", "ast_model", "packages", "ffmpeg",
                "ffprobe", "offline", "models", "models_cached", "agy", "data_files", "bank", "problems",
                "notes", "summary"):
        assert key in r
    assert r["torch_cuda_used"] is False and r["hark_home"] == cfg.HOME
    assert set(r["data_files"]) == set(doctor.DATA_FILES)
    json.dumps(r)


def test_e_d1_bank_line_then_problems(cfg, fake_bank, monkeypatch, tmp_path):
    own = tmp_path / "bank" / "data"
    own.mkdir(parents=True)
    (own / "forge_v2.json").write_text(json.dumps({"entries": {"a/1": {}, "pluck/x": {}}}))
    extra = tmp_path / "mine.json"
    extra.write_text(json.dumps({"entries": {"fit/y": {}}}))
    monkeypatch.setenv("HARK_BANK", str(extra))
    fake_bank(ids=("a/1", "pluck/x", "fit/y"), files=(str(own / "forge_v2.json"), str(extra)))
    monkeypatch.setattr(doctor, "_data_files", lambda: {"listen/vocab.json": False})
    r = doctor.check()
    assert r["bank"]["sounds"] == 3 and r["bank"]["types"] == {"forge": 2, "fit": 1}
    assert r["bank"]["pluck"] == 1 and r["bank"]["extra"] == [str(extra)]
    assert r["bank"]["files"] == [{"file": "forge_v2.json", "entries": 2}, {"file": str(extra), "entries": 1}]
    assert r["summary"].startswith("Sound bank t1: 3 sounds (1 forge, 1 plucks, 1 fitted from CC0 recordings; "
                                   f"extra files from HARK_BANK: {extra}). Problems: ")
    assert "data file hark/data/listen/vocab.json missing" in r["problems"]


def test_e_d2_broken_bank(cfg, fake_bank):
    fake_bank(broken="bad file")
    r = doctor.check()
    assert r["bank"] == {"error": "ValueError: bad file"}
    assert "sound bank: ValueError: bad file" in r["problems"]
    assert "models" in r


def test_healthy_summary(cfg, fake_bank, clap_cached, snapshot, pin_files, monkeypatch):
    snapshot(AST, cfg.PINS[AST]["revision"], pin_files(AST))
    fake_bank(ids=("a/1",))
    monkeypatch.setattr(doctor, "_data_files", lambda: {"listen/vocab.json": True})
    monkeypatch.setattr(doctor, "_package_problems", lambda pkgs: [])
    monkeypatch.setattr(doctor, "_tool_problems", lambda tools: [])
    r = doctor.check()
    assert r["problems"] == []
    assert "hark looks healthy." in r["summary"]


def test_local_folder_incomplete_problem(cfg, monkeypatch, tmp_path):
    folder = tmp_path / "mine"
    folder.mkdir()
    (folder / "config.json").write_text("{}")
    monkeypatch.setenv("HARK_CLAP", str(folder))
    cfg.refresh()
    r = doctor.check()
    assert (f"{folder}: the local model folder lacks model.safetensors (hark never downloads into a folder)"
            in r["problems"])


def test_load_step_records_timings(cfg, monkeypatch):
    monkeypatch.setattr(doctor, "_load_models", lambda: {"clap_load_s": 1.0, "native_bf16": False})
    r = doctor.check(load=True)
    assert r["clap_load_s"] == 1.0 and r["native_bf16"] is False


def test_mlmissing_message_shape():
    exc = MLMissing("CLAP", ("torch",), "pip install x")
    assert str(exc) == "CLAP needs the ml extras. Install them with: pip install x"
