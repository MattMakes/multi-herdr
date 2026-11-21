"""The timing fit by simulation of the rebalance mode (references/design-solver.md, "The timing fit").

The effects of the content (each step, value, cap and cost ratio) are fixed before this step. The fit changes only the
numbers that set the time of each thing and never the character of an element:

1. the wall of each stage of the spine: the value of the spine stat that active_greedy has at the target time of
   that stage;
2. the unlock point of each placed element: the lifetime total that the player has at its placed time, and the
   count of resets that the player has at the target time of a game reset count;
3. the first cost of each placed line: 20 s of the income at its unlock (pw_rebalance.freeze);
4. the reset: the divisor c, so that the median later run is the run length T_X, and the first-reset gain, so that
   the first reset comes at T_X.

Each pass plays active_greedy 1 time and sets each number from that run. The passes stop when each stage, the first
reset and the median later run are inside TIME_TOL of their target times, or after PASSES passes; the fit keeps the
pass that is nearest to the timeline. Reason: a law gives each of these numbers only inside its domain, and the
content factors of a game (large constant factors, unlocks at a count of resets) put the game outside it. The
simulation gives the time of each number directly.
"""

import copy
import math
import statistics

import pw_players
import pw_rebalance as rb

HOUR = 3600.0
PASSES = 12              # the most passes of the fit (each pass plays active_greedy 1 time)
TIME_TOL = 0.10          # a time inside 10% of its target agrees
STEP_MAX = 12.0          # the largest change of ln c in 1 pass
SLOPE_MIN = 0.02         # the smallest slope d ln T / d ln c of a secant step
RUN_TOL = 0.075          # the median later run aims at 92.5% of T_X, inside 7.5% of T_X (PO5: 20% of sigma)
FLOOR_TOL = 0.05         # a median run within 5% of the run floor ends at the run floor
OPEN_STAGES = 100000     # the stage count of the ladder of the fit runs
PASSES_N = 3             # the most runs that set the walls while N changes
GAIN_DIGITS = "%.3g"
STAGE_SHARE = 0.8        # a stage after the first hour comes each 80% of the unlock gap target at most (pw_design)
REPAIR_ROUNDS = 0        # the rounds of the content repair after the fit (each runs the passes again). Off: in
                         # Moss Works a repair moved elements of large effect, the c fit then failed PO5 or PC5.


class Timeline:
    """The target timeline of the targets file, in seconds: the first purchase, the stages of the first hour, the first
    reset, the run length after it, 1 stage in each later run, the demo end and the hard end."""

    def __init__(self, first_min, tv, run_s):
        self.first = [60.0 * t for t in first_min if t < 60.0]
        self.first_purchase = 60.0 * tv["pfirst"]
        self.first_reset = run_s
        self.run = run_s
        self.run_aim = (1.0 - RUN_TOL) * run_s
        self.demo = 60.0 * tv["demo_min"] if tv.get("demo_min") else None
        self.stage_gap = STAGE_SHARE * 60.0 * tv["ugap_max"]
        self.hard_end = 3600.0 * tv["H"]
        self.last_slot = self.hard_end - 60.0 * tv["ugap_max"]

    def rows(self):
        """(what, minutes) of each row of the report table "Target timeline"."""
        rows = [("first purchase", self.first_purchase / 60.0)]
        rows += [(f"stage {i + 1}", t / 60.0) for i, t in enumerate(self.first)]
        rows += [("first reset", self.first_reset / 60.0), ("run length after the first reset", self.run / 60.0),
                 ("stages after the first hour: 1 in each run", self.run / 60.0)]
        if self.demo:
            rows.append(("demo end", self.demo / 60.0))
        return rows + [("hard end", self.hard_end / 60.0)]


class StageSim(rb.LifeSim):
    """active_greedy that takes each stage by the timeline, not by a wall: a stage of the first hour at the first new
    high of the spine stat at or after its target time, and after the first hour 1 stage in each run, at the first
    new high of that run. A new high is a value at or above each value before it. It records the value at each stage:
    with these values as the walls, the model plays the same run, because a wall equal to a new high is first passed
    at that new high, except while the stat stays at 1 value: the model then takes those stages together (the proof
    measures the model). Reason: with "above" only, the second income of Kiln n Kin, which each town multiplies by
    0.16 to 0.92, gave 1 stage."""

    def __init__(self, m, hours, prof, player, fit):
        self.fit_first, self.fit_cur, self.fit_end, self.fit_gap = fit
        self.fit_top, self.fit_run_done, self.fit_last = 0.0, -1, 0.0
        super().__init__(m, hours, prof, player=player)

    def _next_step_req(self, l, inc):
        if l["id"] != "stage":
            return super()._next_step_req(l, inc)
        n = self.ladder_pos["stage"] + 1
        if n <= self.ladder_best["stage"]:
            return self.climb_again(l, n, inc)
        v = self.quantity(l["requires"], inc)[0] or 0.0
        high = v > 0 and v >= self.fit_top
        self.fit_top = max(self.fit_top, v)
        return self.ladder_req(l, n) if high and self.fit_due(n) else None

    def climb_again(self, l, n, inc):
        """A reset that clears the stage (the rule of the game): the player passes each wall that the fit took
        before, at its recorded value, again."""
        req = dict(l["requires"], amount=self.steps["stage"][n][1])
        return req if self.met(req, inc)[0] else None

    def fit_due(self, n):
        """A stage of the first hour at its target time; after the first hour, 1 stage at the first new high of each
        run, or 1 more stage when the last stage is 1 stage gap (STAGE_SHARE x the unlock gap target) old."""
        if n <= len(self.fit_first):
            return self.real >= self.fit_first[n - 1] - 1e-6
        new_run = self.prestiges > self.fit_run_done
        if self.real > self.fit_end + 1e-6 or not (new_run or self.real - self.fit_last >= self.fit_gap - 1e-6):
            return False
        self.fit_run_done, self.fit_last = self.prestiges, self.real
        return True


class Run:
    """The times of 1 run that the fit compares with the timeline, and the value of the spine stat at each stage.
    later is the median length of the runs after the first reset that start before the hard end. Reason: after the
    hard end the runs are short (the content is spent), and over the whole horizon they set the median."""

    def __init__(self, sim, floor, end):
        self.sim = sim
        self.steps = {n: v[0] for n, v in sim.step_clock["stage"].items()}
        self.values = [sim.steps["stage"][n][1] for n in sorted(sim.steps["stage"])]
        self.resets = [e["real"] for e in sim.events if e["kind"] == "prestige"]
        self.later = later_run(sim.runs, end)
        self.first_reset = self.resets[0] if self.resets else None
        self.at_floor = at_floor(self.later, floor)

    def stage_misses(self, tl):
        """The count of stages of the first hour whose time is not inside TIME_TOL of its target."""
        return sum(not agrees(self.steps.get(n + 1), t) for n, t in enumerate(tl.first))

    def score(self, tl):
        """(resets off, stages off, the log distance of the run length): the smaller is nearer to the timeline. The
        resets come first: the run length sets each stage after the first hour and the hard end."""
        off = int(not agrees(self.first_reset, tl.first_reset)) + int(not run_ok(self.later, tl))
        dist = abs(math.log(self.later / tl.run_aim)) if self.later else math.inf
        return off, self.stage_misses(tl), dist


def later_run(runs, end):
    """The median length of the runs after the first that start before the end, or None."""
    secs = [r["minutes"] * 60.0 for r in runs[1:] if r["start_minutes"] * 60.0 < end]
    return statistics.median(secs) if secs else None


def at_floor(later, floor):
    return bool(later and floor and later <= floor * (1.0 + FLOOR_TOL))


def agrees(t, target):
    return t is not None and abs(t - target) <= TIME_TOL * target


def run_ok(t, tl):
    """The median later run is inside [1 - 2 RUN_TOL, 1] x T_X: at most T_X, because a stage comes 1 time in each run
    and the proof row PO10 needs the median gap between stages at or below sigma; at least 0.85 T_X, inside the lower
    bound 0.8 of PO5 (PO5 measures early_reset, whose runs differ a little)."""
    return t is not None and abs(t - tl.run_aim) <= RUN_TOL * tl.run + 1e-9


class Fit:
    """The state of the fit: the numbers it sets, the history of c, and the best pass."""

    def __init__(self, d, ctx):
        self.d, self.ctx = d, ctx
        self.plan = d.plan
        self.hist = []
        self.c0 = getattr(d, "c0", None) or d.params["c"]["value"]
        d.c0 = self.c0
        self.best = None
        self.passes = []

    def numbers(self):
        """A copy of each number that the fit sets (the walls come from the run of these numbers)."""
        d, plan = self.d, self.plan
        nums = copy.deepcopy(dict(amounts=plan.amounts, costs=plan.costs, resets=plan.reset_map,
                                  c=d.params["c"]["value"], gain=d.fit_gain, seed=getattr(plan, "seed_scale", 1.0)))
        return dict(nums, curve=plan.curve)

    def restore(self, nums):
        d, plan = self.d, self.plan
        plan.amounts, plan.costs, plan.reset_map = nums["amounts"], nums["costs"], nums["resets"]
        d.params["c"]["value"], d.fit_gain, plan.seed_scale = nums["c"], nums["gain"], nums["seed"]
        plan.curve = nums["curve"]


def fit(d, ctx):
    """The passes of the fit. ctx: build (the model dict of the design now), load (the model with the defaults),
    gen_cost (the first cost of a placed line at least), spine (the currency of the stages), slope0 and p. Each pass
    plays StageSim 1 time. Then the fit restores the best pass, plays it again, and sets the walls and N from it.
    After that, the content repair moves elements into the content gaps of that run, and the passes run again
    (REPAIR_ROUNDS times at most). Returns the run of the final model (with the walls) and the record of the
    passes."""
    tl = d.timeline
    f = Fit(d, ctx)
    d.fit_gain = getattr(d, "fit_gain", None)
    f.plan.reset_map = getattr(f.plan, "reset_map", None) or {}
    run = fit_passes(f, tl)
    for _ in range(REPAIR_ROUNDS):
        moved = rb.content_slots(f.plan, run.sim, d.tv, tl.hard_end)
        if not moved:
            break
        run = repair_round(f, tl, run, moved)
    return run, f.passes


def repair_round(f, tl, run, moved):
    """1 round of the content repair: the moved elements get the totals of the run at their new times, and the
    passes run again."""
    f.plan.times.update(moved)
    f.plan.curve = rb.Curve(run.sim)
    rb.freeze_some(f.plan, f.ctx["gen_cost"], moved)
    f.best = None
    return fit_passes(f, tl)


def fit_passes(f, tl):
    """The passes, then the walls and N of the best pass, and the run of the final model."""
    run_passes(f, tl)
    f.restore(f.best[1])
    settle_walls(f)
    run = final_run(f.d, f.ctx)
    if reach_resets(f.plan, run, tl):
        settle_walls(f)
        run = final_run(f.d, f.ctx)
    return run


def run_passes(f, tl, left=PASSES):
    """The passes of the fit, until the resets and the stages of the first hour agree, at most PASSES."""
    run = play(f)
    f.passes.append(run.score(tl))
    keep_best(f, run)
    if left > 1 and f.passes[-1][:2] != (0, 0):
        step(f, run, not run_ok(run.later, tl))
        run_passes(f, tl, left - 1)


def final_run(d, ctx):
    m = ctx["load"](ctx["build"]())
    return Run(rb.run_active(m, d.tv["H"] * 1.5), m["prestige"].get("min_run_seconds"), d.timeline.hard_end)


def reach_resets(plan, run, tl):
    """Lower each design count of resets that the final run does not reach by the last slot: the top count becomes
    the count of the run there, and the counts under it keep their order (at least 1 apart). True when a count
    moved. Reason: the runs of Kiln n Kin grew between 2 towns, so the last 3 towns did not come (PC5)."""
    rmap = plan.reset_map or {}
    have = run_count(run.resets, tl.last_slot)
    if not rmap or max(rmap.values()) <= have:
        return False
    keys = sorted(rmap)
    for i, k in enumerate(reversed(keys)):
        rmap[k] = min(rmap[k], max(1, have - i))
    return True


def keep_best(f, run):
    key = run.score(f.d.timeline)
    if f.best is None or key < f.best[0]:
        f.best = (key, f.numbers())


def play(f):
    """1 run of StageSim on the design now (the ladder has no end, so the run goes on to the horizon)."""
    d, tl = f.d, f.d.timeline
    m = f.ctx["load"](open_end(f.ctx["build"]()))
    spec = next(p for p in pw_players.PLAYERS if p["name"] == "active_greedy")
    pl = pw_players.Players(m, d.tv["H"] * 1.5)
    prof = pw_players.reset_profile(m, spec["reset"])
    sim = StageSim(m, pl.horizon(spec), prof, pl.player(spec), (tl.first, f.ctx["spine"], tl.hard_end,
                                                                tl.stage_gap)).play()
    return Run(sim, m["prestige"].get("min_run_seconds"), tl.hard_end)


def open_end(m):
    """The model with a ladder of many stages (StageSim takes them by the timeline, not by their walls)."""
    lad = m["ladders"][0]
    lad["count"] = OPEN_STAGES
    lad["requires"] = dict(lad["requires"], table=None, base=1e300)
    lad["requires"].pop("table")
    return m


def settle_walls(f, left=PASSES_N):
    """The walls and N from a run of StageSim, again while N changes (at most PASSES_N runs): a stage unlock of the
    content is scaled to N, so a new N moves those unlocks."""
    n_old = f.d.fit_N
    set_walls(f.d, play(f))
    if left > 1 and f.d.fit_N != n_old:
        settle_walls(f, left - 1)


def set_walls(d, run):
    """The walls and N from a run of StageSim: the value of the spine stat at each stage up to the hard end."""
    n = stages_by(run, d.timeline.hard_end)
    d.table = [float(v) for v in run.values[:n]]
    d.fit_N = n


def stages_by(run, t):
    return max([k for k, v in run.steps.items() if v <= t + 1e-6], default=1)


def step(f, run, move_c):
    """Set each number from the run: the unlock points, the costs, the reset counts, then c (while the median later
    run is outside TIME_TOL of T_X) and the first-reset gain. The fit keeps the pass that is nearest to the
    timeline."""
    plan, tl = f.plan, f.d.timeline
    plan.curve = rb.Curve(run.sim)
    f.d.fit_N = stages_by(run, tl.hard_end)
    rb.freeze(plan, f.ctx["gen_cost"])
    rb.reach_fix(plan, run.sim, tl.hard_end)
    plan.reset_map = reset_map(plan, tl, run.resets)
    if move_c:
        f.hist.append((math.log(f.d.params["c"]["value"]), math.log(run.later) if run.later else None, run.at_floor))
        f.d.params["c"]["value"] = next_c(f)
        plan.seed_scale = seed_scale(f)
    f.d.fit_gain = first_gain(f, tl.first_reset)


def game_counts(plan):
    """The counts of resets that open an element in the game, from low to high."""
    counts = {(x.e.get("unlock") or {}).get("resets") for x in plan.fitted()}
    return sorted(c for c in counts if c)


def reset_map(plan, tl, resets=None):
    """{game count: design count}. The game counts keep their spacing: count k goes to the time
    first reset + (last slot - first reset) (k - 1) / (k max - 1), and the design count is the count of resets of the
    target timeline at that time (1 at the first reset and 1 more for each run length T_X), or the count of resets of
    the run at that time when it is smaller, and at least 1 more than the count before. Reason: in the game each reset opens its factors, and the design has more resets than the game;
    kept as counts, the factors of the trial Kiln n Kin (about 1e7) came in the first 8 resets, and each later run
    ended at the run floor. The count of the run is an upper bound, so that the player reaches each count (PC5):
    with the timeline count only, the runs of Kiln n Kin grew between 2 towns, and the last 3 towns did not come."""
    counts, out, prev = game_counts(plan), {}, 0
    if rb.keeps(plan, "reset_count"):
        return {k: k for k in counts}         # the count of resets of the game stays (structure, ticket 47)
    top = max(counts, default=1)
    for k in counts:
        t = tl.first_reset + (tl.last_slot - tl.first_reset) * (k - 1) / max(1, top - 1)
        prev = max(prev + 1, min(1 + int((t - tl.first_reset) / tl.run), run_count(resets, t)))
        out[k] = prev
    return out


def run_count(resets, t):
    """The count of resets of the run up to the time t (no bound when the run is not known)."""
    return OPEN_STAGES if resets is None else sum(1 for r in resets if r <= t + 1e-6)


def next_c(f):
    """The next c, from the history of (ln c, ln T, at the floor) of the passes. With a run above and a run below
    T_X (a run at the run floor counts as below), the middle of the nearest 2 in ln c (bisection). Else 1 step toward
    T_X: the secant of ln T on ln c with the slope p q / alpha of L6 (the reward (1 + L)^q), at most STEP_MAX. Reason:
    with the reset rule G >= X L, c changes each gain and L by the same factor, so c moves the run length only
    through the reward; the run length then jumps over T_X at a narrow band of c, and the secant went back and forth
    over it in the trial Kiln n Kin."""
    target = math.log(f.d.timeline.run_aim)
    pts = [h for h in f.hist if h[1] is not None]
    if not pts:
        return f.d.params["c"]["value"]
    mid = bisect_c(pts, target)
    return math.exp(mid if mid is not None else secant_c(pts[-1], target, f.ctx["slope0"]))


def bisect_c(pts, target):
    """The middle in ln c of the nearest pass below and the nearest pass above the target, or None."""
    below = [h[0] for h in pts if h[2] or h[1] < target]
    above = [h[0] for h in pts if not h[2] and h[1] > target]
    if not (below and above):
        return None
    lo, hi = max(below), min(above)
    return 0.5 * (lo + hi) if lo < hi else pts[-1][0]


def secant_c(last, target, slope0):
    """ln c after 1 step from the last pass toward the target, at most STEP_MAX."""
    ln_c, ln_t, floor = last
    delta = STEP_MAX if floor else (target - ln_t) / max(SLOPE_MIN, slope0)
    return ln_c + max(-STEP_MAX, min(STEP_MAX, delta))


def seed_scale(f):
    """The factor of the first cost of each line of the reset currency: (c at the start of the fit / c)^p. A gain is
    k (E / c)^p, so the costs follow the gains and each such line comes at the same point of the game."""
    return (f.c0 / f.d.params["c"]["value"]) ** f.ctx["p"]


def first_gain(f, t):
    """The gain of the first run at the first-reset target t: active_greedy plays with no reset up to t, and the
    gain is k (E / c)^p of its run total E. The first reset then opens at t."""
    m = f.ctx["build"]()
    m["prestige"]["min_gain"] = 1e300
    sim = rb.run_active(f.ctx["load"](m), t / HOUR * 1.05)
    pr = m["prestige"]
    E = rb.Curve(sim).sample(t)["life"].get(pr["basis_currency"], 0.0)
    gain = pr.get("k", 1) * (E / pr["base"]) ** pr["exponent"] if E > 0 else None
    return float(GAIN_DIGITS % gain) if gain else None


def report_lines(d):
    """The section "Target timeline" of the report: the target of each row, and the time of active_greedy in the last
    run of the fit."""
    tl, rec = d.timeline, d.fit_record
    sim = rec["rows"]
    lines = ["## Target timeline", "", "The timing fit (references/design-solver.md, \"The timing fit\") sets the walls, "
             "the unlock points, the first costs and the reset by simulation, so that active_greedy meets this "
             "timeline. The effects of the content do not change.", "",
             "| What | Target (min) | active_greedy (min) |", "| --- | --- | --- |"]
    lines += [f"| {what} | {t:.1f} | {fmt_min(sim, what)} |" for what, t in tl.rows()]
    return lines + ["", f"The row of the stages after the first hour gives the median gap between 2 of them. The hard end "
                    f"is stage {rec['N']}. Passes: {len(rec['passes'])}. Stages of the first hour inside "
                    f"{TIME_TOL:.0%} of their target time: {len(tl.first) - rec['misses']} of {len(tl.first)}.", ""]


def fmt_min(sim, what):
    if what not in sim:
        return "-"
    return "not reached" if sim[what] is None else f"{sim[what] / 60.0:.1f}"


def record(d, run, passes):
    """The times of the run of the final model, for the report and the tests."""
    tl, n = d.timeline, d.fit_N
    nf = len(tl.first)
    rows = {"first reset": run.first_reset, "run length after the first reset": run.later, "hard end": run.steps.get(n),
            "stages after the first hour: 1 in each run": later_gap(run.steps, nf, n),
            "first purchase": next((e["real"] for e in run.sim.events if e["kind"] == "purchase"), None)}
    rows.update({f"stage {k}": run.steps.get(k) for k in range(1, nf + 1)})
    d.fit_record = dict(rows=rows, passes=passes, misses=run.stage_misses(tl), steps=run.steps, N=n)
    d.plan.fit_resets = run_count(run.resets, run.steps.get(n) or tl.hard_end)


def later_gap(steps, nf, n):
    """The median gap between 2 stages after the stages of the first hour, up to stage n, or None."""
    gaps = [steps[k + 1] - steps[k] for k in range(nf + 1, n) if k in steps and k + 1 in steps]
    return statistics.median(gaps) if gaps else None
