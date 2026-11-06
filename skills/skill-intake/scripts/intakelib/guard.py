"""PreToolUse hook: confine role subagents to their trust domain (plain mode only).

Ported from clean-room crlib/guard.py and changed for skill-intake:

- the roles are the 5 of spec 6.3 (auditor, analyst, sanitizer, writer,
  reviewer), each held to its side root of spec 9.3;
- the active-role state lives at <base>/active-role.json, not in ~/.claude;
- the only shell command a role may run is `python3 -I <intake.py> cr verify ...`;
- fleet runs use no guard (decision Q5): each role worker is its own process
  with its own OS sandbox. `intake hook-config` and `intake cr role set`
  refuse in fleet mode.

The controller (main session) marks the active role with `intake cr role set`. While a
role is active, every tool call made *inside a subagent* (hook input carries `agent_id`)
is checked against that role's read/write/shell rules. The main session is never
restricted, so the controller can always run `intake cr role clear`. With no active
role the hook does nothing.

Blocking uses exit code 2 with the reason on stderr (Claude Code PreToolUse semantics).
"""

from __future__ import annotations

import json
import os
import shlex
import sys
from pathlib import Path

from .task import Task, default_base, is_within, load, now, read_json, resolve, write_json

SKILL_DIR = Path(__file__).resolve().parent.parent.parent
INTAKE = str(SKILL_DIR / "scripts" / "intake.py")

# read/write: domain names (see domain_paths). shell: may run `intake.py cr verify`.
# side: the side root a role belongs to; a role never reads the other side roots.
ROLES = {
    "auditor":   {"read": ["contaminated", "skill"], "write": ["sweep"], "shell": False, "side": "contaminated"},
    "analyst":   {"read": ["contaminated", "skill"], "write": ["drafts"], "shell": False, "side": "contaminated"},
    "sanitizer": {"read": ["drafts-root", "skill"], "write": ["drafts", "quarantine-cr"], "shell": False, "side": "drafts"},
    "writer":    {"read": ["clean", "skill"], "write": ["out", "report"], "shell": True, "side": "clean"},
    "reviewer":  {"read": ["clean", "skill"], "write": ["out", "review"], "shell": True, "side": "clean"},
}
READ_TOOLS = {"Read": "file_path", "Glob": "path", "Grep": "path", "LS": "path", "NotebookRead": "notebook_path"}
WRITE_TOOLS = {"Write": "file_path", "Edit": "file_path", "MultiEdit": "file_path", "NotebookEdit": "notebook_path"}
HARMLESS = {"TodoWrite", "TaskCreate", "TaskUpdate", "TaskList", "TaskGet"}
SHELL_META = set(";&|`$<>\n\\")


def state_path() -> Path:
    return default_base() / "active-role.json"


def set_role(task: Task, role: str) -> None:
    if role not in ROLES:
        raise ValueError(f"unknown role '{role}'; roles: {', '.join(ROLES)}")
    write_json(state_path(), {"task_dir": str(task.c), "role": role, "set_at": now()})


def clear_role() -> None:
    state_path().unlink(missing_ok=True)


def domain_paths(task: Task, names: list[str]) -> list[Path]:
    table = {
        "contaminated": task.c, "sweep": task.c / "sweep",
        "drafts-root": task.d, "drafts": task.d / "drafts", "quarantine-cr": task.d / "quarantine-cr",
        "clean": task.k, "out": task.k / "out",
        "report": task.k / "report.json", "review": task.k / "review.json",
        "skill": SKILL_DIR,
    }
    return [table[n] for n in names]


def allowed(path: Path, task: Task, names: list[str]) -> bool:
    return any(is_within(path, root) for root in domain_paths(task, names))


def shell_ok(command: str) -> bool:
    if any(c in SHELL_META for c in command):
        return False
    try:
        argv = shlex.split(command)
    except ValueError:
        return False
    if argv[:1] == ["python3"] and argv[1:2] == ["-I"]:
        argv = argv[2:]
    elif argv[:1] == ["python3"]:
        argv = argv[1:]
    return (len(argv) >= 3 and os.path.realpath(os.path.expanduser(argv[0])) == os.path.realpath(INTAKE)
            and argv[1:3] == ["cr", "verify"])


def decide(event: dict, state: dict | None) -> str | None:
    """Return a denial reason, or None to allow."""
    if not state or not event.get("agent_id"):
        return None
    task = load(state["task_dir"])
    role = state["role"]
    rules = ROLES[role]
    tool = event.get("tool_name", "")
    args = event.get("tool_input") or {}
    cwd = resolve(event.get("cwd") or os.getcwd())

    def target(key: str) -> Path:
        raw = args.get(key) or str(cwd)
        p = Path(os.path.expanduser(raw))
        return (p if p.is_absolute() else cwd / p).resolve()

    if tool in READ_TOOLS:
        path = target(READ_TOOLS[tool])
        if any(is_within(path, h) or is_within(h, path) for h in task.paths_hidden_from(rules["side"])):
            return f"{role} may not read another side root ({path})"
        if not allowed(path, task, rules["read"]):
            return f"{role} may only read: {', '.join(rules['read'])} ({path})"
        return None
    if tool in WRITE_TOOLS:
        path = target(WRITE_TOOLS[tool])
        if not allowed(path, task, rules["write"]):
            return f"{role} may only write: {', '.join(rules['write'])} ({path})"
        return None
    if tool == "Bash":
        if rules["shell"] and shell_ok(args.get("command", "")):
            return None
        return f"{role} may not use the shell" + (
            f" except `python3 -I {INTAKE} cr verify --task <dir> -- <argv>`" if rules["shell"] else "")
    if tool in HARMLESS:
        return None
    return f"{role} may not use {tool} (roles cannot browse the web, spawn agents, or call MCP tools)"


def main() -> int:
    path = state_path()
    try:
        event = json.load(sys.stdin)
        state = read_json(path) if path.is_file() else None
        reason = decide(event, state)
    except Exception as exc:  # fail closed while a role is active
        if path.is_file():
            print(f"intake guard error, blocking: {exc}", file=sys.stderr)
            return 2
        return 0
    if reason:
        print(f"intake guard: {reason}", file=sys.stderr)
        return 2
    return 0
