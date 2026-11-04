#!/usr/bin/env python3
"""Numeric plan checker for a video project timeline.json.

Usage:
  check_timeline.py <timeline.json> [--out <qa/plan-check.json>]
  check_timeline.py --self-test

Exit 0 when every gate passes, 1 when a gate fails, 2 on a usage error.
The JSON report goes to --out (default: <timeline dir>/qa/plan-check.json)
and to stdout. Python 3 standard library only.
"""

import argparse
import json
import math
import os
import sys

RULES = [
    "hook_window",
    "scene_duration",
    "caption_density",
    "speaking_rate",
    "visual_variety",
    "safe_zone",
    "contiguous_timing",
    "schema",
]

KINDS = {"hook", "capture", "motion", "character", "diagram", "callout", "cta"}
ASPECTS = {"16:9": (1920, 1080), "9:16": (1080, 1920), "1:1": (1080, 1080)}
FPS_VALUES = {30, 60}

HOOK_DEADLINE_SEC = 3.0
DURATION_MIN_SEC, DURATION_MAX_SEC = 2.0, 8.0
OVERRIDE_MIN_SEC, OVERRIDE_MAX_SEC = 1.0, 12.0
CAPTION_MAX_WORDS, CAPTION_MAX_CHARS, CAPTION_MAX_WORDS_2_LINES = 7, 42, 14
MAX_WORDS_PER_SEC = 2.7
BREATH_PAD_SEC = 0.25
MAX_SAME_KIND_RUN = 2
FOCAL_SHIFT_MIN = 0.25
CAPTION_ZONE_TOP = 0.82
VERTICAL_UI_EDGE = 0.85
START_TOLERANCE_SEC = 1e-3
TOTAL_TOLERANCE_SEC = 0.05


def is_number(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value)


def is_int(value):
    return isinstance(value, int) and not isinstance(value, bool)


def non_empty_str(value):
    return isinstance(value, str) and value.strip() != ""


def word_count(text):
    return len(text.split())


class Report:
    def __init__(self):
        self.errors = []

    def add(self, rule, message, scene=None):
        entry = {"rule": rule, "message": message}
        if scene is not None:
            entry["scene"] = scene
        self.errors.append(entry)


def scene_label(scene, index):
    if isinstance(scene, dict) and non_empty_str(scene.get("id")):
        return scene["id"]
    return "scenes[%d]" % index


def check_meta(meta, report):
    if not isinstance(meta, dict):
        report.add("schema", "meta must be an object")
        return
    for key in ("id", "title"):
        if not non_empty_str(meta.get(key)):
            report.add("schema", "meta.%s must be a non-empty string" % key)
    aspect = meta.get("aspect")
    if aspect not in ASPECTS:
        report.add("schema", "meta.aspect must be one of %s" % sorted(ASPECTS))
    if meta.get("fps") not in FPS_VALUES or not is_int(meta.get("fps")):
        report.add("schema", "meta.fps must be 30 or 60")
    width, height = meta.get("width"), meta.get("height")
    if not (is_int(width) and is_int(height)):
        report.add("schema", "meta.width and meta.height must be integers")
    elif aspect in ASPECTS and (width, height) != ASPECTS[aspect]:
        report.add("schema", "meta.width x meta.height must be %dx%d for aspect %s" % (ASPECTS[aspect] + (aspect,)))
    if not is_number(meta.get("duration_sec")) or meta.get("duration_sec") <= 0:
        report.add("schema", "meta.duration_sec must be a number > 0")
    if not is_number(meta.get("lufs_target")):
        report.add("schema", "meta.lufs_target must be a number")
    if "allow_dark_intro" in meta and not isinstance(meta["allow_dark_intro"], bool):
        report.add("schema", "meta.allow_dark_intro must be a boolean when present")
    if "allowed_silences" in meta:
        check_allowed_silences(meta["allowed_silences"], meta.get("duration_sec"), report)


def check_allowed_silences(ranges, duration, report):
    message = "meta.allowed_silences must be a list of [start_sec, end_sec] with 0 <= start_sec < end_sec <= meta.duration_sec"
    if not isinstance(ranges, list):
        report.add("schema", message)
        return
    for item in ranges:
        ok = check_box(item, 2) and 0 <= item[0] < item[1]
        if ok and is_number(duration):
            ok = item[1] <= duration
        if not ok:
            report.add("schema", message)
            return


def check_box(value, size):
    return isinstance(value, list) and len(value) == size and all(is_number(v) for v in value)


def check_scene_schema(scene, label, report):
    if not isinstance(scene, dict):
        report.add("schema", "scene must be an object", label)
        return
    if not non_empty_str(scene.get("id")):
        report.add("schema", "id must be a non-empty string", label)
    if scene.get("kind") not in KINDS:
        report.add("schema", "kind must be one of %s" % sorted(KINDS), label)
    for key in ("start_sec", "duration_sec"):
        if not is_number(scene.get(key)) or scene.get(key) < 0:
            report.add("schema", "%s must be a number >= 0" % key, label)
    if "duration_override_reason" in scene and not isinstance(scene["duration_override_reason"], str):
        report.add("schema", "duration_override_reason must be a string when present", label)
    if "static_hold" in scene and not isinstance(scene["static_hold"], bool):
        report.add("schema", "static_hold must be a boolean when present", label)
    beat_in, beat_out = scene.get("beat_in"), scene.get("beat_out")
    if not (is_int(beat_in) and is_int(beat_out) and 0 <= beat_in <= beat_out):
        report.add("schema", "beat_in and beat_out must be integers with 0 <= beat_in <= beat_out", label)
    for key in ("narration", "caption"):
        if not isinstance(scene.get(key), str):
            report.add("schema", "%s must be a string (empty allowed)" % key, label)
    for key in ("plate", "transition_out"):
        if not non_empty_str(scene.get(key)):
            report.add("schema", "%s must be a non-empty string" % key, label)
    motion = scene.get("motion")
    if not isinstance(motion, dict) or not non_empty_str(motion.get("template")):
        report.add("schema", "motion must be an object with a non-empty template", label)
    elif "reveal_sec" in motion and not (is_number(motion["reveal_sec"]) and motion["reveal_sec"] >= 0):
        report.add("schema", "motion.reveal_sec must be a number >= 0 when present", label)
    regions = scene.get("critical_regions")
    if not isinstance(regions, list) or not all(check_box(r, 4) for r in regions):
        report.add("schema", "critical_regions must be a list of [x, y, w, h] number lists", label)
    focal = scene.get("focal_point")
    if not check_box(focal, 2) or not all(0.0 <= v <= 1.0 for v in focal):
        report.add("schema", "focal_point must be [x, y] with values in [0.0, 1.0]", label)


def check_schema(timeline, report):
    if not isinstance(timeline, dict):
        report.add("schema", "timeline must be a JSON object")
        return
    check_meta(timeline.get("meta"), report)
    scenes = timeline.get("scenes")
    if not isinstance(scenes, list) or not scenes:
        report.add("schema", "scenes must be a non-empty list")
        return
    seen = set()
    for index, scene in enumerate(scenes):
        label = scene_label(scene, index)
        check_scene_schema(scene, label, report)
        if isinstance(scene, dict) and non_empty_str(scene.get("id")):
            if scene["id"] in seen:
                report.add("schema", "scene id is not unique", label)
            seen.add(scene["id"])


def check_hook(scenes, report):
    first = scenes[0]
    label = first["id"]
    if first["start_sec"] != 0.0:
        report.add("hook_window", "scenes[0].start_sec must be 0.0", label)
    if first["kind"] != "hook":
        report.add("hook_window", "scenes[0].kind must be hook", label)
    if not first["caption"].strip():
        report.add("hook_window", "the hook scene needs a headline caption", label)
    reveal = first["motion"].get("reveal_sec", first["duration_sec"])
    lands_at = first["start_sec"] + reveal
    if lands_at > HOOK_DEADLINE_SEC:
        report.add(
            "hook_window",
            "hook motion and headline land at %.3f s; the deadline is %.1f s "
            "(set motion.reveal_sec or shorten the scene)" % (lands_at, HOOK_DEADLINE_SEC),
            label,
        )


def check_durations(scenes, report):
    for scene in scenes:
        duration = scene["duration_sec"]
        if non_empty_str(scene.get("duration_override_reason")):
            low, high = OVERRIDE_MIN_SEC, OVERRIDE_MAX_SEC
        else:
            low, high = DURATION_MIN_SEC, DURATION_MAX_SEC
        if not low <= duration <= high:
            report.add(
                "scene_duration",
                "duration_sec %.3f s is outside [%.1f, %.1f] s" % (duration, low, high),
                scene["id"],
            )


def check_captions(scenes, report):
    for scene in scenes:
        caption = scene["caption"]
        if not caption.strip():
            continue
        lines = caption.split("\n")
        words = word_count(caption)
        if len(lines) > 2:
            report.add("caption_density", "caption has %d lines; the limit is 2" % len(lines), scene["id"])
            continue
        long_lines = [line for line in lines if len(line) > CAPTION_MAX_CHARS]
        if long_lines:
            report.add(
                "caption_density",
                "a caption line has %d characters; the limit is %d" % (max(map(len, long_lines)), CAPTION_MAX_CHARS),
                scene["id"],
            )
        word_limit = CAPTION_MAX_WORDS if len(lines) == 1 else CAPTION_MAX_WORDS_2_LINES
        if words > word_limit:
            report.add(
                "caption_density",
                "caption has %d words; the limit is %d for %d line(s)" % (words, word_limit, len(lines)),
                scene["id"],
            )


def check_speaking_rate(scenes, report):
    for scene in scenes:
        words = word_count(scene["narration"])
        if words == 0:
            continue
        speaking_sec = scene["duration_sec"] - BREATH_PAD_SEC
        if speaking_sec <= 0:
            report.add(
                "speaking_rate",
                "scene is too short for narration plus a %.2f s breath pad" % BREATH_PAD_SEC,
                scene["id"],
            )
            continue
        rate = words / speaking_sec
        if rate > MAX_WORDS_PER_SEC:
            report.add(
                "speaking_rate",
                "narration is %.2f words/s (%d words in %.2f s after the breath pad); the limit is %.1f words/s"
                % (rate, words, speaking_sec, MAX_WORDS_PER_SEC),
                scene["id"],
            )


def check_variety(scenes, report):
    run = 1
    for prev, scene in zip(scenes, scenes[1:]):
        run = run + 1 if scene["kind"] == prev["kind"] else 1
        if run == MAX_SAME_KIND_RUN + 1:
            report.add(
                "visual_variety",
                "more than %d consecutive scenes have kind %s" % (MAX_SAME_KIND_RUN, scene["kind"]),
                scene["id"],
            )
        shift = math.dist(prev["focal_point"], scene["focal_point"])
        changed = (
            scene["kind"] != prev["kind"]
            or scene["plate"] != prev["plate"]
            or scene["motion"]["template"] != prev["motion"]["template"]
            or shift >= FOCAL_SHIFT_MIN
        )
        if not changed:
            report.add(
                "visual_variety",
                "scene repeats %s: same kind, plate and motion.template, and focal shift %.3f < %.2f"
                % (prev["id"], shift, FOCAL_SHIFT_MIN),
                scene["id"],
            )


def check_safe_zone(meta, scenes, report):
    vertical = meta["aspect"] == "9:16"
    for scene in scenes:
        for index, (x, y, w, h) in enumerate(scene["critical_regions"]):
            where = "critical_regions[%d]" % index
            if x < 0 or y < 0 or w <= 0 or h <= 0 or x + w > 1.0 or y + h > 1.0:
                report.add("safe_zone", "%s leaves the [0.0, 1.0] frame or has no area" % where, scene["id"])
                continue
            if y + h > CAPTION_ZONE_TOP:
                report.add(
                    "safe_zone",
                    "%s bottom edge %.3f enters the caption zone below y = %.2f" % (where, y + h, CAPTION_ZONE_TOP),
                    scene["id"],
                )
            if vertical and x + w > VERTICAL_UI_EDGE:
                report.add(
                    "safe_zone",
                    "%s right edge %.3f enters the 9:16 platform UI zone right of x = %.2f"
                    % (where, x + w, VERTICAL_UI_EDGE),
                    scene["id"],
                )


def check_timing(meta, scenes, report):
    if scenes[0]["start_sec"] != 0.0:
        report.add("contiguous_timing", "scenes[0].start_sec must be 0.0", scenes[0]["id"])
    for prev, scene in zip(scenes, scenes[1:]):
        expected = prev["start_sec"] + prev["duration_sec"]
        if abs(scene["start_sec"] - expected) > START_TOLERANCE_SEC:
            report.add(
                "contiguous_timing",
                "start_sec %.3f s must equal %.3f s (end of %s)" % (scene["start_sec"], expected, prev["id"]),
                scene["id"],
            )
    total = sum(scene["duration_sec"] for scene in scenes)
    if abs(total - meta["duration_sec"]) > TOTAL_TOLERANCE_SEC:
        report.add(
            "contiguous_timing",
            "scene durations sum to %.3f s; meta.duration_sec is %.3f s" % (total, meta["duration_sec"]),
        )


def check_timeline(timeline):
    """Return the list of rule-keyed errors for a parsed timeline."""
    report = Report()
    check_schema(timeline, report)
    if report.errors:
        return report.errors
    meta, scenes = timeline["meta"], timeline["scenes"]
    check_hook(scenes, report)
    check_durations(scenes, report)
    check_captions(scenes, report)
    check_speaking_rate(scenes, report)
    check_variety(scenes, report)
    check_safe_zone(meta, scenes, report)
    check_timing(meta, scenes, report)
    return report.errors


def run(timeline_path, out_path):
    try:
        with open(timeline_path, encoding="utf-8") as handle:
            timeline = json.load(handle)
        errors = check_timeline(timeline)
    except (OSError, ValueError) as exc:
        errors = [{"rule": "schema", "message": "cannot read timeline: %s" % exc}]
    result = {
        "passed": not errors,
        "timeline": os.path.abspath(timeline_path),
        "rules": RULES,
        "errors": errors,
    }
    if out_path is None:
        out_path = os.path.join(os.path.dirname(os.path.abspath(timeline_path)), "qa", "plan-check.json")
    os.makedirs(os.path.dirname(os.path.abspath(out_path)), exist_ok=True)
    with open(out_path, "w", encoding="utf-8") as handle:
        json.dump(result, handle, indent=2)
        handle.write("\n")
    print(json.dumps(result, indent=2))
    return 0 if result["passed"] else 1


def self_test():
    import copy
    import subprocess
    import tempfile
    import unittest

    def valid_timeline():
        def scene(sid, kind, start, duration, plate, template, focal, caption="", narration="", **extra):
            data = {
                "id": sid,
                "kind": kind,
                "start_sec": start,
                "duration_sec": duration,
                "beat_in": int(start * 2),
                "beat_out": int((start + duration) * 2),
                "narration": narration,
                "caption": caption,
                "plate": plate,
                "motion": {"template": template},
                "transition_out": "cut",
                "critical_regions": [[0.1, 0.2, 0.6, 0.4]],
                "focal_point": focal,
            }
            data.update(extra)
            return data

        return {
            "meta": {
                "id": "demo-launch",
                "title": "Demo launch",
                "aspect": "16:9",
                "fps": 60,
                "width": 1920,
                "height": 1080,
                "duration_sec": 15.0,
                "lufs_target": -14,
            },
            "scenes": [
                scene("s01-hook", "hook", 0.0, 2.5, "scenes/s01-hook/index.html", "slam-in",
                      [0.5, 0.45], caption="Stop flaky tests", narration="Flaky tests waste hours."),
                scene("s02-demo", "capture", 2.5, 5.0, "captures/s02-demo.mp4", "zoom-follow",
                      [0.4, 0.5], caption="One command finds them", narration="Run one command and see every flaky test."),
                scene("s03-how", "diagram", 7.5, 4.5, "scenes/s03-how/index.html", "build-up",
                      [0.6, 0.4], narration="It reruns each test and compares results."),
                scene("s04-cta", "cta", 12.0, 3.0, "scenes/s04-cta/index.html", "fade-up",
                      [0.5, 0.5], caption="Try it today", narration="Try it today."),
            ],
        }

    script = os.path.abspath(__file__)
    skills_dir = os.path.dirname(os.path.dirname(os.path.dirname(script)))

    def rules_of(timeline):
        return {error["rule"] for error in check_timeline(timeline)}

    class CheckTimelineTest(unittest.TestCase):
        def setUp(self):
            self.timeline = valid_timeline()
            self.scenes = self.timeline["scenes"]

        def assert_cli_fails_with(self, rule):
            with tempfile.TemporaryDirectory() as tmp:
                path = os.path.join(tmp, "timeline.json")
                with open(path, "w", encoding="utf-8") as handle:
                    json.dump(self.timeline, handle)
                proc = subprocess.run([sys.executable, script, path], capture_output=True, text=True, timeout=60)
                self.assertNotEqual(proc.returncode, 0, proc.stdout)
                with open(os.path.join(tmp, "qa", "plan-check.json"), encoding="utf-8") as handle:
                    report = json.load(handle)
            self.assertFalse(report["passed"])
            self.assertIn(rule, {error["rule"] for error in report["errors"]})

        def test_valid_timeline_passes_and_writes_report(self):
            with tempfile.TemporaryDirectory() as tmp:
                path = os.path.join(tmp, "timeline.json")
                with open(path, "w", encoding="utf-8") as handle:
                    json.dump(self.timeline, handle)
                proc = subprocess.run([sys.executable, script, path], capture_output=True, text=True, timeout=60)
                self.assertEqual(proc.returncode, 0, proc.stdout)
                out = os.path.join(tmp, "qa", "plan-check.json")
                self.assertTrue(os.path.isfile(out))
                with open(out, encoding="utf-8") as handle:
                    report = json.load(handle)
            self.assertTrue(report["passed"])
            self.assertEqual(report["errors"], [])

        def test_out_flag_writes_given_path(self):
            with tempfile.TemporaryDirectory() as tmp:
                path = os.path.join(tmp, "timeline.json")
                out = os.path.join(tmp, "reports", "check.json")
                with open(path, "w", encoding="utf-8") as handle:
                    json.dump(self.timeline, handle)
                proc = subprocess.run([sys.executable, script, path, "--out", out],
                                      capture_output=True, text=True, timeout=60)
                self.assertEqual(proc.returncode, 0, proc.stdout)
                self.assertTrue(os.path.isfile(out))

        def test_1_hook_window(self):
            self.scenes[0]["motion"]["reveal_sec"] = 3.2
            self.scenes[0]["duration_sec"] = 3.5
            self.scenes[1]["start_sec"] = 3.5
            self.scenes[1]["duration_sec"] = 4.0
            self.assertEqual(rules_of(self.timeline), {"hook_window"})
            self.assert_cli_fails_with("hook_window")

        def test_1_hook_kind(self):
            self.scenes[0]["kind"] = "motion"
            self.assertIn("hook_window", rules_of(self.timeline))

        def test_1_hook_reveal_inside_long_scene_passes(self):
            self.scenes[0]["duration_sec"] = 4.0
            self.scenes[0]["motion"]["reveal_sec"] = 1.2
            self.scenes[1]["start_sec"] = 4.0
            self.scenes[1]["duration_sec"] = 3.5
            self.assertEqual(rules_of(self.timeline), set())

        def test_2_scene_duration(self):
            self.scenes[2]["duration_sec"] = 1.5
            self.scenes[2]["narration"] = "Rerun and compare."
            self.scenes[3]["start_sec"] = 9.0
            self.scenes[3]["duration_sec"] = 6.0
            self.assertEqual(rules_of(self.timeline), {"scene_duration"})
            self.assert_cli_fails_with("scene_duration")

        def test_2_override_reason_widens_bounds(self):
            self.scenes[2]["duration_sec"] = 1.5
            self.scenes[2]["narration"] = "Rerun and compare."
            self.scenes[2]["duration_override_reason"] = "beat-matched stinger"
            self.scenes[3]["start_sec"] = 9.0
            self.scenes[3]["duration_sec"] = 6.0
            self.assertEqual(rules_of(self.timeline), set())
            self.scenes[2]["duration_sec"] = 0.9
            self.scenes[3]["start_sec"] = 8.4
            self.scenes[3]["duration_sec"] = 6.6
            self.assertIn("scene_duration", rules_of(self.timeline))

        def test_3_caption_density(self):
            self.scenes[1]["caption"] = "One single command finds every flaky test fast"
            self.assertEqual(rules_of(self.timeline), {"caption_density"})
            self.assert_cli_fails_with("caption_density")

        def test_3_caption_two_lines(self):
            self.scenes[1]["caption"] = "One single command finds\nevery flaky test in seconds"
            self.assertEqual(rules_of(self.timeline), set())
            self.scenes[1]["caption"] = "a\nb\nc"
            self.assertIn("caption_density", rules_of(self.timeline))

        def test_4_speaking_rate(self):
            self.scenes[0]["narration"] = "Flaky tests waste hours of every engineer's week, every week."
            self.assertEqual(rules_of(self.timeline), {"speaking_rate"})
            self.assert_cli_fails_with("speaking_rate")

        def test_5_visual_variety_same_kind_run(self):
            for index, sid in ((1, "s02-demo"), (2, "s03-how"), (3, "s04-cta")):
                self.scenes[index]["kind"] = "capture"
            self.assertEqual(rules_of(self.timeline), {"visual_variety"})
            self.assert_cli_fails_with("visual_variety")

        def test_5_visual_variety_no_change(self):
            twin = copy.deepcopy(self.scenes[1])
            twin.update({"id": "s02b-demo", "kind": "capture"})
            self.scenes[2] = twin
            self.scenes[2]["start_sec"] = 7.5
            self.scenes[2]["duration_sec"] = 4.5
            self.assertEqual(rules_of(self.timeline), {"visual_variety"})

        def test_6_safe_zone_caption_band(self):
            self.scenes[1]["critical_regions"] = [[0.1, 0.6, 0.5, 0.3]]
            self.assertEqual(rules_of(self.timeline), {"safe_zone"})
            self.assert_cli_fails_with("safe_zone")

        def test_6_safe_zone_vertical_ui(self):
            meta = self.timeline["meta"]
            meta.update({"aspect": "9:16", "width": 1080, "height": 1920})
            self.scenes[1]["critical_regions"] = [[0.5, 0.2, 0.4, 0.3]]
            self.assertEqual(rules_of(self.timeline), {"safe_zone"})
            meta.update({"aspect": "16:9", "width": 1920, "height": 1080})
            self.assertEqual(rules_of(self.timeline), set())

        def test_7_contiguous_timing(self):
            self.scenes[2]["start_sec"] = 7.6
            self.assertEqual(rules_of(self.timeline), {"contiguous_timing"})
            self.assert_cli_fails_with("contiguous_timing")

        def test_7_total_duration(self):
            self.timeline["meta"]["duration_sec"] = 15.1
            self.assertEqual(rules_of(self.timeline), {"contiguous_timing"})

        def test_8_schema(self):
            self.scenes[1]["id"] = ""
            del self.scenes[2]["plate"]
            self.timeline["meta"]["fps"] = 24
            self.assertEqual(rules_of(self.timeline), {"schema"})
            self.assert_cli_fails_with("schema")

        def test_8_static_hold(self):
            self.scenes[2]["static_hold"] = True
            self.assertEqual(rules_of(self.timeline), set())
            self.scenes[2]["static_hold"] = "yes"
            self.assertEqual(rules_of(self.timeline), {"schema"})

        def test_8_allowed_silences(self):
            self.timeline["meta"]["allowed_silences"] = [[2.0, 3.5]]
            self.assertEqual(rules_of(self.timeline), set())
            self.timeline["meta"]["allowed_silences"] = [[3.5, 2.0]]
            self.assertEqual(rules_of(self.timeline), {"schema"})
            self.timeline["meta"]["allowed_silences"] = [[2.0, 99.0]]
            self.assertEqual(rules_of(self.timeline), {"schema"})

        def test_8_unreadable_json(self):
            with tempfile.TemporaryDirectory() as tmp:
                path = os.path.join(tmp, "timeline.json")
                with open(path, "w", encoding="utf-8") as handle:
                    handle.write("{not json")
                proc = subprocess.run([sys.executable, script, path], capture_output=True, text=True, timeout=60)
            self.assertEqual(proc.returncode, 1)
            self.assertEqual(json.loads(proc.stdout)["errors"][0]["rule"], "schema")

        def test_skill_files_under_byte_cap(self):
            for skill in ("video-direction", "video-storyboard"):
                path = os.path.join(skills_dir, skill, "SKILL.md")
                # A fleet bundle holds only the teammate's own skills.
                if os.path.exists(path):
                    self.assertLess(os.path.getsize(path), 12288, path)

    suite = unittest.defaultTestLoader.loadTestsFromTestCase(CheckTimelineTest)
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    return 0 if result.wasSuccessful() else 1


def main(argv=None):
    parser = argparse.ArgumentParser(description="Check a video timeline.json against the numeric plan gates.")
    parser.add_argument("timeline", nargs="?", help="path to timeline.json")
    parser.add_argument("--out", help="report path (default: <timeline dir>/qa/plan-check.json)")
    parser.add_argument("--self-test", action="store_true", help="run the built-in tests")
    args = parser.parse_args(argv)
    if args.self_test:
        return self_test()
    if not args.timeline:
        parser.error("give a timeline.json path or --self-test")
    return run(args.timeline, args.out)


if __name__ == "__main__":
    sys.exit(main())
