# Script: wav_report.py

Usage: copy the block below into a file `wav_report.py`. Then run
`python3 -I wav_report.py --selftest` first, and
`python3 -I wav_report.py [--rate HZ] [--edge N] FILE.wav [FILE.wav ...]` on your files.

The script measures integer PCM WAV files (8, 16, 24 or 32 bit) against the game audio rules. It
prints 1 JSON object per file: rate, length, sample peak, integrated LUFS (ITU-R BS.1770 gated
loudness), momentary max, leading silence, onset, end level, DC offset, `loop_seam_ratio`,
`edge_step_ratio` and the energy share above about 3 kHz. The self-test writes a 997 Hz sine at
0 dBFS and expects about -3.01 LUFS. It uses the Python standard library only. For a long session
recording, use ffmpeg `ebur128` or a streaming meter instead.

```python
#!/usr/bin/env python3
"""Measure WAV files for game audio rules. Prints one JSON object per file.

Usage:
  python3 -I wav_report.py [--rate HZ] [--edge N] FILE.wav [FILE.wav ...]
  python3 -I wav_report.py --selftest
  python3 -I wav_report.py --help

Fields:
  rate, channels, length_ms        from the header (--rate overrides a wrong header only)
  peak_dbfs                        sample peak (not true peak; use ffmpeg ebur128 peak=true for that)
  integrated_lufs                  ITU-R BS.1770 gated loudness (absolute -70, relative -10)
  momentary_max_lufs               loudest 400 ms window, 100 ms hop; a short file is padded with silence
  leading_silence_ms               time before the first sample above -60 dBFS
  onset_ms                         time from the first audible sample to the first sample within 20 dB of peak
  end_dbfs                         level of the last sample (a one-shot should end near silence)
  dc_offset                        mean sample value
  loop_seam_ratio                  |last - first| / 99th-percentile step; at most 1.0 means no click at the seam
  edge_step_ratio                  largest step across the joined last N and first N samples / 99th-percentile step
  high_share_3k                    energy share above about 3 kHz (compare a harsh cue to a known soft cue)

Integer PCM (8, 16, 24 or 32 bit) only. Standard library only. Pure Python: fine for effects and
loops; for a long session recording use ffmpeg ebur128 or a streaming meter instead.
"""

from __future__ import annotations

import json
import math
import sys
import wave


def read_pcm(path: str) -> tuple[int, list[list[float]]]:
    with wave.open(path, "rb") as w:
        channels, width, rate = w.getnchannels(), w.getsampwidth(), w.getframerate()
        raw = w.readframes(w.getnframes())
    scale = float(1 << (8 * width - 1))
    count = len(raw) // width
    values = []
    for i in range(count):
        chunk = raw[i * width:(i + 1) * width]
        if width == 1:
            values.append((chunk[0] - 128) / 128.0)
        else:
            values.append(int.from_bytes(chunk, "little", signed=True) / scale)
    return rate, [values[c::channels] for c in range(channels)]


def biquad(xs: list[float], b0: float, b1: float, b2: float, a1: float, a2: float) -> list[float]:
    out, x1, x2, y1, y2 = [], 0.0, 0.0, 0.0, 0.0
    for x in xs:
        y = b0 * x + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2
        out.append(y)
        x2, x1, y2, y1 = x1, x, y1, y
    return out


def k_weight(xs: list[float], rate: int) -> list[float]:
    """BS.1770 K-weighting at any rate (the bilinear form that reproduces the 48 kHz table)."""
    k = math.tan(math.pi * 1681.974450955533 / rate)
    q, vh = 0.7071752369554196, 10 ** (3.999843853973347 / 20)
    vb = vh ** 0.4996667741545416
    a0 = 1 + k / q + k * k
    shelf = ((vh + vb * k / q + k * k) / a0, 2 * (k * k - vh) / a0, (vh - vb * k / q + k * k) / a0,
             2 * (k * k - 1) / a0, (1 - k / q + k * k) / a0)
    k = math.tan(math.pi * 38.13547087602444 / rate)
    q = 0.5003270373238773
    a0 = 1 + k / q + k * k
    high_pass = (1 / a0, -2 / a0, 1 / a0, 2 * (k * k - 1) / a0, (1 - k / q + k * k) / a0)
    return biquad(biquad(xs, *shelf), *high_pass)


def block_powers(channels: list[list[float]], rate: int) -> list[float]:
    """Mean square per 400 ms block with a 100 ms hop, summed over channels (weight 1.0 each)."""
    block, hop = round(0.4 * rate), round(0.1 * rate)
    sums = []
    for xs in channels:
        k = k_weight(xs, rate)
        k += [0.0] * max(0, block - len(k))
        prefix = [0.0]
        for v in k:
            prefix.append(prefix[-1] + v * v)
        sums.append([(prefix[s + block] - prefix[s]) / block for s in range(0, len(k) - block + 1, hop)])
    return [sum(col) for col in zip(*sums)]


def lufs(power: float) -> float:
    return -0.691 + 10 * math.log10(max(power, 1e-12))


def integrated(powers: list[float]) -> float:
    gated = [p for p in powers if lufs(p) > -70.0]
    if not gated:
        return -70.0
    threshold = lufs(sum(gated) / len(gated)) - 10.0
    gated = [p for p in gated if lufs(p) > threshold]
    return lufs(sum(gated) / len(gated)) if gated else -70.0


def db(x: float) -> float:
    return 20 * math.log10(max(abs(x), 1e-9))


def report(path: str, rate_override: int | None = None, edge: int = 64) -> dict[str, object]:
    rate, channels = read_pcm(path)
    rate = rate_override or rate
    mono = [sum(f) / len(f) for f in zip(*channels)]
    if not mono:
        return {"file": path, "error": "no samples"}
    peak = max(abs(x) for x in mono)
    first = next((i for i, x in enumerate(mono) if db(x) > -60), len(mono))
    onset = next((i for i, x in enumerate(mono) if db(x) > db(peak) - 20), len(mono))
    steps = sorted(abs(mono[i + 1] - mono[i]) for i in range(len(mono) - 1)) or [0.0]
    p99 = max(steps[int(len(steps) * 0.99)], 1e-9)
    joined = mono[-edge:] + mono[:edge]
    edge_step = max((abs(joined[i + 1] - joined[i]) for i in range(len(joined) - 1)), default=0.0)
    c = math.exp(-2 * math.pi * 3000 / rate)  # one-pole split at about 3 kHz
    low, low_energy, total = 0.0, 0.0, 0.0
    for x in mono:
        low = (1 - c) * x + c * low
        low_energy += low * low
        total += x * x
    high_share = max(0.0, 1 - low_energy / total) if total else 0.0
    powers = block_powers(channels, rate)
    return {
        "file": path,
        "rate": rate,
        "channels": len(channels),
        "length_ms": round(1000 * len(mono) / rate, 1),
        "peak_dbfs": round(db(peak), 2),
        "integrated_lufs": round(integrated(powers), 2),
        "momentary_max_lufs": round(lufs(max(powers)), 2),
        "leading_silence_ms": round(1000 * first / rate, 2),
        "onset_ms": round(1000 * max(0, onset - first) / rate, 2),
        "end_dbfs": round(db(mono[-1]), 1),
        "dc_offset": round(sum(mono) / len(mono), 5),
        "loop_seam_ratio": round(abs(mono[-1] - mono[0]) / p99, 2),
        "edge_step_ratio": round(edge_step / p99, 2),
        "high_share_3k": round(high_share, 3),
    }


def selftest() -> int:
    """A 997 Hz sine at 0 dBFS must read about -3.01 LUFS (the BS.1770 reference)."""
    import os
    import struct
    import tempfile
    rate, seconds = 48000, 5
    folder = tempfile.mkdtemp(prefix="wav_report_selftest_")
    path = os.path.join(folder, "sine997.wav")
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(rate)
        w.writeframes(b"".join(struct.pack("<h", round(32767 * math.sin(2 * math.pi * 997 * i / rate)))
                               for i in range(rate * seconds)))
    result = report(path)
    ok = abs(result["integrated_lufs"] - (-3.01)) <= 0.1 and result["length_ms"] == seconds * 1000
    print(json.dumps(result))
    print("SELFTEST " + ("PASS" if ok else "FAIL") + f": integrated {result['integrated_lufs']} LUFS, expected -3.01")
    return 0 if ok else 1


def main(argv: list[str]) -> int:
    if "--help" in argv or "-h" in argv:
        print(__doc__)
        return 0
    if argv == ["--selftest"]:
        return selftest()
    rate, edge, files = None, 64, []
    i = 0
    while i < len(argv):
        if argv[i] == "--rate":
            rate, i = int(argv[i + 1]), i + 2
        elif argv[i] == "--edge":
            edge, i = int(argv[i + 1]), i + 2
        else:
            files.append(argv[i])
            i += 1
    if not files:
        print(__doc__)
        return 2
    for path in files:
        print(json.dumps(report(path, rate, edge)))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
```
