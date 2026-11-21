#!/usr/bin/env python3
"""The comment ban and its ratchet (docs/adr/0004-no-comments-in-code.md).

Code in this repository carries no comments. Old files still hold some, so
the ban is a ratchet: `baseline.json` records, per file, how many lines hold
a comment today, and `check` fails when a file holds more than its baseline.
A file the baseline does not name has a baseline of 0.

    comment_lint.py check                  the lint; exit 1 when a count grew
    comment_lint.py check --update-baseline   lower the baseline to today's counts
                                           (it never raises one; a rise is a hand edit)
    comment_lint.py list [PATH ...]        every counted comment as JSON lines
    comment_lint.py strip PATH ...         delete every counted comment in place
    comment_lint.py verify [--base REF]    prove that only comments changed since REF
    comment_lint.py estimate               comment lines left, per language

A comment that a program reads is not counted and `strip` leaves it: a
shebang, a lint or type-checker directive, a `SAFETY:` note that clippy
demands, a licence header, a `just --list` recipe line, a pinned action
version, and a doc comment that clap prints as help text. A Python docstring
is a string and not a comment.

Files copied from elsewhere (`copied_files` in skills/copied.json) and files
git does not track are out of scope.
"""

from __future__ import annotations

import argparse
import io
import json
import re
import subprocess
import sys
import tokenize
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
BASELINE = Path(__file__).resolve().parent / "baseline.json"

LANGUAGES = {
    ".rs": "rust",
    ".py": "python",
    ".gd": "gdscript",
    ".sh": "shell",
    ".js": "javascript",
    ".mjs": "javascript",
    ".ts": "javascript",
    ".toml": "toml",
    ".yml": "yaml",
    ".yaml": "yaml",
}

DIRECTIVES = {
    "rust": re.compile(r"^(//|/\*)\s*SAFETY:"),
    "python": re.compile(
        r"^#\s*(type:|noqa\b|pylint:|pragma:|fmt:|ruff:|mypy:|pyright:|isort:|-\*-\s*coding)"
    ),
    "gdscript": re.compile(r"^#\s*(region\b|endregion\b|warning-ignore|gdlint:)"),
    "shell": re.compile(r"^#\s*shellcheck\s"),
    "javascript": re.compile(
        r"^(//|/\*)\s*(@ts-|eslint|global\s|globals\s|prettier-ignore|biome-ignore|istanbul|c8\s|deno-|#\s*sourceMappingURL)"
        r"|^///\s*<reference"
    ),
    "toml": re.compile(r"^#:schema\b"),
    "yaml": re.compile(r"^#\s*(yamllint|shellcheck)\s"),
    "just": re.compile(r"^#\s*shellcheck\s"),
}

LICENCE = re.compile(r"SPDX-License-Identifier|Copyright \(c\)|Copyright ©|Licensed under the")
CLAP_DERIVE = re.compile(r"#\[derive\([^\]]*\b(Parser|Subcommand|Args|ValueEnum)\b[^\]]*\)\]")
PINNED_ACTION = re.compile(r"uses:\s*\S+@[0-9a-f]{40}\s*$")
JUST_RECIPE = re.compile(r"^@?[A-Za-z_][A-Za-z0-9_-]*(\s+[^:]*)?:(?!=)")


@dataclass
class Comment:
    start: int
    end: int
    text: str
    exempt: str = ""


def language_of(path: Path) -> str | None:
    if path.name == "justfile":
        return "just"
    lang = LANGUAGES.get(path.suffix)
    if lang or path.suffix:
        return lang
    try:
        with path.open("rb") as f:
            first = f.readline(200)
    except OSError:
        return None
    if not first.startswith(b"#!"):
        return None
    if b"python" in first:
        return "python"
    if re.search(rb"\b(ba|z|da)?sh\b", first):
        return "shell"
    return None


RUST_TOKEN = re.compile(
    r"""
      (?P<raw>\b(?:b|c)?r(?P<hashes>\#*)")
    | (?P<str>(?:\b[bc])?"(?:[^"\\]|\\.)*")
    | (?P<chr>(?:\bb)?'(?:\\(?:u\{[^}]*\}|x[0-9a-fA-F]{2}|.)|[^\\'\n])')
    | (?P<line>//[^\n]*)
    | (?P<block>/\*)
    """,
    re.X | re.S,
)


def lex_rust(src: str) -> list[Comment]:
    out = []
    pos = 0
    while True:
        m = RUST_TOKEN.search(src, pos)
        if not m:
            return out
        if m.lastgroup == "raw":
            close = src.find('"' + m.group("hashes"), m.end())
            pos = len(src) if close < 0 else close + 1 + len(m.group("hashes"))
        elif m.lastgroup == "line":
            out.append(Comment(m.start(), m.end(), m.group()))
            pos = m.end()
        elif m.lastgroup == "block":
            depth, i = 1, m.end()
            while depth and i < len(src):
                nxt = re.compile(r"/\*|\*/").search(src, i)
                if not nxt:
                    i = len(src)
                    break
                depth += 1 if nxt.group() == "/*" else -1
                i = nxt.end()
            out.append(Comment(m.start(), i, src[m.start() : i]))
            pos = i
        else:
            pos = m.end()


JS_TOKEN = re.compile(
    r"""
      (?P<str>"(?:[^"\\\n]|\\.)*"|'(?:[^'\\\n]|\\.)*')
    | (?P<tpl>`)
    | (?P<line>//[^\n]*)
    | (?P<block>/\*.*?\*/)
    | (?P<slash>/)
    """,
    re.X | re.S,
)
JS_REGEX_BODY = re.compile(r"/(?:[^/\\\n\[]|\\.|\[(?:[^\]\\\n]|\\.)*\])+/[A-Za-z]*")
JS_VALUE_END = re.compile(r"[\w$)\]}.]$")
JS_KEYWORD_END = re.compile(r"\b(return|typeof|case|in|of|do|else|void|delete|throw|new|yield|await)$")


def js_template_end(src: str, pos: int) -> int:
    i = pos
    while i < len(src):
        c = src[i]
        if c == "\\":
            i += 2
        elif c == "`":
            return i + 1
        elif c == "$" and src[i + 1 : i + 2] == "{":
            depth, i = 1, i + 2
            while depth and i < len(src):
                c = src[i]
                if c == "`":
                    i = js_template_end(src, i + 1)
                    continue
                if c in "\"'":
                    m = JS_TOKEN.match(src, i)
                    i = m.end() if m and m.lastgroup == "str" else i + 1
                    continue
                depth += (c == "{") - (c == "}")
                i += 1
        else:
            i += 1
    return len(src)


def lex_javascript(src: str) -> list[Comment]:
    out = []
    pos = 0
    while True:
        m = JS_TOKEN.search(src, pos)
        if not m:
            return out
        if m.lastgroup == "tpl":
            pos = js_template_end(src, m.end())
        elif m.lastgroup in ("line", "block"):
            out.append(Comment(m.start(), m.end(), m.group()))
            pos = m.end()
        elif m.lastgroup == "slash":
            before = src[max(0, m.start() - 40) : m.start()].rstrip()
            is_value = bool(JS_VALUE_END.search(before)) and not JS_KEYWORD_END.search(before)
            body = None if is_value else JS_REGEX_BODY.match(src, m.start())
            pos = body.end() if body else m.end()
        else:
            pos = m.end()


def lex_python(src: str) -> list[Comment]:
    starts = [0]
    for line in src.splitlines(keepends=True):
        starts.append(starts[-1] + len(line))
    out = []
    for tok in tokenize.generate_tokens(io.StringIO(src).readline):
        if tok.type == tokenize.COMMENT:
            start = starts[tok.start[0] - 1] + tok.start[1]
            out.append(Comment(start, start + len(tok.string), tok.string))
    return out


def hash_lexer(triple: bool, raw_single: bool, word_start: bool):
    parts = []
    if triple:
        parts.append(r'(?P<triple>"""(?:[^\\]|\\.)*?"""|' + r"'''(?:[^\\]|\\.)*?''')")
    parts.append(r'(?P<dq>"(?:[^"\\\n]|\\.)*")')
    parts.append(r"(?P<sq>'[^'\n]*')" if raw_single else r"(?P<sq>'(?:[^'\\\n]|\\.)*')")
    parts.append(r"(?P<hash>(?<![^\s;(|&])\#[^\n]*)" if word_start else r"(?P<hash>\#[^\n]*)")
    token = re.compile("|".join(parts), re.S)

    def lex(src: str) -> list[Comment]:
        return [
            Comment(m.start(), m.end(), m.group())
            for m in token.finditer(src)
            if m.lastgroup == "hash"
        ]

    return lex


lex_gdscript = hash_lexer(triple=True, raw_single=False, word_start=False)
lex_toml = hash_lexer(triple=True, raw_single=True, word_start=False)
lex_just_plain = hash_lexer(triple=False, raw_single=True, word_start=True)

SHELL_TOKEN = re.compile(
    r"""
      (?P<esc>\\.)
    | (?P<ansi>\$'(?:[^'\\]|\\.)*')
    | (?P<sq>'[^']*')
    | (?P<dq>"(?:[^"\\]|\\.)*")
    | (?P<heredoc>(?<!<)<<(?!<)-?\s*(?P<q>['"]?)(?P<tag>[A-Za-z_][A-Za-z0-9_]*)(?P=q))
    | (?P<hash>(?<![^\s;(|&])\#[^\n]*)
    """,
    re.X | re.S,
)


def lex_shell(src: str) -> list[Comment]:
    out = []
    pos = 0
    pending: list[tuple[str, bool]] = []
    while True:
        m = SHELL_TOKEN.search(src, pos)
        line_end = src.find("\n", pos)
        if pending and line_end >= 0 and (not m or m.start() > line_end):
            pos = line_end + 1
            for tag, dash in pending:
                while pos < len(src):
                    eol = src.find("\n", pos)
                    eol = len(src) if eol < 0 else eol
                    line = src[pos:eol]
                    pos = min(eol + 1, len(src))
                    if (line.lstrip("\t") if dash else line) == tag:
                        break
            pending = []
            continue
        if not m:
            return out
        if m.lastgroup == "heredoc":
            pending.append((m.group("tag"), m.group().startswith("<<-")))
        elif m.lastgroup == "hash":
            out.append(Comment(m.start(), m.end(), m.group()))
        pos = m.end()


YAML_HASH = re.compile(
    r"""
      (?P<dq>"(?:[^"\\\n]|\\.)*")
    | (?P<sq>'(?:[^'\n]|'')*')
    | (?P<hash>(?<![^\s])\#[^\n]*)
    """,
    re.X,
)
YAML_BLOCK = re.compile(r":\s*[|>][-+0-9]*\s*$")


def lex_yaml(src: str) -> list[Comment]:
    out = []
    pos = 0
    block_indent = None
    for line in src.splitlines(keepends=True):
        body = line.rstrip("\n")
        indent = len(body) - len(body.lstrip(" "))
        if block_indent is not None:
            if not body.strip() or indent > block_indent:
                pos += len(line)
                continue
            block_indent = None
        code = body
        for m in YAML_HASH.finditer(body):
            if m.lastgroup == "hash":
                out.append(Comment(pos + m.start(), pos + m.end(), m.group()))
                code = body[: m.start()]
        if YAML_BLOCK.search(code.rstrip()):
            block_indent = indent
        pos += len(line)
    return out


LEXERS = {
    "rust": lex_rust,
    "python": lex_python,
    "gdscript": lex_gdscript,
    "shell": lex_shell,
    "javascript": lex_javascript,
    "toml": lex_toml,
    "yaml": lex_yaml,
    "just": lex_just_plain,
}


def line_bounds(src: str, pos: int) -> tuple[int, int]:
    start = src.rfind("\n", 0, pos) + 1
    end = src.find("\n", pos)
    return start, len(src) if end < 0 else end


def is_full_line(src: str, c: Comment) -> bool:
    start, _ = line_bounds(src, c.start)
    _, end = line_bounds(src, max(c.start, c.end - 1))
    return not src[start : c.start].strip() and not src[c.end : end].strip()


def blank(src: str, comments: list[Comment]) -> str:
    chars = list(src)
    for c in comments:
        for i in range(c.start, c.end):
            if chars[i] != "\n":
                chars[i] = " "
    return "".join(chars)


def clap_ranges(src: str, comments: list[Comment]) -> list[tuple[int, int]]:
    code = blank(src, comments)
    ranges = []
    for m in CLAP_DERIVE.finditer(code):
        head = m.start()
        while True:
            prev_start, _ = line_bounds(src, max(0, line_bounds(src, head)[0] - 1))
            prev = src[prev_start : line_bounds(src, prev_start)[1]].strip()
            if prev_start >= head or not (prev.startswith("///") or prev.startswith("#[")):
                break
            head = prev_start
        opener = code.find("{", m.end())
        semi = code.find(";", m.end())
        if opener < 0 or (0 <= semi < opener):
            ranges.append((head, semi if semi >= 0 else m.end()))
            continue
        depth, i = 1, opener + 1
        while depth and i < len(code):
            t = RUST_TOKEN.match(code, i)
            if t and t.lastgroup in ("str", "chr"):
                i = t.end()
                continue
            depth += (code[i] == "{") - (code[i] == "}")
            i += 1
        ranges.append((head, i))
    return ranges


def mark_exempt(lang: str, src: str, comments: list[Comment]) -> None:
    directive = DIRECTIVES.get(lang)
    for c in comments:
        if c.start == 0 and c.text.startswith("#!"):
            c.exempt = "shebang"
        elif directive and directive.search(c.text):
            c.exempt = "directive"
        elif LICENCE.search(c.text):
            c.exempt = "licence"
    for i, c in enumerate(comments):
        if c.exempt not in ("directive", "licence") or not is_full_line(src, c):
            continue
        if lang == "python" and c.exempt == "directive":
            continue
        j = i + 1
        while (
            j < len(comments)
            and not comments[j].exempt
            and is_full_line(src, comments[j])
            and not src[comments[j - 1].end : comments[j].start].strip()
            and src[comments[j - 1].end : comments[j].start].count("\n") == 1
        ):
            comments[j].exempt = c.exempt
            j += 1
        if c.exempt == "licence":
            j = i - 1
            while (
                j >= 0
                and not comments[j].exempt
                and is_full_line(src, comments[j])
                and src[comments[j].end : comments[j + 1].start].count("\n") == 1
            ):
                comments[j].exempt = "licence"
                j -= 1
    if lang == "rust":
        ranges = clap_ranges(src, comments)
        for c in comments:
            if c.exempt or not c.text.startswith("///") or c.text.startswith("////"):
                continue
            if any(a <= c.start < b for a, b in ranges):
                c.exempt = "clap-help"
    if lang == "yaml":
        for c in comments:
            start, _ = line_bounds(src, c.start)
            if not c.exempt and PINNED_ACTION.search(src[start : c.start]):
                c.exempt = "pinned-version"
    if lang == "just":
        for c in comments:
            _, end = line_bounds(src, c.start)
            nxt = src[end + 1 : line_bounds(src, min(end + 1, len(src)))[1]]
            if not c.exempt and is_full_line(src, c) and JUST_RECIPE.match(nxt):
                c.exempt = "recipe-doc"


def scan(path: Path, src: str | None = None) -> tuple[str, list[Comment]]:
    lang = language_of(path)
    if lang is None:
        return "", []
    if src is None:
        src = path.read_text(encoding="utf-8")
    try:
        comments = LEXERS[lang](src)
    except (tokenize.TokenError, IndentationError, SyntaxError):
        comments = lex_gdscript(src)
    mark_exempt(lang, src, comments)
    return src, comments


def comment_lines(src: str, comments: list[Comment]) -> set[int]:
    lines = set()
    for c in comments:
        if c.exempt:
            continue
        first = src.count("\n", 0, c.start) + 1
        lines.update(range(first, first + c.text.count("\n") + 1))
    return lines


def strip_source(src: str, comments: list[Comment]) -> str:
    cuts = []
    for c in comments:
        if c.exempt:
            continue
        start, _ = line_bounds(src, c.start)
        _, end = line_bounds(src, max(c.start, c.end - 1))
        before, after = src[start : c.start], src[c.end : end]
        if not before.strip() and not after.strip():
            cuts.append((start, min(end + 1, len(src)), True))
        elif not after.strip():
            cuts.append((start + len(before.rstrip()), end, False))
        else:
            cuts.append((c.start, c.end + (1 if after[:1] == " " and before[-1:] == " " else 0), False))
    out = []
    pos = 0
    for start, end, whole in cuts:
        if start < pos:
            continue
        out.append(src[pos:start])
        pos = end
        if whole:
            kept = "".join(out)
            prev_blank = not kept.strip() or kept.endswith("\n\n")
            if prev_blank:
                while src[pos : pos + 1] == "\n":
                    pos += 1
    out.append(src[pos:])
    return "".join(out)


def copied_files(root: Path) -> set[str]:
    manifest = root / "skills" / "copied.json"
    if not manifest.is_file():
        return set()
    return {
        f"skills/{entry['name']}/{rel}"
        for entry in json.loads(manifest.read_text())["skills"]
        for rel in entry["copied_files"]
    }


def tracked_files(root: Path, only: list[str] | None = None) -> list[str]:
    listed = subprocess.run(
        ["git", "ls-files", "-z", "--", *(only or [])],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.split("\0")
    skip = copied_files(root)
    return sorted(p for p in listed if p and p not in skip and (root / p).is_file())


def counts(root: Path, only: list[str] | None = None) -> dict[str, int]:
    out = {}
    for rel in tracked_files(root, only):
        try:
            src, comments = scan(root / rel)
        except UnicodeDecodeError:
            continue
        n = len(comment_lines(src, comments))
        if n:
            out[rel] = n
    return out


def cmd_check(args) -> int:
    root = Path(args.root).resolve()
    baseline_path = Path(args.baseline)
    now = counts(root)
    base = json.loads(baseline_path.read_text()) if baseline_path.is_file() else {}
    if args.update_baseline:
        if baseline_path.is_file():
            base = {p: min(n, base[p]) for p, n in now.items() if p in base}
        else:
            base = now
        baseline_path.write_text(json.dumps(dict(sorted(base.items())), indent=2) + "\n")
        print(f"comment-lint: wrote {sum(base.values())} lines in {len(base)} files to {baseline_path.name}")
    grew = [(p, n, base.get(p, 0)) for p, n in sorted(now.items()) if n > base.get(p, 0)]
    shrank = sum(1 for p, n in base.items() if now.get(p, 0) < n)
    total, limit = sum(now.values()), sum(base.values())
    print(f"comment-lint: {total} comment lines in {len(now)} files (baseline {limit})")
    for path, n, was in grew:
        print(f"FAIL comment-lint: {path} has {n} comment lines, baseline {was}:")
        src, comments = scan(root / path)
        for c in comments:
            if not c.exempt:
                line = src.count("\n", 0, c.start) + 1
                print(f"  {path}:{line}: {c.text.splitlines()[0][:100]}")
    if grew:
        print(
            "Code here carries no comments (docs/adr/0004-no-comments-in-code.md). "
            "Delete the new ones: say it in a name, a type, a test or an ADR."
        )
        return 1
    if shrank:
        print(
            f"comment-lint: {shrank} files are below their baseline. Lower it: "
            "scripts/comments/comment_lint.py check --update-baseline"
        )
    return 0


def cmd_list(args) -> int:
    root = Path(args.root).resolve()
    for rel in tracked_files(root, args.paths):
        try:
            src, comments = scan(root / rel)
        except UnicodeDecodeError:
            continue
        for c in comments:
            if c.exempt and not args.exempt:
                continue
            line = src.count("\n", 0, c.start) + 1
            row = {"file": rel, "line": line, "lines": c.text.count("\n") + 1, "text": c.text}
            if c.exempt:
                row["exempt"] = c.exempt
            print(json.dumps(row))
    return 0


def cmd_strip(args) -> int:
    root = Path(args.root).resolve()
    removed = 0
    for rel in tracked_files(root, args.paths):
        try:
            src, comments = scan(root / rel)
        except UnicodeDecodeError:
            continue
        n = len(comment_lines(src, comments))
        if n:
            (root / rel).write_text(strip_source(src, comments), encoding="utf-8")
            removed += n
            print(f"{rel}: {n}")
    print(f"comment-lint: stripped {removed} comment lines")
    return 0


def code_only(path: Path, src: str) -> list[str]:
    lang = language_of(path)
    try:
        comments = LEXERS[lang](src)
    except (tokenize.TokenError, IndentationError, SyntaxError):
        comments = lex_gdscript(src)
    comments = [c for c in comments if not (c.start == 0 and c.text.startswith("#!"))]
    lines = (line.rstrip() for line in blank(src, comments).split("\n"))
    return [line for line in lines if line]


def cmd_verify(args) -> int:
    root = Path(args.root).resolve()
    changed = subprocess.run(
        ["git", "diff", "--name-only", "-z", "--diff-filter=M", args.base, "--"],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.split("\0")
    scope = set(tracked_files(root))
    bad = checked = 0
    for rel in sorted(p for p in changed if p in scope and language_of(root / p)):
        old = subprocess.run(
            ["git", "show", f"{args.base}:{rel}"], cwd=root, check=True, capture_output=True
        ).stdout.decode("utf-8")
        new = (root / rel).read_text(encoding="utf-8")
        before, after = code_only(root / rel, old), code_only(root / rel, new)
        checked += 1
        if before != after:
            bad += 1
            at = next((i for i, (a, b) in enumerate(zip(before, after)) if a != b), min(len(before), len(after)))
            print(f"FAIL comment-lint verify: {rel}: code changed, first at code line {at + 1}:")
            print(f"  was: {before[at] if at < len(before) else '<end of file>'}")
            print(f"  now: {after[at] if at < len(after) else '<end of file>'}")
    print(f"comment-lint verify: {checked} changed files since {args.base}, {bad} with a code change")
    return 1 if bad else 0


def cmd_estimate(args) -> int:
    root = Path(args.root).resolve()
    per: dict[str, list[int]] = {}
    for rel in tracked_files(root):
        try:
            src, comments = scan(root / rel)
        except UnicodeDecodeError:
            continue
        if not comments:
            continue
        lang = language_of(root / rel)
        whole = partial = 0
        for c in comments:
            if c.exempt:
                continue
            if is_full_line(src, c):
                whole += c.text.count("\n") + 1
            else:
                partial += 1
        row = per.setdefault(lang, [0, 0, 0, 0])
        row[0] += 1 if whole or partial else 0
        row[1] += whole
        row[2] += partial
        row[3] += len({src.count("\n", 0, c.start) for c in comments if c.exempt})
    print(f"{'language':12}{'files':>7}{'whole lines':>13}{'trailing':>10}{'exempt':>8}")
    for lang, row in sorted(per.items(), key=lambda kv: -kv[1][1]):
        print(f"{lang:12}{row[0]:7}{row[1]:13}{row[2]:10}{row[3]:8}")
    total = [sum(r[i] for r in per.values()) for i in range(4)]
    print(f"{'total':12}{total[0]:7}{total[1]:13}{total[2]:10}{total[3]:8}")
    print("whole lines leave the file when the comments go; a trailing comment shortens a line.")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--root", default=str(ROOT))
    sub = parser.add_subparsers(dest="command", required=True)
    check = sub.add_parser("check")
    check.add_argument("--baseline", default=str(BASELINE))
    check.add_argument("--update-baseline", action="store_true")
    check.set_defaults(run=cmd_check)
    lst = sub.add_parser("list")
    lst.add_argument("paths", nargs="*")
    lst.add_argument("--exempt", action="store_true")
    lst.set_defaults(run=cmd_list)
    strip = sub.add_parser("strip")
    strip.add_argument("paths", nargs="+")
    strip.set_defaults(run=cmd_strip)
    verify = sub.add_parser("verify")
    verify.add_argument("--base", default="HEAD")
    verify.set_defaults(run=cmd_verify)
    est = sub.add_parser("estimate")
    est.set_defaults(run=cmd_estimate)
    args = parser.parse_args(argv)
    return args.run(args)


if __name__ == "__main__":
    sys.exit(main())
