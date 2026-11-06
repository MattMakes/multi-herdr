"""`intake roles --task T [--pending] [--lost W]`: the role workers of a task.

Prints 1 JSON line per role worker with its state: running, done, error or
lost. `--pending` prints only the running ones (nothing when none runs).
`--lost W` (fleet mode) marks worker W lost and spawns a fresh worker for the
same brief once. Exit codes: 0 ok, 2 bad input, 8 the brief was lost twice,
10 `--lost` outside fleet mode, 11 horch spawn failed.
"""

from __future__ import annotations

import json
import sys

from . import roles, task

HELP = "list the role workers and their state; --lost marks one lost and spawns it again once"
FIELDS = ("id", "role", "worker", "state", "brief", "status_file", "started", "attempt", "mode")


def add_arguments(p) -> None:
    p.add_argument("--task", required=True, help="the task dir that acquire printed")
    g = p.add_mutually_exclusive_group()
    g.add_argument("--pending", action="store_true", help="only the running role workers")
    g.add_argument("--lost", metavar="WORKER", help="the worker role name (or start id) that stopped without DONE")


def run(args) -> int:
    t = task.load(args.task)
    if args.lost:
        try:
            out = roles.mark_lost(t, args.lost)
        except roles.RoleError as e:
            print(f"intake: {e}", file=sys.stderr)
            return e.code
        print(f"spawned {out['worker'] or 'a worker'} again for {out['brief']}", file=sys.stderr)
    for w in roles.workers(t):
        if not args.pending or w["state"] == "running":
            print(json.dumps({k: w[k] for k in FIELDS}))
    return 0
