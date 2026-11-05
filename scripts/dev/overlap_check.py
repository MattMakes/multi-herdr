#!/usr/bin/env python3
"""Cleanroom overlap check: find upstream wording inside our video files.

Compares each of our files with every upstream text file (and with the
upstream spec notes, which a builder may have copied from):
  - word 8-grams: lowercase, punctuation stripped, spaces collapsed;
  - token 12-grams for code files: identifiers and case kept.
Adjacent matching n-grams merge into one hit. Each hit prints both paths and
the shared text. Exit 1 when any hit is not a common phrase.

Usage: overlap_check.py [--upstream DIR] [--specs DIR] [--root DIR] [FILE ...]
       overlap_check.py --self-test
With no FILE, the files are every tracked skills/video-*, teammates/video-* and
skills/orchestrate/references/video-pipeline.md under --root.
Python standard library only.
"""
import argparse
import re
import subprocess
import sys
import tempfile
from pathlib import Path

WORD_N = 8
CODE_N = 12
# Prose (our .md and .txt files) is read with a shorter window: a phrase of 6 words
# is enough to be someone's wording, while 8 let 6 and 7 word phrases through.
PROSE_N = 6
# kind -> (how the text is split, window size)
KINDS = {"prose": ("word", PROSE_N), "word": ("word", WORD_N), "code": ("code", CODE_N)}
MAX_BYTES = 1_000_000
TEXT_EXT = {".md", ".txt", ".ts", ".tsx", ".js", ".mjs", ".cjs", ".py", ".html",
            ".css", ".sh", ".json", ".yml", ".yaml", ".toml", ".hs", ".rs", ".mdx"}
CODE_EXT = {".ts", ".tsx", ".js", ".mjs", ".cjs", ".py", ".html", ".css", ".sh", ".hs", ".rs"}
SKIP_DIRS = {".git", "node_modules", "dist", "build"}
SKIP_NAMES = {"package-lock.json", "pnpm-lock.yaml", "yarn.lock", "cabal.project.freeze"}

# Allowlist. A hit is common when fewer than MIN_FREE words of it are free
# (not numeric, not in this set). One comment per entry.
ALLOW_WORDS = {
    # ffmpeg CLI flags and options: fixed by the tool, no expression
    "ffmpeg", "ffprobe", "i", "y", "c", "v", "a", "vf", "af", "ss", "t", "to", "r",
    "pix", "fmt", "yuv420p", "libx264", "crf", "preset", "movflags", "faststart",
    "map", "filter", "complex", "vsync", "an", "vn", "f", "lavfi",
    # ffmpeg filter names: fixed by the tool
    "scale", "crop", "pad", "overlay", "fps", "concat", "setpts", "atempo",
    "amix", "volume", "loudnorm", "afade", "fade", "xfade", "blend", "drawtext",
    "aresample", "anullsrc", "color", "format", "palettegen", "paletteuse",
    # standard web and Node API names
    "document", "window", "queryselector", "queryselectorall", "getboundingclientrect",
    "requestanimationframe", "addeventlistener", "innerhtml", "textcontent",
    "classlist", "getcomputedstyle", "const", "let", "var", "function", "return",
    "await", "async", "import", "from", "export", "require", "process", "argv",
    "console", "log", "error", "json", "stringify", "parse", "path", "join", "fs",
    "readfilesync", "writefilesync", "mkdirsync", "existssync", "promises",
    "child", "spawnsync", "execfilesync", "playwright", "chromium", "page",
    "evaluate", "goto", "screenshot", "viewport", "width", "height",
    # python stdlib names
    "argparse", "argumentparser", "add", "argument", "args", "sys", "exit", "def",
    "self", "none", "true", "false", "if", "else", "for", "in", "not", "and", "or",
    "pathlib", "subprocess", "run", "check", "output", "text", "capture", "re",
    "compile", "open", "encoding", "utf", "8", "name", "main",
    # standard seeded PRNG used for deterministic frames: a public algorithm, its
    # name and magic constant are fixed by the algorithm (imul is its Math call)
    "mulberry32", "imul", "x6d2b79f5", "6d2b79f5",
    # placeholder written by normalise() for a command-line flag
    "flag",
    # ffprobe option and field names: fixed by the tool
    "show", "entries", "select", "streams", "stream", "codec", "type", "frame", "rate",
    "duration", "error", "of", "json", "pcm", "s16le", "lavfi", "testsrc2", "ebur128",
    # python and node standard library names: fixed by the runtime
    "os", "dirname", "abspath", "makedirs", "exist", "ok", "insert", "file", "try", "with",
    "as", "math", "min", "max", "floor", "tmpdir", "node", "url", "resolve", "dir",
    "recursive", "force", "rmsync", "mkdirsync", "stdio", "ignore", "pipe", "fileurltopath",
    "pathtofileurl", "readfilesync", "writefilesync", "date", "now", "tofixed", "send",
    "runtime", "expression", "awaitpromise", "returnbyvalue", "target", "createtarget",
    "about", "blank", "emulation", "setdevicemetricsoverride", "devicescalefactor", "mobile",
    # css and html boilerplate: property names and a page skeleton
    "doctype", "html", "head", "meta", "charset", "body", "margin", "padding", "overflow",
    "hidden", "background", "display", "position", "font", "size", "px", "left", "top",
    "monospace", "sans", "serif", "flex", "gap", "white", "space", "pre", "wrap", "svg",
    "xmlns", "http", "www", "w3", "org", "2000", "createelementns", "width", "height",
    "style", "stroke", "linejoin", "round", "cubic", "bezier", "x0", "y0", "x1", "y1",
    "x2", "y2", "lo", "hi", "document", "createtreewalker", "nodefilter", "createrange",
    "selectnodecontents", "nextnode", "parentelement", "getcomputedstyle", "nodetype",
    # ffmpeg filter expressions seen in several projects: motion blur by tmix, trimming
    # silence at both ends, drawbox probes
    "tmix", "eq", "mod", "frames", "silenceremove", "areverse", "periods", "start",
    "threshold", "silence", "drawbox", "iw", "ih", "black",
    # node and javascript idioms: child process imports, timer sleep, split-trim-filter
    "spawn", "spawnsync", "child", "events", "once", "timers", "settimeout", "sleep",
    "trim", "boolean", "split", "number", "h1", "h2", "h3", "h4", "h5", "h6",
    # argparse option idiom, the 0x prefix form of the mulberry32 constant, DOM range
    # walk, and a GSAP tween with a named ease: standard API names and options
    "action", "store", "help", "default", "int", "seed", "0x6d2b79f5", "continue", "range",
    "opacity", "ease", "power2",
    # ffmpeg scale and paletteuse options: fixed by the tool
    "original", "aspect", "ratio", "dither", "bayer",
    # names of our own storyboard gate, scripts/check_timeline.py, and its input
    # timeline.json: the same names occur upstream, reported in the audit DONE
    "scripts", "timeline", "py",
    # canvas drawImage parameter names (source and destination rectangle): MDN names
    "sx", "sy", "sw", "sh", "dx", "dy", "dw", "dh",
    # macos install path of Google Chrome: a path fixed by the vendor
    "applications", "google", "chrome", "app", "contents", "macos",
}
MIN_FREE = 3
# Code hits shorter than CODE_SHORT tokens need this many free words to count:
# a 12 to 19 token window with fewer is one fragment of idiom, not expression.
CODE_SHORT = 20
CODE_SHORT_MIN_FREE = 6
# The allowlist only covers a short shared text. A longer one is a hit however
# plain its words are, unless the idiom rule below clears it: 16 words, 40 tokens.
WORD_ALLOW_MAX = 16
CODE_ALLOW_MAX = 40
# A shared text whose every n-gram occurs in at least this many different
# upstream projects is a common idiom (boilerplate, a standard algorithm), not
# one project's expression.
IDIOM_PROJECTS = 3


def is_free(word):
    return len(word) > 1 and not word.isdigit() and word not in ALLOW_WORDS


def is_common(text, kind="word"):
    """True when the shared text carries too few free words to be expression."""
    if not re.search(r"[A-Za-z0-9]", text.replace("FLAG", "")):
        return True  # only punctuation and flag placeholders: a ruler line, a flag list
    if "4294967296" in text:
        return True  # 2 to the 32: the divisor of every 32-bit seeded PRNG (mulberry32)
    size = len(text.split())
    if size > (CODE_ALLOW_MAX if kind == "code" else WORD_ALLOW_MAX):
        return False
    free = sum(1 for w in re.findall(r"[a-z0-9]+", text.lower()) if is_free(w))
    if kind == "code" and size < CODE_SHORT:
        return free < CODE_SHORT_MIN_FREE
    return free < MIN_FREE


def read_text(path):
    try:
        if path.stat().st_size > MAX_BYTES:
            return None
        return path.read_text(encoding="utf-8")
    except (UnicodeDecodeError, OSError):
        return None


def walk(root):
    for p in sorted(root.rglob("*")):
        if any(part in SKIP_DIRS for part in p.parts) or not p.is_file():
            continue
        if p.suffix.lower() in TEXT_EXT and p.name not in SKIP_NAMES:
            yield p


# Command-line flags are fixed by the tool, so both sides drop them to a
# placeholder: --long-flag[=value] anywhere, and -short flags set off by
# whitespace, quotes or brackets (so "a -b" arithmetic is kept apart from "-v").
FLAG = re.compile(r"(?<![\w-])--[a-z][a-z0-9-]+(?:=[^\s'\",`]+)?"
                  r"|(?<=[\s\"'`\[,])-[A-Za-z][A-Za-z_]*(?=[\s\"'`,\]])")


# Comment rulers (a row of dashes or equals) and import statements carry no
# expression: the first is decoration, the second only names APIs.
RULER = re.compile(r"[-=#*_~/]{4,}")
IMPORT = re.compile(r"\bimport\s+(?:[\w$]+\s*,?\s*)?(?:\{[^}]*\}|\*\s+as\s+\w+)?\s*from\s*['\"][^'\"]+['\"]")


def normalise(text):
    return FLAG.sub(" FLAG ", IMPORT.sub(" import ", RULER.sub(" ", text)))


def words(text):
    text = normalise(text)
    return re.sub(r"[^a-z0-9]+", " ", text.lower()).split()


TOKEN = re.compile(r"[A-Za-z_][A-Za-z0-9_]*|\d+(?:\.\d+)?|\S")


def tokens(text):
    return TOKEN.findall(normalise(text))


def grams(seq, n):
    return [" ".join(seq[i:i + n]) for i in range(len(seq) - n + 1)]


def project_of(label):
    parts = label.split("/")
    return "/".join(parts[:2]) if parts[0] == "video-upstream" else label


def build_index(files, kind):
    """files: list of (label, text). Returns (ngram -> file indexes, ngram -> projects)."""
    index = {}
    projects = {}
    for fi, (_, text) in enumerate(files):
        split, n = KINDS[kind]
        seq = words(text) if split == "word" else tokens(text)
        for g in set(grams(seq, n)):
            lst = index.setdefault(g, [])
            if len(lst) < 8:
                lst.append(fi)
            projects.setdefault(g, set()).add(project_of(files[fi][0]))
    return index, projects


def find_hits(label, text, kind, indexes, sources):
    """Merge adjacent n-gram matches per source file into hits."""
    index, projects = indexes
    split, n = KINDS[kind]
    seq = words(text) if split == "word" else tokens(text)
    gs = grams(seq, n)
    runs = {}  # source index -> [start, last]
    hits = []
    for i, g in enumerate(gs):
        for si in index.get(g, ()):
            r = runs.get(si)
            if r and r[1] == i - 1:
                r[1] = i
            else:
                if r:
                    hits.append((si, r[0], r[1]))
                runs[si] = [i, i]
    hits.extend((si, r[0], r[1]) for si, r in runs.items())
    out = []
    for si, a, b in sorted(set(hits)):
        shared = " ".join(seq[a:b + n])
        idiom = min(len(projects[g]) for g in gs[a:b + 1]) >= IDIOM_PROJECTS
        out.append((kind, label, sources[si][0], shared, idiom))
    return out


def tracked_video_files(root):
    res = subprocess.run(["git", "ls-files", "skills", "teammates"], cwd=root,
                         capture_output=True, text=True, check=True)
    keep = []
    for rel in res.stdout.splitlines():
        if rel.startswith(("skills/video-", "teammates/video-")):
            keep.append(root / rel)
    pipeline = root / "skills/orchestrate/references/video-pipeline.md"
    if pipeline.exists():
        keep.append(pipeline)
    return keep


def load(paths, base):
    out = []
    for p in paths:
        t = read_text(p)
        if t is not None:
            out.append((str(p.relative_to(base)) if base in p.parents else str(p), t))
    return out


SELF_PROSE = ("A scene holds its last caption on screen until the narrator finishes the "
              "closing clause, then the cut lands on the next downbeat of the bed.")
SELF_CODE = """function settle(frames, limit) {
  let streak = 0;
  for (const f of frames) {
    streak = f.delta < limit ? streak + 1 : 0;
    if (streak === 4) return f.index;
  }
  return -1;
}
"""
SELF_COMMON = "ffprobe -v error -show_entries stream=width,height,r_frame_rate -of json"


def self_test():
    """Plant upstream text in temp files. A copy must exit 1, clean text must exit 0."""
    me = str(Path(__file__).resolve())
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        (tmp / "up" / "proj-a").mkdir(parents=True)
        (tmp / "up" / "proj-a" / "guide.md").write_text(
            f"# Guide\n\nIntro text.\n\n{SELF_PROSE}\n\nMore text.\n", encoding="utf-8")
        (tmp / "up" / "proj-a" / "lib.js").write_text(SELF_CODE + f"// {SELF_COMMON}\n", encoding="utf-8")
        (tmp / "ours").mkdir()
        cases = {
            # a sentence copied with other case and punctuation: must be caught
            "sentence.md": ("My notes.\n\n" + SELF_PROSE.upper().replace(",", ";") + "\n", 1, "HIT [prose]"),
            # a code block copied with other spacing: must be caught
            "block.js": (SELF_CODE.replace("  ", "\t"), 1, "HIT [code]"),
            # own words: must pass
            "clean.md": ("Hold the final caption until speech ends. Cut on the next beat.\n", 0, "hits: 0"),
            # a command line made only of tool flags: allowlisted, must pass
            "common.md": (f"Probe with `{SELF_COMMON}` first.\n", 0, "hits: 0"),
        }
        failed = 0
        for name, (text, want, marker) in cases.items():
            (tmp / "ours" / name).write_text(text, encoding="utf-8")
            res = subprocess.run([sys.executable, "-I", me, "--upstream", str(tmp / "up"), "--root",
                                  str(tmp / "ours"), str(tmp / "ours" / name)],
                                 capture_output=True, text=True)
            ok = res.returncode == want and marker in res.stdout
            failed += not ok
            print(f"{'ok  ' if ok else 'FAIL'} {name}: exit {res.returncode}, expected {want}")
    print("self-test passed" if not failed else f"self-test failed: {failed} case(s)")
    return 1 if failed else 0


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--upstream", default="/tmp/video-upstream")
    ap.add_argument("--specs", default=None)
    ap.add_argument("--root", default=".")
    ap.add_argument("--show-common", action="store_true",
                    help="also print the allowlisted hits, longest first, for a manual read")
    ap.add_argument("--self-test", action="store_true")
    ap.add_argument("files", nargs="*")
    a = ap.parse_args()
    if a.self_test:
        return self_test()
    root = Path(a.root).resolve()
    up_root = Path(a.upstream)
    if not up_root.is_dir():
        sys.exit(f"upstream dir missing: {up_root}")
    ours_paths = [Path(f).resolve() for f in a.files] or tracked_video_files(root)
    ours = load(ours_paths, root)

    up_files = load(list(walk(up_root)), up_root.parent)
    up_code = [(l, t) for l, t in up_files if Path(l).suffix.lower() in CODE_EXT]
    sources = {"word": up_files, "code": up_code}
    if a.specs:
        sp = load(list(walk(Path(a.specs))), Path(a.specs).parent)
        sources["word"] = up_files + sp
    sources["prose"] = sources["word"]
    idx_of = {k: build_index(sources[k], k) for k in KINDS}

    found = {}  # (kind, ours, shared) -> upstream paths
    skipped = {}  # the same for allowlisted hits, kept for --show-common
    common = 0
    for label, text in ours:
        kinds = ["word", "code"] if Path(label).suffix.lower() in CODE_EXT else ["prose"]
        for kind in kinds:
            for k, ours_l, src_l, shared, idiom in find_hits(label, text, kind, idx_of[kind], sources[kind]):
                if idiom or is_common(shared, k):
                    common += 1
                    if a.show_common:
                        skipped.setdefault((k, ours_l, shared), []).append(src_l)
                    continue
                found.setdefault((k, ours_l, shared), []).append(src_l)
    for (k, ours_l, shared), srcs in sorted(found.items(), key=lambda kv: -len(kv[0][2])):
        print(f"HIT [{k}] ours={ours_l}\n    upstream={', '.join(sorted(set(srcs)))}\n    {shared}\n")
    for (k, ours_l, shared), srcs in sorted(skipped.items(), key=lambda kv: -len(kv[0][2])):
        print(f"COMMON [{k}] ours={ours_l}\n    upstream={', '.join(sorted(set(srcs)))}\n    {shared}\n")
    bad = len(found)
    print(f"files checked: {len(ours)}; upstream files: {len(up_files)}; "
          f"hits: {bad}; allowlisted: {common}")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
