"""`intake brief <role> --task T [--unit U] [--skill S] [--fix]`: write a role brief (spec 6.5).

The brief comes from the role's fixed template and goes to the role's own side
root. Prints {"brief", "status_file"}. Exit codes: 0 ok, 2 bad input, 9 a
clean-side brief failed the leakage checks.
"""

from __future__ import annotations

import json
import sys

from . import roles, task

HELP = "write a role brief from its fixed template (spec 6.5)"


def add_arguments(p) -> None:
    p.add_argument("role", choices=roles.ROLES)
    p.add_argument("--task", required=True, help="the task dir that acquire printed")
    p.add_argument("--unit", help="a unit id from cr index")
    p.add_argument("--skill", help="the skill name the role works on")
    p.add_argument("--fix", action="store_true",
                   help="writer only: a fix brief that names K/fixes.json from intake verify")


def run(args) -> int:
    try:
        out = roles.brief(task.load(args.task), args.role, args.unit, args.skill, args.fix)
    except roles.RoleError as e:
        print(f"intake: {e}", file=sys.stderr)
        return e.code
    print(json.dumps(out, indent=2))
    return 0
