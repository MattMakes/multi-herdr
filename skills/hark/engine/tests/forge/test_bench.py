"""Benchmark tests: B1-B15 (eval/bench.py, frozen_tone.py, scrub_paths.py, build_refsets.py)."""

from __future__ import annotations

import json
import math
import types

import numpy as np
import pytest
import soundfile as sf

from hark.forge.engine.eval import bench, build_refsets, frozen_tone, scrub_paths

SR = 48000


def _decay(dur=0.5, f=1000.0, tau=0.05, phase=0.0):
    t = np.arange(int(dur * SR)) / SR
    return np.cos(2 * np.pi * f * t + phase) * np.exp(-t / tau)


def _fades(x, ms=10):
    n = int(SR * ms / 1000)
    y = x.copy()
    y[:n] *= np.linspace(0, 1, n)
    y[-n:] *= np.linspace(1, 0, n)
    return y


def _wav(path, x):
    sf.write(str(path), x, SR, subtype="PCM_24")
    return str(path)


# ---------------------------------------------------------------- model-free checks

def test_B1_env_feats_burst():
    n_att = int(0.002 * SR)
    t = np.arange(int(0.6 * SR)) / SR
    tau = 0.2 / (2 * math.log(10))  # amplitude down 40 dB in 200 ms
    burst = np.sin(2 * np.pi * 1000 * t) * np.exp(-t / tau)
    burst[:n_att] *= np.linspace(0, 1, n_att)
    x = np.concatenate([np.zeros(int(0.01 * SR)), burst])
    f = bench.env_feats(x)
    assert f["onset_s"] == pytest.approx(0.010, abs=0.001)
    assert 1 <= f["attack_ms"] <= 3
    assert 195 <= f["t40_ms"] <= 210


def test_B2_artifact_flags():
    # A decaying sine: a constant sine with 10 ms fades still has its last 10 ms within 30 dB of the peak.
    x = 0.5 * _decay(phase=0.0)
    assert "start click" in bench.artifacts(x)["flags"]
    assert bench.artifacts(_fades(x))["flags"] == []
    assert "dc offset" in bench.artifacts(_fades(x) + 0.2)["flags"]
    clip = _fades(x)
    clip[1000:1005] = 1.0
    assert "clipped" in bench.artifacts(clip)["flags"]
    t = np.arange(int(0.5 * SR)) / SR
    rising = np.sin(2 * np.pi * 1000 * t) * (t / t[-1]) ** 2
    rising[: int(0.01 * SR)] *= np.linspace(0, 1, int(0.01 * SR))
    flags = bench.artifacts(rising)["flags"]
    assert "end click" in flags and "tail cut" in flags
    assert bench.artifacts(rising, cat="riser")["flags"] == []


def test_B3_late_start():
    x = np.concatenate([np.zeros(int(0.15 * SR)), _fades(0.5 * _decay())])
    assert "late start" in bench.artifacts(x)["flags"]
    assert "late start" not in bench.artifacts(x, cat="pro_riser")["flags"]


def test_B4_diversity():
    x = _fades(_decay())
    E = np.ones((3, 4))
    d = bench.diversity([x, x, x], E)
    assert d["cos_mean"] == 1.0 and len(d["near_duplicates"]) == 3
    assert bench.diversity([x], E[:1]) == {"n": 1}


def test_B5_inside_real():
    real = [{"tone_db": v, "typ_z": z} for v, z in zip([4, 6, 8, 10, 12], [0, 1, 2, 3, 4])]
    got = bench.inside_real([{"tone_db": 5}], real, metrics=(("tone_db", "low"),))
    assert got["limits"]["tone_db"] == pytest.approx(10.4) and got["per_sound"] == [1]
    hi = bench.inside_real([{"typ_z": 1.0}], real, metrics=(("typ_z", "high"),))
    assert hi["limits"]["typ_z"] == pytest.approx(0.8) and hi["per_sound"] == [1]


def test_B6_envref_self_check():
    x = _fades(_decay())
    ref = bench.EnvRef([x])
    c = ref.check(x)
    assert c["env_oct"] == 0 and c["curve_db"] == 0
    assert ref.check(x, exclude=0) == {}


def test_B7_yardstick_typ_z_needs_3_refs(monkeypatch):
    monkeypatch.setattr(bench, "_typicality", lambda e, A, k: 1.25)
    xs = [_fades(_decay(tau=t)) for t in (0.03, 0.05, 0.08)]
    E = np.eye(3)
    two = bench.Yardstick(["a", "b"], xs[:2], E[:2, :]).check(xs[2], E[2])
    assert "typ_z" not in two and "tone_db" in two and "env_oct" in two
    three = bench.Yardstick(["a", "b", "c"], xs, E).check(xs[0], E[0])
    assert three["typ_z"] == 1.25


def test_B8_gather(tmp_path):
    x = _fades(_decay())
    (tmp_path / "click").mkdir()
    for name in ("click_01.wav", "audition.wav", "_tmp.wav"):
        _wav(tmp_path / "click" / name, x)
    (tmp_path / "pop" / "final").mkdir(parents=True)
    _wav(tmp_path / "pop" / "pop_01.wav", x)
    _wav(tmp_path / "pop" / "final" / "pop_02.wav", x)
    got = bench.gather(tmp_path)
    assert [p.rsplit("/", 1)[-1] for p in got["click"]] == ["click_01.wav"]
    assert [p.rsplit("/", 1)[-1] for p in got["pop"]] == ["pop_02.wav"]
    more = bench.gather(None, [f"tap={tmp_path}/click/*.wav"])
    assert [p.rsplit("/", 1)[-1] for p in more["tap"]] == ["click_01.wav"]


# ---------------------------------------------------------------- main

@pytest.fixture
def registry(tmp_path, monkeypatch):
    path = tmp_path / "refsets.json"
    path.write_text(json.dumps({"version": 1, "categories": {"click": {"forge": [], "bench": []}}}))
    monkeypatch.setenv("HARK_REFSETS", str(path))

    def no_models():
        raise AssertionError("models must not load")

    monkeypatch.setattr(bench, "_load_models", no_models)
    return path


def test_B9_phrase_clash_stops_before_models(registry, tiny, monkeypatch, tmp_path):
    monkeypatch.setattr(tiny, "TARGET", "  A Crisp Tiny PLASTIC click ")
    assert "a crisp tiny plastic click" in bench.recipe_phrases()
    with pytest.raises(SystemExit) as e:
        bench.main(["--out", str(tmp_path / "r.json"), "--real-only"])
    assert "a crisp tiny plastic click" in str(e.value)


def test_B10_unknown_made_category(registry, tmp_path, monkeypatch):
    monkeypatch.setattr(bench, "recipe_phrases", lambda: set())
    (tmp_path / "made" / "nope").mkdir(parents=True)
    _wav(tmp_path / "made" / "nope" / "a.wav", _fades(_decay()))
    with pytest.raises(SystemExit) as e:
        bench.main(["--out", str(tmp_path / "r.json"), "--made", str(tmp_path / "made")])
    assert "nope" in str(e.value) and "click" in str(e.value) and "whoosh_fast" in str(e.value)


def test_B11_retable(tmp_path, capsys):
    res = {"tag": "t", "made_from": "m", "registry": "r", "created": "2026-11-07 10:00",
           "categories": {"click": {"n_bench": 2, "n_forge": 1, "note": "a note", "made": [],
                                    "summary": {"made": {"n": 2, "tone_db": 5.0}, "real_cross": {"n": 1},
                                                "real_loo": {"n": 2}},
                                    "inside": {"per_sound": [3, 4], "n_checks": 7}, "real_per_sound": [5]},
                          "pop": {"n_bench": 0, "n_forge": 0, "note": "", "made": []}}}
    out = tmp_path / "results_t.json"
    out.write_text(json.dumps(res))
    before = out.read_text()
    assert bench.main(["--out", str(out), "--retable"]) == 0
    md = out.with_suffix(".md").read_text()
    assert out.read_text() == before
    assert md.count("| click | made | 2 |") == 1 and md.count("| click | real x | 1 |") == 1
    assert md.count("| click | real loo | 2 |") == 1 and "3.5 / 7 (real 5)" in md
    assert "| pop | no rows |" in md and "click note: a note" in md
    assert "wrote" in capsys.readouterr().out


# ---------------------------------------------------------------- frozen tone

def test_B12_frozen_t40_and_profile():
    rng = np.random.default_rng(0)
    t = np.arange(int(0.8 * SR)) / SR
    tau = 0.3 / (2 * math.log(10))
    x = rng.standard_normal(len(t)) * np.exp(-t / tau)
    assert frozen_tone.t40_ms(x) == pytest.approx(300, abs=10)
    assert frozen_tone.t40_ms(np.ones(int(0.009 * SR))) == 0.0
    p = frozen_tone.profile(x)
    assert len(p) == 28 and np.all(p >= -60)
    assert np.allclose(frozen_tone.profile(0.1 * x), p)


def test_frozen_toneref_check():
    xs = [_fades(_decay(f=f)) for f in (800, 1000, 1200)]
    ref = frozen_tone.ToneRef(xs)
    c = ref.check(xs[0], exclude=0)
    assert c["tone_ratio"] is not None and "tone" in c["words"] and ref.spread is not None
    assert frozen_tone.ToneRef([]).check(xs[0]) == {}


# ---------------------------------------------------------------- scrub

def test_B13_scrub_paths(monkeypatch, tmp_path):
    monkeypatch.setenv("HARK_HOME", str(tmp_path / "home"))
    repo = str(scrub_paths.REPO).replace("\\", "/")
    got = scrub_paths.scrub({"f": f"{repo}/out/refs/click/bench/a.wav", "g": f"see {repo}/hark/x.py, ok",
                             f"{tmp_path}/home/emb/x": 1, "r": repo})
    assert got["f"] == "<refs>/click/bench/a.wav"
    assert got["g"] == "see hark/x.py, ok" and got["r"] == "."
    assert "<HARK_HOME>/emb/x" in got
    assert scrub_paths.leftovers("/Users/x/y.wav")
    assert scrub_paths.leftovers("all clean <refs>/a.wav") == []


def test_scrub_registry_root(monkeypatch, tmp_path):
    reg = {"categories": {"click": {"forge": ["/data/lib/refs/click/forge/a.wav"],
                                    "bench": ["/data/lib/refs/click/bench/b.wav"]}}}
    rs = scrub_paths.roots(reg)
    assert ("/data/lib/refs", "<refs>") in rs
    assert scrub_paths.scrub("/data/lib/refs/click/bench/b.wav", rs=rs) == "<refs>/click/bench/b.wav"


# ---------------------------------------------------------------- build script

def test_B14_build_refsets(tmp_path, monkeypatch, capsys):
    assert build_refsets.family({"id": "fs-1", "author": "u"}) == "fs-author:u"
    assert build_refsets.family({"id": "vsco-x"}) == "vsco"
    assert build_refsets.family({"id": "mk-7"}) == "mk-7"
    plan = {"a": ("q", (0.1, 1.0), [("fs-1", 1, "whole")], []), "b": ("q", (0.1, 1.0), [], [("fs-2", 1, "whole")])}
    monkeypatch.setattr(build_refsets, "CATS", plan)
    src = tmp_path / "sources.json"
    src.write_text(json.dumps([{"id": "fs-1", "author": "u", "path": "x"}, {"id": "fs-2", "author": "u",
                                                                            "path": "y"}]))
    reg = tmp_path / "reg.json"
    code = build_refsets.main(["--plan", "--sources", str(src), "--kit", str(tmp_path / "kit.json"), "--registry",
                               str(reg), "--root", str(tmp_path / "refs")])
    out = capsys.readouterr().out
    assert code == 1 and "family leaks: fs-author:u" in out
    assert not reg.exists() and not (tmp_path / "refs").exists()
    t = np.arange(int(2.0 * SR)) / SR
    x = np.sin(2 * np.pi * 300 * t) * 0.5
    cut = build_refsets.cut_whole(x, (0.1, 1.0))
    assert cut["capped"] is True and cut["b"] - cut["a"] == SR


def test_build_refsets_export_and_pick(tmp_path):
    x = np.column_stack([_decay(dur=0.3)] * 2)
    g = build_refsets.export(x, 0, len(x), tmp_path / "a.wav", 0.03)
    y = sf.read(str(tmp_path / "a.wav"))[0]
    assert 20 * math.log10(np.max(np.abs(y))) == pytest.approx(-1.0, abs=0.01) and isinstance(g, float)
    assert y[0, 0] == 0 and y[-1, 0] == 0
    segs = [{"start": 0.0, "end": 0.2, "z": 3.0, "peak_db": -3}, {"start": 0.1, "end": 0.3, "z": 2.5, "peak_db": -3},
            {"start": 1.0, "end": 1.2, "z": 1.0, "peak_db": -3}, {"start": 2.0, "end": 2.2, "z": 2.8, "peak_db": -30}]
    picks, rej = build_refsets.pick_find(segs, 2, (0.1, 0.5), 2.0, 0.0, 5.0, [])
    assert [p["start"] for p in picks] == [0.0]
    whys = {r["start"]: r["why"] for r in rej}
    assert whys[0.1] == "overlaps a kept trim" and whys[1.0].startswith("z 1.00 <")
    assert whys[2.0] == "27 dB under the loudest"


# ---------------------------------------------------------------- one category with stub models

class _Vocab:
    def embed_files(self, paths):
        return np.array([np.random.default_rng(len(p)).random(8) for p in paths])

    def embed_text(self, texts):
        return np.array([np.random.default_rng(len(t)).random(8) for t in texts])

    def calibrate(self, A, Q):
        return types.SimpleNamespace(z_q=np.asarray(A) @ np.asarray(Q).T)


class _Tagger:
    labels = ["Clicking", "Tick", "Speech"]
    index = {lab: i for i, lab in enumerate(labels)}

    def probs_file(self, path):
        return np.array([0.6, 0.3, 0.1])

    def top(self, p, k=10):
        order = np.argsort(p)[::-1][:k]
        return [(self.labels[i], round(float(p[i]), 3)) for i in order]


def test_bench_category_with_stub_models(tmp_path, monkeypatch):
    monkeypatch.setattr(bench, "_typicality", lambda e, A, k: 0.5)
    files = {}
    for name, f in (("m1", 900), ("m2", 1100), ("b1", 1000), ("b2", 1300), ("b3", 700), ("f1", 1000)):
        files[name] = _wav(tmp_path / f"{name}.wav", _fades(0.5 * _decay(f=f)))
    reg = {"categories": {"click": {"bench": [files["b1"], files["b2"], files["b3"]], "forge": [files["f1"]]}}}
    lines = []
    res = bench.bench_category("click", [files["m1"], files["m2"]], reg, (_Vocab(), _Tagger()), log=lines.append)
    row = res["made"][0]
    for k in ("typ_z", "tone_db", "env_oct", "phrase_z", "margin", "as_p", "floor_db", "artifacts"):
        assert k in row, k
    assert row["as_names"] is True and row["as_p"] == 0.6
    assert len(res["real_cross"]) == 1 and len(res["real_loo"]) == 3 and "inside" in res
    assert res["summary"]["made"]["n"] == 2 and "var_cos" in res["summary"]["made"]
    assert lines[0].startswith("click: 2 made, 3 bench, 1 forge refs; inside")
    empty = bench.bench_category("glitch", [], reg, (_Vocab(), _Tagger()), log=None)
    assert empty == {"n_bench": 0, "n_forge": 0, "note": "", "made": [], "summary": {}}


def test_B15_real_only_run_on_our_registry():
    pytest.skip("needs-data: refs registry and the cut WAVs (licensed third-party recordings)")
