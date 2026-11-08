"""The `hark fit` command and its pipelines: fit a file, morph 2 files, search variations of a recipe."""

from __future__ import annotations

import argparse
import json
import os
import time
from typing import Any, Callable

import numpy as np

USAGE = ("usage: hark fit <file> [-o recipe.py] [--kind auto] | hark fit --morph A B [--amount 0.5] | "
         "hark fit --forge recipe.py [--budget 96]")
KIND_CHOICES = ("auto", "modal", "spectral", "sines", "gesture", "hybrid")
SR = 48000


# ---------------------------------------------------------------- paths and files


def _slash(p: str) -> str:
    return str(p).replace("\\", "/")


def _stem(path: str) -> str:
    return os.path.splitext(os.path.basename(str(path)))[0]


def output_paths(src: str, output: str | None = None, out_dir: str | None = None,
                 render_path: str | None = None) -> dict:
    """recipe, wav, png, model, report paths and the recipe name."""
    if output:
        folder, base = os.path.dirname(os.path.abspath(output)), _stem(output)
    else:
        folder, base = os.path.abspath(out_dir or os.getcwd()), _stem(src) + "_fit"
    os.makedirs(folder, exist_ok=True)
    p = {k: _slash(os.path.join(folder, base + ext)) for k, ext in
         (("recipe", ".py"), ("wav", ".wav"), ("png", ".png"), ("model", ".json"), ("report", ".report.json"))}
    if render_path:
        p["wav"] = _slash(os.path.abspath(render_path))
    p["name"] = base
    return p


def write_wav(path: str, y: np.ndarray) -> str:
    """A 48 kHz WAV with 32-bit float samples; the folder is made when missing."""
    import soundfile

    os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
    soundfile.write(path, np.asarray(y, dtype=np.float32), SR, subtype="FLOAT")
    return path


def _jsonable(o: Any) -> Any:
    if isinstance(o, dict):
        return {str(k): _jsonable(v) for k, v in o.items()}
    if isinstance(o, (list, tuple)):
        return [_jsonable(v) for v in o]
    if isinstance(o, np.ndarray):
        return _jsonable(o.tolist())
    if isinstance(o, np.generic):
        return o.item()
    return o


# ---------------------------------------------------------------- fit mode


def _rows(model: dict) -> list:
    rows = (model.get("fit_engine") or {}).get("candidates") or []
    return [{"candidate": r.get("candidate"), "score": r.get("score"), "fit_s": r.get("fit_s"),
             "error": r.get("error")} for r in rows]


def _code_render(paths: dict, model: dict, seed: int, **kw: Any) -> tuple[Any, np.ndarray]:
    from hark.fit.pipeline import codegen

    codegen.write_recipe(paths["recipe"], model, name=paths["name"], **kw)
    rec = codegen.load_recipe(paths["recipe"])
    return rec, rec.render_seed(seed)


def _fit_text(res: dict, src: str, kind: str) -> str:
    from hark.fit.pipeline import quality as Q

    head = f"{os.path.basename(src)}: {res['kind']}"
    if kind == "auto":
        head += f", auto chose {res['chosen']}"
    lines = [head + f", fit {res['seconds']['fit']:.1f} s", Q.text(res["quality"], res["floor"])]
    if res["floor"] and res["floor"].get("mel_db") is not None:
        lines.append("floor: the distance between 2 seeds of the same model, the best a model with noise can reach")
    lines.append(f"code_renders_fit: {res['code_renders_fit']}")
    lines += [f"wrote {k}: {v}" for k, v in res["files"].items()]
    return "\n".join(lines)


def _write_report(path: str, res: dict, model: dict) -> None:
    from hark.fit.pipeline.render import summary

    data = dict(res, fit_engine=model.get("fit_engine"), summary=summary(model))
    with open(path, "w", encoding="utf-8") as f:
        json.dump(_jsonable(data), f, indent=1, default=float)


def run_fit(path: str, kind: str = "auto", layers: tuple | None = None, output: str | None = None,
            out_dir: str | None = None, render_path: str | None = None, report: bool = False, clap: bool = False,
            describe: bool = False, seed: int = 0, target: str | None = None, fast: bool = False,
            verbose: bool = True, log: Callable = print, takes: bool = False, bed: bool = True) -> dict:
    """The fit pipeline: fit, write and load the recipe, render through it, measure, check, save."""
    from hark.fit.pipeline import auto, quality as Q

    paths = output_paths(path, output, out_dir, render_path)
    t0 = time.perf_counter()
    X = Q.load(path)
    m = auto.fit(X, kind=kind, layers=layers, clap=clap, fast=fast, name=os.path.basename(str(path)),
                 verbose=verbose and kind == "auto", seeds=auto.TAKE_SEEDS if takes else (0,), bed=bed)
    fit_s = time.perf_counter() - t0
    rec, y = _code_render(paths, m, seed, source=str(path), target=target)
    write_wav(paths["wav"], y)
    q = Q.quality(X, y, clap=report or clap, describe=describe)
    fl = Q.floor(lambda s: rec.render_seed(s), clap=False) if report else None
    _, y2 = _code_render(paths, m, seed, source=str(path), target=target, quality=q, floor=fl)
    same = bool(np.array_equal(y2, auto.render(m, seed=seed)) and np.array_equal(y2, y))
    auto.save(m, paths["model"])
    chosen = (m.get("fit_engine") or {}).get("chosen") or kind
    title = f"{os.path.basename(str(path))} -> {os.path.basename(paths['recipe'])} ({auto.kind_of(m)}"
    title += f", auto: {chosen})" if kind == "auto" else ")"
    Q.pair_png(paths["png"], X, y, title=title)
    files = {k: paths[k] for k in ("recipe", "wav", "png", "model")}
    res = {"source": _slash(path), "kind": auto.kind_of(m), "chosen": chosen, "quality": q, "floor": fl,
           "files": files, "code_renders_fit": same,
           "seconds": {"fit": round(fit_s, 2), "total": round(time.perf_counter() - t0, 2)},
           "candidates": _rows(m)}
    if report:
        files["report"] = paths["report"]
        _write_report(paths["report"], res, m)
    res["text"] = _fit_text(res, str(path), kind)
    return res


# ---------------------------------------------------------------- morph mode


def _morph_paths(path_a: str, path_b: str, output: str | None, out_dir: str | None,
                 render_path: str | None) -> dict:
    stem = f"{_stem(path_a)}__{_stem(path_b)}_morph".replace("_fit", "")
    p = output_paths(stem + ".wav", output, out_dir, render_path)
    return {k: v.replace("_morph_fit", "_morph") for k, v in p.items()}


def _pad_mono(xs: list) -> list:
    from hark.fit.pipeline._post import mono

    ms = [mono(np.asarray(x, dtype=np.float64)) for x in xs]
    n = max(len(m) for m in ms)
    return [np.pad(m, (0, n - len(m))) for m in ms]


def run_morph(path_a: str, path_b: str, amount: float = 0.5, output: str | None = None,
              out_dir: str | None = None, render_path: str | None = None, clap: bool = False, seed: int = 0,
              fast: bool = False, verbose: bool = True) -> dict:
    """The morph pipeline: fit both with one kind, write the morph recipe, render, measure against both."""
    from hark.fit.pipeline import auto, codegen, quality as Q

    paths = _morph_paths(path_a, path_b, output, out_dir, render_path)
    ma, mb, kinds = auto.fit_pair(path_a, path_b, clap=clap, fast=fast, verbose=verbose)
    mm = auto.morph_model(ma, mb, amount)
    note = (f"A morph of {os.path.basename(str(path_a))} and {os.path.basename(str(path_b))}: keep both sources' "
            "attribution and licence with this file.")
    codegen.write_recipe(paths["recipe"], mm, name=paths["name"], source=None, licence=note)
    rec = codegen.load_recipe(paths["recipe"])
    y = rec.render_seed(seed)
    write_wav(paths["wav"], y)
    Xa, Xb = Q.load(path_a), Q.load(path_b)
    vs_a, vs_b = Q.quality(Xa, y, clap=clap), Q.quality(Xb, y, clap=clap)
    pa, py, pb = _pad_mono([Xa, y, Xb])
    title = f"morph {float(amount):.2f}: {os.path.basename(str(path_a))} / {os.path.basename(str(path_b))}"
    Q.pair_png(paths["png"], pa, py, title=title, extra=(os.path.basename(str(path_b)), pb))
    auto.save(mm, paths["model"])
    files = {k: paths[k] for k in ("recipe", "wav", "png", "model")}
    res = {"a": _slash(path_a), "b": _slash(path_b), "amount": float(amount), "kind": auto.kind_of(ma),
           "kinds_tried": {k: list(v) for k, v in kinds.items()}, "vs_a": vs_a, "vs_b": vs_b, "files": files}
    lines = [f"morph {float(amount):.2f} of {os.path.basename(str(path_a))} and {os.path.basename(str(path_b))}"
             f" ({res['kind']})", "vs A: " + Q.text(vs_a), "vs B: " + Q.text(vs_b)]
    res["text"] = "\n".join(lines + [f"wrote {k}: {v}" for k, v in files.items()])
    return res


# ---------------------------------------------------------------- forge mode


def run_forge(recipe_path: str, out_dir: str | None = None, refset: str | None = None, budget: int = 96,
              pop: int = 16, variants: int = 4, seed: int = 0) -> dict:
    """The forge critic search for variations of a recipe file."""
    from hark.fit.pipeline import codegen
    from hark.forge.engine import loop

    rec = codegen.load_recipe(recipe_path)
    refset = refset or getattr(rec, "REFSET", None)
    refs = None if refset else (list(getattr(rec, "REFS", []) or []) or None)
    out = out_dir or os.path.splitext(str(recipe_path))[0] + "_forge"
    res = loop.forge(rec, refs=refs, refset=refset, budget=budget, pop=pop, keep=variants, seed=seed, out=out,
                     robust=3, prescreen=3 * pop, shortlist=2, strategy="es")
    res = {k: v for k, v in dict(res).items() if k != "history"}
    picks = res.get("picks") or res.get("variants") or []
    lines = [str(res.get("summary", ""))]
    lines += [f"{p.get('name', '?')}: {p.get('summary', p.get('score', ''))}" for p in picks if isinstance(p, dict)]
    lines.append(f"wrote the variations and the search report to {out}")
    res["text"] = "\n".join(lines)
    return res


# ---------------------------------------------------------------- CLI


def _args_fit(args: argparse.Namespace) -> dict:
    layers = tuple(s.strip() for s in args.layers.split(",") if s.strip()) if args.layers else None
    return dict(kind=args.kind, layers=layers, output=args.output, out_dir=args.dir, render_path=args.render,
                report=args.report, clap=args.clap, describe=args.describe, seed=args.seed, target=args.target,
                fast=args.fast, verbose=not args.quiet and not getattr(args, "json", False), takes=args.takes,
                bed=not args.keep_bed)


def _emit(res: dict, args: argparse.Namespace) -> int:
    from hark.core import report

    text = res.pop("text", None)
    res.setdefault("summary", (text or "").splitlines()[0] if text else "")
    report.emit(res, args, text=text)
    return 0


def handler(args: argparse.Namespace) -> int:
    """Dispatch: --forge, then --morph, then usage without a file, else the fit."""
    if args.forge:
        from hark.models import require_ml

        require_ml("fit --forge")
        return _emit(run_forge(args.forge, args.dir, args.refset, args.budget, args.pop, args.variants,
                               args.seed), args)
    if args.morph:
        return _emit(run_morph(args.morph[0], args.morph[1], args.amount, args.output, args.dir, args.render,
                               args.clap, args.seed, args.fast, not args.quiet), args)
    if not args.file:
        print(USAGE)
        return 2
    flag = next((f for f in ("clap", "describe", "report") if getattr(args, f)), None)
    if flag:
        from hark.models import require_ml

        require_ml(f"fit --{flag}")
    return _emit(run_fit(args.file, **_args_fit(args)), args)


def _add_flags(p: argparse.ArgumentParser) -> None:
    p.add_argument("file", nargs="?", help="the recording to fit")
    p.add_argument("-o", "--output", help="recipe path (.py); its folder and stem name every output")
    p.add_argument("--dir", help="output folder when -o is not given; with --forge the search folder")
    p.add_argument("--kind", default="auto", choices=KIND_CHOICES, help="model kind")
    p.add_argument("--layers", help="comma-separated hybrid layers, for example sines,modal,spectral")
    p.add_argument("--render", help="WAV path of the resynthesis")
    p.add_argument("--report", action="store_true", help="also measure the floor and CLAP; write a report")
    p.add_argument("--clap", action="store_true", help="CLAP takes part in the choice of model")
    p.add_argument("--describe", action="store_true", help="describe the original and the render")
    p.add_argument("--target", help="the critic description for the recipe")
    p.add_argument("--seed", type=int, default=0, help="seed of the written render; with --forge the search seed")
    p.add_argument("--fast", action="store_true", help="cheaper fitter settings, for previews")
    p.add_argument("--takes", action="store_true", help="score every candidate on seeds 0, 1 and 2")
    p.add_argument("--keep-bed", action="store_true", help="keep the bed and fit the unmodified recording")
    p.add_argument("--morph", nargs=2, metavar=("A", "B"), help="fit A and B with one kind and write a morph")
    p.add_argument("--amount", type=float, default=0.5, help="with --morph: 0 is A, 1 is B")
    p.add_argument("--forge", metavar="RECIPE_PY", help="search variations of a recipe file")
    p.add_argument("--budget", type=int, default=96, help="with --forge: candidates the critic scores")
    p.add_argument("--pop", type=int, default=16, help="with --forge: population per generation")
    p.add_argument("--variants", type=int, default=4, help="with --forge: different sounds to keep")
    p.add_argument("--refset", help="with --forge: the critic's refset category")
    p.add_argument("--quiet", action="store_true", help="do not print the candidates while fitting")


def add_cli(sub: argparse._SubParsersAction) -> None:
    """Register `hark fit`."""
    from hark.core import report

    p = sub.add_parser("fit", help="fit a recording and write it as a recipe module")
    report.add_common(p)
    _add_flags(p)
    p.set_defaults(func=handler)
