#!/usr/bin/env python3
"""Fail scene code that moves a thing with a linear ease.

Usage:
  scan_linear_motion.py <file-or-dir> [...]
  scan_linear_motion.py --raster-sizes <raster-sizes.json>
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

import json
import re
import sys
import tempfile
from pathlib import Path

SUFFIXES = {".html", ".htm", ".js", ".mjs", ".css"}
ALLOW = "motion-ok: linear"
MAX_RASTER_SCALE = 1.2

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


SAMPLE_ERROR = "each sample must be an object with positive natural_px and 2-value drawn_px"
RASTER_ERRORS = (FileNotFoundError, UnicodeDecodeError, json.JSONDecodeError, ValueError)
SCAN_ERRORS = (FileNotFoundError, UnicodeDecodeError)


def is_pair(value):
    return isinstance(value, list) and len(value) == 2 and all(isinstance(v, (int, float)) for v in value)


def sample_scale(sample):
    """The larger drawn/real ratio of 1 raster sample."""
    natural, drawn = (sample.get("real_px", sample.get("natural_px")), sample.get("drawn_px")) if isinstance(sample, dict) else (None, None)
    if not (is_pair(natural) and is_pair(drawn) and min(natural) > 0):
        raise ValueError(SAMPLE_ERROR)
    return max(d / n for d, n in zip(drawn, natural))


def raster_upscales(report):
    """Samples whose drawn width or height is more than 1.2x its natural size."""
    samples = report.get("samples") if isinstance(report, dict) else None
    if not isinstance(samples, list):
        raise ValueError("report needs a samples list")
    scaled = [(sample, sample_scale(sample)) for sample in samples]
    return [(sample, scale) for sample, scale in scaled if scale > MAX_RASTER_SCALE + 1e-9]


def read_raster_upscales(path):
    report = json.loads(Path(path).read_text(encoding="utf-8"))
    return raster_upscales(report)


def read_error(prefix, error):
    """Print the error of a bad input and give exit code 2."""
    print(f"scan_linear_motion: {prefix}{error}", file=sys.stderr)
    return 2


def report_hits(lines, summary, failed):
    print(*lines, summary, sep="\n")
    return 1 if failed else 0


def raster_line(path, sample, scale):
    real = sample.get("real_px")
    measured = f"; measured real resolution is {real[0]:g}x{real[1]:g}" if is_pair(real) else ""
    return f"{path}: {sample.get('src', '<unknown>')} at {sample.get('t_sec', '<unknown>')} s is {scale:.3f}x; maximum is {MAX_RASTER_SCALE:.1f}x{measured}"


def check_raster_sizes(path):
    try:
        hits = read_raster_upscales(path)
    except RASTER_ERRORS as e:
        return read_error("cannot read raster sizes: ", e)
    lines = [raster_line(path, sample, scale) for sample, scale in hits]
    summary = f"scan_linear_motion: {len(hits)} raster upscale(s) over {MAX_RASTER_SCALE:.1f}x" if hits else "scan_linear_motion: raster sizes pass"
    return report_hits(lines, summary, hits)


def check_linear_motion(paths):
    try:
        hits = scan(paths)
    except SCAN_ERRORS as e:
        return read_error("cannot read ", e)
    lines = [f"{f}:{line_no}: {rule}: {snippet}" for f, line_no, rule, snippet in hits]
    summary = f"scan_linear_motion: {len(hits)} linear move(s)" if hits else "scan_linear_motion: no linear moves"
    return report_hits(lines, summary, hits)


def usage(_argv=None):
    print(__doc__.strip(), file=sys.stderr)
    return 2


def run_self_test(argv):
    return self_test() if argv == ["--self-test"] else usage()


def run_raster_sizes(argv):
    return check_raster_sizes(argv[1]) if len(argv) == 2 else usage()


COMMANDS = {None: usage, "--self-test": run_self_test, "--raster-sizes": run_raster_sizes}


def main(argv):
    command = COMMANDS.get(argv[0] if argv else None, check_linear_motion)
    return command(argv)


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


def count_failure(root, name, text, want):
    """Write 1 fixture file, then scan it."""
    path = root / name
    path.write_text(text, encoding="utf-8")
    got = scan_text(path.read_text(encoding="utf-8"))
    return [f"{name}: {len(got)} hit(s), want {want}: {got}"] if len(got) != want else []


def fixture_failures(root):
    """Writes every fixture file; exit_failures reads them after this runs."""
    failures = [f for name, (text, want) in FIXTURES.items() for f in count_failure(root, name, text, want)]
    line = scan_text(FIXTURES["none-scale.js"][0])[0][0]
    return failures + ([f"none-scale.js hit on line {line}, want 1"] if line != 1 else [])


def exit_failures(root):
    cases = [
        ([str(root / "linear-x.js")], 1, "main does not exit 1 on a linear move"),
        ([str(root / "clean-spring.js"), str(root / "clean-css.css")], 0, "main does not exit 0 on clean files"),
        ([str(root / "missing.js")], 2, "main does not exit 2 on a missing file"),
    ]
    return [message for argv, want, message in cases if main(argv) != want]


def raster_failures(root):
    failures = []
    if raster_exit(root / "raster-clean.json", [120, 60]) != 0:
        failures.append("1.2x raster sample does not pass")
    if raster_exit(root / "raster-bad.json", [121, 50]) != 1:
        failures.append("1.21x raster sample does not fail")
    return failures + pre_upscale_failures(root)


def pre_upscale_failures(root):
    exit_code = raster_exit(root / "raster-pre-upscaled.json", [100, 50], [50, 25])
    return [] if exit_code == 1 else ["a pre-upscaled raster does not fail at its measured real resolution"]


def raster_exit(path, drawn, real=None):
    sample = {"t_sec": 0, "src": "plate.png", "natural_px": [100, 50], "drawn_px": drawn}
    if real:
        sample["real_px"] = real
    path.write_text(json.dumps({"samples": [sample]}), encoding="utf-8")
    return check_raster_sizes(path)


def all_failures():
    tmp = tempfile.TemporaryDirectory()
    root = Path(tmp.name)
    failures = fixture_failures(root) + exit_failures(root) + raster_failures(root)
    tmp.cleanup()
    return failures


def self_test():
    failures = all_failures()
    for f in failures:
        print(f"FAIL {f}", file=sys.stderr)
    print("scan_linear_motion self-test: " + ("FAILED" if failures else f"{len(FIXTURES)} fixtures pass"))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
