"""Tests for skills/godot-build-verify/scripts/godot-check.sh (GRS-09). Run: python3 -m unittest discover -s scripts/godot/tests -p 'test_godot_check.py'

A fake engine stands in for Godot: it prints the lines in FAKE_LINES and exits with FAKE_EXIT.
"""

import os
import stat
import subprocess
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
SCRIPT = HERE.parents[2] / "skills" / "godot-build-verify" / "scripts" / "godot-check.sh"
RED = "\x1b[31m"
RESET = "\x1b[0m"

FAKE = """#!/bin/bash
for a in "$@"; do
  if [ "$a" = --version ]; then echo 4.7.2.stable.fake; exit 0; fi
done
echo "ARGS $*"
if [ -n "${FAKE_LINES:-}" ]; then printf '%b\\n' "$FAKE_LINES"; fi
exit "${FAKE_EXIT:-0}"
"""


class GodotCheckTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        root = Path(self.tmp.name).resolve()
        self.engine = root / "fake-godot"
        self.engine.write_text(FAKE)
        self.engine.chmod(self.engine.stat().st_mode | stat.S_IXUSR)
        subprocess.run([str(self.engine), "warm", "--version"], capture_output=True, check=False)
        self.project = root / "project"
        self.project.mkdir()
        (self.project / "project.godot").write_text("config_version=5\n")

    def tearDown(self):
        self.tmp.cleanup()

    def run_check(self, *args, lines="", code=0):
        env = {k: v for k, v in os.environ.items() if k not in ("CODEX_SANDBOX", "GODOT_MONO_PATH")}
        env.update(GODOT_PATH=str(self.engine), FAKE_LINES=lines, FAKE_EXIT=str(code))
        return subprocess.run(["bash", str(SCRIPT), *args], cwd=self.project, env=env,
                              capture_output=True, text=True, timeout=60, check=False)

    def test_script_is_executable(self):
        self.assertTrue(os.access(SCRIPT, os.X_OK), SCRIPT)

    def test_clean_run_exits_0(self):
        r = self.run_check()
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertIn("--import", r.stdout)

    def test_no_project_exits_2(self):
        (self.project / "project.godot").unlink()
        r = self.run_check()
        self.assertEqual(r.returncode, 2, r.stdout + r.stderr)

    def test_godot_check_catches_script_error_on_zero_exit(self):
        (self.project / "a.gd").write_text("extends Node\n")
        for line in ("SCRIPT ERROR: Parse Error: x", "Parse Error: bad", "ERROR: Failed to load script", "Nothing was run"):
            r = self.run_check(lines=f"{RED}{line}{RESET}", code=0)
            self.assertEqual(r.returncode, 3, line + r.stdout + r.stderr)

    def test_godot_check_strips_ansi_gut_errors(self):
        (self.project / ".gutconfig.json").write_text("{}\n")
        r = self.run_check(lines=f"{RED}No{RESET} tests\n\x1b[1;31mNothing\x1b[0m was run", code=0)
        self.assertEqual(r.returncode, 3, r.stdout + r.stderr)
        r = self.run_check(lines="\x1b[33mNothing was run\x1b[0m", code=0)
        self.assertEqual(r.returncode, 3, r.stdout + r.stderr)

    def test_pattern_split_by_ansi_is_found(self):
        r = self.run_check(lines=f"SCRIPT{RED} ERROR{RESET}: x")
        self.assertEqual(r.returncode, 3, r.stdout + r.stderr)

    def test_non_zero_engine_exit_passes_through(self):
        r = self.run_check(code=1)
        self.assertEqual(r.returncode, 1, r.stdout + r.stderr)

    def test_parse_check_covers_scripts_outside_godot_and_addons(self):
        (self.project / "a.gd").write_text("extends Node\n")
        (self.project / "addons" / "x").mkdir(parents=True)
        (self.project / "addons" / "x" / "skip.gd").write_text("")
        (self.project / ".godot").mkdir(exist_ok=True)
        (self.project / ".godot" / "skip2.gd").write_text("")
        r = self.run_check()
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertIn("--check-only --script res://a.gd", r.stdout)
        self.assertNotIn("skip", r.stdout)

    def test_gut_runs_when_configured_or_flagged(self):
        r = self.run_check()
        self.assertNotIn("gut_cmdln", r.stdout)
        r = self.run_check("--gut")
        self.assertIn("-s res://addons/gut/gut_cmdln.gd -gconfig=res://.gutconfig.json", r.stdout)
        (self.project / ".gutconfig.json").write_text("{}\n")
        r = self.run_check()
        self.assertIn("gut_cmdln", r.stdout)


if __name__ == "__main__":
    unittest.main()
