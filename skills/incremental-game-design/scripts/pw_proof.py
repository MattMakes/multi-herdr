#!/usr/bin/env python3
"""The proof sheet: the 13 proof obligations of references/balance-math.md as a computed table.

    python3 pw_proof.py <model.json> [--json FILE] [--speed] [--strict-ranges]
    python3 pw_proof.py --systems <systems-out.json> [--pattern continuous|check_in] [--R VALUE] [--json FILE]
                        [--strict-ranges]

The first form reads a model (references/model-schema.md). It computes the law values with the closed
formulas, and it runs the players of the simulator for the measured values. The second form reads the
catalog map of scripts/scan_godot.py --systems-out. It computes only what the map permits.

references/proof-sheet.md gives the source of each value and each rule of the status.
Exit code: 0 when the sheet is ready, 1 when it is not ready, 2 for a usage error or a model error.
"""
import argparse
import collections
import copy
import itertools
import json
import math
import os
import statistics
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import pw_fun  # noqa: E402
import pw_players  # noqa: E402
from pw_analysis import main_currency  # noqa: E402
from pw_model import cost_at, load_model, presence_of, prestige_on  # noqa: E402
from pw_sim import Sim, step_amount, upkeep_factor, upkeep_lines  # noqa: E402

SCHEMA_VERSION = 1
HOUR = 3600.0
X_E, X_P, DG_DEFAULT = 1.0, 10.0, 2.0          # the reset rules of the players and the default gap (systems.json T5)
D_H = pw_players.HUMAN_DECISION                # seconds of real time for each human decision (L9)
SPEED_FACTOR = 10.0                            # PO13: T_X >= 10 x k x D_h
DEFAULT_SPEED = pw_players.DEFAULT_PLAY_TEST_SPEED
DIFF_LIMIT = 0.20                              # a larger difference of math and simulation is a finding
TX_TOLERANCE = 0.20                            # PO5
LENGTH_TOLERANCE = 0.15                        # PO12
LAMBDA_ONE = 0.05                              # L4b needs |lambda - 1| <= this
EPS = 1e-6
FIRST_COST_SECONDS = 15.0                      # PO11: a_1 <= 15 x I_0
FIRST_WINDOW = 300.0                           # PO11: the first 5 minutes
FIRST_UNLOCKS = 3
LARGEST_GAP_RATIO = 20.0 / 15.0                # PO10: largest gap after hour 1 <= 20 min for 15 min sessions (T2)
ABSENCE_HOURS = 24.0                           # PO9: the shortest "longest absence that must pay" (L5)
A_SECONDS = 1e6                                # length of the no-reset run that measures A (L2 trajectory)
LONG_STEP_TOL = 0.05                           # 1 long run with a generator carrier: |R + 1 / (k_mid ln b) - 1| <= this
LONG_R_TOL = 0.01                              # 1 long run: |R after the caps - 1| <= this (BM L2 with R = 1)
GROWTH_FROM_HOURS = 1.0                        # 1 long run: the fit of g starts at hour 1, after the first stages
SPEED_GD = 0.1                                 # L9 property: g k D_h <= 0.1 keeps the slow-down near 1.05
IDLE_HOURS = pw_players.IDLE_HOURS
IDLE_STATES = ("open", "closed")
IDLE_PLAYERS = tuple(f"{s}_{h}h" for s in IDLE_STATES for h in IDLE_HOURS)
PLAYERS = ("active_greedy", "early_reset", "patient_reset") + IDLE_PLAYERS
SYSTEMS_JSON = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "assets", "systems.json")
UNCOVERED = "not covered by L1"
IN_RANGE = "pass (in range)"                   # a target row past its limit and inside the range of the target
READY_STATUSES = ("pass", IN_RANGE, "not applicable")
TOLERANCE = {"no source": 0.20, "sourced": 0.05}     # the range of a target limit, by the source of the target
TARGET_SOURCE = {      # the source class of each target limit that has a range (references/proof-sheet.md)
    "largest_purchase_gap_minutes": "sourced", "first_purchase_minutes": "sourced",
    "largest_unlock_gap_minutes": "no source", "first_hour_unlock_gap_minutes": "no source",
    "ending_unlock_gap_minutes": "no source", "purchase_rate_max_per_minute": "no source",
    "purchase_value": "no source", "content_gap": "no source"}
SIMULATED = "simulated, not proved"            # a row whose law does not cover the model: the simulation meets the limit
STUB = "stub: fill by hand"                    # a part of simulation_path that the model does not hold yet
NOT_MODELLED = "not modelled"                  # a part of simulation_path with no effect on the economy of the model
RESULTS = ("ready", "simulated", "not ready")  # the result of the sheet (references/proof-sheet.md, "The result")
EXIT_CODES = {"ready": 0, "not ready": 1, "simulated": 3}
RESULT_TEXT = {
    "ready": "each row that decides the verdict passes by a law or inside its target: the design can go to a build",
    "simulated": "each row passes or is simulated, not proved, and no part is a stub: the design can go to a build, "
                 "and a play test must confirm the simulated rows",
    "not ready": "a row fails, a row is not computed, or a part is a stub: do not build from this design"}
READY_KINDS = ("obligation", "pattern", "purchase", "upkeep", "content")   # the row kinds that decide the verdict
LINE_LIFE_SHARE = 0.5                          # row CL: a generator line lives to this share of the length; a designer
                                               # rule with no source
NO_OFFLINE = "no offline gain by design"
NO_RESET = "the model has no reset layer"     # PO2 to PO5: the rows of the reset loop (not applicable)
CLOSED_PARTS = ("PO6", "PO7", "PO8", "PO9")    # the obligations with a closed-game part
PURCHASE_GAP_MINUTES = 2.0                     # PC2: CT3 of assets/systems.json (SG:181), single source
FIRST_PURCHASE_MINUTES = 2.0                   # PC2: CT2 of assets/systems.json (SG:180)
UNLOCK_GAP_MINUTES = 10.0                      # PC4: the whole length; designer target, no source (CT6)
UNLOCK_GAP_FIRST_HOUR_MINUTES = 5.0            # PC4: the first hour; designer target, no source (CT6)
PART_HOURS = 1.0                               # PC3: the length of 1 part past the demo; no source, a sheet default
PURCHASE_FLOOR = 0.10                          # PV: designer target; the low end of the proven S02 step (d 1.1)
PURCHASE_FLOOR_SOURCE = "designer target; default 10%: the low end of the proven step d 1.1 to 2 of " \
                        "S02.levelled_factor (assets/systems.json, O:188, D:195)"
PATTERN_ROWS = (
    ("PC1", "S28", "The active player reaches the hard end inside the length target"),
    ("PC2", "CT2, CT3", "The largest gap between 2 purchases of the active player is at most the gap target, and "
                        "the first purchase comes inside the first-purchase target"),
    ("PC3", "CT5", "Each part of the game past the demo end adds new content: a first purchase of a line or a "
                   "milestone, not only an axis step"),
    ("PC4", "CT6", "The largest gap between 2 unlocks of the active player (an axis step, a first purchase or a "
                   "milestone) is at most the unlock gap target over the whole length and in the first hour"),
    ("PC5", "reachable", "The active player buys each purchase line and reaches each milestone before the hard end"),
    ("PC6", "purchase rate", "The purchases of the active player in each minute stay at or below the purchase rate "
                             "target; the automation of the model (S24 auto-buy) takes the rest"),
)
PURCHASE_RATE_MAX = 30                         # PC6: designer target; the most purchases in 1 minute with no automation
RATE_MARGIN = 0.2       # PC6: the automation plan stops at (1 - margin) x the target, so the game has room (33 > 30)

OBLIGATIONS = (
    ("PO1", "L2", "law", "R < 1 in each run phase that has a reset layer. R = 1 only with no uncapped additive line"),
    ("PO2", "L6", "law", "The reset loop is stable: lambda = p q / (1 - R) <= 1, and lambda = 1 for linear progress"),
    ("PO3", "L6", "law", "A best reset exists: s = p / (1 - R) > 1"),
    ("PO4", "L6", "law", "The gap between the patient and the early reset rule is at most Dg"),
    ("PO5", "L6, L8", "law", "The run length T_X = X^(1/s) (c / A)^(1/alpha) is within 20% of the session sigma"),
    ("PO6", "L4", "target", "The idle value rho(T) of L4b is in its band at 1 h, 8 h, 24 h and 7 d, open and closed"),
    ("PO7", "L4", "law", "Idle stays below active for each T: p kappa X^(1/p) < e ln(1 + X)"),
    ("PO8", "L4", "law", "No idle state pays more than active play: f_open <= 1 and f_closed <= 1"),
    ("PO9", "L5", "law", "A presence tool limits open idle, and no open-idle cap is below the longest absence that "
                         "must pay"),
    ("PO10", "L3, L8", "target", "1 unlock for each session after the first hour: 1 / u <= sigma, with "
                                 "w = e^(alpha ldot / u)"),
    ("PO11", "L8", "target", "The first cost a_1 <= 15 I_0, and at least 3 unlocks in the first 5 minutes. An unlock "
                             "is a new axis step, the first purchase of a line, or a milestone, after t = 0"),
    ("PO12", "L8", "target", "The axis count N gives the length H within 15%"),
    ("PO13", "L9", "target", "The play-test speed k keeps T_X >= 10 k D_h"),
)
STATEMENT = {pid: (law, kind, text) for pid, law, kind, text in OBLIGATIONS}

INPUT_TEXT = {
    "R_max": "R (the line exponents of the economy)", "R_end": "R after the caps", "p": "p (the gain exponent)",
    "q": "q (the reward exponent)", "A": "A (the L2 trajectory: it needs a simulation of the model)",
    "c": "c (the reset divisor)", "sigma": "sigma (the session length of the model)",
    "T_X": "T_X (the run length: it needs A and sigma)", "f_open": "f_open (the open income share)",
    "f_closed": "f_closed (the closed income share)", "kappa": "kappa (it needs T_X and f)",
    "w": "w (the wall ratio of the axis)", "N": "N (the axis count)", "H": "H (game.targets.length_hours)",
    "I0": "I_0 (the base income)", "a1": "a_1 (the first cost)", "u_model": "u (the unlock rate: it needs T_X)",
    "tools": "the presence tools of the game",
}
PARAM_USES = {"econ_line": "PO1 PO2 PO3 PO4 PO5", "reward_line": "PO2 PO4 PO5", "p": "PO2 PO3 PO4 PO5 PO6 PO7",
              "g": "PO10 PO12", "f_closed": "PO6 PO8", "T_cap_h": "PO6", "other": ""}
USAGE_ERRORS = ("--R needs --systems", "--speed needs a model, not --systems",
                "--pattern needs --systems: a model has game.play_pattern")


# ----------------------------------------------------------------------------- small helpers
def fmt(v, nd=3):
    if v is None:
        return "n/a"
    if isinstance(v, float) and math.isinf(v):
        return "no limit"
    return f"{v:.{nd}g}" if isinstance(v, float) else str(v)


def mid(band):
    return None if not band else (band[0] + band[1]) / 2.0


def missing(x, keys):
    return [k for k in keys if x.get(k) is None]


def part(value, num=None, ok=None, limit="", fix="", reason=""):
    """1 half of an obligation: the formula half or the simulation half."""
    return dict(value=value, num=num, ok=ok, limit=limit, fix=fix, reason=reason, na=False)


def not_computed(miss):
    return part("n/a", reason="missing input: " + ", ".join(INPUT_TEXT.get(k, k) for k in miss))


def no_formula(reason):
    return part("n/a", reason=reason)


def not_applicable(reason):
    out = part("n/a", reason=reason)
    out["na"] = True
    return out


def rel_diff(a, b):
    if a is None or b is None or math.isinf(a) or math.isinf(b):
        return None
    return abs(a - b) / max(abs(a), 1e-12)


def unstable(x):
    """True when the reset loop has lambda > 1 (or R >= 1). Runs then get shorter with no limit (L6)."""
    if x.get("R_max") is not None and x["R_max"] >= 1:
        return True
    return x.get("lam") is not None and x["lam"] > 1 + EPS


def loop_known(x):
    return not missing(x, ("R_max", "p", "q"))


_RANGES = {}


def proven_ranges():
    """{variant id: variant} of assets/systems.json, read 1 time."""
    found = _RANGES
    if not found:
        found.update(_load_variants())
    return found


def _load_variants():
    with open(SYSTEMS_JSON, encoding="utf-8") as f:
        data = json.load(f)
    return {v["id"]: v for s in data["systems"] for v in s["variants"]}


def range_of(variant, name):
    v = proven_ranges().get(variant) or {}
    prm = (v.get("parameters") or {}).get(name)
    return (prm["min"], prm["max"]) if prm and v.get("sourced_range") else None


def range_note(variant, name, value):
    rng = range_of(variant, name)
    if rng is None or rng[0] - EPS <= value <= rng[1] + EPS:
        return ""
    return f" (outside the proven {name} range {fmt(rng[0])} to {fmt(rng[1])} of {variant})"


# ----------------------------------------------------------------------------- target ranges
def tolerance_of(x, key):
    """(tolerance, source class) of a target limit: game.targets.tolerances[key], else the default of its source
    class (TOLERANCE). A target that the model marks unsupported is no source."""
    cls = "no source" if key in (x.get("unsupported") or ()) else TARGET_SOURCE[key]
    return float((x.get("tolerances") or {}).get(key, TOLERANCE[cls])), cls


def edge(limit, tol, low=False):
    """The end of the range of a target limit: limit x (1 + tol), or limit x (1 - tol) for a floor."""
    return limit * (1.0 - tol if low else 1.0 + tol)


def zone(value, limit, tol, low=False):
    """pass inside the limit, IN_RANGE inside the range of the target, else fail. low: the limit is a floor."""
    sign = -1.0 if low else 1.0
    if sign * (value - limit) <= EPS:
        return "pass"
    return IN_RANGE if sign * (value - edge(limit, tol, low)) <= EPS else "fail"


def worst(zones):
    return next((z for z in ("fail", IN_RANGE) if z in zones), "pass")


def about(x, key, limit, unit, low=False):
    """The text of a target limit with its range: "about 10 min (range to 12 min)"."""
    tol, _ = tolerance_of(x, key)
    return f"about {fmt(limit)} {unit} (range to {fmt(edge(limit, tol, low), 3)} {unit})"


def miss_text(x, key, value, limit, unit, low=False):
    """The text of a value in the range zone: the value, the target, its source class and the end of the range."""
    tol, cls = tolerance_of(x, key)
    return (f"{value} {unit} against a target of about {fmt(limit)} {unit} ({cls}), inside the range to "
            f"{fmt(edge(limit, tol, low), 3)} {unit}")


def zone_note(x, key, num, limit, unit, text=None, low=False):
    """[the miss text] when num is in the range zone of the limit, else []."""
    tol, _ = tolerance_of(x, key)
    if zone(num, limit, tol, low) != IN_RANGE:
        return []
    return [miss_text(x, key, text or f"{num:.2f}", limit, unit, low)]


def zone_of(x, key, num, limit, low=False):
    return zone(num, limit, tolerance_of(x, key)[0], low)


# ----------------------------------------------------------------------------- model inputs
def econ_of(m):
    """The economy currency: the basis currency of the reset, else game.main_currency (the main currency of the
    content file), else the cost currency with the most purchases."""
    pr = m.get("prestige") or {}
    if prestige_on(m) and pr.get("basis_currency"):
        return pr["basis_currency"]
    return (m.get("game") or {}).get("main_currency") or main_currency(m)


def mult_on(p, targets):
    vals = [e["value"] for e in p["effects"] if e["kind"] == "mult" and e["target"] in targets and e["value"] > 1]
    return max(vals, default=None)


def adds_to(p, cur):
    return any(e["kind"] in ("add", "pct") and e["target"] in (cur, "all") for e in p["effects"])


def line_row(p, d):
    c = p["costs"][0]
    r = math.log(d) / math.log(c["growth"]) if c["formula"] == "exp" and c["growth"] > 1 else 0.0
    return dict(id=p["id"], r=r, cap=p["max_level"], b=c["growth"], d=d, a1=c["base"])


def reset_line_constant(lines):
    """K of BM D8: k levels of a line paid in the reset currency cost a1 (b^k - 1) / (b - 1), so a spend of L buys
    d^k = (1 + (b - 1) L / a1)^r, about ((b - 1) / a1)^r L^r. K is the product of ((b - 1) / a1)^r of the uncapped
    lines. The run total holds K^alpha, so the run length is T_X / K. Domain: (b - 1) L / a1 >> 1, after the first
    resets."""
    return math.prod(((ln["b"] - 1.0) / ln["a1"]) ** ln["r"] for ln in lines
                     if ln["cap"] is None and ln["r"] > 0 and ln.get("a1"))


def step_factor(p, targets):
    """The factor of 1 level of an additive line with count steps: m^(1 / n), so r_eff = ln m / (n ln b) (BM 3a,
    "The generator as a carrier"). None with no count step on the targets."""
    steps = p.get("count_steps")
    on = steps and any(e["kind"] == "add" and e["target"] in targets for e in p["effects"])
    return steps["mult"] ** (1.0 / steps["every"]) if on and steps["mult"] > 1 else None


def lines_paid(m, cost_cur, target):
    """The carriers paid in cost_cur on target, each with its r (D5): the mult lines, and the additive lines with
    count steps (r_eff)."""
    tg = (target, "all")
    pairs = [(p, mult_on(p, tg) or step_factor(p, tg)) for p in m["purchases"] if p["costs"][0]["currency"] == cost_cur]
    return [dict(line_row(p, d), steps=bool(p.get("count_steps"))) for p, d in pairs if d]


def reward_q(m, econ):
    """q: the reward exponent of prestige.reward plus r of each uncapped line paid in the reset currency plus the
    share of the factors that a count of resets opens (held_q)."""
    pr = m["prestige"]
    rw = pr.get("reward") or {}
    base = rw.get("exponent", 0.0) if rw.get("target") in (econ, "all") else 0.0
    lines = lines_paid(m, pr["currency"], econ)
    return base + sum(ln["r"] for ln in lines if ln["cap"] is None) + held_q(m, econ), lines


def held_q(m, econ):
    """ln F / (n ln(1 + X)) (BM L6, "Factors at a count of resets"): F is the product of the factors on econ of each
    entry that requires a count of resets (mult value^levels, 1 + the sum of pct value x levels), n is the largest
    such count and X the reset rule. A line paid in the reset currency or with no cap is not such an entry: its r is
    in q already (reward_q), or it has no levels to count."""
    items = [(x, r["amount"]) for x in reset_count_entries(m) for r in x.get("requires") or []
             if r.get("type") == "prestiges"]
    fx = [(f, x.get("max_level") or 1) for x, _ in items for f in x.get("effects") or [] if f.get("target") == econ]
    F = levels_factor(fx)
    if F <= 1.0:
        return 0.0
    x_rule = m["prestige"].get("policy_ratio") or 1.0
    return math.log(F) / (max(n for _, n in items) * math.log(1.0 + x_rule))


def reset_count_entries(m):
    """The milestones, and the capped purchases not paid in the reset currency (the factors that held_q counts)."""
    cur = m["prestige"].get("currency")
    buys = [x for x in m.get("purchases") or [] if x.get("cost_currency") != cur and x.get("max_level")]
    return list(m.get("milestones") or []) + buys


def levels_factor(fx):
    """The product of each mult value^levels times 1 + the sum of each pct value x levels, of (effect, levels)."""
    mult = math.prod(f["value"] ** n for f, n in fx if f["kind"] == "mult")
    return mult * (1.0 + sum(f["value"] * n for f, n in fx if f["kind"] == "pct"))


def gain_exponent(pr, m=None):
    if pr.get("basis_currency") and pr.get("formula", "power") == "power":
        return pr["exponent"], pr["base"] * pr["k"] ** (-1.0 / pr["exponent"]) if pr["k"] > 0 else None
    return ladder_gain_exponent(pr, m)


def ladder_gain_exponent(pr, m):
    """A gain k g^n on the stage n of a ladder whose step n needs base w^n: n = ln(E / base) / ln w, so the gain is
    k (E / base)^p with p = ln g / ln w and c = base k^(-1/p) (BM L6 in the form of a power gain)."""
    req = basis_requirement(pr, m)
    g, w, k = pr.get("growth") or 0.0, req.get("growth") or 0.0, pr.get("k", 1.0)
    if pr.get("formula") != "exp" or min(g, w) <= 1.0 or not req.get("base") or k <= 0:
        return None, None
    p = math.log(g) / math.log(w)
    return p, req["base"] * k ** (-1.0 / p)


def basis_requirement(pr, m):
    """The requirement of the basis ladder of a prestige, or {}."""
    lad = next((l for l in (m or {}).get("ladders") or [] if l["id"] == pr.get("basis_ladder")), None)
    return (lad or {}).get("requires") or {}


def economy_inputs(m, x):
    econ = x["econ"]
    lines = lines_paid(m, econ, econ)
    x["econ_lines"] = lines
    x["R_max"] = carrier_sum(lines)
    x["R_end"] = carrier_sum([ln for ln in lines if ln["cap"] is None])
    x["uncapped_additive"] = [p["id"] for p in m["purchases"]
                              if p["costs"][0]["currency"] == econ and adds_to(p, econ) and p["max_level"] is None]
    x["step_lines"] = [ln for ln in lines if ln.get("steps") and ln["cap"] is None]


def carrier_sum(lines):
    """R of the lines (L1): the sum of r of the mult lines, plus the largest r_eff of the additive lines with count
    steps. Those lines add in 1 group (D6), and a group acts as 1 line with its largest r."""
    return sum(ln["r"] for ln in lines if not ln.get("steps")) + max([ln["r"] for ln in lines if ln.get("steps")], default=0.0)


def reset_inputs(m, x):
    x["has_reset"] = prestige_on(m) and "run_seconds" not in m["prestige"]
    x["p"] = x["c"] = x["q"] = None
    x["reward_lines"] = []
    x["K_q"] = 1.0
    if x["has_reset"]:
        x["p"], x["c"] = gain_exponent(m["prestige"], m)
        x["q"], x["reward_lines"] = reward_q(m, x["econ"])
        x["K_q"] = reset_line_constant(x["reward_lines"])
        x["min_run"] = m["prestige"].get("min_run_seconds", 0)


def start_sim(m):
    sim = Sim(m, 1.0, {})
    sim.check_milestones()
    return sim


def first_cost(m, econ):
    open_lines = [p for p in m["purchases"] if not p.get("requires") and p["costs"][0]["currency"] == econ]
    costs = sorted((cost_at(p, 0), p["id"]) for p in open_lines)
    return costs[0] if costs else (None, None)


def presence_inputs(m, x):
    g = m["game"]
    pres = presence_of(g)
    sim = start_sim(m)
    full, away = sim.income()[x["econ"]], sim.away_income()[x["econ"]]
    frac = away / full if full > 0 else 1.0
    x.update(I0=full, frac=frac, f_open=pres["open_rate"] * frac, f_closed=g["offline_rate"] * frac,
             V=pres["visit_minutes"] * 60.0, open_cap_known=True, closed_cap_h=_hours(pres["offline_cap_minutes"]),
             open_cap_h=_hours(pres["open_cap_minutes"]))
    x["no_offline"] = x["f_closed"] <= EPS
    x["a1"], x["a1_line"] = first_cost(m, x["econ"])
    x["tools"] = presence_tools(m, x)
    x["late_active"] = late_active(m, x["econ"])


def _hours(minutes):
    return None if minutes is None else minutes / 60.0


def presence_tools(m, x):
    tools = ["open income share f_open = %s < 1 (scale tool)" % fmt(x["f_open"])] if x["f_open"] < 1 - EPS else []
    tools += ["open-idle cap %s h" % fmt(x["open_cap_h"])] if x["open_cap_h"] is not None else []
    tools += ["bank cap on %s" % c["id"] for c in m["currencies"] if "bank_cap" in c]
    tools += ["ladder %s needs a visit or has a timer" % l["id"] for l in m["ladders"]
              if any(k in l for k in ("attempt_seconds", "cooldown_seconds", "needs_visit"))]
    return tools


def spine_inputs(m, x):
    ax = pw_players.axis_of(m)
    x["axis"] = ax
    lad = next((l for l in m["ladders"] if l["id"] == ax), None)
    if lad is None:
        x.update(w=None, N=None, spine_income=False, spine_req=None)
        return
    req = lad["requires"]
    x.update(w=req.get("growth"), N=lad["count"], spine_req=req, spine_cur=req.get("currency"),
             spine_income=req["type"] == "rate" and req.get("currency") == x["econ"], kappa_y=1.0)
    second_income(m, x, req)


def second_income(m, x, req):
    """BM L10: the axis tests the income of a second currency y that lines paid in the economy currency raise. ln of
    the income of y rises R_y times as fast as ln of the economy power, so kappa_y = R_y (the r of the uncapped
    lines paid in the economy currency on y). kappa_y = 0 (no such line) leaves the formulas out."""
    y = req.get("currency")
    if req["type"] != "rate" or y == x["econ"] or not any(c["id"] == y for c in m["currencies"]):
        return
    ry = sum(ln["r"] for ln in lines_paid(m, x["econ"], y) if ln["cap"] is None)
    x.update(kappa_y=ry, R_y=ry, spine_income=ry > 0)


def target_inputs(m, x):
    t = m["game"].get("targets") or {}
    x["Dg"] = (t.get("policy_gap") or [0, DG_DEFAULT])[1]
    x["k"] = t.get("play_test_speed", DEFAULT_SPEED)
    x["H"] = mid(t.get("length_hours"))
    x["u_band"] = t.get("unlocks_per_hour")
    x["idle_targets"] = t.get("idle") or {}
    x["sigma"] = m["game"]["session_minutes"] * 60.0
    open_hours = [float(h) for h in (x["idle_targets"].get("open") or {})]
    x["absence_h"] = max([ABSENCE_HOURS] + open_hours)
    x["pattern"] = m["game"].get("play_pattern")
    x.update(gate_targets(t))
    element_inputs(m, x)


GATE_TARGETS = (("gap_max", "largest_purchase_gap_minutes", PURCHASE_GAP_MINUTES),
                ("first_max", "first_purchase_minutes", FIRST_PURCHASE_MINUTES),
                ("part_h", "extension_part_hours", PART_HOURS),
                ("ugap_max", "largest_unlock_gap_minutes", UNLOCK_GAP_MINUTES),
                ("ugap_first", "first_hour_unlock_gap_minutes", UNLOCK_GAP_FIRST_HOUR_MINUTES),
                ("ugap_ending", "ending_unlock_gap_minutes", None), ("length_band", "length_hours", None),
                ("hard_end", "hard_end", None), ("demo_end", "demo_end", None))


def gate_targets(t):
    """The targets of the pattern rows: each key of GATE_TARGETS with its default, the tolerances of the targets and
    the targets that the model marks unsupported."""
    out = {name: t.get(key, default) for name, key, default in GATE_TARGETS}
    return dict(out, tolerances=t.get("tolerances") or {}, unsupported=t.get("unsupported") or [])


def element_inputs(m, x):
    """The inputs of PC5 and PC6: each purchase line and milestone with its requirements, the automation of the
    model, and the purchase rate target."""
    x["elements"] = [(p["id"], "purchase", p.get("requires") or []) for p in m["purchases"]] + \
        [(ms["id"], "milestone", ms.get("requires") or []) for ms in m.get("milestones") or []]
    x["empty_slots"] = empty_slots(m)
    x["generators"] = [p["id"] for p in m["purchases"] if str(p.get("system", "")).lower() == "s01"]
    x["automation"] = m.get("automation") or []
    x["rate_max"] = (m["game"].get("targets") or {}).get("purchase_rate_max_per_minute", PURCHASE_RATE_MAX)


def empty_slots(m):
    """The ids of the content slots with no content: a milestone with no effect and no grant that no purchase, no
    milestone and no automation requires. The solver writes such a slot where the designer must name an element."""
    owners = m["purchases"] + (m.get("milestones") or []) + (m.get("automation") or [])
    needed = {r.get("id") for o in owners for r in o.get("requires") or [] if r.get("type") == "milestone"}
    return sorted(q["id"] for q in m.get("milestones") or []
                  if not q.get("effects") and not q.get("grants") and q["id"] not in needed)


def model_inputs(m):
    x = dict(mode="model", econ=econ_of(m), X_e=X_E, X_p=X_P)
    for step in (economy_inputs, reset_inputs, presence_inputs, spine_inputs, target_inputs):
        step(m, x)
    return x


# ----------------------------------------------------------------------------- derived law values
def derive(x):
    """alpha, s, lambda, s after the caps, T_X, kappa, ldot and u from the inputs (D12, L6, L4b, L8)."""
    R = x.get("R_max")
    x["alpha"] = 1.0 / (1.0 - R) if R is not None and R < 1 else None
    loop_terms(x)
    run_length(x)
    idle_terms(x)
    unlock_rate(x)


def loop_terms(x):
    x["s"] = x["lam"] = x["s_end"] = None
    if loop_known(x) and x["alpha"]:
        x["s"], x["lam"] = x["p"] * x["alpha"], x["p"] * x["q"] * x["alpha"]
    if not missing(x, ("p", "R_end")) and x["R_end"] < 1:
        x["s_end"] = x["p"] / (1.0 - x["R_end"])


def run_length(x):
    x["T_X"] = None
    if missing(x, ("s", "c", "A", "alpha")) or x["c"] is None:
        return
    x["T_X"] = x["X_e"] ** (1.0 / x["s"]) * (x["c"] / x["A"]) ** (1.0 / x["alpha"]) / x.get("K_q", 1.0)


def idle_terms(x):
    x["kappa"] = x["kappa_closed"] = None
    if missing(x, ("T_X", "f_open", "f_closed", "alpha")) or x.get("V") is None:
        return
    try:
        shape = x["alpha"] * (x["V"] / x["T_X"]) ** (x["alpha"] - 1.0)
    except OverflowError:       # a solver candidate with a very short run: the idle share is not known
        return
    x["kappa"], x["kappa_closed"] = x["f_open"] * shape, x["f_closed"] * shape


def unlock_rate(x):
    x["ldot"] = x["u_model"] = None
    if long_law(x):
        long_rate(x)
        return
    if missing(x, ("T_X", "q", "alpha", "w")) or x["w"] <= 1 or x["q"] <= 0:
        return
    x["ldot"] = HOUR * x["q"] * math.log(1 + x["X_e"]) / x["T_X"]
    x["u_model"] = x.get("kappa_y", 1.0) * x["alpha"] * x["ldot"] / math.log(x["w"])


def long_law(x):
    """True for 1 long run (BM, "1 long run with no reset"): no reset layer, R after the caps is 1, and no uncapped
    additive line of the economy. L2 then gives I ~ e^(g t), so 1 wall ratio w gives a constant unlock rate (L3), N
    gives the length (L8), and g sets the largest play-test speed (L9)."""
    R = x.get("R_end")
    if x.get("has_reset") or R is None:
        return False
    return (abs(R - 1.0) <= LONG_R_TOL and not x.get("uncapped_additive")) or bool(x.get("step_law"))


def step_carrier(x):
    """The first uncapped additive line of the economy when each such line has count steps (the generator as a
    carrier; a second generator line is in its group), else None."""
    add, lines = x.get("uncapped_additive") or [], x.get("step_lines") or []
    same = bool(lines) and set(add) == {ln["id"] for ln in lines}
    return lines[0] if same and not x.get("has_reset") else None


def count_exponent(ln, k):
    """The local exponent of the factor k of an additive line at level k: d ln k / d ln S = 1 / (k ln b) (BM 3a)."""
    return 1.0 / (k * math.log(ln["b"])) if k and k > 0 else None


def step_inputs(m, x):
    """The generator as a carrier (BM 3a): the level k_mid of the line at the middle of the run with no stage, the
    local exponent 1 / (k_mid ln b) of its factor k, the share of the content, and step_law: R of the carriers +
    that exponent + that share is 1 within LONG_STEP_TOL."""
    ln = step_carrier(x)
    if not ln:
        return
    hours = m["game"]["target_hours"]
    sim = growth_run(m, x["econ"], hours)
    g = growth_slope(sim, x["econ"])
    x["k_mid"] = max(level_at(sim, q["id"], hours * HOUR / 2.0) for q in x["step_lines"])
    x["R_count"] = count_exponent(ln, x["k_mid"])
    x["R_content"] = content_log(m, x["econ"]) / (g * hours * HOUR) if g and g > 0 else 0.0
    x["g_halves"] = half_slopes(sim, x["econ"], hours)
    step_law(x, g)


def step_law(x, g):
    """step_law and g_run from R of the carriers, R_count and R_content."""
    total = None if x["R_count"] is None else x["R_end"] + x["R_count"] + x["R_content"]
    x["step_law"] = total is not None and abs(total - 1.0) <= LONG_STEP_TOL
    x["g_run"] = g if x["step_law"] else None


def content_log(m, econ):
    """The sum of ln F of the constant factors of the content on econ (BM 3a, "Content in 1 long run"): each mult
    effect of a milestone with a requirement, and each capped mult line paid in econ at its cap."""
    tg = (econ, "all")
    stones = [e["value"] for q in m.get("milestones") or [] if q.get("requires") for e in q.get("effects") or []
              if e["kind"] == "mult" and e["target"] in tg and e["value"] > 0]
    lines = [(mult_on(p, tg), p["max_level"]) for p in m["purchases"]
             if p["max_level"] is not None and p["costs"][0]["currency"] == econ]
    return sum(math.log(v) for v in stones) + sum(n * math.log(d) for d, n in lines if d)


def half_slopes(sim, econ, hours):
    """g of the first and of the second half of the window from GROWTH_FROM_HOURS to hours: the measure of how
    constant g is (BM 3a: simulated, not proved)."""
    cut = (GROWTH_FROM_HOURS + hours) / 2.0 * HOUR
    pts = [(s["t"], math.log(s["income"][econ])) for s in sim.samples
           if s["t"] >= GROWTH_FROM_HOURS * HOUR - EPS and s["income"].get(econ, 0.0) > 0]
    halves = [p for p in pts if p[0] <= cut], [p for p in pts if p[0] > cut]
    return tuple(slope(h) if len(h) >= 3 else None for h in halves)


def long_rate(x):
    """u = 3600 g / ln w (BM L3 with R = 1) and the e-folding time T_e = 1 / g of the long run, when g is known."""
    g, w = x.get("g_run"), x.get("w")
    x["T_e"] = 1.0 / g if g else None
    if g and w and w > 1:
        x["u_model"] = HOUR * g / math.log(w)


def run_growth(m, econ, hours):
    """g of BM L2 with R = 1: the slope of ln I of econ against t in the run with no reset and no stage, from
    GROWTH_FROM_HOURS to hours. The run has no stage, so a stage reward or the hard end does not change the slope.
    None when the run has fewer than 3 samples in that window."""
    return growth_slope(growth_run(m, econ, hours), econ)


def growth_run(m, econ, hours):
    """The run with no reset and no stage of run_growth."""
    m2 = copy.deepcopy(m)
    m2["prestige"] = dict(m2.get("prestige") or {}, enabled=False)
    for lad in m2.get("ladders") or []:
        lad.update(requires=dict(type="rate", currency=econ, base=1e300, growth=2.0), grants=[])
    return Sim(m2, hours, {}).play()


def level_at(sim, pid, t):
    """The level of a line at the time t of a run with no reset: its purchases up to t."""
    return sum(1 for e in sim.events if e["kind"] == "purchase" and e["id"] == pid and e["t"] <= t + EPS)


def growth_slope(sim, econ):
    pts = [(s["t"], math.log(s["income"][econ])) for s in sim.samples
           if s["t"] >= GROWTH_FROM_HOURS * HOUR - EPS and s["income"].get(econ, 0.0) > 0]
    return slope(pts) if len(pts) >= 3 else None


def gap_formula(s, x):
    return (math.log(1 + x["X_p"]) / math.log(1 + x["X_e"])) * (x["X_e"] / x["X_p"]) ** (1.0 / s)


def rho_open(x, kappa, hours):
    """L4b: rho(T) = ln(1 + X (kappa u)^p) / (u ln(1 + X)), u = T / T_X."""
    u = hours * HOUR / x["T_X"]
    X = x["X_e"]
    return math.log1p(X * (kappa * u) ** x["p"]) / (u * math.log1p(X))


def rho_closed(x, hours):
    cap = x["closed_cap_h"]
    eff = hours if cap is None else min(hours, cap)
    return rho_open(x, x["kappa_closed"], eff) * eff / hours


def states(x):
    """The idle states that the sheet checks. A model with no offline gain has no closed-game part."""
    return ("open",) if x.get("no_offline") else IDLE_STATES


def rho_at(x, state, hours):
    return rho_open(x, x["kappa"], hours) if state == "open" else rho_closed(x, hours)


# ----------------------------------------------------------------------------- the simulation
def trajectory_A(m, econ, alpha):
    """A of E_1(x) = A x^alpha: the run earnings of a run with no reset after A_SECONDS (L2, L6)."""
    return trajectory_A_F(m, econ, alpha)[0]


def trajectory_A_F(m, econ, alpha):
    """(A, F): A of trajectory_A, and the persistent factor F that the run with no reset holds at its end. The run
    gets no grant of econ at a stage: a grant adds to the run total that A measures and does not scale the income
    (BM section 7 limit 2). In the trial Tide Forge, the stage grant 50 x 2^n of coin over the 48 stages of the run
    of 1e6 s made A 14 times too large, and T_X of L6 came out at 128 s against 488 s of the simulation."""
    m2 = copy.deepcopy(m)
    m2["prestige"] = dict(m2.get("prestige") or {}, enabled=False)
    for lad in m2.get("ladders") or []:
        lad["grants"] = [g for g in lad.get("grants") or [] if g.get("currency", econ) != econ]
    sim = Sim(m2, A_SECONDS / HOUR, {}).play()
    earned = sim.run[econ]
    return (earned / A_SECONDS ** alpha if earned > 0 else None), persistent_factor(m, sim, math.inf, econ)


def persistent_factor(m, sim, t_end, econ):
    """F of BM L6: the product of the persistent constant factors on econ that sim holds at the time t_end: each
    milestone that persists, and each level of a purchase line that a reset does not set to 0 (a mult per level, a
    pct line as 1 + v n), except an uncapped line of the reset currency (in_q). A persistent factor F shortens each
    run by F^(-1/alpha) (L6: T ~ (c / (A F))^(1/alpha))."""
    lines, persists = persistent_sources(m)
    events = [e for e in sim.events if e["t"] <= t_end]
    held = held_milestones(events, persists)
    levels = collections.Counter(e["id"] for e in events if e["kind"] == "purchase" and e["id"] in lines)
    out = math.prod(effects_factor(q.get("effects") or [], econ, 1) for q in held)
    return out * math.prod(effects_factor(lines[i]["effects"], econ, n) for i, n in levels.items())


def held_milestones(events, persists):
    """The persistent milestones that the events reach (a repeat does not count)."""
    return [persists[e["id"]] for e in events if e["kind"] == "milestone" and e["id"] in persists
            and e["note"] != "repeat"]


def persistent_sources(m):
    """({id: line} of the lines that a reset keeps and that are not in q, {id: milestone} that persist)."""
    pr = m.get("prestige") or {}
    keep = set((pr.get("resets") or {}).get("purchase_systems") or [])
    lines = {p["id"]: p for p in m["purchases"] if p["system"] not in keep and not in_q(p, pr)} \
        if prestige_on(m) else {}
    return lines, {q["id"]: q for q in m.get("milestones") or [] if q.get("persists")}


def in_q(p, pr):
    """True for an uncapped line paid in the reset currency: its r is a part of q (PO2, BM D8), so F does not count
    it."""
    return p.get("max_level") is None and any(c["currency"] == pr.get("currency") for c in p.get("costs") or [])


def effects_factor(effs, econ, n):
    """The factor of n levels of the mult and pct effects on econ (pct: 1 + v n)."""
    return math.prod(level_factor(e, n) for e in effs if e["target"] in (econ, "all"))


def level_factor(e, n):
    if e["kind"] == "mult":
        return e["value"] ** n
    return 1.0 + e["value"] * n if e["kind"] == "pct" else 1.0


def held_factor(m, early, econ):
    """F that early_reset holds at the start of the median run of its later half (the run that T_sim measures)."""
    runs = early.runs
    if not runs:
        return None
    later = runs[len(runs) // 2:]
    mid = sorted(later, key=lambda r: r["minutes"])[len(later) // 2]
    return persistent_factor(m, early, mid["start_minutes"] * 60.0, econ)


def run_points(runs, min_run):
    """(ln L at the start of the run, ln T of the run) for each run after the first that the buyer did not
    end at min_run_seconds."""
    starts = itertools.accumulate((r["gain"] for r in runs), initial=0.0)
    pairs = [(life, r["minutes"] * 60.0) for life, r in zip(starts, runs)]
    return [(math.log(life), math.log(sec)) for life, sec in pairs if life > 0 and sec > min_run + 1]


def slope(pts):
    mx = statistics.fmean(p[0] for p in pts)
    my = statistics.fmean(p[1] for p in pts)
    sxx = sum((p[0] - mx) ** 2 for p in pts)
    return sum((p[0] - mx) * (p[1] - my) for p in pts) / sxx if sxx > 0 else 0.0


def lambda_measured(runs, q, min_run):
    """L6: T_n ~ L_n^e with e = (1 - lambda) / s and lambda = q s, so lambda = q / (e + q)."""
    pts = run_points(runs, min_run)
    if len(pts) < 3 or not q or q <= 0:
        return None
    e = slope(pts)
    return math.inf if e + q <= 0 else q / (e + q)


def later_run_seconds(runs):
    secs = [r["minutes"] * 60.0 for r in runs]
    return statistics.median(secs[len(secs) // 2:]) if secs else None


def unlock_times(sim, ax):
    return sorted(v[0] for v in sim.step_clock[ax].values())


def gaps_after_hour(times):
    return [(b - a) / 60.0 for a, b in zip(times, times[1:]) if a >= HOUR - EPS]


def is_unlock(e):
    first_buy = e["kind"] == "purchase" and e["note"] == "first"
    return first_buy or (e["kind"] == "milestone" and e["note"] != "repeat")


def early_unlocks(sim, ax):
    """Unlocks in the first 5 minutes: new axis steps, first purchases of a line, and milestones after t = 0."""
    events = [e for e in sim.events if 0 < e["real"] <= FIRST_WINDOW and is_unlock(e)]
    steps = [t for t in unlock_times(sim, ax) if 0 < t <= FIRST_WINDOW]
    return len(events) + len(steps)


def peak_first_hour(sim, cur):
    vals = [s["income"].get(cur, 0.0) for s in sim.samples if s["t"] <= HOUR + EPS]
    return max(vals, default=None)


def phases(sim, lines):
    """R of each phase of active_greedy: a phase starts when a line is first bought and ends at a cap."""
    st = {ln["id"]: sim.pstat[ln["id"]] for ln in lines}
    marks = sorted({0.0} | {s["first"] for s in st.values() if s["first"] is not None}
                   | {s["cap_at"] for s in st.values() if s["cap_at"] is not None})
    out = []
    for t in marks:
        _add_phase(out, t, sum(ln["r"] for ln in lines if _open_at(st[ln["id"]], t)))
    return out


def _open_at(s, t):
    return s["first"] is not None and s["first"] <= t + EPS and (s["cap_at"] is None or s["cap_at"] > t + EPS)


def _add_phase(out, t, R):
    last = out[-1]["R"] if out else None
    if last is None or abs(last - R) > EPS:
        out.append(dict(start_minutes=round(t / 60.0, 2), R=round(R, 4)))


def idle_values(block):
    curve = block.get("idle_curve") or {}
    return {s: {h: (curve.get(s) or {}).get(str(h), {}).get("value") for h in IDLE_HOURS} for s in IDLE_STATES}


def measure(m, x, speed):
    """Run the players. Return the measured values of the sheet."""
    _, block, pl = pw_players.run_players(m, m["game"]["target_hours"], list(PLAYERS))
    base = pl.runs["active_greedy"][1]
    ms = dict(block=block, idle=idle_values(block), visits={n: pl.runs[n][1].visits for n in IDLE_PLAYERS if n in pl.runs},
              policy_gap=(block.get("policy_gap") or {}).get("value"),
              length_hours=(block.get("growth_slope") or {}).get("length_hours"),
              unlocks_first_hour=(block.get("growth_slope") or {}).get("unlocks_first_hour"),
              slowdown=(block.get("speed_test") or {}).get("slowdown_at_play_test"),
              **base_end(pl, base))
    measure_loop(ms, x, pl, m)
    measure_rest(ms, m, x, (pl, base))
    ms["speed_test"] = pw_players.speed_test(m, pl, pw_players.SPEEDS_ALL) if speed else None
    return ms


def base_end(pl, base):
    """The horizon, whether active_greedy finishes, its simulated hours and its axis step at the end (PO12)."""
    return dict(horizon_hours=pl.horizon({}) / HOUR, finished=base.axis_done(), sim_hours=base.real / HOUR, axis_reached=base.ladder_best.get(base.axis, 0) if base.axis else None)


def measure_rest(ms, m, x, runs):
    pl, base = runs
    measure_spine(ms, x, base)
    measure_base(ms, m, x, base)
    player_ends(ms, m, pl)


def player_ends(ms, m, pl):
    """The hours when each player reaches the end of the axis (PC1 prints each one; F28 of the Tide Forge trial)."""
    ax = pw_players.axis_of(m)
    if ms.get("continuous") is None or not ax:
        return
    ms["continuous"]["player_ends"] = {n: pw_players._end_hours(sim, ax) for n, (_, sim) in sorted(pl.runs.items())}


def continuous_measure(m, base):
    """The events of active_greedy that the rows PC1 to PC3 read: purchases, unlocks and axis step times."""
    ax = pw_players.axis_of(m)
    steps = {n: v[0] for n, v in base.step_clock[ax].items()} if ax else {}
    buys = [e for e in base.events if e["kind"] == "purchase"]
    out = dict(purchases=[e["real"] for e in buys], steps=steps, firsts={e["id"]: e["real"] for e in buys if
                                                                          e["note"] == "first"})
    out.update(unlocks=times_ids(e for e in base.events if is_unlock(e)), bought=times_ids(e for e in buys if not
                                                                                          e.get("auto")))
    out["auto_bought"] = times_ids(e for e in buys if e.get("auto"))
    out["first_reset"] = first_reset(base.events)
    out["end"] = base.end_real
    return out


def first_reset(events):
    """The time of the first reset of the run, or None."""
    return next((e["real"] for e in events if e["kind"] == "prestige"), None)


def times_ids(events):
    return [(e["real"], e["id"]) for e in events]


def measure_base(ms, m, x, base):
    ms["phases"] = phases(base, x["econ_lines"])
    ms["continuous"] = continuous_measure(m, base) if x.get("pattern") == "continuous" else None
    ms["purchase_gains"] = purchase_gains(m, base) if x.get("pattern") else []
    ms["upkeep"] = upkeep_measure(m, base) if m.get("upkeep") else None
    ms["tops"] = {pid: st["top"] for pid, st in base.pstat.items()}
    ms["line_life"] = {pid: dict(last=st["last"], cap_at=st["cap_at"], top=st["top"]) for pid, st in base.pstat.items()}
    ms["end_seconds"] = base.real


def measure_loop(ms, x, pl, m):
    if not x["has_reset"] or "early_reset" not in pl.runs:
        ms.update(T_sim=None, F_held=None, lambda_sim=None, runs_early=[])
        return
    early = pl.runs["early_reset"][1]
    ms["T_sim"] = later_run_seconds(early.runs) if x["has_reset"] else None
    ms["F_held"] = held_factor(m, early, x["econ"]) if x["has_reset"] else None
    ms["lambda_sim"] = lambda_measured(early.runs, x["q"], x.get("min_run", 0)) if x["has_reset"] else None
    ms["runs_early"] = [round(r["minutes"], 2) for r in early.runs[:40]]


def measure_spine(ms, x, base):
    ax = x["axis"]
    gaps = gaps_after_hour(unlock_times(base, ax)) if ax else []
    ms["median_gap"] = statistics.median(gaps) if gaps else None
    ms["largest_gap"] = max(gaps) if gaps else None
    ms["early_unlocks"] = early_unlocks(base, ax) if ax else None
    ms["P1h"] = peak_first_hour(base, x.get("spine_cur")) if ax else None


# ----------------------------------------------------------------------------- the 13 obligations
def po1(x, ms):
    if not x.get("has_reset"):
        return po1_no_reset(x), None, growth_clauses(x)
    if missing(x, ("R_max",)):
        return not_computed(["R_max"]), None, []
    R = x["R_max"]
    text = f"R = {R:.3f} (given by --R)" if x.get("R_source") == "--R" else r_text(x, R)
    f = part(text, R, R < 1, "< 1", fix="" if R < 1 else fix_r(x, R - 1 + 0.01))
    return f, phase_part(ms), []


GROWTH_FIX = ("g changes over the length: spread the constant factors of the content evenly over the length, or use "
              "finer count steps (a smaller n of S12.count_steps)")


def growth_clauses(x):
    """PO1 with a generator carrier: the simulation half. L1 proves R of the carriers, and no law proves that g
    stays constant while the factor k grows and the content comes in steps. So g of the 2 halves of the window after
    hour 1 must agree within DIFF_LIMIT, the limit of each comparison of math and simulation."""
    a, b = x.get("g_halves") or (None, None)
    if not x.get("step_law") or not a or not b:
        return []
    diff = abs(a - b) / max(a, b)
    return [(f"g of the 2 halves after hour 1 differ by {diff:.0%} (limit {DIFF_LIMIT:.0%}; simulated, not proved)",
             diff <= DIFF_LIMIT, GROWTH_FIX)]


def r_text(x, R):
    lines = ", ".join(f"{ln['id']} {ln['r']:.3f}" for ln in x["econ_lines"]) or "no line"
    return f"R up to {R:.3f} ({lines})"


def po1_no_reset(x):
    if missing(x, ("R_end",)):
        return not_computed(["R_end"])
    R, add = x["R_end"], x.get("uncapped_additive") or []
    if step_carrier(x) and x.get("R_count") is not None:
        return po1_carrier(x, R)
    ok = R < 1 - EPS or (abs(R - 1) <= EPS and not add)
    return part(f"R after the caps {R:.3f}, no reset layer", R, ok, "< 1, or = 1 with no uncapped additive line",
                fix="" if ok else fix_r(x, R - 1 + 0.01))


def po1_carrier(x, R):
    """PO1 with a generator carrier (BM 3a): R of the carriers plus the local exponent 1 / (k ln b) of the factor k
    at the middle of the length is at most 1 (within LONG_STEP_TOL). Above 1 the growth has no limit (L2)."""
    share, ln = x.get("R_content") or 0.0, step_carrier(x)
    total = R + x["R_count"] + share
    ok = total <= 1.0 + LONG_STEP_TOL
    halves = x.get("g_halves") or (None, None)
    content = f" + the content share {share:.3f}" if share else ""
    text = (f"R of the carriers {R:.3f} + 1 / (k ln b) {x['R_count']:.3f} of {ln['id']} at level {x['k_mid']} (the "
            f"middle of the length){content} = {total:.3f}, no reset layer; g of the 2 halves after hour 1: "
            f"{fmt(halves[0], 3)} and {fmt(halves[1], 3)} /s")
    return part(text, total, ok, f"<= 1 (within {LONG_STEP_TOL:g}) at the middle of the length",
                fix="" if ok else fix_r(x, total - 1.0))


def phase_part(ms):
    if not ms or not ms["phases"]:
        return None
    top = max(p["R"] for p in ms["phases"])
    text = "phases of active_greedy: " + ", ".join(f"{p['R']:.3f} from {p['start_minutes']:g} min" for p in ms["phases"])
    return dict(part(text, None, None), note=f"largest measured phase R {top:.3f}")


def fix_r(x, need):
    """The smallest cut of the line exponents that lowers R by need: the lines with the largest r first."""
    ordered = sorted(x.get("econ_lines") or [], key=lambda ln: -ln["r"])
    before = itertools.accumulate((ln["r"] for ln in ordered), initial=0.0)
    cuts = [_cut_text(ln, ln["r"] - min(ln["r"], need - done)) for ln, done in zip(ordered, before)
            if done < need - EPS]
    return "lower R below 1: " + "; ".join(cuts) + ", or cap the lines so that they are not open in the same phase"


def _cut_text(ln, new_r):
    d = ln["b"] ** new_r
    if not in_range("S02.levelled_factor", "d", d):
        return f"give {ln['id']} a cap (a capped line takes its r out of R after the cap)"
    return f"set d of {ln['id']} from {fmt(ln['d'])} to {fmt(d)} (r {ln['r']:.3f} -> {new_r:.3f})"


def in_range(variant, name, value):
    """True when value is inside the proven range of the parameter, or the parameter has no range. A fix text
    proposes only a value inside the proven ranges (SKILL.md: never leave a proven range)."""
    return range_note(variant, name, value) == "" and value == value and abs(value) != math.inf


def po2(x, ms):
    if not x.get("has_reset"):
        return not_applicable(NO_RESET), None, []
    if not loop_known(x):
        return not_computed(missing(x, ("R_max", "p", "q"))), None, []
    sim = lambda_part(ms)
    if x["R_max"] >= 1:
        return part("no limit: R >= 1", math.inf, False, "<= 1", fix="fix PO1 first, then set q = (1 - R) / p"), sim, []
    lam = x["lam"]
    note = " (lambda < 1: runs get longer and progress slows)" if lam < 1 - LAMBDA_ONE else ""
    return part(f"lambda = {lam:.3f}{note}", lam, lam <= 1 + EPS, "<= 1", fix=fix_lambda(x)), sim, []


def lambda_part(ms):
    if not ms or ms.get("lambda_sim") is None:
        return None
    v = ms["lambda_sim"]
    return part(f"lambda from the run lengths of early_reset {fmt(v)}", v, v <= 1 + DIFF_LIMIT)


def fix_lambda(x):
    one_minus = 1 - x["R_max"]
    q_new, p_new = one_minus / x["p"], one_minus / x["q"] if x["q"] else None
    text = f"lower q from {fmt(x['q'])} to {fmt(q_new)}"
    if p_new and in_range("S04.power", "p", p_new):
        text += f", or lower p from {fmt(x['p'])} to {fmt(p_new)}"
    return text


def po3(x, ms):
    if not x.get("has_reset"):
        return not_applicable(NO_RESET), None, []
    if missing(x, ("p", "R_end")):
        return not_computed(missing(x, ("p", "R_end"))), None, []
    if x["R_end"] >= 1:
        return part("s has no value: R >= 1 after the caps", None, False, "> 1", fix="fix PO1 first"), None, []
    s = x["s_end"]
    need = 1 - x["R_end"]
    fix = f"raise p from {fmt(x['p'])} to above {fmt(need)}" if in_range("S04.power", "p", need * 1.01) else \
        fix_r_end(x)
    return part(f"s = {s:.3f} (R after the caps {x['R_end']:.3f})", s, s > 1, "> 1", fix=fix), None, []


def po4(x, ms):
    if not x.get("has_reset"):
        return not_applicable(NO_RESET), None, []
    sim = gap_part(ms)
    if not loop_known(x):
        return not_computed(missing(x, ("R_max", "p", "q"))), sim, []
    if unstable(x):
        return part("no bound: lambda > 1", math.inf, False, f"<= {fmt(x['Dg'])}", fix="fix PO2 first"), sim, []
    gap = gap_formula(x["s"], x)
    return part(f"gap = {gap:.3f} (s = {x['s']:.3f})", gap, gap <= x["Dg"] + EPS, f"<= {fmt(x['Dg'])}",
                fix=fix_gap(x)), sim, []


def gap_part(ms):
    if not ms or ms.get("policy_gap") is None:
        return None
    v = ms["policy_gap"]
    return part(f"policy gap of early_reset / patient_reset {fmt(v)}", v, v <= DG_DEFAULT)


def fix_r_end(x):
    """PO3 when no p of the proven range gives s > 1: R after the caps must rise above 1 - p_max."""
    p_max = (range_of("S04.power", "p") or [0.0, x["p"]])[1]
    return (f"raise R after the caps from {fmt(x['R_end'])} to above 1 - p = {fmt(1.0 - p_max)} (p is at most "
            f"{fmt(p_max)} in S04.power): give a capped mult line of the main currency no cap, add an uncapped mult "
            "line (S02.levelled_factor), or make an additive reset reward a factor (q > 0)")


def fix_gap(x):
    ratio = x["Dg"] * math.log(1 + x["X_e"]) / math.log(1 + x["X_p"])
    if ratio >= 1:
        return "raise Dg"
    s_max = math.log(x["X_e"] / x["X_p"]) / math.log(ratio)
    p_max = s_max * (1 - x["R_max"])
    if not in_range("S04.power", "p", p_max):
        return f"raise Dg, or lower R so that s = p / (1 - R) <= {fmt(s_max)}"
    return f"lower p from {fmt(x['p'])} to below {fmt(p_max)} (s <= {fmt(s_max)})"


def po5(x, ms):
    if not x.get("has_reset"):
        return not_applicable(NO_RESET), None, []
    sim = run_part(ms, "T")
    if loop_known(x) and unstable(x):
        return part("runs shrink with no limit: lambda > 1", None, False, "within 20% of sigma",
                    fix="fix PO2 first"), sim, []
    miss = missing(x, ("R_max", "p", "q", "A", "c", "sigma", "T_X"))
    if miss:
        return not_computed(miss), sim, []
    tx, sg = x["T_X"], x["sigma"]
    ok = abs(tx - sg) / sg <= TX_TOLERANCE
    return part(f"T_X = {tx:.0f} s (A = {fmt(x['A'], 4)}, c = {fmt(x['c'], 5)}{k_text(x)})", tx, ok,
                f"{sg * (1 - TX_TOLERANCE):.0f} to {sg * (1 + TX_TOLERANCE):.0f} s", fix=fix_divisor(x, sg)), sim, []


def run_part(ms, label):
    if not ms or ms.get("T_sim") is None:
        return None
    v = ms["T_sim"]
    return part(f"median run of early_reset (later half) {v:.0f} s", v, None)


def k_text(x):
    """The constant K of the lines of the reset currency (BM D8), and the held factor F (BM L6), when not 1."""
    k, f = x.get("K_q", 1.0), x.get("F_ratio") or 1.0
    out = "" if abs(k - 1.0) < EPS else f", K = {fmt(k, 4)} of the lines of the reset currency (BM D8)"
    return out + ("" if abs(f - 1.0) < EPS else f", F held / F of the trajectory = {fmt(f, 4)} (BM L6)")


def fix_divisor(x, target):
    c_new = x["A"] * (x.get("K_q", 1.0) * target) ** x["alpha"] * x["X_e"] ** (-x["alpha"] / x["s"])
    return f"set the reset divisor c from {fmt(x['c'], 5)} to {fmt(c_new, 5)}"


OUT_OF_DOMAIN = "formula not valid here (kappa u < 1); simulated value used"


AUTO_IDLE = "L4b assumes no purchase while idle; the auto-buy of the model (S24) buys while the game is open, so " \
            "the simulation decides"
AUTO_DOMINANCE = "dominance: the auto-buy (S24) buys for the idle and the active player alike, and the active player " \
                 "also buys by hand, so idle stays below active (L4b assumes no purchase while idle)"


def po6(x, ms):
    sim = idle_part(x, ms)
    f = dict(no_formula(AUTO_IDLE), points={}) if x.get("automation") else idle_formula(x)
    if f["num"] is not None and sim is not None:
        idle_compare(x, ms, f, sim)
    f["point_rows"] = idle_point_rows(x, f, ms)
    return f, sim, outside_clauses(x, f["point_rows"])


def idle_point_rows(x, f, ms):
    """1 entry for each tested point: the formula value, the simulated value, and the source of the value."""
    return [point_row(x, f, ms, s, h) for s in states(x) for h in IDLE_HOURS]


def point_row(x, f, ms, s, h):
    pts = f.get("points") or {}
    inside = (s, h) in pts
    return dict(state=s, hours=h, formula=pts.get((s, h)), simulation=ms["idle"][s][h] if ms else None,
                source="formula" if inside else "simulation", band=idle_band(x, s, h),
                note="" if inside or not pts else OUT_OF_DOMAIN)


def outside_clauses(x, rows):
    """A point outside the domain of L4b takes its status from the simulation, never from the formula."""
    out = [r for r in rows if r["note"]]
    return [(f"{r['state']} {r['hours']} h: {OUT_OF_DOMAIN}"
             + ("" if r["simulation"] is None else f" ({r['simulation']:.3f})"),
             None if r["simulation"] is None else _in(r["simulation"], r["band"]),
             f"move {r['state']} {r['hours']} h into {r['band'][0]:g} to {r['band'][1]:g}") for r in out]


def idle_band(x, state, h):
    rows = x["idle_targets"].get(state) or {}
    return next((b for k, b in rows.items() if float(k) == h), [0.0, 1.0])


def idle_formula(x):
    if long_law(x) and x.get("g_run"):
        return long_idle(x)
    if not x.get("has_reset"):
        return no_formula("L4b needs a reset loop")
    if x.get("lam") is None or abs(x["lam"] - 1) > LAMBDA_ONE:
        return no_formula("L4b needs a stable reset loop with lambda = 1 (PO2)")
    miss = missing(x, ("kappa", "T_X", "p"))
    if miss:
        return not_computed(miss)
    pts = domain_points(x)
    if not pts:
        return no_formula("each tested wait has kappa u < 1, outside the domain of L4b")
    bad = [(s, h) for (s, h), v in pts.items() if not _in(v, idle_band(x, s, h))]
    out = part(idle_text(x, pts), 0.0, not bad, idle_limits(x), fix=fix_idle(x, bad))
    out["points"] = pts
    return out


def rho_long(x, state, hours):
    """BM L4 for 1 long run (R = 1, I = g S): a wait of T at the income share f adds f g T S to the spend S, and the
    visit of V seconds adds g V to ln S, so rho(T) = (ln(1 + f g T) + g V) / (g (T + V)). A closed wait counts up to
    its cap."""
    g, V = x["g_run"], x["V"]
    f = x["f_open"] if state == "open" else x["f_closed"]
    T = hours * HOUR
    cap = x["closed_cap_h"] if state == "closed" else None
    eff = T if cap is None else min(T, cap * HOUR)
    return (math.log1p(f * g * eff) + g * V) / (g * (T + V))


def long_idle(x):
    """PO6 for 1 long run: rho_long at each tested wait against its band."""
    pts = {(s, h): rho_long(x, s, h) for s in states(x) for h in IDLE_HOURS}
    bad = [(s, h) for (s, h), v in pts.items() if not _in(v, idle_band(x, s, h))]
    fix = "" if not bad else f"change f_{bad[0][0]} (the active factor) until {bad[0][0]} {bad[0][1]} h is in its band"
    out = part(idle_text(x, pts), 0.0, not bad, idle_limits(x), fix=fix)
    out["points"] = pts
    return out


def in_domain(x, state, h):
    """L4b assumes a reset at each visit. The rule G >= X L permits it only when the bank of the wait is at
    least the earnings of 1 run: kappa u >= 1 (closed: with the capped time)."""
    kap = x["kappa"] if state == "open" else x["kappa_closed"]
    cap = x["closed_cap_h"] if state == "closed" else None
    eff = h if cap is None else min(h, cap)
    return kap * eff * HOUR / x["T_X"] >= 1.0 - EPS


def domain_points(x):
    return {(s, h): rho_at(x, s, h) for s in states(x) for h in IDLE_HOURS if in_domain(x, s, h)}


def _in(v, band):
    return band[0] - EPS <= v <= band[1] + EPS


def idle_text(x, pts):
    shown = "; ".join(f"{s} {h} h {v:.3f}" for (s, h), v in pts.items())
    out = [f"{s} {h} h" for s in states(x) for h in IDLE_HOURS if (s, h) not in pts]
    return shown + "".join(f"; {o}: {OUT_OF_DOMAIN}" for o in out)


def idle_limits(x):
    rows = [f"{s} {h} h {b[0]:g} to {b[1]:g}" for s in states(x) for h in IDLE_HOURS
            for b in [idle_band(x, s, h)] if b != [0.0, 1.0]]
    return "; ".join(rows + ["other points 0 to 1"])


def fix_idle(x, bad):
    if not bad:
        return ""
    state, h = bad[0]
    band = idle_band(x, state, h)
    key = "f_open" if state == "open" else "f_closed"
    f_new = solve_share(x, state, h, mid(band))
    return f"set {key} from {fmt(x[key])} to {fmt(f_new)} so that {state} {h} h is {fmt(mid(band))}"


def solve_share(x, state, h, goal):
    """The income share f that gives rho(state, h) = goal (bisection; rho rises with f)."""
    lo, hi = 1e-6, 1.0
    y = dict(x)
    for _ in range(60):
        f = (lo + hi) / 2
        y["kappa" if state == "open" else "kappa_closed"] = f * x["alpha"] * (x["V"] / x["T_X"]) ** (x["alpha"] - 1)
        lo, hi = (f, hi) if rho_at(y, state, h) < goal else (lo, f)
    return (lo + hi) / 2


def idle_part(x, ms):
    if not ms:
        return None
    pts = {(s, h): ms["idle"][s][h] for s in states(x) for h in IDLE_HOURS if ms["idle"][s][h] is not None}
    if not pts:
        return None
    bad = [k for k, v in pts.items() if not _in(v, idle_band(x, *k))]
    out = part("; ".join(f"{s} {h} h {v:.3f}" for (s, h), v in pts.items()), None, not bad)
    out["points"] = pts
    out["fix"] = f"{loop_first(x)}set f_open and f_closed with L4b" if bad else ""
    return out


def idle_compare(x, ms, f, sim):
    """The worst point of the idle curve. The tolerance is 20% plus 1 visit of the idle player: the player
    finishes the axis at a visit, so its rate has that resolution."""
    scored = [_score(ms, f, key, v) for key, v in sim["points"].items() if key in f["points"]]
    scored = [x for x in scored if x]
    if not scored:
        return
    _, key, tol = max(scored)
    f["num"], sim["num"], sim["tol"] = f["points"][key], sim["points"][key], tol
    sim["compare_at"] = f"{key[0]} {key[1]} h"


def _score(ms, f, key, v):
    tol = DIFF_LIMIT + 1.0 / max(ms["visits"].get(f"{key[0]}_{key[1]}h", 1), 1)
    d = rel_diff(f["points"][key], v)
    return None if d is None else (d / tol, key, tol)


def po7(x, ms):
    sim = idle_max_part(x, ms)
    if x.get("automation"):
        return part(AUTO_DOMINANCE, ok=True, limit="< 1"), sim, [("simulation: largest idle value below 1",
                                                                 sim["ok"] if sim else None, "")]
    if not x.get("has_reset"):
        return po7_no_reset(x), sim, []
    if x.get("lam") is None or abs(x["lam"] - 1) > LAMBDA_ONE:
        return no_formula("L4b needs a stable reset loop with lambda = 1 (PO2)"), sim, []
    miss = missing(x, ("kappa", "p"))
    if miss:
        return not_computed(miss), sim, []
    if x["p"] <= 1:
        f = po7_falling(x)
        return f, falling_sim(f, ms, sim), below_active_clauses(x, ms)
    return po7_rising(x), sim, []


def po7_rising(x):
    """p > 1: the estimate of the largest rho of L4b (BM L4b)."""
    kap, X = (x["kappa"] if x.get("no_offline") else max(x["kappa"], x["kappa_closed"])), x["X_e"]
    left, right = x["p"] * kap * X ** (1.0 / x["p"]), math.e * math.log1p(X)
    f = part(f"p kappa X^(1/p) = {left:.3f} (rho_max about {left / right:.3f})", left / right, left < right,
             f"< e ln(1 + X) = {right:.3f}", fix=fix_kappa(x, left, right))
    return f


def po7_falling(x):
    """p <= 1: rho of L4b falls as the wait T rises, so its largest value is at the shortest tested wait
    inside the domain of L4b (kappa u >= 1). The estimate p kappa X^(1/p) / (e ln(1 + X)) is the maximum only
    for p > 1."""
    pts = domain_points(x)
    if not pts:
        return no_formula("each tested wait has kappa u < 1, outside the domain of L4b")
    key = max(pts, key=pts.get)
    top = pts[key]
    fix = "" if top < 1 else f"lower f_open (an active-only income term) until rho({key[0]} {key[1]} h) < 1"
    out = part(f"p = {fmt(x['p'])} <= 1: rho falls as T rises, largest inside L4b at {key[0]} {key[1]} h: "
               f"rho = {top:.3f}", top, top < 1, "< 1", fix=fix)
    out["at"] = key
    return out


def below_active_clauses(x, ms):
    """The waits outside the domain of L4b: only the simulation checks that idle stays below active."""
    pts = domain_points(x) if not missing(x, ("kappa", "T_X", "p")) else {}
    keys = [(s, h) for s in states(x) for h in IDLE_HOURS if (s, h) not in pts]
    return [below_active_clause(s, h, ms["idle"][s][h] if ms else None) for s, h in keys]


def below_active_clause(s, h, v):
    return (f"{s} {h} h: {OUT_OF_DOMAIN}" + ("" if v is None else f" ({v:.3f} < 1)"),
            None if v is None else v < 1, f"lower f_open until {s} {h} h is below 1")


def falling_sim(f, ms, sim):
    """The simulated value at the point of the formula, with the tolerance of that point of PO6: 20% plus
    1 / (visits of the idle player)."""
    key = f.get("at")
    v = ms["idle"][key[0]][key[1]] if key and ms else None
    if sim is None or v is None:
        return sim
    sim["num"] = v
    sim["value"] = f"idle value at {key[0]} {key[1]} h {v:.3f}; " + sim["value"]
    sim["tol"] = DIFF_LIMIT + 1.0 / max(ms["visits"].get(f"{key[0]}_{key[1]}h", 1), 1)
    return sim


def po7_no_reset(x):
    miss = missing(x, ("f_open", "f_closed", "R_end"))
    if miss:
        return not_computed(miss)
    ok = max(away_shares(x)) <= 1 + EPS and x["R_end"] > 0
    return part("L4 property 3: f <= 1 and R > 0", None, ok, "f <= 1 and R > 0", fix="" if ok else "raise R above 0")


def away_shares(x):
    return [x["f_open"]] if x.get("no_offline") else [x["f_open"], x["f_closed"]]


def idle_max_part(x, ms):
    if not ms:
        return None
    vals = [v for s in states(x) for v in ms["idle"][s].values() if v is not None]
    return part(f"largest idle value {max(vals):.3f}", max(vals), max(vals) < 1) if vals else None


def fix_kappa(x, left, right):
    key = "f_open" if x["kappa"] >= x["kappa_closed"] else "f_closed"
    return f"lower {key} from {fmt(x[key])} to below {fmt(x[key] * right / left)}"


LATE_ACTIVE = ("f_open = {f} at the start, but the model has active-only income that the start state does not show "
               "(a purchase or a later milestone), so the simulation decides")


def late_active(m, econ):
    """True when an active-only effect on the main income is not in effect at the start: an effect of a purchase, or
    of a milestone with a requirement. Then f_open of the start state is not the open income share of the game."""
    owners = (m.get("purchases") or []) + [x for x in m.get("milestones") or [] if x.get("requires")]
    return any(f.get("active") and f.get("target") == econ for x in owners for f in x.get("effects") or [])


def open_idle(x, ms, ok_of):
    """(formula part, simulation part) of PO8 or PO9 when the simulation decides: the largest open idle value of the
    players of the simulator."""
    f = part(LATE_ACTIVE.format(f=fmt(x["f_open"])), limit="<= 1" if ok_of is None else "< 1")
    f["sim_used"] = True
    vals = [v for v in ((ms or {}).get("idle") or {}).get("open", {}).values() if v is not None]
    if not vals:
        return f, None
    v = max(vals)
    return f, part(f"largest open idle value {v:.3f}", v, v <= 1 + EPS if ok_of is None else v < 1,
                   fix="lower the open income: an active-only income term, or a presence gate")


def po8(x, ms):
    miss = missing(x, ("f_open", "f_closed"))
    if miss:
        return not_computed(miss), None, []
    if x.get("late_active"):
        return (*open_idle(x, ms, None), [])
    fo, fc = x["f_open"], x["f_closed"]
    ok = max(away_shares(x)) <= 1 + EPS
    fix = "" if ok else "lower each income share above 1 to 1 or less (an active-only effect below 1 raises it)"
    return part(f"f_open = {fmt(fo)}, f_closed = {fmt(fc)}", max(fo, fc), ok, "<= 1", fix=fix), None, []


def po9(x, ms):
    if x.get("late_active") and not x.get("tools"):
        return (*open_idle(x, ms, True), [])
    absent = x["absence_h"]
    cap = x.get("open_cap_h") if x.get("open_cap_known") else None
    if cap is not None and cap < absent - EPS:
        return part(f"open-idle cap {fmt(cap)} h", cap, False, f">= {fmt(absent)} h",
                    fix=f"raise the open-idle cap from {fmt(cap)} h to {fmt(absent)} h or more"), None, []
    tools = x.get("tools")
    known = tools is not None and x.get("open_cap_known")
    f = po9_none() if tools == [] else (po9_pass(tools, absent) if known else not_computed(["tools"]))
    return f, None, []


def po9_none():
    return part("no presence tool: f_open = 1, no open-idle cap, no bank cap, no spine step that needs a visit",
                None, False, "1 tool or more", fix=PO9_FIX)


def po9_pass(tools, absent):
    return part("tools: " + "; ".join(tools), None, True, f"no open-idle cap below {fmt(absent)} h")


PO9_FIX = ("add an active-only income term (for example x2 for active play, so f_open = 0.5), or a presence gate "
           "(S07, S08 or S09) whose cap is at least the longest absence that must pay")


def po10(x, ms):
    sim, clauses = gap_parts(x, ms)
    if not (stable_loop(x) or long_law(x)):
        return no_formula("L3 gives a constant unlock rate only for a stable reset loop (PO2), or for 1 long run "
                          "with R = 1 and no reset"), sim, clauses
    if not x.get("spine_income"):
        return no_formula("L3 needs the axis to test the income of the economy currency, or of a second income "
                          "with a multiplicative line (L10)"), sim, clauses
    miss = missing(x, ("u_model", "sigma"))
    return (not_computed(miss) if miss else po10_formula(x)), sim, clauses


def po10_formula(x):
    u, gap = x["u_model"], 60.0 / x["u_model"]
    ok = gap <= x["sigma"] / 60.0 + EPS and (not x["u_band"] or _in(u, x["u_band"]))
    expo = unlock_exponent(x)
    w_new = math.exp(expo) if expo < 700 else math.inf
    fix = f"lower the wall ratio w from {fmt(x['w'], 4)} to {fmt(w_new, 4)}" if in_range("S03.exponential", "g", w_new) \
        else "the wall ratio w for 1 unlock in each session is outside the proven range of S03.exponential: lengthen " \
             "the run (run_min) or relax the unlock target"
    return part(f"u = {u:.2f} unlocks for each hour, 1 / u = {gap:.1f} min", gap, ok,
                f"<= {x['sigma'] / 60:g} min" + (f", u in {x['u_band']}" if x["u_band"] else ""), fix=fix)


def stable_loop(x):
    return bool(x.get("has_reset")) and not unstable(x) and x.get("lam") is not None


def unlock_exponent(x):
    """ln w for 1 unlock in each session sigma: kappa_y alpha ldot sigma (L8), or g sigma for 1 long run (L3)."""
    if long_law(x):
        return x["g_run"] * x["sigma"]
    return x.get("kappa_y", 1.0) * x["alpha"] * x["ldot"] * x["sigma"] / HOUR


def gap_parts(x, ms):
    if not ms or ms.get("median_gap") is None:
        return None, [("largest gap after hour 1", None, "not run")]
    lim = LARGEST_GAP_RATIO * x["sigma"] / 60.0
    sim = part(f"median gap after hour 1 {ms['median_gap']:.1f} min", ms["median_gap"],
               ms["median_gap"] <= x["sigma"] / 60.0 + EPS)
    big = ms["largest_gap"]
    return sim, [(f"largest gap after hour 1 {big:.1f} min (limit {lim:.1f} min)", big <= lim + EPS,
                 "lower the wall ratio w so that each session has an unlock (after PO2)")]


def po11(x, ms):
    clause = early_clause(ms)
    miss = missing(x, ("a1", "I0"))
    if miss:
        return not_computed(miss), None, [clause]
    lim = FIRST_COST_SECONDS * x["I0"]
    ok = x["a1"] <= lim + EPS
    return part(f"a_1 = {fmt(x['a1'])} ({x['a1_line']}), I_0 = {fmt(x['I0'])}", x["a1"], ok, f"<= {fmt(lim)}",
                fix="" if ok else f"lower the first cost from {fmt(x['a1'])} to {fmt(lim)}"), None, [clause]


def early_clause(ms):
    if not ms or ms.get("early_unlocks") is None:
        return ("unlocks in the first 5 minutes", None, "not run")
    n = ms["early_unlocks"]
    return (f"{n} unlocks in the first 5 minutes (limit {FIRST_UNLOCKS} or more)", n >= FIRST_UNLOCKS,
            "add an unlock (an axis step or a new line) before minute 5")


def po12(x, ms):
    sim = length_part(x, ms)
    if x.get("H") is None:
        return no_formula("no length target (game.targets.length_hours of a model)"), sim, []
    f = length_formula(x, ms)
    return f, sim, []


def length_formula(x, ms):
    if not (stable_loop(x) or long_law(x)):
        return no_formula("L8 needs a stable reset loop (PO2)")
    if not x.get("spine_income"):
        return no_formula("L8 needs the axis to test the income of the economy currency, or of a second income with "
                          "a multiplicative line (L10)")
    miss = missing(x, ("u_model", "w", "N"))
    if miss or not ms or not ms.get("P1h") or not ms.get("unlocks_first_hour"):
        return not_computed(miss or ["P(1 h) and the first-hour unlocks (simulation)"])
    u1 = ms["unlocks_first_hour"]
    lead = math.log(ms["P1h"] / step_amount(x["spine_req"], u1)) / math.log(x["w"])
    return length_result(x, u1, lead)


def length_result(x, u1, lead):
    """L8: H = 1 + (N - u_1 - ln(P(1 h) / h_u1) / ln w) / u, and N for the target H."""
    h_pred = 1.0 + (x["N"] - u1 - lead) / x["u_model"]
    n_new = u1 + math.ceil(lead + x["u_model"] * (x["H"] - 1))
    ok = abs(h_pred - x["H"]) / x["H"] <= LENGTH_TOLERANCE
    return part(f"H from N = {x['N']}: {h_pred:.2f} h", h_pred, ok, f"{fmt(x['H'])} h within 15%",
                fix=f"set the axis count N from {x['N']} to {n_new}")


def loop_first(x):
    """The start of a fix text that needs a stable reset loop. A model with no reset layer has no PO2 to fix."""
    return "fix PO2 first, then " if x.get("has_reset") else ""


def length_part(x, ms):
    if not ms or x.get("H") is None:
        return None
    v = ms["length_hours"]
    if v is None:
        return unfinished_part(x, ms)
    ok = abs(v - x["H"]) / x["H"] <= LENGTH_TOLERANCE
    out = part(f"active_greedy finishes at {v:.2f} h", v, ok)
    out["fix"] = f"the game ends at {v:.2f} h: {loop_first(x)}set N with L8"
    return out


def unfinished_part(x, ms):
    """PO12 when active_greedy does not finish: the simulated hours and the stage that it reaches."""
    hours = ms.get("sim_hours") or ms["horizon_hours"]
    late = hours > x["H"] * (1 + LENGTH_TOLERANCE)
    where = "" if ms.get("axis_reached") is None else f" (stage {ms['axis_reached']} of {x.get('N')} at {hours:.2f} h)"
    return part(f"active_greedy does not finish in {hours:.2f} h of simulation{where}", None, False if late else None)


def po13(x, ms):
    lim = SPEED_FACTOR * x["k"] * D_H
    sim = speed_part(ms, lim)
    if long_law(x) and x.get("T_e"):
        return po13_long(x, ms, lim), sim, speed_clause(x, ms, long_kmax(x))
    if not x.get("has_reset"):
        return no_formula("T_X needs a reset loop; the slow-down of the speed test decides"), sim, []
    if loop_known(x) and unstable(x):
        return part("runs shrink with no limit: lambda > 1", None, False, f">= {lim:g} s",
                    fix="fix PO2 first"), sim, []
    miss = missing(x, ("T_X",))
    if miss:
        return not_computed(miss), sim, []
    tx = x["T_X"]
    kmax = math.floor(tx / (SPEED_FACTOR * D_H))
    text = f"T_X = {tx:.0f} s at x{fmt(x['k'])}; largest speed: formula x{kmax}, simulation {speed_text(ms)}"
    return part(text, tx, tx >= lim - EPS, f">= {lim:g} s",
                fix=f"test at x{kmax} or slower, or raise T_X to {lim:g} s (PO5)"), sim, speed_clause(x, ms, kmax)


def long_kmax(x):
    """The largest speed of L9 for 1 long run: g k D_h <= SPEED_GD, so k <= SPEED_GD T_e / D_h."""
    return math.floor(SPEED_GD * x["T_e"] / D_H + EPS)


def po13_long(x, ms, lim):
    """L9 for 1 long run: the relative growth g = 1 / T_e of L2 with R = 1 is the g of L9, so g k D_h <= 0.1 is
    T_e >= 10 k D_h, the form of PO13 with T_e in place of T_X."""
    te, kmax = x["T_e"], long_kmax(x)
    text = (f"T_e = 1 / g = {te:.0f} s at x{fmt(x['k'])}; largest speed: formula x{kmax}, simulation "
            f"{speed_text(ms)}")
    return part(text, te, te >= lim - EPS, f">= {lim:g} s", fix=f"test at x{kmax} or slower")


def sim_speed(ms):
    """The largest simulated speed with a slow-down under SLOWDOWN_MAX: from the full test of --speed, else from
    the short test of the players (x10 and x100). None when no tested speed is under the limit."""
    st = (ms or {}).get("speed_test") or ((ms or {}).get("block") or {}).get("speed_test") or {}
    return st.get("largest_speed")


def speed_text(ms):
    st = (ms or {}).get("speed_test") or ((ms or {}).get("block") or {}).get("speed_test")
    if not st:
        return "not run"
    k = st.get("largest_speed")
    low = min(r["speed"] for r in st["rows"])
    return f"x{k}" if k is not None else f"below x{low} (each tested speed has a slow-down of {SLOW_TEXT} or more)"


SLOW_TEXT = f"{pw_players.SLOWDOWN_MAX:g}"


def speed_clause(x, ms, kmax):
    """L9 in the simulation: the formula counts about 1 human decision for each run, but the active player decides
    at each purchase. The smaller of the 2 largest speeds decides, so the slow-down at k must stay under the limit."""
    slow = (ms or {}).get("slowdown")
    if slow is None:
        return []
    k, ksim = x["k"], sim_speed(ms)
    ok = slow < pw_players.SLOWDOWN_MAX
    low = min(kmax, ksim) if ksim is not None else None
    fix = (f"test at x{low} or slower" if low else f"test below x{k}: run pw_proof.py --speed for the largest "
           "simulated speed") + " (the simulated slow-down includes the decision time of each purchase)"
    return [(f"simulated slow-down at x{fmt(k)} = {fmt(slow)} (limit < {SLOW_TEXT})", ok, fix)]


def speed_part(ms, lim):
    if not ms or (ms.get("T_sim") is None and ms.get("slowdown") is None):
        return None
    t, slow = ms.get("T_sim"), ms.get("slowdown")
    ok = (t is None or t >= lim - EPS) and (slow is None or slow < pw_players.SLOWDOWN_MAX)
    return part(f"median run {fmt(t, 4)} s, slow-down at the play-test speed {fmt(slow)}", t, ok)


# ----------------------------------------------------------------------------- the rows of continuous play
def end_time(cm, mark):
    """The real seconds of active_greedy to a mark: {"axis_step": n} or {"purchase": id}, else the hard end event
    that the simulator records ({"resets": k} or {"milestone": id}). None when not reached."""
    if "axis_step" in mark:
        return cm["steps"].get(int(mark["axis_step"]))
    return cm["firsts"].get(mark["purchase"]) if "purchase" in mark else cm.get("end")


def mark_text(mark):
    return f"axis step {mark['axis_step']}" if "axis_step" in mark else f"purchase {mark.get('purchase')}"


def pc1(x, cm):
    band, mark = x.get("length_band"), x.get("hard_end")
    if not mark:
        return dict(status="fail", value="no hard end", limit="game.targets.hard_end",
                    fix="declare the hard end (S28, decision D10): game.targets.hard_end")
    if not band:
        return dict(status="not computed", value=mark_text(mark), limit="", reason="no length target (length_hours)")
    t = end_time(cm, mark)
    lim = f"{fmt(band[0])} to {fmt(band[1])} h"
    if t is None:
        return dict(status="fail", value=f"active_greedy does not reach {mark_text(mark)}", limit=lim,
                    fix="lower the walls before the hard end, or move the hard end earlier")
    row = hard_end_row(t / HOUR, band, mark, lim)
    row["value"] += others_text(cm, band, x)
    return row


def others_text(cm, band, x=None):
    """The end of the axis for each other active player (a reset rule); BAND OUT marks an end outside the length
    band, with its cause. The row tests the active player only: the length target is the length of active play."""
    ends = {n: h for n, h in (cm.get("player_ends") or {}).items() if "reset" in n and h is not None}
    ends = ends if (x or {}).get("has_reset") else {}        # a game with no reset layer has no reset player
    marks = [f"{n} {h:.2f} h" + ("" if band[0] - EPS <= h <= band[1] + EPS else f" (BAND OUT: {band_cause(n, x)})")
             for n, h in ends.items()]
    return "; other players: " + ", ".join(marks) if marks else ""


def band_cause(name, x):
    """The cause of a reset player outside the length band: its reset rule G >= X L with its own X."""
    if "patient" in name and (x or {}).get("s") and x.get("X_p"):
        return patient_cause(x)
    return "its reset rule G >= X L differs from the rule of active_greedy, so its run lengths differ (PO4)"


def patient_cause(x):
    return (f"it resets at G >= {fmt(x['X_p'])} L, not {fmt(x['X_e'])} L, so each run is about "
            f"{gap_formula(x['s'], x):.2f} times as long (PO4) and fewer resets fit in the length")


def hard_end_row(h, band, mark, lim):
    ok = band[0] - EPS <= h <= band[1] + EPS
    fix = "" if ok else f"the hard end comes at {h:.2f} h: move it, or change the length target"
    return dict(status="pass" if ok else "fail", value=f"active_greedy reaches {mark_text(mark)} at {h:.2f} h",
                limit=lim, fix=fix, num=h)


def purchase_gaps(times, t_end):
    """The gaps in minutes between 0, each purchase up to t_end, and t_end."""
    marks = [0.0] + [t for t in times if t <= t_end + EPS] + [t_end]
    return [(b - a) / 60.0 for a, b in zip(marks, marks[1:])]


def pc2(x, cm):
    times = sorted(cm["purchases"])
    lim = "<= " + about(x, "largest_purchase_gap_minutes", x["gap_max"], "min")
    if not times:
        return dict(status="fail", value="no purchase", limit=lim,
                    fix="add a purchase line that the player can buy at the start")
    t_end = (end_time(cm, x["hard_end"]) if x.get("hard_end") else None) or times[-1]
    gaps = purchase_gaps(times, t_end)
    row = pc2_row(x, max(gaps), times[0] / 60.0, lim)
    row["where"] = gap_where(times, t_end, gaps.index(max(gaps)))
    row["fix"] = "" if row["status"] != "fail" else gap_fix(row["where"])
    return row


def pc2_row(x, big, first, lim):
    """The status, the value, the clause and the range notes of PC2 from its 2 values."""
    keys = (("largest_purchase_gap_minutes", big, x["gap_max"]), ("first_purchase_minutes", first, x["first_max"]))
    zones = [zone_of(x, *k) for k in keys]
    notes = [t for k in keys for t in zone_note(x, *k, "min")]
    return dict(status=worst(zones), num=big, limit=lim, clauses=[first_clause(first, x["first_max"], zones[1])],
                value=f"largest gap {big:.2f} min; first purchase at {first:.2f} min", in_range=notes)


def first_clause(first, first_max, z="pass"):
    return dict(text=f"first purchase at {first:.2f} min (limit {fmt(first_max)} min)", ok=z != "fail")


def gap_where(times, t_end, at):
    """The start and the end in minutes of the gap number at of purchase_gaps."""
    marks = [0.0] + [t / 60.0 for t in times if t <= t_end + EPS] + [t_end / 60.0]
    return dict(start_minutes=round(marks[at], 2), end_minutes=round(marks[at + 1], 2))


def gap_fix(w):
    return (f"add a line or a reward that the player can buy between {w['start_minutes']:.1f} and "
            f"{w['end_minutes']:.1f} min, or a cheaper line, or a lower cost growth where the gap is")


def extension_parts(t_demo, t_end, part_s):
    """Parts of part_s seconds from the demo end to the hard end. A last part under half a part joins the part
    before it."""
    n = max(1, round((t_end - t_demo) / part_s))
    return [(t_demo + i * part_s, t_end if i == n - 1 else t_demo + (i + 1) * part_s) for i in range(n)]


def part_content(cm, lo, hi, empty=()):
    """(the ids of the new content of the part, the ids of its empty slots)."""
    got = sorted({ident for t, ident in cm["unlocks"] if lo < t <= hi + EPS})
    return [i for i in got if i not in empty], [i for i in got if i in empty]


def pc3(x, cm):
    demo = x.get("demo_end")
    if not demo:
        return dict(status="not applicable", value="no demo end", limit="", reason="no demo end stated (no demo)")
    t_demo, t_end = end_time(cm, demo), end_time(cm, x["hard_end"]) if x.get("hard_end") else None
    if t_demo is None or t_end is None or t_end <= t_demo:
        return dict(status="fail", value="the demo end or the hard end is not reached", limit="",
                    fix="the active player must reach the demo end before the hard end")
    parts = [(lo, hi) + part_content(cm, lo, hi, x.get("empty_slots") or ())
             for lo, hi in extension_parts(t_demo, t_end, x["part_h"] * HOUR)]
    return extension_row(parts, x["part_h"])


def extension_row(parts, part_h):
    """The row PC3 from (lo, hi, content ids, empty slot ids) of each part. An empty slot is not content."""
    empty = [i + 1 for i, p in enumerate(parts) if not p[2]]
    texts = [part_text(i, p) for i, p in enumerate(parts)]
    return dict(status="fail" if empty else "pass", value="; ".join(texts), num=len(parts) - len(empty),
                limit=f"new content in each part of {fmt(part_h)} h", fix=extension_fix(parts, empty), extension=[
                    dict(part=t.split(":")[0], content=p[2], empty_slots=p[3]) for t, p in zip(texts, parts)])


def part_text(i, p):
    lo, hi, ids, slots = p
    slot = f" (empty slot: {', '.join(slots)})" if slots else ""
    return f"part {i + 1} ({lo / HOUR:.2f} to {hi / HOUR:.2f} h): {', '.join(ids) or 'nothing new'}{slot}"


def extension_fix(parts, empty):
    if not empty:
        return ""
    slots = [i for n in empty for i in parts[n - 1][3]]
    named = (": give each empty slot (" + ", ".join(slots) + ") a named element with an effect, a grant, or a line "
             "that needs it") if slots else ""
    return ("add new content (a system, an era, an area or a reset mechanic) in part " +
            ", ".join(str(i) for i in empty) + " (SG:185)" + named)


def unlock_marks(cm, t_end, empty=()):
    """0, the time of each unlock up to t_end (axis steps, first purchases, milestones), and t_end. An empty content
    slot is not an unlock."""
    times = {t for t in cm["steps"].values() if t is not None} | {t for t, i in cm["unlocks"] if i not in empty}
    return [0.0] + sorted(t for t in times if 0 < t < t_end - EPS) + [t_end]


def long_gaps(marks, limit_min):
    """Each gap above the limit: (start min, end min, gap min, more unlocks that the gap needs)."""
    pairs = [(a / 60.0, b / 60.0) for a, b in zip(marks, marks[1:])]
    return [(a, b, b - a, int(math.ceil((b - a) / limit_min - EPS)) - 1) for a, b in pairs
            if b - a > limit_min + EPS]


def unlock_fix(gaps):
    if not gaps:
        return ""
    return f"add {sum(g[3] for g in gaps)} more unlocks: " + "; ".join(
        f"{n} between {a:.1f} and {b:.1f} min" for a, b, _, n in gaps)


def pc4(x, cm):
    marks, first = pc4_marks(x, cm)
    parts = gap_parts_pc4(x, marks, first)
    big, big1 = largest_gap(marks), largest_gap(first)
    fails = [failed_gaps(x, part) for part in parts]
    row = dict(status=pc4_status(x, parts), num=big, limit=pc4_limit(x), value=f"largest gap {big:.2f} min; first "
               f"hour {big1:.2f} min; {len(marks) - 2} unlocks")
    row["clauses"] = [dict(text=f"first hour: largest gap {big1:.2f} min (limit {fmt(x['ugap_first'])} min)",
                           ok=not fails[1])]
    early = {g[0] for g in fails[1]}
    gaps = fails[1] + [g for g in fails[0] if g[0] not in early] + fails[2]
    row["in_range"] = [t for part in parts for t in gap_notes(x, part)]
    row["needs"] = [dict(start_minutes=round(a, 2), end_minutes=round(b, 2), gap_minutes=round(g, 2), more=n)
                    for a, b, g, n in gaps]
    row["fix"] = unlock_fix(gaps)
    return row


def pc4_status(x, parts):
    return worst([zone_of(x, key, g[2], lim) for key, lim, gs in parts for g in gs])


def failed_gaps(x, part):
    """The long gaps of 1 part of PC4 that are outside the range of its target."""
    key, lim, gs = part
    return [g for g in gs if zone_of(x, key, g[2], lim) == "fail"]


def gap_notes(x, part):
    """The text of each long gap of 1 part of PC4 that is inside the range of its target."""
    key, lim, gs = part
    return [t for a, b, g, _ in gs for t in zone_note(x, key, g, lim, "min", f"{g:.2f} min from {a:.1f} to {b:.1f}")]


def gap_parts_pc4(x, marks, first):
    """(target key, limit, long gaps) of the 3 parts of PC4: the whole length without the last gap when it has its
    own target, the first hour, and the last gap before the hard end (ending_unlock_gap_minutes)."""
    ending = x.get("ugap_ending")
    body = marks[:-1] if ending and len(marks) > 2 else marks
    last = long_gaps(marks[-2:], ending) if ending and len(marks) > 2 else []
    return [("largest_unlock_gap_minutes", x["ugap_max"], long_gaps(body, x["ugap_max"])),
            ("first_hour_unlock_gap_minutes", x["ugap_first"], long_gaps(first, x["ugap_first"])),
            ("ending_unlock_gap_minutes", ending, last)]


def pc4_limit(x):
    text = "<= " + about(x, "largest_unlock_gap_minutes", x["ugap_max"], "min") + "; first hour <= " + \
        about(x, "first_hour_unlock_gap_minutes", x["ugap_first"], "min")
    if not x.get("ugap_ending"):
        return text
    return text + "; last gap before the hard end <= " + about(x, "ending_unlock_gap_minutes", x["ugap_ending"], "min")


def pc5(x, cm):
    """Reachable: each purchase line and each milestone of the model is bought or reached before the hard end."""
    t_end = (end_time(cm, x["hard_end"]) if x.get("hard_end") else None) or math.inf
    got = {i for t, i in cm["unlocks"] if t <= t_end + EPS}
    els = x.get("elements") or []
    out = [dict(id=i, kind=k, requires=[req_text(r) for r in reqs], change=reach_change(k, reqs))
           for i, k, reqs in els if i not in got]
    names = ", ".join(u["id"] for u in out)
    row = dict(status="fail" if out else "pass", num=len(els) - len(out), limit="each element before the hard end",
               value=f"{len(els) - len(out)} of {len(els)} elements reached" + (f"; not reached: {names}" if out else ""))
    row["unreached"] = out
    row["fix"] = "; ".join(f"{u['id']}: {u['change']}" for u in out)
    return row


def req_text(r):
    t = r.get("type")
    if t == "total":
        return f"lifetime {r.get('currency')} {fmt(r.get('amount'))}"
    if t == "ladder":
        return f"{r.get('id')} {r.get('step')}"
    return f"{t} {r.get('id') or fmt(r.get('amount'))}"


def reach_change(kind, reqs):
    """The change that makes an element reachable: an earlier unlock, more of its currency, or a lower cost."""
    texts = [REACH_TEXT[r["type"]].format(cur=r.get("currency"), amount=fmt(r.get("amount")), ax=r.get("id"))
             for r in reqs if r.get("type") in REACH_TEXT]
    return texts[0] if texts else "lower its first cost" if kind == "purchase" else \
        "move its requirement before the hard end"


REACH_TEXT = {"total": "lower the lifetime {cur} total {amount} (an earlier unlock), or give more {cur} income before "
                       "the hard end",
              "ladder": "move it to a {ax} step at or before the hard end"}


def minute_counts(bought, t_end, auto):
    """{minute: purchases} of the purchases up to t_end; auto {line: from minute} leaves out the purchases that the
    automation makes."""
    kept = (int(t // 60.0) for t, i in bought if t <= t_end + EPS and int(t // 60.0) < auto.get(i, math.inf))
    return collections.Counter(kept)


def automation_plan(bought, t_end, limit, auto):
    """The lines that get auto-buy (S24) and the minute when each starts: the line with the most purchases in the
    minutes above the limit first, from the first such minute, until no minute is above the limit."""
    auto, plan = dict(auto), []
    minutes = [(int(t // 60.0), i) for t, i in bought if t <= t_end + EPS]
    step = next_auto(minutes, limit, auto)
    while step:
        auto[step["line"]] = step["from_minutes"]
        plan.append(step)
        step = next_auto(minutes, limit, auto)
    return plan


def next_auto(minutes, limit, auto):
    """The next line of the plan: the line with the most purchases in the minutes above the limit, from the first
    such minute; None when no minute is above the limit or no line is left."""
    over = busy_minutes(minutes, limit, auto)
    per = collections.Counter(i for m, i in minutes if m in over and i not in auto)
    if not per:
        return None
    line, n = max(per.items(), key=lambda kv: (kv[1], kv[0]))
    start = min(m for m, i in minutes if i == line and m in over)
    return dict(line=line, from_minutes=start, purchases_in_busy_minutes=n)


def busy_minutes(minutes, limit, auto):
    """The minutes with more purchases than the limit, without the purchases of the automation."""
    counts = collections.Counter(m for m, i in minutes if m < auto.get(i, math.inf))
    return {m for m, n in counts.items() if n > limit}


def pc6(x, cm):
    """The purchase rate: the most purchases that the active player makes by hand in 1 minute, against the most that
    a person can do (designer target). The simulator buys with the automation of the model (S24) from its start, and
    those purchases do not count. A failed row gives a plan that goes down to (1 - RATE_MARGIN) x the target."""
    t_end, given = rate_end(x, cm), list(x.get("automation") or [])
    auto = {a["line"]: a["from_minutes"] for a in given}     # a purchase by hand after the auto-buy opens does not count
    human = minute_counts(cm["bought"], t_end, auto)
    lim = x.get("rate_max", PURCHASE_RATE_MAX)
    row = dict(rate_zone(x, max(human.values(), default=0), lim), value=rate_text(cm, t_end, human, len(given)))
    row.update(busiest_minute=max(human, key=lambda m: (human[m], -m)) if human else None, automation=given)
    return rate_plan(row, (cm["bought"], t_end, lim * (1.0 - RATE_MARGIN), auto))


def rate_zone(x, top, lim):
    """The status, the limit and the range note of PC6."""
    key, unit = "purchase_rate_max_per_minute", "purchases in 1 minute"
    return dict(status=zone_of(x, key, top, lim), num=top, limit="<= " + about(x, key, lim, unit),
                in_range=zone_note(x, key, top, lim, unit, f"{top}"))


def rate_plan(row, args):
    """The automation plan and the fix of a failed row; args are those of automation_plan."""
    ok = row["status"] != "fail"
    row["plan"] = [] if ok else automation_plan(*args)
    row["fix"] = "" if ok else rate_fix(row["plan"])
    return row


def rate_end(x, cm):
    """The end time of the purchase rate row: the hard end, else the last purchase."""
    t_end = end_time(cm, x["hard_end"]) if x.get("hard_end") else None
    return t_end or max([t for t, _ in cm["bought"] + cm.get("auto_bought", [])] + [0.0])


def rate_text(cm, t_end, human, n_auto):
    """The busiest minute by hand, the busiest minute with the auto-buy purchases, and the mean of the last hour."""
    top = max(minute_counts(cm["bought"] + cm.get("auto_bought", []), t_end, {}).values(), default=0)
    mean = sum(n for m, n in human.items() if m * 60.0 >= t_end - HOUR) / 60.0
    return (f"busiest minute {max(human.values(), default=0)} purchases by hand ({top} with the auto-buy purchases); "
            f"last hour {mean:.1f} for each minute; {n_auto} lines with auto-buy")


def rate_fix(plan):
    if not plan:
        return "the automation takes no more purchases: add a buy-all control, or fewer lines with a larger cost ratio"
    return "give auto-buy (S24.auto_buy) to: " + "; ".join(f"{a['line']} from minute {a['from_minutes']}" for a in plan)


def pc4_marks(x, cm):
    """The unlock marks up to the hard end, and the marks of the first hour."""
    t_end = end_time(cm, x["hard_end"]) if x.get("hard_end") else None
    t_end = t_end or max([t for t, _ in cm["unlocks"]] + list(cm["steps"].values()) + [0.0])
    marks = unlock_marks(cm, t_end, x.get("empty_slots") or ())
    return marks, [t for t in marks if t < HOUR - EPS] + [min(HOUR, t_end)]


def largest_gap(marks):
    return max([b - a for a, b in zip(marks, marks[1:])], default=0.0) / 60.0


# ----------------------------------------------------------------------------- the purchase value row
PV_STATEMENT = ("At the time when the active player buys it, 1 level of each line raises the income, or the stat "
                "that the line affects, by the floor or more")
PV_LAW = "S02, L1"
PV_WINDOW = 600.0                              # the windows of the table "What the player buys": 10 minutes


def pv_floor(m):
    """(floor, source) of the purchase value row: game.targets.purchase_value, else the default."""
    pv = (m["game"].get("targets") or {}).get("purchase_value") or {}
    return float(pv.get("floor", PURCHASE_FLOOR)), pv.get("source") or PURCHASE_FLOOR_SOURCE


def gain_target(p, econ, cids):
    """The currency whose income 1 level of the line raises, or None (a cap raise, or a stat that is not a
    currency)."""
    effs = [e for e in p["effects"] if e["kind"] in ("add", "pct", "mult")]
    if not effs:
        return None
    t = econ if effs[0]["target"] == "all" else effs[0]["target"]
    return t if t in cids else None


def purchase_gains(m, sim):
    """[(real seconds, line id, gain)] of each purchase of sim: the income of the target currency after the purchase
    against the income before it. Each change of the income is an event of the sim, so the income of the event
    before the purchase is the income before it."""
    targets = gain_targets(m)
    befores = [start_sim(m).income()] + [e["income"] for e in sim.events[:-1]]
    pairs = [(e, targets.get(e["id"]), b, ub) for e, b, ub in zip(sim.events, befores, u_befores(m, sim))
             if e["kind"] == "purchase"]
    return [(e["real"], e["id"], gain_of(e, c, b) if ub is None else gross_gain(e, c, b, ub, m))
            for e, c, b, ub in pairs if c and b.get(c, 0.0) > 0]


def u_befores(m, sim):
    """The upkeep factor u before each event of sim (S30), or None for each event of a model with no upkeep."""
    if not m.get("upkeep"):
        return [None] * len(sim.events)
    return [upkeep_factor(m["upkeep"], {})] + [e["u"] for e in sim.events[:-1]]


def gross_gain(e, c, before, ub, m):
    """PV of a need line (S30): the gain of the line without the change of u. The rows PU1 to PU4 judge u."""
    need, _ = upkeep_lines(m["upkeep"])
    if e["id"] not in need or c != m["upkeep"]["target"] or ub <= 0 or e["u"] <= 0:
        return gain_of(e, c, before)
    return (e["income"][c] / e["u"]) / (before[c] / ub) - 1.0


def gain_targets(m):
    cids = {c["id"] for c in m["currencies"]}
    econ = econ_of(m)
    return {p["id"]: gain_target(p, econ, cids) for p in m["purchases"]}


def gain_of(e, c, before):
    return e["income"][c] / before[c] - 1.0


def effective_caps(m):
    """{line id: its cap with each cap raise of the model}, None for an uncapped line."""
    caps = {p["id"]: p["max_level"] for p in m["purchases"]}
    raisers = [(p["max_level"], p["effects"]) for p in m["purchases"]] + [(1, ms.get("effects") or [])
                                                                          for ms in m.get("milestones") or []]
    for n, effs in raisers:
        for e in effs:
            raise_cap(caps, e, n)
    return caps


def raise_cap(caps, e, n):
    if e["kind"] != "cap" or caps.get(e["target"]) is None:
        return
    caps[e["target"]] = None if n is None else caps[e["target"]] + e["value"] * n


def formula_gain(p, m, caps):
    """The smallest gain of 1 level of the line over all its levels (L1): d - 1 for a mult line; v over the sum of
    its group or its pool at the full caps for a pct or an add line; None for a cap raise."""
    effs = [e for e in p["effects"] if e["kind"] in ("add", "pct", "mult")]
    if not effs:
        return None
    e = effs[0]
    if e["kind"] == "mult":
        return e["value"] - 1.0
    total = pool_total(m, caps, e, gated_after(m, p["id"], caps))
    return 0.0 if total is None else e["value"] / max(total - e["value"], EPS)


def gated_after(m, pid, caps):
    """The lines that open only when line pid (or a line after it) is at its cap: they have no level while the player
    buys the levels of pid, so they are not in its pool (the pool order of the solver)."""
    after, grew = set(), True
    while grew:
        new = {q["id"] for q in m["purchases"] if q["id"] not in after and gates_on(q, after | {pid}, caps)}
        after, grew = after | new, bool(new)
    return after


def gates_on(q, ids, caps):
    return any(r.get("type") == "level" and r.get("id") in ids and caps.get(r["id"]) is not None and
               r.get("level", 1) >= caps[r["id"]] for r in q.get("requires") or [])


def pool_total(m, caps, e, skip=()):
    """The sum of the add pool (with the base income) or of the pct group (with 1) at the full caps, without the
    lines of skip; None when a line of it has no cap."""
    same = [(q["id"], f) for q in m["purchases"] for f in q["effects"] if same_pool(f, e) and q["id"] not in skip]
    if any(caps.get(i) is None for i, _ in same):
        return None
    return pool_base(m, e) + sum(f["value"] * caps[i] for i, f in same)


def same_pool(f, e):
    return f["kind"] == e["kind"] and f["target"] == e["target"] and f.get("group") == e.get("group")


def pool_base(m, e):
    if e["kind"] == "pct":
        return 1.0
    return next((float(c.get("base_income", 0.0)) for c in m["currencies"] if c["id"] == e["target"]), 0.0)


def line_values(m, gains):
    """1 entry for each purchase line: the formula gain, the smallest simulated gain, the levels bought, and the
    lines that a cap raise opens. A line with count steps: the gains are those of 1 count block (block_values), and
    level_gain is the smallest gain of 1 level, which is reported and not tested."""
    caps = effective_caps(m)
    bought = collections.defaultdict(list)
    for _, pid, g in gains:
        bought[pid].append(g)
    return [line_value(p, m, caps, bought.get(p["id"]) or []) for p in m["purchases"]]


def line_value(p, m, caps, got):
    f, low = formula_gain(p, m, caps), min(got) if got else None
    out = dict(id=p["id"], formula_gain=None if f is None else round(f, 6), levels_bought=len(got),
               simulated_gain=None if low is None else round(low, 6),
               raises=[e["target"] for e in p["effects"] if e["kind"] == "cap"])
    return dict(out, **block_values(p["count_steps"], got, low)) if p.get("count_steps") else out


def block_values(steps, got, low):
    """The PV values of a line with count steps: the formula gain of 1 block is m - 1 or more (the n levels also
    add), and the simulated gain is the smallest gain of a full block of n purchases."""
    n = steps["every"]
    blocks = [math.prod(1.0 + g for g in got[i:i + n]) - 1.0 for i in range(0, len(got) - n + 1, n)]
    return dict(formula_gain=round(steps["mult"] - 1.0, 6), simulated_gain=round(min(blocks), 6) if blocks else None,
                block=n, level_gain=None if low is None else round(low, 6))


def line_status(ln, floor, below, tol=0.0):
    if ln["raises"] and ln["formula_gain"] is None:
        return "below" if any(t in below for t in ln["raises"]) else "pass"
    vals = [v for v in (ln["formula_gain"], ln["simulated_gain"]) if v is not None]
    z = zone(min(vals), floor, tol, low=True) if vals else "pass"
    return {"fail": "below", IN_RANGE: "in range"}.get(z, "pass")


def pv_lines(m, gains, floor, tol=0.0):
    """The lines with their status. A cap raise comes after the lines that it raises."""
    lines = line_values(m, gains)
    below = set()
    for ln in sorted(lines, key=lambda q: bool(q["raises"])):
        ln["status"] = line_status(ln, floor, below, tol)
        below |= {ln["id"]} if ln["status"] == "below" else set()
    return lines


def pv_value(lines):
    vals = [(min(v for v in (ln["formula_gain"], ln["simulated_gain"]) if v is not None), ln["id"]) for ln in lines
            if ln["formula_gain"] is not None or ln["simulated_gain"] is not None]
    low = min(vals) if vals else None
    below = [ln["id"] for ln in lines if ln["status"] == "below"]
    head = f"smallest gain {low[0]:.1%} ({low[1]})" if low else "no line with a gain"
    return head + f"; {len(lines)} lines, {len(below)} below the floor" + (": " + ", ".join(below) if below else "") + \
        "".join(block_text(ln) for ln in lines if ln.get("block"))


def block_text(ln):
    """A line with count steps: the tested gain of 1 count block, and the gain of 1 level (not tested)."""
    level = "" if ln.get("level_gain") is None else f"; 1 level gains {ln['level_gain']:.1%} or more (reported, not tested)"
    return f"; {ln['id']}: the tested gain is that of 1 count block of {ln['block']} levels with its step{level}"


PV_WAYS = ("a mult line: d >= {d:g}; an add or pct line: add count steps (S12: a multiplier at each n owned levels, so "
           "that 1 count block is tested), or a larger value, or fewer levels; or lower the purchase value floor "
           "(game.targets.purchase_value.floor: a designer target with no source) when a level that adds less is the "
           "mechanic of the line")


def pv_fix(lines, floor):
    bad = [ln for ln in lines if ln["status"] == "below"]
    if not bad:
        return ""
    return f"raise the step of each line below the floor to {floor:.0%} or more ({PV_WAYS.format(d=1 + floor)}): " + \
        "; ".join(f"{ln['id']} ({gain_text(ln)})" for ln in bad)


def gain_text(ln):
    if ln["raises"] and ln["formula_gain"] is None:
        return "raises " + ", ".join(ln["raises"])
    parts = [f"formula {ln['formula_gain']:.1%}" if ln["formula_gain"] is not None else "",
             f"simulation {ln['simulated_gain']:.1%}" if ln["simulated_gain"] is not None else ""]
    return ", ".join(t for t in parts if t)


def purchase_value_row(m, x, ms):
    """The row "purchase value" (references/proof-sheet.md): each line of a model with a play pattern."""
    floor, source = pv_floor(m)
    r = extra_row("PV", "purchase", PV_STATEMENT, "", "", law=PV_LAW)
    tol = tolerance_of(x, "purchase_value")[0]
    r.update(limit=f">= about {floor:.0%} for each level, or for each count block of a line with count steps (range "
             f"to {edge(floor, tol, True):.1%})", floor=floor, floor_source=source)
    if not x.get("pattern"):
        r.update(status="not applicable", reason="no play pattern (game.play_pattern): a model of version 1")
        return r
    gains = (ms or {}).get("purchase_gains") or []
    parts = pv_parts(m, ms, gains, (floor, tol))
    r.update(lines=parts["lines"], below=parts["below"], value=parts["value"], source="both" if ms else "formula",
             windows=buy_windows(x, ms, gains), dead=parts["dead"])
    r.update(status=parts["status"], fix=parts["fix"], in_range=pv_notes(x, parts["lines"], floor))
    return with_range(r)


def pv_notes(x, lines, floor):
    """The text of each line whose smallest gain is below the floor and inside the range of the floor."""
    low = [(ln["id"], min(v for v in (ln["formula_gain"], ln["simulated_gain"]) if v is not None)) for ln in lines
           if ln["status"] == "in range"]
    return [miss_text(x, "purchase_value", f"{i} {100 * v:.1f}", 100 * floor, "%", True) for i, v in low]


def pv_parts(m, ms, gains, floor_tol):
    """The lines, the lines below the floor, the dead cap raises, the value, the status and the fix of PV."""
    floor, tol = floor_tol
    lines, dead = pv_lines(m, gains, floor, tol), dead_raises(m, ms)
    below = [ln["id"] for ln in lines if ln["status"] == "below"]
    near = any(ln["status"] == "in range" for ln in lines)
    fix = "; ".join(t for t in [pv_fix(lines, floor), dead_fix(dead)] if t)
    return dict(lines=lines, below=below, dead=dead, value=pv_value(lines) + dead_text(dead),
                status="fail" if below or dead else IN_RANGE if near else "pass", fix=fix)


def dead_text(dead):
    if not dead:
        return ""
    return f"; {len(dead)} cap raises open no level: " + ", ".join(f"{a['id']} ({a['target']})" for a in dead)


def dead_raises(m, ms):
    """Each cap effect of a purchase or a milestone that opens no level the player buys: its line has no cap, or the
    active player never buys a level of that line above its base cap."""
    base = {p["id"]: p["max_level"] for p in m["purchases"]}
    tops = (ms or {}).get("tops") or {}
    effs = [(sid, t) for sid, t in cap_effects(m) if t in base]
    return [dict(id=sid, target=t, reason=dead_reason(base[t], tops, t)) for sid, t in effs
            if base[t] is None or (tops and tops.get(t, 0) <= base[t])]


def cap_effects(m):
    """(source id, target) of each cap effect of a purchase or a milestone."""
    srcs = [(p["id"], p["effects"]) for p in m["purchases"]] + [(q["id"], q.get("effects") or [])
                                                                  for q in m.get("milestones") or []]
    return [(sid, e["target"]) for sid, es in srcs for e in es if e["kind"] == "cap"]


def dead_reason(cap, tops, target):
    if cap is None:
        return "the line has no cap"
    return f"the active player buys {target} up to level {tops.get(target, 0)}, not above its cap {cap}"


def dead_fix(dead):
    if not dead:
        return ""
    return "make each cap raise open levels that the player buys: give the line a cap that it reaches, or remove " \
           "the cap effect: " + "; ".join(f"{a['id']} on {a['target']} ({a['reason']})" for a in dead)


def buy_windows(x, ms, gains):
    """The table "What the player buys": for each 10 minutes of active_greedy up to the hard end, the purchases,
    the unlocks, and the smallest and the largest gain of a purchase."""
    cm = (ms or {}).get("continuous")
    if not cm:
        return []
    t_end = end_time(cm, x["hard_end"]) if x.get("hard_end") else None
    t_end = t_end or max([t for t, _, _ in gains] + [0.0])
    marks = unlock_marks(cm, t_end, x.get("empty_slots") or ())[1:-1]
    return [window_row(i, gains, marks) for i in range(int(math.ceil(t_end / PV_WINDOW - EPS)))]


def window_row(i, gains, marks):
    lo, hi = i * PV_WINDOW, (i + 1) * PV_WINDOW
    got = [g for t, _, g in gains if lo < t <= hi + EPS or (i == 0 and t == 0)]
    return dict(start_minutes=round(lo / 60.0), purchases=len(got), unlocks=sum(1 for t in marks if lo < t <= hi),
                gain_min=round(min(got), 4) if got else None, gain_max=round(max(got), 4) if got else None)


CL_STATEMENT = ("Each generator line (system S01) lives for the core of the game: the active player buys it, and it "
                "is not at its cap, at half of the length to the hard end or later")
CL_LAW = "designer rule, no source"


def line_life_rows(x, ms):
    """The row CL (references/proof-sheet.md): the core line life. A model with no play pattern or no simulation has
    no row. The limit is exact: a designer rule with no source, not a target with a range."""
    if not x.get("pattern") or not ms or not ms.get("line_life"):
        return []
    r = extra_row("CL", "purchase", CL_STATEMENT, "", "", law=CL_LAW)
    r.update(limit=f"the last purchase of each generator line at {LINE_LIFE_SHARE:.0%} of the length or later",
             source="simulation")
    gens = x.get("generators") or []
    if not gens:
        r.update(status="not applicable", reason="the model has no line of the system S01")
        return [r]
    t_end = life_end(x, ms)
    lives = [line_life(pid, ms["line_life"][pid], t_end) for pid in gens]
    short = [v for v in lives if v["share"] < LINE_LIFE_SHARE - EPS]
    r.update(status="fail" if short else "pass", value="; ".join(v["text"] for v in lives), lines=lives,
             fix=life_fix(short))
    return [r]


def life_end(x, ms):
    """The length to the hard end in seconds: the hard end of the active player, else the end of its run."""
    cm = ms.get("continuous")
    t = end_time(cm, x["hard_end"]) if cm and x.get("hard_end") else None
    return t or ms.get("end_seconds") or 0.0


def line_life(pid, st, t_end):
    last = st["last"] or 0.0
    share = last / t_end if t_end > 0 else 0.0
    how = "at its cap" if st["cap_at"] is not None else "last purchase"
    return dict(id=pid, level=st["top"], last_minutes=round(last / 60.0, 2), capped=st["cap_at"] is not None,
                share=round(share, 4), text=f"{pid}: level {st['top']}, {how} at {last / 60.0:.1f} min ({share:.0%} "
                f"of {t_end / 60.0:.1f} min)")


def life_fix(short):
    if not short:
        return ""
    return ("make each generator line a line that the player buys to the end: remove its cap and add count steps "
            "(S12: a multiplier at each n owned levels), or raise the cap, or lower its cost growth: " +
            ", ".join(v["id"] for v in short))


def cap_rows(m):
    """A warning row for each line whose cap fell below the cap that the game had (model key caps_reduced)."""
    return [extra_row(f"cap reduced: {c['id']}", "warning", "A line keeps the cap that the game had",
                      f"cap {fmt(c.get('game_cap')) if c.get('game_cap') is not None else 'none'} -> {fmt(c['cap'])}",
                      "warning", fix=c["reason"]) for c in m.get("caps_reduced") or []]


PATTERN_CHECKS = {"PC1": pc1, "PC2": pc2, "PC3": pc3, "PC4": pc4, "PC5": pc5, "PC6": pc6}


def pattern_rows(x, ms):
    """The rows of the play pattern continuous (references/proof-sheet.md). A check_in model has none."""
    if x.get("pattern") != "continuous":
        return []
    return [pattern_row(pid, law, text, x, ms) for pid, law, text in PATTERN_ROWS]


def pattern_row(pid, law, text, x, ms):
    r = extra_row(pid, "pattern", text, "", "", law=law)
    cm = (ms or {}).get("continuous")
    out = PATTERN_CHECKS[pid](x, cm) if cm else dict(status="not computed", reason="no simulation")
    r.update({k: v for k, v in out.items() if k != "num"})
    r["source"] = "simulation" if cm else ""
    r["simulation"] = dict(value=r["value"], num=out.get("num"), ok=r["status"] in ("pass", IN_RANGE)) if cm else None
    return with_range(r)


def with_range(r):
    """A row in the range zone shows the text of each miss in its value."""
    notes = r.get("in_range") or []
    if notes and r["status"] == IN_RANGE:
        r["value"] += "; in range: " + "; ".join(notes)
    return r


CHECKS = {"PO1": po1, "PO2": po2, "PO3": po3, "PO4": po4, "PO5": po5, "PO6": po6, "PO7": po7, "PO8": po8,
          "PO9": po9, "PO10": po10, "PO11": po11, "PO12": po12, "PO13": po13}


# ----------------------------------------------------------------------------- the status rules
def base_row(pid):
    law, kind, text = STATEMENT[pid]
    return dict(id=pid, kind="obligation", statement=text, law=law, value="", limit="", status="", source="",
                formula=None, simulation=None, difference=None, fix="", reason="", clauses=[], parts=[],
                points=[])


def status_of(kind, f, s):
    """(status, source). The formula decides when it is computed. Else a target takes the simulation, and a law
    takes only a failed simulation: a simulation alone does not prove a law."""
    if f["na"]:
        return "not applicable", "formula"
    if f.get("sim_used"):
        return (("pass" if s["ok"] else "fail"), "simulation") if s and s["ok"] is not None else ("not computed", "")
    if f["ok"] is not None:
        return ("pass" if f["ok"] else "fail"), ("both" if s else "formula")
    if s and s["ok"] is not None and (kind == "target" or not s["ok"]):
        return ("pass" if s["ok"] else "fail"), "simulation"
    return "not computed", ""


def apply_clauses(r, clauses):
    r["clauses"] = [dict(text=t, ok=ok) for t, ok, _ in clauses]
    failed = [fix for _, ok, fix in clauses if ok is False]
    unknown = [f"{t}: {fix or 'not run'}" for t, ok, fix in clauses if ok is None]
    if failed:
        r["status"] = "fail"
        r["fix"] = "; ".join(t for t in [r["fix"]] + failed if t)
    if unknown and r["status"] == "pass":
        r["status"] = "not computed"
        r["reason"] = unknown[0]


def compare(r, f, s):
    d = rel_diff(f["num"], s["num"]) if s else None
    r["difference"] = None if d is None else round(d, 4)
    tol = (s or {}).get("tol", DIFF_LIMIT)
    at = f" at {s['compare_at']}" if s and s.get("compare_at") else ""
    if d is None or d <= tol:
        return
    r["finding"] = f"model and math differ by {d:.0%} (limit {tol:.0%}){at}"
    r["status"] = "model and math differ" if r["status"] == "pass" else r["status"]


FLOOR_MARGIN = 0.05                     # a median later run within 5% of min_run_seconds ends at the run floor
RUN_ROWS = ("PO5", "PO10", "PO12", "PO13")      # the rows whose formula uses the run length T_X of L6
RUN_FLOOR = ("formula not valid here (the later runs of early_reset end at the run floor min_run_seconds, so L6 "
             "does not set their length); simulated value used")


FACTOR_TOL = 0.20                       # L6 holds while the persistent factors change T by 20% or less (PO5)


def factor_bound(x):
    """True when the persistent factors of the runs differ from those of the trajectory with no reset by more than L6
    permits: F held at the median later run over F of the trajectory, or its inverse, is above
    (1 + FACTOR_TOL)^(1 / (1 - R)). One A then does not hold for the runs, so T_X of L6 is outside its domain and the
    simulation decides (the old factor budget of the solver was this bound)."""
    f, R = x.get("F_ratio"), x.get("R_max")
    if not f or R is None or R >= 1 or not x.get("has_reset"):
        return False
    return max(f, 1.0 / f) > (1.0 + FACTOR_TOL) ** (1.0 / (1.0 - R)) + EPS


FACTOR_DOMAIN = ("formula not valid here (the runs hold persistent factors that change from run to run, F held / F "
                 "of the trajectory = {r}, outside (1 + 0.2)^(+-1 / (1 - R)) of L6); simulated value used")


def run_floor_bound(x, ms):
    """True when the median later run of early_reset ends at the run floor: the reset rule G >= X L is met before
    min_run_seconds, so T_X of L6 (and K of D8) is outside its domain."""
    T, floor = (ms or {}).get("T_sim"), x.get("min_run") or 0
    return bool(x.get("has_reset") and T and floor > 0 and T <= floor * (1.0 + FLOOR_MARGIN))


WALL_ROWS = ("PO4", "PO10", "PO12")      # the rows whose simulation half reads the axis steps, and whose formula
                                         # needs 1 wall ratio w of L8 (the axis linear in ln P)
WALL_DOMAIN = ("formula not valid here (the walls of the stages come from the timing fit by simulation, 1 wall for "
               "each stage, not from 1 ratio w of L8); simulated value used")


def walls_fitted(m):
    """True when the table of the progress ladder gives a wall for each stage (the timing fit of the solver): the
    stages then do not follow 1 ratio w, so u and N of L8 are outside their domain and the simulation decides."""
    lad = next(iter(m.get("ladders") or []), None)
    table = ((lad or {}).get("requires") or {}).get("table") or []
    return bool(lad) and len(table) >= lad.get("count", 0) > 1


CONTENT_DOMAIN = ("formula not valid here (the idle formula of 1 long run takes the carriers as the whole growth; "
                  "this model has named content with a share of the growth above the tolerance of PO1, or a second "
                  "generator line); simulated value used")


def content_bound(pid, x):
    """True for PO6 of 1 long run whose growth is not held by the carriers alone: a content share above
    LONG_STEP_TOL (the tolerance of the sum of PO1), or 2 or more generator lines. The idle formula of BM 3a is then
    outside its domain, and the simulation decides. With a smaller share the formula holds, and the comparison with
    the simulation checks it as for each row."""
    share = (x.get("R_content") or 0.0) > LONG_STEP_TOL
    return pid == "PO6" and bool(x.get("step_law")) and (share or len(x.get("step_lines") or []) > 1)


def domain_why(pid, x):
    """The text of the formula half outside its domain."""
    if content_bound(pid, x):
        return CONTENT_DOMAIN
    if x.get("walls_fitted") and pid in WALL_ROWS and not (x.get("floor_bound") or x.get("factor_bound")):
        return WALL_DOMAIN
    return FACTOR_DOMAIN.format(r=fmt(x["F_ratio"], 4)) if x.get("factor_bound") and not x.get("floor_bound") \
        else RUN_FLOOR


def out_of_domain(pid, x):
    return ((x.get("floor_bound") or x.get("factor_bound")) and pid in RUN_ROWS) or \
        (x.get("walls_fitted") and pid in WALL_ROWS) or content_bound(pid, x)


def floor_parts(pid, x, s, limit):
    """The formula half outside its domain, and the simulation half that decides (PO5: the band of sigma)."""
    why = domain_why(pid, x)
    f = part(why, limit=limit)
    f["sim_used"] = True
    if pid == "PO5" and s and x.get("sigma"):
        s = dict(s, ok=abs(s["num"] - x["sigma"]) / x["sigma"] <= TX_TOLERANCE)
    return f, s


def obligation_row(pid, x, ms):
    f, s, clauses = CHECKS[pid](x, ms)
    if out_of_domain(pid, x) and not f.get("na"):
        f, s = floor_parts(pid, x, s, f["limit"])
    r = base_row(pid)
    r["status"], r["source"] = status_of(STATEMENT[pid][1], f, s)
    r.update(formula=_half(f), simulation=_half(s), limit=f["limit"], reason=f["reason"])
    r["value"] = " | ".join(t for t in (_shown(f), s and "simulation: " + s["value"]) if t)
    r["fix"] = _fix_of(r, f, s)
    r["points"] = f.get("point_rows", [])
    apply_clauses(r, clauses)
    compare(r, f, s)
    return class_rules(r, x, s)


def class_rules(r, x, s):
    """The closed-game part, then the class of a row with no law (no_law)."""
    closed_part(r, x)
    no_law(r, x, s)
    return r


def no_law(r, x, s):
    """A model with no reset layer is outside the laws of the reset loop (L3, L4b, L6, L8, L9). A row that only the
    simulation decides there gets SIMULATED when the simulation meets its limit, never pass. A failed row stays
    failed. Reason: a simulation alone does not prove a law, and a model that no law covers still needs each target
    that the simulation can measure."""
    held = r["status"] == "pass" or (r["status"] == "not computed" and s and s.get("ok"))
    if x.get("has_reset") or r["source"] not in ("simulation", "") or not held:
        return
    r.update(status=SIMULATED, source="simulation", reason=r["reason"] or "no law covers a model with no reset layer; "
             "the simulation meets the limit")


def closed_part(r, x):
    """Step B of ticket 13: with no offline gain, the closed-game part of PO6 to PO9 is not applicable. The
    open-game part stays computed: a player who leaves the game open must stay below the active player."""
    if r["id"] not in CLOSED_PARTS or not x.get("no_offline"):
        return
    r["parts"] = [dict(part="closed game", status="not applicable", reason=NO_OFFLINE)]
    r["value"] = "; ".join(t for t in (r["value"], f"closed game: not applicable ({NO_OFFLINE})") if t)


def _shown(f):
    return "" if f["value"] == "n/a" else f["value"]


def _half(h):
    return None if h is None else {k: h[k] for k in ("value", "num", "ok") if k in h}


def _fix_of(r, f, s):
    if r["status"] != "fail":
        return ""
    return f["fix"] or (s or {}).get("fix", "") or "fix the obligations above it first"


# ----------------------------------------------------------------------------- the character row
# ----------------------------------------------------------------------------- S30 upkeep (references/balance-math.md 3b)
UPKEEP_BAND_SHARE = 0.90       # PU1: u inside its band for 90% or more of the play time; designer default, no source
UPKEEP_SWITCHES = 4            # PU4: the default count of switches; designer default, no source
UPKEEP_ROWS = (
    ("PU1", "BM 3b", "The upkeep factor u of the active player stays inside its band for 90% or more of the play time"),
    ("PU2", "BM 3b", "No death spiral: the income of the active player never stops rising for longer than the unlock "
                     "gap target, and the active player reaches the hard end"),
    ("PU3", "BM 3b, L1", "1 care level raises the income by the purchase value floor or more while u < 1"),
    ("PU4", "BM 3b", "The decision does not end: the purchases of the active player switch between the producer lines "
                     "and the care lines the stated count of times or more"),
)


def upkeep_trace(m, sim):
    """[(real seconds, u, income of the target)] from the start and each event of sim."""
    up = m["upkeep"]
    start = [(0.0, upkeep_factor(up, {}), start_sim(m).income()[up["target"]])]
    return start + [(e["real"], e["u"], e["income"][up["target"]]) for e in sim.events if "u" in e]


def trace_spans(trace, t_end):
    """[(u, seconds)] of each step of the trace: u holds from its event to the next event (or to t_end)."""
    ends = [t for t, _, _ in trace[1:]] + [t_end]
    return [(u, end - t) for (t, u, _), end in zip(trace, ends)]


def band_share(trace, band, t_end):
    """(the share of the play time with u inside the band, the lowest u that holds for some time)."""
    spans = trace_spans(trace, t_end)
    total = sum(d for _, d in spans)
    inside = sum(d for u, d in spans if band[0] - EPS <= u <= band[1] + EPS)
    low = min((u for u, d in spans if d > 0), default=1.0)
    return (inside / total if total > 0 else 1.0), low


def income_peaks(trace):
    """The times when the income of the target rises above each earlier value."""
    out, peak = [], -1.0
    for t, _, inc in trace:
        out += [t] if inc > peak * (1 + 1e-9) else []
        peak = max(peak, inc)
    return out


def longest_stall(trace, t_end, finished):
    """The longest time in which the income of the target does not rise above its last peak: (seconds, start). The
    last span counts only when the player does not finish. With too little care, the greedy buyer does not buy a
    producer that lowers u, so a death spiral shows as a stall at u = 1 as well as a fall of u."""
    peaks = income_peaks(trace) + ([] if finished else [t_end])
    return max([(b - a, a) for a, b in zip(peaks, peaks[1:])], default=(0.0, 0.0))


def care_gains(m, sim):
    """(gain, room) of each care purchase of sim: the income gain of the target, and the shortfall 1 / u - 1 before."""
    _, care = upkeep_lines(m["upkeep"])
    c = m["upkeep"]["target"]
    befores = [start_sim(m).income()] + [e["income"] for e in sim.events[:-1]]
    rows = zip(sim.events, befores, u_befores(m, sim))
    return [(e["income"][c] / b[c] - 1.0, 1.0 / ub - 1.0 if ub > 0 else math.inf) for e, b, ub in rows
            if e["kind"] == "purchase" and e["id"] in care and b.get(c, 0.0) > 0]


def switch_count(m, sim):
    """The count of changes between a producer purchase and a care purchase of sim, by hand or by the auto-buy."""
    need, care = upkeep_lines(m["upkeep"])
    kinds = ["care" if e["id"] in care else "need" for e in sim.events
             if e["kind"] == "purchase" and (e["id"] in care or e["id"] in need)]
    return sum(1 for a, b in zip(kinds, kinds[1:]) if a != b)


def upkeep_measure(m, sim):
    up = m["upkeep"]
    trace = upkeep_trace(m, sim)
    share, low = band_share(trace, up.get("band", [0.0, 1.0]), sim.real)
    stall, at = longest_stall(trace, sim.real, sim.axis_done())
    return dict(share=share, low=low, stall_s=stall, stall_at_s=at, care_gains=care_gains(m, sim),
                switches=switch_count(m, sim), finished=sim.axis_done())


def upkeep_row(rid, value, status, mark, fix="", **extra):
    law, text = next((law, text) for i, law, text in UPKEEP_ROWS if i == rid)
    r = extra_row(rid, "upkeep", text, value, status, law=law, fix=fix)
    r.update(mark=mark, **extra)
    return r


def upkeep_rows(m, x, ms):
    """The rows PU1 to PU4 of a model with the key upkeep (S30). PU3 is proved by its formula when each care line is a
    factor line; the other rows are simulated, not proved."""
    up = m.get("upkeep")
    if not up:
        return []
    um = (ms or {}).get("upkeep")
    if not um:
        return [upkeep_row(rid, "no simulation", "not computed", "simulated") for rid, _, _ in UPKEEP_ROWS]
    return [pu1(up, um), pu2(up, x, um), pu3(m, up, um), pu4(up, um)]


def sim_status(ok):
    return SIMULATED if ok else "fail"


def pu1(up, um):
    band = up.get("band", [0.0, 1.0])
    ok = um["share"] >= UPKEEP_BAND_SHARE - EPS
    return upkeep_row("PU1", f"u inside {band[0]:g} to {band[1]:g} for {um['share']:.0%} of the time; lowest u "
                      f"{um['low']:.3f}", sim_status(ok), "simulated", "" if ok else "raise the care capacity "
                      "(base_capacity or the step of a care line), or lower the care cost", limit=f">= "
                      f"{UPKEEP_BAND_SHARE:.0%}")


def pu2(up, x, um):
    limit = 60.0 * x.get("ugap_max", UNLOCK_GAP_MINUTES)
    law = up.get("exponent", 1.0) <= 1.0 + EPS
    ok = um["stall_s"] <= limit + EPS and um["finished"] and law
    why = [] if law else ["exponent > 1: 1 producer level can lower the income (BM 3b)"]
    why += [] if um["stall_s"] <= limit + EPS else [f"a stall of {um['stall_s'] / 60:.1f} min from minute "
                                                  f"{um['stall_at_s'] / 60:.1f}"]
    why += [] if um["finished"] else ["the active player does not reach the hard end"]
    value = f"longest stall {um['stall_s'] / 60:.1f} min (limit {limit / 60:g} min); exponent "             f"{up.get('exponent', 1.0):g}" + ("; " + "; ".join(why) if why else "")
    fix = "" if ok else "raise base_capacity or the care step, or lower the care cost, so that the income can buy "                         "care at the lowest u (BM 3b, the condition for no death spiral)"
    return upkeep_row("PU2", value, sim_status(ok), "simulated", fix, limit=f"<= {limit / 60:g} min")


def care_formula(up):
    """The smallest income gain of 1 care level while u < 1: value^e - 1 for a factor line, None for an add line."""
    e = up.get("exponent", 1.0)
    caps = up.get("capacity") or []
    if not caps or any(c.get("kind") != "mult" for c in caps):
        return None
    return min(c["value"] ** e - 1.0 for c in caps)


def pu3(m, up, um):
    """The formula value^e - 1 >= floor proves the row for factor care lines. The simulation checks that each care
    purchase gains the floor, or closes the whole shortfall 1 / u - 1 when it is smaller."""
    floor = pv_floor(m)[0]
    f = care_formula(up)
    gains = um["care_gains"]
    short = [g for g, room in gains if g < min(floor, room) - 1e-4]
    ok = (f is None or f >= floor - EPS) and not short
    status = "fail" if not ok else ("pass" if f is not None else SIMULATED)
    value = (f"formula {f:.1%}" if f is not None else "formula: none (an add care line)") + \
        f"; {len(gains) - len(short)} of {len(gains)} care purchases gain the floor or close the shortfall"
    return upkeep_row("PU3", value, status, "proved" if f is not None else "simulated",
                      "" if ok else f"raise the care step so that value^e - 1 >= {floor:.0%}", limit=f">= {floor:.0%}")


def pu4(up, um):
    need = int(up.get("min_switches", UPKEEP_SWITCHES))
    ok = um["switches"] >= need
    return upkeep_row("PU4", f"{um['switches']} switches between producer and care purchases", sim_status(ok),
                      "simulated", "" if ok else "make care bind more often: a smaller care step, or more need for "
                      "each producer level", limit=f">= {need}")


CK_STATEMENT = ("The design keeps the character of the content: for each line and reward, the effect values, the cap "
                "and the cost ratio of the game stay within 25%, or move into the proven range")
CK_LIMIT = "changed elements <= 50%"
CK_STRUCT_LIMIT = ", and no structure change that the designer did not accept"


def character_row(m):
    """The row CK (references/proof-sheet.md): the measure 'character kept' of a rebalance (model key character).
    A model with no key character (a design forward, the game as built) has no row."""
    ch = m.get("character")
    if not ch:
        return []
    r = extra_row("CK", "purchase", CK_STATEMENT, character_value(ch), "pass" if ch["verdict"] == "ready" else "fail",
                  law="design review")
    r.update(limit=CK_LIMIT + (CK_STRUCT_LIMIT if "structure" in ch else ""), kept=ch["kept"], moved=ch["moved"], changed=ch["changed"], total=ch["total"],
             share=ch["share"], structure=ch.get("structure", []), fix="" if ch["verdict"] == "ready" else ch["reason"] + ": " + ", ".join(
                 e["id"] for e in ch["elements"] if e["state"] == "changed"))
    return [r]


def character_value(ch):
    text = (f"{ch['kept']} of {ch['total']} elements kept ({ch['share']:.0%}), {ch['moved']} moved into the proven "
            f"range, {ch['changed']} changed")
    return text if "structure" not in ch else text + "; " + structure_value(ch["structure"])


def structure_value(st):
    """The structure line of CK (references/proof-sheet.md): each rule change, reset count change, cap change and
    moved milestone, accepted or not."""
    if not st:
        return "structure: 0 changes"
    return f"structure: {len(st)} changes (" + ", ".join(
        f"{c['name']}: game {c['game']}, model {c['model']}, {'accepted' if c['accepted'] else 'not accepted'}"
        for c in st) + ")"


# ----------------------------------------------------------------------------- the wall table row
WS_STATEMENT = ("Each step of the wall table of the axis after the first is a wall: the ratio of each wall to the wall "
                "before it is inside the g range of S03.exponential; the first step is the start of the game")
WS_LAW = "S03.exponential g (assets/systems.json)"


def wall_ratios(table):
    return [b / a for a, b in zip(table, table[1:]) if a > 0]


def wall_rows(m):
    """The row WS (references/proof-sheet.md): 1 row for each ladder with a wall table of 2 walls or more. The first
    step (wall 1 to wall 2) is the start of the game: the row prints it and does not test it. A later step outside
    the g range of S03.exponential fails the row. A model with no wall table has no row."""
    return [wall_row(lad) for lad in m.get("ladders") or [] if len((lad.get("requires") or {}).get("table") or []) > 1]


def wall_row(lad):
    rs = wall_ratios(lad["requires"]["table"])
    g = range_of("S03.exponential", "g") or [1.0, math.inf]
    out = [i + 2 for i, v in enumerate(rs) if i > 0 and (v < g[0] - EPS or v > g[1] + EPS)]
    r = extra_row("WS", "purchase", WS_STATEMENT, ratio_text(rs, out), "fail" if out else "pass", law=WS_LAW,
                  fix=f"set each wall step of {lad['id']} after the first inside g {fmt(g[0])} to {fmt(g[1])}: fewer "
                      "walls in a flat part, more walls in a steep part" if out else "")
    later = rs[1:]
    r.update(limit=f"{fmt(g[0])} <= step <= {fmt(g[1])} after the first step", first_step=rs[0],
             smallest=min(later, default=None), median=statistics.median(later) if later else None,
             largest=max(later, default=None), outside=out)
    return r


def ratio_text(rs, out):
    first = f"first step x{fmt(rs[0])} (first step, not tested)"
    later = rs[1:]
    if not later:
        return first + "; no later step"
    text = (f"{first}; {len(later)} later steps: smallest x{fmt(min(later))}, median "
            f"x{fmt(statistics.median(later))}, largest x{fmt(max(later))}")
    return text + (f"; {len(out)} outside the range (walls {', '.join(str(i) for i in out[:8])}"
                   f"{' ...' if len(out) > 8 else ''})" if out else "; each later step inside the range")


# ----------------------------------------------------------------------------- the content gap row
CG_STATEMENT = ("The largest gap with no new content element of the active player (a first purchase of a line, a "
                "first milestone or the first reset; an axis step is not one) is inside the FM-1 band of the fun "
                "sheet, in the first hour and over the whole length")
CG_LAW = "FM-1 band of the fun sheet (a target with no source)"


def content_band(m):
    """(whole, first hour) in minutes: the FM-1 band of game.targets.fun, else the default band of the fun sheet."""
    band = dict(pw_fun.SPEC_OF["FM-1"]["band"], **(pw_fun.fun_targets(m).get("FM-1") or {}))
    return float(band["max"]), float(band["max_h1"])


def content_marks(cm, t_end, empty=()):
    """0, the time of each content element up to t_end (first purchases, first milestones, the first reset), and
    t_end. An empty content slot is not a content element."""
    times = {t for t, i in cm["unlocks"] if i not in empty} | ({cm["first_reset"]} if cm.get("first_reset") else set())
    return [0.0] + sorted(t for t in times if 0 < t < t_end - EPS) + [t_end]


def content_gap_rows(m, x, ms):
    """The row CG (references/proof-sheet.md): the content gap of the active player against the FM-1 band. Its kind
    content decides the result. The band is a target with no source, so a gap inside its range is IN_RANGE. A model
    with no play pattern continuous or no simulation has no row."""
    cm = (ms or {}).get("continuous")
    if x.get("pattern") != "continuous" or not cm:
        return []
    band = content_band(m)
    marks = content_marks(cm, content_end(x, cm), x.get("empty_slots") or ())
    gaps = content_gaps(marks, *band)
    fails = [g for g in gaps if zone_of(x, "content_gap", g[2], g[4]) == "fail"]
    r = extra_row("CG", "content", CG_STATEMENT, content_value(marks),
                  "fail" if fails else IN_RANGE if gaps else "pass", law=CG_LAW, fix=content_fix(fails))
    r.update(content_keys(x, band, fails), in_range=[content_note(x, g) for g in gaps if g not in fails])
    return [with_range(r)]


def content_value(marks):
    return (f"largest gap {largest_gap(marks):.2f} min; first hour {largest_gap(first_hour(marks)):.2f} min; "
            f"{len(marks) - 2} content elements")


def content_keys(x, band, fails):
    """The limit, the source and the keys needs and missing of CG."""
    limit = "<= " + about(x, "content_gap", band[0], "min") + "; first hour <= " + about(x, "content_gap", band[1], "min")
    needs = [dict(start_minutes=round(a, 2), end_minutes=round(b, 2), gap_minutes=round(g, 2), more=n)
             for a, b, g, n, _ in fails]
    return dict(limit=limit, source="simulation", needs=needs, missing=sum(g[3] for g in fails))


def content_note(x, gap):
    a, b, g, _, lim = gap
    return miss_text(x, "content_gap", f"{g:.2f} min from {a:.1f} to {b:.1f}", lim, "min")


def content_end(x, cm):
    """The end of the gaps of CG: the hard end of the run, else the end of the marks of PC4."""
    return (end_time(cm, x["hard_end"]) if x.get("hard_end") else None) or pc4_marks(x, cm)[0][-1]


def content_gaps(marks, whole_max, first_max):
    """The long gaps of the first hour (they end at minute 60), then the long gaps that end after it. Each gap is
    (start, end, gap, more elements, its limit)."""
    early = long_gaps(first_hour(marks), first_max)
    later = later_gaps(long_gaps(marks, whole_max), {g[0] for g in early}, whole_max)
    return [g + (first_max,) for g in early] + [g + (whole_max,) for g in later]


def first_hour(marks):
    """The marks of the first hour, with the end at minute 60 (or at the end of the run when it is shorter)."""
    return [t for t in marks if t < HOUR - EPS] + [min(HOUR, marks[-1])]


def later_gaps(whole, early_starts, limit_min):
    """The long gaps that end after the first hour. A gap that starts in a long gap of the first hour counts from
    minute 60 only, because the count of the first hour already fills its first part."""
    h = HOUR / 60.0
    cut = [cut_gap(w, h, limit_min) if w[0] < h and w[0] in early_starts else w for w in whole if w[1] > h + EPS]
    return [w for w in cut if w[3] > 0]


def cut_gap(gap, h, limit_min):
    """The gap (a, b, g, n) from minute h only."""
    b = gap[1]
    return h, b, b - h, int(math.ceil((b - h) / limit_min - EPS)) - 1


def content_fix(gaps):
    if not gaps:
        return ""
    return "add content or move an element into each gap: " + "; ".join(
        f"{n} more from minute {a:.1f} to minute {b:.1f}" for a, b, _, n, _ in gaps)


# ----------------------------------------------------------------------------- extra rows
def extra_row(rid, kind, statement, value, status, law="", fix="", uses=""):
    r = base_row("PO1")
    r.update(id=rid, kind=kind, statement=statement, value=value, status=status, law=law, fix=fix, uses=uses,
             source="", limit="")
    return r


def sim_only_rows(m, x):
    """Systems that the laws do not cover (section 7 of the balance math): the simulation alone checks them."""
    keep = {x["econ"], x.get("spine_cur"), (m.get("prestige") or {}).get("currency")}
    rows = [_currency_row(c, ids) for c, ids in foreign_lines(m, x, keep).items()]
    rows += [extra_row(f"limit 4: {p['id']}", "simulation only", "A line whose cost is not geometric",
                       p["costs"][0]["formula"], "simulation only", "section 7.4")
             for p in m["purchases"] if p["costs"][0]["formula"] != "exp"]
    rows += [extra_row(f"limit 9: {l['id']}", "simulation only", "A ladder step with a time cost or a visit",
                       "timer or needs_visit", "simulation only", "section 7.9")
             for l in m["ladders"] if any(k in l for k in ("attempt_seconds", "cooldown_seconds", "needs_visit"))]
    return rows


def foreign_lines(m, x, keep):
    targets = {x["econ"], x.get("spine_cur"), "all"}
    hits = [(p["costs"][0]["currency"], p["id"]) for p in m["purchases"]
            if p["costs"][0]["currency"] not in keep and mult_on(p, targets)]
    return {c: [i for cc, i in hits if cc == c] for c, _ in hits}


def _currency_row(cur, ids):
    return extra_row(f"limit 2: {cur}", "simulation only", "Several currencies with conversion between them",
                     f"lines paid in {cur}: " + ", ".join(ids), "simulation only", "section 7.2")


def model_params(m, x):
    """(name, value, variants, line, use) for each model parameter that a catalog variant gives a range."""
    out = []
    for p in m["purchases"]:
        out += line_params(p, x)
    if x.get("has_reset") and x.get("p") is not None:
        out.append(("p", x["p"], ("S04.power",), None, "p"))
    out += [("g", x["w"], ("S03.exponential",), x["axis"], "g")] if x.get("w") else []
    return out + offline_params(m, x)


def line_params(p, x):
    """The range rows of 1 line, against the card of its own system: a line of the reset currency against S15, a
    factor line of the main income against S02, an add line against S01. A single purchase (max_level 1: its cost
    ratio has no effect) and a line that feeds only a side currency (no card proves it, BM section 7 limit 2) get
    no range row."""
    c = p["costs"][0]
    reset_cur = x.get("reset_cur")
    if not has_range_row(p, c, x):
        return []
    d = mult_on(p, {e["target"] for e in p["effects"]})
    if c["currency"] == reset_cur:
        return [("b", c["growth"], ("S15.geometric",), p["id"], "reward_line")]
    if d:
        use = "econ_line" if mult_on(p, (x["econ"], "all")) else "other"
        return [("b", c["growth"], ("S02.levelled_factor",), p["id"], use),
                ("d", d, ("S02.levelled_factor",), p["id"], use)]
    return additive_params(p, c)


def has_range_row(p, c, x):
    """False for a cost that is not exp, a single purchase, and a line that feeds only a side currency."""
    if c["formula"] != "exp" or p.get("max_level") == 1:
        return False
    on_econ = any(e["target"] in (x["econ"], "all") for e in p["effects"] if e["kind"] != "cap")
    return c["currency"] == x.get("reset_cur") or on_econ


def additive_params(p, c):
    pct = [e["value"] for e in p["effects"] if e["kind"] == "pct"]
    if pct:
        return [("b", c["growth"], ("S02.add_in_group",), p["id"], "other"),
                ("v", max(pct), ("S02.add_in_group",), p["id"], "other")]
    if any(e["kind"] == "add" for e in p["effects"]):
        return [("b", c["growth"], ("S01.geometric", "S01.steep", "S01.ladder"), p["id"], "other")]
    return []


def offline_params(m, x):
    if m["game"]["offline_rate"] <= 0:
        return []
    out = [("f_closed", x["f_closed"], ("S06.penalty_cap",), None, "f_closed")]
    return out + ([("T_cap_h", x["closed_cap_h"], ("S06.penalty_cap",), None, "T_cap_h")]
                  if x.get("closed_cap_h") is not None else [])


def range_rows(params):
    checked = [(o, [r for r in (range_of(v, o[0]) for v in o[2]) if r]) for o in params]
    return [_range_row(*o, rngs) for o, rngs in checked if rngs and not _inside(o[1], rngs)]


def _inside(value, rngs):
    return any(lo - EPS <= value <= hi + EPS for lo, hi in rngs)


def _range_row(name, value, variants, line, use, rngs):
    where = f"{name} of {line}" if line else name
    band = " or ".join(f"{fmt(lo)} to {fmt(hi)}" for lo, hi in rngs)
    uses = PARAM_USES.get(use, "")
    r = extra_row(f"range: {where}", "range", f"{where} is inside its proven range ({'/'.join(variants)})",
                  fmt(value), "outside proven range", uses=uses)
    r["limit"] = band
    r["reason"] = ("used by " + uses) if uses else "no obligation uses it"
    return r


# ----------------------------------------------------------------------------- systems mode
def systems_inputs(mp, r_given, pattern):
    reset = mp.get("reset") or {}
    x = dict(mode="systems", pattern=pattern, X_e=X_E, X_p=X_P, Dg=DG_DEFAULT, k=DEFAULT_SPEED, has_reset=bool(reset.get("found")),
             reset_cur=reset.get("currency"), sigma=None, A=None, c=None, H=None, w=None, N=None, axis=None,
             idle_targets={}, absence_h=ABSENCE_HOURS, u_band=None, spine_income=False, a1=None, I0=None)
    x["econ"] = map_econ(mp, x["reset_cur"])
    map_lines(mp, x, reset.get("multiplies"), r_given)
    x["p"] = reset.get("p") if reset.get("gain_family") == "power" else None
    x["q"] = reset.get("q")
    x["c"] = reset.get("divisor") if x["p"] is not None else None
    map_presence(mp, x)
    return x


def map_econ(mp, reset_cur):
    counts = collections.Counter(ln["currency"] for ln in mp.get("lines") or []
                                 if ln.get("currency") and ln["currency"] != reset_cur)
    return counts.most_common(1)[0][0] if counts else None


def map_lines(mp, x, stat, r_given):
    lines = map_line_rows(mp, x["econ"], stat)
    x["econ_lines"] = lines
    bound = sum(ln["r"] for ln in lines) if stat else None
    after = sum(ln["r"] for ln in lines if ln["cap"] is None) if stat else None
    given = r_given is not None
    x["R_max"], x["R_end"] = (r_given, r_given) if given else (bound, after)
    x["R_source"] = "--R" if given else "upper bound from the lines of the map"


def map_line_rows(mp, econ, stat):
    return [dict(id=ln["id"], r=ln["r"], cap=ln.get("cap"), b=ln["cost_ratio"], d=ln.get("value_ratio"))
            for ln in mp.get("lines") or []
            if ln.get("currency") == econ and ln.get("stat") == stat and ln.get("r") is not None]


def map_offline(off, x):
    """f_closed and the closed cap of the offline rule of the map. A null rate stays None: not read."""
    found_off = off.get("found") not in (None, False, "no")
    rate = off.get("rate") if found_off else 0.0
    x["f_closed"], x["closed_cap_h"] = rate, off.get("cap_hours")
    x["no_offline"] = not found_off or (rate is not None and rate <= EPS)


def map_presence(mp, x):
    found = {s["id"]: s["found"] for s in mp.get("systems") or []}
    map_offline(mp.get("offline") or {}, x)
    active = found.get("S05") == "no"
    x["f_open"] = 1.0 if active else None
    gate_missing = x.get("pattern") == "check_in" and any(e["kind"] == "slot" for e in missing_core(mp, "check_in"))
    x["tools"] = [] if gate_missing and active else None
    x["open_cap_known"] = False
    x["V"] = None


def _systems_data():
    path = SYSTEMS_JSON
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def pattern_core(pattern):
    """(core system ids, presence gate slot or None) of 1 play pattern of assets/systems.json."""
    data = _systems_data()
    pp = {p["id"]: p for p in data["play_patterns"]}[pattern]
    return pp["core"], (data["core_slot"] if pp.get("gate_min") else None)


def missing_core(mp, pattern):
    """Each core system of the pattern that the map does not show as found, and the gate slot when it is core and
    no member is found. A system that the map does not hold (an older scan) counts as missing."""
    core, slot = pattern_core(pattern)
    by = {s["id"]: s for s in mp.get("systems") or []}
    out = [_missing_system(sid, by.get(sid)) for sid in core if (by.get(sid) or {}).get("found", "no") == "no"]
    gate = slot and not any((by.get(g) or {}).get("found") == "yes" for g in slot["members"])
    return out + ([dict(slot, kind="slot")] if gate else [])


def _missing_system(sid, s):
    name = s["name"] if s else CORE_NAMES.get(sid, sid)
    return dict(kind="system", id=sid, name=name, scanned=s is not None)


CORE_NAMES = {"S24": "Automation", "S28": "Hard end", "S05": "Active action"}


def core_rows(mp, pattern):
    if pattern:
        return [_core_row(e, "fail", f"core: {_who(e)}") for e in missing_core(mp, pattern)]
    rows = [extra_row("play pattern", "core", "The play pattern is set (--pattern continuous or --pattern check_in)",
                      "not set", "fail", fix=NO_PATTERN_FIX)]
    for pat in PATTERNS:
        rows += [_core_row(e, "pattern not set", f"core ({pat}): {_who(e)}") for e in missing_core(mp, pat)]
    return rows


def _who(e):
    return e.get("id") or "/".join(e.get("members") or [])


def _core_row(e, status, rid):
    name = e.get("name") or e.get("slot")
    value = "missing" if e.get("scanned", True) else "missing (the scan does not test it: confirm in the code)"
    return extra_row(rid, "core", f"The game has the core system or slot '{name}'", value, status,
                     fix=f"add {name} (default {e.get('default') or _who(e)})")


PATTERNS = ("continuous", "check_in")
NO_PATTERN_FIX = ("give the play pattern of the game: --pattern continuous (Steam, desktop, long sittings) or "
                  "--pattern check_in (mobile, offline gains). The rows below show the missing core systems of both")


def systems_sim_rows(mp, x):
    rows = []
    foreign = sorted({ln["currency"] for ln in mp.get("lines") or []
                      if ln.get("currency") not in (x["econ"], x["reset_cur"]) and ln.get("value_key") == "mult"})
    rows += [_currency_row(c, [ln["id"] for ln in mp["lines"] if ln.get("currency") == c]) for c in foreign]
    return rows + [_uncovered_row(s) for s in mp.get("systems") or [] if _uncovered(s)]


def _uncovered(s):
    v = proven_ranges().get(s.get("variant") or "") or {}
    return s.get("found") == "yes" and v.get("r") == UNCOVERED


def _uncovered_row(s):
    return extra_row(f"limit: {s['id']}", "simulation only", f"{s['name']} ({s['variant']})", UNCOVERED,
                     "simulation only", "section 7")


def systems_range_rows(mp, x):
    rows = []
    econ_ids = {ln["id"] for ln in x["econ_lines"]}
    for o in mp.get("out_of_range") or []:
        use = map_use(o, econ_ids)
        rows.append(_range_row(o["name"], o["value"], (o["variant"],), o.get("line"), use,
                               [(o["min"], o["max"])]))
    return rows


def map_use(o, econ_ids):
    if o.get("line") in econ_ids:
        return "econ_line"
    if o["variant"].startswith("S15"):
        return "reward_line"
    return {"p": "p", "g": "g", "f_closed": "f_closed", "T_cap_h": "T_cap_h"}.get(o["name"], "other")


# ----------------------------------------------------------------------------- the sheet
class Sheet:
    """The rows, the inputs, and the verdict of 1 proof sheet."""

    def __init__(self, mode, source, x, strict):
        self.mode, self.source, self.x, self.strict = mode, source, x, strict
        self.rows, self.speed = [], None

    def verdict(self):
        obligations = [r for r in self.rows if r["kind"] in READY_KINDS]
        core_ok = all(r["status"] != "fail" for r in self.rows if r["kind"] == "core")
        ranges = [r for r in self.rows if r["kind"] == "range"]
        all_in = not ranges
        used_out = any(r["uses"] for r in ranges)
        ready = core_ok and all(r["status"] in READY_STATUSES for r in obligations)
        unproved = any(r["status"] in (SIMULATED, STUB) for r in self.rows)
        return ready and not unproved and not (self.strict and used_out), all_in

    def result(self):
        """ready, simulated or not ready. simulated: each row that decides the verdict passes or meets its limit in
        the simulation, 1 or more rows or parts are SIMULATED, and no part is a stub."""
        if self.verdict()[0]:
            return "ready"
        held = Sheet(self.mode, self.source, self.x, self.strict)
        held.rows = [dict(r, status="pass" if r["status"] == SIMULATED else r["status"]) for r in self.rows]
        stub = any(r["status"] == STUB for r in self.rows)
        return "simulated" if not stub and held.verdict()[0] else "not ready"

    def as_json(self):
        ready, all_in = self.verdict()
        return dict(schema_version=SCHEMA_VERSION, mode=self.mode, source=self.source, ready=ready,
                    result=self.result(), result_text=RESULT_TEXT[self.result()],
                    in_range_rows=[r["id"] for r in self.rows if r["status"] == IN_RANGE],
                    all_in_proven_ranges=all_in, strict_ranges=self.strict,
                    inputs=public_inputs(self.x),
                    rows=self.rows, findings=[f"{r['id']}: {r['finding']}" for r in self.rows if r.get("finding")],
                    speed=self.speed)


INPUT_KEYS = ("econ", "R_max", "R_end", "R_source", "p", "q", "c", "A", "alpha", "s", "s_end", "lam", "X_e", "X_p",
              "Dg", "sigma", "T_X", "f_open", "f_closed", "kappa", "kappa_closed", "V", "open_cap_h", "closed_cap_h",
              "absence_h", "I0", "a1", "w", "N", "k", "H", "ldot", "u_model")
LONG_KEYS = ("g_run", "T_e", "k_mid", "R_count", "R_content")                  # the inputs of 1 long run, only when it has them


def public_inputs(x):
    out = {k: _clean(x.get(k)) for k in INPUT_KEYS}
    out["econ_lines"] = [dict(ln, r=round(ln["r"], 4)) for ln in x.get("econ_lines") or []]
    out["measured"] = x.get("measured")
    out.update({k: _clean(x[k]) for k in LONG_KEYS if x.get(k) is not None})
    if abs((x.get("F_ratio") or 1.0) - 1.0) > EPS:
        out.update(F_ratio=_clean(x["F_ratio"]), F_held=_clean(x["F_held"]), F_traj=_clean(x["F_traj"]),
                   A_traj=_clean(x["A_traj"]))
    return out


def _clean(v):
    if isinstance(v, float):
        return None if math.isnan(v) else ("inf" if math.isinf(v) else round(v, 6))
    return v


def model_sheet(path, speed, strict):
    m = load_model(path)
    x = model_inputs(m)
    x["reset_cur"] = (m.get("prestige") or {}).get("currency") if x["has_reset"] else None
    step_inputs(m, x)
    if long_law(x) and not x.get("step_law"):
        x["g_run"] = run_growth(m, x["econ"], m["game"]["target_hours"])
    derive(x)
    ms = measure(m, x, speed)
    x["floor_bound"] = run_floor_bound(x, ms)
    if x["has_reset"] and x["alpha"]:
        held_A(m, x, ms)
        derive(x)
    x["factor_bound"] = factor_bound(x)
    x["walls_fitted"] = walls_fitted(m)
    x["measured"] = measured_summary(ms)
    sheet = Sheet("model", path, x, strict)
    sheet.rows = model_rows(m, x, ms)
    sheet.speed = speed_block(x, ms) if speed else None
    return sheet


def model_rows(m, x, ms):
    rows = [obligation_row(pid, x, ms) for pid in STATEMENT] + pattern_rows(x, ms) + [purchase_value_row(m, x, ms)]
    rows += line_life_rows(x, ms)
    rows += wall_rows(m) + character_row(m) + content_gap_rows(m, x, ms) + upkeep_rows(m, x, ms)
    return rows + cap_rows(m) + sim_only_rows(m, x) + path_rows(m) + range_rows(model_params(m, x))


PART_STATUS = {"simulated": SIMULATED, "not written": NOT_MODELLED}


def path_rows(m):
    """1 row for each part of simulation_path that no law proves (references/simulation-path.md): a simulated part,
    a stub that a person must fill, and a part that the model does not need."""
    parts = (m.get("simulation_path") or {}).get("parts") or []
    return [part_row(p) for p in parts if p.get("mark") != "law"]


def part_row(p):
    label = p.get("variant") or p.get("name") or p.get("system")
    status = STUB if p.get("fill") else PART_STATUS.get(p.get("mark"), NOT_MODELLED)
    value = "entries: " + ", ".join(p.get("entries") or []) if p.get("entries") else "no entry in the model"
    r = extra_row(f"part: {label}", "simulated", f"The part {label} ({p.get('name') or p.get('system')}) is "
                  f"{p.get('mark')}", value, status, fix=p.get("to_fill", "") if p.get("fill") else "")
    r["reason"] = p.get("reason", "")
    return r


def held_A(m, x, ms):
    """A of the runs (BM L6 with F): A of the trajectory with no reset, times F held at the median later run of
    early_reset over F of that trajectory. The trajectory of 1e6 s reaches each persistent reward, and the runs hold
    only the rewards of their time, so a game with large persistent rewards needs this ratio."""
    A, F_traj = trajectory_A_F(m, x["econ"], x["alpha"])
    x["A_traj"], x["F_traj"], x["F_held"] = A, F_traj, ms.get("F_held")
    ratio = ms["F_held"] / F_traj if A and ms.get("F_held") and F_traj else 1.0
    x["F_ratio"] = ratio
    x["A"] = A * ratio if A else A


def measured_summary(ms):
    keys = ("policy_gap", "length_hours", "unlocks_first_hour", "slowdown", "T_sim", "lambda_sim", "median_gap",
            "largest_gap", "early_unlocks", "P1h", "phases", "runs_early")
    out = {k: _clean(ms.get(k)) for k in keys}
    curve = (ms.get("block") or {}).get("idle_curve") or {}
    out["idle"] = {s: {str(h): v for h, v in ms["idle"][s].items()} for s in IDLE_STATES if s in curve or not curve}
    return out


def speed_block(x, ms):
    tx = x.get("T_X")
    st = ms.get("speed_test") or {}
    out = dict(human_decision_seconds=D_H, rule="T_X >= 10 x k x D_h", T_X=_clean(tx),
               largest_speed_formula=formula_speed(x, tx),
               largest_speed_simulation=st.get("largest_speed"), simulation_rows=st.get("rows"))
    below = 1 if st.get("rows") and st.get("largest_speed") is None else None   # no tested speed is under the limit
    out["play_test_speed"] = _smaller(out["largest_speed_formula"], out["largest_speed_simulation"] or below)
    return out


def formula_speed(x, tx):
    """The largest speed of the formula of PO13: T_X of L6, or T_e of 1 long run (L9)."""
    if long_law(x) and x.get("T_e"):
        return long_kmax(x)
    return None if tx is None or unstable(x) else math.floor(tx / (SPEED_FACTOR * D_H))


def _smaller(a, b):
    known = [k for k in (a, b) if k is not None]
    return min(known) if known else None


def systems_sheet(path, r_given, strict, pattern):
    with open(path, encoding="utf-8") as f:
        mp = json.load(f)
    x = systems_inputs(mp, r_given, pattern)
    derive(x)
    sheet = Sheet("systems", path, x, strict)
    sheet.rows = core_rows(mp, pattern) + [obligation_row(pid, x, None) for pid in STATEMENT]
    sheet.rows += systems_sim_rows(mp, x) + systems_range_rows(mp, x)
    return sheet


# ----------------------------------------------------------------------------- output
def cell(v):
    return str(v).replace("|", "/").replace("\n", " ")


def table_lines(rows):
    head = ["| Id | Obligation | Value | Limit | Status | Law | Source | Fix or reason |", "|" + " --- |" * 8]
    return head + [_table_row(r) for r in rows]


def _table_row(r):
    vals = (r["id"], r["statement"], r["value"], r["limit"], r["status"], r["law"], r["source"], r["fix"] or r["reason"])
    return "| " + " | ".join(cell(v) for v in vals) + " |"


def sheet_lines(sheet):
    data = sheet.as_json()
    head = [f"# Proof sheet: {os.path.basename(sheet.source)} ({sheet.mode})", "",
            f"Result: {data['result']}. Ready: {'yes' if data['ready'] else 'no'}. All parameters in proven ranges: "
            f"{'yes' if data['all_in_proven_ranges'] else 'no'}." + (" Strict ranges: on." if sheet.strict else ""),
            "", f"Meaning: {data['result_text']}.", "", in_range_line(data["in_range_rows"]), ""]
    body = table_lines(sheet.rows) + [""] + clause_lines(sheet.rows) + finding_lines(data) + input_lines(sheet.x)
    return head + body + speed_lines(sheet.speed)


def in_range_line(ids):
    """The line that counts the target rows past their limit and inside their range."""
    return (f"Rows in range: {len(ids)}" + (f" ({', '.join(ids)})" if ids else "") + ". A row in range passes: its "
            "value is past the target and inside the range of the target. Many such rows show a design at the edge.")


def clause_lines(rows):
    out = [f"- {r['id']}: {c['text']}: {'ok' if c['ok'] else 'not run' if c['ok'] is None else 'FAIL'}"
           for r in rows for c in r["clauses"]]
    return (["## Extra conditions", ""] + out + [""]) if out else []


def finding_lines(data):
    return (["## Findings", ""] + [f"- {f}" for f in data["findings"]] + [""]) if data["findings"] else []


def input_lines(x):
    keys = ("econ", "R_max", "R_end", "p", "q", "alpha", "s", "lam", "A", "c", "T_X", "sigma", "f_open", "f_closed",
            "kappa", "u_model", "w", "N", "H", "k")
    return ["## Inputs", "", ", ".join(f"{k} = {fmt(x.get(k), 4)}" for k in keys), ""]


def speed_lines(sp):
    if not sp:
        return []
    return ["## Play-test speed (R13)", "",
            f"Largest speed that keeps PO13 (formula, T_X / (10 x {D_H:g} s)): x{fmt(sp['largest_speed_formula'])}.",
            f"Largest tested speed with a slow-down under {pw_players.SLOWDOWN_MAX:g} (simulation): "
            f"x{fmt(sp['largest_speed_simulation'])}.", f"Play-test speed: x{fmt(sp['play_test_speed'])}, the smaller "
            "value. The formula counts about 1 human decision for each run. The simulation counts each purchase.", ""]


def print_lines(lines):
    out = sys.stdout
    for line in lines:
        out.write(line + "\n")


# ----------------------------------------------------------------------------- entry point
ARGS = (
    (("model",), dict(nargs="?", help="the model JSON")),
    (("--systems",), dict(metavar="FILE", help="the catalog map of scan_godot.py --systems-out")),
    (("--R",), dict(type=float, dest="R", help="with --systems: the R of the run phase with the reset layer")),
    (("--pattern",), dict(choices=PATTERNS, help="with --systems: the play pattern of the game. It sets the core "
                                                 "set. With no --pattern, the sheet shows the core sets of both")),
    (("--json",), dict(metavar="FILE", dest="json_out", help="write the sheet as JSON")),
    (("--speed",), dict(action="store_true", help="add the largest play-test speed that keeps PO13 (R13)")),
    (("--strict-ranges",), dict(action="store_true", dest="strict",
                                help="a parameter outside its proven range that an obligation uses makes the sheet "
                                     "not ready")),
)


def parse(argv):
    ap = argparse.ArgumentParser(description="The 13 proof obligations of a model or of a scanned game.")
    for names, kw in ARGS:
        ap.add_argument(*names, **kw)
    a = ap.parse_args(argv)
    bad = [(not a.model) == (not a.systems), a.R is not None and not a.systems, a.speed and bool(a.systems),
           bool(a.pattern) and not a.systems]
    if any(bad):
        ap.error(("give 1 input: a model or --systems" if bad[0] else USAGE_ERRORS[bad.index(True) - 1]))
    return a


def build(a):
    if a.systems:
        return systems_sheet(a.systems, a.R, a.strict, a.pattern)
    return model_sheet(a.model, a.speed, a.strict)


def safe_build(a):
    """The sheet, or None after an error message for a missing file, bad JSON or a model error."""
    try:
        return build(a)
    except (OSError, ValueError, KeyError) as e:
        sys.stderr.write(f"pw_proof: cannot read the input: {e}\n")
    except SystemExit as e:
        sys.stderr.write(f"{e}\n")
    return None


def write_json(path, sheet):
    data = sheet.as_json()
    if not path:
        return
    with open(path, "w", encoding="utf-8") as f:
        json.dump(data, f, indent=1)


def main(argv=None):
    a = parse(argv)
    sheet = safe_build(a)
    if sheet is None:
        return 2
    print_lines(sheet_lines(sheet))
    write_json(a.json_out, sheet)
    return EXIT_CODES[sheet.result()]


if __name__ == "__main__":
    sys.exit(main())
