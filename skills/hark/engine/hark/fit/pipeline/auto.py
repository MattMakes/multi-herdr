"""The fit engine, fitting side: from a recording to one model.

It strips a bed, measures the sound's character, plans candidates, fits each
with its post stage, scores the renders, confirms the best on more seeds,
checks the bed, adds a space layer and stores the diagnostics. The render
side lives in ``hark.fit.pipeline.render``; this module re-exports it.
"""

from __future__ import annotations

import time
from typing import Any, Callable

import numpy as np

from hark.fit.pipeline import quality as Q
from hark.fit.pipeline._character import ALL, LONG_PLANS, PLANS, bed_of, character, plan, strip_bed
from hark.fit.pipeline._layers import HYBRIDS, ROLE, deterministic, fit_hybrid, fit_kind
from hark.fit.pipeline._model import channels_of, kind_of, load, save
from hark.fit.pipeline._post import apply_post, bed_noise, finish, mono
from hark.fit.pipeline._postfit import fit_post
from hark.fit.pipeline.render import (
    KNOBS, finished, fit_pair, identity, knobs_of, morph, morph_model, render, render_full, summary,
)

__all__ = [
    "fit", "fit_kind", "fit_hybrid", "deterministic", "character", "plan", "bed_of", "strip_bed", "bed_noise",
    "fit_post", "apply_post", "kind_of", "channels_of", "render", "morph", "morph_model", "finish", "knobs_of",
    "identity", "finished", "fit_pair", "summary", "save", "load", "KNOBS", "KINDS", "ROLE", "HYBRIDS", "PLANS",
    "LONG_PLANS", "ALL", "TAKE_SEEDS", "TAKE_VARY", "CONFIRM", "CONFIRM_SEEDS", "CLAP_MARGIN", "CLAP_TOL",
    "CLAP_ENV_TOL", "BED_CHECK", "NOISE_KINDS",
]

KINDS = ("modal", "spectral", "sines", "hybrid", "gesture")
TAKE_SEEDS = (0, 1, 2)
TAKE_VARY = 0.3
CONFIRM = 3
CONFIRM_SEEDS = (2, 3)
CLAP_MARGIN = 0.02
CLAP_TOL = 1.5
CLAP_ENV_TOL = 0.05
BED_CHECK = True
NOISE_KINDS = ("spectral", "gesture")
VARY_SCALE = {"modal": 0.5, "hybrid": 0.5}


# ---------------------------------------------------------------- the scoring context


class _Ctx:
    """What every candidate is scored against: the original, the stripped signal, the bed, the seeds."""

    def __init__(self, X0: np.ndarray, bed: dict | None, seeds: tuple, clap: bool, name: str | None,
                 fast: bool) -> None:
        self.X0, self.bed, self.seeds, self.clap, self.name, self.fast = X0, bed, tuple(seeds), clap, name, fast
        self.X = strip_bed(X0, bed) if bed else X0
        self.emb = None
        self.models: dict[str, dict] = {}

    def quality(self, y: np.ndarray) -> dict:
        q = Q.quality(self.X0, y)
        if self.clap:
            q["clap_cos"] = round(self._clap(y), 4)
        return q

    def _clap(self, y: np.ndarray) -> float:
        from hark.models import require_ml

        require_ml("CLAP")
        from hark.listen import vocabulary

        if self.emb is None:
            self.emb = vocabulary.embed_audio([mono(self.X0).astype(np.float32)])[0]
        e = vocabulary.embed_audio([mono(y).astype(np.float32)])[0]
        return float(np.dot(self.emb, e))


def load_input(x: Any) -> np.ndarray:
    """The recording as float64 at 48 kHz: (n,) when mono or equal channels, else (n, 2)."""
    X = Q.load(x)
    if X.ndim == 2 and X.shape[1] > 2:
        X = X[:, :2]
    return mono(X) if channels_of(X) == 1 else X


# ---------------------------------------------------------------- candidate fits


def _fit_candidate(ctx: _Ctx, cand: str) -> dict:
    if cand.startswith("hybrid:"):
        layers = HYBRIDS[cand.split(":", 1)[1]]
        return fit_hybrid(ctx.X, layers, ctx.fast, base=ctx.models.get(layers[0]), name=ctx.name)
    if cand == "gesture":
        tones = ctx.models.get("sines")
        return fit_kind(ctx.X, "gesture", ctx.fast, ctx.name, tones=tones)
    m = fit_kind(ctx.X, cand, ctx.fast, ctx.name)
    ctx.models[cand] = m
    return m


def _judge(ctx: _Ctx, with_bed: bool) -> Callable[[list], tuple]:
    def judge(ys: list) -> tuple:
        scores, claps = [], []
        for i, y in enumerate(ys):
            if with_bed and ctx.bed:
                y = _with_bed(y, ctx.bed, i)
            q = ctx.quality(y)
            scores.append(Q.score(q))
            claps.append(q.get("clap_cos", 0.0))
        return float(np.mean(scores)), float(np.mean(claps))

    return judge


def _with_bed(y: np.ndarray, bed: dict, seed: int) -> np.ndarray:
    b = bed_noise(bed, len(y), seed)
    return y + (b if y.ndim == 1 else b[:, None])


def _attach_post(ctx: _Ctx, model: dict, X: np.ndarray, judge: Callable | None) -> dict:
    renders = [render_full(model, s, take=False, bed=False, final=False) for s in (0, 1)]
    noise = kind_of(model) in NOISE_KINDS
    model["post"] = fit_post(X, renders, judge=judge, noise=noise)
    return model


def _score_on(ctx: _Ctx, model: dict, seeds: tuple) -> tuple[float, float, dict]:
    scores, claps, first = [], [], None
    for s in seeds:
        q = ctx.quality(render(model, seed=s, take=False))
        first = first or q
        scores.append(Q.score(q))
        claps.append(q.get("clap_cos", 0.0))
    return float(np.mean(scores)), float(np.mean(claps)), first


def _set_bed(model: dict, bed: dict | None) -> None:
    if bed and model.get("post") is not None:
        model["post"]["bed"] = bed


def _run_candidate(ctx: _Ctx, cand: str, post: bool) -> tuple[dict | None, dict]:
    t0 = time.perf_counter()
    try:
        model = _fit_candidate(ctx, cand)
        if post:
            _attach_post(ctx, model, ctx.X, _judge(ctx, True) if ctx.clap else None)
        _set_bed(model, ctx.bed)
        sc, clap, q = _score_on(ctx, model, ctx.seeds)
    except Exception as exc:  # a failing candidate is recorded and the loop goes on
        return None, {"candidate": cand, "score": None, "fit_s": round(time.perf_counter() - t0, 3),
                      "error": f"{type(exc).__name__}: {exc}"}
    row = {"candidate": cand, "score": round(sc, 3), "fit_s": round(time.perf_counter() - t0, 3), "quality": q}
    if ctx.clap:
        row["clap"] = round(clap, 4)
    return model, row


# ---------------------------------------------------------------- choice rules


def _confirm(ctx: _Ctx, kept: dict, rows: dict, n: int) -> str | None:
    if n <= 1 or len(ctx.seeds) != 1 or len(kept) < 2 or set(CONFIRM_SEEDS) & set(ctx.seeds):
        return None
    top = sorted(kept, key=lambda c: rows[c]["score"])[:n]
    for c in top:
        _confirm_row(ctx, kept[c], rows[c])
    return min(top, key=lambda c: rows[c]["score2"])


def _confirm_row(ctx: _Ctx, model: dict, row: dict) -> None:
    if "score2" in row:
        return
    sc, clap, _ = _score_on(ctx, model, CONFIRM_SEEDS)
    row["score2"] = round((row["score"] + 2 * sc) / 3, 3)
    if ctx.clap:
        row["clap2"] = round((row.get("clap", 0.0) + 2 * clap) / 3, 4)


def _clap_margin(ctx: _Ctx, kept: dict, rows: dict, choice: str) -> str:
    base = rows[choice]
    alts = [c for c in kept if c != choice and rows[c].get("clap", 0) >= base.get("clap", 0) + CLAP_MARGIN
            and rows[c]["score"] <= base["score"] + CLAP_TOL]
    for c in alts:
        _confirm_row(ctx, kept[c], rows[c])
    env0 = base["quality"]["env_corr"]
    ok = [c for c in alts if rows[c]["clap2"] >= base["clap2"] + CLAP_MARGIN
          and rows[c]["score2"] <= base["score2"] + CLAP_TOL and rows[c]["quality"]["env_corr"] >= env0 - CLAP_ENV_TOL]
    if not ok:
        return choice
    best = max(ok, key=lambda c: rows[c]["clap2"])
    rows[best]["chosen_by"] = "clap"
    return best


def _choose(ctx: _Ctx, kept: dict, rows: dict, confirm: int) -> str:
    choice = min(kept, key=lambda c: rows[c]["score"])
    confirmed = _confirm(ctx, kept, rows, confirm)
    if confirmed is None:
        return choice
    return _clap_margin(ctx, kept, rows, confirmed) if ctx.clap else confirmed


# ---------------------------------------------------------------- bed check, space, finish


def _bed_check(ctx: _Ctx, model: dict, chosen: str, row: dict, post: bool, seeds: tuple) -> tuple[dict, dict]:
    plain = _Ctx(ctx.X0, None, ctx.seeds, ctx.clap, ctx.name, ctx.fast)
    if chosen == "gesture":
        plain.models["sines"] = fit_kind(ctx.X0, "sines", ctx.fast, ctx.name)
    m, prow = _run_candidate(plain, chosen, post)
    prow["candidate"] = f"plain:{chosen}"
    entry = dict(ctx.bed)
    entry.pop("psd_db", None)
    if m is None:
        entry.update(score_stripped=row.get("score2", row["score"]), score_plain=None, kept=True)
        return model, {"row": prow, "bed": entry}
    sp = _score_on(plain, m, seeds)[0] if seeds != ctx.seeds else prow["score"]
    ss = row.get("score2", row["score"])
    kept = not sp < ss
    entry.update(score_stripped=round(ss, 3), score_plain=round(sp, 3), kept=kept)
    return (model if kept else m), {"row": prow, "bed": entry}


def _add_space(X: np.ndarray, model: dict) -> dict | None:
    from hark.fit.pipeline._model import space_module

    if X.ndim != 2 or not model.get("post"):
        return None
    sp = space_module()
    if not sp.needs_space(X):
        return None
    dry = render_full(model, 0, take=False, bed=False, final=False, space=False)
    space = sp.fit_space(X, dry=dry)
    if space:
        model["post"]["space"] = space
    return {"found": bool(space), "tail_corr": space.get("tail_corr") if space else None}


def _finish_model(model: dict, name: str | None) -> dict:
    vs = VARY_SCALE.get(kind_of(model))
    if vs is not None:
        model["vary_scale"] = vs
    if name is not None:
        model["source"] = name
    return model


# ---------------------------------------------------------------- fit


def _fit_single(X0: np.ndarray, kind: str, layers: tuple | None, fast: bool, name: str | None, post: bool,
                bed: dict | None, space: bool) -> dict:
    ctx = _Ctx(X0, bed, (0,), False, name, fast)
    if kind == "hybrid":
        model = fit_hybrid(ctx.X, tuple(layers or ("modal", "spectral")), fast, name=name)
    else:
        model = fit_kind(ctx.X, kind, fast, name)
    if post:
        _attach_post(ctx, model, ctx.X, None)
        if space:
            _add_space(ctx.X, model)
    _set_bed(model, bed)
    return model


def _candidate_loop(ctx: _Ctx, cands: list, post: bool, verbose: bool) -> tuple[dict, dict, list]:
    kept, rows, order = {}, {}, []
    for cand in cands:
        model, row = _run_candidate(ctx, cand, post)
        order.append(row)
        if verbose:
            print(f"  {cand}: " + (f"{row['score']:.3f}" if model is not None else row["error"]))
        if model is not None:
            kept[cand], rows[cand] = model, row
    if not kept:
        errors = "; ".join(f"{r['candidate']}: {r['error']}" for r in order)
        raise RuntimeError(f"fit: every candidate failed ({errors})")
    return kept, rows, order


def _fit_auto(ctx: _Ctx, candidates: list | None, post: bool, space: bool, confirm: int | None,
              verbose: bool, collect: list | None) -> dict:
    char = character(ctx.X)
    cands = list(candidates) if candidates else plan(char)
    kept, rows, order = _candidate_loop(ctx, cands, post, verbose)
    chosen = _choose(ctx, kept, rows, CONFIRM if confirm is None else confirm)
    model = kept[chosen]
    fe: dict[str, Any] = {"kind": "auto", "chosen": chosen, "character": char, "candidates": order,
                          "seeds": list(ctx.seeds)}
    if ctx.bed and BED_CHECK:
        seeds = ctx.seeds + CONFIRM_SEEDS if "score2" in rows[chosen] else ctx.seeds
        model, info = _bed_check(ctx, model, chosen, rows[chosen], post, seeds)
        order.append(info["row"])
        fe["bed"] = info["bed"]
        if not info["bed"]["kept"]:
            model.get("post", {}).pop("bed", None)
    if post and space:
        X = ctx.X if fe.get("bed", {}).get("kept", True) else ctx.X0
        fe["space"] = _add_space(X, model)
    if collect is not None:
        collect.extend(order)
    model["fit_engine"] = fe
    return model


def fit(x: Any, kind: str = "auto", layers: tuple | None = None, candidates: list | None = None,
        clap: bool = False, fast: bool = False, seeds: tuple = (0,), name: str | None = None,
        verbose: bool = False, collect: list | None = None, post: bool = True, bed: bool = True,
        space: bool = True, confirm: int | None = None) -> dict:
    """Fit a recording (a path or a 48 kHz array) and return the model."""
    t0 = time.perf_counter()
    X0 = load_input(x)
    found = bed_of(X0) if bed else None
    if kind != "auto":
        if kind not in KINDS:
            raise ValueError(f"fit: unknown kind {kind!r}; expected auto or one of {KINDS}")
        model = _fit_single(X0, kind, layers, fast, name, post, found, space)
        model = _finish_model(model, name)
        model["fit_engine"] = {"kind": kind, "seconds": round(time.perf_counter() - t0, 3)}
        return model
    ctx = _Ctx(X0, found, tuple(seeds), clap, name, fast)
    model = _fit_auto(ctx, candidates, post, space, confirm, verbose, collect)
    model = _finish_model(model, name)
    model["fit_engine"]["seconds"] = round(time.perf_counter() - t0, 3)
    return model
