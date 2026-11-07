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


def table_end(lines, start):
    """Return the index of the first non-blank line after start that is not a table row."""
    return next(
        (i for i in range(start, len(lines)) if cells(lines[i]) is None and lines[i].strip()),
        len(lines),
    )


def to_row(header, row):
    return dict(zip(header, row)) if len(row) == len(header) else None


def table_rows(lines, start, header):
    """Return (line number, row dict or None) for each row from start to the table end."""
    found = [(i + 1, cells(lines[i])) for i in range(start, table_end(lines, start))]
    return [(number, to_row(header, row)) for number, row in found if row is not None]


def find_claims_table(text):
    """Return (rows, errors) for the first table with the required columns."""
    lines = text.splitlines()
    index = next((i for i in range(len(lines) - 1) if header_at(lines, i)), None)
    if index is None:
        return [], ["no claims table with Caption claim, Verdict, and Evidence columns"]
    if not separator_is_valid(lines[index + 1]):
        return [], [f"line {index + 2}: invalid table separator"]
    return table_rows(lines, index + 2, header_at(lines, index)), []


def verdict_error(verdict):
    key = verdict.casefold()
    if key == "false":
        return "verdict is false"
    return None if key in ALLOWED_VERDICTS else f"unverified verdict '{verdict}'"


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


SELF_TEST_CASES = {
    "clean ledger": (GOOD, 0),
    "false verdict": (GOOD.replace("| true |", "| false |", 1), 1),
    "empty evidence": (GOOD.replace("Measurement: 4.0 s in the release build.", ""), 1),
    "inferred text": (GOOD.replace("Play test:", "[inferred] Play test:"), 1),
    "unknown verdict": (GOOD.replace("| true |", "| unverified |", 1), 1),
    "missing table": ("# Claims\n\nNo claims yet.\n", 1),
}


def check_file(path):
    with open(path, encoding="utf-8") as handle:
        return check(handle.read())


def case_result(folder, content):
    path = os.path.join(folder, "claims.md")
    with open(path, "w", encoding="utf-8") as handle:
        handle.write(content)
    return 1 if check_file(path) else 0


def self_test():
    with tempfile.TemporaryDirectory() as folder:
        failures = [
            f"{name}: expected {expected}"
            for name, (content, expected) in SELF_TEST_CASES.items()
            if case_result(folder, content) != expected
        ]
    for failure in failures:
        print(f"FAIL {failure}")
    status = "FAILED" if failures else f"{len(SELF_TEST_CASES)} cases passed"
    print(f"check_claims self-test: {status}")
    return 1 if failures else 0


def usage_error(argv):
    if len(argv) != 1:
        return __doc__.strip()
    return None if os.path.isfile(argv[0]) else f"{argv[0]}: no such file"


def report(path):
    errors = check_file(path)
    for error in errors:
        print(f"{path}: {error}")
    print(f"{path}: {'FAIL' if errors else 'PASS'}")
    return 1 if errors else 0


def main(argv):
    if argv == ["--self-test"]:
        return self_test()
    error = usage_error(argv)
    if error:
        print(error, file=sys.stderr)
        return 2
    return report(argv[0])


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
