"""`intake discover` (spec 3.7; T-05, F-variants)."""

from __future__ import annotations

import json
import unittest
from pathlib import Path

from base import IntakeTestCase

import fixtures
from intakelib import discover


class DiscoverCase(IntakeTestCase):
    def acquire_and_discover(self, src: Path):
        r = self.run_intake("acquire", str(src), "--base", str(self.tmp / "base"))
        self.assertEqual(r.returncode, 0, r.stderr)
        c = Path(json.loads(r.stdout)["task_dir"])
        d = self.run_intake("discover", "--task", str(c))
        skills = json.loads((c / "skills.json").read_text()) if (c / "skills.json").exists() else None
        return d, c, skills


class DiscoverTest(DiscoverCase):
    def test_variants_keep_the_claude_copy(self):
        d, c, out = self.acquire_and_discover(fixtures.f_variants(self.tmp / "v"))
        self.assertEqual(d.returncode, 0, d.stderr)
        self.assertEqual(len(out["skills"]), 1)
        s = out["skills"][0]
        self.assertEqual((s["name"], s["rel_path"]), ("pdf-editor", ".claude/skills/pdf-editor"))
        self.assertEqual(s["dropped_variants"], [".cursor/skills/pdf-editor", "skills/pdf-editor"])
        self.assertEqual(s["description"], {"untrusted": "from .claude/skills"})
        self.assertEqual((s["files"], s["bytes"]), (1, len(fixtures.skill_md("pdf-editor", "from .claude/skills"))))
        prov = json.loads((c / "provenance.json").read_text())
        self.assertEqual(prov["skills"], ["pdf-editor"])
        self.assertEqual(json.loads(d.stdout), {"skills": ["pdf-editor"]})

    def test_rank_beats_sort_order(self):
        # ".agents" and "a" sort before ".claude" and "plugin"; the rank must still decide.
        files = {f"{w}/x/SKILL.md": fixtures.skill_md("x") for w in ("a", ".agents/skills", "plugin/skills", "a/skills")}
        src = fixtures.build(self.tmp / "r", files)
        self.assertEqual(discover.discover(src, "r")["skills"][0]["rel_path"], "plugin/skills/x")
        (src / ".claude" / "skills" / "x").mkdir(parents=True)
        (src / ".claude" / "skills" / "x" / "SKILL.md").write_text(fixtures.skill_md("x"))
        self.assertEqual(discover.discover(src, "r")["skills"][0]["rel_path"], ".claude/skills/x")

    def test_skip_dirs(self):
        files = {f"{d}/s-{i}/SKILL.md": fixtures.skill_md(f"s-{i}") for i, d in enumerate(sorted(discover.SKIP_DIRS))}
        files["ok/SKILL.md"] = fixtures.skill_md("ok")
        files["deep/a/b/real/SKILL.md"] = fixtures.skill_md("real")
        files["ok/nested/SKILL.md"] = fixtures.skill_md("nested")  # inside a skill: not searched
        src = fixtures.build(self.tmp / "s", files)
        self.assertEqual([str(p) for p in discover.find(src)], ["deep/a/b/real", "ok"])

    def test_name_normalization(self):
        src = fixtures.build(self.tmp / "n", {
            "My Cool_Skill/SKILL.md": "---\ndescription: x\n---\n",
            "Bad Front/SKILL.md": "---\nname: Not Valid!\n---\n",
            "caf\u00e9-t\u00f6ol/SKILL.md": "no frontmatter\n",
            "\u4e2d\u6587/SKILL.md": "---\nname: \u4e2d\n---\n",
            "\u65e5\u672c/SKILL.md": "---\ndescription: \"quoted \u200b desc\"\n---\n",
            "fm/SKILL.md": "---\nname: good-name\n---\n",
        })
        d, c, out = self.acquire_and_discover(src)
        self.assertEqual(d.returncode, 0, d.stderr)
        names = {s["name"]: s["rel_path"] for s in out["skills"]}
        self.assertEqual(names["my-cool-skill"], "My Cool_Skill")
        self.assertEqual(names["bad-front"], "Bad Front")
        self.assertEqual(names["cafe-tool"], "caf\u00e9-t\u00f6ol")
        self.assertEqual(names["good-name"], "fm")
        # 2 names cannot be normalized: INJ-09, and skill-<n>; rel_path is escaped.
        self.assertEqual(names["skill-1"], "\\u{4E2D}\\u{6587}")
        self.assertEqual(names["skill-2"], "\\u{65E5}\\u{672C}")
        by_name = {s["name"]: s for s in out["skills"]}
        self.assertEqual(by_name["skill-2"]["description"], {"untrusted": "quoted \\u{200B} desc"})
        rows = [json.loads(x) for x in (c / "sweep" / "discover.jsonl").read_text().splitlines()]
        self.assertEqual([r["rule"] for r in rows], ["INJ-09", "INJ-09"])
        self.assertEqual(rows[0]["severity"], "medium")
        for s in out["skills"]:
            self.assertRegex(s["name"], discover.NAME.pattern)

    def test_normalize(self):
        self.assertEqual(discover.normalize("--Hello  World--"), "hello-world")
        self.assertEqual(discover.normalize("a" * 80), "a" * 64)
        self.assertIsNone(discover.normalize("!!!"))

    def test_root_skill_uses_the_source_name(self):
        src = fixtures.build(self.tmp / "Root Skill", {"SKILL.md": "---\ndescription: d\n---\n"})
        d, c, out = self.acquire_and_discover(src)
        self.assertEqual(d.returncode, 0, d.stderr)
        self.assertEqual([(s["name"], s["rel_path"]) for s in out["skills"]], [("root-skill", ".")])

    def test_out_of_scope_files(self):
        src = fixtures.build(self.tmp / "o", {
            "skills/a/SKILL.md": fixtures.skill_md("a") + "Run `../../scripts/used.sh` and see docs/ref.md.\n",
            "scripts/used.sh": "echo used\n",
            "docs/ref.md": "ref\n",
            "scripts/unused.sh": "echo unused\n",
            "README.md": "readme\n",
        })
        d, c, out = self.acquire_and_discover(src)
        self.assertEqual(d.returncode, 0, d.stderr)
        self.assertEqual(out["out_of_scope"], ["README.md", "scripts/unused.sh"])

    def test_no_skill_exits_6(self):
        src = fixtures.build(self.tmp / "e", {"README.md": "nothing\n", "node_modules/x/SKILL.md": "x"})
        d, c, out = self.acquire_and_discover(src)
        self.assertEqual(d.returncode, 6, d.stderr)
        self.assertEqual(out["skills"], [])

    def test_discover_needs_an_acquired_task(self):
        d = self.run_intake("discover", "--task", str(self.tmp))
        self.assertEqual(d.returncode, 2)


if __name__ == "__main__":
    unittest.main()
