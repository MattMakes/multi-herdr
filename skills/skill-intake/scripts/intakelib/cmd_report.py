"""`intake report --task T`: write C/report.md and print a one-paragraph summary (spec 11).

This ticket writes the sections known at triage time; ticket 13 adds the rest.
"""

from __future__ import annotations

import json

from . import report, task

HELP = "write report.md and print a one-paragraph summary"


def add_arguments(p) -> None:
    p.add_argument("--task", required=True, help="the task dir that acquire printed")


def run(args) -> int:
    path, para = report.write(task.load(args.task))
    print(json.dumps({"report": str(path), "summary": para}))
    return 0
