"""Bank file builders and the fingerprint check (maintenance only).

Spec: docs/specs/hark/bank-kit-sfx-bank.md sections 3.1, 7 and 9, and
ARCHITECTURE.md 12.2 (``build --derive``). Every builder is all-or-nothing:
one refused item writes nothing and raises one ``BankError`` with every
failure.
"""

from __future__ import annotations

import copy
import glob
import hashlib
import json
import math
import os
import re
from collections.abc import Callable, Iterable
from pathlib import Path
from typing import Any

import numpy as np

from hark.forge.bank import bank as B
from hark.forge.bank import _measures as M

DEFAULT_TAGS = {"forge_v2": "forge_v2h1", "forge04": "forge04h1", "fit": "fit1h", "pluck": "pluck1h"}
BUILD_TOL = 1e-6
PLUCK_F0 = 523.2511306011972
PLUCK_MEMBERS = (
    ("kalimba", 8001, {}, {}, "a bright steel tine over a small wooden resonator box"),
    ("kalimba_soft", 8009, {"thumb_ms": 1.4},
     {"skin_lvl": 0.4, "knock_lvl": 0.5, "zing_lvl": 0.5, "p2_lvl": 0.75},
     "the soft flesh of the thumb: a darker, gentler attack"),
    ("kalimba_ring", 8006, {}, {"tau1": 1.5}, "a longer ring for end cards and chord tones"),
)
F04_TAKES = 6
F04_CLEAN_MIN = 5
ORIGINALITY_LIMIT = 0.96
NULL_ORIGINAL = {"made": "recipe", "max_ref_clap": None, "ref": "none"}
PITCHED_PARTIAL = ("click", "pro_click", "toggle")
PARTIAL_SPAN_ST = 6.0
PARTIAL_PARAM = "pitch"  # the tink centre of click, pro_click and toggle (forge-core knobs PITCH)
FIT_ID = re.compile(r"^fit/[a-z0-9_.]+/[a-z0-9_]+$")
SOURCE_KEYS = ("id", "title", "author", "license", "url")
FREE_LICENCES = ("cc0", "public domain", "publicdomain", "cc-pdm", "pdm")
REFUSED_WORDS = re.compile(r"\b(nc|nd)\b")
REFUSED_TEXT = ("noncommercial", "non-commercial", "noderiv")
FIT_USE = "a sound made by fitting a model to a public-domain recording"
TOLERANCE_DUR = 0.10
TOLERANCE_CENTROID = 1.26
TOLERANCE_LUFS = 3.0
TOLERANCE_CENTS = 50.0
# Default takes that keep their audit warning (bank-kit-sfx part 2, 3.11): build --derive does not re-derive them.
KNOWN_WARNINGS = frozenset({
    "pro_zip/up", "pro_zip/up_hi", "pro_zip/arc", "logo_sting/zip_ping", "pro_logo_sting/sweep_hit", "pop/2",
    "pro_pop/bloop", "typing_key/light", "typing_key/laptop", "pro_hover_tick/air", "pro_hover_tick/glass",
    "hover_tick/detent", "pro_typing_key/soft", "glitch/dropout", "logo_sting/reverse_bloom", "pro_shimmer/m3",
})


def _hark_version() -> str:
    import hark

    return str(getattr(hark, "__version__", "0.1.0"))


def _say(log: Callable | None, text: str) -> None:
    if log is not None:
        log(text)


def _refuse(failures: list[str]) -> None:
    if failures:
        lines = "\n".join(f"  {f}" for f in failures)
        raise B.BankError(f"{len(failures)} pick(s) refused, nothing written:\n{lines}")


# ---------------------------------------------------------------- writer (3.1)


def _layout(doc: dict) -> str:
    head = [f"{json.dumps(k)}: {json.dumps(v, ensure_ascii=False)}" for k, v in doc.items()
            if k not in ("categories", "entries")]
    lines = ["{" + ", ".join(head) + ","]
    for key, close in (("categories", " },"), ("entries", " }")):
        lines.append(f' "{key}": {{')
        items = [f"  {json.dumps(k)}: {json.dumps(v, ensure_ascii=False)}" for k, v in (doc.get(key) or {}).items()]
        if items:
            lines.append(",\n".join(items))
        lines.append(close)
    lines.append("}")
    return "\n".join(lines) + "\n"


def write_bank(path: str | os.PathLike, doc: dict) -> str:
    """Write ``doc`` with one entry per line, check it parses, then replace ``path``."""
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(f"{path.name}.tmp{os.getpid()}")
    tmp.write_text(_layout(clean(doc)), encoding="utf-8", newline="\n")
    try:
        json.loads(tmp.read_text(encoding="utf-8"))
    except ValueError:
        tmp.unlink()
        raise
    os.replace(tmp, path)
    B.reload()
    return str(path)


def clean(obj: Any) -> Any:
    """JSON-safe copy: numpy numbers become Python numbers, NaN becomes null."""
    fn = next((f for kinds, f in _CLEANERS if isinstance(obj, kinds)), None)
    return obj if fn is None else fn(obj)


def _finite(obj: Any) -> float | None:
    v = float(obj)
    return v if np.isfinite(v) else None


_CLEANERS: tuple[tuple[Any, Callable[[Any], Any]], ...] = (
    (dict, lambda o: {str(k): clean(v) for k, v in o.items()}),
    ((list, tuple), lambda o: [clean(v) for v in o]),
    (np.ndarray, lambda o: clean(o.tolist())),
    ((np.floating, float), _finite),
    (np.integer, int),
)


def _header(name: str, version: str | None, note: str) -> dict:
    return {"bank": B.FORMAT, "name": name, "version": version or DEFAULT_TAGS[name],
            "hark": _hark_version(), "note": note}


def _category(cat: str, recipe: str | None = None, target: str | None = None) -> dict:
    info = B.catalog()["categories"].get(cat, {})
    return {"recipe": recipe or info.get("recipe"), "target": target or info.get("target"),
            "kinds": [*info.get("kinds", [])], "use": info.get("use", "")}


def _size_kb(path: str) -> int:
    return int(round(os.path.getsize(path) / 1024))


def _wrote(log: Callable | None, out: str, doc: dict) -> dict:
    path = write_bank(out, doc)
    _say(log, f"wrote {path}: {len(doc['entries'])} entries ({doc['version']}), {_size_kb(path)} KB")
    return doc


# ---------------------------------------------------------------- entry meta


def _meta(raw: np.ndarray, category: str, params: dict | None = None) -> dict:
    y = B.normalize(raw, B.DEFAULT_PEAK).astype(np.float32)
    meta = M.meta_of(y)
    if category in PITCHED_PARTIAL:
        meta["pitch_hz"] = partial_pitch(y, category, params or {})
    return meta


def partial_pitch(y: np.ndarray, category: str, params: dict) -> float | None:
    """The heard partial of a click or toggle (part 1, 8.2), 2 decimals or None."""
    near = params.get(PARTIAL_PARAM)
    if not isinstance(near, (int, float)) or isinstance(near, bool) or near <= 0:
        return None
    f = M.estimate_pitch(y, near_hz=float(near), span_st=PARTIAL_SPAN_ST)
    return None if f is None else round(f, 2)


def _entry(category: str, member: str, recipe: str, fn: str, params: dict, seed: int, **extra: Any) -> dict:
    entry = {"type": "forge", "category": category, "member": member, "recipe": recipe, "fn": fn,
             "params": params, "seed": int(seed), "peak_db": B.DEFAULT_PEAK, "gain_db": 0.0}
    entry.update({k: v for k, v in extra.items() if v is not None})
    return entry


def _render_entry(entry: dict, sound_id: str) -> np.ndarray:
    return B.renderer({**entry, "id": sound_id})()


# ---------------------------------------------------------------- 7.1 forge finals


def _final_dirs(dirs: Iterable[str]) -> list[str]:
    found = set()
    for pattern in dirs:
        for d in sorted(glob.glob(pattern)) or [pattern]:
            if os.path.isfile(os.path.join(d, "report.json")):
                found.add(os.path.abspath(d))
    return sorted(found)


def _dir_category(folder: str) -> str:
    p = Path(folder)
    return p.parent.name if p.name == "final" else p.name


def _read_wav(path: str) -> np.ndarray:
    import soundfile

    x, _ = soundfile.read(path, dtype="float64", always_2d=True)
    return x


def _pick_render(report: dict, pick: dict) -> tuple[dict, np.ndarray]:
    """The render block of a pick and its raw render; ``ValueError`` when either is missing."""
    from hark.forge.engine import schema

    if not isinstance(pick.get("render"), dict):
        raise ValueError("no render block of its own")
    block = schema.block(report, pick)
    raw = schema.raw(block)
    if raw is None:
        raise ValueError("the render failed or is silent")
    return block, raw


def _same_as_wav(raw: np.ndarray, block: dict, wav: str) -> None:
    """``ValueError`` when the levelled re-render differs from the WAV by more than BUILD_TOL."""
    from hark.forge.engine import schema

    err = schema.compare(schema.level(raw, block), _read_wav(wav))
    if not err <= BUILD_TOL:
        raise ValueError(f"re-render differs from its WAV by {err:.3g} (tolerance {BUILD_TOL:g})")


def _pick_entry(report: dict, pick: dict, folder: str, category: str) -> tuple[str, dict, np.ndarray]:
    block, raw = _pick_render(report, pick)
    _same_as_wav(raw, block, os.path.join(folder, os.path.basename(pick.get("file") or f"{pick.get('name')}.wav")))
    member = block.get("member") or pick.get("name")
    entry = {"type": "forge", "category": category, "member": member}
    entry.update({k: block[k] for k in ("recipe", "fn", "params", "seed", "peak_db", "gain_db", "gain", "mode")
                  if k in block})
    entry["meta"] = _meta(raw, category, block.get("params"))
    return f"{category}/{member}", entry, raw


def build(dirs: Iterable[str], out: str, version: str | None = None, log: Callable | None = None) -> dict:
    """Build the forge file from forge final folders (part 1, 7.1)."""
    doc = _header("forge_v2", version, "built from forge finals; each entry re-renders its WAV within one 24-bit step")
    doc["categories"], doc["entries"] = {}, {}
    failures: list[str] = []
    for folder in _final_dirs(dirs):
        report = json.loads(Path(folder, "report.json").read_text(encoding="utf-8"))
        cat = _dir_category(folder)
        for pick in report.get("picks") or []:
            _build_pick(doc, report, pick, folder, cat, failures, log)
    _refuse(failures)
    return _wrote(log, out, doc)


def _build_pick(doc: dict, report: dict, pick: dict, folder: str, cat: str, failures: list[str],
                log: Callable | None) -> None:
    name = f"{cat}/{pick.get('name')}"
    try:
        sound_id, entry, raw = _pick_entry(report, pick, folder, cat)
    except (ValueError, OSError, B.BankError) as exc:
        failures.append(f"{name}: {exc}")
        return
    if sound_id in doc["entries"]:
        failures.append(f"{sound_id}: two picks with one id")
        return
    doc["entries"][sound_id] = entry
    doc["categories"].setdefault(cat, _category(cat, entry.get("recipe"), report.get("target")))
    _say(log, f"{sound_id}: {len(raw) / B.SR:.3f} s, err <= {BUILD_TOL:g}")


# ---------------------------------------------------------------- 7.2 pluck


def pluck_params(edits: dict, factors: dict) -> dict:
    """The pluck DEFAULTS at C5 with set values and factors applied."""
    from hark.forge import recipes

    params = copy.deepcopy(dict(recipes.load("pluck").DEFAULTS))
    params["f0"] = PLUCK_F0
    params.update(edits)
    for key, factor in factors.items():
        params[key] = params[key] * factor
    return params


def build_pluck(out: str, version: str | None = None, log: Callable | None = None) -> dict:
    """Build the pluck file: 3 kalimba voices (part 1, 7.2)."""
    doc = _header("pluck", version, "tuned kalimba voices at C5; each take re-tunes with the pitch knob")
    doc["categories"] = {"pluck": _category("pluck")}
    doc["entries"] = {}
    for member, seed, edits, factors, notes in PLUCK_MEMBERS:
        sound_id = f"pluck/{member}"
        entry = _entry("pluck", member, "pluck", "render", pluck_params(edits, factors), seed, notes=notes)
        entry["meta"] = _meta(_render_entry(entry, sound_id), "pluck")
        doc["entries"][sound_id] = entry
        _say(log, f"{sound_id}: {entry['meta']['dur_s']:.3f} s")
    return _wrote(log, out, doc)


# ---------------------------------------------------------------- palette members (7.3, 12.2)


def _defaults_for(module: Any, fn: str) -> dict:
    func = getattr(module, fn, None)
    if fn != "render" and func is not None:
        for obj in vars(module).values():
            if getattr(obj, "render", None) is func and isinstance(getattr(obj, "DEFAULTS", None), dict):
                return dict(obj.DEFAULTS)
    return dict(getattr(module, "DEFAULTS", {}) or {})


def _is_space(params: dict) -> bool:
    """True for a search sub-space (ranges as tuples), not a parameter set (hover's FAMILIES)."""
    return any(isinstance(v, tuple) for v in params.values())


def _from_families(module: Any, category: str, member: str) -> dict | None:
    families = getattr(module, "FAMILIES", None) or {}
    for cat in [category, *sorted(families)]:
        found = dict(families.get(cat) or []).get(member)
        if found is not None and not _is_space(found):
            return dict(found)
    return None


def _from_family(module: Any, category: str, member: str) -> dict | None:
    family, names = getattr(module, "FAMILY", None) or {}, getattr(module, "FAMILY_NAMES", None) or {}
    for cat in [category, *sorted(family)]:
        labels = [*names.get(cat, [])]
        if member in labels and labels.index(member) < len(family.get(cat, [])):
            return dict(family[cat][labels.index(member)])
    return None


# Extra palettes kept in their own tables: (params table, names table).
_NAMED_TABLES = (("RUNS", "RUN_NAMES"), ("FAMILY_MID", "FAMILY_NAMES_MID"))


def _names_for(family: str, member: str) -> list[str]:
    """The member name, plus the name without the family's lead word (notify/mid_tine -> tine in mid_ding)."""
    lead = family.split("_", 1)[0] + "_"
    return [member, member[len(lead):]] if "_" in family and member.startswith(lead) else [member]


def _from_named_tables(module: Any, category: str, member: str) -> dict | None:
    for table_name, names_name in _NAMED_TABLES:
        table, names = getattr(module, table_name, None) or {}, getattr(module, names_name, None) or {}
        for family in sorted(table):
            labels = [*names.get(family, [])]
            for name in _names_for(family, member):
                if name in labels and labels.index(name) < len(table[family]):
                    return dict(table[family][labels.index(name)])
    return None


def _from_flat_tables(module: Any, category: str, member: str) -> dict | None:
    """Member -> params tables: FAMILIES_BY_CAT[category], then the flat R6 and FAMILIES (riser)."""
    by_cat = (getattr(module, "FAMILIES_BY_CAT", None) or {}).get(category) or {}
    for table in (by_cat, getattr(module, "R6", None) or {}, getattr(module, "FAMILIES", None) or {}):
        found = table.get(member) if isinstance(table, dict) else None
        if isinstance(found, dict) and not _is_space(found):
            return dict(found)
    return None


def _from_recipe_for(module: Any, category: str, member: str) -> dict | None:
    for hook in ("recipe_for", "preset_full", "preset"):
        func = getattr(module, hook, None)
        try:
            got = func(category, member) if hook == "recipe_for" else func(member) if func else None
        except (KeyError, ValueError, TypeError, IndexError):
            got = None
        if isinstance(got, dict):
            return dict(got)
        if got is not None and isinstance(getattr(got, "DEFAULTS", None), dict):
            return dict(got.DEFAULTS)
    return None


def _from_seed_bank(recipe: str, category: str, member: str) -> dict | None:
    from hark.forge import recipes

    try:
        path = recipes.seed_path(recipe)
    except recipes.UnknownRecipe:  # a recipe outside the package registry has no seed bank
        return None
    if not os.path.isfile(path):
        return None
    entries = json.loads(Path(path).read_text(encoding="utf-8")).get("entries") or []
    hits = [e for e in entries if e.get("member") == member]
    # "delivered" names the category the member was delivered for: prefer that category, then any delivery
    hits.sort(key=lambda e: 0 if e.get("delivered") == category else 1 if e.get("delivered") else 2)
    return dict(hits[0]["params"]) if hits else None


def member_params(recipe: str, fn: str, category: str, member: str) -> dict:
    """Full params of a named palette member, completed with the recipe defaults."""
    from hark.forge import recipes

    module = recipes.load(recipe)
    for finder in (_from_families, _from_family, _from_named_tables, _from_flat_tables, _from_recipe_for):
        found = finder(module, category, member)
        if found is not None:
            return {**_defaults_for(module, fn), **found}
    found = _from_seed_bank(recipe, category, member)
    if found is None:
        raise B.BankError(f"recipe {recipe} has no palette member {member!r}")
    return {**_defaults_for(module, fn), **found}


# ---------------------------------------------------------------- 7.3 forge04


def block_digest(entry: dict) -> str:
    """First 10 hex of the SHA-1 of canonical JSON {recipe, fn, params, seed}."""
    core = {k: entry.get(k) for k in ("recipe", "fn", "params", "seed")}
    text = json.dumps(clean(core), sort_keys=True, separators=(",", ":"))
    return hashlib.sha1(text.encode("utf-8")).hexdigest()[:10]


def _tonal_profile(y: np.ndarray) -> dict:
    from hark.forge.recipes import tonal

    keys = ("dur_s", "attack_ms", "t20_ms", "t40_ms", "centroid_hz", "lo150", "hi4k", "tonal", "glide_st", "corr")
    got = tonal.measure(y)
    return {k: (None if got.get(k) is None else round(float(got[k]), 3)) for k in keys}


def _take_audit(entry: dict, sound_id: str, seed: int) -> dict:
    from hark.forge.bank import sfx

    role = (entry.get("kinds") or [None])[0]
    seeds = range(seed, seed + F04_TAKES)
    results = [sfx.check(B.render({**entry, "id": sound_id}, seed=s, checks=False)[0],
                         category=entry["category"], role=role, sound=sound_id) for s in seeds]
    codes = sorted({w["code"] for r in results for w in r["warnings"]})
    return {"role": role, "seeds": [seed, seed + F04_TAKES - 1], "clean": sum(r["ok"] for r in results),
            "warnings": codes, "own_ok": results[0]["ok"]}


def _originality(sound_id: str, entry: dict, ref_clap: dict | None, recorded: dict) -> dict:
    if ref_clap and sound_id in ref_clap:
        value = float(ref_clap[sound_id])
    else:
        old = recorded.get(sound_id) or {}
        value = old.get("max_ref_clap") if old.get("digest") == block_digest(entry) else None
        if value is None:
            return dict(NULL_ORIGINAL)
    if value > ORIGINALITY_LIMIT:
        raise ValueError(f"originality {value:.3f} is over the limit {ORIGINALITY_LIMIT}")
    return {"made": "recipe", "max_ref_clap": round(value, 3)}


def _f04_member(sound_id: str, row: dict, ref_clap: dict | None, recorded: dict) -> dict:
    category, member = sound_id.split("/", 1)
    params = member_params(row["recipe"], row["fn"], category, member)
    entry = _entry(category, member, row["recipe"], row["fn"], params, row["seed"],
                   kinds=[*row["kinds"]], notes=row.get("notes"))
    raw = _render_entry(entry, sound_id)
    y = B.normalize(raw, B.DEFAULT_PEAK).astype(np.float32)
    entry["meta"] = M.meta_of(y)
    entry["profile"] = _tonal_profile(y)
    audit = _take_audit(entry, sound_id, row["seed"])
    own_ok = audit.pop("own_ok")
    if not own_ok or audit["clean"] < F04_CLEAN_MIN:
        first, last = audit["seeds"]
        raise AuditRefusal(f"take audit: {audit['clean']} / {F04_TAKES} clean on seeds {first}-{last}, own take "
                           f"{'clean' if own_ok else 'not clean'} ({', '.join(audit['warnings'])})")
    entry["audit"] = audit
    entry["original"] = _originality(sound_id, entry, ref_clap, recorded)
    return entry


class AuditRefusal(ValueError):
    """A forge04 member that fails its take audit (left out of the file unless strict)."""


def _f04_left_out(log: Callable | None, refused: list[str]) -> None:
    """Log each member left out by its take audit, then a warning summary."""
    if not refused:
        return
    for line in refused:
        _say(log, f"refused {line}")
    _say(log, f"warning: {len(refused)} member(s) failed the take audit and are left out (--strict refuses the file)")


def build_forge04(out: str, version: str | None = None, ref_clap: dict | None = None,
                  log: Callable | None = None, recorded: dict | None = None, strict: bool = False) -> dict:
    """Build the forge04 file from the recipe palettes (part 1, 7.3).

    ``recorded`` maps an id to ``{digest, max_ref_clap}`` of an earlier
    measurement. Without a value the entry gets the null originality marker.
    A member that fails its take audit is left out and logged; ``strict``
    refuses the whole file instead. Every other refusal refuses the file.
    """
    members = B.catalog()["forge04"]
    doc = _header("forge04", version, "short, soft palette members for launch films cut to music")
    doc["categories"] = {c: _category(c) for c in
                         ("cut_hit", "reverse_swell", "ui_pop", "knock", "mallet", "drip", "shutter")}
    doc["entries"], failures, refused = {}, [], []
    for sound_id in sorted(members):
        try:
            doc["entries"][sound_id] = _f04_member(sound_id, members[sound_id], ref_clap, recorded or {})
        except AuditRefusal as exc:
            refused.append(f"{sound_id}: {exc}")
            continue
        except (ValueError, B.BankError, ImportError, AttributeError) as exc:
            failures.append(f"{sound_id}: {exc}")
            continue
        if ref_clap and sound_id in ref_clap:
            _say(log, f"{sound_id}: digest {block_digest(doc['entries'][sound_id])}")
    _refuse(failures + (refused if strict else []))
    _f04_left_out(log, refused)
    return _wrote(log, out, doc)


# ---------------------------------------------------------------- 12.2 derive


def _derive_fn(recipe: str, category: str) -> str:
    """The render function name of a category: a second render where the category needs one."""
    if category != "pro_typing_run":
        return "render"
    from hark.forge import recipes
    from hark.forge.engine import schema

    module = recipes.load(recipe)
    for obj in vars(module).values():
        if getattr(obj, "REFSET", None) == "pro_typing_run" and callable(getattr(obj, "render", None)):
            ref = schema.render_ref(obj.render)
            if ref:
                return ref[1]
    return "render"


def target_misses(sound_id: str, meta: dict) -> list[str]:
    """Measures of a derived entry that miss its catalogue row (index, Data assets)."""
    row = B.catalog()["targets"].get(sound_id) or {}
    return [m for rule in (_dur_miss, _peak_miss, _centroid_miss, _lufs_miss) if (m := rule(row, meta))]


def _dur_miss(row: dict, meta: dict) -> str | None:
    dur = row.get("dur_s")
    if dur and abs(meta["dur_s"] - dur) > TOLERANCE_DUR * dur:
        return f"dur {meta['dur_s']:.3f} s vs {dur:.3f}"
    return None


def _peak_miss(row: dict, meta: dict) -> str | None:
    dur, peak = row.get("dur_s"), row.get("peak_s")
    if peak is not None and dur and abs(meta["peak_s"] - peak) > max(0.02, 0.1 * dur):
        return f"peak {meta['peak_s']:.4f} s vs {peak:.4f}"
    return None


def _centroid_miss(row: dict, meta: dict) -> str | None:
    cen, got = row.get("centroid_hz"), meta.get("centroid_hz")
    if cen and got and not 1 / TOLERANCE_CENTROID <= got / cen <= TOLERANCE_CENTROID:
        return f"centroid {got:.0f} Hz vs {cen:.0f}"
    return None


def _lufs_miss(row: dict, meta: dict) -> str | None:
    lufs, got = row.get("lufs_m_max"), meta.get("lufs_m_max")
    if lufs is not None and got is not None and abs(got - lufs) > TOLERANCE_LUFS:
        return f"LUFS {got:.1f} vs {lufs:.1f}"
    return None


def _forge_v2_ids() -> list[str]:
    skip = set(B.catalog()["forge04"])
    return sorted(i for i in B.catalog()["targets"]
                  if not i.startswith(("fit/", "pluck/")) and i not in skip)


def _log_gap(got: float, want: float, tolerance: float) -> float:
    """|log(got / want)| in units of a ratio tolerance."""
    return abs(math.log(max(got, 1e-3) / max(want, 1e-3))) / math.log(tolerance)


def _time_gaps(row: dict, meta: dict) -> list[float]:
    dur = row.get("dur_s") or meta["dur_s"]
    gaps = [_log_gap(meta["dur_s"], dur, 1 + TOLERANCE_DUR)]
    if row.get("peak_s") is not None:
        gaps.append(abs(meta["peak_s"] - row["peak_s"]) / max(0.02, 0.1 * dur))
    return gaps


def _tone_gaps(row: dict, meta: dict) -> list[float]:
    gaps = []
    if row.get("centroid_hz") and meta.get("centroid_hz"):
        gaps.append(_log_gap(meta["centroid_hz"], row["centroid_hz"], TOLERANCE_CENTROID))
    if row.get("lufs_m_max") is not None and meta.get("lufs_m_max") is not None:
        gaps.append(abs(meta["lufs_m_max"] - row["lufs_m_max"]) / TOLERANCE_LUFS)
    return gaps


def _target_distance(row: dict, meta: dict) -> float:
    """How far a render's measures are from a catalogue row, in units of the row tolerances."""
    return float(sum(_time_gaps(row, meta)) + sum(_tone_gaps(row, meta)))


def _seed_members(recipe: str, category: str) -> dict:
    """member -> params of the seed-bank entries delivered for the category."""
    from hark.forge import recipes

    try:
        path = recipes.seed_path(recipe)
    except recipes.UnknownRecipe:  # a recipe outside the package registry has no seed bank
        return {}
    if not os.path.isfile(path):
        return {}
    entries = json.loads(Path(path).read_text(encoding="utf-8")).get("entries") or []
    return {e["member"]: dict(e["params"]) for e in entries if e.get("delivered") == category and e.get("member")}


def _candidates(recipe: str, fn: str, category: str, cache: dict) -> dict:
    """member -> (params, meta) of every render the category can fall back on, rendered once."""
    key = (recipe, category)
    if key not in cache:
        from hark.forge import recipes

        base = _defaults_for(recipes.load(recipe), fn)
        pool = {m: {**base, **p} for m, p in _seed_members(recipe, category).items()} or {"defaults": base}
        rows = {m: (p, _meta(_render_entry(_entry(category, m, recipe, fn, p, 0), f"{category}/{m}"), category, p))
                for m, p in pool.items()}
        # Row order (T-41; the spec gives no rule): ascending rendered length, then member id. It breaks distance ties.
        cache[key] = dict(sorted(rows.items(), key=lambda kv: (kv[1][1]["dur_s"], kv[0])))
    return cache[key]


def _ranked(sound_id: str, recipe: str, fn: str, cache: dict, used: set) -> list[str]:
    """Candidates nearest first; a candidate used before costs 1 tolerance unit more; ties keep the row order."""
    category = sound_id.split("/", 1)[0]
    row = B.catalog()["targets"].get(sound_id) or {}
    pool = _candidates(recipe, fn, category, cache)
    return sorted(pool, key=lambda m: _target_distance(row, pool[m][1]) + ((category, m) in used))


def _closest(sound_id: str, recipe: str, fn: str, cache: dict, used: set) -> tuple[str, dict]:
    """The candidate closest to the catalogue row."""
    category = sound_id.split("/", 1)[0]
    best = _ranked(sound_id, recipe, fn, cache, used)[0]
    used.add((category, best))
    return best, dict(_candidates(recipe, fn, category, cache)[best][0])


class Dropped(ValueError):
    """A derived id with no render whose default take passes sfx check; the file lists it under ``dropped``."""


def _derived(sound_id: str, recipe: str, fn: str, params: dict) -> dict | None:
    """The entry for these params, or None when its default take fails sfx check (a known warning passes)."""
    category, member = sound_id.split("/", 1)
    entry = _entry(category, member, recipe, fn, params, 0, mode="designed")
    if sound_id not in KNOWN_WARNINGS and not B.render({**entry, "id": sound_id})[1]["checks"]["ok"]:
        return None
    entry["meta"] = _meta(_render_entry(entry, sound_id), category, entry["params"])
    return entry


def _first_source(sound_id: str, recipe: str, fn: str, cache: dict, used: set,
                  log: Callable | None) -> tuple[str | None, dict]:
    """The named palette member, else the closest render (ARCHITECTURE 12.2): (closest member or None, params)."""
    category, member = sound_id.split("/", 1)
    try:
        return None, member_params(recipe, fn, category, member)
    except B.BankError:
        source, params = _closest(sound_id, recipe, fn, cache, used)
        _say(log, f"{sound_id}: no palette member {member!r}; closest render is {recipe} {category}/{source}")
        return source, params


def _derive_entry(sound_id: str, cache: dict, used: set, log: Callable | None) -> dict:
    """The derived entry whose default take passes sfx check; ``Dropped`` when no candidate render passes."""
    category = sound_id.split("/", 1)[0]
    recipe = B.catalog()["categories"][category]["recipe"]
    fn = _derive_fn(recipe, category)
    source, params = _first_source(sound_id, recipe, fn, cache, used, log)
    entry = _derived(sound_id, recipe, fn, params)
    if entry is not None:
        return entry
    # T-41: the first render fails sfx check, so try the next-closest renders in order
    for member in _ranked(sound_id, recipe, fn, cache, used):
        if member == source:
            continue
        entry = _derived(sound_id, recipe, fn, dict(_candidates(recipe, fn, category, cache)[member][0]))
        if entry is not None:
            used.add((category, member))
            _say(log, f"{sound_id}: first render fails sfx check; next-closest passing render is {category}/{member}")
            return entry
    raise Dropped(f"no render of {recipe} {category} passes sfx check (first render and "
                  f"{len(_candidates(recipe, fn, category, cache))} candidate(s) tried)")


def build_derive(out: str, version: str | None = None, log: Callable | None = None) -> dict:
    """The forge file from the recipe family tables, seed 0 (ARCHITECTURE 12.2).

    Every entry's default take passes sfx check, except the known warnings. An id with
    no passing render is left out and listed under ``dropped`` with its reason.
    """
    doc = _header("forge_v2", version, "derived from the recipe family tables (seed 0, designed); no critic run")
    doc["dropped"], doc["categories"], doc["entries"], failures = {}, {}, {}, []
    cache: dict = {}
    used: set = set()
    for sound_id in _forge_v2_ids():
        try:
            entry = _derive_entry(sound_id, cache, used, log)
        except Dropped as exc:
            doc["dropped"][sound_id] = str(exc)
            _say(log, f"{sound_id}: dropped: {exc}")
            continue
        except (ValueError, B.BankError, ImportError, AttributeError, KeyError) as exc:
            failures.append(f"{sound_id}: {exc}")
            continue
        doc["entries"][sound_id] = entry
        doc["categories"].setdefault(entry["category"], _category(entry["category"]))
        misses = target_misses(sound_id, entry["meta"])
        _say(log, f"{sound_id}: {entry['meta']['dur_s']:.3f} s" + (f"; misses {'; '.join(misses)}" if misses else ""))
    _refuse(failures)
    return _wrote(log, out, doc)


# ---------------------------------------------------------------- 7.4 add-fit


def _load_json_arg(value: dict | str, what: str) -> dict:
    if isinstance(value, dict):
        return copy.deepcopy(value)
    try:
        return json.loads(Path(value).read_text(encoding="utf-8"))
    except (OSError, ValueError) as exc:
        raise B.BankError(f"cannot read the {what} {value}: {exc}") from None


def _load_model(model: dict | str) -> dict:
    if isinstance(model, dict):
        return copy.deepcopy(model)
    import hark.fit

    return hark.fit.load(str(model))


def check_source(source: dict, allow_license: bool) -> None:
    """The source keys and licence rules of a fit entry (part 1, 7.4 steps 3-4)."""
    _source_keys(source)
    _licence_rules(source["license"], allow_license)


def _source_keys(source: dict) -> None:
    missing = [k for k in SOURCE_KEYS if not isinstance(source.get(k), str) or not source.get(k).strip()]
    if missing:
        raise B.BankError(f"source misses {', '.join(missing)} (needs {', '.join(SOURCE_KEYS)})")


def _licence_rules(licence: str, allow_license: bool) -> None:
    lic = licence.lower()
    if REFUSED_WORDS.search(lic) or any(t in lic for t in REFUSED_TEXT):
        raise B.BankError(f"licence {licence!r} forbids a derivative inside the package")
    if not any(t in lic for t in FREE_LICENCES) and not allow_license:
        raise B.BankError(f"licence {licence!r} is not CC0 or public domain (--allow-license to override)")


def _has_vary_knob(model: dict) -> bool:
    from hark.fit.pipeline import auto

    return "vary" in auto.knobs_of(model)


def _fit_doc(path: str) -> dict:
    if os.path.exists(path):
        return B._read_file(path)
    doc = _header("fit", "fit1h", "recipes fitted from CC0 recordings")
    doc["categories"] = {"fit": {"recipe": None, "target": None, "kinds": [], "use": FIT_USE}}
    doc["entries"] = {}
    return doc


def _fit_entry(model: dict, sound_id: str, source: dict, seed: int, vary: float, kinds: list | None,
               notes: str | None) -> dict:
    from hark.fit.pipeline import auto

    report = model.pop("report", None) or {}
    role = sound_id.split("/")[1]
    entry = {"type": "fit", "category": "fit", "member": sound_id[len("fit/"):], "model": model, "seed": int(seed),
             "vary": float(vary), "source": {k: source[k] for k in SOURCE_KEYS},
             "fit": {"kind": auto.kind_of(model), "chosen": report.get("chosen", model.get("chosen")),
                     "class": report.get("class", model.get("class"))},
             "kinds": [*(kinds or [role])],
             "notes": notes or f"fitted from {source['title']} ({source['license']})"}
    y = B.normalize(_render_entry(entry, sound_id), B.DEFAULT_PEAK).astype(np.float32)
    entry["pitch_hz"] = M.fit_note(y, tonal=report.get("tonal", True) is not False)
    entry["meta"] = M.meta_of(y)
    return entry


def add_fit(model: dict | str, sound_id: str, source: dict | str, bank: str | None = None, seed: int = 0,
            vary: float = 0.0, kinds: list[str] | None = None, notes: str | None = None,
            allow_license: bool = False, force: bool = False, log: Callable | None = None) -> dict:
    """Add one fitted model to a bank file (part 1, 7.4). Returns the entry plus its id."""
    _check_fit_id(sound_id)
    model, source = _load_model(model), _load_json_arg(source, "source")
    check_source(source, allow_license)
    _check_vary(model, sound_id, vary)
    path = str(bank or Path(B.BANK_DIR) / "fit.json")
    doc = _fit_doc(path)
    _check_free(doc, sound_id, path, force)
    entry = _fit_entry(model, sound_id, source, seed, vary, kinds, notes)
    doc["entries"][sound_id] = entry
    doc["hark"] = _hark_version()
    write_bank(path, doc)
    _say(log, _fit_line(sound_id, path, entry, source))
    return {**entry, "id": sound_id}


def _check_fit_id(sound_id: Any) -> None:
    if not isinstance(sound_id, str) or not FIT_ID.match(sound_id):
        raise B.BankError(f"fit id {sound_id!r} must look like fit/<role>/<name> ({FIT_ID.pattern})")


def _check_vary(model: dict, sound_id: str, vary: float) -> None:
    if vary and not _has_vary_knob(model):
        raise B.BankError(f"{sound_id}: this fit has no vary knob; takes still differ by seed")


def _check_free(doc: dict, sound_id: str, path: str, force: bool) -> None:
    if sound_id in doc["entries"] and not force:
        raise B.BankError(f"{sound_id} is already in {path} (use --force to replace it)")


def _fit_line(sound_id: str, path: str, entry: dict, source: dict) -> str:
    pitch = f", {entry['pitch_hz']} Hz" if entry["pitch_hz"] else ""
    return f"{sound_id} -> {path}: {entry['meta']['dur_s']:.3f} s, {entry['fit']['kind']}{pitch}, {source['license']}"


# ---------------------------------------------------------------- 9 check


def check(ids: Iterable[str] | None = None, log: Callable | None = None) -> dict:
    """Re-render each entry at its own seed and compare with its stored meta."""
    rows = [_check_one(i) for i in (ids or B.ids())]
    for r in rows:
        state = "ok" if r["ok"] else "DIFFERS"
        _say(log, f"{r['id']}: len {r['len_diff']:+d}, peak {r['peak_diff_ms']:+.2f} ms, env {r['env_db']:.2f} dB {state}")
    bad = [r["id"] for r in rows if not r["ok"]]
    summary = f"{len(rows) - len(bad)} / {len(rows)} entries re-render as stored"
    if bad:
        summary += f"; differ: {', '.join(bad)}"
    _say(log, summary)
    return {"summary": summary, "ok": not bad, "rows": rows, "differ": bad}


def _check_one(sound_id: str) -> dict:
    entry = B.get(sound_id)
    try:
        y, _ = B.render(entry, checks=False)
    except (B.BankError, ValueError) as exc:
        return {"id": sound_id, "len_diff": 0, "peak_diff_ms": 0.0, "env_db": 0.0, "ok": False, "error": str(exc)}
    return {"id": sound_id, **M.compare_fp(entry.get("meta") or {}, M.meta_of(y))}
