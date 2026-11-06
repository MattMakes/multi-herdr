"""The output overlap scan of `intake verify` (spec 6.10): does a rewritten file
hold wording from a source text file or from a handed-off spec?

Ported from this repo's scripts/dev/overlap_check.py with the same windows and
thresholds:
- prose (.md, .txt and other non-code files): word 6-grams, lowercase,
  punctuation stripped;
- code files: word 8-grams and token 12-grams, identifiers and case kept.
Adjacent matching n-grams merge into one hit. A hit is common, and ignored,
when it has too few free words (words not in the allowlist): under 3 for a
short text, and for a code hit under 20 tokens, under 6. A shared text longer
than 16 words (40 tokens for code) is always a hit. The allowlist keeps only
the generic language and standard-library names of the original; its
ffmpeg and video names are dropped.
"""

from __future__ import annotations

import re

WORD_N = 8
CODE_N = 12
PROSE_N = 6
KINDS = {"prose": ("word", PROSE_N), "word": ("word", WORD_N), "code": ("code", CODE_N)}
CODE_EXT = frozenset(".ts .tsx .js .mjs .cjs .py .html .css .sh .hs .rs".split())

ALLOW_WORDS = frozenset({
    # standard web and Node API names
    "document", "window", "queryselector", "queryselectorall", "addeventlistener", "innerhtml",
    "textcontent", "const", "let", "var", "function", "return", "await", "async", "import", "from",
    "export", "require", "process", "argv", "console", "log", "error", "json", "stringify", "parse",
    "path", "join", "fs", "readfilesync", "writefilesync", "mkdirsync", "existssync", "promises",
    "child", "spawnsync", "execfilesync",
    # python stdlib names
    "argparse", "argumentparser", "add", "argument", "args", "sys", "exit", "def", "self", "none",
    "true", "false", "if", "else", "for", "in", "not", "and", "or", "pathlib", "subprocess", "run",
    "check", "output", "text", "capture", "re", "compile", "open", "encoding", "utf", "8", "name",
    "main",
    # placeholder written by normalise() for a command-line flag
    "flag",
    # python and node standard library names: fixed by the runtime
    "os", "dirname", "abspath", "makedirs", "exist", "ok", "insert", "file", "try", "with", "as",
    "math", "min", "max", "floor", "tmpdir", "node", "url", "resolve", "dir", "recursive", "force",
    "rmsync", "stdio", "ignore", "pipe", "fileurltopath", "pathtofileurl", "date", "now",
    # node and javascript idioms
    "spawn", "events", "once", "timers", "settimeout", "sleep", "trim", "boolean", "split", "number",
    "h1", "h2", "h3", "h4", "h5", "h6",
    # argparse option idiom
    "action", "store", "help", "default", "int", "continue", "range",
})
MIN_FREE = 3
CODE_SHORT = 20
CODE_SHORT_MIN_FREE = 6
WORD_ALLOW_MAX = 16
CODE_ALLOW_MAX = 40

FLAG = re.compile(r"(?<![\w-])--[a-z][a-z0-9-]+(?:=[^\s'\",`]+)?"
                  r"|(?<=[\s\"'`\[,])-[A-Za-z][A-Za-z_]*(?=[\s\"'`,\]])")
RULER = re.compile(r"[-=#*_~/]{4,}")
IMPORT = re.compile(r"\bimport\s+(?:[\w$]+\s*,?\s*)?(?:\{[^}]*\}|\*\s+as\s+\w+)?\s*from\s*['\"][^'\"]+['\"]")
TOKEN = re.compile(r"[A-Za-z_][A-Za-z0-9_]*|\d+(?:\.\d+)?|\S")


def is_free(word: str) -> bool:
    return len(word) > 1 and not word.isdigit() and word not in ALLOW_WORDS


def is_common(text: str, kind: str = "word") -> bool:
    """True when the shared text carries too few free words to be expression."""
    if not re.search(r"[A-Za-z0-9]", text.replace("FLAG", "")):
        return True
    size = len(text.split())
    if size > (CODE_ALLOW_MAX if kind == "code" else WORD_ALLOW_MAX):
        return False
    free = sum(1 for w in re.findall(r"[a-z0-9]+", text.lower()) if is_free(w))
    if kind == "code" and size < CODE_SHORT:
        return free < CODE_SHORT_MIN_FREE
    return free < MIN_FREE


def normalise(text: str) -> str:
    return FLAG.sub(" FLAG ", IMPORT.sub(" import ", RULER.sub(" ", text)))


def words(text: str) -> list[str]:
    return re.sub(r"[^a-z0-9]+", " ", normalise(text).lower()).split()


def tokens(text: str) -> list[str]:
    return TOKEN.findall(normalise(text))


def _seq(text: str, kind: str) -> list[str]:
    return words(text) if KINDS[kind][0] == "word" else tokens(text)


def grams(seq: list[str], n: int) -> list[str]:
    return [" ".join(seq[i:i + n]) for i in range(len(seq) - n + 1)]


def is_code(name: str) -> bool:
    dot = name.rfind(".")
    return dot > 0 and name[dot:].lower() in CODE_EXT


class Index:
    """The n-grams of every source text, per kind. `sources` is [(label, text)];
    `code` is the labels of the source code files."""

    def __init__(self, sources: list[tuple[str, str]], code: set[str]):
        by_kind = {"prose": sources, "word": sources, "code": [s for s in sources if s[0] in code]}
        self.labels = {k: [label for label, _ in v] for k, v in by_kind.items()}
        self.grams: dict[str, dict[str, list[int]]] = {}
        for kind, items in by_kind.items():
            idx: dict[str, list[int]] = {}
            for i, (_, text) in enumerate(items):
                for g in set(grams(_seq(text, kind), KINDS[kind][1])):
                    idx.setdefault(g, []).append(i)
            self.grams[kind] = idx

    def hits(self, name: str, text: str) -> list[dict]:
        """Every non-common shared text between `text` (file `name`) and a source."""
        out = []
        for kind in (("word", "code") if is_code(name) else ("prose",)):
            n = KINDS[kind][1]
            seq = _seq(text, kind)
            runs: dict[int, list[int]] = {}
            spans = []
            for i, g in enumerate(grams(seq, n)):
                for si in self.grams[kind].get(g, ()):
                    r = runs.get(si)
                    if r and r[1] == i - 1:
                        r[1] = i
                    else:
                        if r:
                            spans.append((si, r[0], r[1]))
                        runs[si] = [i, i]
            spans.extend((si, r[0], r[1]) for si, r in runs.items())
            for si, a, b in sorted(set(spans)):
                shared = " ".join(seq[a:b + n])
                if not is_common(shared, kind):
                    out.append({"kind": kind, "source": self.labels[kind][si], "shared": shared})
        return out
