"""`intake run <source> [acquire options]`: acquire, discover, scan and triage in 1 call
(the deterministic front half, spec 10).

Each stage's JSON goes to stderr; stdout gets 1 JSON line:
{"task_dir", "stage", "exit"}. The exit code is the code of the first stage
that stops (acquire 2/3, discover 6, triage 4 or 14). With no audit.jsonl yet,
triage gives 14 (pending-audit): the controller runs the auditor, then
`intake triage` again.
"""

from __future__ import annotations

import argparse
import contextlib
import io
import json
import sys

from . import cmd_acquire, cmd_discover, cmd_scan, cmd_triage

HELP = "acquire, discover, scan and triage in one call"
STAGES = (("discover", cmd_discover), ("scan", cmd_scan), ("triage", cmd_triage))


def add_arguments(p) -> None:
    cmd_acquire.add_arguments(p)


def _stage(fn, args) -> tuple[int, str]:
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        code = fn(args)
    out = buf.getvalue()
    sys.stderr.write(out)
    return code, out


def run(args) -> int:
    code, out = _stage(cmd_acquire.run, args)
    task_dir = None
    for ln in out.splitlines():
        try:
            task_dir = json.loads(ln).get("task_dir", task_dir)
        except ValueError:
            pass
    stage = "acquire"
    if code == 0:
        for stage, mod in STAGES:
            code, _ = _stage(mod.run, argparse.Namespace(task=task_dir))
            if code != 0:
                break
    print(json.dumps({"task_dir": task_dir, "stage": stage, "exit": code}))
    return code
