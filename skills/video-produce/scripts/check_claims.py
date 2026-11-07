#!/usr/bin/env python3
"""Check the caption claims ledger.

Usage:
  check_claims.py claims.md
  check_claims.py --self-test

claims.md must contain a Markdown table with the columns "Caption claim",
"Verdict", and "Evidence". Verdicts are true, partly true, or false. The
check fails for a false or unknown verdict, an empty evidence cell, or the
text "[inferred]" in any cell.

Exit codes: 0 pass, 1 a claim rule fails, 2 usage or a missing file.
Python standard library only.
"""

import os
import re
import sys
import tempfile


REQUIRED_COLUMNS = ("caption claim", "verdict", "evidence")
ALLOWED_VERDICTS = {"true", "partly true", "false"}
SEPARATOR = re.compile(r"^:?-{3,}:?$")


def cells(line):
    """Return stripped cells from one simple Markdown table row."""
    line = line.strip()
    if not line.startswith("|") or not line.endswith("|"):
        return None
    return [cell.strip() for cell in line[1:-1].split("|")]


def header_at(lines, index):
    """Return the normalized header at index, or None when no claims table starts there."""
    header = cells(lines[index])
    separator = cells(lines[index + 1])
    if header is None or separator is None or len(header) != len(separator):
        return None
    normalized = [name.casefold() for name in header]
    if not all(name in normalized for name in REQUIRED_COLUMNS):
        return None
    return normalized


def separator_is_valid(line):
    return all(SEPARATOR.fullmatch(cell) for cell in cells(line))


def table_rows(lines, start, header):
    """Return (line number, row dict or None) for each row from start to the table end."""
    rows = []
    for line_number, row_line in enumerate(lines[start:], start + 1):
        row = cells(row_line)
        if row is None:
            if row_line.strip():
                break
            continue
        rows.append((line_number, dict(zip(header, row)) if len(row) == len(header) else None))
    return rows


def find_claims_table(text):
    """Return (rows, errors) for the first table with the required columns."""
    lines = text.splitlines()
    for index in range(len(lines) - 1):
        header = header_at(lines, index)
        if header is None:
            continue
        if not separator_is_valid(lines[index + 1]):
            return [], [f"line {index + 2}: invalid table separator"]
        return table_rows(lines, index + 2, header), []
    return [], ["no claims table with Caption claim, Verdict, and Evidence columns"]


def verdict_error(verdict):
    if verdict.casefold() not in ALLOWED_VERDICTS:
        return f"unverified verdict '{verdict}'"
    if verdict.casefold() == "false":
        return "verdict is false"
    return None


def row_errors(row):
    """Return the rule errors of 1 claims row."""
    if row is None:
        return ["table row has the wrong number of cells"]
    found = [
        "caption claim is empty" if not row["caption claim"] else None,
        "[inferred] is not evidence" if "[inferred]" in " | ".join(row.values()).casefold() else None,
        verdict_error(row["verdict"]),
        "evidence is empty" if not row["evidence"] else None,
    ]
    return [error for error in found if error]


def check(text):
    rows, errors = find_claims_table(text)
    if errors:
        return errors
    if not rows:
        return ["claims table has no rows"]
    return [
        f"line {line_number}: {error}"
        for line_number, row in rows
        for error in row_errors(row)
    ]


GOOD = """# Claims

| Caption claim | Verdict | Evidence |
| --- | --- | --- |
| The app saves 4 s. | true | Measurement: 4.0 s in the release build. |
| Setup is automatic. | partly true | Play test: the user still selects a folder. |
"""


def self_test():
    cases = {
        "clean ledger": (GOOD, 0),
        "false verdict": (GOOD.replace("| true |", "| false |", 1), 1),
        "empty evidence": (GOOD.replace("Measurement: 4.0 s in the release build.", ""), 1),
        "inferred text": (GOOD.replace("Play test:", "[inferred] Play test:"), 1),
        "unknown verdict": (GOOD.replace("| true |", "| unverified |", 1), 1),
        "missing table": ("# Claims\n\nNo claims yet.\n", 1),
    }
    failures = []
    with tempfile.TemporaryDirectory() as folder:
        for name, (content, expected) in cases.items():
            path = os.path.join(folder, "claims.md")
            with open(path, "w", encoding="utf-8") as handle:
                handle.write(content)
            with open(path, encoding="utf-8") as handle:
                result = 1 if check(handle.read()) else 0
            if result != expected:
                failures.append(f"{name}: expected {expected}, got {result}")
    for failure in failures:
        print(f"FAIL {failure}")
    print(
        "check_claims self-test: "
        + ("FAILED" if failures else f"{len(cases)} cases passed")
    )
    return 1 if failures else 0


def main(argv):
    if argv == ["--self-test"]:
        return self_test()
    if len(argv) != 1:
        print(__doc__.strip(), file=sys.stderr)
        return 2
    path = argv[0]
    if not os.path.isfile(path):
        print(f"{path}: no such file", file=sys.stderr)
        return 2
    with open(path, encoding="utf-8") as handle:
        errors = check(handle.read())
    for error in errors:
        print(f"{path}: {error}")
    print(f"{path}: {'FAIL' if errors else 'PASS'}")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
