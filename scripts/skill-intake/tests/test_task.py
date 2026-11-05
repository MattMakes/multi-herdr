"""Task workspace: opaque ids, the 3 side roots, the base and its refusals (T-04)."""

from __future__ import annotations

import os
import re
import stat
import unittest
from pathlib import Path

from base import IntakeTestCase

from intakelib import task as tasklib

ID = re.compile(r"^\d{8}-\d{6}-[0-9a-f]{6}$")


class TaskIdTest(IntakeTestCase):
    def test_id_is_utc_time_plus_6_hex(self):
        self.assertRegex(tasklib.new_id(), ID)

    def test_now_seam_fixes_the_clock(self):
        os.environ["SKILL_INTAKE_NOW"] = "2026-11-05T09:08:07Z"
        a, b = tasklib.new_id(), tasklib.new_id()
        self.assertTrue(a.startswith("20261105-090807-"), a)
        self.assertTrue(b.startswith("20261105-090807-"), b)
        self.assertNotEqual(a, b)
        self.assertEqual(tasklib.now(), "2026-11-05T09:08:07+00:00")

    def test_now_seam_converts_an_offset_to_utc(self):
        os.environ["SKILL_INTAKE_NOW"] = "2026-11-05T10:00:00+02:00"
        self.assertTrue(tasklib.new_id().startswith("20261105-080000-"))

    def test_bad_now_seam_is_an_error(self):
        os.environ["SKILL_INTAKE_NOW"] = "yesterday"
        with self.assertRaises(tasklib.TaskError):
            tasklib.new_id()


class CreateTest(IntakeTestCase):
    def test_create_makes_3_side_roots_under_the_xdg_base(self):
        t = tasklib.create()
        b = self.state / "skill-intake"
        self.assertRegex(t.id, ID)
        self.assertEqual(t.c, b / "contaminated" / t.id)
        self.assertEqual(t.d, b / "drafts" / t.id)
        self.assertEqual(t.k, b / "clean" / t.id)
        for sub in ("quarantine", "sweep", "contaminated", "briefs", "status"):
            self.assertTrue((t.c / sub).is_dir(), sub)
        for sub in ("drafts", "quarantine-cr", "briefs", "status"):
            self.assertTrue((t.d / sub).is_dir(), sub)
        for sub in ("handoff", "out", "verify-log", "briefs", "status"):
            self.assertTrue((t.k / sub).is_dir(), sub)
        for root in (t.c, t.d, t.k):
            self.assertEqual(stat.S_IMODE(root.stat().st_mode), 0o700, root)

    def test_task_id_carries_no_source_name(self):
        t = tasklib.create()
        for root in (t.c, t.d, t.k):
            self.assertEqual(root.name, t.id)

    def test_default_base_without_xdg_is_home_local_state(self):
        del os.environ["XDG_STATE_HOME"]
        t = tasklib.create()
        self.assertEqual(t.base, self.home / ".local" / "state" / "skill-intake")

    def test_explicit_safe_base(self):
        base = self.tmp / "elsewhere"
        t = tasklib.create(str(base))
        self.assertEqual(t.base, base)
        self.assertTrue(t.c.is_dir())

    def test_manifest_round_trips_through_load(self):
        t = tasklib.create()
        again = tasklib.load(str(t.c))
        self.assertEqual(again.id, t.id)
        self.assertEqual((again.c, again.d, again.k), (t.c, t.d, t.k))
        tasklib.set_phase(again, "acquired")
        self.assertEqual(tasklib.load(str(t.c)).manifest["phase"], "acquired")

    def test_load_refuses_a_folder_without_a_manifest(self):
        with self.assertRaises(tasklib.TaskError):
            tasklib.load(str(self.tmp))

    def test_clean_side_must_never_read_c_or_d(self):
        t = tasklib.create()
        self.assertEqual(t.paths_hidden_from("clean"), [t.c, t.d])
        self.assertEqual(t.paths_hidden_from("drafts"), [t.c, t.k])


class FleetModeTest(IntakeTestCase):
    def test_fleet_base_ignores_xdg(self):
        os.environ["SKILL_INTAKE_MODE"] = "fleet"
        t = tasklib.create()
        self.assertEqual(t.base, self.home / ".local" / "state" / "skill-intake")

    def test_fleet_refuses_base(self):
        os.environ["SKILL_INTAKE_MODE"] = "fleet"
        with self.assertRaises(tasklib.BaseRefused) as cm:
            tasklib.create(str(self.tmp / "elsewhere"))
        self.assertIn("fleet", str(cm.exception))
        self.assertFalse((self.tmp / "elsewhere").exists())

    def test_plain_mode_allows_base(self):
        os.environ["SKILL_INTAKE_MODE"] = "plain"
        self.assertEqual(tasklib.create(str(self.tmp / "b")).base, self.tmp / "b")


class UnsafeBaseTest(IntakeTestCase):
    """T-04: a --base inside a skills location, a repo or a synced folder is refused."""

    def assert_refused(self, base: Path) -> None:
        with self.assertRaises(tasklib.BaseRefused, msg=str(base)):
            tasklib.create(str(base))
        self.assertFalse(base.exists(), f"{base} was created")

    def test_inside_home_claude(self):
        self.assert_refused(self.home / ".claude" / "x")

    def test_home_claude_itself(self):
        (self.home / ".claude").mkdir()
        with self.assertRaises(tasklib.BaseRefused):
            tasklib.create(str(self.home / ".claude"))

    def test_inside_home_codex(self):
        self.assert_refused(self.home / ".codex" / "x")

    def test_inside_home_agents(self):
        self.assert_refused(self.home / ".agents" / "x")

    def test_inside_a_project_claude_skills_dir(self):
        self.assert_refused(self.tmp / "proj" / ".claude" / "skills" / "x")

    def test_inside_a_plugin_dir(self):
        plugin = self.tmp / "my-plugin"
        (plugin / ".claude-plugin").mkdir(parents=True)
        self.assert_refused(plugin / "skills" / "x")

    def test_inside_the_current_repo(self):
        repo = self.tmp / "repo"
        (repo / ".git").mkdir(parents=True)
        (repo / "sub").mkdir()
        os.chdir(repo / "sub")
        self.assert_refused(repo / "state")

    def test_a_worktree_git_file_marks_a_repo(self):
        repo = self.tmp / "wt"
        repo.mkdir()
        (repo / ".git").write_text("gitdir: /nowhere\n")
        os.chdir(repo)
        self.assert_refused(repo / "state")

    def test_relative_base_resolves_against_the_current_repo(self):
        repo = self.tmp / "repo"
        (repo / ".git").mkdir(parents=True)
        os.chdir(repo)
        with self.assertRaises(tasklib.BaseRefused):
            tasklib.create("state")

    def test_inside_icloud_drive(self):
        self.assert_refused(self.home / "Library" / "Mobile Documents" / "x")

    def test_inside_cloud_storage(self):
        self.assert_refused(self.home / "Library" / "CloudStorage" / "iCloud" / "x")

    def test_inside_documents_and_desktop(self):
        self.assert_refused(self.home / "Documents" / "x")
        self.assert_refused(self.home / "Desktop" / "x")

    def test_symlink_into_claude_is_refused(self):
        (self.home / ".claude").mkdir()
        link = self.tmp / "innocent"
        link.symlink_to(self.home / ".claude")
        with self.assertRaises(tasklib.BaseRefused):
            tasklib.create(str(link / "x"))

    def test_an_unsafe_xdg_state_home_is_refused_too(self):
        os.environ["XDG_STATE_HOME"] = str(self.home / ".claude" / "state")
        with self.assertRaises(tasklib.BaseRefused):
            tasklib.create()

    def test_a_safe_sibling_name_is_not_refused(self):
        t = tasklib.create(str(self.home / ".claude-notes" / "b"))
        self.assertTrue(t.c.is_dir())


if __name__ == "__main__":
    unittest.main()
