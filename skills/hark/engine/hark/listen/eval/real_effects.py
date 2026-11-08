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
    col = np.array([list(keys).index(t) for t in titles])
    own_z = Z[np.arange(len(titles)), col]
    mask = np.ones_like(Z, bool)
    mask[np.arange(len(titles)), col] = False
    beat = ((Z > own_z[:, None]) & mask).sum(axis=1)
    return {"verdicts": verdict_counts(own_z), "median_z": round(float(np.median(own_z)), 2),
            "auc": mt.auc_pairwise(own_z, Z[mask]), "top1": int((beat < 1).sum()), "top5": int((beat < 5).sum())}


def _own_job(i: int, jobs: set[int], Z: np.ndarray, C: np.ndarray, cheap: np.ndarray, scale: float) -> dict:
    """One sound: its own-job z, the z of the other jobs, and whether a cheap-synth text won."""
    own = max(jobs, key=lambda j: Z[i, j])
    others = Z[i, [j for j in range(Z.shape[1]) if j not in jobs]]
    p = voc.softmax([C[i, own], *cheap[i]], scale)
    return {"z": Z[i, own], "others": others, "cheap": int(np.argmax(p) > 0)}


def way_stats(Z: np.ndarray, C: np.ndarray, filed: Sequence[set[int]], cheap: np.ndarray, scale: float) -> dict:
    """Own-job verdicts, AUC own against other jobs, other jobs passed and cheap-synth wins (Z, C: sounds x jobs)."""
    rows = [_own_job(i, jobs, Z, C, cheap, scale) for i, jobs in enumerate(filed) if jobs]
    own_z = [r["z"] for r in rows]
    neg = np.concatenate([r["others"] for r in rows]) if rows else np.zeros(0)
    passed = [float(np.mean(r["others"] >= PASS_Z)) for r in rows if len(r["others"])]
    median = round(float(np.median(own_z)), 2) if own_z else None
    return {"n": len(rows), "verdicts": verdict_counts(own_z), "median_z": median, "auc": mt.auc_pairwise(own_z, neg),
            "other_jobs_passed": mt.mean_or_none(passed), "cheap_won": sum(r["cheap"] for r in rows)}


def best_over(Zt: np.ndarray, Ct: np.ndarray, groups: Sequence[Sequence[int]]) -> tuple[np.ndarray, np.ndarray]:
    """Per job, the best z over its texts and the cosine of that text."""
    Z = np.zeros((Zt.shape[0], len(groups)))
    C = np.zeros_like(Z)
    for j, cols in enumerate(groups):
        pick = np.asarray(cols)[np.argmax(Zt[:, cols], axis=1)]
        Z[:, j] = Zt[np.arange(len(Zt)), pick]
        C[:, j] = Ct[np.arange(len(Ct)), pick]
    return Z, C


def column_groups(texts: Sequence[Sequence[str]]) -> tuple[list[str], list[list[int]]]:
    """The flat text list and, per job, the columns of its texts."""
    flat = [t for group in texts for t in group]
    ends = np.cumsum([len(g) for g in texts]).tolist()
    return flat, [list(range(e - len(g), e)) for g, e in zip(texts, ends)]


def way_texts(jobs: Mapping[str, Mapping]) -> dict[str, list[list[str]]]:
    """The texts of each way of asking, per job."""
    job = [[j["description"]] for j in jobs.values()]
    sound = [list(j["phrasings"]) for j in jobs.values()]
    return {"job": job, "sound_only": sound, "union": [a + b for a, b in zip(job, sound)]}


def read_library(lib_path: Path) -> dict:
    """The sounds, jobs with 2 phrasings, filed jobs per sound, titles and the texts of each way."""
    lib = json.loads(lib_path.read_text(encoding="utf-8"))
    jobs = {k: v for k, v in lib["jobs"].items() if len(v.get("phrasings", [])) == 2}
    names = list(jobs)
    sounds = lib["sounds"]
    titles = [s["title"].strip().lower() for s in sounds]
    return {"sounds": sounds, "names": names, "paths": [str(lib_path.parent / s["file"]) for s in sounds],
            "titles": titles, "keys": sorted(set(titles)), "texts": way_texts(jobs),
            "filed": [{names.index(j) for j in s.get("jobs", []) if j in jobs} for s in sounds]}


def pad_stats(v: voc.Vocab, lib: Mapping, pad: str) -> dict:
    """The title check and every way of asking for one pad."""
    A = voc.embed_files(lib["paths"], pad=pad)
    res = {"title": title_stats(v.calibrate(A, v.embed_text(lib["keys"])).z_q, lib["titles"], lib["keys"])}
    cheap = np.asarray(A, np.float64) @ np.asarray(v.embed_text(CHEAP_TEXTS), np.float64).T
    for way in WAYS:
        flat, groups = column_groups(lib["texts"][way])
        cal = v.calibrate(A, v.embed_text(flat))
        res[way] = way_stats(*best_over(cal.z_q, cal.Cq, groups), lib["filed"], cheap, v.logit_scale)
    return res


def run(lib_path: Path, pads: Sequence[str] = voc.PADS) -> dict:
    lib = read_library(lib_path)
    v = voc.get()
    sounds = lib["sounds"]
    out = {"n_sounds": len(sounds), "n_short": sum(s.get("duration", 9) < 1 for s in sounds),
           "n_jobs": len(lib["names"])}
    return {**out, **{pad: pad_stats(v, lib, pad) for pad in pads}}


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
