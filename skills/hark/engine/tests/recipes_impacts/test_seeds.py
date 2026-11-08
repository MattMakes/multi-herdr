"""Tests of part seeds: the impact and subdrop seed files and the seed store (recipes-impacts index 8.17)."""

from __future__ import annotations

import json
import math
import pathlib

import pytest

from hark.forge.recipes.impacts import impact, subdrop

SEEDS = pathlib.Path(impact.__file__).resolve().parents[2] / "seeds" / "impacts"
ENTRY_FIELDS = {"params", "rank_score", "context", "total", "tone_db", "passes", "source"}


def _load(name: str) -> dict:
    return json.loads((SEEDS / f"{name}.json").read_text())


def _six_digits(v) -> bool:
    return not isinstance(v, float) or v == 0 or not math.isfinite(v) or float(f"{v:.6g}") == v


@pytest.mark.parametrize("name,mod", [("impact", impact), ("subdrop", subdrop)])
def test_seed_file_format(name, mod):
    bank = _load(name)
    assert set(bank) == {"recipe", "entries"} and bank["recipe"] == name
    for e in bank["entries"]:
        assert ENTRY_FIELDS <= set(e)
        assert e["context"] == mod.TARGETS[e["delivered"]] + " | "
        assert isinstance(e["passes"], bool)
        assert all(_six_digits(v) for v in e["params"].values())
        assert {"delivered", "member", "seed"} <= set(e)


@pytest.mark.parametrize("name,mod", [("impact", impact), ("subdrop", subdrop)])
def test_seed_file_holds_the_family_tables(name, mod):
    """Pinned entries DERIVE from the family tables, with seeds 0 .. n-1 (S2.9)."""
    by_tag: dict[str, list[dict]] = {}
    for e in _load(name)["entries"]:
        by_tag.setdefault(e["delivered"], []).append(e)
    assert set(by_tag) == set(mod.FAMILIES)
    for cat, members in mod.FAMILIES.items():
        entries = by_tag[cat]
        assert [e["member"] for e in entries] == [n for n, _ in members]
        assert [e["seed"] for e in entries] == list(range(len(members)))
        for e, (_, params) in zip(entries, members):
            assert set(e["params"]) == set(params)
            for k, v in params.items():
                assert e["params"][k] == pytest.approx(v, rel=1e-5), (cat, e["member"], k)


def test_subdrop_seed_file_uses_the_full_space():
    for e in _load("subdrop")["entries"]:
        assert set(e["params"]) == set(subdrop.SPACE)
        assert e["tone_db"] is None


def test_8_17_seed_store(tmp_path):
    search = pytest.importorskip("hark.forge.engine.search")
    target = subdrop.TARGET
    assert search.load_seeds("subdrop", target=target, refs=[], folder=tmp_path) == []
    picks = [{"params": dict(p), "seed": i, "score": {"total": 1.0, "passes": True}, "name": n}
             for i, (n, p) in enumerate(subdrop.FAMILIES["sub_drop"])]
    bank = search.save_seeds("subdrop", picks, target=target, refs=[], source="test", folder=tmp_path,
                             delivered="sub_drop")
    saved = json.loads((tmp_path / "subdrop.json").read_text())
    assert saved["recipe"] == "subdrop" and bank["recipe"] == "subdrop"
    first = saved["entries"][0]
    assert ENTRY_FIELDS <= set(first) and first["delivered"] == "sub_drop"
    loaded = search.load_seeds("subdrop", target=target, refs=[], folder=tmp_path)
    assert loaded[0] == pytest.approx(saved["entries"][0]["params"])
