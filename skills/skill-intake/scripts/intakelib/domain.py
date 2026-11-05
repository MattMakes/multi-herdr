"""The domain snapshot (spec 6.10): a sha256 of every file in each side root,
taken before and after each role. A file that a role added, changed or removed
outside its write set (spec 6.3, 6.6) is a domain incident, and an incident
fails GATE 2.

`take` makes a snapshot. `compare` checks 2 snapshots against a role's write
set and records an incident in C/contaminated/incidents/. Ticket 12 calls them
around each role (`intake spawn` before, `intake role-result` after).
"""

from __future__ import annotations

import fnmatch
import hashlib
import os
import secrets

from . import evidence
from .task import SIDES, Task, now, write_json

SHORT = {"contaminated": "C", "drafts": "D", "clean": "K"}
CHECK = "domain-snapshot"

# Write set per role, as patterns on "<C|D|K>/<path in that side root>".
# Each role also writes its status file in the status/ folder of its own side
# (spec 6.6; the analyst brief is in C/briefs/, its status file in D/status/).
WRITE_SETS = {
    "auditor": ("C/sweep/audit.jsonl", "C/sweep/opaque/*.md", "C/status/auditor-*.json"),
    "analyst": ("D/drafts/*", "D/status/analyst-*.json"),
    "sanitizer": ("D/drafts/*", "D/quarantine-cr/*", "D/status/sanitizer-*.json"),
    "writer": ("K/out/*", "K/report.json", "K/persona.md", "K/verify-log/*", "K/status/writer-*.json"),
    "reviewer": ("K/out/*", "K/review.json", "K/verify-log/*", "K/status/reviewer-*.json"),
}
# Files the controller writes through `intake` while a role runs: the task
# manifest, the role log, briefs and the role snapshots. A change to them is
# not counted against a role.
CONTROLLER = ("C/task.json", "C/roles.jsonl", "C/briefs/*", "D/briefs/*", "K/briefs/*",
              "C/contaminated/snapshots/*")


def take(task: Task) -> dict:
    """{"taken", "files": {"<C|D|K>/<rel>": sha256}} over every file of the 3 side roots.
    Symlinks are recorded by their target text, never followed."""
    files = {}
    for side in SIDES:
        root = task.root(side)
        for d, dirs, names in os.walk(root, followlinks=False):
            dirs.sort()
            # os.walk lists a symlink to a folder under dirs; record it as a link too.
            for name in sorted(names + [x for x in dirs if os.path.islink(os.path.join(d, x))]):
                p = os.path.join(d, name)
                rel = SHORT[side] + "/" + os.path.relpath(p, root).replace(os.sep, "/")
                if os.path.islink(p):
                    files[rel] = "link:" + hashlib.sha256(os.readlink(p).encode()).hexdigest()
                elif os.path.isfile(p):
                    files[rel] = _sha256(p)
    return {"taken": now(), "files": files}


def _sha256(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def _allowed(path: str, patterns) -> bool:
    return any(fnmatch.fnmatchcase(path, pat) for pat in patterns)


def changes(before: dict, after: dict) -> list[tuple[str, str]]:
    """[(change, path)] with change in added, changed, removed; sorted by path."""
    a, b = before["files"], after["files"]
    out = [("added", p) for p in b if p not in a]
    out += [("removed", p) for p in a if p not in b]
    out += [("changed", p) for p in a if p in b and a[p] != b[p]]
    return sorted(out, key=lambda c: c[1])


def compare(task: Task, role: str, before: dict, after: dict) -> dict | None:
    """Check the changes between 2 snapshots against `role`'s write set.
    Return None when every change is in the write set (or the controller's set).
    Else write an incident and return it."""
    if role not in WRITE_SETS:
        raise ValueError(f"unknown role {role!r}; known: {', '.join(WRITE_SETS)}")
    allowed = WRITE_SETS[role] + CONTROLLER
    outside = [(c, p) for c, p in changes(before, after) if not _allowed(p, allowed)]
    if not outside:
        return None
    inc_id = "inc-" + secrets.token_hex(4)
    shown = [f"{c} {evidence.escape(p)}" for c, p in outside[:50]]
    more = f"; {len(outside) - 50} more" if len(outside) > 50 else ""
    incident = {
        "incident_id": inc_id, "detected": now(), "artifact": f"role {role}",
        "checks": [CHECK], "action": "escalated-to-user",
        "notes": f"{len(outside)} write(s) outside the {role} write set: " + "; ".join(shown) + more,
    }
    write_json(task.c / "contaminated" / "incidents" / f"{inc_id}.json", incident)
    return incident
