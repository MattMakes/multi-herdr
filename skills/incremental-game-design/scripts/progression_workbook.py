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

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))      # `python3 -I` does not add it

from pw_analysis import (analyse, dead_zones, derive_links, ledger_rows, lull_causes, mark_robust,  # noqa: E402,F401
                         purchase_rows, run_profile, sensitivity, summary, wall_rows)
from pw_model import cost_at, fail, load_model, primary_name, profiles_for  # noqa: E402,F401
from pw_sim import WEIGHT, Sim  # noqa: E402,F401


def clean(x):
    """Plain JSON data: no infinity, no private keys."""
    if isinstance(x, dict):
        return {str(k): clean(v) for k, v in x.items() if not str(k).startswith("_")}
    if isinstance(x, (list, tuple)):
        return [clean(v) for v in x]
    if isinstance(x, float):
        if x != x or x in (float("inf"), float("-inf")):
            return None
        return round(x, 4) if abs(x) < 1e6 else x
    return x


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("model")
    ap.add_argument("out")
    ap.add_argument("--hours", type=float, help="hours of play to simulate (default: game.target_hours)")
    ap.add_argument("--analysis", help="path for the analysis JSON (default: <out>.analysis.json)")
    ap.add_argument("--profile", help="the primary buyer profile: eager, patient, doubler, or model "
                                      "(default: game.primary_profile, or eager)")
    ap.add_argument("--compare", metavar="ANALYSIS_JSON",
                    help="the analysis JSON of an earlier run (the 'before'). Fills the Compare sheet")
    ap.add_argument("--no-sensitivity", action="store_true", help="skip the Sensitivity sheet (faster)")
    ap.add_argument("--beat-weight", type=float, default=3)
    ap.add_argument("--merge-seconds", type=float, default=30, help="beats closer than this are one moment")
    ap.add_argument("--lull-ratio", type=float, default=1.0)
    ap.add_argument("--max-gap-sessions", type=float, default=2)
    ap.add_argument("--late-ratio", type=float, default=0.25,
                    help="late in the game a lull must also be longer than this value x time played")
    ap.add_argument("--min-lull-minutes", type=float, default=10)
    ap.add_argument("--ratio-floor-minutes", type=float, default=10)
    ap.add_argument("--target-first-gap", type=float, default=0.5, help="Target sheet: minutes to the first beat")
    ap.add_argument("--target-stretch", type=float, default=1.12, help="Target sheet: each gap x this value")
    a = ap.parse_args()
    t_start = time.time()
    m = load_model(a.model)
    g = m["game"]
    if a.hours is not None and a.hours <= 0:
        sys.exit("--hours must be more than 0")
    hours = a.hours or g["target_hours"]
    if a.target_first_gap <= 0 or a.target_stretch <= 0:
        sys.exit("--target-first-gap and --target-stretch must be more than 0")
    P = dict(beat_weight=a.beat_weight, merge_seconds=a.merge_seconds, lull_ratio=a.lull_ratio,
             max_gap_sessions=a.max_gap_sessions, late_ratio=a.late_ratio, min_lull_minutes=a.min_lull_minutes,
             ratio_floor_minutes=a.ratio_floor_minutes, session_minutes=g["session_minutes"],
             target_first_gap=a.target_first_gap, target_stretch=a.target_stretch, target_hours=hours)
    other = None
    if a.compare:
        try:
            other = json.load(open(a.compare))
            if not isinstance(other, dict):
                raise ValueError("the top level is not an object")
        except OSError as e:
            sys.exit(f"cannot read '{a.compare}': {e.strerror}")
        except ValueError as e:
            sys.exit(f"'{a.compare}' is not an analysis JSON: {e}")

    # ---- one simulation for each buyer profile
    profs = profiles_for(m)
    names = [p[0] for p in profs]
    if a.profile:
        if a.profile == "model" and "model" not in names and len(profs) > 1:
            g["primary_profile"] = "model"
            profs = profiles_for(m)
            names = [p[0] for p in profs]
        if a.profile not in names:
            if len(profs) > 1:
                fail(f"--profile must be one of: {', '.join(names)}")
            print(f"note: --profile {a.profile} has no effect: this model has one buyer profile ('{names[0]}')")
        else:
            g["primary_profile"] = a.profile
    primary = primary_name(m, profs)
    results, seconds = {}, {}
    for name, prof, _ in profs:
        t0 = time.time()
        results[name] = run_profile(m, hours, prof, P)
        seconds[name] = time.time() - t0
    sim, A = results[primary]
    lull_causes(m, sim, A, P)
    mark_robust(results, primary)
    profiles = [dict(name=n, text=text, summary=summary(m, *results[n])) for n, _, text in profs]

    # ---- warnings
    warnings = list(m["_warnings"])
    if A["beat_events"] == 0:
        top = max([e["weight"] for e in sim.events] or [0])
        warnings.append(f"No event reaches the beat weight ({P['beat_weight']:g}). The highest weight in the run is "
                        f"{top:g}. Add milestones, set 'excitement' on the events that matter, or run with "
                        f"--beat-weight {max(1, top):g}. Until then the whole run is one lull.")
    if sim.cut:
        warnings.append(f"The event limit cut the run at {A['simulated_hours']:.2f} hours of play. The indicators "
                        "cover that span only.")
    for s in A["stalled"]:
        if not sim.cut:
            warnings.append(f"The buyer stalled at hour {s['at_minutes'] / 60:.2f}: {s['name']} ({s['note']}).")
    if A["prestige_count"] == 0:
        did = [p for p in profiles if p["summary"]["prestiges"]]
        if did:
            warnings.append(f"Under the primary profile '{primary}' the buyer never reset: the prestige gain for each "
                            "minute of the run never fell from its best value. Under the profile "
                            f"'{did[0]['name']}' it reset {did[0]['summary']['prestiges']} times (first at minute "
                            f"{did[0]['summary']['first_prestige_minutes']:.0f}). Read the Profiles sheet, and "
                            f"consider --profile {did[0]['name']}.")
    A["warnings"] = warnings

    # ---- tables
    R = dict(sim=sim, A=A, P=P, primary=primary, profiles=profiles, warnings=warnings,
             purchases=purchase_rows(m, sim), ledger=ledger_rows(m, sim), walls=wall_rows(m, sim),
             compare=other, compare_path=a.compare or "")
    sens_out = None
    if not a.no_sensitivity:
        A["_bought"] = {x["id"]: x["levels_bought"] for x in R["purchases"]}
        prof = next(p[1] for p in profs if p[0] == primary)
        R["sens"] = sensitivity(m, hours, prof, P, A, seconds[primary])
        R["sens_base"] = dict(lulls=len(A["lulls"]), longest_gap_minutes=round(A["longest_gap_minutes"], 1),
                              first_prestige_minutes=A["first_prestige_minutes"],
                              ladder_step=profiles[names.index(primary)]["summary"]["ladder_step"], beats=A["beats"])
        sens_out = dict(tested=R["sens"][1], possible=R["sens"][2], base=R["sens_base"], rows=R["sens"][0])
    ref = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets", "reference_unlock_curve.json")
    from pw_sheets import write_workbook
    write_workbook(m, R, a.out, ref)

    # ---- analysis JSON
    apath = a.analysis or os.path.splitext(a.out)[0] + ".analysis.json"
    brief = {k: v for k, v in A.items() if k not in ("gaps", "moments")}
    brief.update(
        game=g["name"], primary_profile=primary, params=P,
        beat_times_minutes=[mo["at"] for mo in A["moments"]],
        beat_list=[dict(at_minutes=mo["at"], weight=mo["weight"], stacked=mo["stacked"],
                        name=" + ".join(mo["names"])) for mo in A["moments"]][:400],
        profiles={p["name"]: dict(p["summary"], reset_rule=p["text"]) for p in profiles},
        purchases=[{k: x[k] for k in ("id", "status", "first_bought_minutes", "highest_level", "cap_at_end",
                                      "payback_first_seconds", "payback_last_seconds")} for x in R["purchases"]],
        prestige_runs=[dict(x, top_steps=x["top_steps"]) for x in sim.runs[:200]],
        ledger=[dict(currency=c, direction=d, where=w, amount=v) for c, d, w, v in R["ledger"]],
        sensitivity=sens_out,
        compared_with=a.compare,
    )
    try:
        json.dump(clean(brief), open(apath, "w"), indent=1)
    except OSError as e:
        sys.exit(f"cannot write '{apath}': {e.strerror}")

    # ---- console
    print(f"workbook: {a.out}\nanalysis: {apath}")
    print(f"simulated {A['simulated_hours']:.1f} h of play | primary profile: {primary} | events {A['events']} | "
          f"beats {A['beats']} (+{A['stacked_beats']} stacked) | lulls {len(A['lulls'])} | "
          f"longest gap {A['longest_gap_minutes']:.0f} min | median gap ratio {A['median_gap_ratio']:.2f} | "
          f"prestiges {A['prestige_count']} | {time.time() - t_start:.1f} s")
    for gp in A["lulls"][:12]:
        print(f"  LULL {gp['after'] / 60:.2f} h -> {gp['at_minutes'] / 60:.2f} h ({gp['gap_minutes']:.0f} min) "
              f"[{gp['robust']}] cause: {gp['cause']}. Ends with: {gp['ends_with']}")
    for z in A["dead_zones"]:
        if z["lulls"] > 1:
            print(f"  DEAD ZONE {z['start_minutes'] / 60:.2f} h -> {z['end_minutes'] / 60:.2f} h: {z['lulls']} lulls, "
                  f"{z['beats_inside']} beats inside")
    if len(profiles) > 1:
        for p in profiles:
            s = p["summary"]
            fp = "never" if s["first_prestige_minutes"] is None else f"{s['first_prestige_minutes']:.0f} min"
            print(f"  PROFILE {p['name']:8s} lulls {s['lulls']} | longest gap {s['longest_gap_minutes']:.0f} min | "
                  f"beats {s['beats']} | first prestige {fp} | prestiges {s['prestiges']} | "
                  f"lulls at h: {', '.join(f'{x[0]:.1f}-{x[1]:.1f}' for x in s['lull_hours'][:8]) or 'none'}")
    for w in warnings:
        print(f"  WARNING: {w}")
    for k in ("currencies_without_source", "currencies_without_sink", "orphan_systems", "never_bought", "never_reached"):
        if A[k]:
            print(f"  {k}: {', '.join(A[k][:15])}")


if __name__ == "__main__":
    main()
