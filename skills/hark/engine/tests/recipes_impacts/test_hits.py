"""Tests of part hits: cuthit, subdrop, thump, knock (recipes-impacts index 8.4-8.9 and part hits)."""

from __future__ import annotations

import numpy as np
import pytest

from hark.forge.recipes import tonal
from hark.forge.recipes.impacts import cuthit, impact, knock, subdrop

SR = 48000


def _median_measure(mod, params: dict, takes: int = 8) -> dict:
    ms = [tonal.measure(mod.render(dict(params), np.random.default_rng(s))) for s in range(1, takes + 1)]
    out = {}
    for key in ms[0]:
        vals = [m[key] for m in ms if m[key] is not None]
        out[key] = float(np.median(vals)) if vals else None
    return out


def _inside(m: dict, study: dict) -> int:
    return sum(1 for k, (lo, hi, _) in study.items() if m.get(k) is not None and lo <= m[k] <= hi)


# ---- subdrop

def test_8_4_subdrop_length(take):
    pro = dict(dict(subdrop.FAMILIES["sub_drop"])["pro"])
    assert subdrop.seconds_of(pro) == pytest.approx(1.4)
    assert len(take(subdrop.render, pro)) == 67200
    punch = dict(dict(subdrop.FAMILIES["sub_drop"])["punch"])
    assert subdrop.seconds_of(punch) == pytest.approx(0.8)
    assert len(take(subdrop.render, punch)) == 38400


def test_8_5_subdrop_band(take):
    p = {"click": 0, "thump": 0, "grit": 0, "room": 0, "width": 0}
    y = take(subdrop.render, p).mean(axis=1)
    spec = np.abs(np.fft.rfft(y)) ** 2
    f = np.fft.rfftfreq(len(y), 1 / SR)
    ratio = spec[f > 400].sum() / spec.sum()
    assert 10 * np.log10(ratio + 1e-30) <= -60


def test_8_6_subdrop_search_space():
    pro = subdrop.space_for("pro_sub_drop")
    assert set(pro) == {"harm", "harm_lp", "click", "click_f", "click_ms", "length"}
    assert pro["length"] == (0.9, 1.6)
    assert pro["click_ms"] == (15.0, 45.0, "log")
    lib = subdrop.space_for("sub_drop")
    assert len(lib) == 5
    assert all(lib[k] == subdrop.SPACE[k] for k in lib)


def test_subdrop_recipe_for_and_reexports():
    r = subdrop.recipe_for("pro_sub_drop", "ref")
    assert r.PRESET == "ref" and r.REFSET == "pro_sub_drop" and r.TARGET == subdrop.TARGETS["pro_sub_drop"]
    assert r.DEFAULTS == dict(subdrop.FAMILIES["pro_sub_drop"])["ref"]
    assert subdrop.recipe_for("sub_drop", 1).PRESET == "pro"
    with pytest.raises(KeyError):
        subdrop.recipe_for("sub_drop", "nope")
    assert subdrop.measure is impact.measure
    assert subdrop.study_penalty is impact.study_penalty
    assert subdrop.study_extra is impact.study_extra
    assert dict(subdrop.FAMILIES["sub_drop"])["808"] and dict(subdrop.FAMILIES["pro_sub_drop"])["ref"]


def test_subdrop_pro_ref_meets_the_pro_study(take):
    m = impact.measure(take(subdrop.render, dict(subdrop.FAMILIES["pro_sub_drop"])["ref"]))
    assert impact.study_penalty(m, subdrop.STUDY["pro_sub_drop"]) == 0.0


def test_subdrop_harmonics_stop_below_300_hz(take):
    p = {"click": 0, "thump": 0, "grit": 0, "room": 0, "width": 0, "harm": 0.8, "harm_lp": 650, "f0": 220}
    y = take(subdrop.render, p).mean(axis=1)
    spec = np.abs(np.fft.rfft(y)) ** 2
    f = np.fft.rfftfreq(len(y), 1 / SR)
    assert spec[f > 400].sum() / spec.sum() < 1e-6


def test_subdrop_ends_on_a_fade(take):
    y = take(subdrop.render, {})
    assert np.max(np.abs(y[-10:])) < 1e-3


# ---- cuthit

def test_cuthit_family_and_space():
    names = [n for n, _ in cuthit.FAMILIES["cut_hit"]]
    assert names == ["sub", "kick", "punch", "rumble", "air"]
    assert cuthit.DEFAULTS == dict(cuthit.FAMILIES["cut_hit"])["sub"]
    assert cuthit.space_for("cut_hit") == cuthit.SPACE and cuthit.space_for("cut_hit") is not cuthit.SPACE
    assert len(cuthit.SPACE) == 30
    r = cuthit.recipe_for("cut_hit", "rumble")
    assert r.PRESET == "rumble" and r.REFSET == "cut_hit" and r.DEFAULTS["rumble"] == 1.2
    with pytest.raises(KeyError):
        cuthit.recipe_for("cut_hit", "nope")


def test_cuthit_length_and_fade(take):
    for name, p in cuthit.FAMILIES["cut_hit"]:
        y = take(cuthit.render, p)
        assert y.shape[1] == 2
        assert len(y) <= max(1, round(p["length"] * SR)), name
        assert np.max(np.abs(y[-4:])) < 1e-2, name


def test_cuthit_sub_member_matches_the_film_hit(take):
    """The sub member: a brief hit of about 0.26 s with its peak in the first 140 ms, mostly low."""
    m = tonal.measure(take(cuthit.render, cuthit.DEFAULTS))
    assert 0.11 <= m["dur_s"] <= 0.59
    lev = np.abs(take(cuthit.render, cuthit.DEFAULTS)).max(axis=1)
    assert np.argmax(lev) / SR <= 0.137
    assert m["centroid_hz"] <= 598 and m["lo150"] >= 0.25


def test_cuthit_width_decorrelates_the_rumble(take):
    narrow = tonal.measure(take(cuthit.render, {**dict(cuthit.FAMILIES["cut_hit"])["rumble"], "width": 0.0}))
    wide = tonal.measure(take(cuthit.render, dict(cuthit.FAMILIES["cut_hit"])["rumble"]))
    assert narrow["corr"] > wide["corr"]


def test_cuthit_end_cut():
    y = np.zeros((48000, 2))
    y[:4800] = 1.0
    out = cuthit.end_cut(y, -60.0)
    assert len(out) == 4800 + round(0.03 * SR)
    assert len(cuthit.end_cut(np.ones((500, 2)))) == 500


# ---- thump

def test_8_7_thump_off_equals_the_pass_engine(take):
    whoosh = pytest.importorskip("hark.forge.recipes.motion.whoosh")
    from hark.forge.recipes.impacts import thump

    q = {**thump.BASE, "thump_db": -40.0}
    x = np.asarray(whoosh.render({k: v for k, v in q.items() if k not in thump.THUMP_KEYS},
                                 np.random.default_rng(1)), dtype=np.float64)
    want = x * 0.89 / (np.max(np.abs(x)) + 1e-12)
    assert np.allclose(take(thump.render, {"thump_db": -40.0}), want, atol=1e-12)


def test_thump_presets_and_recipe():
    pytest.importorskip("hark.forge.recipes.motion.whoosh")
    from hark.forge.recipes.impacts import thump

    assert thump.THUMP_KEYS == ("thump_db", "thump_f0", "thump_f1", "thump_tau", "thump_lead")
    assert [n for n, _ in thump.FAMILIES["whoosh_big"]] == ["thump", "thump_deep", "thump_airy"]
    deep = thump.preset("thump_deep")
    assert deep["f_peak"] == 150 and deep["hold"] == thump.BASE["hold"]
    assert thump.preset_full("whoosh_big", {"rise": 0.2})["rise"] == 0.2
    r = thump.recipe("whoosh_big", "thump_airy")
    assert r.PRESET == "thump_airy" and r.DEFAULTS == thump.preset("thump_airy") and r.REFSET == "whoosh_big"
    assert set(thump.THUMP_KEYS) <= set(r.SPACE)
    with pytest.raises(KeyError):
        thump.preset("nope")
    assert thump.peak_time({"rise": 0.13, "start_db": -30, "rise_k": 0.8}) == pytest.approx(0.13)


def test_thump_take_independent_of_thump_level(take):
    pytest.importorskip("hark.forge.recipes.motion.whoosh")
    from hark.forge.recipes.impacts import thump

    a = take(thump.render, {"thump_db": -40.0})
    b = take(thump.render, {"thump_db": -6.0})
    assert a.shape == b.shape


# ---- knock

def test_8_8_knock_profile():
    wood = knock.FAMILY["knock"][knock.FAMILY_NAMES["knock"].index("wood")]
    assert _inside(_median_measure(knock, wood), knock.STUDY) >= 5


def test_8_9_knock_family_space():
    wood = knock.FAMILY["knock"][0]
    space = knock.family_space(wood)
    for key in ("mat", "f1", "bend", "body", "jit"):
        assert space[key] == [wood[key]]
    assert len(knock.FINE) == 24
    for key in knock.FINE:
        assert space[key] == knock.SPACE[key]


def test_knock_family_tables():
    assert knock.FAMILY_NAMES["knock"] == ["wood", "plastic", "snap", "desk", "double"]
    assert all(set(m) == set(knock.SPACE) for m in knock.FAMILY["knock"])
    assert knock.FAMILY["knock"][4]["mat"] == "tok"


def test_knock_unknown_material(take):
    with pytest.raises(ValueError):
        take(knock.render, {"mat": "glass"})


def test_knock_length_cap():
    p = {**knock.DEFAULTS, "decay": 0.2, "slope": -0.4, "body": 6, "body_ms": 60, "flam": -2, "bounce": -2}
    assert knock.length_of(p) == 1.0
    assert knock.length_of(knock.DEFAULTS) < 1.0


def test_knock_high_modes_skipped():
    freqs, _, _ = knock._main_modes({**knock.DEFAULTS, "mat": "desk", "f1": 4000})
    assert all(f < 0.42 * SR for f in freqs)
    assert len(freqs) < len(knock.MATERIALS["desk"])


def test_knock_bounce_adds_a_second_contact(take):
    base = {**knock.DEFAULTS, "jit": 0, "room": 0, "width": 0, "bounce_ms": 40}
    off = take(knock.render, {**base, "bounce": -60})
    on = take(knock.render, {**base, "bounce": -2})
    k = round(0.045 * SR)
    assert np.abs(on[k:k + 960]).max() > 3 * np.abs(off[k:k + 960]).max()
