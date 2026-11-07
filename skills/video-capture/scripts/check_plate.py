#!/usr/bin/env python3
"""Plate gate for 1 capture scene. Python standard library, ffmpeg and ffprobe only.

Usage, from the video project root:
  check_plate.py <scene-id>     print 1 JSON line, exit 0 on pass and 1 on any fail
  check_plate.py --self-test    gate synthetic plates in a temp folder
"""
import json, math, os, re, subprocess, sys, tempfile
from itertools import groupby


def run(*a):
    return subprocess.run(a, capture_output=True, text=True).stdout


def frame_hashes(path):
    text = run("ffmpeg", "-v", "error", "-i", path, "-map", "0:v:0", "-f", "framemd5", "-")
    return [line.rsplit(",", 1)[-1].strip() for line in text.splitlines()
            if line and not line.startswith("#")]


def run_lengths(hashes):
    """Return (start frame, frame count) for each run of identical frames."""
    runs, start = [], 0
    for _, group in groupby(hashes):
        count = len(list(group))
        runs.append((start, count))
        start += count
    return runs


def seconds(frames, fps):
    return round(frames / fps, 6) if fps else 0


def longest_frozen_run(path, fps):
    start, count = max(run_lengths(frame_hashes(path)), key=lambda r: r[1], default=(0, 0))
    return {"start_sec": seconds(start, fps), "length_sec": seconds(count, fps), "frames": count}


def valid_number(value, minimum=0):
    return (isinstance(value, (int, float)) and not isinstance(value, bool)
            and math.isfinite(value) and value >= minimum)


def is_int(value):
    return isinstance(value, int) and not isinstance(value, bool)


def valid_actions(actions):
    return isinstance(actions, dict) and all(
        isinstance(name, str) and name and valid_number(sec) for name, sec in actions.items())


PIXEL_FACT_KEYS = {"subject_width_px", "nearest_distance_px"}


def valid_pixel_facts(facts):
    return (isinstance(facts, dict) and set(facts) == PIXEL_FACT_KEYS
            and all(valid_number(facts[k]) for k in PIXEL_FACT_KEYS))


SIDECAR_RULES = {
    "clip": (lambda v: isinstance(v, str), "clip must be a string"),
    "fps": (lambda v: is_int(v) and v > 0, "fps must be a positive integer"),
    "frame_count": (lambda v: is_int(v) and v >= 0, "frame_count must be a non-negative integer"),
    "seed": (is_int, "seed must be an integer"),
    "build": (lambda v: isinstance(v, str) and bool(v), "build must be a non-empty string"),
    "key_actions_sec": (valid_actions, "key_actions_sec must map names to non-negative seconds"),
    "pixel_facts": (valid_pixel_facts, "pixel_facts must contain non-negative pixel measurements"),
    "max_frozen_sec": (valid_number, "max_frozen_sec must be non-negative seconds"),
}


def sidecar_failures(sidecar, path, fps, frame_count):
    if not isinstance(sidecar, dict):
        return ["G6 sidecar must be a JSON object"]
    fails = []
    if set(sidecar) != set(SIDECAR_RULES):
        fails.append("G6 sidecar fields do not match the capture contract")
    if not set(SIDECAR_RULES).issubset(sidecar):
        return fails
    valid = {key for key, (ok, _) in SIDECAR_RULES.items() if ok(sidecar[key])}
    fails += [f"G6 sidecar {msg}" for key, (_, msg) in SIDECAR_RULES.items() if key not in valid]
    expected = {"clip": path, "fps": fps, "frame_count": frame_count}
    fails += [f"G6 sidecar {key} {sidecar[key]!r}, clip {want!r}"
              for key, want in expected.items() if key in valid and sidecar[key] != want]
    return fails


def gate(sid):
    timeline = json.load(open("timeline.json"))
    scene = next(x for x in timeline["scenes"] if x["id"] == sid)
    meta = timeline["meta"]
    path = scene.get("plate", f"captures/{sid}.mp4")
    sidecar_path = os.path.splitext(path)[0] + ".json"
    is_app_plate = path.startswith("plates/")
    telemetry = None if is_app_plate else json.load(open(f"captures/{sid}.json"))
    probe = json.loads(run("ffprobe", "-v", "error", "-count_frames", "-select_streams", "v:0",
        "-show_entries", "stream=codec_type,width,height,r_frame_rate,nb_read_frames:format=duration",
        "-of", "json", path))
    video = (probe.get("streams") or [{}])[0]
    duration = float(probe.get("format", {}).get("duration", 0))
    rate = video.get("r_frame_rate", "0/1").split("/")
    fps = float(rate[0]) / float(rate[1]) if len(rate) == 2 and float(rate[1]) else 0
    frame_count = int(video.get("nb_read_frames") or 0)
    y = dict(re.findall(r"signalstats\.(YMIN|YMAX)=(\d+)", run("ffmpeg", "-v", "error", "-i", path, "-vf",
        r"select=eq(n\,0),signalstats,metadata=print:file=-", "-frames:v", "1", "-f", "null", "-")))
    plate_size = (video.get("width"), video.get("height"))
    if telemetry is None:
        size_ok = plate_size in {
            (meta["width"], meta["height"]),
            (2 * meta["width"], 2 * meta["height"]),
        }
    else:
        size_ok = (plate_size == (telemetry.get("width"), telemetry.get("height"))
                   and telemetry.get("width") in (meta["width"], 2 * meta["width"]))
    frozen = longest_frozen_run(path, fps)
    fails = [msg for ok, msg in [
        (video.get("codec_type") == "video", "G1 no video stream"),
        (fps == meta["fps"], f"G1 rate {video.get('r_frame_rate')}"),
        (size_ok, f"G1 size {video.get('width')}x{video.get('height')}"),
        (abs(duration - scene["duration_sec"]) <= 0.05,
         f"G2 duration {duration} s, timeline {scene['duration_sec']} s"),
        (int(y.get("YMAX", 0)) - int(y.get("YMIN", 0)) >= 24,
         f"G3 frame 0 is blank: {y}"),
        (frozen["frames"] <= 1,
         f"G5 frozen run starts at {frozen['start_sec']} s and lasts {frozen['length_sec']} s ({frozen['frames']} frames)"),
    ] if not ok]
    if is_app_plate and os.path.exists(sidecar_path):
        try:
            fails.extend(sidecar_failures(json.load(open(sidecar_path)), path, fps, frame_count))
        except (json.JSONDecodeError, OSError) as error:
            fails.append(f"G6 sidecar cannot be read: {error}")
    return duration, fails, frozen


CASES = [
    {"id": "pass-1x", "source": "testsrc2=size=320x180:rate=60", "seconds": 1.0, "width": 320, "want": []},
    {"id": "pass-2x", "source": "testsrc2=size=640x360:rate=60", "seconds": 1.0, "width": 640, "want": []},
    {"id": "blank", "source": "color=c=black:size=320x180:rate=60", "seconds": 1.0, "width": 320, "want": ["G3", "G5"]},
    {"id": "short", "source": "testsrc2=size=320x180:rate=60", "seconds": 0.5, "width": 320, "want": ["G2"]},
    {"id": "rate", "source": "testsrc2=size=320x180:rate=30", "seconds": 1.0, "width": 320, "want": ["G1 rate"]},
    {"id": "size", "source": "testsrc2=size=480x270:rate=60", "seconds": 1.0, "width": 480, "want": ["G1 size"]},
    {"id": "freeze-pass", "source": "testsrc2=size=320x180:rate=60", "seconds": 1.0, "width": 320, "want": []},
    {"id": "freeze-fail", "source": "testsrc2=size=320x180:rate=60,trim=end_frame=1,tpad=stop_mode=clone:stop_duration=1", "seconds": 1.0, "width": 320, "want": ["G5"]},
    {"id": "sidecar-pass", "source": "testsrc2=size=320x180:rate=60", "seconds": 1.0, "width": 320, "plate": True, "want": []},
    {"id": "sidecar-fail", "source": "testsrc2=size=320x180:rate=60", "seconds": 1.0, "width": 320, "plate": True, "bad_sidecar": True, "want": ["G6 sidecar fields", "G6 sidecar fps", "G6 sidecar frame_count"]},
]


def self_test():
    failures = []
    old_cwd = os.getcwd()
    with tempfile.TemporaryDirectory(prefix="check-plate-") as root:
        os.makedirs(os.path.join(root, "captures"))
        os.makedirs(os.path.join(root, "plates"))
        os.chdir(root)
        scenes = []
        for case in CASES:
            scene = {"id": case["id"], "kind": "capture", "duration_sec": 1.0}
            if case.get("plate"):
                scene["plate"] = f"plates/{case['id']}.mp4"
            scenes.append(scene)
        json.dump({"meta": {"fps": 60, "width": 320, "height": 180}, "scenes": scenes},
                  open("timeline.json", "w"))
        for case in CASES:
            sid = case["id"]
            path = f"plates/{sid}.mp4" if case.get("plate") else f"captures/{sid}.mp4"
            subprocess.run(["ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", case["source"],
                            "-t", str(case["seconds"]), "-c:v", "libx264", "-pix_fmt", "yuv420p", path], check=True)
            if case.get("plate"):
                sidecar = {
                    "clip": path, "fps": 60, "frame_count": 60, "seed": 7, "build": "self-test",
                    "key_actions_sec": {"action": 0.5},
                    "pixel_facts": {"subject_width_px": 120, "nearest_distance_px": 40},
                    "max_frozen_sec": 0.017,
                }
                if case.get("bad_sidecar"):
                    sidecar.update({"fps": 30, "frame_count": 59, "unexpected": True})
                json.dump(sidecar, open(f"plates/{sid}.json", "w"))
            else:
                width = case["width"]
                json.dump({"width": width, "height": width * 9 // 16}, open(f"captures/{sid}.json", "w"))
            _, fails, _ = gate(sid)
            want = case["want"]
            unexpected = [f for f in fails if not any(f.startswith(prefix) for prefix in want)]
            missing = [prefix for prefix in want if not any(f.startswith(prefix) for f in fails)]
            if unexpected or missing:
                failures.append(f"{sid}: unexpected {unexpected}, missing {missing}")
        os.chdir(old_cwd)
    print(f"check_plate self-test: {len(CASES) - len(failures)} of {len(CASES)} cases pass")
    for failure in failures:
        print("  " + failure)
    return 1 if failures else 0


def main():
    if sys.argv[1:] == ["--self-test"]:
        sys.exit(self_test())
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    sid = sys.argv[1]
    duration, fails, frozen = gate(sid)
    print(json.dumps({"scene_id": sid, "duration_sec": duration,
                      "longest_frozen_run": frozen, "fails": fails}))
    sys.exit(1 if fails else 0)


if __name__ == "__main__":
    main()
