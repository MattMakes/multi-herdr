"""Space layer: stereo measures and a seeded diffuse tail layer for stereo fits.

A space model is a layer, not a kind: it has no KIND face. The wet layer
convolves the dry render with seeded-noise impulse responses. Its side part
leaves the mono sum unchanged.
"""

from __future__ import annotations

from typing import Any

import numpy as np
from scipy.fft import irfft, next_fast_len, rfft
from scipy.ndimage import uniform_filter1d

from hark.fit.analysis._common import SR, as_stereo, db, mel_db_distance, resample
from hark.fit.analysis._kernels import finish, shaped_side, tri_masks

__all__ = ["BANDS", "as_stereo", "is_stereo", "side_powers", "diffuse_power", "band_powers",
           "active_span", "tail_span", "corr", "tail_corr", "needs_space", "shaped_side",
           "impulse", "wet", "tail_s", "fit_space", "render_space", "describe"]

BANDS = (125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0)
VERSION = 1
SALT = {"side": 4099, "mid": 8191}
OFF = -150.0


# ---------------------------------------------------------------- measures

def is_stereo(x: Any, tol_db: float = -60) -> bool:
    """True when x has 2 channels whose side is more than ``tol_db`` re the mid."""
    a = np.asarray(x, dtype=float)
    if a.ndim != 2 or a.shape[1] < 2:
        return False
    m = (a[:, 0] + a[:, 1]) / 2
    s = (a[:, 0] - a[:, 1]) / 2
    em, es = float(np.sum(m ** 2)), float(np.sum(s ** 2))
    if es <= 0:
        return False
    return bool(em <= 0 or 10 * np.log10(es / em) > tol_db)


def _frames(m: np.ndarray, nfft: int, hop: int) -> np.ndarray:
    k = len(m) // hop + 1
    buf = np.zeros((k - 1) * hop + nfft)
    buf[nfft // 2:nfft // 2 + len(m)] = m[:len(buf) - nfft // 2]
    win = np.hanning(nfft + 2)[1:-1]
    idx = np.arange(k)[:, None] * hop + np.arange(nfft)[None, :]
    return np.fft.rfft(buf[idx] * win, axis=1)


def side_powers(x: Any, centers=BANDS, nfft: int = 1024, hop: int = 240) -> dict:
    """Per band and frame: mid power PM, side power PS and Re(S conj(M)) as C."""
    a = as_stereo(x)
    fm = _frames((a[:, 0] + a[:, 1]) / 2, nfft, hop)
    fs = _frames((a[:, 0] - a[:, 1]) / 2, nfft, hop)
    w = tri_masks(centers, nfft).T
    return {"t": np.arange(fm.shape[0]) * hop / SR,
            "PM": (np.abs(fm) ** 2 @ w).T,
            "PS": (np.abs(fs) ** 2 @ w).T,
            "C": (np.real(fs * np.conj(fm)) @ w).T}


def diffuse_power(P: dict) -> np.ndarray:
    return np.maximum(P["PS"] - P["C"] ** 2 / np.maximum(P["PM"], 1e-30), 0.0)


def band_powers(m: np.ndarray, centers=BANDS, nfft: int = 1024, hop: int = 240) -> np.ndarray:
    """Per band x frame power of a mono signal."""
    f = _frames(np.asarray(m, dtype=float), nfft, hop)
    return (np.abs(f) ** 2 @ tri_masks(centers, nfft).T).T


def _env_1ms(m: np.ndarray) -> np.ndarray:
    n = len(m)
    c = np.concatenate([[0.0], np.cumsum(m ** 2)])
    centres = np.arange(0, n, 48)
    lo = np.clip(centres - 120, 0, n)
    hi = np.clip(centres + 120, 0, n)
    return (c[hi] - c[lo]) / np.maximum(hi - lo, 1)


def active_span(x: Any) -> tuple[int, int, int]:
    """(onset, peak, end) sample indices of the active part."""
    a = np.asarray(x, dtype=float)
    m = a.mean(axis=1) if a.ndim == 2 else a
    n = len(m)
    env = _env_1ms(m)
    if len(env) < 3:
        return 0, 0, n
    env = uniform_filter1d(env, 10, mode="nearest")
    ip = int(np.argmax(env))
    pk = float(env[ip])
    if pk <= 0:
        return 0, 0, n
    thr = _active_threshold(env, pk)
    onset = int(np.argmax(env >= pk * 1e-3))
    above = np.nonzero(db(env) >= db(pk) + thr)[0]
    last = int(above[-1]) if len(above) else ip
    return onset * 48, ip * 48, min(n, last * 48 + 48)


def _active_threshold(env: np.ndarray, pk: float) -> float:
    if len(env) < 80:
        return -40.0
    d = db(env)
    fl = min(float(np.median(d[5:50])), float(np.median(d[-25:-12])))
    return float(np.clip(fl + 3 - float(db(pk)), -45.0, -12.0))


def tail_span(x: Any, start_s: float = 0.03, min_s: float = 0.02) -> tuple[int, int] | None:
    _, peak, end = active_span(x)
    first = peak + int(round(start_s * SR))
    if end - first < min_s * SR:
        return None
    return first, end


def corr(L: np.ndarray, R: np.ndarray) -> float | None:
    el, er = float(np.sum(L * L)), float(np.sum(R * R))
    if el < 1e-20 or er < 1e-20:
        return None
    return float(np.sum(L * R) / np.sqrt(el * er))


def tail_corr(x: Any, span: tuple[int, int] | None = None) -> float | None:
    a = as_stereo(x)
    span = span or tail_span(a)
    if span is None:
        return None
    return corr(a[span[0]:span[1], 0], a[span[0]:span[1], 1])


def needs_space(x: Any, max_corr: float = 0.7) -> bool:
    a = np.asarray(x, dtype=float)
    if not is_stereo(a):
        return False
    tc = tail_corr(a)
    return tc is not None and tc <= max_corr


# ---------------------------------------------------------------- layer

def _on(model: dict, part: str) -> np.ndarray:
    return np.asarray(model.get(f"{part}_db", [OFF] * len(model["bands"])), dtype=float) > -140


def impulse(model: dict, part: str = "side", seed: int = 0, flat_iters: int = 4) -> np.ndarray:
    """The impulse response of one part (unit-energy bands at their gains)."""
    gains = np.asarray(model[f"{part}_db"], dtype=float)
    on = gains > -140
    out = np.zeros(1)
    if np.any(on):
        out = _impulse_bands(model, part, seed, flat_iters, gains, on)
    return out


def _impulse_bands(model, part, seed, flat_iters, gains, on) -> np.ndarray:
    rt = np.clip(np.asarray(model["rt60"], dtype=float), 0.02, 3.0)
    t, rise = _rise(model, float(rt[on].max()))
    length = len(t)
    r = np.random.default_rng((int(seed) * 7919 + SALT[part]) & 0xFFFFFFFF)
    nfft = next_fast_len(2 * length)
    spec = rfft(r.standard_normal(nfft))
    masks = tri_masks(model["bands"], nfft)
    out = np.zeros(length)
    for b in np.nonzero(on)[0]:
        env = rise * np.exp(-6.907755 * np.maximum(t, 0) / rt[b])
        out += _unit_band(spec, masks[b], env, nfft, flat_iters) * 10.0 ** (gains[b] / 20.0)
    return out


def _rise(model: dict, rt_max: float) -> tuple[np.ndarray, np.ndarray]:
    """Times from the predelay and the build-up curve over the impulse length."""
    build = max(float(model.get("build", 1e-4)), 1e-4)
    pre = float(model.get("predelay", 0.002))
    length = int((pre + build + min(3.0, rt_max)) * SR) + 1
    t = np.arange(length) / SR - pre
    return t, np.where(t >= 0, 1.0 - np.exp(-np.maximum(t, 0) / build), 0.0)


def _unit_band(spec, mask, env, nfft, flat_iters) -> np.ndarray:
    h = irfft(spec * mask, nfft)[:len(env)] * env
    h = _flatten(h, mask, env, nfft, flat_iters)
    e = np.sqrt(np.sum(h ** 2))
    return h / e if e > 0 else np.zeros(len(h))


def _flatten(h: np.ndarray, mask: np.ndarray, env: np.ndarray, nfft: int, iters: int) -> np.ndarray:
    for _ in range(iters):
        sp = rfft(h, nfft)
        sp = mask * np.exp(1j * np.angle(sp))
        h = irfft(sp, nfft)[:len(h)] * env
    return h


def wet(model: dict, dry: np.ndarray, seed: int = 0, n: int | None = None,
        part: str = "side") -> np.ndarray:
    """One part of the wet layer: the dry's channel mean convolved with its impulse."""
    from scipy.signal import fftconvolve
    d = np.asarray(dry, dtype=float)
    m = d.mean(axis=1) if d.ndim == 2 else d
    n = len(m) if n is None else int(n)
    h = impulse(model, part, seed)
    if not np.any(h) or len(m) == 0:
        return np.zeros(n)
    y = fftconvolve(m, h)
    return np.pad(y, (0, max(0, n - len(y))))[:n]


def tail_s(model: dict) -> float:
    on = _on(model, "side") | _on(model, "mid")
    if not np.any(on):
        return 0.0
    rt = np.clip(np.asarray(model["rt60"], dtype=float), 0.02, 3.0)
    return float(model.get("predelay", 0.002) + max(model.get("build", 1e-4), 1e-4)
                 + min(3.0, float(rt[on].max())))


def _band_split(x: np.ndarray, nfft: int, masks: np.ndarray) -> np.ndarray:
    sp = rfft(x, nfft)
    return np.array([irfft(sp * m, nfft)[:len(x)] for m in masks])


def _orthogonal(ws: np.ndarray, refs: list[np.ndarray], bands) -> np.ndarray:
    n = len(ws)
    nfft = next_fast_len(n + 8192)
    masks = tri_masks(bands, nfft)
    wb = _band_split(ws, nfft, masks)
    rb = [_band_split(r, nfft, masks) for r in refs]
    for b in range(len(masks)):
        basis: list[np.ndarray] = []
        for r in rb:
            v = r[b].copy()
            for u in basis:
                v -= np.dot(v, u) * u
            nv = np.linalg.norm(v)
            if nv >= 1e-12:
                basis.append(v / nv)
        for u in basis:
            wb[b] -= np.dot(wb[b], u) * u
    return wb.sum(axis=0)


def render_space(model: dict | None, dry: np.ndarray, seed: int = 0, width: float = 1,
                 ring: bool = False, limit: bool = True) -> np.ndarray:
    """Dry plus wet as (n, 2). A null model returns the dry with no finish."""
    y = as_stereo(dry)
    if model and len(y):
        y = _with_space(model, y, seed, width, ring, limit)
    return y


def _with_space(model: dict, y: np.ndarray, seed: int, width: float, ring: bool, limit: bool) -> np.ndarray:
    if ring:
        y = np.vstack([y, np.zeros((int(tail_s(model) * SR) + 1, 2))])
    n = len(y)
    wm = _part_wet(model, y, seed, n, "mid")
    ws = _part_wet(model, y, seed, n, "side")
    if np.any(ws):
        refs = [(y[:, 0] - y[:, 1]) / 2, (y[:, 0] + y[:, 1]) / 2 + wm]
        ws = _orthogonal(ws, refs, model["bands"]) * width
    y = y + np.column_stack([wm + ws, wm - ws])
    return finish(y, SR, 0.0, -1.0, limit)


def _part_wet(model: dict, y: np.ndarray, seed: int, n: int, part: str) -> np.ndarray:
    return wet(model, y, seed, n, part) if np.any(_on(model, part)) else np.zeros(n)


# ---------------------------------------------------------------- fit

def fit_space(x: Any, sr: int = 48000, dry: np.ndarray | None = None, max_corr: float = 0.7,
              iters: int = 3, need_db: float = -15, mid_need_db: float = -10) -> dict | None:
    """A space model for a stereo recording with a decorrelated tail, or None."""
    a = as_stereo(x) if isinstance(x, str) else as_stereo(resample(np.asarray(x, float), sr))
    st = _space_start(a, dry, max_corr, mid_need_db)
    out = None
    if st is not None and st[3]["rel"] >= need_db:
        out = _space_finish(a, *st, iters, need_db, mid_need_db)
    return out


def _space_start(a: np.ndarray, dry, max_corr: float, mid_need_db: float):
    """(span, tail corr, context, need) when a has a decorrelated tail to model, else None."""
    span = tail_span(a) if is_stereo(a) else None
    tc = tail_corr(a, span) if span else None
    if tc is None or tc > max_corr:
        return None
    ctx = _tail_context(a, span)
    need = None if ctx is None else _needs(ctx, a, dry, span, tc, mid_need_db)
    return None if need is None else (span, tc, ctx, need)


def _space_finish(a, span, tc, ctx, need, iters, need_db, mid_need_db) -> dict | None:
    model = _start_model(ctx, need, span, tc)
    src = need["dry"] if need["dry"] is not None else a
    for _ in range(iters):
        _iterate(model, src, ctx, need)
    _mid_check(model, a, src)
    return _level_side(model, a, src, span, tc, need, iters, need_db, mid_need_db)


def _tail_context(a: np.ndarray, span: tuple[int, int]) -> dict | None:
    P = side_powers(a)
    frames = (P["t"] * SR >= span[0]) & (P["t"] * SR < span[1])
    if np.count_nonzero(frames) < 3:
        return None
    dif = uniform_filter1d(diffuse_power(P), 5, axis=1, mode="nearest")
    mid = uniform_filter1d(P["PM"], 5, axis=1, mode="nearest")
    return {"P": P, "frames": frames, "dif": dif, "mid": mid,
            "E_dif": dif[:, frames].sum(axis=1), "E_mid": mid[:, frames].sum(axis=1)}


def _needs(ctx: dict, a, dry, span, tc, mid_need_db) -> dict | None:
    parts = (ctx["E_dif"].copy(), np.zeros(len(BANDS)), 1.0, None)
    if dry is not None:
        parts = _dry_needs(ctx, a, dry, span, tc, mid_need_db)
    return None if parts is None else _need_dict(ctx, *parts)


def _dry_needs(ctx: dict, a, dry, span, tc, mid_need_db):
    """(side need, mid need, dry tail corr, padded dry), or None when the dry is not drier."""
    d2 = as_stereo(dry)
    d2 = np.pad(d2, ((0, max(0, len(a) - len(d2))), (0, 0)))[:len(a)]
    tc_dry = tail_corr(d2, span)
    if tc_dry is None or tc_dry <= tc + 0.1:
        return None
    side_need, mid_need = ctx["E_dif"].copy(), np.zeros(len(BANDS))
    dctx = _tail_context(d2, span)
    if dctx is not None:
        side_need = np.maximum(ctx["E_dif"] - dctx["E_dif"], 0.0)
        short = np.maximum(ctx["E_mid"] - dctx["E_mid"], 0.0)
        mid_need = np.where(short >= ctx["E_mid"] * 10 ** (mid_need_db / 10), short, 0.0)
    return side_need, mid_need, tc_dry, d2


def _need_dict(ctx: dict, side_need, mid_need, tc_dry, d2) -> dict | None:
    tot = float(side_need.sum())
    if tot <= 0 or ctx["E_dif"].sum() <= 0:
        return None
    rel = float(db(tot) - db(ctx["E_dif"].sum()))
    return {"side": side_need, "mid": mid_need, "rel": rel, "tc_dry": tc_dry, "dry": d2}


def _decay_slope(curve_db: np.ndarray, t: np.ndarray) -> float | None:
    sel = curve_db >= curve_db.max() - 30
    if np.count_nonzero(sel) < 4:
        return None
    slope = float(np.polyfit(t[sel], curve_db[sel], 1)[0])
    return slope if slope < -1 else None


def _start_model(ctx: dict, need: dict, span, tc) -> dict:
    live = need["side"] > need["side"].max() * 1e-4
    rt_max = float(np.clip(4 * (span[1] - span[0]) / SR, 0.1, 3.0))
    rt = _band_rts(ctx, live, rt_max)
    live_mid = need["mid"] > max(need["mid"].max(), 1e-300) * 1e-4
    live_mid &= need["mid"] > 0
    return {"kind": "space", "version": VERSION, "predelay": 0.002,
            "build": _build_s(ctx), "bands": list(BANDS),
            "rt60": [round(float(r), 4) for r in rt],
            "side_db": _start_gains(live), "mid_db": _start_gains(live_mid),
            "tail_corr": round(float(tc), 4), "_rt_max": rt_max,
            "_live": {"side": live, "mid": live_mid}}


def _build_s(ctx: dict) -> float:
    """Build-up time: the diffuse power peak after the mid power peak."""
    tdif = ctx["P"]["t"][int(np.argmax(ctx["dif"].sum(axis=0)))]
    tmid = ctx["P"]["t"][int(np.argmax(ctx["mid"].sum(axis=0)))]
    return float(np.clip(tdif - tmid, 0.005, 0.08))


def _band_rts(ctx: dict, live: np.ndarray, rt_max: float) -> np.ndarray:
    """RT60 per band from the diffuse decay slope. Bands with no slope get the median."""
    t = ctx["P"]["t"][ctx["frames"]]
    rts = []
    for b in range(len(BANDS)):
        s = _decay_slope(db(ctx["dif"][b, ctx["frames"]]), t) if live[b] else None
        rts.append(-60.0 / s if s else None)
    known = [r for r in rts if r]
    fill = float(np.median(known)) if known else 0.5
    return np.clip([r if r else fill for r in rts], 0.05, rt_max)


def _start_gains(live: np.ndarray) -> list[float]:
    return [0.0 if v else OFF for v in live]


def _iterate(model: dict, src: np.ndarray, ctx: dict, need: dict) -> None:
    t = ctx["P"]["t"][ctx["frames"]]
    for part in ("side", "mid"):
        live = model["_live"][part]
        if not np.any(live):
            continue
        w = wet(model, src, 0, len(src), part)
        bp = uniform_filter1d(band_powers(w), 5, axis=1, mode="nearest")
        ew = bp[:, ctx["frames"]].sum(axis=1)
        gains = np.asarray(model[f"{part}_db"], dtype=float)
        ok = live & (ew > 0) & (need[part] > 0)
        gains[ok] += 10 * np.log10(need[part][ok] / ew[ok])
        if part == "side":
            _rate_fix(model, bp[:, ctx["frames"]], ctx["dif"][:, ctx["frames"]], t, live)
        model[f"{part}_db"] = [round(float(g), 2) for g in np.clip(gains, OFF, 40.0)]


def _rate_fix(model, wet_p, need_p, t, live) -> None:
    rt = np.asarray(model["rt60"], dtype=float)
    for b in np.nonzero(live)[0]:
        sw = _decay_slope(db(wet_p[b]), t)
        sn = _decay_slope(db(need_p[b]), t)
        if sw and sn:
            rt[b] = np.clip(rt[b] * np.clip(sw / sn, 0.5, 2.0), 0.05, model["_rt_max"])
    model["rt60"] = [round(float(r), 4) for r in rt]


def _mid_check(model: dict, a: np.ndarray, src: np.ndarray) -> None:
    if not np.any(model["_live"]["mid"]):
        return
    m = a.mean(axis=1)
    base = src.mean(axis=1)
    with_mid = base + wet(model, src, 0, len(src), "mid")
    if mel_db_distance(m, with_mid)[0] > mel_db_distance(m, base)[0] + 0.02:
        model["mid_db"] = [OFF] * len(BANDS)
        model["_live"]["mid"] = np.zeros(len(BANDS), bool)


def _pool(model, src, span):
    s0, s1 = span
    acc = np.zeros(6)
    for seed in range(4):
        wm = wet(model, src, seed, len(src), "mid")[s0:s1]
        ws = wet(model, src, seed, len(src), "side")[s0:s1]
        a_, b_ = src[s0:s1, 0] + wm, src[s0:s1, 1] + wm
        acc += [a_ @ a_, b_ @ b_, ws @ ws, a_ @ b_, a_ @ ws, b_ @ ws]
    return acc


def _corr_at(acc, g):
    aa, bb, ss, ab, as_, bs = acc
    den = (aa + 2 * g * as_ + g * g * ss) * (bb - 2 * g * bs + g * g * ss)
    return (ab - g * as_ + g * bs - g * g * ss) / np.sqrt(max(den, 1e-300))


def _level_side(model, a, src, span, tc, need, iters, need_db, mid_need_db) -> dict | None:
    best = _side_offset(model, src, span, tc)
    live = model["_live"]["side"]
    model["side_db"] = [round(float(np.clip(g + best, OFF, 40.0)), 2) if live[i] else OFF
                        for i, g in enumerate(model["side_db"])]
    tc_wet = tail_corr(render_space(_public(model), src, 0, limit=False), span)
    if tc_wet is None or abs(tc_wet - tc) >= abs(need["tc_dry"] - tc):
        return None
    out = _public(model)
    out["fit"] = _fit_info(model, need, iters, need_db, mid_need_db, best, tc_wet)
    return out


def _side_offset(model, src, span, tc) -> float:
    """Side gain offset (dB) that best matches the recording's tail correlation."""
    acc = _pool(model, src, span)
    offs = np.arange(-12.0, 12.0001, 0.5)
    cs = [_corr_at(acc, 10 ** (o / 20)) for o in offs]
    return float(offs[int(np.argmin(np.abs(np.asarray(cs) - tc)))])


def _fit_info(model, need, iters, need_db, mid_need_db, best, tc_wet) -> dict:
    return {"need_db": need_db,
            "mid_need_db": mid_need_db if np.any(model["_live"]["mid"]) else None,
            "iters": iters, "dry": need["dry"] is not None, "side_offset_db": best,
            "tail_corr_dry": round(float(need["tc_dry"]), 4),
            "tail_corr_wet": round(float(tc_wet), 4)}


def _public(model: dict) -> dict:
    return {k: v for k, v in model.items() if not k.startswith("_")}


def describe(model: dict | None) -> str:
    if not model:
        return "no space layer"
    side = np.asarray(model["side_db"]) > -140
    mid = np.asarray(model["mid_db"]) > -140
    on = side | mid
    if not np.any(on):
        return "space layer: silent"
    rt = np.asarray(model["rt60"])[on]
    bands = np.asarray(model["bands"])[on]
    parts = "side + mid" if np.any(mid) else "side only"
    return (f"space layer: {parts}, rt60 {rt.min():.2f}-{rt.max():.2f} s over "
            f"{bands.min():.0f}-{bands.max():.0f} Hz, build {model['build'] * 1000:.0f} ms, "
            f"recording tail correlation {model['tail_corr']:.2f}")
