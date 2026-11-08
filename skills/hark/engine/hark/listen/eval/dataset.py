"""Data locations, label files and the labelled libraries of the evaluation (listen-eval Data sets).

Layout of the labelled library root (``HARK_EVAL_ASSETS``), our NEEDS-DATA fallback:

    <root>/<category>/...            the tuning library, one folder per category
    <root>/heldout/<category>/...    the held-out library
    <root>/manifests/*.json          {file_id: [need, ...]} or {file_id: {"needs": [...]}} for tuning files
    <root>/find/checks.json          the key-press recordings, slices, room tone and one-shots of E12

A file id is the file's base name without its extension.
"""

from __future__ import annotations

import json
import os
from collections.abc import Mapping, Sequence
from dataclasses import dataclass, field
from pathlib import Path

from hark.core import audio

from hark.listen.eval import metrics

HERE = Path(__file__).resolve().parent
LOCAL_PATHS = HERE / "local_paths.json"
LOCATIONS = {"HARK_EVAL_ASSETS": "assets", "HARK_EVAL_REFS": "refs", "HARK_EVAL_SFX_LIBRARY": "sfx_library"}
HELDOUT_DIR = "heldout"
HELDOUT_ORDER = ("toy",) + tuple(c for c in metrics.CATEGORIES if c != "toy")


class EvalError(Exception):
    """An evaluation input is missing or wrong (one error line)."""


def location(var: str, local: Path = LOCAL_PATHS) -> Path:
    """A data location from the environment, then from local_paths.json; EvalError names the variable."""
    value = os.environ.get(var)
    if not value and local.exists():
        value = json.loads(local.read_text(encoding="utf-8")).get(LOCATIONS[var])
    if not value:
        raise EvalError(f"{var} is not set (nor '{LOCATIONS[var]}' in {local.name})")
    path = Path(value).expanduser()
    if not path.exists():
        raise EvalError(f"{var} points to {path}, which does not exist")
    return path


def read_label(name: str) -> dict:
    """A label file beside the scripts."""
    return json.loads((HERE / name).read_text(encoding="utf-8"))


def file_id(path: str) -> str:
    """The file id: the base name without its extension."""
    return Path(path).stem


@dataclass
class Library:
    """A labelled set: paths, ids, category folders, classes per file."""

    paths: list[str] = field(default_factory=list)
    ids: list[str] = field(default_factory=list)
    folders: list[str] = field(default_factory=list)
    classes: list[list[str]] = field(default_factory=list)

    def add(self, path: str, folder: str, classes: Sequence[str]) -> None:
        self.paths.append(path)
        self.ids.append(file_id(path))
        self.folders.append(folder)
        self.classes.append(list(classes))

    def subset(self, keep: Sequence[bool]) -> "Library":
        out = Library()
        for i, k in enumerate(keep):
            if k:
                out.add(self.paths[i], self.folders[i], self.classes[i])
        return out

    def split(self, name: str) -> "Library":
        """The dev or test split (rule 2); 'all' is the library itself."""
        return self if name == "all" else self.subset([metrics.split_of(i) == name for i in self.ids])


def category_files(root: Path, categories: Sequence[str] = metrics.CATEGORIES) -> list[tuple[str, str]]:
    """(path, category) of every media file under root/<category>, in category order."""
    out = []
    for cat in categories:
        folder = root / cat
        if folder.is_dir():
            out += [(p, cat) for p in audio.list_media([str(folder)])]
    return out


def read_needs(root: Path) -> dict[str, list[str]]:
    """file id -> needs, merged over every manifest."""
    needs: dict[str, list[str]] = {}
    folder = root / "manifests"
    for f in sorted(folder.glob("*.json")) if folder.is_dir() else []:
        for fid, v in json.loads(f.read_text(encoding="utf-8")).items():
            needs.setdefault(fid, []).extend(v.get("needs", []) if isinstance(v, Mapping) else v)
    return needs


def need_map(queries: Mapping) -> dict[str, str]:
    """need -> class id, from queries.json."""
    return {n: c["id"] for c in queries["classes"] for n in c["needs"]}


def tuning_library(root: Path, queries: Mapping) -> Library:
    """The tuning library with classes from the manifests (music folder files are class music)."""
    nmap, needs, lib = need_map(queries), read_needs(root), Library()
    for path, cat in category_files(root):
        cls = ["music"] if cat == "music" else sorted({nmap[n] for n in needs.get(file_id(path), []) if n in nmap})
        lib.add(path, cat, cls)
    return lib


def heldout_library(root: Path, labels: Mapping) -> Library:
    """The held-out library: one entry per id (toy first), classes from heldout_labels.json (rule 4, 5)."""
    files = labels.get("files", {})
    music = labels.get("music_class", "music")
    seen, lib = set(), Library()
    for path, cat in category_files(root / HELDOUT_DIR, HELDOUT_ORDER):
        fid = file_id(path)
        if fid in seen:
            continue
        seen.add(fid)
        if cat == "music":
            cls = [music]
        elif fid in files:
            cls = list(files[fid])
        else:
            raise EvalError(f"held-out recording {fid!r} has no entry in heldout_labels.json")
        lib.add(path, cat, cls)
    return lib


def extra_refs() -> list[str]:
    """The extra reference recordings (films, mixes, stems)."""
    return audio.list_media([str(location("HARK_EVAL_REFS"))])
