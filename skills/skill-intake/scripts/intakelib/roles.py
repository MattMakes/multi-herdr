"""Role orchestration (spec 6.5-6.8): briefs, spawns, results, the plain fallback.

A role runs 1 at a time per task, in the clean-room order, because the domain
snapshot of a role is compared against that role's write set only (domain.py).

C/roles.jsonl is append-only. A `start` line records a role worker (fleet) or
a subagent (plain): role, worker, brief, status file, start time, attempt,
snapshot. An `end` line closes 1 start with `done`, `error` or `lost`. An
`abort` line records the second loss of a brief (exit 8).

A brief comes from a fixed template per role. It takes no free text: only the
task's paths, the controller's role name (HORCH_ROLE), and an optional unit
id or skill name that must be a plain name.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
from pathlib import Path

from . import domain, evidence, guard, scan
from . import task as tasklib
from .task import Task, TaskError, is_within, now, read_json, resolve, write_json

ROLES = ("auditor", "analyst", "sanitizer", "writer", "reviewer")
SIDE = {"auditor": "contaminated", "analyst": "contaminated", "sanitizer": "drafts",
        "writer": "clean", "reviewer": "clean"}
ROLES_DIR = guard.SKILL_DIR / "roles"
TEAMMATE_ENV = "SKILL_INTAKE_ROLE_TEAMMATE"
LOG = "roles.jsonl"
STATUS_KEYS = {"role", "status", "outputs", "error"}
NAME = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,63}")
SPAWNED = re.compile(r"^spawned (\S+) \(pane ", re.M)
ERROR_MAX = 200
SPAWN_TIMEOUT = 300  # seconds

# Exit codes (spec 10).
LOST_TWICE = 8
BRIEF_LEAK = 9
WRONG_MODE = 10
SPAWN_FAILED = 11
BAD_RESULT = 12
ROLE_ACTIVE = 14


class RoleError(TaskError):
    def __init__(self, code: int, message: str):
        super().__init__(message)
        self.code = code


def side_root(t: Task, role: str) -> Path:
    return t.root(SIDE[role])


def _plain_name(flag: str, value: str | None) -> None:
    if value is not None and not NAME.fullmatch(value):
        raise TaskError(f"{flag} must be a plain name ({NAME.pattern}): {evidence.escape(value)}")


# ---- briefs ----

def _io(t: Task, role: str, skill: str | None) -> tuple[list[Path], list[Path]]:
    """(read, write) paths of a role (spec 6.3)."""
    c, d, k = t.c, t.d, t.k
    out = k / "out" / skill if skill else k / "out"
    return {
        "auditor": ([c / "quarantine" / "src", c / "sweep"], [c / "sweep" / "audit.jsonl", c / "sweep" / "opaque"]),
        "analyst": ([c / "contaminated" / "source-index.json", c / "quarantine" / "src", c / "sweep",
                     c / "mitigation.json"], [d / "drafts"]),
        "sanitizer": ([d / "drafts"], [d / "drafts", d / "quarantine-cr"]),
        "writer": ([k / "handoff", k / "context.json"], [out, k / "report.json", k / "persona.md"]),
        "reviewer": ([k / "handoff", k / "context.json", out, k / "report.json", k / "persona.md"],
                     [out, k / "review.json"]),
    }[role]


def _next_number(t: Task, role: str) -> int:
    briefs = side_root(t, role) / "briefs"
    used = [int(m.group(1)) for p in briefs.glob(f"{role}-*.md")
            if (m := re.fullmatch(rf"{role}-(\d+)\.md", p.name))]
    return max(used, default=0) + 1


def render(t: Task, role: str, n: int, unit: str | None, skill: str | None, controller: str | None) -> str:
    """The fixed brief template of `role`."""
    reads, writes = _io(t, role, skill)
    status = side_root(t, role) / "status" / f"{role}-{n}.json"
    lines = [f"# Brief {role}-{n}", "", f"- Role: {role}", f"- Task: {t.id}",
             f"- Controller: {controller or 'none (plain session)'}"]
    if unit:
        lines.append(f"- Unit: {unit}")
    if skill:
        lines.append(f"- Skill: {skill}")
    lines += ["", "## Read only these paths", ""] + [f"- {p}" for p in reads]
    lines += ["", "## Write only these paths", ""] + [f"- {p}" for p in writes] + [f"- {status}"]
    lines += ["", "## Status file", "", f"- {status}", "",
              'Write it as JSON with exactly these keys: "role" ("' + role + '"), "status" ("ok" or "error"),'
              ' "outputs" (a list of the absolute paths you wrote), "error" (null, or 1 line).',
              "Put no source text in it.", "", "## When you finish", ""]
    if controller:
        lines += ["1. Write the status file.",
                  "2. Inside a ledger run, run horch ledger progress after each unit of work.",
                  f'3. Run: horch tell {controller} "[<your role>] DONE: {status}"',
                  f'4. Run: horch done "{role} <status> {status}"']
    else:
        lines += ["1. Write the status file.", "2. Stop. The controller reads the status file."]
    return "\n".join(lines) + "\n"


def _check_clean(t: Task, text: str, template: str) -> list[str]:
    """A clean-side brief names only K paths and passes the cr scan leakage checks.
    The words of the fixed template (`template`: the brief without a unit or a
    skill) are intake's own, so only the --unit and --skill values can hit."""
    problems = [f"names a path outside K: {evidence.escape(p)}"
                for p in re.findall(r"(?<![\w.])/[^\s\"]+", text) if not is_within(resolve(p), t.k)]
    own = set(re.findall(r"[A-Za-z0-9_]+", template))
    problems += [f"{f.check}: {evidence.escape(f.detail)}"
                 for f in scan.scan_text(text, t, own) if f.severity == "block"]
    return problems


def brief(t: Task, role: str, unit: str | None = None, skill: str | None = None) -> dict:
    """Write the next brief of `role`. Returns {"brief", "status_file"}. Exit 9 when a
    clean-side brief fails the leakage checks; nothing is written then."""
    if role not in ROLES:
        raise TaskError(f"unknown role {role!r}; roles: {', '.join(ROLES)}")
    _plain_name("--unit", unit)
    _plain_name("--skill", skill)
    controller = os.environ.get("HORCH_ROLE") if tasklib.fleet_mode() else None
    _plain_name("HORCH_ROLE", controller)
    n = _next_number(t, role)
    text = render(t, role, n, unit, skill, controller)
    if SIDE[role] == "clean":
        problems = _check_clean(t, text, render(t, role, n, None, None, controller))
        if problems:
            raise RoleError(BRIEF_LEAK, "the clean brief failed the leakage checks: " + "; ".join(problems))
    root = side_root(t, role)
    path = root / "briefs" / f"{role}-{n}.md"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    return {"brief": str(path), "status_file": str(root / "status" / f"{role}-{n}.json")}


# ---- the role log ----

def _log(t: Task) -> Path:
    return t.c / LOG


def _append(t: Task, line: dict) -> None:
    with open(_log(t), "a", encoding="utf-8") as f:
        f.write(json.dumps(line, ensure_ascii=True) + "\n")


def _lines(t: Task) -> list[dict]:
    path = _log(t)
    if not path.is_file():
        return []
    return [json.loads(ln) for ln in path.read_text(encoding="utf-8").splitlines() if ln.strip()]


def workers(t: Task) -> list[dict]:
    """Each start line with its state: running, done, error or lost."""
    starts, ends = [], {}
    for ln in _lines(t):
        if ln.get("kind") == "start":
            starts.append(dict(ln, state="running"))
        elif ln.get("kind") == "end":
            ends[ln["id"]] = ln["state"]
    for s in starts:
        s["state"] = ends.get(s["id"], "running")
    return starts


def aborts(t: Task) -> list[dict]:
    return [ln for ln in _lines(t) if ln.get("kind") == "abort"]


def _require_idle(t: Task) -> None:
    if aborts(t):
        raise RoleError(LOST_TWICE, f"the role stage of task {t.id} aborted: a brief was lost twice")
    active = [w for w in workers(t) if w["state"] == "running"]
    if active:
        w = active[0]
        raise RoleError(ROLE_ACTIVE, f"refused: role {w['role']} ({w['worker']}) is active and has no "
                                     f"role-result yet; roles run 1 at a time")


def _snapshot(t: Task, brief_path: Path) -> Path:
    path = t.c / "contaminated" / "snapshots" / f"{brief_path.stem}.json"
    write_json(path, domain.take(t))
    return path


def _start(t: Task, role: str, worker: str | None, brief_path: Path, mode: str, attempt: int,
           snapshot: Path) -> dict:
    line = {"kind": "start", "id": f"{brief_path.stem}#{attempt}", "role": role, "worker": worker,
            "brief": str(brief_path), "status_file": str(side_root(t, role) / "status" / f"{brief_path.stem}.json"),
            "started": now(), "attempt": attempt, "mode": mode, "snapshot": str(snapshot)}
    _append(t, line)
    return line


# ---- fleet: spawn ----

def spawn_argv(role: str, brief_path: Path) -> list[str]:
    """The exact argv of spec 6.5, after the horch binary."""
    teammate = os.environ.get(TEAMMATE_ENV) or f"intake-{role}"
    return ["spawn", teammate, "--exact", f"Read and follow {brief_path} exactly."]


def _run_spawn(role: str, brief_path: Path) -> str | None:
    """Run horch spawn; return the worker role name it printed. Exit 11 on failure."""
    argv = [tasklib.horch_bin(), *spawn_argv(role, brief_path)]
    try:
        r = subprocess.run(argv, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=SPAWN_TIMEOUT)
    except (OSError, subprocess.TimeoutExpired) as e:
        raise RoleError(SPAWN_FAILED, f"horch spawn failed: {evidence.escape(str(e))}") from e
    if r.returncode != 0:
        raise RoleError(SPAWN_FAILED, f"horch spawn exited {r.returncode}: "
                                      f"{evidence.escape(r.stderr.strip()[-ERROR_MAX:])}")
    m = SPAWNED.search(r.stderr + "\n" + r.stdout)
    return m.group(1) if m else None


def _require_fleet(what: str) -> None:
    if not tasklib.fleet_mode():
        raise RoleError(WRONG_MODE, f"refused: {what} runs in fleet mode only (intake mode prints plain)")


def spawn(t: Task, role: str, unit: str | None = None, skill: str | None = None) -> dict:
    """Brief + domain snapshot + horch spawn; records the start (spec 6.5)."""
    _require_fleet("intake spawn")
    _require_idle(t)
    b = Path(brief(t, role, unit, skill)["brief"])
    snap = _snapshot(t, b)
    worker = _run_spawn(role, b)
    return _start(t, role, worker, b, "fleet", 1, snap)


def mark_lost(t: Task, worker_id: str) -> dict:
    """Mark a running start lost, then spawn a fresh worker for the same brief once.
    A second loss of a brief writes an abort line and exits 8."""
    _require_fleet("intake roles --lost")
    match = [w for w in workers(t) if worker_id in (w["id"], w["worker"])]
    if not match:
        raise TaskError(f"no role worker {evidence.escape(worker_id)} in task {t.id}")
    w = match[-1]
    if w["state"] != "running":
        raise TaskError(f"role worker {w['id']} is {w['state']}, not running")
    _append(t, {"kind": "end", "id": w["id"], "state": "lost", "at": now()})
    if w["attempt"] >= 2:
        _append(t, {"kind": "abort", "brief": w["brief"], "role": w["role"], "at": now(),
                    "reason": f"brief {Path(w['brief']).name} was lost 2 times"})
        raise RoleError(LOST_TWICE, f"abort: brief {w['brief']} was lost 2 times")
    b = Path(w["brief"])
    # The first attempt's snapshot stays: role-result checks both workers' writes.
    worker = _run_spawn(w["role"], b)
    return _start(t, w["role"], worker, b, "fleet", w["attempt"] + 1, Path(w["snapshot"]))


# ---- plain: subagent prompt ----

def _brief_task(brief_path: Path) -> tuple[Task, str]:
    """The task and role of an intake brief path <B>/<side>/<task>/briefs/<role>-<n>.md."""
    m = re.fullmatch(r"([a-z]+)-\d+\.md", brief_path.name)
    if not m or m.group(1) not in ROLES or brief_path.parent.name != "briefs" or not brief_path.is_file():
        raise TaskError(f"not an intake brief: {brief_path}")
    role, root = m.group(1), brief_path.parent.parent
    t = tasklib.load(str(root.parent.parent / "contaminated" / root.name))
    if root != side_root(t, role):
        raise TaskError(f"brief {brief_path.name} is not in the {SIDE[role]} side root")
    return t, role


def subagent_prompt(role: str, brief_path: str) -> str:
    """Plain mode: the role file text plus 1 brief line. Takes the snapshot,
    sets the guard role, and records the start."""
    if tasklib.fleet_mode():
        raise RoleError(WRONG_MODE, "refused: subagent-prompt runs in plain mode only (intake mode prints fleet)")
    b = resolve(brief_path)
    t, brief_role = _brief_task(b)
    if brief_role != role:
        raise TaskError(f"brief {b.name} is a {brief_role} brief, not a {role} brief")
    role_file = ROLES_DIR / f"{role}.md"
    if not role_file.is_file():
        raise TaskError(f"no role file {role_file}")
    _require_idle(t)
    snap = _snapshot(t, b)
    guard.set_role(t, role)
    _start(t, role, "subagent", b, "plain", 1, snap)
    text = role_file.read_text(encoding="utf-8").rstrip("\n")
    return f"{text}\n\nYour brief is `{b}`. Read it and do only what it says.\n"


# ---- role-result ----

def _status_task(path: Path) -> Task:
    root = path.parent.parent
    if path.parent.name != "status" or root.parent.name not in tasklib.SIDES:
        raise RoleError(BAD_RESULT, f"not an intake status file: {path}")
    return tasklib.load(str(root.parent.parent / "contaminated" / root.name))


def _schema_problems(doc, w: dict, allowed: list[Path], own: Path) -> list[str]:
    if not isinstance(doc, dict):
        return ["the status file is not a JSON object"]
    problems = []
    extra, missing = set(doc) - STATUS_KEYS, STATUS_KEYS - set(doc)
    if extra:
        problems.append(f"extra field(s): {evidence.escape(', '.join(sorted(extra)))}")
    if missing:
        problems.append(f"missing field(s): {', '.join(sorted(missing))}")
    if doc.get("role") != w["role"]:
        problems.append(f"role is not {w['role']}")
    status, err = doc.get("status"), doc.get("error")
    if status not in ("ok", "error"):
        problems.append("status is not ok or error")
    if status == "ok" and err is not None:
        problems.append("error must be null when status is ok")
    if status == "error" and not (isinstance(err, str) and err.strip()):
        problems.append("error must be 1 line when status is error")
    if isinstance(err, str) and ("\n" in err or "\r" in err or len(err) > ERROR_MAX):
        problems.append(f"error is more than 1 line (or over {ERROR_MAX} characters)")
    elif err is not None and not isinstance(err, str):
        problems.append("error is not null or a string")
    outputs = doc.get("outputs")
    if not isinstance(outputs, list) or not all(isinstance(p, str) for p in outputs):
        problems.append("outputs is not a list of paths")
    else:
        for p in outputs:
            # The role's own status file is a write path of the brief; no other file under status/ is.
            if not os.path.isabs(p) or not (resolve(p) == own or any(is_within(resolve(p), a) for a in allowed)):
                problems.append(f"output outside the {w['role']} write paths: {evidence.escape(p)}")
    return problems


def role_result(status_path: str) -> dict:
    """Check a status file (spec 6.6) and the domain snapshot; record the end."""
    path = resolve(status_path)
    t = _status_task(path)
    match = [w for w in workers(t) if w["status_file"] == str(path) and w["state"] == "running"]
    if not match:
        raise RoleError(BAD_RESULT, f"no running role in task {t.id} has the status file {path}")
    w = match[-1]
    if not path.is_file():
        raise RoleError(BAD_RESULT, f"the {w['role']} role wrote no status file yet: {path}")
    incident = domain.compare(t, w["role"], read_json(Path(w["snapshot"])), domain.take(t))
    if w["mode"] == "plain":
        guard.clear_role()
    problems = []
    try:
        doc = read_json(path)
    except (OSError, ValueError) as e:
        doc, problems = None, [f"unreadable status file: {evidence.escape(str(e))}"]
    if doc is not None:
        # The write paths the brief names (`_io`); K/out covers every skill folder.
        problems = _schema_problems(doc, w, _io(t, w["role"], None)[1], path)
    if incident:
        problems.append(f"domain incident {incident['incident_id']}: {incident['notes']}")
    state = "error" if problems or doc.get("status") != "ok" else "done"
    _append(t, {"kind": "end", "id": w["id"], "state": state, "at": now(),
                "incident": incident["incident_id"] if incident else None})
    if problems:
        raise RoleError(BAD_RESULT, "bad role result: " + "; ".join(problems))
    return {"role": w["role"], "worker": w["worker"], "status": doc["status"],
            "outputs": [str(resolve(p)) for p in doc["outputs"]],
            "error": evidence.escape(doc["error"]) if doc["error"] else None}
