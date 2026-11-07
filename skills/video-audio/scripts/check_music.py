#!/usr/bin/env python3
"""Check a continuous music bed and each planned edit.

Usage:
  check_music.py audio/bed.wav audio/beats.json
                 [--edit-plan audio/music-edit.md]
                 [--out audio/music-continuity.json]
  check_music.py path/to/song.wav --write-grid <series-root>/music/grids/<song-id>.json
  check_music.py --self-test

Exit codes: 0 every check passes, 1 a check fails, 2 an input or tool fails.
Python standard library and ffmpeg only.
"""

import argparse
import array
import hashlib
import json
import math
import os
import pathlib
import shutil
import statistics
import subprocess
import sys
import tempfile
import wave

from beat_grid import BeatError, measure as measure_beats

RATE = 8000
WINDOW_MS = 50
WINDOW_SAMPLES = RATE * WINDOW_MS // 1000
PHRASE_BARS = 4
BEATS_PER_BAR = 4
MAX_DROP_DB = 12.0
MAX_PHASE_MS = 15.0
EXCERPT_SEC = 6.0
PITCH_NAMES = ("C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B")
CLASH_STEPS = (1, 6, 11)
FLUX_HZ = (80, 160, 320, 640, 1280, 2000, 3000, 3800)
FLUX_FRAME = RATE // 100
PCM_FORMAT = "s16le" if sys.byteorder == "little" else "s16be"
GRID_MEASURER = "check_music.py 1"


class CheckError(Exception):
    """An input or required local tool cannot be used."""


# ---------------------------------------------------------------- input

def _ffmpeg(args):
    """Run ffmpeg with args and return its stdout bytes."""
    if not shutil.which("ffmpeg"):
        raise CheckError("ffmpeg is not on PATH")
    proc = subprocess.run(["ffmpeg", "-hide_banner", "-v", "error", *args],
                          capture_output=True, check=False)
    tail = proc.stderr.decode(errors="replace")[-300:]
    if proc.returncode != 0:
        raise CheckError(f"ffmpeg fails: {tail}")
    return proc.stdout


def decode(path):
    """Decode a bed to mono signed 16-bit PCM at RATE."""
    raw = _ffmpeg(["-i", path, "-ac", "1", "-ar", str(RATE), "-f", PCM_FORMAT, "-"])
    samples = array.array("h", raw[:len(raw) // 2 * 2])
    if not samples:
        raise CheckError(f"{path} has no audio samples")
    return samples


def _is_num(value):
    """Return True for a finite int or float that is not a bool."""
    return isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value)


def _increasing(values):
    """Return True for a list of at least 2 strictly increasing numbers."""
    numbers = isinstance(values, list) and len(values) >= 2 and all(map(_is_num, values))
    return numbers and all(b > a for a, b in zip(values, values[1:]))


def load_beats(path):
    """Read the beat grid and check beats_sec and bpm."""
    data = json.loads(pathlib.Path(path).read_text(encoding="utf-8"))
    grid = data if isinstance(data, dict) else {}
    if not _increasing(grid.get("beats_sec")):
        raise CheckError("beat grid needs at least 2 strictly increasing beats_sec values")
    if not (_is_num(grid.get("bpm")) and grid["bpm"] > 0):
        raise CheckError("beat grid needs bpm > 0")
    return grid


# ---------------------------------------------------------------- level continuity

def _db(samples):
    """Return the RMS level of samples in dBFS."""
    power = sum(v * v for v in samples) / len(samples) if samples else 0
    return 10.0 * math.log10(power / (32768.0 * 32768.0)) if power else -math.inf


def window_levels(samples):
    """Return the RMS level in dBFS of each 50 ms window."""
    starts = range(0, len(samples) - WINDOW_SAMPLES + 1, WINDOW_SAMPLES)
    return [_db(samples[i:i + WINDOW_SAMPLES]) for i in starts]


def _phrase_radius(beats):
    """Return half of a 4-bar phrase, in 50 ms windows."""
    beat_sec = statistics.median(b - a for a, b in zip(beats, beats[1:]))
    windows = round(PHRASE_BARS * BEATS_PER_BAR * beat_sec * 1000 / WINDOW_MS)
    return max(1, windows // 2)


def _drop(levels, index, radius):
    """Return how far window index is under the median of its phrase, in dB."""
    local = [v for v in levels[max(0, index - radius):index + radius + 1] if v != -math.inf]
    if levels[index] == -math.inf:
        return math.inf if local else -math.inf
    return statistics.median(local) - levels[index]


def _drop_entry(index, drop):
    """Return the report entry of 1 window that drops too far."""
    return {"sec": round((index + 0.5) * WINDOW_MS / 1000, 3),
            "drop_db": None if math.isinf(drop) else round(drop, 2)}


def _worst_key(entry):
    """Sort a silent window before every finite drop."""
    return math.inf if entry["drop_db"] is None else entry["drop_db"]


def level_check(samples, beats):
    """Check 50 ms levels against a centred 4-bar neighbourhood."""
    levels = window_levels(samples)
    radius = _phrase_radius(beats)
    drops = ((index, _drop(levels, index, radius)) for index in range(len(levels)))
    worst = [_drop_entry(index, drop) for index, drop in drops if drop > MAX_DROP_DB]
    silence = -math.inf in levels
    return {"digital_silence": silence, "worst": sorted(worst, key=_worst_key, reverse=True)[:20],
            "pass": not (silence or worst)}


# ---------------------------------------------------------------- edit plan

def _table_rows(text):
    """Return the cells of each Markdown table line."""
    lines = (line.strip() for line in text.splitlines())
    return [[cell.strip() for cell in line.strip("|").split("|")] for line in lines if line.startswith("|")]


def _headers(rows, path):
    """Return the normalized header row, and check the required columns."""
    headers = [value.lower().replace("_", " ") for value in rows[0]] if rows else []
    if not {"film in", "crossfade ms"} <= set(headers):
        raise CheckError(f"{path} needs a Markdown table with film in and crossfade ms columns")
    return headers


def _number(text, unit, row):
    """Parse a number with an optional unit suffix."""
    value = text.rstrip(unit + " ")
    if not value.replace(".", "", 1).isdigit():
        raise CheckError(f"invalid edit time {text!r} in row {row}")
    return float(value)


def _edit(values):
    """Return 1 edit from 1 table row."""
    return {"film_sec": _number(values["film in"], "s", values),
            "crossfade_ms": _number(values["crossfade ms"], "ms", values),
            "edit_type": values.get("edit type", "")}


def read_edits(path):
    """Read edit rows from the Markdown table in audio/music-edit.md."""
    rows = _table_rows(pathlib.Path(path).read_text(encoding="utf-8")) if path else []
    headers = _headers(rows, path) if path else []
    edits = [_edit(dict(zip(headers, row))) for row in rows[2:] if len(row) == len(headers)]
    return [edit for edit in edits if edit["film_sec"] > 0]


# ---------------------------------------------------------------- edit measures

def _side_pairs(beats, sec, left):
    """Return up to 4 beat intervals on 1 side of sec, or every interval."""
    pairs = list(zip(beats, beats[1:]))
    before = [pair for pair in pairs if pair[1] <= sec][-4:]
    after = [pair for pair in pairs if pair[0] >= sec][:4]
    return (before if left else after) or pairs


def _local_tempo(beats, sec, left):
    """Return the tempo in BPM on 1 side of sec."""
    intervals = [b - a for a, b in _side_pairs(beats, sec, left)]
    return round(60.0 / statistics.median(intervals), 2)


def _goertzel(block, freq):
    """Return the Goertzel power of block at freq."""
    coeff = 2.0 * math.cos(2.0 * math.pi * freq / RATE)
    q1 = q2 = 0.0
    for value in block:
        q1, q2 = value + coeff * q1 - q2, q1
    return q1 * q1 + q2 * q2 - coeff * q1 * q2


def _pitch_energy(samples, pitch):
    """Return the energy of 1 pitch class over octaves 3 to 6."""
    midis = (12 * (octave + 1) + pitch for octave in range(3, 7))
    return sum(_goertzel(samples, 440.0 * 2 ** ((midi - 69) / 12)) for midi in midis)


def _chroma(samples):
    """Return the strongest pitch class, or None for a silent block."""
    energies = [_pitch_energy(samples, pitch) for pitch in range(12)] if any(samples) else []
    return max(range(12), key=energies.__getitem__) if energies else None


def _side(samples, sec, left):
    """Return 1 s of samples before (left) or after sec."""
    edge = max(0, min(len(samples), round(sec * RATE)))
    return samples[max(0, edge - RATE):edge] if left else samples[edge:edge + RATE]


def _key(samples, sec, left):
    """Return the pitch class and its name on 1 side of sec."""
    pitch = _chroma(_side(samples, sec, left))
    return pitch, ("unpitched" if pitch is None else PITCH_NAMES[pitch])


def _key_clash(left_pitch, right_pitch):
    """Return True when 2 pitch classes are a half step or a tritone apart."""
    known = left_pitch is not None and right_pitch is not None
    return known and (right_pitch - left_pitch) % 12 in CLASH_STEPS


def _spectrum(block):
    """Return the normalized energy of block in the FLUX_HZ bands."""
    values = [max(0.0, _goertzel(block, freq)) for freq in FLUX_HZ]
    total = sum(values)
    return [value / total for value in values] if total else [0.0] * len(values)


def _flux(before, after):
    """Return the positive spectral flux from 1 spectrum to the next."""
    return math.sqrt(sum(max(0.0, b - a) ** 2 for a, b in zip(before, after)))


def _flux_curve(samples, lo, hi):
    """Return the spectral flux of each 10 ms frame from lo to hi."""
    starts = range(lo, hi - FLUX_FRAME + 1, FLUX_FRAME)
    spectra = [_spectrum(samples[i:i + FLUX_FRAME]) for i in starts]
    return [_flux(a, b) for a, b in zip(spectra, spectra[1:])]


def _ratio(value, baseline):
    """Return value / baseline, 999 for a spike on a flat baseline, else 0."""
    spike = 999.0 if value else 0.0
    return round(value / baseline, 2) if baseline else spike


def _join_ratio(flux, join):
    """Return the flux at join divided by the median of the frames around it."""
    nearby = [v for i, v in enumerate(flux) if abs(i - join) > 1 and v > 0]
    return _ratio(flux[join], statistics.median(nearby) if nearby else 0.0)


def _flux_ratio(samples, sec):
    """Return join spectral flux divided by nearby median spectral flux."""
    center = round(sec * RATE)
    lo = max(0, center - RATE)
    flux = _flux_curve(samples, lo, min(len(samples), center + RATE))
    join = min(len(flux) - 1, max(0, (center - lo) // FLUX_FRAME - 1))
    return _join_ratio(flux, join) if flux else 0.0


# ---------------------------------------------------------------- edit check

def _bar_ms(grid):
    """Return the length of 1 bar in ms."""
    return BEATS_PER_BAR * 60000.0 / grid["bpm"]


def _short(edit, grid):
    """Return True when the crossfade of edit is shorter than 1 bar."""
    return edit["crossfade_ms"] < _bar_ms(grid)


def _timing(grid, sec):
    """Return the tempo on each side of sec and the beat-phase offset."""
    beats = grid["beats_sec"]
    return {"film_sec": sec, "tempo_left": _local_tempo(beats, sec, True),
            "tempo_right": _local_tempo(beats, sec, False),
            "phase_ms": round(min(abs(sec - beat) for beat in beats) * 1000.0, 2)}


def _keys(samples, sec):
    """Return the key on each side of sec and the clash flag."""
    left_pitch, left_key = _key(samples, sec, True)
    right_pitch, right_key = _key(samples, sec, False)
    return {"key_left": left_key, "key_right": right_key,
            "key_clash": _key_clash(left_pitch, right_pitch)}


def _edit_result(samples, grid, edit):
    """Measure 1 edit point and return its report entry."""
    sec = edit["film_sec"]
    result = {**_timing(grid, sec), **_keys(samples, sec)}
    result.update(flux_ratio=_flux_ratio(samples, sec), crossfade_ms=edit["crossfade_ms"])
    clash = _short(edit, grid) and result["key_clash"]
    result["pass"] = result["phase_ms"] <= MAX_PHASE_MS and not clash
    return result


def _is_loop_join(edit, grid):
    """Return True for a short loop join."""
    return _short(edit, grid) and "loop" in edit["edit_type"].lower()


def _excerpt_path(folder, number, sec):
    """Return the excerpt path of edit number at sec."""
    label = f"{sec:.3f}".rstrip("0").rstrip(".")
    return os.path.join(folder, f"edit-{number}-{label}.wav")


def _write_excerpt(bed, sec, number, folder):
    """Write a 6 s excerpt around sec and print its path."""
    os.makedirs(folder, exist_ok=True)
    path = _excerpt_path(folder, number, sec)
    start = max(0.0, sec - EXCERPT_SEC / 2)
    _ffmpeg(["-y", "-ss", str(start), "-i", bed, "-t", str(EXCERPT_SEC), "-c:a", "pcm_s16le", path])
    print(path)


def _write_excerpts(bed, edits, folder):
    """Write the excerpt of each edit and return the count."""
    written = [_write_excerpt(bed, edit["film_sec"], n, folder) for n, edit in enumerate(edits, 1)]
    return len(written)


def check_edits(bed, samples, grid, edits, excerpt_dir):
    """Measure each edit, write its excerpt and count the short loop joins."""
    _write_excerpts(bed, edits, excerpt_dir)
    return {"edits": [_edit_result(samples, grid, edit) for edit in edits],
            "loop_joins": sum(_is_loop_join(edit, grid) for edit in edits)}


def _excerpt_dir(bed, excerpt_dir):
    """Return the excerpt folder: the given one, or edit-excerpts next to the bed."""
    beside = os.path.join(os.path.dirname(os.path.abspath(bed)), "edit-excerpts")
    return excerpt_dir or beside


def _report(levels, joins):
    """Return the audio/music-continuity.json report."""
    passed = levels["pass"] and all(item["pass"] for item in joins["edits"])
    return {"window_ms": WINDOW_MS, "phrase_bars": PHRASE_BARS, "max_drop_db": int(MAX_DROP_DB),
            "digital_silence": levels["digital_silence"], "worst": levels["worst"],
            **joins, "pass": passed}


def check(bed, beat_path, edit_path=None, excerpt_dir=None):
    """Return the continuity report of bed."""
    grid = load_beats(beat_path)
    samples = decode(bed)
    return _report(level_check(samples, grid["beats_sec"]),
                   check_edits(bed, samples, grid, read_edits(edit_path), _excerpt_dir(bed, excerpt_dir)))


def write_report(report, path):
    """Write the report as JSON and make its folder."""
    target = pathlib.Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(report, indent=2, allow_nan=False) + "\n", encoding="utf-8")


def _sha256(path):
    """Return the SHA-256 digest of a file."""
    source = pathlib.Path(path)
    with source.open("rb") as handle:
        digest = hashlib.file_digest(handle, "sha256")
    return digest.hexdigest()


def write_grid(song_path, out_path):
    """Measure song_path and write its series-level grid file."""
    measured = measure_beats(song_path)
    grid = {
        "song": pathlib.Path(out_path).stem,
        "source_sha256": _sha256(song_path),
        "tempo_bpm": measured["tempo_bpm"],
        "bar_sec": measured["bar_sec"],
        "downbeats_sec": measured["downbeats_sec"],
        "measured_by": GRID_MEASURER,
    }
    write_report(grid, out_path)
    return grid


# ---------------------------------------------------------------- self-test

TEST_RATE = 16000
TEST_SECONDS = 8
TEST_HZ = 293.66
LEVEL_CASES = (("continuous bed passes", {}, True),
               ("bed with a gap fails", {"gap": (3.0, 3.5)}, False),
               ("10 dB breakdown passes", {"dip": (3.0, 4.0, 10.0)}, True))
EDIT_CASES = (("half-step splice fails", 349.23, 329.63, 4.0, False),
              ("compatible splice passes", 293.66, 329.63, 4.0, True),
              ("30 ms phase offset fails", 293.66, 329.63, 4.03, False))


def _gain(sec, spec):
    """Return the test tone gain at sec."""
    gap, dip = spec.get("gap"), spec.get("dip")
    in_gap = bool(gap) and gap[0] <= sec < gap[1]
    in_dip = bool(dip) and dip[0] <= sec < dip[1]
    return 0.0 if in_gap else 0.25 * (10 ** (-dip[2] / 20) if in_dip else 1.0)


def _tone_value(sec, spec):
    """Return 1 test tone sample at sec."""
    hz = spec["right_hz"] if sec >= spec["join"] else spec["left_hz"]
    return round(32767 * _gain(sec, spec) * math.sin(2 * math.pi * hz * sec))


def _tone(path, **spec):
    """Write an 8 s mono test tone WAV: left_hz, then right_hz from join."""
    spec = {"right_hz": spec["left_hz"], "join": 4.0, **spec}
    values = array.array("h", (_tone_value(i / TEST_RATE, spec) for i in range(TEST_SECONDS * TEST_RATE)))
    wav = wave.open(path, "wb")
    wav.setparams((1, 2, TEST_RATE, len(values), "NONE", "not compressed"))
    wav.writeframes(values.tobytes())
    wav.close()


def _edit_plan(path, film_sec, crossfade=40):
    """Write a 1-row audio/music-edit.md plan."""
    pathlib.Path(path).write_text(
        "| source | section | source in | source out | film in | film out | edit type | crossfade ms |\n"
        "| --- | --- | --- | --- | --- | --- | --- | --- |\n"
        f"| a | verse | 0 | 4 | {film_sec} | 8 | splice | {crossfade} |\n", encoding="utf-8")


def _write_beats(tmp):
    """Write a 120 BPM beat grid of 8 s and return its path."""
    path = os.path.join(tmp, "beats.json")
    grid = {"bpm": 120, "beats_sec": [i * 0.5 for i in range(17)], "downbeats_sec": [0, 2, 4, 6, 8]}
    pathlib.Path(path).write_text(json.dumps(grid), encoding="utf-8")
    return path


def _click_track(path, bpm=120.0):
    """Write an 8 s click track with its first click at 0 s."""
    beat_samples = round(TEST_RATE * 60.0 / bpm)
    click_samples = TEST_RATE // 100
    values = array.array("h", (24000 if i % beat_samples < click_samples else 0
                                for i in range(TEST_SECONDS * TEST_RATE)))
    with wave.open(path, "wb") as wav:
        wav.setparams((1, 2, TEST_RATE, len(values), "NONE", "not compressed"))
        wav.writeframes(values.tobytes())


def _grid_case(tmp):
    """Write and validate a grid measured from a synthetic click track."""
    song = os.path.join(tmp, "synthetic-click.wav")
    out = os.path.join(tmp, "music", "grids", "synthetic-click.json")
    _click_track(song)
    grid = write_grid(song, out)
    saved = json.loads(pathlib.Path(out).read_text(encoding="utf-8"))
    return (abs(grid["tempo_bpm"] - 120.0) <= 0.1 and saved == grid
            and len(grid["source_sha256"]) == 64 and len(grid["downbeats_sec"]) >= 2)


def _level_case(tmp, beats, case):
    """Run 1 level case and return True when its result is as expected."""
    name, spec, passed = case
    bed = os.path.join(tmp, name.replace(" ", "-") + ".wav")
    _tone(bed, left_hz=TEST_HZ, **spec)
    return check(bed, beats)["pass"] is passed


def _edit_files(tmp, number, case):
    """Write the bed and the edit plan of 1 edit case and return their paths."""
    bed, plan = (os.path.join(tmp, f"edit-{number}.{ext}") for ext in ("wav", "md"))
    _tone(bed, left_hz=case[1], right_hz=case[2], join=case[3])
    _edit_plan(plan, case[3])
    return bed, plan


def _edit_report(tmp, beats, number, case):
    """Check the bed of 1 edit case and return the report."""
    bed, plan = _edit_files(tmp, number, case)
    return check(bed, beats, plan, os.path.join(tmp, f"excerpts-{number}"))


def _edit_case(tmp, beats, number, case):
    """Run 1 edit case and return True when its result is as expected."""
    report = _edit_report(tmp, beats, number, case)
    return report["pass"] is case[4] and len(report["edits"]) == 1


def _report_case(tmp, beats):
    """Write music-continuity.json for a continuous bed and read it back."""
    out = os.path.join(tmp, "audio", "music-continuity.json")
    write_report(check(os.path.join(tmp, "continuous-bed-passes.wav"), beats), out)
    return json.loads(pathlib.Path(out).read_text(encoding="utf-8"))["pass"] is True


def _case_list(tmp, beats):
    """Return the name, the function and the arguments of each case."""
    levels = [(case[0], _level_case, (tmp, beats, case)) for case in LEVEL_CASES]
    edits = [(case[0], _edit_case, (tmp, beats, n, case)) for n, case in enumerate(EDIT_CASES, 1)]
    return levels + edits + [
        ("writes music-continuity.json", _report_case, (tmp, beats)),
        ("writes a measured grid within 0.1 BPM", _grid_case, (tmp,)),
    ]


def _run_case(name, func, args):
    """Run 1 case, print its result and return True when it passes."""
    ok = func(*args)
    print(f"{'ok  ' if ok else 'FAIL'} {name}")
    return ok


def self_test():
    """Run the synthetic cases and return the exit code."""
    _ffmpeg(["-version"])
    tmp = tempfile.mkdtemp(prefix="check-music-")
    cases = _case_list(tmp, _write_beats(tmp))
    failures = [name for name, func, args in cases if not _run_case(name, func, args)]
    shutil.rmtree(tmp, ignore_errors=True)
    print("self-test FAILED: " + "; ".join(failures) if failures else "self-test PASSED")
    return 1 if failures else 0


# ---------------------------------------------------------------- CLI

def _parser():
    """Return the command-line parser."""
    parser = argparse.ArgumentParser(description="Check music-bed continuity and planned edits.")
    parser.add_argument("bed", nargs="?", help="continuous music bed WAV")
    parser.add_argument("beats", nargs="?", help="audio/beats.json")
    parser.add_argument("--edit-plan", default="audio/music-edit.md")
    parser.add_argument("--out", default="audio/music-continuity.json")
    parser.add_argument("--excerpt-dir", default="audio/edit-excerpts")
    parser.add_argument("--write-grid", metavar="OUT_JSON",
                        help="measure BED as a song and write its series grid")
    parser.add_argument("--self-test", action="store_true")
    return parser


def _run_check(args):
    """Check the bed, write the report and return the exit code."""
    edit_plan = args.edit_plan if os.path.isfile(args.edit_plan) else None
    report = check(args.bed, args.beats, edit_plan, args.excerpt_dir)
    write_report(report, args.out)
    print(json.dumps(report, indent=2, allow_nan=False))
    return 0 if report["pass"] else 1


def _dispatch(parser, args):
    """Run the self-test or the check that args select."""
    _require_dispatch_input(parser, args)
    action = _dispatch_action(args)
    return action(args)


def _require_dispatch_input(parser, args):
    """Require the inputs for one check_music mode."""
    missing = not args.self_test and not (args.bed and (args.beats or args.write_grid))
    if missing:
        parser.error("give the music bed WAV and beat grid, use --write-grid, or use --self-test")
    return not missing


def _dispatch_action(args):
    """Return the function for the selected command mode."""
    return _run_self_test if args.self_test else (_run_grid if args.write_grid else _run_check)


def _run_self_test(_args):
    """Run the built-in tests through the command dispatcher."""
    return self_test()


def _run_grid(args):
    """Write a measured grid through the command dispatcher."""
    print(json.dumps(write_grid(args.bed, args.write_grid), indent=2))
    return 0


def _input_error(exc):
    """Print an input error and return exit code 2."""
    print(json.dumps({"pass": False, "error": str(exc)}))
    return 2


def main(argv=None):
    """Parse argv and run. Input and tool errors exit 2 in the __main__ block."""
    parser = _parser()
    return _dispatch(parser, parser.parse_args(argv))


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (BeatError, CheckError, OSError, ValueError) as exc:
        sys.exit(_input_error(exc))
