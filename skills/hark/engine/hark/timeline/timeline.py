"""hark timeline: events, loudness, beat grid, sections, sync and a transcript of a soundtrack or film."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import time
from typing import Any

import numpy as np

from hark.timeline._beats import music_grid
from hark.timeline._dsp import SR, TimelineError, mono_of
from hark.timeline._events import detect_events, level_envs, level_peak
from hark.timeline._loudness import loudness
from hark.timeline._sections import novelty_bounds
from hark.timeline._sync import cue_key, read_cues, sync, top_accents

__all__ = ["timeline", "detect_events", "level_envs", "level_peak", "loudness", "music_grid", "read_cues", "sync",
           "top_accents", "novelty_bounds", "TimelineError", "is_picture", "add_cli"]

CODE_VERSION = 1
CONTEXT_S = 30.0
DETAILS = ("low", "normal", "high")


# ---------------------------------------------------------------- inputs

def is_picture(path: str, probe: dict) -> bool:
    """A real picture stream: 0 < fps <= 240 and not an audio extension (cover art is not a picture)."""
    from hark.core.audio import AUDIO_EXTS

    if not probe.get("has_video"):
        return False
    fps = (probe.get("video") or {}).get("fps")
    ext = os.path.splitext(path)[1].lower()
    return bool(fps) and 0 < fps <= 240 and ext not in AUDIO_EXTS


def _check_args(path, video, detail) -> None:
    if detail not in DETAILS:
        raise TimelineError(f"detail must be one of low, normal, high (got {detail!r})")
    missing = [p for p in (path, video) if p is not None and not os.path.exists(p)]
    if missing:
        raise FileNotFoundError(f"file not found: {missing[0]}")


class _Inputs:
    def __init__(self, path, video, cues, detail):
        from hark.core.audio import probe

        _check_args(path, video, detail)
        self.path, self.probe = path, probe(path)
        self.has_audio = bool(self.probe.get("codec"))
        self.video = self._picture(video)
        if not self.has_audio and self.video is None:
            raise TimelineError(f"{path} has no audio stream")
        self.cues, self.cue_fps = read_cues(cues)
        self.lossy = bool(self.probe.get("lossy"))

    def _picture(self, video):
        from hark.core.audio import probe

        if video is None:
            return self.path if is_picture(self.path, self.probe) else None
        if not is_picture(video, probe(video)):
            raise TimelineError(f"{video} has no picture stream")
        return video


# ---------------------------------------------------------------- result cache

def _home() -> str:
    home = os.environ.get("HARK_HOME")
    if home:
        return home
    from hark.core import config

    return str(config.HOME)


def _core_version() -> str:
    import hark
    from hark.core import audio

    return f"{getattr(hark, '__version__', '?')}|{getattr(audio, 'DECODE_VERSION', '?')}"


def cache_path(path, video, cues, start, end, detail) -> str:
    from hark.core.audio import file_id

    parts = [file_id(path), file_id(video) if video else "none", cue_key(cues), str(start), str(end), detail,
             f"timeline{CODE_VERSION}", _core_version()]
    key = hashlib.sha1("|".join(parts).encode()).hexdigest()[:24]
    return os.path.join(_home(), "timeline", key + ".json")


def _read_cache(p: str) -> dict | None:
    try:
        with open(p, encoding="utf-8") as fh:
            r = json.load(fh)
        return r if isinstance(r, dict) else None
    except (OSError, ValueError):
        return None


def _write_cache(p: str, r: dict) -> None:
    from hark.core.report import clean

    try:
        os.makedirs(os.path.dirname(p), exist_ok=True)
        tmp = f"{p}.{os.getpid()}.tmp"
        with open(tmp, "w", encoding="utf-8") as fh:
            json.dump(clean({k: v for k, v in r.items() if k != "cached"}), fh)
        os.replace(tmp, p)
    except OSError:
        pass


# ---------------------------------------------------------------- loading

class _Range:
    """The decoded context and the analysed window, in file time."""

    def __init__(self, inp: _Inputs, start, end):
        from hark.core import audio

        if not inp.has_audio:
            self._picture_only(inp, start, end)
            return
        if start is None and end is None:
            self.ctx = audio.load(inp.path)
            self.duration = len(self.ctx) / SR
            if self.duration < 0.1:
                raise TimelineError(f"{inp.path} is {self.duration:.3f} s long; timeline needs at least 0.1 s "
                                    f"(use describe or realness for one-shots)")
            self.t0, self.t1, self.c0 = 0.0, self.duration, 0.0
        else:
            self.duration = float(inp.probe.get("duration") or 0) or audio.duration(inp.path)
            self._window(start, end, inp.path)
            self.c0 = max(0.0, self.t0 - CONTEXT_S)
            self.ctx = audio.load(inp.path, start=self.c0, end=min(self.duration, self.t1 + CONTEXT_S))
        self._cut()

    def _picture_only(self, inp, start, end):
        self.duration = float(inp.probe.get("duration") or 0)
        self.ctx = np.zeros((int(round(self.duration * SR)), 2), dtype=np.float32)
        self.c0 = 0.0
        self.t0 = max(0.0, start or 0.0)
        self.t1 = min(self.duration, end) if end is not None else self.duration
        self._cut()

    def _window(self, start, end, path):
        self.t0 = max(0.0, float(start or 0.0))
        self.t1 = min(self.duration, float(end)) if end is not None else self.duration
        if self.t1 - self.t0 < 0.1:
            to = f"{end:g} s" if end is not None else "the end"
            raise TimelineError(f"nothing to analyse between {self.t0:g} s and {to}: {os.path.basename(path)} is "
                                f"{self.duration:.2f} s long")

    def _cut(self):
        a = int(round((self.t0 - self.c0) * SR))
        b = min(int(round((self.t1 - self.c0) * SR)), len(self.ctx))
        self.win = self.ctx[a:b]
        self.dur = len(self.win) / SR
        self.wa, self.wb = self.t0 - self.c0, self.t0 - self.c0 + self.dur


# ---------------------------------------------------------------- stages

class _Clock:
    def __init__(self):
        self.t, self.start, self.marks = time.perf_counter(), time.perf_counter(), {}

    def mark(self, name: str) -> None:
        now = time.perf_counter()
        self.marks[name] = round(now - self.t, 2)
        self.t = now

    def done(self) -> dict:
        self.marks["total_s"] = round(time.perf_counter() - self.start, 2)
        return self.marks


def _picture_cuts(inp: _Inputs, rg: _Range) -> tuple[list[float], float | None]:
    if inp.video is None:
        return [], None
    from hark.timeline import video as vid

    fps = vid.info(inp.video).get("fps")
    cuts = [c - rg.c0 for c in vid.cuts(inp.video) if rg.t0 + 0.05 < c < rg.t0 + rg.dur - 0.05]
    return cuts, fps


def _window_cues(inp: _Inputs, rg: _Range) -> list[dict]:
    lo, hi = rg.t0 - 1e-6, rg.t0 + rg.dur + 1e-6
    return [dict(c, t=c["t"] - rg.c0) for c in inp.cues if lo <= c["t"] <= hi]


def _section_pass(rg: _Range, silent: bool, detail: str, span_cache) -> tuple[list[dict], dict]:
    from hark.timeline._sections import sections
    from hark.timeline._tags import BUDGETS

    if silent:
        sec = [{"start": 0.0, "end": round(rg.dur, 2), "windows": 0, "label": "digital silence", "lufs": None,
                "digital_silence": True}] if rg.dur >= 1.0 else []
        return sec, {"windows": 0, "ast": 0}
    _, _, win, hop, max_win, ast_sec = BUDGETS[detail]
    hop = max(hop, round((rg.dur - win) / max(1, max_win - 1), 2))
    return sections(rg.win, rg.t0, win, hop, ast_sec, span_cache)


def _sync_blocks(cuts, fps, cues, inp, events, grid, envs) -> dict:
    beats = grid["beats"] if grid["confidence"] in ("clear", "weak") else None
    out = {}
    if cuts:
        out["cuts"] = sync(cuts, events, beats, 1 / fps if fps else None, envs)
    if cues:
        out["cues"] = sync(cues, events, beats, 1 / inp.cue_fps if inp.cue_fps else None, envs)
    return out


# ---------------------------------------------------------------- shifting to file time

def _shift_loudness(L: dict, t0: float) -> None:
    for g in L["silences"]:
        g["start"], g["end"] = round(g["start"] + t0, 2), round(g["end"] + t0, 2)
    if "max_momentary" in L:
        L["max_momentary"]["t"] = round(L["max_momentary"]["t"] + t0, 3)
    if "max_short_term" in L:
        st = L["max_short_term"]
        st["start"], st["end"] = round(st["start"] + t0, 3), round(st["end"] + t0, 3)
    for k in ("momentary", "short_term"):
        if k in L:
            L[k]["t0"] = round(L[k]["t0"] + t0, 3)
    L["clipping"]["times"] = [round(t + t0, 3) for t in L["clipping"]["times"]]


def _shift_sync(blocks: dict, c0: float) -> None:
    for b in blocks.values():
        for it in b["items"]:
            it["t"] = round(it["t"] + c0, 3)
            if it.get("event_t") is not None:
                it["event_t"] = round(it["event_t"] + c0, 3)
        if "no_sound" in b["stats"]:
            b["stats"]["no_sound"] = [round(t + c0, 3) for t in b["stats"]["no_sound"]]


def _shift_grid(grid: dict, c0: float, lo: float, hi: float) -> dict:
    g = dict(grid)
    for k in ("beats", "downbeats"):
        g[k] = [round(t + c0, 3) for t in grid.get(k, []) if lo - 1e-6 <= t + c0 <= hi + 1e-6]
    return g


def _window_events(events, rg: _Range, grid: dict) -> list[dict]:
    beats = np.asarray(grid["beats"], dtype=float) if grid["confidence"] in ("clear", "weak") else np.zeros(0)
    out = []
    for e in events:
        if rg.wa - 1e-6 <= e["t"] < rg.wb:
            ev = dict(e, t=round(e["t"] + rg.c0, 3))
            if beats.size:
                ev["beat_offset_ms"] = round((e["t"] - beats[np.argmin(np.abs(beats - e["t"]))]) * 1000, 1)
            out.append(ev)
    return out


# ---------------------------------------------------------------- the run

def _sound_pass(rg: _Range, clock: _Clock) -> dict:
    """Loudness, onset events, level envelopes and the beat grid of the context."""
    L = loudness(rg.win, at_start=rg.t0 <= 0.01, at_end=rg.t0 + rg.dur >= rg.duration - 0.01)
    clock.mark("loudness_s")
    events = [] if L["digital_silence"] else detect_events(rg.ctx, lead=rg.c0 == 0)
    clock.mark("events_s")
    envs, grid = level_envs(rg.ctx), music_grid(mono_of(rg.ctx))
    clock.mark("beats_s")
    return {"L": L, "silent": bool(L["digital_silence"]), "events": events, "envs": envs, "grid": grid}


def _tag_pass(rg: _Range, events: list[dict], near: list[float], detail: str, span_cache) -> dict:
    from hark.timeline._tags import tag_events

    win_ev = [e for e in events if rg.wa - 1e-6 <= e["t"] < rg.wb]
    return tag_events(win_ev, mono_of(rg.ctx), len(rg.ctx) / SR, detail, near, rg.wa, rg.wb, span_cache, rg.c0)


def _analyse(inp: _Inputs, rg: _Range, detail: str, reuse: bool, clock: _Clock) -> dict:
    from hark.core.audio import file_id
    from hark.timeline._tags import SpanCache

    snd = _sound_pass(rg, clock)
    cuts, fps = _picture_cuts(inp, rg)
    clock.mark("cuts_s")
    cues = _window_cues(inp, rg)
    span_cache = SpanCache(file_id(inp.path) if inp.has_audio else None, reuse)
    tags = _tag_pass(rg, snd["events"], cuts + [c["t"] for c in cues], detail, span_cache)
    clock.mark("tag_events_s")
    secs, sec_calls = _section_pass(rg, snd["silent"], detail, span_cache)
    clock.mark("sections_s")
    blocks = _sync_blocks(cuts, fps, cues, inp, snd["events"], snd["grid"], snd["envs"])
    return {"L": snd["L"], "events": _window_events(snd["events"], rg, snd["grid"]), "grid": snd["grid"],
            "cuts": cuts, "fps": fps, "blocks": blocks, "secs": secs, "tags": tags, "sec_calls": sec_calls}


def _model_calls(a: dict) -> dict:
    return {"clap_event_windows": a["tags"]["clap_windows"], "ast_event_windows": a["tags"]["ast"],
            "clap_section_windows": a["sec_calls"]["windows"], "ast_section_windows": a["sec_calls"]["ast"]}


def _span_fields(inp: _Inputs, rg: _Range) -> dict:
    return {"file": inp.path, "video": inp.video, "start": round(rg.t0, 3), "end": round(rg.t0 + rg.dur, 3),
            "duration": round(rg.dur, 3), "file_duration": round(rg.duration, 3),
            "context": [round(rg.c0, 3), round(rg.c0 + len(rg.ctx) / SR, 3)], "has_audio": inp.has_audio,
            "lossy": inp.lossy}


def _shift_all(a: dict, rg: _Range) -> None:
    """Move every time in the analysis from window time to file time."""
    _shift_loudness(a["L"], rg.t0)
    _shift_sync(a["blocks"], rg.c0)
    for s in a["secs"]:
        s["start"], s["end"] = round(s["start"] + rg.t0, 2), round(s["end"] + rg.t0, 2)


def _assemble(inp: _Inputs, rg: _Range, a: dict, detail: str, clock: _Clock) -> dict:
    from hark.core.report import clean
    from hark.timeline import _text

    _shift_all(a, rg)
    r: dict[str, Any] = {
        **_span_fields(inp, rg), "loudness": a["L"],
        "music": _shift_grid(a["grid"], rg.c0, rg.t0, rg.t0 + rg.dur), "events": a["events"],
        "sections": a["secs"], "sync": a["blocks"], "cuts": [round(c + rg.c0, 3) for c in a["cuts"]],
        "cut_method": "frame difference" if inp.video else None, "fps": a["fps"], "model_calls": _model_calls(a),
        "timing_s": clock.done(), "detail": detail,
    }
    r = clean(r)
    r["summary"] = _text.summary(r)
    r["text"] = _text.transcript(r)
    r["cached"] = False
    return r


def timeline(path, video=None, cues=None, start=None, end=None, detail="normal", cache=True, fresh=False) -> dict:
    """Listen through a soundtrack or film: events, loudness, beat grid, sections, sync and a transcript."""
    clock = _Clock()
    inp = _Inputs(path, video, cues, detail)
    cpath = cache_path(path, inp.video if video else None, cues, start, end, detail)
    if cache and not fresh and os.path.isfile(cpath):
        hit = _read_cache(cpath)
        if hit is not None:
            return dict(hit, cached=True, file=path)
    rg = _Range(inp, start, end)
    clock.mark("decode_s")
    r = _assemble(inp, rg, _analyse(inp, rg, detail, not fresh, clock), detail, clock)
    if cache or fresh:
        _write_cache(cpath, r)
    return r


# ---------------------------------------------------------------- CLI

def add_window_flags(p: argparse.ArgumentParser) -> None:
    p.add_argument("file", help="audio file or video")
    p.add_argument("--video", help="the picture for cuts and frames (default: FILE when it is a video)")
    p.add_argument("--cues", help='cue JSON file, JSON text, or seconds like "1.5,3.2"')
    p.add_argument("--from", "--start", dest="start", type=float, help="zoom start in seconds")
    p.add_argument("--to", "--end", dest="end", type=float, help="zoom end in seconds")
    p.add_argument("--detail", choices=DETAILS, default="normal", help="model budget (default normal)")


def _handler(args: argparse.Namespace) -> int:
    from hark.core.report import emit
    from hark.models import require_ml

    require_ml("timeline")
    r = timeline(args.file, video=args.video, cues=args.cues, start=args.start, end=args.end, detail=args.detail,
                 fresh=args.fresh)
    emit(r, args)
    return 0


def add_cli(sub) -> None:
    """Register `hark timeline`."""
    from hark.core.report import add_common

    p = sub.add_parser("timeline", help="events, loudness, beats, sections and sync of a soundtrack or film")
    add_window_flags(p)
    p.add_argument("--fresh", action="store_true", help="recompute everything and store the new result")
    add_common(p)
    p.set_defaults(func=_handler)
