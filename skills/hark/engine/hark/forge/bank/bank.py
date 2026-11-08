"""The hark sound bank: exact render blocks of named sounds, rendered on demand.

Spec: docs/specs/hark/bank-kit-sfx-bank.md (part 1) and the index
docs/specs/hark/bank-kit-sfx.md, section 3.5. A bank entry is not audio. It is
the recipe call that makes the sound. ``render`` runs it again and gives one
take at its level.
"""

from __future__ import annotations

import copy
import difflib
import json
import math
import numbers
import os
from collections.abc import Callable, Iterable
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import numpy as np

from hark.forge.bank._measures import (  # noqa: F401  (public re-exports)
    SR, compare_fp, estimate_pitch, fingerprint, measure, meta_of, times,
)

__all__ = [  # noqa: F822  (VERSION comes from the module __getattr__)
    "VERSION", "BankError", "ids", "get", "list", "list_sounds", "category_info", "renderer", "render",
    "render_final", "level", "check_sr", "normalize", "add_warning", "times", "measure", "meta_of",
    "fingerprint", "compare_fp", "estimate_pitch", "build", "build_pluck", "build_forge04", "build_derive",
    "add_fit", "check", "reload", "write_bank", "catalog",
]

FORMAT = 1
DEFAULT_PEAK = -1.0
RATES = (44100, 48000, 88200, 96000)
SILENT_FLOOR = 1e-7
WEIGHT_MAX = 4.0
PKG_DIR = Path(__file__).resolve().parent
BANK_DIR: Path = PKG_DIR / "data"
CATALOG_PATH = PKG_DIR / "catalog.json"
LIST_COMMAND = "hark sfx list"


class BankError(ValueError):
    """A bank problem: an unknown id, a bad file, a failed render or a bad level."""


@dataclass
class _Bank:
    paths: tuple[str, ...]
    entries: dict[str, dict] = field(default_factory=dict)
    categories: dict[str, dict] = field(default_factory=dict)
    versions: tuple[str, ...] = ()
    files: dict[str, str] = field(default_factory=dict)


_CACHE: dict[tuple, _Bank] = {}
_CATALOG: dict[str, Any] = {}


# ---------------------------------------------------------------- catalogue


def catalog() -> dict[str, Any]:
    """The built-in catalogue: categories, forge04 members and target rows."""
    if not _CATALOG:
        _CATALOG.update(json.loads(CATALOG_PATH.read_text(encoding="utf-8")))
    return _CATALOG


# ---------------------------------------------------------------- loading


def _package_files() -> tuple[str, ...]:
    folder = Path(BANK_DIR)
    if not folder.is_dir():
        return ()
    names = sorted((p.name for p in folder.glob("*.json")), key=_load_rank)
    return tuple(str(folder / n) for n in names)


def _load_rank(name: str) -> tuple[int, str]:
    """forge_v2.json first, then the other forge files, then the rest; by name inside each group."""
    if name == "forge_v2.json":
        return 0, name
    return (1 if name.startswith("forge") else 2), name


def _env_files() -> tuple[str, ...]:
    raw = os.environ.get("HARK_BANK", "")
    return tuple(os.path.abspath(p) for p in raw.split(os.pathsep) if p)


def bank_files() -> tuple[str, ...]:
    """The bank files in load order: the package folder, then ``HARK_BANK``."""
    return _package_files() + _env_files()


def _read_file(path: str) -> dict:
    if not os.path.exists(path):
        raise BankError(f"bank file {path} does not exist (HARK_BANK joins paths with {os.pathsep!r})")
    try:
        with open(path, encoding="utf-8") as fh:
            doc = json.load(fh)
    except (OSError, ValueError) as exc:
        raise BankError(f"cannot read bank file {path}: {type(exc).__name__}: {exc}") from None
    if not isinstance(doc, dict) or doc.get("bank") != FORMAT or not isinstance(doc.get("entries"), dict):
        raise BankError(f"{path} is not a bank file (needs \"bank\": 1 and an \"entries\" object)")
    return doc


def _merge(bank: _Bank, path: str, doc: dict) -> None:
    for name, info in (doc.get("categories") or {}).items():
        bank.categories.setdefault(name, info)
    for sound_id, entry in doc["entries"].items():
        bank.entries[sound_id] = entry
        bank.files[sound_id] = path
    bank.versions += (str(doc.get("version") or Path(path).stem),)


def _bank() -> _Bank:
    paths = bank_files()
    if paths not in _CACHE:
        bank = _Bank(paths)
        for path in paths:
            _merge(bank, path, _read_file(path))
        _CACHE.clear()
        _CACHE[paths] = bank
    return _CACHE[paths]


def reload() -> None:
    """Forget the loaded bank, so the next lookup reads the files again."""
    _CACHE.clear()


def __getattr__(name: str) -> Any:
    if name == "VERSION":
        return "+".join(_bank().versions)
    raise AttributeError(f"module {__name__!r} has no attribute {name!r}")


# ---------------------------------------------------------------- lookups


def ids() -> list[str]:
    """Every sound id, sorted."""
    return sorted(_bank().entries)


def _hint(sound_id: str, known: list[str]) -> str:
    close = difflib.get_close_matches(sound_id, known, n=4, cutoff=0.5)
    if close:
        return f"Did you mean {', '.join(close)}?"
    cat = sound_id.split("/", 1)[0]
    same = [i for i in known if i.split("/", 1)[0] == cat][:8]
    return f"{cat} has: {', '.join(same)}." if same else ""


def get(sound_id: str) -> dict:
    """A deep copy of one entry plus its ``id``; ``BankError`` for an unknown id."""
    bank = _bank()
    entry = bank.entries.get(sound_id) if isinstance(sound_id, str) else None
    if entry is None:
        known = ids()
        parts = [f"unknown sound {sound_id!r}.", _hint(str(sound_id), known),
                 f"The bank has {len(known)} sounds: run {LIST_COMMAND}."]
        raise BankError(" ".join(p for p in parts if p))
    out = copy.deepcopy(entry)
    out["id"] = sound_id
    return out


def category_info(cat: str) -> dict:
    """``{recipe, target, kinds, use}`` of a category, filled from the built-in table."""
    builtin = catalog()["categories"].get(cat, {})
    info = dict(_bank().categories.get(cat) or {})
    out = {"recipe": info.get("recipe", builtin.get("recipe")),
           "target": info.get("target", builtin.get("target"))}
    out["kinds"] = [*(info.get("kinds") or builtin.get("kinds") or [])]
    out["use"] = info.get("use") or builtin.get("use") or ""
    return out


def entry_kinds(entry: dict) -> list[str]:
    """The entry's own kinds, else its category's kinds, else []."""
    own = entry.get("kinds")
    if own:
        return [*own]
    return category_info(entry.get("category", ""))["kinds"]


def _kind_match(kinds: Iterable[str], kind: str) -> bool:
    return any(k == kind or k.split(".", 1)[0] == kind for k in kinds)


def _row(sound_id: str, entry: dict) -> dict:
    from hark.forge.engine import knobs

    meta = entry.get("meta") or {}
    cat = entry.get("category", sound_id.split("/", 1)[0])
    row: dict[str, Any] = {"id": sound_id, "category": cat, "member": entry.get("member"),
                           "type": entry.get("type", "forge")}
    for key in ("dur_s", "peak_s", "onset_s", "end_s", "centroid_hz", "attack_ms", "lufs_m_max"):
        row[key] = meta.get(key)
    row["kinds"] = entry_kinds(entry)
    row["knobs"] = knobs.describe({**entry, "id": sound_id})
    row["notes"] = entry.get("notes") or category_info(cat)["use"] or ""
    if row["type"] == "fit":
        row["source"] = entry.get("source")
    return row


def list_sounds(category: str | None = None, kind: str | None = None) -> list[dict]:
    """The ``sfx list`` rows, sorted by id, with the category and kind filters."""
    rows = []
    for sound_id in ids():
        entry = _bank().entries[sound_id]
        if category is not None and entry.get("category") != category:
            continue
        if kind is not None and not _kind_match(entry_kinds(entry), kind):
            continue
        rows.append(_row(sound_id, entry))
    return rows


# ---------------------------------------------------------------- level and rate


def _is_number(v: Any) -> bool:
    return isinstance(v, numbers.Real) and not isinstance(v, bool) and math.isfinite(float(v))


def level(peak_db: float = DEFAULT_PEAK, weight: float | None = None) -> float:
    """The sample peak in dBFS that a take is written at (part 1, 5.2)."""
    _check_peak(peak_db)
    if weight is None:
        return float(peak_db)
    _check_weight(weight)
    out = float(peak_db) + 20 * math.log10(weight)
    if out > 0:
        raise BankError(f"weight {weight} on peak_db {peak_db} gives {out:.2f} dBFS, over 0: lower peak_db")
    return out


def _check_peak(peak_db: Any) -> None:
    if not _is_number(peak_db):
        raise BankError("peak_db must be a number")
    if peak_db > 0:
        raise BankError(f"peak_db {peak_db} is over 0 dBFS and clips: give 0 or less")


def _check_weight(weight: Any) -> None:
    if not _is_number(weight):
        raise BankError("weight must be a finite number")
    if not 0 < weight <= WEIGHT_MAX:
        raise BankError(f"weight {weight} is outside (0, {WEIGHT_MAX:g}]")


def check_sr(sr: Any) -> int:
    """``sr`` as an int when it is one of the 4 write rates, else ``BankError``."""
    ok = isinstance(sr, numbers.Real) and not isinstance(sr, bool) and float(sr).is_integer()
    if ok and int(sr) in RATES:
        return int(sr)
    raise BankError(f"sample rate {sr!r} is not allowed: use one of {', '.join(map(str, RATES))}")


def normalize(y: np.ndarray, peak_db: float) -> np.ndarray:
    """``y`` scaled so its largest absolute sample is ``10^(peak_db/20)``."""
    x = np.asarray(y, dtype=np.float64)
    peak = float(np.max(np.abs(x))) if x.size else 0.0
    if peak <= 0:
        return x
    return x * (10 ** (peak_db / 20) / peak)


def add_warning(checks: dict | None, warning: dict | None, msg: str | None = None) -> dict | None:
    """Add one ``{code, msg}`` warning to an audit result and rebuild its summary.

    ``add_warning(result, {"code": c, "msg": m})`` or ``add_warning(result, c, m)``.
    """
    if checks is None or warning is None:
        return checks
    if isinstance(warning, str):
        warning = {"code": warning, "msg": msg or warning}
    checks.setdefault("warnings", []).append(warning)
    checks["ok"] = False
    name = str(checks.get("summary") or "sound").split(":", 1)[0]
    n = len(checks["warnings"])
    msgs = "; ".join(w["msg"] for w in checks["warnings"])
    checks["summary"] = f"{name}: {n} warning{'' if n == 1 else 's'}: {msgs}"
    return checks


# ---------------------------------------------------------------- rendering


def _validated(x: Any, sound_id: str) -> np.ndarray:
    y = np.asarray(x, dtype=np.float64) if x is not None else np.zeros(0)
    if y.size == 0 or not np.all(np.isfinite(y)) or float(np.max(np.abs(y))) < SILENT_FLOOR:
        raise BankError(f"{sound_id}: the render failed or is silent")
    return y


def _rng(seed: int) -> np.random.Generator:
    from hark.forge.engine import dsp

    return dsp.rng(int(seed))


def _forge_fn(entry: dict) -> Callable:
    from hark.forge import recipes

    name, fn = entry.get("recipe"), entry.get("fn") or "render"
    try:
        return getattr(recipes.load(name), fn)
    except (ImportError, AttributeError, KeyError, TypeError) as exc:
        raise BankError(f"{entry.get('id')}: render {name}.{fn} not found: {exc}") from None


def _forge_run(entry: dict, seed: int) -> Callable[..., np.ndarray]:
    fn, params, sound_id = _forge_fn(entry), dict(entry.get("params") or {}), entry.get("id", "?")

    def run(edit: dict | None = None, fit_knobs: dict | None = None) -> np.ndarray:
        p = copy.deepcopy(params)
        p.update(edit or {})
        return _validated(fn(p, _rng(seed)), sound_id)

    return run


def _fit_run(entry: dict, seed: int) -> Callable[..., np.ndarray]:
    import hark.fit

    model, vary, sound_id = entry.get("model"), float(entry.get("vary") or 0.0), entry.get("id", "?")

    def run(edit: dict | None = None, fit_knobs: dict | None = None) -> np.ndarray:
        kw: dict[str, Any] = {"take": True, "bed": False}
        if vary:
            kw["vary"] = vary
        kw.update(fit_knobs or {})
        return _validated(hark.fit.render(model, int(seed), **kw), sound_id)

    return run


def renderer(entry: dict, seed: int | None = None) -> Callable[..., np.ndarray]:
    """``run(edit=None, fit_knobs=None)``: the raw float64 render of an entry."""
    kind = entry.get("type", "forge")
    use = int(entry.get("seed", 0) if seed is None else seed)
    if kind == "forge":
        return _forge_run(entry, use)
    if kind == "fit":
        return _fit_run(entry, use)
    raise BankError(f"{entry.get('id')}: unknown entry type {kind!r}")


def _resolve(sound: str | dict) -> dict:
    if isinstance(sound, dict):
        return sound
    return get(sound)


def _raw_take(entry: dict, run: Callable, dur: Any, apex: Any, pitch: Any) -> tuple[np.ndarray, dict]:
    if dur is None and apex is None and pitch is None:
        return run(), {}
    from hark.forge.engine import knobs

    x, edits = knobs.apply(entry, run, dur=dur, apex=apex, pitch=pitch)
    return _validated(x, entry.get("id", "?")), dict(edits)


def _audit(y: np.ndarray, entry: dict, edits: dict) -> dict:
    from hark.forge.bank import sfx
    from hark.forge.engine import knobs

    result = sfx.check(y, category=entry.get("category"), sound=entry.get("id"))
    return add_warning(result, knobs.pitch_warning(edits))


def _to_rate(y: np.ndarray, sr: int) -> np.ndarray:
    if sr == SR:
        return y
    from hark.core import audio

    return np.asarray(audio.resample(y, SR, sr), dtype=np.float32)


def render(sound: str | dict, seed: int | None = None, dur: float | None = None, apex: float | None = None,
           pitch: Any = None, peak_db: float = DEFAULT_PEAK, weight: float | None = None, sr: int = SR,
           checks: bool = True) -> tuple[np.ndarray, dict]:
    """One take of a sound at its level: ``(audio float32, meta)`` (part 1, section 5)."""
    target = level(peak_db, weight)
    sr = check_sr(sr)
    entry = _resolve(sound)
    use_seed = int(entry.get("seed", 0) if seed is None else seed)
    raw, edits = _raw_take(entry, renderer(entry, use_seed), dur, apex, pitch)
    if weight is not None:
        edits["weight"] = {"weight": weight, "peak_db": round(target, 2)}
    y = normalize(raw, target).astype(np.float32)
    meta: dict[str, Any] = {"id": entry.get("id"), "seed": use_seed, "sr": sr}
    meta.update(measure(y))
    meta["edits"] = edits
    meta["checks"] = _audit(y, entry, edits) if checks else None
    y = _to_rate(y, sr)
    meta["dur_s"] = round(len(y) / sr, 4)
    return y, meta


def render_final(sound: str | dict) -> np.ndarray:
    """A forge entry exactly as the forge wrote its final, palette gain applied."""
    entry = _resolve(sound)
    if entry.get("type", "forge") != "forge":
        raise BankError(f"{entry.get('id')}: render_final needs a forge entry, not {entry.get('type')!r}")
    from hark.forge.engine import schema

    block = {k: entry[k] for k in ("recipe", "fn", "member", "params", "seed", "peak_db", "gain_db", "gain", "mode")
             if k in entry}
    try:
        return schema.rerender(block)
    except ValueError as exc:
        raise BankError(f"{entry.get('id')}: {exc}") from None


# ---------------------------------------------------------------- builds and checks (hark.forge.bank._build)


def write_bank(path: str | os.PathLike, doc: dict) -> str:
    """Write a bank document in the one-entry-per-line layout, atomically."""
    from hark.forge.bank import _build

    return _build.write_bank(path, doc)


def build(dirs: Iterable[str], out: str, version: str | None = None, log: Callable | None = None) -> dict:
    """Build the forge file from forge final folders (part 1, 7.1)."""
    from hark.forge.bank import _build

    return _build.build(dirs, out, version, log)


def build_pluck(out: str, version: str | None = None, log: Callable | None = None) -> dict:
    """Build the pluck file (part 1, 7.2)."""
    from hark.forge.bank import _build

    return _build.build_pluck(out, version, log)


def build_forge04(out: str, version: str | None = None, ref_clap: dict | None = None,
                  log: Callable | None = None) -> dict:
    """Build the forge04 file from the recipe palettes (part 1, 7.3)."""
    from hark.forge.bank import _build

    return _build.build_forge04(out, version, ref_clap, log)


def build_derive(out: str, version: str | None = None, log: Callable | None = None) -> dict:
    """Build the forge file from the recipe family tables (ARCHITECTURE 12.2)."""
    from hark.forge.bank import _build

    return _build.build_derive(out, version, log)


def add_fit(model: dict | str, sound_id: str, source: dict | str, bank: str | None = None, seed: int = 0,
            vary: float = 0.0, kinds: list[str] | None = None, notes: str | None = None,
            allow_license: bool = False, force: bool = False, log: Callable | None = None) -> dict:
    """Add one fit entry to a bank file (part 1, 7.4)."""
    from hark.forge.bank import _build

    return _build.add_fit(model, sound_id, source, bank, seed, vary, kinds, notes, allow_license, force, log)


def check(ids: Iterable[str] | None = None, log: Callable | None = None) -> dict:
    """Re-render entries against their stored fingerprints (part 1, section 9)."""
    from hark.forge.bank import _build

    return _build.check(ids, log)


list = list_sounds  # noqa: A001  (the spec name; never call the builtin in this module)
