"""`intake install --target standalone` (spec 8.5, 3.8; F-collide, T-15).

The front half and verify run for real on the F-clean fixture. Every install
goes into a temp `--skills-dir`; no test touches the real ~/.claude/skills.
"""

from __future__ import annotations

import json
import shutil
from pathlib import Path
from unittest import mock

from test_verify import CLEAN_SCRIPT, CLEAN_SKILL, CLEAN_TEST, VerifyCase, spec

import fixtures
from intakelib import install
from intakelib import task as tasklib
from intakelib.task import read_json, write_json

PDF_SKILL = CLEAN_SKILL.replace("clean-fixture", "pdf-editor") + "\nThe pdf-editor skill reads 1 file.\n"
OWNER = "acme"


class InstallCase(VerifyCase):
    def setUp(self):
        super().setUp()
        self.skills = self.tmp / "skills"

    def verified(self, name: str = "pdf-editor") -> tasklib.Task:
        """A task with a standalone GATE 2 pass and 1 output skill `name`. The
        source folder is named OWNER, so the owner of a renamed skill is OWNER."""
        files = fixtures.f_clean()
        src = self.tmp / OWNER
        shutil.rmtree(src, ignore_errors=True)
        fixtures.build(src, files)
        r = self.run_intake("run", str(src), "--base", str(self.tmp / "base"))
        c = Path(json.loads(r.stdout)["task_dir"])
        (c / "sweep" / "audit.jsonl").write_text(
            json.dumps({"kind": "purpose", "skill": "clean-fixture", "purpose": "Formats text."}) + "\n")
        self.assertEqual(self.run_intake("triage", "--task", str(c)).returncode, 0)
        t = tasklib.load(str(c))
        self.clean_side(t, [spec(name)])
        self.out(t, {f"{name}/SKILL.md": PDF_SKILL.replace("pdf-editor", name),
                     f"{name}/scripts/tally.py": CLEAN_SCRIPT,
                     f"{name}/scripts/test_tally.py": CLEAN_TEST})
        self.review(t, [("spec-tally", "b1")])
        self.verify(t)
        return tasklib.load(str(c))

    def install(self, t: tasklib.Task, *extra: str, expect: int = 0) -> dict:
        r = self.run_intake("install", "--task", str(t.c), "--target", "standalone",
                            "--skills-dir", str(self.skills), *extra)
        self.assertEqual(r.returncode, expect, r.stdout + r.stderr)
        return json.loads(r.stdout) if expect == 0 else {}

    def operator_skill(self, name: str = "pdf-editor") -> dict[str, bytes]:
        fixtures.build(self.skills / name, {"SKILL.md": fixtures.skill_md(name, "The operator's own."),
                                            "notes.txt": "keep me\n"})
        return snapshot(self.skills / name)

    def leftovers(self) -> list[str]:
        return sorted(p.name for p in self.skills.iterdir() if p.name.startswith(".intake-tmp-"))


def snapshot(root: Path) -> dict[str, bytes]:
    return {p.relative_to(root).as_posix(): p.read_bytes() for p in sorted(root.rglob("*")) if p.is_file()}


class StandaloneInstallTest(InstallCase):
    def test_clean_install_with_provenance_and_log(self):
        t = self.verified()
        out = self.install(t)
        self.assertEqual(out["installed"], [{"name": "pdf-editor", "renamed_from": None,
                                             "path": str(self.skills / "pdf-editor")}])
        folder = self.skills / "pdf-editor"
        self.assertEqual((folder / "SKILL.md").read_text(), PDF_SKILL)
        self.assertEqual(self.leftovers(), [])
        rec = read_json(folder / install.PROVENANCE)
        self.assertEqual((rec["task_id"], rec["name"], rec["renamed_from"]), (t.id, "pdf-editor", None))
        self.assertEqual(rec["sha256"], install.tree_sha256(folder))
        log = (t.base / install.LOG).read_text().splitlines()
        self.assertEqual([json.loads(ln)["name"] for ln in log], ["pdf-editor"])
        prov = read_json(t.c / "provenance.json")
        self.assertEqual(prov["installed"], [{"name": "pdf-editor", "target": "standalone", "renamed_from": None,
                                              "sha256": rec["sha256"], "path": str(folder)}])
        v = self.run_intake("cr", "validate", "--task", str(t.c), str(t.c / "provenance.json"))
        self.assertEqual(v.returncode, 0, v.stdout + v.stderr)
        self.assertEqual(read_json(t.c / "task.json")["phase"], "installed")

    def test_f_collide_renames_and_keeps_the_original(self):
        before = self.operator_skill()
        t = self.verified()
        out = self.install(t)
        new = f"pdf-editor-{OWNER}"
        self.assertEqual(out["installed"][0]["name"], new)
        self.assertEqual(out["installed"][0]["renamed_from"], "pdf-editor")
        self.assertEqual(snapshot(self.skills / "pdf-editor"), before)
        text = (self.skills / new / "SKILL.md").read_text()
        self.assertIn(f"name: {new}\n", text)
        self.assertIn(f"The {new} skill reads", text)
        self.assertNotIn("pdf-editor ", text)
        self.assertEqual(read_json(t.c / "provenance.json")["installed"][0]["renamed_from"], "pdf-editor")

    def test_second_collision_gets_a_number(self):
        self.operator_skill()
        self.operator_skill(f"pdf-editor-{OWNER}")
        out = self.install(self.verified())
        self.assertEqual(out["installed"][0]["name"], f"pdf-editor-{OWNER}-2")

    def test_resolve_skip_new_name_and_replace(self):
        before = self.operator_skill()
        t = self.verified()
        out = self.install(t, "--resolve", json.dumps({"pdf-editor": "SKIP"}))
        self.assertEqual((out["installed"], out["skipped"]), ([], ["pdf-editor"]))

        out = self.install(t, "--resolve", json.dumps({"pdf-editor": "my-pdf"}))
        self.assertEqual(out["installed"][0]["name"], "my-pdf")
        self.assertIn("name: my-pdf\n", (self.skills / "my-pdf" / "SKILL.md").read_text())

        out = self.install(t, "--resolve", json.dumps({"pdf-editor": "REPLACE"}))
        self.assertEqual(out["installed"][0]["name"], "pdf-editor")
        self.assertEqual((self.skills / "pdf-editor" / "SKILL.md").read_text(), PDF_SKILL)
        self.assertEqual(snapshot(t.c / "replaced" / "pdf-editor"), before)
        self.assertEqual(self.leftovers(), [])

    def test_unresolvable_collision_exits_7_and_writes_nothing(self):
        self.operator_skill()
        self.operator_skill("taken")
        t = self.verified()
        self.install(t, "--resolve", json.dumps({"pdf-editor": "taken"}), expect=7)
        self.install(t, "--resolve", json.dumps({"pdf-editor": "Bad Name"}), expect=7)
        with mock.patch.object(install, "MAX_SUFFIX", 0):
            self.assertRaises(install.Unresolvable, install.plan, ["pdf-editor"], self.skills, OWNER, {},
                              t.c / "replaced")
        (t.c / "replaced" / "pdf-editor").mkdir(parents=True)
        self.install(t, "--resolve", json.dumps({"pdf-editor": "REPLACE"}), expect=7)
        self.assertEqual(sorted(p.name for p in self.skills.iterdir()), ["pdf-editor", "taken"])
        self.assertFalse((t.base / install.LOG).exists())

    def test_t15_copy_failure_leaves_no_temp_folder(self):
        t = self.verified()
        real = shutil.copyfile
        calls = []

        def failing(src, dst, *a, **kw):
            calls.append(src)
            if len(calls) == 2:
                raise OSError("injected copy failure")
            return real(src, dst, *a, **kw)

        with mock.patch.object(install.shutil, "copyfile", failing):
            self.assertRaises(OSError, install.run, t, self.skills, None)
        self.assertEqual(len(calls), 2)
        self.assertEqual(self.leftovers(), [])
        self.assertFalse((self.skills / "pdf-editor").exists())
        self.assertFalse((t.base / install.LOG).exists())

    def test_output_changed_after_verify_is_refused(self):
        t = self.verified()
        (t.k / "out" / "pdf-editor" / "SKILL.md").write_text(PDF_SKILL + "more\n")
        self.install(t, expect=2)
        self.assertEqual(self.leftovers(), [])
        (t.k / "out" / "pdf-editor" / "SKILL.md").write_text(PDF_SKILL)
        (t.k / "out" / "pdf-editor" / "extra.md").write_text("new\n")
        self.install(t, expect=2)
        self.assertFalse((self.skills / "pdf-editor").exists())

    def test_install_needs_a_standalone_gate2_pass(self):
        t = self.verified()
        doc = read_json(t.c / "verify.json")
        write_json(t.c / "verify.json", {**doc, "target": "repo"})
        self.install(t, expect=2)
        write_json(t.c / "verify.json", doc)
        t.manifest["gate2"] = "abort"
        tasklib.set_phase(t, "aborted-gate-2")
        self.install(t, expect=2)
        self.assertFalse(self.skills.exists())

    def test_owner_rules(self):
        gh = {"source": {"kind": "github", "input": "Acme/r", "url": "https://github.com/Acme/r.git"}}
        self.assertEqual(install.owner(gh), "acme")
        self.assertEqual(install.owner({"source": {"kind": "archive", "input": "/x/My_Pack.zip"}}), "my-pack")
        self.assertEqual(install.owner({"source": {"kind": "local", "input": "~/tools/"}}), "tools")
