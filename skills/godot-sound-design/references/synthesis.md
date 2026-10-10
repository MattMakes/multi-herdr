# Synthesis recipes: procedural, offline, deterministic

> Back to [SKILL.md](../SKILL.md). Design rules are in
> [feedback-design.md](feedback-design.md); loudness targets are in
> [mixing.md](mixing.md).

Make the sound set with a script, not by hand and not from third-party
samples. A script gives 4 things:

1. **No third-party rights.** Every sample comes from math in the repository.
2. **Determinism.** The same script gives the same bytes. Seed every noise
   source with a fixed string (`random.Random("cut_0")`), never with the time.
3. **Review.** A change to a sound is a code diff with a reason.
4. **Measurement.** The same kit measures what it makes (section 4).

The 3 files below use the Python standard library only. Put them in the
game's tools folder (for example `tools/sfx/`), run the recipes from the
project root, and commit both the script and the `.wav` output. Godot
imports the `.wav` files. Write 16-bit mono PCM at 44.1 kHz, or match
`audio/driver/mix_rate`.

## 1. The kit: `sfxkit.py`

Signals are plain `list[float]` in -1..1. Every helper returns a new list.

```python
#!/usr/bin/env python3
"""Deterministic offline sound synthesis: standard library only."""

from __future__ import annotations

import math
import random
import struct
import wave
from pathlib import Path
from typing import Callable

RATE = 44_100
Signal = list[float]


def silence(seconds: float) -> Signal:
    return [0.0] * round(seconds * RATE)


def times(seconds: float) -> list[float]:
    return [i / RATE for i in range(round(seconds * RATE))]


def env_perc(seconds: float, attack: float, decay_per_s: float, release: float = 0.004) -> Signal:
    """Fast linear attack, exponential decay, short linear fade at the end."""
    out = []
    for t in times(seconds):
        rise = min(1.0, t / max(attack, 1e-6))
        fall = min(1.0, (seconds - t) / max(release, 1e-6))
        out.append(rise * fall * math.exp(-decay_per_s * t))
    return out


def sweep(seconds: float, start_hz: float, end_hz: float, curve: float = 1.0) -> Signal:
    """Sine whose frequency glides from start_hz to end_hz (phase-continuous)."""
    phase = 0.0
    out = []
    count = round(seconds * RATE)
    for i in range(count):
        ratio = (i / max(count - 1, 1)) ** curve
        phase += math.tau * (start_hz + (end_hz - start_hz) * ratio) / RATE
        out.append(math.sin(phase))
    return out


def noise(seconds: float, seed: str) -> Signal:
    rng = random.Random(seed)
    return [rng.uniform(-1.0, 1.0) for _ in range(round(seconds * RATE))]


def lowpass(signal: Signal, cutoff_hz: float) -> Signal:
    """One-pole low-pass filter."""
    k = 1.0 - math.exp(-math.tau * cutoff_hz / RATE)
    state, out = 0.0, []
    for x in signal:
        state += k * (x - state)
        out.append(state)
    return out


def highpass(signal: Signal, cutoff_hz: float) -> Signal:
    low = lowpass(signal, cutoff_hz)
    return [x - y for x, y in zip(signal, low)]


def mul(a: Signal, b: Signal) -> Signal:
    return [x * y for x, y in zip(a, b)]


def mix(*parts: tuple[float, Signal]) -> Signal:
    length = max(len(s) for _, s in parts)
    out = [0.0] * length
    for gain, s in parts:
        for i, x in enumerate(s):
            out[i] += gain * x
    return out


def normalize(signal: Signal, peak_dbfs: float) -> Signal:
    peak = max(max(abs(x) for x in signal), 1e-9)
    scale = 10 ** (peak_dbfs / 20) / peak
    return [x * scale for x in signal]


def biquad(signal: Signal, b: tuple[float, float, float], a: tuple[float, float, float]) -> list[float]:
    b0, b1, b2 = (v / a[0] for v in b)
    a1, a2 = a[1] / a[0], a[2] / a[0]
    x1 = x2 = y1 = y2 = 0.0
    out = []
    for x in signal:
        y = b0 * x + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2
        x2, x1, y2, y1 = x1, x, y1, y
        out.append(y)
    return out


def k_weight(signal: Signal, rate: int = RATE) -> Signal:
    """ITU-R BS.1770 K-weighting at any sample rate: a high shelf, then a
    high pass. At 48 kHz the coefficients equal the table in BS.1770."""
    k = math.tan(math.pi * 1681.974450955533 / rate)
    q, vh = 0.7071752369554196, 10 ** (3.999843853973347 / 20)
    vb = vh ** 0.4996667741545416
    shelf = biquad(signal, (vh + vb * k / q + k * k, 2 * (k * k - vh), vh - vb * k / q + k * k),
                   (1 + k / q + k * k, 2 * (k * k - 1), 1 - k / q + k * k))
    k = math.tan(math.pi * 38.13547087602444 / rate)
    q = 0.5003270373238773
    return biquad(shelf, (1.0, -2.0, 1.0), (1 + k / q + k * k, 2 * (k * k - 1), 1 - k / q + k * k))


def loudness_lufs(signal: Signal, rate: int = RATE) -> float:
    """Maximum momentary loudness (BS.1770 K-weighting, 400 ms window).
    A file shorter than 400 ms is measured as 1 window, padded with silence."""
    k = k_weight(signal, rate)
    block, hop = round(0.4 * rate), round(0.01 * rate)
    k += [0.0] * max(0, block - len(k))
    squares = [x * x for x in k]
    window = sum(squares[:block])
    best = window
    for start in range(hop, len(k) - block + 1, hop):
        window = sum(squares[start:start + block])
        best = max(best, window)
    return -0.691 + 10 * math.log10(max(best / block, 1e-12))


def normalize_loudness(signal: Signal, target_lufs: float, ceiling_dbfs: float = -1.0) -> Signal:
    """Scale to a momentary-loudness target. The peak never exceeds the ceiling."""
    gain = 10 ** ((target_lufs - loudness_lufs(signal)) / 20)
    peak = max(max(abs(x) for x in signal), 1e-9)
    gain = min(gain, 10 ** (ceiling_dbfs / 20) / peak)
    return [x * gain for x in signal]


def write_wav(path: Path, signal: Signal) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    pcm = b"".join(struct.pack("<h", round(max(-1.0, min(1.0, x)) * 32_767)) for x in signal)
    with wave.open(str(path), "wb") as wav:
        wav.setnchannels(1)
        wav.setsampwidth(2)
        wav.setframerate(RATE)
        wav.writeframes(pcm)


def read_wav(path: Path) -> Signal:
    with wave.open(str(path), "rb") as wav:
        if wav.getsampwidth() != 2:
            raise ValueError(f"{path}: only 16-bit PCM is supported")
        channels = wav.getnchannels()
        raw = wav.readframes(wav.getnframes())
    values = struct.unpack(f"<{len(raw) // 2}h", raw)
    return [sum(values[i:i + channels]) / channels / 32_768 for i in range(0, len(values), channels)]
```

`loudness_lufs` measures the loudest 400 ms window (momentary loudness, ITU-R
BS.1770 K-weighting). A sound shorter than 400 ms is measured as 1 window
padded with silence, so short sounds compare fairly with each other. Self
test: a 997 Hz sine at 0 dBFS reads -3.0 LUFS (the BS.1770 reference value is
-3.01).

## 2. Recipes: `recipes.py`

Each recipe follows the transient, body, tail plan of
[feedback-design.md](feedback-design.md) section 1. `main` normalizes each
sound to a momentary-loudness target for its group, with a -1 dBFS peak
ceiling.

```python
from sfxkit import *


def tick(seconds: float = 0.05, hz: float = 1800.0) -> Signal:
    """Transient-only click: a pitched blip that drops 1 octave."""
    return mul(sweep(seconds, hz, hz * 0.5), env_perc(seconds, 0.001, 90.0))


def cut(seed: str, seconds: float = 0.08) -> Signal:
    """Short cut or rustle: band-limited noise plus a falling snip."""
    body = highpass(lowpass(noise(seconds, seed), 6000.0), 900.0)
    snip = sweep(seconds, 2600.0, 1400.0)
    return mul(mix((1.0, body), (0.25, snip)), env_perc(seconds, 0.002, 45.0))


def impact(seed: str, seconds: float = 0.25) -> Signal:
    """Thump: a low sine that drops in pitch, a noise click on top."""
    thump = mul(sweep(seconds, 140.0, 55.0, 0.5), env_perc(seconds, 0.002, 14.0))
    click = mul(lowpass(noise(seconds, seed), 3000.0), env_perc(seconds, 0.001, 120.0))
    return mix((1.0, thump), (0.5, click))


def pickup(seconds: float = 0.12, base_hz: float = 880.0) -> Signal:
    """2-note rise: the second note is a fifth above the first."""
    half = seconds / 2
    first = mul(sweep(half, base_hz, base_hz), env_perc(half, 0.002, 30.0))
    second = mul(sweep(half, base_hz * 1.5, base_hz * 1.5), env_perc(half, 0.002, 18.0))
    return first + second


def reward(tier: int, base_hz: float = 523.25) -> Signal:
    """Plucked major chord. A higher tier adds a note, an octave and tail."""
    ratios = [1.0, 1.25, 1.5, 2.0, 2.5][: 2 + min(tier, 3)]
    seconds = 0.35 + 0.2 * tier
    notes = [mul(mix((1.0, sweep(seconds, base_hz * r, base_hz * r)),
                     (0.15 * tier, sweep(seconds, base_hz * r * 2, base_hz * r * 2))),
                 env_perc(seconds, 0.003, 6.0 / seconds)) for r in ratios]
    return mix(*[(1.0 / len(notes), n) for n in notes])


def break_sound(seconds: float = 0.2) -> Signal:
    """Bad news: a falling, dull tone, quieter than the rewards."""
    return lowpass(mul(sweep(seconds, 420.0, 180.0), env_perc(seconds, 0.003, 12.0)), 1200.0)


def engine_loop(seconds: float = 2.4, base_hz: float = 55.0) -> Signal:
    """Seamless loop: every partial and LFO completes whole cycles in the loop."""
    def whole(hz: float) -> float:
        return round(hz * seconds) / seconds
    out = []
    for t in times(seconds):
        lfo = 0.8 + 0.12 * math.sin(math.tau * whole(5.0) * t)
        motor = sum(a * math.sin(math.tau * whole(base_hz * h) * t) for h, a in ((1, 1.0), (2, 0.4), (3, 0.2)))
        out.append(motor * lfo)
    return out


def ambient_loop(seed: str, seconds: float = 8.0, fade: float = 0.5) -> Signal:
    """Soft filtered noise bed. The end crossfades into the start."""
    raw = lowpass(highpass(noise(seconds + fade, seed), 200.0), 1800.0)
    body = raw[: round(seconds * RATE)]
    tail = raw[round(seconds * RATE):]
    for i, x in enumerate(tail):
        w = i / len(tail)
        body[i] = body[i] * w + x * (1.0 - w)
    return body


def main(out: Path) -> None:
    # Loudness targets per sound group, in momentary LUFS (see mixing.md).
    write_wav(out / "tick.wav", normalize_loudness(tick(), -26.0))
    for n in range(3):
        write_wav(out / f"cut_{n}.wav", normalize_loudness(cut(f"cut_{n}"), -28.0))
    write_wav(out / "impact.wav", normalize_loudness(impact("impact"), -20.0))
    write_wav(out / "pickup.wav", normalize_loudness(pickup(), -22.0))
    for tier in range(4):
        write_wav(out / f"reward_{tier}.wav", normalize_loudness(reward(tier), -21.0 + tier))
    write_wav(out / "break.wav", normalize_loudness(break_sound(), -24.0))
    write_wav(out / "engine_loop.wav", normalize_loudness(engine_loop(), -24.0))
    write_wav(out / "ambient_loop.wav", normalize_loudness(ambient_loop("ambient"), -32.0))


if __name__ == "__main__":
    import sys
    main(Path(sys.argv[1]))
```

Run it: `python3 tools/sfx/recipes.py audio/sfx`. Run it twice and compare
the outputs byte for byte (`diff -r`): a difference means a seed or a clock
leaked in.

How each recipe meets the rules:

| Recipe | Transient | Body | Tail | Use |
| --- | --- | --- | --- | --- |
| `tick` | 1 ms attack | pitched blip, falls 1 octave | none | UI, combo step, near-miss tick |
| `cut` | 2 ms attack | band-limited noise, falling snip | short | very frequent actions; make 3 or more seeds |
| `impact` | noise click | low sine that drops in pitch | 0.25 s | hits taken, heavy contact |
| `pickup` | 2 ms attack | 2 notes, a fifth apart, rising | short | small gains |
| `reward(tier)` | 3 ms attack | major chord; more notes per tier | grows with tier | real net gains only (honesty rule) |
| `break_sound` | 3 ms attack | falling, low-passed tone | short | losses, broken streak; quieter than rewards |
| `engine_loop` | n/a | harmonics and an LFO in whole cycles | loops | continuous engine or motor |
| `ambient_loop` | n/a | filtered noise | loops with a crossfade | world bed |

Variation: change the seed for noise recipes and the base frequency by a few
percent for tonal recipes to make variants. Godot adds random pitch and
volume at play time (`AudioStreamRandomizer`).

Loops: a loop is seamless when every periodic part completes a whole number
of cycles in the loop length (`engine_loop`), or when the end crossfades into
the start (`ambient_loop`). Set the loop mode in the import settings or on the
`AudioStreamWAV`.

## 3. Pitch and tuning

Use musical ratios for tonal feedback: a fifth is 1.5, a major third 1.25, an
octave 2.0. Equal temperament: `f = 440 * 2 ** ((midi - 69) / 12)`. Keep all
reward sounds in 1 key, so that 2 rewards that overlap do not clash.

## 4. Measure: `sfx_report.py`

```python
#!/usr/bin/env python3
"""Measure feedback sounds: length, peak, onset, end click, loudness, loop seam.

loop_seam_ratio: the jump from the last sample to the first, divided by the
99th-percentile step inside the file. At most 1.0 means no click at the seam.

Usage: python3 sfx_report.py <wav files...>
Prints one JSON object per file. 16-bit PCM WAV only. Standard library only.
"""

from __future__ import annotations

import json
import math
import sys
from pathlib import Path

from sfxkit import loudness_lufs, read_wav


def db(x: float) -> float:
    return 20 * math.log10(max(abs(x), 1e-9))


def report(path: Path, rate: int = 44_100) -> dict[str, object]:
    s = read_wav(path)
    peak = max(abs(x) for x in s)
    first_audible = next((i for i, x in enumerate(s) if db(x) > -60), len(s))
    onset = next((i for i, x in enumerate(s) if db(x) > db(peak) - 20), len(s))
    last_audible = max((i for i, x in enumerate(s) if db(x) > db(peak) - 40), default=0)
    steps = sorted(abs(s[i + 1] - s[i]) for i in range(len(s) - 1))
    large_step = steps[int(len(steps) * 0.99)] if steps else 0.0
    return {
        "file": path.name,
        "length_ms": round(1000 * len(s) / rate, 1),
        "audible_ms": round(1000 * (last_audible - first_audible) / rate, 1),
        "peak_dbfs": round(db(peak), 1),
        "leading_silence_ms": round(1000 * first_audible / rate, 2),
        "onset_ms": round(1000 * (onset - first_audible) / rate, 2),
        "end_dbfs": round(db(s[-1]), 1),
        "loudness_lufs": round(loudness_lufs(s, rate), 1),
        "loop_seam_ratio": round(abs(s[-1] - s[0]) / max(large_step, 1e-9), 1),
    }


if __name__ == "__main__":
    for arg in sys.argv[1:]:
        print(json.dumps(report(Path(arg))))
```

Run it on every sound after each change:
`python3 tools/sfx/sfx_report.py audio/sfx/*.wav`. The pass marks are in
[review-checklist.md](review-checklist.md) section 2.
