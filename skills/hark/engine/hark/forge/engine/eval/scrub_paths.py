"""Remove machine paths from benchmark results before they are committed.

`scrub(obj)` rewrites every string (dict keys too): known roots become `<refs_pro>`, `<refs>`, `<HARK_HOME>` or a
repo-relative path. File names are kept. `leftovers(text)` finds the drive and home paths that remain.
"""

from __future__ import annotations

import argparse
import glob
import json
import os
import re
from collections.abc import Sequence
from pathlib import Path
from typing import Any

# The hark engine project folder (skills/hark/engine): the "repo" of the eval scripts.
REPO = Path(__file__).resolve().parents[4]
EVAL_DIR = Path(__file__).resolve().parent
_STOP = r"[^\s\"'`;,)\]<>|*?]*"
_LEFT = re.compile(r"(?<![\w])(?:[A-Za-z]:[/\\][^\s\"'`]*|/(?:Users|home)/[^\s\"'`]*)")


def _slash(p: Any) -> str:
    return str(p).replace("\\", "/").rstrip("/")


def _home() -> str:
    return os.environ.get("HARK_HOME") or os.path.join(os.path.expanduser("~"), ".cache", "hark")


def _registry_root(registry: dict | None, repo: str) -> str | None:
    files = [f for e in ((registry or {}).get("categories") or {}).values() for s in ("forge", "bench")
             for f in e.get(s) or []]
    if not files:
        return None
    tops = {_slash(Path(_slash(f)).parents[2]) for f in files if len(Path(_slash(f)).parents) > 2}
    if len(tops) != 1:
        return None
    top = tops.pop()
    inside = top.lower() == repo.lower() or top.lower().startswith(repo.lower() + "/")
    return None if inside or len(top) <= 3 else top


def roots(registry: dict | None = None, repo: Any = None) -> list[tuple[str, str]]:
    """(root, label) pairs, longest root first, duplicates dropped (case-insensitive)."""
    repo = _slash(repo or REPO)
    cand = [(_slash(os.environ.get("HARK_REFS_PRO") or f"{repo}/out/refs_pro"), "<refs_pro>"),
            (f"{repo}/out/refs", "<refs>"), (_slash(_home()), "<HARK_HOME>"), (repo, "")]
    extra = _registry_root(registry, repo)
    if extra:
        cand.append((extra, "<refs>"))
    seen, out = set(), []
    for r, lab in sorted(cand, key=lambda c: -len(c[0])):
        if r.lower() not in seen:
            seen.add(r.lower())
            out.append((r, lab))
    return out


def _pattern(root: str) -> re.Pattern:
    body = r"[/\\]".join(re.escape(part) for part in root.split("/"))
    return re.compile(rf"(?<![\w.]){body}(?![\w.\-])((?:[/\\]{_STOP})?)", re.IGNORECASE)


def _sub_one(text: str, root: str, lab: str) -> str:
    def rep(m: re.Match) -> str:
        rest = m.group(1).replace("\\", "/")
        if lab:
            return lab + rest
        return rest.lstrip("/") or "."

    return _pattern(root).sub(rep, text)


def scrub_text(text: str, rs: Sequence[tuple[str, str]]) -> str:
    """The text with every root rewritten."""
    for root, lab in rs:
        text = _sub_one(text, root, lab)
    return text


def scrub(obj: Any, registry: dict | None = None, rs: Sequence[tuple[str, str]] | None = None) -> Any:
    """The object with every string (and dict key) scrubbed."""
    rs = roots(registry) if rs is None else rs
    if isinstance(obj, str):
        return scrub_text(obj, rs)
    if isinstance(obj, dict):
        return {scrub(k, rs=rs): scrub(v, rs=rs) for k, v in obj.items()}
    if isinstance(obj, (list, tuple)):
        return [scrub(v, rs=rs) for v in obj]
    return obj


def leftovers(text: str) -> list[str]:
    """Drive paths and home paths that remain in the text."""
    return _LEFT.findall(text)


def _scrub_file(path: Path, check: bool) -> tuple[str, list[str]]:
    old = path.read_text()
    if path.suffix == ".json":
        new = json.dumps(scrub(json.loads(old)), indent=1) + ("\n" if old.endswith("\n") else "")
    else:
        new = scrub_text(old, roots())
    if new == old:
        state = "clean"
    elif check:
        state = "would change"
    else:
        path.write_text(new)
        state = "scrubbed"
    return state, leftovers(new)


def main(argv: Sequence[str] | None = None) -> int:
    """Scrub results files; exit 1 on leftovers, or with --check when a file would change."""
    ap = argparse.ArgumentParser(prog="python -m hark.forge.engine.eval.scrub_paths")
    ap.add_argument("files", nargs="*")
    ap.add_argument("--check", action="store_true")
    a = ap.parse_args(argv)
    files = a.files or sorted(glob.glob(str(EVAL_DIR / "results_*.json")) + glob.glob(str(EVAL_DIR / "results_*.md")))
    bad = False
    for f in files:
        state, left = _scrub_file(Path(f), a.check)
        print(f"{f}: {state}" + (f"; left: {', '.join(left[:3])}" if left else ""))
        bad = bad or bool(left) or state == "would change"
    return 1 if bad else 0


if __name__ == "__main__":
    raise SystemExit(main())
