# Asserted edit helper: `rep.py`

Usage: `python3 -I rep.py <file> <pairs.json>`

The script replaces text in 1 file by exact pairs. `pairs.json` is a JSON list of `[old, new]` strings. Each old text must occur exactly once at the time the script applies it. The script applies all pairs in memory and writes the file once, only when every pair passes. It exits 0 on success and 1 when a pair fails; the file then stays unchanged.

Copy the block into a file named `rep.py`. `python3 rep.py --help` prints the usage.

```python
"""Usage: python3 -I rep.py <file> <pairs.json>   (or: python3 -I rep.py --help)

Replace text in 1 file by exact pairs. pairs.json is a list of [old, new] strings.
Each old text must occur exactly once in the file at the time it is applied.
All pairs apply in memory; the file is written once, only when every pair passes.
Exit 0 on success, 1 when a pair fails (the file stays unchanged).
"""
import json
import sys


def apply_pairs(text, pairs):
    for n, (old, new) in enumerate(pairs, 1):
        if old == new:
            raise ValueError(f"pair {n}: old equals new; drop the pair")
        hits = text.count(old)
        if hits != 1:
            raise ValueError(f"pair {n}: old text found {hits} times, need 1: {old[:60]!r}")
        text = text.replace(old, new)
    return text


def main(argv):
    if len(argv) >= 2 and argv[1] in ("-h", "--help"):
        print(__doc__)
        return 0
    if len(argv) != 3:
        print(__doc__, file=sys.stderr)
        return 64
    path, pairs_path = argv[1], argv[2]
    with open(pairs_path, encoding="utf-8") as f:
        pairs = json.load(f)
    with open(path, encoding="utf-8", newline="") as f:
        text = f.read()
    try:
        out = apply_pairs(text, pairs)
    except ValueError as e:
        print(f"rep: {path}: {e}; file unchanged", file=sys.stderr)
        return 1
    with open(path, "w", encoding="utf-8", newline="") as f:
        f.write(out)
    print(f"rep: {path}: {len(pairs)} pairs applied")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
```
