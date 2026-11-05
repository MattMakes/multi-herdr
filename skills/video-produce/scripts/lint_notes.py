#!/usr/bin/env python3
"""Lint a taste notes file (.video/NOTES.md or the fleet copy).

Usage:
  lint_notes.py <NOTES.md> [<NOTES.md> ...]
  lint_notes.py --self-test

An entry starts with a "## <title>" line. Its fields are "- <key>: <value>"
lines under the title:

  ## Hold the logo 0.5 s longer at the end
  - kind: preference
  - topics: ending, pacing
  - evidence: "the logo leaves too fast" (operator, launch-v2 round 1)
  - films: launch-v2

Rules:
  - kind is preference, observed or verified.
  - topics names 1 or more topics, separated by commas.
  - evidence is not empty.
  - a preference quotes the operator's own words in double quotes.
  - a verified entry lists 2 or more films, or its evidence starts with
    "measured".
  - fields are only kind, topics, evidence, films and date. Each field is
    given once.
  - titles are unique.
  - a notes file never sits inside a skills/ folder. Taste notes do not ship.

Exit codes: 0 clean, 1 a rule fails, 2 usage or a missing file.
Python standard library only.
"""

import os
import re
import sys
import tempfile

KINDS = ("preference", "observed", "verified")
FIELDS = ("kind", "topics", "evidence", "films", "date")
FIELD_RE = re.compile(r"^-\s+([A-Za-z_]+)\s*:\s*(.*)$")


def parse(text):
    """Return (entries, errors). An entry is (title, line, fields)."""
    entries, errors, current = [], [], None
    for number, raw in enumerate(text.splitlines(), 1):
        line = raw.strip()
        if line.startswith("## "):
            current = (line[3:].strip(), number, {})
            entries.append(current)
            continue
        match = FIELD_RE.match(line)
        if not match or current is None:
            continue
        key, value = match.group(1).lower(), match.group(2).strip()
        title, _, fields = current
        if key not in FIELDS:
            errors.append(f"line {number}: '{title}': unknown field '{key}'")
        elif key in fields:
            errors.append(f"line {number}: '{title}': field '{key}' is given twice")
        else:
            fields[key] = value
    return entries, errors


def split_list(value):
    return [part.strip() for part in value.split(",") if part.strip()]


def check_entry(title, line, fields):
    where = f"line {line}: '{title}'"
    errors = []
    kind = fields.get("kind", "")
    if kind not in KINDS:
        errors.append(f"{where}: kind must be 1 of {', '.join(KINDS)} (got '{kind}')")
    if not split_list(fields.get("topics", "")):
        errors.append(f"{where}: topics is missing or empty")
    evidence = fields.get("evidence", "")
    if not evidence:
        errors.append(f"{where}: evidence is missing or empty")
    if kind == "preference" and evidence and not re.search(r'"[^"]+"', evidence):
        errors.append(f"{where}: a preference quotes the operator's words in double quotes")
    if kind == "verified":
        films = split_list(fields.get("films", ""))
        if len(films) < 2 and not evidence.lower().startswith("measured"):
            errors.append(f"{where}: verified needs 2 or more films, or evidence that starts with 'measured'")
    return errors


def lint_text(text, path=None):
    errors = []
    parts = os.path.normpath(os.path.abspath(path)).split(os.sep) if path else []
    if "skills" in parts[:-1]:
        errors.append(f"{path}: a notes file never sits inside a skills/ folder")
    entries, parse_errors = parse(text)
    errors.extend(parse_errors)
    seen = set()
    for title, line, fields in entries:
        if title.lower() in seen:
            errors.append(f"line {line}: '{title}': the title is used twice")
        seen.add(title.lower())
        errors.extend(check_entry(title, line, fields))
    return len(entries), errors


def lint_file(path):
    with open(path, encoding="utf-8") as handle:
        return lint_text(handle.read(), path)


CLEAN = """# Taste notes

## Hold the logo 0.5 s longer at the end
- kind: preference
- topics: ending, pacing
- evidence: "the logo leaves too fast" (operator, launch-v2 round 1)
- films: launch-v2

## Hook text over 96 px reads on a phone
- kind: verified
- topics: typography, hook
- evidence: critic rounds of 2 films scored axis 3 at 5
- films: launch-v2, cli-demo
"""

BAD = {
    "no kind": "## A\n- topics: x\n- evidence: e\n",
    "bad kind": "## A\n- kind: rule\n- topics: x\n- evidence: e\n",
    "no topics": "## A\n- kind: observed\n- evidence: e\n",
    "no evidence": "## A\n- kind: observed\n- topics: x\n",
    "unquoted preference": "## A\n- kind: preference\n- topics: x\n- evidence: operator said so\n",
    "verified with 1 film": "## A\n- kind: verified\n- topics: x\n- evidence: e\n- films: one\n",
    "unknown field": "## A\n- kind: observed\n- topics: x\n- evidence: e\n- rule: always\n",
    "duplicate title": "## A\n- kind: observed\n- topics: x\n- evidence: e\n"
    "## A\n- kind: observed\n- topics: y\n- evidence: f\n",
}


def self_test():
    failures = []
    count, errors = lint_text(CLEAN)
    if count != 2 or errors:
        failures.append(f"clean: expected 2 entries and no errors, got {count} and {errors}")
    measured = "## A\n- kind: verified\n- topics: x\n- evidence: measured 3.1 dB margin\n"
    if lint_text(measured)[1]:
        failures.append("a measured verified entry must pass")
    for name, text in BAD.items():
        errors = lint_text(text)[1]
        if len(errors) != 1:
            failures.append(f"{name}: expected 1 error, got {errors}")
    with tempfile.TemporaryDirectory() as tmp:
        shipped = os.path.join(tmp, "skills", "video-produce", "NOTES.md")
        if not any("skills/ folder" in e for e in lint_text(CLEAN, shipped)[1]):
            failures.append("a notes file under skills/ must fail")
    for failure in failures:
        print(f"FAIL {failure}")
    print("lint_notes self-test: " + ("FAILED" if failures else f"{2 + len(BAD) + 1} cases passed"))
    return 1 if failures else 0


def main(argv):
    if argv == ["--self-test"]:
        return self_test()
    if not argv or any(a.startswith("-") for a in argv):
        print(__doc__.strip(), file=sys.stderr)
        return 2
    status = 0
    for path in argv:
        if not os.path.isfile(path):
            print(f"{path}: no such file", file=sys.stderr)
            return 2
        count, errors = lint_file(path)
        for error in errors:
            print(f"{path}: {error}")
        print(f"{path}: {count} entries, {len(errors)} errors")
        if errors:
            status = 1
    return status


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
