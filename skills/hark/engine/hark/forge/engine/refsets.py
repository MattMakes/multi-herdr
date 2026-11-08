"""Reference sets: real recordings per category, split into forge and bench.

The recordings are licensed third-party sounds and are never shipped. A local
JSON registry lists them. A source recording and its sister takes sit in one
split only, so the benchmark scores the forge on sounds it never heard.
"""

from __future__ import annotations

import json
import os
from collections.abc import Iterable, Sequence
from pathlib import Path
from typing import Any

SPLITS = ("forge", "bench")

CATEGORIES: dict[str, str] = {
    "whoosh_fast": "short crisp pass-by of a rod through air, 0.2-0.8 s",
    "whoosh_big": "big deep slower swoop, 0.8-3 s",
    "swish": "light thin quick swish of a thrown object, 0.15-0.5 s",
    "swipe": "soft cloth or paper slide, 0.25-1 s",
    "riser": "swell that builds to a peak, 1.5-6 s",
    "downlifter": "downward sweep that fades out after a hit, 1-4 s",
    "impact": "deep cinematic boom with a long tail, 1-8 s",
    "sub_drop": "sub-bass pitch drop under a hit, 1-4 s",
    "click": "small plastic UI button click, 30-350 ms",
    "tap": "soft finger tap on glass or a hard surface, 30-300 ms",
    "toggle": "a switch flipped, crisp, often two-part, 50-450 ms",
    "hover_tick": "very small soft tick for hover and scroll steps, 15-150 ms",
    "pop": "short round pop (bubble, mouth, cork), 50-500 ms",
    "notify": "short bright notification ding or chime, 0.4-2 s",
    "shimmer": "sparkling chime sweep or bell-tree, 1-6 s",
    "typing_key": "one mechanical keyboard key press, 60-450 ms",
    "glitch": "digital glitch or stutter burst, designed, 0.1-1 s",
    "logo_sting": "brief logo sound: a struck hit that rings out, 1-6 s",
}

# Stems of our kit files that were cut from a bench source. Empty until the kit
# manifest names such a file.
KIT_BENCH: list[str] = []

AUDIO_EXTS = (".wav", ".flac", ".ogg", ".mp3", ".aif", ".aiff")


class RefsetsMissing(FileNotFoundError):
    """The refs registry or one of its files is absent."""


def _home() -> Path:
    home = os.environ.get("HARK_HOME") or os.path.join("~", ".cache", "hark")
    return Path(home).expanduser().resolve()


def registry_path() -> Path:
    """HARK_REFSETS when set, else <HARK_HOME>/refsets.json."""
    env = os.environ.get("HARK_REFSETS")
    return Path(env).expanduser().resolve() if env else _home() / "refsets.json"


def _norm(p: Any, base: Path | None = None) -> str:
    q = Path(str(p).replace("\\", "/")).expanduser()
    if base is not None and not q.is_absolute():
        q = base / q
    return os.path.normpath(str(q.absolute())).replace("\\", "/")


def _missing_message(path: Path) -> str:
    return (f"no refs registry at {path}. The reference recordings are licensed and not shipped. Get them one of 3 "
            "ways: lay out <root>/<category>/{forge,bench}/* and run refsets.write(refsets.scan(root)); set "
            "HARK_REFSETS to a registry JSON; or build one with python -m hark.forge.engine.eval.build_refsets.")


def load(path: Any = None) -> dict:
    """The registry dict with absolute paths."""
    p = Path(path) if path else registry_path()
    if not p.is_file():
        raise RefsetsMissing(_missing_message(p))
    reg = json.loads(p.read_text())
    base = p.resolve().parent
    for entry in (reg.get("categories") or {}).values():
        for split in SPLITS:
            entry[split] = [_norm(x, base) for x in entry.get(split) or []]
    return reg


def categories(path: Any = None) -> list[str]:
    """Sorted category names of the registry."""
    return sorted(load(path).get("categories") or {})


def refset(category: str, split: str = "forge", path: Any = None) -> list[str]:
    """The files of one category split; every file must exist."""
    if split not in SPLITS:
        raise ValueError(f"split must be one of {SPLITS}, not {split!r}")
    cats = load(path).get("categories") or {}
    if category not in cats:
        raise KeyError(f"unknown refset category {category!r}; known: {', '.join(sorted(cats))}")
    files = list(cats[category].get(split) or [])
    gone = [f for f in files if not os.path.isfile(f)]
    if gone:
        raise RefsetsMissing(f"{len(gone)} {category}/{split} ref(s) missing, e.g. {gone[0]}: rebuild the refs")
    return files


def note(category: str, path: Any = None) -> str:
    """The category's note, '' when none."""
    return str(((load(path).get("categories") or {}).get(category) or {}).get("note") or "")


def _key(p: Any) -> str:
    return os.path.normcase(_norm(p))


def _bench_set(path: Any) -> set[str]:
    try:
        reg = load(path)
    except RefsetsMissing:
        return set()
    return {_key(f) for e in (reg.get("categories") or {}).values() for f in e.get("bench") or []}


def held_out(paths: Iterable[Any], kit: bool = True, path: Any = None) -> list:
    """The paths that must not be forge refs (bench files, and kit files cut from bench sources)."""
    bench = _bench_set(path)
    stems = set(KIT_BENCH) if kit else set()
    return [p for p in paths if _key(p) in bench or Path(str(p)).stem in stems]


def _files(folder: Path, exts: Sequence[str]) -> list[str]:
    if not folder.is_dir():
        return []
    want = {e.lower() for e in exts}
    return sorted(_norm(f) for f in folder.iterdir() if f.is_file() and f.suffix.lower() in want)


def scan(root: Any, exts: Sequence[str] = AUDIO_EXTS) -> dict:
    """A registry from a folder layout <root>/<category>/{forge,bench}/*."""
    cats: dict[str, dict] = {}
    for sub in sorted(Path(root).iterdir()):
        if not sub.is_dir() or sub.name.startswith(("_", ".")):
            continue
        entry = {s: _files(sub / s, exts) for s in SPLITS}
        if any(entry.values()):
            cats[sub.name] = {**entry, "note": ""}
    return {"version": 1, "categories": cats}


def _source_problems(cats: dict, sources: dict) -> list[str]:
    where: dict[str, dict[str, set]] = {}
    for cat, entry in cats.items():
        for split in SPLITS:
            for f in entry.get(split) or []:
                sid = sources.get(f)
                if sid is not None:
                    where.setdefault(sid, {s: set() for s in SPLITS})[split].add(cat)
    return [f"source {sid} is forge in {sorted(w['forge'])} and bench in {sorted(w['bench'])}"
            for sid, w in where.items() if w["forge"] and w["bench"]]


def check_split(reg: dict, sources: dict | None = None) -> list[str]:
    """Problems of a registry: a file in both splits, or a source on both sides."""
    cats = reg.get("categories") or {}
    out = []
    for cat, entry in cats.items():
        both = set(entry.get("forge") or []) & set(entry.get("bench") or [])
        out += [f"{cat}: {os.path.basename(f)} in both splits" for f in sorted(both)]
    if sources:
        out += _source_problems(cats, sources)
    return out


def write(reg: dict, path: Any = None) -> Path:
    """Write the registry; refuse when check_split finds a problem."""
    problems = check_split(reg)
    if problems:
        raise ValueError("refs registry not written: " + "; ".join(problems[:3]))
    p = Path(path) if path else registry_path()
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(json.dumps(reg, indent=1))
    return p
