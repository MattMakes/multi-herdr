"""recipes-impacts part UI: the tonal recipes ui_pop and shutter."""

from __future__ import annotations

import numpy as np
import pytest

from hark.forge.recipes import tonal
from hark.forge.recipes.ui import shutter, ui_pop

SR = 48000


def median_measure(mod, p: dict, takes: int = 8) -> dict:
    rows = [tonal.measure(mod.render(dict(p), np.random.default_rng(s))) for s in range(1, takes + 1)]
    out = {}
    for key in rows[0]:
        vals = [r[key] for r in rows if r.get(key) is not None]
        out[key] = float(np.median(vals)) if vals else None
    return out


def inside(m: dict, study: dict) -> int:
    return sum(1 for k, (lo, hi, _) in study.items() if m.get(k) is not None and lo <= m[k] <= hi)


# ---------------------------------------------------------------- ui_pop

def test_ui_pop_metadata():
    assert ui_pop.TARGET == "a soft rounded pop, a small bubble popping in a user interface"
    assert ui_pop.ALTS == ["a mouse click", "a water drop", "a wooden knock", "a synth beep", "a finger snap"]
    assert not hasattr(ui_pop, "REFSET")
    assert set(ui_pop.FINE) | {"f0", "bend", "sweep", "m2", "m2r", "dbl", "dgap", "dratio", "jit"} == set(ui_pop.SPACE)


def test_ui_pop_palette():
    assert ui_pop.FAMILY_NAMES["ui_pop"] == ["soft", "step", "boop", "blip", "fall", "wide"]
    soft = dict(ui_pop.FAMILIES["ui_pop"])["soft"]
    assert soft == ui_pop.DEFAULTS
    assert dict(ui_pop.FAMILIES["ui_pop"])["step"]["f0"] == 466.16
    for full in ui_pop.FAMILY["ui_pop"]:
        assert set(full) == set(ui_pop.DEFAULTS)


def test_ui_pop_soft_lands_in_the_film_ranges():
    m = median_measure(ui_pop, ui_pop.DEFAULTS)
    assert inside(m, ui_pop.STUDY) >= 5, m
    assert 400 <= m["centroid_hz"] <= 900


def test_ui_pop_study_penalty_and_extra():
    y = ui_pop.render(dict(ui_pop.DEFAULTS), np.random.default_rng(1))
    pen = ui_pop.study_penalty(y)
    assert pen >= 0
    assert ui_pop.study_extra(y) == pytest.approx(-pen)


def test_ui_pop_family_space_pins_character_keys():
    member = dict(ui_pop.FAMILIES["ui_pop"])["boop"]
    fs = ui_pop.family_space(member)
    for key in ("f0", "bend", "sweep", "m2", "m2r", "dbl"):
        assert fs[key] == [member[key]]
    for key in ui_pop.FINE:
        assert fs[key] == ui_pop.SPACE[key]


def test_ui_pop_double_adds_a_second_pop():
    single = ui_pop.render(dict(ui_pop.DEFAULTS, jit=0, room=0), np.random.default_rng(1))
    double = ui_pop.render(dict(ui_pop.DEFAULTS, jit=0, room=0, dbl=-3, dgap=0.1), np.random.default_rng(1))
    assert len(double) > len(single)


def test_ui_pop_length_cap():
    p = dict(ui_pop.DEFAULTS, tau=0.09, hold=0.03, onset=0.07, dbl=0, dgap=0.12, jit=0, room=0)
    y = ui_pop.render(p, np.random.default_rng(1))
    assert len(y) <= int(0.8 * SR) + 1


def test_ui_pop_sweep_replaces_glide():
    p = dict(ui_pop.DEFAULTS, bend=24, sweep=0.1, jit=0, room=0)
    assert np.isfinite(ui_pop.render(p, np.random.default_rng(1))).all()


# --------------------------------------------------------------- shutter

def test_shutter_metadata_and_tables():
    assert shutter.TARGET == "a camera shutter click"
    assert len(shutter.ALTS) == 5
    assert set(shutter.SPACE) - set(shutter.FINE) == {"gap", "b_hz"}
    assert [r for r, _, _ in shutter.BOX_MODES] == [1.0, 1.62, 2.31, 3.17, 4.4]
    assert [r for r, _ in shutter.LATCH_MODES] == [1.0, 1.71, 2.63]


def test_shutter_palette():
    assert shutter.FAMILY_NAMES["shutter"] == ["dslr", "snap", "mirror", "film", "quick"]
    film = dict(shutter.FAMILIES["shutter"])["film"]
    assert film["a_modes"] == shutter.DEFAULTS["a_modes"]
    assert film["gap"] == 0.095


def test_shutter_dslr_lands_in_the_film_ranges():
    m = median_measure(shutter, dict(shutter.FAMILIES["shutter"])["dslr"])
    assert inside(m, shutter.STUDY) >= 5, m
    assert 624 <= m["centroid_hz"] <= 2187


def _stage_peaks(y: np.ndarray, gap: float) -> tuple[float, float]:
    env = np.abs(y).max(axis=1)
    cut = int(0.75 * gap * SR)
    return float(env[:cut].max()), float(env[cut:].max())


def test_shutter_stage_b_louder_by_default():
    p = dict(shutter.DEFAULTS, whirr=-60, room=0, gap=0.08)
    a, b = _stage_peaks(shutter.render(p, np.random.default_rng(1)), 0.08)
    assert b > a
    p = dict(p, a_db=6, body=-60, b_noise=-40)
    a, b = _stage_peaks(shutter.render(p, np.random.default_rng(1)), 0.08)
    assert a > 0.3 * b


def test_shutter_length_cap():
    p = dict(shutter.DEFAULTS, gap=0.14, b_decay=0.08, body_ms=50, room=0)
    assert len(shutter.render(p, np.random.default_rng(1))) <= SR + 1


def test_shutter_family_space_pins_gap_and_box():
    member = dict(shutter.FAMILIES["shutter"])["mirror"]
    fs = shutter.family_space(member)
    assert fs["gap"] == [member["gap"]] and fs["b_hz"] == [member["b_hz"]]
    assert fs["a_db"] == shutter.SPACE["a_db"]


def test_shutter_study_penalty():
    y = shutter.render(dict(shutter.DEFAULTS), np.random.default_rng(1))
    assert shutter.study_extra(y) == pytest.approx(-shutter.study_penalty(y))
