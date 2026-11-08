"""Fitting the post stage: EQ, level, attack clamp, envelope calibration and gate."""

from __future__ import annotations

from typing import Any, Callable

import numpy as np
from scipy import ndimage, signal

from hark.fit.pipeline import quality as Q
from hark.fit.pipeline._post import ATTACK_FRAME_S, ATTACK_HOP, apply_post, frame_db, mono, smooth121

SR = 48000
N_BANDS = 28
GATE_HOP = 24            # 0.5 ms
GATE_WIN = 48            # 1 ms
NOISE_HP_HZ = 200

Judge = Callable[[list], tuple]


# ---------------------------------------------------------------- scoring


def _scorer(X: np.ndarray, renders: list, judge: Judge | None) -> Callable[[dict], tuple]:
    """sc(p): (mean score, mean CLAP) of the renders through post stage p."""
    xm = mono(X)

    def sc(p: dict) -> tuple:
        ys = [apply_post(y, p) for y in renders]
        if judge is not None:
            return judge(ys)
        return float(np.mean([Q.score(Q.quality(xm, mono(y))) for y in ys])), 0.0

    return sc


# ---------------------------------------------------------------- EQ and gain


def _profile_power(y: np.ndarray) -> np.ndarray:
    return 10 ** (Q.profile(mono(y)) / 10)


def _eq_step(target: np.ndarray, ys: list) -> np.ndarray:
    cur = 10 * np.log10(np.mean([_profile_power(y) for y in ys], axis=0))
    d = target - cur
    w = 10 ** (target / 10)
    d = d - float(np.sum(w * d) / np.sum(w))
    d = np.where(cur < -50, np.minimum(d, 6.0), d)
    return smooth121(d)


def fit_eq(X: np.ndarray, renders: list, iters: int = 2, max_db: float = 12) -> list:
    """28 EQ gains that move the renders' tone balance to the recording's."""
    target = Q.profile(mono(X))
    total = np.zeros(N_BANDS)
    for _ in range(iters):
        ys = [apply_post(y, {"eq_db": total.tolist(), "gain_db": 0.0}) for y in renders]
        total = np.clip(total + _eq_step(target, ys), -max_db, max_db)
    return np.round(total, 2).tolist()


def level_gain(X: np.ndarray, renders: list, p: dict) -> float:
    """The gain that matches the renders' loudest 50 ms to the recording's."""
    lv = np.mean([Q.peak_rms_db(apply_post(y, p)) for y in renders])
    return float(p.get("gain_db", 0.0) + Q.peak_rms_db(X) - lv)


# ---------------------------------------------------------------- attack clamp


def attack_pre(X: np.ndarray, noise: bool = False) -> dict | None:
    """The attack clamp entry of a recording, or None."""
    m = mono(X)
    e = frame_db(m, ATTACK_HOP)
    if len(e) <= 4:
        return None
    on = int(np.argmax(e >= e.max() - 20))
    pk = on + int(np.argmax(e[on: on + 101]))
    alen = max(24, pk - on + 16)
    struck = Q.attack(m)[1] >= 15
    if not (on >= 2 or (struck and len(e) - on > 4)):
        return None
    pre: dict[str, Any] = {"t_on": round(on * ATTACK_FRAME_S, 6), "db": np.round(e[:on], 2).tolist(),
                           "attack_db": np.round(e[on: on + alen], 2).tolist() if struck else [], "v": 2}
    if noise:
        pre["hp_hz"] = NOISE_HP_HZ
    return pre


def _with_clamp(X: np.ndarray, renders: list, p: dict, noise: bool) -> dict | None:
    pre = attack_pre(X, noise)
    if pre is None:
        return None
    q = dict(p, pre=pre)
    q["gain_db"] = level_gain(X, renders, q)
    return q


# ---------------------------------------------------------------- envelope calibration


def _env_curve(X: np.ndarray, renders: list, p: dict) -> dict | None:
    e0 = frame_db(mono(X), 240)
    es = [frame_db(mono(apply_post(y, p)), 240, len(e0)) for y in renders if len(y) >= len(e0) * 240]
    if not es or len(e0) < 3:
        return None
    er = np.mean(es, axis=0)
    top = max(e0.max(), er.max())
    g = np.clip(e0 - er, -12, 6)
    g[(e0 < top - 70) & (er < top - 70)] = 0
    start = int(np.ceil((float((p.get("pre") or {}).get("t_on", 0)) + 0.008) / 0.005))
    g[: min(start, len(g))] = 0
    g = smooth121(smooth121(g))
    g[:2] *= np.array([0.33, 0.67])[: len(g[:2])]
    return {"hop": 0.005, "db": np.round(g, 2).tolist()}


# ---------------------------------------------------------------- gate


def _gate_env(m: np.ndarray) -> np.ndarray:
    k = (len(m) - GATE_WIN) // GATE_HOP + 1 if len(m) >= GATE_WIN else 0
    if k <= 0:
        return np.zeros(0)
    idx = np.arange(GATE_WIN)[None, :] + GATE_HOP * np.arange(k)[:, None]
    return 10 * np.log10(np.maximum(np.mean(m[idx] ** 2, axis=1), 1e-20))


def _render_env(renders: list, n: int) -> np.ndarray:
    ps = []
    for y in renders:
        e = _gate_env(mono(y))[:n]
        ps.append(np.pad(10 ** (e / 10), (0, n - len(e)), constant_values=1e-20))
    return 10 * np.log10(np.mean(ps, axis=0))


def _dips(e0: np.ndarray) -> np.ndarray:
    sm = ndimage.uniform_filter1d(e0, 10, mode="nearest")
    peaks, _ = signal.find_peaks(sm, prominence=6, distance=16, height=sm.max() - 25)
    dip = np.zeros(len(e0), dtype=bool)
    for a, b in zip(peaks[:-1], peaks[1:]):
        low = min(sm[a], sm[b])
        seg = np.arange(a, b + 1)
        dip[seg] = sm[seg] < low - 6
    return dip


def _fell(e0: np.ndarray) -> np.ndarray:
    run_max = ndimage.maximum_filter1d(e0, 41, origin=20, mode="nearest")
    return (e0 <= run_max - 12) | (e0 <= e0.max() - 40)


def _raw_cut(e0: np.ndarray, er: np.ndarray, start: int) -> np.ndarray:
    d = e0 - er
    hit = ((d < -4) & _fell(e0)) | ((d < -2) & _dips(e0))
    top = max(e0.max(), er.max())
    cut = np.where(hit & (er > top - 90), np.maximum(d + 1, -80), 0.0)
    cut = np.minimum(cut, 0.0)
    cut[:start] = 0.0
    edit = max(start, len(cut) - 24)
    if edit < len(cut):
        cut[edit:] = cut[edit - 1] if edit > start else 0.0
    return cut


def _runs(neg: np.ndarray) -> list:
    out, i = [], 0
    while i < len(neg):
        if neg[i]:
            j = i
            while j < len(neg) and neg[j]:
                j += 1
            out.append((i, j))
            i = j
        else:
            i += 1
    return out


def _clean_runs(cut: np.ndarray) -> np.ndarray:
    out = np.zeros_like(cut)
    n = len(cut)
    for a, b in _runs(cut < 0):
        seg = cut[a:b]
        at_end = b == n
        if seg.min() > -6 or (b - a < 6 and not at_end):
            continue
        if b - a >= 5:
            seg = smooth121(ndimage.median_filter(seg, 5, mode="nearest"))
        seg = np.minimum(seg, 0.0)
        if not at_end:
            seg[-1] = 0.0
        out[a:b] = seg
    return out


def _rdp(pts: np.ndarray, tol: float) -> np.ndarray:
    from hark.fit.pipeline._morph import rdp

    return np.asarray(rdp(pts.tolist(), tol))


def fit_gate(X: np.ndarray, renders: list, pre: dict | None = None) -> dict | None:
    """A gain curve that cuts where the renders ring on after the recording fell, or None."""
    e0 = _gate_env(mono(X))
    if len(e0) < 8 or not renders:
        return None
    er = _render_env(renders, len(e0))
    e0, er = ndimage.median_filter(e0, 3, mode="nearest"), ndimage.median_filter(er, 3, mode="nearest")
    if pre:
        start = int(round(float(pre["t_on"]) / 0.0005))
    else:
        start = int(np.argmax(e0 >= e0.max() - 20))
    cut = _clean_runs(_raw_cut(e0, er, start))
    if not np.any(cut < -0.5):
        return None
    t = (np.arange(len(cut)) + 1) * 0.0005
    pts = _rdp(np.column_stack([t, cut]), 0.5)
    return {"pts": [[round(float(a), 5), round(float(b), 1)] for a, b in pts]}


# ---------------------------------------------------------------- the post stage


def _try_gate(X: np.ndarray, renders: list, p: dict, sc: Callable, base: tuple) -> dict:
    gate = fit_gate(X, [apply_post(y, p) for y in renders], p.get("pre"))
    if gate is None:
        return p
    q = dict(p, gate=gate)
    q["gain_db"] = level_gain(X, renders, q)
    s, c = sc(q)
    return q if s < base[0] and c >= base[1] - 0.005 else p


def fit_post(X: np.ndarray, renders: list, iters: int = 2, max_db: float = 12, envelope: bool = False,
             gate: bool = True, judge: Judge | None = None, sharp: bool = True, noise: bool = False) -> dict:
    """A post stage fitted from renders (not finished) against the recording X."""
    X = np.asarray(X, dtype=np.float64)
    renders = [np.asarray(y, dtype=np.float64) for y in renders]
    sc = _scorer(X, renders, judge)
    p: dict[str, Any] = {"eq_db": fit_eq(X, renders, iters, max_db), "gain_db": 0.0}
    p["gain_db"] = round(level_gain(X, renders, p), 3)
    if sharp:
        q = _with_clamp(X, renders, p, noise)
        if q is not None and (not noise or sc(q)[0] < sc(p)[0]):
            p = q
    if envelope:
        env = _env_curve(X, renders, p)
        if env is not None:
            p = dict(p, env=env)
            p["eq_db"] = fit_eq(X, [apply_post(y, {"env": env}) for y in renders], 1, max_db)
            p["gain_db"] = level_gain(X, renders, p)
    if gate:
        p = _try_gate(X, renders, p, sc, sc(p))
    return p
