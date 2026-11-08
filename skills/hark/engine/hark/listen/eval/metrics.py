"""Metrics of the held-out evaluation (listen-eval E2-E12), as pure functions over numbers.

Nothing here loads a model or a file, so every formula has a fixed-number test.
"""

from __future__ import annotations

import hashlib
import math
from collections.abc import Iterable, Mapping, Sequence

import numpy as np

GROUPS: dict[str, tuple[str, ...]] = {
    "G1": ("snap_lock", "toy_clicks", "latch", "lego", "switch", "soft_landing", "dominoes"),
    "G2": ("tiny_steps", "soft_landing", "cursor_click", "key_thock", "spacebar", "enter", "typing"),
    "G3": ("wind_grass", "tree", "grass_steps", "meadow", "tiny_crowd", "fruit_drop"),
    "G4": ("small_whoosh", "big_whoosh"),
    "G5": ("fruit_drop", "soft_landing"),
    "G6": ("note", "ting", "chime_sweep"),
    "G7": ("swell", "chime_sweep", "impact"),
    "G8": ("small_pop", "stretch_pop"),
    "G9": ("plunge", "underwater"),
    "G10": ("room_tone", "rain"),
}
CLASSES = ("room_tone", "rain", "meadow", "wind_grass", "tiny_steps", "soft_landing", "glass_squeak",
           "stretch_pop", "snap_lock", "small_pop", "grass_steps", "soil", "tree", "fruit_drop", "tiny_crowd",
           "key_thock", "spacebar", "typing", "enter", "cursor_click", "ting", "chime_sweep", "note",
           "small_whoosh", "big_whoosh", "plunge", "underwater", "swell", "impact", "music", "toy_clicks",
           "lego", "dominoes", "latch", "switch")
CATEGORIES = ("ambience", "foley", "garden", "keyboard", "magic", "motion", "music", "toy")
VERDICT_MIN_LR = (("strong", 100.0), ("plausible", 20.0), ("weak", 5.0))
THRESHOLD_GRID = tuple(6.0 - 0.5 * i for i in range(13))
INSPECTION_GRID = tuple(0.25 * i for i in range(25))
BAND_CUTS = (3.0, 2.5, 2.0)
BASE_RATE = 0.03
KEY_LEAD_MAX_S = 0.060
KEY_END_SLACK_S = 0.005
ATTACK_LEVEL = 0.1
SCENE_SLACK_S = 0.050
NEGATIVE_MOODS = ("dark and tense music", "sad and melancholic music", "mysterious music", "aggressive music",
                  "suspenseful build-up music")


# ---------------------------------------------------------------- data split and labels


def split_of(file_id: str) -> str:
    """'dev' when SHA-1 of the UTF-8 id, read as an integer, is even, else 'test' (rule 2)."""
    return "dev" if int(hashlib.sha1(file_id.encode("utf-8")).hexdigest(), 16) % 2 == 0 else "test"


def left_out(classes: Iterable[str]) -> set[str]:
    """The union of every confusable group that holds one of the classes, without the classes themselves."""
    own = set(classes)
    out: set[str] = set()
    for members in GROUPS.values():
        if own & set(members):
            out |= set(members)
    return out - own


def label_row(classes: Iterable[str], columns: Sequence[str], lenient: bool) -> list[int]:
    """+1 own class, 0 no class or (lenient) confusable, -1 wrong (E8 step 1)."""
    own = set(classes)
    if not own:
        return [0] * len(columns)
    skip = left_out(own) if lenient else set()
    return [1 if c in own else 0 if c in skip else -1 for c in columns]


def label_matrix(file_classes: Sequence[Iterable[str]], columns: Sequence[str] = CLASSES,
                 lenient: bool = True) -> np.ndarray:
    """The files x classes label matrix of E8."""
    return np.array([label_row(c, columns, lenient) for c in file_classes], dtype=np.int8).reshape(
        len(file_classes), len(columns))


# ---------------------------------------------------------------- ranking metrics


def average_precision(scores: Sequence[float], relevant: Sequence[bool]) -> float | None:
    """AP over the relevant files, best score first (E2); None without a relevant file."""
    order = np.argsort(-np.asarray(scores, np.float64), kind="stable")
    rel = np.asarray(relevant, bool)[order]
    if not rel.any():
        return None
    hits = np.cumsum(rel)
    pos = np.flatnonzero(rel) + 1
    return float(np.mean(hits[pos - 1] / pos))


def precision_at(scores: Sequence[float], relevant: Sequence[bool], k: int) -> float:
    """The share of relevant files among the top k (E3)."""
    order = np.argsort(-np.asarray(scores, np.float64), kind="stable")[:k]
    return float(np.asarray(relevant, bool)[order].sum() / k)


def top_hit(scores: Sequence[float], relevant: Sequence[bool], k: int) -> bool:
    """True when a relevant file is in the top k."""
    order = np.argsort(-np.asarray(scores, np.float64), kind="stable")[:k]
    return bool(np.asarray(relevant, bool)[order].any())


def mean_or_none(values: Iterable[float | None]) -> float | None:
    """The mean of the values that are not None (None when none is left)."""
    v = [x for x in values if x is not None]
    return float(np.mean(v)) if v else None


def auc_rank_sum(pos: Sequence[float], neg: Sequence[float]) -> float | None:
    """AUC by rank sum, ties broken by sort order (E8 step 4); None without positives or negatives."""
    n_pos, n_neg = len(pos), len(neg)
    if not n_pos or not n_neg:
        return None
    values = np.concatenate([np.asarray(neg, np.float64), np.asarray(pos, np.float64)])
    ranks = np.empty(len(values))
    ranks[np.argsort(values, kind="stable")] = np.arange(1, len(values) + 1)
    r_pos = ranks[n_neg:].sum()
    return float((r_pos - n_pos * (n_pos + 1) / 2) / (n_pos * n_neg))


def auc_pairwise(pos: Sequence[float], neg: Sequence[float]) -> float | None:
    """AUC over every (positive, negative) pair, ties count half (E13)."""
    if not len(pos) or not len(neg):
        return None
    p = np.asarray(pos, np.float64)[:, None]
    n = np.asarray(neg, np.float64)[None, :]
    return float(((p > n).sum() + 0.5 * (p == n).sum()) / (p.size * n.size))


def pooled_pairs(z: np.ndarray, labels: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """The z values of the true and the wrong pairs (labels +1 / -1; 0 is left out)."""
    z, labels = np.asarray(z, np.float64), np.asarray(labels)
    return z[labels == 1], z[labels == -1]


def pooled_auc(z: np.ndarray, labels: np.ndarray) -> float | None:
    """The pooled AUC of E8."""
    return auc_rank_sum(*pooled_pairs(z, labels))


def tag_rank(u_row: Sequence[float], own: Iterable[int]) -> int:
    """1 + the best 0-based position of an own class when the row is sorted by u (E7)."""
    order = list(np.argsort(-np.asarray(u_row, np.float64), kind="stable"))
    return 1 + min(order.index(i) for i in own)


def tagging(U: np.ndarray, file_classes: Sequence[Iterable[str]], columns: Sequence[str] = CLASSES) -> dict:
    """Accuracy and MRR of E7 for one phrasing set (U: files x len(columns) bias-free scores)."""
    ranks = []
    for row, classes in zip(np.asarray(U, np.float64), file_classes):
        own = [columns.index(c) for c in classes if c in columns]
        if own:
            ranks.append(tag_rank(row, own))
    if not ranks:
        return {"n": 0, "accuracy": None, "mrr": None}
    r = np.asarray(ranks, np.float64)
    return {"n": len(r), "accuracy": float(np.mean(r == 1)), "mrr": float(np.mean(1 / r))}


# ---------------------------------------------------------------- verdict thresholds and bands


def band_counts(pos: np.ndarray, neg: np.ndarray, lo: float, hi: float) -> tuple[int, int]:
    """The true and wrong pairs with lo <= z < hi."""
    pos, neg = np.asarray(pos, np.float64), np.asarray(neg, np.float64)
    return int(((pos >= lo) & (pos < hi)).sum()), int(((neg >= lo) & (neg < hi)).sum())


def lr_of(tp: int, fp: int, n_pos: int, n_neg: int) -> float:
    """(tp / n+) / (fp / n-); +inf when fp = 0 and tp > 0, 0 when both are 0."""
    if fp == 0:
        return math.inf if tp > 0 else 0.0
    return (tp / max(1, n_pos)) / (fp / max(1, n_neg))


def band_lr(pos: np.ndarray, neg: np.ndarray, lo: float, hi: float) -> float:
    """The band likelihood ratio of E9."""
    tp, fp = band_counts(pos, neg, lo, hi)
    return lr_of(tp, fp, len(pos), len(neg))


def pick_thresholds(pos: np.ndarray, neg: np.ndarray, grid: Sequence[float] = THRESHOLD_GRID,
                    minimums: Sequence[tuple[str, float]] = VERDICT_MIN_LR) -> dict[str, float | None]:
    """The verdict threshold rule of E9, from the top down."""
    out: dict[str, float | None] = {}
    hi = math.inf
    for name, need in minimums:
        pick = None
        for t in grid:
            if t >= hi:
                continue
            if band_lr(pos, neg, t, hi) >= need:
                pick = t
            elif pick is not None:
                break
        out[name] = pick
        if pick is not None:
            hi = pick
    return out


def precision_at_base(lr: float, base: float = BASE_RATE) -> float:
    """Precision at a library base rate b: LR b / (LR b + (1 - b))."""
    if math.isinf(lr):
        return 1.0
    return lr * base / (lr * base + (1 - base))


def band_row(name: str, pos: np.ndarray, neg: np.ndarray, lo: float, hi: float) -> dict:
    """One row of the band table."""
    tp, fp = band_counts(pos, neg, lo, hi)
    lr = lr_of(tp, fp, len(pos), len(neg))
    return {"band": name, "lo": None if math.isinf(lo) else lo, "tp": tp, "fp": fp,
            "lr": None if fp == 0 else round(lr, 2), "precision": tp / max(1, tp + fp),
            "precision_even_odds": 1.0 if math.isinf(lr) else lr / (lr + 1),
            "recall_at_or_above": float((pos >= lo).sum() / max(1, len(pos)))}


def band_table(pos: np.ndarray, neg: np.ndarray, cuts: Sequence[float] = BAND_CUTS) -> dict:
    """Per-band tp, fp, LR and precisions, not cumulative (E10)."""
    pos, neg = np.asarray(pos, np.float64), np.asarray(neg, np.float64)
    edges = [math.inf, *cuts, -math.inf]
    names = ("strong", "plausible", "weak", "no match")
    bands = [band_row(name, pos, neg, edges[i + 1], edges[i]) for i, name in enumerate(names)]
    n_pos, n_neg = len(pos), len(neg)
    return {"n_pos": n_pos, "n_neg": n_neg, "base_rate": n_pos / max(1, n_pos + n_neg), "bands": bands}


def cumulative_grid(pos: np.ndarray, neg: np.ndarray, grid: Sequence[float] = INSPECTION_GRID) -> list[dict]:
    """tp, fp, recall, false-positive rate, cumulative LR and precision of z >= t (inspection only)."""
    pos, neg = np.asarray(pos, np.float64), np.asarray(neg, np.float64)
    rows = []
    for t in grid:
        tp, fp = int((pos >= t).sum()), int((neg >= t).sum())
        lr = lr_of(tp, fp, len(pos), len(neg))
        rows.append({"t": t, "tp": tp, "fp": fp, "recall": tp / max(1, len(pos)), "fpr": fp / max(1, len(neg)),
                     "lr": None if fp == 0 else round(lr, 2), "precision": tp / max(1, tp + fp)})
    return rows


# ---------------------------------------------------------------- query metrics on a score matrix


def category_metrics(scores: np.ndarray, folders: Sequence[str], categories: Sequence[str] = CATEGORIES) -> dict:
    """E4: MAP, mean P@5, mean P@10 and AP per category (scores: files x categories)."""
    ap, p5, p10 = {}, [], []
    for j, cat in enumerate(categories):
        rel = [f == cat for f in folders]
        ap[cat] = average_precision(scores[:, j], rel)
        if any(rel):
            p5.append(precision_at(scores[:, j], rel, 5))
            p10.append(precision_at(scores[:, j], rel, 10))
    return {"map": mean_or_none(ap.values()), "p5": mean_or_none(p5), "p10": mean_or_none(p10), "ap": ap}


def class_metrics(scores: np.ndarray, file_classes: Sequence[Iterable[str]], columns: Sequence[str]) -> dict:
    """E5: class MAP and top-3 over the phrasing columns whose class has a file in the set."""
    sets = [set(c) for c in file_classes]
    aps, top3 = [], []
    for j, cls in enumerate(columns):
        rel = [cls in s for s in sets]
        if not any(rel):
            continue
        aps.append(average_precision(scores[:, j], rel))
        top3.append(top_hit(scores[:, j], rel, 3))
    return {"map": mean_or_none(aps), "top3": mean_or_none(top3), "n_phrasings": len(aps)}


def specific_relevant(query: Mapping, file_ids: Sequence[str], file_classes: Sequence[Iterable[str]]) -> list[bool]:
    """E6: class overlap, or id in the list, or id with one of the prefixes."""
    classes, ids, prefixes = set(query.get("classes", ())), set(query.get("ids", ())), tuple(query.get("prefixes", ()))
    return [bool(classes & set(c)) or fid in ids or (bool(prefixes) and fid.startswith(prefixes))
            for fid, c in zip(file_ids, file_classes)]


def mood_share(top_moods: Sequence[str], n_mood_terms: int, negatives: Sequence[str] = NEGATIVE_MOODS) -> dict:
    """The share of tracks whose top mood is negative, and the chance share."""
    n = len(top_moods)
    neg = sum(m in negatives for m in top_moods)
    return {"n": n, "negative": neg, "share": neg / n if n else None, "chance": len(negatives) / n_mood_terms}


# ---------------------------------------------------------------- find checks


def attack_time(x: np.ndarray, start_s: float, sr: int, level: float = ATTACK_LEVEL) -> float:
    """The time of the first sample with |x| >= level * peak of the slice (E12 step 1)."""
    a = np.abs(np.asarray(x, np.float64))
    if a.size == 0 or a.max() <= 0:
        return start_s
    return start_s + int(np.argmax(a >= level * a.max())) / sr


def key_hit(segment: tuple[float, float], attacks: Sequence[float]) -> tuple[bool, int, float]:
    """(hit, index of the nearest press, lead) of one segment (E12 steps 2-3)."""
    start, end = segment
    idx = int(np.argmin([abs(a - start) for a in attacks]))
    lead = attacks[idx] - start
    nxt = attacks[idx + 1] if idx + 1 < len(attacks) else math.inf
    ok = -1e-9 <= lead <= KEY_LEAD_MAX_S + 1e-9 and end <= nxt + KEY_END_SLACK_S + 1e-9
    return ok, idx, lead


def _inside(t: float, ranges: Sequence[tuple[float, float]]) -> bool:
    return any(a <= t < b for a, b in ranges)


def lead_stats(leads: Sequence[float]) -> dict:
    """The median and range of the leads in ms (None without a lead)."""
    if not leads:
        return {"lead_median_ms": None, "lead_range_ms": None}
    ms = 1000 * np.asarray(leads, np.float64)
    return {"lead_median_ms": round(float(np.median(ms)), 1),
            "lead_range_ms": [round(float(ms.min()), 1), round(float(ms.max()), 1)]}


def _segment_class(start: float, unlabelled: Sequence[tuple[float, float]],
                   skips: Sequence[tuple[float, float]]) -> str:
    if _inside(start, unlabelled):
        return "unlabelled"
    return "skip" if _inside(start, skips) else "judged"


def key_report(segments: Sequence[tuple[float, float]], attacks: Sequence[float],
               unlabelled: Sequence[tuple[float, float]] = (), skips: Sequence[tuple[float, float]] = ()) -> dict:
    """Hits, distinct presses, lead stats, unlabelled and skip-range segments (E12)."""
    kinds = [_segment_class(s[0], unlabelled, skips) for s in segments]
    judged = [key_hit(s, attacks) for s, k in zip(segments, kinds) if k == "judged"]
    hits = [(idx, lead) for ok, idx, lead in judged if ok]
    return {"segments": len(segments), "hits": len(hits), "presses_hit": len({i for i, _ in hits}),
            **lead_stats([lead for _, lead in hits]), "unlabelled": kinds.count("unlabelled"),
            "in_skip": kinds.count("skip")}


def _hit_events(seg: Mapping, events: Sequence[float]) -> list[float]:
    return [e for e in events if seg["start"] - SCENE_SLACK_S <= e < seg["end"]]


def scene_found(segments: Sequence[Mapping], events: Sequence[float]) -> dict:
    """Which events a find result found, top-1 and tight trims (E12 scene rule)."""
    found: set[float] = set()
    leads = []
    for seg in segments:
        hit = _hit_events(seg, events)
        new = [e for e in hit if e not in found]
        found.update(hit)
        if new and seg.get("kind") != "window":
            leads.append(new[0] + 0.010 - seg["start"])
    top1 = bool(segments) and bool(_hit_events(segments[0], events))
    return {"found": sorted(found), "n_found": len(found), "top1": top1, "tight": len(leads),
            "leads_ms": [round(1000 * v, 1) for v in leads]}


# ---------------------------------------------------------------- calibration variants (E1)

METHODS = ("raw", "sound", "center", "both", "term_sd", "both_sd")


def calibrate_variant(A: np.ndarray, E: np.ndarray, Q: np.ndarray, method: str, m: np.ndarray | None = None,
                      term: tuple[np.ndarray, np.ndarray] | None = None,
                      query: tuple[np.ndarray, np.ndarray] | None = None) -> tuple[np.ndarray, np.ndarray]:
    """(u_q, z_q) of the queries Q for the sounds A under one calibration method.

    ``m`` is the reference mean embedding (center, both). ``term`` and ``query`` are the (mean, std) of the
    term and query cosines over the references (term_sd, both_sd).
    """
    if method not in METHODS:
        raise ValueError(f"unknown calibration method {method!r}")
    A, E, Q = (np.atleast_2d(np.asarray(v, np.float64)) for v in (A, E, Q))
    C, Cq = A @ E.T, A @ Q.T
    if method in ("center", "both"):
        m = np.asarray(m, np.float64)
        C, Cq = C - (E @ m)[None, :], Cq - (Q @ m)[None, :]
    elif method in ("term_sd", "both_sd"):
        (mu_t, sd_t), (mu_q, sd_q) = term, query
        C = (C - mu_t[None, :]) / (sd_t[None, :] + 1e-9)
        Cq = (Cq - mu_q[None, :]) / (sd_q[None, :] + 1e-9)
    if method in ("sound", "both", "both_sd"):
        mu, sd = C.mean(axis=1, keepdims=True), C.std(axis=1, keepdims=True) + 1e-9
        return Cq, (Cq - mu) / sd
    return Cq, Cq.copy()
