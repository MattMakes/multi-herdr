"""Leakage scanner: checks an artifact headed for the clean side for source expression.

Ported from clean-room crlib/scan.py and changed for skill-intake:

- source paths are the contaminated side root C, the quarantined tree, and a
  local source path from provenance.json;
- source names come from provenance.json (the GitHub owner and repo, or the
  local folder or archive name), not from a source root's folder name;
- the spec's public contract names and its `skill.name` may cross the wall.

Run by the controller through intake, because it compares against source fingerprints.
"""

from __future__ import annotations

import json
import re
from dataclasses import dataclass
from pathlib import Path, PurePosixPath

from .index import shingles, source_root
from .task import Task, read_json

STRUCTURAL = {
    "code-fence": re.compile(r"```|~~~"),
    "diff": re.compile(r"(?m)^(diff --git |@@ -\d+(,\d+)? \+\d+(,\d+)? @@|--- a/|\+\+\+ b/)"),
    "decompiler": re.compile(r"\b(decompil\w*|disassembl\w*|jadx|ghidra|apktool|ida pro|hex-rays|smali)\b", re.I),
    "numbered-listing": re.compile(r"(?m)(^\s*\d{1,5}[:|]\s+\S.*\n){3,}"),
    "code-line": re.compile(
        r"(?m)^\s*(def \w+\(|class \w+\s*[:({]|import [\w.]+\s*$|from [\w.]+ import \w|function\s*\w*\s*\(|"
        r"(const|let|var) \w+\s*=|fn \w+\s*[(<]|func \w+\s*\(|#include\s*[<\"]|(public|private|protected) [\w<>\[\]]+ \w+\s*\()"
    ),
}
ARCHIVE_SUFFIXES = (".zip", ".skill")


@dataclass
class Finding:
    check: str
    detail: str
    severity: str  # "block" or "warn"


def strings_of(data) -> list[str]:
    """Only JSON string values can leak; keys are our own schema field names."""
    if isinstance(data, str):
        return [data]
    if isinstance(data, dict):
        return [s for v in data.values() for s in strings_of(v)]
    if isinstance(data, list):
        return [s for v in data for s in strings_of(v)]
    return []


def public_names(data) -> set[str]:
    """Names the spec declares as public contract may appear verbatim (compatibility)."""
    if not isinstance(data, dict):
        return set()
    names = {c.get("name", "") for c in data.get("public_contract", []) if isinstance(c, dict)}
    skill = data.get("skill")
    if isinstance(skill, dict) and isinstance(skill.get("name"), str):
        names.add(skill["name"])
    return names - {""}


def _provenance_source(task: Task) -> dict:
    path = task.c / "provenance.json"
    if not path.is_file():
        return {}
    src = read_json(path).get("source")
    return src if isinstance(src, dict) else {}


def source_paths(task: Task) -> list[str]:
    paths = [str(task.c), str(source_root(task))]
    raw = _provenance_source(task).get("input")
    if isinstance(raw, str) and raw.startswith("/"):
        paths.append(raw.rstrip("/"))
    return paths


def source_names(task: Task) -> set[str]:
    """Names that point at the source: GitHub owner and repo, or the local folder or archive stem."""
    src = _provenance_source(task)
    names: set[str] = set()
    url = src.get("url")
    if isinstance(url, str) and url:
        parts = [p for p in url.rstrip("/").split("/") if p]
        names.update(p[:-4] if p.endswith(".git") else p for p in parts[-2:])
    raw = src.get("input")
    if isinstance(raw, str) and raw and not url:
        name = PurePosixPath(raw.rstrip("/")).name
        for suffix in ARCHIVE_SUFFIXES:
            if name.lower().endswith(suffix):
                name = name[: -len(suffix)]
        names.add(name)
    return {n for n in names if len(n) >= 4}


def scan_text(text: str, task: Task, allowed: set[str] = frozenset()) -> list[Finding]:
    findings = [Finding(name, f"matched /{rx.pattern[:40]}/", "block")
                for name, rx in STRUCTURAL.items() if rx.search(text)]

    lowered = text.lower()
    if any(p.lower() in lowered for p in source_paths(task)):
        findings.append(Finding("source-path", "contains an absolute source path", "block"))
    allowed_lower = {a.lower() for a in allowed}
    for name in sorted(source_names(task)):
        if name.lower() not in allowed_lower and re.search(rf"\b{re.escape(name.lower())}\b", lowered):
            findings.append(Finding("source-name", f"mentions the source name '{name}'", "block"))

    fp_path = task.c / "contaminated" / "fingerprints.json"
    if fp_path.is_file():
        fp = read_json(fp_path)
        words = set(re.findall(r"[A-Za-z_][A-Za-z0-9_]{5,}", text))
        leaked = sorted((words & set(fp["identifiers"])) - set(allowed))
        if leaked:
            findings.append(Finding("identifiers", f"{len(leaked)} source identifier(s): {', '.join(leaked[:8])}", "block"))
        shared = shingles(text) & set(fp["shingles"])
        if shared:
            findings.append(Finding("verbatim", f"{len(shared)} sampled token run(s) shared with source", "block"))
    else:
        findings.append(Finding("fingerprints", "no source index yet; identifier and verbatim checks skipped", "warn"))
    return findings


def scan_file(path: Path, task: Task) -> list[Finding]:
    raw = path.read_text(encoding="utf-8")
    try:
        data = json.loads(raw)
    except json.JSONDecodeError:
        return scan_text(raw, task)
    return scan_text("\n".join(strings_of(data)), task, public_names(data))


def passed(findings: list[Finding]) -> bool:
    return not any(f.severity == "block" for f in findings)
