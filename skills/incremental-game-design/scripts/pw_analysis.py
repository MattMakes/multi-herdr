"""Indicators for progression_workbook.py. All times are minutes of play unless a name says hours."""
import copy
import functools
import itertools
import math
import statistics
import time

from pw_model import INF, prestige_on
from pw_sim import Sim

TARGET_HOURS = (1, 3, 10)


def pm(e):
    """Minutes of play of an event, rounded as the Timeline sheet shows it."""
    return round(e["t"] / 60.0, 4)


def is_lull(gap, before, P):
    """The lull rule. The Timeline, Beats, and Dashboard formulas use the same rule."""
    return gap >= P["min_lull_minutes"] and (
        gap / max(before, P["ratio_floor_minutes"]) > P["lull_ratio"]
        or gap > max(P["max_gap_sessions"] * P["session_minutes"], P["late_ratio"] * before))


def _new_moment(e, t):
    return dict(at=t, real=round(e["real"] / 60.0, 4), names=[e["name"]], weight=e["weight"],
                systems=[e["system"]], stacked=1)


def _stack_beat(mo, e):
    mo["names"].append(e["name"])
    mo["weight"] = max(mo["weight"], e["weight"])
    mo["stacked"] += 1
    if e["system"] not in mo["systems"]:
        mo["systems"].append(e["system"])


def _add_beat(out, e, anchor, P):
    """Add beat e to a new moment or to the last moment. Return the new anchor."""
    t = pm(e)
    if (t - anchor) * 60 >= P["merge_seconds"]:
        out.append(_new_moment(e, t))
        return t
    _stack_beat(out[-1], e)
    return anchor


def _beat_events(events, P):
    return [e for e in events if e["weight"] >= P["beat_weight"]]


def find_moments(events, P):
    """Merge beats closer than merge_seconds to the start of a moment into that moment."""
    out, anchor = [], -1e6
    beats = _beat_events(events, P)
    for e in beats:
        anchor = _add_beat(out, e, anchor, P)
    return out


def bucket_size(end, session):
    b = session
    while end / b > 300:
        b *= 2
    return b, max(1, math.ceil(end / b - 1e-9))


def ideal_time(n, g0, r):
    return g0 * n if abs(r - 1) < 1e-12 else g0 * (r ** n - 1) / (r - 1)


def ideal_count(minutes, g0, r):
    if abs(r - 1) < 1e-12 or g0 <= 0:
        return math.floor(minutes / g0) if g0 > 0 else 0
    return math.floor(math.log(1 + minutes * (r - 1) / g0) / math.log(r))


def _ends_with(names):
    return " + ".join(names[:3]) + (" ..." if len(names) > 3 else "")


def _gap_row(mo, last, P):
    """Set the gap fields of moment mo and return its row of gaps."""
    gap = mo["at"] - last
    mo["after"], mo["gap"], mo["ratio"] = last, gap, gap / max(last, P["ratio_floor_minutes"])
    mo["lull"] = is_lull(gap, last, P)
    return dict(at_minutes=mo["at"], after=last, gap_minutes=gap, ratio=mo["ratio"], lull=mo["lull"],
                ends_with=_ends_with(mo["names"]), system=mo["systems"][0], tail=False)


def _gap_rows(moments, P):
    gaps, last = [], 0.0
    for mo in moments:
        gaps.append(_gap_row(mo, last, P))
        last = mo["at"]
    return gaps, last


def _tail_row(end, last, tail, P):
    return dict(at_minutes=end, after=last, gap_minutes=tail, ratio=tail / max(last, P["ratio_floor_minutes"]),
                lull=True, ends_with="(end of simulation: no beat)", system="-", tail=True)


def _longest_gap(gaps, tail):
    return max([g["gap_minutes"] for g in gaps if not g["tail"]] + [tail])


def _median_ratio(moments):
    ratios = [mo["ratio"] for mo in moments]
    return statistics.median(ratios) if ratios else 0


def _gap_part(moments, end, P):
    gaps, last = _gap_rows(moments, P)
    tail = end - last
    if is_lull(tail, last, P):
        gaps.append(_tail_row(end, last, tail, P))
    return dict(lulls=[g for g in gaps if g["lull"]], gaps=gaps, moments=moments, last_beat_minutes=last,
                tail_gap_minutes=tail, longest_gap_minutes=_longest_gap(gaps, tail),
                median_gap_ratio=_median_ratio(moments))


def _count_part(sim, end, moments, P):
    n_beat = len(_beat_events(sim.events, P))
    return dict(simulated_hours=end / 60.0, simulated_end_minutes=end, real_hours=sim.real / 3600.0,
                events=len(sim.events), beats=len(moments), beat_events=n_beat, stacked_beats=n_beat - len(moments),
                beats_first_hour=sum(1 for mo in moments if mo["at"] < 60))


FIRST_KEYS = (("first_purchase_minutes", "kind", "purchase"), ("first_system_unlock_minutes", "note", "system_unlock"),
              ("first_prestige_minutes", "kind", "prestige"))


def _first(ev, field, value):
    """Minutes of play of the first event whose field has the value, or None."""
    return next((pm(e) for e in ev if e[field] == value), None)


def _first_part(sim):
    out = {key: _first(sim.events, field, value) for key, field, value in FIRST_KEYS}
    out.update(prestige_count=sim.prestiges, prestige_run_minutes=[round(x["minutes"], 1) for x in sim.runs])
    return out


def _buckets(beat_events, end, P):
    """(bucket minutes, bucket count, share of buckets with no beat)."""
    bucket, n_b = bucket_size(end, P["session_minutes"])
    per = [0] * n_b
    for e in beat_events:
        per[min(n_b - 1, int(pm(e) // bucket))] += 1
    return bucket, n_b, sum(1 for x in per if x == 0) / n_b


def _session_part(sim, end, P):
    bucket, n_b, dry = _buckets(_beat_events(sim.events, P), end, P)
    stalled = [dict(at_minutes=pm(e), name=e["name"], note=e["note"]) for e in sim.events if e["kind"] == "stall"]
    return dict(bucket_minutes=bucket, sessions=n_b, dry_session_share=dry, stalled=stalled,
                event_limit_reached=sim.cut)


def _effect_targets(x, cids):
    """The currencies that the effects and the grants of x raise."""
    raised = {c for e in x["effects"] if e["kind"] != "cap" for c in (cids if e["target"] == "all" else [e["target"]])}
    return raised | {g["currency"] for g in x.get("grants", [])}


def _sources(m, cids):
    out = {c["id"] for c in m["currencies"] if c.get("base_income", 0) > 0 or c.get("start", 0) > 0}
    for x in m["purchases"] + m["milestones"] + m["ladders"]:
        out |= _effect_targets(x, cids)
    if prestige_on(m):
        out.add(m["prestige"]["currency"])
    return out


def _structure_part(m):
    cids = [c["id"] for c in m["currencies"]]
    sinks = {c["currency"] for p in m["purchases"] for c in p["costs"]}
    links = derive_links(m)
    touched = {a for a, _, _, _ in links} | {b for _, b, _, _ in links}
    return dict(currencies_without_source=sorted(set(cids) - _sources(m, cids)),
                currencies_without_sink=sorted({c["id"] for c in m["currencies"] if not c.get("stat")} - sinks),
                systems=len(m["systems"]), orphan_systems=sorted({s["id"] for s in m["systems"]} - touched),
                links=len(links))


def _weighted_options(sim):
    """(options within 5 minutes, seconds that the value holds) for each sample."""
    s = sim.samples
    return [(a["soon"], b["t"] - a["t"]) for a, b in zip(s, s[1:] + [dict(t=sim.t)])]


def _median_option(w, tot, default):
    if not tot > 0:
        return default
    pairs = sorted(w)
    sums = itertools.accumulate(d for _, d in pairs)
    return next((v for (v, _), acc in zip(pairs, sums) if acc >= tot / 2), default)


def _decision_part(sim):
    """Decision indicators, weighted by time."""
    w = _weighted_options(sim)
    tot = sum(d for _, d in w)
    default = sim.samples[0]["soon"] if sim.samples else 0
    no_opt = sum(d for v, d in w if v == 0) / tot if tot > 0 else 0.0
    return dict(median_options_within_5_min=_median_option(w, tot, default),
                share_of_time_with_no_option_within_5_min=no_opt)


def _behind_at(h, moments, end, P):
    if h * 60 > end + 1e-9:
        return None
    reached = sum(1 for mo in moments if mo["at"] <= h * 60)
    return ideal_count(h * 60, P["target_first_gap"], P["target_stretch"]) - reached


def _reach_part(m, sim, moments, end, P):
    return dict(never_bought=sorted(p["id"] for p in m["purchases"] if p["id"] not in sim.ever),
                never_reached=sorted(x["id"] for x in m["milestones"] if x["id"] not in sim.ever_done),
                ladder_reached={l["id"]: sim.ladder_best[l["id"]] for l in m["ladders"]},
                beats_behind_target={str(h): _behind_at(h, moments, end, P) for h in TARGET_HOURS})


def analyse(m, sim, P):
    end = round(sim.t / 60.0, 4)
    moments = find_moments(sim.events, P)
    gap_part = _gap_part(moments, end, P)
    A = _count_part(sim, end, moments, P)
    A.update(gap_part)
    A.update(_first_part(sim))
    A.update(_session_part(sim, end, P))
    A.update(_structure_part(m))
    A.update(_decision_part(sim))
    A.update(_reach_part(m, sim, moments, end, P))
    return A


def _system_of(items, item_id):
    return next((s["system"] for s in items if s["id"] == item_id), None)


class _Links:
    """The links of derive_links, in the order that they are found, and the (from, to, type) keys seen."""

    def __init__(self, m):
        self.m = m
        self.owner = {c["id"]: c.get("system") for c in m["currencies"]}
        self.psys = {p["id"]: p["system"] for p in m["purchases"]}
        self.out = [(i["from"], i["to"], i.get("type", "buff"), i.get("effect", "")) for i in m["interactions"]]
        self.seen = {(a, b, t) for a, b, t, _ in self.out}

    def add(self, a, b, t, note):
        if not (a and b and a != b and (a, b, t) not in self.seen):
            return
        self.seen.add((a, b, t))
        self.out.append((a, b, t, note))

    def add_cap(self, x, e):
        self.add(x["system"], self.psys.get(e["target"]), "buff", f"{x['name']} raises the cap of {e['target']}")

    def add_raise(self, x, e):
        targets = [e["target"]] if e["target"] != "all" else list(self.owner)
        for c in targets:
            self.add(x["system"], self.owner.get(c), "buff", f"{x['name']} raises {c}")

    def add_effects(self, x):
        effects = x["effects"]
        for e in effects:
            (self.add_cap if e["kind"] == "cap" else self.add_raise)(x, e)

    def add_grants(self, x):
        grants = x.get("grants", [])
        for g in grants:
            self.add(x["system"], self.owner.get(g["currency"]), "resource", f"{x['name']} grants {g['currency']}")

    def add_costs(self, x):
        costs = x.get("costs", [])
        for c in costs:
            self.add(self.owner.get(c["currency"]), x["system"], "resource", f"{c['currency']} pays for {x['name']}")

    def source_of(self, r):
        """The system that requirement r depends on, or None."""
        find = REQ_SOURCES.get(r.get("type"))
        return find(self, r) if find else None

    def add_requires(self, x):
        reqs = x.get("requires", [])
        for r in (reqs if isinstance(reqs, list) else [reqs]):
            self.add(self.source_of(r), x["system"], "gate", f"{x['name']} needs {r.get('type')}")

    def add_item(self, x):
        self.add_effects(x)
        self.add_grants(x)
        self.add_costs(x)
        self.add_requires(x)

    def add_ladder_basis(self, pr):
        src = _system_of(self.m["ladders"], pr["basis_ladder"])
        self.add(src, pr["system"], "resource", f"the step on {pr['basis_ladder']} sets the prestige gain")

    def add_currency_basis(self, pr):
        self.add(self.owner.get(pr["basis_currency"]), pr["system"], "resource",
                 f"{pr['basis_currency']} earned in the run sets the prestige gain")

    def add_prestige(self):
        if not prestige_on(self.m):
            return
        pr = self.m["prestige"]
        (self.add_ladder_basis if "basis_ladder" in pr else self.add_currency_basis)(pr)


def _currency_source(links, r):
    return links.owner.get(r["currency"])


def _level_source(links, r):
    return links.psys.get(r["id"])


def _milestone_source(links, r):
    return _system_of(links.m["milestones"], r["id"])


def _ladder_source(links, r):
    return _system_of(links.m["ladders"], r["id"])


REQ_SOURCES = {"total": _currency_source, "run_total": _currency_source, "have": _currency_source,
               "rate": _currency_source, "level": _level_source, "milestone": _milestone_source,
               "ladder": _ladder_source}


def derive_links(m):
    """(from_system, to_system, type, note) from the model: declared links plus links implied by the data."""
    links = _Links(m)
    for x in m["purchases"] + m["milestones"] + m["ladders"]:
        links.add_item(x)
    links.add_prestige()
    return links.out


# ------------------------------------------------------------------ lulls: causes, zones, robustness
def main_currency(m):
    n = {}
    for p in m["purchases"]:
        c = p["costs"][0]["currency"]
        n[c] = n.get(c, 0) + 1
    return max(n, key=n.get) if n else m["currencies"][0]["id"]


def _window(sim, a, b):
    """The (sample, next sample) pairs that start in the gap from minute a to minute b."""
    s = sim.samples
    return [(x, y) for x, y in zip(s, s[1:] + [dict(t=sim.t)]) if a * 60 - 1e-6 <= x["t"] < b * 60]


def _time_mean(inside, value):
    """The mean of value(sample) over the pairs, weighted by time. None for no time."""
    tot = sum(y["t"] - x["t"] for x, y in inside)
    return sum(value(x) * (y["t"] - x["t"]) for x, y in inside) / tot if tot > 0 else None


def _soon(x):
    return x["soon"]


def _capped(x):
    return x["capped"] / x["lines"] if x["lines"] else 0


def _income_growth(inside, mc):
    inc = [x["income"][mc] for x, _ in inside if x["income"][mc] > 0]
    return math.log10(max(inc) / inc[0]) if inc else None


def _sample_stats(sim, a, b, mc):
    inside = _window(sim, a, b)
    return dict(opts=_time_mean(inside, _soon), capped=_time_mean(inside, _capped), growth=_income_growth(inside, mc),
                wall=next((x["wall"][2] for x, _ in inside if x["wall"]), None))


def _event_stats(sim, A, g):
    a, b = g["after"], g["at_minutes"]
    ev = [e for e in sim.events if a < pm(e) <= b]
    stall = any(e["kind"] == "stall" for e in ev) or (g["tail"] and bool(A["stalled"]))
    return dict(pres=sum(1 for e in ev if e["kind"] == "prestige"), buys=sum(1 for e in ev if e["kind"] == "purchase"),
                stall=stall)


def _unlock_stats(unlocks, a):
    return dict(since=min([a - u for u in unlocks if u <= a], default=None), later=any(u > a for u in unlocks))


def _is_capped_end(st):
    return st["capped"] is not None and st["capped"] >= 0.6 and not st["later"]


def _is_wall(st):
    wall, growth = st["wall"], st["growth"]
    return wall is not None and wall != INF and wall >= 1.2 and growth is not None and growth < 0.3


def _why_capped(st):
    return f"{st['capped']:.0%} of the lines with a max level are at the max, and no new system opens later."


def _why_wall(st):
    return (f"The next step needs x{st['wall']:.2f} of the value at the gap start, and {st['mc']} income grew "
            f"x{10 ** st['growth']:.2f} in the gap.")


def _why_flat(st):
    return f"{st['buys']} purchases in the gap and {st['opts']:.1f} options within 5 minutes, but none is a beat."


LULL_RULES = (
    (lambda st: st["stall"], "End of content", lambda st: "The buyer had nothing left to buy or reach."),
    (_is_capped_end, "End of content", _why_capped),
    (lambda st: st["pres"] >= 2, "Reset that does not pay",
     lambda st: f"{st['pres']} resets in the gap, and none of the runs reached a new beat."),
    (_is_wall, "Wall", _why_wall),
    (lambda st: st["opts"] is not None and st["opts"] < 2, "Empty shop",
     lambda st: f"{st['opts']:.1f} options within 5 minutes on average."),
    (lambda st: st["buys"] > 0, "Flat menu", _why_flat),
    (lambda st: True, "Unclear", lambda st: "No rule matched. Read the Timeline rows of the gap."),
)


def _lull_fields(st):
    wall = st["wall"]
    return dict(options_within_5_min=st["opts"], capped_share=st["capped"], income_growth_log10=st["growth"],
                wall_ratio_at_start=None if wall in (None, INF) else wall, prestiges_in_gap=st["pres"],
                purchases_in_gap=st["buys"], minutes_since_system_unlock=st["since"])


def _explain_lull(g, sim, A, mc, unlocks):
    st = _sample_stats(sim, g["after"], g["at_minutes"], mc)
    st.update(_event_stats(sim, A, g), mc=mc)
    st.update(_unlock_stats(unlocks, g["after"]))
    _, cause, why = next(rule for rule in LULL_RULES if rule[0](st))
    g.update(cause=cause, why=why(st), **_lull_fields(st))


def lull_causes(m, sim, A, P):
    """Add cause data and a probable cause to each lull of A. Rules on data that the simulation has."""
    mc = main_currency(m)
    unlocks = [pm(e) for e in sim.events if e["kind"] == "milestone" and e["note"] == "system_unlock"]
    for g in A["lulls"]:
        _explain_lull(g, sim, A, mc, unlocks)


def _zone_for(zones, g, near):
    """The zone that lull g extends, or None."""
    last = zones[-1] if zones else None
    return last if last and g["after"] - last["end_minutes"] < near else None


def _merge_lull(zones, g, near):
    z = _zone_for(zones, g, near)
    if z is None:
        zones.append(dict(start_minutes=g["after"], end_minutes=g["at_minutes"], lulls=1))
    else:
        z["end_minutes"], z["lulls"] = g["at_minutes"], z["lulls"] + 1
    g["zone"] = len(zones)


def _finish_zone(z, A):
    z["minutes"] = z["end_minutes"] - z["start_minutes"]
    z["beats_inside"] = sum(1 for mo in A["moments"] if z["start_minutes"] < mo["at"] < z["end_minutes"])
    z["share_of_run"] = z["minutes"] / max(A["simulated_end_minutes"], 1e-9)


def dead_zones(A, P):
    """Merge neighbouring lulls. Two lulls are neighbours when less than the max gap is between them."""
    zones = []
    near = P["max_gap_sessions"] * P["session_minutes"]
    for g in A["lulls"]:
        _merge_lull(zones, g, near)
    for z in zones:
        _finish_zone(z, A)
    return zones


def _overlaps(g, o):
    return min(g["at_minutes"], o["at_minutes"]) - max(g["after"], o["after"]) > 0


def _robust_label(g, others):
    if not others:
        return "one profile"
    hit = all(any(_overlaps(g, o) for o in A["lulls"]) for A in others)
    return "robust" if hit else "depends on reset policy"


def mark_robust(results, primary):
    """A lull of the primary run is robust when a lull overlaps it in each other profile."""
    others = [A for n, (_, A) in results.items() if n != primary]
    lulls = results[primary][1]["lulls"]
    for g in lulls:
        g["robust"] = _robust_label(g, others)


def _ladder_step(ax, sim):
    return sim.ladder_best[ax] if ax else max(sim.ladder_best.values(), default=None)


SUMMARY_COPIED = ("beats", "stacked_beats", "beats_first_hour", "first_purchase_minutes",
                  "first_system_unlock_minutes", "first_prestige_minutes")


def _summary_gaps(A):
    out = dict(lulls=len(A["lulls"]), longest_gap_minutes=round(A["longest_gap_minutes"], 1),
               dead_zone_minutes=round(sum(z["minutes"] for z in A["dead_zones"]), 1))
    out.update((k, A[k]) for k in SUMMARY_COPIED)
    out.update(prestiges=A["prestige_count"])
    return out


def _summary_runs(m, sim):
    pr = m["prestige"]["currency"] if prestige_on(m) else None
    runs = [x["minutes"] for x in sim.runs]
    return dict(prestiges_with_nothing_new=sum(1 for x in sim.runs[1:] if not x["new_beats"]),
                median_run_minutes=round(statistics.median(runs), 1) if runs else None,
                lifetime_prestige_currency=sim.life[pr] if pr else None,
                ladder_step=_ladder_step(m["game"].get("progress_axis"), sim))


def _summary_time(A):
    return dict(dry_bucket_share=round(A["dry_session_share"], 3),
                median_options_within_5_min=A["median_options_within_5_min"],
                share_of_time_with_no_option=round(A["share_of_time_with_no_option_within_5_min"], 3),
                stalled=bool(A["stalled"]),
                lull_hours=[[round(g["after"] / 60.0, 2), round(g["at_minutes"] / 60.0, 2)] for g in A["lulls"]])


def summary(m, sim, A):
    """The main indicators of one profile, for the Profiles sheet and the analysis JSON."""
    out = _summary_gaps(A)
    out.update(_summary_runs(m, sim))
    out.update(_summary_time(A))
    return out


def run_profile(m, hours, prof, P):
    sim = Sim(m, hours, prof, P["beat_weight"]).play()
    A = analyse(m, sim, P)
    A["dead_zones"] = dead_zones(A, P)
    return sim, A


# ----------------------------------------------------------------------------- tables
def _never_status(st):
    if st["first_avail"] is None:
        return "never affordable: its requirements were never met"
    if st["first_afford"] is None:
        return "never affordable: the price was never reached"
    return "affordable and never chosen"


def _purchase_status(st):
    if st["first"] is None:
        return _never_status(st)
    if st["cap_at"] is not None:
        return f"capped at hour {st['cap_at'] / 3600.0:.2f}"
    return "active"


def _minutes_2(seconds):
    return None if seconds is None else round(seconds / 60.0, 2)


def _purchase_row(p, sim):
    st = sim.pstat[p["id"]]
    cap = sim.cap_of(p)
    return dict(id=p["id"], name=p["name"], system=p["system"], status=_purchase_status(st),
                first_bought_minutes=_minutes_2(st["first"]), last_bought_minutes=_minutes_2(st["last"]),
                highest_level=st["top"], levels_bought=st["bought"], cap_at_end=cap,
                spent=dict(st["spent"]), payback_first_seconds=st["pay_first"],
                payback_last_seconds=st["pay_last"])


def purchase_rows(m, sim):
    return [_purchase_row(p, sim) for p in m["purchases"]]


def _minus_amount(kv):
    return -kv[1]


def _by_amount(items):
    return sorted(items, key=_minus_amount)


def _currency_ledger(sim, c):
    out = [(c, "source", k, v) for k, v in _by_amount(sim.earn[c].items())]
    out += [(c, "sink", "purchases: " + k, v) for k, v in _by_amount(sim.spent[c].items())]
    if sim.lost[c] > 0:
        out.append((c, "sink", "lost at resets", sim.lost[c]))
    out.append((c, "balance", "balance at the end", sim.cur[c]))
    return out


def ledger_rows(m, sim):
    """(currency, direction, label, amount) rows."""
    return [row for c in sim.cids for row in _currency_ledger(sim, c)]


def _wall_times(hit, prev, now):
    reached = hit is not None
    return dict(reached_minutes=hit[0] / 60.0 if reached else None,
                wait_minutes=(hit[0] - prev) / 60.0 if reached else None,
                value=hit[1] if reached else now, reached=reached)


def _wall_row(l, step, hit, prev, now):
    r = l["requires"]
    return dict(ladder=l["id"], step=step, base=r["base"], growth=r["growth"], type=r["type"],
                currency=r.get("currency"), **_wall_times(hit, prev, now))


def _ladder_wall_rows(sim, l, inc, limit):
    prev = 0.0
    now = sim.quantity(sim.ladder_req(l, 1), inc)[0]
    rows = []
    for step in range(1, min(l["count"], limit) + 1):
        hit = sim.steps[l["id"]].get(step)
        rows.append(_wall_row(l, step, hit, prev, now))
        prev = prev if hit is None else hit[0]
    return rows


def wall_rows(m, sim, limit=300):
    out = []
    inc = sim.income()
    for l in m["ladders"]:
        out += _ladder_wall_rows(sim, l, inc, limit)
    return out


# ----------------------------------------------------------------------------- sensitivity
def _scaled(c, f):
    """Growth x f. For an exp cost the change is held to half of (growth - 1): x0.95 on 1.07 would make
    the line almost free."""
    g = c["growth"] * f
    if c["formula"] == "exp" and c["growth"] > 1:
        g = min(max(g, 1 + 0.5 * (c["growth"] - 1)), 1 + 1.5 * (c["growth"] - 1))
    return g


def _scale_ladder_growth(i, mm, f):
    req = mm["ladders"][i]["requires"]
    req["growth"] = req["growth"] * f


def _scale_prestige_exponent(mm, f):
    mm["prestige"]["exponent"] = mm["prestige"]["exponent"] * f


def _scale_cost_growth(i, mm, f):
    costs = mm["purchases"][i]["costs"]
    for c in costs:
        c["growth"] = _scaled(c, f)


def _ladder_params(m):
    return [(f"ladder {l['id']}: requirement growth", l["requires"]["growth"], functools.partial(_scale_ladder_growth, i))
            for i, l in enumerate(m["ladders"])]


def _prestige_params(m):
    if prestige_on(m) and m["prestige"]["formula"] == "power":
        return [("prestige: exponent", m["prestige"]["exponent"], _scale_prestige_exponent)]
    return []


def _purchase_param(m, i):
    p = m["purchases"][i]
    c0 = p["costs"][0]
    return (f"purchase {p['id']}: cost growth", c0["growth"], functools.partial(_scale_cost_growth, i),
            _scaled(c0, 0.95), _scaled(c0, 1.05))


def _purchase_params(m, base_A):
    bought = base_A["_bought"]
    order = sorted(range(len(m["purchases"])), key=lambda i: -bought.get(m["purchases"][i]["id"], 0))
    return [_purchase_param(m, i) for i in order]


def _sensitivity_run(m, hours, prof, P, change, f):
    """The main indicators of 1 run of a copy of m with 1 number changed by f."""
    mm = copy.deepcopy(m)
    change(mm, f)
    sim = Sim(mm, hours, prof, P["beat_weight"]).play()
    A = analyse(mm, sim, P)
    return dict(lulls=len(A["lulls"]), longest_gap_minutes=round(A["longest_gap_minutes"], 1),
                first_prestige_minutes=A["first_prestige_minutes"],
                ladder_step=_ladder_step(m["game"].get("progress_axis"), sim), beats=A["beats"])


def _sensitivity_row(param, run, base_gap):
    label, value, change, *used = param
    lo, hi = run(change, 0.95), run(change, 1.05)
    lo_v, hi_v = used or (value * 0.95, value * 1.05)
    return dict(parameter=label, base=value, low_value=lo_v, high_value=hi_v, low=lo,
                high=hi, change_low=round(lo["longest_gap_minutes"] - base_gap, 1),
                change_high=round(hi["longest_gap_minutes"] - base_gap, 1))


def _sensitivity_rows(params, run, base_gap, budget):
    """1 row for each parameter, until the run time is more than the budget."""
    t0 = time.time()
    within = itertools.takewhile(lambda _: not time.time() - t0 > budget, params)
    return [_sensitivity_row(param, run, base_gap) for param in within]


def _tested_count(possible, budget, base_seconds):
    """The count of parameters that 2 runs each can test within the budget."""
    return max(1, min(possible, int(budget / max(2 * base_seconds, 1e-3))))


def sensitivity(m, hours, prof, P, base_A, base_seconds, budget=6.0):
    """Run the primary profile again with one number changed at a time. Returns (rows, tested, possible)."""
    params = _ladder_params(m) + _prestige_params(m) + _purchase_params(m, base_A)
    n = _tested_count(len(params), budget, base_seconds)
    run = functools.partial(_sensitivity_run, m, hours, prof, P)
    rows = _sensitivity_rows(params[:n], run, round(base_A["longest_gap_minutes"], 1), budget)
    rows.sort(key=lambda x: -max(abs(x["change_low"]), abs(x["change_high"])))
    return rows, len(rows), len(params)
