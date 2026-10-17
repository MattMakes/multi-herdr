# Script: fight_table.py

Usage: `python3 -I fight_table.py <logs> --targets bands.json`

Copy the block below into a file named `fight_table.py`, then run it. It reads JSON-lines bot fight
logs and prints 1 table per encounter and steering, plus the pooled honest row. The bands file maps
each encounter id to the bands of its class. The script prints PASS or FAIL for each band, the coach
check and the cheap check. It exits 1 when a band or check fails and 2 on bad input. The docstring
documents the log line and the bands format. It uses the Python standard library only.

```python
#!/usr/bin/env python3
"""Build a fight table from JSON-lines bot fight logs and check it against target bands.

Usage:
  python3 -I fight_table.py LOG_OR_DIR [LOG_OR_DIR ...] [--targets targets.json]
         [--cheap ram,wait] [--coached coached] [--json out.json]

Each log line is 1 fight try, a JSON object. Lines that are not JSON are skipped
(engine output often shares the file). Fields:
  case        encounter id (string), required
  steering    bot line, for example "coached", "tight", "wide", "wait", "ram"; required
  seed        int, required
  try         1-based try index for that seed, required
  result      "kill" or "loss", required
  end_reason  for example "kill", "resource", "timer", "fight_cap"; optional
  length_s    fight length in seconds; optional
  resource_left  share of the resource left at the end (0..1); optional
  rescued     true when a refill or rescue item saved the try; a rescued kill counts as a loss
  hits        {"source": count, ...}; optional

Per case and steering it prints: seeds, first-try wins, mean tries to the first clean kill
(a seed with no kill counts as max try + 1 and is also counted as "no kill"), median length,
median resource left at the kill, end reasons, hits by source, and the number of distinct
outcomes (a bot that never misses gives few distinct fights).

Targets file (optional): {"<case>": {"first_try_rate": [lo, hi], "mean_tries": [lo, hi],
"median_length_s": [lo, hi], "median_resource_left": [lo, hi]}}. Bands apply to the pool of
honest steerings (every steering not named in --cheap).

Checks per case:
  coach check: the coached line has at least (best other honest line - 1) first-try wins.
  cheap check: every cheap line has fewer first-try wins than the coached line.
Exit code: 0 when every band and check passes, 1 when any fails, 2 on bad input.
Standard library only.
"""
import argparse
import json
import os
import statistics
import sys
from collections import Counter, defaultdict

REQUIRED = ("case", "steering", "seed", "try", "result")


def read_rows(paths):
    rows, bad = [], 0
    files = []
    for p in paths:
        if os.path.isdir(p):
            for root, _dirs, names in os.walk(p):
                for n in sorted(names):
                    if n.endswith((".jsonl", ".log")):
                        files.append(os.path.join(root, n))
        else:
            files.append(p)
    for f in sorted(files):
        with open(f, encoding="utf-8", errors="replace") as fh:
            for line in fh:
                line = line.strip()
                if not line.startswith("{"):
                    continue
                try:
                    r = json.loads(line)
                except ValueError:
                    bad += 1
                    continue
                if not all(k in r for k in REQUIRED):
                    bad += 1
                    continue
                r["_log"] = os.path.basename(f)
                rows.append(r)
    return rows, bad, len(files)


def clean_kill(r):
    return r["result"] == "kill" and not r.get("rescued", False)


def median(xs):
    return statistics.median(xs) if xs else None


def summarize(rows):
    by_seed = defaultdict(list)
    for r in rows:
        by_seed[(r["steering"], r["_log"], r["seed"])].append(r)
    max_try = max(int(r["try"]) for r in rows)
    first_try_wins, tries, no_kill = 0, [], 0
    lengths, left, reasons, hits, outcomes = [], [], Counter(), Counter(), set()
    for _key, rs in sorted(by_seed.items(), key=lambda kv: str(kv[0])):
        rs.sort(key=lambda r: int(r["try"]))
        kill_try = next((int(r["try"]) for r in rs if clean_kill(r)), None)
        if kill_try == 1:
            first_try_wins += 1
        if kill_try is None:
            no_kill += 1
            tries.append(max_try + 1)
        else:
            tries.append(kill_try)
        for r in rs:
            reasons[r.get("end_reason", r["result"])] += 1
            for src, n in (r.get("hits") or {}).items():
                hits[src] += n
            if r.get("length_s") is not None:
                lengths.append(float(r["length_s"]))
            if clean_kill(r) and r.get("resource_left") is not None:
                left.append(float(r["resource_left"]))
            outcomes.add((r["result"], r.get("end_reason"), round(float(r.get("length_s") or 0), 1)))
    seeds = len(by_seed)
    return {
        "seeds": seeds,
        "first_try_wins": first_try_wins,
        "first_try_rate": first_try_wins / seeds if seeds else 0.0,
        "mean_tries": statistics.mean(tries) if tries else None,
        "no_kill_seeds": no_kill,
        "median_length_s": median(lengths),
        "median_resource_left": median(left),
        "end_reasons": dict(reasons),
        "hits": dict(hits),
        "distinct_outcomes": len(outcomes),
    }


def fmt(v):
    if v is None:
        return "-"
    if isinstance(v, float):
        return f"{v:.2f}"
    return str(v)


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("paths", nargs="+")
    ap.add_argument("--targets")
    ap.add_argument("--cheap", default="ram,wait", help="comma list of off-skill steerings")
    ap.add_argument("--coached", default="coached")
    ap.add_argument("--json")
    a = ap.parse_args(argv)

    rows, bad, nfiles = read_rows(a.paths)
    if not rows:
        print(f"no fight rows in {nfiles} file(s); {bad} bad line(s)", file=sys.stderr)
        return 2
    cheap = {s for s in a.cheap.split(",") if s}
    targets = {}
    if a.targets:
        with open(a.targets, encoding="utf-8") as fh:
            targets = json.load(fh)

    groups = defaultdict(lambda: defaultdict(list))
    for r in rows:
        groups[str(r["case"])][str(r["steering"])].append(r)

    out, failed = {}, False
    print(f"{len(rows)} fight rows from {nfiles} file(s); {bad} bad line(s) skipped")
    for case in sorted(groups):
        per = {s: summarize(rs) for s, rs in sorted(groups[case].items())}
        honest = [r for s, rs in groups[case].items() if s not in cheap for r in rs]
        pooled = summarize(honest) if honest else None
        print(f"\n## {case}")
        print("| steering | seeds | try-1 wins | mean tries | no kill | median s | median left | distinct |")
        print("| --- | --- | --- | --- | --- | --- | --- | --- |")
        for s, m in per.items():
            print(f"| {s} | {m['seeds']} | {m['first_try_wins']} | {fmt(m['mean_tries'])} | "
                  f"{m['no_kill_seeds']} | {fmt(m['median_length_s'])} | "
                  f"{fmt(m['median_resource_left'])} | {m['distinct_outcomes']} |")
        if pooled:
            print(f"| POOLED honest | {pooled['seeds']} | {pooled['first_try_wins']} | "
                  f"{fmt(pooled['mean_tries'])} | {pooled['no_kill_seeds']} | "
                  f"{fmt(pooled['median_length_s'])} | {fmt(pooled['median_resource_left'])} | "
                  f"{pooled['distinct_outcomes']} |")
        for s, m in per.items():
            print(f"- {s} end reasons: {m['end_reasons']}; hits: {m['hits'] or '-'}")
        checks = []
        for metric, band in (targets.get(case) or {}).items():
            v = pooled.get(metric) if pooled else None
            ok = v is not None and band[0] <= v <= band[1]
            checks.append((f"band {metric} {fmt(v)} in [{band[0]}, {band[1]}]", ok))
        if a.coached in per:
            c = per[a.coached]["first_try_wins"]
            others = [m["first_try_wins"] for s, m in per.items() if s != a.coached and s not in cheap]
            if others:
                checks.append((f"coach check {c} >= {max(others)} - 1", c >= max(others) - 1))
            for s in sorted(cheap & set(per)):
                w = per[s]["first_try_wins"]
                checks.append((f"cheap check {s} {w} < coached {c}", w < c))
        for text, ok in checks:
            print(f"{'PASS' if ok else 'FAIL'} {text}")
            failed |= not ok
        out[case] = {"steerings": per, "pooled_honest": pooled,
                     "checks": [{"check": t, "pass": ok} for t, ok in checks]}
    if a.json:
        with open(a.json, "w", encoding="utf-8") as fh:
            json.dump(out, fh, indent=2, sort_keys=True)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
```
