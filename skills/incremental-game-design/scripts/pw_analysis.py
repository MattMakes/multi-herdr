"""Indicators for progression_workbook.py. All times are minutes of play unless a name says hours."""
import copy
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


def find_moments(events, P):
    """Merge beats closer than merge_seconds to the start of a moment into that moment."""
    out, anchor = [], -1e6
    for e in events:
        if e["weight"] < P["beat_weight"]:
            continue
        t = pm(e)
        if (t - anchor) * 60 >= P["merge_seconds"]:
            out.append(dict(at=t, real=round(e["real"] / 60.0, 4), names=[e["name"]], weight=e["weight"],
                            systems=[e["system"]], stacked=1))
            anchor = t
        else:
            mo = out[-1]
            mo["names"].append(e["name"])
            mo["weight"] = max(mo["weight"], e["weight"])
            mo["stacked"] += 1
            if e["system"] not in mo["systems"]:
                mo["systems"].append(e["system"])
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


def analyse(m, sim, P):
    ev = sim.events
    end = round(sim.t / 60.0, 4)
    moments = find_moments(ev, P)
    floor = P["ratio_floor_minutes"]
    gaps, last = [], 0.0
    for mo in moments:
        gap = mo["at"] - last
        mo["after"], mo["gap"], mo["ratio"] = last, gap, gap / max(last, floor)
        mo["lull"] = is_lull(gap, last, P)
        gaps.append(dict(at_minutes=mo["at"], after=last, gap_minutes=gap, ratio=mo["ratio"], lull=mo["lull"],
                         ends_with=" + ".join(mo["names"][:3]) + (" ..." if len(mo["names"]) > 3 else ""),
                         system=mo["systems"][0], tail=False))
        last = mo["at"]
    tail = end - last
    tail_lull = is_lull(tail, last, P)
    if tail_lull:
        gaps.append(dict(at_minutes=end, after=last, gap_minutes=tail, ratio=tail / max(last, floor), lull=True,
                         ends_with="(end of simulation: no beat)", system="-", tail=True))

    def first(pred):
        for e in ev:
            if pred(e):
                return pm(e)
        return None

    beat_events = [e for e in ev if e["weight"] >= P["beat_weight"]]
    bucket, n_b = bucket_size(end, P["session_minutes"])
    per = [0] * n_b
    for e in beat_events:
        per[min(n_b - 1, int(pm(e) // bucket))] += 1
    # decision indicators, weighted by time
    s = sim.samples
    w = [(a["soon"], b["t"] - a["t"]) for a, b in zip(s, s[1:] + [dict(t=sim.t)])]
    tot = sum(d for _, d in w)
    med_opt, acc = (s[0]["soon"] if s else 0), 0.0
    for v, d in sorted(w):
        acc += d
        if tot > 0 and acc >= tot / 2:
            med_opt = v
            break
    no_opt = sum(d for v, d in w if v == 0) / tot if tot > 0 else 0.0
    # structure
    cids = [c["id"] for c in m["currencies"]]
    sources, sinks = set(), set()
    for c in m["currencies"]:
        if c.get("base_income", 0) > 0 or c.get("start", 0) > 0:
            sources.add(c["id"])
    for x in m["purchases"] + m["milestones"] + m["ladders"]:
        for e in x["effects"]:
            if e["kind"] != "cap":
                sources.update(cids if e["target"] == "all" else [e["target"]])
        for g in x.get("grants", []):
            sources.add(g["currency"])
    for p in m["purchases"]:
        sinks.update(c["currency"] for c in p["costs"])
    if prestige_on(m):
        sources.add(m["prestige"]["currency"])
    links = derive_links(m)
    touched = {a for a, _, _, _ in links} | {b for _, b, _, _ in links}
    g0, r = P["target_first_gap"], P["target_stretch"]
    behind = {}
    for h in TARGET_HOURS:
        behind[str(h)] = (None if h * 60 > end + 1e-9 else
                          ideal_count(h * 60, g0, r) - sum(1 for mo in moments if mo["at"] <= h * 60))
    ratios = [mo["ratio"] for mo in moments]
    A = dict(
        simulated_hours=end / 60.0, simulated_end_minutes=end, real_hours=sim.real / 3600.0,
        events=len(ev), beats=len(moments), beat_events=len(beat_events),
        stacked_beats=len(beat_events) - len(moments),
        beats_first_hour=sum(1 for mo in moments if mo["at"] < 60),
        lulls=[g for g in gaps if g["lull"]], gaps=gaps, moments=moments,
        last_beat_minutes=last, tail_gap_minutes=tail,
        longest_gap_minutes=max([g["gap_minutes"] for g in gaps if not g["tail"]] + [tail]),
        median_gap_ratio=statistics.median(ratios) if ratios else 0,
        first_purchase_minutes=first(lambda e: e["kind"] == "purchase"),
        first_system_unlock_minutes=first(lambda e: e["note"] == "system_unlock"),
        first_prestige_minutes=first(lambda e: e["kind"] == "prestige"),
        prestige_count=sim.prestiges, prestige_run_minutes=[round(x["minutes"], 1) for x in sim.runs],
        bucket_minutes=bucket, sessions=n_b, dry_session_share=sum(1 for x in per if x == 0) / n_b,
        stalled=[dict(at_minutes=pm(e), name=e["name"], note=e["note"]) for e in ev if e["kind"] == "stall"],
        event_limit_reached=sim.cut,
        currencies_without_source=sorted(set(cids) - sources),
        currencies_without_sink=sorted({c["id"] for c in m["currencies"] if not c.get("stat")} - sinks),
        systems=len(m["systems"]), orphan_systems=sorted({s["id"] for s in m["systems"]} - touched),
        links=len(links), median_options_within_5_min=med_opt,
        share_of_time_with_no_option_within_5_min=no_opt,
        never_bought=sorted(p["id"] for p in m["purchases"] if p["id"] not in sim.ever),
        never_reached=sorted(x["id"] for x in m["milestones"] if x["id"] not in sim.ever_done),
        ladder_reached={l["id"]: sim.ladder_best[l["id"]] for l in m["ladders"]},
        beats_behind_target=behind,
    )
    return A


def derive_links(m):
    """(from_system, to_system, type, note) from the model: declared links plus links implied by the data."""
    owner = {c["id"]: c.get("system") for c in m["currencies"]}
    psys = {p["id"]: p["system"] for p in m["purchases"]}
    out = [(i["from"], i["to"], i.get("type", "buff"), i.get("effect", "")) for i in m["interactions"]]
    seen = {(a, b, t) for a, b, t, _ in out}

    def add(a, b, t, note):
        if a and b and a != b and (a, b, t) not in seen:
            seen.add((a, b, t))
            out.append((a, b, t, note))

    for x in m["purchases"] + m["milestones"] + m["ladders"]:
        for e in x["effects"]:
            if e["kind"] == "cap":
                add(x["system"], psys.get(e["target"]), "buff", f"{x['name']} raises the cap of {e['target']}")
                continue
            for c in ([e["target"]] if e["target"] != "all" else list(owner)):
                add(x["system"], owner.get(c), "buff", f"{x['name']} raises {c}")
        for g in x.get("grants", []):
            add(x["system"], owner.get(g["currency"]), "resource", f"{x['name']} grants {g['currency']}")
        for c in x.get("costs", []):
            add(owner.get(c["currency"]), x["system"], "resource", f"{c['currency']} pays for {x['name']}")
        reqs = x.get("requires", [])
        for r in (reqs if isinstance(reqs, list) else [reqs]):
            src = None
            if r.get("type") in ("total", "run_total", "have", "rate"):
                src = owner.get(r["currency"])
            elif r.get("type") == "level":
                src = psys.get(r["id"])
            elif r.get("type") == "milestone":
                src = next((s["system"] for s in m["milestones"] if s["id"] == r["id"]), None)
            elif r.get("type") == "ladder":
                src = next((s["system"] for s in m["ladders"] if s["id"] == r["id"]), None)
            add(src, x["system"], "gate", f"{x['name']} needs {r.get('type')}")
    if prestige_on(m):
        pr = m["prestige"]
        if "basis_ladder" in pr:
            src = next((l["system"] for l in m["ladders"] if l["id"] == pr["basis_ladder"]), None)
            add(src, pr["system"], "resource", f"the step on {pr['basis_ladder']} sets the prestige gain")
        else:
            add(owner.get(pr["basis_currency"]), pr["system"], "resource",
                f"{pr['basis_currency']} earned in the run sets the prestige gain")
    return out


# ------------------------------------------------------------------ lulls: causes, zones, robustness
def main_currency(m):
    n = {}
    for p in m["purchases"]:
        c = p["costs"][0]["currency"]
        n[c] = n.get(c, 0) + 1
    return max(n, key=n.get) if n else m["currencies"][0]["id"]


def lull_causes(m, sim, A, P):
    """Add cause data and a probable cause to each lull of A. Rules on data that the simulation has."""
    s = sim.samples
    mc = main_currency(m)
    unlocks = [pm(e) for e in sim.events if e["kind"] == "milestone" and e["note"] == "system_unlock"]
    for g in A["lulls"]:
        a, b = g["after"], g["at_minutes"]
        inside = [(x, y) for x, y in zip(s, s[1:] + [dict(t=sim.t)]) if a * 60 - 1e-6 <= x["t"] < b * 60]
        tot = sum(y["t"] - x["t"] for x, y in inside)
        opts = sum(x["soon"] * (y["t"] - x["t"]) for x, y in inside) / tot if tot > 0 else None
        capped = (sum((x["capped"] / x["lines"] if x["lines"] else 0) * (y["t"] - x["t"]) for x, y in inside) / tot
                  if tot > 0 else None)
        inc = [x["income"][mc] for x, _ in inside if x["income"][mc] > 0]
        growth = math.log10(max(inc) / inc[0]) if inc else None
        walls = [x["wall"][2] for x, _ in inside if x["wall"]]
        wall = walls[0] if walls else None
        ev = [e for e in sim.events if a < pm(e) <= b]
        pres = sum(1 for e in ev if e["kind"] == "prestige")
        buys = sum(1 for e in ev if e["kind"] == "purchase")
        stall = any(e["kind"] == "stall" for e in ev) or (g["tail"] and bool(A["stalled"]))
        since = min([a - u for u in unlocks if u <= a], default=None)
        later = any(u > a for u in unlocks)
        if stall:
            cause, why = "End of content", "The buyer had nothing left to buy or reach."
        elif capped is not None and capped >= 0.6 and not later:
            cause = "End of content"
            why = f"{capped:.0%} of the lines with a max level are at the max, and no new system opens later."
        elif pres >= 2:
            cause = "Reset that does not pay"
            why = f"{pres} resets in the gap, and none of the runs reached a new beat."
        elif wall is not None and wall != INF and wall >= 1.2 and growth is not None and growth < 0.3:
            cause = "Wall"
            why = (f"The next step needs x{wall:.2f} of the value at the gap start, and {mc} income grew "
                   f"x{10 ** growth:.2f} in the gap.")
        elif opts is not None and opts < 2:
            cause, why = "Empty shop", f"{opts:.1f} options within 5 minutes on average."
        elif buys > 0:
            cause = "Flat menu"
            why = f"{buys} purchases in the gap and {opts:.1f} options within 5 minutes, but none is a beat."
        else:
            cause, why = "Unclear", "No rule matched. Read the Timeline rows of the gap."
        g.update(cause=cause, why=why, options_within_5_min=opts, capped_share=capped, income_growth_log10=growth,
                 wall_ratio_at_start=None if wall in (None, INF) else wall, prestiges_in_gap=pres,
                 purchases_in_gap=buys, minutes_since_system_unlock=since)


def dead_zones(A, P):
    """Merge neighbouring lulls. Two lulls are neighbours when less than the max gap is between them."""
    zones = []
    near = P["max_gap_sessions"] * P["session_minutes"]
    for g in A["lulls"]:
        if zones and g["after"] - zones[-1]["end_minutes"] < near:
            z = zones[-1]
            z["end_minutes"], z["lulls"] = g["at_minutes"], z["lulls"] + 1
        else:
            zones.append(dict(start_minutes=g["after"], end_minutes=g["at_minutes"], lulls=1))
        g["zone"] = len(zones)
    for z in zones:
        z["minutes"] = z["end_minutes"] - z["start_minutes"]
        z["beats_inside"] = sum(1 for mo in A["moments"] if z["start_minutes"] < mo["at"] < z["end_minutes"])
        z["share_of_run"] = z["minutes"] / max(A["simulated_end_minutes"], 1e-9)
    return zones


def mark_robust(results, primary):
    """A lull of the primary run is robust when a lull overlaps it in each other profile."""
    others = [A for n, (_, A) in results.items() if n != primary]
    for g in results[primary][1]["lulls"]:
        if not others:
            g["robust"] = "one profile"
            continue
        hit = all(any(min(g["at_minutes"], o["at_minutes"]) - max(g["after"], o["after"]) > 0 for o in A["lulls"])
                  for A in others)
        g["robust"] = "robust" if hit else "depends on reset policy"


def summary(m, sim, A):
    """The main indicators of one profile, for the Profiles sheet and the analysis JSON."""
    pr = m["prestige"]["currency"] if prestige_on(m) else None
    ax = m["game"].get("progress_axis")
    runs = [x["minutes"] for x in sim.runs]
    return dict(
        lulls=len(A["lulls"]), longest_gap_minutes=round(A["longest_gap_minutes"], 1),
        dead_zone_minutes=round(sum(z["minutes"] for z in A["dead_zones"]), 1),
        beats=A["beats"], stacked_beats=A["stacked_beats"], beats_first_hour=A["beats_first_hour"],
        first_purchase_minutes=A["first_purchase_minutes"],
        first_system_unlock_minutes=A["first_system_unlock_minutes"],
        first_prestige_minutes=A["first_prestige_minutes"], prestiges=A["prestige_count"],
        prestiges_with_nothing_new=sum(1 for x in sim.runs[1:] if not x["new_beats"]),
        median_run_minutes=round(statistics.median(runs), 1) if runs else None,
        lifetime_prestige_currency=sim.life[pr] if pr else None,
        ladder_step=(sim.ladder_best[ax] if ax else max(sim.ladder_best.values(), default=None)),
        dry_bucket_share=round(A["dry_session_share"], 3),
        median_options_within_5_min=A["median_options_within_5_min"],
        share_of_time_with_no_option=round(A["share_of_time_with_no_option_within_5_min"], 3),
        stalled=bool(A["stalled"]),
        lull_hours=[[round(g["after"] / 60.0, 2), round(g["at_minutes"] / 60.0, 2)] for g in A["lulls"]],
    )


def run_profile(m, hours, prof, P):
    sim = Sim(m, hours, prof, P["beat_weight"]).play()
    A = analyse(m, sim, P)
    A["dead_zones"] = dead_zones(A, P)
    return sim, A


# ----------------------------------------------------------------------------- tables
def purchase_rows(m, sim):
    out = []
    for p in m["purchases"]:
        st = sim.pstat[p["id"]]
        cap = sim.cap_of(p)
        if st["first"] is None:
            if st["first_avail"] is None:
                status = "never affordable: its requirements were never met"
            elif st["first_afford"] is None:
                status = "never affordable: the price was never reached"
            else:
                status = "affordable and never chosen"
        elif st["cap_at"] is not None:
            status = f"capped at hour {st['cap_at'] / 3600.0:.2f}"
        else:
            status = "active"
        out.append(dict(id=p["id"], name=p["name"], system=p["system"], status=status,
                        first_bought_minutes=None if st["first"] is None else round(st["first"] / 60.0, 2),
                        last_bought_minutes=None if st["last"] is None else round(st["last"] / 60.0, 2),
                        highest_level=st["top"], levels_bought=st["bought"], cap_at_end=cap,
                        spent=dict(st["spent"]), payback_first_seconds=st["pay_first"],
                        payback_last_seconds=st["pay_last"]))
    return out


def ledger_rows(m, sim):
    """(currency, direction, label, amount) rows."""
    out = []
    for c in sim.cids:
        for k, v in sorted(sim.earn[c].items(), key=lambda kv: -kv[1]):
            out.append((c, "source", k, v))
        for k, v in sorted(sim.spent[c].items(), key=lambda kv: -kv[1]):
            out.append((c, "sink", "purchases: " + k, v))
        if sim.lost[c] > 0:
            out.append((c, "sink", "lost at resets", sim.lost[c]))
        out.append((c, "balance", "balance at the end", sim.cur[c]))
    return out


def wall_rows(m, sim, limit=300):
    out = []
    inc = sim.income()
    for l in m["ladders"]:
        r = l["requires"]
        prev = 0.0
        now = sim.quantity(sim.ladder_req(l, 1), inc)[0]
        for step in range(1, min(l["count"], limit) + 1):
            hit = sim.steps[l["id"]].get(step)
            out.append(dict(ladder=l["id"], step=step, base=r["base"], growth=r["growth"], type=r["type"],
                            currency=r.get("currency"), reached_minutes=None if hit is None else hit[0] / 60.0,
                            wait_minutes=None if hit is None else (hit[0] - prev) / 60.0,
                            value=hit[1] if hit is not None else now, reached=hit is not None))
            if hit is not None:
                prev = hit[0]
    return out


# ----------------------------------------------------------------------------- sensitivity
def sensitivity(m, hours, prof, P, base_A, base_seconds, budget=6.0):
    """Run the primary profile again with one number changed at a time. Returns (rows, tested, possible)."""
    ax = m["game"].get("progress_axis")
    params = []

    def scaled(c, f):
        """Growth x f. For an exp cost the change is held to half of (growth - 1): x0.95 on 1.07 would make
        the line almost free."""
        g = c["growth"] * f
        if c["formula"] == "exp" and c["growth"] > 1:
            g = min(max(g, 1 + 0.5 * (c["growth"] - 1)), 1 + 1.5 * (c["growth"] - 1))
        return g

    for i, l in enumerate(m["ladders"]):
        params.append((f"ladder {l['id']}: requirement growth", l["requires"]["growth"],
                       lambda mm, f, i=i: mm["ladders"][i]["requires"].__setitem__(
                           "growth", mm["ladders"][i]["requires"]["growth"] * f)))
    if prestige_on(m) and m["prestige"]["formula"] == "power":
        params.append(("prestige: exponent", m["prestige"]["exponent"],
                       lambda mm, f: mm["prestige"].__setitem__("exponent", mm["prestige"]["exponent"] * f)))
    order = sorted(range(len(m["purchases"])), key=lambda i: -base_A["_bought"].get(m["purchases"][i]["id"], 0))
    for i in order:
        p = m["purchases"][i]

        def change(mm, f, i=i):
            for c in mm["purchases"][i]["costs"]:
                c["growth"] = scaled(c, f)
        c0 = p["costs"][0]
        params.append((f"purchase {p['id']}: cost growth", c0["growth"], change, scaled(c0, 0.95), scaled(c0, 1.05)))
    possible = len(params)
    n = max(1, min(possible, int(budget / max(2 * base_seconds, 1e-3))))
    t0 = time.time()

    def one(change, f):
        mm = copy.deepcopy(m)
        change(mm, f)
        sim = Sim(mm, hours, prof, P["beat_weight"]).play()
        A = analyse(mm, sim, P)
        return dict(lulls=len(A["lulls"]), longest_gap_minutes=round(A["longest_gap_minutes"], 1),
                    first_prestige_minutes=A["first_prestige_minutes"],
                    ladder_step=(sim.ladder_best[ax] if ax else max(sim.ladder_best.values(), default=None)),
                    beats=A["beats"])

    rows, base_gap = [], round(base_A["longest_gap_minutes"], 1)
    for label, value, change, *used in params[:n]:
        if time.time() - t0 > budget:
            break
        lo, hi = one(change, 0.95), one(change, 1.05)
        lo_v, hi_v = used or (value * 0.95, value * 1.05)
        rows.append(dict(parameter=label, base=value, low_value=lo_v, high_value=hi_v, low=lo,
                         high=hi, change_low=round(lo["longest_gap_minutes"] - base_gap, 1),
                         change_high=round(hi["longest_gap_minutes"] - base_gap, 1)))
    rows.sort(key=lambda x: -max(abs(x["change_low"]), abs(x["change_high"])))
    return rows, len(rows), possible
