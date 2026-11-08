"""shimmer (recipes-motion-tonal section 13): AT17 to AT20 and the family members."""

from __future__ import annotations

import numpy as np
import pytest

SR = 48000


def test_at_17_scale_freqs(load):
    m = load("shimmer")
    got = m.scale_freqs(400, 1000, "penta", 0)
    assert np.allclose(got, [440.00, 523.25, 587.33, 659.26, 783.99, 880.00], atol=0.01)
    assert np.allclose(m.scale_freqs(500, 510, "penta", 0), [504.98], atol=0.01)


def test_at_18_envelope(load):
    m = load("shimmer")
    p = {**m.DEFAULTS, "start_db": -42, "swell": 0.85, "rise_curve": 1.2, "hold": 0.1, "fade": 0.85}
    env = m.envelope(p, int(2.0 * SR))
    for t, want in ((0.0, 0.00794), (0.85, 1.0), (1.8, 0.00562)):
        assert env[int(round(t * SR))] == pytest.approx(want, rel=0.01), t


def test_at_19_pro_sparkle_is_wide(load, run, p1):
    m = load("shimmer")
    p = m.FAMILY["pro_shimmer"][0]
    assert p["style"] == "sparkle"
    x = run(m, p)
    assert len(x) == p1(1.95) == 93600
    assert m.measure(x)["corr"] < 0.6


def test_at_20_ring_out_run(load, run, p1):
    m = load("shimmer")
    p = m.FAMILY["shimmer"][0]
    assert p["style"] == "run" and p["ring"] == 1
    x = run(m, p)
    assert len(x) == p1(4.2) == 201600
    assert np.all(x[0] == 0.0)
    assert np.all(np.abs(x[-1]) < 1e-9 * np.max(np.abs(x)))


@pytest.mark.parametrize("cat", ("pro_shimmer", "shimmer"))
def test_family_members_render(cat, load, run, p1):
    m = load("shimmer")
    for i, p in enumerate(m.FAMILY[cat]):
        x = run(m, p)
        assert len(x) == p1(p["length"]), (cat, i)
        assert np.all(np.isfinite(x)) and 1e-4 <= np.max(np.abs(x)) <= 1.0, (cat, i)


def test_round_2_path_renders(load, run):
    m = load("shimmer")
    p = {**m.FAMILY["pro_shimmer"][3], "r2_path": 1}
    x = run(m, p)
    assert np.all(np.isfinite(x)) and not np.array_equal(x, run(m, m.FAMILY["pro_shimmer"][3]))


def test_family_space_pins_keys_outside_fine(load):
    m = load("shimmer")
    space = m.family_space(m.FAMILY["pro_shimmer"][0])
    assert space["style"] == ["sparkle"]
    assert space["tilt"] == m.SPACE["tilt"]
    assert space["length"] == [m.FAMILY["pro_shimmer"][0]["length"]]
