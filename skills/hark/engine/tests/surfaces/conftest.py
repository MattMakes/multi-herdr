"""Fixtures of the surfaces test area.

Stand-ins: core (audio, features, report, config) and models are built by other units. While such a module is
absent, this file installs a small stand-in that follows ARCHITECTURE.md, only for this test session. When the
real module exists, the real one is used. Nothing here goes into the package.

The fake agy is 1 shell script per session. It logs its arguments and prints the reply file that a test names
in FAKE_AGY_REPLY. It is warmed once, because a new executable is slow at its first run on macOS.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import math
import os
import subprocess
import sys
import types
from collections.abc import Callable, Iterator
from pathlib import Path
from typing import Any

import numpy as np
import pytest

SR = 48000
AUDIO_EXTS = frozenset({".wav", ".flac", ".ogg", ".oga", ".opus", ".mp3", ".m4a", ".aac", ".aif", ".aiff", ".wma"})
VIDEO_EXTS = frozenset({".mp4", ".mov", ".m4v", ".webm", ".mkv", ".avi"})


def module_absent(name: str) -> bool:
    """True when the module cannot be found (a missing parent counts as absent)."""
    if name in sys.modules:
        return False
    try:
        return importlib.util.find_spec(name) is None
    except (ImportError, ValueError):
        return True


# ---------------------------------------------------------------- stand-in bodies


class AudioError(RuntimeError):
    pass


def _sa_load(path: str, sr: int = SR, mono: bool = False, start: float | None = None, end: float | None = None,
             cache: bool = True) -> np.ndarray:
    import soundfile as sf
    from scipy.signal import resample_poly
    if not os.path.isfile(path):
        raise AudioError(f"no such file: {path}")
    x, rate = sf.read(path, dtype="float32", always_2d=True)
    a = int(round((start or 0.0) * rate))
    b = x.shape[0] if end is None else int(round(end * rate))
    x = x[a:b]
    if rate != sr:
        g = math.gcd(int(rate), int(sr))
        x = resample_poly(x, sr // g, rate // g, axis=0).astype(np.float32)
    if mono:
        return x.mean(axis=1).astype(np.float32)
    return (np.column_stack([x[:, 0], x[:, 0]]) if x.shape[1] == 1 else x[:, :2]).astype(np.float32)


def _sa_duration(path: str) -> float:
    import soundfile as sf
    info = sf.info(path)
    return info.frames / info.samplerate


def _sa_probe(path: str) -> dict:
    return {"duration": _sa_duration(path)}


def _sa_list_media(paths: Any, recursive: bool = True, exts: frozenset = AUDIO_EXTS | VIDEO_EXTS) -> list[str]:
    import glob
    out: list[str] = []
    for p in [paths] if isinstance(paths, str) else paths:
        hits = [p] if os.path.exists(p) else sorted(glob.glob(p))
        for h in hits:
            if os.path.isdir(h):
                for root, _, files in os.walk(h):
                    out += sorted(os.path.join(root, f) for f in files if os.path.splitext(f)[1].lower() in exts)
            elif os.path.isfile(h) and os.path.splitext(h)[1].lower() in exts:
                out.append(h)
    return out


def _sa_file_id(path: str) -> str:
    st = os.stat(path)
    return hashlib.sha1(f"{os.path.abspath(path)}|{st.st_size}|{st.st_mtime_ns}".encode()).hexdigest()[:20]


def _sf_measure(src: Any, music: bool = False, start: float | None = None, end: float | None = None,
                light: bool = False) -> dict:
    x = src if isinstance(src, np.ndarray) else _sa_load(src, start=start, end=end)
    x = np.asarray(x, dtype=np.float64)
    n = x.shape[0]
    peak = float(np.max(np.abs(x))) if n else 0.0
    rms = float(np.sqrt(np.mean(x ** 2))) if n else 0.0
    peak_db = round(20 * math.log10(peak), 1) if peak > 0 else -200.0
    return {"duration": round(n / SR, 3), "peak_dbfs": peak_db,
            "digital_silence_frac": float(np.mean(np.all(np.atleast_2d(x.T).T == 0, axis=-1))) if n else 1.0,
            "lufs": round(20 * math.log10(rms) - 0.7, 1) if rms > 0 else None, "short_term_max_lufs": None,
            "rms_dbfs": round(20 * math.log10(rms), 1) if rms > 0 else -200.0,
            "true_peak_dbtp": peak_db, "clipped_samples": int(np.sum(np.abs(x) >= 0.999)), "lra_lu": None,
            "noise_floor_dbfs": -90.0, "brightness_hz": 0, "bandwidth_hz": 0, "attack_ms": None,
            "decay_ms": None, "onset_count": None}


def _sr_add_common(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--out", default=None)
    parser.add_argument("--threads", type=int, default=None)


def _sr_clean(obj: Any) -> Any:
    if isinstance(obj, dict):
        return {str(k): _sr_clean(v) for k, v in obj.items()}
    if isinstance(obj, (list, tuple)):
        return [_sr_clean(v) for v in obj]
    if isinstance(obj, float) and not math.isfinite(obj):
        return None
    return obj


def _sr_emit(result: dict, args: argparse.Namespace, text: str | None = None) -> dict:
    clean = _sr_clean(result)
    if getattr(args, "out", None):
        Path(args.out).write_text(json.dumps(clean, indent=1))
    print(json.dumps(clean, indent=1) if getattr(args, "json", False) else (text or str(result.get("summary"))))
    return result


def _sr_error_line(cmd: str, msg: str) -> None:
    print(f"hark {cmd}: error: {' '.join(str(msg).split())}", file=sys.stderr)


class MLMissing(RuntimeError):
    def __init__(self, feature: str, missing: tuple[str, ...], command: str) -> None:
        super().__init__(f"{feature} needs the extras ({', '.join(missing)})")
        self.feature, self.missing, self.command = feature, missing, command


def _install_command(extra: str = "ml") -> str:
    return f'{sys.executable} -m pip install "mcp>=2"' if extra == "mcp" else f"{sys.executable} -m pip install torch"


def _require_extra(extra: str, feature: str) -> None:
    mods = ("mcp",) if extra == "mcp" else ("torch", "transformers")
    missing = tuple(m for m in mods if importlib.util.find_spec(m) is None)
    if missing:
        raise MLMissing(feature, missing, _install_command(extra))


STANDINS: dict[str, dict[str, Any]] = {
    "hark.core.audio": {"SR": SR, "AudioError": AudioError, "load": _sa_load, "duration": _sa_duration,
                        "probe": _sa_probe, "list_media": _sa_list_media, "file_id": _sa_file_id,
                        "AUDIO_EXTS": AUDIO_EXTS, "VIDEO_EXTS": VIDEO_EXTS, "MEDIA_EXTS": AUDIO_EXTS | VIDEO_EXTS},
    "hark.core.features": {"measure": _sf_measure},
    "hark.core.report": {"add_common": _sr_add_common, "emit": _sr_emit, "clean": _sr_clean,
                         "error_line": _sr_error_line},
    "hark.core.config": {"HOME": os.path.expanduser("~/.cache/hark")},
    "hark.models": {"MLMissing": MLMissing, "install_command": _install_command, "require_extra": _require_extra,
                    "require_ml": lambda feature: _require_extra("ml", feature)},
}


def _attach(name: str, mod: types.ModuleType) -> None:
    parent, _, child = name.rpartition(".")
    if parent in sys.modules:
        setattr(sys.modules[parent], child, mod)


def _new_module(name: str, attrs: dict[str, Any], package: bool = False) -> types.ModuleType:
    mod = types.ModuleType(name, f"stand-in for {name} (tests/surfaces only)")
    if package:
        mod.__path__ = []  # type: ignore[attr-defined]
    for key, val in attrs.items():
        setattr(mod, key, val)
    sys.modules[name] = mod
    _attach(name, mod)
    return mod


@pytest.fixture(scope="session", autouse=True)
def standins() -> Iterator[list[str]]:
    """Install a stand-in for every absent core or models module; remove them after the session."""
    installed: list[str] = []
    if module_absent("hark.core"):
        _new_module("hark.core", {}, package=True)
        installed.append("hark.core")
    for name, attrs in STANDINS.items():
        if module_absent(name):
            _new_module(name, attrs, package=name == "hark.models")
            installed.append(name)
    yield installed
    for name in reversed(installed):
        sys.modules.pop(name, None)


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
