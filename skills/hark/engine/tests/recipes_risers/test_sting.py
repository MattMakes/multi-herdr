"""sting (recipes-motion part 5): AT24 to AT28, the 4 idioms and the families."""

from __future__ import annotations

import numpy as np
import pytest

pytest.importorskip("hark.forge.recipes.impacts.impact")
pytest.importorskip("hark.forge.recipes.impacts.subdrop")

from hark.forge.recipes.risers import sting  # noqa: E402


def test_at_24_chirp_mode(render_of):
    x = render_of(sting, {"mode": "chirp"})
    assert len(x) == 40800
    assert np.array_equal(x[:, 0], x[:, 1])
    assert np.max(np.abs(x)) == pytest.approx(0.89, abs=1e-9)


def test_at_25_double_chirp_length(render_of):
    p = dict(sting.FAMILIES_R4["pro_logo_sting"])["double"]
    x = render_of(sting, p)
    assert len(x) == 48000 + 2805 + 576 == 51381


def test_at_26_whoosh_glass_lead_stays_under_the_hit(render_of, p1):
    p = dict(sting.FAMILIES["logo_sting"])["whoosh_glass"]
    x = render_of(sting, p)
    assert np.argmax(np.abs(x).max(axis=1)) >= p1(0.1896) - p1(0.012) == 8525
    assert np.max(np.abs(x)) == pytest.approx(0.89, abs=1e-9)


@pytest.mark.parametrize("name", [n for n, _ in sting.FAMILIES["pro_logo_sting"]])
def test_at_27_gesture_members(render_of, name):
    x = render_of(sting, dict(sting.FAMILIES["pro_logo_sting"])[name])
    assert np.all(np.isfinite(x))
    assert np.max(np.abs(x)) == pytest.approx(0.89, abs=1e-9)
    m = x.mean(axis=1)
    P = np.abs(np.fft.rfft(m)) ** 2
    f = np.fft.rfftfreq(len(m), 1 / 48000)
    assert 10 * np.log10(P[f > 20000].sum() / P.sum() + 1e-30) < -40


def test_at_28_space_for_and_recipe_for():
    s = sting.space_for("pro_logo_sting")
    assert len(s) == 15
    assert s["g_warm"] == (0.0, 0.45)
    assert s["g_lead"] == (0.0, 1.0)
    a = sting.recipe_for("logo_sting", "spark")
    b = sting.recipe_for("logo_sting", 3)
    for r in (a, b):
        assert r.PRESET == "spark"
        assert r.DEFAULTS["material"] == "triangle"
        assert r.REFSET == "logo_sting"
        assert r.MODULE == "hark.forge.recipes.risers.sting" and r.render is sting.render
    assert sting.space_for("logo_sting")["lead_s"] == (0.15, 0.6)


def test_defaults_and_bases():
    assert sting.DEFAULTS["mode"] == "launch"
    assert sting.DEFAULTS["low"] == 0.04 and "shimmer_smooth" not in sting.DEFAULTS
    assert dict(sting.FAMILIES_R4["pro_logo_sting"])["bell"]["chirp_f0"] == 1500.0
    assert dict(sting.FAMILIES_R3["pro_logo_sting"])["bell"]["chirp_f0"] == 1900.0
    assert sting.CHIRP_R4["double_gap"] == 0.05 and sting.CHIRP_R3["sub_db"] == 100.0


def test_render_does_not_merge_defaults(render_of):
    """A partial set overlays the base of its mode; a set without a mode uses the launch base."""
    a = render_of(sting, {"root": 1500.0})
    b = render_of(sting, {**sting.LAUNCH, "root": 1500.0})
    assert np.array_equal(a, b)


def test_unknown_mode_renders_reveal(render_of):
    a = render_of(sting, {"mode": "zzz", "hit": 0.0, "sub": 0.0})
    b = render_of(sting, {"mode": "reveal", "hit": 0.0, "sub": 0.0})
    assert np.array_equal(a, b)


def test_unknown_gesture_renders_swell_sting(render_of):
    p = dict(sting.FAMILIES["pro_logo_sting"])["swell_sting"]
    assert np.array_equal(render_of(sting, {**p, "gesture": "zzz"}), render_of(sting, p))


@pytest.mark.parametrize("name", [n for n, _ in sting.FAMILIES_R2["logo_sting"]])
def test_reveal_members(render_of, name):
    x = render_of(sting, dict(sting.FAMILIES_R2["logo_sting"])[name])
    assert np.all(np.isfinite(x)) and np.max(np.abs(x)) == pytest.approx(0.89, abs=1e-9)


@pytest.mark.parametrize("family", ["FAMILIES_R2", "FAMILIES_R3", "FAMILIES_R4"])
def test_chirp_members_are_mono(render_of, family):
    for name, p in getattr(sting, family)["pro_logo_sting"]:
        x = render_of(sting, p)
        assert np.array_equal(x[:, 0], x[:, 1]), name


def test_launch_lead_edge_skips_tiny_leads(render_of, p1):
    p = {**sting.LAUNCH, "lead_s": 0.001, "low": 0.0}
    q = {**sting.LAUNCH, "lead": "none", "low": 0.0}
    assert len(render_of(sting, p)) == len(render_of(sting, q))


def test_low_strike_is_deterministic():
    a = sting.low_strike(55, 0.16, 4800)
    assert np.array_equal(a, sting.low_strike(55, 0.16, 4800))
    assert np.max(np.abs(a)) == pytest.approx(1.0)


def test_reexports_from_impact():
    from hark.forge.recipes.impacts import impact

    for name in ("hall_ir", "measure", "study_extra", "study_penalty"):
        assert getattr(sting, name) is getattr(impact, name)
