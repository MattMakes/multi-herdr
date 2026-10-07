"""Tests for scripts/godot/gameplay_scenarios.py (U-25). Run: python3 -m unittest discover -s scripts/godot/tests

The scenario runs need Godot (GODOT_PATH, `godot` on PATH or the macOS app
bundle); without it they are skipped. The gate step `godot_skills` runs this
module, so the GW9 gameplay bundles run headless on every gate.
"""

import shutil
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

import gameplay_scenarios as gs  # noqa: E402

GW9 = ["godot-combat-system", "godot-economy-system", "godot-gameplay-loops", "godot-quest-system"]


class LayoutTest(unittest.TestCase):
    def test_every_gw9_bundle_has_a_fixture(self):
        self.assertEqual(gs.scenarios(), GW9)
        for skill in GW9:
            self.assertTrue((gs.SKILLS / skill / "SKILL.md").is_file(), skill)

    def test_blocks_are_written_by_class_name_or_file_and_ordinal(self):
        with tempfile.TemporaryDirectory() as tmp:
            names = gs.write_blocks(gs.SKILLS / "godot-quest-system", Path(tmp))
            self.assertIn("QuestLog.gd", names)
            self.assertIn("SKILL_4.gd", names)
            self.assertIn("ui-and-world_1.gd", names)
            text = (Path(tmp) / "QuestLog.gd").read_text()
            self.assertTrue(text.startswith("class_name QuestLog\n"))

    def test_two_blocks_with_one_file_name_stop_the_run(self):
        with tempfile.TemporaryDirectory() as tmp:
            skill = Path(tmp) / "skill"
            skill.mkdir()
            block = "```gdscript\nclass_name Twice\nextends Node\n```\n"
            (skill / "SKILL.md").write_text(block + block)
            with self.assertRaises(SystemExit):
                gs.write_blocks(skill, Path(tmp) / "out")


def fake_bundle(root: Path, check: str) -> None:
    """A fixture `godot-fake` with check.gd and a skill with 1 class block."""
    fixture = root / "fixtures" / "godot-fake"
    fixture.mkdir(parents=True)
    shutil.copyfile(gs.FIXTURES / "expect.gd", root / "fixtures" / "expect.gd")
    (fixture / "project.godot").write_text('config_version=5\n[application]\nconfig/name="fake"\n')
    (fixture / "check.gd").write_text(check)
    skill = root / "skills" / "godot-fake"
    skill.mkdir(parents=True)
    (skill / "SKILL.md").write_text("```gdscript\nclass_name FakeThing\nextends RefCounted\n\n"
                                    "func answer() -> int:\n\treturn 42\n```\n")


CHECK = """extends SceneTree
const Expect = preload("res://expect.gd")

func _initialize() -> void:
	var e := Expect.new()
	e.watch(self, 3.0)
	e.check(FakeThing.new().answer() == %s, "the skill block runs")
	%s
	e.finish(self)
"""


@unittest.skipIf(gs.find_godot() is None, "no Godot")
class ScenarioTest(unittest.TestCase):
    def test_gw9_scenarios_pass(self):
        godot = gs.find_godot()
        for skill in GW9:
            with self.subTest(skill=skill):
                r = gs.run_one(godot, skill, timeout=120)
                self.assertTrue(r.ok, f"{skill}: {r.detail}\n{r.output[-4000:]}")

    def run_fake(self, check: str) -> "gs.Result":
        gs.SCRATCH.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=gs.SCRATCH) as tmp:
            root = Path(tmp)
            fake_bundle(root, check)
            with mock.patch.object(gs, "FIXTURES", root / "fixtures"), \
                    mock.patch.object(gs, "SKILLS", root / "skills"):
                return gs.run_one(gs.find_godot(), "godot-fake", timeout=60)

    def test_a_passing_fake_passes(self):
        r = self.run_fake(CHECK % ("42", "pass"))
        self.assertTrue(r.ok, r.output)
        self.assertEqual(r.detail, "scenario: 1 of 1 checks pass")

    def test_a_failed_check_fails(self):
        r = self.run_fake(CHECK % ("41", "pass"))
        self.assertFalse(r.ok)
        self.assertEqual(r.detail, "exit 1")

    def test_a_script_error_fails_even_with_exit_0(self):
        r = self.run_fake(CHECK % ("42", "var n: Node = null\n\tn.get_name()"))
        self.assertFalse(r.ok)
        self.assertIn("error lines, exit 2", r.detail)
        self.assertIn("scenario watchdog", r.output)

    def test_no_summary_fails(self):
        r = self.run_fake("extends SceneTree\nfunc _initialize() -> void:\n\tquit(0)\n")
        self.assertFalse(r.ok)
        self.assertIn("no 'scenario", r.detail)


if __name__ == "__main__":
    unittest.main()
