# Script: text_fit.py

Usage: `python3 -I text_fit.py LINES.json`

The script checks that every game text line fits the character limit of its line kind. A line
whose kind has no limit is an error. It prints 1 line per failure and a summary, and exits 1
when any line fails. It uses the Python standard library only. Copy the block into a file named
`text_fit.py` to use it. The procedure that uses it is the text fit check in `ui-review.md`.

```python
#!/usr/bin/env python3
"""Check that every game text line fits the limit of its line kind. Standard library only.

Usage:
  python3 -I text_fit.py LINES.json

LINES.json shape:
  {
    "limits": {"toast": 40, "title": 18, "page": 280},
    "lines": [
      {"kind": "toast", "id": "first_win", "text": "You cleared the first area!"},
      {"kind": "title", "text": "Settings"}
    ]
  }

A limit counts characters after surrounding spaces are removed. A line whose kind has no
limit is an error, so a new kind cannot slip past the check. Prints 1 line per failure and a
summary. Exit code 0 when every line fits, 1 when any line is over or has an unknown kind.
"""
import json
import sys


def check(doc):
    limits = doc.get("limits", {})
    failures = []
    for i, line in enumerate(doc.get("lines", [])):
        kind = line.get("kind")
        name = line.get("id", f"#{i}")
        text = line.get("text", "").strip()
        if kind not in limits:
            failures.append(f"UNKNOWN KIND {kind!r} {name}")
            continue
        if len(text) > limits[kind]:
            failures.append(f"OVER {kind} {name} {len(text)}/{limits[kind]} "
                            f"by {len(text) - limits[kind]}: {text}")
    return failures


def main(argv):
    if argv[:1] in (["--help"], ["-h"]):
        print(__doc__.strip())
        return 0
    if len(argv) != 1:
        print("\n".join(__doc__.strip().splitlines()[2:4]))
        return 2
    with open(argv[0], encoding="utf-8") as f:
        doc = json.load(f)
    failures = check(doc)
    for line in failures:
        print(line)
    total = len(doc.get("lines", []))
    print(f"{total - len(failures)}/{total} lines fit")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
```

An example `LINES.json` file:

```json
{
  "limits": {"toast": 40, "title": 18, "page": 280},
  "lines": [
    {"kind": "toast", "id": "first_win", "text": "You cleared the first area!"},
    {"kind": "title", "text": "Settings"}
  ]
}
```
