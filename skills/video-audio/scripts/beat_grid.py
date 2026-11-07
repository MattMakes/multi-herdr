#!/usr/bin/env python3
"""Measure a simple beat grid from an audio file with ffmpeg."""

import array
import math
import shutil
import subprocess
import sys

RATE = 8000
HOP = 16
MIN_BPM = 60
MAX_BPM = 180
BEATS_PER_BAR = 4


class BeatError(Exception):
    """The audio or local decoder cannot provide a beat grid."""


def _decode(path):
    """Decode path to mono signed 16-bit PCM."""
    if not shutil.which("ffmpeg"):
        raise BeatError("ffmpeg is not on PATH")
    proc = subprocess.run(
        ["ffmpeg", "-hide_banner", "-v", "error", "-i", path, "-ac", "1",
         "-ar", str(RATE), "-f", "s16le", "-"],
        capture_output=True, check=False,
    )
    if proc.returncode != 0:
        tail = proc.stderr.decode(errors="replace")[-300:]
        raise BeatError(f"ffmpeg cannot decode {path}: {tail}")
    samples = array.array("h")
    samples.frombytes(proc.stdout[:len(proc.stdout) // 2 * 2])
    if sys.byteorder == "big":
        samples.byteswap()
    if len(samples) < RATE:
        raise BeatError("beat measurement needs at least 1 second of audio")
    return samples


def _flux(samples):
    """Return positive changes in short-window energy."""
    energy = [sum(value * value for value in samples[i:i + HOP])
              for i in range(0, len(samples) - HOP + 1, HOP)]
    return [max(0, right - left) for left, right in zip(energy, energy[1:])]


def _tempo_lag(flux):
    """Return the strongest autocorrelation lag in frames."""
    low = math.ceil(60 * RATE / (MAX_BPM * HOP))
    high = math.floor(60 * RATE / (MIN_BPM * HOP))

    def score(lag):
        return sum(flux[i] * flux[i + lag] for i in range(len(flux) - lag))

    lag = max(range(low, min(high, len(flux) - 1) + 1), key=score)
    if score(lag) == 0:
        raise BeatError("the audio has no measurable beat onsets")
    return lag


def _phase(flux, lag):
    """Return the strongest beat phase in frames."""
    return max(range(lag), key=lambda phase: sum(flux[phase::lag]))


def measure(path):
    """Return tempo, beats, downbeats and bar length measured from path."""
    samples = _decode(path)
    flux = _flux(samples)
    lag = _tempo_lag(flux)
    phase = _phase(flux, lag)
    step_sec = lag * HOP / RATE
    duration_sec = len(samples) / RATE
    first_sec = phase * HOP / RATE
    beats = []
    beat = first_sec
    while beat <= duration_sec + 1e-9:
        beats.append(round(beat, 6))
        beat += step_sec
    return {
        "tempo_bpm": round(60.0 / step_sec, 4),
        "bar_sec": round(BEATS_PER_BAR * step_sec, 6),
        "beats_sec": beats,
        "downbeats_sec": beats[::BEATS_PER_BAR],
    }
