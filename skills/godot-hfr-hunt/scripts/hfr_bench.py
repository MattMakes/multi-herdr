#!/usr/bin/env python3
import argparse
import json
import statistics
import subprocess
import sys
from pathlib import Path

PASS = "PASS"
REGRESSION = "FRAME_TIME_REGRESSION"
INVALID = "INFRASTRUCTURE_INVALID"
INFO_FPS = (240, 300)
HOT = {"fair", "serious", "critical"}
MATCH = ("renderer", "rendering_driver", "resolution", "arguments")
MIN_SAMPLE_MS = 20000


def rank(ordered, pct):
    return ordered[-(-pct * len(ordered) // 100) - 1]


def summarize(frames):
    ms = sorted(row["frame_ms"] for row in frames)
    if not ms:
        raise ValueError("no frames")
    p99 = rank(ms, 99)
    return {
        "frames": len(ms),
        "frame_ms_p50": rank(ms, 50),
        "frame_ms_p95": rank(ms, 95),
        "frame_ms_p99": p99,
        "worst_frame_ms": ms[-1],
        "one_percent_low_fps": 1000 / p99,
        "over_3_333_ms": sum(t > 1000 / 300 for t in ms),
        "over_4_167_ms": sum(t > 1000 / 240 for t in ms),
    }


def load(path):
    try:
        doc = json.loads(Path(path).read_text())
        if doc.get("schema_version") != 1:
            return None, "schema_version is not 1"
        doc["summary"] = summarize(doc["frames"])
        return doc, None
    except FileNotFoundError:
        return None, "no take file"
    except (ValueError, KeyError, TypeError, AttributeError):
        return None, "bad take file"


def problem(doc):
    validity = doc.get("validity", {})
    if not validity.get("foreground", False):
        return "lost foreground"
    if validity.get("vsync_period_detected", True):
        return "V-Sync period detected"
    if doc.get("provenance", {}).get("thermal_end") in HOT:
        return "thermal state above nominal"
    if sum(row["frame_ms"] for row in doc["frames"]) < MIN_SAMPLE_MS:
        return "under 20 s of samples"
    return None


def run_take(export, scenario, out):
    out.unlink(missing_ok=True)
    argv = [
        export, "--windowed", "--resolution", "1920x1080", "--disable-vsync",
        "--max-fps", "0", "--", f"--hfr-bench={scenario}", "--warmup-seconds=5",
        "--sample-seconds=30", f"--hfr-out={out}",
    ]
    try:
        code = subprocess.run(argv, stdout=sys.stderr, check=False).returncode
    except OSError as e:
        return None, f"cannot run export: {e.strerror}"
    if code != 0:
        return None, f"export exited {code}"
    return load(out)


def pair_row(n, takes):
    row = {"pair": n, "p99_delta": None, "valid": False, "reason": None}
    reasons = []
    for role, (doc, err) in takes.items():
        if doc is not None:
            for p in (50, 95, 99):
                row[f"{role}_p{p}"] = doc["summary"][f"frame_ms_p{p}"]
            err = problem(doc)
        if err:
            reasons.append(f"{role}: {err}")
    base, cand = takes["baseline"][0], takes["candidate"][0]
    if base is not None and cand is not None:
        row["p99_delta"] = row["candidate_p99"] - row["baseline_p99"]
        for key in MATCH:
            if base.get("provenance", {}).get(key) != cand.get("provenance", {}).get(key):
                reasons.append(f"{key} differs")
    row["reason"] = "; ".join(reasons) or None
    row["valid"] = not reasons
    return row


def median_of(rows, key):
    values = [r[key] for r in rows]
    return statistics.median(values) if values else None


def decide(rows, target):
    valid = [r for r in rows if r["valid"]]
    if len(valid) < 3:
        return INVALID
    deltas = [r["p99_delta"] for r in valid]
    need = len(valid) - 1 if len(valid) >= 5 else len(valid)
    floor = max(0.10, median_of(valid, "baseline_p99") * 0.05)
    if sum(d > 0 for d in deltas) >= need and statistics.median(deltas) > floor:
        return REGRESSION
    if median_of(valid, "candidate_p99") > 1000 / target:
        return f"{target}_FPS_BUDGET_FAILURE"
    return PASS


def cell(value):
    return "-" if value is None else f"{value:.3f}"


def print_table(rows):
    print("pair  baseline p50/p95/p99    candidate p50/p95/p99   p99 delta  valid")
    for r in rows:
        base = "/".join(cell(r.get(f"baseline_p{p}")) for p in (50, 95, 99))
        cand = "/".join(cell(r.get(f"candidate_p{p}")) for p in (50, 95, 99))
        state = "yes" if r["valid"] else f"no ({r['reason']})"
        print(f"{r['pair']:>4}  {base:<23} {cand:<23} {cell(r['p99_delta']):>9}  {state}")


def print_budgets(candidate_p99):
    for fps in INFO_FPS:
        budget = 1000 / fps
        if candidate_p99 is None:
            print(f"info: {fps} fps budget {budget:.3f} ms: no valid pairs")
            continue
        state = "met" if candidate_p99 <= budget else "not met"
        print(f"info: {fps} fps budget {budget:.3f} ms: {state} (median candidate p99 {candidate_p99:.3f} ms)")


def compare(args):
    out = Path(args.out).resolve()
    out.mkdir(parents=True, exist_ok=True)
    name = args.scenario
    run_take(args.baseline, name, out / f"{name}-warmup.json")
    rows = []
    for n in range(1, args.pairs + 1):
        takes = {
            role: run_take(export, name, out / f"{name}-{role}-{n:02d}.json")
            for role, export in (("baseline", args.baseline), ("candidate", args.candidate))
        }
        rows.append(pair_row(n, takes))
    valid = [r for r in rows if r["valid"]]
    verdict = decide(rows, args.target_fps)
    report = {
        "scenario": name,
        "target_fps": args.target_fps,
        "valid_pairs": len(valid),
        "median_p99_delta": median_of(valid, "p99_delta"),
        "median_baseline_p99": median_of(valid, "baseline_p99"),
        "median_candidate_p99": median_of(valid, "candidate_p99"),
        "verdict": verdict,
        "pairs": rows,
    }
    (out / "compare.json").write_text(json.dumps(report, indent=2) + "\n")
    print_table(rows)
    print_budgets(report["median_candidate_p99"])
    print(verdict)
    return {PASS: 0, INVALID: 2}.get(verdict, 1)


def summarize_take(args):
    try:
        frames = json.loads(Path(args.take).read_text())["frames"]
        summary = summarize(frames)
    except (OSError, ValueError, KeyError, TypeError) as e:
        print(f"hfr_bench: cannot summarize {args.take}: {e}", file=sys.stderr)
        return 2
    print(json.dumps(summary, indent=2))
    return 0


def target_fps(text):
    try:
        value = int(text)
    except ValueError:
        value = 0
    if not 200 <= value <= 300:
        raise argparse.ArgumentTypeError("must be an integer from 200 to 300")
    return value


def pair_count(text):
    value = int(text)
    if value < 1:
        raise argparse.ArgumentTypeError("must be at least 1")
    return value


def main(argv=None):
    parser = argparse.ArgumentParser(prog="hfr_bench.py")
    sub = parser.add_subparsers(dest="command", required=True)
    s = sub.add_parser("summarize")
    s.add_argument("take")
    s.set_defaults(func=summarize_take)
    c = sub.add_parser("compare")
    c.add_argument("--baseline", required=True)
    c.add_argument("--candidate", required=True)
    c.add_argument("--scenario", required=True)
    c.add_argument("--out", required=True)
    c.add_argument("--pairs", type=pair_count, default=5)
    c.add_argument("--target-fps", type=target_fps, default=240)
    c.set_defaults(func=compare)
    args = parser.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
