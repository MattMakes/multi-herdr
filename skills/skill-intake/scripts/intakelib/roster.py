"""Read and edit the frontmatter of a `teammates/*.md` file (no YAML library).

The roster writes every list on 1 line in flow style (`skills: [a, b]`) or as
a block of `  - item` lines. `skills_when` is a flow map whose values are flow
lists. Only these forms are read; anything else is left as it is.
"""

from __future__ import annotations

import re
from pathlib import Path

_KEY = re.compile(r"^([A-Za-z0-9_-]+):[ \t]*(.*)$")
_ITEM = re.compile(r"^\s+-\s+(.*)$")


def unquote(v: str) -> str:
    v = v.strip()
    if len(v) >= 2 and v[0] == v[-1] and v[0] in "\"'":
        return v[1:-1]
    return v


def bounds(lines: list[str]) -> tuple[int, int] | None:
    """(first, end) line indexes of the frontmatter body, or None."""
    if not lines or lines[0].rstrip("\n").strip() != "---":
        return None
    for i in range(1, len(lines)):
        if lines[i].rstrip("\n").strip() == "---":
            return 1, i
    return None


def flow(value: str) -> list[str] | None:
    """`[a, "b"]` -> ["a", "b"]; None when `value` is not a 1-line flow list."""
    v = value.split(" #", 1)[0].strip()
    if not (v.startswith("[") and v.endswith("]")):
        return None
    inner = v[1:-1].strip()
    return [unquote(x) for x in inner.split(",") if x.strip()] if inner else []


def read(path: Path) -> dict:
    """{key: raw value} for top-level keys, with a block list read as `[a, b]` text."""
    lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    b = bounds(lines)
    if b is None:
        return {}
    out: dict[str, str] = {}
    i = b[0]
    while i < b[1]:
        m = _KEY.match(lines[i])
        i += 1
        if not m:
            continue
        key, value = m.group(1), m.group(2).strip()
        if not value:
            items = []
            while i < b[1] and _ITEM.match(lines[i]):
                items.append(unquote(_ITEM.match(lines[i]).group(1)))
                i += 1
            if items:
                value = "[" + ", ".join(items) + "]"
        out.setdefault(key, value)
    return out


def skill_names(fm: dict) -> set[str]:
    """Every skill a teammate names in `skills`, `available_skills` or `skills_when`."""
    names: set[str] = set()
    for key in ("skills", "available_skills"):
        names.update(flow(fm.get(key, "")) or [])
    for group in re.findall(r"\[[^\]]*\]", fm.get("skills_when", "")):
        names.update(flow(group) or [])
    return names


def visible(teammates_dir: Path) -> list[tuple[str, dict]]:
    """(name, frontmatter) of every teammate the roster offers: no `_` file, not hidden."""
    out = []
    for p in sorted(teammates_dir.glob("*.md")):
        if p.name.startswith("_") or p.name == "README.md" or p.is_symlink():
            continue
        fm = read(p)
        if not fm or fm.get("hidden", "false").strip().lower() == "true":
            continue
        out.append((p.stem, fm))
    return out


def _is_sorted(items: list[str]) -> bool:
    return items == sorted(items)


def add_to_list(text: str, key: str, names: list[str], after: str = "skills") -> str:
    """Add `names` to the frontmatter list `key`, keeping its style: flow or block, and
    sorted or in order (a sorted list stays sorted; any other list gets them at the end).
    A missing key gets a flow line after `after` (or before the closing `---`)."""
    lines = text.splitlines(keepends=True)
    b = bounds(lines)
    if b is None:
        raise ValueError("no frontmatter")
    for i in range(b[0], b[1]):
        m = _KEY.match(lines[i].rstrip("\n"))
        if not m or m.group(1) != key:
            continue
        value = m.group(2).strip()
        items = flow(value)
        if items is not None:
            new = _merge(items, names)
            lines[i] = f"{key}: [{', '.join(new)}]\n"
            return "".join(lines)
        if value:
            raise ValueError(f"{key} is not a list this tool can edit")
        j = i + 1
        while j < b[1] and _ITEM.match(lines[j].rstrip("\n")):
            j += 1
        block = [unquote(_ITEM.match(ln.rstrip("\n")).group(1)) for ln in lines[i + 1:j]]
        indent = re.match(r"^(\s+)-", lines[i + 1]).group(1) if j > i + 1 else "  "
        new = _merge(block, names)
        lines[i + 1:j] = [f"{indent}- {x}\n" for x in new]
        return "".join(lines)
    at = b[1]
    for i in range(b[0], b[1]):
        m = _KEY.match(lines[i].rstrip("\n"))
        if m and m.group(1) == after:
            at = i + 1
            while at < b[1] and _ITEM.match(lines[at].rstrip("\n")):
                at += 1
            break
    lines.insert(at, f"{key}: [{', '.join(sorted(set(names)))}]\n")
    return "".join(lines)


def _merge(items: list[str], names: list[str]) -> list[str]:
    add = [n for n in sorted(set(names)) if n not in items]
    return sorted(items + add) if _is_sorted(items) else items + add
