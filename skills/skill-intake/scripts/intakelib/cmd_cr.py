"""`intake cr <action> --task T`: the clean-room commands, ported from clean-room cr.py.

Changes from clean-room (spec 6.3, 6.10, 9.3):
- the task already exists (intake acquire made it); `cr init` writes only the
  unattended preflight, with no approval question;
- `cr index` makes 1 unit per skill, plus 1 per script over 300 lines;
- `cr scan` checks files headed for the clean side (in D or K, never in C);
- `cr handoff` is the only way from D to K;
- `cr verify` runs offline in sandbox-exec or bwrap, else marks the run
  `unverified` (exit 13);
- `cr role set` refuses in fleet mode (decision Q5: no guard in fleet runs).

Exit codes: 0 ok; 1 leak, invalid artifact, rejected handoff or failed
integrity; 2 bad input; 10 refused in fleet mode; 13 verify unverified;
otherwise the verified command's own exit code.

`cr init`, `cr index`, `cr handoff` and `cr verify` refuse (exit 2) a task whose
GATE 1 result is not pass (`intake triage`).
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from . import evidence, guard, handoff, index, sandbox, scan, schema, triage
from . import task as tasklib
from .task import is_within, read_json, resolve, write_json

HELP = "the clean-room commands: init, index, role, scan, handoff, context, verify, status, validate"
FLEET_REFUSED = 10
ARTIFACT_KINDS = {  # file name -> schema kind, for `cr status` and `cr validate` auto-detection
    "preflight.json": "preflight", "report.json": "report", "review.json": "report",
    "package.json": "handoff", "findings.json": "findings", "mitigation.json": "mitigation",
    "provenance.json": "provenance", "verify.json": "verify",
}


def kind_for(path: Path) -> str | None:
    if path.name in ARTIFACT_KINDS:
        return ARTIFACT_KINDS[path.name]
    if path.name.startswith("spec-"):
        return "behavior-spec"
    if path.name.startswith("inc-"):
        return "incident"
    return None


def _out(data) -> None:
    print(json.dumps(data, indent=2, ensure_ascii=True))


def _err(line: str) -> None:
    print(line, file=sys.stderr)


def preflight(task: tasklib.Task) -> dict:
    """The generated preflight of spec 6.3. Needs provenance.json from intake acquire."""
    prov_path = task.c / "provenance.json"
    if not prov_path.is_file():
        raise tasklib.TaskError(f"no provenance.json in {task.c}; run intake acquire first")
    prov = read_json(prov_path)
    source = prov.get("source", {}).get("input") or "an unnamed source"
    lic = prov.get("licence", {})
    licence = lic.get("spdx") or "no SPDX id"
    return {
        "goal": "Rewrite each skill of this task as a clean skill with the same observable behavior",
        "target_stack": "Claude Code skill: Markdown plus standard-library scripts",
        "authorization": {"confirmed": True,
                          "basis": f"operator invoked skill-intake on {evidence.escape(source)}; "
                                   f"licence {evidence.escape(licence)}/{lic.get('class', 'none')}"},
        "source_scope": [],
        "destination": "the clean output folder of this task",
        "policies": {"compatibility": "behavioral", "dependencies": "stdlib-only",
                     "license": "the operator's own terms", "features": "parity"},
        "mode": "unattended",
        "max_iterations": 3,
        "open_questions": [],
        "confirmed_by_user": True,
    }


def cmd_init(a) -> int:
    t = tasklib.load(a.task)
    triage.require_gate1_pass(t)
    path = t.c / "contaminated" / "preflight.json"
    write_json(path, preflight(t))
    tasklib.set_phase(t, "preflight")
    _out({"preflight": str(path)})
    return 0


def cmd_index(a) -> int:
    t = tasklib.load(a.task)
    triage.require_gate1_pass(t)
    idx = index.build(t)
    tasklib.set_phase(t, "decompose")
    _out({"source_index": str(t.c / "contaminated" / "source-index.json"),
          "fingerprints": str(t.c / "contaminated" / "fingerprints.json"),
          "files": idx["totals"]["files"], "lines": idx["totals"]["lines"],
          "units": [{"unit_id": u["unit_id"], "kind": u["kind"]} for u in idx["units"]]})
    _err(f"indexed {idx['totals']['files']} files, {idx['totals']['lines']} lines, {len(idx['units'])} units")
    return 0


def _inside(path: Path, t: tasklib.Task | None) -> bool:
    return t is None or any(is_within(path, t.root(s)) for s in tasklib.SIDES)


def cmd_validate(a) -> int:
    t = tasklib.load(a.task) if a.task else None
    results, failed = [], 0
    for p in a.paths:
        path = resolve(p)
        kind = a.kind or kind_for(path)
        if not _inside(path, t):
            results.append({"path": str(path), "valid": False, "errors": ["outside the task's side roots"]})
        elif not kind:
            results.append({"path": str(path), "valid": False,
                            "errors": [f"cannot tell the artifact kind; pass --kind ({', '.join(schema.kinds())})"]})
        else:
            errors = schema.validate(read_json(path), schema.load_schema(kind))
            results.append({"path": str(path), "kind": kind, "valid": not errors,
                            "errors": [evidence.escape(e) for e in errors]})
        failed += not results[-1]["valid"]
        _err(f"{path}: {'ok' if results[-1]['valid'] else 'INVALID'}")
    _out({"results": results})
    return 1 if failed else 0


def cmd_scan(a) -> int:
    t = tasklib.load(a.task)
    results, bad = [], 0
    for p in a.paths:
        path = resolve(p)
        if is_within(path, t.c) or not (is_within(path, t.d) or is_within(path, t.k)):
            raise tasklib.TaskError(f"cr scan checks files in the drafts or clean root only: {path}")
        findings = scan.scan_file(path, t)
        ok = scan.passed(findings)
        results.append({"path": str(path), "passed": ok, "findings": [
            {"check": f.check, "severity": f.severity, "detail": evidence.escape(f.detail)} for f in findings]})
        _err(f"{path}: {'clean' if ok else 'LEAK'}")
        bad += not ok
    _out({"results": results})
    return 1 if bad else 0


def cmd_handoff(a) -> int:
    t = tasklib.load(a.task)
    triage.require_gate1_pass(t)
    done, failures = handoff.handoff(t, a.drafts)
    if done:
        tasklib.set_phase(t, "clean")
    _out({"handed_off": done, "rejected": [evidence.escape(f) for f in failures],
          "package": str(t.k / "handoff" / "package.json")})
    for f in failures:
        _err(f"REJECTED {evidence.escape(f)}")
    return 1 if failures else 0


def cmd_context(a) -> int:
    _out({"context": str(handoff.write_context(tasklib.load(a.task)))})
    return 0


def cmd_verify(a) -> int:
    argv = a.argv[1:] if a.argv[:1] == ["--"] else a.argv
    t = tasklib.load(a.task)
    triage.require_gate1_pass(t)
    code, entry = sandbox.run(t, argv, a.timeout)
    _out({k: entry[k] for k in ("status", "sandbox", "exit_code")})
    if entry["output_tail"]:
        _err(entry["output_tail"])
    if entry["status"] == "unverified":
        _err("no usable sandbox: the command did not run; marked unverified")
    return code


def cmd_role(a) -> int:
    if a.action == "clear":
        guard.clear_role()
        _out({"role": None})
        return 0
    if a.action == "show":
        path = guard.state_path()
        _out(read_json(path) if path.is_file() else {"role": None})
        return 0
    if tasklib.fleet_mode():
        _err("intake: refused: no guard in fleet runs (each role worker has its own OS sandbox)")
        return FLEET_REFUSED
    if not a.role or not a.task:
        raise tasklib.TaskError("usage: intake cr role set <role> --task <dir>")
    guard.set_role(tasklib.load(a.task), a.role)
    _out({"role": a.role})
    _err(f"active role: {a.role} (subagent tool calls are now confined)")
    return 0


def cmd_status(a) -> int:
    t = tasklib.load(a.task)
    cc = t.c / "contaminated"
    checks = [
        ("preflight", cc / "preflight.json"), ("source index", cc / "source-index.json"),
        ("fingerprints", cc / "fingerprints.json"), ("handoff package", t.k / "handoff" / "package.json"),
        ("clean context", t.k / "context.json"), ("writer report", t.k / "report.json"),
        ("review", t.k / "review.json"),
    ]
    states = {}
    for label, path in checks:
        if not path.is_file():
            state = "missing"
        else:
            kind = kind_for(path)
            errs = schema.validate(read_json(path), schema.load_schema(kind)) if kind else []
            state = "ok" if not errs else "invalid"
        states[label] = state
    integrity = [p for p in handoff.verify_package(t) if p != "no handoff package yet"]
    status = {
        "task_id": t.id, "phase": t.manifest["phase"], "artifacts": states,
        "drafts": len(list((t.d / "drafts").glob("spec-*.json"))),
        "incidents": len(list(handoff.incidents_dir(t).glob("inc-*.json"))),
        "handoff_integrity": integrity,
    }
    _out(status)
    _err(f"task {t.id}  phase: {t.manifest['phase']}")
    for label, state in states.items():
        _err(f"  {label:<22} {state}")
    for p in integrity:
        _err(f"  HANDOFF INTEGRITY: {p}")
    return 1 if integrity else 0


def add_arguments(p: argparse.ArgumentParser) -> None:
    sub = p.add_subparsers(dest="cr_action", metavar="<action>", required=True)

    def task_cmd(name, fn, help_, required=True):
        s = sub.add_parser(name, help=help_)
        s.add_argument("--task", required=required, help="the task's contaminated side root (acquire prints it)")
        s.set_defaults(_cr=fn)
        return s

    task_cmd("init", cmd_init, "write the unattended preflight.json")
    task_cmd("index", cmd_index, "index the quarantined source into units and fingerprints")
    s = task_cmd("role", cmd_role, "set, clear or show the active role for the guard hook (plain mode)", required=False)
    s.add_argument("action", choices=["set", "clear", "show"])
    s.add_argument("role", nargs="?", choices=list(guard.ROLES))
    task_cmd("scan", cmd_scan, "leakage-scan files headed for the clean side").add_argument("paths", nargs="+")
    task_cmd("handoff", cmd_handoff, "move approved drafts from D/drafts to K/handoff").add_argument("drafts", nargs="+")
    task_cmd("context", cmd_context, "write K/context.json from the preflight")
    s = task_cmd("verify", cmd_verify, "run a test command offline in the sandbox (cwd = K/out)")
    s.add_argument("--timeout", type=int, default=600)
    s.add_argument("argv", nargs=argparse.REMAINDER)
    task_cmd("status", cmd_status, "show which artifacts exist and the handoff integrity")
    s = task_cmd("validate", cmd_validate, "schema-check artifacts", required=False)
    s.add_argument("paths", nargs="+")
    s.add_argument("--kind", choices=schema.kinds())


def run(args) -> int:
    try:
        return args._cr(args)
    except (ValueError, RuntimeError, FileNotFoundError, json.JSONDecodeError) as exc:
        _err(f"intake cr: {evidence.escape(str(exc))}")
        return 2
