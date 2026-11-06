"""Stage 4 of `intake triage` (spec 2, 4.3-4.5, 5, section 13 Q1 and Q6).

Inputs, all under C:
- sweep/acquire.jsonl, sweep/discover.jsonl, sweep/scan.jsonl: scanner hits;
- sweep/audit.jsonl: the auditor role's lines (schemas/audit-line.schema.json);
- sweep/opaque/<scan id>.md: the auditor's inference file for an OPQ hit;
- skills.json and provenance.json.

The controller rule (spec 2): an auditor line can add a finding or raise a
scanner hit. A line that tries to lower, close or delete a scanner hit is
dropped and becomes an AUD-TAMPER finding (high). Any other line that fails the
schema is dropped and becomes an AUD-INVALID finding (medium).

GATE 1 results:
- `abort`: a malicious finding, a restrictive licence, a critical or high
  finding with no possible disposition, an OPQ hit with no auditor
  disposition, or every skill removed (spec 5);
- `pending-audit`: no abort condition, but sweep/audit.jsonl does not exist,
  so the gate cannot pass yet (orchestrator decision); triage runs again
  after the auditor;
- `pass`: no abort condition, and audit.jsonl exists.
The result is stored as `gate1` in C/task.json. Later stages call
`require_gate1_pass`.
"""

from __future__ import annotations

import json
import re
from pathlib import Path

from . import evidence, schema, task
from .sweep import SEVERITIES

RANK = {s: i for i, s in enumerate(SEVERITIES)}  # 0 is the most severe
SCANNER_FILES = ("sweep/acquire.jsonl", "sweep/discover.jsonl", "sweep/scan.jsonl")
AUDIT = "sweep/audit.jsonl"
OPAQUE = "sweep/opaque"

PASS, ABORT, PENDING = "pass", "abort", "pending-audit"

# SCR rules whose behavior is not legitimate in a skill: the plan removes it.
# Every other SCR rule is rewritten, with access stated as a declared capability.
SCR_REMOVE = frozenset({"SCR-03", "SCR-04", "SCR-05", "SCR-07", "SCR-09", "SCR-10", "SCR-12"})
# SUP rules: replace with own standard-library code, or remove (never keep).
SUP_REPLACE = frozenset({"SUP-01", "SUP-02", "SUP-03", "SUP-07"})
ACQ_REMOVED = frozenset({"ACQ-01", "ACQ-02", "ACQ-03", "ACQ-04", "ACQ-05"})

# A key that names a hit and a verb or field that would close or lower it.
TARGET_KEYS = ("target", "id", "finding", "scan_id", "hit")
DISPOSITION_LINE = re.compile(
    r"^[\s>*_#-]*disposition[\s*_]*:[\s*_`]*([a-z]+)", re.IGNORECASE | re.MULTILINE
)


class TriageError(task.TaskError):
    pass


# ---- inputs -------------------------------------------------------------------

def _jsonl(path: Path) -> list[str]:
    if not path.is_file():
        return []
    return [ln for ln in path.read_text(encoding="utf-8").splitlines() if ln.strip()]


def scanner_hits(c: Path) -> list[dict]:
    """The hits of acquire, discover and scan, in that order."""
    if not (c / "sweep" / "scan.jsonl").is_file():
        raise TriageError(f"no sweep/scan.jsonl in {c}: run intake scan first")
    hits = []
    for rel in SCANNER_FILES:
        for ln in _jsonl(c / rel):
            hits.append(json.loads(ln))
    return hits


def skill_of(file: str | None, skills: list[dict]) -> str | None:
    """The skill whose folder holds `file` (escaped path from the source root)."""
    if not file:
        return None
    best = None
    for s in skills:
        rel = s["rel_path"]
        if (file == rel or file.startswith(rel + "/")) and (best is None or len(rel) > len(best[1])):
            best = (s["name"], rel)
    return best[0] if best else None


# ---- the auditor's lines ---------------------------------------------------------

def _line_schema(kind: str) -> dict:
    root = schema.load_schema("audit-line")
    return {"$ref": f"#/$defs/{kind}", "$defs": root["$defs"]}


def _tamper_reason(row: dict, scanner: dict[str, dict]) -> str | None:
    """Why a line that names a scanner hit is tampering, or None."""
    named = [row[k] for k in TARGET_KEYS if isinstance(row.get(k), str) and row[k] in scanner]
    if not named:
        return None
    hit = scanner[named[0]]
    if row.get("kind") != "raise" or set(row) - {"kind", "target", "severity", "malicious", "reason"}:
        return "the line tries to change a scanner hit other than by a raise"
    sev = row.get("severity")
    if sev in RANK and RANK[sev] > RANK[hit["severity"]]:
        return "the line lowers the severity of a scanner hit"
    if hit.get("malicious") and row.get("malicious") is False:
        return "the line clears the malicious flag of a scanner hit"
    return None


def parse_audit(path: Path, scanner: dict[str, dict]) -> dict:
    """Read audit.jsonl. Returns purposes, added findings, raises and dropped lines.

    `scanner` maps a scanner hit id to the hit. Dropped lines come back as
    {"rule": "AUD-TAMPER"|"AUD-INVALID", "line": n, "detail": text}.
    """
    out = {"present": path.is_file(), "purposes": {}, "findings": [], "raises": [], "dropped": []}
    if not out["present"]:
        return out
    for n, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not raw.strip():
            continue
        try:
            row = json.loads(raw)
        except ValueError:
            out["dropped"].append({"rule": "AUD-INVALID", "line": n, "detail": "not JSON: " + raw})
            continue
        if not isinstance(row, dict):
            out["dropped"].append({"rule": "AUD-INVALID", "line": n, "detail": "not an object: " + raw})
            continue
        why = _tamper_reason(row, scanner)
        if why:
            out["dropped"].append({"rule": "AUD-TAMPER", "line": n, "detail": f"{why}: {raw}"})
            continue
        kind = row.get("kind")
        errors = (schema.validate(row, _line_schema(kind)) if kind in ("purpose", "finding", "raise")
                  else ["$.kind: not purpose, finding or raise"])
        if not errors and kind == "finding" and row["malicious"] and "reason" not in row:
            errors = ["$: a malicious finding needs a reason"]
        if not errors and kind == "raise" and row["target"] not in scanner:
            errors = ["$.target: names no scanner hit"]
        if errors:
            out["dropped"].append({"rule": "AUD-INVALID", "line": n, "detail": f"{errors[0]}: {raw}"})
            continue
        if kind == "purpose":
            out["purposes"][row["skill"]] = evidence.escape(row["purpose"])
        elif kind == "finding":
            out["findings"].append(row)
        else:
            out["raises"].append(row)
    return out


# ---- merge ----------------------------------------------------------------------------

def merge(hits: list[dict], audit: dict, skills: list[dict]) -> list[dict]:
    """The merged findings, each with a schema id `<rule>#<n>` (n counts from 1)."""
    raises: dict[str, list[dict]] = {}
    for r in audit["raises"]:
        raises.setdefault(r["target"], []).append(r)
    rows = []
    for h in hits:
        row = {
            "rule": h["rule"], "severity": h["severity"], "file": h.get("file") or "",
            "malicious": bool(h.get("malicious")), "source": "scan",
            "skill": skill_of(h.get("file"), skills), "scan_id": h["id"],
            "evidence": h.get("evidence") or evidence.untrusted(""),
        }
        if isinstance(h.get("line"), int):
            row["line"] = h["line"]
        for r in raises.get(h["id"], []):
            if RANK[r["severity"]] < RANK[row["severity"]]:
                row["severity"] = r["severity"]
            if r.get("malicious"):
                row["malicious"] = True
            row.setdefault("raised", []).append(evidence.escape(r["reason"]))
        rows.append(row)
    for f in audit["findings"]:
        row = {
            "rule": f["rule"], "severity": f["severity"], "file": evidence.escape(f.get("file", "")),
            "malicious": f["malicious"], "source": "audit", "skill": f["skill"],
            "evidence": evidence.untrusted(f["note"]),
        }
        if "line" in f:
            row["line"] = f["line"]
        if "reason" in f:
            row["reason"] = evidence.escape(f["reason"])
        if "disposition" in f:
            row["audit_disposition"] = f["disposition"]
        if f.get("whole_skill"):
            row["whole_skill"] = True
        rows.append(row)
    for d in audit["dropped"]:
        rows.append({
            "rule": d["rule"], "severity": "high" if d["rule"] == "AUD-TAMPER" else "medium",
            "file": AUDIT, "line": d["line"], "malicious": False, "source": "audit", "skill": None,
            "evidence": evidence.untrusted(d["detail"]),
        })
    return [{"id": f"{r['rule']}#{i}", **r} for i, r in enumerate(rows, 1)]


# ---- plan -------------------------------------------------------------------------------

def opaque_disposition(c: Path, scan_id: str | None) -> str | None:
    """`replace` or `remove` from sweep/opaque/<scan id>.md, or None.

    Several disposition lines that disagree give `remove` (the stricter one).
    An unknown word gives None.
    """
    if not scan_id or not re.fullmatch(r"[A-Za-z0-9#-]{1,40}", scan_id):
        return None
    path = c / OPAQUE / f"{scan_id}.md"
    if not path.is_file():
        return None
    words = {m.lower() for m in DISPOSITION_LINE.findall(path.read_text(encoding="utf-8", errors="replace"))}
    if not words or words - {"replace", "remove"}:
        return None
    return "remove" if "remove" in words else "replace"


def plan_one(f: dict, c: Path) -> tuple[str, str]:
    """(disposition, status) for one finding, by the spec 5 table."""
    rule = f["rule"]
    family = rule.split("-", 1)[0]
    if rule in ACQ_REMOVED:
        return "removed", "removed"
    if family == "INJ":
        return "rewrite", "planned"
    if family == "SCR":
        return ("remove" if rule in SCR_REMOVE else "rewrite"), "planned"
    if family == "SUP":
        return ("replace" if rule in SUP_REPLACE else "remove"), "planned"
    if family == "OPQ":
        d = opaque_disposition(c, f.get("scan_id"))
        if d:
            return d, "planned"
        if rule == "OPQ-06":
            return "remove", "planned"
        return "remove", "unresolved"  # a missing auditor disposition fails GATE 1
    if family == "AUD":
        d = f.get("audit_disposition", "rewrite")
        if d == "none":
            return "remove", ("unresolved" if RANK[f["severity"]] <= RANK["high"] else "planned")
        return d, "planned"
    return "remove", "unresolved"


def plan(findings: list[dict], c: Path) -> list[dict]:
    entries = []
    for f in findings:
        disposition, status = plan_one(f, c)
        entries.append({
            "finding": f["id"], "severity": f["severity"], "skill": f["skill"],
            "disposition": disposition, "spec_item": None,
            "note": evidence.untrusted(f["evidence"]["untrusted"]), "status": status,
        })
    return entries


# ---- GATE 1 ----------------------------------------------------------------------------

def gate(findings: list[dict], entries: list[dict], licence: dict, skills: list[dict],
         audit_present: bool) -> tuple[str, list[str]]:
    """(result, abort reasons). Reasons are fixed text plus finding ids."""
    reasons = []
    malicious = [f["id"] for f in findings if f["malicious"]]
    if malicious:
        reasons.append("malicious finding: " + ", ".join(malicious))
    if (licence or {}).get("class") == "restrictive":
        reasons.append("restrictive licence")
    unresolved = [e["finding"] for e in entries if e["status"] == "unresolved"]
    if unresolved:
        reasons.append("finding with no possible disposition: " + ", ".join(unresolved))
    removed = {f["skill"] for f in findings
               if f.get("whole_skill") and f.get("audit_disposition") == "remove"}
    names = {s["name"] for s in skills}
    if not names or names <= removed:
        reasons.append("every discovered skill would be removed")
    if reasons:
        return ABORT, reasons
    return (PASS if audit_present else PENDING), []


def summary(findings: list[dict], entries: list[dict], result: str, reasons: list[str],
            audit: dict, licence: dict, skills: list[dict]) -> dict:
    counts = {s: sum(1 for f in findings if f["severity"] == s) for s in SEVERITIES}
    per_skill = {}
    for s in skills:
        mine = [f for f in findings if f["skill"] == s["name"]]
        per_skill[s["name"]] = {sev: sum(1 for f in mine if f["severity"] == sev) for sev in SEVERITIES}
    return {
        "gate1": result, "reasons": reasons,
        "audit": "present" if audit["present"] else "missing",
        "findings": len(findings), "counts": counts,
        "malicious": sum(1 for f in findings if f["malicious"]),
        "unresolved": sum(1 for e in entries if e["status"] == "unresolved"),
        "dropped_audit_lines": len(audit["dropped"]),
        "licence": {"class": (licence or {}).get("class"), "flag": (licence or {}).get("flag")},
        "skills": per_skill, "purposes": audit["purposes"],
    }


# ---- the stage ----------------------------------------------------------------------------

def run(t: task.Task) -> dict:
    """Run triage on a task. Writes findings.json, mitigation.json, summary.json
    and the gate result in task.json. Returns the summary."""
    c = t.c
    if not (c / "skills.json").is_file() or not (c / "provenance.json").is_file():
        raise TriageError(f"no skills.json or provenance.json in {c}: run intake discover first")
    skills = task.read_json(c / "skills.json")["skills"]
    prov = task.read_json(c / "provenance.json")
    hits = scanner_hits(c)
    audit = parse_audit(c / AUDIT, {h["id"]: h for h in hits})
    findings = merge(hits, audit, skills)
    entries = plan(findings, c)
    result, reasons = gate(findings, entries, prov.get("licence"), skills, audit["present"])
    task.write_json(c / "sweep" / "findings.json", {"findings": findings})
    task.write_json(c / "mitigation.json", {"entries": entries})
    s = summary(findings, entries, result, reasons, audit, prov.get("licence"), skills)
    task.write_json(c / "sweep" / "summary.json", s)
    t.manifest["gate1"] = result
    task.set_phase(t, {PASS: "triaged", ABORT: "aborted-gate-1", PENDING: "pending-audit"}[result])
    return s


def require_gate1_pass(t: task.Task) -> None:
    """Later stages call this: they refuse a task whose GATE 1 result is not pass."""
    result = t.manifest.get("gate1")
    if result != PASS:
        raise task.TaskError(
            f"task {t.id}: GATE 1 result is {result or 'not run'}, not pass: "
            "run intake triage after the auditor writes sweep/audit.jsonl"
        )
