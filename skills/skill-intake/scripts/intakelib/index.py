"""Source index (contaminated side only): what files exist, and how to split them into units.

Ported from clean-room crlib/index.py and changed for skill-intake (spec 6.3):

- the source is the quarantined tree C/quarantine/src/;
- the units come from C/skills.json (intake discover): 1 unit per skill, plus
  1 more unit for each script over SCRIPT_UNIT_LINES lines (the skill unit
  then leaves that script out);
- source-index.json and fingerprints.json go to C/contaminated/.

Also records the two fingerprints the leakage scanner needs:
  - distinctive identifiers (camelCase / snake_case / digit-bearing names from the source)
  - sampled token-shingle hashes, so verbatim runs can be detected without storing source text
"""

from __future__ import annotations

import hashlib
import os
import re
from collections import Counter
from pathlib import Path

from .task import Task, TaskError, is_within, now, read_json, write_json

SKIP_DIRS = {".git", ".hg", ".svn", "node_modules", "vendor", "dist", "build", "target", "out",
             "__pycache__", ".venv", "venv", ".tox", ".idea", ".vscode", "Pods", ".gradle"}
MAX_FILE_BYTES = 2_000_000
SCRIPT_UNIT_LINES = 300
REL_COMPONENT = re.compile(r"[A-Za-z0-9._-]+")
LANGS = {
    ".py": "python", ".js": "javascript", ".mjs": "javascript", ".cjs": "javascript", ".jsx": "javascript",
    ".ts": "typescript", ".tsx": "typescript", ".go": "go", ".rs": "rust", ".java": "java", ".kt": "kotlin",
    ".swift": "swift", ".c": "c", ".h": "c", ".cc": "cpp", ".cpp": "cpp", ".hpp": "cpp", ".cs": "csharp",
    ".rb": "ruby", ".php": "php", ".m": "objc", ".scala": "scala", ".lua": "lua", ".sh": "shell",
}
IMPORT_RES = {
    "python": [r"^\s*import\s+([\w.]+)", r"^\s*from\s+([\w.]+)\s+import"],
    "javascript": [r"""\bfrom\s+['"]([^'"]+)['"]""", r"""\brequire\(\s*['"]([^'"]+)['"]\s*\)""", r"""^\s*import\s+['"]([^'"]+)['"]"""],
    "go": [r'^\s*import\s+"([^"]+)"', r'^\s*"([\w./-]+)"\s*$'],
    "rust": [r"^\s*use\s+([\w:]+)", r"^\s*mod\s+(\w+)\s*;"],
    "java": [r"^\s*import\s+([\w.]+)\s*;"],
    "kotlin": [r"^\s*import\s+([\w.]+)"],
    "swift": [r"^\s*import\s+(\w+)"],
    "c": [r'^\s*#\s*include\s+[<"]([^>"]+)[>"]'],
}
IMPORT_RES["typescript"] = IMPORT_RES["javascript"]
IMPORT_RES["cpp"] = IMPORT_RES["objc"] = IMPORT_RES["c"]
SYMBOL_RE = re.compile(
    r"^\s*(?:export\s+)?(?:pub(?:\(\w+\))?\s+)?(?:async\s+)?"
    r"(?:def|class|function|fn|func|struct|enum|trait|interface|type|protocol)\s+([A-Za-z_]\w*)",
    re.M,
)
TOKEN_RE = re.compile(r"[A-Za-z_][A-Za-z0-9_]*|\d+|[^\sA-Za-z0-9_]")
IDENT_RE = re.compile(r"[A-Za-z_][A-Za-z0-9_]{5,}")
SHINGLE_K = 8
SAMPLE_MOD = 4  # keep 1 in 4 shingles; a ~20-token verbatim run is still almost surely caught
MAX_IDENTIFIERS = 20_000


def source_root(task: Task) -> Path:
    """The quarantined source tree that intake acquire fetched."""
    return task.c / "quarantine" / "src"


def tokens(text: str) -> list[str]:
    return [t.lower() for t in TOKEN_RE.findall(text)]


def shingles(text: str) -> set[str]:
    toks = tokens(text)
    out = set()
    for i in range(len(toks) - SHINGLE_K + 1):
        h = hashlib.blake2b(" ".join(toks[i:i + SHINGLE_K]).encode(), digest_size=8).hexdigest()
        if int(h, 16) % SAMPLE_MOD == 0:
            out.add(h)
    return out


def is_distinctive(name: str) -> bool:
    """camelCase, snake_case, or digit-bearing names are code-shaped; plain words are not."""
    inner_cap = any(c.isupper() for c in name[1:]) and any(c.islower() for c in name)
    return "_" in name.strip("_") or inner_cap or any(c.isdigit() for c in name)


def read_text(path: Path) -> str | None:
    try:
        if path.stat().st_size > MAX_FILE_BYTES:
            return None
        data = path.read_bytes()
    except OSError:
        return None
    if b"\0" in data[:8192]:
        return None  # binary
    return data.decode("utf-8", errors="replace")


def walk(root: Path):
    for dirpath, dirnames, filenames in os.walk(root, followlinks=False):
        dirnames[:] = sorted(d for d in dirnames if d not in SKIP_DIRS and not d.startswith("."))
        for name in sorted(filenames):
            p = Path(dirpath) / name
            if not p.is_symlink():
                yield p


def check_rel_path(root: Path, rel: str) -> Path:
    """The skill folder for `rel`. Refuse a path that is not plain components or resolves outside `root`."""
    parts = rel.split("/") if isinstance(rel, str) else []
    if not parts or any(not REL_COMPONENT.fullmatch(p) or p in (".", "..") for p in parts):
        raise TaskError(f"refused skills.json rel_path {rel!r}: components must match [A-Za-z0-9._-]+")
    folder = (root / rel).resolve()
    if not is_within(folder, root.resolve()) or folder == root.resolve():
        raise TaskError(f"refused skills.json rel_path {rel!r}: it resolves outside the quarantine root")
    if not folder.is_dir():
        raise TaskError(f"refused skills.json rel_path {rel!r}: no such folder in the quarantine root")
    return folder


def load_skills(task: Task) -> list[dict]:
    path = task.c / "skills.json"
    if not path.is_file():
        raise TaskError(f"no skills.json in {task.c}; run intake discover first")
    skills = read_json(path).get("skills", [])
    if not skills:
        raise TaskError("skills.json lists no skill")
    for s in skills:
        check_rel_path(source_root(task), s.get("rel_path"))
    return skills


def _owner(rel: str, skills: list[dict]) -> str | None:
    """The skill whose folder holds `rel`, or None (out of scope)."""
    for s in skills:
        base = s["rel_path"]
        if rel.startswith(base + "/"):
            return s["name"]
    return None


def build(task: Task) -> dict:
    root = source_root(task)
    if not root.is_dir():
        raise TaskError(f"no quarantined source at {root}; run intake acquire first")
    skills = load_skills(task)
    files, idents, prints = [], Counter(), set()
    for path in walk(root):
        text = read_text(path)
        if text is None:
            continue
        lang = LANGS.get(path.suffix.lower(), "other")
        rel = path.relative_to(root).as_posix()
        imports = sorted({m for pat in IMPORT_RES.get(lang, []) for m in re.findall(pat, text, re.M)})
        files.append({
            "path": rel, "skill": _owner(rel, skills), "language": lang,
            "lines": text.count("\n") + 1, "bytes": len(text.encode()),
            "sha256": hashlib.sha256(text.encode()).hexdigest(),
            "imports": imports, "symbols": sorted(set(SYMBOL_RE.findall(text))),
        })
        idents.update(n for n in IDENT_RE.findall(text) if is_distinctive(n))
        prints |= shingles(text)

    index = {
        "task_id": task.id, "created": now(),
        "totals": {"files": len(files), "lines": sum(f["lines"] for f in files)},
        "languages": dict(Counter(f["language"] for f in files).most_common()),
        "files": files,
        "units": units(files, skills),
        "out_of_scope": [f["path"] for f in files if f["skill"] is None],
    }
    cdir = task.c / "contaminated"
    write_json(cdir / "source-index.json", index)
    write_json(cdir / "fingerprints.json", {
        "shingle_k": SHINGLE_K, "sample_mod": SAMPLE_MOD,
        "identifiers": [n for n, _ in idents.most_common(MAX_IDENTIFIERS)],
        "shingles": sorted(prints),
    })
    return index


def units(files: list[dict], skills: list[dict]) -> list[dict]:
    """1 unit per skill, plus 1 unit per script over SCRIPT_UNIT_LINES lines."""
    out: list[dict] = []
    for s in skills:
        own = [f for f in files if f["skill"] == s["name"]]
        big = [f for f in own if f["language"] != "other" and f["lines"] > SCRIPT_UNIT_LINES]
        rest = [f["path"] for f in own if f not in big]
        out.append({"unit_id": f"u{len(out) + 1}", "skill": s["name"], "kind": "skill", "files": rest})
        for f in big:
            out.append({"unit_id": f"u{len(out) + 1}", "skill": s["name"], "kind": "script", "files": [f["path"]]})
    return out
