"""The report (spec 11): the sections known at triage time.

Ticket 13 adds the later sections. Every piece of source text goes through the
evidence-escape function, and evidence appears only in the "Untrusted
evidence" block, never in a table cell.
"""

from __future__ import annotations

from pathlib import Path

from . import evidence, task
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


def findings_section(findings: list[dict], entries: list[dict]) -> list[str]:
    by_id = {e["finding"]: e for e in entries}
    lines = ["## Findings", "", "| severity | count |", "|---|---|"]
    for s in SEVERITIES:
        lines.append(f"| {s} | {sum(1 for f in findings if f['severity'] == s)} |")
    lines += ["", "| id | severity | skill | disposition | status | verify |", "|---|---|---|---|---|---|"]
    for f in findings:
        e = by_id.get(f["id"], {})
        lines.append(
            f"| {_cell(f['id'])}{' (malicious)' if f['malicious'] else ''} | {f['severity']} | "
            f"{_cell(f.get('skill'))} | {_cell(e.get('disposition'))} | {_cell(e.get('status'))} | pending |"
        )
    lines.append("")
    if findings:
        texts = []
        for f in findings:
            where = f["file"] + (f":{f['line']}" if isinstance(f.get("line"), int) else "")
            texts.append(f"{f['id']} {where}: {f['evidence']['untrusted']}")
        lines.append(evidence.markdown(texts))
    return lines


def sweep_report(summary: dict, findings: list[dict], entries: list[dict]) -> str:
    """sweep/report.md: the gate result and the findings."""
    lines = ["# Sweep report", "", f"- GATE 1: {OUTCOME[summary['gate1']]}",
             f"- auditor: {summary['audit']}"]
    lines += [f"- abort reason: {_cell(r)}" for r in summary["reasons"]]
    lines.append("")
    lines += findings_section(findings, entries)
    return "\n".join(lines).rstrip("\n") + "\n"


def write(t: task.Task) -> tuple[Path, str]:
    """Write C/report.md. Returns its path and a one-paragraph summary."""
    c = t.c
    prov = task.read_json(c / "provenance.json") if (c / "provenance.json").is_file() else {}
    summary_path = c / "sweep" / "summary.json"
    if not summary_path.is_file():
        raise task.TaskError(f"no sweep/summary.json in {c}: run intake triage first")
    summary = task.read_json(summary_path)
    findings = task.read_json(c / "sweep" / "findings.json")["findings"]
    entries = task.read_json(c / "mitigation.json")["entries"]
    outcome = OUTCOME[summary["gate1"]]
    lines = ["# skill-intake report", "", f"- outcome: {outcome}", f"- task: {_cell(t.id)}",
             f"- auditor: {summary['audit']}"]
    lines += [f"- abort reason: {_cell(r)}" for r in summary["reasons"]]
    lines.append("")
    lines += source_section(prov)
    lines += findings_section(findings, entries)
    path = c / "report.md"
    path.write_text("\n".join(lines).rstrip("\n") + "\n", encoding="utf-8")
    counts = ", ".join(f"{summary['counts'][s]} {s}" for s in SEVERITIES)
    src = _cell((prov.get("source") or {}).get("input"))
    para = (f"skill-intake {outcome} for {src}. Findings: {summary['findings']} ({counts}), "
            f"{summary['malicious']} malicious. Licence class {_cell(summary['licence']['class'])}"
            f", flag {_cell(summary['licence']['flag'])}. Report: {path}")
    if summary["reasons"]:
        para += ". Abort reasons: " + "; ".join(_cell(r) for r in summary["reasons"])
    return path, para
