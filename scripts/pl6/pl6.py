#!/usr/bin/env python3
"""The test of PL-6: each test has the same result before the move and after it.

Usage:
  pl6.py record <log> <out>
      Read the output of `cargo test --workspace --no-fail-fast` (a plain log, a
      CI log with a time prefix, or `gh run view --log` output) and write 1 line
      `<key>\\t<result>` for each test, sorted. The result is ok, failed or ignored.
  pl6.py compare <before> <after> [--gaps <file>]
      Print each difference of 2 records and exit 1 when 1 exists.
      A test that is absent after is an error. A new test must pass.
      With --gaps, a key of the file may go from failed to ok (fixed) or to
      ignored (skipped). The gaps file has 1 line `<key>\\t<row>\\t<fixed|skipped>`.

Exit 2 on a bad input. Python standard library only.
"""
import argparse
import re
import sys
from collections import defaultdict
from pathlib import Path

PREFIX = re.compile(r"^(?:[^\t]*\t[^\t]*\t)?\ufeff?\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d+)?Z ")
RUNNING = re.compile(r"^\s*Running (?:unittests )?(.+?) \((.+)\)$")
DOC_TESTS = re.compile(r"^\s*Doc-tests (\S+)$")
TEST = re.compile(r"^test (.+) \.\.\. (ok|FAILED|ignored)(?:, .*)?$")
SUMMARY = re.compile(r"^test result: ")
LINE_NUMBER = re.compile(r" \(line \d+\)")
HASH = re.compile(r"-[0-9a-f]+$")
RESULTS = {"ok": "ok", "FAILED": "failed", "ignored": "ignored"}
GAP_RESULT = {"fixed": "ok", "skipped": "ignored"}


class Pl6Error(Exception):
    pass


def strip_prefix(line):
    return PREFIX.sub("", line.rstrip("\r\n"), count=1)


def binary_prefix(source, binary):
    stem = binary.replace("\\", "/").rsplit("/", 1)[-1]
    stem = HASH.sub("", stem.removesuffix(".exe"))
    return f"{source.replace(chr(92), '/')}|{stem}"


def parse_log(lines):
    """Return {key: result} for each test of a cargo test log."""
    found = defaultdict(list)
    current = None
    open_binary = None
    summaries = 0
    for raw in lines:
        line = strip_prefix(raw)
        running = RUNNING.match(line)
        doc = DOC_TESTS.match(line)
        if running or doc:
            if open_binary:
                raise Pl6Error(f"incomplete run: {open_binary} has no 'test result:' line")
            if running:
                current = binary_prefix(running.group(1), running.group(2))
            else:
                current = f"doc|{doc.group(1)}"
            open_binary = line.strip()
            continue
        if SUMMARY.match(line):
            summaries += 1
            open_binary = None
            continue
        test = TEST.match(line)
        if test and open_binary:
            name = test.group(1)
            if current.startswith("doc|"):
                name = LINE_NUMBER.sub("", name)
            found[f"{current}::{name}"].append(RESULTS[test.group(2)])
    if open_binary:
        raise Pl6Error(f"incomplete run: {open_binary} has no 'test result:' line")
    if summaries == 0:
        raise Pl6Error("the log has no 'test result:' line")
    records = {}
    for key, results in found.items():
        if len(results) == 1:
            records[key] = results[0]
            continue
        for index, result in enumerate(sorted(results), start=1):
            records[f"{key}#{index}"] = result
    return records


def write_record(path, records):
    lines = [f"{key}\t{result}\n" for key, result in sorted(records.items())]
    Path(path).write_text("".join(lines), encoding="utf-8")


def read_record(path):
    records = {}
    for number, line in enumerate(Path(path).read_text(encoding="utf-8").splitlines(), 1):
        key, sep, result = line.rpartition("\t")
        if not sep or result not in RESULTS.values():
            raise Pl6Error(f"{path}:{number}: expected '<key>\\t<ok|failed|ignored>'")
        records[key] = result
    return records


def read_gaps(path):
    """Return {key: allowed result after} from a gaps file."""
    gaps = {}
    for number, line in enumerate(Path(path).read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip():
            continue
        parts = line.split("\t")
        if len(parts) != 3 or not parts[1].strip() or parts[2] not in GAP_RESULT:
            raise Pl6Error(f"{path}:{number}: expected '<key>\\t<row of section 6>\\t<fixed|skipped>'")
        gaps[parts[0]] = GAP_RESULT[parts[2]]
    return gaps


def compare(before, after, gaps):
    """Print and return each difference of 2 records."""
    diffs = []
    for key in sorted(before.keys() | after.keys()):
        old, new = before.get(key), after.get(key)
        if old == new:
            continue
        if new is None:
            diffs.append(f"{key}: removed or renamed (was {old})")
        elif old is None:
            if new != "ok":
                diffs.append(f"{key}: new test is {new}, a new test must pass")
        elif old == "failed" and gaps.get(key) == new:
            continue
        else:
            diffs.append(f"{key}: {old} -> {new}")
    for diff in diffs:
        print(diff)
    return diffs


def main(argv):
    parser = argparse.ArgumentParser(prog="pl6.py", description=__doc__.split("\n")[0])
    sub = parser.add_subparsers(dest="command", required=True)
    record = sub.add_parser("record")
    record.add_argument("log")
    record.add_argument("out")
    cmp = sub.add_parser("compare")
    cmp.add_argument("before")
    cmp.add_argument("after")
    cmp.add_argument("--gaps")
    args = parser.parse_args(argv)
    try:
        if args.command == "record":
            with open(args.log, encoding="utf-8", errors="replace") as log:
                write_record(args.out, parse_log(log))
            return 0
        gaps = read_gaps(args.gaps) if args.gaps else {}
        before, after = read_record(args.before), read_record(args.after)
        diffs = compare(before, after, gaps)
        if diffs:
            print(f"PL-6 FAIL: {len(diffs)} difference(s) of {len(before)} tests")
            return 1
        print(f"PL-6 ok: {len(after)} tests, no difference")
        return 0
    except (Pl6Error, OSError) as err:
        print(f"pl6: {err}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
