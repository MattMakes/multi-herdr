"""Fixtures of the fit analysis tests: synthetic sounds S1 .. S5."""

from __future__ import annotations

import types

import numpy as np
import pytest

SR = 48000


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
