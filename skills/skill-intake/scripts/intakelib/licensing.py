"""Static licence detection and classification (spec 3.8, section 13 Q1).

Nothing is executed and no network is used. Sources: LICENSE*, LICENCE*,
COPYING*, UNLICENSE files; the `license` field of package.json,
pyproject.toml and Cargo.toml; and the `license` key of SKILL.md frontmatter.

Classes: permissive, copyleft, none, restrictive. When sources disagree, the
worst class wins: restrictive > copyleft > none (unrecognized) > permissive.
The flag is null for permissive, `copyleft`, `no-licence` (nothing found),
`unknown-licence` (found but not recognized) or `restrictive`.
"""

from __future__ import annotations

import json
import re
from pathlib import Path

from . import evidence, frontmatter

FILE_PREFIXES = ("license", "licence", "copying")
FILE_NAMES = ("unlicense",)
MANIFESTS = ("package.json", "pyproject.toml", "Cargo.toml")
RANK = {"permissive": 0, "none": 1, "copyleft": 2, "restrictive": 3}
_SPDX_SAFE = re.compile(r"^[A-Za-z0-9.+() -]{1,64}$")
_TOML_LICENSE = re.compile(r'^\s*license\s*=\s*(?:\{[^}]*?text\s*=\s*)?"([^"]{1,200})"', re.M)

# Text markers. A clause that forbids analysis makes any text restrictive,
# even with a grant. Weaker markers count only when no known grant matches
# (GPL, for example, says "proprietary" and "may not ... modify").
_FORBIDS_ANALYSIS = re.compile(
    r"\b(?:not|no|never|prohibit\w*|forbid\w*)\W+(?:\w+\W+){0,6}?"
    r"(?:reverse[- ]?engineer|decompil|disassembl)|\bno ?derivatives?\b"
)
_RESTRICTIVE = (
    "reverse engineer", "reverse-engineer", "decompil", "disassembl",
    "may not be modified", "may not modify", "proprietary",
    "end user license agreement", "all rights reserved",
)
_COPYLEFT = (
    ("gnu affero general public license", "AGPL-3.0"),
    ("gnu lesser general public license", "LGPL"),
    ("gnu general public license", "GPL"),
    ("mozilla public license", "MPL-2.0"),
    ("eclipse public license", "EPL-2.0"),
    ("sharealike", "CC-BY-SA-4.0"),
)
_PERMISSIVE = (
    ("permission is hereby granted, free of charge", "MIT"),
    ("redistribution and use in source and binary forms", "BSD"),
    ("apache license", "Apache-2.0"),
    ("permission to use, copy, modify, and/or distribute", "ISC"),
    ("free and unencumbered software released into the public domain", "Unlicense"),
    ("cc0", "CC0-1.0"),
    ("creative commons attribution 4.0", "CC-BY-4.0"),
)


def classify_spdx(spdx: str) -> str:
    s = spdx.strip().upper()
    if s in ("UNLICENSED", "PROPRIETARY") or "-ND" in s:
        return "restrictive"
    if s.startswith(("GPL", "LGPL", "AGPL", "MPL", "EPL", "CC-BY-SA", "EUPL", "OSL")):
        return "copyleft"
    if s.startswith(("MIT", "BSD", "APACHE", "ISC", "0BSD", "UNLICENSE", "CC0", "CC-BY", "ZLIB", "BSL-1.0", "PYTHON")):
        return "permissive"
    return "none"


def classify_text(text: str) -> tuple[str, str | None]:
    t = " ".join(text.lower().split())
    if _FORBIDS_ANALYSIS.search(t):
        return "restrictive", None
    for marker, spdx in _COPYLEFT:
        if marker in t:
            if spdx == "GPL":
                spdx = "GPL-3.0" if "version 3" in t else "GPL-2.0" if "version 2" in t else "GPL"
            return "copyleft", spdx
    for marker, spdx in _PERMISSIVE:
        if marker in t:
            return "permissive", spdx
    if any(m in t for m in _RESTRICTIVE):
        return "restrictive", None
    return "none", None


def _is_licence_file(name: str) -> bool:
    n = name.lower()
    return n.startswith(FILE_PREFIXES) or n in FILE_NAMES


def _read(path: Path, limit: int = 262144) -> str:
    with open(path, encoding="utf-8", errors="replace") as f:
        return f.read(limit)


def _manifest_licence(path: Path) -> str | None:
    try:
        text = _read(path)
    except OSError:
        return None
    if path.name == "package.json":
        try:
            data = json.loads(text)
        except ValueError:
            return None
        lic = data.get("license") if isinstance(data, dict) else None
        if isinstance(lic, dict):
            lic = lic.get("type")
        return lic if isinstance(lic, str) and lic.strip() else None
    m = _TOML_LICENSE.search(text)
    return m.group(1) if m else None


def _safe_spdx(spdx: str) -> str:
    return spdx if _SPDX_SAFE.match(spdx) else evidence.escape(spdx)


def detect(src: Path, dirs: list[Path]) -> dict:
    """Detect the licence. `dirs` are the folders to check for licence files
    and manifests (the source root and every folder that holds a SKILL.md)."""
    found: list[tuple[str, str | None, str]] = []  # (class, spdx, relpath)

    def rel(p: Path) -> str:
        return evidence.escape(p.relative_to(src).as_posix())

    for d in dirs:
        try:
            entries = sorted(d.iterdir())
        except OSError:
            continue
        for p in entries:
            if not p.is_file() or p.is_symlink():
                continue
            if _is_licence_file(p.name):
                try:
                    cls, spdx = classify_text(_read(p))
                except OSError:
                    continue
                found.append((cls, spdx, rel(p)))
            elif p.name in MANIFESTS:
                lic = _manifest_licence(p)
                if lic:
                    found.append((classify_spdx(lic), _safe_spdx(lic), rel(p)))
            elif p.name == "SKILL.md":
                lic = frontmatter.read(p).get("license")
                if lic:
                    cls = classify_spdx(lic)
                    if cls == "none":
                        cls = classify_text(lic)[0]
                    found.append((cls, _safe_spdx(lic), rel(p)))
    if not found:
        return {"spdx": None, "files": [], "class": "none", "flag": "no-licence"}
    worst = max(found, key=lambda f: RANK[f[0]])
    cls = worst[0]
    spdx = worst[1] or next((f[1] for f in found if f[0] == cls and f[1]), None)
    flag = {"permissive": None, "copyleft": "copyleft", "none": "unknown-licence", "restrictive": "restrictive"}[cls]
    files = sorted({f[2] for f in found})
    return {"spdx": spdx, "files": files, "class": cls, "flag": flag}
