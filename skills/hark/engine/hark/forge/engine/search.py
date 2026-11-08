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
_TIME_FIELDS = ("attack_ms", "t10_ms", "t20_ms", "t40_ms", "centroid_ms")
_MIN_SCALE = np.array([0.2] * 5 + [2 / 6] * 2)
_DEAD_SCORE = {"total": -math.inf, "passes": False, "summary": "render failed or silent"}


class MidRangeFill(UserWarning):
    """A start key got the spec's mid-range because no DEFAULTS had it."""


def workers_of(workers: int | None = None) -> int:
    """Worker count: the argument, else HARK_THREADS, else 2; clamped to 1..8."""
    cnt = workers
    if not cnt:
        try:
            cnt = int(os.environ.get("HARK_THREADS") or 2)
        except ValueError:
            cnt = 2
    return int(min(8, max(1, cnt)))


# ---------------------------------------------------------------- encoding

def _range_type(sp: Any) -> str:
    if isinstance(sp, list):
        return "choice"
    return sp[2] if len(sp) > 2 else "lin"


def _from_unit_geo(low: float, high: float, val: float) -> float:
    return float(math.exp(math.log(low) + val * (math.log(high) - math.log(low))))


def _from_unit_round(low: float, high: float, val: float) -> int:
    return int(round(low + val * (high - low)))


def _from_unit_affine(low: float, high: float, val: float) -> float:
    return float(low + val * (high - low))


_UNIT_MAPPERS = {"log": _from_unit_geo, "int": _from_unit_round}


def _value_at(sp: Any, val: float) -> Any:
    val = min(1.0, max(0.0, float(val)))
    if isinstance(sp, list):
        return sp[min(len(sp) - 1, int(val * len(sp)))]
    return _UNIT_MAPPERS.get(_range_type(sp), _from_unit_affine)(sp[0], sp[1], val)


def _position(form: str, low: float, high: float, val: Any) -> float:
    if form == "log":
        return (math.log(max(float(val), 1e-12)) - math.log(low)) / (math.log(high) - math.log(low))
    return (float(val) - low) / (high - low) if high != low else 0.5


def _range_position(sp: Any, val: Any) -> float:
    try:
        pt = _position(_range_type(sp), sp[0], sp[1], val)
    except (TypeError, ValueError):
        pt = 0.5
    return min(1.0, max(0.0, pt))


def _coord_of(sp: Any, val: Any) -> float:
    if val is None:
        pt = 0.5
    elif isinstance(sp, list):
        pt = (sp.index(val) + 0.5) / len(sp) if val in sp else 0.5
    else:
        pt = _range_position(sp, val)
    return pt


def encode(space: dict, params: dict) -> np.ndarray:
    """Unit coordinates of params, in SPACE order; a missing key is 0.5."""
    return np.array([_coord_of(sp, params.get(ky)) for ky, sp in space.items()], dtype=float)


def decode(space: dict, u: Sequence[float]) -> dict:
    """Params from unit coordinates, in SPACE order."""
    return {ky: _value_at(sp, val) for (ky, sp), val in zip(space.items(), u)}


def midrange(spec: Any) -> Any:
    """The value that unit coordinate 0.5 decodes to."""
    return _value_at(spec, 0.5)


def inside(spec: Any, v: Any) -> bool:
    """True when v is a value of the spec."""
    try:
        if isinstance(spec, list):
            return v in spec
        within = spec[0] <= v <= spec[1]
        return bool(within and (_range_type(spec) != "int" or float(v) == int(v)))
    except TypeError:
        return False


def clip_to(spec: Any, v: Any) -> Any:
    """The nearest value in a numeric range (rounded for int); None for choices."""
    if isinstance(spec, list):
        return None
    try:
        cand = min(spec[1], max(spec[0], float(v)))
    except (TypeError, ValueError):
        return None
    return int(round(cand)) if _range_type(spec) == "int" else cand


# ---------------------------------------------------------------- starts

def _owner_module(rcp: Any) -> Any:
    func = getattr(rcp, "render", None)
    while isinstance(func, functools.partial):
        func = func.func
    return inspect.getmodule(func) if func is not None else None


def recipe_defaults(recipe: Any) -> dict:
    """DEFAULTS of the object, over the DEFAULTS of the module that defines render."""
    own_d = dict(getattr(recipe, "DEFAULTS", {}) or {})
    module = _owner_module(recipe)
    if module is not None and module is not recipe and isinstance(getattr(module, "SPACE", None), dict) \
            and isinstance(getattr(module, "DEFAULTS", None), dict):
        return {**module.DEFAULTS, **own_d}
    return own_d


def _lookup_value(fld: str, origin: dict, dflt: dict, own_d: dict) -> tuple[Any, str]:
    sources = ((origin, "start"), (dflt, "DEFAULTS" if fld in own_d else "module DEFAULTS"))
    return next(((dd[fld], lbl) for dd, lbl in sources if fld in dd), (None, "mid-range"))


def _clamped_note(fld: str, sp: Any, val: Any, origin_s: str) -> tuple[Any, dict]:
    clipped_v = clip_to(sp, val)
    clipped_v = midrange(sp) if clipped_v is None else clipped_v
    keep_l, lbl = (val, "start clipped") if origin_s == "start" else (clipped_v, "DEFAULTS clipped")
    return keep_l, {"key": fld, "value": clipped_v, "from": lbl, "was": val}


def _resolve_key(fld: str, sp: Any, origin: dict, dflt: dict, own_d: dict) -> tuple[Any, dict | None]:
    val, origin_s = _lookup_value(fld, origin, dflt, own_d)
    if origin_s == "mid-range":
        centre_v = midrange(sp)
        outcome = (centre_v, {"key": fld, "value": centre_v, "from": "mid-range"})
    elif inside(sp, val):
        outcome = (val, {"key": fld, "value": val, "from": origin_s} if origin_s == "module DEFAULTS" else None)
    else:
        outcome = _clamped_note(fld, sp, val, origin_s)
    return outcome


def fill_start(space: dict, start: dict, defaults: dict, own: dict | None = None) -> tuple[dict, list[dict]]:
    """A complete start dict for space, with notes on where each odd value came from."""
    own = defaults if own is None else own
    complete, remarks = dict(start), []
    for fld, sp in space.items():
        complete[fld], remark = _resolve_key(fld, sp, start, defaults, own)
        if remark:
            remarks.append(remark)
    return complete, remarks


# ---------------------------------------------------------------- render

_SEEN_FAILS: set[str] = set()
_SEEN_GUARD = threading.Lock()


def _label_for(rcp: Any) -> str:
    return registry.short_name(rcp) or str(getattr(rcp, "__name__", "recipe"))


def _note_crash(rcp: Any, el: Exception) -> None:
    rname = _label_for(rcp)
    with _SEEN_GUARD:
        if rname in _SEEN_FAILS:
            return
        _SEEN_FAILS.add(rname)
    frame_tb = el.__traceback__
    while frame_tb is not None and frame_tb.tb_next is not None:
        frame_tb = frame_tb.tb_next
    loc = f"{frame_tb.tb_frame.f_code.co_filename}:{frame_tb.tb_lineno}" if frame_tb else "?"
    print(f"forge: recipe {rname} raised {type(el).__name__}: {el} ({loc}); later failures of this recipe are not "
          "printed", file=sys.stderr)


def render(recipe: Any, params: dict, seed: int) -> np.ndarray | None:
    """One take peak-normalised to -1 dBFS as float32, or None when it fails or is silent."""
    try:
        sig = np.asarray(recipe.render(dict(params), dsp.rng(int(seed))), dtype=np.float64)
    except Exception as el:  # noqa: BLE001 - a failing recipe scores -inf, it never stops a search
        _note_crash(recipe, el)
        return None
    if sig.size == 0 or not np.all(np.isfinite(sig)) or np.max(np.abs(sig)) < 1e-7:
        return None
    return dsp.normalize(sig, -1.0).astype(np.float32)


def render_many(recipe: Any, jobs: Sequence[tuple[dict, int]], workers: int | None = None) -> list:
    """Renders of [(params, seed), ...] in order; threads when useful."""
    cnt = workers_of(workers)
    if cnt > 1 and len(jobs) >= 2:
        with ThreadPoolExecutor(cnt) as executor:
            return list(executor.map(lambda jx: render(recipe, jx[0], jx[1]), jobs))
    return [render(recipe, prm, one) for prm, one in jobs]


# ---------------------------------------------------------------- descriptor

def _downmix(sig: Any) -> np.ndarray:
    return tone.as_mono(sig)


def _frame_power(mono: np.ndarray, cnt: int) -> tuple[np.ndarray, np.ndarray]:
    ky = len(mono) // cnt
    power = np.mean(mono[: ky * cnt].reshape(ky, cnt) ** 2, axis=1)
    return power, 10 * np.log10(power + 1e-18)


def _fall_ms(el: np.ndarray, peak_ix: int, peak: float, drop_db: float, step_ms: float) -> float:
    below = np.flatnonzero(el[peak_ix:] < peak - drop_db)
    return float((below[0] if len(below) else len(el) - peak_ix) * step_ms)


def _crest_db_of(mono: np.ndarray, el: np.ndarray, onset: int, peak: float, cnt: int) -> float:
    alive = np.repeat(el >= peak - 40, cnt)
    alive[: onset * cnt] = False
    segment = mono[: len(alive)][alive]
    if not len(segment):
        return 0.0
    rms_v = math.sqrt(float(np.mean(segment ** 2)))
    return float(20 * math.log10(np.max(np.abs(mono)) / rms_v)) if rms_v > 0 else 0.0


def _timing_fields(el: np.ndarray, peak_ix: int, onset: int, frame_ms: float) -> dict:
    peak = float(np.max(el))
    res = {"attack_ms": (peak_ix - onset) * frame_ms + frame_ms / 2}
    res.update({f"t{dd}_ms": _fall_ms(el, peak_ix, peak, dd, frame_ms) for dd in (10, 20, 40)})
    return res


def _energy_centre(power: np.ndarray, onset: int, frame_ms: float) -> float:
    frames_ix = np.arange(onset, len(power))
    wt = power[onset:]
    return float(np.sum((frames_ix - onset) * wt) / (np.sum(wt) + 1e-30)) * frame_ms + frame_ms / 2


def _tail_level(power: np.ndarray, peak: float) -> float:
    quarter = max(1, len(power) // 4)
    return max(-90.0, float(10 * math.log10(np.mean(power[-quarter:]) + 1e-18) - peak))


def envelope_stats(x: Any, hop_ms: float = 2.0) -> dict:
    """Attack, decay times, centroid time, crest and end level of the mono sum."""
    mono = _downmix(x)
    cnt = int(round(SR * hop_ms / 1000))
    if len(mono) // cnt < 2:
        return {**{ky: 0.0 for ky in ENV_KEYS}, "end_db": -90.0}
    power, el = _frame_power(mono, cnt)
    peak_ix, peak = int(np.argmax(el)), float(np.max(el))
    onset = int(np.argmax(el > peak - 30))
    res = _timing_fields(el, peak_ix, onset, hop_ms)
    res.update(centroid_ms=_energy_centre(power, onset, hop_ms), crest_db=_crest_db_of(mono, el, onset, peak, cnt), end_db=_tail_level(power, peak))
    return res


def _max_streak(mask: np.ndarray) -> int:
    top = streak = 0
    for fl in mask:
        streak = streak + 1 if fl else 0
        top = max(top, streak)
    return top


def _count_dropouts(mono: np.ndarray) -> int:
    zs = np.concatenate([[False], mono == 0, [False]]).astype(int)
    start_list, stops = np.flatnonzero(np.diff(zs) == 1), np.flatnonzero(np.diff(zs) == -1)
    return int(sum(1 for lhs, rhs in zip(start_list, stops) if lhs > 0 and rhs < len(mono) and rhs - lhs > 0.005 * SR))


def _hf_share_db(mono: np.ndarray) -> float:
    mat = np.abs(np.fft.rfft(mono)) ** 2
    fl = np.fft.rfftfreq(len(mono), 1 / SR)
    return float(10 * math.log10(mat[fl > 20000].sum() / (mat.sum() + 1e-30) + 1e-12))


def _clip_check(mono: np.ndarray, peak: float) -> list[tuple[str, Any, float]]:
    flats = (np.abs(mono[1:]) > 0.98 * peak) & (np.abs(np.diff(mono)) <= 1e-6 * peak)
    streak_len = _max_streak(flats)
    return [("flat_top", streak_len, 1.0)] if flats.sum() >= 4 and streak_len >= 4 else []


def _body_checks(mono: np.ndarray, peak: float) -> list[tuple[str, Any, float]]:
    res = _clip_check(mono, peak)
    rms_v = math.sqrt(float(np.mean(mono ** 2))) + 1e-30
    if abs(float(np.mean(mono))) / rms_v > 0.1:
        res.append(("dc", round(float(np.mean(mono)) / rms_v, 3), 0.5))
    if abs(mono[0]) / peak > 0.05:
        res.append(("start_jump", round(float(abs(mono[0]) / peak), 3), 0.5))
    return res


def _tail_checks(mono: np.ndarray, peak: float) -> list[tuple[str, Any, float]]:
    res: list[tuple[str, Any, float]] = []
    tail = mono[-int(0.005 * SR):]
    tail_db = 20 * math.log10(math.sqrt(float(np.mean(tail ** 2))) / peak + 1e-12)
    if tail_db > -45:
        res.append(("cut_end", round(tail_db, 1), 1.0))
    holes = _count_dropouts(mono)
    if holes:
        res.append(("gap", holes, 1.0))
    hf_db = _hf_share_db(mono)
    if hf_db > -25:
        res.append(("ultrasonic", round(hf_db, 1), 0.5))
    return res


def _all_checks(mono: np.ndarray, peak: float) -> list[tuple[str, Any, float]]:
    return _body_checks(mono, peak) + _tail_checks(mono, peak)


def artifacts(x: Any) -> dict:
    """Cheap artifact checks: {'flags': {...}, 'penalty'}."""
    mono = _downmix(x)
    if not len(mono):
        return {"flags": {"silent": 1}, "penalty": 9.0}
    found = _all_checks(mono, float(np.max(np.abs(mono))) + 1e-12)
    return {"flags": {ky: val for ky, val, _ in found}, "penalty": float(sum(prm for _, _, prm in found))}


def describe(x: Any) -> dict:
    """Model-free descriptor: tone profile, envelope stats and cheap artifacts."""
    return {"profile": tone.profile(x), "env": envelope_stats(x), "art": artifacts(x)}


def env_vector(env: dict) -> np.ndarray:
    """Envelope vector: log times, then crest and end level in 6 dB units."""
    val = [math.log(max(float(env.get(ky, 0.0) or 0.0), 0.5)) for ky in _TIME_FIELDS]
    return np.array(val + [float(env.get("crest_db", 0.0)) / 6, float(env.get("end_db", -90.0)) / 6])


def sound_dist(a: dict, b: dict) -> float:
    """Profile RMS difference + 4 x envelope vector RMS difference."""
    dprof = np.asarray(a["profile"]) - np.asarray(b["profile"])
    denv = env_vector(a["env"]) - env_vector(b["env"])
    return float(np.sqrt(np.mean(dprof ** 2)) + 4 * np.sqrt(np.mean(denv ** 2)))


# ---------------------------------------------------------------- pre-screen

def _ref_monos(ref_list: Any) -> list[np.ndarray]:
    return [_downmix(rw) for rw in ref_list or []]


def _loo_spread(mat: np.ndarray) -> float:
    if len(mat) < 3:
        return 4.0
    held = [float(np.sqrt(np.mean((mat[ix] - np.median(np.delete(mat, ix, 0), axis=0)) ** 2))) for ix in range(len(mat))]
    return max(float(np.median(held)), 0.5)


class Prescreen:
    """Ranks sounds against refs by measurement only."""

    def __init__(self, refs: Any = None, tone_target: float | None = None, env_w: float = 0.5,
                 env_keys: Sequence[str] = ("t40_ms", "crest_db", "end_db"), polish_ref: Any = None,
                 polish: bool = True, study: dict | None = None):
        self.refs = list(refs or [])
        self.arrays = _ref_monos(self.refs)
        self.env_weight, self.env_fields = env_w, tuple(env_keys)
        self.study = self._study_targets(study)
        self.polish_ref = polish_ref if polish_ref is not None else self._polish_checker(polish)
        self.descs = [describe(lhs) for lhs in self.arrays]
        self._fit_refs(tone_target)

    def _study_targets(self, stud: dict | None) -> dict | None:
        if not stud or len(self.arrays) >= 3:
            return None
        from hark.forge.engine import critic

        return critic.widen_targets(stud, [critic.study_measures(lhs) for lhs in self.arrays])

    def _polish_checker(self, use_polish: bool) -> Any:
        if not use_polish:
            return None
        try:
            from hark.forge.engine import polish as pol

            return pol.PolishRef(self.arrays)
        except Exception:  # noqa: BLE001 - the cheap artifact checks rank alone
            return None

    def _fit_refs(self, tgt_spread: float | None) -> None:
        if not self.descs:
            self.prof_centre = self.spread = self.env_centre = self.env_scale = None
            return
        mat = np.array([dd["profile"] for dd in self.descs])
        self.prof_centre = np.median(mat, axis=0)
        self.spread = float(tgt_spread) if tgt_spread else _loo_spread(mat)
        envs = np.array([env_vector(dd["env"]) for dd in self.descs])
        self.env_centre = np.median(envs, axis=0)
        quart = np.percentile(envs, 75, axis=0) - np.percentile(envs, 25, axis=0)
        self.env_scale = np.maximum(quart / 1.35, _MIN_SCALE)

    def _polish_penalty(self, sig: Any, rrow: dict, art_v: float) -> float:
        try:
            cand = self.polish_ref.check(sig)
        except Exception:  # noqa: BLE001 - keep the cheap penalty
            return art_v
        rrow["polish"] = {"art_pen": cand["art_pen"], "polish_pen": cand["polish_pen"], "hard": cand["hard"]}
        return cand["art_pen"] + 0.75 * cand["polish_pen"] + 1.0 * len(cand["hard"])

    def _study_penalty(self, sig: Any, rrow: dict) -> float:
        from hark.forge.engine import critic

        pens, miss, _ = critic.study_penalties(critic.study_measures(sig), self.study)
        rrow["study"] = miss
        return pens.get("tone", 0.0) + pens.get("env", 0.0) + 0.75 * pens.get("polish", 0.0)

    def _ref_penalty(self, dd: dict, rrow: dict) -> float:
        tdist = float(np.sqrt(np.mean((np.asarray(dd["profile"]) - self.prof_centre) ** 2)))
        rel = tdist / self.spread
        zs = (env_vector(dd["env"]) - self.env_centre) / self.env_scale
        z_named = {ky: round(float(zs[ENV_KEYS.index(ky)]), 2) for ky in ENV_KEYS}
        env_p = float(np.mean([max(0.0, abs(z_named[ky]) - 1.5) for ky in self.env_fields]))
        rrow.update(tone_db=round(tdist, 2), tone_ratio=round(rel, 3), env_z=z_named, env_pen=env_p)
        return rel + self.env_weight * env_p

    def row(self, x: Any, polish: bool = True) -> dict:
        """Pre-screen row of one sound; higher 'pre' is better."""
        dd = describe(x)
        rrow: dict = {"desc": dd}
        art_v = dd["art"]["penalty"]
        if polish and self.polish_ref is not None:
            art_v = self._polish_penalty(x, rrow, art_v)
        if self.study:
            art_v += self._study_penalty(x, rrow)
        rrow["art"] = dd["art"]
        rrow["art_pen"] = art_v
        ref_v = self._ref_penalty(dd, rrow) if self.prof_centre is not None else 0.0
        rrow["pre"] = -(ref_v + art_v)
        return rrow

    def score(self, xs: Sequence[Any], polish: bool = True) -> list[dict]:
        """A row per sound; a None sound ranks last."""
        return [self.row(sig, polish) if sig is not None else
                {"pre": -math.inf, "art": {"flags": {"silent": 1}, "penalty": 9.0}} for sig in xs]

    def nearest(self, desc: dict) -> int | None:
        """Index of the ref with the smallest sound distance, or None without refs."""
        if not self.descs:
            return None
        return int(np.argmin([sound_dist(desc, rw) for rw in self.descs]))


def prescreen_for(critic: Any) -> Prescreen:
    """A Prescreen from a critic's refs, tone target, polish ref and study targets."""
    return Prescreen(getattr(critic, "refs", None) or None, tone_target=getattr(critic, "tone_target", None),
                     polish_ref=getattr(critic, "polish_ref", None), study=getattr(critic, "study", None) or None)


# ---------------------------------------------------------------- separable CMA-ES

def _reflect_unit(sig: np.ndarray) -> np.ndarray:
    rw = np.mod(sig, 2.0)
    return np.where(rw > 1, 2 - rw, rw)


class SepCMA:
    """Separable CMA-ES in the unit cube (diagonal covariance)."""

    def __init__(self, dd: int, popsize: int, mu_share: float = 0.5, elite_f: float = 0.0):
        self.dim, self.popsize, self.elite_part = dd, popsize, elite_f
        self.n_par = max(1, int(round(popsize * mu_share)))
        wt = np.log(self.n_par + 0.5) - np.log(np.arange(1, self.n_par + 1))
        self.wts = wt / wt.sum()
        self.eff_n = 1.0 / np.sum(self.wts ** 2)
        self._learning_rates()
        self.top: list[np.ndarray] = []

    def _learning_rates(self) -> None:
        dd, mu_eff = self.dim, self.eff_n
        self.c_sig = (mu_eff + 2) / (dd + mu_eff + 5)
        self.damp = 1 + 2 * max(0.0, math.sqrt((mu_eff - 1) / (dd + 1)) - 1) + self.c_sig
        self.c_path = (4 + mu_eff / dd) / (dd + 4 + 2 * mu_eff / dd)
        sep_f = (dd + 2) / 3
        self.r_one = min(1.0, 2 * sep_f / ((dd + 1.3) ** 2 + mu_eff))
        self.r_mu = min(1 - self.r_one, 2 * sep_f * (mu_eff - 2 + 1 / mu_eff) / ((dd + 2) ** 2 + mu_eff))
        self.exp_norm = math.sqrt(dd) * (1 - 1 / (4 * dd) + 1 / (21 * dd * dd))

    def reset(self, m: np.ndarray, sigma: float) -> None:
        self.mean, self.scale = np.asarray(m, dtype=float).copy(), float(sigma)
        self.diag = np.ones(self.dim)
        self.p_sig = np.zeros(self.dim)
        self.p_cov = np.zeros(self.dim)
        self.step = 0

    def ask(self, n: int, R: np.random.Generator) -> np.ndarray:
        noise = R.standard_normal((n, self.dim))
        n_elite = int(round(n * self.elite_part)) if self.top else 0
        means = [self.top[ix % len(self.top)] for ix in range(n_elite)] + [self.mean] * (n - n_elite)
        return _reflect_unit(np.array(means) + self.scale * np.sqrt(self.diag) * noise)

    def tell(self, U: np.ndarray, totals: Sequence[float]) -> None:
        rank_ix = np.argsort(-np.asarray(totals, dtype=float), kind="stable")[: self.n_par]
        steps = (np.asarray(U)[rank_ix] - self.mean) / self.scale
        nrm = np.sqrt(np.sum(steps ** 2 / self.diag, axis=1))
        steps = steps * np.minimum(1.0, 2 * self.exp_norm / np.maximum(nrm, 1e-12))[:, None]
        wt = self.wts[: len(steps)] / self.wts[: len(steps)].sum()
        step_mean = wt @ steps
        self.mean = np.clip(self.mean + self.scale * step_mean, 0, 1)
        self.step += 1
        self._evolve_paths(step_mean, wt, steps)

    def _evolve_paths(self, step_mean: np.ndarray, wt: np.ndarray, steps: np.ndarray) -> None:
        c_s, c_c, mu_eff = self.c_sig, self.c_path, self.eff_n
        self.p_sig = (1 - c_s) * self.p_sig + math.sqrt(c_s * (2 - c_s) * mu_eff) * step_mean / np.sqrt(self.diag)
        ps_norm = float(np.linalg.norm(self.p_sig))
        h_sig = ps_norm / math.sqrt(1 - (1 - c_s) ** (2 * self.step)) < (1.4 + 2 / (self.dim + 1)) * self.exp_norm
        self.p_cov = (1 - c_c) * self.p_cov + h_sig * math.sqrt(c_c * (2 - c_c) * mu_eff) * step_mean
        self._evolve_diag(h_sig, wt, steps)
        self.scale = float(np.clip(self.scale * math.exp((c_s / self.damp) * (ps_norm / self.exp_norm - 1)), 1e-3, 0.6))

    def _evolve_diag(self, h_sig: bool, wt: np.ndarray, steps: np.ndarray) -> None:
        c_c = self.c_path
        self.diag = ((1 - self.r_one - self.r_mu) * self.diag + self.r_one * (self.p_cov ** 2 + (1 - h_sig) * c_c * (2 - c_c) * self.diag)
                  + self.r_mu * (wt @ (steps ** 2)))
        self.diag = np.clip(self.diag, 1e-4, 16)


# ---------------------------------------------------------------- optimiser

def _fill_text(flagged: list[dict]) -> str:
    parts = "; ".join(f"start {wt['start']} {wt['key']} = {wt['value']} ({wt['from']})" for wt in flagged[:8])
    text = f"forge: {len(flagged)} start value(s) not from the start or DEFAULTS: {parts}"
    return text + (" ..." if len(flagged) > 8 else "")


def _emit_fill(flagged: list[dict], logger: Callable | None) -> None:
    text = _fill_text(flagged)
    if logger:
        logger(text)
    if any(wt["from"] == "mid-range" for wt in flagged):
        warnings.warn(text, MidRangeFill, stacklevel=5)


def _complete_starts(space: dict, start_list: list[dict], rcp: Any, dflt: dict, logger: Callable | None) -> tuple:
    rec_dflt = recipe_defaults(rcp)
    complete, flagged = [], []
    for jx, one in enumerate(start_list):
        fl, remarks = fill_start(space, one, rec_dflt, own=dflt)
        complete.append(fl)
        flagged += [{"start": jx, **cnt} for cnt in remarks if cnt["from"] != "module DEFAULTS"]
    if flagged:
        _emit_fill(flagged, logger)
    return complete, flagged


class _Run:
    """State of one optimize() call."""

    def __init__(self, rcp: Any, critic: Any, cfg: dict):
        self.rcp, self.critic, self.cfg = rcp, critic, cfg
        self.dflt = dict(getattr(rcp, "DEFAULTS", {}) or {})
        self.space = cfg["space"] if cfg["space"] is not None else rcp.SPACE
        self.names = list(self.space)
        self.gen = np.random.default_rng(cfg["seed"])
        self._init_starts()
        self.n_gen = max(1, cfg["budget"] // cfg["pop"])
        self.n_screen = cfg["prescreen"] if cfg["prescreen"] > cfg["pop"] else 0
        self.pscr = cfg["pre"] if cfg["pre"] is not None else (prescreen_for(critic) if self.n_screen else None)
        self.log_rows: list[dict] = []
        self.top: list[dict] = []
        self.gen_rows: list[dict] = []
        self.clock = {"render_s": 0.0, "pre_s": 0.0, "score_s": 0.0, "rendered": 0, "scored": 0}
        self.started = time.time()
        self._init_cma()

    def _init_starts(self) -> None:
        start_list = [dict(self.dflt)] + [{**self.dflt, **ix} for ix in self.cfg["init"] or []]
        self.start_list, self.fill_notes = _complete_starts(self.space, start_list, self.rcp, self.dflt, self.cfg["log"])
        self.start_pts = [encode(self.space, one) for one in self.start_list]

    def _init_cma(self) -> None:
        self.opt = None
        self.n_jumps, self.flat_gens, self.peak, self.visited = 0, 0, -math.inf, []
        if self.cfg["strategy"] != "cma":
            return
        conf = {"sigma0": 0.15, "mu": 0.5, "elite": 0.0, **(self.cfg["cma"] or {})}
        self.scale0 = conf["sigma0"]
        self.opt = SepCMA(max(1, len(self.names)), self.cfg["pop"], conf["mu"], conf["elite"])
        self.opt.reset(self.start_pts[0], self.scale0)

    # -- candidates

    def _draw_seed(self) -> int:
        return int(self.gen.integers(0, 2 ** 31))

    def _es_batch(self, gi: int, want: int) -> tuple[list, list, list]:
        cfg = self.cfg
        step_sig = cfg["sigma0"] * (cfg["sigma1"] / cfg["sigma0"]) ** (gi / max(1, self.n_gen - 1))
        self.scale = step_sig
        pts, sds, anchor = [], [], []
        for jx in range(want):
            if gi == 0 and jx < len(self.start_pts) and jx < cfg["pop"]:
                pt, fix_ix = self.start_pts[jx].copy(), jx
            elif gi == 0:
                pt, fix_ix = self.gen.random(len(self.names)), None
            else:
                pt, fix_ix = self._es_mutant(), None
            pts.append(pt)
            anchor.append(fix_ix)
            sds.append(self._draw_seed())
        return pts, sds, anchor

    def _es_mutant(self) -> np.ndarray:
        if self.gen.random() < self.cfg["fresh"] or not self.top:
            return self.gen.random(len(self.names))
        ky = int(self.gen.integers(0, max(1, min(self.cfg["elite"], len(self.top)))))
        parent = self.top[min(len(self.top) - 1, ky)]["u"]
        return np.clip(parent + self.gen.normal(0, self.scale, len(self.names)), 0, 1)

    def _cma_batch(self, gi: int, want: int) -> tuple[list, list, list]:
        pts, anchor = [], []
        if gi == 0:
            for jx, pt in enumerate(self.start_pts[: self.cfg["pop"]]):
                pts.append(pt.copy())
                anchor.append(jx)
        remaining = want - len(pts)
        if remaining > 0:
            pts += list(self.opt.ask(remaining, self.gen))
            anchor += [None] * remaining
        self.scale = self.opt.scale
        return pts, [self._draw_seed() for _ in pts], anchor

    def _to_params(self, pt: np.ndarray, anchor: int | None) -> dict:
        prm = {**self.dflt, **decode(self.space, pt)}
        if anchor is not None:
            stat = self.start_list[anchor]
            for ky, val in stat.items():
                if ky not in self.space or inside(self.space[ky], val):
                    prm[ky] = val
        return prm

    # -- evaluation

    def _screen_batch(self, mat: list[dict], sds: list[int], anchor: list) -> tuple[list, list, list, list]:
        tic = time.time()
        wavs = render_many(self.rcp, list(zip(mat, sds)), self.cfg["workers"])
        self.clock["render_s"] += time.time() - tic
        self.clock["rendered"] += len(wavs)
        tic = time.time()
        tbl = self.pscr.score(wavs, polish=True)
        self.clock["pre_s"] += time.time() - tic
        rank_ix = sorted(range(len(mat)), key=lambda ix: (anchor[ix] is None, -tbl[ix]["pre"], ix))[: self.cfg["pop"]]
        return rank_ix, [wavs[ix] for ix in rank_ix], [tbl[ix] for ix in rank_ix], tbl

    def _judge(self, mat: list[dict], sds: list[int], first_x: list | None) -> list[dict | None]:
        n_reps = max(1, self.cfg["reps"])
        tasks = [(jx, ky, sds[jx] + 7919 * ky) for jx in range(len(mat)) for ky in range(n_reps)]
        pending = [(jx, ky, one) for jx, ky, one in tasks if first_x is None or ky > 0]
        takes = self._render_set(mat, pending, count=first_x is None)
        takes.update({(jx, 0): sig for jx, sig in enumerate(first_x or [])})
        self.first_takes = [takes.get((jx, 0)) for jx in range(len(mat))]
        return self._score_set(len(mat), n_reps, takes)

    def _render_set(self, mat: list[dict], pending: list[tuple], count: bool) -> dict:
        tic = time.time()
        wavs = render_many(self.rcp, [(mat[jx], one) for jx, _, one in pending], self.cfg["workers"])
        self.clock["render_s"] += time.time() - tic
        self.clock["rendered"] += len(wavs) if count else 0
        return {(jx, ky): sig for (jx, ky, _), sig in zip(pending, wavs)}

    def _score_set(self, cnt: int, n_reps: int, takes: dict) -> list[dict | None]:
        alive = [(key2, sig) for key2, sig in sorted(takes.items()) if sig is not None]
        tic = time.time()
        tbl = self.critic.score([sig for _, sig in alive]) if alive else []
        self.clock["score_s"] += time.time() - tic
        self.clock["scored"] += len(tbl)
        scored = {key2: rw for (key2, _), rw in zip(alive, tbl)}
        return [_join_reps(scored, jx, n_reps) for jx in range(cnt)]

    # -- generation

    def step_gen(self, g: int) -> None:
        want = self.n_screen or self.cfg["pop"]
        pts, sds, anchor = (self._cma_batch if self.opt else self._es_batch)(g, want)
        mat = [self._to_params(pt, fl) for pt, fl in zip(pts, anchor)]
        pts, sds, mat, first_x, screen_rows = self._maybe_screen(pts, sds, anchor, mat)
        tbl = self._judge(mat, sds, first_x)
        dlist = self._describe_all(screen_rows)
        prerows = screen_rows or [None] * len(mat)
        recs = [self._log_row(g, pts[ix], mat[ix], sds[ix], tbl[ix], dlist[ix], prerows[ix]) for ix in range(len(mat))]
        self._refresh_top()
        self._summarise_gen(g, recs, self.n_screen or len(mat))
        if self.opt:
            self._cma_feedback(g, pts, recs)

    def _maybe_screen(self, pts: list, sds: list, anchor: list, mat: list) -> tuple:
        """With a prescreen: the kept candidates in prescreen order, their renders and prescreen rows."""
        if not self.n_screen:
            return pts, sds, mat, None, None
        rank_ix, first_x, screen_rows, _ = self._screen_batch(mat, sds, anchor)
        pts, sds, mat = ([val[ix] for ix in rank_ix] for val in (pts, sds, mat))
        return pts, sds, mat, first_x, screen_rows

    def _refresh_top(self) -> None:
        self.top = sorted([hent for hent in self.log_rows if math.isfinite(hent["score"]["total"])],
                             key=lambda hent: -hent["score"]["total"])[: self.cfg["elite"]]

    def _describe_all(self, screen_rows: list | None) -> list:
        if screen_rows is not None:
            return [rw.get("desc") for rw in screen_rows]
        wavs = self.first_takes
        if self.pscr is not None:
            return [rw.get("desc") for rw in self.pscr.score(wavs, polish=False)]
        return [describe(sig) if sig is not None else None for sig in wavs]

    def _log_row(self, gi: int, pt: np.ndarray, prm: dict, sd_v: int, rrow: dict | None, dsc: Any,
                pre_row: dict | None) -> dict:
        hent = {"gen": gi, "u": np.asarray(pt), "params": prm, "seed": sd_v, "score": rrow or dict(_DEAD_SCORE)}
        if dsc is not None:
            hent["desc"] = dsc
        if pre_row is not None and self.pscr is not None and self.pscr.descs and "pre" in pre_row:
            hent["pre"] = {ky: pre_row.get(ky) for ky in ("pre", "tone_db", "tone_ratio", "env_pen")}
            hent["pre"]["art"] = pre_row.get("art", {}).get("flags")
            if "polish" in pre_row:
                hent["pre"]["polish"] = pre_row["polish"]
        self.log_rows.append(hent)
        return hent

    def _summarise_gen(self, gi: int, tbl: list[dict], n_rend: int) -> None:
        vals = [rw["score"]["total"] for rw in tbl if math.isfinite(rw["score"]["total"])]
        top = self.top[0]["score"] if self.top else {}
        stat = {"gen": gi + 1, "best": top.get("total"), "gen_best": max(vals) if vals else None,
              "gen_median": float(np.median(vals)) if vals else None,
              "passes": sum(1 for rw in tbl if rw["score"].get("passes")), "scored": len(tbl),
              "rendered": n_rend, "secs": round(time.time() - self.started, 2), "sigma": round(self.scale, 4)}
        if top.get("tone_db") is not None:
            stat["best_tone_db"] = top["tone_db"]
        self.gen_rows.append(stat)
        self._print_gen(stat, top)

    def _print_gen(self, stat: dict, top: dict) -> None:
        logger = self.cfg["log"]
        if not logger or not self.top:
            return
        msg_line = (f"gen {stat['gen']}/{self.n_gen}: best total {top['total']:.2f} "
                f"({'PASS' if top.get('passes') else 'no pass'}) z {top.get('match_z')} "
                f"p_real {top.get('p_real')} typ {top.get('ref_typ_z')} pen {top.get('feat_pen')} "
                f"sigma {stat['sigma']}")
        if stat.get("restart"):
            msg_line += f" restart {stat['restart']}"
        if self.n_screen:
            msg_line += f" [{self.n_screen} rendered, {self.cfg['pop']} scored]"
        logger(msg_line)

    def _cma_feedback(self, gi: int, pts: list, tbl: list[dict]) -> None:
        good = [(pt, rw["score"]["total"]) for pt, rw in zip(pts, tbl) if math.isfinite(rw["score"]["total"])]
        if good:
            self.opt.tell(np.array([pt for pt, _ in good]), [tic for _, tic in good])
        self._note_progress()
        stalled = self.opt.scale < self.cfg["sigma_stop"] or self.flat_gens >= self.cfg["patience"]
        if gi < self.n_gen - 1 and stalled:
            self._jump()

    def _note_progress(self) -> None:
        top = self.top[0]["score"]["total"] if self.top else -math.inf
        if top > self.peak + 1e-3:
            self.peak, self.flat_gens = top, 0
        else:
            self.flat_gens += 1

    def _jump(self) -> None:
        self.visited.append(self.opt.mean.copy())
        dd = max(1, len(self.names))
        top_h = sorted([hent for hent in self.log_rows if math.isfinite(hent["score"]["total"])],
                        key=lambda hent: -hent["score"]["total"])[:24]
        distant = [hent for hent in top_h if all(np.linalg.norm(hent["u"] - tic) > 0.2 * math.sqrt(dd) for tic in self.visited)]
        self.n_jumps += 1
        if self.n_jumps % 2 == 1 and distant:
            origin, form = distant[0]["u"], "elite"
        else:
            origin, form = self.gen.random(dd), "random"
        self.opt.reset(origin, self.scale0)
        self.flat_gens = 0
        self.gen_rows[-1]["restart"] = form

    def summary(self) -> dict:
        good = sorted([hent for hent in self.log_rows if math.isfinite(hent["score"]["total"])],
                     key=lambda hent: -hent["score"]["total"])
        tmg = dict(self.clock)
        elapsed = time.time() - self.started
        minutes = max(elapsed / 60, 1e-9)
        tmg.update(render_s=round(tmg["render_s"], 2), pre_s=round(tmg["pre_s"], 2), score_s=round(tmg["score_s"], 2),
                  total_s=round(elapsed, 2), scored_per_min=round(tmg["scored"] / minutes, 1),
                  rendered_per_min=round(tmg["rendered"] / minutes, 1))
        return {"best": good[: max(self.cfg["elite"], 8)], "history": self.log_rows, "generations": self.gen_rows,
                "gens": self.n_gen, "timing": tmg, "strategy": self.cfg["strategy"], "prescreen": self.n_screen,
                "restarts": self.n_jumps, "full_space": self.cfg["space"] is None,
                "fill_warnings": self.fill_notes}


def _join_reps(scored: dict, jx: int, n_reps: int) -> dict | None:
    rep_rows = [scored.get((jx, ky)) for ky in range(n_reps)]
    return None if any(rw is None for rw in rep_rows) else _avg_reps(rep_rows)


def _avg_reps(tbl: list[dict]) -> dict:
    if len(tbl) == 1:
        return tbl[0]
    sums = [rw["total"] for rw in tbl]
    res = dict(tbl[0])
    res["total"] = float(np.mean(sums)) if all(math.isfinite(tic) for tic in sums) else -math.inf
    res["total_reps"] = sums
    res["passes"] = sum(1 for rw in tbl if rw.get("passes")) >= len(tbl) / 2
    return res


OPTIMIZE_DEFAULTS: dict[str, Any] = dict(
    budget=160, pop=16, elite=4, seed=0, init=None, sigma0=0.22, sigma1=0.04, fresh=0.15, log=None, space=None,
    reps=1, strategy="es", prescreen=0, workers=None, patience=4, sigma_stop=0.03, pre=None, cma=None)


def optimize(recipe: Any, critic: Any, **opts: Any) -> dict:
    """Search a recipe's SPACE against a critic; returns best, history, generations and timing.

    Options and defaults are OPTIMIZE_DEFAULTS: budget, pop, elite, seed, init, sigma0, sigma1, fresh, log,
    space, reps, strategy ("es" or "cma"), prescreen, workers, patience, sigma_stop, pre and cma.
    """
    extra_keys = sorted(set(opts) - set(OPTIMIZE_DEFAULTS))
    if extra_keys:
        raise TypeError(f"optimize() got unexpected keyword argument(s): {', '.join(extra_keys)}")
    cfg = {**OPTIMIZE_DEFAULTS, **opts}
    one = _Run(recipe, critic, cfg)
    for gi in range(one.n_gen):
        one.step_gen(gi)
    return one.summary()


# ---------------------------------------------------------------- robust and diverse

def _total_stats(sums: list[float]) -> dict:
    if not sums:
        return {"mean": None, "min": None, "max": None, "sd": None}
    return {"mean": float(np.mean(sums)), "min": min(sums), "max": max(sums), "sd": float(np.std(sums))}


def _share_passing(tbl: list[dict], cnt: int) -> float:
    return round(sum(1 for rw in tbl if rw.get("passes")) / max(1, cnt), 3)


def _avg_tone(tbl: list[dict]) -> dict:
    tones = [rw["tone_db"] for rw in tbl if rw.get("tone_db") is not None]
    return {"tone_db_mean": float(np.mean(tones))} if tones else {}


def _seed_range(sds: Any, first_seed: int) -> list:
    return list(sds) if isinstance(sds, (list, tuple)) else list(range(first_seed, first_seed + int(sds)))


def robust(recipe: Any, critic: Any, params: dict, seeds: Any = 4, seed0: int = 5000,
           workers: int | None = None) -> dict:
    """Score a parameter set on several seeds: mean, min, max, sd and pass rate."""
    seed_l = _seed_range(seeds, seed0)
    wavs = render_many(recipe, [(params, one) for one in seed_l], workers)
    alive = [sig for sig in wavs if sig is not None]
    tbl = critic.score(alive) if alive else []
    sums = [float(rw["total"]) for rw in tbl if math.isfinite(rw["total"])]
    res = {"n": len(seed_l), "seeds": seed_l, "totals": [round(tic, 3) for tic in sums], "failed": len(wavs) - len(alive),
           **_total_stats(sums), "pass_rate": _share_passing(tbl, len(seed_l))}
    res.update(_avg_tone(tbl))
    return res

def _order_key(rw: dict) -> tuple:
    return (not rw["score"].get("passes"), -rw["score"]["total"])


def _is_dup(rw: dict, cand: dict, u_min: float, snd_min: float, by_sound: bool) -> bool:
    pt, val = np.asarray(rw["u"]), np.asarray(cand["u"])
    close_u = np.linalg.norm(pt - val) / math.sqrt(max(1, len(pt))) < u_min
    close_s = bool(by_sound and rw.get("desc") and cand.get("desc") and sound_dist(rw["desc"], cand["desc"]) < snd_min)
    return cand is rw or close_u or close_s


def _is_new(rw: dict, picked: list[dict], u_min: float, snd_min: float, by_sound: bool) -> bool:
    return not any(_is_dup(rw, cand, u_min, snd_min, by_sound) for cand in picked)


def diverse(results: Sequence[dict], recipe: Any, k: int = 4, min_dist: float = 0.08,
            min_sound: float = 2.0) -> list[dict]:
    """Up to k strong results with different parameters and sound."""
    good = [rw for rw in results if math.isfinite(rw["score"]["total"])]
    ranked_pool = sorted(good if good else list(results), key=_order_key)
    picked: list[dict] = []
    for by_sound in ([True, False] if min_sound else [False]):
        for rw in ranked_pool:
            if len(picked) >= k:
                break
            if _is_new(rw, picked, min_dist, min_sound, by_sound):
                picked.append(rw)
    return sorted(picked, key=_order_key)


# ---------------------------------------------------------------- seed bank

def seeds_path(recipe: Any, folder: Any = None) -> Path:
    """<folder>/<recipe>.json, else the registry's seed path of the recipe."""
    rname = recipe if isinstance(recipe, str) else _label_for(recipe)
    if folder is not None:
        return Path(folder) / f"{rname}.json"
    try:
        return registry.seed_path(rname)
    except KeyError:
        return SEEDS_DIR / f"{rname}.json"


def _ctx_label(tgt: str | None, ref_list: Any) -> str:
    base_names = sorted(os.path.basename(str(rw).replace("\\", "/")) for rw in ref_list or [])
    return f"{tgt or ''} | {','.join(base_names)}"


def _six_digits(val: Any) -> Any:
    if isinstance(val, np.integer):
        res = int(val)
    elif isinstance(val, np.floating) or (isinstance(val, float) and math.isfinite(val)):
        res = float(f"{float(val):.6g}")
    else:
        res = val
    return res


def _load_bank(fpath: Path, rname: str) -> dict:
    if fpath.is_file():
        try:
            return json.loads(fpath.read_text())
        except (OSError, ValueError):
            pass
    return {"recipe": rname, "entries": []}


def _rank_of(el: dict) -> float:
    rw = el.get("rank_score")
    return float(rw) if isinstance(rw, (int, float)) and math.isfinite(rw) else -1e9


def load_seeds(recipe: Any, target: str | None = None, refs: Any = None, n: int = 6, folder: Any = None) -> list:
    """Up to n start param dicts from the seed bank, same context and delivered first."""
    fpath = seeds_path(recipe, folder)
    if not fpath.is_file():
        return []
    ctx_key = _ctx_label(target, refs)
    rows_e = _load_bank(fpath, "").get("entries") or []
    rows_e = sorted(rows_e, key=lambda el: (el.get("context") != ctx_key, not el.get("delivered"), -_rank_of(el)))
    return [dict(el["params"]) for el in rows_e[:n]]


def _is_real(val: Any) -> bool:
    return isinstance(val, (int, float)) and math.isfinite(val)


def _member_name(prm: dict) -> Any:
    return next((prm[ky] for ky in ("family", "preset", "member", "name") if prm.get(ky) is not None), None)


def _extra_fields(prm: dict, rob: dict, pin_tag: str | None) -> dict:
    more = {}
    if rob:
        more.update(robust_mean=rob.get("mean"), robust_pass_rate=rob.get("pass_rate"))
    if pin_tag is not None:
        more.update(delivered=pin_tag, member=_member_name(prm), seed=prm.get("seed"))
    return more


def _bank_row(prm: dict, ctx_key: str, origin_txt: str, pin_tag: str | None) -> dict | None:
    scr = prm.get("score") or {}
    rob = prm.get("robust") or {}
    rank_v = rob.get("mean") if rob.get("mean") is not None else scr.get("total")
    is_fin = _is_real(rank_v)
    if not is_fin and pin_tag is None:
        return None
    el = {"params": {ky: _six_digits(val) for ky, val in prm["params"].items()}, "rank_score": round(rank_v, 3) if is_fin else None,
         "context": ctx_key, "total": scr.get("total"), "tone_db": scr.get("tone_db"), "passes": scr.get("passes"),
         "source": origin_txt}
    el.update(_extra_fields(prm, rob, pin_tag))
    return el


def _best_of_ctx(rows_e: list[dict], locked: list[dict], ctx_key: Any, per_ctx: int) -> list[dict]:
    """Up to per_context best undelivered entries of one context, without repeated params."""
    seen_p = [el["params"] for el in locked if el.get("context") == ctx_key]
    keep_l: list[dict] = []
    open_e = sorted([el for el in rows_e if el.get("context") == ctx_key and not el.get("delivered")], key=lambda el: -_rank_of(el))
    for el in open_e:
        if len(keep_l) < per_ctx and el["params"] not in seen_p:
            seen_p.append(el["params"])
            keep_l.append(el)
    return keep_l


def _prune(rows_e: list[dict], per_ctx: int, limit: int) -> list[dict]:
    locked = [el for el in rows_e if el.get("delivered")]
    keep_l: list[dict] = []
    for ctx_key in dict.fromkeys(el.get("context") for el in rows_e):
        keep_l += _best_of_ctx(rows_e, locked, ctx_key, per_ctx)
    return locked + keep_l[: max(0, limit - len(locked))]


def _release_tag(prior: list[dict], pin_tag: str | None) -> None:
    """Earlier entries delivered under the same tag lose the tag."""
    for el in prior:
        if pin_tag is not None and el.get("delivered") == pin_tag:
            el.pop("delivered")
            el["was_delivered"] = pin_tag


def _store_bank(fpath: Path, store: dict) -> None:
    fpath.parent.mkdir(parents=True, exist_ok=True)
    tmp_p = fpath.with_name(f"{fpath.name}.tmp{os.getpid()}")
    tmp_p.write_text(json.dumps(store, indent=1, default=str))
    os.replace(tmp_p, fpath)


def save_seeds(recipe: Any, picks: Sequence[dict], target: str | None = None, refs: Any = None, source: str = "",
               folder: Any = None, per_context: int = 6, cap: int = 48, delivered: Any = None) -> dict:
    """Add picks to the seed bank and write it atomically."""
    rname = recipe if isinstance(recipe, str) else _label_for(recipe)
    fpath = seeds_path(recipe, folder)
    prior = _load_bank(fpath, rname).get("entries") or []
    ctx_key = _ctx_label(target, refs)
    pin_tag = ctx_key if delivered is True else (str(delivered) if delivered else None)
    fresh = [el for el in (_bank_row(prm, ctx_key, source, pin_tag) for prm in picks) if el is not None]
    _release_tag(prior, pin_tag)
    store = {"recipe": rname, "entries": _prune(fresh + prior, per_context, cap)}
    _store_bank(fpath, store)
    return store


def report_members(report: dict) -> list[dict]:
    """A report's picks as save_seeds picks."""
    res = []
    for prm in report.get("picks") or []:
        mono = {ky: prm[ky] for ky in ("score", "robust", "seed", "family", "preset", "member", "name") if ky in prm}
        mono["params"] = {**(prm.get("family_params") or {}), **(prm.get("params") or {})}
        mono.setdefault("score", {})
        res.append(mono)
    return res


def pin_delivered(recipe: Any, report: Any, category: str, target: str | None = None, refs: Any = None,
                  folder: Any = None, source: str | None = None) -> dict:
    """Pin a delivered report's members in the seed bank under category."""
    fpath = None
    if not isinstance(report, dict):
        fpath = Path(report)
        report = json.loads((fpath / "report.json" if fpath.is_dir() else fpath).read_text())
    origin_s = source or f"delivered final ({category})" + (f" {fpath}" if fpath else "")
    return save_seeds(recipe, report_members(report), target=target or report.get("target"),
                      refs=refs if refs is not None else report.get("refs"), source=origin_s, folder=folder,
                      delivered=category)
