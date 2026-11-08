"""Forge test helpers.

The forge engine calls a few core functions (load, write_wav, k_weight, report
helpers). When the core unit is not built yet, this file installs small
stand-ins for exactly those names. It installs nothing when the real modules
exist, so the integration run uses the real core.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import math
import sys
import types
from pathlib import Path

import numpy as np
from scipy import signal

SR = 48000


def _absent(name: str) -> bool:
    try:
        return importlib.util.find_spec(name) is None
    except ModuleNotFoundError:
        return True


def _audio_stand_in() -> types.ModuleType:
    import soundfile as sf

    mod = types.ModuleType("hark.core.audio")
    mod.SR = SR
    mod.Audio = np.ndarray

    class AudioError(RuntimeError):
        pass

    def to_mono(x):
        x = np.asarray(x)
        return x.mean(axis=1) if x.ndim == 2 else x

    def load(path, sr=SR, mono=False, start=None, end=None, cache=True):
        if not Path(path).is_file():
            raise AudioError(f"no such file: {path}")
        x, rate = sf.read(str(path), dtype="float32", always_2d=True)
        if rate != sr:
            g = math.gcd(rate, sr)
            x = signal.resample_poly(x, sr // g, rate // g, axis=0).astype(np.float32)
        i0 = int(round((start or 0) * sr))
        i1 = len(x) if end is None else int(round(end * sr))
        x = x[i0:i1]
        if x.shape[1] == 1:
            x = np.column_stack([x[:, 0], x[:, 0]])
        return to_mono(x).astype(np.float32) if mono else x[:, :2].astype(np.float32)

    def write_wav(path, x, sr=SR, subtype="PCM_24"):
        p = Path(path)
        p.parent.mkdir(parents=True, exist_ok=True)
        sf.write(str(p), np.asarray(x, dtype=np.float64), sr, subtype=subtype)
        return str(p)

    mod.AudioError, mod.to_mono, mod.load, mod.write_wav = AudioError, to_mono, load, write_wav
    mod.MEDIA_EXTS = frozenset({".wav", ".flac", ".aif", ".aiff", ".ogg", ".mp3"})
    return mod


_K = np.array([[1.53512485958697, -2.69169618940638, 1.19839281085285, 1.0, -1.69065929318241, 0.73248077421585],
               [1.0, -2.0, 1.0, 1.0, -1.99004745483398, 0.99007225036621]])


def _features_stand_in() -> types.ModuleType:
    mod = types.ModuleType("hark.core.features")

    def k_weight(x, sr=SR):
        return signal.sosfilt(_K, np.asarray(x, dtype=float), axis=0)

    mod.k_weight = k_weight
    return mod


def _clean(obj):
    if isinstance(obj, dict):
        return {str(k): _clean(v) for k, v in obj.items()}
    if isinstance(obj, (list, tuple)):
        return [_clean(v) for v in obj]
    if isinstance(obj, np.ndarray):
        return _clean(obj.tolist())
    if isinstance(obj, (np.floating, float)):
        v = float(obj)
        return None if not math.isfinite(v) else v
    if isinstance(obj, np.integer):
        return int(obj)
    if obj is None or isinstance(obj, (bool, int, str)):
        return obj
    return str(obj)


def _report_stand_in() -> types.ModuleType:
    mod = types.ModuleType("hark.core.report")

    def add_common(parser: argparse.ArgumentParser) -> None:
        parser.add_argument("--json", action="store_true")
        parser.add_argument("--out", default=None)
        parser.add_argument("--threads", type=int, default=0)

    def to_json(result, indent=1):
        return json.dumps(_clean(result), indent=indent)

    def emit(result, args, text=None):
        if getattr(args, "out", None):
            Path(args.out).parent.mkdir(parents=True, exist_ok=True)
            Path(args.out).write_text(to_json(result))
        print(to_json(result) if getattr(args, "json", False) else (text or result.get("text") or
                                                                    result.get("summary", "")))
        return result

    mod.add_common, mod.to_json, mod.emit, mod.clean = add_common, to_json, emit, _clean
    return mod


def _install() -> None:
    import hark

    if _absent("hark.core"):
        pkg = types.ModuleType("hark.core")
        pkg.__path__ = []
        sys.modules["hark.core"] = pkg
        hark.core = pkg
    makers = {"audio": _audio_stand_in, "features": _features_stand_in, "report": _report_stand_in}
    for name, make in makers.items():
        full = f"hark.core.{name}"
        if full not in sys.modules and _absent(full):
            mod = make()
            sys.modules[full] = mod
            setattr(sys.modules["hark.core"], name, mod)


_install()
