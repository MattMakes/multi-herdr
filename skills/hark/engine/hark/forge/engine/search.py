"""The forge search: parameter encoding, rendering, the model-free pre-screen,
the optimiser (ES and separable CMA-ES), robust re-renders, diverse picks and
the seed bank.
"""

from __future__ import annotations

import functools
import inspect
import json
import math
import os
import sys
import threading
import time
import warnings
from collections.abc import Callable, Sequence
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from typing import Any

import numpy as np

from hark.forge import recipes as registry
from hark.forge.engine import dsp, tone

SR = 48000
SEEDS_DIR = Path(registry.__file__).resolve().parent.parent / "seeds"
ENV_KEYS = ("attack_ms", "t10_ms", "t20_ms", "t40_ms", "centroid_ms", "crest_db", "end_db")
_LOG_KEYS = ("attack_ms", "t10_ms", "t20_ms", "t40_ms", "centroid_ms")
_ENV_FLOOR = np.array([0.2] * 5 + [2 / 6] * 2)
_FAILED_ROW = {"total": -math.inf, "passes": False, "summary": "render failed or silent"}


class MidRangeFill(UserWarning):
    """A start key got the spec's mid-range because no DEFAULTS had it."""


def workers_of(workers: int | None = None) -> int:
    """Worker count: the argument, else HARK_THREADS, else 2; clamped to 1..8."""
    n = workers
    if not n:
        try:
            n = int(os.environ.get("HARK_THREADS") or 2)
        except ValueError:
            n = 2
    return int(min(8, max(1, n)))


# ---------------------------------------------------------------- encoding

def _kind(spec: Any) -> str:
    if isinstance(spec, list):
        return "choice"
    return spec[2] if len(spec) > 2 else "lin"


def _decode_one(spec: Any, v: float) -> Any:
    v = min(1.0, max(0.0, float(v)))
    kind = _kind(spec)
    if kind == "choice":
        return spec[min(len(spec) - 1, int(v * len(spec)))]
    lo, hi = spec[0], spec[1]
    if kind == "log":
        return float(math.exp(math.log(lo) + v * (math.log(hi) - math.log(lo))))
    if kind == "int":
        return int(round(lo + v * (hi - lo)))
    return float(lo + v * (hi - lo))


def _encode_one(spec: Any, v: Any) -> float:
    if v is None:
        return 0.5
    kind = _kind(spec)
    if kind == "choice":
        return (spec.index(v) + 0.5) / len(spec) if v in spec else 0.5
    lo, hi = spec[0], spec[1]
    try:
        if kind == "log":
            u = (math.log(max(float(v), 1e-12)) - math.log(lo)) / (math.log(hi) - math.log(lo))
        else:
            u = (float(v) - lo) / (hi - lo) if hi != lo else 0.5
    except (TypeError, ValueError):
        return 0.5
    return min(1.0, max(0.0, u))


def encode(space: dict, params: dict) -> np.ndarray:
    """Unit coordinates of params, in SPACE order; a missing key is 0.5."""
    return np.array([_encode_one(spec, params.get(k)) for k, spec in space.items()], dtype=float)


def decode(space: dict, u: Sequence[float]) -> dict:
    """Params from unit coordinates, in SPACE order."""
    return {k: _decode_one(spec, v) for (k, spec), v in zip(space.items(), u)}


def midrange(spec: Any) -> Any:
    """The value that unit coordinate 0.5 decodes to."""
    return _decode_one(spec, 0.5)


def inside(spec: Any, v: Any) -> bool:
    """True when v is a value of the spec."""
    try:
        if isinstance(spec, list):
            return v in spec
        ok = spec[0] <= v <= spec[1]
        return bool(ok and (_kind(spec) != "int" or float(v) == int(v)))
    except TypeError:
        return False


def clip_to(spec: Any, v: Any) -> Any:
    """The nearest value in a numeric range (rounded for int); None for choices."""
    if isinstance(spec, list):
        return None
    try:
        c = min(spec[1], max(spec[0], float(v)))
    except (TypeError, ValueError):
        return None
    return int(round(c)) if _kind(spec) == "int" else c


# ---------------------------------------------------------------- starts

def _render_module(recipe: Any) -> Any:
    fn = getattr(recipe, "render", None)
    while isinstance(fn, functools.partial):
        fn = fn.func
    return inspect.getmodule(fn) if fn is not None else None


def recipe_defaults(recipe: Any) -> dict:
    """DEFAULTS of the object, over the DEFAULTS of the module that defines render."""
    own = dict(getattr(recipe, "DEFAULTS", {}) or {})
    mod = _render_module(recipe)
    if mod is not None and mod is not recipe and isinstance(getattr(mod, "SPACE", None), dict) \
            and isinstance(getattr(mod, "DEFAULTS", None), dict):
        return {**mod.DEFAULTS, **own}
    return own


def _fill_key(key: str, spec: Any, start: dict, defaults: dict, own: dict) -> tuple[Any, dict | None]:
    if key in start:
        v, src = start[key], "start"
    elif key in defaults:
        v, src = defaults[key], "DEFAULTS" if key in own else "module DEFAULTS"
    else:
        mid = midrange(spec)
        return mid, {"key": key, "value": mid, "from": "mid-range"}
    if inside(spec, v):
        return v, ({"key": key, "value": v, "from": src} if src == "module DEFAULTS" else None)
    to = clip_to(spec, v)
    to = midrange(spec) if to is None else to
    if src == "start":
        return v, {"key": key, "value": to, "from": "start clipped", "was": v}
    return to, {"key": key, "value": to, "from": "DEFAULTS clipped", "was": v}


def fill_start(space: dict, start: dict, defaults: dict, own: dict | None = None) -> tuple[dict, list[dict]]:
    """A complete start dict for space, with notes on where each odd value came from."""
    own = defaults if own is None else own
    full, notes = dict(start), []
    for key, spec in space.items():
        full[key], note = _fill_key(key, spec, start, defaults, own)
        if note:
            notes.append(note)
    return full, notes


# ---------------------------------------------------------------- render

_WARNED: set[str] = set()
_WARN_LOCK = threading.Lock()


def _name_of(recipe: Any) -> str:
    return registry.short_name(recipe) or str(getattr(recipe, "__name__", "recipe"))


def _report_failure(recipe: Any, e: Exception) -> None:
    name = _name_of(recipe)
    with _WARN_LOCK:
        if name in _WARNED:
            return
        _WARNED.add(name)
    tb = e.__traceback__
    while tb is not None and tb.tb_next is not None:
        tb = tb.tb_next
    where = f"{tb.tb_frame.f_code.co_filename}:{tb.tb_lineno}" if tb else "?"
    print(f"forge: recipe {name} raised {type(e).__name__}: {e} ({where}); later failures of this recipe are not "
          "printed", file=sys.stderr)


def render(recipe: Any, params: dict, seed: int) -> np.ndarray | None:
    """One take peak-normalised to -1 dBFS as float32, or None when it fails or is silent."""
    try:
        x = np.asarray(recipe.render(dict(params), dsp.rng(int(seed))), dtype=np.float64)
    except Exception as e:  # noqa: BLE001 - a failing recipe scores -inf, it never stops a search
        _report_failure(recipe, e)
        return None
    if x.size == 0 or not np.all(np.isfinite(x)) or np.max(np.abs(x)) < 1e-7:
        return None
    return dsp.normalize(x, -1.0).astype(np.float32)


def render_many(recipe: Any, jobs: Sequence[tuple[dict, int]], workers: int | None = None) -> list:
    """Renders of [(params, seed), ...] in order; threads when useful."""
    n = workers_of(workers)
    if n > 1 and len(jobs) >= 2:
        with ThreadPoolExecutor(n) as ex:
            return list(ex.map(lambda j: render(recipe, j[0], j[1]), jobs))
    return [render(recipe, p, s) for p, s in jobs]


# ---------------------------------------------------------------- descriptor

def _mono(x: Any) -> np.ndarray:
    return tone.as_mono(x)


def _env_frames(m: np.ndarray, n: int) -> tuple[np.ndarray, np.ndarray]:
    k = len(m) // n
    pw = np.mean(m[: k * n].reshape(k, n) ** 2, axis=1)
    return pw, 10 * np.log10(pw + 1e-18)


def _drop_time(e: np.ndarray, ip: int, pk: float, db: float, hop: float) -> float:
    after = np.flatnonzero(e[ip:] < pk - db)
    return float((after[0] if len(after) else len(e) - ip) * hop)


def _crest(m: np.ndarray, e: np.ndarray, on: int, pk: float, n: int) -> float:
    live = np.repeat(e >= pk - 40, n)
    live[: on * n] = False
    seg = m[: len(live)][live]
    if not len(seg):
        return 0.0
    rms = math.sqrt(float(np.mean(seg ** 2)))
    return float(20 * math.log10(np.max(np.abs(m)) / rms)) if rms > 0 else 0.0


def envelope_stats(x: Any, hop_ms: float = 2.0) -> dict:
    """Attack, decay times, centroid time, crest and end level of the mono sum."""
    m = _mono(x)
    n = int(round(SR * hop_ms / 1000))
    if len(m) // n < 2:
        return {**{k: 0.0 for k in ENV_KEYS}, "end_db": -90.0}
    pw, e = _env_frames(m, n)
    ip, pk = int(np.argmax(e)), float(np.max(e))
    on = int(np.argmax(e > pk - 30))
    idx = np.arange(on, len(pw))
    w = pw[on:]
    q = max(1, len(pw) // 4)
    return {"attack_ms": (ip - on) * hop_ms + hop_ms / 2,
            "t10_ms": _drop_time(e, ip, pk, 10, hop_ms), "t20_ms": _drop_time(e, ip, pk, 20, hop_ms),
            "t40_ms": _drop_time(e, ip, pk, 40, hop_ms),
            "centroid_ms": float(np.sum((idx - on) * w) / (np.sum(w) + 1e-30)) * hop_ms + hop_ms / 2,
            "crest_db": _crest(m, e, on, pk, n),
            "end_db": max(-90.0, float(10 * math.log10(np.mean(pw[-q:]) + 1e-18) - pk))}


def _longest_run(flags: np.ndarray) -> int:
    best = cur = 0
    for f in flags:
        cur = cur + 1 if f else 0
        best = max(best, cur)
    return best


def _zero_gaps(m: np.ndarray) -> int:
    z = np.concatenate([[False], m == 0, [False]]).astype(int)
    starts, ends = np.flatnonzero(np.diff(z) == 1), np.flatnonzero(np.diff(z) == -1)
    return int(sum(1 for a, b in zip(starts, ends) if a > 0 and b < len(m) and b - a > 0.005 * SR))


def _ultra_db(m: np.ndarray) -> float:
    P = np.abs(np.fft.rfft(m)) ** 2
    f = np.fft.rfftfreq(len(m), 1 / SR)
    return float(10 * math.log10(P[f > 20000].sum() / (P.sum() + 1e-30) + 1e-12))


def _art_checks(m: np.ndarray, pk: float) -> list[tuple[str, Any, float]]:
    out: list[tuple[str, Any, float]] = []
    flat = (np.abs(m[1:]) > 0.98 * pk) & (np.abs(np.diff(m)) <= 1e-6 * pk)
    run = _longest_run(flat)
    if flat.sum() >= 4 and run >= 4:
        out.append(("flat_top", run, 1.0))
    rms = math.sqrt(float(np.mean(m ** 2))) + 1e-30
    if abs(float(np.mean(m))) / rms > 0.1:
        out.append(("dc", round(float(np.mean(m)) / rms, 3), 0.5))
    if abs(m[0]) / pk > 0.05:
        out.append(("start_jump", round(float(abs(m[0]) / pk), 3), 0.5))
    end = m[-int(0.005 * SR):]
    end_db = 20 * math.log10(math.sqrt(float(np.mean(end ** 2))) / pk + 1e-12)
    if end_db > -45:
        out.append(("cut_end", round(end_db, 1), 1.0))
    gaps = _zero_gaps(m)
    if gaps:
        out.append(("gap", gaps, 1.0))
    ultra = _ultra_db(m)
    if ultra > -25:
        out.append(("ultrasonic", round(ultra, 1), 0.5))
    return out


def artifacts(x: Any) -> dict:
    """Cheap artifact checks: {'flags': {...}, 'penalty'}."""
    m = _mono(x)
    if not len(m):
        return {"flags": {"silent": 1}, "penalty": 9.0}
    checks = _art_checks(m, float(np.max(np.abs(m))) + 1e-12)
    return {"flags": {k: v for k, v, _ in checks}, "penalty": float(sum(p for _, _, p in checks))}


def describe(x: Any) -> dict:
    """Model-free descriptor: tone profile, envelope stats and cheap artifacts."""
    return {"profile": tone.profile(x), "env": envelope_stats(x), "art": artifacts(x)}


def env_vector(env: dict) -> np.ndarray:
    """Envelope vector: log times, then crest and end level in 6 dB units."""
    v = [math.log(max(float(env.get(k, 0.0) or 0.0), 0.5)) for k in _LOG_KEYS]
    return np.array(v + [float(env.get("crest_db", 0.0)) / 6, float(env.get("end_db", -90.0)) / 6])


def sound_dist(a: dict, b: dict) -> float:
    """Profile RMS difference + 4 x envelope vector RMS difference."""
    dp = np.asarray(a["profile"]) - np.asarray(b["profile"])
    de = env_vector(a["env"]) - env_vector(b["env"])
    return float(np.sqrt(np.mean(dp ** 2)) + 4 * np.sqrt(np.mean(de ** 2)))


# ---------------------------------------------------------------- pre-screen

def _load_refs(refs: Any) -> list[np.ndarray]:
    return [_mono(r) for r in refs or []]


def _ref_spread(P: np.ndarray) -> float:
    if len(P) < 3:
        return 4.0
    loo = [float(np.sqrt(np.mean((P[i] - np.median(np.delete(P, i, 0), axis=0)) ** 2))) for i in range(len(P))]
    return max(float(np.median(loo)), 0.5)


class Prescreen:
    """Ranks sounds against refs by measurement only."""

    def __init__(self, refs: Any = None, tone_target: float | None = None, env_w: float = 0.5,
                 env_keys: Sequence[str] = ("t40_ms", "crest_db", "end_db"), polish_ref: Any = None,
                 polish: bool = True, study: dict | None = None):
        self.refs = list(refs or [])
        self.arrays = _load_refs(self.refs)
        self.env_w, self.env_keys = env_w, tuple(env_keys)
        self.study = self._study(study)
        self.polish_ref = polish_ref if polish_ref is not None else self._polish(polish)
        self.descs = [describe(a) for a in self.arrays]
        self._ref_stats(tone_target)

    def _study(self, study: dict | None) -> dict | None:
        if not study or len(self.arrays) >= 3:
            return None
        from hark.forge.engine import critic

        return critic.widen_targets(study, [critic.study_measures(a) for a in self.arrays])

    def _polish(self, polish: bool) -> Any:
        if not polish:
            return None
        try:
            from hark.forge.engine import polish as pol

            return pol.PolishRef(self.arrays)
        except Exception:  # noqa: BLE001 - the cheap artifact checks rank alone
            return None

    def _ref_stats(self, tone_target: float | None) -> None:
        if not self.descs:
            self.med = self.spread = self.env_med = self.env_sd = None
            return
        P = np.array([d["profile"] for d in self.descs])
        self.med = np.median(P, axis=0)
        self.spread = float(tone_target) if tone_target else _ref_spread(P)
        V = np.array([env_vector(d["env"]) for d in self.descs])
        self.env_med = np.median(V, axis=0)
        iqr = np.percentile(V, 75, axis=0) - np.percentile(V, 25, axis=0)
        self.env_sd = np.maximum(iqr / 1.35, _ENV_FLOOR)

    def _polish_art(self, x: Any, row: dict, art: float) -> float:
        try:
            c = self.polish_ref.check(x)
        except Exception:  # noqa: BLE001 - keep the cheap penalty
            return art
        row["polish"] = {"art_pen": c["art_pen"], "polish_pen": c["polish_pen"], "hard": c["hard"]}
        return c["art_pen"] + 0.75 * c["polish_pen"] + 1.0 * len(c["hard"])

    def _study_art(self, x: Any, row: dict) -> float:
        from hark.forge.engine import critic

        pen, off, _ = critic.study_penalties(critic.study_measures(x), self.study)
        row["study"] = off
        return pen.get("tone", 0.0) + pen.get("env", 0.0) + 0.75 * pen.get("polish", 0.0)

    def _ref_terms(self, d: dict, row: dict) -> float:
        tone_db = float(np.sqrt(np.mean((np.asarray(d["profile"]) - self.med) ** 2)))
        ratio = tone_db / self.spread
        z = (env_vector(d["env"]) - self.env_med) / self.env_sd
        zk = {k: round(float(z[ENV_KEYS.index(k)]), 2) for k in ENV_KEYS}
        env_pen = float(np.mean([max(0.0, abs(zk[k]) - 1.5) for k in self.env_keys]))
        row.update(tone_db=round(tone_db, 2), tone_ratio=round(ratio, 3), env_z=zk, env_pen=env_pen)
        return ratio + self.env_w * env_pen

    def row(self, x: Any, polish: bool = True) -> dict:
        """Pre-screen row of one sound; higher 'pre' is better."""
        d = describe(x)
        row: dict = {"desc": d}
        art = d["art"]["penalty"]
        if polish and self.polish_ref is not None:
            art = self._polish_art(x, row, art)
        if self.study:
            art += self._study_art(x, row)
        row["art"] = d["art"]
        row["art_pen"] = art
        ref = self._ref_terms(d, row) if self.med is not None else 0.0
        row["pre"] = -(ref + art)
        return row

    def score(self, xs: Sequence[Any], polish: bool = True) -> list[dict]:
        """A row per sound; a None sound ranks last."""
        return [self.row(x, polish) if x is not None else
                {"pre": -math.inf, "art": {"flags": {"silent": 1}, "penalty": 9.0}} for x in xs]

    def nearest(self, desc: dict) -> int | None:
        """Index of the ref with the smallest sound distance, or None without refs."""
        if not self.descs:
            return None
        return int(np.argmin([sound_dist(desc, r) for r in self.descs]))


def prescreen_for(critic: Any) -> Prescreen:
    """A Prescreen from a critic's refs, tone target, polish ref and study targets."""
    return Prescreen(getattr(critic, "refs", None) or None, tone_target=getattr(critic, "tone_target", None),
                     polish_ref=getattr(critic, "polish_ref", None), study=getattr(critic, "study", None) or None)


# ---------------------------------------------------------------- separable CMA-ES

def _mirror(x: np.ndarray) -> np.ndarray:
    r = np.mod(x, 2.0)
    return np.where(r > 1, 2 - r, r)


class SepCMA:
    """Separable CMA-ES in the unit cube (diagonal covariance)."""

    def __init__(self, d: int, lam: int, mu_frac: float = 0.5, elite: float = 0.0):
        self.d, self.lam, self.elite_share = d, lam, elite
        self.mu = max(1, int(round(lam * mu_frac)))
        w = np.log(self.mu + 0.5) - np.log(np.arange(1, self.mu + 1))
        self.w = w / w.sum()
        self.mueff = 1.0 / np.sum(self.w ** 2)
        self._rates()
        self.elites: list[np.ndarray] = []

    def _rates(self) -> None:
        d, me = self.d, self.mueff
        self.cs = (me + 2) / (d + me + 5)
        self.ds = 1 + 2 * max(0.0, math.sqrt((me - 1) / (d + 1)) - 1) + self.cs
        self.cc = (4 + me / d) / (d + 4 + 2 * me / d)
        sep = (d + 2) / 3
        self.c1 = min(1.0, 2 * sep / ((d + 1.3) ** 2 + me))
        self.cmu = min(1 - self.c1, 2 * sep * (me - 2 + 1 / me) / ((d + 2) ** 2 + me))
        self.chi = math.sqrt(d) * (1 - 1 / (4 * d) + 1 / (21 * d * d))

    def reset(self, m: np.ndarray, sigma: float) -> None:
        self.m, self.sigma = np.asarray(m, dtype=float).copy(), float(sigma)
        self.C = np.ones(self.d)
        self.ps = np.zeros(self.d)
        self.pc = np.zeros(self.d)
        self.g = 0

    def ask(self, n: int, R: np.random.Generator) -> np.ndarray:
        Z = R.standard_normal((n, self.d))
        n_el = int(round(n * self.elite_share)) if self.elites else 0
        centres = [self.elites[i % len(self.elites)] for i in range(n_el)] + [self.m] * (n - n_el)
        return _mirror(np.array(centres) + self.sigma * np.sqrt(self.C) * Z)

    def tell(self, U: np.ndarray, totals: Sequence[float]) -> None:
        order = np.argsort(-np.asarray(totals, dtype=float), kind="stable")[: self.mu]
        Y = (np.asarray(U)[order] - self.m) / self.sigma
        norm = np.sqrt(np.sum(Y ** 2 / self.C, axis=1))
        Y = Y * np.minimum(1.0, 2 * self.chi / np.maximum(norm, 1e-12))[:, None]
        w = self.w[: len(Y)] / self.w[: len(Y)].sum()
        yw = w @ Y
        self.m = np.clip(self.m + self.sigma * yw, 0, 1)
        self.g += 1
        self._update_paths(yw, w, Y)

    def _update_paths(self, yw: np.ndarray, w: np.ndarray, Y: np.ndarray) -> None:
        cs, cc, me = self.cs, self.cc, self.mueff
        self.ps = (1 - cs) * self.ps + math.sqrt(cs * (2 - cs) * me) * yw / np.sqrt(self.C)
        nps = float(np.linalg.norm(self.ps))
        hs = nps / math.sqrt(1 - (1 - cs) ** (2 * self.g)) < (1.4 + 2 / (self.d + 1)) * self.chi
        self.pc = (1 - cc) * self.pc + hs * math.sqrt(cc * (2 - cc) * me) * yw
        self.C = ((1 - self.c1 - self.cmu) * self.C + self.c1 * (self.pc ** 2 + (1 - hs) * cc * (2 - cc) * self.C)
                  + self.cmu * (w @ (Y ** 2)))
        self.C = np.clip(self.C, 1e-4, 16)
        self.sigma = float(np.clip(self.sigma * math.exp((cs / self.ds) * (nps / self.chi - 1)), 1e-3, 0.6))


# ---------------------------------------------------------------- optimiser

def _fill_warnings(space: dict, starts: list[dict], recipe: Any, defaults: dict, log: Callable | None) -> tuple:
    rdef = recipe_defaults(recipe)
    full, warn = [], []
    for j, s in enumerate(starts):
        f, notes = fill_start(space, s, rdef, own=defaults)
        full.append(f)
        warn += [{"start": j, **n} for n in notes if n["from"] != "module DEFAULTS"]
    if warn:
        items = "; ".join(f"start {w['start']} {w['key']} = {w['value']} ({w['from']})" for w in warn[:8])
        msg = f"forge: {len(warn)} start value(s) not from the start or DEFAULTS: {items}"
        if len(warn) > 8:
            msg += " ..."
        if log:
            log(msg)
        if any(w["from"] == "mid-range" for w in warn):
            warnings.warn(msg, MidRangeFill, stacklevel=3)
    return full, warn


class _Search:
    """State of one optimize() call."""

    def __init__(self, recipe: Any, critic: Any, kw: dict):
        self.rec, self.critic, self.kw = recipe, critic, kw
        self.base = dict(getattr(recipe, "DEFAULTS", {}) or {})
        self.space = kw["space"] if kw["space"] is not None else recipe.SPACE
        self.keys = list(self.space)
        self.R = np.random.default_rng(kw["seed"])
        starts = [dict(self.base)] + [{**self.base, **i} for i in kw["init"] or []]
        self.starts, self.fill_warn = _fill_warnings(self.space, starts, recipe, self.base, kw["log"])
        self.U0 = [encode(self.space, s) for s in self.starts]
        self.gens = max(1, kw["budget"] // kw["pop"])
        self.K = kw["prescreen"] if kw["prescreen"] > kw["pop"] else 0
        self.pre = kw["pre"] if kw["pre"] is not None else (prescreen_for(critic) if self.K else None)
        self.history: list[dict] = []
        self.elites: list[dict] = []
        self.stats: list[dict] = []
        self.timing = {"render_s": 0.0, "pre_s": 0.0, "score_s": 0.0, "rendered": 0, "scored": 0}
        self.t0 = time.time()
        self._cma_setup()

    def _cma_setup(self) -> None:
        self.cma = None
        self.restarts, self.stale, self.best_seen, self.tried = 0, 0, -math.inf, []
        if self.kw["strategy"] != "cma":
            return
        opts = {"sigma0": 0.15, "mu": 0.5, "elite": 0.0, **(self.kw["cma"] or {})}
        self.cma_sigma0 = opts["sigma0"]
        self.cma = SepCMA(max(1, len(self.keys)), self.kw["pop"], opts["mu"], opts["elite"])
        self.cma.reset(self.U0[0], self.cma_sigma0)

    # -- candidates

    def _seed(self) -> int:
        return int(self.R.integers(0, 2 ** 31))

    def _es_candidates(self, g: int, n_new: int) -> tuple[list, list, list]:
        kw = self.kw
        sig = kw["sigma0"] * (kw["sigma1"] / kw["sigma0"]) ** (g / max(1, self.gens - 1))
        self.sigma = sig
        U, seeds, fixed = [], [], []
        for j in range(n_new):
            if g == 0 and j < len(self.U0) and j < kw["pop"]:
                u, fx = self.U0[j].copy(), j
            elif g == 0:
                u, fx = self.R.random(len(self.keys)), None
            else:
                u, fx = self._es_child(), None
            U.append(u)
            fixed.append(fx)
            seeds.append(self._seed())
        return U, seeds, fixed

    def _es_child(self) -> np.ndarray:
        if self.R.random() < self.kw["fresh"] or not self.elites:
            return self.R.random(len(self.keys))
        k = int(self.R.integers(0, max(1, min(self.kw["elite"], len(self.elites)))))
        parent = self.elites[min(len(self.elites) - 1, k)]["u"]
        return np.clip(parent + self.R.normal(0, self.sigma, len(self.keys)), 0, 1)

    def _cma_candidates(self, g: int, n_new: int) -> tuple[list, list, list]:
        U, fixed = [], []
        if g == 0:
            for j, u in enumerate(self.U0[: self.kw["pop"]]):
                U.append(u.copy())
                fixed.append(j)
        rest = n_new - len(U)
        if rest > 0:
            U += list(self.cma.ask(rest, self.R))
            fixed += [None] * rest
        self.sigma = self.cma.sigma
        return U, [self._seed() for _ in U], fixed

    def _params(self, u: np.ndarray, fixed: int | None) -> dict:
        p = {**self.base, **decode(self.space, u)}
        if fixed is not None:
            st = self.starts[fixed]
            for k, v in st.items():
                if k not in self.space or inside(self.space[k], v):
                    p[k] = v
        return p

    # -- evaluation

    def _prescreen(self, P: list[dict], seeds: list[int], fixed: list) -> tuple[list, list, list, list]:
        t = time.time()
        X = render_many(self.rec, list(zip(P, seeds)), self.kw["workers"])
        self.timing["render_s"] += time.time() - t
        self.timing["rendered"] += len(X)
        t = time.time()
        rows = self.pre.score(X, polish=True)
        self.timing["pre_s"] += time.time() - t
        order = sorted(range(len(P)), key=lambda i: (fixed[i] is None, -rows[i]["pre"], i))[: self.kw["pop"]]
        return order, [X[i] for i in order], [rows[i] for i in order], rows

    def _evaluate(self, P: list[dict], seeds: list[int], first: list | None) -> list[dict | None]:
        reps = max(1, self.kw["reps"])
        jobs = [(j, k, seeds[j] + 7919 * k) for j in range(len(P)) for k in range(reps)]
        todo = [(j, k, s) for j, k, s in jobs if first is None or k > 0]
        t = time.time()
        X = render_many(self.rec, [(P[j], s) for j, _, s in todo], self.kw["workers"])
        self.timing["render_s"] += time.time() - t
        if first is None:
            self.timing["rendered"] += len(X)
        renders = {(j, k): x for (j, k, _), x in zip(todo, X)}
        if first is not None:
            renders.update({(j, 0): x for j, x in enumerate(first)})
        self.rep0 = [renders.get((j, 0)) for j in range(len(P))]
        return self._score_reps(len(P), reps, renders)

    def _score_reps(self, n: int, reps: int, renders: dict) -> list[dict | None]:
        live = [(jk, x) for jk, x in sorted(renders.items()) if x is not None]
        t = time.time()
        rows = self.critic.score([x for _, x in live]) if live else []
        self.timing["score_s"] += time.time() - t
        self.timing["scored"] += len(rows)
        got = {jk: r for (jk, _), r in zip(live, rows)}
        out: list[dict | None] = []
        for j in range(n):
            rs = [got.get((j, k)) for k in range(reps)]
            out.append(None if any(r is None for r in rs) else _merge_reps(rs))
        return out

    # -- generation

    def generation(self, g: int) -> None:
        n_new = self.K or self.kw["pop"]
        U, seeds, fixed = (self._cma_candidates if self.cma else self._es_candidates)(g, n_new)
        P = [self._params(u, f) for u, f in zip(U, fixed)]
        pre_rows = first = None
        if self.K:
            order, first, pre_rows, _ = self._prescreen(P, seeds, fixed)
            U, seeds, fixed, P = ([v[i] for i in order] for v in (U, seeds, fixed, P))
        rows = self._evaluate(P, seeds, first)
        descs = self._descs(pre_rows)
        gen_rows = [self._record(g, U[i], P[i], seeds[i], rows[i], descs[i], pre_rows[i] if pre_rows else None)
                    for i in range(len(P))]
        self.elites = sorted([h for h in self.history if math.isfinite(h["score"]["total"])],
                             key=lambda h: -h["score"]["total"])[: self.kw["elite"]]
        self._gen_stats(g, gen_rows, len(P) if not self.K else self.K)
        if self.cma:
            self._cma_tell(g, U, gen_rows)

    def _descs(self, pre_rows: list | None) -> list:
        if pre_rows is not None:
            return [r.get("desc") for r in pre_rows]
        X = self.rep0
        if self.pre is not None:
            return [r.get("desc") for r in self.pre.score(X, polish=False)]
        return [describe(x) if x is not None else None for x in X]

    def _record(self, g: int, u: np.ndarray, p: dict, seed: int, row: dict | None, desc: Any,
                pre: dict | None) -> dict:
        h = {"gen": g, "u": np.asarray(u), "params": p, "seed": seed, "score": row or dict(_FAILED_ROW)}
        if desc is not None:
            h["desc"] = desc
        if pre is not None and self.pre is not None and self.pre.descs and "pre" in pre:
            h["pre"] = {k: pre.get(k) for k in ("pre", "tone_db", "tone_ratio", "env_pen")}
            h["pre"]["art"] = pre.get("art", {}).get("flags")
            if "polish" in pre:
                h["pre"]["polish"] = pre["polish"]
        self.history.append(h)
        return h

    def _gen_stats(self, g: int, rows: list[dict], rendered: int) -> None:
        tot = [r["score"]["total"] for r in rows if math.isfinite(r["score"]["total"])]
        best = self.elites[0]["score"] if self.elites else {}
        st = {"gen": g + 1, "best": best.get("total"), "gen_best": max(tot) if tot else None,
              "gen_median": float(np.median(tot)) if tot else None,
              "passes": sum(1 for r in rows if r["score"].get("passes")), "scored": len(rows),
              "rendered": rendered, "secs": round(time.time() - self.t0, 2), "sigma": round(self.sigma, 4)}
        if best.get("tone_db") is not None:
            st["best_tone_db"] = best["tone_db"]
        self.stats.append(st)
        self._log_gen(st, best)

    def _log_gen(self, st: dict, best: dict) -> None:
        log = self.kw["log"]
        if not log or not self.elites:
            return
        line = (f"gen {st['gen']}/{self.gens}: best total {best['total']:.2f} "
                f"({'PASS' if best.get('passes') else 'no pass'}) z {best.get('match_z')} "
                f"p_real {best.get('p_real')} typ {best.get('ref_typ_z')} pen {best.get('feat_pen')} "
                f"sigma {st['sigma']}")
        if st.get("restart"):
            line += f" restart {st['restart']}"
        if self.K:
            line += f" [{self.K} rendered, {self.kw['pop']} scored]"
        log(line)

    def _cma_tell(self, g: int, U: list, rows: list[dict]) -> None:
        fin = [(u, r["score"]["total"]) for u, r in zip(U, rows) if math.isfinite(r["score"]["total"])]
        if fin:
            self.cma.tell(np.array([u for u, _ in fin]), [t for _, t in fin])
        best = self.elites[0]["score"]["total"] if self.elites else -math.inf
        if best > self.best_seen + 1e-3:
            self.best_seen, self.stale = best, 0
        else:
            self.stale += 1
        if g < self.gens - 1 and (self.cma.sigma < self.kw["sigma_stop"] or self.stale >= self.kw["patience"]):
            self._restart()

    def _restart(self) -> None:
        self.tried.append(self.cma.m.copy())
        d = max(1, len(self.keys))
        ranked = sorted([h for h in self.history if math.isfinite(h["score"]["total"])],
                        key=lambda h: -h["score"]["total"])[:24]
        far = [h for h in ranked if all(np.linalg.norm(h["u"] - t) > 0.2 * math.sqrt(d) for t in self.tried)]
        self.restarts += 1
        if self.restarts % 2 == 1 and far:
            start, kind = far[0]["u"], "elite"
        else:
            start, kind = self.R.random(d), "random"
        self.cma.reset(start, self.cma_sigma0)
        self.stale = 0
        self.stats[-1]["restart"] = kind

    def result(self) -> dict:
        fin = sorted([h for h in self.history if math.isfinite(h["score"]["total"])],
                     key=lambda h: -h["score"]["total"])
        tm = dict(self.timing)
        total_s = time.time() - self.t0
        mins = max(total_s / 60, 1e-9)
        tm.update(render_s=round(tm["render_s"], 2), pre_s=round(tm["pre_s"], 2), score_s=round(tm["score_s"], 2),
                  total_s=round(total_s, 2), scored_per_min=round(tm["scored"] / mins, 1),
                  rendered_per_min=round(tm["rendered"] / mins, 1))
        return {"best": fin[: max(self.kw["elite"], 8)], "history": self.history, "generations": self.stats,
                "gens": self.gens, "timing": tm, "strategy": self.kw["strategy"], "prescreen": self.K,
                "restarts": self.restarts, "full_space": self.kw["space"] is None,
                "fill_warnings": self.fill_warn}


def _merge_reps(rows: list[dict]) -> dict:
    if len(rows) == 1:
        return rows[0]
    tots = [r["total"] for r in rows]
    out = dict(rows[0])
    out["total"] = float(np.mean(tots)) if all(math.isfinite(t) for t in tots) else -math.inf
    out["total_reps"] = tots
    out["passes"] = sum(1 for r in rows if r.get("passes")) >= len(rows) / 2
    return out


def optimize(recipe: Any, critic: Any, budget: int = 160, pop: int = 16, elite: int = 4, seed: int = 0,
             init: Sequence[dict] | None = None, sigma0: float = 0.22, sigma1: float = 0.04, fresh: float = 0.15,
             log: Callable | None = None, space: dict | None = None, reps: int = 1, strategy: str = "es",
             prescreen: int = 0, workers: int | None = None, patience: int = 4, sigma_stop: float = 0.03,
             pre: Prescreen | None = None, cma: dict | None = None) -> dict:
    """Search a recipe's SPACE against a critic; returns best, history, generations and timing."""
    kw = dict(budget=budget, pop=pop, elite=elite, seed=seed, init=init, sigma0=sigma0, sigma1=sigma1, fresh=fresh,
              log=log, space=space, reps=reps, strategy=strategy, prescreen=prescreen, workers=workers,
              patience=patience, sigma_stop=sigma_stop, pre=pre, cma=cma)
    s = _Search(recipe, critic, kw)
    for g in range(s.gens):
        s.generation(g)
    return s.result()


# ---------------------------------------------------------------- robust and diverse

def robust(recipe: Any, critic: Any, params: dict, seeds: Any = 4, seed0: int = 5000,
           workers: int | None = None) -> dict:
    """Score a parameter set on several seeds: mean, min, max, sd and pass rate."""
    sl = list(seeds) if isinstance(seeds, (list, tuple)) else list(range(seed0, seed0 + int(seeds)))
    X = render_many(recipe, [(params, s) for s in sl], workers)
    live = [x for x in X if x is not None]
    rows = critic.score(live) if live else []
    tots = [float(r["total"]) for r in rows if math.isfinite(r["total"])]
    out = {"n": len(sl), "seeds": sl, "totals": [round(t, 3) for t in tots], "failed": len(X) - len(live),
           "mean": None, "min": None, "max": None, "sd": None,
           "pass_rate": round(sum(1 for r in rows if r.get("passes")) / max(1, len(sl)), 3)}
    if tots:
        out.update(mean=float(np.mean(tots)), min=min(tots), max=max(tots), sd=float(np.std(tots)))
    tdb = [r["tone_db"] for r in rows if r.get("tone_db") is not None]
    if tdb:
        out["tone_db_mean"] = float(np.mean(tdb))
    return out


def _rank_key(r: dict) -> tuple:
    return (not r["score"].get("passes"), -r["score"]["total"])


def _accept(r: dict, chosen: list[dict], min_dist: float, min_sound: float, sound: bool) -> bool:
    for c in chosen:
        if c is r:
            return False
        u, v = np.asarray(r["u"]), np.asarray(c["u"])
        if np.linalg.norm(u - v) / math.sqrt(max(1, len(u))) < min_dist:
            return False
        if sound and r.get("desc") and c.get("desc") and sound_dist(r["desc"], c["desc"]) < min_sound:
            return False
    return True


def diverse(results: Sequence[dict], recipe: Any, k: int = 4, min_dist: float = 0.08,
            min_sound: float = 2.0) -> list[dict]:
    """Up to k strong results with different parameters and sound."""
    fin = [r for r in results if math.isfinite(r["score"]["total"])]
    pool = sorted(fin if fin else list(results), key=_rank_key)
    chosen: list[dict] = []
    for sound in ([True, False] if min_sound else [False]):
        for r in pool:
            if len(chosen) >= k:
                break
            if _accept(r, chosen, min_dist, min_sound, sound):
                chosen.append(r)
    return sorted(chosen, key=_rank_key)


# ---------------------------------------------------------------- seed bank

def seeds_path(recipe: Any, folder: Any = None) -> Path:
    """<folder>/<recipe>.json, else the registry's seed path of the recipe."""
    name = recipe if isinstance(recipe, str) else _name_of(recipe)
    if folder is not None:
        return Path(folder) / f"{name}.json"
    try:
        return registry.seed_path(name)
    except KeyError:
        return SEEDS_DIR / f"{name}.json"


def _context(target: str | None, refs: Any) -> str:
    names = sorted(os.path.basename(str(r).replace("\\", "/")) for r in refs or [])
    return f"{target or ''} | {','.join(names)}"


def _sig6(v: Any) -> Any:
    if isinstance(v, float) and math.isfinite(v):
        return float(f"{v:.6g}")
    if isinstance(v, np.floating):
        return float(f"{float(v):.6g}")
    if isinstance(v, np.integer):
        return int(v)
    return v


def _read_bank(path: Path, name: str) -> dict:
    if path.is_file():
        try:
            return json.loads(path.read_text())
        except (OSError, ValueError):
            pass
    return {"recipe": name, "entries": []}


def _rank(e: dict) -> float:
    r = e.get("rank_score")
    return float(r) if isinstance(r, (int, float)) and math.isfinite(r) else -1e9


def load_seeds(recipe: Any, target: str | None = None, refs: Any = None, n: int = 6, folder: Any = None) -> list:
    """Up to n start param dicts from the seed bank, same context and delivered first."""
    path = seeds_path(recipe, folder)
    if not path.is_file():
        return []
    ctx = _context(target, refs)
    entries = _read_bank(path, "").get("entries") or []
    entries = sorted(entries, key=lambda e: (e.get("context") != ctx, not e.get("delivered"), -_rank(e)))
    return [dict(e["params"]) for e in entries[:n]]


def _entry(p: dict, ctx: str, source: str, tag: str | None) -> dict | None:
    sc = p.get("score") or {}
    rb = p.get("robust") or {}
    rank = rb.get("mean") if rb.get("mean") is not None else sc.get("total")
    finite = isinstance(rank, (int, float)) and math.isfinite(rank)
    if not finite and tag is None:
        return None
    e = {"params": {k: _sig6(v) for k, v in p["params"].items()}, "rank_score": round(rank, 3) if finite else None,
         "context": ctx, "total": sc.get("total"), "tone_db": sc.get("tone_db"), "passes": sc.get("passes"),
         "source": source}
    if rb:
        e.update(robust_mean=rb.get("mean"), robust_pass_rate=rb.get("pass_rate"))
    if tag is not None:
        member = next((p[k] for k in ("family", "preset", "member", "name") if p.get(k) is not None), None)
        e.update(delivered=tag, member=member, seed=p.get("seed"))
    return e


def _keep_entries(entries: list[dict], per_context: int, cap: int) -> list[dict]:
    pinned = [e for e in entries if e.get("delivered")]
    kept: list[dict] = []
    for ctx in dict.fromkeys(e.get("context") for e in entries):
        seen = [e["params"] for e in pinned if e.get("context") == ctx]
        added = 0
        for e in sorted([e for e in entries if e.get("context") == ctx and not e.get("delivered")],
                        key=lambda e: -_rank(e)):
            if added >= per_context:
                break
            if e["params"] in seen:
                continue
            seen.append(e["params"])
            kept.append(e)
            added += 1
    return pinned + kept[: max(0, cap - len(pinned))]


def save_seeds(recipe: Any, picks: Sequence[dict], target: str | None = None, refs: Any = None, source: str = "",
               folder: Any = None, per_context: int = 6, cap: int = 48, delivered: Any = None) -> dict:
    """Add picks to the seed bank and write it atomically."""
    name = recipe if isinstance(recipe, str) else _name_of(recipe)
    path = seeds_path(recipe, folder)
    bank = _read_bank(path, name)
    ctx = _context(target, refs)
    tag = ctx if delivered is True else (str(delivered) if delivered else None)
    new = [e for e in (_entry(p, ctx, source, tag) for p in picks) if e is not None]
    old = bank.get("entries") or []
    if tag is not None:
        for e in old:
            if e.get("delivered") == tag:
                e.pop("delivered")
                e["was_delivered"] = tag
    bank = {"recipe": name, "entries": _keep_entries(new + old, per_context, cap)}
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(f"{path.name}.tmp{os.getpid()}")
    tmp.write_text(json.dumps(bank, indent=1, default=str))
    os.replace(tmp, path)
    return bank


def report_members(report: dict) -> list[dict]:
    """A report's picks as save_seeds picks."""
    out = []
    for p in report.get("picks") or []:
        m = {k: p[k] for k in ("score", "robust", "seed", "family", "preset", "member", "name") if k in p}
        m["params"] = {**(p.get("family_params") or {}), **(p.get("params") or {})}
        m.setdefault("score", {})
        out.append(m)
    return out


def pin_delivered(recipe: Any, report: Any, category: str, target: str | None = None, refs: Any = None,
                  folder: Any = None, source: str | None = None) -> dict:
    """Pin a delivered report's members in the seed bank under category."""
    path = None
    if not isinstance(report, dict):
        path = Path(report)
        report = json.loads((path / "report.json" if path.is_dir() else path).read_text())
    src = source or f"delivered final ({category})" + (f" {path}" if path else "")
    return save_seeds(recipe, report_members(report), target=target or report.get("target"),
                      refs=refs if refs is not None else report.get("refs"), source=src, folder=folder,
                      delivered=category)
