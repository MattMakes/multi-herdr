#!/usr/bin/env python3
"""The content step: how many content elements the targets need, and what the selection or the content file gives.

references/content-step.md is the text of this file. A content element has the meaning of the proof row CG
(references/proof-sheet.md): a first purchase of a line, a first milestone or the first reset. A stage of the spine
alone is not a content element.

    pw_content.py need <targets.json> [--selection <selection.json>] [--content <content.json>] [--json <file>]
    pw_content.py need <targets.json> --content <content.json> --model <model-built.json> [--json <file>]
    pw_content.py template <targets.json> [--count N] [--selection <selection.json>] [--out <content.json>]

Each limit comes from the code of the proof rows: the content gap band of row CG (pw_design.target_values: fun_gap,
fun_gap_h1), the unlock gap targets of row PC4 (ugap_max, ugap_first), the parts of row PC3 (pw_proof.PART_HOURS,
pw_proof.extension_parts) and the gap count of row CG (pw_proof.content_gaps). This file holds no limit of its own.

Exit code 0: no element is missing. 1: content is missing. 2: bad input (also an element with no name).
"""

import argparse
import json
import math
import os
import pathlib
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import pw_design as pd  # noqa: E402
import pw_model  # noqa: E402
import pw_players  # noqa: E402
import pw_proof  # noqa: E402
import pw_rebalance as rb  # noqa: E402

EPS = 1e-9
HOUR_MIN = pw_proof.HOUR / 60.0
SLOT_NAME = re.compile(r"^(content|element|slot)[_ -]?\d*$|the designer names it", re.I)
TIME_KEY = "opens_min"
MAIN = "coin"                                   # the id of the main currency in each model of the solver
# What the solver writes for a selected system with no content file: pd.SOLVER_WRITES. S02 is in s02_lines().
NOT_WRITTEN = ("S04.none",)                     # a variant that gives no element
USAGE = ("need <targets.json> [--selection FILE] [--content FILE [--model FILE]] [--json FILE]\n"
         "template <targets.json> [--count N] [--selection FILE] [--out FILE]")
ARGS = {
    "need": ((("--selection",), dict(metavar="FILE", help="the selection so far: count the elements that the "
                                                          "solver writes for it")),
             (("--content",), dict(metavar="FILE", help="the content file: count its named elements at their "
                                                        "times (key opens_min); it replaces --selection")),
             (("--model",), dict(metavar="FILE", help="with --content of a built game: the model as built "
                                                      "(pw_design.py --as-built); the times come from its active player")),
             (("--json",), dict(metavar="FILE", help="write the result as JSON"))),
    "template": ((("--count",), dict(type=int, metavar="N", help="the count of new elements (default: the need)")),
                 (("--selection",), dict(metavar="FILE", help="also write 1 frame for each line of the selection")),
                 (("--out",), dict(metavar="FILE", help="write the frame of the content file (default: print it)"))),
}


# ----------------------------------------------------------------------------- the limits and the need
def limits(tg):
    """The limits of the rows CG, PC4 and PC3 from the targets file, in minutes."""
    tv = pd.target_values(tg)
    return dict(end=tv["H"] * HOUR_MIN, content_h1=tv["fun_gap_h1"], content=tv["fun_gap"],
                unlock_h1=tv["ugap_first"], unlock=tv["ugap_max"], demo=tv["demo_min"],
                part=pw_proof.PART_HOURS * HOUR_MIN, pattern=tg.get("play_pattern"))


def places(end, first, later):
    """The fewest times in minutes, so that no gap is above first in the first hour or above later after it."""
    early = steps(0.0, first, min(HOUR_MIN, end))
    return early + steps(([0.0] + early)[-1], later, end)


def steps(t, step, end):
    """The times t + step, t + 2 step, ... that leave a gap of step or less before end: ceil((end - t) / step) - 1
    times, the count of row CG for 1 gap (pw_proof.long_gaps)."""
    n = int(math.ceil((end - t) / step - EPS)) - 1
    return [t + step * (i + 1) for i in range(max(0, n))]


def marks(times, end):
    """The marks of row CG in seconds: 0, each time inside the run, and the end."""
    return [0.0] + sorted(t * 60.0 for t in times if EPS < t < end - EPS) + [end * 60.0]


def gaps(times, end, first, later):
    """The long gaps of row CG (pw_proof.content_gaps): the start, the end and the count of more elements. The row
    gives each gap as a tuple that starts with (start, end, gap, more)."""
    found = pw_proof.content_gaps(marks(times, end), later, first)
    return [dict(start_minutes=round(g[0], 2), end_minutes=round(g[1], 2), more=g[3]) for g in found]


def count_h1(times):
    return len([t for t in times if EPS < t < HOUR_MIN - EPS])


def need_of(lim):
    """The need of row CG, and the unlock need of row PC4 (the stages of the spine count for PC4 only)."""
    content = places(lim["end"], lim["content_h1"], lim["content"])
    unlock = places(lim["end"], lim["unlock_h1"], lim["unlock"])
    return dict(first_hour=count_h1(content), whole=len(content), times_min=[round(t, 2) for t in content],
                unlock_first_hour=count_h1(unlock), unlock_whole=len(unlock))


# ----------------------------------------------------------------------------- what the selection gives
def s02_lines(entries):
    """1 line for each S02 entry that is not care. With 1 entry or none, the solver writes 2 groups."""
    groups = [e for e in entries if e["system"] == "S02" and e.get("role") != "care"]
    many = [dict(id=e["id"], system="S02", text=e.get("role") or "a multiplier group") for e in groups]
    two = [dict(id=i, system="S02", text="a multiplier group") for i in ("m1", "m2")]
    return many if len(groups) > 1 else two


def selection_elements(sel, cat):
    """The elements that the solver writes for the selection (references/content-step.md, "What the selection
    gives"). Each has no time: the solver opens it in the first minutes."""
    entries = pd.selection_entries(sel, cat)
    chosen = pd.chosen_systems(sel, cat)
    fixed = [dict(id=i, system=s, text=text) for s, i, text in pd.SOLVER_WRITES
             if s in chosen and chosen[s] not in NOT_WRITTEN]
    lines = s02_lines(entries) if "S02" in chosen else []
    return [dict(e, name="", opens_min=0.0) for e in fixed[:1] + lines + fixed[1:]]


def given_selection(path, lim):
    els = selection_elements(pd.read_json(path, "selection"), pd.Catalog())
    start = [min(lim["content_h1"], lim["end"] / 2.0)] if els else []
    note = ("the lines of the selection open in the first minutes, so together they fill 1 place of hour 1 "
            "(the best case)")
    return dict(source="selection", elements=els, times=start, note=note)


# ----------------------------------------------------------------------------- what the content file gives
def name_fault(e):
    name = e.get("name")
    bad = not isinstance(name, str) or not name.strip() or name.strip() == e.get("id") or SLOT_NAME.search(name)
    return "no name" if bad else None


def time_fault(e):
    t = e.get(TIME_KEY, None if e.get("unlock") else 0.0)
    ok = isinstance(t, (int, float)) and not isinstance(t, bool) and t >= 0
    return None if ok else f"no opening time ({TIME_KEY})"


def card_fault(e, cat):
    ok = e.get("kind") in rb.KINDS and e.get("system") in cat.systems
    return None if ok else f"no kind (1 of {', '.join(rb.KINDS)}) or no system card"


def faults(e, cat):
    found = [f for f in (name_fault(e), time_fault(e), card_fault(e, cat)) if f]
    where = f"minute {e[TIME_KEY]:g}" if isinstance(e.get(TIME_KEY), (int, float)) else "no time"
    return [f"{e.get('id')} ({where}): {', '.join(found)}"] if found else []


def read_elements(path):
    data = pd.read_json(path, "content")
    els = data.get("elements") if isinstance(data, dict) else None
    pd.check(not isinstance(els, list) or not all(isinstance(e, dict) for e in els),
             "the content file needs a list of elements (references/content-step.md)")
    return els


def given_content(path, lim):
    """The named elements of the content file at their times. An element with no name, no time, no kind or no
    system card is bad input: a slot is never a result."""
    cat = pd.Catalog()
    els = read_elements(path)
    bad = [text for e in els for text in faults(e, cat)]
    pd.check(bad and all(TIME_KEY not in e for e in els), NO_TIME_TEXT)
    pd.check(bad, f"content is missing: {len(bad)} elements of {path} are not complete, so they do not count: "
             + "; ".join(bad))
    out = [dict(id=e["id"], system=e["system"], name=e["name"], opens_min=float(e.get(TIME_KEY, 0.0))) for e in els]
    return dict(source="content", elements=out, times=[e["opens_min"] for e in out], note="")


# ----------------------------------------------------------------------------- the content file of a built game
BUILT_KINDS = ("line", "reward", "system")      # a grant and the stages (S03) are not content elements (row CG)
NO_TIME_TEXT = ("the content file of the scan has no opening times: add --model with the model as built "
                "(pw_design.py --as-built), so that the times come from its active player")


def built_times(path):
    """{element id: minute}: the first purchase of each line, the first time of each milestone, and the first reset
    (id reset) of the active player of the model as built."""
    m = pw_model.load_model(path)
    spec = next(p for p in pw_players.PLAYERS if p["name"] == pw_players.BASE)
    sim = pw_players.Players(m, m["game"]["target_hours"]).run(spec)
    out = {}
    for e in sim.events:
        key = "reset" if e["kind"] == "prestige" else e["id"] if pw_proof.is_unlock(e) else None
        out.setdefault(key, e["real"] / 60.0)
    out.pop(None, None)
    return out


def built_element(e):
    return e.get("kind") in BUILT_KINDS and e.get("system") != "S03"


def given_built(path, model, lim):
    """The elements of the content file of a built game at the times of the active player of the model as built.
    The game names each element, so the name check and the card check do not apply. An element that the player does
    not reach before the end is not given."""
    times = built_times(model)
    els = [e for e in read_elements(path) if built_element(e)]
    out = [dict(id=e["id"], system=e.get("system"), name=e.get("name") or e["id"], opens_min=round(times[e["id"]], 2))
           for e in els if e["id"] in times]
    gone = [e["id"] for e in els if e["id"] not in times]
    note = ("not reached by the active player of the model as built, so not given: " + ", ".join(gone)) if gone else ""
    return dict(source="content", elements=out, times=[e["opens_min"] for e in out], note=note)


def given_of(a, lim):
    if a.content and a.model:
        return given_built(a.content, a.model, lim)
    if a.content:
        return given_content(a.content, lim)
    if a.selection:
        return given_selection(a.selection, lim)
    return dict(source="none", elements=[], times=[], note="")


# ----------------------------------------------------------------------------- the shortfall
def demo_parts(times, lim):
    """Row PC3: the parts past the demo end, each with the count of elements that open in it."""
    if lim["demo"] is None or lim["demo"] >= lim["end"]:
        return []
    parts = pw_proof.extension_parts(lim["demo"], lim["end"], lim["part"])
    return [dict(part=i + 1, start_minutes=round(lo, 2), end_minutes=round(hi, 2),
                 elements=len([t for t in times if lo < t <= hi + EPS])) for i, (lo, hi) in enumerate(parts)]


def shortfall_of(times, lim):
    where = gaps(times, lim["end"], lim["content_h1"], lim["content"])
    early = sum(g["more"] for g in where if g["end_minutes"] <= HOUR_MIN + EPS)
    parts = demo_parts(times, lim)
    return dict(first_hour=early, whole=sum(g["more"] for g in where), where=where, parts_past_demo=parts,
                empty_parts=[p["part"] for p in parts if not p["elements"]])


def result_of(a):
    lim = limits(pd.read_json(a.targets, "targets"))
    pd.check(lim["end"] <= 0, "the targets file needs length_h.value above 0")
    given = given_of(a, lim)
    times = given.pop("times")
    given.update(count=len(given["elements"]), first_hour=count_h1(times) if given["source"] == "content" else None)
    return dict(limits=lim, need=need_of(lim), given=given, shortfall=shortfall_of(times, lim))


# ----------------------------------------------------------------------------- the printed lines
def need_lines(r):
    lim, n = r["limits"], r["need"]
    out = [f"content need (row CG: no gap above {lim['content_h1']:g} min in hour 1 and {lim['content']:g} min "
           f"after it): {n['first_hour']} elements in hour 1, {n['whole']} over {lim['end']:g} min",
           f"unlock need (row PC4: no gap above {lim['unlock_h1']:g} min in hour 1 and {lim['unlock']:g} min after "
           f"it): {n['unlock_first_hour']} in hour 1, {n['unlock_whole']} over the whole length; the stages of the "
           "spine count for PC4, not for CG"]
    other = [] if lim["pattern"] == "continuous" else ["the rows CG, PC3 and PC4 are rows of the play pattern "
                                                        "continuous; for check_in the need is the band of FM-1 only"]
    return out + other


def given_lines(g):
    if g["source"] == "none":
        return ["given: nothing (no --selection and no --content)"]
    ids = ", ".join(e["name"] or e["id"] for e in g["elements"]) or "none"
    h1 = "" if g["first_hour"] is None else f", {g['first_hour']} of them inside hour 1 after the start"
    note = [f"  {g['note']}"] if g["note"] else []
    return [f"the {g['source']} gives: {g['count']} elements{h1} ({ids})"] + note


def shortfall_lines(s):
    if not s["whole"] and not s["empty_parts"]:
        return ["shortfall: 0 in hour 1, 0 over the whole length"]
    out = [f"content is missing: {s['first_hour']} elements in hour 1, {s['whole']} over the whole length"]
    out += [f"  {g['more']} more between minute {g['start_minutes']:g} and minute {g['end_minutes']:g}"
            for g in s["where"]]
    return out + [f"  part {p} past the demo end has no content element (row PC3)" for p in s["empty_parts"]]


def write_json(path, data):
    pathlib.Path(path).write_text(json.dumps(data, indent=1) + "\n", encoding="utf-8")


def run_need(a):
    pd.check(a.model and not a.content, "--model needs --content (the content file of the built game)")
    r = result_of(a)
    pw_proof.print_lines(need_lines(r) + given_lines(r["given"]) + shortfall_lines(r["shortfall"]))
    if a.json:
        write_json(a.json, r)
    return 1 if r["shortfall"]["whole"] or r["shortfall"]["empty_parts"] else 0


# ----------------------------------------------------------------------------- the template
def spread(times, end, count):
    """Add times in the middle of the largest gap until there are count times."""
    for _ in range(count - len(times)):
        times = split_largest(times, end)
    return times


def split_largest(times, end):
    m = [0.0] + sorted(times) + [end]
    a, b = max(zip(m, m[1:]), key=lambda p: p[1] - p[0])
    return sorted(times + [(a + b) / 2.0])


def frame(k, t):
    """1 empty element: the agent writes the name, the kind, the system card, the levels, the cost and the effects."""
    return dict(id=f"element_{k}", name="", pitch_words="", kind=None, system=None, currency=MAIN, cost=None,
                levels=None, effects=[], unlock={}, role=None, place="pitch", **{TIME_KEY: round(t, 2)})


def core_frame(e):
    """1 element of the selection: it keeps its system card and opens at the start."""
    return dict(frame(0, 0.0), id=e["id"], kind="line", system=e["system"], pitch_words=e["text"])


def template_of(a):
    lim = limits(pd.read_json(a.targets, "targets"))
    need = places(lim["end"], lim["content_h1"], lim["content"])
    count = len(need) if a.count is None else a.count
    pd.check(count < len(need), f"--count {count} is below the need of {len(need)} elements (row CG): content is "
             f"missing: {len(need) - count} elements")
    core = selection_elements(pd.read_json(a.selection, "selection"), pd.Catalog()) if a.selection else []
    new = [frame(k + 1, t) for k, t in enumerate(spread(need, lim["end"], count))]
    return dict(schema_version=1, game="", source="pw_content.py template", currencies=dict(main=MAIN, reset=None,
                side=[]), stats={MAIN: MAIN}, elements=[core_frame(e) for e in core if e["id"] != "reset"] + new)


def run_template(a):
    data = template_of(a)
    if a.out:
        write_json(a.out, data)
    text = f"pw_content: {len(data['elements'])} empty frames in {a.out}: name each one (references/content-step.md)"
    pw_proof.print_lines([text] if a.out else [json.dumps(data, indent=1)])
    return 0


# ----------------------------------------------------------------------------- main
def parse(argv):
    ap = argparse.ArgumentParser(description="The content step: the count of content elements that the targets "
                                             "need (rows CG, PC3 and PC4), and the frame of a content file.",
                                 epilog=USAGE, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="command", required=True)
    for name, args in ARGS.items():
        p = sub.add_parser(name)
        p.add_argument("targets", metavar="targets.json")
        [p.add_argument(*names, **kw) for names, kw in args]
    return ap.parse_args(argv)


def main(argv=None):
    a = parse(argv)
    run = dict(need=run_need, template=run_template)[a.command]
    try:
        return run(a)
    except pd.InputError as e:
        sys.stderr.write(f"pw_content: {e}\n")
        return 2


if __name__ == "__main__":
    sys.exit(main())
