"""Config: pins, offline flags, threads, cache root, and the environment scan (core-io 2.1, 3.1, 3.7, 4.11)."""

from __future__ import annotations

import ast
import re
from pathlib import Path

import pytest

PKG = Path(__file__).resolve().parents[2] / "hark"


def test_at_17_pins(cfg):
    assert set(cfg.PINS) == {cfg.DEFAULT_CLAP, cfg.DEFAULT_AST}
    for pin in cfg.PINS.values():
        assert re.fullmatch(r"[0-9a-f]{40}", pin["revision"])
        assert pin["weights"] == "model.safetensors"


@pytest.mark.parametrize("flag", ["HARK_OFFLINE", "HF_HUB_OFFLINE", "TRANSFORMERS_OFFLINE"])
@pytest.mark.parametrize("value, on", [("1", True), ("true", True), (" YES ", True), ("on", True),
                                       ("0", False), ("", False)])
def test_at_18_offline_flags_read_at_call_time(cfg, monkeypatch, flag, value, on):
    assert cfg.offline() is None
    monkeypatch.setenv(flag, value)
    assert cfg.offline() == (flag if on else None)


def test_offline_flag_order(cfg, monkeypatch):
    monkeypatch.setenv("TRANSFORMERS_OFFLINE", "1")
    monkeypatch.setenv("HF_HUB_OFFLINE", "1")
    assert cfg.offline() == "HF_HUB_OFFLINE"


def _writes_flag(node: ast.AST, flags: set[str]) -> bool:
    """True for os.environ[FLAG] = / del os.environ[FLAG] / environ.pop(FLAG) / setdefault / putenv."""
    if isinstance(node, (ast.Assign, ast.AugAssign, ast.Delete)):
        targets = node.targets if isinstance(node, (ast.Assign, ast.Delete)) else [node.target]
        return any(isinstance(t, ast.Subscript) and _const(t.slice) in flags for t in targets)
    if isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute):
        if node.func.attr in {"pop", "setdefault", "putenv", "unsetenv", "update"} and node.args:
            return _const(node.args[0]) in flags
    return False


def _const(node: ast.AST) -> object:
    return node.value if isinstance(node, ast.Constant) else None


def test_at_19_no_code_writes_offline_flags(cfg):
    flags = set(cfg.OFFLINE_FLAGS)
    offenders = []
    for path in PKG.rglob("*.py"):
        tree = ast.parse(path.read_text(encoding="utf-8"))
        offenders += [f"{path}:{n.lineno}" for n in ast.walk(tree) if _writes_flag(n, flags)]
    assert offenders == []


def test_threads(cfg, monkeypatch):
    monkeypatch.delenv("HARK_THREADS", raising=False)
    assert 1 <= cfg.threads() <= 4
    monkeypatch.setenv("HARK_THREADS", "-3")
    assert cfg.threads() == 1
    monkeypatch.setenv("HARK_THREADS", "6")
    assert cfg.threads() == 6
    monkeypatch.setenv("HARK_THREADS", "abc")
    with pytest.raises(ValueError):
        cfg.threads()


def test_default_threads_without_cpu_count(cfg, monkeypatch):
    monkeypatch.setattr(cfg.os, "cpu_count", lambda: None)
    assert cfg.default_threads() == 1
    monkeypatch.setattr(cfg.os, "cpu_count", lambda: 16)
    assert cfg.default_threads() == 4


def test_q7_cache_root(cfg, monkeypatch, hark_home, tmp_path):
    assert cfg.HOME == str(Path(hark_home).resolve())
    monkeypatch.delenv("HARK_HOME")
    monkeypatch.setenv("HOME", str(tmp_path / "user"))
    cfg.refresh()
    assert cfg.HOME == str((tmp_path / "user" / ".cache" / "hark").resolve())
    assert cfg.HF_CACHE == str(Path(cfg.HOME) / "hf")


def test_ensure_dirs(cfg):
    cfg.ensure_dirs()
    for folder in (cfg.HOME, cfg.HF_CACHE, cfg.EMB_CACHE, cfg.DEC_CACHE):
        assert Path(folder).is_dir()


def test_model_overrides(cfg, monkeypatch):
    monkeypatch.setenv("HARK_CLAP", "org/other")
    monkeypatch.setenv("HARK_CLAP_REVISION", "")
    monkeypatch.setenv("HARK_PRECISION", " FP32 ")
    cfg.refresh()
    assert (cfg.CLAP_MODEL, cfg.CLAP_REVISION, cfg.PRECISION) == ("org/other", None, "fp32")


def test_data_dir_and_venv_default(cfg):
    assert cfg.DATA_DIR == str(PKG.resolve() / "data")
    assert cfg.VENV_PYTHON_DEFAULT.endswith(".local/state/hark/venv/bin/python")
    assert "~" not in cfg.VENV_PYTHON_DEFAULT
