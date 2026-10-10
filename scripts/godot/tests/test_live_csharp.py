"""Tests for scripts/godot/live_csharp.sh (U-24). Run: python3 -m unittest discover -s scripts/godot/tests

The gate never runs the live check itself; these tests run only its quick
exits: SKIP without Godot .NET, FAIL on a Godot build without .NET.
"""

import os
import subprocess
import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

from api_check import find_godot  # noqa: E402

SCRIPT = HERE.parent / "live_csharp.sh"


def run(godot: str) -> subprocess.CompletedProcess:
    env = dict(os.environ, GODOT_MONO_PATH=godot)
    return subprocess.run(["bash", str(SCRIPT)], env=env, capture_output=True, text=True, timeout=60)


class LiveCsharpTest(unittest.TestCase):
    def test_no_godot_dotnet_skips(self):
        r = run("/nonexistent/Godot")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertTrue(r.stdout.startswith("SKIP godot-dotnet: no Godot .NET at '/nonexistent/Godot'"), r.stdout)

    @unittest.skipIf(find_godot() is None, "no Godot")
    def test_a_build_without_dotnet_fails(self):
        version = subprocess.run([find_godot(), "--headless", "--version"], capture_output=True, text=True).stdout
        if "mono" in version:
            self.skipTest("the Godot found is a .NET build")
        r = run(find_godot())
        self.assertEqual(r.returncode, 1)
        self.assertIn("FAIL godot-dotnet: want a 4.7 mono build", r.stdout)


if __name__ == "__main__":
    unittest.main()
