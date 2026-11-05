#!/usr/bin/env python3
"""Fail scene code that moves a thing with a linear ease.

Usage:
  scan_linear_motion.py <file-or-dir> [...]
  scan_linear_motion.py --self-test

A thing that moves, grows or turns at a constant speed looks mechanical. The
scan reads .html, .js, .mjs and .css files and reports:
  - a tween call (.to, .fromTo, .from, .animate) that sets a moving property
    (x, y, z, scale, rotate, rotation, translate, transform, left, top) and a
    linear ease ("linear", "none" or cubic-bezier(0, 0, 1, 1));
  - a CSS transition on a moving property (or "all") with a linear timing.
Opacity, colour and other non-moving properties may be linear.

A constant move is sometimes right: a slow camera drift, a ticker, a progress
bar. Mark it with the comment "motion-ok: linear <reason>" on the line of the
call or the line above it.

Exit codes: 0 clean, 1 a linear move found, 2 usage or input error.
Needs only the Python standard library.
"""

import re
import sys
import tempfile
from pathlib import Path

SUFFIXES = {".html", ".htm", ".js", ".mjs", ".css"}
ALLOW = "motion-ok: linear"

CALL = re.compile(r"\.(to|fromTo|from|animate)\s*\(")
MOVING_KEY = re.compile(
    r"(?<![\w$.-])[\"']?(x|y|z|scale[XYZ]?|rotate[XYZ]?|rotation[XYZ]?|translate[XYZ]?|transform|left|top)[\"']?\s*:"
)
LINEAR_EASE = re.compile(
    r"\beas(?:e|ing)\s*:\s*([\"'`])\s*(?:linear|none|cubic-bezier\(\s*0\s*,\s*0\s*,\s*1\s*,\s*1\s*\))\s*\1"
)
TRANSITION = re.compile(r"(?<![\w-])transition\s*:([^;{}]*)", re.IGNORECASE)
MOVING_CSS = re.compile(r"(?<![\w-])(all|transform|translate|scale|rotate|left|top)(?![\w-])", re.IGNORECASE)
LINEAR_CSS = re.compile(r"(?<![\w-])linear(?![\w-])|cubic-bezier\(\s*0\s*,\s*0\s*,\s*1\s*,\s*1\s*\)", re.IGNORECASE)


def call_text(text, open_at):
    """The text of a call from its "(" to the matching ")", skipping strings."""
    depth, quote, i = 0, None, open_at
    while i < len(text):
        c = text[i]
        if quote:
            if c == "\\":
                i += 1
            elif c == quote:
                quote = None
        elif c in "\"'`":
            quote = c
        elif c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
            if depth == 0:
                return text[open_at : i + 1]
        i += 1
    return text[open_at:]


def allowed(lines, line_no):
    return any(ALLOW in lines[n - 1] for n in (line_no - 1, line_no) if 1 <= n <= len(lines))


def scan_text(text):
    """A list of (line number, rule, snippet) for each linear move in text."""
    lines = text.splitlines()
    hits = []

    def add(offset, rule, snippet):
        line_no = text.count("\n", 0, offset) + 1
        if not allowed(lines, line_no):
            hits.append((line_no, rule, " ".join(snippet.split())[:100]))

    for m in CALL.finditer(text):
        body = call_text(text, m.end() - 1)
        if MOVING_KEY.search(body) and LINEAR_EASE.search(body):
            add(m.start(), f"linear .{m.group(1)}() tween on a moving property", m.group(0) + body[1:])
    for m in TRANSITION.finditer(text):
        # Each comma item of a transition list has its own property and timing.
        items = re.split(r",(?![^()]*\))", m.group(1))
        if any(MOVING_CSS.search(item) and LINEAR_CSS.search(item) for item in items):
            add(m.start(), "linear CSS transition on a moving property", m.group(0))
    return hits


def scene_files(paths):
    files = []
    for p in map(Path, paths):
        if p.is_dir():
            files += sorted(f for f in p.rglob("*") if f.is_file() and f.suffix in SUFFIXES)
        elif p.is_file():
            files.append(p)
        else:
            raise FileNotFoundError(p)
    return files


def scan(paths):
    return [(f, *hit) for f in scene_files(paths) for hit in scan_text(f.read_text(encoding="utf-8"))]


def main(argv):
    if not argv:
        print(__doc__.strip(), file=sys.stderr)
        return 2
    if argv == ["--self-test"]:
        return self_test()
    try:
        hits = scan(argv)
    except (FileNotFoundError, UnicodeDecodeError) as e:
        print(f"scan_linear_motion: cannot read {e}", file=sys.stderr)
        return 2
    for f, line_no, rule, snippet in hits:
        print(f"{f}:{line_no}: {rule}: {snippet}")
    print(f"scan_linear_motion: {len(hits)} linear move(s)" if hits else "scan_linear_motion: no linear moves")
    return 1 if hits else 0


# ---------------------------------------------------------------- self-test
FIXTURES = {
    # name: (file text, expected hit count)
    "linear-x.js": ('tl.to("#card", { x: 120, duration: 0.6, ease: "linear" }, 0);\n', 1),
    "none-scale.js": ("tl.fromTo(dot, { scale: 0 },\n  { scale: 1, ease: 'none' }, 1);\n", 1),
    "bezier-rotate.html": ('<script>\nm.to(w, { rotate: 90, ease: "cubic-bezier(0, 0, 1, 1)" });\n</script>\n', 1),
    "waapi-transform.js": ('el.animate([{ transform: "translateX(0)" }, { transform: "translateX(9px)" }], '
                           '{ duration: 400, easing: "linear" });\n', 1),
    "css-transition.css": (".card { transition: transform 300ms linear; }\n.a { transition: all 1s linear }\n"
                           ".b { transition: opacity 1s ease, left 2s cubic-bezier(0, 0, 1, 1); }\n", 3),
    "clean-spring.js": ('tl.to("#card", { x: 120, ease: "spring-ui" }, 0);\n'
                        'tl.to("#card", { opacity: 0, duration: 0.3, ease: "linear" }, 1);\n'
                        'tl.to(bar, { width: "100%", ease: "linear" }, 0);\n', 0),
    "clean-css.css": (".fade { transition: opacity 200ms linear, transform 300ms ease-out; }\n", 0),
    "allowed-drift.js": ('// motion-ok: linear slow camera drift\ntl.to(cam, { x: -40, duration: 6, ease: "linear" }, 0);\n'
                         'tl.to(tick, { y: -200, ease: "none" }, 0); // motion-ok: linear ticker\n', 0),
}


def self_test():
    failures = []
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        for name, (text, _) in FIXTURES.items():
            (root / name).write_text(text, encoding="utf-8")
        for name, (_, want) in FIXTURES.items():
            got = scan_text((root / name).read_text(encoding="utf-8"))
            if len(got) != want:
                failures.append(f"{name}: {len(got)} hit(s), want {want}: {got}")
        if main([str(root / "linear-x.js")]) != 1:
            failures.append("main does not exit 1 on a linear move")
        if main([str(root / "clean-spring.js"), str(root / "clean-css.css")]) != 0:
            failures.append("main does not exit 0 on clean files")
        if main([str(root / "missing.js")]) != 2:
            failures.append("main does not exit 2 on a missing file")
        line = scan_text(FIXTURES["none-scale.js"][0])[0][0]
        if line != 1:
            failures.append(f"none-scale.js hit on line {line}, want 1")
    for f in failures:
        print(f"FAIL {f}", file=sys.stderr)
    print("scan_linear_motion self-test: " + ("FAILED" if failures else f"{len(FIXTURES)} fixtures pass"))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
