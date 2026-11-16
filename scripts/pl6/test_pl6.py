#!/usr/bin/env python3
"""Tests of pl6.py. Run: python3 -I scripts/pl6/test_pl6.py"""
import io
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import pl6  # noqa: E402

LOG = """\
   Compiling horch v0.4.8
     Running unittests src/lib.rs (target/debug/deps/horch-1a2b3c4d5e6f7a8b)

running 2 tests
test parse::reads_flags ... ok
test parse::slow ... ignored, needs a live herdr

test result: ok. 1 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out

     Running unittests src/main.rs (target/debug/deps/horch-9f8e7d6c5b4a3f2e)

running 1 test
test parse::reads_flags ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

     Running tests/arch_scan.rs (target/debug/deps/arch_scan-0123456789abcdef)

running 1 test
test no_cfg_outside_platform ... FAILED

failures:

---- no_cfg_outside_platform stdout ----
test something ... in a panic message is not a result

failures:
    no_cfg_outside_platform

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out

   Doc-tests horch_core

running 1 test
test crates/horch-core/src/lib.rs - clock::now (line 42) ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
"""

EXPECTED = {
    "src/lib.rs|horch::parse::reads_flags": "ok",
    "src/lib.rs|horch::parse::slow": "ignored",
    "src/main.rs|horch::parse::reads_flags": "ok",
    "tests/arch_scan.rs|arch_scan::no_cfg_outside_platform": "failed",
    "doc|horch_core::crates/horch-core/src/lib.rs - clock::now": "ok",
}


def records(text):
    return pl6.parse_log(text.splitlines())


def compare(before, after, gaps=None):
    out = io.StringIO()
    with redirect_stdout(out):
        diffs = pl6.compare(before, after, gaps or {})
    return diffs


class ParseTest(unittest.TestCase):
    def test_parses_lib_bin_integration_and_doc_tests(self):
        got = records(LOG)
        self.assertEqual(got, EXPECTED)
        binaries = {key.split("::", 1)[0] for key in got}
        self.assertEqual(len(binaries), 4)

    def test_doc_test_key_has_no_line_number(self):
        got = records(LOG)
        doc = [key for key in got if key.startswith("doc|")]
        self.assertEqual(doc, ["doc|horch_core::crates/horch-core/src/lib.rs - clock::now"])
        moved = LOG.replace("(line 42)", "(line 97)")
        self.assertEqual(records(moved), got)

    def test_parses_ci_log_prefixes(self):
        stamp = "2026-11-16T21:27:57.1234567Z "
        ci = "\n".join(stamp + line for line in LOG.splitlines())
        gh = "\n".join(
            "test (windows-latest)\tRun cargo test --workspace\t" + stamp + line
            for line in LOG.splitlines()
        )
        crlf = LOG.replace("\n", "\r\n")
        self.assertEqual(records(ci), EXPECTED)
        self.assertEqual(records(gh), EXPECTED)
        self.assertEqual(records(crlf), EXPECTED)

    def test_refuses_log_with_no_result_line(self):
        with self.assertRaisesRegex(pl6.Pl6Error, "no 'test result:' line"):
            records("   Compiling horch v0.4.8\nerror: could not compile\n")

    def test_refuses_binary_with_no_result_line(self):
        cut = LOG.split("   Doc-tests horch_core")[0] + "   Doc-tests horch_core\n\nrunning 1 test\n"
        with self.assertRaisesRegex(pl6.Pl6Error, "incomplete run.*Doc-tests horch_core"):
            records(cut)
        crash = LOG.replace(
            "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n\n"
            "     Running tests/arch_scan.rs",
            "error: test failed, to rerun pass `--bin horch`\n\n"
            "     Running tests/arch_scan.rs",
        )
        with self.assertRaisesRegex(pl6.Pl6Error, "incomplete run.*src/main.rs"):
            records(crash)

    def test_record_writes_sorted_lines(self):
        with tempfile.TemporaryDirectory() as tmp:
            log = Path(tmp, "test.log")
            out = Path(tmp, "out.txt")
            log.write_text(LOG)
            self.assertEqual(pl6.main(["record", str(log), str(out)]), 0)
            lines = out.read_text().splitlines()
            self.assertEqual(lines, sorted(lines))
            self.assertEqual(pl6.read_record(out), EXPECTED)


class CompareTest(unittest.TestCase):
    def test_same_results_pass(self):
        self.assertEqual(compare(EXPECTED, dict(EXPECTED)), [])

    def test_changed_result_fails_and_names_the_test(self):
        after = dict(EXPECTED, **{"src/main.rs|horch::parse::reads_flags": "failed"})
        diffs = compare(EXPECTED, after)
        self.assertEqual(len(diffs), 1)
        self.assertIn("src/main.rs|horch::parse::reads_flags", diffs[0])
        self.assertIn("ok -> failed", diffs[0])

    def test_missing_test_fails(self):
        after = dict(EXPECTED)
        del after["src/lib.rs|horch::parse::slow"]
        diffs = compare(EXPECTED, after)
        self.assertEqual(len(diffs), 1)
        self.assertIn("src/lib.rs|horch::parse::slow", diffs[0])
        self.assertIn("removed or renamed", diffs[0])

    def test_new_test_must_pass(self):
        passing = dict(EXPECTED, **{"tests/arch_scan.rs|arch_scan::new_rule": "ok"})
        self.assertEqual(compare(EXPECTED, passing), [])
        failing = dict(EXPECTED, **{"tests/arch_scan.rs|arch_scan::new_rule": "failed"})
        diffs = compare(EXPECTED, failing)
        self.assertEqual(len(diffs), 1)
        self.assertIn("tests/arch_scan.rs|arch_scan::new_rule", diffs[0])
        self.assertIn("new test", diffs[0])

    def test_expected_failure_that_still_fails_passes(self):
        key = "tests/arch_scan.rs|arch_scan::no_cfg_outside_platform"
        self.assertEqual(EXPECTED[key], "failed")
        self.assertEqual(compare(EXPECTED, dict(EXPECTED)), [])

    def test_gap_file_allows_a_named_windows_fix_or_skip(self):
        fixed_key = "tests/arch_scan.rs|arch_scan::no_cfg_outside_platform"
        skipped_key = "src/lib.rs|horch::parse::gap_skip"
        other_key = "src/lib.rs|horch::parse::gap_other"
        before = dict(EXPECTED, **{skipped_key: "failed", other_key: "failed"})
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp, "gaps.tsv")
            path.write_text(
                f"{fixed_key}\t06 S3\tfixed\n{skipped_key}\t06 S11\tskipped\n"
            )
            gaps = pl6.read_gaps(path)

        allowed = dict(before, **{fixed_key: "ok", skipped_key: "ignored"})
        self.assertEqual(compare(before, allowed, gaps), [])

        wrong = dict(before, **{fixed_key: "ignored", skipped_key: "ok"})
        diffs = compare(before, wrong, gaps)
        self.assertEqual(len(diffs), 2)

        not_in_file = dict(before, **{other_key: "ok"})
        diffs = compare(before, not_in_file, gaps)
        self.assertEqual(len(diffs), 1)
        self.assertIn(other_key, diffs[0])

        gone = dict(before)
        del gone[fixed_key]
        self.assertEqual(len(compare(before, gone, gaps)), 1)

    def test_refuses_malformed_gaps_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp, "gaps.tsv")
            path.write_text("some|key::name\t06 S3\tmaybe\n")
            with self.assertRaisesRegex(pl6.Pl6Error, "gaps.tsv:1"):
                pl6.read_gaps(path)

    def test_compare_exit_code(self):
        with tempfile.TemporaryDirectory() as tmp:
            before = Path(tmp, "before.txt")
            after = Path(tmp, "after.txt")
            pl6.write_record(before, EXPECTED)
            pl6.write_record(after, EXPECTED)
            with redirect_stdout(io.StringIO()):
                self.assertEqual(pl6.main(["compare", str(before), str(after)]), 0)
            pl6.write_record(after, dict(EXPECTED, **{"src/lib.rs|horch::parse::slow": "ok"}))
            out = io.StringIO()
            with redirect_stdout(out):
                self.assertEqual(pl6.main(["compare", str(before), str(after)]), 1)
            self.assertIn("src/lib.rs|horch::parse::slow", out.getvalue())


if __name__ == "__main__":
    unittest.main()
