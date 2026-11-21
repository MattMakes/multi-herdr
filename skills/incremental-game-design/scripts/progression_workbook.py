#!/usr/bin/env python3
"""Simulate an incremental-game model and write a pacing workbook.

    python3 progression_workbook.py model.json out.xlsx [options]

The model format is in references/model-schema.md. The script plays the model with a greedy buyer under
one to four reset rules (buyer profiles), records each event, and writes:
  * out.xlsx  - Dashboard, Params, Beats, Lulls, Swimlane, Profiles, Timeline, Sessions, Target, Walls,
                Purchases, Prestige, Ledger, Sensitivity, Compare, Income, Curves, Systems, Matrix, Links,
                Ideas, Reference. Each result is there as a value of the script run ("at build") and,
                where a Params threshold decides it, as a live formula.
  * analysis JSON (default: <out>.analysis.json) - the same indicators as plain data, for each profile.

The code is in four modules beside this file: pw_model (load and check), pw_sim (the buyer),
pw_analysis (indicators), pw_sheets (the workbook).
"""
import argparse
import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from pw_analysis import (analyse, dead_zones, derive_links, ledger_rows, lull_causes, mark_robust,  # noqa: E402,F401
                         purchase_rows, run_profile, sensitivity, summary, wall_rows)
from pw_model import cost_at, fail, load_model, primary_name, profiles_for  # noqa: E402,F401
from pw_sim import WEIGHT, Sim  # noqa: E402,F401
import pw_players  # noqa: E402


ARGUMENTS = (
    (("model",), {}),
    (("out",), {}),
    (("--hours",), dict(type=float, help="hours of play to simulate (default: game.target_hours)")),
    (("--analysis",), dict(help="path for the analysis JSON (default: <out>.analysis.json)")),
    (("--profile",), dict(help="the primary buyer profile: eager, patient, doubler, or model "
                               "(default: game.primary_profile, or eager)")),
    (("--compare",), dict(metavar="ANALYSIS_JSON",
                          help="the analysis JSON of an earlier run (the 'before'). Fills the Compare sheet")),
    (("--no-sensitivity",), dict(action="store_true", help="skip the Sensitivity sheet (faster)")),
    (("--beat-weight",), dict(type=float, default=3)),
    (("--merge-seconds",), dict(type=float, default=30, help="beats closer than this are one moment")),
    (("--lull-ratio",), dict(type=float, default=1.0)),
    (("--max-gap-sessions",), dict(type=float, default=2)),
    (("--late-ratio",), dict(type=float, default=0.25,
                             help="late in the game a lull must also be longer than this value x time played")),
    (("--min-lull-minutes",), dict(type=float, default=10)),
    (("--ratio-floor-minutes",), dict(type=float, default=10)),
    (("--target-first-gap",), dict(type=float, default=0.5, help="Target sheet: minutes to the first beat")),
    (("--target-stretch",), dict(type=float, default=1.12, help="Target sheet: each gap x this value")),
    (("--players",), dict(metavar="LIST", help="players to simulate (pw_players.py): 'all', 'fast', 'none', or a "
                                                "comma list of names. Default: 'fast' when the model has "
                                                "game.presence or game.targets, else 'none'")),
)

PURCHASE_KEYS = ("id", "status", "first_bought_minutes", "highest_level", "cap_at_end",
                 "payback_first_seconds", "payback_last_seconds")

LIST_KEYS = ("currencies_without_source", "currencies_without_sink", "orphan_systems", "never_bought", "never_reached")


def _clean_dict(x):
    return {str(k): clean(v) for k, v in x.items() if not str(k).startswith("_")}


def _clean_list(x):
    return [clean(v) for v in x]


def _clean_float(x):
    finite = x == x and x not in (float("inf"), float("-inf"))
    plain = round(x, 4) if finite and abs(x) < 1e6 else x
    return plain if finite else None


CLEANERS = ((dict, _clean_dict), ((list, tuple), _clean_list), (float, _clean_float))


def clean(x):
    """Plain JSON data: no infinity, no private keys."""
    cleaner = next((fn for kind, fn in CLEANERS if isinstance(x, kind)), None)
    return cleaner(x) if cleaner else x


class Run:
    """The state of 1 script run. Each step reads and sets attributes."""
    players = presence = None


def parse_args():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    for flags, options in ARGUMENTS:
        ap.add_argument(*flags, **options)
    return ap.parse_args()


def arg_error(a):
    """Return the message of the first bad option, or None."""
    checks = ((a.hours is not None and a.hours <= 0, "--hours must be more than 0"),
              (a.target_first_gap <= 0 or a.target_stretch <= 0,
               "--target-first-gap and --target-stretch must be more than 0"))
    return next((message for bad, message in checks if bad), None)


def make_params(a, g, hours):
    return dict(beat_weight=a.beat_weight, merge_seconds=a.merge_seconds, lull_ratio=a.lull_ratio,
                max_gap_sessions=a.max_gap_sessions, late_ratio=a.late_ratio, min_lull_minutes=a.min_lull_minutes,
                ratio_floor_minutes=a.ratio_floor_minutes, session_minutes=g["session_minutes"],
                target_first_gap=a.target_first_gap, target_stretch=a.target_stretch, target_hours=hours)


def load_compare(path):
    other = json.load(open(path))
    if not isinstance(other, dict):
        raise ValueError("the top level is not an object")
    return other


def read_compare(path):
    """Return the analysis JSON of an earlier run. Exit on a bad file."""
    try:
        other = load_compare(path)
    except OSError as e:
        sys.exit(f"cannot read '{path}': {e.strerror}")
    except ValueError as e:
        sys.exit(f"'{path}' is not an analysis JSON: {e}")
    return other


def setup(r):
    r.a = parse_args()
    r.t_start = time.time()
    r.m = load_model(r.a.model)
    r.g = r.m["game"]
    error = arg_error(r.a)
    if error:
        sys.exit(error)
    r.hours = r.a.hours or r.g["target_hours"]
    r.P = make_params(r.a, r.g, r.hours)
    r.other = read_compare(r.a.compare) if r.a.compare else None


def set_profiles(r):
    r.profs = profiles_for(r.m)
    r.names = [p[0] for p in r.profs]


def select_model_profile(r):
    r.g["primary_profile"] = "model"
    set_profiles(r)


def apply_model_profile(r):
    """--profile model selects the model rule when the model does not name it."""
    use_model = r.a.profile == "model" and "model" not in r.names and len(r.profs) > 1
    if use_model:
        select_model_profile(r)


def apply_profile_option(r):
    apply_model_profile(r)
    if r.a.profile in r.names:
        r.g["primary_profile"] = r.a.profile
        return
    if len(r.profs) > 1:
        fail(f"--profile must be one of: {', '.join(r.names)}")
    print(f"note: --profile {r.a.profile} has no effect: this model has one buyer profile ('{r.names[0]}')")


def choose_profiles(r):
    set_profiles(r)
    if r.a.profile:
        apply_profile_option(r)
    r.primary = primary_name(r.m, r.profs)


def time_profile(r, name, prof):
    t0 = time.time()
    r.results[name] = run_profile(r.m, r.hours, prof, r.P)
    r.seconds[name] = time.time() - t0


def simulate(r):
    """One simulation for each buyer profile."""
    r.results, r.seconds = {}, {}
    for name, prof, _ in r.profs:
        time_profile(r, name, prof)
    r.sim, r.A = r.results[r.primary]
    lull_causes(r.m, r.sim, r.A, r.P)
    mark_robust(r.results, r.primary)
    r.profiles = [dict(name=n, text=text, summary=summary(r.m, *r.results[n])) for n, _, text in r.profs]


def simulate_players(r):
    """The players block (pw_players.py). Empty when the option and the model ask for no player."""
    names = pw_players.wanted(r.m, r.a.players)
    r.players = r.presence = None
    if not names:
        return
    v1_runs = {n: sim for n, (sim, _) in r.results.items()}
    r.players, r.presence, _ = pw_players.run_players(r.m, r.hours, names, r.primary, v1_runs)


def beat_warning(r):
    top = max([e["weight"] for e in r.sim.events] or [0])
    return (f"No event reaches the beat weight ({r.P['beat_weight']:g}). The highest weight in the run is "
            f"{top:g}. Add milestones, set 'excitement' on the events that matter, or run with "
            f"--beat-weight {max(1, top):g}. Until then the whole run is one lull.")


def cut_warning(r):
    return (f"The event limit cut the run at {r.A['simulated_hours']:.2f} hours of play. The indicators "
            "cover that span only.")


def stall_warnings(r):
    stalled = [] if r.sim.cut else r.A["stalled"]
    return [f"The buyer stalled at hour {s['at_minutes'] / 60:.2f}: {s['name']} ({s['note']})." for s in stalled]


def reset_warning(r, did):
    s = did["summary"]
    return (f"Under the primary profile '{r.primary}' the buyer never reset: the prestige gain for each "
            "minute of the run never fell from its best value. Under the profile "
            f"'{did['name']}' it reset {s['prestiges']} times (first at minute "
            f"{s['first_prestige_minutes']:.0f}). Read the Profiles sheet, and "
            f"consider --profile {did['name']}.")


def reset_warnings(r):
    if r.A["prestige_count"] != 0:
        return []
    did = [p for p in r.profiles if p["summary"]["prestiges"]]
    return [reset_warning(r, did[0])] if did else []


def collect_warnings(r):
    warnings = list(r.m["_warnings"])
    if r.A["beat_events"] == 0:
        warnings.append(beat_warning(r))
    if r.sim.cut:
        warnings.append(cut_warning(r))
    warnings += stall_warnings(r)
    warnings += reset_warnings(r)
    r.warnings = warnings
    r.A["warnings"] = warnings


def make_tables(r):
    m, sim = r.m, r.sim
    r.R = dict(sim=sim, A=r.A, P=r.P, primary=r.primary, profiles=r.profiles, warnings=r.warnings,
               purchases=purchase_rows(m, sim), ledger=ledger_rows(m, sim), walls=wall_rows(m, sim),
               compare=r.other, compare_path=r.a.compare or "", players=r.players, presence=r.presence)


def sensitivity_base(r):
    A = r.A
    return dict(lulls=len(A["lulls"]), longest_gap_minutes=round(A["longest_gap_minutes"], 1),
                first_prestige_minutes=A["first_prestige_minutes"],
                ladder_step=r.profiles[r.names.index(r.primary)]["summary"]["ladder_step"], beats=A["beats"])


def run_sensitivity(r):
    R = r.R
    r.A["_bought"] = {x["id"]: x["levels_bought"] for x in R["purchases"]}
    prof = next(p[1] for p in r.profs if p[0] == r.primary)
    R["sens"] = sensitivity(r.m, r.hours, prof, r.P, r.A, r.seconds[r.primary])
    R["sens_base"] = sensitivity_base(r)
    return dict(tested=R["sens"][1], possible=R["sens"][2], base=R["sens_base"], rows=R["sens"][0])


def write_sheets(r):
    r.sens_out = None if r.a.no_sensitivity else run_sensitivity(r)
    ref = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets", "reference_unlock_curve.json")
    from pw_sheets import write_workbook
    write_workbook(r.m, r.R, r.a.out, ref)


def beat_list(moments):
    return [dict(at_minutes=mo["at"], weight=mo["weight"], stacked=mo["stacked"],
                 name=" + ".join(mo["names"])) for mo in moments][:400]


def analysis_brief(r):
    A, R = r.A, r.R
    brief = {k: v for k, v in A.items() if k not in ("gaps", "moments")}
    brief.update(
        game=r.g["name"], primary_profile=r.primary, params=r.P,
        beat_times_minutes=[mo["at"] for mo in A["moments"]],
        beat_list=beat_list(A["moments"]),
        profiles={p["name"]: dict(p["summary"], reset_rule=p["text"]) for p in r.profiles},
        purchases=[{k: x[k] for k in PURCHASE_KEYS} for x in R["purchases"]],
        prestige_runs=[dict(x, top_steps=x["top_steps"]) for x in r.sim.runs[:200]],
        ledger=[dict(currency=c, direction=d, where=w, amount=v) for c, d, w, v in R["ledger"]],
        sensitivity=r.sens_out,
        compared_with=r.a.compare,
    )
    if r.players is not None:
        brief.update(players=r.players, presence=r.presence)
    return brief


def write_analysis(r):
    r.apath = r.a.analysis or os.path.splitext(r.a.out)[0] + ".analysis.json"
    brief = analysis_brief(r)
    try:
        json.dump(clean(brief), open(r.apath, "w"), indent=1)
    except OSError as e:
        sys.exit(f"cannot write '{r.apath}': {e.strerror}")


def print_lines(lines):
    """Print each line, as print() does for each one."""
    sys.stdout.write("".join(line + "\n" for line in lines))


def print_summary(r):
    A = r.A
    print(f"workbook: {r.a.out}\nanalysis: {r.apath}")
    print(f"simulated {A['simulated_hours']:.1f} h of play | primary profile: {r.primary} | events {A['events']} | "
          f"beats {A['beats']} (+{A['stacked_beats']} stacked) | lulls {len(A['lulls'])} | "
          f"longest gap {A['longest_gap_minutes']:.0f} min | median gap ratio {A['median_gap_ratio']:.2f} | "
          f"prestiges {A['prestige_count']} | {time.time() - r.t_start:.1f} s")


def lull_line(gp):
    return (f"  LULL {gp['after'] / 60:.2f} h -> {gp['at_minutes'] / 60:.2f} h ({gp['gap_minutes']:.0f} min) "
            f"[{gp['robust']}] cause: {gp['cause']}. Ends with: {gp['ends_with']}")


def dead_zone_line(z):
    return (f"  DEAD ZONE {z['start_minutes'] / 60:.2f} h -> {z['end_minutes'] / 60:.2f} h: {z['lulls']} lulls, "
            f"{z['beats_inside']} beats inside")


def profile_line(p):
    s = p["summary"]
    fp = "never" if s["first_prestige_minutes"] is None else f"{s['first_prestige_minutes']:.0f} min"
    lulls_at = ", ".join(f"{x[0]:.1f}-{x[1]:.1f}" for x in s["lull_hours"][:8]) or "none"
    return (f"  PROFILE {p['name']:8s} lulls {s['lulls']} | longest gap {s['longest_gap_minutes']:.0f} min | "
            f"beats {s['beats']} | first prestige {fp} | prestiges {s['prestiges']} | "
            f"lulls at h: {lulls_at}")


def print_console(r):
    A, profiles = r.A, r.profiles
    print_summary(r)
    print_lines(lull_line(gp) for gp in A["lulls"][:12])
    print_lines(dead_zone_line(z) for z in A["dead_zones"] if z["lulls"] > 1)
    print_lines(profile_line(p) for p in profiles if len(profiles) > 1)
    print_lines(f"  WARNING: {w}" for w in r.warnings)
    print_lines(f"  {k}: {', '.join(A[k][:15])}" for k in LIST_KEYS if A[k])
    if r.players is not None:
        print_lines(pw_players.player_line(row) for row in r.players)
        print_lines(pw_players.presence_lines(r.presence))


def main():
    r = Run()
    setup(r)
    choose_profiles(r)
    simulate(r)
    simulate_players(r)
    collect_warnings(r)
    make_tables(r)
    write_sheets(r)
    write_analysis(r)
    print_console(r)


if __name__ == "__main__":
    main()
