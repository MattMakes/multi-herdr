"""Render blocks let each delivered sound be rebuilt from its report.json and nothing else.

A block records the recipe module, the render function, the full params, the
seed, the peak level and the gain. The WAV on disk is 24-bit PCM of
float32(normalise(fn(params, rng(seed)), peak_db) * gain).
"""

from __future__ import annotations

import argparse
import functools
import glob
import json
import math
import os
import sys
from collections.abc import Callable, Sequence
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from typing import Any

import numpy as np

from hark.forge import recipes as registry
from hark.forge.engine import dsp

SCHEMA = 2
TOL = 1e-6
PEAK_DB = -1.0
BLOCK_KEYS = ("recipe", "fn", "member", "params", "seed", "mode", "peak_db", "gain_db", "gain")
LEGACY: dict[str, tuple[str, str]] = {}

_DEFAULT_FN = "render"
_REPORT = "report.json"
_SILENT = 1e-7


class NotReproducible(ValueError):
    """Raised for a pick whose render cannot be found or does not run."""


def _tail_name(dotted_path: Any) -> str:
    return str(dotted_path).rsplit(".", 1)[-1]


def _has_block(one_pick: dict) -> bool:
    return isinstance(one_pick.get("render"), dict)


def _is_plain_int(field_val: Any) -> bool:
    return isinstance(field_val, int) and not isinstance(field_val, bool)


# Building and reading blocks.

def _lookup(mod_path: str, fn_attr: str) -> Any:
    try:
        mod_obj = __import__(mod_path, fromlist=[fn_attr])
    except Exception:  # noqa: BLE001 - an import that fails means the name cannot be found
        return None
    return getattr(mod_obj, fn_attr, None)


def render_ref(fn: Any) -> tuple[str, str] | None:
    """Return (recipe short name, function name) for a recipe render function, or None."""
    dest_path: Any = fn.func if isinstance(fn, functools.partial) else fn
    mod_path: str | None = getattr(dest_path, "__module__", None)
    fn_attr: str | None = getattr(dest_path, "__name__", None)
    short_id: str | None = registry.short_name(dest_path) if mod_path else None
    if not short_id or not fn_attr or fn_attr == "<lambda>":
        return None
    if _lookup(mod_path, fn_attr) is not dest_path:
        return None
    return short_id, fn_attr


def resolve(block: dict) -> Callable:
    """Return the render function that the block names."""
    recipe_id: Any = block.get("recipe")
    fn_attr: str | None = block.get("fn", _DEFAULT_FN)
    try:
        recipe_mod = registry.load(recipe_id)
        return getattr(recipe_mod, fn_attr or _DEFAULT_FN)
    except (ImportError, AttributeError, KeyError, TypeError) as fault:
        raise NotReproducible(f"render {recipe_id}.{fn_attr} not found: {fault}") from fault


def _keyed(*block_vals: Any) -> dict:
    # BLOCK_KEYS lists the fields in block order. Only "gain" is optional.
    return dict(zip(BLOCK_KEYS, block_vals))


def _gain_db(gain: float | None, gain_db: float | None) -> float:
    if gain_db is not None:
        return float(gain_db)
    return round(20 * math.log10(gain), 2) if gain else 0.0


def make_block(fn: Any, params: dict, seed: int, member: Any = None, mode: str | None = None,
               gain: float | None = None, gain_db: float | None = None, peak_db: float = PEAK_DB,
               ref: tuple[str, str] | None = None) -> dict | None:
    """Build a render block. Return None when the function has no name that imports."""
    ref_pair: tuple[str, str] | None = ref or render_ref(fn)
    if not ref_pair:
        return None
    member_str: str | None = str(member) if member is not None else None
    new_blk = _keyed(ref_pair[0], ref_pair[1], member_str, dict(params), int(seed), mode, float(peak_db),
                   _gain_db(gain, gain_db))
    if gain is not None:
        new_blk["gain"] = float(gain)
    return new_blk


def _old_module(rep_json: dict) -> tuple[str, str]:
    dotted_path: Any = rep_json.get("recipe_module")
    if dotted_path:
        return _tail_name(dotted_path), _DEFAULT_FN
    recipe_id: Any = rep_json.get("recipe")
    if recipe_id in LEGACY:
        dotted_path, fn_attr = LEGACY[recipe_id]
        return _tail_name(dotted_path), fn_attr
    try:
        registry.folder_of(recipe_id)
    except (KeyError, TypeError) as fault:
        raise NotReproducible(
            f"recipe {recipe_id} is no module of hark.forge.recipes and not in schema.LEGACY") from fault
    return recipe_id, _DEFAULT_FN


def _old_params(rep_json: dict, one_pick: dict) -> dict:
    given_val: Any = dict(one_pick.get("params") or {})
    if set(given_val) != {"variant"}:
        return given_val
    if one_pick.get("family_params"):
        return dict(one_pick["family_params"])
    try:
        member_list = rep_json["family"]["members"]
        return dict(member_list[given_val["variant"]])
    except (KeyError, IndexError, TypeError) as fault:
        raise NotReproducible(f"pick {one_pick.get('name')}: variant params cannot be resolved") from fault


def _named_member(one_pick: dict) -> str | None:
    for pick_field in ("member", "family_name", "family", "preset"):
        field_val = one_pick.get(pick_field)
        if isinstance(field_val, str) or _is_plain_int(field_val):
            return str(field_val)
    return None


def _family_label(rep_json: dict, pick_pos: int | None) -> str | None:
    family_info: Any = rep_json.get("family")
    label_list: Any = family_info.get("labels") if isinstance(family_info, dict) else None
    if not isinstance(label_list, list) or pick_pos is None or pick_pos >= len(label_list):
        return None
    return str(label_list[pick_pos])


def _old_member(rep_json: dict, one_pick: dict, pick_pos: int | None) -> str | None:
    member_found = _named_member(one_pick)
    if member_found is not None:
        return member_found
    if one_pick.get("family_member") is not None:
        return str(one_pick["family_member"])
    return _family_label(rep_json, pick_pos)


def _old_mode(rep_json: dict) -> str | None:
    critic_info: Any = rep_json.get("critic")
    mode_pick: Any = critic_info.get("mode") if isinstance(critic_info, dict) else None
    if not isinstance(mode_pick, str):
        mode_pick = rep_json.get("mode")
    return mode_pick if isinstance(mode_pick, str) else None


def legacy_block(report: dict, pick: dict) -> dict:
    """Infer a block for a report from before schema 2."""
    pick_pos: int | None = None
    for idx_at, other_pick in enumerate(report.get("picks") or []):
        if other_pick is pick:
            pick_pos = idx_at
            break
    recipe_id, fn_attr = _old_module(report)
    member = _old_member(report, pick, pick_pos)
    full_params: dict = _old_params(report, pick)
    seed_int: int = int(pick.get("seed") or 0)
    gain_level: float = float(pick.get("gain_db") or 0.0)
    return _keyed(recipe_id, fn_attr, member, full_params, seed_int, _old_mode(report), PEAK_DB, gain_level)


def block(report: dict, pick: dict) -> dict:
    """Return the block stored on the pick, or else an inferred one."""
    stored_blk: Any = pick.get("render")
    if isinstance(stored_blk, dict):
        return stored_blk
    return legacy_block(report, pick)


# Rendering a block.

def raw(b: dict) -> np.ndarray | None:
    """Return the float64 output of the render function. Return None if it fails or is silent."""
    render_fn: Callable = resolve(b)
    try:
        seed_rng = dsp.rng(int(b.get("seed") or 0))
        rendered = np.asarray(render_fn(dict(b["params"]), seed_rng), dtype=np.float64)
    except Exception:  # noqa: BLE001 - a render that raises counts as no render
        return None
    if rendered.size == 0 or not np.all(np.isfinite(rendered)) or np.max(np.abs(rendered)) < _SILENT:
        return None
    return rendered


def _linear_gain(blk_in: dict) -> float:
    given_val: Any = blk_in.get("gain")
    if given_val is not None:
        return float(given_val)
    return 10 ** (float(blk_in.get("gain_db") or 0.0) / 20)


def level(x: np.ndarray, b: dict) -> np.ndarray:
    """Scale a raw render to the level that was written, as float32."""
    peak_level: float = float(b.get("peak_db", PEAK_DB))
    scaled32: np.ndarray = dsp.normalize(np.asarray(x, dtype=np.float64), peak_level).astype(np.float32)
    gain_lin: float = _linear_gain(b)
    if gain_lin == 1.0:
        return scaled32
    return (scaled32.astype(np.float64) * gain_lin).astype(np.float32)


def rerender(b: dict) -> np.ndarray:
    """Return level(raw(b), b). Raise NotReproducible when the render fails."""
    rendered: np.ndarray | None = raw(b)
    if rendered is None:
        raise NotReproducible(f"render {b.get('recipe')}.{b.get('fn')} failed or is silent")
    return level(rendered, b)


def compare(x: np.ndarray, y: np.ndarray) -> float:
    """Largest absolute difference between samples. inf when the shapes are not equal."""
    got_arr: np.ndarray = np.asarray(x)
    if got_arr.ndim == 1:
        got_arr = got_arr[:, None]
    wav_arr: np.ndarray = np.asarray(y)
    if got_arr.shape != wav_arr.shape:
        return math.inf
    if not got_arr.size:
        return 0.0
    return float(np.max(np.abs(got_arr.astype(np.float64) - wav_arr.astype(np.float64))))


# Checking a delivered folder.

def _open_report(rep_json: Any, out_dir: Any) -> tuple[dict, Path]:
    if isinstance(rep_json, dict):
        if out_dir is None:
            raise ValueError("check_final with a report dict needs folder")
        return rep_json, Path(out_dir)
    given_val: Any = Path(rep_json)
    rep_file: Path = given_val / _REPORT if given_val.is_dir() else given_val
    rep_dir: Path = Path(out_dir) if out_dir else rep_file.parent
    return json.loads(rep_file.read_text()), rep_dir


def _wav_samples(path: Path) -> np.ndarray:
    import soundfile as sf

    rep_json, _rate = sf.read(str(path), dtype="float64", always_2d=True)
    return rep_json


def _wav_name(one_pick: dict, pick_label: str) -> str:
    if not one_pick.get("file"):
        return f"{pick_label}.wav"
    return os.path.basename(str(one_pick["file"]).replace("\\", "/"))


def _measure_pick(result_row: dict, rep_json: dict, one_pick: dict, wav_path: Path, err_limit: float) -> None:
    pick_blk: dict = block(rep_json, one_pick)
    result_row.update(recipe=pick_blk.get("recipe"), fn=pick_blk.get("fn"), member=pick_blk.get("member"))
    if wav_path.is_file():
        result_row["err"] = compare(rerender(pick_blk), _wav_samples(wav_path))
        result_row["why"] = None if result_row["err"] <= err_limit else "samples differ"
    else:
        result_row.update(err=math.inf, ok=False, why="WAV missing")


def _verify_pick(rep_json: dict, one_pick: dict, out_dir: Path, err_limit: float) -> dict:
    pick_label: str = one_pick.get("name") or "pick"
    wav_file: str = _wav_name(one_pick, pick_label)
    blk_origin: str = "block" if _has_block(one_pick) else "legacy"
    result_row: dict = {"name": pick_label, "file": wav_file, "source": blk_origin}
    try:
        _measure_pick(result_row, rep_json, one_pick, out_dir / wav_file, err_limit)
    except NotReproducible as fault:
        result_row.update(err=math.inf, why=str(fault))
    result_row["ok"] = result_row["err"] <= err_limit
    return result_row


def worker_count(workers: int | None = None) -> int:
    """Number of threads: the argument, else HARK_THREADS, else 2."""
    if workers:
        return max(1, int(workers))
    try:
        n_pool = int(os.environ.get("HARK_THREADS", "") or 2)
    except ValueError:
        return 2
    return max(1, n_pool)


def check_final(report: Any, folder: Any = None, tol: float = TOL, workers: int | None = None) -> list[dict]:
    """Render each pick again and compare it with its WAV. Return 1 row for each pick."""
    rep_json, rep_dir = _open_report(report, folder)
    pick_list: list = rep_json.get("picks") or []

    def check_one(one_pick: dict) -> dict:
        return _verify_pick(rep_json, one_pick, rep_dir, tol)

    n_pool: int = worker_count(workers)
    if n_pool <= 1 or len(pick_list) <= 1:
        return [check_one(each_pick) for each_pick in pick_list]
    with ThreadPoolExecutor(n_pool) as executor:
        return list(executor.map(check_one, pick_list))


# Completing params and upgrading old reports.

def _default_tables(blk_in: dict, render_fn: Callable) -> list[dict]:
    recipe_mod = registry.load(blk_in["recipe"])
    hits_list: list[dict] = []

    def add_table(dflt_table: Any) -> None:
        if isinstance(dflt_table, dict) and not any(dflt_table is kind_set for kind_set in hits_list):
            hits_list.append(dflt_table)

    if (blk_in.get("fn") or _DEFAULT_FN) == _DEFAULT_FN:
        add_table(getattr(recipe_mod, "DEFAULTS", None))
    mod_vars: dict[str, Any] = dict(vars(recipe_mod))
    for field_val in mod_vars.values():
        if getattr(field_val, "render", None) is render_fn:
            add_table(getattr(field_val, "DEFAULTS", None))
    for dflt_key, field_val in mod_vars.items():
        if dflt_key.endswith("DEFAULTS"):
            add_table(field_val)
    return hits_list


def _renders_same(blk_in: dict, full_params: dict, ref_audio: np.ndarray) -> bool:
    try:
        trial_params = raw({**blk_in, "params": full_params})
    except Exception:  # noqa: BLE001
        return False
    if trial_params is None or trial_params.shape != ref_audio.shape:
        return False
    return bool(np.array_equal(trial_params, ref_audio))


def fill(b: dict, x: np.ndarray | None = None) -> dict:
    """Complete the block params from DEFAULTS wherever the render stays bit-identical."""
    ref_audio: np.ndarray | None = raw(b) if x is None else x
    if ref_audio is None:
        return b
    render_fn: Callable = resolve(b)
    merged: dict = dict(b["params"])
    for dflt_table in _default_tables(b, render_fn):
        missing_keys = {dflt_key: dflt_val for dflt_key, dflt_val in dflt_table.items() if dflt_key not in merged}
        if missing_keys:
            trial_params = {**missing_keys, **merged}
            if _renders_same(b, trial_params, ref_audio):
                merged = trial_params
    return {**b, "params": merged}


def _to_json(field_val: Any) -> Any:
    if isinstance(field_val, np.ndarray):
        return field_val.tolist()
    if isinstance(field_val, np.generic):
        return field_val.item()
    return str(field_val)


def write_json(path: Path, obj: Any) -> None:
    """Write JSON with indent 1 to a temporary file, then move it over path."""
    tmp_path: Path = path.with_name(f"{path.name}.tmp{os.getpid()}")
    tmp_path.write_text(json.dumps(obj, indent=1, default=_to_json))
    os.replace(tmp_path, path)


def _upgrade_pick(rep_json: dict, one_pick: dict, rep_dir: Path) -> dict:
    had_blk: bool = _has_block(one_pick)
    base_blk: dict = block(rep_json, one_pick)
    rendered: np.ndarray | None = raw(base_blk)
    filled_blk: dict = base_blk if had_blk else fill(base_blk, rendered)
    wav_path: Path = rep_dir / os.path.basename(str(one_pick.get("file") or f"{one_pick.get('name')}.wav"))
    if rendered is not None and wav_path.is_file():
        fault = compare(level(rendered, filled_blk), _wav_samples(wav_path))
    else:
        fault = math.inf
    if not had_blk:
        one_pick["render"] = filled_blk
    n_added: int = len(filled_blk["params"]) - len(base_blk["params"])
    return {"name": one_pick.get("name"), "err": fault, "ok": fault <= TOL, "filled": n_added, "had_block": had_blk}


def _save_upgrade(rep_json: dict, result_rows: list[dict], rep_dir: Path, log_fn: Callable | None) -> None:
    bad_rows: list[dict] = [each_row for each_row in result_rows if not each_row["ok"]]
    if bad_rows:
        bad_text = ", ".join(f"{each_row['name']} err {each_row['err']:.3g}" for each_row in bad_rows)
        raise NotReproducible("not written: " + bad_text)
    dest_path: Any = rep_dir / _REPORT
    write_json(dest_path, rep_json)
    if log_fn:
        log_fn(f"wrote {dest_path}: {len(result_rows)} render blocks (schema 2)")


def upgrade(report: Any, write: bool = False, tol: float = TOL, folder: Any = None,
            log: Callable | None = print) -> tuple[dict, list[dict]]:
    """Add render blocks to an old report. Write it only when every pick matches."""
    rep_json, rep_dir = _open_report(report, folder)
    result_rows: list[dict] = [_upgrade_pick(rep_json, each_pick, rep_dir) for each_pick in rep_json.get("picks") or []]
    for result_row in result_rows:
        result_row["ok"] = result_row["err"] <= tol
    if all(result_row["ok"] for result_row in result_rows):
        rep_json["schema"] = SCHEMA
    if write:
        _save_upgrade(rep_json, result_rows, rep_dir, log)
    return rep_json, result_rows


# Command line.

def _output_dirs(cli_arg: str) -> set[str]:
    """Return the forge output folders (each with a report.json) that 1 argument names."""
    glob_hits: list[str] = glob.glob(cli_arg) or [cli_arg]
    return {cand_dir for cand_dir in glob_hits if (Path(cand_dir) / _REPORT).is_file()}


def _all_output_dirs(cli_args: Sequence[str]) -> list[str]:
    hits_list: set[str] = set()
    for cli_arg in cli_args:
        hits_list.update(_output_dirs(cli_arg))
    return sorted(hits_list)


def _unmatched(cli_args: Sequence[str]) -> list[str]:
    """Return the arguments that name no forge output folder."""
    return [cli_arg for cli_arg in cli_args if not _output_dirs(cli_arg)]


def _summary(out_dir: str, result_rows: list[dict], err_limit: float, kind_text: str) -> str:
    n_ok = len([each_row for each_row in result_rows if each_row["ok"]])
    max_err: float = max((each_row["err"] for each_row in result_rows), default=0.0)
    counts = f"{n_ok} / {len(result_rows)}"
    return f"{out_dir}: {counts} re-render within {err_limit:g} (max err {max_err:.3g}; {kind_text})"


def _kinds(result_rows: list[dict], flag_key: str, kind_words: tuple[str, str]) -> str:
    yes_word, no_word = kind_words
    kind_set: set[str] = {yes_word if each_row.get(flag_key) else no_word for each_row in result_rows}
    return ", ".join(sorted(kind_set)) or "-"


def _report_failures(result_rows: list[dict], show_why: bool) -> bool:
    for each_row in result_rows:
        if each_row["ok"]:
            continue
        if show_why:
            print(f"  {each_row['name']}: err {each_row['err']:.3g} {each_row.get('why') or ''}")
        else:
            print(f"  {each_row['name']}: err {each_row['err']:.3g}")
    return all(each_row["ok"] for each_row in result_rows)


def _cli_check(out_dir: str, err_limit: float) -> bool:
    result_rows: list[dict] = check_final(out_dir, tol=err_limit)
    kind_text: str = ", ".join(sorted({each_row["source"] for each_row in result_rows})) or "-"
    print(_summary(out_dir, result_rows, err_limit, kind_text))
    return _report_failures(result_rows, show_why=True)


def _cli_upgrade(out_dir: str, err_limit: float, do_write: bool) -> bool:
    try:
        result_rows = upgrade(out_dir, write=do_write, tol=err_limit)[1]
    except NotReproducible as fault:
        print(f"{out_dir}: {fault}")
        return False
    new_word: str = "block added" if do_write else "block inferred, not written"
    print(_summary(out_dir, result_rows, err_limit, _kinds(result_rows, "had_block", ("had a block", new_word))))
    return _report_failures(result_rows, show_why=False)


def main(argv: Sequence[str] | None = None) -> int:
    """Check or upgrade forge output folders. Exit 1 when a pick fails."""
    cli_parser = argparse.ArgumentParser(prog="python -m hark.forge.engine.schema")
    cli_parser.add_argument("dirs", nargs="+")
    for switch in ("--upgrade", "--write"):
        cli_parser.add_argument(switch, action="store_true")
    cli_parser.add_argument("--tol", type=float, default=TOL)
    cli_opts = cli_parser.parse_args(argv)
    no_match: list[str] = _unmatched(cli_opts.dirs)
    for cli_arg in no_match:
        print(f"{cli_arg}: no forge output folder (no report.json)", file=sys.stderr)

    def run_one(out_dir: str) -> bool:
        if cli_opts.upgrade:
            return _cli_upgrade(out_dir, cli_opts.tol, cli_opts.write)
        return _cli_check(out_dir, cli_opts.tol)

    outcomes: list[bool] = [run_one(out_dir) for out_dir in _all_output_dirs(cli_opts.dirs)]
    return 1 if no_match or not all(outcomes) else 0

if __name__ == "__main__":
    raise SystemExit(main())
