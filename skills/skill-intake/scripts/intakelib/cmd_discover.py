"""`intake discover --task T`: stage 2 (spec 3.7).

Writes C/skills.json and the INJ-09 and ACQ-05 findings to
C/sweep/discover.jsonl, and
records the skill names in provenance.json. Exit 0, or 6 when no skill is found.
"""

from __future__ import annotations

import json
import sys
from pathlib import PurePosixPath

from . import discover, evidence, task

HELP = "list the skills in the quarantined tree (skills.json)"
FINDINGS = "sweep/discover.jsonl"


def add_arguments(p) -> None:
    p.add_argument("--task", required=True, help="the task dir that acquire printed")


def _root_name(source: dict) -> str:
    """The folder name for a skill at the source root."""
    if source.get("subdir"):
        return PurePosixPath(source["subdir"]).name
    if source.get("kind") == "github" and source.get("url"):
        return PurePosixPath(source["url"]).name.removesuffix(".git")
    name = PurePosixPath(source.get("input", "").rstrip("/")).name
    return name.rsplit(".", 1)[0] if source.get("kind") == "archive" else name


def run(args) -> int:
    t = task.load(args.task)
    src = t.c / "quarantine" / "src"
    prov_path = t.c / "provenance.json"
    if not src.is_dir() or not prov_path.is_file() or t.manifest.get("phase") == "refused":
        raise task.TaskError(f"no acquired source in task {t.id}: run intake acquire first")
    prov = task.read_json(prov_path)
    result = discover.discover(src, _root_name(prov.get("source") or {}))
    with open(t.c / FINDINGS, "w", encoding="utf-8") as f:
        for n, row in enumerate(result.pop("findings"), 1):
            detail = row.pop("detail")
            f.write(json.dumps({
                "id": f"{row['rule']}#d{n}", **row, "line": None,
                "evidence": evidence.untrusted(detail), "malicious": False,
            }, ensure_ascii=True) + "\n")
    task.write_json(t.c / "skills.json", result)
    prov["skills"] = [s["name"] for s in result["skills"]]
    task.write_json(prov_path, prov)
    if not result["skills"]:
        task.set_phase(t, "no-skills")
        print("intake discover: no skill found (no folder holds a SKILL.md)", file=sys.stderr)
        print(json.dumps({"skills": []}))
        return 6
    task.set_phase(t, "discovered")
    print(json.dumps({"skills": prov["skills"]}))
    return 0
