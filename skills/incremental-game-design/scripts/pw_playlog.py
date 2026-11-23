#!/usr/bin/env python3
"""The play log analysis (references/play-data.md).

The script reads a folder of play logs (1 JSON Lines file for each session). For each live fun value it reports the
value at the "felt good" marks (F7), at the "dragged" marks (F8) and at the quit points, the quit-point histogram
over play time and the settings of each session. It proposes a band edge for a value only when the counts are
sufficient. A proposed band is advice from a small sample: the output states the counts beside it.

Usage: pw_playlog.py <folder> [--json FILE] [--md FILE] [--window S] [--bin M]
"""
import argparse
import json
import os
import pathlib
import statistics
import sys

SCHEMA = "playlog-analysis/1"
MINUTE = 60.0
WINDOW_S = 90.0
BIN_MIN = 10.0
MIN_MARKS = 50
MIN_MARK_SESSIONS = 5
MIN_QUITS = 20
MIN_GROUP = 5
MIN_SPLIT = 0.75
GOOD, DRAG, NOTE = "good", "drag", "note"
ADVICE = ("A proposed band is advice from a small sample. It shows the taste of the players who marked, not a "
          "proof. The counts are beside each proposal.")


def value(name, unit, row=None, key=None):
    """1 known fun value: its name, its unit in the log, and the fun sheet row and band key it fits."""
    return dict(name=name, unit=unit, row=row, band_key=key)


VALUES = {
    "fm1_now": value("content drought", "s", "FM-1", "max"),
    "fm1_h1": value("largest content gap in hour 1", "s", "FM-1", "max_h1"),
    "fm1_gap": value("largest content gap", "s"),
    "fm2_last": value("gain of the last hand purchase", "%"),
    "fm2_share": value("share of hand purchases below +5 %", "share", "FM-2", "max"),
    "fm3": value("distinct share of the lines", "share", "FM-3", "min"),
    "fm4_now": value("time since the last jolt", "s", "FM-4", "max"),
    "fm4_gap": value("largest gap between jolts", "s"),
    "fm6_chore": value("largest chore", "presses", "FM-6", "max_presses"),
    "fm6_hand": value("hand share in hour 1", "share", "FM-6", "min_hand_share_h1"),
    "fm7": value("verb count of the hour", "verbs", "FM-7", "min"),
    "fm8": value("last comeback ratio", "ratio"),
    "fm9": value("top-line share of the hour", "share", "FM-9", "max"),
    "fm10": value("felt beat share", "share", "FM-10", "min"),
    "fm12": value("reset novelty share", "share", "FM-12", "min"),
    "fm16": value("tend rate", "per min"),
    "pc2_now": value("time since the last hand purchase", "s"),
    "pc2_gap": value("largest hand purchase gap", "s"),
    "pc4_now": value("time since the last unlock", "s"),
    "pc4_gap_h1": value("largest unlock gap in hour 1", "s"),
    "pc4_gap": value("largest unlock gap", "s"),
    "pc6": value("hand purchases in the last minute", "purchases"),
    "growth": value("income growth each minute", "ln/min"),
}
SKIP_KEYS = ("out",)
SIDE_OF_KEY = {"max": "above", "min": "below"}


class PlaylogError(Exception):
    """A log that the script cannot read: the message names the file and the line."""


def parse_line(path, number, text):
    try:
        row = json.loads(text)
    except ValueError as exc:
        raise PlaylogError(f"{path}:{number}: the line is not valid JSON ({exc}). Delete or repair the line.") from exc
    if not isinstance(row, dict) or not isinstance(row.get("e"), str):
        raise PlaylogError(f"{path}:{number}: the line is not a JSON object with a field \"e\".")
    if not isinstance(row.get("rt", 0), (int, float)):
        raise PlaylogError(f"{path}:{number}: the field \"rt\" is not a number.")
    return row


def read_file(path, warnings=None):
    """The rows of 1 log. A cut last line (not valid JSON) is left out with a warning; a broken line elsewhere is an
    error. The game repairs its newest log at start, but an old log can still end in a cut line."""
    lines = [(n, text) for n, text in enumerate(pathlib.Path(path).read_text(encoding="utf-8").splitlines(), 1)
             if text.strip()]
    if lines and not is_json(lines[-1][1]):
        n = lines.pop()[0]
        warn(warnings, f"{path}:{n}: the last line is cut (not valid JSON). The analysis leaves it out.")
    rows = [parse_line(path, n, text) for n, text in lines]
    if not rows:
        raise PlaylogError(f"{path}: the file has no line.")
    return rows


def is_json(text):
    try:
        json.loads(text)
    except ValueError:
        return False
    return True


def warn(warnings, text):
    print(f"pw_playlog: warning: {text}", file=sys.stderr)
    if warnings is not None:
        warnings.append(text)


def log_files(folder):
    if not os.path.isdir(folder):
        raise PlaylogError(f"{folder}: the folder does not exist.")
    names = sorted(n for n in os.listdir(folder) if n.endswith(".jsonl"))
    if not names:
        raise PlaylogError(f"{folder}: the folder has no .jsonl file.")
    return [os.path.join(folder, n) for n in names]


def rt_of(row):
    return float(row.get("rt") or 0.0)


def fun_samples(rows):
    """(rt, fun) of each line with a "fun" object, in the order of the file."""
    return [(rt_of(r), r["fun"]) for r in rows if isinstance(r.get("fun"), dict)]


def sample_at(samples, at, window):
    """The last fun sample in [at - window, at], or None."""
    inside = [fun for t, fun in samples if at - window <= t <= at]
    return inside[-1] if inside else None


def quit_row(rows):
    """The quit point: the last line before session_end, else the last line. None for a session with 1 line."""
    body = rows[:-1] if rows[-1]["e"] == "session_end" else rows
    body = [r for r in body if r["e"] != "session_start"]
    return body[-1] if body else None


def session_settings(rows):
    start = rows[0] if rows[0]["e"] == "session_start" else {}
    return dict(set_at_start=start.get("set"), controls=start.get("controls") or {},
                setting_loads=[dict(minute=round(rt_of(r) / MINUTE, 1), name=r.get("name"))
                               for r in rows if r["e"] == "setting_load"],
                control_changes=[dict(minute=round(rt_of(r) / MINUTE, 1), control=r.get("control"),
                                      before=r.get("from"), after=r.get("to"))
                                 for r in rows if r["e"] == "control_change"])


def session_end_info(rows):
    end = rows[-1] if rows[-1]["e"] == "session_end" else None
    if end is None:
        return dict(length_s=rt_of(rows[-1]), active_s=None, recovered=True, end_written=False)
    return dict(length_s=end.get("length_s", rt_of(end)), active_s=end.get("active_s"),
                recovered=bool(end.get("recovered")), end_written=True)


def event_record(row, samples, window):
    """The minute, the setting and the fun values of 1 event. A line with its own fun object uses it."""
    own = row.get("fun")
    return dict(minute=round(rt_of(row) / MINUTE, 2), set=row.get("set"),
                fun=own if isinstance(own, dict) else sample_at(samples, rt_of(row), window))


def quit_record(q, samples, window):
    if q is None:
        return None
    return dict(event_record(dict(q, fun=None), samples, window), stage=q.get("stage"), event=q["e"])


def session_head(path, rows):
    start = rows[0] if rows[0]["e"] == "session_start" else {}
    return dict(sid=rows[0].get("sid"), file=os.path.basename(path), build=start.get("build"),
                data_hash=start.get("data_hash"), save_state=start.get("save_state"), lines=len(rows))


def read_session(path, window, warnings=None):
    rows = read_file(path, warnings)
    samples = fun_samples(rows)
    marks = [dict(valence=r.get("valence"), **event_record(r, samples, window)) for r in rows if r["e"] == "mark"]
    q = quit_row(rows)
    return dict(session_head(path, rows), marks=marks, quit=quit_record(q, samples, window),
                others=other_samples(samples, q, window), **session_end_info(rows), **session_settings(rows))


def other_samples(samples, q, window):
    """The fun samples of a session before the window of its quit point (the play that went on)."""
    if q is None:
        return [fun for _, fun in samples]
    return [fun for t, fun in samples if t < rt_of(q) - window]


def shown(key, raw):
    """A value of the log in the unit of the report: seconds become minutes."""
    if not isinstance(raw, (int, float)) or isinstance(raw, bool):
        return None
    return raw / MINUTE if unit_of(key) == "s" else float(raw)


def unit_of(key):
    return VALUES.get(key, value(key, ""))["unit"]


def shown_unit(key):
    return "min" if unit_of(key) == "s" else unit_of(key)


def values_of(funs, key):
    out = [shown(key, f.get(key)) for f in funs if f]
    return [v for v in out if v is not None]


def summary(values):
    if not values:
        return dict(n=0, median=None, min=None, max=None)
    return dict(n=len(values), median=r3(statistics.median(values)), min=r3(min(values)), max=r3(max(values)))


def r3(x):
    return None if x is None else round(x, 3)


def value_keys(groups):
    found = [k for funs in groups.values() for f in funs for k in (f or {}) if k not in SKIP_KEYS]
    return list(dict.fromkeys(list(VALUES) + found))


def mark_groups(sessions):
    marks = [m for s in sessions for m in s["marks"]]
    return {GOOD: [m["fun"] for m in marks if m["valence"] == GOOD],
            DRAG: [m["fun"] for m in marks if m["valence"] == DRAG],
            "quit": [s["quit"]["fun"] for s in sessions if s["quit"]]}


def value_table(groups):
    table = {}
    for key in value_keys(groups):
        meta = VALUES.get(key, value(key, ""))
        table[key] = dict(meta, unit=shown_unit(key), **{g: summary(values_of(funs, key)) for g, funs in groups.items()})
    return table


def correct_count(edge, side, high, low):
    """The samples on the correct side: `high` above the edge and `low` at or below it, or the reverse."""
    if side == "above":
        return sum(v > edge for v in high) + sum(v <= edge for v in low)
    return sum(v < edge for v in high) + sum(v >= edge for v in low)


def best_edge(high, low, prefer):
    """(edge, side, correct) of the midpoint that puts the most samples on the correct side; None with 1 value.
    `high` is the group that must be beyond the edge. A tie keeps the side `prefer`, then the lowest edge."""
    points = sorted(set(high + low))
    if len(points) < 2:
        return None
    mids = [(a + b) / 2 for a, b in zip(points, points[1:])]
    sides = [prefer] + [s for s in ("above", "below") if s != prefer]
    best = max(((correct_count(m, s, high, low), -sides.index(s), -m, s) for s in sides for m in mids))
    return -best[2], best[3], best[0]


def edge_record(key, high, low, prefer):
    found = best_edge(high, low, prefer)
    if found is None:
        return dict(value=key, reason="all samples have 1 value")
    edge, side, correct = found
    beyond = correct_count(edge, side, high, [])
    inside = correct_count(edge, side, [], low)
    return dict(value=key, edge=r3(edge), unit=shown_unit(key), side=side, beyond=beyond, beyond_n=len(high),
                inside=inside, inside_n=len(low), share=r3(correct / (len(high) + len(low))))


def unit_words(key):
    unit = shown_unit(key)
    return {"min": " minutes", "share": "", "ratio": ""}.get(unit, f" {unit}")


def sentence(rec, who):
    name = VALUES.get(rec["value"], value(rec["value"], ""))["name"]
    return (f"the {who} come after a {name} {rec['side']} {rec['edge']:g}{unit_words(rec['value'])} "
            f"({rec['beyond']} of {rec['beyond_n']} {rec['who_n']} {rec['side']}; {rec['inside']} of "
            f"{rec['inside_n']} {rec['other_n']} on the other side)")


def prefer_side(key):
    return SIDE_OF_KEY.get((VALUES.get(key, {}).get("band_key") or "")[:3], "above")


def group_edges(key_table, high, low, labels):
    """An edge record for each value with MIN_GROUP samples or more on each side."""
    return [value_edge(key, values_of(high, key), values_of(low, key), labels) for key in key_table]


def value_edge(key, h, lo, labels):
    short = dict(value=key, beyond_n=len(h), inside_n=len(lo),
                 reason=f"not enough data: {len(h)} and {len(lo)} samples; each side needs {MIN_GROUP}")
    return short if min(len(h), len(lo)) < MIN_GROUP else judge(edge_record(key, h, lo, prefer_side(key)), labels)


def judge(rec, labels):
    if "edge" not in rec:
        return rec
    rec.update(who_n=labels[1], other_n=labels[2])
    if rec["share"] < MIN_SPLIT:
        rec["reason"] = f"no clear edge: {rec['share']:.2f} of the samples on the correct side; a band needs {MIN_SPLIT}"
        return rec
    rec["sentence"] = sentence(rec, labels[0])
    return rec


def mark_counts(sessions):
    marks = [m for s in sessions for m in s["marks"]]
    return dict(good=sum(m["valence"] == GOOD for m in marks), drag=sum(m["valence"] == DRAG for m in marks),
                note=sum(m["valence"] == NOTE for m in marks),
                sessions_with_marks=sum(1 for s in sessions if any(m["valence"] in (GOOD, DRAG) for m in s["marks"])))


def mark_proposals(sessions, table, groups):
    c = mark_counts(sessions)
    n = c["good"] + c["drag"]
    if n < MIN_MARKS or c["sessions_with_marks"] < MIN_MARK_SESSIONS:
        return dict(enough=False, reason=f"not enough data: {n} marks (F7 {c['good']}, F8 {c['drag']}) over "
                                         f"{c['sessions_with_marks']} sessions; a proposal needs {MIN_MARKS} marks "
                                         f"over {MIN_MARK_SESSIONS} sessions", edges=[])
    edges = group_edges(table, groups[DRAG], groups[GOOD], ("F8 marks", "F8 marks", "F7 marks"))
    return dict(enough=True, reason=None, edges=edges)


def quit_proposals(sessions, table, groups):
    n = len(groups["quit"])
    if n < MIN_QUITS:
        return dict(enough=False, reason=f"not enough data: {n} quit points; a proposal needs {MIN_QUITS}", edges=[])
    others = [f for s in sessions for f in s["others"]]
    edges = group_edges(table, groups["quit"], others, ("quit points", "quits", "heartbeats that went on"))
    return dict(enough=True, reason=None, edges=edges)


def target_note(rec):
    meta = VALUES.get(rec["value"], {})
    key = meta.get("band_key")
    if not key:
        return None, "the value has no band key of the fun sheet"
    if "sentence" not in rec:
        return None, rec.get("reason")
    if SIDE_OF_KEY[key[:3]] != rec["side"]:
        return None, (f"the F8 marks are {rec['side']} the edge, but the band key {meta['row']} {key} needs them "
                      f"{SIDE_OF_KEY[key[:3]]}: no band")
    return (meta["row"], key), None


def fun_targets(marks, counts):
    """The block for game.targets.fun: 1 band key for each supported edge, with its mark and counts."""
    places = [(mark_target(rec), rec["edge"] if "edge" in rec else None) for rec in marks["edges"]]
    out = {}
    for place, edge in (p for p in places if p[0]):
        out.setdefault(place[0], {"band_mark": band_mark(counts)})[place[1]] = edge
    return out


def mark_target(rec):
    """(row, band key) of a supported edge, else None. Writes in_targets and targets_note into the record."""
    place, note = target_note(rec)
    rec.update(in_targets=place is not None, targets_note=note)
    return place


def band_mark(c):
    return (f"play data (developer: {c['drag']} F8 and {c['good']} F7 marks over {c['sessions_with_marks']} "
            f"sessions; small sample)")


def histogram(sessions, bin_min):
    minutes = [s["quit"]["minute"] for s in sessions if s["quit"]]
    if not minutes:
        return dict(bin_minutes=bin_min, bins=[])
    count = int(max(minutes) // bin_min) + 1
    bins = [dict(from_min=i * bin_min, to_min=(i + 1) * bin_min, quits=0) for i in range(count)]
    for m in minutes:
        bins[int(m // bin_min)]["quits"] += 1
    return dict(bin_minutes=bin_min, bins=bins)


def marks_by_setting(sessions):
    out = {}
    for m in (m for s in sessions for m in s["marks"]):
        row = out.setdefault(str(m["set"]), {GOOD: 0, DRAG: 0, NOTE: 0})
        row[m["valence"]] = row.get(m["valence"], 0) + 1
    return out


def read_sessions(folder, window):
    """(the sessions of the folder, the warnings of the reading)."""
    warnings = []
    return [read_session(p, window, warnings) for p in log_files(folder)], warnings


def analyse(folder, window=WINDOW_S, bin_min=BIN_MIN):
    sessions, warnings = read_sessions(folder, window)
    return dict(summarise(sessions, folder, window, bin_min), warnings=warnings)


def summarise(sessions, folder, window, bin_min):
    groups = mark_groups(sessions)
    table = value_table(groups)
    counts = dict(sessions=len(sessions), quits=len(groups["quit"]), **mark_counts(sessions))
    marks = mark_proposals(sessions, table, groups)
    quits = quit_proposals(sessions, table, groups)
    targets = dict(fun=fun_targets(marks, counts))
    return dict(schema=SCHEMA, folder=folder, window_s=window, counts=counts, values=table,
                quit_histogram=histogram(sessions, bin_min), marks_by_setting=marks_by_setting(sessions),
                proposals=dict(marks=marks, quits=quits), targets=targets, advice=ADVICE,
                sessions=[{k: v for k, v in s.items() if k != "others"} for s in sessions])


def cell(s):
    return "-" if not s["n"] else f"{s['median']:g} [{s['min']:g} to {s['max']:g}] n={s['n']}"


def value_lines(result):
    rows = [(k, v) for k, v in result["values"].items() if v[GOOD]["n"] or v[DRAG]["n"] or v["quit"]["n"]]
    return ["| Value | Unit | F7 marks | F8 marks | Quit points |", "| --- | --- | --- | --- | --- |"] + [
        f"| {v['name']} (`{k}`) | {v['unit']} | {cell(v[GOOD])} | {cell(v[DRAG])} | {cell(v['quit'])} |"
        for k, v in rows]


def histogram_lines(h):
    if not h["bins"]:
        return ["No quit point."]
    return [f"- {b['from_min']:g} to {b['to_min']:g} min: {b['quits']}" for b in h["bins"]]


def setting_text(s):
    changes = ", ".join(f"{c['control']} {c['before']} to {c['after']} at {c['minute']:g} min"
                        for c in s["control_changes"]) or "none"
    loads = ", ".join(f"\"{x['name']}\" at {x['minute']:g} min" for x in s["setting_loads"]) or "none"
    quit_set = s["quit"]["set"] if s["quit"] else "-"
    return (f"- `{s['file']}`: {s['length_s'] / MINUTE:.1f} min; setting at start \"{s['set_at_start']}\"; loads: "
            f"{loads}; control changes: {changes}; setting at the quit point \"{quit_set}\"; marks: "
            f"{len(s['marks'])}{'; no session_end (recovered)' if s['recovered'] else ''}")


def proposal_lines(p):
    if not p["enough"]:
        return [p["reason"] + "."]
    out = [f"- {r['sentence']}." for r in p["edges"] if "sentence" in r]
    return out or ["No value has a clear edge."]


def report(result):
    c = result["counts"]
    out = [f"# Play log analysis: {result['folder']}", "",
           f"Sessions: {c['sessions']}. Marks: F7 {c['good']}, F8 {c['drag']}, notes {c['note']}, over "
           f"{c['sessions_with_marks']} sessions. Quit points: {c['quits']}.",
           f"The value at an event is the last sample in the {result['window_s']:g} s before it.", ""]
    out += [f"Warning: {w}" for w in result["warnings"]] + ([""] if result["warnings"] else [])
    out += ["## Values at the marks and the quit points", "", "Each cell: median [min to max] n.", ""]
    out += value_lines(result)
    out += ["", f"## Quit points over play time ({result['quit_histogram']['bin_minutes']:g} min bins)", ""]
    out += histogram_lines(result["quit_histogram"])
    out += ["", "## Settings of each session", ""] + [setting_text(s) for s in result["sessions"]]
    out += ["", "## Marks under each setting", ""]
    out += [f"- \"{k}\": F7 {v[GOOD]}, F8 {v[DRAG]}" for k, v in result["marks_by_setting"].items()] or ["No mark."]
    out += ["", "## Proposed band edges from the marks", ""] + proposal_lines(result["proposals"]["marks"])
    out += ["", "## Proposed band edges from the quit points", ""] + proposal_lines(result["proposals"]["quits"])
    out += ["", "## Bands for game.targets.fun", "", json.dumps(result["targets"]["fun"], indent=1) if
            result["targets"]["fun"] else "No band: the data supports none.", "", result["advice"]]
    return "\n".join(out) + "\n"


def write_text(path, text):
    pathlib.Path(path).write_text(text, encoding="utf-8")


def main(argv=None):
    ap = argparse.ArgumentParser(description="Relate the marks and the quit points of play logs to the fun values.")
    ap.add_argument("folder")
    ap.add_argument("--json")
    ap.add_argument("--md")
    ap.add_argument("--window", type=float, default=WINDOW_S)
    ap.add_argument("--bin", type=float, default=BIN_MIN)
    args = ap.parse_args(argv)
    try:
        result = analyse(args.folder, args.window, args.bin)
    except PlaylogError as exc:
        print(f"pw_playlog: {exc}", file=sys.stderr)
        return 2
    text = report(result)
    print(text, end="")
    if args.md:
        write_text(args.md, text)
    if args.json:
        write_text(args.json, json.dumps(result, indent=1) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
