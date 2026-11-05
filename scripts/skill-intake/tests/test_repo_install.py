"""`intake install --target repo`, mode detection and `intake candidates`
(spec 8.1-8.4, 9.1; F-repo, T-16, T-17).

Every repo install goes into a git-initialised temp copy of this repo's
bookkeeping files (fixtures.f_repo). No test writes this repo.
"""

from __future__ import annotations

import json
from pathlib import Path

from base import REPO
from test_install import OWNER
from test_verify import CLEAN_SCRIPT, CLEAN_SKILL, VerifyCase, spec

import fixtures
from intakelib import candidates, repo_install, roster, targets
from intakelib import task as tasklib
from intakelib.task import read_json, write_json

SKILL = "pdf-editor"
REPO_SKILL = (CLEAN_SKILL.replace("clean-fixture", SKILL)
              .replace("scripts/tally.py", "the script in references/script-tally.md"))
PERSONA = ("You are the fleet's PDF SPECIALIST. You edit PDF files.\n\nFollow the `pdf-editor` skill.\n\n"
           "Standing rules:\n1. Check every page you change.\n")
GODOT = {
    "godot-gameplay": fixtures.teammate_md("godot-gameplay", "Godot 4 gameplay: combat, inventory, save games.",
                                           "skills: [godot-combat-system, godot-gameplay-loops]\n"
                                           "available_skills: [godot-save]\n"),
    "godot-ui": fixtures.teammate_md("godot-ui", "Godot 4 menus and HUD.",
                                     'skills_when: {"*.csproj": [godot-csharp-ui]}\n'),
    "web-dev": fixtures.teammate_md("web-dev", "Web pages and forms.", "skills:\n  - web-forms\n"),
    "godot-hidden": fixtures.teammate_md("godot-hidden", "Hidden.", "hidden: true\nskills: [godot-x]\n"),
}
ANSWER_INDEPENDENT = {
    f"skills/{SKILL}/SKILL.md", f"skills/{SKILL}/references/script-tally.md", "skills/copied.json",
    "skills/README.md", "crates/horch-core/tests/skills_catalog.rs",
    f"docs/cleanroom/{SKILL}/README.md", f"docs/cleanroom/{SKILL}/1-tally.md",
    f"docs/cleanroom/{SKILL}/provenance.json", f"docs/cleanroom/{SKILL}/sweep.md",
}


class RepoCase(VerifyCase):
    def setUp(self):
        super().setUp()
        self.repo = fixtures.f_repo(self.tmp / "checkout", REPO, roster=GODOT)

    def verified(self) -> tasklib.Task:
        """A task with a repo GATE 2 pass and 1 output skill. The source folder is named OWNER."""
        src = fixtures.build(self.tmp / OWNER, fixtures.f_clean())
        r = self.run_intake("run", str(src), "--base", str(self.tmp / "base"))
        c = Path(json.loads(r.stdout)["task_dir"])
        (c / "sweep" / "audit.jsonl").write_text(
            json.dumps({"kind": "purpose", "skill": "clean-fixture", "purpose": "Edits pdf pages."}) + "\n")
        self.assertEqual(self.run_intake("triage", "--task", str(c)).returncode, 0)
        t = tasklib.load(str(c))
        self.clean_side(t, [spec(SKILL)], "repo")
        self.out(t, {f"{SKILL}/SKILL.md": REPO_SKILL,
                     f"{SKILL}/references/script-tally.md": "# tally\n\n```python\n" + CLEAN_SCRIPT + "```\n"})
        self.review(t, [("spec-tally", "b1")])
        self.verify(t, "repo")
        (t.k / "persona.md").write_text(PERSONA)
        return tasklib.load(str(c))

    def install(self, t: tasklib.Task, mapping: dict | None = None, *extra: str, expect: int = 0) -> dict:
        args = ["install", "--task", str(t.c), "--target", "repo", "--repo", str(self.repo), *extra]
        if mapping is not None:
            path = self.tmp / "mapping.json"
            write_json(path, mapping)
            args += ["--mapping", str(path)]
        r = self.run_intake(*args)
        self.assertEqual(r.returncode, expect, r.stdout + r.stderr)
        return json.loads(r.stdout) if expect == 0 else {}

    def changed(self) -> set[str]:
        """Every path git sees as changed or new in the fixture checkout."""
        out = fixtures.git(self.repo, "ls-files", "--modified", "--others", "--exclude-standard")
        return set(out.splitlines())

    def read(self, rel: str) -> str:
        return (self.repo / rel).read_text()


class ModeTest(RepoCase):
    def test_f_repo_is_repo_mode_and_a_missing_marker_is_standalone(self):
        mode = targets.detect(self.repo)
        self.assertEqual((mode["target"], mode["repo"]), ("repo", str(self.repo)))
        self.assertIn("marker", mode["rule"])
        self.assertEqual(targets.detect(self.repo / "skills")["target"], "repo")
        bare = fixtures.f_repo(self.tmp / "bare", REPO, catalog=False)
        mode = targets.detect(bare)
        self.assertEqual(mode["target"], "standalone")
        self.assertIn("skills_catalog.rs", mode["rule"])
        self.assertEqual(targets.detect(self.tmp / "work")["rule"], "detected: not a git work tree")

    def test_flag_overrides_the_rule(self):
        self.assertEqual(targets.detect(self.repo, "standalone"),
                         {"target": "standalone", "repo": str(self.repo), "rule": "flag: --target standalone"})

    def test_a_linked_worktree_is_repo_mode(self):
        wt = self.tmp / "wt"
        fixtures.git(self.repo, "worktree", "add", "-q", "-b", "intake/x", str(wt))
        mode = targets.detect(wt)
        self.assertEqual((mode["target"], mode["repo"]), ("repo", str(wt.resolve())))

    def test_a_symlinked_marker_is_not_a_regular_file(self):
        p = self.repo / "teammates" / "_template.md"
        p.rename(self.tmp / "t.md")
        p.symlink_to(self.tmp / "t.md")
        self.assertEqual(targets.detect(self.repo)["target"], "standalone")


class CandidatesTest(RepoCase):
    def test_t17_option_3_for_a_godot_skill_with_godot_teammates(self):
        skills = [{"name": "godot-dialogue", "purpose": "Branching dialogue trees."}]
        cands = candidates.find(skills, self.repo / "teammates")
        self.assertEqual([c["teammate"] for c in cands], ["godot-gameplay", "godot-ui"])
        self.assertEqual(candidates.recommend(skills, cands), 3)

    def test_t17_option_1_against_an_empty_roster(self):
        empty = self.tmp / "empty"
        empty.mkdir()
        skills = [{"name": "godot-dialogue", "purpose": "Branching dialogue trees."}]
        self.assertEqual(candidates.find(skills, empty), [])
        self.assertEqual(candidates.recommend(skills, []), 1)

    def test_option_2_when_the_skills_are_a_specialty_of_their_own(self):
        cands = [{"teammate": "godot-gameplay", "skills": ["godot-a"], "reasons": []}]
        two = [{"name": "godot-a", "purpose": ""}, {"name": "pdf-x", "purpose": ""}]
        self.assertEqual(candidates.recommend(two, cands), 2)
        three = [{"name": f"godot-{x}", "purpose": ""} for x in "abc"]
        cands = [{"teammate": "g", "skills": ["godot-a", "godot-b", "godot-c"], "reasons": []}]
        self.assertEqual(candidates.recommend(three, cands), 2)

    def test_brief_words_need_2_shared_non_stop_words(self):
        one = [{"name": "forms", "purpose": "Checks the forms."}]
        self.assertEqual(candidates.find(one, self.repo / "teammates"), [])
        two = [{"name": "forms", "purpose": "Checks web forms."}]
        self.assertEqual([c["teammate"] for c in candidates.find(two, self.repo / "teammates")], ["web-dev"])

    def test_cli_reads_the_task_skills_and_purposes(self):
        t = self.verified()
        r = self.run_intake("candidates", "--task", str(t.c), "--repo", str(self.repo))
        self.assertEqual(r.returncode, 0, r.stderr)
        doc = json.loads(r.stdout)
        self.assertEqual(doc["skills"], [{"name": "clean-fixture", "purpose": "Edits pdf pages."}])
        self.assertEqual((doc["recommend"], doc["candidates"], doc["field"]), (1, [], "available_skills"))


class RepoInstallTest(RepoCase):
    def test_t16_without_a_mapping_writes_only_the_answer_independent_set(self):
        t = self.verified()
        before = self.read("teammates/README.md")
        out = self.install(t)
        self.assertEqual(self.changed(), ANSWER_INDEPENDENT)
        self.assertEqual((out["mapping"], out["teammate"], out["cleanroom"]), (None, None, f"docs/cleanroom/{SKILL}"))
        self.assertEqual(self.read("teammates/README.md"), before)
        entry = [e for e in json.loads(self.read("skills/copied.json"))["skills"] if e["name"] == SKILL]
        self.assertEqual(entry, [{"name": SKILL, "copied_files": [], "verbatim": False}])
        names = [e["name"] for e in json.loads(self.read("skills/copied.json"))["skills"]]
        self.assertEqual(names, sorted(names))
        self.assertIn(f'    "{SKILL}",\n', self.read("crates/horch-core/tests/skills_catalog.rs"))
        self.assertEqual(rust_list(self.read("crates/horch-core/tests/skills_catalog.rs"), "REPO_ORIGINAL"),
                         sorted(rust_list(self.read("crates/horch-core/tests/skills_catalog.rs"), "REPO_ORIGINAL")))
        readme = self.read("skills/README.md")
        self.assertIn(f"| [{SKILL}]({SKILL}/SKILL.md) | own text (clean-room) |", readme)
        self.assertIn(f"[docs/cleanroom/{SKILL}/](../docs/cleanroom/{SKILL}/)", readme)
        self.assertEqual((self.repo / "skills" / SKILL / "SKILL.md").read_text(), REPO_SKILL)
        self.assertFalse(list((self.repo / "skills").glob(".intake-*")))
        self.assertFalse((self.repo / "skills" / SKILL / ".intake-provenance.json").exists())
        cr = self.read(f"docs/cleanroom/{SKILL}/README.md")
        self.assertIn("| [1-tally.md](1-tally.md) | Tally words in a file |", cr)
        self.assertIn("`acme`", cr)
        self.assertNotIn(str(self.tmp), cr + self.read(f"docs/cleanroom/{SKILL}/provenance.json"))
        self.assertIn("b1. Given any text", self.read(f"docs/cleanroom/{SKILL}/1-tally.md"))
        self.assertEqual(read_json(t.c / "task.json")["phase"], "installed-awaiting-mapping")

    def test_t16_option_1_new_teammate(self):
        t = self.verified()
        self.install(t)
        out = self.install(t, {"option": 1, "teammate": "pdf-specialist",
                               "brief_description": "PDF edits: pages, forms, text."})
        self.assertEqual(out["teammate"], "pdf-specialist")
        self.assertEqual(self.changed(), ANSWER_INDEPENDENT | {
            "teammates/pdf-specialist.md", "crates/horch-core/tests/baseline_oracles.rs",
            "teammates/README.md", "docs/skills-and-teams.md", "CHANGELOG.md"})
        mate = self.read("teammates/pdf-specialist.md")
        fm = roster.read(self.repo / "teammates" / "pdf-specialist.md")
        for key, value in {"base": "fleet-worker", "agent": "claude", "model": "sonnet", "effort": "medium",
                           "permission_mode": "auto", "inherit_plugins": "false", "mcp_servers": "{}",
                           "disallowed_tools": "[Agent]", "fallbacks": "[codex-sol]",
                           "disabled_skills": "[herdr-orchestrator, herdr-worker]",
                           "skills": f"[{SKILL}]"}.items():
            self.assertEqual(fm[key], value, key)
        sibling = roster.read(self.repo / "teammates" / "sonnet.md")
        self.assertEqual(fm["allowed_tools"], sibling["allowed_tools"])
        self.assertTrue(mate.endswith("---\n" + PERSONA))
        for rs in ("skills_catalog.rs", "baseline_oracles.rs"):
            listed = rust_list(self.read(f"crates/horch-core/tests/{rs}"), "SKIP_NEW_TEAMMATES")
            self.assertEqual(listed.count("pdf-specialist"), 1)
            self.assertEqual(listed, sorted(listed))
        self.assertIn("| `pdf-specialist` | implementation | claude, sonnet, medium |", self.read("teammates/README.md"))
        self.assertIn(f"### Clean-room intake: {SKILL}", self.read("docs/skills-and-teams.md"))
        log = self.read("CHANGELOG.md").split("## Unreleased\n", 1)[1].split("\n## ", 1)[0]
        self.assertEqual(log.count("`pdf-specialist`"), 1)
        self.assertEqual(read_json(t.c / "task.json")["phase"], "installed")

        # A second run with the same answer changes nothing.
        snap = {p: self.read(p) for p in self.changed() if (self.repo / p).is_file()}
        self.install(t, {"option": 1, "teammate": "pdf-specialist",
                         "brief_description": "PDF edits: pages, forms, text."})
        self.assertEqual({p: self.read(p) for p in snap}, snap)
        self.assertFalse((self.repo / "teammates" / "pdf-specialist-2.md").exists())
        self.assertFalse((self.repo / "skills" / f"{SKILL}-{OWNER}").exists())

    def test_t16_option_2_new_and_existing_teammates(self):
        t = self.verified()
        self.install(t, {"option": 2, "teammate": "pdf-specialist", "brief_description": "PDF edits.",
                         "existing": ["web-dev", "godot-gameplay"]})
        self.assertEqual(self.changed(), ANSWER_INDEPENDENT | {
            "teammates/pdf-specialist.md", "crates/horch-core/tests/baseline_oracles.rs",
            "teammates/README.md", "docs/skills-and-teams.md", "CHANGELOG.md",
            "teammates/web-dev.md", "teammates/godot-gameplay.md"})
        self.assertIn(f"available_skills: [godot-save, {SKILL}]\n", self.read("teammates/godot-gameplay.md"))
        self.assertIn(f"skills:\n  - web-forms\navailable_skills: [{SKILL}]\n", self.read("teammates/web-dev.md"))

    def test_t16_option_3_existing_teammates_only(self):
        t = self.verified()
        self.install(t, {"option": 3, "existing": ["godot-ui"], "field": "skills"})
        self.assertEqual(self.changed(), ANSWER_INDEPENDENT | {
            "teammates/godot-ui.md", "docs/skills-and-teams.md", "CHANGELOG.md"})
        self.assertIn(f"skills: [{SKILL}]\n", self.read("teammates/godot-ui.md"))
        self.assertIn("under `skills`", self.read("docs/skills-and-teams.md"))

    def test_teammate_name_collision_gets_2(self):
        t = self.verified()
        (self.repo / "teammates" / "godot-ui-2.md").write_text("x\n")
        out = self.install(t, {"option": 1, "teammate": "godot-ui", "brief_description": "PDF edits."})
        self.assertEqual(out["teammate"], "godot-ui-3")
        self.assertIn("name: godot-ui-3\n", self.read("teammates/godot-ui-3.md"))

    def test_skill_name_collision_renames_like_standalone(self):
        fixtures.build(self.repo / "skills" / SKILL, {"SKILL.md": fixtures.skill_md(SKILL)})
        t = self.verified()
        out = self.install(t)
        new = f"{SKILL}-{OWNER}"
        self.assertEqual(out["installed"], [{"name": new, "renamed_from": SKILL}])
        self.assertIn(f"name: {new}\n", self.read(f"skills/{new}/SKILL.md"))
        self.assertTrue((self.repo / "docs" / "cleanroom" / new / "README.md").is_file())

    def test_budget_exempt_reason_goes_into_copied_json(self):
        t = self.verified()
        doc = read_json(t.c / "verify.json")
        write_json(t.c / "verify.json", {**doc, "budget_exempt": {SKILL: "a big table"}})
        self.install(t)
        entry = [e for e in json.loads(self.read("skills/copied.json"))["skills"] if e["name"] == SKILL]
        self.assertEqual(entry[0]["budget_exempt"], "a big table")

    def test_bad_mapping_or_persona_exits_2_and_writes_nothing(self):
        t = self.verified()
        bad = [{"option": 4}, {"option": 1, "teammate": "Bad Name", "brief_description": "x"},
               {"option": 1, "teammate": "ok", "brief_description": "x" * 121},
               {"option": 3, "existing": []}, {"option": 3, "existing": ["godot-hidden"]},
               {"option": 3, "existing": ["nobody"]}, {"option": 1, "teammate": "ok", "brief_description": "x",
                                                       "existing": ["web-dev"]},
               {"option": 3, "existing": ["web-dev"], "extra": 1}]
        for m in bad:
            self.install(t, m, expect=2)
        (t.k / "persona.md").write_text(PERSONA + "\u200b\n")
        self.install(t, {"option": 1, "teammate": "ok", "brief_description": "x"}, expect=2)
        (t.k / "persona.md").unlink()
        self.install(t, {"option": 1, "teammate": "ok", "brief_description": "x"}, expect=2)
        self.assertEqual(self.changed(), set())

    def test_repo_install_needs_a_repo_gate2_pass_and_a_checkout(self):
        t = self.verified()
        doc = read_json(t.c / "verify.json")
        write_json(t.c / "verify.json", {**doc, "target": "standalone"})
        self.install(t, expect=2)
        write_json(t.c / "verify.json", doc)
        bare = fixtures.f_repo(self.tmp / "bare", REPO, catalog=False)
        r = self.run_intake("install", "--task", str(t.c), "--target", "repo", "--repo", str(bare))
        self.assertEqual(r.returncode, 2, r.stderr)
        self.assertEqual(self.changed(), set())

    def test_without_target_the_mode_rule_decides(self):
        t = self.verified()
        r = self.run_intake("install", "--task", str(t.c), "--repo", str(self.repo))
        self.assertEqual(r.returncode, 0, r.stderr)
        out = json.loads(r.stdout)
        self.assertEqual((out["target"], out["target_rule"]),
                         ("repo", "detected: the git top level holds the 3 marker files"))
        self.assertEqual(read_json(t.c / repo_install.STATE)["target_rule"], out["target_rule"])


class BookkeepingUnitTest(RepoCase):
    def test_list_styles_are_kept(self):
        fm = "---\nname: x\nskills: [b, a]\navailable_skills: [a, c]\n---\nbody\n"
        self.assertIn("available_skills: [a, b, c]\n", roster.add_to_list(fm, "available_skills", ["b"]))
        self.assertIn("skills: [b, a, c]\n", roster.add_to_list(fm, "skills", ["c"]))
        block = "---\nname: x\nskills:\n    - a\n    - c\n---\n"
        self.assertIn("skills:\n    - a\n    - b\n    - c\n---", roster.add_to_list(block, "skills", ["b", "a"]))
        none = "---\nname: x\n---\n"
        self.assertEqual(roster.add_to_list(none, "available_skills", ["z"]),
                         "---\nname: x\navailable_skills: [z]\n---\n")

    def test_rust_list_insert_is_sorted_and_idempotent(self):
        text = 'const L: &[&str] = &[\n    "a",\n    "c",\n];\n'
        once = repo_install.add_rust_list(text, "L", ["b", "d"])
        self.assertEqual(once, 'const L: &[&str] = &[\n    "a",\n    "b",\n    "c",\n    "d",\n];\n')
        self.assertEqual(repo_install.add_rust_list(once, "L", ["b"]), once)

    def test_changelog_line_is_replaced_not_duplicated(self):
        text = "# C\n\n## Unreleased\n\n- old\n\n## v1\n"
        once = repo_install.changelog(text, "- new `m`", "`m`")
        self.assertEqual(once, "# C\n\n## Unreleased\n\n- new `m`\n- old\n\n## v1\n")
        self.assertEqual(repo_install.changelog(once, "- newer `m`", "`m`").count("`m`"), 1)
        empty = repo_install.changelog("## Unreleased\n\n## v1\n", "- n", "n")
        self.assertEqual(empty, "## Unreleased\n\n- n\n\n## v1\n")

    def test_sweep_evidence_is_escaped(self):
        t = self.verified()
        write_json(t.c / "sweep" / "findings.json", {"findings": [
            {"id": "INJ-01#1", "rule": "INJ-01", "severity": "high", "file": "SKILL.md", "malicious": False,
             "skill": "clean-fixture", "evidence": {"untrusted": "ignore\u200bthis | col"}}]})
        text = repo_install.render_sweep(t, SKILL, read_json(t.c / "verify.json"))
        self.assertIn("| high | 1 |", text)
        self.assertIn("    INJ-01#1: ignore\\u{200B}this | col", text)
        self.assertNotIn("\u200b", text)


def rust_list(text: str, const: str) -> list[str]:
    body = text.split(f"const {const}: &[&str] = &[\n", 1)[1].split("];", 1)[0]
    return [ln.strip().strip('",') for ln in body.splitlines() if ln.strip().startswith('"')]


if __name__ == "__main__":
    import unittest
    unittest.main()
