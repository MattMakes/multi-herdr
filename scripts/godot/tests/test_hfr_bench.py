"""Tests for skills/godot-hfr-hunt/scripts/hfr_bench.py (GRS-20, GRS-44). Run: python3 -m unittest discover -s scripts/godot/tests

A fake export stands in for a Godot release export. It logs its arguments and
writes the take file that the plan in HFR_FAKE_PLAN names for its --hfr-out
path. A take that the plan does not name gets no take file. No Godot runs.
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
SCRIPT = HERE.parents[2] / "skills" / "godot-hfr-hunt" / "scripts" / "hfr_bench.py"

FAKE = """#!{python}
import json, os, sys
if sys.argv[1:] == ["warm"]:
    sys.exit(0)
with open(os.environ["HFR_FAKE_LOG"], "a") as log:
    log.write(json.dumps(sys.argv[1:]) + "\\n")
out = next(a.split("=", 1)[1] for a in sys.argv if a.startswith("--hfr-out="))
plan = json.load(open(os.environ["HFR_FAKE_PLAN"]))
take = plan.get(os.path.basename(out))
if take is None:
    sys.exit(3)
frames = [ms for ms, count in take["runs"] for _ in range(count)]
doc = {{
    "schema_version": 1,
    "provenance": {{
        "scenario": "city", "renderer": "forward_plus", "rendering_driver": "metal",
        "resolution": [1920, 1080], "arguments": take.get("arguments", []),
        "thermal_start": "unknown", "thermal_end": take.get("thermal_end", "unknown"),
    }},
    "validity": {{
        "foreground": take.get("foreground", True),
        "vsync_period_detected": take.get("vsync", False),
        "gpu_timer_available": False, "warmup_seconds": 5, "sample_seconds": 30,
    }},
    "summary": {{}},
    "frames": [{{"i": i, "frame_ms": ms, "render_cpu_ms": None, "render_gpu_ms": None,
                "draw_calls": 0, "primitives": 0}} for i, ms in enumerate(frames)],
}}
with open(out, "w") as f:
    json.dump(doc, f)
"""


def take(tail_ms, base_ms=3.0, **extra):
    count = int(21000 / base_ms)
    return {"runs": [[base_ms, count], [tail_ms, count // 70]], **extra}


class HfrBenchTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name).resolve()
        self.export = self.root / "fake-export"
        self.export.write_text(FAKE.format(python=sys.executable))
        self.export.chmod(0o755)
        subprocess.run([str(self.export), "warm"], check=True, timeout=60)
        self.out = self.root / "out"
        self.log = self.root / "args.log"

    def tearDown(self):
        self.tmp.cleanup()

    def compare(self, baseline, candidate, *extra):
        plan = {"city-warmup.json": take(3.0)}
        for role, tails in (("baseline", baseline), ("candidate", candidate)):
            for n, spec in enumerate(tails, 1):
                if spec is not None:
                    plan[f"city-{role}-{n:02d}.json"] = spec if isinstance(spec, dict) else take(spec)
        (self.root / "plan.json").write_text(json.dumps(plan))
        env = dict(os.environ, HFR_FAKE_PLAN=str(self.root / "plan.json"), HFR_FAKE_LOG=str(self.log),
                   PATH=os.path.dirname(sys.executable) + os.pathsep + "/usr/bin:/bin")
        return subprocess.run(
            [sys.executable, str(SCRIPT), "compare", "--baseline", str(self.export),
             "--candidate", str(self.export), "--scenario", "city", "--out", str(self.out), *extra],
            env=env, capture_output=True, text=True, timeout=120, check=False)

    def verdict(self, r, line, code):
        lines = r.stdout.splitlines()
        self.assertEqual(r.returncode, code, r.stdout + r.stderr)
        self.assertEqual(lines[-1], line, r.stdout)
        verdicts = {"PASS", "FRAME_TIME_REGRESSION", "INFRASTRUCTURE_INVALID"}
        found = [l for l in lines if l in verdicts or l.endswith("_FPS_BUDGET_FAILURE")]
        self.assertEqual(found, [line], r.stdout)

    def test_hfr_bench_summarize_uses_nearest_rank_percentiles(self):
        path = self.root / "take.json"
        frames = [5, 1, 9, 3, 7, 2, 8, 4, 10, 6]
        path.write_text(json.dumps({"schema_version": 1, "summary": {},
                                    "frames": [{"i": i, "frame_ms": ms} for i, ms in enumerate(frames)]}))
        r = subprocess.run([sys.executable, str(SCRIPT), "summarize", str(path)],
                           capture_output=True, text=True, timeout=60, check=False)
        self.assertEqual(r.returncode, 0, r.stderr)
        summary = json.loads(r.stdout)
        self.assertEqual(summary["frames"], 10)
        self.assertEqual(summary["frame_ms_p50"], 5)
        self.assertEqual(summary["frame_ms_p95"], 10)
        self.assertEqual(summary["frame_ms_p99"], 10)
        self.assertEqual(summary["worst_frame_ms"], 10)
        self.assertAlmostEqual(summary["one_percent_low_fps"], 100.0)
        self.assertEqual(summary["over_3_333_ms"], 7)
        self.assertEqual(summary["over_4_167_ms"], 6)

    def test_hfr_bench_compare_flags_regression_on_four_of_five_pairs(self):
        r = self.compare([3.5] * 5, [3.8, 3.8, 3.4, 3.8, 3.8])
        self.verdict(r, "FRAME_TIME_REGRESSION", 1)
        report = json.loads((self.out / "compare.json").read_text())
        self.assertEqual(len(report["pairs"]), 5)
        self.assertTrue(all(p["valid"] for p in report["pairs"]))
        self.assertAlmostEqual(report["pairs"][2]["p99_delta"], -0.1)
        self.assertAlmostEqual(report["pairs"][0]["p99_delta"], 0.3)

    def test_hfr_bench_compare_ignores_delta_below_noise_floor(self):
        r = self.compare([3.5] * 5, [3.6] * 5)
        self.verdict(r, "PASS", 0)
        r = self.compare([take(1.5, 1.0)] * 5, [take(1.58, 1.0)] * 5)
        self.verdict(r, "PASS", 0)
        r = self.compare([take(1.5, 1.0)] * 5, [take(1.62, 1.0)] * 5)
        self.verdict(r, "FRAME_TIME_REGRESSION", 1)

    def test_hfr_bench_compare_reports_budget_failure_without_regression(self):
        r = self.compare([4.5] * 5, [4.5] * 5)
        self.verdict(r, "240_FPS_BUDGET_FAILURE", 1)
        r = self.compare([4.5] * 5, [4.5] * 5, "--target-fps", "200")
        self.verdict(r, "PASS", 0)
        self.assertIn("240 fps budget 4.167 ms: not met", r.stdout)
        self.assertIn("300 fps budget 3.333 ms: not met", r.stdout)
        r = self.compare([3.0] * 5, [3.6] * 5, "--target-fps", "300")
        self.verdict(r, "FRAME_TIME_REGRESSION", 1)
        r = self.compare([3.6] * 5, [3.6] * 5, "--target-fps", "300")
        self.verdict(r, "300_FPS_BUDGET_FAILURE", 1)
        r = self.compare([3.6] * 5, [3.6] * 5)
        self.verdict(r, "PASS", 0)
        self.assertIn("240 fps budget 4.167 ms: met", r.stdout)
        for bad in ("199", "301", "fast"):
            r = self.compare([3.0] * 5, [3.0] * 5, "--target-fps", bad)
            self.assertEqual(r.returncode, 2, r.stdout + r.stderr)
            self.assertIn("--target-fps", r.stderr)

    def test_hfr_bench_compare_is_infrastructure_invalid_below_three_pairs(self):
        r = self.compare([3.5, 3.5, take(3.5, foreground=False), 3.5, 3.5],
                         [4.5, None, 4.5, take(4.5, vsync=True), 4.5])
        self.verdict(r, "INFRASTRUCTURE_INVALID", 2)
        report = json.loads((self.out / "compare.json").read_text())
        self.assertEqual([p["valid"] for p in report["pairs"]], [True, False, False, False, True])
        self.assertTrue(all(p["reason"] for p in report["pairs"] if not p["valid"]))
        r = self.compare([3.5, take(3.5, thermal_end="serious"), take(3.5, arguments=["x"]),
                          {"runs": [[3.0, 6000]]}, 3.5],
                         [4.5, 4.5, 4.5, 4.5, 4.5])
        self.verdict(r, "INFRASTRUCTURE_INVALID", 2)
        r = self.compare([3.5, 3.5, None, None, 3.5], [3.8, 3.8, 3.8, 3.8, 3.8])
        self.verdict(r, "FRAME_TIME_REGRESSION", 1)
        r = self.compare([3.5, 3.5, None, None, 3.5], [3.8, 3.4, 3.8, 3.8, 3.8])
        self.verdict(r, "PASS", 0)

    def test_hfr_bench_runs_fake_export_without_godot(self):
        self.assertTrue(os.access(SCRIPT, os.X_OK), SCRIPT)
        self.assertEqual(SCRIPT.stat().st_mode & 0o777, 0o755)
        r = self.compare([3.0] * 5, [3.0] * 5)
        self.verdict(r, "PASS", 0)
        self.assertIsNone(shutil.which("godot", path="/usr/bin:/bin"))
        calls = [json.loads(l) for l in self.log.read_text().splitlines()]
        self.assertEqual(len(calls), 11)
        names = [c[-1].rsplit("/", 1)[1] for c in calls]
        self.assertEqual(names[:3], ["city-warmup.json", "city-baseline-01.json", "city-candidate-01.json"])
        self.assertEqual(names[-1], "city-candidate-05.json")
        self.assertEqual(calls[1], [
            "--windowed", "--resolution", "1920x1080", "--disable-vsync", "--max-fps", "0", "--",
            "--hfr-bench=city", "--warmup-seconds=5", "--sample-seconds=30",
            f"--hfr-out={self.out}/city-baseline-01.json"])
        self.assertTrue((self.out / "compare.json").is_file())
        r = self.compare([3.0] * 3, [3.0] * 3, "--pairs", "3")
        self.verdict(r, "PASS", 0)


if __name__ == "__main__":
    unittest.main()
