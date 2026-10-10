#!/usr/bin/env python3
"""Check that every cross-reference in the godot-* skills resolves.

Usage:
    scripts/godot/xref_check.py [--skills DIR] [--teammates DIR] [<skill dirs...>]

With no skill dirs it checks every `godot-*` directory under `--skills`
(default `skills/`). It checks 2 things in every text file of a skill:

1. Every `godot-<name>` word names an existing skill directory
   `<skills>/godot-<name>/`. These mentions are not skill references and are
   skipped: a teammate (`<teammates>/godot-<name>.md`), a name in EXTERNAL
   (repositories and projects such as godot-cpp), and a word inside a URL, a
   path (`org/godot-x`, `references/godot-x.md`, `.godot-x`) or a link anchor
   (`#godot-47-additions`).
2. Every relative markdown link `[text](target)` names an existing file, and
   a `#anchor` on a markdown target names a heading of that file (GitHub
   slugs: lower case, punctuation dropped, spaces to `-`).

An `xref-check: allow <word>` comment on a line allows that word on purpose.

Exit status: 0 when every reference resolves, 1 otherwise, 2 on a usage
error. Each broken reference prints as `file:line: no skill godot-x` or
`file:line: broken link target`.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]

# godot-* names of projects outside this catalog.
EXTERNAL = {
    "godot-cpp", "godot-docs", "godot-headers", "godot-ios-plugins", "godot-proposals",
    "godot-rust", "godot-wfc", "godot-xr-tools",
}

NAME = re.compile(r"(?<![\w/.#-])godot-[a-z0-9]+(?:-[a-z0-9]+)*(?![\w/-]|\.\w)")
URL = re.compile(r"https?://\S+")
LINK = re.compile(r"\[[^\]]*\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)")
HEADING = re.compile(r"^#{1,6}\s+(.*?)\s*#*\s*$")
FENCE = re.compile(r"^\s*(`{3,}|~{3,})")
ALLOW = re.compile(r"xref-check:\s*allow\s+([\w@.,\s-]+)")


def slug(heading: str) -> str:
    """The GitHub anchor of a markdown heading."""
    s = re.sub(r"`|\*\*|__", "", heading).strip().lower()
    s = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", s)
    s = re.sub(r"[^\w\- ]", "", s)
    return s.replace(" ", "-")


_anchors: dict[Path, set[str]] = {}


def anchors(path: Path) -> set[str]:
    if path not in _anchors:
        out: set[str] = set()
        counts: dict[str, int] = {}
        fence = None
        for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
            m = FENCE.match(line)
            if m:
                fence = None if fence and m[1][0] == fence else (fence or m[1][0])
                continue
            if fence:
                continue
            h = HEADING.match(line)
            if h:
                s = slug(h[1])
                n = counts.get(s, 0)
                counts[s] = n + 1
                out.add(s if n == 0 else f"{s}-{n}")
        _anchors[path] = out
    return _anchors[path]


def check_file(path: Path, skills: Path, teammates: Path) -> list[str]:
    found = []
    for n, line in enumerate(path.read_text(encoding="utf-8", errors="replace").splitlines(), 1):
        allow = set()
        for m in ALLOW.finditer(line):
            allow.update(x for x in re.split(r"[\s,]+", m[1]) if x)
        bare = URL.sub(" ", line)
        for m in NAME.finditer(bare):
            name = m[0]
            if name in allow or name in EXTERNAL:
                continue
            if (skills / name).is_dir() or (teammates / f"{name}.md").is_file():
                continue
            found.append(f"{path}:{n}: no skill {name}")
        if path.suffix != ".md":
            continue
        for m in LINK.finditer(line):
            target = m[1].strip("<>")
            if re.match(r"[a-z][a-z0-9+.-]*:", target) or target in allow:
                continue
            file_part, _, anchor = target.partition("#")
            dest = (path.parent / file_part).resolve() if file_part else path.resolve()
            if not dest.exists():
                found.append(f"{path}:{n}: broken link {target}")
            elif anchor and dest.suffix == ".md" and anchor.lower() not in anchors(dest):
                found.append(f"{path}:{n}: broken anchor {target}")
    return found


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--skills", type=Path, default=REPO / "skills", help="the skills directory")
    ap.add_argument("--teammates", type=Path, default=REPO / "teammates", help="the teammates directory")
    ap.add_argument("dirs", nargs="*", type=Path, help="skill directories (default: every godot-* skill)")
    args = ap.parse_args(argv)
    if not args.skills.is_dir():
        print(f"xref_check.py: no skills directory {args.skills}", file=sys.stderr)
        return 2
    dirs = args.dirs or sorted(d for d in args.skills.glob("godot-*") if d.is_dir())
    files = [f for d in dirs for f in sorted(d.rglob("*")) if f.is_file() and f.suffix in (".md", ".gd", ".txt", ".json")]
    found = [x for f in files for x in check_file(f, args.skills, args.teammates)]
    for x in found:
        print(x)
    print(f"xref_check: {len(dirs)} skills, {len(files)} files, {len(found)} broken references", file=sys.stderr)
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main())
