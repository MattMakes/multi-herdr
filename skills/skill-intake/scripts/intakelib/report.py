"""The report (spec 11), after any outcome.

Every piece of source text goes through the evidence-escape function, and
evidence appears only in the one "Untrusted evidence" block at the end, never
in a table cell or a bullet. `write` also writes one incident file for each
AUD-TAMPER and AUD-INVALID finding (ticket 13).
"""

from __future__ import annotations

import hashlib
import re
from pathlib import Path

from . import evidence, guard, handoff, repo_install, roles, sandbox, task, verify
from .sweep import SEVERITIES

OUTCOME = {
    "pass": "passed GATE 1 (so far)",
    "abort": "aborted at GATE 1",
    "pending-audit": "pending audit (GATE 1 cannot pass until the auditor writes sweep/audit.jsonl)",
}


def _cell(value) -> str:
    """A table cell: escaped, with no pipe and no backtick."""
    text = "-" if value is None or value == "" else evidence.escape(str(value))
    return text.replace("|", "\\u{007C}").replace("`", "\\u{0060}")


def source_section(prov: dict) -> list[str]:
    src = prov.get("source") or {}
    lic = prov.get("licence") or {}
    return [
        "## Source",
        "",
        f"- source: {_cell(src.get('input'))}",
        f"- ref: {_cell(src.get('ref'))}",
        f"- commit: {_cell(src.get('commit'))}",
        f"- archive sha256: {_cell(prov.get('archive_sha256'))}",
        f"- content sha256: {_cell(prov.get('content_sha256'))}",
        f"- licence: {_cell(lic.get('spdx'))} (class {_cell(lic.get('class'))}, flag {_cell(lic.get('flag'))})",
        "",
    ]


def finding_evidence(findings: list[dict]) -> list[str]:
    out = []
    for f in findings:
        where = f["file"] + (f":{f['line']}" if isinstance(f.get("line"), int) else "")
        out.append(f"{f['id']} {where}: {f['evidence']['untrusted']}")
    return out


def findings_section(findings: list[dict], entries: list[dict], vstatus: dict[str, str] | None = None) -> list[str]:
    """The counts and the rows. The evidence goes in the report's one evidence block."""
    by_id = {e["finding"]: e for e in entries}
    lines = ["## Findings", "", "| severity | count |", "|---|---|"]
    for s in SEVERITIES:
        lines.append(f"| {s} | {sum(1 for f in findings if f['severity'] == s)} |")
    lines += ["", "| id | severity | skill | disposition | status | verify |", "|---|---|---|---|---|---|"]
    for f in findings:
        e = by_id.get(f["id"], {})
        lines.append(
            f"| {_cell(f['id'])}{' (malicious)' if f['malicious'] else ''} | {f['severity']} | "
            f"{_cell(f.get('skill'))} | {_cell(e.get('disposition'))} | {_cell(e.get('status'))} | {_cell((vstatus or {}).get(f['id'], 'pending'))} |"
        )
    lines.append("")
    return lines


def sweep_report(summary: dict, findings: list[dict], entries: list[dict]) -> str:
    """sweep/report.md: the gate result and the findings."""
    lines = ["# Sweep report", "", f"- GATE 1: {OUTCOME[summary['gate1']]}",
             f"- auditor: {summary['audit']}"]
    lines += [f"- abort reason: {_cell(r)}" for r in summary["reasons"]]
    lines.append("")
    lines += findings_section(findings, entries)
    if findings:
        lines.append(evidence.markdown(finding_evidence(findings)))
    return "\n".join(lines).rstrip("\n") + "\n"




# ---- inputs the report reads ---------------------------------------------------------

def _json(path: Path, default):
    return task.read_json(path) if path.is_file() else default


def write_incidents(t: task.Task, findings: list[dict]) -> None:
    """One incident file for each AUD-TAMPER and AUD-INVALID finding. The id comes from the
    task, the rule and the audit line, so a second report writes nothing new."""
    for f in findings:
        if f["rule"] not in ("AUD-TAMPER", "AUD-INVALID"):
            continue
        key = f"{t.id}:{f['rule']}:{f.get('line')}"
        inc_id = "inc-" + hashlib.sha256(key.encode()).hexdigest()[:8]
        path = handoff.incidents_dir(t) / f"{inc_id}.json"
        if path.exists():
            continue
        task.write_json(path, {
            "incident_id": inc_id, "detected": task.now(), "artifact": f"sweep/audit.jsonl line {f.get('line')}",
            "checks": [f["rule"]], "action": "escalated-to-user" if f["rule"] == "AUD-TAMPER" else "quarantined",
            "notes": f["evidence"]["untrusted"],
        })


def incidents(t: task.Task) -> list[dict]:
    d = handoff.incidents_dir(t)
    return [task.read_json(p) for p in sorted(d.glob("inc-*.json"))] if d.is_dir() else []


def guard_registered() -> str | None:
    """The settings file that registers the guard hook, or None."""
    needle = f"{guard.INTAKE} guard"
    here = Path.cwd()
    for p in (Path.home() / ".claude" / "settings.json", here / ".claude" / "settings.json",
              here / ".claude" / "settings.local.json"):
        try:
            if needle in p.read_text(encoding="utf-8"):
                return str(p)
        except OSError:
            continue
    return None


def mode_lines(t: task.Task) -> list[str]:
    starts = [w for w in roles.workers(t)]
    try:
        mode, forced = task.mode(), bool(task.os.environ.get(task.MODE_ENV))
    except task.TaskError:
        mode, forced = "unknown", False
    ran = sorted({w["mode"] for w in starts})
    if ran:
        mode = ran[0] if len(ran) == 1 else "/".join(ran)
    how = (f"forced by {task.MODE_ENV}" if forced else
           "HORCH_ROLE, HORCH_RECORD_ID and HERDR_PANE_ID are set and horch --version exits 0"
           if mode == "fleet" else "a fleet variable is not set or horch --version failed (spec 6.2)")
    lines = [f"- mode: {_cell(mode)}", f"- how detected: {_cell(how)} (checked when the report was written)"]
    if mode == "fleet":
        lines.append("- guard hook: not active (fleet runs use no guard; each role worker has its own OS sandbox)")
        lines.append("- role OS sandbox: each role worker runs in its own teammate sandbox (spec 6.4)")
    else:
        reg = guard_registered()
        lines.append("- guard hook: " + (f"registered in {_cell(reg)}" if reg else
                                         "not registered in any settings file intake can read"))
        lines.append("- role OS sandbox: none for the role subagents unless the operator's own session has one")
    box = sandbox.kind()
    lines.append(f"- script-test sandbox: {box or 'none on this host (scripts are not run and stay unverified)'}")
    return lines


def opaque_rows(t: task.Task, findings: list[dict], entries: dict[str, dict]) -> tuple[list[str], list[str]]:
    """(table lines, evidence lines) for every OPQ finding."""
    rows, ev = [], []
    for f in findings:
        if not f["rule"].startswith("OPQ-"):
            continue
        scan_id = f.get("scan_id") or ""
        md = t.c / "sweep" / "opaque" / f"{scan_id}.md"
        text = md.read_text(encoding="utf-8", errors="replace") if scan_id and md.is_file() and not md.is_symlink() else ""

        def field(name: str) -> str | None:
            m = re.search(rf"^[\s>*_#-]*{name}[\s*_]*:[\s*_`]*(.+)$", text, re.IGNORECASE | re.MULTILINE)
            return m.group(1).strip() if m else None

        conf = (field("confidence") or "").strip("*_` ").lower()
        conf = conf if conf in ("high", "medium", "low") else "-"
        rec = _json(t.c / "sweep" / "opaque" / f"{scan_id}.json", {}) if scan_id else {}
        ev.append(f"{f['id']} inferred purpose: {field('inferred purpose') or 'none written'}"
                  + (f" (artifact {rec['path']})" if rec.get("path") else ""))
        rows.append(f"| {_cell(f['id'])} | {_cell(f.get('skill'))} | {conf} | "
                    f"{_cell(entries.get(f['id'], {}).get('disposition'))} |")
    return rows, ev


def capability_lines(t: task.Task, vdoc: dict) -> list[str]:
    caps = {s["spec_id"]: s.get("capabilities") or {} for s in verify.load_specs(t)}
    out = []
    for a in vdoc.get("accepted", []):
        declared = "; ".join(f"{k}: {', '.join(v)}" for k, v in sorted(caps.get(a["spec_id"], {}).items()) if v)
        out.append(f"- {_cell(a['rule'])} in skill {_cell(a.get('skill'))}, accepted by {_cell(a['spec_id'])}: "
                   f"{_cell(declared)}")
    return out


def repo_lines(t: task.Task, state: dict, ev: list[str]) -> list[str]:
    repo = Path(state["repo"])
    branch = repo_install.current_branch(repo)
    base = repo_install.default_branch(repo)
    log = repo_install._git(repo, "log", "--format=%h %s", f"{base}..HEAD") if branch else None
    commits = [ln for ln in (log or "").splitlines() if ln.strip()]
    ev += [f"commit {ln}" for ln in commits]
    m = state.get("mapping")
    lines = ["## Multi-herdr", "", f"- repo: {_cell(state['repo'])}",
             f"- branch: {_cell(branch or 'none (detached or unreadable)')}",
             f"- commits ahead of {_cell(base)}: {len(commits)} (subjects are in the evidence block)",
             f"- gate result: {_cell(t.manifest.get('repo_gate') or 'not recorded (pass --gate-result)')}"]
    if m:
        lines.append(f"- mapping answer: option {m['option']}, teammate {_cell(m.get('teammate'))}, existing "
                     f"{_cell(', '.join(m.get('existing') or []))}, field {_cell(m.get('field'))}")
        if m.get("brief_description"):
            ev.append(f"mapping brief_description: {m['brief_description']}")
    else:
        lines.append("- mapping answer: not given yet (only the answer-independent set was written)")
    for label, key in (("files written", "written"), ("files created by intake", "created"),
                       ("files removed or reverted by a later mapping", "removed")):
        items = state.get(key) or []
        lines.append(f"- {label}:" + ("" if items else " none"))
        lines += [f"  - {_cell(x)}" for x in items]
    lines += ["- next step: run `/push-pr` to publish", ""]
    return lines


# ---- outcome -------------------------------------------------------------------------

def flags_of(summary: dict, vdoc: dict, incs: list[dict], state: dict | None, mapping_missing: bool) -> list[str]:
    flags = []
    lic = summary["licence"].get("flag")
    if lic:
        flags.append(f"licence flag {lic}")
    flags += [f"verify flag: {f}" for f in vdoc.get("flags", [])]
    listed = vdoc.get("listed") or {}
    if listed.get("unresolved") or listed.get("new_hits"):
        flags.append("medium or lower findings left unresolved")
    flags += [f"script left unverified: {s['path']}" for s in vdoc.get("scripts", []) if s["status"] == "unverified"]
    if incs:
        flags.append(f"{len(incs)} incident(s)")
    if mapping_missing:
        flags.append("mapping answer not given")
    return flags


def outcome_of(t: task.Task, summary: dict, vdoc: dict, flags: list[str], lost: list[dict]) -> str:
    if lost:
        return "aborted for a lost role"
    if summary["gate1"] == "abort":
        return "aborted at GATE 1"
    if summary["gate1"] == "pending-audit":
        return "pending audit (GATE 1 cannot pass until the auditor writes sweep/audit.jsonl)"
    if t.manifest.get("gate2") == "abort":
        return "aborted at GATE 2"
    if str(t.manifest.get("phase", "")).startswith("installed"):
        return "installed with flags" if flags else "installed"
    return "stopped before install (GATE 1 passed; GATE 2 has not run)" if not t.manifest.get("gate2") else \
        "stopped before install (GATE 2 passed)"


# ---- the report ---------------------------------------------------------------------------

def write(t: task.Task) -> tuple[Path, str]:
    """Write C/report.md. Returns its path and a one-paragraph summary (no source text)."""
    c = t.c
    prov = _json(c / "provenance.json", {})
    summary_path = c / "sweep" / "summary.json"
    if not summary_path.is_file():
        raise task.TaskError(f"no sweep/summary.json in {c}: run intake triage first")
    summary = task.read_json(summary_path)
    findings = task.read_json(c / "sweep" / "findings.json")["findings"]
    entries = task.read_json(c / "mitigation.json")["entries"]
    by_entry = {e["finding"]: e for e in entries}
    vdoc = _json(c / "verify.json", {})
    state = _json(c / repo_install.STATE, None)
    write_incidents(t, findings)
    incs = incidents(t)
    lost = [w for w in roles.workers(t) if w["state"] == "lost"]
    aborted = roles.aborts(t)
    installed = prov.get("installed", [])
    mapping_missing = bool(state) and not state.get("mapping")
    flags = flags_of(summary, vdoc, incs, state, mapping_missing) if installed else []
    outcome = outcome_of(t, summary, vdoc, flags, aborted)
    ev: list[str] = []

    lines = ["# skill-intake report", "", f"- outcome: {outcome}", f"- task: {_cell(t.id)}",
             f"- auditor: {summary['audit']}"]
    lines += [f"- abort reason: {_cell(r)}" for r in summary["reasons"]]
    lines += [f"- abort reason: {_cell(r)}" for r in vdoc.get("reasons", []) if t.manifest.get("gate2") == "abort"]
    lines += [f"- role abort: {_cell(a['reason'])}" for a in aborted]
    lines += [f"- flag: {_cell(f)}" for f in flags]
    lines.append("")
    lines += source_section(prov)

    lines += ["## Mode and install targets", ""] + mode_lines(t)
    rule = (state or {}).get("target_rule") or t.manifest.get("target_rule")
    lines.append(f"- install target rule: {_cell(rule or 'no install ran')}")
    for r in installed:
        lines.append(f"- installed: {_cell(r['name'])} ({_cell(r['target'])})"
                     + (f", renamed from {_cell(r['renamed_from'])}" if r.get("renamed_from") else ""))
    if not installed:
        lines.append("- installed: nothing")
    lines.append("")
    if state:
        lines += repo_lines(t, state, ev)

    lines += findings_section(findings, entries, {r["finding"]: r["status"] for r in vdoc.get("findings", [])})
    ev += finding_evidence(findings)

    rows, opq_ev = opaque_rows(t, findings, by_entry)
    lines += ["## Opaque artifacts", ""]
    if rows:
        lines += ["| id | skill | confidence | disposition |", "|---|---|---|---|", *rows]
        ev += opq_ev
    else:
        lines.append("none")
    lines.append("")

    removed = [e for e in entries if e["disposition"] in ("remove", "removed")]
    lines += ["## Removed features", ""]
    lines += [f"- {_cell(e['finding'])} ({_cell(e.get('skill'))}): removed" for e in removed] or ["none"]
    lines += ["", "## Accepted capabilities", ""]
    lines += (capability_lines(t, vdoc) if vdoc else []) or ["none"]
    lines += ["", "## Leakage and confinement", ""]
    if vdoc:
        lines += ["| check | result |", "|---|---|", *[f"| {_cell(ch['check'])} | {_cell(ch['result'])} |" for ch in vdoc["checks"]]]
        ev += [f"check {ch['check']}: {ch['detail']}" for ch in vdoc["checks"] if ch.get("detail")]
    else:
        lines.append("GATE 2 did not run: no verify checks to report")
    lines += ["", "### Incidents", ""]
    if incs:
        lines += ["| id | checks | action |", "|---|---|---|",
                  *[f"| {_cell(i['incident_id'])} | {_cell(', '.join(i['checks']))} | {_cell(i['action'])} |" for i in incs]]
        ev += [f"{i['incident_id']} {i['artifact']}: {i.get('notes', '')}" for i in incs]
    else:
        lines.append("none")
    unverified = [s["path"] for s in vdoc.get("scripts", []) if s["status"] == "unverified"]
    lines += ["", "### Scripts left unverified", ""]
    lines += [f"- {_cell(p)}" for p in unverified] or ["none"]
    lines += ["", "### Lost roles", ""]
    lines += [f"- role {_cell(w['role'])}, worker {_cell(w['worker'])}, attempt {w['attempt']}" for w in lost] or ["none"]
    lines.append("")
    if ev:
        lines.append(evidence.markdown(ev))
    path = c / "report.md"
    path.write_text("\n".join(lines).rstrip("\n") + "\n", encoding="utf-8")

    counts = ", ".join(f"{summary['counts'][s]} {s}" for s in SEVERITIES)
    para = (f"skill-intake {outcome}. Findings: {summary['findings']} ({counts}), {summary['malicious']} malicious. "
            f"Licence class {_cell(summary['licence']['class'])}, flag {_cell(summary['licence']['flag'])}. "
            f"Install targets: " + (", ".join(f"{_cell(r['name'])} ({_cell(r['target'])})" for r in installed) or "none") + ". ")
    if state and state.get("mapping") is None:
        para += "The mapping answer is still open. "
    para += f"Report: {path}"
    reasons = summary["reasons"] + [a["reason"] for a in aborted]
    if reasons:
        para += ". Abort reasons: " + "; ".join(_cell(r) for r in reasons)
    return path, para
