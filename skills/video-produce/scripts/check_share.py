#!/usr/bin/env python3
"""Check the share kit render/share.md against the film's one takeaway.

Usage:
  check_share.py render/share.md --brief BRIEF.md
  check_share.py --self-test

The takeaway is the first text line under the "## Core takeaway" heading of
BRIEF.md. share.md needs a "## X" section and a "## LinkedIn" section. Each
section holds the takeaway sentence word for word (case, spacing, quotes and
the end mark do not count). The X post is at most 280 characters. The
LinkedIn post is at most 3000 characters.

Exit codes: 0 pass, 1 a rule fails, 2 usage or a missing file.
Python standard library only.
"""

import os
import re
import sys

LIMITS = {"X": 280, "LinkedIn": 3000}


def sections(text):
    """Map each "## <name>" heading to the text under it."""
    found, name, lines = {}, None, []
    for line in text.splitlines():
        if line.startswith("## "):
            if name is not None:
                found[name] = "\n".join(lines).strip()
            name, lines = line[3:].strip(), []
        elif name is not None:
            lines.append(line)
    if name is not None:
        found[name] = "\n".join(lines).strip()
    return found


def normal(text):
    text = re.sub(r"[\"'`*_“”‘’]", "", text.lower())
    return re.sub(r"\s+", " ", text).strip().rstrip(".!?").strip()


def takeaway(brief_text):
    for name, body in sections(brief_text).items():
        if name.lower() == "core takeaway":
            for line in body.splitlines():
                if line.strip():
                    return line.strip()
    return None


def check(share_text, brief_text):
    sentence = takeaway(brief_text)
    if not sentence:
        return ["BRIEF.md has no text under '## Core takeaway'"]
    errors = []
    posts = sections(share_text)
    for name, limit in LIMITS.items():
        post = posts.get(name)
        if post is None:
            errors.append(f"no '## {name}' section")
            continue
        if normal(sentence) not in normal(post):
            errors.append(f"the {name} post does not hold the takeaway: {sentence}")
        if len(post) > limit:
            errors.append(f"the {name} post has {len(post)} characters (limit {limit})")
    return errors


BRIEF = """# Brief

## Audience
Backend developers who run CI on every pull request.

## Core takeaway
Every frame of your demo renders the same way twice.

## Call to action
Run horch video.
"""

SHARE = """# Share kit

## X
Every frame of your demo renders the same way twice. 30 s film below.

## LinkedIn
We made a 30 s launch film. The point: "every frame of your demo renders the
same way twice!" It took 1 brief and 6 waves.
"""


def self_test():
    failures = []
    if check(SHARE, BRIEF):
        failures.append(f"clean kit: {check(SHARE, BRIEF)}")
    bad = {
        "no takeaway in brief": (SHARE, BRIEF.replace("## Core takeaway", "## Core idea")),
        "no X section": (SHARE.replace("## X", "## Twitter"), BRIEF),
        "LinkedIn misses the takeaway": (SHARE.replace("renders the\nsame way", "renders"), BRIEF),
        "X post too long": (SHARE.replace("film below.", "film below. " + "x" * 300), BRIEF),
    }
    for name, (share, brief) in bad.items():
        errors = check(share, brief)
        if len(errors) != 1:
            failures.append(f"{name}: expected 1 error, got {errors}")
    for failure in failures:
        print(f"FAIL {failure}")
    print("check_share self-test: " + ("FAILED" if failures else f"{1 + len(bad)} cases passed"))
    return 1 if failures else 0


def main(argv):
    if argv == ["--self-test"]:
        return self_test()
    if len(argv) != 3 or argv[1] != "--brief":
        print(__doc__.strip(), file=sys.stderr)
        return 2
    share_path, brief_path = argv[0], argv[2]
    for path in (share_path, brief_path):
        if not os.path.isfile(path):
            print(f"{path}: no such file", file=sys.stderr)
            return 2
    with open(share_path, encoding="utf-8") as handle:
        share_text = handle.read()
    with open(brief_path, encoding="utf-8") as handle:
        brief_text = handle.read()
    errors = check(share_text, brief_text)
    for error in errors:
        print(f"{share_path}: {error}")
    print(f"{share_path}: {'FAIL' if errors else 'PASS'}")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
