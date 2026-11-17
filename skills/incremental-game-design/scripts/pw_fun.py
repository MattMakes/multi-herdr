#!/usr/bin/env python3
"""The fun sheet: the tier 1 fun metrics FM-1 to FM-12 of a model (references/fun-analysis.md).

The sheet plays the model with active_greedy of pw_players.py, the run of the proof sheet, and computes each row
from the event list of that run and from the model. Each row has a band with its mark, a status, a reason that
names a place in the game, and 1 change to try. The result is advice with evidence marks, not a proof. It never
changes the gate of the proof sheet.

Usage: pw_fun.py <model.json> [--build <model-built.json>] [--md FILE] [--json FILE]
"""
import argparse
import bisect
import collections
import functools
import json
import math
import os
import pathlib
import statistics
import sys

import pw_players
from pw_model import load_model
from pw_sim import Sim

SCHEMA_VERSION = 1
HOUR = 3600.0
MINUTE = 60.0
EPS = 1e-6
WINDOW = 600.0                 # PR-4: windows of 10 minutes
JOLT = 1.5                     # FM-4: a jump of x1.5 of the income of c*
JOLT_SPAN = 60.0               # FM-4: within 60 s, and 60 s between 2 jolts
SHIFT_START = 60.0             # FM-11: a system with an event in the first 60 s is not a shift
SHIFT_MERGE = 30.0             # FM-11: shifts within 30 s are 1 shift
STACK = 30.0                   # PR-12: unlocks within 30 s are stacked
SLOPE_START = 300.0            # PR-8: the gaps that start after 300 s
SLOPE_POINTS = 4               # PR-8: no slope for fewer than 4 points
VERB_HOUR = 3                  # FM-7: the hours after hour 3
VERB_FLOOR = 3                 # FM-7 guard: a system needs 3 presses in the hour to count as a verb
VARIETY_PRESSES = 20           # FM-9: the hours with 20 or more presses
BEAT_WEIGHT = 2                # FM-10: a beat has a weight of 2 or more
NOVEL_WEIGHT = 3               # FM-12: a reset of weight 3 or more follows a run with a new beat
LIKELY_DULL = 2                # the result rule: 2 or more counting rows out (no source)
ENOUGH_ROWS = 6                # the result rule: 6 or more counting rows ok for "no finding"
GAIN_KINDS = ("add", "pct", "mult")
BASE = pw_players.BASE
ADVICE = "advice with evidence marks, not a proof"
NO_PATTERN = ("The model has no game.play_pattern. The sheet plays it with active_greedy and uses the bands of the "
              "continuous pattern.")
LIMITS = (
    "Each row is a proxy. A sheet with no finding does not prove that a game is fun.",
    "The result is advice with evidence marks, not a proof. It never changes the gate of the proof sheet.",
    "Most bands are \"no source\". The sheet shows the mark of each band beside the value.",
    "The simulated player is perfect and has no boredom. Only a play test tells whether a player wants a reward, "
    "whether a surprise surprises, whether the theme carries the mechanics, and why a player stops.",
)
PLAY_TEST = (
    "FM-21 Felt pace probe: each 10 minutes, ask \"too slow, right, too fast\".",
    "FM-22 Goal comprehension: at minutes 5, 20 and 60, ask \"What are you working toward now, and what does it "
    "cost?\" with the screen hidden.",
    "FM-23 Sitting length and stop point: record the first wish to stop, the stop, and its reason.",
    "FM-24 Need satisfaction and meaning: the PENS or PXI items after the sitting, and the plans that the player "
    "states aloud.",
)
CONTROLS = {"C1": "opening pace", "C2": "first price", "C3": "unlock spacing", "C4": "stage wall",
            "C5": "game length", "C6": "upgrade punch", "C7": "growth pressure", "C8": "milestone pay",
            "C9": "run length", "C10": "reset weight", "C11": "first reset", "C12": "tend boost", "C13": "bank",
            "C14": "auto-buy start", "C15": "garden rhythm", "C16": "feedback"}


def ctl(cid):
    """The control id with its name, for a change text: 'C3 (unlock spacing)'."""
    return f"{cid} ({CONTROLS[cid]})"


def mins(t):
    return t / MINUTE


def r4(x):
    return None if x is None else round(float(x), 4)


def rounded(x):
    """x with each float rounded to 4 decimals, so 2 runs write the same bytes."""
    if isinstance(x, float):
        return round(x, 4)
    if isinstance(x, dict):
        return {k: rounded(v) for k, v in x.items()}
    if isinstance(x, (list, tuple)):
        return [rounded(v) for v in x]
    return x


def gaps_of(times, lo, hi):
    """[(gap seconds, start, end)] of the distinct times in (lo, hi), with lo and hi added."""
    marks = [lo] + sorted({t for t in times if lo + EPS < t < hi - EPS}) + [hi]
    return [(b - a, a, b) for a, b in zip(marks, marks[1:])]


def gap_size(g):
    return g[0]


def largest(times, lo, hi):
    gaps = gaps_of(times, lo, hi)
    return max(gaps, key=gap_size)


def span_text(g):
    return f"minute {mins(g[1]):.0f} to minute {mins(g[2]):.0f}"


def median(xs):
    return statistics.median(xs) if xs else None


def share(xs, test):
    return sum(1 for x in xs if test(x)) / len(xs) if xs else None


def names(ids, n=4):
    ids = list(ids)
    return ", ".join(ids[:n]) + (f" and {len(ids) - n} more" if len(ids) > n else "")


# ----------------------------------------------------------------------------- the run
def step_real(sim, ax, step):
    hit = sim.step_clock.get(ax, {}).get(int(step)) if ax else None
    return None if hit is None else hit[0]


def mark_time(sim, ax, mark):
    if "axis_step" in mark:
        return step_real(sim, ax, mark["axis_step"])
    if "purchase" not in mark:
        return sim.end_real
    return next((e["real"] for e in sim.events if e["kind"] == "purchase" and e["id"] == mark.get("purchase")), None)


def length(m, sim, ax):
    """(E in real seconds, its source): the hard end, else the axis end, else the last event."""
    mark = (m["game"].get("targets") or {}).get("hard_end")
    hard = mark_time(sim, ax, mark) if mark else None
    if hard is not None:
        return hard, "hard"
    end = step_real(sim, ax, sim.lmap[ax]["count"]) if ax else None
    if end is not None:
        return end, "axis"
    return (sim.events[-1]["real"] if sim.events else 0.0), "last"


def economy_currency(m):
    """c*: the currency that the most add, pct and mult effects of the purchases target; first on a tie."""
    cids = [c["id"] for c in m["currencies"]]
    hits = [e["target"] for p in m["purchases"] for e in p["effects"] if e["kind"] in GAIN_KINDS and
            e["target"] in cids]
    return max(dict.fromkeys(hits), key=hits.count) if hits else cids[0]


def line_target(p, cstar, cids):
    """The currency of the first add, pct or mult effect of the line ('all' is c*), or None."""
    e = next((e for e in p["effects"] if e["kind"] in GAIN_KINDS), None)
    t = None if e is None else (cstar if e["target"] == "all" else e["target"])
    return t if t in cids else None


def start_income(m):
    sim = Sim(m, 1.0, {})
    sim.check_milestones()
    return dict(sim.income())


def is_hand(e):
    return e["kind"] == "purchase" and not e.get("auto")


def is_content(e, first_reset):
    """A content element: a first purchase, a first milestone, or the first reset. An axis step is not one."""
    first_buy = e["kind"] == "purchase" and e["note"] == "first"
    return first_buy or (e["kind"] == "milestone" and e["note"] != "repeat") or e is first_reset


def is_unlock(e):
    return e["kind"] == "ladder" and e["note"] != "repeat"


def event_level(e):
    """The level of the line after a purchase event: the simulator names it '<name> L<level>'."""
    tail = e["name"].rsplit(" L", 1)[-1]
    return int(tail) if tail.isdigit() else None


def block_gains(ev, pairs, steps):
    """{index of the event: block gain} for each purchase that reaches a count step of its line (count_steps).
    The block gain is the income of the line target after the step against the income before the first level of
    the block (references/fun-analysis.md, "Count steps")."""
    start, out = {}, {}
    for i, (e, before) in enumerate(pairs):
        cs = steps.get(e["id"]) if e["kind"] == "purchase" else None
        block_step(start, out, i, (e["id"], before, cs and event_level(e), cs and cs["every"]))
    return out


def block_step(start, out, i, item):
    """Open the block of the line at its first level; close it at the count step (block_gains)."""
    line, before, level, n = item
    if not level:
        return
    start.update({line: before} if (level - 1) % n == 0 else {})
    out.update({i: start.pop(line)} if level % n == 0 and line in start else {})


class Play:
    """The active run of 1 model and the event lists that the rows read."""

    def __init__(self, m):
        self.m = m
        self.ax = pw_players.axis_of(m)
        players = pw_players.Players(m, m["game"]["target_hours"])
        self.horizon = players.horizon({})
        self.sim = players.run_all([BASE])[BASE][1]
        self.E, self.e_src = length(m, self.sim, self.ax)
        self.cstar = economy_currency(m)
        self.build_factor = None
        self._lists()

    def _lists(self):
        ev = self.events = [e for e in self.sim.events if e["real"] <= self.E + EPS]
        self.pairs = list(zip(ev, [start_income(self.m)] + [e["income"] for e in ev]))
        self.times = [e["real"] for e in ev]
        self._purchases(ev)
        self._content(ev)

    def _purchases(self, ev):
        cids = {c["id"] for c in self.m["currencies"]}
        self.targets = {p["id"]: line_target(p, self.cstar, cids) for p in self.m["purchases"]}
        self.hand = [e for e in ev if is_hand(e)]
        self.presses = sorted({(e["real"], e["id"]) for e in self.hand})
        self.steps = {p["id"]: p["count_steps"] for p in self.m["purchases"] if p.get("count_steps")}
        self.block_start = block_gains(ev, self.pairs, self.steps)

    def block_gain(self, i, c):
        """The block gain on the currency c of the step event i, or None (no step, or no income before)."""
        before = self.block_start.get(i)
        e = self.events[i]
        return e["income"].get(c, 0.0) / before[c] if before and before.get(c, 0.0) > 0 else None

    def _content(self, ev):
        self.resets = [e["real"] for e in ev if e["kind"] == "prestige"]
        first_reset = next((e for e in ev if e["kind"] == "prestige"), None)
        self.content = [(e["real"], e["id"]) for e in ev if is_content(e, first_reset)]
        self.unlocks = sorted({e["real"] for e in ev if e["real"] > 0 and (is_unlock(e) or is_content(e, None))})


def result(value, unit, text, status, reason, change, sub=None, where=None):
    return dict(value=r4(value), unit=unit, value_text=text, sub=sub or {}, status=status, reason=reason,
                change=change, where=where or {})


def not_applicable(why):
    return result(None, "", "not applicable", "not applicable", why, "none: the row does not apply")


# ----------------------------------------------------------------------------- FM-1 content drought
def empty_windows(times, end):
    n = max(1, math.ceil(end / WINDOW - EPS))
    full = {min(int(t // WINDOW), n - 1) for t in times if 0 < t <= end + EPS}
    return n - len(full), n


def gap_cv(times, end):
    gaps = [g for g, _, _ in gaps_of(times, 0.0, end) if g > 0]
    return statistics.pstdev(gaps) / statistics.mean(gaps) if len(gaps) > 1 else None


def stacked(times):
    ts = sorted(set(times))
    return sum(1 for a, b in zip(ts, ts[1:]) if b - a <= STACK + EPS) / len(ts) if ts else None


def ls_slope(pts):
    mx, my = statistics.mean(x for x, _ in pts), statistics.mean(y for _, y in pts)
    sxx = sum((x - mx) ** 2 for x, _ in pts)
    return sum((x - mx) * (y - my) for x, y in pts) / sxx if sxx > 0 else None


def gap_slope(times, end):
    pts = [(math.log((a + b) / 2), math.log(g)) for g, a, b in gaps_of(times, 0.0, end) if a > SLOPE_START and g > 1]
    return ls_slope(pts) if len(pts) >= SLOPE_POINTS else None


def drought_sub(p, h1, whole):
    empty, n = empty_windows(p.unlocks, p.E)
    return dict(drought_hour1_min=r4(mins(h1[0])), drought_min=r4(mins(whole[0])), empty_windows=empty,
                windows=n, unlock_gap_cv=r4(gap_cv(p.unlocks, p.E)), stacked_share=r4(stacked(p.unlocks)),
                gap_slope=r4(gap_slope(p.unlocks, p.E)))


def fm1(p, band):
    times = [t for t, _ in p.content]
    h1, whole = largest(times, 0.0, min(HOUR, p.E)), largest(times, 0.0, p.E)
    bad = [(g, lim) for g, lim in ((whole, band["max"]), (h1, band["max_h1"])) if mins(g[0]) > lim + EPS]
    g, lim = bad[0] if bad else (whole, band["max"])
    text = f"hour 1 {mins(h1[0]):.1f} min; whole {mins(whole[0]):.1f} min"
    where = dict(start_minutes=r4(mins(g[1])), end_minutes=r4(mins(g[2])), hour1_start_minutes=r4(mins(h1[1])),
                 hour1_end_minutes=r4(mins(h1[2])))
    return result(mins(whole[0]), "min", text, "out" if bad else "ok", drought_reason(g, lim, bad),
                  drought_change(g, bad), drought_sub(p, h1, whole), where)


def drought_reason(g, lim, bad):
    if bad:
        return f"No new content from {span_text(g)} ({mins(g[0]):.1f} min; band {lim:g} min)."
    return f"The largest gap with no new content is {mins(g[0]):.1f} min, from {span_text(g)} (band {lim:g} min)."


def drought_change(g, bad):
    if not bad:
        return f"None needed. {ctl('C3')} and {ctl('C5')} move this row."
    return (f"Move 1 milestone or 1 line into {span_text(g)} (content), or lower {ctl('C3')}.")


# ----------------------------------------------------------------------------- FM-2 felt purchase share
def felt_gain(p, i, c):
    """The felt gain of the hand purchase i on its target c: the block gain at a count step, None at another level
    of a line with count steps, else the income after against the income before."""
    e, b = p.pairs[i]
    if e["id"] in p.steps:
        return p.block_gain(i, c)
    return e["income"][c] / b[c] if b.get(c, 0.0) > 0 else None


def hand_gains(p):
    """[(line id, gain, real seconds)] of each hand purchase with a felt gain (a line with count steps: its steps)."""
    rows = [(i, e, p.targets.get(e["id"])) for i, (e, _) in enumerate(p.pairs) if is_hand(e)]
    gains = [(e["id"], felt_gain(p, i, c) if c else None, e["real"]) for i, e, c in rows]
    return [x for x in gains if x[1] is not None]


def first_value(p, kind):
    return next((e["value"] for e in p["effects"] if e["kind"] == kind), None)


def small_line(p, step):
    mult, pct = first_value(p, "mult"), first_value(p, "pct")
    return (mult is not None and mult < 1 + step - EPS) or (pct is not None and pct < step - EPS)


def below(limit, x):
    return x[1] < limit - EPS


def share_below(gains, limit):
    return share(gains, functools.partial(below, limit))


def fm2(p, band):
    gains = hand_gains(p)
    small = [x["id"] for x in p.m["purchases"] if small_line(x, band["step"])]
    status = felt_status(share_below(gains, 1 + band["step"]), len(small), band)
    if status == "not applicable":
        return not_applicable("no hand purchase raises the income of a currency")
    return felt_row(p, band, gains, small, status)


def felt_sub(p, gains, small):
    return dict(hand_purchases=len(gains), share_below_10=r4(share_below(gains, 1.1)),
                median_gain=r4(median([g - 1 for _, g, _ in gains])), model_part=len(small), lines=len(p.m["purchases"]),
                count_steps=sum(1 for i, _, _ in gains if i in p.steps),
                block_levels=sum(1 for e in p.hand if e["id"] in p.steps) - sum(1 for i, _, _ in gains if i in p.steps))


def felt_text(value, step, n_small, lines):
    head = "no hand purchase" if value is None else f"{value:.2f} below +{step * 100:g} %"
    return f"{head}; model part {n_small}/{lines}"


def felt_row(p, band, gains, small, status):
    low = collections.Counter(i for i, g, _ in gains if g < 1 + band["step"] - EPS)
    value = share_below(gains, 1 + band["step"])
    return result(value, "share", felt_text(value, band["step"], len(small), len(p.m["purchases"])), status,
                  felt_reason(value, gains, low, small, band) + step_note(p, gains),
                  felt_change(status, low, small, band["step"]),
                  felt_sub(p, gains, small), dict(low_lines=[i for i, _ in low.most_common(5)], model_part_lines=small))


def step_note(p, gains):
    """The sentence on the lines with count steps, or ''."""
    if not p.steps:
        return ""
    steps = sum(1 for i, _, _ in gains if i in p.steps)
    return (f" The lines with count steps ({names(p.steps)}) count their {steps} steps with the gain of the block; "
            "their other levels are left out.")


def felt_status(value, n_small, band):
    if n_small > band["model_max"]:
        return "out"
    if value is None:
        return "not applicable"
    return "out" if value > band["max"] + EPS else "ok"


def felt_reason(value, gains, low, small, band):
    step = band["step"] * 100
    text = "" if value is None else (f"{value:.2f} of {len(gains)} hand purchases raise their income by less than "
                                     f"+{step:g} % (band {band['max']:g})")
    text += f"; the most are {names(i for i, _ in low.most_common(3))}." if low else smallest_gain(gains)
    if small:
        text += f" {len(small)} lines have a first step below +{step:g} %: {names(small)}."
    return text.strip()


def smallest_gain(gains):
    if not gains:
        return ""
    i, g, t = min(gains, key=gap_of_gain)
    return f"; the smallest gain is +{(g - 1) * 100:.1f} % ({i}, minute {mins(t):.0f})."


def gap_of_gain(x):
    return x[1]


def felt_change(status, low, small, step):
    top = names(i for i, _ in low.most_common(3))
    texts = (f"Raise the first step of {names(small)} to x{1 + step:g} or more with {ctl('C6')}, or tag them as 1 "
             "list family.", f"Raise the step d of {top} with {ctl('C6')}, or lower {ctl('C7')}.",
             f"None needed. {ctl('C6')} and {ctl('C7')} move this row.")
    return texts[0 if small else 1 if status == "out" else 2]


# ----------------------------------------------------------------------------- FM-3 line character
def cost_key(p):
    return (p["cost_currency"], p["cost_formula"], f"{float(p['base_cost']):.2g}", round(float(p["cost_growth"]), 3))


def effect_key(p):
    cs = p.get("count_steps")
    steps = (("count_steps", int(cs["every"]), round(float(cs["mult"]), 4)),) if cs else ()
    return tuple(sorted((e["kind"], str(e["target"]), round(float(e["value"]), 4)) for e in p["effects"])) + steps


def root(parent, x):
    while parent[x] != x:
        x = parent[x]
    return x


def join_by(lines, key, parent):
    first = {}
    for p in lines:
        parent[root(parent, p["id"])] = root(parent, first.setdefault(key(p), p["id"]))


def clone_groups(lines):
    """The groups of 2 or more lines that are cost clones or effect clones of each other."""
    parent = {p["id"]: p["id"] for p in lines}
    for key in (cost_key, effect_key):
        join_by(lines, key, parent)
    groups = collections.defaultdict(list)
    for p in lines:
        groups[root(parent, p["id"])].append(p["id"])
    return sorted(g for g in groups.values() if len(g) > 1)


def level_factor(p):
    """The first mult value, else 1 + the first pct, else the step factor of the count steps, else None."""
    e = next((e for e in p["effects"] if e["kind"] in ("mult", "pct")), None)
    if e is None:
        return float(p["count_steps"]["mult"]) if p.get("count_steps") else None
    return e["value"] if e["kind"] == "mult" else 1 + e["value"]


def character(m):
    """The distinct share, the clone groups, the median level factor and the size spread of the purchase lines.
    A line with the key family leaves the distinct share."""
    lines = [p for p in m["purchases"] if not p.get("family")]
    groups = clone_groups(lines)
    factors = [f for f in (level_factor(p) for p in m["purchases"]) if f is not None]
    steps = [math.log10(f - 1) for f in factors if f > 1 + EPS]
    return dict(lines=len(lines), ids=[p["id"] for p in lines], groups=groups, median_factor=median(factors),
                distinct=distinct_share(groups, len(lines)), spread=statistics.pstdev(steps) if len(steps) > 1 else None)


def distinct_share(groups, n):
    return 1 - sum(len(g) for g in groups) / n if n else None


def drift(new, built):
    ok = new and built and new > 1 + EPS and built > 1 + EPS
    return math.log(new) / math.log(built) if ok else None


def fm3(p, band):
    ch = character(p.m)
    if ch["distinct"] is None:
        return not_applicable("each purchase line has the key family")
    d = drift(ch["median_factor"], p.build_factor)
    bad = [ch["distinct"] < band["min"] - EPS, d is not None and d < band["min_drift"] - EPS]
    sub = dict(distinct_share=r4(ch["distinct"]), median_factor=r4(ch["median_factor"]), drift=r4(d),
               drift_note="not computed: no --build" if p.build_factor is None else "", size_spread=r4(ch["spread"]),
               lines=ch["lines"])
    text = f"distinct share {ch['distinct']:.2f}" + ("" if d is None else f"; drift {d:.2f}")
    return result(ch["distinct"], "share", text, "out" if any(bad) else "ok", character_reason(ch, d, bad, band),
                  character_change(ch, bad), sub, dict(clone_groups=ch["groups"]))


def character_reason(ch, d, bad, band):
    big = max(ch["groups"], key=len) if ch["groups"] else []
    text = (f"{len(big)} lines are clones in cost or effect: {names(big)} (distinct share {ch['distinct']:.2f}; band "
            f"{band['min']:g})." if big else f"No 2 of the lines {names(ch['ids'])} are clones (distinct share {ch['distinct']:.2f}).")
    if bad[1]:
        text += f" The median level factor fell to {ch['median_factor']:.4g} (drift {d:.2f}; band {band['min_drift']:g})."
    return text


def character_change(ch, bad):
    if not any(bad):
        return "None needed. No control moves this row (content: keep the step, cost growth and cap of each line apart)."
    if bad[1] and not bad[0]:
        return "Keep the level factor of the build (content: the 'keep the character' rule); no control moves this row."
    big = max(ch["groups"], key=len)
    return (f"Give {names(big)} each its own step, cost growth or cap (content), or tag them as 1 list family; no "
            "control moves this row.")


# ----------------------------------------------------------------------------- FM-4 largest gap between jolts
def in_spans(spans, t):
    return any(a < t <= b for a, b in spans)


def quiet_after_reset(p):
    """The test 'in the 60 s after a reset' when the game resets more than 1 time each 10 minutes, else never."""
    often = len(p.resets) > p.E / WINDOW
    spans = [(r, r + JOLT_SPAN) for r in p.resets] if often else []
    return functools.partial(in_spans, spans)


def is_jolt(p, inc, i):
    if i in p.block_start:
        return True
    j = bisect.bisect_left(p.times, p.times[i] - JOLT_SPAN - EPS)
    before = inc[j]
    return inc[i] >= JOLT * before - EPS if before > 0 else inc[i] > 0


def spaced(times):
    """The times with each time that comes within 60 s of the kept time before it left out."""
    out = []
    for t in times:
        out += [t] if not out or t - out[-1] >= JOLT_SPAN - EPS else []
    return out


def jolt_times(p):
    inc = [e["income"].get(p.cstar, 0.0) for e in p.events]
    quiet = quiet_after_reset(p)
    return spaced([t for i, t in enumerate(p.times) if not quiet(t) and is_jolt(p, inc, i)])


def fm4(p, band):
    jolts = jolt_times(p)
    g = largest(jolts, 0.0, p.E)
    out = mins(g[0]) > band["max"] + EPS
    sub = dict(jolts=len(jolts), jolts_per_hour=r4(len(jolts) / (p.E / HOUR)), currency=p.cstar,
               count_steps=len(p.block_start))
    reason = (f"No jump of x{JOLT:g} in the {p.cstar} income from {span_text(g)} ({mins(g[0]):.1f} min; band "
              f"{band['max']:g} min).")
    change = (f"Raise {ctl('C6')} or {ctl('C8')} so that a purchase or a milestone in {span_text(g)} gives x{JOLT:g}."
              if out else f"None needed. {ctl('C6')}, {ctl('C8')} and {ctl('C7')} move this row.")
    where = dict(start_minutes=r4(mins(g[1])), end_minutes=r4(mins(g[2])))
    return result(mins(g[0]), "min", f"{mins(g[0]):.1f} min", "out" if out else "ok", reason, change, sub, where)


# ----------------------------------------------------------------------------- FM-5 end arc
def end_ratio(times, end):
    late = len({t for t in times if t >= 0.9 * end - EPS})
    alls = len(set(times))
    return (late / (0.1 * end)) / (alls / end) if alls else None


def fm5(p, band):
    times = [t for t, _ in p.content]
    last = max(p.content, default=(0.0, "none"))
    tail = (p.E - last[0]) / p.E
    shifts = shift_times(p)
    sub = dict(end_ratio=r4(end_ratio(times, p.E)), late_share=r4(share(times, lambda t: t > p.E / 2)),
               last_shift_share=r4(max(shifts, default=0.0) / p.E))
    out = tail > band["max"] + EPS
    reason = (f"The last new content ({last[1]}) comes at minute {mins(last[0]):.0f} of {mins(p.E):.0f} (tail "
              f"{tail:.2f}; band {band['max']:g}).")
    change = (f"Add content after minute {mins(0.9 * p.E):.0f}, or move the last unlocks later with {ctl('C3')} above "
              f"1, or end the game nearer minute {mins(last[0]):.0f} with {ctl('C5')}." if out else
              f"None needed. {ctl('C5')} and {ctl('C3')} move this row.")
    return result(tail, "share", f"tail {tail:.2f}", "out" if out else "ok", reason, change, sub,
                  dict(last_content=last[1], last_minutes=r4(mins(last[0]))))


# ----------------------------------------------------------------------------- FM-6 chore and automation
def auto_cutoffs(p):
    """{line: real seconds when the player stops pressing it}: from_minutes of its automation entry, else its first
    auto purchase, else E."""
    rows = [a for a in p.m.get("automation") or [] if a.get("line") in p.sim.pmap]
    cut = {a["line"]: MINUTE * float(a["from_minutes"]) for a in rows if "from_minutes" in a and not a.get("requires")}
    firsts = {}
    for e in p.events:
        if e["kind"] == "purchase" and e.get("auto"):
            firsts.setdefault(e["id"], e["real"])
    for a in rows:
        cut.setdefault(a["line"], firsts.get(a["line"], p.E))
    return cut


def chore_counts(p, cut):
    return collections.Counter(i for t, i in p.presses if t < cut.get(i, p.E) + EPS)


def hand_share_h1(p):
    buys = [e for e in p.events if e["kind"] == "purchase" and e["real"] <= min(HOUR, p.E) + EPS]
    return share(buys, is_hand)


def press_time(p, line, n):
    times = [t for t, i in p.presses if i == line]
    return times[n - 1] if len(times) >= n else None


def fm6(p, band):
    cut = auto_cutoffs(p)
    counts = chore_counts(p, cut)
    line, top = counts.most_common(1)[0] if counts else ("none", 0)
    above = sorted(i for i, n in counts.items() if n > band["max_presses"] and i not in cut)
    hs = hand_share_h1(p)
    bad = [top > band["max_presses"], hs is not None and hs < band["min_hand_share_h1"] - EPS]
    where = dict(line=line, lines_above_no_auto=above, presses=dict(sorted(counts.items())))
    return chore_row(p, band, (line, top, hs, above), bad, where)


def chore_row(p, band, found, bad, where):
    line, top, hs, above = found
    sub = dict(largest_presses=top, line=line, lines_above_no_auto=len(above), hand_share_hour1=r4(hs))
    text = f"{top} presses on {line}; hand share hour 1 " + ("none" if hs is None else f"{hs:.2f}")
    return result(top, "presses", text, "out" if any(bad) else "ok", chore_reason(line, top, hs, bad, band),
                  chore_change(p, line, bad, band), sub, where)


def chore_reason(line, top, hs, bad, band):
    hs_text = "no purchase in hour 1" if hs is None else f"hand share in hour 1 {hs:.2f} (band {band['min_hand_share_h1']:g})"
    first = (f"{line} gets {top} hand presses before its auto-buy (band {band['max_presses']:g})" if bad[0] else
             f"The largest chore is {top} presses on {line} (band {band['max_presses']:g})")
    return f"{first}; {hs_text}."


def chore_change(p, line, bad, band):
    at = press_time(p, line, int(band["max_presses"]) + 1)
    when = "" if at is None else f" before minute {mins(at):.0f}"
    texts = (f"Add an automation entry for {line}{when} with {ctl('C14')}.",
             f"Start the auto-buy later with {ctl('C14')}, so that the player buys by hand in hour 1.",
             f"None needed. {ctl('C14')} moves this row.")
    return texts[0 if bad[0] else 1 if bad[1] else 2]


# ----------------------------------------------------------------------------- FM-7 verbs for each hour
def has_active(m):
    owners = m["purchases"] + (m.get("milestones") or []) + m["ladders"]
    return any(e.get("active") for x in owners for e in x.get("effects") or [])


def verbs_in(p, h, floor):
    lo, hi = h * HOUR, (h + 1) * HOUR
    presses = collections.Counter(p.sim.pmap[i]["system"] for t, i in p.presses if lo <= t < hi)
    reset = any(lo <= t < hi for t in p.resets)
    return sum(1 for n in presses.values() if n >= floor) + int(reset)


def hour_verbs(p, floor):
    active = int(has_active(p.m))
    return [verbs_in(p, h, floor) + active for h in range(math.ceil(p.E / HOUR - EPS))]


def fm7(p, band):
    verbs, raw = hour_verbs(p, VERB_FLOOR), hour_verbs(p, 1)
    late = verbs[VERB_HOUR:]
    if not late:
        return not_applicable(f"the game is not longer than {VERB_HOUR} h (E {mins(p.E):.0f} min)")
    low, worst = min(late), VERB_HOUR + late.index(min(late))
    out = low < band["min"] - EPS
    sub = dict(minimum=low, median=r4(median(late)), by_hour=verbs, by_hour_no_floor=raw)
    reason = (f"Hour {worst + 1} (minute {worst * 60} to {worst * 60 + 60}) has {low} verbs (band {band['min']:g}; a "
              f"system needs {VERB_FLOOR} presses in the hour).")
    change = (f"Add a verb in hour {worst + 1}: a new system, a reset or an active action (content), or move the "
              f"auto-buy with {ctl('C14')}, {ctl('C15')} or {ctl('C9')}." if out else
              f"None needed. {ctl('C14')}, {ctl('C15')} and {ctl('C9')} move this row.")
    return result(low, "verbs", f"minimum {low}; median {median(late):g}", "out" if out else "ok", reason, change,
                  sub, dict(hour=worst + 1))


# ----------------------------------------------------------------------------- FM-8 comeback ratio
def run_of(p, a, b):
    """(start, end, [axis step times]) of the run between the events a and b (-1 and len for the ends)."""
    start = 0.0 if a < 0 else p.events[a]["real"]
    end = p.E if b >= len(p.events) else p.events[b]["real"]
    return start, end, [e["real"] for e in p.events[a + 1:b] if e["kind"] == "ladder" and e["id"] == p.ax]


def split_runs(p):
    """[(start, end, [axis step times])] of each run of the active player up to E."""
    cuts = [i for i, e in enumerate(p.events) if e["kind"] == "prestige"]
    return [run_of(p, a, b) for a, b in zip([-1] + cuts, cuts + [len(p.events)])]


def comeback(old, new):
    """The ratio of 1 pair of runs, None when the new run does not pass the old peak, or 'skip' for an old run with
    no axis step."""
    peak = len(old[2])
    if peak == 0:
        return "skip"
    if len(new[2]) < peak:
        return None
    return (new[2][peak - 1] - new[0]) / max(old[1] - old[0], EPS)


def fm8(p, band):
    if not p.resets or not p.ax:
        return not_applicable("no reset in the run" if not p.resets else "no progress axis")
    runs = split_runs(p)
    pairs = [x for x in (comeback(a, b) for a, b in zip(runs, runs[1:])) if x != "skip"]
    passed = [x for x in pairs if x is not None]
    return comeback_row(p, band, passed, pairs)


def comeback_row(p, band, passed, pairs):
    med = median(passed)
    out = med is None or not band["min"] - EPS <= med <= band["max"] + EPS
    sub = dict(median=r4(med), passes=len(passed), pairs=len(pairs))
    text = ("no run passes" if med is None else f"median {med:.2f}") + f"; {len(passed)} of {len(pairs)} pass"
    reason = comeback_reason(med, passed, pairs, band) + f" The first reset is at minute {mins(p.resets[0]):.0f}."
    return result(med, "ratio", text, "out" if out else "ok", reason,
                  comeback_change(med, band), sub, dict(resets_minutes=[r4(mins(t)) for t in p.resets[:10]]))


def comeback_reason(med, passed, pairs, band):
    if med is None:
        return f"No new run passes the peak of the run before ({len(pairs)} runs after a reset)."
    return (f"A new run passes the peak of the run before in {med:.2f} of the length of that run (median of "
            f"{len(passed)} runs; band {band['min']:g} to {band['max']:g}).")


def comeback_change(med, band):
    if med is None or med > band["max"] + EPS:
        return f"Raise {ctl('C10')}, or make longer runs with {ctl('C9')}, so that a reset pays."
    if med < band["min"] - EPS:
        return f"Lower {ctl('C10')}, or move {ctl('C11')} later, so that a new run meets a wall."
    return f"None needed. {ctl('C9')}, {ctl('C10')} and {ctl('C11')} move this row."


# ----------------------------------------------------------------------------- FM-9 purchase variety
def press_weight(p, line):
    """1 for a press, 1 / n for a press of a line with count steps: the n presses of 1 block are 1 choice."""
    cs = p.steps.get(line)
    return 1.0 / cs["every"] if cs else 1.0


def hour_tops(p):
    """[(hour, top line, share)] of each hour with 20 or more presses. The share counts choices (press_weight)."""
    hours, raw = collections.defaultdict(collections.Counter), collections.Counter()
    for t, i in p.presses:
        hours[int(t // HOUR)][i] += press_weight(p, i)
        raw[int(t // HOUR)] += 1
    busy = [(h, c) for h, c in sorted(hours.items()) if raw[h] >= VARIETY_PRESSES]
    return [(h, c.most_common(1)[0][0], c.most_common(1)[0][1] / sum(c.values())) for h, c in busy]


def fm9(p, band):
    tops = hour_tops(p)
    if not tops:
        return not_applicable(f"no hour has {VARIETY_PRESSES} or more presses")
    h, line, top = max(tops, key=lambda x: x[2])
    out = top > band["max"] + EPS
    sub = dict(maximum=r4(top), median=r4(median([s for _, _, s in tops])), hours=len(tops))
    reason = f"In hour {h + 1}, {line} gets {top:.2f} of the presses (band {band['max']:g})."
    change = (f"Start the auto-buy of {line} sooner with {ctl('C14')}, or change prices with {ctl('C2')}." if out
              else f"None needed. {ctl('C14')} moves this row.")
    return result(top, "share", f"{top:.2f} ({line}, hour {h + 1})", "out" if out else "ok", reason, change, sub,
                  dict(hour=h + 1, line=line))


# ----------------------------------------------------------------------------- FM-10 felt beat share
def is_beat(e):
    return e["weight"] >= BEAT_WEIGHT and e["kind"] != "prestige" and not e.get("auto")


def beat_rise(p, i):
    """The rise of c* at the beat i: the block gain at a count step of a hand purchase, else after against before."""
    e, b = p.pairs[i]
    if i in p.block_start and is_hand(e):
        return p.block_gain(i, p.cstar)
    ok = is_beat(e) and b.get(p.cstar, 0.0) > 0
    return e["income"].get(p.cstar, 0.0) / b[p.cstar] if ok else None


def beat_rises(p):
    rises = [(e, beat_rise(p, i)) for i, (e, _) in enumerate(p.pairs)]
    return [(e, r) for e, r in rises if r is not None]


def beat_reason(p, felt, n, flat, band):
    kinds = ", ".join(f"{c} {k}" for k, c in flat.most_common()) or "none"
    return (f"{felt:.2f} of {n} beats raise the {p.cstar} income by +{band['step'] * 100:g} % or more (band "
            f"{band['min']:g}); the flat beats are {kinds}.")


def fm10(p, band):
    rises = beat_rises(p)
    if not rises:
        return not_applicable("no beat while the economy income is above 0")
    felt = 1 - share_below(rises, band["step"] + 1)
    flat = collections.Counter(e["kind"] for e, r in rises if r < band["step"] + 1 - EPS)
    out = felt < band["min"] - EPS
    sub = dict(beats=len(rises), median_rise=r4(median([r - 1 for _, r in rises])), currency=p.cstar)
    reason = beat_reason(p, felt, len(rises), flat, band)
    change = (f"Raise the reward of the milestones and stages with {ctl('C8')} or {ctl('C6')}." if out else
              f"None needed. {ctl('C8')} moves this row.")
    return result(felt, "share", f"{felt:.2f} ({len(rises)} beats)", "out" if out else "ok", reason, change, sub,
                  dict(flat_kinds=dict(flat)))


# ----------------------------------------------------------------------------- FM-11 shift stretch
def merge_times(times):
    out = []
    for t in sorted(times):
        out += [t] if not out or t - out[-1] > SHIFT_MERGE + EPS else []
    return out


def shift_times(p):
    """The shifts in (60 s, E]: the first event of a system with no event in the first 60 s, and the first reset."""
    early = {e["system"] for e in p.events if e["real"] <= SHIFT_START + EPS}
    firsts = {}
    for e in p.events:
        firsts.setdefault(e["system"], e["real"])
    times = [t for s, t in firsts.items() if s not in early and s != "-"] + p.resets[:1]
    return [t for t in merge_times(times) if t > SHIFT_START + EPS]


def fm11(p, band):
    shifts = shift_times(p)
    g = largest(shifts, 0.0, p.E)
    stretch = g[0] / p.E
    out = stretch > band["max"] + EPS
    sub = dict(shifts=len(shifts), last_shift_share=r4(max(shifts, default=0.0) / p.E))
    reason = f"No new system and no first reset from {span_text(g)} ({stretch:.2f} of E; band {band['max']:g})."
    change = (f"Add a new system or a reset mechanic in {span_text(g)} (content); no control moves this row." if out
              else "None needed. No control moves this row; only new content does.")
    return result(stretch, "share", f"{stretch:.2f} of E", "out" if out else "ok", reason, change, sub,
                  dict(start_minutes=r4(mins(g[1])), end_minutes=r4(mins(g[2]))))


# ----------------------------------------------------------------------------- FM-12 reset novelty share
def dull_at(resets):
    return next(e["real"] for e in resets if e["weight"] < NOVEL_WEIGHT)


def fm12(p, band):
    resets = [e for e in p.events if e["kind"] == "prestige"]
    if not resets:
        return not_applicable("no reset in the run")
    novel = share(resets, lambda e: e["weight"] >= NOVEL_WEIGHT)
    dull = [e["name"] for e in resets if e["weight"] < NOVEL_WEIGHT]
    out = novel < band["min"] - EPS
    reason = (f"{novel:.2f} of {len(resets)} resets follow a run with a new beat (band {band['min']:g})" +
              (f"; nothing new before {names(dull, 3)} (the first at minute {mins(dull_at(resets)):.0f})." if dull else
               f"; the first reset is at minute {mins(resets[0]['real']):.0f}."))
    change = (f"Make longer runs with {ctl('C9')}, or move {ctl('C11')} later, so that each run reaches a new beat."
              if out else f"None needed. {ctl('C9')} and {ctl('C11')} move this row.")
    return result(novel, "share", f"{novel:.2f} ({len(resets)} resets)", "out" if out else "ok", reason, change,
                  dict(resets=len(resets)), dict(dull_resets=dull[:10]))


# ----------------------------------------------------------------------------- the rows
SPEC_KEYS = ("id", "name", "kind", "fn", "band", "text", "mark", "controls", "evidence", "direction", "guards")


def spec(*values):
    """1 row of ROWS as a dict of SPEC_KEYS; a row with no guard gives 10 values."""
    return dict(zip(SPEC_KEYS, values + ((),) * (len(SPEC_KEYS) - len(values))))


ROWS = (
    spec("FM-1", "Content drought", "counts", fm1, dict(max_h1=10.0, max=20.0),
         "largest gap with no new content at most {max_h1:g} min in hour 1, {max:g} min later", "no source",
         ["C3", "C5", "C15", "C1", "C2"], ["MA S8 simulated agents predict quit points (sourced)",
                                          "Tower Wizard lost players at a wall, MA S11 (sourced)"], "less",
         ("FM-2", "FM-3")),
    spec("FM-2", "Felt purchase share", "counts", fm2, dict(max=0.2, step=0.05, model_max=0),
         "at most {max:g} of hand purchases below +5 % (step inferred from the Weber fraction); model part at most "
         "{model_max:g} lines", "no source", ["C6", "C7"],
         ["PR E2 perception threshold (measured; use inferred)", "MA S3 competence predicts enjoyment (measured)"],
         "less"),
    spec("FM-3", "Line character", "counts", fm3, dict(min=0.7, min_drift=0.5),
         "distinct share at least {min:g}; drift at least {min_drift:g}", "no source", [],
         ["DS E3 interesting decisions need a tradeoff (sourced)", "MA S1 coherent meaning of each variable (sourced)"],
         "more"),
    spec("FM-4", "Largest gap between jolts", "counts", fm4, dict(max=20.0), "at most {max:g} min", "no source",
         ["C6", "C8", "C7"], ["PR E3 income steps make purchase spikes (sourced)", "PR E4 the peak shapes memory "
                                                                                  "(measured)"], "less", ("FM-1",)),
    spec("FM-5", "End arc", "counts", fm5, dict(max=0.1), "tail after the last new content at most {max:g} of E",
         "no source", ["C5", "C3"], ["PR E4 peak-end rule (measured)", "MA S1 an end that lets you go (sourced)"],
         "less", ("FM-11",)),
    spec("FM-6", "Chore length and automation timing", "counts", fm6, dict(max_presses=150, min_hand_share_h1=0.3),
         "at most {max_presses:g} hand presses on 1 line before its auto-buy; hand share in hour 1 at least "
         "{min_hand_share_h1:g}", "no source", ["C14"],
         ["EF E2 auto-clickers are an error in design (sourced)", "EF E8 a late self-acting step made a chore "
                                                                  "(sourced)"], "band", ("FM-9",)),
    spec("FM-7", "Verbs for each hour", "counts", fm7, dict(min=3), "{min:g} or more verbs in each hour after hour 3",
         "no source", ["C14", "C15", "C9", "C12"], ["EF E1 idle games shift between clicking and waits (sourced)"],
         "more"),
    spec("FM-8", "Comeback ratio", "counts", fm8, dict(min=0.3, max=0.7), "median {min:g} to {max:g}", "no source",
         ["C9", "C10", "C11", "C4"], ["MA S1 designer quotes (sourced)", "MA S2 66 games (sourced)"], "band",
         ("FM-12",)),
    spec("FM-9", "Purchase variety", "advice", fm9, dict(max=0.6),
         "top-line share at most {max:g} in each hour with 20 or more presses", "no source", ["C14", "C2"],
         ["EF E3 criteria list from expert review (sourced)"], "less", ("FM-2",)),
    spec("FM-10", "Felt beat share", "advice", fm10, dict(min=0.3, step=0.05), "at least {min:g} of beats at +5 %",
         "no source", ["C8", "C6"], ["FC E1 action-outcome binding (measured)"], "more"),
    spec("FM-11", "Shift stretch", "advice", fm11, dict(max=0.35),
         "longest stretch with no new system at most {max:g} of E", "inferred", [],
         ["MA S1 designers build the arc from shifts (sourced); 3 acts of Universal Paperclips give 0.33 (inferred)"],
         "less", ("FM-3",)),
    spec("FM-12", "Reset novelty share", "advice", fm12, dict(min=0.8), "at least {min:g}", "no source", ["C9", "C11"],
         ["MA S1 (sourced)", "MA S2 (sourced)"], "more"),
)
DIRECTION = {s["id"]: s["direction"] for s in ROWS}


SPEC_OF = {s["id"]: s for s in ROWS}
FUN_META = ("band_mark",)      # a key of game.targets.fun that is not a band key: the sheet does not read it


def fun_targets(m):
    return (m["game"].get("targets") or {}).get("fun") or {}


def check_fun_targets(m):
    """Stop (SystemExit) on a row id or a band key of game.targets.fun that the sheet does not know."""
    return [check_band_keys(rid, own) for rid, own in fun_targets(m).items()]


def check_band_keys(rid, own):
    if rid not in SPEC_OF:
        raise SystemExit(f"game.targets.fun has the unknown row {rid}; valid rows: {', '.join(SPEC_OF)}")
    valid = list(SPEC_OF[rid]["band"])
    if not isinstance(own, dict):
        raise SystemExit(f"game.targets.fun {rid} is not an object of band keys; valid keys: {', '.join(valid)}")
    bad = [k for k in own if k not in valid and k not in FUN_META]
    if bad:
        raise SystemExit(f"game.targets.fun {rid} has the unknown band key {bad[0]}; valid keys: {', '.join(valid)}")


def band_of(m, s):
    """The band of a row and its mark: a band key of game.targets.fun overrides the default. The mark is
    'designer setting' only when such a key changed the band."""
    own = fun_targets(m).get(s["id"]) or {}
    band = dict(s["band"], **{k: v for k, v in own.items() if k in s["band"]})
    return band, ("designer setting" if band != s["band"] else s["mark"])


def make_row(p, s):
    band, mark = band_of(p.m, s)
    r = s["fn"](p, band) if p.E > EPS else not_computed("the active run has no event before the end")
    head = dict(id=s["id"], name=s["name"], tier=1, kind=s["kind"])
    tail = dict(band=s["text"].format(**band), band_values=band, band_mark=mark, controls=s["controls"],
                evidence=s["evidence"], guard={})
    return {**head, **r, **tail}


def not_computed(missing):
    r = result(None, "", "not computed", "not computed", missing, "none: give the missing input")
    r["missing"] = missing
    return r


def add_guards(rows):
    """Name each guard row in the sheet beside the row; a guard that is out adds 1 sentence to the reason."""
    status = {r["id"]: r["status"] for r in rows}
    for r, s in zip(rows, ROWS):
        guard_row(r, s["guards"], status)


def guard_row(r, guards, status):
    r["guard"] = {g: status[g] for g in guards}
    r["reason"] += "".join(f" Guard {g} is out: read this row with {g}." for g in guards if status[g] == "out")


def sheet_rows(p):
    rows = [make_row(p, s) for s in ROWS]
    add_guards(rows)
    return rows


def result_of(rows):
    """(result, ids of the counting rows that are out) by the rule of references/fun-analysis.md."""
    counting = [r for r in rows if r["kind"] == "counts"]
    out = [r["id"] for r in counting if r["status"] == "out"]
    ok = sum(1 for r in counting if r["status"] == "ok")
    if len(out) >= LIKELY_DULL:
        return "likely dull", out
    if out:
        return "watch", out
    return ("no finding" if ok >= ENOUGH_ROWS else "not enough data"), out


def worse(r, b):
    """True when the design row is out and the build row is not, or both are out and the design is further out."""
    if r["status"] != "out":
        return False
    if b["status"] != "out":
        return True
    d = DIRECTION[r["id"]]
    if d == "band" or r["value"] is None or b["value"] is None:
        return False
    return r["value"] > b["value"] + EPS if d == "less" else r["value"] < b["value"] - EPS


def compare(rows, brows):
    pairs = zip(rows, brows)
    for r, b in pairs:
        r.update(build_value=b["value"], build_value_text=b["value_text"], build_status=b["status"], worse=worse(r, b))


def notes_of(m, p):
    notes = [] if m["game"].get("play_pattern") else [NO_PATTERN]
    if p.e_src == "last":
        notes.append(f"The run does not reach the hard end or the axis end in {p.horizon:g} h, so E is the last "
                     "event.")
    return notes


def run_sheet(m, source):
    p = Play(m)
    return p, dict(schema_version=SCHEMA_VERSION, game=m["game"].get("name") or os.path.basename(source or ""),
                   source=source, build=None, scan=None, economy_currency=p.cstar,
                   length=dict(seconds=r4(p.E), minutes=r4(mins(p.E)), source=p.e_src),
                   runs=[dict(player=BASE, horizon_hours=p.horizon)], notes=notes_of(m, p))


def fun_sheet(m, source=None, build=None, build_path=None):
    """The fun sheet of the loaded model m as the dict of the JSON form. build: the loaded model of the game as
    built; the sheet then shows each build value and marks each row that the design made worse."""
    check_fun_targets(m)
    bsheet = fun_sheet(build, build_path) if build is not None else None
    p, sheet = run_sheet(m, source)
    p.build_factor = character(build)["median_factor"] if build is not None else None
    rows = sheet["rows"] = sheet_rows(p)
    sheet["result"], sheet["counting_out"] = result_of(rows)
    sheet["limits"] = list(LIMITS)
    if bsheet:
        compare(rows, bsheet["rows"])
        sheet.update(build=build_path, build_result=bsheet["result"], build_counting_out=bsheet["counting_out"])
    return rounded(sheet)


# ----------------------------------------------------------------------------- Markdown
def cell(x):
    return str(x).replace("|", "/").replace("\n", " ")


def row_line(r, built):
    cells = [r["id"], r["name"], r["value_text"], f"{r['band']} ({r['band_mark']})", r["status"], r["reason"],
             r["change"]]
    if built:
        cells += [r.get("build_value_text", ""), r.get("build_status", ""), "yes" if r.get("worse") else ""]
    return "| " + " | ".join(cell(c) for c in cells) + " |"


def table(rows, built):
    head = "| Id | Name | Value | Band (mark) | Status | Reason | Change to try |"
    head += " Built value | Built status | Worse than built |" if built else ""
    rule = "|" + " --- |" * (10 if built else 7)
    return [head, rule] + [row_line(r, built) for r in rows]


def head_lines(s):
    out = [f"# {s['game']}: fun sheet: {s['result']}", "", "Advice with evidence marks, not a proof.", "", "Limits:"]
    out += [f"{i}. {t}" for i, t in enumerate(s["limits"], 1)] + [""]
    out.append(f"Result: {s['result']} (counting rows out: {', '.join(s['counting_out']) or 'none'}): {ADVICE}.")
    if s.get("build"):
        out.append(f"As built ({s['build']}): {s['build_result']}. Counting rows out: "
                   f"{', '.join(s['build_counting_out']) or 'none'}.")
    out.append(f"Length E: {s['length']['minutes']:.1f} min ({s['length']['source']}). Economy currency c*: "
               f"{s['economy_currency']}. Run: {BASE}, horizon {s['runs'][0]['horizon_hours']:g} h.")
    return out + [f"Note: {n}" for n in s["notes"]]


def markdown(s):
    built = bool(s.get("build"))
    rows = s["rows"]
    missing = [r for r in rows if r["status"] == "not computed"]
    out = head_lines(s) + ["", "## Counting rows", ""] + table([r for r in rows if r["kind"] == "counts"], built)
    out += ["", "## Advice rows", ""] + table([r for r in rows if r["kind"] == "advice"], built)
    out += ["", "## Not computed", ""]
    out += ["| Id | Missing input |", "| --- | --- |"] + [f"| {r['id']} | {cell(r['missing'])} |" for r in missing] \
        if missing else ["None."]
    out += ["", "## Play test", "", "The sheet cannot compute these 4 measures (fun-metrics.md section 6):"]
    return "\n".join(out + [f"- {t}" for t in PLAY_TEST]) + "\n"


# ----------------------------------------------------------------------------- command line
ARGS = (
    (("model",), dict(help="the model JSON (references/model-schema.md)")),
    (("--build",), dict(metavar="FILE", help="the model of the game as built: the sheet shows it beside the design, "
                                             "marks each row that the design made worse, and gives the FM-3 drift")),
    (("--md",), dict(metavar="FILE", help="write the sheet as Markdown")),
    (("--json",), dict(metavar="FILE", dest="json_out", help="write the sheet as JSON")),
)


def parse(argv):
    ap = argparse.ArgumentParser(description="The fun sheet: the tier 1 fun metrics FM-1 to FM-12 of a model. "
                                             "The result is advice with evidence marks, not a proof.")
    for flags, kw in ARGS:
        ap.add_argument(*flags, **kw)
    return ap.parse_args(argv)


def build_sheet(a):
    build = load_model(a.build) if a.build else None
    return fun_sheet(load_model(a.model), a.model, build, a.build)


def safe_sheet(a):
    """The sheet, or None after an error message for a missing file, bad JSON or a model error."""
    try:
        return build_sheet(a)
    except (OSError, ValueError, KeyError) as e:
        sys.stderr.write(f"pw_fun: cannot read the input: {e}\n")
    except SystemExit as e:
        sys.stderr.write(f"pw_fun: {e}\n")
    return None


def write(path, text):
    pathlib.Path(path).write_text(text, encoding="utf-8")


def main(argv=None):
    a = parse(argv)
    s = safe_sheet(a)
    if s is None:
        return 2
    if a.md:
        write(a.md, markdown(s))
    if a.json_out:
        write(a.json_out, json.dumps(s, indent=1) + "\n")
    sys.stdout.write(markdown(s) if not (a.md or a.json_out) else
                     f"{s['game']}: fun sheet: {s['result']} ({', '.join(s['counting_out']) or 'no row out'}); {ADVICE}.\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
