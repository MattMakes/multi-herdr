"""Forge loop tests: L2-L9, S1, S2, plus refs, report text, CLI and the lazy entry module."""

from __future__ import annotations

import argparse
import json
import math
import types

import numpy as np
import pytest
import soundfile as sf

from hark.forge.engine import loop, schema
from hark.forge.engine.cli import USAGE_RENDER, add_cli

SR = 48000
BOX_SPACE = {"f": (100, 10000, "log"), "g": (0, 1), "n": (1, 5, "int"), "k": ["x", "y"]}


def _wav(path):
    return sf.read(str(path), dtype="float64", always_2d=True)[0]


def _quiet(*_a, **_k):
    return None


# ---------------------------------------------------------------- family_box

def test_L2_family_box_widths():
    rec = types.SimpleNamespace(SPACE=BOX_SPACE)
    b = loop.family_box(rec, {"f": 1000, "g": 0.5, "n": 3, "k": "x"}, 0.15)
    lo, hi, kind = b["f"]
    w = 0.15 * math.log(100)
    assert kind == "log" and lo == pytest.approx(1000 * math.exp(-w)) and hi == pytest.approx(1000 * math.exp(w))
    assert 480 < lo < 520 and 1950 < hi < 2050
    assert b["g"] == pytest.approx((0.35, 0.65))
    assert b["n"][:2] == pytest.approx((2.4, 3.6)) and b["n"][2] == "int"
    assert "k" not in b


def test_L3_family_box_edges_and_zero_width():
    rec = types.SimpleNamespace(SPACE={**BOX_SPACE, "z": (2.0, 2.0)})
    b = loop.family_box(rec, {"f": 10000, "g": 0.0, "n": 5, "z": 2.0}, 0.15)
    assert b["f"][1] == pytest.approx(10000) and b["f"][0] < 10000
    assert b["g"][0] == 0.0 and b["n"][1] == 5
    assert "z" not in b
    out = loop.family_box(rec, {"f": 50000, "g": 0.5}, 0.15, keys=["f"])
    assert set(out) == {"f"} and out["f"][1] <= 10000


# ---------------------------------------------------------------- palette

MEMBERS = [{"params": {"tau": 0.02}, "seed": 1, "family": "short"},
           {"params": {"tau": 0.08}, "seed": 2, "family": "mid"},
           {"params": {"tau": 0.2, "noise": 0.0}, "seed": 3, "family": "long"}]


def test_L4_palette_matches_loudness_and_peak(tiny, toy_critic):
    r = loop.palette(tiny, MEMBERS, toy_critic(), log=None)
    assert r["loudness"]["before"]["spread_lu"] > 1.0
    assert r["loudness"]["after"]["spread_lu"] < 0.5
    assert len(r["picks"]) == 3 and [p["family"] for p in r["picks"]] == ["short", "mid", "long"]
    peaks = [p["gain_db"] for p in r["picks"]]
    # every render is at -1 dBFS before gain, so the loudest peak is -1 + max gain_db
    assert -1.0 + max(peaks) == pytest.approx(-1.0, abs=0.01)


def test_S2_palette_with_gains_passes_check_final(tiny, toy_critic, tmp_path):
    r = loop.palette(tiny, MEMBERS, toy_critic(), out=tmp_path, log=None, outputs=False)
    assert any(abs(p["gain_db"]) > 0.1 for p in r["picks"])
    rows = schema.check_final(tmp_path)
    assert len(rows) == 3 and all(row["ok"] for row in rows)
    assert max(row["err"] for row in rows) <= 1.2e-7
    peak = max(float(np.max(np.abs(_wav(tmp_path / f"{p['name']}.wav")))) for p in r["picks"])
    assert 20 * math.log10(peak) == pytest.approx(-1.0, abs=0.01)
    assert (tmp_path / "_audition.wav").is_file() and not (tmp_path / "compare.png").exists()


def test_palette_drops_failed_member(tiny, toy_critic):
    lines = []
    r = loop.palette(tiny, [{"tau": 0.05}, {"tau": 0.05, "freq": float("nan")}], toy_critic(), log=lines.append)
    assert len(r["picks"]) == 1 and r["picks"][0]["seed"] == 0
    assert any("1" in ln and "removed" in ln for ln in lines)


# ---------------------------------------------------------------- forge

@pytest.fixture
def run(tiny, toy_critic, tmp_path):
    out = tmp_path / "run"
    r = loop.forge(tiny, budget=32, pop=8, keep=2, out=out, critic=toy_critic(), log=_quiet)
    return r, out


def test_L5_forge_writes_reproducible_picks(run):
    r, out = run
    name = r["recipe"]
    assert name == "forgetest_tiny"
    assert (out / f"{name}_01.wav").is_file() and (out / f"{name}_02.wav").is_file()
    rep = json.loads((out / "report.json").read_text())
    assert rep["schema"] == 2 and all(isinstance(p["render"], dict) for p in rep["picks"])
    rows = schema.check_final(out)
    assert all(row["ok"] for row in rows)


def test_S1_forge_check_final_error_bound(run):
    _, out = run
    rows = schema.check_final(out)
    assert max(row["err"] for row in rows) <= 1.2e-7


def test_forge_report_keys_and_outputs(run):
    r, out = run
    for k in ("recipe", "target", "alts", "budget", "seed", "pop", "reps", "strategy", "critic", "prescreen",
              "restarts", "n_bank", "generations", "gens", "timing", "pick_rule", "picks", "ref_scores",
              "ref_scores_loo", "summary", "history", "files"):
        assert k in r, k
    assert "forge_s" in r["timing"] and r["prescreen"] == 0 and r["n_bank"] == 0
    assert r["summary"].startswith("forge forgetest_tiny: 'a short bright ping' - ")
    assert r["summary"].endswith("(references scored leave-one-out).")
    assert all("i" not in p["score"] for p in r["picks"])
    assert set(r["history"][0]["score"]) == set(loop.SCORE_KEYS)
    assert (out / "report.md").read_text().startswith("# forge forgetest_tiny: 'a short bright ping'")
    assert (out / "audition.wav").is_file() and (out / "audition.txt").is_file()
    assert (out / "compare.png").is_file() and "compare_error" not in r["files"]


def test_forge_compare_error_is_recorded(tiny, toy_critic, tmp_path, monkeypatch):
    from hark.forge.engine import viz

    def boom(*a, **k):
        raise RuntimeError("no plot")

    monkeypatch.setattr(viz, "compare_png", boom)
    r = loop.forge(tiny, budget=16, pop=8, keep=1, out=tmp_path, critic=toy_critic(), log=_quiet)
    assert r["files"]["compare_error"] == "RuntimeError: no plot"
    assert (tmp_path / "report.json").is_file()


def test_forge_lambda_render_drops_schema(toy_critic):
    rec = types.SimpleNamespace(__name__="lam", SPACE={"a": (0.1, 1.0)}, DEFAULTS={"a": 0.5}, TARGET="t", ALTS=[],
                                render=lambda p, r: np.sin(np.arange(4800) * p["a"]) * np.hanning(4800))
    r = loop.forge(rec, budget=16, pop=8, keep=1, critic=toy_critic(), log=_quiet, holdout=False)
    assert "schema" not in r and r["picks"][0]["render"] is None


def test_forge_robust_shortlist(tiny, toy_critic):
    r = loop.forge(tiny, budget=16, pop=8, keep=1, robust=2, shortlist=2, critic=toy_critic(), log=_quiet)
    assert len(r["picks"]) == 1 and r["picks"][0]["robust"]["n"] == 2
    assert "re-rendered on 2 new seeds" in r["pick_rule"]


def test_forge_bank_round_trip(tiny, toy_critic, tmp_path):
    lines = []
    loop.forge(tiny, budget=16, pop=8, keep=2, bank=True, bank_dir=tmp_path, critic=toy_critic(), log=lines.append)
    bank = json.loads((tmp_path / "forgetest_tiny.json").read_text())
    assert bank["entries"] and bank["entries"][0]["source"] == "forge es budget 16 seed 0"
    r = loop.forge(tiny, budget=16, pop=8, keep=1, bank=True, bank_dir=tmp_path, critic=toy_critic(), log=_quiet)
    assert r["n_bank"] >= 1


# ---------------------------------------------------------------- families

def test_L6_families_give_a_palette(tiny, toy_critic, tmp_path):
    fam = {"a": {"freq": 400.0, "tau": 0.03}, "b": {"init": {"freq": 1500.0}, "fixed": {"shape": "tri"}}}
    r = loop.forge(tiny, budget=16, pop=8, families=fam, critic=toy_critic(), out=tmp_path, log=_quiet,
                   outputs=False)
    assert len(r["picks"]) == 2 and len(r["runs"]) == 2
    assert [p["family"] for p in r["picks"]] == ["a", "b"]
    assert r["picks"][1]["params"]["shape"] == "tri"
    assert "forge_s" in r["timing"] and (tmp_path / "runs" / "a" / "report.json").is_file()
    assert json.loads((tmp_path / "report.json").read_text())["runs"][0]["family"] == "a"


def test_family_member_keeps_its_value_outside_the_box(tiny, toy_critic):
    fam = [("only", {"init": {"noise": 0.9}, "space": {"freq": (500.0, 700.0, "log")}})]
    r = loop.forge(tiny, budget=16, pop=8, families=fam, critic=toy_critic(), log=_quiet, outputs=False)
    assert r["picks"][0]["params"]["noise"] == 0.9


# ---------------------------------------------------------------- audition

def test_L7_audition_length_and_cues(tmp_path):
    x = np.sin(np.arange(4800) * 0.05)
    y = np.column_stack([np.sin(np.arange(9600) * 0.02)] * 2) * 0.1
    res = loop.audition(tmp_path / "a.wav", [("m", x)], [("r", y)], [0])
    reel = _wav(tmp_path / "a.wav")
    assert len(reel) == len(x) + int(0.6 * SR) + len(y) + int(1.2 * SR)
    assert res["cues"][0] == {"t": 0.0, "kind": "made", "name": "m"}
    assert res["cues"][1]["kind"] == "ref" and res["cues"][1]["t"] == pytest.approx(len(x) / SR + 0.6, abs=0.005)
    assert 20 * math.log10(np.max(np.abs(reel))) == pytest.approx(-3.0, abs=0.01)
    assert (tmp_path / "a.txt").read_text().splitlines()[1].endswith("made  m")


def test_audition_empty_and_keep_levels(tmp_path):
    loop.audition(tmp_path / "e.wav", [], [], [])
    assert _wav(tmp_path / "e.wav").shape == (4800, 2)
    loop.audition(tmp_path / "k.wav", [("m", 0.1 * np.ones(480))], [], [None], keep_made_levels=True)
    assert np.max(np.abs(_wav(tmp_path / "k.wav"))) == pytest.approx(0.1, abs=1e-4)


# ---------------------------------------------------------------- render_params

def test_L9_render_params_matches_forge_picks(run, tiny, tmp_path):
    r, out = run
    res = loop.render_params(tiny, r, out=tmp_path / "re")
    assert len(res["files"]) == len(r["picks"])
    for f, p in zip(res["files"], r["picks"]):
        assert f["seed"] == p["seed"]
        assert np.max(np.abs(_wav(f["file"]) - _wav(out / f"{p['name']}.wav"))) <= 1e-6


def test_L9_render_params_after_palette_gain(tiny, toy_critic, tmp_path):
    r = loop.palette(tiny, MEMBERS, toy_critic(), out=tmp_path / "pal", log=None, outputs=False)
    res = loop.render_params(tiny, r, out=tmp_path / "re")
    for f, p in zip(res["files"], r["picks"]):
        made = _wav(tmp_path / "pal" / f"{p['name']}.wav")
        assert np.max(np.abs(_wav(f["file"]) * p["render"]["gain"] - made)) <= 1e-6


def test_render_params_plain_list_and_seeds(tiny, tmp_path):
    res = loop.render_params(tiny, [{"freq": 300.0}, {"tau": 0.1}], seeds=[1, 2], out=tmp_path)
    names = sorted(f["file"].rsplit("/", 1)[-1] for f in res["files"])
    assert names == ["forgetest_tiny_r01_s1.wav", "forgetest_tiny_r01_s2.wav", "forgetest_tiny_r02_s1.wav",
                     "forgetest_tiny_r02_s2.wav"]
    bad = loop.render_params(tiny, {"freq": float("nan")}, out=tmp_path)
    assert bad["files"][0]["file"] is None


# ---------------------------------------------------------------- variant recipe

def test_variant_recipe_and_block(tiny):
    v = loop.variant_recipe(tiny.render, [{**tiny.DEFAULTS, "freq": 300.0}, {**tiny.DEFAULTS, "freq": 900.0}],
                            names=["low", "high"], TARGET="t")
    assert v.__name__ == tiny.__name__ and v.SPACE == {"variant": [0, 1]} and v.TARGET == "t"
    b = loop.render_block(v, {"variant": 1}, 7)
    assert b["fn"] == "render" and b["params"]["freq"] == 900.0 and b["member"] == "high" and b["seed"] == 7
    assert loop.variant_recipe(lambda p, r: 0, [{}]).__name__ == __name__


# ---------------------------------------------------------------- refs

def _write_ref(path):
    sf.write(str(path), 0.1 * np.sin(np.arange(4800) * 0.1), SR)
    return str(path)


def test_pick_refs_drops_held_out(tmp_path, monkeypatch):
    a, b = _write_ref(tmp_path / "a.wav"), _write_ref(tmp_path / "b.wav")
    reg = {"categories": {"pings": {"forge": [a], "bench": [b]}}}
    (tmp_path / "refsets.json").write_text(json.dumps(reg))
    monkeypatch.setenv("HARK_REFSETS", str(tmp_path / "refsets.json"))
    lines = []
    got, cat = loop.pick_refs([str(tmp_path)], recipe=None, log=lines.append)
    assert cat is None and [p.rsplit("/", 1)[-1] for p in got] == ["a.wav"] and "b.wav" in lines[0]
    assert loop.pick_refs(refset="pings") == ([a], "pings")
    rec = types.SimpleNamespace(REFSET="pings")
    assert loop.pick_refs(recipe=rec) == ([a], "pings")
    rec = types.SimpleNamespace(REFSET="nope", REFS=[b, str(tmp_path / "gone.wav")])
    assert loop.resolve_refs(recipe=rec) is None


# ---------------------------------------------------------------- report text

def test_report_md_sections(run):
    r, _ = run
    md = loop.report_md(r)
    for h in ("**Asked:**", "**Budget:**", "## Search", "## Picks", "## Files"):
        assert h in md
    assert "## Gap to the real references" not in md
    assert md.index("## Search") < md.index("## Picks") < md.index("## Files")


def test_palette_md(tiny, toy_critic):
    r = loop.palette(tiny, MEMBERS, toy_critic(), name="pings", log=None)
    md = loop.palette_md(r)
    assert md.startswith("# palette pings (forgetest_tiny)") and "## Critic words" in md
    assert r["summary"].startswith("palette pings: 3 members")


# ---------------------------------------------------------------- CLI

def _cli(argv):
    ap = argparse.ArgumentParser(prog="hark")
    add_cli(ap.add_subparsers(dest="cmd"))
    args = ap.parse_args(["forge", *argv])
    return args.func(args)


def test_L8_render_without_params_prints_usage(capsys):
    assert _cli(["--render", "click"]) == 2
    assert capsys.readouterr().out.strip() == USAGE_RENDER
    assert _cli(["--render", "--params", "x.json"]) == 2


def test_cli_render_and_list(tiny, tmp_path, capsys):
    (tmp_path / "p.json").write_text(json.dumps({"freq": 500.0}))
    assert _cli(["--render", "forgetest_tiny", "--params", str(tmp_path / "p.json"), "--dir", str(tmp_path)]) == 0
    assert (tmp_path / "forgetest_tiny_r01_s0.wav").is_file()
    capsys.readouterr()
    assert _cli(["--list"]) == 0
    out = capsys.readouterr().out
    assert out.startswith("recipes (name: TARGET):") and "  forgetest_tiny: a short bright ping" in out
    assert "refsets:" in out


def test_cli_check_runs_schema(run):
    _, out = run
    assert _cli(["--check", str(out)]) == 0


# ---------------------------------------------------------------- lazy entry module

def test_hark_forge_lazy_names(tiny):
    import hark.forge as hf

    assert hf.forge is loop.forge and hf.SCORE_KEYS[0] == "total"
    assert "forgetest_tiny" in hf.recipes() and hf.load_recipe("forgetest_tiny") is tiny
    with pytest.raises(AttributeError):
        hf.no_such_name  # noqa: B018
