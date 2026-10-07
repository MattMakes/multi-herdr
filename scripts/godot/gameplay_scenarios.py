#!/usr/bin/env python3
"""Run the deterministic scenarios of the GW9 gameplay bundles headless (U-25).

Usage:
    scripts/godot/gameplay_scenarios.py [--timeout SECONDS] [--keep] [skill ...]

Each directory `scripts/godot/tests/fixtures/gameplay/<skill>/` is a small
Godot project: `project.godot` and a `SceneTree` script `check.gd`. For each
one the runner:

1. copies the project and `fixtures/gameplay/expect.gd` to a scratch
   directory under `.worktrees/_scratch/godot-gameplay/`;
2. writes every ```gdscript block of `skills/<skill>/` into `res://skill/`,
   exactly as the skill text has it: a block with `class_name X` is `X.gd`,
   another block is `<file stem>_<n>.gd`, where n counts the gdscript blocks
   of that markdown file from 1;
3. runs `--import`, so the `class_name` scripts are registered;
4. runs `res://check.gd`, with a time bound.

A scenario passes when Godot exits 0, prints `scenario: N of N checks
pass` with N > 0, and prints no `SCRIPT ERROR` or `ERROR:` line.
`expect.gd` has the check counter and a watchdog: a runtime error stops
`_initialize`, and the watchdog quits after 20 s with an `ERROR:` line. HOME and
the XDG directories point into the scratch directory, so `user://` and the
logs stay out of the real home.

Godot is found as `api_check.py` finds it. With no Godot the runner prints
"skipped: no Godot" and exits 0. Exit status: 0 when every scenario passes,
1 otherwise, 2 on a usage error.
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from api_check import REPO, find_godot  # noqa: E402
from gdscript_blocks_check import CLASS_NAME, extract  # noqa: E402

FIXTURES = Path(__file__).resolve().parent / "tests" / "fixtures" / "gameplay"
SKILLS = REPO / "skills"
SCRATCH = REPO / ".worktrees" / "_scratch" / "godot-gameplay"
SUMMARY = re.compile(r"^scenario: (\d+) of (\d+) checks pass$", re.MULTILINE)
ERROR = re.compile(r"^(SCRIPT ERROR|ERROR):", re.MULTILINE)


@dataclass
class Result:
    skill: str
    ok: bool
    detail: str
    output: str


def scenarios() -> list[str]:
    return sorted(p.name for p in FIXTURES.iterdir() if (p / "check.gd").is_file())


def write_blocks(skill_dir: Path, out: Path) -> list[str]:
    """Write the gdscript blocks of a skill into `out`; return the file names."""
    out.mkdir(parents=True, exist_ok=True)
    names = []
    for md in sorted(skill_dir.rglob("*.md")):
        for n, block in enumerate(extract(md), 1):
            m = next((m for ln in block.code.split("\n") if (m := CLASS_NAME.match(ln))), None)
            name = f"{m[1]}.gd" if m else f"{md.stem}_{n}.gd"
            if name in names:
                raise SystemExit(f"gameplay_scenarios.py: 2 blocks of {skill_dir.name} write {name}")
            (out / name).write_text(block.code)
            names.append(name)
    return names


def run_one(godot: str, skill: str, timeout: int, keep: bool = False) -> Result:
    SCRATCH.mkdir(parents=True, exist_ok=True)
    root = Path(tempfile.mkdtemp(prefix=f"{skill}-", dir=SCRATCH))
    try:
        project = root / "project"
        shutil.copytree(FIXTURES / skill, project)
        shutil.copyfile(FIXTURES / "expect.gd", project / "expect.gd")
        write_blocks(SKILLS / skill, project / "skill")
        home = root / "home"
        env = dict(os.environ, HOME=str(home), XDG_DATA_HOME=str(home / "data"),
                   XDG_CONFIG_HOME=str(home / "config"), XDG_CACHE_HOME=str(home / "cache"))
        base = [godot, "--headless", "--path", str(project)]
        try:
            imp = subprocess.run(base + ["--import"], env=env, capture_output=True, text=True, timeout=timeout)
            run = subprocess.run(base + ["--script", "res://check.gd"], env=env,
                                 capture_output=True, text=True, timeout=timeout)
        except subprocess.TimeoutExpired as e:
            return Result(skill, False, f"timed out after {timeout} s", str(e.stdout or "") + str(e.stderr or ""))
        output = run.stdout + run.stderr
        if imp.returncode != 0:
            return Result(skill, False, f"--import exited {imp.returncode}", imp.stdout + imp.stderr)
        errors = ERROR.findall(output)
        summary = SUMMARY.search(output)
        if errors:
            detail = f"{len(errors)} error lines, exit {run.returncode}"
        elif run.returncode != 0:
            detail = f"exit {run.returncode}"
        elif summary is None or int(summary[2]) == 0 or summary[1] != summary[2]:
            detail = "no 'scenario: N of N checks pass' line"
        else:
            return Result(skill, True, summary[0], output)
        return Result(skill, False, detail, output)
    finally:
        if keep:
            print(f"scratch project: {root}", file=sys.stderr)
        else:
            shutil.rmtree(root, ignore_errors=True)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--timeout", type=int, default=120, help="seconds for each Godot run")
    ap.add_argument("--keep", action="store_true", help="keep the scratch projects")
    ap.add_argument("skills", nargs="*", help="the fixtures to run (default: all)")
    args = ap.parse_args(argv)
    known = scenarios()
    unknown = [s for s in args.skills if s not in known]
    if unknown:
        print(f"gameplay_scenarios.py: no fixture for {', '.join(unknown)}", file=sys.stderr)
        return 2
    godot = find_godot()
    if godot is None:
        print("skipped: no Godot")
        return 0
    failed = 0
    for skill in args.skills or known:
        r = run_one(godot, skill, args.timeout, args.keep)
        print(f"{'ok  ' if r.ok else 'FAIL'} {skill}: {r.detail}")
        if not r.ok:
            failed += 1
            print(r.output[-6000:])
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
