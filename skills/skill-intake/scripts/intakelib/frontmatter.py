"""A small reader for the YAML frontmatter of a SKILL.md (no YAML library).

It reads top-level `key: value` lines between the first two `---` lines. A
value can be quoted, or a `>` / `|` block of indented lines. Anything else is
kept as the raw text after the colon. The result is source text: escape it
before it is printed or stored.
"""

from __future__ import annotations

import re

_KEY = re.compile(r"^([A-Za-z0-9_-]+):[ \t]*(.*)$")


def _unquote(value: str) -> str:
    value = value.strip()
    if len(value) >= 2 and value[0] == value[-1] and value[0] in "\"'":
        return value[1:-1]
    return value


def parse(text: str) -> dict[str, str]:
    lines = text.splitlines()
    if not lines or lines[0].strip() != "---":
        return {}
    out: dict[str, str] = {}
    i = 1
    while i < len(lines):
        line = lines[i]
        if line.strip() == "---":
            break
        m = _KEY.match(line)
        i += 1
        if not m:
            continue
        key, value = m.group(1), m.group(2).strip()
        if value and value[0] in ">|":
            block = []
            while i < len(lines) and (lines[i].startswith((" ", "\t")) or not lines[i].strip()):
                if lines[i].strip() == "---":
                    break
                block.append(lines[i].strip())
                i += 1
            sep = " " if value[0] == ">" else "\n"
            value = sep.join(b for b in block if b)
        else:
            value = _unquote(value)
        out.setdefault(key, value)
    return out


def read(path) -> dict[str, str]:
    """Parse the frontmatter of a file; an unreadable file gives {}."""
    try:
        with open(path, encoding="utf-8", errors="replace") as f:
            return parse(f.read(65536))
    except OSError:
        return {}
