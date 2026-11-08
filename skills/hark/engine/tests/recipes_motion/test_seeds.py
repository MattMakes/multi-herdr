"""The motion seed banks: AT29 and the DERIVE fallback (recipes-motion index, "Data assets")."""

from __future__ import annotations

import importlib
import json
from pathlib import Path

import numpy as np
import pytest

SEEDS = Path(__file__).resolve().parents[2] / "hark" / "forge" / "seeds" / "motion"
BANKS = {"whoosh": 18, "swish": 5, "swipe": 5, "zip": 5, "shimmer": 11, "hover": 10}
KEYS = {"params", "rank_score", "context", "total", "tone_db", "passes", "source",
        "delivered", "member", "seed"}


def bank(name: str) -> dict:
    return json.loads((SEEDS / f"{name}.json").read_text())


def derive(name: str, load) -> dict:
    load(name)
    return importlib.import_module("hark.forge.recipes.motion._seeds").derive(name)


@pytest.mark.parametrize("name", sorted(BANKS))
def test_bank_file_format(name):
    data = bank(name)
    assert data["recipe"] == name
    assert len(data["entries"]) == BANKS[name]
    for e in data["entries"]:
        assert KEYS <= set(e)
        assert e["source"] == "derived" and e["seed"] == 0 and e["rank_score"] is None
        assert e["context"].endswith(" |")


@pytest.mark.parametrize("name", sorted(BANKS))
def test_bank_file_equals_derive(name, load):
    assert bank(name) == json.loads(json.dumps(derive(name, load)))


def test_bank_members():
    assert [e["member"] for e in bank("zip")["entries"]] == ["up_hi", "up", "arc", "down", "down_warm"]
    shimmer = [e["member"] for e in bank("shimmer")["entries"]]
    assert shimmer == [f"shimmer_{i:02d}" for i in range(1, 7)] + [f"pro_shimmer_{i:02d}" for i in range(1, 6)]
    cats = {e["delivered"] for e in bank("whoosh")["entries"]}
    assert cats == {"whoosh_fast", "whoosh_big", "pro_whoosh_big"}


def test_derived_banks_write_to_a_folder(tmp_path, load):
    for name in BANKS:
        load(name)
    seeds = importlib.import_module("hark.forge.recipes.motion._seeds")
    paths = seeds.write(tmp_path / "seeds")
    assert sorted(p.name for p in paths) == sorted(f"{n}.json" for n in BANKS)
    assert json.loads(paths[0].read_text())["recipe"] == paths[0].stem


@pytest.mark.parametrize("name", sorted(BANKS))
def test_at_29_bank_entries_render_through_the_wrapper(name, load):
    module = load(name)
    pytest.importorskip("hark.forge.engine.search", reason="needs the forge wrapper (T-16)")
    search = importlib.import_module("hark.forge.engine.search")
    for e in bank(name)["entries"]:
        x = search.render(module, e["params"], e["seed"])
        assert x is not None, e["member"]
        peak_db = 20 * np.log10(np.max(np.abs(x)))
        assert peak_db == pytest.approx(-1.0, abs=0.01), e["member"]
