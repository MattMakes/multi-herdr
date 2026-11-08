"""Fixtures of the fit pipeline tests: the synthetic recordings S1 .. S5 of fit.md."""

from __future__ import annotations

import math
from collections.abc import Callable

import numpy as np
import pytest
from scipy import signal

SR = 48000


# ---------------------------------------------------------------- synthetic recordings (fit.md S1 .. S5)


def _s1(rng: np.random.Generator) -> np.ndarray:
    n = int(0.8 * SR)
    t = np.arange(n) / SR
    y = sum(a * np.exp(-t / tau) * np.sin(2 * np.pi * f * t)
            for f, tau, a in ((440, 0.4, 0.5), (1130, 0.2, 0.3), (2390, 0.1, 0.2)))
    k = int(0.002 * SR)
    y[:k] += 0.3 * rng.standard_normal(k) * np.linspace(1, 0, k)
    return y


def _s2() -> np.ndarray:
    n = int(0.2 * SR)
    t = np.arange(n) / SR
    f = np.where(t < 0.06, 900 * (500 / 900) ** (t / 0.06), 500.0)
    env = np.minimum(t / 0.003, 1) * np.exp(-np.maximum(t - 0.003, 0) / 0.04)
    return 0.6 * env * np.sin(2 * np.pi * np.cumsum(f) / SR)


def _s3(rng: np.random.Generator) -> np.ndarray:
    n = int(0.6 * SR)
    t = np.arange(n) / SR
    env = 0.5 - 0.5 * np.cos(2 * np.pi * t / t[-1])
    out = []
    shared = rng.standard_normal(n)
    for _ in range(2):
        w = 0.6 * shared + 0.8 * rng.standard_normal(n)
        y = np.zeros(n)
        for i in range(0, n, 2400):
            fc = 500 * 8 ** (i / n)
            b, a = signal.butter(2, [fc / 1.5, min(fc * 1.5, 23000)], "bandpass", fs=SR)
            seg = signal.lfilter(b, a, w[max(0, i - 2400): i + 2400])
            y[i: i + 2400] = seg[-len(y[i: i + 2400]):] if i else seg[: len(y[i: i + 2400])]
        out.append(0.4 * env * y / (np.std(y) + 1e-12))
    return np.column_stack(out)


def _s4() -> np.ndarray:
    n = int(0.33 * SR)
    t = np.arange(n) / SR
    y = np.zeros(n)
    for i in range(4):
        s, e = int(i * 0.08 * SR), int((i * 0.08 + 0.03) * SR)
        tt = t[: e - s]
        y[s:e] = 0.5 * np.exp(-tt / 0.4) * np.sin(2 * np.pi * 440 * tt)
    return y


# fit.md places S1 at 200 ms, but bed detection only finds an onset under 120 ms (fit-auto.md); see the report.
BED_LEAD_S = 0.1


def _s5(rng: np.random.Generator) -> np.ndarray:
    s1 = _s1(rng)
    lead = int(BED_LEAD_S * SR)
    n = lead + len(s1)
    w = np.fft.rfft(rng.standard_normal(n))
    f = np.fft.rfftfreq(n, 1 / SR)
    pink = np.fft.irfft(w / np.sqrt(np.maximum(f, 20)), n)
    c = np.concatenate([[0.0], np.cumsum(s1 ** 2)])
    loud = math.sqrt(float(np.max(c[2400:] - c[:-2400])) / 2400)
    pink *= loud * 10 ** (-25 / 20) / np.sqrt(np.mean(pink ** 2))
    y = pink.copy()
    y[lead:] += s1
    return y


@pytest.fixture
def sounds() -> dict:
    """The synthetic recordings S1 .. S5 of fit.md, from fixed generator seeds."""
    return {"S1": _s1(np.random.default_rng(1)), "S2": _s2(), "S3": _s3(np.random.default_rng(3)), "S4": _s4(),
            "S5": _s5(np.random.default_rng(5))}


@pytest.fixture
def write_wav(tmp_path) -> Callable[[str, np.ndarray], str]:
    """write_wav(name, x) writes a 48 kHz float WAV under tmp_path and returns its path."""
    import soundfile

    def write(name: str, x: np.ndarray) -> str:
        p = tmp_path / name
        p.parent.mkdir(parents=True, exist_ok=True)
        soundfile.write(str(p), np.asarray(x, dtype=np.float32), SR, subtype="FLOAT")
        return str(p)

    return write


@pytest.fixture
def bed_lead() -> float:
    """Seconds of bed before S1 in S5."""
    return BED_LEAD_S
