"""`intake subagent-prompt <role> --brief P`: the plain-mode subagent prompt (spec 6.8).

Prints the text of roles/<role>.md and 1 brief line, nothing else. Before it
prints, it takes the domain snapshot, sets the guard role (`cr role set`) and
records the start; `intake role-result` compares and clears. Exit codes: 0 ok,
2 bad input, 8 the role stage aborted, 10 in fleet mode, 14 another role is active.
"""

from __future__ import annotations

import sys

from . import roles

HELP = "plain mode: print the subagent prompt of a role (role file + 1 brief line)"


def add_arguments(p) -> None:
    p.add_argument("role", choices=roles.ROLES)
    p.add_argument("--brief", required=True, help="the brief path that intake brief printed")


def run(args) -> int:
    try:
        text = roles.subagent_prompt(args.role, args.brief)
    except roles.RoleError as e:
        print(f"intake: {e}", file=sys.stderr)
        return e.code
    sys.stdout.write(text)
    return 0
