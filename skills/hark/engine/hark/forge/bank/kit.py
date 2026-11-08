"""``hark kit``: a film's sounds by role, rendered into cached takes and a lock.

Spec: docs/specs/hark/bank-kit-sfx-kit.md (part 3). The mix helpers at the end
are pure numpy and scipy functions that a mixer calls.
"""

from __future__ import annotations

import argparse
import difflib
import hashlib
import json
import math
import numbers
import os
import time
from collections.abc import Callable
from pathlib import Path
from typing import Any

import numpy as np

from hark.forge.bank import bank, sfx
from hark.forge.bank._measures import LUFS_OFFSET, SR, k_power, loudest_mean, measure

SCHEMA = 1
LOCK_NAME = "kit.lock.json"
TAKES_MAX = 32
REROLL = 3
REROLL_STEP = 1000
END_RING_S = 0.03
ALIGNS = ("onset", "peak", "end")
EDIT_KEYS = ("dur", "apex", "pitch", "peak_db", "weight")
ROLE_KEYS = ("sound", "takes", "seed", "edits", "pitches", "align", "max_s", "level_db", "note", "bed_db",
             "offset_ms", "min_gap_ms", "width")
MIX_RANGES = {"bed_db": (0.5, 30.0), "offset_ms": (-500.0, 500.0), "min_gap_ms": (0.0, 2000.0), "width": (0.0, 1.0)}
MAX_S_MIN = 0.01
ROLE_PATTERN = r"^[a-z][a-z0-9_]*(\.[a-z0-9_]+)*$"
ROLES = ("tick", "tick.crisp", "tock", "hit", "land", "land.light", "pop", "whoosh.short", "whoosh.long", "suck",
         "riser", "drop", "key", "key.space", "key.return", "click", "click.confirm", "snap", "swell", "bell", "text")
ALIGN_DEFAULT = {"whoosh.short": "peak", "whoosh.long": "peak", "suck": "end", "riser": "end", "swell": "end"}
RESTRAINED = ("riser", "swell", "suck")
WIDTH_TOL = 0.05
WIDTH_STEPS = 40
DECOR_DELAYS = (149, 263, 397)
DECOR_GAINS = (0.6, -0.55, 0.5)
DECOR_HP_HZ = 250.0
DECOR_FADE = 240
DECOR_MAX = 4.0
LOUD_WIN = 2400
BED_SPAN_S = 1.0
BED_FLOOR = -60.0
AUDITION_BED = -24.0
GAIN_CEILING = -1.0
AUDITION_CEILING = -1.0
PRESETS_DIR = Path(__file__).resolve().parent.parent / "kits"
UNUSED_SOUNDS = {
    "pro_zip/up": "the pitch goes up, which reads as a cheap synth whip",
    "pro_zip/up_hi": "the pitch goes up, which reads as a cheap synth whip",
    "pro_zip/arc": "the pitch goes up, which reads as a cheap synth whip",
    "riser/shepard": "an endless (Shepard) rise",
    "downlifter/shepard_fall": "an endless (Shepard) fall",
    "downlifter/synth_fall": "a synthesizer fall",
}
UNUSED_CATEGORIES = {
    "logo_sting": "a layered sting: lead-in, hit and chime at once",
    "pro_logo_sting": "a layered launch gesture",
    "shimmer": "a shimmer wash",
    "pro_shimmer": "a granular shimmer",
    "glitch": "a glitch",
    "pro_glitch": "a glitch",
    "pro_pop": "tuned pops, which can read as 8-bit sound",
}


class KitError(ValueError):
    """A kit problem: not JSON, an invalid kit, an unknown preset, no sounded role."""


# ---------------------------------------------------------------- helpers


def _number(v: Any) -> bool:
    return isinstance(v, numbers.Real) and not isinstance(v, bool) and math.isfinite(float(v))


def _integer(v: Any) -> bool:
    return isinstance(v, numbers.Integral) and not isinstance(v, bool)


def canonical(obj: Any) -> str:
    """Canonical JSON: keys sorted, separators ``,`` and ``:``, other values as text."""
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), default=str)


def digest(obj: Any) -> str:
    """SHA-1 hex of the canonical JSON of ``obj``."""
    return hashlib.sha1(canonical(obj).encode("utf-8")).hexdigest()


def _knobs() -> Any:
    from hark.forge.engine import knobs

    return knobs


def default_align(role: str) -> str:
    """The default sync point of a role (part 3, section 2)."""
    return ALIGN_DEFAULT.get(role, "onset")


def _hark_version() -> str:
    import hark

    return str(getattr(hark, "__version__", "0.1.0"))


# ---------------------------------------------------------------- load and validate


def load(kit: Any) -> dict:
    """Read a kit (path or dict) and validate it; ``KitError`` on any problem."""
    doc = _read(kit)
    found = problems(doc)
    if found:
        raise KitError("invalid kit: " + "; ".join(found))
    return doc


def _read(kit: Any) -> dict:
    if isinstance(kit, dict):
        return kit
    try:
        text = Path(kit).read_text(encoding="utf-8")
    except OSError as exc:
        raise KitError(f"cannot read {kit}: {exc}") from None
    try:
        return json.loads(text)
    except ValueError as exc:
        raise KitError(f"{kit} is not JSON: {exc}") from None


def problems(kit: Any) -> list[str]:
    """Every problem of a kit, ``[]`` when it is valid (part 3, section 4)."""
    if not isinstance(kit, dict) or kit.get("kit") != SCHEMA:
        return ["not a kit file"]
    out: list[str] = []
    if kit.get("sr", SR) not in bank.RATES or isinstance(kit.get("sr", SR), bool):
        out.append(f"sr {kit.get('sr')!r} is not one of {', '.join(map(str, bank.RATES))}")
    roles = kit.get("roles")
    if not isinstance(roles, dict) or not roles:
        return out + ["roles must be a non-empty object"]
    for role, spec in roles.items():
        out += _role_problems(role, spec)
    out += _collisions(kit)
    out += _mix_problems(kit.get("mix"), roles)
    return [*dict.fromkeys(out)]


def _role_problems(role: str, spec: Any) -> list[str]:
    import re

    out = [] if re.match(ROLE_PATTERN, str(role)) else [f"role {role!r}: the name must be lowercase words joined by dots"]
    if spec is None:
        return out
    if not isinstance(spec, dict):
        return out + [f"role {role}: must be an object or null"]
    for check in (_keys_problem, _sound_problem, _count_problems, _edits_shape, _shape_problems, _value_problems):
        out += check(role, spec)
    return out


def _keys_problem(role: str, spec: dict) -> list[str]:
    extra = sorted(set(spec) - set(ROLE_KEYS))
    if not extra:
        return []
    return [f"role {role}: unknown keys {', '.join(extra)} (known: {', '.join(ROLE_KEYS)})"]


def _sound_problem(role: str, spec: dict) -> list[str]:
    sound = spec.get("sound")
    known = bank.ids()
    if isinstance(sound, str) and sound in known:
        return []
    close = difflib.get_close_matches(str(sound), known, n=3, cutoff=0.5)
    hint = f" (did you mean {', '.join(close)}?)" if close else ""
    return [f"role {role}: sound {sound!r} is not a bank id{hint}"]


def _count_problems(role: str, spec: dict) -> list[str]:
    out = []
    takes = spec.get("takes", 1)
    if not _integer(takes) or not 1 <= takes <= TAKES_MAX:
        out.append(f"role {role}: takes must be an integer from 1 to {TAKES_MAX}")
    if "seed" in spec and not _integer(spec["seed"]):
        out.append(f"role {role}: seed must be an integer")
    return out


def _edits_of(spec: dict) -> dict:
    edits = spec.get("edits", {})
    if not isinstance(edits, dict) or set(edits) - set(EDIT_KEYS):
        return {}
    return edits


def _pitches_of(spec: dict) -> list | None:
    p = spec.get("pitches")
    return p if isinstance(p, list) and p else None


def _edits_shape(role: str, spec: dict) -> list[str]:
    edits = spec.get("edits", {})
    if not isinstance(edits, dict):
        return [f"role {role}: edits must be an object"]
    extra = sorted(set(edits) - set(EDIT_KEYS))
    return [f"role {role}: unknown edits {', '.join(extra)} (known: {', '.join(EDIT_KEYS)})"] if extra else []


def _shape_problems(role: str, spec: dict) -> list[str]:
    out = []
    if "pitches" in spec and _pitches_of(spec) is None:
        out.append(f"role {role}: pitches must be a non-empty list")
    if _pitches_of(spec) is not None and "pitch" in _edits_of(spec):
        out.append(f"role {role}: give pitches or edits.pitch, not both")
    if spec.get("align", default_align(role)) not in ALIGNS:
        out.append(f"role {role}: align must be one of {', '.join(ALIGNS)}")
    if "level_db" in spec and not _number(spec["level_db"]):
        out.append(f"role {role}: level_db must be a finite number")
    if spec.get("max_s") is not None and not (_number(spec["max_s"]) and spec["max_s"] >= MAX_S_MIN):
        out.append(f"role {role}: max_s must be a number of at least {MAX_S_MIN}")
    for key, (lo, hi) in MIX_RANGES.items():
        if key in spec and not (_number(spec[key]) and lo <= spec[key] <= hi):
            out.append(f"role {role}: {key} must be a number from {lo:g} to {hi:g}")
    return out


def _value_problems(role: str, spec: dict) -> list[str]:
    edits = _edits_of(spec)
    out = _level_problem(role, edits)
    timing = {k: edits[k] for k in ("dur", "apex") if k in edits}
    if any(not _number(v) for v in timing.values()):
        return out + [f"role {role}: dur and apex must be numbers"]
    if spec.get("sound") not in bank.ids():
        return out
    entry = bank.get(spec["sound"])
    knobs = _knobs()
    if timing:
        out += _knob_problem(role, "", lambda: knobs.validate(entry, **timing))
    for p in _pitches_of(spec) or ([edits["pitch"]] if "pitch" in edits else []):
        out += _knob_problem(role, f" pitch {p!r}", lambda p=p: knobs.validate(entry, pitch=p))
    return out


def _level_problem(role: str, edits: dict) -> list[str]:
    try:
        bank.level(edits.get("peak_db", bank.DEFAULT_PEAK), edits.get("weight"))
    except bank.BankError as exc:
        return [f"role {role}: {exc}"]
    return []


def _knob_problem(role: str, what: str, call: Callable[[], Any]) -> list[str]:
    try:
        call()
    except ValueError as exc:
        return [f"role {role}{what}: {exc}"]
    return []


def _labels(pitches: list) -> list[str] | None:
    try:
        return [_knobs().label(p) for p in pitches]
    except ValueError:
        return None


def _collisions(kit: dict) -> list[str]:
    seen: dict[str, tuple[str, str, Any]] = {}
    out = []
    for role, spec in kit["roles"].items():
        if not isinstance(spec, dict) or _labels(_pitches_of(spec) or []) is None:
            continue
        for name, pitch in _file_names(role, spec):
            key = name.lower()
            if key in seen:
                out.append(_clash(role, name, pitch, seen[key]))
            else:
                seen[key] = (role, name, pitch)
    return out


def _clash(role: str, name: str, pitch: Any, first: tuple[str, str, Any]) -> str:
    role0, name0, pitch0 = first
    if role0 == role and pitch0 != pitch:
        return f"role {role}: pitches {pitch0} and {pitch} are one pitch ({name}): list each pitch once"
    return f"take file {name} clashes with {name0} of role {role0} when case is ignored: give the role another name"


def _mix_problems(mix: Any, roles: dict) -> list[str]:
    if mix is None:
        return []
    if not isinstance(mix, dict):
        return ["mix must be an object"]
    out = []
    if "duck_db" in mix and not _number(mix["duck_db"]):
        out.append("mix.duck_db must be a finite number")
    stop = mix.get("stop_down")
    if stop is not None:
        out += _stop_down_problems(stop, roles)
    return out


def _pair_ok(v: Any) -> bool:
    return isinstance(v, list) and len(v) == 2 and all(_number(x) for x in v) and v[0] <= v[1]


def _stop_down_problems(stop: Any, roles: dict) -> list[str]:
    if not isinstance(stop, dict):
        return ["mix.stop_down must be an object"]
    out = []
    before = stop.get("before", [])
    if not isinstance(before, list) or any(r not in roles for r in before):
        out.append("mix.stop_down.before must list role names of this kit")
    for key in ("depth_db", "length_s"):
        if key in stop and not _pair_ok(stop[key]):
            out.append(f"mix.stop_down.{key} must be a [lo, hi] pair of numbers with lo <= hi")
    return out


# ---------------------------------------------------------------- plan and keys


def _file_names(role: str, spec: dict) -> list[tuple[str, Any]]:
    stem = role.replace(".", "-")
    n = spec.get("takes", 1) if _integer(spec.get("takes", 1)) else 1
    pitches = _pitches_of(spec)
    if pitches is None:
        pitch = _edits_of(spec).get("pitch")
        return [(f"{stem}_{k}.wav", pitch) for k in range(1, n + 1)]
    labels = _labels(pitches) or []
    names = []
    for p, label in zip(pitches, labels):
        names += [(f"{stem}_{label}.wav" if n == 1 else f"{stem}_{label}_{k}.wav", p) for k in range(1, n + 1)]
    return names


def plan(kit: Any) -> list[dict]:
    """Every take a kit renders, in role order (part 3, 5.1)."""
    doc = _read(kit)
    clashes = _collisions(doc)
    if clashes:
        raise KitError("; ".join(clashes))
    takes = []
    for role, spec in doc["roles"].items():
        if spec is not None:
            takes += _role_takes(role, spec)
    return takes


def _role_takes(role: str, spec: dict) -> list[dict]:
    entry = bank.get(spec["sound"])
    base = int(spec.get("seed", entry.get("seed", 0)))
    n = int(spec.get("takes", 1))
    edits = {k: v for k, v in _edits_of(spec).items() if k != "pitch"}
    out = []
    for i, (name, pitch) in enumerate(_file_names(role, spec)):
        k = i % n + 1
        take = {"role": role, "file": name, "sound": spec["sound"], "seed": base + k - 1, "pitch": pitch,
                "edits": dict(edits), "max_s": spec.get("max_s"), "k": k}
        if "width" in spec:
            take["width"] = float(spec["width"])
        out.append(take)
    return out


def take_key(take: dict, sr: int = SR) -> str:
    """The cache key of a planned take (part 3, 5.2)."""
    doc = {"sound": take["sound"], "entry": digest(bank.get(take["sound"])), "seed": take["seed"],
           "pitch": take.get("pitch"), "edits": take.get("edits") or {}, "max_s": take.get("max_s"), "sr": sr,
           "bank": bank.VERSION, "hark": _hark_version()}
    if "width" in take:
        doc["width"] = take["width"]
    return digest(doc)


# ---------------------------------------------------------------- one take


def _render_once(take: dict, seed: int, entry: dict) -> tuple[np.ndarray, dict, dict]:
    edits = take.get("edits") or {}
    y, meta = bank.render(take["sound"], seed=seed, dur=edits.get("dur"), apex=edits.get("apex"),
                          pitch=take.get("pitch"), peak_db=edits.get("peak_db", bank.DEFAULT_PEAK),
                          weight=edits.get("weight"), checks=False)
    max_s = take.get("max_s")
    if max_s is not None and len(y) > round(max_s * SR):
        y = np.asarray(_knobs().fit_length(y, max_s)[0], dtype=np.float32)
    if "width" in take:
        y, meta["width"] = stereo_width(y, take["width"])
        y = y.astype(np.float32)
    audit = sfx.check(y, category=entry.get("category"), role=take["role"], sound=take["sound"])
    bank.add_warning(audit, _knobs().pitch_warning(meta.get("edits") or {}))
    return y, audit, meta


def render_take(take: dict) -> tuple[np.ndarray, dict, dict, int]:
    """One take at 48 kHz with re-rolls: ``(audio, audit, meta, seed used)``."""
    entry = bank.get(take["sound"])
    first = None
    for j in range(REROLL + 1):
        seed = take["seed"] + REROLL_STEP * j
        y, audit, meta = _render_once(take, seed, entry)
        if audit["ok"]:
            return y, audit, meta, seed
        first = first or (y, audit, meta, seed)
    return first


# ---------------------------------------------------------------- render and the lock


def _sha1_file(path: Path) -> str:
    return hashlib.sha1(path.read_bytes()).hexdigest()


def _old_lock(out: Path) -> dict:
    try:
        lock = json.loads((out / LOCK_NAME).read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}
    return lock if isinstance(lock, dict) else {}


def _old_takes(lock: dict) -> dict[tuple[str, str], dict]:
    index = {}
    for spec in (lock.get("roles") or {}).values():
        for t in (spec or {}).get("takes") or []:
            index[(t.get("key"), t.get("file"))] = t
    return index


def _lock_role(role: str, spec: dict | None) -> dict | None:
    if spec is None:
        return None
    out = {"sound": spec["sound"], "align": spec.get("align", default_align(role)),
           "level_db": float(spec.get("level_db", 0.0)), "max_s": spec.get("max_s")}
    out.update({k: float(spec[k]) for k in MIX_RANGES if k in spec})
    out["takes"] = []
    return out


def _new_lock(doc: dict, sr: int) -> dict:
    lock = {"kit": SCHEMA, "hark": _hark_version(), "bank": bank.VERSION, "sr": sr, "name": doc.get("name"),
            "roles": {r: _lock_role(r, s) for r, s in doc["roles"].items()}}
    if "mix" in doc:
        lock["mix"] = doc["mix"]
    return lock


def _resampled(y: np.ndarray, sr: int) -> np.ndarray:
    if sr == SR:
        return y
    from hark.core import audio

    return np.asarray(audio.resample(y, SR, sr), dtype=np.float32)


def _record(take: dict, key: str, path: Path, sr: int, role_spec: dict) -> dict:
    y, audit, meta, used = render_take(take)
    m = measure(y)
    written = _resampled(y, sr)
    sfx.write(str(path), written, sr)
    rec = {"file": take["file"], "seed": used, "pitch": take.get("pitch"), "onset_s": m["onset_s"],
           "peak_s": m["peak_s"], "end_s": m["end_s"], "dur_s": round(len(written) / sr, 4),
           "peak_db": m["peak_db"], "lufs_m_max": m["lufs_m_max"], "sha1": _sha1_file(path), "key": key,
           "checks": [w["code"] for w in audit["warnings"]]}
    if used != take["seed"]:
        rec["planned_seed"] = take["seed"]
    if "bed_db" in role_spec:
        rec["l50_lufs"] = loud_lufs(written, sr)
    if "width" in take:
        rec["corr"] = meta["width"]["measured"]
    return rec


def _cached(old: dict, take: dict, key: str, path: Path, force: bool) -> dict | None:
    rec = old.get((key, take["file"]))
    if force or rec is None or not path.is_file() or _sha1_file(path) != rec.get("sha1"):
        return None
    return dict(rec)


def _add_l50(rec: dict, path: Path, sr: int) -> None:
    from hark.core import audio

    rec["l50_lufs"] = loud_lufs(np.asarray(audio.load(str(path), sr=SR)), SR)


def _remove_stale(out: Path, old_lock: dict, new_lock: dict) -> int:
    keep = {t["file"] for s in new_lock["roles"].values() if s for t in s["takes"]}
    removed = 0
    for (_, name) in _old_takes(old_lock):
        if name and name not in keep and os.path.basename(name) == name and (out / name).is_file():
            (out / name).unlink()
            removed += 1
    return removed


def _write_lock(out: Path, lock: dict) -> Path:
    from hark.forge.bank._build import clean

    tmp = out / f"{LOCK_NAME}.tmp"
    tmp.write_text(json.dumps(clean(lock), indent=1) + "\n", encoding="utf-8")
    os.replace(tmp, out / LOCK_NAME)
    return out / LOCK_NAME


def _render_summary(lock: dict, out: Path, stats: dict) -> str:
    pairs = sorted({f"{r}:{c}" for r, s in lock["roles"].items() if s for t in s["takes"] for c in t["checks"]})
    stale = f", {stats['removed']} stale removed" if stats["removed"] else ""
    tail = "checks clean" if not pairs else "warnings: " + ", ".join(pairs)
    return (f"kit {lock.get('name')}: {stats['takes']} takes in {out} ({stats['rendered']} rendered, "
            f"{stats['cached']} cached{stale}) in {stats['seconds']} s; {tail}")


def _log_take(log: Callable | None, take: dict, rec: dict) -> None:
    if log is None:
        return
    warn = f" WARN {','.join(rec['checks'])}" if rec["checks"] else ""
    log(f"{rec['file']}: {take['sound']} seed {take['seed']}, {rec['dur_s']:.3f} s{warn}")


def render(kit: Any, out: str | os.PathLike, force: bool = False, log: Callable | None = None) -> dict:
    """Render every take into ``out`` and write ``out/kit.lock.json`` (part 3, 5.4)."""
    t0 = time.monotonic()
    doc = load(kit)
    sr = int(doc.get("sr", SR))
    folder = Path(out)
    folder.mkdir(parents=True, exist_ok=True)
    old_lock = _old_lock(folder)
    old = _old_takes(old_lock)
    lock = _new_lock(doc, sr)
    stats = {"takes": 0, "rendered": 0, "cached": 0, "removed": 0}
    for take in plan(doc):
        spec = doc["roles"][take["role"]]
        key, path = take_key(take, sr), folder / take["file"]
        rec = _cached(old, take, key, path, force)
        if rec is not None:
            if "bed_db" in spec and "l50_lufs" not in rec:
                _add_l50(rec, path, sr)
            stats["cached"] += 1
        else:
            rec = _record(take, key, path, sr, spec)
            stats["rendered"] += 1
            _log_take(log, take, rec)
        lock["roles"][take["role"]]["takes"].append(rec)
        stats["takes"] += 1
    stats["removed"] = _remove_stale(folder, old_lock, lock)
    lock_path = _write_lock(folder, lock)
    stats["seconds"] = round(time.monotonic() - t0, 1)
    return {**lock, "stats": stats, "summary": _render_summary(lock, folder, stats),
            "file": str(lock_path).replace(os.sep, "/")}


# ---------------------------------------------------------------- check


def _role_notes(role: str, spec: dict | None) -> list[str]:
    if spec is None:
        return []
    notes = []
    word = role.split(".", 1)[0]
    if word in RESTRAINED:
        notes.append(f"role {role} is on: calm, crisp and minimal keep riser, swell and suck off, because a reveal "
                     f"lands better on one sound than on a build plus a hit; launch sounds one riser or swell "
                     f"where a section changes")
    notes += _unused_note(role, spec.get("sound", ""))
    notes += _ring_note(role, spec)
    if word not in {r.split(".", 1)[0] for r in ROLES}:
        notes.append(f"role {role} is not a shared role: a renderer must know it")
    return notes


def _unused_note(role: str, sound: str) -> list[str]:
    reason = UNUSED_SOUNDS.get(sound) or UNUSED_CATEGORIES.get(sound.split("/", 1)[0])
    return [f"role {role}: the presets never use {sound}: {reason}"] if reason else []


def _ring_note(role: str, spec: dict) -> list[str]:
    if spec.get("align", default_align(role)) != "end" or spec.get("sound") not in bank.ids():
        return []
    meta = bank.get(spec["sound"]).get("meta") or {}
    ring = (meta.get("end_s") or 0) - (meta.get("peak_s") or 0)
    if ring <= END_RING_S:
        return []
    return [f"role {role}: {spec['sound']} rings on {ring:.2f} s after its peak, so its climax comes that much "
            f"before the cue; with align peak the climax is on the cue"]


def _lock_index(out: str | os.PathLike | None) -> dict[str, dict]:
    if out is None:
        return {}
    lock = _old_lock(Path(out))
    return {k: t for (k, _), t in _old_takes(lock).items()}


def _audit_row(take: dict, index: dict, out: str | os.PathLike | None, sr: int) -> dict:
    row = {"file": take["file"], "sound": take["sound"], "seed": take["seed"], "pitch": take.get("pitch")}
    try:
        row.update(_audit_take(take, index, out, sr))
    except (ValueError, OSError) as exc:
        row.update(ok=False, warnings=[{"code": "render", "msg": f"{type(exc).__name__}: {exc}"}], notes=[])
    return row


def _audit_take(take: dict, index: dict, out: str | os.PathLike | None, sr: int) -> dict:
    rec = index.get(take_key(take, sr))
    path = Path(out) / take["file"] if out is not None else None
    if rec is not None and path is not None and path.is_file():
        entry = bank.get(take["sound"])
        audit = sfx.check(str(path), category=entry.get("category"), role=take["role"], sound=take["sound"])
        if "pitch_off" in (rec.get("checks") or []):
            bank.add_warning(audit, {"code": "pitch_off", "msg": f"pitch {take.get('pitch')} missed when rendered"})
        return {"source": "rendered file", "ok": audit["ok"], "warnings": audit["warnings"], "notes": audit["notes"]}
    _, audit, _, used = render_take(take)
    row = {"source": "rendered in memory", "ok": audit["ok"], "warnings": audit["warnings"], "notes": audit["notes"]}
    if used != take["seed"]:
        row.update(seed=used, planned_seed=take["seed"])
    return row


def _check_summary(doc: dict, rows: list[dict], notes: list[str]) -> str:
    clean = sum(1 for r in rows if r["ok"])
    out = f"kit {doc.get('name')}: {clean} / {len(rows)} takes clean"
    bad = [f"{r['file']}: {','.join(w['code'] for w in r['warnings'])}" for r in rows if not r["ok"]]
    if bad:
        out += f", {len(bad)} with warnings: " + "; ".join(bad)
    off = [r for r, s in doc["roles"].items() if s is None]
    if off:
        out += f"; off: {', '.join(off)}"
    if notes:
        out += f"; {len(notes)} note{'' if len(notes) == 1 else 's'}"
    return out


def check(kit: Any, out: str | os.PathLike | None = None, log: Callable | None = None) -> dict:
    """Validate a kit and audit every take: ``{ok, summary, problems, roles, notes}``."""
    try:
        doc = _read(kit)
    except KitError as exc:
        return {"ok": False, "summary": f"kit unreadable: {exc}", "problems": [str(exc)], "roles": {}, "notes": []}
    found = problems(doc)
    if found:
        n = len(found)
        return {"ok": False, "summary": f"{n} problem{'' if n == 1 else 's'}: " + "; ".join(found),
                "problems": found, "roles": {}, "notes": []}
    notes = [n for r, s in doc["roles"].items() for n in _role_notes(r, s)]
    index, sr = _lock_index(out), int(doc.get("sr", SR))
    roles: dict[str, list[dict]] = {}
    for take in plan(doc):
        row = _audit_row(take, index, out, sr)
        roles.setdefault(take["role"], []).append(row)
        if log is not None:
            log(f"{row['file']}: {row['sound']} " + ("ok" if row["ok"] else "; ".join(w["msg"] for w in row["warnings"])))
    rows = [r for rs in roles.values() for r in rs]
    return {"ok": all(r["ok"] for r in rows), "summary": _check_summary(doc, rows, notes), "problems": [],
            "roles": roles, "notes": notes}


# ---------------------------------------------------------------- audition


def audition_dir(doc: dict) -> Path:
    """The default takes folder of an audition under ``HARK_HOME``."""
    from hark.core import config

    return Path(config.HOME) / "kits" / f"{doc.get('name') or 'kit'}_{digest(doc)[:10]}"


def _stereo(x: np.ndarray) -> np.ndarray:
    x = np.asarray(x, dtype=np.float32)
    if x.ndim == 1:
        return np.column_stack([x, x])
    if x.shape[1] == 1:
        return np.column_stack([x[:, 0], x[:, 0]])
    return x[:, :2]


def _audition_parts(lock: dict, folder: Path, gap_s: float, role_gap_s: float) -> tuple[list[np.ndarray], list[dict]]:
    import soundfile

    sr = int(lock["sr"])
    parts, index, t = [], [], 0.0
    for role, spec in lock["roles"].items():
        if not spec:
            continue
        for take in spec["takes"]:
            x, _ = soundfile.read(str(folder / take["file"]), dtype="float32", always_2d=True)
            x = _stereo(x) * 10 ** (gain_db(take, spec, AUDITION_BED) / 20)
            index.append({"t": round(t, 3), "role": role, "file": take["file"], "sound": spec["sound"],
                          "pitch": take.get("pitch")})
            parts += [x, np.zeros((int(round(gap_s * sr)), 2), dtype=np.float32)]
            t += (len(x) + int(round(gap_s * sr))) / sr
        parts.append(np.zeros((int(round(role_gap_s * sr)), 2), dtype=np.float32))
        t += int(round(role_gap_s * sr)) / sr
    return parts, index


def _index_text(index: list[dict]) -> str:
    return "".join(f"{e['t']:8.3f}  {e['role']:<14}{e['sound']:<26}{e['file']}\n" for e in index)


def audition(kit: Any, out_wav: str, takes: str | None = None, gap_s: float = 0.3,
             role_gap_s: float = 0.8) -> dict:
    """Every take of a kit, role by role, at its role level, in one WAV plus an index."""
    doc = load(kit)
    if not any(s is not None for s in doc["roles"].values()):
        raise KitError("the kit sounds no role")
    folder = Path(takes) if takes else audition_dir(doc)
    lock = render(doc, folder)
    parts, index = _audition_parts(lock, folder, gap_s, role_gap_s)
    mix = np.concatenate(parts, axis=0)
    peak = float(np.max(np.abs(mix))) if mix.size else 0.0
    if peak > 10 ** (AUDITION_CEILING / 20):
        mix = mix * (10 ** (AUDITION_CEILING / 20) / peak)
    file = sfx.write(out_wav, mix, int(lock["sr"]))
    txt = str(Path(out_wav).with_suffix(".txt"))
    Path(txt).write_text(_index_text(index), encoding="utf-8")
    roles = len({e["role"] for e in index})
    summary = (f"wrote {file}: {len(index)} takes of {roles} roles, {len(mix) / lock['sr']:.1f} s "
               f"(index in {Path(txt).name})")
    return {"summary": summary, "file": file, "index": txt.replace(os.sep, "/"),
            "takes_dir": str(folder).replace(os.sep, "/")}


# ---------------------------------------------------------------- presets and init


def presets() -> list[str]:
    """The preset names: the sorted stems of the ``.json`` files in the presets folder."""
    return sorted(p.stem for p in PRESETS_DIR.glob("*.json"))


def preset(name: str) -> dict:
    """One preset document; ``KitError`` that lists the presets for an unknown name."""
    if name not in presets():
        raise KitError(f"unknown preset {name!r}: use one of {', '.join(presets())}")
    return json.loads((PRESETS_DIR / f"{name}.json").read_text(encoding="utf-8"))


def init(preset: str = "calm", out: str = "audio/kit.json", force: bool = False) -> dict:  # noqa: A002
    """Write a preset kit to ``out`` (part 3, 5.8)."""
    doc = globals()["preset"](preset)
    path = Path(out)
    if path.exists() and not force:
        raise KitError(f"{out} exists: use --force to overwrite it")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(doc, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")
    on = [r for r, s in doc["roles"].items() if s is not None]
    off = [r for r, s in doc["roles"].items() if s is None]
    summary = f"wrote {out}: preset {preset}, {len(on)} roles sounded, {len(off)} off ({', '.join(off)})"
    return {"summary": summary, "file": str(out).replace(os.sep, "/"), "kit": doc}


# ---------------------------------------------------------------- mix helpers (section 6)


def _at48(y: np.ndarray, sr: int) -> np.ndarray:
    if sr == SR:
        return np.asarray(y, dtype=np.float64)
    from hark.core import audio

    return np.asarray(audio.resample(np.asarray(y, dtype=np.float32), sr, SR), dtype=np.float64)


def kpower(y: np.ndarray) -> np.ndarray:
    """K-weighted power summed over channels; a mono signal counts twice (+3.01 dB)."""
    x = np.asarray(y, dtype=np.float64)
    if x.ndim == 1 or x.shape[1] == 1:
        return 2 * k_power(x.reshape(len(x)))
    return k_power(x)


def loud_lufs(y: np.ndarray, sr: int = SR, win: int = LOUD_WIN) -> float:
    """The K-weighted level of the loudest 50 ms: a take's ``l50_lufs``."""
    mean = loudest_mean(kpower(_at48(y, sr)), win)
    return round(LUFS_OFFSET + 10 * math.log10(mean + 1e-24), 2)


def bed_level(music: Any, t: float, sr: int = SR) -> float | None:
    """The median 50 ms K-weighted level of the music in the 1 s before ``t``."""
    if isinstance(music, (str, os.PathLike)):
        from hark.core import audio

        x = np.asarray(audio.load(str(music), sr=SR), dtype=np.float64)
    else:
        x = _at48(music, sr)
    seg = x[int(round(max(0.0, t - BED_SPAN_S) * SR)): int(round(max(0.0, t) * SR))]
    blocks = len(seg) // LOUD_WIN
    if blocks == 0:
        raise KitError(f"no music before t = {t} s")
    p = kpower(seg[: blocks * LOUD_WIN])
    level = LUFS_OFFSET + 10 * math.log10(float(np.median(p.reshape(blocks, LOUD_WIN).mean(axis=1))) + 1e-24)
    return None if level < BED_FLOOR else round(level, 2)


def over_bed(bed_db: float) -> float:
    """How far a take alone sits over the bed so that take plus bed sit ``bed_db`` over it."""
    return 10 * math.log10(10 ** (bed_db / 10) - 1)


def gain_db(take: dict, role: dict, bed_lufs: float | None = None, ceiling_db: float = GAIN_CEILING) -> float:
    """The gain in dB of one take at one cue (part 3, 6.1)."""
    ok_bed = bed_lufs is not None and bed_lufs >= BED_FLOOR
    if ok_bed and role.get("bed_db") is not None and take.get("l50_lufs") is not None:
        g = bed_lufs + over_bed(role["bed_db"]) - take["l50_lufs"]
        if take.get("peak_db") is not None:
            g = min(g, ceiling_db - take["peak_db"])
        return round(g, 2)
    return float(role.get("level_db") or 0.0)


def place(take: dict, role: dict, cue_s: float) -> float:
    """The start time of a take file for a cue: cue + offset - the take's sync point."""
    align = role.get("align") or "onset"
    return round(cue_s + (role.get("offset_ms") or 0.0) / 1000 - take[f"{align}_s"], 4)


def space_run(times: list[float], min_gap_ms: float | None) -> list[float]:
    """Sorted cue times spaced at least ``min_gap_ms`` apart."""
    out: list[float] = []
    gap = (min_gap_ms or 0.0) / 1000
    for t in sorted(times):
        out.append(round(t if not out else max(t, out[-1] + gap), 4))
    return out


def corr(y: np.ndarray) -> float:
    """L/R correlation; 1 for mono."""
    x = np.asarray(y, dtype=np.float64)
    if x.ndim == 1 or x.shape[1] == 1:
        return 1.0
    left, right = x[:, 0], x[:, 1]
    return float((left * right).sum() / (math.sqrt((left * left).sum() * (right * right).sum()) + 1e-20))


def _allpass(x: np.ndarray, d: int, g: float) -> np.ndarray:
    from scipy.signal import lfilter

    b = np.zeros(d + 1)
    a = np.zeros(d + 1)
    b[0], b[d], a[0], a[d] = -g, 1.0, 1.0, -g
    return lfilter(b, a, x)


def decorrelated(mid: np.ndarray) -> np.ndarray:
    """A decorrelated copy of the mid: 3 all-pass filters, a 250 Hz high-pass, a 5 ms end fade."""
    from scipy.signal import butter, sosfilt

    d = mid
    for delay, g in zip(DECOR_DELAYS, DECOR_GAINS):
        d = _allpass(d, delay, g)
    d = sosfilt(butter(2, DECOR_HP_HZ, "highpass", fs=SR, output="sos"), d)
    k = min(len(d), DECOR_FADE)
    if k:
        d[-k:] *= np.cos(np.linspace(0, np.pi / 2, k)) ** 2
    return d


def _bisect(f: Callable[[float], float], lo: float, hi: float, target: float) -> float:
    for _ in range(WIDTH_STEPS):
        mid = (lo + hi) / 2
        if f(mid) > target:
            lo = mid
        else:
            hi = mid
    return (lo + hi) / 2


def _as_stereo64(y: np.ndarray) -> np.ndarray:
    x = np.asarray(y, dtype=np.float64)
    if x.ndim == 1:
        return np.column_stack([x, x])
    return np.column_stack([x[:, 0], x[:, 0]]) if x.shape[1] == 1 else x[:, :2].copy()


def _side_for(m: np.ndarray, s: np.ndarray, c0: float, target: float) -> np.ndarray:
    def with_side(side: np.ndarray) -> float:
        return corr(np.column_stack([m + side, m - side]))

    if c0 < target:
        g = _bisect(lambda v: with_side(v * s), 0.0, 1.0, target)
        return g * s
    d = decorrelated(m)
    if with_side(s + DECOR_MAX * d) > target:
        return s + DECOR_MAX * d
    return s + _bisect(lambda h: with_side(s + h * d), 0.0, DECOR_MAX, target) * d


def stereo_width(y: np.ndarray, target: float, tol: float = WIDTH_TOL) -> tuple[np.ndarray, dict]:
    """Set the L/R correlation of a take to ``target`` (part 3, 6.5): ``(take, info)``."""
    x = _as_stereo64(y)
    peak = float(np.max(np.abs(x))) if x.size else 0.0
    m, s = (x[:, 0] + x[:, 1]) / 2, (x[:, 0] - x[:, 1]) / 2
    c0, goal = corr(x), min(1.0, max(0.0, float(target)))
    if abs(c0 - goal) > tol / 4:
        side = _side_for(m, s, c0, goal)
        x = np.column_stack([m + side, m - side])
        now = float(np.max(np.abs(x)))
        x = x * (peak / now) if now > 0 else x
    measured = corr(x)
    info = {"target": round(goal, 3), "before": round(c0, 3), "measured": round(measured, 3),
            "ok": abs(measured - goal) <= tol}
    return x, info


# ---------------------------------------------------------------- CLI


def _emit(result: dict, args: argparse.Namespace, text: str) -> None:
    from hark.core import report

    view = argparse.Namespace(**{**vars(args), "out": getattr(args, "json_out", None)})
    report.emit(result, view, text=text)


def _printer(args: argparse.Namespace) -> Callable[[str], None] | None:
    return None if args.json else print


def _cmd_init(args: argparse.Namespace) -> int:
    result = init(args.preset, args.output, args.force)
    _emit(result, args, result["summary"])
    return 0


def _cmd_render(args: argparse.Namespace) -> int:
    result = render(args.kit, args.out, args.force, log=_printer(args))
    _emit(result, args, result["summary"])
    return 0


def _cmd_check(args: argparse.Namespace) -> int:
    result = check(args.kit, args.out, log=_printer(args))
    _emit(result, args, "\n".join([result["summary"]] + [f"note: {n}" for n in result["notes"]]))
    return 0 if result["ok"] else 1


def _cmd_audition(args: argparse.Namespace) -> int:
    result = audition(args.kit, args.output, takes=args.out)
    _emit(result, args, result["summary"])
    return 0


def _plain_flags(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--json", action="store_true", help="print the full result as JSON")
    parser.add_argument("--threads", type=int, default=0, help="CPU threads (0 = default)")


def add_cli(sub: argparse._SubParsersAction) -> None:
    """Register ``hark kit`` with ``init``, ``render``, ``check`` and ``audition``."""
    from hark.core import report
    from hark.forge.bank.sfx import _help_of

    p = sub.add_parser("kit", help="a film's sound kit: cached takes, a lock, an audit, an audition")
    p.set_defaults(func=_help_of(p))
    s = p.add_subparsers(dest="kit_command")
    ip = s.add_parser("init", help="write a preset kit")
    ip.add_argument("--preset", default="calm", choices=("calm", "crisp", "minimal", "launch"))
    ip.add_argument("-o", "--output", default="audio/kit.json")
    ip.add_argument("--force", action="store_true")
    report.add_common(ip)
    ip.set_defaults(func=_cmd_init)
    rp = s.add_parser("render", help="render the takes and the lock")
    rp.add_argument("kit")
    rp.add_argument("--out", required=True, help="the takes folder")
    rp.add_argument("--force", action="store_true")
    rp.set_defaults(func=_cmd_render)
    cp = s.add_parser("check", help="validate the kit and audit every take")
    cp.add_argument("kit")
    cp.add_argument("--out", help="the folder of rendered takes")
    cp.set_defaults(func=_cmd_check)
    ap = s.add_parser("audition", help="every take in one WAV with an index")
    ap.add_argument("kit")
    ap.add_argument("-o", "--output", required=True)
    ap.add_argument("--out", help="the takes folder (default: a cache folder under HARK_HOME)")
    ap.set_defaults(func=_cmd_audition)
    for leaf in (rp, cp, ap):
        _plain_flags(leaf)
