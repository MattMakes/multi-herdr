"""`intake triage --task T`: stage 4 and GATE 1 (spec 5).

Writes C/sweep/findings.json, C/mitigation.json, C/sweep/summary.json and
C/sweep/report.md, and stores the GATE 1 result as `gate1` in C/task.json.
Exit codes: 0 pass, 4 abort, 14 pending-audit (no sweep/audit.jsonl yet:
GATE 1 cannot pass; run triage again after the auditor).
"""

from __future__ import annotations

import json
import sys

from . import report, task, triage

HELP = "merge scanner and auditor findings, plan mitigations, keep GATE 1"
EXIT = {triage.PASS: 0, triage.ABORT: 4, triage.PENDING: 14}


def add_arguments(p) -> None:
    p.add_argument("--task", required=True, help="the task dir that acquire printed")


def run(args) -> int:
    t = task.load(args.task)
    s = triage.run(t)
    findings = task.read_json(t.c / "sweep" / "findings.json")["findings"]
    entries = task.read_json(t.c / "mitigation.json")["entries"]
    (t.c / "sweep" / "report.md").write_text(report.sweep_report(s, findings, entries), encoding="utf-8")
    print(json.dumps({"gate1": s["gate1"], "reasons": s["reasons"], "audit": s["audit"],
                      "findings": s["findings"], "counts": s["counts"], "malicious": s["malicious"]}))
    print(f"intake triage: GATE 1 {s['gate1']}, {s['findings']} finding(s)", file=sys.stderr)
    for r in s["reasons"]:
        print(f"intake triage: abort reason: {r}", file=sys.stderr)
    return EXIT[s["gate1"]]
