"""Registry tests and loop tests L1, L10-L14 (the tonal helpers)."""

import types

import numpy as np
import pytest

from hark.forge import recipes as reg
from hark.forge.recipes import tonal

SR = 48000


def _tree(root, layout):
    for folder, mods in layout.items():
        d = root / folder
        d.mkdir(parents=True, exist_ok=True)
        (d / "__init__.py").write_text("")
        for m in mods:
            (d / f"{m}.py").write_text("X = 1\n")


def test_l1_names_skip_helpers(tmp_path, monkeypatch):
    _tree(tmp_path, {"ui": ["a", "_b"], "motion": ["c"]})
    monkeypatch.setattr(reg, "_ROOT", tmp_path)
    assert reg.names() == ["a", "c"]
    assert reg() == ["a", "c"]
    assert reg.folder_of("c") == "motion"
    assert reg.module_path("a") == "hark.forge.recipes.ui.a"


def test_registry_duplicate_names_raise(tmp_path, monkeypatch):
    _tree(tmp_path, {"ui": ["same"], "risers": ["same"]})
    monkeypatch.setattr(reg, "_ROOT", tmp_path)
    with pytest.raises(RuntimeError):
        reg.names()


def test_registry_unknown_and_seed_path(tmp_path, monkeypatch):
    _tree(tmp_path, {"impacts": ["knock"]})
    monkeypatch.setattr(reg, "_ROOT", tmp_path)
    with pytest.raises(reg.UnknownRecipe):
        reg.folder_of("nope")
    assert issubclass(reg.UnknownRecipe, KeyError)
    p = reg.seed_path("knock")
    assert p.parts[-3:] == ("seeds", "impacts", "knock.json")


def test_registry_short_name():
    mod = types.ModuleType("hark.forge.recipes.motion.whoosh")
    assert reg.short_name(mod) == "whoosh"

    def render(p, r):
        return None

    render.__module__ = "hark.forge.recipes.ui.click"
    assert reg.short_name(render) == "click"
    render.__module__ = "somewhere.else"
    assert reg.short_name(render) is None
    obj = types.SimpleNamespace(SPACE={}, MODULE="hark.forge.recipes.ui.pop", render=render)
    assert reg.short_name(obj) == "pop"


def test_l10_env_shape():
    t = np.array([0.0, 0.03, 0.03 + 2.3 * 0.035])
    e = tonal.env(t, 0.03, 0.035)
    assert e[0] == 0 and abs(e[1] - 1) < 1e-12
    assert abs(e[2] - 0.1) < 0.005


def test_l11_glide():
    t = np.array([0.0, 1.0])
    f = tonal.glide(t, 560, 3, 0.02)
    assert abs(f[0] - 560 * 2 ** 0.25) < 1e-9 and abs(f[0] - 665.9) < 0.1
    assert abs(f[1] - 560) < 1e-6


def test_l12_outside():
    assert tonal.outside({"a": 5}, {"a": (0, 1, 2), "b": (0, 1, 1)}) == 3.0


def test_l13_finish_stereo_ends_at_zero():
    t = np.arange(int(0.2 * SR)) / SR
    y = tonal.finish(np.sin(2 * np.pi * 440 * t), np.random.default_rng(0))
    assert y.ndim == 2 and y.shape[1] == 2
    assert np.all(y[0] == 0) and np.all(y[-1] == 0)


def test_l14_measure_decaying_sine():
    t = np.arange(SR) / SR
    m = tonal.measure(np.sin(2 * np.pi * 1000 * t) * np.exp(-t / 0.05))
    assert 105 <= m["t20_ms"] <= 125
    assert 220 <= m["t40_ms"] <= 245
    assert m["attack_ms"] < 10
    assert 900 <= m["centroid_hz"] <= 1100
    assert m["tonal"] > 0.8
    assert m["corr"] == 1.0


def test_tonal_blocks_deterministic():
    def take(seed):
        r = np.random.default_rng(seed)
        n = 4800
        t = np.arange(n) / SR
        e = tonal.env(t, 0.005, 0.02)
        a = tonal.tone(tonal.glide(t, 800, 2), r, [(2.0, 0.3, 1.5), (3.0, 0.0, 1.0)], e)
        b = tonal.follow_noise(n, r, 2000, 4, e)
        c = tonal.burst(n, r, 1000, 8000, 0.002)
        d = tonal.modes(n, r, [500, 1300, 30000], [1, 0.5, 1], [0.05, 0.03, 0.01], jitter=0.01, bend=0.02)
        return a + b + c + d + tonal.thump(n, 90, 0.05)
    assert np.array_equal(take(3), take(3))
    assert not np.array_equal(take(3), take(4))


def test_tonal_room_bypass_draws_nothing():
    r = np.random.default_rng(1)
    s = r.bit_generator.state
    y = np.zeros((100, 2))
    assert tonal.room(y, r, 0.004, 0.2) is y
    assert r.bit_generator.state == s


def test_tonal_family_space():
    space = {"a": (0, 1), "b": (1, 2), "c": [1, 2]}
    assert tonal.family_space(space, {"a": 0.5, "b": 1.5}, ["a"]) == {"a": (0, 1), "b": [1.5]}
