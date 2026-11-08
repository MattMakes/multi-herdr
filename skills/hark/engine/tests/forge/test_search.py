"""Acceptance tests R1-R12 of forge-core part 7 (search)."""

import json
import math
import types
import warnings

import numpy as np
import pytest

from hark.forge.engine import search


def _rec(tiny, **kw):
    return types.SimpleNamespace(SPACE=dict(tiny.SPACE), DEFAULTS=dict(tiny.DEFAULTS), TARGET=tiny.TARGET,
                                 ALTS=list(tiny.ALTS), render=tiny.render, __name__="tinyrec", **kw)


def test_r1_encode_decode_round_trip():
    space = {"f": (100.0, 10000.0, "log"), "g": (0.0, 2.0), "k": ["a", "b", "c"], "n": (1, 9, "int")}
    p = {"f": 1000.0, "g": 0.5, "k": "b", "n": 4}
    q = search.decode(space, search.encode(space, p))
    assert abs(q["f"] - 1000) < 1e-9 and abs(q["g"] - 0.5) < 1e-12 and q["k"] == "b" and q["n"] == 4
    assert search.decode(space, search.encode(space, {**p, "n": 4.4}))["n"] == 4
    assert search.encode({"x": (0, 1)}, {})[0] == 0.5


def test_r2_fill_start_notes():
    full, notes = search.fill_start({"a": (0, 1)}, {}, {})
    assert full["a"] == 0.5 and notes[0]["from"] == "mid-range"
    full, notes = search.fill_start({"a": (0, 1)}, {}, {"a": 2.0})
    assert full["a"] == 1.0 and notes[0]["from"] == "DEFAULTS clipped" and notes[0]["was"] == 2.0


def test_r3_midrange_warning(tiny, toy_critic):
    rec = _rec(tiny)
    rec.SPACE["extra"] = (0.0, 1.0)
    with pytest.warns(search.MidRangeFill):
        r = search.optimize(rec, toy_critic(), budget=8, pop=8, seed=0)
    assert any(w["from"] == "mid-range" and w["key"] == "extra" for w in r["fill_warnings"])


def _sig(r):
    return [(h["params"], h["seed"], h["score"]["total"]) for h in r["history"]]


def test_r4_deterministic_any_workers(tiny, toy_critic):
    rec = _rec(tiny)
    a = search.optimize(rec, toy_critic(), budget=32, pop=8, seed=1, workers=1)
    b = search.optimize(rec, toy_critic(), budget=32, pop=8, seed=1, workers=1)
    c = search.optimize(rec, toy_critic(), budget=32, pop=8, seed=1, workers=4)
    assert _sig(a) == _sig(b) == _sig(c)
    assert len(a["history"]) == 32


def test_r5_generation_zero_scores_defaults(tiny, toy_critic):
    rec = _rec(tiny)
    r = search.optimize(rec, toy_critic(), budget=16, pop=8, seed=2)
    first = r["history"][0]["params"]
    assert all(first[k] == v for k, v in tiny.DEFAULTS.items())


def test_r6_prescreen_counts(tiny, toy_critic):
    r = search.optimize(_rec(tiny), toy_critic(), budget=16, pop=8, seed=3, prescreen=24)
    assert r["prescreen"] == 24 and r["gens"] == 2
    assert all(g["rendered"] == 24 and g["scored"] == 8 for g in r["generations"])
    assert r["timing"]["rendered"] == 24 * 2


def test_r7_reps_mean(tiny, toy_critic):
    r = search.optimize(_rec(tiny), toy_critic(), budget=8, pop=8, seed=4, reps=2)
    for h in r["history"]:
        s = h["score"]
        assert abs(s["total"] - np.mean(s["total_reps"])) < 1e-12 and len(s["total_reps"]) == 2


def test_r8_robust_seeds(tiny, toy_critic):
    out = search.robust(_rec(tiny), toy_critic(), dict(tiny.DEFAULTS), seeds=4)
    assert out["n"] == 4 and out["seeds"] == [5000, 5001, 5002, 5003]
    assert len(out["totals"]) == 4 and out["mean"] is not None


def test_r9_diverse_min_distance(tiny, toy_critic):
    r = search.optimize(_rec(tiny), toy_critic(), budget=48, pop=16, seed=5)
    picks = search.diverse(r["history"], None, k=6)
    for i, a in enumerate(picks):
        for b in picks[i + 1:]:
            assert np.linalg.norm(a["u"] - b["u"]) / math.sqrt(len(a["u"])) >= 0.08


def _picks(n, offset=0.0):
    return [{"params": {"x": i + offset}, "score": {"total": float(i), "passes": True}} for i in range(n)]


def test_r10_seed_bank_context_and_pins(tmp_path):
    for _ in range(2):
        bank = search.save_seeds("r", _picks(10), target="t", refs=["a.wav"], folder=tmp_path)
    assert len(bank["entries"]) == 6
    pinned = search.save_seeds("p", _picks(10), target="t", refs=["a.wav"], folder=tmp_path, delivered="cat")
    assert len(pinned["entries"]) == 10 and all(e["delivered"] == "cat" for e in pinned["entries"])
    again = search.save_seeds("p", _picks(10, 0.5), target="t", refs=["a.wav"], folder=tmp_path, delivered="cat")
    was = [e for e in again["entries"] if e.get("was_delivered") == "cat"]
    assert was and all("delivered" not in e for e in was)
    assert sum(1 for e in again["entries"] if e.get("delivered") == "cat") == 10
    assert json.loads((tmp_path / "p.json").read_text()) == again


def test_r11_load_seeds_delivered_first(tmp_path):
    search.save_seeds("q", _picks(5), target="t", refs=[], folder=tmp_path)
    search.save_seeds("q", [{"params": {"x": -1.0}, "score": {"total": -5.0}}], target="t", refs=[],
                      folder=tmp_path, delivered="cat")
    search.save_seeds("q", [{"params": {"x": 99.0}, "score": {"total": 50.0}}], target="other", refs=[],
                      folder=tmp_path)
    got = search.load_seeds("q", target="t", refs=[], folder=tmp_path)
    assert got[0] == {"x": -1.0}
    assert {"x": 99.0} not in got[:5]
    assert search.load_seeds("missing", folder=tmp_path) == []


def test_r12_cma_restarts_on_flat_critic(tiny, toy_critic):
    r = search.optimize(_rec(tiny), toy_critic(flat=True), budget=48, pop=8, seed=6, strategy="cma", patience=1)
    assert r["restarts"] >= 1
    assert any(g.get("restart") for g in r["generations"])


def test_render_failure_is_none(capsys):
    def bad(p, r):
        raise ValueError("boom")
    rec = types.SimpleNamespace(render=bad, __name__="badrec")
    assert search.render(rec, {}, 0) is None
    assert search.render(rec, {}, 1) is None
    assert capsys.readouterr().err.count("boom") == 1


def test_prescreen_ranks_against_refs(tiny):
    refs = [search.render(tiny, dict(tiny.DEFAULTS), s) for s in range(3)]
    pre = search.Prescreen(refs, polish=False)
    near = search.render(tiny, dict(tiny.DEFAULTS), 9)
    far = search.render(tiny, {**tiny.DEFAULTS, "freq": 1900.0, "noise": 1.0, "tau": 0.2}, 9)
    rows = pre.score([near, far, None])
    assert rows[0]["pre"] > rows[1]["pre"] > rows[2]["pre"]
    assert pre.nearest(rows[0]["desc"]) in (0, 1, 2)
