"""`intake candidates`: the recommended answer to the mapping question (spec 8.2).

A candidate is a visible teammate whose `skills`, `available_skills` or
`skills_when` share a name prefix with an intake skill (`godot-`), or whose
`brief_description` shares 2 or more non-stop words with a skill's purpose.

Recommendation: 1 (new teammate) with no candidates; 2 (new teammate and
existing ones) with candidates when the skills also form a specialty of their
own (3 or more skills, or a skill no candidate covers); 3 (existing teammates
only) otherwise. It is a suggestion, not a decision.
"""

from __future__ import annotations

import re
from pathlib import Path

from . import evidence, roster, task

STOP = frozenset("""
a about after all also an and any are as at be been before but by can do does each for from has have
how in into is it its make makes may more most new no not of on one or other out over own same so
such than that the their them then there these they this those through to under up use used uses
using via was what when where which while who will with without you your skill skills file files
work works worker tool tools""".split())


def words(text: str) -> set[str]:
    return {w for w in re.findall(r"[a-z0-9]+", text.lower()) if len(w) >= 3 and w not in STOP}


def prefix(name: str) -> str | None:
    head = name.split("-", 1)[0]
    return head + "-" if "-" in name and head else None


def find(skills: list[dict], teammates_dir: Path) -> list[dict]:
    """[{"teammate", "skills", "reasons"}] for every candidate, sorted by name."""
    out = []
    for name, fm in roster.visible(teammates_dir):
        named = roster.skill_names(fm)
        brief = words(roster.unquote(fm.get("brief_description", "")))
        covered, reasons = [], []
        for s in skills:
            pre = prefix(s["name"])
            shared_names = sorted(n for n in named if pre and n.startswith(pre))
            shared_words = sorted(brief & words(s["purpose"]))
            hit = []
            if shared_names:
                hit.append(f"{s['name']}: names {pre}* skills ({', '.join(shared_names[:3])})")
            if len(shared_words) >= 2:
                hit.append(f"{s['name']}: brief shares {', '.join(shared_words[:5])}")
            if hit:
                covered.append(s["name"])
                reasons.extend(hit)
        if covered:
            out.append({"teammate": name, "skills": covered, "reasons": reasons})
    return out


def recommend(skills: list[dict], cands: list[dict]) -> int:
    if not cands:
        return 1
    covered = {s for c in cands for s in c["skills"]}
    if len(skills) >= 3 or any(s["name"] not in covered for s in skills):
        return 2
    return 3


def intake_skills(t: task.Task) -> list[dict]:
    """[{"name", "purpose"}]: the discovered skills with the auditor's purpose (escaped)."""
    names = [s["name"] for s in task.read_json(t.c / "skills.json")["skills"]]
    summary = t.c / "sweep" / "summary.json"
    purposes = task.read_json(summary).get("purposes", {}) if summary.is_file() else {}
    return [{"name": n, "purpose": evidence.escape(purposes.get(n, ""))} for n in sorted(names)]


def run(t: task.Task, repo: Path) -> dict:
    teammates = repo / "teammates"
    if not teammates.is_dir():
        raise task.TaskError(f"no teammates/ folder in {repo}")
    if not (t.c / "skills.json").is_file():
        raise task.TaskError("no C/skills.json: run intake discover first")
    skills = intake_skills(t)
    cands = find(skills, teammates)
    return {"recommend": recommend(skills, cands), "skills": skills, "candidates": cands,
            "field": "available_skills"}
