#!/usr/bin/env python3
"""Blind A/B labels for 2 masters of 1 brief, and the checks around them.

Usage:
  ab_blind.py assign --seed <int> --map .video/ab-labels.json --out-dir render/ab \\
              <direction>=<master.mp4> <direction>=<master.mp4>
  ab_blind.py lint-plan --map .video/ab-labels.json <critic-plan.md>
  ab_blind.py check-rank qa/ab-rank.md [--map .video/ab-labels.json]
  ab_blind.py --self-test

assign shuffles the 2 directions with random.Random(seed), copies the masters
to <out-dir>/A.mp4 and <out-dir>/B.mp4, and writes the label map. Only the
producer reads the map. The same seed always gives the same labels.

lint-plan fails when the ranker's plan names a direction, a source path or
the .video/ folder. The ranker sees only A and B.

check-rank validates the ranker's report: a "PREFERENCE: A" or "PREFERENCE: B"
line, 1 table row per rubric axis with the A score, the B score and a margin
that matches them ("A +1", "B +2" or "0"), and a "Weakest shot A:" and
"Weakest shot B:" line, each with an [MM:SS.mmm] time. With --map it also
fails when the report names a direction, and it prints the preferred
direction.

Exit codes: 0 pass, 1 a rule fails, 2 usage or a missing file.
Python standard library only.
"""

import argparse
import json
import os
import random
import re
import shutil
import sys
import tempfile

AXES = (
    "Hook & First-Frame Read",
    "Narrative & Pacing",
    "Typography & Safe Zones",
    "Motion Craft & Physics",
    "Product/Demo Clarity",
    "Character/Illustration Cohesion",
    "Audio-Visual Sync",
    "Brand & Aesthetic Cohesion",
)
LABELS = ("A", "B")
ROW_RE = re.compile(r"^\|\s*\d+\s*\|\s*([^|]+?)\s*\|\s*([1-5])\s*\|\s*([1-5])\s*\|\s*([^|]+?)\s*\|\s*$")
TIME_RE = r"\[\d{2}:\d{2}\.\d{3}\]"


def assign(seed, pairs, out_dir, map_path):
    names = sorted(pairs)
    random.Random(seed).shuffle(names)
    os.makedirs(out_dir, exist_ok=True)
    labels = {}
    for label, name in zip(LABELS, names):
        shutil.copyfile(pairs[name], os.path.join(out_dir, f"{label}.mp4"))
        labels[label] = {"direction": name, "source": pairs[name]}
    os.makedirs(os.path.dirname(os.path.abspath(map_path)), exist_ok=True)
    with open(map_path, "w", encoding="utf-8") as handle:
        json.dump({"seed": seed, "labels": labels}, handle, indent=2)
        handle.write("\n")
    return labels


def secrets(label_map):
    """The words a ranker must never see: direction names, sources, .video/."""
    words = [".video/"]
    for entry in label_map["labels"].values():
        words += [entry["direction"], entry["source"]]
    return words


def leaks(text, label_map):
    found = []
    for word in secrets(label_map):
        pattern = re.escape(word)
        if re.match(r"\w", word):
            pattern = r"(?<![\w-])" + pattern + r"(?![\w-])"
        if re.search(pattern, text, re.I):
            found.append(word)
    return found


def margin_for(a, b):
    if a == b:
        return "0"
    return f"A +{a - b}" if a > b else f"B +{b - a}"


def check_rank(text):
    errors = []
    preference = re.findall(r"^PREFERENCE:\s*(\S+)\s*$", text, re.M)
    if len(preference) != 1 or preference[0] not in LABELS:
        errors.append("needs exactly 1 line 'PREFERENCE: A' or 'PREFERENCE: B'")
    rows = {}
    for line in text.splitlines():
        match = ROW_RE.match(line.strip())
        if match:
            axis, a, b, margin = match.group(1), int(match.group(2)), int(match.group(3)), match.group(4)
            rows[axis] = (a, b, margin)
    for axis in AXES:
        if axis not in rows:
            errors.append(f"no table row for axis '{axis}' with A, B and margin")
            continue
        a, b, margin = rows[axis]
        if margin != margin_for(a, b):
            errors.append(f"axis '{axis}': margin '{margin}' does not match A {a} and B {b} (want '{margin_for(a, b)}')")
    for label in LABELS:
        if not re.search(rf"^Weakest shot {label}:\s*{TIME_RE}\s*\S", text, re.M):
            errors.append(f"needs a 'Weakest shot {label}: [MM:SS.mmm] <scene and issue>' line")
    return (preference[0] if len(preference) == 1 else None), errors


def read(path):
    with open(path, encoding="utf-8") as handle:
        return handle.read()


def run_assign(args):
    pairs = {}
    for item in args.variants:
        name, sep, path = item.partition("=")
        if not sep or not name or not os.path.isfile(path):
            print(f"variant '{item}' must be <direction>=<existing file>", file=sys.stderr)
            return 2
        pairs[name] = path
    if len(pairs) != 2:
        print("assign needs exactly 2 different directions", file=sys.stderr)
        return 2
    labels = assign(args.seed, pairs, args.out_dir, args.map)
    print(f"wrote {args.out_dir}/A.mp4, {args.out_dir}/B.mp4 and {args.map} ({len(labels)} labels, seed {args.seed})")
    return 0


def run_lint(args):
    found = leaks(read(args.plan), json.load(open(args.map, encoding="utf-8")))
    for word in found:
        print(f"{args.plan}: names '{word}'; the ranker sees only A and B")
    print(f"{args.plan}: {'FAIL' if found else 'clean'}")
    return 1 if found else 0


def run_check(args):
    text = read(args.rank)
    preferred, errors = check_rank(text)
    label_map = json.load(open(args.map, encoding="utf-8")) if args.map else None
    if label_map:
        errors += [f"names '{word}'; the report must use only A and B" for word in leaks(text, label_map)]
    for error in errors:
        print(f"{args.rank}: {error}")
    if errors:
        print(f"{args.rank}: FAIL")
        return 1
    if label_map:
        print(f"{args.rank}: PASS, preferred {preferred} = {label_map['labels'][preferred]['direction']}")
    else:
        print(f"{args.rank}: PASS, preferred {preferred}")
    return 0


GOOD_RANK = "\n".join(
    ["# A/B rank", "", "PREFERENCE: B", "", "| # | Axis | A | B | Margin |", "| :--- | :--- | :--- | :--- | :--- |"]
    + [f"| {i} | {axis} | 4 | {5 if i == 1 else 4} | {'B +1' if i == 1 else '0'} |" for i, axis in enumerate(AXES, 1)]
    + ["", "Weakest shot A: [00:01.200] Scene s01-hook: claim appears late", "Weakest shot B: [00:04.000] Scene s02-cta: URL is small", ""]
)


def self_test():
    failures = []
    preferred, errors = check_rank(GOOD_RANK)
    if errors or preferred != "B":
        failures.append(f"clean rank: expected B and no errors, got {preferred} and {errors}")
    bad = {
        "no preference": GOOD_RANK.replace("PREFERENCE: B\n", ""),
        "preference C": GOOD_RANK.replace("PREFERENCE: B", "PREFERENCE: C"),
        "wrong margin": GOOD_RANK.replace("| 5 | B +1 |", "| 5 | A +1 |"),
        "missing axis": "\n".join(line for line in GOOD_RANK.splitlines() if "Audio-Visual Sync" not in line),
        "no weakest shot time": GOOD_RANK.replace("Weakest shot A: [00:01.200]", "Weakest shot A:"),
    }
    for name, text in bad.items():
        if len(check_rank(text)[1]) != 1:
            failures.append(f"{name}: expected 1 error, got {check_rank(text)[1]}")
    with tempfile.TemporaryDirectory() as tmp:
        for name in ("calm", "bold"):
            with open(os.path.join(tmp, f"{name}.mp4"), "w") as handle:
                handle.write(name)
        pairs = {n: os.path.join(tmp, f"{n}.mp4") for n in ("calm", "bold")}
        first = assign(7, pairs, os.path.join(tmp, "ab"), os.path.join(tmp, ".video", "ab-labels.json"))
        again = assign(7, pairs, os.path.join(tmp, "ab2"), os.path.join(tmp, ".video", "ab-labels-2.json"))
        if first != again:
            failures.append("the same seed must give the same labels")
        for label in LABELS:
            if read(os.path.join(tmp, "ab", f"{label}.mp4")) != first[label]["direction"]:
                failures.append(f"{label}.mp4 is not the copy of its direction")
        seeds = {assign(s, pairs, os.path.join(tmp, "ab3"), os.path.join(tmp, "m.json"))["A"]["direction"] for s in range(8)}
        if seeds != {"calm", "bold"}:
            failures.append("8 seeds must give both directions the A label")
        label_map = json.load(open(os.path.join(tmp, ".video", "ab-labels.json")))
        clean_plan = "Rank render/ab/A.mp4 against render/ab/B.mp4 with the ranking mode of the rubric."
        if leaks(clean_plan, label_map):
            failures.append(f"clean plan: unexpected leaks {leaks(clean_plan, label_map)}")
        for plan in ("A is the Bold direction.", "Read .video/ab-labels.json first.", f"A is {pairs['calm']}."):
            if not leaks(plan, label_map):
                failures.append(f"plan '{plan}' must fail")
        if leaks("A has a bold-ish palette, B is boldface.", {"labels": {"A": {"direction": "bold-type", "source": "x"}}}):
            failures.append("a direction name must match as a whole word only")
    for failure in failures:
        print(f"FAIL {failure}")
    print("ab_blind self-test: " + ("FAILED" if failures else "all cases passed"))
    return 1 if failures else 0


def main(argv):
    if argv == ["--self-test"]:
        return self_test()
    parser = argparse.ArgumentParser(description="Blind A/B labels and checks.")
    sub = parser.add_subparsers(dest="command", required=True)
    p = sub.add_parser("assign")
    p.add_argument("--seed", type=int, required=True)
    p.add_argument("--map", required=True)
    p.add_argument("--out-dir", required=True)
    p.add_argument("variants", nargs=2)
    p = sub.add_parser("lint-plan")
    p.add_argument("--map", required=True)
    p.add_argument("plan")
    p = sub.add_parser("check-rank")
    p.add_argument("--map")
    p.add_argument("rank")
    args = parser.parse_args(argv)
    inputs = [] if args.command == "assign" else [args.map, getattr(args, "plan", None), getattr(args, "rank", None)]
    for path in inputs:
        if path and not os.path.isfile(path):
            print(f"{path}: no such file", file=sys.stderr)
            return 2
    return {"assign": run_assign, "lint-plan": run_lint, "check-rank": run_check}[args.command](args)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
