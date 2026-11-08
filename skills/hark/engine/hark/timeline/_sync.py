"""Cue parsing, sync of sound to cuts and cues, chance and top accents (analysis 2.6, 2.7, 2.11)."""

from __future__ import annotations

import json
import os
from typing import Any

import numpy as np

from hark.timeline._dsp import TimelineError, nearest

TIME_KEYS = ("t", "time", "sec", "seconds")
FRAME_KEYS = ("f", "frame")


# ---------------------------------------------------------------- cues

def _parse_text(text: str) -> Any:
    s = text.strip()
    if s.startswith("[") or s.startswith("{"):
        return json.loads(s)
    try:
        return [float(p) for p in s.replace(";", ",").split(",") if p.strip()]
    except ValueError:
        raise TimelineError(
            "cannot read cues: give a cue JSON file, JSON text, or seconds like \"1.5,3.2\" "
            f"(got {s[:60]!r})") from None


def _load(cues: Any) -> Any:
    if isinstance(cues, (str, os.PathLike)):
        path = os.fspath(cues)
        if os.path.isfile(path):
            with open(path, encoding="utf-8") as fh:
                return json.load(fh)
        return _parse_text(path)
    return cues


def _cue_time(item: dict, fps: float | None) -> float:
    for k in TIME_KEYS:
        if k in item:
            return float(item[k])
    for k in FRAME_KEYS:
        if k in item:
            if not fps:
                raise TimelineError('a frame cue needs a frame rate: pass {"fps": 60, "events": [{"f": 120}]}')
            return float(item[k]) / float(fps)
    raise TimelineError(f"cue without a time: {json.dumps(item)[:80]} (give \"t\" in seconds, or \"f\" with \"fps\")")


def _cue(item: Any, fps: float | None) -> dict:
    if isinstance(item, (int, float)) and not isinstance(item, bool):
        return {"t": round(float(item), 4), "kind": "cue", "id": None}
    if not isinstance(item, dict):
        raise TimelineError(f"cue without a time: {json.dumps(item)[:80]} (give \"t\" in seconds, or \"f\" with \"fps\")")
    return {"t": round(_cue_time(item, fps), 4), "kind": item.get("kind") or "cue", "id": item.get("id")}


def read_cues(cues: Any) -> tuple[list[dict], float | None]:
    """Parse cues from a file, JSON text, a number list string, a dict or a list."""
    data = _load(cues)
    if data is None:
        return [], None
    fps = None
    if isinstance(data, dict):
        fps = data.get("fps")
        data = data.get("events", data.get("cues", []))
    items = [_cue(i, fps) for i in (data or [])]
    return sorted(items, key=lambda c: c["t"]), (float(fps) if fps else None)


def cue_key(cues: Any) -> str:
    """Stable key of a cue value for the result cache."""
    if cues is None:
        return "none"
    if isinstance(cues, (str, os.PathLike)) and os.path.isfile(os.fspath(cues)):
        from hark.core.audio import file_id

        return "file:" + file_id(os.fspath(cues))
    return "value:" + json.dumps(cues, sort_keys=True, default=str)


# ---------------------------------------------------------------- chance

def chance(times, lo: float, hi: float, tol: float) -> float:
    t = np.sort(np.asarray(list(times), dtype=float))
    if t.size == 0:
        return 0.0
    grid = np.arange(lo, hi, 0.005)
    if grid.size == 0:
        return 0.0
    k = np.clip(np.searchsorted(t, grid), 1, t.size - 1) if t.size > 1 else np.zeros(grid.size, dtype=int)
    d = np.abs(t[k] - grid)
    if t.size > 1:
        d = np.minimum(d, np.abs(t[k - 1] - grid))
    return round(float(np.mean(d <= tol + 1e-9)), 3)


# ---------------------------------------------------------------- sync

def _prominence_thresholds(prom: np.ndarray) -> tuple[float, float]:
    """(strong, accent) prominence thresholds in dB."""
    if not prom.size:
        return 6.0, 6.0
    return min(6.0, float(np.percentile(prom, 40))), max(6.0, float(np.percentile(prom, 80)))


class _Sets:
    def __init__(self, events: list[dict]):
        prom = np.array([e.get("prominence_db", 0.0) for e in events], dtype=float)
        strong_thr, self.accent_thr = _prominence_thresholds(prom)
        self.all = _Index(events)
        self.strong = _Index([e for e, p in zip(events, prom) if p >= strong_thr])
        self.swells = _Index([e for e in events if e.get("shape") == "swell"])
        self.accents = _Index([e for e, p in zip(events, prom) if p >= self.accent_thr])


class _Index:
    def __init__(self, events: list[dict]):
        self.events = sorted(events, key=lambda e: e["t"])
        self.times = np.array([e["t"] for e in self.events], dtype=float)

    def within(self, t: float, r: float) -> dict | None:
        k = nearest(self.times, t)
        if k is None or abs(self.times[k] - t) > r + 1e-9:
            return None
        return self.events[k]


def answer(t: float, sets: _Sets, tol1: float, levels: dict | None) -> tuple[dict | None, str]:
    rules = ((sets.all, tol1, "OK"), (sets.strong, 0.1, "near"), (sets.all, 0.1, "near"),
             (sets.swells, 0.3, "swell"))
    for index, r, status in rules:
        ev = index.within(t, r)
        if ev is not None:
            return ev, status
    if levels:
        from hark.timeline._events import level_peak

        pk = level_peak(levels, t)
        if pk is not None:
            return {"t": pk["t"], "prominence_db": pk["rise_db"], "shape": "peak", "band": pk["band"]}, "peak"
    ev = sets.strong.within(t, 0.25)
    return (ev, "off") if ev is not None else (None, "no sound")


def _ms(a: dict | None, t: float) -> float | None:
    """Offset of event a from time t in ms, or None when there is no event."""
    return round((a["t"] - t) * 1000, 1) if a is not None else None


def _target_fields(target: Any) -> dict:
    if not isinstance(target, dict):
        return {}
    return {k: target[k] for k in ("kind", "id") if target.get(k) is not None}


def _event_fields(ev: dict | None) -> dict:
    if ev is None:
        return {}
    out = {"event_t": ev["t"], "event_prominence_db": ev.get("prominence_db")}
    if ev.get("label"):
        out["event"] = ev["label"]
    return out


def _item(target: Any, sets: _Sets, tol1: float, levels, beats: np.ndarray | None) -> dict:
    t = float(target["t"] if isinstance(target, dict) else target)
    item: dict = {"t": round(t, 3), **_target_fields(target)}
    ev, status = answer(t, sets, tol1, levels)
    item.update(offset_ms=_ms(ev, t), status=status, **_event_fields(ev))
    item["accent_offset_ms"] = _ms(sets.accents.within(t, 0.1), t)
    if beats is not None and beats.size:
        item["beat_offset_ms"] = round((t - beats[nearest(beats, t)]) * 1000, 1)
    return item


def _share(items: list[dict], ok) -> float:
    return round(sum(1 for i in items if ok(i)) / len(items), 3)


def _count(items: list[dict], *status: str) -> int:
    return sum(1 for i in items if i["status"] in status)


def _hit_stats(items: list[dict], K: str) -> dict:
    offs = [abs(i["offset_ms"]) for i in items if i["offset_ms"] is not None]
    return {"hit_" + K: _share(items, lambda i: i["status"] == "OK"),
            "hit_within_100ms": _share(items, lambda i: i["status"] in ("OK", "near")),
            "median_abs_offset_ms": round(float(np.median(offs)), 1) if offs else None,
            "answered_by_swell": _count(items, "swell"), "answered_by_level_peak": _count(items, "peak"),
            "no_sound": [i["t"] for i in items if i["status"] in ("no sound", "off")]}


def _accent_hit(lim: float):
    return lambda i: i["accent_offset_ms"] is not None and abs(i["accent_offset_ms"]) <= lim


def _accent_stats(items: list[dict], sets: _Sets, tol1: float, K: str, span: tuple) -> dict:
    if not sets.accents.times.size:
        return {}
    lo, hi = span
    return {"accent_min_db": round(sets.accent_thr, 1),
            "accent_" + K: _share(items, _accent_hit(tol1 * 1000 + 1e-6)),
            "accent_chance_" + K: chance(sets.accents.times, lo, hi, tol1),
            "accent_within_100ms": _share(items, _accent_hit(100 + 1e-6)),
            "accent_chance_within_100ms": chance(sets.accents.times, lo, hi, 0.1)}


def _chance_stats(sets: _Sets, tol1: float, K: str, span: tuple) -> dict:
    if not sets.all.times.size:
        return {}
    lo, hi = span
    return {"chance_" + K: chance(sets.all.times, lo, hi, tol1),
            "chance_within_100ms": chance(sets.all.times, lo, hi, 0.1)}


def _beat_stats(items: list[dict], beats) -> dict:
    if beats is None or beats.size < 2:
        return {}
    return {"on_beat": _share(items, lambda i: abs(i.get("beat_offset_ms", 1e9)) <= 50),
            "on_beat_chance": round(min(1.0, 2 * 0.05 / float(np.median(np.diff(beats)))), 3)}


def _stats(items: list[dict], sets: _Sets, tol1: float, frame_s, beats) -> dict:
    st: dict = {"n": len(items)}
    if not items:
        return st
    K = "within_1_frame" if frame_s else "within_40ms"
    span = (min(i["t"] for i in items) - 1, max(i["t"] for i in items) + 1)
    st.update(_hit_stats(items, K))
    st["frame_ms" if frame_s else "tolerance_ms"] = round(tol1 * 1000, 1)
    st.update(_chance_stats(sets, tol1, K, span))
    st.update(_accent_stats(items, sets, tol1, K, span))
    st.update(_beat_stats(items, beats))
    return st


def sync(times, events, beats=None, frame_s=None, levels=None) -> dict:
    """Match each target time (number or cue object) to the event that answers it."""
    sets = _Sets(list(events))
    tol1 = float(frame_s) if frame_s else 0.04
    b = np.sort(np.asarray(beats, dtype=float)) if beats is not None and len(beats) else None
    items = [_item(t, sets, tol1, levels, b) for t in times]
    return {"items": items, "stats": _stats(items, sets, tol1, frame_s, b)}


def top_accents(events, n: int = 3) -> list[dict]:
    """The strongest events over a running bed, at least 0.5 s apart."""
    kept: list[dict] = []
    for e in sorted((e for e in events if e.get("bed_db", -80) > -60), key=lambda e: -e.get("prominence_db", 0)):
        if all(abs(e["t"] - k["t"]) >= 0.5 for k in kept):
            kept.append(e)
        if len(kept) >= n:
            break
    return kept
