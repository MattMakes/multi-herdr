# Script: report.py

Usage: `python3 report.py <trace file or folder>... --out <dir> [--json [<path>]] [--walls <wall.json>...] [--targets <targets.json>] [--near 0.10] [--baseline balanced] [--group-key group] [--chapter-key chapter]`

Copy the block into a file named `report.py` in a tools folder of your project. The script reads every `diag-trace/1` file under the paths you give, ranks the policies, and writes `report.md` and `report.html` to `--out`. With `--json` it also writes `balance.json` (schema in `balance-json.md`). It uses the Python standard library only. Its unit tests and fixture traces are in `script-test-report.md`.

If you rebuild the report for your own trace format, build it from small fixture traces with unit tests first, then run it on 1 real trace.

```python
#!/usr/bin/env python3
"""Create a comparison report from diag-trace/1 files.

    python3 report.py <trace file or folder>... --out <dir> [--json [<path>]] [--walls <wall.json>...]
                      [--targets <targets.json>] [--near 0.10] [--baseline balanced]
                      [--group-key group] [--chapter-key chapter]

Writes report.md and report.html to --out, and balance.json with --json. Folders are searched at every depth;
JSON files without a diag-trace/1 schema are skipped. The run field "chapter" holds the chapter (level, zone, act)
and the run field "group" holds the group (class, character or vehicle); the spoke field "group" is the spoke's group.
If your traces use other names, pass them with --chapter-key and --group-key; no other field name changes.
Schemas: references/contract-template.md and references/balance-json.md.
"""
import argparse
import html
import json
import statistics
import sys
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path

# The trace field names of the chapter and the group. main() sets them from --chapter-key and --group-key.
CHAPTER_KEY = "chapter"
GROUP_KEY = "group"


def minutes(seconds): return seconds / 60
def trace_label(trace): return "/".join(Path(trace["_file"]).parts[-2:])  # <policy folder>/trace.json
def policy_name(trace): return trace["meta"]["policy"]["name"]
def seed(trace): return int(trace["meta"]["policy"].get("seed", 0) or 0)  # Godot writes JSON numbers as floats.
def major_kills(trace):
    return [(run, boss) for run in trace["runs"] for boss in run.get("bosses", [])
            if boss.get("kind") == "major" and boss.get("result") == "killed" and boss.get("first_kill")]
def progress(trace): return len({boss["id"] for _, boss in major_kills(trace)})
def total_minutes(trace):
    kills = major_kills(trace)
    runs = trace["runs"]
    return minutes((kills[-1][0] if kills else runs[-1]).get("game_s", 0)) if runs else 0
def major_clear_minutes(trace): return {boss["id"]: minutes(run["game_s"]) for run, boss in major_kills(trace)}
def stall(trace):
    best = current = 0
    for run in trace["runs"]:
        quiet = not any(b.get("first_kill") for b in run.get("bosses", [])) and not run.get("new_field", False)
        current = current + 1 if quiet else 0; best = max(best, current)
    return best
def tries_per_kill(trace):
    result = {}
    for i, run in enumerate(trace["runs"]):
        for boss in run.get("bosses", []):
            if boss.get("first_kill") and boss.get("result") == "killed":
                result[boss["id"]] = sum(any(x.get("id") == boss["id"] for x in r.get("bosses", [])) for r in trace["runs"][:i + 1])
    return result
def buys_per_run(trace):
    values = [len(run.get("purchases", [])) for run in trace["runs"]]
    return {"median": statistics.median(values) if values else 0, "min": min(values) if values else 0,
            "zero_share": sum(v == 0 for v in values) / len(values) if values else 0}
def chapter_sequence(trace): return list(dict.fromkeys(run.get(CHAPTER_KEY, "") for run in trace["runs"]))
def currency_availability(trace):
    currencies = trace["meta"].get("data", {}).get("spend_order", [])
    answer = {}
    for chapter in chapter_sequence(trace):
        runs = [r for r in trace["runs"] if r.get(CHAPTER_KEY) == chapter]
        paid = {c: sum(p.get("paid", {}).get(c, 0) for r in runs for p in r.get("purchases", [])) for c in currencies}
        total = sum(paid.values())
        answer[chapter] = {c: {"haul_median": statistics.median([r.get("haul", {}).get(c, 0) for r in runs]),
                              "bank_median": statistics.median([r.get("bank_before", {}).get(c, 0) for r in runs]),
                              "bank_max": max(r.get("bank_before", {}).get(c, 0) for r in runs), "paid": paid[c],
                              "paid_share": paid[c] / total if total else 0} for c in currencies}
    return answer
def spoke_milestones(trace):
    answer = defaultdict(dict)
    for run in trace["runs"]:
        for spoke, level in run.get("levels", {}).items():
            for mark in (5, 10, 15, 20):
                if level >= mark and mark not in answer[spoke]: answer[spoke][mark] = (run["run"], minutes(run["game_s"]))
    return dict(answer)
def growth_curves(trace): return [(minutes(r.get("game_s", 0)), r.get("stats", {}).get("power", 0), r.get("haul_pool", 0)) for r in trace["runs"]]
def rank(traces): return sorted(traces, key=lambda t: (-progress(t), total_minutes(t)))
def near_optimal(traces, near):
    if not traces: return []
    top = max(map(progress, traces)); group = [t for t in traces if progress(t) == top]; best = min(map(total_minutes, group))
    return [t for t in group if total_minutes(t) <= (1 + near) * best]
def seed_groups(traces):
    groups = defaultdict(list)
    for trace in traces: groups[policy_name(trace)].append(trace)
    rows = [(name, len(items), statistics.mean(map(total_minutes, items)), min(map(total_minutes, items)), max(map(total_minutes, items))) for name, items in groups.items()]
    return sorted(rows, key=lambda r: r[2] if r[1] >= 2 else float("inf"))
def leave_one_out(traces):
    rows = []
    for trace in traces:
        policy = trace["meta"]["policy"]
        if len(policy.get("skip", [])) != 1 or not policy.get("baseline"): continue
        base = [t for t in traces if policy_name(t) == policy["baseline"]]
        if not base: continue
        bp = statistics.mean(map(progress, base)); p = progress(trace)
        worth = None if p != bp else total_minutes(trace) - statistics.mean(map(total_minutes, base))
        rows.append((policy_name(trace), policy["skip"][0], p, bp, worth))
    return rows
def policies(traces, near):
    grouped = defaultdict(list)
    for trace in traces: grouped[policy_name(trace)].append(trace)
    rows = []
    for name, items in grouped.items():
        totals = [total_minutes(item) for item in items]
        rows.append({"name": name, "skill": items[0]["meta"]["policy"].get("skill", "strong"),
                     "traces": len(items), "progress": min(map(progress, items)),
                     "total_min_mean": statistics.mean(totals), "total_min_min": min(totals),
                     "total_min_max": max(totals)})
    rows.sort(key=lambda row: (-row["progress"], row["total_min_mean"], row["name"]))
    if rows:
        top = rows[0]["progress"]
        best = min(row["total_min_mean"] for row in rows if row["progress"] == top)
        for index, row in enumerate(rows, 1):
            row["rank"] = index
            row["near_optimal"] = row["progress"] == top and row["total_min_mean"] <= (1 + near) * best
    return rows
def chapter_order(traces):
    order = []
    for trace in traces:
        for chapter in chapter_sequence(trace):
            if chapter not in order: order.append(chapter)
    return order
def chapter_minutes(trace, chapter, order):
    runs = trace["runs"]
    starts = [index for index, run in enumerate(runs) if run.get(CHAPTER_KEY, "") == chapter]
    if not starts: return None
    start = starts[0]
    chapter_index = order.index(chapter)
    if chapter_index + 1 < len(order):
        next_runs = [index for index, run in enumerate(runs) if run.get(CHAPTER_KEY, "") == order[chapter_index + 1]]
        if not next_runs: return None
        end = next_runs[0] - 1
    else:
        kills = [(index, boss) for index, run in enumerate(runs) for boss in run.get("bosses", [])
                 if boss.get("kind") == "major" and boss.get("result") == "killed" and boss.get("first_kill")]
        kills = [item for item in kills if item[0] >= start]
        if not kills: return None
        end = kills[-1][0]
    before = runs[start - 1].get("game_s", 0) if start else 0
    return minutes(runs[end].get("game_s", 0) - before)
def chapters(traces, targets, baseline):
    order = chapter_order(traces)
    names = sorted({policy_name(trace) for trace in traces})
    rows = []
    for index, chapter in enumerate(order, 1):
        target = targets.get(chapter)
        target_min = target.get("min") if isinstance(target, dict) else target
        values = {}
        for name in names:
            minutes_for_policy = [chapter_minutes(trace, chapter, order) for trace in traces if policy_name(trace) == name]
            minutes_for_policy = [value for value in minutes_for_policy if value is not None]
            if minutes_for_policy: values[name] = statistics.mean(minutes_for_policy)
        mark = target.get("mark", "") if isinstance(target, dict) else ""
        if not mark and target_min is not None and baseline in values:
            mark = "SLOW" if values[baseline] > 1.15 * target_min else "FAST" if values[baseline] < .85 * target_min else "OK"
        rows.append({"id": chapter, "order": index, "target_min": target_min, "minutes": values,
                     "best_policy": min(values, key=values.get) if values else "", "mark": mark})
    return rows
def spoke_ids(traces):
    ids = []
    for trace in traces:
        for spoke in trace["meta"].get("spokes", []):
            if spoke["id"] not in ids: ids.append(spoke["id"])
        for run in trace["runs"]:
            for spoke in run.get("levels", {}):
                if spoke not in ids: ids.append(spoke)
            for purchase in run.get("purchases", []):
                if purchase.get("spoke") and purchase["spoke"] not in ids: ids.append(purchase["spoke"])
    return ids
def spokes(traces, policy_rows, wall_files, baseline):
    metadata = {spoke["id"]: spoke for trace in traces for spoke in trace["meta"].get("spokes", [])}
    loo = {skip: (progress_value, baseline_progress, worth) for _, skip, progress_value, baseline_progress, worth in leave_one_out(traces)}
    answers = []
    for spoke in spoke_ids(traces):
        level5 = {}
        closed = []
        for name in {policy_name(trace) for trace in traces}:
            values = []
            for trace in (item for item in traces if policy_name(item) == name):
                for run in trace["runs"]:
                    if run.get("levels", {}).get(spoke, 0) >= 5:
                        values.append(minutes(run.get("game_s", 0))); break
                if any(event.get("kind") == "spoke_closed" and event.get("id") == spoke for run in trace["runs"] for event in run.get("events", [])):
                    closed.append(name)
            level5[name] = statistics.mean(values) if values else None
        worth = loo.get(spoke, (None, None, None))
        breaks = []
        for wall in wall_files:
            for probe in wall.get("probes", []):
                if probe.get("spoke") == spoke:
                    breaks.append({"boss": wall.get("boss", ""), "bump": probe.get("bump", 0),
                                   "health_delta": probe.get("health_delta", 0), "killed": probe.get("result") == "killed"})
        breaks.sort(key=lambda item: item["health_delta"])
        info = metadata.get(spoke, {})
        answers.append({"id": spoke, "kind": info.get("kind", "stat"), "group": info.get(GROUP_KEY, ""),
                        "loo_worth_min": worth[2], "loo_progress_lost": (worth[1] - worth[0]) if worth[2] is None and worth[0] is not None else 0,
                        "level5_min": level5, "closed_in": sorted(set(closed)), "wall_breaks": breaks})
    return answers
def groups(traces, policy_rows):
    near_names = {row["name"] for row in policy_rows if row["near_optimal"]}
    result = {}
    for group in sorted({run.get(GROUP_KEY, "") for trace in traces for run in trace["runs"]}):
        selected = [trace for trace in traces if policy_name(trace) in near_names]
        gains = defaultdict(float); first = {}
        for trace in selected:
            for run in trace["runs"]:
                if run.get(GROUP_KEY, "") != group: continue
                for purchase in run.get("purchases", []):
                    spoke = purchase.get("spoke")
                    if spoke:
                        gains[spoke] += 1
                        first.setdefault(spoke, run.get("run", 0))
        count = len(selected) or 1
        level_gain = {spoke: value / count for spoke, value in gains.items()}
        result[group] = {"chapters": list(dict.fromkeys(run.get(CHAPTER_KEY, "") for trace in traces for run in trace["runs"] if run.get(GROUP_KEY, "") == group)),
                         "spoke_order": sorted(level_gain, key=lambda spoke: (-level_gain[spoke], first[spoke])), "level_gain": level_gain}
    return result
def currencies(traces):
    values = defaultdict(lambda: defaultdict(lambda: {"haul": [], "bank": [], "paid": 0, "total": 0}))
    for trace in traces:
        currencies = trace["meta"].get("data", {}).get("spend_order", [])
        for run in trace["runs"]:
            for currency in currencies:
                entry = values[run.get(CHAPTER_KEY, "")][currency]
                entry["haul"].append(run.get("haul", {}).get(currency, 0)); entry["bank"].append(run.get("bank_before", {}).get(currency, 0))
                paid = sum(purchase.get("paid", {}).get(currency, 0) for purchase in run.get("purchases", []))
                entry["paid"] += paid
                entry["total"] += sum(sum(purchase.get("paid", {}).values()) for purchase in run.get("purchases", []))
    return {chapter: {currency: {"haul_median": statistics.median(data["haul"]), "bank_median": statistics.median(data["bank"]), "bank_max": max(data["bank"]), "paid_share": data["paid"] / data["total"] if data["total"] else 0} for currency, data in items.items()} for chapter, items in values.items()}
def walls(wall_files):
    answer = []
    for wall in wall_files:
        top = [{"spoke": probe.get("spoke", ""), "bump": probe.get("bump", 0), "health_delta": probe.get("health_delta", 0), "killed": probe.get("result") == "killed"} for probe in wall.get("probes", [])]
        answer.append({"boss": wall.get("boss", ""), "try": wall.get("try", 0), "baseline_health_left": wall.get("baseline", {}).get("health_left"), "top": sorted(top, key=lambda item: item["health_delta"])[:5]})
    return answer
def comparability_warnings(traces):
    """Ranks by minutes are fair only between traces with the same stop rule. A short trace (a smoke run, a --max-runs
    cap) looks fast. Warn when traces with the same progress differ in run count by more than 10 %."""
    by_progress = defaultdict(list)
    for trace in traces: by_progress[progress(trace)].append(len(trace["runs"]))
    warnings = []
    for level, counts in sorted(by_progress.items()):
        if len(counts) > 1 and max(counts) > 1.1 * min(counts):
            warnings.append(f"WARNING: {len(counts)} traces with progress {level} have {min(counts)} to {max(counts)} runs; "
                            "they may stop by different rules, so their minutes do not compare. Report 1 sweep with 1 stop rule.")
    return warnings
def balance_json(traces, near, baseline, wall_files, targets, inputs):
    policy_rows = policies(traces, near)
    return {"schema": "skill-tree-balance/1", "generated_utc": datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z"),
            "inputs": {"traces": inputs, "walls": [wall.get("_file", "") for wall in wall_files], "targets": targets.get("_file", "")}, "near": near, "baseline": baseline,
            "policies": policy_rows, "chapters": chapters(traces, targets, baseline), "spokes": spokes(traces, policy_rows, wall_files, baseline),
            "groups": groups(traces, policy_rows), "currencies": currencies(traces), "walls": walls(wall_files), "notes": comparability_warnings(traces)}
def load_json_files(paths, schema=None):
    result = []
    for raw in paths:
        data = json.loads(Path(raw).read_text())
        if schema and data.get("schema") != schema: raise ValueError(f"bad input: {raw} is not {schema}")
        data["_file"] = raw; result.append(data)
    return result
def load_paths(inputs):
    paths = []
    for raw in inputs:
        path = Path(raw)
        # A sweep root holds 1 folder per policy (<root>/<policy>/trace.json), so a folder is searched at every depth.
        # A JSON file with no "schema" key there (state.json, a save snapshot) is not a trace and is skipped silently.
        candidates = sorted(path.rglob("*.json")) if path.is_dir() else [path]
        for candidate in candidates:
            try:
                data = json.loads(candidate.read_text())
            except (OSError, json.JSONDecodeError) as error: print(f"warning: skip {candidate}: {error}", file=sys.stderr); continue
            if not isinstance(data, dict) or (path.is_dir() and data.get("schema") != "diag-trace/1"): continue
            if data.get("schema") != "diag-trace/1": print(f"warning: skip {candidate}: not diag-trace/1", file=sys.stderr); continue
            if "meta" not in data or "runs" not in data: raise ValueError(f"bad trace: {candidate} lacks meta or runs")
            data["_file"] = str(candidate); paths.append(data)
    if not paths: raise ValueError("no trace found")
    return paths
def table(headers, rows):
    line = "| " + " | ".join(headers) + " |\n"; divider = "| " + " | ".join("---" for _ in headers) + " |\n"
    return line + divider + "".join("| " + " | ".join(str(x) for x in row) + " |\n" for row in rows)
def fmt(value): return f"{value:.2f}" if isinstance(value, float) else str(value)
def markdown(traces, near, balance=None):
    out = ["# Diagnostic report\n"] + [f"> {line}\n" for line in comparability_warnings(traces)] + ["## Traces\n", table(["file", "policy", "seed", "runs", "progress", "total minutes"], [[trace_label(t), policy_name(t), seed(t), len(t["runs"]), progress(t), fmt(total_minutes(t))] for t in traces]), "## Rank and the near-optimal set\n"]
    out.append(table(["rank", "policy", "seed", "progress", "total minutes", "near"], [[i, policy_name(t), seed(t), progress(t), fmt(total_minutes(t)), "yes" if t in near_optimal(traces, near) else ""] for i, t in enumerate(rank(traces), 1)]))
    out += ["## Seed groups\n", table(["policy", "count", "mean", "min", "max"], [[name, count, fmt(mean), fmt(low), fmt(high)] for name, count, mean, low, high in seed_groups(traces)]), "## Leave-one-out worth\n"]
    out.append(table(["policy", "skip", "progress", "baseline progress", "worth minutes"], [[name, spoke, p, bp, "LESS PROGRESS" if worth is None else fmt(worth)] for name, spoke, p, bp, worth in leave_one_out(traces)]))
    out += ["## Major clear minutes per trace\n", table(["policy", "boss", "minutes"], [[policy_name(t), boss, fmt(value)] for t in traces for boss, value in major_clear_minutes(t).items()]), "## Stalls, tries per kill, buys per run\n"]
    out.append(table(["policy", "stall", "tries per kill", "buys median", "buys min", "zero share"], [[policy_name(t), stall(t), ", ".join(f"{k}: {v}" for k,v in tries_per_kill(t).items()), fmt(buys_per_run(t)["median"]), buys_per_run(t)["min"], fmt(buys_per_run(t)["zero_share"])] for t in traces]))
    out += ["## Currency availability per chapter\n"]
    rows=[]
    for t in traces:
        for chapter, currencies in currency_availability(t).items():
            for currency, values in currencies.items(): rows.append([policy_name(t), chapter, currency, *map(fmt, values.values())])
    out.append(table(["policy", "chapter", "currency", "haul median", "bank median", "bank max", "paid", "paid share"], rows))
    out += ["## Spoke milestones per trace\n"]
    rows=[]
    for t in traces:
        for spoke, marks in spoke_milestones(t).items():
            rows.append([policy_name(t), spoke, "; ".join(f"{m}: run {v[0]}, {v[1]:.2f}m" for m,v in marks.items())])
    out.append(table(["policy", "spoke", "milestones"], rows))
    if balance is not None:
        out += ["## Balance summary\n"]
        out.append(table(["chapter", "target", "baseline minutes", "mark"], [[row["id"], fmt(row["target_min"]), fmt(row["minutes"].get(balance["baseline"], "")), row["mark"]] for row in balance["chapters"]]))
        out.append(table(["spoke", "loo worth minutes"], [[row["id"], fmt(row["loo_worth_min"])] for row in sorted(balance["spokes"], key=lambda row: float("-inf") if row["loo_worth_min"] is None else -row["loo_worth_min"])[:10]]))
        out.append(table(["group", "spoke order"], [[group, ", ".join(values["spoke_order"][:10])] for group, values in balance["groups"].items()]))
    return "\n".join(out)
def chart(traces, index, title):
    lines=[]
    for n, trace in enumerate(traces):
        points=growth_curves(trace); max_x=max([p[0] for p in points] or [1]); max_y=max([p[index] for p in points] or [1]) or 1
        coords=" ".join(f"{20+260*p[0]/max_x:.1f},{180-150*p[index]/max_y:.1f}" for p in points)
        lines.append(f'<polyline fill="none" stroke="hsl({n*90},70%,40%)" points="{coords}"/><text x="20" y="{20+n*16}">{html.escape(policy_name(trace))}</text>')
    return f"<h2>{title}</h2><svg viewBox=\"0 0 300 200\" role=\"img\">{''.join(lines)}</svg>"
def html_report(traces, near, balance=None):
    body = markdown(traces, near, balance).replace("&", "&amp;").replace("<", "&lt;")
    tables=[]
    for block in body.split("\n\n"):
        if block.startswith("|"):
            lines=block.strip().splitlines(); cells=lambda line: line.strip("|").split("|")
            tables.append("<table>"+"<tr>"+"".join(f"<th>{c.strip()}</th>" for c in cells(lines[0]))+"</tr>"+"".join("<tr>"+"".join(f"<td>{c.strip()}</td>" for c in cells(line))+"</tr>" for line in lines[2:])+"</table>")
        else: tables.append("".join(f"<h2>{line[3:]}</h2>" if line.startswith("## ") else f"<h1>{line[2:]}</h1>" if line.startswith("# ") else "" for line in block.splitlines()))
    return "<!doctype html><html><body>"+"\n".join(tables)+chart(traces,1,"Power against minutes")+chart(traces,2,"Pool haul against minutes")+"</body></html>"
def main(argv=None):
    global CHAPTER_KEY, GROUP_KEY
    parser=argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter); parser.add_argument("inputs", nargs="+"); parser.add_argument("--out", required=True); parser.add_argument("--near", type=float, default=.10); parser.add_argument("--json", nargs="?", const=""); parser.add_argument("--walls", nargs="*", default=[]); parser.add_argument("--targets"); parser.add_argument("--baseline", default="balanced")
    parser.add_argument("--chapter-key", default="chapter", help="the run field that holds the chapter id"); parser.add_argument("--group-key", default="group", help="the run and spoke field that holds the group id"); args=parser.parse_args(argv)
    CHAPTER_KEY, GROUP_KEY = args.chapter_key, args.group_key
    try: traces=load_paths(args.inputs)
    except ValueError as error: print(error, file=sys.stderr); return 2
    try:
        wall_files = load_json_files(args.walls, "wall-probe/1")
        targets = load_json_files([args.targets])[0] if args.targets else {}
    except (OSError, ValueError, json.JSONDecodeError) as error: print(error, file=sys.stderr); return 2
    output = Path(args.out); output.mkdir(parents=True, exist_ok=True)
    balance = balance_json(traces, args.near, args.baseline, wall_files, targets, args.inputs)
    output.joinpath("report.md").write_text(markdown(traces, args.near, balance))
    output.joinpath("report.html").write_text(html_report(traces, args.near, balance))
    if args.json is not None:
        json_path = Path(args.json) if args.json else output / "balance.json"
        json_path.write_text(json.dumps(balance, indent=2) + "\n")
    return 0
if __name__ == "__main__": raise SystemExit(main())
```
