"""Shared test base for the intake suite.

Every test runs with HOME and XDG_STATE_HOME set to a fresh temp dir, so no
test reads or writes the real ~/.claude or the real state folder. The
SKILL_INTAKE_* seams are cleared unless a test sets them.
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

# Importing intakelib must never write __pycache__ into the skill folder:
# the build bundles every file under skills/.
sys.dont_write_bytecode = True

REPO = Path(__file__).resolve().parents[3]
SKILL = REPO / "skills" / "skill-intake"
SCRIPTS = SKILL / "scripts"
INTAKE = SCRIPTS / "intake.py"
TESTS = Path(__file__).resolve().parent

if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))


class IntakeTestCase(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory(prefix="intake-test-")
        self.addCleanup(self._tmp.cleanup)
        # resolve(): on macOS /var is a symlink to /private/var.
        self.tmp = Path(self._tmp.name).resolve()
        self.home = self.tmp / "home"
        self.state = self.tmp / "state"
        self.home.mkdir()
        self.state.mkdir()
        saved = dict(os.environ)
        self.addCleanup(self._restore_env, saved)
        # No test passes an API key to anything (repo rule): drop them all.
        for key in list(os.environ):
            if key.startswith("SKILL_INTAKE_") or key.endswith("_API_KEY"):
                del os.environ[key]
        os.environ["HOME"] = str(self.home)
        os.environ["XDG_STATE_HOME"] = str(self.state)
        cwd = os.getcwd()
        self.addCleanup(os.chdir, cwd)
        work = self.tmp / "work"
        work.mkdir()
        os.chdir(work)

    @staticmethod
    def _restore_env(saved: dict) -> None:
        os.environ.clear()
        os.environ.update(saved)

    def run_intake(self, *args: str, env: dict | None = None) -> subprocess.CompletedProcess:
        """Run the CLI as the skill does: `python3 -I`, plus -B for no bytecode."""
        full_env = dict(os.environ)
        full_env.update(env or {})
        return subprocess.run(
            [sys.executable, "-I", "-B", str(INTAKE), *args],
            capture_output=True,
            text=True,
            env=full_env,
            timeout=60,
        )
