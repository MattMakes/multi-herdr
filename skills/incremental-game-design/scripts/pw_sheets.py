"""The workbook writer for progression_workbook.py.

Rules: each formula uses Excel 2007 functions only. A value that comes from the simulation is written as a
value, and the sheet says so. Thresholds on Params are live inputs for the Timeline, Beats, Sessions,
Swimlane, Target, and Dashboard formulas.
"""
import bisect
import json
import math
import os

from pw_analysis import TARGET_HOURS, derive_links, ideal_count, ideal_time, is_lull, pm
from pw_model import INF, cost_one

# Timeline columns
TL = dict(real="A", hours="B", pm="C", ph="D", event="E", kind="F", system="G", weight="H", cost="I",
          currency="J", note="K", prest="L", beat="M", moment="N", last="O", gap="P", ratio="Q", lull="R",
          cum="S")
TL_NAMES = ["Minutes (real time)", "Hours (real time)", "Played min", "Played hours", "Event", "Kind", "System",
            "Weight", "Cost", "Currency", "Note", "Prestiges", "Beat", "Moment (first beat of a stack)",
            "Last moment at (played min)", "Gap (min)", "Gap ratio", "Lull", "Cumulative moments",
            "Gap at build (min)", "Lull at build", "Cumulative moments at build"]
TL_WIDTHS = [9, 8, 9, 8, 30, 10, 14, 7, 12, 10, 26, 8, 6, 9, 11, 10, 9, 7, 9, 9, 8, 10]
PARAM_ROWS = ["session", "beat", "merge", "ratio", "maxgap", "late", "minlull", "floor", "g0", "stretch",
              "target_hours", "swim"]
PR = {k: f"Params!$B${i}" for i, k in enumerate(PARAM_ROWS, 2)}
PR["end"] = "Params!$B$16"
PR["bucket"] = "Params!$B$17"
LAST_EDGE = 1e99
ORDER = ["Dashboard", "Params", "Beats", "Lulls", "Swimlane", "Profiles", "Timeline", "Sessions", "Target",
         "Walls", "Purchases", "Prestige", "Ledger", "Sensitivity", "Compare", "Income", "Curves", "Systems",
         "Matrix", "Links", "Ideas", "Reference"]


def lull_formula(gap, before):
    """The lull rule as a formula on two cell references."""
    return (f'IF(AND({gap}>={PR["minlull"]},OR({gap}/MAX({before},{PR["floor"]})>{PR["ratio"]},'
            f'{gap}>MAX({PR["maxgap"]}*{PR["session"]},{PR["late"]}*{before}))),"LULL","")')


def fin(x):
    """A number that a cell can hold, or None."""
    if x is None or isinstance(x, str):
        return x
    if x != x or x in (INF, -INF):
        return None
    return min(x, 1e300)


def write_workbook(m, R, out, ref_path):
    from openpyxl import Workbook
    from openpyxl.chart import BarChart, Reference, ScatterChart, Series
    from openpyxl.formatting.rule import CellIsRule, ColorScaleRule, FormulaRule
    from openpyxl.styles import Alignment, Border, Font, PatternFill, Side
    from openpyxl.utils import get_column_letter as L

    sim, A, P = R["sim"], R["A"], R["P"]
    F, B = Font(name="Arial", size=10), Font(name="Arial", size=10, bold=True)
    H = Font(name="Arial", size=10, bold=True, color="FFFFFF")
    T = Font(name="Arial", size=14, bold=True)
    INP = Font(name="Arial", size=10, color="0000FF")
    HF = PatternFill("solid", start_color="1F3864")
    YEL = PatternFill("solid", start_color="FFFF00")
    RED = PatternFill("solid", start_color="F8CBAD")
    GRN = PatternFill("solid", start_color="C6EFCE")
    GREY = PatternFill("solid", start_color="EDEDED")
    side = Side(style="thin", color="BFBFBF")
    box = Border(left=side, right=side, top=side, bottom=side)
    wrap = Alignment(wrap_text=True, vertical="top")

    def put(ws, r, c, v, font=F, fill=None, fmt=None, text=False):
        cell = ws.cell(row=r, column=c, value=fin(v))
        if text and isinstance(v, str):
            cell.data_type = "s"
        cell.font, cell.alignment, cell.border = font, wrap, box
        if fill:
            cell.fill = fill
        if fmt:
            cell.number_format = fmt
        return cell

    def head(ws, r, names, widths, freeze=True):
        for i, (n, w) in enumerate(zip(names, widths), 1):
            put(ws, r, i, n, H, HF)
            ws.column_dimensions[L(i)].width = w
        if freeze:
            ws.freeze_panes = ws.cell(row=r + 1, column=1)

    def note(ws, r, text, last_col=8, height=30, font=B):
        put(ws, r, 1, text, font, text=True)
        ws.merge_cells(start_row=r, start_column=1, end_row=r, end_column=last_col)
        ws.row_dimensions[r].height = height

    def chart_common(ch, title, x, y, w=22, h=9):
        ch.title, ch.x_axis.title, ch.y_axis.title = title, x, y
        ch.width, ch.height = w, h
        ch.roundedCorners = False
        ch.x_axis.delete = False
        ch.y_axis.delete = False
        if ch.legend is not None:
            ch.legend.overlay = False

    def line(ws, ycol, xcol, r1, r2, title=None, head_row=None):
        """A scatter series with no markers. Data rows r1..r2."""
        if title is None:
            s = Series(Reference(ws, min_col=ycol, min_row=head_row, max_row=r2),
                       Reference(ws, min_col=xcol, min_row=r1, max_row=r2), title_from_data=True)
        else:
            s = Series(Reference(ws, min_col=ycol, min_row=r1, max_row=r2),
                       Reference(ws, min_col=xcol, min_row=r1, max_row=r2), title=title)
        s.marker.symbol = "none"
        return s

    wb = Workbook()
    g = m["game"]
    end = A["simulated_end_minutes"]
    primary = R["primary"]

    # ---- Params ---------------------------------------------------------------
    wp = wb.active
    wp.title = "Params"
    head(wp, 1, ["Parameter", "Value", "Meaning"], [34, 12, 100])
    swim_n = min(48, A["sessions"])
    swim_bucket = P["session_minutes"] * max(1, math.ceil(end / P["session_minutes"] / 48 - 1e-9))
    params = [
        ("Session minutes", P["session_minutes"], "Length of one play session. From the model."),
        ("Beat weight", P["beat_weight"], "An event is a beat when its weight is this value or more. 1 = each purchase. 3 = features, prestiges that reach something new, system unlocks."),
        ("Merge seconds", P["merge_seconds"], "Beats closer than this to the first beat of a stack count as one moment."),
        ("Lull ratio", P["lull_ratio"], "A gap is a lull when gap / (time played before the gap) is more than this value. This rule decides early in the game."),
        ("Max gap in sessions", P["max_gap_sessions"], "A gap is also a lull when it is longer than this number of sessions, and longer than the late lull ratio x time played."),
        ("Late lull ratio", P["late_ratio"], "Later in the game a gap must also be longer than this value x time played. 0.25 = a 2.5-hour wait at hour 10 is normal."),
        ("Min lull minutes", P["min_lull_minutes"], "Gaps shorter than this value are never a lull."),
        ("Ratio floor minutes", P["ratio_floor_minutes"], "Lowest value for 'time played before the gap'. This prevents false lulls in the first minutes."),
        ("Target: first gap minutes", P["target_first_gap"], "Target sheet. Minutes from the start to the first ideal beat."),
        ("Target: stretch ratio", P["target_stretch"], "Target sheet. Each ideal gap is this value x the gap before it."),
        ("Target hours", P["target_hours"], "Target sheet. The ideal curve is shown to this time."),
        ("Swimlane bucket minutes", swim_bucket, f"Width of one column in the first two blocks of the Swimlane sheet. The sheet has {swim_n} columns; the last one counts all later events."),
    ]
    for i, (a, v, d) in enumerate(params, 2):
        put(wp, i, 1, a, B)
        put(wp, i, 2, v, INP, YEL)
        put(wp, i, 3, d)
    put(wp, 15, 1, "From the simulation (not inputs)", H, HF)
    put(wp, 15, 2, None, H, HF)
    put(wp, 15, 3, None, H, HF)
    put(wp, 16, 1, "Simulated end (minutes of play)", B)
    put(wp, 16, 2, end)
    put(wp, 16, 3, "The end of the simulated run. The Dashboard uses it for the hours, the tail gap, and the rates.")
    put(wp, 17, 1, "Bucket minutes (Sessions sheet)", B)
    put(wp, 17, 2, A["bucket_minutes"])
    put(wp, 17, 3, f"Width of one row on the Sessions sheet. The sheet has {A['sessions']} rows. The script sets it: the session length, or more for a long run.")
    put(wp, 19, 1, "Blue text on yellow: inputs. Change a value and the formulas on Timeline, Beats, Sessions, "
                   "Swimlane, Target, and Dashboard follow. The rows of all sheets come from the simulation: run "
                   "the script again after a change to the model. The rows of Beats, Lulls, and Target use the "
                   "beat weight and the merge seconds of the script run.", B)
    wp.merge_cells("A19:C19")
    wp.row_dimensions[19].height = 45

    # ---- Timeline -------------------------------------------------------------
    ev = sim.events
    for i, e in enumerate(ev):
        e["_i"] = i

    def routine_row(e):
        return e["weight"] < 2 and e["kind"] != "stall" and e["weight"] < P["beat_weight"]

    keep = [e for e in ev if not routine_row(e)]
    routine = [e for e in ev if routine_row(e)]
    room = 6000 - len(keep)
    dropped = len(routine) > max(room, 0)
    if dropped:
        routine = routine[::math.ceil(len(routine) / max(room, 1))] if room > 0 else []
    rows = sorted(keep + routine, key=lambda e: e["_i"])
    wt = wb.create_sheet("Timeline")
    curs = [c["id"] for c in m["currencies"]]
    names = TL_NAMES + [f"log10 income {c}" for c in curs]
    head(wt, 1, names, TL_WIDTHS + [11] * len(curs))
    c_ = TL
    n_inc = len(TL_NAMES) + 1
    anchor, cum = -1e6, 0
    for i, e in enumerate(rows, 2):
        put(wt, i, 1, round(e["real"] / 60.0, 4), fmt="0.0")
        put(wt, i, 2, round(e["real"] / 3600.0, 5), fmt="0.00")
        put(wt, i, 3, pm(e), fmt="0.0")
        put(wt, i, 4, round(pm(e) / 60.0, 5), fmt="0.00")
        # the same rule as the live columns M to S, as values of the script run
        if e["weight"] >= P["beat_weight"] and (pm(e) - anchor) * 60 >= P["merge_seconds"]:
            before = max(anchor, 0)
            cum += 1
            put(wt, i, 20, pm(e) - before, fmt="0.0")
            put(wt, i, 21, "LULL" if is_lull(pm(e) - before, before, P) else None)
            anchor = pm(e)
        else:
            put(wt, i, 20, None)
            put(wt, i, 21, None)
        put(wt, i, 22, cum)
        put(wt, i, 5, e["name"], text=True)
        put(wt, i, 6, e["kind"])
        put(wt, i, 7, e["system"], text=True)
        put(wt, i, 8, e["weight"], INP)
        put(wt, i, 9, e["cost"], fmt="0.00E+00")
        put(wt, i, 10, e["currency"], text=True)
        put(wt, i, 11, e["note"], text=True)
        put(wt, i, 12, e["prestiges"])
        put(wt, i, 13, f"=IF(H{i}>={PR['beat']},1,0)")
        put(wt, i, 14, f"=IF(AND(M{i}=1,(C{i}-O{i})*60>={PR['merge']}),1,0)")
        put(wt, i, 15, -1000000 if i == 2 else f"=IF(N{i - 1}=1,C{i - 1},O{i - 1})", fmt="0.0")
        put(wt, i, 16, f'=IF(N{i}=1,C{i}-MAX(O{i},0),"")', fmt="0.0")
        put(wt, i, 17, f'=IF(N{i}=1,P{i}/MAX(MAX(O{i},0),{PR["floor"]}),"")', fmt="0.00")
        put(wt, i, 18, f'=IF(N{i}=1,{lull_formula(f"P{i}", f"MAX(O{i},0)")},"")')
        put(wt, i, 19, f"=N{i}" if i == 2 else f"=S{i - 1}+N{i}")
        for k, c in enumerate(curs, n_inc):
            put(wt, i, k, round(math.log10(e["income"][c] + 1), 4), fmt="0.00")
    nT = max(len(rows) + 1, 2)
    wt.auto_filter.ref = f"A1:{L(len(names))}{nT}"
    wt.conditional_formatting.add(f"A2:{L(len(names))}{nT}", FormulaRule(formula=['$U2="LULL"'], fill=RED))
    wt.conditional_formatting.add(f"A2:{L(len(names))}{nT}", FormulaRule(formula=["$H2>=5"], fill=GRN))

    def tl(key):
        return f"Timeline!${c_[key]}$2:${c_[key]}${nT}"

    tC, tH, tM, tN, tF, tG, tK = tl("pm"), tl("weight"), tl("beat"), tl("moment"), tl("kind"), tl("system"), tl("note")

    # ---- Sessions -------------------------------------------------------------
    nS = A["sessions"]
    bucket = A["bucket_minutes"]
    wsn = wb.create_sheet("Sessions")
    head(wsn, 1, ["Bucket", "Start (played min)", "End (played min)", "Start (h)", "Beats (at build)",
                  "Beat weight sum (at build)", "Flag (at build)", "Beats (live)", "Beat weight sum (live)",
                  "Events (live)", "Purchases (live)", "Top weight (live)", "Flag (live)"],
         [8, 11, 11, 9, 9, 11, 9, 9, 11, 9, 10, 10, 9])
    sb_n, sb_w = [0] * nS, [0] * nS
    for e in ev:
        if e["weight"] >= P["beat_weight"]:
            k = min(nS - 1, int(pm(e) // bucket))
            sb_n[k] += 1
            sb_w[k] += e["weight"]
    for i in range(2, nS + 2):
        lt = "<=" if i == nS + 1 else "<"
        span = f'{tC},">="&B{i},{tC},"{lt}"&C{i}'
        put(wsn, i, 1, i - 1)
        put(wsn, i, 2, (i - 2) * bucket)
        put(wsn, i, 3, (i - 1) * bucket)
        put(wsn, i, 4, round((i - 2) * bucket / 60.0, 4), fmt="0.0")
        put(wsn, i, 5, sb_n[i - 2])
        put(wsn, i, 6, sb_w[i - 2])
        put(wsn, i, 7, None if sb_n[i - 2] else "DRY", fill=None if sb_n[i - 2] else RED)
        put(wsn, i, 8, f"=COUNTIFS({span},{tM},1)")
        put(wsn, i, 9, f"=SUMIFS({tH},{span},{tM},1)")
        put(wsn, i, 10, f"=COUNTIFS({span})")
        put(wsn, i, 11, f'=COUNTIFS({span},{tF},"purchase")')
        put(wsn, i, 12, f"=SUMPRODUCT(MAX(({tC}>=B{i})*({tC}{lt}C{i})*{tH}))")
        put(wsn, i, 13, f'=IF(H{i}=0,"DRY","")')
    wsn.conditional_formatting.add(f"F2:F{nS + 1}", ColorScaleRule(
        start_type="num", start_value=0, start_color="FFFFFF", mid_type="percentile", mid_value=50,
        mid_color="FFE699", end_type="max", end_color="63BE7B"))
    wsn.conditional_formatting.add(f"M2:M{nS + 1}", CellIsRule(operator="equal", formula=['"DRY"'], fill=RED))
    bc = BarChart()
    bc.add_data(Reference(wsn, min_col=6, min_row=1, max_row=nS + 1), titles_from_data=True)
    bc.set_categories(Reference(wsn, min_col=4, min_row=2, max_row=nS + 1))
    chart_common(bc, "Beat weight in each bucket (no bar = no beat = a dry session)", "Start hour (played)",
                 "Sum of the weights of the beats", 26, 9)
    bc.legend = None
    wsn.add_chart(bc, "O2")
    put(wsn, nS + 3, 1, "'At build' columns are values from the script run: a reader sees them with no "
                        "recalculation, and the chart uses them. 'Live' columns are formulas: they follow the beat "
                        "weight on Params, so the two can differ after an edit. Buckets count beats before stacked "
                        "beats are merged. Times are minutes of play.", B)
    wsn.merge_cells(start_row=nS + 3, start_column=1, end_row=nS + 3, end_column=13)
    wsn.row_dimensions[nS + 3].height = 45

    # ---- Beats ----------------------------------------------------------------
    wbt = wb.create_sheet("Beats")
    mos = A["moments"]
    head(wbt, 1, ["#", "Played min", "Played hours", "Real hours", "Beat", "Top weight", "Beats in the stack",
                  "Systems", "Gap (min, at build)", "Gap ratio (at build)", "Lull (at build)", "Cause of the lull",
                  "Last beat at (min, live)", "Gap (min, live)", "Gap ratio (live)", "Lull (live)"],
         [6, 10, 9, 9, 50, 8, 8, 18, 10, 9, 8, 18, 10, 10, 9, 8])
    tail_gap = dict(at=end, after=A["last_beat_minutes"], gap=A["tail_gap_minutes"],
                    ratio=A["tail_gap_minutes"] / max(A["last_beat_minutes"], P["ratio_floor_minutes"]),
                    lull=is_lull(A["tail_gap_minutes"], A["last_beat_minutes"], P))
    cause_at = {round(x["at_minutes"], 4): x.get("cause") for x in A["lulls"]}
    for i, mo in enumerate(mos + [None], 2):
        if mo is None:
            mo = tail_gap
            put(wbt, i, 1, "end")
            put(wbt, i, 4, round(sim.real / 3600.0, 3), fmt="0.00")
            put(wbt, i, 5, "(end of simulation: no beat)", B)
            for k in (6, 7, 8):
                put(wbt, i, k, None)
        else:
            put(wbt, i, 1, i - 1)
            put(wbt, i, 4, round(mo["real"] / 60.0, 3), fmt="0.00")
            put(wbt, i, 5, " + ".join(mo["names"]), text=True)
            put(wbt, i, 6, mo["weight"])
            put(wbt, i, 7, mo["stacked"])
            put(wbt, i, 8, ", ".join(mo["systems"]), text=True)
        put(wbt, i, 2, mo["at"], fmt="0.0")
        put(wbt, i, 3, round(mo["at"] / 60.0, 5), fmt="0.00")
        put(wbt, i, 9, mo["gap"], fmt="0.0")
        put(wbt, i, 10, mo["ratio"], fmt="0.00")
        put(wbt, i, 11, "LULL" if mo["lull"] else None)
        put(wbt, i, 12, cause_at.get(round(mo["at"], 4)) if mo["lull"] else None)
        put(wbt, i, 13, 0 if i == 2 else f"=B{i - 1}", fmt="0.0")
        put(wbt, i, 14, f"=B{i}-M{i}", fmt="0.0")
        put(wbt, i, 15, f"=N{i}/MAX(M{i},{PR['floor']})", fmt="0.00")
        put(wbt, i, 16, "=" + lull_formula(f"N{i}", f"M{i}"))
    nB = len(mos) + 2                      # the last row is the end of the simulation
    wbt.conditional_formatting.add(f"A2:L{nB}", FormulaRule(formula=['$K2="LULL"'], fill=RED))
    wbt.conditional_formatting.add(f"M2:P{nB}", FormulaRule(formula=['$P2="LULL"'], fill=RED))
    note(wbt, nB + 2, "One row for each moment: beats closer than the merge seconds are one row. Red rows end a "
                      "lull. 'At build' columns are values from the script run. 'Live' columns are formulas: they "
                      "follow the lull thresholds on Params, so the two can differ after an edit. The rows "
                      "themselves use the beat weight and the merge seconds of the script run.", 16, 45)
    # lull bands for the pacing chart: one rectangle outline for each lull, from the script run
    put(wbt, 1, 18, "Lull band: hours", H, HF)
    put(wbt, 1, 19, "Lull band: beats", H, HF)
    wbt.column_dimensions["R"].width = wbt.column_dimensions["S"].width = 11
    r = 2
    for gp in A["lulls"]:
        for x, y in ((gp["after"], 0), (gp["after"], len(mos)), (gp["at_minutes"], len(mos)), (gp["at_minutes"], 0)):
            put(wbt, r, 18, round(x / 60.0, 4), fmt="0.00")
            put(wbt, r, 19, y)
            r += 1
        r += 1                             # an empty row breaks the line between two bands
    nBand = max(r - 2, 2)

    # ---- Lulls ----------------------------------------------------------------
    wu = wb.create_sheet("Lulls")
    head(wu, 1, ["Starts at (h)", "Ends at (h)", "Gap (min)", "Gap ratio", "Ends with", "System that ends it",
                 "Sessions in the gap (at build)", "In all buyer profiles?", "Probable cause", "Why (from the simulation)",
                 "Options within 5 min (mean)", "Capped lines (share)", "log10 income growth", "Wall: need / have",
                 "Resets in the gap", "Purchases in the gap", "Min since last new system", "Dead zone",
                 "Diagnosis", "Candidate fix"],
         [9, 9, 9, 8, 30, 14, 9, 14, 16, 46, 10, 9, 9, 9, 8, 9, 10, 7, 44, 44])
    notes = [dict(x) for x in ((m.get("review") or {}).get("lull_notes") or []) if isinstance(x, dict)]
    for i, gp in enumerate(A["lulls"], 2):
        start = gp["after"] / 60.0
        put(wu, i, 1, round(start, 2), fmt="0.00")
        put(wu, i, 2, round(gp["at_minutes"] / 60.0, 2), fmt="0.00")
        put(wu, i, 3, round(gp["gap_minutes"], 1), fmt="0.0")
        put(wu, i, 4, round(gp["ratio"], 2), fmt="0.00")
        put(wu, i, 5, gp["ends_with"], text=True)
        put(wu, i, 6, gp["system"], text=True)
        put(wu, i, 7, math.floor(round(gp["gap_minutes"], 1) / P["session_minutes"]))
        put(wu, i, 8, gp.get("robust"), fill=GRN if gp.get("robust") == "robust" else None)
        put(wu, i, 9, gp.get("cause"), B)
        put(wu, i, 10, gp.get("why"), text=True)
        put(wu, i, 11, gp.get("options_within_5_min"), fmt="0.0")
        put(wu, i, 12, gp.get("capped_share"), fmt="0%")
        put(wu, i, 13, gp.get("income_growth_log10"), fmt="0.00")
        put(wu, i, 14, gp.get("wall_ratio_at_start"), fmt="0.00")
        put(wu, i, 15, gp.get("prestiges_in_gap"))
        put(wu, i, 16, gp.get("purchases_in_gap"))
        put(wu, i, 17, gp.get("minutes_since_system_unlock"), fmt="0")
        put(wu, i, 18, gp.get("zone"))
        # the nearest note that is close enough, and each note one time only
        near = [(abs((x.get("lull_at_hours") or 0) - start), k) for k, x in enumerate(notes)
                if not x.get("_used") and abs((x.get("lull_at_hours") or 0) - start) <= max(0.25, 0.1 * start)]
        nt = notes[min(near)[1]] if near else {}
        if nt:
            nt["_used"] = True
        put(wu, i, 19, nt.get("diagnosis"), INP, YEL, text=True)
        put(wu, i, 20, nt.get("fix"), INP, YEL, text=True)
    r = len(A["lulls"]) + 3
    note(wu, r, "These rows use the thresholds of the script run and the primary buyer profile "
                f"('{primary}'). The Beats and Timeline sheets mark lulls live. 'Probable cause' is a rule on "
                "simulation data: check it against the Timeline. 'depends on reset policy' means that one or more "
                "of the other buyer profiles has no lull at this time (see Profiles). All cells are values of the "
                "script run: after a Params edit, the live lull marks are on the Beats sheet.", 20, 45)
    r += 2
    for k, n in enumerate(["Dead zone", "Starts at (h)", "Ends at (h)", "Minutes", "Lulls in it", "Beats inside",
                           "Share of the run"], 1):
        put(wu, r, k, n, H, HF)
    for z, zn in enumerate(A["dead_zones"], 1):
        r += 1
        put(wu, r, 1, z)
        put(wu, r, 2, round(zn["start_minutes"] / 60.0, 2), fmt="0.00")
        put(wu, r, 3, round(zn["end_minutes"] / 60.0, 2), fmt="0.00")
        put(wu, r, 4, round(zn["minutes"], 1), fmt="0")
        put(wu, r, 5, zn["lulls"])
        put(wu, r, 6, zn["beats_inside"])
        put(wu, r, 7, round(zn["share_of_run"], 4), fmt="0%")
    note(wu, r + 2, "A dead zone is a group of lulls with less than the max gap between them. It is the stretch "
                    "that a player feels as 'the game went quiet', also when one or two beats fall inside it.", 20)

    # ---- Swimlane -------------------------------------------------------------
    wsw = wb.create_sheet("Swimlane")
    first_ev = {}
    for e in ev:
        if e["system"] != "-" and e["system"] not in first_ev:
            first_ev[e["system"]] = e["t"]
    loops = {s["id"]: s["loop"] for s in m["systems"]}
    lanes = sorted(first_ev, key=first_ev.get) + [s["id"] for s in m["systems"] if s["id"] not in first_ev]
    log_edges = [0.0, 5.0]
    while log_edges[-1] < end and len(log_edges) < 40:
        log_edges.append(log_edges[-1] * 2)
    log_edges = log_edges[:-1] if len(log_edges) > 2 and log_edges[-2] >= end else log_edges
    put(wsw, 1, 1, "Swimlane: which system gives beats, and when", T).border = Border()
    put(wsw, 2, 1, "Four blocks. Beats: events at the beat weight or more (before merging). Activity: all events on "
                   "the Timeline. First with equal buckets (Params: swimlane bucket minutes), then with buckets "
                   "that double (the last column counts all later events). A red cell in an ALL row is a bucket with no beat. A filled activity cell over an "
                   "empty beat cell means 'busy, but nothing new'. Times are minutes of play.")
    wsw.merge_cells("A2:P2")
    wsw.row_dimensions[2].height = 45
    wsw.column_dimensions["A"].width = 16
    wsw.column_dimensions["B"].width = 11
    wsw.column_dimensions["C"].width = 8
    swim_blocks = []
    times = {"ALL": ([], [])}              # lane -> (times of all rows, times of beat rows)
    for e in rows:
        for key in (e["system"], "ALL"):
            lane = times.setdefault(key, ([], []))
            lane[0].append(pm(e))
            if e["weight"] >= P["beat_weight"]:
                lane[1].append(pm(e))

    def count(sid, lo, hi, beats_only):
        ts = times.get(sid, ([], []))[1 if beats_only else 0]
        return bisect.bisect_left(ts, hi) - bisect.bisect_left(ts, lo)

    lin_edges = [k * swim_bucket for k in range(swim_n)]
    r = 4
    for live in (False, True):
        put(wsw, r, 1, "LIVE: the same four blocks as formulas. They follow the beat weight and the swimlane bucket "
                       "on Params." if live else
            "AT BUILD: values of the script run. A reader sees them with no recalculation.", B).border = Border()
        r += 2
        for title, beats_only, edges in (("Beats, equal buckets", True, None), ("Activity, equal buckets", False, None),
                                         ("Beats, buckets that double", True, log_edges[:-1]),
                                         ("Activity, buckets that double", False, log_edges[:-1])):
            linear = edges is None
            edges = lin_edges if linear else edges
            n = len(edges)
            put(wsw, r, 1, title + (" (live)" if live else " (at build)"), B).border = Border()
            e_row, h_row = r + 1, r + 2
            put(wsw, e_row, 1, "Start (min)", B)
            put(wsw, e_row, 2, None)
            put(wsw, e_row, 3, None)
            for k in range(n):
                col = 4 + k
                put(wsw, e_row, col, f"={k}*{PR['swim']}" if live and linear else edges[k], fmt="0")
                put(wsw, h_row, col, f"={L(col)}{e_row}/60" if live and linear else edges[k] / 60.0, H, HF,
                    fmt="0.0")
                wsw.column_dimensions[L(col)].width = 5.5
            put(wsw, e_row, 4 + n, LAST_EDGE, fmt="0E+00")       # the last bucket has no end
            for k, nme in enumerate(["System", "Loop", "Total"], 1):
                put(wsw, h_row, k, nme, H, HF)
            for j, sid in enumerate(lanes + ["ALL"]):
                rr = h_row + 1 + j
                put(wsw, rr, 1, sid, B, text=True)
                put(wsw, rr, 2, loops.get(sid, "") if sid != "ALL" else "")
                vals = [count(sid, lo, hi, beats_only) for lo, hi in zip(edges, edges[1:] + [LAST_EDGE])]
                put(wsw, rr, 3, f"=SUM({L(4)}{rr}:{L(3 + n)}{rr})" if live else sum(vals))
                for k in range(n):
                    col = 4 + k
                    if not live:
                        put(wsw, rr, col, vals[k], fmt="0;-0;;@")
                        continue
                    crit = f'{tC},">="&{L(col)}${e_row},{tC},"<"&{L(col + 1)}${e_row}'
                    if sid != "ALL":
                        crit = f"{tG},$A{rr}," + crit
                    if beats_only:
                        crit += f",{tM},1"
                    put(wsw, rr, col, f"=COUNTIFS({crit})", fmt="0;-0;;@")
            top, bot = h_row + 1, h_row + len(lanes) + 1
            if lanes:
                wsw.conditional_formatting.add(f"{L(4)}{top}:{L(3 + n)}{bot - 1}", ColorScaleRule(
                    start_type="num", start_value=0, start_color="FFFFFF", end_type="max",
                    end_color="63BE7B" if beats_only else "9BC2E6"))
            all_area = f"{L(4)}{bot}:{L(3 + n)}{bot}"
            if beats_only:
                wsw.conditional_formatting.add(all_area, CellIsRule(operator="equal", formula=["0"], fill=RED))
                wsw.conditional_formatting.add(all_area, CellIsRule(operator="greaterThan", formula=["0"], fill=GRN))
            swim_blocks.append(dict(title=title, live=live, edge_row=e_row, first=top, last=bot, n=n, beats=beats_only))
            r = bot + 3
    wsw.freeze_panes = "D4"

    # ---- Profiles -------------------------------------------------------------
    wpf = wb.create_sheet("Profiles")
    profs = R["profiles"]
    head(wpf, 1, ["Indicator"] + [p["name"] + (" (primary)" if p["name"] == primary else "") for p in profs],
         [40] + [22] * len(profs))
    prow = [("Reset rule of the buyer", "text", None), ("Lulls", "lulls", "0"),
            ("Longest gap between beats (min)", "longest_gap_minutes", "0.0"),
            ("Minutes in dead zones", "dead_zone_minutes", "0"), ("Beats (after merging)", "beats", "0"),
            ("Stacked beats", "stacked_beats", "0"), ("Beats in the first hour", "beats_first_hour", "0"),
            ("Minutes to first purchase", "first_purchase_minutes", "0.0"),
            ("Minutes to first new system", "first_system_unlock_minutes", "0.0"),
            ("Minutes to first prestige", "first_prestige_minutes", "0.0"), ("Prestiges", "prestiges", "0"),
            ("Prestiges after a run with nothing new", "prestiges_with_nothing_new", "0"),
            ("Median run length (min)", "median_run_minutes", "0.0"),
            ("Lifetime prestige currency", "lifetime_prestige_currency", "0"),
            ("Highest ladder step", "ladder_step", "0"), ("Share of buckets with no beat", "dry_bucket_share", "0%"),
            ("Median options within 5 minutes", "median_options_within_5_min", "0"),
            ("Share of time with no option within 5 minutes", "share_of_time_with_no_option", "0%"),
            ("Simulation stalled", "stalled", None)]
    for i, (label, key, fmt) in enumerate(prow, 2):
        put(wpf, i, 1, label, B)
        for k, p in enumerate(profs, 2):
            v = p["text"] if key == "text" else p["summary"].get(key)
            if key in ("first_purchase_minutes", "first_system_unlock_minutes", "first_prestige_minutes") and v is None:
                v = "never"
            if key == "stalled":
                v = "yes" if v else "no"
            put(wpf, i, k, v, fmt=fmt, text=True)
    wpf.row_dimensions[2].height = 55
    r = len(prow) + 3
    note(wpf, r, "Each column is one full simulation of the same model with a different reset rule. A lull that "
                 "all columns show is a property of the game. A lull in one column only is a property of the "
                 "player model. All values come from the simulation at the thresholds of the script run.",
         1 + len(profs), 60)
    r += 2
    for k, n in enumerate(["Profile", "Lull starts (h)", "Lull ends (h)", "Minutes"], 1):
        put(wpf, r, k, n, H, HF)
    for p in profs:
        for a, b in p["summary"]["lull_hours"]:
            r += 1
            put(wpf, r, 1, p["name"])
            put(wpf, r, 2, a, fmt="0.00")
            put(wpf, r, 3, b, fmt="0.00")
            put(wpf, r, 4, round((b - a) * 60), fmt="0")

    # ---- Target ---------------------------------------------------------------
    wtg = wb.create_sheet("Target")
    n_ideal = max(1, ideal_count(P["target_hours"] * 60, P["target_first_gap"], P["target_stretch"]))
    nG = min(500, max(n_ideal, len(mos)))
    n_act = min(len(mos), nG)
    head(wtg, 1, ["Beat #", "Ideal time (min, at build)", "Actual time (min)", "Error (min, at build)",
                  "Ideal (h, at build)", "Actual (h)", "Ideal time (min, live)", "Error (min, live)",
                  "Actual / ideal (live)"], [8, 12, 12, 11, 10, 10, 12, 11, 10])
    for i in range(2, nG + 2):
        ideal = ideal_time(i - 1, P["target_first_gap"], P["target_stretch"])
        put(wtg, i, 1, i - 1)
        put(wtg, i, 2, ideal, fmt="0.0")
        put(wtg, i, 5, round(ideal / 60.0, 5), fmt="0.00")
        put(wtg, i, 7, f"=IF({PR['stretch']}=1,{PR['g0']}*A{i},{PR['g0']}*({PR['stretch']}^A{i}-1)/"
                       f"({PR['stretch']}-1))", fmt="0.0")
        if i - 2 < n_act:
            at = mos[i - 2]["at"]
            put(wtg, i, 3, at, fmt="0.0")
            put(wtg, i, 4, at - ideal, fmt="0.0")
            put(wtg, i, 6, round(at / 60.0, 5), fmt="0.00")
            put(wtg, i, 8, f"=C{i}-G{i}", fmt="0.0")
            put(wtg, i, 9, f'=IF(G{i}>0,C{i}/G{i},"")', fmt="0.00")
    wtg.conditional_formatting.add(f"D2:D{nG + 1}", ColorScaleRule(
        start_type="num", start_value=0, start_color="FFFFFF", end_type="max", end_color="F8696B"))
    tc = ScatterChart()
    tc.style = 13
    tc.series.append(line(wtg, 1, 5, 2, nG + 1, title="Ideal (at build)"))
    if n_act:
        tc.series.append(line(wtg, 1, 6, 2, n_act + 1, title="Actual (simulation)"))
    chart_common(tc, "Beats against the target curve (actual to the right of ideal = late)", "Hours played",
                 "Beat number", 24, 11)
    wtg.add_chart(tc, "K2")
    note(wtg, nG + 3, "Ideal time of beat n = first gap x (stretch^n - 1) / (stretch - 1). The three inputs are on "
                      "Params. Actual times come from the simulation (the Beats sheet). The part where the error "
                      "grows fastest is where the game needs a new beat. 'At build' columns and the chart use the inputs "
                      "of the script run. 'Live' columns follow Params.", 9, 45)

    # ---- Walls ----------------------------------------------------------------
    wwl = wb.create_sheet("Walls")
    head(wwl, 1, ["Ladder", "Step", "Requirement (at build)", "Compares", "Reached at (h)",
                  "Wait since the step before (min)", "Value at that time", "Status", "log10 requirement",
                  "log10 value", "Base (what-if)", "Growth (what-if)", "Requirement (live what-if)"],
         [14, 7, 13, 16, 10, 12, 13, 22, 10, 10, 11, 9, 13])
    walls = R["walls"]
    spans = {}
    for i, w in enumerate(walls, 2):
        spans.setdefault(w["ladder"], [i, i])[1] = i
        try:
            req = min(w["base"] * w["growth"] ** (w["step"] - 1), 1e300)
        except OverflowError:
            req = 1e300
        val = fin(w["value"])
        put(wwl, i, 1, w["ladder"], text=True)
        put(wwl, i, 2, w["step"])
        put(wwl, i, 3, req, fmt="0.00E+00")
        put(wwl, i, 4, f"{w['type']} {w['currency'] or ''}".strip(), text=True)
        put(wwl, i, 5, None if w["reached_minutes"] is None else round(w["reached_minutes"] / 60.0, 3), fmt="0.00")
        put(wwl, i, 6, None if w["wait_minutes"] is None else round(w["wait_minutes"], 2), fmt="0.0")
        put(wwl, i, 7, val, fmt="0.00E+00")
        put(wwl, i, 8, "reached" if w["reached"] else "not reached: value at the end")
        put(wwl, i, 9, round(math.log10(max(req, 1e-300)), 4), fmt="0.00")
        put(wwl, i, 10, round(math.log10(val), 4) if val and val > 0 else None, fmt="0.00")
        put(wwl, i, 11, w["base"], INP, YEL)
        put(wwl, i, 12, w["growth"], INP, YEL)
        put(wwl, i, 13, f"=IFERROR(K{i}*L{i}^(B{i}-1),1E+300)", fmt="0.00E+00")
    nW = len(walls) + 1
    if walls:
        wwl.conditional_formatting.add(f"F2:F{nW}", ColorScaleRule(
            start_type="num", start_value=0, start_color="FFFFFF", end_type="max", end_color="F8696B"))
        wwl.conditional_formatting.add(f"A2:J{nW}", FormulaRule(formula=['LEFT($H2,3)="not"'], fill=GREY))
    for k, (lid, (r1, r2)) in enumerate(list(spans.items())[:3]):
        wc_ = ScatterChart()
        wc_.style = 13
        wc_.series.append(line(wwl, 9, 2, r1, r2, title="log10 requirement"))
        wc_.series.append(line(wwl, 10, 2, r1, r2, title="log10 value (reached steps: at that time; later steps: at the end)"))
        chart_common(wc_, f"Wall on '{lid}': requirement against the value that the buyer had", "Step",
                     "log10", 24, 10)
        wwl.add_chart(wc_, f"O{2 + 21 * k}")
    note(wwl, nW + 2, "One row for each ladder step. Time, wait, and value come from the simulation (the first time "
                      "that the step was reached). The yellow cells are a what-if for the last column only: the chart "
                      "and the simulation do not follow them. For a "
                      "step that was not reached, the value is the value at the end of the run: where the "
                      "requirement line leaves the value line is the wall." if walls else
         "The model has no ladder. A ladder is the spine of the game: see model-schema.md.", 13, 45)

    # ---- Purchases ------------------------------------------------------------
    wpu = wb.create_sheet("Purchases")
    head(wpu, 1, ["Line", "Id", "System", "Status", "First bought (min)", "Last bought (min)", "Highest level",
                  "Levels bought", "Max level at the end", "Spent", "Currency", "Share of spending in the currency",
                  "Payback of the first level (s)", "Payback of the last bought level (s)"],
         [22, 14, 12, 40, 10, 10, 8, 8, 9, 12, 10, 11, 12, 12])
    pur = R["purchases"]
    nP = max(len(pur) + 1, 2)
    spent_all = {}
    for x, p in zip(pur, m["purchases"]):
        c0 = p["costs"][0]["currency"]
        spent_all[c0] = spent_all.get(c0, 0.0) + x["spent"].get(c0, 0.0)
    for i, x in enumerate(pur, 2):
        c0 = m["purchases"][i - 2]["costs"][0]["currency"]
        st = x["status"]
        put(wpu, i, 1, x["name"], B, text=True)
        put(wpu, i, 2, x["id"], text=True)
        put(wpu, i, 3, x["system"], text=True)
        put(wpu, i, 4, st, fill=RED if st.startswith(("never", "affordable")) else (GREY if st.startswith("capped") else None))
        put(wpu, i, 5, x["first_bought_minutes"], fmt="0.0")
        put(wpu, i, 6, x["last_bought_minutes"], fmt="0.0")
        put(wpu, i, 7, x["highest_level"])
        put(wpu, i, 8, x["levels_bought"])
        put(wpu, i, 9, x["cap_at_end"])
        put(wpu, i, 10, x["spent"].get(c0, 0.0), fmt="0.00E+00")
        put(wpu, i, 11, c0, text=True)
        put(wpu, i, 12, x["spent"].get(c0, 0.0) / spent_all[c0] if spent_all.get(c0) else 0, fmt="0%")
        put(wpu, i, 13, x["payback_first_seconds"], fmt="0")
        put(wpu, i, 14, x["payback_last_seconds"], fmt="0")
    note(wpu, nP + 2, "All values come from the simulation, primary buyer profile. Payback = cost / the rise in "
                      "income of the cost currency. It is empty when the line does not raise the currency that "
                      "pays for it. 'Spent' is the first cost currency of the line. Levels bought counts all runs.",
         14, 45)

    # ---- Prestige -------------------------------------------------------------
    wpr = wb.create_sheet("Prestige")
    runs = sim.runs[:3000]
    lad = g.get("progress_axis") or (m["ladders"][0]["id"] if m["ladders"] else None)
    head(wpr, 1, ["Run", "Start (h)", "Length (min)", "Gain", "Gain each minute", f"Highest step ({lad or 'no ladder'})",
                  "New beats in the run"], [7, 9, 10, 12, 11, 11, 9])
    for i, x in enumerate(runs, 2):
        put(wpr, i, 1, x["run"])
        put(wpr, i, 2, round(x["start_minutes"] / 60.0, 3), fmt="0.00")
        put(wpr, i, 3, round(x["minutes"], 2), fmt="0.0")
        put(wpr, i, 4, x["gain"], fmt="0.00E+00" if x["gain"] >= 1e6 else "0")
        put(wpr, i, 5, x["gain_per_minute"], fmt="0.00")
        put(wpr, i, 6, x["top_steps"].get(lad) if lad else None)
        put(wpr, i, 7, x["new_beats"])
    nR = len(runs) + 1
    if runs:
        wpr.conditional_formatting.add(f"G2:G{nR}", CellIsRule(operator="equal", formula=["0"], fill=RED))
        for k, (col, ttl, ylab) in enumerate(((3, "Run length (healthy: shorter at first, then steady or slowly longer)",
                                               "Minutes"), (6, "Highest ladder step in each run (flat = nothing new)", "Step"))):
            if col == 6 and not lad:
                continue
            ch = ScatterChart()
            ch.style = 13
            ch.series.append(line(wpr, col, 1, 2, nR, title=ylab))
            chart_common(ch, ttl, "Run number", ylab, 20, 8)
            ch.legend = None
            wpr.add_chart(ch, f"I{2 + 18 * k}")
    note(wpr, nR + 2, ("One row for each finished run, from the simulation, primary buyer profile "
                       f"('{primary}'). A red cell: the run reached no new beat, so its prestige has weight 1 and "
                       "is not a beat. The run lengths depend on the reset rule: compare the Profiles sheet.")
         if runs else "No prestige happened in the simulation.", 7, 45)

    # ---- Ledger ---------------------------------------------------------------
    wle = wb.create_sheet("Ledger")
    head(wle, 1, ["Currency", "Direction", "Where", "Amount", "Share of the direction"], [14, 11, 34, 14, 12])
    led = R["ledger"]
    nLe = max(len(led) + 1, 2)
    led_tot = {}
    for c, d, label, amt in led:
        led_tot[(c, d)] = led_tot.get((c, d), 0.0) + amt
    for i, (c, d, label, amt) in enumerate(led, 2):
        put(wle, i, 1, c, B, text=True)
        put(wle, i, 2, d)
        put(wle, i, 3, label, text=True)
        put(wle, i, 4, amt, fmt="0.00E+00")
        put(wle, i, 5, amt / led_tot[(c, d)] if led_tot[(c, d)] > 0 else 0, fmt="0%")
    note(wle, nLe + 2, "Sources and sinks of each currency over the whole simulation, by system. Income is split by "
                       "the system that owns the 'add' effects; multipliers are not split. A source with no sink "
                       "of the same size piles up. All values come from the simulation.", 5, 45)

    # ---- Sensitivity ----------------------------------------------------------
    wse = wb.create_sheet("Sensitivity")
    sens = R.get("sens")
    if sens is None:
        put(wse, 1, 1, "The script ran with --no-sensitivity. Run it without that option to fill this sheet.", B)
        wse.column_dimensions["A"].width = 90
    else:
        srows, tested, possible = sens
        heads = ["Parameter", "Base value", "Low value (x0.95)", "High value (x1.05)"]
        for side_ in ("Low", "High"):
            heads += [f"{side_}: lulls", f"{side_}: longest gap (min)", f"{side_}: first prestige (min)",
                      f"{side_}: ladder step", f"{side_}: beats"]
        heads += ["Change in longest gap, low (min)", "Change in longest gap, high (min)"]
        head(wse, 1, heads, [36, 10, 10, 10] + [9, 11, 11, 9, 8] * 2 + [13, 13])
        base = R["sens_base"]
        put(wse, 2, 1, "(base run)", B)
        for k in (2, 3, 4):
            put(wse, 2, k, None)
        for off in (5, 10):
            for k, key in enumerate(("lulls", "longest_gap_minutes", "first_prestige_minutes", "ladder_step", "beats")):
                put(wse, 2, off + k, base[key], fmt="0.0" if "minutes" in key else "0")
        put(wse, 2, 15, 0)
        put(wse, 2, 16, 0)
        for i, x in enumerate(srows, 3):
            put(wse, i, 1, x["parameter"], B, text=True)
            put(wse, i, 2, x["base"], fmt="0.000")
            put(wse, i, 3, x["low_value"], fmt="0.000")
            put(wse, i, 4, x["high_value"], fmt="0.000")
            for off, sd in ((5, x["low"]), (10, x["high"])):
                for k, key in enumerate(("lulls", "longest_gap_minutes", "first_prestige_minutes", "ladder_step", "beats")):
                    put(wse, i, off + k, sd[key], fmt="0.0" if "minutes" in key else "0")
            put(wse, i, 15, x["change_low"], fmt="0.0")
            put(wse, i, 16, x["change_high"], fmt="0.0")
        nSe = len(srows) + 2
        if srows:
            for col in ("O", "P"):
                wse.conditional_formatting.add(f"{col}3:{col}{nSe}", CellIsRule(operator="lessThan", formula=["0"], fill=GRN))
                wse.conditional_formatting.add(f"{col}3:{col}{nSe}", CellIsRule(operator="greaterThan", formula=["0"], fill=RED))
            sb = BarChart()
            sb.type = "bar"
            top_n = min(len(srows), 15) + 2
            sb.add_data(Reference(wse, min_col=15, max_col=16, min_row=1, max_row=top_n), titles_from_data=True)
            sb.set_categories(Reference(wse, min_col=1, min_row=2, max_row=top_n))
            chart_common(sb, "Change in the longest gap when one number changes by 5%", "Parameter", "Minutes", 24, 12)
            wse.add_chart(sb, f"A{nSe + 5}")
        note(wse, nSe + 2, f"Each row is two more runs of the primary buyer profile ('{primary}') with one number "
                           f"changed. Sorted by the change in the longest gap. {tested} of {possible} parameters "
                           "were tested" + (" (time limit: ladders and the prestige exponent first, then the lines "
                                            "that the buyer bought most)." if tested < possible else ".")
             + " The Low and High columns show the values that were used: for an exp cost the change is held "
               "to half of (growth - 1)." + " A small change can move a beat across a threshold: read large values as 'this number matters', "
               "not as a forecast.", 16, 45)

    # ---- Compare --------------------------------------------------------------
    wcp = wb.create_sheet("Compare")
    other = R.get("compare")
    if other is None:
        put(wcp, 1, 1, "No second run. To compare two models: run the script for the first model, then run it for "
                       "the second model with --compare <first>.analysis.json.", B)
        wcp.column_dimensions["A"].width = 110
    else:
        head(wcp, 1, ["Indicator", "Before", "After", "Delta (at build)", "Delta (live)"], [44, 14, 14, 12, 12])
        keys = [("Simulated hours", "simulated_hours", "0.0"), ("Beats (after merging)", "beats", "0"),
                ("Stacked beats", "stacked_beats", "0"), ("Beats in the first hour", "beats_first_hour", "0"),
                ("Lulls", "_lulls", "0"), ("Longest gap between beats (min)", "longest_gap_minutes", "0.0"),
                ("Minutes in dead zones", "_zone_minutes", "0"), ("Median gap ratio", "median_gap_ratio", "0.00"),
                ("Minutes to first purchase", "first_purchase_minutes", "0.0"),
                ("Minutes to first new system", "first_system_unlock_minutes", "0.0"),
                ("Minutes to first prestige", "first_prestige_minutes", "0.0"),
                ("Prestiges", "prestige_count", "0"), ("Share of buckets with no beat", "dry_session_share", "0%"),
                ("Median options within 5 minutes", "median_options_within_5_min", "0"),
                ("Share of time with no option within 5 minutes", "share_of_time_with_no_option_within_5_min", "0%"),
                ("Purchases never bought", "_never_bought", "0"), ("Milestones never reached", "_never_reached", "0"),
                ("Systems", "systems", "0"), ("Links between systems", "links", "0")]

        def val(d, key):
            if key == "_lulls":
                return len(d.get("lulls") or [])
            if key == "_zone_minutes":
                return round(sum(z.get("minutes", 0) for z in d.get("dead_zones") or []), 1)
            if key in ("_never_bought", "_never_reached"):
                return len(d.get(key[1:]) or [])
            v = d.get(key)
            return "never" if v is None and key.startswith("first_") else v

        for i, (label, key, fmt) in enumerate(keys, 2):
            put(wcp, i, 1, label, B)
            put(wcp, i, 2, val(other, key), fmt=fmt)
            put(wcp, i, 3, val(A, key), fmt=fmt)
            vb, va = val(other, key), val(A, key)
            ok = all(isinstance(v, (int, float)) and not isinstance(v, bool) for v in (vb, va))
            put(wcp, i, 4, va - vb if ok else None, fmt=fmt)
            put(wcp, i, 5, f'=IF(AND(ISNUMBER(B{i}),ISNUMBER(C{i})),C{i}-B{i},"")', fmt=fmt)
        r = len(keys) + 3
        note(wcp, r, f"Before: {other.get('game', '?')} ({os.path.basename(R['compare_path'])}). After: {g['name']} "
                     "(this workbook). Both columns come from simulations. Check that the two runs used the same "
                     "thresholds and the same buyer profile.", 4, 45)
        put(wcp, 1, 7, "Before: hours", H, HF)
        put(wcp, 1, 8, "Before: beats", H, HF)
        put(wcp, 1, 9, "After: hours", H, HF)
        put(wcp, 1, 10, "After: beats", H, HF)
        for col in "GHIJ":
            wcp.column_dimensions[col].width = 11
        bt = [x for x in (other.get("beat_times_minutes") or []) if isinstance(x, (int, float))]
        for i, x in enumerate(bt, 2):
            put(wcp, i, 7, round(x / 60.0, 4), fmt="0.00")
            put(wcp, i, 8, i - 1)
        for i, mo in enumerate(mos, 2):
            put(wcp, i, 9, round(mo["at"] / 60.0, 4), fmt="0.00")
            put(wcp, i, 10, i - 1)
        cc = ScatterChart()
        cc.style = 13
        if bt:
            cc.series.append(line(wcp, 8, 7, 2, len(bt) + 1, title="Before"))
        if mos:
            cc.series.append(line(wcp, 10, 9, 2, len(mos) + 1, title="After"))
        chart_common(cc, "Cumulative beats: before and after (flat parts are lulls)", "Hours played", "Beats", 24, 11)
        wcp.add_chart(cc, "L2")

    # ---- Income ---------------------------------------------------------------
    sm = sim.samples
    if len(sm) > 500:
        sm = sm[::math.ceil(len(sm) / 500)]
    wi = wb.create_sheet("Income")
    head(wi, 1, ["Played hours"] + [f"log10 income/s {c}" for c in curs] + ["Options affordable now", "Options within 5 min"],
         [8] + [13] * len(curs) + [12, 12])
    for i, s in enumerate(sm, 2):
        put(wi, i, 1, round(s["t"] / 3600.0, 4), fmt="0.00")
        for k, c in enumerate(curs, 2):
            put(wi, i, k, round(math.log10(s["income"][c] + 1), 4), fmt="0.00")
        put(wi, i, len(curs) + 2, s["now"])
        put(wi, i, len(curs) + 3, s["soon"])
    nI = max(len(sm) + 1, 2)
    sc = ScatterChart()
    sc.style = 13
    for k in range(2, len(curs) + 2):
        sc.series.append(line(wi, k, 1, 2, nI, head_row=1))
    chart_common(sc, "Income growth (flat parts are stalls; steps are purchases and resets)", "Hours played",
                 "log10 of income per second")
    wi.add_chart(sc, f"{L(len(curs) + 5)}2")
    sc2 = ScatterChart()
    sc2.style = 13
    for k in (len(curs) + 2, len(curs) + 3):
        sc2.series.append(line(wi, k, 1, 2, nI, head_row=1))
    chart_common(sc2, "Decision density (0 = nothing to do; more than 7 = too many options)", "Hours played", "Options")
    wi.add_chart(sc2, f"{L(len(curs) + 5)}22")

    # ---- Curves ---------------------------------------------------------------
    wc = wb.create_sheet("Curves")
    lv = [0, 1, 5, 10, 25, 50, 100]
    c_lv = 11
    c0 = c_lv + len(lv)
    c_live = c0 + 5

    def cell_cost(p, n):
        v = cost_one(p["costs"][0], n)
        return None if v == INF else v

    head(wc, 1, ["Purchase", "System", "Cost currency", "Formula", "Base cost (what-if)", "Growth (what-if)",
                 "Max level (model)", "Max level at the end", "Other costs", "Effects"]
         + [f"Cost at L{x} (at build)" for x in lv]
         + ["Levels to 10x cost (at build)", "First bought (min)", "Highest level reached",
            "Payback of the first level (s)", "Payback of the last bought level (s)"]
         + [f"Cost at L{x} (live what-if)" for x in lv] + ["Levels to 10x cost (live what-if)"],
         [22, 12, 10, 8, 10, 8, 8, 8, 22, 34] + [11] * len(lv) + [10, 10, 10, 12, 12] + [11] * len(lv) + [10])
    for i, p in enumerate(m["purchases"], 2):
        x = pur[i - 2]
        put(wc, i, 1, p["name"], B, text=True)
        put(wc, i, 2, p["system"], text=True)
        put(wc, i, 3, p["cost_currency"], text=True)
        put(wc, i, 4, p["cost_formula"])
        put(wc, i, 5, p["base_cost"], INP, YEL)
        put(wc, i, 6, p["cost_growth"], INP, YEL)
        put(wc, i, 7, p["max_level"])
        put(wc, i, 8, x["cap_at_end"])
        put(wc, i, 9, "; ".join(f"{c['currency']}: {c['base']:g}, {c['formula']} {c['growth']:g}"
                                for c in p["costs"][1:]), text=True)
        put(wc, i, 10, "; ".join(f"{e['kind']} {e['value']:g} -> {e['target']}" for e in p["effects"])
            or "(no effect)", text=True)
        for k, n in enumerate(lv):
            put(wc, i, c_lv + k, cell_cost(p, n), fmt="0.00E+00")
            put(wc, i, c_live + k, f'=IF($D{i}="linear",$E{i}+$F{i}*{n},IF($D{i}="poly",$E{i}*({n}+1)^$F{i},'
                                   f'$E{i}*$F{i}^{n}))', fmt="0.00E+00")
        put(wc, i, c0, math.log(10) / math.log(p["cost_growth"])
            if p["cost_formula"] == "exp" and p["cost_growth"] > 1 else None, fmt="0.0")
        put(wc, i, c_live + len(lv), f'=IF(AND($D{i}="exp",$F{i}>1),LN(10)/LN($F{i}),"")', fmt="0.0")
        put(wc, i, c0 + 1, x["first_bought_minutes"], fmt="0.0")
        put(wc, i, c0 + 2, x["highest_level"])
        put(wc, i, c0 + 3, x["payback_first_seconds"], fmt="0")
        put(wc, i, c0 + 4, x["payback_last_seconds"], fmt="0")
    nC = len(m["purchases"]) + 1
    note_r = nC + 2
    note(wc, note_r, "Yellow cells are a what-if for this sheet only: they move the 'live what-if' columns at the "
                     "right. They do not change the 'at build' columns, the chart, or the simulation. To see the result on pacing, edit model.json and run the script "
                     "again. The last four columns come from the simulation. Payback = cost / the rise in income of "
                     "the cost currency: when the payback of the last level is far above the first, the line has "
                     "slowed down and needs other systems to multiply it.", 12, 60)
    top = note_r + 2
    shown = m["purchases"][:10]
    put(wc, top, 1, "Level", H, HF)
    for k, p in enumerate(shown, 2):
        put(wc, top, k, p["name"], H, HF, text=True)
    for n in range(0, 51):
        r = top + 1 + n
        put(wc, r, 1, n)
        for k, p in enumerate(shown, 2):
            src = 2 + m["purchases"].index(p)
            cap = pur[src - 2]["cap_at_end"]
            if cap is not None and n > cap:
                continue                   # the chart stops at the max level of the line
            v = cell_cost(p, n)
            if v is not None:
                put(wc, r, k, round(math.log10(max(v, 1e-9)), 4), fmt="0.00")
    if shown:
        cc = ScatterChart()
        cc.style = 13
        for k in range(2, len(shown) + 2):
            cc.series.append(line(wc, k, 1, top + 1, top + 51, head_row=top))
        chart_common(cc, "Cost curves: first 10 lines, each to its max level (currencies differ: compare shapes)",
                     "Level", "log10 of cost", 24, 11)
        wc.add_chart(cc, f"{L(len(shown) + 4)}{top}")

    # ---- Systems + Matrix + Links ---------------------------------------------
    links = derive_links(m)
    wl = wb.create_sheet("Links")
    head(wl, 1, ["From system", "To system", "Type", "Detail"], [18, 18, 10, 70])
    for i, (a, b, t, n) in enumerate(links, 2):
        for k, v in enumerate([a, b, t, n], 1):
            put(wl, i, k, v, text=True)
    nL = max(len(links) + 1, 2)
    wsy = wb.create_sheet("Systems")
    head(wsy, 1, ["System", "Name", "Loop", "Persists through prestige", "Clock", "First event (played min, at build)",
                  "Events on the Timeline (at build)", "Links out (at build)", "Links in (at build)",
                  "Role (at build)", "Description", "First event (played min, live)", "Events on the Timeline (live)", "Links out (live)",
                  "Links in (live)", "Role (live)"],
         [14, 20, 11, 10, 8, 10, 9, 8, 8, 16, 60, 10, 8, 8, 8, 16])

    def role(o, i_):
        return "ORPHAN" if o + i_ == 0 else "SINK ONLY" if o == 0 else "SOURCE ONLY" if i_ == 0 else "CONNECTED"

    for i, s in enumerate(m["systems"], 2):
        ts = times.get(s["id"], ([], []))[0]
        n_out = sum(1 for x in links if x[0] == s["id"])
        n_in = sum(1 for x in links if x[1] == s["id"])
        put(wsy, i, 1, s["id"], B, text=True)
        put(wsy, i, 2, s["name"], text=True)
        put(wsy, i, 3, s["loop"])
        put(wsy, i, 4, "yes" if s["persists"] else "no")
        put(wsy, i, 5, s["clock"])
        put(wsy, i, 6, ts[0] if ts else "never", fmt="0.0")
        put(wsy, i, 7, len(ts))
        put(wsy, i, 8, n_out)
        put(wsy, i, 9, n_in)
        put(wsy, i, 10, role(n_out, n_in), fill=RED if n_out + n_in == 0 else None)
        put(wsy, i, 11, s.get("description"), text=True)
        put(wsy, i, 12, f'=IFERROR(INDEX({tC},MATCH($A{i},{tG},0)),"never")', fmt="0.0")
        put(wsy, i, 13, f"=COUNTIF({tG},$A{i})")
        put(wsy, i, 14, f"=COUNTIF(Links!$A$2:$A${nL},$A{i})")
        put(wsy, i, 15, f"=COUNTIF(Links!$B$2:$B${nL},$A{i})")
        put(wsy, i, 16, f'=IF(N{i}+O{i}=0,"ORPHAN",IF(N{i}=0,"SINK ONLY",IF(O{i}=0,"SOURCE ONLY","CONNECTED")))')
    nY = max(len(m["systems"]) + 1, 2)
    wsy.conditional_formatting.add(f"P2:P{nY}", CellIsRule(operator="equal", formula=['"ORPHAN"'], fill=RED))
    note(wsy, nY + 2, "'At build' columns are values from the script run. 'Live' columns are formulas on the "
                      "Timeline and Links sheets: they differ only if you edit those sheets.", 16)
    wx = wb.create_sheet("Matrix")
    put(wx, 1, 1, "From \\ To", H, HF)
    wx.column_dimensions["A"].width = 18
    for k, s in enumerate(m["systems"], 2):
        put(wx, 1, k, s["id"], H, HF, text=True).alignment = Alignment(text_rotation=90, vertical="bottom")
        put(wx, k, 1, s["id"], B, text=True)
        wx.column_dimensions[L(k)].width = 5
    nM = len(m["systems"]) + 1
    sys_ids = [s["id"] for s in m["systems"]]
    for r in range(2, nM + 1):
        for c in range(2, nM + 1):
            put(wx, r, c, sum(1 for x in links if x[0] == sys_ids[r - 2] and x[1] == sys_ids[c - 2]), fmt="0;-0;;@")
    wx.row_dimensions[1].height = 80
    if nM >= 2:
        wx.conditional_formatting.add(f"B2:{L(nM)}{nM}", ColorScaleRule(
            start_type="num", start_value=0, start_color="FFFFFF", end_type="max", end_color="F8696B"))
    note(wx, nM + 2, "Count of links from the row system to the column system. Values of the script run, from the "
                     "Links sheet.", max(nM, 6))

    # ---- Ideas ----------------------------------------------------------------
    wd = wb.create_sheet("Ideas")
    cols = ["Idea", "Archetype", "Problem it solves", "Feeds (core-loop value)", "Fed by", "Unlock point",
            "Persists through prestige", "Scope (S/M/L)", "Juice moment", "Risk"]
    keys = ["idea", "archetype", "problem", "feeds", "fed_by", "unlock", "persists", "scope", "juice", "risk"]
    head(wd, 1, cols, [26, 18, 40, 30, 26, 22, 10, 9, 36, 36])
    ideas = [x for x in ((m.get("review") or {}).get("ideas") or []) if isinstance(x, dict)]
    for i, idea in enumerate(ideas, 2):
        for k, key in enumerate(keys, 1):
            v = idea.get(key)
            put(wd, i, k, v if isinstance(v, (str, int, float)) or v is None else str(v), text=True)
    if not ideas:
        put(wd, 2, 1, "Add ideas to model.review.ideas and run the script again, or type them here.")
        wd.merge_cells("A2:J2")

    # ---- Reference ------------------------------------------------------------
    wr = wb.create_sheet("Reference")
    try:
        ref = json.load(open(ref_path)) if ref_path and os.path.exists(ref_path) else None
    except ValueError:
        ref = None
    if ref:
        ax = g.get("progress_axis")
        ax_count = next((l["count"] for l in m["ladders"] if l["id"] == ax), None)
        x_name = (f"share of the ladder '{ax}' (step at the first event of the system / {ax_count} steps)" if ax
                  else "share of simulated play time (the model sets no game.progress_axis)")
        put(wr, 1, 1, ref["title"], T, text=True)
        put(wr, 2, 1, ref["note"], text=True)
        wr.merge_cells("A2:I2")
        wr.row_dimensions[2].height = 45
        put(wr, 3, 1, f"X axis of this game: {x_name}. X axis of the reference game: share of its main level axis. "
                      "Y axis of both: share of all systems that are open. "
                      + ("Both x axes are a progress axis, so the two curves can be compared in shape."
                         if ax else "A level axis is close to a log-time axis: against it, a time axis makes a "
                                    "healthy game look front-loaded. Set game.progress_axis to compare."), B)
        wr.merge_cells("A3:I3")
        wr.row_dimensions[3].height = 45
        head(wr, 5, ["Reference: main level", "Share of main axis", "System that opens", "Share of systems open", "",
                     "This game: system", "First event (played h)", "Share of progress (x)", "Share of systems open"],
             [12, 10, 26, 10, 3, 20, 10, 12, 10], freeze=False)
        nref = len(ref["system_unlocks"])
        for i, u in enumerate(ref["system_unlocks"], 6):
            put(wr, i, 1, u["level"])
            put(wr, i, 2, round(u["level"] / ref["axis_max"], 4), fmt="0%")
            put(wr, i, 3, u["system"], text=True)
            put(wr, i, 4, round((i - 5) / nref, 4), fmt="0%")
        opened = {}
        for e in sim.events:
            if e["system"] not in opened and e["system"] != "-":
                opened[e["system"]] = (e["t"] / 3600.0, (e["axis"] or 0) / ax_count if ax else None)
        nsys = max(len(m["systems"]), 1)
        for i, (s, (h, share)) in enumerate(sorted(opened.items(), key=lambda kv: kv[1][0]), 6):
            put(wr, i, 6, s, text=True)
            put(wr, i, 7, round(h, 3), fmt="0.00")
            put(wr, i, 8, round(share if ax else h * 60 / max(end, 1e-9), 4), fmt="0%")
            put(wr, i, 9, round((i - 5) / nsys, 4), fmt="0%")
        rc = ScatterChart()
        rc.style = 13
        n1, n2 = nref + 5, len(opened) + 5
        rc.series.append(Series(Reference(wr, min_col=4, min_row=6, max_row=n1),
                                Reference(wr, min_col=2, min_row=6, max_row=n1), title="Reference game"))
        if opened:
            rc.series.append(Series(Reference(wr, min_col=9, min_row=6, max_row=n2),
                                    Reference(wr, min_col=8, min_row=6, max_row=n2), title="This game"))
        chart_common(rc, "Share of systems open over the course of the game", "Share of progress",
                     "Share of systems open", 24, 11)
        wr.add_chart(rc, "K5")
    else:
        put(wr, 1, 1, "The reference curve file was not found (assets/reference_unlock_curve.json).", B)

    # ---- Dashboard ------------------------------------------------------------
    wdash = wb.create_sheet("Dashboard", 0)
    wdash.column_dimensions["A"].width = 44
    for c, w in zip("BCDEFGH", (13, 13, 9, 9, 10, 10, 90)):
        wdash.column_dimensions[c].width = w
    put(wdash, 1, 1, f"{g['name']}: progression pacing", T, text=True).border = Border()
    away = (f" Sessions of {g['session_minutes']:g} min with {g['away_minutes']:g} min away between them "
            f"({A['real_hours']:.1f} h of real time)." if g["away_minutes"] else "")
    put(wdash, 2, 1, f"Simulated with a greedy buyer for {A['simulated_hours']:.1f} hours of play, buyer profile "
                     f"'{primary}' ({len(profs)} profile{'s' if len(profs) > 1 else ''} on the Profiles sheet). "
                     f"{A['events']} events.{away} 'At build' = the value of the script run: it needs no "
                     "recalculation. 'Live' = a formula that follows the yellow cells on Params; the two can differ "
                     "after an edit. Bands are starting values: change the blue cells for your game.", text=True)
    wdash.merge_cells("A2:H2")
    wdash.row_dimensions[2].height = 45
    for i, n in enumerate(["Indicator", "At build", "Live", "Low", "High", "Status (at build)", "Status (live)",
                           "How to read it"], 1):
        put(wdash, 4, i, n, H, HF)
    b = m.get("bands") or {}

    def band(key, lo, hi):
        v = b.get(key, [lo, hi])
        return (v[0], v[1]) if isinstance(v, list) and len(v) == 2 else (lo, hi)

    never = 'IFERROR(INDEX({0},MATCH("{1}",{2},0)),"never")'
    rob = sum(1 for x in A["lulls"] if x.get("robust") == "robust")
    SIM = "From the simulation. "

    def nv(x):
        return "never" if x is None else x

    hours = max(A["simulated_hours"], 1e-9)
    gap_high = max(P["max_gap_sessions"] * P["session_minutes"], P["late_ratio"] * A["last_beat_minutes"])
    n_orphan = len(A["orphan_systems"])
    # (name, value at build, live formula or None, low, high, format, how to read it)
    # the live cells of 'Last beat' (C13) and 'Tail gap' (C14) are used by the two rows after them: keep the order
    ind = [
        ("Simulated hours of play", A["simulated_hours"], f"={PR['end']}/60", None, None, "0.0", "The span that the other values cover."),
        ("Minutes to first purchase", nv(A["first_purchase_minutes"]), "=" + never.format(tC, "purchase", tF), *band("first_purchase", 0, 1), "0.0",
         "The first decision. Slow starts lose players before the loop shows itself."),
        ("Minutes to first new system", nv(A["first_system_unlock_minutes"]), "=" + never.format(tC, "system_unlock", tK), *band("first_system", 2, 15), "0.0",
         "The first proof that the game is larger than its first screen."),
        ("Minutes to first prestige", nv(A["first_prestige_minutes"]), "=" + never.format(tC, "prestige", tF), *band("first_prestige", 20, 120), "0.0",
         "The meta loop starts here. It depends on the buyer profile: see Profiles. 'never' is correct only for a game with no reset."),
        ("Beats (moments at the beat weight or more)", A["beats"], f"=SUM({tN})", None, None, "0",
         "Count of moments that a player will remember. Stacked beats count one time."),
        ("Stacked beats", A["stacked_beats"], f"=SUM({tM})-SUM({tN})", None, None, "0",
         "Beats that land within the merge seconds of another beat. They are lost as separate moments: spread them out."),
        ("Beats for each hour", A["beats"] / hours, f"=IFERROR(SUM({tN})/({PR['end']}/60),0)", *band("beats_per_hour", 3, 30), "0.0",
         "Low: long empty stretches. High: unlocks arrive faster than a player can absorb them."),
        ("Beats in the first hour", A["beats_first_hour"], f'=COUNTIFS({tN},1,{tC},"<60")', *band("beats_first_hour", 3, 30), "0",
         "The mean over the run hides a crowded start. Compare this value with the beats for each hour."),
        ("Last beat at (played min)", A["last_beat_minutes"], f"=SUMPRODUCT(MAX({tC}*{tN}))", None, None, "0.0", "The start of the last moment."),
        ("Tail gap (min)", A["tail_gap_minutes"], f"={PR['end']}-C13", None, None, "0.0",
         "From the last beat to the end of the simulation. It counts as a gap."),
        ("Lulls", len(A["lulls"]), f'=COUNTIF({tl("lull")},"LULL")+IF({lull_formula("C14", "C13")}="LULL",1,0)', *band("lulls", 0, 0), "0",
         "Gaps that fail the Params thresholds, with the tail gap. Each one is on the Beats sheet in red."),
        ("Longest gap between beats (min)", A["longest_gap_minutes"], f"=MAX(MAX({tl('gap')}),C14)", 0, round(gap_high, 2), "0.0",
         "The worst wait, with the tail gap. The high band is the lull limit at the last beat (max gap in sessions, or late lull ratio x time played)."),
        ("Median gap ratio", A["median_gap_ratio"], f"=IFERROR(MEDIAN({tl('ratio')}),0)", *band("gap_ratio", 0.05, 0.6), "0.00",
         "Gap / time played before it, each moment. A steady value means that waits grow in step with the game. A rising value means that the game runs out of new things."),
        ("Share of buckets with no beat", A["dry_session_share"], f'=COUNTIF(Sessions!$M$2:$M${nS + 1},"DRY")/{nS}', *band("dry_share", 0, 0.25), "0%",
         "A session with no beat gives no reason to come back. Buckets are on the Sessions sheet."),
    ]
    for h in TARGET_HOURS:
        v = A["beats_behind_target"][str(h)]
        ind.append((f"Beats behind target at hour {h}", "n/a" if v is None else v,
                    f'=IFERROR(IF({PR["end"]}<{60 * h},"n/a",ROUNDDOWN(LN(1+{60 * h}*({PR["stretch"]}-1)/{PR["g0"]})/'
                    f'LN({PR["stretch"]}),0)-COUNTIFS({tC},"<="&{60 * h},{tN},1)),"n/a")', None, None, "0",
                    "Ideal beats by this hour (Target sheet inputs on Params) minus actual beats. Above 0 = late. 'n/a' = the run is shorter."))
    ind += [
        ("Lulls that all buyer profiles show", rob, None, None, None, "0",
         SIM + (f"Of {len(A['lulls'])} lulls in this run. The other lulls depend on when the buyer resets: see Lulls and Profiles."
                if len(profs) > 1 else "One buyer profile only.")),
        ("Minutes in dead zones", round(sum(z["minutes"] for z in A["dead_zones"]), 1), None, None, None, "0",
         SIM + (f"{len(A['dead_zones'])} dead zone(s): " + "; ".join(
             f"hour {z['start_minutes'] / 60:.1f} to {z['end_minutes'] / 60:.1f}" for z in A["dead_zones"][:6])
                if A["dead_zones"] else "No dead zone.")),
        ("Median options within 5 minutes", A["median_options_within_5_min"], None, *band("options", 2, 7), "0",
         SIM + "Weighted by time. Below 2: no real choice. Above 7: choice overload."),
        ("Share of time with no option within 5 minutes", A["share_of_time_with_no_option_within_5_min"], None,
         *band("no_option", 0, 0.2), "0%", SIM + "Weighted by time. Pure waiting with nothing to plan toward."),
        ("Systems", A["systems"], f"=COUNTA(Systems!$A$2:$A${nY})", None, None, "0", "Scope. Each system needs content, UI, balance, and juice."),
        ("Links between systems", A["links"], f"=COUNTA(Links!$A$2:$A${nL})", None, None, "0", "Declared links plus links implied by costs, effects, and requirements."),
        ("Links for each system", A["links"] / max(A["systems"], 1), f"=IFERROR(COUNTA(Links!$A$2:$A${nL})/COUNTA(Systems!$A$2:$A${nY}),0)", *band("links_per_system", 1.5, 8), "0.0",
         "Low: systems sit side by side and do not feed each other. Very high: hard to balance and to explain."),
        ("Orphan systems", n_orphan, f'=COUNTIF(Systems!$P$2:$P${nY},"ORPHAN")', 0, 0, "0",
         (", ".join(A["orphan_systems"][:12]) + ". ") * bool(n_orphan) + "Systems with no link in or out. They do not affect the core loop."),
        ("Currencies with no source", len(A["currencies_without_source"]), None, 0, 0, "0", ", ".join(A["currencies_without_source"]) or "None."),
        ("Currencies with no sink", len(A["currencies_without_sink"]), None, 0, 0, "0", ", ".join(A["currencies_without_sink"]) or "None."),
        ("Purchases never bought in the simulation", len(A["never_bought"]), None, 0, 0, "0",
         (", ".join(A["never_bought"][:12]) + ". The Purchases sheet gives the reason.") if A["never_bought"] else "None."),
        ("Milestones never reached in the simulation", len(A["never_reached"]), None, 0, 0, "0", ", ".join(A["never_reached"][:12]) or "None."),
        ("Prestiges in the simulation", A["prestige_count"], None, None, None, "0",
         "Run lengths in minutes: " + (", ".join(f"{x:.0f}" for x in A["prestige_run_minutes"][:15]) or "none")
         + ". See the Prestige sheet."),
        ("Prestiges after a run with nothing new", sum(1 for x in sim.runs[1:] if not x["new_beats"]), None, None, None, "0",
         SIM + "These resets have weight 1 and are not beats. Many of them = a treadmill: all runs are the same."),
        ("Simulation stalled", "yes" if A["stalled"] else "no", None, None, None, None,
         "; ".join(f"at {s['at_minutes'] / 60:.1f} h: {s['name']} ({s['note']})" for s in A["stalled"]) or "The buyer always had a goal."),
    ]
    assert ind[8][0].startswith("Last beat") and ind[9][0].startswith("Tail gap")       # rows 13 and 14

    def status(v, lo, hi):
        if isinstance(v, bool) or not isinstance(v, (int, float)):
            return "CHECK"
        return "LOW" if v < lo else "HIGH" if v > hi else "OK"

    dash_rows = {}
    for i, (name, val, live, lo, hi, fmt, read) in enumerate(ind, 5):
        dash_rows[name] = i
        put(wdash, i, 1, name, B)
        put(wdash, i, 2, val, fmt=fmt)
        put(wdash, i, 3, live if live else "(simulation)", fmt=fmt)
        put(wdash, i, 4, lo, INP, YEL if lo is not None else None)
        put(wdash, i, 5, hi, INP, YEL if hi is not None else None)
        if lo is not None:
            st = status(val, lo, hi)
            put(wdash, i, 6, st, fill={"OK": GRN, "CHECK": YEL}.get(st, RED))
            x = f"C{i}" if live else f"B{i}"
            put(wdash, i, 7, f'=IF(NOT(ISNUMBER({x})),"CHECK",IF({x}<D{i},"LOW",IF({x}>E{i},"HIGH","OK")))')
        else:
            put(wdash, i, 6, None)
            put(wdash, i, 7, None)
        put(wdash, i, 8, read, text=True)
    last = 4 + len(ind)
    for word, fill in (("OK", GRN), ("LOW", RED), ("HIGH", RED), ("CHECK", YEL)):
        wdash.conditional_formatting.add(f"G5:G{last}", CellIsRule(operator="equal", formula=[f'"{word}"'], fill=fill))
    r = last + 2

    def block(r, title, lines):
        put(wdash, r, 1, title, H, HF)
        wdash.merge_cells(start_row=r, start_column=1, end_row=r, end_column=8)
        for k, f_ in enumerate(lines, 1):
            put(wdash, r + k, 1, f_ if isinstance(f_, str) else str(f_), text=True)
            wdash.merge_cells(start_row=r + k, start_column=1, end_row=r + k, end_column=8)
            wdash.row_dimensions[r + k].height = 30
        return r + len(lines) + 2

    findings = (m.get("review") or {}).get("findings") or []
    r = block(r, "Review findings", findings or ["Add findings to model.review.findings and run the script again."])
    warns = list(R["warnings"])
    if dropped:
        warns.append("The Timeline keeps all events of weight 2 or more, and a sample of the routine purchases. "
                     "The Events and Purchases counts on Sessions and Swimlane are low for that reason.")
    r = block(r, "Warnings from the script", warns or ["None."])
    r = block(r, "Where to look", [
        "Beats: the moments, with gaps. Lulls: each lull with a probable cause, and the dead zones. Swimlane: which system carries which part of the run.",
        "Profiles: the same model under other reset rules. Target: beats against an ideal curve. Walls, Purchases, Prestige, Ledger: what the buyer did. Sensitivity: which number moves the longest gap.",
        "Formulas follow the yellow cells on Params. Values marked 'From the simulation' change only when the script runs again."])
    pc = ScatterChart()
    pc.style = 13
    pc.series.append(line(wt, 22, 4, 2, nT, title="Cumulative beats (at build)"))
    if A["lulls"]:
        s2 = line(wbt, 19, 18, 2, nBand + 1, title="Lull (script run)")
        s2.graphicalProperties.line.solidFill = "FF0000"
        s2.graphicalProperties.line.width = 19050
        pc.series.append(s2)
    chart_common(pc, "Pacing: cumulative beats (flat parts are lulls; red boxes mark the lulls of the script run)",
                 "Hours played", "Beats", 26, 11)
    wdash.add_chart(pc, f"A{r}")

    wb._sheets = [wb[n] for n in ORDER]
    wb.active = 0
    try:
        wb.save(out)
    except OSError as e:
        raise SystemExit(f"cannot write '{out}': {e.strerror}")
    return dict(timeline_rows=nT - 1, timeline_sampled=dropped, swim_blocks=swim_blocks, dash_rows=dash_rows)
