"""Acceptance tests M-T1 .. M-T23 of the MCP server and match (surfaces-mcp.md M7).

In-process tests call ``mcp_server.call_tool`` with fake backing modules. The stdio tests start the server
in a child process and need the ``mcp`` extra; they skip when it is absent. Match tests use the real
``hark.listen.match`` (unit listen, T-14) and the real CLAP model, so they skip until both are present.
"""

from __future__ import annotations

import argparse
import asyncio
import io
import json
import os
import sys
from pathlib import Path

import numpy as np
import pytest

from hark.surfaces import mcp_server as ms

ENGINE = Path(__file__).resolve().parents[2]
CONFTEST = Path(__file__).with_name("conftest.py")
TOOL_ORDER = ["measure", "describe", "match", "rank", "find", "compare", "ab", "realness", "timeline", "sheet",
              "critique", "doctor", "llm_critique", "sfx_list", "sfx_render", "kit_render"]


def compact_of(text: str) -> dict:
    return json.loads(text.split("JSON (compact): ", 1)[1])


def full_of(text: str) -> dict:
    return json.loads(text.split("JSON: ", 1)[1])


@pytest.fixture
def sine1(wav, tone) -> str:
    return wav(tone(freq=440.0, dur=1.0, amp=0.5), name="sine.wav")


# ---------------------------------------------------------------- tool list


def test_m_t1_tool_list(capsys):
    rows = ms.tool_list()["tools"]
    assert [r["name"] for r in rows] == TOOL_ORDER
    assert all(set(r) == {"name", "read_only", "network", "description"} for r in rows)
    assert [r["name"] for r in rows if r["network"]] == ["llm_critique"]
    assert sorted(r["name"] for r in rows if not r["read_only"]) == ["kit_render", "llm_critique", "sfx_render",
                                                                      "sheet"]
    parser = argparse.ArgumentParser(prog="hark")
    ms.add_cli(parser.add_subparsers(dest="command"))
    args = parser.parse_args(["mcp", "--tools", "--json"])
    assert args.func(args) == 0
    out = json.loads(capsys.readouterr().out)
    assert [r["name"] for r in out["tools"]] == TOOL_ORDER and "summary" in out


def test_m_t1_tools_text():
    lines = ms.tool_list()["text"].splitlines()
    assert "16 tools" in lines[0] and "claude mcp add hark" in lines[0]
    assert len(lines) == 17
    assert lines[1].startswith("measure      ") and lines[1].endswith(".")
    assert all("  " not in ms.description(t) for t in TOOL_ORDER)


# ---------------------------------------------------------------- stdio

BOOT = """
import os, runpy, sys, types
runpy.run_path(os.environ["SURFACES_CONFTEST"])["install_standins"]()
if os.environ.get("FAKE_DOCTOR"):
    def check(load=False):
        print("noise on stdout from the backing function")
        return {"summary": "doctor ok", "load": load}
    mod = types.ModuleType("hark.doctor")
    mod.check = check
    sys.modules["hark.doctor"] = mod
from hark.surfaces.mcp_server import main
sys.exit(main())
"""


def field(obj, snake: str, camel: str):
    """A result field under its mcp 2.x (snake) or 1.x (camel) name."""
    return getattr(obj, snake) if hasattr(obj, snake) else getattr(obj, camel)


def _stdio(calls, fake_doctor: bool = False):
    pytest.importorskip("mcp")
    from mcp import ClientSession, StdioServerParameters
    from mcp.client.stdio import stdio_client

    path = os.pathsep.join(p for p in (str(ENGINE), os.environ.get("PYTHONPATH", "")) if p)
    env = dict(os.environ, PYTHONPATH=path, SURFACES_CONFTEST=str(CONFTEST))
    if fake_doctor:
        env["FAKE_DOCTOR"] = "1"
    params = StdioServerParameters(command=sys.executable, args=["-c", BOOT], env=env)

    async def go():
        async with stdio_client(params) as (read, write):
            async with ClientSession(read, write) as session:
                init = await session.initialize()
                return init, await calls(session)

    return asyncio.run(asyncio.wait_for(go(), 300))


@pytest.mark.slow
def test_m_t2_stdio_tools():
    async def calls(session):
        return await session.list_tools()

    init, listed = _stdio(calls)
    assert field(init, "server_info", "serverInfo").name == "hark"
    assert init.instructions and "absolute" in init.instructions
    tools = listed.tools
    assert [t.name for t in tools] == TOOL_ORDER
    for t in tools:
        assert t.annotations.title == f"hark {t.name}"
        assert field(t.annotations, "destructive_hint", "destructiveHint") is False


@pytest.mark.slow
def test_m_t3_stdio_measure(sine1):
    async def calls(session):
        return await session.call_tool("measure", {"path": sine1})

    _, res = _stdio(calls)
    assert not field(res, "is_error", "isError") and len(res.content) == 1
    text = res.content[0].text
    assert text.splitlines()[0].startswith("1.000 s")
    js = compact_of(text)
    assert "summary" not in js and "text" not in js


@pytest.mark.slow
def test_m_t16_stdout_during_call():
    async def calls(session):
        return await session.call_tool("doctor", {})

    _, res = _stdio(calls, fake_doctor=True)
    assert not field(res, "is_error", "isError") and "doctor ok" in res.content[0].text


def test_m_t16_stdout_goes_to_stderr(fake_module, capsys):
    def check(load=False):
        print("chatter")
        return {"summary": "doctor ok"}

    fake_module("hark.doctor", check=check)
    res = ms.call_tool("doctor")
    captured = capsys.readouterr()
    assert "chatter" in captured.err and "chatter" not in captured.out
    assert res.text.startswith("doctor ok")


# ---------------------------------------------------------------- measure


def test_m_t3_measure_in_process(sine1):
    text = ms.call_tool("measure", path=sine1).text
    assert text.splitlines()[0].startswith("1.000 s")
    js = compact_of(text)
    assert "summary" not in js and "text" not in js and js["duration"] == 1.0


def test_m_t4_measure_zeros(wav, silence):
    text = ms.call_tool("measure", path=wav(silence(dur=1.0), name="zeros.wav")).text
    first = text.splitlines()[0]
    assert "digitally silent" in first and "mean nothing" in first


def test_measure_summary_sentence():
    m = {"duration": 2.0, "peak_dbfs": -3.0, "lufs": -16.2, "short_term_max_lufs": -14.0, "true_peak_dbtp": -2.9,
         "clipped_samples": 3, "lra_lu": 4.1, "noise_floor_dbfs": -60.0, "brightness_hz": 2500, "bandwidth_hz": 0,
         "attack_ms": 5, "decay_ms": 300, "onset_count": 4, "tempo": 120.0, "key": [{"key": "C major"}],
         "digital_silence_frac": 0.2}
    text = ms.measure_summary(m)
    assert text.startswith("2.000 s, -16.2 LUFS integrated (short-term max -14.0), true peak -2.9 dBTP, 3 clipped")
    assert "brightness 2.5 kHz" in text and "bandwidth" not in text
    assert "attack 5 ms, decay 300 ms, 4 onsets, tempo ~120.0 BPM, key C major." in text
    assert "20.0% exact digital zeros" in text
    assert ms.measure_summary({"duration": 0.01, "error": "too short"}) == "0.010 s: too short."
    assert "too short for LUFS" in ms.measure_summary({"duration": 0.1, "peak_dbfs": -6, "rms_dbfs": -12})


# ---------------------------------------------------------------- arguments


def test_m_t5_relative_missing_path(sine1, monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    with pytest.raises(ms.ToolError) as err:
        ms.call_tool("measure", path="nope.wav")
    msg = str(err.value)
    assert str(tmp_path / "nope.wav") in msg and f"working folder is {os.getcwd()}" in msg
    assert ms.call_tool("measure", path=sine1).text.startswith("1.000 s")


def test_file_arg_rules(tmp_path, sine1):
    with pytest.raises(ms.ToolError, match="a file path is required"):
        ms.file_arg("  ")
    with pytest.raises(ms.ToolError, match="folder"):
        ms.file_arg(str(tmp_path))
    assert ms.file_arg(f' "{sine1}" ') == sine1


def test_m_t6_rank_empty_folder(wav, tone, tmp_path):
    wav(tone(), name="full/a.wav")
    (tmp_path / "empty").mkdir()
    with pytest.raises(ms.ToolError) as err:
        ms.call_tool("rank", paths=[str(tmp_path / "full"), str(tmp_path / "empty")], query="a click")
    assert str(tmp_path / "empty") in str(err.value) and str(tmp_path / "full") + "," not in str(err.value)
    with pytest.raises(ms.ToolError, match="at least 1"):
        ms.paths_arg([], "paths")


def test_m_t7_find_bad_kind(sine1):
    with pytest.raises(ms.ToolError) as err:
        ms.call_tool("find", path=sine1, query="a click", kind="blob")
    assert all(k in str(err.value) for k in ("auto", "event", "texture"))
    with pytest.raises(ms.ToolError, match="low, normal, high"):
        ms.call_tool("timeline", path=sine1, detail="max")


# ---------------------------------------------------------------- result shaping


def test_m_t8_compact_and_cut(sine1, fake_module):
    big = {"summary": "big result", "text": "x" * 30000,
           "items": [f"item {i} " + "y" * 300 for i in range(50)], "value": 1.23456789}
    fake_module("hark.listen.describe", describe=lambda path, start=None, end=None, top=8: big)
    text = ms.call_tool("describe", path=sine1).text
    part = text.split("JSON (compact): ", 1)[1]
    assert len(part) <= 7000
    js = json.loads(part)
    assert js["items"][-1].startswith("... +") and js["items"][-1].endswith(" more")
    assert js["value"] == 1.235
    assert "text cut at 24000 characters" in text and "full=true" in text
    full = ms.call_tool("describe", path=sine1, full=True).text
    assert "x" * 30000 in full
    assert full_of(full)["items"] == big["items"]


def test_m_t9_huge_field(sine1, fake_module):
    huge = {"summary": "s", "big": {f"key{i}": i for i in range(3000)}, "small": 1}
    fake_module("hark.listen.describe", describe=lambda path, **kw: huge)
    js = compact_of(ms.call_tool("describe", path=sine1).text)
    assert js["cut_fields"] == ["big"] and "full=true" in js["note"] and js["small"] == 1


def test_compact_not_a_dict():
    out = json.loads(ms.compact_json([{f"k{i}": i for i in range(3000)}]))
    assert "cut" in out


def test_m_t10_ignored_argument(sine1, fake_module):
    fake_module("hark.listen.describe", describe=lambda path, top=8: {"summary": "ok"})
    text = ms.call_tool("describe", path=sine1, start=1.0).text
    assert "(this hark version ignored: start)" in text


def test_nan_becomes_null(sine1, fake_module):
    fake_module("hark.listen.describe", describe=lambda path, **kw: {"summary": "ok", "v": float("nan"),
                                                                       "a": np.array([1.0, np.inf])})
    js = compact_of(ms.call_tool("describe", path=sine1).text)
    assert js["v"] is None and js["a"] == [1.0, None]


# ---------------------------------------------------------------- sheet


def _fake_sheet(fake_module, width=3000, height=1000, write=True):
    from PIL import Image
    seen = {}

    def sheet(path, out_png=None, video=None, cues=None, start=None, end=None, detail="normal"):
        seen.update(out_png=out_png, start=start, end=end)
        if write:
            Image.new("RGB", (width, height), (40, 80, 120)).save(out_png)
        return {"summary": "sheet drawn", "png": out_png}

    fake_module("hark.timeline.sheet", sheet=sheet)
    return seen


def test_m_t11_sheet_image(sine1, fake_module, hark_home):
    from PIL import Image
    seen = _fake_sheet(fake_module)
    res = ms.call_tool("sheet", path=sine1)
    png = seen["out_png"]
    assert png.startswith(os.path.join(hark_home, "sheets") + os.sep)
    stem = os.path.basename(png)
    assert stem.startswith("sine_") and len(stem) == len("sine_") + 8 + len(".png")
    assert res.text.startswith(f"PNG: {png}")
    with Image.open(io.BytesIO(res.image)) as im:
        assert im.format == "JPEG" and max(im.size) == 1600 and im.size == (1600, 533)


def test_m_t12_sheet_span_name(sine1, fake_module, hark_home):
    seen = _fake_sheet(fake_module)
    ms.call_tool("sheet", path=sine1, start=2)
    assert seen["out_png"].endswith("_2-end.png")
    assert ms.sheet_path(sine1, 0.5, 3.0).endswith("_0.5-3.png")


def test_sheet_without_png(sine1, fake_module, hark_home):
    _fake_sheet(fake_module, write=False)
    res = ms.call_tool("sheet", path=sine1)
    assert "(no PNG found at " in res.text and res.image is None


# ---------------------------------------------------------------- realness


def test_m_t13_realness_folder(wav, tone, fake_module, tmp_path):
    a = wav(tone(), name="set/a.wav")
    b = wav(tone(freq=880), name="set/b.wav")
    model = tmp_path / "model.json"
    model.write_text("{}")
    got = {}

    def many(paths, ast=False, start=None, end=None):
        got["paths"] = paths
        return {"summary": "2 files judged", "files": [
            {"path": a, "p_real": 0.91, "verdict": "real or pro-produced", "why": "rich noise"},
            {"path": b, "p_real": None, "verdict": "error", "summary": "could not decode"}]}

    fake_module("hark.listen.realness", realness=lambda path, **kw: {}, realness_many=many, MODEL_PATH=str(model))
    text = ms.call_tool("realness", path=str(tmp_path / "set")).text
    lines = text.splitlines()
    head = lines.index("p_real  verdict                  file: main reason")
    assert lines[head + 1] == "  0.91  real or pro-produced     a.wav: rich noise"
    assert lines[head + 2].startswith("     -  error") and lines[head + 2].endswith("b.wav: could not decode")
    assert sorted(got["paths"]) == sorted([a, b])


def test_realness_model_missing(sine1, fake_module, tmp_path):
    fake_module("hark.listen.realness", realness=lambda path, **kw: {}, MODEL_PATH=str(tmp_path / "none.json"))
    with pytest.raises(ms.ToolError, match="NOT checked"):
        ms.call_tool("realness", path=sine1)


# ---------------------------------------------------------------- timeline arguments


def test_m_t14_video_default(fake_module, tmp_path):
    film = tmp_path / "film.mp4"
    film.write_bytes(b"film")
    got = {}
    fake_module("hark.timeline.timeline", timeline=lambda path, **kw: got.update(kw, path=path) or {"summary": "t"})
    ms.call_tool("timeline", path=str(film))
    assert got["video"] == str(film) and got["path"] == str(film)


def test_m_t15_cues(sine1, fake_module):
    got = {}
    fake_module("hark.timeline.timeline", timeline=lambda path, **kw: got.update(kw) or {"summary": "t"})
    ms.call_tool("timeline", path=sine1, cues="1.2,3.4")
    assert got["cues"] == "1.2,3.4"
    with pytest.raises(ms.ToolError, match="no such file /abs/cues.json"):
        ms.call_tool("timeline", path=sine1, cues="/abs/cues.json")


# ---------------------------------------------------------------- missing and failing backing


def test_backing_absent_lists_present(sine1, monkeypatch):
    monkeypatch.setitem(ms.TOOL_BY_NAME, "describe", ms.Tool("describe", "hark.no_such_unit.describe",
                                                             "describe", True, False))
    with pytest.raises(ms.ToolError) as err:
        ms.call_tool("describe", path=sine1)
    assert "not part of this install" in str(err.value) and "present tools:" in str(err.value)
    assert "llm_critique" in str(err.value)


def test_backing_raises(sine1, fake_module):
    def boom(path, **kw):
        raise ValueError("bad thing")

    fake_module("hark.listen.describe", describe=boom)
    with pytest.raises(ms.ToolError, match="describe failed: ValueError: bad thing"):
        ms.call_tool("describe", path=sine1)


def test_backing_import_broken(sine1, monkeypatch, tmp_path):
    pkg = tmp_path / "brokenpkg_surfaces"
    pkg.mkdir()
    (pkg / "__init__.py").write_text("raise RuntimeError('half updated')\n")
    monkeypatch.syspath_prepend(str(tmp_path))
    monkeypatch.setitem(ms.TOOL_BY_NAME, "describe", ms.Tool("describe", "brokenpkg_surfaces", "describe",
                                                             True, False))
    with pytest.raises(ms.ToolError) as err:
        ms.call_tool("describe", path=sine1)
    assert "RuntimeError: half updated" in str(err.value) and "hark doctor" in str(err.value)


def test_sfx_list_text(fake_module):
    sounds = [{"id": "whoosh_fast/crisp", "dur_s": 0.6, "peak_s": 0.31, "kinds": ["whoosh"],
               "knobs": {"dur": "0.3-1.2", "apex": True, "pitch": {"note": "E5"}}},
              {"id": "click/soft", "dur_s": 0.05, "peak_s": 0.002, "kinds": [], "knobs": {"dur": "fixed"}}]
    fake_module("hark.forge.bank.sfx", sfx_list=lambda category=None, kind=None: {"summary": "2 sounds",
                                                                                   "sounds": sounds})
    text = ms.call_tool("sfx_list").text
    assert "whoosh_fast/crisp: 0.600 s, peak 0.310 s; whoosh; dur 0.3-1.2, apex, pitch E5" in text
    assert "click/soft: 0.050 s, peak 0.002 s; -; dur fixed" in text


def test_preference_free_texts():
    banned = ("the user rejects", "the user prefers", "the user hates", "the user wants", "never use synthetic")
    texts = [ms.INSTRUCTIONS] + [ms.description(t) for t in TOOL_ORDER]
    assert not [b for t in texts for b in banned if b in t.lower()]


def test_mcp_extra_missing_exits(monkeypatch, capsys):
    import importlib.util
    real = importlib.util.find_spec
    monkeypatch.setattr(importlib.util, "find_spec", lambda name, *a: None if name == "mcp" else real(name, *a))
    assert ms.serve() == 3
    assert "mcp" in capsys.readouterr().err


# ---------------------------------------------------------------- match (unit listen)


def _vocabulary():
    return pytest.importorskip("hark.listen.vocabulary", reason="needs unit listen (T-14)")


def test_m_t19_zf():
    vocab = _vocabulary()
    assert [vocab.zf(2.96), vocab.zf(-0.04), vocab.zf(3.0)] == ["+2.9", "-0.1", "+3.0"]


def test_m_t20_verdict():
    vocab = _vocabulary()
    assert [vocab.verdict(2.99), vocab.verdict(2.5), vocab.verdict(1.99)] == ["plausible", "plausible", "no match"]


def _match_ready():
    pytest.importorskip("hark.listen.match", reason="needs unit listen (T-14)")


@pytest.mark.ml
def test_m_t17_two_queries(wav, clicks):
    _match_ready()
    path = wav(clicks(), name="clicks.wav")
    js = full_of(ms.call_tool("match", path=path, query=["a click", "a whoosh"], full=True).text)
    zs = [r["z"] for r in js["results"]]
    assert zs == sorted(zs, reverse=True)
    assert abs(sum(r["p_vs_alts"] for r in js["results"]) - 1) <= 0.001


@pytest.mark.ml
def test_m_t18_alts(wav, clicks):
    _match_ready()
    path = wav(clicks(), name="clicks.wav")
    text = ms.call_tool("match", path=path, query="a click", alts=["a whoosh", "a dog bark"], full=True).text
    js = full_of(text)
    assert abs(js["p_vs_alts"] + sum(a["p"] for a in js["alts"]) - 1) <= 0.001
    assert "%" in js["summary"]


@pytest.mark.ml
def test_m_t21_near_silent(wav, tone):
    _match_ready()
    path = wav(tone(amp=1e-5), name="quiet.wav")
    js = full_of(ms.call_tool("match", path=path, query="a click", full=True).text)
    assert "near-silent" in js["summary"].split(":", 1)[0].lower() or js["summary"].lower().startswith("warning")


@pytest.mark.ml
def test_m_t23_unvalidated(wav, tone):
    _match_ready()
    vocab = _vocabulary()
    path = wav(tone(dur=2.0), name="tone.wav")
    query = "sad melancholic music"
    if vocab.get().nearest_category(query) not in vocab.UNVALIDATED:
        pytest.skip("the shipped vocabulary maps this query outside the unvalidated categories")
    js = full_of(ms.call_tool("match", path=path, query=query, full=True).text)
    assert js["validated"] is False and "unvalidated" in js["summary"]
    assert "held-out library search" not in js["summary"]


def test_m_t22_match_cli(wav, tone, capsys):
    match = pytest.importorskip("hark.listen.match", reason="needs unit listen (T-14)")
    path = wav(tone(dur=2.0), name="two.wav")
    parser = argparse.ArgumentParser(prog="hark")
    match.add_cli(parser.add_subparsers(dest="command"))
    args = parser.parse_args(["match", path, "a click", "--start", "5"])
    assert args.func(args) == 1
    err = capsys.readouterr().err.strip().splitlines()
    assert len(err) == 1 and err[0].startswith("hark match: error:")
    args = parser.parse_args(["match", path, "a click", "--start", "5", "--json"])
    args.func(args)
    out = json.loads(capsys.readouterr().out)
    assert set(out) >= {"summary", "error"}
