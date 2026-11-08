"""Single-kind fits, deterministic copies and hybrid fits."""

from __future__ import annotations

import copy
from typing import Any

import numpy as np

from hark.fit.pipeline import _model
from hark.fit.pipeline._model import as_two, channels_of, kind_of

FIT_KINDS = ("modal", "sines", "spectral", "gesture")
ROLE = {"sines": "tone", "modal": "body", "spectral": "air"}
HYBRIDS = {
    "modal+spectral": ("modal", "spectral"),
    "sines+spectral": ("sines", "spectral"),
    "sines+modal+spectral": ("sines", "modal", "spectral"),
}
FAST = {
    "modal": {"passes": 1},
    "sines": {"windows": (1024,)},
    "spectral": {"texture": False, "iters": 1, "seeds": (1,)},
    "gesture": {"fast": True},
}


def fit_kind(x: Any, kind: str, fast: bool = False, name: str | None = None, **opts: Any) -> dict:
    """One fitter: modal, sines, spectral or gesture."""
    if kind not in FIT_KINDS:
        raise ValueError(f"fit_kind: unknown kind {kind!r}; expected one of {FIT_KINDS}")
    X = np.asarray(x, dtype=np.float64)
    kw = dict(FAST[kind]) if fast else {}
    kw.update(opts)
    if name is not None and kind in ("spectral", "gesture"):
        kw.setdefault("name", name)
    model = _model.module(kind).fit(X, **kw)
    if kind in ("sines", "spectral"):
        model["channels"] = channels_of(X)
    if name is not None:
        model.setdefault("source", name)
    return model


def _quiet_modal(m: dict) -> None:
    noise = m.get("noise") or {}
    noise["env"] = [[[float(p[0]), -240.0] for p in band] for band in noise.get("env", [])]
    m["noise"] = noise
    m["side"] = None
    m.pop("side_t", None)


def _quiet_sines(m: dict) -> None:
    res = m.get("residual") or {}
    bands = list(res.get("bands") or [1000.0])[:1]
    times = list(res.get("times") or [0.0])
    m["residual"] = dict(res, bands=bands, times=times, db=[[-150.0] * len(times)])
    m["width"] = -120.0


_QUIET = {"modal": _quiet_modal, "sines": _quiet_sines}


def deterministic(model: dict) -> dict:
    """A copy of a modal or sines model without its noise."""
    name = kind_of(model)
    if name not in _QUIET:
        raise ValueError(f"deterministic: a {name} model has no deterministic copy")
    m = {k: copy.deepcopy(v) for k, v in model.items() if k not in ("post", "fit_engine")}
    _QUIET[name](m)
    m["channels"] = 1
    m["layer"] = "deterministic"
    return m


def _raw(model: dict, seed: int, **kw: Any) -> np.ndarray:
    from hark.fit.pipeline.render import render_full

    return render_full(model, seed, take=False, bed=False, final=False, **kw)


def _side_energy(r: np.ndarray, y: np.ndarray) -> float:
    return float(np.sum((r[:, 0] - y[:, 0] - r[:, 1] + y[:, 1]) ** 2))


def _layer_take(layer: dict, residual: np.ndarray) -> np.ndarray:
    """The part of the residual that a deterministic layer takes, as (n, channels)."""
    n, ch = residual.shape
    best = _fit_len(as_two(_raw(layer, 0, stereo=False), ch), n)
    if kind_of(layer) == "sines" and layer.get("side") and ch == 2:
        best_e = _side_energy(residual, best)
        for s in range(8):
            y = _fit_len(as_two(_raw(layer, s, stereo=True), 2), n)
            e = _side_energy(residual, y)
            if e < best_e:
                best, best_e = y, e
    return best


def _fit_len(y: np.ndarray, n: int) -> np.ndarray:
    return y[:n] if len(y) >= n else np.pad(y, ((0, n - len(y)), (0, 0)))


def _check_layers(layers: tuple) -> None:
    ok = 2 <= len(layers) <= 3 and all(k in ROLE for k in layers) and layers[-1] != "sines"
    if not ok:
        raise ValueError(f"fit_hybrid: bad layers {layers}; use 2 or 3 of modal, sines, spectral, the last not sines")


def fit_hybrid(x: Any, layers: tuple = ("modal", "spectral"), fast: bool = False, base: dict | None = None,
               name: str | None = None) -> dict:
    """A hybrid: deterministic layers, then a noise layer fitted on what they leave."""
    layers = tuple(layers)
    _check_layers(layers)
    X = np.asarray(x, dtype=np.float64)
    ch = channels_of(X)
    residual = as_two(X, ch)
    entries = []
    for i, kind in enumerate(layers):
        src = residual if ch == 2 else residual[:, 0]
        m = base if (i == 0 and base is not None and kind_of(base) == kind) else fit_kind(src, kind, fast, name)
        if i < len(layers) - 1:
            m = deterministic(m)
            residual = residual - _layer_take(m, residual)
        entries.append({"role": ROLE[kind], "gain_db": 0.0, "model": m})
    return {"kind": "hybrid", "version": 1, "sr": 48000, "length_s": round(len(X) / 48000, 6), "channels": ch,
            "layers": entries, "source": name}
