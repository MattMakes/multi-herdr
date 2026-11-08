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

class _Sets:
    def __init__(self, events: list[dict]):
        prom = np.array([e.get("prominence_db", 0.0) for e in events], dtype=float)
        strong_thr = min(6.0, float(np.percentile(prom, 40))) if prom.size else 6.0
        self.accent_thr = max(6.0, float(np.percentile(prom, 80))) if prom.size else 6.0
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


def _item(target: Any, sets: _Sets, tol1: float, levels, beats: np.ndarray | None) -> dict:
    t = float(target["t"] if isinstance(target, dict) else target)
    item: dict = {"t": round(t, 3)}
    if isinstance(target, dict):
        for k in ("kind", "id"):
            if target.get(k) is not None:
                item[k] = target[k]
    ev, status = answer(t, sets, tol1, levels)
    item["offset_ms"] = round((ev["t"] - t) * 1000, 1) if ev is not None else None
    item["status"] = status
    if ev is not None:
        item["event_t"] = ev["t"]
        item["event_prominence_db"] = ev.get("prominence_db")
        if ev.get("label"):
            item["event"] = ev["label"]
    acc = sets.accents.within(t, 0.1)
    item["accent_offset_ms"] = round((acc["t"] - t) * 1000, 1) if acc is not None else None
    if beats is not None and beats.size:
        b = beats[nearest(beats, t)]
        item["beat_offset_ms"] = round((t - b) * 1000, 1)
    return item


def _share(items: list[dict], ok) -> float:
    return round(sum(1 for i in items if ok(i)) / len(items), 3)


def _stats(items: list[dict], sets: _Sets, tol1: float, frame_s, beats) -> dict:
    n = len(items)
    st: dict = {"n": n}
    if n == 0:
        return st
    K = "within_1_frame" if frame_s else "within_40ms"
    st["hit_" + K] = _share(items, lambda i: i["status"] == "OK")
    st["hit_within_100ms"] = _share(items, lambda i: i["status"] in ("OK", "near"))
    offs = [abs(i["offset_ms"]) for i in items if i["offset_ms"] is not None]
    st["median_abs_offset_ms"] = round(float(np.median(offs)), 1) if offs else None
    st["answered_by_swell"] = sum(1 for i in items if i["status"] == "swell")
    st["answered_by_level_peak"] = sum(1 for i in items if i["status"] == "peak")
    st["no_sound"] = [i["t"] for i in items if i["status"] in ("no sound", "off")]
    st["frame_ms" if frame_s else "tolerance_ms"] = round(tol1 * 1000, 1)
    lo, hi = min(i["t"] for i in items) - 1, max(i["t"] for i in items) + 1
    if sets.all.times.size:
        st["chance_" + K] = chance(sets.all.times, lo, hi, tol1)
        st["chance_within_100ms"] = chance(sets.all.times, lo, hi, 0.1)
    if sets.accents.times.size:
        st["accent_min_db"] = round(sets.accent_thr, 1)
        hit = lambda lim: lambda i: i["accent_offset_ms"] is not None and abs(i["accent_offset_ms"]) <= lim  # noqa: E731
        st["accent_" + K] = _share(items, hit(tol1 * 1000 + 1e-6))
        st["accent_chance_" + K] = chance(sets.accents.times, lo, hi, tol1)
        st["accent_within_100ms"] = _share(items, hit(100 + 1e-6))
        st["accent_chance_within_100ms"] = chance(sets.accents.times, lo, hi, 0.1)
    if beats is not None and beats.size >= 2:
        st["on_beat"] = _share(items, lambda i: abs(i.get("beat_offset_ms", 1e9)) <= 50)
        st["on_beat_chance"] = round(min(1.0, 2 * 0.05 / float(np.median(np.diff(beats)))), 3)
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
