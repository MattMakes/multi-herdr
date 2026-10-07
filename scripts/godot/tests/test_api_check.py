"""Tests for scripts/godot/api_check.py. Run: python3 -m unittest discover -s scripts/godot/tests"""

import contextlib
import io
import json
import shutil
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

import api_check  # noqa: E402

FIXTURE = HERE / "fixtures" / "api_check"
DOCTOOL = FIXTURE / "doctool"
CONTENT = FIXTURE / "content"
CLASSES = FIXTURE / "classes"


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

    def test_bare_class_names_must_exist(self):
        known = api_check.load_known_classes(CLASSES / "known_classes.txt")
        api = api_check.Api.load(DOCTOOL)
        found = [u.text() for u in api_check.check_file(api, CLASSES / "types.md", known)]
        path = CLASSES / "types.md"
        self.assertEqual(found, [
            f"{path}:8: unknown class NoSuchClass",
            f"{path}:9: unknown class Gone",
            f"{path}:9: unknown class MissingType",
            f"{path}:11: unknown class Phantom",
            f"{path}:12: unknown class Ghost",
            f"{path}:14: unknown class Lost",
            f"{path}:20: unknown class Missing",
        ])

    def test_bundle_classes_and_known_list(self):
        known = api_check.load_known_classes(CLASSES / "known_classes.txt")
        api = api_check.Api.load(DOCTOOL)
        path = CLASSES / "skill" / "SKILL.md"
        found = [u.text() for u in api_check.check_file(api, path, known)]
        # SkillThing: references/defs.md; KnownAddon: listed for "skill";
        # OtherAddon: listed for another skill only.
        self.assertEqual(found, [f"{path}:6: unknown class OtherAddon"])

    def test_deprecated_names_fail_in_own_text(self):
        code, out = run("--doctool", DOCTOOL, CLASSES / "deprecated.md")
        path = CLASSES / "deprecated.md"
        self.assertEqual(code, 1)
        self.assertEqual(out.splitlines(), [
            f"{path}:4: deprecated Node.old_make_things (Use [method add_child] instead.)",
            f"{path}:5: deprecated OldNode (Use [Node] instead.)",
            f"{path}:6: deprecated Node.old_make_things (Use [method add_child] instead.)",
        ])

    def test_deprecated_names_in_copied_are_counted_against_the_baseline(self):
        with tempfile.TemporaryDirectory() as tmp:
            baseline = Path(tmp) / "baseline.json"
            baseline.write_text('{"api_check": {"up": 3}}')
            args = ("--doctool", DOCTOOL, "--strict-own", CLASSES / "copied.json", "--baseline", baseline)
            code, out = run(*args, CLASSES / "up")
            self.assertEqual(code, 0, out)
            self.assertEqual(out.splitlines(), [
                "api_check: 3 copied deprecated names fail in 1 skills, report only (baseline 3, baseline.json)",
                "  per skill: up 3",
            ])
            # U-22: a count above the baseline fails; --update-baseline lowers it.
            baseline.write_text('{"api_check": {"up": 2}}')
            code, out = run(*args, CLASSES / "up")
            self.assertEqual(code, 1)
            self.assertIn("FAIL api_check: up has 3 copied deprecated names that fail, baseline 2.", out)
            code, _ = run(*args, "--update-baseline", CLASSES / "up")
            self.assertEqual(code, 0)
            self.assertEqual(json.loads(baseline.read_text()), {"api_check": {"up": 3}})

    def test_deprecated_json_sidecar(self):
        with tempfile.TemporaryDirectory() as tmp:
            dump = Path(tmp) / "dump"
            shutil.copytree(DOCTOOL, dump)
            (dump / "deprecated.json").write_text(json.dumps({"Node": {"add_child": "Gone."}}))
            api = api_check.Api.load(dump)
            self.assertEqual(api.deprecation("Node", "add_child"), "Gone.")
            self.assertEqual(api.deprecation("Node", "old_make_things"), "Use [method add_child] instead.")
            self.assertIsNone(api.deprecation("Node", "is_inside_tree"))

    def test_snake(self):
        self.assertEqual(api_check.snake("IsActionPressed"), "is_action_pressed")
        self.assertEqual(api_check.snake("GetUIDPath"), "get_uid_path")


if __name__ == "__main__":
    unittest.main()
