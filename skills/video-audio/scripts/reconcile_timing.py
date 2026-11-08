#!/usr/bin/env python3
"""Reconcile timeline.json to the spoken length of each voice-over file.

Usage:
  reconcile_timing.py timeline.json [--audio-dir audio]
  reconcile_timing.py --self-test

For each scene with narration, ffprobe measures audio/vo-<id>.wav. The rule:

  duration_sec = max(planned duration_sec, vo_duration_sec + 0.25)

With meta.bpm, the end of a scene that grew moves up to the next beat of the
grid k * 60 / bpm, so every cut stays on the beat grid of step 3. Then every
scene beat_in and beat_out is recomputed from its edges. Without meta.bpm
there is no grid: a scene that grew is not snapped, and the beats stay.

Then every scene start_sec and meta.duration_sec are recomputed, and every
other project time moves with the scenes:
  rhythm[].start_sec, rhythm[].end_sec, rhythm[].events_sec,
  meta.hero_sec, meta.payoff_breakdown_sec, meta.allowed_silences.

A time inside a scene that grew scales with that scene: it keeps its fraction
of the scene. The voice now runs slower than planned, so an event planned at
a word stays near that word, and the gaps between events grow by the same
factor, not all in the scene tail. A time in a scene that did not change keeps
its offset from the scene start. A time at or after the old film end moves by
the total change. Times relative to a scene start (motion.reveal_sec) do not
change.

The file is written in place. Stdout is a JSON report with each changed scene.
Run check_timeline.py of video-storyboard after it.

Exit codes: 0 written, 2 usage, input or tool error.
Needs ffprobe on PATH. Python standard library only.
"""

import argparse
import functools
import json
import math
import os
import subprocess
import sys

BREATH_PAD_SEC = 0.25


class ToolError(Exception):
    """ffprobe or an input file cannot be used."""


def probe_duration(path):
    try:
        proc = subprocess.run(["ffprobe", "-v", "error", "-show_entries", "format=duration",
                               "-of", "csv=p=0", path], capture_output=True, text=True, check=False)
    except FileNotFoundError as exc:
        raise ToolError("ffprobe is not on PATH") from exc
    if proc.returncode != 0:
        raise ToolError(f"ffprobe cannot read {path}: {proc.stderr.strip()}")
    return float(proc.stdout)


def time_map(old_edges, new_edges):
    """Return a function that moves a project time from the old scene edges to the new ones."""
    spans = list(zip(zip(old_edges, old_edges[1:]), zip(new_edges, new_edges[1:])))
    return functools.partial(_move_time, old_edges[0], spans, new_edges[-1] - old_edges[-1])


def _scale_into(t, span):
    """Scale time t from an old scene span to its new span."""
    (a, b), (na, nb) = span
    return round(na + (t - a) * (nb - na) / (b - a), 3)


def _move_time(first, spans, shift, t):
    """Keep t before the first edge, scale it inside a scene, shift it after the film end."""
    inside = [span for span in spans if t < span[0][1]]
    moved = _scale_into(t, inside[0]) if inside else round(t + shift, 3)
    return t if t <= first else moved


RANGE_KEYS = ("hero_sec", "payoff_breakdown_sec")


def _scene_edges(scenes):
    """Return every scene start and the end of the last scene."""
    last = scenes[-1]
    return [scene["start_sec"] for scene in scenes] + [last["start_sec"] + last["duration_sec"]]


def _record_voice(scene, vo_durations):
    """Record vo_duration_sec on a narrated scene. Return it rounded, or None."""
    vo = vo_durations.get(scene["id"])
    narrated = vo is not None
    if narrated:
        scene["vo_duration_sec"] = round(vo, 3)
    return scene["vo_duration_sec"] if narrated else None


def _grown_end(start, vo, planned, beat):
    """Return the end of a scene that grows to its voice plus the pad, snapped up to a beat.

    Return None when the voice plus the pad fits in the planned duration.
    """
    grows = vo is not None and round(vo + BREATH_PAD_SEC, 3) > planned
    end = start + (vo or 0.0) + BREATH_PAD_SEC
    snapped = math.ceil(end / beat - 1e-6) * beat if beat else end
    return snapped if grows else None


def _grow(scene, start, vo, beat):
    """Grow the scene when its voice plus the pad is longer than planned. Return its end."""
    planned = scene["duration_sec"]
    end = _grown_end(start, vo, planned, beat)
    scene["duration_sec"] = planned if end is None else round(end - start, 3)
    return start + planned if end is None else end


def _change_list(scene, planned):
    """Return [the change entry] when the scene grew, else []."""
    grew = scene["duration_sec"] != planned
    change = {"scene": scene["id"], "old_duration_sec": planned,
              "new_duration_sec": scene["duration_sec"]}
    return [change] if grew else []


def _fit_scene(scene, start, plan):
    """Set the scene start, record its voice and grow it to fit. Return its end."""
    scene["start_sec"] = round(start, 3)
    planned = scene["duration_sec"]
    end = _grow(scene, start, _record_voice(scene, plan["vo"]), plan["beat"])
    plan["changes"].extend(_change_list(scene, planned))
    return end


def _lay_out(scenes, vo_durations, beat):
    """Lay the scenes end to end from 0 s. Return (new_edges, changes)."""
    edges, changes = [0.0], []
    plan = {"vo": vo_durations, "beat": beat, "changes": changes}
    for scene in scenes:
        edges.append(_fit_scene(scene, edges[-1], plan))
    return [round(edge, 3) for edge in edges], changes


def _beat_marks(scene, beat):
    """Return beat_in and beat_out of the scene on the beat grid."""
    return {"beat_in": round(scene["start_sec"] / beat),
            "beat_out": round((scene["start_sec"] + scene["duration_sec"]) / beat)}


def _set_beats(scenes, beat):
    """With a beat grid, recompute beat_in and beat_out of every scene from its edges."""
    targets = scenes if beat else []
    marks = [_beat_marks(scene, beat) for scene in targets]
    for scene, mark in zip(targets, marks):
        scene.update(mark)


def _move_rhythm(item, move):
    """Move 1 rhythm stretch and its events with the scenes."""
    item["start_sec"] = move(item["start_sec"])
    item["end_sec"] = move(item["end_sec"])
    item.update({"events_sec": [move(t) for t in item["events_sec"]]} if "events_sec" in item else {})


def _move_times(timeline, move):
    """Move rhythm, the meta ranges and meta.allowed_silences with the scenes."""
    meta = timeline["meta"]
    for item in timeline.get("rhythm", []):
        _move_rhythm(item, move)
    meta.update({key: [move(t) for t in meta[key]] for key in RANGE_KEYS if key in meta})
    silences = meta.get("allowed_silences")
    moved = [[move(a), move(b)] for a, b in silences or []]
    meta.update({"allowed_silences": moved} if silences is not None else {})


def reconcile(timeline, vo_durations):
    """Apply the timing rule in place. vo_durations maps a scene id to seconds.

    Return the changed scenes as [{"scene", "old_duration_sec", "new_duration_sec"}].
    """
    old_edges = _scene_edges(timeline["scenes"])
    beat = 60.0 / timeline["meta"]["bpm"] if "bpm" in timeline["meta"] else None
    new_edges, changes = _lay_out(timeline["scenes"], vo_durations, beat)
    timeline["meta"]["duration_sec"] = new_edges[-1]
    _set_beats(timeline["scenes"], beat)
    _move_times(timeline, time_map(old_edges, new_edges))
    return changes


def measure(timeline, audio_dir):
    durations = {}
    for scene in timeline["scenes"]:
        if scene.get("narration"):
            path = os.path.join(audio_dir, f"vo-{scene['id']}.wav")
            if not os.path.isfile(path):
                raise ToolError(f"missing voice file {path}")
            durations[scene["id"]] = probe_duration(path)
    return durations


def main(argv=None):
    parser = argparse.ArgumentParser(description="Reconcile timeline.json to the voice-over lengths.")
    parser.add_argument("timeline", nargs="?", help="timeline.json, written in place")
    parser.add_argument("--audio-dir", help="folder with vo-<scene-id>.wav (default: audio/ next to the timeline)")
    parser.add_argument("--self-test", action="store_true", help="run the built-in tests")
    args = parser.parse_args(argv)
    if args.self_test:
        return self_test()
    if not args.timeline:
        parser.error("give timeline.json or --self-test")
    audio_dir = args.audio_dir or os.path.join(os.path.dirname(os.path.abspath(args.timeline)), "audio")
    try:
        with open(args.timeline, encoding="utf-8") as handle:
            timeline = json.load(handle)
        changes = reconcile(timeline, measure(timeline, audio_dir))
    except (OSError, ToolError, KeyError, TypeError, ValueError, IndexError) as exc:
        print(json.dumps({"written": False, "error": str(exc)}))
        return 2
    with open(args.timeline, "w", encoding="utf-8") as handle:
        json.dump(timeline, handle, indent=2)
        handle.write("\n")
    print(json.dumps({"written": args.timeline, "duration_sec": timeline["meta"]["duration_sec"],
                      "changed": changes}))
    return 0


# ---------------------------------------------------------------- self-test

FIXTURE_KEYS = ("id", "kind", "start_sec", "duration_sec", "template", "focal_point", "caption",
                "narration")


def _fixture_scene(*spec):
    """Return 1 fixture scene from values in FIXTURE_KEYS order."""
    scene = dict(zip(FIXTURE_KEYS, spec))
    end = scene["start_sec"] + scene["duration_sec"]
    scene.update(beat_in=int(scene["start_sec"] * 2), beat_out=int(end * 2),
                 plate=f"scenes/{scene['id']}/index.html", motion={"template": scene.pop("template")},
                 transition_out="cut", critical_regions=[[0.1, 0.2, 0.6, 0.4]])
    return scene


def _music_timeline():
    """A plan that passes check_timeline.py: bpm 120 (2 bars = 4.0 s), 4 scenes, 15.0 s."""
    scene = _fixture_scene
    return {
        "meta": {"id": "demo", "title": "Demo", "aspect": "16:9", "fps": 60, "width": 1920,
                 "height": 1080, "duration_sec": 15.0, "lufs_target": -14, "bpm": 120,
                 "hero_sec": [10.0, 11.5], "payoff_breakdown_sec": [9.5, 10.0],
                 "allowed_silences": [[9.5, 10.0]]},
        "scenes": [
            scene("s01-hook", "hook", 0.0, 2.5, "slam-in", [0.5, 0.45], "Stop flaky tests",
                  "Flaky tests waste hours."),
            scene("s02-demo", "capture", 2.5, 5.0, "zoom-follow", [0.4, 0.5],
                  "One command finds them", "Run one command and see every flaky test."),
            scene("s03-how", "diagram", 7.5, 4.5, "build-up", [0.6, 0.4], "",
                  "It reruns each test and compares results."),
            scene("s04-cta", "cta", 12.0, 3.0, "fade-up", [0.5, 0.5], "Try it today", "Try it today."),
        ],
        "rhythm": [
            {"start_sec": 0.0, "end_sec": 2.5, "energy": 8},
            {"start_sec": 2.5, "end_sec": 7.5, "energy": 5, "events_sec": [5.0]},
            {"start_sec": 7.5, "end_sec": 9.5, "energy": 6},
            {"start_sec": 9.5, "end_sec": 10.0, "energy": 0, "stop": True},
            {"start_sec": 10.0, "end_sec": 12.0, "energy": 10},
            {"start_sec": 12.0, "end_sec": 15.0, "energy": 7},
        ],
    }


def _starts_only(timeline, vo_durations):
    """The earlier procedure: scene starts move, the other project times stay."""
    start = 0.0
    for scene in timeline["scenes"]:
        if scene["id"] in vo_durations:
            scene["duration_sec"] = round(max(scene["duration_sec"],
                                              vo_durations[scene["id"]] + BREATH_PAD_SEC), 3)
        scene["start_sec"] = round(start, 3)
        start += scene["duration_sec"]
    timeline["meta"]["duration_sec"] = round(start, 3)


def _beats(timeline):
    return [(s["beat_in"], s["beat_out"]) for s in timeline["scenes"]]


def self_test():
    import copy
    import shutil
    import tempfile

    failures = []

    def expect(name, ok, detail=""):
        print(f"{'ok  ' if ok else 'FAIL'} {name}")
        if not ok:
            failures.append(f"{name}: {detail}")

    here = os.path.dirname(os.path.abspath(__file__))
    sys.path.insert(0, os.path.join(here, "..", "..", "video-storyboard", "scripts"))
    import check_timeline  # noqa: E402 - the sibling plan checker

    def rules(timeline):
        return {error["rule"] for error in check_timeline.check_timeline(timeline)}

    plan = _music_timeline()
    expect("fixture plan passes check_timeline", rules(plan) == set(), rules(plan))

    # s02-demo has 5.0 s planned; its voice runs 6.75 s, so the scene becomes 7.0 s.
    long_voice = {"s01-hook": 2.0, "s02-demo": 6.75, "s03-how": 3.0, "s04-cta": 1.5}
    old = copy.deepcopy(plan)
    _starts_only(old, long_voice)
    expect("moving only scene starts breaks event_gap", "event_gap" in rules(old), rules(old))

    new = copy.deepcopy(plan)
    changes = reconcile(new, long_voice)
    expect("reconciled plan passes check_timeline", rules(new) == set(), rules(new))
    expect("1 changed scene reported", changes == [
        {"scene": "s02-demo", "old_duration_sec": 5.0, "new_duration_sec": 7.0}], changes)
    expect("scene starts and film length", [s["start_sec"] for s in new["scenes"]]
           == [0.0, 2.5, 9.5, 14.0] and new["meta"]["duration_sec"] == 17.0, new["scenes"])
    expect("hero, breakdown and allowed silences move by 2.0 s",
           new["meta"]["hero_sec"] == [12.0, 13.5] and new["meta"]["payoff_breakdown_sec"] == [11.5, 12.0]
           and new["meta"]["allowed_silences"] == [[11.5, 12.0]], new["meta"])
    expect("an event inside the grown scene scales (5.0 s -> 6.0 s)",
           new["rhythm"][1] == {"start_sec": 2.5, "end_sec": 9.5, "energy": 5, "events_sec": [6.0]},
           new["rhythm"][1])
    expect("the stretch before the change holds, the last one ends at the film end",
           new["rhythm"][0]["end_sec"] == 2.5 and new["rhythm"][-1]["end_sec"] == 17.0, new["rhythm"])
    expect("vo_duration_sec is recorded", new["scenes"][1]["vo_duration_sec"] == 6.75, new["scenes"][1])
    expect("beat_in and beat_out follow the moved edges", _beats(new)
           == [(0, 5), (5, 19), (19, 28), (28, 34)], _beats(new))

    # s02-demo needs 6.6 + 0.25 = 6.85 s. Its end snaps up to the next beat (0.5 s at bpm 120).
    off_beat = copy.deepcopy(plan)
    reconcile(off_beat, {"s02-demo": 6.6})
    expect("a grown scene ends on the next beat (6.85 s -> 7.0 s)",
           [s["start_sec"] for s in off_beat["scenes"]] == [0.0, 2.5, 9.5, 14.0]
           and off_beat["meta"]["duration_sec"] == 17.0, off_beat["scenes"])
    misses = [s["id"] for s in off_beat["scenes"] if abs(s["start_sec"] * 2 - round(s["start_sec"] * 2))
              > 2 / off_beat["meta"]["fps"]]
    expect("every cut is within 1 frame of the beat grid (the render beat check)", misses == [], misses)
    expect("beat_in and beat_out of the snapped plan", _beats(off_beat)
           == [(0, 5), (5, 19), (19, 28), (28, 34)], _beats(off_beat))
    expect("snapped plan passes check_timeline", rules(off_beat) == set(), rules(off_beat))

    no_music = copy.deepcopy(plan)
    del no_music["meta"]["bpm"]
    reconcile(no_music, {"s02-demo": 6.6})
    expect("without meta.bpm a grown scene is not snapped and beats stay",
           no_music["scenes"][1]["duration_sec"] == 6.85 and _beats(no_music) == _beats(plan),
           no_music["scenes"])

    same = copy.deepcopy(plan)
    expect("short voices change nothing", reconcile(same, {"s01-hook": 1.0}) == []
           and {k: v for k, v in same.items() if k != "scenes"}
           == {k: v for k, v in plan.items() if k != "scenes"}, same)

    move = time_map([0.0, 2.0, 4.0], [0.0, 3.0, 5.0])
    expect("time_map: before, inside, at edge, after", [move(t) for t in (-1.0, 1.0, 2.0, 3.0, 4.0, 6.0)]
           == [-1.0, 1.5, 3.0, 4.0, 5.0, 7.0], [move(t) for t in (-1.0, 1.0, 2.0, 3.0, 4.0, 6.0)])

    if not shutil.which("ffmpeg") or not shutil.which("ffprobe"):
        print("FAIL ffmpeg and ffprobe must be on PATH")
        return 1
    with tempfile.TemporaryDirectory(prefix="reconcile-test-") as tmp:
        audio = os.path.join(tmp, "audio")
        os.makedirs(audio)
        for sid, seconds in long_voice.items():
            subprocess.run(["ffmpeg", "-hide_banner", "-v", "error", "-y", "-f", "lavfi", "-i",
                            f"sine=f=220:sample_rate=48000:d={seconds}", "-c:a", "pcm_s16le",
                            os.path.join(audio, f"vo-{sid}.wav")], check=True)
        path = os.path.join(tmp, "timeline.json")
        with open(path, "w", encoding="utf-8") as handle:
            json.dump(plan, handle)
        code = main([path])
        with open(path, encoding="utf-8") as handle:
            written = json.load(handle)
        expect("CLI measures the voices and writes the file", code == 0
               and written["meta"]["duration_sec"] == 17.0 and rules(written) == set(),
               f"exit {code}, {written['meta']}")
        os.remove(os.path.join(audio, "vo-s04-cta.wav"))
        code = main([path])
        expect("missing voice file exits 2", code == 2, f"exit {code}")
    print("self-test PASSED" if not failures else "self-test FAILED:\n  " + "\n  ".join(failures))
    return 0 if not failures else 1


if __name__ == "__main__":
    sys.exit(main())
