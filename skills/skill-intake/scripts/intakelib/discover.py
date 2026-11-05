"""Stage 2: find the skills in the quarantined tree (spec 3.7).

A port of harvest's discovery: a skill is a directory that holds a SKILL.md
(the search does not go into a skill directory); skip folders; keep one copy
per name, ranked .claude/skills > plugin/skills > .agents/skills > skills/ >
other (harvest's variantRank). The source root itself is a skill when it
holds a SKILL.md. Files outside every skill directory that no skill text
names are listed as out of scope.
"""

from __future__ import annotations

import os
import re
import unicodedata
from pathlib import Path, PurePosixPath

from . import evidence, frontmatter

SKIP_DIRS = frozenset({".git", "node_modules", ".github", "dist", "build", "vendor", "site-packages", "__pycache__"})
NAME = re.compile(r"^[a-z0-9][a-z0-9-]{0,63}$")
TEXT_LIMIT = 1024 * 1024


def variant_rank(rel: str) -> int:
    p = f"/{rel}/"
    if "/.claude/skills/" in p:
        return 0
    if "/plugin/skills/" in p:
        return 1
    if "/.agents/skills/" in p:
        return 2
    if "/skills/" in p:
        return 3
    return 4


def normalize(text: str) -> str | None:
    """Map a name onto ^[a-z0-9][a-z0-9-]{0,63}$, or None when nothing is left."""
    ascii_text = unicodedata.normalize("NFKD", text).encode("ascii", "ignore").decode()
    s = re.sub(r"[^a-z0-9]+", "-", ascii_text.lower()).strip("-")
    s = s[:64].rstrip("-")
    return s if NAME.match(s) else None


def find(src: Path) -> list[PurePosixPath]:
    """Relative paths of every skill directory, sorted (harvest's findSkills)."""
    found: list[PurePosixPath] = []
    if (src / "SKILL.md").is_file():
        return [PurePosixPath(".")]

    def walk(d: Path, rel: PurePosixPath) -> None:
        for e in sorted(d.iterdir(), key=lambda p: p.name):
            if e.is_symlink() or not e.is_dir() or e.name in SKIP_DIRS:
                continue
            r = rel / e.name
            if (e / "SKILL.md").is_file():
                found.append(r)
            else:
                walk(e, r)

    walk(src, PurePosixPath())
    return found


def _files(d: Path) -> list[Path]:
    return sorted(p for p in d.rglob("*") if p.is_file() and not p.is_symlink())


def _text(p: Path) -> str:
    try:
        with open(p, "rb") as f:
            data = f.read(TEXT_LIMIT)
    except OSError:
        return ""
    if b"\0" in data:
        return ""
    return data.decode("utf-8", "replace")


def discover(src: Path, root_name: str) -> dict:
    """Return {"skills": [...], "out_of_scope": [...], "findings": [...]}.

    `root_name` is the folder name to use for a skill at the source root.
    Every source text in the result is escaped.
    """
    candidates = []
    findings = []
    unnamed = 0
    for rel in find(src):
        d = src / rel
        fm = frontmatter.read(d / "SKILL.md")
        dir_name = root_name if str(rel) == "." else rel.name
        fm_name = fm.get("name", "").strip()
        name = fm_name if NAME.match(fm_name) else normalize(dir_name)
        if name is None:
            unnamed += 1
            name = f"skill-{unnamed}"
            findings.append({
                "rule": "INJ-09", "severity": "medium", "file": evidence.escape(f"{rel}/SKILL.md"),
                "detail": f"the skill name cannot be normalized: {dir_name!r}; it is now {name}",
            })
        candidates.append((name, rel, fm))
    best: dict[str, tuple[str, PurePosixPath, dict]] = {}
    dropped: dict[str, list[str]] = {}
    for cand in candidates:
        name, rel = cand[0], cand[1]
        cur = best.get(name)
        if cur is None:
            best[name] = cand
        elif variant_rank(str(rel)) < variant_rank(str(cur[1])):
            dropped.setdefault(name, []).append(str(cur[1]))
            best[name] = cand
        else:
            dropped.setdefault(name, []).append(str(rel))
    skills = []
    for name in sorted(best):
        _, rel, fm = best[name]
        files = _files(src / rel)
        skills.append({
            "name": name,
            "rel_path": evidence.escape(str(rel)),
            "description": evidence.untrusted(fm.get("description", "")),
            "files": len(files),
            "bytes": sum(p.stat().st_size for p in files),
            "dropped_variants": sorted(evidence.escape(v) for v in dropped.get(name, [])),
        })
    return {
        "skills": skills,
        "out_of_scope": _out_of_scope(src, [src / c[1] for c in candidates]),
        "findings": findings,
    }


def _out_of_scope(src: Path, skill_dirs: list[Path]) -> list[str]:
    """Files outside every skill directory that no skill text names, by path
    from the source root or by a relative path from the skill directory."""
    inside: set[Path] = set()
    texts: list[tuple[Path, str]] = []
    for d in skill_dirs:
        for p in _files(d):
            inside.add(p)
            texts.append((d, _text(p)))
    out = []
    for p in _files(src):
        if p in inside:
            continue
        rel = p.relative_to(src).as_posix()
        if any(rel in text or _relative(p, d) in text for d, text in texts):
            continue
        out.append(evidence.escape(rel))
    return out


def _relative(p: Path, d: Path) -> str:
    return os.path.relpath(p, d).replace("\\", "/")
