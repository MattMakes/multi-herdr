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


def _short(rc: Any) -> str:
    return str(getattr(rc, "__name__", "recipe")).rsplit(".", 1)[-1]


def _registered(rc: Any) -> bool:
    return _short(rc) in recipes()


def _leaf(pd: Any) -> str:
    return os.path.basename(str(pd).replace("\\", "/"))


# ---------------------------------------------------------------- references

def _without_held(outs: list[str], is_kit: bool, say: Callable | None) -> list[str]:
    withheld = set(refsets.held_out(outs, kit=is_kit))
    if withheld and say:
        say(f"dropped {len(withheld)} held-out ref(s): {', '.join(sorted(_leaf(pd) for pd in withheld))}")
    return [fp for fp in outs if fp not in withheld]


def _audio_files(ref_l: Any) -> list[str]:
    from hark.core.audio import list_media

    return list(list_media([ref_l] if isinstance(ref_l, (str, os.PathLike)) else list(ref_l)))


def _category_refs(rc: Any) -> tuple[list[str] | None, str | None]:
    label = getattr(rc, "REFSET", None)
    if not label:
        return None, None
    try:
        found = refsets.refset(label)
    except (refsets.RefsetsMissing, KeyError):
        return None, None
    return (found, label) if found else (None, None)


def _own_refs(rcp: Any, say: Callable | None) -> tuple[list[str] | None, str | None]:
    """The recipe's refset, else its REFS files that exist."""
    rc = load_recipe(rcp) if isinstance(rcp, str) else rcp
    found, label = _category_refs(rc)
    if found:
        return found, label
    exist = [pd for pd in getattr(rc, "REFS", None) or [] if os.path.isfile(pd)]
    return _without_held(exist, True, say) or None, None


def pick_refs(refs: Any = None, refset: str | None = None, recipe: Any = None,
              log: Callable | None = None) -> tuple[list[str] | None, str | None]:
    """(reference files or None, refset category used or None)."""
    if refset:
        result = (refsets.refset(refset), refset)
    elif refs:
        result = (_without_held(_audio_files(refs), False, log) or None, None)
    else:
        result = _own_refs(recipe, log)
    return result


def resolve_refs(refs: Any = None, refset: str | None = None, recipe: Any = None,
                 log: Callable | None = None) -> list[str] | None:
    """The reference file list of pick_refs."""
    return pick_refs(refs, refset, recipe, log)[0]


# ---------------------------------------------------------------- render blocks

class _MemberSwitch(types.SimpleNamespace):
    def render(self, p: dict, r: np.random.Generator) -> Any:
        return self.RENDER_FN(dict(self.MEMBERS[int(p["variant"])]), r)


def _home_module(func: Any) -> str:
    while isinstance(func, functools.partial):
        func = func.func
    return getattr(func, "__module__", None) or "hark.forge.recipes.variant"


def variant_recipe(render_fn: Callable, members: Sequence[dict], names: Sequence[Any] | None = None,
                   name: str | None = None, **attrs: Any) -> Any:
    """A stand-in recipe that searches one key 'variant' over fixed member param dicts."""
    meas = len(members)
    return _MemberSwitch(__name__=name or _home_module(render_fn), SPACE={"variant": list(range(meas))},
                    DEFAULTS={"variant": 0}, MEMBERS=[dict(pd) for pd in members],
                    MEMBER_NAMES=list(names) if names is not None else [None] * meas, RENDER_FN=render_fn, **attrs)


def render_block(rec: Any, params: dict, seed: int, member: Any = None, mode: str | None = None,
                 gain: float | None = None) -> dict | None:
    """The render block of one pick, or None when the render function has no importable name."""
    if set(params) == {"variant"} and getattr(rec, "MEMBERS", None) is not None:
        val = int(params["variant"])
        member = member if member is not None else rec.MEMBER_NAMES[val]
        return schema.make_block(rec.RENDER_FN, rec.MEMBERS[val], seed, member=member, mode=mode, gain=gain)
    return schema.make_block(rec.render, params, seed, member=member, mode=mode, gain=gain)


# ---------------------------------------------------------------- family box

def _geo_window(sp_v: tuple, val: float, frac: float) -> tuple | None:
    low, high = float(sp_v[0]), float(sp_v[1])
    if low <= 0:
        return None
    half = frac * (math.log(high) - math.log(low))
    centre = math.log(min(max(float(val), low), high))
    # Clamp after exp: exp(log(hi)) can round past hi.
    return max(low, math.exp(max(math.log(low), centre - half))), min(high, math.exp(min(math.log(high), centre + half))), "log"


def _flat_window(sp_v: tuple, val: float, frac: float) -> tuple:
    low, high = float(sp_v[0]), float(sp_v[1])
    half = frac * (high - low)
    acc = (max(low, float(val) - half), min(high, float(val) + half))
    return acc + ("int",) if len(sp_v) > 2 and sp_v[2] == "int" else acc


def _can_window(ky: str, sp_v: Any, prm: dict, only: Sequence[str] | None) -> bool:
    return not (isinstance(sp_v, list) or ky not in prm or (only is not None and ky not in only))


def _window(sp_v: tuple, val: float, frac: float) -> tuple | None:
    form = sp_v[2] if len(sp_v) > 2 else "lin"
    return _geo_window(sp_v, val, frac) if form == "log" else _flat_window(sp_v, val, frac)


def family_box(recipe: Any, params: dict, box: float = 0.15, keys: Sequence[str] | None = None) -> dict:
    """A narrow search space around one preset."""
    acc = {}
    for ky, sp_v in recipe.SPACE.items():
        bx = _window(sp_v, params[ky], box) if _can_window(ky, sp_v, params, keys) else None
        if bx is not None and bx[0] < bx[1]:
            acc[ky] = bx
    return acc


# ---------------------------------------------------------------- forge: setup

def _build_judge(rc: Any, cfg: dict) -> Any:
    from hark.forge.engine.critic import Critic

    ref_l = cfg["refs"]
    if ref_l or cfg["refset"]:
        ref_l = resolve_refs(ref_l, cfg["refset"], rc, cfg["log"])
    return Critic(cfg["target"] or rc.TARGET, cfg["alts"] or list(getattr(rc, "ALTS", None) or []), refs=ref_l,
                  tone=cfg["tone"], tone_target=cfg["tone_target"], weights=cfg["weights"], mode=cfg["mode"],
                  category=cfg["refset"] or getattr(rc, "REFSET", None))


def _seed_starts(rc: Any, cfg: dict, judge: Any) -> tuple[list[dict], int]:
    seeds_in = [dict(pd) for pd in cfg["init"] or []]
    from_bank = 0
    if cfg["bank"] and _registered(rc):
        found = search.load_seeds(rc, judge.target, getattr(judge, "refs", None), folder=cfg["bank_dir"])
        seeds_in += found
        from_bank = len(found)
    return seeds_in, from_bank


def _search(rc: Any, judge: Any, cfg: dict, seeds_in: list[dict], scr: Any) -> dict:
    return search.optimize(rc, judge, budget=cfg["budget"], pop=cfg["pop"], seed=cfg["seed"], init=seeds_in or None,
                           space=cfg["space"], sigma0=cfg["sigma0"], sigma1=cfg["sigma1"], reps=cfg["reps"],
                           strategy=cfg["strategy"], prescreen=cfg["prescreen"], workers=cfg["workers"], pre=scr,
                           log=cfg["log"])


# ---------------------------------------------------------------- forge: picks

def _candidates(result: dict) -> list[dict]:
    good = [hrow for hrow in result["history"] if math.isfinite(hrow["score"]["total"])]
    best = sorted(good, key=lambda hrow: -hrow["score"]["total"])[:POOL_HISTORY]
    return list(result["best"]) + best


def _by_robust(pd: dict) -> tuple:
    rob_d = pd.get("robust") or {}
    avg = rob_d.get("mean")
    return (not (rob_d.get("pass_rate") or 0) >= 0.5, -(avg if avg is not None else -1e9))


def _rule_text(cfg: dict, sd: int) -> str:
    n_keep, n_rob = cfg["keep"], cfg["robust"]
    if n_rob > 0 and sd > 1:
        return (f"{n_keep * sd} diverse candidates were re-rendered on {n_rob} new seeds; the {n_keep} best robust means "
                "were kept, those passing on at least half the seeds first.")
    why = "The strongest candidates with different parameters and sound were kept, passing ones first."
    return why + (f" Each was re-rendered on {n_rob} new seeds (robust stats)." if n_rob > 0 else "")


def _select(rc: Any, judge: Any, result: dict, cfg: dict) -> tuple[list[dict], str]:
    sd = max(1, cfg["shortlist"]) if cfg["robust"] > 0 else 1
    chosen = [dict(pd) for pd in search.diverse(_candidates(result), rc, k=cfg["keep"] * sd)]
    if cfg["robust"] > 0:
        for pd in chosen:
            pd["robust"] = search.robust(rc, judge, pd["params"], seeds=cfg["robust"], workers=cfg["workers"])
        if sd > 1:
            chosen = sorted(chosen, key=_by_robust)[: cfg["keep"]]
    return chosen, _rule_text(cfg, sd)


# ---------------------------------------------------------------- measurement

def _flag_names(desc: dict) -> list[str]:
    return sorted((desc.get("art") or {}).get("flags") or {})


def _ref_extents(scr: Any) -> tuple[list, list]:
    t40s = [desc["env"]["t40_ms"] for desc in scr.descs]
    lens = [len(opt) / SR for opt in scr.arrays]
    return [round(min(t40s)), round(max(t40s))], [round(min(lens), 3), round(max(lens), 3)]


def _measure_vs_refs(scr: Any, wav: np.ndarray) -> tuple[dict, int | None]:
    line = scr.row(wav)
    desc = line["desc"]
    jx = scr.nearest(desc)
    t40_rng, len_rng = _ref_extents(scr)
    meas = {"tone_db": line.get("tone_db"), "tone_ratio": line.get("tone_ratio"), "ref_spread": round(scr.spread, 2),
         "env_z": line.get("env_z"), "t40_ms": desc["env"]["t40_ms"], "length_s": round(len(wav) / SR, 3),
         "t40_ref": t40_rng, "length_ref": len_rng, "nearest_ref": _leaf(scr.refs[jx]) if jx is not None else None,
         "nearest_dist": round(search.sound_dist(desc, scr.descs[jx]), 2) if jx is not None else None,
         "art": _flag_names(desc)}
    return meas, jx


def _tag_all(judge: Any, wavs: list, enabled: bool) -> list:
    if not enabled or not hasattr(judge, "holdout"):
        return [None] * len(wavs)
    alive = [ix for ix, wav in enumerate(wavs) if wav is not None]
    found = judge.holdout([wavs[ix] for ix in alive]) if alive else []
    acc: list = [None] * len(wavs)
    for ix, hrow in zip(alive, found):
        acc[ix] = hrow
    return acc


def _strip_index(rep_d: dict) -> dict:
    return {ky: val for ky, val in rep_d.items() if ky != "i"}


def _pick_entry(rc: Any, nm: str, ix: int, pd: dict, state: dict) -> dict:
    wav, md = state["X"][ix], state["mode"]
    entry = {"name": f"{nm}_{ix + 1:02d}", "params": pd["params"], "seed": pd["seed"], "score": _strip_index(pd["score"]),
            "render": render_block(rc, pd["params"], pd["seed"], mode=md)}
    if state["H"][ix] is not None:
        entry["holdout"] = state["H"][ix]
    if "robust" in pd:
        entry["robust"] = pd["robust"]
    if wav is not None and state["pre"].descs:
        entry["measured"], state["pairs"][ix] = _measure_vs_refs(state["pre"], wav)
    return entry


def _save_audio(acc: Path | None, entries: list[dict], takes: list) -> None:
    if acc is None:
        return
    from hark.core.audio import write_wav

    acc.mkdir(parents=True, exist_ok=True)
    for entry, wav in zip(entries, takes):
        if wav is not None:
            entry["file"] = write_wav(acc / f"{entry['name']}.wav", wav, SR, "PCM_24")


# ---------------------------------------------------------------- reference scores, summary, gap

def _loo_tone(profs: np.ndarray, ix: int) -> float:
    rest = np.delete(profs, ix, 0)
    return round(float(np.sqrt(np.mean((profs[ix] - np.median(rest, axis=0)) ** 2))), 2)


def _ref_entry(rep_d: dict, ix: int, profs: np.ndarray | None, scr: Any, tags: list) -> dict:
    line = {"file": rep_d.get("file"), "alt_zs": rep_d.get("alt_zs"), **{ky: rep_d.get(ky) for ky in SCORE_KEYS}}
    if profs is not None and ix < len(profs) and len(profs) > 1:
        line.update(tone_loo_db=_loo_tone(profs, ix), t40_ms=scr.descs[ix]["env"]["t40_ms"],
                   length_s=round(len(scr.arrays[ix]) / SR, 3))
    if ix < len(tags) and tags[ix] is not None:
        line["holdout"] = tags[ix]
    return line


def _ref_table(judge: Any, scr: Any, tags: list) -> list[dict]:
    if not getattr(judge, "refs", None) or not hasattr(judge, "score_refs"):
        return []
    profs = np.array([desc["profile"] for desc in scr.descs]) if scr.descs else None
    return [_ref_entry(rep_d, ix, profs, scr, tags) for ix, rep_d in enumerate(judge.score_refs())]


def _num(val: Any, digits: int = 2) -> str:
    return "-" if val is None or (isinstance(val, float) and not math.isfinite(val)) else f"{val:.{digits}f}" \
        if isinstance(val, float) else str(val)


def _headline(nm: str, tgt: str, entries: list[dict], ref_tbl: list[dict]) -> str:
    passing = sum(1 for pd in entries if pd["score"].get("passes"))
    first = entries[0]["score"] if entries else {}
    sd = (f"forge {nm}: '{tgt}' - {passing} of {len(entries)} kept sounds pass (best total {_num(first.get('total'))}, "
         f"match z {_num(first.get('match_z'))}, p_real {_num(first.get('p_real'))})")
    ref_tot = [rep_d["total"] for rep_d in ref_tbl if isinstance(rep_d.get("total"), (int, float)) and math.isfinite(rep_d["total"])]
    if ref_tot:
        sd += f"; the real references score up to {max(ref_tot):.2f} with the same critic"
    return sd + " (references scored leave-one-out)."


def _gap_note(entries: list[dict], ref_tbl: list[dict]) -> str | None:
    ours = [pd["measured"]["tone_db"] for pd in entries if (pd.get("measured") or {}).get("tone_db") is not None]
    theirs = [rep_d["tone_loo_db"] for rep_d in ref_tbl if rep_d.get("tone_loo_db") is not None]
    if not ours or not theirs:
        return None
    return (f"Tone distance to the refs' median: picks {min(ours):.1f}-{max(ours):.1f} dB, real refs "
            f"{min(theirs):.1f}-{max(theirs):.1f} dB; a pick inside the ref range is as typical as a real recording.")


# ---------------------------------------------------------------- forge: report

def _judge_info(judge: Any) -> dict:
    return {"version": getattr(judge, "version", None), "mode": getattr(judge, "mode", None),
            "weights": dict(getattr(judge, "W", None) or {})}


def _tone_w(judge: Any) -> Any:
    if (getattr(judge, "version", 0) or 0) >= 2:
        return (getattr(judge, "W", None) or {}).get("tone")
    return getattr(judge, "tone", None)


def _slim_history(result: dict) -> list[dict]:
    acc = []
    for hrow in result["history"]:
        line = {"gen": hrow["gen"], "params": hrow["params"], "seed": hrow["seed"],
               "score": {ky: hrow["score"].get(ky) for ky in SCORE_KEYS}}
        if "pre" in hrow:
            line["pre"] = hrow["pre"]
        acc.append(line)
    return acc


def _base_report(nm: str, judge: Any, cfg: dict, result: dict, from_bank: int) -> dict:
    rep_d = {"schema": schema.SCHEMA, "recipe": nm, "target": judge.target, "alts": list(judge.alts),
         "refs": list(getattr(judge, "refs", None) or []), "refset": cfg["refset"], "budget": cfg["budget"],
         "seed": cfg["seed"], "pop": cfg["pop"], "reps": cfg["reps"], "strategy": cfg["strategy"],
         "tone_weight": _tone_w(judge), "tone_target": getattr(judge, "tone_target", None),
         "critic": _judge_info(judge), "prescreen": result.get("prescreen", 0), "restarts": result.get("restarts", 0),
         "n_bank": from_bank, "generations": result["generations"], "gens": result["gens"], "timing": dict(result["timing"])}
    if cfg["space"] is not None:
        rep_d["space"] = cfg["space"]
    if result.get("fill_warnings"):
        rep_d["fill_warnings"] = result["fill_warnings"]
    return rep_d


def _emit_files(acc: Path, rep_d: dict, entries: list[dict], takes: list, scr: Any, near_ix: list) -> None:
    from hark.forge.engine import viz

    mine_l = [(pd["name"], wav) for pd, wav in zip(entries, takes) if wav is not None]
    made_near = [jx for jx, wav in zip(near_ix, takes) if wav is not None]
    ref_l = [(_leaf(pd), opt) for pd, opt in zip(scr.refs, scr.arrays)]
    outs: dict = {}
    try:
        outs["compare"] = viz.compare_png(acc / "compare.png", mine_l, ref_l, pairs=made_near, title=rep_d["summary"])["file"]
    except Exception as err:  # noqa: BLE001 - a plotting error must not lose the run
        outs["compare_error"] = f"{type(err).__name__}: {err}"
    outs["audition"] = audition(acc / "audition.wav", mine_l, ref_l, made_near)["file"]
    outs["report_md"] = str(acc / "report.md")
    rep_d["files"] = outs
    (acc / "report.md").write_text(report_md(rep_d))


def _bank_store(rc: Any, judge: Any, cfg: dict, entries: list[dict]) -> None:
    if not (cfg["bank"] and _registered(rc) and entries):
        return
    origin = f"forge {cfg['strategy']} budget {cfg['budget']} seed {cfg['seed']}" + (f" refset {cfg['refset']}"
                                                                         if cfg["refset"] else "")
    try:
        search.save_seeds(rc, entries, judge.target, getattr(judge, "refs", None), source=origin, folder=cfg["bank_dir"])
    except OSError as err:
        if cfg["log"]:
            cfg["log"](f"seed bank not updated: {err}")


def _wrap_up(rep_d: dict, cfg: dict, began: float, entries: list[dict], takes: list, scr: Any, near_ix: list) -> None:
    rep_d["timing"]["forge_s"] = round(time.time() - began, 1)
    acc = Path(cfg["out"]) if cfg["out"] is not None else None
    if acc is None:
        return
    acc.mkdir(parents=True, exist_ok=True)
    if cfg["outputs"]:
        _emit_files(acc, rep_d, entries, takes, scr, near_ix)
    schema.write_json(acc / "report.json", rep_d)


# ---------------------------------------------------------------- forge

def _render_selected(rc: Any, judge: Any, chosen: list[dict], cfg: dict, scr: Any) -> tuple[list[dict], dict, list]:
    """Render the picks, take holdouts and write the WAVs: (items, pick context, ref holdouts)."""
    nm = _short(rc)
    takes = search.render_many(rc, [(pd["params"], pd["seed"]) for pd in chosen], cfg["workers"])
    state = {"X": takes, "H": _tag_all(judge, takes, cfg["holdout"]), "pre": scr, "pairs": [None] * len(chosen),
           "mode": getattr(judge, "mode", None)}
    ref_tags = _tag_all(judge, list(getattr(judge, "ref_audio", None) or []), cfg["holdout"])
    entries = [_pick_entry(rc, nm, ix, pd, state) for ix, pd in enumerate(chosen)]
    _save_audio(Path(cfg["out"]) if cfg["out"] is not None else None, entries, takes)
    return entries, state, ref_tags


def _attach_picks(rep_d: dict, judge: Any, entries: list[dict], why: str, ref_tbl: list[dict]) -> None:
    note = _gap_note(entries, ref_tbl)
    rep_d.update(pick_rule=why, picks=entries, ref_scores=ref_tbl, ref_scores_loo=True,
             summary=_headline(rep_d["recipe"], judge.target, entries, ref_tbl) + (f" {note}" if note else ""))
    if note:
        rep_d["gap"] = note


def _single(rc: Any, judge: Any, cfg: dict, began: float) -> dict:
    seeds_in, from_bank = _seed_starts(rc, cfg, judge)
    scr = search.prescreen_for(judge)
    result = _search(rc, judge, cfg, seeds_in, scr)
    chosen, why = _select(rc, judge, result, cfg)
    entries, state, ref_tags = _render_selected(rc, judge, chosen, cfg, scr)
    rep_d = _base_report(_short(rc), judge, cfg, result, from_bank)
    if any(pd["render"] is None for pd in entries):
        rep_d.pop("schema")
    _attach_picks(rep_d, judge, entries, why, _ref_table(judge, scr, ref_tags))
    rep_d["history"] = _slim_history(result)
    _wrap_up(rep_d, cfg, began, entries, state["X"], scr, state["pairs"])
    _bank_store(rc, judge, cfg, entries)
    return rep_d


FORGE_DEFAULTS: dict[str, Any] = dict(
    target=None, alts=None, refs=None, budget=160, pop=16, keep=4, seed=0, out=None, log=print, init=None,
    holdout=True, robust=0, space=None, sigma0=0.22, sigma1=0.04, critic=None, tone=0.0, tone_target=None, reps=1,
    strategy="es", prescreen=0, workers=None, bank=False, shortlist=1, refset=None, outputs=True, bank_dir=None,
    weights=None, mode="auto", families=None, box=0.15)


def forge(recipe: Any, *args: Any, **opts: Any) -> dict:
    """Search a recipe against a critic; return the run report.

    Options and defaults are FORGE_DEFAULTS, in signature order.
    """
    began = time.time()
    cfg = {"recipe": recipe, **bind("forge", FORGE_DEFAULTS, args, opts), "t0": began}
    rc = load_recipe(recipe) if isinstance(recipe, str) else recipe
    judge = cfg["critic"] if cfg["critic"] is not None else _build_judge(rc, cfg)
    if cfg["families"] is not None:
        return _by_family(rc, cfg["families"], judge, cfg, began)
    return _single(rc, judge, cfg, began)


with_signature(forge, ("recipe",), FORGE_DEFAULTS)


# ---------------------------------------------------------------- families

_FAMILY_FIELDS = ("init", "space", "fixed", "budget")


def _family_pairs(fams: Any) -> list[tuple[str, dict]]:
    return list(fams.items()) if isinstance(fams, dict) else [tuple(fp) for fp in fams]


def _split_spec(sp_v: dict) -> tuple[dict, dict, Any, Any]:
    """(init, fixed, space, budget) of a family spec; a plain dict is the init alone."""
    if not any(ky in sp_v for ky in _FAMILY_FIELDS):
        return dict(sp_v), {}, None, None
    return dict(sp_v.get("init") or {}), dict(sp_v.get("fixed") or {}), sp_v.get("space"), sp_v.get("budget")


def _prepare_member(rc: Any, sp_v: dict, frac: float) -> tuple[dict, dict, dict | None]:
    start_p, pinned, space_d, bud = _split_spec(sp_v)
    start = {**rc.DEFAULTS, **start_p, **pinned}
    space_d = dict(space_d) if space_d else family_box(rc, start, frac)
    for ky in pinned:
        space_d.pop(ky, None)
    return start, space_d, bud


def _proxy(rc: Any, start: dict) -> Any:
    attrs = {ky: getattr(rc, ky) for ky in ("TARGET", "ALTS", "REFSET", "REFS") if hasattr(rc, ky)}
    return types.SimpleNamespace(__name__=getattr(rc, "__name__", "recipe"), SPACE=rc.SPACE, DEFAULTS=start,
                                 render=rc.render, **attrs)


def _search_member(rc: Any, judge: Any, cfg: dict, ky: int, fam: str, sp_v: dict) -> dict:
    start, space_d, bud = _prepare_member(rc, sp_v, cfg["box"])
    subdir = Path(cfg["out"]) / "runs" / str(fam) if cfg["out"] is not None else None
    opts_run = {fld: cfg[fld] for fld in ("pop", "sigma0", "sigma1", "reps", "strategy", "prescreen", "workers", "robust",
                                 "shortlist", "refset", "log")}
    return forge(_proxy(rc, start), critic=judge, space=space_d, init=[start], keep=1, budget=bud or cfg["budget"],
                 seed=cfg["seed"] + 101 * ky, out=subdir, holdout=False, outputs=False, bank=False, **opts_run)


def _winner(chosen: list[dict], fam: str) -> dict | None:
    if not chosen:
        return None
    meas = {"params": chosen[0]["params"], "seed": chosen[0]["seed"], "family": fam}
    if "robust" in chosen[0]:
        meas["robust"] = chosen[0]["robust"]
    return meas


def _run_entry(cfg: dict, fam: str, rep_d: dict, chosen: list[dict]) -> dict:
    return {"family": fam, "out": str(Path(cfg["out"]) / "runs" / str(fam)) if cfg["out"] else None,
            "budget": rep_d["budget"], "timing": rep_d["timing"], "total": chosen[0]["score"]["total"] if chosen else None}


def _by_family(rc: Any, fams: Any, judge: Any, cfg: dict, began: float) -> dict:
    cfg = {"budget": FAMILY_BUDGET, **cfg}
    mems, run_l = [], []
    for ky, (fam, sp_v) in enumerate(_family_pairs(fams)):
        rep_d = _search_member(rc, judge, cfg, ky, fam, sp_v)
        chosen = rep_d.get("picks") or []
        meas = _winner(chosen, fam)
        mems += [meas] if meas is not None else []
        run_l.append(_run_entry(cfg, fam, rep_d, chosen))
    pal = palette(rc, mems, judge, out=cfg["out"], name=cfg["refset"], holdout=cfg["holdout"], log=cfg["log"],
                  outputs=cfg["outputs"])
    pal["runs"] = run_l
    pal["timing"] = {"forge_s": round(time.time() - began, 1)}
    if cfg["out"] is not None:
        schema.write_json(Path(cfg["out"]) / "report.json", pal)
    return pal


# ---------------------------------------------------------------- palette

def _unpack(rc: Any, meas: dict) -> tuple[dict, int, dict]:
    own_p = meas["params"] if isinstance(meas.get("params"), dict) else meas
    info = meas if isinstance(meas.get("params"), dict) else {}
    return {**rc.DEFAULTS, **own_p}, int(info.get("seed") or 0), info


def _level_gains(takes: list[np.ndarray], lufs_d: dict, enabled: bool, pk_db: float) -> tuple[list[float], float]:
    lu = lufs_d["lufs"]
    gains = [10 ** ((min(lu) - val) / 20) for val in lu] if enabled and lu else [1.0] * len(takes)
    loudest = max((float(np.max(np.abs(wav))) * gn_i for wav, gn_i in zip(takes, gains)), default=1.0)
    return gains, 10 ** (pk_db / 20) / max(loudest, 1e-12)


def _cosines(embs: Any) -> dict | None:
    if embs is None or len(embs) < 2:
        return None
    embs = np.asarray(embs, dtype=float)
    unit_e = embs / (np.linalg.norm(embs, axis=1, keepdims=True) + 1e-12)
    cos = unit_e @ unit_e.T
    offdiag = cos[~np.eye(len(cos), dtype=bool)]
    return {"cos_mean": round(float(offdiag.mean()), 3), "cos_max": round(float(offdiag.max()), 3)}


def _tone_gaps(levelled: list[np.ndarray]) -> tuple[float | None, float | None]:
    profs = [tone.profile(lv) for lv in levelled]
    desc = [float(np.sqrt(np.mean((profs[ix] - profs[jx]) ** 2))) for ix in range(len(profs)) for jx in range(ix + 1, len(profs))]
    return (round(min(desc), 2), round(float(np.mean(desc)), 2)) if desc else (None, None)


def _embeddings(judge: Any, levelled: list) -> Any:
    if hasattr(judge, "embed") and len(levelled) >= 2:
        return judge.embed(levelled)
    return None


def _spread_info(judge: Any, levelled: list, embs: Any) -> dict:
    t_lo, t_avg = _tone_gaps(levelled)
    ref_emb = getattr(judge, "A", None)
    return {"members": _cosines(embs), "refs": _cosines(ref_emb) if getattr(judge, "refs", None) else None,
            "tone_min_db": t_lo, "tone_mean_db": t_avg}


def _render_set(rc: Any, mems: Sequence[dict], nthr: int | None, say: Callable | None) -> list[tuple]:
    triples = [_unpack(rc, meas) for meas in mems]
    takes = search.render_many(rc, [(pd, sd) for pd, sd, _ in triples], nthr)
    failed = [ix for ix, wav in enumerate(takes) if wav is None]
    if failed and say:
        say(f"palette: members {', '.join(map(str, failed))} failed or are silent and were removed")
    return [(pd, sd, info, wav) for (pd, sd, info), wav in zip(triples, takes) if wav is not None]


def _label_of(info: dict) -> Any:
    return next((info[ky] for ky in ("family", "member", "preset") if info.get(ky) is not None), None)


def _pal_measure(scr: Any, lv: np.ndarray) -> dict | None:
    if not scr.descs:
        return None
    meas, _ = _measure_vs_refs(scr, lv)
    return {ky: meas[ky] for ky in ("tone_db", "t40_ms", "length_s", "nearest_ref", "nearest_dist")}


def _pal_entry(rc: Any, ky: int, alive: tuple, lv: np.ndarray, state: dict) -> dict:
    pd, sd, info, _ = alive
    gains = state["g"][ky] * state["c"]
    entry = {"name": info.get("name") or f"{state['category']}_{ky + 1:02d}", "family": info.get("family"),
            "params": pd, "seed": sd, "gain_db": round(20 * math.log10(gains), 2), "score": _strip_index(state["rows"][ky]),
            "render": render_block(rc, pd, sd, member=_label_of(info), mode=state["mode"], gain=gains)}
    rob_d = info.get("robust")
    if rob_d is None and state["robust"] > 0:
        rob_d = search.robust(rc, state["critic"], pd, seeds=state["robust"], workers=state["workers"])
    if rob_d is not None:
        entry["robust"] = rob_d
    if state["H"][ky] is not None:
        entry["holdout"] = state["H"][ky]
    entry["measured"] = _pal_measure(state["pre"], lv)
    return entry


def _pal_headline(label: str, entries: list[dict], vary: dict, lufs_d: dict) -> str:
    passing = sum(1 for pd in entries if pd["score"].get("passes"))
    cos_m = (vary.get("members") or {}).get("cos_mean")
    cos_r = (vary.get("refs") or {}).get("cos_mean")
    return (f"palette {label}: {len(entries)} members, {passing} pass; variety cos {_num(cos_m, 3)} (refs {_num(cos_r, 3)}), "
            f"closest pair {_num(vary.get('tone_min_db'))} dB apart (1/3 oct); loudness spread "
            f"{_num(lufs_d['before']['spread_lu'])} -> {_num(lufs_d['after']['spread_lu'])} LU")


def _pal_write(acc: Path, rep_d: dict, levelled: list, scr: Any, pics: bool) -> None:
    from hark.core.audio import write_wav

    acc.mkdir(parents=True, exist_ok=True)
    for entry, lv in zip(rep_d["picks"], levelled):
        entry["file"] = write_wav(acc / f"{entry['name']}.wav", lv, SR, "PCM_24")
    mine_l = [(pd["name"], lv) for pd, lv in zip(rep_d["picks"], levelled)]
    ref_l = [(_leaf(pd), opt) for pd, opt in zip(scr.refs, scr.arrays)]
    near_ix = [scr.nearest(search.describe(lv)) for lv in levelled]
    outs = {"audition": audition(acc / "_audition.wav", mine_l, ref_l, near_ix, keep_made_levels=True)["file"]}
    if pics:
        _pal_pics(acc, rep_d, mine_l, ref_l, near_ix, outs)
    rep_d["files"] = outs
    schema.write_json(acc / "report.json", rep_d)


def _pal_pics(acc: Path, rep_d: dict, mine_l: list, ref_l: list, near_ix: list, outs: dict) -> None:
    from hark.forge.engine import viz

    try:
        outs["compare"] = viz.compare_png(acc / "compare.png", mine_l, ref_l, pairs=near_ix, title=rep_d["summary"])["file"]
    except Exception as err:  # noqa: BLE001 - a plotting error must not lose the palette
        outs["compare_error"] = f"{type(err).__name__}: {err}"
    outs["report_md"] = str(acc / "report.md")
    (acc / "report.md").write_text(palette_md(rep_d))


def _scaled(wav: np.ndarray, gn: float) -> np.ndarray:
    return (np.asarray(wav, dtype=np.float64) * gn).astype(np.float32)


PALETTE_DEFAULTS: dict[str, Any] = dict(out=None, name=None, peak_db=-1.0, loudness=True, robust=0, workers=None,
                                        holdout=True, log=print, outputs=True)


def _render_levelled(rc: Any, mems: Sequence[dict], opt: dict) -> tuple[list, list, dict, tuple]:
    """Render the members and level them: (live renders, levelled audio, loudness before, (gains, common gain))."""
    alive = _render_set(rc, mems, opt["workers"], opt["log"])
    takes = [val[3] for val in alive]
    pre_lufs = polish.loudness_spread(takes)
    gains, centre = _level_gains(takes, pre_lufs, opt["loudness"], opt["peak_db"])
    return alive, [_scaled(wav, gn_i * centre) for wav, gn_i in zip(takes, gains)], pre_lufs, (gains, centre)


def _pal_state(rc: Any, judge: Any, levelled: list, embs: Any, gain_pair: tuple, opt: dict) -> dict:
    tbl = judge.score(levelled, E=embs) if levelled else []
    label = opt["name"] or getattr(rc, "REFSET", None) or _short(rc)
    return {"g": gain_pair[0], "c": gain_pair[1], "rows": tbl, "category": label, "mode": getattr(judge, "mode", None),
            "robust": opt["robust"], "critic": judge, "workers": opt["workers"], "H": _tag_all(judge, levelled, opt["holdout"]),
            "pre": search.prescreen_for(judge)}


def _pal_report(rc: Any, judge: Any, state: dict, entries: list[dict], vary: dict, lufs_d: dict,
                    pk_db: float) -> dict:
    label = state["category"]
    rep_d = {"schema": schema.SCHEMA, "recipe": _short(rc), "category": label, "target": judge.target,
         "refs": list(getattr(judge, "refs", None) or []),
         "critic": {"version": getattr(judge, "version", None), "mode": state["mode"], "category": label},
         "pick_rule": "The exact member set, no diversity drop; each member matched to the quietest member's "
                      f"loudness, then one common gain puts the loudest peak at {pk_db:g} dBFS.",
         "variety": vary, "loudness": lufs_d, "picks": entries, "summary": _pal_headline(label, entries, vary, lufs_d)}
    if any(pd["render"] is None for pd in entries):
        rep_d.pop("schema")
    return rep_d


def palette(recipe: Any, members: Sequence[dict], critic: Any, *args: Any, **opts: Any) -> dict:
    """Score an exact member set together and write it at matched loudness.

    Options and defaults are PALETTE_DEFAULTS, in signature order.
    """
    opt = bind("palette", PALETTE_DEFAULTS, args, opts)
    rc = load_recipe(recipe) if isinstance(recipe, str) else recipe
    alive, levelled, pre_lufs, gain_pair = _render_levelled(rc, members, opt)
    embs = _embeddings(critic, levelled)
    state = _pal_state(rc, critic, levelled, embs, gain_pair, opt)
    entries = [_pal_entry(rc, ky, val, lv, state) for ky, (val, lv) in enumerate(zip(alive, levelled))]
    vary = _spread_info(critic, levelled, embs)
    lufs_d = {"before": pre_lufs, "after": polish.loudness_spread(levelled)}
    rep_d = _pal_report(rc, critic, state, entries, vary, lufs_d, opt["peak_db"])
    if opt["out"] is not None:
        _pal_write(Path(opt["out"]), rep_d, levelled, state["pre"], opt["outputs"])
    if opt["log"]:
        opt["log"](rep_d["summary"])
    return rep_d


with_signature(palette, ("recipe", "members", "critic"), PALETTE_DEFAULTS)


# ---------------------------------------------------------------- render without a search

def _sets_from_dict(prm: dict) -> list[tuple[dict, dict | None, int | None]]:
    if "picks" in prm:
        result = [(dict(pd.get("params") or {}), pd, pd.get("seed")) for pd in prm["picks"]]
    elif "entries" in prm:
        result = [(dict(err["params"]), None, None) for err in prm["entries"]]
    else:
        result = [(dict(prm), None, prm.get("seed") if isinstance(prm.get("seed"), int) else None)]
    return result


def _param_sets(prm: Any) -> list[tuple[dict, dict | None, int | None]]:
    """[(params, pick or None, own seed)] of a report, seed bank, dict or list."""
    if isinstance(prm, dict):
        return _sets_from_dict(prm)
    return [(dict(pd), None, None) for pd in prm]


def _block_render(rc: Any, rpt: dict, pk: dict) -> tuple[Any, dict] | None:
    bx = schema.block(rpt, pk)
    if not isinstance(pk.get("render"), dict) and bx.get("recipe") != _short(rc):
        return None
    try:
        func = schema.resolve(bx)
    except (schema.NotReproducible, KeyError, TypeError, ValueError):
        return None
    return types.SimpleNamespace(__name__=bx["recipe"], render=func), dict(bx["params"])


def _take(rc: Any, rpt: Any, pd: dict, pk: dict | None, sd0: int) -> np.ndarray | None:
    if pk is not None:
        found = _block_render(rc, rpt, pk)
        if found is not None:
            wav = search.render(found[0], found[1], sd0)
            if wav is not None:
                return wav
    return search.render(rc, {**rc.DEFAULTS, **pd}, sd0)


def _file_measure(scr: Any, wav: np.ndarray) -> dict:
    meas, _ = _measure_vs_refs(scr, wav)
    return {ky: meas[ky] for ky in ("tone_db", "tone_ratio", "t40_ms", "art", "nearest_ref")}


def _write_take(rc: Any, prm: Any, task: tuple, start: Path, scr: Any) -> dict:
    from hark.core.audio import write_wav

    num, pd, pk, sd = task
    wav = _take(rc, prm, pd, pk, sd)
    line: dict = {"set": num, "seed": sd, "file": None}
    if wav is not None:
        line["file"] = write_wav(start / f"{_short(rc)}_r{num:02d}_s{sd}.wav", wav, SR, "PCM_24")
        if scr is not None and scr.descs:
            line["measured"] = _file_measure(scr, wav)
    return line


def render_params(recipe: Any, params: Any, seeds: Sequence[int] | None = None, out: Any = ".",
                  refs: Any = None) -> dict:
    """Render param sets without a search; {'recipe', 'files'}."""
    rc = load_recipe(recipe) if isinstance(recipe, str) else recipe
    start = Path(out)
    start.mkdir(parents=True, exist_ok=True)
    scr = search.Prescreen(refs) if refs else None
    tasks = [(num, pd, pk, int(sd)) for num, (pd, pk, own_p) in enumerate(_param_sets(params), start=1)
            for sd in (list(seeds) if seeds else [own_p if own_p is not None else 0])]
    return {"recipe": _short(rc), "files": [_write_take(rc, params, jx, start, scr) for jx in tasks]}
