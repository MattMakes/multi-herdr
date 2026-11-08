"""Shared fixtures and markers for every hark test area.

Signal fixtures are factories. Each returns float32 audio at 48 kHz:
shape (n,) for mono, (n, 2) for stereo. The ``wav`` fixture writes a signal to
a file under ``tmp_path``. This file imports no hark module, so every area can
run before the other units exist.
"""

from __future__ import annotations

import importlib.util
import os
import sys
from collections.abc import Callable, Sequence

import numpy as np
import pytest

SR = 48000
ML_MODULES = ("torch", "transformers")
OFFLINE_VARS = ("HF_HUB_OFFLINE", "TRANSFORMERS_OFFLINE")


def _ml_present() -> bool:
    return all(importlib.util.find_spec(m) is not None for m in ML_MODULES)


def pytest_collection_modifyitems(config: pytest.Config, items: list[pytest.Item]) -> None:
    skip_ml = None if _ml_present() else pytest.mark.skip(reason="needs the ML extras (torch, transformers)")
    skip_live = None if os.environ.get("HARK_LIVE") == "1" else pytest.mark.skip(reason="live test: set HARK_LIVE=1")
    for item in items:
        if skip_ml is not None and "ml" in item.keywords:
            item.add_marker(skip_ml)
        if skip_live is not None and "live" in item.keywords:
            item.add_marker(skip_live)


@pytest.fixture(autouse=True)
def _hub_offline(request: pytest.FixtureRequest, monkeypatch: pytest.MonkeyPatch) -> None:
    """Keep every test that is not marked ``ml`` or ``live`` off the Hugging Face Hub.

    huggingface_hub reads HF_HUB_OFFLINE once, at import, into
    ``huggingface_hub.constants``. The fixture sets the variables for an import
    during the test and patches the constant when the package is already loaded.
    An ``ml`` or ``live`` test gets the constant that the caller's environment gives.
    """
    keywords = request.node.keywords
    hermetic = "ml" not in keywords and "live" not in keywords
    if hermetic:
        for var in OFFLINE_VARS:
            monkeypatch.setenv(var, "1")
    constants = sys.modules.get("huggingface_hub.constants")
    if constants is not None:
        caller = any(os.environ.get(v, "").upper() in ("1", "ON", "YES", "TRUE") for v in OFFLINE_VARS)
        monkeypatch.setattr(constants, "HF_HUB_OFFLINE", hermetic or caller)


def _n(seconds: float) -> int:
    return max(1, int(round(seconds * SR)))


def _shape(x: np.ndarray, stereo: bool) -> np.ndarray:
    x = x.astype(np.float32)
    return np.column_stack([x, x]) if stereo else x


@pytest.fixture
def sr() -> int:
    """The hark sample rate, 48000 Hz."""
    return SR


@pytest.fixture
def rng() -> np.random.Generator:
    """A generator with the fixed seed 0."""
    return np.random.default_rng(0)


@pytest.fixture
def has_ml() -> bool:
    """True when torch and transformers can be imported."""
    return _ml_present()


@pytest.fixture
def tone() -> Callable[..., np.ndarray]:
    """tone(freq=440, dur=1.0, amp=0.5, stereo=False, lead=0.0, decay=None, phase=0.0).

    A sine of ``dur`` seconds after ``lead`` seconds of silence. With ``decay``
    (seconds) the sine has the envelope exp(-t / decay) from its start.
    """

    def make(freq: float = 440.0, dur: float = 1.0, amp: float = 0.5, stereo: bool = False,
             lead: float = 0.0, decay: float | None = None, phase: float = 0.0) -> np.ndarray:
        t = np.arange(_n(dur)) / SR
        x = amp * np.sin(2 * np.pi * freq * t + phase)
        if decay is not None:
            x = x * np.exp(-t / decay)
        if lead > 0:
            x = np.concatenate([np.zeros(_n(lead)), x])
        return _shape(x, stereo)

    return make


@pytest.fixture
def noise() -> Callable[..., np.ndarray]:
    """noise(dur=1.0, amp=0.5, seed=0, stereo=False, decorrelated=False, decay=None).

    Uniform white noise in [-amp, amp]. ``decorrelated`` draws an independent
    right channel. ``decay`` (seconds) applies exp(-t / decay).
    """

    def make(dur: float = 1.0, amp: float = 0.5, seed: int = 0, stereo: bool = False,
             decorrelated: bool = False, decay: float | None = None) -> np.ndarray:
        r = np.random.default_rng(seed)
        n = _n(dur)
        env = np.exp(-np.arange(n) / SR / decay) if decay is not None else np.ones(n)
        left = amp * r.uniform(-1.0, 1.0, n) * env
        if stereo and decorrelated:
            right = amp * r.uniform(-1.0, 1.0, n) * env
            return np.column_stack([left, right]).astype(np.float32)
        return _shape(left, stereo)

    return make


@pytest.fixture
def clicks() -> Callable[..., np.ndarray]:
    """clicks(times=(0.0, 0.5, 1.0), dur=1.5, freq=2000, length=0.03, amp=0.8, stereo=False).

    Short decaying sine bursts that start at the given times (seconds).
    """

    def make(times: Sequence[float] = (0.0, 0.5, 1.0), dur: float = 1.5, freq: float = 2000.0,
             length: float = 0.03, amp: float = 0.8, stereo: bool = False) -> np.ndarray:
        x = np.zeros(_n(dur))
        k = _n(length)
        t = np.arange(k) / SR
        burst = amp * np.sin(2 * np.pi * freq * t) * np.exp(-t / (length / 4))
        for start in times:
            i = int(round(start * SR))
            j = min(len(x), i + k)
            if i < len(x):
                x[i:j] += burst[: j - i]
        return _shape(x, stereo)

    return make


@pytest.fixture
def silence() -> Callable[..., np.ndarray]:
    """silence(dur=1.0, stereo=False): digital silence."""

    def make(dur: float = 1.0, stereo: bool = False) -> np.ndarray:
        return _shape(np.zeros(_n(dur)), stereo)

    return make


@pytest.fixture
def wav(tmp_path) -> Callable[..., str]:
    """wav(x, name="sound.wav", sr=48000, subtype="PCM_24") -> str path under tmp_path."""
    import soundfile as sf

    def write(x: np.ndarray, name: str = "sound.wav", sr: int = SR, subtype: str = "PCM_24") -> str:
        path = tmp_path / name
        path.parent.mkdir(parents=True, exist_ok=True)
        sf.write(str(path), np.asarray(x, dtype=np.float32), sr, subtype=subtype)
        return str(path)

    return write


@pytest.fixture
def hark_home(tmp_path, monkeypatch) -> str:
    """Point HARK_HOME at an empty folder under tmp_path and return its path."""
    home = tmp_path / "hark_home"
    home.mkdir()
    monkeypatch.setenv("HARK_HOME", str(home))
    return str(home)
