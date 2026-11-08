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


def _stft_power(m: np.ndarray, n_fft: int, hop: int) -> np.ndarray:
    k = max(1, int(math.ceil(len(m) / hop)))
    buf = np.zeros((k - 1) * hop + n_fft)
    seg = m[: len(buf) - n_fft // 2]
    buf[n_fft // 2: n_fft // 2 + len(seg)] = seg
    idx = np.arange(n_fft)[None, :] + hop * np.arange(k)[:, None]
    win = np.hanning(n_fft + 2)[1:-1]
    return np.abs(np.fft.rfft(buf[idx] * win, axis=1)) ** 2


def _band(n_fft: int) -> np.ndarray:
    f = np.arange(n_fft // 2 + 1) * SR / n_fft
    return (f >= 60) & (f <= 16000)


def _peaks(db: np.ndarray, over: float, med_n: int, floor: np.ndarray) -> np.ndarray:
    """Bins that equal the 3-bin max, stand over a running median, and are above a floor."""
    mx = ndimage.maximum_filter1d(db, 3, axis=1, mode="nearest")
    med = ndimage.median_filter(db, size=(1, med_n), mode="nearest")
    return (db >= mx) & (db > med + over) & (db >= floor)


# ---------------------------------------------------------------- envelope measures


def _db(p: np.ndarray) -> np.ndarray:
    return 10 * np.log10(np.maximum(p, 1e-20))


def _t40_frames(e: np.ndarray, pk: int) -> float:
    below = np.nonzero(e[pk:] <= e[pk] - 40)[0]
    return float(below[0]) if len(below) else float(len(e) - 1 - pk)


def _crest_db(m: np.ndarray, e: np.ndarray, pk: int) -> float:
    mean_p = float(np.mean(10 ** (e[e >= e[pk] - 40] / 10)))
    return 20 * math.log10(max(float(np.max(np.abs(m))), 1e-10)) - 10 * math.log10(max(mean_p, 1e-20))


def _rise_db(m: np.ndarray) -> float:
    """The largest 2 ms rise on a 0.5 ms envelope, within 40 dB of its maximum."""
    e = frame_db(m, 24)
    if len(e) <= 4:
        return 0.0
    ok = e[4:] >= e.max() - 40
    return float((e[4:] - e[:-4])[ok].max()) if ok.any() else 0.0


def _env_measures(m: np.ndarray) -> dict:
    e = frame_db(m, 48)
    if len(e) == 0:
        return {"attack_ms": 0.0, "t40_ms": 0.0, "crest_db": 0.0, "rise_db": 0.0}
    pk = int(np.argmax(e))
    on = int(np.argmax(e >= e[pk] - 30))
    return {"attack_ms": float(pk - on), "t40_ms": _t40_frames(e, pk), "crest_db": _crest_db(m, e, pk),
            "rise_db": _rise_db(m)}


def _weighted_flatness(p: np.ndarray, w: np.ndarray) -> float:
    flat = np.exp(np.mean(np.log(np.maximum(p, 1e-30)), axis=1)) / np.maximum(p.mean(axis=1), 1e-30)
    return float(np.exp(np.sum(w * np.log(np.maximum(flat, 1e-30))) / w.sum()))


def _persistence(p: np.ndarray, pk: np.ndarray, mask: np.ndarray, act: np.ndarray) -> float:
    """Share of peak power whose bin is still under the next active frame's mask."""
    both = act[:-1] & act[1:]
    pp = (p[:-1] * pk[:-1])[both]
    den = float(pp.sum())
    return float((pp * mask[1:][both]).sum()) / den if den > 0 else 0.0


def _spectral_measures(m: np.ndarray) -> dict:
    p = _stft_power(m, 2048, 256)[:, _band(2048)]
    en = p.sum(axis=1)
    act = en > en.max() * 10 ** -3.5 if en.max() > 0 else np.zeros(len(en), bool)
    if not act.any():
        return {"tonal_share": 0.0, "flatness": 1.0, "persistence": 0.0}
    db = _db(p)
    pk = _peaks(db, 12, 21, db.max(axis=1, keepdims=True) - 50)
    mask = ndimage.maximum_filter1d(pk.astype(np.uint8), 5, axis=1).astype(bool)
    w = en * act
    share = (p * mask).sum(axis=1) / np.maximum(en, 1e-30)
    return {"tonal_share": float(np.sum(share * w) / w.sum()), "flatness": _weighted_flatness(p, w),
            "persistence": _persistence(p, pk, mask, act)}


def _refined_hz(row: np.ndarray, k: int, f: np.ndarray) -> float:
    """Parabolic peak refinement, only for a negative curvature."""
    if not 0 < k < len(f) - 1:
        return float(f[k])
    a, b, c = row[k - 1], row[k], row[k + 1]
    curv = a - 2 * b + c
    return float(f[k] + 0.5 * (a - c) / curv * (f[1] - f[0])) if curv < 0 else float(f[k])


def _glide_points(m: np.ndarray) -> list:
    sel = _band(1024)
    f = (np.arange(513) * SR / 1024)[sel]
    p = _stft_power(m, 1024, 128)[:, sel]
    db = _db(p)
    en = p.sum(axis=1)
    pk = _peaks(db, 15, 11, np.full((1, 1), db.max() - 50))
    pts = []
    for t in np.nonzero((en > en.max() * 10 ** -3.5) & pk.any(axis=1))[0]:
        k = int(np.argmax(np.where(pk[t], db[t], -np.inf)))
        pts.append((int(t), math.log2(max(_refined_hz(db[t], k, f), 20.0)), float(db[t, k])))
    return pts


def _longest_piece(pts: list) -> list:
    pieces, cur = [], [pts[0]]
    for q in pts[1:]:
        if abs(q[1] - cur[-1][1]) >= 1 / 6 or q[0] - cur[-1][0] > 2.5:
            pieces.append(cur)
            cur = [q]
        else:
            cur.append(q)
    pieces.append(cur)
    return max(pieces, key=len)


def glide(m: np.ndarray) -> tuple[float, float]:
    """(glide_oct, track_ms) of the loudest peak track."""
    pts = _glide_points(m) if len(m) and np.any(m) else []
    if len(pts) < 4:
        return 0.0, 0.0
    top = max(q[2] for q in pts)
    pts = [q for q in pts if q[2] >= top - 15]
    if len(pts) < 4:
        return 0.0, 0.0
    piece = _longest_piece(pts)
    track = (piece[-1][0] - piece[0][0]) * 128 / SR * 1000
    if len(piece) < 4:
        return 0.0, track
    v = [q[1] for q in piece]
    k = max(1, len(v) // 5)
    return float(np.median(v[-k:]) - np.median(v[:k])), track


def _strikes(x: np.ndarray) -> int:
    try:
        return len(_model.module("modal").onsets(x))
    except Exception:
        return 1


def _class(c: dict) -> str:
    if c["gliding"]:
        return "glide"
    if c["tonal_share"] >= 0.8 and c["persistence"] >= 0.9 and c["t40_ms"] > 300:
        return "ringing"
    if c["impulsive"]:
        return "struck"
    if c["attack_ms"] > 40 and c["crest_db"] < 18:
        return "gesture"
    return "mixed"


def character(x: np.ndarray) -> dict:
    """The measurement dict that picks the candidate plan."""
    x = np.asarray(x, dtype=np.float64)
    m = mono(x)
    c = {"dur_s": round(len(m) / SR, 4)}
    c.update(_env_measures(m))
    c.update(_spectral_measures(m))
    c["glide_oct"], c["track_ms"] = glide(m)
    c["strikes"] = _strikes(x)
    c["channels"] = _model.channels_of(x)
    c["impulsive"] = c["attack_ms"] <= 40 and (c["t40_ms"] <= 400 or c["crest_db"] >= 18)
    c["tonal"] = c["tonal_share"] >= 0.45 and c["persistence"] >= 0.5
    c["noisy"] = c["tonal_share"] < 0.25
    c["gliding"] = abs(c["glide_oct"]) >= 2 / 12 and c["track_ms"] >= 15 and c["tonal_share"] >= 0.25
    c["class"] = _class(c)
    return {k: (round(v, 4) if isinstance(v, float) else v) for k, v in c.items()}


def plan(c: dict, max_candidates: int | None = None) -> list:
    """The ordered candidate list of a character."""
    long = c.get("dur_s", 0) >= 1.0
    head = (LONG_PLANS if long else PLANS)[c.get("class", "mixed")]
    order = list(head) + [k for k in ALL if k not in head]
    if "gesture" in order and "sines" in order and order.index("gesture") < order.index("sines"):
        order.remove("gesture")
        order.insert(order.index("sines") + 1, "gesture")
    limit = max_candidates or (4 if long else 7)
    return order[:limit]


# ---------------------------------------------------------------- bed


def _bed_onset(m: np.ndarray) -> int | None:
    e = frame_db(m, 48)
    top = e.max() if len(e) else -200
    for k in range(35, min(120, len(e))):
        if k - 2 <= 3:
            continue
        if e[k] >= top - 30 and e[k] >= np.median(e[3: k - 2]) + 10:
            return k
    return None


def _refine(x: np.ndarray, on: int) -> int:
    try:
        ons = [int(o) for o in _model.module("modal").onsets(x)]
    except Exception:
        return on
    near = [o for o in ons if abs(o - on) <= 144]
    return min(near, key=lambda o: abs(o - on)) if near else on


def _level(seg: np.ndarray) -> float:
    return 10 * math.log10(max(float(np.mean(seg ** 2)), 1e-20))


def bed_of(x: np.ndarray) -> dict | None:
    """A steady pre-roll 12 to 40 dB under the sound, or None."""
    from hark.fit.pipeline.quality import peak_rms_db

    m = mono(np.asarray(x, dtype=np.float64))
    if len(m) < 1440 or not np.any(m):
        return None
    k = _bed_onset(m)
    if k is None:
        return None
    on = _refine(np.asarray(x, dtype=np.float64), k * 48)
    seg = m[144: on - 96]
    if len(seg) < 1440:
        return None
    level = _level(seg) - peak_rms_db(m)
    q = [_level(part) for part in np.array_split(seg, 4)]
    if not (-40 <= level <= -12) or max(q) - min(q) > 6 or q[3] - q[0] > 3:
        return None
    _, _, z = signal.stft(seg, window="hann", nperseg=1024, noverlap=768, boundary=None, padded=False)
    psd = np.median(np.abs(z) ** 2, axis=1)
    return {"t_on": round(on / SR, 6), "level_db": round(level, 2), "n": int(len(m)), "nfft": 1024,
            "psd_db": np.round(10 * np.log10(np.maximum(psd, 1e-30)), 1).tolist()}


def _strip_gain(m: np.ndarray, bed: dict) -> np.ndarray:
    _, _, z = signal.stft(m, window="hann", nperseg=1024, noverlap=768)
    cell = np.abs(z) ** 2
    bp = 10 ** (np.asarray(bed["psd_db"], dtype=np.float64) / 10)[:, None]
    g = np.sqrt(np.maximum(1 - 2 * bp / np.maximum(cell, 1e-30), 10 ** (-25 / 10)))
    pad = np.pad(g, ((0, 0), (1, 1)), mode="edge")
    return (pad[:, :-2] + pad[:, 1:-1] + pad[:, 2:]) / 3


def strip_bed(x: np.ndarray, bed: dict) -> np.ndarray:
    """The recording with its bed spectrally removed and its pre-roll muted."""
    x = np.asarray(x, dtype=np.float64)
    g = _strip_gain(mono(x), bed)
    chans = [x] if x.ndim == 1 else [x[:, c] for c in range(x.shape[1])]
    out = []
    for ch in chans:
        _, _, z = signal.stft(ch, window="hann", nperseg=1024, noverlap=768)
        _, y = signal.istft(z * g, window="hann", nperseg=1024, noverlap=768)
        out.append(np.pad(y, (0, max(0, len(ch) - len(y))))[: len(ch)])
    y = out[0] if x.ndim == 1 else np.column_stack(out)
    on = int(round(float(bed["t_on"]) * SR))
    ramp = np.ones(len(y))
    a, b = max(0, on - 96), max(0, on - 24)
    ramp[:a] = 0.0
    if b > a:
        ramp[a:b] = 0.5 - 0.5 * np.cos(np.pi * np.arange(b - a) / (b - a))
    return y * (ramp if y.ndim == 1 else ramp[:, None])
