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
    raw = _ffmpeg_pcm(path)
    samples = _pcm_samples(raw)
    if len(samples) < RATE:
        raise BeatError("beat measurement needs at least 1 second of audio")
    return samples


def _ffmpeg_pcm(path):
    """Return mono PCM bytes decoded by ffmpeg."""
    _require_ffmpeg()
    proc = subprocess.run(_ffmpeg_command(path), capture_output=True, check=False)
    _require_decode(proc, path)
    return proc.stdout


def _require_ffmpeg():
    """Require ffmpeg on PATH."""
    binary = shutil.which("ffmpeg")
    if not binary:
        raise BeatError("ffmpeg is not on PATH")


def _ffmpeg_command(path):
    """Return the command that decodes path for beat analysis."""
    return ["ffmpeg", "-hide_banner", "-v", "error", "-i", path, "-ac", "1",
            "-ar", str(RATE), "-f", "s16le", "-"]


def _require_decode(proc, path):
    """Raise a beat error when ffmpeg did not decode path."""
    tail = proc.stderr.decode(errors="replace")[-300:]
    if proc.returncode != 0:
        raise BeatError(f"ffmpeg cannot decode {path}: {tail}")


def _pcm_samples(raw):
    """Return native-endian signed 16-bit samples from little-endian bytes."""
    samples = array.array("h")
    samples.frombytes(raw[:len(raw) // 2 * 2])
    if sys.byteorder == "big":
        samples.byteswap()
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
    scores = [sum(flux[phase::lag]) for phase in range(lag)]
    return max(range(lag), key=scores.__getitem__)


def _beat_times(first_sec, step_sec, duration_sec):
    """Return measured beat times through the audio duration."""
    count = math.floor((duration_sec - first_sec) / step_sec) + 1
    return [round(first_sec + index * step_sec, 6) for index in range(count)]


def _grid(lag, phase, sample_count):
    """Build a grid from the measured lag and phase."""
    step_sec = lag * HOP / RATE
    beats = _beat_times(phase * HOP / RATE, step_sec, sample_count / RATE)
    return {
        "tempo_bpm": round(60.0 / step_sec, 4),
        "bar_sec": round(BEATS_PER_BAR * step_sec, 6),
        "beats_sec": beats,
        "downbeats_sec": beats[::BEATS_PER_BAR],
    }


def measure(path):
    """Return tempo, beats, downbeats and bar length measured from path."""
    samples = _decode(path)
    flux = _flux(samples)
    lag = _tempo_lag(flux)
    return _grid(lag, _phase(flux, lag), len(samples))
