"""The `intake` entry point: runs under `python3 -I`, JSON on stdout, humans on stderr."""

from __future__ import annotations

import ast
import unittest

from base import INTAKE, SKILL, IntakeTestCase


class CliTest(IntakeTestCase):
    def test_no_command_prints_usage_on_stderr_and_exits_2(self):
        r = self.run_intake()
        self.assertEqual(r.returncode, 2, r.stderr)
        self.assertEqual(r.stdout, "")
        self.assertIn("usage:", r.stderr)

    def test_help_exits_0(self):
        r = self.run_intake("--help")
        self.assertEqual(r.returncode, 0, r.stderr)

    def test_unknown_command_exits_2(self):
        r = self.run_intake("no-such-command")
        self.assertEqual(r.returncode, 2)
        self.assertEqual(r.stdout, "")

    def test_running_writes_no_bytecode_into_the_skill(self):
        self.run_intake("--help")
        self.assertEqual(list(SKILL.rglob("__pycache__")), [])

    def test_commands_register_with_one_line_each(self):
        """COMMANDS is a flat tuple of module names: a new command adds 1 line."""
        tree = ast.parse(INTAKE.read_text(encoding="utf-8"))
        found = [
            n for n in tree.body
            if isinstance(n, ast.Assign) and any(getattr(t, "id", "") == "COMMANDS" for t in n.targets)
        ]
        self.assertEqual(len(found), 1)
        self.assertIsInstance(found[0].value, ast.Tuple)
        for elt in found[0].value.elts:
            self.assertIsInstance(elt, ast.Constant)
            self.assertIsInstance(elt.value, str)


if __name__ == "__main__":
    unittest.main()
