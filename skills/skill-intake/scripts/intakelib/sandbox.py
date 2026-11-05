"""Run clean verification commands (the rewritten scripts' tests) offline in an OS sandbox.

Ported from clean-room crlib/sandbox.py and changed for skill-intake:

- macOS uses sandbox-exec; Linux uses bwrap (bubblewrap);
- with no usable sandbox, or with SKILL_INTAKE_NO_SANDBOX=1, nothing runs and
  the run is logged as `unverified`;
- cwd is K/out/; writes are allowed only there and in a throwaway temp dir;
  reads of every task's C and D roots and of credential folders are denied;
- each run is appended to K/verify-log/log.json.

No container images, so nothing is pulled from any registry.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from .task import SIDES, Task, TaskError, is_within, now, read_json, resolve, write_json

SANDBOX_EXEC = "/usr/bin/sandbox-exec"
CREDENTIAL_DIRS = [".ssh", ".aws", ".gnupg", ".config/gh", ".docker", ".kube", ".netrc",
                   ".npmrc", ".pypirc", ".claude", ".codex", "Library/Keychains"]
PASS_ENV = ("PATH", "LANG", "LC_ALL", "TERM")
OUTPUT_TAIL = 4000
UNVERIFIED = 13  # exit code: no sandbox, nothing ran
PROBE_TIMEOUT = 10


def _q(path: str) -> str:
    return '"' + path.replace("\\", "\\\\").replace('"', '\\"') + '"'


def profile(deny_read: list[str], allow_write: list[str]) -> str:
    reads = "\n".join(f"(deny file-read* (subpath {_q(p)}))" for p in deny_read)
    writes = " ".join(f"(subpath {_q(p)})" for p in allow_write)
    return (
        "(version 1)\n(allow default)\n(deny network*)\n"
        f"{reads}\n"
        f"(deny file-write* (require-not (require-any {writes} (subpath \"/dev\"))))\n"
    )


def _works(argv: list[str]) -> bool:
    try:
        return subprocess.run(argv, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                              timeout=PROBE_TIMEOUT).returncode == 0
    except (OSError, subprocess.TimeoutExpired):
        return False


def kind() -> str | None:
    """The usable sandbox on this host: "sandbox-exec", "bwrap", or None."""
    if os.environ.get("SKILL_INTAKE_NO_SANDBOX") == "1":
        return None
    if sys.platform == "darwin" and Path(SANDBOX_EXEC).exists():
        if _works([SANDBOX_EXEC, "-p", "(version 1)(allow default)", "/usr/bin/true"]):
            return "sandbox-exec"
        return None
    if sys.platform.startswith("linux"):
        bwrap = shutil.which("bwrap")
        if bwrap and _works([bwrap, "--ro-bind", "/", "/", "--dev", "/dev", "--unshare-net", "true"]):
            return "bwrap"
    return None


def _hidden(task: Task) -> list[str]:
    """Paths no verify run may read: every task's C and D, and credential folders."""
    home = Path.home()
    paths = [task.base / s for s in SIDES if s != "clean"]
    paths += [home / d for d in CREDENTIAL_DIRS]
    return [os.path.realpath(p) for p in paths if p.exists()]


def _wrap(sandbox: str, argv: list[str], out: str, tmp: str, hidden: list[str]) -> list[str]:
    if sandbox == "sandbox-exec":
        prof = Path(tmp) / "profile.sb"
        prof.write_text(profile(hidden, [out, tmp]))
        return [SANDBOX_EXEC, "-f", str(prof), *argv]
    cmd = [shutil.which("bwrap") or "bwrap", "--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc",
           "--unshare-net", "--unshare-pid", "--die-with-parent"]
    for p in hidden:
        cmd += ["--tmpfs", p] if os.path.isdir(p) else ["--ro-bind", "/dev/null", p]
    cmd += ["--bind", out, out, "--bind", tmp, tmp, "--chdir", out, "--"]
    return cmd + argv


def _check_argv(task: Task, argv: list[str]) -> None:
    """A verify run never names a file of C or D (no script from quarantine ever runs)."""
    for arg in argv:
        if "/" not in arg:
            continue
        p = resolve(arg) if os.path.isabs(os.path.expanduser(arg)) else resolve(task.k / "out" / arg)
        if any(is_within(p, task.base / s) for s in SIDES if s != "clean"):
            raise TaskError(f"refused: verify argument points into a contaminated or drafts root: {arg}")


def _log(task: Task, entry: dict) -> None:
    path = task.k / "verify-log" / "log.json"
    log = read_json(path) if path.is_file() else {"runs": []}
    log["runs"].append(entry)
    write_json(path, log)


def run(task: Task, argv: list[str], timeout: int = 600) -> tuple[int, dict]:
    """Run argv in the sandbox with cwd K/out/. Return (exit code, log entry)."""
    if not argv:
        raise ValueError("no command given")
    _check_argv(task, argv)
    out = os.path.realpath(task.k / "out")
    started = now()
    sandbox = kind()
    if sandbox is None:
        entry = {"argv": argv, "started": started, "sandbox": None, "status": "unverified",
                 "exit_code": None, "output_tail": ""}
        _log(task, entry)
        return UNVERIFIED, entry

    tmp = os.path.realpath(tempfile.mkdtemp(prefix="intake-verify-"))
    try:
        env = {k: os.environ[k] for k in PASS_ENV if k in os.environ}
        env.update(HOME=tmp, TMPDIR=tmp, XDG_CACHE_HOME=f"{tmp}/cache", PIP_NO_INDEX="1",
                   npm_config_offline="true", PYTHONDONTWRITEBYTECODE="1")
        try:
            proc = subprocess.run(_wrap(sandbox, argv, out, tmp, _hidden(task)), cwd=out, env=env,
                                  stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
                                  timeout=timeout, errors="replace")
            code, output = proc.returncode, proc.stdout
        except subprocess.TimeoutExpired as exc:
            partial = exc.stdout or ""
            if isinstance(partial, bytes):
                partial = partial.decode("utf-8", "replace")
            code, output = 124, f"timed out after {timeout}s\n{partial}"
        except FileNotFoundError as exc:
            code, output = 127, str(exc)
    finally:
        shutil.rmtree(tmp, ignore_errors=True)

    entry = {"argv": argv, "started": started, "sandbox": sandbox,
             "status": "pass" if code == 0 else "fail", "exit_code": code,
             "output_tail": output[-OUTPUT_TAIL:]}
    _log(task, entry)
    return code, entry
