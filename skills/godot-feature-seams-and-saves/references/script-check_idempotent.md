# Script: check_idempotent.py

Usage: `python3 check_idempotent.py --file project.godot -- <tool command...>`

The script copies the target file, runs the setup tool, and prints the diff of the first run. Then it runs the tool again and fails if the second run changes the file. Exit codes: 0 when the tool is idempotent, 1 when the second run changes the file, 2 when a tool run fails. Copy the block into a file to use it. It uses the Python standard library only.

```python
#!/usr/bin/env python3
"""Check that a setup tool changes a file as intended once, then never again.

Usage: python3 check_idempotent.py --file project.godot -- <tool command...>

Example:
  python3 check_idempotent.py --file project.godot -- \
      godot --headless --path . -s res://tools/setup_input.gd

The script copies the file, runs the tool, prints the diff of the first run,
runs the tool again and fails if the second run changes the file.
Exit codes: 0 idempotent, 1 second run changed the file, 2 a tool run failed.
"""
import argparse
import difflib
import subprocess
import sys
from pathlib import Path


def run(cmd):
    result = subprocess.run(cmd)
    if result.returncode != 0:
        print(f"tool exited with {result.returncode}: {' '.join(cmd)}", file=sys.stderr)
        sys.exit(2)


def diff(before, after, label):
    return list(difflib.unified_diff(
        before.splitlines(keepends=True), after.splitlines(keepends=True),
        fromfile=f"{label} before", tofile=f"{label} after"))


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--file", required=True, help="file the tool writes")
    parser.add_argument("cmd", nargs=argparse.REMAINDER, help="-- tool command")
    args = parser.parse_args()
    cmd = args.cmd[1:] if args.cmd[:1] == ["--"] else args.cmd
    if not cmd:
        parser.error("give the tool command after --")
    target = Path(args.file)
    original = target.read_text() if target.exists() else ""

    run(cmd)
    first = target.read_text()
    first_diff = diff(original, first, "run 1")
    print(f"run 1: {len(first_diff)} diff lines")
    sys.stdout.writelines(first_diff)

    run(cmd)
    second = target.read_text()
    second_diff = diff(first, second, "run 2")
    if second_diff:
        print("run 2 changed the file: the tool is not idempotent")
        sys.stdout.writelines(second_diff)
        return 1
    print("run 2: no change (idempotent)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
```
