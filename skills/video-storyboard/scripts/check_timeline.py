#!/usr/bin/env python3
"""Numeric plan checker for a video project timeline.json.

Usage:
  check_timeline.py <timeline.json> [--out <qa/plan-check.json>]
  check_timeline.py --self-test

Exit 0 when every gate passes, 1 when a gate fails, 2 on a usage error.
The JSON report goes to --out (default: <timeline dir>/qa/plan-check.json)
and to stdout. The CLI also checks DIRECTION.md and its key frames in the
timeline folder, and materials/materials.md when a scene has kind capture.
Python 3 standard library only.
"""

import argparse
import json
import math
import os
import re
import sys

RULES = [
    "hook_window",
    "scene_duration",
    "caption_density",
    "caption_place",
    "speaking_rate",
    "visual_variety",
    "safe_zone",
    "contiguous_timing",
    "sync",
    "read_hold",
    "terms",
    "rhythm",
    "event_gap",
    "hero",
    "menu_time",
    "directions",
    "materials",
    "schema",
]

KINDS = {"hook", "capture", "motion", "character", "diagram", "callout", "cta", "ui", "menu"}
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
READ_MIN_SEC, READ_BASE_SEC, READ_PER_WORD_SEC = 1.5, 1.0, 0.3
CAPTION_BAND_TOP, CAPTION_BAND_BOTTOM = 0.84, 0.95
VERTICAL_CAPTION_BOTTOM, VERTICAL_CAPTION_RIGHT = 0.80, 0.85
CUT_SYNC_SEC, ACTION_SYNC_SEC = 0.020, 0.070
MENU_SEC_PER_FILM_SEC = 8.0 / 80.0
GAUGE_LEAD_MIN_SEC, GAUGE_TAIL_MIN_SEC = 0.3, 1.2
ENERGY_MIN, ENERGY_MAX = 0, 10
MAX_GAP_BARS = 2
BEATS_PER_BAR = 4
HERO_EARLIEST, HERO_LATEST = 0.30, 0.90
MIN_DIRECTIONS = 3
PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"
HEX_COLOR = re.compile(r"#[0-9A-Fa-f]{6}\b")


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
    if "bpm" in meta and not (is_number(meta["bpm"]) and meta["bpm"] > 0):
        report.add("schema", "meta.bpm must be a number > 0 when present")
    if "terms" in meta and not (isinstance(meta["terms"], list) and all(non_empty_str(t) for t in meta["terms"])):
        report.add("schema", "meta.terms must be a list of non-empty strings when present")
    for key in ("hero_sec", "payoff_silence_sec"):
        if key in meta and not is_range(meta[key], meta.get("duration_sec")):
            report.add("schema", "meta.%s must be [start_sec, end_sec] with 0 <= start_sec < end_sec <= meta.duration_sec" % key)
    if "payoff_silence_sec" in meta and "hero_sec" not in meta:
        report.add("schema", "meta.payoff_silence_sec needs meta.hero_sec")


def is_range(value, duration):
    if not (check_box(value, 2) and 0 <= value[0] < value[1]):
        return False
    return not is_number(duration) or value[1] <= duration


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


def is_normalized_rect(value):
    return (
        check_box(value, 4)
        and value[0] >= 0.0
        and value[1] >= 0.0
        and value[2] > 0.0
        and value[3] > 0.0
        and value[0] + value[2] <= 1.0
        and value[1] + value[3] <= 1.0
    )


def has_exact_fields(value, fields):
    return isinstance(value, dict) and set(value) == set(fields)


def check_scene_schema(scene, label, report):
    if not isinstance(scene, dict):
        report.add("schema", "scene must be an object", label)
        return
    for check in (check_scene_identity, check_scene_holds, check_scene_text, check_scene_layout,
                  check_optional_scene_fields):
        check(scene, label, report)


def check_scene_identity(scene, label, report):
    if not non_empty_str(scene.get("id")):
        report.add("schema", "id must be a non-empty string", label)
    if scene.get("kind") not in KINDS:
        report.add("schema", "kind must be one of %s" % sorted(KINDS), label)
    for key in ("start_sec", "duration_sec"):
        if not is_number(scene.get(key)) or scene.get(key) < 0:
            report.add("schema", "%s must be a number >= 0" % key, label)


def check_scene_holds(scene, label, report):
    if "duration_override_reason" in scene and not isinstance(scene["duration_override_reason"], str):
        report.add("schema", "duration_override_reason must be a string when present", label)
    if "static_hold" in scene and not isinstance(scene["static_hold"], bool):
        report.add("schema", "static_hold must be a boolean when present", label)
    if scene.get("static_hold") is True and not non_empty_str(scene.get("static_hold_reason")):
        report.add("schema", "static_hold true needs a non-empty static_hold_reason string", label)
    if "static_hold_reason" in scene and not isinstance(scene["static_hold_reason"], str):
        report.add("schema", "static_hold_reason must be a string when present", label)


def check_scene_text(scene, label, report):
    text = scene.get("on_screen_text", [])
    if not (isinstance(text, list) and all(non_empty_str(t) for t in text)):
        report.add("schema", "on_screen_text must be a list of non-empty strings when present", label)
    for key in ("narration", "caption"):
        if not isinstance(scene.get(key), str):
            report.add("schema", "%s must be a string (empty allowed)" % key, label)
    for key in ("plate", "transition_out"):
        if not non_empty_str(scene.get(key)):
            report.add("schema", "%s must be a non-empty string" % key, label)


def check_scene_layout(scene, label, report):
    beat_in, beat_out = scene.get("beat_in"), scene.get("beat_out")
    if not (is_int(beat_in) and is_int(beat_out) and 0 <= beat_in <= beat_out):
        report.add("schema", "beat_in and beat_out must be integers with 0 <= beat_in <= beat_out", label)
    motion = scene.get("motion")
    if not isinstance(motion, dict) or not non_empty_str(motion.get("template")):
        report.add("schema", "motion must be an object with a non-empty template", label)
    elif "reveal_sec" in motion and not at_least(motion, "reveal_sec", 0):
        report.add("schema", "motion.reveal_sec must be a number >= 0 when present", label)
    regions = scene.get("critical_regions")
    if not isinstance(regions, list) or not all(check_box(r, 4) for r in regions):
        report.add("schema", "critical_regions must be a list of [x, y, w, h] number lists", label)
    if not is_normalized_point(scene.get("focal_point")):
        report.add("schema", "focal_point must be [x, y] with values in [0.0, 1.0]", label)


def is_normalized_point(value):
    return check_box(value, 2) and all(0.0 <= v <= 1.0 for v in value)


def at_least(value, key, floor):
    return is_number(value[key]) and value[key] >= floor


def positive(value, key):
    return is_number(value[key]) and value[key] > 0


def in_scene(sec, duration):
    return is_number(sec) and is_number(duration) and 0.0 <= sec <= duration


def valid_reframe(value, duration):
    if not has_exact_fields(value, ("scale", "focal_point", "from_sec", "to_sec")):
        return False
    start, end = value["from_sec"], value["to_sec"]
    if not (in_scene(start, duration) and in_scene(end, duration) and start < end):
        return False
    return positive(value, "scale") and is_normalized_point(value["focal_point"])


def valid_gauge_inset(value, duration):
    if not has_exact_fields(value, ("source_rect", "scale", "event_sec", "lead_sec", "tail_sec", "anchor")):
        return False
    if not (is_normalized_rect(value["source_rect"]) and positive(value, "scale")):
        return False
    if not (at_least(value, "lead_sec", GAUGE_LEAD_MIN_SEC) and at_least(value, "tail_sec", GAUGE_TAIL_MIN_SEC)):
        return False
    return in_scene(value["event_sec"], duration) and non_empty_str(value["anchor"])


def valid_plate_hold(value, duration):
    if not has_exact_fields(value, ("at_sec", "duration_sec")):
        return False
    if not (at_least(value, "at_sec", 0.0) and positive(value, "duration_sec")):
        return False
    return in_scene(value["at_sec"] + value["duration_sec"], duration)


def valid_locked_actions(value, duration):
    return isinstance(value, list) and all(in_scene(sec, duration) for sec in value)


def valid_rect(value, _duration):
    return is_normalized_rect(value)


OPTIONAL_SCENE_FIELDS = (
    ("reframe", valid_reframe,
     "reframe must have positive scale, normalized focal_point and 0 <= from_sec < to_sec <= duration_sec"),
    ("gauge_inset", valid_gauge_inset,
     "gauge_inset needs a normalized source_rect, positive scale, in-scene event_sec, lead_sec >= 0.3, tail_sec >= 1.2 and anchor"),
    ("plate_hold", valid_plate_hold,
     "plate_hold needs 0 <= at_sec and positive duration_sec inside the scene"),
    ("hud_crop", valid_rect, "hud_crop must be a normalized [x, y, w, h] rectangle"),
    ("caption_box", valid_rect, "caption_box must be a normalized [x, y, w, h] rectangle"),
    ("locked_actions_sec", valid_locked_actions,
     "locked_actions_sec must be a list of scene-relative seconds inside the scene"),
)


def check_optional_scene_fields(scene, label, report):
    duration = scene.get("duration_sec")
    for key, valid, message in OPTIONAL_SCENE_FIELDS:
        if key in scene and not valid(scene[key], duration):
            report.add("schema", message, label)


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
    if "rhythm" in timeline:
        check_rhythm_schema(timeline["rhythm"], timeline.get("meta"), report)


def check_rhythm_schema(rhythm, meta, report):
    if not (isinstance(meta, dict) and "bpm" in meta):
        report.add("schema", "rhythm needs meta.bpm (the bar length)")
    if not isinstance(rhythm, list):
        report.add("schema", "rhythm must be a list of stretches")
        return
    for index, item in enumerate(rhythm):
        where = "rhythm[%d]" % index
        if not isinstance(item, dict):
            report.add("schema", "%s must be an object" % where)
            continue
        if not all(is_number(item.get(k)) for k in ("start_sec", "end_sec", "energy")):
            report.add("schema", "%s needs numbers start_sec, end_sec and energy" % where)
        events = item.get("events_sec", [])
        if not (isinstance(events, list) and all(is_number(e) for e in events)):
            report.add("schema", "%s.events_sec must be a list of numbers when present" % where)
        if "stop" in item and not isinstance(item["stop"], bool):
            report.add("schema", "%s.stop must be a boolean when present" % where)


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


def check_caption_place(meta, scenes, report):
    aspect = meta["aspect"]
    for scene in scenes:
        box = scene.get("caption_box")
        if box is None:
            continue
        x, y, width, height = box
        if aspect == "16:9" and (y < CAPTION_BAND_TOP or y + height > CAPTION_BAND_BOTTOM):
            report.add(
                "caption_place",
                "caption_box y range %.3f to %.3f must stay in the 16:9 safe band %.2f to %.2f"
                % (y, y + height, CAPTION_BAND_TOP, CAPTION_BAND_BOTTOM),
                scene["id"],
            )
        if aspect == "9:16" and (y + height > VERTICAL_CAPTION_BOTTOM or x + width > VERTICAL_CAPTION_RIGHT):
            report.add(
                "caption_place",
                "caption_box must stay above y = %.2f and left of x = %.2f in 9:16"
                % (VERTICAL_CAPTION_BOTTOM, VERTICAL_CAPTION_RIGHT),
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


def distance_to_beat(value, beat_sec):
    return abs(value - round(value / beat_sec) * beat_sec)


def check_sync(meta, scenes, report):
    if "bpm" not in meta:
        return
    beat_sec = 60.0 / meta["bpm"]
    for scene in scenes[1:]:
        distance = distance_to_beat(scene["start_sec"], beat_sec)
        if distance > CUT_SYNC_SEC:
            report.add(
                "sync",
                "cut at %.3f s is %.3f s from the beat; the limit is %.3f s"
                % (scene["start_sec"], distance, CUT_SYNC_SEC),
                scene["id"],
            )
    for scene in scenes:
        for action_sec in scene.get("locked_actions_sec", []):
            film_sec = scene["start_sec"] + action_sec
            distance = distance_to_beat(film_sec, beat_sec)
            if distance > ACTION_SYNC_SEC:
                report.add(
                    "sync",
                    "locked action at %.3f s is %.3f s from the beat; the limit is %.3f s"
                    % (film_sec, distance, ACTION_SYNC_SEC),
                    scene["id"],
                )


def shown_text(scene):
    texts = [scene["caption"]] if scene["caption"].strip() else []
    return texts + scene.get("on_screen_text", [])


def check_read_hold(scenes, report):
    for scene in scenes:
        hold = scene["duration_sec"] - scene["motion"].get("reveal_sec", 0.0)
        for text in shown_text(scene):
            words = word_count(text)
            need = max(READ_MIN_SEC, READ_BASE_SEC + READ_PER_WORD_SEC * words)
            if hold + START_TOLERANCE_SEC < need:
                report.add(
                    "read_hold",
                    "%r (%d words) holds %.2f s after the reveal; it needs %.2f s" % (text, words, hold, need),
                    scene["id"],
                )


def normalized(text):
    return " " + " ".join(re.findall(r"\w+", text.lower())) + " "


def check_terms(meta, scenes, report):
    said = "".join(normalized(scene["narration"]) for scene in scenes)
    shown = "".join(normalized(text) for scene in scenes for text in shown_text(scene))
    for term in meta.get("terms", []):
        key = normalized(term)
        missing = [where for where, text in (("said", said), ("shown", shown)) if key not in text]
        if missing:
            report.add("terms", "term %r is not %s" % (term, " or ".join(missing)))


def check_rhythm(meta, rhythm, report):
    if "bpm" in meta and not rhythm:
        report.add("rhythm", "a film with music (meta.bpm) needs a rhythm sheet")
    previous_end = 0.0
    for index, item in enumerate(rhythm):
        where = "rhythm[%d]" % index
        start, end, energy = item["start_sec"], item["end_sec"], item["energy"]
        if not (is_int(energy) and ENERGY_MIN <= energy <= ENERGY_MAX):
            report.add("rhythm", "%s.energy %s must be an integer %d to %d" % (where, energy, ENERGY_MIN, ENERGY_MAX))
        if start + START_TOLERANCE_SEC < previous_end or end <= start or end > meta["duration_sec"] + TOTAL_TOLERANCE_SEC:
            report.add("rhythm", "%s [%.3f, %.3f] s overlaps the stretch before or leaves the film" % (where, start, end))
        if any(not start <= event <= end for event in item.get("events_sec", [])):
            report.add("rhythm", "%s has an event outside its stretch" % where)
        previous_end = max(previous_end, end)


def open_pieces(start, end, stops):
    """The parts of [start, end] that no stop range covers."""
    pieces = [(start, end)]
    for stop_start, stop_end in stops:
        cut = []
        for a, b in pieces:
            if stop_end <= a or stop_start >= b:
                cut.append((a, b))
                continue
            if stop_start > a:
                cut.append((a, stop_start))
            if stop_end < b:
                cut.append((stop_end, b))
        pieces = cut
    return pieces


def check_event_gap(meta, scenes, rhythm, report):
    if "bpm" not in meta:
        return
    limit = MAX_GAP_BARS * BEATS_PER_BAR * 60.0 / meta["bpm"]
    events = {0.0, meta["duration_sec"]} | {scene["start_sec"] for scene in scenes}
    events |= {event for item in rhythm for event in item.get("events_sec", [])}
    stops = [(item["start_sec"], item["end_sec"]) for item in rhythm if item.get("stop")]
    if "payoff_silence_sec" in meta:
        stops.append(tuple(meta["payoff_silence_sec"]))
    ordered = sorted(events)
    for a, b in zip(ordered, ordered[1:]):
        for start, end in open_pieces(a, b, stops):
            if end - start > limit + START_TOLERANCE_SEC:
                report.add(
                    "event_gap",
                    "%.3f s to %.3f s has no event for %.2f s; the limit is %d bars (%.2f s) outside a declared stop"
                    % (start, end, end - start, MAX_GAP_BARS, limit),
                )


def check_hero(meta, report):
    duration = meta["duration_sec"]
    hero = meta.get("hero_sec")
    if hero is None:
        if "bpm" in meta:
            report.add("hero", "a film with music (meta.bpm) needs meta.hero_sec")
        return
    if hero[0] < HERO_EARLIEST * duration or hero[1] > HERO_LATEST * duration:
        report.add(
            "hero",
            "meta.hero_sec [%.3f, %.3f] s must sit inside %.3f s to %.3f s (30 %% to 90 %% of the film)"
            % (hero[0], hero[1], HERO_EARLIEST * duration, HERO_LATEST * duration),
        )
    silence = meta.get("payoff_silence_sec")
    if silence is None:
        if "bpm" in meta:
            report.add("hero", "a film with music (meta.bpm) needs meta.payoff_silence_sec before the hero")
    elif silence[1] > hero[0] + START_TOLERANCE_SEC:
        report.add("hero", "meta.payoff_silence_sec ends at %.3f s, after the hero start %.3f s" % (silence[1], hero[0]))


def check_menu_time(meta, scenes, report):
    menu_sec = sum(scene["duration_sec"] for scene in scenes if scene["kind"] in ("ui", "menu"))
    limit = meta["duration_sec"] * MENU_SEC_PER_FILM_SEC
    if menu_sec > limit:
        report.add(
            "menu_time",
            "ui and menu scenes total %.3f s; the scaled limit is %.3f s (8 s per 80 s of film)"
            % (menu_sec, limit),
        )


def direction_blocks(text):
    """Map each `### D<n>` heading under `## Directions` to its `- Key: value` lines."""
    section = re.search(r"^## Directions[ \t]*$(.*?)(?=^## |\Z)", text, re.M | re.S)
    blocks = {}
    for match in re.finditer(r"^### (D\d+)\b[^\n]*\n(.*?)(?=^### |\Z)", section.group(1) if section else "", re.M | re.S):
        fields = dict(re.findall(r"^- ([A-Za-z ]+?):[ \t]*(.*?)[ \t]*$", match.group(2), re.M))
        blocks[match.group(1)] = {key.lower(): value for key, value in fields.items()}
    return blocks


def check_direction_block(folder, code, fields, report):
    for key in ("motion language", "material", "palette source"):
        if not fields.get(key):
            report.add("directions", "%s needs a '- %s:' line" % (code, key.capitalize()))
    if not HEX_COLOR.search(fields.get("palette", "")):
        report.add("directions", "%s needs a '- Palette:' line with hex colors" % code)
    frame = fields.get("key frame", "")
    path = os.path.join(folder, frame)
    if not (frame.startswith("qa/directions/") and frame.endswith(".png") and os.path.isfile(path)):
        report.add("directions", "%s key frame %r is not a file under qa/directions/" % (code, frame))
        return
    with open(path, "rb") as handle:
        if handle.read(len(PNG_SIGNATURE)) != PNG_SIGNATURE:
            report.add("directions", "%s key frame %s is not a PNG" % (code, frame))


def check_directions(folder):
    """Return the `directions` errors for DIRECTION.md and its key frames in `folder`."""
    report = Report()
    path = os.path.join(folder, "DIRECTION.md")
    try:
        with open(path, encoding="utf-8") as handle:
            text = handle.read()
    except OSError as exc:
        report.add("directions", "cannot read DIRECTION.md: %s" % exc)
        return report.errors
    blocks = direction_blocks(text)
    if len(blocks) < MIN_DIRECTIONS:
        report.add("directions", "DIRECTION.md lists %d directions under '## Directions'; it needs %d" % (len(blocks), MIN_DIRECTIONS))
    for code, fields in blocks.items():
        check_direction_block(folder, code, fields, report)
    frames = {fields.get("key frame") for fields in blocks.values()}
    if len(frames) < len(blocks):
        report.add("directions", "each direction needs its own key frame")
    depth = [code for code, fields in blocks.items() if fields.get("depth", "").lower() == "yes"]
    if len(depth) != 1:
        report.add("directions", "exactly 1 direction has 'Depth: yes'; found %d" % len(depth))
    looks = {(f.get("motion language", "").lower(), f.get("material", "").lower()) for f in blocks.values()}
    if len(looks) < len(blocks):
        report.add("directions", "2 directions share both motion language and material")
    chosen = re.findall(r"^chosen:[ \t]*(D\d+)\b", text, re.M)
    if len(chosen) != 1 or chosen[0] not in blocks:
        report.add("directions", "DIRECTION.md needs 1 'chosen: D<n>' line that names a listed direction")
    return report.errors


def listed_materials(text):
    """File names in the `| File | Source URL | What |` table of materials.md."""
    names = []
    for line in text.splitlines():
        cells = [cell.strip().strip("`") for cell in line.strip().strip("|").split("|")]
        if line.startswith("|") and len(cells) == 3 and cells[0] not in ("", "File") and set(cells[0]) != {"-"}:
            names.append(cells[0])
    return names


def check_materials(timeline, folder):
    """Return the `materials` errors: a capture scene needs materials/materials.md and every file it lists."""
    report = Report()
    scenes = timeline.get("scenes") if isinstance(timeline, dict) else None
    captures = [s.get("id") for s in scenes or [] if isinstance(s, dict) and s.get("kind") == "capture"]
    if not captures:
        return report.errors
    path = os.path.join(folder, "materials", "materials.md")
    try:
        with open(path, encoding="utf-8") as handle:
            names = listed_materials(handle.read())
    except OSError:
        report.add("materials", "capture scenes %s need materials/materials.md (video-capture materials.mjs)" % ", ".join(captures))
        return report.errors
    for name in names:
        if not os.path.isfile(os.path.join(folder, "materials", name)):
            report.add("materials", "materials/materials.md lists %s, but the file is missing" % name)
    return report.errors


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
    check_caption_place(meta, scenes, report)
    check_speaking_rate(scenes, report)
    check_variety(scenes, report)
    check_safe_zone(meta, scenes, report)
    check_timing(meta, scenes, report)
    check_sync(meta, scenes, report)
    rhythm = timeline.get("rhythm", [])
    check_read_hold(scenes, report)
    check_terms(meta, scenes, report)
    check_rhythm(meta, rhythm, report)
    check_event_gap(meta, scenes, rhythm, report)
    check_hero(meta, report)
    check_menu_time(meta, scenes, report)
    return report.errors


def run(timeline_path, out_path):
    timeline = None
    try:
        with open(timeline_path, encoding="utf-8") as handle:
            timeline = json.load(handle)
        errors = check_timeline(timeline)
    except (OSError, ValueError) as exc:
        errors = [{"rule": "schema", "message": "cannot read timeline: %s" % exc}]
    folder = os.path.dirname(os.path.abspath(timeline_path))
    errors += check_directions(folder) + check_materials(timeline, folder)
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

    def tiny_png(width=4, height=4):
        import struct
        import zlib

        def chunk(kind, data):
            body = kind + data
            return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

        rows = b"".join(b"\x00" + b"\x80\x80\x80" * width for _ in range(height))
        header = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
        return PNG_SIGNATURE + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b"")

    def direction_md(chosen="D2"):
        blocks = [
            ("D1", "Calm drift", "slow springs and long holds", "paper cut-outs", "no", "#F4F1EA #1D1F24 #E4572E"),
            ("D2", "Hard cuts", "snap cuts on every downbeat", "flat UI captures", "no", "#111318 #F5F5F2 #4F7CFF"),
            ("D3", "Glass stack", "parallax camera push", "layered glass panels", "yes", "#0B1020 #E8ECF8 #7CD4FF"),
        ]
        lines = ["# Direction", "", "## Directions", ""]
        for code, name, motion, material, depth, colors in blocks:
            lines += [
                "### %s: %s" % (code, name),
                "- Motion language: %s" % motion,
                "- Material: %s" % material,
                "- Depth: %s" % depth,
                "- Key frame: qa/directions/%s.png" % code.lower(),
                "- Palette: %s" % colors,
                "- Palette source: BRAND.md color tokens",
                "",
            ]
        if chosen:
            lines.append("chosen: %s" % chosen)
        return "\n".join(lines) + "\n"

    def write_materials(folder, listed=("first-screen.png", "colors.json"), present=("first-screen.png", "colors.json")):
        os.makedirs(os.path.join(folder, "materials"), exist_ok=True)
        rows = "".join("| `%s` | http://localhost:8000/ | fixture |\n" % name for name in listed)
        with open(os.path.join(folder, "materials", "materials.md"), "w", encoding="utf-8") as handle:
            handle.write("# Materials\n\n| File | Source URL | What |\n|---|---|---|\n" + rows)
        for name in present:
            with open(os.path.join(folder, "materials", name), "wb") as handle:
                handle.write(b"x")

    def write_project(folder, timeline, direction=None, pngs=("d1", "d2", "d3")):
        with open(os.path.join(folder, "timeline.json"), "w", encoding="utf-8") as handle:
            json.dump(timeline, handle)
        write_materials(folder)
        with open(os.path.join(folder, "DIRECTION.md"), "w", encoding="utf-8") as handle:
            handle.write(direction_md() if direction is None else direction)
        os.makedirs(os.path.join(folder, "qa", "directions"), exist_ok=True)
        for name in pngs:
            with open(os.path.join(folder, "qa", "directions", name + ".png"), "wb") as handle:
                handle.write(tiny_png())
        return os.path.join(folder, "timeline.json")

    def directions_rules(direction=None, pngs=("d1", "d2", "d3")):
        with tempfile.TemporaryDirectory() as tmp:
            write_project(tmp, valid_timeline(), direction, pngs)
            return {error["rule"] for error in check_directions(tmp)}

    def music_timeline():
        """valid_timeline() with music: bpm 120 (1 bar = 2 s), rhythm, hero, silence and terms."""
        timeline = valid_timeline()
        timeline["meta"].update({
            "bpm": 120,
            "terms": ["flaky tests"],
            "hero_sec": [12.0, 13.5],
            "payoff_silence_sec": [11.5, 12.0],
        })
        timeline["scenes"][0]["narration"] = "Flaky tests waste hours."
        timeline["scenes"][0]["caption"] = "Stop flaky tests"
        timeline["rhythm"] = [
            {"start_sec": 0.0, "end_sec": 2.5, "energy": 8},
            {"start_sec": 2.5, "end_sec": 7.5, "energy": 5, "events_sec": [5.0]},
            {"start_sec": 7.5, "end_sec": 11.5, "energy": 6, "events_sec": [10.0]},
            {"start_sec": 11.5, "end_sec": 12.0, "energy": 0, "stop": True},
            {"start_sec": 12.0, "end_sec": 15.0, "energy": 10},
        ]
        return timeline

    class CheckTimelineTest(unittest.TestCase):
        def setUp(self):
            self.timeline = valid_timeline()
            self.scenes = self.timeline["scenes"]

        def assert_cli_fails_with(self, rule):
            with tempfile.TemporaryDirectory() as tmp:
                path = write_project(tmp, self.timeline)
                proc = subprocess.run([sys.executable, script, path], capture_output=True, text=True, timeout=60)
                self.assertNotEqual(proc.returncode, 0, proc.stdout)
                with open(os.path.join(tmp, "qa", "plan-check.json"), encoding="utf-8") as handle:
                    report = json.load(handle)
            self.assertFalse(report["passed"])
            self.assertIn(rule, {error["rule"] for error in report["errors"]})

        def test_valid_timeline_passes_and_writes_report(self):
            with tempfile.TemporaryDirectory() as tmp:
                path = write_project(tmp, self.timeline)
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
                path = write_project(tmp, self.timeline)
                out = os.path.join(tmp, "reports", "check.json")
                proc = subprocess.run([sys.executable, script, path, "--out", out],
                                      capture_output=True, text=True, timeout=60)
                self.assertEqual(proc.returncode, 0, proc.stdout)
                self.assertTrue(os.path.isfile(out))

        def test_1_hook_window(self):
            # The caption still holds 2.0 s after the late reveal, so only the hook fails.
            self.scenes[0]["motion"]["reveal_sec"] = 3.2
            self.scenes[0]["duration_sec"] = 5.2
            self.scenes[1]["start_sec"] = 5.2
            self.scenes[1]["duration_sec"] = 2.3
            self.scenes[1]["narration"] = "Run one command."
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

        def test_3_caption_place_pass_and_fail(self):
            self.scenes[1]["caption_box"] = [0.1, 0.85, 0.8, 0.08]
            self.assertEqual(rules_of(self.timeline), set())
            self.scenes[1]["caption_box"] = [0.1, 0.75, 0.8, 0.08]
            self.assertEqual(rules_of(self.timeline), {"caption_place"})

        def test_3_caption_place_vertical_pass_and_fail(self):
            meta = self.timeline["meta"]
            meta.update({"aspect": "9:16", "width": 1080, "height": 1920})
            self.scenes[1]["caption_box"] = [0.1, 0.7, 0.7, 0.1]
            self.assertEqual(rules_of(self.timeline), set())
            self.scenes[1]["caption_box"] = [0.2, 0.7, 0.7, 0.1]
            self.assertEqual(rules_of(self.timeline), {"caption_place"})

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
            self.scenes[2]["static_hold_reason"] = "A planned end-card hold."
            self.assertEqual(rules_of(self.timeline), set())
            self.scenes[2]["static_hold"] = "yes"
            self.assertEqual(rules_of(self.timeline), {"schema"})

        def test_8_optional_scene_fields_pass(self):
            self.scenes[1].update({
                "reframe": {"scale": 1.5, "focal_point": [0.5, 0.4], "from_sec": 0.0, "to_sec": 1.2},
                "gauge_inset": {"source_rect": [0.7, 0.05, 0.2, 0.12], "scale": 2.0,
                                "event_sec": 1.1, "lead_sec": 0.3, "tail_sec": 1.2, "anchor": "top-right"},
                "plate_hold": {"at_sec": 2.0, "duration_sec": 0.5},
                "hud_crop": [0.72, 0.0, 0.28, 0.18],
                "caption_box": [0.1, 0.85, 0.8, 0.08],
                "locked_actions_sec": [0.5, 1.5],
            })
            self.assertEqual(rules_of(self.timeline), set())

        def test_8_optional_scene_fields_fail(self):
            bad_values = {
                "reframe": {"scale": 1.5, "focal_point": [1.5, 0.4], "from_sec": 0.0, "to_sec": 1.2},
                "gauge_inset": {"source_rect": [0.7, 0.05, 0.2, 0.12], "scale": 2.0,
                                "event_sec": 1.1, "lead_sec": 0.2, "tail_sec": 1.2, "anchor": "top-right"},
                "plate_hold": {"at_sec": 4.9, "duration_sec": 0.5},
                "hud_crop": [0.9, 0.0, 0.2, 0.2],
                "caption_box": [0.9, 0.8, 0.2, 0.1],
                "locked_actions_sec": [5.1],
            }
            for key, value in bad_values.items():
                timeline = valid_timeline()
                timeline["scenes"][1][key] = value
                self.assertEqual(rules_of(timeline), {"schema"}, key)
            timeline = valid_timeline()
            timeline["scenes"][1]["static_hold"] = True
            self.assertEqual(rules_of(timeline), {"schema"}, "static_hold_reason")
            timeline = valid_timeline()
            timeline["scenes"][1]["gauge_inset"] = {
                "source_rect": [0.7, 0.05, 0.2, 0.12], "scale": 2.0,
                "event_sec": 1.1, "lead_sec": 0.3, "tail_sec": 1.1, "anchor": "top-right",
            }
            self.assertEqual(rules_of(timeline), {"schema"}, "gauge_inset.tail_sec")

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

        def test_9_read_hold(self):
            # 4 words need max(1.5, 1.0 + 4 x 0.3) = 2.2 s after the reveal.
            self.scenes[1]["motion"]["reveal_sec"] = 3.0
            self.assertEqual(rules_of(self.timeline), {"read_hold"})
            self.assert_cli_fails_with("read_hold")

        def test_9_read_hold_counts_on_screen_text(self):
            self.scenes[2]["on_screen_text"] = ["Rerun", "Compare every result"]
            self.scenes[2]["motion"]["reveal_sec"] = 2.4
            self.assertEqual(rules_of(self.timeline), set())
            self.scenes[2]["motion"]["reveal_sec"] = 2.7
            self.assertEqual(rules_of(self.timeline), {"read_hold"})

        def test_9_read_hold_floor_is_1_5_s(self):
            self.scenes[3]["caption"] = "Go"
            self.scenes[3]["motion"]["reveal_sec"] = 1.6
            self.assertEqual(rules_of(self.timeline), {"read_hold"})

        def test_10_terms(self):
            self.timeline["meta"]["terms"] = ["flaky tests", "one command"]
            self.assertEqual(rules_of(self.timeline), set())
            self.timeline["meta"]["terms"] = ["rerun"]
            self.assertEqual(rules_of(self.timeline), {"terms"})
            self.assert_cli_fails_with("terms")

        def test_10_term_said_but_not_shown(self):
            self.timeline["meta"]["terms"] = ["hours"]
            self.assertEqual(rules_of(self.timeline), {"terms"})
            self.scenes[2]["on_screen_text"] = ["Hours saved"]
            self.assertEqual(rules_of(self.timeline), set())

        def test_10_term_matches_whole_words(self):
            self.timeline["meta"]["terms"] = ["test"]
            self.assertEqual(rules_of(self.timeline), {"terms"})

        def test_11_music_timeline_passes(self):
            self.timeline = music_timeline()
            self.assertEqual(rules_of(self.timeline), set())
            with tempfile.TemporaryDirectory() as tmp:
                proc = subprocess.run([sys.executable, script, write_project(tmp, self.timeline)],
                                      capture_output=True, text=True, timeout=60)
            self.assertEqual(proc.returncode, 0, proc.stdout)

        def test_11_sync_cut_pass_and_fail(self):
            self.timeline = music_timeline()
            self.assertNotIn("sync", rules_of(self.timeline))
            self.scenes = self.timeline["scenes"]
            self.scenes[0]["duration_sec"] = 2.519
            self.scenes[1]["start_sec"] = 2.519
            self.scenes[1]["duration_sec"] = 4.981
            self.assertNotIn("sync", rules_of(self.timeline))
            self.scenes[0]["duration_sec"] = 2.021
            self.scenes[1]["start_sec"] = 2.021
            self.scenes[1]["duration_sec"] = 5.479
            self.assertEqual(rules_of(self.timeline), {"sync"})

        def test_11_sync_locked_action_pass_and_fail(self):
            self.timeline = music_timeline()
            self.timeline["scenes"][1]["locked_actions_sec"] = [0.068]
            self.assertNotIn("sync", rules_of(self.timeline))
            self.timeline["scenes"][1]["locked_actions_sec"] = [0.072]
            self.assertEqual(rules_of(self.timeline), {"sync"})

        def test_11_menu_time_pass_and_fail(self):
            self.assertNotIn("menu_time", rules_of(self.timeline))
            self.scenes[1]["kind"] = "ui"
            self.assertEqual(rules_of(self.timeline), {"menu_time"})

        def test_11_rhythm_energy_range(self):
            self.timeline = music_timeline()
            self.timeline["rhythm"][1]["energy"] = 11
            self.assertEqual(rules_of(self.timeline), {"rhythm"})
            self.assert_cli_fails_with("rhythm")

        def test_11_rhythm_overlap(self):
            self.timeline = music_timeline()
            self.timeline["rhythm"][2]["start_sec"] = 7.0
            self.assertEqual(rules_of(self.timeline), {"rhythm"})

        def test_11_rhythm_needed_with_music(self):
            self.timeline = music_timeline()
            del self.timeline["rhythm"]
            # The rhythm events go too, so the 5.0 s scene also breaks event_gap.
            self.assertIn("rhythm", rules_of(self.timeline))

        def test_12_event_gap(self):
            # bpm 120: 2 bars = 4.0 s. s02-demo runs 5.0 s; its mid event is removed.
            self.timeline = music_timeline()
            del self.timeline["rhythm"][1]["events_sec"]
            self.assertEqual(rules_of(self.timeline), {"event_gap"})
            self.assert_cli_fails_with("event_gap")

        def test_12_event_gap_skips_a_declared_stop(self):
            self.timeline = music_timeline()
            del self.timeline["rhythm"][1]["events_sec"]
            self.timeline["rhythm"][1]["stop"] = True
            self.assertEqual(rules_of(self.timeline), set())

        def test_13_hero_window(self):
            # 30 % of 15 s is 4.5 s, 90 % is 13.5 s.
            self.timeline = music_timeline()
            self.timeline["meta"]["hero_sec"] = [13.0, 14.0]
            self.timeline["meta"]["payoff_silence_sec"] = [12.5, 13.0]
            self.timeline["rhythm"][3:] = [
                {"start_sec": 11.5, "end_sec": 12.5, "energy": 4},
                {"start_sec": 12.5, "end_sec": 13.0, "energy": 0, "stop": True},
                {"start_sec": 13.0, "end_sec": 15.0, "energy": 10},
            ]
            self.assertEqual(rules_of(self.timeline), {"hero"})
            self.assert_cli_fails_with("hero")

        def test_13_hero_needs_silence_with_music(self):
            self.timeline = music_timeline()
            del self.timeline["meta"]["payoff_silence_sec"]
            self.timeline["rhythm"][3]["stop"] = False
            self.timeline["rhythm"][3]["events_sec"] = [11.75]
            self.assertEqual(rules_of(self.timeline), {"hero"})

        def test_13_silence_must_end_by_hero_start(self):
            self.timeline = music_timeline()
            self.timeline["meta"]["payoff_silence_sec"] = [11.5, 12.5]
            self.assertEqual(rules_of(self.timeline), {"hero"})

        def test_13_hero_without_music_needs_no_silence(self):
            self.timeline["meta"]["hero_sec"] = [12.0, 13.5]
            self.assertEqual(rules_of(self.timeline), set())

        def test_13_hero_needed_with_music(self):
            self.timeline = music_timeline()
            del self.timeline["meta"]["hero_sec"]
            del self.timeline["meta"]["payoff_silence_sec"]
            self.timeline["rhythm"][3]["stop"] = False
            self.timeline["rhythm"][3]["events_sec"] = [11.75]
            self.assertEqual(rules_of(self.timeline), {"hero"})

        def test_14_schema_of_new_fields(self):
            for key, value in (("bpm", 0), ("terms", "flaky"), ("hero_sec", [3.0]),
                               ("payoff_silence_sec", [1.0, 0.5])):
                timeline = music_timeline()
                timeline["meta"][key] = value
                self.assertEqual(rules_of(timeline), {"schema"}, key)
            timeline = valid_timeline()
            timeline["rhythm"] = [{"start_sec": 0.0, "end_sec": 2.0, "energy": 3}]
            self.assertEqual(rules_of(timeline), {"schema"}, "rhythm without meta.bpm")
            timeline = valid_timeline()
            timeline["scenes"][0]["on_screen_text"] = "Stop"
            self.assertEqual(rules_of(timeline), {"schema"}, "on_screen_text")

        def test_15_directions_pass(self):
            self.assertEqual(directions_rules(), set())

        def test_15_directions_fewer_than_3(self):
            text = direction_md().split("### D3")[0] + "chosen: D2\n"
            self.assertEqual(directions_rules(text), {"directions"})

        def test_15_directions_missing_png(self):
            self.assertEqual(directions_rules(pngs=("d1", "d2")), {"directions"})

        def test_15_directions_need_exactly_1_with_depth(self):
            self.assertEqual(directions_rules(direction_md().replace("Depth: no", "Depth: yes", 1)), {"directions"})

        def test_15_directions_must_differ(self):
            text = direction_md().replace("snap cuts on every downbeat", "slow springs and long holds")
            text = text.replace("flat UI captures", "paper cut-outs")
            self.assertEqual(directions_rules(text), {"directions"})

        def test_15_directions_need_palette_source(self):
            text = direction_md().replace("- Palette source: BRAND.md color tokens\n", "", 1)
            self.assertEqual(directions_rules(text), {"directions"})

        def test_15_directions_need_chosen_line(self):
            self.assertEqual(directions_rules(direction_md(chosen=None)), {"directions"})
            self.assertEqual(directions_rules(direction_md(chosen="D7")), {"directions"})

        def test_15_cli_fails_without_direction_file(self):
            with tempfile.TemporaryDirectory() as tmp:
                path = write_project(tmp, self.timeline)
                os.remove(os.path.join(tmp, "DIRECTION.md"))
                proc = subprocess.run([sys.executable, script, path], capture_output=True, text=True, timeout=60)
            self.assertEqual(proc.returncode, 1)
            self.assertEqual({e["rule"] for e in json.loads(proc.stdout)["errors"]}, {"directions"})

        def test_16_materials_pass(self):
            with tempfile.TemporaryDirectory() as tmp:
                write_materials(tmp)
                self.assertEqual(check_materials(self.timeline, tmp), [])

        def test_16_capture_scene_needs_materials_list(self):
            with tempfile.TemporaryDirectory() as tmp:
                path = write_project(tmp, self.timeline)
                os.remove(os.path.join(tmp, "materials", "materials.md"))
                proc = subprocess.run([sys.executable, script, path], capture_output=True, text=True, timeout=60)
            self.assertEqual(proc.returncode, 1)
            self.assertEqual({e["rule"] for e in json.loads(proc.stdout)["errors"]}, {"materials"})

        def test_16_listed_material_is_missing(self):
            with tempfile.TemporaryDirectory() as tmp:
                write_materials(tmp, present=("colors.json",))
                errors = check_materials(self.timeline, tmp)
            self.assertEqual([e["rule"] for e in errors], ["materials"])
            self.assertIn("first-screen.png", errors[0]["message"])

        def test_16_no_capture_scene_needs_no_materials(self):
            self.scenes[1]["kind"] = "motion"
            with tempfile.TemporaryDirectory() as tmp:
                self.assertEqual(check_materials(self.timeline, tmp), [])

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
