"""The launcher skills/hark/scripts/hark: finds its engine, uses the venv python, offers the venv (ARCH 4.4, 13)."""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

LAUNCHER = Path(__file__).resolve().parents[3] / "scripts" / "hark"


def _run(cmd: list[str], env: dict) -> subprocess.CompletedProcess:
    base = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": env.get("HOME", os.environ.get("HOME", ""))}
    return subprocess.run(cmd, env={**base, **env}, capture_output=True, text=True, stdin=subprocess.DEVNULL,
                          timeout=300)


def test_launcher_is_executable():
    assert LAUNCHER.is_file() and os.access(LAUNCHER, os.X_OK)


def test_no_venv_non_tty_exits_3_with_the_command(tmp_path, hark_home):
    missing = tmp_path / "nowhere" / "venv" / "bin" / "python"
    proc = _run([str(LAUNCHER), "doctor"], {"HARK_PYTHON": str(missing), "HARK_HOME": hark_home})
    assert proc.returncode == 3
    assert "python3 -m venv" in proc.stderr and str(tmp_path / "nowhere" / "venv") in proc.stderr
    assert "[y/N]" not in proc.stderr
    assert not missing.exists()


def test_runs_the_engine_through_a_symlink(tmp_path, hark_home):
    link_dir = tmp_path / "bin"
    link_dir.mkdir()
    link = link_dir / "hark"
    link.symlink_to(LAUNCHER)
    proc = _run([str(link), "--version"], {"HARK_PYTHON": sys.executable, "HARK_HOME": hark_home})
    assert proc.returncode == 0, proc.stderr
    assert proc.stdout.strip() == "hark 0.1.0"


def test_engine_is_found_from_a_copy(tmp_path, hark_home):
    root = tmp_path / "copy" / "hark"
    shutil.copytree(LAUNCHER.parents[1] / "engine" / "hark", root / "engine" / "hark",
                    ignore=shutil.ignore_patterns("__pycache__"))
    init = root / "engine" / "hark" / "__init__.py"
    init.write_text(init.read_text(encoding="utf-8").replace('"0.1.0"', '"9.9.9"'), encoding="utf-8")
    (root / "scripts").mkdir()
    shutil.copy2(LAUNCHER, root / "scripts" / "hark")
    proc = _run([str(root / "scripts" / "hark"), "--version"], {"HARK_PYTHON": sys.executable, "HARK_HOME": hark_home})
    assert proc.returncode == 0, proc.stderr
    assert proc.stdout.strip() == "hark 9.9.9"


@pytest.mark.skipif(not Path(os.path.expanduser("~/.local/state/hark/venv/bin/python")).exists(),
                    reason="needs the hark venv")
def test_default_python_is_the_hark_venv(hark_home):
    env = {"HARK_HOME": hark_home}
    proc = _run([str(LAUNCHER), "--version"], env)
    assert proc.returncode == 0, proc.stderr
