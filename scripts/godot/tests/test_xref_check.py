"""Tests for scripts/godot/xref_check.py. Run: python3 -m unittest discover -s scripts/godot/tests"""

import contextlib
import io
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

import xref_check  # noqa: E402


class XrefCheckTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        root = Path(self.tmp.name)
        self.skills = root / "skills"
        self.teammates = root / "teammates"
        self.teammates.mkdir()
        (self.teammates / "godot-tech-lead.md").write_text("x")
        a = self.skills / "godot-alpha"
        (a / "references").mkdir(parents=True)
        (self.skills / "godot-beta").mkdir()
        (self.skills / "godot-beta" / "SKILL.md").write_text("# Beta\n")
        (a / "references" / "notes.md").write_text("# Notes\n\n## Godot 4.7 Additions\n")
        (a / "SKILL.md").write_text(
            "# Alpha\n"
            "See godot-beta and `godot-alpha`.\n"  # 2: fine
            "Ask godot-tech-lead. Build godot-cpp.\n"  # 3: teammate, external
            "See godot-gamma for loops.\n"  # 4: broken
            "[n](references/notes.md#godot-47-additions) [f](references/godot-4.7-x.md)\n"  # 5: anchor ok, file broken
            "https://github.com/x/godot-delta and .godot-cache/ and org/godot-eps\n"  # 6: skipped
            "[n](references/notes.md#missing) [w](https://example.com/a)\n"  # 7: broken anchor
            "Old name godot-zeta. <!-- xref-check: allow godot-zeta -->\n"  # 8: allowed
        )

    def tearDown(self):
        self.tmp.cleanup()

    def run_check(self, *args):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = xref_check.main(["--skills", str(self.skills), "--teammates", str(self.teammates), *args])
        return code, out.getvalue().splitlines()

    def test_broken_references(self):
        code, out = self.run_check()
        path = (self.skills / "godot-alpha" / "SKILL.md")
        self.assertEqual(code, 1)
        self.assertEqual(out, [
            f"{path}:4: no skill godot-gamma",
            f"{path}:5: broken link references/godot-4.7-x.md",
            f"{path}:7: broken anchor references/notes.md#missing",
        ])

    def test_clean_skill_passes(self):
        code, out = self.run_check(str(self.skills / "godot-beta"))
        self.assertEqual((code, out), (0, []))

    def test_slug(self):
        self.assertEqual(xref_check.slug("Godot 4.7 Additions"), "godot-47-additions")
        self.assertEqual(xref_check.slug("`AreaLight3D` (Godot 4.7)"), "arealight3d-godot-47")


if __name__ == "__main__":
    unittest.main()
