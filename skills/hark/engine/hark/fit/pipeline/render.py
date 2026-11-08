"""The fit engine, render side: knobs, render, takes, finish, gate time map, morph, fit pair, summary."""

from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Callable, Iterator

import numpy as np

from hark.fit.pipeline import _model, _post
from hark.fit.pipeline._model import channels_of, kind_of, load, save, strip
from hark.fit.pipeline._post import (
    FADE_IN_S, FADE_MIN_S, PEAK_DB, TAKE_DC_HZ, apply_post, bed_noise, finish,
)

SR = 48000
GATE_MAP_S = 0.002
GATE_SMOOTH_S = _post.GATE_SMOOTH_S
EXTRA = ("stereo", "gain", "gain_db", "ring")
MORPH_RANGE = (0.2, 0.8)

HYBRID_KNOBS: dict[str, tuple] = {
    "pitch": ((-3.0, 3.0), 0.0, "semitones on every layer"),
    "decay": ((0.7, 1.43, "log"), 1.0, "decay of the tone and body layers"),
    "bright": ((-3.0, 3.0), 0.0, "dB per octave on every layer"),
    "vary": ((0.0, 0.6), 0.0, "seeded jitter of the tone and body layers"),
    "width": ((0.5, 1.5), 1.0, "factor on every layer's side"),
    "tone_db": ((-6.0, 6.0), 0.0, "level of the sines layer"),
    "body_db": ((-6.0, 6.0), 0.0, "level of the modal layer"),
    "air_db": ((-6.0, 6.0), 0.0, "level of the spectral layer"),
}

__all__ = [
    "render", "KNOBS", "knobs_of", "identity", "finished", "morph", "morph_model", "fit_pair", "summary",
    "save", "load", "kind_of", "channels_of", "finish", "apply_post", "bed_noise", "gate_map",
    "FADE_IN_S", "FADE_MIN_S", "PEAK_DB", "TAKE_DC_HZ", "GATE_MAP_S", "GATE_SMOOTH_S",
]


class _KnobTables(Mapping):
    """Kind name -> knob table. The 4 analysis kinds come from their modules (read on first use)."""

    def __getitem__(self, name: str) -> dict:
        if name == "hybrid":
            return HYBRID_KNOBS
        if name not in _model.VALUES:
            raise KeyError(name)
        return dict(_model.module(name).KNOBS)

    def __iter__(self) -> Iterator[str]:
        return iter(("modal", "sines", "spectral", "gesture", "hybrid"))

    def __len__(self) -> int:
        return 5


KNOBS: Mapping[str, dict] = _KnobTables()


# ---------------------------------------------------------------- knobs


def _roles(model: dict) -> set:
    return {layer.get("role") for layer in model.get("layers", [])}


def knobs_of(model: dict) -> dict:
    """The knob table of a model: name -> (search range, identity, description)."""
    name = kind_of(model)
    if name == "morph":
        table = dict(knobs_of(model["a"]))
        table["morph"] = (MORPH_RANGE, float(model.get("amount", 0.5)), "position between the 2 recordings")
        return table
    table = dict(KNOBS[name])
    if name == "hybrid":
        roles = _roles(model)
        return {k: v for k, v in table.items() if not k.endswith("_db") or k[:-3] in roles}
    if name == "gesture" and not model.get("tones"):
        return {k: v for k, v in table.items() if k not in ("tone_db", "vary")}
    return table


def identity(model: dict) -> dict:
    """The knob values at which the render reproduces the fit."""
    return {k: v[1] for k, v in knobs_of(model).items()}


def _check_knobs(model: dict, knobs: dict) -> None:
    known = list(knobs_of(model)) + list(EXTRA)
    bad = [k for k in knobs if k not in known]
    if bad:
        raise ValueError(f"unknown fit knob(s) {bad}; known knobs: {known}")


def finished(model: dict) -> bool:
    """True when renders of this model get the finish step."""
    name = kind_of(model)
    if name in ("modal", "sines"):
        return bool(model.get("coherent"))
    if name == "hybrid":
        return any(bool(layer["model"].get("coherent")) for layer in model.get("layers", []))
    if name == "spectral":
        return bool(model.get("finish"))
    return True


# ---------------------------------------------------------------- raw renders


def _rename(knobs: dict, old: str, new: str) -> dict:
    k = dict(knobs)
    if old in k:
        v = k.pop(old)
        k.setdefault(new, v)
    return k


def _raw_modal(model: dict, seed: int, ring: bool, knobs: dict) -> np.ndarray:
    return _model.module("modal").render(model, seed=seed, ring=ring, **_rename(knobs, "gain_db", "gain"))


def _raw_sines(model: dict, seed: int, ring: bool, knobs: dict) -> np.ndarray:
    k = _rename(knobs, "gain", "gain_db")
    k.setdefault("stereo", channels_of(model) == 2)
    return _model.module("sines").render(model, seed=seed, ring=ring, **k)


def _raw_gesture(model: dict, seed: int, ring: bool, knobs: dict) -> np.ndarray:
    return _model.module("gesture").render(model, seed=seed, ring=ring, **_rename(knobs, "gain", "gain_db"))


def _raw_spectral(model: dict, seed: int, ring: bool, knobs: dict) -> np.ndarray:
    k = _rename(knobs, "gain", "gain_db")
    k.pop("stereo", None)
    y = np.asarray(_model.module("spectral").render(model, seed=seed, limit=False, **k), dtype=np.float64)
    return y.mean(axis=1) if channels_of(model) == 1 and y.ndim == 2 else y


def _layer_render(layer: dict, i: int, seed: int, two: bool, ring: bool, kn: dict) -> np.ndarray:
    m = layer["model"]
    s = (seed * 7 + 101 * (i + 1)) & 0x7FFFFFFF
    name = kind_of(m)
    pitch, decay, bright = kn.get("pitch", 0.0), kn.get("decay", 1.0), kn.get("bright", 0.0)
    vary, width = kn.get("vary", 0.0), kn.get("width", 1.0)
    if name == "sines":
        y = _model.module("sines").render(m, seed=s, transpose=pitch, decay=decay, brightness=bright, vary=vary,
                                          ring=ring, stereo=bool(two and m.get("side")), width=width)
    elif name == "modal":
        size = 2.0 ** (-pitch / 12)
        y = _model.module("modal").render(m, seed=s, size=size, damping=size / max(decay, 0.001),
                                          brightness=bright, vary=vary, width=width, ring=ring,
                                          stereo=bool(two and m.get("side") is not None))
    else:
        y = _model.module("spectral").render(m, seed=s, shift=pitch / 12, tilt=bright, width=width,
                                             limit=False, steady=False)
    gain = float(layer.get("gain_db", 0.0)) + float(kn.get(f"{layer.get('role')}_db", 0.0))
    return _model.as_two(y, 2 if two else 1) * 10 ** (gain / 20)


def _raw_hybrid(model: dict, seed: int, ring: bool, knobs: dict) -> np.ndarray:
    kn = dict(knobs)
    gain = kn.pop("gain_db", kn.pop("gain", 0.0))
    stereo = kn.pop("stereo", None)
    two = (channels_of(model) == 2) if stereo is None else bool(stereo)
    parts = [_layer_render(layer, i, seed, two, ring, kn) for i, layer in enumerate(model["layers"])]
    n = max(len(p) for p in parts)
    y = np.zeros((n, parts[0].shape[1]))
    for p in parts:
        y[: len(p)] += p
    y = y * 10 ** (float(gain) / 20)
    return y if two else y[:, 0]


_RAW: dict[str, Callable] = {
    "modal": _raw_modal, "sines": _raw_sines, "gesture": _raw_gesture,
    "spectral": _raw_spectral, "hybrid": _raw_hybrid,
}


# ---------------------------------------------------------------- time scale and gate map


def _time_scale(model: dict, name: str, kn: dict) -> tuple[float, float, float]:
    scale = tail = 1.0
    anchor = 0.0
    if name == "spectral":
        scale = kn.get("length", 1.0) / max(kn.get("speed", 1.0), 0.001)
    elif name == "gesture":
        scale = kn.get("length", 1.0)
        anchor = float((model.get("tones") or {}).get("onset", 0.0) or 0.0)
    if name == "modal":
        tail = kn.get("size", 1.0) / kn.get("damping", 1.0)
    elif name == "sines":
        tail = kn.get("stretch", 1.0) * kn.get("decay", 1.0)
    elif name == "hybrid":
        tail = kn.get("decay", 1.0)
    return float(scale), float(tail), anchor


def _curve(pts: list) -> Callable[[float], float]:
    p = np.asarray(pts, dtype=np.float64).reshape(-1, 2)
    return lambda t: float(np.interp(t, p[:, 0], p[:, 1], left=0.0, right=p[-1, 1]))


def _increasing(pts: list) -> list:
    out: list = []
    for t, d in pts:
        if not out or t > out[-1][0]:
            out.append([t, d])
    return out


def _new_strikes(ts: list, spacing: float) -> list:
    tn = [ts[0]]
    for i in range(1, len(ts)):
        tn.append(tn[-1] + max(0.003, (ts[i] - ts[i - 1]) * spacing))
    return tn


def _owner(t: float, ts: list) -> int:
    i = int(np.searchsorted(ts, t, side="right")) - 1
    return max(i, 0)


def modal_gate_map(pts: list, ts: list, spacing: float) -> list:
    """Move gate breakpoints with the strikes when the spacing knob changes their gaps."""
    tn = _new_strikes(ts, spacing)
    g = _curve(pts)
    out: list = []
    last = len(ts) - 1
    for i in range(len(ts)):
        mine = [p for p in pts if _owner(p[0], ts) == i]
        d = tn[i] - ts[i]
        if i == last:
            out += [[t + d, v] for t, v in mine]
            continue
        s, ns = max(ts[i + 1] - GATE_MAP_S, ts[i]), max(tn[i + 1] - GATE_MAP_S, tn[i])
        out += [[t + d, v] for t, v in mine if t < s and t + d < ns]
        e = min(s + d, ns)
        out += [[e - 1e-6, g(e - d)], [ns, g(s)]]
        dn = tn[i + 1] - ts[i + 1]
        out += [[t + dn, v] for t, v in mine if t >= s]
    return _increasing(out)


def _strike_times(model: dict) -> list:
    return [float(s["t"]) for s in model.get("strikes", [])]


def gate_map(model: dict, name: str, kn: dict) -> Callable[[list], list] | None:
    """The gate's time map for a kind and its knob values, or None."""
    if name == "sines" and kn.get("stretch", 1.0) != 1:
        on, st = float(model.get("onset", 0.0)), kn["stretch"]
        return lambda pts: [[on + (t - on) * st, d] for t, d in pts]
    if name == "spectral":
        sc = kn.get("length", 1.0) / max(kn.get("speed", 1.0), 0.001)
        if sc != 1:
            return lambda pts: [[t * sc, d] for t, d in pts]
    if name == "gesture" and kn.get("length", 1.0) != 1:
        on, ln = float((model.get("tones") or {}).get("onset", 0.0) or 0.0), kn["length"]
        return lambda pts: [[on + (t - on) * ln, d] for t, d in pts]
    if name == "modal" and kn.get("spacing", 1.0) != 1 and len(model.get("strikes", [])) >= 2:
        ts, sp = _strike_times(model), kn["spacing"]
        return lambda pts: modal_gate_map(pts, ts, sp)
    return lambda pts: pts


# ---------------------------------------------------------------- render


def _vary_knobs(model: dict, knobs: dict) -> dict:
    kn = dict(knobs)
    if "vary_scale" in model and kn.get("vary"):
        kn["vary"] = kn["vary"] * float(model["vary_scale"])
    return kn


def _post_and_layers(model: dict, name: str, y: np.ndarray, seed: int, take: bool, bed: bool,
                     space: bool, kn: dict) -> np.ndarray:
    post = model.get("post")
    if not post:
        return y
    scale, tail, anchor = _time_scale(model, name, kn)
    tmap = gate_map(model, name, kn) if post.get("gate") else None
    y = apply_post(y, post, scale=scale, tail=tail, tmap=tmap, anchor=anchor)
    if space and post.get("space"):
        y = np.asarray(_model.space_module().render_space(post["space"], y, seed, width=kn.get("width", 1.0),
                                                          ring=take, limit=False), dtype=np.float64)
    if bed is not False and post.get("bed"):
        y = _post.add_bed(y, post["bed"], seed)
    return y


def render_full(model: dict, seed: int = 0, take: bool | None = None, bed: bool = True, final: bool = True,
                space: bool = True, **knobs: Any) -> np.ndarray:
    """render with the internal switches: final=False leaves out the finish, space=False the space layer."""
    name = kind_of(model)
    _check_knobs(model, knobs)
    if take is None:
        take = float(knobs.get("vary", 0.0) or 0.0) > 0
    if name == "morph":
        amount = knobs.pop("morph", model.get("amount", 0.5))
        m = morph(model["a"], model["b"], amount)
        return render_full(m, seed, take, bed, final, space, **knobs)
    kn = _vary_knobs(model, knobs)
    ring = kn.pop("ring", None)
    ring = bool(take) if ring is None else bool(ring)
    y = np.asarray(_RAW[name](model, int(seed), ring, kn), dtype=np.float64)
    y = _post_and_layers(model, name, y, int(seed), bool(take), bed, space, kn)
    if take:
        y = _post.take_highpass(y)
    if final and finished(model):
        y = finish(y)
    return y


def render(model: dict, seed: int = 0, take: bool | None = None, bed: bool = True, **knobs: Any) -> np.ndarray:
    """The render of a model: float64 at 48 kHz, (n, 2) for a stereo model, else (n,)."""
    return render_full(model, seed, take, bed, True, True, **knobs)


# ---------------------------------------------------------------- morph


def morph(a: dict, b: dict, amount: float = 0.5) -> dict:
    """A new model between 2 fits of one kind."""
    from hark.fit.pipeline import _morph

    return _morph.morph(a, b, amount)


def morph_model(a: dict, b: dict, amount: float = 0.5) -> dict:
    """A morph model that holds both sources and an amount."""
    if kind_of(a) == "morph" or kind_of(b) == "morph":
        raise ValueError("a morph of a morph model is not supported")
    if kind_of(a) != kind_of(b):
        raise ValueError(f"cannot morph a {kind_of(a)} model with a {kind_of(b)} model; refit both with one kind")
    return {"kind": "morph", "version": 1, "amount": float(amount), "a": strip(a), "b": strip(b)}


# ---------------------------------------------------------------- fit pair


def _winner_score(model: dict) -> float:
    fe = model.get("fit_engine") or {}
    for row in fe.get("candidates", []):
        if row.get("candidate") == fe.get("chosen") and row.get("score") is not None:
            return float(row.get("score2", row["score"]))
    return 1e9


def _single(x: Any, cand: str, clap: bool, fast: bool) -> tuple[dict | None, float]:
    from hark.fit.pipeline import auto

    try:
        m = auto.fit(x, candidates=[cand], clap=clap, fast=fast, confirm=0)
        return m, _winner_score(m)
    except Exception:
        return None, 1e9


def fit_pair(xa: Any, xb: Any, clap: bool = False, fast: bool = False, verbose: bool = False) -> tuple:
    """2 recordings fitted with one kind: (model a, model b, {kind: (score a, score b)})."""
    from hark.fit.pipeline import auto

    ma = auto.fit(xa, clap=clap, fast=fast, verbose=verbose)
    mb = auto.fit(xb, clap=clap, fast=fast, verbose=verbose)
    ca, cb = ma["fit_engine"]["chosen"], mb["fit_engine"]["chosen"]
    if kind_of(ma) == kind_of(mb) and ca == cb:
        return ma, mb, {ca: (_winner_score(ma), _winner_score(mb))}
    tried: dict = {}
    pairs: dict = {}
    oa, sa = _single(xa, cb, clap, fast)
    ob, sb = _single(xb, ca, clap, fast)
    tried[ca] = (_winner_score(ma), sb)
    tried[cb] = (sa, _winner_score(mb))
    pairs[ca] = (ma, ob)
    pairs[cb] = (oa, mb)
    best = min(tried, key=lambda k: tried[k][0] + tried[k][1])
    a, b = pairs[best]
    if a is None or b is None:
        raise RuntimeError("fit_pair: no kind fits both recordings")
    return a, b, tried


# ---------------------------------------------------------------- summary


def _indent(text: Any, pad: str = "  ") -> str:
    s = text if isinstance(text, str) else str(text)
    return "\n".join(pad + line for line in s.splitlines())


def _kind_summary(model: dict) -> str:
    name = kind_of(model)
    if name == "morph":
        head = f"morph {float(model.get('amount', 0.5)):.2f} of 2 {kind_of(model['a'])} fits"
        return "\n".join([head, "a:", _indent(_kind_summary(model["a"])), "b:", _indent(_kind_summary(model["b"]))])
    if name == "hybrid":
        layers = model.get("layers", [])
        lines = [f"hybrid of {len(layers)} layers, {1000 * float(model.get('length_s', 0)):.0f} ms"]
        for layer in layers:
            m = layer["model"]
            det = "deterministic" if m.get("layer") == "deterministic" else "noise"
            lines.append(f"{layer.get('role')}: {kind_of(m)} ({det}), gain {float(layer.get('gain_db', 0)):+.1f} dB")
            lines.append(_indent(_kind_summary(m), "    "))
        return "\n".join(lines)
    s = _model.module(name).summary(model)
    return s if isinstance(s, str) else "\n".join(f"{k}: {v}" for k, v in s.items())


def summary(model: dict) -> str:
    """Readable lines about a model."""
    lines = []
    fe = model.get("fit_engine") or {}
    if fe.get("kind") == "auto":
        rows = ", ".join(f"{r['candidate']} {r['score']:.2f}" if r.get("score") is not None
                         else f"{r['candidate']} failed" for r in fe.get("candidates", []))
        lines.append(f"auto chose {fe.get('chosen')} from: {rows}")
    lines.append(_kind_summary(model))
    return "\n".join(lines)
