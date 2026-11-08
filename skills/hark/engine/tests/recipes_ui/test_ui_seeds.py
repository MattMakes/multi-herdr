"""recipes-impacts index AT-17 (seed store) and part seeds S1-S2 for the UI seed files."""

from __future__ import annotations

import importlib
import json
import math
from pathlib import Path

import pytest

ENGINE = Path(__file__).resolve().parents[2]
SEEDS = ENGINE / "hark" / "forge" / "seeds" / "ui"
FILES = ("click", "keypress", "pop", "popin", "toggle", "typing")
ENTRY_KEYS = {"params", "rank_score", "context", "total", "tone_db", "passes", "source"}


def load(name: str):
    return importlib.import_module(f"hark.forge.recipes.ui.{name}")


def bank(name: str) -> dict:
    return json.loads((SEEDS / f"{name}.json").read_text())


def sig6(v):
    return float(f"{v:.6g}") if isinstance(v, float) and v != 0 and math.isfinite(v) else v


def registry():
    mod = importlib.import_module("hark.forge.recipes")
    if not hasattr(mod, "names"):
        pytest.skip("the recipe registry (T-16) is not built yet")
    return mod


@pytest.mark.parametrize("name", FILES)
def test_seed_file_format(name):
    b = bank(name)
    assert b["recipe"] == name
    assert b["entries"]
    for e in b["entries"]:
        assert ENTRY_KEYS <= set(e)
        assert e["delivered"] and e["member"] and isinstance(e["seed"], int)
        assert " | " in e["context"]
        assert e["rank_score"] is None and e["passes"] is False
        for v in e["params"].values():
            assert sig6(v) == v


@pytest.mark.parametrize("name", ["click", "toggle", "popin"])
def test_seed_pins_derive_from_families(name):
    mod = load(name)
    entries = bank(name)["entries"]
    expected = [(cat, member, i, {k: sig6(v) for k, v in p.items()})
                for cat, members in mod.FAMILIES.items() for i, (member, p) in enumerate(members)]
    got = [(e["delivered"], e["member"], e["seed"], e["params"]) for e in entries]
    assert got == expected
    for e in entries:
        assert e["context"] == f"{mod.TARGETS[e['delivered']]} | "


def test_seed_pop_palettes():
    pop = load("pop")
    entries = bank("pop")["entries"]
    tags = [e["delivered"] for e in entries]
    assert tags == ["pro_pop"] * 6 + ["pop"] * 5
    assert [e["member"] for e in entries[6:]] == ["pop_01", "pop_02", "pop_03", "pop_04", "pop_05"]
    assert entries[0]["params"]["n6"] == 1 and entries[6]["params"]["style"] == "bubble"
    assert entries[0]["context"] == f"{pop.V2_TARGET} | " and entries[6]["context"] == f"{pop.TARGET} | "


def test_seed_needs_data_fallbacks():
    kp = load("keypress")
    keys = bank("keypress")["entries"]
    assert [e["member"] for e in keys] == [f"typing_key_0{i}" for i in range(1, 7)]
    assert len({e["params"]["key"] for e in keys}) == 6
    ty = load("typing")
    typ = bank("typing")["entries"]
    assert [e["delivered"] for e in typ] == ["pro_typing_key"] * 6 + ["pro_typing_run"] * 6
    assert {e["context"] for e in typ[:6]} == {f"{ty.TARGET} | "}
    assert {e["context"] for e in typ[6:]} == {f"{ty.RUN_TARGET} | "}
    assert set(keys[0]["params"]) == set(kp.DEFAULTS)


@pytest.mark.parametrize("name", FILES)
def test_seed_params_render(name):
    import numpy as np

    mod = load(name)
    for e in bank(name)["entries"][:2]:
        recipe = mod.RUN if e["delivered"] == "pro_typing_run" else mod
        y = recipe.render(dict(e["params"]), np.random.default_rng(e["seed"]))
        assert np.isfinite(y).all()


def test_registry_finds_the_ui_recipes():
    reg = registry()
    names = reg.names()
    for name in ("click", "keypress", "typing", "toggle", "shutter", "ui_pop", "popin", "pop"):
        assert name in names
        assert reg.folder_of(name) == "ui"
    assert "_layers" not in names
    assert reg.seed_path("click") == SEEDS / "click.json"


def test_at_17_seed_store(tmp_path):
    search = pytest.importorskip("hark.forge.engine.search")
    click = load("click")
    target = click.TARGETS["click"]
    assert search.load_seeds("click", target=target, refs=[], folder=tmp_path) == []
    picks = [{"params": p, "score": {"total": -1.0}, "seed": i, "member": n}
             for i, (n, p) in enumerate(click.FAMILIES["click"])]
    search.save_seeds("click", picks, target=target, refs=[], source="test", folder=tmp_path, delivered="click")
    saved = json.loads((tmp_path / "click.json").read_text())
    assert saved["recipe"] == "click"
    assert all(e.get("delivered") == "click" for e in saved["entries"][:5])
    loaded = search.load_seeds("click", target=target, refs=[], folder=tmp_path)
    assert loaded[0] == {k: sig6(v) for k, v in click.FAMILIES["click"][0][1].items()}


def test_at_17_shipped_bank_loads_delivered_first():
    search = pytest.importorskip("hark.forge.engine.search")
    click = load("click")
    entries = bank("click")["entries"]
    loaded = search.load_seeds("click", target=click.TARGETS["click"], refs=[], folder=SEEDS)
    assert loaded[0] == entries[5]["params"]  # the first entry delivered in the click context
    loaded = search.load_seeds("click", target=click.TARGETS["pro_click"], refs=[], folder=SEEDS)
    assert loaded[0] == entries[0]["params"]  # the first entry delivered in the pro_click context
