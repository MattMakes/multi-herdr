"""`intake triage`, GATE 1, `intake report` and `intake run` (spec 4.3-4.5, 5, 10, 11; T-08, T-09, T-18 part).

No LLM role runs: the auditor's audit.jsonl and opaque inference files are canned.
SUP and OPQ cases use canned scan lines in the ticket 05 hit format.
"""

from __future__ import annotations

import json
import unittest
from pathlib import Path

from base import IntakeTestCase

import fixtures
from fixtures import ZWSP

PENDING_EXIT = 14


class TriageCase(IntakeTestCase):
    def run_source(self, files: dict[str, str], expect: int) -> Path:
        src = fixtures.build(self.tmp / f"src-{len(list(self.tmp.iterdir()))}", files)
        r = self.run_intake("run", str(src), "--base", str(self.tmp / "base"))
        self.assertEqual(r.returncode, expect, r.stdout + r.stderr)
        out = json.loads(r.stdout)
        self.assertEqual(out["exit"], expect)
        return Path(out["task_dir"])

    def audit(self, c: Path, *rows) -> None:
        lines = [r if isinstance(r, str) else json.dumps(r) for r in rows]
        (c / "sweep" / "audit.jsonl").write_text("".join(ln + "\n" for ln in lines))

    def triage(self, c: Path, expect: int) -> dict:
        r = self.run_intake("triage", "--task", str(c))
        self.assertEqual(r.returncode, expect, r.stdout + r.stderr)
        return json.loads(r.stdout)

    def add_scan(self, c: Path, *hits) -> None:
        with open(c / "sweep" / "scan.jsonl", "a", encoding="utf-8") as f:
            for i, (rule, sev, file) in enumerate(hits, 900):
                f.write(json.dumps({"id": f"scan-{i:04d}", "rule": rule, "severity": sev, "file": file,
                                    "line": None, "evidence": {"untrusted": "x"}, "malicious": False}) + "\n")

    def read(self, c: Path, rel: str):
        return json.loads((c / rel).read_text())

    def findings(self, c: Path) -> list[dict]:
        return self.read(c, "sweep/findings.json")["findings"]

    def entries(self, c: Path) -> dict[str, dict]:
        return {e["finding"]: e for e in self.read(c, "mitigation.json")["entries"]}

    def scan_hits(self, c: Path) -> list[dict]:
        return [json.loads(ln) for ln in (c / "sweep" / "scan.jsonl").read_text().splitlines() if ln]

    def validate(self, c: Path) -> None:
        for rel in ("sweep/findings.json", "mitigation.json"):
            v = self.run_intake("cr", "validate", "--task", str(c), str(c / rel))
            self.assertEqual(v.returncode, 0, f"{rel}: {v.stdout} {v.stderr}")


PURPOSE = {"kind": "purpose", "skill": "clean-fixture", "purpose": "Counts words in a notes file."}
SKILL_DIR = "skills/clean-fixture"


class GateTest(TriageCase):
    """T-08 and the orchestrator decision on a missing audit.jsonl."""

    def test_missing_audit_gives_pending_audit(self):
        c = self.run_source(fixtures.f_clean(), PENDING_EXIT)
        s = self.read(c, "sweep/summary.json")
        self.assertEqual((s["gate1"], s["audit"], s["reasons"]), ("pending-audit", "missing", []))
        self.assertEqual(self.read(c, "task.json")["gate1"], "pending-audit")
        self.validate(c)
        self.assertEqual(self.triage(c, PENDING_EXIT)["gate1"], "pending-audit")

    def test_missing_audit_with_an_abort_level_hit_aborts(self):
        c = self.run_source(fixtures.f_exfil(), 4)
        s = self.read(c, "sweep/summary.json")
        self.assertEqual((s["gate1"], s["audit"]), ("abort", "missing"))
        self.assertTrue(any(r.startswith("malicious finding: SCR-04#") for r in s["reasons"]), s["reasons"])

    def test_audit_present_gives_pass(self):
        c = self.run_source(fixtures.f_clean(), PENDING_EXIT)
        self.audit(c, PURPOSE)
        out = self.triage(c, 0)
        self.assertEqual((out["gate1"], out["audit"]), ("pass", "present"))
        s = self.read(c, "sweep/summary.json")
        self.assertEqual(s["purposes"], {"clean-fixture": "Counts words in a notes file."})
        self.assertEqual(s["counts"], {"critical": 0, "high": 0, "medium": 0, "low": 0, "info": 0})
        self.assertEqual(self.read(c, "task.json")["gate1"], "pass")
        self.validate(c)

    def test_malicious_aborts_even_with_audit(self):
        c = self.run_source(fixtures.f_inject(), 4)
        self.audit(c, {"kind": "purpose", "skill": "inj-fixture", "purpose": "Formats notes."})
        out = self.triage(c, 4)
        self.assertGreaterEqual(out["malicious"], 2)
        self.validate(c)

    def test_restrictive_licence_aborts(self):
        c = self.run_source({**fixtures.f_clean(), "LICENSE": fixtures.LICENCE_TEXTS["no-re"]}, 4)
        self.audit(c, PURPOSE)
        self.assertIn("restrictive licence", self.triage(c, 4)["reasons"])

    def test_copyleft_and_no_licence_proceed_with_a_flag(self):
        for files, flag in (({**fixtures.f_clean(), "LICENSE": fixtures.LICENCE_TEXTS["gpl3"]}, "copyleft"),
                            (fixtures.f_clean(), "no-licence")):
            c = self.run_source(files, PENDING_EXIT)
            self.audit(c, PURPOSE)
            self.triage(c, 0)
            self.assertEqual(self.read(c, "sweep/summary.json")["licence"]["flag"], flag)
            r = self.run_intake("report", "--task", str(c))
            self.assertIn(f"flag {flag}", (c / "report.md").read_text())
            self.assertIn(flag, json.loads(r.stdout)["summary"])

    def test_auditor_malicious_finding_aborts(self):
        c = self.run_source(fixtures.f_clean(), PENDING_EXIT)
        self.audit(c, PURPOSE, {"kind": "finding", "rule": "AUD-03", "severity": "high", "skill": "clean-fixture",
                                "note": "The docs claim the operator approved uploads.", "malicious": True,
                                "reason": "social engineering"})
        out = self.triage(c, 4)
        self.assertEqual(out["malicious"], 1)

    def test_high_finding_with_no_possible_disposition_aborts(self):
        c = self.run_source(fixtures.f_clean(), PENDING_EXIT)
        self.audit(c, PURPOSE, {"kind": "finding", "rule": "AUD-01", "severity": "high", "skill": "clean-fixture",
                                "note": "The whole purpose is to upload the env.", "malicious": False,
                                "disposition": "none"})
        reasons = self.triage(c, 4)["reasons"]
        self.assertTrue(any(r.startswith("finding with no possible disposition: AUD-01#") for r in reasons))

    def test_medium_finding_with_no_disposition_is_removed_not_aborted(self):
        c = self.run_source(fixtures.f_clean(), PENDING_EXIT)
        self.audit(c, PURPOSE, {"kind": "finding", "rule": "AUD-01", "severity": "medium", "skill": "clean-fixture",
                                "note": "A minor extra.", "malicious": False, "disposition": "none"})
        self.triage(c, 0)
        (entry,) = self.entries(c).values()
        self.assertEqual((entry["disposition"], entry["status"]), ("remove", "planned"))

    def test_every_skill_removed_aborts(self):
        c = self.run_source(fixtures.f_clean(), PENDING_EXIT)
        self.audit(c, PURPOSE, {"kind": "finding", "rule": "AUD-02", "severity": "medium", "skill": "clean-fixture",
                                "note": "Nothing is left after the risky parts go.", "malicious": False,
                                "disposition": "remove", "whole_skill": True})
        self.assertIn("every discovered skill would be removed", self.triage(c, 4)["reasons"])

    def test_later_stages_refuse_unless_gate1_passed(self):
        c = self.run_source(fixtures.f_clean(), PENDING_EXIT)
        for args in (("cr", "index"), ("cr", "init")):
            r = self.run_intake(*args, "--task", str(c))
            self.assertEqual(r.returncode, 2, r.stderr)
            self.assertIn("GATE 1 result is pending-audit", r.stderr)
        self.audit(c, PURPOSE)
        self.triage(c, 0)
        self.assertEqual(self.run_intake("cr", "index", "--task", str(c)).returncode, 0)
        aborted = self.run_source(fixtures.f_exfil(), 4)
        self.assertIn("GATE 1 result is abort", self.run_intake("cr", "index", "--task", str(aborted)).stderr)


class AuditTamperTest(TriageCase):
    """T-09: a role can add or raise, never lower, close or delete a scanner hit."""

    def test_tamper_lines_are_dropped_and_logged(self):
        c = self.run_source(fixtures.f_inject_soft(), PENDING_EXIT)
        hits = self.scan_hits(c)
        self.assertTrue(hits)
        first = hits[0]
        lowered = {"critical": "high", "high": "medium", "medium": "low", "low": "info"}[first["severity"]]
        self.audit(
            c,
            {"kind": "purpose", "skill": "soft-fixture", "purpose": "Writes notes."},
            {"kind": "raise", "target": first["id"], "severity": lowered, "reason": "looks fine"},
            {"kind": "close", "target": first["id"], "reason": "false positive"},
            {"kind": "delete", "id": first["id"]},
            {"kind": "raise", "target": first["id"], "severity": first["severity"], "reason": "x", "status": "closed"},
            "not json {",
            {"kind": "finding", "rule": "SCR-04", "severity": "info", "skill": None, "note": "n", "malicious": False},
        )
        self.triage(c, 0)
        rows = self.findings(c)
        tamper = [f for f in rows if f["rule"] == "AUD-TAMPER"]
        invalid = [f for f in rows if f["rule"] == "AUD-INVALID"]
        self.assertEqual(len(tamper), 4)
        self.assertEqual({f["severity"] for f in tamper}, {"high"})
        self.assertEqual(len(invalid), 2)
        kept = next(f for f in rows if f.get("scan_id") == first["id"])
        self.assertEqual(kept["severity"], first["severity"])
        self.assertEqual(len([f for f in rows if f["source"] == "scan"]), len(hits))
        self.assertEqual(self.read(c, "sweep/summary.json")["dropped_audit_lines"], 6)
        self.validate(c)

    def test_raise_and_add_are_kept(self):
        c = self.run_source(fixtures.f_inject_soft(), PENDING_EXIT)
        first = self.scan_hits(c)[0]
        self.audit(
            c,
            {"kind": "purpose", "skill": "soft-fixture", "purpose": "Writes notes."},
            {"kind": "raise", "target": first["id"], "severity": "critical", "reason": "trigger hijack"},
            {"kind": "finding", "rule": "AUD-02", "severity": "medium", "skill": "soft-fixture",
             "file": "skills/soft-fixture/SKILL.md", "line": 3, "note": "Claims every writing task.",
             "malicious": False},
        )
        self.triage(c, 0)
        rows = self.findings(c)
        raised = next(f for f in rows if f.get("scan_id") == first["id"])
        self.assertEqual(raised["severity"], "critical")
        self.assertEqual(raised["raised"], ["trigger hijack"])
        added = [f for f in rows if f["rule"] == "AUD-02"]
        self.assertEqual(len(added), 1)
        self.assertEqual(self.entries(c)[added[0]["id"]]["disposition"], "rewrite")
        self.assertFalse([f for f in rows if f["rule"].startswith("AUD-T") or f["rule"] == "AUD-INVALID"])


class PlanTest(TriageCase):
    """The spec 5 table, OPQ inference files, and the mitigation entry shape."""

    def test_dispositions_follow_the_table(self):
        c = self.run_source(fixtures.f_clean(), PENDING_EXIT)
        f = f"{SKILL_DIR}/scripts/count.py"
        self.add_scan(c, ("INJ-04", "high", f"{SKILL_DIR}/SKILL.md"), ("SCR-01", "medium", f),
                      ("SCR-05", "critical", f), ("SUP-02", "high", f), ("SUP-04", "high", f),
                      ("SUP-01", "info", f), ("SUP-06", "high", f))
        self.audit(c, PURPOSE)
        self.triage(c, 0)
        got = {self.findings_by_id(c)[k]["rule"]: (e["disposition"], e["status"]) for k, e in self.entries(c).items()}
        self.assertEqual(got, {
            "INJ-04": ("rewrite", "planned"), "SCR-01": ("rewrite", "planned"), "SCR-05": ("remove", "planned"),
            "SUP-02": ("replace", "planned"), "SUP-04": ("remove", "planned"), "SUP-01": ("replace", "planned"),
            "SUP-06": ("remove", "planned"),
        })
        for e in self.entries(c).values():
            self.assertIsNone(e["spec_item"])
            self.assertEqual(e["skill"], "clean-fixture")
            self.assertNotIn("keep", e["disposition"])
        self.validate(c)

    def findings_by_id(self, c: Path) -> dict[str, dict]:
        return {f["id"]: f for f in self.findings(c)}

    def test_opaque_dispositions_come_from_the_auditor(self):
        c = self.run_source(fixtures.f_clean(), PENDING_EXIT)
        self.add_scan(c, ("OPQ-01", "high", f"{SKILL_DIR}/bin/fmt"), ("OPQ-06", "low", f"{SKILL_DIR}/logo.png"))
        self.audit(c, PURPOSE)
        out = self.triage(c, 4)  # no inference file for OPQ-01: a gate failure, not a default
        self.assertTrue(any(r.startswith("finding with no possible disposition: OPQ-01#") for r in out["reasons"]))
        opq = {self.findings_by_id(c)[k]["rule"]: e for k, e in self.entries(c).items()}
        self.assertEqual((opq["OPQ-01"]["status"], opq["OPQ-06"]["disposition"]), ("unresolved", "remove"))
        md = c / "sweep" / "opaque" / "scan-0900.md"
        md.parent.mkdir(exist_ok=True)
        cases = (("**Inferred purpose**: formats files.\n\n**Disposition**: replace\n", 0, "replace"),
                 ("Disposition: replace\nDisposition: remove\n", 0, "remove"),
                 ("Disposition: keep\n", 4, "remove"))
        for text, code, want in cases:
            md.write_text(text)
            self.triage(c, code)
            opq = {self.findings_by_id(c)[k]["rule"]: e for k, e in self.entries(c).items()}
            self.assertEqual(opq["OPQ-01"]["disposition"], want, text)
            self.assertEqual(opq["OPQ-01"]["status"], "unresolved" if code else "planned", text)

    def test_real_scan_sup_and_opq_hits_get_their_dispositions(self):
        """Scan, then triage, on a real tree: the OPQ inference file is sweep/opaque/<scan id>.md."""
        files = {**fixtures.f_clean(), f"{SKILL_DIR}/requirements.txt": "flask==3.0.0\n",
                 f"{SKILL_DIR}/bin/fmt": fixtures.binary(fixtures.MACHO_64, "--format")}
        c = self.run_source(files, 4)  # no inference file for OPQ-01 yet: GATE 1 cannot pass
        hits = {h["rule"]: h for h in self.scan_hits(c)}
        self.assertEqual(set(hits), {"SUP-01", "OPQ-01"})
        opq_id = hits["OPQ-01"]["id"]
        self.assertTrue((c / "sweep" / "opaque" / f"{opq_id}.json").is_file())
        (c / "sweep" / "opaque" / f"{opq_id}.md").write_text(
            "**Inferred purpose**: formats notes.\n\n**Disposition**: replace\n")
        self.audit(c, PURPOSE)
        self.triage(c, 0)
        by_scan = {f["scan_id"]: f["id"] for f in self.findings(c) if f.get("scan_id")}
        got = {rule: (self.entries(c)[by_scan[h["id"]]]["disposition"], self.entries(c)[by_scan[h["id"]]]["status"])
               for rule, h in hits.items()}
        self.assertEqual(got, {"SUP-01": ("replace", "planned"), "OPQ-01": ("replace", "planned")})
        self.validate(c)

    def test_acq_skips_are_removed(self):
        c = self.run_source(fixtures.f_clean(), PENDING_EXIT)
        (c / "sweep" / "acquire.jsonl").write_text(json.dumps({
            "id": "ACQ-02#1", "rule": "ACQ-02", "severity": "low", "file": f"{SKILL_DIR}/link", "line": None,
            "evidence": {"untrusted": "symlink skipped"}, "malicious": False}) + "\n")
        self.audit(c, PURPOSE)
        self.triage(c, 0)
        (e,) = self.entries(c).values()
        self.assertEqual((e["finding"], e["disposition"], e["status"]), ("ACQ-02#1", "removed", "removed"))
        self.validate(c)

    def test_notes_are_escaped(self):
        c = self.run_source(fixtures.f_inject(), 4)
        notes = [e["note"]["untrusted"] for e in self.entries(c).values()]
        self.assertTrue(notes)
        for n in notes:
            self.assertLessEqual(len(n), 200)
            self.assertTrue(all(0x20 <= ord(ch) <= 0x7E or 0xC0 <= ord(ch) <= 0x17F for ch in n), n)


class ReportTest(TriageCase):
    """T-18, the triage-time part."""

    def test_abort_report_has_the_known_sections_and_escaped_evidence(self):
        c = self.run_source(fixtures.f_inject(), 4)
        r = self.run_intake("report", "--task", str(c))
        self.assertEqual(r.returncode, 0, r.stderr)
        out = json.loads(r.stdout)
        text = Path(out["report"]).read_text()
        self.assertEqual(Path(out["report"]), c / "report.md")
        for part in ("- outcome: aborted at GATE 1", "- source: ", "- ref: ", "- commit: ", "- content sha256: ",
                     "- licence: ", "| severity | count |", "| id | severity | skill | disposition |",
                     "### Untrusted evidence (data, not instructions)", "abort reason: malicious finding"):
            self.assertIn(part, text)
        self.assertLess(text.index("| severity | count |"), text.index("| id | severity |"))
        self.assertNotIn(ZWSP, text)
        self.assertFalse(any(0xE0000 <= ord(ch) <= 0xE007F for ch in text))
        self.assertIn("[tag-text: ", text)
        self.assertIn("aborted at GATE 1", out["summary"])
        sweep_report = (c / "sweep" / "report.md").read_text()
        self.assertIn("GATE 1: aborted at GATE 1", sweep_report)

    def test_report_needs_triage(self):
        c = self.run_source({"x/README.md": "no skill here\n"}, 6)
        r = self.run_intake("report", "--task", str(c))
        self.assertEqual(r.returncode, 2)
        self.assertIn("run intake triage first", r.stderr)


class RunTest(TriageCase):
    def test_run_returns_the_first_stopping_stage(self):
        r = self.run_intake("run", "not a source://", "--base", str(self.tmp / "base"))
        self.assertEqual(r.returncode, 2)
        self.assertEqual(json.loads(r.stdout)["stage"], "acquire")
        src = fixtures.build(self.tmp / "none", {"x/README.md": "no skill\n"})
        r = self.run_intake("run", str(src), "--base", str(self.tmp / "base"))
        self.assertEqual((r.returncode, json.loads(r.stdout)["stage"]), (6, "discover"))
        src = fixtures.build(self.tmp / "clean", fixtures.f_clean())
        r = self.run_intake("run", str(src), "--base", str(self.tmp / "base"))
        out = json.loads(r.stdout)
        self.assertEqual((r.returncode, out["stage"]), (PENDING_EXIT, "triage"))
        self.assertEqual(len(r.stdout.strip().splitlines()), 1)
        c = Path(out["task_dir"])
        for rel in ("provenance.json", "skills.json", "sweep/scan.jsonl", "sweep/findings.json", "mitigation.json",
                    "sweep/summary.json", "sweep/report.md"):
            self.assertTrue((c / rel).is_file(), rel)


if __name__ == "__main__":
    unittest.main()
