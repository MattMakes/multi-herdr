"""The forge run loop: search, pick, re-render, write and report.

`forge()` searches a recipe against a critic and keeps a few diverse strong
sounds. `palette()` scores and writes an exact member set at matched
loudness. The report and audition text live in `loop_report`.
"""

from __future__ import annotations

import functools
import math
import os
import time
import types
from collections.abc import Callable, Sequence
from pathlib import Path
from typing import Any

import numpy as np

from hark.forge import recipes as registry
from hark.forge.engine import polish, refsets, schema, search, tone
from hark.forge.engine.args import bind, with_signature
from hark.forge.engine.loop_report import audition, palette_md, report_md

SR = 48000
SCORE_KEYS = ("total", "passes", "match_z", "alt_margin", "p_real", "ref_typ_z", "feat_pen", "tone_db", "tone_ratio",
              "sounds_like", "total_v1", "mode", "tone_pen", "env_db", "env_ratio", "env_out", "env_pen", "art_pen",
              "polish_pen", "hard", "polish_words")
POOL_HISTORY = 64
FAMILY_BUDGET = 96

__all__ = ["SCORE_KEYS", "audition", "family_box", "forge", "load_recipe", "palette", "palette_md", "pick_refs",
           "recipes", "render_block", "render_params", "report_md", "resolve_refs", "variant_recipe"]


# ---------------------------------------------------------------- recipes

def recipes() -> list[str]:
    """Sorted recipe module names; helper modules (leading _) are not listed."""
    return registry.names()


def load_recipe(name: str) -> types.ModuleType:
    """The imported recipe module."""
    return registry.load(name)


def _name(rec: Any) -> str:
    return str(getattr(rec, "__name__", "recipe")).rsplit(".", 1)[-1]


def _is_real(rec: Any) -> bool:
    return _name(rec) in recipes()


def _base(p: Any) -> str:
    return os.path.basename(str(p).replace("\\", "/"))


# ---------------------------------------------------------------- references

def _drop_held(files: list[str], kit: bool, log: Callable | None) -> list[str]:
    held = set(refsets.held_out(files, kit=kit))
    if held and log:
        log(f"dropped {len(held)} held-out ref(s): {', '.join(sorted(_base(p) for p in held))}")
    return [f for f in files if f not in held]


def _media(refs: Any) -> list[str]:
    from hark.core.audio import list_media

    return list(list_media([refs] if isinstance(refs, (str, os.PathLike)) else list(refs)))


def _recipe_refset(rec: Any) -> tuple[list[str] | None, str | None]:
    cat = getattr(rec, "REFSET", None)
    if not cat:
        return None, None
    try:
        got = refsets.refset(cat)
    except (refsets.RefsetsMissing, KeyError):
        return None, None
    return (got, cat) if got else (None, None)


def _recipe_refs(recipe: Any, log: Callable | None) -> tuple[list[str] | None, str | None]:
    """The recipe's refset, else its REFS files that exist."""
    rec = load_recipe(recipe) if isinstance(recipe, str) else recipe
    got, cat = _recipe_refset(rec)
    if got:
        return got, cat
    have = [p for p in getattr(rec, "REFS", None) or [] if os.path.isfile(p)]
    return _drop_held(have, True, log) or None, None


def pick_refs(refs: Any = None, refset: str | None = None, recipe: Any = None,
              log: Callable | None = None) -> tuple[list[str] | None, str | None]:
    """(reference files or None, refset category used or None)."""
    if refset:
        res = (refsets.refset(refset), refset)
    elif refs:
        res = (_drop_held(_media(refs), False, log) or None, None)
    else:
        res = _recipe_refs(recipe, log)
    return res


def resolve_refs(refs: Any = None, refset: str | None = None, recipe: Any = None,
                 log: Callable | None = None) -> list[str] | None:
    """The reference file list of pick_refs."""
    return pick_refs(refs, refset, recipe, log)[0]


# ---------------------------------------------------------------- render blocks

class _Variant(types.SimpleNamespace):
    def render(self, p: dict, r: np.random.Generator) -> Any:
        return self.RENDER_FN(dict(self.MEMBERS[int(p["variant"])]), r)


def _fn_module(fn: Any) -> str:
    while isinstance(fn, functools.partial):
        fn = fn.func
    return getattr(fn, "__module__", None) or "hark.forge.recipes.variant"


def variant_recipe(render_fn: Callable, members: Sequence[dict], names: Sequence[Any] | None = None,
                   name: str | None = None, **attrs: Any) -> Any:
    """A stand-in recipe that searches one key 'variant' over fixed member param dicts."""
    m = len(members)
    return _Variant(__name__=name or _fn_module(render_fn), SPACE={"variant": list(range(m))},
                    DEFAULTS={"variant": 0}, MEMBERS=[dict(p) for p in members],
                    MEMBER_NAMES=list(names) if names is not None else [None] * m, RENDER_FN=render_fn, **attrs)


def render_block(rec: Any, params: dict, seed: int, member: Any = None, mode: str | None = None,
                 gain: float | None = None) -> dict | None:
    """The render block of one pick, or None when the render function has no importable name."""
    if set(params) == {"variant"} and getattr(rec, "MEMBERS", None) is not None:
        v = int(params["variant"])
        member = member if member is not None else rec.MEMBER_NAMES[v]
        return schema.make_block(rec.RENDER_FN, rec.MEMBERS[v], seed, member=member, mode=mode, gain=gain)
    return schema.make_block(rec.render, params, seed, member=member, mode=mode, gain=gain)


# ---------------------------------------------------------------- family box

def _log_box(spec: tuple, v: float, box: float) -> tuple | None:
    lo, hi = float(spec[0]), float(spec[1])
    if lo <= 0:
        return None
    w = box * (math.log(hi) - math.log(lo))
    c = math.log(min(max(float(v), lo), hi))
    # Clamp after exp: exp(log(hi)) can round past hi.
    return max(lo, math.exp(max(math.log(lo), c - w))), min(hi, math.exp(min(math.log(hi), c + w))), "log"


def _lin_box(spec: tuple, v: float, box: float) -> tuple:
    lo, hi = float(spec[0]), float(spec[1])
    w = box * (hi - lo)
    out = (max(lo, float(v) - w), min(hi, float(v) + w))
    return out + ("int",) if len(spec) > 2 and spec[2] == "int" else out


def _boxable(k: str, spec: Any, params: dict, keys: Sequence[str] | None) -> bool:
    return not (isinstance(spec, list) or k not in params or (keys is not None and k not in keys))


def _box_of(spec: tuple, v: float, box: float) -> tuple | None:
    kind = spec[2] if len(spec) > 2 else "lin"
    return _log_box(spec, v, box) if kind == "log" else _lin_box(spec, v, box)


def family_box(recipe: Any, params: dict, box: float = 0.15, keys: Sequence[str] | None = None) -> dict:
    """A narrow search space around one preset."""
    out = {}
    for k, spec in recipe.SPACE.items():
        b = _box_of(spec, params[k], box) if _boxable(k, spec, params, keys) else None
        if b is not None and b[0] < b[1]:
            out[k] = b
    return out


# ---------------------------------------------------------------- forge: setup

def _make_critic(rec: Any, o: dict) -> Any:
    from hark.forge.engine.critic import Critic

    refs = o["refs"]
    if refs or o["refset"]:
        refs = resolve_refs(refs, o["refset"], rec, o["log"])
    return Critic(o["target"] or rec.TARGET, o["alts"] or list(getattr(rec, "ALTS", None) or []), refs=refs,
                  tone=o["tone"], tone_target=o["tone_target"], weights=o["weights"], mode=o["mode"],
                  category=o["refset"] or getattr(rec, "REFSET", None))


def _starts(rec: Any, o: dict, critic: Any) -> tuple[list[dict], int]:
    starts = [dict(p) for p in o["init"] or []]
    n_bank = 0
    if o["bank"] and _is_real(rec):
        got = search.load_seeds(rec, critic.target, getattr(critic, "refs", None), folder=o["bank_dir"])
        starts += got
        n_bank = len(got)
    return starts, n_bank


def _optimize(rec: Any, critic: Any, o: dict, starts: list[dict], pre: Any) -> dict:
    return search.optimize(rec, critic, budget=o["budget"], pop=o["pop"], seed=o["seed"], init=starts or None,
                           space=o["space"], sigma0=o["sigma0"], sigma1=o["sigma1"], reps=o["reps"],
                           strategy=o["strategy"], prescreen=o["prescreen"], workers=o["workers"], pre=pre,
                           log=o["log"])


# ---------------------------------------------------------------- forge: picks

def _pool(res: dict) -> list[dict]:
    fin = [h for h in res["history"] if math.isfinite(h["score"]["total"])]
    top = sorted(fin, key=lambda h: -h["score"]["total"])[:POOL_HISTORY]
    return list(res["best"]) + top


def _robust_key(p: dict) -> tuple:
    rb = p.get("robust") or {}
    mean = rb.get("mean")
    return (not (rb.get("pass_rate") or 0) >= 0.5, -(mean if mean is not None else -1e9))


def _pick_rule(o: dict, s: int) -> str:
    keep, rob = o["keep"], o["robust"]
    if rob > 0 and s > 1:
        return (f"{keep * s} diverse candidates were re-rendered on {rob} new seeds; the {keep} best robust means "
                "were kept, those passing on at least half the seeds first.")
    rule = "The strongest candidates with different parameters and sound were kept, passing ones first."
    return rule + (f" Each was re-rendered on {rob} new seeds (robust stats)." if rob > 0 else "")


def _choose(rec: Any, critic: Any, res: dict, o: dict) -> tuple[list[dict], str]:
    s = max(1, o["shortlist"]) if o["robust"] > 0 else 1
    picks = [dict(p) for p in search.diverse(_pool(res), rec, k=o["keep"] * s)]
    if o["robust"] > 0:
        for p in picks:
            p["robust"] = search.robust(rec, critic, p["params"], seeds=o["robust"], workers=o["workers"])
        if s > 1:
            picks = sorted(picks, key=_robust_key)[: o["keep"]]
    return picks, _pick_rule(o, s)


# ---------------------------------------------------------------- measurement

def _art_list(d: dict) -> list[str]:
    return sorted((d.get("art") or {}).get("flags") or {})


def _ref_range(pre: Any) -> tuple[list, list]:
    t40 = [d["env"]["t40_ms"] for d in pre.descs]
    ln = [len(a) / SR for a in pre.arrays]
    return [round(min(t40)), round(max(t40))], [round(min(ln), 3), round(max(ln), 3)]


def _measured(pre: Any, x: np.ndarray) -> tuple[dict, int | None]:
    row = pre.row(x)
    d = row["desc"]
    j = pre.nearest(d)
    t40_ref, len_ref = _ref_range(pre)
    m = {"tone_db": row.get("tone_db"), "tone_ratio": row.get("tone_ratio"), "ref_spread": round(pre.spread, 2),
         "env_z": row.get("env_z"), "t40_ms": d["env"]["t40_ms"], "length_s": round(len(x) / SR, 3),
         "t40_ref": t40_ref, "length_ref": len_ref, "nearest_ref": _base(pre.refs[j]) if j is not None else None,
         "nearest_dist": round(search.sound_dist(d, pre.descs[j]), 2) if j is not None else None,
         "art": _art_list(d)}
    return m, j


def _holdouts(critic: Any, xs: list, on: bool) -> list:
    if not on or not hasattr(critic, "holdout"):
        return [None] * len(xs)
    live = [i for i, x in enumerate(xs) if x is not None]
    got = critic.holdout([xs[i] for i in live]) if live else []
    out: list = [None] * len(xs)
    for i, h in zip(live, got):
        out[i] = h
    return out


def _score_row(r: dict) -> dict:
    return {k: v for k, v in r.items() if k != "i"}


def _pick_item(rec: Any, name: str, i: int, p: dict, ctx: dict) -> dict:
    x, mode = ctx["X"][i], ctx["mode"]
    item = {"name": f"{name}_{i + 1:02d}", "params": p["params"], "seed": p["seed"], "score": _score_row(p["score"]),
            "render": render_block(rec, p["params"], p["seed"], mode=mode)}
    if ctx["H"][i] is not None:
        item["holdout"] = ctx["H"][i]
    if "robust" in p:
        item["robust"] = p["robust"]
    if x is not None and ctx["pre"].descs:
        item["measured"], ctx["pairs"][i] = _measured(ctx["pre"], x)
    return item


def _write_wavs(out: Path | None, items: list[dict], X: list) -> None:
    if out is None:
        return
    from hark.core.audio import write_wav

    out.mkdir(parents=True, exist_ok=True)
    for item, x in zip(items, X):
        if x is not None:
            item["file"] = write_wav(out / f"{item['name']}.wav", x, SR, "PCM_24")


# ---------------------------------------------------------------- reference scores, summary, gap

def _tone_loo(P: np.ndarray, i: int) -> float:
    others = np.delete(P, i, 0)
    return round(float(np.sqrt(np.mean((P[i] - np.median(others, axis=0)) ** 2))), 2)


def _ref_row(r: dict, i: int, P: np.ndarray | None, pre: Any, H: list) -> dict:
    row = {"file": r.get("file"), "alt_zs": r.get("alt_zs"), **{k: r.get(k) for k in SCORE_KEYS}}
    if P is not None and i < len(P) and len(P) > 1:
        row.update(tone_loo_db=_tone_loo(P, i), t40_ms=pre.descs[i]["env"]["t40_ms"],
                   length_s=round(len(pre.arrays[i]) / SR, 3))
    if i < len(H) and H[i] is not None:
        row["holdout"] = H[i]
    return row


def _ref_rows(critic: Any, pre: Any, H: list) -> list[dict]:
    if not getattr(critic, "refs", None) or not hasattr(critic, "score_refs"):
        return []
    P = np.array([d["profile"] for d in pre.descs]) if pre.descs else None
    return [_ref_row(r, i, P, pre, H) for i, r in enumerate(critic.score_refs())]


def _fmt(v: Any, nd: int = 2) -> str:
    return "-" if v is None or (isinstance(v, float) and not math.isfinite(v)) else f"{v:.{nd}f}" \
        if isinstance(v, float) else str(v)


def _summary(name: str, target: str, items: list[dict], ref_rows: list[dict]) -> str:
    n_pass = sum(1 for p in items if p["score"].get("passes"))
    s0 = items[0]["score"] if items else {}
    s = (f"forge {name}: '{target}' - {n_pass} of {len(items)} kept sounds pass (best total {_fmt(s0.get('total'))}, "
         f"match z {_fmt(s0.get('match_z'))}, p_real {_fmt(s0.get('p_real'))})")
    tots = [r["total"] for r in ref_rows if isinstance(r.get("total"), (int, float)) and math.isfinite(r["total"])]
    if tots:
        s += f"; the real references score up to {max(tots):.2f} with the same critic"
    return s + " (references scored leave-one-out)."


def _gap(items: list[dict], ref_rows: list[dict]) -> str | None:
    mine = [p["measured"]["tone_db"] for p in items if (p.get("measured") or {}).get("tone_db") is not None]
    real = [r["tone_loo_db"] for r in ref_rows if r.get("tone_loo_db") is not None]
    if not mine or not real:
        return None
    return (f"Tone distance to the refs' median: picks {min(mine):.1f}-{max(mine):.1f} dB, real refs "
            f"{min(real):.1f}-{max(real):.1f} dB; a pick inside the ref range is as typical as a real recording.")


# ---------------------------------------------------------------- forge: report

def _critic_info(critic: Any) -> dict:
    return {"version": getattr(critic, "version", None), "mode": getattr(critic, "mode", None),
            "weights": dict(getattr(critic, "W", None) or {})}


def _tone_weight(critic: Any) -> Any:
    if (getattr(critic, "version", 0) or 0) >= 2:
        return (getattr(critic, "W", None) or {}).get("tone")
    return getattr(critic, "tone", None)


def _history(res: dict) -> list[dict]:
    out = []
    for h in res["history"]:
        row = {"gen": h["gen"], "params": h["params"], "seed": h["seed"],
               "score": {k: h["score"].get(k) for k in SCORE_KEYS}}
        if "pre" in h:
            row["pre"] = h["pre"]
        out.append(row)
    return out


def _report(name: str, critic: Any, o: dict, res: dict, n_bank: int) -> dict:
    r = {"schema": schema.SCHEMA, "recipe": name, "target": critic.target, "alts": list(critic.alts),
         "refs": list(getattr(critic, "refs", None) or []), "refset": o["refset"], "budget": o["budget"],
         "seed": o["seed"], "pop": o["pop"], "reps": o["reps"], "strategy": o["strategy"],
         "tone_weight": _tone_weight(critic), "tone_target": getattr(critic, "tone_target", None),
         "critic": _critic_info(critic), "prescreen": res.get("prescreen", 0), "restarts": res.get("restarts", 0),
         "n_bank": n_bank, "generations": res["generations"], "gens": res["gens"], "timing": dict(res["timing"])}
    if o["space"] is not None:
        r["space"] = o["space"]
    if res.get("fill_warnings"):
        r["fill_warnings"] = res["fill_warnings"]
    return r


def _write_outputs(out: Path, r: dict, items: list[dict], X: list, pre: Any, pairs: list) -> None:
    from hark.forge.engine import viz

    made = [(p["name"], x) for p, x in zip(items, X) if x is not None]
    mpairs = [j for j, x in zip(pairs, X) if x is not None]
    refs = [(_base(p), a) for p, a in zip(pre.refs, pre.arrays)]
    files: dict = {}
    try:
        files["compare"] = viz.compare_png(out / "compare.png", made, refs, pairs=mpairs, title=r["summary"])["file"]
    except Exception as e:  # noqa: BLE001 - a plotting error must not lose the run
        files["compare_error"] = f"{type(e).__name__}: {e}"
    files["audition"] = audition(out / "audition.wav", made, refs, mpairs)["file"]
    files["report_md"] = str(out / "report.md")
    r["files"] = files
    (out / "report.md").write_text(report_md(r))


def _save_bank(rec: Any, critic: Any, o: dict, items: list[dict]) -> None:
    if not (o["bank"] and _is_real(rec) and items):
        return
    src = f"forge {o['strategy']} budget {o['budget']} seed {o['seed']}" + (f" refset {o['refset']}"
                                                                         if o["refset"] else "")
    try:
        search.save_seeds(rec, items, critic.target, getattr(critic, "refs", None), source=src, folder=o["bank_dir"])
    except OSError as e:
        if o["log"]:
            o["log"](f"seed bank not updated: {e}")


def _finish(r: dict, o: dict, t0: float, items: list[dict], X: list, pre: Any, pairs: list) -> None:
    r["timing"]["forge_s"] = round(time.time() - t0, 1)
    out = Path(o["out"]) if o["out"] is not None else None
    if out is None:
        return
    out.mkdir(parents=True, exist_ok=True)
    if o["outputs"]:
        _write_outputs(out, r, items, X, pre, pairs)
    schema.write_json(out / "report.json", r)


# ---------------------------------------------------------------- forge

def _render_picks(rec: Any, critic: Any, picks: list[dict], o: dict, pre: Any) -> tuple[list[dict], dict, list]:
    """Render the picks, take holdouts and write the WAVs: (items, pick context, ref holdouts)."""
    name = _name(rec)
    X = search.render_many(rec, [(p["params"], p["seed"]) for p in picks], o["workers"])
    ctx = {"X": X, "H": _holdouts(critic, X, o["holdout"]), "pre": pre, "pairs": [None] * len(picks),
           "mode": getattr(critic, "mode", None)}
    ref_h = _holdouts(critic, list(getattr(critic, "ref_audio", None) or []), o["holdout"])
    items = [_pick_item(rec, name, i, p, ctx) for i, p in enumerate(picks)]
    _write_wavs(Path(o["out"]) if o["out"] is not None else None, items, X)
    return items, ctx, ref_h


def _add_picks(r: dict, critic: Any, items: list[dict], rule: str, ref_rows: list[dict]) -> None:
    gap = _gap(items, ref_rows)
    r.update(pick_rule=rule, picks=items, ref_scores=ref_rows, ref_scores_loo=True,
             summary=_summary(r["recipe"], critic.target, items, ref_rows) + (f" {gap}" if gap else ""))
    if gap:
        r["gap"] = gap


def _run(rec: Any, critic: Any, o: dict, t0: float) -> dict:
    starts, n_bank = _starts(rec, o, critic)
    pre = search.prescreen_for(critic)
    res = _optimize(rec, critic, o, starts, pre)
    picks, rule = _choose(rec, critic, res, o)
    items, ctx, ref_h = _render_picks(rec, critic, picks, o, pre)
    r = _report(_name(rec), critic, o, res, n_bank)
    if any(p["render"] is None for p in items):
        r.pop("schema")
    _add_picks(r, critic, items, rule, _ref_rows(critic, pre, ref_h))
    r["history"] = _history(res)
    _finish(r, o, t0, items, ctx["X"], pre, ctx["pairs"])
    _save_bank(rec, critic, o, items)
    return r


FORGE_DEFAULTS: dict[str, Any] = dict(
    target=None, alts=None, refs=None, budget=160, pop=16, keep=4, seed=0, out=None, log=print, init=None,
    holdout=True, robust=0, space=None, sigma0=0.22, sigma1=0.04, critic=None, tone=0.0, tone_target=None, reps=1,
    strategy="es", prescreen=0, workers=None, bank=False, shortlist=1, refset=None, outputs=True, bank_dir=None,
    weights=None, mode="auto", families=None, box=0.15)


def forge(recipe: Any, *args: Any, **opts: Any) -> dict:
    """Search a recipe against a critic; return the run report.

    Options and defaults are FORGE_DEFAULTS, in signature order.
    """
    t0 = time.time()
    o = {"recipe": recipe, **bind("forge", FORGE_DEFAULTS, args, opts), "t0": t0}
    rec = load_recipe(recipe) if isinstance(recipe, str) else recipe
    critic = o["critic"] if o["critic"] is not None else _make_critic(rec, o)
    if o["families"] is not None:
        return _forge_families(rec, o["families"], critic, o, t0)
    return _run(rec, critic, o, t0)


with_signature(forge, ("recipe",), FORGE_DEFAULTS)


# ---------------------------------------------------------------- families

_SPEC_KEYS = ("init", "space", "fixed", "budget")


def _family_items(families: Any) -> list[tuple[str, dict]]:
    return list(families.items()) if isinstance(families, dict) else [tuple(f) for f in families]


def _member_parts(spec: dict) -> tuple[dict, dict, Any, Any]:
    """(init, fixed, space, budget) of a family spec; a plain dict is the init alone."""
    if not any(k in spec for k in _SPEC_KEYS):
        return dict(spec), {}, None, None
    return dict(spec.get("init") or {}), dict(spec.get("fixed") or {}), spec.get("space"), spec.get("budget")


def _member_setup(rec: Any, spec: dict, box: float) -> tuple[dict, dict, dict | None]:
    init, fixed, sp, budget = _member_parts(spec)
    base = {**rec.DEFAULTS, **init, **fixed}
    sp = dict(sp) if sp else family_box(rec, base, box)
    for k in fixed:
        sp.pop(k, None)
    return base, sp, budget


def _stand_in(rec: Any, base: dict) -> Any:
    attrs = {k: getattr(rec, k) for k in ("TARGET", "ALTS", "REFSET", "REFS") if hasattr(rec, k)}
    return types.SimpleNamespace(__name__=getattr(rec, "__name__", "recipe"), SPACE=rec.SPACE, DEFAULTS=base,
                                 render=rec.render, **attrs)


def _member_run(rec: Any, critic: Any, o: dict, k: int, fname: str, spec: dict) -> dict:
    base, sp, budget = _member_setup(rec, spec, o["box"])
    sub = Path(o["out"]) / "runs" / str(fname) if o["out"] is not None else None
    run = {k2: o[k2] for k2 in ("pop", "sigma0", "sigma1", "reps", "strategy", "prescreen", "workers", "robust",
                                 "shortlist", "refset", "log")}
    return forge(_stand_in(rec, base), critic=critic, space=sp, init=[base], keep=1, budget=budget or o["budget"],
                 seed=o["seed"] + 101 * k, out=sub, holdout=False, outputs=False, bank=False, **run)


def _family_member(picks: list[dict], fname: str) -> dict | None:
    if not picks:
        return None
    m = {"params": picks[0]["params"], "seed": picks[0]["seed"], "family": fname}
    if "robust" in picks[0]:
        m["robust"] = picks[0]["robust"]
    return m


def _family_run(o: dict, fname: str, r: dict, picks: list[dict]) -> dict:
    return {"family": fname, "out": str(Path(o["out"]) / "runs" / str(fname)) if o["out"] else None,
            "budget": r["budget"], "timing": r["timing"], "total": picks[0]["score"]["total"] if picks else None}


def _forge_families(rec: Any, families: Any, critic: Any, o: dict, t0: float) -> dict:
    o = {"budget": FAMILY_BUDGET, **o}
    members, runs = [], []
    for k, (fname, spec) in enumerate(_family_items(families)):
        r = _member_run(rec, critic, o, k, fname, spec)
        picks = r.get("picks") or []
        m = _family_member(picks, fname)
        members += [m] if m is not None else []
        runs.append(_family_run(o, fname, r, picks))
    rep = palette(rec, members, critic, out=o["out"], name=o["refset"], holdout=o["holdout"], log=o["log"],
                  outputs=o["outputs"])
    rep["runs"] = runs
    rep["timing"] = {"forge_s": round(time.time() - t0, 1)}
    if o["out"] is not None:
        schema.write_json(Path(o["out"]) / "report.json", rep)
    return rep


# ---------------------------------------------------------------- palette

def _member(rec: Any, m: dict) -> tuple[dict, int, dict]:
    own = m["params"] if isinstance(m.get("params"), dict) else m
    meta = m if isinstance(m.get("params"), dict) else {}
    return {**rec.DEFAULTS, **own}, int(meta.get("seed") or 0), meta


def _gains(X: list[np.ndarray], loud: dict, on: bool, peak_db: float) -> tuple[list[float], float]:
    lufs = loud["lufs"]
    g = [10 ** ((min(lufs) - v) / 20) for v in lufs] if on and lufs else [1.0] * len(X)
    peak = max((float(np.max(np.abs(x))) * gi for x, gi in zip(X, g)), default=1.0)
    return g, 10 ** (peak_db / 20) / max(peak, 1e-12)


def _cos_stats(E: Any) -> dict | None:
    if E is None or len(E) < 2:
        return None
    E = np.asarray(E, dtype=float)
    U = E / (np.linalg.norm(E, axis=1, keepdims=True) + 1e-12)
    C = U @ U.T
    off = C[~np.eye(len(C), dtype=bool)]
    return {"cos_mean": round(float(off.mean()), 3), "cos_max": round(float(off.max()), 3)}


def _tone_variety(Y: list[np.ndarray]) -> tuple[float | None, float | None]:
    P = [tone.profile(y) for y in Y]
    d = [float(np.sqrt(np.mean((P[i] - P[j]) ** 2))) for i in range(len(P)) for j in range(i + 1, len(P))]
    return (round(min(d), 2), round(float(np.mean(d)), 2)) if d else (None, None)


def _embed(critic: Any, Y: list) -> Any:
    if hasattr(critic, "embed") and len(Y) >= 2:
        return critic.embed(Y)
    return None


def _variety(critic: Any, Y: list, E: Any) -> dict:
    tmin, tmean = _tone_variety(Y)
    A = getattr(critic, "A", None)
    return {"members": _cos_stats(E), "refs": _cos_stats(A) if getattr(critic, "refs", None) else None,
            "tone_min_db": tmin, "tone_mean_db": tmean}


def _render_members(rec: Any, members: Sequence[dict], workers: int | None, log: Callable | None) -> list[tuple]:
    sets = [_member(rec, m) for m in members]
    X = search.render_many(rec, [(p, s) for p, s, _ in sets], workers)
    bad = [i for i, x in enumerate(X) if x is None]
    if bad and log:
        log(f"palette: members {', '.join(map(str, bad))} failed or are silent and were removed")
    return [(p, s, meta, x) for (p, s, meta), x in zip(sets, X) if x is not None]


def _member_name(meta: dict) -> Any:
    return next((meta[k] for k in ("family", "member", "preset") if meta.get(k) is not None), None)


def _palette_measured(pre: Any, y: np.ndarray) -> dict | None:
    if not pre.descs:
        return None
    m, _ = _measured(pre, y)
    return {k: m[k] for k in ("tone_db", "t40_ms", "length_s", "nearest_ref", "nearest_dist")}


def _palette_item(rec: Any, k: int, live: tuple, y: np.ndarray, ctx: dict) -> dict:
    p, s, meta, _ = live
    g = ctx["g"][k] * ctx["c"]
    item = {"name": meta.get("name") or f"{ctx['category']}_{k + 1:02d}", "family": meta.get("family"),
            "params": p, "seed": s, "gain_db": round(20 * math.log10(g), 2), "score": _score_row(ctx["rows"][k]),
            "render": render_block(rec, p, s, member=_member_name(meta), mode=ctx["mode"], gain=g)}
    rb = meta.get("robust")
    if rb is None and ctx["robust"] > 0:
        rb = search.robust(rec, ctx["critic"], p, seeds=ctx["robust"], workers=ctx["workers"])
    if rb is not None:
        item["robust"] = rb
    if ctx["H"][k] is not None:
        item["holdout"] = ctx["H"][k]
    item["measured"] = _palette_measured(ctx["pre"], y)
    return item


def _palette_summary(cat: str, items: list[dict], var: dict, loud: dict) -> str:
    n_pass = sum(1 for p in items if p["score"].get("passes"))
    cm = (var.get("members") or {}).get("cos_mean")
    cr = (var.get("refs") or {}).get("cos_mean")
    return (f"palette {cat}: {len(items)} members, {n_pass} pass; variety cos {_fmt(cm, 3)} (refs {_fmt(cr, 3)}), "
            f"closest pair {_fmt(var.get('tone_min_db'))} dB apart (1/3 oct); loudness spread "
            f"{_fmt(loud['before']['spread_lu'])} -> {_fmt(loud['after']['spread_lu'])} LU")


def _palette_files(out: Path, r: dict, Y: list, pre: Any, outputs: bool) -> None:
    from hark.core.audio import write_wav

    out.mkdir(parents=True, exist_ok=True)
    for item, y in zip(r["picks"], Y):
        item["file"] = write_wav(out / f"{item['name']}.wav", y, SR, "PCM_24")
    made = [(p["name"], y) for p, y in zip(r["picks"], Y)]
    refs = [(_base(p), a) for p, a in zip(pre.refs, pre.arrays)]
    pairs = [pre.nearest(search.describe(y)) for y in Y]
    files = {"audition": audition(out / "_audition.wav", made, refs, pairs, keep_made_levels=True)["file"]}
    if outputs:
        _palette_pictures(out, r, made, refs, pairs, files)
    r["files"] = files
    schema.write_json(out / "report.json", r)


def _palette_pictures(out: Path, r: dict, made: list, refs: list, pairs: list, files: dict) -> None:
    from hark.forge.engine import viz

    try:
        files["compare"] = viz.compare_png(out / "compare.png", made, refs, pairs=pairs, title=r["summary"])["file"]
    except Exception as e:  # noqa: BLE001 - a plotting error must not lose the palette
        files["compare_error"] = f"{type(e).__name__}: {e}"
    files["report_md"] = str(out / "report.md")
    (out / "report.md").write_text(palette_md(r))


def _level(x: np.ndarray, gain: float) -> np.ndarray:
    return (np.asarray(x, dtype=np.float64) * gain).astype(np.float32)


PALETTE_DEFAULTS: dict[str, Any] = dict(out=None, name=None, peak_db=-1.0, loudness=True, robust=0, workers=None,
                                        holdout=True, log=print, outputs=True)


def _level_members(rec: Any, members: Sequence[dict], a: dict) -> tuple[list, list, dict, tuple]:
    """Render the members and level them: (live renders, levelled audio, loudness before, (gains, common gain))."""
    live = _render_members(rec, members, a["workers"], a["log"])
    X = [v[3] for v in live]
    before = polish.loudness_spread(X)
    g, c = _gains(X, before, a["loudness"], a["peak_db"])
    return live, [_level(x, gi * c) for x, gi in zip(X, g)], before, (g, c)


def _palette_ctx(rec: Any, critic: Any, Y: list, E: Any, gc: tuple, a: dict) -> dict:
    rows = critic.score(Y, E=E) if Y else []
    cat = a["name"] or getattr(rec, "REFSET", None) or _name(rec)
    return {"g": gc[0], "c": gc[1], "rows": rows, "category": cat, "mode": getattr(critic, "mode", None),
            "robust": a["robust"], "critic": critic, "workers": a["workers"], "H": _holdouts(critic, Y, a["holdout"]),
            "pre": search.prescreen_for(critic)}


def _palette_report(rec: Any, critic: Any, ctx: dict, items: list[dict], var: dict, loud: dict,
                    peak_db: float) -> dict:
    cat = ctx["category"]
    r = {"schema": schema.SCHEMA, "recipe": _name(rec), "category": cat, "target": critic.target,
         "refs": list(getattr(critic, "refs", None) or []),
         "critic": {"version": getattr(critic, "version", None), "mode": ctx["mode"], "category": cat},
         "pick_rule": "The exact member set, no diversity drop; each member matched to the quietest member's "
                      f"loudness, then one common gain puts the loudest peak at {peak_db:g} dBFS.",
         "variety": var, "loudness": loud, "picks": items, "summary": _palette_summary(cat, items, var, loud)}
    if any(p["render"] is None for p in items):
        r.pop("schema")
    return r


def palette(recipe: Any, members: Sequence[dict], critic: Any, *args: Any, **opts: Any) -> dict:
    """Score an exact member set together and write it at matched loudness.

    Options and defaults are PALETTE_DEFAULTS, in signature order.
    """
    a = bind("palette", PALETTE_DEFAULTS, args, opts)
    rec = load_recipe(recipe) if isinstance(recipe, str) else recipe
    live, Y, before, gc = _level_members(rec, members, a)
    E = _embed(critic, Y)
    ctx = _palette_ctx(rec, critic, Y, E, gc, a)
    items = [_palette_item(rec, k, v, y, ctx) for k, (v, y) in enumerate(zip(live, Y))]
    var = _variety(critic, Y, E)
    loud = {"before": before, "after": polish.loudness_spread(Y)}
    r = _palette_report(rec, critic, ctx, items, var, loud, a["peak_db"])
    if a["out"] is not None:
        _palette_files(Path(a["out"]), r, Y, ctx["pre"], a["outputs"])
    if a["log"]:
        a["log"](r["summary"])
    return r


with_signature(palette, ("recipe", "members", "critic"), PALETTE_DEFAULTS)


# ---------------------------------------------------------------- render without a search

def _dict_sets(params: dict) -> list[tuple[dict, dict | None, int | None]]:
    if "picks" in params:
        res = [(dict(p.get("params") or {}), p, p.get("seed")) for p in params["picks"]]
    elif "entries" in params:
        res = [(dict(e["params"]), None, None) for e in params["entries"]]
    else:
        res = [(dict(params), None, params.get("seed") if isinstance(params.get("seed"), int) else None)]
    return res


def _sets_of(params: Any) -> list[tuple[dict, dict | None, int | None]]:
    """[(params, pick or None, own seed)] of a report, seed bank, dict or list."""
    if isinstance(params, dict):
        return _dict_sets(params)
    return [(dict(p), None, None) for p in params]


def _pick_render(rec: Any, report: dict, pick: dict) -> tuple[Any, dict] | None:
    b = schema.block(report, pick)
    if not isinstance(pick.get("render"), dict) and b.get("recipe") != _name(rec):
        return None
    try:
        fn = schema.resolve(b)
    except (schema.NotReproducible, KeyError, TypeError, ValueError):
        return None
    return types.SimpleNamespace(__name__=b["recipe"], render=fn), dict(b["params"])


def _render_one(rec: Any, report: Any, p: dict, pick: dict | None, seed: int) -> np.ndarray | None:
    if pick is not None:
        got = _pick_render(rec, report, pick)
        if got is not None:
            x = search.render(got[0], got[1], seed)
            if x is not None:
                return x
    return search.render(rec, {**rec.DEFAULTS, **p}, seed)


def _file_measured(pre: Any, x: np.ndarray) -> dict:
    m, _ = _measured(pre, x)
    return {k: m[k] for k in ("tone_db", "tone_ratio", "t40_ms", "art", "nearest_ref")}


def _param_file(rec: Any, params: Any, job: tuple, base: Path, pre: Any) -> dict:
    from hark.core.audio import write_wav

    n, p, pick, s = job
    x = _render_one(rec, params, p, pick, s)
    row: dict = {"set": n, "seed": s, "file": None}
    if x is not None:
        row["file"] = write_wav(base / f"{_name(rec)}_r{n:02d}_s{s}.wav", x, SR, "PCM_24")
        if pre is not None and pre.descs:
            row["measured"] = _file_measured(pre, x)
    return row


def render_params(recipe: Any, params: Any, seeds: Sequence[int] | None = None, out: Any = ".",
                  refs: Any = None) -> dict:
    """Render param sets without a search; {'recipe', 'files'}."""
    rec = load_recipe(recipe) if isinstance(recipe, str) else recipe
    base = Path(out)
    base.mkdir(parents=True, exist_ok=True)
    pre = search.Prescreen(refs) if refs else None
    jobs = [(n, p, pick, int(s)) for n, (p, pick, own) in enumerate(_sets_of(params), start=1)
            for s in (list(seeds) if seeds else [own if own is not None else 0])]
    return {"recipe": _name(rec), "files": [_param_file(rec, params, j, base, pre) for j in jobs]}
