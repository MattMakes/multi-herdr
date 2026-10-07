"""Tests for scripts/godot/gdscript_blocks_check.py. Run: python3 -m unittest discover -s scripts/godot/tests

The parse tests need Godot (GODOT_PATH, `godot` on PATH or the macOS app
bundle); without it they are skipped.
"""

import contextlib
import io
import json
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


def make_skills(root: Path) -> Path:
    """A skills/ dir: godot-x/SKILL.md is copied, references/own.md is own text."""
    skill = root / "skills" / "godot-x"
    (skill / "references").mkdir(parents=True)
    shutil.copy(SAMPLE, skill / "SKILL.md")
    shutil.copy(SAMPLE, skill / "references" / "own.md")
    copied = {"skills": [{"name": "godot-x", "copied_files": ["SKILL.md"]}]}
    path = root / "skills" / "copied.json"
    path.write_text(json.dumps(copied))
    return path


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

    def test_copied_files_come_from_copied_json(self):
        with tempfile.TemporaryDirectory() as tmp:
            copied = make_skills(Path(tmp))
            self.assertEqual(
                gbc.copied_files(copied), {copied.resolve().parent / "godot-x" / "SKILL.md"}
            )

    def test_no_godot_skips(self):
        with mock.patch.object(gbc, "find_godot", return_value=None):
            code, out, _ = run(SAMPLE)
        self.assertEqual((code, out), (0, "skipped: no Godot\n"))


@unittest.skipIf(gbc.find_godot() is None, "no Godot")
class ParseTest(unittest.TestCase):
    def setUp(self):
        # A clean checkout (the gate's) has no .worktrees/_scratch yet.
        parent = gbc.REPO / ".worktrees" / "_scratch"
        parent.mkdir(parents=True, exist_ok=True)
        self.scratch = Path(tempfile.mkdtemp(dir=parent))
        self.addCleanup(shutil.rmtree, self.scratch, True)

    def test_sample(self):
        code, out, err = run("--scratch", self.scratch, SAMPLE)
        self.assertEqual(code, 1)
        lines = out.splitlines()
        self.assertEqual(len(lines), 1, out)
        self.assertTrue(lines[0].startswith(f"{SAMPLE}:57: Parse Error:"), lines[0])
        self.assertIn("6 blocks, 5 parse (3 whole, 1 with an added extends, 1 in a func), 1 fail", err)
        self.assertEqual(list(self.scratch.iterdir()), [], "the scratch project is removed")

    def test_strict_own_counts_copied_and_fails_own(self):
        copied = make_skills(self.scratch)
        skill = copied.parent / "godot-x"
        baseline = self.scratch / "baseline.json"
        baseline.write_text('{"gdscript_blocks_check": {"godot-x": 1}}')
        args = ("--scratch", self.scratch, "--strict-own", copied, "--baseline", baseline)
        code, out, err = run(*args, skill / "SKILL.md")
        self.assertEqual(code, 0, out)
        # U-22: a copied failure is counted, not printed.
        self.assertNotIn("Parse Error", out)
        self.assertIn("gdscript_blocks_check: 1 copied blocks fail in 1 skills, report only (baseline 1,", out)
        self.assertIn("1 fail (1 copied)", err)
        code, out, err = run(*args, skill)
        self.assertEqual(code, 1)
        self.assertIn("own.md:57: Parse Error:", out)
        self.assertIn("2 fail (1 copied)", err)

    def test_copied_count_above_the_baseline_fails(self):
        copied = make_skills(self.scratch)
        baseline = self.scratch / "baseline.json"
        baseline.write_text("{}")
        code, out, _ = run(
            "--scratch", self.scratch, "--strict-own", copied, "--baseline", baseline,
            copied.parent / "godot-x" / "SKILL.md",
        )
        self.assertEqual(code, 1)
        self.assertIn("FAIL gdscript_blocks_check: godot-x has 1 copied blocks that fail, baseline 0.", out)
        self.assertIn("SKILL.md:57: Parse Error:", out)


if __name__ == "__main__":
    unittest.main()
