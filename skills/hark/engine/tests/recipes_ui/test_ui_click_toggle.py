"""recipes-impacts part UI: the shared layers L1-L9, click and toggle."""

from __future__ import annotations

import math

import numpy as np
import pytest

from hark.forge.recipes.ui import _layers as L
from hark.forge.recipes.ui import click, toggle

SR = 48000


def rng(seed: int = 1) -> np.random.Generator:
    return np.random.default_rng(seed)


def spectral_centroid(y: np.ndarray) -> float:
    m = y.mean(axis=1) if y.ndim == 2 else y
    pw = np.abs(np.fft.rfft(m)) ** 2
    f = np.fft.rfftfreq(len(m), 1 / SR)
    return float(np.sum(f * pw) / np.sum(pw))


# ---------------------------------------------------------------- L1-L9

def test_l1_band_burst_length_and_peak():
    y = L.band_burst(0.001, rng(), 2000, 8000)
    assert len(y) == L.n_of(max(0.006, 0.004))
    assert abs(np.max(np.abs(y)) - 1) < 1e-9
    assert len(L.band_burst(0.0002, rng(), 2000, 8000)) == L.n_of(0.004)


def test_l2_partials_peak_and_onset():
    y = L.partials(3000, (1.0, 1.62), (1.0, 0.55), 0.005, 0.05, rng(), rise=0.001)
    assert len(y) == L.n_of(0.05)
    assert abs(np.max(np.abs(y)) - 1) < 1e-9
    assert y[0] == 0.0


def test_l3_designed_thump_glides_down():
    y = L.designed_thump(200, 0.2, 0.002, 0.01, 0.03, drop=0.5, harm=0.0)
    assert abs(np.max(np.abs(y)) - 1) < 1e-9
    zc = np.nonzero(np.diff(np.signbit(y)))[0]
    early = np.diff(zc[:6]).mean()
    late = np.diff(zc[-6:]).mean()
    assert early < late


def test_l4_end_gate_cuts_and_falls_to_zero():
    x = np.ones((1000, 2))
    y = L.end_gate(x, 0.01, 0.002, start=0.002)
    assert len(y) == L.n_of(0.012)
    assert y[0, 0] == 1.0 and y[-1, 0] == 0.0
    assert np.all(np.diff(y[:, 0]) <= 1e-12)


def test_l5_finish_chain_high_pass_order():
    x = np.zeros(4800)
    x[0] = 1.0
    y = L.finish_chain(x, hp=1000)
    lo = np.abs(np.fft.rfft(y))[np.fft.rfftfreq(4800, 1 / SR) < 200]
    assert lo.max() < 0.01


def test_l6_tiny_space_dry_returns_mono_pair():
    x = rng().standard_normal(480)
    st = L.tiny_space(x, rng(), 0.001)
    assert st.shape == (480, 2) and np.array_equal(st[:, 0], x) and np.array_equal(st[:, 1], x)
    wet = L.tiny_space(x, rng(), 0.2, width=0.1)
    assert wet.shape == (480, 2) and not np.array_equal(wet[:, 0], wet[:, 1])


def test_l7_send_bus_holds_only_broadband():
    p = dict(click.DEFAULTS, grit=0.0)
    main, snd = L.click_layers(p, rng(), send=True)
    assert abs(np.max(np.abs(main)) - 1) < 1e-9
    assert np.max(np.abs(snd)) < np.max(np.abs(main)) + 1e-12
    assert np.all(snd[L.n_of(0.02):] == pytest.approx(0, abs=1e-6))


def test_l8_room_tail_off_returns_input():
    st = np.ones((100, 2))
    assert L.room_tail(st, np.ones(100), rng(), dict(click.DEFAULTS, verb=0.0)) is st
    out = L.room_tail(st, np.ones(100), rng(), dict(click.DEFAULTS, verb=0.2))
    assert len(out) > 100


def test_l9_narrowing_and_centre():
    space = {"a": (1, 100, "log"), "b": (0.0, 1.0), "n": (1, 3, "int"), "c": ["x", "y"]}
    box = {"a": (10, 40, "log"), "b": (0.2, 0.4), "n": (2, 3), "c": "y"}
    narrow = L.narrow_space(space, box)
    assert narrow == {"a": (10, 40, "log"), "b": (0.2, 0.4), "n": (2, 3, "int"), "c": ["y"]}
    centre = L.box_centre({"a": 5.0, "b": 0.5, "n": 1, "c": "x"}, box)
    assert centre["a"] == pytest.approx(20.0) and centre["b"] == pytest.approx(0.3)
    assert centre["n"] == 2 and isinstance(centre["n"], int) and centre["c"] == "y"


# ----------------------------------------------------------------- click

def test_click_metadata():
    assert click.TARGET == "a clean bright user interface click"
    assert click.TARGETS["pro_click"] == "a crisp high digital interface tick for a product video"
    assert click.REFSET == "click"
    assert [r.rsplit("/", 1)[1] for r in click.REFS] == [f"ui_click_0{i}.wav" for i in range(1, 5)]


def test_click_ref_root(monkeypatch):
    monkeypatch.setenv("HARK_REFS", "C:\\refs\\")
    assert L.ref_root() == "C:/refs"


def test_click_families_names():
    assert [n for n, _ in click.FAMILIES["pro_click"]] == ["tink", "snap", "mouse", "glass", "soft"]
    assert [n for n, _ in click.FAMILIES["click"]] == ["soft", "thock", "tink", "snap", "double"]


def test_click_thock_space_leaves_full_minimum():
    fs = click.family_space("click", "thock")
    assert fs["body_f"] == (120, 260, "log") and fs["hp"] == (50, 100, "log")
    assert click.SPACE["body_f"][0] <= 120 and click.SPACE["hp"][0] <= 50


def test_click_family_errors():
    with pytest.raises(KeyError):
        click.family_space("click", "nope")
    with pytest.raises(KeyError):
        click.family_params("nope", "soft")


def test_click_family_members_render_in_their_box():
    for cat, members in click.FAMILIES.items():
        for name, p in members:
            box = click.BOXES[cat][name]
            for key, v in box.items():
                if isinstance(v, tuple):
                    assert v[0] <= p[key] <= v[1], (cat, name, key)
                else:
                    assert p[key] == v
            y = click.render(p, rng())
            assert np.isfinite(y).all() and y.shape[1] == 2


def test_click_pro_tick_is_bright_and_short():
    for name in ("tink", "snap", "glass"):
        p = dict(click.FAMILIES["pro_click"])[name]
        y = click.render(p, rng())
        assert 5000 <= spectral_centroid(y) <= 10000, name
        assert len(y) / SR <= 0.002 + 0.06


def test_click_mouse_has_a_low_body():
    p = dict(click.FAMILIES["pro_click"])["mouse"]
    assert spectral_centroid(click.render(p, rng())) < 2000


def test_click_attack_at_lead():
    y = click.render(dict(click.DEFAULTS), rng())
    env = np.abs(y).max(axis=1)
    ip = int(np.argmax(env))
    assert L.n_of(0.002) <= ip <= L.n_of(0.002 + 0.003)
    assert np.max(np.abs(y[: L.n_of(0.0015)])) < 1e-6


def test_click_direct_sound_is_mono_without_space():
    p = dict(click.DEFAULTS, room=0.0, verb=0.0)
    y = click.render(p, rng())
    assert np.allclose(y[:, 0], y[:, 1])


# ---------------------------------------------------------------- toggle

def test_toggle_space_has_no_click_edit_keys():
    assert not {"rel", "rel_dt", "length"} & set(toggle.SPACE)
    assert toggle.SPACE["hp_steep"] == [0, 1]


def test_toggle_families():
    assert [n for n, _ in toggle.FAMILIES["toggle"]] == ["on", "off", "latch", "micro", "rocker"]
    for name in ("on", "off", "latch", "micro", "rocker"):
        assert toggle.family_space("toggle", name)["hp_steep"] == [1]
        assert toggle.family_params("toggle", name)["hp_steep"] == 1
    assert dict(toggle.FAMILIES["toggle"])["on"]["dir"] > 1 > dict(toggle.FAMILIES["toggle"])["off"]["dir"]
    assert dict(toggle.FAMILIES["toggle"])["rocker"]["a_lvl"] < 1


def _contact_peaks(y: np.ndarray, gap: float) -> tuple[float, float]:
    env = np.abs(y).max(axis=1)
    pre = L.n_of(0.002)
    mid = pre + L.n_of(gap / 2)
    return float(env[:mid].max()), float(env[mid:].max())


def test_toggle_loud_contact_a_survives_limiter():
    p = dict(toggle.DEFAULTS, a_lvl=2.5, slide=0.0, verb=0.0, room=0.0, spring=0.0)
    a, b = _contact_peaks(toggle.render(p, rng()), p["gap"])
    assert a > b
    p = dict(p, a_lvl=0.2)
    a, b = _contact_peaks(toggle.render(p, rng()), p["gap"])
    assert a < b


def test_toggle_peak_level():
    y = toggle.render(dict(toggle.DEFAULTS), rng())
    assert np.max(np.abs(y)) <= 10 ** (-1 / 20) + 1e-9


def test_toggle_end_gate_after_contact_b():
    for seed in range(1, 4):
        n = len(toggle.render(dict(toggle.DEFAULTS, tail=0.1), rng(seed)))
        assert n > L.n_of(0.002 + 0.1)
        assert math.isclose(n / SR, 0.002 + 0.06 + 0.1, rel_tol=0.2)


@pytest.mark.parametrize("steep", [0, 1])
def test_toggle_spring_paths(steep):
    p = dict(toggle.DEFAULTS, spring=0.08, hp_steep=steep)
    y = toggle.render(p, rng())
    assert np.isfinite(y).all() and np.max(np.abs(y)) < 0.999
