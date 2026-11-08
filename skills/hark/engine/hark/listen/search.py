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

import numpy

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


def query_text(prompt: Any) -> str:
    """The trimmed query; a value error when it is empty."""
    if not isinstance(prompt, str) or not prompt.strip():
        raise ValueError("no query given")
    return prompt.strip()


# ================================================================ rank


def _rank_rows(vec: voc.Vocab, paths: list[str], prompt: str) -> list[dict]:
    A = voc.embed_files(paths)
    cal = vec.calibrate(A, vec.embed_text([prompt]))
    records = []
    for ix, pth in enumerate(paths):
        z = round(float(cal.z_q[ix, 0]), 2)
        silent = voc.peak_db(pth, mono=True) < NEAR_SILENT_DB
        term, _, tz, _ = vec.ranked_terms(cal.Z[ix], cal.C[ix], 1, exclude=voc.NOT_SOUNDS_LIKE)[0]
        records.append({"path": pth, "name": os.path.basename(pth), "z": z, "cosine": round(float(cal.Cq[ix, 0]), 3),
                     "vocab_percentile": round(100 * float(cal.percentile[ix, 0]), 1),
                     "verdict": voc.NO_MATCH if silent else voc.verdict(z), "near_silent": silent,
                     "sounds_most_like": f"{term} (z {voc.zf(tz)})"})
    return sorted(records, key=lambda row_: (row_["near_silent"], -row_["z"]))


def _band_counts(records: Sequence[dict]) -> dict[str, int]:
    live = [row_ for row_ in records if not row_["near_silent"]]
    return {rhs: sum(row_["verdict"] == rhs for row_ in live) for rhs in voc.VERDICT_THRESHOLDS_Z}


def _best_text(listed: list[dict]) -> str:
    live = [row_ for row_ in listed if not row_["near_silent"]][:3]
    return ", ".join(f"{row_['name']} (z {voc.zf(row_['z'])}, {row_['verdict']})" for row_ in live) or "none (all near-silent)"


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


def _silent_text(records: list[dict]) -> str | None:
    count = sum(row_["near_silent"] for row_ in records)
    return f"{count} near-silent file(s) ranked last; their scores mean nothing." if count else None


def _rank_summary(prompt: str, records: list[dict], listed: list[dict], skipped: list[dict]) -> str:
    counts = _band_counts(records)
    pieces = [f"Ranked {len(records)} files for '{prompt}'. Best: {_best_text(listed)}.",
             f"Bands: {counts['strong']} strong, {counts['plausible']} plausible, {counts['weak']} weak.",
             _weak_best_text(listed), _strong_text(counts["strong"]), _silent_text(records)]
    return " ".join(pth for pth in pieces if pth) + _skipped_note(skipped)


def _skipped_note(skipped: Sequence[dict]) -> str:
    if not skipped:
        return ""
    names = ", ".join(os.path.basename(sv["path"]) for sv in skipped[:3]) + (", ..." if len(skipped) > 3 else "")
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
    records = _rank_rows(voc.get(), ok, query)
    listed = records[:top] if top else records
    return {"summary": _rank_summary(query, records, listed, skipped), "query": query, "n_files": len(ok),
            "results": listed, "skipped": skipped}


def display_names(records: Sequence[dict]) -> list[str]:
    """Base names, or '<parent>/<name>' when 2 results share a base name."""
    names = [row_["name"] for row_ in records]
    return [f"{os.path.basename(os.path.dirname(row_['path']))}/{count}" if names.count(count) > 1 else count
            for row_, count in zip(records, names)]


def rank_text(result: dict) -> str:
    """Text output of rank."""
    lines = [result["summary"]]
    if result["results"]:
        lines.append("")
    for ix, (row_, name) in enumerate(zip(result["results"], display_names(result["results"])), 1):
        flag = "  NEAR-SILENT" if row_["near_silent"] else ""
        lines.append(f"{ix:>3}. z {voc.zf(row_['z'])}  {row_['verdict']:<9}  cos {row_['cosine']:.3f}  {name}  "
                     f"(sounds most like {row_['sounds_most_like']}){flag}")
    return "\n".join(lines)


# ================================================================ find: envelope helpers


def envelope(mat: numpy.ndarray, frame: int = FRAME) -> numpy.ndarray:
    """Frame levels in dB of non-overlapping frames."""
    mat = numpy.asarray(mat, numpy.float64)
    if len(mat) < frame:
        return numpy.zeros(0)
    return 20 * numpy.log10(numpy.maximum(features.frame_rms(mat, frame), 1e-12))


def noise_floor(envl: numpy.ndarray) -> float:
    """The 10th percentile of the frame levels."""
    return float(numpy.percentile(envl, 10)) if len(envl) else -120.0


def seg_frames(envl: numpy.ndarray, lhs: float, rhs: float) -> numpy.ndarray:
    """The frames of [a, b) (F8 rule)."""
    ix = int(math.floor(lhs / FRAME_S))
    jj = max(ix + 2, int(math.floor(rhs / FRAME_S)))
    return envl[max(0, ix):max(0, min(len(envl), jj))]


def edge_levels(envl: numpy.ndarray, lhs: float, rhs: float) -> tuple[float, float]:
    """(head, tail) level of [a, b) against its own peak, in dB (F8)."""
    fv = seg_frames(envl, lhs, rhs)
    if not len(fv):
        return 0.0, 0.0
    best_n = float(fv.max())
    return float(fv[0] - best_n), float(fv[-2:].max() - best_n)


def has_gap(frames: numpy.ndarray) -> bool:
    """True when 2 or more consecutive frames are at or below -100 dB."""
    low = frames <= GAP_DB
    return bool(numpy.any(low[1:] & low[:-1])) if len(low) > 1 else False


def edit_gap(envl: numpy.ndarray, lhs: float, rhs: float) -> bool:
    """An edit gap (digital silence between takes) inside [a, b)."""
    return has_gap(seg_frames(envl, lhs, rhs))


def _quiet_levels(envl: numpy.ndarray, noise_lo: float, ix_lo: int, ix_hi: int) -> tuple[float, float]:
    peak = float(envl[ix_lo:ix_hi].max())
    return max(noise_lo + 6, peak - 40), max(noise_lo + 4, peak - DECAY_DB)


def _longest_run(mask: numpy.ndarray) -> int:
    winner = run = 0
    for vec in mask:
        run = run + 1 if vec else 0
        winner = max(winner, run)
    return winner


def multiple_sounds(envl: numpy.ndarray, noise_lo: float, lhs: float, rhs: float) -> bool:
    """True when a window holds 2 or more separate sounds (F9)."""
    ix_lo, ix_hi = int(round(lhs / FRAME_S)), min(len(envl), int(round(rhs / FRAME_S)))
    if ix_hi - ix_lo < 3:
        return False
    qtext, _ = _quiet_levels(envl, noise_lo, ix_lo, ix_hi)
    above = numpy.nonzero(envl[ix_lo:ix_hi] > qtext)[0]
    if len(above) < 2:
        return False
    mid = envl[ix_lo + above[0]:ix_lo + above[-1] + 1]
    return has_gap(mid) or _longest_run(mid <= qtext) >= QUIET_RUN


# ================================================================ find: windows


def _default_win(span: float) -> float:
    frame_w = min(5.0, max(1.0, span / 6))
    while frame_w < 10 and (span - frame_w) / (frame_w / 2) + 1 > WINDOW_TARGET:
        frame_w = min(10.0, 1.25 * frame_w)
    return frame_w


def window_grid(t0: float, dd: float, win: float | None = None, hop: float | None = None) -> list[tuple[float, float]]:
    """Sliding windows that cover [t0, d) with no gaps (F3)."""
    span = dd - t0
    win = win or _default_win(span)
    if span <= win:
        return [(t0, dd)]
    if not hop:
        hop = win / 2
        if (span - win) / hop + 1 > WINDOW_TARGET:
            hop = min(win, (span - win) / (WINDOW_TARGET - 1))
    starts, sv = [], t0
    while sv <= dd - win + 1e-6:
        starts.append(sv)
        sv = t0 + len(starts) * hop
    if starts[-1] + win < dd - 0.001:
        starts.append(dd - win)
    return [(lhs, lhs + win) for lhs in starts]


def _search(envl: numpy.ndarray, frm: int, stop: int, step: int, level: float) -> int | None:
    """The first frame from frm towards stop (exclusive) at or below level."""
    for ix in range(frm, stop, step):
        if envl[ix] <= level:
            return ix
    return None


def _snap_start(envl: numpy.ndarray, ix_lo: int, ix_hi: int, qtext: float, nlo: int) -> int:
    win_n = SLOPE_FRAMES
    falling = ix_lo - win_n >= nlo and envl[ix_lo - win_n:ix_lo].mean() > envl[ix_lo:ix_lo + win_n].mean() + SLOPE_DB
    if falling:
        hit = _search(envl, ix_lo, min(ix_hi - 1, ix_lo + SNAP_FRAMES + 1), 1, qtext)
        return hit if hit is not None and (ix_hi - hit) * FRAME_S >= MIN_SNAP_S else ix_lo
    hit = _search(envl, ix_lo, max(nlo, ix_lo - SNAP_FRAMES) - 1, -1, qtext)
    return hit if hit is not None else ix_lo


def _keeps_decaying(envl: numpy.ndarray, ix: int, low: float, qe: float) -> bool:
    return envl[ix] > qe and envl[ix + 1] <= low + CLIMB_DB


def _decay_walk(envl: numpy.ndarray, prev_: int, qe: float, nhi: int) -> int:
    ix, low = prev_, float(envl[prev_])
    stop = min(nhi - 1, prev_ + SNAP_FRAMES)
    while ix < stop and _keeps_decaying(envl, ix, low, qe):
        ix += 1
        low = min(low, float(envl[ix]))
    return ix + 1


def _snap_end(envl: numpy.ndarray, ix_lo: int, ix_hi: int, qtext: float, qe: float, nhi: int) -> int:
    prev_, win_n = ix_hi - 1, SLOPE_FRAMES
    attack = prev_ + 1 + win_n <= nhi and envl[prev_ + 1:prev_ + 1 + win_n].mean() > envl[max(ix_lo, prev_ - win_n):prev_ + 1].mean() + SLOPE_DB
    if attack:
        hit = _search(envl, prev_, ix_lo, -1, qtext)
        return hit + 1 if hit is not None and (hit + 1 - ix_lo) * FRAME_S >= MIN_SNAP_S else ix_hi
    return _decay_walk(envl, prev_, qe, nhi)


def _tighten(envl: numpy.ndarray, ix_lo: int, ix_hi: int, qtext: float, qe: float) -> tuple[int, int]:
    up = numpy.nonzero(envl[ix_lo:ix_hi] > qtext)[0]
    if len(up):
        ix_lo = max(ix_lo, ix_lo + int(up[0]) - 1)
    dec = numpy.nonzero(envl[ix_lo:ix_hi] > qe)[0]
    if len(dec):
        ix_hi = ix_lo + int(dec[-1]) + 1
    return ix_lo, ix_hi


def _start_edge(envl: numpy.ndarray, ix_lo: int, ix_hi: int, qtext: float, nlo: int) -> int:
    return _snap_start(envl, ix_lo, ix_hi, qtext, nlo) if envl[ix_lo] > qtext else ix_lo


def _end_edge(envl: numpy.ndarray, ix_lo: int, ix_hi: int, levels: tuple[float, float], nhi: int) -> int:
    qtext, qe = levels
    return _snap_end(envl, ix_lo, ix_hi, qtext, qe, nhi) if envl[ix_hi - 1] > qe else ix_hi


def _snapped(envl: numpy.ndarray, ix_lo: int, ix_hi: int, qtext: float, qe: float, near: tuple[int, int]) -> tuple[int, int]:
    """Frame edges moved to the quiet around the sound, inside the near frames (nlo, nhi)."""
    ix_lo = _start_edge(envl, ix_lo, ix_hi, qtext, near[0])
    ix_hi = _end_edge(envl, ix_lo, ix_hi, (qtext, qe), near[1])
    return _tighten(envl, ix_lo, ix_hi, qtext, qe)


def snap_window(envl: numpy.ndarray, noise_lo: float, lhs: float, rhs: float, lo: float, hi: float) -> tuple[float, float]:
    """Move window edges to the quiet before and after the sound (F4)."""
    count = len(envl)
    ix_lo = int(round(lhs / FRAME_S))
    ix_hi = min(count, max(ix_lo + 1, int(round(rhs / FRAME_S))))
    if ix_lo >= ix_hi - 1:
        return lhs, rhs
    qtext, qe = _quiet_levels(envl, noise_lo, ix_lo, ix_hi)
    ix_lo, ix_hi = _snapped(envl, ix_lo, ix_hi, qtext, qe, (max(0, int(round(lo / FRAME_S))), min(count, int(round(hi / FRAME_S)))))
    t_to = ix_hi * FRAME_S
    return max(lo, ix_lo * FRAME_S), min(hi, t_to + END_PAD_S if t_to < hi else t_to)


def dedupe(pool: Sequence[dict]) -> list[dict]:
    """Drop a candidate that an earlier kept one overlaps by >= 0.9 of their joint extent (F7)."""
    kept: list[dict] = []
    for cand in pool:
        if not any(min(kk["end"], cand["end"]) - max(kk["start"], cand["start"])
                   >= DEDUPE * (max(kk["end"], cand["end"]) - min(kk["start"], cand["start"])) for kk in kept):
            kept.append(cand)
    return kept


def round_robin(pool: Sequence[dict], count: int = CAP) -> list[dict]:
    """Take one candidate per 2 s bucket per round, best priority first (F6)."""
    buckets: dict[int, list[dict]] = {}
    for cand in pool:
        buckets.setdefault(int(math.floor(cand["start"] / BUCKET_S)), []).append(cand)
    queues = [sorted(buckets[kk], key=lambda cand: -cand["priority"]) for kk in sorted(buckets)]
    result: list[dict] = []
    while len(result) < count and any(queues):
        for qtext in queues:
            if qtext and len(result) < count:
                result.append(qtext.pop(0))
    return result


# ================================================================ find: strokes and events


def _attack_time(env1: numpy.ndarray, noise_lo: float, o: float) -> float | None:
    """The attack near onset o (F5 step 3.4); None when no frame reaches floor + 10 dB (an onset in the noise)."""
    a0 = max(0, int(math.floor((o - ATTACK_REACH_S) / 0.001)))
    a1 = min(len(env1), int(math.floor((o + ATTACK_REACH_S) / 0.001)) + 1)
    if a1 <= a0:
        return o
    seg = env1[a0:a1]
    hit = numpy.nonzero(seg >= max(noise_lo + 10, float(seg.max()) - 30))[0]
    return (a0 + int(hit[0])) * 0.001 if len(hit) else None


def _stroke(envl: numpy.ndarray, env1: numpy.ndarray, noise_lo: float, o: float, nx: float, guard: float,
            prev_end: float) -> dict | None:
    f0 = int(math.floor(o / FRAME_S))
    f1 = min(len(envl), max(f0 + 1, int(math.floor(nx / FRAME_S))))
    attack = _attack_time(env1, noise_lo, o)
    if f0 >= f1 or attack is None:
        return None
    pk = f0 + int(numpy.argmax(envl[f0:min(f1, f0 + PEAK_FRAMES)]))
    below = numpy.nonzero(envl[pk:f1] < max(envl[pk] - DECAY_DB, noise_lo + STROKE_FLOOR_DB))[0]
    t_to = (pk + int(below[0])) * FRAME_S + END_PAD_S if len(below) else nx
    t_from = max(0.0, attack - PRE_ROLL, prev_end)
    t_to = min(t_to, guard, t_from + EVENT_MAX_S)
    if t_to - t_from < MIN_STROKE_S:
        return None
    prom = float(envl[pk] - noise_lo)
    return {"start": t_from, "end": t_to, "kind": "stroke", "priority": prom, "prom": prom}


def strokes_of(envl: numpy.ndarray, env1: numpy.ndarray, noise_lo: float, onsets: Sequence[float], dd: float) -> list[dict]:
    """One stroke per onset: just before the attack to 35 dB down or the next onset (F5 step 3)."""
    result: list[dict] = []
    for ix, o in enumerate(onsets):
        prev_ = ix == len(onsets) - 1
        nx = dd if prev_ else onsets[ix + 1]
        sv = _stroke(envl, env1, noise_lo, o, nx, dd if prev_ else nx - NEXT_GUARD_S, result[-1]["end"] if result else 0.0)
        if sv:
            result.append(sv)
    return result


def _event(first: dict, prev_: dict, prio: float) -> dict:
    return {"start": first["start"], "end": prev_["end"], "kind": "event", "priority": prio, "prom": prio}


def _joined(envl: numpy.ndarray, lhs: dict, rhs: dict) -> bool:
    return rhs["start"] - lhs["end"] < JOIN_GAP_S and not edit_gap(envl, lhs["start"], rhs["start"])


def pairs_of(envl: numpy.ndarray, stt: Sequence[dict]) -> list[dict]:
    """Events of 2 joined strokes, never led by a much weaker stroke (F5 step 6)."""
    return [_event(lhs, rhs, max(lhs["prom"], rhs["prom"])) for lhs, rhs in zip(stt, stt[1:])
            if _joined(envl, lhs, rhs) and rhs["end"] - lhs["start"] <= EVENT_MAX_S and lhs["prom"] >= rhs["prom"] - WEAK_LEAD_DB]


def chains_of(envl: numpy.ndarray, stt: Sequence[dict]) -> list[dict]:
    """Events of more than 2 joined strokes within 2 s (F5 step 7)."""
    result, ix = [], 0
    while ix < len(stt):
        jj = ix
        while jj + 1 < len(stt) and _joined(envl, stt[jj], stt[jj + 1]) and stt[jj + 1]["end"] - stt[ix]["start"] <= EVENT_MAX_S:
            jj += 1
        chain = list(stt[ix:jj + 1])
        best_n = max(sv["prom"] for sv in chain)
        while chain and chain[0]["prom"] < best_n - WEAK_LEAD_DB:
            chain.pop(0)
        if len(chain) > 2:
            result.append(_event(chain[0], chain[-1], best_n))
        ix = jj + 1
    return result


def _isolate(envl: numpy.ndarray, noise_lo: float, cand: dict) -> None:
    fv = int(math.floor(cand["start"] / FRAME_S))
    pre = envl[max(0, fv - SLOPE_FRAMES):max(0, fv)]
    if (not len(pre) or pre.mean() <= noise_lo + 10) and edge_levels(envl, cand["start"], cand["end"])[1] <= -25:
        cand["priority"] += ISOLATION_BONUS


def onset_candidates(mat: numpy.ndarray, envl: numpy.ndarray, noise_lo: float, dd: float) -> list[dict]:
    """Strokes, pairs and chains of the local audio, with the isolation bonus (F5)."""
    onsets = [float(o) for o in features.onsets(numpy.asarray(mat, numpy.float32), SR)]
    if not onsets:
        return []
    stt = strokes_of(envl, envelope(mat, ATTACK_FRAME), noise_lo, onsets, dd)
    pool = stt + pairs_of(envl, stt) + chains_of(envl, stt)
    for cand in pool:
        _isolate(envl, noise_lo, cand)
    return pool


# ================================================================ find: query kind and scoring


def query_kind(vec: voc.Vocab, qtext: numpy.ndarray) -> tuple[str, float]:
    """'event', 'texture' or 'either' from the query's text embedding (F2)."""
    ex = numpy.asarray(vec.embed_text(EVENT_ANCHORS), numpy.float64).mean(axis=0)
    tx = numpy.asarray(vec.embed_text(TEXTURE_ANCHORS), numpy.float64).mean(axis=0)
    score = float(numpy.asarray(qtext, numpy.float64) @ ex - numpy.asarray(qtext, numpy.float64) @ tx)
    kind = "event" if score >= KIND_EVENT else "texture" if score <= KIND_TEXTURE else "either"
    return kind, round(score, 3)


def score_cands(ctx: dict, pool: list[dict]) -> None:
    """Embed every candidate as one CLAP span and add its z and cosine."""
    if not pool:
        return
    sp = [(ctx["m0"] + cand["start"], ctx["m0"] + cand["end"]) for cand in pool]
    W = voc.span_embeddings([(ctx["path"], sp)])[0]
    cal = ctx["vocab"].calibrate(W, ctx["q"][None, :])
    for ix, cand in enumerate(pool):
        cand["z"], cand["cosine"] = round(float(cal.z_q[ix, 0]), 2), float(cal.Cq[ix, 0])


def _windows(ctx: dict, frame_w: float | None, hop: float | None) -> list[dict]:
    grid = window_grid(ctx["off"], ctx["d"], frame_w, hop)
    ctx["n_windows"], ctx["win_s"] = len(grid), grid[0][1] - grid[0][0]
    if ctx["prefer"]:
        grid = [snap_window(ctx["env"], ctx["floor"], lhs, rhs, ctx["off"], ctx["d"]) for lhs, rhs in grid]
    wins = [{"start": lhs, "end": rhs, "kind": "window", "priority": 0.0} for lhs, rhs in grid if rhs > lhs]
    return dedupe(wins) if ctx["prefer"] else wins


def _overlaps(lhs: dict, rhs: dict, tol: float = 0.0) -> bool:
    return lhs["start"] < rhs["end"] - tol and rhs["start"] < lhs["end"] - tol


def _events(ctx: dict, wins: list[dict]) -> list[dict]:
    pool = [cand for cand in onset_candidates(ctx["m"], ctx["env"], ctx["floor"], ctx["d"])
             if cand["start"] + PRE_ROLL >= ctx["off"] - 0.001]
    ctx["n_onset"] = len(pool)
    ctx["stages"] = 1 if ctx["s1"] - ctx["s0"] <= ONE_STAGE_S else 2
    if ctx["stages"] == 2:
        winner = sorted(wins, key=lambda win_n: -win_n["z"])[:BEST_WINDOWS]
        pool = [cand for cand in pool if any(_overlaps(cand, win_n) for win_n in winner)]
    picked = dedupe(round_robin(pool))
    score_cands(ctx, picked)
    return picked


def _judge(ctx: dict, pool: list[dict]) -> None:
    for cand in pool:
        cand["head_db"], cand["tail_db"] = edge_levels(ctx["env"], cand["start"], cand["end"])
        judged = ctx["use_events"] and (ctx["prefer"] or cand["kind"] != "window")
        heads = judged and cand["head_db"] > CUT_DB
        tails = judged and cand["tail_db"] > CUT_DB
        cand["cut"] = ", ".join(win_n for win_n, on in (("head cut", heads), ("tail cut", tails)) if on)
        cand["score"] = cand["z"] - CUT_PENALTY * bool(cand["cut"])


def _excluded(ctx: dict, win_n: dict, events: list[dict]) -> bool:
    inside = [ex["z"] for ex in events
              if ex["start"] >= win_n["start"] - INSIDE_TOL_S and ex["end"] <= win_n["end"] + INSIDE_TOL_S]
    if inside and max(inside) >= win_n["z"] - REFINE_Z:
        return True
    return bool(events) and multiple_sounds(ctx["env"], ctx["floor"], win_n["start"], win_n["end"])


def _local_peak(ctx: dict, cand: dict) -> float:
    lhs, rhs = int(round(cand["start"] * SR)), int(round(cand["end"] * SR))
    return voc.array_peak_db(ctx["x"][lhs:rhs])


def _select(ctx: dict, pool: list[dict], best_n: int) -> list[dict]:
    loud = voc.array_peak_db(ctx["x"]) >= NEAR_SILENT_DB
    taken: list[dict] = []
    for cand in sorted(pool, key=lambda cand: -cand["score"]):
        if len(taken) >= best_n:
            break
        if cand.get("excluded") or any(_overlaps(cand, tx, 0.001) for tx in taken):
            continue
        cand["peak_dbfs"] = _local_peak(ctx, cand)
        if not loud or cand["peak_dbfs"] >= NEAR_SILENT_DB:
            taken.append(cand)
    return taken


def _segment(ctx: dict, cand: dict) -> dict:
    sv, ex = ctx["m0"] + cand["start"], ctx["m0"] + cand["end"]
    return {"start": round(sv, 3), "end": round(ex, 3), "duration": round(ex - sv, 3), "score": round(cand["score"], 2),
            "z": cand["z"], "cosine": round(cand["cosine"], 3), "verdict": voc.verdict(cand["z"]), "kind": cand["kind"],
            "peak_dbfs": round(cand["peak_dbfs"], 1), "head_db": round(cand["head_db"], 1),
            "tail_db": round(cand["tail_db"], 1), "cut": cand["cut"]}


# ================================================================ find: driver


def _span_of(fpath: str, t_from: float | None, t_to: float | None) -> tuple[float, float, float]:
    """(searched start, searched end, load start with the margin)."""
    secs = voc.check_range(fpath, t_from, t_to)
    s0 = t_from or 0.0
    s1 = min(t_to, secs) if t_to is not None else secs
    return s0, s1, max(0.0, s0 - MARGIN_S)


def _query_read(prompt: str) -> dict:
    vec = voc.get()
    qtext = numpy.asarray(vec.embed_text([prompt])[0], numpy.float64)
    read, kscore = query_kind(vec, qtext)
    return {"vocab": vec, "q": qtext, "read": read, "kind_score": kscore}


def _plan(kind: str, read: str) -> dict:
    return {"use_events": kind == "event" or (kind == "auto" and read != "texture"),
            "prefer": kind == "event" or (kind == "auto" and read == "event")}


def _context(fpath: str, prompt: str, kind: str, t_from: float | None, t_to: float | None) -> dict:
    if kind not in KINDS:
        raise ValueError(f"kind must be one of {', '.join(KINDS)}")
    s0, s1, m0 = _span_of(fpath, t_from, t_to)
    sig = audio.load(fpath, start=m0, end=s1)
    mat = audio.to_mono(numpy.asarray(sig, numpy.float32))
    qr = _query_read(prompt)
    envl = envelope(mat)
    return {"path": fpath, "query": prompt, "kind": kind, "s0": s0, "s1": s1, "m0": m0, "off": s0 - m0, "x": sig,
            "m": mat, "d": len(mat) / SR, **qr, "env": envl, "floor": noise_floor(envl), **_plan(kind, qr["read"]),
            "n_onset": 0, "stages": 0}


def _candidates(ctx: dict, frame_w: float | None, hop: float | None) -> list[dict]:
    wins = _windows(ctx, frame_w, hop)
    score_cands(ctx, wins)
    events = _events(ctx, wins) if ctx["use_events"] else []
    ctx["dropped"] = ctx["kind"] == "event" and bool(events)
    if ctx["dropped"]:
        wins = []
    pool = events + wins
    _judge(ctx, pool)
    if ctx["prefer"]:
        for win_n in wins:
            win_n["excluded"] = _excluded(ctx, win_n, events)
    ctx["n_events"], ctx["n_candidates"] = len(events), len(pool)
    return pool


def find(path: str, query: str, win: float | None = None, hop: float | None = None, top: int = 5,
         start: float | None = None, end: float | None = None, kind: str = "auto") -> dict:
    """The best non-overlapping moments of one recording for a description, as trims."""
    query = query_text(query)
    ctx = _context(path, query, kind, start, end)
    segs = [_segment(ctx, cand) for cand in _select(ctx, _candidates(ctx, win, hop), top)]
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
    pieces = [f"Searched {where} ({how}) with {ctx['n_windows']} windows of about {ctx['win_s']:.1f} s"]
    if ctx["prefer"]:
        pieces.append("edges snapped to the sound")
    if ctx.get("dropped"):
        pieces.append("windows only located events")
    if ctx["use_events"]:
        stages = "one stage" if ctx["stages"] == 1 else "two stages: events inside the 6 best windows"
        pieces.append(f"{ctx['n_onset']} onset-based candidates, {stages}")
    return "; ".join(pieces) + "."


def _segments_text(segs: Sequence[dict]) -> str:
    items = [f"{report.fmt_t(sv['start'])}-{report.fmt_t(sv['end'])} ({sv['duration']:.3f} s, {sv['kind']}, "
             f"z {voc.zf(sv['z'])}, {sv['verdict']}{', ' + sv['cut'] if sv['cut'] else ''})" for sv in segs]
    return ("Best: " + "; ".join(items) + ". Strokes and events start 5 ms before the attack; windows are snapped "
            "to the sound or are plain stretches to crossfade; a head or tail cut trim cuts into a sound.")


def find_summary(ctx: dict, segs: Sequence[dict]) -> str:
    """The find paragraph (F11)."""
    pieces = [_how_text(ctx)]
    if segs:
        pieces.append(_segments_text(segs))
        if segs[0]["verdict"] in ("weak", voc.NO_MATCH):
            pieces.append("The recording may not contain this sound.")
    else:
        pieces.append("No candidate segments.")
    text = " ".join(pieces)
    peak = voc.array_peak_db(ctx["x"][int(round(ctx["off"] * SR)):])
    if peak < NEAR_SILENT_DB:
        text = f"Warning: the searched audio is near-silent (peak {peak:.0f} dBFS); scores mean nothing. " + text
    return text


def find_text(result: dict) -> str:
    """Text output of find."""
    lines = [result["summary"]]
    if result["segments"]:
        lines.append("")
    for ix, sv in enumerate(result["segments"], 1):
        cut = f"  score {sv['score']:+.2f} ({sv['cut']})" if sv["cut"] else ""
        lines.append(f"{ix:>2}. {sv['start']:.3f}-{sv['end']:.3f}  [{report.fmt_t(sv['start'])}]  {sv['duration']:.3f} s  "
                     f"z {voc.zf(sv['z'])}  {sv['verdict']}  {sv['kind']}  peak {sv['peak_dbfs']:.1f}  "
                     f"head {sv['head_db']:.0f} dB  tail {sv['tail_db']:.0f} dB{cut}")
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
    pth = sub.add_parser("rank", help="Rank files by how well they fit one description.")
    pth.add_argument("query", help="the description")
    pth.add_argument("paths", nargs="+", help="files, folders or globs")
    pth.add_argument("--top", type=int, default=None, help="list only the best N files")
    report.add_common(pth)
    pth.set_defaults(func=_run_rank)
    fv = sub.add_parser("find", help="The best moments of one recording for a description, as trims.")
    fv.add_argument("path", help="the recording")
    fv.add_argument("query", help="the description")
    fv.add_argument("--top", type=int, default=5, help="number of non-overlapping segments")
    fv.add_argument("--win", type=float, default=None, help="window length (s)")
    fv.add_argument("--hop", type=float, default=None, help="hop between windows (s)")
    fv.add_argument("--kind", choices=KINDS, default="auto", help="candidate kinds")
    fv.add_argument("--start", type=float, default=None)
    fv.add_argument("--end", type=float, default=None)
    report.add_common(fv)
    fv.set_defaults(func=_run_find)
