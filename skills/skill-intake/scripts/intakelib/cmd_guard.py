"""`intake guard`: the PreToolUse hook entry point (plain mode only; reads hook JSON on stdin).

Ported from clean-room cr.py `guard`. Exit 2 blocks the tool call.
"""

from __future__ import annotations

from . import guard

HELP = "PreToolUse hook entry point for role subagents (plain mode; hook JSON on stdin)"


def add_arguments(parser) -> None:
    pass


def run(args) -> int:
    return guard.main()
