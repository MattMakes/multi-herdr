import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent.parent / "gdcomplexity.py"

INSTALL_HINT = 'install uv, or run: uv tool install gdtoolkit==4.5.0 or pipx install "gdtoolkit==4.*"'

TANGLED = """extends Node

var x = 1

func simple(a, b: int, c := 2):
\treturn a


func tangled(v):
\tvar f = func(q): return q if q > 1 else 0
\tfor a in v:
\t\tif a > 1 and a < 9 or a == 3:
\t\t\tpass
\t\telif a == 0:
\t\t\tpass
\twhile x:
\t\tpass
\tmatch v:
\t\t1:
\t\t\tpass
\t\tvar z when z > 2:
\t\t\tpass
\treturn 0

class Inner:
\tfunc m(a, b, c, d, e, f, g, h):
\t\tif a:
\t\t\treturn b
\t\treturn a

var after = 2
"""


def runner():
    if shutil.which("uv"):
        return ["uv", "run", "--quiet", "--with", "gdtoolkit==4.5.0", "python3", "-I"]
    probe = subprocess.run(
        [sys.executable, "-c", "import gdtoolkit.parser"], capture_output=True
    )
    if probe.returncode == 0:
        return [sys.executable, "-B"]
    raise AssertionError("no gdscript analyser: " + INSTALL_HINT)


def analyse(source, *args):
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "s.gd"
        path.write_text(source)
        out = subprocess.run(
            runner() + [str(SCRIPT), "--format", "json", *args, str(path)],
            capture_output=True,
            text=True,
            cwd=tmp,
        )
        if out.returncode != 0:
            raise AssertionError(out.stderr)
        report = json.loads(out.stdout)
        return report, str(path)


class GdComplexityTest(unittest.TestCase):
    def test_gdcomplexity_counts_branches_and_lambdas(self):
        report, path = analyse(TANGLED)
        self.assertEqual(
            report["files"],
            [
                {
                    "filePath": path,
                    "functions": [
                        {"name": "simple", "line": 5, "endLine": 6},
                        {"name": "", "line": 7, "endLine": 7},
                        {"name": "tangled", "line": 9, "endLine": 23},
                        {"name": "", "line": 24, "endLine": 24},
                        {"name": "Inner.m", "line": 26, "endLine": 29},
                        {"name": "", "line": 30, "endLine": 30},
                    ],
                }
            ],
        )
        got = [
            (v["functionName"], v["line"], v["metric"], v["value"], v["threshold"])
            for v in report["violations"]
        ]
        self.assertEqual(
            got,
            [
                ("tangled", 9, "cyclomaticComplexity", 11, 10),
                ("Inner.m", 26, "parameterCount", 8, 7),
            ],
        )
        self.assertTrue(all(v["filePath"] == path for v in report["violations"]))

    def test_gdcomplexity_reads_config_thresholds(self):
        with tempfile.TemporaryDirectory() as tmp:
            json_cfg = Path(tmp) / ".static-analysisrc.json"
            json_cfg.write_text(
                json.dumps(
                    {
                        "thresholds": {
                            "cyclomaticComplexity": {"warn": 1, "error": 20},
                            "halsteadEffort": {"warn": 1},
                        }
                    }
                )
            )
            yaml_cfg = Path(tmp) / ".static-analysisrc.yml"
            yaml_cfg.write_text("thresholds:\n  parameterCount:\n    error: 9\n")
            pkg = Path(tmp) / "package.json"
            pkg.write_text(
                json.dumps(
                    {"static-analysis": {"thresholds": {"parameterCount": {"warn": 2}}}}
                )
            )
            by_json, _ = analyse(TANGLED, "--config", str(json_cfg))
            by_yaml, _ = analyse(TANGLED, "--config", str(yaml_cfg))
            by_pkg, _ = analyse(TANGLED, "--config", str(pkg))

        def picked(report):
            return [
                (v["functionName"], v["metric"], v["value"], v["threshold"])
                for v in report["violations"]
            ]

        self.assertEqual(
            picked(by_json),
            [
                ("tangled", "cyclomaticComplexity", 11, 1),
                ("Inner.m", "cyclomaticComplexity", 2, 1),
                ("Inner.m", "parameterCount", 8, 7),
            ],
        )
        self.assertEqual(
            picked(by_yaml), [("tangled", "cyclomaticComplexity", 11, 10)]
        )
        self.assertEqual(
            picked(by_pkg),
            [
                ("simple", "parameterCount", 3, 2),
                ("tangled", "cyclomaticComplexity", 11, 10),
                ("Inner.m", "parameterCount", 8, 2),
            ],
        )


if __name__ == "__main__":
    unittest.main()
