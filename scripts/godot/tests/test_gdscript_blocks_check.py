"""Tests for scripts/godot/gdscript_blocks_check.py. Run: python3 -m unittest discover -s scripts/godot/tests

The parse tests need Godot (GODOT_PATH, `godot` on PATH or the macOS app
bundle); without it they are skipped.
"""

import contextlib
import io
import shutil
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

import gdscript_blocks_check as gbc  # noqa: E402

SAMPLE = HERE / "fixtures" / "blocks" / "sample.md"


def run(*args):
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        code = gbc.main([str(a) for a in args])
    return code, out.getvalue(), err.getvalue()


class ExtractTest(unittest.TestCase):
    def test_extracts_tagged_blocks_and_honours_skip(self):
        blocks = gbc.extract(SAMPLE)
        self.assertEqual([b.line for b in blocks], [6, 22, 33, 40, 47, 54])
        self.assertTrue(blocks[0].code.startswith("class_name Health\n"))

    def test_no_godot_skips(self):
        with mock.patch.object(gbc, "find_godot", return_value=None):
            code, out, _ = run(SAMPLE)
        self.assertEqual((code, out), (0, "skipped: no Godot\n"))


@unittest.skipIf(gbc.find_godot() is None, "no Godot")
class ParseTest(unittest.TestCase):
    def setUp(self):
        self.scratch = Path(tempfile.mkdtemp(dir=gbc.REPO / ".worktrees" / "_scratch"))
        self.addCleanup(shutil.rmtree, self.scratch, True)

    def test_sample(self):
        code, out, err = run("--scratch", self.scratch, SAMPLE)
        self.assertEqual(code, 1)
        lines = out.splitlines()
        self.assertEqual(len(lines), 1, out)
        self.assertTrue(lines[0].startswith(f"{SAMPLE}:57: Parse Error:"), lines[0])
        self.assertIn("6 blocks, 5 parse (3 whole, 1 with an added extends, 1 in a func), 1 fail", err)
        self.assertEqual(list(self.scratch.iterdir()), [], "the scratch project is removed")


if __name__ == "__main__":
    unittest.main()
