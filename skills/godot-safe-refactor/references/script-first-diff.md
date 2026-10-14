# Script: first_diff.py

Usage: `python3 -I first_diff.py BASE.log NEW.log [--tol 1e-6]`

This script finds the first line that differs between 2 trace or event logs. It compares each
line as JSON when both sides parse, and numbers then match within `--tol` (default 0, exact).
It compares other lines as text. It prints the SHA-256 of each file, so a match can go into
notes as proof. It exits 0 when the logs match, 1 at the first difference, and 2 on a usage
error. Copy the block into a file, for example `first_diff.py`, to use it.

```python
#!/usr/bin/env python3
"""Find the first line that differs between two trace or event logs.

Usage: python3 -I first_diff.py BASE.log NEW.log [--tol 1e-6]

Each line is compared as JSON when both sides parse; numbers then match within --tol
(default 0, exact). Lines that are not JSON are compared as text.
Exit 0 when the logs match, 1 at the first difference, 2 on a usage error.
Prints the SHA-256 of each file, so a match can go into notes as proof.
"""
import argparse
import hashlib
import json
import sys


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 16), b""):
            h.update(block)
    return h.hexdigest()


def same(a, b, tol):
    if isinstance(a, bool) or isinstance(b, bool):
        return a == b
    if isinstance(a, (int, float)) and isinstance(b, (int, float)):
        return abs(a - b) <= tol
    if isinstance(a, dict) and isinstance(b, dict):
        return a.keys() == b.keys() and all(same(a[k], b[k], tol) for k in a)
    if isinstance(a, list) and isinstance(b, list):
        return len(a) == len(b) and all(same(x, y, tol) for x, y in zip(a, b))
    return a == b


def parse(line):
    try:
        return True, json.loads(line)
    except ValueError:
        return False, line


def main():
    ap = argparse.ArgumentParser(description="First difference between two logs.")
    ap.add_argument("base")
    ap.add_argument("new")
    ap.add_argument("--tol", type=float, default=0.0, help="numeric tolerance for JSON values")
    args = ap.parse_args()
    print(f"base {sha256(args.base)}  {args.base}")
    print(f"new  {sha256(args.new)}  {args.new}")
    with open(args.base, encoding="utf-8") as fa, open(args.new, encoding="utf-8") as fb:
        n = 0
        while True:
            la, lb = fa.readline(), fb.readline()
            n += 1
            if not la and not lb:
                print(f"MATCH: {n - 1} lines (tolerance {args.tol})")
                return 0
            if not la or not lb:
                short = "base" if not la else "new"
                print(f"DIFF at line {n}: {short} ends early")
                print(f"  base: {la.rstrip()}")
                print(f"  new:  {lb.rstrip()}")
                return 1
            ja, va = parse(la)
            jb, vb = parse(lb)
            ok = same(va, vb, args.tol) if (ja and jb) else la.rstrip("\n") == lb.rstrip("\n")
            if not ok:
                print(f"DIFF at line {n}")
                print(f"  base: {la.rstrip()}")
                print(f"  new:  {lb.rstrip()}")
                return 1


if __name__ == "__main__":
    sys.exit(main())
```
