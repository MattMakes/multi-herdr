"""The progress listening file: first-version finals beside their nearest second-version finals, plus a table.

The table compares the two versions against the real rows of the benchmark results. The blind test decides;
this file only shows the direction of change.
"""

from __future__ import annotations

import glob
import json
import math
import os
import re
import time
from collections.abc import Sequence
from pathlib import Path
from typing import Any

import numpy as np

from hark.forge.engine.eval import blind_trim as bt
from hark.forge.engine.eval import frozen_tone as ft
from hark.forge.engine.eval.scrub_paths import EVAL_DIR, REPO

SR = bt.SR
V1_ROOT = REPO / "out" / "forge_v1"
V2_ROOT = REPO / "out" / "forge_v2"
# (answer name, category, first-version final folder, file stem)
V1 = [("key press", "typing_key"), ("UI click", "click"), ("pop", "pop"), ("fast whoosh", "whoosh_fast"),
      ("impact", "impact")]
CHECKS = [("var_cos", "span", 0.01), ("tone_db", "low", 0.3), ("env_oct", "low", 0.03), ("curve_db", "low", 0.5),
          ("t40_ratio", "one", 0.03), ("artifacts", "low", 0.5), ("stereo_corr", "span", 0.02),
          ("floor_db", "span", 1.0), ("darken", "span", 0.05), ("typ_z", "model", 0.05), ("phrase_z", "model", 0.05),
          ("margin", "model", 0.05), ("as_names", "model", 0.05)]
GAP_S, GROUP_GAP_S, LEAD_S, TAIL_S = 1.0, 3.0, 1.0, 1.0


# ---------------------------------------------------------------- verdicts

def _dist(rule: str, v: float, reals: list[float]) -> float:
    if rule == "low":
        return max(0.0, v - (min(reals) if reals else 0.0))
    if rule == "one":
        lo, hi = min([1.0] + reals), max([1.0] + reals)
        u = max(v, 1e-3)
        return max(0.0, math.log(lo / u), math.log(u / hi))
    return max(0.0, min(reals) - v, v - max(reals))


def _step(a: float, b: float, eps: float, words: tuple[str, str]) -> str:
    """words[0] when b rises past a by more than eps, words[1] when it falls by more than eps, else "same"."""
    return words[0] if b > a + eps else words[1] if b < a - eps else "same"


def _model_verdict(a: Any, b: Any, reals: list[float], eps: float) -> str:
    above = " (v1 sat above the real rows)" if reals and a > max(reals) else ""
    return _step(a, b, eps, ("up", "down")) + above


def _dist_verdict(rule: str, a: Any, b: Any, reals: list[float], eps: float) -> str:
    da, db = _dist(rule, float(a), reals), _dist(rule, float(b), reals)
    if da == 0 and db == 0:
        res = "same (both within the real rows)"
    else:
        res = _step(da, db, eps, ("worse", "better"))
    return res


def verdict(rule: str, a: Any, b: Any, reals: Sequence[Any], eps: float) -> str:
    """Direction of change from v1 (a) to v2 (b) against the real rows."""
    vals = [float(r) for r in reals if r is not None]
    if a is None or b is None or (rule == "span" and not vals):
        res = ""
    elif rule == "model":
        res = _model_verdict(a, b, vals, eps)
    else:
        res = _dist_verdict(rule, a, b, vals, eps)
    return res


# ---------------------------------------------------------------- results

def _results(path: Path | None) -> tuple[dict, dict, Path]:
    v1 = EVAL_DIR / "results_v1.json"
    if path is None:
        cands = [(int(m.group(1)), p) for p in glob.glob(str(EVAL_DIR / "results_v2r*.json"))
                 if (m := re.search(r"results_v2r(\d+)\.json$", p))]
        path = Path(max(cands)[1]) if cands else None
    if path is None or not v1.is_file() or not path.is_file():
        raise SystemExit("progress: no first-version and second-version benchmark results found")
    return json.loads(v1.read_text()), json.loads(path.read_text()), path


def _value(cat: dict, key: str, set_: str) -> Any:
    if key == "var_cos":
        return ((cat.get("summary") or {}).get(set_) or {}).get("var_cos")
    return ((cat.get("summary") or {}).get(set_) or {}).get(key)


def _reals(cat: dict, key: str) -> list:
    if key == "var_cos":
        return [(cat.get(d) or {}).get("cos_mean") for d in ("diversity_bench", "diversity_forge")]
    return [_value(cat, key, s) for s in ("real_loo", "real_cross")]


def _inside_row(c1: dict, c2: dict) -> str:
    m = [float(np.median(c["inside"]["per_sound"])) if c.get("inside") else None for c in (c1, c2)]
    if None in m:
        return "-"
    return "more" if m[1] > m[0] + 0.4 else "fewer" if m[1] < m[0] - 0.4 else "same"


def _table(cat: str, r1: dict, r2: dict) -> tuple[list[str], dict]:
    c1, c2 = (r1.get("categories") or {}).get(cat) or {}, (r2.get("categories") or {}).get(cat) or {}
    rows = ["| check | v1 | v2 | real loo | real x | verdict |", "|---|---|---|---|---|---|"]
    tally = {"better": 0, "worse": 0}
    for key, rule, eps in CHECKS:
        a, b = _value(c1, key, "made"), _value(c2, key, "made")
        reals = _reals(c2, key)
        v = verdict(rule, a, b, reals, eps)
        if rule != "model" and v in tally:
            tally[v] += 1
        rows.append(f"| {key} | {_c(a)} | {_c(b)} | {_c(reals[0])} | {_c(reals[1])} | {v} |")
    rows.append(f"| inside | | | | | {_inside_row(c1, c2)} |")
    return rows, tally


def _c(v: Any) -> str:
    return "-" if v is None else f"{v:g}" if isinstance(v, (int, float)) else str(v)


def _made_time(r2: dict) -> float | None:
    try:
        res = time.mktime(time.strptime(r2.get("created", ""), "%Y-%m-%d %H:%M"))
    except ValueError:
        res = None
    return res


def _seen_names(r2: dict, cat: str) -> list[str]:
    made = ((r2.get("categories") or {}).get(cat) or {}).get("made") or []
    return sorted(os.path.basename(m["file"]) for m in made)


def _cat_stale(r2: dict, cat: str, made: float) -> bool:
    files = sorted(glob.glob(str(V2_ROOT / cat / "final" / f"{cat}_[0-9][0-9].wav")))
    seen = _seen_names(r2, cat)
    return [os.path.basename(f) for f in files] != seen or any(os.path.getmtime(f) > made + 60 for f in files)


def _stale(r2: dict, cats: Sequence[str]) -> list[str]:
    made = _made_time(r2)
    return [] if made is None else [cat for cat in cats if _cat_stale(r2, cat, made)]


# ---------------------------------------------------------------- listening file

def _ext(x: np.ndarray) -> float:
    return max(bt.extent_s(x), 1e-4)


def _nearest(v1: np.ndarray, v2: list[tuple[str, np.ndarray]], used: set) -> tuple[str, np.ndarray, float]:
    p = ft.profile(v1)
    best = None
    for name, x in v2:
        if name in used:
            continue
        d = float(np.sqrt(np.mean((p - ft.profile(x)) ** 2))) + 6 * abs(math.log2(_ext(x) / _ext(v1)))
        if best is None or d < best[2]:
            best = (name, x, d)
    if best is None:
        raise SystemExit("progress: not enough second-version finals")
    used.add(best[0])
    return best


def _group(name: str, cat: str) -> list[dict]:
    v1 = [(str(V1_ROOT / cat / f"{cat}_{k:02d}.wav")) for k in (1, 2)]
    v2 = [(f, bt.load_audio(f)) for f in sorted(glob.glob(str(V2_ROOT / cat / "final" / f"{cat}_[0-9][0-9].wav")))]
    used: set = set()
    out = []
    for f in v1:
        if not os.path.isfile(f):
            raise SystemExit(f"progress: first-version final {f} is missing")
        x = bt.load_audio(f)
        n2, x2, d = _nearest(x, v2, used)
        out += [{"sound": name, "version": "v1", "file": f, "x": x},
                {"sound": name, "version": "v2", "file": n2, "x": x2, "nearest_db": round(d, 2)}]
    return out


def _level_group(g: list[dict]) -> None:
    cls = "long" if any(len(s["x"]) / SR >= bt.SHORT_S for s in g) else "short"
    for s in g:
        s["gain_db"] = bt.TARGET[cls] - bt.loud_max(s["x"], bt.WIN[cls])
    over = max(0.0, max(bt.true_peak(s["x"]) + s["gain_db"] for s in g) - bt.TP_MAX)
    for s in g:
        s["gain_db"] -= over
        s["y"] = s["x"] * 10 ** (s["gain_db"] / 20)
        s["loud"] = round(bt.loud_max(s["y"], bt.WIN[cls]), 2)
        s["tp"] = round(bt.true_peak(s["y"]), 2)


def _reel(groups: list[list[dict]]) -> np.ndarray:
    parts, t = [np.zeros((int(LEAD_S * SR), 2))], LEAD_S
    for gi, g in enumerate(groups):
        for si, s in enumerate(g):
            s["start"] = round(t, 2)
            last = si == len(g) - 1
            gap = GROUP_GAP_S if last and gi < len(groups) - 1 else GAP_S if not last else TAIL_S
            parts += [s["y"], np.zeros((int(gap * SR), 2))]
            t += len(s["y"]) / SR + gap
    return np.concatenate(parts)


def _progress_line(s: dict) -> str:
    near = f"  nearest {s['nearest_db']}" if "nearest_db" in s else ""
    return (f"{s['start']:8.2f} s  {s['sound']:<12} {s['version']}  loudest {s['loud']} LUFS  tp {s['tp']} dBTP  "
            f"{os.path.basename(s['file'])}" + near + "\n")


def _progress_rows(flat: list[dict]) -> list[dict]:
    keys = ("sound", "version", "file", "start", "gain_db", "loud", "tp")
    return [{k: s[k] for k in keys if k in s} for s in flat]


def build_progress(pdir: Any, results_path: Any = None, quiet: bool = False) -> Path:
    """Write v1_vs_v2.wav, progress.txt, progress.json and progress.md."""
    from hark.core.audio import write_wav

    r1, r2, rpath = _results(Path(results_path) if results_path else None)
    groups = [_group(name, cat) for name, cat in V1]
    for g in groups:
        _level_group(g)
    reel = _reel(groups)
    pdir = Path(pdir)
    pdir.mkdir(parents=True, exist_ok=True)
    write_wav(str(pdir / "v1_vs_v2.wav"), reel, SR, "PCM_24")
    flat = [s for g in groups for s in g]
    (pdir / "progress.txt").write_text("".join(_progress_line(s) for s in flat))
    (pdir / "progress.json").write_text(json.dumps(_progress_rows(flat), indent=1))
    (pdir / "progress.md").write_text(_markdown(r1, r2, rpath))
    if not quiet:
        print(f"wrote {pdir}")
    return pdir


def _markdown(r1: dict, r2: dict, rpath: Path) -> str:
    out = [f"# progress: first version against {rpath.stem}", ""]
    stale = _stale(r2, [c for _, c in V1])
    if stale:
        out += [f"Warning: finals changed after the results were made: {', '.join(stale)}.", ""]
    totals = []
    for name, cat in V1:
        kept = sorted(Path(p).stem for p in glob.glob(str(V2_ROOT / cat / "final" / "KEPT_*.txt")))
        out += [f"## {name} ({cat})" + (f" [{', '.join(kept)}]" if kept else ""), ""]
        rows, tally = _table(cat, r1, r2)
        out += rows + [""]
        totals.append(f"| {name} | {tally['better']} | {tally['worse']} |")
    out += ["## Model-free checks", "", "| sound | better | worse |", "|---|---|---|"] + totals + [""]
    out += ["How to read:", "- Verdicts compare against the real rows (real loo and real x).",
            "- Model checks saturate and only show up or down.",
            "- The inside row mixes 4 model and 3 model-free checks.",
            "- First-version impact numbers are optimistic.", "- The blind test decides."]
    return "\n".join(out) + "\n"
