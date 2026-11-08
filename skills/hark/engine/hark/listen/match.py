"""match: how well a sound fits one or more text descriptions, as calibrated z with a verdict band.

Each query competes with every vocabulary term for this very sound. The verdict comes from z alone,
and the summary quotes the held-out evidence of that band.
"""

from __future__ import annotations

import argparse
import os
from collections.abc import Sequence
from typing import Any

import numpy

from hark.core import report
from hark.listen import vocabulary as voc

NEAR_SILENT_DB = -70.0
CLOSEST = 4
UNVALIDATED_MARK = " (unvalidated: music / instrument / voice description)"


def normalise_queries(prompt: str | Sequence[str] | None) -> list[str]:
    """A list of trimmed queries; a value error when it is empty or holds a blank item."""
    items = [prompt] if isinstance(prompt, str) else list(prompt or [])
    if not items or any(not isinstance(qtext, str) or not qtext.strip() for qtext in items):
        raise ValueError("no query given")
    return [qtext.strip() for qtext in items]


def _softmax_rows(cq: numpy.ndarray, n_q: int, scale: float) -> tuple[list[float | None], list[list[float]]]:
    """p_vs_alts per query and the alternative probabilities per query."""
    options = cq[n_q:]
    if len(options):
        records = [voc.softmax([cq[jj], *options], scale) for jj in range(n_q)]
        return [round(float(row_[0]), 3) for row_ in records], [[round(float(pth), 3) for pth in row_[1:]] for row_ in records]
    if n_q > 1:
        pth = voc.softmax(cq[:n_q], scale)
        return [round(float(sig), 3) for sig in pth], [[] for _ in range(n_q)]
    return [None], [[]]


def score_rows(vec: voc.Vocab, lhs: numpy.ndarray, prompts: list[str], options: list[str]) -> list[dict]:
    """One result row per query, sorted by z descending (alternatives share the calibration)."""
    texts = prompts + options
    cal = vec.calibrate(lhs[None, :], vec.embed_text(texts))
    recs = [vec._record(cal, 0, jj, tx) for jj, tx in enumerate(texts)]
    p_q, p_alts = _softmax_rows(cal.Cq[0], len(prompts), vec.logit_scale)
    records = []
    for jj, rec in enumerate(recs[:len(prompts)]):
        row = {**rec, "verdict": voc.verdict(rec["z"]), "validated": vec.validated(rec["query"]), "p_vs_alts": p_q[jj]}
        if options:
            row["alts"] = [{"query": row_["query"], "p": pth, "z": row_["z"], "cosine": row_["cosine"]}
                           for row_, pth in zip(recs[len(prompts):], p_alts[jj])]
        records.append(row)
    return sorted(records, key=lambda row_: -row_["z"])


def segment_text(t_from: float | None, t_to: float | None) -> str:
    """' [a-b s]', ' [from a s]' or ''."""
    if t_to is not None:
        return f" [{(t_from or 0.0):.3f}-{t_to:.3f} s]"
    if t_from:
        return f" [from {t_from:.3f} s]"
    return ""


def _row_text(row_: dict) -> str:
    mark = "" if row_["validated"] else UNVALIDATED_MARK
    return (f"'{row_['query']}' -> {row_['verdict']}{mark} (z {voc.zf(row_['z'])}; would rank #{row_['rank_in_vocab']} among "
            f"{row_['vocab_size']} vocabulary descriptions; cosine {row_['cosine']:.2f})")


def _choice_text(records: list[dict]) -> str:
    winner = records[0]
    if "alts" in winner and winner["p_vs_alts"] is not None:
        items = [(winner["query"], winner["p_vs_alts"])] + [(sig["query"], sig["p"]) for sig in winner["alts"]]
        return "Forced choice against the alternatives: " + ", ".join(
            f"'{qtext}' {voc.pct_text(pth)}" for qtext, pth in items) + "."
    if len(records) > 1:
        return "Forced choice among the queries: " + ", ".join(
            f"'{row_['query']}' {voc.pct_text(row_['p_vs_alts'])}" for row_ in records) + "."
    return ""


def _evidence_text(winner: dict) -> str:
    if winner["verdict"] == voc.NO_MATCH or not winner["validated"]:
        return ""
    ev = voc.VERDICT_EVIDENCE[winner["verdict"]]
    return (f"In a held-out library search with about 3% true pairs, a {winner['verdict']} verdict meant the right "
            f"kind of sound {round(ev['precision_library'] * 100)}% of the time and the exact variant "
            f"{round(ev['precision_library_strict'] * 100)}% (for the exact variant, a near class counts as a miss, "
            "for example a return key heard for a space bar).")


def _like_text(records: list[dict], closest: list[dict]) -> str:
    prompts = {row_["query"] for row_ in records}
    like = [cand for cand in closest if cand["term"] not in prompts]
    if not like:
        return ""
    return "It sounds most like: " + ", ".join(f"'{cand['term']}' (z {voc.zf(cand['z'])})" for cand in like) + "."


def build_summary(fpath: str, t_from: float | None, t_to: float | None, records: list[dict], closest: list[dict],
                  peak: float) -> str:
    """The match paragraph (surfaces-mcp M3.3)."""
    pieces = [f"{os.path.basename(fpath)}{segment_text(t_from, t_to)}: " + "; ".join(_row_text(row_) for row_ in records) + ".",
             _choice_text(records), _like_text(records, closest), _evidence_text(records[0])]
    text = " ".join(pth for pth in pieces if pth)
    if peak < NEAR_SILENT_DB:
        text = f"Warning: the input is near-silent (peak {peak:.0f} dBFS), so these scores mean nothing. " + text
    return text


BEST_KEYS = ("query", "cosine", "z", "percentile", "rank_in_vocab", "p_vs_alts", "verdict", "validated")


def _closest(vec: voc.Vocab, lhs: numpy.ndarray) -> list[dict]:
    return [{"term": tx, "category": cand, "z": z, "cosine": cos}
            for tx, cand, z, cos in vec.top_terms(lhs, CLOSEST, exclude=voc.NOT_SOUNDS_LIKE)]


def _alt_list(options: Sequence[str] | None) -> list[str]:
    return [lhs.strip() for lhs in (options or []) if isinstance(lhs, str) and lhs.strip()]


def match(path: str, query: str | Sequence[str], alts: Sequence[str] | None = None, start: float | None = None,
          end: float | None = None) -> dict:
    """Judge how well a sound fits each query."""
    prompts, alt_list = normalise_queries(query), _alt_list(alts)
    voc.check_range(path, start, end)
    vec = voc.get()
    lhs = voc.embed_file(path, start, end)
    records = score_rows(vec, numpy.asarray(lhs, numpy.float64), prompts, alt_list)
    closest = _closest(vec, lhs)
    peak = voc.peak_db(path, start, end)
    winner = records[0]
    result: dict[str, Any] = {
        "summary": build_summary(path, start, end, records, closest, peak), "path": path, "start": start, "end": end,
        "peak_dbfs": round(peak, 1), **{kk: winner[kk] for kk in BEST_KEYS}}
    if alt_list:
        result["alts"] = winner["alts"]
    result.update({"results": records, "closest_vocab": closest, "verdict_thresholds_z": dict(voc.VERDICT_THRESHOLDS_Z),
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
    pth = sub.add_parser("match", help="How well a sound fits one or more descriptions (calibrated z, verdict).")
    pth.add_argument("path", help="audio or video file")
    pth.add_argument("query", nargs="+", help="descriptions to judge")
    pth.add_argument("--alts", nargs="+", default=None, help="alternative descriptions for a forced choice")
    pth.add_argument("--start", type=float, default=None)
    pth.add_argument("--end", type=float, default=None)
    report.add_common(pth)
    pth.set_defaults(func=_run)
