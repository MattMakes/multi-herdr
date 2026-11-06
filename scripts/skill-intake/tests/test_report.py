"""`intake report`, the full report of spec 11 (T-18, ticket 13).

One report per outcome kind, each built from a canned task dir: the front half
runs for real on a fixture source; the auditor lines, the clean side, the role
log and the repo are canned.
"""

from __future__ import annotations

import json
from pathlib import Path

from test_install import InstallCase
from test_repo_install import SKILL, RepoCase
from test_triage import PENDING_EXIT, TriageCase

import fixtures
from fixtures import ZWSP
from intakelib import evidence
from intakelib.task import read_json, write_json

SECTIONS = ("## Source", "## Mode and install targets", "## Findings", "## Opaque artifacts", "## Removed features",
            "## Accepted capabilities", "## Leakage and confinement", "### Incidents", "### Scripts left unverified",
            "### Lost roles")


class ReportMixin:
    def report(self, t_or_c, *extra: str) -> tuple[str, dict]:
        c = getattr(t_or_c, "c", t_or_c)
        r = self.run_intake("report", "--task", str(c), *extra)
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        out = json.loads(r.stdout)
        self.assertEqual(Path(out["report"]), c / "report.md")
        return (c / "report.md").read_text(), out

    def check_shape(self, text: str, out: dict) -> None:
        """Every section is there; evidence sits only in the one evidence block; the summary is 1 paragraph."""
        for part in SECTIONS:
            self.assertIn(part, text)
        self.assertEqual(text.count(evidence.HEADING), 1 if evidence.HEADING in text else 0)
        if evidence.HEADING in text:
            before, block = text.split(evidence.HEADING)
            self.assertTrue(all(ln.startswith("    ") or not ln for ln in block.strip("\n").splitlines()))
            self.assertFalse([ln for ln in before.splitlines() if ln.startswith("    ")])
        self.assertNotIn("\n", out["summary"])
        self.assertIn(out["report"], out["summary"])
        self.assertFalse(any(ord(ch) > 0x17F for ch in text))


class OutcomeTest(ReportMixin, InstallCase):
    def test_installed(self):
        t = self.verified()
        self.install(t)
        text, out = self.report(t)
        self.check_shape(text, out)
        self.assertIn("- outcome: installed with flags", text)  # the script is unverified (no sandbox)
        self.assertIn("- script left unverified: pdf-editor/scripts/tally.py", text.replace("flag: ", ""))
        self.assertIn("- installed: pdf-editor (standalone)", text)
        self.assertIn("- mode: plain", text)
        self.assertIn("- install target rule: flag: --target standalone", text)
        self.assertIn("| scripts | unverified |", text)
        self.assertIn("- pdf-editor/scripts/tally.py", text)
        self.assertIn("installed with flags", out["summary"])
        self.assertIn("pdf-editor (standalone)", out["summary"])

    def test_installed_clean_has_no_flags_and_lists_renames(self):
        t = self.verified()
        doc = read_json(t.c / "verify.json")
        write_json(t.c / "verify.json", {**doc, "scripts": []})
        summary = read_json(t.c / "sweep" / "summary.json")
        write_json(t.c / "sweep" / "summary.json", {**summary, "licence": {"class": "permissive", "flag": None}})
        self.operator_skill()
        self.install(t)
        text, out = self.report(t)
        self.assertIn("- outcome: installed\n", text)
        self.assertIn("renamed from pdf-editor", text)
        self.assertNotIn("- flag:", text)

    def test_aborted_at_gate_2(self):
        t = self.clean_task()
        self.review(t, [("spec-tally", "b1")], result="fail")
        self.verify(t, expect=5)
        text, out = self.report(t)
        self.check_shape(text, out)
        self.assertIn("- outcome: aborted at GATE 2", text)
        self.assertIn("- abort reason: review-coverage", text)
        self.assertIn("| review-coverage | fail |", text)
        self.assertIn("installed: nothing", text)

    def test_stopped_before_install(self):
        t = self.clean_task()
        text, out = self.report(t)
        self.check_shape(text, out)
        self.assertIn("- outcome: stopped before install (GATE 1 passed; GATE 2 has not run)", text)

    def test_a_lost_role_aborts_the_run(self):
        t = self.clean_task()
        with open(t.c / "roles.jsonl", "w") as f:
            for ln in ({"kind": "start", "id": "writer-1#1", "role": "writer", "worker": "w-1", "brief": "b",
                        "attempt": 1, "mode": "fleet"},
                       {"kind": "end", "id": "writer-1#1", "state": "lost"},
                       {"kind": "start", "id": "writer-1#2", "role": "writer", "worker": "w-2", "brief": "b",
                        "attempt": 2, "mode": "fleet"},
                       {"kind": "end", "id": "writer-1#2", "state": "lost"},
                       {"kind": "abort", "brief": "writer-1.md", "role": "writer", "reason": "brief writer-1.md was lost 2 times"}):
                f.write(json.dumps(ln) + "\n")
        text, out = self.report(t)
        self.check_shape(text, out)
        self.assertIn("- outcome: aborted for a lost role", text)
        self.assertIn("- role writer, worker w-1, attempt 1", text)
        self.assertIn("- role writer, worker w-2, attempt 2", text)
        self.assertIn("- mode: fleet", text)
        self.assertIn("guard hook: not active", text)
        self.assertIn("lost 2 times", out["summary"])


class Gate1Test(ReportMixin, TriageCase):
    def test_aborted_at_gate_1_keeps_hostile_text_in_the_evidence_block(self):
        c = self.run_source(fixtures.f_inject(), 4)
        text, out = self.report(c)
        self.check_shape(text, out)
        self.assertIn("- outcome: aborted at GATE 1", text)
        self.assertIn("[tag-text: ", text)
        self.assertNotIn(ZWSP, text)
        self.assertNotIn("[tag-text", out["summary"])
        self.assertIn("malicious finding", out["summary"])

    def test_pending_audit(self):
        c = self.run_source(fixtures.f_clean(), PENDING_EXIT)
        text, out = self.report(c)
        self.check_shape(text, out)
        self.assertIn("- outcome: pending audit", text)

    def test_opaque_artifact_has_purpose_confidence_and_disposition(self):
        c = self.run_source({**fixtures.f_clean(), "skills/clean-fixture/bin/tool.bin": b"\x7fELF\x02\x01\x01\x00" + b"\x00" * 64},
                            4)
        hit = next(h for h in self.scan_hits(c) if h["rule"].startswith("OPQ-"))
        (c / "sweep" / "opaque" / f"{hit['id']}.md").write_text(
            "Inferred purpose: speeds up word counting.\nConfidence: High\nDisposition: replace\n")
        self.audit(c, {"kind": "purpose", "skill": "clean-fixture", "purpose": "Counts words."})
        self.triage(c, 0)
        text, out = self.report(c)
        self.check_shape(text, out)
        self.assertRegex(text, r"\| OPQ-\d+#\d+ \| clean-fixture \| high \| replace \|")
        self.assertIn("inferred purpose: speeds up word counting.", text.split(evidence.HEADING)[1])
        self.assertNotIn("speeds up", text.split(evidence.HEADING)[0])

    def test_audit_tamper_and_invalid_lines_write_one_incident_each(self):
        c = self.run_source(fixtures.f_clean(), PENDING_EXIT)
        self.add_scan(c, ("SCR-01", "high", "skills/clean-fixture/SKILL.md"))
        self.audit(c, PURPOSE_LINE, {"kind": "raise", "target": "scan-0900", "severity": "low"}, "not json at all")
        self.triage(c, 0)
        text, out = self.report(c)
        inc_dir = c / "contaminated" / "incidents"
        files = sorted(inc_dir.glob("inc-*.json"))
        self.assertEqual(len(files), 2)
        checks = sorted(read_json(p)["checks"][0] for p in files)
        self.assertEqual(checks, ["AUD-INVALID", "AUD-TAMPER"])
        for p in files:
            v = self.run_intake("cr", "validate", "--task", str(c), str(p))
            self.assertEqual(v.returncode, 0, v.stdout + v.stderr)
            self.assertIn(read_json(p)["incident_id"], text)
        self.report(c)  # a second report writes no new file
        self.assertEqual(sorted(inc_dir.glob("inc-*.json")), files)
        self.assertIn("| AUD-TAMPER | escalated-to-user |", text)


PURPOSE_LINE = {"kind": "purpose", "skill": "clean-fixture", "purpose": "Counts words in a notes file."}


class RepoReportTest(ReportMixin, RepoCase):
    def test_repo_report_has_branch_commits_gate_mapping_and_push_pr(self):
        t = self.verified()
        self.install(t)
        text, _ = self.report(t)
        self.assertIn("- mapping answer: not given yet", text)
        self.assertIn("- flag: mapping answer not given", text)
        self.install(t, {"option": 1, "teammate": "pdf-specialist", "brief_description": "PDF edits."})
        fixtures.git(self.repo, "add", "-A")
        fixtures.git(self.repo, "commit", "-q", "-m", "Skills: add pdf-editor")
        self.install(t, {"option": 3, "existing": ["godot-ui"], "field": "skills"})
        text, out = self.report(t, "--gate-result", "GATE GREEN")
        self.check_shape(text, out)
        self.assertIn("## Multi-herdr", text)
        self.assertIn("- branch: fleet/pdf-editor", text)
        self.assertIn("- gate result: GATE GREEN", text)
        self.assertIn("- mapping answer: option 3, teammate -, existing godot-ui, field skills", text)
        self.assertIn("run `/push-pr` to publish", text)
        written = text.split("- files written:")[1].split("- files created")[0]
        for rel in (f"skills/{SKILL}", f"docs/cleanroom/{SKILL}/README.md", "teammates/godot-ui.md", "CHANGELOG.md"):
            self.assertIn(rel, written)
        self.assertNotIn("teammates/pdf-specialist.md", written)
        self.assertIn("teammates/pdf-specialist.md", text.split("files removed or reverted by a later mapping:")[1].split("- next step")[0])
        self.assertIn("commit ", text.split(evidence.HEADING)[1])
        self.assertIn("Skills: add pdf-editor", text.split(evidence.HEADING)[1])
        self.assertIn("(repo)", out["summary"])
        self.assertIn("- install target rule: ", text)
