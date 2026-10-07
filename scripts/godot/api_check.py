#!/usr/bin/env python3
"""Check that every `Class.member` an engine class is used with exists in Godot.

Usage:
    scripts/godot/api_check.py [--doctool DIR] [--strict-own COPIED] <paths...>
    scripts/godot/api_check.py --pin-deprecations

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

In GDScript a bare class name must exist too: after `extends`, in a type
annotation (`var x: Foo`, a `func` parameter `a: Foo`, `for x: Foo in`),
after `->`, in `Foo.new()`, `is Foo`, `as Foo` and `Array[Foo]`. A name the
same skill defines (`class_name`, an inner `class`, an `enum`, a `const`)
is allowed: the bundle is the directory that holds `SKILL.md`, or the file.
`scripts/godot/known_classes.txt` lists addon and example project classes,
each allowed in the skills it names.

Deprecated engine names are reported as `file:line: deprecated Name (message)`:
a `Class.member` use, a bare deprecated class, and a call `.some_method(` on
any value when every class with that method marks it deprecated (only names
with an underscore: `.resolve(` on a project object is not an engine call).
The `--doctool` dump from a release binary has no deprecation marks, so the
marks for each Godot version are committed next to this script as
`deprecated-<version>.json` (`{"Class": {"": class message, "member":
message}}`), and every host gets the same answer. Without `--doctool` the
script uses the file for the version of the Godot it finds. A version with no
file fails the run (exit 2); `--pin-deprecations` writes the file: a headless
editor run, with its home in `.worktrees/_scratch`, writes the editor doc
cache (`editor_doc_cache-<x.y>.res`, from the class reference built into the
editor), and a script reads the marks out of it. With `--doctool` the marks
come from `deprecated="..."` in the dump XML and from `deprecated.json` next
to it; a dump with no marks fails the run (exit 2).

With `--strict-own skills/copied.json`, a deprecated name in a file that
copied.json lists as copied does not print and does not fail the run by
itself: the run counts these names per skill and fails when a count is above
`scripts/godot/copied-baseline.json` (the ratchet of
`gdscript_blocks_check.py`, see `copied_ratchet.py`).
Unknown names fail in every file.

A `api-check: allow <Name>` comment on a line allows that name on purpose
(`<Name>` is `Class.member`, `member` or a class), for example in a "this
does not exist" example. A line that holds only that comment allows the
names for the rest of its code block (a `.gd` file: the rest of the file).

Exit status: 0 when every name is known and no own text uses a deprecated
name, 1 otherwise, 2 on a usage error. Each unknown name prints as
`file:line: unknown Class.member` (or `unknown class Foo`).
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET
from dataclasses import dataclass, field
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(Path(__file__).resolve().parent))
import copied_ratchet  # noqa: E402
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
ALLOW_LINE = re.compile(r"^\s*(#+|//)\s*api-check:\s*allow\b")
KNOWN_CLASSES = Path(__file__).resolve().parent / "known_classes.txt"

# GDScript type names that have no class XML.
GD_TYPES = {"void", "Variant", "Self"}
# Places where a GDScript line names a class.
CLASS_USES = [
    re.compile(r"^\s*(?:@\w+(?:\([^)]*\))?\s+)*(?:class_name\s+\w+\s+)?extends\s+([A-Za-z_]\w*)"),
    re.compile(r"^\s*class\s+\w+\s+extends\s+([A-Za-z_]\w*)"),
    re.compile(r"\b(?:var|const)\s+\w+\s*:\s*([A-Z]\w*)"),
    re.compile(r"\bfor\s+\w+\s*:\s*([A-Z]\w*)"),
    re.compile(r"->\s*([A-Z]\w*)"),
    re.compile(r"(?<![\w.])([A-Z]\w*)\.new\s*\("),
    re.compile(r"\b(?:is|as)\s+(?:not\s+)?([A-Z]\w*)"),
    re.compile(r"\b(?:Array|Dictionary)\s*\[\s*([A-Z]\w*)"),
    re.compile(r"\bDictionary\s*\[\s*\w+\s*,\s*([A-Z]\w*)"),
]
FUNC_SIG = re.compile(r"^\s*(?:static\s+)?func\s+\w*\s*\((.*)")
PARAM_TYPE = re.compile(r"(?:^|,)\s*\w+\s*:\s*([A-Z]\w*)")
DEFINES = [
    re.compile(r"\bclass_name\s+([A-Za-z_]\w*)"),
    re.compile(r"^\s*class\s+([A-Za-z_]\w*)"),
    re.compile(r"^\s*enum\s+([A-Za-z_]\w*)"),
    re.compile(r"^\s*(?:static\s+)?const\s+([A-Za-z_]\w*)"),
]
STRING = re.compile(r'"(?:\\.|[^"\\])*"|\'(?:\\.|[^\'\\])*\'')


@dataclass
class EngineClass:
    name: str
    inherits: str | None = None
    names: set[str] = field(default_factory=set)
    # True for a global enum (Key, MouseButton): it has no class XML.
    enum_only: bool = True


class Api:
    def __init__(self, classes: dict[str, EngineClass], deprecated: dict[str, dict[str, str]] | None = None):
        self.classes = classes
        # deprecated[Class][member] = message; member "" is the class itself.
        self.deprecated = deprecated or {}

    @classmethod
    def load(cls, doctool: Path, sidecar: bool = True) -> "Api":
        classes: dict[str, EngineClass] = {}

        def get(name: str) -> EngineClass:
            return classes.setdefault(name, EngineClass(name))

        deprecated: dict[str, dict[str, str]] = {}
        for xml in sorted(doctool.rglob("*.xml")):
            root = ET.parse(xml).getroot()
            if root.tag != "class":
                continue
            c = get(root.get("name"))
            for el in root.iter():
                msg = el.get("deprecated")
                if msg is not None:
                    member = "" if el is root else el.get("name", "")
                    deprecated.setdefault(c.name, {})[member] = msg
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
        api = cls(classes, deprecated)
        if sidecar:
            api.add_deprecations(doctool / "deprecated.json")
        return api

    def add_deprecations(self, path: Path) -> None:
        """Merge a `{"Class": {"member": message}}` file, when there is one."""
        if path.is_file():
            for cname, members in json.loads(path.read_text()).items():
                self.deprecated.setdefault(cname, {}).update(members)

    def deprecation(self, cls_name: str, member: str) -> str | None:
        """The message when `member` is deprecated where `cls_name` gets it."""
        seen = set()
        c = self.classes.get(cls_name)
        while c is not None and c.name not in seen:
            seen.add(c.name)
            if member in c.names:
                return self.deprecated.get(c.name, {}).get(member)
            c = self.classes.get(c.inherits) if c.inherits else None
        return None

    def deprecated_everywhere(self) -> dict[str, tuple[str, str]]:
        """member -> (Class, message) for members deprecated in every class that has them."""
        owners: dict[str, list[str]] = {}
        for c in self.classes.values():
            for n in c.names:
                owners.setdefault(n, []).append(c.name)
        out = {}
        for cname, members in self.deprecated.items():
            for m, msg in members.items():
                if m and all(m in self.deprecated.get(o, {}) for o in owners.get(m, [cname])):
                    out[m] = (cname, msg)
        return out

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
    # "unknown" or "deprecated"
    kind: str = "unknown"
    message: str = ""

    def text(self) -> str:
        if self.kind == "deprecated":
            msg = " ".join(self.message.split())
            return f"{self.path}:{self.line}: deprecated {self.name}" + (f" ({msg})" if msg else "")
        return f"{self.path}:{self.line}: unknown {self.name}"


def code_lines(path: Path):
    """Yield (line number, text, lang, block number) for every checked code line in a file."""
    text = path.read_text(encoding="utf-8", errors="replace")
    if path.suffix in (".gd", ".cs"):
        lang = path.suffix[1:]
        for n, line in enumerate(text.splitlines(), 1):
            yield n, line, lang, 0
        return
    fence = None
    lang = None
    block = 0
    for n, line in enumerate(text.splitlines(), 1):
        m = FENCE.match(line)
        if fence is None:
            if m:
                fence = m[2]
                tag = m[3].lower()
                lang = "gd" if tag in GD_LANGS else "cs" if tag in CS_LANGS else None
                block += 1
            continue
        if m and m[2].startswith(fence[0]) and len(m[2]) >= len(fence) and not m[3]:
            fence = None
            continue
        if lang:
            yield n, line, lang, block


def allowed_names(line: str) -> set[str]:
    out = set()
    for m in ALLOW.finditer(line):
        out.update(x for x in re.split(r"[\s,]+", m[1]) if x)
    return out


def gd_code(line: str) -> str:
    """The line with strings blanked and the comment cut."""
    line = STRING.sub('""', line)
    return line.split("#", 1)[0]


def bundle_of(path: Path) -> Path:
    for d in path.resolve().parents:
        if (d / "SKILL.md").is_file():
            return d
    return path.resolve()


_defined: dict[Path, set[str]] = {}


def defined_names(path: Path) -> set[str]:
    """Class, enum and const names that the file's bundle (its skill) defines."""
    bundle = bundle_of(path)
    if bundle not in _defined:
        files = [bundle] if bundle.is_file() else sorted(
            x for x in bundle.rglob("*") if x.suffix in (".md", ".gd") and x.is_file()
        )
        names = set()
        for f in files:
            for _, line, lang, _ in code_lines(f):
                if lang == "gd":
                    code = gd_code(line)
                    for rx in DEFINES:
                        names.update(rx.findall(code))
        _defined[bundle] = names
    return _defined[bundle]


def load_known_classes(path: Path = KNOWN_CLASSES) -> dict[str, set[str]]:
    """Name -> the skill directory names where known_classes.txt allows it."""
    out: dict[str, set[str]] = {}
    if not path.is_file():
        return out
    for line in path.read_text().splitlines():
        line = line.split("#", 1)[0].strip()
        if not line:
            continue
        name, _, skills = line.partition(":")
        out.setdefault(name.strip(), set()).update(x.strip() for x in skills.split(",") if x.strip())
    return out


def class_uses(code: str):
    """Yield every class name a GDScript line names in a type position."""
    for rx in CLASS_USES:
        yield from rx.findall(code)
    m = FUNC_SIG.match(code)
    if m:
        yield from PARAM_TYPE.findall(m[1].split(")", 1)[0])


def check_file(api: Api, path: Path, known: dict[str, set[str]] | None = None) -> list[Unknown]:
    unknown = []
    known = load_known_classes() if known is None else known
    skill = bundle_of(path).name
    everywhere = api.deprecated_everywhere()
    block_allow: dict[int, set[str]] = {}
    for n, line, lang, block in code_lines(path):
        allow = allowed_names(line)
        if allow and ALLOW_LINE.match(line):
            block_allow.setdefault(block, set()).update(allow)
        allow |= block_allow.get(block, set())
        seen: set[str] = set()

        def deprecated(name: str, msg: str) -> None:
            if name not in seen:
                seen.add(name)
                unknown.append(Unknown(path, n, name, "deprecated", msg))

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
            hit = next((c for c in candidates(member, lang) if api.knows(cls_name, c)), None)
            if hit is None and lang == "cs" and api.knows_cs_enum(cls_name, member):
                continue
            if hit is None:
                unknown.append(Unknown(path, n, full))
                continue
            msg = api.deprecation(cls_name, hit)
            if msg is not None:
                deprecated(f"{cls_name}.{hit}", msg)
        if lang != "gd":
            continue
        code = gd_code(line)
        for m in re.finditer(r"\.([a-z]\w*_\w*)\s*\(", code):
            if m[1] in everywhere and m[1] not in allow:
                cname, msg = everywhere[m[1]]
                deprecated(f"{cname}.{m[1]}", msg)
        for name in class_uses(code):
            if name in allow:
                continue
            if name in api.classes and not api.classes[name].enum_only:
                msg = api.deprecated.get(name, {}).get("")
                if msg is not None:
                    deprecated(name, msg)
            elif (
                name not in api.classes
                and name not in GD_TYPES
                and skill not in known.get(name, ())
                and name not in defined_names(path)
            ):
                unknown.append(Unknown(path, n, f"class {name}"))
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


DOC_CACHE_SCRIPT = """extends SceneTree
func _init():
	var out := {}
	var res = load("res://doc_cache.res")
	for c in res.get_meta("classes"):
		var marks := {}
		if c.get("deprecated") != null:
			marks[""] = c["deprecated"]
		for section in c.keys():
			if typeof(c[section]) != TYPE_ARRAY:
				continue
			for item in c[section]:
				if typeof(item) == TYPE_DICTIONARY and item.get("deprecated") != null and item.has("name"):
					marks[item["name"]] = item["deprecated"]
		if not marks.is_empty():
			out[c["name"]] = marks
	var f := FileAccess.open("res://deprecated.json", FileAccess.WRITE)
	f.store_string(JSON.stringify(out, "\\t", true))
	f.close()
	quit()
"""


def pinned_deprecations(version: str) -> Path:
    return Path(__file__).resolve().parent / f"deprecated-{version}.json"


def pin_deprecations(godot: str, version: str, out: Path) -> int:
    """Write the deprecation marks of `godot` to `out`; return the count of marks.

    A headless editor run writes the doc cache to the user cache directory.
    HOME and the XDG and Windows directories point into a scratch directory,
    so the run does not touch the real ones.
    """
    minor = ".".join(version.split(".")[:2])
    scratch = REPO / ".worktrees" / "_scratch"
    scratch.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="godot-doccache-", dir=scratch) as tmp:
        root = Path(tmp)
        home, project = root / "home", root / "project"
        home.mkdir()
        project.mkdir()
        (project / "project.godot").write_text('[application]\nconfig/name="doc-cache"\n')
        env = dict(os.environ, HOME=str(home), XDG_CACHE_HOME=str(home / "cache"),
                   XDG_DATA_HOME=str(home / "data"), XDG_CONFIG_HOME=str(home / "config"),
                   LOCALAPPDATA=str(home / "local"), APPDATA=str(home / "roaming"))
        subprocess.run(
            [godot, "--headless", "--editor", "--path", str(project), "--quit-after", "1200"],
            env=env, capture_output=True, timeout=600, check=False,
        )
        cache = next(home.rglob(f"editor_doc_cache-{minor}.res"), None)
        if cache is None:
            raise SystemExit(f"api_check.py: the editor wrote no editor_doc_cache-{minor}.res under {home}")
        shutil.copyfile(cache, project / "doc_cache.res")
        (project / "dump.gd").write_text(DOC_CACHE_SCRIPT)
        subprocess.run(
            [godot, "--headless", "--path", str(project), "-s", "dump.gd"],
            env=env, capture_output=True, timeout=300, check=False,
        )
        marks = json.loads((project / "deprecated.json").read_text())
    # One class per line, so a new Godot version diffs by class.
    lines = [
        f"{json.dumps(c)}:{json.dumps(marks[c], sort_keys=True, ensure_ascii=False, separators=(',', ':'))}"
        for c in sorted(marks)
    ]
    out.write_text("{\n" + ",\n".join(lines) + "\n}\n", encoding="utf-8")
    return sum(len(v) for v in marks.values())


def ensure_doctool(godot: str) -> Path:
    """Reuse or make the dump in .worktrees/_scratch/godot-doctool-<version>/."""
    version = godot_version(godot)
    dump = REPO / ".worktrees" / "_scratch" / f"godot-doctool-{version}"
    if not any(dump.glob("doc/classes/*.xml")):
        dump.mkdir(parents=True, exist_ok=True)
        subprocess.run([godot, "--headless", "--doctool", "."], cwd=dump, capture_output=True, check=True)
    return dump


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--doctool", type=Path, help="a `Godot --doctool` dump directory")
    ap.add_argument(
        "--strict-own", type=Path, metavar="COPIED",
        help="count per skill, and fail only above the baseline, deprecated names in files "
        "that COPIED (skills/copied.json) lists as copied",
    )
    ap.add_argument("--baseline", type=Path, default=copied_ratchet.BASELINE, help="the copied-failure baseline")
    ap.add_argument("--update-baseline", action="store_true", help="write the copied counts to --baseline")
    ap.add_argument(
        "--pin-deprecations", action="store_true",
        help="write deprecated-<version>.json for the Godot found, then exit",
    )
    ap.add_argument("paths", nargs="*", type=Path)
    args = ap.parse_args(argv)
    if not args.paths and not args.pin_deprecations:
        ap.error("the following arguments are required: paths")

    doctool = args.doctool
    pinned = None
    if doctool is None:
        godot = find_godot()
        if godot is None:
            print("skipped: no Godot")
            return 0
        version = godot_version(godot)
        pinned = pinned_deprecations(version)
        if args.pin_deprecations:
            n = pin_deprecations(godot, version, pinned)
            print(f"api_check: wrote {n} deprecation marks to {pinned}")
            return 0
        if not pinned.is_file():
            print(
                f"api_check.py: no deprecation data for Godot {version}: run "
                f"scripts/godot/api_check.py --pin-deprecations and commit {pinned.name}",
                file=sys.stderr,
            )
            return 2
        doctool = ensure_doctool(godot)
    if not doctool.is_dir():
        print(f"api_check.py: no doctool dump at {doctool}", file=sys.stderr)
        return 2
    api = Api.load(doctool, sidecar=pinned is None)
    if pinned is not None:
        api.add_deprecations(pinned)
    if not api.deprecated:
        print(f"api_check.py: no deprecation marks in {doctool}", file=sys.stderr)
        return 2
    copied = copied_ratchet.copied_skills(args.strict_own) if args.strict_own else {}

    files = files_under(args.paths)
    known = load_known_classes()
    found = [u for f in files for u in check_file(api, f, known)]
    failing = 0
    up = []
    for u in found:
        if u.kind == "deprecated" and u.path.resolve() in copied:
            up.append((u.path, u.text()))
        else:
            print(u.text())
            failing += 1
    unknown = sum(1 for u in found if u.kind == "unknown")
    print(
        f"api_check: {len(files)} files, {unknown} unknown names, "
        f"{len(found) - unknown} deprecated ({len(up)} copied)",
        file=sys.stderr,
    )
    grew = 0
    if args.strict_own:
        grew = copied_ratchet.apply(
            "api_check", "deprecated names", up, files, copied, args.baseline, args.update_baseline
        )
    return 1 if failing or grew else 0


if __name__ == "__main__":
    sys.exit(main())
