"""`intake candidates --task T --repo R`: the mapping suggestion (spec 8.2, 10).

stdout: {"recommend": 1|2|3, "skills": [{"name", "purpose"}], "candidates":
[{"teammate", "skills", "reasons"}], "field": "available_skills"}. Exit 0.
"""

from __future__ import annotations

import json
from pathlib import Path

from . import candidates, task

HELP = "suggest the mapping answer: new teammate, both, or existing teammates"


def add_arguments(p) -> None:
    p.add_argument("--task", required=True, help="the task dir that acquire printed")
    p.add_argument("--repo", required=True, help="the multi-herdr checkout (or worktree)")


def run(args) -> int:
    t = task.load(args.task)
    doc = candidates.run(t, task.resolve(Path(args.repo)))
    print(json.dumps(doc, ensure_ascii=False))
    return 0
