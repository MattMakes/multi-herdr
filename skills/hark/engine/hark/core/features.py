"""Model-free measurements of a sound: levels, loudness, envelope, spectrum, pitch, rhythm, key.

Everything here runs at 48 kHz. librosa is imported only inside the functions
that need it, so ``measure(light=True)`` never loads it.
"""

from __future__ import annotations

import argparse
import os
from typing import Any

import numpy as np

from hark.core import audio, report
from hark.core.safe import attempt

SR = 48000
LIBROSA_SR = 22050

BANDS: dict[str, tuple[float, float]] = {
    "sub": (20.0, 60.0),
    "low": (60.0, 250.0),
    "lowmid": (250.0, 1000.0),
    "mid": (1000.0, 4000.0),
    "presence": (4000.0, 8000.0),
    "air": (8000.0, 16000.0),
    "top": (16000.0, 24000.0),
}

K_SHELF_48K = ((1.53512485958697, -2.69169618940638, 1.19839281085285), (1.0, -1.69065929318241, 0.73248077421585))
K_HIGHPASS_48K = ((1.0, -2.0, 1.0), (1.0, -1.99004745483398, 0.99007225036621))

KEY_MAJOR = (6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88)
KEY_MINOR = (6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17)
NOTE_NAMES = ("C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B")

MIN_SAMPLES = 240
LUFS_FROM_S = 0.4
SHORT_TERM_FROM_S = 3.0
LRA_FROM_S = 6.0
MUSIC_FROM_S = 5.0
ABS_GATE = -70.0
CLIP = 0.999
DIGITAL_ZERO = 1e-6
ONSETS_LISTED = 200
TONALITY_S = 30.0


def _r(v: float | None, digits: int) -> float | None:
    return None if v is None else round(float(v), digits)


def _signal():
    import scipy.signal

    return scipy.signal


# ---------------------------------------------------------------- envelope


def _centred_bounds(n: int, w2: int) -> tuple[np.ndarray, np.ndarray]:
    """[a, b) of the window around each 1 ms centre 48 k + 24, clipped to [0, n]."""
    centres = 48 * np.arange((n + 47) // 48) + 24
    return np.clip(centres - w2, 0, n), np.clip(centres + w2, 0, n)


def smooth_env(m: np.ndarray, win_ms: float) -> np.ndarray:
    """Centred RMS of win_ms, one value per millisecond."""
    m = np.asarray(m, dtype=np.float64)
    a, b = _centred_bounds(len(m), max(1, int(win_ms * SR / 2000)))
    csum = np.concatenate([[0.0], np.cumsum(m * m)])
    return np.sqrt((csum[b] - csum[a]) / np.maximum(b - a, 1) + 1e-20)


def _rms_rows(frames: np.ndarray) -> np.ndarray:
    return np.sqrt(np.mean(frames * frames, axis=-1) + 1e-20)


def frame_rms(m: np.ndarray, win: int) -> np.ndarray:
    """RMS of non-overlapping frames of win samples; one value when shorter."""
    m = np.asarray(m, dtype=np.float64)
    if len(m) < win:
        return np.atleast_1d(_rms_rows(m))
    return _rms_rows(m[: len(m) - len(m) % win].reshape(-1, win))


def _decay_ms(e10: np.ndarray, ip: int) -> float | None:
    tail = e10[ip:]
    padded = np.concatenate([tail, np.full(20, tail[-1])])
    ahead = np.lib.stride_tricks.sliding_window_view(padded, 21).max(axis=1)[: len(tail)]
    below = np.nonzero(ahead < e10[ip] * 10 ** (-30 / 20))[0]
    return float(below[0]) if below.size else None


def envelope_times(m: np.ndarray) -> dict:
    """attack_at, peak_at (s) and attack_ms, decay_ms (ms or None) of a mono mix."""
    e5, e10 = smooth_env(m, 5), smooth_env(m, 10)
    i0 = int(np.argmax(e5 >= 0.1 * e5.max()))
    rise = e5[i0: i0 + 300]
    attack = float(np.argmax(rise >= 0.9 * rise.max())) if rise.size else None
    ip = int(np.argmax(e10))
    return {"attack_at": i0 / 1000, "peak_at": ip / 1000, "attack_ms": attack, "decay_ms": _decay_ms(e10, ip)}


# ---------------------------------------------------------------- levels and loudness


def _as_stereo(x: np.ndarray) -> np.ndarray:
    x = np.asarray(x, dtype=np.float64)
    return np.column_stack([x, x]) if x.ndim == 1 else x


def true_peak_dbtp(x: np.ndarray) -> float:
    """Peak of the 4x oversampled signal in dB."""
    up = _signal().resample_poly(np.asarray(x, dtype=np.float64), 4, 1, axis=0)
    return audio.db(float(np.max(np.abs(up))) if up.size else 0.0)


_SHELF_HZ = 1681.974450955533
_SHELF_GAIN_DB = 3.999843853973347
_SHELF_Q = 0.7071752369554196
_SHELF_VB_EXP = 0.4996667741545416
_HIGHPASS_HZ = 38.13547087602444
_HIGHPASS_Q = 0.5003270373238773


def _denominator(k: float, q: float) -> tuple[float, tuple[float, float, float]]:
    """a0 and the normalised denominator of a bilinear biquad with prewarped k."""
    a0 = 1 + k / q + k * k
    return a0, (1.0, 2 * (k * k - 1) / a0, (1 - k / q + k * k) / a0)


def _shelf_numerator(k: float, a0: float) -> tuple[float, float, float]:
    vh = 10 ** (_SHELF_GAIN_DB / 20)
    vbq = vh ** _SHELF_VB_EXP * k / _SHELF_Q
    return (vh + vbq + k * k) / a0, 2 * (k * k - vh) / a0, (vh - vbq + k * k) / a0


def _shelf(sr: int) -> tuple[tuple, tuple]:
    """BS.1770 stage 1 (high shelf) at any rate."""
    k = np.tan(np.pi * _SHELF_HZ / sr)
    a0, a = _denominator(k, _SHELF_Q)
    return _shelf_numerator(k, a0), a


def _highpass(sr: int) -> tuple[tuple, tuple]:
    """BS.1770 stage 2 (high pass) at any rate."""
    _, a = _denominator(np.tan(np.pi * _HIGHPASS_HZ / sr), _HIGHPASS_Q)
    return (1.0, -2.0, 1.0), a


def k_weight(x: np.ndarray, sr: int = 48000) -> np.ndarray:
    """The BS.1770 K filter (high shelf, then high pass) along axis 0, float64."""
    stages = (K_SHELF_48K, K_HIGHPASS_48K) if sr == 48000 else (_shelf(sr), _highpass(sr))
    y = np.asarray(x, dtype=np.float64)
    for b, a in stages:
        y = _signal().lfilter(b, a, y, axis=0)
    return y


def _block_starts(n: int, w: int, h: int) -> np.ndarray:
    return np.array([0]) if n < w else np.arange(0, n - w + 1, h)


def _block_power(y: np.ndarray, starts: np.ndarray, w: int) -> np.ndarray:
    """Mean square per block and channel, from running sums."""
    csum = np.vstack([np.zeros((1, y.shape[1])), np.cumsum(y * y, axis=0)])
    return (csum[np.minimum(starts + w, len(y))] - csum[starts]) / w


def loudness_curve(x: np.ndarray, win: float = 0.4, hop: float = 0.1) -> tuple[np.ndarray, np.ndarray]:
    """Block loudness (LUFS) at block mid times."""
    y = k_weight(_as_stereo(x))
    w = int(win * SR)
    starts = _block_starts(len(y), w, max(1, int(hop * SR)))
    w = w if len(y) >= w else max(len(y), 1)
    power = _block_power(y, starts, w).sum(axis=1)
    return (starts + w / 2) / SR, -0.691 + 10 * np.log10(power + 1e-20)


def _power_mean(levels: np.ndarray) -> float:
    return float(10 * np.log10(np.mean(10 ** (levels / 10))))


def integrated_lufs(x: np.ndarray) -> float:
    """BS.1770 gated integrated loudness."""
    _, blocks = loudness_curve(x, 0.4, 0.1)
    kept = blocks[blocks > ABS_GATE]
    if kept.size == 0:
        return ABS_GATE
    threshold = _power_mean(kept) - 10
    final = kept[kept > threshold]
    return _power_mean(final) if final.size else threshold + 10


def lra(x: np.ndarray) -> float | None:
    """EBU loudness range in LU, or None with too few blocks."""
    _, blocks = loudness_curve(x, 3.0, 0.1)
    kept = blocks[blocks > ABS_GATE]
    if kept.size < 5:
        return None
    final = kept[kept > _power_mean(kept) - 20]
    if final.size <= 4:
        return None
    return float(np.percentile(final, 95) - np.percentile(final, 10))


def _levels(x: np.ndarray) -> dict:
    return {
        "peak_dbfs": _r(audio.db(float(np.max(np.abs(x)))), 1),
        "true_peak_dbtp": _r(true_peak_dbtp(x), 1),
        "rms_dbfs": _r(audio.db(float(np.sqrt(np.mean(x * x)))), 1),
    }


def _lufs_field(x: np.ndarray, dur: float) -> dict:
    return {"lufs": _r(integrated_lufs(x), 1)} if dur >= LUFS_FROM_S else {}


def _short_term_field(x: np.ndarray, dur: float) -> dict:
    peak = float(np.max(loudness_curve(x, 3.0, 0.5)[1])) if dur >= SHORT_TERM_FROM_S else None
    return {} if peak is None else {"short_term_max_lufs": _r(peak, 1)}


def _lra_field(x: np.ndarray, dur: float) -> dict:
    value = lra(x) if dur >= LRA_FROM_S else None
    return {} if value is None else {"lra_lu": _r(value, 1)}


def _loudness(x: np.ndarray, dur: float) -> dict:
    return {**_lufs_field(x, dur), **_short_term_field(x, dur), **_lra_field(x, dur)}


# ---------------------------------------------------------------- time domain


def _rms(v: np.ndarray) -> float:
    return float(np.sqrt(np.mean(v * v)))


def _side_to_mid(left: np.ndarray, right: np.ndarray) -> float | None:
    side, mid = _rms((left - right) / 2), _rms((left + right) / 2)
    return _r(audio.db(side) - audio.db(mid + 1e-12), 1) if side > 1e-7 else None


def _correlation(left: np.ndarray, right: np.ndarray) -> dict:
    """{"correlation": r}, or {} when either channel is constant."""
    varied = np.std(left) > 1e-9 and np.std(right) > 1e-9
    return {"correlation": _r(float(np.corrcoef(left, right)[0, 1]), 3)} if varied else {}


def _stereo_image(x: np.ndarray) -> dict:
    left, right = x[:, 0], x[:, 1]
    return {"side_to_mid_db": _side_to_mid(left, right), **_correlation(left, right)}


def _floor_fields(m: np.ndarray) -> dict:
    frames = frame_rms(m, 960)
    floor = _r(audio.db(float(np.percentile(frames, 10))), 1)
    return {"noise_floor_dbfs": floor, "dynamic_range_db": _r(audio.db(float(frames.max())) - floor, 1)}


def _time_domain(x: np.ndarray, m: np.ndarray) -> dict:
    clipped = int(np.count_nonzero(np.abs(x) >= CLIP))
    zeros = _r(float(np.mean(np.abs(m) < DIGITAL_ZERO)), 4)
    return {"clipped_samples": clipped, "dc": _r(float(np.mean(m)), 5), **_floor_fields(m),
            "digital_silence_frac": zeros, **_stereo_image(x)}


def _envelope(m: np.ndarray) -> dict:
    env = envelope_times(m)
    whole = {k: (None if env[k] is None else int(round(env[k]))) for k in ("attack_ms", "decay_ms")}
    return {"attack_at": _r(env["attack_at"], 3), "peak_at": _r(env["peak_at"], 3), **whole}


# ---------------------------------------------------------------- spectrum


def _welch(m: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    n = len(m)
    e = int(np.clip(np.ceil(np.log2(max(1024, min(n, 65536)))), 10, 16))
    f, p = _signal().welch(m, fs=SR, nperseg=min(2 ** e, n))
    return f, p + 1e-30


_THIRD_EDGES = 20.0 * 2.0 ** (np.arange(31) / 3)


def _band_level(f: np.ndarray, p: np.ndarray, lo: float, hi: float) -> float:
    sel = p[(f >= lo) & (f < hi)]
    return 10 * np.log10(np.mean(sel)) if sel.size else -300.0


def _bandwidth(f: np.ndarray, p: np.ndarray) -> int:
    """Upper edge of the highest third-octave band within 60 dB of the loudest band."""
    levels = np.array([_band_level(f, p, lo, hi) for lo, hi in zip(_THIRD_EDGES[:-1], _THIRD_EDGES[1:])])
    kept = np.nonzero(levels >= levels.max() - 60)[0]
    return int(_THIRD_EDGES[kept[-1] + 1])


def _bands(f: np.ndarray, p: np.ndarray, total: float) -> dict:
    return {name: _r(10 * np.log10(p[(f >= lo) & (f < hi)].sum() / total + 1e-12), 1)
            for name, (lo, hi) in BANDS.items()}


def _rolloff(f: np.ndarray, p: np.ndarray, total: float) -> int:
    cum = np.cumsum(p) / total
    return int(f[min(int(np.searchsorted(cum, 0.95, side="left")), len(f) - 1)])


def _shape_fields(p: np.ndarray) -> dict:
    flatness = float(np.exp(np.mean(np.log(p))) / np.mean(p))
    return {"flatness": _r(flatness, 4), "tonal_peak_ratio_db": _r(10 * np.log10(np.max(p) / np.median(p)), 1)}


def _spectrum(m: np.ndarray) -> dict:
    f, p = _welch(m)
    total = float(p.sum())
    return {"centroid_hz": int(np.sum(f * p) / total), "rolloff95_hz": _rolloff(f, p, total),
            "bands_db": _bands(f, p, total), "bandwidth_hz": _bandwidth(f, p), **_shape_fields(p)}


def _stft_mag(m: np.ndarray, sr: int) -> tuple[np.ndarray, np.ndarray]:
    f, _, z = _signal().stft(m, fs=sr, window="hann", nperseg=1024, noverlap=768, boundary=None, padded=True)
    return f, np.abs(z)


def _active_frames(mag: np.ndarray) -> np.ndarray:
    """Frames whose magnitude sum is within 30 dB (amplitude) of the loudest frame."""
    energy = mag.sum(axis=0)
    return mag[:, energy >= energy.max() * 10 ** (-30 / 20)] if energy.max() > 0 else mag[:, :0]


def _median_centroid(f: np.ndarray, active: np.ndarray) -> int | None:
    if active.shape[1] == 0:
        return None
    cents = (f[:, None] * active).sum(axis=0) / (active.sum(axis=0) + 1e-20)
    return int(np.median(cents))


def brightness_hz(m: np.ndarray, sr: int = 48000) -> int | None:
    """Median magnitude centroid over STFT frames within 30 dB of the loudest."""
    m = audio.to_mono(np.asarray(m, dtype=np.float64))
    if len(m) < 1024:
        return None
    f, mag = _stft_mag(m, sr)
    return _median_centroid(f, _active_frames(mag))


# ---------------------------------------------------------------- librosa features


def _to22(m: np.ndarray, sr: int) -> np.ndarray:
    return audio.resample(np.asarray(m, dtype=np.float32), sr, LIBROSA_SR)


def _harmonic_ratio(yt: np.ndarray) -> float:
    import librosa

    harm, perc = librosa.effects.hpss(yt)
    eh, ep = float(np.sum(harm.astype(np.float64) ** 2)), float(np.sum(perc.astype(np.float64) ** 2))
    return eh / (eh + ep + 1e-20)


def _f0(yt: np.ndarray) -> float | None:
    import librosa

    pitch = librosa.yin(yt, fmin=50, fmax=2000, sr=LIBROSA_SR, frame_length=2048, hop_length=512)
    rms = librosa.feature.rms(y=yt, frame_length=2048, hop_length=512)[0][: len(pitch)]
    voiced = pitch[: len(rms)][rms > 0.1 * rms.max()]
    return float(np.median(voiced)) if voiced.size else None


def _f0_field(yt: np.ndarray, ratio: float) -> dict:
    """f0_hz when harmonic_ratio >= 0.8; a pitch-tracker failure only drops it (E-F10)."""
    f0 = attempt(_f0, yt) if ratio >= 0.8 else None
    return {} if f0 is None else {"f0_hz": round(f0, 1)}


def _tonality(y22: np.ndarray) -> dict:
    yt = y22[: int(TONALITY_S * LIBROSA_SR)]
    if len(yt) < 2048:
        return {}
    ratio = round(_harmonic_ratio(yt), 3)
    return {"harmonic_ratio": ratio, **_f0_field(yt, ratio)}


def _onset_times(y22: np.ndarray, delta: float) -> list[float]:
    import librosa

    times = librosa.onset.onset_detect(y=y22, sr=LIBROSA_SR, hop_length=256, backtrack=True,
                                       delta=delta, units="time")
    return [round(float(t), 3) for t in times]


def onsets(m: np.ndarray, sr: int = 48000, delta: float = 0.07) -> list[float]:
    """Onset times in seconds (librosa, backtracked, hop 256 at 22,050 Hz)."""
    return _onset_times(_to22(audio.to_mono(m), sr), delta)


def _onset_fields(y22: np.ndarray, dur: float) -> dict:
    times = _onset_times(y22, 0.07)
    return {"onsets": times[:ONSETS_LISTED], "onset_count": len(times),
            "onsets_per_s": round(len(times) / dur, 2) if dur > 0 else 0.0}


def _key_row(chroma: np.ndarray, mode: str, k: int, profile: tuple[float, ...]) -> dict:
    """Pearson r of the chroma with the profile rotated right by k."""
    r = float(np.corrcoef(chroma, np.roll(np.array(profile), k))[0, 1])
    return {"key": f"{NOTE_NAMES[k]} {mode}", "r": r if np.isfinite(r) else -1.0}


def _key_rows(chroma: np.ndarray) -> list[dict]:
    profiles = (("major", KEY_MAJOR), ("minor", KEY_MINOR))
    return [_key_row(chroma, mode, k, profile) for mode, profile in profiles for k in range(12)]


def key_estimate(m: np.ndarray, sr: int = 48000) -> list[dict]:
    """Best 3 Krumhansl-Kessler keys of a mono signal as {key, r}."""
    import librosa

    y22 = _to22(audio.to_mono(m), sr)
    chroma = librosa.feature.chroma_cqt(y=y22, sr=LIBROSA_SR, hop_length=512).mean(axis=1)
    rows = sorted(_key_rows(chroma), key=lambda d: (d["r"], d["key"]), reverse=True)
    return [{"key": d["key"], "r": round(d["r"], 3)} for d in rows[:3]]


def _beats(y22: np.ndarray) -> tuple[float, np.ndarray]:
    import librosa

    env = librosa.onset.onset_strength(y=y22, sr=LIBROSA_SR, hop_length=256)
    tempo, beats = librosa.beat.beat_track(onset_envelope=env, sr=LIBROSA_SR, hop_length=256, units="time")
    return float(np.atleast_1d(tempo)[0]), np.asarray(beats)


def _ibi_field(beats: np.ndarray) -> dict:
    return {"beat_ibi_std_ms": round(1000 * float(np.std(np.diff(beats))), 1)} if len(beats) > 4 else {}


def _music(y22: np.ndarray, m: np.ndarray) -> dict:
    tempo, beats = _beats(y22)
    out: dict[str, Any] = {"tempo": round(tempo, 1), "beats": [round(float(b), 3) for b in beats]}
    out.update(_ibi_field(beats))
    out["key"] = key_estimate(m)
    return out


# ---------------------------------------------------------------- measure


def _num(v: float | None) -> str:
    return "?" if v is None else f"{v:g}"


def _require(ok: bool, message: str) -> None:
    """Raise ValueError(message) when ok is false."""
    failed = not ok
    error = ValueError(message)
    if failed:
        raise error


def _probed_duration(src: Any, start: float | None) -> float:
    """The probed duration of a path when start > 0, else 0 (not checked)."""
    checked = isinstance(src, (str, os.PathLike)) and bool(start)
    return (audio.probe(str(src)).get("duration") or 0.0) if checked else 0.0


def _check_range(src: Any, start: float | None, end: float | None) -> None:
    """The range ValueErrors of core-io 3.10, before any decode."""
    first = start or 0.0
    _require(start is None or start >= 0, f"start must be >= 0 s (got {_num(start)})")
    _require(end is None or end > first, f"end ({_num(end)} s) must be greater than start ({_num(first)} s)")
    known = _probed_duration(src, start)
    name = os.path.basename(str(src)) if known else ""
    _require(not known or first < known, f"start {_num(start)} s is at or beyond the end of {name} "
                                         f"({_num(known)} s long)")


def _input(src: Any, start: float | None, end: float | None) -> np.ndarray:
    if isinstance(src, (str, os.PathLike)):
        return _as_stereo(audio.load(str(src), start=start, end=end))
    x = _as_stereo(np.asarray(src))
    i0 = max(0, int(round((start or 0.0) * SR)))
    i1 = len(x) if end is None else min(len(x), int(round(end * SR)))
    return x[i0:i1]


def _librosa_fields(m: np.ndarray, dur: float, music: bool) -> dict:
    y22 = _to22(m, SR)
    out = {**_tonality(y22), **_onset_fields(y22, dur)}
    out.update(_music(y22, m) if music and dur >= MUSIC_FROM_S else {})
    return out


def _analysis(x: np.ndarray, m: np.ndarray, dur: float, music: bool, light: bool) -> dict:
    out = {**_levels(x), **_loudness(x, dur), **_time_domain(x, m), **_envelope(m), **_spectrum(m)}
    out["brightness_hz"] = brightness_hz(m, SR)
    out.update({} if light else _librosa_fields(m, dur, music))
    return out


def measure(src: Any, music: bool = False, start: float | None = None, end: float | None = None,
            light: bool = False) -> dict:
    """Objective measurements of a file or a 48 kHz array (no models)."""
    _check_range(src, start, end)
    x = _input(src, start, end)
    dur = len(x) / SR
    result: dict[str, Any] = {"duration": round(dur, 3)}
    if len(x) < MIN_SAMPLES:
        result["error"] = "too short to measure"
        return result
    m = x.mean(axis=1)
    result.update(_analysis(x, m, dur, music, light))
    return result


# ---------------------------------------------------------------- summary


def _silent_line(name: str, r: dict) -> str:
    what = "digitally silent" if r.get("digital_silence_frac", 0) >= 0.999 else "near-silent"
    return (f"{name}: {r['duration']:.3f} s, {what} (peak {r['peak_dbfs']:.0f} dBFS): nothing to measure; "
            "loudness, spectrum and envelope values of silence mean nothing.")


def _envelope_part(r: dict) -> str:
    parts = [f"noise floor {r.get('noise_floor_dbfs')} dBFS"]
    if r.get("attack_ms") is not None:
        parts.append(f"attack {r['attack_ms']} ms")
    decay = r.get("decay_ms")
    parts.append(f"decay {decay} ms" if decay is not None else "no -30 dB decay (sustained or cut off)")
    parts.append(f"brightness {r.get('brightness_hz')} Hz, bandwidth {r.get('bandwidth_hz')} Hz")
    return ", ".join(parts)


def _loud_part(r: dict) -> str | None:
    if "lufs" not in r:
        return None
    lra_text = f", LRA {r['lra_lu']} LU" if "lra_lu" in r else ""
    return f"{r['lufs']} LUFS{lra_text}"


def _music_part(r: dict) -> str | None:
    if "tempo" not in r:
        return None
    best = r["key"][0]["key"] if r.get("key") else "?"
    return f"tempo {r['tempo']} BPM, key {best}"


def _bands_part(r: dict) -> str | None:
    bands = r.get("bands_db")
    if not bands:
        return None
    return "bands " + " ".join(f"{k} {v}" for k, v in bands.items()) + " dB"


def summary_line(name: str, r: dict) -> str:
    """The one-line plain-language summary of a measure result."""
    if "error" in r:
        return f"{name}: {r['error']}"
    if r["peak_dbfs"] < -70:
        return _silent_line(name, r)
    head = f"{name}: {r['duration']:.3f} s, peak {r['peak_dbfs']} dBFS (true peak {r['true_peak_dbtp']} dBTP)"
    clipped = f"{r['clipped_samples']} clipped samples" if r.get("clipped_samples") else None
    parts = [head, _loud_part(r), _envelope_part(r), clipped, _music_part(r), _bands_part(r)]
    return "; ".join(p for p in parts if p) + "."


# ---------------------------------------------------------------- CLI


def _measure_file(path: str, args: argparse.Namespace) -> dict:
    r = measure(path, music=args.music, start=args.start, end=args.end, light=args.light)
    return {"file": path, "summary": summary_line(os.path.basename(path), r), **r}


def _media_files(inputs: list[str]) -> list[str]:
    audio.check_paths(inputs, what="files")
    files = audio.list_media(inputs)
    if not files:
        raise audio.AudioError(f"files: nothing playable in {', '.join(inputs)}")
    return files


def _combined(results: list[dict]) -> tuple[dict, str]:
    """The result to emit and its text form: one file as is, several under `files`."""
    single = len(results) == 1
    whole = {"summary": f"{len(results)} files measured.", "files": results}
    text = "\n".join(r["summary"] for r in results)
    return (results[0], text) if single else (whole, text)


def run_cli(args: argparse.Namespace) -> int:
    """Handler of `hark measure`."""
    results = [_measure_file(f, args) for f in _media_files(args.files)]
    result, text = _combined(results)
    report.emit(result, args, text=text)
    return 0


def add_cli(sub: Any) -> None:
    """Register `hark measure`."""
    p = sub.add_parser("measure", help="Objective measurements only, no models.",
                       description="Objective measurements only, no models.")
    p.add_argument("files", nargs="+", help="files, folders or globs")
    p.add_argument("--music", action="store_true", help="also tempo, beats and key (from 5 s)")
    p.add_argument("--light", action="store_true", help="skip pitch, tonality and onsets (no librosa)")
    p.add_argument("--start", type=float, default=None, metavar="S", help="start of the range in seconds")
    p.add_argument("--end", type=float, default=None, metavar="S", help="end of the range in seconds")
    report.add_common(p)
    p.set_defaults(func=run_cli)
