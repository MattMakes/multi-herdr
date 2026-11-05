#!/usr/bin/env python3
"""Reconcile timeline.json to the spoken length of each voice-over file.

Usage:
  reconcile_timing.py timeline.json [--audio-dir audio]
  reconcile_timing.py --self-test

For each scene with narration, ffprobe measures audio/vo-<id>.wav. The rule:

  duration_sec = max(planned duration_sec, vo_duration_sec + 0.25)

Then every scene start_sec and meta.duration_sec are recomputed, and every
other project time moves with the scenes:
  rhythm[].start_sec, rhythm[].end_sec, rhythm[].events_sec,
  meta.hero_sec, meta.payoff_silence_sec, meta.allowed_silences.

A time inside a scene that grew scales with that scene: it keeps its fraction
of the scene. The voice now runs slower than planned, so an event planned at
a word stays near that word, and the gaps between events grow by the same
factor, not all in the scene tail. A time in a scene that did not change keeps
its offset from the scene start. A time at or after the old film end moves by
the total change. Times relative to a scene start (motion.reveal_sec) do not
change. beat_in and beat_out do not change.

The file is written in place. Stdout is a JSON report with each changed scene.
Run check_timeline.py of video-storyboard after it.

Exit codes: 0 written, 2 usage, input or tool error.
Needs ffprobe on PATH. Python standard library only.
"""

import argparse
import json
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
    def move(t):
        if t <= old_edges[0]:
            return t
        for (a, b), (na, nb) in zip(zip(old_edges, old_edges[1:]), zip(new_edges, new_edges[1:])):
            if t < b:
                return round(na + (t - a) * (nb - na) / (b - a), 3)
        return round(t + new_edges[-1] - old_edges[-1], 3)
    return move


def reconcile(timeline, vo_durations):
    """Apply the timing rule in place. vo_durations maps a scene id to seconds.

    Return the changed scenes as [{"scene", "old_duration_sec", "new_duration_sec"}].
    """
    scenes = timeline["scenes"]
    old_edges = [scene["start_sec"] for scene in scenes]
    old_edges.append(scenes[-1]["start_sec"] + scenes[-1]["duration_sec"])
    changes = []
    start = 0.0
    new_edges = []
    for scene in scenes:
        if scene["id"] in vo_durations:
            vo = round(vo_durations[scene["id"]], 3)
            scene["vo_duration_sec"] = vo
            planned = scene["duration_sec"]
            scene["duration_sec"] = round(max(planned, vo + BREATH_PAD_SEC), 3)
            if scene["duration_sec"] != planned:
                changes.append({"scene": scene["id"], "old_duration_sec": planned,
                                "new_duration_sec": scene["duration_sec"]})
        scene["start_sec"] = round(start, 3)
        new_edges.append(scene["start_sec"])
        start += scene["duration_sec"]
    new_edges.append(round(start, 3))
    meta = timeline["meta"]
    meta["duration_sec"] = round(start, 3)

    move = time_map(old_edges, new_edges)
    for item in timeline.get("rhythm", []):
        item["start_sec"] = move(item["start_sec"])
        item["end_sec"] = move(item["end_sec"])
        if "events_sec" in item:
            item["events_sec"] = [move(t) for t in item["events_sec"]]
    for key in ("hero_sec", "payoff_silence_sec"):
        if key in meta:
            meta[key] = [move(t) for t in meta[key]]
    if "allowed_silences" in meta:
        meta["allowed_silences"] = [[move(a), move(b)] for a, b in meta["allowed_silences"]]
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

def _music_timeline():
    """A plan that passes check_timeline.py: bpm 120 (2 bars = 4.0 s), 4 scenes, 15.0 s."""
    def scene(sid, kind, start, duration, template, focal, caption, narration):
        return {"id": sid, "kind": kind, "start_sec": start, "duration_sec": duration,
                "beat_in": int(start * 2), "beat_out": int((start + duration) * 2),
                "narration": narration, "caption": caption, "plate": f"scenes/{sid}/index.html",
                "motion": {"template": template}, "transition_out": "cut",
                "critical_regions": [[0.1, 0.2, 0.6, 0.4]], "focal_point": focal}
    return {
        "meta": {"id": "demo", "title": "Demo", "aspect": "16:9", "fps": 60, "width": 1920,
                 "height": 1080, "duration_sec": 15.0, "lufs_target": -14, "bpm": 120,
                 "hero_sec": [10.0, 11.5], "payoff_silence_sec": [9.5, 10.0],
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
    expect("hero, silence and allowed silences move by 2.0 s",
           new["meta"]["hero_sec"] == [12.0, 13.5] and new["meta"]["payoff_silence_sec"] == [11.5, 12.0]
           and new["meta"]["allowed_silences"] == [[11.5, 12.0]], new["meta"])
    expect("an event inside the grown scene scales (5.0 s -> 6.0 s)",
           new["rhythm"][1] == {"start_sec": 2.5, "end_sec": 9.5, "energy": 5, "events_sec": [6.0]},
           new["rhythm"][1])
    expect("the stretch before the change holds, the last one ends at the film end",
           new["rhythm"][0]["end_sec"] == 2.5 and new["rhythm"][-1]["end_sec"] == 17.0, new["rhythm"])
    expect("vo_duration_sec is recorded", new["scenes"][1]["vo_duration_sec"] == 6.75, new["scenes"][1])

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
