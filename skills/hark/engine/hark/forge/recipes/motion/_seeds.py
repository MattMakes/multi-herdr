"""The DERIVE seed banks of the motion recipes (recipes-motion index, "Data assets").

This module is a helper (its name starts with ``_``), so the recipe registry does not
list it. ``derive(name)`` builds the bank of one recipe from its family tables: one
pinned entry per delivered member, seed 0, no rank score and no reference names.

Write the shipped files with:
``python -m hark.forge.recipes.motion._seeds <seeds dir>``.
"""

from __future__ import annotations

import importlib
import json
import sys
from pathlib import Path
from typing import Any

BANKS: tuple[str, ...] = ("whoosh", "swish", "swipe", "zip", "shimmer", "hover")

WHOOSH_CATS: tuple[tuple[str, str], ...] = (
    ("fast", "whoosh_fast"), ("big", "whoosh_big"), ("pro_big", "pro_whoosh_big"))
SHIMMER_TARGETS: dict[str, str] = {
    "pro_shimmer": "a sparkling magical shimmer",
    "shimmer": "bar chimes and wind chimes ringing out",
}
HOVER_MEMBERS: dict[str, tuple[str, ...]] = {
    "pro_hover_tick": ("ping", "air", "glass", "soft", "detent"),
    "hover_tick": ("high", "mid", "ping", "air", "detent"),
}
ZIP_DELIVERED: tuple[str, ...] = ("up_hi", "up", "arc", "down", "down_warm")


def _module(name: str) -> Any:
    return importlib.import_module(f"hark.forge.recipes.motion.{name}")


def entry(params: dict, category: str, member: str, target: str) -> dict[str, Any]:
    """One pinned, derived seed-bank entry."""
    return {"params": dict(params), "rank_score": None, "context": f"{target} |", "total": None,
            "tone_db": None, "passes": None, "source": "derived", "delivered": category,
            "member": member, "seed": 0}


def _whoosh() -> list[dict]:
    m = _module("whoosh")
    out = []
    for kind, cat in WHOOSH_CATS:
        for member in m.PRESETS[kind]:
            params = {**m.OPTIONAL, **m.preset(kind, member)}
            out.append(entry(params, cat, member, m.KINDS[kind]["target"]))
    return out


def _pass_kind(name: str) -> list[dict]:
    m = _module(name)
    optional = _module("whoosh").OPTIONAL
    return [entry({**optional, **m.preset(m.KIND, member)}, m.KIND, member, m.TARGET)
            for member in m.PRESETS[m.KIND]]


def _zip() -> list[dict]:
    m = _module("zip")
    fam = m.FAMILIES_BY_CAT["pro_zip"]
    return [entry(fam[k], "pro_zip", k, m.TARGET) for k in ZIP_DELIVERED]


def _shimmer() -> list[dict]:
    m = _module("shimmer")
    out = []
    for cat in ("shimmer", "pro_shimmer"):
        for i, params in enumerate(m.FAMILY[cat], start=1):
            out.append(entry(params, cat, f"{cat}_{i:02d}", SHIMMER_TARGETS[cat]))
    return out


def _hover() -> list[dict]:
    m = _module("hover")
    return [entry(m.family_params(cat, fam), cat, fam, m.TARGETS[cat])
            for cat, fams in HOVER_MEMBERS.items() for fam in fams]


_BUILDERS = {"whoosh": _whoosh, "zip": _zip, "shimmer": _shimmer, "hover": _hover,
             "swish": lambda: _pass_kind("swish"), "swipe": lambda: _pass_kind("swipe")}


def derive(name: str) -> dict[str, Any]:
    """The derived seed bank of one motion recipe: {"recipe": name, "entries": [...]}."""
    return {"recipe": name, "entries": _BUILDERS[name]()}


def write(folder: Path) -> list[Path]:
    """Write every derived bank as <folder>/<name>.json and return the paths."""
    folder.mkdir(parents=True, exist_ok=True)
    paths = []
    for name in BANKS:
        path = folder / f"{name}.json"
        path.write_text(json.dumps(derive(name), indent=1, sort_keys=True) + "\n")
        paths.append(path)
    return paths


if __name__ == "__main__":
    for written in write(Path(sys.argv[1])):
        print(written)
