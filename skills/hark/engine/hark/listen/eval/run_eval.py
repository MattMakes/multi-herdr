"""Held-out evaluation of the text-scoring commands (listen-eval). A developer tool, not a user command.

    python -m hark.listen.eval.run_eval embed --set tune|heldout|tuneref [--pad P] [--part i/n]
    python -m hark.listen.eval.run_eval stats | analyze [--templates t1|t3|t5] | heldout | kind | find [--pad P] | timing

Every sub-command writes one JSON result file (default: beside this script; ``--out DIR`` moves it) and
prints one line per result row. Result files hold only aggregates, file ids and base names.
"""

from __future__ import annotations

import argparse
import json
import shutil
import sys
import tempfile
import time
from collections.abc import Callable, Mapping, Sequence
from pathlib import Path

import numpy as np

from hark.core import audio
from hark.listen import search, vocabulary as voc
from hark.listen.eval import dataset as ds
from hark.listen.eval import metrics as mt

SPLITS = ("all", "dev", "test")
TEMPLATE_SETS = ("t1", "t3", "t5")


# ---------------------------------------------------------------- output


def write_result(out: Path, name: str, result: Mapping, rows: Sequence[str]) -> Path:
    """Write the result JSON and print one line per row."""
    out.mkdir(parents=True, exist_ok=True)
    path = out / name
    path.write_text(json.dumps(result, indent=1, default=_jsonable) + "\n", encoding="utf-8")
    for r in rows:
        print(r)
    return path


def _jsonable(v):
    if isinstance(v, np.generic):
        return v.item()
    if isinstance(v, np.ndarray):
        return v.tolist()
    raise TypeError(type(v).__name__)


def _r(v: float | None, n: int = 3) -> float | None:
    return None if v is None else round(float(v), n)


# ---------------------------------------------------------------- queries and templates


def templates_of(name: str) -> list[str]:
    """t1 = the phrase alone, t3 = the phrase plus 2 templates, t5 = the shipped 5 templates."""
    shipped = voc.read_vocab()["templates"]
    rest = [t for t in shipped if t != "{}"]
    return {"t1": ["{}"], "t3": ["{}"] + rest[:2], "t5": list(shipped)}[name]


def query_columns(queries: Mapping) -> tuple[list[str], dict[str, slice]]:
    """The query texts in E1 order and the column slice of each part."""
    cats = [queries["categories"][c] for c in mt.CATEGORIES]
    p1 = [c["phrasings"][0] for c in queries["classes"]]
    p2 = [c["phrasings"][1] for c in queries["classes"]]
    spec = [q["text"] for q in queries.get("specific", [])]
    n1, n2, n3 = len(cats), len(cats) + len(p1), len(cats) + 2 * len(p1)
    parts = {"cat": slice(0, n1), "p1": slice(n1, n2), "p2": slice(n2, n3), "spec": slice(n3, n3 + len(spec))}
    return cats + p1 + p2 + spec, parts


def class_ids(queries: Mapping) -> list[str]:
    return [c["id"] for c in queries["classes"]]


# ---------------------------------------------------------------- metrics of one scored library


def pooled_tagging(lib: ds.Library, U: np.ndarray, parts: Mapping[str, slice], classes: list[str]) -> dict:
    """E7 over both phrasing sets: accuracy and MRR as means over every (file, set) pair."""
    tags = [mt.tagging(U[:, parts[p]], lib.classes, classes) for p in ("p1", "p2")]
    n = sum(t["n"] for t in tags)
    if not n:
        return {"n": 0, "accuracy": None, "mrr": None}
    return {"n": n, **{k: sum(t["n"] * (t[k] or 0) for t in tags) / n for k in ("accuracy", "mrr")}}


def class_z(Z: np.ndarray, parts: Mapping[str, slice]) -> np.ndarray:
    """The 70 class columns: phrasing 1 of every class, then phrasing 2."""
    return np.concatenate([Z[:, parts["p1"]], Z[:, parts["p2"]]], axis=1)


def library_metrics(lib: ds.Library, U: np.ndarray, Z: np.ndarray, parts: Mapping[str, slice], classes: list[str],
                    queries: Mapping | None = None) -> dict:
    """E4, E5, E7, E8 (and E6 when ``queries`` is given) on one library."""
    zc = class_z(Z, parts)
    out = {"n": len(lib.paths), "category": mt.category_metrics(Z[:, parts["cat"]], lib.folders),
           "class": mt.class_metrics(zc, lib.classes, classes + classes),
           "tagging": pooled_tagging(lib, U, parts, classes)}
    for mode, lenient in (("auc", True), ("auc_strict", False)):
        L = mt.label_matrix(lib.classes, classes, lenient)
        out[mode] = mt.pooled_auc(zc, np.concatenate([L, L], axis=1))
    if queries and queries.get("specific"):
        out["specific"] = specific_metrics(lib, Z[:, parts["spec"]], queries["specific"])
    return out


def specific_metrics(lib: ds.Library, Z: np.ndarray, specific: Sequence[Mapping]) -> dict:
    """E6: top-1, top-3, MAP and the best file per specific query."""
    rows, aps, t1, t3 = [], [], [], []
    for j, q in enumerate(specific):
        rel = mt.specific_relevant(q, lib.ids, lib.classes)
        best = int(np.argmax(Z[:, j]))
        ap = mt.average_precision(Z[:, j], rel)
        rows.append({"query": q["text"], "ap": _r(ap), "best": Path(lib.paths[best]).name, "top1": bool(rel[best])})
        if ap is not None:
            aps.append(ap)
            t1.append(rel[best])
            t3.append(mt.top_hit(Z[:, j], rel, 3))
    return {"n": len(aps), "top1": int(sum(t1)), "top3": int(sum(t3)), "map": mt.mean_or_none(aps), "queries": rows}


def pooled(lib: ds.Library, Z: np.ndarray, parts: Mapping[str, slice], classes: list[str],
           lenient: bool) -> tuple[np.ndarray, np.ndarray]:
    L = mt.label_matrix(lib.classes, classes, lenient)
    return mt.pooled_pairs(class_z(Z, parts), np.concatenate([L, L], axis=1))


def bands_of(lib: ds.Library, Z: np.ndarray, parts: Mapping[str, slice], classes: list[str]) -> dict:
    """The band table (E10) for lenient and strict labels, plus the inspection grid."""
    out = {}
    for mode, lenient in (("lenient", True), ("strict", False)):
        pos, neg = pooled(lib, Z, parts, classes, lenient)
        out[mode] = mt.band_table(pos, neg)
        out[mode]["grid"] = mt.cumulative_grid(pos, neg)
    return out


# ---------------------------------------------------------------- embedding


def embed_set(paths: Sequence[str], pad: str, max_win: int | None = None) -> np.ndarray:
    return voc.embed_files(list(paths), pad=pad, max_win=max_win)


def ref_stats(R: np.ndarray, paths: Sequence[str], E: np.ndarray, Q: np.ndarray, uniform: bool = False) -> dict:
    """The reference mean and the term / query cosine stats for the calibration variants."""
    if uniform:
        w = np.full(len(paths), 1.0 / max(1, len(paths)))
        R = np.asarray(R, np.float64)
        return {"m": (w[:, None] * R).sum(axis=0)}
    m, mu_t, sd_t = voc.reference_stats(R, paths, E)
    _, mu_q, sd_q = voc.reference_stats(R, paths, Q)
    return {"m": m, "term": (mu_t, sd_t), "query": (mu_q, sd_q)}


def score(A: np.ndarray, E: np.ndarray, Q: np.ndarray, method: str, st: Mapping) -> tuple[np.ndarray, np.ndarray]:
    return mt.calibrate_variant(A, E, Q, method, st.get("m"), st.get("term"), st.get("query"))


def music_like(path: str) -> bool:
    """A video file, a music folder, a mix or a synth stem (the 'without music-like refs' variant)."""
    p = path.lower()
    return audio.probe(path).get("has_video", False) or any(k in p for k in ("music", "mix", "synth"))


# ---------------------------------------------------------------- commands


def cmd_embed(args, out: Path) -> None:
    i, n = parse_part(args.part)
    root = ds.location("HARK_EVAL_ASSETS")
    queries = ds.read_label("queries.json")
    if args.set == "tune":
        paths, max_win = ds.tuning_library(root, queries).paths, None
    elif args.set == "heldout":
        paths, max_win = ds.heldout_library(root, ds.read_label("heldout_labels.json")).paths, None
    else:
        paths, max_win = ds.heldout_library(root, ds.read_label("heldout_labels.json")).paths + ds.extra_refs(), \
            voc.REF_MAX_WIN
    part = paths[i - 1::n]
    t0 = time.time()
    embed_set(part, args.pad, max_win)
    write_result(out, f"embed_{args.set}_{args.pad}_{i}of{n}.json",
                 {"set": args.set, "pad": args.pad, "part": f"{i}/{n}", "files": len(part),
                  "seconds": round(time.time() - t0, 1)},
                 [f"embedded {len(part)} files of {args.set} ({args.pad}, part {i}/{n})"])


def parse_part(text: str) -> tuple[int, int]:
    """'i/n' with 1 <= i <= n."""
    try:
        i, n = (int(v) for v in text.split("/"))
    except ValueError:
        raise ds.EvalError(f"--part must be i/n, got {text!r}") from None
    if n < 1 or not 1 <= i <= n:
        raise ds.EvalError(f"--part {text}: i must be in 1..n")
    return i, n


def cmd_stats(args, out: Path) -> None:
    root = ds.location("HARK_EVAL_ASSETS")
    refs = ds.tuning_library(root, ds.read_label("queries.json")).paths + ds.extra_refs()
    stats = voc.build_stats(refs)
    write_result(out, "stats.json", {k: stats[k] for k in ("model", "pad", "n_ref", "n_folders", "reference_set")},
                 [f"statistics rebuilt from {stats['reference_set']}"])


def analyze_setup(templates_name: str) -> dict:
    """The tuning library, its references, the term and query embeddings of one template set."""
    root = ds.location("HARK_EVAL_ASSETS")
    queries = ds.read_label("queries.json")
    templates = templates_of(templates_name)
    terms = [p for c in voc.read_vocab()["categories"].values() for p in c]
    texts, parts = query_columns(queries)
    refs = ds.heldout_library(root, ds.read_label("heldout_labels.json")).paths + ds.extra_refs()
    return {"queries": queries, "tune": ds.tuning_library(root, queries), "refs": refs, "terms": terms,
            "E": voc.embed_phrases(terms, templates), "Q": voc.embed_phrases(texts, templates), "parts": parts,
            "classes": class_ids(queries)}


def analyze_pad(cx: Mapping, pad: str) -> tuple[list[dict], dict | None]:
    """Every calibration method for one pad; the shipped extras when this pad is the shipped one."""
    tune, refs = cx["tune"], cx["refs"]
    A = embed_set(tune.paths, pad)
    R = embed_set(refs, pad, voc.REF_MAX_WIN)
    st = ref_stats(R, refs, cx["E"], cx["Q"])
    cells, shipped = [], None
    for method in mt.METHODS:
        U, Z = score(A, cx["E"], cx["Q"], method, st)
        cells.append({"pad": pad, "method": method, "splits": {
            s: library_metrics(tune.split(s), *_rows(tune, s, U, Z), cx["parts"], cx["classes"],
                               cx["queries"] if s == "all" else None) for s in SPLITS}})
        if pad == voc.PAD and method == "both":
            shipped = shipped_extras(cx, A, R, Z)
    return cells, shipped


def cmd_analyze(args, out: Path) -> None:
    cx = analyze_setup(args.templates)
    result = {"templates": args.templates, "n_tune": len(cx["tune"].paths), "n_refs": len(cx["refs"]), "cells": []}
    for pad in voc.PADS:
        cells, shipped = analyze_pad(cx, pad)
        result["cells"] += cells
        if shipped is not None:
            result["shipped"] = shipped
    write_result(out, "results.json", result, [cell_row(c) for c in result["cells"]])


def _rows(lib: ds.Library, split: str, U: np.ndarray, Z: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    keep = np.array([split == "all" or mt.split_of(i) == split for i in lib.ids], bool)
    return U[keep], Z[keep]


def cell_row(cell: Mapping) -> str:
    a = cell["splits"]["all"]
    return (f"{cell['pad']:9s} {cell['method']:8s} cat MAP {_r(a['category']['map'])} class MAP "
            f"{_r(a['class']['map'])} top-3 {_r(a['class']['top3'])} tag {_r(a['tagging']['accuracy'])} "
            f"AUC {_r(a['auc'])} strict {_r(a['auc_strict'])}")


def shipped_extras(cx: Mapping, A: np.ndarray, R: np.ndarray, Z: np.ndarray) -> dict:
    """Thresholds (E9), bands (E10), reference sensitivity and the extreme term biases of the shipped cell."""
    tune, parts, classes, terms = cx["tune"], cx["parts"], cx["classes"], cx["terms"]
    pos, neg = pooled(tune, Z, parts, classes, True)
    bias = cx["E"].astype(np.float64) @ ref_stats(R, cx["refs"], cx["E"], cx["Q"])["m"]
    order = np.argsort(-bias)
    return {"thresholds": mt.pick_thresholds(pos, neg), "bands": bands_of(tune, Z, parts, classes),
            "reference_sensitivity": sensitivity(cx, A, R),
            "bias_top10": [[terms[i], _r(bias[i], 4)] for i in order[:10]],
            "bias_bottom5": [[terms[i], _r(bias[i], 4)] for i in order[-5:]]}


def sensitivity(cx: Mapping, A: np.ndarray, R: np.ndarray) -> dict:
    """dev / test AUC for each reference variant of the shipped cell."""
    tune, refs, E, Q = cx["tune"], cx["refs"], cx["E"], cx["Q"]
    keep = np.array([not music_like(p) for p in refs], bool)
    variants: dict[str, Callable[[], Mapping]] = {
        "folder_balanced": lambda: ref_stats(R, refs, E, Q),
        "uniform": lambda: ref_stats(R, refs, E, Q, uniform=True),
        "without_music_like": lambda: ref_stats(R[keep], [p for p, k in zip(refs, keep) if k], E, Q),
        "transductive": lambda: ref_stats(A, tune.paths, E, Q),
        "no_term_bias": lambda: {"m": np.zeros(A.shape[1])},
    }
    out = {}
    for name, make in variants.items():
        U, Z = score(A, E, Q, "both", make())
        out[name] = {s: _r(library_metrics(tune.split(s), *_rows(tune, s, U, Z), cx["parts"], cx["classes"])["auc"])
                     for s in ("dev", "test")}
    return out


def heldout_variants(v: voc.Vocab) -> tuple[list[tuple[str, voc.Vocab | None]], str | None]:
    """shipped, pre-registered (when its file exists) and raw cosine; a note when there is no pre-registered file."""
    pre = ds.HERE / "vocab_preregistered.json"
    if pre.exists():
        return [("shipped", v), ("preregistered", voc.Vocab(path=pre)), ("raw", None)], None
    return [("shipped", v), ("raw", None)], "no vocab_preregistered.json: the shipped vocabulary was written before any score"


def vocab_row(name: str, m: Mapping) -> str:
    return (f"{name:13s} cat MAP {_r(m['category']['map'])} class MAP {_r(m['class']['map'])} "
            f"tag {_r(m['tagging']['accuracy'])} AUC {_r(m['auc'])} strict {_r(m['auc_strict'])}")


def cmd_heldout(args, out: Path) -> None:
    root = ds.location("HARK_EVAL_ASSETS")
    queries = ds.read_label("queries.json")
    held = ds.heldout_library(root, ds.read_label("heldout_labels.json"))
    texts, parts = query_columns(queries)
    A = embed_set(held.paths, voc.PAD)
    v = voc.get()
    variants, note = heldout_variants(v)
    scored = {name: library_metrics(held, *heldout_scores(vocab or v, A, texts, raw=vocab is None), parts,
                                    class_ids(queries)) for name, vocab in variants}
    mood = mood_check(v, root, queries)
    result = {"n": len(held.paths), "with_class": sum(bool(c) for c in held.classes), "vocabularies": scored,
              "bands": bands_of(held, heldout_scores(v, A, texts)[1], parts, class_ids(queries)), "mood": mood}
    if note:
        result["note"] = note
    rows = [vocab_row(n, m) for n, m in scored.items()]
    rows.append(f"mood negative {mood['negative']}/{mood['n']} chance {_r(mood['chance'])}")
    write_result(out, "heldout.json", result, rows)


def heldout_scores(v: voc.Vocab, A: np.ndarray, texts: Sequence[str], raw: bool = False):
    Q = v.embed_text(texts)
    if raw:
        C = np.asarray(A, np.float64) @ np.asarray(Q, np.float64).T
        return C, C
    cal = v.calibrate(A, Q)
    return cal.Cq - v.bias(Q)[None, :], cal.z_q


def mood_check(v: voc.Vocab, root: Path, queries: Mapping) -> dict:
    """The top music_mood term by z of every music track in both libraries."""
    paths = [p for p, _ in ds.category_files(root, ("music",))] + \
        [p for p, _ in ds.category_files(root / ds.HELDOUT_DIR, ("music",))]
    moods = [i for i, c in enumerate(v.term_category) if c == "music_mood"]
    if not paths:
        return mt.mood_share([], len(moods))
    Z = v.calibrate(embed_set(paths, voc.PAD)).Z
    top = [v.terms[moods[int(np.argmax(row[moods]))]] for row in Z]
    return mt.mood_share(top, len(moods))


def kind_row(true: str, texts: Sequence[str], read: Callable[[str], str]) -> dict:
    """Counts of each read kind for texts of one true kind, and the costly errors."""
    reads = {t: read(t) for t in texts}
    wrong = "texture" if true == "event" else "event"
    counts = {k: sum(r == k for r in reads.values()) for k in ("event", "either", "texture")}
    return {"n": len(texts), **counts, "wrong": [t for t, r in reads.items() if r == wrong]}


def kind_counts(labels: Mapping, read: Callable[[str], str]) -> dict:
    """E11: per part and true kind, the counts of each read kind and the costly errors."""
    return {part: {true: kind_row(true, texts, read) for true, texts in kinds.items()}
            for part, kinds in labels.items() if not part.startswith("_")}


def cmd_kind(args, out: Path) -> None:
    v = voc.get()
    labels = ds.read_label("kind_queries.json")
    result = kind_counts(labels, lambda t: search.query_kind(v, v.embed_text([t])[0])[0])
    rows = [f"{p:5s} {k:7s} ({r['n']}): event {r['event']}, either {r['either']}, texture {r['texture']}"
            for p, ks in result.items() for k, r in ks.items()]
    write_result(out, "kind.json", result, rows)


def cmd_find(args, out: Path) -> None:
    from hark.listen.eval import find_checks

    root = ds.location("HARK_EVAL_ASSETS")
    result = find_checks.run_all(root / "find", out / "scenes", args.pad)
    rows = [f"{k}: {v.get('error') or find_checks.row_text(v)}" for k, v in result.items()]
    write_result(out, f"find_{args.pad}.json", result, rows)


def cmd_timing(args, out: Path) -> None:
    from hark.listen.eval import find_checks

    root = ds.location("HARK_EVAL_ASSETS")
    result = find_checks.timings(root, out / "scenes")
    write_result(out, "timing.json", result, [f"{k}: {v}" for k, v in result.items()])


def fresh_copy(path: str, folder: Path) -> str:
    """A copy of a file in a new temporary folder under ``folder`` (so no cache hit)."""
    folder.mkdir(parents=True, exist_ok=True)
    d = Path(tempfile.mkdtemp(dir=folder))
    return shutil.copy(path, d / Path(path).name)


# ---------------------------------------------------------------- CLI


COMMANDS = {"embed": cmd_embed, "stats": cmd_stats, "analyze": cmd_analyze, "heldout": cmd_heldout,
            "kind": cmd_kind, "find": cmd_find, "timing": cmd_timing}


def build_parser() -> argparse.ArgumentParser:
    ap = argparse.ArgumentParser(prog="python -m hark.listen.eval.run_eval", description=__doc__.splitlines()[0])
    ap.add_argument("--out", default=str(ds.HERE), help="folder for the result files (default: beside the script)")
    sub = ap.add_subparsers(dest="cmd", required=True)
    e = sub.add_parser("embed", help="embed part i of n of a set")
    e.add_argument("--set", choices=("tune", "heldout", "tuneref"), required=True)
    e.add_argument("--pad", choices=voc.PADS, default=voc.PAD)
    e.add_argument("--part", default="1/1")
    sub.add_parser("stats", help="rebuild the shipped reference statistics")
    a = sub.add_parser("analyze", help="every variant on the tuning library")
    a.add_argument("--templates", choices=TEMPLATE_SETS, default="t5")
    sub.add_parser("heldout", help="the shipped configuration on the held-out library")
    sub.add_parser("kind", help="the query-kind reader on the labelled texts")
    f = sub.add_parser("find", help="key-press, typing and scene checks of find")
    f.add_argument("--pad", choices=voc.PADS, default=voc.PAD)
    sub.add_parser("timing", help="wall-clock time per call")
    return ap


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        COMMANDS[args.cmd](args, Path(args.out))
    except ds.EvalError as exc:
        print(f"run_eval {args.cmd}: error: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
