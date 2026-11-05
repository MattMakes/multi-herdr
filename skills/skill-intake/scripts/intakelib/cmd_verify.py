"""`intake verify --task T --target repo|standalone`: stage 12 and GATE 2 (spec 7.1).

Checks K/out/ before anything is installed and writes C/verify.json. Stores
the GATE 2 result as `gate2` in C/task.json. Exit codes: 0 pass, 5 abort, 2 bad
input. stdout gets the gate, the abort reasons and the counts; it holds no
source text.
"""

from __future__ import annotations

import json
import sys

from . import evidence, targets, task, verify

HELP = "check the rewritten skills (re-sweep, leakage, review, scripts, target rules); keep GATE 2"
EXIT = {verify.PASS: 0, verify.ABORT: 5}


def add_arguments(p) -> None:
    p.add_argument("--task", required=True, help="the task dir that acquire printed")
    p.add_argument("--target", required=True, choices=targets.TARGETS)
    p.add_argument("--budget-exempt", action="append", default=[], metavar="SKILL=REASON",
                   help="repo target: the budget_exempt reason of an output skill over 160 KB")


def _exempt(values: list[str]) -> dict[str, str]:
    out = {}
    for v in values:
        name, sep, reason = v.partition("=")
        if not sep or not name or not reason.strip():
            raise task.TaskError(f"--budget-exempt needs SKILL=REASON: {evidence.escape(v)}")
        out[name] = reason.strip()
    return out


def run(args) -> int:
    t = task.load(args.task)
    doc = verify.run(t, args.target, _exempt(args.budget_exempt))
    status = {}
    for row in doc["findings"]:
        status[row["status"]] = status.get(row["status"], 0) + 1
    print(json.dumps({
        "gate2": doc["gate"], "reasons": doc["reasons"], "verify": str(t.c / "verify.json"),
        "checks": {ch["check"]: ch["result"] for ch in doc["checks"]}, "findings": status,
        "listed": doc["listed"], "accepted": len(doc["accepted"]),
    }))
    print(f"intake verify: GATE 2 {doc['gate']}", file=sys.stderr)
    for r in doc["reasons"]:
        print(f"intake verify: abort reason: {r}", file=sys.stderr)
    return EXIT[doc["gate"]]
