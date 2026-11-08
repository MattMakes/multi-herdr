"""rank and find: which files fit a description best, and the best moments inside one recording.

``rank`` embeds every readable file and sorts them by the calibrated z of one query. ``find`` reads the
query as one-shot or continuous, builds candidate segments (onset strokes, grouped events, sliding
windows), scores each with CLAP and returns the best non-overlapping ones as sample trims.
"""

from __future__ import annotations

import argparse
import math
import os
from collections.abc import Sequence
from typing import Any

import numpy as np

from hark.core import audio, features, report
from hark.listen import vocabulary as voc

SR = 48000
NEAR_SILENT_DB = -70.0

# find: envelope and trims
FRAME_S = 0.005
FRAME = 240
ATTACK_FRAME = 48
PRE_ROLL = 0.005
MARGIN_S = 0.1
EVENT_MAX_S = 2.0
JOIN_GAP_S = 0.25
DECAY_DB = 35.0
STROKE_FLOOR_DB = 4.0
END_PAD_S = 0.02
NEXT_GUARD_S = 0.002
MIN_STROKE_S = 0.02
PEAK_FRAMES = 13
ATTACK_REACH_S = 0.03
WEAK_LEAD_DB = 15.0
ISOLATION_BONUS = 10.0
CAP = 48
BUCKET_S = 2.0
WINDOW_TARGET = 40
ONE_STAGE_S = 60.0
BEST_WINDOWS = 6
REFINE_Z = 0.5
INSIDE_TOL_S = 0.05
CUT_DB = -15.0
CUT_PENALTY = 0.5
GAP_DB = -100.0
SNAP_FRAMES = 200
SLOPE_FRAMES = 6
SLOPE_DB = 1.0
CLIMB_DB = 6.0
MIN_SNAP_S = 0.1
DEDUPE = 0.9
QUIET_RUN = 50
KIND_EVENT = -0.08
KIND_TEXTURE = -0.27
KINDS = ("auto", "event", "texture")
EVENT_ANCHORS = ("a single short sound effect", "one short hit", "a click", "a single impact", "one short pop",
                 "a quick one-shot sound")
TEXTURE_ANCHORS = ("a continuous sound", "a long background atmosphere", "a steady drone", "a piece of music",
                   "ambient room noise", "a soundscape")


def query_text(query: Any) -> str:
    """The trimmed query; a value error when it is empty."""
    if not isinstance(query, str) or not query.strip():
        raise ValueError("no query given")
    return query.strip()


# ================================================================ rank


def _rank_rows(v: voc.Vocab, paths: list[str], query: str) -> list[dict]:
    A = voc.embed_files(paths)
    cal = v.calibrate(A, v.embed_text([query]))
    rows = []
    for i, p in enumerate(paths):
        z = round(float(cal.z_q[i, 0]), 2)
        silent = voc.peak_db(p, mono=True) < NEAR_SILENT_DB
        term, _, tz, _ = v.ranked_terms(cal.Z[i], cal.C[i], 1, exclude=voc.NOT_SOUNDS_LIKE)[0]
        rows.append({"path": p, "name": os.path.basename(p), "z": z, "cosine": round(float(cal.Cq[i, 0]), 3),
                     "vocab_percentile": round(100 * float(cal.percentile[i, 0]), 1),
                     "verdict": voc.NO_MATCH if silent else voc.verdict(z), "near_silent": silent,
                     "sounds_most_like": f"{term} (z {voc.zf(tz)})"})
    return sorted(rows, key=lambda r: (r["near_silent"], -r["z"]))


def _band_counts(rows: Sequence[dict]) -> dict[str, int]:
    live = [r for r in rows if not r["near_silent"]]
    return {b: sum(r["verdict"] == b for r in live) for b in voc.VERDICT_THRESHOLDS_Z}


def _best_text(listed: list[dict]) -> str:
    live = [r for r in listed if not r["near_silent"]][:3]
    return ", ".join(f"{r['name']} (z {voc.zf(r['z'])}, {r['verdict']})" for r in live) or "none (all near-silent)"


def _weak_best_text(listed: list[dict]) -> str | None:
    first = listed[0] if listed else None
    if not first or first["near_silent"] or first["verdict"] not in ("weak", voc.NO_MATCH):
        return None
    how = "only a weak match" if first["verdict"] == "weak" else "not a match"
    return f"Even the best is {how}; it sounds more like {first['sounds_most_like']}."


def _strong_text(n_strong: int) -> str | None:
    if n_strong <= 1:
        return None
    ev = voc.VERDICT_EVIDENCE["strong"]
    return (f"Held-out precision of strong: the right kind of sound {round(ev['precision_library'] * 100)}%"
            f", the exact variant {round(ev['precision_library_strict'] * 100)}% (a close variant, such as a "
            "return key for a space bar, counts as wrong in the strict number).")


def _silent_text(rows: list[dict]) -> str | None:
    n = sum(r["near_silent"] for r in rows)
    return f"{n} near-silent file(s) ranked last; their scores mean nothing." if n else None


def _rank_summary(query: str, rows: list[dict], listed: list[dict], skipped: list[dict]) -> str:
    counts = _band_counts(rows)
    parts = [f"Ranked {len(rows)} files for '{query}'. Best: {_best_text(listed)}.",
             f"Bands: {counts['strong']} strong, {counts['plausible']} plausible, {counts['weak']} weak.",
             _weak_best_text(listed), _strong_text(counts["strong"]), _silent_text(rows)]
    return " ".join(p for p in parts if p) + _skipped_note(skipped)


def _skipped_note(skipped: Sequence[dict]) -> str:
    if not skipped:
        return ""
    names = ", ".join(os.path.basename(s["path"]) for s in skipped[:3]) + (", ..." if len(skipped) > 3 else "")
    return f" Skipped {len(skipped)} unreadable file(s): {names}."


def rank(paths: str | Sequence[str], query: str, top: int | None = None) -> dict:
    """Rank files by how well they fit one description."""
    query = query_text(query)
    paths = [paths] if isinstance(paths, str) else list(paths)
    audio.check_paths(paths)
    ok, skipped = voc.split_readable(audio.list_media(paths))
    if not ok:
        return {"summary": "No readable media files found." + _skipped_note(skipped), "query": query, "n_files": 0,
                "results": [], "skipped": skipped}
    rows = _rank_rows(voc.get(), ok, query)
    listed = rows[:top] if top else rows
    return {"summary": _rank_summary(query, rows, listed, skipped), "query": query, "n_files": len(ok),
            "results": listed, "skipped": skipped}


def display_names(rows: Sequence[dict]) -> list[str]:
    """Base names, or '<parent>/<name>' when 2 results share a base name."""
    names = [r["name"] for r in rows]
    return [f"{os.path.basename(os.path.dirname(r['path']))}/{n}" if names.count(n) > 1 else n
            for r, n in zip(rows, names)]


def rank_text(result: dict) -> str:
    """Text output of rank."""
    lines = [result["summary"]]
    if result["results"]:
        lines.append("")
    for i, (r, name) in enumerate(zip(result["results"], display_names(result["results"])), 1):
        flag = "  NEAR-SILENT" if r["near_silent"] else ""
        lines.append(f"{i:>3}. z {voc.zf(r['z'])}  {r['verdict']:<9}  cos {r['cosine']:.3f}  {name}  "
                     f"(sounds most like {r['sounds_most_like']}){flag}")
    return "\n".join(lines)


# ================================================================ find: envelope helpers


def envelope(m: np.ndarray, frame: int = FRAME) -> np.ndarray:
    """Frame levels in dB of non-overlapping frames."""
    m = np.asarray(m, np.float64)
    if len(m) < frame:
        return np.zeros(0)
    return 20 * np.log10(np.maximum(features.frame_rms(m, frame), 1e-12))


def noise_floor(env: np.ndarray) -> float:
    """The 10th percentile of the frame levels."""
    return float(np.percentile(env, 10)) if len(env) else -120.0


def seg_frames(env: np.ndarray, a: float, b: float) -> np.ndarray:
    """The frames of [a, b) (F8 rule)."""
    i = int(math.floor(a / FRAME_S))
    j = max(i + 2, int(math.floor(b / FRAME_S)))
    return env[max(0, i):max(0, min(len(env), j))]


def edge_levels(env: np.ndarray, a: float, b: float) -> tuple[float, float]:
    """(head, tail) level of [a, b) against its own peak, in dB (F8)."""
    f = seg_frames(env, a, b)
    if not len(f):
        return 0.0, 0.0
    top = float(f.max())
    return float(f[0] - top), float(f[-2:].max() - top)


def has_gap(frames: np.ndarray) -> bool:
    """True when 2 or more consecutive frames are at or below -100 dB."""
    low = frames <= GAP_DB
    return bool(np.any(low[1:] & low[:-1])) if len(low) > 1 else False


def edit_gap(env: np.ndarray, a: float, b: float) -> bool:
    """An edit gap (digital silence between takes) inside [a, b)."""
    return has_gap(seg_frames(env, a, b))


def _quiet_levels(env: np.ndarray, floor: float, i0: int, i1: int) -> tuple[float, float]:
    peak = float(env[i0:i1].max())
    return max(floor + 6, peak - 40), max(floor + 4, peak - DECAY_DB)


def _longest_run(mask: np.ndarray) -> int:
    best = run = 0
    for v in mask:
        run = run + 1 if v else 0
        best = max(best, run)
    return best


def multiple_sounds(env: np.ndarray, floor: float, a: float, b: float) -> bool:
    """True when a window holds 2 or more separate sounds (F9)."""
    i0, i1 = int(round(a / FRAME_S)), min(len(env), int(round(b / FRAME_S)))
    if i1 - i0 < 3:
        return False
    q, _ = _quiet_levels(env, floor, i0, i1)
    above = np.nonzero(env[i0:i1] > q)[0]
    if len(above) < 2:
        return False
    mid = env[i0 + above[0]:i0 + above[-1] + 1]
    return has_gap(mid) or _longest_run(mid <= q) >= QUIET_RUN


# ================================================================ find: windows


def _default_win(span: float) -> float:
    win = min(5.0, max(1.0, span / 6))
    while win < 10 and (span - win) / (win / 2) + 1 > WINDOW_TARGET:
        win = min(10.0, 1.25 * win)
    return win


def window_grid(t0: float, d: float, win: float | None = None, hop: float | None = None) -> list[tuple[float, float]]:
    """Sliding windows that cover [t0, d) with no gaps (F3)."""
    span = d - t0
    win = win or _default_win(span)
    if span <= win:
        return [(t0, d)]
    if not hop:
        hop = win / 2
        if (span - win) / hop + 1 > WINDOW_TARGET:
            hop = min(win, (span - win) / (WINDOW_TARGET - 1))
    starts, s = [], t0
    while s <= d - win + 1e-6:
        starts.append(s)
        s = t0 + len(starts) * hop
    if starts[-1] + win < d - 0.001:
        starts.append(d - win)
    return [(a, a + win) for a in starts]


def _search(env: np.ndarray, frm: int, stop: int, step: int, level: float) -> int | None:
    """The first frame from frm towards stop (exclusive) at or below level."""
    for i in range(frm, stop, step):
        if env[i] <= level:
            return i
    return None


def _snap_start(env: np.ndarray, i0: int, i1: int, q: float, nlo: int) -> int:
    w = SLOPE_FRAMES
    falling = i0 - w >= nlo and env[i0 - w:i0].mean() > env[i0:i0 + w].mean() + SLOPE_DB
    if falling:
        hit = _search(env, i0, min(i1 - 1, i0 + SNAP_FRAMES + 1), 1, q)
        return hit if hit is not None and (i1 - hit) * FRAME_S >= MIN_SNAP_S else i0
    hit = _search(env, i0, max(nlo, i0 - SNAP_FRAMES) - 1, -1, q)
    return hit if hit is not None else i0


def _decay_walk(env: np.ndarray, last: int, qe: float, nhi: int) -> int:
    i, low = last, float(env[last])
    while i + 1 < nhi and i + 1 - last <= SNAP_FRAMES and env[i] > qe and env[i + 1] <= low + CLIMB_DB:
        i += 1
        low = min(low, float(env[i]))
    return i + 1


def _snap_end(env: np.ndarray, i0: int, i1: int, q: float, qe: float, nhi: int) -> int:
    last, w = i1 - 1, SLOPE_FRAMES
    attack = last + 1 + w <= nhi and env[last + 1:last + 1 + w].mean() > env[max(i0, last - w):last + 1].mean() + SLOPE_DB
    if attack:
        hit = _search(env, last, i0, -1, q)
        return hit + 1 if hit is not None and (hit + 1 - i0) * FRAME_S >= MIN_SNAP_S else i1
    return _decay_walk(env, last, qe, nhi)


def _tighten(env: np.ndarray, i0: int, i1: int, q: float, qe: float) -> tuple[int, int]:
    up = np.nonzero(env[i0:i1] > q)[0]
    if len(up):
        i0 = max(i0, i0 + int(up[0]) - 1)
    dec = np.nonzero(env[i0:i1] > qe)[0]
    if len(dec):
        i1 = i0 + int(dec[-1]) + 1
    return i0, i1


def snap_window(env: np.ndarray, floor: float, a: float, b: float, lo: float, hi: float) -> tuple[float, float]:
    """Move window edges to the quiet before and after the sound (F4)."""
    n = len(env)
    i0 = int(round(a / FRAME_S))
    i1 = min(n, max(i0 + 1, int(round(b / FRAME_S))))
    if i0 >= i1 - 1:
        return a, b
    q, qe = _quiet_levels(env, floor, i0, i1)
    nlo, nhi = max(0, int(round(lo / FRAME_S))), min(n, int(round(hi / FRAME_S)))
    if env[i0] > q:
        i0 = _snap_start(env, i0, i1, q, nlo)
    if env[i1 - 1] > qe:
        i1 = _snap_end(env, i0, i1, q, qe, nhi)
    i0, i1 = _tighten(env, i0, i1, q, qe)
    end = i1 * FRAME_S
    return max(lo, i0 * FRAME_S), min(hi, end + END_PAD_S if end < hi else end)


def dedupe(cands: Sequence[dict]) -> list[dict]:
    """Drop a candidate that an earlier kept one overlaps by >= 0.9 of their joint extent (F7)."""
    kept: list[dict] = []
    for c in cands:
        if not any(min(k["end"], c["end"]) - max(k["start"], c["start"])
                   >= DEDUPE * (max(k["end"], c["end"]) - min(k["start"], c["start"])) for k in kept):
            kept.append(c)
    return kept


def round_robin(cands: Sequence[dict], n: int = CAP) -> list[dict]:
    """Take one candidate per 2 s bucket per round, best priority first (F6)."""
    buckets: dict[int, list[dict]] = {}
    for c in cands:
        buckets.setdefault(int(math.floor(c["start"] / BUCKET_S)), []).append(c)
    queues = [sorted(buckets[k], key=lambda c: -c["priority"]) for k in sorted(buckets)]
    out: list[dict] = []
    while len(out) < n and any(queues):
        for q in queues:
            if q and len(out) < n:
                out.append(q.pop(0))
    return out


# ================================================================ find: strokes and events


def _attack_time(env1: np.ndarray, floor: float, o: float) -> float:
    a0 = max(0, int(math.floor((o - ATTACK_REACH_S) / 0.001)))
    a1 = min(len(env1), int(math.floor((o + ATTACK_REACH_S) / 0.001)) + 1)
    if a1 <= a0:
        return o
    seg = env1[a0:a1]
    hit = np.nonzero(seg >= max(floor + 10, float(seg.max()) - 30))[0]
    return (a0 + int(hit[0])) * 0.001 if len(hit) else o


def _stroke(env: np.ndarray, env1: np.ndarray, floor: float, o: float, nx: float, guard: float,
            prev_end: float) -> dict | None:
    f0 = int(math.floor(o / FRAME_S))
    f1 = min(len(env), max(f0 + 1, int(math.floor(nx / FRAME_S))))
    if f0 >= f1:
        return None
    pk = f0 + int(np.argmax(env[f0:min(f1, f0 + PEAK_FRAMES)]))
    below = np.nonzero(env[pk:f1] < max(env[pk] - DECAY_DB, floor + STROKE_FLOOR_DB))[0]
    end = (pk + int(below[0])) * FRAME_S + END_PAD_S if len(below) else nx
    start = max(0.0, _attack_time(env1, floor, o) - PRE_ROLL, prev_end)
    end = min(end, guard, start + EVENT_MAX_S)
    if end - start < MIN_STROKE_S:
        return None
    prom = float(env[pk] - floor)
    return {"start": start, "end": end, "kind": "stroke", "priority": prom, "prom": prom}


def strokes_of(env: np.ndarray, env1: np.ndarray, floor: float, onsets: Sequence[float], d: float) -> list[dict]:
    """One stroke per onset: just before the attack to 35 dB down or the next onset (F5 step 3)."""
    out: list[dict] = []
    for i, o in enumerate(onsets):
        last = i == len(onsets) - 1
        nx = d if last else onsets[i + 1]
        s = _stroke(env, env1, floor, o, nx, d if last else nx - NEXT_GUARD_S, out[-1]["end"] if out else 0.0)
        if s:
            out.append(s)
    return out


def _event(first: dict, last: dict, prio: float) -> dict:
    return {"start": first["start"], "end": last["end"], "kind": "event", "priority": prio, "prom": prio}


def _joined(env: np.ndarray, a: dict, b: dict) -> bool:
    return b["start"] - a["end"] < JOIN_GAP_S and not edit_gap(env, a["start"], b["start"])


def pairs_of(env: np.ndarray, st: Sequence[dict]) -> list[dict]:
    """Events of 2 joined strokes, never led by a much weaker stroke (F5 step 6)."""
    return [_event(a, b, max(a["prom"], b["prom"])) for a, b in zip(st, st[1:])
            if _joined(env, a, b) and b["end"] - a["start"] <= EVENT_MAX_S and a["prom"] >= b["prom"] - WEAK_LEAD_DB]


def chains_of(env: np.ndarray, st: Sequence[dict]) -> list[dict]:
    """Events of more than 2 joined strokes within 2 s (F5 step 7)."""
    out, i = [], 0
    while i < len(st):
        j = i
        while j + 1 < len(st) and _joined(env, st[j], st[j + 1]) and st[j + 1]["end"] - st[i]["start"] <= EVENT_MAX_S:
            j += 1
        chain = list(st[i:j + 1])
        top = max(s["prom"] for s in chain)
        while chain and chain[0]["prom"] < top - WEAK_LEAD_DB:
            chain.pop(0)
        if len(chain) > 2:
            out.append(_event(chain[0], chain[-1], top))
        i = j + 1
    return out


def _isolate(env: np.ndarray, floor: float, c: dict) -> None:
    f = int(math.floor(c["start"] / FRAME_S))
    pre = env[max(0, f - SLOPE_FRAMES):max(0, f)]
    if (not len(pre) or pre.mean() <= floor + 10) and edge_levels(env, c["start"], c["end"])[1] <= -25:
        c["priority"] += ISOLATION_BONUS


def onset_candidates(m: np.ndarray, env: np.ndarray, floor: float, d: float) -> list[dict]:
    """Strokes, pairs and chains of the local audio, with the isolation bonus (F5)."""
    onsets = [float(o) for o in features.onsets(np.asarray(m, np.float32), SR)]
    if not onsets:
        return []
    st = strokes_of(env, envelope(m, ATTACK_FRAME), floor, onsets, d)
    cands = st + pairs_of(env, st) + chains_of(env, st)
    for c in cands:
        _isolate(env, floor, c)
    return cands


# ================================================================ find: query kind and scoring


def query_kind(v: voc.Vocab, q: np.ndarray) -> tuple[str, float]:
    """'event', 'texture' or 'either' from the query's text embedding (F2)."""
    e = np.asarray(v.embed_text(EVENT_ANCHORS), np.float64).mean(axis=0)
    t = np.asarray(v.embed_text(TEXTURE_ANCHORS), np.float64).mean(axis=0)
    score = float(np.asarray(q, np.float64) @ e - np.asarray(q, np.float64) @ t)
    kind = "event" if score >= KIND_EVENT else "texture" if score <= KIND_TEXTURE else "either"
    return kind, round(score, 3)


def score_cands(ctx: dict, cands: list[dict]) -> None:
    """Embed every candidate as one CLAP span and add its z and cosine."""
    if not cands:
        return
    sp = [(ctx["m0"] + c["start"], ctx["m0"] + c["end"]) for c in cands]
    W = voc.span_embeddings([(ctx["path"], sp)])[0]
    cal = ctx["vocab"].calibrate(W, ctx["q"][None, :])
    for i, c in enumerate(cands):
        c["z"], c["cosine"] = round(float(cal.z_q[i, 0]), 2), float(cal.Cq[i, 0])


def _windows(ctx: dict, win: float | None, hop: float | None) -> list[dict]:
    grid = window_grid(ctx["off"], ctx["d"], win, hop)
    ctx["n_windows"], ctx["win_s"] = len(grid), grid[0][1] - grid[0][0]
    if ctx["prefer"]:
        grid = [snap_window(ctx["env"], ctx["floor"], a, b, ctx["off"], ctx["d"]) for a, b in grid]
    wins = [{"start": a, "end": b, "kind": "window", "priority": 0.0} for a, b in grid if b > a]
    return dedupe(wins) if ctx["prefer"] else wins


def _overlaps(a: dict, b: dict, tol: float = 0.0) -> bool:
    return a["start"] < b["end"] - tol and b["start"] < a["end"] - tol


def _events(ctx: dict, wins: list[dict]) -> list[dict]:
    cands = [c for c in onset_candidates(ctx["m"], ctx["env"], ctx["floor"], ctx["d"])
             if c["start"] + PRE_ROLL >= ctx["off"] - 0.001]
    ctx["n_onset"] = len(cands)
    ctx["stages"] = 1 if ctx["s1"] - ctx["s0"] <= ONE_STAGE_S else 2
    if ctx["stages"] == 2:
        best = sorted(wins, key=lambda w: -w["z"])[:BEST_WINDOWS]
        cands = [c for c in cands if any(_overlaps(c, w) for w in best)]
    picked = dedupe(round_robin(cands))
    score_cands(ctx, picked)
    return picked


def _judge(ctx: dict, cands: list[dict]) -> None:
    for c in cands:
        c["head_db"], c["tail_db"] = edge_levels(ctx["env"], c["start"], c["end"])
        judged = ctx["use_events"] and (ctx["prefer"] or c["kind"] != "window")
        heads = judged and c["head_db"] > CUT_DB
        tails = judged and c["tail_db"] > CUT_DB
        c["cut"] = ", ".join(w for w, on in (("head cut", heads), ("tail cut", tails)) if on)
        c["score"] = c["z"] - CUT_PENALTY * bool(c["cut"])


def _excluded(ctx: dict, w: dict, events: list[dict]) -> bool:
    inside = [e["z"] for e in events
              if e["start"] >= w["start"] - INSIDE_TOL_S and e["end"] <= w["end"] + INSIDE_TOL_S]
    if inside and max(inside) >= w["z"] - REFINE_Z:
        return True
    return bool(events) and multiple_sounds(ctx["env"], ctx["floor"], w["start"], w["end"])


def _local_peak(ctx: dict, c: dict) -> float:
    a, b = int(round(c["start"] * SR)), int(round(c["end"] * SR))
    return voc.array_peak_db(ctx["x"][a:b])


def _select(ctx: dict, cands: list[dict], top: int) -> list[dict]:
    loud = voc.array_peak_db(ctx["x"]) >= NEAR_SILENT_DB
    taken: list[dict] = []
    for c in sorted(cands, key=lambda c: -c["score"]):
        if len(taken) >= top:
            break
        if c.get("excluded") or any(_overlaps(c, t, 0.001) for t in taken):
            continue
        c["peak_dbfs"] = _local_peak(ctx, c)
        if not loud or c["peak_dbfs"] >= NEAR_SILENT_DB:
            taken.append(c)
    return taken


def _segment(ctx: dict, c: dict) -> dict:
    s, e = ctx["m0"] + c["start"], ctx["m0"] + c["end"]
    return {"start": round(s, 3), "end": round(e, 3), "duration": round(e - s, 3), "score": round(c["score"], 2),
            "z": c["z"], "cosine": round(c["cosine"], 3), "verdict": voc.verdict(c["z"]), "kind": c["kind"],
            "peak_dbfs": round(c["peak_dbfs"], 1), "head_db": round(c["head_db"], 1),
            "tail_db": round(c["tail_db"], 1), "cut": c["cut"]}


# ================================================================ find: driver


def _context(path: str, query: str, kind: str, start: float | None, end: float | None) -> dict:
    if kind not in KINDS:
        raise ValueError(f"kind must be one of {', '.join(KINDS)}")
    dur = voc.check_range(path, start, end)
    s0 = start or 0.0
    s1 = min(end, dur) if end is not None else dur
    m0 = max(0.0, s0 - MARGIN_S)
    x = audio.load(path, start=m0, end=s1)
    m = audio.to_mono(np.asarray(x, np.float32))
    v = voc.get()
    q = np.asarray(v.embed_text([query])[0], np.float64)
    read, kscore = query_kind(v, q)
    env = envelope(m)
    return {"path": path, "query": query, "kind": kind, "s0": s0, "s1": s1, "m0": m0, "off": s0 - m0, "x": x,
            "m": m, "d": len(m) / SR, "vocab": v, "q": q, "read": read, "kind_score": kscore, "env": env,
            "floor": noise_floor(env), "use_events": kind == "event" or (kind == "auto" and read != "texture"),
            "prefer": kind == "event" or (kind == "auto" and read == "event"), "n_onset": 0, "stages": 0}


def _candidates(ctx: dict, win: float | None, hop: float | None) -> list[dict]:
    wins = _windows(ctx, win, hop)
    score_cands(ctx, wins)
    events = _events(ctx, wins) if ctx["use_events"] else []
    ctx["dropped"] = ctx["kind"] == "event" and bool(events)
    if ctx["dropped"]:
        wins = []
    cands = events + wins
    _judge(ctx, cands)
    if ctx["prefer"]:
        for w in wins:
            w["excluded"] = _excluded(ctx, w, events)
    ctx["n_events"], ctx["n_candidates"] = len(events), len(cands)
    return cands


def find(path: str, query: str, win: float | None = None, hop: float | None = None, top: int = 5,
         start: float | None = None, end: float | None = None, kind: str = "auto") -> dict:
    """The best non-overlapping moments of one recording for a description, as trims."""
    query = query_text(query)
    ctx = _context(path, query, kind, start, end)
    segs = [_segment(ctx, c) for c in _select(ctx, _candidates(ctx, win, hop), top)]
    return {"summary": find_summary(ctx, segs), "path": path, "query": query,
            "query_kind": ctx["read"] if kind == "auto" else kind, "query_kind_score": ctx["kind_score"],
            "duration": round(ctx["s1"] - ctx["s0"], 3), "n_candidates": ctx["n_candidates"],
            "n_windows": ctx["n_windows"], "n_events": ctx["n_events"], "segments": segs}


# ================================================================ find: summary and text


def _how_text(ctx: dict) -> str:
    how = f"query reads as {ctx['read']}" if ctx["kind"] == "auto" else f"kind={ctx['kind']}"
    span = ctx["s1"] - ctx["s0"]
    where = f"all {span:.1f} s" if ctx["s0"] == 0 and ctx["s1"] >= audio.duration(ctx["path"]) - 1e-6 else \
        f"{report.fmt_t(ctx['s0'])}-{report.fmt_t(ctx['s1'])} ({span:.1f} s)"
    parts = [f"Searched {where} ({how}) with {ctx['n_windows']} windows of about {ctx['win_s']:.1f} s"]
    if ctx["prefer"]:
        parts.append("edges snapped to the sound")
    if ctx.get("dropped"):
        parts.append("windows only located events")
    if ctx["use_events"]:
        stages = "one stage" if ctx["stages"] == 1 else "two stages: events inside the 6 best windows"
        parts.append(f"{ctx['n_onset']} onset-based candidates, {stages}")
    return "; ".join(parts) + "."


def _segments_text(segs: Sequence[dict]) -> str:
    items = [f"{report.fmt_t(s['start'])}-{report.fmt_t(s['end'])} ({s['duration']:.3f} s, {s['kind']}, "
             f"z {voc.zf(s['z'])}, {s['verdict']}{', ' + s['cut'] if s['cut'] else ''})" for s in segs]
    return ("Best: " + "; ".join(items) + ". Strokes and events start 5 ms before the attack; windows are snapped "
            "to the sound or are plain stretches to crossfade; a head or tail cut trim cuts into a sound.")


def find_summary(ctx: dict, segs: Sequence[dict]) -> str:
    """The find paragraph (F11)."""
    parts = [_how_text(ctx)]
    if segs:
        parts.append(_segments_text(segs))
        if segs[0]["verdict"] in ("weak", voc.NO_MATCH):
            parts.append("The recording may not contain this sound.")
    else:
        parts.append("No candidate segments.")
    text = " ".join(parts)
    peak = voc.array_peak_db(ctx["x"][int(round(ctx["off"] * SR)):])
    if peak < NEAR_SILENT_DB:
        text = f"Warning: the searched audio is near-silent (peak {peak:.0f} dBFS); scores mean nothing. " + text
    return text


def find_text(result: dict) -> str:
    """Text output of find."""
    lines = [result["summary"]]
    if result["segments"]:
        lines.append("")
    for i, s in enumerate(result["segments"], 1):
        cut = f"  score {s['score']:+.2f} ({s['cut']})" if s["cut"] else ""
        lines.append(f"{i:>2}. {s['start']:.3f}-{s['end']:.3f}  [{report.fmt_t(s['start'])}]  {s['duration']:.3f} s  "
                     f"z {voc.zf(s['z'])}  {s['verdict']}  {s['kind']}  peak {s['peak_dbfs']:.1f}  "
                     f"head {s['head_db']:.0f} dB  tail {s['tail_db']:.0f} dB{cut}")
    return "\n".join(lines)


# ================================================================ CLI


def _require(name: str) -> None:
    import hark.models

    hark.models.require_ml(name)


@report.expected_errors("rank")
def _run_rank(args: argparse.Namespace) -> int | None:
    _require("rank")
    result = rank(args.paths, args.query, top=args.top)
    report.emit(result, args, text=rank_text(result))
    return 0


@report.expected_errors("find")
def _run_find(args: argparse.Namespace) -> int | None:
    _require("find")
    result = find(args.path, args.query, win=args.win, hop=args.hop, top=args.top, start=args.start, end=args.end,
                  kind=args.kind)
    report.emit(result, args, text=find_text(result))
    return 0


def add_cli(sub: argparse._SubParsersAction) -> None:
    """Register ``rank`` and ``find``."""
    p = sub.add_parser("rank", help="Rank files by how well they fit one description.")
    p.add_argument("query", help="the description")
    p.add_argument("paths", nargs="+", help="files, folders or globs")
    p.add_argument("--top", type=int, default=None, help="list only the best N files")
    report.add_common(p)
    p.set_defaults(func=_run_rank)
    f = sub.add_parser("find", help="The best moments of one recording for a description, as trims.")
    f.add_argument("path", help="the recording")
    f.add_argument("query", help="the description")
    f.add_argument("--top", type=int, default=5, help="number of non-overlapping segments")
    f.add_argument("--win", type=float, default=None, help="window length (s)")
    f.add_argument("--hop", type=float, default=None, help="hop between windows (s)")
    f.add_argument("--kind", choices=KINDS, default="auto", help="candidate kinds")
    f.add_argument("--start", type=float, default=None)
    f.add_argument("--end", type=float, default=None)
    report.add_common(f)
    f.set_defaults(func=_run_find)
