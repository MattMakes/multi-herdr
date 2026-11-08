"""Take measures of the bank: times, loudness, fingerprint and pitch.

Spec: docs/specs/hark/bank-kit-sfx-bank.md sections 6 and 8. Every measure
works on the mono mix (the channel mean of a 2-D input).
"""

from __future__ import annotations

import math
from typing import Any

import numpy as np

SR = 48000
ONSET_DB = -30.0
END_DB = -50.0
FP_HOP_S = 0.05
FP_FLOOR_DB = -60.0
FP_COMPARE_FLOOR_DB = -40.0
CHECK_PEAK_MS = 5.0
CHECK_ENV_DB = 0.5
LUFS_WIN_S = 0.4
LUFS_OFFSET = -0.691
PITCH_SEG_S = 0.6
PITCH_MIN_LEN = 512
PITCH_SPAN_ST = 4.0
PITCH_REJECT = 10.0
FIT_NOTE_BAND = (60.0, 5000.0)
FIT_NOTE_NFFT = 2 ** 17


def mono(y: np.ndarray) -> np.ndarray:
    """The channel mean as float64; a 1-D input is returned as float64."""
    x = np.asarray(y, dtype=np.float64)
    return x.mean(axis=1) if x.ndim == 2 else x


def channels(y: np.ndarray) -> list[np.ndarray]:
    """Each channel as a float64 1-D array."""
    x = np.asarray(y, dtype=np.float64)
    return [x[:, c] for c in range(x.shape[1])] if x.ndim == 2 else [x]


def rms_env(m: np.ndarray, win: int, hop: int) -> np.ndarray:
    """RMS of windows that start at 0, hop, ... while start <= max(0, n - win).

    A window cut by the end of the signal is still divided by ``win``.
    """
    n = len(m)
    starts = np.arange(0, max(0, n - win) + 1, hop)
    csum = np.concatenate([[0.0], np.cumsum(m * m)])
    ends = np.minimum(starts + win, n)
    return np.sqrt((csum[ends] - csum[starts]) / win)


def _r4(v: float) -> float:
    return round(float(v), 4)


def times(y: np.ndarray, sr: int = SR) -> dict[str, float]:
    """``{onset_s, peak_s, end_s, dur_s}`` of a take (part 1, 6.1)."""
    m = mono(y)
    peak, end = _peak_end(m, sr)
    return {"onset_s": _r4(_onset(m, sr) / sr), "peak_s": _r4(peak / sr), "end_s": _r4(end / sr),
            "dur_s": _r4(len(m) / sr)}


def _onset(m: np.ndarray, sr: int) -> int:
    """First sample of the 1 ms envelope within ONSET_DB of its loudest."""
    ms = sr / 1000.0
    hop = max(1, int(round(ms / 4)))
    e = rms_env(m, int(round(ms)), hop)
    return int(np.argmax(e >= e.max() * 10 ** (ONSET_DB / 20))) * hop


def _peak_end(m: np.ndarray, sr: int) -> tuple[float, int]:
    """Peak centre and end (samples) on the 5 ms envelope; the end is the last frame within END_DB."""
    ms = sr / 1000.0
    win, hop = int(round(5 * ms)), int(round(ms))
    e = rms_env(m, win, hop)
    ip = int(np.argmax(e))
    live = np.nonzero(e >= e[ip] * 10 ** (END_DB / 20))[0]
    return ip * hop + win / 2, min(len(m), int(live[-1]) * hop + win)


def peak_db(y: np.ndarray) -> float:
    """Sample peak over every sample and channel in dBFS, 2 decimals."""
    peak = float(np.max(np.abs(y))) if np.size(y) else 0.0
    return round(20 * math.log10(peak + 1e-12), 2)


def k_power(y: np.ndarray, sr: int = SR) -> np.ndarray:
    """K-weighted power summed over the channels (no mono doubling)."""
    from hark.core.features import k_weight

    total = None
    for ch in channels(y):
        p = np.asarray(k_weight(ch, sr), dtype=np.float64) ** 2
        total = p if total is None else total + p
    return total


def loudest_mean(p: np.ndarray, win: int) -> float:
    """Largest mean over ``win`` samples; a shorter signal: its sum / win."""
    if len(p) < win:
        return float(p.sum()) / win
    csum = np.concatenate([[0.0], np.cumsum(p)])
    return float(np.max(csum[win:] - csum[:-win])) / win


def lufs_m_max(y: np.ndarray, sr: int = SR) -> float | None:
    """Momentary-max loudness (400 ms), 2 decimals; None away from 48 kHz."""
    if sr != SR:
        return None
    mean = loudest_mean(k_power(y, sr), int(round(LUFS_WIN_S * sr)))
    return round(LUFS_OFFSET + 10 * math.log10(mean + 1e-24), 2)


def measure(y: np.ndarray, sr: int = SR) -> dict[str, Any]:
    """The take measures: the 4 times, ``peak_db`` and ``lufs_m_max``."""
    out: dict[str, Any] = times(y, sr)
    out = {k: out[k] for k in ("dur_s", "onset_s", "peak_s", "end_s")}
    out["peak_db"] = peak_db(y)
    out["lufs_m_max"] = lufs_m_max(y, sr)
    return out


def centroid_hz(y: np.ndarray, sr: int = SR) -> float:
    """Power-weighted mean frequency of one FFT of the mono mix, 1 decimal."""
    m = mono(y)
    power = np.abs(np.fft.rfft(m)) ** 2
    freqs = np.fft.rfftfreq(len(m), 1.0 / sr)
    return round(float((power * freqs).sum() / (power.sum() + 1e-30)), 1)


def attack_ms(y: np.ndarray) -> float | None:
    """The forge-core polish attack measure of the mono mix."""
    from hark.forge.engine import polish

    return polish.attack(mono(y)).get("attack_ms")


def fingerprint(y: np.ndarray, sr: int = SR) -> dict[str, Any]:
    """50 ms frame levels re the loudest frame, cut after the last frame over -60 dB."""
    m = mono(y)
    hop = int(round(FP_HOP_S * sr))
    frames = max(1, -(-len(m) // hop))
    pad = np.zeros(frames * hop)
    pad[: len(m)] = m
    lv = 10 * np.log10((pad.reshape(frames, hop) ** 2).mean(axis=1) + 1e-24)
    lv = lv - lv.max()
    last = int(np.nonzero(lv > FP_FLOOR_DB)[0][-1])
    return {"hop_s": FP_HOP_S, "env_db": [round(float(v), 1) for v in lv[: last + 1]]}


def meta_of(y: np.ndarray, sr: int = SR) -> dict[str, Any]:
    """The entry meta: times, attack, centroid, loudness, channels, fingerprint."""
    t = times(y, sr)
    out: dict[str, Any] = {k: t[k] for k in ("dur_s", "onset_s", "peak_s", "end_s")}
    out["attack_ms"] = attack_ms(y)
    out["centroid_hz"] = centroid_hz(y, sr)
    out["lufs_m_max"] = lufs_m_max(y, sr)
    out["channels"] = 2 if np.ndim(y) == 2 and np.shape(y)[1] == 2 else 1
    out["fp"] = fingerprint(y, sr)
    return out


def compare_fp(stored: dict[str, Any], new: dict[str, Any], sr: int = SR) -> dict[str, Any]:
    """Compare a stored meta with a new one (part 1, section 9)."""
    len_diff = int(round((float(new["dur_s"]) - float(stored["dur_s"])) * sr))
    peak_diff = round(1000 * (float(new["peak_s"]) - float(stored["peak_s"])), 2)
    env = _env_diff(_env_of(stored), _env_of(new))
    ok = len_diff == 0 and abs(peak_diff) <= CHECK_PEAK_MS and env <= CHECK_ENV_DB
    return {"len_diff": len_diff, "peak_diff_ms": peak_diff, "env_db": env, "ok": ok}


def _env_of(meta: dict[str, Any]) -> list[float]:
    return (meta.get("fp") or {}).get("env_db") or []


def _env_diff(a: list[float], b: list[float]) -> float:
    """Largest envelope difference over the stored frames above FP_COMPARE_FLOOR_DB."""
    diffs = [abs(x - y) for x, y in zip(a, b) if x > FP_COMPARE_FLOOR_DB]
    return round(max(diffs), 2) if diffs else 0.0


def _power_spectrum(seg: np.ndarray, nfft: int) -> np.ndarray:
    return np.abs(np.fft.rfft(seg * np.hanning(len(seg)), nfft)) ** 2


def _onset_segment(m: np.ndarray, sr: int) -> np.ndarray:
    start = int(round(times(m, sr)["onset_s"] * sr))
    return m[start: start + int(round(PITCH_SEG_S * sr))]


def _parabola(logp: np.ndarray, j: int) -> float:
    a, b, c = logp[j - 1], logp[j], logp[j + 1]
    den = a - 2 * b + c
    return 0.0 if den == 0 else 0.5 * (a - c) / den


def _span_bins(size: int, df: float, near_hz: float, span_st: float) -> np.ndarray:
    """FFT bins within ``span_st`` semitones of ``near_hz``, without the 2 edge bins."""
    lo, hi = near_hz * 2 ** (-span_st / 12), near_hz * 2 ** (span_st / 12)
    band = np.arange(int(math.ceil(lo / df)), int(math.floor(hi / df)) + 1)
    return band[(band >= 1) & (band < size - 1)]


def _clear_of_octave(power: np.ndarray, j: int, df: float, near_hz: float) -> bool:
    """True when bin ``j`` is PITCH_REJECT times the median of the octave around ``near_hz``."""
    octave = power[int(math.ceil(near_hz / 2 / df)): int(math.ceil(2 * near_hz / df))]
    return len(octave) > 0 and power[j] >= PITCH_REJECT * float(np.median(octave))


def _peak_near(power: np.ndarray, nfft: int, sr: int, near_hz: float, span_st: float) -> float | None:
    df = sr / nfft
    band = _span_bins(len(power), df, near_hz, span_st)
    if len(band) < 3:
        return None
    j = int(band[np.argmax(power[band])])
    if not _clear_of_octave(power, j, df, near_hz):
        return None
    logp = np.log(power + 1e-300)
    return float((j + _parabola(logp, j)) * sr / nfft)


def _strongest_in_band(m: np.ndarray, sr: int) -> float | None:
    seg = _onset_segment(m, sr)
    if len(seg) < 2 * PITCH_MIN_LEN:
        return None
    power = _power_spectrum(seg, FIT_NOTE_NFFT)
    df = sr / FIT_NOTE_NFFT
    lo, hi = int(math.ceil(FIT_NOTE_BAND[0] / df)), int(math.floor(FIT_NOTE_BAND[1] / df))
    return (lo + int(np.argmax(power[lo: hi + 1]))) * df


def estimate_pitch(y: np.ndarray, sr: int = SR, near_hz: float | None = None,
                   span_st: float | None = None) -> float | None:
    """The strongest clear spectral peak near ``near_hz`` in Hz, or None.

    With ``near_hz`` None, the strongest bin in 60-5000 Hz is the centre
    (part 1, 8.3 without its tonal-class gate). ``span_st`` None is 4.0.
    """
    m = mono(y)
    if near_hz is None:
        near_hz = _strongest_in_band(m, sr)
        if near_hz is None:
            return None
        span_st = 1.0 if span_st is None else span_st
    span = PITCH_SPAN_ST if span_st is None else float(span_st)
    seg = _onset_segment(m, sr)
    if len(seg) < PITCH_MIN_LEN:
        return None
    nfft = 2 ** (int(math.ceil(math.log2(max(len(seg), 16384)))) + 2)
    return _peak_near(_power_spectrum(seg, nfft), nfft, sr, float(near_hz), span)


def fit_note(y: np.ndarray, tonal: bool = True, sr: int = SR) -> float | None:
    """The note of a tonal fit (part 1, 8.3), 2 decimals, or None."""
    if not tonal:
        return None
    f = estimate_pitch(y, sr)
    return None if f is None else round(f, 2)
