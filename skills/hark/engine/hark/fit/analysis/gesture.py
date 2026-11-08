"""Gesture fitter: tones (a noise-free sines or modal fit) plus air (a spectral fit of the rest).

The layers are separated on STFT magnitudes, so the air never holds tone
energy that the recording lacks.
"""

from __future__ import annotations

import copy
import json
import time
from typing import Any

import numpy as np
from scipy.signal import istft, stft

from hark.fit.analysis import modal, sinusoidal, spectral
from hark.fit.analysis._common import (SR, as_stereo, check_kind, is_mono_pair, load_json, resample,
                                       save_json, source_name)

KIND = "gesture"
NAME = "gesture"
VERSION = 1

KNOBS: dict[str, tuple] = {
    "length": ((0.8, 1.25, "log"), 1.0, "Time-stretch. The tones stretch around their first strike."),
    "sweep": ((0.7, 1.43, "log"), 1.0, "Factor on the air's centroid excursion."),
    "doppler": ((-2.0, 2.0), 0.0, "Doppler bend in semitones, turning at the air's peak."),
    "proximity": ((-0.5, 1.0), 0.0, "Makes the air's softer keyframes darker and softer."),
    "pan": ((0.5, 1.5), 1.0, "Factor on the air's L/R level differences."),
    "motion": ((-0.6, 0.6), 0.0, "Moves the full sound across the stereo field as it passes."),
    "width": ((0.5, 1.5), 1.0, "Factor on the tones' side and the air's width."),
    "roughness": ((0.5, 1.5), 1.0, "The air's texture depth (flutter, crackle)."),
    "tone_db": ((-6.0, 6.0), 0.0, "Level of the tones."),
    "air_db": ((-6.0, 6.0), 0.0, "Level of the air."),
    "vary": ((0.0, 0.4), 0.0, "Seeded jitter of the tones."),
}

ALPHA = 3.0
AIR_OPTS = {"gesture_ms": 5, "quiet_db": 70, "grain_norm_ms": 5, "grain_halves": True}
FAST_AIR = {"iters": 1, "texture": False, "seeds": (1,)}

__all__ = ["KIND", "NAME", "VERSION", "KNOBS", "fit", "render", "check", "summary", "save", "load",
           "air", "residual", "gain_tones"]


# ================================================================ magnitude residual

def _chan(Y: np.ndarray, c: int, n: int) -> np.ndarray:
    y = Y if Y.ndim == 1 else Y[:, min(c, Y.shape[1] - 1)]
    return np.pad(y[:n], (0, max(0, n - len(y))))


def residual(X: np.ndarray, Y: np.ndarray, alpha: float = ALPHA, n_fft: int = 1024) -> np.ndarray:
    """X minus Y in the magnitude domain, with X's phase."""
    X = np.asarray(X, dtype=float)
    Y = np.asarray(Y, dtype=float)
    n = len(X)
    cols = [X] if X.ndim == 1 else [X[:, c] for c in range(X.shape[1])]
    out = []
    for c, x in enumerate(cols):
        kw = {"window": "hann", "nperseg": n_fft, "noverlap": n_fft - 256}
        Sx = stft(x, **kw)[2]
        Sy = stft(_chan(Y, c, n), **kw)[2]
        g = np.sqrt(np.clip(1 - alpha * np.abs(Sy) ** 2 / (np.abs(Sx) ** 2 + 1e-30), 0.0, 1.0))
        r = istft(Sx * g, **kw)[1][:n]
        out.append(np.pad(r, (0, n - len(r))))
    return out[0] if X.ndim == 1 else np.column_stack(out)


# ================================================================ tones

def _deterministic(model: dict, two: bool, min_partial_ms: float) -> dict:
    det = copy.deepcopy({k: v for k, v in model.items() if k not in ("post", "fit_engine")})
    det["layer"] = "deterministic"
    det["channels"] = 2 if two else 1
    if det["kind"] == "modal":
        L = det["length_s"]
        det["noise"]["env"] = [[[0.0, -240.0], [L, -240.0]] for _ in det["noise"]["env"]]
        det["onset"] = det["strikes"][0]["t"] if det["strikes"] else 0.0
        if not two:
            det.pop("side", None)
            det.pop("side_t", None)
        return det
    life = min_partial_ms / 1000.0
    det["partials"] = [p for p in det["partials"] if p["t1"] + p["rel"] - p["t0"] >= life]
    det["residual"] = {"bands": [1000.0], "times": [0.0, det["duration"]], "db": [[-150.0, -150.0]]}
    if not two:
        det.pop("side", None)
    return det


def _tones_length(det: dict) -> float:
    return det["length_s"] if det["kind"] == "modal" else det["duration"]


def _render_tones(det: dict, seed: int, s: float, vary: float, width: float, tone_db: float,
                  ring: bool, two: bool) -> np.ndarray:
    if det["kind"] == "modal":
        return modal.render(det, seed, spacing=s, damping=1 / s, vary=vary, width=width, gain=tone_db,
                            ring=ring, stereo=two)
    return sinusoidal.render(det, seed, stretch=s, vary=vary, width=width, gain_db=tone_db, ring=ring,
                             stereo=two)


def gain_tones(det: dict, gain_db: float) -> dict:
    """A copy of a tones model, gain_db louder."""
    out = copy.deepcopy(det)
    g = 10 ** (gain_db / 20)
    if out["kind"] == "modal":
        for st in out["strikes"]:
            st["modes"] = [[r[0], r[1], r[2] * g, r[3]] for r in st["modes"]]
            st["attack"] = [[r[0], r[1], r[2] * g, r[3]] for r in st["attack"]]
        if out.get("sub"):
            out["sub"]["x"] = [v * g for v in out["sub"]["x"]]
        return out
    for p in out["partials"]:
        p["amp"] = [[t, a + gain_db] for t, a in p["amp"]]
    for tr in out.get("transients", []):
        tr["atoms"] = [[f, tau, a * g, ph] for f, tau, a, ph in tr["atoms"]]
    return out


def _fit_tones(a: np.ndarray, two: bool, kind: str, fast: bool) -> dict:
    src = a if two else a[:, 0]
    if kind == "modal":
        return modal.fit(src, passes=1 if fast else 2)
    return sinusoidal.fit(src, windows=(1024,) if fast else sinusoidal.WINDOWS)


# ================================================================ air

def air(x: Any, y_det: np.ndarray | None = None, alpha: float = ALPHA, fast: bool = False,
        name: str | None = None, mask_fft: int = 1024, **kw) -> dict:
    """A spectral model of x, or of the remainder of x after the noise-free render y_det."""
    a = as_stereo(x)
    R = a if y_det is None else residual(a, as_stereo(y_det), alpha, mask_fft)
    opts = dict(spectral.DETAIL, **AIR_OPTS)
    if fast:
        opts.update(FAST_AIR)
    opts.update(kw)
    m = spectral.fit(R, name=name, **opts)
    m["channels"] = 1 if is_mono_pair(a) else 2
    return m


# ================================================================ fit

def fit(x: Any, name: str | None = None, tones: Any = None, alpha: float = ALPHA,
        tone_min: float = 0.05, min_partial_ms: float = 0, fast: bool = False,
        tones_kind: str = "sines", verbose: bool = False, sr: int = SR, **air_kw) -> dict:
    """A gesture model: deterministic tones plus a fine spectral air model of the rest."""
    if tones_kind not in ("sines", "modal"):
        raise ValueError(f"tones_kind must be 'sines' or 'modal', not {tones_kind!r}")
    t0 = time.time()
    a = as_stereo(x) if isinstance(x, str) else as_stereo(resample(np.asarray(x, float), sr))
    two = not is_mono_pair(a)
    n = len(a)
    e_x = float(np.sum(a ** 2))
    det, share, y_det = None, 0.0, None
    if tones is not False:
        src = tones if isinstance(tones, dict) else _fit_tones(a, two, tones_kind, fast)
        det = _deterministic(src, two, min_partial_ms)
        y_det = _fit_len(as_stereo(_render_tones(det, 0, 1.0, 0.0, 1.0, 0.0, False, two)), n)
        share = float(np.sum(y_det ** 2)) / e_x if e_x > 0 else 0.0
        if share < tone_min:
            det, y_det = None, None
    t1 = time.time()
    nm = name or source_name(x)
    am = air(a, y_det, alpha, fast, nm, **air_kw)
    model = _store(a, nm, det, am, alpha, share, two, (t1 - t0, time.time() - t1))
    if verbose:
        print(summary(model))
    return model


def _fit_len(y: np.ndarray, n: int) -> np.ndarray:
    if len(y) >= n:
        return y[:n]
    return np.pad(y, ((0, n - len(y)), (0, 0)))


def _store(a, name, det, am, alpha, share, two, secs) -> dict:
    parts = 0
    if det is not None:
        parts = (sum(len(s["modes"]) for s in det["strikes"]) if det["kind"] == "modal"
                 else len(det["partials"]))
    return {"kind": KIND, "version": VERSION, "sr": SR, "dur": round(len(a) / SR, 6),
            "channels": 2 if two else 1, "source": name, "tones": det, "air": am,
            "traj": spectral.trajectory(am),
            "fit": {"alpha": alpha, "tones_kind": det["kind"] if det else None,
                    "tone_share": round(share if det else 0.0, 4), "partials": parts,
                    "air_keys": len(am["env"]["t"]), "ridges": len(am.get("ridges") or []),
                    "seconds": {"tones": round(secs[0], 3), "air": round(secs[1], 3)}}}


# ================================================================ render

def _extend_air(am: dict, extra: float) -> dict:
    out = copy.deepcopy(am)
    env = out["env"]
    t_end = out["dur"] + extra
    env["t"].append(round(t_end, 6))
    env["db"].append([v - 60.0 for v in env["db"][-1]])
    if out.get("stereo"):
        out["stereo"]["ild"].append(list(out["stereo"]["ild"][-1]))
        out["stereo"]["icc"].append(list(out["stereo"]["icc"][-1]))
    out["dur"] = t_end
    return out


def _shift(y: np.ndarray, k: int) -> np.ndarray:
    if k > 0:
        return np.vstack([np.zeros((k, y.shape[1])), y])
    return y[-k:]


def _place(parts: list, m: int) -> np.ndarray:
    out = np.zeros((m, 2))
    for p in parts:
        k = min(m, len(p))
        out[:k] += p[:k]
    return out


def render(model: dict, seed: int = 0, length: float = 1, sweep: float = 1, doppler: float = 0,
           proximity: float = 0, pan: float = 1, motion: float = 0, width: float = 1,
           roughness: float = 1, tone_db: float = 0, air_db: float = 0, vary: float = 0,
           gain_db: float = 0, stereo: bool | None = None, ring: bool = False) -> np.ndarray:
    """float64 at 48 kHz: (n, 2) for a stereo source or stereo=True, else (n,)."""
    check(model)
    two = model.get("channels", 1) == 2 if stereo is None else bool(stereo)
    s = max(length, 0.001)
    n = max(64, int(round(model["dur"] * s * SR)))
    det = model.get("tones")
    am = model["air"]
    parts = []
    if det:
        parts.append(as_stereo(_render_tones(det, seed, s, vary, width, tone_db, ring, two)))
        if ring:
            am = _extend_air(am, max(0.06, len(parts[0]) / SR / s - am["dur"]))
    y_air = spectral.render(am, (int(seed) * 7 + 101) & 0x7FFFFFFF, length=s, sweep=sweep,
                            roughness=roughness, width=width, pan=pan, proximity=proximity,
                            gain_db=air_db, limit=False).astype(float)
    on = float(det.get("onset", 0.0)) if det else 0.0
    parts.append(_shift(y_air, int(round(on * (1 - s) * SR))))
    m = max([n] + [len(p) for p in parts]) if ring else n
    y = _place(parts, m)
    if doppler or motion:
        tr = model["traj"]
        t_pk = tr["peak_s"] * s
        tau = max(0.015, 0.25 * (tr["rise_s"] + tr["fall_s"]) * s)
        if doppler:
            y = spectral.bend(y, t_pk, tau, doppler)
        if motion:
            y = spectral.pass_pan(y, t_pk, tau, motion)
    y = y * 10 ** (gain_db / 20)
    if ring:
        y = _end_fade(y)
    return y if two else y.mean(axis=1)


def _end_fade(y: np.ndarray) -> np.ndarray:
    k = int(min(len(y) / 3, 0.01 * SR))
    if k > 1:
        y = y.copy()
        y[-k:] *= (0.5 + 0.5 * np.cos(np.pi * np.arange(k) / k))[:, None]
    return y


# ================================================================ face

def check(model: Any) -> None:
    """ValueError when the model is not a gesture model of a supported version."""
    check_kind(model, KIND, VERSION)
    tk = (model.get("fit") or {}).get("tones_kind")
    if tk not in (None, "sines", "modal"):
        raise ValueError(f"gesture tones_kind {tk!r} is not sines or modal")


def save(model: dict, path: str) -> None:
    """Write compact JSON."""
    save_json(model, path)


def load(path: str) -> dict:
    """Read a model and check its kind."""
    m = load_json(path)
    check(m)
    return m


def summary(model: dict) -> str:
    """Readable lines: source, tones, air, trajectory and size."""
    lines = [f"gesture {model.get('source') or 'array'}: {model['dur'] * 1000:.0f} ms"]
    lines.append(_tones_words(model.get("tones"), model["fit"]["tone_share"]))
    lines.append("air: " + spectral.summary(model["air"]))
    tr = model["traj"]
    cen = tr["centroid_hz"]
    pan = tr["pan_db"]
    lines.append(f"trajectory: peak {tr['peak_s'] * 1000:.0f} ms, rise {tr['rise_s'] * 1000:.0f} ms, "
                 f"fall {tr['fall_s'] * 1000:.0f} ms, centroid {cen[0]:.0f} / "
                 f"{cen[int(np.argmax(tr['level_db']))]:.0f} / {cen[-1]:.0f} Hz, pan {min(pan):+.1f} to "
                 f"{max(pan):+.1f} dB, median width {float(np.median(tr['width'])):.2f}")
    lines.append(f"size {len(json.dumps(model, separators=(',', ':'))) / 1024:.1f} kB")
    return "\n".join(lines)


def _tones_words(det: dict | None, share: float) -> str:
    if not det:
        return "no tones (air only)"
    pct = f"{100 * share:.0f} % of the energy"
    if det["kind"] == "modal":
        f = [r[0] for s in det["strikes"] for r in s["modes"]]
        rng = f"{min(f):.0f}-{max(f):.0f} Hz" if f else "none"
        return f"tones: modal, {len(det['strikes'])} strikes, {len(f)} modes {rng}, {pct}"
    f0 = [p["freq"][0][1] for p in det["partials"]]
    rng = f"{min(f0):.0f}-{max(f0):.0f} Hz" if f0 else "none"
    return f"tones: sines, {len(f0)} partials from {rng}, {pct}"
