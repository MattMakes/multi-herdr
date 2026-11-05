#!/usr/bin/env python3
"""Validate a batch manifest and write the batch summary table.

Usage:
  validate_batch.py <batch.json>
  validate_batch.py <batch.json> --summary <batch-summary.md>
  validate_batch.py --self-test

The manifest:

  {
    "batch": "weekly-2026-45",
    "shared": {"brand": "brand/BRAND.md", "voice": "Samantha",
               "notes": "video/.video/NOTES.md"},
    "films": [
      {"slug": "launch-v3", "request": "30 s launch film for the CLI",
       "workflow": "launch-film", "folder": "video/launch-v3",
       "length_sec": 30, "aspects": ["16:9", "9:16"]}
    ]
  }

Rules: "batch" is a slug. "shared" holds only brand, voice and notes, each a
non-empty string. "films" holds 1 or more films. Each film has exactly the 6
keys above. slug is unique and uses a-z, 0-9 and "-". folder is unique and
relative. workflow is launch-film, app-explainer or ab. length_sec is 15, 30,
60 or 90. aspects is a non-empty list of 16:9, 9:16 and 1:1, with no repeat.

--summary reads each film folder (relative to the manifest folder) and
writes 1 table row per film: critic rounds, the verdict and average of the
last round, and whether render/master.mp4 and render/share.md exist. A film
is "shipped" only with VERDICT: PASS, a master and a share kit.

Exit codes: 0 valid, 1 a rule fails, 2 usage or a missing file.
Python standard library only.
"""

import json
import os
import re
import sys
import tempfile

WORKFLOWS = ("launch-film", "app-explainer", "ab")
LENGTHS = (15, 30, 60, 90)
ASPECTS = ("16:9", "9:16", "1:1")
FILM_KEYS = {"slug", "request", "workflow", "folder", "length_sec", "aspects"}
SHARED_KEYS = {"brand", "voice", "notes"}
SLUG_RE = re.compile(r"^[a-z0-9]+(-[a-z0-9]+)*$")


def check_film(index, film, slugs, folders):
    where = f"films[{index}]"
    if not isinstance(film, dict):
        return [f"{where}: must be an object"]
    errors = []
    if set(film) != FILM_KEYS:
        missing = sorted(FILM_KEYS - set(film))
        extra = sorted(set(film) - FILM_KEYS)
        errors.append(f"{where}: keys must be {sorted(FILM_KEYS)} (missing {missing}, unknown {extra})")
    slug = film.get("slug")
    if not isinstance(slug, str) or not SLUG_RE.match(slug):
        errors.append(f"{where}: slug must use a-z, 0-9 and '-'")
    elif slug in slugs:
        errors.append(f"{where}: slug '{slug}' is used twice")
    slugs.add(slug if isinstance(slug, str) else None)
    if not isinstance(film.get("request"), str) or not film.get("request", "").strip():
        errors.append(f"{where}: request must be a non-empty string")
    if film.get("workflow") not in WORKFLOWS:
        errors.append(f"{where}: workflow must be 1 of {', '.join(WORKFLOWS)}")
    folder = film.get("folder")
    if not isinstance(folder, str) or not folder or os.path.isabs(folder) or ".." in folder.split("/"):
        errors.append(f"{where}: folder must be a relative path inside the batch folder")
    elif os.path.normpath(folder) in folders:
        errors.append(f"{where}: folder '{folder}' is used twice")
    else:
        folders.add(os.path.normpath(folder))
    if film.get("length_sec") not in LENGTHS or isinstance(film.get("length_sec"), bool):
        errors.append(f"{where}: length_sec must be 1 of {LENGTHS}")
    aspects = film.get("aspects")
    if (
        not isinstance(aspects, list)
        or not aspects
        or any(a not in ASPECTS for a in aspects)
        or len(set(aspects)) != len(aspects)
    ):
        errors.append(f"{where}: aspects must be a non-empty list of {', '.join(ASPECTS)} with no repeat")
    return errors


def validate(manifest):
    if not isinstance(manifest, dict):
        return ["the manifest must be a JSON object"]
    errors = []
    unknown = sorted(set(manifest) - {"batch", "shared", "films"})
    if unknown:
        errors.append(f"unknown top-level keys {unknown}")
    if not isinstance(manifest.get("batch"), str) or not SLUG_RE.match(manifest.get("batch", "")):
        errors.append("batch must be a slug (a-z, 0-9 and '-')")
    shared = manifest.get("shared")
    if not isinstance(shared, dict) or set(shared) - SHARED_KEYS:
        errors.append(f"shared must be an object with only {sorted(SHARED_KEYS)}")
    else:
        for key, value in shared.items():
            if not isinstance(value, str) or not value.strip():
                errors.append(f"shared.{key} must be a non-empty string")
    films = manifest.get("films")
    if not isinstance(films, list) or not films:
        errors.append("films must be a non-empty list")
        return errors
    slugs, folders = set(), set()
    for index, film in enumerate(films):
        errors.extend(check_film(index, film, slugs, folders))
    return errors


def last_round(qa_dir):
    """Return (rounds, verdict, average) from qa/critique-round-<N>.md."""
    rounds = []
    if os.path.isdir(qa_dir):
        for name in os.listdir(qa_dir):
            match = re.fullmatch(r"critique-round-(\d+)\.md", name)
            if match:
                rounds.append((int(match.group(1)), name))
    if not rounds:
        return 0, "none", "-"
    _, name = max(rounds)
    with open(os.path.join(qa_dir, name), encoding="utf-8") as handle:
        text = handle.read()
    verdict = re.search(r"^VERDICT:\s*(PASS|FAIL)\b", text, re.M)
    average = re.search(r"^Average score:\s*([0-9.]+)", text, re.M)
    return len(rounds), verdict.group(1) if verdict else "none", average.group(1) if average else "-"


def summary(manifest, base):
    lines = [
        f"# Batch {manifest['batch']}",
        "",
        "| Film | Workflow | Rounds | Verdict | Average | Master | Share kit | Status |",
        "| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |",
    ]
    shipped = 0
    for film in manifest["films"]:
        folder = os.path.join(base, film["folder"])
        rounds, verdict, average = last_round(os.path.join(folder, "qa"))
        master = os.path.isfile(os.path.join(folder, "render", "master.mp4"))
        share = os.path.isfile(os.path.join(folder, "render", "share.md"))
        status = "shipped" if verdict == "PASS" and master and share else "open"
        shipped += status == "shipped"
        lines.append(
            f"| {film['slug']} | {film['workflow']} | {rounds} | {verdict} | {average} | "
            f"{'yes' if master else 'no'} | {'yes' if share else 'no'} | {status} |"
        )
    lines += ["", f"{shipped} of {len(manifest['films'])} films shipped.", ""]
    return "\n".join(lines)


def load(path):
    with open(path, encoding="utf-8") as handle:
        return json.load(handle)


def good_manifest():
    film = {"request": "30 s launch film", "workflow": "launch-film", "length_sec": 30, "aspects": ["16:9"]}
    return {
        "batch": "weekly-2026-45",
        "shared": {"brand": "brand/BRAND.md", "voice": "Samantha", "notes": ".video/NOTES.md"},
        "films": [
            dict(film, slug="launch-v3", folder="video/launch-v3"),
            dict(film, slug="cli-demo", folder="video/cli-demo", workflow="app-explainer"),
        ],
    }


def bad_cases():
    def edit(change):
        manifest = good_manifest()
        change(manifest)
        return manifest

    return {
        "no films": edit(lambda m: m.update(films=[])),
        "bad batch slug": edit(lambda m: m.update(batch="Weekly 45")),
        "unknown shared key": edit(lambda m: m["shared"].update(music="x.wav")),
        "duplicate slug": edit(lambda m: m["films"][1].update(slug="launch-v3")),
        "duplicate folder": edit(lambda m: m["films"][1].update(folder="video/launch-v3/")),
        "absolute folder": edit(lambda m: m["films"][1].update(folder="/tmp/x")),
        "bad workflow": edit(lambda m: m["films"][0].update(workflow="trailer")),
        "bad length": edit(lambda m: m["films"][0].update(length_sec=45)),
        "bad aspect": edit(lambda m: m["films"][0].update(aspects=["4:3"])),
        "unknown film key": edit(lambda m: m["films"][0].update(music="x.wav")),
    }


def write(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as handle:
        handle.write(text)


def self_test():
    failures = []
    if validate(good_manifest()):
        failures.append(f"clean manifest: {validate(good_manifest())}")
    for name, manifest in bad_cases().items():
        errors = validate(manifest)
        if len(errors) != 1:
            failures.append(f"{name}: expected 1 error, got {errors}")
    with tempfile.TemporaryDirectory() as tmp:
        # Dry run of 2 films: 1 passes in round 2, 1 fails round 1 with no share kit.
        launch = os.path.join(tmp, "video", "launch-v3")
        demo = os.path.join(tmp, "video", "cli-demo")
        write(os.path.join(launch, "qa", "critique-round-1.md"), "VERDICT: FAIL\nAverage score: 3.50 / 5.\n")
        write(os.path.join(launch, "qa", "critique-round-2.md"), "VERDICT: PASS\nAverage score: 4.25 / 5.\n")
        write(os.path.join(launch, "render", "master.mp4"), "x")
        write(os.path.join(launch, "render", "share.md"), "x")
        write(os.path.join(demo, "qa", "critique-round-1.md"), "VERDICT: FAIL\nAverage score: 3.10 / 5.\n")
        write(os.path.join(demo, "render", "master.mp4"), "x")
        table = summary(good_manifest(), tmp)
        rows = [line for line in table.splitlines() if line.startswith("| ") and not line.startswith("| Film")]
        rows = [row for row in rows if not row.startswith("| :")]
        want = [
            "| launch-v3 | launch-film | 2 | PASS | 4.25 | yes | yes | shipped |",
            "| cli-demo | app-explainer | 1 | FAIL | 3.10 | yes | no | open |",
        ]
        if rows != want:
            failures.append(f"summary rows: expected {want}, got {rows}")
        if "1 of 2 films shipped." not in table:
            failures.append("summary must count 1 of 2 films shipped")
    for failure in failures:
        print(f"FAIL {failure}")
    print("validate_batch self-test: " + ("FAILED" if failures else f"{2 + len(bad_cases())} cases passed"))
    return 1 if failures else 0


def main(argv):
    if argv == ["--self-test"]:
        return self_test()
    if len(argv) not in (1, 3) or (len(argv) == 3 and argv[1] != "--summary"):
        print(__doc__.strip(), file=sys.stderr)
        return 2
    path = argv[0]
    if not os.path.isfile(path):
        print(f"{path}: no such file", file=sys.stderr)
        return 2
    try:
        manifest = load(path)
    except json.JSONDecodeError as error:
        print(f"{path}: not valid JSON: {error}")
        return 1
    errors = validate(manifest)
    for error in errors:
        print(f"{path}: {error}")
    if errors:
        return 1
    print(f"{path}: valid, {len(manifest['films'])} films")
    if len(argv) == 3:
        write(os.path.abspath(argv[2]), summary(manifest, os.path.dirname(os.path.abspath(path))))
        print(f"wrote {argv[2]}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
