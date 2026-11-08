"""Report helpers: emit, JSON cleaning, text form, error lines, fmt_t (core-io 3.6, 4.4, 4.5)."""

from __future__ import annotations

import argparse
import io
import json
import math
import sys
from pathlib import Path

import numpy as np
import pytest

from hark.core import report


def _args(**kw) -> argparse.Namespace:
    return argparse.Namespace(**{"json": False, "out": None, **kw})


def _stream(encoding: str) -> io.TextIOWrapper:
    return io.TextIOWrapper(io.BytesIO(), encoding=encoding, errors="strict")


def _bytes(stream: io.TextIOWrapper) -> bytes:
    stream.flush()
    return stream.buffer.getvalue()


def test_at_11_json_on_a_cp1252_stream(monkeypatch):
    out = _stream("cp1252")
    monkeypatch.setattr(sys, "stdout", out)
    report.emit({"summary": "ok", "file": "キー音.wav"}, _args(json=True))
    parsed = json.loads(_bytes(out).decode("cp1252"))
    assert parsed["file"] == "キー音.wav"


def test_e_c8_text_on_an_ascii_stream_escapes(monkeypatch):
    out = _stream("ascii")
    monkeypatch.setattr(sys, "stdout", out)
    report.emit({"summary": "キー音.wav measured"}, _args())
    text = _bytes(out).decode("ascii")
    assert "\\u30ad" in text


def test_e_c9_out_makes_folders_and_keeps_non_ascii(tmp_path, capsys):
    target = tmp_path / "a" / "b" / "r.json"
    report.emit({"summary": "ok", "name": "é"}, _args(out=str(target)))
    assert json.loads(target.read_text(encoding="utf-8"))["name"] == "é"
    assert "é" in target.read_text(encoding="utf-8")
    assert capsys.readouterr().out.startswith("ok")


def test_emit_text_choice(capsys):
    report.emit({"summary": "s", "text": "from field"}, _args())
    report.emit({"summary": "s", "text": "from field"}, _args(), text="from arg")
    report.emit({"summary": "s", "x": 1}, _args())
    assert capsys.readouterr().out.splitlines() == ["from field", "from arg", "s", "x: 1"]


def test_render_text_cuts_long_values():
    text = report.render_text({"summary": "s", "text": "skip", "big": list(range(1000)), "d": {"a": 1}})
    lines = text.splitlines()
    assert lines[0] == "s"
    assert lines[1].startswith("big: [0,1,2") and lines[1].endswith(" ...")
    assert len(lines[1]) == len("big: ") + 600 + 4
    assert lines[2] == 'd: {"a":1}'


def test_e_f13_clean_numpy_nan_path():
    raw = {1: (np.float32(1.5), np.int64(3)), "a": np.array([1.0, np.nan]), "p": Path("x/y"),
           "inf": math.inf, "b": np.bool_(True), "o": object}
    out = report.clean(raw)
    assert out["1"] == [1.5, 3]
    assert out["a"] == [1.0, None]
    assert out["p"] == str(Path("x/y"))
    assert out["inf"] is None and out["b"] is True
    assert isinstance(out["o"], str)
    json.dumps(out, allow_nan=False)


def test_e_c11_error_line_is_one_line(capsys):
    report.error_line("measure", "first\n  second\tthird ")
    report.error_line("x", "  ")
    err = capsys.readouterr().err.splitlines()
    assert err == ["hark measure: error: first second third", "hark x: error: unknown error"]


@pytest.mark.parametrize("t, text", [(63.5, "1:03.50"), (4.19, "0:04.19"), (None, "-"), (0, "0:00.00"),
                                     (59.999, "1:00.00"), (600.0, "10:00.00")])
def test_fmt_t(t, text):
    assert report.fmt_t(t) == text


def test_add_common_flags():
    p = argparse.ArgumentParser()
    report.add_common(p)
    args = p.parse_args(["--json", "--out", "f.json", "--threads", "3"])
    assert (args.json, args.out, args.threads) == (True, "f.json", 3)


def test_expected_errors_path(capsys):
    from hark.core.audio import AudioError

    @report.expected_errors("describe")
    def handler(args):
        raise AudioError("no such file: x.wav")

    assert handler(_args(json=True)) == 1
    captured = capsys.readouterr()
    assert captured.err.strip() == "hark describe: error: no such file: x.wav"
    assert json.loads(captured.out) == {"summary": "error: no such file: x.wav", "error": "no such file: x.wav"}


def test_expected_errors_lets_other_errors_pass():
    @report.expected_errors("describe")
    def handler(args):
        raise KeyError("other")

    with pytest.raises(KeyError):
        handler(_args())
