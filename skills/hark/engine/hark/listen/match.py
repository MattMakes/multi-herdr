"""match: how well a sound fits one or more text descriptions, as calibrated z with a verdict band.

Each query competes with every vocabulary term for this very sound. The verdict comes from z alone,
and the summary quotes the held-out evidence of that band.
"""

from __future__ import annotations

import argparse
import os
from collections.abc import Sequence
from typing import Any

import numpy as np

from hark.core import report
from hark.listen import vocabulary as voc

NEAR_SILENT_DB = -70.0
CLOSEST = 4
UNVALIDATED_MARK = " (unvalidated: music / instrument / voice description)"


def normalise_queries(query: str | Sequence[str] | None) -> list[str]:
    """A list of trimmed queries; a value error when it is empty or holds a blank item."""
    items = [query] if isinstance(query, str) else list(query or [])
    if not items or any(not isinstance(q, str) or not q.strip() for q in items):
        raise ValueError("no query given")
    return [q.strip() for q in items]


def _softmax_rows(cq: np.ndarray, n_q: int, scale: float) -> tuple[list[float | None], list[list[float]]]:
    """p_vs_alts per query and the alternative probabilities per query."""
    alts = cq[n_q:]
    if len(alts):
        rows = [voc.softmax([cq[j], *alts], scale) for j in range(n_q)]
        return [round(float(r[0]), 3) for r in rows], [[round(float(p), 3) for p in r[1:]] for r in rows]
    if n_q > 1:
        p = voc.softmax(cq[:n_q], scale)
        return [round(float(x), 3) for x in p], [[] for _ in range(n_q)]
    return [None], [[]]


def score_rows(v: voc.Vocab, a: np.ndarray, queries: list[str], alts: list[str]) -> list[dict]:
    """One result row per query, sorted by z descending (alternatives share the calibration)."""
    texts = queries + alts
    cal = v.calibrate(a[None, :], v.embed_text(texts))
    recs = [v._record(cal, 0, j, t) for j, t in enumerate(texts)]
    p_q, p_alts = _softmax_rows(cal.Cq[0], len(queries), v.logit_scale)
    rows = []
    for j, rec in enumerate(recs[:len(queries)]):
        row = {**rec, "verdict": voc.verdict(rec["z"]), "validated": v.validated(rec["query"]), "p_vs_alts": p_q[j]}
        if alts:
            row["alts"] = [{"query": r["query"], "p": p, "z": r["z"], "cosine": r["cosine"]}
                           for r, p in zip(recs[len(queries):], p_alts[j])]
        rows.append(row)
    return sorted(rows, key=lambda r: -r["z"])


def segment_text(start: float | None, end: float | None) -> str:
    """' [a-b s]', ' [from a s]' or ''."""
    if end is not None:
        return f" [{(start or 0.0):.3f}-{end:.3f} s]"
    if start:
        return f" [from {start:.3f} s]"
    return ""


def _row_text(r: dict) -> str:
    mark = "" if r["validated"] else UNVALIDATED_MARK
    return (f"'{r['query']}' -> {r['verdict']}{mark} (z {voc.zf(r['z'])}; would rank #{r['rank_in_vocab']} among "
            f"{r['vocab_size']} vocabulary descriptions; cosine {r['cosine']:.2f})")


def _choice_text(rows: list[dict]) -> str:
    best = rows[0]
    if "alts" in best and best["p_vs_alts"] is not None:
        items = [(best["query"], best["p_vs_alts"])] + [(x["query"], x["p"]) for x in best["alts"]]
        return "Forced choice against the alternatives: " + ", ".join(
            f"'{q}' {voc.pct_text(p)}" for q, p in items) + "."
    if len(rows) > 1:
        return "Forced choice among the queries: " + ", ".join(
            f"'{r['query']}' {voc.pct_text(r['p_vs_alts'])}" for r in rows) + "."
    return ""


def _evidence_text(best: dict) -> str:
    if best["verdict"] == voc.NO_MATCH or not best["validated"]:
        return ""
    ev = voc.VERDICT_EVIDENCE[best["verdict"]]
    return (f"In a held-out library search with about 3% true pairs, a {best['verdict']} verdict meant the right "
            f"kind of sound {round(ev['precision_library'] * 100)}% of the time and the exact variant "
            f"{round(ev['precision_library_strict'] * 100)}% (for the exact variant, a near class counts as a miss, "
            "for example a return key heard for a space bar).")


def build_summary(path: str, start: float | None, end: float | None, rows: list[dict], closest: list[dict],
                  peak: float) -> str:
    """The match paragraph (surfaces-mcp M3.3)."""
    parts = [f"{os.path.basename(path)}{segment_text(start, end)}: " + "; ".join(_row_text(r) for r in rows) + "."]
    parts.append(_choice_text(rows))
    queries = {r["query"] for r in rows}
    like = [c for c in closest if c["term"] not in queries]
    if like:
        parts.append("It sounds most like: " + ", ".join(f"'{c['term']}' (z {voc.zf(c['z'])})" for c in like) + ".")
    parts.append(_evidence_text(rows[0]))
    text = " ".join(p for p in parts if p)
    if peak < NEAR_SILENT_DB:
        text = f"Warning: the input is near-silent (peak {peak:.0f} dBFS), so these scores mean nothing. " + text
    return text


def match(path: str, query: str | Sequence[str], alts: Sequence[str] | None = None, start: float | None = None,
          end: float | None = None) -> dict:
    """Judge how well a sound fits each query."""
    queries = normalise_queries(query)
    alt_list = [a.strip() for a in (alts or []) if isinstance(a, str) and a.strip()]
    voc.check_range(path, start, end)
    v = voc.get()
    a = voc.embed_file(path, start, end)
    rows = score_rows(v, np.asarray(a, np.float64), queries, alt_list)
    closest = [{"term": t, "category": c, "z": z, "cosine": cos}
               for t, c, z, cos in v.top_terms(a, CLOSEST, exclude=voc.NOT_SOUNDS_LIKE)]
    peak = voc.peak_db(path, start, end)
    best = rows[0]
    result: dict[str, Any] = {
        "summary": build_summary(path, start, end, rows, closest, peak), "path": path, "start": start, "end": end,
        "peak_dbfs": round(peak, 1), "query": best["query"], "cosine": best["cosine"], "z": best["z"],
        "percentile": best["percentile"], "rank_in_vocab": best["rank_in_vocab"], "p_vs_alts": best["p_vs_alts"],
        "verdict": best["verdict"], "validated": best["validated"]}
    if alt_list:
        result["alts"] = best["alts"]
    result.update({"results": rows, "closest_vocab": closest, "verdict_thresholds_z": dict(voc.VERDICT_THRESHOLDS_Z),
                   "verdict_evidence": voc.VERDICT_EVIDENCE})
    return result


@report.expected_errors("match")
def _run(args: argparse.Namespace) -> int | None:
    import hark.models

    hark.models.require_ml("match")
    result = match(args.path, args.query, alts=args.alts, start=args.start, end=args.end)
    report.emit(result, args, text=result["summary"])
    return 0


def add_cli(sub: argparse._SubParsersAction) -> None:
    """Register ``match``."""
    p = sub.add_parser("match", help="How well a sound fits one or more descriptions (calibrated z, verdict).")
    p.add_argument("path", help="audio or video file")
    p.add_argument("query", nargs="+", help="descriptions to judge")
    p.add_argument("--alts", nargs="+", default=None, help="alternative descriptions for a forced choice")
    p.add_argument("--start", type=float, default=None)
    p.add_argument("--end", type=float, default=None)
    report.add_common(p)
    p.set_defaults(func=_run)
