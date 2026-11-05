"""`intake acquire <source>`: stage 1 and provenance (spec 3.1-3.5, 3.8).

stdout: {"task_dir": C} (plus "refused" on exit 3). Exit codes: 0 ok,
2 bad input, 3 refused (ACQ-01 or ACQ-04; nothing is left in quarantine).
ACQ findings go to C/sweep/acquire.jsonl, one JSON line each.
"""

from __future__ import annotations

import json
import shutil
import sys
from pathlib import Path

from . import acquire, evidence, licensing, task
from .acquire import BadSource, Caps, Refused

HELP = "fetch a skill source into a read-only quarantine and write provenance.json"
FINDINGS = "sweep/acquire.jsonl"
SEVERITY = {"ACQ-01": "high", "ACQ-02": "low", "ACQ-03": "low", "ACQ-04": "high"}


def add_arguments(p) -> None:
    caps = Caps()
    p.add_argument("source", help="a GitHub form, a local folder, or a .zip/.skill archive")
    p.add_argument("--ref", help="branch, tag or 40-hex commit (GitHub only)")
    p.add_argument("--path", help="a subfolder of the source to take")
    p.add_argument("--base", help="the workspace base (not in fleet mode)")
    p.add_argument("--max-total-bytes", type=int, default=caps.total_bytes)
    p.add_argument("--max-files", type=int, default=caps.files)
    p.add_argument("--max-file-bytes", type=int, default=caps.file_bytes)
    p.add_argument("--max-depth", type=int, default=caps.depth)


def finding(n: int, rule: str, file: str | None, detail: str) -> dict:
    return {
        "id": f"{rule}#{n}", "rule": rule, "severity": SEVERITY[rule],
        "file": evidence.escape(file) if file is not None else None, "line": None,
        "evidence": evidence.untrusted(detail), "malicious": False,
    }


def write_findings(t: task.Task, rows: list[dict]) -> None:
    path = t.c / FINDINGS
    with open(path, "w", encoding="utf-8") as f:
        for row in rows:
            f.write(json.dumps(row, ensure_ascii=True) + "\n")


def _skill_dirs(src: Path) -> list[Path]:
    return sorted({p.parent for p in src.rglob("SKILL.md") if p.is_file() and not p.is_symlink()})


def run(args) -> int:
    caps = Caps(args.max_total_bytes, args.max_files, args.max_file_bytes, args.max_depth)
    if min(caps.total_bytes, caps.files, caps.file_bytes, caps.depth) < 1:
        print("intake acquire: every cap must be 1 or more", file=sys.stderr)
        return 2
    try:
        src = acquire.parse(args.source, args.ref, args.path)
        if src.kind == "github":
            acquire.clone_url(src)  # check the test seam before any task exists
    except BadSource as e:
        print(f"intake acquire: {e}\n{acquire.ACCEPTED_FORMS}", file=sys.stderr)
        return 2
    base = task.resolve(args.base) if args.base is not None else task.default_base()
    if src.path is not None and task.overlaps(src.path, base):
        print("intake acquire: the source and the workspace base overlap", file=sys.stderr)
        return 2
    t = task.create(args.base)
    q = t.c / "quarantine" / "src"
    q.mkdir()
    prov = {
        "task_id": t.id, "fetched_at": task.now(),
        "source": {"kind": src.kind, "input": evidence.escape(src.input), "url": src.url,
                   "ref": src.ref, "commit": None, "subdir": src.subdir},
        "content_sha256": None, "archive_sha256": None, "licence": None,
        "skills": [], "intake_version": None, "sweep": None, "installed": [],
    }
    skipped = acquire.Skipped()
    repo_licence = None  # the repo root licence when only a subfolder is taken
    try:
        if src.kind == "github":
            staging = t.c / "quarantine" / ".clone"
            try:
                prov["source"]["commit"] = acquire.clone(src, staging)
                root = staging / src.subdir if src.subdir else staging
                if not root.is_dir() or root.is_symlink():
                    raise BadSource(f"--path {src.subdir!r} is not a folder in the repo")
                skipped = acquire.copy_tree(root, q, caps, skip_git=True)
                if src.subdir:
                    repo_licence = licensing.detect(staging, [staging])
            finally:
                shutil.rmtree(staging, ignore_errors=True)
        elif src.kind == "folder":
            root = src.path / src.subdir if src.subdir else src.path
            if not root.is_dir() or root.is_symlink():
                raise BadSource(f"--path {src.subdir!r} is not a folder in the source")
            skipped = acquire.copy_tree(root, q, caps, skip_git=True)
        else:
            prov["archive_sha256"] = acquire.sha256_file(src.path)
            acquire.extract(src.path, q, caps)
    except (BadSource, OSError) as e:
        acquire.clear(t.c / "quarantine")
        task.set_phase(t, "failed")
        print(f"intake acquire: {e}", file=sys.stderr)
        return 2
    except Refused as e:
        acquire.clear(q)
        write_findings(t, [finding(1, e.rule, e.file, e.reason)])
        task.set_phase(t, "refused")
        task.write_json(t.c / "provenance.json", prov)
        where = f" ({evidence.escape(e.file)})" if e.file else ""
        print(f"intake acquire: refused, {e.rule}: {e.reason}{where}", file=sys.stderr)
        print(json.dumps({"task_dir": str(t.c), "refused": e.rule}))
        return 3
    write_findings(t, [finding(i, r, f, d) for i, (r, f, d) in enumerate(skipped.items, 1)])
    prov["content_sha256"] = acquire.tree_sha256(q)
    found = licensing.detect(q, [q, *_skill_dirs(q)])
    if repo_licence and repo_licence["files"] and (
        not found["files"] or licensing.RANK[repo_licence["class"]] > licensing.RANK[found["class"]]
    ):
        found = repo_licence
    prov["licence"] = found
    acquire.make_read_only(q)
    task.write_json(t.c / "provenance.json", prov)
    task.set_phase(t, "acquired")
    for rule, rel, _ in skipped.items:
        print(f"intake acquire: skipped {rule} {evidence.escape(rel)}", file=sys.stderr)
    print(json.dumps({"task_dir": str(t.c)}))
    return 0
