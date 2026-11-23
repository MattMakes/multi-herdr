#!/usr/bin/env python3
"""The simulation path: a starting model from a selection, and the timing fit by simulation of a model with no reset.

    python3 pw_scaffold.py <selection.json> <targets.json> --out MODEL [--no-fit]
    python3 pw_scaffold.py --fit MODEL <targets.json> --out MODEL
    python3 pw_scaffold.py --vary MODEL <targets.json>

The first form writes a model from the selection. Each selected system that the scaffold can express becomes model
lines with the default values of assets/systems.json. Each other system is a stub with a key that says what to fill.
The key simulation_path of the model marks each part: "law" (written, and a law covers it with no reset layer),
"simulated" (written, the simulation alone checks it) or "not written" (with the reason; "fill": true when a person
must write it). The fit then sets the walls, the unlock points and the first costs from the simulator.

The second form fits a model that a person completed by hand. The third form changes each hand-set number of
simulation_path.hand_set by -20% and +20% and prints the targets of the active player for each change.

references/simulation-path.md gives the procedure. Exit code: 0 when the model is written and the fit keeps each
target of the timeline in its range (or each varied model keeps the targets); 1 when the model is written and the fit
misses a target of the timeline (lines "FIT MISS" and "FIT CAUSE"), or when 1 or more varied models miss a target;
2 for bad input, also a hand-set path that does not resolve.
"""
import argparse
import copy
import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import pw_design as pd  # noqa: E402
import pw_players  # noqa: E402
import pw_proof  # noqa: E402
import pw_rebalance as rb  # noqa: E402

ECON, AXIS = pd.ECON, pd.AXIS
LAW, SIMULATED, NOT_WRITTEN = "law", "simulated", "not written"
NO_RESET_LAWS = ("S01", "S02", "S05")
STAGE_SHARE = pd.UNLOCK_SHARE
FIT_PASSES = 4
REFINE_PASSES = 6
FIRST_COST_SECONDS = 20.0
VISITOR_BANK_S = 60.0
AUTO_SHARE = 0.25
COUNT_STEPS = 4
PLACE_GROUPS = (1.0, 4.0)
PLACE_STEP = 3.0
VARY = (0.8, 1.2)
RATIO_KEYS = ("cost_growth", "growth")
WALL_SHADE = 1.0 - 1e-5
STAGE_TOL = 0.20
CAUSE_STEP_S = 30.0
FALL_SHARE = 0.01
HOUR = 3600.0


class Scaffold:
    """The model that the writers fill, and the parts with their marks."""

    def __init__(self, d):
        self.d, self.cat, self.tv = d, d.cat, d.tv
        self.m = dict(game={}, currencies=[], systems=[], purchases=[], milestones=[], ladders=[],
                      prestige=dict(enabled=False))
        self.parts, self.notes, self.hand_set, self.base_income = [], [], [], 1.0
        self.more_currencies = []

    def param(self, vid, name):
        p = self.cat.param(vid, name)
        return self.d.over.get(vid, {}).get(name, p["default"])

    def has(self, kind, ident):
        return any(x["id"] == ident for x in self.m[kind])


def add(v, active=False):
    e = dict(target=ECON, kind="add", value=v)
    if active:
        e["active"] = True
    return e


def write_s01(sc, vid):
    poly = vid == "S01.polynomial"
    ratio = sc.param(vid, "k") if poly else sc.param(vid, pd.UNIT_PARAM.get(vid, "b"))
    p = pd.line("gen", "Generator", "s01", ECON, pd.GEN_COST, ratio, add(pd.GEN_VALUE))
    if poly:
        p["cost_formula"] = "poly"
    sc.m["purchases"].append(p)
    return ["gen"], "BM L1: an additive line (r = 0)"


def write_s02(sc, vid):
    """1 multiplier group for each S02 entry of the selection that is not a care entry, as the solver does on the
    path "1 long run" (pw_design.group_entries); 2 groups with no entry id when the selection has 1 S02 entry."""
    entries = pd.group_entries(sc.d)
    variants = {e["id"]: e["variant"] for e in pd.s02_entries(sc.d)}
    out = [s02_line(sc, variants.get(eid, vid), i, cost, eid)
           for i, (cost, eid) in enumerate(zip(pd.group_costs(sc.d), entries))]
    sc.m["purchases"] += out
    return [p["id"] for p in out], "BM L1: r = ln d / ln b for each line"


def s02_line(sc, vid, i, cost, entry=None):
    """Multiplier group i + 1: a levelled factor d, or a share v added in its group."""
    pid = f"m{i + 1}"
    eff = (pd.mult(sc.param(vid, "d")) if vid == "S02.levelled_factor" else
           dict(target=ECON, kind="pct", value=sc.param(vid, "v"), group=pid))
    p = pd.line(pid, pd.group_name(i, entry), "s02", ECON, cost, sc.param(vid, "b"), eff)
    p["place_minutes"] = PLACE_GROUPS[i] if i < len(PLACE_GROUPS) else PLACE_GROUPS[-1] + PLACE_STEP * (
        i - len(PLACE_GROUPS) + 1)
    return dict(p, entry=entry) if entry else p


def write_s03(sc, vid):
    req = dict(type="rate", currency=ECON, base=1.0, growth=sc.param(vid, "g"))
    sc.m["ladders"].append(dict(id=AXIS, name="Stage {n}", system="s03", count=1, excitement=3, requires=req,
                                effects=[]))
    return [AXIS], "BM L8 needs the reset loop; the timing fit sets each wall by simulation"


def write_s05(sc, vid):
    if vid == "S05.active_lane":
        a = sc.tv["a"] if sc.tv["a"] is not None else sc.param(vid, "a")
        sc.base_income = 1.0 / a
        eff = dict(target=ECON, kind="mult", value=a, active=True)
        sc.m["milestones"].append(dict(id="active_play", name="Active play multiplies income", system="s05",
                                       kind="feature", persists=True, effects=[eff]))
        return ["active_play"], "BM L5: an active income term a gives f_open = 1 / a"
    sc.base_income = 0.0
    sc.m["milestones"].append(dict(id="manual", name="Manual action", system="s05", kind="feature", persists=True,
                                   effects=[add(1.0, active=True)]))
    sc.notes.append(f"{vid}: the manual action pays 1 coin each second while the player plays; the generator is the "
                    "automation. A phase change: no law covers it (BM section 7).")
    return ["manual"], "a phase change (BM section 7)"


def write_s06(sc, vid):
    sc.m["game"]["offline_rate"] = sc.param(vid, "f_closed")
    sc.m["game"].setdefault("presence", {})["offline_cap_minutes"] = sc.param(vid, "T_cap_h") * 60.0
    return [], "game.offline_rate and game.presence.offline_cap_minutes"


def write_s10(sc, vid):
    t_mean = (sc.param(vid, "t_min_s") + sc.param(vid, "t_max_s")) / 2.0
    f = 1.0 + sc.param(vid, "share") * VISITOR_BANK_S / t_mean
    sc.m["milestones"].append(dict(id="visitor", name="Random visitor", system="s10", kind="feature", persists=True,
                                   place_minutes=None,
                                   effects=[dict(target=ECON, kind="mult", value=round(f, 4), active=True)]))
    sc.notes.append(f"{vid}: chance, modelled as its mean rate: x{f:.4f} while the player plays (1 visitor each "
                    f"{t_mean:g} s, a reward of share x the bank, and a bank of about {VISITOR_BANK_S:g} s of income).")
    sc.hand_set.append(dict(name="visitor factor", path=["milestones", "visitor", "effects", 0, "value"]))
    return ["visitor"], "chance (BM section 7, limit 1)"


def write_s12(sc, vid):
    if not sc.has("purchases", "gen"):
        return None
    n, mv = sc.param(vid, "n"), sc.param(vid, "m")
    steps = [count_step(k, n, mv) for k in range(1, COUNT_STEPS + 1)]
    sc.m["milestones"] += steps
    return [x["id"] for x in steps], f"x{mv:g} for each {n:g} generators"


def count_step(k, n, mv):
    return dict(id=f"count_{k}", name=f"{int(n * k)} generators", system="s12", kind="reward", persists=True,
                requires=[dict(type="level", id="gen", level=int(n * k))], effects=[pd.mult(mv)])


def write_s13(sc, vid):
    gen = next((p for p in sc.m["purchases"] if p["id"] == "gen"), None)
    if gen is None or not sc.has("ladders", AXIS):
        return None
    gen["max_level"] = int(sc.param(vid, "c0"))
    sc.m["ladders"][0]["effects"].append(dict(target="gen", kind="cap", value=sc.param(vid, "c1")))
    return ["gen", AXIS], "cap = c0 + c1 x stage: a cap moves the line out of L1"


def write_s24(sc, vid):
    if not sc.has("purchases", "gen"):
        return None
    t = round(AUTO_SHARE * sc.tv["H"] * 60.0, 1)
    sc.m["automation"] = [dict(line="gen", from_minutes=t, place_minutes=t, reason="S24.auto_buy: the auto-buy of "
                               "the generator, placed at 25% of the length (a hand-set number)")]
    sc.hand_set.append(dict(name="auto-buy unlock total", path=["automation", "gen", "requires", 0, "amount"]))
    return ["automation: gen"], "no sourced formula for the value of automation"


RESET_MIN_RUN = 0.5


def write_s04(sc, vid):
    """S04.power: the reset layer of the solver (pw_design.prestige) with the default p of the catalog. finish_reset
    sets q and c when the lines are written."""
    sc.m["prestige"] = dict(enabled=True, system="s04", currency=pd.RESET_CUR, basis_currency=ECON, formula="power",
                            base=None, exponent=sc.param(vid, "p"), k=1, floor=False, min_gain=1,
                            min_run_seconds=RESET_MIN_RUN * sc.tv["run_min"] * 60.0, policy="ratio",
                            policy_ratio=pd.X_E, resets=dict(currencies=[ECON], purchase_systems=[], ladders=[]),
                            reward=dict(target=ECON, exponent=None))
    sc.more_currencies.append(dict(id=pd.RESET_CUR, name="Reset currency", system="s04"))
    sc.hand_set.append(dict(name="reset divisor c", path=["prestige", "base"]))
    return ["prestige"], "simulation only: q = (1 - R) / p of the written lines (BM L6), and c gives a gain of 1 at " \
                         "the end of the first run (run_min); the walls of the fit do not follow the reset loop"


def write_s30(sc, vid):
    """S30.need_capacity: the upkeep block of the solver (pw_design.upkeep_block) on the written lines, and 1 care
    line. u = min(1, C / N)^e, with N the levels of the producer lines and C the base capacity times the care
    factor."""
    lines = [p["id"] for p in sc.m["purchases"] if p["system"] in ("s01", "s02")]
    if not lines:
        return None
    entry = pd.care_entry(sc.d)
    care = pd.line(entry or "care", f"Care line ({entry})" if entry else "Care line", "s30", ECON,
                   pd.group_costs(sc.d)[0] / pd.CARE_FIRST_COST, 1.5, None)
    care["effects"] = []
    sc.m["purchases"].append(dict(care, entry=entry) if entry else care)
    sc.m["upkeep"] = upkeep_of(sc.param(vid, "e"), sc.tv["pv_floor"], lines, care["id"])
    sc.hand_set.append(dict(name="care step", path=["upkeep", "capacity", 0, "value"]))
    return ["upkeep", care["id"]], ("BM 3b: the care step c has c^e - 1 = 2 x the PV floor (PU3); no law gives the "
                                    "path of u")


def upkeep_of(e, floor, lines, care="care"):
    """The upkeep block: the care step c with c^e - 1 = 2 x the PV floor, and the band of 2 care steps below 1."""
    step = (1.0 + 2.0 * floor) ** (1.0 / e)
    return dict(system="s30", target=ECON, rule="need_capacity", exponent=e, floor=0.0, base_need=0.0,
                base_capacity=pd.CARE_BASE_NEED, band=[round(step ** (-2.0 * e), 2), 1.0],
                need=[dict(purchase=pid, per_level=1.0) for pid in lines],
                capacity=[dict(purchase=care, kind="mult", value=round(step, 4))],
                min_switches=pw_proof.UPKEEP_SWITCHES)


def write_s28(sc, vid):
    if vid != "S28.last_step" or not sc.has("ladders", AXIS):
        return None
    return ["game.targets.hard_end"], "the last stage N; BM L8 needs the reset loop, so the fit sets N"


WRITERS = {"S01": write_s01, "S02": write_s02, "S03": write_s03, "S04": write_s04, "S05": write_s05,
           "S06": write_s06, "S10": write_s10, "S12": write_s12, "S13": write_s13, "S24": write_s24, "S28": write_s28,
           "S30": write_s30}
CAN_WRITE = {"S01": ("S01.geometric", "S01.steep", "S01.ladder", "S01.polynomial"),
             "S02": ("S02.levelled_factor", "S02.add_in_group"), "S03": ("S03.exponential",),
             "S04": ("S04.power",), "S30": ("S30.need_capacity",), "S05": ("S05.active_lane", "S05.manual_then_automate"), "S06": ("S06.penalty_cap",),
             "S10": ("S10.window",), "S12": ("S12.count_steps",), "S13": ("S13.raised_cap",),
             "S24": ("S24.auto_buy",), "S28": ("S28.last_step",)}
ORDER = ("S01", "S02", "S30", "S03", "S04", "S05", "S06", "S13", "S12", "S10", "S24", "S28")
NOT_WRITTEN_WHY = {
    "S04.none": "the game has no reset layer: the model has no prestige block, and the laws of the reset loop (PO2 to "
                "PO5) do not apply",
    "S29": "the simulator plays 1 continuous run and buys at any time: a run limit that only stops the purchases "
           "has no effect on the economy of the model. If the game stops the income between runs, add an uptime "
           "factor (a mult below 1) by hand (references/simulation-path.md). A play test must confirm the rhythm",
}


CANNOT_EXPRESS = {
    "S21": "the model cannot express an allocation pool: no construct splits 1 pool between 2 or more sinks, so "
           "'speed = share^e for each sink' has no model line. The fold-in method: write 1 purchase line for each "
           "sink (for example producer levels and care levels), and let the buyer of the simulator choose between "
           "them. The direction of the error: each line is linear in its levels (e = 1 in place of e < 1), so the "
           "model has no diminishing return inside a sink; it overstates the income of a player who puts the pool "
           "in 1 sink, and the simulated pace is an upper bound for that player. A play test must confirm the split",
    "S18": "the model cannot express a chain of producers: an effect adds income to a currency, and no effect adds "
           "levels to a purchase line, so 'x_i <- x_i + x_(i+1) x rate_i x dt' has no model line. The fold-in "
           "method: 2 currencies. Tier 2 is a line that makes a side currency, and the side currency buys the levels "
           "of tier 1. The direction of the error: tier 1 does not grow by itself, so the model has no power-of-time "
           "growth of the chain; it understates the late income, and the simulated hard end is later than the hard "
           "end of the game (a lower bound of the pace). A play test must confirm the late pace",
}


def write_parts(sc):
    chosen = dict(sc.d.chosen)
    chosen.setdefault("S04", "S04.none")
    for sid in sorted(chosen, key=lambda s: (ORDER.index(s) if s in ORDER else len(ORDER), s)):
        sc.parts.append(write_part(sc, sid, chosen[sid]))


def write_part(sc, sid, vid):
    base = dict(system=sid, variant=vid, name=sc.cat.systems[sid]["name"])
    why = NOT_WRITTEN_WHY.get(vid) or NOT_WRITTEN_WHY.get(sid)
    if why:
        return dict(base, mark=NOT_WRITTEN, fill=False, entries=[], reason=why)
    done = WRITERS[sid](sc, vid) if vid in CAN_WRITE.get(sid, ()) else None
    if done is None:
        return stub(sc, base, vid)
    entries, why = done
    return dict(base, mark=mark_of(sid, vid), entries=entries, reason=why)


def mark_of(sid, vid):
    return LAW if sid in NO_RESET_LAWS and vid in pd.SUPPORTED.get(sid, ()) else SIMULATED


def stub(sc, base, vid):
    v = sc.cat.variant(vid)
    defaults = {k: p["default"] for k, p in (v.get("parameters") or {}).items()}
    defaults.update(sc.d.over.get(vid, {}))
    entries, why = [], "the scaffold has no writer for this variant"
    if base["system"] == "S03":
        entries, _ = write_s03(sc, "S03.exponential")
        why += "; the model has a placeholder spine (the walls of the fit), so that the players have an axis"
    return dict(base, mark=NOT_WRITTEN, fill=True, entries=entries, defaults=defaults, reason=why,
                to_fill=stub_text(base["system"], v))


def stub_text(sid, v):
    head = (CANNOT_EXPRESS[sid] + "; with these defaults" if sid in CANNOT_EXPRESS else
            f"write '{v['family']}' as model lines and effects with these defaults")
    return head + ", mark it simulated, and fit again (references/simulation-path.md, 'Complete a stub')"


def scaffold(d):
    """The starting model of the selection: each part written or stubbed, with the key simulation_path."""
    sc = Scaffold(d)
    write_parts(sc)
    m = sc.m
    m["currencies"] = [dict(id=ECON, name="Main currency", system="s01", base_income=sc.base_income)] + \
        sc.more_currencies
    m["systems"] = [dict(id=sid.lower(), name=d.cat.systems[sid]["name"], loop="core") for sid in sorted(d.chosen)]
    m["game"] = dict(game_block(d), **m["game"])
    m["game"]["assumptions"] += sc.notes
    m["simulation_path"] = dict(parts=sc.parts, hand_set=sc.hand_set, to_add=to_add(sc.parts))
    if m["prestige"].get("enabled"):
        finish_reset(m, d.tv)
    return m


def no_walls(m):
    """A copy of m whose axis has no wall that the player passes and no end, so that the run goes on to its horizon."""
    out = copy.deepcopy(m)
    out["ladders"] = [open_ladder(lad) for lad in out["ladders"]]
    return out


def open_ladder(lad):
    req = {k: v for k, v in lad["requires"].items() if k != "table"}
    return dict(lad, count=100000, requires=dict(req, base=1e300))


NO_RESET_NOTE = "The model has no reset layer."
RESET_NOTE = ("S04.power: the model has a reset layer (written by the scaffold). The timing fit sets the walls by "
              "simulation; no law of the reset loop sets them on the simulation path.")


def finish_reset(m, tv):
    """q = (1 - R) / p with R the r sum of the uncapped mult lines (BM L1, L6); the lines of the written systems
    reset; c is the lifetime total of the main currency of active_greedy with no reset at run_min, so the first gain
    is 1 there."""
    pr = m["prestige"]
    m["game"]["assumptions"] = [RESET_NOTE if a.startswith(NO_RESET_NOTE) else a for a in m["game"]["assumptions"]]
    R = r_sum(m["purchases"])
    pr["reward"]["exponent"] = math.floor(1e4 * (1.0 - R) / pr["exponent"]) / 1e4 if R < 1.0 else 0.0
    pr["resets"]["purchase_systems"] = sorted({p["system"] for p in m["purchases"]})
    pr["base"] = first_divisor(m, tv["run_min"] * 60.0)


def r_sum(lines):
    return sum(math.log(f["value"]) / math.log(p["cost_growth"]) for p in lines for f in p["effects"]
               if f.get("kind") == "mult" and p.get("max_level") is None)


def first_divisor(m, t):
    """The lifetime total of the main currency of active_greedy with no reset at the time t (at least 1)."""
    m["prestige"]["base"] = 1e300
    sim = rb.run_active(pd.load(no_walls(m)), t / HOUR * 1.05)
    return float("%.4g" % max(1.0, rb.Curve(sim).sample(t)["life"].get(ECON, 1.0)))


def game_block(d):
    tv = d.tv
    targets = dict(pd.targets(d, tv["H"]), hard_end=dict(axis_step=1), largest_purchase_gap_minutes=tv["pgap"],
                   first_purchase_minutes=tv["pfirst"], largest_unlock_gap_minutes=tv["ugap_max"],
                   first_hour_unlock_gap_minutes=tv["ugap_first"], purchase_rate_max_per_minute=tv["rate_max"])
    if tv.get("demo_min"):
        targets["demo_end"] = dict(axis_step=1, minutes=tv["demo_min"], before_system=tv.get("demo_before"))
    session = tv["ugap_max"] if d.pattern == "continuous" else tv["run_min"]
    notes = ["pw_scaffold.py wrote this model from the selection: the simulation path (references/simulation-path.md). "
             "The key simulation_path marks each part as law, simulated or not written.",
             "The model has no reset layer. The walls, the unlock points and the first costs come from the timing fit "
             "by simulation, not from a law."]
    return dict(name="Scaffold of pw_scaffold.py", target_hours=tv["H"], session_minutes=session, progress_axis=AXIS,
                offline_rate=0.0, presence=dict(open_rate=1.0, visit_minutes=pd.VISIT_MINUTES), targets=targets,
                assumptions=notes, play_pattern=d.pattern, main_currency=ECON)


def to_add(parts):
    return [f"{p['system']} {p['variant']}: {p['to_fill']}" for p in parts if p.get("fill")]


def stage_times(tv):
    """The target time of each stage in seconds: each STAGE_SHARE x the first-hour unlock gap target in the first hour,
    then each STAGE_SHARE x the unlock gap target, and the last stage at the hard end (the length target)."""
    end = tv["H"] * HOUR
    g1, g = STAGE_SHARE * tv["ugap_first"] * 60.0, STAGE_SHARE * tv["ugap_max"] * 60.0
    out, t = [], g1
    while t < min(HOUR, end) - 1e-6:
        out.append(t)
        t += g1
    t = max(t, HOUR)
    while t < end - 0.5 * g:
        out.append(t)
        t += g
    return out + [end]


class FitSim(rb.LifeSim):
    """active_greedy that takes stage n of the axis at its target time, not by a wall. The step records the value of
    the spine stat at that time (Sim.steps). With these values as the walls, the model plays the same run while the
    stat rises (the method of the rebalance mode, pw_timing.StageSim)."""

    def __init__(self, m, hours, prof, player, times, axis):
        self.fit_times, self.fit_axis = times, axis
        super().__init__(m, hours, prof, player=player)

    def _due(self, l):
        n = self.ladder_pos[l["id"]] + 1
        return n <= min(l["count"], len(self.fit_times)) and self.t >= self.fit_times[n - 1] - 1e-6

    def _next_step_req(self, l, inc):
        if l["id"] != self.fit_axis:
            return super()._next_step_req(l, inc)
        return self.ladder_req(l, self.ladder_pos[l["id"]] + 1) if self._due(l) else None

    def _step_wait(self, l, inc):
        if l["id"] != self.fit_axis:
            return super()._step_wait(l, inc)
        n = self.ladder_pos[l["id"]] + 1
        return max(0.0, self.fit_times[n - 1] - self.t) if n <= len(self.fit_times) else float("inf")


def axis_of(m):
    return m["game"].get("progress_axis") or AXIS


def main_of(m):
    return m["game"].get("main_currency") or ECON


def active_spec():
    return next(p for p in pw_players.PLAYERS if p["name"] == "active_greedy")


def fit_run(m, times):
    lm = pd.load(m)
    pl, spec = pw_players.Players(lm, lm["game"]["target_hours"]), active_spec()
    prof = pw_players.reset_profile(lm, spec["reset"])
    return FitSim(lm, pl.horizon(spec), prof, pl.player(spec), times, axis_of(m)).play()


def active_run(m):
    """active_greedy of the players on the model as it is."""
    lm = pd.load(m)
    pl = pw_players.Players(lm, lm["game"]["target_hours"])
    return pl.run(active_spec())


def fit(m, tv):
    """The timing fit of a model with no reset: FIT_PASSES passes, each with 1 run of FitSim. Each pass sets the walls
    of the axis, the unlock point (a lifetime total) of each entry with place_minutes, and the first cost of each
    placed purchase. Returns the fitted model with simulation_path.fit (the target timeline against the simulation)."""
    m = copy.deepcopy(m)
    times = stage_times(tv)
    lad = next(l for l in m["ladders"] if l["id"] == axis_of(m))
    lad["count"] = len(times)
    set_ends(m, times, tv)
    for _ in range(FIT_PASSES):
        apply_pass(m, fit_run(m, times), lad, times)
    m = refine(m, times)
    m.setdefault("simulation_path", {})["fit"] = fit_rows(m, times, tv)
    return m


def refine(m, times):
    """REFINE_PASSES passes with the walls in play: each pass plays the model, scores the stage times against the
    timeline, and sets each wall to the spine stat of that run at the target time of its stage. Returns the model of
    the best score. Reason: a wall that a cap raise or a milestone passes early or late moves each later stage."""
    runs = [refine_pass(m, times)]
    for _ in range(REFINE_PASSES):
        runs.append(refine_pass(runs[-1][2], times))
    best = min(range(len(runs)), key=lambda i: runs[i][0])
    return runs[best][1]


def refine_pass(m, times):
    """(the score of m, m, a copy of m with each wall at the spine stat of the run of m at its target time)."""
    sim = active_run(m)
    nxt = copy.deepcopy(m)
    walls_at(nxt, sim, times)
    return stage_miss(sim, axis_of(m), times), m, nxt


def stage_miss(sim, ax, times):
    """The mean miss of the stage times in minutes, with the miss of the hard end counted 3 times."""
    got = {n: v[0] for n, v in sim.step_clock[ax].items()}
    miss = [abs(got[n] - t) if n in got else times[-1] for n, t in enumerate(times, 1)]
    return (sum(miss) + 2 * miss[-1]) / (len(miss) + 2) / 60.0


def walls_at(m, sim, times):
    """Each wall at the spine stat of sim at its target time. A run with no sample (each wall passed at the start)
    keeps the walls."""
    if not sim.samples:
        return
    lad = next(l for l in m["ladders"] if l["id"] == axis_of(m))
    cur = lad["requires"].get("currency", main_of(m))
    curve = rb.Curve(sim)
    vals = [curve.sample(t)["income"].get(cur) for t in times]
    lad["requires"]["table"] = rising(vals, lad["requires"].get("growth", 2.0))
    lad["requires"]["base"] = lad["requires"]["table"][0]


def set_ends(m, times, tv):
    t = m["game"]["targets"]
    t["hard_end"] = dict(axis_step=len(times))
    if tv.get("demo_min") and "demo_end" in t:
        t["demo_end"]["axis_step"] = max(1, sum(1 for x in times if x <= tv["demo_min"] * 60.0 + 1e-6))


def apply_pass(m, sim, lad, times):
    vals = [sim.steps[lad["id"]][n][1] for n in sorted(sim.steps[lad["id"]])]
    vals = (vals or [1.0]) + [None] * (len(times) - len(vals))
    lad["requires"]["table"] = rising(vals, lad["requires"].get("growth", 2.0))
    lad["requires"]["base"] = lad["requires"]["table"][0]
    curve = rb.Curve(sim)
    for x in placed(m):
        place(m, x, curve)


def rising(vals, growth):
    """The walls: each value at or above the one before it (a missing value grows by the ratio of the ladder)."""
    out = []
    for v in vals:
        last = out[-1] if out else 0.0
        out.append(float("%.6g" % (max(last, v if v is not None else last * growth) * WALL_SHADE)))
    return out


def placed(m):
    entries = m["purchases"] + m["milestones"] + (m.get("automation") or [])
    return [x for x in entries if "place_minutes" in x]


def place(m, x, curve):
    """The unlock point of 1 entry at its placed minute: the lifetime total of the main currency of the fit run then.
    An entry with place_minutes null takes the middle of the largest gap between 2 placed times of the first hour."""
    if x["place_minutes"] is None:
        x["place_minutes"] = free_minute(m)
    s = curve.sample(x["place_minutes"] * 60.0)
    cur = main_of(m)
    set_total(x, cur, max(1.0, s["life"].get(cur, 0.0)))
    place_cost(x, max(1.0, FIRST_COST_SECONDS * s["income"].get(cur, 0.0)))


def set_total(x, cur, amount):
    """The unlock point of x: 1 lifetime total of cur, in place of each lifetime total of cur that x had."""
    keep = [r for r in x.get("requires") or [] if not (r.get("type") == "total" and r.get("currency") == cur)]
    x["requires"] = keep + [dict(type="total", currency=cur, amount=float("%.4g" % amount))]


def place_cost(x, cost):
    """An auto-buy entry (key line) opens at its placed minute. A purchase gets the first cost, unless fit_cost is
    false (a cost that a person set by hand)."""
    priced = "cost_currency" in x and x.get("fit_cost", True)
    if "line" in x:
        x["from_minutes"] = x["place_minutes"]
    elif priced:
        x["base_cost"] = float("%.4g" % cost)


def free_minute(m):
    used = sorted({0.0, 60.0} | {x["place_minutes"] for x in placed(m) if x["place_minutes"] is not None})
    gaps = [(b - a, (a + b) / 2.0) for a, b in zip(used, used[1:])]
    return round(max(gaps)[1], 2)


def fit_rows(m, times, tv):
    """The target timeline against active_greedy of the fitted model: the first purchase, each stage, the hard end.
    misses: each target of the timeline outside its range, then the flat stages and the stages at 1 minute; causes:
    the probable cause of each miss (fit_causes)."""
    sim = active_run(m)
    got = {n: v[0] for n, v in sim.step_clock[axis_of(m)].items()}
    first = next((e["t"] for e in sim.events if e["kind"] == "purchase"), None)
    rows = [row("first purchase (latest)", tv["pfirst"], first)]
    rows += [row(f"stage {n}", t / 60.0, got.get(n)) for n, t in enumerate(times, 1)]
    end = got.get(len(times))
    end_h = None if end is None else round(end / HOUR, 3)
    flat = flat_stages(m)
    misses = timeline_misses(rows, tv, end_h, flat)
    return dict(passes=FIT_PASSES, rows=rows, hard_end_target_h=tv["H"], flat_stages=flat, hard_end_sim_h=end_h,
                stage_tolerance=STAGE_TOL, misses=misses, causes=fit_causes(m, sim, rows) if misses else [])


def timeline_misses(rows, tv, end_h, flat):
    """The texts of the targets of the timeline outside their range: the hard end outside the length tolerance, the
    first purchase after its latest minute, and each stage outside STAGE_TOL of its target minute. With 1 or more
    misses, also the flat stages and each group of stages at the same minute."""
    lo, hi = tv["H"] * (1.0 - tv["H_tol"]), tv["H"] * (1.0 + tv["H_tol"])
    out = [] if end_h is not None and lo - 1e-9 <= end_h <= hi + 1e-9 else [
        f"hard end {'not reached' if end_h is None else f'{end_h:.2f} h'} is outside the length range {lo:.2f} to "
        f"{hi:.2f} h (target {tv['H']:g} h, tolerance {tv['H_tol']:.0%})"]
    out += [row_miss(r) for r in rows if row_missed(r)]
    if not out:
        return []
    if flat:
        out.append(f"flat stages: {stages_text(flat)} with a wall not above the wall before it, so each such stage "
                   "comes with the stage before it")
    return out + same_minute(rows)


def row_missed(r):
    if r["sim_min"] is None:
        return True
    if r["what"].startswith("first purchase"):
        return r["sim_min"] > r["target_min"] + 1e-9
    return abs(r["sim_min"] - r["target_min"]) > STAGE_TOL * r["target_min"] + 1e-9


def row_miss(r):
    got = "not reached" if r["sim_min"] is None else f"at {r['sim_min']:g} min"
    if r["what"].startswith("first purchase"):
        return f"{r['what']}: {got}, the latest minute is {r['target_min']:g}"
    return f"{r['what']}: {got} against {r['target_min']:g} min (tolerance {STAGE_TOL:.0%})"


def same_minute(rows):
    """1 text for each group of 2 or more stages at the same simulated minute."""
    at = {}
    for r in stage_rows(rows):
        at.setdefault(r["sim_min"], []).append(stage_no(r))
    return [f"{stages_text(ns)} at the same minute {t:g}" for t, ns in sorted(at.items()) if len(ns) > 1]


def stage_rows(rows):
    return [r for r in rows if r["what"].startswith("stage ") and r["sim_min"] is not None]


def fit_causes(m, sim, rows):
    """The probable causes of the misses, from the run of active_greedy: a spine stat that falls (a stage comes
    early), and content that the run does not reach or that stops at its cap (a stage comes late or not at all)."""
    flat = flat_stages(m)
    early = [stage_no(r) for r in stage_rows(rows) if is_early(r) and stage_no(r) not in flat]
    late = [stage_no(r) for r in rows if r["what"].startswith("stage ") and row_missed(r)]
    late = [n for n in late if n not in early and n not in flat]
    out = [fall_cause(m, sim, early, rows)] if early else []
    return out + ([late_cause(m, sim, late, flat)] if late or flat else [])


def is_early(r):
    return r["sim_min"] < r["target_min"] * (1.0 - STAGE_TOL)


def stage_no(r):
    return int(r["what"][len("stage "):])


def stages_text(ns):
    return ("stage " if len(ns) == 1 else "stages ") + ", ".join(str(n) for n in ns)


def fall_cause(m, sim, early, rows):
    """The first fall of the spine stat before the last early stage: then the stat meets a wall before its time."""
    cur = spine_currency(m)
    t_max = max(r["target_min"] for r in rows if r["what"].startswith("stage ") and stage_no(r) in early) * 60.0
    drop = first_fall(sim, cur, t_max)
    names = stages_text(early)
    if drop is None:
        return f"{names}: early, because the wall of the fit is below the {cur} income at the target time"
    return (f"{names}: early, because the {cur} income falls from {drop[1]:.4g} to {drop[2]:.4g} near minute "
            f"{drop[0] / 60.0:g}, so the income meets the wall before its target time (a penalty such as the "
            "upkeep, or a cap, stops the rise of the spine stat)")


def spine_currency(m):
    lad = next(l for l in m["ladders"] if l["id"] == axis_of(m))
    return lad["requires"].get("currency", main_of(m))


def first_fall(sim, cur, t_max):
    """(time, value before, value after) of the first drop of the income of cur by more than FALL_SHARE in 1 step of
    CAUSE_STEP_S up to t_max, or None."""
    if not sim.samples:
        return None
    curve = rb.Curve(sim)
    pts = [i * CAUSE_STEP_S for i in range(int(t_max // CAUSE_STEP_S) + 1)]
    vals = [curve.sample(t)["income"].get(cur, 0.0) for t in pts]
    i = next((i for i in range(1, len(vals)) if vals[i] < vals[i - 1] * (1.0 - FALL_SHARE)), None)
    return None if i is None else (pts[i], vals[i - 1], vals[i])


def late_cause(m, sim, late, flat):
    """The placed entries that the run never buys or reaches, and the lines at their cap at the end of the run."""
    where = "; ".join(([stages_text(late) + ": late"] if late else []) + ([stages_text(flat) + ": flat walls"]
                                                                         if flat else []))
    parts = content_parts(m, sim)
    return f"{where}, because the income does not rise enough; " + (
        "; ".join(parts) if parts else "no content raises the income there; add content")


def content_parts(m, sim):
    bought = {e.get("id") for e in sim.events}
    gone = [f"{entry_id(x)} ({x['place_minutes']:g} min)" for x in placed(m)
            if x.get("place_minutes") is not None and entry_id(x) not in bought]
    capped = [p["id"] for p in m["purchases"] if at_cap(p, sim)]
    return ([f"the run never reaches or buys these placed entries: {', '.join(gone)}"] if gone else []) + \
        ([f"these lines are at their cap at the end of the run: {', '.join(capped)}"] if capped else [])


def entry_id(x):
    return x.get("id", x.get("line"))


def at_cap(p, sim):
    return p.get("max_level") is not None and sim.level.get(p["id"], 0) >= p["max_level"]


def flat_stages(m):
    """The stages whose wall is not above the wall before it: the income does not rise before them."""
    table = next(l for l in m["ladders"] if l["id"] == axis_of(m))["requires"]["table"]
    return [n for n in range(2, len(table) + 1) if table[n - 1] <= table[n - 2]]


def row(what, target_min, t):
    return dict(what=what, target_min=round(target_min, 2), sim_min=None if t is None else round(t / 60.0, 2))


def at_path(m, path):
    """(container, key) of a path of simulation_path.hand_set: the keys and indexes from the model root
    (references/model-schema.md). A text step in a list of objects is the id of an entry (or the line of an auto-buy
    entry). A step that does not resolve is a bad input: --vary must never vary a number that is not in the model."""
    pd.check(not isinstance(path, list) or not path, f"hand-set path {path!r}: a path is a list of keys and indexes")
    node = walk(m, path[:-1], path)
    key = step_key(node, path[-1], path)
    return node, key


def walk(node, keys, path):
    for k in keys:
        node = node[step_key(node, k, path)]
    return node


def step_key(node, k, path):
    """The key or index of node for the step k of path; an id step in a list becomes the index of the entry."""
    if isinstance(node, dict):
        pd.check(k not in node, f"hand-set path {path!r} does not resolve: no key {k!r}")
        return k
    pd.check(not isinstance(node, list), f"hand-set path {path!r} does not resolve: {k!r} is below a value")
    if isinstance(k, int):
        pd.check(not -len(node) <= k < len(node), f"hand-set path {path!r} does not resolve: no index {k}")
        return k
    hit = [i for i, x in enumerate(node) if isinstance(x, dict) and x.get("id", x.get("line")) == k]
    pd.check(not hit, f"hand-set path {path!r} does not resolve: no entry with the id {k!r}")
    return hit[0]


def measure(m):
    """(hard end hours, largest purchase gap minutes, largest unlock gap minutes) of active_greedy."""
    sim = active_run(m)
    end = sim.step_clock[axis_of(m)].get(m["game"]["targets"]["hard_end"]["axis_step"], (None,))[0]
    t_end = end if end is not None else sim.t
    buys = sorted(e["t"] for e in sim.events if e["kind"] == "purchase" and e["t"] <= t_end)
    return (None if end is None else end / HOUR), largest(buys, t_end), largest(unlock_times(m, sim, t_end), t_end)


def unlock_times(m, sim, t_end):
    """The times of the unlocks and of the stages up to t_end."""
    return sorted([e["t"] for e in sim.events if pw_proof.is_unlock(e) and e["t"] <= t_end] +
                  [v[0] for v in sim.step_clock[axis_of(m)].values() if v[0] <= t_end])


def largest(times, t_end):
    pts = [0.0] + times + [t_end]
    return max(b - a for a, b in zip(pts, pts[1:])) / 60.0


def holds(res, tv):
    end, pgap, ugap = res
    in_len = end is not None and abs(end - tv["H"]) <= tv["H_tol"] * tv["H"] + 1e-9
    return in_len and pgap <= tv["pgap"] + 1e-9 and ugap <= tv["ugap_max"] + 1e-9


def vary(m, tv):
    """Each hand-set number at -20% and +20%: rows (name, k, result, note) with the targets of the active player."""
    hand = (m.get("simulation_path") or {}).get("hand_set") or []
    return [("as fitted", None, measure(m), "")] + [vary_row(m, h, k) for h in hand for k in VARY]


def vary_row(m, h, k):
    mm = varied(m, h["path"], k)
    return h["name"] + bonus_tag(m, h["path"]), k, measure(mm), change_note(m, mm, h["path"])


def is_ratio(node, key):
    """True for a number whose part above 1 is the bonus: the value of a mult effect (or a mult capacity step), and a
    cost ratio."""
    return (key == "value" and node.get("kind") == "mult") or key in RATIO_KEYS


def bonus_tag(m, path):
    node, key = at_path(m, path)
    return " bonus" if is_ratio(node, key) else ""


def varied(m, path, k):
    """A copy of m with the number at path changed by k: the bonus part (value - 1) of a ratio, else the value."""
    mm = copy.deepcopy(m)
    node, key = at_path(mm, path)
    v = node[key]
    pd.check(isinstance(v, bool) or not isinstance(v, (int, float)), f"hand-set path {path!r} is not a number: {v!r}")
    node[key] = 1.0 + (v - 1.0) * k if is_ratio(node, key) else v * k
    return mm


def change_note(m, mm, path):
    """The value before and after, and each line that the change moves out of its proven range."""
    return "; ".join([f"{value_at(m, path):.6g} -> {value_at(mm, path):.6g}"] + range_notes(m, mm))


def value_at(m, path):
    node, key = at_path(m, path)
    return node[key]


def range_notes(m, mm):
    """The range rows of the purchase lines (pw_proof) that mm has and m has not."""
    before = {r["id"] for r in line_range_rows(m)}
    return [f"{r['id'][len('range: '):]} {r['value']} is outside the proven range {r['limit']}"
            for r in line_range_rows(mm) if r["id"] not in before]


def line_range_rows(m):
    lm = pd.load(copy.deepcopy(m))
    reset = (lm.get("prestige") or {}).get("currency") if (lm.get("prestige") or {}).get("enabled") else None
    x = dict(econ=main_of(lm), reset_cur=reset)
    return pw_proof.range_rows([o for p in lm["purchases"] for o in pw_proof.line_params(p, x)])


def no_effect(rows):
    """The names whose 2 changes give the result of the model as fitted: the number has no effect on the targets of
    the active player (true when, for example, the line reaches its cap early)."""
    base = rows[0][2]
    names = {r[0] for r in rows[1:]}
    return {n for n in names if all(r[2] == base for r in rows[1:] if r[0] == n)}


def vary_lines(rows, tv):
    out = [f"targets: length {tv['H']:g} h within {tv['H_tol']:.0%}, purchase gap <= {tv['pgap']:g} min, unlock gap "
           f"<= {tv['ugap_max']:g} min"]
    flat = no_effect(rows)
    return out + [vary_line(r, tv, r[0] in flat) for r in rows]


def vary_line(r, tv, flat):
    name, k, res, note = r
    tag = "" if k is None else f" x{k:g}"
    note = f" ({note}{'; no effect on these targets' if flat else ''})" if note else ""
    return f"VARY {name}{tag}{note}: {result_text(res)}: " + ("holds" if holds(res, tv) else "MISSES")


def result_text(res):
    end, pgap, ugap = res
    end_t = "not reached" if end is None else f"{end:.2f} h"
    return f"hard end {end_t}, purchase gap {pgap:.2f} min, unlock gap {ugap:.2f} min"


def design_of(sel_path, tg_path):
    """The Design of pw_design.py for a selection and a targets file (the checks of the solver, step 2)."""
    a = argparse.Namespace(selection=sel_path, targets=tg_path, content=None)
    return pd.prepare(a, pd.Catalog())


def targets_of(path):
    tg = pd.read_json(path, "targets")
    pd.pattern_of(tg)
    return pd.target_values(tg)


def read_model(path):
    """The model JSON of a path, as a dict. A file that does not parse is a bad input (exit code 2)."""
    return pd.read_json(path, "model")


def parse(argv):
    ap = argparse.ArgumentParser(description="The simulation path: a starting model and the timing fit by simulation.")
    ap.add_argument("files", nargs="+", help="<selection.json> <targets.json>, or <targets.json> with --fit or --vary")
    ap.add_argument("--out", metavar="MODEL", help="write the model JSON")
    ap.add_argument("--fit", metavar="MODEL", help="fit a model that a person completed by hand")
    ap.add_argument("--vary", metavar="MODEL", help="change each hand-set number by -20%% and +20%%")
    ap.add_argument("--no-fit", action="store_true", help="write the scaffold without the timing fit")
    return ap.parse_args(argv)


def run(a):
    if a.vary:
        return run_vary(a)
    pd.check(not a.out, "--out is necessary")
    m = fit(read_model(a.fit), targets_of(a.files[-1])) if a.fit else new_model(a)
    pd.load(m)
    pd.write_model(a.out, m)
    pw_proof.print_lines(summary_lines(m, a.out))
    return 1 if (m["simulation_path"].get("fit") or {}).get("misses") else 0


def run_vary(a):
    """--vary: exit code 1 when 1 or more changes miss a target."""
    tv = targets_of(a.files[-1])
    lines = vary_lines(vary(read_model(a.vary), tv), tv)
    pw_proof.print_lines(lines)
    return 0 if all(line.endswith("holds") for line in lines[1:]) else 1


def new_model(a):
    """The scaffold of the selection, fitted unless --no-fit."""
    pd.check(len(a.files) != 2, "the scaffold needs the selection file and the targets file")
    d = design_of(*a.files)
    m = scaffold(d)
    return m if a.no_fit else fit(m, d.tv)


def summary_lines(m, path):
    sp = m["simulation_path"]
    out = [f"pw_scaffold: the model is in {path}"]
    out += [f"  PART {p.get('variant') or p.get('name')}: {p['mark']}" + (" (fill by hand)" if p.get("fill") else "")
            for p in sp.get("parts") or []]
    if sp.get("fit"):
        out.append(f"  FIT hard end: target {sp['fit']['hard_end_target_h']:g} h, simulated "
                   f"{sp['fit']['hard_end_sim_h']} h")
        out += [f"  FIT MISS {x}" for x in sp["fit"].get("misses") or []]
        out += [f"  FIT CAUSE (probable) {x}" for x in sp["fit"].get("causes") or []]
    return out


def main(argv=None):
    a = parse(argv)
    try:
        return run(a)
    except pd.InputError as e:
        return pd.input_error(e)


if __name__ == "__main__":
    sys.exit(main())
