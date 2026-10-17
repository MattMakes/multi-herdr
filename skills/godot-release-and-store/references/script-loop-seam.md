# Loop seam script

Usage: `python3 loop_seam.py MUSIC.wav [--max-jump 0.1]`

The script measures the jump from the last sample to the first sample of a 16-bit PCM WAV file, per channel, as a fraction of full scale. It prints the median and p99 step between neighbour samples for comparison. It exits 0 when every seam step is at most `--max-jump`, else 1. Copy the block into a file named `loop_seam.py` before you run it. It uses the Python standard library only.

```python
#!/usr/bin/env python3
"""Usage: python3 loop_seam.py MUSIC.wav [--max-jump 0.1]

Measure the jump at the loop seam of a 16-bit PCM WAV file: the step from the last
sample to the first sample, per channel, as a fraction of full scale. The typical
step between neighbour samples inside the file is printed for comparison.
A large seam step against a small typical step is an audible click on loop.
Exit 0 when every channel's seam step is at most --max-jump (default 0.1). Exit 1
otherwise. Standard library only (wave, array).
"""
import argparse
import array
import sys
import wave


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("wav")
    ap.add_argument("--max-jump", type=float, default=0.1)
    args = ap.parse_args()
    with wave.open(args.wav, "rb") as w:
        if w.getsampwidth() != 2:
            print("only 16-bit PCM is supported", file=sys.stderr)
            return 2
        ch = w.getnchannels()
        samples = array.array("h", w.readframes(w.getnframes()))
    if sys.byteorder == "big":
        samples.byteswap()
    frames = len(samples) // ch
    if frames < 2:
        print("file too short", file=sys.stderr)
        return 2
    ok = True
    for c in range(ch):
        s = samples[c::ch]
        seam = abs(s[0] - s[-1]) / 32768.0
        steps = sorted(abs(s[i + 1] - s[i]) for i in range(len(s) - 1))
        typical = steps[len(steps) // 2] / 32768.0
        p99 = steps[int(len(steps) * 0.99)] / 32768.0
        flag = "OK" if seam <= args.max_jump else "CLICK"
        ok = ok and seam <= args.max_jump
        print(f"channel {c}: seam step {seam:.4f}, median step {typical:.4f}, p99 step {p99:.4f} -> {flag}")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
```
