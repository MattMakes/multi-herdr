#!/usr/bin/env python3
"""Check a continuous music bed and each planned edit.

Usage:
  check_music.py audio/bed.wav audio/beats.json
                 [--edit-plan audio/music-edit.md]
                 [--out audio/music-continuity.json]
  check_music.py --self-test

Exit codes: 0 every check passes, 1 a check fails, 2 an input or tool fails.
Python standard library and ffmpeg only.
"""

import argparse
import array
import json
import math
import os
import shutil
import statistics
import subprocess
import sys
import tempfile
import wave

RATE = 8000
WINDOW_MS = 50
WINDOW_SAMPLES = RATE * WINDOW_MS // 1000
PHRASE_BARS = 4
BEATS_PER_BAR = 4
MAX_DROP_DB = 12.0
MAX_PHASE_MS = 15.0
EXCERPT_SEC = 6.0
PITCH_NAMES = ("C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B")


class CheckError(Exception):
    """An input or required local tool cannot be used."""


def _run(cmd):
    try:
        return subprocess.run(cmd, capture_output=True, check=False)
    except FileNotFoundError as exc:
        raise CheckError(f"{cmd[0]} is not on PATH") from exc


def decode(path):
    """Decode a bed to mono signed 16-bit PCM at RATE."""
    proc = _run(["ffmpeg", "-hide_banner", "-v", "error", "-i", path,
                 "-ac", "1", "-ar", str(RATE), "-f", "s16le", "-"])
    if proc.returncode != 0:
        raise CheckError(f"ffmpeg cannot decode {path}: {proc.stderr.decode(errors='replace')[-300:]}")
    samples = array.array("h")
    samples.frombytes(proc.stdout[:len(proc.stdout) // 2 * 2])
    if sys.byteorder == "big":
        samples.byteswap()
    if not samples:
        raise CheckError(f"{path} has no audio samples")
    return samples


def load_beats(path):
    try:
        with open(path, encoding="utf-8") as handle:
            data = json.load(handle)
    except (OSError, ValueError) as exc:
        raise CheckError(f"cannot read {path}: {exc}") from exc
    beats = data.get("beats_sec") if isinstance(data, dict) else None
    bpm = data.get("bpm") if isinstance(data, dict) else None
    if (not isinstance(beats, list) or len(beats) < 2
            or not all(isinstance(v, (int, float)) and math.isfinite(v) for v in beats)
            or any(b <= a for a, b in zip(beats, beats[1:]))):
        raise CheckError("beat grid needs at least 2 strictly increasing beats_sec values")
    if not isinstance(bpm, (int, float)) or bpm <= 0:
        raise CheckError("beat grid needs bpm > 0")
    return data


def _db(samples):
    if not samples:
        return -math.inf
    power = sum(v * v for v in samples) / len(samples)
    return 10.0 * math.log10(power / (32768.0 * 32768.0)) if power else -math.inf


def level_check(samples, beats):
    """Check 50 ms levels against a centred 4-bar neighbourhood."""
    levels = [_db(samples[i:i + WINDOW_SAMPLES])
              for i in range(0, len(samples) - WINDOW_SAMPLES + 1, WINDOW_SAMPLES)]
    beat_sec = statistics.median(b - a for a, b in zip(beats, beats[1:]))
    phrase_windows = max(1, round(PHRASE_BARS * BEATS_PER_BAR * beat_sec * 1000 / WINDOW_MS))
    radius = max(1, phrase_windows // 2)
    digital_silence = any(value == -math.inf for value in levels)
    worst = []
    failed = digital_silence
    for index, value in enumerate(levels):
        local = [v for v in levels[max(0, index - radius):min(len(levels), index + radius + 1)]
                 if v != -math.inf]
        median = statistics.median(local) if local else -math.inf
        drop = math.inf if value == -math.inf and median != -math.inf else median - value
        if drop > MAX_DROP_DB:
            failed = True
            worst.append({"sec": round((index + 0.5) * WINDOW_MS / 1000, 3),
                          "drop_db": None if math.isinf(drop) else round(drop, 2)})
    worst.sort(key=lambda item: math.inf if item["drop_db"] is None else item["drop_db"], reverse=True)
    return digital_silence, worst[:20], not failed


def read_edits(path):
    """Read edit rows from the Markdown table in audio/music-edit.md."""
    if not path:
        return []
    try:
        lines = open(path, encoding="utf-8").read().splitlines()
    except OSError as exc:
        raise CheckError(f"cannot read {path}: {exc}") from exc
    rows = [[cell.strip() for cell in line.strip().strip("|").split("|")]
            for line in lines if line.strip().startswith("|")]
    if not rows:
        raise CheckError(f"{path} has no Markdown table")
    headers = [value.lower().replace("_", " ") for value in rows[0]]
    required = ("film in", "crossfade ms")
    if any(name not in headers for name in required):
        raise CheckError(f"{path} needs film in and crossfade ms columns")
    edits = []
    for row in rows[2:]:
        if len(row) != len(headers):
            continue
        values = dict(zip(headers, row))
        try:
            film_sec = float(values["film in"].rstrip("s "))
            crossfade_ms = float(values["crossfade ms"].rstrip("ms "))
        except ValueError as exc:
            raise CheckError(f"invalid edit time in {path}: {row}") from exc
        if film_sec > 0:
            edits.append({"film_sec": film_sec, "crossfade_ms": crossfade_ms,
                          "edit_type": values.get("edit type", "")})
    return edits


def _local_tempo(beats, sec, left):
    pairs = [(a, b) for a, b in zip(beats, beats[1:]) if (b <= sec if left else a >= sec)]
    pairs = pairs[-4:] if left else pairs[:4]
    if not pairs:
        pairs = list(zip(beats, beats[1:]))
    return round(60.0 / statistics.median(b - a for a, b in pairs), 2)


def _chroma(samples):
    """Return the strongest pitch class from 4 octaves of Goertzel energy."""
    if not samples or max(map(abs, samples), default=0) == 0:
        return None
    energies = []
    for pitch in range(12):
        total = 0.0
        for octave in range(3, 7):
            midi = 12 * (octave + 1) + pitch
            freq = 440.0 * 2 ** ((midi - 69) / 12)
            coeff = 2.0 * math.cos(2.0 * math.pi * freq / RATE)
            q1 = q2 = 0.0
            for value in samples:
                q0 = value + coeff * q1 - q2
                q2, q1 = q1, q0
            total += q1 * q1 + q2 * q2 - coeff * q1 * q2
        energies.append(total)
    return max(range(12), key=energies.__getitem__)


def _key(samples, sec, left):
    edge = max(0, min(len(samples), round(sec * RATE)))
    span = RATE
    segment = samples[max(0, edge - span):edge] if left else samples[edge:min(len(samples), edge + span)]
    pitch = _chroma(segment)
    return pitch, (PITCH_NAMES[pitch] if pitch is not None else "unpitched")


def _flux_ratio(samples, sec):
    """Return join spectral flux divided by nearby median spectral flux."""
    frame = max(1, RATE // 100)
    center = round(sec * RATE)
    lo, hi = max(0, center - RATE), min(len(samples), center + RATE)
    frequencies = (80, 160, 320, 640, 1280, 2000, 3000, 3800)

    def spectrum(block):
        values = []
        for freq in frequencies:
            coeff = 2.0 * math.cos(2.0 * math.pi * freq / RATE)
            q1 = q2 = 0.0
            for value in block:
                q0 = value + coeff * q1 - q2
                q2, q1 = q1, q0
            values.append(max(0.0, q1 * q1 + q2 * q2 - coeff * q1 * q2))
        total = sum(values)
        return [value / total for value in values] if total else [0.0] * len(values)

    spectra = [spectrum(samples[i:i + frame]) for i in range(lo, hi - frame + 1, frame)]
    flux = [math.sqrt(sum(max(0.0, right - left) ** 2
                          for left, right in zip(a, b)))
            for a, b in zip(spectra, spectra[1:])]
    if not flux:
        return 0.0
    join = min(len(flux) - 1, max(0, (center - lo) // frame - 1))
    nearby = [v for i, v in enumerate(flux) if abs(i - join) > 1 and v > 0]
    baseline = statistics.median(nearby) if nearby else 0.0
    return round(flux[join] / baseline, 2) if baseline else (999.0 if flux[join] else 0.0)


def _write_excerpt(bed, sec, number, folder):
    os.makedirs(folder, exist_ok=True)
    label = f"{sec:.3f}".rstrip("0").rstrip(".")
    path = os.path.join(folder, f"edit-{number}-{label}.wav")
    start = max(0.0, sec - EXCERPT_SEC / 2)
    proc = _run(["ffmpeg", "-hide_banner", "-v", "error", "-y", "-ss", str(start),
                 "-i", bed, "-t", str(EXCERPT_SEC), "-c:a", "pcm_s16le", path])
    if proc.returncode != 0:
        raise CheckError(f"cannot write excerpt {path}: {proc.stderr.decode(errors='replace')[-300:]}")
    print(path)


def check_edits(bed, samples, grid, edits, excerpt_dir):
    beats = grid["beats_sec"]
    bar_ms = BEATS_PER_BAR * 60000.0 / grid["bpm"]
    results = []
    loop_joins = 0
    for number, edit in enumerate(edits, 1):
        sec = edit["film_sec"]
        phase_ms = min(abs(sec - beat) for beat in beats) * 1000.0
        left_pitch, left_key = _key(samples, sec, True)
        right_pitch, right_key = _key(samples, sec, False)
        distance = None if left_pitch is None or right_pitch is None else (right_pitch - left_pitch) % 12
        key_clash = distance in (1, 6, 11)
        short = edit["crossfade_ms"] < bar_ms
        passed = phase_ms <= MAX_PHASE_MS and not (short and key_clash)
        if short and "loop" in edit["edit_type"].lower():
            loop_joins += 1
        results.append({"film_sec": sec,
                        "tempo_left": _local_tempo(beats, sec, True),
                        "tempo_right": _local_tempo(beats, sec, False),
                        "phase_ms": round(phase_ms, 2),
                        "key_left": left_key, "key_right": right_key,
                        "key_clash": key_clash, "flux_ratio": _flux_ratio(samples, sec),
                        "crossfade_ms": edit["crossfade_ms"], "pass": passed})
        _write_excerpt(bed, sec, number, excerpt_dir)
    return results, loop_joins


def check(bed, beat_path, edit_path=None, excerpt_dir=None):
    grid = load_beats(beat_path)
    samples = decode(bed)
    digital_silence, worst, level_pass = level_check(samples, grid["beats_sec"])
    edits = read_edits(edit_path)
    if excerpt_dir is None:
        excerpt_dir = os.path.join(os.path.dirname(os.path.abspath(bed)), "edit-excerpts")
    edit_results, loop_joins = check_edits(bed, samples, grid, edits, excerpt_dir)
    return {"window_ms": WINDOW_MS, "phrase_bars": PHRASE_BARS,
            "max_drop_db": int(MAX_DROP_DB), "digital_silence": digital_silence,
            "worst": worst, "edits": edit_results, "loop_joins": loop_joins,
            "pass": level_pass and all(item["pass"] for item in edit_results)}


def write_report(report, path):
    parent = os.path.dirname(path)
    if parent:
        os.makedirs(parent, exist_ok=True)
    with open(path, "w", encoding="utf-8") as handle:
        json.dump(report, handle, indent=2, allow_nan=False)
        handle.write("\n")


def _tone(path, left_hz, right_hz=None, join=4.0, dip=None, gap=None):
    rate, seconds = 48000, 8
    with wave.open(path, "wb") as wav:
        wav.setparams((1, 2, rate, seconds * rate, "NONE", "not compressed"))
        values = array.array("h")
        for index in range(seconds * rate):
            sec = index / rate
            hz = left_hz if right_hz is None or sec < join else right_hz
            gain = 0.25
            if dip and dip[0] <= sec < dip[1]:
                gain *= 10 ** (-dip[2] / 20)
            if gap and gap[0] <= sec < gap[1]:
                gain = 0.0
            values.append(round(32767 * gain * math.sin(2 * math.pi * hz * sec)))
        wav.writeframes(values.tobytes())


def _edit_plan(path, film_sec, crossfade=40):
    with open(path, "w", encoding="utf-8") as handle:
        handle.write("| source | section | source in | source out | film in | film out | edit type | crossfade ms |\n")
        handle.write("| --- | --- | --- | --- | --- | --- | --- | --- |\n")
        handle.write(f"| a | verse | 0 | 4 | {film_sec} | 8 | splice | {crossfade} |\n")


def self_test():
    failures = []
    if not shutil.which("ffmpeg"):
        print("FAIL ffmpeg must be on PATH")
        return 1
    with tempfile.TemporaryDirectory(prefix="check-music-") as tmp:
        beats = os.path.join(tmp, "beats.json")
        with open(beats, "w", encoding="utf-8") as handle:
            json.dump({"bpm": 120, "beats_sec": [index * 0.5 for index in range(17)],
                       "downbeats_sec": [0, 2, 4, 6, 8]}, handle)

        def expect(name, report, passed):
            ok = report["pass"] is passed
            print(f"{'ok  ' if ok else 'FAIL'} {name}")
            if not ok:
                failures.append(name)

        continuous = os.path.join(tmp, "continuous.wav")
        _tone(continuous, 293.66)
        expect("continuous bed passes", check(continuous, beats), True)
        gap = os.path.join(tmp, "gap.wav")
        _tone(gap, 293.66, gap=(3.0, 3.5))
        expect("bed with a gap fails", check(gap, beats), False)
        breakdown = os.path.join(tmp, "breakdown.wav")
        _tone(breakdown, 293.66, dip=(3.0, 4.0, 10.0))
        expect("10 dB breakdown passes", check(breakdown, beats), True)

        cases = (("half-step splice fails", 349.23, 329.63, 4.0, False),
                 ("compatible splice passes", 293.66, 329.63, 4.0, True),
                 ("30 ms phase offset fails", 293.66, 329.63, 4.03, False))
        for number, (name, left, right, join, passed) in enumerate(cases, 1):
            bed = os.path.join(tmp, f"edit-{number}.wav")
            plan = os.path.join(tmp, f"edit-{number}.md")
            _tone(bed, left, right, join)
            _edit_plan(plan, join)
            report = check(bed, beats, plan, os.path.join(tmp, f"excerpts-{number}"))
            expect(name, report, passed)
            if len(report["edits"]) != 1:
                failures.append(f"{name}: missing edit result")

        out = os.path.join(tmp, "audio", "music-continuity.json")
        report = check(continuous, beats)
        write_report(report, out)
        ok = json.load(open(out, encoding="utf-8"))["pass"] is True
        print(f"{'ok  ' if ok else 'FAIL'} writes music-continuity.json")
        if not ok:
            failures.append("write report")
    print("self-test PASSED" if not failures else "self-test FAILED: " + "; ".join(failures))
    return 0 if not failures else 1


def main(argv=None):
    parser = argparse.ArgumentParser(description="Check music-bed continuity and planned edits.")
    parser.add_argument("bed", nargs="?", help="continuous music bed WAV")
    parser.add_argument("beats", nargs="?", help="audio/beats.json")
    parser.add_argument("--edit-plan", default="audio/music-edit.md")
    parser.add_argument("--out", default="audio/music-continuity.json")
    parser.add_argument("--excerpt-dir", default="audio/edit-excerpts")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args(argv)
    if args.self_test:
        return self_test()
    if not args.bed or not args.beats:
        parser.error("give the music bed WAV and beat grid, or use --self-test")
    edit_plan = args.edit_plan if os.path.isfile(args.edit_plan) else None
    try:
        report = check(args.bed, args.beats, edit_plan, args.excerpt_dir)
        write_report(report, args.out)
    except (CheckError, OSError) as exc:
        print(json.dumps({"pass": False, "error": str(exc)}))
        return 2
    print(json.dumps(report, indent=2, allow_nan=False))
    return 0 if report["pass"] else 1


if __name__ == "__main__":
    sys.exit(main())
