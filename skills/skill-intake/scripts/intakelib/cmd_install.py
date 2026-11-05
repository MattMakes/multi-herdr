"""`intake install --task T --target standalone [--resolve JSON] [--skills-dir DIR]`:
stage 13 (spec 8.5, 10). Ticket 11 adds the repo target.

Exit codes: 0 ok, 2 bad input, 7 a name collision that no rule resolves.
stdout gets the installed names, renames and skips as JSON.
"""

from __future__ import annotations

import json
import sys

from . import install, targets, task

HELP = "install the verified skills (standalone: into ~/.claude/skills)"


def add_arguments(p) -> None:
    p.add_argument("--task", required=True, help="the task dir that acquire printed")
    p.add_argument("--target", required=True, choices=targets.TARGETS)
    p.add_argument("--resolve", metavar="JSON",
                   help='name collisions: {"<skill>": "new-name" | "SKIP" | "REPLACE"}')
    p.add_argument("--skills-dir", metavar="DIR", help="the skills dir (default ~/.claude/skills; tests set it)")


def run(args) -> int:
    if args.target != "standalone":
        raise task.TaskError("install --target repo is not built yet (ticket 11)")
    t = task.load(args.task)
    try:
        doc = install.run(t, args.skills_dir or install.default_skills_dir(), args.resolve)
    except install.Unresolvable as e:
        print(f"intake install: {e}", file=sys.stderr)
        return 7
    print(json.dumps({
        "installed": [{"name": r["name"], "renamed_from": r["renamed_from"], "path": r["path"]}
                      for r in doc["installed"]],
        "skipped": doc["skipped"],
    }))
    for r in doc["installed"]:
        if r["renamed_from"]:
            print(f"intake install: renamed {r['renamed_from']} to {r['name']}", file=sys.stderr)
        print(f"intake install: installed {r['path']}", file=sys.stderr)
    for s in doc["skipped"]:
        print(f"intake install: skipped {s}", file=sys.stderr)
    return 0
