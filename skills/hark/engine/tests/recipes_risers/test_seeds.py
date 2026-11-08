"""The riser-folder seed banks: AT29 (every delivered entry renders through the wrapper) and AT30 (DERIVE)."""

from __future__ import annotations

import json
import pathlib

import numpy as np
import pytest

from hark.forge.recipes.risers import _seedbank

SEEDS = pathlib.Path(_seedbank.__file__).resolve().parents[2] / "seeds" / "risers"
ENTRY_KEYS = {"params", "rank_score", "context", "total", "tone_db", "passes", "source", "delivered", "member",
              "seed"}


def _bank(name: str) -> dict:
    return json.loads((SEEDS / f"{name}.json").read_text())


def _cases() -> list[tuple[str, int]]:
    return [(bank, i) for bank in _seedbank.BANKS for i in range(len(_bank(bank)["entries"]))]


@pytest.mark.parametrize("bank", _seedbank.BANKS)
def test_seed_file_matches_the_tables(bank):
    """The shipped JSON is exactly what the builder makes from the preset tables."""
    assert _bank(bank) == json.loads(json.dumps(_seedbank.build(bank)))


@pytest.mark.parametrize("bank", _seedbank.BANKS)
def test_seed_file_format(bank):
    data = _bank(bank)
    assert data["recipe"] == bank
    for e in data["entries"]:
        assert set(e) == ENTRY_KEYS
        assert e["delivered"] and e["rank_score"] is None and e["context"].endswith(" | ")


@pytest.mark.parametrize("bank,index", _cases())
def test_at_29_delivered_entries_render(bank, index, risers, wrapper):
    e = _bank(bank)["entries"][index]
    y = wrapper(risers[bank], e["params"], e["seed"])
    assert y is not None
    assert 20 * np.log10(np.max(np.abs(y))) == pytest.approx(-1.0, abs=1e-6)


def test_at_30_derive_notify_bank(tmp_path, risers, render_of):
    paths = _seedbank.write_all(tmp_path)
    assert (tmp_path / "notify.json") in paths
    data = json.loads((tmp_path / "notify.json").read_text())
    entries = data["entries"]
    assert len(entries) == 11 and all(e["delivered"] for e in entries)
    pro = [e["member"] for e in entries if e["delivered"] == "pro_notify"]
    assert pro == ["ding_1822", "ding_1930", "ding_2045", "ding_2167", "ding_2296"]
    assert [e["member"] for e in entries if e["delivered"] == "notify"] == [f"notify_{i:02d}" for i in range(1, 7)]
    for e in entries:
        x = render_of(risers["notify"], e["params"])
        assert x.shape[1] == 2 and x.shape[0] >= 480 and np.all(np.isfinite(x)) and np.max(np.abs(x)) >= 1e-4


def test_bank_counts():
    counts = {bank: len(_bank(bank)["entries"]) for bank in _seedbank.BANKS}
    assert counts == {"riser": 10, "downlifter": 6, "glitch": 7, "notify": 11, "sting": 10}
    riser = _bank("riser")["entries"]
    assert [e["params"].get("r6", 0.0) for e in riser[:5]] == [0.0, 1.0, 1.0, 0.0, 1.0]
    assert {e["seed"] for e in _bank("glitch")["entries"] if e["delivered"] == "glitch"} == {3}


def test_params_rounded_to_6_significant_digits():
    assert _seedbank._round6(1930.3378) == 1930.34
    assert _seedbank._round6(0.000123456789) == 0.000123457
    assert _seedbank._round6("glide") == "glide"
