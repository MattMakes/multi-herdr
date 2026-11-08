"""The real-effects check (listen-eval E13): does the model hear real one-shot effects as what they are?

    python -m hark.listen.eval.real_effects [library.json] [--out DIR]

The library JSON (default: ``HARK_EVAL_SFX_LIBRARY``), file paths relative to it:

    {"jobs": {"<job>": {"description": "<filed text>", "phrasings": ["<sound only>", "<sound only>"]}},
     "sounds": [{"file": "a.wav", "duration": 0.4, "title": "Cork Pop", "jobs": ["<job>", ...]}]}

It is not a realness test.
"""

from __future__ import annotations

import argparse
import json
import sys
from collections.abc import Mapping, Sequence
from pathlib import Path

import numpy as np

from hark.listen import realness, vocabulary as voc
from hark.listen.eval import dataset as ds
from hark.listen.eval import metrics as mt
from hark.listen.eval.run_eval import write_result

PASS_Z = 2.0
CHEAP_TEXTS = realness.SYNTH_PROMPTS[:4]
WAYS = ("job", "sound_only", "union")


def verdict_counts(z: Sequence[float]) -> dict:
    out = {k: 0 for k in (*voc.VERDICT_THRESHOLDS_Z, voc.NO_MATCH)}
    for v in z:
        out[voc.verdict(float(v))] += 1
    return out


def title_stats(Z: np.ndarray, titles: Sequence[str], keys: Sequence[str]) -> dict:
    """Own-title verdicts, median, AUC against other titles, top-1 and top-5 (Z: sounds x unique titles)."""
    own_z, pos, neg, top1, top5 = [], [], [], 0, 0
    for i, t in enumerate(titles):
        own = [j for j, k in enumerate(keys) if k == t]
        z = Z[i, own[0]]
        other = np.delete(Z[i], own)
        own_z.append(z)
        pos.append(z)
        neg.extend(other.tolist())
        beat = int((other > z).sum())
        top1 += beat < 1
        top5 += beat < 5
    return {"verdicts": verdict_counts(own_z), "median_z": round(float(np.median(own_z)), 2),
            "auc": mt.auc_pairwise(pos, neg), "top1": top1, "top5": top5}


def way_stats(Z: np.ndarray, C: np.ndarray, filed: Sequence[set[int]], cheap: np.ndarray, scale: float) -> dict:
    """Own-job verdicts, AUC own against other jobs, other jobs passed and cheap-synth wins (Z, C: sounds x jobs)."""
    own_z, pos, neg, passed, cheap_won = [], [], [], [], 0
    for i, jobs in enumerate(filed):
        if not jobs:
            continue
        own = max(jobs, key=lambda j: Z[i, j])
        others = [j for j in range(Z.shape[1]) if j not in jobs]
        own_z.append(Z[i, own])
        pos.append(Z[i, own])
        neg.extend(Z[i, others].tolist())
        if others:
            passed.append(float(np.mean(Z[i, others] >= PASS_Z)))
        p = voc.softmax([C[i, own], *cheap[i]], scale)
        cheap_won += int(np.argmax(p) > 0)
    return {"n": len(own_z), "verdicts": verdict_counts(own_z),
            "median_z": round(float(np.median(own_z)), 2) if own_z else None,
            "auc": mt.auc_pairwise(pos, neg), "other_jobs_passed": mt.mean_or_none(passed), "cheap_won": cheap_won}


def best_over(Zt: np.ndarray, Ct: np.ndarray, groups: Sequence[Sequence[int]]) -> tuple[np.ndarray, np.ndarray]:
    """Per job, the best z over its texts and the cosine of that text."""
    Z = np.zeros((Zt.shape[0], len(groups)))
    C = np.zeros_like(Z)
    for j, cols in enumerate(groups):
        pick = np.asarray(cols)[np.argmax(Zt[:, cols], axis=1)]
        Z[:, j] = Zt[np.arange(len(Zt)), pick]
        C[:, j] = Ct[np.arange(len(Ct)), pick]
    return Z, C


def run(lib_path: Path, pads: Sequence[str] = voc.PADS) -> dict:
    lib = json.loads(lib_path.read_text(encoding="utf-8"))
    jobs = {k: v for k, v in lib["jobs"].items() if len(v.get("phrasings", [])) == 2}
    names = list(jobs)
    sounds = lib["sounds"]
    paths = [str(lib_path.parent / s["file"]) for s in sounds]
    titles = [s["title"].strip().lower() for s in sounds]
    keys = sorted(set(titles))
    filed = [{names.index(j) for j in s.get("jobs", []) if j in jobs} for s in sounds]
    texts = {"job": [[jobs[n]["description"]] for n in names], "sound_only": [jobs[n]["phrasings"] for n in names]}
    texts["union"] = [a + b for a, b in zip(texts["job"], texts["sound_only"])]
    v = voc.get()
    out = {"n_sounds": len(sounds), "n_short": sum(s.get("duration", 9) < 1 for s in sounds), "n_jobs": len(names)}
    for pad in pads:
        A = voc.embed_files(paths, pad=pad)
        res = {"title": title_stats(v.calibrate(A, v.embed_text(keys)).z_q, titles, keys)}
        cheap = np.asarray(A, np.float64) @ np.asarray(v.embed_text(CHEAP_TEXTS), np.float64).T
        for way in WAYS:
            flat = [t for group in texts[way] for t in group]
            groups, n = [], 0
            for g in texts[way]:
                groups.append(list(range(n, n + len(g))))
                n += len(g)
            cal = v.calibrate(A, v.embed_text(flat))
            Z, C = best_over(cal.z_q, cal.Cq, groups)
            res[way] = way_stats(Z, C, filed, cheap, v.logit_scale)
        out[pad] = res
    return out


def rows_of(result: Mapping) -> list[str]:
    rows = []
    for pad in voc.PADS:
        for way, r in result.get(pad, {}).items():
            vc = r["verdicts"]
            rows.append(f"{pad:9s} {way:10s} strong {vc['strong']} plausible {vc['plausible']} weak {vc['weak']} "
                        f"no match {vc[voc.NO_MATCH]} AUC {r['auc'] and round(r['auc'], 3)}")
    return rows


def main(argv: Sequence[str] | None = None) -> int:
    ap = argparse.ArgumentParser(prog="python -m hark.listen.eval.real_effects", description=__doc__.splitlines()[0])
    ap.add_argument("library", nargs="?")
    ap.add_argument("--out", default=str(ds.HERE))
    args = ap.parse_args(argv)
    try:
        lib = Path(args.library) if args.library else ds.location("HARK_EVAL_SFX_LIBRARY")
        result = run(lib)
    except ds.EvalError as exc:
        print(f"real_effects: error: {exc}", file=sys.stderr)
        return 1
    write_result(Path(args.out), "real_effects.json", result, rows_of(result))
    return 0


if __name__ == "__main__":
    sys.exit(main())
