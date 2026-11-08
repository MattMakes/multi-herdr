"""The `hark forge` command: list recipes, search one, render params, or check output folders."""

from __future__ import annotations

import argparse
import json
import sys
from typing import Any

USAGE_RENDER = "usage: hark forge --render <recipe> --params file.json [--seeds 1 2 3] [--dir out]"


_ARGS: list[tuple[tuple[str, ...], dict]] = [
    (("recipe",), dict(nargs="?", default=None)),
    (("--list",), dict(action="store_true")),
    (("--render",), dict(nargs="?", const="", default=None, metavar="RECIPE")),
    (("--params",), dict(default=None)),
    (("--check",), dict(nargs="+", default=None, metavar="DIR")),
    (("--seeds",), dict(nargs="+", type=int, default=None)),
    (("--target",), dict(default=None)),
    (("--alts",), dict(nargs="+", default=None)),
    (("--refs",), dict(nargs="+", default=None)),
    (("--refset",), dict(default=None)),
    (("--budget",), dict(type=int, default=160)),
    (("--pop",), dict(type=int, default=16)),
    (("--variants", "--keep"), dict(dest="keep", type=int, default=4)),
    (("--seed",), dict(type=int, default=0)),
    (("--dir",), dict(default=None)),
    (("--strategy",), dict(choices=("es", "cma"), default="es")),
    (("--prescreen",), dict(type=int, default=None)),
    (("--robust",), dict(type=int, default=3)),
    (("--shortlist",), dict(type=int, default=2)),
    (("--no-bank",), dict(action="store_true")),
    (("--workers",), dict(type=int, default=None)),
    (("--tone",), dict(type=float, default=None)),
    (("--mode",), dict(choices=("auto", "real", "designed"), default="auto")),
    (("--reps",), dict(type=int, default=1)),
    (("--tone-target",), dict(type=float, default=None)),
]


def add_cli(sub: argparse._SubParsersAction) -> None:
    """Register the forge subcommand."""
    from hark.core.report import add_common

    p = sub.add_parser("forge", help="make sounds in code and search them against a critic")
    for names, kw in _ARGS:
        p.add_argument(*names, **kw)
    add_common(p)
    p.set_defaults(func=handler)


def _check(args: argparse.Namespace) -> int:
    from hark.forge.engine import schema

    return int(schema.main(list(args.check)) or 0)


def handler(args: argparse.Namespace) -> int:
    """Dispatch: --check, then --render, then --list, else a search."""
    action = (_check if args.check else _render if args.render is not None
              else _list if args.list or not args.recipe else _search)
    return action(args)


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
    from hark.core.report import emit
    from hark.forge.engine import loop

    with open(args.params) as fh:
        data = json.load(fh)
    rec = loop.load_recipe(name)
    refs = loop.resolve_refs(args.refs, args.refset, rec, _note_printer(args))
    res = loop.render_params(rec, data, seeds=args.seeds, out=args.dir or ".", refs=refs)
    lines = [_render_line(f) for f in res["files"]]
    made = sum(1 for f in res["files"] if f.get("file"))
    summary = f"{name}: {made} of {len(res['files'])} render(s) written"
    emit({"summary": summary, "recipe": name, "files": res["files"]}, args, text="\n".join(lines))
    return 0


def _note_printer(args: argparse.Namespace) -> Any:
    """Progress notes go to stdout, or to stderr when stdout carries JSON."""
    if getattr(args, "json", False):
        return lambda msg: print(msg, file=sys.stderr)
    return print


# ---------------------------------------------------------------- --list

def _recipe_row(loop: Any, name: str) -> dict:
    try:
        rec = loop.load_recipe(name)
    except Exception as e:  # noqa: BLE001 - the listing goes on
        return {"name": name, "error": type(e).__name__}
    return {"name": name, "target": getattr(rec, "TARGET", ""), "refset": getattr(rec, "REFSET", None)}


def _recipe_line(row: dict) -> str:
    if "error" in row:
        return f"  {row['name']}: (does not load: {row['error']})"
    return f"  {row['name']}: {row['target']}" + (f"  [refset {row['refset']}]" if row["refset"] else "")


def _list(args: argparse.Namespace | None = None) -> int:
    from hark.core.report import emit
    from hark.forge.engine import loop, refsets

    rows = [_recipe_row(loop, name) for name in loop.recipes()]
    try:
        cats = refsets.categories()
    except refsets.RefsetsMissing:
        cats = []
    lines = ["recipes (name: TARGET):", *(_recipe_line(r) for r in rows),
             f"refsets: {', '.join(cats)}" if cats else "refsets: none on this machine (no refs registry)"]
    result = {"summary": f"{len(rows)} recipes, {len(cats)} refset categories", "recipes": rows, "refsets": cats}
    emit(result, args if args is not None else argparse.Namespace(), text="\n".join(lines))
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
