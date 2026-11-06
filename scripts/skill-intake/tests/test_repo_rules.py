"""Repo rules for the skill, its tests and its fixtures (spec 12.2, 12.4)."""

from __future__ import annotations

import unittest
from pathlib import Path

from base import SKILL, TESTS, IntakeTestCase

import fixtures

# Built from 2 parts so that this file does not hold the full literal.
KEY_NAME = "ANTHROPIC" + "_API_KEY"


def files_under(*roots: Path) -> list[Path]:
    out = []
    for root in roots:
        for p in sorted(root.rglob("*")):
            if p.is_file() and "__pycache__" not in p.parts:
                out.append(p)
    return out


class RepoRulesTest(IntakeTestCase):
    def test_no_file_holds_the_full_key_name(self):
        for p in files_under(SKILL, TESTS):
            self.assertNotIn(KEY_NAME.encode(), p.read_bytes(), str(p))

    def test_files_are_plain_ascii_text(self):
        """No raw payload, invisible character or binary: printable ASCII, \\n, \\t only."""
        for p in files_under(SKILL, TESTS):
            data = p.read_bytes()
            bad = [b for b in data if not (0x20 <= b <= 0x7E or b in (0x0A, 0x09))]
            self.assertEqual(bad, [], str(p))

    def test_skill_holds_no_bytecode(self):
        self.assertEqual([p for p in SKILL.rglob("*") if p.suffix == ".pyc" or p.name == "__pycache__"], [])


class FixtureBuilderTest(IntakeTestCase):
    def test_build_writes_text_and_bytes(self):
        root = fixtures.build(self.tmp / "f", {"SKILL.md": "a\u200bb", "bin/x": b"\x7fELF"})
        self.assertEqual((root / "SKILL.md").read_text(encoding="utf-8"), "a\u200bb")
        self.assertEqual((root / "bin" / "x").read_bytes(), b"\x7fELF")

    def test_build_refuses_a_real_host(self):
        with self.assertRaises(ValueError):
            fixtures.build(self.tmp / "f", {"s.py": "u = 'https://example.com/x'"})
        self.assertFalse((self.tmp / "f").exists())

    def test_build_accepts_invalid_hosts(self):
        url = f"https://{fixtures.host('exfil')}/up and git://user@{fixtures.host('a.b')}:9/x"
        fixtures.build(self.tmp / "f", {"s.py": url})

    def test_build_refuses_escaping_paths(self):
        for rel in ("../x", "/abs"):
            with self.assertRaises(ValueError, msg=rel):
                fixtures.build(self.tmp / "f", {rel: "x"})

    def test_names(self):
        self.assertEqual(fixtures.host("exfil"), "exfil.invalid")
        self.assertEqual(fixtures.secret("TOKEN"), "INTAKE_FIXTURE_TOKEN")
        self.assertEqual(fixtures.secret("INTAKE_FIXTURE_X"), "INTAKE_FIXTURE_X")


if __name__ == "__main__":
    unittest.main()
