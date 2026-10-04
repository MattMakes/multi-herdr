"""Tests for scripts/godot/api_check.py. Run: python3 -m unittest discover -s scripts/godot/tests"""

import contextlib
import io
import sys
import unittest
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

import api_check  # noqa: E402

FIXTURE = HERE / "fixtures" / "api_check"
DOCTOOL = FIXTURE / "doctool"
CONTENT = FIXTURE / "content"


def run(*args):
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        code = api_check.main([str(a) for a in args])
    return code, out.getvalue()


class ApiCheckTest(unittest.TestCase):
    def test_known_names_pass(self):
        code, out = run("--doctool", DOCTOOL, CONTENT / "good.md")
        self.assertEqual(out, "")
        self.assertEqual(code, 0)

    def test_unknown_names_fail_with_file_and_line(self):
        code, out = run("--doctool", DOCTOOL, CONTENT / "bad.md")
        self.assertEqual(code, 1)
        path = CONTENT / "bad.md"
        self.assertEqual(out.splitlines(), [
            f"{path}:4: unknown OS.gc",
            f"{path}:5: unknown Node.no_method",
            f"{path}:11: unknown OS.Gc",
            f"{path}:12: unknown Key.NoSuchKey",
        ])

    def test_gd_files_are_checked_whole(self):
        code, out = run("--doctool", DOCTOOL, CONTENT / "script.gd")
        self.assertEqual(code, 1)
        self.assertEqual(out, f"{CONTENT / 'script.gd'}:4: unknown Vector2.ONE\n")

    def test_directory_is_searched(self):
        code, out = run("--doctool", DOCTOOL, CONTENT)
        self.assertEqual(code, 1)
        self.assertEqual(len(out.splitlines()), 5)

    def test_no_godot_skips(self):
        with mock.patch.object(api_check, "find_godot", return_value=None):
            code, out = run(CONTENT)
        self.assertEqual(code, 0)
        self.assertEqual(out, "skipped: no Godot\n")

    def test_snake(self):
        self.assertEqual(api_check.snake("IsActionPressed"), "is_action_pressed")
        self.assertEqual(api_check.snake("GetUIDPath"), "get_uid_path")


if __name__ == "__main__":
    unittest.main()
