#!/usr/bin/env python3
"""The design solver: from the targets of a designer and the selected systems to a proved model.

    python3 pw_design.py <selection.json> <targets.json> [--out MODEL] [--report FILE] [--proof FILE]

The solver follows the design procedure of references/balance-math.md section 3. It takes each
parameter from a proven range of assets/systems.json, writes a model (references/model-schema.md),
and runs the proof sheet (scripts/pw_proof.py --strict-ranges) on it. It does not compute an
obligation itself: the proof sheet decides.

references/design-solver.md explains the inputs, the order of the solve and the report.
Exit code: 0 when the model is ready, 1 when no model inside the proven ranges is ready (the report
states the conflict), 2 for bad input.
"""
import argparse
import copy
import itertools
import json
import math
import operator
import os
import statistics
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import pw_fun  # noqa: E402
import pw_players  # noqa: E402
import pw_proof  # noqa: E402
import pw_rebalance as rb  # noqa: E402
import pw_timing  # noqa: E402
from pw_model import load_model  # noqa: E402

SKILL = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SYSTEMS_JSON = os.path.join(SKILL, "assets", "systems.json")
HOUR = 3600.0
CORE = ("S01", "S02", "S03", "S04", "S05", "S06")
PATTERNS = ("continuous", "check_in")
ECON, RESET_CUR, BANK_CUR, AXIS = "coin", "seed", "dew", "stage"
X_E, X_P = pw_proof.X_E, pw_proof.X_P
S_CANDIDATES = (1.1, 1.15, 1.2, 1.25, 1.3, 1.4, 1.5, 1.75, 2.0, 2.5, 3.0)
GEN_COST, GEN_VALUE, GEN_CAP = 10.0, 0.5, 40
GROUP_COSTS = (100.0, 1000.0)
BANK_LINE_CAP = 1
FIRST_TIMES_MIN = (0.5, 2.0, 5.0, 10.0, 25.0, 45.0)
VISIT_MINUTES = 2
SUPPORTED = {"S01": ("S01.geometric", "S01.steep", "S01.ladder"), "S02": ("S02.levelled_factor",),
             "S03": ("S03.exponential",), "S04": ("S04.power",), "S05": ("S05.active_lane",),
             "S06": ("S06.penalty_cap",), "S08": ("S08.timed_bank",), "S15": ("S15.geometric",),
             "S28": ("S28.last_step",)}
MODELLED = tuple(SUPPORTED) + ("S07", "S09", "S30")
UPKEEP_WRITTEN = ("S30.need_capacity",)
CARE_ROLE = "care"
UNIT_PARAM = {"S01.ladder": "b_i"}
PLACE_HEADROOM = 2
UNLOCK_SHARE = 0.8
PART_HOURS = pw_proof.PART_HOURS
LONG_RUN_OUT = ("S22", "S29")
LONG_R = 1.0


class InputError(Exception):
    """Bad input: exit code 2."""


class Conflict(Exception):
    """No model inside the proven ranges meets a target: exit code 1."""

    def __init__(self, target, obligation, rng, ways):
        super().__init__(target)
        self.target, self.obligation, self.rng, self.ways = target, obligation, rng, ways


class VariantConflict(Conflict):
    """A selected variant that no law of the solver covers: the solver takes the simulation path (way out 2)."""


def check(bad, msg):
    if not bad:
        return
    raise InputError(msg)


def read_json(path, label):
    try:
        data = _load_json(path)
    except (OSError, ValueError) as e:
        raise InputError(f"cannot read the {label} file {path}: {e}")
    return data


def _load_json(path):
    f = open(path, encoding="utf-8")
    with f:
        return json.load(f)


def write_file(path, text):
    f = open(path, "w", encoding="utf-8")
    with f:
        f.write(text)


class Catalog:
    """assets/systems.json, with lookups by system id and variant id."""

    def __init__(self, path=SYSTEMS_JSON):
        self.data = read_json(path, "catalog")
        self.systems = {s["id"]: s for s in self.data["systems"]}
        self.variants = {v["id"]: (s["id"], v) for s in self.data["systems"] for v in s["variants"]}
        self.bands = {b["id"]: b for b in self.data["target_bands"]}
        self.pattern = {p["id"]: p for p in self.data["play_patterns"]}

    def variant(self, vid):
        return self.variants[vid][1]

    def param(self, vid, name):
        return self.variant(vid)["parameters"][name]

    def starter(self, sid):
        return next((k for k in self.data["starter_sets"] if k["id"] == sid), None)


def pattern_of(targets):
    pat = targets.get("play_pattern")
    check(pat not in PATTERNS, f"the targets file needs play_pattern: one of {', '.join(PATTERNS)}")
    return pat


def chosen_systems(sel, cat):
    """{system id: variant id} from the selection record: the variant of the first entry of each system. A starter
    set fills each system that the record does not name. A null variant of a starter set takes the default variant
    (default_variant)."""
    out = {}
    for e in selection_entries(sel, cat):
        out.setdefault(e["system"], e["variant"])
    return out


def selection_entries(sel, cat):
    """The entries of the selection record (references/design-solver.md, "The selection record"): each entry is
    {id, system, variant, role, parameters}. A system can have more than 1 entry; each entry then needs its own id.
    The entries of the record for a system replace the entry of the starter set. An old record (1 entry for each
    system, no id) gives the id of the system in lower case."""
    rec = [read_entry(e, cat) for e in sel.get("selected") or []]
    named = {e["system"] for e in rec}
    out = [new_entry(s, v) for s, v in starter_systems(sel, cat).items() if s not in named] + rec
    refused = sel.get("refused") or []
    out = [e for e in out if e["system"] not in refused]
    check_entry_ids(out)
    return out


def new_entry(sid, vid, eid=None, role=None, params=None):
    return dict(id=eid or sid.lower(), system=sid, variant=vid, role=role, parameters=params or {})


def read_entry(entry, cat):
    vid = entry.get("variant")
    check(vid not in cat.variants, f"unknown variant {vid!r}")
    check(cat.variants[vid][0] != entry.get("system"), f"variant {vid} is not a variant of {entry.get('system')}")
    check(entry.get("role") is not None and not isinstance(entry.get("role"), str), f"the role of {vid} must be text")
    return new_entry(entry["system"], vid, entry.get("id"), entry.get("role"), entry.get("parameters"))


def check_entry_ids(entries):
    """Each id once. 2 entries of 1 system with no id have the same id, so the message asks for an id."""
    ids = [e["id"] for e in entries]
    twice = next((e for e in entries if ids.count(e["id"]) > 1), None)
    check(twice is not None, f"2 entries have the id {(twice or {}).get('id')!r}: give each entry of "
                             f"{(twice or {}).get('system')} its own id")


def check_roles(entries, chosen):
    """The role care: an S02 entry whose lines are the care lines of S30 (the upkeep)."""
    care = [e for e in entries if e.get("role") == CARE_ROLE]
    bad = next((e for e in care if e["system"] != "S02"), None)
    check(bad is not None, f"entry {(bad or {}).get('id')}: the role care is for an S02 entry only")
    check(bool(care) and "S30" not in chosen, f"entry {care[0]['id'] if care else ''}: the role care needs S30 "
                                              "(Upkeep) in the selection")


def starter_systems(sel, cat):
    sid = sel.get("starter_set")
    if not sid:
        return {}
    k = cat.starter(sid)
    check(k is None, f"unknown starter set {sid!r}")
    chosen = dict(k["systems"])
    chosen.update(k["added_by_rule"])
    return {s: v or default_variant(cat.systems[s]) for s, v in chosen.items()}


def default_variant(system):
    """The recommended variant of the interview, else the first variant of the card (interview section 7)."""
    return system.get("recommended") or next(v["id"] for v in system["variants"] if v["kind"] == "variant")


def parameter_overrides(sel, cat):
    """{variant id: {name: value}} from the starter set and the record."""
    out = {}
    k = cat.starter(sel.get("starter_set") or "")
    for vid, vals in ((k or {}).get("parameters") or {}).items():
        out.setdefault(vid, {}).update(vals)
    for entry in sel.get("selected") or []:
        out.setdefault(entry["variant"], {}).update(entry.get("parameters") or {})
    return out


def validate(chosen, pattern, cat):
    """The rules of step 2. Each failure names its rule."""
    rules = cat.pattern[pattern]
    for sid in rules["core"]:
        check(not chosen.get(sid), f"rule core ({pattern}): the core system {sid} "
                                   f"({cat.systems[sid]['name']}) has no variant")
    gates = [s for s in cat.data["core_slot"]["members"] if s in chosen]
    check(len(gates) < rules["gate_min"], f"rule presence gate ({pattern}): select 1 or more of "
                                           f"{', '.join(cat.data['core_slot']['members'])}")
    check_no_reset(chosen, pattern, rules)
    for sid in chosen:
        check_needs(sid, chosen, cat)
        check_conflicts(sid, chosen, cat)


def check_no_reset(chosen, pattern, rules):
    """Decisions D10 and D11 (rule R11): a continuous game with no reset layer needs S29 or S22 for permanent
    progress, or S28 for 1 long run to a hard end."""
    if chosen.get("S04") not in (None, "S04.none") or pattern != "continuous":
        return
    need = rules.get("no_reset_needs") or []
    check(not set(need) & set(chosen), f"rule no reset ({pattern}): with S04.none or no S04, select 1 of "
                                       f"{', '.join(need)} (decision D10)")


def check_needs(sid, chosen, cat):
    unmet = [g for g in cat.systems[sid]["needs"] if not set(g) & set(chosen)]
    check(bool(unmet), f"rule needs: {sid} needs 1 of {', '.join(unmet[0] if unmet else [])}")


def check_conflicts(sid, chosen, cat):
    hits = [c for c in cat.systems[sid]["conflicts"] if c["with"] in chosen and conflict_applies(c, chosen)]
    check(bool(hits), conflict_text(sid, hits))


def conflict_text(sid, hits):
    c = hits[0] if hits else dict(condition="", source="")
    return f"rule conflicts: {sid} conflicts with {c.get('with')} when {c['condition']} ({c['source']})"


def conflict_applies(c, chosen):
    """The 2 conditional conflicts of the catalog. The solver writes neither case, so each applies only
    when the selection asks for it: S06 at 100% with no cap, or an S15 line with r >= 1."""
    return False


def target_values(tg):
    t = tg.get("targets") or {}
    length, session, unlocks = t.get("length_h") or {}, t.get("session_min") or {}, t.get("unlocks") or {}
    gap, speed = t.get("reset_gap") or {}, t.get("play_test_speed") or {}
    return dict(H=length.get("value", 20.0), H_tol=length.get("tolerance", 0.15),
                H_extend=length.get("extend_to_h"), gap_med=session.get("median_gap_max", 15.0),
                gap_max=session.get("largest_gap_max", 20.0), run_min=t.get("run_min") or session.get(
                    "median_gap_max", 15.0), first5=unlocks.get("first_5_min_min", 3),
                u1=unlocks.get("first_hour", 6), u1_tol=unlocks.get("first_hour_tolerance", 1),
                u_min=unlocks.get("each_later_hour_min", 4), idle=t.get("idle") or {},
                Dg=gap.get("Dg", 2.0), k=speed.get("k", 10), a=(t.get("active_factor") or {}).get("value"),
                slow=t.get("active_slow_max", 1.25), raw=t, **fun_values(tg), **continuous_values(t))


def fun_values(tg):
    """fun (the fun block of the targets file), fun_gap and fun_gap_h1."""
    fun = fun_block(tg)
    return dict(fun=fun, **fun_gaps(fun))


def check_fun(fun):
    """InputError (exit 2) for an unknown row id or band key of the fun block."""
    model = dict(game=dict(targets=dict(fun=fun)))
    try:
        pw_fun.check_fun_targets(model)
    except SystemExit as e:
        raise InputError(f"the targets file: {e}") from None


def fun_gaps(fun):
    """The FM-1 band in minutes (fun_gap over the whole length, fun_gap_h1 in the first hour): the band of the fun
    targets, else the default band of the fun sheet. The placement spaces the content by it."""
    if fun:
        check_fun(fun)
    band = dict(pw_fun.SPEC_OF["FM-1"]["band"], **((fun or {}).get("FM-1") or {}))
    return dict(fun_gap=float(band["max"]), fun_gap_h1=float(band["max_h1"]))


def fun_block(tg):
    """The fun targets of the targets file: the top-level key fun (the export of the tuning panel), else the key fun
    under targets. The solver copies the block into game.targets.fun of the model, so that the fun sheet reads each
    band that the developer set (references/fun-analysis.md)."""
    fun = tg.get("fun")
    if fun is None:
        fun = (tg.get("targets") or {}).get("fun")
    return copy.deepcopy(fun) if fun else None


def continuous_values(t):
    """The targets of group A3 of the interview: the purchase gap, the demo end and the purchase value floor."""
    pg, demo, pv = t.get("purchase_gap") or {}, t.get("demo_end") or {}, t.get("purchase_value") or {}
    return dict(pv_floor=pv.get("floor", pw_proof.PURCHASE_FLOOR),
                pv_source=pv.get("source") or pw_proof.PURCHASE_FLOOR_SOURCE,pgap=pg.get("largest_max_min", pw_proof.PURCHASE_GAP_MINUTES),
                pfirst=pg.get("first_max_min", pw_proof.FIRST_PURCHASE_MINUTES),
                ugap_max=pg.get("unlock_max_min", pw_proof.UNLOCK_GAP_MINUTES),
                ugap_first=pg.get("unlock_first_hour_max_min", pw_proof.UNLOCK_GAP_FIRST_HOUR_MINUTES),
                ugap_ending=pg.get("unlock_max_min_ending"), tolerances=target_tolerances(t),
                no_source=[GAP_KEYS[k] for k in ("largest_max_min", "first_max_min") if pg.get("unsupported")],
                demo_min=demo.get("minutes"), demo_before=demo.get("before_system"),
                rate_max=(t.get("purchase_rate") or {}).get("max_per_min", pw_proof.PURCHASE_RATE_MAX))


GAP_KEYS = {"largest_max_min": "largest_purchase_gap_minutes", "first_max_min": "first_purchase_minutes",
            "unlock_max_min": "largest_unlock_gap_minutes", "unlock_first_hour_max_min": "first_hour_unlock_gap_minutes",
            "unlock_max_min_ending": "ending_unlock_gap_minutes"}
GROUP_KEYS = {"purchase_value": "purchase_value", "purchase_rate": "purchase_rate_max_per_minute"}


def target_tolerances(t):
    """{model target key: tolerance} of each target whose group has the key tolerance in the targets file: a number
    for each limit of the group, or (purchase_gap only) an object with 1 number for each limit. The proof sheet takes
    the default of the source class for each other target (pw_proof.TOLERANCE)."""
    tol = (t.get("purchase_gap") or {}).get("tolerance")
    out = {mk: float(tol[k] if isinstance(tol, dict) else tol) for k, mk in GAP_KEYS.items()
           if tol is not None and (not isinstance(tol, dict) or k in tol)}
    groups = [(mk, (t.get(g) or {}).get("tolerance")) for g, mk in GROUP_KEYS.items()]
    out.update({mk: float(v) for mk, v in groups if v is not None})
    return out


def target_warnings(tv, cat, pattern):
    """Step 3: each target against target_bands. A band with no proven game is a default."""
    if pattern == "continuous":
        return continuous_warnings(tv, cat)
    out = pace_notes(tv, cat.bands) + idle_notes(tv, cat.bands["T4"], pattern) + loop_notes(tv, cat.bands)
    return [w for w in out + active_notes(tv, cat) if w]


def continuous_warnings(tv, cat):
    """The bands of group A3 (play pattern continuous, SG of references/short-games.md)."""
    b = {x["id"]: x for x in cat.pattern["continuous"]["target_bands"]}
    out = [range_note("length to the hard end", tv["H"], b["CT1"]),
           range_note("largest gap between purchases", tv["pgap"], b["CT3"]),
           range_note("first purchase", tv["pfirst"], b["CT2"]), demo_note(tv, b["CT4"])]
    out += idle_notes(tv, cat.bands["T4"], "continuous") + [no_source_note(b["CT6"])] + active_notes(tv, cat)
    return [w for w in out if w]


def range_note(name, value, band):
    lo, hi = band["band"]
    if lo - 1e-9 <= value <= hi + 1e-9:
        return ""
    return f"warning: the {name} target {value:g} is outside the evidence band {lo:g} to {hi:g} ({band['source']})"


def demo_note(tv, band):
    """The demo end against the 2 bands of the guide: the general band, and the band of a strategy or management
    game (band_management). The solver does not know the genre, so a value in the second band is a note."""
    m = tv["demo_min"]
    if m is None:
        return "the demo end: not stated (no demo); the proof row PC3 is not applicable"
    lo, hi = band["band"]
    mlo, mhi = band.get("band_management") or (lo, hi)
    if m <= hi:
        return ""
    if mlo <= m <= mhi:
        return (f"note: the demo end at {m:g} minutes is inside the guide band {mlo:g} to {mhi:g} minutes of a "
                f"strategy or management game, and above the band {lo:g} to {hi:g} minutes of each other game "
                f"({band['source']}); the full game must add new content in each part past the demo (SG:185)")
    return (f"flag: the demo end at {m:g} minutes is longer than the guide bands {lo:g} to {hi:g} minutes, and {mlo:g} "
            f"to {mhi:g} minutes for a strategy or management game ({band['source']}); the nearest proof is the 1 "
            "to 2 hour Tower Wizard prototype (SG:189), so the full game must add new content in each part past the "
            "demo (SG:185)")


def no_source_note(band):
    return "no source: " + ", ".join(band["targets"]) + f" ({band['source']}); each such target is a designer " \
           "target, not supported by a source"


def pace_notes(tv, b):
    t1, t2, t3 = b["T1"], b["T2"], b["T3"]
    return [band_note("length", tv["H"], t1, abs(tv["H"] / t1["default"] - 1) > t1["tolerance"]),
            band_note("session", tv["gap_med"], t2, tv["gap_med"] > t2["band"]["median_gap_max"]),
            band_note("unlocks", tv["u1"], t3, tv["u1"] < t3["default"]["first_hour"] - 1)]


def loop_notes(tv, b):
    return [band_note("reset gap", tv["Dg"], b["T5"], tv["Dg"] > b["T5"]["default"]["Dg"]),
            band_note("play-test speed", tv["k"], b["T6"], tv["k"] > b["T6"]["default"]["k"])]


def band_note(name, value, band, outside):
    src = band["source"]
    lines = []
    if outside:
        lines.append(f"warning: the {name} target {value:g} is outside the evidence band "
                     f"{band_text(band)} ({src})")
    if band.get("proven_game") is None:
        lines.append(f"the {name} target: default, not from a proven game ({src})")
    return "; ".join(lines)


def band_text(band):
    d = band.get("band") or band["default"]
    if isinstance(d, dict):
        return ", ".join(f"{k} {v:g}" for k, v in d.items() if isinstance(v, (int, float)))
    return f"{d:g}" + (f" within {band['tolerance']:.0%}" if "tolerance" in band else "")


def idle_items(tv):
    return [(k, rng) for k, rng in sorted(tv["idle"].items()) if k != "unsupported"]


def idle_notes(tv, band, pattern):
    return [n for key, rng in idle_items(tv) for n in idle_note(key, rng, band, pattern)]


def idle_note(key, rng, band, pattern):
    dflt = band["default"].get(key)
    out = []
    if dflt and (rng[0] > dflt["max"] or rng[1] < dflt["min"]):
        out.append(f"warning: the idle target {key} {rng} is outside the evidence band "
                   f"{dflt['min']:g} to {dflt['max']:g} ({band['source']})")
    if key.startswith("closed") and pattern == "continuous":
        out.append(f"warning: the idle target {key} has no meaning for a continuous game with no offline gain")
    return out


def active_notes(tv, cat):
    if tv["a"] is None:
        return []
    src = cat.param("S05.active_lane", "a")
    if tv["a"] > src["max"]:
        return [f"the active factor {tv['a']:g}: designer target, above the sourced value {src['max']:g} "
                f"({src['source']})"]
    return []


def static_target_conflicts(tv):
    """A target that no model can meet: an idle value of 1 or more (BM L4 property 3)."""
    bad = [(key, rng) for key, rng in idle_items(tv) if rng[0] >= 1.0]
    if not bad:
        return
    key, rng = bad[0]
    raise Conflict(f"idle.{key} = {rng[0]:g} to {rng[1]:g}", "PO6 and PO8 (BM L4 property 3: idle stays "
                   "below active for each T when f <= 1)", "f_open and f_closed <= 1",
                   [f"relax idle.{key} to a band below 1, for example 0 to 0.5",
                    "an idle player can never match an active player in a model with f <= 1"])


class Design:
    """The selection, the targets, the parameters with their sources, and the model."""

    def __init__(self, chosen, overrides, tv, pattern, cat, entries=None):
        self.chosen, self.over, self.tv, self.pattern, self.cat = chosen, overrides, tv, pattern, cat
        self.params, self.model, self.sheet, self.rounds, self.ext = {}, None, None, [], None
        self.plan, self.proof, self.refused, self.named, self.placed = None, None, [], [], []
        self.entries = entries if entries is not None else [new_entry(s, v) for s, v in sorted(chosen.items())]

    def setp(self, name, value, rng, source, by):
        self.params[name] = dict(value=value, range=rng, source=source, set_by=by)

    def val(self, name):
        return self.params[name]["value"]

    def tv_a(self):
        """The active factor a: the target, else the default of S05.active_lane."""
        return self.tv["a"] if self.tv["a"] is not None else self.cat.param("S05.active_lane", "a")["default"]

    def catalog_param(self, vid, name, by, value=None):
        """A catalog parameter: the override, else the default. Returns the value."""
        p = self.cat.param(vid, name)
        v = value if value is not None else self.over.get(vid, {}).get(name, p["default"])
        check(not p["min"] - 1e-9 <= v <= p["max"] + 1e-9,
              f"{vid}.{name} = {v:g} is outside its proven range {p['min']:g} to {p['max']:g} ({p['source']})")
        self.setp(f"{vid}.{name}", v, [p["min"], p["max"]], p["source"], by)
        return v


VARIANT_CAUSE = {
    "S02.add_in_group": "PO3: r = 0, so R = 0 and s = p / (1 - R) = p <= 0.5 < 1 for each p of the proven range",
    "S04.none": "PO1 to PO5 of the solver need a reset layer; a game with no reset (S29 or S22) needs a simulation, "
                "because the laws of BM give no run length for it",
    "S05.manual_then_automate": "a phase change (BM section 7): the laws prove only a constant active factor "
                                "(S05.active_lane); a simulation must check the manual phase",
}
GENERIC_CAUSE = "the laws L1 and L6 of the solver need another family; balance it by simulation"


def long_run(d):
    """True for 1 long run: a continuous design with no content file, no reset layer, no S29 and no S22 (BM, "1 long
    run with no reset"). The solver proves it with L1, L2, L3, L8 and L9; the rows of the reset loop are not
    applicable. With a content file (rebalance mode), the selection takes the simulation path."""
    return d.plan is None and d.pattern == "continuous" and d.chosen.get("S04", "S04.none") == "S04.none" and \
        not set(LONG_RUN_OUT) & set(d.chosen)


def supported_or_conflict(d):
    chosen = dict(d.chosen, S04=d.chosen.get("S04", "S04.none"))
    if long_run(d):
        chosen.pop("S04")
    bad = [(sid, vid) for sid, vid in sorted(chosen.items())
           if SUPPORTED.get(sid) is not None and vid not in SUPPORTED[sid]]
    if not bad:
        return
    causes = "; ".join(f"{vid}: {VARIANT_CAUSE.get(vid, GENERIC_CAUSE)}" for _, vid in bad)
    ways = [f"select {SUPPORTED[sid][0]} in place of {vid}" for sid, vid in bad]
    raise VariantConflict("the selected variants " + ", ".join(vid for _, vid in bad), causes,
                   "; ".join(f"{sid}: {', '.join(SUPPORTED[sid])}" for sid, _ in bad),
                   ways + ["keep the variants and balance the game by simulation: the laws of BM do not prove them"])


def solve_lines(d, s):
    """Steps 1 to 3 of the procedure: the line families and ratios, p, then R and q (PO1, PO2, PO3)."""
    gen_v = d.chosen["S01"]
    d.catalog_param(gen_v, UNIT_PARAM.get(gen_v, "b"), "S01 default (an additive line has r = 0)")
    p = d.catalog_param("S04.power", "p", "S04 default; the largest p gives the largest idle value (L4b)")
    s_max = s_limit(d.tv["Dg"])
    if s_max <= 1.0:
        raise Conflict(f"reset_gap.Dg = {d.tv['Dg']:g}", "PO3 and PO4 (L6)", "s > 1 and s <= s_max",
                       ["raise Dg to 1.5 or more", "select S04.none with S22 (no reset layer)"])
    R = 1.0 - p / s
    d.setp("s", s, [1.0, s_max], "BM L6 design rule 2 and 3", f"PO3 (s > 1) and PO4 (gap <= {d.tv['Dg']:g})")
    d.setp("R", R, [0.0, 1.0], "BM L1: R = 1 - p / s", "PO1 (R < 1) with s and p")
    d.setp("q", (1.0 - R) / p, None, "BM L6 design rule 1: q = (1 - R) / p", "PO2 (lambda = 1)")
    if d.plan is None:
        solve_groups(d, R)
        solve_upkeep(d)


def s_limit(Dg):
    """The largest s with gap(s) <= Dg (PO4): (ln(1 + X_p) / ln(1 + X_e)) (X_e / X_p)^(1/s) <= Dg."""
    ratio = Dg * math.log1p(X_E) / math.log1p(X_P)
    if ratio >= 1.0:
        return float("inf")
    return math.log(X_E / X_P) / math.log(ratio)


def solve_groups(d, R, carriers=0):
    """The multiplier groups share R (1 long run: with `carriers` more carriers, the generator with its count steps).
    Each group takes the finest steps inside the range: d at its minimum and b = d^(1/r). Coarse steps move the
    trajectory away from A x^alpha for hours (BM section 7, limit 6), and then the run length of the simulation does
    not follow L6 (PO5)."""
    vid = "S02.levelled_factor"
    r = R / (len(group_costs(d)) + carriers)
    bp, dp = d.cat.param(vid, "b"), d.cat.param(vid, "d")
    b = max(bp["min"], dp["min"] ** (1.0 / r))
    if b ** r > dp["max"] or b > bp["max"]:
        raise Conflict(f"R = {R:.3f}", "PO1 and PO3 (L1: r = ln d / ln b)", f"{vid} b {bp['min']:g} to "
                       f"{bp['max']:g}, d {dp['min']:g} to {dp['max']:g}", ["add a third multiplier group",
                                                                            "raise p inside its range"])
    d.catalog_param(vid, "b", "moved from the default to the finest steps that keep d in range (PO5, BM "
                    "section 7 limit 6)", b)
    d.catalog_param(vid, "d", f"d = b^r with r = R / {len(group_costs(d)) + carriers} = {r:.4f} (BM L1, PO1)", b ** r)
    d.setp("r_group", r, None, "BM L1: R is the sum of the largest r of each group", "PO1")


def solve_presence(d):
    """Step 6 and 7: the active factor, the offline rule and the bank."""
    a_src = d.cat.param("S05.active_lane", "a")
    a = d.tv["a"] if d.tv["a"] is not None else a_src["default"]
    by = "designer target, above the sourced value" if a > a_src["max"] else "S05 default"
    d.setp("S05.active_lane.a", a, [a_src["min"], a_src["max"]], a_src["source"], by)
    d.setp("f_open", 1.0 / a, [0.0, 1.0], "BM L5: an active income term a is the scale tool f = 1 / a",
           "PO8, PO9 and the idle targets")
    if "S06" in d.chosen and d.pattern == "check_in":
        d.catalog_param("S06.penalty_cap", "f_closed", "S06 default")
        d.catalog_param("S06.penalty_cap", "T_cap_h", "S06 default")
    if "S08" in d.chosen:
        d.catalog_param("S08.timed_bank", "period_s", "S08 default")
        d.catalog_param("S08.timed_bank", "cap", "S08 default")


def run_minutes(d):
    """(the run length T_X in minutes, its source). Rebalance mode of a continuous game: a stage tests the full
    income, and the full income peaks at the end of each run, so the stages come 1 time in each run. The run length
    is then the gap between stages: T_X <= UNLOCK_SHARE x the unlock gap target (PC4)."""
    if d.plan is None or d.pattern != "continuous":
        return d.tv["run_min"], "BM L8 step 1: 1 run for each session"
    if rb.keeps(d.plan, "reset_count"):
        k = d.plan.structure["reset_count"]
        return 60.0 * d.tv["H"] / (k + 1), (f"the game has at most {k} resets (max_resets, the structure of the game): "
                                           f"T_X = the length / {k + 1}")
    t = min(d.tv["run_min"], UNLOCK_SHARE * d.tv["ugap_max"])
    return t, (f"continuous play: the stages come at the end of each run, so T_X <= {UNLOCK_SHARE:g} x the unlock gap "
               f"target {d.tv['ugap_max']:g} min (PC4); run_min {d.tv['run_min']:g} is the upper limit")


def solve_loop(d):
    """Step 5: A from the L2 trajectory, then c for the run length T_X (L6, L8)."""
    minutes, source = run_minutes(d)
    T_X = minutes * 60.0
    alpha = 1.0 / (1.0 - d.val("R"))
    d.setp("T_X", T_X, None, source, "PO5 (sigma) and PO13")
    A, note, K = trajectory(d, alpha)
    d.setp("A", A, None, "the L2 trajectory of the model with no reset (pw_proof.trajectory_A)" + note, "PO5")
    if getattr(d, "no_k", False):
        K, note = 1.0, "; no K: the later runs end at the run floor, outside the domain of K (BM D8)"
    else:
        note = ""
    if abs(K - 1.0) > 1e-9:
        d.setp("K_q", K, None, "the product of ((b - 1) / a1)^r of the uncapped lines of the reset currency: a spend "
               "of L buys their factor about K L^r, not L^r (BM D8)", "PO5")
    c = A * (K * T_X) ** alpha * X_E ** (-alpha / d.val("s")) * getattr(d, "c_scale", 1.0)
    d.setp("c", c, None, "BM L6 design rule 4: c = A (K T_X)^alpha X^(-alpha/s)" + note + c_note(d),
           "PO5 (T_X within 20% of sigma)")


def c_note(d):
    k = getattr(d, "c_scale", 1.0)
    return "" if abs(k - 1.0) < 1e-12 else f", times {k:.4g}: the simulated median later run of early_reset set c " \
                                           "(the simulator in the loop, T ~ c^(1/alpha))"


def trajectory(d, alpha):
    """(A, note, K). Rebalance mode: the model with the spine of the last placement round, so that the stage rewards
    count; with no spine yet, A x (the factor of the stage rewards)^alpha. K: the constant of the lines of the reset
    currency (BM D8, pw_proof.reset_line_constant)."""
    spined = d.plan is not None and getattr(d, "table", None)
    m = load(build_model(d) if spined else build_model(d, spine=placeholder_spine(spine_cur(d))))
    A = pw_proof.trajectory_A(m, ECON, alpha)
    K = reset_constant(m)
    if spined:
        return A, "; the model has the spine of the last placement round, so the stage rewards count", K
    if d.plan is None:
        return A, "", K
    return A * rb.ladder_factor(d.plan) ** alpha, "; x (the factor of the stage rewards)^alpha, because that run " \
                                                  "reaches no stage", K


def reset_constant(m):
    pr = m.get("prestige") or {}
    if not pr.get("currency"):
        return 1.0
    return pw_proof.reset_line_constant(pw_proof.lines_paid(m, pr["currency"], ECON))


def solve_spine(d):
    """Step 8: w from the unlock target, the first stage powers on the trajectory, and N (L8)."""
    gr = spine_rate(d)
    g = d.cat.param("S03.exponential", "g")
    u_need = max(d.tv["u_min"], 60.0 / d.tv["gap_med"], content_unlocks(d))
    w = wall_ratio(gr, g, u_need)
    if w < g["min"]:
        raise Conflict(f"unlocks.each_later_hour_min = {d.tv['u_min']:g}", "PO10 (L3, L8: w = e^(alpha ldot / u))",
                       f"S03.exponential g {g['min']:g} to {g['max']:g}",
                       [f"relax the unlock target to {gr / math.log(g['min']):.1f} or less for each hour",
                        "lengthen the run (run_min) so that ln M rises more slowly"])
    by = "S03 default" if w == g["default"] else f"moved from the default so that u >= {u_need:g} for each hour"
    d.catalog_param("S03.exponential", "g", by, w)
    d.setp("u", gr / math.log(w), [u_need, None], "BM L8: u = alpha ldot / ln w", "PO10")
    first_stages(d)


def spine_rate(d):
    """The rise of ln P for each hour: alpha ldot of the reset loop (L3, L8), R times that on a second income (L10),
    or 3600 g of 1 long run (L2 with R = 1)."""
    if long_run(d):
        return HOUR * d.val("g_run")
    gr = (HOUR * d.val("q") * math.log1p(X_E) / d.val("T_X")) / (1.0 - d.val("R"))
    if spine_cur(d) != ECON:
        gr *= d.val("R")
    return gr


def content_unlocks(d):
    """Rebalance mode and 1 long run: the stages alone come at most each UNLOCK_SHARE x the unlock gap target (PC4).
    The other content fills the gaps between stages."""
    return 60.0 / (UNLOCK_SHARE * d.tv["ugap_max"]) if d.plan is not None or long_run(d) else 0.0


def wall_ratio(gr, g, u_need):
    """The default g when it gives at least u_need unlocks for each hour, else the g that gives u_need."""
    return g["default"] if gr / math.log(g["default"]) >= u_need - 1e-9 else math.exp(gr / u_need)


def first_stages(d):
    m = load(build_model(d, spine=placeholder_spine(spine_cur(d))))
    peaks, p1h = trajectory_peaks(m, stage_times(d), spine_cur(d))
    table = increasing([float("%.2g" % v) for v in peaks])
    if long_run(d):
        table = wall_steps(table, d.cat.param("S03.exponential", "g"))
    d.table, d.p1h = table, p1h
    d.setp("first_stages", table, None, "the trajectory of active_greedy at the target times (BM L8 step 4)",
           "PO11 and the first-hour unlock target")
    d.setp("N", stage_count(d, d.tv["H"]), None, "BM L8: N = u_1 + ceil((ln P(1 h) - ln h_u1) / ln w + "
           "u (H - 1))", "PO12 (length)")


WALL_MARGIN = 1.005


def wall_steps(table, g):
    """The first walls with each step after the first inside the g range of S03.exponential (row WS): a step below
    the range moves its wall up (fewer walls in a flat part), and a step above the range gets walls between at equal
    ratios (more walls in a steep part). The first step is the start of the game and stays."""
    out = list(table[:2])
    for v in table[2:]:
        out.extend(walls_to(out[-1], v, g["min"] * WALL_MARGIN, g["max"] / WALL_MARGIN))
    return out


def walls_to(prev, v, lo, hi):
    """The walls from the wall prev up to the wall v: v is at least lo x prev, and each step is at most hi."""
    v = max(v, float("%.4g" % (prev * lo)))
    n = max(1, int(math.ceil(math.log(v / prev) / math.log(hi) - 1e-9)))
    step = (v / prev) ** (1.0 / n)
    return [float("%.4g" % (prev * step ** (k + 1))) for k in range(n - 1)] + [v]


def stage_times(d):
    """The target times of the first stages. Rebalance mode: a stage each UNLOCK_SHARE x the first-hour unlock gap
    target, up to 1 hour (PC4)."""
    if d.plan is None and not long_run(d):
        return first_times(d.tv["u1"])
    step = UNLOCK_SHARE * d.tv["ugap_first"]
    return list(FIRST_TIMES_MIN[:2]) + [step * k for k in range(1, int(60.0 / step)) if step * k > FIRST_TIMES_MIN[1]]


def first_times(u1):
    if u1 == len(FIRST_TIMES_MIN):
        return list(FIRST_TIMES_MIN)
    return [45.0 * ((i + 1) / u1) ** 2 for i in range(u1)]


def increasing(vals):
    out = []
    for v in vals:
        out.append(max(v, out[-1] * 1.05) if out else v)
    return out


def stage_count(d, H):
    if getattr(d, "fit_N", None) and H == d.tv["H"]:
        return d.fit_N
    sim_n = sim_stage(d, H)
    if sim_n:
        return sim_n
    w = d.val("S03.exponential.g")
    tail = (math.log(d.p1h) - math.log(d.table[-1])) / math.log(w) + d.val("u") * (H - 1.0)
    n = len(d.table) + max(1, int(math.ceil(tail)))
    return n * PLACE_HEADROOM if getattr(d, "placing", False) else n


def sim_stage(d, H):
    """Rebalance mode: the stage of active_greedy at H hours in the last placement run, when that run reaches it."""
    steps = getattr(d, "sim_steps", None)
    if not steps or H * HOUR > max(steps.values()):
        return None
    return max([n for n, t in steps.items() if t <= H * HOUR + 1e-9], default=None)


def trajectory_peaks(m, times_min, cur=ECON):
    """The highest income of the economy of active_greedy up to each time, and up to 1 hour."""
    m = copy.deepcopy(m)
    m["game"]["target_hours"] = 1.0
    pl = pw_players.Players(m, 1.0 / 3.0)
    spec = next(p for p in pw_players.PLAYERS if p["name"] == "active_greedy")
    sim = pl.run(spec)
    return [peak_until(sim, t * 60.0, cur) for t in times_min], peak_until(sim, HOUR, cur)


def peak_until(sim, t, cur=ECON):
    return max([s["income"][cur] for s in sim.samples if s["t"] <= t + 1e-9], default=0.0)


def placeholder_spine(cur=ECON):
    return dict(table=None, base=1e300, growth=2.0, count=1000, cur=cur)


def final_spine(d, H):
    return dict(table=d.table, base=d.table[0], growth=d.val("S03.exponential.g"), count=stage_count(d, H),
                cur=spine_cur(d))


def build_model(d, spine=None, H=None):
    H = H if H is not None else d.tv["H"]
    ends, slots = continuous_parts(d, H) if spine is None else ({}, [])
    spine = spine or final_spine(d, H)
    sys_ids = model_system_ids(d)
    m = dict(game=game_block(d, H, ends), currencies=currencies(d), systems=[dict(id=s, loop=loop_of(s)) for s in
             sys_ids], purchases=generic_purchases(d), milestones=milestones(d) + slots, ladders=[ladder(spine)],
             prestige=prestige(d))
    return finish_model(d, m, spine)


def model_system_ids(d):
    """The system rows of the model. 1 long run has no S04 row (no reset layer)."""
    skip = {"S06", "S15"} | ({"S04"} if long_run(d) else set())
    return sorted({sys_id(s) for s in d.chosen if s in MODELLED and s not in skip})


def finish_model(d, m, spine):
    if upkeep_on(d) and d.plan is None:
        m["upkeep"] = upkeep_block(d)
    if long_run(d) and d.automation:
        m["automation"] = d.automation
    return content_model(d, m, spine) if d.plan is not None else m


def content_lists(d, m):
    plan = d.plan
    m["systems"] += [dict(id="s15", loop="meta", persists=True)] + rb.side_system_rows(plan)
    m["currencies"] = [c for c in m["currencies"] if c["id"] != BANK_CUR] + rb.side_currencies(plan) + \
        rb.spine_currencies(plan) + content_bank(d)
    m["purchases"] = purchases_of(plan, d)
    m["milestones"] = [x for x in m["milestones"] if not x["id"].startswith("content_")] + rb.milestones(plan)


def generic_purchases(d):
    return purchases(d) if d.plan is None else []


def content_model(d, m, spine):
    """Rebalance mode: the elements of the content in place of the generic lines."""
    plan = d.plan
    plan.N = spine["count"] if spine["table"] else None
    plan.gen_value, plan.gen_cost = GEN_VALUE, GEN_COST
    content_lists(d, m)
    rb.apply_structure(plan, m)
    m["ladders"][0]["grants"] = rb.grants(plan)
    if rb.grants(plan, "reset"):
        m["prestige"]["grants"] = rb.grants(plan, "reset")
    content_loop(d, m)
    rb.set_system_rows(plan, m)
    m["field_map"] = rb.field_map(plan, m, d)
    m["caps_reduced"] = rb.caps_reduced(plan)
    m["identity"] = rb.identity_check(plan, m)
    m["rules"] = rb.rules(m)
    rb.diff_rows(plan, m)
    m["character"] = rb.character(plan.content, m, plan)
    if getattr(d, "automation", None):
        m["automation"] = progress_unlocks(d, d.automation)
    return m


def content_loop(d, m):
    """The reset reward keeps the part of q that the lines of the reset currency and the factors that a count of
    resets opens do not carry (q_held); the first gain."""
    q_lines = sum(x.model.get("r", 0.0) for x in d.plan.fitted() if x.model.get("persist"))
    m["prestige"]["reward"]["exponent"] = max(0.0, d.val("q") - q_lines - q_held(d, m))
    m["prestige"]["min_gain"], m["prestige"]["min_run_seconds"] = first_gain(d)


def q_held(d, m):
    """The share of q that the factors of a count of resets carry, read from the model m as the proof reads it
    (pw_proof.held_q: ln F / (n ln(1 + X))). Reason: L6 holds lambda = p q alpha = 1 only when the reward is the only
    factor that a reset adds; these factors add F over n resets, so lambda was above 1 and each run ended at the run
    floor (trial Kiln n Kin: towns x3, glazes and pieces x1.25 at 8 counts, 245 s runs at a floor of 240 s; with the
    reward exponent 0 the runs grew from 246 s to 1328 s). 0 before the timing fit maps the counts (plan.reset_map)."""
    if not getattr(d.plan, "reset_map", None):
        return 0.0
    return pw_proof.held_q(m, ECON)


FIRST_RUN_SHARE = 0.5


def first_gain(d):
    """The rule of the first reset: the reset opens at the gain factor^(-s) and after a run of FIRST_RUN_SHARE x T_X.
    c holds the full content (A of all factors), so a run with none of the factors gives G = 1 only after a run
    longer than T_X by the factor; the gain factor^(-s) opens it at T_X (BM L6). The run floor is the proof that no
    reset comes in seconds: no run is shorter than T_X / 2, whatever the gain. With no floor, the trial game Kiln &
    Kin opened the reset at a gain of 0.03 and the players reset 7 times in 2 minutes."""
    f = rb.factor_product(d.plan, d)
    g = float("%.3g" % f ** (-d.val("s")))
    d.setp("first_gain", g, None, f"factor_actual^(-s) with factor_actual {f:.4g} (the product of the factors of "
           "the content)", "PO5 (the first runs)")
    if getattr(d, "fit_gain", None):
        g = d.fit_gain
        d.setp("first_gain", g, None, "the timing fit: the gain k (E / c)^p of the run total E of active_greedy at "
               "the first-reset target T_X, with no reset before it", "the target timeline (the first reset at T_X)")
    run = float("%.3g" % (FIRST_RUN_SHARE * d.val("T_X")))
    d.setp("first_run_seconds", run, None, f"{FIRST_RUN_SHARE:g} x T_X: no run is shorter, so no reset comes in "
           "seconds", "PO5 (the first runs)")
    return g, run


def purchases_of(plan, d):
    return rb.purchases(plan, d, None) + rb.bank_sink(d)


def content_bank(d):
    if "S08" not in d.chosen:
        return []
    return [dict(id=rb.BANK_CUR, name="Bank charges", system="s08", base_income=1.0 / d.val("S08.timed_bank.period_s"),
                 bank_cap=d.val("S08.timed_bank.cap"))]


def continuous_parts(d, H):
    """The hard end (S28.last_step: the last stage N), the demo end, the purchase gap targets, and 1 content slot
    for each part past the demo (proof row PC3). A slot is a milestone with no effect: the designer fills it."""
    if d.pattern != "continuous" or "S28" not in d.chosen:
        return {}, []
    N = stage_count(d, H)
    ends = dict(hard_end=dict(axis_step=N), largest_purchase_gap_minutes=d.tv["pgap"],
                purchase_rate_max_per_minute=d.tv["rate_max"],
                first_purchase_minutes=d.tv["pfirst"], largest_unlock_gap_minutes=d.tv["ugap_max"],
                first_hour_unlock_gap_minutes=d.tv["ugap_first"], **range_targets(d.tv))
    if d.tv["demo_min"] is None:
        return ends, []
    Hd = d.tv["demo_min"] / 60.0
    ends["demo_end"] = dict(axis_step=demo_step(d, Hd), minutes=d.tv["demo_min"], before_system=d.tv["demo_before"])
    if d.named:
        return ends, []
    return ends, content_slots(d, Hd, H, ends["demo_end"]["axis_step"], N)


def range_targets(tv):
    """The optional targets of the ranges: the limit of the last unlock gap before the hard end, the tolerance of
    each target that the designer set, and the purchase gap targets that the designer marks unsupported."""
    pairs = (("ending_unlock_gap_minutes", tv.get("ugap_ending")), ("tolerances", tv.get("tolerances")),
             ("unsupported", tv.get("no_source")))
    return {k: v for k, v in pairs if v}


def demo_step(d, Hd):
    """The stage that active_greedy reaches at Hd hours: the first-hour table, else BM L8."""
    if Hd >= 1.0:
        return stage_count(d, Hd)
    return max(1, len([t for t in first_times(d.tv["u1"]) if t <= Hd * 60.0 + 1e-9]))


def content_slots(d, Hd, H, n_demo, N):
    parts = pw_proof.extension_parts(Hd, H, PART_HOURS)
    return [content_slot(k + 1, min(N, max(n_demo + 1, demo_step(d, (lo + hi) / 2.0)))) for k, (lo, hi) in
            enumerate(parts)]


def content_slot(k, step):
    req = dict(type="ladder", id=AXIS, step=step)
    return dict(id=f"content_{k}", name=f"New content of part {k} past the demo (the designer names it)",
                system="s03", kind="feature", requires=[req], effects=[])


def sys_id(sid):
    return sid.lower()


def loop_of(s):
    return "meta" if s == "s04" else "core"


def game_block(d, H, ends):
    g = dict(name="Design of pw_design.py", target_hours=H, session_minutes=run_minutes(d)[0], progress_axis=AXIS,
             primary_profile="model", offline_rate=offline_rate(d), presence=presence(d),
             targets=dict(targets(d, H), **ends), assumptions=assumptions(d), play_pattern=d.pattern)
    return g


def offline_rate(d):
    return d.val("S06.penalty_cap.f_closed") if "S06.penalty_cap.f_closed" in d.params else 0.0


def presence(d):
    out = dict(open_rate=1.0, visit_minutes=VISIT_MINUTES)
    if "S06.penalty_cap.T_cap_h" in d.params:
        out["offline_cap_minutes"] = d.val("S06.penalty_cap.T_cap_h") * 60.0
    return out


def targets(d, H):
    tv = d.tv
    t = dict(policy_gap=[0, tv["Dg"]], unlocks_first_hour=first_hour_band(d),
             length_hours=[H * (1 - tv["H_tol"]), H * (1 + tv["H_tol"])], unlocks_per_hour=[tv["u_min"], 60],
             play_test_speed=tv["k"], idle=idle_targets(tv["idle"]),
             purchase_value=dict(floor=tv["pv_floor"], source=tv["pv_source"]))
    if tv.get("fun"):
        t["fun"] = copy.deepcopy(tv["fun"])
    return t


def first_hour_band(d):
    """The band of the unlocks in the first hour. Play pattern continuous: the first-hour gap target g of PC4 needs
    ceil(60 / g) - 1 unlocks or more, so the band is that count up to 60 (an upper limit below it would conflict
    with PC4; the purchase rate PC6 limits a busy hour)."""
    tv = d.tv
    if d.pattern != "continuous":
        return [tv["u1"] - tv["u1_tol"], tv["u1"] + tv["u1_tol"]]
    return [int(math.ceil(60.0 / tv["ugap_first"] - 1e-9)) - 1, 60]


IDLE_KEY_HOURS = {"1h": 1, "8h": 8, "24h": 24, "7d": 168}


def idle_targets(idle):
    keys = [(k.split("_"), rng) for k, rng in idle.items() if k != "unsupported"]
    states = sorted({state for (state, _), _ in keys})
    return {st: {str(IDLE_KEY_HOURS[h]): list(rng) for (s, h), rng in keys if s == st} for st in states}


def assumptions(d):
    out = ["pw_design.py wrote this model from the selection and the targets. Each parameter and its source "
           "is in the report of the solver.",
           "Power is the income at the active rate. The active factor multiplies income only while the player "
           "is present.",
           "The reset currency is not spent on the reset. The reward multiplies income by (1 + L)^q."]
    if d.pattern == "continuous":
        out.append("Play pattern continuous: the game pays nothing while it is closed (offline_rate 0).")
    if long_run(d):
        out[2] = "1 long run: no reset layer. R = 1, so the income grows as e^(g t) and the stages come at a constant " \
                 "rate (BM, 1 long run with no reset)."
    return out


def currencies(d):
    base = (rb.main_base(d.plan) if d.plan is not None else None) or 1.0 / d.val("S05.active_lane.a")
    out = [dict(id=ECON, name="Main currency", system="s01", base_income=base)]
    if not long_run(d):
        out.append(dict(id=RESET_CUR, name="Reset currency", system="s04"))
    if "S08" in d.chosen:
        out.append(dict(id=BANK_CUR, name="Bank charges", system="s08",
                        base_income=1.0 / d.val("S08.timed_bank.period_s"), bank_cap=d.val("S08.timed_bank.cap")))
    return out


def purchases(d):
    gen_v = d.chosen["S01"]
    out = [generator(d, d.val(f"{gen_v}.{UNIT_PARAM.get(gen_v, 'b')}"))]
    scale = d.params.get("cost_scale", {}).get("value", 1.0)
    for i, (cost, entry) in enumerate(zip(group_costs(d), group_entries(d))):
        p = line(f"m{i + 1}", core_names(d).get(f"m{i + 1}") or group_name(i, entry), "s02", ECON,
                 float("%.4g" % (cost * scale)),
                 d.val("S02.levelled_factor.b"), mult(d.val("S02.levelled_factor.d")))
        out.append(dict(p, entry=entry) if entry else p)
    out += care_lines(d, scale)
    reward_note(d)
    out += bank_lines(d)
    return out + [e for e in d.placed if "cost_currency" in e]


def generator(d, b):
    """The generator line. 1 long run: no cap, and the count steps of the solver (count_steps)."""
    eff = dict(target=ECON, kind="add", value=GEN_VALUE)
    name = core_names(d).get("gen", "Generator")
    if f"{STEP_VARIANT}.n" not in d.params:
        return line("gen", name, "s01", ECON, GEN_COST, b, eff, d.params.get("gen_cap", {}).get("value", GEN_CAP))
    return dict(line("gen", name, "s01", ECON, GEN_COST, b, eff), count_steps=step_block(d))


def step_block(d):
    return dict(every=int(d.val(f"{STEP_VARIANT}.n")), mult=float("%.4g" % d.val(f"{STEP_VARIANT}.m")))


SIDE_FOLDS = 0.3
PLACE_SECONDS = 30.0
SOLVER_WRITES = (("S01", "gen", "the generator line"), ("S30", "care", "the care line of the upkeep"),
                 ("S08", "bank", "the bank line"), ("S04", "reset", "the first reset"))


def start_elements(d):
    return [e for e in d.named if not e.get("opens_min")]


def core_names(d):
    """{solver line id: name of the content file} of the elements that are open at the start: the generator (role
    generator, else the first S01 line), 1 S02 line for each group in the order of the file, and the care line (the
    first element with the role care, else the first S30 line)."""
    start = start_elements(d)
    by = {sid: [e for e in start if e.get("system") == sid and e.get("role") != CARE_ROLE] for sid in ("S01", "S02")}
    gen = [e for e in by["S01"] if e.get("role") == "generator"] or by["S01"]
    pairs = [("gen", gen[:1]), ("care", care_elements(start)[:1])]
    pairs += [(f"m{i + 1}", [e]) for i, e in enumerate(by["S02"])]
    return {pid: es[0]["name"] for pid, es in pairs if es and es[0].get("name")}


def care_elements(start):
    """The care line of the content file: the elements with the role care, else the S30 lines."""
    return [e for e in start if e.get("role") == CARE_ROLE] or [e for e in start if e.get("system") == "S30"]


def core_ids(d):
    """The ids of the start elements that core_names maps to a line of the solver."""
    names = set(core_names(d).values())
    return [e["id"] for e in start_elements(d) if e.get("name") in names]


def timed_elements(d):
    return sorted([e for e in d.named if e.get("opens_min")], key=operator.itemgetter("opens_min"))


def place_named(d):
    """Place each named element with a time at its target minute: 1 run of active_greedy to that minute on the
    model with the elements before it gives the lifetime total (the unlock point) and the income (the first cost)."""
    d.placed = []
    for e in timed_elements(d):
        d.placed.append(named_entry(d, e, state_at(d, e["opens_min"])))


def state_at(d, minutes):
    """The lifetime total and the income of the economy of active_greedy at the minute, in the model with no
    stage."""
    m = load(build_model(d, spine=placeholder_spine()))
    m["game"]["target_hours"] = minutes / 60.0
    spec = next(p for p in pw_players.PLAYERS if p["name"] == "active_greedy")
    sim = pw_players.Players(m, minutes / 180.0).run(spec)
    income = sim.income()[ECON]
    return dict(total=sim.life[ECON], income=income)


def named_entry(d, e, at):
    """The model entry of 1 named element: a milestone (kind reward or system) or a purchase line, with the unlock
    point of its target minute."""
    req = [dict(type="total", currency=ECON, amount=float("%.4g" % at["total"]))]
    head = dict(id=e["id"], name=e["name"], system=sys_id(e.get("system") or "S03"), requires=req,
                place_minutes=e["opens_min"])
    if e.get("kind") != "line":
        kind = "system_unlock" if e.get("kind") == "system" else "reward"
        return dict(head, kind=kind, effects=[named_effect(f) for f in e.get("effects") or []])
    return named_line(d, e, head, at)


def named_effect(f):
    return dict(target=ECON, kind=f["kind"], value=f["value"])


def named_line(d, e, head, at):
    """A named line. A line with an add effect is a second generator (second_generator). Each other line is a side
    line (side_line)."""
    cost = float("%.3g" % (PLACE_SECONDS * at["income"]))
    eff = (e.get("effects") or [dict(kind="mult", value=d.val("S02.levelled_factor.d"))])[0]
    p = (second_generator if eff["kind"] == "add" else side_line)(d, e, (head["system"], cost, eff))
    return dict(p, requires=head["requires"], place_minutes=head["place_minutes"])


def second_generator(d, e, args):
    """The cost ratio and the count steps of the generator, and the value v (c / a)^r for its first cost c, with a
    and v of the generator and r of 1 carrier. Its value over cost^r is then that of the generator, so the 2 lines
    are 1 group with the same r (L1, D6)."""
    system, cost, _ = args
    value = float("%.3g" % (GEN_VALUE * (cost / GEN_COST) ** d.val("r_group")))
    p = line(e["id"], e["name"], system, ECON, cost, d.gen_b, dict(target=ECON, kind="add", value=value))
    return dict(p, count_steps=step_block(d))


def side_line(d, e, args):
    """A side line: its step inside the range of d of S02, its cost ratio inside the range of b, and a cap
    (side_cap)."""
    system, cost, eff = args
    bp, step = d.cat.param("S02.levelled_factor", "b"), side_step(d, eff)
    ratio = min(max((e.get("cost") or {}).get("ratio") or bp["default"], bp["min"]), bp["max"])
    return line(e["id"], e["name"], system, ECON, cost, ratio, mult(step), side_cap(d, e, step))


def side_step(d, eff):
    """The step of a side line: its value moved into the proven range of d of S02."""
    dp = d.cat.param("S02.levelled_factor", "d")
    return min(max(eff["value"], dp["min"]), dp["max"])


def side_cap(d, e, step):
    """The cap of a side line: the levels of the content file, else the levels that give its part of the e-folds of
    the free side lines (SIDE_FOLDS for each such line in total). The part of a line is the time from its target
    minute to the next free side line, or to the hard end, over the time from the first such line to the hard end:
    the content then adds to ln I at a steady rate (BM 3a). A capped line adds a constant factor and nothing to R
    (L7)."""
    if e.get("levels"):
        return int(e["levels"])
    times = [q["opens_min"] for q in free_sides(d)] + [d.tv["H"] * 60.0]
    at = times.index(e["opens_min"])
    part = (times[at + 1] - times[at]) / max(times[-1] - times[0], 1e-9)
    return max(1, int(round(SIDE_FOLDS * (len(times) - 1) * part / math.log(step))))


def free_sides(d):
    """The named side lines whose cap the solver sets: a timed line with a mult effect and no levels."""
    return [e for e in timed_elements(d) if e.get("kind") == "line" and not e.get("levels") and
            any(f.get("kind") == "mult" for f in e.get("effects") or [])
            and not any(f.get("kind") == "add" for f in e.get("effects") or [])]


def named_lines(d):
    """The section "Named content" of the report: each element of the content file, where the model holds it, its
    target minute and the minute of the active player."""
    if not d.named:
        return []
    times = named_times(d)
    names = {v: k for k, v in core_names(d).items()}
    out = ["## Named content", "", "The content file gives the named elements. An element that is open at the start "
           "is a line of the solver with its name. Each other element has its unlock point (a lifetime total) and "
           "its first cost from the run of the active player at its target minute. A second generator line takes "
           "the count steps of the generator. Each other named line is a side line with a cap: it adds a constant "
           "factor and nothing to R (L7).", "", "| Id | Name | Model entry | Target (min) | Active player (min) |",
           "| --- | --- | --- | --- | --- |"]
    return out + [named_row(e, names, times) for e in d.named] + [""] + not_written_lines(d, names)


def named_times(d):
    """{model id: minute of the first purchase or of the milestone} of active_greedy, from the proof run."""
    m = load(d.model)
    _, _, pl = pw_players.run_players(m, m["game"]["target_hours"], ["active_greedy"])
    events = pl.runs["active_greedy"][1].events
    return {e["id"]: e["real"] / 60.0 for e in reversed(events) if pw_proof.is_unlock(e)}


def named_row(e, names, times):
    mid = names.get(e.get("name")) or e["id"]
    t = times.get(mid)
    held = mid if (mid in times or e.get("opens_min")) else "not written"
    return (f"| {e['id']} | {e.get('name')} | {held} | {e.get('opens_min') or 0:g} | "
            f"{'not reached' if t is None else format(t, '.1f')} |")


def not_written_lines(d, names):
    left = [e["id"] for e in start_elements(d) if e.get("name") not in names]
    return [f"Not written: {', '.join(left)}. The solver has no line for a start element of that system.", ""] \
        if left else []


def s02_entries(d):
    return [e for e in d.entries if e["system"] == "S02" and e.get("role") != CARE_ROLE]


def group_entries(d):
    """The entry id of each multiplier group: 1 group for each S02 entry when the selection has 2 or more; else the
    2 groups of worked example 2 (no entry id, as in the old format)."""
    ents = s02_entries(d)
    return [e["id"] for e in ents] if len(ents) > 1 else [None] * len(GROUP_COSTS)


def group_costs(d):
    """The first cost of each group: GROUP_COSTS, then x10 for each more group."""
    n = len(group_entries(d))
    return tuple(GROUP_COSTS[0] * 10.0 ** i for i in range(n)) if n > len(GROUP_COSTS) else GROUP_COSTS


def group_name(i, entry):
    return f"Multiplier group {i + 1}" + (f" ({entry})" if entry else "")


def upkeep_on(d):
    return d.chosen.get("S30") in UPKEEP_WRITTEN


def care_entry(d):
    return next((e["id"] for e in d.entries if e.get("role") == CARE_ROLE), None)


CARE_BASE_NEED = 3.0
CARE_FIRST_COST = 2
CARE_GEN_COSTS = 5


def care_lines(d, scale):
    """S30.need_capacity: 1 care line (no income effect; a factor on the capacity of the upkeep block)."""
    if not upkeep_on(d):
        return []
    entry = care_entry(d)
    cost = CARE_GEN_COSTS * GEN_COST if long_run(d) else group_costs(d)[0] * scale / CARE_FIRST_COST
    p = line(entry or "care", core_names(d).get("care") or (f"Care line ({entry})" if entry else "Care line"),
             "s30", ECON,
             float("%.4g" % cost), d.val("S02.levelled_factor.b"), None)
    p["effects"] = []
    return [dict(p, entry=entry) if entry else p]


def solve_upkeep(d):
    """The parameters of S30.need_capacity: the exponent e (catalog), the care step c (a factor on the capacity) with
    c^e - 1 = 2 x the purchase value floor (PU3 with a margin), the base capacity, and the band of u."""
    if not upkeep_on(d):
        return
    e = d.catalog_param("S30.need_capacity", "e", "S30 default (u = min(1, C / N)^e)")
    step = (1.0 + 2.0 * d.tv["pv_floor"]) ** (1.0 / e)
    d.setp("care_step", step, None, f"c^e - 1 = 2 x the PV floor {d.tv['pv_floor']:g} (PU3 with a margin)", "PU3")
    d.setp("care_base", CARE_BASE_NEED, None, "a choice of the solver: the base capacity meets the first 3 producer "
           "levels", "PU1 and PU2 (no stall at the start)")
    d.setp("care_band", [round(step ** (-2.0 * e), 2), 1.0], None, "u falls at most 2 care steps below 1 for a buyer "
           "that buys care when u < 1", "PU1")


def need_lines(d):
    """The lines whose levels add to the need: the generator (the units) and each multiplier group."""
    return ["gen"] + [f"m{i + 1}" for i in range(len(group_entries(d)))]


def upkeep_block(d):
    p = d.params
    care = care_entry(d) or "care"
    return dict(system="s30", target=ECON, rule="need_capacity", exponent=d.val("S30.need_capacity.e"), floor=0.0,
                base_need=0.0, base_capacity=p["care_base"]["value"], band=p["care_band"]["value"],
                need=[dict(purchase=pid, per_level=1.0) for pid in need_lines(d)],
                capacity=[dict(purchase=care, kind="mult", value=round(p["care_step"]["value"], 4))],
                min_switches=pw_proof.UPKEEP_SWITCHES)


def line(pid, name, system, cur, cost, growth, effect, cap=None):
    p = dict(id=pid, name=name, system=system, cost_currency=cur, base_cost=cost, cost_growth=growth,
             effects=[effect])
    if cap is not None:
        p["max_level"] = cap
    return p


def mult(v):
    return dict(target=ECON, kind="mult", value=v)


def reward_note(d):
    """S15 is selected: its reward is the reset reward M = (1 + L)^q. A shop line with steps lags this
    formula by up to 1 step (a factor b^q). In the first test of the solver, that lag made the simulated runs
    about 15% longer than L6, and the unlock gaps of the simulation then differed from L8 by more than 20%."""
    if "S15" not in d.chosen:
        return
    d.setp("S15.reward", "M = (1 + L)^q", None, "BM D8; the reset reward of the model (prestige.reward)",
           "PO2 and PO10 (a shop line with steps lags L6)")


def bank_lines(d):
    if "S08" not in d.chosen:
        return []
    vid = "S02.levelled_factor"
    b, dv = d.cat.param(vid, "b")["default"], d.cat.param(vid, "d")["default"]
    d.setp("bank_line", dict(b=b, d=dv, cap=BANK_LINE_CAP), None, f"{vid} defaults; the cap is a choice of the "
           "solver (L7: a capped line adds a constant factor and nothing to R)", "none (a simulation-only row)")
    return [line("bank", "Bank line", "s08", BANK_CUR, 1.0, b, mult(dv), BANK_LINE_CAP)]


def milestones(d):
    eff = dict(target=ECON, kind="mult", value=d.val("S05.active_lane.a"), active=True)
    return [dict(id="active_play", name="Active play multiplies income", system="s05", kind="feature",
                 persists=True, effects=[eff])] + [e for e in d.placed if "cost_currency" not in e]


def spine_cur(d):
    """The currency that the stages test: the second income of the content (BM L10), else the main income."""
    return (rb.spine_id(d.plan) if d.plan is not None else None) or ECON


def need_head(need):
    return [f"The content cannot meet the gap target. Add {sum(n['more'] for n in need)} more unlocks:", "",
            "This list is valid for this model only: a new element moves the stages and the lines in the next solve, so "
            "solve again after each change.", ""]


def ladder(spine):
    req = dict(type="rate", currency=spine.get("cur", ECON), base=spine["base"], growth=spine["growth"])
    if spine["table"]:
        req["table"] = spine["table"]
    return dict(id=AXIS, name="Stage {n}", system="s03", count=spine["count"], excitement=3, requires=req)


def prestige(d):
    if long_run(d):
        return dict(enabled=False)
    pr = dict(enabled=True, system="s04", currency=RESET_CUR, basis_currency=ECON, formula="power",
              base=d.params["c"]["value"] if "c" in d.params else 1e300, exponent=d.val("S04.power.p"), k=1,
              floor=False, min_gain=1, min_run_seconds=0, policy="ratio", policy_ratio=X_E,
              resets=dict(currencies=[ECON], purchase_systems=["s01", "s02"] + (["s30"] if upkeep_on(d) else []),
                          ladders=[]))
    pr["reward"] = dict(target=ECON, exponent=d.val("q"))
    return pr


def load(m):
    """The model as pw_model.load_model returns it (the simulator needs the filled defaults)."""
    tmp = tempfile.TemporaryDirectory()
    path = os.path.join(tmp.name, "model.json")
    write_model(path, m)
    out = load_model(path)
    tmp.cleanup()
    return out


def write_model(path, m):
    write_file(path, json.dumps(m, indent=1, sort_keys=True) + "\n")


def prove(m, speed=True):
    tmp = tempfile.TemporaryDirectory()
    path = os.path.join(tmp.name, "model.json")
    write_model(path, m)
    sheet = pw_proof.model_sheet(path, speed, True)
    tmp.cleanup()
    data = sheet.as_json()
    data["source"] = "model.json"
    return sheet, data


C_ROUNDS = 4
C_TOL = 0.10


def solve_round(d, s):
    d.no_k, d.c_scale, d.c_hist = False, 1.0, []
    solve_parts(d, s)
    if d.plan is not None:
        return record_round(d, s)
    fit_c(d, s)
    if floor_bound_with_k(d):
        d.no_k = True
        solve_parts(d, s)
    return record_round(d, s)


def solve_parts(d, s, spine_open=False):
    d.params, d.table, d.sim_steps, d.fit_N, d.fit_gain, d.automation, d.c0 = {}, None, None, None, None, None, None
    if d.plan is not None:
        d.plan.seed_scale, d.plan.reset_map, d.plan.spine_open = 1.0, {}, spine_open
    solve_lines(d, s)
    solve_presence(d)
    if d.plan is not None:
        place_content(d)
    else:
        solve_loop(d)
        solve_spine(d)
    d.model = build_model(d)
    prove_round(d)
    automate(d)
    refit_automated(d)
    if spine_ceiling(d):
        solve_parts(d, s, spine_open=True)


def spine_ceiling(d):
    """Rebalance mode: True when each line of the second income has a game cap and the stages of the timing fit stop
    before the hard end less ugap_max (the second income has a ceiling). Then the solve runs again 1 time with 1 cap
    dropped (pw_rebalance.SPINE_CEILING)."""
    plan = d.plan
    if plan is None or not getattr(plan, "spine_capped", False) or getattr(plan, "spine_open", False):
        return False
    if not any(n == "all" or n.startswith("cap:") for n in plan.accepted):
        return False
    rec = getattr(d, "fit_record", None) or {}
    last = max((rec.get("steps") or {}).values(), default=0.0)
    return last < d.timeline.last_slot


def refit_automated(d):
    """Rebalance mode: the auto-buy of the automation plan changes the purchases of the active player, so the timing
    fit runs again with it, and the round is proven again."""
    if d.plan is None or not d.automation:
        return
    fit_timing(d)
    d.model = build_model(d)
    prove_round(d)


def fit_c(d, s):
    """The solves of refit_c (at most C_ROUNDS), then the solve of best_c when it is not the last."""
    list(itertools.takewhile(bool, (step_c(d, s) for _ in range(C_ROUNDS))))
    best = best_c(d)
    if best is not None:
        d.c_scale = math.exp(best)
        solve_parts(d, s)


def step_c(d, s):
    """1 step of refit_c and its solve; False when the run is inside C_TOL of sigma (no solve)."""
    again = refit_c(d)
    if again:
        solve_parts(d, s)
    return again


def timing(d):
    """(proof inputs, T_sim, sigma, alpha) of the last solve."""
    inp = (d.proof or {}).get("inputs") or {}
    return inp, (inp.get("measured") or {}).get("T_sim"), inp.get("sigma"), inp.get("alpha")


def refit_c(d):
    """The simulator in the loop (rebalance mode): the median later run of early_reset (T_sim of the proof sheet)
    against sigma. When it differs by more than C_TOL, c takes a new factor and the round is solved again. The first
    factor is F held / F of the trajectory when it is outside 1 + FACTOR_TOL (L6 with F: c = A F (K T_X)^alpha
    X^(-alpha/s), as PO5 of the proof sheet); else and after it, the steps of next_log_c. Reason: the game rewards
    keep their values, so the runs hold other persistent factors than the trajectory with no reset, and c of rule 4
    alone gave runs of 5 times T_X (Moss Works) or runs at the run floor (Kiln n Kin)."""
    inp, T, sigma, alpha = timing(d)
    if d.plan is None or not (T and sigma and alpha) or abs(T / sigma - 1.0) <= C_TOL:
        return False
    d.c_hist.append(c_point(d, T))
    d.c_scale = math.exp(next_c(d, inp.get("F_ratio"), math.log(sigma), alpha))
    return True


def next_c(d, F, target, alpha):
    """The next ln c factor: the first one takes F held / F of the trajectory when it is outside 1 + FACTOR_TOL."""
    if len(d.c_hist) == 1 and factor_outside(F):
        return math.log(d.c_scale) + math.log(F)
    return next_log_c(d.c_hist, target, alpha)


def factor_outside(F):
    return bool(F) and abs(math.log(F)) > math.log(1.0 + pw_proof.FACTOR_TOL)


def c_point(d, T):
    """(ln c factor, ln T_sim, at the run floor, count of failed rows) of the last solve."""
    return math.log(d.c_scale), math.log(T), po5_formula(d) == pw_proof.RUN_FLOOR, len(failed_ids(d.proof))


def best_c(d):
    """The ln c factor of the tried solve with the fewest failed rows (then the run nearest to sigma), when it is not
    the last solve; else None. Reason: with the reset rule G >= X L, a smaller c raises each gain and L by the same
    factor, so c moves the run length only through the reward (1 + L)^q. In Moss Works c / e^8 moved the run only
    from 1832 s to 697 s, and in Kiln n Kin c x e^13 left the runs at the run floor and the active player reached 28
    of 66 elements, not 64."""
    _, T, sigma, _ = timing(d)
    if not (d.c_hist and T and sigma):
        return None
    tried = d.c_hist + [c_point(d, T)]
    best = min(tried, key=lambda h: (h[3], abs(h[1] - math.log(sigma))))
    return None if best is tried[-1] else best[0]


def next_log_c(hist, target, alpha):
    """The next ln c factor. With a run above and a run below sigma: log interpolation between the 2 nearest
    (T ~ c^(1/alpha) between them). Else 1 step of L6, ln c + alpha ln(sigma / T), doubled for each run in a row at
    the run floor (a run at the floor hides how short the run is)."""
    above = [h for h in hist if h[1] > target]
    below = [h for h in hist if h[1] < target]
    if above and below:
        return bracket_c(min(above, key=lambda h: h[1]), max(below, key=lambda h: h[1]), target)
    last = hist[-1]
    return last[0] + alpha * (target - last[1]) * 2 ** floor_run(hist)


def bracket_c(hi, lo, target):
    """ln c between a run above (hi) and a run below sigma (lo): the middle when lo is at the run floor, else log
    interpolation."""
    if lo[2]:
        return 0.5 * (hi[0] + lo[0])
    return lo[0] + (hi[0] - lo[0]) * (target - lo[1]) / (hi[1] - lo[1])


def floor_run(hist):
    """The count of the last solves in a row whose runs end at the run floor."""
    flags = [h[2] for h in hist]
    return len(flags) - 1 - max((i for i, f in enumerate(flags) if not f), default=-1)


def floor_bound_with_k(d):
    """K of D8 holds only when L6 sets the run length. When the later runs end at the run floor (the proof row PO5
    names it), c sets the gain of each run, not its length, and K in c only raises that gain: the solver solves the
    round again with c = A T_X^alpha X^(-alpha/s) of rule 4. Reason: in the trial Kiln n Kin, K in c ended the game
    at 3.3 h to 4.3 h, not 6 h; the round solved with no K ended at 5.35 h."""
    K = d.params.get("K_q", {}).get("value", 1.0)
    return abs(K - 1.0) > 1e-9 and po5_formula(d) == pw_proof.RUN_FLOOR


def po5_formula(d):
    """The formula value of the proof row PO5 (pw_proof.RUN_FLOOR when the later runs end at the run floor)."""
    po5 = next((r for r in d.proof["rows"] if r["id"] == "PO5"), {})
    return (po5.get("formula") or {}).get("value")


def record_round(d, s):
    """The extension of a ready round, and the record of the round: ready, and the failed rows. Rebalance mode: an
    element that the active player does not reach is not fitted (rb.mark_unreached)."""
    if d.plan is not None:
        rb.mark_unreached(d.plan, d.proof)
    d.ext = extension(d) if d.proof["ready"] else None
    ext_failed = [f"{pid} at {d.tv['H_extend']:g} h" for pid in failed_ids((d.ext or {}).get("proof"))]
    ready = d.proof["ready"] and not ext_failed
    d.rounds.append(dict(s=s, ready=ready, failed=failed_ids(d.proof) + ext_failed, kept=character_rank(d.model)))
    return ready


def place_content(d):
    """Rebalance mode, steps 2 and 3: the parameters of each element, then the unlock points. Each placement round
    solves c and the spine with the content at its unlock times, runs active_greedy, and moves the unlocks into
    the largest gaps (proof row PC4). The last round turns each unlock time into a lifetime total."""
    plan = d.plan
    plan.times, plan.curve, plan.req_kind, plan.amounts, plan.costs = {}, None, "time", {}, {}
    plan.bank = "S08" in d.chosen
    rb.fit_all(plan, d)
    plan.growth = 3600.0 * d.val("q") * math.log1p(X_E) / (run_minutes(d)[0] * 60.0) / (1.0 - d.val("R"))
    plan.guess_times(d.tv["H"])
    place_rounds(d, plan)
    N = stage_count(d, d.tv["H"])
    rb.reach_fix(plan, plan.last_sim, plan.steps.get(N, d.tv["H"] * HOUR))
    fit_timing(d)


def fit_timing(d):
    """The timing fit by simulation (pw_timing): the walls, the unlock points, the first costs and the reset follow
    the target timeline, with each effect held. N is the count of stages of the timeline."""
    plan = d.plan
    d.timeline = pw_timing.Timeline(stage_times(d), d.tv, d.val("T_X"))
    p, q, alpha = d.val("S04.power.p"), d.val("q"), 1.0 / (1.0 - d.val("R"))
    ctx = dict(build=lambda: build_model(d), load=load, gen_cost=GEN_COST, spine=spine_cur(d), slope0=p * q / alpha,
               p=p)
    run, passes = pw_timing.fit(d, ctx)
    pw_timing.record(d, run, passes)
    plan.last_sim = run.sim
    note = "; then the timing fit by simulation: the median later run of active_greedy is T_X"
    d.params["c"]["source"] = d.params["c"]["source"].replace(note, "") + note
    d.setp("N", d.fit_N, None, "the count of stages of the target timeline (the timing fit puts stage N at H)",
           "PC1 (the hard end inside the length)")


def place_rounds(d, plan):
    """The placement rounds. Their ladders have PLACE_HEADROOM x the stages, so the stage at H comes from the run."""
    d.sim_steps, d.placing = None, True
    for i in range(rb.PLACE_ROUNDS):
        place_round(d, plan, i < rb.PLACE_ROUNDS - 1)
        plan.req_kind = "total"
    d.sim_steps, d.placing = plan.steps, False


def place_round(d, plan, move):
    """1 placement round. The last round does not move the unlocks: the final model is the model of its run."""
    solve_loop(d)
    solve_spine(d)
    m = load(build_model(d))
    sim = rb.run_active(m, d.tv["H"] * 1.5)
    plan.curve = rb.Curve(sim)
    ax = pw_players.axis_of(m)
    plan.steps = {n: v[0] for n, v in sim.step_clock[ax].items()}
    plan.last_sim = sim
    if not move:
        return
    plan.t_end = min(d.tv["H"] * HOUR, sim.t)
    plan.times = rb.place_once(plan, sim, ax, d.tv, plan.t_end)
    rb.freeze(plan, GEN_COST)


AUTOMATE_ROUNDS = 3


def automate(d):
    """Step 5 of the rebalance: when the purchase rate (PC6) is above its target and S24 is selected, the plan of
    the row goes into the model (key automation: the line and the minute when its auto-buy starts), and the round is
    proven again. The simulator buys with the auto-buy, so the purchases move; the step repeats while a plan is left."""
    rounds = iter(range(AUTOMATE_ROUNDS))
    while next(rounds, None) is not None and automate_once(d):
        prove_round(d)


def automate_once(d):
    """Put the plan of PC6 into the model. False when S24 is not selected or the row has no plan."""
    row = next((r for r in d.proof["rows"] if r["id"] == "PC6"), None)
    plan = (row or {}).get("plan") if "S24" in d.chosen else None
    if plan:
        d.automation = merge_automation(d.automation or [], plan, d.tv["rate_max"])
        d.model["automation"] = progress_unlocks(d, d.automation)
    return bool(plan)


def progress_unlocks(d, rows):
    """Rebalance mode: each auto-buy opens at a lifetime total of the main currency, the total of active_greedy at
    the minute of the plan (a game opens a thing at progress, not at minutes of active play). from_minutes stays as
    the minute that the simulator reports."""
    if d.plan is None or d.plan.curve is None:
        return rows
    return [dict(a, requires=[dict(type="total", currency=ECON, amount=float("%.4g" % d.plan.total_at(
        ECON, 60.0 * a["from_minutes"])))]) for a in rows]


def merge_automation(given, plan, rate_max):
    """The automation of the model with the plan: 1 row for each line, at the earlier start."""
    rows = {a["line"]: a for a in given}
    why = (f"the purchases of the active player in 1 minute go above {(1 - pw_proof.RATE_MARGIN) * rate_max:g}, the "
           f"target {rate_max:g} with a margin of {pw_proof.RATE_MARGIN:.0%} (PC6)")
    rows.update({a["line"]: dict(a, reason=why) for a in plan
                 if a["from_minutes"] < rows.get(a["line"], {}).get("from_minutes", math.inf)})
    return sorted(rows.values(), key=lambda a: (a["from_minutes"], a["line"]))


def prove_round(d):
    """The proof sheet of the round with no speed test; a ready round runs it again with the speed test."""
    d.sheet, d.proof = prove(d.model, speed=False)
    if d.proof["ready"]:
        d.sheet, d.proof = prove(d.model)


def failed_ids(proof):
    rows = (proof or {}).get("rows", [])
    return [r["id"] for r in rows if r["kind"] in pw_proof.READY_KINDS and r["status"] not in
            pw_proof.READY_STATUSES]


def solve(d):
    supported_or_conflict(d)
    static_target_conflicts(d.tv)
    if long_run(d):
        return solve_long(d)
    if run_rounds(d):
        return True
    if not d.proof.get("speed"):
        d.sheet, d.proof = prove(d.model)
    raise proof_conflict(d)


def solve_long(d):
    """1 long run (references/design-solver.md, "1 long run"): 1 round with R = 1 and no s. True when the result of
    the proof sheet is ready or simulated; else the conflict of the lead failed row."""
    long_round(d)
    d.rounds.append(dict(s=None, ready=d.proof["result"] != "not ready", failed=failed_ids(d.proof), kept=(0, 0, 0)))
    if d.proof["result"] != "not ready":
        return True
    raise proof_conflict(d)


def long_round(d):
    """The lines (R = 1), the active factor, g and the spine, the proof, then the automation plan of PC6 (S24). With
    an automation plan, g and the spine are solved again with the auto-buy in the model."""
    d.params, d.table, d.sim_steps, d.fit_N, d.fit_gain, d.automation = {}, None, None, None, None, None
    long_lines(d)
    solve_presence(d)
    d.placed = []
    long_spine(d)
    automate(d)
    if d.automation:
        long_spine(d)


def long_lines(d):
    """The lines of 1 long run: the generator with no cap and with count steps, the multiplier groups, R = 1 over
    these carriers at the middle of the length (long_carriers), and the group costs from g of L2 (long_costs)."""
    gen_v = d.chosen["S01"]
    b = d.catalog_param(gen_v, UNIT_PARAM.get(gen_v, "b"), "S01 default (the cost ratio of the generator)")
    d.gen_b, d.k_mid = b, gen_level(d, b, long_goal(d))
    long_carriers(d)
    solve_upkeep(d)
    long_costs(d)


def long_carriers(d):
    """R of the carriers (BM 3a, "The generator as a carrier"): the factor k of the generator has the local exponent
    1 / (k ln b), so the carriers share R = 1 - 1 / (k_mid ln b), with k_mid the generator level at the middle of
    the length. The generator takes its share as count steps, each group as b and d."""
    R = LONG_R - 1.0 / (d.k_mid * math.log(d.gen_b)) - content_share(d)
    d.setp("k_mid", d.k_mid, None, "the generator level at the middle of the length: L1 first, then the run with no "
           "stage (pw_proof.step_inputs)", "PO1 (R = 1 at the middle of the length)")
    d.setp("R", R, [0.0, LONG_R], "BM 3a: the carriers share R = 1 - 1 / (k_mid ln b) - the content share; with "
           "the factor k of the generator and the constant factors of the named content the sum is 1 at the middle "
           "of the length, so I ~ e^(g t) (L2) and 1 wall ratio gives a near constant unlock rate (L3)",
           "PO1, PO10 and PO12")
    solve_groups(d, R, 1)
    count_steps(d, d.gen_b)


STEP_VARIANT = "S12.count_steps"


def count_steps(d, b):
    """The count steps of the generator: x m at each n owned levels with r_eff = ln m / (n ln b) = r of 1 carrier.
    n is the selection parameter, else the smallest n of the proven range that keeps m inside its range (the finest
    steps, as for the groups)."""
    r, mp, np_ = d.val("r_group"), d.cat.param(STEP_VARIANT, "m"), d.cat.param(STEP_VARIANT, "n")
    n = d.over.get(STEP_VARIANT, {}).get("n") or max(np_["min"], math.ceil(math.log(mp["min"]) / (r * math.log(b))))
    if b ** (r * n) > mp["max"] + 1e-9:
        raise Conflict(f"r of the generator = {r:.3f} with n = {n:g}", "PO1 (BM 3a: r_eff = ln m / (n ln b))",
                       f"{STEP_VARIANT} m {mp['min']:g} to {mp['max']:g}, n {np_['min']:g} to {np_['max']:g}",
                       ["lower n of S12.count_steps", "add a multiplier group (S02), so that each carrier has a "
                                                      "smaller r"])
    d.catalog_param(STEP_VARIANT, "n", "the finest steps of the proven range (BM section 7, limit 6)", n)
    d.catalog_param(STEP_VARIANT, "m", f"m = b^(r n) with r = {r:.4f} of 1 carrier and b = {b:g} of the generator "
                    "(BM 3a: r_eff = ln m / (n ln b))", b ** (r * n))


GEN_R_FIRST = 0.3


def content_share(d):
    """The share of the growth that the named content carries (BM 3a, "Content in 1 long run"): the sum of ln F of
    its constant factors over g_goal H. 0 with no named content."""
    logs = sum(named_log(d, e) for e in timed_elements(d))
    return logs / (long_goal(d) * d.tv["H"] * HOUR)


def named_log(d, e):
    """ln of the whole constant factor of 1 named element: its mult effects, to the power of its cap for a line. A
    second generator line has no constant factor."""
    effs = [f for f in e.get("effects") or [] if f.get("kind") == "mult"]
    if e.get("kind") != "line":
        return sum(math.log(f["value"]) for f in effs if f["value"] > 0)
    step = side_step(d, effs[0]) if effs else None
    return side_cap(d, e, step) * math.log(step) if step else 0.0


def gen_level(d, b, goal, share=0.5):
    """The level of the generator at the share of the length, from L1 with R = 1 (S = I / g, I = I_0 e^(g t)): the
    first guess of k_mid. The solver then takes k_mid from the run (solve_growth)."""
    spend = GEN_R_FIRST * math.exp(goal * share * d.tv["H"] * HOUR) / goal
    return max(1.0, math.log(max(spend * (b - 1.0) / GEN_COST, b)) / math.log(b))


def long_goal(d):
    """g_goal = u ln(g of S03, default) / 3600: u unlocks for each hour at the default wall ratio."""
    return long_unlocks(d) * math.log(d.cat.param("S03.exponential", "g")["default"]) / HOUR


def long_costs(d):
    """The group costs a_g = GROUP_COSTS x F. L1 gives g = a v k prod (r (b_c - 1) / a_c)^r over the carriers (BM
    3a), with k = k_mid. F sets g to g_goal. The generator keeps its first cost, so it is the first purchase."""
    r, b, goal = d.val("r_group"), d.val("S02.levelled_factor.b"), long_goal(d)
    g1 = d.tv_a() * GEN_VALUE * d.k_mid * (r * (d.gen_b - 1.0) / GEN_COST) ** r * math.prod(
        (r * (b - 1.0) / a) ** r for a in group_costs(d))
    share = r * len(group_costs(d))
    d.setp("g_goal", goal, None, "u ln(g of S03, default) / 3600 with u the unlock target (L3)",
           "PO10 at the default wall ratio")
    d.setp("cost_scale", (g1 / goal) ** (1.0 / share), None, f"BM 3a: g = a v k prod (r (b_c - 1) / a_c)^r = "
           f"{g1:.4g} / F^{share:.3g} with k = k_mid = {d.k_mid:.3g}", "g = g_goal (PO10, PO13)")


def long_unlocks(d):
    return max(d.tv["u_min"], 60.0 / d.tv["gap_med"], content_unlocks(d))


def long_spine(d):
    solve_growth(d)
    solve_spine(d)
    d.model = build_model(d)
    prove_long(d)


GROWTH_TOL = 0.10
GROWTH_STEPS = 6


def solve_growth(d):
    """g of L2: the slope of ln I in the run with no stage (pw_proof.run_growth), and k_mid of that run, with the
    named content at its target times. While g differs from g_goal by more than GROWTH_TOL, or R with the factor k
    is not 1 at k_mid, the carriers take the new k_mid and the group costs take the ratio (rescale_carriers), at
    most GROWTH_STEPS times."""
    g, _ = fit_growth(d, measure_placed(d), GROWTH_STEPS)
    d.setp("g_run", g, None, "BM L2: the slope of ln I from hour 1 to H in the run with no stage "
           "(pw_proof.run_growth)", "PO6, PO10, PO12 and PO13")


def measure_placed(d):
    """Place the named content (none with no content file), then measure (g, k_mid)."""
    d.placed = []
    place_named(d)
    return measure_growth(d)


def fit_growth(d, gk, steps):
    """(g, k_mid) after at most `steps` steps of rescale_carriers; each step places the named content again."""
    if steps == 0 or growth_fits(d, *gk):
        return gk
    d.placed = []
    rescale_carriers(d, *gk)
    return fit_growth(d, measure_placed(d), steps - 1)


def growth_fits(d, g, k):
    off = d.val("R") + 1.0 / (k * math.log(d.gen_b)) + content_share(d) - LONG_R
    return abs(g / d.val("g_goal") - 1.0) <= GROWTH_TOL and abs(off) <= pw_proof.LONG_STEP_TOL / 2.0



def rescale_carriers(d, g, k):
    """The carriers at the measured k_mid, and the group costs times (g / g_goal)^(1 / share), with share the part
    of R that the groups hold (g ~ F^-share)."""
    p = d.params["cost_scale"]
    d.k_mid = level_at_goal(d, g, k)
    long_carriers(d)
    share = d.val("r_group") * len(group_costs(d))
    d.setp("cost_scale", p["value"] * (g / d.val("g_goal")) ** (1.0 / share), None, p["source"].split("; times")[0] +
           "; times (the measured g / g_goal)^(1 / share) (the simulator in the loop)", p["set_by"])


def level_at_goal(d, g, k):
    """k_mid at g_goal from the measured (g, k): L1 gives the spend S = I / g with I ~ e^(g t), and level k costs
    c b^k, so k moves by ((g_goal - g) H / 2 - ln(g_goal / g)) / ln b."""
    goal = d.val("g_goal")
    move = (goal - g) * d.tv["H"] * HOUR / 2.0 - math.log(goal / g)
    return max(1.0, k + move / math.log(d.gen_b))


def measure_growth(d):
    """(g, k_mid) of the run with no stage."""
    sim = pw_proof.growth_run(load(build_model(d, spine=placeholder_spine())), ECON, d.tv["H"])
    g, k = pw_proof.growth_slope(sim, ECON), pw_proof.level_at(sim, "gen", d.tv["H"] * HOUR / 2.0)
    if not g or g <= 0 or not k:
        raise Conflict(f"length_h = {d.tv['H']:g}", "PO10 and PO12 (L2 with R = 1: I ~ e^(g t))",
                       "g > 0 after hour 1", ["raise length_h above 1 hour", "select S04.power (a reset layer)"])
    return g, k


def prove_long(d):
    """The proof sheet of the round; a round that is not "not ready" runs it again with the speed test."""
    d.sheet, d.proof = prove(d.model, speed=False)
    if d.proof["result"] != "not ready":
        d.sheet, d.proof = prove(d.model)


def steps_line(d):
    """1 line on the count steps of the generator: refused in the selection, selected, or added by the solver."""
    if "S12" in d.refused:
        return STEPS_REFUSED
    return STEPS_ADDED if "S12" not in d.chosen else "The count steps of the generator are the selected S12.count_steps."


def long_report_lines(d, warnings):
    """The report of 1 long run: the result, the lists Proved and Simulated, not proved, the marks, then the detail."""
    parts = solver_marks(d)
    head = [f"# Design report ({d.pattern}): 1 long run with no reset", "",
            f"Result: {long_result(d)}.", "", LONG_TEXT, "", steps_line(d), ""] + omitted_lines(d)
    body = proved_lines(d.proof, parts) + mark_lines(parts) + checked_lines(d.proof)
    return head + body + detail_lines(d, warnings, None)


def long_result(d):
    if omitted(d):
        return LONG_RESULT["not ready"] + f" ({len(omitted(d))} selected systems are not written)"
    return LONG_RESULT[d.proof["result"]]


LONG_RESULT = {k: f"{k}: {v}" for k, v in pw_proof.RESULT_TEXT.items()}
LONG_TEXT = ("The selection has no reset layer, no S29 and no S22: 1 long run to the hard end. A game with no reset "
             "is a valid design, and its result has the same meaning as that of a game with a reset. The generator "
             "has no cap: it is a growth carrier with count steps (BM 3a). The solver sets R = 1 over the carriers at "
             "the middle of the length, measures g of I ~ e^(g t) (L2), sets 1 wall ratio w for a near constant "
             "unlock rate u = 3600 g / ln w (L3), and the stage count N for the length (L8). The rows of the reset "
             "loop (PO2 to PO5) are not applicable: no reset layer.")
STEPS_REFUSED = ("The selection refuses S12, and the solver still adds count steps (S12.count_steps) to the generator: "
                 "an uncapped additive line with no step adds nothing to R (L1), so it stops growing against the "
                 "multipliers. The way out is a generator with a cap, which then fails the row CL (core line life).")
STEPS_ADDED = ("The solver adds count steps (S12.count_steps) to the generator: the card is the proven form of a "
               "generator that the player buys to the end.")


def run_rounds(d):
    """The rounds of s from low to high. True at the first ready round. With no ready round, the solver solves the
    round with the fewest failed rows again, so that the model, the proof and the report are of that round."""
    s_max = s_limit(d.tv["Dg"])
    states = (round_state(d, s) for s in [s for s in S_CANDIDATES if s <= s_max] or [min(S_CANDIDATES)])
    first = next((st for st in states if st != "next"), "next")
    if first != "ready":
        best_round(d)
    return first == "ready"


def character_rank(m):
    """(identity findings, changed elements, changed fields) of a model: the smaller keeps more of the character of
    the content."""
    ch = (m or {}).get("character") or {}
    fields = sum(f["state"] == "changed" for e in ch.get("elements") or [] for f in e["fields"])
    return len(((m or {}).get("identity") or {}).get("findings") or []), ch.get("changed", 0), fields


def best_round(d):
    """The round with the fewest failed rows, then the fewest failed rows that are not content (CONTENT_ROWS), then
    the most character (character_rank: a larger s gives the caps more room), then the first; solved again when it
    is not the last."""
    best = min(d.rounds, key=round_key)
    if best is d.rounds[-1]:
        return
    rounds = d.rounds
    solve_round(d, best["s"])
    d.rounds = rounds
    d.best_s = best["s"]


def round_key(r):
    return len(r["failed"]), len(set(r["failed"]) - CONTENT_ROWS), r.get("kept", (0, 0, 0))


def round_state(d, s):
    """ready, short (the next rounds cannot fix it) or next."""
    if solve_round(d, s):
        return "ready"
    return "short" if content_short(d) else "next"


CONTENT_ROWS = {"PC2", "PC3", "PC4", "PC5", "PC6", "PV", "CG", "CL", "PO13"}


def content_short(d):
    """Only PC2, PC4, PV or PO13 fails, or an element is not fitted: a larger s does not add content, change the
    steps of the lines or the decision count of the player, so the next rounds cannot fix it."""
    failed = set(d.rounds[-1]["failed"])
    rows = CONTENT_ROWS - {"PV"} if dead_raises(d.proof) else CONTENT_ROWS
    return bool(failed) and failed <= rows or (d.plan is not None and bool(rb.not_fitted_hard(d.plan)))


def dead_raises(proof):
    """True when the row PV names a dead cap raise: the cap room of a larger s can give its line a cap (BM L7), so
    such a PV fail is not content (ticket 35: Moss Works stopped at s = 1.15 with 10 dead raises)."""
    pv = next((r for r in (proof or {}).get("rows") or [] if r["id"] == "PV"), None)
    return bool((pv or {}).get("dead"))


def not_held(proof):
    """The ids of the rows that make the result not ready: a row that is simulated, not proved, is not one."""
    held = pw_proof.READY_STATUSES + (pw_proof.SIMULATED,)
    return [r["id"] for r in proof["rows"] if r["kind"] in pw_proof.READY_KINDS and r["status"] not in held]


NAMED_ROWS = ("PC3", "CG")


def lead_row(failed):
    """The failed row that the report names first. A row of the balance comes first. The rows that only ask for
    named content (NAMED_ROWS) come after it. PO13 is a speed of the play test, not a fault of the design, so it
    comes last."""
    late = NAMED_ROWS + ("PO13",)
    rows = [p for p in failed if p not in late] + [p for q in late for p in failed if p == q]
    return rows[0] if rows else "ranges"


def proof_conflict(d):
    last = d.rounds[-1]
    rows = {r["id"]: r for r in d.proof["rows"]}
    pid = lead_row(not_held(d.proof) or failed_ids(d.proof) or last["failed"])
    row = rows.get(pid, {})
    rng = "1 long run, R = 1" if last["s"] is None else "s from %s to %s" % (d.rounds[0]["s"], last["s"])
    return Conflict(TARGET_OF.get(pid, pid), f"{pid}: {row.get('statement', 'a parameter outside its range')}",
                    rng + "; each parameter inside its range of assets/systems.json", ways_out(pid, row))


TARGET_OF = {"PC5": "reachable (each element before the hard end)", "PC6": "purchase_rate.max_per_min",
             "PC2": "purchase_gap.largest_max_min", "PV": "purchase_value.floor", "PO5": "run_min (the run length)", "PO6": "idle", "PO10": "session_min and unlocks.each_later_hour_min",
             "PO11": "unlocks.first_5_min_min", "PO12": "length_h", "PO13": "play_test_speed.k",
             "PO4": "reset_gap.Dg", "PC4": "purchase_gap.unlock_max_min and purchase_gap.unlock_first_hour_max_min",
             "PC3": "demo_end (new content in each part past the demo)", "CG": "fun FM-1 (the gap between content "
             "elements)", "CL": "the core line life (a designer rule with no source)"}


DIFFER_WAYS = ["the formula and the simulation differ by more than 20%: the simulated runs do not follow the law of "
               "the row (the content factors come late, or the first runs are longer than T_X). Read the simulated "
               "value of the row and the players of the workbook: they show the game that the player plays",
               "move the content that comes late (the section 'Elements' gives each unlock) earlier, so that each "
               "run holds the same factors"]


def ways_out(pid, row):
    fix = row.get("fix") or row.get("reason") or ""
    out = [f"the proof sheet gives: {fix}"] if fix else []
    if row.get("status") == "model and math differ":
        return out + DIFFER_WAYS
    out += WAYS.get(pid, ["relax the target of the failed obligation", "select another variant of S02 or S04"])
    return out


WAYS = {"PC2": ["add a cheap line or a reward where the gap is: the table 'What the player buys' of the last round "
                "gives the purchases of each 10 minutes", "relax purchase_gap.largest_max_min (CT3, SG:181)"],
        "PO6": ["relax the idle band to the value of the proof sheet", "add a presence gate (S07 or S08)"],
        "PC5": ["the section 'Reachable' gives the change for each element that the player does not reach"],
        "PC6": ["select S24.auto_buy: the solver writes the plan of the section 'Purchase rate' into the model",
                "relax purchase_rate.max_per_min (a designer target)"],
        "PO10": ["relax unlocks.each_later_hour_min", "lengthen run_min"],
        "PO12": ["relax length_h to the simulated length", "add content: more stages"],
        "PO13": ["lower play_test_speed.k", "lengthen run_min"],
        "PC3": ["name the content of each part past the demo: give each slot `content_<k>` of the model an element "
                "with an effect, a grant, or a line that needs it (the section 'Hard end and demo end')",
                "move the demo end later, or leave it out: with no demo end the row is not applicable"],
        "CG": ["add named content elements (a new line, a milestone with an effect) where the gaps are: the section "
               "'Gaps between content elements' gives the count and the minutes", "relax the FM-1 band of the fun "
               "targets (targets.fun, a target with no source)"],
        "CL": ["give the generator line no cap and count steps (S12.count_steps)", "raise the cap of the line"],
        "PV": ["give an additive line count steps (S12.count_steps): 1 count block is then tested, not 1 level",
               "lower purchase_value.floor (a designer target with no source)"],
        "PC4": ["add content where the gaps are: the section 'Gaps between unlocks' of the last round gives the count "
                "of unlocks and the minutes of each gap", "relax purchase_gap.unlock_max_min or "
                "purchase_gap.unlock_first_hour_max_min (designer targets with no source)"]}


def extension(d):
    """The same formulas with more content: only the stage count N grows (BM L8)."""
    H = d.tv["H_extend"]
    if not H:
        return None
    m = build_model(d, H=H)
    _, data = prove(m, speed=False)
    return dict(hours=H, N=stage_count(d, H), N_demo=stage_count(d, d.tv["H"]), proof=data)


def fmtv(v):
    if isinstance(v, float):
        return f"{v:.4g}"
    if isinstance(v, list):
        return "[" + ", ".join(fmtv(x) for x in v) + "]"
    if isinstance(v, dict):
        return ", ".join(f"{k} {fmtv(x)}" for k, x in v.items())
    return "" if v is None else str(v)


def param_lines(d):
    head = ["| Parameter | Value | Range | Source | Set by |", "| --- | --- | --- | --- | --- |"]
    rows = [param_row(d, n, p) for n, p in sorted(d.params.items())]
    return head + rows


def param_row(d, n, p):
    """1 row of the table Parameters. With a wall table, the row of g gives the ratios of the table, not g."""
    table = (((d.model or {}).get("ladders") or [{}])[0].get("requires") or {}).get("table") or []
    if n != "S03.exponential.g" or len(table) < 2:
        return f"| {n} | {fmtv(p['value'])} | {fmtv(p['range']) or 'derived'} | {p['source']} | {p['set_by']} |"
    rs = pw_proof.wall_ratios(table)
    return (f"| wall table steps (not g) | smallest {min(rs):.4g}, median {statistics.median(rs):.4g}, largest "
            f"{max(rs):.4g} | {fmtv(p['range'])} | the wall table of the timing fit ({len(table)} walls) | WS |")


def selection_lines(d):
    return [x for sid, vid in sorted(d.chosen.items()) for x in [system_line(sid, vid)] + entry_lines(d, sid)]


def system_line(sid, vid):
    return f"- {sid}: {vid}" + ("" if sid in MODELLED else " (selected; the solver writes no line for it: simulation "
                                "and judgment, see references/design-solver.md)")


def entry_lines(d, sid):
    """1 line for each entry of a system with 2 or more entries (references/design-solver.md, "Selection entries")."""
    ents = [e for e in d.entries if e["system"] == sid]
    return [f"  - entry {e['id']}: {e['variant']}" + (f", role {e['role']}" if e.get("role") else "") for e in ents
            if len(ents) > 1]


def idle_lines(proof, pattern):
    meas = (proof["inputs"].get("measured") or {}).get("idle") or {}
    row = next(r for r in proof["rows"] if r["id"] == "PO6")
    pts = (row.get("formula") or {}).get("value") or row["value"]
    closed = "closed: not applicable (no offline gain by design)" if pattern == "continuous" else \
        f"closed {meas.get('closed')}"
    return [f"- Formula (L4b): {pts}", f"- Simulation: open {meas.get('open')}; {closed}"]


def extension_lines(ext):
    if not ext:
        return []
    p = ext["proof"]
    cell = pw_proof.cell
    rows = [f"| {r['id']} | {cell(r['value'])} | {cell(r['limit'])} | {r['status']} |" for r in p["rows"]
            if r["kind"] == "obligation"]
    return ["## Extension to %g hours" % ext["hours"], "",
            f"The formulas do not change. Only the stage count grows: N = {ext['N_demo']} for the demo, "
            f"N = {ext['N']} for {ext['hours']:g} hours (BM L8: each later hour adds u stages). The next "
            "content (more stages, and the systems that the stages open) fills the added stages.",
            f"Proof sheet of the {ext['hours']:g}-hour model: ready = {p['ready']}.", "",
            "| Id | Value | Limit | Status |", "| --- | --- | --- | --- |"] + rows + [""]


def end_lines(d):
    """The hard end, the demo end and the parts past the demo (rows PC1 to PC3 of the proof sheet)."""
    rows = {r["id"]: r for r in d.proof["rows"] if r["kind"] == "pattern"}
    if not rows:
        return []
    t = d.model["game"]["targets"]
    demo = (f"{d.tv['demo_min']:g} minutes, at stage {t['demo_end']['axis_step']}, before "
            f"{d.tv['demo_before'] or 'the next system'}") if "demo_end" in t else "no demo"
    out = ["## Hard end and demo end", "",
           f"- Hard end ({d.chosen['S28']}): stage {t['hard_end']['axis_step']}. PC1: {rows['PC1']['value']} "
           f"({rows['PC1']['status']}).", f"- Demo end: {demo}.",
           f"- PC2: {rows['PC2']['value']} ({rows['PC2']['status']})" + pc2_where(rows["PC2"]) + ".",
           f"- PC3, the parts past the demo: {rows['PC3']['value']} ({rows['PC3']['status']}).", ""]
    return out + SLOT_TEXT if "demo_end" in t else out


def pc2_where(row):
    w = row.get("where")
    if row["status"] in PASSES or not w:
        return ""
    return (f"; the largest gap is between {w['start_minutes']:g} and {w['end_minutes']:g} min: the content has no "
            "purchase there (add a line or a reward there)")


SLOT_TEXT = ["Each `content_<k>` is a slot: a milestone with no effect. The solver puts 1 slot in each part past the "
             "demo. An empty slot is not content: PC3 fails for its part until the designer gives the slot a named "
             "element (a system, an era, an area or a reset mechanic, SG:185) with an effect, a grant, or a line "
             "that needs it. The solver does not balance that content, so a simulation of the filled model must "
             "check it.", ""]


def report_lines(d, warnings, ext):
    head = [f"# Design report ({d.pattern})", "", f"Ready: {ready_text(d)}. "
            f"Rounds: {len(d.rounds)} ({'; '.join(round_text(r) for r in d.rounds)}).", ""] + best_line(d)
    return head + omitted_lines(d) + structure_head(d) + detail_lines(d, warnings, ext)


def structure_head(d):
    """Rebalance mode: the section "Structure of the game" near the top of the report (pw_rebalance)."""
    if d.plan is None or not d.model:
        return []
    return [""] + rb.structure_lines(d.plan, d.model, failed_ids(d.proof))


def has_level_reward(m):
    """S12 in the model: a milestone that a level of a line opens (a count step)."""
    return any(r.get("type") == "level" for x in m.get("milestones") or [] for r in x.get("requires") or [])


def has_cap_raise(m):
    """S13 in the model: a cap effect on a line with a cap."""
    capped = {p["id"] for p in m.get("purchases") or [] if p.get("max_level") is not None}
    owners = (m.get("purchases") or []) + (m.get("milestones") or []) + (m.get("ladders") or [])
    return any(f.get("kind") == "cap" and f.get("target") in capped for x in owners for f in x.get("effects") or [])


IN_MODEL = {"S12": has_level_reward, "S13": has_cap_raise}


def omitted(d):
    """Item 7 of ticket 47: each selected system that the solver path does not write, and that no element of the model
    writes. Each one makes the result not ready, and the report names it."""
    m = d.model or {}
    written = {sid.upper() for sid in [x.get("system") or "" for x in (m.get("purchases") or []) +
                                       (m.get("milestones") or [])]}
    return [p for p in solver_marks(d) if p["mark"] == "not written" and p["system"] not in written and
            not IN_MODEL.get(p["system"], lambda _m: False)(m) and not (p["system"] == "S04" and long_run(d))]


def design_parts(d):
    """The sections "Selected systems not written" and "Structure of the game" of a conflict report."""
    return omitted_part(d) + (structure_head(d)[1:] + [""] if d is not None else [])


def omitted_part(d):
    return omitted_lines(d)[1:] + [""] if d is not None and omitted(d) else []


def omitted_lines(d):
    out = omitted(d)
    if not out:
        return []
    return ["", "## Selected systems not written", "", f"{len(out)} selected systems are not written. Each system "
            "below is in the selection, and the model has no element of it. The result is not ready until a person writes it or the selection drops it.", ""] + \
        [f"- {p['system']} {p['variant']}: {p['reason']}" for p in out]


def best_line(d):
    s = getattr(d, "best_s", None)
    return ["", f"The model, the proof and this report are of the round s = {s:g}: the round with the fewest "
                "failed rows."] if s is not None else []


def ready_text(d):
    if d.proof["ready"] and omitted(d):
        return f"no ({len(omitted(d))} selected systems are not written: {', '.join(p['system'] for p in omitted(d))})"
    if d.proof["ready"] and not_fitted_count(d):
        return f"no ({not_fitted_count(d)} elements not fitted)"
    if d.proof["ready"] and identity_count(d):
        return f"no ({identity_count(d)} pairs of lines have the same shape)"
    return "yes" if d.proof["ready"] else "no"


def detail_lines(d, warnings, ext):
    speed = d.proof.get("speed") or {}
    head = ["## Selected systems", ""] + selection_lines(d) + ["", "## Target warnings", ""]
    body = ([f"- {w}" for w in warnings] or ["- none"]) + [""] + end_lines(d) + timeline_lines(d)
    body += unlock_gap_lines(d.proof)
    body += purchase_value_lines(d.proof) + reach_lines(d.proof) + rate_lines(d)
    body += content_lines(d) + ["## Parameters", ""] + param_lines(d) + ["", "## Idle value against active play", ""]
    body += idle_lines(d.proof, d.pattern) + ["", "## Play-test speed", "",
                                   f"Largest speed (formula): x{speed.get('largest_speed_formula')}. "
                                   f"Largest tested speed (simulation): x{speed.get('largest_speed_simulation')}. "
                                   f"Play tests run at x{speed.get('play_test_speed')} or slower: the smaller value.",
                                   ""]
    body += content_gap_lines(d.proof) + named_lines(d) + fun_lines(d)
    return head + body + extension_lines(ext) + ["## Proof sheet", ""] + pw_proof.sheet_lines(d.sheet)[2:]


FUN_SOLVER_ROWS = ("FM-1", "FM-2", "FM-4", "FM-5")


def content_gap_lines(proof):
    """The row CG: the largest content gap against the FM-1 band, and the content that each long gap needs."""
    row = next((r for r in proof["rows"] if r["id"] == "CG"), None)
    if row is None:
        return []
    out = ["## Gaps between content elements", "", f"CG: {row['value']} ({row['status']}; limit {row['limit']}). "
           "An axis step and an empty content slot are not content elements. The band is the FM-1 band of the fun "
           "sheet: a target with no source, so the row has the range of such a target. The row decides the result.",
           ""]
    need = row.get("needs") or []
    if not need:
        return out
    out += [f"The content has too few elements for the band: {row['missing']} more elements are needed.", "",
            "| From (min) | To (min) | Gap (min) | More elements |", "| --- | --- | --- | --- |"]
    out += [f"| {n['start_minutes']:g} | {n['end_minutes']:g} | {n['gap_minutes']:g} | {n['more']} |" for n in need]
    return out + [""]


def fun_lines(d):
    """The section "Fun rows": the fun sheet of the model, a second result beside the proof result. The rows of
    FUN_SOLVER_ROWS are listed, and each row that is out has its cause. d.fun keeps the sheet."""
    if d.model is None or d.pattern != "continuous":
        return []
    d.fun = pw_fun.fun_sheet(load(d.model))
    rows = [r for r in d.fun["rows"] if r["id"] in FUN_SOLVER_ROWS or r["id"] in d.fun["counting_out"]]
    out = ["## Fun rows", "", "The fun sheet (scripts/pw_fun.py, references/fun-analysis.md) gives a second result "
           "beside the proof result. It is advice with evidence marks, not a proof: a fun row never makes the result "
           "ready or not ready.", "", f"Fun result (FM-1 to FM-8): {d.fun['result']}; counting rows out: "
           f"{', '.join(d.fun['counting_out']) or 'none'}.", "",
           "| Id | Name | Value | Band (mark) | Status | Cause | Change to try |", "| --- | --- | --- | --- | --- | --- | --- |"]
    out += [fun_row_line(r) for r in rows]
    return out + [""]


def fun_row_line(r):
    out = r["status"] == "out"
    cells = [r["id"], r["name"], r["value_text"], f"{r['band']} ({r['band_mark']})", r["status"],
             r["reason"] if out else "", r["change"] if out else ""]
    return "| " + " | ".join(str(c).replace("|", "/") for c in cells) + " |"


def timeline_lines(d):
    """The section "Target timeline" of the rebalance mode (the timing fit), else nothing."""
    return pw_timing.report_lines(d) if getattr(d, "fit_record", None) else []


def unlock_gap_lines(proof):
    """PC4: the largest gap between 2 unlocks, and the unlocks that each long gap needs."""
    row = next((r for r in proof["rows"] if r["id"] == "PC4"), None)
    if row is None:
        return []
    out = ["## Gaps between unlocks", "", f"PC4: {row['value']} ({row['status']}; limit {row['limit']}).", ""]
    need = row.get("needs") or []
    if need:
        out += need_head(need)
        out += [f"- {n['more']} between {n['start_minutes']:.1f} and {n['end_minutes']:.1f} min (gap "
                f"{n['gap_minutes']:.1f} min)" for n in need] + [""]
    return out


def purchase_value_lines(proof):
    """The row PV: each line below the floor, and the table "What the player buys" for each 10 minutes."""
    row = next((r for r in proof["rows"] if r["id"] == "PV"), None)
    if row is None or not row.get("lines"):
        return []
    out = ["## Purchase value", "", f"PV: {row['value']} ({row['status']}; limit {row['limit']}). Floor source: "
           f"{row['floor_source']}.", ""]
    out += [f"- {ln['id']}: below the floor ({pw_proof.gain_text(ln)})" for ln in row["lines"]
            if ln["status"] == "below"]
    out += [f"- {a['id']}: its cap raise on {a['target']} opens no level that the player buys ({a['reason']})"
            for a in row.get("dead") or []]
    return out + window_lines(row.get("windows") or [])


def reach_lines(proof):
    """PC5: each element that the active player does not reach before the hard end, and the change."""
    row = next((r for r in proof["rows"] if r["id"] == "PC5"), None)
    if row is None:
        return []
    out = ["## Reachable", "", f"PC5: {row['value']} ({row['status']}).", ""]
    return out + [f"- {u['id']} ({u['kind']}, {', '.join(u['requires']) or 'no requirement'}): {u['change']}"
                  for u in row.get("unreached") or []] + ([""] if row.get("unreached") else [])


def rate_lines(d):
    """PC6: the purchases for each minute of the active player and the automation (S24) of the model."""
    row = next((r for r in d.proof["rows"] if r["id"] == "PC6"), None)
    if row is None:
        return []
    out = ["## Purchase rate", "", f"PC6: {row['value']} ({row['status']}; limit {row['limit']}). The target is the "
           "most purchases that a person makes in 1 minute with no automation (designer target).", ""]
    auto = d.model.get("automation") or []
    if auto:
        out += ["### Automation (S24 auto-buy)", "", "| Line | Auto-buy from minute | Purchases in the busy minutes |",
                "| --- | --- | --- |"] + [f"| {a['line']} | {a['from_minutes']} | {a['purchases_in_busy_minutes']} |"
                                         for a in auto] + [""]
    elif "S24" not in d.chosen and row["status"] not in PASSES:
        out += [f"- S24 is not selected: {row['fix']}.", ""]
    if row["status"] not in PASSES and row.get("plan"):
        out += [f"- More automation is needed: {row['fix']}.", ""]
    return out


def window_lines(windows):
    if not windows:
        return []
    head = ["### What the player buys", "", "| Minutes | Purchases | Unlocks | Smallest gain | Largest gain |",
            "| --- | --- | --- | --- | --- |"]
    rows = [f"| {w['start_minutes']} to {w['start_minutes'] + 10} | {w['purchases']} | {w['unlocks']} | "
            f"{pct_text(w['gain_min'])} | {pct_text(w['gain_max'])} |" for w in windows]
    return head + rows + [""]


def pct_text(v):
    return "" if v is None else f"{v:.1%}"


def content_lines(d):
    """Rebalance mode: each element with its old and new values, the elements not fitted, the field map and the
    rules of the game."""
    plan = d.plan
    if plan is None:
        return []
    m = d.model
    out = ["## Content", "", rb.counts_line(plan), "", "### Character kept", ""] + rb.character_lines(m["character"])
    out += ["### Elements", "", rb.base_income_line(plan, m), ""] + rb.element_rows(plan)
    out += ["", "### Identity", ""] + rb.identity_lines(m["identity"])
    out += ["", "### Elements not fitted", ""] + rb.not_fitted_lines(plan) + ["", "### Bank", ""] + bank_text(d)
    out += ["", "### Limits of the rebalance", ""] + rb.limit_lines(plan)
    out += ["", "### Rules that the game must change", ""] + rb.rule_lines(m["rules"], plan.content)
    return out + ["", "### Field map", ""] + rb.map_lines(m["field_map"]) + [""]


def bank_text(d):
    if "S08" not in d.chosen:
        return ["- No S08 bank is selected."]
    return [f"- The bank ({d.chosen['S08']}) fills 1 charge each {d.val('S08.timed_bank.period_s'):g} s, up to "
            f"{d.val('S08.timed_bank.cap'):g}. Its repeated sink is the line `bank`: 1 charge for the first level, cost growth "
            f"{rb.BANK_SINK_GROWTH:g}, "
            f"{rb.bank_levels(d)} levels, x{rb.BANK_SINK_VALUE:g} each (a constant factor, BM L7)."]


def round_text(r):
    if r["s"] is None:
        return "1 long run: " + ("ready" if r["ready"] else "failed " + ", ".join(r["failed"] or ["ranges"]))
    return f"s = {r['s']:g}: " + ("ready" if r["ready"] else "failed " + ", ".join(r["failed"] or ["ranges"]))


def long_note(d):
    """The conflict report of 1 long run: the path, the line on the count steps, and the lists Proved, Simulated, not
    proved and Failed of the round."""
    if d is None or d.proof is None or not long_run(d):
        return []
    return ["", "## 1 long run with no reset", "", LONG_TEXT, "", steps_line(d), ""] + \
        proved_lines(d.proof, solver_marks(d))


def conflict_lines(c, warnings, d):
    out = ["# Design report: not ready", "", "No model inside the proven ranges meets the targets.", "",
           f"- Target: {c.target}", f"- Obligation: {c.obligation}", f"- Range: {c.rng}", "", "Ways out:", ""]
    out += [f"{i + 1}. {w}" for i, w in enumerate(c.ways)] + long_note(d) + ["", "## Target warnings", ""]
    out += [f"- {w}" for w in warnings] or ["- none"]
    out += [""] + mark_lines(solver_marks(d)) + checked_lines(d.proof if d is not None else None)
    out += design_parts(d)
    if d is not None and d.rounds:
        out += ["", "## Rounds", ""] + [f"- {round_text(r)}" for r in d.rounds] + best_line(d)
    if d is not None and d.proof is not None:
        out += ["", "# The last round", ""] + detail_lines(d, [], d.ext)
    return out


def write_text(path, lines):
    if not path:
        return
    write_file(path, "\n".join(lines) + "\n")


def write_proof(path, proof):
    if not path:
        return
    write_file(path, json.dumps(proof, indent=1, sort_keys=True) + "\n")


ARGS = (
    (("selection",), dict(nargs="?", help="the selection record (references/interview.md section 8, without "
                                         "targets)")),
    (("targets",), dict(nargs="?", help="the targets file: play_pattern and the targets of the interview")),
    (("--as-built",), dict(action="store_true", help="with --content: write the model of the game as built, with its "
                                                    "present numbers and no change (the targets file is optional)")),
    (("--out",), dict(metavar="MODEL", help="write the model JSON")),
    (("--report",), dict(metavar="FILE", help="write the report (markdown)")),
    (("--proof",), dict(metavar="FILE", help="write the proof sheet JSON of the model")),
    (("--content",), dict(metavar="FILE", help="rebalance mode: the content file of the game (scan_godot.py "
                                               "--content-out); each element keeps its place in the model")),
    (("--accept-change",), dict(metavar="NAME", action="append", default=[], help="rebalance mode: accept 1 named "
                                "change of the structure of the game (stage_after_reset, stage_test, "
                                "reset_gain_rounding, reset_count, cap:<line id>, all); the report lists the options")),
)


def parse(argv):
    ap = argparse.ArgumentParser(description="The design solver: targets and selected systems to a model.")
    for names, kw in ARGS:
        ap.add_argument(*names, **kw)
    return ap.parse_args(argv)


def prepare(a, cat):
    sel, tg = read_json(a.selection, "selection"), read_json(a.targets, "targets")
    pattern = pattern_of(tg)
    entries = selection_entries(sel, cat)
    chosen = chosen_systems(sel, cat)
    validate(chosen, pattern, cat)
    check_roles(entries, chosen)
    tv = target_values(tg)
    d = Design(chosen, parameter_overrides(sel, cat), tv, pattern, cat, entries)
    d.refused = sel.get("refused") or []
    if a.content:
        read_content(a, d, tv)
    return d


def read_content(a, d, tv):
    """The content file. 1 long run with a file of the content step (each element has opens_min,
    references/content-step.md): the law path places the named elements (d.named). Each other case: the rebalance
    mode (d.plan)."""
    elements = read_json(a.content, "content").get("elements") or []
    if long_run(d) and any("opens_min" in e for e in elements):
        d.named = elements
        return
    d.plan = content_plan(a.content)
    d.plan.inputs = rb.solver_inputs(tv["raw"].get("solver"))
    d.plan.accepted = accepted_changes(a.accept_change)


def accepted_changes(names):
    """The structure changes that the designer accepts (--accept-change). An unknown name stops the solver."""
    bad = [n for n in names if n not in rb.STRUCTURE_NAMES and not n.startswith("cap:")]
    check(bad, f"--accept-change: unknown name {', '.join(bad)}; the names are {', '.join(rb.STRUCTURE_NAMES)}")
    return set(names)


def content_plan(path):
    try:
        plan = rb.Plan(rb.read_content(path))
    except rb.ContentError as e:
        raise InputError(str(e))
    rb.check_lines(plan)
    return plan


DEFAULT_TARGETS = os.path.join(SKILL, "assets", "default_targets.json")


def run_as_built(a):
    """--as-built: the model of the game with its present numbers, for the proof sheet and the players of the game
    as it is. The targets file is the last positional argument, else assets/default_targets.json."""
    check(not a.content, "--as-built needs --content")
    check(not a.out, "--as-built needs --out")
    tg = read_json(a.targets or a.selection or DEFAULT_TARGETS, "targets")
    pattern, tv = pattern_of(tg), target_values(tg)
    try:
        content = rb.read_content(a.content)
        m = rb.as_built_model(content)
    except rb.ContentError as e:
        raise InputError(str(e))
    m["game"] = as_built_game(content, m, tv, pattern)
    load(m)
    write_model(a.out, m)
    pw_proof.print_lines([f"pw_design: the model of {content.get('game') or 'the game'} as built is in {a.out}"])
    return 0


def as_built_game(content, m, tv, pattern):
    d = Design({}, {}, tv, pattern, None)
    end = (content.get("end") or {}).get("mark") or dict(axis_step=m["ladders"][0]["count"])
    ends = dict(hard_end=dict(end), largest_purchase_gap_minutes=tv["pgap"],
                purchase_rate_max_per_minute=tv["rate_max"], first_purchase_minutes=tv["pfirst"],
                largest_unlock_gap_minutes=tv["ugap_max"], first_hour_unlock_gap_minutes=tv["ugap_first"])
    notes = ["pw_design.py --as-built wrote this model from the content file with the numbers of the game and no "
             "change.", "The active player plays the whole time: an active_only effect pays at its full value "
             "(active_fraction 1)."]
    m["milestones"] += pw_players.active_action(content, notes)
    off = content.get("offline") or {}
    presence = dict(open_rate=1.0, visit_minutes=VISIT_MINUTES)
    if off.get("cap_h"):
        presence["offline_cap_minutes"] = off["cap_h"] * 60.0
    return dict(name=f"{content.get('game') or 'Game'} as built", target_hours=tv["H"],
                session_minutes=tv["run_min"], progress_axis=AXIS, primary_profile="model", active_fraction=1.0,
                offline_rate=built_offline(content, pattern, tv), presence=presence, play_pattern=pattern,
                main_currency=content["currencies"]["main"], targets=dict(targets(d, tv["H"]), **ends),
                assumptions=notes)


def built_offline(content, pattern, tv):
    """The offline gain of the game: content key offline.rate (the S06 map), else 0 for a continuous game. The model
    as built keeps the gain that the game has, so its closed players show it."""
    off = content.get("offline") or {}
    if off.get("rate") is not None:
        return float(off["rate"])
    return 0.0 if pattern == "continuous" else tv["raw"].get("offline_rate", 0.0)


def run(a):
    if a.as_built:
        return run_as_built(a)
    check(not a.selection or not a.targets, "the solver needs the selection file and the targets file")
    cat = Catalog()
    d = prepare(a, cat)
    warnings = target_warnings(d.tv, cat, d.pattern)
    try:
        solve(d)
    except VariantConflict as c:
        return simulation_path(a, d, c, warnings)
    except Conflict as c:
        lines = conflict_lines(c, warnings, d)
        write_outputs(a, d)
        write_text(a.report, lines)
        pw_proof.print_lines(lines)
        return 1
    finish(a, d, warnings)
    if omitted(d):
        return 1
    if long_run(d):
        return PATH_EXIT[d.proof["result"]]
    return 0 if not_fitted_count(d) == 0 and identity_count(d) == 0 else 1


MARK_TEXT = {"law": "written, proved by a law", "simulated": "written, simulation only", "not written": "not written"}
SIM_WRITTEN = ("S07", "S09", "S24")


def solver_marks(d):
    """The mark of each selected system in the model of the solver (the parts of references/simulation-path.md)."""
    if d is None:
        return []
    chosen = dict(d.chosen)
    chosen.setdefault("S04", "S04.none")
    marks = [solver_mark(sid, vid, d.cat) for sid, vid in sorted(chosen.items())]
    marks = [long_mark(p) if long_run(d) and p["system"] == "S04" else p for p in marks]
    return [dict(p, entries=mark_entries(d, p["system"])) for p in marks]


def mark_entries(d, sid):
    """The model lines of a system, as 'entry id: line ids' for each selection entry of the system."""
    lines = model_lines(d, sid)
    if sid == "S30" and (d.model or {}).get("upkeep"):
        return ["upkeep"] + [p["id"] for p in lines]
    ents = [e for e in d.entries if e["system"] == sid]
    if len(ents) < 2:
        return []
    return [entry_text(e, lines + model_lines(d, "S30")) for e in ents]


def model_lines(d, sid):
    return [p for p in (d.model or {}).get("purchases") or [] if p.get("system") == sid.lower()]


def entry_text(e, lines):
    ids = [p["id"] for p in lines if p.get("entry") == e["id"]]
    return f"{e['id']} ({e.get('role') or e['variant']}): " + (", ".join(ids) or "no line")


def long_mark(p):
    """S04.none in 1 long run: the model has no reset layer, and no law of the reset loop applies. The mark stays
    simulated: the formula of PO6 holds only with no auto-buy in the model (pw_proof.AUTO_IDLE)."""
    return dict(p, mark="simulated", reason="1 long run: no reset layer, so the rows of the reset loop (PO2 to PO5) "
                "are not applicable; the laws of 1 long run prove the timing, and an auto-buy plan (S24) moves the "
                "idle row PO6 to the simulation")


def solver_mark(sid, vid, cat):
    base = dict(system=sid, variant=vid, name=cat.systems[sid]["name"], entries=[])
    if vid in UPKEEP_WRITTEN:
        return dict(base, mark="simulated", reason="the solver writes the upkeep block and 1 care line; no law gives the "
                    "path of u, so PU1, PU2 and PU4 are simulated, and PU3 is proved by its formula")
    if vid in SUPPORTED.get(sid, ()):
        return dict(base, mark="law", reason="a law of the solver covers the variant")
    if sid in SIM_WRITTEN:
        return dict(base, mark="simulated", reason="the solver writes it; no law covers it")
    why = VARIANT_CAUSE.get(vid) or ("the solver writes no line for it: simulation and judgment" if sid not in
                                     SUPPORTED else GENERIC_CAUSE)
    return dict(base, mark="not written", reason=why)


def mark_lines(parts):
    head = ["## Systems and marks", "", "Each selected system has 1 mark: written and proved by a law, written and "
            "checked by the simulation only, or not written (with the reason).", "",
            "| System | Variant | Mark | Entries | Reason |", "| --- | --- | --- | --- | --- |"]
    rows = [f"| {p['system']} {p.get('name', '')} | {p.get('variant') or '-'} | {mark_name(p)} | "
            f"{', '.join(p.get('entries') or []) or '-'} | {p.get('reason', '')} |" for p in parts]
    return head + rows + [""]


def mark_name(p):
    return MARK_TEXT.get(p.get("mark"), p.get("mark")) + (", fill by hand" if p.get("fill") else "")


def checked_lines(proof):
    """The targets that the solver could check: each row that decides the verdict, with its status."""
    head = ["## Targets checked", ""]
    if not proof:
        return head + ["None: the solver stopped before the proof sheet, so no target row has a status.", ""]
    rows = [r for r in proof["rows"] if r["kind"] in pw_proof.READY_KINDS]
    done = [f"{r['id']} ({r['status']})" for r in rows if r["status"] not in ("not computed", "not applicable")]
    left = [r["id"] for r in rows if r["status"] == "not computed"]
    return head + ["Checked: " + (", ".join(done) or "none") + ".",
                   "Not computed: " + (", ".join(left) or "none") + ".", ""]


PATH_EXIT = {"ready": 0, "simulated": 3, "not ready": 1}


def simulation_path(a, d, c, warnings):
    """Way out 2 of a variant conflict (references/simulation-path.md): the scaffold of pw_scaffold.py, the timing
    fit by simulation, and the proof sheet. The model marks each part (simulation_path). Exit code 3 is "simulated",
    1 is "not ready"."""
    import pw_scaffold
    d.model = pw_scaffold.fit(pw_scaffold.scaffold(d), d.tv)
    d.sheet, d.proof = prove(d.model)
    write_outputs(a, d)
    lines = path_lines(c, warnings, d)
    write_text(a.report, lines)
    pw_proof.print_lines(lines[:8])
    return PATH_EXIT[d.proof["result"]]


RESULT_TEXT = {k: f"{k}: {v}" for k, v in pw_proof.RESULT_TEXT.items()}


CONTENT_UNUSED = ("The content file is not used on the simulation path: the scaffold writes the lines of the "
                  "selection only, and the model holds no named element of the file. Write each named element as a "
                  "model line by hand (references/simulation-path.md, \"Complete a stub\"), and run the proof sheet "
                  "again.")


def content_unused(d):
    """1 warning when the run has a content file: the scaffold of the simulation path does not read it."""
    return [CONTENT_UNUSED, ""] if d.plan is not None or d.named else []


def path_lines(c, warnings, d):
    parts = d.model["simulation_path"]["parts"]
    head = [f"# Design report: {d.proof['result']} (simulation path)", "",
            f"The laws of the solver do not cover {c.target}: {c.obligation}.", "",
            "The solver took the simulation path (way out 2): scripts/pw_scaffold.py wrote the model, and the timing "
            "fit set the walls, the unlock points and the first costs by simulation (references/simulation-path.md).",
            "", f"Result: {RESULT_TEXT[d.proof['result']]}.", ""] + content_unused(d)
    body = proved_lines(d.proof, parts) + add_lines(d.model) + mark_lines(parts) + checked_lines(d.proof)
    body += fit_lines(d.model) + ["## Target warnings", ""] + ([f"- {w}" for w in warnings] or ["- none"]) + [""]
    body += ["## Ways out of the conflict", ""] + [f"{i + 1}. {w}" for i, w in enumerate(c.ways)] + [""]
    return head + body + ["## Proof sheet", ""] + pw_proof.sheet_lines(d.sheet)[2:]


PASSES = ("pass", pw_proof.IN_RANGE)


def proved_lines(proof, parts):
    """The 2 lists of the report rule, and the failed rows."""
    rows = [r for r in proof["rows"] if r["kind"] in pw_proof.READY_KINDS]
    failed = [f"- {r['id']}: {r['value']} (limit {r['limit']}). Fix: {r['fix']}" for r in rows if r["status"] == "fail"]
    return (report_list("Proved", proved_items(rows, parts)) + report_list("Simulated, not proved",
            simulated_items(rows, parts)) + report_list("Failed", failed))


def report_list(head, items):
    return [f"## {head}", ""] + (items or ["- none"]) + [""]


def proved_items(rows, parts):
    """Each row that passes by a formula, and each part with the mark law."""
    proved = [f"- {r['id']}: {r['value']}" for r in rows if r["status"] in PASSES and r["source"] != "simulation"]
    return proved + [f"- {p['variant']}: {p['reason']}" for p in parts if p["mark"] == "law"]


def simulated_items(rows, parts):
    """Each row that only the simulation decides, and each part with the mark simulated."""
    sim = [f"- {r['id']}: {r['value']}" for r in rows if r["status"] == pw_proof.SIMULATED or
           (r["status"] in PASSES and r["source"] == "simulation")]
    return sim + [f"- {p.get('variant') or p.get('name')}: {p['reason']}" for p in parts if p["mark"] == "simulated"]


def add_lines(m):
    todo = m["simulation_path"].get("to_add") or []
    return ["## To add by hand", ""] + ([f"- {t}" for t in todo] or ["- nothing"]) + [""]


def fit_lines(m):
    fit = m["simulation_path"].get("fit")
    if not fit:
        return []
    rows = [f"| {r['what']} | {r['target_min']} | {r['sim_min'] if r['sim_min'] is not None else 'not reached'} |"
            for r in fit["rows"]]
    return (["## Target timeline (the timing fit)", "", f"Hard end: target {fit['hard_end_target_h']:g} h, "
             f"simulated {fit['hard_end_sim_h']} h.", "", flat_text(fit), "", "| What | Target (min) | Simulated (min) |",
             "| --- | --- | --- |"] + rows + [""])


def flat_text(fit):
    flat = fit.get("flat_stages") or []
    if not flat:
        return "The income rises between each 2 stages."
    causes = fit.get("causes") or []
    why = (" Probable cause: " + " ".join(causes)) if causes else " Content is possibly missing there (case 3 of the gate)."
    return ("The income does not rise before the stages " + ", ".join(str(n) for n in flat) + ": each such stage comes "
            "with the stage before it." + why)


def not_fitted_count(d):
    return 0 if d.plan is None else sum(1 for x in d.plan.els if x.status == "not fitted")


def identity_count(d):
    return len(((d.model or {}).get("identity") or {}).get("findings") or [])


def write_outputs(a, d):
    """The model and the proof sheet of the last round, also when the round is not ready."""
    if d.model is None:
        return
    if a.out:
        write_model(a.out, d.model)
    write_proof(a.proof, d.proof)


def finish(a, d, warnings):
    ext = d.ext
    write_outputs(a, d)
    lines = long_report_lines(d, warnings) if long_run(d) else report_lines(d, warnings, ext)
    write_text(a.report, lines)
    pw_proof.print_lines(lines[:4])


def main(argv=None):
    a = parse(argv)
    try:
        code = run(a)
    except InputError as e:
        code = input_error(e)
    return code


def input_error(e):
    sys.stderr.write(f"pw_design: {e}\n")
    return 2


if __name__ == "__main__":
    sys.exit(main())
