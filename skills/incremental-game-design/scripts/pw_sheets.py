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


# ---- Styles -------------------------------------------------------------------
FONTS = dict(F=dict(name="Arial", size=10), B=dict(name="Arial", size=10, bold=True),
             H=dict(name="Arial", size=10, bold=True, color="FFFFFF"), T=dict(name="Arial", size=14, bold=True),
             INP=dict(name="Arial", size=10, color="0000FF"))
FILLS = dict(HF="1F3864", YEL="FFFF00", RED="F8CBAD", GRN="C6EFCE", GREY="EDEDED")
RED_SCALE = dict(start_type="num", start_value=0, start_color="FFFFFF", end_type="max", end_color="F8696B")
SESSION_SCALE = dict(start_type="num", start_value=0, start_color="FFFFFF", mid_type="percentile", mid_value=50,
                     mid_color="FFE699", end_type="max", end_color="63BE7B")
SIM = "From the simulation. "

# ---- Params -------------------------------------------------------------------
PARAM_INPUTS = (
    ("Session minutes", "session_minutes", "Length of one play session. From the model."),
    ("Beat weight", "beat_weight", "An event is a beat when its weight is this value or more. 1 = each purchase. 3 = features, prestiges that reach something new, system unlocks."),
    ("Merge seconds", "merge_seconds", "Beats closer than this to the first beat of a stack count as one moment."),
    ("Lull ratio", "lull_ratio", "A gap is a lull when gap / (time played before the gap) is more than this value. This rule decides early in the game."),
    ("Max gap in sessions", "max_gap_sessions", "A gap is also a lull when it is longer than this number of sessions, and longer than the late lull ratio x time played."),
    ("Late lull ratio", "late_ratio", "Later in the game a gap must also be longer than this value x time played. 0.25 = a 2.5-hour wait at hour 10 is normal."),
    ("Min lull minutes", "min_lull_minutes", "Gaps shorter than this value are never a lull."),
    ("Ratio floor minutes", "ratio_floor_minutes", "Lowest value for 'time played before the gap'. This prevents false lulls in the first minutes."),
    ("Target: first gap minutes", "target_first_gap", "Target sheet. Minutes from the start to the first ideal beat."),
    ("Target: stretch ratio", "target_stretch", "Target sheet. Each ideal gap is this value x the gap before it."),
    ("Target hours", "target_hours", "Target sheet. The ideal curve is shown to this time."),
)
SWIM_PARAM = ("Swimlane bucket minutes", "Width of one column in the first two blocks of the Swimlane sheet. The "
              "sheet has {} columns; the last one counts all later events.")
END_TEXT = "The end of the simulated run. The Dashboard uses it for the hours, the tail gap, and the rates."
BUCKET_TEXT = ("Width of one row on the Sessions sheet. The sheet has {} rows. The script sets it: the session "
               "length, or more for a long run.")
PARAMS_NOTE = ("Blue text on yellow: inputs. Change a value and the formulas on Timeline, Beats, Sessions, "
               "Swimlane, Target, and Dashboard follow. The rows of all sheets come from the simulation: run "
               "the script again after a change to the model. The rows of Beats, Lulls, and Target use the "
               "beat weight and the merge seconds of the script run.")

# ---- Timeline -----------------------------------------------------------------
TL_REFS = (("tC", "pm"), ("tH", "weight"), ("tM", "beat"), ("tN", "moment"), ("tF", "kind"), ("tG", "system"),
           ("tK", "note"))
# (column, formula, value or formula of the first row, number format)
TL_LIVE = (
    (13, f"=IF(H{{i}}>={PR['beat']},1,0)", None, None),
    (14, f"=IF(AND(M{{i}}=1,(C{{i}}-O{{i}})*60>={PR['merge']}),1,0)", None, None),
    (15, "=IF(N{p}=1,C{p},O{p})", -1000000, "0.0"),
    (16, '=IF(N{i}=1,C{i}-MAX(O{i},0),"")', None, "0.0"),
    (17, f'=IF(N{{i}}=1,P{{i}}/MAX(MAX(O{{i}},0),{PR["floor"]}),"")', None, "0.00"),
    (18, "=IF(N{i}=1," + lull_formula("P{i}", "MAX(O{i},0)") + ',"")', None, None),
    (19, "=S{p}+N{i}", "=N2", None),
)

# ---- Sessions -----------------------------------------------------------------
SESSION_NAMES = ["Bucket", "Start (played min)", "End (played min)", "Start (h)", "Beats (at build)",
                 "Beat weight sum (at build)", "Flag (at build)", "Beats (live)", "Beat weight sum (live)",
                 "Events (live)", "Purchases (live)", "Top weight (live)", "Flag (live)"]
SESSION_WIDTHS = [8, 11, 11, 9, 9, 11, 9, 9, 11, 9, 10, 10, 9]
SESSIONS_NOTE = ("'At build' columns are values from the script run: a reader sees them with no "
                 "recalculation, and the chart uses them. 'Live' columns are formulas: they follow the beat "
                 "weight on Params, so the two can differ after an edit. Buckets count beats before stacked "
                 "beats are merged. Times are minutes of play.")

# ---- Beats --------------------------------------------------------------------
BEATS_NAMES = ["#", "Played min", "Played hours", "Real hours", "Beat", "Top weight", "Beats in the stack",
               "Systems", "Gap (min, at build)", "Gap ratio (at build)", "Lull (at build)", "Cause of the lull",
               "Last beat at (min, live)", "Gap (min, live)", "Gap ratio (live)", "Lull (live)"]
BEATS_WIDTHS = [6, 10, 9, 9, 50, 8, 8, 18, 10, 9, 8, 18, 10, 10, 9, 8]
BEATS_LIVE = (
    (14, "=B{i}-M{i}", "0.0"),
    (15, f"=N{{i}}/MAX(M{{i}},{PR['floor']})", "0.00"),
    (16, "=" + lull_formula("N{i}", "M{i}"), None),
)
BEATS_NOTE = ("One row for each moment: beats closer than the merge seconds are one row. Red rows end a "
              "lull. 'At build' columns are values from the script run. 'Live' columns are formulas: they "
              "follow the lull thresholds on Params, so the two can differ after an edit. The rows "
              "themselves use the beat weight and the merge seconds of the script run.")

# ---- Lulls --------------------------------------------------------------------
LULLS_NAMES = ["Starts at (h)", "Ends at (h)", "Gap (min)", "Gap ratio", "Ends with", "System that ends it",
               "Sessions in the gap (at build)", "In all buyer profiles?", "Probable cause",
               "Why (from the simulation)", "Options within 5 min (mean)", "Capped lines (share)",
               "log10 income growth", "Wall: need / have", "Resets in the gap", "Purchases in the gap",
               "Min since last new system", "Dead zone", "Diagnosis", "Candidate fix"]
LULLS_WIDTHS = [9, 9, 9, 8, 30, 14, 9, 14, 16, 46, 10, 9, 9, 9, 8, 9, 10, 7, 44, 44]
LULL_FACTS = ((11, "options_within_5_min", "0.0"), (12, "capped_share", "0%"), (13, "income_growth_log10", "0.00"),
              (14, "wall_ratio_at_start", "0.00"), (15, "prestiges_in_gap", None), (16, "purchases_in_gap", None),
              (17, "minutes_since_system_unlock", "0"), (18, "zone", None))
LULLS_NOTE = ("These rows use the thresholds of the script run and the primary buyer profile "
              "('{}'). The Beats and Timeline sheets mark lulls live. 'Probable cause' is a rule on "
              "simulation data: check it against the Timeline. 'depends on reset policy' means that one or more "
              "of the other buyer profiles has no lull at this time (see Profiles). All cells are values of the "
              "script run: after a Params edit, the live lull marks are on the Beats sheet.")
DEAD_ZONE_NAMES = ["Dead zone", "Starts at (h)", "Ends at (h)", "Minutes", "Lulls in it", "Beats inside",
                   "Share of the run"]
DEAD_ZONE_NOTE = ("A dead zone is a group of lulls with less than the max gap between them. It is the stretch "
                  "that a player feels as 'the game went quiet', also when one or two beats fall inside it.")

# ---- Swimlane -----------------------------------------------------------------
SWIM_TITLE = "Swimlane: which system gives beats, and when"
SWIM_INTRO = ("Four blocks. Beats: events at the beat weight or more (before merging). Activity: all events on "
              "the Timeline. First with equal buckets (Params: swimlane bucket minutes), then with buckets "
              "that double (the last column counts all later events). A red cell in an ALL row is a bucket with "
              "no beat. A filled activity cell over an "
              "empty beat cell means 'busy, but nothing new'. Times are minutes of play.")
SWIM_LIVE_TITLE = ("LIVE: the same four blocks as formulas. They follow the beat weight and the swimlane bucket "
                   "on Params.")
SWIM_BUILD_TITLE = "AT BUILD: values of the script run. A reader sees them with no recalculation."
# (title, beats only, equal buckets)
SWIM_BLOCKS = (("Beats, equal buckets", True, True), ("Activity, equal buckets", False, True),
               ("Beats, buckets that double", True, False), ("Activity, buckets that double", False, False))

# ---- Profiles -----------------------------------------------------------------
PROFILE_ROWS = [("Reset rule of the buyer", "text", None), ("Lulls", "lulls", "0"),
                ("Longest gap between beats (min)", "longest_gap_minutes", "0.0"),
                ("Minutes in dead zones", "dead_zone_minutes", "0"), ("Beats (after merging)", "beats", "0"),
                ("Stacked beats", "stacked_beats", "0"), ("Beats in the first hour", "beats_first_hour", "0"),
                ("Minutes to first purchase", "first_purchase_minutes", "0.0"),
                ("Minutes to first new system", "first_system_unlock_minutes", "0.0"),
                ("Minutes to first prestige", "first_prestige_minutes", "0.0"), ("Prestiges", "prestiges", "0"),
                ("Prestiges after a run with nothing new", "prestiges_with_nothing_new", "0"),
                ("Median run length (min)", "median_run_minutes", "0.0"),
                ("Lifetime prestige currency", "lifetime_prestige_currency", "0"),
                ("Highest ladder step", "ladder_step", "0"),
                ("Share of buckets with no beat", "dry_bucket_share", "0%"),
                ("Median options within 5 minutes", "median_options_within_5_min", "0"),
                ("Share of time with no option within 5 minutes", "share_of_time_with_no_option", "0%"),
                ("Simulation stalled", "stalled", None)]
NEVER_KEYS = ("first_purchase_minutes", "first_system_unlock_minutes", "first_prestige_minutes")
PROFILES_NOTE = ("Each column is one full simulation of the same model with a different reset rule. A lull that "
                 "all columns show is a property of the game. A lull in one column only is a property of the "
                 "player model. All values come from the simulation at the thresholds of the script run.")

# ---- Target -------------------------------------------------------------------
TARGET_NAMES = ["Beat #", "Ideal time (min, at build)", "Actual time (min)", "Error (min, at build)",
                "Ideal (h, at build)", "Actual (h)", "Ideal time (min, live)", "Error (min, live)",
                "Actual / ideal (live)"]
TARGET_WIDTHS = [8, 12, 12, 11, 10, 10, 12, 11, 10]
TARGET_LIVE = (f"=IF({PR['stretch']}=1,{PR['g0']}*A{{i}},{PR['g0']}*({PR['stretch']}^A{{i}}-1)/"
               f"({PR['stretch']}-1))")
TARGET_NOTE = ("Ideal time of beat n = first gap x (stretch^n - 1) / (stretch - 1). The three inputs are on "
               "Params. Actual times come from the simulation (the Beats sheet). The part where the error "
               "grows fastest is where the game needs a new beat. 'At build' columns and the chart use the inputs "
               "of the script run. 'Live' columns follow Params.")

# ---- Walls --------------------------------------------------------------------
WALLS_NAMES = ["Ladder", "Step", "Requirement (at build)", "Compares", "Reached at (h)",
               "Wait since the step before (min)", "Value at that time", "Status", "log10 requirement",
               "log10 value", "Base (what-if)", "Growth (what-if)", "Requirement (live what-if)"]
WALLS_WIDTHS = [14, 7, 13, 16, 10, 12, 13, 22, 10, 10, 11, 9, 13]
WALL_VALUE_TITLE = "log10 value (reached steps: at that time; later steps: at the end)"
WALLS_NOTE = ("One row for each ladder step. Time, wait, and value come from the simulation (the first time "
              "that the step was reached). The yellow cells are a what-if for the last column only: the chart "
              "and the simulation do not follow them. For a "
              "step that was not reached, the value is the value at the end of the run: where the "
              "requirement line leaves the value line is the wall.")
NO_LADDER_NOTE = "The model has no ladder. A ladder is the spine of the game: see model-schema.md."

# ---- Purchases ----------------------------------------------------------------
PURCHASE_NAMES = ["Line", "Id", "System", "Status", "First bought (min)", "Last bought (min)", "Highest level",
                  "Levels bought", "Max level at the end", "Spent", "Currency", "Share of spending in the currency",
                  "Payback of the first level (s)", "Payback of the last bought level (s)"]
PURCHASE_WIDTHS = [22, 14, 12, 40, 10, 10, 8, 8, 9, 12, 10, 11, 12, 12]
PURCHASES_NOTE = ("All values come from the simulation, primary buyer profile. Payback = cost / the rise in "
                  "income of the cost currency. It is empty when the line does not raise the currency that "
                  "pays for it. 'Spent' is the first cost currency of the line. Levels bought counts all runs.")

# ---- Prestige -----------------------------------------------------------------
PRESTIGE_NAMES = ["Run", "Start (h)", "Length (min)", "Gain", "Gain each minute"]
PRESTIGE_WIDTHS = [7, 9, 10, 12, 11, 11, 9]
PRESTIGE_CHARTS = ((3, "Run length (healthy: shorter at first, then steady or slowly longer)", "Minutes"),
                   (6, "Highest ladder step in each run (flat = nothing new)", "Step"))
PRESTIGE_NOTE = ("One row for each finished run, from the simulation, primary buyer profile "
                 "('{}'). A red cell: the run reached no new beat, so its prestige has weight 1 and "
                 "is not a beat. The run lengths depend on the reset rule: compare the Profiles sheet.")

# ---- Ledger -------------------------------------------------------------------
LEDGER_NOTE = ("Sources and sinks of each currency over the whole simulation, by system. Income is split by "
               "the system that owns the 'add' effects; multipliers are not split. A source with no sink "
               "of the same size piles up. All values come from the simulation.")

# ---- Sensitivity --------------------------------------------------------------
SENS_KEYS = ("lulls", "longest_gap_minutes", "first_prestige_minutes", "ladder_step", "beats")
SENS_HEADS = (["Parameter", "Base value", "Low value (x0.95)", "High value (x1.05)"]
              + [f"{side}: {name}" for side in ("Low", "High")
                 for name in ("lulls", "longest gap (min)", "first prestige (min)", "ladder step", "beats")]
              + ["Change in longest gap, low (min)", "Change in longest gap, high (min)"])
SENS_WIDTHS = [36, 10, 10, 10] + [9, 11, 11, 9, 8] * 2 + [13, 13]
NO_SENS_TEXT = "The script ran with --no-sensitivity. Run it without that option to fill this sheet."
SENS_NOTE = ("Each row is two more runs of the primary buyer profile ('{primary}') with one number "
             "changed. Sorted by the change in the longest gap. {tested} of {possible} parameters "
             "were tested{limit}"
             " The Low and High columns show the values that were used: for an exp cost the change is held "
             "to half of (growth - 1)." " A small change can move a beat across a threshold: read large values "
             "as 'this number matters', not as a forecast.")
SENS_LIMIT = " (time limit: ladders and the prestige exponent first, then the lines that the buyer bought most)."

# ---- Compare ------------------------------------------------------------------
NO_COMPARE_TEXT = ("No second run. To compare two models: run the script for the first model, then run it for "
                   "the second model with --compare <first>.analysis.json.")
COMPARE_KEYS = [("Simulated hours", "simulated_hours", "0.0"), ("Beats (after merging)", "beats", "0"),
                ("Stacked beats", "stacked_beats", "0"), ("Beats in the first hour", "beats_first_hour", "0"),
                ("Lulls", "_lulls", "0"), ("Longest gap between beats (min)", "longest_gap_minutes", "0.0"),
                ("Minutes in dead zones", "_zone_minutes", "0"), ("Median gap ratio", "median_gap_ratio", "0.00"),
                ("Minutes to first purchase", "first_purchase_minutes", "0.0"),
                ("Minutes to first new system", "first_system_unlock_minutes", "0.0"),
                ("Minutes to first prestige", "first_prestige_minutes", "0.0"),
                ("Prestiges", "prestige_count", "0"), ("Share of buckets with no beat", "dry_session_share", "0%"),
                ("Median options within 5 minutes", "median_options_within_5_min", "0"),
                ("Share of time with no option within 5 minutes", "share_of_time_with_no_option_within_5_min",
                 "0%"),
                ("Purchases never bought", "_never_bought", "0"), ("Milestones never reached", "_never_reached", "0"),
                ("Systems", "systems", "0"), ("Links between systems", "links", "0")]
COMPARE_NOTE = ("Before: {} ({}). After: {} (this workbook). Both columns come from simulations. Check that the "
                "two runs used the same thresholds and the same buyer profile.")

# ---- Income -------------------------------------------------------------------
INCOME_CHART = ("Income growth (flat parts are stalls; steps are purchases and resets)", "Hours played",
                "log10 of income per second")
OPTIONS_CHART = ("Decision density (0 = nothing to do; more than 7 = too many options)", "Hours played", "Options")

# ---- Curves -------------------------------------------------------------------
CURVE_LEVELS = [0, 1, 5, 10, 25, 50, 100]
C_LV = 11
C0 = C_LV + len(CURVE_LEVELS)
C_LIVE = C0 + 5
CURVES_NAMES = (["Purchase", "System", "Cost currency", "Formula", "Base cost (what-if)", "Growth (what-if)",
                 "Max level (model)", "Max level at the end", "Other costs", "Effects"]
                + [f"Cost at L{x} (at build)" for x in CURVE_LEVELS]
                + ["Levels to 10x cost (at build)", "First bought (min)", "Highest level reached",
                   "Payback of the first level (s)", "Payback of the last bought level (s)"]
                + [f"Cost at L{x} (live what-if)" for x in CURVE_LEVELS] + ["Levels to 10x cost (live what-if)"])
CURVES_WIDTHS = ([22, 12, 10, 8, 10, 8, 8, 8, 22, 34] + [11] * len(CURVE_LEVELS) + [10, 10, 10, 12, 12]
                 + [11] * len(CURVE_LEVELS) + [10])
CURVE_LIVE = '=IF($D{i}="linear",$E{i}+$F{i}*{n},IF($D{i}="poly",$E{i}*({n}+1)^$F{i},$E{i}*$F{i}^{n}))'
CURVES_NOTE = ("Yellow cells are a what-if for this sheet only: they move the 'live what-if' columns at the "
               "right. They do not change the 'at build' columns, the chart, or the simulation. To see the result "
               "on pacing, edit model.json and run the script "
               "again. The last four columns come from the simulation. Payback = cost / the rise in income of "
               "the cost currency: when the payback of the last level is far above the first, the line has "
               "slowed down and needs other systems to multiply it.")
CURVES_CHART = "Cost curves: first 10 lines, each to its max level (currencies differ: compare shapes)"

# ---- Systems and Matrix -------------------------------------------------------
SYSTEMS_NAMES = ["System", "Name", "Loop", "Persists through prestige", "Clock", "First event (played min, at build)",
                 "Events on the Timeline (at build)", "Links out (at build)", "Links in (at build)",
                 "Role (at build)", "Description", "First event (played min, live)", "Events on the Timeline (live)",
                 "Links out (live)", "Links in (live)", "Role (live)"]
SYSTEMS_WIDTHS = [14, 20, 11, 10, 8, 10, 9, 8, 8, 16, 60, 10, 8, 8, 8, 16]
SYSTEM_LIVE = ((12, '=IFERROR(INDEX({tC},MATCH($A{i},{tG},0)),"never")', "0.0"),
               (13, "=COUNTIF({tG},$A{i})", None),
               (14, "=COUNTIF(Links!$A$2:$A${nL},$A{i})", None),
               (15, "=COUNTIF(Links!$B$2:$B${nL},$A{i})", None),
               (16, '=IF(N{i}+O{i}=0,"ORPHAN",IF(N{i}=0,"SINK ONLY",IF(O{i}=0,"SOURCE ONLY","CONNECTED")))', None))
SYSTEMS_NOTE = ("'At build' columns are values from the script run. 'Live' columns are formulas on the "
                "Timeline and Links sheets: they differ only if you edit those sheets.")
MATRIX_NOTE = ("Count of links from the row system to the column system. Values of the script run, from the "
               "Links sheet.")

# ---- Ideas --------------------------------------------------------------------
IDEA_COLS = ["Idea", "Archetype", "Problem it solves", "Feeds (core-loop value)", "Fed by", "Unlock point",
             "Persists through prestige", "Scope (S/M/L)", "Juice moment", "Risk"]
IDEA_KEYS = ["idea", "archetype", "problem", "feeds", "fed_by", "unlock", "persists", "scope", "juice", "risk"]
IDEA_WIDTHS = [26, 18, 40, 30, 26, 22, 10, 9, 36, 36]
NO_IDEAS_TEXT = "Add ideas to model.review.ideas and run the script again, or type them here."

# ---- Reference ----------------------------------------------------------------
REF_NAMES = ["Reference: main level", "Share of main axis", "System that opens", "Share of systems open", "",
             "This game: system", "First event (played h)", "Share of progress (x)", "Share of systems open"]
REF_WIDTHS = [12, 10, 26, 10, 3, 20, 10, 12, 10]
REF_AXIS_TEXT = ("X axis of this game: {}. X axis of the reference game: share of its main level axis. "
                 "Y axis of both: share of all systems that are open. ")
REF_AXIS_PROGRESS = "Both x axes are a progress axis, so the two curves can be compared in shape."
REF_AXIS_TIME = ("A level axis is close to a log-time axis: against it, a time axis makes a "
                 "healthy game look front-loaded. Set game.progress_axis to compare.")
NO_REF_TEXT = "The reference curve file was not found (assets/reference_unlock_curve.json)."

# ---- Dashboard ----------------------------------------------------------------
DASH_HEADS = ["Indicator", "At build", "Live", "Low", "High", "Status (at build)", "Status (live)",
              "How to read it"]
DASH_INTRO_TAIL = ("'At build' = the value of the script run: it needs no "
                   "recalculation. 'Live' = a formula that follows the yellow cells on Params; the two can differ "
                   "after an edit. Bands are starting values: change the blue cells for your game.")
DASH_NEVER = 'IFERROR(INDEX({0},MATCH("{1}",{2},0)),"never")'
DASH_LULLS_LIVE = '=COUNTIF({lull},"LULL")+IF(' + lull_formula("C14", "C13") + '="LULL",1,0)'
DASH_TARGET_LIVE = (f'=IFERROR(IF({PR["end"]}<{{m}},"n/a",ROUNDDOWN(LN(1+{{m}}*({PR["stretch"]}-1)/{PR["g0"]})/'
                    f'LN({PR["stretch"]}),0)-COUNTIFS({{tC}},"<="&{{m}},{{tN}},1)),"n/a")')
DASH_TARGET_READ = ("Ideal beats by this hour (Target sheet inputs on Params) minus actual beats. Above 0 = late. "
                    "'n/a' = the run is shorter.")
NO_FINDINGS_TEXT = "Add findings to model.review.findings and run the script again."
DROPPED_WARNING = ("The Timeline keeps all events of weight 2 or more, and a sample of the routine purchases. "
                   "The Events and Purchases counts on Sessions and Swimlane are low for that reason.")
WHERE_TO_LOOK = [
    "Beats: the moments, with gaps. Lulls: each lull with a probable cause, and the dead zones. Swimlane: which "
    "system carries which part of the run.",
    "Profiles: the same model under other reset rules. Target: beats against an ideal curve. Walls, Purchases, "
    "Prestige, Ledger: what the buyer did. Sensitivity: which number moves the longest gap.",
    "Formulas follow the yellow cells on Params. Values marked 'From the simulation' change only when the script "
    "runs again."]
PACING_TITLE = "Pacing: cumulative beats (flat parts are lulls; red boxes mark the lulls of the script run)"
DASH_STATUS = ('=IF(NOT(ISNUMBER({x})),"CHECK",IF({x}<D{i},"LOW",IF({x}>E{i},"HIGH","OK")))')


# ---- Shared helpers -----------------------------------------------------------
def col_letter(i):
    from openpyxl.utils import get_column_letter
    return get_column_letter(i)


def make_styles(bk):
    from openpyxl.styles import Alignment, Border, Font, PatternFill, Side
    for name, kw in FONTS.items():
        setattr(bk, name, Font(**kw))
    for name, color in FILLS.items():
        setattr(bk, name, PatternFill("solid", start_color=color))
    side = Side(style="thin", color="BFBFBF")
    bk.box = Border(left=side, right=side, top=side, bottom=side)
    bk.wrap = Alignment(wrap_text=True, vertical="top")
    bk.NOB = Border()


class Book:
    """The state of 1 workbook: the inputs, the styles, and the values that a later sheet reads."""

    def __init__(self, m, R, ref_path):
        from openpyxl import Workbook
        self.m, self.R, self.g, self.ref_path = m, R, m["game"], ref_path
        self.sim, self.A, self.P = R["sim"], R["A"], R["P"]
        self.end, self.primary = self.A["simulated_end_minutes"], R["primary"]
        make_styles(self)
        self.wb = Workbook()

    def put(self, ws, r, c, v, font=None, fill=None, fmt=None, text=False):
        cell = ws.cell(row=r, column=c, value=fin(v))
        if text and isinstance(v, str):
            cell.data_type = "s"
        cell.font, cell.alignment, cell.border = self.F if font is None else font, self.wrap, self.box
        if fill:
            cell.fill = fill
        if fmt:
            cell.number_format = fmt
        return cell

    def head(self, ws, r, names, widths, freeze=True):
        cols = enumerate(zip(names, widths), 1)
        for i, (n, w) in cols:
            self.head_cell(ws, r, i, n, w)
        if freeze:
            ws.freeze_panes = ws.cell(row=r + 1, column=1)

    def head_cell(self, ws, r, i, n, w):
        self.put(ws, r, i, n, self.H, self.HF)
        ws.column_dimensions[col_letter(i)].width = w

    def note(self, ws, r, text, last_col=8, height=30):
        self.put(ws, r, 1, text, self.B, text=True)
        ws.merge_cells(start_row=r, start_column=1, end_row=r, end_column=last_col)
        ws.row_dimensions[r].height = height

    def heads(self, ws, r, names, first=1):
        cols = enumerate(names, first)
        for k, n in cols:
            self.put(ws, r, k, n, self.H, self.HF)


def chart_common(ch, title, x, y, w=22, h=9):
    ch.title, ch.x_axis.title, ch.y_axis.title = title, x, y
    ch.width, ch.height = w, h
    ch.roundedCorners = False
    ch.x_axis.delete = False
    ch.y_axis.delete = False
    if ch.legend is not None:
        ch.legend.overlay = False


def xy_series(ws, y, x, r2, title):
    """A scatter series: y and x are (column, first row). A title of None takes the title from the data."""
    from openpyxl.chart import Reference, Series
    return Series(Reference(ws, min_col=y[0], min_row=y[1], max_row=r2),
                  Reference(ws, min_col=x[0], min_row=x[1], max_row=r2), title=title, title_from_data=title is None)


def line(ws, ycol, xcol, r1, r2, title=None, head_row=None):
    """A scatter series with no markers. Data rows r1..r2."""
    s = xy_series(ws, (ycol, head_row if title is None else r1), (xcol, r1), r2, title)
    s.marker.symbol = "none"
    return s


def scatter():
    from openpyxl.chart import ScatterChart
    ch = ScatterChart()
    ch.style = 13
    return ch


def add_rule(ws, area, rule_name, **kw):
    from openpyxl.formatting import rule
    ws.conditional_formatting.add(area, getattr(rule, rule_name)(**kw))


# ---- Params -------------------------------------------------------------------
def sheet_params(bk):
    wp = bk.wb.active
    wp.title = "Params"
    bk.head(wp, 1, ["Parameter", "Value", "Meaning"], [34, 12, 100])
    session = bk.P["session_minutes"]
    bk.swim_n = min(48, bk.A["sessions"])
    bk.swim_bucket = session * max(1, math.ceil(bk.end / session / 48 - 1e-9))
    for i, row in enumerate(param_rows(bk), 2):
        param_row(bk, wp, i, row)
    params_sim(bk, wp)
    bk.note(wp, 19, PARAMS_NOTE, 3, 45)


def param_rows(bk):
    rows = [(a, bk.P[k], d) for a, k, d in PARAM_INPUTS]
    return rows + [(SWIM_PARAM[0], bk.swim_bucket, SWIM_PARAM[1].format(bk.swim_n))]


def param_row(bk, ws, i, row):
    a, v, d = row
    bk.put(ws, i, 1, a, bk.B)
    bk.put(ws, i, 2, v, bk.INP, bk.YEL)
    bk.put(ws, i, 3, d)


def params_sim(bk, ws):
    bk.put(ws, 15, 1, "From the simulation (not inputs)", bk.H, bk.HF)
    bk.put(ws, 15, 2, None, bk.H, bk.HF)
    bk.put(ws, 15, 3, None, bk.H, bk.HF)
    bk.put(ws, 16, 1, "Simulated end (minutes of play)", bk.B)
    bk.put(ws, 16, 2, bk.end)
    bk.put(ws, 16, 3, END_TEXT)
    bk.put(ws, 17, 1, "Bucket minutes (Sessions sheet)", bk.B)
    bk.put(ws, 17, 2, bk.A["bucket_minutes"])
    bk.put(ws, 17, 3, BUCKET_TEXT.format(bk.A["sessions"]))


# ---- Timeline -----------------------------------------------------------------
def routine_row(e, P):
    return e["weight"] < 2 and e["kind"] != "stall" and e["weight"] < P["beat_weight"]


def sample_routine(routine, room):
    """Return the routine rows that the Timeline keeps, and True when it dropped some."""
    if len(routine) <= max(room, 0):
        return routine, False
    return (routine[::math.ceil(len(routine) / max(room, 1))] if room > 0 else []), True


def number_events(ev):
    pairs = enumerate(ev)
    for i, e in pairs:
        e["_i"] = i


def timeline_rows(bk):
    ev = bk.sim.events
    number_events(ev)
    keep = [e for e in ev if not routine_row(e, bk.P)]
    routine, bk.dropped = sample_routine([e for e in ev if routine_row(e, bk.P)], 6000 - len(keep))
    bk.rows = sorted(keep + routine, key=lambda e: e["_i"])


def sheet_timeline(bk):
    timeline_rows(bk)
    wt = bk.wt = bk.wb.create_sheet("Timeline")
    bk.curs = [c["id"] for c in bk.m["currencies"]]
    names = TL_NAMES + [f"log10 income {c}" for c in bk.curs]
    bk.head(wt, 1, names, TL_WIDTHS + [11] * len(bk.curs))
    bk.anchor, bk.cum = -1e6, 0
    for i, e in enumerate(bk.rows, 2):
        timeline_row(bk, wt, i, e)
    bk.nT = max(len(bk.rows) + 1, 2)
    timeline_formats(bk, wt, col_letter(len(names)))
    timeline_refs(bk)


def timeline_row(bk, ws, i, e):
    tl_times(bk, ws, i, e)
    tl_build(bk, ws, i, e)
    tl_event(bk, ws, i, e)
    tl_live(bk, ws, i)
    tl_income(bk, ws, i, e)


def tl_times(bk, ws, i, e):
    bk.put(ws, i, 1, round(e["real"] / 60.0, 4), fmt="0.0")
    bk.put(ws, i, 2, round(e["real"] / 3600.0, 5), fmt="0.00")
    bk.put(ws, i, 3, pm(e), fmt="0.0")
    bk.put(ws, i, 4, round(pm(e) / 60.0, 5), fmt="0.00")


def tl_build(bk, ws, i, e):
    """The same rule as the live columns M to S, as values of the script run."""
    t = pm(e)
    if e["weight"] >= bk.P["beat_weight"] and (t - bk.anchor) * 60 >= bk.P["merge_seconds"]:
        tl_beat(bk, ws, i, t)
    else:
        bk.put(ws, i, 20, None)
        bk.put(ws, i, 21, None)
    bk.put(ws, i, 22, bk.cum)


def tl_beat(bk, ws, i, t):
    before = max(bk.anchor, 0)
    bk.cum += 1
    bk.put(ws, i, 20, t - before, fmt="0.0")
    bk.put(ws, i, 21, "LULL" if is_lull(t - before, before, bk.P) else None)
    bk.anchor = t


def tl_event(bk, ws, i, e):
    bk.put(ws, i, 5, e["name"], text=True)
    bk.put(ws, i, 6, e["kind"])
    bk.put(ws, i, 7, e["system"], text=True)
    bk.put(ws, i, 8, e["weight"], bk.INP)
    bk.put(ws, i, 9, e["cost"], fmt="0.00E+00")
    bk.put(ws, i, 10, e["currency"], text=True)
    bk.put(ws, i, 11, e["note"], text=True)
    bk.put(ws, i, 12, e["prestiges"])


def tl_live(bk, ws, i):
    cells = TL_LIVE
    for col, tpl, first, fmt in cells:
        bk.put(ws, i, col, first if i == 2 and first is not None else tpl.format(i=i, p=i - 1), fmt=fmt)


def tl_income(bk, ws, i, e):
    cols = enumerate(bk.curs, len(TL_NAMES) + 1)
    for k, c in cols:
        bk.put(ws, i, k, round(math.log10(e["income"][c] + 1), 4), fmt="0.00")


def timeline_formats(bk, ws, last):
    nT = bk.nT
    ws.auto_filter.ref = f"A1:{last}{nT}"
    add_rule(ws, f"A2:{last}{nT}", "FormulaRule", formula=['$U2="LULL"'], fill=bk.RED)
    add_rule(ws, f"A2:{last}{nT}", "FormulaRule", formula=["$H2>=5"], fill=bk.GRN)


def tl_range(nT, key):
    return f"Timeline!${TL[key]}$2:${TL[key]}${nT}"


def timeline_refs(bk):
    refs = TL_REFS
    for attr, key in refs:
        setattr(bk, attr, tl_range(bk.nT, key))


# ---- Sessions -----------------------------------------------------------------
def sheet_sessions(bk):
    bk.nS = nS = bk.A["sessions"]
    ws = bk.wb.create_sheet("Sessions")
    bk.head(ws, 1, SESSION_NAMES, SESSION_WIDTHS)
    counts = session_counts(bk)
    for i in range(2, nS + 2):
        session_build(bk, ws, i, counts)
        session_live(bk, ws, i)
    add_rule(ws, f"F2:F{nS + 1}", "ColorScaleRule", **SESSION_SCALE)
    add_rule(ws, f"M2:M{nS + 1}", "CellIsRule", operator="equal", formula=['"DRY"'], fill=bk.RED)
    sessions_chart(ws, nS)
    bk.note(ws, nS + 3, SESSIONS_NOTE, 13, 45)


def session_counts(bk):
    nS, bucket = bk.nS, bk.A["bucket_minutes"]
    sb_n, sb_w = [0] * nS, [0] * nS
    beats = [e for e in bk.sim.events if e["weight"] >= bk.P["beat_weight"]]
    for e in beats:
        add_session_beat(sb_n, sb_w, min(nS - 1, int(pm(e) // bucket)), e["weight"])
    return sb_n, sb_w


def add_session_beat(sb_n, sb_w, k, weight):
    sb_n[k] += 1
    sb_w[k] += weight


def session_build(bk, ws, i, counts):
    bucket, n = bk.A["bucket_minutes"], counts[0][i - 2]
    bk.put(ws, i, 1, i - 1)
    bk.put(ws, i, 2, (i - 2) * bucket)
    bk.put(ws, i, 3, (i - 1) * bucket)
    bk.put(ws, i, 4, round((i - 2) * bucket / 60.0, 4), fmt="0.0")
    bk.put(ws, i, 5, n)
    bk.put(ws, i, 6, counts[1][i - 2])
    bk.put(ws, i, 7, None if n else "DRY", fill=None if n else bk.RED)


def session_live(bk, ws, i):
    tC, lt = bk.tC, "<=" if i == bk.nS + 1 else "<"
    span = f'{tC},">="&B{i},{tC},"{lt}"&C{i}'
    bk.put(ws, i, 8, f"=COUNTIFS({span},{bk.tM},1)")
    bk.put(ws, i, 9, f"=SUMIFS({bk.tH},{span},{bk.tM},1)")
    bk.put(ws, i, 10, f"=COUNTIFS({span})")
    bk.put(ws, i, 11, f'=COUNTIFS({span},{bk.tF},"purchase")')
    bk.put(ws, i, 12, f"=SUMPRODUCT(MAX(({tC}>=B{i})*({tC}{lt}C{i})*{bk.tH}))")
    bk.put(ws, i, 13, f'=IF(H{i}=0,"DRY","")')


def sessions_chart(ws, nS):
    from openpyxl.chart import BarChart, Reference
    bc = BarChart()
    bc.add_data(Reference(ws, min_col=6, min_row=1, max_row=nS + 1), titles_from_data=True)
    bc.set_categories(Reference(ws, min_col=4, min_row=2, max_row=nS + 1))
    chart_common(bc, "Beat weight in each bucket (no bar = no beat = a dry session)", "Start hour (played)",
                 "Sum of the weights of the beats", 26, 9)
    bc.legend = None
    ws.add_chart(bc, "O2")


# ---- Beats --------------------------------------------------------------------
def sheet_beats(bk):
    ws = bk.wbt = bk.wb.create_sheet("Beats")
    bk.mos = bk.A["moments"]
    bk.head(ws, 1, BEATS_NAMES, BEATS_WIDTHS)
    tail, cause_at = tail_gap(bk), {round(x["at_minutes"], 4): x.get("cause") for x in bk.A["lulls"]}
    for i, mo in enumerate(bk.mos + [None], 2):
        beat_row(bk, ws, i, (tail, mo), cause_at)
    nB = len(bk.mos) + 2                   # the last row is the end of the simulation
    add_rule(ws, f"A2:L{nB}", "FormulaRule", formula=['$K2="LULL"'], fill=bk.RED)
    add_rule(ws, f"M2:P{nB}", "FormulaRule", formula=['$P2="LULL"'], fill=bk.RED)
    bk.note(ws, nB + 2, BEATS_NOTE, 16, 45)
    lull_bands(bk, ws)


def tail_gap(bk):
    A, P = bk.A, bk.P
    return dict(at=bk.end, after=A["last_beat_minutes"], gap=A["tail_gap_minutes"],
                ratio=A["tail_gap_minutes"] / max(A["last_beat_minutes"], P["ratio_floor_minutes"]),
                lull=is_lull(A["tail_gap_minutes"], A["last_beat_minutes"], P))


def beat_row(bk, ws, i, pair, cause_at):
    tail, mo = pair
    if mo is None:
        beat_end(bk, ws, i)
        mo = tail
    else:
        beat_moment(bk, ws, i, mo)
    beat_gap(bk, ws, i, mo, cause_at)
    beat_live(bk, ws, i)


def beat_end(bk, ws, i):
    bk.put(ws, i, 1, "end")
    bk.put(ws, i, 4, round(bk.sim.real / 3600.0, 3), fmt="0.00")
    bk.put(ws, i, 5, "(end of simulation: no beat)", bk.B)
    for k in (6, 7, 8):
        bk.put(ws, i, k, None)


def beat_moment(bk, ws, i, mo):
    bk.put(ws, i, 1, i - 1)
    bk.put(ws, i, 4, round(mo["real"] / 60.0, 3), fmt="0.00")
    bk.put(ws, i, 5, " + ".join(mo["names"]), text=True)
    bk.put(ws, i, 6, mo["weight"])
    bk.put(ws, i, 7, mo["stacked"])
    bk.put(ws, i, 8, ", ".join(mo["systems"]), text=True)


def beat_gap(bk, ws, i, mo, cause_at):
    bk.put(ws, i, 2, mo["at"], fmt="0.0")
    bk.put(ws, i, 3, round(mo["at"] / 60.0, 5), fmt="0.00")
    bk.put(ws, i, 9, mo["gap"], fmt="0.0")
    bk.put(ws, i, 10, mo["ratio"], fmt="0.00")
    bk.put(ws, i, 11, "LULL" if mo["lull"] else None)
    bk.put(ws, i, 12, cause_at.get(round(mo["at"], 4)) if mo["lull"] else None)


def beat_live(bk, ws, i):
    bk.put(ws, i, 13, 0 if i == 2 else f"=B{i - 1}", fmt="0.0")
    cells = BEATS_LIVE
    for col, tpl, fmt in cells:
        bk.put(ws, i, col, tpl.format(i=i), fmt=fmt)


def lull_bands(bk, ws):
    """Lull bands for the pacing chart: one rectangle outline for each lull, from the script run."""
    bk.put(ws, 1, 18, "Lull band: hours", bk.H, bk.HF)
    bk.put(ws, 1, 19, "Lull band: beats", bk.H, bk.HF)
    ws.column_dimensions["R"].width = ws.column_dimensions["S"].width = 11
    r = 2
    for gp in bk.A["lulls"]:
        r = lull_band(bk, ws, r, gp) + 1   # an empty row breaks the line between two bands
    bk.nBand = max(r - 2, 2)


def lull_band(bk, ws, r, gp):
    top = len(bk.mos)
    corners = ((gp["after"], 0), (gp["after"], top), (gp["at_minutes"], top), (gp["at_minutes"], 0))
    for k, (x, y) in enumerate(corners):
        bk.put(ws, r + k, 18, round(x / 60.0, 4), fmt="0.00")
        bk.put(ws, r + k, 19, y)
    return r + 4


# ---- Lulls --------------------------------------------------------------------
def sheet_lulls(bk):
    ws = bk.wb.create_sheet("Lulls")
    bk.head(ws, 1, LULLS_NAMES, LULLS_WIDTHS)
    notes = lull_notes(bk.m)
    for i, gp in enumerate(bk.A["lulls"], 2):
        lull_row(bk, ws, i, gp, notes)
    r = len(bk.A["lulls"]) + 3
    bk.note(ws, r, LULLS_NOTE.format(bk.primary), 20, 45)
    dead_zone_rows(bk, ws, r + 2)


def lull_notes(m):
    return [dict(x) for x in ((m.get("review") or {}).get("lull_notes") or []) if isinstance(x, dict)]


def lull_row(bk, ws, i, gp, notes):
    start = gp["after"] / 60.0
    lull_span(bk, ws, i, gp, start)
    lull_facts(bk, ws, i, gp)
    nt = pick_note(notes, start)
    bk.put(ws, i, 19, nt.get("diagnosis"), bk.INP, bk.YEL, text=True)
    bk.put(ws, i, 20, nt.get("fix"), bk.INP, bk.YEL, text=True)


def lull_span(bk, ws, i, gp, start):
    bk.put(ws, i, 1, round(start, 2), fmt="0.00")
    bk.put(ws, i, 2, round(gp["at_minutes"] / 60.0, 2), fmt="0.00")
    bk.put(ws, i, 3, round(gp["gap_minutes"], 1), fmt="0.0")
    bk.put(ws, i, 4, round(gp["ratio"], 2), fmt="0.00")
    bk.put(ws, i, 5, gp["ends_with"], text=True)
    bk.put(ws, i, 6, gp["system"], text=True)
    bk.put(ws, i, 7, math.floor(round(gp["gap_minutes"], 1) / bk.P["session_minutes"]))
    bk.put(ws, i, 8, gp.get("robust"), fill=bk.GRN if gp.get("robust") == "robust" else None)
    bk.put(ws, i, 9, gp.get("cause"), bk.B)
    bk.put(ws, i, 10, gp.get("why"), text=True)


def lull_facts(bk, ws, i, gp):
    facts = LULL_FACTS
    for col, key, fmt in facts:
        bk.put(ws, i, col, gp.get(key), fmt=fmt)


def note_distance(x, start):
    return abs((x.get("lull_at_hours") or 0) - start)


def pick_note(notes, start):
    """The nearest note that is close enough, and each note one time only."""
    near = [(note_distance(x, start), k) for k, x in enumerate(notes)
            if not x.get("_used") and note_distance(x, start) <= max(0.25, 0.1 * start)]
    nt = notes[min(near)[1]] if near else {}
    if nt:
        nt["_used"] = True
    return nt


def dead_zone_rows(bk, ws, r):
    bk.heads(ws, r, DEAD_ZONE_NAMES)
    for z, zn in enumerate(bk.A["dead_zones"], 1):
        dead_zone_row(bk, ws, r + z, z, zn)
    bk.note(ws, r + len(bk.A["dead_zones"]) + 2, DEAD_ZONE_NOTE, 20)


def dead_zone_row(bk, ws, r, z, zn):
    bk.put(ws, r, 1, z)
    bk.put(ws, r, 2, round(zn["start_minutes"] / 60.0, 2), fmt="0.00")
    bk.put(ws, r, 3, round(zn["end_minutes"] / 60.0, 2), fmt="0.00")
    bk.put(ws, r, 4, round(zn["minutes"], 1), fmt="0")
    bk.put(ws, r, 5, zn["lulls"])
    bk.put(ws, r, 6, zn["beats_inside"])
    bk.put(ws, r, 7, round(zn["share_of_run"], 4), fmt="0%")


# ---- Swimlane -----------------------------------------------------------------
class Block:
    """1 block of the Swimlane sheet: its rows and its bucket edges."""

    def __init__(self, bk, r, spec, live):
        self.title, self.beats, self.linear = spec
        self.live = live
        self.edges = bk.lin_edges if self.linear else bk.log_edges[:-1]
        self.n = len(self.edges)
        self.e_row, self.h_row = r + 1, r + 2
        self.top, self.bot = r + 3, r + 3 + len(bk.lanes)


def sheet_swimlane(bk):
    ws = bk.wb.create_sheet("Swimlane")
    swim_lanes(bk)
    swim_header(bk, ws)
    swim_times(bk)
    bk.swim_blocks = []
    r = 4
    for live in (False, True):
        r = swim_half(bk, ws, r, live)
    ws.freeze_panes = "D4"


def first_events(ev):
    first = {}
    named = [e for e in ev if e["system"] != "-"]
    for e in named:
        first.setdefault(e["system"], e["t"])
    return first


def swim_lanes(bk):
    first = first_events(bk.sim.events)
    bk.loops = {s["id"]: s["loop"] for s in bk.m["systems"]}
    bk.lanes = sorted(first, key=first.get) + [s["id"] for s in bk.m["systems"] if s["id"] not in first]
    bk.log_edges = log_edges(bk.end)
    bk.lin_edges = [k * bk.swim_bucket for k in range(bk.swim_n)]


def log_edges(end):
    edges = [0.0, 5.0]
    while edges[-1] < end and len(edges) < 40:
        edges.append(edges[-1] * 2)
    return edges[:-1] if len(edges) > 2 and edges[-2] >= end else edges


def swim_header(bk, ws):
    bk.put(ws, 1, 1, SWIM_TITLE, bk.T).border = bk.NOB
    bk.put(ws, 2, 1, SWIM_INTRO)
    ws.merge_cells("A2:P2")
    ws.row_dimensions[2].height = 45
    widths = (("A", 16), ("B", 11), ("C", 8))
    for c, w in widths:
        ws.column_dimensions[c].width = w


def swim_times(bk):
    """Lane -> (times of all rows, times of beat rows)."""
    bk.times = {"ALL": ([], [])}
    rows = bk.rows
    for e in rows:
        add_swim_time(bk, e)


def add_swim_time(bk, e):
    beat = e["weight"] >= bk.P["beat_weight"]
    for key in (e["system"], "ALL"):
        add_lane_time(bk.times.setdefault(key, ([], [])), pm(e), beat)


def add_lane_time(lane, t, beat):
    lane[0].append(t)
    if beat:
        lane[1].append(t)


def swim_count(bk, sid, lo, hi, beats_only):
    ts = bk.times.get(sid, ([], []))[1 if beats_only else 0]
    return bisect.bisect_left(ts, hi) - bisect.bisect_left(ts, lo)


def swim_half(bk, ws, r, live):
    bk.put(ws, r, 1, SWIM_LIVE_TITLE if live else SWIM_BUILD_TITLE, bk.B).border = bk.NOB
    r += 2
    specs = SWIM_BLOCKS
    for spec in specs:
        r = swim_block(bk, ws, Block(bk, r, spec, live))
    return r


def swim_block(bk, ws, blk):
    bk.put(ws, blk.e_row - 1, 1, blk.title + (" (live)" if blk.live else " (at build)"), bk.B).border = bk.NOB
    swim_edges(bk, ws, blk)
    lanes = bk.lanes + ["ALL"]
    for j, sid in enumerate(lanes):
        swim_lane(bk, ws, blk, blk.top + j, sid)
    swim_formats(bk, ws, blk)
    bk.swim_blocks.append(dict(title=blk.title, live=blk.live, edge_row=blk.e_row, first=blk.top, last=blk.bot,
                               n=blk.n, beats=blk.beats))
    return blk.bot + 3


def swim_edges(bk, ws, blk):
    bk.put(ws, blk.e_row, 1, "Start (min)", bk.B)
    bk.put(ws, blk.e_row, 2, None)
    bk.put(ws, blk.e_row, 3, None)
    for k in range(blk.n):
        swim_edge(bk, ws, blk, k)
    bk.put(ws, blk.e_row, 4 + blk.n, LAST_EDGE, fmt="0E+00")       # the last bucket has no end
    bk.heads(ws, blk.h_row, ["System", "Loop", "Total"])


def swim_edge(bk, ws, blk, k):
    col, formula = 4 + k, blk.live and blk.linear
    bk.put(ws, blk.e_row, col, f"={k}*{PR['swim']}" if formula else blk.edges[k], fmt="0")
    bk.put(ws, blk.h_row, col, f"={col_letter(col)}{blk.e_row}/60" if formula else blk.edges[k] / 60.0, bk.H,
           bk.HF, fmt="0.0")
    ws.column_dimensions[col_letter(col)].width = 5.5


def swim_lane(bk, ws, blk, rr, sid):
    bk.put(ws, rr, 1, sid, bk.B, text=True)
    bk.put(ws, rr, 2, bk.loops.get(sid, "") if sid != "ALL" else "")
    his = blk.edges[1:] + [LAST_EDGE]
    vals = [swim_count(bk, sid, lo, hi, blk.beats) for lo, hi in zip(blk.edges, his)]
    bk.put(ws, rr, 3, f"=SUM({col_letter(4)}{rr}:{col_letter(3 + blk.n)}{rr})" if blk.live else sum(vals))
    cell = swim_live_cell if blk.live else swim_build_cell
    for k in range(blk.n):
        cell(bk, ws, blk, (rr, sid, k), vals)


def swim_build_cell(bk, ws, blk, at, vals):
    rr, sid, k = at
    bk.put(ws, rr, 4 + k, vals[k], fmt="0;-0;;@")


def swim_live_cell(bk, ws, blk, at, vals):
    rr, sid, k = at
    col, e_row, tC = 4 + k, blk.e_row, bk.tC
    crit = f'{tC},">="&{col_letter(col)}${e_row},{tC},"<"&{col_letter(col + 1)}${e_row}'
    if sid != "ALL":
        crit = f"{bk.tG},$A{rr}," + crit
    if blk.beats:
        crit += f",{bk.tM},1"
    bk.put(ws, rr, col, f"=COUNTIFS({crit})", fmt="0;-0;;@")


def swim_formats(bk, ws, blk):
    first, last = col_letter(4), col_letter(3 + blk.n)
    if bk.lanes:
        add_rule(ws, f"{first}{blk.top}:{last}{blk.bot - 1}", "ColorScaleRule", start_type="num", start_value=0,
                 start_color="FFFFFF", end_type="max", end_color="63BE7B" if blk.beats else "9BC2E6")
    if not blk.beats:
        return
    area = f"{first}{blk.bot}:{last}{blk.bot}"
    add_rule(ws, area, "CellIsRule", operator="equal", formula=["0"], fill=bk.RED)
    add_rule(ws, area, "CellIsRule", operator="greaterThan", formula=["0"], fill=bk.GRN)


# ---- Profiles -----------------------------------------------------------------
def sheet_profiles(bk):
    ws = bk.wb.create_sheet("Profiles")
    bk.profs = profs = bk.R["profiles"]
    names = ["Indicator"] + [p["name"] + (" (primary)" if p["name"] == bk.primary else "") for p in profs]
    bk.head(ws, 1, names, [40] + [22] * len(profs))
    for i, row in enumerate(PROFILE_ROWS, 2):
        profile_row(bk, ws, i, row)
    ws.row_dimensions[2].height = 55
    r = len(PROFILE_ROWS) + 3
    bk.note(ws, r, PROFILES_NOTE, 1 + len(profs), 60)
    profile_lulls(bk, ws, r + 2)


def profile_row(bk, ws, i, row):
    label, key, fmt = row
    bk.put(ws, i, 1, label, bk.B)
    for k, p in enumerate(bk.profs, 2):
        bk.put(ws, i, k, profile_value(p, key), fmt=fmt, text=True)


def profile_value(p, key):
    v = p["text"] if key == "text" else p["summary"].get(key)
    if key in NEVER_KEYS and v is None:
        return "never"
    return ("yes" if v else "no") if key == "stalled" else v


def profile_lulls(bk, ws, r):
    bk.heads(ws, r, ["Profile", "Lull starts (h)", "Lull ends (h)", "Minutes"])
    spans = [(p["name"], a, b) for p in bk.profs for a, b in p["summary"]["lull_hours"]]
    for k, span in enumerate(spans, r + 1):
        profile_lull(bk, ws, k, span)


def profile_lull(bk, ws, r, span):
    name, a, b = span
    bk.put(ws, r, 1, name)
    bk.put(ws, r, 2, a, fmt="0.00")
    bk.put(ws, r, 3, b, fmt="0.00")
    bk.put(ws, r, 4, round((b - a) * 60), fmt="0")


# ---- Target -------------------------------------------------------------------
def sheet_target(bk):
    ws = bk.wb.create_sheet("Target")
    P = bk.P
    n_ideal = max(1, ideal_count(P["target_hours"] * 60, P["target_first_gap"], P["target_stretch"]))
    nG = min(500, max(n_ideal, len(bk.mos)))
    n_act = min(len(bk.mos), nG)
    bk.head(ws, 1, TARGET_NAMES, TARGET_WIDTHS)
    for i in range(2, nG + 2):
        target_row(bk, ws, i, n_act)
    add_rule(ws, f"D2:D{nG + 1}", "ColorScaleRule", **RED_SCALE)
    target_chart(ws, nG, n_act)
    bk.note(ws, nG + 3, TARGET_NOTE, 9, 45)


def target_row(bk, ws, i, n_act):
    ideal = ideal_time(i - 1, bk.P["target_first_gap"], bk.P["target_stretch"])
    bk.put(ws, i, 1, i - 1)
    bk.put(ws, i, 2, ideal, fmt="0.0")
    bk.put(ws, i, 5, round(ideal / 60.0, 5), fmt="0.00")
    bk.put(ws, i, 7, TARGET_LIVE.format(i=i), fmt="0.0")
    if i - 2 < n_act:
        target_actual(bk, ws, i, ideal)


def target_actual(bk, ws, i, ideal):
    at = bk.mos[i - 2]["at"]
    bk.put(ws, i, 3, at, fmt="0.0")
    bk.put(ws, i, 4, at - ideal, fmt="0.0")
    bk.put(ws, i, 6, round(at / 60.0, 5), fmt="0.00")
    bk.put(ws, i, 8, f"=C{i}-G{i}", fmt="0.0")
    bk.put(ws, i, 9, f'=IF(G{i}>0,C{i}/G{i},"")', fmt="0.00")


def target_chart(ws, nG, n_act):
    tc = scatter()
    tc.series.append(line(ws, 1, 5, 2, nG + 1, title="Ideal (at build)"))
    if n_act:
        tc.series.append(line(ws, 1, 6, 2, n_act + 1, title="Actual (simulation)"))
    chart_common(tc, "Beats against the target curve (actual to the right of ideal = late)", "Hours played",
                 "Beat number", 24, 11)
    ws.add_chart(tc, "K2")


# ---- Walls --------------------------------------------------------------------
def sheet_walls(bk):
    ws = bk.wb.create_sheet("Walls")
    bk.head(ws, 1, WALLS_NAMES, WALLS_WIDTHS)
    walls, spans = bk.R["walls"], {}
    for i, w in enumerate(walls, 2):
        spans.setdefault(w["ladder"], [i, i])[1] = i
        wall_row(bk, ws, i, w)
    nW = len(walls) + 1
    walls_formats(bk, ws, nW)
    walls_charts(ws, spans)
    bk.note(ws, nW + 2, WALLS_NOTE if walls else NO_LADDER_NOTE, 13, 45)


def wall_requirement(w):
    base, growth, step = w["base"], w["growth"], w["step"]
    try:
        return min(base * growth ** (step - 1), 1e300)
    except OverflowError:
        return 1e300


def wall_row(bk, ws, i, w):
    req = wall_requirement(w)
    wall_reach(bk, ws, i, w, req)
    wall_values(bk, ws, i, w, req)


def wall_reach(bk, ws, i, w, req):
    bk.put(ws, i, 1, w["ladder"], text=True)
    bk.put(ws, i, 2, w["step"])
    bk.put(ws, i, 3, req, fmt="0.00E+00")
    bk.put(ws, i, 4, f"{w['type']} {w['currency'] or ''}".strip(), text=True)
    bk.put(ws, i, 5, None if w["reached_minutes"] is None else round(w["reached_minutes"] / 60.0, 3), fmt="0.00")
    bk.put(ws, i, 6, None if w["wait_minutes"] is None else round(w["wait_minutes"], 2), fmt="0.0")


def wall_values(bk, ws, i, w, req):
    val = fin(w["value"])
    bk.put(ws, i, 7, val, fmt="0.00E+00")
    bk.put(ws, i, 8, "reached" if w["reached"] else "not reached: value at the end")
    bk.put(ws, i, 9, round(math.log10(max(req, 1e-300)), 4), fmt="0.00")
    bk.put(ws, i, 10, round(math.log10(val), 4) if val and val > 0 else None, fmt="0.00")
    bk.put(ws, i, 11, w["base"], bk.INP, bk.YEL)
    bk.put(ws, i, 12, w["growth"], bk.INP, bk.YEL)
    bk.put(ws, i, 13, f"=IFERROR(K{i}*L{i}^(B{i}-1),1E+300)", fmt="0.00E+00")


def walls_formats(bk, ws, nW):
    if not bk.R["walls"]:
        return
    add_rule(ws, f"F2:F{nW}", "ColorScaleRule", **RED_SCALE)
    add_rule(ws, f"A2:J{nW}", "FormulaRule", formula=['LEFT($H2,3)="not"'], fill=bk.GREY)


def walls_charts(ws, spans):
    first = list(spans.items())[:3]
    for k, (lid, (r1, r2)) in enumerate(first):
        wall_chart(ws, k, lid, (r1, r2))


def wall_chart(ws, k, lid, span):
    r1, r2 = span
    ch = scatter()
    ch.series.append(line(ws, 9, 2, r1, r2, title="log10 requirement"))
    ch.series.append(line(ws, 10, 2, r1, r2, title=WALL_VALUE_TITLE))
    chart_common(ch, f"Wall on '{lid}': requirement against the value that the buyer had", "Step", "log10", 24, 10)
    ws.add_chart(ch, f"O{2 + 21 * k}")


# ---- Purchases ----------------------------------------------------------------
def first_currency(p):
    return p["costs"][0]["currency"]


def sheet_purchases(bk):
    ws = bk.wb.create_sheet("Purchases")
    bk.head(ws, 1, PURCHASE_NAMES, PURCHASE_WIDTHS)
    bk.pur = pur = bk.R["purchases"]
    spent_all = spent_totals(pur, bk.m["purchases"])
    for i, x in enumerate(pur, 2):
        purchase_status(bk, ws, i, x)
        purchase_spend(bk, ws, i, x, spent_all)
    bk.note(ws, max(len(pur) + 1, 2) + 2, PURCHASES_NOTE, 14, 45)


def spent_totals(pur, purchases):
    spent_all = {}
    pairs = zip(pur, purchases)
    for x, p in pairs:
        c0 = first_currency(p)
        spent_all[c0] = spent_all.get(c0, 0.0) + x["spent"].get(c0, 0.0)
    return spent_all


def status_fill(bk, st):
    if st.startswith(("never", "affordable")):
        return bk.RED
    return bk.GREY if st.startswith("capped") else None


def purchase_status(bk, ws, i, x):
    bk.put(ws, i, 1, x["name"], bk.B, text=True)
    bk.put(ws, i, 2, x["id"], text=True)
    bk.put(ws, i, 3, x["system"], text=True)
    bk.put(ws, i, 4, x["status"], fill=status_fill(bk, x["status"]))
    bk.put(ws, i, 5, x["first_bought_minutes"], fmt="0.0")
    bk.put(ws, i, 6, x["last_bought_minutes"], fmt="0.0")
    bk.put(ws, i, 7, x["highest_level"])
    bk.put(ws, i, 8, x["levels_bought"])
    bk.put(ws, i, 9, x["cap_at_end"])


def purchase_spend(bk, ws, i, x, spent_all):
    c0 = first_currency(bk.m["purchases"][i - 2])
    spent = x["spent"].get(c0, 0.0)
    bk.put(ws, i, 10, spent, fmt="0.00E+00")
    bk.put(ws, i, 11, c0, text=True)
    bk.put(ws, i, 12, spent / spent_all[c0] if spent_all.get(c0) else 0, fmt="0%")
    bk.put(ws, i, 13, x["payback_first_seconds"], fmt="0")
    bk.put(ws, i, 14, x["payback_last_seconds"], fmt="0")


# ---- Prestige -----------------------------------------------------------------
def sheet_prestige(bk):
    ws = bk.wb.create_sheet("Prestige")
    runs = bk.sim.runs[:3000]
    bk.lad = lad = bk.g.get("progress_axis") or (bk.m["ladders"][0]["id"] if bk.m["ladders"] else None)
    names = PRESTIGE_NAMES + [f"Highest step ({lad or 'no ladder'})", "New beats in the run"]
    bk.head(ws, 1, names, PRESTIGE_WIDTHS)
    for i, x in enumerate(runs, 2):
        prestige_row(bk, ws, i, x)
    nR = len(runs) + 1
    prestige_charts(bk, ws, nR, runs)
    bk.note(ws, nR + 2, PRESTIGE_NOTE.format(bk.primary) if runs else "No prestige happened in the simulation.",
            7, 45)


def prestige_row(bk, ws, i, x):
    bk.put(ws, i, 1, x["run"])
    bk.put(ws, i, 2, round(x["start_minutes"] / 60.0, 3), fmt="0.00")
    bk.put(ws, i, 3, round(x["minutes"], 2), fmt="0.0")
    bk.put(ws, i, 4, x["gain"], fmt="0.00E+00" if x["gain"] >= 1e6 else "0")
    bk.put(ws, i, 5, x["gain_per_minute"], fmt="0.00")
    bk.put(ws, i, 6, x["top_steps"].get(bk.lad) if bk.lad else None)
    bk.put(ws, i, 7, x["new_beats"])


def prestige_charts(bk, ws, nR, runs):
    if not runs:
        return
    add_rule(ws, f"G2:G{nR}", "CellIsRule", operator="equal", formula=["0"], fill=bk.RED)
    charts = [(k, c) for k, c in enumerate(PRESTIGE_CHARTS) if c[0] != 6 or bk.lad]
    for k, spec in charts:
        prestige_chart(ws, k, spec, nR)


def prestige_chart(ws, k, spec, nR):
    col, ttl, ylab = spec
    ch = scatter()
    ch.series.append(line(ws, col, 1, 2, nR, title=ylab))
    chart_common(ch, ttl, "Run number", ylab, 20, 8)
    ch.legend = None
    ws.add_chart(ch, f"I{2 + 18 * k}")


# ---- Ledger -------------------------------------------------------------------
def sheet_ledger(bk):
    ws = bk.wb.create_sheet("Ledger")
    bk.head(ws, 1, ["Currency", "Direction", "Where", "Amount", "Share of the direction"], [14, 11, 34, 14, 12])
    led = bk.R["ledger"]
    totals = ledger_totals(led)
    for i, row in enumerate(led, 2):
        ledger_row(bk, ws, i, row, totals)
    bk.note(ws, max(len(led) + 1, 2) + 2, LEDGER_NOTE, 5, 45)


def ledger_totals(led):
    tot = {}
    rows = led
    for c, d, label, amt in rows:
        tot[(c, d)] = tot.get((c, d), 0.0) + amt
    return tot


def ledger_row(bk, ws, i, row, totals):
    c, d, label, amt = row
    total = totals[(c, d)]
    bk.put(ws, i, 1, c, bk.B, text=True)
    bk.put(ws, i, 2, d)
    bk.put(ws, i, 3, label, text=True)
    bk.put(ws, i, 4, amt, fmt="0.00E+00")
    bk.put(ws, i, 5, amt / total if total > 0 else 0, fmt="0%")


# ---- Sensitivity --------------------------------------------------------------
def sheet_sensitivity(bk):
    ws = bk.wb.create_sheet("Sensitivity")
    sens = bk.R.get("sens")
    (no_sensitivity if sens is None else sens_table)(bk, ws, sens)


def no_sensitivity(bk, ws, sens):
    bk.put(ws, 1, 1, NO_SENS_TEXT, bk.B)
    ws.column_dimensions["A"].width = 90


def sens_table(bk, ws, sens):
    srows, tested, possible = sens
    bk.head(ws, 1, SENS_HEADS, SENS_WIDTHS)
    sens_base_row(bk, ws, bk.R["sens_base"])
    for i, x in enumerate(srows, 3):
        sens_row(bk, ws, i, x)
    nSe = len(srows) + 2
    sens_chart(bk, ws, srows, nSe)
    limit = SENS_LIMIT if tested < possible else "."
    bk.note(ws, nSe + 2, SENS_NOTE.format(primary=bk.primary, tested=tested, possible=possible, limit=limit), 16, 45)


def sens_base_row(bk, ws, base):
    bk.put(ws, 2, 1, "(base run)", bk.B)
    for k in (2, 3, 4):
        bk.put(ws, 2, k, None)
    sens_side(bk, ws, 2, 5, base)
    sens_side(bk, ws, 2, 10, base)
    bk.put(ws, 2, 15, 0)
    bk.put(ws, 2, 16, 0)


def sens_side(bk, ws, i, off, sd):
    keys = enumerate(SENS_KEYS)
    for k, key in keys:
        bk.put(ws, i, off + k, sd[key], fmt="0.0" if "minutes" in key else "0")


def sens_row(bk, ws, i, x):
    bk.put(ws, i, 1, x["parameter"], bk.B, text=True)
    bk.put(ws, i, 2, x["base"], fmt="0.000")
    bk.put(ws, i, 3, x["low_value"], fmt="0.000")
    bk.put(ws, i, 4, x["high_value"], fmt="0.000")
    sens_side(bk, ws, i, 5, x["low"])
    sens_side(bk, ws, i, 10, x["high"])
    bk.put(ws, i, 15, x["change_low"], fmt="0.0")
    bk.put(ws, i, 16, x["change_high"], fmt="0.0")


def sens_chart(bk, ws, srows, nSe):
    from openpyxl.chart import BarChart, Reference
    if not srows:
        return
    sens_formats(bk, ws, nSe)
    sb = BarChart()
    sb.type = "bar"
    top_n = min(len(srows), 15) + 2
    sb.add_data(Reference(ws, min_col=15, max_col=16, min_row=1, max_row=top_n), titles_from_data=True)
    sb.set_categories(Reference(ws, min_col=1, min_row=2, max_row=top_n))
    chart_common(sb, "Change in the longest gap when one number changes by 5%", "Parameter", "Minutes", 24, 12)
    ws.add_chart(sb, f"A{nSe + 5}")


def sens_formats(bk, ws, nSe):
    cols = ("O", "P")
    for col in cols:
        sign_rules(bk, ws, f"{col}3:{col}{nSe}")


def sign_rules(bk, ws, area):
    add_rule(ws, area, "CellIsRule", operator="lessThan", formula=["0"], fill=bk.GRN)
    add_rule(ws, area, "CellIsRule", operator="greaterThan", formula=["0"], fill=bk.RED)


# ---- Compare ------------------------------------------------------------------
def count_lulls(d, key):
    return len(d.get("lulls") or [])


def zone_minutes(d, key):
    return round(sum(z.get("minutes", 0) for z in d.get("dead_zones") or []), 1)


def count_list(d, key):
    return len(d.get(key[1:]) or [])


COMPARE_SPECIAL = {"_lulls": count_lulls, "_zone_minutes": zone_minutes, "_never_bought": count_list,
                   "_never_reached": count_list}


def compare_value(d, key):
    special = COMPARE_SPECIAL.get(key)
    if special:
        return special(d, key)
    v = d.get(key)
    return "never" if v is None and key.startswith("first_") else v


def sheet_compare(bk):
    ws = bk.wb.create_sheet("Compare")
    other = bk.R.get("compare")
    (no_compare if other is None else compare_table)(bk, ws, other)


def no_compare(bk, ws, other):
    bk.put(ws, 1, 1, NO_COMPARE_TEXT, bk.B)
    ws.column_dimensions["A"].width = 110


def compare_table(bk, ws, other):
    bk.head(ws, 1, ["Indicator", "Before", "After", "Delta (at build)", "Delta (live)"], [44, 14, 14, 12, 12])
    for i, row in enumerate(COMPARE_KEYS, 2):
        compare_row(bk, ws, i, row, other)
    text = COMPARE_NOTE.format(other.get("game", "?"), os.path.basename(bk.R["compare_path"]), bk.g["name"])
    bk.note(ws, len(COMPARE_KEYS) + 3, text, 4, 45)
    compare_beats(bk, ws, other)


def is_number(v):
    return isinstance(v, (int, float)) and not isinstance(v, bool)


def compare_row(bk, ws, i, row, other):
    label, key, fmt = row
    vb, va = compare_value(other, key), compare_value(bk.A, key)
    bk.put(ws, i, 1, label, bk.B)
    bk.put(ws, i, 2, vb, fmt=fmt)
    bk.put(ws, i, 3, va, fmt=fmt)
    bk.put(ws, i, 4, va - vb if is_number(vb) and is_number(va) else None, fmt=fmt)
    bk.put(ws, i, 5, f'=IF(AND(ISNUMBER(B{i}),ISNUMBER(C{i})),C{i}-B{i},"")', fmt=fmt)


def compare_beats(bk, ws, other):
    bk.heads(ws, 1, ["Before: hours", "Before: beats", "After: hours", "After: beats"], 7)
    for col in "GHIJ":
        ws.column_dimensions[col].width = 11
    bt = [x for x in (other.get("beat_times_minutes") or []) if isinstance(x, (int, float))]
    compare_series(bk, ws, 7, bt)
    compare_series(bk, ws, 9, [mo["at"] for mo in bk.mos])
    compare_chart(ws, len(bt), len(bk.mos))


def compare_series(bk, ws, col, times):
    rows = enumerate(times, 2)
    for i, x in rows:
        compare_point(bk, ws, i, col, x)


def compare_point(bk, ws, i, col, x):
    bk.put(ws, i, col, round(x / 60.0, 4), fmt="0.00")
    bk.put(ws, i, col + 1, i - 1)


def compare_chart(ws, n_before, n_after):
    cc = scatter()
    if n_before:
        cc.series.append(line(ws, 8, 7, 2, n_before + 1, title="Before"))
    if n_after:
        cc.series.append(line(ws, 10, 9, 2, n_after + 1, title="After"))
    chart_common(cc, "Cumulative beats: before and after (flat parts are lulls)", "Hours played", "Beats", 24, 11)
    ws.add_chart(cc, "L2")


# ---- Income -------------------------------------------------------------------
def sheet_income(bk):
    sm = bk.sim.samples
    sm = sm[::math.ceil(len(sm) / 500)] if len(sm) > 500 else sm
    ws = bk.wb.create_sheet("Income")
    curs = bk.curs
    bk.head(ws, 1, ["Played hours"] + [f"log10 income/s {c}" for c in curs]
            + ["Options affordable now", "Options within 5 min"], [8] + [13] * len(curs) + [12, 12])
    for i, s in enumerate(sm, 2):
        income_row(bk, ws, i, s)
    nI, n = max(len(sm) + 1, 2), len(curs)
    income_chart(ws, range(2, n + 2), nI, INCOME_CHART, f"{col_letter(n + 5)}2")
    income_chart(ws, (n + 2, n + 3), nI, OPTIONS_CHART, f"{col_letter(n + 5)}22")


def income_row(bk, ws, i, s):
    n = len(bk.curs)
    bk.put(ws, i, 1, round(s["t"] / 3600.0, 4), fmt="0.00")
    for k, c in enumerate(bk.curs, 2):
        bk.put(ws, i, k, round(math.log10(s["income"][c] + 1), 4), fmt="0.00")
    bk.put(ws, i, n + 2, s["now"])
    bk.put(ws, i, n + 3, s["soon"])


def income_chart(ws, cols, nI, titles, anchor):
    ch = scatter()
    for k in cols:
        ch.series.append(line(ws, k, 1, 2, nI, head_row=1))
    chart_common(ch, *titles)
    ws.add_chart(ch, anchor)


# ---- Curves -------------------------------------------------------------------
def cell_cost(p, n):
    v = cost_one(p["costs"][0], n)
    return None if v == INF else v


def sheet_curves(bk):
    ws = bk.wb.create_sheet("Curves")
    bk.head(ws, 1, CURVES_NAMES, CURVES_WIDTHS)
    for i, p in enumerate(bk.m["purchases"], 2):
        curve_row(bk, ws, i, p)
    note_r = len(bk.m["purchases"]) + 3
    bk.note(ws, note_r, CURVES_NOTE, 12, 60)
    curve_table(bk, ws, note_r + 2)


def curve_row(bk, ws, i, p):
    x = bk.pur[i - 2]
    curve_model(bk, ws, i, p, x)
    curve_levels(bk, ws, i, p)
    curve_sim(bk, ws, i, p, x)


def other_costs(p):
    return "; ".join(f"{c['currency']}: {c['base']:g}, {c['formula']} {c['growth']:g}" for c in p["costs"][1:])


def effects_text(p):
    text = "; ".join(f"{e['kind']} {e['value']:g} -> {e['target']}" for e in p["effects"]) or "(no effect)"
    cs = p.get("count_steps")
    return text + (f"; count steps x{cs['mult']:g} at each {cs['every']} levels" if cs else "")


def curve_model(bk, ws, i, p, x):
    bk.put(ws, i, 1, p["name"], bk.B, text=True)
    bk.put(ws, i, 2, p["system"], text=True)
    bk.put(ws, i, 3, p["cost_currency"], text=True)
    bk.put(ws, i, 4, p["cost_formula"])
    bk.put(ws, i, 5, p["base_cost"], bk.INP, bk.YEL)
    bk.put(ws, i, 6, p["cost_growth"], bk.INP, bk.YEL)
    bk.put(ws, i, 7, p["max_level"])
    bk.put(ws, i, 8, x["cap_at_end"])
    bk.put(ws, i, 9, other_costs(p), text=True)
    bk.put(ws, i, 10, effects_text(p), text=True)


def curve_levels(bk, ws, i, p):
    levels = enumerate(CURVE_LEVELS)
    for k, n in levels:
        curve_level(bk, ws, i, p, (k, n))


def curve_level(bk, ws, i, p, level):
    k, n = level
    bk.put(ws, i, C_LV + k, cell_cost(p, n), fmt="0.00E+00")
    bk.put(ws, i, C_LIVE + k, CURVE_LIVE.format(i=i, n=n), fmt="0.00E+00")


def curve_sim(bk, ws, i, p, x):
    exp = p["cost_formula"] == "exp" and p["cost_growth"] > 1
    bk.put(ws, i, C0, math.log(10) / math.log(p["cost_growth"]) if exp else None, fmt="0.0")
    bk.put(ws, i, C_LIVE + len(CURVE_LEVELS), f'=IF(AND($D{i}="exp",$F{i}>1),LN(10)/LN($F{i}),"")', fmt="0.0")
    bk.put(ws, i, C0 + 1, x["first_bought_minutes"], fmt="0.0")
    bk.put(ws, i, C0 + 2, x["highest_level"])
    bk.put(ws, i, C0 + 3, x["payback_first_seconds"], fmt="0")
    bk.put(ws, i, C0 + 4, x["payback_last_seconds"], fmt="0")


def curve_table(bk, ws, top):
    shown = bk.m["purchases"][:10]
    bk.put(ws, top, 1, "Level", bk.H, bk.HF)
    for k, p in enumerate(shown, 2):
        bk.put(ws, top, k, p["name"], bk.H, bk.HF, text=True)
    for n in range(0, 51):
        curve_table_row(bk, ws, top + 1 + n, n, shown)
    curves_chart(ws, top, shown)


def curve_table_row(bk, ws, r, n, shown):
    bk.put(ws, r, 1, n)
    for k, p in enumerate(shown, 2):
        curve_point(bk, ws, (r, k), n, p)


def curve_point(bk, ws, at, n, p):
    cap = bk.pur[bk.m["purchases"].index(p)]["cap_at_end"]
    if cap is not None and n > cap:
        return                             # the chart stops at the max level of the line
    v = cell_cost(p, n)
    if v is not None:
        bk.put(ws, at[0], at[1], round(math.log10(max(v, 1e-9)), 4), fmt="0.00")


def curves_chart(ws, top, shown):
    if not shown:
        return
    cc = scatter()
    for k in range(2, len(shown) + 2):
        cc.series.append(line(ws, k, 1, top + 1, top + 51, head_row=top))
    chart_common(cc, CURVES_CHART, "Level", "log10 of cost", 24, 11)
    ws.add_chart(cc, f"{col_letter(len(shown) + 4)}{top}")


# ---- Count steps --------------------------------------------------------------
STEP_NAMES = ["Line", "Id", "Levels in a block (n)", "Step factor (m)", "r_eff = ln m / (n ln b)", "Block",
              "Levels", "Block cost", "Output of the line at the step (each s)", "Block gain (formula)",
              "Step reached (min)", "Block gain (simulation)"]
STEP_WIDTHS = [22, 14, 9, 9, 11, 7, 10, 12, 14, 11, 10, 11]
STEP_BLOCKS = 40                       # the most blocks of 1 line on the sheet
STEP_NOTE = ("1 row for each count block of a line with count steps (model key count_steps): the n levels up to and "
             "with 1 step. At the step the output of the line takes the factor m. Block cost: the sum of the costs "
             "of the n levels. Output: the add effects of the line at the step level, value x level x "
             "m^floor(level / n). Block gain (formula): the output at the step against the output before the first "
             "level of the block, (to / (from - 1)) x m; empty for block 1. The sheet shows the blocks up to the "
             "highest level of the simulation and 1 block more. Step reached and Block gain (simulation) come from "
             "the simulation, primary buyer profile, first run: the income of the target after the step against "
             "the income before the first level of the block. The row PV of the proof sheet tests this gain.")


def step_lines(m):
    return [p for p in m["purchases"] if p.get("count_steps")]


def sheet_count_steps(bk):
    """The sheet of the count blocks; only a model with a line with count steps gets it."""
    lines = step_lines(bk.m)
    if not lines:
        return
    ws = bk.wb.create_sheet("Count steps")
    bk.head(ws, 1, STEP_NAMES, STEP_WIDTHS)
    sim = step_events(bk.sim.events, {p["id"]: p["count_steps"]["every"] for p in lines})
    r = 2
    for p in lines:
        r = step_rows(bk, ws, r, (p, sim.get(p["id"], {})))
    bk.note(ws, r + 1, STEP_NOTE, 12, 75)


def event_level(e):
    tail = e["name"].rsplit(" L", 1)[-1]
    return int(tail) if e["kind"] == "purchase" and tail.isdigit() else None


def step_events(events, every):
    """{line: {block: (minutes, {currency: income before the block}, income after the step)}}, first run only."""
    out, start = {}, {}
    befores = [None] + [e["income"] for e in events]
    for e, before in zip(events, befores):
        step_event(out, start, (e, event_level(e), every.get(e["id"]), before))
    return out


def step_event(out, start, item):
    e, level, n, before = item
    if not (level and n) or before is None:
        return
    start.update({e["id"]: before} if (level - 1) % n == 0 else {})
    if level % n == 0 and e["id"] in start:
        first_step(out.setdefault(e["id"], {}), level // n, (e["real"] / 60.0, start.pop(e["id"]), e["income"]))


def first_step(seen, k, hit):
    seen.setdefault(k, hit)


def line_output(p, level):
    cs = p["count_steps"]
    add = sum(e["value"] for e in p["effects"] if e["kind"] == "add")
    return add * level * cs["mult"] ** (level // cs["every"])


def step_rows(bk, ws, r, item):
    p, sim = item
    n = p["count_steps"]["every"]
    top = bk.pur[bk.m["purchases"].index(p)]["highest_level"] or 0
    for k in range(1, min(top // n + 1, STEP_BLOCKS) + 1):
        step_row(bk, ws, r, (p, k, sim.get(k)))
        r += 1
    return r


def step_target(p):
    return next((e["target"] for e in p["effects"] if e["kind"] == "add"), None)


def step_r_eff(cs, b):
    m, n = cs["mult"], cs["every"]
    return math.log(m) / (n * math.log(b)) if b > 1 and m > 0 else None


def step_block(p, k):
    """(levels text, block cost, output at the step, formula gain) of the block k of the line p."""
    n = p["count_steps"]["every"]
    lo, hi = (k - 1) * n + 1, k * n
    gain = line_output(p, hi) / line_output(p, lo - 1) if k > 1 else None
    return f"{lo} to {hi}", sum(cell_cost(p, x) or 0.0 for x in range(lo - 1, hi)), line_output(p, hi), gain


STEP_FORMATS = (None, None, None, "0.000", "0.0000", None, None, "0.00E+00", "0.00E+00", "0.000")


def step_cells(p, k):
    cs = p["count_steps"]
    return (p["name"], p["id"], cs["every"], cs["mult"], step_r_eff(cs, p["cost_growth"]), k) + step_block(p, k)


def step_row(bk, ws, r, item):
    p, k, hit = item
    for c, (v, fmt) in enumerate(zip(step_cells(p, k), STEP_FORMATS), 1):
        bk.put(ws, r, c, v, bk.B if c == 1 else None, fmt=fmt, text=isinstance(v, str))
    step_sim(bk, ws, r, (step_target(p), hit))


def step_sim(bk, ws, r, item):
    c, hit = item
    if hit is None:
        return
    t, before, after = hit
    bk.put(ws, r, 11, t, fmt="0.0")
    bk.put(ws, r, 12, after.get(c, 0.0) / before[c] if before.get(c, 0.0) > 0 else None, fmt="0.000")


# ---- Systems + Matrix + Links -------------------------------------------------
def sheet_links(bk):
    bk.links = links = derive_links(bk.m)
    ws = bk.wb.create_sheet("Links")
    bk.head(ws, 1, ["From system", "To system", "Type", "Detail"], [18, 18, 10, 70])
    for i, link in enumerate(links, 2):
        link_row(bk, ws, i, link)
    bk.nL = max(len(links) + 1, 2)


def link_row(bk, ws, i, link):
    a, b, t, n = link
    for k, v in enumerate([a, b, t, n], 1):
        bk.put(ws, i, k, v, text=True)


def role(o, i_):
    return "ORPHAN" if o + i_ == 0 else "SINK ONLY" if o == 0 else "SOURCE ONLY" if i_ == 0 else "CONNECTED"


def sheet_systems(bk):
    ws = bk.wb.create_sheet("Systems")
    bk.head(ws, 1, SYSTEMS_NAMES, SYSTEMS_WIDTHS)
    for i, s in enumerate(bk.m["systems"], 2):
        system_build(bk, ws, i, s)
        system_links(bk, ws, i, s["id"])
        system_live(bk, ws, i, s)
    bk.nY = nY = max(len(bk.m["systems"]) + 1, 2)
    add_rule(ws, f"P2:P{nY}", "CellIsRule", operator="equal", formula=['"ORPHAN"'], fill=bk.RED)
    bk.note(ws, nY + 2, SYSTEMS_NOTE, 16)


def system_build(bk, ws, i, s):
    ts = bk.times.get(s["id"], ([], []))[0]
    bk.put(ws, i, 1, s["id"], bk.B, text=True)
    bk.put(ws, i, 2, s["name"], text=True)
    bk.put(ws, i, 3, s["loop"])
    bk.put(ws, i, 4, "yes" if s["persists"] else "no")
    bk.put(ws, i, 5, s["clock"])
    bk.put(ws, i, 6, ts[0] if ts else "never", fmt="0.0")
    bk.put(ws, i, 7, len(ts))


def system_links(bk, ws, i, sid):
    n_out = sum(1 for x in bk.links if x[0] == sid)
    n_in = sum(1 for x in bk.links if x[1] == sid)
    bk.put(ws, i, 8, n_out)
    bk.put(ws, i, 9, n_in)
    bk.put(ws, i, 10, role(n_out, n_in), fill=bk.RED if n_out + n_in == 0 else None)


def system_live(bk, ws, i, s):
    bk.put(ws, i, 11, s.get("description"), text=True)
    fields = dict(i=i, tC=bk.tC, tG=bk.tG, nL=bk.nL)
    for col, tpl, fmt in SYSTEM_LIVE:
        bk.put(ws, i, col, tpl.format(**fields), fmt=fmt)


def sheet_matrix(bk):
    ws = bk.wb.create_sheet("Matrix")
    bk.put(ws, 1, 1, "From \\ To", bk.H, bk.HF)
    ws.column_dimensions["A"].width = 18
    for k, s in enumerate(bk.m["systems"], 2):
        matrix_head(bk, ws, k, s["id"])
    nM = len(bk.m["systems"]) + 1
    matrix_counts(bk, ws, nM)
    ws.row_dimensions[1].height = 80
    if nM >= 2:
        add_rule(ws, f"B2:{col_letter(nM)}{nM}", "ColorScaleRule", **RED_SCALE)
    bk.note(ws, nM + 2, MATRIX_NOTE, max(nM, 6))


def matrix_head(bk, ws, k, sid):
    from openpyxl.styles import Alignment
    bk.put(ws, 1, k, sid, bk.H, bk.HF, text=True).alignment = Alignment(text_rotation=90, vertical="bottom")
    bk.put(ws, k, 1, sid, bk.B, text=True)
    ws.column_dimensions[col_letter(k)].width = 5


def link_count(links, a, b):
    return sum(1 for x in links if x[0] == a and x[1] == b)


def matrix_counts(bk, ws, nM):
    ids = [s["id"] for s in bk.m["systems"]]
    cells = [(r, c) for r in range(2, nM + 1) for c in range(2, nM + 1)]
    for r, c in cells:
        bk.put(ws, r, c, link_count(bk.links, ids[r - 2], ids[c - 2]), fmt="0;-0;;@")


# ---- Ideas --------------------------------------------------------------------
def sheet_ideas(bk):
    ws = bk.wb.create_sheet("Ideas")
    bk.head(ws, 1, IDEA_COLS, IDEA_WIDTHS)
    ideas = [x for x in ((bk.m.get("review") or {}).get("ideas") or []) if isinstance(x, dict)]
    for i, idea in enumerate(ideas, 2):
        idea_row(bk, ws, i, idea)
    if not ideas:
        bk.put(ws, 2, 1, NO_IDEAS_TEXT)
        ws.merge_cells("A2:J2")


def idea_cell(v):
    return v if isinstance(v, (str, int, float)) or v is None else str(v)


def idea_row(bk, ws, i, idea):
    keys = enumerate(IDEA_KEYS, 1)
    for k, key in keys:
        bk.put(ws, i, k, idea_cell(idea.get(key)), text=True)


# ---- Reference ----------------------------------------------------------------
def load_ref(ref_path):
    found = ref_path and os.path.exists(ref_path)
    try:
        return json.load(open(ref_path)) if found else None
    except ValueError:
        return None


def sheet_reference(bk):
    ws = bk.wb.create_sheet("Reference")
    ref = load_ref(bk.ref_path)
    (reference_table if ref else no_reference)(bk, ws, ref)


def no_reference(bk, ws, ref):
    bk.put(ws, 1, 1, NO_REF_TEXT, bk.B)


def reference_table(bk, ws, ref):
    ax = bk.g.get("progress_axis")
    ax_count = next((lad["count"] for lad in bk.m["ladders"] if lad["id"] == ax), None)
    reference_head(bk, ws, ref, ax, ax_count)
    nref = len(ref["system_unlocks"])
    for i, u in enumerate(ref["system_unlocks"], 6):
        reference_unlock(bk, ws, i, u, (ref["axis_max"], nref))
    opened = opened_systems(bk, ax, ax_count)
    nsys = max(len(bk.m["systems"]), 1)
    order = sorted(opened.items(), key=lambda kv: kv[1][0])
    for i, item in enumerate(order, 6):
        reference_game_row(bk, ws, i, item, (ax, nsys))
    reference_chart(ws, nref, len(opened))


def axis_name(ax, ax_count):
    if ax:
        return f"share of the ladder '{ax}' (step at the first event of the system / {ax_count} steps)"
    return "share of simulated play time (the model sets no game.progress_axis)"


def reference_head(bk, ws, ref, ax, ax_count):
    bk.put(ws, 1, 1, ref["title"], bk.T, text=True)
    bk.put(ws, 2, 1, ref["note"], text=True)
    ws.merge_cells("A2:I2")
    ws.row_dimensions[2].height = 45
    text = REF_AXIS_TEXT.format(axis_name(ax, ax_count)) + (REF_AXIS_PROGRESS if ax else REF_AXIS_TIME)
    bk.put(ws, 3, 1, text, bk.B)
    ws.merge_cells("A3:I3")
    ws.row_dimensions[3].height = 45
    bk.head(ws, 5, REF_NAMES, REF_WIDTHS, freeze=False)


def reference_unlock(bk, ws, i, u, scale):
    axis_max, nref = scale
    bk.put(ws, i, 1, u["level"])
    bk.put(ws, i, 2, round(u["level"] / axis_max, 4), fmt="0%")
    bk.put(ws, i, 3, u["system"], text=True)
    bk.put(ws, i, 4, round((i - 5) / nref, 4), fmt="0%")


def opened_systems(bk, ax, ax_count):
    opened = {}
    named = [e for e in bk.sim.events if e["system"] != "-"]
    for e in named:
        open_system(opened, e, ax, ax_count)
    return opened


def open_system(opened, e, ax, ax_count):
    if e["system"] in opened:
        return
    opened[e["system"]] = (e["t"] / 3600.0, (e["axis"] or 0) / ax_count if ax else None)


def reference_game_row(bk, ws, i, item, scale):
    s, (h, share) = item
    bk.put(ws, i, 6, s, text=True)
    bk.put(ws, i, 7, round(h, 3), fmt="0.00")
    bk.put(ws, i, 8, round(share if scale[0] else h * 60 / max(bk.end, 1e-9), 4), fmt="0%")
    bk.put(ws, i, 9, round((i - 5) / scale[1], 4), fmt="0%")


def reference_chart(ws, nref, n_open):
    rc = scatter()
    rc.series.append(xy_series(ws, (4, 6), (2, 6), nref + 5, "Reference game"))
    if n_open:
        rc.series.append(xy_series(ws, (9, 6), (8, 6), n_open + 5, "This game"))
    chart_common(rc, "Share of systems open over the course of the game", "Share of progress",
                 "Share of systems open", 24, 11)
    ws.add_chart(rc, "K5")


# ---- Dashboard ----------------------------------------------------------------
def band(bk, key, lo, hi):
    v = (bk.m.get("bands") or {}).get(key, [lo, hi])
    return (v[0], v[1]) if isinstance(v, list) and len(v) == 2 else (lo, hi)


def nv(x):
    return "never" if x is None else x


def status(v, lo, hi):
    if isinstance(v, bool) or not isinstance(v, (int, float)):
        return "CHECK"
    return "LOW" if v < lo else "HIGH" if v > hi else "OK"


def sheet_dashboard(bk):
    ws = bk.wb.create_sheet("Dashboard", 0)
    dash_header(bk, ws)
    ind = dash_indicators(bk)
    bk.dash_rows = {}
    for i, row in enumerate(ind, 5):
        dash_row(bk, ws, i, row)
    last = 4 + len(ind)
    words = (("OK", bk.GRN), ("LOW", bk.RED), ("HIGH", bk.RED), ("CHECK", bk.YEL))
    for word, fill in words:
        add_rule(ws, f"G5:G{last}", "CellIsRule", operator="equal", formula=[f'"{word}"'], fill=fill)
    r = dash_blocks(bk, ws, last + 2)
    pacing_chart(bk, ws, r)
    dash_presence_rows(bk, ws, r + PACING_CHART_ROWS, words)


def dash_header(bk, ws):
    ws.column_dimensions["A"].width = 44
    for c, w in zip("BCDEFGH", (13, 13, 9, 9, 10, 10, 90)):
        ws.column_dimensions[c].width = w
    bk.put(ws, 1, 1, f"{bk.g['name']}: progression pacing", bk.T, text=True).border = bk.NOB
    bk.put(ws, 2, 1, dash_intro(bk), text=True)
    ws.merge_cells("A2:H2")
    ws.row_dimensions[2].height = 45
    bk.heads(ws, 4, DASH_HEADS)


def dash_away(g, A):
    if not g["away_minutes"]:
        return ""
    return (f" Sessions of {g['session_minutes']:g} min with {g['away_minutes']:g} min away between them "
            f"({A['real_hours']:.1f} h of real time).")


def dash_intro(bk):
    A, n = bk.A, len(bk.profs)
    return (f"Simulated with a greedy buyer for {A['simulated_hours']:.1f} hours of play, buyer profile "
            f"'{bk.primary}' ({n} profile{'s' if n > 1 else ''} on the Profiles sheet). "
            f"{A['events']} events.{dash_away(bk.g, A)} " + DASH_INTRO_TAIL)


def dash_indicators(bk):
    """(name, value at build, live formula or None, low, high, format, how to read it)."""
    ind = (dash_starts(bk) + dash_beats(bk) + dash_gaps(bk) + dash_spread(bk)
           + [dash_target(bk, h) for h in TARGET_HOURS] + dash_robust(bk) + dash_options(bk) + dash_links(bk)
           + dash_misses(bk) + dash_runs(bk))
    # the live cells of 'Last beat' (C13) and 'Tail gap' (C14) are used by the two rows after them: keep the order
    assert ind[8][0].startswith("Last beat") and ind[9][0].startswith("Tail gap")       # rows 13 and 14
    return ind


def dash_starts(bk):
    A, tC, tF = bk.A, bk.tC, bk.tF
    return [
        ("Simulated hours of play", A["simulated_hours"], f"={PR['end']}/60", None, None, "0.0",
         "The span that the other values cover."),
        ("Minutes to first purchase", nv(A["first_purchase_minutes"]), "=" + DASH_NEVER.format(tC, "purchase", tF),
         *band(bk, "first_purchase", 0, 1), "0.0",
         "The first decision. Slow starts lose players before the loop shows itself."),
        ("Minutes to first new system", nv(A["first_system_unlock_minutes"]),
         "=" + DASH_NEVER.format(tC, "system_unlock", bk.tK), *band(bk, "first_system", 2, 15), "0.0",
         "The first proof that the game is larger than its first screen."),
        ("Minutes to first prestige", nv(A["first_prestige_minutes"]), "=" + DASH_NEVER.format(tC, "prestige", tF),
         *band(bk, "first_prestige", 20, 120), "0.0",
         "The meta loop starts here. It depends on the buyer profile: see Profiles. 'never' is correct only for a "
         "game with no reset."),
    ]


def dash_beats(bk):
    A, tN = bk.A, bk.tN
    hours = max(A["simulated_hours"], 1e-9)
    return [
        ("Beats (moments at the beat weight or more)", A["beats"], f"=SUM({tN})", None, None, "0",
         "Count of moments that a player will remember. Stacked beats count one time."),
        ("Stacked beats", A["stacked_beats"], f"=SUM({bk.tM})-SUM({tN})", None, None, "0",
         "Beats that land within the merge seconds of another beat. They are lost as separate moments: spread "
         "them out."),
        ("Beats for each hour", A["beats"] / hours, f"=IFERROR(SUM({tN})/({PR['end']}/60),0)",
         *band(bk, "beats_per_hour", 3, 30), "0.0",
         "Low: long empty stretches. High: unlocks arrive faster than a player can absorb them."),
        ("Beats in the first hour", A["beats_first_hour"], f'=COUNTIFS({tN},1,{bk.tC},"<60")',
         *band(bk, "beats_first_hour", 3, 30), "0",
         "The mean over the run hides a crowded start. Compare this value with the beats for each hour."),
    ]


def dash_gaps(bk):
    A = bk.A
    return [
        ("Last beat at (played min)", A["last_beat_minutes"], f"=SUMPRODUCT(MAX({bk.tC}*{bk.tN}))", None, None,
         "0.0", "The start of the last moment."),
        ("Tail gap (min)", A["tail_gap_minutes"], f"={PR['end']}-C13", None, None, "0.0",
         "From the last beat to the end of the simulation. It counts as a gap."),
        ("Lulls", len(A["lulls"]), DASH_LULLS_LIVE.format(lull=tl_range(bk.nT, "lull")), *band(bk, "lulls", 0, 0),
         "0", "Gaps that fail the Params thresholds, with the tail gap. Each one is on the Beats sheet in red."),
    ]


def dash_spread(bk):
    A, P, nS = bk.A, bk.P, bk.nS
    gap_high = max(P["max_gap_sessions"] * P["session_minutes"], P["late_ratio"] * A["last_beat_minutes"])
    return [
        ("Longest gap between beats (min)", A["longest_gap_minutes"], f"=MAX(MAX({tl_range(bk.nT, 'gap')}),C14)",
         0, round(gap_high, 2), "0.0",
         "The worst wait, with the tail gap. The high band is the lull limit at the last beat (max gap in "
         "sessions, or late lull ratio x time played)."),
        ("Median gap ratio", A["median_gap_ratio"], f"=IFERROR(MEDIAN({tl_range(bk.nT, 'ratio')}),0)",
         *band(bk, "gap_ratio", 0.05, 0.6), "0.00",
         "Gap / time played before it, each moment. A steady value means that waits grow in step with the game. "
         "A rising value means that the game runs out of new things."),
        ("Share of buckets with no beat", A["dry_session_share"], f'=COUNTIF(Sessions!$M$2:$M${nS + 1},"DRY")/{nS}',
         *band(bk, "dry_share", 0, 0.25), "0%",
         "A session with no beat gives no reason to come back. Buckets are on the Sessions sheet."),
    ]


def dash_target(bk, h):
    v = bk.A["beats_behind_target"][str(h)]
    live = DASH_TARGET_LIVE.format(m=60 * h, tC=bk.tC, tN=bk.tN)
    return (f"Beats behind target at hour {h}", "n/a" if v is None else v, live, None, None, "0", DASH_TARGET_READ)


def robust_text(bk):
    n = len(bk.A["lulls"])
    if len(bk.profs) > 1:
        return (f"Of {n} lulls in this run. The other lulls depend on when the buyer resets: see "
                "Lulls and Profiles.")
    return "One buyer profile only."


def zones_text(zones):
    if not zones:
        return "No dead zone."
    return f"{len(zones)} dead zone(s): " + "; ".join(
        f"hour {z['start_minutes'] / 60:.1f} to {z['end_minutes'] / 60:.1f}" for z in zones[:6])


def dash_robust(bk):
    A = bk.A
    rob = sum(1 for x in A["lulls"] if x.get("robust") == "robust")
    return [
        ("Lulls that all buyer profiles show", rob, None, None, None, "0", SIM + robust_text(bk)),
        ("Minutes in dead zones", round(sum(z["minutes"] for z in A["dead_zones"]), 1), None, None, None, "0",
         SIM + zones_text(A["dead_zones"])),
    ]


def dash_options(bk):
    A = bk.A
    return [
        ("Median options within 5 minutes", A["median_options_within_5_min"], None, *band(bk, "options", 2, 7), "0",
         SIM + "Weighted by time. Below 2: no real choice. Above 7: choice overload."),
        ("Share of time with no option within 5 minutes", A["share_of_time_with_no_option_within_5_min"], None,
         *band(bk, "no_option", 0, 0.2), "0%", SIM + "Weighted by time. Pure waiting with nothing to plan toward."),
        ("Systems", A["systems"], f"=COUNTA(Systems!$A$2:$A${bk.nY})", None, None, "0",
         "Scope. Each system needs content, UI, balance, and juice."),
        ("Links between systems", A["links"], f"=COUNTA(Links!$A$2:$A${bk.nL})", None, None, "0",
         "Declared links plus links implied by costs, effects, and requirements."),
    ]


def dash_links(bk):
    A, nL, nY = bk.A, bk.nL, bk.nY
    n_orphan = len(A["orphan_systems"])
    return [
        ("Links for each system", A["links"] / max(A["systems"], 1),
         f"=IFERROR(COUNTA(Links!$A$2:$A${nL})/COUNTA(Systems!$A$2:$A${nY}),0)",
         *band(bk, "links_per_system", 1.5, 8), "0.0",
         "Low: systems sit side by side and do not feed each other. Very high: hard to balance and to explain."),
        ("Orphan systems", n_orphan, f'=COUNTIF(Systems!$P$2:$P${nY},"ORPHAN")', 0, 0, "0",
         (", ".join(A["orphan_systems"][:12]) + ". ") * bool(n_orphan)
         + "Systems with no link in or out. They do not affect the core loop."),
        ("Currencies with no source", len(A["currencies_without_source"]), None, 0, 0, "0",
         ", ".join(A["currencies_without_source"]) or "None."),
        ("Currencies with no sink", len(A["currencies_without_sink"]), None, 0, 0, "0",
         ", ".join(A["currencies_without_sink"]) or "None."),
    ]


def dash_misses(bk):
    A = bk.A
    return [
        ("Purchases never bought in the simulation", len(A["never_bought"]), None, 0, 0, "0",
         (", ".join(A["never_bought"][:12]) + ". The Purchases sheet gives the reason.") if A["never_bought"]
         else "None."),
        ("Milestones never reached in the simulation", len(A["never_reached"]), None, 0, 0, "0",
         ", ".join(A["never_reached"][:12]) or "None."),
    ]


def dash_runs(bk):
    A = bk.A
    return [
        ("Prestiges in the simulation", A["prestige_count"], None, None, None, "0",
         "Run lengths in minutes: " + (", ".join(f"{x:.0f}" for x in A["prestige_run_minutes"][:15]) or "none")
         + ". See the Prestige sheet."),
        ("Prestiges after a run with nothing new", sum(1 for x in bk.sim.runs[1:] if not x["new_beats"]), None,
         None, None, "0",
         SIM + "These resets have weight 1 and are not beats. Many of them = a treadmill: all runs are the same."),
        ("Simulation stalled", "yes" if A["stalled"] else "no", None, None, None, None,
         "; ".join(f"at {s['at_minutes'] / 60:.1f} h: {s['name']} ({s['note']})" for s in A["stalled"])
         or "The buyer always had a goal."),
    ]


def dash_row(bk, ws, i, row):
    bk.dash_rows[row[0]] = i
    dash_values(bk, ws, i, row)
    dash_bands(bk, ws, i, row)
    bk.put(ws, i, 8, row[6], text=True)


def dash_values(bk, ws, i, row):
    name, val, live, fmt = row[0], row[1], row[2], row[5]
    bk.put(ws, i, 1, name, bk.B)
    bk.put(ws, i, 2, val, fmt=fmt)
    bk.put(ws, i, 3, live if live else "(simulation)", fmt=fmt)


def dash_bands(bk, ws, i, row):
    lo, hi = row[3], row[4]
    bk.put(ws, i, 4, lo, bk.INP, bk.YEL if lo is not None else None)
    bk.put(ws, i, 5, hi, bk.INP, bk.YEL if hi is not None else None)
    (dash_status if lo is not None else dash_no_status)(bk, ws, i, row)


def dash_status(bk, ws, i, row):
    st = status(row[1], row[3], row[4])
    bk.put(ws, i, 6, st, fill={"OK": bk.GRN, "CHECK": bk.YEL}.get(st, bk.RED))
    bk.put(ws, i, 7, DASH_STATUS.format(x=f"C{i}" if row[2] else f"B{i}", i=i))


def dash_no_status(bk, ws, i, row):
    bk.put(ws, i, 6, None)
    bk.put(ws, i, 7, None)


def dash_block(bk, ws, r, title, lines):
    bk.put(ws, r, 1, title, bk.H, bk.HF)
    ws.merge_cells(start_row=r, start_column=1, end_row=r, end_column=8)
    for k, f_ in enumerate(lines, 1):
        dash_line(bk, ws, r + k, f_)
    return r + len(lines) + 2


def dash_line(bk, ws, r, f_):
    bk.put(ws, r, 1, f_ if isinstance(f_, str) else str(f_), text=True)
    ws.merge_cells(start_row=r, start_column=1, end_row=r, end_column=8)
    ws.row_dimensions[r].height = 30


def dash_warnings(bk):
    warns = list(bk.R["warnings"])
    if bk.dropped:
        warns.append(DROPPED_WARNING)
    return warns


def dash_blocks(bk, ws, r):
    findings = (bk.m.get("review") or {}).get("findings") or []
    r = dash_block(bk, ws, r, "Review findings", findings or [NO_FINDINGS_TEXT])
    r = dash_block(bk, ws, r, "Warnings from the script", dash_warnings(bk) or ["None."])
    return dash_block(bk, ws, r, "Where to look", WHERE_TO_LOOK)


def lull_series(bk):
    s2 = line(bk.wbt, 19, 18, 2, bk.nBand + 1, title="Lull (script run)")
    s2.graphicalProperties.line.solidFill = "FF0000"
    s2.graphicalProperties.line.width = 19050
    return s2


def pacing_chart(bk, ws, r):
    pc = scatter()
    pc.series.append(line(bk.wt, 22, 4, 2, bk.nT, title="Cumulative beats (at build)"))
    if bk.A["lulls"]:
        pc.series.append(lull_series(bk))
    chart_common(pc, PACING_TITLE, "Hours played", "Beats", 26, 11)
    ws.add_chart(pc, f"A{r}")


# ---- Players and Presence (only when the run has a players block) ------------------
PLAYERS_HEADS = ["Player", "What it does", "Decides each (s)", "Visits each (h)", "Between visits", "Reset rule",
                 "Wall h to 25%", "Wall h to 50%", "Wall h to 75%", "Wall h to end", "Active min to 25%",
                 "Active min to 50%", "Active min to 75%", "Active min to end", "Visits", "Resets", "Axis reached",
                 "Finished"]
PLAYERS_WIDTHS = [15, 50, 9, 9, 10, 22, 9, 9, 9, 9, 9, 9, 9, 9, 8, 8, 9, 9]
PLAYERS_NOTE = ("1 row for each player, from the simulation (pw_players.py). The new players use the payback buyer "
                "(the buyer of the measured game). The version 1 profiles use the buyer of version 1. Wall hours "
                "count real time with the time away. Active minutes count play while the player is present. For a "
                "player who is present all the time, 'Visits' counts the moments with a purchase or a reset.")
QUARTER_KEYS = ("25%", "50%", "75%", "end")
IDLE_HEADS = ["State", "Wait (h)", "Idle value (rho)", "Gain from 1 wait + 1 visit (steps)",
              "Gain of active_greedy in the same time", "Gain ratio"]
BAND_HEADS = ["Indicator", "Value", "Low", "High", "Band from", "Status"]
PRESENCE_NOTE = ("Idle value (rho, law L4): the progress rate of the player who visits each T, divided by the rate "
                 "of active_greedy. Gain: progress steps from 1 wait and 1 visit with no reset, from the state of "
                 "active_greedy at 25% of the progress axis. See references/indicators.md.")
BAND_FILL = {"in band": "GRN", "out of band": "RED", "not run": "YEL"}


def has_players(bk):
    return bk.R.get("players") is not None


def sheet_players(bk):
    if not has_players(bk):
        return
    ws = bk.wb.create_sheet("Players")
    bk.head(ws, 1, PLAYERS_HEADS, PLAYERS_WIDTHS)
    for i, row in enumerate(bk.R["players"], 2):
        player_sheet_row(bk, ws, i, row)
    bk.note(ws, len(bk.R["players"]) + 3, PLAYERS_NOTE, 10, 45)


def player_sheet_row(bk, ws, i, row):
    head = [row["name"], row["text"], row["decide_seconds"], row["visit_hours"], row["between"], row["reset_rule"]]
    times = [row["wall_hours"].get(k) for k in QUARTER_KEYS] + [row["active_minutes"].get(k) for k in QUARTER_KEYS]
    tail = [row["visits"], row["resets"], row["axis_reached"], "yes" if row["finished"] else "no"]
    for k, v in enumerate(head + times + tail, 1):
        bk.put(ws, i, k, v, bk.B if k == 1 else None, fmt="0.00" if 7 <= k <= 14 else None, text=True)


def sheet_presence(bk):
    if not has_players(bk):
        return
    ws = bk.wb.create_sheet("Presence")
    for c, w in zip("ABCDEF", (44, 12, 12, 16, 16, 12)):
        ws.column_dimensions[c].width = w
    pr = bk.R["presence"]
    note = pr.get("left_out_note")
    bk.note(ws, 1, PRESENCE_NOTE + (f" Left out: {note}." if note else ""), 6, 45)
    r = presence_table(bk, ws, 3, "Bands", BAND_HEADS, [band_cells(b) for b in pr["bands"]], band_fill)
    r = presence_table(bk, ws, r, "Idle curve", IDLE_HEADS, idle_cells(pr["idle_curve"]))
    r = presence_table(bk, ws, r, "Growth slope of active_greedy for each hour",
                       ["Hour", "log10 income", "Change of log10 income", "Axis", "Change of axis"], growth_cells(pr))
    r = presence_table(bk, ws, r, "Policy gap", ["Indicator", "Value"], gap_cells(pr["policy_gap"]))
    r = presence_table(bk, ws, r, "Speed test (law L9: a human decides each 2 real seconds)",
                       ["Speed", "Decision gap (game s)", "Slow-down"], speed_cells(pr["speed_test"]))
    presence_table(bk, ws, r, "Lines and currencies", ["List", "Ids"], line_cells(pr))


def presence_table(bk, ws, r, title, heads, rows, fill=None):
    bk.put(ws, r, 1, title, bk.H, bk.HF)
    ws.merge_cells(start_row=r, start_column=1, end_row=r, end_column=6)
    bk.heads(ws, r + 1, heads)
    for i, cells in enumerate(rows or [["none"]], r + 2):
        presence_row(bk, ws, i, cells, fill)
    return r + 3 + len(rows or [1])


def presence_row(bk, ws, i, cells, fill):
    colour = fill(bk, cells) if fill else None
    for k, v in enumerate(cells, 1):
        bk.put(ws, i, k, v, fill=colour if k == len(cells) else None, text=True)


def band_cells(b):
    return [b["indicator"], b["value"], b["low"], b["high"], b["source"], b["status"]]


def band_fill(bk, cells):
    name = BAND_FILL.get(cells[-1])
    return getattr(bk, name) if name else None


def idle_cells(curve):
    if not curve:
        return []
    return [[state, int(h), p["value"], p["gain"], p["active_gain"], p["gain_ratio"]]
            for state in ("open", "closed") if state in curve for h, p in curve[state].items()]


def growth_cells(pr):
    gs = pr["growth_slope"]
    return [[x["hour"], x["log10_income"], x["d_log10_income"], x["axis"], x["d_axis"]] for x in gs["rows"]] if gs else []


def gap_cells(pg):
    if not pg:
        return []
    return [["early_reset / patient_reset time", pg["value"]], ["early_reset hours", pg["early_hours"]],
            ["patient_reset hours", pg["patient_hours"]], ["common axis step", pg["axis_step"]],
            ["early_reset gains", ", ".join(f"{g:.4g}" for g in pg["early_gains"][:20])],
            ["patient_reset gains", ", ".join(f"{g:.4g}" for g in pg["patient_gains"][:20])]]


def speed_cells(sp):
    if not sp:
        return []
    rows = [[f"x{x['speed']:g}", x["decision_game_seconds"], x["slowdown"]] for x in sp["rows"]]
    return rows + [["largest speed with a slow-down under 1.1", sp["largest_speed"], None]]


def line_cells(pr):
    nb, ub = pr["never_bought"], pr["unbounded"]
    return [["lines that no player buys", ", ".join(nb["no_player"]) or "none"],
            ["lines that only 1 player buys", ", ".join(f"{x['id']} ({x['player']})" for x in nb["one_player"]) or "none"],
            ["lines with r >= 1 and no cap", ", ".join(f"{x['id']} (r {x['r']:g})" for x in ub["lines"]) or "none"],
            ["currencies whose only sinks have no cap", ", ".join(ub["currencies"]) or "none"]]


def dash_presence(bk):
    """1 Dashboard row for each band with a status. LOW or HIGH marks a band that failed."""
    if not has_players(bk):
        return []
    return [dash_band(b) for b in bk.R["presence"]["bands"] if b["status"] in ("in band", "out of band")]


PACING_CHART_ROWS = 24      # the pacing chart is 11 cm high: about 21 rows of the default height
PRESENCE_TITLE = "Presence bands (from the players: see the Players and Presence sheets)"


def dash_presence_rows(bk, ws, r, words):
    """The band rows of the players, under the pacing chart, so that each row of version 1 keeps its cell."""
    rows = dash_presence(bk)
    if not rows:
        return
    bk.put(ws, r, 1, PRESENCE_TITLE, bk.B, text=True)
    bk.heads(ws, r + 1, DASH_HEADS)
    for i, row in enumerate(rows, r + 2):
        dash_row(bk, ws, i, row)
    for word, fill in words:
        add_rule(ws, f"G{r + 2}:G{r + 1 + len(rows)}", "CellIsRule", operator="equal", formula=[f'"{word}"'], fill=fill)


def dash_band(b):
    numeric = isinstance(b["value"], (int, float))
    value = b["value"] if numeric else (1 if b["status"] == "in band" else 0)
    read = f"{b['status']} ({b['source']} band). Value: {b['value']}. See the Presence sheet."
    return (b["indicator"], value, None, b["low"], b["high"], "0.000", read)


# ---- Workbook -----------------------------------------------------------------
SHEET_STEPS = (sheet_params, sheet_timeline, sheet_sessions, sheet_beats, sheet_lulls, sheet_swimlane,
               sheet_profiles, sheet_target, sheet_walls, sheet_purchases, sheet_prestige, sheet_ledger,
               sheet_sensitivity, sheet_compare, sheet_income, sheet_curves, sheet_count_steps, sheet_links,
               sheet_systems,
               sheet_matrix, sheet_ideas, sheet_reference, sheet_players, sheet_presence, sheet_dashboard)


def save(wb, out):
    names = [x for n in ORDER for x in ([n, "Count steps"] if n == "Curves" else [n]) if x in wb.sheetnames]
    wb._sheets = [wb[n] for n in names + [n for n in ("Players", "Presence") if n in wb.sheetnames]]
    wb.active = 0
    try:
        wb.save(out)
    except OSError as e:
        raise SystemExit(f"cannot write '{out}': {e.strerror}")


def write_workbook(m, R, out, ref_path):
    bk = Book(m, R, ref_path)
    steps = SHEET_STEPS
    for step in steps:
        step(bk)
    save(bk.wb, out)
    return dict(timeline_rows=bk.nT - 1, timeline_sampled=bk.dropped, swim_blocks=bk.swim_blocks,
                dash_rows=bk.dash_rows)
