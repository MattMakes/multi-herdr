"""Event detection: hits (onsets refined to their attack), swells and level peaks (analysis 2.2, 2.3)."""

from __future__ import annotations

import numpy as np
from scipy import ndimage

from hark.timeline._dsp import SR, band_filter, logmel, mono_of, ms_env, robust_z

HOP = 240
BED_FLOOR = 1e-4


# ---------------------------------------------------------------- onset strength

def onset_strength(m: np.ndarray) -> np.ndarray:
    L = logmel(m, n_fft=1024, hop=HOP, n_mels=96, fmin=30, fmax=16000)
    L = np.maximum(L, L.max() - 80)
    odf = np.zeros(L.shape[1])
    if L.shape[1] > 2:
        ref = ndimage.maximum_filter1d(L, size=3, axis=0, mode="nearest")
        flux = L[:, 2:] - ref[:, :-2]
        odf[2:] = np.maximum(flux, 0).mean(axis=0)
    return odf


def candidates(odf: np.ndarray, zthr: float) -> tuple[np.ndarray, np.ndarray]:
    z = robust_z(odf, 401)
    peak = odf == ndimage.maximum_filter1d(odf, size=11, mode="nearest")
    return np.flatnonzero(peak & (z >= zthr) & (odf > 0)), z


def band_envs(x: np.ndarray, m: np.ndarray) -> dict[str, np.ndarray]:
    return {
        "full": ms_env(x),
        "low": ms_env(band_filter(m, "lowpass", 200.0)),
        "high": ms_env(band_filter(m, "highpass", 2000.0)),
    }


# ---------------------------------------------------------------- attack refine

def _bed(e: np.ndarray, ic: int) -> float:
    pre = e[max(0, ic - 160): max(0, ic - 40)]
    bed = np.median(pre) if pre.size >= 20 else np.median(e[0: max(1, ic + 250)])
    return max(float(bed), BED_FLOOR)


def refine_band(e: np.ndarray, ic: int):
    lo, hi = max(0, ic - 15), min(e.size, ic + 50)
    if hi - lo < 5:
        return None
    ip = lo + int(np.argmax(e[lo:hi]))
    bed = _bed(e, ic)
    thr = bed + 0.1 * (e[ip] - bed)
    j = ip
    stop = max(0, ic - 40)
    while j > stop and e[j - 1] >= thr:
        j -= 1
    return e[ip] / bed, j, ip, bed


def length_ms(e: np.ndarray, j: int, ip: int, bed: float) -> int:
    floor = max(0.1 * e[ip], 1.41 * bed)
    seg = e[ip: ip + 2000] < floor
    k = seg.size
    if seg.size >= 30:
        held = np.convolve(seg.astype(np.int32), np.ones(30, dtype=np.int32), mode="valid")
        hit = np.flatnonzero(held == 30)
        if hit.size:
            k = int(hit[0])
    return int(min(ip + k - j, 2000))


def refine(envs: dict, frame: int, z: float, min_contrast_db: float) -> dict | None:
    ic = int(round(frame * HOP / SR * 1000))
    found = {}
    for band, e in envs.items():
        r = refine_band(e, ic)
        if r is not None:
            found[band] = r
    if not found:
        return None
    best = max(found, key=lambda b: found[b][0])
    contrast = 20 * np.log10(found[best][0])
    if contrast < min_contrast_db:
        return None
    timing = "full" if "full" in found and found["full"][0] >= 4.0 else best
    _, j, ip, bed = found[timing]
    return {"t": j / 1000, "shape": "hit", "band": best, "contrast_db": min(float(contrast), 60.0),
            "z": min(float(z), 99.0), "length_ms": length_ms(envs[timing], j, ip, bed)}


def merge_close(hits: list[dict], min_gap: float) -> list[dict]:
    kept: list[dict] = []
    for h in sorted(hits, key=lambda e: e["t"]):
        if kept and h["t"] - kept[-1]["t"] < min_gap:
            if h["contrast_db"] > kept[-1]["contrast_db"]:
                kept[-1] = h
            continue
        kept.append(h)
    return kept


def lead_event(e: np.ndarray, hits: list[dict]) -> dict | None:
    if e.size <= 5 or e[:50].max() < 1e-3 or any(h["t"] < 0.05 for h in hits):
        return None
    ip = int(np.argmax(e[:50]))
    j = int(np.flatnonzero(e[: ip + 1] >= 0.1 * e[ip])[0])
    return {"t": j / 1000, "shape": "hit", "band": "full", "contrast_db": 60.0, "z": 0.0,
            "length_ms": length_ms(e, j, ip, BED_FLOOR), "starts_file": True}


# ---------------------------------------------------------------- prominence

def k_power_curve(x: np.ndarray) -> np.ndarray:
    from hark.core.features import k_weight

    y = np.asarray(k_weight(np.asarray(x, dtype=np.float64)), dtype=np.float64)
    p = (y ** 2).sum(axis=1) if y.ndim == 2 else y ** 2
    cs = np.concatenate([[0.0], np.cumsum(p)])
    starts = np.arange(0, p.size - 480, 48) if p.size > 480 else np.array([0])
    ends = np.minimum(starts + 480, p.size)
    mean = (cs[ends] - cs[starts]) / np.maximum(ends - starts, 1)
    return 10 * np.log10(mean + 1e-12)


def prominence(curve: np.ndarray, ev: dict) -> None:
    j = int(round(ev["t"] * 1000))
    if ev.get("starts_file"):
        bed = -80.0
    else:
        pre = curve[max(0, j - 300): max(0, j - 20)]
        bed = float(np.median(pre)) if pre.size >= 20 else float(np.percentile(curve[: max(1, j + 2000)], 10))
    bed = max(bed, -80.0)
    ev["prominence_db"] = max(0.0, float(curve[j: j + 80].max()) - bed) if j < curve.size else 0.0
    ev["bed_db"] = round(bed, 1)


# ---------------------------------------------------------------- level envelopes, swells, level peaks

def level_envs(x: np.ndarray) -> dict[str, np.ndarray]:
    """10 ms RMS level envelopes in dB (full and 2 kHz high-pass), 90 ms smoothed."""
    m = mono_of(x)
    n = m.size // 480
    if n < 3:
        return {}
    out = {}
    for band, s in (("full", m), ("high", band_filter(m, "highpass", 2000.0))):
        rms = np.sqrt((s[: n * 480].reshape(n, 480) ** 2).mean(axis=1))
        lvl = np.maximum(20 * np.log10(rms + 1e-9), -80.0)
        out[band] = ndimage.uniform_filter1d(lvl, size=9, mode="nearest")
    return out


def _is_swell(s: np.ndarray, i: int, j: int) -> bool:
    """A rise of 8 dB or more over 0.25 s or more, mostly rising, then a fall of 4 dB or more."""
    after = s[i: i + 100]
    if s[i] - s[j] < 8 or i - j < 25 or after.size < 10 or s[i] - after.min() < 4:
        return False
    return bool(np.mean(np.diff(s[j: i + 1]) > 0) >= 0.6)


def _swell_at(s: np.ndarray, i: int):
    a = max(0, i - 200)
    j = a + int(np.argmin(s[a: i - 15])) if i - a >= 25 else -1
    if j < 0 or not _is_swell(s, i, j):
        return None
    below = np.flatnonzero(s[i: i + 100] < s[i] - 10)
    return j, float(s[i] - s[j]), (i - j) / 100, int(10 * below[0]) if below.size else 1000


def _swell_row(s: np.ndarray, band: str, t: float, r: tuple) -> dict:
    j, rise, dur, length = r
    return {"t": float(t), "shape": "swell", "band": band, "contrast_db": rise, "prominence_db": rise,
            "bed_db": round(float(s[j]), 1), "z": 0.0, "rise_ms": int(dur * 1000), "length_ms": length}


def _taken(t: float, hit_t: np.ndarray, found: list[dict]) -> bool:
    """True when a hit explains the level rise at t, or a swell is already within 0.15 s."""
    if hit_t.size and np.any(np.abs(hit_t - (t - 0.03)) <= 0.05):
        return True
    return any(abs(w["t"] - t) <= 0.15 for w in found)


def _band_swells(s: np.ndarray, band: str, hit_t: np.ndarray, found: list[dict]) -> None:
    peaks = np.flatnonzero((s == ndimage.maximum_filter1d(s, size=31, mode="nearest")) & (s > -70))
    for i in peaks:
        r = _swell_at(s, int(i))
        if r is not None and not _taken(i / 100, hit_t, found):
            found.append(_swell_row(s, band, i / 100, r))


def swells(envs: dict, hits: list[dict]) -> list[dict]:
    if "full" not in envs or envs["full"].size < 60:
        return []
    hit_t = np.array([h["t"] for h in hits])
    found: list[dict] = []
    for band, s in envs.items():
        _band_swells(s, band, hit_t, found)
    return found


def _falls(s: np.ndarray, i: int, min_fall_db: float) -> bool:
    after = s[i: i + 31]
    return bool(after.size >= 5 and s[i] - after.min() >= min_fall_db)


def _band_peak(s: np.ndarray, c: int, rf: int, lf: int, min_rise_db: float, min_fall_db: float):
    """(frame, rise dB) of the local level maximum within rf frames of frame c, or None."""
    lo, hi = max(0, c - rf), min(s.size, c + rf + 1)
    i = lo + int(np.argmax(s[lo:hi])) if hi - lo >= 3 else -1
    ok = i >= 5 and s[i] >= s[max(0, i - lf): i + lf + 1].max() and _falls(s, i, min_fall_db)
    rise = float(s[i] - s[max(0, i - 60): i].min()) if ok else -np.inf
    return (i, rise) if rise >= min_rise_db else None


def level_peak(envs: dict, t: float, radius: float = 0.15, min_rise_db: float = 3.0, min_fall_db: float = 2.0,
               local: float = 0.3) -> dict | None:
    rf, lf, c = int(round(radius * 100)), int(round(local * 100)), int(round(t * 100))
    found = [(r, band) for band, s in envs.items() if (r := _band_peak(s, c, rf, lf, min_rise_db, min_fall_db))]
    if not found:
        return None
    (i, rise), band = max(found, key=lambda f: f[0][1])
    return {"t": round(i / 100, 3), "band": band, "rise_db": round(rise, 1)}


# ---------------------------------------------------------------- per-event levels

def excess_centroid(m: np.ndarray, t: float, length_s: float) -> int | None:
    a = int(t * SR)
    n = int(min(max(length_s, 0.03), 0.12) * SR)
    seg = m[a: a + n]
    if seg.size < 256:
        return None
    P = _power(seg)
    pre = m[max(0, a - int(0.15 * SR)): max(0, a - int(0.01 * SR))]
    if pre.size >= 256:
        D = np.maximum(P - _power(pre), 0)
        if D.sum() > 0.2 * P.sum():
            P = D
    f = np.fft.rfftfreq(4096, 1 / SR)
    keep = (f >= 20) & (f <= 20000)
    return int((f[keep] * P[keep]).sum() / (P[keep].sum() + 1e-30))


def _power(seg: np.ndarray) -> np.ndarray:
    w = seg * np.hanning(seg.size + 1)[:-1]
    X = np.fft.rfft(w[:4096], n=4096)
    return (X.real ** 2 + X.imag ** 2) / seg.size


def event_levels(x: np.ndarray, m: np.ndarray, ev: dict, mom: tuple) -> None:
    from hark.timeline._dsp import db

    a = np.abs(np.asarray(x, dtype=np.float64))
    if a.ndim == 2:
        a = a.max(axis=1)
    d = min(max(ev["length_ms"] / 1000, 0.05), 0.3)
    i0 = int(ev["t"] * SR)
    seg = a[i0: i0 + max(1, int(d * SR))]
    ev["peak_dbfs"] = round(float(db(seg.max() if seg.size else 0.0)), 1)
    ev["local_lufs"] = round(float(np.interp(ev["t"], mom[0], mom[1])), 1)
    ev["centroid_hz"] = excess_centroid(m, ev["t"], ev["length_ms"] / 1000)
    ev["t"] = round(ev["t"], 3)
    for k in ("prominence_db", "contrast_db", "z"):
        ev[k] = round(float(ev[k]), 1)


def momentary_points(x: np.ndarray) -> tuple:
    from hark.core.features import loudness_curve

    if len(x) <= 0.4 * SR:
        return np.array([0.0]), np.array([-70.0])
    t, L = loudness_curve(x, 0.4, 0.05)
    return np.asarray(t, dtype=float), np.asarray(L, dtype=float)


# ---------------------------------------------------------------- the detector

def detect_events(x: np.ndarray, zthr: float = 3.0, min_contrast_db: float = 3.0, min_gap: float = 0.03,
                  lead: bool = True) -> list[dict]:
    """Hits and swells of x, sorted by time, times relative to x."""
    m = mono_of(x)
    if m.size < 0.1 * SR:
        return []
    frames, z = candidates(onset_strength(m), zthr)
    envs = band_envs(x, m)
    hits = [h for h in (refine(envs, int(f), z[f], min_contrast_db) for f in frames) if h is not None]
    hits = merge_close(hits, min_gap)
    first = lead_event(envs["full"], hits) if lead else None
    if first is not None:
        hits.insert(0, first)
    curve = k_power_curve(x)
    for h in hits:
        prominence(curve, h)
    events = sorted(hits + swells(level_envs(x), hits), key=lambda e: e["t"])
    mom = momentary_points(x)
    for ev in events:
        event_levels(x, m, ev, mom)
    return events
