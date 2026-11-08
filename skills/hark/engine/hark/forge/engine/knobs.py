"""Knobs: aim a bank sound at target numbers by re-rendering its own parameters.

A knob never resamples or stretches audio. Length, apex, pitch, decay, onset,
brightness and width move the recipe's parameters, and a short secant or
bracket search lands the measured value.
"""

from __future__ import annotations

import math
import re
from collections.abc import Callable
from types import SimpleNamespace
from typing import Any

import numpy as np

SR = 48000
APEX_TOL = 0.005
APEX_KEEP = 0.005
PITCH_TOL_CENTS = 50
PARTIAL_SPAN_ST = 6.0
DUR_REL, DUR_ABS = 0.01, 0.005
MAX_FIX = 2
SCALE = (0.15, 6.0)
WHOOSH = ("whoosh_fast", "whoosh_big", "pro_whoosh_big", "swish", "swipe")
WHOOSH_MAX_S = 4.4
HEAD_MAX_DB = -20.0
FADE_MIN_S = 0.002
FIT_PITCH = {"hybrid": "pitch", "sines": "transpose", "modal": "size"}
FIT_LENGTH = {"spectral": "length", "sines": "stretch", "gesture": "length"}
FIT_PITCH_ST = 12
TONAL_LIMITS = {"decay": (2, 3000), "onset": (0.5, 300), "bright": (50, 16000), "width": (-1, 1)}
TONAL_TOL = {"decay": (0.1, 3.0), "onset": (0.15, 3.0), "bright": (0.08, 30.0), "width": (0.0, 0.05)}
TONAL_ORDER = ("decay", "onset", "bright", "width")
TONAL_FIX = 4
TONAL_UNITS = {"decay": "t20 ms", "onset": "attack ms", "bright": "centroid Hz", "width": "L/R correlation"}
_TONAL_MEASURE = {"decay": "t20_ms", "onset": "attack_ms", "bright": "centroid_hz", "width": "corr"}

LENGTH: dict[str, str] = {
    **{c: "length" for c in ("riser", "pro_riser", "downlifter", "sub_drop", "pro_sub_drop", "shimmer",
                             "pro_shimmer", "pro_zip", "click", "pro_click", "hover_tick", "pro_hover_tick", "tap",
                             "pro_tap", "pro_pop_in", "pop", "pro_pop", "notify", "pro_notify", "pro_typing_key",
                             "reverse_swell")},
    **{c: "dur" for c in ("glitch", "pro_glitch", "pro_typing_run")},
    "pro_logo_sting": "g_length",
}


def _p(param: str, rng: tuple | None = None, co: tuple = (), needs: tuple | None = None, measure: str | None = "start",
       mode: str | None = None) -> dict:
    return {"param": param, "range": rng, "co": co, "needs": needs, "measure": measure, "mode": mode}


PITCH: dict[str, dict] = {
    "pluck": _p("f0", (130, 2100)),
    "notify": _p("f0", (220, 3200)),
    "pro_notify": _p("f0", (600, 3200)),
    "click": _p("pitch", (1200, 9500), mode="partial"),
    "pro_click": _p("pitch", (1200, 9500), mode="partial"),
    "toggle": _p("pitch", (1600, 8000), mode="partial"),
    "hover_tick": _p("ping_f", (3000, 15000), ("tick_f",), ("ping", 0.1)),
    "pro_hover_tick": _p("ping_f", (3000, 15000), ("tick_f",), ("ping", 0.1)),
    "tap": _p("body_f", (70, 220), ("knock_f", "glass_f")),
    "pro_tap": _p("body_f", (70, 220), ("knock_f", "glass_f")),
    "pro_pop_in": _p("body_f", (70, 220), ("knock_f", "glass_f")),
    "pop": _p("f0", (400, 3500), ("f_hi", "f_lo", "f_blip")),
    "pro_pop": _p("f0", (280, 1400), ("f_hi", "f_lo", "f_blip", "n_f0", "n_f1")),
    "sub_drop": _p("f1", (28, 62), ("f0",), measure="end"),
    "pro_sub_drop": _p("f1", (28, 62), ("f0",), measure="end"),
    "pro_zip": _p("f1", (80, 12000), ("f0", "fpk"), measure="end"),
    "riser": _p("tf0", (60, 4000), needs=("tone_db", -40), measure=None),
    "pro_riser": _p("tf0", (60, 4000), needs=("tone_db", -40), measure=None),
    "downlifter": _p("tf0", (150, 2500), needs=("tone_db", -40), measure=None),
    "logo_sting": _p("root", (300, 2200)),
    "pro_logo_sting": _p("g_root", (50, 1800), ("root",)),
    "impact": _p("body_f1", (34, 75), ("body_f0", "sub_f"), measure=None),
    "pro_impact": _p("body_f1", (34, 75), ("body_f0", "sub_f"), measure=None),
    "pro_whoosh_big": _p("sub_f", (25, 120), needs=("sub_db", -30), measure="end"),
    "shimmer": _p("key", measure=None, mode="pc"),
    "pro_shimmer": _p("key", measure=None, mode="pc"),
    "ui_pop": _p("f0", (250, 1200)),
    "mallet": _p("f0", (150, 3200)),
    "knock": _p("f1", (250, 2600)),
    "drip": _p("f0", (250, 1600), measure=None),
    "cut_hit": _p("sub_f1", (36, 130), ("sub_f0",), measure=None),
}

UNTUNED: dict[str, str] = {
    "whoosh_fast": "a noise pass-by: its band moves with the motion, so it has no note",
    "whoosh_big": "a noise pass-by: its band moves with the motion, so it has no note",
    "swish": "a noise flick: its whistle follows the speed, so it has no note",
    "swipe": "friction noise has no note",
    "glitch": "a digital stutter has no note",
    "pro_glitch": "a digital stutter has no note",
    "typing_key": "keys are not tuned: takes and seeds vary each press",
    "pro_typing_key": "keys are not tuned: takes and seeds vary each press",
    "pro_typing_run": "a typed run has no note",
}


def _t(param: str, rng: tuple, co: tuple = (), lin: bool = False, inverse: bool = False,
       styles: tuple | None = None) -> dict:
    return {"param": param, "range": rng, "co": co, "lin": lin, "inverse": inverse, "styles": styles}


_WIDTH_SMALL = _t("width", (0.0, 1.5), lin=True, inverse=True)
TONAL: dict[str, dict[str, dict]] = {
    "decay": {"notify": _t("ring", (0.05, 4.0)), "pro_notify": _t("ring", (0.05, 4.0)),
              "pluck": _t("tau1", (0.03, 2.4)), "ui_pop": _t("tau", (0.006, 0.2)), "drip": _t("tau", (0.008, 0.25)),
              "knock": _t("decay", (0.002, 0.2)), "mallet": _t("decay", (0.02, 2.0), ("gate",)),
              "shutter": _t("b_decay", (0.002, 0.15), ("body_ms",))},
    "onset": {"notify": _t("soft", (0.0, 0.12), lin=True), "pro_notify": _t("soft", (0.0, 0.12), lin=True),
              "ui_pop": _t("onset", (0.002, 0.12)), "drip": _t("onset", (0.0005, 0.12)),
              "mallet": _t("onset", (0.0003, 0.12))},
    "bright": {"notify": _t("hard", (0.0, 1.0), lin=True, styles=("bell",)),
               "mallet": _t("hard", (0.0, 1.0), lin=True, styles=("note",)),
               "ui_pop": _t("h2", (0.0, 1.2), lin=True), "drip": _t("h2", (0.0, 1.0), lin=True),
               "shutter": _t("b_hz", (300, 3000))},
    "width": {"notify": _t("width", (0.0, 3.0), lin=True, inverse=True),
              "pro_notify": _t("width", (0.0, 3.0), lin=True, inverse=True),
              **{c: _WIDTH_SMALL for c in ("ui_pop", "drip", "knock", "mallet", "shutter", "pluck")}},
}


class KnobError(ValueError):
    """A knob the sound cannot take, or a target out of range."""


# ---------------------------------------------------------------- other units (function-level back-edges)

def _estimate_pitch(x: np.ndarray, near_hz: float | None, span_st: float | None = None) -> float | None:
    from hark.forge.bank.bank import estimate_pitch

    return estimate_pitch(x, near_hz=near_hz, span_st=span_st)


def _peak_s(x: np.ndarray) -> float:
    from hark.forge.bank.bank import times

    return float(times(x)["peak_s"])


def _kind_of(model: dict) -> str:
    from hark.fit.pipeline.auto import kind_of

    return kind_of(model)


def _tonal_measure(x: np.ndarray) -> dict:
    from hark.forge.recipes.tonal import measure

    return measure(x)


# ---------------------------------------------------------------- pitch text

_NOTE = re.compile(r"^([A-Ga-g])([#sb]{0,2})(-?\d+)$")
_PC = {"C": 0, "D": 2, "E": 4, "F": 5, "G": 7, "A": 9, "B": 11}
_NAMES = ("C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B")
_HZ = re.compile(r"^\s*(\d+(?:\.\d*)?|\.\d+)\s*(hz)?\s*$", re.IGNORECASE)


def midi(pitch: Any) -> int | None:
    """MIDI number of a note name, or None."""
    if not isinstance(pitch, str):
        return None
    m = _NOTE.match(pitch.strip())
    if not m:
        return None
    acc = m.group(2)
    return 12 * (int(m.group(3)) + 1) + _PC[m.group(1).upper()] + acc.count("#") + acc.count("s") - acc.count("b")


def _parse_hz(pitch: Any) -> float | None:
    n = midi(pitch)
    if n is not None:
        return 440.0 * 2 ** ((n - 69) / 12)
    m = _HZ.match(str(pitch))
    return float(m.group(1)) if m else None


def _raw_hz(pitch: Any) -> float:
    if isinstance(pitch, bool):
        raise KnobError("pitch must be a note name, a number of Hz or text like 440hz, not a bool")
    f = float(pitch) if isinstance(pitch, (int, float)) else _parse_hz(pitch)
    if f is None:
        raise KnobError(f"pitch {pitch!r} is not a note name (A4, C#5, Cs5, Eb3), a number of Hz, or text like 440hz")
    return f


def hz(pitch: Any) -> float:
    """Hz of a note name, a number, or text like 440hz (8 Hz to 20 kHz)."""
    f = _raw_hz(pitch)
    if not (math.isfinite(f) and 8 <= f <= 20000):
        raise KnobError(f"pitch {pitch!r} is {f:g} Hz: outside 8 Hz to 20 kHz")
    return f


def note(f: float) -> str:
    """Nearest note name, with cents when 5 or more off."""
    n = 69 + 12 * math.log2(f / 440)
    k = round(n)
    cents = round(100 * (n - k))
    name = f"{_NAMES[k % 12]}{k // 12 - 1}"
    return f"{name} {cents:+d}c" if abs(cents) >= 5 else name


def label(pitch: Any) -> str:
    """Canonical file-name label of a pitch: Cs5, or 445hz."""
    f = hz(pitch)
    n = 69 + 12 * math.log2(f / 440)
    k = round(n)
    if abs(n - k) < 0.05:
        return _sharp_label(k)
    return f"{round(f, 2):g}hz"


def _sharp_label(k: int) -> str:
    sharp = ("C", "Cs", "D", "Ds", "E", "F", "Fs", "G", "Gs", "A", "As", "B")
    return f"{sharp[k % 12]}{k // 12 - 1}"


# ---------------------------------------------------------------- specs

def _err(entry: dict, msg: str) -> KnobError:
    return KnobError(f"{entry.get('id', '?')}: {msg}")


def _is_fit(entry: dict) -> bool:
    return entry.get("type") == "fit"


def _recipe_defaults(entry: dict) -> dict:
    try:
        from hark.forge import recipes

        return dict(getattr(recipes.load(entry["recipe"]), "DEFAULTS", {}) or {})
    except Exception:  # noqa: BLE001 - an unknown recipe has no extra DEFAULTS
        return {}


def _fit_length_knob(entry: dict) -> str | None:
    try:
        return FIT_LENGTH.get(_kind_of(entry.get("model") or {}))
    except Exception:  # noqa: BLE001
        return None


def _dur_method(entry: dict) -> str:
    if _is_fit(entry):
        return "param" if _fit_length_knob(entry) else "trim"
    cat = entry.get("category")
    has_len = cat in LENGTH and LENGTH[cat] in (entry.get("params") or {})
    return "shape" if cat in WHOOSH else "param" if has_len else "trim"


def _fit_pitch_spec(entry: dict) -> dict:
    f = entry.get("pitch_hz")
    if not f:
        raise _err(entry, "this fit has no measured note")
    kind = _kind_of(entry.get("model") or {})
    if kind not in FIT_PITCH:
        raise _err(entry, f"a {kind} fit has no pitch knob")
    return {"param": FIT_PITCH[kind], "native_hz": f, "note": note(f), "fit": True,
            "range_hz": [round(f / 2, 1), round(f * 2, 1)]}


def _partial_spec(entry: dict, ps: dict, v: float) -> dict:
    lo, hi = ps["range"]
    spec = {"param": ps["param"], "mode": "partial", "param_hz": v}
    meta = entry.get("meta") or {}
    if "pitch_hz" not in meta:
        return {**spec, "native_hz": None, "note": None, "range_hz": [lo, hi], "heard": "measured when rendered"}
    heard = meta["pitch_hz"]
    if heard is None:
        raise _err(entry, "no clear tonal partial near its tink, so it has no note to tune")
    r = heard / v
    return {**spec, "native_hz": heard, "note": note(heard), "range_hz": [lo * r, hi * r]}


def _pitch_param(entry: dict) -> tuple[dict, dict]:
    """The PITCH row of the entry's category and its params; raises when it has no pitch parameter."""
    cat = entry.get("category")
    if cat in UNTUNED:
        raise _err(entry, UNTUNED[cat])
    ps = PITCH.get(cat)
    params = entry.get("params") or {}
    if ps is None or ps["param"] not in params:
        raise _err(entry, "its recipe has no pitch parameter")
    return ps, params


def _hz_spec(entry: dict, ps: dict, params: dict) -> dict:
    v = params[ps["param"]]
    if ps["needs"] and float(params.get(ps["needs"][0], -1e9)) < ps["needs"][1]:
        raise _err(entry, f"its tonal layer is off ({ps['needs'][0]} under {ps['needs'][1]}), it has no note to tune")
    return {"param": ps["param"], "native_hz": float(v), "note": note(float(v)), "range_hz": list(ps["range"])}


def _pitch_spec(entry: dict) -> dict:
    if _is_fit(entry):
        return _fit_pitch_spec(entry)
    ps, params = _pitch_param(entry)
    v = params[ps["param"]]
    if ps["mode"] == "pc":
        spec = {"param": ps["param"], "mode": "pc", "key": int(v), "note": _NAMES[int(v) % 12]}
    elif ps["mode"] == "partial":
        spec = _partial_spec(entry, ps, v)
    else:
        spec = _hz_spec(entry, ps, params)
    return spec


def _tonal_row(entry: dict, knob: str) -> dict:
    """The TONAL row of the entry's category for a knob; raises for a fit or a category without it."""
    if _is_fit(entry):
        raise _err(entry, f"the {knob} knob does not exist for a fitted recording; only pitch and length apply")
    cats = TONAL[knob]
    cat = entry.get("category")
    if cat not in cats:
        raise _err(entry, f"no {knob} knob for category {cat}; categories with it: {', '.join(sorted(cats))}")
    return cats[cat]


def _tonal_spec(entry: dict, knob: str) -> dict:
    spec = _tonal_row(entry, knob)
    base = {**_recipe_defaults(entry), **(entry.get("params") or {})}
    if spec["param"] not in base:
        raise _err(entry, f"its recipe has no {spec['param']} parameter for the {knob} knob")
    if spec["styles"] and base.get("style") not in spec["styles"]:
        raise _err(entry, f"the {knob} knob needs style {' or '.join(spec['styles'])}, not {base.get('style')}")
    return spec


def describe(entry: dict) -> dict:
    """What the knobs can do on one bank entry."""
    out: dict = {"dur": _dur_method(entry), "apex": entry.get("category") in WHOOSH}
    try:
        out["pitch"] = _pitch_spec(entry)
    except KnobError as e:
        out["pitch"], out["why"] = None, str(e).split(": ", 1)[-1]
    tonal = {}
    for knob in TONAL_ORDER:
        try:
            tonal[knob] = _tonal_spec(entry, knob)["param"]
        except KnobError:
            tonal[knob] = None
    out["tonal"] = tonal
    return out


def partial_hz(entry: dict, x: np.ndarray) -> float | None:
    """The heard partial of a click or toggle render, or None."""
    ps = PITCH.get(entry.get("category"))
    if not ps or ps["mode"] != "partial":
        return None
    return _estimate_pitch(x, float(entry["params"][ps["param"]]), PARTIAL_SPAN_ST)


def pitch_warning(edits: dict) -> dict | None:
    """{'code': 'pitch_off', 'msg'} when the pitch knob missed its note."""
    p = edits.get("pitch") or {}
    if p.get("ok") is not False:
        return None
    m = p.get("measured_hz")
    if m:
        msg = (f"pitch {p.get('note')}: measured {note(m)} ({m:.1f} Hz, {int(p.get('cents_off') or 0):+d} cents); "
               "the knob did not land the note (tolerance 50 cents)")
    else:
        msg = f"pitch {p.get('note')}: no clear partial near it in the render"
    return {"code": "pitch_off", "msg": msg}


# ---------------------------------------------------------------- validate

def _number(entry: dict, name: str, v: Any) -> float:
    if isinstance(v, bool) or not isinstance(v, (int, float)) or not math.isfinite(v):
        raise _err(entry, f"{name} must be a number")
    return float(v)


def _validate_tonal(entry: dict, tonal: dict) -> None:
    for knob, v in tonal.items():
        if v is None:
            continue
        v = _number(entry, knob, v)
        lo, hi = TONAL_LIMITS[knob]
        if not lo <= v <= hi:
            raise _err(entry, f"{knob} {v:g} {TONAL_UNITS[knob]} is outside {lo}..{hi}")
        _tonal_spec(entry, knob)


def _validate_dur(entry: dict, dur: Any, whoosh: bool) -> None:
    d = _number(entry, "dur", dur)
    if not 0.01 <= d <= 60:
        raise _err(entry, f"dur {d:g} s is outside 0.01..60 s")
    if whoosh and d > WHOOSH_MAX_S:
        raise _err(entry, f"dur {d:g} s is over {WHOOSH_MAX_S} s, the longest pass this engine renders")


def _apex_value(entry: dict, apex: Any, whoosh: bool) -> float:
    a = _number(entry, "apex", apex)
    if not whoosh:
        raise _err(entry, "apex works only on whooshes; aim a riser with dur and align its end")
    if not 0.01 <= a <= WHOOSH_MAX_S - 0.1:
        raise _err(entry, f"apex {a:g} s is outside 0.01..{WHOOSH_MAX_S - 0.1:g} s")
    return a


def _validate_time(entry: dict, dur: Any, apex: Any) -> None:
    whoosh = entry.get("category") in WHOOSH
    if dur is not None:
        _validate_dur(entry, dur, whoosh)
    a = _apex_value(entry, apex, whoosh) if apex is not None else None
    if a is not None and dur is not None and a >= float(dur) - 0.02:
        raise _err(entry, "apex must come at least 20 ms before the end (dur)")


def validate(entry: dict, dur: Any = None, apex: Any = None, pitch: Any = None, decay: Any = None, onset: Any = None,
             bright: Any = None, width: Any = None) -> None:
    """Raise KnobError for anything apply would refuse, without a render."""
    _validate_tonal(entry, {"decay": decay, "onset": onset, "bright": bright, "width": width})
    _validate_time(entry, dur, apex)
    if pitch is not None:
        _plan_pitch(entry, pitch)


# ---------------------------------------------------------------- pitch planning

def _plan_pc(spec: dict, pitch: Any) -> tuple[dict, dict, dict]:
    n = midi(pitch)
    n = n if n is not None else round(69 + 12 * math.log2(hz(pitch) / 440))
    info = {"param": spec["param"], "key": n % 12, "note": _NAMES[n % 12], "native_key": spec["key"],
            "mode": "pitch class only: the register stays"}
    return {spec["param"]: n % 12}, {}, info


def _plan_unmeasured(entry: dict, spec: dict, f: float) -> tuple[dict, dict, dict]:
    """A click or toggle whose heard partial is not measured yet: land it after a render."""
    lo, hi = spec["range_hz"]
    if not lo <= f <= hi:
        raise _err(entry, f"{note(f)} ({f:.1f} Hz) is outside {lo:g}-{hi:g} Hz")
    return {}, {}, {"target_hz": round(f, 3), "note": note(f), "native_hz": None, "param": spec["param"],
                    "mode": "partial"}


def _check_range(entry: dict, spec: dict, f: float) -> None:
    lo, hi = spec["range_hz"]
    if not lo <= f <= hi:
        raise _err(entry, f"{note(f)} ({f:.1f} Hz) is outside {lo:g}-{hi:g} Hz ({note(lo)} to {note(hi)}); "
                          f"the sound sits at {spec['note']}")


def _plan_fit(spec: dict, st: float, info: dict) -> tuple[dict, dict, dict]:
    v = 2 ** (-st / 12) if spec["param"] == "size" else st
    info["knobs"] = {spec["param"]: v}
    return {}, {spec["param"]: v}, info


def _plan_partial(entry: dict, spec: dict, ratio: tuple[float, float], info: dict) -> tuple[dict, dict, dict]:
    upd = {spec["param"]: entry["params"][spec["param"]] * ratio[0] / ratio[1]}
    info.update(mode="partial", params={k: round(v, 3) for k, v in upd.items()})
    return upd, {}, info


def _plan_hz(entry: dict, spec: dict, ratio: tuple[float, float], info: dict) -> tuple[dict, dict, dict]:
    f, native = ratio
    params = entry["params"]
    upd = {spec["param"]: f}
    for c in PITCH[entry["category"]]["co"]:
        v = params.get(c)
        if isinstance(v, (int, float)) and not isinstance(v, bool):
            upd[c] = v * f / native
    info["params"] = {k: round(v, 3) for k, v in upd.items()}
    return upd, {}, info


def _plan_shift(entry: dict, spec: dict, f: float) -> tuple[dict, dict, dict]:
    native = spec["native_hz"]
    st = 12 * math.log2(f / native)
    info = {"target_hz": round(f, 3), "note": note(f), "native_hz": native, "semitones": round(st, 2),
            "param": spec["param"]}
    if spec.get("fit"):
        res = _plan_fit(spec, st, info)
    elif spec.get("mode") == "partial":
        res = _plan_partial(entry, spec, (f, native), info)
    else:
        res = _plan_hz(entry, spec, (f, native), info)
    return res


def _plan_pitch(entry: dict, pitch: Any) -> tuple[dict, dict, dict]:
    """(param updates, fit-knob updates, info)."""
    spec = _pitch_spec(entry)
    if spec.get("mode") == "pc":
        return _plan_pc(spec, pitch)
    f = hz(pitch)
    if spec.get("mode") == "partial" and spec["native_hz"] is None:
        return _plan_unmeasured(entry, spec, f)
    _check_range(entry, spec, f)
    return _plan_shift(entry, spec, f)


def _measure_native(entry: dict, run: Callable, pe: dict, fk: dict, p0: float) -> float:
    heard = _estimate_pitch(run({**pe}, fk), p0, PARTIAL_SPAN_ST)
    if heard is None:
        raise _err(entry, "no clear tonal partial near its tink in the render")
    return heard


def _aim_partial(render: Callable, v: float, f: float, rng: tuple) -> tuple[tuple[float, float], int]:
    """Re-render until the heard partial is within half the tolerance of f; (best (value, cents), renders)."""
    lo, hi = rng
    best: tuple[float, float] | None = None
    n = 0
    for _ in range(MAX_FIX + 1):
        v = min(hi, max(lo, v))
        got = _estimate_pitch(render(v), f, PARTIAL_SPAN_ST)
        n += 1
        if got is None:
            best = best or (v, math.inf)
            break
        c = 1200 * math.log2(got / f)
        best = (v, c) if best is None or abs(c) < abs(best[1]) else best
        if abs(c) <= PITCH_TOL_CENTS / 2:
            break
        v *= f / got
    return best, n


def _land_partial(entry: dict, run: Callable, pe: dict, fk: dict, info: dict) -> dict:
    ps = PITCH[entry["category"]]
    k, f = ps["param"], info["target_hz"]
    p0 = float(entry["params"][k])
    v, renders = pe.get(k), 0
    if v is None:
        info["native_hz"] = _measure_native(entry, run, pe, fk, p0)
        v, renders = p0 * f / info["native_hz"], 1
    best, n = _aim_partial(lambda val: run({**pe, k: val}, fk), v, f, ps["range"])
    info.update(params={k: round(best[0], 3)}, renders=renders + n)
    return {k: best[0]}


def _measure_pitch(entry: dict, x: np.ndarray, info: dict) -> None:
    where = "start" if _is_fit(entry) else (PITCH.get(entry.get("category")) or {}).get("measure", "start")
    if where is None or info.get("target_hz") is None:
        info["ok"] = None
        return
    m = np.asarray(x, dtype=float)
    seg = m[len(m) // 2:] if where == "end" else m
    got = _estimate_pitch(seg, info["target_hz"], None)
    info["measured_hz"] = round(got, 2) if got else None
    if got:
        info["cents_off"] = round(1200 * math.log2(got / info["target_hz"]))
        info["ok"] = abs(info["cents_off"]) <= PITCH_TOL_CENTS
    else:
        info["ok"] = None


# ---------------------------------------------------------------- tonal aiming

def _coords(knob: str, spec: dict) -> tuple[Callable, Callable, Callable]:
    if knob == "width":
        def y(c: float) -> float:
            c = min(0.999, max(-0.99, c))
            return (1 - c) / (1 + c)
        return (lambda v: v * v), (lambda X: math.sqrt(max(X, 0.0))), y
    if spec["lin"]:
        return (lambda v: v), (lambda X: X), (lambda m: m)
    return (lambda v: math.log(max(v, 1e-9))), math.exp, (lambda m: math.log(max(m, 1e-9)))


def _tol_ok(knob: str, m: float | None, target: float) -> bool:
    rel, ab = TONAL_TOL[knob]
    return m is not None and abs(m - target) <= max(ab, rel * abs(target))


class _Aim:
    """Search state of one tonal knob."""

    def __init__(self, knob: str, spec: dict, target: float, render: Callable):
        self.knob, self.spec, self.target, self.render = knob, spec, target, render
        self.X, self.Xi, self.Y = _coords(knob, spec)
        self.pts: list[tuple[float, float | None]] = []
        self.sign = -1 if spec["inverse"] else 1

    def measure(self, v: float) -> float | None:
        m = _tonal_measure(self.render(v)).get(_TONAL_MEASURE[self.knob])
        self.pts.append((v, m))
        return m

    def best(self) -> tuple[float, float | None]:
        return min(self.pts, key=lambda p: math.inf if p[1] is None else abs(p[1] - self.target))

    def _sides(self, yt: float) -> tuple[list, list]:
        """Measured points (search coordinates) under and over the target."""
        pts = [(self.X(v), self.Y(m)) for v, m in self.pts if m is not None]
        return [p for p in pts if p[1] < yt], [p for p in pts if p[1] > yt]

    def _nearest_pair(self, yt: float) -> tuple | None:
        """The closest (under, over) pair of measured points around the target."""
        under, over = self._sides(yt)
        if not under or not over:
            return None
        return min(((u, o) for u in under for o in over), key=lambda uo: abs(uo[0][0] - uo[1][0]))

    def _bracket(self) -> float | None:
        yt = self.Y(self.target)
        pair = self._nearest_pair(yt)
        if pair is None:
            return None
        a, b = pair
        x = a[0] + (yt - a[1]) * (b[0] - a[0]) / (b[1] - a[1])
        lo, hi = sorted((a[0], b[0]))
        return self.Xi(min(hi - 0.15 * (hi - lo), max(lo + 0.15 * (hi - lo), x)))

    def _last_two(self) -> tuple | None:
        """(x0, x1, y0, y1) of the last 2 points when a secant through them is defined."""
        if self.knob == "width" or len(self.pts) < 2:
            return None
        (v0, m0), (v1, m1) = self.pts[-2], self.pts[-1]
        ok = not (m0 is None or m1 is None or v0 == v1 or m0 == m1)
        return (self.X(v0), self.X(v1), self.Y(m0), self.Y(m1)) if ok else None

    def _secant(self) -> float | None:
        got = self._last_two()
        slope = (got[3] - got[2]) / (got[1] - got[0]) if got else 0.0
        if slope * self.sign <= 0:
            return None
        x1, y1 = got[1], got[3]
        lo, hi = self.spec["range"]
        reach = 0.5 * (hi - lo) if self.spec["lin"] else math.log(4)
        x = x1 + (self.Y(self.target) - y1) / slope
        return self.Xi(min(x1 + reach, max(x1 - reach, x)))

    def _width_step(self) -> float:
        v1, m1 = self.pts[-1]
        if abs(v1) < 1e-9 or m1 is None:
            return 0.5
        return math.sqrt(self.Y(self.target) / max(self.Y(m1) / (v1 * v1), 1e-9))

    def _lin_step(self) -> float:
        v1, m1 = self.pts[-1]
        lo, hi = self.spec["range"]
        if self.knob == "onset" and m1 is not None:
            return v1 + 0.9 * (self.target - m1) / 1000
        d = 1 if m1 is None else math.copysign(1, self.target - m1) * self.sign
        return v1 + 0.25 * (hi - lo) * d

    def _log_step(self) -> float:
        v1, m1 = self.pts[-1]
        return v1 * 2 if not m1 else v1 * (self.target / m1) ** self.sign

    def _first_step(self) -> float:
        if self.knob == "width":
            return self._width_step()
        return self._lin_step() if self.spec["lin"] else self._log_step()

    def propose(self) -> float:
        for step in (self._bracket, self._secant):
            v = step()
            if v is not None:
                return v
        return self._first_step()

    def run(self, v0: float) -> dict:
        self.measure(v0)
        lo, hi = self.spec["range"]
        for _ in range(TONAL_FIX):
            if _tol_ok(self.knob, self.best()[1], self.target):
                break
            v = min(hi, max(lo, self.propose()))
            if any(abs(v - p) <= 1e-9 * max(1.0, abs(p)) for p, _ in self.pts):
                break
            self.measure(v)
        v, m = self.best()
        return {"target": self.target, "param": self.spec["param"], "value": round(v, 6), "renders": len(self.pts),
                "search_measured": round(m, 3) if m is not None else None}


def _aim_tonal(entry: dict, run: Callable, knob: str, target: float, pe: dict, fk: dict) -> tuple[dict, dict]:
    spec = _tonal_spec(entry, knob)
    k = spec["param"]
    base = {**_recipe_defaults(entry), **(entry.get("params") or {}), **pe}
    v0 = float(base[k])

    def updates(v: float) -> dict:
        u = {k: v}
        if v0 > 0:
            u.update({c: base[c] * v / v0 for c in spec["co"] if isinstance(base.get(c), (int, float))})
        return u

    info = _Aim(knob, spec, float(target), lambda v: run({**pe, **updates(v)}, fk)).run(v0)
    upd = updates(info["value"])
    if spec["co"]:
        info["co"] = {c: upd[c] for c in spec["co"] if c in upd}
    return upd, info


# ---------------------------------------------------------------- length

def _fade_len(n: int, keep_s: float | None) -> int:
    k = min(n, max(int(0.01 * SR), min(int(0.15 * SR), n // 4)))
    if keep_s is not None:
        k = min(n, max(int(FADE_MIN_S * SR), min(k, n - int(round(keep_s * SR)))))
    return k


def _cut_fade(x: np.ndarray, n: int, k: int) -> np.ndarray:
    y = x[:n].astype(float)
    g = 0.5 + 0.5 * np.cos(np.linspace(0, np.pi, k))
    g[-1] = 0.0
    y[n - k:] *= g[:, None] if y.ndim == 2 else g
    return y


def fit_length(x: np.ndarray, dur: float, keep_s: float | None = None) -> tuple[np.ndarray, str]:
    """Pad or cut a render to dur seconds; a cut ends with a half raised-cosine fade."""
    x = np.asarray(x)
    n = int(round(dur * SR))
    if len(x) == n:
        return x, "exact"
    if len(x) < n:
        pad = np.zeros((n - len(x),) + x.shape[1:])
        return np.concatenate([x, pad]), f"pad {(n - len(x)) / SR * 1000:.0f} ms"
    k = _fade_len(n, keep_s)
    note_ = f"trim {(len(x) - n) / SR * 1000:.0f} ms" + (f" (fade {k / SR * 1000:.0f} ms)" if keep_s is not None else "")
    return _cut_fade(x, n, k), note_


def _secant_done(L: float, Lp: float, s: float, sp: float, target: float, tol: float) -> bool:
    return abs(L - target) <= tol or abs(L - Lp) < 1e-4 or s == sp


def _secant_step(s: float, sp: float, L: float, Lp: float, target: float) -> float:
    return min(SCALE[1], max(SCALE[0], s + (target - L) * (s - sp) / (L - Lp)))


def _secant_len(make: Callable, measure: Callable, s0: float, L0: float, target: float,
                tol: float | None = None) -> tuple[Any, float, int]:
    tol = tol if tol is not None else max(DUR_ABS, DUR_REL * target)
    s = min(SCALE[1], max(SCALE[0], s0 * target / max(L0, 1e-6)))
    x = make(s)
    L = measure(x)
    best, renders = (abs(L - target), x, s), 1
    sp, Lp = s0, L0
    for _ in range(MAX_FIX):
        if _secant_done(L, Lp, s, sp, target, tol):
            break
        sp, Lp, s = s, L, _secant_step(s, sp, L, Lp, target)
        x = make(s)
        L = measure(x)
        renders += 1
        best = (abs(L - target), x, s) if abs(L - target) < best[0] else best
    return best[1], best[2], renders


def _len_s(x: np.ndarray) -> float:
    return len(x) / SR


def _param_length(entry: dict, run: Callable, dur: float, pe: dict, fk: dict) -> tuple[np.ndarray, dict]:
    if _dur_method(entry) == "trim":
        return run(pe, fk), {"target": dur, "method": "trim", "renders": 1}
    meta = entry.get("meta") or {}
    extra = 0
    L0 = meta.get("dur_s")
    if L0 is None:
        L0, extra = _len_s(run(pe, fk)), 1
    if _is_fit(entry):
        knob = _fit_length_knob(entry)
        x, s, n = _secant_len(lambda s: run(pe, {**fk, knob: s}), _len_s, 1.0, L0, dur)
        return x, {"target": dur, "method": "param", "knob": knob, "value": round(s, 4), "renders": n + extra}
    k = LENGTH[entry["category"]]
    v0 = float({**entry["params"], **pe}[k])
    x, s, n = _secant_len(lambda s: run({**pe, k: v0 * s}, fk), _len_s, 1.0, L0, dur)
    return x, {"target": dur, "method": "param", "param": k, "value": round(v0 * s, 5), "renders": n + extra}


# ---------------------------------------------------------------- whoosh path

def _head_trim(x: np.ndarray, n: int) -> tuple[np.ndarray, str]:
    """Cut n samples off the head with a 3 ms fade in, unless that would cut into the pass."""
    mono = np.abs(x.mean(axis=1) if x.ndim == 2 else x)
    lvl = 20 * math.log10(float(mono[:n].max()) / (float(mono.max()) + 1e-30) + 1e-30)
    if lvl > HEAD_MAX_DB:
        return x, f"head kept: a {n / SR * 1000:.0f} ms trim would remove part of the pass ({lvl:.0f} dB re peak)"
    y = x[n:].astype(float)
    k = min(len(y), int(0.003 * SR))
    g = 0.5 - 0.5 * np.cos(np.linspace(0, np.pi, k))
    y[:k] *= g[:, None] if y.ndim == 2 else g
    return y, f"head trim {n / SR * 1000:.1f} ms"


def place_apex(x: np.ndarray, pk: float, apex: float) -> tuple[np.ndarray, str]:
    """Shift a render so its peak lands on apex (whole 1 ms hops)."""
    x = np.asarray(x)
    n = int(round((pk - apex) * SR / 48)) * 48
    if n == 0:
        res = (x, "exact")
    elif n < 0:
        res = (np.concatenate([np.zeros((-n,) + x.shape[1:]), x]), f"head pad {-n / SR * 1000:.1f} ms")
    else:
        res = _head_trim(x, n)
    return res


def _whoosh_base(entry: dict, pe: dict) -> dict:
    return {**_recipe_defaults(entry), **entry["params"], **pe}


def _whoosh_dur(entry: dict, run: Callable, dur: float, pe: dict, L0: float) -> tuple[np.ndarray, dict]:
    b = _whoosh_base(entry, pe)

    def make(s: float) -> np.ndarray:
        return run({**pe, "rise": b["rise"] * s, "hold": b.get("hold", 0) * s, "fall": b["fall"] * s})

    x, s, n = _secant_len(make, _len_s, 1.0, L0, dur)
    return x, {"target": dur, "method": "shape", "scale": round(s, 4), "renders": n}


def _apex_plan(b: dict, apex: float, dur: float | None, L0: float, pk0: float) -> SimpleNamespace:
    """Targets and first guesses of an apex search: rise r1 and post-peak scale q0."""
    k = (max(-b["start_db"], 1) / 30) ** (1 / b["rise_k"])
    post_t = dur - apex if dur is not None else None
    q0 = 1.0 if dur is None else min(SCALE[1], max(SCALE[0], post_t / max(L0 - pk0, 1e-6)))
    return SimpleNamespace(apex=apex, dur=dur, post_t=post_t, r1=apex / k, q0=q0)


def _apex_err(a: SimpleNamespace, pk: float, L: float) -> float:
    return abs(pk - a.apex) + (abs((L - pk) - a.post_t) if a.dur is not None else 0.0)


def _apex_done(a: SimpleNamespace, pk: float, L: float) -> bool:
    return abs(pk - a.apex) <= APEX_TOL and (a.dur is None or abs(L - a.dur) <= max(DUR_ABS, DUR_REL * a.dur))


def _apex_next(a: SimpleNamespace, rise: float, q: float, pk: float, L: float) -> tuple[float, float]:
    rise = min(4 * a.r1, max(0.25 * a.r1, rise * a.apex / max(pk, 1e-3)))
    if a.dur is not None:
        q = min(SCALE[1], max(SCALE[0], q * a.post_t / max(L - pk, 1e-3)))
    return rise, q


def _apex_search(render: Callable, a: SimpleNamespace) -> tuple[tuple, int]:
    """Best (err, x, peak, rise, q) over the search, and the number of renders."""
    rise, q, best = a.r1, a.q0, None
    for i in range(MAX_FIX + 1):
        x = render(rise, q)
        pk, L = _peak_s(x), _len_s(x)
        err = _apex_err(a, pk, L)
        best = (err, x, pk, rise, q) if best is None or err < best[0] else best
        if _apex_done(a, pk, L):
            break
        rise, q = _apex_next(a, rise, q, pk, L)
    return best, i + 1


def _whoosh_apex(entry: dict, run: Callable, apex: float, dur: float | None, pe: dict, L0: float,
                 pk0: float) -> tuple[np.ndarray, dict, dict | None]:
    b = _whoosh_base(entry, pe)
    a = _apex_plan(b, apex, dur, L0, pk0)
    best, n = _apex_search(lambda rise, q: run({**pe, "rise": rise, "hold": b.get("hold", 0) * q,
                                                "fall": b["fall"] * q}), a)
    _, x, pk, rise, q = best
    x, place = place_apex(x, pk, apex)
    info = {"target": apex, "search_err_ms": round((pk - apex) * 1000, 1), "rise": round(rise, 5),
            "renders": n, "fit": place}
    dinfo = {"target": dur, "method": "shape", "post_scale": round(q, 4), "renders": n} if dur else None
    return x, info, dinfo


def _whoosh(entry: dict, run: Callable, dur: float | None, apex: float | None, pe: dict,
            edits: dict) -> np.ndarray:
    meta = entry.get("meta") or {}
    L0, pk0 = meta.get("dur_s"), meta.get("peak_s")
    if L0 is None or pk0 is None:
        x0 = run(pe)
        L0, pk0 = _len_s(x0), _peak_s(x0)
    if apex is None:
        x, edits["dur"] = _whoosh_dur(entry, run, dur, pe, L0)
        return x
    x, edits["apex"], dinfo = _whoosh_apex(entry, run, apex, dur, pe, L0, pk0)
    if dinfo:
        edits["dur"] = dinfo
    return x


def _set_apex_measure(x: np.ndarray, apex: float, edits: dict) -> None:
    pk = _peak_s(x)
    edits["apex"].update(measured=round(pk, 4), err_ms=round((pk - apex) * 1000, 1), ok=abs(pk - apex) <= APEX_TOL)


# ---------------------------------------------------------------- apply

def _render(entry: dict, run: Callable, dur: Any, apex: Any, pe: dict, fk: dict, edits: dict) -> np.ndarray:
    if apex is not None or (dur is not None and entry.get("category") in WHOOSH):
        return _whoosh(entry, run, dur, apex, pe, edits)
    if dur is not None:
        x, edits["dur"] = _param_length(entry, run, float(dur), pe, fk)
        return x
    return run(pe, fk)


def _apply_pitch(entry: dict, run: Callable, pitch: Any, pe: dict, fk: dict, edits: dict) -> None:
    upd, fku, info = _plan_pitch(entry, pitch)
    pe.update(upd)
    fk.update(fku)
    edits["pitch"] = info
    if info.get("mode") == "partial":
        pe.update(_land_partial(entry, run, pe, fk, info))


def _finish_length(x: np.ndarray, dur: Any, apex: Any, edits: dict) -> np.ndarray:
    if dur is not None:
        x, fit = fit_length(x, float(dur), keep_s=apex + APEX_KEEP if apex is not None else None)
        edits.setdefault("dur", {"target": dur})["fit"] = fit
    if apex is not None:
        _set_apex_measure(x, float(apex), edits)
    return x


def _tonal_results(x: np.ndarray, tonal: dict, edits: dict) -> None:
    m = _tonal_measure(x)
    for knob, target in tonal.items():
        got = m.get(_TONAL_MEASURE[knob])
        edits[knob]["measured"] = got
        edits[knob]["ok"] = _tol_ok(knob, got, float(target))


def _aim_all(entry: dict, run: Callable, tonal: dict, pe: dict, fk: dict, edits: dict) -> None:
    for knob, target in tonal.items():
        upd, edits[knob] = _aim_tonal(entry, run, knob, target, pe, fk)
        pe.update(upd)


def _apply(entry: dict, run: Callable, dur: Any, apex: Any, pitch: Any, tonal: dict) -> tuple[np.ndarray, dict]:
    pe: dict = {}
    fk: dict = {}
    edits: dict = {}
    if pitch is not None:
        _apply_pitch(entry, run, pitch, pe, fk, edits)
    _aim_all(entry, run, tonal, pe, fk, edits)
    x = _render(entry, run, dur, apex, pe, fk, edits)
    x = _finish_length(x, dur, apex, edits)
    if "pitch" in edits:
        _measure_pitch(entry, x, edits["pitch"])
    if tonal:
        _tonal_results(x, tonal, edits)
    return x, edits


def apply(entry: dict, run: Callable, dur: Any = None, apex: Any = None, pitch: Any = None, decay: Any = None,
          onset: Any = None, bright: Any = None, width: Any = None) -> tuple[np.ndarray, dict]:
    """Re-render an entry aimed at the given targets; returns (raw render, edits)."""
    validate(entry, dur, apex, pitch, decay, onset, bright, width)
    tonal = {k: v for k, v in zip(TONAL_ORDER, (decay, onset, bright, width)) if v is not None}
    return _apply(entry, run, dur, apex, pitch, tonal)
