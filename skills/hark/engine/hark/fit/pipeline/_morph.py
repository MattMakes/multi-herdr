"""Morphs between 2 fits of one kind: pairing, time maps and the per-kind mixes."""

from __future__ import annotations

import copy
import math
from typing import Any, Callable

import numpy as np
from scipy.optimize import linear_sum_assignment

from hark.fit.pipeline import _model
from hark.fit.pipeline._model import kind_of, strip

TMap = Callable[[Any], Any]


# ---------------------------------------------------------------- shared tools


def geo(a: float, b: float, w: float) -> float:
    return float(math.exp((1 - w) * math.log(max(a, 1e-300)) + w * math.log(max(b, 1e-300))))


def lin(a: float, b: float, w: float) -> float:
    return float((1 - w) * a + w * b)


def circ(pa: float, pb: float, w: float) -> float:
    return float(np.angle((1 - w) * np.exp(1j * pa) + w * np.exp(1j * pb)))


def wdb(w: float) -> float:
    return 20 * math.log10(w) if w > 0 else -300.0


def pair(fa: list, la: list, fb: list, lb: list) -> list[tuple[int, int]]:
    """Optimal pairs (i, j) within 1 octave, by log frequency and level (dB, relative to each side's max)."""
    if not fa or not fb:
        return []
    fa_, fb_ = np.log2(np.maximum(fa, 1e-9)), np.log2(np.maximum(fb, 1e-9))
    la_, lb_ = np.asarray(la) - max(la), np.asarray(lb) - max(lb)
    oct_ = np.abs(fa_[:, None] - fb_[None, :])
    cost = oct_ + 0.003 * np.abs(la_[:, None] - lb_[None, :])
    cost = np.where(oct_ > 1, 1e6, cost)
    rows, cols = linear_sum_assignment(cost)
    return [(int(i), int(j)) for i, j in zip(rows, cols) if oct_[i, j] <= 1]


def increasing(ts: list, step: float = 1e-6) -> list:
    out: list = []
    for t in ts:
        out.append(float(t) if not out else max(float(t), out[-1] + step))
    return out


def tmap(src: list, dst: list) -> TMap:
    """Piecewise linear through anchors, slope 1 outside."""
    s, d = np.asarray(src, dtype=np.float64), np.asarray(dst, dtype=np.float64)

    def f(t: Any) -> Any:
        t = np.asarray(t, dtype=np.float64)
        y = np.interp(t, s, d)
        y = np.where(t < s[0], d[0] + (t - s[0]), y)
        y = np.where(t > s[-1], d[-1] + (t - s[-1]), y)
        return float(y) if y.ndim == 0 else y

    return f


def anchor_maps(ta: list, tb: list, w: float) -> tuple[TMap, TMap, list]:
    ta, tb = increasing(ta), increasing(tb)
    tm = increasing([lin(x, y, w) for x, y in zip(ta, tb)])
    return tmap(ta, tm), tmap(tb, tm), tm


def curve_mix(ca: list, cb: list, ma: TMap, mb: TMap, w: float, nd: int = 6) -> tuple[np.ndarray, np.ndarray]:
    """2 curves [[t, v], ...] on the morph axis: union times, each interpolated, weighted sum."""
    a, b = np.asarray(ca, dtype=np.float64).reshape(-1, 2), np.asarray(cb, dtype=np.float64).reshape(-1, 2)
    ta, tb = ma(a[:, 0]), mb(b[:, 0])
    t = np.unique(np.round(np.concatenate([np.atleast_1d(ta), np.atleast_1d(tb)]), nd))
    va = np.interp(t, np.atleast_1d(ta), a[:, 1])
    vb = np.interp(t, np.atleast_1d(tb), b[:, 1])
    return t, (1 - w) * va + w * vb


def _farthest(p: np.ndarray, i: int, j: int) -> tuple[int, float]:
    """The point between i and j farthest in value from the line i-j, and its error."""
    t = p[i + 1: j, 0]
    span = p[j, 0] - p[i, 0]
    line = p[i, 1] + (p[j, 1] - p[i, 1]) * ((t - p[i, 0]) / span if span > 0 else 0.0)
    err = np.abs(p[i + 1: j, 1] - line)
    k = int(np.argmax(err))
    return i + 1 + k, float(err[k])


def rdp(pts: list, tol: float) -> list:
    """Ramer-Douglas-Peucker on [[t, v], ...] with the value error as distance."""
    if len(pts) < 3:
        return [list(p) for p in pts]
    p = np.asarray(pts, dtype=np.float64)
    keep = np.zeros(len(p), dtype=bool)
    keep[[0, -1]] = True
    stack = [(0, len(p) - 1)]
    while stack:
        i, j = stack.pop()
        if j - i < 2:
            continue
        m, e = _farthest(p, i, j)
        if e > tol:
            keep[m] = True
            stack += [(i, m), (m, j)]
    return p[keep].tolist()


def extras(w: float, xa: list, xb: list, fa: Callable, fb: Callable) -> list:
    """Unpaired items of both sides: a's while w < 1, then b's while w > 0."""
    out = [fa(x) for x in xa] if w < 1 else []
    return out + ([fb(x) for x in xb] if w > 0 else [])


def left(items: list, used: set) -> list:
    """The items whose index is not in used."""
    return [x for i, x in enumerate(items) if i not in used]


def set_or_drop(m: dict, key: str, value: Any) -> None:
    """m[key] = value when value is truthy, else remove key."""
    if value:
        m[key] = value
    else:
        m.pop(key, None)


# ---------------------------------------------------------------- mode rows


def _pos(rows: list) -> list:
    out = []
    for f, tau, amp, ph in rows:
        out.append([f, tau, -amp, ph + math.pi] if amp < 0 else [f, tau, amp, ph])
    return out


def _row_db(r: list) -> float:
    return 10 * math.log10(max(r[2] ** 2 * r[1], 1e-30))


def _row_pairs(ra: list, rb: list) -> list:
    return pair([r[0] for r in ra], [_row_db(r) for r in ra], [r[0] for r in rb], [_row_db(r) for r in rb])


def _scale_row(r: list, g: float) -> list:
    return [r[0], r[1], r[2] * g, r[3]]


def _paired_row(x: list, y: list, w: float) -> list:
    return [geo(x[0], y[0], w), geo(x[1], y[1], w), geo(x[2], y[2], w), circ(x[3], y[3], w)]


def rows_mix(ra: list, rb: list, w: float) -> list:
    """Mix 2 sets of [f, tau, amp, phase] rows; unpaired rows fade with their side's weight."""
    ra, rb = _pos(ra or []), _pos(rb or [])
    pairs = _row_pairs(ra, rb)
    out = [_paired_row(ra[i], rb[j], w) for i, j in pairs]
    out += extras(w, left(ra, {i for i, _ in pairs}), left(rb, {j for _, j in pairs}),
                  lambda r: _scale_row(r, 1 - w), lambda r: _scale_row(r, w))
    return sorted(out, key=lambda r: r[0])


def _scale_rows(rows: list, g: float) -> list:
    return [[f, tau, amp * g, ph] for f, tau, amp, ph in rows]


# ---------------------------------------------------------------- anchors per kind


def _modal_anchors(m: dict, k: int) -> list:
    return [0.0] + [float(s["t"]) for s in m.get("strikes", [])[:k]] + [float(m.get("length_s", 0.0))]


def _sines_onsets(m: dict) -> list:
    ons = list(m.get("onsets") or [])
    return ons if ons else [float(m.get("onset", 0.0))]


def _sines_anchors(m: dict, k: int) -> list:
    return [0.0] + [float(t) for t in _sines_onsets(m)[:k]] + [float(m.get("duration", 0.0))]


def _keyframe_peak(m: dict) -> float:
    env = m.get("env") or {}
    t, db = env.get("t") or [0.0], env.get("db") or [[0.0]]
    p = [float(np.sum(10 ** (np.asarray(row) / 10))) for row in db]
    return float(t[int(np.argmax(p))]) if p else 0.0


def _spectral_anchors(m: dict) -> list:
    return [0.0, _keyframe_peak(m), float(m.get("dur", 0.0))]


def _modal_maps(a: dict, b: dict, w: float) -> tuple:
    k = min(len(a.get("strikes", [])), len(b.get("strikes", [])))
    return anchor_maps(_modal_anchors(a, k), _modal_anchors(b, k), w)


def _sines_maps(a: dict, b: dict, w: float) -> tuple:
    k = min(len(_sines_onsets(a)), len(_sines_onsets(b)))
    return anchor_maps(_sines_anchors(a, k), _sines_anchors(b, k), w)


def _spectral_maps(a: dict, b: dict, w: float) -> tuple:
    return anchor_maps(_spectral_anchors(a), _spectral_anchors(b), w)


def _gesture_maps(a: dict, b: dict, w: float) -> tuple:
    ta, tb = a.get("tones"), b.get("tones")
    same = ta and tb and kind_of(ta) == kind_of(tb)
    return maps_for(ta, tb, w) if same else maps_for(a["air"], b["air"], w)


def _shared_layer(a: dict, b: dict) -> tuple | None:
    """The models of the first role (tone, body, air) that both hybrids hold with one kind."""
    pairs = ((_layer(a, r), _layer(b, r)) for r in ("tone", "body", "air"))
    return next(((la["model"], lb["model"]) for la, lb in pairs
                 if la and lb and kind_of(la["model"]) == kind_of(lb["model"])), None)


def _hybrid_maps(a: dict, b: dict, w: float) -> tuple | None:
    shared = _shared_layer(a, b)
    return None if shared is None else maps_for(shared[0], shared[1], w)


_MAPS: dict[str, Callable] = {
    "modal": _modal_maps, "sines": _sines_maps, "spectral": _spectral_maps,
    "gesture": _gesture_maps, "hybrid": _hybrid_maps,
}


def maps_for(a: dict, b: dict, w: float) -> tuple[TMap, TMap, list]:
    """The time maps a -> morph and b -> morph of 2 models of one kind."""
    maker = _MAPS.get(kind_of(a))
    maps = maker(a, b, w) if maker else None
    if maps is not None:
        return maps
    ident = tmap([0.0, 1.0], [0.0, 1.0])
    return ident, ident, [0.0, 1.0]


# ---------------------------------------------------------------- modal


def _strike_mix(sa: dict, sb: dict, t: float, w: float) -> dict:
    return {"t": round(t, 6), "ramp_ms": lin(float(sa.get("ramp_ms", 0)), float(sb.get("ramp_ms", 0)), w),
            "modes": rows_mix(sa.get("modes"), sb.get("modes"), w),
            "attack": rows_mix(sa.get("attack"), sb.get("attack"), w)}


def _strike_extra(s: dict, mp: TMap, g: float) -> dict:
    return {"t": round(float(mp(float(s["t"]))), 6), "ramp_ms": s.get("ramp_ms", 0),
            "modes": _scale_rows(s.get("modes") or [], g), "attack": _scale_rows(s.get("attack") or [], g)}


def _modal_strikes(a: dict, b: dict, w: float, tm: list, ma: TMap, mb: TMap) -> list:
    sa, sb = a.get("strikes", []), b.get("strikes", [])
    k = min(len(sa), len(sb))
    out = [_strike_mix(sa[i], sb[i], tm[i + 1], w) for i in range(k)]
    out += extras(w, sa[k:], sb[k:], lambda s: _strike_extra(s, ma, 1 - w), lambda s: _strike_extra(s, mb, w))
    return sorted(out, key=lambda s: s["t"])


def _modal_noise(a: dict, b: dict, w: float, ma: TMap, mb: TMap) -> dict:
    na, nb = a.get("noise") or {}, b.get("noise") or {}
    if list(na.get("edges", [])) != list(nb.get("edges", [])):
        raise ValueError("modal morph: the noise band edges of the 2 models do not match")
    env = []
    for ea, eb in zip(na.get("env", []), nb.get("env", [])):
        t, v = curve_mix(ea, eb, ma, mb, w)
        env.append([[round(float(x), 6), round(float(y), 3)] for x, y in rdp(np.column_stack([t, v]).tolist(), 0.25)])
    out = copy.deepcopy(na)
    out["env"] = env
    return out


def _modal_sub(a: dict, b: dict, w: float) -> dict | None:
    xa = np.asarray((a.get("sub") or {}).get("x") or [], dtype=np.float64)
    xb = np.asarray((b.get("sub") or {}).get("x") or [], dtype=np.float64)
    n = max(len(xa), len(xb))
    x = (1 - w) * np.pad(xa, (0, n - len(xa))) + w * np.pad(xb, (0, n - len(xb)))
    if not n or not np.any(x):
        return None
    return {"rate": 100, "x": x.tolist()}


def _modal_side(a: dict, b: dict, w: float) -> list | None:
    if a.get("side") is None and b.get("side") is None:
        return None
    sa, sb = list(a.get("side") or []), list(b.get("side") or [])
    n = max(len(sa), len(sb))
    sa, sb = sa + [0.0] * (n - len(sa)), sb + [0.0] * (n - len(sb))
    return [round(lin(x, y, w), 4) for x, y in zip(sa, sb)]


def _side_morph(sa: Any, sb: Any, w: float, ma: TMap, mb: TMap) -> Any:
    if not sa and not sb:
        return None
    return _model.module("sines").morph_side(sa, sb, w, map_a=ma, map_b=mb)


def _modal_scalars(a: dict, b: dict, w: float) -> dict:
    return {
        "channels": max(int(a.get("channels", 1)), int(b.get("channels", 1))),
        "fade_out_s": lin(float(a.get("fade_out_s", 0)), float(b.get("fade_out_s", 0)), w),
        "coherent": bool(a.get("coherent") or b.get("coherent")),
        "version": max(int(a.get("version", 1)), int(b.get("version", 1))),
    }


def morph_modal(a: dict, b: dict, w: float) -> dict:
    ma, mb, tm = maps_for(a, b, w)
    m = copy.deepcopy(a)
    m["strikes"] = _modal_strikes(a, b, w, tm, ma, mb)
    m["noise"] = _modal_noise(a, b, w, ma, mb)
    m["sub"] = _modal_sub(a, b, w)
    m["side"] = _modal_side(a, b, w)
    set_or_drop(m, "side_t", _side_morph(a.get("side_t"), b.get("side_t"), w, ma, mb))
    m["length_s"] = round(tm[-1], 6)
    m.update(_modal_scalars(a, b, w))
    return m


# ---------------------------------------------------------------- sines


def _peak_db(p: dict) -> float:
    return max((float(v) for _, v in p.get("amp", [[0, -150]])), default=-150.0)


def _energy_db(p: dict) -> float:
    return _peak_db(p) + 10 * math.log10(float(p["t1"]) - float(p["t0"]) + 0.01)


def _life_curve(c: list, t0: float, t1: float, mp: TMap) -> np.ndarray:
    a = np.asarray(c, dtype=np.float64).reshape(-1, 2)
    m0, m1 = mp(t0), mp(t1)
    u = (np.atleast_1d(mp(a[:, 0])) - m0) / max(m1 - m0, 1e-9)
    return np.column_stack([u, a[:, 1]])


def _life_mix(ca: list, cb: list, pa: dict, pb: dict, ma: TMap, mb: TMap, w: float, log: bool) -> tuple:
    ua = _life_curve(ca, float(pa["t0"]), float(pa["t1"]), ma)
    ub = _life_curve(cb, float(pb["t0"]), float(pb["t1"]), mb)
    if log:
        ua[:, 1], ub[:, 1] = np.log2(np.maximum(ua[:, 1], 1e-9)), np.log2(np.maximum(ub[:, 1], 1e-9))
    u = np.unique(np.round(np.concatenate([ua[:, 0], ub[:, 0]]), 9))
    v = (1 - w) * np.interp(u, ua[:, 0], ua[:, 1]) + w * np.interp(u, ub[:, 0], ub[:, 1])
    return u, (2 ** v if log else v)


def _partial_span(pa: dict, pb: dict, ma: TMap, mb: TMap, w: float) -> tuple[float, float]:
    t0 = lin(ma(float(pa["t0"])), mb(float(pb["t0"])), w)
    t1 = max(lin(ma(float(pa["t1"])), mb(float(pb["t1"])), w), t0 + 0.0001)
    return t0, t1


def _on_span(uv: tuple, t0: float, t1: float) -> list:
    """Life-relative curve points (u in 0..1) as absolute [t, value] points."""
    return [[float(t0 + u * (t1 - t0)), float(v)] for u, v in zip(*uv)]


def _partial_mix(pa: dict, pb: dict, ma: TMap, mb: TMap, w: float) -> dict:
    t0, t1 = _partial_span(pa, pb, ma, mb, w)
    freq = _life_mix(pa["freq"], pb["freq"], pa, pb, ma, mb, w, True)
    amp = _life_mix(pa["amp"], pb["amp"], pa, pb, ma, mb, w, False)
    return {"t0": t0, "rise": lin(float(pa["rise"]), float(pb["rise"]), w), "t1": t1,
            "rel": lin(float(pa["rel"]), float(pb["rel"]), w), "ph": circ(float(pa["ph"]), float(pb["ph"]), w),
            "freq": _on_span(freq, t0, t1), "amp": _on_span(amp, t0, t1)}


def _partial_mapped(p: dict, mp: TMap, gain_db: float) -> dict:
    q = copy.deepcopy(p)
    q["t0"], q["t1"] = float(mp(float(p["t0"]))), float(mp(float(p["t1"])))
    q["freq"] = [[float(mp(float(t))), f] for t, f in p["freq"]]
    q["amp"] = [[float(mp(float(t))), float(d) + gain_db] for t, d in p["amp"]]
    return q


def _partial_pairs(pa: list, pb: list) -> list:
    sines = _model.module("sines")
    return pair([sines.partial_freq(p) for p in pa], [_energy_db(p) for p in pa],
                [sines.partial_freq(p) for p in pb], [_energy_db(p) for p in pb])


def _sines_partials(a: dict, b: dict, w: float, ma: TMap, mb: TMap) -> list:
    pa, pb = a.get("partials", []), b.get("partials", [])
    pairs = _partial_pairs(pa, pb)
    out = [_partial_mix(pa[i], pb[j], ma, mb, w) for i, j in pairs]
    out += extras(w, left(pa, {i for i, _ in pairs}), left(pb, {j for _, j in pairs}),
                  lambda p: _partial_mapped(p, ma, wdb(1 - w)), lambda p: _partial_mapped(p, mb, wdb(w)))
    return sorted(out, key=_peak_db, reverse=True)


def _transients(a: dict, b: dict, w: float, ma: TMap, mb: TMap) -> list:
    ta, tb = a.get("transients", []), b.get("transients", [])
    k = min(len(ta), len(tb))
    out = [{"t": lin(float(ta[i]["t"]), float(tb[i]["t"]), w), "atoms": rows_mix(ta[i]["atoms"], tb[i]["atoms"], w)}
           for i in range(k)]
    out += extras(w, ta[k:], tb[k:], lambda s: _transient_extra(s, ma, 1 - w), lambda s: _transient_extra(s, mb, w))
    return sorted(out, key=lambda s: s["t"])


def _transient_extra(s: dict, mp: TMap, g: float) -> dict:
    return {"t": float(mp(float(s["t"]))), "atoms": _scale_rows(s["atoms"], g)}


def _mapped_times(r: dict, mp: TMap) -> np.ndarray:
    return np.atleast_1d(mp(np.asarray(r.get("times", [0.0]))))


def _band_rows(t: np.ndarray, tA: np.ndarray, tB: np.ndarray, ra: dict, rb: dict, w: float) -> list:
    return [np.round((1 - w) * np.interp(t, tA, da) + w * np.interp(t, tB, dbb), 2).tolist()
            for da, dbb in zip(ra.get("db", []), rb.get("db", []))]


def _residual(a: dict, b: dict, w: float, ma: TMap, mb: TMap) -> dict:
    ra, rb = a.get("residual") or {}, b.get("residual") or {}
    if list(ra.get("bands", [])) != list(rb.get("bands", [])):
        raise ValueError("sines morph: the residual bands of the 2 models do not match")
    tA, tB = _mapped_times(ra, ma), _mapped_times(rb, mb)
    t = np.unique(np.round(np.concatenate([tA, tB]), 6))
    out = copy.deepcopy(ra if w < 0.5 else rb)
    out.update({"bands": list(ra.get("bands", [])), "times": t.tolist(), "db": _band_rows(t, tA, tB, ra, rb, w)})
    return out


def _onsets(a: dict, b: dict, w: float, tm: list, ma: TMap, mb: TMap) -> list:
    oa, ob = list(a.get("onsets") or []), list(b.get("onsets") or [])
    k = min(len(oa), len(ob))
    out = [lin(float(x), float(y), w) for x, y in zip(oa[:k], ob[:k])]
    out += extras(w, oa[k:], ob[k:], lambda t: float(ma(float(t))), lambda t: float(mb(float(t))))
    return sorted(out)


def _sines_scalars(a: dict, b: dict, w: float, tm: list) -> dict:
    return {
        "duration": round(tm[-1], 5),
        "peak_db": lin(float(a.get("peak_db", 0)), float(b.get("peak_db", 0)), w),
        "width": lin(float(a.get("width", -120)), float(b.get("width", -120)), w),
        "channels": max(_model.channels_of(a), _model.channels_of(b)),
        "coherent": bool(a.get("coherent") or b.get("coherent")),
        "analysis": {},
    }


def _with_summary(m: dict) -> dict:
    s = _model.module("sines").summary(m)
    if isinstance(s, dict):
        m["summary"] = s
    return m


def morph_sines(a: dict, b: dict, w: float) -> dict:
    ma, mb, tm = maps_for(a, b, w)
    m = copy.deepcopy(a)
    m["partials"] = _sines_partials(a, b, w, ma, mb)
    m["transients"] = _transients(a, b, w, ma, mb)
    m["residual"] = _residual(a, b, w, ma, mb)
    m["onsets"] = _onsets(a, b, w, tm, ma, mb)
    m["onset"] = m["onsets"][0] if m["onsets"] else 0.0
    m.update(_sines_scalars(a, b, w, tm))
    set_or_drop(m, "side", _side_morph(a.get("side"), b.get("side"), w, ma, mb))
    return _with_summary(m)


# ---------------------------------------------------------------- spectral


def _stereo_grid(st: dict | None, tk: np.ndarray, grid: np.ndarray, groups: int, key: str, empty: float) -> np.ndarray:
    if not st:
        return np.full((len(grid), groups), empty)
    v = np.asarray(st[key], dtype=np.float64)
    return np.column_stack([np.interp(grid, tk, v[:, g]) for g in range(v.shape[1])])


def _spectral_env(a: dict, b: dict, w: float, ma: TMap, mb: TMap) -> tuple[np.ndarray, list, np.ndarray, np.ndarray]:
    ea, eb = a["env"], b["env"]
    tA, tB = np.atleast_1d(ma(np.asarray(ea["t"]))), np.atleast_1d(mb(np.asarray(eb["t"])))
    grid = np.unique(np.round(np.concatenate([tA, tB]), 5))
    da, db = np.asarray(ea["db"], dtype=np.float64), np.asarray(eb["db"], dtype=np.float64)
    rows = [(1 - w) * np.interp(grid, tA, da[:, k]) + w * np.interp(grid, tB, db[:, k]) for k in range(da.shape[1])]
    env = np.round(np.column_stack(rows), 2).tolist()
    return grid, env, tA, tB


def _spectral_stereo(a: dict, b: dict, w: float, grid: np.ndarray, tA: np.ndarray, tB: np.ndarray) -> dict | None:
    sa, sb = a.get("stereo"), b.get("stereo")
    if not sa and not sb:
        return None
    base = sa or sb
    ng = len(base["ild"][0]) if base["ild"] else 0
    out = copy.deepcopy(base)
    for key, empty, nd in (("ild", 0.0, 2), ("icc", 1.0, 3)):
        va = _stereo_grid(sa, tA, grid, ng, key, empty)
        vb = _stereo_grid(sb, tB, grid, ng, key, empty)
        out[key] = np.round((1 - w) * va + w * vb, nd).tolist()
    return out


def _ridges(a: dict, b: dict, w: float, ma: TMap, mb: TMap) -> list:
    out = []
    for m, mp, g in ((a, ma, 1 - w), (b, mb, w)):
        if g <= 0:
            continue
        for r in m.get("ridges") or []:
            q = copy.deepcopy(r)
            q["t"] = np.round(np.atleast_1d(mp(np.asarray(r["t"]))), 5).tolist() if isinstance(r["t"], list) \
                else float(mp(float(r["t"])))
            q["db"] = (np.asarray(r["db"]) + wdb(g)).tolist() if isinstance(r["db"], list) else float(r["db"]) + wdb(g)
            out.append(q)
    return out


def _grains(ta: dict | None, tb: dict | None, near: dict, w: float) -> dict | None:
    ga, gb = (ta or {}).get("grains"), (tb or {}).get("grains")
    if ga and gb:
        return dict(near.get("grains") or ga, density=geo(float(ga["density"]), float(gb["density"]), w))
    one, weight = (ga, 1 - w) if ga else (gb, w)
    return copy.deepcopy(one) if one and weight >= 0.5 else None


def _texture(a: dict, b: dict, w: float) -> dict | None:
    ta, tb = a.get("texture"), b.get("texture")
    near = (ta if w < 0.5 else tb) or ta or tb
    if not near:
        return None
    out = copy.deepcopy(near)
    out["grains"] = _grains(ta, tb, near, w)
    out["am"] = _am(ta, tb, w)
    return out


def _am_both(aa: dict, ab: dict, w: float) -> dict:
    out = copy.deepcopy(aa if w < 0.5 else ab)
    if len(aa["depth"]) == len(ab["depth"]):
        out["depth"] = [lin(x, y, w) for x, y in zip(aa["depth"], ab["depth"])]
    return out


def _am_one(one: dict | None, weight: float) -> dict | None:
    if not one:
        return None
    out = copy.deepcopy(one)
    out["depth"] = [d * weight for d in one["depth"]]
    return out


def _am(ta: dict | None, tb: dict | None, w: float) -> dict | None:
    aa, ab = (ta or {}).get("am"), (tb or {}).get("am")
    if aa and ab:
        return _am_both(aa, ab, w)
    return _am_one(*((aa, 1 - w) if aa else (ab, w)))


def _fine(a: dict, b: dict, w: float) -> dict | None:
    fa, fb = a.get("fine"), b.get("fine")
    if fa and fb and len(fa["db"]) == len(fb["db"]):
        return dict(fa, db=[lin(x, y, w) for x, y in zip(fa["db"], fb["db"])])
    near = fa if w < 0.5 else fb
    return copy.deepcopy(near)


def _spectral_scalars(a: dict, b: dict, w: float, tm: list) -> dict:
    return {
        "level_db": lin(float(a.get("level_db", 0)), float(b.get("level_db", 0)), w),
        "dur": round(tm[-1], 4),
        "channels": max(_model.channels_of(a), _model.channels_of(b)),
        "finish": bool(a.get("finish") or b.get("finish")),
        "source": None,
        "fit": {},
    }


def morph_spectral(a: dict, b: dict, w: float) -> dict:
    if a.get("bands") != b.get("bands"):
        raise ValueError("spectral morph: the band axes of the 2 models do not match")
    ma, mb, tm = maps_for(a, b, w)
    grid, env, tA, tB = _spectral_env(a, b, w, ma, mb)
    m = copy.deepcopy(a)
    m["env"] = {"t": grid.tolist(), "db": env}
    m["stereo"] = _spectral_stereo(a, b, w, grid, tA, tB)
    m["ridges"] = _ridges(a, b, w, ma, mb)
    m["texture"] = _texture(a, b, w)
    m["fine"] = _fine(a, b, w)
    m.update(_spectral_scalars(a, b, w, tm))
    return m


# ---------------------------------------------------------------- gesture and hybrid


def _tones_same(ta: dict, tb: dict, w: float) -> dict:
    t = KIND_MORPH[kind_of(ta)](ta, tb, w)
    t["layer"] = "deterministic"
    if kind_of(t) == "modal" and t.get("strikes"):
        t["onset"] = t["strikes"][0]["t"]
    return t


def _tones_one(one: dict | None, weight: float) -> dict | None:
    if not one or weight <= 0:
        return None
    return _model.module("gesture").gain_tones(one, wdb(weight))


def _tones(a: dict, b: dict, w: float) -> dict | None:
    ta, tb = a.get("tones"), b.get("tones")
    if ta and tb:
        return _tones_same(ta, tb, w) if kind_of(ta) == kind_of(tb) else copy.deepcopy(ta if w < 0.5 else tb)
    return _tones_one(*((ta, 1 - w) if ta else (tb, w)))


def morph_gesture(a: dict, b: dict, w: float) -> dict:
    m = copy.deepcopy(a)
    ch = max(int(a.get("channels", 1)), int(b.get("channels", 1)))
    m["tones"] = _tones(a, b, w)
    if m["tones"]:
        m["tones"]["channels"] = ch
    m["air"] = morph_spectral(a["air"], b["air"], w)
    m["air"]["channels"] = ch
    m["traj"] = _model.module("spectral").trajectory(m["air"])
    m["dur"] = round(lin(float(a.get("dur", 0)), float(b.get("dur", 0)), w), 6)
    m["channels"] = ch
    return m


def _layer(m: dict, role: str) -> dict | None:
    return next((layer for layer in m.get("layers", []) if layer.get("role") == role), None)


def _role_layer(role: str, la: dict | None, lb: dict | None, w: float) -> dict | None:
    """The morph layer of one role: both mixed, or one side faded by its weight."""
    if la and lb:
        return {"role": role, "gain_db": lin(float(la.get("gain_db", 0)), float(lb.get("gain_db", 0)), w),
                "model": morph(la["model"], lb["model"], w)}
    one, weight = (la, 1 - w) if la else (lb, w)
    if not one or weight <= 0:
        return None
    q = copy.deepcopy(one)
    q["gain_db"] = float(one.get("gain_db", 0)) + wdb(weight)
    return q


def morph_hybrid(a: dict, b: dict, w: float) -> dict:
    layers = [_role_layer(r, _layer(a, r), _layer(b, r), w) for r in ("tone", "body", "air")]
    m = copy.deepcopy(a)
    m["layers"] = [layer for layer in layers if layer is not None]
    m["length_s"] = lin(float(a.get("length_s", 0)), float(b.get("length_s", 0)), w)
    m["channels"] = max(int(a.get("channels", 1)), int(b.get("channels", 1)))
    return m


KIND_MORPH: dict[str, Callable[[dict, dict, float], dict]] = {
    "modal": morph_modal, "sines": morph_sines, "spectral": morph_spectral,
    "gesture": morph_gesture, "hybrid": morph_hybrid,
}


# ---------------------------------------------------------------- post stage mix


def _resample(v: list, n: int) -> np.ndarray:
    v = np.asarray(v, dtype=np.float64)
    if len(v) == 0:
        return np.zeros(n)
    if len(v) == 1:
        return np.full(n, v[0])
    return np.interp(np.linspace(0, 1, n), np.linspace(0, 1, len(v)), v)


def _attack_mix(pa: dict, pb: dict, w: float) -> list:
    aa, ab = pa.get("attack_db") or [], pb.get("attack_db") or []
    if not (aa and ab):
        return []
    k = max(2, int(round(lin(len(aa), len(ab), w))))
    return ((1 - w) * _resample(aa, k) + w * _resample(ab, k)).tolist()


def _pre_flags(pa: dict, pb: dict) -> dict:
    out = {}
    if pa.get("v") == 2 and pb.get("v") == 2:
        out["v"] = 2
    if pa.get("hp_hz") and pb.get("hp_hz"):
        out["hp_hz"] = max(pa["hp_hz"], pb["hp_hz"])
    return out


def _pre_mix(pa: dict, pb: dict, w: float) -> dict:
    t_on = lin(float(pa["t_on"]), float(pb["t_on"]), w)
    n = max(1, int(round(t_on / 0.00025)))
    db = (1 - w) * _resample(pa.get("db") or [], n) + w * _resample(pb.get("db") or [], n)
    out = {"t_on": t_on, "db": db.tolist(), "attack_db": _attack_mix(pa, pb, w)}
    out.update(_pre_flags(pa, pb))
    return out


def _gate_mix(ga: dict | None, gb: dict | None, w: float, ma: TMap, mb: TMap) -> dict:
    pa = (ga or {}).get("pts") or [[0.0, 0.0]]
    pb = (gb or {}).get("pts") or [[0.0, 0.0]]
    a, b = np.asarray(pa, dtype=np.float64).reshape(-1, 2), np.asarray(pb, dtype=np.float64).reshape(-1, 2)
    ta, tb = np.atleast_1d(ma(a[:, 0])), np.atleast_1d(mb(b[:, 0]))
    t = np.unique(np.round(np.concatenate([ta, tb]), 6))
    va = np.interp(t, ta, a[:, 1], left=0.0, right=a[-1, 1])
    vb = np.interp(t, tb, b[:, 1], left=0.0, right=b[-1, 1])
    return {"pts": [[float(x), round(float(y), 3)] for x, y in zip(t, (1 - w) * va + w * vb)]}


_ZERO_POST = {"eq_db": [0.0] * 28, "gain_db": 0.0}


def _mix_eq(xa: dict, xb: dict, w: float) -> list:
    ea, eb = xa.get("eq_db") or _ZERO_POST["eq_db"], xb.get("eq_db") or _ZERO_POST["eq_db"]
    return [lin(float(x), float(y), w) for x, y in zip(ea, eb)]


def _pre_and_gate(a: dict, b: dict, xa: dict, xb: dict, w: float) -> dict:
    out = {}
    if xa.get("pre") and xb.get("pre"):
        out["pre"] = _pre_mix(xa["pre"], xb["pre"], w)
    if xa.get("gate") or xb.get("gate"):
        ma, mb, _ = maps_for(a, b, w)
        out["gate"] = _gate_mix(xa.get("gate"), xb.get("gate"), w, ma, mb)
    return out


def _near_parts(near: dict) -> dict:
    """Space, bed and envelope entries, copied from the nearer source."""
    return {key: copy.deepcopy(near[key]) for key in ("space", "bed", "env") if near.get(key)}


def post_mix(a: dict, b: dict, w: float) -> dict | None:
    pa, pb = a.get("post"), b.get("post")
    if not pa and not pb:
        return None
    xa, xb = pa or _ZERO_POST, pb or _ZERO_POST
    out: dict = {"eq_db": _mix_eq(xa, xb, w), "gain_db": lin(float(xa.get("gain_db", 0)), float(xb.get("gain_db", 0)), w)}
    out.update(_pre_and_gate(a, b, xa, xb, w))
    out.update(_near_parts(xa if w < 0.5 else xb))
    return out


# ---------------------------------------------------------------- entry


def _check_pair(ka: str, kb: str) -> None:
    if "morph" in (ka, kb):
        raise ValueError("a morph of a morph model is not supported")
    if ka != kb:
        raise ValueError(f"cannot morph a {ka} model with a {kb} model; refit both with one kind")


def _mixed(a: dict, b: dict, kind: str, w: float) -> dict:
    m = KIND_MORPH[kind](strip(a), strip(b), w)
    set_or_drop(m, "post", post_mix(a, b, w))
    if "vary_scale" in a or "vary_scale" in b:
        m["vary_scale"] = round(lin(float(a.get("vary_scale", 1)), float(b.get("vary_scale", 1)), w), 4)
    return m


def morph(a: dict, b: dict, amount: float = 0.5) -> dict:
    """A new model between 2 fits of one kind; amount 0 or 1 gives that source exactly."""
    ka = kind_of(a)
    _check_pair(ka, kind_of(b))
    w = float(min(max(amount, 0.0), 1.0))
    return strip(a if w == 0.0 else b) if w in (0.0, 1.0) else _mixed(a, b, ka, w)
