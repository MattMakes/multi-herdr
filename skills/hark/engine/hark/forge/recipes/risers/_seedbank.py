"""The DERIVE seed banks of the riser folder: one pinned entry per delivered member, built from the preset tables.

The source's tuned values are not reproducible, so each delivered member is the hand-designed member of the same
role (recipes-motion, Data assets). Run ``python -m hark.forge.recipes.risers._seedbank <folder>`` to rewrite the
JSON files; the tests rebuild them and compare.
"""

from __future__ import annotations

import json
import math
import pathlib
import sys
from typing import Any

from hark.forge.recipes.risers import downlifter, glitch, notify, riser, sting

SOURCE = "derived from the preset tables (recipes-motion, Data assets); critic fields not measured"
BANKS = ("riser", "downlifter", "glitch", "notify", "sting")


def _round6(v: Any) -> Any:
    if isinstance(v, bool) or not isinstance(v, (int, float)):
        return v
    if v == 0 or not math.isfinite(v):
        return float(v)
    return float(f"{v:.6g}")


def entry(params: dict, category: str, target: str, member: str, seed: int = 0) -> dict:
    """One pinned seed-bank entry (forge-core seed bank format)."""
    return {
        "params": {k: _round6(v) for k, v in params.items()},
        "rank_score": None,
        "context": f"{target} | ",
        "total": None,
        "tone_db": None,
        "passes": False,
        "source": SOURCE,
        "delivered": category,
        "member": member,
        "seed": seed,
    }


def _riser_members() -> list[tuple[str, str, dict]]:
    by_cat = riser.FAMILIES_BY_CAT["riser"]
    edits = dict(zip(("air", "tension", "shepard", "rush", "cymbal"), riser.R4_EDITS["riser"]))
    r14 = {name: {**by_cat[name], **edits[name]} for name in ("air", "tension")}
    # riser_01 and riser_04 are round 1 to 4 sets; riser_02, 03 and 05 are round-6 builds (r6 = 1).
    rows = [("riser", "riser_01", r14["air"]), ("riser", "riser_02", riser.R6["air"]),
            ("riser", "riser_03", riser.R6["tension"]), ("riser", "riser_04", r14["tension"]),
            ("riser", "riser_05", riser.R6["shepard"])]
    pro = riser.FAMILIES_BY_CAT["pro_riser"]
    rows += [("pro_riser", f"pro_riser_{i + 1:02d}", p) for i, p in enumerate(pro.values())]
    return rows


def _downlifter_members() -> list[tuple[str, str, dict, int]]:
    names = ("shepard_fall", "flange_air", "synth_fall", "air_fall", "tape", "sub_sink")
    return [("downlifter", name, downlifter.FAMILIES[name], 0) for name in names]


def _glitch_members() -> list[tuple[str, str, dict, int]]:
    rows = [("pro_glitch", "pro_glitch_01", dict(glitch.DEFAULTS), 0)]
    return rows + [("glitch", name, glitch.full6(name), 3) for name, _ in glitch.FAMILY6]


def _notify_members() -> list[tuple[str, str, dict, int]]:
    pro = [("pro_notify", m, p, 0) for m, p in notify.delivered("pro_notify")]
    return pro + [("notify", f"notify_{i + 1:02d}", p, 0) for i, (_, p) in enumerate(notify.delivered("notify"))]


def _sting_members() -> list[tuple[str, str, dict, int]]:
    return [(cat, m, p, 0) for cat in ("pro_logo_sting", "logo_sting") for m, p in sting.FAMILIES[cat]]


def _riser_rows() -> list[tuple[str, str, dict, int]]:
    return [(c, m, p, 0) for c, m, p in _riser_members()]


_MEMBERS = (("riser", _riser_rows), ("downlifter", _downlifter_members), ("glitch", _glitch_members),
            ("notify", _notify_members), ("sting", _sting_members))


def _members(bank: str) -> list[tuple[str, str, dict, int]]:
    """(category, member, params, seed) of every delivered entry of a bank; KeyError for an unknown bank."""
    return dict(_MEMBERS)[bank]()


def _target(bank: str, category: str) -> str:
    module = {"riser": riser, "downlifter": downlifter, "glitch": glitch, "notify": notify, "sting": sting}[bank]
    targets = getattr(module, "TARGETS", {})
    if bank == "notify" and category == "notify":
        return "a struck bell notification chime"
    return targets.get(category, module.TARGET)


def build(bank: str) -> dict:
    """The derived seed bank of one recipe."""
    entries = [entry(p, cat, _target(bank, cat), member, seed) for cat, member, p, seed in _members(bank)]
    return {"recipe": bank, "entries": entries}


def write_all(folder: pathlib.Path) -> list[pathlib.Path]:
    """Write every derived bank as <folder>/<recipe>.json (indent 1)."""
    folder.mkdir(parents=True, exist_ok=True)
    paths = []
    for bank in BANKS:
        path = folder / f"{bank}.json"
        path.write_text(json.dumps(build(bank), indent=1) + "\n")
        paths.append(path)
    return paths


if __name__ == "__main__":
    for written in write_all(pathlib.Path(sys.argv[1])):
        print(written)
