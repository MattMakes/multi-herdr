"""T-14: no `intake` command executes a file from quarantine/.

Every intake command runs in this process with subprocess patched to record
the argv of each program start. Only git, nm, otool, objdump and the sandbox
wrappers (sandbox-exec, bwrap) may be started, and never with a quarantine
path as the program. For a sandbox wrapper, the program it starts is checked
too. The horch binary from SKILL_INTAKE_HORCH (intake mode, intake spawn) may
be started too, and never gets a quarantine path in any argument.
os.system, os.exec* and os.spawn* must never be called.

Later tickets add their own commands to COMMANDS_RUN and to this flow.
"""

from __future__ import annotations

import importlib.util
import io
import json
import os
import shutil
import subprocess
import sys
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from unittest import mock

from base import INTAKE, REPO, IntakeTestCase

import fixtures
from intakelib import acquire, roles, sandbox
from intakelib.task import read_json, write_json

ALLOWED = {"git", "nm", "otool", "objdump", "sandbox-exec", "bwrap"}
WRAPPERS = {"sandbox-exec", "bwrap"}
FLEET = {"HORCH_ROLE": "skill-importer-1", "HORCH_RECORD_ID": "rec-1", "HERDR_PANE_ID": "p0"}
HORCH_STUB = '#!/bin/sh\nif [ "$1" = spawn ]; then echo "spawned $2-1 (pane p1): none none" >&2; fi\nexit 0\n'


def load_cli():
    spec = importlib.util.spec_from_file_location("intake_cli_t14", INTAKE)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def inner_program(argv: list[str]) -> str | None:
    """The program a sandbox wrapper starts."""
    name = Path(argv[0]).name
    if name == "sandbox-exec":
        i = 1
        while i < len(argv) and argv[i] in ("-f", "-p", "-n", "-D"):
            i += 2
        return argv[i] if i < len(argv) else None
    if name == "bwrap" and "--" in argv:
        rest = argv[argv.index("--") + 1:]
        return rest[0] if rest else None
    return None


class ArgvTest(IntakeTestCase):
    def setUp(self):
        super().setUp()
        self.cli = load_cli()
        self.calls: list[list[str]] = []
        self.ran: set[str] = set()
        gh = self.tmp / "gh"
        fixtures.git_repo(gh / "o" / "r.git", {**fixtures.f_binary(), "LICENSE": fixtures.LICENCE_TEXTS["mit"]})
        os.environ[acquire.BASE_ENV] = f"file://{gh}"
        self.base = str(self.tmp / "base")

    def intake(self, *argv: str, stdin: str = "") -> tuple[int, str]:
        """Run 1 intake command in this process with every program start recorded."""
        calls = self.calls
        real = subprocess.Popen

        class Recording(real):
            def __init__(self, args, *a, **kw):
                calls.append([str(x) for x in (args if isinstance(args, (list, tuple)) else [args])])
                super().__init__(args, *a, **kw)

        def refused(*a, **kw):
            raise AssertionError(f"a forbidden process call: {a!r}")

        patches = [mock.patch.object(subprocess, "Popen", Recording), mock.patch.object(os, "system", refused)]
        for name in dir(os):
            if name.startswith(("exec", "spawn", "posix_spawn")) and callable(getattr(os, name)):
                patches.append(mock.patch.object(os, name, refused))
        buf = io.StringIO()
        for p in patches:
            p.start()
        try:
            with redirect_stdout(buf), redirect_stderr(io.StringIO()), \
                    mock.patch.object(sys, "stdin", io.StringIO(stdin)):
                code = self.cli.main(list(argv))
        finally:
            for p in reversed(patches):
                p.stop()
        self.ran.add(argv[0] if argv[0] != "cr" else f"cr {argv[1]}")
        return code, buf.getvalue()

    def task_dir(self, out: str) -> str:
        return next(json.loads(ln)["task_dir"] for ln in out.splitlines() if '"task_dir"' in ln)

    def test_t14_no_command_runs_a_quarantine_file(self):
        code, out = self.intake("run", "o/r", "--base", self.base)
        self.assertEqual(code, 4, out)  # OPQ hits have no auditor disposition yet
        c = Path(self.task_dir(out))
        code, out = self.intake("acquire", "o/r", "--base", self.base)
        self.assertEqual(code, 0, out)
        c2 = self.task_dir(out)
        self.assertEqual(self.intake("discover", "--task", c2)[0], 0)
        self.assertEqual(self.intake("scan", "--task", c2)[0], 0)

        (c / "sweep" / "audit.jsonl").write_text(
            json.dumps({"kind": "purpose", "skill": "bin-fixture", "purpose": "Formats notes."}) + "\n")
        for ln in (c / "sweep" / "scan.jsonl").read_text().splitlines():
            hit = json.loads(ln)
            if hit["rule"].startswith("OPQ-"):
                (c / "sweep" / "opaque" / f"{hit['id']}.md").write_text("Disposition: remove\n")
        self.assertEqual(self.intake("triage", "--task", str(c))[0], 0)
        self.assertEqual(self.intake("report", "--task", str(c))[0], 0)

        self.assertEqual(self.intake("cr", "init", "--task", str(c))[0], 0)
        self.assertEqual(self.intake("cr", "index", "--task", str(c))[0], 0)
        base = Path(self.base)
        d, k = base / "drafts" / c.name, base / "clean" / c.name
        findings = read_json(c / "sweep" / "findings.json")["findings"]
        items = [{"id": f"R-{i}", "finding": f["id"], "action": "remove", "behavior": "The feature is gone."}
                 for i, f in enumerate((f for f in findings if f["rule"].startswith("OPQ-")), 1)]
        draft = d / "drafts" / "spec-notes.json"
        write_json(draft, {
            "spec_id": "spec-notes", "title": "Lay out notes", "summary": "Tidies the spacing of a notes file.",
            "skill": {"name": "bin-fixture", "purpose": "Tidies notes.", "triggers": ["a tidy request"],
                      "outputs": ["the tidied file"]},
            "replace_or_remove": items,
            "behaviors": [{"id": "b1", "given": "a notes file", "when": "tidied", "then": "spacing is even"}],
            "test_scenarios": [{"id": "t1", "description": "one file", "expected": "even spacing"}],
            "review": {"reviewer": "sanitizer", "status": "approved"}})
        self.assertEqual(self.intake("cr", "scan", "--task", str(c), str(draft))[0], 0)
        self.assertEqual(self.intake("cr", "handoff", "--task", str(c), str(draft))[0], 0)
        self.assertEqual(self.intake("cr", "context", "--task", str(c), "--target", "standalone")[0], 0)
        fixtures.build(k / "out", {"bin-fixture/SKILL.md": "---\nname: bin-fixture\ndescription: Makes gaps in"
                                   " written notes even.\n---\n# Spacing\n\nEdit the spacing by hand.\n"})
        code, _ = self.intake("cr", "verify", "--task", str(c), "--", "python3", "-B", "-c", "print(1)")
        self.assertIn(code, (0, 13))
        write_json(k / "review.json", {
            "role": "reviewer", "status": "complete", "spec_ids": ["spec-notes"], "changed_files": [],
            "verification": [], "verdicts": [{"spec_id": "spec-notes", "item": i, "result": "pass"}
                                             for i in ["b1"] + [x["id"] for x in items]]})
        self.assertEqual(self.intake("cr", "status", "--task", str(c))[0], 0)
        self.assertEqual(self.intake("cr", "validate", "--task", str(c), str(k / "review.json"))[0], 0)
        self.assertEqual(self.intake("cr", "role", "set", "writer", "--task", str(c))[0], 0)
        self.assertIn(self.intake("guard", stdin=json.dumps(
            {"tool_name": "Read", "tool_input": {"file_path": str(c / "quarantine" / "src" / "x")}}))[0], (0, 2))
        self.assertEqual(self.intake("cr", "role", "clear")[0], 0)
        self.assertEqual(self.intake("hook-config")[0], 0)
        code, out = self.intake("verify", "--task", str(c), "--target", "standalone")
        self.assertEqual(code, 0, out)
        skills = self.tmp / "skills"
        code, out = self.intake("install", "--task", str(c), "--target", "standalone", "--skills-dir", str(skills))
        self.assertEqual(code, 0, out)
        self.assertTrue((skills / "bin-fixture" / "SKILL.md").is_file())

        # The repo target: candidates, then install without and with a mapping (ticket 11).
        repo = fixtures.f_repo(self.tmp / "checkout", REPO)
        code, out = self.intake("candidates", "--task", str(c), "--repo", str(repo))
        self.assertEqual(code, 0, out)
        self.assertEqual(self.intake("cr", "context", "--task", str(c), "--target", "repo")[0], 0)
        code, out = self.intake("verify", "--task", str(c), "--target", "repo")
        self.assertEqual(code, 0, out)
        code, out = self.intake("install", "--task", str(c), "--target", "repo", "--repo", str(repo))
        self.assertEqual(code, 0, out)
        mapping = self.tmp / "mapping.json"
        write_json(mapping, {"option": 3, "existing": ["sonnet"]})
        code, out = self.intake("install", "--task", str(c), "--repo", str(repo), "--mapping", str(mapping))
        self.assertEqual(code, 0, out)
        self.assertTrue((repo / "skills" / "bin-fixture" / "SKILL.md").is_file())

        # Role orchestration: fleet mode by the real rule, with a stub horch.
        horch = self.tmp / "bin" / "horch"
        horch.parent.mkdir()
        horch.write_text(HORCH_STUB)
        horch.chmod(0o755)
        os.environ.update(FLEET, SKILL_INTAKE_HORCH=str(horch))
        code, out = self.intake("mode")
        self.assertEqual((code, out.strip()), (0, "fleet"))
        self.assertEqual(self.intake("brief", "writer", "--task", str(c))[0], 0)
        code, out = self.intake("spawn", "auditor", "--task", str(c))
        self.assertEqual(code, 0, out)
        status = Path(json.loads(out)["status_file"])
        write_json(status, {"role": "auditor", "status": "ok", "outputs": [], "error": None})
        self.assertEqual(self.intake("role-result", str(status))[0], 0)
        self.assertEqual(self.intake("roles", "--task", str(c))[0], 0)
        for key in FLEET:
            del os.environ[key]
        code, out = self.intake("brief", "reviewer", "--task", str(c))
        role_dir = self.tmp / "roles"
        fixtures.build(role_dir, {"reviewer.md": "# Reviewer\n"})
        with mock.patch.object(roles, "ROLES_DIR", role_dir):
            self.assertEqual(self.intake("subagent-prompt", "reviewer", "--brief", json.loads(out)["brief"])[0], 0)

        # Every command that exists at this point ran.
        commands = set(self.cli.COMMANDS) - {"cr"}
        cr_actions = {"init", "index", "role", "scan", "handoff", "context", "verify", "status", "validate"}
        self.assertEqual(self.ran, commands | {f"cr {a}" for a in cr_actions})

        quarantine = [str((base / "contaminated" / x / "quarantine").resolve()) for x in (c.name, Path(c2).name)]
        self.assertTrue(self.calls)
        self.assertEqual(violations(self.calls, quarantine, str(horch)), [])
        programs = {Path(a[0]).name for a in self.calls}
        self.assertIn("git", programs)
        self.assertIn(["--version"], [a[1:] for a in self.calls if a[0] == str(horch)])
        self.assertIn("spawn", [a[1] for a in self.calls if a[0] == str(horch)])
        readers = {Path(inner_program(a)).name for a in self.calls if Path(a[0]).name in WRAPPERS}
        if Path(sandbox.SANDBOX_EXEC).exists() or shutil.which("bwrap"):
            self.assertTrue(readers & {"nm", "otool", "objdump"}, self.calls)

    def test_checker_catches_a_quarantine_program(self):
        q = str(self.tmp / "q")
        self.assertTrue(violations([[f"{q}/bin/fmt"]], [q]))
        self.assertTrue(violations([["/usr/bin/sandbox-exec", "-f", "p.sb", f"{q}/bin/fmt"]], [q]))
        self.assertTrue(violations([["/usr/bin/bwrap", "--ro-bind", "/", "/", "--", f"{q}/x"]], [q]))
        self.assertTrue(violations([["/usr/bin/python3", "x.py"]], [q]))
        self.assertEqual(violations([["/usr/bin/sandbox-exec", "-f", "p.sb", "/usr/bin/nm", "-gU", f"{q}/a"]], [q]), [])
        h = str(self.tmp / "bin" / "horch")
        self.assertEqual(violations([[h, "spawn", "intake-writer", "--exact", "Read and follow /k/b.md exactly."]], [q], h), [])
        self.assertTrue(violations([[h, "spawn", "x", "--exact", f"Read and follow {q}/src/x exactly."]], [q], h))
        self.assertTrue(violations([["/usr/local/bin/horch", "--version"]], [q], h))


def violations(calls: list[list[str]], quarantine: list[str], horch: str | None = None) -> list[str]:
    """Each program start that is not allowed, or whose program is a quarantine path.
    `horch` (the SKILL_INTAKE_HORCH path) is allowed; no argument of it may hold a quarantine path."""
    bad = []
    for argv in calls:
        if horch is not None and argv[0] == horch:
            if any(q in arg for arg in argv for q in quarantine):
                bad.append(f"quarantine path passed to horch: {argv}")
            continue
        prog = Path(argv[0]).name
        if prog not in ALLOWED:
            bad.append(f"not allowed: {argv}")
            continue
        for p in [argv[0]] + ([inner_program(argv)] if prog in WRAPPERS else []):
            if p is None or any(os.path.realpath(p).startswith(q) for q in quarantine):
                bad.append(f"quarantine program: {argv}")
    return bad
