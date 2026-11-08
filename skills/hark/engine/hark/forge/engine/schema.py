"""Render blocks: every delivered sound re-renders from its report.json alone.

A block names the recipe module and function, the complete params, the seed,
the peak level and the gain. The written WAV equals
float32(normalise(fn(params, rng(seed)), peak_db) * gain) as PCM 24-bit.
"""

from __future__ import annotations

import argparse
import functools
import glob
import json
import math
import os
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


class NotReproducible(ValueError):
    """A pick whose render cannot be resolved or fails."""


# ---------------------------------------------------------------- blocks

def _resolved(mod_name: str, name: str) -> Any:
    try:
        res = getattr(__import__(mod_name, fromlist=[name]), name, None)
    except Exception:  # noqa: BLE001 - any import failure means "not resolvable"
        res = None
    return res


def render_ref(fn: Any) -> tuple[str, str] | None:
    """(recipe short name, function name) of a recipe render function, or None."""
    if isinstance(fn, functools.partial):
        fn = fn.func
    mod_name = getattr(fn, "__module__", None)
    name = getattr(fn, "__name__", None)
    short = registry.short_name(fn) if mod_name else None
    ok = bool(short) and bool(name) and name != "<lambda>" and _resolved(mod_name, name) is fn
    return (short, name) if ok else None


def resolve(block: dict) -> Callable:
    """The render function a block names."""
    recipe, fn = block.get("recipe"), block.get("fn", "render")
    try:
        return getattr(registry.load(recipe), fn or "render")
    except (ImportError, AttributeError, KeyError, TypeError) as e:
        raise NotReproducible(f"render {recipe}.{fn} not found: {e}") from e


def _gain_db(gain: float | None, gain_db: float | None) -> float:
    if gain_db is not None:
        return float(gain_db)
    if gain:
        return round(20 * math.log10(gain), 2)
    return 0.0


def make_block(fn: Any, params: dict, seed: int, member: Any = None, mode: str | None = None,
               gain: float | None = None, gain_db: float | None = None, peak_db: float = PEAK_DB,
               ref: tuple[str, str] | None = None) -> dict | None:
    """A render block, or None when the function has no importable name."""
    ref = ref or render_ref(fn)
    if not ref:
        return None
    b = {"recipe": ref[0], "fn": ref[1], "member": None if member is None else str(member),
         "params": dict(params), "seed": int(seed), "mode": mode, "peak_db": float(peak_db),
         "gain_db": _gain_db(gain, gain_db)}
    if gain is not None:
        b["gain"] = float(gain)
    return b


def _legacy_module(report: dict) -> tuple[str, str]:
    name = report.get("recipe_module")
    if name:
        return str(name).rsplit(".", 1)[-1], "render"
    rec = report.get("recipe")
    if rec in LEGACY:
        mod, fn = LEGACY[rec]
        return str(mod).rsplit(".", 1)[-1], fn
    try:
        registry.folder_of(rec)
    except (KeyError, TypeError) as e:
        raise NotReproducible(f"recipe {rec} is no module of hark.forge.recipes and not in schema.LEGACY") from e
    return rec, "render"


def _legacy_params(report: dict, pick: dict) -> dict:
    params = dict(pick.get("params") or {})
    if set(params) != {"variant"}:
        return params
    if pick.get("family_params"):
        return dict(pick["family_params"])
    try:
        return dict(report["family"]["members"][params["variant"]])
    except (KeyError, IndexError, TypeError) as e:
        raise NotReproducible(f"pick {pick.get('name')}: variant params cannot be resolved") from e


def _member_key(pick: dict) -> str | None:
    vals = [str(v) for v in (pick.get(k) for k in ("member", "family_name", "family", "preset"))
            if isinstance(v, str) or (isinstance(v, int) and not isinstance(v, bool))]
    return vals[0] if vals else None


def _label_at(report: dict, index: int | None) -> str | None:
    fam = report.get("family")
    labels = fam.get("labels") if isinstance(fam, dict) else None
    ok = isinstance(labels, list) and index is not None and index < len(labels)
    return str(labels[index]) if ok else None


def _legacy_member(report: dict, pick: dict, index: int | None) -> str | None:
    res = _member_key(pick)
    if res is None and pick.get("family_member") is not None:
        res = str(pick["family_member"])
    if res is None:
        res = _label_at(report, index)
    return res


def _legacy_mode(report: dict) -> str | None:
    mode = (report.get("critic") or {}).get("mode") if isinstance(report.get("critic"), dict) else None
    mode = mode if isinstance(mode, str) else report.get("mode")
    return mode if isinstance(mode, str) else None


def legacy_block(report: dict, pick: dict) -> dict:
    """An inferred block for a report written before schema 2."""
    picks = report.get("picks") or []
    index = next((i for i, p in enumerate(picks) if p is pick), None)
    recipe, fn = _legacy_module(report)
    return {"recipe": recipe, "fn": fn, "member": _legacy_member(report, pick, index),
            "params": _legacy_params(report, pick), "seed": int(pick.get("seed") or 0),
            "mode": _legacy_mode(report), "peak_db": PEAK_DB, "gain_db": float(pick.get("gain_db") or 0.0)}


def block(report: dict, pick: dict) -> dict:
    """The pick's own block, else the inferred one."""
    b = pick.get("render")
    return b if isinstance(b, dict) else legacy_block(report, pick)


# ---------------------------------------------------------------- rendering

def raw(b: dict) -> np.ndarray | None:
    """The render function's float64 output, or None when it fails or is silent."""
    fn = resolve(b)
    try:
        x = np.asarray(fn(dict(b["params"]), dsp.rng(int(b.get("seed") or 0))), dtype=np.float64)
    except Exception:  # noqa: BLE001 - a failing render is "no render"
        return None
    if x.size == 0 or not np.all(np.isfinite(x)) or np.max(np.abs(x)) < 1e-7:
        return None
    return x


def level(x: np.ndarray, b: dict) -> np.ndarray:
    """The raw render at the written level, float32."""
    y = dsp.normalize(np.asarray(x, dtype=np.float64), float(b.get("peak_db", PEAK_DB))).astype(np.float32)
    gain = b.get("gain")
    gain = float(gain) if gain is not None else 10 ** (float(b.get("gain_db") or 0.0) / 20)
    if gain != 1.0:
        y = (y.astype(np.float64) * gain).astype(np.float32)
    return y


def rerender(b: dict) -> np.ndarray:
    """level(raw(b), b); raises NotReproducible when the render fails."""
    x = raw(b)
    if x is None:
        raise NotReproducible(f"render {b.get('recipe')}.{b.get('fn')} failed or is silent")
    return level(x, b)


def compare(x: np.ndarray, y: np.ndarray) -> float:
    """Max absolute sample difference; inf when shapes differ."""
    x = np.asarray(x)
    x = x[:, None] if x.ndim == 1 else x
    y = np.asarray(y)
    if x.shape != y.shape:
        return math.inf
    if x.size == 0:
        return 0.0
    return float(np.max(np.abs(x.astype(np.float64) - y.astype(np.float64))))


# ---------------------------------------------------------------- check

def _load_report(report: Any, folder: Any) -> tuple[dict, Path]:
    if isinstance(report, dict):
        if folder is None:
            raise ValueError("check_final with a report dict needs folder")
        return report, Path(folder)
    p = Path(report)
    path = p / "report.json" if p.is_dir() else p
    return json.loads(path.read_text()), Path(folder) if folder else path.parent


def _read_wav(path: Path) -> np.ndarray:
    import soundfile as sf

    return sf.read(str(path), dtype="float64", always_2d=True)[0]


def _pick_file(pick: dict, name: str) -> str:
    return os.path.basename(str(pick["file"]).replace("\\", "/")) if pick.get("file") else f"{name}.wav"


def _fill_err(row: dict, report: dict, pick: dict, wav: Path, tol: float) -> None:
    b = block(report, pick)
    row.update(recipe=b.get("recipe"), fn=b.get("fn"), member=b.get("member"))
    if not wav.is_file():
        row.update(err=math.inf, ok=False, why="WAV missing")
    else:
        row["err"] = compare(rerender(b), _read_wav(wav))
        row["why"] = None if row["err"] <= tol else "samples differ"


def _check_row(report: dict, pick: dict, folder: Path, tol: float) -> dict:
    name = pick.get("name") or "pick"
    fname = _pick_file(pick, name)
    row: dict = {"name": name, "file": fname, "source": "block" if isinstance(pick.get("render"), dict) else "legacy"}
    try:
        _fill_err(row, report, pick, folder / fname, tol)
    except NotReproducible as e:
        row.update(err=math.inf, why=str(e))
    row["ok"] = row["err"] <= tol
    return row


def worker_count(workers: int | None = None) -> int:
    """Thread count: the argument, else HARK_THREADS, else 2."""
    if workers:
        return max(1, int(workers))
    try:
        return max(1, int(os.environ.get("HARK_THREADS", "") or 2))
    except ValueError:
        return 2


def check_final(report: Any, folder: Any = None, tol: float = TOL, workers: int | None = None) -> list[dict]:
    """Re-render every pick and compare it with its WAV; one row per pick."""
    rep, base = _load_report(report, folder)
    picks = rep.get("picks") or []
    n = worker_count(workers)
    if n > 1 and len(picks) > 1:
        with ThreadPoolExecutor(n) as ex:
            return list(ex.map(lambda p: _check_row(rep, p, base, tol), picks))
    return [_check_row(rep, p, base, tol) for p in picks]


# ---------------------------------------------------------------- fill and upgrade

def _defaults_tables(b: dict, fn: Callable) -> list[dict]:
    mod = registry.load(b["recipe"])
    tables: list[dict] = []

    def add(t: Any) -> None:
        if isinstance(t, dict) and all(t is not u for u in tables):
            tables.append(t)

    if (b.get("fn") or "render") == "render":
        add(getattr(mod, "DEFAULTS", None))
    for v in list(vars(mod).values()):
        if getattr(v, "render", None) is fn:
            add(getattr(v, "DEFAULTS", None))
    for k, v in list(vars(mod).items()):
        if k.endswith("DEFAULTS"):
            add(v)
    return tables


def _same(b: dict, params: dict, x: np.ndarray) -> bool:
    try:
        y = raw({**b, "params": params})
    except Exception:  # noqa: BLE001
        return False
    return y is not None and y.shape == x.shape and np.array_equal(y, x)


def fill(b: dict, x: np.ndarray | None = None) -> dict:
    """The block with params completed from DEFAULTS where the render stays bit-identical."""
    x = raw(b) if x is None else x
    if x is None:
        return b
    fn = resolve(b)
    params = dict(b["params"])
    for table in _defaults_tables(b, fn):
        missing = {k: v for k, v in table.items() if k not in params}
        if not missing:
            continue
        cand = {**missing, **params}
        if _same(b, cand, x):
            params = cand
    return {**b, "params": params}


def _json_default(o: Any) -> Any:
    if isinstance(o, np.ndarray):
        return o.tolist()
    if isinstance(o, np.generic):
        return o.item()
    return str(o)


def write_json(path: Path, obj: Any) -> None:
    """Atomic JSON write (temporary file, then replace), indent 1."""
    tmp = path.with_name(f"{path.name}.tmp{os.getpid()}")
    tmp.write_text(json.dumps(obj, indent=1, default=_json_default))
    os.replace(tmp, path)


def _upgrade_row(report: dict, pick: dict, base: Path) -> dict:
    has = isinstance(pick.get("render"), dict)
    b = block(report, pick)
    x = raw(b)
    bf = b if has else fill(b, x)
    fname = os.path.basename(str(pick.get("file") or f"{pick.get('name')}.wav"))
    wav = base / fname
    err = compare(level(x, bf), _read_wav(wav)) if x is not None and wav.is_file() else math.inf
    if not has:
        pick["render"] = bf
    return {"name": pick.get("name"), "err": err, "ok": err <= TOL,
            "filled": len(bf["params"]) - len(b["params"]), "had_block": has}


def _write_upgrade(rep: dict, rows: list[dict], base: Path, log: Callable | None) -> None:
    bad = [r for r in rows if not r["ok"]]
    if bad:
        raise NotReproducible("not written: " + ", ".join(f"{r['name']} err {r['err']:.3g}" for r in bad))
    path = base / "report.json"
    write_json(path, rep)
    if log:
        log(f"wrote {path}: {len(rows)} render blocks (schema 2)")


def upgrade(report: Any, write: bool = False, tol: float = TOL, folder: Any = None,
            log: Callable | None = print) -> tuple[dict, list[dict]]:
    """Add render blocks to an older report; write it only when every pick matches."""
    rep, base = _load_report(report, folder)
    rows = [_upgrade_row(rep, p, base) for p in rep.get("picks") or []]
    for r in rows:
        r["ok"] = r["err"] <= tol
    if all(r["ok"] for r in rows):
        rep["schema"] = SCHEMA
    if write:
        _write_upgrade(rep, rows, base, log)
    return rep, rows


# ---------------------------------------------------------------- CLI

def _folders(items: Sequence[str]) -> list[str]:
    out: set[str] = set()
    for item in items:
        for p in glob.glob(item) or [item]:
            if (Path(p) / "report.json").is_file():
                out.add(p)
    return sorted(out)


def _line(d: str, rows: list[dict], tol: float, sources: str) -> str:
    ok = sum(1 for r in rows if r["ok"])
    e = max((r["err"] for r in rows), default=0.0)
    return f"{d}: {ok} / {len(rows)} re-render within {tol:g} (max err {e:.3g}; {sources})"


def _sources(rows: list[dict], key: str, names: tuple[str, str]) -> str:
    kinds = sorted({names[0] if r.get(key) else names[1] for r in rows})
    return ", ".join(kinds) or "-"


def _run_check(d: str, tol: float) -> bool:
    rows = check_final(d, tol=tol)
    src = ", ".join(sorted({r["source"] for r in rows})) or "-"
    print(_line(d, rows, tol, src))
    for r in rows:
        if not r["ok"]:
            print(f"  {r['name']}: err {r['err']:.3g} {r.get('why') or ''}")
    return all(r["ok"] for r in rows)


def _run_upgrade(d: str, tol: float, write: bool) -> bool:
    try:
        _, rows = upgrade(d, write=write, tol=tol)
    except NotReproducible as e:
        print(f"{d}: {e}")
        return False
    added = "block added" if write else "block inferred, not written"
    print(_line(d, rows, tol, _sources(rows, "had_block", ("had a block", added))))
    for r in rows:
        if not r["ok"]:
            print(f"  {r['name']}: err {r['err']:.3g}")
    return all(r["ok"] for r in rows)


def main(argv: Sequence[str] | None = None) -> int:
    """Check or upgrade forge output folders; exit 1 when any pick fails."""
    ap = argparse.ArgumentParser(prog="python -m hark.forge.engine.schema")
    ap.add_argument("dirs", nargs="+")
    ap.add_argument("--upgrade", action="store_true")
    ap.add_argument("--write", action="store_true")
    ap.add_argument("--tol", type=float, default=TOL)
    a = ap.parse_args(argv)
    good = True
    for d in _folders(a.dirs):
        ok = _run_upgrade(d, a.tol, a.write) if a.upgrade else _run_check(d, a.tol)
        good = good and ok
    return 0 if good else 1


if __name__ == "__main__":
    raise SystemExit(main())
