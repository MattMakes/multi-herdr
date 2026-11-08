"""Scene builders of the find checks (listen-eval E12): a room-tone bed, one-shots, S1 and S2.

The builders work on arrays at the analysis rate. ``write_scene`` writes 32-bit float WAV files
under the evaluation output folder, never into the repository.
"""

from __future__ import annotations

from collections.abc import Sequence
from pathlib import Path

import numpy as np

from hark.core import audio

SR = 48000
BED_DBFS = -55.0
SHOT_PEAK_DBFS = -6.0
SHOT_START_LEVEL = 0.05
SHOT_PRE_S = 0.010
SHOT_MAX_S = 2.5
SHOT_FADE_S = 0.050
S1_LEN_S = 44.0
S1_STARTS = tuple(2.0 + 5.0 * k for k in range(8))
S1_QUERIES = ("a glass ting", "a fast whoosh", "a cork pop", "a lego brick click", "an object dropping into water",
              "a light switch click", "a spacebar press", "a small bell")
S2_LEN_S = 300.0
S2_KEY_PEAK_DBFS = -12.0
S2_KEY_START_S = 1.0
S2_KEY_LAST_S = 298.0
S2_SPACING_S = (1.3, 1.4, 1.5)
S2_BEFORE_S = 0.7
S2_AFTER_S = 2.6
S2_EVENTS = ((20.0, "ting"), (150.0, "ting"), (275.0, "ting"), (205.0, "pop"), (240.0, "whoosh"))
S2_QUERIES = {"ting": "a glass ting", "pop": "a cork pop", "whoosh": "a fast whoosh"}


def mono(x: np.ndarray) -> np.ndarray:
    """A mono float64 copy (channel mean)."""
    x = np.asarray(x, np.float64)
    return x.mean(axis=1) if x.ndim == 2 else x.copy()


def rms_db(x: np.ndarray) -> float:
    """RMS level in dBFS (-inf for silence)."""
    r = float(np.sqrt(np.mean(np.square(x)))) if len(x) else 0.0
    return 20 * np.log10(r) if r > 0 else -np.inf


def peak_db(x: np.ndarray) -> float:
    """Peak level in dBFS (-inf for silence)."""
    p = float(np.max(np.abs(x))) if len(x) else 0.0
    return 20 * np.log10(p) if p > 0 else -np.inf


def room_bed(tone: np.ndarray, seconds: float, sr: int = SR, dbfs: float = BED_DBFS) -> np.ndarray:
    """A room-tone recording, mono, tiled to the length and scaled to ``dbfs`` RMS."""
    x = mono(tone)
    n = int(round(seconds * sr))
    bed = np.resize(x, n) if len(x) else np.zeros(n)
    level = rms_db(bed)
    if not np.isfinite(level):
        raise ValueError("the room-tone recording is silent")
    return bed * 10 ** ((dbfs - level) / 20)


def one_shot(x: np.ndarray, sr: int = SR, peak_dbfs: float = SHOT_PEAK_DBFS) -> np.ndarray:
    """Start 10 ms before the first sample at 5 % of peak, keep <= 2.5 s, 50 ms fade-out, scale to the peak."""
    x = mono(x)
    a = np.abs(x)
    if not len(x) or a.max() <= 0:
        raise ValueError("the one-shot recording is silent")
    first = int(np.argmax(a >= SHOT_START_LEVEL * a.max()))
    begin = max(0, first - int(round(SHOT_PRE_S * sr)))
    y = x[begin:begin + int(round(SHOT_MAX_S * sr))].copy()
    nf = min(len(y), int(round(SHOT_FADE_S * sr)))
    if nf:
        y[-nf:] *= np.linspace(1.0, 0.0, nf)
    return y * 10 ** (peak_dbfs / 20) / np.max(np.abs(y))


def add_at(bed: np.ndarray, shot: np.ndarray, start_s: float, sr: int = SR) -> None:
    """Add a shot into the bed at ``start_s`` (cut at the bed end)."""
    i = int(round(start_s * sr))
    n = max(0, min(len(shot), len(bed) - i))
    bed[i:i + n] += shot[:n]


def build_s1(tone: np.ndarray, shots: Sequence[np.ndarray], sr: int = SR) -> tuple[np.ndarray, list[float]]:
    """S1: a 44 s bed plus 8 one-shots at 2 + 5k s; returns the scene and the starts."""
    if len(shots) != len(S1_STARTS):
        raise ValueError(f"S1 needs {len(S1_STARTS)} one-shots, got {len(shots)}")
    bed = room_bed(tone, S1_LEN_S, sr)
    for shot, t in zip(shots, S1_STARTS):
        add_at(bed, one_shot(shot, sr), t, sr)
    return bed, list(S1_STARTS)


def key_starts(events: Sequence[float], first: float = S2_KEY_START_S, last: float = S2_KEY_LAST_S,
               spacing: Sequence[float] = S2_SPACING_S) -> list[float]:
    """Key-press starts: every 1.3, 1.4 or 1.5 s in turn, skipped near a planted event."""
    out, t, k = [], first, 0
    while t <= last + 1e-9:
        if not any(e - S2_BEFORE_S <= t < e + S2_AFTER_S for e in events):
            out.append(round(t, 6))
        t += spacing[k % len(spacing)]
        k += 1
    return out


def build_s2(tone: np.ndarray, keys: Sequence[np.ndarray], shots: dict[str, np.ndarray],
             sr: int = SR) -> tuple[np.ndarray, list[float], dict[str, list[float]]]:
    """S2: a 300 s bed, key presses (-12 dBFS peak, in turn), and the 5 planted one-shots."""
    if not keys:
        raise ValueError("S2 needs at least one key-press slice")
    bed = room_bed(tone, S2_LEN_S, sr)
    events = [t for t, _ in S2_EVENTS]
    starts = key_starts(events)
    for n, t in enumerate(starts):
        k = mono(keys[n % len(keys)])
        add_at(bed, k * 10 ** (S2_KEY_PEAK_DBFS / 20) / max(np.max(np.abs(k)), 1e-12), t, sr)
    planted: dict[str, list[float]] = {}
    for t, name in S2_EVENTS:
        add_at(bed, one_shot(shots[name], sr), t, sr)
        planted.setdefault(name, []).append(t)
    return bed, starts, planted


def write_scene(path: Path, x: np.ndarray, sr: int = SR) -> Path:
    """Write (overwrite) a 32-bit float WAV."""
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    audio.write_wav(str(path), np.asarray(x, np.float32), sr, subtype="FLOAT")
    return path
