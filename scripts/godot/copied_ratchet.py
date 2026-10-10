"""The ratchet for failures in copied Godot text (U-22).

Copied text (a `copied_files` path in skills/copied.json) stays unchanged, so
some of its code blocks fail a check. A check run with `--strict-own` does
not print each such failure. It counts them per skill and compares the counts
with `copied-baseline.json`:

- a count above its baseline fails the run and prints that skill's failures;
- a count below its baseline passes and tells the reader to lower the
  baseline (`--update-baseline` writes the current counts of the check);
- a skill that the baseline does not name has a baseline of 0.

Only the skills of the scanned files are compared, so a run on 1 skill does
not report the others. The baseline file holds one object per check:
`{"<check>": {"<skill>": <count>, ...}, ...}`. A count of 0 is not stored.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

BASELINE = Path(__file__).resolve().parent / "copied-baseline.json"


def copied_skills(copied: Path) -> dict[Path, str]:
    """Resolved path of every copied file -> the name of its skill.

    A `copied_files` path `<rel>` of skill `<skill>` is the file
    `<copied.json dir>/<skill>/<rel>`.
    """
    root = copied.resolve().parent
    out = {}
    for entry in json.loads(copied.read_text())["skills"]:
        for rel in entry["copied_files"]:
            out[(root / entry["name"] / rel).resolve()] = entry["name"]
    return out


def load(path: Path) -> dict[str, dict[str, int]]:
    return json.loads(path.read_text()) if path.is_file() else {}


def apply(
    check: str,
    unit: str,
    failures: list[tuple[Path, str]],
    scanned: list[Path],
    copied: dict[Path, str],
    baseline_path: Path = BASELINE,
    update: bool = False,
    out=None,
) -> int:
    """Count copied failures per skill and compare with the baseline.

    `failures` is (file, report line) for each failure in a copied file;
    `unit` names what is counted ("blocks", "deprecated names").
    Return 1 when a count grew, else 0.
    """
    out = out or sys.stdout
    lines: dict[str, list[str]] = {}
    for path, text in failures:
        lines.setdefault(copied[path.resolve()], []).append(text)
    counts = {skill: len(v) for skill, v in lines.items()}
    skills = {copied[p.resolve()] for p in scanned if p.resolve() in copied}
    data = load(baseline_path)
    base = data.get(check, {})

    if update:
        kept = {s: n for s, n in base.items() if s not in skills}
        kept.update(counts)
        data[check] = dict(sorted(kept.items()))
        baseline_path.write_text(json.dumps(dict(sorted(data.items())), indent=2) + "\n")
        base = data[check]
        print(f"{check}: wrote the {check} counts to {baseline_path}", file=out)

    total = sum(counts.values())
    limit = sum(n for s, n in base.items() if s in skills)
    print(
        f"{check}: {total} copied {unit} fail in {len(counts)} skills, report only "
        f"(baseline {limit}, {baseline_path.name})",
        file=out,
    )
    if counts:
        per = ", ".join(f"{s} {n}" for s, n in sorted(counts.items(), key=lambda x: (-x[1], x[0])))
        print(f"  per skill: {per}", file=out)
    grew = 0
    for skill in sorted(skills):
        now, was = counts.get(skill, 0), base.get(skill, 0)
        if now > was:
            grew = 1
            print(
                f"FAIL {check}: {skill} has {now} copied {unit} that fail, baseline {was}. "
                f"Fix it; if the copy is new, raise the baseline with --update-baseline:",
                file=out,
            )
            for text in lines[skill]:
                print(f"  {text}", file=out)
        elif now < was:
            print(
                f"{check}: {skill} has {now} copied {unit} that fail, baseline {was}. "
                f"Lower the baseline: run {check}.py with --update-baseline.",
                file=out,
            )
    return grew
