# Script: ab_report.py

Usage: `python3 ab_report.py FRAMES.csv [--a A] [--b B] [--skip N] [--threshold MS] [--max-load L]`

Copy the block below into a file named `ab_report.py`, then run it on the frame CSV of an
interleaved A/B take. The CSV has the columns `variant`, `pair` and `ms`, and optionally `load`.
The script prints one row per take (frames, average, p50, p99, worst frame, frames over the
threshold, load, valid), the median and the best median p50 per variant, and the median gap
B - A with its share of the A frame. It marks the result WEAK when the pair gaps straddle 0 or
when fewer than 3 valid pairs exist. It uses the Python standard library only.

```python
#!/usr/bin/env python3
"""Summarize an interleaved A/B frame-time measure.

Usage: python3 ab_report.py FRAMES.csv [--a A] [--b B] [--skip N] [--threshold MS] [--max-load L]

FRAMES.csv has a header row with at least these columns:
  variant  the variant name of the take (for example "base" or "new")
  pair     the pair number; the A take and the B take of one pair share it
  ms       the frame time of one frame in milliseconds
Optional column:
  load     the machine load average at that frame (the take uses its largest value)

The script prints one row per take (frames, avg, p50, p99, worst, frames over the threshold,
load, valid), the median and best median p50 per variant, and the median gap B - A with its
share of the A frame. It marks the result WEAK when the pair gaps straddle 0 or fewer than 3
valid pairs exist. Standard library only.
"""
import argparse
import csv
import statistics
import sys
from collections import defaultdict


def pct(sorted_vals, p):
    if not sorted_vals:
        return float("nan")
    k = (len(sorted_vals) - 1) * p / 100.0
    lo = int(k)
    hi = min(lo + 1, len(sorted_vals) - 1)
    return sorted_vals[lo] + (sorted_vals[hi] - sorted_vals[lo]) * (k - lo)


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("csv")
    ap.add_argument("--a", help="variant name of A (default: first variant seen)")
    ap.add_argument("--b", help="variant name of B (default: second variant seen)")
    ap.add_argument("--skip", type=int, default=0, help="frames to drop at the start of each take")
    ap.add_argument("--threshold", type=float, default=20.0, help="hitch threshold in ms")
    ap.add_argument("--max-load", type=float, default=None, help="mark a take invalid above this load")
    args = ap.parse_args()

    frames = defaultdict(list)
    loads = defaultdict(float)
    order = []
    with open(args.csv, newline="") as f:
        reader = csv.DictReader(f)
        for col in ("variant", "pair", "ms"):
            if col not in (reader.fieldnames or []):
                sys.exit("missing column: %s" % col)
        for row in reader:
            key = (row["variant"], row["pair"])
            if row["variant"] not in order:
                order.append(row["variant"])
            frames[key].append(float(row["ms"]))
            if row.get("load"):
                loads[key] = max(loads[key], float(row["load"]))

    a = args.a or (order[0] if order else None)
    b = args.b or (order[1] if len(order) > 1 else None)
    if a is None or b is None:
        sys.exit("need 2 variants, found: %s" % order)

    stats = {}
    print("variant,pair,frames,avg,p50,p99,worst,over_threshold,load,valid")
    for key in sorted(frames, key=lambda k: (k[1], k[0])):
        vals = frames[key][args.skip:]
        s = sorted(vals)
        valid = bool(s) and (args.max_load is None or loads[key] <= args.max_load)
        st = {
            "avg": statistics.fmean(s) if s else float("nan"),
            "p50": pct(s, 50),
            "p99": pct(s, 99),
            "worst": s[-1] if s else float("nan"),
            "over": sum(1 for v in s if v > args.threshold),
            "valid": valid,
        }
        stats[key] = st
        print("%s,%s,%d,%.3f,%.3f,%.3f,%.3f,%d,%.2f,%s" % (
            key[0], key[1], len(s), st["avg"], st["p50"], st["p99"], st["worst"],
            st["over"], loads[key], "yes" if valid else "INVALID"))

    print()
    med = {}
    for v in (a, b):
        p50s = [st["p50"] for (vv, _), st in stats.items() if vv == v and st["valid"]]
        if not p50s:
            sys.exit("no valid takes for variant %s" % v)
        med[v] = statistics.median(p50s)
        print("%s: median p50 %.3f ms, best p50 %.3f ms, valid takes %d"
              % (v, med[v], min(p50s), len(p50s)))

    gaps = []
    for (v, pair), st in stats.items():
        if v != a or not st["valid"]:
            continue
        other = stats.get((b, pair))
        if other and other["valid"]:
            gaps.append(other["p50"] - st["p50"])
    if not gaps:
        sys.exit("no valid pairs")
    gap = statistics.median(gaps)
    share = 100.0 * gap / med[a] if med[a] else float("nan")
    weak = []
    if min(gaps) < 0 < max(gaps):
        weak.append("pair gaps straddle 0 (%.3f to %.3f ms)" % (min(gaps), max(gaps)))
    if len(gaps) < 3:
        weak.append("only %d valid pairs" % len(gaps))
    print("gap %s - %s: median %+.3f ms (%+.1f %% of %s), pairs %d"
          % (b, a, gap, share, a, len(gaps)))
    print("result: %s" % ("WEAK: " + "; ".join(weak) if weak else "OK"))


if __name__ == "__main__":
    main()
```
