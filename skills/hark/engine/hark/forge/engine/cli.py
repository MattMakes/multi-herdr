"""The `hark forge` command: list recipes, search one, render params, or check output folders."""

from __future__ import annotations

import argparse
import json
from typing import Any

USAGE_RENDER = "usage: hark forge --render <recipe> --params file.json [--seeds 1 2 3] [--dir out]"


def add_cli(sub: argparse._SubParsersAction) -> None:
    """Register the forge subcommand."""
    from hark.core.report import add_common

    p = sub.add_parser("forge", help="make sounds in code and search them against a critic")
    p.add_argument("recipe", nargs="?", default=None)
    p.add_argument("--list", action="store_true")
    p.add_argument("--render", nargs="?", const="", default=None, metavar="RECIPE")
    p.add_argument("--params", default=None)
    p.add_argument("--check", nargs="+", default=None, metavar="DIR")
    p.add_argument("--seeds", nargs="+", type=int, default=None)
    p.add_argument("--target", default=None)
    p.add_argument("--alts", nargs="+", default=None)
    p.add_argument("--refs", nargs="+", default=None)
    p.add_argument("--refset", default=None)
    p.add_argument("--budget", type=int, default=160)
    p.add_argument("--pop", type=int, default=16)
    p.add_argument("--variants", "--keep", dest="keep", type=int, default=4)
    p.add_argument("--seed", type=int, default=0)
    p.add_argument("--dir", default=None)
    p.add_argument("--strategy", choices=("es", "cma"), default="es")
    p.add_argument("--prescreen", type=int, default=None)
    p.add_argument("--robust", type=int, default=3)
    p.add_argument("--shortlist", type=int, default=2)
    p.add_argument("--no-bank", action="store_true")
    p.add_argument("--workers", type=int, default=None)
    p.add_argument("--tone", type=float, default=None)
    p.add_argument("--mode", choices=("auto", "real", "designed"), default="auto")
    p.add_argument("--reps", type=int, default=1)
    p.add_argument("--tone-target", type=float, default=None)
    add_common(p)
    p.set_defaults(func=handler)


def handler(args: argparse.Namespace) -> int:
    """Dispatch: --check, then --render, then --list, else a search."""
    if args.check:
        from hark.forge.engine import schema

        return int(schema.main(list(args.check)) or 0)
    if args.render is not None:
        return _render(args)
    if args.list or not args.recipe:
        return _list()
    return _search(args)


# ---------------------------------------------------------------- --render

def _render_line(f: dict) -> str:
    if not f.get("file"):
        return "failed or silent"
    m = f.get("measured")
    if not m:
        return str(f["file"])
    return f"{f['file']}  tone {m.get('tone_db')} dB, t40 {m.get('t40_ms')} ms, nearest {m.get('nearest_ref')}"


def _render(args: argparse.Namespace) -> int:
    name = args.render or args.recipe
    if not name or not args.params:
        print(USAGE_RENDER)
        return 2
    from hark.forge.engine import loop

    with open(args.params) as fh:
        data = json.load(fh)
    rec = loop.load_recipe(name)
    refs = loop.resolve_refs(args.refs, args.refset, rec, print)
    res = loop.render_params(rec, data, seeds=args.seeds, out=args.dir or ".", refs=refs)
    for f in res["files"]:
        print(_render_line(f))
    return 0


# ---------------------------------------------------------------- --list

def _recipe_line(loop: Any, name: str) -> str:
    try:
        rec = loop.load_recipe(name)
    except Exception as e:  # noqa: BLE001 - the listing goes on
        return f"  {name}: (does not load: {type(e).__name__})"
    rs = getattr(rec, "REFSET", None)
    return f"  {name}: {getattr(rec, 'TARGET', '')}" + (f"  [refset {rs}]" if rs else "")


def _list() -> int:
    from hark.forge.engine import loop, refsets

    print("recipes (name: TARGET):")
    for name in loop.recipes():
        print(_recipe_line(loop, name))
    try:
        cats = refsets.categories()
    except refsets.RefsetsMissing:
        cats = []
    print(f"refsets: {', '.join(cats)}" if cats else "refsets: none on this machine (no refs registry)")
    return 0


# ---------------------------------------------------------------- search

def _forge_kwargs(args: argparse.Namespace, refs: Any, cat: str | None) -> dict:
    return dict(target=args.target, alts=args.alts, refs=refs, budget=args.budget, pop=args.pop, keep=args.keep,
                seed=args.seed, out=args.dir, robust=args.robust, strategy=args.strategy,
                prescreen=args.prescreen if args.prescreen is not None else 3 * args.pop, workers=args.workers,
                tone=args.tone or 0.0, weights={"tone": 0.0} if args.tone == 0 else None,
                tone_target=args.tone_target, reps=args.reps, bank=not args.no_bank, shortlist=args.shortlist,
                refset=cat, mode=args.mode)


def _search(args: argparse.Namespace) -> int:
    from hark.core.report import emit
    from hark.forge.engine import loop

    rec = loop.load_recipe(args.recipe)
    refs, cat = loop.pick_refs(args.refs, args.refset, rec, print)
    if not refs:
        print("note: no references; the critic cannot compare with real sounds")
    elif cat and not args.refset:
        print(f"refs: the recipe's refset {cat} ({len(refs)} files)")
    r = loop.forge(rec, **_forge_kwargs(args, refs, cat))
    lines = [r["summary"]] + [f"  {p['name']}: {p['score'].get('summary', '')}" for p in r["picks"]]
    if args.dir:
        lines.append(f"wrote {len(r['picks'])} WAV files and report.json to {args.dir}")
    emit({k: v for k, v in r.items() if k != "history"}, args, text="\n".join(lines))
    return 0
