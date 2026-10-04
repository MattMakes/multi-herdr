#!/usr/bin/env python3
"""Check that every `Class.member` an engine class is used with exists in Godot.

Usage:
    scripts/godot/api_check.py [--doctool DIR] <paths...>

Scans `.gd` files and the ```gdscript / ```gd / ```csharp / ```cs code blocks
of `.md` files (a directory is searched for both). For each `Class.name` where
`Class` is an engine class, a `Variant` built-in (Array, String, Vector2, ...)
or a global enum (Key, Error, ...), `name` must be a method, property,
setter, getter, constant, enum, signal or theme item of that class or one of
its ancestors. C# names are matched in snake_case (`IsActionPressed` ->
`is_action_pressed`, `Vector2.Zero` -> `ZERO`).

The engine API comes from a `--doctool` dump (`Godot --headless --doctool
DIR`). Without `--doctool` the script reuses or makes the dump in
`.worktrees/_scratch/godot-doctool-<version>/`, finding Godot through
`GODOT_PATH`, `godot` on PATH, or the macOS app bundle. With no Godot it
prints "skipped: no Godot" and exits 0.

A `api-check: allow <Name>` comment on a line allows that name on purpose
(`<Name>` is `Class.member` or `member`), for example in a "this does not
exist" example.

Exit status: 0 when every name is known, 1 when any is unknown, 2 on a usage
error. Each unknown name prints as `file:line: unknown Class.member`.
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
import xml.etree.ElementTree as ET
from dataclasses import dataclass, field
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
MAC_GODOT = "/Applications/Godot.app/Contents/MacOS/Godot"

GD_LANGS = {"gdscript", "gd"}
CS_LANGS = {"csharp", "cs", "c#"}

# Names every script class answers to that the dump does not list.
GD_ALWAYS = {"new"}
# C# bindings: generated nested classes and helpers with no doctool entry.
CS_ALWAYS = {
    "SignalName", "PropertyName", "MethodName", "From", "Create", "CreateFrom",
    "Singleton", "IsInstanceValid",
}

FENCE = re.compile(r"^(\s*)(`{3,}|~{3,})\s*([^\s`]*)")
USE = re.compile(r"(?<![\w.$%@])([A-Z@][A-Za-z0-9_]*)\.([A-Za-z_][A-Za-z0-9_]*)")
ALLOW = re.compile(r"api-check:\s*allow\s+([A-Za-z0-9_@.,\s]+)")


@dataclass
class EngineClass:
    name: str
    inherits: str | None = None
    names: set[str] = field(default_factory=set)
    # True for a global enum (Key, MouseButton): it has no class XML.
    enum_only: bool = True


class Api:
    def __init__(self, classes: dict[str, EngineClass]):
        self.classes = classes

    @classmethod
    def load(cls, doctool: Path) -> "Api":
        classes: dict[str, EngineClass] = {}

        def get(name: str) -> EngineClass:
            return classes.setdefault(name, EngineClass(name))

        for xml in sorted(doctool.rglob("*.xml")):
            root = ET.parse(xml).getroot()
            if root.tag != "class":
                continue
            c = get(root.get("name"))
            c.inherits = root.get("inherits")
            c.enum_only = False
            for el in root.iter():
                name = el.get("name")
                if el.tag in ("method", "signal", "theme_item", "annotation") and name:
                    c.names.add(name)
                elif el.tag == "member" and name:
                    c.names.add(name)
                    for accessor in (el.get("setter"), el.get("getter")):
                        if accessor:
                            c.names.add(accessor)
                elif el.tag == "constant" and name:
                    c.names.add(name)
                    enum = el.get("enum")
                    if enum:
                        owner, _, enum_name = enum.rpartition(".")
                        if root.get("name") == "@GlobalScope":
                            # Global enums: `Key.KEY_A`; `Variant.Type` (owner Variant).
                            if owner:
                                get(owner).names.add(enum_name)
                            else:
                                get(enum_name).names.add(name)
                        else:
                            c.names.add(enum_name)
        if not classes:
            raise SystemExit(f"api_check.py: no class XML under {doctool}")
        return cls(classes)

    def knows_cs_enum(self, cls_name: str, member: str) -> bool:
        """C# drops the prefix of a global enum value: `Key.Space` is KEY_SPACE."""
        c = self.classes.get(cls_name)
        if c is None or not c.enum_only:
            return False
        tail = "_" + snake(member).upper()
        return any(n.endswith(tail) for n in c.names)

    def knows(self, cls_name: str, member: str) -> bool:
        seen = set()
        c = self.classes.get(cls_name)
        while c is not None and c.name not in seen:
            seen.add(c.name)
            if member in c.names:
                return True
            c = self.classes.get(c.inherits) if c.inherits else None
        return False


def snake(name: str) -> str:
    s = re.sub(r"([A-Z]+)([A-Z][a-z])", r"\1_\2", name)
    s = re.sub(r"([a-z0-9])([A-Z])", r"\1_\2", s)
    return s.lower()


def cs_class(name: str) -> str:
    return {"GodotObject": "Object"}.get(name, name)


def candidates(member: str, lang: str) -> list[str]:
    if lang != "cs":
        return [member]
    s = snake(member)
    out = [member, s, s.upper()]
    if member.endswith("Enum"):
        out.append(member[: -len("Enum")])
    return out


@dataclass
class Unknown:
    path: Path
    line: int
    name: str


def code_lines(path: Path):
    """Yield (line number, text, lang) for every checked code line in a file."""
    text = path.read_text(encoding="utf-8", errors="replace")
    if path.suffix == ".gd":
        for n, line in enumerate(text.splitlines(), 1):
            yield n, line, "gd"
        return
    if path.suffix == ".cs":
        for n, line in enumerate(text.splitlines(), 1):
            yield n, line, "cs"
        return
    fence = None
    lang = None
    for n, line in enumerate(text.splitlines(), 1):
        m = FENCE.match(line)
        if fence is None:
            if m:
                fence = m[2]
                tag = m[3].lower()
                lang = "gd" if tag in GD_LANGS else "cs" if tag in CS_LANGS else None
            continue
        if m and m[2].startswith(fence[0]) and len(m[2]) >= len(fence) and not m[3]:
            fence = None
            continue
        if lang:
            yield n, line, lang


def allowed_names(line: str) -> set[str]:
    out = set()
    for m in ALLOW.finditer(line):
        out.update(x for x in re.split(r"[\s,]+", m[1]) if x)
    return out


def check_file(api: Api, path: Path) -> list[Unknown]:
    unknown = []
    for n, line, lang in code_lines(path):
        allow = allowed_names(line)
        for m in USE.finditer(line):
            cls_name, member = m[1], m[2]
            if lang == "cs":
                cls_name = cs_class(cls_name)
                if member in CS_ALWAYS:
                    continue
            elif member in GD_ALWAYS:
                continue
            if cls_name not in api.classes:
                continue
            full = f"{m[1]}.{member}"
            if full in allow or member in allow:
                continue
            if any(api.knows(cls_name, c) for c in candidates(member, lang)):
                continue
            if lang == "cs" and api.knows_cs_enum(cls_name, member):
                continue
            unknown.append(Unknown(path, n, full))
    return unknown


def files_under(paths: list[Path]) -> list[Path]:
    out = []
    for p in paths:
        if p.is_dir():
            out.extend(sorted(x for x in p.rglob("*") if x.suffix in (".md", ".gd") and x.is_file()))
        elif p.is_file():
            out.append(p)
        else:
            raise SystemExit(f"api_check.py: no such file or directory: {p}")
    return out


def find_godot() -> str | None:
    for candidate in (os.environ.get("GODOT_PATH"), shutil.which("godot"), MAC_GODOT):
        if candidate and Path(candidate).is_file() and os.access(candidate, os.X_OK):
            return candidate
    return None


def godot_version(godot: str) -> str:
    out = subprocess.run([godot, "--headless", "--version"], capture_output=True, text=True, check=True)
    line = out.stdout.strip().splitlines()[-1]
    return ".".join(x for x in line.split(".")[:3] if x.isdigit())


def ensure_doctool(godot: str) -> Path:
    """Reuse or make the dump in .worktrees/_scratch/godot-doctool-<version>/."""
    dump = REPO / ".worktrees" / "_scratch" / f"godot-doctool-{godot_version(godot)}"
    if not any(dump.glob("doc/classes/*.xml")):
        dump.mkdir(parents=True, exist_ok=True)
        subprocess.run([godot, "--headless", "--doctool", "."], cwd=dump, capture_output=True, check=True)
    return dump


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--doctool", type=Path, help="a `Godot --doctool` dump directory")
    ap.add_argument("paths", nargs="+", type=Path)
    args = ap.parse_args(argv)

    doctool = args.doctool
    if doctool is None:
        godot = find_godot()
        if godot is None:
            print("skipped: no Godot")
            return 0
        doctool = ensure_doctool(godot)
    if not doctool.is_dir():
        print(f"api_check.py: no doctool dump at {doctool}", file=sys.stderr)
        return 2
    api = Api.load(doctool)

    files = files_under(args.paths)
    unknown = [u for f in files for u in check_file(api, f)]
    for u in unknown:
        print(f"{u.path}:{u.line}: unknown {u.name}")
    print(f"api_check: {len(files)} files, {len(unknown)} unknown names", file=sys.stderr)
    return 1 if unknown else 0


if __name__ == "__main__":
    sys.exit(main())
