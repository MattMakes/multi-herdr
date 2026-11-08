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


def _with_slash(pv: str) -> str:
    return str(pv).replace("\\", "/")


def _stem_of(path: str) -> str:
    return os.path.splitext(os.path.basename(str(path)))[0]


def output_paths(src: str, output: str | None = None, out_dir: str | None = None,
                 render_path: str | None = None) -> dict:
    """recipe, wav, png, model, report paths and the recipe name."""
    if output:
        dirv, basev = os.path.dirname(os.path.abspath(output)), _stem_of(output)
    else:
        dirv, basev = os.path.abspath(out_dir or os.getcwd()), _stem_of(src) + "_fit"
    os.makedirs(dirv, exist_ok=True)
    pv = {key_: _with_slash(os.path.join(dirv, basev + extv)) for key_, extv in
         (("recipe", ".py"), ("wav", ".wav"), ("png", ".png"), ("model", ".json"), ("report", ".report.json"))}
    if render_path:
        pv["wav"] = _with_slash(os.path.abspath(render_path))
    pv["name"] = basev
    return pv


def write_wav(path: str, y: np.ndarray) -> str:
    """A 48 kHz WAV with 32-bit float samples; the folder is made when missing."""
    import soundfile

    os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
    soundfile.write(path, np.asarray(y, dtype=np.float32), SR, subtype="FLOAT")
    return path


_JSON_FLAG = (
    (dict, lambda ov: {str(key_): _to_json_safe(val) for key_, val in ov.items()}),
    ((list, tuple), lambda ov: [_to_json_safe(val) for val in ov]),
    (np.ndarray, lambda ov: _to_json_safe(ov.tolist())),
    (np.generic, lambda ov: ov.item()),
)


def _to_json_safe(ov: Any) -> Any:
    cnv = next((fnv for types, fnv in _JSON_FLAG if isinstance(ov, types)), None)
    return ov if cnv is None else cnv(ov)


# ---------------------------------------------------------------- fit mode


def _row_list(model: dict) -> list:
    rws = (model.get("fit_engine") or {}).get("candidates") or []
    return [{"candidate": rv.get("candidate"), "score": rv.get("score"), "fit_s": rv.get("fit_s"),
             "error": rv.get("error")} for rv in rws]


def _render_code(path_l: dict, model: dict, seed: int, **kw: Any) -> tuple[Any, np.ndarray]:
    from hark.fit.pipeline import codegen

    codegen.write_recipe(path_l["recipe"], model, name=path_l["name"], **kw)
    recipe = codegen.load_recipe(path_l["recipe"])
    return recipe, recipe.render_seed(seed)


def _text_of_fit(result: dict, srcv: str, kind: str) -> str:
    from hark.fit.pipeline import quality as Q

    headv = f"{os.path.basename(srcv)}: {result['kind']}"
    if kind == "auto":
        headv += f", auto chose {result['chosen']}"
    line_l = [headv + f", fit {result['seconds']['fit']:.1f} s", Q.text(result["quality"], result["floor"])]
    if result["floor"] and result["floor"].get("mel_db") is not None:
        line_l.append("floor: the distance between 2 seeds of the same model, the best a model with noise can reach")
    line_l.append(f"code_renders_fit: {result['code_renders_fit']}")
    line_l += [f"wrote {key_}: {val}" for key_, val in result["files"].items()]
    return "\n".join(line_l)


def _report_write(path: str, result: dict, model: dict) -> None:
    from hark.fit.pipeline.render import summary

    datav = dict(result, fit_engine=model.get("fit_engine"), summary=summary(model))
    with open(path, "w", encoding="utf-8") as fv:
        json.dump(_to_json_safe(datav), fv, indent=1, default=float)


def run_fit(path: str, kind: str = "auto", layers: tuple | None = None, output: str | None = None,
            out_dir: str | None = None, render_path: str | None = None, report: bool = False, clap: bool = False,
            describe: bool = False, seed: int = 0, target: str | None = None, fast: bool = False,
            verbose: bool = True, log: Callable = print, takes: bool = False, bed: bool = True) -> dict:
    """The fit pipeline: fit, write and load the recipe, render through it, measure, check, save."""
    return _pipeline_fit(output_paths(path, output, out_dir, render_path), time.perf_counter(), path, kind=kind,
                         layers=layers, report=report, clap=clap, describe=describe, seed=seed, target=target,
                         fast=fast, verbose=verbose, takes=takes, bed=bed)


def _model_fit(path: str, ov: dict) -> tuple[np.ndarray, dict]:
    """The recording and its fitted model."""
    from hark.fit.pipeline import auto, quality as Q

    xmat = Q.load(path)
    seeds = auto.TAKE_SEEDS if ov["takes"] else (0,)
    mv = auto.fit(xmat, kind=ov["kind"], layers=ov["layers"], clap=ov["clap"], fast=ov["fast"],
                 name=os.path.basename(str(path)), verbose=ov["verbose"] and ov["kind"] == "auto", seeds=seeds,
                 bed=ov["bed"])
    return xmat, mv


def _measure_fit(xmat: np.ndarray, yv: np.ndarray, recipe: Any, ov: dict) -> tuple[dict, dict | None]:
    """The quality of the render, and the floor when a report is asked for."""
    from hark.fit.pipeline import quality as Q

    qv = Q.quality(xmat, yv, clap=ov["report"] or ov["clap"], describe=ov["describe"])
    flr = Q.floor(lambda sv: recipe.render_seed(sv), clap=False) if ov["report"] else None
    return qv, flr


def _fit_renders(path_l: dict, mv: dict, yv: np.ndarray, path: str, ov: dict, qv: dict, flr: dict | None) -> bool:
    """Rewrite the recipe with the quality, and check that it renders the engine's samples."""
    from hark.fit.pipeline import auto

    _, yv2 = _render_code(path_l, mv, ov["seed"], source=str(path), target=ov["target"], quality=qv, floor=flr)
    return bool(np.array_equal(yv2, auto.render(mv, seed=ov["seed"])) and np.array_equal(yv2, yv))


def _png_of_fit(path_l: dict, xmat: np.ndarray, yv: np.ndarray, path: str, mv: dict, kind: str) -> str:
    """Draw the pair picture and return the chosen candidate."""
    from hark.fit.pipeline import auto, quality as Q

    pick = (mv.get("fit_engine") or {}).get("chosen") or kind
    ttl = f"{os.path.basename(str(path))} -> {os.path.basename(path_l['recipe'])} ({auto.kind_of(mv)}"
    ttl += f", auto: {pick})" if kind == "auto" else ")"
    Q.pair_png(path_l["png"], xmat, yv, title=ttl)
    return pick


def _result_of_fit(path: str, mv: dict, pick: str, path_l: dict, seconds: dict) -> dict:
    from hark.fit.pipeline import auto

    file_l = {key_: path_l[key_] for key_ in ("recipe", "wav", "png", "model")}
    return {"source": _with_slash(path), "kind": auto.kind_of(mv), "chosen": pick, "files": file_l,
            "seconds": seconds, "candidates": _row_list(mv)}


def _pipeline_fit(path_l: dict, tm0: float, path: str, **ov: Any) -> dict:
    from hark.fit.pipeline import auto

    xmat, mv = _model_fit(path, ov)
    fit_sec = time.perf_counter() - tm0
    recipe, yv = _render_code(path_l, mv, ov["seed"], source=str(path), target=ov["target"])
    write_wav(path_l["wav"], yv)
    qv, flr = _measure_fit(xmat, yv, recipe, ov)
    samev = _fit_renders(path_l, mv, yv, path, ov, qv, flr)
    auto.save(mv, path_l["model"])
    pick = _png_of_fit(path_l, xmat, yv, path, mv, ov["kind"])
    result = _result_of_fit(path, mv, pick, path_l,
                      {"fit": round(fit_sec, 2), "total": round(time.perf_counter() - tm0, 2)})
    result.update(quality=qv, floor=flr, code_renders_fit=samev)
    if ov["report"]:
        result["files"]["report"] = path_l["report"]
        _report_write(path_l["report"], result, mv)
    result["text"] = _text_of_fit(result, str(path), ov["kind"])
    return result


# ---------------------------------------------------------------- morph mode


def _paths_of_morph(pa_path: str, pb_path: str, output: str | None, dir_out: str | None,
                 wav_path: str | None) -> dict:
    stemv = f"{_stem_of(pa_path)}__{_stem_of(pb_path)}_morph".replace("_fit", "")
    pv = output_paths(stemv + ".wav", output, dir_out, wav_path)
    return {key_: val.replace("_morph_fit", "_morph") for key_, val in pv.items()}


def _mono_padded(xs_l: list) -> list:
    from hark.fit.pipeline._post import mono

    msv = [monov(np.asarray(xv, dtype=np.float64)) for xv in xs_l]
    cnt = max(len(mv) for mv in msv)
    return [np.pad(mv, (0, cnt - len(mv))) for mv in msv]


def _recipe_of_morph(path_l: dict, mmv: dict, pa_path: str, pb_path: str, seed: int) -> np.ndarray:
    """Write and load the morph recipe, write its render, and return the render."""
    from hark.fit.pipeline import codegen

    notev = (f"A morph of {os.path.basename(str(pa_path))} and {os.path.basename(str(pb_path))}: keep both sources' "
            "attribution and licence with this file.")
    codegen.write_recipe(path_l["recipe"], mmv, name=path_l["name"], source=None, licence=notev)
    yv = codegen.load_recipe(path_l["recipe"]).render_seed(seed)
    write_wav(path_l["wav"], yv)
    return yv


def _png_of_morph(path_l: dict, xs_l: list, amount: float, pa_path: str, pb_path: str) -> None:
    from hark.fit.pipeline import quality as Q

    pav, pyv, pbv = _mono_padded(xs_l)
    ttl = f"morph {float(amount):.2f}: {os.path.basename(str(pa_path))} / {os.path.basename(str(pb_path))}"
    Q.pair_png(path_l["png"], pav, pyv, title=ttl, extra=(os.path.basename(str(pb_path)), pbv))


def _text_of_morph(result: dict, pa_path: str, pb_path: str) -> str:
    from hark.fit.pipeline import quality as Q

    line_l = [f"morph {result['amount']:.2f} of {os.path.basename(str(pa_path))} and {os.path.basename(str(pb_path))}"
             f" ({result['kind']})", "vs A: " + Q.text(result["vs_a"]), "vs B: " + Q.text(result["vs_b"])]
    return "\n".join(line_l + [f"wrote {key_}: {val}" for key_, val in result["files"].items()])


def run_morph(path_a: str, path_b: str, amount: float = 0.5, output: str | None = None,
              out_dir: str | None = None, render_path: str | None = None, clap: bool = False, seed: int = 0,
              fast: bool = False, verbose: bool = True) -> dict:
    """The morph pipeline: fit both with one kind, write the morph recipe, render, measure against both."""
    from hark.fit.pipeline import auto, quality as Q

    path_l = _paths_of_morph(path_a, path_b, output, out_dir, render_path)
    mav, mb, kinds = auto.fit_pair(path_a, path_b, clap=clap, fast=fast, verbose=verbose)
    mmv = auto.morph_model(mav, mb, amount)
    yv = _recipe_of_morph(path_l, mmv, path_a, path_b, seed)
    xa_m, xb_m = Q.load(path_a), Q.load(path_b)
    _png_of_morph(path_l, [xa_m, yv, xb_m], amount, path_a, path_b)
    auto.save(mmv, path_l["model"])
    result = {"a": _with_slash(path_a), "b": _with_slash(path_b), "amount": float(amount), "kind": auto.kind_of(mav),
           "kinds_tried": {key_: list(val) for key_, val in kinds.items()}, "vs_a": Q.quality(xa_m, yv, clap=clap),
           "vs_b": Q.quality(xb_m, yv, clap=clap), "files": {key_: path_l[key_] for key_ in ("recipe", "wav", "png", "model")}}
    result["text"] = _text_of_morph(result, path_a, path_b)
    return result


# ---------------------------------------------------------------- forge mode


def _refs_of_forge(recipe: Any, refset: str | None) -> tuple[str | None, list | None]:
    """The refset (given, else the recipe's), and the recipe's REFS when no refset exists."""
    refset = refset or getattr(recipe, "REFSET", None)
    ref_l = None if refset else (list(getattr(recipe, "REFS", []) or []) or None)
    return refset, ref_l


def _text_of_forge(result: dict, acc: str) -> str:
    picks = result.get("picks") or result.get("variants") or []
    line_l = [str(result.get("summary", ""))]
    line_l += [f"{pv.get('name', '?')}: {pv.get('summary', pv.get('score', ''))}" for pv in picks if isinstance(pv, dict)]
    line_l.append(f"wrote the variations and the search report to {acc}")
    return "\n".join(line_l)


def run_forge(recipe_path: str, out_dir: str | None = None, refset: str | None = None, budget: int = 96,
              pop: int = 16, variants: int = 4, seed: int = 0) -> dict:
    """The forge critic search for variations of a recipe file."""
    from hark.fit.pipeline import codegen
    from hark.forge.engine import loop

    recipe = codegen.load_recipe(recipe_path)
    refset, ref_l = _refs_of_forge(recipe, refset)
    acc = out_dir or os.path.splitext(str(recipe_path))[0] + "_forge"
    result = loop.forge(recipe, refs=ref_l, refset=refset, budget=budget, pop=pop, keep=variants, seed=seed, out=acc,
                     robust=3, prescreen=3 * pop, shortlist=2, strategy="es")
    result = {key_: val for key_, val in dict(result).items() if key_ != "history"}
    result["text"] = _text_of_forge(result, acc)
    return result


# ---------------------------------------------------------------- CLI


def _fit_args(args: argparse.Namespace) -> dict:
    layers = tuple(sv.strip() for sv in args.layers.split(",") if sv.strip()) if args.layers else None
    return dict(kind=args.kind, layers=layers, output=args.output, out_dir=args.dir, render_path=args.render,
                report=args.report, clap=args.clap, describe=args.describe, seed=args.seed, target=args.target,
                fast=args.fast, verbose=not args.quiet and not getattr(args, "json", False), takes=args.takes,
                bed=not args.keep_bed)


def _print_out(result: dict, args: argparse.Namespace) -> int:
    from hark.core import report

    text = result.pop("text", None)
    result.setdefault("summary", (text or "").splitlines()[0] if text else "")
    report.emit(result, args, text=text)
    return 0


def _mode_forge(args: argparse.Namespace) -> int:
    from hark.models import require_ml

    require_ml("fit --forge")
    return _print_out(run_forge(args.forge, args.dir, args.refset, args.budget, args.pop, args.variants, args.seed), args)


def _mode_morph(args: argparse.Namespace) -> int:
    return _print_out(run_morph(args.morph[0], args.morph[1], args.amount, args.output, args.dir, args.render,
                           args.clap, args.seed, args.fast, not args.quiet), args)


def _usage_text(args: argparse.Namespace) -> int:
    print(USAGE)
    return 2


def _mode_fit(args: argparse.Namespace) -> int:
    flg = next((fv for fv in ("clap", "describe", "report") if getattr(args, fv)), None)
    if flg:
        from hark.models import require_ml

        require_ml(f"fit --{flg}")
    return _print_out(run_fit(args.file, **_fit_args(args)), args)


def _which_mode(args: argparse.Namespace) -> Callable[[argparse.Namespace], int]:
    if args.forge:
        return _mode_forge
    return _mode_morph if args.morph else (_mode_fit if args.file else _usage_text)


def handler(args: argparse.Namespace) -> int:
    """Dispatch: --forge, then --morph, then usage without a file, else the fit."""
    return _which_mode(args)(args)


_FLAGS = (
    (("file",), {"nargs": "?", "help": "the recording to fit"}),
    (("-o", "--output"), {"help": "recipe path (.py); its folder and stem name every output"}),
    (("--dir",), {"help": "output folder when -o is not given; with --forge the search folder"}),
    (("--kind",), {"default": "auto", "choices": KIND_CHOICES, "help": "model kind"}),
    (("--layers",), {"help": "comma-separated hybrid layers, for example sines,modal,spectral"}),
    (("--render",), {"help": "WAV path of the resynthesis"}),
    (("--report",), {"action": "store_true", "help": "also measure the floor and CLAP; write a report"}),
    (("--clap",), {"action": "store_true", "help": "CLAP takes part in the choice of model"}),
    (("--describe",), {"action": "store_true", "help": "describe the original and the render"}),
    (("--target",), {"help": "the critic description for the recipe"}),
    (("--seed",), {"type": int, "default": 0, "help": "seed of the written render; with --forge the search seed"}),
    (("--fast",), {"action": "store_true", "help": "cheaper fitter settings, for previews"}),
    (("--takes",), {"action": "store_true", "help": "score every candidate on seeds 0, 1 and 2"}),
    (("--keep-bed",), {"action": "store_true", "help": "keep the bed and fit the unmodified recording"}),
    (("--morph",), {"nargs": 2, "metavar": ("A", "B"), "help": "fit A and B with one kind and write a morph"}),
    (("--amount",), {"type": float, "default": 0.5, "help": "with --morph: 0 is A, 1 is B"}),
    (("--forge",), {"metavar": "RECIPE_PY", "help": "search variations of a recipe file"}),
    (("--budget",), {"type": int, "default": 96, "help": "with --forge: candidates the critic scores"}),
    (("--pop",), {"type": int, "default": 16, "help": "with --forge: population per generation"}),
    (("--variants",), {"type": int, "default": 4, "help": "with --forge: different sounds to keep"}),
    (("--refset",), {"help": "with --forge: the critic's refset category"}),
    (("--quiet",), {"action": "store_true", "help": "do not print the candidates while fitting"}),
)


def _flags_add(pv: argparse.ArgumentParser) -> None:
    for names, kw in _FLAGS:
        pv.add_argument(*names, **kw)


def add_cli(sub: argparse._SubParsersAction) -> None:
    """Register `hark fit`."""
    from hark.core import report

    pv = sub.add_parser("fit", help="fit a recording and write it as a recipe module")
    report.add_common(pv)
    _flags_add(pv)
    pv.set_defaults(func=handler)
