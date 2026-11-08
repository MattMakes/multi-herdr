"""Acceptance tests C-1 .. C-13 of fit-codegen.md and A-15 of fit.md."""

from __future__ import annotations

import argparse
import importlib
import json
import os
import subprocess
import sys
import types

import numpy as np
import pytest
import soundfile

from hark.fit.pipeline import auto, cli, codegen

SR = 48000
KINDS = ("modal", "sines", "spectral", "gesture", "hybrid", "auto")


def _strip(m: dict) -> dict:
    return {k: v for k, v in m.items() if k != "fit_engine"}


def _eq_bitwise(a, b) -> bool:
    if isinstance(a, dict):
        return isinstance(b, dict) and list(a) == list(b) and all(_eq_bitwise(a[k], b[k]) for k in a)
    if isinstance(a, (list, tuple)):
        return isinstance(b, list) and len(a) == len(b) and all(_eq_bitwise(x, y) for x, y in zip(a, b))
    if isinstance(a, float):
        return isinstance(b, float) and np.float64(a).tobytes() == np.float64(b).tobytes()
    return a == b and type(a) is type(b)


@pytest.fixture
def models(sounds) -> dict:
    """One model per kind on the synthetic sounds (S1 for tonal kinds, S3 for noise kinds)."""
    src = {"modal": "S1", "sines": "S2", "spectral": "S3", "gesture": "S3", "hybrid": "S1", "auto": "S1"}
    return {k: auto.fit(sounds[src[k]], kind=k) for k in KINDS}


def _parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="hark")
    cli.add_cli(p.add_subparsers(dest="command"))
    return p


def _run_script(path: str, args: list, cwd: str, env: dict) -> None:
    code = f"import runpy, sys\nsys.argv = [{path!r}] + {args!r}\nrunpy.run_path({path!r}, run_name='__main__')\n"
    subprocess.run([sys.executable, "-c", code], check=True, cwd=cwd, env=env, capture_output=True)


@pytest.fixture
def child_env() -> dict:
    env = dict(os.environ)
    root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    env["PYTHONPATH"] = root + os.pathsep + env.get("PYTHONPATH", "")
    return env


def test_c_1_literal_evaluates_to_the_model(models):
    for kind, m in models.items():
        ns: dict = {}
        exec(codegen.literal(m), ns)  # noqa: S102
        assert _eq_bitwise(json.loads(json.dumps(_strip(m))), ns["MODEL"]), kind


def test_c_1_literal_special_values():
    m = {"kind": "modal", "a": float("nan"), "b": float("inf"), "c": float("-inf"), "d": np.float64(0.1),
         "e": np.int64(3), "f": np.bool_(True), "g": list(range(80)), "h": [], "i": {}, "fit_engine": {"x": 1}}
    text = codegen.literal(m)
    ns: dict = {}
    exec(text, ns)  # noqa: S102
    got = ns["MODEL"]
    assert np.isnan(got["a"]) and got["b"] == float("inf") and got["c"] == float("-inf")
    assert got["d"] == 0.1 and got["e"] == 3 and got["f"] is True and got["g"] == list(range(80))
    assert "fit_engine" not in got
    assert max(len(line) for line in text.splitlines()) <= 116


def test_c_2_recipe_renders_bit_for_bit(models, tmp_path):
    for kind, m in models.items():
        rec = codegen.load_recipe(codegen.write_recipe(str(tmp_path / f"{kind}.py"), m))
        first = next(iter(rec.SPACE))
        moved = {first: auto.knobs_of(m)[first][0][1]}
        for s in (0, 1, 7):
            assert np.array_equal(rec.render_seed(s), auto.render(m, seed=s)), (kind, s)
            assert np.array_equal(rec.render_seed(s, **moved), auto.render(m, seed=s, **moved)), (kind, s)


def test_c_3_render_draws_one_seed(models, tmp_path):
    m = models["modal"]
    rec = codegen.load_recipe(codegen.write_recipe(str(tmp_path / "r.py"), m))
    r, r2 = np.random.default_rng(5), np.random.default_rng(5)
    want = auto.render(rec.MODEL, seed=int(r2.integers(0, 2**31 - 1)), **rec.DEFAULTS)
    assert np.array_equal(rec.render({}, r), want)
    assert np.array_equal(rec.render({"nosuchknob": 3}, np.random.default_rng(9)),
                          rec.render({}, np.random.default_rng(9)))


def test_c_4_names_match_the_knob_table(models, tmp_path):
    for kind, m in models.items():
        rec = codegen.load_recipe(codegen.write_recipe(str(tmp_path / f"{kind}.py"), m))
        table = auto.knobs_of(m)
        assert rec.DEFAULTS == auto.identity(m)
        assert list(rec.SPACE) == list(table)
        assert all(list(rec.SPACE[k]) == list(table[k][0]) for k in table)
        assert rec.KIND == auto.kind_of(m)


def test_c_5_morph_recipe(sounds, tmp_path):
    a = auto.fit(sounds["S2"], kind="sines")
    b = auto.fit(sounds["S2"][::-1].copy(), kind="sines")
    rec = codegen.load_recipe(codegen.write_recipe(str(tmp_path / "mm.py"), auto.morph_model(a, b, 0.5)))
    assert list(rec.SPACE["morph"]) == [0.2, 0.8] and rec.DEFAULTS["morph"] == 0.5
    assert np.array_equal(rec.render_seed(0, morph=0), auto.render(_strip(a), seed=0))
    assert np.array_equal(rec.render_seed(0, morph=1), auto.render(_strip(b), seed=0))


def test_c_6_name_and_main_block(models, tmp_path, child_env):
    path = codegen.write_recipe(str(tmp_path / "out" / "my-fit.v2.py"), models["modal"])
    rec = codegen.load_recipe(path)
    assert rec.__name__ == "my_fit_v2"
    _run_script(os.path.abspath(path), [], str(tmp_path), child_env)
    y, rate = soundfile.read(str(tmp_path / "my_fit_v2.wav"), dtype="float32")
    assert rate == SR and soundfile.info(str(tmp_path / "my_fit_v2.wav")).subtype == "FLOAT"
    assert np.array_equal(y, rec.render_seed(0).astype(np.float32))


def test_load_recipe_gives_independent_modules(models, tmp_path):
    path = codegen.write_recipe(str(tmp_path / "x.py"), models["modal"])
    a, b = codegen.load_recipe(path), codegen.load_recipe(path)
    assert a is not b and a.__name__ == "x"
    assert not any(m is a for m in sys.modules.values())


@pytest.mark.parametrize("character,want", [
    ({"gliding": True}, "a short round pop with a pitch glide"),
    ({"tonal": True, "t40_ms": 500}, "a bright ringing chime"),
    ({"tonal": True, "t40_ms": 100}, "a short pitched ding"),
    ({"impulsive": True, "dur_s": 0.2}, "a short crisp click"),
    ({"impulsive": True}, "a deep hit with a long tail"),
    ({"noisy": True}, "an air whoosh"),
    ({}, "a sound effect"),
    (None, "a sound effect"),
])
def test_c_7_critic_words_from_character(character, want, monkeypatch):
    monkeypatch.setitem(sys.modules, "hark.forge.engine.refsets", None)
    assert codegen.critic_words(character=character) == (want, [], None)


def _fake_registry(monkeypatch, src: str, recipe: types.ModuleType | None) -> None:
    refsets = types.ModuleType("hark.forge.engine.refsets")
    refsets.load = lambda path=None: {"version": 1, "categories": {
        "click": {"forge": [os.path.abspath(src)], "bench": [], "note": ""}}}
    refsets.CATEGORIES = {"click": "a short crisp click (UI)"}
    recipes = types.ModuleType("hark.forge.recipes")
    mods = {"broken": None, "fake": recipe} if recipe is not None else {"broken": None}

    def load(name):
        if mods[name] is None:
            raise ImportError("broken recipe")
        return mods[name]

    recipes.names = lambda: sorted(mods)
    recipes.load = load
    monkeypatch.setitem(sys.modules, "hark.forge.engine.refsets", refsets)
    monkeypatch.setitem(sys.modules, "hark.forge.recipes", recipes)
    # "from package import name" reads the package attribute first: replace it too, in case an earlier test
    # imported the real module.
    monkeypatch.setattr(importlib.import_module("hark.forge.engine"), "refsets", refsets, raising=False)
    monkeypatch.setattr(importlib.import_module("hark.forge"), "recipes", recipes, raising=False)


def test_c_8_critic_words_from_registry(monkeypatch, tmp_path):
    src = str(tmp_path / "rec.wav")
    rec = types.ModuleType("fake")
    rec.REFSET, rec.TARGETS, rec.TARGET, rec.ALTS = "click", {"click": "X"}, "T", ["Y"]
    _fake_registry(monkeypatch, src, rec)
    assert codegen.critic_words(src) == ("X", ["Y"], "click")
    del rec.TARGETS
    assert codegen.critic_words(src) == ("T", ["Y"], "click")
    _fake_registry(monkeypatch, src, None)
    assert codegen.critic_words(src) == ("a short crisp click", [], "click")


def test_c_9_docstring_escaping(models):
    from hark.fit.pipeline import quality as Q

    m = models["modal"]
    qd = Q.quality(np.ones(4800) * 0.1, np.ones(4800) * 0.1)
    text = codegen.to_recipe(m, source='a"""b\\c.wav', quality=qd)
    compile(text, "r.py", "exec")
    doc = text.split('"""', 2)[1]
    assert '"""' not in doc and "\\" not in doc
    assert "a'''b/c.wav" in doc
    assert Q.text(qd) in doc


@pytest.fixture
def stub_clap(monkeypatch) -> None:
    """--report measures CLAP: replace the model check and the ML guard with stubs."""
    from hark.fit.pipeline import quality as Q

    importlib.import_module("hark.models")
    monkeypatch.setattr(sys.modules["hark.models"], "require_ml", lambda feature: None)
    monkeypatch.setattr(Q, "clap_cos", lambda a, b: 0.9)


def _check_fit_files(res: dict, report: bool) -> None:
    for key in ("recipe", "wav", "png", "model"):
        assert os.path.exists(res["files"][key]), key
    assert ("report" in res["files"]) == report


def test_c_10_run_fit_files_and_report(sounds, write_wav, tmp_path, stub_clap):
    src = write_wav("s1.wav", sounds["S1"])
    res = cli.run_fit(src, out_dir=str(tmp_path / "o"), report=True, verbose=False)
    assert res["code_renders_fit"] is True
    _check_fit_files(res, True)
    assert isinstance(res["floor"], dict)
    from hark.fit.pipeline import quality as Q

    assert Q.text(res["quality"], res["floor"]) in open(res["files"]["recipe"]).read()
    res2 = cli.run_fit(src, out_dir=str(tmp_path / "o2"), verbose=False)
    _check_fit_files(res2, False)
    assert res2["floor"] is None and not os.path.exists(str(tmp_path / "o2" / "s1_fit.report.json"))


def test_c_11_morph_paths(sounds, write_wav, tmp_path, monkeypatch):
    write_wav("x/a_fit.wav", sounds["S2"])
    write_wav("x/b.wav", sounds["S2"] * 0.7)
    monkeypatch.chdir(tmp_path)
    res = cli.run_morph("x/a_fit.wav", "x/b.wav", out_dir="o", verbose=False)
    for ext, key in ((".py", "recipe"), (".wav", "wav"), (".png", "png"), (".json", "model")):
        want = (tmp_path / "o" / f"a__b_morph{ext}").as_posix()
        assert res["files"][key] == want and os.path.exists(want)
    assert {"a", "b", "amount", "kind", "kinds_tried", "vs_a", "vs_b", "files"} <= set(res)


def test_c_12_usage_without_file(capsys):
    args = _parser().parse_args(["fit"])
    assert args.func(args) == 2
    assert "--morph" in capsys.readouterr().out


def test_c_12_bad_kind_is_a_usage_error():
    with pytest.raises(SystemExit) as e:
        _parser().parse_args(["fit", "x.wav", "--kind", "nope"])
    assert e.value.code == 2


def test_c_13_forge_mode(models, tmp_path, monkeypatch):
    importlib.import_module("hark.models")
    monkeypatch.setattr(sys.modules["hark.models"], "require_ml", lambda feature: None)
    seen = {}
    loop = types.ModuleType("hark.forge.engine.loop")

    def forge(recipe, **kw):
        seen.update(kw, recipe=recipe)
        return {"summary": "done", "picks": [], "history": [1, 2]}

    loop.forge = forge
    monkeypatch.setitem(sys.modules, "hark.forge.engine.loop", loop)
    monkeypatch.setattr(sys.modules["hark.forge.engine"], "loop", loop, raising=False)
    path = codegen.write_recipe(str(tmp_path / "rec.py"), models["modal"], refset="click")
    args = _parser().parse_args(["fit", "--forge", path, "--morph", "a", "b", "--json"])
    assert args.func(args) == 0
    assert seen["refs"] is None and seen["refset"] == "click"
    assert (seen["robust"], seen["prescreen"], seen["shortlist"], seen["strategy"]) == (3, 48, 2, "es")
    assert seen["out"] == str(tmp_path / "rec") + "_forge"
    assert (seen["budget"], seen["pop"], seen["keep"]) == (96, 16, 4)


def test_a_15_cli_fit_with_report(sounds, write_wav, tmp_path, monkeypatch, capsys, child_env,
                                  stub_clap):
    src = write_wav("S1.wav", sounds["S1"])
    monkeypatch.chdir(tmp_path)
    args = _parser().parse_args(["fit", src, "-o", "out/s1.py", "--report", "--json"])
    assert args.func(args) == 0
    out = capsys.readouterr().out
    res = json.loads(out[out.index("{"):])  # report.emit prints the JSON over several lines
    for ext in (".py", ".wav", ".png", ".json", ".report.json"):
        assert (tmp_path / "out" / f"s1{ext}").exists(), ext
    assert {"kind", "chosen", "quality", "floor", "files", "seconds", "candidates"} <= set(res)
    assert res["code_renders_fit"] is True
    _run_script(str(tmp_path / "out" / "s1.py"), ["0", "x.wav"], str(tmp_path), child_env)
    a, _ = soundfile.read(str(tmp_path / "x.wav"), dtype="float32")
    b, _ = soundfile.read(str(tmp_path / "out" / "s1.wav"), dtype="float32")
    assert np.array_equal(a, b)
