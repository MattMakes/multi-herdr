"""Beat grid: tempo, beats, downbeat phase, pulse clarity (analysis 2.5)."""

from __future__ import annotations

import numpy as np
from scipy import ndimage

from hark.timeline._dsp import SR, logmel, power_frames

NOTE = ("beats have 10 ms resolution; downbeats are a guess that assumes 4/4; the tempo can be half or double "
        "the felt tempo")


def empty_grid(note: str) -> dict:
    return {"tempo": None, "beats": [], "downbeats": [], "confidence": "none", "note": note}


def pulse_envelopes(m: np.ndarray) -> list[np.ndarray]:
    L = logmel(m, n_fft=2048, hop=480, n_mels=64, fmin=30, fmax=8000)
    L = np.maximum(L, L.max() - 80)
    d = np.zeros_like(L)
    d[:, 1:] = np.maximum(np.diff(L, axis=1), 0)
    out = []
    for env in (d.mean(axis=0), d[:6].mean(axis=0)):
        e = np.maximum(env - ndimage.uniform_filter1d(env, size=101, mode="nearest"), 0)
        out.append(e / (e.std() + 1e-9))
    return out


def autocorr(env: np.ndarray) -> np.ndarray:
    v = env - env.mean()
    n = v.size
    f = np.fft.rfft(v, 2 * n)
    ac = np.fft.irfft(f * np.conj(f))[:n]
    return ac / (ac[0] if ac[0] != 0 else 1.0)


def period(env: np.ndarray, min_bpm: float, max_bpm: float):
    """(best score, period P in frames, clarity, ac) or None."""
    n = env.size
    ac = autocorr(env)
    lags = np.arange(int(6000 / max_bpm), min(int(6000 / min_bpm), n // 2))
    if lags.size == 0:
        return None
    prior = np.exp(-0.5 * (np.log2(6000 / lags / 120) / 0.9) ** 2)
    score = (ac[lags] + 0.5 * ac[np.minimum(2 * lags, n - 1)]) * prior
    k = int(np.argmax(score))
    P = float(lags[k])
    if 0 < k < lags.size - 1:
        y0, y1, y2 = score[k - 1], score[k], score[k + 1]
        P += 0.5 * (y0 - y2) / (y0 - 2 * y1 + y2 + 1e-12)
    return float(score[k]), P, float(ac[int(round(P))]), ac


def _forward(env: np.ndarray, P: float) -> tuple[np.ndarray, np.ndarray]:
    """Dynamic-programming pass: cumulative score and back-pointer per frame."""
    lags = np.arange(max(1, int(P / 2)), int(2 * P) + 1)
    pen = 100 * np.log(lags / P) ** 2
    score = env.astype(np.float64).copy()
    back = np.full(env.size, -1)
    for t in range(max(int(P / 2), int(lags[0])), env.size):
        _step(score, back, t, lags[lags <= t], pen[lags <= t])
    return score, back


def _step(score: np.ndarray, back: np.ndarray, t: int, lags: np.ndarray, pen: np.ndarray) -> None:
    c = score[t - lags] - pen
    k = int(np.argmax(c))
    if c[k] > 0:
        score[t] += c[k]
        back[t] = t - lags[k]


def track(env: np.ndarray, P: float) -> list[int]:
    score, back = _forward(env, P)
    tail = max(1, int(P))
    t = env.size - tail + int(np.argmax(score[env.size - tail:]))
    beats = []
    while t >= 0:
        beats.append(int(t))
        t = back[t]
    return beats[::-1]


def _chroma_low(m: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    n_fft = 4096
    f = np.fft.rfftfreq(n_fft, 1 / SR)
    band = (f >= 55) & (f <= 2000)
    cls = np.mod(np.round(12 * np.log2(f[band] / 440)).astype(int), 12)
    onehot = np.zeros((band.sum(), 12))
    onehot[np.arange(cls.size), cls] = 1
    low = f < 150
    chroma, energy = [], []
    for _, p in power_frames(m, n_fft, 480):
        mag = np.sqrt(p)
        chroma.append(mag[:, band] @ onehot)
        energy.append(np.log1p(mag[:, low].sum(axis=1)))
    return np.concatenate(chroma), np.concatenate(energy)


def _beat_profiles(chroma: np.ndarray, b: np.ndarray) -> np.ndarray:
    """Unit-length mean chroma of each beat span."""
    ends = np.append(b[1:], b[-1])
    prof = np.array([chroma[a:z].mean(axis=0) if z > a else chroma[a] for a, z in zip(b, ends)])
    return prof / (np.linalg.norm(prof, axis=1, keepdims=True) + 1e-9)


def _kick(low: np.ndarray, b: np.ndarray) -> np.ndarray:
    dlow = np.zeros_like(low)
    dlow[1:] = np.maximum(np.diff(low), 0)
    return np.array([dlow[max(0, i - 3): i + 4].max() for i in b])


def downbeat_phase(m: np.ndarray, beats: list[int]) -> int:
    chroma, low = _chroma_low(m)
    b = np.minimum(np.asarray(beats), chroma.shape[0] - 1)
    prof = _beat_profiles(chroma, b)
    harm = np.zeros(b.size)
    harm[1:] = 1 - (prof[1:] * prof[:-1]).sum(axis=1)
    kick = _kick(low, b)
    accent = harm / (harm.std() + 1e-9) + kick / (kick.std() + 1e-9)
    return int(np.argmax([accent[p::4].mean() if accent[p::4].size else -np.inf for p in range(4)]))


def _confidence(clarity: float, regularity: float) -> str:
    if clarity >= 0.30 and regularity < 0.1:
        return "clear"
    return "weak" if clarity >= 0.15 else "none"


def _best_period(m: np.ndarray, min_bpm: float, max_bpm: float):
    """((score, P, clarity, ac), envelope) of the clearer pulse envelope, or None."""
    pairs = [(period(e, min_bpm, max_bpm), e) for e in pulse_envelopes(m)]
    pairs = [(r, e) for r, e in pairs if r is not None]
    return max(pairs, key=lambda p: p[0][0]) if pairs else None


def _grid(m: np.ndarray, beats: list[int], best: tuple, max_bpm: float) -> dict:
    """Grid dict from the tracked beats and the (score, P, clarity, ac) of the chosen period."""
    _, P, clarity, ac = best
    ibi = np.diff(beats)
    regularity = float(ibi.std() / ibi.mean())
    phase = downbeat_phase(m, beats)
    alt = round(12000 / P, 1) if 12000 / P <= max_bpm and ac[int(round(P / 2))] >= 0.5 * clarity else None
    times = [round(b / 100, 3) for b in beats]
    return {"tempo": round(6000 / P, 1), "tempo_alt": alt, "beats": times, "downbeats": times[phase::4],
            "pulse_clarity": round(clarity, 3), "beat_regularity": round(regularity, 3),
            "confidence": _confidence(clarity, regularity), "note": NOTE}


def music_grid(m: np.ndarray, min_bpm: float = 60, max_bpm: float = 200) -> dict:
    """Beat grid of the mono signal m."""
    m = np.asarray(m, dtype=np.float64)
    best = _best_period(m, min_bpm, max_bpm) if m.size >= 5 * SR else None
    if best is None:
        return empty_grid("the file is too short for a beat grid (under 5 s)")
    beats = track(best[1], best[0][1])
    if len(beats) < 4:
        return empty_grid("no steady pulse was found")
    return _grid(m, beats, best[0], max_bpm)
