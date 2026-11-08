"""Fixtures of the surfaces test area.

The fake agy is 1 shell script per session. It logs its arguments and prints the reply file that a test names
in FAKE_AGY_REPLY. It is warmed once, because a new executable is slow at its first run on macOS.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import types
from collections.abc import Callable
from pathlib import Path
from typing import Any

import pytest

# ---------------------------------------------------------------- fake agy

FAKE_AGY = """#!/bin/sh
if [ -n "$FAKE_AGY_LOG" ]; then
  printf '%s\\n' "$@" >> "$FAKE_AGY_LOG"
  echo "----" >> "$FAKE_AGY_LOG"
fi
if [ -n "$FAKE_AGY_REPLY" ]; then cat "$FAKE_AGY_REPLY"; fi
exit ${FAKE_AGY_EXIT:-0}
"""


@pytest.fixture(scope="session")
def agy_bin(tmp_path_factory: pytest.TempPathFactory) -> str:
    """A folder that holds the fake agy, warmed once."""
    folder = tmp_path_factory.mktemp("agy_bin")
    exe = folder / "agy"
    exe.write_text(FAKE_AGY)
    exe.chmod(0o755)
    subprocess.run([str(exe)], capture_output=True, timeout=120, env={"PATH": "/usr/bin:/bin"})
    return str(folder)


class FakeAgy:
    """Controls the fake agy: the reply it prints and the log of its calls."""

    def __init__(self, folder: Path, monkeypatch: pytest.MonkeyPatch) -> None:
        self.log = folder / "agy.log"
        self.reply_file = folder / "agy_reply.json"
        self._mp = monkeypatch
        monkeypatch.setenv("FAKE_AGY_LOG", str(self.log))

    def reply(self, response: str | None = None, raw: str | None = None, **extra: Any) -> None:
        body = raw if raw is not None else json.dumps({"response": response, "status": "SUCCESS", **extra})
        self.reply_file.write_text(body)
        self._mp.setenv("FAKE_AGY_REPLY", str(self.reply_file))

    def calls(self) -> list[list[str]]:
        if not self.log.exists():
            return []
        chunks = self.log.read_text().split("----\n")
        return [c.rstrip("\n").split("\n") for c in chunks if c.strip()]


@pytest.fixture
def fake_agy(agy_bin: str, tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> FakeAgy:
    """The fake agy first on PATH, with its log under tmp_path."""
    monkeypatch.setenv("PATH", agy_bin + os.pathsep + os.environ.get("PATH", ""))
    monkeypatch.delenv("HARK_LLM_MODEL", raising=False)
    return FakeAgy(tmp_path, monkeypatch)


@pytest.fixture
def no_agy(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    """A PATH without any agy."""
    empty = tmp_path / "empty_bin"
    empty.mkdir()
    monkeypatch.setenv("PATH", str(empty))


@pytest.fixture
def fake_module(monkeypatch: pytest.MonkeyPatch) -> Callable[..., types.ModuleType]:
    """fake_module(name, **attrs): put a fake module in sys.modules for 1 test."""

    def make(name: str, **attrs: Any) -> types.ModuleType:
        mod = types.ModuleType(name, "test fake")
        for key, val in attrs.items():
            setattr(mod, key, val)
        monkeypatch.setitem(sys.modules, name, mod)
        parent, _, child = name.rpartition(".")
        if parent in sys.modules:
            monkeypatch.setattr(sys.modules[parent], child, mod, raising=False)
        return mod

    return make
