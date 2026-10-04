#!/usr/bin/env python3
"""Parse-check every ```gdscript and ```gd code block in markdown files.

Usage:
    scripts/godot/gdscript_blocks_check.py [--scratch DIR] [--keep] <paths...>

Each block is written to a scratch Godot project and loaded by a headless
`SceneTree` script. Godot's own parse errors decide the result: `load()`
returns a script even when it has a parse error, and `--check-only` exits 0
on errors, so neither is trusted.

A block can be a whole script or a fragment. Each block gets up to 3 tries,
and the first one that parses passes the block:
  1. the block as a whole script;
  2. if it has no top-level `extends`: `extends Node` + the block;
  3. `extends Node` + `func _f():` + the block, indented (a statement list).
A failing block is reported with the error of try 1, at the markdown line
that the error points to.

`class_name` types are registered for every block, so a block can use a class
that another block declares. When 2 blocks declare the same `class_name`,
the first one registers it and the other is checked without its
`class_name`.

With `--strict-own skills/provenance.json`, a failing block in a file that
provenance.json lists as copied from upstream is reported with
"(upstream, report only)" and does not fail the run. Blocks in own files
(not a `sources` path) still fail it.

A `<!-- gdscript-check: skip -->` line just before the opening fence (blank
lines between are allowed) skips the block.

Godot is found through `GODOT_PATH`, `godot` on PATH, or the macOS app
bundle. With no Godot the script prints "skipped: no Godot" and exits 0.
Exit status: 0 when every block parses, 1 when any (strict) block fails, 2 on a usage error.
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from api_check import REPO, find_godot  # noqa: E402

LANGS = {"gdscript", "gd"}
FENCE = re.compile(r"^(\s*)(`{3,}|~{3,})\s*([^\s`]*)")
SKIP = re.compile(r"^\s*<!--\s*gdscript-check:\s*skip\s*-->\s*$")
CLASS_NAME = re.compile(r"^class_name\s+([A-Za-z_]\w*)\s*")
EXTENDS = re.compile(r"^(?:class_name\s+\w+\s+)?extends\s+([\w.\"/:]+)", re.MULTILINE)
# Bases tried for a fragment, in order. A fragment parses if it parses on any.
BASES = ["Node", "Node2D", "Node3D", "Control", "CharacterBody2D", "CharacterBody3D", "Resource"]
FRAGMENT_KIND = "in a func of Node"
DEFAULT_SCRATCH = REPO / ".worktrees" / "_scratch" / "godot-blockcheck"

CHECKER = """extends SceneTree

func _init() -> void:
\tvar f := FileAccess.open("res://list.txt", FileAccess.READ)
\twhile not f.eof_reached():
\t\tvar p := f.get_line().strip_edges()
\t\tif p.is_empty():
\t\t\tcontinue
\t\tprinterr("@@BEGIN ", p)
\t\tResourceLoader.load(p, "", ResourceLoader.CACHE_MODE_IGNORE)
\t\tprinterr("@@END ", p)
\tquit(0)
"""


@dataclass
class Try:
    kind: str  # "whole", "extends <Base>" or "in a func of <Base>"
    source: str
    offset: int  # lines added before the block


@dataclass
class Block:
    path: Path
    line: int  # markdown line of the first code line
    code: str
    index: int = 0
    class_name: str | None = None
    errors: dict[str, str] = field(default_factory=dict)  # try kind -> error
    passed: str | None = None  # the kind of the try that parsed


def extract(path: Path) -> list[Block]:
    blocks = []
    lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    fence = None
    take = False
    start = 0
    body: list[str] = []
    for n, line in enumerate(lines, 1):
        m = FENCE.match(line)
        if fence is None:
            if m:
                fence = m[2]
                take = m[3].lower() in LANGS and not skipped(lines, n - 1)
                start = n + 1
                body = []
            continue
        if m and m[2][0] == fence[0] and len(m[2]) >= len(fence) and not m[3]:
            if take:
                blocks.append(Block(path, start, "\n".join(body) + "\n"))
            fence = None
            continue
        body.append(line)
    return blocks


def skipped(lines: list[str], fence_index: int) -> bool:
    i = fence_index - 1
    while i >= 0 and not lines[i].strip():
        i -= 1
    return i >= 0 and bool(SKIP.match(lines[i]))


def has_extends(code: str) -> bool:
    return bool(EXTENDS.search(code))


def tries(block: Block, owns_class: bool) -> list[Try]:
    """Return the tries for a block, in order."""
    code = block.code
    if block.class_name and not owns_class:
        # Another block registers this class_name; keep the line count.
        code = "\n".join(
            CLASS_NAME.sub(lambda m: "", ln, count=1) or f"# class_name {block.class_name}"
            if CLASS_NAME.match(ln) else ln
            for ln in code.split("\n")
        )
    out = [Try("whole", code, 0)]
    if not has_extends(code):
        out += [Try(f"extends {base}", f"extends {base}\n" + code, 1) for base in BASES]
    indent = "\t" if re.search(r"^\t", code, re.MULTILINE) or not re.search(r"^ ", code, re.MULTILINE) else "    "
    body = "".join(f"{indent}{ln}\n" if ln.strip() else "\n" for ln in code.split("\n"))
    out += [
        Try(f"in a func of {base}", f"extends {base}\nfunc _f() -> void:\n" + body + f"{indent}pass\n", 2)
        for base in BASES
    ]
    return out


def class_cache(entries: list[tuple[str, str, str]]) -> str:
    items = []
    for name, base, res in entries:
        items.append(
            '{\n"base": &"%s",\n"class": &"%s",\n"icon": "",\n"is_abstract": false,\n'
            '"is_tool": false,\n"language": &"GDScript",\n"path": "%s"\n}' % (base, name, res)
        )
    return "list=[" + ", ".join(items) + "]\n"


def run_godot(godot: str, project: Path, files: list[str], timeout: int) -> dict[str, str]:
    """Load each res:// file in one Godot run; return {file: first error} for failures."""
    (project / "list.txt").write_text("\n".join(files) + "\n")
    proc = subprocess.run(
        [godot, "--headless", "--path", str(project), "--script", "res://check.gd"],
        capture_output=True, text=True, timeout=timeout,
    )
    errors: dict[str, str] = {}
    current = None
    located = False
    seen_end = set()
    for line in proc.stderr.splitlines():
        if line.startswith("@@BEGIN "):
            current, located = line[len("@@BEGIN "):], False
        elif line.startswith("@@END "):
            seen_end.add(line[len("@@END "):])
            current = None
        elif current is None:
            continue
        elif current not in errors and line.startswith(("SCRIPT ERROR:", "ERROR:")):
            errors[current] = line.split(":", 1)[1].strip()
        elif current in errors and not located:
            # "   at: GDScript::reload (res://blocks/b00001_1.gd:4)"
            m = re.search(r"\((res://[^:)]+):(\d+)\)", line)
            if m and m[1] == current:
                errors[current] += f" @line {m[2]}"
                located = True
    for f in files:
        if f not in seen_end:
            errors.setdefault(f, f"Godot did not finish loading the file (exit {proc.returncode})")
    return errors


def check(blocks: list[Block], godot: str, project: Path, timeout: int) -> None:
    project.mkdir(parents=True, exist_ok=True)
    (project / "project.godot").write_text('[application]\nconfig/name="gdscript-blocks-check"\n')
    (project / "check.gd").write_text(CHECKER)
    blocks_dir = project / "blocks"
    if blocks_dir.exists():
        shutil.rmtree(blocks_dir)
    blocks_dir.mkdir()
    (project / ".godot").mkdir(exist_ok=True)

    owners: dict[str, Block] = {}
    registry = []
    for i, b in enumerate(blocks):
        b.index = i
        m = next((m for ln in b.code.split("\n") if (m := CLASS_NAME.match(ln))), None)
        if m:
            b.class_name = m[1]
            if m[1] not in owners:
                owners[m[1]] = b
                ext = EXTENDS.search(b.code)
                base = ext[1] if ext and re.fullmatch(r"\w+", ext[1]) else "RefCounted"
                registry.append((m[1], base, f"res://blocks/b{i:05d}_0.gd"))
    (project / ".godot" / "global_script_class_cache.cfg").write_text(class_cache(registry))

    plans = {b.index: tries(b, owners.get(b.class_name) is b) for b in blocks}
    # Round k loads try k of every block that has not parsed yet.
    for k in range(max(len(p) for p in plans.values())):
        files = {}
        for b in blocks:
            if b.passed is None and k < len(plans[b.index]):
                t = plans[b.index][k]
                res = f"res://blocks/b{b.index:05d}_{k}.gd"
                (project / res[len("res://"):]).write_text(t.source)
                files[res] = (b, t)
        if not files:
            break
        errors = run_godot(godot, project, sorted(files), timeout)
        for res, (b, t) in files.items():
            if res in errors:
                b.errors[t.kind] = shift(errors[res], t.offset)
            else:
                b.passed = t.kind


def shift(msg: str, offset: int) -> str:
    """Make the " @line N" of an error count from the first line of the block."""
    return re.sub(r" @line (\d+)$", lambda m: f" @line {int(m[1]) - offset}", msg)


def report_line(b: Block) -> tuple[int, str]:
    # A statement list fails as a whole script with "Unexpected ... in class
    # body"; the error inside a function of Node says more.
    msg = b.errors.get("whole", "")
    statements = not has_extends(b.code) and not re.search(r"^(static\s+)?func\s", b.code, re.MULTILINE)
    if statements and "in class body" in msg and FRAGMENT_KIND in b.errors:
        msg = b.errors[FRAGMENT_KIND]
    m = re.search(r" @line (-?\d+)$", msg)
    if m:
        return b.line + max(int(m[1]), 1) - 1, msg[: m.start()]
    return b.line, msg


def copied_files(provenance: Path) -> set[Path]:
    """Files that provenance.json lists as copied from upstream.

    A source path `skills/<upstream name>/<rel>` is the file
    `<provenance dir>/<skill>/<rel>`. Other sources (the LICENSE) are skipped.
    Our edits to a copied file (an API fix, a "Fleet additions" list) do not
    make it own text; a reference that is not a source is own text.
    """
    root = provenance.resolve().parent
    out = set()
    for entry in json.loads(provenance.read_text())["skills"]:
        for src in entry.get("sources", []):
            parts = src["path"].split("/")
            if len(parts) >= 3 and parts[0] == "skills":
                out.add(root / entry["name"] / "/".join(parts[2:]))
    return out


def files_under(paths: list[Path]) -> list[Path]:
    out = []
    for p in paths:
        if p.is_dir():
            out.extend(sorted(x for x in p.rglob("*.md") if x.is_file()))
        elif p.is_file():
            out.append(p)
        else:
            raise SystemExit(f"gdscript_blocks_check.py: no such file or directory: {p}")
    return out


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--scratch", type=Path, help=f"parent of the scratch project (default {DEFAULT_SCRATCH})")
    ap.add_argument("--keep", action="store_true", help="keep the scratch project")
    ap.add_argument("--timeout", type=int, default=600, help="seconds per Godot run")
    ap.add_argument(
        "--strict-own", type=Path, metavar="PROVENANCE",
        help="report, but do not fail on, blocks in files that PROVENANCE (skills/provenance.json) "
        "lists as copied from upstream",
    )
    ap.add_argument("paths", nargs="+", type=Path)
    args = ap.parse_args(argv)
    upstream = copied_files(args.strict_own) if args.strict_own else set()

    godot = find_godot()
    if godot is None:
        print("skipped: no Godot")
        return 0
    files = files_under(args.paths)
    blocks = [b for f in files for b in extract(f)]
    if not blocks:
        print(f"gdscript_blocks_check: {len(files)} files, 0 blocks", file=sys.stderr)
        return 0
    parent = args.scratch or DEFAULT_SCRATCH
    parent.mkdir(parents=True, exist_ok=True)
    project = Path(tempfile.mkdtemp(prefix="run-", dir=parent))
    try:
        check(blocks, godot, project, args.timeout)
    finally:
        if args.keep:
            print(f"scratch project: {project}", file=sys.stderr)
        else:
            shutil.rmtree(project, ignore_errors=True)

    failed = [b for b in blocks if b.passed is None]
    strict = [b for b in failed if b.path.resolve() not in upstream]
    for b in failed:
        line, msg = report_line(b)
        tag = "" if b in strict else " (upstream, report only)"
        print(f"{b.path}:{line}:{tag} {msg}")
    whole = sum(1 for b in blocks if b.passed == "whole")
    extends = sum(1 for b in blocks if b.passed and b.passed.startswith("extends"))
    in_func = sum(1 for b in blocks if b.passed and b.passed.startswith("in a func"))
    print(
        f"gdscript_blocks_check: {len(files)} files, {len(blocks)} blocks, "
        f"{len(blocks) - len(failed)} parse ({whole} whole, {extends} with an added extends, "
        f"{in_func} in a func), {len(failed)} fail ({len(failed) - len(strict)} upstream, report only)",
        file=sys.stderr,
    )
    return 1 if strict else 0


if __name__ == "__main__":
    sys.exit(main())
