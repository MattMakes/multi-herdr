"""The pass engine family: whoosh (AT3), brightswish (AT4), swish, swipe, and the version-1 path."""

from __future__ import annotations

import numpy as np
import pytest

SR = 48000


def loudest_block_start(x: np.ndarray, block: int = 480) -> float:
    """Start time (s) of the loudest whole 10 ms block of the mono fold."""
    m = x.mean(axis=1)
    k = len(m) // block
    e = np.sqrt(np.mean(m[: k * block].reshape(k, block) ** 2, axis=1))
    return int(np.argmax(e)) * block / SR


def raised_cos_out(y: np.ndarray, seconds: float) -> np.ndarray:
    """P9 fade with no fade-in: the last P1(seconds) samples times 0.5 + 0.5 cos(pi i / (b - 1))."""
    y = np.array(y, dtype=float)
    b = min(len(y), max(1, int(round(seconds * SR))))
    if b > 1:
        y[-b:] *= (0.5 + 0.5 * np.cos(np.pi * np.arange(b) / (b - 1)))[:, None]
    return y


def test_at_3_whoosh_defaults(load, run, p1):
    m = load("whoosh")
    x = run(m, m.DEFAULTS)
    assert len(x) <= p1(4.5)
    assert np.all(x[0] == 0.0)
    assert np.all(x[-1] == 0.0)
    _, _, tp, _ = m.envelope(m.DEFAULTS)
    assert abs(loudest_block_start(x) - tp) <= 0.060


def test_whoosh_envelope_peak_is_one(load):
    m = load("whoosh")
    t, a, tp, n = m.envelope(m.DEFAULTS)
    assert len(t) == len(a) == n
    assert a.max() == pytest.approx(1.0)
    assert 0.0 < tp < n / SR


def test_at_4_brightswish_without_comb(load, run):
    bright = load("brightswish")
    whoosh = load("whoosh")
    p = {**bright.DEFAULTS, "comb_mix": 0.0}
    got = run(bright, p, 1)
    q = {**bright.SWISH, **p}
    plain = run(whoosh, {k: v for k, v in q.items() if k not in bright.COMB_KEYS}, 1)
    plain = raised_cos_out(plain, min(q["end_fade"], 0.3 * len(plain) / SR))
    want = 0.89 * plain / (np.max(np.abs(plain)) + 1e-12)
    assert got.shape == want.shape
    assert np.max(np.abs(got - want)) <= 1e-12


def test_brightswish_peak_is_0_89(load, run):
    m = load("brightswish")
    for cat, members in m.FAMILIES.items():
        for name, params in members:
            x = run(m, m.preset_full(cat, params))
            assert np.max(np.abs(x)) == pytest.approx(0.89, abs=1e-9), (cat, name)


@pytest.mark.parametrize("name", ("whoosh", "swish", "swipe"))
def test_every_preset_member_renders(name, load, run):
    m = load(name)
    for kind, members in m.PRESETS.items():
        for member in members:
            x = run(m, m.preset(kind, member))
            assert np.all(np.isfinite(x)) and x.shape[1] == 2, (kind, member)
            assert 1e-4 <= np.max(np.abs(x)) <= 1.0, (kind, member)


def test_whoosh_version_1_path(load, run):
    m = load("whoosh")
    p = dict(m.DEFAULTS_V1)
    assert "rise" not in p
    x = run(m, p)
    assert np.all(np.isfinite(x)) and x.shape[1] == 2
    assert 1e-4 <= np.max(np.abs(x)) <= 1.0
    assert not np.array_equal(x, run(m, p, 2))
