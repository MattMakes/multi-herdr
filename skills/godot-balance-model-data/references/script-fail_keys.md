# Script: fail_keys.py

Usage: `python3 fail_keys.py before.txt after.txt [--pattern REGEX]`

The script compares the FAIL keys of 2 balance-model outputs. A FAIL key is
the check name plus its subject, without the measured numbers. The default key
is the text before the word FAIL. `--pattern` with a `(?P<key>...)` group sets
your own key. The script prints the new and the cleared keys, and exits 1 when
a new key appears. Copy the block into a file named `fail_keys.py` to use it.
It uses the Python standard library only.

```python
#!/usr/bin/env python3
"""Compare FAIL keys between two balance-model outputs.

Usage: python3 fail_keys.py BEFORE.txt AFTER.txt [--pattern REGEX]

A FAIL key is the check name plus its subject, without the measured numbers,
so a value that moves inside an old FAIL does not count as new. By default a
line counts when it contains the word FAIL, and its key is the text before
that word, without a leading "12." line number. When FAIL starts the line, the
key is the whole line with every standalone number replaced by "#".
Pass --pattern to choose lines with your own regex; if it has a group named
"key", that group is the key.
Exit code: 0 when no new FAIL key appears, 1 when one or more appear.
"""
import argparse
import re
import sys

NUMBER = re.compile(r"(?<![\w.])[-+]?\d+(?:[.,]\d+)*(?:[eE][-+]?\d+)?%?(?![\w])")
LINE_NUMBER = re.compile(r"^\s*\d+[.)]\s*")


def keys(path, pattern):
    found = set()
    with open(path, encoding="utf-8", errors="replace") as handle:
        for line in handle:
            match = pattern.search(line)
            if not match:
                continue
            if "key" in pattern.groupindex:
                text = match.group("key")
            else:
                text = LINE_NUMBER.sub("", line[: match.start()])
                if not text.strip():
                    text = NUMBER.sub("#", line)
            text = " ".join(text.split())
            found.add(text)
    return found


def main():
    parser = argparse.ArgumentParser(description="Diff FAIL keys of 2 model outputs.")
    parser.add_argument("before")
    parser.add_argument("after")
    parser.add_argument("--pattern", default=r"\bFAIL\b")
    args = parser.parse_args()
    pattern = re.compile(args.pattern)
    before = keys(args.before, pattern)
    after = keys(args.after, pattern)
    new = sorted(after - before)
    cleared = sorted(before - after)
    print(f"before: {len(before)} keys, after: {len(after)} keys")
    print(f"new FAIL keys: {len(new)}")
    for key in new:
        print(f"  + {key}")
    print(f"cleared FAIL keys: {len(cleared)}")
    for key in cleared:
        print(f"  - {key}")
    return 1 if new else 0


if __name__ == "__main__":
    sys.exit(main())
```
