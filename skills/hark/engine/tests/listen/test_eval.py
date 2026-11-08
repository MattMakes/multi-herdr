"""EVS-1 .. EVS-12: the held-out evaluation tools (metrics, threshold rule, bands, find checks, scenes)."""

from __future__ import annotations

import hashlib
import json
import math
import os

import numpy as np
import pytest

from hark.listen.eval import dataset as ds
from hark.listen.eval import metrics as mt
from hark.listen.eval import run_eval as re_
from hark.listen.eval import scenes

SR = 48000


def test_evs_1_split():
    for fid in ("abc", "k01", "rain_long", "ü-é"):
        want = "dev" if int(hashlib.sha1(fid.encode("utf-8")).hexdigest(), 16) % 2 == 0 else "test"
        assert mt.split_of(fid) == want
    assert mt.split_of("abc") == mt.split_of("abc")


def test_evs_2_average_precision():
    assert mt.average_precision([0.9, 0.8, 0.7, 0.6], [1, 0, 1, 0]) == pytest.approx((1 + 2 / 3) / 2, abs=1e-4)
    assert mt.average_precision([0.9, 0.8], [0, 0]) is None
    assert mt.mean_or_none([None, 0.5, 1.0]) == 0.75
    assert mt.precision_at([0.9, 0.8, 0.7], [1, 0, 1], 2) == 0.5


def test_evs_3_auc_rank_sum():
    assert mt.auc_rank_sum([3, 2], [1, 0]) == 1.0
    assert mt.auc_rank_sum([0], [1]) == 0.0
    assert mt.auc_rank_sum([2, 0], [1, 1.5]) == 0.5
    assert mt.auc_rank_sum([], [1]) is None and mt.auc_rank_sum([1], []) is None
    assert mt.auc_pairwise([1], [1]) == 0.5


def test_evs_4_lenient_and_strict_labels():
    cols = ["enter", "spacebar", "rain"]
    assert mt.label_row(["enter"], cols, lenient=True) == [1, 0, -1]
    assert mt.label_row(["enter"], cols, lenient=False) == [1, -1, -1]
    assert mt.label_row([], cols, True) == [0, 0, 0] == mt.label_row([], cols, False)
    assert mt.label_matrix([["enter"], []], cols).tolist() == [[1, 0, -1], [0, 0, 0]]
    assert "enter" not in mt.left_out(["enter"]) and "soft_landing" in mt.left_out(["fruit_drop"])


def test_evs_5_tagging_uses_u():
    cols = ["a", "b", "c"]
    u = np.array([[0.30, 0.20, 0.10]])
    z = (u - u.mean()) / u.std() * 3 + 1
    assert mt.tagging(u, [["b"]], cols) == mt.tagging(z, [["b"]], cols)
    t = mt.tagging(u, [["b"]], cols)
    assert mt.tag_rank(u[0], [1]) == 2 and t["accuracy"] == 0 and t["mrr"] == 0.5


def _pairs(rows, n_pos=100, n_neg=10000):
    pos, neg = [], []
    for z, tp, fp in rows:
        pos += [z] * tp
        neg += [z] * fp
    pos += [0.0] * (n_pos - len(pos))
    neg += [0.0] * (n_neg - len(neg))
    return np.array(pos), np.array(neg)


def test_evs_6_threshold_rule():
    pos, neg = _pairs([(3.2, 50, 33), (2.7, 20, 80), (2.2, 15, 250), (1.7, 10, 1000)])
    assert mt.pick_thresholds(pos, neg) == {"strong": 3.0, "plausible": 2.5, "weak": 2.0}
    assert mt.band_lr(pos, neg, 3.0, math.inf) == pytest.approx(151.5, abs=0.1)
    assert mt.band_lr(pos, neg, 2.5, math.inf) == pytest.approx(61.9, abs=0.1)
    assert mt.band_lr(pos, neg, 2.5, 3.0) == pytest.approx(25)
    assert mt.band_lr(pos, neg, 2.0, 3.0) == pytest.approx(10.6, abs=0.1)
    assert mt.band_lr(pos, neg, 2.0, 2.5) == pytest.approx(6)
    assert mt.band_lr(pos, neg, 1.5, 2.5) == pytest.approx(2.0)
    pos, neg = _pairs([(3.2, 50, 33), (2.7, 10, 100), (2.2, 15, 250), (1.7, 10, 1000)])
    assert mt.pick_thresholds(pos, neg) == {"strong": 3.0, "plausible": None, "weak": 2.0}
    assert mt.band_lr(pos, neg, 2.0, 3.0) == pytest.approx(7.1, abs=0.05)
    assert mt.band_lr(pos, neg, 1.5, 3.0) == pytest.approx(2.6, abs=0.05)


def test_evs_6_lr_edges():
    assert mt.lr_of(3, 0, 10, 10) == math.inf and mt.lr_of(0, 0, 10, 10) == 0


def test_evs_7_band_table():
    pos = np.array([3.5] * 5 + [0.0] * 5)
    neg = np.array([3.5] * 2 + [0.0] * 998)
    t = mt.band_table(pos, neg)
    strong = t["bands"][0]
    assert t["n_pos"] == 10 and t["n_neg"] == 1000 and t["base_rate"] == pytest.approx(10 / 1010)
    assert strong["lr"] == 250 and strong["precision"] == pytest.approx(0.714, abs=1e-3)
    assert strong["precision_even_odds"] == pytest.approx(0.996, abs=1e-3)
    assert strong["recall_at_or_above"] == 0.5
    assert mt.precision_at_base(250, 0.03) == pytest.approx(0.885, abs=1e-3)
    assert [b["band"] for b in t["bands"]] == ["strong", "plausible", "weak", "no match"]
    assert t["bands"][1]["lr"] is None and t["bands"][3]["recall_at_or_above"] == 1.0
    grid = mt.cumulative_grid(pos, neg)
    assert len(grid) == 25 and grid[0]["tp"] == 10


def test_evs_8_key_press_hit():
    attacks = [1.000, 2.000]
    ok, idx, lead = mt.key_hit((1.990, 2.500), attacks)
    assert ok and idx == 1 and lead == pytest.approx(0.010)
    assert not mt.key_hit((0.930, 1.500), attacks)[0]
    assert mt.key_hit((0.995, 2.004), attacks)[0]
    assert not mt.key_hit((0.995, 2.010), attacks)[0]
    rep = mt.key_report([(1.990, 2.5), (0.995, 2.004), (5.0, 5.2), (9.5, 9.6)], attacks,
                        unlabelled=[(5.0, 6.0)], skips=[(9.3, 10.45)])
    assert rep["hits"] == 2 and rep["presses_hit"] == 2 and rep["unlabelled"] == 1 and rep["in_skip"] == 1


def test_evs_8_attack_time():
    x = np.zeros(1000)
    x[300], x[500] = 0.05, 1.0
    assert mt.attack_time(x, 2.0, 1000) == pytest.approx(2.5)
    x[400] = 0.1
    assert mt.attack_time(x, 0.0, 1000) == pytest.approx(0.4)


def _tone(seconds=3.0, seed=0):
    return np.random.default_rng(seed).standard_normal(int(seconds * SR)) * 0.01


def _shot(seed):
    rng = np.random.default_rng(seed)
    t = np.arange(int(0.8 * SR)) / SR
    x = np.concatenate([np.zeros(int(0.1 * SR)), 0.4 * rng.standard_normal(len(t)) * np.exp(-t / 0.1)])
    return x


def test_evs_9_scene_builder_s1():
    shots = [_shot(k) for k in range(8)]
    for s in shots:
        y = scenes.one_shot(s)
        assert scenes.peak_db(y) == pytest.approx(-6.0, abs=0.1) and len(y) <= 2.5 * SR
    x, starts = scenes.build_s1(_tone(), shots)
    assert len(x) / SR == pytest.approx(44.0) and starts == [2.0 + 5 * k for k in range(8)]
    quiet = np.concatenate([x[int((s + 3.0) * SR):int((s + 4.9) * SR)] for s in starts[:-1]])
    assert scenes.rms_db(quiet) == pytest.approx(-55.0, abs=0.5)
    for s in starts:
        seg = np.abs(x[int((s - 0.02) * SR):int((s + 0.2) * SR)])
        onset = (np.argmax(seg >= 0.05 * seg.max())) / SR - 0.02
        assert 0.0 <= onset <= 0.02


def test_evs_9_scene_builder_s2():
    events = [t for t, _ in scenes.S2_EVENTS]
    starts = scenes.key_starts(events)
    assert starts[0] == 1.0 and starts[-1] <= 298.0
    for t in starts:
        assert not any(e - 0.7 <= t < e + 2.6 for e in events)
    gaps = np.round(np.diff(starts[:4]), 6).tolist()
    assert gaps == [1.3, 1.4, 1.5]
    keys = [_shot(1)[: int(0.2 * SR)], _shot(2)[: int(0.2 * SR)]]
    shots = {"ting": _shot(3), "pop": _shot(4), "whoosh": _shot(5)}
    x, ks, planted = scenes.build_s2(_tone(), keys, shots)
    assert len(x) == 300 * SR and ks == starts and planted == {"ting": [20.0, 150.0, 275.0], "pop": [205.0],
                                                             "whoosh": [240.0]}


def test_scene_found():
    segs = [{"start": 1.98, "end": 2.5, "kind": "stroke"}, {"start": 5.0, "end": 10.0, "kind": "window"}]
    r = mt.scene_found(segs, [2.0, 7.0])
    assert r["top1"] and r["n_found"] == 2 and r["tight"] == 1 and r["leads_ms"] == [30.0]
    assert not mt.scene_found(segs[1:], [2.0])["top1"]


def test_calibrate_variant_matches_vocab(fake_clap):
    from hark.listen import vocabulary as voc

    v = voc.get()
    rng = np.random.default_rng(0)
    A = voc.unit(rng.standard_normal((4, 512)))
    Q = v.embed_text(["a glass ting", "a dog bark"])
    cal = v.calibrate(A, Q)
    m = v.mean if v.mean is not None else np.zeros(512)
    _, z = mt.calibrate_variant(A, v.E, Q, "both", m)
    assert np.allclose(z, cal.z_q, atol=1e-6)
    u, z = mt.calibrate_variant(A, v.E, Q, "raw")
    assert np.allclose(u, cal.Cq) and np.allclose(z, cal.Cq)
    with pytest.raises(ValueError):
        mt.calibrate_variant(A, v.E, Q, "nope")


def test_mood_share():
    r = mt.mood_share(["mysterious music", "happy and upbeat music"], 16)
    assert r["share"] == 0.5 and r["chance"] == 5 / 16


def test_label_files():
    q = ds.read_label("queries.json")
    assert set(q["categories"]) == set(mt.CATEGORIES)
    assert [c["id"] for c in q["classes"]] == list(mt.CLASSES)
    assert all(len(c["phrasings"]) == 2 for c in q["classes"]) and len(q["specific"]) == 24
    assert all(set(s["classes"]) <= set(mt.CLASSES) for s in q["specific"])
    k = ds.read_label("kind_queries.json")
    assert len(k["test"]["event"]) == 30 and len(k["test"]["texture"]) == 20
    assert ds.read_label("heldout_labels.json")["music_class"] == "music"


def test_missing_location_names_the_variable(monkeypatch, tmp_path, run_cli, capsys):
    monkeypatch.delenv("HARK_EVAL_ASSETS", raising=False)
    with pytest.raises(ds.EvalError, match="HARK_EVAL_ASSETS"):
        ds.location("HARK_EVAL_ASSETS", local=tmp_path / "none.json")
    monkeypatch.setattr(ds, "LOCAL_PATHS", tmp_path / "none.json")
    code = re_.main(["--out", str(tmp_path), "analyze"])
    assert code == 1 and "HARK_EVAL_ASSETS" in capsys.readouterr().err


def test_part_flag():
    assert re_.parse_part("2/3") == (2, 3)
    for bad in ("0/2", "3/2", "x", "1/0"):
        with pytest.raises(ds.EvalError):
            re_.parse_part(bad)


def test_kind_counts():
    labels = {"_doc": "", "test": {"event": ["a click", "rain"], "texture": ["wind"]}}
    reads = {"a click": "event", "rain": "texture", "wind": "either"}
    r = re_.kind_counts(labels, reads.get)
    assert r["test"]["event"] == {"n": 2, "event": 1, "either": 0, "texture": 1, "wrong": ["rain"]}
    assert r["test"]["texture"]["either"] == 1 and r["test"]["texture"]["wrong"] == []


# ---------------------------------------------------------------- a tiny labelled library, stub CLAP


@pytest.fixture
def assets(tmp_path, write, monkeypatch):
    rng = np.random.default_rng(4)
    needs = {}
    for c, cat in enumerate(mt.CATEGORIES):
        for i in range(2):
            f = 300 + 150 * c + 40 * i
            x = 0.3 * np.sin(2 * np.pi * f * np.arange(SR) / SR) + 0.02 * rng.standard_normal(SR)
            write(f"assets/{cat}/{cat}{i}.wav", x)
            write(f"assets/heldout/{cat}/h_{cat}{i}.wav", x[::-1].copy())
            needs[f"{cat}{i}"] = [["rain", "spacebar", "ting", "plunge", "music", "lego", "tree", "typing"][c]]
    (tmp_path / "assets" / "manifests").mkdir()
    (tmp_path / "assets" / "manifests" / "m.json").write_text(json.dumps(needs))
    labels = {"music_class": "music", "files": {f"h_{k}": v for k, v in needs.items()}}
    monkeypatch.setattr(ds, "read_label", lambda name, _orig=ds.read_label: labels if name == "heldout_labels.json"
                        else _orig(name))
    monkeypatch.setenv("HARK_EVAL_ASSETS", str(tmp_path / "assets"))
    monkeypatch.setenv("HARK_EVAL_REFS", str(tmp_path / "assets" / "heldout" / "music"))
    return tmp_path / "assets"


def test_libraries(assets):
    q = ds.read_label("queries.json")
    tune = ds.tuning_library(assets, q)
    assert len(tune.paths) == 16 and tune.classes[0] == ["rain"] and tune.classes[8] == ["music"]
    held = ds.heldout_library(assets, ds.read_label("heldout_labels.json"))
    assert len(held.paths) == 16 and held.folders[0] == "toy"
    assert len(tune.split("dev").paths) + len(tune.split("test").paths) == 16


def test_heldout_needs_every_label(assets, monkeypatch):
    with pytest.raises(ds.EvalError, match="h_toy0"):
        ds.heldout_library(assets, {"files": {}})


def test_analyze_and_heldout_run(assets, tmp_path, fake_clap, capsys):
    out = tmp_path / "out"
    assert re_.main(["--out", str(out), "analyze"]) == 0
    res = json.loads((out / "results.json").read_text())
    assert len(res["cells"]) == 3 * len(mt.METHODS)
    shipped = res["shipped"]
    assert set(shipped["thresholds"]) == {"strong", "plausible", "weak"}
    assert set(shipped["reference_sensitivity"]) == {"folder_balanced", "uniform", "without_music_like",
                                                     "transductive", "no_term_bias"}
    assert len(shipped["bias_top10"]) == 10 and len(shipped["bias_bottom5"]) == 5
    assert "/" not in json.dumps(res["cells"][0]["splits"]["all"]["specific"]["queries"][0]["best"])
    assert re_.main(["--out", str(out), "heldout"]) == 0
    held = json.loads((out / "heldout.json").read_text())
    assert set(held["vocabularies"]) == {"shipped", "raw"} and held["mood"]["n"] == 4
    assert str(tmp_path) not in (out / "heldout.json").read_text()
    assert len(capsys.readouterr().out.splitlines()) >= 18


# ---------------------------------------------------------------- [model] checks


@pytest.mark.ml
def test_evs_10_heldout_targets(tmp_path):
    if not os.environ.get("HARK_EVAL_ASSETS"):
        pytest.skip("needs-data: a held-out library built by the NEEDS-DATA fallback (HARK_EVAL_ASSETS)")
    assert re_.main(["--out", str(tmp_path), "heldout"]) == 0
    held = json.loads((tmp_path / "heldout.json").read_text())
    assert held["vocabularies"]["shipped"]["auc"] >= 0.95
    strong = held["bands"]["lenient"]["bands"][0]
    assert strong["lr"] is None or strong["lr"] >= 100


@pytest.mark.ml
def test_evs_11_query_kind(tmp_path):
    from hark.listen import search, vocabulary as voc

    v = voc.get()
    r = re_.kind_counts(ds.read_label("kind_queries.json"),
                        lambda t: search.query_kind(v, v.embed_text([t])[0])[0])
    assert r["tune"]["event"]["event"] == r["tune"]["event"]["n"]
    assert r["test"]["event"]["texture"] <= 0.1 * r["test"]["event"]["n"]
    assert r["test"]["texture"]["event"] == 0


@pytest.mark.ml
def test_evs_12_find_scenes(tmp_path):
    root = os.environ.get("HARK_EVAL_ASSETS")
    if not root or not (ds.Path(root) / "find" / "checks.json").exists():
        pytest.skip("needs-data: one-shots and a room tone in HARK_EVAL_ASSETS/find/checks.json")
    from hark.listen.eval import find_checks

    res = find_checks.run_all(ds.Path(root) / "find", tmp_path / "scenes")
    assert res["S1"]["top1"] >= 7
    assert res["S2"]["found"] == 5 and res["S2"]["tight"] >= 4


def test_find_checks_run_with_stub(tmp_path, write, fake_clap):
    from hark.listen import vocabulary as voc
    from hark.listen.eval import find_checks

    keys = np.zeros(4 * SR)
    slices = []
    for k in range(3):
        a = 0.5 + k
        keys[int(a * SR):int(a * SR) + int(0.15 * SR)] += _shot(10 + k)[int(0.1 * SR):int(0.25 * SR)]
        slices.append([a - 0.05, a + 0.4])
    write("find/keys.wav", keys + 1e-4 * np.random.default_rng(0).standard_normal(len(keys)))
    write("find/room.wav", _tone(2.0))
    for name in ("a", "b", "c"):
        write(f"find/{name}.wav", _shot(ord(name)))
    cfg = {"K1": {"file": "keys.wav", "slices": slices}, "K2": {"file": "keys.wav", "slices": slices},
           "K3": {"file": "missing.wav", "slices": slices}, "room_tone": "room.wav",
           "s1_shots": ["a.wav", "b.wav", "c.wav"] * 2 + ["a.wav", "b.wav"],
           "s2_shots": {"ting": "a.wav", "pop": "b.wav", "whoosh": "c.wav"}}
    (tmp_path / "find" / "checks.json").write_text(json.dumps(cfg))
    orig = voc.span_embeddings
    res = find_checks.run_all(tmp_path / "find", tmp_path / "out", pad="zero")
    assert voc.span_embeddings is orig
    assert "error" in res["K3"] and "missing.wav" in res["K3"]["error"]
    assert res["K1"]["segments"] >= 1 and "lead_range_ms" in res["K1"]
    assert len(res["S1"]["queries"]) == 8 and len(res["S2"]["queries"]) == 3
    assert (tmp_path / "out" / "s1.wav").exists() and (tmp_path / "out" / "s2.wav").exists()
    assert find_checks.row_text(res["S1"]).startswith("top-1")


def test_real_effects_run_with_stub(tmp_path, write, fake_clap, capsys):
    from hark.listen.eval import real_effects as rx

    for n in range(3):
        write(f"sfx/s{n}.wav", _shot(20 + n))
    lib = {"jobs": {"pop": {"description": "the cork leaves the bottle", "phrasings": ["a cork pop", "a small pop"]},
                    "tap": {"description": "a tap on glass", "phrasings": ["a glass ting", "a bright tap"]},
                    "half": {"description": "no phrasings yet", "phrasings": ["one"]}},
           "sounds": [{"file": "s0.wav", "duration": 0.9, "title": "Cork Pop", "jobs": ["pop"]},
                      {"file": "s1.wav", "duration": 0.9, "title": "Glass Tap ", "jobs": ["tap", "half"]},
                      {"file": "s2.wav", "duration": 1.2, "title": "cork pop", "jobs": []}]}
    p = tmp_path / "sfx" / "library.json"
    p.write_text(json.dumps(lib))
    assert rx.main([str(p), "--out", str(tmp_path / "out")]) == 0
    r = json.loads((tmp_path / "out" / "real_effects.json").read_text())
    assert r["n_jobs"] == 2 and r["n_short"] == 2 and set(r) >= set(rx.voc.PADS)
    rp = r["repeatpad"]
    assert sum(rp["title"]["verdicts"].values()) == 3 and rp["union"]["n"] == 2
    assert len(capsys.readouterr().out.splitlines()) == 12
