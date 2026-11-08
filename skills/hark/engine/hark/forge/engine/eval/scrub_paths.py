"""Strip machine-specific paths out of benchmark results before a commit.

`scrub(obj)` walks a JSON-like value and rewrites each string, keys included. A known root turns into `<refs_pro>`,
`<refs>` or `<HARK_HOME>`, and the engine folder itself turns into a relative path. The file name at the end stays.
`leftovers(text)` reports any drive-letter or home-folder path still present.
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

# skills/hark/engine, which the eval scripts treat as their repo root.
REPO = Path(__file__).resolve().parents[4]
EVAL_DIR = Path(__file__).resolve().parent

_SEP = r"[/\\]"
_TRAIL = r"[^\s\"'`<>|;,*?)\]]*"
_MACHINE_PATH = re.compile(r"(?<![\w])(?:/(?:Users|home)/[^\s\"'`]*|[A-Za-z]:[/\\][^\s\"'`]*)")
_REGISTRY_GROUPS = ("forge", "bench")


def _forward(value: Any) -> str:
    """Forward slashes only, and no slash at the end."""
    return str(value).replace("\\", "/").rstrip("/")


def _cache_home() -> str:
    explicit = os.environ.get("HARK_HOME")
    if explicit:
        return explicit
    return os.path.join(os.path.expanduser("~"), ".cache", "hark")


def _registry_files(registry: dict | None) -> list:
    groups = ((registry or {}).get("categories") or {}).values()
    return [item for entry in groups for key in _REGISTRY_GROUPS for item in entry.get(key) or []]


def _grandparent(item: Any) -> str | None:
    ancestors = Path(_forward(item)).parents
    return _forward(ancestors[2]) if len(ancestors) > 2 else None


def _library_folder(registry: dict | None, repo: str) -> str | None:
    """The one library folder (three levels above every ref file) when it lies outside the repo."""
    folders = {_grandparent(item) for item in _registry_files(registry)} - {None}
    if len(folders) != 1:
        return None
    folder = folders.pop()
    lowered, base = folder.lower(), repo.lower()
    if lowered == base or lowered.startswith(base + "/") or len(folder) <= 3:
        return None
    return folder


def roots(registry: dict | None = None, repo: Any = None) -> list[tuple[str, str]]:
    """(root, label) pairs sorted longest first; a root seen twice (ignoring case) is kept once."""
    engine = _forward(repo or REPO)
    pro = os.environ.get("HARK_REFS_PRO") or engine + "/out/refs_pro"
    options = [(_forward(pro), "<refs_pro>"), (engine + "/out/refs", "<refs>"),
               (_forward(_cache_home()), "<HARK_HOME>"), (engine, "")]
    library = _library_folder(registry, engine)
    if library:
        options.append((library, "<refs>"))
    taken: set[str] = set()
    ordered = []
    for path, tag in sorted(options, key=lambda pair: -len(pair[0])):
        key = path.lower()
        if key in taken:
            continue
        taken.add(key)
        ordered.append((path, tag))
    return ordered


def _root_regex(root: str) -> re.Pattern:
    escaped = _SEP.join(map(re.escape, root.split("/")))
    return re.compile(r"(?<![\w.])" + escaped + r"(?![\w.\-])((?:" + _SEP + _TRAIL + r")?)", re.IGNORECASE)


def _replace_root(text: str, root: str, tag: str) -> str:
    def swap(found: re.Match) -> str:
        tail = found.group(1).replace("\\", "/")
        return tag + tail if tag else (tail.lstrip("/") or ".")

    return _root_regex(root).sub(swap, text)


def scrub_text(text: str, rs: Sequence[tuple[str, str]]) -> str:
    """Apply every (root, label) rewrite to one string, in order."""
    for root, tag in rs:
        text = _replace_root(text, root, tag)
    return text


def scrub(obj: Any, registry: dict | None = None, rs: Sequence[tuple[str, str]] | None = None) -> Any:
    """Copy of obj with each string and dict key scrubbed; tuples come back as lists."""
    pairs = roots(registry) if rs is None else rs
    if isinstance(obj, str):
        cleaned = scrub_text(obj, pairs)
    elif isinstance(obj, dict):
        cleaned = {scrub(key, rs=pairs): scrub(val, rs=pairs) for key, val in obj.items()}
    elif isinstance(obj, (list, tuple)):
        cleaned = [scrub(val, rs=pairs) for val in obj]
    else:
        cleaned = obj
    return cleaned


def leftovers(text: str) -> list[str]:
    """Every drive-letter or home-folder path that is still in the text."""
    return _MACHINE_PATH.findall(text)


def _rewritten(path: Path, before: str) -> str:
    if path.suffix != ".json":
        return scrub_text(before, roots())
    ending = "\n" if before.endswith("\n") else ""
    return json.dumps(scrub(json.loads(before)), indent=1) + ending


def _process(path: Path, dry_run: bool) -> tuple[str, list[str]]:
    before = path.read_text()
    after = _rewritten(path, before)
    if after == before:
        outcome = "clean"
    elif dry_run:
        outcome = "would change"
    else:
        path.write_text(after)
        outcome = "scrubbed"
    return outcome, leftovers(after)


def _default_targets() -> list[str]:
    found = glob.glob(str(EVAL_DIR / "results_*.json")) + glob.glob(str(EVAL_DIR / "results_*.md"))
    return sorted(found)


def main(argv: Sequence[str] | None = None) -> int:
    """Scrub the results files. Exit 1 when a path is left, or under --check when a file would change."""
    parser = argparse.ArgumentParser(prog="python -m hark.forge.engine.eval.scrub_paths")
    parser.add_argument("files", nargs="*")
    parser.add_argument("--check", action="store_true")
    opts = parser.parse_args(argv)
    failed = False
    for name in opts.files or _default_targets():
        outcome, remaining = _process(Path(name), opts.check)
        note = "; left: " + ", ".join(remaining[:3]) if remaining else ""
        print(name + ": " + outcome + note)
        failed = failed or bool(remaining) or outcome == "would change"
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
