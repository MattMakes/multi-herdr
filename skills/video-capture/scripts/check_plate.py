#!/usr/bin/env python3
"""Plate gate for 1 capture scene. Python standard library, ffmpeg and ffprobe only.

Usage, from the video project root:
  check_plate.py <scene-id>     print 1 JSON line, exit 0 on pass and 1 on any fail
  check_plate.py --self-test    gate synthetic plates in a temp folder
"""
import json, os, re, subprocess, sys, tempfile


def run(*a):
    return subprocess.run(a, capture_output=True, text=True).stdout


def gate(sid):
    p = f"captures/{sid}.mp4"
    t = json.load(open("timeline.json")); s = next(x for x in t["scenes"] if x["id"] == sid)
    tel = json.load(open(f"captures/{sid}.json")); m = t["meta"]
    st = json.loads(run("ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries",
        "stream=codec_type,width,height,r_frame_rate:format=duration", "-of", "json", p))
    v = (st.get("streams") or [{}])[0]; dur = float(st.get("format", {}).get("duration", 0))
    y = dict(re.findall(r"signalstats\.(YMIN|YMAX)=(\d+)", run("ffmpeg", "-v", "error", "-i", p, "-vf",
        r"select=eq(n\,0),signalstats,metadata=print:file=-", "-frames:v", "1", "-f", "null", "-")))
    fails = [msg for ok, msg in [
        (v.get("codec_type") == "video", "G1 no video stream"),
        (v.get("r_frame_rate") == f"{m['fps']}/1", f"G1 rate {v.get('r_frame_rate')}"),
        ((v.get("width"), v.get("height")) == (tel["width"], tel["height"]) and tel["width"] in (m["width"], 2 * m["width"]),
         f"G1 size {v.get('width')}x{v.get('height')}"),
        (abs(dur - s["duration_sec"]) <= 0.05, f"G2 duration {dur} s, timeline {s['duration_sec']} s"),
        (int(y.get("YMAX", 0)) - int(y.get("YMIN", 0)) >= 24, f"G3 frame 0 is blank: {y}")] if not ok]
    return dur, fails


# (scene id, lavfi source, rate in fps, duration in s, telemetry width, expected fail prefixes)
CASES = [
    ("pass-1x", "testsrc2=size=320x180", 60, 1.0, 320, []),
    ("pass-2x", "testsrc2=size=640x360", 60, 1.0, 640, []),
    ("blank", "color=c=black:size=320x180", 60, 1.0, 320, ["G3"]),
    ("short", "testsrc2=size=320x180", 60, 0.5, 320, ["G2"]),
    ("rate", "testsrc2=size=320x180", 30, 1.0, 320, ["G1 rate"]),
    ("size", "testsrc2=size=480x270", 60, 1.0, 480, ["G1 size"]),
]


def self_test():
    failures = []
    with tempfile.TemporaryDirectory(prefix="check-plate-") as root:
        os.makedirs(os.path.join(root, "captures"))
        os.chdir(root)
        timeline = {"meta": {"fps": 60, "width": 320, "height": 180},
                    "scenes": [{"id": c[0], "kind": "capture", "duration_sec": 1.0} for c in CASES]}
        json.dump(timeline, open("timeline.json", "w"))
        for sid, src, fps, sec, tw, want in CASES:
            subprocess.run(["ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", f"{src}:rate={fps}", "-t", str(sec),
                            "-c:v", "libx264", "-pix_fmt", "yuv420p", f"captures/{sid}.mp4"], check=True)
            json.dump({"width": tw, "height": tw * 9 // 16}, open(f"captures/{sid}.json", "w"))
            _, fails = gate(sid)
            got = [f for f in fails if not any(f.startswith(w) for w in want)]
            missing = [w for w in want if not any(f.startswith(w) for f in fails)]
            if got or missing:
                failures.append(f"{sid}: unexpected {got}, missing {missing}")
        os.chdir("/")
    print(f"check_plate self-test: {len(CASES) - len(failures)} of {len(CASES)} cases pass")
    for f in failures:
        print("  " + f)
    return 1 if failures else 0


def main():
    if sys.argv[1:] == ["--self-test"]:
        sys.exit(self_test())
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    sid = sys.argv[1]
    dur, fails = gate(sid)
    print(json.dumps({"scene_id": sid, "duration_sec": dur, "fails": fails}))
    sys.exit(1 if fails else 0)


if __name__ == "__main__":
    main()
