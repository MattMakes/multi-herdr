"""Tests for scripts/godot/rename.py. Run: python3 -m unittest discover -s scripts/godot/tests"""

import contextlib
import hashlib
import io
import json
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

import rename  # noqa: E402

FIXTURE = HERE / "fixtures" / "rename"


class RenameTest(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp)
        self.upstream = self.tmp / "upstream"
        shutil.copytree(FIXTURE, self.upstream)
        # Dotfiles are made here, not committed: git ignores some of them.
        (self.upstream / "skills" / "state-machine" / ".DS_Store").write_bytes(b"\0")
        (self.upstream / "skills" / ".hidden").mkdir()
        (self.upstream / "skills" / ".hidden" / "SKILL.md").write_text("---\nname: x\n---\n")
        self.out = self.tmp / "out"

    def run_rename(self, *args):
        stdout = io.StringIO()
        with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(io.StringIO()):
            code = rename.main([str(self.upstream / "skills"), *args, "--out", str(self.out), "--revision", "abc123"])
        return code, stdout.getvalue()

    def test_copies_and_renames(self):
        code, _ = self.run_rename("state-machine", "godot-ui")
        self.assertEqual(code, 0)
        self.assertEqual(sorted(p.name for p in self.out.iterdir()), ["godot-state-machine", "godot-ui"])
        files = sorted(p.relative_to(self.out).as_posix() for p in self.out.rglob("*") if p.is_file())
        self.assertEqual(files, [
            "godot-state-machine/LICENSE",
            "godot-state-machine/SKILL.md",
            "godot-state-machine/references/more.md",
            "godot-ui/LICENSE",
            "godot-ui/SKILL.md",
        ])
        self.assertEqual(
            (self.out / "godot-ui" / "LICENSE").read_bytes(), (self.upstream / "LICENSE").read_bytes()
        )

    def test_rewrites_references_but_not_description(self):
        self.run_rename("state-machine")
        text = (self.out / "godot-state-machine" / "SKILL.md").read_text()
        self.assertIn("name: godot-state-machine\n", text)
        self.assertIn(
            "description: Use with **godot-ui** and `state-machine` — the description is never rewritten\n", text
        )
        body = text.split("\n---\n", 1)[1]
        for expected in [
            "> **Related skills:** **godot-ui** for menus, **godot-save-load** for persistence.",
            "Invoke `godot-ui` or any `godot-*` skill.",
            "See `godot-state-machine` and `skills/godot-save-load/SKILL.md` (see godot-save-load skill).",
            "Install path: ~/.config/skills/godot-save-load/SKILL.md",
            "    godot-state-machine/\n",
            "Not references: save-load-extra, my_state-machine, `save-loader`, **Save-load**.",
        ]:
            self.assertIn(expected, body)
        more = (self.out / "godot-state-machine" / "references" / "more.md").read_text()
        self.assertEqual(more, "Deeper notes. See the **godot-state-machine** skill.\n")

    def test_skills_not_copied_are_still_in_the_map(self):
        self.run_rename("godot-ui")
        self.assertIn("See **godot-state-machine**.", (self.out / "godot-ui" / "SKILL.md").read_text())

    def test_prints_sha256_of_each_upstream_file(self):
        _, out = self.run_rename("godot-ui")
        lic = hashlib.sha256((self.upstream / "LICENSE").read_bytes()).hexdigest()
        skill = hashlib.sha256((self.upstream / "skills" / "godot-ui" / "SKILL.md").read_bytes()).hexdigest()
        self.assertEqual(out.splitlines(), [f"{skill}  skills/godot-ui/SKILL.md", f"{lic}  LICENSE"])

    def test_json_sources(self):
        _, out = self.run_rename("godot-ui", "--json")
        sources = json.loads(out[out.index("{"):])["godot-ui"]
        self.assertEqual([s["path"] for s in sources], ["skills/godot-ui/SKILL.md", "LICENSE"])
        self.assertEqual({s["revision"] for s in sources}, {"abc123"})
        self.assertEqual({s["license"] for s in sources}, {"MIT"})

    def test_dry_run_writes_nothing_and_shows_diff(self):
        code, out = self.run_rename("state-machine", "--dry-run")
        self.assertEqual(code, 0)
        self.assertFalse(self.out.exists())
        self.assertIn("-name: state-machine\n+name: godot-state-machine\n", out)

    def test_existing_target_needs_force(self):
        self.run_rename("godot-ui")
        own = self.out / "godot-ui" / "references" / "own.md"
        own.parent.mkdir()
        own.write_text("mine\n")
        code, _ = self.run_rename("godot-ui")
        self.assertEqual(code, 1)
        code, _ = self.run_rename("godot-ui", "--force")
        self.assertEqual(code, 0)
        self.assertEqual(own.read_text(), "mine\n")

    def test_unknown_skill_is_an_error(self):
        code, _ = self.run_rename("no-such-skill")
        self.assertEqual(code, 2)


if __name__ == "__main__":
    unittest.main()
