"""`intake spawn <role> --task T [--unit U] [--skill S] [--fix]`: start a role worker (spec 6.5).

Writes the brief, takes the domain snapshot, runs
`horch spawn intake-<role> --exact "Read and follow <brief> exactly."` and
records the start in C/roles.jsonl. Roles run 1 at a time. Exit codes: 0 ok,
2 bad input, 8 the role stage aborted (a brief was lost twice), 9 the brief
failed the leakage checks, 10 not in fleet mode, 11 horch spawn failed,
14 another role of the task is active.
"""

from __future__ import annotations

import json
import sys

from . import roles, task

HELP = "brief + domain snapshot + horch spawn of a role worker (fleet mode)"


def add_arguments(p) -> None:
    p.add_argument("role", choices=roles.ROLES)
    p.add_argument("--task", required=True, help="the task dir that acquire printed")
    p.add_argument("--unit", help="a unit id from cr index")
    p.add_argument("--skill", help="the skill name the role works on")
    p.add_argument("--fix", action="store_true",
                   help="writer only: a fix brief that names K/fixes.json from intake verify")


def run(args) -> int:
    try:
        out = roles.spawn(task.load(args.task), args.role, args.unit, args.skill, args.fix)
    except roles.RoleError as e:
        print(f"intake: {e}", file=sys.stderr)
        return e.code
    print(json.dumps(out, indent=2))
    print(f"spawned {out['worker'] or 'a worker'} for {out['role']}; end the turn and wait for its DONE line",
          file=sys.stderr)
    return 0
