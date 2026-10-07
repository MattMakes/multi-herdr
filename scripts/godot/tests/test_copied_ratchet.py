"""Tests for scripts/godot/copied_ratchet.py (U-22). Run: python3 -m unittest discover -s scripts/godot/tests"""

import io
import json
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

import copied_ratchet  # noqa: E402


class RatchetTest(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = Path(tmp.name)
        skills = self.root / "skills"
        for name in ("godot-a", "godot-b", "godot-c"):
            (skills / name).mkdir(parents=True)
            (skills / name / "SKILL.md").write_text("x\n")
        (skills / "copied.json").write_text(json.dumps({"skills": [
            {"name": n, "copied_files": ["SKILL.md"]} for n in ("godot-a", "godot-b", "godot-c")
        ]}))
        self.copied = copied_ratchet.copied_skills(skills / "copied.json")
        self.a = skills / "godot-a" / "SKILL.md"
        self.b = skills / "godot-b" / "SKILL.md"
        self.c = skills / "godot-c" / "SKILL.md"
        self.baseline = self.root / "baseline.json"
        self.baseline.write_text(json.dumps({"chk": {"godot-a": 2, "godot-b": 1}}))

    def apply(self, failures, scanned, update=False):
        out = io.StringIO()
        code = copied_ratchet.apply(
            "chk", "blocks", failures, scanned, self.copied, self.baseline, update, out
        )
        return code, out.getvalue()

    def fail(self, path, n):
        return [(path, f"{path}:{i}: error") for i in range(n)]

    def test_copied_skills_maps_each_file_to_its_skill(self):
        self.assertEqual(self.copied[self.a.resolve()], "godot-a")
        self.assertEqual(len(self.copied), 3)

    def test_counts_at_the_baseline_pass_with_a_summary_only(self):
        code, out = self.apply(self.fail(self.a, 2) + self.fail(self.b, 1), [self.a, self.b, self.c])
        self.assertEqual(code, 0)
        self.assertEqual(out.splitlines(), [
            "chk: 3 copied blocks fail in 2 skills, report only (baseline 3, baseline.json)",
            "  per skill: godot-a 2, godot-b 1",
        ])

    def test_a_count_above_the_baseline_fails_and_lists_that_skill(self):
        code, out = self.apply(self.fail(self.a, 2) + self.fail(self.c, 1), [self.a, self.b, self.c])
        self.assertEqual(code, 1)
        self.assertIn("FAIL chk: godot-c has 1 copied blocks that fail, baseline 0.", out)
        self.assertIn(f"  {self.c}:0: error", out)
        self.assertNotIn(f"{self.a}:0: error", out)
        self.assertIn("godot-b has 0 copied blocks that fail, baseline 1. Lower the baseline", out)

    def test_a_count_below_the_baseline_passes_and_asks_for_a_lower_baseline(self):
        code, out = self.apply(self.fail(self.a, 1) + self.fail(self.b, 1), [self.a, self.b])
        self.assertEqual(code, 0)
        self.assertIn("godot-a has 1 copied blocks that fail, baseline 2. Lower the baseline", out)

    def test_only_scanned_skills_are_compared(self):
        code, out = self.apply(self.fail(self.a, 2), [self.a])
        self.assertEqual(code, 0)
        self.assertNotIn("godot-b", out)
        self.assertIn("(baseline 2,", out)

    def test_update_writes_the_counts_of_the_scanned_skills(self):
        code, _ = self.apply(self.fail(self.c, 4), [self.a, self.c], update=True)
        self.assertEqual(code, 0)
        data = json.loads(self.baseline.read_text())
        # godot-a was scanned with 0 failures: dropped. godot-b was not scanned: kept.
        self.assertEqual(data, {"chk": {"godot-b": 1, "godot-c": 4}})


if __name__ == "__main__":
    unittest.main()
