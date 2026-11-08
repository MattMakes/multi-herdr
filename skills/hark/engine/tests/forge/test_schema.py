"""Acceptance tests S3-S8 of forge-core part 6 (S1 and S2 run in test_loop.py)."""

import json
import math

import numpy as np
import pytest
import soundfile as sf

from hark.forge.engine import schema

TINY = "forgetest_tiny"


def _folder(tmp_path, tiny, seeds=(1, 2), with_block=True):
    picks = []
    for i, s in enumerate(seeds):
        b = schema.make_block(tiny.render, dict(tiny.DEFAULTS), s)
        name = f"{TINY}_{i + 1:02d}"
        sf.write(str(tmp_path / f"{name}.wav"), schema.rerender(b), 48000, subtype="PCM_24")
        pick = {"name": name, "params": dict(tiny.DEFAULTS), "seed": s, "file": str(tmp_path / f"{name}.wav")}
        if with_block:
            pick["render"] = b
        picks.append(pick)
    rep = {"recipe": TINY, "picks": picks}
    if with_block:
        rep["schema"] = 2
    (tmp_path / "report.json").write_text(json.dumps(rep, indent=1))
    return rep


def test_s3_edited_seed_fails(tmp_path, tiny, capsys):
    _folder(tmp_path, tiny)
    assert all(r["ok"] for r in schema.check_final(tmp_path))
    rep = json.loads((tmp_path / "report.json").read_text())
    rep["picks"][0]["render"]["seed"] = 99
    (tmp_path / "report.json").write_text(json.dumps(rep))
    rows = schema.check_final(tmp_path)
    assert not rows[0]["ok"] and rows[0]["err"] > 1e-3 and rows[1]["ok"]
    assert schema.main([str(tmp_path)]) == 1
    assert "1 / 2 re-render" in capsys.readouterr().out


def test_s4_missing_wav(tmp_path, tiny):
    _folder(tmp_path, tiny)
    (tmp_path / f"{TINY}_02.wav").unlink()
    row = schema.check_final(tmp_path, workers=1)[1]
    assert row["why"] == "WAV missing" and row["err"] == math.inf and not row["ok"]


def test_s5_make_block():
    assert schema.make_block(lambda p, r: None, {}, 0) is None
    b = schema.make_block(None, {}, 0, ref=("m", "render"))
    assert b["gain_db"] == 0.0 and "gain" not in b
    assert set(b) == set(schema.BLOCK_KEYS) - {"gain"}
    g = schema.make_block(None, {}, 0, gain=0.5, ref=("m", "render"))
    assert g["gain_db"] == -6.02 and g["gain"] == 0.5


def test_s6_fill_only_when_identical(tiny, monkeypatch):
    params = {k: v for k, v in tiny.DEFAULTS.items()}
    b = schema.make_block(tiny.render_k, params, 3)
    assert b["fn"] == "render_k"
    assert schema.fill(b)["params"]["k"] == 1.0
    monkeypatch.setattr(tiny, "K_DEFAULTS", {"k": 2.0})
    assert "k" not in schema.fill(b)["params"]


def test_s7_upgrade_writes_only_when_all_match(tmp_path, tiny):
    _folder(tmp_path, tiny, with_block=False)
    rep, rows = schema.upgrade(tmp_path, write=True, log=None)
    assert all(r["ok"] for r in rows)
    saved = json.loads((tmp_path / "report.json").read_text())
    assert saved["schema"] == 2 and all("render" in p for p in saved["picks"])

    bad = tmp_path / "bad"
    bad.mkdir()
    _folder(bad, tiny, with_block=False)
    rep = json.loads((bad / "report.json").read_text())
    rep["picks"][1]["seed"] = 77
    text = json.dumps(rep)
    (bad / "report.json").write_text(text)
    with pytest.raises(schema.NotReproducible):
        schema.upgrade(bad, write=True, log=None)
    assert (bad / "report.json").read_text() == text


def test_s8_compare_shapes():
    assert schema.compare(np.zeros(10), np.zeros((10, 2))) == math.inf
    assert schema.compare(np.zeros(10), np.zeros((10, 1))) == 0.0


def test_resolve_unknown_is_not_reproducible():
    with pytest.raises(schema.NotReproducible):
        schema.resolve({"recipe": "no_such_recipe", "fn": "render"})
