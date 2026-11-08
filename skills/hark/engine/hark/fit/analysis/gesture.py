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
from hark.fit.analysis._common import (SR, as_stereo, check_kind, merge_knobs, is_mono_pair, load_json, resample,
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

def _one_channel(ymat: np.ndarray, cv: int, cnt: int) -> np.ndarray:
    yv = ymat if ymat.ndim == 1 else ymat[:, min(cv, ymat.shape[1] - 1)]
    return np.pad(yv[:cnt], (0, max(0, cnt - len(yv))))


def _channel_residual(x: np.ndarray, yv: np.ndarray, alph: float, fft_n: int) -> np.ndarray:
    kw = {"window": "hann", "nperseg": fft_n, "noverlap": fft_n - 256}
    sx_m = stft(x, **kw)[2]
    sy_m = stft(yv, **kw)[2]
    gv = np.sqrt(np.clip(1 - alph * np.abs(sy_m) ** 2 / (np.abs(sx_m) ** 2 + 1e-30), 0.0, 1.0))
    rv = istft(sx_m * gv, **kw)[1][:len(x)]
    return np.pad(rv, (0, len(x) - len(rv)))


def residual(X: np.ndarray, Y: np.ndarray, alpha: float = ALPHA, n_fft: int = 1024) -> np.ndarray:
    """X minus Y in the magnitude domain, with X's phase."""
    X = np.asarray(X, dtype=float)
    Y = np.asarray(Y, dtype=float)
    if X.ndim == 1:
        return _channel_residual(X, _one_channel(Y, 0, len(X)), alpha, n_fft)
    return np.column_stack([_channel_residual(X[:, cv], _one_channel(Y, cv, len(X)), alpha, n_fft)
                            for cv in range(X.shape[1])])


# ================================================================ tones

def _modal_part(tonal: dict, pair2: bool) -> dict:
    lwin = tonal["length_s"]
    tonal["noise"]["env"] = [[[0.0, -240.0], [lwin, -240.0]] for _ in tonal["noise"]["env"]]
    tonal["onset"] = tonal["strikes"][0]["t"] if tonal["strikes"] else 0.0
    if not pair2:
        tonal.pop("side", None)
        tonal.pop("side_t", None)
    return tonal


def _sines_part(tonal: dict, pair2: bool, min_part_ms: float) -> dict:
    lifev = min_part_ms / 1000.0
    tonal["partials"] = [pv for pv in tonal["partials"] if pv["t1"] + pv["rel"] - pv["t0"] >= lifev]
    tonal["residual"] = {"bands": [1000.0], "times": [0.0, tonal["duration"]], "db": [[-150.0, -150.0]]}
    if not pair2:
        tonal.pop("side", None)
    return tonal


def _deterministic(model: dict, pair2: bool, min_part_ms: float) -> dict:
    tonal = copy.deepcopy({ix: vv for ix, vv in model.items() if ix not in ("post", "fit_engine")})
    tonal["layer"] = "deterministic"
    tonal["channels"] = 2 if pair2 else 1
    if tonal["kind"] == "modal":
        return _modal_part(tonal, pair2)
    return _sines_part(tonal, pair2, min_part_ms)


def _tonal_len(tonal: dict) -> float:
    return tonal["length_s"] if tonal["kind"] == "modal" else tonal["duration"]


def _tonal_render(tonal: dict, seed: int, sv: float, varyv: float, wdt: float, tone_db: float,
                  ringv: bool, pair2: bool) -> np.ndarray:
    if tonal["kind"] == "modal":
        return modal.render(tonal, seed, spacing=sv, damping=1 / sv, vary=varyv, width=wdt, gain=tone_db,
                            ring=ringv, stereo=pair2)
    return sinusoidal.render(tonal, seed, stretch=sv, vary=varyv, width=wdt, gain_db=tone_db, ring=ringv,
                             stereo=pair2)


def _rows_scaled(rws: list, gv: float) -> list:
    return [[rv[0], rv[1], rv[2] * gv, rv[3]] for rv in rws]


def _modal_gain(acc: dict, gv: float) -> None:
    for stv in acc["strikes"]:
        stv["modes"] = _rows_scaled(stv["modes"], gv)
        stv["attack"] = _rows_scaled(stv["attack"], gv)
    if acc.get("sub"):
        acc["sub"]["x"] = [vv * gv for vv in acc["sub"]["x"]]


def _sines_gain(acc: dict, gdb: float) -> None:
    for pv in acc["partials"]:
        pv["amp"] = [[tv, av + gdb] for tv, av in pv["amp"]]
    for trk in acc.get("transients", []):
        trk["atoms"] = _rows_scaled(trk["atoms"], 10 ** (gdb / 20))


def gain_tones(det: dict, gain_db: float) -> dict:
    """A copy of a tones model, gain_db louder."""
    acc = copy.deepcopy(det)
    if acc["kind"] == "modal":
        _modal_gain(acc, 10 ** (gain_db / 20))
    else:
        _sines_gain(acc, gain_db)
    return acc


def _tonal_fit(av: np.ndarray, pair2: bool, kindv: str, quick: bool) -> dict:
    srcv = av if pair2 else av[:, 0]
    if kindv == "modal":
        return modal.fit(srcv, passes=1 if quick else 2)
    return sinusoidal.fit(srcv, windows=(1024,) if quick else sinusoidal.WINDOWS)


# ================================================================ air

def air(x: Any, y_det: np.ndarray | None = None, alpha: float = ALPHA, fast: bool = False,
        name: str | None = None, mask_fft: int = 1024, **kw) -> dict:
    """A spectral model of x, or of the remainder of x after the noise-free render y_det."""
    av = as_stereo(x)
    rmat = av if y_det is None else residual(av, as_stereo(y_det), alpha, mask_fft)
    cfg = dict(spectral.DETAIL, **AIR_OPTS)
    if fast:
        cfg.update(FAST_AIR)
    cfg.update(kw)
    mv = spectral.fit(rmat, name=name, **cfg)
    mv["channels"] = 1 if is_mono_pair(av) else 2
    return mv


# ================================================================ fit

def _tonal_layer(av: np.ndarray, pair2: bool, tonal_l: Any, kindv: str, quick: bool,
                 min_part_ms: float, tonal_min: float) -> tuple:
    """(deterministic tones or None, tone share, tones render (n, 2) or None)."""
    if tonal_l is False:
        return None, 0.0, None
    srcv = tonal_l if isinstance(tonal_l, dict) else _tonal_fit(av, pair2, kindv, quick)
    tonal = _deterministic(srcv, pair2, min_part_ms)
    y_tonal = _len_of_fit(as_stereo(_tonal_render(tonal, 0, 1.0, 0.0, 1.0, 0.0, False, pair2)), len(av))
    ex_v = float(np.sum(av ** 2))
    frac = float(np.sum(y_tonal ** 2)) / ex_v if ex_v > 0 else 0.0
    if frac < tonal_min:
        return None, frac, None
    return tonal, frac, y_tonal


FIT_KNOBS = {"tones": None, "alpha": ALPHA, "tone_min": 0.05, "min_partial_ms": 0, "fast": False,
             "tones_kind": "sines", "verbose": False, "sr": SR}


def fit(x: Any, name: str | None = None, **kw) -> dict:
    """A gesture model: deterministic tones plus a fine spectral air model of the rest.

    Knobs and defaults: FIT_KNOBS. Other keywords go to the air fit (spectral.fit options)."""
    ix = dict(FIT_KNOBS, **{kname: kw.pop(kname) for kname in list(kw) if kname in FIT_KNOBS})
    if ix["tones_kind"] not in ("sines", "modal"):
        raise ValueError(f"tones_kind must be 'sines' or 'modal', not {ix['tones_kind']!r}")
    av = as_stereo(x) if isinstance(x, str) else as_stereo(resample(np.asarray(x, float), ix["sr"]))
    model = _fit_core(av, lambda: name or source_name(x), ix, kw)
    if ix["verbose"]:
        print(summary(model))
    return model


def _fit_core(av: np.ndarray, name, ix: dict, air_kw: dict) -> dict:
    tm0 = time.time()
    pair2 = not is_mono_pair(av)
    tonal, frac, y_tonal = _tonal_layer(av, pair2, ix["tones"], ix["tones_kind"], ix["fast"], ix["min_partial_ms"],
                                     ix["tone_min"])
    tm1 = time.time()
    nmv = name()
    amv = air(av, y_tonal, ix["alpha"], ix["fast"], nmv, **air_kw)
    return _keep_model(av, nmv, tonal, amv, ix["alpha"], frac, pair2, (tm1 - tm0, time.time() - tm1))


def _len_of_fit(yv: np.ndarray, cnt: int) -> np.ndarray:
    if len(yv) >= cnt:
        return yv[:cnt]
    return np.pad(yv, ((0, cnt - len(yv)), (0, 0)))


def _keep_model(av, name, tonal, amv, alph, frac, pair2, sec_v) -> dict:
    pieces = 0
    if tonal is not None:
        pieces = (sum(len(sv["modes"]) for sv in tonal["strikes"]) if tonal["kind"] == "modal"
                 else len(tonal["partials"]))
    return {"kind": KIND, "version": VERSION, "sr": SR, "dur": round(len(av) / SR, 6),
            "channels": 2 if pair2 else 1, "source": name, "tones": tonal, "air": amv,
            "traj": spectral.trajectory(amv),
            "fit": {"alpha": alph, "tones_kind": tonal["kind"] if tonal else None,
                    "tone_share": round(frac if tonal else 0.0, 4), "partials": pieces,
                    "air_keys": len(amv["env"]["t"]), "ridges": len(amv.get("ridges") or []),
                    "seconds": {"tones": round(sec_v[0], 3), "air": round(sec_v[1], 3)}}}


# ================================================================ render

def _air_extend(amv: dict, more: float) -> dict:
    acc = copy.deepcopy(amv)
    envl = acc["env"]
    tm_end = acc["dur"] + more
    envl["t"].append(round(tm_end, 6))
    envl["db"].append([vv - 60.0 for vv in envl["db"][-1]])
    if acc.get("stereo"):
        acc["stereo"]["ild"].append(list(acc["stereo"]["ild"][-1]))
        acc["stereo"]["icc"].append(list(acc["stereo"]["icc"][-1]))
    acc["dur"] = tm_end
    return acc


def _time_shift(yv: np.ndarray, ix: int) -> np.ndarray:
    if ix > 0:
        return np.vstack([np.zeros((ix, yv.shape[1])), yv])
    return yv[-ix:]


def _put_at(pieces: list, mv: int) -> np.ndarray:
    acc = np.zeros((mv, 2))
    for pv in pieces:
        ix = min(mv, len(pv))
        acc[:ix] += pv[:ix]
    return acc


def _layer_set(model: dict, seed: int, sv: float, ix: dict, pair2: bool, ringv: bool) -> list:
    """The tones render and the aligned air render, each (m, 2)."""
    tonal = model.get("tones")
    amv = model["air"]
    pieces = []
    if tonal:
        pieces.append(as_stereo(_tonal_render(tonal, seed, sv, ix["vary"], ix["width"], ix["tone_db"], ringv, pair2)))
        if ringv:
            amv = _air_extend(amv, max(0.06, len(pieces[0]) / SR / sv - amv["dur"]))
    yv_air = spectral.render(amv, (int(seed) * 7 + 101) & 0x7FFFFFFF, length=sv, sweep=ix["sweep"],
                            roughness=ix["roughness"], width=ix["width"], pan=ix["pan"],
                            proximity=ix["proximity"], gain_db=ix["air_db"], limit=False).astype(float)
    start_i = float(tonal.get("onset", 0.0)) if tonal else 0.0
    pieces.append(_time_shift(yv_air, int(round(start_i * (1 - sv) * SR))))
    return pieces


def _flyby(yv: np.ndarray, trj: dict, sv: float, doppler: float, motion: float) -> np.ndarray:
    t_pk = trj["peak_s"] * sv
    tau = max(0.015, 0.25 * (trj["rise_s"] + trj["fall_s"]) * sv)
    if doppler:
        yv = spectral.bend(yv, t_pk, tau, doppler)
    if motion:
        yv = spectral.pass_pan(yv, t_pk, tau, motion)
    return yv


RENDER_DEFAULTS = {"length": 1, "sweep": 1, "doppler": 0, "proximity": 0, "pan": 1, "motion": 0, "width": 1,
                   "roughness": 1, "tone_db": 0, "air_db": 0, "vary": 0, "gain_db": 0, "stereo": None,
                   "ring": False}


def render(model: dict, seed: int = 0, **knobs) -> np.ndarray:
    """float64 at 48 kHz: (n, 2) for a stereo source or stereo=True, else (n,). Knobs and defaults: RENDER_DEFAULTS."""
    check(model)
    ix = merge_knobs(RENDER_DEFAULTS, knobs, "gesture render")
    pair2 = model.get("channels", 1) == 2 if ix["stereo"] is None else bool(ix["stereo"])
    yv = _render_pair(model, seed, ix, pair2)
    return yv if pair2 else yv.mean(axis=1)


def _render_pair(model: dict, seed: int, ix: dict, pair2: bool) -> np.ndarray:
    sv = max(ix["length"], 0.001)
    cnt = max(64, int(round(model["dur"] * sv * SR)))
    ringv = ix["ring"]
    pieces = _layer_set(model, seed, sv, ix, pair2, ringv)
    yv = _put_at(pieces, max([cnt] + [len(pv) for pv in pieces]) if ringv else cnt)
    if ix["doppler"] or ix["motion"]:
        yv = _flyby(yv, model["traj"], sv, ix["doppler"], ix["motion"])
    yv = yv * 10 ** (ix["gain_db"] / 20)
    return _fade_end(yv) if ringv else yv


def _fade_end(yv: np.ndarray) -> np.ndarray:
    ix = int(min(len(yv) / 3, 0.01 * SR))
    if ix > 1:
        yv = yv.copy()
        yv[-ix:] *= (0.5 + 0.5 * np.cos(np.pi * np.arange(ix) / ix))[:, None]
    return yv


# ================================================================ face

def check(model: Any) -> None:
    """ValueError when the model is not a gesture model of a supported version."""
    check_kind(model, KIND, VERSION)
    tkv = (model.get("fit") or {}).get("tones_kind")
    if tkv not in (None, "sines", "modal"):
        raise ValueError(f"gesture tones_kind {tkv!r} is not sines or modal")


def save(model: dict, path: str) -> None:
    """Write compact JSON."""
    save_json(model, path)


def load(path: str) -> dict:
    """Read a model and check its kind."""
    mv = load_json(path)
    check(mv)
    return mv


def summary(model: dict) -> str:
    """Readable lines: source, tones, air, trajectory and size."""
    line_l = [f"gesture {model.get('source') or 'array'}: {model['dur'] * 1000:.0f} ms"]
    line_l.append(_tonal_words(model.get("tones"), model["fit"]["tone_share"]))
    line_l.append("air: " + spectral.summary(model["air"]))
    trk = model["traj"]
    cenv = trk["centroid_hz"]
    panv = trk["pan_db"]
    line_l.append(f"trajectory: peak {trk['peak_s'] * 1000:.0f} ms, rise {trk['rise_s'] * 1000:.0f} ms, "
                 f"fall {trk['fall_s'] * 1000:.0f} ms, centroid {cenv[0]:.0f} / "
                 f"{cenv[int(np.argmax(trk['level_db']))]:.0f} / {cenv[-1]:.0f} Hz, pan {min(panv):+.1f} to "
                 f"{max(panv):+.1f} dB, median width {float(np.median(trk['width'])):.2f}")
    line_l.append(f"size {len(json.dumps(model, separators=(',', ':'))) / 1024:.1f} kB")
    return "\n".join(line_l)


def _tonal_words(tonal: dict | None, frac: float) -> str:
    if not tonal:
        return "no tones (air only)"
    pctv = f"{100 * frac:.0f} % of the energy"
    if tonal["kind"] == "modal":
        fq = [rv[0] for sv in tonal["strikes"] for rv in sv["modes"]]
        gen = f"{min(fq):.0f}-{max(fq):.0f} Hz" if fq else "none"
        return f"tones: modal, {len(tonal['strikes'])} strikes, {len(fq)} modes {gen}, {pctv}"
    fq0 = [pv["freq"][0][1] for pv in tonal["partials"]]
    gen = f"{min(fq0):.0f}-{max(fq0):.0f} Hz" if fq0 else "none"
    return f"tones: sines, {len(fq0)} partials from {gen}, {pctv}"
