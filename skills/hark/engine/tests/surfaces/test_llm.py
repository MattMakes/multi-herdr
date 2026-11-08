"""Acceptance tests C-T1 .. C-T20 of the LLM critique (surfaces-critique.md C9, ARCHITECTURE.md 10.5).

agy is a fake executable on PATH in every test except the 1 test marked live.
The clip is WAV (ARCHITECTURE.md 10.5 overrides the MP3 of C2), so C-T4 checks WAV bytes.
"""

from __future__ import annotations

import argparse
import io
import json
import os
import re

import numpy as np
import pytest
import soundfile as sf

from hark.surfaces import llm, mcp_server

FULL_ANSWER = {
    "overall": {"score": 6, "verdict": "Capable mix with a late hit"},
    "scores": {"sync": 5, "realism": 7, "mix_balance": 6, "loudness": 8, "music": None},
    "notes": {"sync": "the hit at 4 s drags", "realism": ""},
    "synthetic_or_cheap": [{"start": 1.0, "end": 1.5, "what": "beep", "why": "bare sine"}],
    "issues": [
        {"start": 2.0, "end": 2.5, "severity": "low", "category": "mix", "problem": "pad masks voice", "fix": "cut 3 dB"},
        {"start": 4.0, "end": 4.2, "severity": "high", "category": "sync", "problem": "hit late", "fix": "move 80 ms"},
        {"start": 0.5, "severity": "medium", "category": "noise", "problem": "hiss", "fix": "denoise"},
    ],
    "strengths": ["clean low end"],
    "top_fixes": ["Move the hit at 4.0 s 80 ms earlier", "Cut the pad 3 dB"],
}


@pytest.fixture
def clip10(wav, tone) -> str:
    """A 10 s sine WAV."""
    return wav(tone(freq=440.0, dur=10.0, amp=0.3), name="clip10.wav")


def _parser():
    parser = argparse.ArgumentParser(prog="hark")
    sub = parser.add_subparsers(dest="command")
    llm.add_cli(sub)
    return parser


def run_cli(argv: list[str]) -> int:
    args = _parser().parse_args(argv)
    return args.func(args)


def wav_info(data: bytes) -> sf._SoundFileInfo:
    return sf.info(io.BytesIO(data))


# ---------------------------------------------------------------- gating


def test_c_t1_no_consent(clip10, fake_agy, capsys):
    res = llm.critique(clip10, consent=False)
    assert res["enabled"] is False and res["ok"] is False
    assert len(res["how_to_enable"]) == 2
    assert os.path.basename(clip10) in res["summary"] and res["model"] in res["summary"]
    assert "  1. " in res["text"] and "  2. " in res["text"]
    assert fake_agy.calls() == []
    assert run_cli(["llm", clip10]) == 1
    assert fake_agy.calls() == []


def test_c_t2_agy_not_on_path(clip10, no_agy):
    res = llm.critique(clip10, consent=True)
    assert res["enabled"] is False and res["ok"] is False
    steps = " ".join(res["how_to_enable"]).lower()
    assert "install" in steps and "sign in" in steps
    assert "rest of hark works" in res["summary"]


def test_c_t3_does_not_hear_exits_2(clip10, fake_agy, monkeypatch, capsys):
    monkeypatch.setattr(llm, "AGY_VERDICT", "DOES-NOT-HEAR")
    assert run_cli(["llm", clip10, "--yes-upload"]) == 2
    err = capsys.readouterr().err
    assert "hark llm: error:" in err and "cannot hear audio" in err
    assert fake_agy.calls() == []
    with pytest.raises(llm.AgyCannotHear):
        llm.critique(clip10, consent=True)
    with pytest.raises(mcp_server.ToolError, match="cannot hear audio"):
        mcp_server.call_tool("llm_critique", path=clip10, consent=True)
    assert fake_agy.calls() == []


# ---------------------------------------------------------------- clip


def test_c_t4_encode_range(clip10):
    data, mime, info = llm.encode(clip10, start=2, end=5)
    assert data[:4] == b"RIFF" and mime == "audio/wav"
    meta = wav_info(data)
    assert meta.samplerate == 16000 and meta.channels == 1 and meta.subtype == "PCM_16"
    assert info["start"] == 2.0 and info["end"] == 5.0 and info["seconds"] == 3.0
    assert info["trimmed"] is False and info["codec"] == "wav mono 16 kHz 16-bit"
    assert info["file_duration"] == 10.0 and info["bytes"] == len(data)
    assert abs(meta.frames - 3 * 16000) <= 2


def test_c_t5_encode_trim_and_summary(clip10, fake_agy):
    _, _, info = llm.encode(clip10, max_seconds=4)
    assert info["end"] == 4.0 and info["trimmed"] is True
    fake_agy.reply(json.dumps(FULL_ANSWER))
    res = llm.critique(clip10, consent=True, max_seconds=4)
    assert res["uploaded"]["trimmed"] is True
    assert "Only the first 4 s were sent" in res["summary"]


def test_c_t6_empty_range(clip10, fake_agy):
    with pytest.raises(ValueError, match="empty range 8.000-3.000 s"):
        llm.encode(clip10, start=8, end=3)
    res = llm.critique(clip10, consent=True, start=8, end=3)
    assert res["enabled"] is True and res["ok"] is False and res["uploaded"] is None
    assert "empty range" in res["error"] and res["summary"].startswith("LLM critique failed: ")
    assert fake_agy.calls() == []


# ---------------------------------------------------------------- prompt


def test_c_t7_prompt_with_question_and_intent():
    text = llm.build_prompt("CTX LINE", question="Is the hit late?", synth_intended=True)
    assert "\n\nContext:\nCTX LINE" in text
    assert llm._INTENT in text and "Is the hit late?" in text
    assert '"answer"' in text
    assert text.index("Context:") < text.index(llm._INTENT) < text.index("Is the hit late?")


def test_c_t8_prompt_plain():
    text = llm.build_prompt("CTX LINE")
    assert '"answer"' not in text and llm._INTENT not in text
    assert text.rstrip().endswith("CTX LINE")


# ---------------------------------------------------------------- answer


def test_c_t9_parse_fenced_prose_trailing_comma():
    text = 'Here is my review:\n```json\n{"overall": {"score": 7, "verdict": "ok",}, "issues": [1, 2,],}\n```'
    obj = llm.parse_json(text)
    assert isinstance(obj, dict) and obj["overall"]["score"] == 7
    assert llm.parse_json('Sure. {"a": "x } y", "b": {"c": 1}} trailing words') == {"a": "x } y", "b": {"c": 1}}


def test_c_t10_parse_none():
    assert llm.parse_json("no json here") is None


def test_c_t11_normalize_severity_and_offset():
    obj = {"issues": [{"severity": "HIGH", "start": 5}, {"severity": "odd", "start": 1},
                      {"severity": "low", "start": None}, "not a dict"]}
    issues = llm.normalize(obj, offset=10, clip_len=20)["issues"]
    assert [i["severity"] for i in issues] == ["high", "medium", "low"]
    assert [i["start"] for i in issues] == [15.0, 11.0, None]


def test_c_t12_normalize_clamps_clock_time():
    issues = llm.normalize({"issues": [{"start": "0:30", "severity": "low"}]}, offset=0, clip_len=20)["issues"]
    assert issues[0]["start"] == 20.0 and issues[0]["end"] == 20.0
    assert llm.seconds_of("0:12.5") == 12.5 and llm.seconds_of("12.4s") == 12.4 and llm.seconds_of(True) is None


def test_c_t13_normalize_scores():
    c = llm.normalize({"scores": {"sync": None, "realism": "8/10", "mix_balance": 12}})
    assert c["scores"] == {"sync": None, "realism": 8.0, "mix_balance": 10.0, "loudness": None, "music": None}
    assert c["overall"] == {"score": None, "verdict": ""}


def test_c_t14_full_answer(clip10, fake_agy):
    fake_agy.reply(json.dumps(FULL_ANSWER), usage={"input_tokens": 9000, "output_tokens": 300})
    res = llm.critique(clip10, consent=True)
    assert res["ok"] is True and res["enabled"] is True
    assert "6/10" in res["summary"] and "3 issues (1 high)" in res["summary"] and "Top fix:" in res["summary"]
    assert res["model"] == llm.AGY_DEFAULT_MODEL and res["uploaded"]["method"] == "agy"
    assert res["usage"] == {"promptTokenCount": 9000, "candidatesTokenCount": 300}
    lines = res["text"].splitlines()
    assert any(line.startswith("scores (1-10, LLM opinion): sync 5") for line in lines)
    issue_lines = [line for line in lines if re.match(r"  \d+:\d\d\.\d\d", line) and "-> fix:" in line]
    assert len(issue_lines) == 3 and "high sync" in issue_lines[0]
    assert "  0:04.00-0:04.20 high sync: hit late -> fix: move 80 ms" in lines
    argv = fake_agy.calls()[0]
    assert argv[0] == "-p" and "--add-dir" in argv and "--output-format" in argv
    assert argv[argv.index("--model") + 1] == llm.AGY_DEFAULT_MODEL
    assert argv[argv.index("--print-timeout") + 1] == "240s"
    folder = argv[argv.index("--add-dir") + 1]
    assert os.path.join(folder, "clip.wav") in argv[1] and not os.path.exists(folder)


def test_c_t14_model_override(clip10, fake_agy, monkeypatch):
    fake_agy.reply(json.dumps(FULL_ANSWER))
    monkeypatch.setenv("HARK_LLM_MODEL", "gemini-3.8-flash-high")
    llm.critique(clip10, consent=True)
    argv = fake_agy.calls()[-1]
    assert argv[argv.index("--model") + 1] == "gemini-3.8-flash-high"


def test_c_t15_answer_not_json(clip10, fake_agy, capsys):
    fake_agy.reply("I liked it a lot, no JSON from me.")
    res = llm.critique(clip10, consent=True)
    assert res["ok"] is False and res["raw"].startswith("I liked it")
    assert "critique" not in res
    assert run_cli(["llm", clip10, "--yes-upload"]) == 1


def test_c_t15_agy_failure_is_a_result(clip10, fake_agy):
    fake_agy.reply(raw=json.dumps({"response": "", "status": "ERROR", "finished_reason": "print_timeout"}))
    res = llm.critique(clip10, consent=True)
    assert res["ok"] is False and res["enabled"] is True and "agy stopped" in res["error"]


# ---------------------------------------------------------------- context


def test_c_t16_cut_line(wav, tone, fake_module, tmp_path):
    film = tmp_path / "film.mp4"
    film.write_bytes(b"not really a film")
    audio = wav(tone(dur=12.0), name="mix.wav")
    fake_module("hark.timeline.video", cuts=lambda video: [1.0, 4.0, 12.0])
    clip = {"start": 3.0, "end": 10.0, "seconds": 7.0, "file_duration": 12.0, "codec": llm.CLIP_CODEC}
    text, notes = llm.context(audio, clip, video=str(film), with_timeline=False)
    cut_lines = [line for line in text.splitlines() if line.startswith("Picture cuts")]
    assert len(cut_lines) == 1
    assert re.search(r"clip seconds: (.*?)\. The detector", cut_lines[0]).group(1) == "1"
    assert notes == []


def test_c_t17_timeline_fails(clip10, fake_agy, fake_module):
    def broken(*a, **kw):
        raise RuntimeError("no models here")

    fake_module("hark.timeline.timeline", timeline=broken)
    fake_agy.reply(json.dumps(FULL_ANSWER))
    res = llm.critique(clip10, consent=True, with_timeline=True)
    assert res["ok"] is True
    assert any("NOT sent" in n and "RuntimeError" in n for n in res["notes"])
    assert "(note: listening transcript NOT sent" in res["text"]


def test_c_t17_timeline_transcript_sent(clip10, fake_agy, fake_module):
    fake_module("hark.timeline.timeline", timeline=lambda *a, **kw: {"summary": "s", "text": "0.50 s hit"})
    fake_agy.reply(json.dumps(FULL_ANSWER))
    res = llm.critique(clip10, consent=True, with_timeline=True, start=1.0)
    assert "0.50 s hit" in res["context_sent"] and "clip starts at file second 1" in res["context_sent"]


# ---------------------------------------------------------------- MCP and keys


def test_c_t18_mcp_without_consent(clip10, fake_agy):
    res = mcp_server.call_tool("llm_critique", path=clip10)
    assert "how" in res.text.lower() and "consent" in res.text.lower()
    assert "JSON (compact): " in res.text
    row = next(t for t in mcp_server.tool_list()["tools"] if t["name"] == "llm_critique")
    assert row["network"] is True and row["read_only"] is False
    assert mcp_server.TOOL_BY_NAME["llm_critique"].network is True
    assert fake_agy.calls() == []


class EnvGuard(dict):
    """A copy of the environment that records every key-like name read through it."""

    pattern = re.compile(r"API_?KEY", re.IGNORECASE)

    def __init__(self, data):
        super().__init__(data)
        self.touched: list[str] = []

    def _check(self, key):
        if self.pattern.search(str(key)):
            self.touched.append(str(key))

    def __getitem__(self, key):
        self._check(key)
        return super().__getitem__(key)

    def get(self, key, default=None):
        self._check(key)
        return super().get(key, default)

    def __contains__(self, key):
        self._check(key)
        return super().__contains__(key)


def test_c_t19_no_key_read(clip10, fake_agy, monkeypatch):
    fake_agy.reply(json.dumps(FULL_ANSWER))
    for name in ("ANTHROPIC_API_KEY", "GEMINI_API_KEY", "GOOGLE_API_KEY"):
        monkeypatch.setenv(name, "sk-test-not-a-real-key-000000")
    guard = EnvGuard(os.environ)
    monkeypatch.setattr(os, "environ", guard)
    res = llm.critique(clip10, consent=True)
    assert res["ok"] is True
    assert guard.touched == []
    env = llm.child_env()
    assert not any(EnvGuard.pattern.search(k) for k in env)
    assert guard.touched == []


def test_c_t19_secret_scrubbed():
    assert "***" in llm.scrub("failed with key AIzaSyA1234567890abcdefghijklmn")


# ---------------------------------------------------------------- live


@pytest.mark.live
def test_c_t20_live_agy(wav, clicks):
    path = wav(clicks(times=(0.0, 0.5, 1.0, 1.5), dur=2.0), name="clicks.wav")
    res = llm.critique(path, consent=True)
    assert isinstance(res, dict) and "Traceback" not in res.get("text", "")
    assert res["ok"] is True or res["summary"].startswith(("LLM critique failed", "agy answered"))


def test_unknown_provider(clip10):
    with pytest.raises(ValueError, match="providers: agy"):
        llm.critique(clip10, provider="nope")


def test_missing_file(tmp_path):
    with pytest.raises(RuntimeError, match="no such file"):
        llm.critique(str(tmp_path / "missing.wav"))


def test_render_span_formats():
    assert llm._span_text({"start": None}) == "(no time)"
    assert llm._span_text({"start": 62.0, "end": 62.0}) == "1:02.00"
    assert llm._span_text({"start": 4.2, "end": 5.0}) == "0:04.20-0:05.00"


def test_wav_clip_is_mono_16k(wav):
    stereo = np.column_stack([np.zeros(48000), 0.5 * np.ones(48000)]).astype(np.float32)
    data, _, info = llm.encode(wav(stereo, name="st.wav"))
    assert wav_info(data).channels == 1 and info["seconds"] == 1.0
