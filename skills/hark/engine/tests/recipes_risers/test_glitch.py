"""glitch and burstglitch (recipes-motion part 3): AT9, AT10, the 2 generators and the glitch measures."""

from __future__ import annotations

import numpy as np
import pytest

from hark.forge.recipes.risers import _glitchkit, burstglitch, glitch


def test_at_9_glitch_v2_defaults(render_of):
    assert glitch.DEFAULTS["dur"] == 0.65 and glitch.DEFAULTS["end_tail"] == 0.012
    x = render_of(glitch, glitch.DEFAULTS)
    assert len(x) == 31200 + 2304 == 33504
    assert x[:2].tolist() == [[0.0, 0.0], [0.0, 0.0]]


def test_glitch_defaults_omit_round3_keys():
    assert "skip" not in glitch.DEFAULTS and "accent_db" not in glitch.DEFAULTS
    assert "skip" in glitch.SPACE and "accent_db" in glitch.SPACE


def test_glitch_round3_keys_change_the_render(render_of):
    a = render_of(glitch, glitch.DEFAULTS)
    b = render_of(glitch, {**glitch.DEFAULTS, "skip": 0.3, "accent_db": 6.0})
    assert a.shape == b.shape and not np.array_equal(a, b)


@pytest.mark.parametrize("name", [n for n, _ in glitch.FAMILY6])
def test_glitch_v6_members(render_of, p1, name):
    p = glitch.full6(name)
    assert "roll0" in p
    x = render_of(glitch, p)
    assert len(x) == p1(p["dur"]) + p1(4 * p["end_tail"])
    assert np.max(np.abs(x)) == pytest.approx(0.89, abs=1e-9)
    assert x[:2].tolist() == [[0.0, 0.0], [0.0, 0.0]]


def test_glitch_v6_path_is_chosen_by_roll0(render_of):
    p = glitch.full6("stutter")
    assert np.array_equal(render_of(glitch, p), glitch.render6(dict(p), np.random.default_rng(1)))


def test_glitch_pattern_fixes_the_gesture():
    """The pattern generator lays out the bursts: same pattern, same layout; the noise varies per render seed."""
    a = _glitchkit.pattern_rng(11, 13).random(5)
    b = _glitchkit.pattern_rng(11, 13).random(5)
    c = _glitchkit.pattern_rng(12, 13).random(5)
    assert np.array_equal(a, b) and not np.array_equal(a, c)


def test_grid_accelerates_into_the_cut():
    slots = _glitchkit.grid(0.2, 0.65, 0.032, 0.022, 1.2)
    lengths = [d for _, d in slots]
    assert lengths[0] == pytest.approx(0.032)
    assert lengths[-1] < lengths[0]
    t, d = slots[-1]
    assert t + d <= 0.65 + 0.25 * d
    assert _glitchkit.grid(0.6, 0.6005, 0.03, 0.02, 1.0) == [(0.6, pytest.approx(0.001))]


def test_glitch_stats_fields(render_of):
    x = render_of(glitch, glitch.DEFAULTS)
    s = glitch.glitch_stats(x)
    assert set(s) == {"corr", "centroid_hz", "s412_db", "shares", "end_db", "bursts", "early_ms", "late_ms",
                      "med2_db"}
    assert len(s["shares"]) == 5 and s["bursts"] >= 6 and s["centroid_hz"] > 3000
    assert glitch.glitch_stats(x[:, 0])["corr"] == 1.0
    assert -3.0 <= glitch.study_extra(x) <= 0.0
    assert -3.0 <= glitch.generic_extra(x) <= 0.0
    assert glitch.PRO_SHARES == (-21.7, -24.8, -17.0, -1.2, -6.6)


@pytest.mark.parametrize("name", [n for n, _ in burstglitch.FAMILIES["glitch"]])
def test_at_10_burstglitch_members(render_of, p1, name):
    p = dict(burstglitch.FAMILIES["glitch"])[name]
    x = render_of(burstglitch, p)
    assert len(x) == p1(p["dur"]) + p1(5 * p["end_tail"])
    assert np.max(np.abs(x)) <= 0.89 + 1e-9
    assert x[:2].tolist() == [[0.0, 0.0], [0.0, 0.0]]


def test_burstglitch_first_burst_is_loudest(render_of):
    """mid has almost no crackle lead-in: its loudest moment is in the first 60 ms."""
    p = dict(burstglitch.FAMILIES["glitch"])["mid"]
    x = render_of(burstglitch, p)
    assert np.argmax(np.abs(x).max(axis=1)) / 48000 < 0.06


def test_burstglitch_chirp_steps_down():
    """Tonal buffers step down in pitch through the buffer, never up (11.5 step 3)."""
    p = dict(burstglitch.FAMILIES["glitch"])["chirp"]
    for pattern in range(40):
        track = burstglitch.step_track(p, 3000, _glitchkit.pattern_rng(pattern, 101))
        assert np.all(np.diff(track) <= 0)
