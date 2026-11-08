"""recipes-impacts index section 8, the shared tests, for the 8 UI recipes.

AT-1 contract and AT-2 determinism run for each recipe and recipe object. This
file also renders every recipe at its space extremes (ticket step 3).
"""

from __future__ import annotations

import importlib

import numpy as np
import pytest

SR = 48000
UI = ("click", "keypress", "typing", "toggle", "shutter", "ui_pop", "popin", "pop")
FORGE_PEAK = 10 ** (-1 / 20)  # the forge normalises every render to -1 dBFS


def load(name: str):
    return importlib.import_module(f"hark.forge.recipes.ui.{name}")


def recipes() -> list[tuple[str, object]]:
    out = [(name, load(name)) for name in UI]
    out.append(("typing.RUN", load("typing").RUN))
    out.append(("pop.V2", load("pop").V2))
    return out


RECIPES = recipes()
IDS = [name for name, _ in RECIPES]


def take(recipe, params: dict, seed: int) -> np.ndarray:
    return recipe.render(dict(params), np.random.default_rng(seed & 0xFFFFFFFF))


def check_audio(y: np.ndarray) -> None:
    assert y.dtype == np.float64
    assert y.ndim == 1 or (y.ndim == 2 and y.shape[1] == 2)
    assert len(y) > 0
    assert np.isfinite(y).all()
    pk = float(np.max(np.abs(y)))
    assert pk > 1e-7
    # Recipes keep the spec level, so a raw render can pass full scale. The forge output must not.
    assert float(np.max(np.abs(forge_level(y)))) <= FORGE_PEAK + 1e-12


def forge_level(y: np.ndarray) -> np.ndarray:
    """The forge's -1 dBFS peak normalisation (ARCH 8.2): recipes do not do it themselves."""
    return y * (FORGE_PEAK / float(np.max(np.abs(y))))


@pytest.mark.parametrize("name,recipe", RECIPES, ids=IDS)
def test_at_1_contract(name, recipe):
    for attr in ("SPACE", "DEFAULTS", "TARGET", "ALTS", "render"):
        assert hasattr(recipe, attr), attr
    assert isinstance(recipe.TARGET, str) and recipe.TARGET
    assert all(isinstance(a, str) for a in recipe.ALTS)
    assert set(recipe.DEFAULTS) <= set(recipe.SPACE) | {"key"}
    check_audio(take(recipe, recipe.DEFAULTS, 1))


@pytest.mark.parametrize("name,recipe", RECIPES, ids=IDS)
def test_at_2_determinism(name, recipe):
    a = take(recipe, recipe.DEFAULTS, 1)
    b = take(recipe, recipe.DEFAULTS, 1)
    c = take(recipe, recipe.DEFAULTS, 2)
    assert np.array_equal(a, b)
    assert a.shape != c.shape or not np.array_equal(a, c)


def test_at_2_seed_mask():
    click = load("click")
    a = click.render(dict(click.DEFAULTS), np.random.default_rng((2 ** 32 + 5) & 0xFFFFFFFF))
    b = click.render(dict(click.DEFAULTS), np.random.default_rng(5))
    assert np.array_equal(a, b)


def test_recipe_objects_carry_module():
    assert load("typing").RUN.MODULE == "hark.forge.recipes.ui.typing"
    assert load("pop").V2.MODULE == "hark.forge.recipes.ui.pop"


def extreme(space: dict, end: int) -> dict:
    out = {}
    for key, spec in space.items():
        if isinstance(spec, list):
            out[key] = spec[0] if end == 0 else spec[-1]
        else:
            out[key] = spec[end]
    return out


@pytest.mark.parametrize("end", [0, 1], ids=["lo", "hi"])
@pytest.mark.parametrize("name,recipe", RECIPES, ids=IDS)
def test_extremes_render_clean(name, recipe, end):
    p = extreme(recipe.SPACE, end)
    if name == "pop.V2":
        for style in recipe.SPACE["style"]:
            check_audio(take(recipe, dict(p, style=style), 3))
        return
    check_audio(take(recipe, p, 3))


def test_pop_v2_extremes_without_natural_paths():
    pop = load("pop")
    for end in (0, 1):
        p = dict(extreme(pop.V2_SPACE, end), n6=0, c_nat=0)
        for style in pop.STYLES:
            check_audio(take(pop.V2, dict(p, style=style), 4))


def n_of(s: float) -> int:
    return max(1, int(round(s * SR)))


def test_duration_click():
    click = load("click")
    assert len(take(click, click.DEFAULTS, 1)) == n_of(0.002 + 0.06)
    p = dict(click.DEFAULTS, rel=0.5, rel_dt=0.09, length=0.03)
    assert len(take(click, p, 1)) == n_of(0.002 + 0.105)


def test_duration_toggle():
    toggle = load("toggle")
    for seed in range(1, 6):
        n = len(take(toggle, toggle.DEFAULTS, seed))
        assert n_of(0.002 + 0.06 * 0.85 + 0.06) <= n <= n_of(0.002 + 0.06 * 1.15 + 0.06)


def test_duration_popin():
    popin = load("popin")
    assert len(take(popin, popin.DEFAULTS, 1)) == n_of(0.002 + 0.18)
    assert len(take(popin, dict(popin.DEFAULTS, length=0.3), 1)) == n_of(0.302)


def test_duration_keypress():
    kp = load("keypress")
    for seed in range(1, 6):
        s = len(take(kp, kp.DEFAULTS, seed)) / SR
        assert 0.006 + 0.0003 + 0.24 * 0.8 < s < 0.006 + 0.007 + 0.24 * 1.2


def test_duration_typing_key():
    ty = load("typing")
    for seed in range(1, 6):
        s = len(take(ty, ty.DEFAULTS, seed)) / SR
        assert 0.075 * 0.8 < s < 0.075 * 1.2


def test_duration_tonal_recipes_capped():
    for name in ("ui_pop", "shutter"):
        mod = load(name)
        for _, p in mod.FAMILIES[name]:
            assert len(take(mod, p, 1)) / SR < 1.0 + 0.6


def test_duration_pop():
    pop = load("pop")
    s = len(take(pop, pop.DEFAULTS, 1)) / SR
    assert 0.0269276 + 0.274425 <= s <= 0.0269276 + 0.012 + 0.274425 + 0.001
    bubble = dict(pop.V2_DEFAULTS, style="bubble", length=0.2)
    assert len(take(pop, bubble, 1)) == n_of(0.2)
    blip = dict(pop.V2_DEFAULTS, style="blip", length=0.03, box=0.02, b_rel=0.003, fade=0.004)
    assert len(take(pop, blip, 1)) == n_of(0.03)
    short = dict(blip, box=0.005, length=0.03)
    assert len(take(pop, short, 1)) == n_of(0.005 + 0.003 + 0.004 + 0.004)
