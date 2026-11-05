"""The deterministic scanner of `intake scan` (spec 4.2): the rule framework, plus
the INJ rules (text aimed at the agent) and the SCR rules (scripts).

The scanner reads quarantined files as bytes and text only. It never executes,
imports or loads them, and it starts no process.

Text rules apply to every text file. Script rules apply to a "code unit": a
file with a script extension or a shebang, or a fenced Markdown code block
whose language is a shell or programming language. A hit is

    {id, rule, severity, file, line, evidence, malicious}

`rule` is the rule id (INJ-04), `id` is the hit id (scan-0001). `file` and
`evidence` go through the evidence-escape function; `evidence` is wrapped as
{"untrusted": ...}. One hit per rule, file and line: the most severe wins.

Hostile code points are built with chr() or \\U escapes made at run time, so
this file stays printable ASCII. The Anthropic key variable name is built from
2 parts (spec 12.4): its full literal is in no file of the skill.
"""

from __future__ import annotations

import json
import math
import os
import re
import tempfile
from collections import namedtuple
from pathlib import Path

from . import evidence

SEVERITIES = ("critical", "high", "medium", "low", "info")
_RANK = {s: i for i, s in enumerate(SEVERITIES)}

KEY_PREFIX = "ANTHROPIC"
KEY_SUFFIX = "_API_KEY"
KEY_NAME = KEY_PREFIX + KEY_SUFFIX

Source = namedtuple("Source", "rel name lines")
Unit = namedtuple("Unit", "lines makefile")  # lines: [(line number, text)]

SCRIPT_EXTS = frozenset(
    ".sh .bash .zsh .fish .ksh .py .pyw .js .mjs .cjs .ts .tsx .jsx .rb .pl .pm .php "
    ".ps1 .psm1 .lua .go .rs .swift .java .kt .kts .c .cc .cpp .h .hpp .cs .bat .cmd "
    ".applescript .scpt .mk".split()
)
MAKEFILE_NAMES = frozenset(("Makefile", "makefile", "GNUmakefile"))
MARKDOWN_EXTS = frozenset((".md", ".markdown", ".mdx"))
HIDDEN_EXTS = MARKDOWN_EXTS | {".html", ".htm", ".xhtml", ".svg"}
FENCE_LANGS = frozenset(
    "sh bash zsh shell console shell-session shellsession fish ksh powershell ps1 pwsh "
    "bat cmd batch python py python3 javascript js node mjs cjs typescript ts tsx jsx "
    "ruby rb perl pl php lua go golang rust rs swift java kotlin kt c cpp c++ cs csharp "
    "makefile make mk dockerfile applescript osascript".split()
)


def _cp_class(*ranges: tuple[int, int]) -> re.Pattern:
    parts = [f"\\U{a:08x}" if a == b else f"\\U{a:08x}-\\U{b:08x}" for a, b in ranges]
    return re.compile("[" + "".join(parts) + "]")


BOM = chr(0xFEFF)
INVISIBLE = _cp_class(
    (0x200B, 0x200F), (0x202A, 0x202E), (0x2060, 0x2064), (0x2066, 0x2069),
    (0xFEFF, 0xFEFF), (0x00AD, 0x00AD), (0x180E, 0x180E),
)
BIDI = _cp_class((0x202A, 0x202E), (0x2066, 0x2069))
TAGS = _cp_class((0xE0000, 0xE007F))

# ---- INJ patterns -----------------------------------------------------------

AGENT_IMPERATIVE = re.compile(
    r"\b(?:ignore|disregard|forget|override)\b[^\n]{0,40}?"
    r"\b(?:previous|prior|above|earlier|all|any|system|developer)\b[^\n]{0,20}?"
    r"\b(?:instructions?|prompts?|rules|directions|guidelines)\b"
    r"|\b(?:do\s+not|don'?t|never)\s+(?:tell|mention|show|inform|notify|alert)\b[^\n]{0,30}?"
    r"\b(?:user|operator|human)\b"
    r"|\bsilently\b|\bwithout\s+asking\b|\byou\s+are\s+now\b|\bnew\s+instructions\b"
    r"|\bsystem\s+prompt\b|\bdeveloper\s+message\b",
    re.I,
)
IMPERATIVE = re.compile(
    r"(?:^|[.!?:]\s+|\n\s*)(?:run|execute|delete|remove|send|upload|post|read|write|copy|"
    r"install|download|fetch|open|ignore|disable|do\s+not|don'?t|never|always)\b",
    re.I,
)
COMMENT = re.compile(r"<!--(.*?)(?:-->|\Z)", re.S)
LINK_COMMENT = re.compile(r"^ {0,3}\[[^\]]*\]:\s*<?#>?(?:\s+(.*))?$")
HTML_ELEMENT = re.compile(r"<([A-Za-z][\w-]*)\b([^>]*)>(.*?)</\1\s*>", re.S)
DETAILS = re.compile(r"<details\b[^>]*>(.*?)(?:</details\s*>|\Z)", re.S | re.I)
HIDDEN_ATTR = re.compile(
    r"(?:^|\s)hidden\b|display\s*:\s*none|visibility\s*:\s*hidden"
    r"|font-size\s*:\s*0+(?:\.0+)?(?:px|em|rem|pt|%)?\s*(?=[;\"'}]|$)",
    re.I,
)
_WHITE = r"(?:white|#fff(?:fff)?\b|rgb\(\s*255\s*,\s*255\s*,\s*255\s*\))"
WHITE_TEXT = re.compile(r"(?<![-\w])color\s*:\s*" + _WHITE, re.I)
WHITE_BACK = re.compile(r"background(?:-color)?\s*:\s*" + _WHITE, re.I)

AGENT_CONFIG = re.compile(
    r"~/\.claude\b|\$\{?HOME\}?/\.claude\b|\bsettings(?:\.local)?\.json\b|\.claude/hooks\b"
    r"|\"hooks\"\s*:|^\s*hooks\s*:|\b(?:PreToolUse|PostToolUse|UserPromptSubmit|SessionStart)\b"
    r"|\bCLAUDE\.md\b|\bAGENTS\.md\b|\bMEMORY\.md\b|\.mcp\.json\b|--dangerously-skip-permissions"
    r"|\bpermission_?mode\b|\ballowedTools\b|\.claude/skills/|\.agents/skills/|\.codex/"
    r"|\bteammates/|\.(?:bashrc|bash_profile|zshrc|zprofile|zshenv|profile|config/fish)\b"
    r"|\.gitconfig\b|\bauthorized_keys\b",
    re.I,
)
PRELOAD = re.compile(r"!`[^`\n]+`")
REMOTE = re.compile(
    r"\b(?:fetch|read|open|load|visit|download|get|curl|retrieve)\b[^\n]{0,80}?\bhttps?://\S+"
    r"[^\n]{0,80}?\b(?:follow|apply|execute|run|obey)\b"
    r"|\bhttps?://\S+[^\n]{0,60}?\b(?:as|are|contains?)\s+(?:your\s+|the\s+|new\s+)?"
    r"(?:instructions|commands|prompt|rules)\b"
    r"|\binstructions?\s+(?:at|from|in)\s+<?https?://",
    re.I,
)
BASE64_RUN = re.compile(r"[A-Za-z0-9+/_-]{200,}={0,2}")
HEX_RUN = re.compile(r"(?:[0-9a-fA-F]{2}){100,}")
DATA_URI = re.compile(r"\bdata:(?!image/)[\w.+-]+/[\w.+-]+[;,]", re.I)
INLINE_CODE = re.compile(r"`([^`\n]+)`")
LETTERS = re.compile(r"[^\W\d_]+")

FRONT_KEY = re.compile(r"^([A-Za-z_][\w-]*)\s*:(.*)$")
BLOCK_MARKS = frozenset((">", "|", ">-", "|-", ">+", "|+"))
BASH_UNSCOPED = re.compile(r"\bBash\b(?!\s*\()|\bBash\(\s*\*(?:\s*:\s*\*)?\s*\)")
CLAIMS_EVERY_TASK = re.compile(r"\b(?:always\s+use|use\s+for\s+every|use\s+before\s+any)\b", re.I)
KEY_REF = re.compile(r"\b" + KEY_NAME + r"\b")

# ---- SCR patterns -----------------------------------------------------------

NETWORK = re.compile(
    r"(?<![\w./-])(?:curl|wget)\b|(?<![\w./-])(?:nc|ncat|netcat|ssh|scp)\s+[^\s|;&]"
    r"|(?<![\w./-])rsync\b[^\n]*\s[\w.@-]+:|\bfetch\s*\(|\bhttps?\.(?:request|get)\b"
    r"|\bXMLHttpRequest\b|\bWebSocket\b|\burllib\b|\brequests\.|\bhttpx\b|\bsocket\.|\bdns\."
    r"|\bgit\s+(?:clone|fetch|push)\b|\bgh\s+api\b"
)
ENV_SECRET = re.compile(
    r"(?:\$\{?|\$env:|\bos\.environ(?:\.get)?\s*[\[(]\s*['\"]|\bos\.getenv\s*\(\s*['\"]"
    r"|\bprocess\.env\.|\bprocess\.env\[\s*['\"]|\bENV\[\s*['\"]|\bgetenv\s*\(\s*['\"]"
    r"|\bDeno\.env\.get\s*\(\s*['\"])"
    r"[A-Za-z_]*(?:TOKEN|SECRET|KEY|PASSWORD|PASSWD|CREDENTIAL|AUTH|COOKIE|SESSION)\w*",
    re.I,
)
KEYCHAIN = re.compile(r"\bsecurity\s+find-[\w-]+-password\b|Library/Keychains")
CRED_FILE = re.compile(
    r"\.ssh\b|\.aws\b|\.config/gh\b|\.netrc\b|\.npmrc\b|\.pypirc\b|\.docker/config\.json"
    r"|\.kube\b|Library/Application Support/(?:Google/Chrome|BraveSoftware|Firefox|Microsoft Edge)"
    r"|\.mozilla/firefox|\.config/(?:google-chrome|chromium|BraveSoftware)"
)
WHOLE_ENV = re.compile(
    r"(?:^|[|;&]|\$\(|`)\s*(?:env|printenv)\s*(?:$|[|>;&)`])"
    r"|\bos\.environ\.(?:items|keys|values|copy)\s*\(|\bin\s+os\.environ\b|\(\s*os\.environ\s*\)"
    r"|\bObject\.(?:keys|entries|values|assign)\s*\(\s*(?:\{\}\s*,\s*)?process\.env\b"
    r"|\bJSON\.stringify\s*\(\s*process\.env\b|\.\.\.process\.env\b|\bin\s+process\.env\b"
)
GH_TOKEN = re.compile(r"\bgh\s+auth\s+token\b")
SECRET_CLASSES = (
    ("keychain", KEYCHAIN), ("credfile", CRED_FILE), ("wholeenv", WHOLE_ENV),
    ("ghtoken", GH_TOKEN), ("env", ENV_SECRET),
)
MALICIOUS_SECRETS = frozenset(("credfile", "keychain", "wholeenv"))

PIPE_TO_SHELL = re.compile(
    r"(?<![\w-])(?:curl|wget)\b[^\n;&]*\|\s*(?:sudo\s+)?(?:env\s+)?"
    r"(?:sh|bash|zsh|dash|python[\d.]*|node|perl|ruby)\b"
    r"|\b(?:sh|bash|zsh)\s+<\(\s*(?:curl|wget)\b"
    r"|\b(?:iex|Invoke-Expression)\b[^\n]*\b(?:iwr|irm|Invoke-WebRequest|Invoke-RestMethod|DownloadString)\b"
    r"|\beval\s+\"?\$\(\s*(?:curl|wget)\b",
    re.I,
)
DYNAMIC = re.compile(
    r"(?<![\w.$])eval\b|(?<![\w.$])exec\s*\(|\bnew\s+Function\s*\("
    r"|child_process['\"]?\)?\.exec(?:Sync)?\s*\(|\bexecSync\s*\("
    r"|\bsubprocess\.\w+\((?=[^\n]*\bshell\s*=\s*True)|\bos\.system\s*\("
    r"|\b(?:sh|bash|zsh)\s+-c\s+(?=\S)|\bpickle\.loads?\s*\("
    r"|\byaml\.load\s*\((?![^\n]*Loader\s*=\s*(?:yaml\.)?C?SafeLoader)"
    r"|\bimportlib\.import_module\s*\(|\b__import__\s*\("
)
LITERAL_ARG = re.compile(r"""\s*\(?\s*[rbuRBU]?(?:'[^'\\\n]*'|"[^"\\$`{}%\n]*")\s*(?:[),;]|$)""")
DECODE = re.compile(
    r"\bbase64\s+(?:-d|--decode|-D)\b|\bb64decode\b|\batob\s*\(|\bBuffer\.from\([^)]*base64"
    r"|\bfromCharCode\b|\bzlib\.(?:decompress|inflate)|\binflateSync\b|\bxxd\s+-r"
    r"|\bbytes\.fromhex\b|\bunhexlify\b|\bcodecs\.decode\b|\bgunzip\b",
    re.I,
)
RUNS_PIPE = re.compile(r"\|\s*(?:sudo\s+)?(?:sh|bash|zsh|dash|python[\d.]*|node|perl|ruby)\b")
ASSIGN = re.compile(r"^\s*(?:export\s+|const\s+|let\s+|var\s+|local\s+)?([A-Za-z_]\w*)\s*=")
REDIRECT = re.compile(r">\s*['\"]?([^\s'\";|&>]+)")

_OUTSIDE = (
    r"(?:~|\$HOME|\$\{HOME\})/(?:\.[\w.-]+|Library/LaunchAgents)|/Library/Launch(?:Agents|Daemons)"
    r"|\.claude/|\.codex/|\.agents/|(?<![\w.])/etc/|/usr/local/|\.git/hooks|/skills/"
)
OUTSIDE = re.compile(_OUTSIDE)
WRITE_REDIRECT = re.compile(r"(?:>>?|\btee\s+(?:-a\s+)?)\s*['\"]?(?:" + _OUTSIDE + ")")
WRITE_COPY = re.compile(r"\b(?:cp|mv|ln|install|rsync)\s[^\n;&|]*\s['\"]?(?:" + _OUTSIDE + r")[^\s;&|]*['\"]?\s*(?:$|[;&|])")
WRITE_CALL = re.compile(
    r"\b(?:mkdir|touch)\s|\bsed\s+-i|\bopen\([^)]*,\s*['\"][wax]|\bwriteFile|\bappendFile|\bwrite_text\b"
    r"|\bwrite_bytes\b|\bcopyfile\b|\bcopytree\b|\bshutil\.(?:copy|move)|\bcreateWriteStream\b"
)
PIPE_CRONTAB = re.compile(r"\|\s*crontab\s+-")

PERSIST = re.compile(r"\blaunchctl\b|\bcrontab\s+-(?:\s|$)|\bsystemctl\s+--user\b|\bcmdclass\s*=")
NPM_HOOK = re.compile(r"\"(?:preinstall|install|postinstall|prepare)\"\s*:")
PRIVILEGE = re.compile(
    r"(?<![\w-])sudo\b|\bchmod\s+(?:-\w+\s+)*(?:[ugoa]*\+\w*s|[2467][0-7]{3}\b)"
    r"|\bxattr\b[^\n]*com\.apple\.quarantine|\bspctl\b|\bcsrutil\b|\bcodesign\b[^\n]*--remove-signature"
)
RM_RECURSIVE = re.compile(r"(?<![\w-])rm\s+((?:-[\w-]+\s+)+)(['\"]?[^\s;&|]+)")
TEMP_PATH = re.compile(r"tmp|temp|mktemp|/var/folders", re.I)
GIT_DESTRUCTIVE = re.compile(
    r"\bgit\s+push\b[^\n]*(?:--force\b|--force-with-lease\b|\s-f\b)|\bgit\s+reset\s+--hard\b"
    r"|\bgit\s+clean\s+-(?=\w*f)(?=\w*d)(?=\w*x)\w+|\bdd\b[^\n]*\bof=/dev/"
)
HEX_ESCAPES = re.compile(r"(?:\\x[0-9a-fA-F]{2}){41,}")
QUOTED_LONG = re.compile(r"""(['"])([^'"\n]{101,})\1""")


# ---- framework ----------------------------------------------------------------


def _hit(rule: str, severity: str, src: Source, line: int, text: str, malicious: bool = False) -> dict:
    return {
        "rule": rule,
        "severity": severity,
        "file": evidence.escape(src.rel),
        "line": line,
        "evidence": evidence.untrusted(text.strip()),
        "malicious": malicious,
    }


def _numbered(src: Source) -> list[tuple[int, str]]:
    return list(enumerate(src.lines, 1))


def _line_rule(src: Source, rule: str, severity: str, pattern: re.Pattern) -> list[dict]:
    return [_hit(rule, severity, src, n, line) for n, line in _numbered(src) if pattern.search(line)]


def _better(a: dict, b: dict) -> dict:
    """The hit to keep when 2 share rule, file and line: malicious, then the more severe."""
    ka = (not a["malicious"], _RANK[a["severity"]])
    kb = (not b["malicious"], _RANK[b["severity"]])
    return b if kb < ka else a


def finish(hits: list[dict]) -> list[dict]:
    """One hit per (rule, file, line), sorted, with ids scan-0001, scan-0002, ..."""
    kept: dict[tuple, dict] = {}
    for h in hits:
        key = (h["file"], h["line"], h["rule"])
        kept[key] = _better(kept[key], h) if key in kept else h
    out = []
    for i, key in enumerate(sorted(kept), 1):
        out.append({"id": f"scan-{i:04d}", **kept[key]})
    return out


def read_text(path: Path) -> str | None:
    """The file as text, or None for binary data (a NUL byte in the first 8 KB)."""
    with open(path, "rb") as f:
        data = f.read()
    if b"\0" in data[:8192]:
        return None
    return data.decode("utf-8", errors="replace")


def walk(root: Path) -> list[tuple[str, Path]]:
    """Every regular file under `root`, sorted; symlinks are never followed."""
    out = []
    for d, dirs, names in os.walk(root, followlinks=False):
        dirs.sort()
        for name in sorted(names):
            p = Path(d) / name
            if p.is_symlink() or not p.is_file():
                continue
            out.append((p.relative_to(root).as_posix(), p))
    return out


def scan_tree(root: Path) -> list[dict]:
    hits: list[dict] = []
    for rel, path in walk(root):
        text = read_text(path)
        if text is not None:
            hits.extend(scan_file(rel, text))
    return finish(hits)


def scan_file(rel: str, text: str) -> list[dict]:
    """Every INJ and SCR hit in one text file (not yet numbered: see `finish`)."""
    if text.startswith(BOM):
        text = text[1:]  # U+FEFF at byte 0 is a BOM, not INJ-01
    lines = [ln[:-1] if ln.endswith("\r") else ln for ln in text.split("\n")]
    src = Source(rel, rel.rsplit("/", 1)[-1], lines)
    units, code_lines = code_units(src)
    return text_hits(src, code_lines) + script_hits(src, units)


def write_jsonl(path: Path, hits: list[dict]) -> None:
    """Atomic write: one JSON object per line."""
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=path.parent, prefix=".tmp-", suffix=".jsonl")
    with os.fdopen(fd, "w", encoding="utf-8") as f:
        for h in hits:
            f.write(json.dumps(h, ensure_ascii=True) + "\n")
    os.replace(tmp, path)


# ---- code units ---------------------------------------------------------------

FENCE = re.compile(r"^ {0,3}(`{3,}|~{3,})\s*([^\s`]*)")


def _suffix(name: str) -> str:
    return os.path.splitext(name)[1].lower()


def is_script(src: Source) -> bool:
    if _suffix(src.name) in SCRIPT_EXTS or src.name in MAKEFILE_NAMES:
        return True
    return bool(src.lines) and src.lines[0].startswith("#!")


def _closes(line: str, fence: str) -> bool:
    s = line.strip()
    return len(s) >= len(fence) and set(s) == {fence[0]}


def fenced_blocks(lines: list[str]) -> list[tuple[str, list[tuple[int, str]]]]:
    """Fenced Markdown code blocks as (language, [(line number, text)]). An unclosed
    fence runs to the end of the file."""
    blocks: list[tuple[str, list[tuple[int, str]]]] = []
    current = None
    for n, line in enumerate(lines, 1):
        if current is None:
            m = FENCE.match(line)
            if m:
                current = (m.group(1), m.group(2).strip("{}.").lower(), [])
            continue
        if _closes(line, current[0]):
            blocks.append((current[1], current[2]))
            current = None
        else:
            current[2].append((n, line))
    if current is not None:
        blocks.append((current[1], current[2]))
    return blocks


def code_units(src: Source) -> tuple[list[Unit], set[int]]:
    """The code units of a file and the line numbers inside any code (for INJ-08)."""
    if is_script(src):
        lines = _numbered(src)
        return [Unit(lines, src.name in MAKEFILE_NAMES or _suffix(src.name) == ".mk")], {n for n, _ in lines}
    if _suffix(src.name) not in MARKDOWN_EXTS:
        return [], set()
    units, code_lines = [], set()
    for lang, body in fenced_blocks(src.lines):
        code_lines.update(n for n, _ in body)
        if lang in FENCE_LANGS:
            units.append(Unit(body, lang in ("makefile", "make", "mk")))
    return units, code_lines


# ---- INJ rules ----------------------------------------------------------------


def text_hits(src: Source, code_lines: set[int]) -> list[dict]:
    hits = _line_rule(src, "INJ-01", "high", INVISIBLE)
    hits += [_hit("INJ-02", "critical", src, n, ln, True) for n, ln in _numbered(src) if TAGS.search(ln)]
    regions = hidden_regions(src, code_lines)
    hits += _inj03(src, regions)
    hits += _inj04(src, regions)
    hits += _line_rule(src, "INJ-05", "high", AGENT_CONFIG)
    if src.name == "SKILL.md":
        hits += _line_rule(src, "INJ-06", "high", PRELOAD)
        hits += _inj09(src)
    hits += _line_rule(src, "INJ-07", "high", REMOTE)
    hits += _inj08(src, code_lines)
    hits += _line_rule(src, "SCR-03", "critical", KEY_REF)  # any reference, in any text file
    return hits


Region = namedtuple("Region", "start end text kind")


def _line_of(text: str, offset: int) -> int:
    return text.count("\n", 0, offset) + 1


def _element_regions(text: str) -> list[tuple[int, int, str, str]]:
    out = []
    for m in HTML_ELEMENT.finditer(text):
        attrs = m.group(2)
        hidden = HIDDEN_ATTR.search(attrs) or (WHITE_TEXT.search(attrs) and WHITE_BACK.search(attrs))
        if hidden:
            out.append((m.start(), m.end(), m.group(3), "style"))
    for m in DETAILS.finditer(text):
        out.append((m.start(), m.end(), m.group(1), "details"))
    return out


def hidden_regions(src: Source, code_lines: set[int]) -> list[Region]:
    """Text a reader of the rendered Markdown or HTML does not see (INJ-03)."""
    if _suffix(src.name) not in HIDDEN_EXTS:
        return []
    text = "\n".join(src.lines)
    spans = [(m.start(), m.end(), m.group(1), "comment") for m in COMMENT.finditer(text)]
    spans += _element_regions(text)
    regions = []
    for start, end, body, kind in spans:
        first = _line_of(text, start)
        if first not in code_lines:
            regions.append(Region(first, _line_of(text, end), body, kind))
    for n, line in _numbered(src):
        m = LINK_COMMENT.match(line)
        if m and n not in code_lines:
            regions.append(Region(n, n, m.group(1) or "", "link-comment"))
    return regions


def _inj03(src: Source, regions: list[Region]) -> list[dict]:
    hits = []
    for r in regions:
        flat = r.text.replace("\n", " ")
        agent = AGENT_IMPERATIVE.search(flat)
        if r.kind == "details" and not agent and not IMPERATIVE.search(r.text):
            continue  # a <details> block is a finding only when it holds an imperative
        hits.append(_hit("INJ-03", "high" if agent else "medium", src, r.start, flat))
    return hits


def _decode_tags(line: str) -> str:
    return "".join(chr(ord(c) - 0xE0000) for c in line if 0xE0020 <= ord(c) <= 0xE007E)


def _visible_imperative(line: str) -> tuple[bool, bool]:
    """(found, hidden) for an INJ-04 phrase on a visible line. The phrase counts as
    hidden when invisible code points split it, bidi controls reorder the line, or
    it is spelled in tag characters."""
    if AGENT_IMPERATIVE.search(_decode_tags(line)):
        return True, True
    if not AGENT_IMPERATIVE.search(INVISIBLE.sub("", line)):
        return False, False
    return True, bool(BIDI.search(line)) or not AGENT_IMPERATIVE.search(line)


def _inj04(src: Source, regions: list[Region]) -> list[dict]:
    hits, covered = [], set()
    for r in regions:
        covered.update(range(r.start, r.end + 1))
        flat = INVISIBLE.sub("", r.text.replace("\n", " "))
        if AGENT_IMPERATIVE.search(flat):
            hits.append(_hit("INJ-04", "critical", src, r.start, flat, True))
    for n, line in _numbered(src):
        if n in covered:
            continue
        found, hidden = _visible_imperative(line)
        if found:
            hits.append(_hit("INJ-04", "critical" if hidden else "high", src, n, line, hidden))
    return hits


def _mixed_script(text: str) -> bool:
    for word in LETTERS.findall(text):
        latin = any(c.isascii() for c in word)
        other = any(0x0370 <= ord(c) <= 0x04FF for c in word)
        if latin and other:
            return True
    return False


def _encoded(line: str, in_code: bool) -> bool:
    if BASE64_RUN.search(line) or HEX_RUN.search(line) or DATA_URI.search(line):
        return True
    commands = [line] if in_code else INLINE_CODE.findall(line)
    return any(_mixed_script(c) for c in commands)


def _inj08(src: Source, code_lines: set[int]) -> list[dict]:
    hits = []
    for n, line in _numbered(src):
        in_code = n in code_lines
        if _encoded(line, in_code):
            hits.append(_hit("INJ-08", "high" if in_code else "medium", src, n, line))
    return hits


def frontmatter(lines: list[str]) -> dict[str, tuple[int, str]]:
    """Top-level frontmatter keys of a SKILL.md: key -> (line number, value text).
    A block or list value is joined onto one line. The first copy of a key wins."""
    if not lines or lines[0].strip() != "---":
        return {}
    keys: dict[str, list] = {}
    current = None
    for n, line in enumerate(lines[1:], 2):
        if line.strip() in ("---", "..."):
            break
        m = FRONT_KEY.match(line)
        if m:
            current = m.group(1).lower()
            keys.setdefault(current, [n, []])[1].append(m.group(2).strip())
        elif current and line.strip():
            keys[current][1].append(line.strip())
    return {k: (n, " ".join(p for p in parts if p and p not in BLOCK_MARKS)) for k, (n, parts) in keys.items()}


def _inj09(src: Source) -> list[dict]:
    fm = frontmatter(src.lines)
    hits = []
    tools = fm.get("allowed-tools") or fm.get("allowed_tools")
    if tools and BASH_UNSCOPED.search(tools[1]):
        hits.append(_hit("INJ-09", "medium", src, tools[0], "allowed-tools: " + tools[1]))
    for key, severity in (("hooks", "high"), ("model", "medium")):
        if key in fm:
            hits.append(_hit("INJ-09", severity, src, fm[key][0], f"{key}: {fm[key][1]}"))
    desc = fm.get("description")
    if desc and (len(desc[1].strip("'\"")) > 1024 or CLAIMS_EVERY_TASK.search(desc[1])):
        hits.append(_hit("INJ-09", "medium", src, desc[0], "description: " + desc[1]))
    return hits


# ---- SCR rules ----------------------------------------------------------------


def secret_class(line: str) -> str | None:
    for name, pattern in SECRET_CLASSES:
        if pattern.search(line):
            return name
    return None


def _dynamic(line: str) -> str | None:
    m = DYNAMIC.search(line)
    if not m:
        return None
    return "medium" if LITERAL_ARG.match(line, m.end()) else "high"


def _writes_outside(line: str) -> str | None:
    if WRITE_REDIRECT.search(line) or WRITE_COPY.search(line) or PIPE_CRONTAB.search(line):
        return "high"
    if WRITE_CALL.search(line) and OUTSIDE.search(line):
        return "high"
    return None


def _destructive(line: str) -> str | None:
    if GIT_DESTRUCTIVE.search(line):
        return "medium"
    for m in RM_RECURSIVE.finditer(line):
        flags = m.group(1).split()
        short = "".join(f[1:] for f in flags if not f.startswith("--"))
        recursive = "r" in short.lower() or "--recursive" in flags
        force = "f" in short or "--force" in flags
        if recursive and force and not TEMP_PATH.search(m.group(2)):
            return "medium"
    return None


def entropy(s: str) -> float:
    n = len(s)
    return -sum(c / n * math.log2(c / n) for c in (s.count(ch) for ch in set(s)))


def _obfuscated(line: str) -> str | None:
    if len(line) > 1000 or HEX_ESCAPES.search(line):
        return "high"
    if any(entropy(m.group(2)) > 5.0 for m in QUOTED_LONG.finditer(line)):
        return "high"
    return None


def _regex(pattern: re.Pattern, severity: str):
    return lambda line: severity if pattern.search(line) else None


LINE_CHECKS = (
    ("SCR-01", _regex(NETWORK, "medium")),
    ("SCR-05", _regex(PIPE_TO_SHELL, "critical")),
    ("SCR-06", _dynamic),
    ("SCR-08", _writes_outside),
    ("SCR-09", _regex(PERSIST, "high")),
    ("SCR-10", _regex(PRIVILEGE, "high")),
    ("SCR-11", _destructive),
    ("SCR-12", _obfuscated),
)


class UnitFacts:
    """What one code unit holds, for the rules that join lines (SCR-03/04/07)."""

    def __init__(self) -> None:
        self.network: list[tuple[int, str]] = []
        self.secrets: list[tuple[int, str, str]] = []


def script_hits(src: Source, units: list[Unit]) -> list[dict]:
    hits: list[dict] = []
    facts = []
    for unit in units:
        f = UnitFacts()
        hits += _unit_lines(src, unit, f)
        hits += _exfil(src, f.secrets, f.network)
        hits += _key_sent(src, f)
        hits += _decode_then_run(src, unit)
        facts.append(f)
    hits += _flow_between_units(src, facts)
    if src.name == "package.json":
        hits += _line_rule(src, "SCR-09", "high", NPM_HOOK)
    if src.name == "build.rs" and src.lines:
        hits.append(_hit("SCR-09", "high", src, 1, "build.rs: runs at cargo build"))
    return hits


def _unit_lines(src: Source, unit: Unit, f: UnitFacts) -> list[dict]:
    hits = []
    for n, line in unit.lines:
        for rule, check in LINE_CHECKS:
            severity = check(line)
            if severity:
                hits.append(_hit(rule, severity, src, n, line))
        kind = secret_class(line)
        if kind:
            hits.append(_hit("SCR-02", "high", src, n, line))
            f.secrets.append((n, line, kind))
        if NETWORK.search(line):
            f.network.append((n, line))
            if unit.makefile:
                hits.append(_hit("SCR-09", "high", src, n, line))
    return hits


def _exfil(src: Source, secrets: list[tuple[int, str, str]], network: list[tuple[int, str]]) -> list[dict]:
    """SCR-04: a secret read and a network call in one code unit."""
    if not secrets or not network:
        return []
    sn, sline, _ = secrets[0]
    nn, nline = network[0]
    malicious = any(kind in MALICIOUS_SECRETS for _, _, kind in secrets)
    text = f"secret read (line {sn}): {sline.strip()} | network (line {nn}): {nline.strip()}"
    return [_hit("SCR-04", "critical", src, sn, text, malicious)]


def _flow_between_units(src: Source, facts: list[UnitFacts]) -> list[dict]:
    """SCR-04 across the code blocks of one file: a credential-file, keychain or
    whole-env read in one block and a network call in another."""
    for i, a in enumerate(facts):
        creds = [s for s in a.secrets if s[2] in MALICIOUS_SECRETS]
        for j, b in enumerate(facts):
            if creds and b.network and i != j:
                return _exfil(src, creds, b.network)
    return []


def _key_sent(src: Source, f: UnitFacts) -> list[dict]:
    """SCR-03 malicious: the key variable is read in a unit that also calls the network."""
    if not f.network:
        return []
    return [
        _hit("SCR-03", "critical", src, n, line, True)
        for n, line, kind in f.secrets
        if kind == "env" and KEY_REF.search(line)
    ]


def _runs_file(line: str, path: str) -> bool:
    name = re.escape(path)
    return bool(
        re.search(
            r"(?:\b(?:sh|bash|zsh|python[\d.]*|node|perl|ruby|source|exec)\s+|chmod\s+\+x\s+|^\s*|[;&|]\s*|\.\s+)"
            r"(?:\./)?" + name + r"(?:\s|$|[;&|])",
            line,
        )
    )


def _decode_then_run(src: Source, unit: Unit) -> list[dict]:
    """SCR-07: decoded data that reaches SCR-06, an interpreter, or a file that runs."""
    names: set[str] = set()
    files: set[str] = set()
    hits = []
    for n, line in unit.lines:
        if DECODE.search(line):
            if DYNAMIC.search(line) or RUNS_PIPE.search(line):
                hits.append(_hit("SCR-07", "critical", src, n, line, True))
            names.update(m.group(1) for m in [ASSIGN.match(line)] if m)
            files.update(m.group(1).lstrip("./") for m in [REDIRECT.search(line)] if m)
            continue
        uses = DYNAMIC.search(line) and any(re.search(rf"\b{re.escape(v)}\b", line) for v in names)
        if uses or any(_runs_file(line, p) for p in files):
            hits.append(_hit("SCR-07", "critical", src, n, line, True))
    return hits
