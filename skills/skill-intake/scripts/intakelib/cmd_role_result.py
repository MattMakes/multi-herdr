"""`intake role-result <status file>`: check a role's result (spec 6.6).

Accepts only the status schema {"role", "status", "outputs", "error"}: no
extra field, an error of at most 1 line, every output inside the role's side
root. Compares the domain snapshot against the role's write set. Prints the
result; the error line is escaped. Exit codes: 0 ok, 12 bad status file or a
domain incident.
"""

from __future__ import annotations

import json
import sys

from . import roles

HELP = "check a role's status file and the domain snapshot (spec 6.6)"


def add_arguments(p) -> None:
    p.add_argument("status_file")


def run(args) -> int:
    try:
        out = roles.role_result(args.status_file)
    except roles.RoleError as e:
        print(f"intake: {e}", file=sys.stderr)
        return e.code
    print(json.dumps(out, indent=2, ensure_ascii=True))
    return 0
