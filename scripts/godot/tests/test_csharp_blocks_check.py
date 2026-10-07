"""Tests for scripts/godot/csharp_blocks_check.py. Run: python3 -m unittest discover -s scripts/godot/tests"""

import contextlib
import io
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

import csharp_blocks_check as cbc  # noqa: E402

FIXTURE = HERE / "fixtures" / "csharp"
SAMPLE = FIXTURE / "sample.md"


def run(*args):
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        code = cbc.main([str(a) for a in args])
    return code, out.getvalue(), err.getvalue()


class ExtractTest(unittest.TestCase):
    def test_blocks_lines_and_skip(self):
        blocks = cbc.extract(SAMPLE)
        self.assertEqual([b.line for b in blocks], [4, 8, 15, 25, 37])
        self.assertTrue(all("does not compile" not in b.code for b in blocks))

    def test_forms(self):
        blocks = cbc.extract(SAMPLE)
        with tempfile.TemporaryDirectory() as tmp:
            for n, b in enumerate(blocks):
                cbc.write_forms(b, n, Path(tmp))
            # A class declaration is one whole form; a fragment gets 7 bases x 2.
            self.assertEqual([f.name for f in blocks[1].forms], ["whole"])
            self.assertEqual(len(blocks[0].forms), 14)
            text = (Path(tmp) / blocks[3].forms[0].file).read_text()
            self.assertIn("using Godot.Collections;", text)
            self.assertNotIn("using System;", text)


@unittest.skipIf(shutil.which("dotnet") is None, "no dotnet")
class BuildTest(unittest.TestCase):
    def test_own_blocks_fail_with_markdown_lines(self):
        code, out, _ = run(SAMPLE)
        self.assertEqual(code, 1)
        self.assertEqual(out.splitlines(), [
            f"{SAMPLE}:4: CS0266: Cannot implicitly convert type 'double' to 'int'. "
            "An explicit conversion exists (are you missing a cast?)",
            f"{SAMPLE}:38: CS1061: 'Node' does not contain a definition for 'NoSuchMethod' and no accessible "
            "extension method 'NoSuchMethod' accepting a first argument of type 'Node' could be found "
            "(are you missing a using directive or an assembly reference?)",
        ])

    def test_copied_blocks_are_counted_against_the_baseline(self):
        with tempfile.TemporaryDirectory() as tmp:
            baseline = Path(tmp) / "baseline.json"
            baseline.write_text('{"csharp_blocks_check": {"up": 2}}')
            args = ("--strict-own", FIXTURE / "copied.json", "--baseline", baseline, FIXTURE / "up")
            code, out, _ = run(*args)
            self.assertEqual(code, 0, out)
            self.assertEqual(out.splitlines(), [
                "csharp_blocks_check: 2 copied blocks fail in 1 skills, report only (baseline 2, baseline.json)",
                "  per skill: up 2",
            ])
            # U-22: a count above the baseline fails.
            baseline.write_text('{"csharp_blocks_check": {"up": 1}}')
            code, out, _ = run(*args)
            self.assertEqual(code, 1)
            self.assertIn("FAIL csharp_blocks_check: up has 2 copied blocks that fail, baseline 1.", out)


if __name__ == "__main__":
    unittest.main()
