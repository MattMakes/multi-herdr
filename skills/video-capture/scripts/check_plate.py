#!/usr/bin/env python3
"""Plate gate for 1 capture scene. Python standard library, ffmpeg and ffprobe only.

Usage, from the video project root:
  check_plate.py <scene-id> [--zoom-plan <plan.json>]
                                print 1 JSON line, exit 0 on pass and 1 on any fail
  check_plate.py --self-test    gate synthetic plates in a temp folder
"""
import argparse, json, math, os, re, subprocess, sys, tempfile
from itertools import accumulate, groupby
from operator import itemgetter


def run(*a):
    return subprocess.run(a, capture_output=True, text=True).stdout


def frame_hashes(path):
    text = run("ffmpeg", "-v", "error", "-i", path, "-map", "0:v:0", "-f", "framemd5", "-")
    return [line.rsplit(",", 1)[-1].strip() for line in text.splitlines()
            if line and not line.startswith("#")]


def run_lengths(hashes):
    """Return (start frame, frame count) for each run of identical frames."""
    counts = [len(list(group)) for _, group in groupby(hashes)]
    return list(zip(accumulate([0] + counts[:-1]), counts))


def seconds(frames, fps):
    return round(frames / fps, 6) if fps else 0


def longest_frozen_run(path, fps):
    start, count = max(run_lengths(frame_hashes(path)), key=itemgetter(1), default=(0, 0))
    return {"start_sec": seconds(start, fps), "length_sec": seconds(count, fps), "frames": count}


def valid_number(value, minimum=0):
    return (isinstance(value, (int, float)) and not isinstance(value, bool)
            and math.isfinite(value) and value >= minimum)


def is_int(value):
    return isinstance(value, int) and not isinstance(value, bool)


def valid_actions(actions):
    return isinstance(actions, dict) and all(
        isinstance(name, str) and name and valid_number(sec) for name, sec in actions.items())


def valid_names(names):
    return isinstance(names, list) and all(isinstance(name, str) and name for name in names)


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
    "render_scale": (lambda value: valid_number(value, 1.0), "render_scale must be a number >= 1.0"),
    "timers_forced": (valid_names, "timers_forced must be a list of names"),
}


def rule_failures(sidecar):
    return [f"G6 sidecar {msg}" for key, (ok, msg) in SIDECAR_RULES.items() if not ok(sidecar[key])]


def match_failures(sidecar, expected):
    """Compare each well-formed sidecar field with the value measured from the clip."""
    return [f"G6 sidecar {key} {sidecar[key]!r}, clip {want!r}" for key, want in expected.items()
            if SIDECAR_RULES[key][0](sidecar[key]) and sidecar[key] != want]


def sidecar_failures(sidecar, expected):
    if not isinstance(sidecar, dict):
        return ["G6 sidecar must be a JSON object"]
    fields = set(SIDECAR_RULES)
    fails = [] if set(sidecar) == fields else ["G6 sidecar fields do not match the capture contract"]
    if fields.issubset(sidecar):
        fails += rule_failures(sidecar) + match_failures(sidecar, expected)
    return fails


def sidecar_path_of(path):
    return os.path.splitext(path)[0] + ".json"


def has_sidecar(path):
    """Only an app plate under plates/ has a sidecar. Its sidecar is optional."""
    return path.startswith("plates/") and os.path.exists(sidecar_path_of(path))


def read_sidecar(path, info):
    """Validate plates/<clip>.json against its contract and against the clip."""
    expected = {"clip": path, "fps": info["fps"], "frame_count": info["frame_count"]}
    sidecar_path = sidecar_path_of(path)
    try:
        sidecar = json.load(open(sidecar_path))
    except (json.JSONDecodeError, OSError) as error:
        return None, [f"G6 sidecar cannot be read: {error}"]
    return sidecar, sidecar_failures(sidecar, expected)


def zoom_required_scale(path, zoom_plan):
    scales = [row.get("required_capture_scale") for row in zoom_plan
              if isinstance(row, dict) and row.get("plate") == path]
    valid = [scale for scale in scales if valid_number(scale, 1.0)]
    return max(valid, default=None)


def render_scale_of(sidecar):
    return sidecar.get("render_scale") if isinstance(sidecar, dict) else None


def scale_satisfies(actual, required):
    return required is None or (valid_number(actual, 1.0) and actual >= required)


def scale_label(value):
    return f"{value:g}x" if valid_number(value) else "missing"


def failure_list(passed, message):
    return [] if passed else [message]


def zoom_plan_failures(path, sidecar, zoom_plan):
    required = zoom_required_scale(path, zoom_plan)
    actual = render_scale_of(sidecar)
    message = (f"G6 sidecar render_scale {scale_label(actual)} is below zoom plan "
               f"required scale {scale_label(required)} for {path}")
    return failure_list(scale_satisfies(actual, required), message)


def parse_rate(text):
    parts = text.split("/")
    if len(parts) != 2 or not float(parts[1]):
        return 0
    return float(parts[0]) / float(parts[1])


def probe(path):
    data = json.loads(run("ffprobe", "-v", "error", "-count_frames", "-select_streams", "v:0",
        "-show_entries", "stream=codec_type,width,height,r_frame_rate,nb_read_frames:format=duration",
        "-of", "json", path))
    video = (data.get("streams") or [{}])[0]
    rate = video.get("r_frame_rate", "0/1")
    return {"codec_type": video.get("codec_type"), "size": (video.get("width"), video.get("height")),
            "rate": rate, "fps": parse_rate(rate), "frame_count": int(video.get("nb_read_frames") or 0),
            "duration": float(data.get("format", {}).get("duration", 0))}


def luma(path):
    return dict(re.findall(r"signalstats\.(YMIN|YMAX)=(\d+)", run("ffmpeg", "-v", "error", "-i", path, "-vf",
        r"select=eq(n\,0),signalstats,metadata=print:file=-", "-frames:v", "1", "-f", "null", "-")))


def size_ok(size, meta, telemetry):
    """An app plate is 1x or 2x the timeline size. A CDP plate matches its telemetry."""
    width, height = meta["width"], meta["height"]
    if telemetry is None:
        return size in {(width, height), (2 * width, 2 * height)}
    return (size == (telemetry.get("width"), telemetry.get("height"))
            and telemetry.get("width") in (width, 2 * width))


def telemetry_for(sid, path):
    return None if path.startswith("plates/") else json.load(open(f"captures/{sid}.json"))


def stream_checks(info, meta, scene, path):
    telemetry = telemetry_for(scene["id"], path)
    width, height = info["size"]
    return [
        (info["codec_type"] == "video", "G1 no video stream"),
        (info["fps"] == meta["fps"], f"G1 rate {info['rate']}"),
        (size_ok(info["size"], meta, telemetry), f"G1 size {width}x{height}"),
        (abs(info["duration"] - scene["duration_sec"]) <= 0.05,
         f"G2 duration {info['duration']} s, timeline {scene['duration_sec']} s"),
    ]


def picture_checks(path, frozen):
    y = luma(path)
    return [
        (int(y.get("YMAX", 0)) - int(y.get("YMIN", 0)) >= 24, f"G3 frame 0 is blank: {y}"),
        (frozen["frames"] <= 1, f"G5 frozen run starts at {frozen['start_sec']} s and lasts "
                                f"{frozen['length_sec']} s ({frozen['frames']} frames)"),
    ]


def load_scene(sid):
    timeline = json.load(open("timeline.json"))
    return next(x for x in timeline["scenes"] if x["id"] == sid), timeline["meta"]


def failed_checks(checks):
    return [message for ok, message in checks if not ok]


def metadata_failures(path, info, zoom_plan):
    sidecar, failures = read_sidecar(path, info) if has_sidecar(path) else (None, [])
    return failures + zoom_plan_failures(path, sidecar, zoom_plan)


def plate_failures(path, scene, meta, info, frozen, zoom_plan):
    return (failed_checks(stream_checks(info, meta, scene, path) + picture_checks(path, frozen))
            + metadata_failures(path, info, zoom_plan))


def gate(sid, zoom_plan=()):
    scene, meta = load_scene(sid)
    path = scene.get("plate", f"captures/{sid}.mp4")
    info = probe(path)
    frozen = longest_frozen_run(path, info["fps"])
    return info["duration"], plate_failures(path, scene, meta, info, frozen, zoom_plan), frozen


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
    {"id": "sidecar-fail", "source": "testsrc2=size=320x180:rate=60", "seconds": 1.0, "width": 320, "plate": True, "bad_sidecar": True, "want": ["G6 sidecar fields", "G6 sidecar fps", "G6 sidecar frame_count", "G6 sidecar timers_forced"]},
    {"id": "render-scale-fail", "source": "testsrc2=size=320x180:rate=60", "seconds": 1.0, "width": 320, "plate": True, "render_scale": 0.5, "want": ["G6 sidecar render_scale must"]},
    {"id": "zoom-plan-pass", "source": "testsrc2=size=320x180:rate=60", "seconds": 1.0, "width": 320, "plate": True, "render_scale": 2.0, "want": []},
    {"id": "zoom-plan-fail", "source": "testsrc2=size=320x180:rate=60", "seconds": 1.0, "width": 320, "plate": True, "render_scale": 1.0, "want": ["G6 sidecar render_scale 1x is below zoom plan required scale 2x"]},
]


def case_path(case):
    folder = "plates" if case.get("plate") else "captures"
    return f"{folder}/{case['id']}.mp4"


def scene_for(case):
    scene = {"id": case["id"], "kind": "capture", "duration_sec": 1.0}
    if case.get("plate"):
        scene["plate"] = case_path(case)
    return scene


def write_clip(case):
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", case["source"],
                    "-t", str(case["seconds"]), "-c:v", "libx264", "-pix_fmt", "yuv420p",
                    case_path(case)], check=True)


def sidecar_for(case):
    sidecar = {
        "clip": case_path(case), "fps": 60, "frame_count": 60, "seed": 7, "build": "self-test",
        "key_actions_sec": {"action": 0.5},
        "pixel_facts": {"subject_width_px": 120, "nearest_distance_px": 40},
        "max_frozen_sec": 0.017,
        "render_scale": case.get("render_scale", 1.0),
        "timers_forced": ["hit_stop"],
    }
    if case.get("bad_sidecar"):
        sidecar.update({"fps": 30, "frame_count": 59, "timers_forced": "hit_stop", "unexpected": True})
    return sidecar


def write_metadata(case):
    """Write the sidecar of an app plate, or the telemetry of a CDP capture."""
    telemetry = {"width": case["width"], "height": case["width"] * 9 // 16}
    data = sidecar_for(case) if case.get("plate") else telemetry
    json.dump(data, open(os.path.splitext(case_path(case))[0] + ".json", "w"))


def case_failure(case, zoom_plan):
    """Gate 1 synthetic case. Return a failure line, or None when the fails match the case."""
    write_clip(case)
    write_metadata(case)
    _, fails, _ = gate(case["id"], zoom_plan)
    unexpected, missing = fail_mismatch(case["want"], fails)
    if unexpected or missing:
        return f"{case['id']}: unexpected {unexpected}, missing {missing}"
    return None


def fail_mismatch(want, fails):
    """Return the fails no wanted prefix covers, and the wanted prefixes no fail has."""
    unexpected = [f for f in fails if not f.startswith(tuple(want))]
    return unexpected, [prefix for prefix in want if not any(f.startswith(prefix) for f in fails)]


def prepare_case_root(root):
    os.chdir(root)
    os.makedirs("captures")
    os.makedirs("plates")
    json.dump({"meta": {"fps": 60, "width": 320, "height": 180},
               "scenes": [scene_for(case) for case in CASES]}, open("timeline.json", "w"))


def self_test_zoom_plan():
    return [
        {"scene_id": "zoom-plan-pass", "plate": "plates/zoom-plan-pass.mp4",
         "zoom_need": 2.0, "required_capture_scale": 2.0},
        {"scene_id": "zoom-plan-fail", "plate": "plates/zoom-plan-fail.mp4",
         "zoom_need": 2.0, "required_capture_scale": 2.0},
    ]


def run_cases(root):
    """Gate every case in root. self_test restores the working folder after the run."""
    prepare_case_root(root)
    zoom_plan = self_test_zoom_plan()
    return [failure for failure in (case_failure(case, zoom_plan) for case in CASES) if failure]


def self_test():
    old_cwd = os.getcwd()
    with tempfile.TemporaryDirectory(prefix="check-plate-") as root:
        failures = run_cases(root)
    os.chdir(old_cwd)
    print(f"check_plate self-test: {len(CASES) - len(failures)} of {len(CASES)} cases pass")
    sys.stdout.write("".join(f"  {failure}\n" for failure in failures))
    return 1 if failures else 0


def cli_args():
    parser = argparse.ArgumentParser(description="Gate one captured video plate.")
    parser.add_argument("scene_id", nargs="?", help="timeline scene id")
    parser.add_argument("--zoom-plan", help="zoom plan written by check_timeline.py")
    parser.add_argument("--self-test", action="store_true", help="gate synthetic plates")
    args = parser.parse_args()
    if not args.self_test and not args.scene_id:
        parser.error("give a scene id or --self-test")
    return args


def run_cli(args):
    zoom_plan = json.load(open(args.zoom_plan)) if args.zoom_plan else []
    duration, fails, frozen = gate(args.scene_id, zoom_plan)
    print(json.dumps({"scene_id": args.scene_id, "duration_sec": duration,
                      "longest_frozen_run": frozen, "fails": fails}))
    return 1 if fails else 0


def main():
    args = cli_args()
    sys.exit(self_test() if args.self_test else run_cli(args))


if __name__ == "__main__":
    main()
