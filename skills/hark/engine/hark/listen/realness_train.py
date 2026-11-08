"""Training of the realness model file (listen-realness R12), run as ``python -m hark.listen.realness``.

Labelled folder layout: ``<dir>/real/<group>/...`` and ``<dir>/synth/<group>/...``. Each group folder is
one cross-validation group (an asset category or a generator family). ``--forge-negatives N`` adds N
renders of every forge recipe as synthesized clips (one group per recipe folder).
"""

from __future__ import annotations

import argparse
import hashlib
import json
from collections.abc import Sequence
from pathlib import Path

import numpy as np

from hark.core import audio
from hark.listen import realness as rl
from hark.listen import vocabulary as voc

SR = 48000
CLAP_GRID = (0.001, 0.003, 0.01)
SIGNAL_C = 0.1
FOLDS = 5
TARGET = 0.925
SILENCE_STEPS = (0.005, 0.02, 0.05, 0.1, 0.2, 0.3, 0.5)
FACT_PRECISION = 0.9
FACT_SUPPORT = 25


# ---------------------------------------------------------------- clips


def file_clips(path: str, windows: int = 1) -> list[np.ndarray]:
    """Training clips of one file: the whole sound up to 6 s, else evenly spaced 4 s windows."""
    x = np.asarray(audio.load(path, mono=True), np.float64)
    w = int(rl.WIN_S * SR)
    if len(x) <= int(rl.LONG_S * SR):
        clips = [x]
    else:
        starts = np.linspace(0, len(x) - w, windows + 2)[1:-1] if windows > 1 else [(len(x) - w) // 2]
        clips = [x[int(s):int(s) + w] for s in starts]
    return [c for c in clips if rl.rms_db(c) > rl.SILENT_DB]


def labelled_clips(root: str) -> list[dict]:
    """Clips of the labelled folder with label (1 real, 0 synth), group and source id."""
    out = []
    for side, label in (("real", 1), ("synth", 0)):
        base = Path(root) / side
        if not base.is_dir():
            continue
        for group in sorted(p for p in base.iterdir() if p.is_dir()):
            for f in audio.list_media([str(group)]):
                for k, c in enumerate(file_clips(f)):
                    out.append({"x": c, "y": label, "group": f"{side}:{group.name}", "source": f, "id": f"{f}#{k}"})
    return out


def forge_clips(n: int) -> list[dict]:
    """n renders of every forge recipe, as synthesized clips (the training fallback negatives)."""
    from hark.forge import recipes
    from hark.forge.engine.search import render

    out = []
    for name in recipes.names():
        mod = recipes.load(name)
        for seed in range(n):
            x = render(mod, dict(mod.DEFAULTS), seed)
            if x is not None:
                m = audio.to_mono(np.asarray(x, np.float64))
                out.append({"x": m, "y": 0, "group": f"forge:{recipes.folder_of(name)}", "source": name,
                            "id": f"forge:{name}#{seed}"})
    return out


# ---------------------------------------------------------------- features and weights


def featurise(clips: list[dict]) -> tuple[np.ndarray, np.ndarray, list[dict]]:
    """CLAP embeddings and signal vectors of the judgeable clips."""
    kept = [c for c in clips if rl.signal_features(c["x"]) is not None]
    for c in kept:
        c["feats"] = rl.signal_features(c["x"])
    E = voc.embed_audio([c["x"] for c in kept]).astype(np.float64)
    S = np.stack([rl.signal_vector(c["feats"]) for c in kept]) if kept else np.zeros((0, 13))
    return E, S, kept


def sample_weights(y: np.ndarray, groups: Sequence[str]) -> np.ndarray:
    """Each class weighs 1, split equally over its groups, then over a group's clips; rescaled to sum n."""
    w = np.zeros(len(y))
    for label in (0, 1):
        gs = sorted({g for g, yy in zip(groups, y) if yy == label})
        for g in gs:
            idx = [i for i, (gg, yy) in enumerate(zip(groups, y)) if gg == g and yy == label]
            w[idx] = 1.0 / len(gs) / len(idx)
    return w * len(y) / w.sum() if w.sum() else w


# ---------------------------------------------------------------- logistic heads


def prep(X: np.ndarray) -> dict:
    """Column mean, clip range and scale of a design matrix."""
    X = np.asarray(X, np.float64)
    mean0 = np.nanmean(X, axis=0)
    filled = np.where(np.isfinite(X), X, mean0)
    lo, hi = np.percentile(filled, 0.5, axis=0), np.percentile(filled, 99.5, axis=0)
    clipped = np.clip(filled, lo, hi)
    return {"mean": clipped.mean(axis=0), "scale": clipped.std(axis=0) + 1e-6, "clip": [lo, hi]}


def standardise(X: np.ndarray, p: dict) -> np.ndarray:
    """Fill NaN with the mean, clip and standardise."""
    X = np.where(np.isfinite(X), X, p["mean"])
    return (np.clip(X, p["clip"][0], p["clip"][1]) - p["mean"]) / p["scale"]


def fit_logistic(Z: np.ndarray, y: np.ndarray, w: np.ndarray, C: float,
                 bounds: Sequence[tuple[float | None, float | None]] | None = None) -> tuple[np.ndarray, float]:
    """Weighted L2 logistic regression with a free intercept (L-BFGS-B)."""
    from scipy.optimize import minimize

    s = np.where(y > 0, 1.0, -1.0)

    def loss(theta: np.ndarray) -> tuple[float, np.ndarray]:
        beta, b = theta[:-1], theta[-1]
        m = Z @ beta + b
        t = -s * m
        lv = np.logaddexp(0, t)
        g = -s * w / (1 + np.exp(-t)) * C
        return float(C * np.sum(w * lv) + 0.5 * beta @ beta), np.append(Z.T @ g + beta, g.sum())

    bnds = list(bounds or [(None, None)] * Z.shape[1]) + [(None, None)]
    res = minimize(loss, np.zeros(Z.shape[1] + 1), jac=True, method="L-BFGS-B", bounds=bnds,
                   options={"maxiter": 3000})
    return res.x[:-1], float(res.x[-1])


def signal_bounds() -> list[tuple[float | None, float | None]]:
    """Physical-sign bounds of the signal head coefficients."""
    return [(0.0, None) if k in rl.POSITIVE else (None, 0.0) for k in rl.SIGNAL_INPUTS]


def fit_head(X: np.ndarray, y: np.ndarray, w: np.ndarray, C: float, bounds=None) -> dict:
    """A head dict (mean, scale, clip, coef, intercept) fitted on X."""
    p = prep(X)
    coef, b = fit_logistic(standardise(X, p), y, w, C, bounds)
    return {"mean": p["mean"].tolist(), "scale": p["scale"].tolist(),
            "clip": [p["clip"][0].tolist(), p["clip"][1].tolist()], "coef": coef.tolist(), "intercept": b}


def logits(head: dict, X: np.ndarray) -> np.ndarray:
    """Head logits of every row."""
    return np.array([rl.head_logit(head, x) for x in X])


def folds_of(groups: Sequence[str]) -> list[np.ndarray]:
    """5 grouped folds: sorted group names dealt round-robin."""
    names = sorted(set(groups))
    fold = {g: i % FOLDS for i, g in enumerate(names)}
    return [np.array([fold[g] == k for g in groups]) for k in range(min(FOLDS, len(names)))]


def wlogloss(p: np.ndarray, y: np.ndarray, w: np.ndarray) -> float:
    """Weighted log loss."""
    p = np.clip(p, 1e-9, 1 - 1e-9)
    return float(-np.sum(w * (y * np.log(p) + (1 - y) * np.log(1 - p))) / np.sum(w))


def out_of_fold(E: np.ndarray, S: np.ndarray, y: np.ndarray, w: np.ndarray, groups: Sequence[str],
                C: float) -> np.ndarray:
    """Out-of-fold logits (n, 2) of the CLAP and signal heads."""
    out = np.zeros((len(y), 2))
    for test in folds_of(groups):
        tr = ~test
        if len(set(y[tr])) < 2:
            continue
        out[test, 0] = logits(fit_head(E[tr], y[tr], w[tr], C), E[test])
        out[test, 1] = logits(fit_head(S[tr], y[tr], w[tr], SIGNAL_C, signal_bounds()), S[test])
    return out


def choose_c(E: np.ndarray, S: np.ndarray, y: np.ndarray, w: np.ndarray, groups: Sequence[str]) -> float:
    """The CLAP head C with the lowest grouped out-of-fold weighted log loss."""
    scores = {C: wlogloss(1 / (1 + np.exp(-out_of_fold(E, S, y, w, groups, C)[:, 0])), y, w) for C in CLAP_GRID}
    return min(scores, key=scores.get)


# ---------------------------------------------------------------- thresholds, facts, base rates


def threshold_hi(p: np.ndarray, y: np.ndarray, target: float = TARGET) -> float:
    """The lowest p above which at least the target share is real (clamped 0.5 to 0.95)."""
    order = np.argsort(-p, kind="stable")
    prec = np.cumsum(y[order]) / np.arange(1, len(p) + 1)
    ok = np.nonzero(prec >= target)[0]
    v = float(p[order][ok[-1]]) if len(ok) else 0.95
    return round(min(0.95, max(0.5, v)), 2)


def threshold_lo(p: np.ndarray, y: np.ndarray, target: float = TARGET) -> float:
    """The highest p below which at least the target share is synthesized (clamped 0.05 to 0.5)."""
    order = np.argsort(p, kind="stable")
    prec = np.cumsum(1 - y[order]) / np.arange(1, len(p) + 1)
    ok = np.nonzero(prec >= target)[0]
    v = float(p[order][ok[-1]]) if len(ok) else 0.05
    return round(min(0.5, max(0.05, v)), 2)


def _fact_for(v: np.ndarray, y: np.ndarray, side: str, op: str) -> dict | None:
    want = 1 if side == "real" else 0
    best = None
    for t in np.unique(v):
        beyond = v <= t if op == "<=" else v >= t
        n = int(beyond.sum())
        if n >= FACT_SUPPORT and np.mean(y[beyond] == want) >= FACT_PRECISION:
            weaker = best is None or (t > best["value"] if op == "<=" else t < best["value"])
            if weaker:
                best = {"value": round(float(t), 4), "precision": round(float(np.mean(y[beyond] == want)), 3),
                        "support": n}
    return best


def facts_of(S: np.ndarray, y: np.ndarray) -> list[dict]:
    """The weakest 90 %-precision threshold per measurement and side (R6, R12)."""
    out = []
    for j, feat in enumerate(rl.SIGNAL_INPUTS):
        if feat == "silence_frac":
            continue
        v, ok = S[:, j], np.isfinite(S[:, j])
        for side in ("synth", "real"):
            for op in ("<=", ">="):
                f = _fact_for(v[ok], y[ok], side, op)
                if f:
                    out.append({"feature": feat, "side": side, "op": op, **f})
    return out


def silence_base(S: np.ndarray, y: np.ndarray) -> list[dict]:
    """Share of clips per class with at least each silence fraction."""
    sf = S[:, 0]
    return [{"at_least": a, "real": round(float(np.mean(sf[y == 1] >= a)), 3) if (y == 1).any() else 0.0,
             "synth": round(float(np.mean(sf[y == 0] >= a)), 3) if (y == 0).any() else 0.0} for a in SILENCE_STEPS]


def clap_direction(head: dict) -> dict:
    """The 8 vocabulary terms and prompts nearest to and farthest from the CLAP head direction."""
    v = voc.get()
    d = np.asarray(head["coef"]) / np.asarray(head["scale"])
    texts = v.terms + list(rl.REAL_PROMPTS + rl.SYNTH_PROMPTS)
    T = np.vstack([v.E, voc.embed_plain_text(list(rl.REAL_PROMPTS + rl.SYNTH_PROMPTS))]).astype(np.float64)
    order = np.argsort(-(T @ d), kind="stable")
    return {"toward_real": [texts[i] for i in order[:8]], "toward_synth": [texts[i] for i in order[::-1][:8]],
            "how": "texts whose embedding has the highest and lowest cosine with coef / scale"}


def file_thresholds(p: np.ndarray, y: np.ndarray, sources: Sequence[str], clip: dict) -> dict:
    """File thresholds on sources with at least 2 clips (file p = mean clip p)."""
    by: dict[str, list[int]] = {}
    for i, s in enumerate(sources):
        by.setdefault(s, []).append(i)
    long = [idx for idx in by.values() if len(idx) >= 2]
    if not long:
        return {"hi": clip["hi"], "lo": max(0.40, clip["lo"]), "files": 0, "synth_files": 0,
                "how": "no long sources: clip thresholds, lo at least 0.40"}
    fp = np.array([p[idx].mean() for idx in long])
    fy = np.array([y[idx[0]] for idx in long])
    return {"hi": max(threshold_hi(fp, fy), clip["hi"]), "lo": max(threshold_lo(fp, fy), clip["lo"]),
            "files": len(long), "synth_files": int((fy == 0).sum()), "how": "R12 rule on long sources"}


# ---------------------------------------------------------------- model


def _training_set(root: str | None, forge_negatives: int) -> dict:
    """Featurised clips with labels, groups and weights; ValueError without both classes."""
    clips = (labelled_clips(root) if root else []) + (forge_clips(forge_negatives) if forge_negatives else [])
    E, S, kept = featurise(clips)
    y = np.array([c["y"] for c in kept], np.float64)
    if len(set(y.tolist())) < 2:
        raise ValueError("training needs both real and synthesized clips")
    groups = [c["group"] for c in kept]
    return {"E": E, "S": S, "kept": kept, "y": y, "groups": groups, "w": sample_weights(y, groups)}


def _fit(ts: dict) -> dict:
    """C, the fusion layer on out-of-fold head scores, clip thresholds and both heads."""
    E, S, y, w, groups = ts["E"], ts["S"], ts["y"], ts["w"], ts["groups"]
    C = choose_c(E, S, y, w, groups)
    oof = out_of_fold(E, S, y, w, groups, C)
    fc, fb = fit_logistic(oof, y, w, 1.0, [(0.0, None), (0.0, None)])
    p_oof = 1 / (1 + np.exp(-(oof @ fc + fb)))
    return {"C": C, "fc": fc, "fb": fb, "p_oof": p_oof,
            "clip_th": {"hi": threshold_hi(p_oof, y), "lo": threshold_lo(p_oof, y)},
            "clap": {**fit_head(E, y, w, C), "inputs": "clap_embedding_512"},
            "signal": {**fit_head(S, y, w, SIGNAL_C, signal_bounds()), "inputs": list(rl.SIGNAL_INPUTS)}}


def _data_block(kept: list, y: np.ndarray, groups: list[str]) -> dict:
    return {"clips": len(kept), "clean": len(kept), "augmented": 0, "real": int(y.sum()),
            "synth": int((1 - y).sum()), "groups": sorted(set(groups))}


def train_model(root: str | None, forge_negatives: int = 0) -> dict:
    """Train both heads and the fusion layer; return the model dict (R12, without augmentation)."""
    ts = _training_set(root, forge_negatives)
    kept, y, S, groups = ts["kept"], ts["y"], ts["S"], ts["groups"]
    fit = _fit(ts)
    ids = sorted(c["id"] for c in kept)
    return {
        "version": 1, "trained": hashlib.sha1("\n".join(ids).encode()).hexdigest()[:12],
        "about": f"trained on {len(kept)} clips in {len(set(groups))} groups (no augmented copies)",
        "data": _data_block(kept, y, groups), "long_s": rl.LONG_S, "win_s": rl.WIN_S, "silent_db": rl.SILENT_DB,
        "heads": {"clap": fit["clap"], "signal": fit["signal"]},
        "fusion": {"heads": ["clap", "signal"], "coef": [float(fit["fc"][0]), float(fit["fc"][1])],
                   "intercept": fit["fb"]},
        "C": {"clap": fit["C"], "signal": SIGNAL_C}, "thresholds": fit["clip_th"],
        "file_thresholds": file_thresholds(fit["p_oof"], y, [c["source"] for c in kept], fit["clip_th"]),
        "prompts": {"real": list(rl.REAL_PROMPTS), "synth": list(rl.SYNTH_PROMPTS)},
        "facts": facts_of(S, y), "silence_base": silence_base(S, y), "clap_direction": clap_direction(fit["clap"]),
        "eval": {"how": "grouped 5-fold out-of-fold predictions", "n": len(y)},
    }


def write_model(model: dict, out: str | Path) -> None:
    """Write the model file: one top-level key per line."""
    keys = list(model)
    lines = ["{"] + [f" {json.dumps(k)}: {json.dumps(model[k])}" + ("," if i < len(keys) - 1 else "")
                     for i, k in enumerate(keys)] + ["}"]
    Path(out).parent.mkdir(parents=True, exist_ok=True)
    Path(out).write_text("\n".join(lines) + "\n", encoding="utf-8")


def main(argv: Sequence[str] | None = None) -> int:
    """``python -m hark.listen.realness --train DIR [-o PATH] [--forge-negatives N]``."""
    ap = argparse.ArgumentParser(prog="python -m hark.listen.realness")
    ap.add_argument("--train", metavar="DIR", help="labelled folder: real/<group>/..., synth/<group>/...")
    ap.add_argument("--forge-negatives", type=int, default=0, help="renders per forge recipe as negatives")
    ap.add_argument("-o", "--out", default=str(rl.MODEL_PATH), help="model file to write")
    args = ap.parse_args(argv)
    if not args.train and not args.forge_negatives:
        ap.print_help()
        return 2
    model = train_model(args.train, args.forge_negatives)
    write_model(model, args.out)
    print(f"{model['data']['clips']} clips, thresholds lo {model['thresholds']['lo']} / hi "
          f"{model['thresholds']['hi']} -> {args.out}")
    return 0
