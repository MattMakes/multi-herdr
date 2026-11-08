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
    av = np.asarray(x, dtype=float)
    if av.ndim != 2 or av.shape[1] < 2:
        return False
    mv = (av[:, 0] + av[:, 1]) / 2
    sv = (av[:, 0] - av[:, 1]) / 2
    emv, esv = float(np.sum(mv ** 2)), float(np.sum(sv ** 2))
    if esv <= 0:
        return False
    return bool(emv <= 0 or 10 * np.log10(esv / emv) > tol_db)


def _frame_grid(mv: np.ndarray, fft_n: int, step_n: int) -> np.ndarray:
    ix = len(mv) // step_n + 1
    bufv = np.zeros((ix - 1) * step_n + fft_n)
    bufv[fft_n // 2:fft_n // 2 + len(mv)] = mv[:len(bufv) - fft_n // 2]
    winv = np.hanning(fft_n + 2)[1:-1]
    sel_i = np.arange(ix)[:, None] * step_n + np.arange(fft_n)[None, :]
    return np.fft.rfft(bufv[sel_i] * winv, axis=1)


def side_powers(x: Any, centers=BANDS, nfft: int = 1024, hop: int = 240) -> dict:
    """Per band and frame: mid power PM, side power PS and Re(S conj(M)) as C."""
    av = as_stereo(x)
    fmv = _frame_grid((av[:, 0] + av[:, 1]) / 2, nfft, hop)
    fsv = _frame_grid((av[:, 0] - av[:, 1]) / 2, nfft, hop)
    wv = tri_masks(centers, nfft).T
    return {"t": np.arange(fmv.shape[0]) * hop / SR,
            "PM": (np.abs(fmv) ** 2 @ wv).T,
            "PS": (np.abs(fsv) ** 2 @ wv).T,
            "C": (np.real(fsv * np.conj(fmv)) @ wv).T}


def diffuse_power(P: dict) -> np.ndarray:
    return np.maximum(P["PS"] - P["C"] ** 2 / np.maximum(P["PM"], 1e-30), 0.0)


def band_powers(m: np.ndarray, centers=BANDS, nfft: int = 1024, hop: int = 240) -> np.ndarray:
    """Per band x frame power of a mono signal."""
    f = _frame_grid(np.asarray(m, dtype=float), nfft, hop)
    return (np.abs(f) ** 2 @ tri_masks(centers, nfft).T).T


def _env_per_ms(mv: np.ndarray) -> np.ndarray:
    cnt = len(mv)
    cv = np.concatenate([[0.0], np.cumsum(mv ** 2)])
    cen_l = np.arange(0, cnt, 48)
    low_i = np.clip(cen_l - 120, 0, cnt)
    high_i = np.clip(cen_l + 120, 0, cnt)
    return (cv[high_i] - cv[low_i]) / np.maximum(high_i - low_i, 1)


def active_span(x: Any) -> tuple[int, int, int]:
    """(onset, peak, end) sample indices of the active part."""
    av = np.asarray(x, dtype=float)
    mv = av.mean(axis=1) if av.ndim == 2 else av
    cnt = len(mv)
    envl = _env_per_ms(mv)
    if len(envl) < 3:
        return 0, 0, cnt
    envl = uniform_filter1d(envl, 10, mode="nearest")
    ipv = int(np.argmax(envl))
    pkv = float(envl[ipv])
    if pkv <= 0:
        return 0, 0, cnt
    thrv = _on_threshold(envl, pkv)
    ons = int(np.argmax(envl >= pkv * 1e-3))
    over = np.nonzero(db(envl) >= db(pkv) + thrv)[0]
    last_i = int(over[-1]) if len(over) else ipv
    return ons * 48, ipv * 48, min(cnt, last_i * 48 + 48)


def _on_threshold(envl: np.ndarray, pkv: float) -> float:
    if len(envl) < 80:
        return -40.0
    dv = db(envl)
    flv = min(float(np.median(dv[5:50])), float(np.median(dv[-25:-12])))
    return float(np.clip(flv + 3 - float(db(pkv)), -45.0, -12.0))


def tail_span(x: Any, start_s: float = 0.03, min_s: float = 0.02) -> tuple[int, int] | None:
    _, pkv2, stop_i = active_span(x)
    first_i = pkv2 + int(round(start_s * SR))
    if stop_i - first_i < min_s * SR:
        return None
    return first_i, stop_i


def corr(L: np.ndarray, R: np.ndarray) -> float | None:
    elv, erv = float(np.sum(L * L)), float(np.sum(R * R))
    if elv < 1e-20 or erv < 1e-20:
        return None
    return float(np.sum(L * R) / np.sqrt(elv * erv))


def tail_corr(x: Any, span: tuple[int, int] | None = None) -> float | None:
    av = as_stereo(x)
    span = span or tail_span(av)
    if span is None:
        return None
    return corr(av[span[0]:span[1], 0], av[span[0]:span[1], 1])


def needs_space(x: Any, max_corr: float = 0.7) -> bool:
    av = np.asarray(x, dtype=float)
    if not is_stereo(av):
        return False
    tcorr = tail_corr(av)
    return tcorr is not None and tcorr <= max_corr


# ---------------------------------------------------------------- layer

def _is_on(model: dict, piece: str) -> np.ndarray:
    return np.asarray(model.get(f"{piece}_db", [OFF] * len(model["bands"])), dtype=float) > -140


def impulse(model: dict, part: str = "side", seed: int = 0, flat_iters: int = 4) -> np.ndarray:
    """The impulse response of one part (unit-energy bands at their gains)."""
    gain_l = np.asarray(model[f"{part}_db"], dtype=float)
    start_i = gain_l > -140
    res = np.zeros(1)
    if np.any(start_i):
        res = _bands_of_impulse(model, part, seed, flat_iters, gain_l, start_i)
    return res


def _bands_of_impulse(model, piece, seed, n_flat, gain_l, start_i) -> np.ndarray:
    rtv = np.clip(np.asarray(model["rt60"], dtype=float), 0.02, 3.0)
    tv, rise = _rise_curve(model, float(rtv[start_i].max()))
    len_n = len(tv)
    rv = np.random.default_rng((int(seed) * 7919 + SALT[piece]) & 0xFFFFFFFF)
    fft_n = next_fast_len(2 * len_n)
    specv = rfft(rv.standard_normal(fft_n))
    mask_l = tri_masks(model["bands"], fft_n)
    res = np.zeros(len_n)
    for bv in np.nonzero(start_i)[0]:
        envl = rise * np.exp(-6.907755 * np.maximum(tv, 0) / rtv[bv])
        res += _band_unit(specv, mask_l[bv], envl, fft_n, n_flat) * 10.0 ** (gain_l[bv] / 20.0)
    return res


def _rise_curve(model: dict, max_rt: float) -> tuple[np.ndarray, np.ndarray]:
    """Times from the predelay and the build-up curve over the impulse length."""
    bld = max(float(model.get("build", 1e-4)), 1e-4)
    prev = float(model.get("predelay", 0.002))
    len_n = int((prev + bld + min(3.0, max_rt)) * SR) + 1
    tv = np.arange(len_n) / SR - prev
    return tv, np.where(tv >= 0, 1.0 - np.exp(-np.maximum(tv, 0) / bld), 0.0)


def _band_unit(specv, msk, envl, fft_n, n_flat) -> np.ndarray:
    hv = irfft(specv * msk, fft_n)[:len(envl)] * envl
    hv = _flat_spec(hv, msk, envl, fft_n, n_flat)
    ev = np.sqrt(np.sum(hv ** 2))
    return hv / ev if ev > 0 else np.zeros(len(hv))


def _flat_spec(hv: np.ndarray, msk: np.ndarray, envl: np.ndarray, fft_n: int, n_iter: int) -> np.ndarray:
    for _ in range(n_iter):
        spv = rfft(hv, fft_n)
        spv = msk * np.exp(1j * np.angle(spv))
        hv = irfft(spv, fft_n)[:len(hv)] * envl
    return hv


def wet(model: dict, dry: np.ndarray, seed: int = 0, n: int | None = None,
        part: str = "side") -> np.ndarray:
    """One part of the wet layer: the dry's channel mean convolved with its impulse."""
    from scipy.signal import fftconvolve
    dv = np.asarray(dry, dtype=float)
    mv = dv.mean(axis=1) if dv.ndim == 2 else dv
    n = len(mv) if n is None else int(n)
    hv = impulse(model, part, seed)
    if not np.any(hv) or len(mv) == 0:
        return np.zeros(n)
    yv = fftconvolve(mv, hv)
    return np.pad(yv, (0, max(0, n - len(yv))))[:n]


def tail_s(model: dict) -> float:
    start_i = _is_on(model, "side") | _is_on(model, "mid")
    if not np.any(start_i):
        return 0.0
    rtv = np.clip(np.asarray(model["rt60"], dtype=float), 0.02, 3.0)
    return float(model.get("predelay", 0.002) + max(model.get("build", 1e-4), 1e-4)
                 + min(3.0, float(rtv[start_i].max())))


def _split_bands(x: np.ndarray, fft_n: int, mask_l: np.ndarray) -> np.ndarray:
    spv = rfft(x, fft_n)
    return np.array([irfft(spv * mv, fft_n)[:len(x)] for mv in mask_l])


def _ortho(wsv: np.ndarray, ref_l: list[np.ndarray], band_l) -> np.ndarray:
    cnt = len(wsv)
    fft_n = next_fast_len(cnt + 8192)
    mask_l = tri_masks(band_l, fft_n)
    wbv = _split_bands(wsv, fft_n, mask_l)
    rbv = [_split_bands(rv, fft_n, mask_l) for rv in ref_l]
    for bv in range(len(mask_l)):
        base_m: list[np.ndarray] = []
        for rv in rbv:
            vv = rv[bv].copy()
            for uv in base_m:
                vv -= np.dot(vv, uv) * uv
            nvv = np.linalg.norm(vv)
            if nvv >= 1e-12:
                base_m.append(vv / nvv)
        for uv in base_m:
            wbv[bv] -= np.dot(wbv[bv], uv) * uv
    return wbv.sum(axis=0)


def render_space(model: dict | None, dry: np.ndarray, seed: int = 0, width: float = 1,
                 ring: bool = False, limit: bool = True) -> np.ndarray:
    """Dry plus wet as (n, 2). A null model returns the dry with no finish."""
    yv = as_stereo(dry)
    if model and len(yv):
        yv = _add_space(model, yv, seed, width, ring, limit)
    return yv


def _add_space(model: dict, yv: np.ndarray, seed: int, width: float, ringv: bool, lim: bool) -> np.ndarray:
    if ringv:
        yv = np.vstack([yv, np.zeros((int(tail_s(model) * SR) + 1, 2))])
    cnt = len(yv)
    wmv = _wet_part(model, yv, seed, cnt, "mid")
    wsv = _wet_part(model, yv, seed, cnt, "side")
    if np.any(wsv):
        ref_l = [(yv[:, 0] - yv[:, 1]) / 2, (yv[:, 0] + yv[:, 1]) / 2 + wmv]
        wsv = _ortho(wsv, ref_l, model["bands"]) * width
    yv = yv + np.column_stack([wmv + wsv, wmv - wsv])
    return finish(yv, SR, 0.0, -1.0, lim)


def _wet_part(model: dict, yv: np.ndarray, seed: int, cnt: int, piece: str) -> np.ndarray:
    return wet(model, yv, seed, cnt, piece) if np.any(_is_on(model, piece)) else np.zeros(cnt)


# ---------------------------------------------------------------- fit

def fit_space(x: Any, sr: int = 48000, dry: np.ndarray | None = None, max_corr: float = 0.7,
              iters: int = 3, need_db: float = -15, mid_need_db: float = -10) -> dict | None:
    """A space model for a stereo recording with a decorrelated tail, or None."""
    av = as_stereo(x) if isinstance(x, str) else as_stereo(resample(np.asarray(x, float), sr))
    stv = _begin_space(av, dry, max_corr, mid_need_db)
    res = None
    if stv is not None and stv[3]["rel"] >= need_db:
        res = _end_space(av, *stv, iters, need_db, mid_need_db)
    return res


def _begin_space(av: np.ndarray, dry, max_corr: float, mid_want_db: float):
    """(span, tail corr, context, need) when a has a decorrelated tail to model, else None."""
    spn = tail_span(av) if is_stereo(av) else None
    tcorr = tail_corr(av, spn) if spn else None
    if tcorr is None or tcorr > max_corr:
        return None
    state = _tail_state(av, spn)
    want = None if state is None else _need_calc(state, av, dry, spn, tcorr, mid_want_db)
    return None if want is None else (spn, tcorr, state, want)


def _end_space(av, spn, tcorr, state, want, n_iter, want_db, mid_want_db) -> dict | None:
    model = _seed_model(state, want, spn, tcorr)
    srcv = want["dry"] if want["dry"] is not None else av
    for _ in range(n_iter):
        _refine_loop(model, srcv, state, want)
    _check_mid(model, av, srcv)
    return _side_level(model, av, srcv, spn, tcorr, want, n_iter, want_db, mid_want_db)


def _tail_state(av: np.ndarray, spn: tuple[int, int]) -> dict | None:
    pmat = side_powers(av)
    frm = (pmat["t"] * SR >= spn[0]) & (pmat["t"] * SR < spn[1])
    if np.count_nonzero(frm) < 3:
        return None
    diffv = uniform_filter1d(diffuse_power(pmat), 5, axis=1, mode="nearest")
    midv = uniform_filter1d(pmat["PM"], 5, axis=1, mode="nearest")
    return {"P": pmat, "frames": frm, "dif": diffv, "mid": midv,
            "E_dif": diffv[:, frm].sum(axis=1), "E_mid": midv[:, frm].sum(axis=1)}


def _need_calc(state: dict, av, dry, spn, tcorr, mid_want_db) -> dict | None:
    pieces = (state["E_dif"].copy(), np.zeros(len(BANDS)), 1.0, None)
    if dry is not None:
        pieces = _needs_of_dry(state, av, dry, spn, tcorr, mid_want_db)
    return None if pieces is None else _need_record(state, *pieces)


def _needs_of_dry(state: dict, av, dry, spn, tcorr, mid_want_db):
    """(side need, mid need, dry tail corr, padded dry), or None when the dry is not drier."""
    dv2 = as_stereo(dry)
    dv2 = np.pad(dv2, ((0, max(0, len(av) - len(dv2))), (0, 0)))[:len(av)]
    tcorr_dry = tail_corr(dv2, spn)
    if tcorr_dry is None or tcorr_dry <= tcorr + 0.1:
        return None
    side_want, mid_want = state["E_dif"].copy(), np.zeros(len(BANDS))
    dstate = _tail_state(dv2, spn)
    if dstate is not None:
        side_want = np.maximum(state["E_dif"] - dstate["E_dif"], 0.0)
        shrt = np.maximum(state["E_mid"] - dstate["E_mid"], 0.0)
        mid_want = np.where(shrt >= state["E_mid"] * 10 ** (mid_want_db / 10), shrt, 0.0)
    return side_want, mid_want, tcorr_dry, dv2


def _need_record(state: dict, side_want, mid_want, tcorr_dry, dv2) -> dict | None:
    totv = float(side_want.sum())
    if totv <= 0 or state["E_dif"].sum() <= 0:
        return None
    relv = float(db(totv) - db(state["E_dif"].sum()))
    return {"side": side_want, "mid": mid_want, "rel": relv, "tc_dry": tcorr_dry, "dry": dv2}


def _slope_of_decay(crv_db: np.ndarray, tv: np.ndarray) -> float | None:
    pick = crv_db >= crv_db.max() - 30
    if np.count_nonzero(pick) < 4:
        return None
    slp = float(np.polyfit(tv[pick], crv_db[pick], 1)[0])
    return slp if slp < -1 else None


def _seed_model(state: dict, want: dict, spn, tcorr) -> dict:
    alive = want["side"] > want["side"].max() * 1e-4
    max_rt = float(np.clip(4 * (spn[1] - spn[0]) / SR, 0.1, 3.0))
    rtv = _rt_per_band(state, alive, max_rt)
    alive_mid = want["mid"] > max(want["mid"].max(), 1e-300) * 1e-4
    alive_mid &= want["mid"] > 0
    return {"kind": "space", "version": VERSION, "predelay": 0.002,
            "build": _build_side(state), "bands": list(BANDS),
            "rt60": [round(float(rv), 4) for rv in rtv],
            "side_db": _seed_gains(alive), "mid_db": _seed_gains(alive_mid),
            "tail_corr": round(float(tcorr), 4), "_rt_max": max_rt,
            "_live": {"side": alive, "mid": alive_mid}}


def _build_side(state: dict) -> float:
    """Build-up time: the diffuse power peak after the mid power peak."""
    tdiff = state["P"]["t"][int(np.argmax(state["dif"].sum(axis=0)))]
    tmidv = state["P"]["t"][int(np.argmax(state["mid"].sum(axis=0)))]
    return float(np.clip(tdiff - tmidv, 0.005, 0.08))


def _rt_per_band(state: dict, alive: np.ndarray, max_rt: float) -> np.ndarray:
    """RT60 per band from the diffuse decay slope. Bands with no slope get the median."""
    tv = state["P"]["t"][state["frames"]]
    rt_l = []
    for bv in range(len(BANDS)):
        sv = _slope_of_decay(db(state["dif"][bv, state["frames"]]), tv) if alive[bv] else None
        rt_l.append(-60.0 / sv if sv else None)
    knw = [rv for rv in rt_l if rv]
    fillv = float(np.median(knw)) if knw else 0.5
    return np.clip([rv if rv else fillv for rv in rt_l], 0.05, max_rt)


def _seed_gains(alive: np.ndarray) -> list[float]:
    return [0.0 if vv else OFF for vv in alive]


def _refine_loop(model: dict, srcv: np.ndarray, state: dict, want: dict) -> None:
    tv = state["P"]["t"][state["frames"]]
    for piece in ("side", "mid"):
        alive = model["_live"][piece]
        if not np.any(alive):
            continue
        wv = wet(model, srcv, 0, len(srcv), piece)
        bpv = uniform_filter1d(band_powers(wv), 5, axis=1, mode="nearest")
        ewv = bpv[:, state["frames"]].sum(axis=1)
        gain_l = np.asarray(model[f"{piece}_db"], dtype=float)
        good = alive & (ewv > 0) & (want[piece] > 0)
        gain_l[good] += 10 * np.log10(want[piece][good] / ewv[good])
        if piece == "side":
            _fix_rate(model, bpv[:, state["frames"]], state["dif"][:, state["frames"]], tv, alive)
        model[f"{piece}_db"] = [round(float(gv), 2) for gv in np.clip(gain_l, OFF, 40.0)]


def _fix_rate(model, wetp, wantp, tv, alive) -> None:
    rtv = np.asarray(model["rt60"], dtype=float)
    for bv in np.nonzero(alive)[0]:
        swv = _slope_of_decay(db(wetp[bv]), tv)
        snv = _slope_of_decay(db(wantp[bv]), tv)
        if swv and snv:
            rtv[bv] = np.clip(rtv[bv] * np.clip(swv / snv, 0.5, 2.0), 0.05, model["_rt_max"])
    model["rt60"] = [round(float(rv), 4) for rv in rtv]


def _check_mid(model: dict, av: np.ndarray, srcv: np.ndarray) -> None:
    if not np.any(model["_live"]["mid"]):
        return
    mv = av.mean(axis=1)
    basev = srcv.mean(axis=1)
    has_mid = basev + wet(model, srcv, 0, len(srcv), "mid")
    if mel_db_distance(mv, has_mid)[0] > mel_db_distance(mv, basev)[0] + 0.02:
        model["mid_db"] = [OFF] * len(BANDS)
        model["_live"]["mid"] = np.zeros(len(BANDS), bool)


def _pooled(model, srcv, spn):
    sv0, sv1 = spn
    accum = np.zeros(6)
    for seed in range(4):
        wmv = wet(model, srcv, seed, len(srcv), "mid")[sv0:sv1]
        wsv = wet(model, srcv, seed, len(srcv), "side")[sv0:sv1]
        av_, bv_ = srcv[sv0:sv1, 0] + wmv, srcv[sv0:sv1, 1] + wmv
        accum += [av_ @ av_, bv_ @ bv_, wsv @ wsv, av_ @ bv_, av_ @ wsv, bv_ @ wsv]
    return accum


def _corr_window(accum, gv):
    aav, bbv, ssv, abv, asv, bsv = accum
    denom = (aav + 2 * gv * asv + gv * gv * ssv) * (bbv - 2 * gv * bsv + gv * gv * ssv)
    return (abv - gv * asv + gv * bsv - gv * gv * ssv) / np.sqrt(max(denom, 1e-300))


def _side_level(model, av, srcv, spn, tcorr, want, n_iter, want_db, mid_want_db) -> dict | None:
    winner = _offset_side(model, srcv, spn, tcorr)
    alive = model["_live"]["side"]
    model["side_db"] = [round(float(np.clip(gv + winner, OFF, 40.0)), 2) if alive[ii] else OFF
                        for ii, gv in enumerate(model["side_db"])]
    tcorr_wet = tail_corr(render_space(_public_view(model), srcv, 0, limit=False), spn)
    if tcorr_wet is None or abs(tcorr_wet - tcorr) >= abs(want["tc_dry"] - tcorr):
        return None
    res = _public_view(model)
    res["fit"] = _info_of_fit(model, want, n_iter, want_db, mid_want_db, winner, tcorr_wet)
    return res


def _offset_side(model, srcv, spn, tcorr) -> float:
    """Side gain offset (dB) that best matches the recording's tail correlation."""
    accum = _pooled(model, srcv, spn)
    off_l = np.arange(-12.0, 12.0001, 0.5)
    csv = [_corr_window(accum, 10 ** (ov / 20)) for ov in off_l]
    return float(off_l[int(np.argmin(np.abs(np.asarray(csv) - tcorr)))])


def _info_of_fit(model, want, n_iter, want_db, mid_want_db, winner, tcorr_wet) -> dict:
    return {"need_db": want_db,
            "mid_need_db": mid_want_db if np.any(model["_live"]["mid"]) else None,
            "iters": n_iter, "dry": want["dry"] is not None, "side_offset_db": winner,
            "tail_corr_dry": round(float(want["tc_dry"]), 4),
            "tail_corr_wet": round(float(tcorr_wet), 4)}


def _public_view(model: dict) -> dict:
    return {ix: vv for ix, vv in model.items() if not ix.startswith("_")}


def describe(model: dict | None) -> str:
    if not model:
        return "no space layer"
    side_v = np.asarray(model["side_db"]) > -140
    midv = np.asarray(model["mid_db"]) > -140
    start_i = side_v | midv
    if not np.any(start_i):
        return "space layer: silent"
    rtv = np.asarray(model["rt60"])[start_i]
    band_l = np.asarray(model["bands"])[start_i]
    pieces = "side + mid" if np.any(midv) else "side only"
    return (f"space layer: {pieces}, rt60 {rtv.min():.2f}-{rtv.max():.2f} s over "
            f"{band_l.min():.0f}-{band_l.max():.0f} Hz, build {model['build'] * 1000:.0f} ms, "
            f"recording tail correlation {model['tail_corr']:.2f}")
