#!/usr/bin/env python3
"""Phase 3 audio gate for a video project: checks audio/mix.wav before render.

Usage:
  check_audio.py <audio/mix.wav> [--timeline timeline.json] [--beats audio/beats.json]
                 [--out qa/audio-check.json]
  check_audio.py --self-test

Exit codes: 0 every gate passes, 1 a gate fails, 2 usage or tool error.
Needs ffmpeg and ffprobe on PATH. Python standard library only.
"""

import argparse
import json
import math
import os
import re
import shutil
import subprocess
import sys
import tempfile

SAMPLE_RATE = 48000
CHANNELS = 2
DURATION_TOLERANCE_SEC = 0.05
LUFS_MIN = -15.0
LUFS_MAX = -13.0
TRUE_PEAK_MAX_DBTP = -1.0
EDGE_SILENCE_MAX_SEC = 0.20
DROPOUT_MAX_SEC = 0.80
ALLOWED_SILENCE_SLACK_SEC = 0.05
EDGE_EPSILON_SEC = 0.01
SILENCE_FILTER = "ebur128=peak=true,silencedetect=n=-50dB:d=0.2"
SKILL_MD_MAX_BYTES = 12288
REFERENCE_MAX_BYTES = 24576
TEXT_SUFFIXES = (".md", ".py", ".txt")


class ToolError(Exception):
    """ffmpeg, ffprobe or an input file cannot be used."""


def _run(cmd):
    try:
        return subprocess.run(cmd, capture_output=True, text=True, check=False)
    except FileNotFoundError as exc:
        raise ToolError(f"{cmd[0]} is not on PATH") from exc


def probe(path):
    """Return sample rate, channels and duration of the first audio stream."""
    proc = _run([
        "ffprobe", "-v", "error", "-select_streams", "a:0",
        "-show_entries", "stream=sample_rate,channels:format=duration",
        "-of", "json", path,
    ])
    if proc.returncode != 0:
        raise ToolError(f"ffprobe cannot read {path}: {proc.stderr.strip()}")
    data = json.loads(proc.stdout or "{}")
    streams = data.get("streams") or []
    if not streams:
        raise ToolError(f"{path} has no audio stream")
    return {
        "sample_rate": int(streams[0].get("sample_rate", 0)),
        "channels": int(streams[0].get("channels", 0)),
        "duration_sec": float(data.get("format", {}).get("duration", 0.0)),
    }


def _db(text):
    return -math.inf if text == "-inf" else float(text)


def measure(path):
    """Run ebur128 and silencedetect. Return loudness, true peak and silences."""
    proc = _run([
        "ffmpeg", "-hide_banner", "-nostats", "-i", path,
        "-af", SILENCE_FILTER, "-f", "null", "-",
    ])
    if proc.returncode != 0:
        raise ToolError(f"ffmpeg cannot analyse {path}: {proc.stderr.strip()[-400:]}")
    log = proc.stderr
    summary = log[log.rfind("Summary:"):] if "Summary:" in log else ""
    lufs = re.search(r"I:\s+(-?[\d.]+|-inf) LUFS", summary)
    peak = re.search(r"True peak:\s+Peak:\s+(-?[\d.]+|-inf) dBFS", summary)
    if not lufs or not peak:
        raise ToolError("ebur128 summary is missing from the ffmpeg log")
    silences = []
    start = None
    for line in log.splitlines():
        found = re.search(r"silence_start: (-?[\d.]+)", line)
        if found:
            start = max(0.0, float(found.group(1)))
            continue
        found = re.search(r"silence_end: (-?[\d.]+)", line)
        if found and start is not None:
            silences.append([start, float(found.group(1))])
            start = None
    return {
        "integrated_lufs": _db(lufs.group(1)),
        "true_peak_dbtp": _db(peak.group(1)),
        "silences": silences,
    }


def _err(errors, rule, message):
    errors.append({"rule": rule, "message": message})


def _load_json(path):
    try:
        with open(path, encoding="utf-8") as handle:
            return json.load(handle)
    except (OSError, ValueError) as exc:
        raise ToolError(f"cannot read JSON {path}: {exc}") from exc


def _allowed(timeline):
    meta = timeline.get("meta", {}) if isinstance(timeline, dict) else {}
    ranges = meta.get("allowed_silences", []) if isinstance(meta, dict) else []
    out = []
    for item in ranges if isinstance(ranges, list) else []:
        if isinstance(item, list) and len(item) == 2 and all(_is_num(v) for v in item):
            out.append((float(item[0]), float(item[1])))
    return out


def _is_num(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value)


def check_silences(silences, duration, allowed, errors):
    for start, end in silences:
        length = end - start
        if start <= EDGE_EPSILON_SEC:
            if length > EDGE_SILENCE_MAX_SEC:
                _err(errors, "silence.leading",
                     f"leading silence is {length:.3f} s; the limit is {EDGE_SILENCE_MAX_SEC} s")
        elif end >= duration - EDGE_EPSILON_SEC:
            if length > EDGE_SILENCE_MAX_SEC:
                _err(errors, "silence.trailing",
                     f"trailing silence is {length:.3f} s; the limit is {EDGE_SILENCE_MAX_SEC} s")
        elif length > DROPOUT_MAX_SEC and not any(
            start >= lo - ALLOWED_SILENCE_SLACK_SEC and end <= hi + ALLOWED_SILENCE_SLACK_SEC
            for lo, hi in allowed
        ):
            _err(errors, "silence.dropout",
                 f"silence below -50 dBFS from {start:.3f} s to {end:.3f} s "
                 f"({length:.3f} s) is not in meta.allowed_silences")


def check_beats(beats, duration, errors):
    if not isinstance(beats, dict):
        _err(errors, "beats.schema", "beats.json must be a JSON object")
        return
    bpm = beats.get("bpm")
    if not _is_num(bpm) or bpm <= 0:
        _err(errors, "beats.bpm", f"bpm must be a number > 0, got {bpm!r}")
    for key in ("beats_sec", "downbeats_sec"):
        values = beats.get(key)
        if not isinstance(values, list) or not values or not all(_is_num(v) for v in values):
            _err(errors, "beats.schema", f"{key} must be a non-empty list of numbers")
            continue
        if any(b <= a for a, b in zip(values, values[1:])):
            _err(errors, "beats.order", f"{key} is not strictly increasing")
        if values[0] < 0 or values[-1] > duration + DURATION_TOLERANCE_SEC:
            _err(errors, "beats.range", f"{key} leaves [0, {duration:.3f}] s")


def check(mix, timeline_path=None, beats_path=None):
    """Return the report dict. Raises ToolError when an input cannot be read."""
    errors = []
    info = probe(mix)
    metrics = dict(info)
    metrics.update(measure(mix))
    timeline = _load_json(timeline_path) if timeline_path else {}

    if info["sample_rate"] != SAMPLE_RATE:
        _err(errors, "format.sample_rate",
             f"sample rate is {info['sample_rate']} Hz; it must be {SAMPLE_RATE} Hz")
    if info["channels"] != CHANNELS:
        _err(errors, "format.channels", f"channels is {info['channels']}; it must be {CHANNELS}")
    if timeline_path:
        target = (timeline.get("meta") or {}).get("duration_sec") if isinstance(timeline, dict) else None
        if not _is_num(target):
            _err(errors, "format.duration", "timeline.json meta.duration_sec is missing")
        elif abs(info["duration_sec"] - target) > DURATION_TOLERANCE_SEC:
            _err(errors, "format.duration",
                 f"mix is {info['duration_sec']:.3f} s; timeline is {target:.3f} s "
                 f"(tolerance {DURATION_TOLERANCE_SEC} s)")

    lufs = metrics["integrated_lufs"]
    if not LUFS_MIN <= lufs <= LUFS_MAX:
        _err(errors, "loudness.integrated",
             f"integrated loudness is {lufs} LUFS; it must be in [{LUFS_MIN}, {LUFS_MAX}] LUFS")
    peak = metrics["true_peak_dbtp"]
    if peak > TRUE_PEAK_MAX_DBTP:
        _err(errors, "loudness.true_peak",
             f"true peak is {peak} dBTP; it must be <= {TRUE_PEAK_MAX_DBTP} dBTP")

    check_silences(metrics["silences"], info["duration_sec"], _allowed(timeline), errors)
    if beats_path:
        check_beats(_load_json(beats_path), info["duration_sec"], errors)

    for key in ("integrated_lufs", "true_peak_dbtp"):
        if math.isinf(metrics[key]):
            metrics[key] = None
    return {"passed": not errors, "file": mix, "errors": errors, "metrics": metrics}


def write_report(report, out):
    if not out:
        return
    parent = os.path.dirname(out)
    if parent:
        os.makedirs(parent, exist_ok=True)
    with open(out, "w", encoding="utf-8") as handle:
        json.dump(report, handle, indent=2)
        handle.write("\n")


def main(argv=None):
    parser = argparse.ArgumentParser(description="Check audio/mix.wav against the Phase 3 gates.")
    parser.add_argument("mix", nargs="?", help="path to audio/mix.wav")
    parser.add_argument("--timeline", help="timeline.json with meta.duration_sec")
    parser.add_argument("--beats", help="audio/beats.json with bpm, beats_sec, downbeats_sec")
    parser.add_argument("--out", help="write the JSON report here, e.g. qa/audio-check.json")
    parser.add_argument("--self-test", action="store_true", help="run the built-in tests")
    args = parser.parse_args(argv)
    if args.self_test:
        return self_test()
    if not args.mix:
        parser.error("give a mix path or --self-test")
    try:
        report = check(args.mix, args.timeline, args.beats)
    except ToolError as exc:
        print(json.dumps({"passed": False, "errors": [{"rule": "input", "message": str(exc)}]}))
        return 2
    write_report(report, args.out)
    print(json.dumps(report, indent=2))
    return 0 if report["passed"] else 1


# ---------------------------------------------------------------- self-test

def _synth(path, expr, seconds, target_lufs, rate=SAMPLE_RATE, channels=CHANNELS):
    """Render an aevalsrc expression, then set its integrated loudness to target_lufs."""
    raw = path + ".raw.wav"
    exprs = "|".join([expr] * channels)
    proc = _run([
        "ffmpeg", "-hide_banner", "-v", "error", "-y", "-f", "lavfi",
        "-i", f"aevalsrc=exprs='{exprs}':s={rate}:d={seconds}",
        "-c:a", "pcm_s16le", raw,
    ])
    if proc.returncode != 0:
        raise ToolError(proc.stderr)
    gain = target_lufs - measure(raw)["integrated_lufs"]
    proc = _run([
        "ffmpeg", "-hide_banner", "-v", "error", "-y", "-i", raw,
        "-af", f"volume={gain:.3f}dB", "-c:a", "pcm_s16le", path,
    ])
    os.remove(raw)
    if proc.returncode != 0:
        raise ToolError(proc.stderr)


def _write(path, data):
    with open(path, "w", encoding="utf-8") as handle:
        json.dump(data, handle)


def _rules(report):
    return sorted({e["rule"] for e in report["errors"]})


def self_test():
    here = os.path.dirname(os.path.abspath(__file__))
    skill_dir = os.path.dirname(here)
    failures = []

    def expect(name, report, rules):
        got = _rules(report)
        ok = report["passed"] if not rules else set(rules) <= set(got) and not report["passed"]
        print(f"{'ok  ' if ok else 'FAIL'} {name}: rules={got}")
        if not ok:
            failures.append(f"{name}: want {rules}, got {got} {report['metrics']}")

    if not shutil.which("ffmpeg") or not shutil.which("ffprobe"):
        print("FAIL ffmpeg and ffprobe must be on PATH")
        return 1

    tone = "0.2*sin(2*PI*440*t)"
    with tempfile.TemporaryDirectory(prefix="check-audio-") as tmp:
        def p(name):
            return os.path.join(tmp, name)

        _synth(p("good.wav"), tone, 4, -14.0)
        _write(p("timeline.json"), {"meta": {"duration_sec": 4.0}})
        _write(p("beats.json"), {"bpm": 120, "beats_sec": [0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5],
                                  "downbeats_sec": [0.0, 2.0]})
        expect("pass -14 LUFS stereo", check(p("good.wav"), p("timeline.json"), p("beats.json")), [])

        _synth(p("quiet.wav"), tone, 4, -20.0)
        expect("fail -20 LUFS", check(p("quiet.wav")), ["loudness.integrated"])

        burst = f"{tone}+if(between(t,1.5,1.52),0.8*sin(2*PI*3000*t),0)"
        _synth(p("hot.wav"), burst, 4, -14.0)
        expect("fail true peak > -1.0 dBTP", check(p("hot.wav")), ["loudness.true_peak"])

        gap = f"if(between(t,1.8,3.0),0,{tone})"
        _synth(p("gap.wav"), gap, 5, -14.0)
        expect("fail 1.2 s internal dropout", check(p("gap.wav")), ["silence.dropout"])
        _write(p("allowed.json"), {"meta": {"duration_sec": 5.0, "allowed_silences": [[1.8, 3.0]]}})
        expect("pass allowed silence range", check(p("gap.wav"), p("allowed.json")), [])

        _synth(p("lead.wav"), f"if(lt(t,0.5),0,{tone})", 4, -14.0)
        expect("fail 0.5 s leading silence", check(p("lead.wav")), ["silence.leading"])
        _synth(p("tail.wav"), f"if(gt(t,3.5),0,{tone})", 4, -14.0)
        expect("fail 0.5 s trailing silence", check(p("tail.wav")), ["silence.trailing"])

        _synth(p("mono.wav"), tone, 4, -14.0, rate=44100, channels=1)
        expect("fail mono 44.1 kHz", check(p("mono.wav")), ["format.channels", "format.sample_rate"])

        _write(p("long.json"), {"meta": {"duration_sec": 4.2}})
        expect("fail duration mismatch", check(p("good.wav"), p("long.json")), ["format.duration"])

        _write(p("bad-beats.json"), {"bpm": 0, "beats_sec": [0.0, 1.0, 0.5],
                                      "downbeats_sec": [0.0, 9.0]})
        expect("fail invalid beats.json", check(p("good.wav"), None, p("bad-beats.json")),
               ["beats.bpm", "beats.order", "beats.range"])

        out = p("qa/audio-check.json")
        code = subprocess.run([sys.executable, os.path.abspath(__file__), p("quiet.wav"),
                               "--out", out], capture_output=True, check=False).returncode
        ok = code == 1 and os.path.isfile(out) and _load_json(out)["passed"] is False
        print(f"{'ok  ' if ok else 'FAIL'} CLI exit 1 and --out report")
        if not ok:
            failures.append(f"CLI: exit {code}, report exists {os.path.isfile(out)}")

    skill_md = os.path.join(skill_dir, "SKILL.md")
    size = os.path.getsize(skill_md) if os.path.isfile(skill_md) else SKILL_MD_MAX_BYTES
    print(f"{'ok  ' if size < SKILL_MD_MAX_BYTES else 'FAIL'} SKILL.md is {size} bytes")
    if size >= SKILL_MD_MAX_BYTES:
        failures.append(f"SKILL.md is {size} bytes; the limit is < {SKILL_MD_MAX_BYTES}")
    ref = os.path.join(skill_dir, "references", "lavfi-sfx-recipes.md")
    ref_size = os.path.getsize(ref) if os.path.isfile(ref) else REFERENCE_MAX_BYTES
    print(f"{'ok  ' if ref_size < REFERENCE_MAX_BYTES else 'FAIL'} reference is {ref_size} bytes")
    if ref_size >= REFERENCE_MAX_BYTES:
        failures.append(f"references/lavfi-sfx-recipes.md is missing or >= {REFERENCE_MAX_BYTES} bytes")
    binaries = [os.path.join(root, name) for root, _, names in os.walk(skill_dir)
                for name in names if not name.endswith(TEXT_SUFFIXES)
                and "__pycache__" not in root]
    print(f"{'ok  ' if not binaries else 'FAIL'} no binary files in the skill: {binaries}")
    if binaries:
        failures.append(f"binary files in the skill: {binaries}")

    print("self-test PASSED" if not failures else "self-test FAILED:\n  " + "\n  ".join(failures))
    return 0 if not failures else 1


if __name__ == "__main__":
    sys.exit(main())
