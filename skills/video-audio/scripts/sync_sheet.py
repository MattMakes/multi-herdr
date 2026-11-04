#!/usr/bin/env python3
"""Write audio/sync-sheet.json: voice segments, word times and the beat grid.

Usage:
  sync_sheet.py --timeline timeline.json [--audio-dir audio] [--beats audio/beats.json]
                [--out audio/sync-sheet.json] [--whisper-model <model.bin>]
  sync_sheet.py --self-test

Run it after the timing reconciliation and the beat grid, so that every
scene start_sec is final. Word times are in project time (seconds from the
start of the video).

Estimated route (always): ffmpeg silencedetect splits each audio/vo-<id>.wav
into phrases. The words of a phrase get times in proportion to their syllable
count. The sheet says "timing": "estimated".

Measured route (optional): when a whisper.cpp binary (whisper-cli or
whisper-cpp) is on PATH and --whisper-model names a model file on disk, its
word timestamps replace the estimate for each segment whose words match the
narration. The sheet says "timing": "measured" only when every segment is
measured. Nothing is downloaded.

Exit codes: 0 sheet written, 2 usage, input or tool error.
Needs ffmpeg and ffprobe on PATH. Python standard library only.
"""

import argparse
import difflib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile

SILENCE_DB = -40
SILENCE_MIN_SEC = 0.15
SPAN_MIN_SEC = 0.04
DEFAULT_LEAD_IN_SEC = 0.15
PHRASE_END = (",", ".", ";", ":", "!", "?")
DASHES = ("-", "--", "–")
WHISPER_BINARIES = ("whisper-cli", "whisper-cpp")
WHISPER_MATCH_MIN = 0.8
PHRASE_START_TOLERANCE_SEC = 0.10


class ToolError(Exception):
    """ffmpeg, ffprobe or an input file cannot be used."""


def _run(cmd):
    try:
        return subprocess.run(cmd, capture_output=True, text=True, check=False)
    except FileNotFoundError as exc:
        raise ToolError(f"{cmd[0]} is not on PATH") from exc


def _load_json(path):
    try:
        with open(path, encoding="utf-8") as handle:
            return json.load(handle)
    except (OSError, ValueError) as exc:
        raise ToolError(f"cannot read JSON {path}: {exc}") from exc


def duration_of(path):
    proc = _run(["ffprobe", "-v", "error", "-show_entries", "format=duration",
                 "-of", "csv=p=0", path])
    if proc.returncode != 0:
        raise ToolError(f"ffprobe cannot read {path}: {proc.stderr.strip()}")
    try:
        return float(proc.stdout.strip())
    except ValueError as exc:
        raise ToolError(f"ffprobe gave no duration for {path}") from exc


def speech_spans(path, duration):
    """Return [start, end] spans of speech: the gaps between detected silences."""
    proc = _run(["ffmpeg", "-hide_banner", "-nostats", "-i", path, "-af",
                 f"silencedetect=n={SILENCE_DB}dB:d={SILENCE_MIN_SEC}", "-f", "null", "-"])
    if proc.returncode != 0:
        raise ToolError(f"ffmpeg cannot analyse {path}: {proc.stderr.strip()[-400:]}")
    silences, start = [], None
    for line in proc.stderr.splitlines():
        found = re.search(r"silence_start: (-?[\d.]+)", line)
        if found:
            start = max(0.0, float(found.group(1)))
            continue
        found = re.search(r"silence_end: (-?[\d.]+)", line)
        if found and start is not None:
            silences.append((start, min(duration, float(found.group(1)))))
            start = None
    if start is not None:
        silences.append((start, duration))
    spans, cursor = [], 0.0
    for lo, hi in silences:
        if lo - cursor >= SPAN_MIN_SEC:
            spans.append([cursor, lo])
        cursor = max(cursor, hi)
    if duration - cursor >= SPAN_MIN_SEC:
        spans.append([cursor, duration])
    return spans or [[0.0, duration]]


def syllables(word):
    """Count vowel groups as syllables, and 1 for each digit. At least 1."""
    text = re.sub(r"[^a-z0-9]", "", word.lower())
    digits = sum(ch.isdigit() for ch in text)
    letters = re.sub(r"\d", "", text)
    groups = len(re.findall(r"[aeiouy]+", letters))
    if groups > 1 and letters.endswith("e") and not letters.endswith(("le", "ee")):
        groups -= 1
    return max(1, groups + digits)


def tokenize(text):
    """Return (words, text_phrases). A text phrase ends at punctuation or a dash."""
    words, phrases, current = [], [], []
    for token in text.split():
        if not re.search(r"[A-Za-z0-9]", token):
            if token in DASHES and current:
                phrases.append(current)
                current = []
            continue
        current.append(len(words))
        words.append(token)
        if token.endswith(PHRASE_END):
            phrases.append(current)
            current = []
    if current:
        phrases.append(current)
    return words, phrases


def _layout(words, indexes, lo, hi):
    weights = [syllables(words[i]) for i in indexes]
    total, cursor, out = sum(weights), lo, []
    for i, weight in zip(indexes, weights):
        end = cursor + (hi - lo) * weight / total
        out.append({"word": words[i], "start_sec": cursor, "end_sec": end})
        cursor = end
    return out


def _assign_by_mass(words, spans):
    """Give each word the span that holds its syllable midpoint on the speech time line."""
    weights = [syllables(w) for w in words]
    total = sum(weights)
    lengths = [hi - lo for lo, hi in spans]
    speech = sum(lengths)
    bounds, acc = [], 0.0
    for length in lengths:
        acc += length / speech
        bounds.append(acc)
    groups = [[] for _ in spans]
    mass = 0.0
    for i, weight in enumerate(weights):
        mid = (mass + weight / 2) / total
        mass += weight
        span = next((k for k, b in enumerate(bounds) if mid <= b), len(spans) - 1)
        groups[span].append(i)
    return groups


def estimate_words(text, spans):
    """Return (phrases, words) with times relative to the voice file start."""
    words, text_phrases = tokenize(text)
    if not words:
        return [], []
    groups = text_phrases if len(text_phrases) == len(spans) else _assign_by_mass(words, spans)
    phrases, timed = [], []
    for (lo, hi), indexes in zip(spans, groups):
        if indexes:
            phrases.append([lo, hi])
            timed.extend(_layout(words, indexes, lo, hi))
    return phrases, timed


def _norm(word):
    return re.sub(r"[^a-z0-9]", "", word.lower())


def parse_whisper_json(data):
    """Return [(text, start_sec, end_sec)] from whisper.cpp -oj output with -ml 1."""
    out = []
    for item in data.get("transcription", []) if isinstance(data, dict) else []:
        text = str(item.get("text", "")).strip()
        offsets = item.get("offsets") or {}
        if not _norm(text) or "from" not in offsets or "to" not in offsets:
            continue
        out.append((text, offsets["from"] / 1000.0, offsets["to"] / 1000.0))
    return out


def measured_words(text, tokens, duration):
    """Map whisper tokens onto the narration words. Return None when they do not match."""
    words, _ = tokenize(text)
    if len(tokens) != len(words) or not words:
        return None
    ratio = difflib.SequenceMatcher(
        a=[_norm(w) for w in words], b=[_norm(t[0]) for t in tokens], autojunk=False).ratio()
    if ratio < WHISPER_MATCH_MIN:
        return None
    out, floor = [], 0.0
    for word, (_, start, end) in zip(words, tokens):
        start = min(max(start, floor), duration)
        end = min(max(end, start), duration)
        out.append({"word": word, "start_sec": start, "end_sec": end})
        floor = end
    return out


def run_whisper(binary, model, path):
    """Return whisper.cpp word tokens for path, or None when the run fails."""
    with tempfile.TemporaryDirectory(prefix="sync-sheet-") as tmp:
        wav16 = os.path.join(tmp, "vo16.wav")
        base = os.path.join(tmp, "words")
        proc = _run(["ffmpeg", "-hide_banner", "-v", "error", "-y", "-i", path,
                     "-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le", wav16])
        if proc.returncode != 0:
            return None
        proc = _run([binary, "-m", model, "-f", wav16, "-ml", "1", "-sow", "-oj",
                     "-of", base, "-np"])
        if proc.returncode != 0 or not os.path.isfile(base + ".json"):
            return None
        try:
            return parse_whisper_json(_load_json(base + ".json"))
        except ToolError:
            return None


def whisper_route(model):
    """Return (binary, model) when the measured route can run, else None."""
    if not model or not os.path.isfile(model):
        return None
    binary = next((shutil.which(b) for b in WHISPER_BINARIES if shutil.which(b)), None)
    return (binary, model) if binary else None


def _narration(scene):
    value = scene.get("narration")
    if isinstance(value, dict):
        lead = value.get("lead_in_sec", DEFAULT_LEAD_IN_SEC)
        return str(value.get("text", "")), float(lead)
    return (value if isinstance(value, str) else ""), DEFAULT_LEAD_IN_SEC


def _shift(items, offset):
    return [{"word": w["word"], "start_sec": round(w["start_sec"] + offset, 3),
             "end_sec": round(w["end_sec"] + offset, 3)} for w in items]


def build_segment(scene, audio_dir, whisper):
    text, lead_in = _narration(scene)
    path = os.path.join(audio_dir, f"vo-{scene['id']}.wav")
    if not os.path.isfile(path):
        raise ToolError(f"{path} is missing for scene {scene['id']}")
    duration = duration_of(path)
    phrases, words = estimate_words(text, speech_spans(path, duration))
    timing = "estimated"
    if whisper:
        tokens = run_whisper(whisper[0], whisper[1], path)
        measured = measured_words(text, tokens, duration) if tokens else None
        if measured:
            words, timing = measured, "measured"
    start = float(scene["start_sec"]) + lead_in
    return {
        "scene_id": scene["id"],
        "file": path,
        "text": text,
        "start_sec": round(start, 3),
        "end_sec": round(start + duration, 3),
        "timing": timing,
        "phrases": [[round(lo + start, 3), round(hi + start, 3)] for lo, hi in phrases],
        "words": _shift(words, start),
    }


def build(timeline_path, audio_dir, beats_path, whisper_model=None):
    timeline = _load_json(timeline_path)
    beats = _load_json(beats_path)
    scenes = timeline.get("scenes", []) if isinstance(timeline, dict) else []
    whisper = whisper_route(whisper_model)
    segments = [build_segment(s, audio_dir, whisper) for s in scenes if _narration(s)[0].strip()]
    measured = bool(segments) and all(s["timing"] == "measured" for s in segments)
    return {
        "version": 1,
        "timing": "measured" if measured else "estimated",
        "bpm": beats.get("bpm"),
        "beats_sec": beats.get("beats_sec", []),
        "downbeats_sec": beats.get("downbeats_sec", []),
        "vo_segments": segments,
    }


def write_json(data, out):
    parent = os.path.dirname(out)
    if parent:
        os.makedirs(parent, exist_ok=True)
    with open(out, "w", encoding="utf-8") as handle:
        json.dump(data, handle, indent=2)
        handle.write("\n")


def main(argv=None):
    parser = argparse.ArgumentParser(description="Write audio/sync-sheet.json.")
    parser.add_argument("--timeline", help="reconciled timeline.json")
    parser.add_argument("--audio-dir", default="audio", help="folder with vo-<scene-id>.wav")
    parser.add_argument("--beats", default="audio/beats.json", help="the beat grid")
    parser.add_argument("--out", default="audio/sync-sheet.json", help="the sheet to write")
    parser.add_argument("--whisper-model", help="a whisper.cpp model already on disk (optional)")
    parser.add_argument("--self-test", action="store_true", help="run the built-in tests")
    args = parser.parse_args(argv)
    if args.self_test:
        return self_test()
    if not args.timeline:
        parser.error("give --timeline or --self-test")
    try:
        sheet = build(args.timeline, args.audio_dir, args.beats, args.whisper_model)
    except (ToolError, KeyError, TypeError, ValueError) as exc:
        print(json.dumps({"written": False, "error": str(exc)}))
        return 2
    write_json(sheet, args.out)
    print(json.dumps({"written": args.out, "timing": sheet["timing"],
                      "segments": len(sheet["vo_segments"])}))
    return 0


# ---------------------------------------------------------------- self-test

def _tone(path, expr, seconds):
    proc = _run(["ffmpeg", "-hide_banner", "-v", "error", "-y", "-f", "lavfi", "-i",
                 f"aevalsrc=exprs='{expr}':s=48000:d={seconds}", "-c:a", "pcm_s16le", path])
    if proc.returncode != 0:
        raise ToolError(proc.stderr)


def self_test():
    failures = []

    def expect(name, ok, detail=""):
        print(f"{'ok  ' if ok else 'FAIL'} {name}")
        if not ok:
            failures.append(f"{name}: {detail}")

    expect("syllables", [syllables(w) for w in ("Ship", "every", "frame.", "today", "2026")]
           == [1, 3, 1, 2, 4], [syllables(w) for w in ("Ship", "every", "frame.", "today", "2026")])
    words, phrases = tokenize("Ship every frame - then try it today.")
    expect("tokenize breaks at a dash and a full stop", phrases == [[0, 1, 2], [3, 4, 5, 6]],
           f"{words} {phrases}")
    _, timed = estimate_words("one two three", [[0.0, 1.0], [2.0, 3.0]])
    expect("mass assignment keeps order and spans",
           [w["word"] for w in timed] == ["one", "two", "three"]
           and timed[0]["start_sec"] == 0.0 and timed[-1]["end_sec"] == 3.0, timed)

    tokens = parse_whisper_json({"transcription": [
        {"offsets": {"from": 120, "to": 400}, "text": " Ship"},
        {"offsets": {"from": 400, "to": 400}, "text": " "},
        {"offsets": {"from": 400, "to": 700}, "text": " every"},
        {"offsets": {"from": 700, "to": 1150}, "text": " frame."}]})
    got = measured_words("Ship every frame.", tokens, 2.0)
    expect("whisper words map onto the narration",
           got and [w["start_sec"] for w in got] == [0.12, 0.4, 0.7], got)
    expect("whisper words that do not match are refused",
           measured_words("Try it today.", tokens, 2.0) is None)
    expect("no whisper model means no measured route", whisper_route(None) is None)

    if not shutil.which("ffmpeg") or not shutil.which("ffprobe"):
        print("FAIL ffmpeg and ffprobe must be on PATH")
        return 1
    with tempfile.TemporaryDirectory(prefix="sync-sheet-test-") as tmp:
        audio = os.path.join(tmp, "audio")
        os.makedirs(audio)
        # 2 phrases with a known gap: 0.05-1.00 s and 1.60-2.50 s.
        voice = "if(between(t,0.05,1.0)+between(t,1.6,2.5),0.4*sin(2*PI*220*t),0)"
        _tone(os.path.join(audio, "vo-s01.wav"), voice, 2.55)
        timeline = {"meta": {"duration_sec": 6.0}, "scenes": [
            {"id": "s01", "start_sec": 1.0, "duration_sec": 3.0,
             "narration": "Ship every frame. Try it today."},
            {"id": "s02", "start_sec": 4.0, "duration_sec": 2.0, "narration": ""}]}
        write_json(timeline, os.path.join(tmp, "timeline.json"))
        write_json({"bpm": 120, "beats_sec": [0.0, 0.5], "downbeats_sec": [0.0]},
                   os.path.join(tmp, "beats.json"))
        out = os.path.join(tmp, "sync-sheet.json")
        code = main(["--timeline", os.path.join(tmp, "timeline.json"), "--audio-dir", audio,
                     "--beats", os.path.join(tmp, "beats.json"), "--out", out])
        sheet = _load_json(out) if code == 0 else {}
        segments = sheet.get("vo_segments", [])
        expect("sheet written with 1 segment, estimated", code == 0 and len(segments) == 1
               and sheet["timing"] == "estimated" and sheet["beats_sec"] == [0.0, 0.5], sheet)
        if segments:
            words = segments[0]["words"]
            starts = {w["word"]: w["start_sec"] for w in words}
            want = {"Ship": 1.0 + 0.15 + 0.05, "Try": 1.0 + 0.15 + 1.6}
            ok = all(abs(starts.get(k, -9) - v) <= PHRASE_START_TOLERANCE_SEC
                     for k, v in want.items())
            expect("each phrase start within 0.10 s", ok, f"got {starts}, want {want}")
            expect("2 phrases found", len(segments[0]["phrases"]) == 2, segments[0]["phrases"])
            ordered = all(a["end_sec"] <= b["start_sec"] + 1e-9 for a, b in zip(words, words[1:]))
            expect("words in order", ordered and len(words) == 6, words)
        code = main(["--timeline", os.path.join(tmp, "timeline.json"), "--audio-dir", tmp,
                     "--beats", os.path.join(tmp, "beats.json"), "--out", out])
        expect("missing voice file exits 2", code == 2, f"exit {code}")
    print("self-test PASSED" if not failures else "self-test FAILED:\n  " + "\n  ".join(failures))
    return 0 if not failures else 1


if __name__ == "__main__":
    sys.exit(main())
