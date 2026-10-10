#!/usr/bin/env python3
"""Compile-check every ```csharp and ```cs code block in markdown files.

Usage:
    scripts/godot/csharp_blocks_check.py [--strict-own skills/copied.json] [--scratch DIR] [--keep] <paths...>

Each block goes into a scratch `Godot.NET.Sdk/<version>` project and
`dotnet build` compiles them all. csc reports only the first stage that has
errors (syntax, declarations, method bodies), so the build repeats without
the forms that got errors until a pass has none (at most 8 passes). Godot_mono is not needed: the SDK and the
GodotSharp assemblies come from NuGet. A block can be a whole file, class
members, or statements, so every block is compiled in several forms at once
and passes when one form has no error:

1. whole: the block as a file (when it declares a class, struct, interface,
   enum or record);
2. members: the block inside `public partial class __B : <Base>`;
3. statements: the block inside an `async void` method of that class.

`<Base>` is each of Node, Node2D, Node3D, Control, CharacterBody2D,
CharacterBody3D and Resource. Every form gets its own namespace, so blocks
do not see each other. `using` lines move to the top of the file. A block
with no `using` line gets `Godot`, `System`, `System.Collections.Generic`,
`System.Linq` and `System.Threading.Tasks`; a block with its own gets only
`Godot` added. Only compiler errors count; warnings do
not.

A failing block prints as `file:line: CS0103: message`, at the markdown line
of the error in the form that got furthest (the latest pass, then the fewest
errors). `<!-- csharp-check: skip -->`
on the line before the fence (blank lines between are allowed) skips the
block.

With `--strict-own skills/copied.json`, a failing block in a file that
copied.json lists as copied does not print and does not fail the run by
itself: the run counts these blocks per skill and fails when a count is above
`scripts/godot/copied-baseline.json` (the ratchet of
`gdscript_blocks_check.py`, see `copied_ratchet.py`).

The NuGet packages go to `<scratch>/nuget` (`NUGET_PACKAGES`) and the dotnet
home to `<scratch>/dotnet-home`, so nothing is written under the real home.
The first run downloads the Godot SDK packages; later runs reuse them. With
no `dotnet` on PATH it prints "skipped: no dotnet" and exits 0.

Exit status: 0 when every own block compiles, 1 otherwise, 2 on a usage
error or a build that did not run.
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import copied_ratchet  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
DEFAULT_SCRATCH = REPO / ".worktrees" / "_scratch" / "godot-csblockcheck"
GODOT_SDK = "4.7.2"
TARGET = "net8.0"
MAX_PASSES = 8

CS_LANGS = {"csharp", "cs", "c#"}
BASES = ["Node", "Node2D", "Node3D", "Control", "CharacterBody2D", "CharacterBody3D", "Resource"]
USINGS = ["Godot", "System", "System.Collections.Generic", "System.Linq", "System.Threading.Tasks"]

FENCE = re.compile(r"^(\s*)(`{3,}|~{3,})\s*([^\s`]*)")
SKIP = re.compile(r"<!--\s*csharp-check:\s*skip\s*-->")
USING_LINE = re.compile(r"^\s*(global\s+)?using\s+(static\s+)?[\w.]+(\s*=\s*[\w.<>, ]+)?\s*;\s*(//.*)?$")
TYPE_DECL = re.compile(
    r"^\s*(\[[^\]]*\]\s*)*((public|internal|private|protected|static|sealed|abstract|partial|readonly|file)\s+)*"
    r"(class|struct|interface|enum|record)\s+\w+"
)
NAMESPACE = re.compile(r"^\s*namespace\s+[\w.]+")
ERROR = re.compile(r"^(.*?\.cs)\((\d+),(\d+)\): error (\w+): (.*?)(?: \[[^\]]*\])?$")


@dataclass
class Form:
    name: str  # "whole", "members Node", "statements Node2D", ...
    file: str  # file name in the project
    first_line: int  # project line of the block's first code line
    errors: list[tuple[int, str]] = field(default_factory=list)  # (project line, "CS0103: msg")
    stage: int = 0  # the build pass that found the errors; a later pass went deeper


@dataclass
class Block:
    path: Path
    line: int  # markdown line of the first code line
    code: str
    forms: list[Form] = field(default_factory=list)

    @property
    def passed(self) -> bool:
        return any(not f.errors for f in self.forms)


def extract(path: Path) -> list[Block]:
    """The csharp blocks of a markdown file, without the skipped ones."""
    lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    blocks = []
    i = 0
    while i < len(lines):
        m = FENCE.match(lines[i])
        if not m:
            i += 1
            continue
        fence, tag = m[2], m[3].lower()
        start = i
        i += 1
        body = []
        while i < len(lines):
            e = FENCE.match(lines[i])
            if e and e[2].startswith(fence[0]) and len(e[2]) >= len(fence) and not e[3]:
                break
            body.append(lines[i])
            i += 1
        i += 1
        if tag not in CS_LANGS:
            continue
        j = start - 1
        while j >= 0 and not lines[j].strip():
            j -= 1
        if j >= 0 and SKIP.search(lines[j]):
            continue
        indent = len(m[1])
        code = "\n".join(x[indent:] if x[:indent].strip() == "" else x for x in body)
        blocks.append(Block(path, start + 2, code))
    return blocks


def split_usings(code: str) -> tuple[list[str], list[str]]:
    """(using lines, the other lines); a blank line keeps each line's number."""
    usings, rest = [], []
    for line in code.splitlines():
        if USING_LINE.match(line):
            usings.append(line.strip())
            rest.append("")
        else:
            rest.append(line)
    return usings, rest


def has_type_decl(lines: list[str]) -> bool:
    return any(TYPE_DECL.match(x) for x in lines)


def write_forms(block: Block, n: int, src: Path) -> None:
    usings, rest = split_usings(block.code)
    if usings:
        # The block names its usings: add only Godot, which every script needs.
        header = ([] if "using Godot;" in usings else ["using Godot;"]) + usings
    else:
        header = [f"using {u};" for u in USINGS]
    if any(NAMESPACE.match(x) for x in rest):
        shapes = [("whole", None, [], [])]
    elif has_type_decl(rest):
        shapes = [("whole", None, [f"namespace __b{n}_whole {{"], ["}"])]
    else:
        shapes = []
        for base in BASES:
            ns = f"__b{n}_{base}"
            shapes.append((f"members {base}", base, [f"namespace {ns}_m {{", f"public partial class __B : {base} {{"], ["}", "}"]))
            shapes.append((
                f"statements {base}", base,
                [f"namespace {ns}_s {{", f"public partial class __B : {base} {{", "public async void __Run() {"],
                ["}", "}", "}"],
            ))
    for i, (name, _base, before, after) in enumerate(shapes):
        file = f"b{n:04d}_{i:02d}.cs"
        text = header + before
        first = len(text) + 1
        text += rest + after
        (src / file).write_text("\n".join(text) + "\n")
        block.forms.append(Form(name, file, first))


def project_files(project: Path) -> None:
    (project / "Check.csproj").write_text(
        f'<Project Sdk="Godot.NET.Sdk/{GODOT_SDK}">\n'
        "  <PropertyGroup>\n"
        f"    <TargetFramework>{TARGET}</TargetFramework>\n"
        "    <EnableDynamicLoading>true</EnableDynamicLoading>\n"
        "    <Nullable>disable</Nullable>\n"
        "    <TreatWarningsAsErrors>false</TreatWarningsAsErrors>\n"
        "    <NoWarn>$(NoWarn);CS1998;CS0168;CS0219;CS0162;CS0414;CS0649;CS0169;CS8321</NoWarn>\n"
        "    <GenerateDocumentationFile>false</GenerateDocumentationFile>\n"
        "  </PropertyGroup>\n"
        "</Project>\n"
    )
    (project / "project.godot").write_text('[application]\nconfig/name="csblockcheck"\n[dotnet]\nproject/assembly_name="Check"\n')


def build(project: Path, scratch: Path, timeout: int) -> tuple[int, str]:
    env = dict(os.environ)
    env.update({
        "NUGET_PACKAGES": str(scratch / "nuget"),
        "DOTNET_CLI_HOME": str(scratch / "dotnet-home"),
        "DOTNET_CLI_TELEMETRY_OPTOUT": "1",
        "DOTNET_NOLOGO": "1",
        "DOTNET_SKIP_FIRST_TIME_EXPERIENCE": "1",
    })
    env.pop("MSBuildSDKsPath", None)
    out = subprocess.run(
        ["dotnet", "build", "-nologo", "-clp:NoSummary", "-v:q", "-p:GenerateFullPaths=true", str(project / "Check.csproj")],
        cwd=project, env=env, capture_output=True, text=True, timeout=timeout,
    )
    return out.returncode, out.stdout + out.stderr


def check(blocks: list[Block], project: Path, scratch: Path, timeout: int) -> tuple[int, str]:
    src = project / "blocks"
    src.mkdir(parents=True)
    project_files(project)
    by_file: dict[str, Form] = {}
    for n, b in enumerate(blocks):
        write_forms(b, n, src)
        for f in b.forms:
            by_file[f.file] = f
    # csc stops at the first stage with errors (syntax, then declarations,
    # then method bodies), so one build cannot report every form. Each pass
    # removes the forms that got errors, and the next pass goes deeper.
    code, log = 0, ""
    for stage in range(1, MAX_PASSES + 1):
        code, log = build(project, scratch, timeout)
        bad = errors_by_form(log, by_file)
        for form in bad:
            form.stage = stage
            (src / form.file).unlink(missing_ok=True)
            del by_file[form.file]
        if not bad:
            break
    else:
        # No clean pass: the forms that are left were never fully compiled.
        for form in by_file.values():
            form.errors.append((form.first_line, f"not checked: no clean build in {MAX_PASSES} passes"))
    return code, log


def errors_by_form(log: str, by_file: dict[str, Form]) -> list[Form]:
    """Record each error on its form; return the forms that got errors."""
    seen = set()
    bad: list[Form] = []
    for line in log.splitlines():
        m = ERROR.match(line.strip())
        if not m or line in seen:
            continue
        seen.add(line)
        form = by_file.get(Path(m[1]).name)
        if form is None:
            continue
        form.errors.append((int(m[2]), f"{m[4]}: {m[5]}"))
        if form not in bad:
            bad.append(form)
    return bad


def report_line(b: Block) -> tuple[int, str]:
    best = min(b.forms, key=lambda f: (-f.stage, len(f.errors)))
    line, msg = best.errors[0]
    return b.line + max(line - best.first_line, 0), msg


def files_under(paths: list[Path]) -> list[Path]:
    out = []
    for p in paths:
        if p.is_dir():
            out.extend(sorted(x for x in p.rglob("*.md") if x.is_file()))
        elif p.is_file():
            out.append(p)
        else:
            raise SystemExit(f"csharp_blocks_check.py: no such file or directory: {p}")
    return out


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--scratch", type=Path, help=f"scratch directory (default {DEFAULT_SCRATCH})")
    ap.add_argument("--keep", action="store_true", help="keep the scratch project")
    ap.add_argument("--timeout", type=int, default=900, help="seconds for the dotnet build")
    ap.add_argument(
        "--strict-own", type=Path, metavar="COPIED",
        help="count per skill, and fail only above the baseline, blocks in files that "
        "COPIED (skills/copied.json) lists as copied",
    )
    ap.add_argument("--baseline", type=Path, default=copied_ratchet.BASELINE, help="the copied-failure baseline")
    ap.add_argument("--update-baseline", action="store_true", help="write the copied counts to --baseline")
    ap.add_argument("paths", nargs="+", type=Path)
    args = ap.parse_args(argv)

    if shutil.which("dotnet") is None:
        print("skipped: no dotnet")
        return 0
    copied = copied_ratchet.copied_skills(args.strict_own) if args.strict_own else {}
    files = files_under(args.paths)
    blocks = [b for f in files for b in extract(f)]
    if not blocks:
        print(f"csharp_blocks_check: {len(files)} files, 0 blocks", file=sys.stderr)
        return 0
    scratch = (args.scratch or DEFAULT_SCRATCH).resolve()
    scratch.mkdir(parents=True, exist_ok=True)
    project = Path(tempfile.mkdtemp(prefix="run-", dir=scratch))
    try:
        code, log = check(blocks, project, scratch, args.timeout)
        if code != 0 and not any(f.errors for b in blocks for f in b.forms):
            print(log[-4000:], file=sys.stderr)
            print("csharp_blocks_check.py: the build failed with no block error (restore or SDK problem)", file=sys.stderr)
            return 2
    finally:
        if args.keep:
            print(f"scratch project: {project}", file=sys.stderr)
        else:
            shutil.rmtree(project, ignore_errors=True)

    failed = [b for b in blocks if not b.passed]
    strict = [b for b in failed if b.path.resolve() not in copied]
    up = []
    for b in failed:
        line, msg = report_line(b)
        if b in strict:
            print(f"{b.path}:{line}: {msg}")
        else:
            up.append((b.path, f"{b.path}:{line}: {msg}"))
    whole = sum(1 for b in blocks if b.passed and b.forms[0].name == "whole")
    print(
        f"csharp_blocks_check: {len(files)} files, {len(blocks)} blocks, {len(blocks) - len(failed)} compile "
        f"({whole} whole), {len(failed)} fail ({len(failed) - len(strict)} copied)",
        file=sys.stderr,
    )
    grew = 0
    if args.strict_own:
        grew = copied_ratchet.apply(
            "csharp_blocks_check", "blocks", up, files, copied, args.baseline, args.update_baseline
        )
    return 1 if strict or grew else 0


if __name__ == "__main__":
    sys.exit(main())
