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


def _number(val: Any) -> bool:
    return isinstance(val, numbers.Real) and not isinstance(val, bool) and math.isfinite(float(val))


def _integer(val: Any) -> bool:
    return isinstance(val, numbers.Integral) and not isinstance(val, bool)


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


def _read(kdoc: Any) -> dict:
    if isinstance(kdoc, dict):
        return kdoc
    try:
        text = Path(kdoc).read_text(encoding="utf-8")
    except OSError as exc:
        raise KitError(f"cannot read {kdoc}: {exc}") from None
    try:
        return json.loads(text)
    except ValueError as exc:
        raise KitError(f"{kdoc} is not JSON: {exc}") from None


def problems(kit: Any) -> list[str]:
    """Every problem of a kit, ``[]`` when it is valid (part 3, section 4)."""
    if not isinstance(kit, dict) or kit.get("kit") != SCHEMA:
        return ["not a kit file"]
    acc: list[str] = []
    if kit.get("sr", SR) not in bank.RATES or isinstance(kit.get("sr", SR), bool):
        acc.append(f"sr {kit.get('sr')!r} is not one of {', '.join(map(str, bank.RATES))}")
    role_map = kit.get("roles")
    if not isinstance(role_map, dict) or not role_map:
        return acc + ["roles must be a non-empty object"]
    for rid, rspec in role_map.items():
        acc += _role_problems(rid, rspec)
    acc += _collisions(kit)
    acc += _mix_problems(kit.get("mix"), role_map)
    return [*dict.fromkeys(acc)]


def _role_problems(rid: str, rspec: Any) -> list[str]:
    import re

    acc = [] if re.match(ROLE_PATTERN, str(rid)) else [f"role {rid!r}: the name must be lowercase words joined by dots"]
    if rspec is None:
        return acc
    if not isinstance(rspec, dict):
        return acc + [f"role {rid}: must be an object or null"]
    for check in (_keys_problem, _sound_problem, _count_problems, _edits_shape, _shape_problems, _value_problems):
        acc += check(rid, rspec)
    return acc


def _keys_problem(rid: str, rspec: dict) -> list[str]:
    extra = sorted(set(rspec) - set(ROLE_KEYS))
    if not extra:
        return []
    return [f"role {rid}: unknown keys {', '.join(extra)} (known: {', '.join(ROLE_KEYS)})"]


def _sound_problem(rid: str, rspec: dict) -> list[str]:
    sound = rspec.get("sound")
    known = bank.ids()
    if isinstance(sound, str) and sound in known:
        return []
    close = difflib.get_close_matches(str(sound), known, n=3, cutoff=0.5)
    hint = f" (did you mean {', '.join(close)}?)" if close else ""
    return [f"role {rid}: sound {sound!r} is not a bank id{hint}"]


def _count_problems(rid: str, rspec: dict) -> list[str]:
    acc = []
    takes = rspec.get("takes", 1)
    if not _integer(takes) or not 1 <= takes <= TAKES_MAX:
        acc.append(f"role {rid}: takes must be an integer from 1 to {TAKES_MAX}")
    if "seed" in rspec and not _integer(rspec["seed"]):
        acc.append(f"role {rid}: seed must be an integer")
    return acc


def _edits_of(rspec: dict) -> dict:
    tweaks = rspec.get("edits", {})
    if not isinstance(tweaks, dict) or set(tweaks) - set(EDIT_KEYS):
        return {}
    return tweaks


def _pitches_of(rspec: dict) -> list | None:
    pv = rspec.get("pitches")
    return pv if isinstance(pv, list) and pv else None


def _edits_shape(rid: str, rspec: dict) -> list[str]:
    tweaks = rspec.get("edits", {})
    if not isinstance(tweaks, dict):
        return [f"role {rid}: edits must be an object"]
    extra = sorted(set(tweaks) - set(EDIT_KEYS))
    return [f"role {rid}: unknown edits {', '.join(extra)} (known: {', '.join(EDIT_KEYS)})"] if extra else []


def _shape_problems(rid: str, rspec: dict) -> list[str]:
    return _pitch_shape(rid, rspec) + _scalar_shape(rid, rspec) + _mix_shape(rid, rspec)


def _pitch_shape(rid: str, rspec: dict) -> list[str]:
    acc = []
    if "pitches" in rspec and _pitches_of(rspec) is None:
        acc.append(f"role {rid}: pitches must be a non-empty list")
    if _pitches_of(rspec) is not None and "pitch" in _edits_of(rspec):
        acc.append(f"role {rid}: give pitches or edits.pitch, not both")
    return acc


def _scalar_shape(rid: str, rspec: dict) -> list[str]:
    acc = []
    if rspec.get("align", default_align(rid)) not in ALIGNS:
        acc.append(f"role {rid}: align must be one of {', '.join(ALIGNS)}")
    if "level_db" in rspec and not _number(rspec["level_db"]):
        acc.append(f"role {rid}: level_db must be a finite number")
    if rspec.get("max_s") is not None and not (_number(rspec["max_s"]) and rspec["max_s"] >= MAX_S_MIN):
        acc.append(f"role {rid}: max_s must be a number of at least {MAX_S_MIN}")
    return acc


def _mix_shape(rid: str, rspec: dict) -> list[str]:
    return [f"role {rid}: {kname} must be a number from {lo:g} to {hi:g}"
            for kname, (lo, hi) in MIX_RANGES.items()
            if kname in rspec and not (_number(rspec[kname]) and lo <= rspec[kname] <= hi)]


def _value_problems(rid: str, rspec: dict) -> list[str]:
    tweaks = _edits_of(rspec)
    acc = _level_problem(rid, tweaks)
    timing = {key_: tweaks[key_] for key_ in ("dur", "apex") if key_ in tweaks}
    if any(not _number(val) for val in timing.values()):
        return acc + [f"role {rid}: dur and apex must be numbers"]
    if rspec.get("sound") not in bank.ids():
        return acc
    return acc + _knob_problems(rid, rspec, tweaks, timing)


def _knob_problems(rid: str, rspec: dict, tweaks: dict, timing: dict) -> list[str]:
    """Problems the knob engine finds in the timing and pitch edits of a known sound."""
    entry = bank.get(rspec["sound"])
    knobs = _knobs()
    acc = _knob_problem(rid, "", lambda: knobs.validate(entry, **timing)) if timing else []
    for pv in _pitches_of(rspec) or ([tweaks["pitch"]] if "pitch" in tweaks else []):
        acc += _knob_problem(rid, f" pitch {pv!r}", lambda pv=pv: knobs.validate(entry, pitch=pv))
    return acc


def _level_problem(rid: str, tweaks: dict) -> list[str]:
    try:
        bank.level(tweaks.get("peak_db", bank.DEFAULT_PEAK), tweaks.get("weight"))
    except bank.BankError as exc:
        return [f"role {rid}: {exc}"]
    return []


def _knob_problem(rid: str, what: str, call: Callable[[], Any]) -> list[str]:
    try:
        call()
    except ValueError as exc:
        return [f"role {rid}{what}: {exc}"]
    return []


def _labels(pitches: list) -> list[str] | None:
    try:
        return [_knobs().label(pv) for pv in pitches]
    except ValueError:
        return None


def _collisions(kdoc: dict) -> list[str]:
    seen: dict[str, tuple[str, str, Any]] = {}
    acc = []
    for rid, rspec in kdoc["roles"].items():
        if not isinstance(rspec, dict) or _labels(_pitches_of(rspec) or []) is None:
            continue
        for label, pitch in _file_names(rid, rspec):
            kname = label.lower()
            if kname in seen:
                acc.append(_clash(rid, label, pitch, seen[kname]))
            else:
                seen[kname] = (rid, label, pitch)
    return acc


def _clash(rid: str, label: str, pitch: Any, first: tuple[str, str, Any]) -> str:
    role0, name0, pitch0 = first
    if role0 == rid and pitch0 != pitch:
        return f"role {rid}: pitches {pitch0} and {pitch} are one pitch ({label}): list each pitch once"
    return f"take file {label} clashes with {name0} of role {role0} when case is ignored: give the role another name"


def _mix_problems(mix: Any, roles: dict) -> list[str]:
    if mix is None:
        return []
    if not isinstance(mix, dict):
        return ["mix must be an object"]
    acc = []
    if "duck_db" in mix and not _number(mix["duck_db"]):
        acc.append("mix.duck_db must be a finite number")
    stop = mix.get("stop_down")
    if stop is not None:
        acc += _stop_down_problems(stop, roles)
    return acc


def _pair_ok(val: Any) -> bool:
    return isinstance(val, list) and len(val) == 2 and all(_number(buf) for buf in val) and val[0] <= val[1]


def _stop_down_problems(stop: Any, role_map: dict) -> list[str]:
    if not isinstance(stop, dict):
        return ["mix.stop_down must be an object"]
    acc = []
    before = stop.get("before", [])
    if not isinstance(before, list) or any(rkey not in role_map for rkey in before):
        acc.append("mix.stop_down.before must list role names of this kit")
    for kname in ("depth_db", "length_s"):
        if kname in stop and not _pair_ok(stop[kname]):
            acc.append(f"mix.stop_down.{kname} must be a [lo, hi] pair of numbers with lo <= hi")
    return acc


# ---------------------------------------------------------------- plan and keys


def _file_names(rid: str, rspec: dict) -> list[tuple[str, Any]]:
    stem = rid.replace(".", "-")
    count = rspec.get("takes", 1) if _integer(rspec.get("takes", 1)) else 1
    pitches = _pitches_of(rspec)
    if pitches is None:
        pitch = _edits_of(rspec).get("pitch")
        return [(f"{stem}_{key_}.wav", pitch) for key_ in range(1, count + 1)]
    labels = _labels(pitches) or []
    names = []
    for pv, label in zip(pitches, labels):
        names += [(f"{stem}_{label}.wav" if count == 1 else f"{stem}_{label}_{key_}.wav", pv) for key_ in range(1, count + 1)]
    return names


def plan(kit: Any) -> list[dict]:
    """Every take a kit renders, in role order (part 3, 5.1)."""
    doc = _read(kit)
    clashes = _collisions(doc)
    if clashes:
        raise KitError("; ".join(clashes))
    takes = []
    for rid, rspec in doc["roles"].items():
        if rspec is not None:
            takes += _role_takes(rid, rspec)
    return takes


def _role_takes(rid: str, rspec: dict) -> list[dict]:
    entry = bank.get(rspec["sound"])
    base = int(rspec.get("seed", entry.get("seed", 0)))
    count = int(rspec.get("takes", 1))
    tweaks = {key_: val for key_, val in _edits_of(rspec).items() if key_ != "pitch"}
    acc = []
    for idx, (label, pitch) in enumerate(_file_names(rid, rspec)):
        key_ = idx % count + 1
        tk = {"role": rid, "file": label, "sound": rspec["sound"], "seed": base + key_ - 1, "pitch": pitch,
                "edits": dict(tweaks), "max_s": rspec.get("max_s"), "k": key_}
        if "width" in rspec:
            tk["width"] = float(rspec["width"])
        acc.append(tk)
    return acc


def take_key(take: dict, sr: int = SR) -> str:
    """The cache key of a planned take (part 3, 5.2)."""
    doc = {"sound": take["sound"], "entry": digest(bank.get(take["sound"])), "seed": take["seed"],
           "pitch": take.get("pitch"), "edits": take.get("edits") or {}, "max_s": take.get("max_s"), "sr": sr,
           "bank": bank.VERSION, "hark": _hark_version()}
    if "width" in take:
        doc["width"] = take["width"]
    return digest(doc)


# ---------------------------------------------------------------- one take


def _render_once(tk: dict, seed: int, entry: dict) -> tuple[np.ndarray, dict, dict]:
    tweaks = tk.get("edits") or {}
    wave, info = bank.render(tk["sound"], seed=seed, dur=tweaks.get("dur"), apex=tweaks.get("apex"),
                          pitch=tk.get("pitch"), peak_db=tweaks.get("peak_db", bank.DEFAULT_PEAK),
                          weight=tweaks.get("weight"), checks=False)
    wave = _shape_take(wave, info, tk)
    audit = sfx.check(wave, category=entry.get("category"), role=tk["role"], sound=tk["sound"])
    bank.add_warning(audit, _knobs().pitch_warning(info.get("edits") or {}))
    return wave, audit, info


def _shape_take(wave: np.ndarray, info: dict, tk: dict) -> np.ndarray:
    """Apply ``max_s`` and ``width``; the width info goes into ``meta``."""
    max_s = tk.get("max_s")
    if max_s is not None and len(wave) > round(max_s * SR):
        wave = np.asarray(_knobs().fit_length(wave, max_s)[0], dtype=np.float32)
    if "width" in tk:
        wave, info["width"] = stereo_width(wave, tk["width"])
        wave = wave.astype(np.float32)
    return wave


def render_take(take: dict) -> tuple[np.ndarray, dict, dict, int]:
    """One take at 48 kHz with re-rolls: ``(audio, audit, meta, seed used)``."""
    entry = bank.get(take["sound"])
    first = None
    for j in range(REROLL + 1):
        seed = take["seed"] + REROLL_STEP * j
        wave, audit, info = _render_once(take, seed, entry)
        if audit["ok"]:
            return wave, audit, info, seed
        first = first or (wave, audit, info, seed)
    return first


# ---------------------------------------------------------------- render and the lock


def _sha1_file(fpath: Path) -> str:
    return hashlib.sha1(fpath.read_bytes()).hexdigest()


def _old_lock(acc: Path) -> dict:
    try:
        lock = json.loads((acc / LOCK_NAME).read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}
    return lock if isinstance(lock, dict) else {}


def _old_takes(lock: dict) -> dict[tuple[str, str], dict]:
    listing = {}
    for rspec in (lock.get("roles") or {}).values():
        for tsec in (rspec or {}).get("takes") or []:
            listing[(tsec.get("key"), tsec.get("file"))] = tsec
    return listing


def _lock_role(rid: str, rspec: dict | None) -> dict | None:
    if rspec is None:
        return None
    acc = {"sound": rspec["sound"], "align": rspec.get("align", default_align(rid)),
           "level_db": float(rspec.get("level_db", 0.0)), "max_s": rspec.get("max_s")}
    acc.update({key_: float(rspec[key_]) for key_ in MIX_RANGES if key_ in rspec})
    acc["takes"] = []
    return acc


def _new_lock(doc: dict, rate: int) -> dict:
    lock = {"kit": SCHEMA, "hark": _hark_version(), "bank": bank.VERSION, "sr": rate, "name": doc.get("name"),
            "roles": {rkey: _lock_role(rkey, rs) for rkey, rs in doc["roles"].items()}}
    if "mix" in doc:
        lock["mix"] = doc["mix"]
    return lock


def _resampled(wave: np.ndarray, rate: int) -> np.ndarray:
    if rate == SR:
        return wave
    from hark.core import audio

    return np.asarray(audio.resample(wave, SR, rate), dtype=np.float32)


def _record(tk: dict, kname: str, fpath: Path, rate: int, role_spec: dict) -> dict:
    wave, audit, info, used = render_take(tk)
    mval = measure(wave)
    written = _resampled(wave, rate)
    sfx.write(str(fpath), written, rate)
    take_rec = {"file": tk["file"], "seed": used, "pitch": tk.get("pitch"), "onset_s": mval["onset_s"],
           "peak_s": mval["peak_s"], "end_s": mval["end_s"], "dur_s": round(len(written) / rate, 4),
           "peak_db": mval["peak_db"], "lufs_m_max": mval["lufs_m_max"], "sha1": _sha1_file(fpath), "key": kname,
           "checks": [wv["code"] for wv in audit["warnings"]]}
    if used != tk["seed"]:
        take_rec["planned_seed"] = tk["seed"]
    if "bed_db" in role_spec:
        take_rec["l50_lufs"] = loud_lufs(written, rate)
    if "width" in tk:
        take_rec["corr"] = info["width"]["measured"]
    return take_rec


def _cached(old: dict, tk: dict, kname: str, fpath: Path, force: bool) -> dict | None:
    take_rec = old.get((kname, tk["file"]))
    if force or take_rec is None or not fpath.is_file() or _sha1_file(fpath) != take_rec.get("sha1"):
        return None
    return dict(take_rec)


def _add_l50(take_rec: dict, fpath: Path, rate: int) -> None:
    from hark.core import audio

    take_rec["l50_lufs"] = loud_lufs(np.asarray(audio.load(str(fpath), sr=SR)), SR)


def _remove_stale(acc: Path, old_lock: dict, new_lock: dict) -> int:
    keep = {tsec["file"] for rs in new_lock["roles"].values() if rs for tsec in rs["takes"]}
    removed = 0
    for (_, label) in _old_takes(old_lock):
        if label and label not in keep and os.path.basename(label) == label and (acc / label).is_file():
            (acc / label).unlink()
            removed += 1
    return removed


def _write_lock(acc: Path, lock: dict) -> Path:
    from hark.forge.bank._build import clean

    tmp = acc / f"{LOCK_NAME}.tmp"
    tmp.write_text(json.dumps(clean(lock), indent=1) + "\n", encoding="utf-8")
    os.replace(tmp, acc / LOCK_NAME)
    return acc / LOCK_NAME


def _render_summary(lock: dict, acc: Path, stats: dict) -> str:
    pairs = sorted({f"{rkey}:{cnt}" for rkey, rs in lock["roles"].items() if rs for tsec in rs["takes"] for cnt in tsec["checks"]})
    stale = f", {stats['removed']} stale removed" if stats["removed"] else ""
    tail = "checks clean" if not pairs else "warnings: " + ", ".join(pairs)
    return (f"kit {lock.get('name')}: {stats['takes']} takes in {acc} ({stats['rendered']} rendered, "
            f"{stats['cached']} cached{stale}) in {stats['seconds']} s; {tail}")


def _log_take(logger: Callable | None, tk: dict, take_rec: dict) -> None:
    if logger is None:
        return
    warn = f" WARN {','.join(take_rec['checks'])}" if take_rec["checks"] else ""
    logger(f"{take_rec['file']}: {tk['sound']} seed {tk['seed']}, {take_rec['dur_s']:.3f} s{warn}")


def render(kit: Any, out: str | os.PathLike, force: bool = False, log: Callable | None = None) -> dict:
    """Render every take into ``out`` and write ``out/kit.lock.json`` (part 3, 5.4)."""
    t0 = time.monotonic()
    doc, rate, takes_dir = _open(kit, out)
    old_lock = _old_lock(takes_dir)
    lock = _new_lock(doc, rate)
    stats = {"takes": 0, "rendered": 0, "cached": 0, "removed": 0}
    old = _old_takes(old_lock)
    for tk in plan(doc):
        take_rec = _take_record(tk, doc["roles"][tk["role"]], old, takes_dir, rate, force, stats, log)
        lock["roles"][tk["role"]]["takes"].append(take_rec)
    stats["removed"] = _remove_stale(takes_dir, old_lock, lock)
    lock_path = _write_lock(takes_dir, lock)
    stats["seconds"] = round(time.monotonic() - t0, 1)
    return {**lock, "stats": stats, "summary": _render_summary(lock, takes_dir, stats),
            "file": str(lock_path).replace(os.sep, "/")}


def _open(kdoc: Any, acc: str | os.PathLike) -> tuple[dict, int, Path]:
    """Load the kit and create the takes folder: ``(doc, sr, folder)``."""
    doc = load(kdoc)
    takes_dir = Path(acc)
    takes_dir.mkdir(parents=True, exist_ok=True)
    return doc, int(doc.get("sr", SR)), takes_dir


def _take_record(tk: dict, rspec: dict, old: dict, takes_dir: Path, rate: int, force: bool, stats: dict,
                 logger: Callable | None) -> dict:
    """The lock record of one take: reused from the cache, or rendered now. Counts it in ``stats``."""
    kname, fpath = take_key(tk, rate), takes_dir / tk["file"]
    stats["takes"] += 1
    take_rec = _cached(old, tk, kname, fpath, force)
    if take_rec is None:
        take_rec = _record(tk, kname, fpath, rate, rspec)
        stats["rendered"] += 1
        _log_take(logger, tk, take_rec)
        return take_rec
    if "bed_db" in rspec and "l50_lufs" not in take_rec:
        _add_l50(take_rec, fpath, rate)
    stats["cached"] += 1
    return take_rec


# ---------------------------------------------------------------- check


def _role_notes(rid: str, rspec: dict | None) -> list[str]:
    if rspec is None:
        return []
    remarks = []
    word = rid.split(".", 1)[0]
    if word in RESTRAINED:
        remarks.append(f"role {rid} is on: calm, crisp and minimal keep riser, swell and suck off, because a reveal "
                     f"lands better on one sound than on a build plus a hit; launch sounds one riser or swell "
                     f"where a section changes")
    remarks += _unused_note(rid, rspec.get("sound", ""))
    remarks += _ring_note(rid, rspec)
    if word not in {rkey.split(".", 1)[0] for rkey in ROLES}:
        remarks.append(f"role {rid} is not a shared role: a renderer must know it")
    return remarks


def _unused_note(rid: str, sound: str) -> list[str]:
    reason = UNUSED_SOUNDS.get(sound) or UNUSED_CATEGORIES.get(sound.split("/", 1)[0])
    return [f"role {rid}: the presets never use {sound}: {reason}"] if reason else []


def _ring_note(rid: str, rspec: dict) -> list[str]:
    if rspec.get("align", default_align(rid)) != "end" or rspec.get("sound") not in bank.ids():
        return []
    info = bank.get(rspec["sound"]).get("meta") or {}
    ring = (info.get("end_s") or 0) - (info.get("peak_s") or 0)
    if ring <= END_RING_S:
        return []
    return [f"role {rid}: {rspec['sound']} rings on {ring:.2f} s after its peak, so its climax comes that much "
            f"before the cue; with align peak the climax is on the cue"]


def _lock_index(acc: str | os.PathLike | None) -> dict[str, dict]:
    if acc is None:
        return {}
    lock = _old_lock(Path(acc))
    return {key_: tsec for (key_, _), tsec in _old_takes(lock).items()}


def _audit_row(tk: dict, listing: dict, acc: str | os.PathLike | None, rate: int) -> dict:
    row = {"file": tk["file"], "sound": tk["sound"], "seed": tk["seed"], "pitch": tk.get("pitch")}
    try:
        row.update(_audit_take(tk, listing, acc, rate))
    except (ValueError, OSError) as exc:
        row.update(ok=False, warnings=[{"code": "render", "msg": f"{type(exc).__name__}: {exc}"}], notes=[])
    return row


def _audit_take(tk: dict, listing: dict, acc: str | os.PathLike | None, rate: int) -> dict:
    take_rec = listing.get(take_key(tk, rate))
    fpath = Path(acc) / tk["file"] if acc is not None else None
    if take_rec is not None and fpath is not None and fpath.is_file():
        entry = bank.get(tk["sound"])
        audit = sfx.check(str(fpath), category=entry.get("category"), role=tk["role"], sound=tk["sound"])
        if "pitch_off" in (take_rec.get("checks") or []):
            bank.add_warning(audit, {"code": "pitch_off", "msg": f"pitch {tk.get('pitch')} missed when rendered"})
        return {"source": "rendered file", "ok": audit["ok"], "warnings": audit["warnings"], "notes": audit["notes"]}
    _, audit, _, used = render_take(tk)
    row = {"source": "rendered in memory", "ok": audit["ok"], "warnings": audit["warnings"], "notes": audit["notes"]}
    if used != tk["seed"]:
        row.update(seed=used, planned_seed=tk["seed"])
    return row


def _check_summary(doc: dict, rows: list[dict], remarks: list[str]) -> str:
    clean = sum(1 for rkey in rows if rkey["ok"])
    acc = f"kit {doc.get('name')}: {clean} / {len(rows)} takes clean"
    return acc + _bad_part(rows) + _off_part(doc) + _notes_part(remarks)


def _bad_part(rows: list[dict]) -> str:
    bad = [f"{rkey['file']}: {','.join(wv['code'] for wv in rkey['warnings'])}" for rkey in rows if not rkey["ok"]]
    return f", {len(bad)} with warnings: " + "; ".join(bad) if bad else ""


def _off_part(doc: dict) -> str:
    off = [rkey for rkey, rs in doc["roles"].items() if rs is None]
    return f"; off: {', '.join(off)}" if off else ""


def _notes_part(remarks: list[str]) -> str:
    return f"; {len(remarks)} note{'' if len(remarks) == 1 else 's'}" if remarks else ""


def check(kit: Any, out: str | os.PathLike | None = None, log: Callable | None = None) -> dict:
    """Validate a kit and audit every take: ``{ok, summary, problems, roles, notes}``."""
    try:
        doc = _read(kit)
    except KitError as exc:
        return _failed(f"kit unreadable: {exc}", [str(exc)])
    found = problems(doc)
    if found:
        count = len(found)
        return _failed(f"{count} problem{'' if count == 1 else 's'}: " + "; ".join(found), found)
    remarks = [count for rkey, rs in doc["roles"].items() for count in _role_notes(rkey, rs)]
    role_map = _audit_roles(doc, out, log)
    rows = [rkey for rs in role_map.values() for rkey in rs]
    return {"ok": all(rkey["ok"] for rkey in rows), "summary": _check_summary(doc, rows, remarks), "problems": [],
            "roles": role_map, "notes": remarks}


def _failed(summary: str, found: list[str]) -> dict:
    return {"ok": False, "summary": summary, "problems": found, "roles": {}, "notes": []}


def _audit_roles(doc: dict, acc: str | os.PathLike | None, logger: Callable | None) -> dict[str, list[dict]]:
    """Audit every planned take; rows grouped by role."""
    listing, rate = _lock_index(acc), int(doc.get("sr", SR))
    role_map: dict[str, list[dict]] = {}
    for tk in plan(doc):
        row = _audit_row(tk, listing, acc, rate)
        role_map.setdefault(tk["role"], []).append(row)
        if logger is not None:
            logger(f"{row['file']}: {row['sound']} " + ("ok" if row["ok"] else "; ".join(wv["msg"] for wv in row["warnings"])))
    return role_map


# ---------------------------------------------------------------- audition


def audition_dir(doc: dict) -> Path:
    """The default takes folder of an audition under ``HARK_HOME``."""
    home = os.environ.get("HARK_HOME")
    if not home:
        from hark.core import config

        home = str(config.HOME)
    return Path(home).expanduser().resolve() / "kits" / f"{doc.get('name') or 'kit'}_{digest(doc)[:10]}"


def _stereo(buf: np.ndarray) -> np.ndarray:
    buf = np.asarray(buf, dtype=np.float32)
    if buf.ndim == 1:
        return np.column_stack([buf, buf])
    if buf.shape[1] == 1:
        return np.column_stack([buf[:, 0], buf[:, 0]])
    return buf[:, :2]


def _audition_parts(lock: dict, takes_dir: Path, gap_s: float, role_gap_s: float) -> tuple[list[np.ndarray], list[dict]]:
    import soundfile

    rate = int(lock["sr"])
    gap, role_gap = int(round(gap_s * rate)), int(round(role_gap_s * rate))
    parts: list[np.ndarray] = []
    listing: list[dict] = []
    for rid, rspec in lock["roles"].items():
        if not rspec:
            continue
        for tk in rspec["takes"]:
            buf, _ = soundfile.read(str(takes_dir / tk["file"]), dtype="float32", always_2d=True)
            buf = _stereo(buf) * 10 ** (gain_db(tk, rspec, AUDITION_BED) / 20)
            listing.append({"t": round(_frames(parts) / rate, 3), "role": rid, "file": tk["file"],
                          "sound": rspec["sound"], "pitch": tk.get("pitch")})
            parts += [buf, _silence(gap)]
        parts.append(_silence(role_gap))
    return parts, listing


def _silence(frames: int) -> np.ndarray:
    return np.zeros((frames, 2), dtype=np.float32)


def _frames(parts: list[np.ndarray]) -> int:
    return sum(len(buf) for buf in parts)


def _under_ceiling(mix: np.ndarray) -> np.ndarray:
    """Scale the mix down when its peak is over AUDITION_CEILING."""
    peak = float(np.max(np.abs(mix))) if mix.size else 0.0
    ceiling = 10 ** (AUDITION_CEILING / 20)
    return mix * (ceiling / peak) if peak > ceiling else mix


def _index_text(listing: list[dict]) -> str:
    return "".join(f"{ev['t']:8.3f}  {ev['role']:<14}{ev['sound']:<26}{ev['file']}\n" for ev in listing)


def audition(kit: Any, out_wav: str, takes: str | None = None, gap_s: float = 0.3,
             role_gap_s: float = 0.8) -> dict:
    """Every take of a kit, role by role, at its role level, in one WAV plus an index."""
    doc = load(kit)
    if not any(rs is not None for rs in doc["roles"].values()):
        raise KitError("the kit sounds no role")
    takes_dir = Path(takes) if takes else audition_dir(doc)
    lock = render(doc, takes_dir)
    parts, listing = _audition_parts(lock, takes_dir, gap_s, role_gap_s)
    mix = _under_ceiling(np.concatenate(parts, axis=0))
    return {**_write_audition(out_wav, mix, int(lock["sr"]), listing), "takes_dir": str(takes_dir).replace(os.sep, "/")}


def _write_audition(out_wav: str, mix: np.ndarray, rate: int, listing: list[dict]) -> dict:
    """Write the WAV and its ``.txt`` index: ``{summary, file, index}``."""
    file = sfx.write(out_wav, mix, rate)
    txt = Path(out_wav).with_suffix(".txt")
    txt.write_text(_index_text(listing), encoding="utf-8")
    role_map = len({ev["role"] for ev in listing})
    summary = f"wrote {file}: {len(listing)} takes of {role_map} roles, {len(mix) / rate:.1f} s (index in {txt.name})"
    return {"summary": summary, "file": file, "index": str(txt).replace(os.sep, "/")}


# ---------------------------------------------------------------- presets and init


def presets() -> list[str]:
    """The preset names: the sorted stems of the ``.json`` files in the presets folder."""
    return sorted(pv.stem for pv in PRESETS_DIR.glob("*.json"))


def preset(name: str) -> dict:
    """One preset document; ``KitError`` that lists the presets for an unknown name."""
    if name not in presets():
        raise KitError(f"unknown preset {name!r}: use one of {', '.join(presets())}")
    return json.loads((PRESETS_DIR / f"{name}.json").read_text(encoding="utf-8"))


def init(preset: str = "calm", out: str = "audio/kit.json", force: bool = False) -> dict:  # noqa: A002
    """Write a preset kit to ``out`` (part 3, 5.8)."""
    doc = globals()["preset"](preset)
    fpath = Path(out)
    if fpath.exists() and not force:
        raise KitError(f"{out} exists: use --force to overwrite it")
    fpath.parent.mkdir(parents=True, exist_ok=True)
    fpath.write_text(json.dumps(doc, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")
    on = [rkey for rkey, rs in doc["roles"].items() if rs is not None]
    off = [rkey for rkey, rs in doc["roles"].items() if rs is None]
    summary = f"wrote {out}: preset {preset}, {len(on)} roles sounded, {len(off)} off ({', '.join(off)})"
    return {"summary": summary, "file": str(out).replace(os.sep, "/"), "kit": doc}


# ---------------------------------------------------------------- mix helpers (section 6)


def _at48(wave: np.ndarray, rate: int) -> np.ndarray:
    if rate == SR:
        return np.asarray(wave, dtype=np.float64)
    from hark.core import audio

    return np.asarray(audio.resample(np.asarray(wave, dtype=np.float32), rate, SR), dtype=np.float64)


def kpower(y: np.ndarray) -> np.ndarray:
    """K-weighted power summed over channels; a mono signal counts twice (+3.01 dB)."""
    buf = np.asarray(y, dtype=np.float64)
    if buf.ndim == 1 or buf.shape[1] == 1:
        return 2 * k_power(buf.reshape(len(buf)))
    return k_power(buf)


def loud_lufs(y: np.ndarray, sr: int = SR, win: int = LOUD_WIN) -> float:
    """The K-weighted level of the loudest 50 ms: a take's ``l50_lufs``."""
    mean = loudest_mean(kpower(_at48(y, sr)), win)
    return round(LUFS_OFFSET + 10 * math.log10(mean + 1e-24), 2)


def bed_level(music: Any, t: float, sr: int = SR) -> float | None:
    """The median 50 ms K-weighted level of the music in the 1 s before ``t``."""
    if isinstance(music, (str, os.PathLike)):
        from hark.core import audio

        buf = np.asarray(audio.load(str(music), sr=SR), dtype=np.float64)
    else:
        buf = _at48(music, sr)
    seg = buf[int(round(max(0.0, t - BED_SPAN_S) * SR)): int(round(max(0.0, t) * SR))]
    blocks = len(seg) // LOUD_WIN
    if blocks == 0:
        raise KitError(f"no music before t = {t} s")
    pv = kpower(seg[: blocks * LOUD_WIN])
    level = LUFS_OFFSET + 10 * math.log10(float(np.median(pv.reshape(blocks, LOUD_WIN).mean(axis=1))) + 1e-24)
    return None if level < BED_FLOOR else round(level, 2)


def over_bed(bed_db: float) -> float:
    """How far a take alone sits over the bed so that take plus bed sit ``bed_db`` over it."""
    return 10 * math.log10(10 ** (bed_db / 10) - 1)


def gain_db(take: dict, role: dict, bed_lufs: float | None = None, ceiling_db: float = GAIN_CEILING) -> float:
    """The gain in dB of one take at one cue (part 3, 6.1)."""
    ok_bed = bed_lufs is not None and bed_lufs >= BED_FLOOR
    if ok_bed and role.get("bed_db") is not None and take.get("l50_lufs") is not None:
        gn = bed_lufs + over_bed(role["bed_db"]) - take["l50_lufs"]
        if take.get("peak_db") is not None:
            gn = min(gn, ceiling_db - take["peak_db"])
        return round(gn, 2)
    return float(role.get("level_db") or 0.0)


def place(take: dict, role: dict, cue_s: float) -> float:
    """The start time of a take file for a cue: cue + offset - the take's sync point."""
    align = role.get("align") or "onset"
    return round(cue_s + (role.get("offset_ms") or 0.0) / 1000 - take[f"{align}_s"], 4)


def space_run(times: list[float], min_gap_ms: float | None) -> list[float]:
    """Sorted cue times spaced at least ``min_gap_ms`` apart."""
    acc: list[float] = []
    gap = (min_gap_ms or 0.0) / 1000
    for tsec in sorted(times):
        acc.append(round(tsec if not acc else max(tsec, acc[-1] + gap), 4))
    return acc


def corr(y: np.ndarray) -> float:
    """L/R correlation; 1 for mono."""
    buf = np.asarray(y, dtype=np.float64)
    if buf.ndim == 1 or buf.shape[1] == 1:
        return 1.0
    left, right = buf[:, 0], buf[:, 1]
    return float((left * right).sum() / (math.sqrt((left * left).sum() * (right * right).sum()) + 1e-20))


def _allpass(buf: np.ndarray, dl: int, gn: float) -> np.ndarray:
    from scipy.signal import lfilter

    ff = np.zeros(dl + 1)
    fb = np.zeros(dl + 1)
    ff[0], ff[dl], fb[0], fb[dl] = -gn, 1.0, 1.0, -gn
    return lfilter(ff, fb, buf)


def decorrelated(mid: np.ndarray) -> np.ndarray:
    """A decorrelated copy of the mid: 3 all-pass filters, a 250 Hz high-pass, a 5 ms end fade."""
    from scipy.signal import butter, sosfilt

    dl = mid
    for delay, gn in zip(DECOR_DELAYS, DECOR_GAINS):
        dl = _allpass(dl, delay, gn)
    dl = sosfilt(butter(2, DECOR_HP_HZ, "highpass", fs=SR, output="sos"), dl)
    key_ = min(len(dl), DECOR_FADE)
    if key_:
        dl[-key_:] *= np.cos(np.linspace(0, np.pi / 2, key_)) ** 2
    return dl


def _bisect(f: Callable[[float], float], lo: float, hi: float, target: float) -> float:
    for _ in range(WIDTH_STEPS):
        mid = (lo + hi) / 2
        if f(mid) > target:
            lo = mid
        else:
            hi = mid
    return (lo + hi) / 2


def _as_stereo64(wave: np.ndarray) -> np.ndarray:
    buf = np.asarray(wave, dtype=np.float64)
    if buf.ndim == 1:
        return np.column_stack([buf, buf])
    return np.column_stack([buf[:, 0], buf[:, 0]]) if buf.shape[1] == 1 else buf[:, :2].copy()


def _side_for(mval: np.ndarray, rs: np.ndarray, c0: float, target: float) -> np.ndarray:
    def with_side(side: np.ndarray) -> float:
        return corr(np.column_stack([mval + side, mval - side]))

    if c0 < target:
        gn = _bisect(lambda val: with_side(val * rs), 0.0, 1.0, target)
        return gn * rs
    dl = decorrelated(mval)
    if with_side(rs + DECOR_MAX * dl) > target:
        return rs + DECOR_MAX * dl
    return rs + _bisect(lambda hh: with_side(rs + hh * dl), 0.0, DECOR_MAX, target) * dl


def _rebuild(mval: np.ndarray, side: np.ndarray, peak: float) -> np.ndarray:
    """L/R from mid and side, scaled back to the original ``peak``."""
    buf = np.column_stack([mval + side, mval - side])
    now = float(np.max(np.abs(buf)))
    return buf * (peak / now) if now > 0 else buf


def stereo_width(y: np.ndarray, target: float, tol: float = WIDTH_TOL) -> tuple[np.ndarray, dict]:
    """Set the L/R correlation of a take to ``target`` (part 3, 6.5): ``(take, info)``."""
    buf = _as_stereo64(y)
    peak = float(np.max(np.abs(buf))) if buf.size else 0.0
    mval, rs = (buf[:, 0] + buf[:, 1]) / 2, (buf[:, 0] - buf[:, 1]) / 2
    c0, goal = corr(buf), min(1.0, max(0.0, float(target)))
    if abs(c0 - goal) > tol / 4:
        buf = _rebuild(mval, _side_for(mval, rs, c0, goal), peak)
    measured = corr(buf)
    info = {"target": round(goal, 3), "before": round(c0, 3), "measured": round(measured, 3),
            "ok": abs(measured - goal) <= tol}
    return buf, info


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
    _emit(result, args, "\n".join([result["summary"]] + [f"note: {count}" for count in result["notes"]]))
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

    ap = sub.add_parser("kit", help="a film's sound kit: cached takes, a lock, an audit, an audition")
    ap.set_defaults(func=_help_of(ap))
    rs = ap.add_subparsers(dest="kit_command")
    ip = rs.add_parser("init", help="write a preset kit")
    ip.add_argument("--preset", default="calm", help="calm (default), crisp, minimal or launch")
    ip.add_argument("-o", "--output", default="audio/kit.json")
    ip.add_argument("--force", action="store_true")
    report.add_common(ip)
    ip.set_defaults(func=_cmd_init)
    for leaf in (_add_render(rs), _add_check(rs), _add_audition(rs)):
        _plain_flags(leaf)


def _add_render(rs: argparse._SubParsersAction) -> argparse.ArgumentParser:
    rp = rs.add_parser("render", help="render the takes and the lock")
    rp.add_argument("kit")
    rp.add_argument("--out", required=True, help="the takes folder")
    rp.add_argument("--force", action="store_true")
    rp.set_defaults(func=_cmd_render)
    return rp


def _add_check(rs: argparse._SubParsersAction) -> argparse.ArgumentParser:
    cp = rs.add_parser("check", help="validate the kit and audit every take")
    cp.add_argument("kit")
    cp.add_argument("--out", help="the folder of rendered takes")
    cp.set_defaults(func=_cmd_check)
    return cp


def _add_audition(rs: argparse._SubParsersAction) -> argparse.ArgumentParser:
    ap = rs.add_parser("audition", help="every take in one WAV with an index")
    ap.add_argument("kit")
    ap.add_argument("-o", "--output", required=True)
    ap.add_argument("--out", help="the takes folder (default: a cache folder under HARK_HOME)")
    ap.set_defaults(func=_cmd_audition)
    return ap
