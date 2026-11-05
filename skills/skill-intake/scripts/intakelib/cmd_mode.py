"""`intake mode`: prints `fleet` or `plain` by the rule of spec 6.2. Exit 0."""

from __future__ import annotations

from . import task

HELP = "print fleet or plain (the mode rule of spec 6.2)"


def add_arguments(p) -> None:
    pass


def run(args) -> int:
    print(task.mode())
    return 0
