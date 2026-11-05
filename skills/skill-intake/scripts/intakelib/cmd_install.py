"""`intake install --task T [--target repo|standalone] [--repo R] [--mapping FILE]
[--resolve JSON] [--skills-dir DIR]`: stage 13 (spec 8, 10).

Without --target, the spec 8.1 rule picks the target in the invocation dir (or
in --repo when given); the output names the rule that decided. The repo target
without --mapping writes only the answer-independent set (spec 8.2).

Exit codes: 0 ok, 2 bad input, 7 a name collision that no rule resolves.
stdout gets the result as JSON.
"""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path

from . import install, repo_install, targets, task

HELP = "install the verified skills (standalone: ~/.claude/skills; repo: bundled skills and bookkeeping)"


def add_arguments(p) -> None:
    p.add_argument("--task", required=True, help="the task dir that acquire printed")
    p.add_argument("--target", choices=targets.TARGETS, help="override the mode rule of spec 8.1")
    p.add_argument("--repo", metavar="R", help="the multi-herdr checkout (default: the git top level here)")
    p.add_argument("--mapping", metavar="FILE", help="repo target: the mapping answer (spec 8.2) as JSON")
    p.add_argument("--resolve", metavar="JSON",
                   help='name collisions: {"<skill>": "new-name" | "SKIP" | "REPLACE"}')
    p.add_argument("--skills-dir", metavar="DIR", help="the skills dir (default ~/.claude/skills; tests set it)")


def run(args) -> int:
    mode = targets.detect(Path(args.repo) if args.repo else Path(os.getcwd()), args.target)
    t = task.load(args.task)
    try:
        if mode["target"] == "repo":
            if args.skills_dir:
                raise task.TaskError("--skills-dir is for the standalone target")
            repo = args.repo or mode["repo"]
            if not repo:
                raise task.TaskError("--target repo needs --repo or a git work tree here")
            doc = repo_install.run(t, Path(repo), args.mapping, args.resolve, mode["rule"])
            print(json.dumps(doc, ensure_ascii=False))
            for r in doc["installed"]:
                if r["renamed_from"]:
                    print(f"intake install: renamed {r['renamed_from']} to {r['name']}", file=sys.stderr)
            if doc["mapping"] is None:
                print("intake install: wrote the answer-independent set; run install again with --mapping",
                      file=sys.stderr)
            return 0
        if args.mapping:
            raise task.TaskError("--mapping is for the repo target")
        doc = install.run(t, args.skills_dir or install.default_skills_dir(), args.resolve)
    except install.Unresolvable as e:
        print(f"intake install: {e}", file=sys.stderr)
        return 7
    print(json.dumps({
        "target": "standalone", "target_rule": mode["rule"],
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
