#!/usr/bin/env python3
"""intake: the skill-intake CLI. Standard library only. Run as `python3 -I intake.py <cmd> ...`.

JSON for machines goes to stdout; lines for humans go to stderr.

Each command is one module, intakelib/cmd_<name>.py, with:

    HELP = "<one line>"
    def add_arguments(parser): ...      # argparse sub-parser
    def run(args) -> int: ...           # the exit code

To add a command, write its module and add 1 line to COMMANDS.
"""

from __future__ import annotations

import argparse
import importlib
import sys
from pathlib import Path

# Never write __pycache__ into the skill folder (its digest covers every file).
sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

from intakelib.task import TaskError  # noqa: E402

COMMANDS = (
    "cr",
    "guard",
    "hook-config",
    "scan",
    "acquire",
    "discover",
    "triage",
    "report",
    "run",
    "verify",
    "install",
    "mode",
    "brief",
    "spawn",
    "roles",
    "role-result",
    "subagent-prompt",
)


def parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="intake", description="skill-intake: safe harvest of a skill source")
    sub = p.add_subparsers(dest="command", metavar="<command>")
    for name in COMMANDS:
        mod = importlib.import_module(f"intakelib.cmd_{name.replace('-', '_')}")
        sp = sub.add_parser(name, help=mod.HELP)
        mod.add_arguments(sp)
        sp.set_defaults(_run=mod.run)
    return p


def main(argv: list[str] | None = None) -> int:
    p = parser()
    args = p.parse_args(argv)
    if not getattr(args, "_run", None):
        p.print_usage(sys.stderr)
        print("intake: error: give a command", file=sys.stderr)
        return 2
    try:
        return args._run(args)
    except TaskError as e:
        print(f"intake: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
