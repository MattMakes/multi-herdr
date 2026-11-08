"""Fixtures of the fit analysis tests: synthetic sounds S1 .. S5 and core stand-ins.

The stand-ins replace hark.core.audio and hark.core.features only while the
real modules (T-13) are absent. They are never part of the package.
"""

from __future__ import annotations

import importlib.util
import sys
import types

import numpy as np
import pytest

SR = 48000


def _k_weight(x, sr=48000):
    """ITU-R BS.1770 K filter at 48 kHz (stand-in for hark.core.features.k_weight)."""
    from scipy.signal import lfilter
    b1 = [1.53512485958697, -2.69169618940638, 1.19839281085285]
    a1 = [1.0, -1.69065929318241, 0.73248077421585]
    b2 = [1.0, -2.0, 1.0]
    a2 = [1.0, -1.99004745483398, 0.99007225036621]
    return lfilter(b2, a2, lfilter(b1, a1, np.asarray(x, dtype=float), axis=0), axis=0)


def _load(path, sr=48000, mono=False, start=None, end=None, cache=True):
    import soundfile as sf
    x, fs = sf.read(path, dtype="float32", always_2d=True)
    if fs != sr:
        from scipy.signal import resample_poly
        x = resample_poly(x, sr, fs, axis=0).astype(np.float32)
    x = x[:, :2]
    if mono:
        return x.mean(axis=1)
    return x[:, 0] if x.shape[1] == 1 else x


def _absent(name: str) -> bool:
    try:
        return importlib.util.find_spec(name) is None
    except ModuleNotFoundError:
        return True


def _install_stand_ins() -> None:
    if _absent("hark.core"):
        sys.modules["hark.core"] = types.ModuleType("hark.core")
    if _absent("hark.core.features"):
        mod = types.ModuleType("hark.core.features")
        mod.k_weight = _k_weight
        sys.modules["hark.core.features"] = mod
    if _absent("hark.core.audio"):
        mod = types.ModuleType("hark.core.audio")
        mod.SR = SR
        mod.load = _load
        mod.to_mono = lambda x: np.asarray(x).mean(axis=1) if np.ndim(x) == 2 else np.asarray(x)
        sys.modules["hark.core.audio"] = mod


_install_stand_ins()


def make_s1(seed: int = 7) -> np.ndarray:
    """S1 struck: 3 decaying sines after a 2 ms noise click, 0.8 s, mono."""
    r = np.random.default_rng(seed)
    n = int(0.8 * SR)
    t = np.arange(n) / SR
    x = np.zeros(n)
    x[:96] = 0.3 * r.standard_normal(96) * np.hanning(96)
    tt = np.maximum(t - 0.002, 0.0)
    on = t >= 0.002
    for f, tau, a in ((440, 0.4, 0.5), (1130, 0.2, 0.3), (2390, 0.1, 0.2)):
        x += on * a * np.exp(-tt / tau) * np.sin(2 * np.pi * f * tt)
    return x


def make_s2(semitones: float = 0.0) -> np.ndarray:
    """S2 glide pop: 900 to 500 Hz over 60 ms, 3 ms attack, 40 ms decay, 0.2 s."""
    n = int(0.2 * SR)
    t = np.arange(n) / SR
    k = 2 ** (semitones / 12)
    f = k * (900 + (500 - 900) * np.clip(t / 0.06, 0, 1))
    ph = 2 * np.pi * np.cumsum(f) / SR
    env = np.clip(t / 0.003, 0, 1) * np.exp(-np.maximum(t - 0.003, 0) / 0.04)
    return 0.6 * env * np.sin(ph)


def make_s3(seed: int = 3) -> np.ndarray:
    """S3 whoosh: noise through a band-pass sweeping 500 Hz to 4 kHz, raised-cosine level, stereo."""
    from scipy.signal import stft, istft
    r = np.random.default_rng(seed)
    n = int(0.6 * SR)
    common = r.standard_normal(n)
    chans = []
    for c in range(2):
        w = 0.7 * common + 0.71 * r.standard_normal(n)
        f, tt, Z = stft(w, SR, nperseg=1024)
        fc = 500 * 8 ** np.clip(tt / 0.6, 0, 1)
        g = np.exp(-0.5 * (np.log2(np.maximum(f[:, None], 1) / fc[None, :]) / 0.4) ** 2)
        y = istft(Z * g, SR, nperseg=1024)[1][:n]
        chans.append(y)
    t = np.arange(n) / SR
    env = 0.5 - 0.5 * np.cos(2 * np.pi * t / 0.6)
    y = np.column_stack(chans) * env[:, None]
    return 0.5 * y / np.max(np.abs(y))


def make_s4() -> np.ndarray:
    """S4 gated pulses: 4 pulses of S1's first sine, each cut after 30 ms, 80 ms apart."""
    n = int(0.4 * SR)
    t = np.arange(n) / SR
    x = np.zeros(n)
    for k in range(4):
        t0 = 0.01 + 0.08 * k
        tt = t - t0
        on = (tt >= 0) & (tt < 0.03)
        x += on * 0.5 * np.exp(-np.maximum(tt, 0) / 0.4) * np.sin(2 * np.pi * 440 * np.maximum(tt, 0))
    return x


@pytest.fixture(scope="session")
def s1() -> np.ndarray:
    return make_s1()


@pytest.fixture(scope="session")
def s2() -> np.ndarray:
    return make_s2()


@pytest.fixture(scope="session")
def s3() -> np.ndarray:
    return make_s3()


@pytest.fixture(scope="session")
def s4() -> np.ndarray:
    return make_s4()


@pytest.fixture(scope="session")
def make() -> types.SimpleNamespace:
    """The sound factories, for tests that need a variant (a pitch shift, another seed)."""
    return types.SimpleNamespace(s1=make_s1, s2=make_s2, s3=make_s3, s4=make_s4)


@pytest.fixture(scope="session")
def models(s1, s2, s3, s4) -> dict:
    """One fit per kind of the synthetic sounds, made once per session."""
    from hark.fit.analysis import gesture, modal, sinusoidal, spectral
    return {
        "modal_s1": modal.fit(s1),
        "modal_s4": modal.fit(s4),
        "sines_s1": sinusoidal.fit(s1),
        "sines_s2": sinusoidal.fit(s2),
        "spectral_s3": spectral.fit(s3),
        "spectral_s1": spectral.fit(s1),
        "gesture_s3": gesture.fit(s3),
        "gesture_s1": gesture.fit(s1),
    }
