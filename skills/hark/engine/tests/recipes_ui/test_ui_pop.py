"""recipes-impacts part pop: pop (versions 1 and 2) and popin, plus index AT-18 (mono output)."""

from __future__ import annotations

import importlib

import numpy as np
import pytest

from hark.forge.recipes.ui import pop, popin

SR = 48000


def rng(seed: int = 1) -> np.random.Generator:
    return np.random.default_rng(seed)


def has(name: str) -> bool:
    try:
        importlib.import_module(name)
    except ModuleNotFoundError:
        return False
    return True


# ------------------------------------------------------------- pop: v1

def test_pop_metadata():
    assert pop.TARGET == "a small bubble pop" and pop.REFSET == "pop"
    assert pop.V2.TARGET == "a short tonal pop" and pop.V2.REFSET == "pro_pop"
    assert pop.V2.ALTS == ["a mouse click", "a laser zap", "a synth beep", "a finger snap"]
    assert "cut_db" not in pop.DEFAULTS


def test_pop_dispatch_by_style():
    v1 = pop.render(dict(pop.DEFAULTS), rng())
    assert v1.ndim == 1
    v2 = pop.render(dict(pop.V2_DEFAULTS, style="bubble"), rng())
    assert v2.ndim == 2


def test_pop_v1_families_merge_over_space():
    fs = pop.v1_family_space("bloop")
    assert fs["f0"] == (350, 900, "log") and fs["q"] == (18, 40, "log")
    assert fs["hiss_f"] == pop.SPACE["hiss_f"]
    with pytest.raises(KeyError):
        pop.v1_family_space("nope")


def test_pop_v1_rupture_has_zero_net_area():
    pulse = pop.rupture(1e-4, 200)
    assert pulse.sum() == pytest.approx(0.0, abs=1e-12)
    assert pulse[:5].max() > 0 > pulse.min()


def test_pop_v1_tear_crackle_counts_jerks():
    one = pop.tear_crackle(1e-4, 1e-3, 1, 480, rng())
    many = pop.tear_crackle(1e-4, 1e-3, 6, 480, rng())
    assert np.count_nonzero(many) > np.count_nonzero(one)


def test_pop_v1_early_round_path_without_clip():
    p = {k: v for k, v in pop.DEFAULTS.items() if k not in ("clip", "hp", "lp", "cut_fade")}
    y = pop.render(p, rng())
    assert np.isfinite(y).all() and y.ndim == 1


def test_pop_v1_cut_waits_for_the_ring():
    early = pop.render(dict(pop.DEFAULTS, clip=0.14), rng())
    waited = pop.render(dict(pop.DEFAULTS, clip=0.14, cut_db=-56, q=40), rng())
    assert len(waited) >= len(early)
    assert len(waited) <= int((pop.DEFAULTS["pre"] + 0.012 + 0.42) * SR) + 2


def test_pop_v1_kit_length():
    y = pop.render(dict(pop.DEFAULTS), rng())
    assert 0.19 <= len(y) / SR <= 0.41


# ------------------------------------------------------------- pop: v2

def test_pop_v2_space_and_fine_keys():
    assert pop.V2_SPACE["style"] == ["chirp", "blip", "bubble"]
    assert set(pop.V2_DEFAULTS) == set(pop.V2_SPACE)
    for style, keys in pop.FINE.items():
        assert set(keys) <= set(pop.V2_SPACE), style


def test_pop_v2_family_space():
    member = dict(pop.FAMILIES["pro_pop"])["boop"]
    fs = pop.family_space(member)
    assert set(fs) == set(pop.V2_SPACE)
    assert fs["n_f0"] == [1200] and fs["ease"] == pop.V2_SPACE["ease"]
    fs = pop.family_space({"f0": 900}, style="bubble")
    assert fs["f0"] == [900] and fs["breath"] == pop.V2_SPACE["breath"]
    assert fs["length"] == [pop.V2_DEFAULTS["length"]]
    with pytest.raises(ValueError):
        pop.family_space({}, style="laser")


def test_pop_v2_palettes():
    assert [n for n, _ in pop.FAMILIES["pro_pop"]] == ["chirp", "boop", "bloop", "double", "tink", "thup"]
    assert [n for n, _ in pop.FAMILIES["pop"]] == ["pop_01", "pop_02", "pop_03", "pop_04", "pop_05"]
    for _, p in pop.FAMILIES["pro_pop"]:
        assert p["n6"] == 1 and p["style"] == "chirp" and set(p) == set(pop.V2_DEFAULTS)
    for _, p in pop.FAMILIES["pop"]:
        assert p["style"] == "bubble" and p["tick_hz"] == 1700
    assert [n for n, _ in pop.ROUND5_FAMILY] == ["chirp", "mouth", "blip", "drop", "plip", "droplet"]
    assert [n for n, _ in pop.ROUND4_FAMILY] == ["m0", "m1", "m2", "m3", "m4", "m5"]
    assert dict(pop.ROUND4_FAMILY)["m1"]["c_air"] == -20
    assert dict(pop.ROUND4_FAMILY)["m2"]["style"] == "blip"


@pytest.mark.parametrize("family", ["pro_pop", "pop", "round5", "round4"])
def test_pop_v2_members_render(family):
    members = {"round5": pop.ROUND5_FAMILY, "round4": pop.ROUND4_FAMILY}.get(family) or pop.FAMILIES[family]
    for name, p in members:
        y = pop.render(p, rng())
        assert np.isfinite(y).all() and np.max(np.abs(y)) > 1e-7, name


def test_pop_v2_harmonics_fade_near_18k():
    ph = np.linspace(0, 100, 4800)
    low = pop.harmonics(ph, np.full(4800, 1000.0), 0.5, 0.5)
    high = pop.harmonics(ph, np.full(4800, 9000.0), 0.5, 0.5)
    assert np.allclose(high, np.sin(ph))
    assert not np.allclose(low, np.sin(ph))


@pytest.mark.parametrize("ms", [1.5, 6.0])
def test_pop_v2_click_peak(ms):
    c = pop.click(4800, -6.0, 3000, rng(), ms)
    assert np.max(np.abs(c)) == pytest.approx(10 ** (-6 / 20))
    assert np.all(c[int(0.004 * SR) + 1:] == 0)


def test_pop_v2_stepped_chirp_falls():
    p = dict(pop.V2_DEFAULTS, pre_db=-40, tick_db=-40, drive=0, steps=0, harm=0)
    y = pop.render(p, rng())
    m = y if y.ndim == 1 else y.mean(axis=1)
    half = len(m) // 3
    zc_early = np.count_nonzero(np.diff(np.signbit(m[:half])))
    zc_late = np.count_nonzero(np.diff(np.signbit(m[2 * half:])))
    assert zc_early > zc_late


def test_pop_v2_unknown_style():
    with pytest.raises(ValueError):
        pop.render({"style": "laser"}, rng())


def test_at_18_mono_output():
    p = dict(pop.V2_DEFAULTS, style="chirp", c_room=0.0, n6=0, c_nat=0)
    y = pop.render(p, rng())
    assert y.ndim == 1
    dsp = importlib.import_module("hark.forge.engine.dsp")
    st = dsp.stereo_of(y)
    assert st.shape == (len(y), 2) and np.array_equal(st[:, 0], st[:, 1])
    if not has("hark.forge.engine.search"):
        pytest.skip("hark.forge.engine.search (T-16) is not built yet: the forge render check waits for it")
    search = importlib.import_module("hark.forge.engine.search")
    out = search.render(pop.V2, p, 1)
    assert out is not None and np.isfinite(out).all()


def test_pop_v2_penalty():
    pytest.importorskip("hark.forge.recipes.risers.notify")
    y = pop.render(dict(pop.FAMILIES["pro_pop"])["boop"], rng())
    pen = pop.pro_penalty(y, "pro_pop")
    assert 0 <= pen <= 8
    assert pop.extra_for("pro_pop", 2.0)(y) == pytest.approx(-2 * pen)
    assert pop.pro_penalty(y, "click") == 0.0


def test_pop_v2_penalty_without_targets_needs_no_notify():
    assert pop.pro_penalty(np.zeros(100), "toggle") == 0.0


# ---------------------------------------------------------------- popin

def test_popin_metadata():
    assert popin.TARGETS["pro_tap"] == popin.TARGETS["tap"] == "a soft low thump of a finger touching a screen"
    assert "a kick drum" not in popin.ALTS
    assert popin.REFSET == "pro_pop_in"


def test_popin_families():
    assert [n for n, _ in popin.FAMILIES["pro_pop_in"]] == ["round", "double", "knock", "deep", "airy"]
    assert [n for n, _ in popin.FAMILIES["pro_tap"]] == ["finger", "glass", "soft", "knock", "double"]
    assert [n for n, _ in popin.FAMILIES["tap"]] == ["finger", "glass", "table", "knuckle", "soft"]
    assert popin.family_space("tap", "knuckle")["att_ms"] == (0.8, 1.6, "log")
    for _, p in popin.FAMILIES["pro_tap"]:
        assert p["drop"] <= 0.15 and p["sub"] <= 0.2


def test_popin_family_members_render():
    for cat, members in popin.FAMILIES.items():
        for name, p in members:
            y = popin.render(p, rng())
            assert np.isfinite(y).all() and np.max(np.abs(y)) <= 10 ** (-1 / 20) + 1e-9, (cat, name)


def test_popin_low_thump_centroid():
    y = popin.render(dict(popin.DEFAULTS, tick_db=-34), rng())
    m = y.mean(axis=1)
    pw = np.abs(np.fft.rfft(m)) ** 2
    f = np.fft.rfftfreq(len(m), 1 / SR)
    assert np.sum(f * pw) / np.sum(pw) < 400


def test_popin_haas_width_keeps_correlation():
    y = popin.render(dict(popin.DEFAULTS, width=1.2, drop=0, harm=0, sub=0, tick_db=-34), rng())
    assert np.corrcoef(y[:, 0], y[:, 1])[0, 1] >= 0.25
    for cat in ("pro_pop_in", "pro_tap"):
        for name, p in popin.FAMILIES[cat]:
            y = popin.render(p, rng())
            assert 0.38 <= np.corrcoef(y[:, 0], y[:, 1])[0, 1] <= 0.78, (cat, name)
    mono = popin.render(dict(popin.DEFAULTS, width=0.0, tick_width=0.0, tick_db=-34), rng())
    assert np.allclose(mono[:, 0], mono[:, 1])


def test_popin_contact_normalises_first():
    y = popin.render(dict(popin.DEFAULTS, contact_db=3), rng())
    assert np.max(np.abs(y)) <= 10 ** (-1 / 20) + 1e-9


def test_popin_old_sets_without_round3_keys():
    p = {k: v for k, v in popin.DEFAULTS.items() if not k.startswith(("contact_", "ring_"))}
    y = popin.render(p, rng())
    assert np.isfinite(y).all()


def test_popin_double_and_ring():
    y = popin.render(dict(popin.DEFAULTS, dbl=0.8, ring_db=-12, knock=0.4, knock_f=300, air_db=-24), rng())
    assert np.isfinite(y).all()
