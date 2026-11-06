"""The wall crossing: move sanitizer-approved specs to the clean side, or quarantine them.

Ported from clean-room crlib/handoff.py and changed to the 3 side roots (spec 9.3):

- drafts are read only from D/drafts/;
- a rejected draft is copied to D/quarantine-cr/, and its incident goes to
  C/contaminated/incidents/;
- approved specs, stripped of provenance and review, go to K/handoff/ with
  their sha256 in K/handoff/package.json. This is the only way from D to K.
"""

from __future__ import annotations

import hashlib
import json
import secrets
import shutil
from pathlib import Path

from . import scan, schema
from .targets import RULES
from .task import Task, is_within, now, read_json, resolve, write_json

STRIPPED = ("provenance", "review")


def incidents_dir(task: Task) -> Path:
    return task.c / "contaminated" / "incidents"


def _incident(task: Task, artifact: Path, checks: list[str], notes: str) -> str:
    inc_id = "inc-" + secrets.token_hex(4)
    write_json(incidents_dir(task) / f"{inc_id}.json", {
        "incident_id": inc_id, "detected": now(), "artifact": artifact.name,
        "checks": checks, "action": "quarantined", "notes": notes,
    })
    shutil.copy2(artifact, task.d / "quarantine-cr" / f"{inc_id}-{artifact.name}")
    return inc_id


def handoff(task: Task, drafts: list[str]) -> tuple[list[str], list[str]]:
    """Return (handed-off spec ids, failure messages)."""
    drafts_dir = task.d / "drafts"
    out_dir = task.k / "handoff"
    pkg_path = out_dir / "package.json"
    package = read_json(pkg_path) if pkg_path.is_file() else {"task_id": task.id, "created": now(), "items": []}
    done, failures = [], []

    for raw in drafts:
        path = resolve(raw)
        if not is_within(path, drafts_dir) or not path.is_file():
            failures.append(f"{path.name}: drafts must be files in {drafts_dir}")
            continue
        try:
            spec = read_json(path)
        except json.JSONDecodeError as e:
            failures.append(f"{path.name}: not JSON ({e.msg})")
            continue
        errors = schema.validate(spec, schema.load_schema("behavior-spec"))
        if errors:
            failures.append(f"{path.name}: schema: " + "; ".join(errors[:5]))
            continue
        review = spec.get("review", {})
        if review.get("status") != "approved":
            failures.append(f"{path.name}: not approved by the sanitizer (review.status={review.get('status')!r})")
            continue

        clean = {k: v for k, v in spec.items() if k not in STRIPPED}
        findings = scan.scan_text("\n".join(scan.strings_of(clean)), task, scan.public_names(clean))
        if not scan.passed(findings):
            checks = [f.check for f in findings if f.severity == "block"]
            inc = _incident(task, path, checks, "; ".join(f.detail for f in findings if f.severity == "block"))
            failures.append(f"{path.name}: leakage ({', '.join(checks)}); quarantined as {inc}")
            continue

        body = (json.dumps(clean, indent=2, ensure_ascii=False) + "\n").encode()
        fname = f"{clean['spec_id']}.json"
        (out_dir / fname).write_bytes(body)
        package["items"] = [i for i in package["items"] if i["spec_id"] != clean["spec_id"]]
        package["items"].append({"spec_id": clean["spec_id"], "file": fname, "sha256": hashlib.sha256(body).hexdigest()})
        done.append(clean["spec_id"])

    package["created"] = now()
    write_json(pkg_path, package)
    return done, failures


def verify_package(task: Task) -> list[str]:
    """Check every handed-off file still matches its recorded hash."""
    out_dir = task.k / "handoff"
    pkg_path = out_dir / "package.json"
    if not pkg_path.is_file():
        return ["no handoff package yet"]
    problems = []
    for item in read_json(pkg_path)["items"]:
        f = out_dir / item["file"]
        if not f.is_file():
            problems.append(f"missing {item['file']}")
        elif hashlib.sha256(f.read_bytes()).hexdigest() != item["sha256"]:
            problems.append(f"{item['file']} changed after handoff")
    return problems


def write_context(task: Task, target: str | None = None) -> Path:
    """K/context.json: the only task description clean roles ever see. No source paths.
    With a `target`, it also holds that target's rules (spec 7.3), which verify checks."""
    pre_path = task.c / "contaminated" / "preflight.json"
    if not pre_path.is_file():
        raise ValueError("no preflight.json; run intake cr init first")
    pre = read_json(pre_path)
    errors = schema.validate(pre, schema.load_schema("preflight"))
    if errors:
        raise ValueError("preflight.json is invalid: " + "; ".join(errors[:5]))
    if not pre["confirmed_by_user"] or pre["open_questions"]:
        raise ValueError("preflight must be confirmed with no open questions")
    pkg = task.k / "handoff" / "package.json"
    context = {
        "task_id": task.id,
        "goal": pre["goal"],
        "target_stack": pre["target_stack"],
        "destination": pre["destination"],
        "policies": pre["policies"],
        "max_iterations": pre.get("max_iterations", 3),
        "clean_root": str(task.k),
        "out_root": str(task.k / "out"),
        "specs": [i["file"] for i in read_json(pkg)["items"]] if pkg.is_file() else [],
    }
    if target is not None:
        context["target"] = target
        context["target_rules"] = RULES[target]
    findings = scan.scan_text("\n".join(scan.strings_of({k: v for k, v in context.items() if not k.endswith("_root")})), task)
    if not scan.passed(findings):
        raise ValueError("context would leak: " + "; ".join(f.check for f in findings if f.severity == "block"))
    path = task.k / "context.json"
    write_json(path, context)
    return path
