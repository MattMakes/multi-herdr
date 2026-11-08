"""Model kinds, channel counts, kind modules and model files."""

from __future__ import annotations

import copy
import json
import math
from types import ModuleType
from typing import Any

import numpy as np

SR = 48000

# model "kind" value -> kind_of name
NAMES = {
    "modal": "modal",
    "sines": "sines",
    "spectral_noise": "spectral",
    "gesture": "gesture",
    "hybrid": "hybrid",
    "morph": "morph",
}
# kind_of name -> model "kind" value, for the 4 analysis kinds
VALUES = {"modal": "modal", "sines": "sines", "spectral": "spectral_noise", "gesture": "gesture"}
VERSIONS = {"hybrid": 1, "morph": 1}


def kind_of(model: Any) -> str:
    """The kind_of name of a model; ValueError for an unknown kind."""
    kind = model.get("kind") if isinstance(model, dict) else None
    if kind not in NAMES:
        raise ValueError(f"unknown fit model kind {kind!r}; expected one of {sorted(NAMES)}")
    return NAMES[kind]


def module(name: str) -> ModuleType:
    """The analysis module of a kind_of name (modal, sines, spectral, gesture)."""
    from hark.fit import analysis

    return analysis.module_for(VALUES[name])


def space_module() -> ModuleType:
    from hark.fit.analysis import space

    return space


def check(model: Any) -> None:
    """ValueError for a wrong kind or a version above the supported one."""
    name = kind_of(model)
    if name in VERSIONS:
        if int(model.get("version", 1)) > VERSIONS[name]:
            raise ValueError(f"{name} model version {model.get('version')} is newer than 1")
        return
    module(name).check(model)


def array_channels(x: np.ndarray) -> int:
    """1 for a 1-D array, 1 column, or 2 equal channels; else 2."""
    x = np.asarray(x)
    if x.ndim == 1 or x.shape[1] == 1:
        return 1
    return 1 if np.array_equal(x[:, 0], x[:, 1]) else 2


def _model_channels(model: dict) -> int:
    name = kind_of(model)
    if name == "morph":
        return max(channels_of(model["a"]), channels_of(model["b"]))
    if name == "spectral":
        return 2 if model.get("stereo") else int(model.get("channels", 1))
    if name == "sines":
        if "channels" in model:
            return int(model["channels"])
        return 2 if float(model.get("width", -120)) > -50 else 1
    return int(model.get("channels", 1))


def channels_of(x_or_model: Any) -> int:
    """1 or 2: channels of an array or of a model's render."""
    if isinstance(x_or_model, dict):
        return _model_channels(x_or_model)
    return array_channels(x_or_model)


def strip(model: dict) -> dict:
    """A deep copy without fit_engine."""
    return {k: copy.deepcopy(v) for k, v in model.items() if k != "fit_engine"}


def as_two(y: np.ndarray, channels: int) -> np.ndarray:
    """Shape a render to (n, channels): mono repeated, stereo mean-mixed."""
    y = np.asarray(y, dtype=np.float64)
    if channels == 2:
        if y.ndim == 1:
            return np.column_stack([y, y])
        return y[:, :2] if y.shape[1] >= 2 else np.column_stack([y[:, 0], y[:, 0]])
    m = y if y.ndim == 1 else y.mean(axis=1)
    return m[:, None]


def _jsonable(o: Any) -> Any:
    if isinstance(o, dict):
        return {str(k): _jsonable(v) for k, v in o.items()}
    if isinstance(o, (list, tuple)):
        return [_jsonable(v) for v in o]
    if isinstance(o, np.ndarray):
        return _jsonable(o.tolist())
    if isinstance(o, np.generic):
        return o.item()
    if isinstance(o, float) and not math.isfinite(o):
        return None
    return o


def save(model: dict, path: str) -> None:
    """Write a model as compact JSON."""
    with open(path, "w", encoding="utf-8") as f:
        json.dump(_jsonable(model), f, separators=(",", ":"))


def load(path: str) -> dict:
    """Read a model file and check its kind."""
    with open(path, encoding="utf-8") as f:
        model = json.load(f)
    check(model)
    return model
