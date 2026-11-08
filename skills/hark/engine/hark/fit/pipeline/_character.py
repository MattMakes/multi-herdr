"""The sound's character, the candidate plan, and the bed (detection and strip)."""

from __future__ import annotations

import math

import numpy as np
from scipy import ndimage, signal

from hark.fit.pipeline import _model
from hark.fit.pipeline._post import frame_db, mono

SR = 48000

ALL = ("modal", "sines", "gesture", "spectral", "hybrid:modal+spectral", "hybrid:sines+spectral",
       "hybrid:sines+modal+spectral")
PLANS = {
    "glide": ("sines", "hybrid:sines+modal+spectral", "hybrid:sines+spectral", "modal"),
    "ringing": ("sines", "modal", "hybrid:sines+spectral", "spectral"),
    "struck": ("modal", "hybrid:sines+modal+spectral", "hybrid:modal+spectral", "sines"),
    "gesture": ("spectral", "hybrid:sines+spectral", "modal", "sines"),
    "mixed": ("modal", "spectral", "hybrid:modal+spectral", "sines"),
}
LONG_PLANS = {
    "glide": ("sines", "gesture", "hybrid:sines+modal+spectral", "hybrid:sines+spectral"),
    "ringing": ("sines", "gesture", "modal", "hybrid:sines+spectral"),
    "struck": ("modal", "sines", "hybrid:modal+spectral", "spectral"),
    "gesture": ("sines", "gesture", "modal", "hybrid:sines+spectral"),
    "mixed": ("modal", "sines", "gesture", "hybrid:modal+spectral"),
}


# ---------------------------------------------------------------- spectra


def _power_stft(mv: np.ndarray, fft_n: int, step_n: int) -> np.ndarray:
    ix = max(1, int(math.ceil(len(mv) / step_n)))
    bufv = np.zeros((ix - 1) * step_n + fft_n)
    chunk = mv[: len(bufv) - fft_n // 2]
    bufv[fft_n // 2: fft_n // 2 + len(chunk)] = chunk
    sel_i = np.arange(fft_n)[None, :] + step_n * np.arange(ix)[:, None]
    win2 = np.hanning(fft_n + 2)[1:-1]
    return np.abs(np.fft.rfft(bufv[sel_i] * win2, axis=1)) ** 2


def _band_sum(fft_n: int) -> np.ndarray:
    fq = np.arange(fft_n // 2 + 1) * SR / fft_n
    return (fq >= 60) & (fq <= 16000)


def _peak_list(dbv: np.ndarray, overv: float, n_med: int, floor: np.ndarray) -> np.ndarray:
    """Bins that equal the 3-bin max, stand over a running median, and are above a floor."""
    mxv = ndimage.maximum_filter1d(dbv, 3, axis=1, mode="nearest")
    medv = ndimage.median_filter(dbv, size=(1, n_med), mode="nearest")
    return (dbv >= mxv) & (dbv > medv + overv) & (dbv >= floor)


# ---------------------------------------------------------------- envelope measures


def _to_db(pv: np.ndarray) -> np.ndarray:
    return 10 * np.log10(np.maximum(pv, 1e-20))


def _frames_t40(ev: np.ndarray, pkv: int) -> float:
    under = np.nonzero(ev[pkv:] <= ev[pkv] - 40)[0]
    return float(under[0]) if len(under) else float(len(ev) - 1 - pkv)


def _crest(mv: np.ndarray, ev: np.ndarray, pkv: int) -> float:
    p_mean = float(np.mean(10 ** (ev[ev >= ev[pkv] - 40] / 10)))
    return 20 * math.log10(max(float(np.max(np.abs(mv))), 1e-10)) - 10 * math.log10(max(p_mean, 1e-20))


def _rise_level(mv: np.ndarray) -> float:
    """The largest 2 ms rise on a 0.5 ms envelope, within 40 dB of its maximum."""
    ev = frame_db(mv, 24)
    if len(ev) <= 4:
        return 0.0
    good = ev[4:] >= ev.max() - 40
    return float((ev[4:] - ev[:-4])[good].max()) if good.any() else 0.0


def _env_stats(mv: np.ndarray) -> dict:
    ev = frame_db(mv, 48)
    if len(ev) == 0:
        return {"attack_ms": 0.0, "t40_ms": 0.0, "crest_db": 0.0, "rise_db": 0.0}
    pkv = int(np.argmax(ev))
    start_i = int(np.argmax(ev >= ev[pkv] - 30))
    return {"attack_ms": float(pkv - start_i), "t40_ms": _frames_t40(ev, pkv), "crest_db": _crest(mv, ev, pkv),
            "rise_db": _rise_level(mv)}


def _flatness_w(pv: np.ndarray, wv: np.ndarray) -> float:
    flatv = np.exp(np.mean(np.log(np.maximum(pv, 1e-30)), axis=1)) / np.maximum(pv.mean(axis=1), 1e-30)
    return float(np.exp(np.sum(wv * np.log(np.maximum(flatv, 1e-30))) / wv.sum()))


def _persist(pv: np.ndarray, pkv: np.ndarray, msk: np.ndarray, live: np.ndarray) -> float:
    """Share of peak power whose bin is still under the next active frame's mask."""
    bothv = live[:-1] & live[1:]
    ppv = (pv[:-1] * pkv[:-1])[bothv]
    denom = float(ppv.sum())
    return float((ppv * msk[1:][bothv]).sum()) / denom if denom > 0 else 0.0


def _spec_stats(mv: np.ndarray) -> dict:
    pv = _power_stft(mv, 2048, 256)[:, _band_sum(2048)]
    env_v = pv.sum(axis=1)
    live = env_v > env_v.max() * 10 ** -3.5 if env_v.max() > 0 else np.zeros(len(env_v), bool)
    if not live.any():
        return {"tonal_share": 0.0, "flatness": 1.0, "persistence": 0.0}
    dbv = _to_db(pv)
    pkv = _peak_list(dbv, 12, 21, dbv.max(axis=1, keepdims=True) - 50)
    msk = ndimage.maximum_filter1d(pkv.astype(np.uint8), 5, axis=1).astype(bool)
    wv = env_v * live
    frac = (pv * msk).sum(axis=1) / np.maximum(env_v, 1e-30)
    return {"tonal_share": float(np.sum(frac * wv) / wv.sum()), "flatness": _flatness_w(pv, wv),
            "persistence": _persist(pv, pkv, msk, live)}


def _hz_refined(rw: np.ndarray, ix: int, fq: np.ndarray) -> float:
    """Parabolic peak refinement, only for a negative curvature."""
    if not 0 < ix < len(fq) - 1:
        return float(fq[ix])
    av, bv, cv = rw[ix - 1], rw[ix], rw[ix + 1]
    crv = av - 2 * bv + cv
    return float(fq[ix] + 0.5 * (av - cv) / crv * (fq[1] - fq[0])) if crv < 0 else float(fq[ix])


def _glide_pts(mv: np.ndarray) -> list:
    pick = _band_sum(1024)
    fq = (np.arange(513) * SR / 1024)[pick]
    pv = _power_stft(mv, 1024, 128)[:, pick]
    dbv = _to_db(pv)
    env_v = pv.sum(axis=1)
    pkv = _peak_list(dbv, 15, 11, np.full((1, 1), dbv.max() - 50))
    points = []
    for tv in np.nonzero((env_v > env_v.max() * 10 ** -3.5) & pkv.any(axis=1))[0]:
        ix = int(np.argmax(np.where(pkv[tv], dbv[tv], -np.inf)))
        points.append((int(tv), math.log2(max(_hz_refined(dbv[tv], ix, fq), 20.0)), float(dbv[tv, ix])))
    return points


def _piece_longest(points: list) -> list:
    pc_l, now_v = [], [points[0]]
    for qv in points[1:]:
        if abs(qv[1] - now_v[-1][1]) >= 1 / 6 or qv[0] - now_v[-1][0] > 2.5:
            pc_l.append(now_v)
            now_v = [qv]
        else:
            now_v.append(qv)
    pc_l.append(now_v)
    return max(pc_l, key=len)


def _loud_pts(mv: np.ndarray) -> list:
    """Peak points within 15 dB of the loudest, or [] when fewer than 4 points exist."""
    points = _glide_pts(mv) if len(mv) and np.any(mv) else []
    if len(points) < 4:
        return []
    topv = max(qv[2] for qv in points)
    return [qv for qv in points if qv[2] >= topv - 15]


def _octaves_of_piece(pc: list) -> float:
    if len(pc) < 4:
        return 0.0
    vv = [qv[1] for qv in pc]
    ix = max(1, len(vv) // 5)
    return float(np.median(vv[-ix:]) - np.median(vv[:ix]))


def glide(m: np.ndarray) -> tuple[float, float]:
    """(glide_oct, track_ms) of the loudest peak track."""
    points = _loud_pts(m)
    if len(points) < 4:
        return 0.0, 0.0
    pc = _piece_longest(points)
    trk = (pc[-1][0] - pc[0][0]) * 128 / SR * 1000
    return _octaves_of_piece(pc), trk


def _strike_list(x: np.ndarray) -> int:
    try:
        return len(_model.module("modal").onsets(x))
    except Exception:
        return 1


_RULES_CLASS = (
    ("glide", lambda cv: cv["gliding"]),
    ("ringing", lambda cv: cv["tonal_share"] >= 0.8 and cv["persistence"] >= 0.9 and cv["t40_ms"] > 300),
    ("struck", lambda cv: cv["impulsive"]),
    ("gesture", lambda cv: cv["attack_ms"] > 40 and cv["crest_db"] < 18),
)


def _classify(cv: dict) -> str:
    return next((name for name, rulev in _RULES_CLASS if rulev(cv)), "mixed")


def _flag_list(cv: dict) -> dict:
    return {
        "impulsive": cv["attack_ms"] <= 40 and (cv["t40_ms"] <= 400 or cv["crest_db"] >= 18),
        "tonal": cv["tonal_share"] >= 0.45 and cv["persistence"] >= 0.5,
        "noisy": cv["tonal_share"] < 0.25,
        "gliding": abs(cv["glide_oct"]) >= 2 / 12 and cv["track_ms"] >= 15 and cv["tonal_share"] >= 0.25,
    }


def character(x: np.ndarray) -> dict:
    """The measurement dict that picks the candidate plan."""
    x = np.asarray(x, dtype=np.float64)
    mv = mono(x)
    cv = {"dur_s": round(len(mv) / SR, 4)}
    cv.update(_env_stats(mv))
    cv.update(_spec_stats(mv))
    cv["glide_oct"], cv["track_ms"] = glide(mv)
    cv["strikes"] = _strike_list(x)
    cv["channels"] = _model.channels_of(x)
    cv.update(_flag_list(cv))
    cv["class"] = _classify(cv)
    return {ix: (round(vv, 4) if isinstance(vv, float) else vv) for ix, vv in cv.items()}


def plan(c: dict, max_candidates: int | None = None) -> list:
    """The ordered candidate list of a character."""
    longv = c.get("dur_s", 0) >= 1.0
    headv = (LONG_PLANS if longv else PLANS)[c.get("class", "mixed")]
    ordv = list(headv) + [ix for ix in ALL if ix not in headv]
    if "gesture" in ordv and "sines" in ordv and ordv.index("gesture") < ordv.index("sines"):
        ordv.remove("gesture")
        ordv.insert(ordv.index("sines") + 1, "gesture")
    lim = max_candidates or (4 if longv else 7)
    return ordv[:lim]


# ---------------------------------------------------------------- bed


def _onset_of_bed(mv: np.ndarray) -> int | None:
    ev = frame_db(mv, 48)
    topv = ev.max() if len(ev) else -200
    for ix in range(35, min(120, len(ev))):
        if ix - 2 <= 3:
            continue
        if ev[ix] >= topv - 30 and ev[ix] >= np.median(ev[3: ix - 2]) + 10:
            return ix
    return None


def _refine_start(x: np.ndarray, start_i: int) -> int:
    try:
        ons = [int(ov) for ov in _model.module("modal").onsets(x)]
    except Exception:
        return start_i
    nearv = [ov for ov in ons if abs(ov - start_i) <= 144]
    return min(nearv, key=lambda ov: abs(ov - start_i)) if nearv else start_i


def _level_db(chunk: np.ndarray) -> float:
    return 10 * math.log10(max(float(np.mean(chunk ** 2)), 1e-20))


def _start_of_bed(x: np.ndarray, mv: np.ndarray) -> int | None:
    """The refined sound onset in samples, or None when no bed onset exists."""
    ix = _onset_of_bed(mv) if len(mv) >= 1440 and np.any(mv) else None
    return None if ix is None else _refine_start(x, ix * 48)


def _steady_part(chunk: np.ndarray, lvl: float) -> bool:
    qv = [_level_db(part) for part in np.array_split(chunk, 4)]
    return -40 <= lvl <= -12 and max(qv) - min(qv) <= 6 and qv[3] - qv[0] <= 3


def _psd_level(chunk: np.ndarray) -> list:
    _, _, zv = signal.stft(chunk, window="hann", nperseg=1024, noverlap=768, boundary=None, padded=False)
    psd = np.median(np.abs(zv) ** 2, axis=1)
    return np.round(10 * np.log10(np.maximum(psd, 1e-30)), 1).tolist()


def bed_of(x: np.ndarray) -> dict | None:
    """A steady pre-roll 12 to 40 dB under the sound, or None."""
    from hark.fit.pipeline.quality import peak_rms_db

    x = np.asarray(x, dtype=np.float64)
    mv = mono(x)
    start_i = _start_of_bed(x, mv)
    chunk = mv[144: start_i - 96] if start_i is not None else mv[:0]
    if len(chunk) < 1440:
        return None
    lvl = _level_db(chunk) - peak_rms_db(mv)
    if not _steady_part(chunk, lvl):
        return None
    return {"t_on": round(start_i / SR, 6), "level_db": round(lvl, 2), "n": int(len(mv)), "nfft": 1024,
            "psd_db": _psd_level(chunk)}


def _gain_strip(mv: np.ndarray, bed: dict) -> np.ndarray:
    _, _, zv = signal.stft(mv, window="hann", nperseg=1024, noverlap=768)
    cell = np.abs(zv) ** 2
    bp = 10 ** (np.asarray(bed["psd_db"], dtype=np.float64) / 10)[:, None]
    gv = np.sqrt(np.maximum(1 - 2 * bp / np.maximum(cell, 1e-30), 10 ** (-25 / 10)))
    pad = np.pad(gv, ((0, 0), (1, 1)), mode="edge")
    return (pad[:, :-2] + pad[:, 1:-1] + pad[:, 2:]) / 3


def _channel_strip(chv: np.ndarray, gv: np.ndarray) -> np.ndarray:
    _, _, zv = signal.stft(chv, window="hann", nperseg=1024, noverlap=768)
    _, yv = signal.istft(zv * gv, window="hann", nperseg=1024, noverlap=768)
    return np.pad(yv, (0, max(0, len(chv) - len(yv))))[: len(chv)]


def _ramp_preroll(n: int, start_i: int) -> np.ndarray:
    """0 before on - 2 ms, a raised-cosine rise to on - 0.5 ms, then 1."""
    rmp = np.ones(n)
    av, bv = max(0, start_i - 96), max(0, start_i - 24)
    rmp[:av] = 0.0
    if bv > av:
        rmp[av:bv] = 0.5 - 0.5 * np.cos(np.pi * np.arange(bv - av) / (bv - av))
    return rmp


def strip_bed(x: np.ndarray, bed: dict) -> np.ndarray:
    """The recording with its bed spectrally removed and its pre-roll muted."""
    x = np.asarray(x, dtype=np.float64)
    gv = _gain_strip(mono(x), bed)
    if x.ndim == 1:
        yv = _channel_strip(x, gv)
    else:
        yv = np.column_stack([_channel_strip(x[:, cv], gv) for cv in range(x.shape[1])])
    rmp = _ramp_preroll(len(yv), int(round(float(bed["t_on"]) * SR)))
    return yv * (rmp if yv.ndim == 1 else rmp[:, None])
