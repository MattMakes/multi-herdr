"""Fixtures and stand-ins for the fit pipeline tests.

The pipeline needs modules of other units: core audio and report, the models
guard, the forge EQ and refsets, and the 5 fit analysis modules. While one of
them is absent, the autouse fixture ``standins`` installs a small stand-in for
it in ``sys.modules`` for the duration of each test. A stand-in follows the
ARCHITECTURE.md 9.2 face and the model fields of its fit part, with simple
algorithms. A real module, once present, is always used instead.
"""

from __future__ import annotations

import argparse
import copy
import importlib.util
import json
import math
import sys
import types
from collections.abc import Callable

import numpy as np
import pytest
from scipy import signal

SR = 48000


# ---------------------------------------------------------------- shared helpers


def _mono(x: np.ndarray) -> np.ndarray:
    x = np.asarray(x, dtype=np.float64)
    return x.mean(axis=1) if x.ndim == 2 else x


def _two(x: np.ndarray) -> np.ndarray:
    x = np.asarray(x, dtype=np.float64)
    if x.ndim == 1:
        return np.column_stack([x, x])
    return x[:, :2] if x.shape[1] >= 2 else np.column_stack([x[:, 0], x[:, 0]])


def _is_stereo(x: np.ndarray) -> bool:
    x = np.asarray(x)
    return x.ndim == 2 and x.shape[1] >= 2 and not np.allclose(x[:, 0], x[:, 1])


def _db(p):
    return 10 * np.log10(np.maximum(p, 1e-20))


def _frames_db(m: np.ndarray, hop: int) -> np.ndarray:
    k = len(m) // hop
    return _db(np.mean(m[: k * hop].reshape(k, hop) ** 2, axis=1)) if k else np.zeros(0)


def _check(model, kind: str) -> None:
    if not isinstance(model, dict) or model.get("kind") != kind:
        raise ValueError(f"not a {kind} model")
    if int(model.get("version", 1)) > 1:
        raise ValueError(f"{kind} model version {model.get('version')} is newer than 1")


def _save(model: dict, path: str) -> None:
    with open(path, "w", encoding="utf-8") as f:
        json.dump(model, f, separators=(",", ":"))


def _loader(kind: str) -> Callable[[str], dict]:
    def load(path: str) -> dict:
        with open(path, encoding="utf-8") as f:
            m = json.load(f)
        _check(m, kind)
        return m

    return load


def _band_noise(r: np.random.Generator, n: int, lo: float, hi: float) -> np.ndarray:
    """Unit-power white noise limited to [lo, hi) Hz."""
    w = r.standard_normal(n)
    z = np.fft.rfft(w)
    f = np.fft.rfftfreq(n, 1 / SR)
    z[(f < lo) | (f >= hi)] = 0
    y = np.fft.irfft(z, n)
    p = float(np.mean(y ** 2))
    return y / math.sqrt(p) if p > 0 else y


def _noise_layer(r: np.random.Generator, n: int, edges: list, env: list, gain_db: float) -> np.ndarray:
    y = np.zeros(n)
    t = np.arange(n) / SR
    for (lo, hi), pts in zip(zip(edges[:-1], edges[1:]), env):
        p = np.asarray(pts, dtype=np.float64).reshape(-1, 2)
        if not len(p) or p[:, 1].max() <= -200:
            continue
        lv = np.interp(t, p[:, 0], p[:, 1], left=-200, right=-200) + gain_db
        y += _band_noise(r, n, lo, hi) * 10 ** (lv / 20)
    return y


def _env_points(m: np.ndarray, lo: float, hi: float, hop: int = 480, top: float | None = None) -> list:
    """[t, dB] band power per frame; levels 50 dB under top are off (-200)."""
    z = np.fft.rfft(m)
    f = np.fft.rfftfreq(len(m), 1 / SR)
    z[(f < lo) | (f >= hi)] = 0
    b = np.fft.irfft(z, len(m))
    e = _frames_db(b, hop)
    if top is not None:
        e = np.where(e < top - 50, -200.0, e)
    return [[round((k + 0.5) * hop / SR, 6), round(float(v), 2)] for k, v in enumerate(e)]


def _onsets(x: np.ndarray, max_n: int = 10) -> list:
    m = _mono(x)
    e = _frames_db(m, 48)
    if len(e) < 4:
        return [0]
    top = e.max()
    out = []
    for k in range(3, len(e)):
        if e[k] >= top - 30 and e[k] - e[k - 3] >= 10 and (not out or k * 48 - out[-1] >= 12 * 48):
            out.append(max(0, (k - 2) * 48))
    first = int(np.argmax(e >= top - 30)) * 48
    if not out or out[0] - first > 48 * 3:
        out.insert(0, first)
    return out[:max_n]


def _peaks(seg: np.ndarray, k: int = 6) -> list:
    """(Hz, amplitude) of the k strongest spectral peaks of a segment."""
    n = max(len(seg), 2048)
    w = np.hanning(len(seg))
    z = np.fft.rfft(seg * w, n)
    mag = np.abs(z) * 2 / max(w.sum(), 1e-9)
    f = np.fft.rfftfreq(n, 1 / SR)
    idx = signal.find_peaks(mag, distance=max(3, int(40 / (SR / n))))[0]
    idx = idx[(f[idx] > 40) & (f[idx] < 18000)]
    idx = idx[np.argsort(mag[idx])[::-1][:k]]
    return [(float(f[i]), float(mag[i])) for i in sorted(idx) if mag[i] > mag.max() * 1e-3]


def _tau(m: np.ndarray, f0: float) -> float:
    """Decay time constant of the band around f0, from its dB slope."""
    b, a = signal.butter(2, [f0 / 1.12, min(f0 * 1.12, 23000)], "bandpass", fs=SR)
    e = _frames_db(signal.lfilter(b, a, m), 240)
    if len(e) < 3:
        return 0.05
    pk = int(np.argmax(e))
    tail = e[pk:]
    good = tail >= tail[0] - 40
    t = np.arange(len(tail))[good] * 0.005
    if len(t) < 2:
        return 0.05
    slope = np.polyfit(t, tail[good], 1)[0]
    return float(np.clip(-8.686 / slope, 0.003, 5.0)) if slope < 0 else 5.0


def _ls_rows(seg: np.ndarray, peaks: list) -> list:
    """Rows [Hz, tau, amp, phase] with amplitudes and phases from least squares on the segment."""
    if not peaks:
        return []
    t = np.arange(len(seg)) / SR
    taus = [_tau(seg, f) for f, _ in peaks]
    cols = []
    for (f, _), tau in zip(peaks, taus):
        e = np.exp(-t / tau)
        cols += [e * np.cos(2 * np.pi * f * t), e * np.sin(2 * np.pi * f * t)]
    coef = np.linalg.lstsq(np.column_stack(cols), seg, rcond=None)[0]
    rows = []
    for i, ((f, _), tau) in enumerate(zip(peaks, taus)):
        c, d = coef[2 * i], coef[2 * i + 1]
        rows.append([f, tau, float(math.hypot(c, d)), float(math.atan2(-d, c))])
    return rows


def _modes_sum(rows: list, n: int, t0: float, rf: Callable[[int], tuple]) -> np.ndarray:
    y = np.zeros(n)
    s = int(round(t0 * SR))
    if s >= n:
        return y
    t = np.arange(n - s) / SR
    for i, (f, tau, amp, ph) in enumerate(rows):
        jf, ja = rf(i)
        y[s:] += amp * ja * np.exp(-t / tau) * np.cos(2 * np.pi * f * jf * t + ph)
    return y


# ---------------------------------------------------------------- modal stand-in


MODAL_EDGES = [20.0, 500.0, 2000.0, 8000.0, 22000.0]
MODAL_KNOBS = {
    "size": ((0.8, 1.25, "log"), 1.0, "object size"),
    "damping": ((0.7, 1.43, "log"), 1.0, "decays / damping"),
    "brightness": ((-3.0, 3.0), 0.0, "dB per octave about 1 kHz"),
    "hardness": ((0.7, 1.43, "log"), 1.0, "contact hardness"),
    "noise": ((-6.0, 4.0), 0.0, "dB on the noise bands"),
    "width": ((0.5, 1.5), 1.0, "factor on the side"),
    "room": ((0.0, 0.25), 0.0, "small room wet"),
    "vary": ((0.0, 0.6), 0.0, "seeded jitter"),
    "spacing": ((0.8, 1.25, "log"), 1.0, "time between strikes"),
}


def _modal_fit(x, passes: int = 2, **kw) -> dict:
    X = np.asarray(x, dtype=np.float64)
    m = _mono(X)
    ons = _onsets(X)
    strikes, det = [], np.zeros(len(m))
    for i, o in enumerate(ons):
        end = ons[i + 1] if i + 1 < len(ons) else len(m)
        seg = m[o:end] - det[o:end]
        rows = _ls_rows(seg, _peaks(seg[: min(len(seg), 9600)]))
        st = {"t": round(o / SR, 6), "ramp_ms": 0.5, "modes": rows, "attack": []}
        strikes.append(st)
        det += _modes_sum(rows, len(m), o / SR, lambda j: (1.0, 1.0))
    res = m - det
    top = float(_frames_db(m, 480).max()) if len(m) >= 480 else 0.0
    env = [_env_points(res, lo, hi, top=top) for lo, hi in zip(MODAL_EDGES[:-1], MODAL_EDGES[1:])]
    ch = 2 if _is_stereo(X) else 1
    return {"kind": "modal", "version": 1, "sr": SR, "length_s": round(len(m) / SR, 6), "channels": ch,
            "fade_out_s": 0.0, "strikes": strikes, "sub": None,
            "noise": {"edges": MODAL_EDGES, "h_erb": 0.4, "env": env},
            "side": [0.3] * 4 if ch == 2 else None, "coherent": True, "fit": {"modes": sum(len(s["modes"]) for s in strikes)}}


def _modal_render(model: dict, seed: int = 0, size=1.0, damping=1.0, brightness=0.0, hardness=1.0, noise=0.0,
                  width=1.0, room=0.0, vary=0.0, spacing=1.0, gain=0.0, stereo=None, ring=None) -> np.ndarray:
    _check(model, "modal")
    r = np.random.default_rng([seed & 0xFFFFFFFF, 3])
    ts = [s["t"] for s in model["strikes"]]
    tn = ts[:1]
    for i in range(1, len(ts)):
        tn.append(tn[-1] + max(0.003, (ts[i] - ts[i - 1]) * spacing))
    taus = [row[1] * size / damping for s in model["strikes"] for row in s["modes"]] or [0.01]
    n = int(round(model["length_s"] * SR))
    if ring:
        n = max(n, int((tn[-1] + 6.9 * max(taus)) * SR) if tn else n)
    y = np.zeros(n)
    for s, t0 in zip(model["strikes"], tn):
        rows = [[f / size, tau * size / damping, a * 2 ** (brightness * math.log2(max(f, 1) / 1000) / 6), ph]
                for f, tau, a, ph in s["modes"]]
        jit = r.standard_normal((max(len(rows), 1), 2)) * vary * 0.02
        y += _modes_sum(rows, n, t0, lambda j: (1 + jit[j, 0], 1 + 5 * jit[j, 1]))
    y += _noise_layer(r, n, model["noise"]["edges"], model["noise"]["env"], noise)
    y *= 10 ** (gain / 20)
    two = (model.get("channels", 1) == 2) if stereo is None else bool(stereo)
    if not two:
        return y
    side = 0.3 * width * np.roll(y, 41) if model.get("side") else np.zeros(n)
    return np.column_stack([y + side, y - side])


# ---------------------------------------------------------------- sines stand-in


SINES_KNOBS = {
    "transpose": ((-3.0, 3.0), 0.0, "semitones"),
    "stretch": ((0.85, 1.18, "log"), 1.0, "time factor about the first strike"),
    "decay": ((0.7, 1.43, "log"), 1.0, "factor on every decay"),
    "brightness": ((-3.0, 3.0), 0.0, "dB per octave about the centroid"),
    "vibrato": ((0.0, 20.0), 0.0, "cents"),
    "drift": ((0.0, 10.0), 0.0, "cents of slow wander"),
    "residual": ((0.5, 2.0, "log"), 1.0, "gain on the noise"),
    "transient": ((0.5, 2.0, "log"), 1.0, "gain on the strike noise"),
    "vary": ((0.0, 0.6), 0.0, "seeded jitter"),
    "inharm": ((-0.03, 0.03), 0.0, "partial ratio stretch"),
    "width": ((0.5, 1.5), 1.0, "factor on the fitted side"),
}
RES_BANDS = [250.0, 1000.0, 4000.0]


def _track(m: np.ndarray, f0: float, hop: int = 256, nfft: int = 2048) -> tuple[list, list]:
    f, t, z = signal.stft(m, SR, nperseg=nfft, noverlap=nfft - hop, boundary="even", padded=True)
    mag = np.abs(z) * 2
    fr, am, cur = [], [], f0
    top = mag.max()
    for k in range(mag.shape[1]):
        sel = np.nonzero((f > cur / 1.08) & (f < cur * 1.08))[0]
        if not len(sel):
            break
        i = sel[int(np.argmax(mag[sel, k]))]
        if mag[i, k] < top * 1e-3:
            if fr:
                break
            continue
        cur = float(f[i])
        fr.append([round(float(t[k]), 5), cur])
        am.append([round(float(t[k]), 5), round(float(20 * np.log10(mag[i, k] + 1e-12)), 2)])
    return fr, am


def _sines_fit(x, windows=(512, 1024, 2048, 4096), source=None, **kw) -> dict:
    X = np.asarray(x, dtype=np.float64)
    m = _mono(X)
    ons = [o / SR for o in _onsets(X)]
    parts = []
    for f0, _a in _peaks(m[: min(len(m), 4800)] if len(m) else m, 6):
        fr, am = _track(m, f0)
        if len(fr) >= 2:
            parts.append({"t0": fr[0][0], "rise": 0.0002, "t1": fr[-1][0], "rel": 0.005, "ph": 0.0,
                          "freq": fr, "amp": am})
    parts.sort(key=lambda p: -max(a for _, a in p["amp"]))
    model = {"kind": "sines", "version": 1, "sr": SR, "duration": round(len(m) / SR, 5),
             "onset": ons[0] if ons else 0.0, "onsets": ons, "peak_db": round(float(_db(np.max(m ** 2))), 2),
             "width": -120.0, "partials": parts, "transients": [], "coherent": True, "analysis": {},
             "source": source}
    det = _sines_partials(model, len(m), 1.0, 1.0, 0.0, None, False)
    res = m - det
    times = [round(k * 0.01, 4) for k in range(int(len(m) / 480) + 1)]
    db = []
    for c in RES_BANDS:
        pts = _env_points(res, c / 2, c * 2, 480, top=float(_frames_db(m, 480).max()) if len(m) >= 480 else 0.0)
        db.append([float(np.interp(t, [p[0] for p in pts] or [0], [p[1] for p in pts] or [-150])) for t in times])
    model["residual"] = {"bands": RES_BANDS, "times": times, "db": db, "hf": []}
    if _is_stereo(X):
        S = (X[:, 0] - X[:, 1]) / 2
        model["width"] = round(float(_db(np.sum(S ** 2)) - _db(np.sum(m ** 2))), 1)
        model["side"] = {"bands": [1000.0], "times": [0.0], "pan": [[0.0]], "diffuse": [[0.3]]}
    return model


def _sines_partials(model: dict, n: int, stretch: float, decay: float, transpose: float,
                    r: np.random.Generator | None, ring: bool, vary: float = 0.0) -> np.ndarray:
    y = np.zeros(n)
    t = np.arange(n) / SR
    on = float(model.get("onset", 0.0))
    tsrc = on + (t - on) / stretch
    for p in model["partials"]:
        fr, am = np.asarray(p["freq"]), np.asarray(p["amp"])
        ipk = int(np.argmax(am[:, 1]))
        tp, apk = am[ipk, 0], am[ipk, 1]
        a = np.interp(tsrc, am[:, 0], am[:, 1], left=-200, right=am[-1, 1])
        a = np.where(tsrc > tp, apk + (a - apk) / decay, a)
        if ring and len(am) >= 2:
            slope = min((am[-1, 1] - am[max(0, len(am) - 4), 1]) / max(am[-1, 0] - am[max(0, len(am) - 4), 0], 1e-3), -20)
            a = np.where(tsrc > am[-1, 0], am[-1, 1] + slope / decay * (tsrc - am[-1, 0]), a)
        else:
            a = np.where(tsrc > p["t1"] + p["rel"], -200, a)
        f = np.interp(tsrc, fr[:, 0], fr[:, 1]) * 2 ** (transpose / 12)
        if r is not None and vary:
            f = f * (1 + vary * 0.01 * r.standard_normal())
            a = a + vary * 2 * r.standard_normal()
        a = np.where(tsrc < p["t0"], -200, a)
        y += 10 ** (a / 20) * np.cos(2 * np.pi * np.cumsum(f) / SR + p["ph"])
    return y


def _sines_render(model: dict, seed: int = 0, knobs=None, transpose=0.0, stretch=1.0, decay=1.0, brightness=0.0,
                  vibrato=0.0, drift=0.0, residual=1.0, transient=1.0, vary=0.0, inharm=0.0, width=1.0,
                  gain_db=0.0, stereo=False, ring=False) -> np.ndarray:
    _check(model, "sines")
    r = np.random.default_rng([seed & 0xFFFFFFFF, 5])
    n = int(round(model["duration"] * stretch * SR)) + (int(0.3 * SR) if ring else 0)
    y = _sines_partials(model, n, stretch, decay, transpose, r, ring, vary)
    res = model["residual"]
    edges = []
    env = []
    for c, lv in zip(res["bands"], res["db"]):
        edges.append((c / 2, c * 2))
        env.append([[t * stretch, v] for t, v in zip(res["times"], lv)])
    for (lo, hi), pts in zip(edges, env):
        y += _noise_layer(r, n, [lo, hi], [pts], 20 * math.log10(residual))
    y *= 10 ** (gain_db / 20)
    if not stereo:
        return y
    side = 0.3 * width * np.roll(y, 37) if model.get("side") else np.zeros(n)
    return np.column_stack([y + side, y - side])


# ---------------------------------------------------------------- spectral stand-in


SPECTRAL_KNOBS = {
    "length": ((0.8, 1.25, "log"), 1.0, "time-stretch"),
    "speed": ((0.8, 1.25, "log"), 1.0, "shorter and higher above 1"),
    "sweep": ((0.7, 1.43, "log"), 1.0, "centroid excursion"),
    "shift": ((-0.4, 0.4), 0.0, "octaves"),
    "tilt": ((-2.5, 2.5), 0.0, "dB per octave about 1 kHz"),
    "roughness": ((0.5, 1.5), 1.0, "texture depth"),
    "width": ((0.5, 1.5), 1.0, "side and pan amount"),
    "ridges": ((0.5, 1.5), 1.0, "tonal ridge gain"),
}
SPEC_EDGES = np.geomspace(40, 20000, 13)
NPERSEG, HOP = 1024, 480


def _stft(x: np.ndarray):
    return signal.stft(x, SR, nperseg=NPERSEG, noverlap=NPERSEG - HOP)


def _band_db(z: np.ndarray, f: np.ndarray) -> np.ndarray:
    p = np.abs(z) ** 2
    out = np.full((len(SPEC_EDGES) - 1, p.shape[1]), -200.0)
    for b, (lo, hi) in enumerate(zip(SPEC_EDGES[:-1], SPEC_EDGES[1:])):
        sel = (f >= lo) & (f < hi)
        out[b] = _db(p[sel].mean(axis=0))
    return out


def _spectral_fit(x, name=None, texture=True, iters=3, seeds=(1, 2), **kw) -> dict:
    X = np.asarray(x, dtype=np.float64)
    m = _mono(X)
    f, t, z = _stft(m)
    db = _band_db(z, f)
    stereo = None
    if _is_stereo(X):
        icc = []
        _, _, zl = _stft(X[:, 0])
        _, _, zr = _stft(X[:, 1])
        for k in range(zl.shape[1]):
            a, b = zl[:, k], zr[:, k]
            den = math.sqrt(float(np.sum(np.abs(a) ** 2) * np.sum(np.abs(b) ** 2))) or 1.0
            icc.append([round(float(np.real(np.sum(a * np.conj(b)))) / den, 3)])
        stereo = {"groups": [0, 24000], "ild": [[0.0] for _ in icc], "icc": icc}
    return {"kind": "spectral_noise", "version": 1, "source": name, "sr": SR, "dur": round(len(m) / SR, 4),
            "level_db": 0.0, "bands": {"n": len(SPEC_EDGES) - 1, "f_lo": 40, "f_hi": 20000, "scale": "geom"},
            "fit": {}, "env": {"t": [round(float(v), 4) for v in t], "db": np.round(db.T, 2).tolist()},
            "stereo": stereo, "fine": None, "texture": None, "ridges": [], "finish": True,
            "channels": 2 if stereo else 1}


def _spectral_noise(model: dict, r: np.random.Generator, n: int, scale: float, shift: float, tilt: float) -> np.ndarray:
    w = r.standard_normal(n + NPERSEG)
    f, t, z = _stft(w)
    c0 = np.sum(np.hanning(NPERSEG) ** 2) / np.sum(np.hanning(NPERSEG)) ** 2
    env_t = np.asarray(model["env"]["t"]) * scale
    env = np.asarray(model["env"]["db"]).T
    cen = np.sqrt(SPEC_EDGES[:-1] * SPEC_EDGES[1:]) * 2 ** shift
    lvl = np.array([np.interp(t, env_t, env[b], left=-200, right=-200) for b in range(env.shape[0])])
    per_bin = np.array([np.interp(np.log2(np.maximum(f, 1)), np.log2(cen), lvl[:, k]) for k in range(len(t))]).T
    per_bin += tilt * np.log2(np.maximum(f, 1) / 1000)[:, None]
    per_bin[f < 30] = -200
    _, y = signal.istft(z * np.sqrt(10 ** (per_bin / 10) / c0), SR, nperseg=NPERSEG, noverlap=NPERSEG - HOP)
    return np.pad(y, (0, max(0, n - len(y))))[:n]


def _spectral_render(model: dict, seed: int = 0, length=1.0, speed=1.0, sweep=1.0, shift=0.0, tilt=0.0,
                     roughness=1.0, width=1.0, ridges=1.0, gain_db=0.0, doppler=0.0, proximity=0.0, pan=1.0,
                     motion=0.0, level=True, limit=True, steady=True) -> np.ndarray:
    _check(model, "spectral_noise")
    scale = length / speed
    n = max(64, int(round(model["dur"] * scale * SR)))
    r = np.random.default_rng([seed & 0xFFFFFFFF, 9])
    a = _spectral_noise(model, r, n, scale, shift + math.log2(speed), tilt)
    if not model.get("stereo"):
        return (np.column_stack([a, a]) * 10 ** (gain_db / 20)).astype(np.float32)
    b = _spectral_noise(model, r, n, scale, shift + math.log2(speed), tilt)
    icc = float(np.clip(np.mean(model["stereo"]["icc"]), -1, 1))
    c = 1 - (1 - icc) * width
    right = c * a + math.sqrt(max(0.0, 1 - c * c)) * b
    return (np.column_stack([a, right]) * 10 ** (gain_db / 20)).astype(np.float32)


# ---------------------------------------------------------------- gesture stand-in


GESTURE_KNOBS = {
    "length": ((0.8, 1.25, "log"), 1.0, "time-stretch"),
    "sweep": ((0.7, 1.43, "log"), 1.0, "air centroid excursion"),
    "doppler": ((-2.0, 2.0), 0.0, "doppler bend, semitones"),
    "proximity": ((-0.5, 1.0), 0.0, "softer keyframes darker"),
    "pan": ((0.5, 1.5), 1.0, "factor on L/R level differences"),
    "motion": ((-0.6, 0.6), 0.0, "stereo motion"),
    "width": ((0.5, 1.5), 1.0, "tones' side and air width"),
    "roughness": ((0.5, 1.5), 1.0, "air texture depth"),
    "tone_db": ((-6.0, 6.0), 0.0, "level of the tones"),
    "air_db": ((-6.0, 6.0), 0.0, "level of the air"),
    "vary": ((0.0, 0.4), 0.0, "seeded jitter of the tones"),
}


def _quiet_tones(t: dict) -> dict:
    t = copy.deepcopy(t)
    for k in ("post", "fit_engine"):
        t.pop(k, None)
    if t["kind"] == "sines":
        res = t["residual"]
        t["residual"] = dict(res, bands=res["bands"][:1], db=[[-150.0] * len(res["times"])])
    else:
        t["noise"]["env"] = [[[p[0], -240.0] for p in band] for band in t["noise"]["env"]]
        t["onset"] = t["strikes"][0]["t"] if t["strikes"] else 0.0
    t["layer"] = "deterministic"
    t.setdefault("onset", 0.0)
    return t


def _gesture_fit(x, name=None, tones=None, fast=False, tones_kind="sines", **kw) -> dict:
    X = np.asarray(x, dtype=np.float64)
    m = _mono(X)
    ch = 2 if _is_stereo(X) else 1
    if tones is None:
        tones = _sines_fit(X)
    tones = _quiet_tones(tones) if tones else None
    rest = _two(X) if ch == 2 else m
    share = 0.0
    if tones:
        y = _SINES.render(tones) if tones["kind"] == "sines" else _MODAL.render(tones)
        y = np.pad(y, (0, max(0, len(m) - len(y))))[: len(m)]
        share = float(np.sum(y ** 2) / max(np.sum(m ** 2), 1e-20))
        if share < 0.05:
            tones = None
        else:
            rest = rest - (y[:, None] if ch == 2 else y)
    air = _spectral_fit(rest, name=name)
    return {"kind": "gesture", "version": 1, "sr": SR, "dur": round(len(m) / SR, 6), "channels": ch,
            "source": name, "tones": tones, "air": air, "traj": {}, "fit": {"tone_share": round(share, 4)}}


def _gesture_render(model: dict, seed: int = 0, length=1.0, sweep=1.0, doppler=0.0, proximity=0.0, pan=1.0,
                    motion=0.0, width=1.0, roughness=1.0, tone_db=0.0, air_db=0.0, vary=0.0, gain_db=0.0,
                    stereo=None, ring=False) -> np.ndarray:
    _check(model, "gesture")
    two = model["channels"] == 2 if stereo is None else bool(stereo)
    air = _spectral_render(model["air"], (seed * 7 + 101) & 0x7FFFFFFF, length=length, width=width,
                           gain_db=air_db).astype(np.float64)
    parts = [air]
    t = model.get("tones")
    if t:
        if t["kind"] == "sines":
            y = _sines_render(t, seed, stretch=length, vary=vary, gain_db=tone_db, ring=ring)
        else:
            y = _modal_render(t, seed, vary=vary, gain=tone_db, ring=ring)
        parts.append(_two(y))
    n = max(len(p) for p in parts) if ring else max(64, int(round(model["dur"] * length * SR)))
    out = np.zeros((n, 2))
    for p in parts:
        k = min(n, len(p))
        out[:k] += p[:k]
    out *= 10 ** (gain_db / 20)
    return out if two else out.mean(axis=1)


# ---------------------------------------------------------------- space stand-in


OCT = [125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0]


def _env1ms(m: np.ndarray) -> np.ndarray:
    return _frames_db(m, 48)


def _active_span(x) -> tuple:
    m = _mono(x)
    e = _env1ms(m)
    if len(e) < 3:
        return (0, 0, len(m))
    pk = int(np.argmax(e))
    on = int(np.argmax(e >= e[pk] - 30))
    above = np.nonzero(e >= e[pk] - 40)[0]
    last = int(above[-1]) if len(above) else pk
    return (on * 48, pk * 48, min(len(m), last * 48 + 48))


def _side_powers(x, centers=OCT, nfft=1024, hop=240) -> dict:
    X = _two(x)
    M, S = (X[:, 0] + X[:, 1]) / 2, (X[:, 0] - X[:, 1]) / 2
    f, t, zm = signal.stft(M, SR, nperseg=nfft, noverlap=nfft - hop)
    _, _, zs = signal.stft(S, SR, nperseg=nfft, noverlap=nfft - hop)
    edges = [c / math.sqrt(2) for c in centers] + [centers[-1] * math.sqrt(2)]
    PM, PS, C = [], [], []
    for lo, hi in zip(edges[:-1], edges[1:]):
        sel = (f >= lo) & (f < hi)
        PM.append(np.sum(np.abs(zm[sel]) ** 2, axis=0))
        PS.append(np.sum(np.abs(zs[sel]) ** 2, axis=0))
        C.append(np.sum(np.real(zs[sel] * np.conj(zm[sel])), axis=0))
    return {"t": t, "PM": np.array(PM), "PS": np.array(PS), "C": np.array(C)}


def _tail_span(x, start_s=0.03, min_s=0.02):
    on, pk, end = _active_span(x)
    first = pk + int(start_s * SR)
    return (first, end) if end - first >= min_s * SR else None


def _corr(L, R):
    el, er = float(np.sum(np.square(L))), float(np.sum(np.square(R)))
    if el < 1e-20 or er < 1e-20:
        return None
    return float(np.sum(L * R) / math.sqrt(el * er))


def _tail_corr(x, span=None):
    X = _two(x)
    span = span or _tail_span(X)
    return None if span is None else _corr(X[span[0]: span[1], 0], X[span[0]: span[1], 1])


def _needs_space(x, max_corr=0.7) -> bool:
    if not _is_stereo(x):
        return False
    tc = _tail_corr(x)
    return tc is not None and tc <= max_corr


def _fit_space(x, sr=SR, dry=None, max_corr=0.7, **kw):
    X = _two(x)
    span = _tail_span(X)
    if span is None or dry is None:
        return None
    S = (X[:, 0] - X[:, 1]) / 2
    d = _mono(dry)
    a, b = span[0], min(span[1], len(d))
    if b <= a:
        return None
    g = math.sqrt(float(np.sum(S[a:b] ** 2)) / max(float(np.sum(d[a:b] ** 2)), 1e-20))
    return {"kind": "space", "version": 1, "gain": round(g, 4), "tail_corr": round(_tail_corr(X) or 0, 4)}


def _render_space(model, dry, seed=0, width=1.0, ring=False, limit=True) -> np.ndarray:
    D = _two(dry).copy()
    if not model:
        return D
    if ring:
        D = np.pad(D, ((0, int(0.05 * SR)), (0, 0)))
    r = np.random.default_rng([(seed * 7919 + 4099) & 0xFFFFFFFF])
    k = int(0.05 * SR)
    ir = r.standard_normal(k) * np.exp(-np.arange(k) / (0.01 * SR))
    ir /= math.sqrt(float(np.sum(ir ** 2)))
    side = signal.fftconvolve(D.mean(axis=1), ir)[: len(D)] * float(model["gain"]) * width
    D[:, 0] += side
    D[:, 1] -= side
    return D


# ---------------------------------------------------------------- forge EQ, core, models


def _eq_bands(x, gains_db, phase="min", taps=4096):
    cen = 1000.0 * 2.0 ** (np.arange(-15, 13) / 3.0)
    nf = 4 * taps
    f = np.fft.rfftfreq(nf, 1 / SR)
    g = np.interp(np.log2(np.maximum(f, 1)), np.log2(cen), np.asarray(gains_db, dtype=np.float64))
    H = 10 ** (g / 20)
    full = np.concatenate([H, H[-2:0:-1]])
    c = np.real(np.fft.ifft(np.log(np.maximum(full, 1e-12))))
    fold = np.zeros(nf)
    fold[0] = c[0]
    fold[1: nf // 2] = 2 * c[1: nf // 2]
    fold[nf // 2] = c[nf // 2]
    h = np.real(np.fft.ifft(np.exp(np.fft.fft(fold))))[:taps]
    win = np.ones(taps)
    win[taps // 2:] = np.hanning(taps)[taps // 2:]
    x = np.asarray(x, dtype=np.float64)
    return signal.fftconvolve(x, h * win)[: len(x)]


class _MLMissing(RuntimeError):
    def __init__(self, feature: str) -> None:
        super().__init__(f"{feature} needs the ml extras (torch, transformers)")
        self.feature, self.missing, self.command = feature, ("torch", "transformers"), "pip install torch"


def _ml_ok() -> bool:
    return all(importlib.util.find_spec(m) is not None for m in ("torch", "transformers"))


def _require_ml(feature: str) -> None:
    if not _ml_ok():
        raise _MLMissing(feature)


def _audio_load(path, sr=SR, mono=False, start=None, end=None, cache=True):
    import soundfile

    y, rate = soundfile.read(str(path), dtype="float32", always_2d=True)
    if rate != SR:
        raise RuntimeError("the stand-in loader reads 48 kHz files only")
    y = y.mean(axis=1) if mono else (np.column_stack([y[:, 0], y[:, 0]]) if y.shape[1] == 1 else y[:, :2])
    return y.astype(np.float32)


def _add_common(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--out")
    parser.add_argument("--threads", type=int)


def _emit(result: dict, args, text=None) -> dict:
    if getattr(args, "json", False):
        print(json.dumps(result, default=lambda o: o.tolist() if hasattr(o, "tolist") else str(o)))
    else:
        print(text or result.get("summary", ""))
    return result


def _partial_freq(p: dict) -> float:
    fr, am = np.asarray(p["freq"]), np.asarray(p["amp"])
    e = 10 ** (np.interp(fr[:, 0], am[:, 0], am[:, 1]) / 10)
    return float(np.sum(fr[:, 1] * e) / max(float(np.sum(e)), 1e-30))


def _morph_side(sa, sb, w, map_a=None, map_b=None):
    if not sa and not sb:
        return None
    base = copy.deepcopy(sa or sb)
    for key in ("pan", "diffuse"):
        va = np.asarray((sa or {}).get(key) or np.zeros_like(base[key]), dtype=np.float64)
        vb = np.asarray((sb or {}).get(key) or np.zeros_like(base[key]), dtype=np.float64)
        base[key] = ((1 - w) * va + w * vb).tolist()
    return base


def _gain_tones(det: dict, gain_db: float) -> dict:
    t = copy.deepcopy(det)
    g = 10 ** (gain_db / 20)
    if t["kind"] == "sines":
        for p in t["partials"]:
            p["amp"] = [[a, b + gain_db] for a, b in p["amp"]]
    else:
        for st in t["strikes"]:
            st["modes"] = [[f, tau, amp * g, ph] for f, tau, amp, ph in st["modes"]]
    return t


# ---------------------------------------------------------------- module objects


def _module(name: str, **attrs) -> types.ModuleType:
    mod = types.ModuleType(name)
    mod.__dict__.update(attrs)
    return mod


def _kind_module(name: str, kind: str, short: str, knobs: dict, fit, render, **extra) -> types.ModuleType:
    def summary(model: dict) -> str:
        return f"{short} stand-in model, {len(json.dumps(model)) // 1024} kB"

    return _module(name, KIND=kind, NAME=short, VERSION=1, KNOBS=knobs, fit=fit, render=render,
                   check=lambda m: _check(m, kind), summary=summary, save=_save, load=_loader(kind), **extra)


_MODAL = _kind_module("hark.fit.analysis.modal", "modal", "modal", MODAL_KNOBS, _modal_fit, _modal_render,
                      onsets=lambda x, **kw: _onsets(x))
_SINES = _kind_module("hark.fit.analysis.sinusoidal", "sines", "sines", SINES_KNOBS, _sines_fit, _sines_render,
                      onsets=lambda x, sr=SR: [{"t": o / SR, "rise": 0.001, "strength": 1.0} for o in _onsets(x)],
                      partial_freq=_partial_freq, morph_side=_morph_side)
_SPECTRAL = _kind_module("hark.fit.analysis.spectral", "spectral_noise", "spectral", SPECTRAL_KNOBS, _spectral_fit,
                         _spectral_render, trajectory=lambda m: {})
_GESTURE = _kind_module("hark.fit.analysis.gesture", "gesture", "gesture", GESTURE_KNOBS, _gesture_fit,
                        _gesture_render, gain_tones=_gain_tones)
_SPACE = _module("hark.fit.analysis.space", active_span=_active_span, side_powers=_side_powers,
                 tail_span=_tail_span, corr=_corr, tail_corr=_tail_corr, needs_space=_needs_space,
                 is_stereo=_is_stereo, fit_space=_fit_space, render_space=_render_space)
_KIND_MODULES = {"sines": _SINES, "modal": _MODAL, "spectral_noise": _SPECTRAL, "gesture": _GESTURE}


def _module_for(kind: str) -> types.ModuleType:
    if kind not in _KIND_MODULES:
        raise ValueError(f"unknown fit kind {kind!r}")
    return _KIND_MODULES[kind]


def _standin_table() -> dict:
    return {
        "hark.core": _module("hark.core", __path__=[]),
        "hark.core.audio": _module("hark.core.audio", SR=SR, load=_audio_load, to_mono=_mono, Audio=np.ndarray),
        "hark.core.report": _module("hark.core.report", add_common=_add_common, emit=_emit),
        "hark.models": _module("hark.models", MLMissing=_MLMissing, require_ml=_require_ml, ml_available=_ml_ok),
        "hark.forge": _module("hark.forge", __path__=[]),
        "hark.forge.engine": _module("hark.forge.engine", __path__=[]),
        "hark.forge.engine.fx": _module("hark.forge.engine.fx", eq_bands=_eq_bands),
        "hark.fit.analysis": _module("hark.fit.analysis", __path__=[], module_for=_module_for,
                                     KINDS={k: m.__name__ for k, m in _KIND_MODULES.items()}),
        "hark.fit.analysis.modal": _MODAL,
        "hark.fit.analysis.sinusoidal": _SINES,
        "hark.fit.analysis.spectral": _SPECTRAL,
        "hark.fit.analysis.gesture": _GESTURE,
        "hark.fit.analysis.space": _SPACE,
    }


def _absent(name: str) -> bool:
    try:
        return importlib.util.find_spec(name) is None
    except (ImportError, ValueError):
        return True


def install_standins() -> list:
    """Install the stand-ins for absent modules for good (used by child processes of the tests)."""
    installed = []
    for name, mod in _standin_table().items():
        if name not in sys.modules and _absent(name):
            mod._hark_standin = True
            sys.modules[name] = mod
            parent, _, leaf = name.rpartition(".")
            if parent in sys.modules:
                setattr(sys.modules[parent], leaf, mod)
            installed.append(name)
    return installed


@pytest.fixture(autouse=True)
def standins(monkeypatch: pytest.MonkeyPatch) -> list:
    """Install each stand-in whose real module is absent; return the installed names."""
    installed = []
    for name, mod in _standin_table().items():
        if name in sys.modules and not getattr(sys.modules[name], "_hark_standin", False):
            continue
        if name not in sys.modules and not _absent(name):
            continue
        mod._hark_standin = True
        monkeypatch.setitem(sys.modules, name, mod)
        parent, _, leaf = name.rpartition(".")
        if parent in sys.modules:
            monkeypatch.setattr(sys.modules[parent], leaf, mod, raising=False)
        installed.append(name)
    return installed


@pytest.fixture
def analysis_is_real(standins: list) -> bool:
    """True when the real fit analysis modules (unit 20a) are present."""
    return "hark.fit.analysis.modal" not in standins


# ---------------------------------------------------------------- synthetic recordings (fit.md S1 .. S5)


def _s1(rng: np.random.Generator) -> np.ndarray:
    n = int(0.8 * SR)
    t = np.arange(n) / SR
    y = sum(a * np.exp(-t / tau) * np.sin(2 * np.pi * f * t)
            for f, tau, a in ((440, 0.4, 0.5), (1130, 0.2, 0.3), (2390, 0.1, 0.2)))
    k = int(0.002 * SR)
    y[:k] += 0.3 * rng.standard_normal(k) * np.linspace(1, 0, k)
    return y


def _s2() -> np.ndarray:
    n = int(0.2 * SR)
    t = np.arange(n) / SR
    f = np.where(t < 0.06, 900 * (500 / 900) ** (t / 0.06), 500.0)
    env = np.minimum(t / 0.003, 1) * np.exp(-np.maximum(t - 0.003, 0) / 0.04)
    return 0.6 * env * np.sin(2 * np.pi * np.cumsum(f) / SR)


def _s3(rng: np.random.Generator) -> np.ndarray:
    n = int(0.6 * SR)
    t = np.arange(n) / SR
    env = 0.5 - 0.5 * np.cos(2 * np.pi * t / t[-1])
    out = []
    shared = rng.standard_normal(n)
    for _ in range(2):
        w = 0.6 * shared + 0.8 * rng.standard_normal(n)
        y = np.zeros(n)
        for i in range(0, n, 2400):
            fc = 500 * 8 ** (i / n)
            b, a = signal.butter(2, [fc / 1.5, min(fc * 1.5, 23000)], "bandpass", fs=SR)
            seg = signal.lfilter(b, a, w[max(0, i - 2400): i + 2400])
            y[i: i + 2400] = seg[-len(y[i: i + 2400]):] if i else seg[: len(y[i: i + 2400])]
        out.append(0.4 * env * y / (np.std(y) + 1e-12))
    return np.column_stack(out)


def _s4() -> np.ndarray:
    n = int(0.33 * SR)
    t = np.arange(n) / SR
    y = np.zeros(n)
    for i in range(4):
        s, e = int(i * 0.08 * SR), int((i * 0.08 + 0.03) * SR)
        tt = t[: e - s]
        y[s:e] = 0.5 * np.exp(-tt / 0.4) * np.sin(2 * np.pi * 440 * tt)
    return y


# fit.md places S1 at 200 ms, but bed detection only finds an onset under 120 ms (fit-auto.md); see the report.
BED_LEAD_S = 0.1


def _s5(rng: np.random.Generator) -> np.ndarray:
    s1 = _s1(rng)
    lead = int(BED_LEAD_S * SR)
    n = lead + len(s1)
    w = np.fft.rfft(rng.standard_normal(n))
    f = np.fft.rfftfreq(n, 1 / SR)
    pink = np.fft.irfft(w / np.sqrt(np.maximum(f, 20)), n)
    c = np.concatenate([[0.0], np.cumsum(s1 ** 2)])
    loud = math.sqrt(float(np.max(c[2400:] - c[:-2400])) / 2400)
    pink *= loud * 10 ** (-25 / 20) / np.sqrt(np.mean(pink ** 2))
    y = pink.copy()
    y[lead:] += s1
    return y


@pytest.fixture
def sounds() -> dict:
    """The synthetic recordings S1 .. S5 of fit.md, from fixed generator seeds."""
    return {"S1": _s1(np.random.default_rng(1)), "S2": _s2(), "S3": _s3(np.random.default_rng(3)), "S4": _s4(),
            "S5": _s5(np.random.default_rng(5))}


@pytest.fixture
def write_wav(tmp_path) -> Callable[[str, np.ndarray], str]:
    """write_wav(name, x) writes a 48 kHz float WAV under tmp_path and returns its path."""
    import soundfile

    def write(name: str, x: np.ndarray) -> str:
        p = tmp_path / name
        p.parent.mkdir(parents=True, exist_ok=True)
        soundfile.write(str(p), np.asarray(x, dtype=np.float32), SR, subtype="FLOAT")
        return str(p)

    return write


@pytest.fixture
def bed_lead() -> float:
    """Seconds of bed before S1 in S5."""
    return BED_LEAD_S


@pytest.fixture
def child_prelude() -> str:
    """Python source that a child process runs first: it installs the same stand-ins."""
    here = __file__
    return ("import importlib.util, sys\n"
            f"_s = importlib.util.spec_from_file_location('_fit_pipeline_conftest', {here!r})\n"
            "_m = importlib.util.module_from_spec(_s)\n_s.loader.exec_module(_m)\n_m.install_standins()\n")
