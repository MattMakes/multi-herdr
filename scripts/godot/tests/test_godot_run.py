"""Tests for skills/godot-build-verify/scripts/godot-run.sh (GDW-10, GDW-12). Run: python3 -m unittest discover -s scripts/godot/tests

A fake engine stands in for Godot: it prints its environment and arguments,
prints the lines in FAKE_LINES, and exits with FAKE_EXIT. No real Godot runs.
"""

import os
import platform
import stat
import subprocess
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
SCRIPT = HERE.parents[2] / "skills" / "godot-build-verify" / "scripts" / "godot-run.sh"
E3 = 'ERROR: Condition "ret != noErr" is true. Returning: ""'
DARWIN = platform.system() == "Darwin"
SETTINGS = Path(".godot/horch-home/Library/Application Support/Godot/editor_settings-4.7.tres")
KEY = 'network/tls/editor_tls_certificates = "/etc/ssl/cert.pem"'

FAKE = """#!/bin/bash
for a in "$@"; do
  if [ "$a" = --version ]; then echo 4.7.2.stable.fake; exit 0; fi
done
echo "ARGS $*"
echo "ENV HOME=$HOME"
echo "ENV XDG_DATA_HOME=$XDG_DATA_HOME"
echo "ENV XDG_CONFIG_HOME=$XDG_CONFIG_HOME"
echo "ENV XDG_CACHE_HOME=$XDG_CACHE_HOME"
if [ -n "${FAKE_LINES:-}" ]; then printf '%s\\n' "$FAKE_LINES"; fi
exit "${FAKE_EXIT:-0}"
"""


class GodotRunTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        root = Path(self.tmp.name).resolve()
        self.engine = root / "fake-godot"
        self.engine.write_text(FAKE)
        self.engine.chmod(self.engine.stat().st_mode | stat.S_IXUSR)
        self.project = root / "project"
        self.project.mkdir()
        (self.project / "project.godot").write_text("config_version=5\n")

    def tearDown(self):
        self.tmp.cleanup()

    def run_script(self, *args, lines="", code=0, sandbox=None, cwd=None, mono=None):
        env = {k: v for k, v in os.environ.items()
               if k not in ("CODEX_SANDBOX", "GODOT_MONO_PATH")}
        env.update(GODOT_PATH=str(self.engine), FAKE_LINES=lines, FAKE_EXIT=str(code))
        if sandbox:
            env["CODEX_SANDBOX"] = sandbox
        if mono is not None:
            env["GODOT_MONO_PATH"] = mono
        return subprocess.run(["bash", str(SCRIPT), *args], cwd=cwd or self.project, env=env,
                              capture_output=True, text=True, timeout=60)

    def test_script_is_executable(self):
        self.assertTrue(os.access(SCRIPT, os.X_OK), SCRIPT)

    def test_no_project_exits_2(self):
        r = self.run_script("--import", cwd=self.project.parent)
        self.assertEqual(r.returncode, 2, r.stdout + r.stderr)
        self.assertIn("project.godot", r.stdout + r.stderr)

    def test_user_dirs_are_private(self):
        r = self.run_script("--import")
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        home = self.project / ".godot" / "horch-home"
        self.assertIn(f"ENV HOME={home}\n", r.stdout)
        for var in ("XDG_DATA_HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME"):
            line = next(l for l in r.stdout.splitlines() if l.startswith(f"ENV {var}="))
            self.assertTrue(line.split("=", 1)[1].startswith(f"{home}/"), line)
            self.assertTrue(Path(line.split("=", 1)[1]).is_dir(), line)

    def test_headless_is_added_once(self):
        r = self.run_script("--path", ".", "--import")
        self.assertIn("ARGS --headless --path . --import\n", r.stdout)
        r = self.run_script("--path", ".", "--headless", "--import")
        self.assertIn("ARGS --path . --headless --import\n", r.stdout)

    def test_exit_code_passes_through(self):
        self.assertEqual(self.run_script("--import", code=0).returncode, 0)
        self.assertEqual(self.run_script("--import", code=1).returncode, 1)

    def test_output_is_logged(self):
        r = self.run_script("--import", lines="MARKER_LINE")
        logs = list((self.project / ".godot" / "horch-home" / "logs").glob("godot-run-*.log"))
        self.assertEqual(len(logs), 1, logs)
        self.assertIn("MARKER_LINE", logs[0].read_text())
        self.assertIn("MARKER_LINE", r.stdout)

    def test_e3_line_exits_3(self):
        r = self.run_script("--import", lines=E3, code=0)
        self.assertEqual(r.returncode, 3, r.stdout + r.stderr)
        self.assertIn("godot-run: SANDBOX: 1 line(s); log: ", r.stdout + r.stderr)

    def test_error_path_outside_project_exits_3(self):
        line = "ERROR: Could not create directory: '/Users/x/Library/Application Support/Godot/app_userdata/P'."
        r = self.run_script("--import", lines=line)
        self.assertEqual(r.returncode, 3, r.stdout + r.stderr)

    def test_error_path_inside_project_is_not_sandbox(self):
        line = f"ERROR: Could not create directory: '{self.project}/.godot/horch-home/x'."
        r = self.run_script("--import", lines=line)
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertNotIn("SANDBOX", r.stdout + r.stderr)

    def test_without_sandbox_writes_no_tls_files(self):
        r = self.run_script("--import")
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertFalse((self.project / "override.cfg").exists())
        self.assertFalse((self.project / SETTINGS).exists())

    @unittest.skipUnless(DARWIN, "macOS only")
    def test_sandbox_writes_override_and_editor_setting(self):
        r = self.run_script("--import", sandbox="seatbelt")
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        cfg = (self.project / "override.cfg").read_text()
        self.assertTrue(cfg.startswith("; horch godot-run.sh: Codex sandbox TLS override."), cfg)
        self.assertIn('[network]\n\ntls/certificate_bundle_override="/etc/ssl/cert.pem"\n', cfg)
        self.assertIn(KEY, (self.project / SETTINGS).read_text())

    @unittest.skipUnless(DARWIN, "macOS only")
    def test_sandbox_keeps_other_editor_settings(self):
        settings = self.project / SETTINGS
        settings.parent.mkdir(parents=True)
        settings.write_text('[gd_resource type="EditorSettings" format=3]\n\n[resource]\n'
                            'a/b = 1\nnetwork/tls/editor_tls_certificates = ""\nc/d = 2\n')
        r = self.run_script("--import", sandbox="seatbelt")
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        text = settings.read_text()
        self.assertIn(f"a/b = 1\n{KEY}\nc/d = 2\n", text)
        self.assertEqual(text.count("editor_tls_certificates"), 1, text)

    @unittest.skipUnless(DARWIN, "macOS only")
    def test_sandbox_keeps_override_with_key(self):
        cfg = self.project / "override.cfg"
        cfg.write_text('[network]\ntls/certificate_bundle_override="/x.pem"\n')
        r = self.run_script("--import", sandbox="seatbelt")
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertEqual(cfg.read_text(), '[network]\ntls/certificate_bundle_override="/x.pem"\n')

    @unittest.skipUnless(DARWIN, "macOS only")
    def test_sandbox_refuses_override_without_key(self):
        cfg = self.project / "override.cfg"
        cfg.write_text('[display]\nwindow/size/viewport_width=320\n')
        r = self.run_script("--import", sandbox="seatbelt")
        self.assertEqual(r.returncode, 2, r.stdout + r.stderr)
        self.assertIn("tls/certificate_bundle_override", r.stdout + r.stderr)
        self.assertEqual(cfg.read_text(), '[display]\nwindow/size/viewport_width=320\n')
        self.assertNotIn("ARGS", r.stdout)

    def fake_mono(self):
        mono = self.engine.parent / "fake-godot-mono"
        mono.write_text(FAKE.replace("4.7.2.stable.fake", "4.7.2.stable.mono.fake")
                        .replace('echo "ARGS $*"', 'echo "MONO ARGS $*"'))
        mono.chmod(mono.stat().st_mode | stat.S_IXUSR)
        return mono

    def test_gdscript_project_uses_the_standard_engine(self):
        r = self.run_script("--import", mono=str(self.fake_mono()))
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertIn("ARGS --headless --import\n", r.stdout)
        self.assertNotIn("MONO", r.stdout)

    def test_csharp_feature_uses_the_mono_engine(self):
        (self.project / "project.godot").write_text(
            'config_version=5\n[application]\nconfig/features=PackedStringArray("4.7", "C#", "Forward Plus")\n')
        r = self.run_script("--import", mono=str(self.fake_mono()))
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertIn("MONO ARGS --headless --import\n", r.stdout)

    def test_csproj_uses_the_mono_engine(self):
        (self.project / "Game.csproj").write_text("<Project Sdk=\"Godot.NET.Sdk/4.7.2\" />\n")
        r = self.run_script("--version", mono=str(self.fake_mono()))
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertIn("mono", r.stdout)

    @unittest.skipIf(Path("/Applications/Godot_mono.app/Contents/MacOS/Godot").exists(),
                     "the host has the Godot .NET app")
    def test_csharp_project_without_mono_engine_exits_2(self):
        (self.project / "Game.csproj").write_text("<Project />\n")
        r = self.run_script("--import", mono="")
        self.assertEqual(r.returncode, 2, r.stdout + r.stderr)
        self.assertIn("GODOT_MONO_PATH", r.stderr)
        self.assertIn("/Applications/Godot_mono.app/Contents/MacOS/Godot", r.stderr)
        self.assertNotIn("ARGS", r.stdout)

    def test_csharp_project_with_bad_mono_path_exits_2(self):
        (self.project / "Game.csproj").write_text("<Project />\n")
        r = self.run_script("--import", mono=str(self.project / "no-such-godot"))
        self.assertEqual(r.returncode, 2, r.stdout + r.stderr)
        self.assertIn("the standard engine cannot run C#", r.stderr)
        self.assertNotIn("ARGS", r.stdout)


if __name__ == "__main__":
    unittest.main()
