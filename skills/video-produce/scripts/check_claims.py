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


def find_claims_table(text):
    """Return (rows, errors) for the first table with the required columns."""
    lines = text.splitlines()
    for index, line in enumerate(lines[:-1]):
        header = cells(line)
        separator = cells(lines[index + 1])
        if header is None or separator is None or len(header) != len(separator):
            continue
        normalized = [name.casefold() for name in header]
        if not all(name in normalized for name in REQUIRED_COLUMNS):
            continue
        if not all(SEPARATOR.fullmatch(cell) for cell in separator):
            return [], [f"line {index + 2}: invalid table separator"]
        rows = []
        for line_number, row_line in enumerate(lines[index + 2 :], index + 3):
            row = cells(row_line)
            if row is None:
                if row_line.strip():
                    break
                continue
            if len(row) != len(header):
                rows.append((line_number, None))
                continue
            rows.append((line_number, dict(zip(normalized, row))))
        return rows, []
    return [], ["no claims table with Caption claim, Verdict, and Evidence columns"]


def check(text):
    rows, errors = find_claims_table(text)
    if errors:
        return errors
    if not rows:
        return ["claims table has no rows"]
    for line_number, row in rows:
        if row is None:
            errors.append(f"line {line_number}: table row has the wrong number of cells")
            continue
        claim = row["caption claim"]
        verdict = row["verdict"].casefold()
        evidence = row["evidence"]
        if not claim:
            errors.append(f"line {line_number}: caption claim is empty")
        if "[inferred]" in " | ".join(row.values()).casefold():
            errors.append(f"line {line_number}: [inferred] is not evidence")
        if verdict not in ALLOWED_VERDICTS:
            errors.append(f"line {line_number}: unverified verdict '{row['verdict']}'")
        elif verdict == "false":
            errors.append(f"line {line_number}: verdict is false")
        if not evidence:
            errors.append(f"line {line_number}: evidence is empty")
    return errors


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
