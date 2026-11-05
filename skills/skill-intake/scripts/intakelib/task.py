"""Task layout: one task, 3 side roots, one per trust domain.

Ported from clean-room crlib/task.py and changed to the layout of spec 9.3:

B/contaminated/<task>/   = C  (controller, auditor, analyst); holds task.json
B/drafts/<task>/         = D  (analyst writes, sanitizer reads and writes)
B/clean/<task>/          = K  (writer, reviewer)

B is ${XDG_STATE_HOME:-~/.local/state}/skill-intake/. In fleet mode B is
always ~/.local/state/skill-intake/ and a --base is refused, because the role
teammates' sandbox rules name that path. The task id is
<UTC yyyymmdd-hhmmss>-<6 hex>: opaque, so no path carries the source name.
"""

from __future__ import annotations

import json
import os
import secrets
import tempfile
from datetime import datetime, timezone
from pathlib import Path

SIDES = ("contaminated", "drafts", "clean")
SUBDIRS = {
    "contaminated": ("quarantine", "sweep", "contaminated", "briefs", "status"),
    "drafts": ("drafts", "quarantine-cr", "briefs", "status"),
    "clean": ("handoff", "out", "verify-log", "briefs", "status"),
}
MANIFEST = "task.json"


class TaskError(Exception):
    pass


class BaseRefused(TaskError):
    """The workspace base is in a place spec 3.5 forbids."""


def _clock() -> datetime:
    """UTC now, or the fixed time in SKILL_INTAKE_NOW (test seam, ISO 8601)."""
    fixed = os.environ.get("SKILL_INTAKE_NOW")
    if not fixed:
        return datetime.now(timezone.utc).replace(microsecond=0)
    try:
        dt = datetime.fromisoformat(fixed.replace("Z", "+00:00"))
    except ValueError as e:
        raise TaskError(f"SKILL_INTAKE_NOW is not an ISO 8601 time: {fixed!r}") from e
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=timezone.utc)
    return dt.astimezone(timezone.utc).replace(microsecond=0)


def now() -> str:
    return _clock().isoformat()


def new_id() -> str:
    return _clock().strftime("%Y%m%d-%H%M%S") + "-" + secrets.token_hex(3)


def resolve(p: str | os.PathLike) -> Path:
    return Path(os.path.expanduser(str(p))).resolve()


def is_within(path: Path, root: Path) -> bool:
    try:
        path.relative_to(root)
        return True
    except ValueError:
        return False


def overlaps(a: Path, b: Path) -> bool:
    return is_within(a, b) or is_within(b, a)


def write_json(path: Path, data) -> None:
    """Atomic write so a crash never leaves a half-written artifact."""
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=path.parent, prefix=".tmp-", suffix=".json")
    with os.fdopen(fd, "w", encoding="utf-8") as f:
        json.dump(data, f, indent=2, ensure_ascii=False)
        f.write("\n")
    os.replace(tmp, path)


def read_json(path: Path):
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def fleet_mode() -> bool:
    # Ticket 12 adds the full mode rule (spec 6.2); SKILL_INTAKE_MODE overrides it.
    return os.environ.get("SKILL_INTAKE_MODE") == "fleet"


def _home() -> Path:
    return Path.home().resolve()


def default_base() -> Path:
    if fleet_mode():
        return _home() / ".local" / "state" / "skill-intake"
    xdg = os.environ.get("XDG_STATE_HOME")
    state = resolve(xdg) if xdg else _home() / ".local" / "state"
    return state / "skill-intake"


def _current_repo() -> Path | None:
    """The git work tree that holds the current directory (a .git dir or file)."""
    here = Path.cwd().resolve()
    for d in (here, *here.parents):
        if (d / ".git").exists():
            return d
    return None


def check_base(base: Path) -> None:
    """Raise BaseRefused when `base` (resolved) is inside a forbidden place (spec 3.5)."""
    home = _home()
    forbidden = [
        (home / ".claude", "~/.claude"),
        (home / ".codex", "~/.codex"),
        (home / ".agents", "~/.agents"),
        (home / "Library" / "Mobile Documents", "an iCloud-synced folder"),
        (home / "Library" / "CloudStorage", "a cloud-synced folder"),
        # iCloud "Desktop & Documents" can sync these; refuse them always.
        (home / "Documents", "a folder iCloud can sync (~/Documents)"),
        (home / "Desktop", "a folder iCloud can sync (~/Desktop)"),
    ]
    repo = _current_repo()
    if repo is not None:
        forbidden.append((repo, f"the current repo {repo}"))
    for root, label in forbidden:
        if is_within(base, root.resolve()):
            raise BaseRefused(f"refused --base {base}: it is inside {label}")
    parts = base.parts
    for i in range(len(parts) - 1):
        if parts[i] in (".claude", ".codex", ".agents") and parts[i + 1] == "skills":
            raise BaseRefused(f"refused --base {base}: it is inside a {parts[i]}/skills folder")
    for d in (base, *base.parents):
        if (d / ".claude-plugin").is_dir():
            raise BaseRefused(f"refused --base {base}: it is inside the plugin folder {d}")


class Task:
    def __init__(self, base: Path, manifest: dict):
        self.base = base
        self.manifest = manifest

    @property
    def id(self) -> str:
        return self.manifest["task_id"]

    def root(self, side: str) -> Path:
        if side not in SIDES:
            raise TaskError(f"unknown side root {side!r}; known: {', '.join(SIDES)}")
        return self.base / side / self.id

    @property
    def c(self) -> Path:
        return self.root("contaminated")

    @property
    def d(self) -> Path:
        return self.root("drafts")

    @property
    def k(self) -> Path:
        return self.root("clean")

    def paths_hidden_from(self, side: str) -> list[Path]:
        """The side roots a role in `side` must never read."""
        return [self.root(s) for s in SIDES if s != side]


def create(base: str | None = None) -> Task:
    """Make a new task with its 3 side roots. `base` is the --base value."""
    if base is not None and fleet_mode():
        raise BaseRefused("refused --base: fleet mode always uses ~/.local/state/skill-intake/")
    base_path = resolve(base) if base is not None else default_base()
    check_base(base_path)
    manifest = {"task_id": new_id(), "created": now(), "phase": "created"}
    t = Task(base_path, manifest)
    for side in SIDES:
        root = t.root(side)
        root.parent.mkdir(parents=True, exist_ok=True)
        root.mkdir(mode=0o700, exist_ok=False)
        os.chmod(root, 0o700)
        for sub in SUBDIRS[side]:
            (root / sub).mkdir()
    write_json(t.c / MANIFEST, manifest)
    return t


def load(task_dir: str) -> Task:
    """Load a task from its contaminated side root (the `task_dir` acquire prints)."""
    c = resolve(task_dir)
    path = c / MANIFEST
    if not path.is_file():
        raise TaskError(f"not an intake task (no {MANIFEST}): {c}")
    manifest = read_json(path)
    if c.name != manifest.get("task_id") or c.parent.name != "contaminated":
        raise TaskError(f"not a contaminated side root of an intake task: {c}")
    return Task(c.parent.parent, manifest)


def set_phase(task: Task, phase: str) -> None:
    task.manifest["phase"] = phase
    write_json(task.c / MANIFEST, task.manifest)
