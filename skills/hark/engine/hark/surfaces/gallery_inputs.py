"""Gallery inputs: forge folders, version comparison, forge reports, benchmark results and references."""

from __future__ import annotations

import glob
import importlib
import json
import math
import os
import re
import time
from typing import Any

SOUND_EXTS = (".wav", ".flac")
LABEL_RE = re.compile(r"^([A-Za-z0-9_.+-]{2,}):(.*)$")
STALE_TOL_S = 0.011
RENAME_TOL_S = 0.0015
PEAK_TOL_DB = 0.3
MTIME_GRACE_S = 60.0
AUDITION_NAMES = ("_audition.wav", "audition.wav")
AUDITION_LINE = re.compile(r"^\s*[-+0-9.eE]+\s*s\s+ref\s+(.+?)\s*$")


def slash(p: str) -> str:
    return p.replace("\\", "/")


def _is_sound(name: str) -> bool:
    return name.lower().endswith(SOUND_EXTS) and not name.lstrip("_").lower().startswith("audition")


def sound_files(folder: str) -> list[str]:
    """The sound files of a folder, sorted by full path, forward slashes."""
    if not os.path.isdir(folder):
        return []
    names = [n for n in os.listdir(folder) if _is_sound(n) and os.path.isfile(os.path.join(folder, n))]
    return sorted(slash(os.path.join(folder, n)) for n in names)


def category_of(folder: str) -> str:
    """The folder's name, or its parent's name when the folder's name starts with ``final``."""
    folder = os.path.normpath(folder)
    name = os.path.basename(folder)
    return os.path.basename(os.path.dirname(folder)) if name.lower().startswith("final") else name


def _hidden(name: str) -> bool:
    return name.startswith(("_", "."))


def discover(folder: str, sub: str = "final") -> dict[str, str]:
    """``{category: sound folder}`` for a forge tree, a category folder or a final folder."""
    if not os.path.isdir(folder):
        raise FileNotFoundError(f"no such folder: {folder}")
    if sound_files(folder):
        return {category_of(folder): slash(folder)}
    if sound_files(os.path.join(folder, sub)):
        return {os.path.basename(os.path.normpath(folder)): slash(os.path.join(folder, sub))}
    found = {c: slash(os.path.join(folder, c, sub)) for c in sorted(os.listdir(folder))
             if not _hidden(c) and sound_files(os.path.join(folder, c, sub))}
    if not found:
        raise FileNotFoundError(f"no sounds in {folder}: expected the layout <category>/{sub}/*.wav")
    return found


def parse_alias(alias: Any) -> dict[str, str]:
    """``OLD=NEW`` items (or a dict) to a dict; ValueError on a bad item."""
    if isinstance(alias, dict):
        return {str(k): str(v) for k, v in alias.items()}
    out: dict[str, str] = {}
    for item in alias or []:
        old, eq, new = str(item).partition("=")
        if not eq or not old.strip() or not new.strip():
            raise ValueError(f"bad --alias {item!r}: use OLD=NEW")
        out[old.strip()] = new.strip()
    return out


def split_label(item: str) -> tuple[str | None, str]:
    """``LABEL:REST`` with a label of 2+ safe characters; a 1-letter prefix is a drive letter."""
    m = LABEL_RE.match(item)
    return (m.group(1), m.group(2)) if m else (None, item)


def _compare_folders(pattern: str) -> list[str]:
    hits = sorted(glob.glob(pattern))
    if not hits and os.path.isdir(pattern):
        hits = [pattern]
    out = []
    for hit in hits:
        if not os.path.isdir(hit):
            continue
        if not sound_files(hit) and sound_files(os.path.join(hit, "final")):
            hit = os.path.join(hit, "final")
        if sound_files(hit):
            out.append(hit)
    return out


def _add_compare(versions: dict[str, dict[str, str]], label: str, pattern: str, alias: dict[str, str]) -> None:
    cats = versions.setdefault(label, {})
    for folder in _compare_folders(pattern):
        cat = category_of(folder)
        if _hidden(cat):
            continue
        cats.setdefault(alias.get(cat, cat), slash(folder))


def parse_compare(items: list[str], alias: dict[str, str]) -> dict[str, dict[str, str]]:
    """Versions in order of first appearance: ``{label: {category: folder}}``."""
    versions: dict[str, dict[str, str]] = {}
    for item in items:
        label, pattern = split_label(item)
        if label is None:
            raise ValueError(f"bad --compare {item!r}: use LABEL:GLOB (a label of 2 or more characters)")
        _add_compare(versions, label, pattern, alias)
    if not any(versions.values()):
        raise FileNotFoundError(f"--compare found no folder with sounds: {', '.join(items)}")
    return versions


# ---------------------------------------------------------------- forge report


def _read_json(path: str) -> Any:
    try:
        with open(path, encoding="utf-8") as fh:
            return json.load(fh)
    except (OSError, ValueError):
        return None


def finite(v: Any) -> bool:
    return isinstance(v, (int, float)) and not isinstance(v, bool) and math.isfinite(v)


def _recipe_text(rep: dict) -> str | None:
    recipe = rep.get("recipe")
    module = rep.get("recipe_module")
    text = module if module and module != recipe else (recipe or module)
    kind = rep.get("kind")
    if text and kind and kind != text:
        text = f"{text} ({kind})"
    return text


def _pick_key(pick: dict) -> str | None:
    if pick.get("file"):
        return os.path.basename(str(pick["file"]))
    return f"{pick['name']}.wav" if pick.get("name") else None


def read_report(folder: str) -> dict[str, Any]:
    """The parts of ``<folder>/report.json`` the page shows; empty fields when missing."""
    rep = _read_json(os.path.join(folder, "report.json"))
    rep = rep if isinstance(rep, dict) else {}
    critic = rep.get("critic") if isinstance(rep.get("critic"), dict) else {}
    picks = {}
    for p in rep.get("picks") or []:
        key = _pick_key(p) if isinstance(p, dict) else None
        if key:
            picks[key] = p
    return {"picks": picks, "target": rep.get("target"), "mode": rep.get("mode") or critic.get("mode"),
            "tag": rep.get("tag"), "refs": [r for r in rep.get("refs") or [] if isinstance(r, str)],
            "recipe": _recipe_text(rep), "method": rep.get("method") or rep.get("summary")}


def variant_of(pick: dict) -> str | None:
    for key in ("member", "family", "preset", "style", "variant"):
        v = pick.get(key)
        if isinstance(v, str) and v:
            return v
    return None


def _num(d: dict, *path: str) -> float | None:
    v: Any = d
    for k in path:
        v = v.get(k) if isinstance(v, dict) else None
    return v if finite(v) else None


def _first(*vals: Any) -> Any:
    return next((v for v in vals if v is not None), None)


def _audioset(pick: dict) -> str | None:
    top = (pick.get("holdout") or {}).get("audioset_top") if isinstance(pick.get("holdout"), dict) else None
    if isinstance(top, list) and top and isinstance(top[0], (list, tuple)) and top[0]:
        return str(top[0][0])
    return None


def _words(pick: dict) -> Any:
    score, meas = pick.get("score") or {}, pick.get("measured") or {}
    polish = meas.get("polish") if isinstance(meas.get("polish"), dict) else {}
    return score.get("polish_words") or polish.get("words") or ""


def critic_of(pick: dict) -> dict[str, Any]:
    """The critic numbers of 1 pick (finite numbers only)."""
    score = pick.get("score") if isinstance(pick.get("score"), dict) else {}
    meas = pick.get("measured") if isinstance(pick.get("measured"), dict) else {}
    robust = pick.get("robust") if isinstance(pick.get("robust"), dict) else {}
    out = {k: _num(score, k) for k in ("total", "match_z", "env_pen", "polish_pen", "art_pen")}
    out.update(passes=score.get("passes") if isinstance(score.get("passes"), bool) else None,
               tone_db=_first(_num(score, "tone_db"), _num(meas, "tone_db")),
               t40_ms=_num(meas, "t40_ms"), t40_ref=_num(meas, "t40_ref"),
               nearest_ref=score.get("nearest_ref") or meas.get("nearest_ref"),
               sounds_like=[s for s in (score.get("sounds_like") or []) if isinstance(s, str)][:3],
               words=_words(pick), robust_mean=_num(robust, "mean"), robust_pass=_num(robust, "pass_rate"),
               audioset=_audioset(pick))
    return out


# ---------------------------------------------------------------- benchmark results


def _made_entries(cat: dict) -> tuple[dict[str, dict], list[dict]]:
    per = (cat.get("inside") or {}).get("per_sound") or []
    made, rows = {}, []
    for i, e in enumerate(cat.get("made") or []):
        if not isinstance(e, dict):
            continue
        e = dict(e, inside_n=per[i] if i < len(per) else None)
        rows.append(e)
        made[os.path.basename(str(e.get("file") or ""))] = e
    return made, rows


def load_results(path: str, label: str | None) -> dict[str, Any]:
    data = _read_json(path)
    if not isinstance(data, dict) or not isinstance(data.get("categories"), dict):
        raise ValueError(f"{path} is not a benchmark results JSON (no categories)")
    cats = {}
    for name, cat in data["categories"].items():
        cat = cat if isinstance(cat, dict) else {}
        made, rows = _made_entries(cat)
        cats[name] = {"made": made, "rows": rows, "summary": cat.get("summary") or {},
                      "inside": cat.get("inside") or {}, "n_bench": cat.get("n_bench"), "n_forge": cat.get("n_forge")}
    return {"label": label, "path": slash(os.path.abspath(path)), "tag": data.get("tag"),
            "created": data.get("created"), "cats": cats}


def assign_results(items: Any, labels: list[str]) -> list[dict[str, Any]]:
    """Results files with their version labels; ValueError for unknown or missing labels."""
    items = [items] if isinstance(items, str) else list(items or [])
    out = []
    for item in items:
        label, path = split_label(item)
        if label is None:
            if len(labels) > 1:
                raise ValueError("with several versions, give each results file a label: LABEL:PATH")
            label = labels[0]
        elif label not in labels:
            raise ValueError(f"results label {label!r} matches no version; labels: {', '.join(labels)}")
        out.append(load_results(path, label))
    return out


def _created_ts(created: Any) -> float | None:
    try:
        return time.mktime(time.strptime(str(created), "%Y-%m-%d %H:%M"))
    except (ValueError, OverflowError):
        return None


def _newer(sound: dict, created: Any) -> bool:
    ts = _created_ts(created)
    try:
        return ts is not None and os.path.getmtime(sound["path"]) > ts + MTIME_GRACE_S
    except OSError:
        return False


def stale_reason(entry: dict | None, sound: dict, created: Any, tol: float = STALE_TOL_S) -> str:
    """Why a benchmark entry does not describe the file; empty when it does."""
    if not entry:
        return ""
    if _newer(sound, created):
        return "file newer than the benchmark run"
    length = entry.get("length_s")
    if finite(length) and abs(length - sound["dur_s"]) > tol:
        return f"length {sound['dur_s']:.3f} s, benchmark {length:.3f} s"
    peak = (entry.get("artifact_detail") or {}).get("peak_dbfs")
    if finite(peak) and sound.get("peak_dbfs") is not None and abs(peak - sound["peak_dbfs"]) > PEAK_TOL_DB:
        return f"peak {sound['peak_dbfs']:.1f} dBFS, benchmark {peak:.1f}"
    return ""


def _case(p: str) -> str:
    return os.path.normcase(os.path.normpath(p))


def same_file(entry_file: str, sound_path: str, results_path: str) -> bool:
    """True when the entry's ``file`` names exactly this sound path."""
    target = _case(os.path.abspath(sound_path))
    if os.path.isabs(entry_file):
        return _case(entry_file) == target
    roots = (os.getcwd(), os.path.dirname(os.path.dirname(os.path.abspath(results_path))))
    return any(_case(os.path.join(r, entry_file)) == target for r in roots)


def bench_numbers(entry: dict, n_checks: Any) -> dict[str, Any]:
    out = {k: entry.get(k) if finite(entry.get(k)) else None
           for k in ("tone_db", "t40_ratio", "typ_z", "phrase_z", "stereo_corr")}
    top = entry.get("as_top")
    label = str(top[0][0]).split(",")[0] if isinstance(top, list) and top and isinstance(top[0], (list, tuple)) and top[0] else None
    out.update(inside=entry.get("inside_n") if finite(entry.get("inside_n")) else None,
               n_checks=n_checks if finite(n_checks) else None,
               artifacts=[str(a) for a in entry.get("artifacts") or []], audioset=label)
    return out


def _rename_match(rows: list[dict], sound: dict, created: Any) -> dict | None:
    hits = [e for e in rows if not stale_reason(e, sound, created, RENAME_TOL_S)]
    return hits[0] if len(hits) == 1 else None


def match_entry(cat: dict, sound: dict, created: Any, notes: dict[str, list[str]]) -> dict | None:
    """The benchmark entry of a sound, with the rename fallback; records renames and stale files."""
    entry = cat["made"].get(os.path.basename(sound["path"]))
    reason = stale_reason(entry, sound, created) if entry else "no entry"
    if not reason:
        return entry
    if reason != "file newer than the benchmark run":
        alt = _rename_match(cat["rows"], sound, created)
        if alt is not None:
            notes["renames"].append(f"{os.path.basename(sound['path'])} = {os.path.basename(str(alt.get('file')))}")
            return alt
    if entry:
        notes["stale"].append(f"{os.path.basename(sound['path'])} ({reason})")
    return None


def _median(vals: list[Any]) -> float | None:
    import statistics
    nums = [v for v in vals if finite(v)]
    return statistics.median(nums) if nums else None


def _fmt(v: Any, fmt: str) -> str:
    return fmt.format(v) if finite(v) else "-"


def _short(v: Any) -> str:
    return f"{v:g}" if finite(v) else "-"


SUMMARY_ROWS = (
    ("variety", "var_cos", "{:.3f}", "mean CLAP similarity of the variants to each other; lower means more variety"),
    ("tail t40", "t40_ratio", "{:.2f} x", "tail time over the bench median; 1 means equal"),
    ("tone distance", "tone_db", "{:.1f} dB", "1/3-octave spectral distance from the bench median; lower is closer"),
)
INSIDE_HINT = "median per sound of how many checks fall within the spread of the real recordings"


def summary_rows(cat: dict) -> list[dict[str, str]]:
    s = cat.get("summary") or {}
    cols = [s.get("made") or {}, s.get("real_cross") or {}, s.get("real_loo") or {}]
    rows = []
    for label, key, fmt, hint in SUMMARY_ROWS:
        vals = [c.get(key) for c in cols]
        if any(finite(v) for v in vals):
            rows.append({"label": label, "made": _fmt(vals[0], fmt), "forge": _fmt(vals[1], fmt),
                         "loo": _fmt(vals[2], fmt), "hint": hint})
    inside = cat.get("inside") or {}
    n = inside.get("n_checks")
    if inside.get("per_sound") and finite(n):
        rows.append({"label": "inside", "made": f"{_short(_median(inside['per_sound']))} / {n}",
                     "forge": _short(_median(inside.get("real_per_sound") or [])), "loo": "-", "hint": INSIDE_HINT})
    return rows


def bench_summary(cat: dict) -> dict[str, Any]:
    made = (cat.get("summary") or {}).get("made") or {}
    return {"rows": summary_rows(cat), "n_bench": cat.get("n_bench"), "n_forge": cat.get("n_forge"),
            "n_made": made.get("n"), "artifacts": made.get("artifacts")}


# ---------------------------------------------------------------- alignment


def align(versions: list[list[dict] | None]) -> list[list[dict | None] | None]:
    """Display orders: later versions put equal variants under the base variants."""
    base = next(([s.get("variant") for s in v] for v in versions if v), None)
    return [_align_one(v, base) if v and base else v for v in versions]


def _realignable(sounds: list[dict], base: list[Any]) -> bool:
    names = [s.get("variant") for s in sounds]
    return all(names) and len(set(names)) == len(names) and sum(n in base for n in names) >= 2


def _align_one(sounds: list[dict], base: list[Any]) -> list[dict | None]:
    if not _realignable(sounds, base):
        return list(sounds)
    slots: list[dict | None] = [None] * max(len(base), len(sounds))
    rest = []
    for s in sounds:
        i = base.index(s["variant"]) if s["variant"] in base else -1
        if i >= 0 and slots[i] is None:
            slots[i] = s
        else:
            rest.append(s)
    for s in rest:
        free = next((i for i, v in enumerate(slots) if v is None), None)
        slots.append(s) if free is None else slots.__setitem__(free, s)
    while slots and slots[-1] is None:
        slots.pop()
    return slots


# ---------------------------------------------------------------- references


def refsets() -> Any:
    """The forge reference-set module, imported only when needed."""
    return importlib.import_module("hark.forge.engine.refsets")


def category_description(cat: str, pro_text: dict[str, str]) -> str | None:
    if cat in pro_text:
        return pro_text[cat]
    try:
        cats = getattr(refsets(), "CATEGORIES", {}) or {}
    except Exception:
        return None
    if cat in cats:
        return cats[cat]
    base = cat[4:] if cat.startswith("pro_") else None
    return f"designed version: {cats[base]}" if base and base in cats else None


def ref_paths(mode: str, cat: str, report_refs: list[str]) -> list[str]:
    if mode == "forge":
        try:
            return [slash(p) for p in refsets().refset(cat, "forge")]
        except Exception:
            return [slash(p) for p in report_refs if os.path.isfile(p)]
    forge_dir = os.path.join(mode, cat, "forge")
    return sound_files(forge_dir) or sound_files(os.path.join(mode, cat))


def guard_refs(cat: str, paths: list[str], warnings: list[str]) -> list[str]:
    """Drop held-out references; with no guard, drop every reference of the card."""
    try:
        held = {_case(os.path.abspath(p)) for p in refsets().held_out(paths)}
    except Exception as exc:
        if paths:
            warnings.append(f"{cat}: no references on this card, because the held-out check could not run "
                            f"({type(exc).__name__}: {exc})")
        return []
    kept = [p for p in paths if _case(os.path.abspath(p)) not in held]
    if len(kept) < len(paths):
        warnings.append(f"{cat}: {len(paths) - len(kept)} held-out reference(s) dropped")
    return kept


def pair_refs(order: list[dict | None], refs: list[str], cap: int) -> list[str | None]:
    """1 reference per shown sound: its nearest_ref when listed, else the references in turn."""
    if not refs:
        return []
    by_name = {os.path.basename(p): p for p in refs}
    out: list[str | None] = []
    turn = 0
    for s in order:
        if s is None:
            out.append(None)
            continue
        near = (s.get("critic") or {}).get("nearest_ref")
        if near in by_name:
            out.append(by_name[near])
        else:
            out.append(refs[turn % len(refs)])
            turn += 1
    out = out[:cap]
    real = {p for p in out if p}
    return [next(iter(real))] if len(real) == 1 else out


def _held_names(names: list[str]) -> list[str]:
    mod = refsets()
    try:
        reg = mod.load()
    except Exception as exc:
        if type(exc).__name__ != "RefsetsMissing" and not isinstance(exc, FileNotFoundError):
            raise
        reg = {"categories": {}}
    bench = {os.path.basename(p) for c in reg.get("categories", {}).values() for p in c.get("bench") or []}
    stems = set(getattr(mod, "KIT_BENCH", ()) or ())
    return [n for n in names if os.path.basename(n) in bench or os.path.splitext(os.path.basename(n))[0] in stems]


def _index_names(index: str) -> list[str] | None:
    try:
        with open(index, encoding="utf-8") as fh:
            lines = fh.read().splitlines()
    except OSError:
        return None
    return [m.group(1) for m in map(AUDITION_LINE.match, lines) if m]


def audition_reel(folder: str, cat: str, warnings: list[str]) -> str | None:
    """The audition reel of a folder when its index can be checked and lists no held-out reference."""
    reel = next((os.path.join(folder, n) for n in AUDITION_NAMES if os.path.isfile(os.path.join(folder, n))), None)
    if reel is None:
        return None
    names = _index_names(os.path.splitext(reel)[0] + ".txt")
    if names is None:
        warnings.append(f"{cat}: audition reel left out: no index file beside it")
        return None
    try:
        held = _held_names(names)
    except Exception as exc:
        warnings.append(f"{cat}: audition reel left out: the held-out check is unavailable ({type(exc).__name__})")
        return None
    if held:
        warnings.append(f"{cat}: audition reel left out: it plays held-out refs ({', '.join(held[:3])})")
        return None
    return slash(reel)
