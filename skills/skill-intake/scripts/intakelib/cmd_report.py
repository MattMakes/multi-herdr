"""`intake report --task T`: write C/report.md and print a one-paragraph summary (spec 11).

Works after any outcome. `--gate-result` records the multi-herdr gate result for the report.
"""

from __future__ import annotations

import json

from . import evidence, report, task

HELP = "write report.md and print a one-paragraph summary"


def add_arguments(p) -> None:
    p.add_argument("--task", required=True, help="the task dir that acquire printed")
    p.add_argument("--gate-result", metavar="TEXT", help="multi-herdr: the result of the repo gate run")


def run(args) -> int:
    t = task.load(args.task)
    if args.gate_result is not None:
        t.manifest["repo_gate"] = evidence.escape(args.gate_result)
        task.write_json(t.c / task.MANIFEST, t.manifest)
    path, para = report.write(t)
    print(json.dumps({"report": str(path), "summary": para}))
    return 0
