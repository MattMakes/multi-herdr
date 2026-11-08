"""Fixtures of the core test area: a fresh config, a fake hub cache, a CLI runner."""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path

import pytest

ENGINE = Path(__file__).resolve().parents[2]
LAUNCHER = ENGINE.parent / "scripts" / "hark"

_CONFIG_ATTRS = ("HOME", "HF_CACHE", "EMB_CACHE", "DEC_CACHE", "CLAP_MODEL", "AST_MODEL",
                 "CLAP_REVISION", "AST_REVISION", "PRECISION")


@pytest.fixture(autouse=True)
def cfg(hark_home, monkeypatch):
    """hark.core.config read again with HARK_HOME in tmp_path and no model or offline overrides.

    Autouse: no core test writes a cache outside tmp_path.
    """
    from hark.core import config

    for name in ("HARK_CLAP", "HARK_AST", "HARK_CLAP_REVISION", "HARK_AST_REVISION", "HARK_PRECISION",
                 *config.OFFLINE_FLAGS):
        monkeypatch.delenv(name, raising=False)
    saved = {a: getattr(config, a) for a in _CONFIG_ATTRS}
    config.refresh()
    yield config
    for attr, value in saved.items():
        setattr(config, attr, value)


@pytest.fixture
def snapshot(cfg) -> Callable[..., Path]:
    """snapshot(model_id, commit, files, ref=None): write a fake hub snapshot and return its folder."""

    def make(model_id: str, commit: str, files, ref: str | None = None) -> Path:
        base = Path(cfg.HF_CACHE) / f"models--{model_id.replace('/', '--')}"
        folder = base / "snapshots" / commit
        folder.mkdir(parents=True, exist_ok=True)
        for name in files:
            (folder / name).write_text("{}")
        if ref:
            ref_file = base / "refs" / ref
            ref_file.parent.mkdir(parents=True, exist_ok=True)
            ref_file.write_text(commit)
        return folder

    return make


@pytest.fixture
def pin_files(cfg) -> Callable[[str], list[str]]:
    """pin_files(model_id): every file of a pinned snapshot."""

    def files(model_id: str) -> list[str]:
        pin = cfg.PINS[model_id]
        return list(pin["files"]) + [pin["weights"]]

    return files


@pytest.fixture
def fake_download(cfg, monkeypatch, snapshot):
    """Replace hub._download with a recorder that writes the allowed files into the snapshot."""
    from hark.models import hub

    calls: list[dict] = []

    def download(repo_id, revision, cache_dir, allow_patterns):
        calls.append({"repo_id": repo_id, "revision": revision, "cache_dir": cache_dir,
                      "allow_patterns": list(allow_patterns)})
        names = [n for n in allow_patterns if "?" not in n and "*" not in n]
        if re.fullmatch(r"[0-9a-f]{40}", revision):
            snapshot(repo_id, revision, names)
        else:
            snapshot(repo_id, "f" * 40, names, ref=revision)
        return cache_dir

    monkeypatch.setattr(hub, "_download", download)
    return calls


@pytest.fixture
def run_hark(hark_home) -> Callable[..., subprocess.CompletedProcess]:
    """run_hark(*args, env=None, stdin=None): run `python -m hark` in a fresh process."""

    def run(*args: str, env: dict | None = None, code: str | None = None) -> subprocess.CompletedProcess:
        full = {**os.environ, "PYTHONPATH": str(ENGINE), "HARK_HOME": hark_home, **(env or {})}
        cmd = [sys.executable, "-c", code] if code else [sys.executable, "-m", "hark", *args]
        return subprocess.run(cmd, env=full, capture_output=True, text=True, stdin=subprocess.DEVNULL,
                              timeout=300, cwd=str(ENGINE))

    return run


@pytest.fixture
def ffmpeg() -> str:
    """Path of ffmpeg, or skip."""
    found = shutil.which("ffmpeg")
    if not found:
        pytest.skip("ffmpeg not on PATH")
    return found
