"""Hub cache: resolve, fetch, ensure, cache names (core-io 3.7, 4.9, 4.10)."""

from __future__ import annotations

import os
from pathlib import Path

import pytest

from hark.models import hub

CLAP = "laion/larger_clap_general"
AST = "MIT/ast-finetuned-audioset-10-10-0.4593"
LEGACY_CLAP = "ada0c23a36c4e8582805bb38fec3905903f18b41"
COMMIT = "0123456789abcdef0123456789abcdef01234567"


def _pin(cfg, model_id: str) -> str:
    return cfg.PINS[model_id]["revision"]


def test_at_20_pinned_clap_states(cfg, snapshot, pin_files):
    spec = hub.resolve(CLAP)
    assert spec.state == "missing" and spec.snapshot is None
    folder = snapshot(CLAP, _pin(cfg, CLAP), [f for f in pin_files(CLAP) if f != "model.safetensors"])
    spec = hub.resolve(CLAP)
    assert spec.state == "incomplete" and "model.safetensors" in spec.missing
    (folder / "model.safetensors").write_text("w")
    spec = hub.resolve(CLAP)
    assert spec.state == "cached" and spec.snapshot == str(folder) and spec.pinned
    assert spec.cache_name == "laion--larger_clap_general" and hub.cache_name(CLAP) == spec.cache_name
    assert spec.sha == _pin(cfg, CLAP) and spec.label == f"{CLAP}@16c8cc3"


def test_at_21_legacy_snapshot_is_not_used(cfg, snapshot, pin_files):
    """ARCHITECTURE 15, core Q1: hark drops the legacy pickle state for its own cache."""
    files = [f for f in pin_files(CLAP) if f != "model.safetensors"] + ["pytorch_model.bin"]
    snapshot(CLAP, LEGACY_CLAP, files)
    assert hub.resolve(CLAP).state == "missing"
    assert hub.resolve(CLAP, _pin(cfg, CLAP)).state == "missing"


def test_at_22_custom_id_with_refs_main(cfg, snapshot):
    folder = snapshot("org/custom", COMMIT, ["config.json", "model.safetensors"], ref="main")
    spec = hub.resolve("org/custom")
    assert spec.state == "cached" and not spec.pinned and spec.snapshot == str(folder)
    assert spec.cache_tag == "--" + COMMIT[:8]
    assert spec.cache_name == "org--custom--" + COMMIT[:8]


def test_e_m8_branch_override_not_cached(cfg, snapshot, fake_download):
    spec = hub.resolve(CLAP, "dev/branch")
    assert spec.state == "missing" and not spec.pinned and spec.cache_tag == "--dev_bran"


def test_sharded_safetensors_index_counts(cfg, snapshot):
    snapshot("org/sharded", COMMIT, ["config.json", "model.safetensors.index.json"], ref="main")
    assert hub.resolve("org/sharded").weights == "model.safetensors.index.json"


def test_snapshot_dir_nested_pr_ref(cfg, snapshot):
    folder = snapshot("org/m", COMMIT, ["config.json"], ref="refs/pr/2")
    assert hub.snapshot_dir("org/m", "refs/pr/2") == str(folder)
    assert hub.snapshot_dir("org/m", "nope") is None


def test_at_23_offline_ensure_never_downloads(cfg, monkeypatch, fake_download):
    monkeypatch.setenv("HF_HUB_OFFLINE", "1")
    with pytest.raises(RuntimeError, match="HF_HUB_OFFLINE"):
        hub.ensure(CLAP)
    assert fake_download == []


def test_at_23_pinned_and_custom_allow_lists(cfg, fake_download, pin_files):
    spec = hub.ensure(CLAP)
    assert spec.state == "cached"
    assert len(fake_download) == 1
    assert sorted(fake_download[0]["allow_patterns"]) == sorted(pin_files(CLAP))
    assert fake_download[0]["revision"] == _pin(cfg, CLAP)
    hub.ensure("org/custom")
    allow = fake_download[1]["allow_patterns"]
    assert not any("*" in a for a in allow) and not any(a.endswith(".bin") for a in allow)
    assert "model.safetensors" in allow


def test_ensure_returns_cached_without_download(cfg, snapshot, pin_files, fake_download):
    snapshot(AST, _pin(cfg, AST), pin_files(AST))
    assert hub.ensure(AST).state == "cached"
    assert fake_download == []


def test_at_24_local_folder(cfg, monkeypatch, tmp_path, fake_download):
    folder = tmp_path / "my model:1"
    folder.mkdir()
    (folder / "config.json").write_text("{}")
    (folder / "model.safetensors").write_text("w")
    monkeypatch.setenv("HARK_OFFLINE", "1")
    spec = hub.ensure(str(folder))
    assert (spec.state, spec.revision, spec.folder) == ("cached", "local", True)
    assert spec.cache_name.startswith("local--my_model_1--local")
    assert os.sep not in spec.cache_name and ":" not in spec.cache_name
    assert spec.label == f"the local model folder {folder}"
    assert fake_download == []


def test_at_24_e_m5_folder_without_weights_never_downloads(cfg, tmp_path, fake_download):
    folder = tmp_path / "empty_model"
    folder.mkdir()
    (folder / "config.json").write_text("{}")
    spec = hub.resolve(str(folder))
    assert spec.state == "incomplete" and spec.missing == ("model.safetensors",)
    with pytest.raises(RuntimeError, match="never downloads into it"):
        hub.ensure(str(folder))
    assert fake_download == []


def test_local_folder_pickle_weights_load(cfg, tmp_path):
    folder = tmp_path / "old"
    folder.mkdir()
    (folder / "config.json").write_text("{}")
    (folder / "pytorch_model.bin").write_text("w")
    assert hub.resolve(str(folder)).weights == "pytorch_model.bin"


def test_local_folder_cache_tag_follows_weights(cfg, tmp_path):
    folder = tmp_path / "m"
    folder.mkdir()
    (folder / "config.json").write_text("{}")
    (folder / "model.safetensors").write_text("w")
    first = hub.resolve(str(folder)).cache_name
    (folder / "model.safetensors").write_text("new weights")
    os.utime(folder / "model.safetensors", ns=(1, 1))
    assert hub.resolve(str(folder)).cache_name != first


def test_e_m7_folder_named_like_a_pin_is_ignored(cfg, monkeypatch, tmp_path):
    (tmp_path / "laion" / "larger_clap_general").mkdir(parents=True)
    monkeypatch.chdir(tmp_path)
    assert not hub.is_folder(CLAP)
    assert hub.resolve(CLAP).folder is False


def test_e_m3_dangling_weights_symlink(cfg, snapshot, pin_files, fake_download):
    folder = snapshot(AST, _pin(cfg, AST), ["config.json", "preprocessor_config.json"])
    (folder / "model.safetensors").symlink_to(folder / "gone.blob")
    assert hub.resolve(AST).state == "incomplete"
    os.remove(folder / "model.safetensors")
    assert hub.ensure(AST).state == "cached"
    assert len(fake_download) == 1


def test_e_m1_offline_message_names_model_cache_flag(cfg, monkeypatch):
    monkeypatch.setenv("HARK_OFFLINE", "yes")
    with pytest.raises(RuntimeError) as err:
        hub.ensure(AST)
    text = str(err.value)
    assert AST in text and cfg.HF_CACHE in text and "HARK_OFFLINE" in text


def test_e_m9_pickle_only_repo(cfg, monkeypatch, snapshot):
    def download(repo_id, revision, cache_dir, allow_patterns):
        snapshot(repo_id, COMMIT, ["config.json", "pytorch_model.bin"], ref=revision)

    monkeypatch.setattr(hub, "_download", download)
    with pytest.raises(RuntimeError, match="safetensors weights only"):
        hub.ensure("org/pickle-only")


def test_e_m10_download_failure_is_one_line(cfg, monkeypatch):
    def download(*a, **kw):
        raise OSError("network down\nretry later")

    monkeypatch.setattr(hub, "_download", download)
    with pytest.raises(RuntimeError) as err:
        hub.ensure(CLAP)
    assert str(err.value).startswith(f"could not download {CLAP}@16c8cc3 into {cfg.HF_CACHE}: OSError: network down")


def test_download_needs_the_ml_extras(cfg, monkeypatch):
    from hark import models

    monkeypatch.setattr(models, "_absent", lambda names: tuple(names))
    with pytest.raises(models.MLMissing):
        hub.ensure(CLAP)
    assert not Path(cfg.HF_CACHE, "models--laion--larger_clap_general").exists()
