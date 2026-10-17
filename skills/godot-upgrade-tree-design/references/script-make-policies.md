# Script: make_policies.py

Usage: `python3 make_policies.py --data <diag_tree.json> --out <dir> [--themes <themes.json>] [--random 5] [--specials early] [--group-key group]`

Copy the block into a file named `make_policies.py`. The script reads the diag tree data file and writes the policy set of a sweep (contract section 5): a balanced baseline, 1 policy per theme, 1 leave-one-out policy per shared stat spoke, and N seeded random policies. A second run gives byte-identical files. It uses the Python standard library only.

```python
#!/usr/bin/env python3
"""Write the policy set of a diag sweep (contract section 5) from a diag_tree.json data file.

    python3 make_policies.py --data <diag_tree.json> --out <dir> [--themes <themes.json>] [--random 5] [--specials early]
                           [--group-key group]

Output in <dir>:
  balanced.json                 every weight 1: the baseline.
  <theme>.json                  1 per theme of --themes: {"fight": {"dmg_pct": 3, "tick_pct": 3}, ...}; the other
                                spokes keep weight 1.
  loo/no-<spoke>.json           1 per shared stat spoke: balanced with skip [spoke] and baseline "balanced".
  random/random-<k>.json        k = 1..N: a seeded random pick by weight.
A theme weight for a spoke id that the data does not have is an error (a typo would silently test nothing).
A second run gives byte-identical files.
"""
import argparse
import json
import os
import shutil
import sys


def policy(name, kind, weights, specials, **extra):
    row = {"name": name, "kind": kind, "weights": weights, "default_weight": 1, "skip": [], "baseline": "",
           "specials": specials, "seed": 0}
    row.update(extra)
    return row


def write(path, row):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        json.dump(row, f, indent=1, sort_keys=True)
        f.write("\n")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--data", required=True, help="the diag_tree.json data file")
    parser.add_argument("--out", required=True, help="the policy folder (loo/ and random/ are rewritten)")
    parser.add_argument("--themes", help="a JSON file: {theme name: {spoke id: weight}}")
    parser.add_argument("--random", type=int, default=5, help="the count of random policies")
    parser.add_argument("--specials", default="early", choices=["early", "late", "never"])
    parser.add_argument("--group-key", default="group",
                        help="the spoke field that holds the group id; an empty value means a shared spoke")
    args = parser.parse_args(argv)

    with open(args.data, encoding="utf-8") as f:
        spokes = json.load(f)["spokes"]
    ids = {s["id"] for s in spokes}
    shared_stats = [s["id"] for s in spokes if s.get("kind") == "stat" and not s.get(args.group_key)]
    themes = {}
    if args.themes:
        with open(args.themes, encoding="utf-8") as f:
            themes = json.load(f)
    for theme, weights in themes.items():
        unknown = sorted(set(weights) - ids)
        if unknown:
            sys.exit(f"theme {theme}: unknown spoke ids {unknown}")

    write(os.path.join(args.out, "balanced.json"), policy("balanced", "weights", {}, args.specials))
    for theme, weights in sorted(themes.items()):
        write(os.path.join(args.out, f"{theme}.json"), policy(theme, "weights", weights, args.specials))
    for sub in ("loo", "random"):
        shutil.rmtree(os.path.join(args.out, sub), ignore_errors=True)
    for spoke in shared_stats:
        write(os.path.join(args.out, "loo", f"no-{spoke}.json"),
              policy(f"no-{spoke}", "weights", {}, args.specials, skip=[spoke], baseline="balanced"))
    for k in range(1, args.random + 1):
        write(os.path.join(args.out, "random", f"random-{k}.json"),
              policy(f"random-{k}", "random", {}, args.specials, seed=k))
    print(f"wrote {1 + len(themes)} base, {len(shared_stats)} leave-one-out and {args.random} random policies "
          f"to {args.out}")


if __name__ == "__main__":
    main()
```
