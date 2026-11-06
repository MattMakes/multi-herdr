"""`intake hook-config`: print the settings.json snippet that registers the guard hook.

Ported from clean-room cr.py `hook-config`. Plain mode only: fleet runs use no
guard (decision Q5), so this exits 10 in fleet mode.
"""

from __future__ import annotations

import json
import sys

from . import guard
from .task import fleet_mode

HELP = "print the settings.json snippet that registers the guard hook (plain mode)"
FLEET_REFUSED = 10


def add_arguments(parser) -> None:
    pass


def run(args) -> int:
    if fleet_mode():
        print("intake: refused: no guard in fleet runs (each role worker has its own OS sandbox)", file=sys.stderr)
        return FLEET_REFUSED
    print(json.dumps({"hooks": {"PreToolUse": [{"matcher": ".*", "hooks": [
        {"type": "command", "command": f"python3 -I {guard.INTAKE} guard"}]}]}}, indent=2))
    return 0
