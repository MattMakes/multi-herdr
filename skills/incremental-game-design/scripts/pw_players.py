"""The players of the simulator, and the indicators that compare them.

Design note (ticket 02, step 1). Each capability is 1 small change to pw_sim.Sim. Each new model field is
optional, and its default gives the version 1 run.

1. Decision interval. Sim(player={"decide": D}) moves each purchase to the next multiple of D seconds.
   No player (D = 0) is the continuous buyer of version 1.
2. Visits. A player with "visit_every" plays visits of "visit_seconds" and is away between them. Sim
   reuses the session loop of version 1: the visit is the session, and the away block is open or closed.
   Away time runs in event steps (Sim._away_span). Income is constant between 2 events, so each step is
   exact.
3. Active-only income. An effect with "active": true is off while the player is away. Sim keeps a second
   source list with no active-only effect (away_sources). A requirement still compares the full income,
   so a stat such as power does not fall while the player is away.
4. Open and closed. game.presence.open_rate and open_cap_minutes set the open state. game.offline_rate
   and game.presence.offline_cap_minutes set the closed state. Closed time stops the game clock.
5. Bank. A currency with bank_cap puts its away income in a bank that stops at the cap. A visit adds the
   bank to the balance.
6. Spine step with a time cost. A ladder with attempt_seconds, cooldown_seconds or needs_visit takes a met
   step only when its timers let it. With needs_visit, it takes a step only while the player is present.
7. Reset that needs a visit. prestige.needs_visit (default true) is the rule of version 1: a reset happens
   only in a session. With false, the reset rule also runs while the player is away.
8. Clock. A source or a ladder with "clock": "real" (or in a system with that clock) does not follow the
   game speed. Sim divides its add effects by the speed and multiplies its timers by the speed.
9. Reset rules. A player resets by the rule G >= X * L (the version 1 policy "ratio" with policy_ratio X)
   or by a version 1 profile. Sim needs no new code for this.

3 more optional fields serve worked example 2 of references/balance-math.md: prestige.floor (false: the
gain is not rounded down), prestige.reward (income x (1 + L)^exponent), and a table of amounts for the
first steps of a ladder requirement.
"""
import copy
import itertools
import math
import statistics

from pw_analysis import main_currency
from pw_model import IDLE_STATES, PROFILES, presence_of, prestige_on, primary_name, profiles_for
from pw_sim import Sim

HOUR = 3600.0
IDLE_HOURS = (1, 8, 24, 168)
QUARTERS = (("25%", 0.25), ("50%", 0.5), ("75%", 0.75), ("end", 1.0))
HUMAN_DECISION = 2.0                    # seconds of real time between 2 decisions of a human (law L9)
SPEEDS_ALL = (2, 5, 10, 20, 50, 100, 200, 500, 1000)
SPEEDS_FAST = (10, 100)
SLOWDOWN_MAX = 1.1
PURCHASE_RATE_MAX = 30                  # purchases by hand in 1 minute (designer target; the default of row PC6)
DEFAULT_PLAY_TEST_SPEED = 10
VISIT_HORIZON = 40                      # a player with visits plays at most this many visit intervals
GROWTH_ROWS = 48
ROUND_SECONDS = 1.0                     # the length of 1 purchase round of a round player
ROUND_MINUTES = (1, 5)                  # the players that buy only each N minutes (round_1m, round_5m)
BUYER = "payback"                       # the buyer of the new players (pw_sim.Sim._payback_pick)


def _active(name, decide, text, reset="model"):
    return dict(name=name, decide=decide, reset=reset, text=text)


def _visit(hours, between):
    state = "open (the game runs with no input)" if between == "open" else "closed (offline rules)"
    return dict(name=f"{between}_{hours}h", decide=1, visit_hours=hours, between=between, reset="model",
                text=f"Visits each {hours} h. The game is {state} between visits. Decides each 1 s in a visit.")


def _round(minutes):
    return dict(name=f"round_{minutes}m", decide=1, round_minutes=minutes, between="open", reset="model",
                text=f"Buys only each {minutes} min: 1 purchase round of 1 s, then {minutes} min of open game with "
                     "no input. The reset rule of the model.")


def _v1(name):
    return dict(name=name, v1=True, reset=name, text="Version 1 profile, with no change. " + PROFILES[name]["text"])


PLAYERS = (
    (_active("active_greedy", 1, "Present all the time. Decides each 1 s. The reset rule of the model."),
     _active("active_slow", 5, "Present all the time. Decides each 5 s. The reset rule of the model."))
    + tuple(_visit(h, "open") for h in IDLE_HOURS) + tuple(_visit(h, "closed") for h in IDLE_HOURS)
    + (_active("early_reset", 1, "Present all the time. Decides each 1 s. Resets when G >= 1 x L.", 1.0),
       _active("patient_reset", 1, "Present all the time. Decides each 1 s. Resets when G >= 10 x L.", 10.0))
    + tuple(_round(n) for n in ROUND_MINUTES)
    + (dict(name="no_input", decide=1, no_input=True, between="open", reset="model",
            text="Opens the game and never gives input: no purchase and no reset. The game stays open."),)
    + tuple(_v1(n) for n in PROFILES))
NAMES = tuple(p["name"] for p in PLAYERS)
FAST = ("active_greedy", "open_8h", "closed_24h", "early_reset", "patient_reset")
BASE = "active_greedy"
LEFT_OUT = ("play pattern continuous: the closed-game players ({names}) and the closed-idle bands are left out, "
            "because the game has no offline gain by design")
RESET_PLAYERS = ("early_reset", "patient_reset")
LEFT_OUT_RESET = ("no reset layer: the reset players ({names}) and the policy gap are left out, because the game has "
                  "no reset")


# ----------------------------------------------------------------------------- the play pattern
def is_continuous(m):
    return m["game"].get("play_pattern") == "continuous"


def idle_states(m):
    """The idle states that the players test. A continuous game has no closed state by design."""
    return ("open",) if is_continuous(m) else IDLE_STATES


def closed_player(name):
    return name.startswith("closed_")


def no_reset(m):
    """True for a model of the simulation path (the key simulation_path) with no reset layer: a reset rule has nothing
    to decide. A model of version 1 with no prestige keeps each player."""
    return "simulation_path" in m and not prestige_on(m)


def left_out(m, names):
    """The players of names that the model leaves out, and the 1 line that states it: the closed-game players of a
    continuous model, and the reset players of a model with no reset layer."""
    groups = [(closed_out(names), LEFT_OUT)] if is_continuous(m) else []
    groups += [(list(RESET_PLAYERS), LEFT_OUT_RESET)] if no_reset(m) else []
    gone = [n for g, _ in groups for n in g]
    return gone, "; ".join(text.format(names=", ".join(g)) for g, text in groups) or None


def closed_out(names):
    return [n for n in names if closed_player(n)] or [n for n in NAMES if closed_player(n)]


# ----------------------------------------------------------------------------- choices
def wanted(m, option):
    """The player names for the option --players: None, 'all', 'fast', 'none' or a comma list."""
    if option is None:
        g = m["game"]
        return _kept(m, list(FAST)) if "presence" in g or "targets" in g else []
    if option == "none":
        return []
    names = list(NAMES) if option == "all" else list(FAST) if option == "fast" else _listed(option)
    return _kept(m, names if BASE in names else [BASE] + names)


def _kept(m, names):
    names = [n for n in names if not closed_player(n)] if is_continuous(m) else names
    return [n for n in names if n not in RESET_PLAYERS] if no_reset(m) else names


def _listed(option):
    names = [x.strip() for x in option.split(",") if x.strip()]
    bad = [x for x in names if x not in NAMES]
    if bad:
        raise SystemExit(f"--players: unknown player '{bad[0]}'. Use 'all', 'fast', 'none' or names from: "
                         + ", ".join(NAMES))
    return names


def axis_of(m):
    ax = m["game"].get("progress_axis")
    if ax:
        return ax
    return m["ladders"][0]["id"] if len(m["ladders"]) == 1 else None


def reset_profile(m, rule, primary=None):
    """The Sim profile of a reset rule: 'model', a version 1 profile name, or a number X for G >= X * L."""
    if not prestige_on(m):
        return {}
    if isinstance(rule, (int, float)):
        return dict(policy="ratio", policy_ratio=float(rule))
    profs = profiles_for(m)
    name = (primary or primary_name(m, profs)) if rule == "model" else rule
    return next((s for n, s, _ in profs if n == name), {})


RESET_RULES = ("policy", "policy_ratio", "peak_tolerance", "max_resets", "unlock_in_run")


def reset_rules(values, main=None):
    """The reset rules of the player of a built game, from the values of its reset (system S04) in the content file:
    the policy with its tolerance or ratio, the most resets, a reset that tests the stage of the run, and the reward
    of values.reward_exponent on the main currency. The model as built copies them into its prestige block
    (references/reverse-engineering.md section 7)."""
    out = {k: values[k] for k in RESET_RULES if k in values}
    if values.get("reward_exponent") and main:      # income x (1 + lifetime reset currency)^exponent
        out["reward"] = dict(target=main, exponent=values["reward_exponent"])
    return out


def active_action(content, notes):
    """The milestone of the active action of a built game (system S05) from the content key active_factor
    {value, stat, place}: x value on the stat (default the main currency) while the player is active. The rebalance
    ignores the key, because it writes S05 itself. A note goes to notes."""
    af = content.get("active_factor")
    if not af:
        return []
    stat = af.get("stat") or content["currencies"]["main"]
    notes.append(f"content key active_factor: x{af['value']:g} on {stat} while the player is active "
                 f"({af.get('place') or 'no place'}).")
    return [dict(id="active_factor", name="Active action", system="main", persists=True, requires=[],
                 effects=[dict(target=stat, kind="mult", value=af["value"], active=True)])]


def rule_text(rule):
    if isinstance(rule, (int, float)):
        return f"G >= {rule:g} x L"
    return "the reset rule of the model" if rule == "model" else f"version 1 profile '{rule}'"


# ----------------------------------------------------------------------------- runs
class Players:
    """The runs of 1 model: 1 Sim for each player, and the base state of the single-wait idle runs."""

    def __init__(self, m, hours, primary=None, v1_runs=None):
        self.m, self.hours, self.primary = m, hours, primary
        self.v1_runs = v1_runs or {}
        self.axis = axis_of(m)
        self.main = main_currency(m)
        self.visit_seconds = presence_of(m["game"])["visit_minutes"] * 60.0
        self.runs = {}

    def player(self, spec, speed=1.0):
        p = dict(decide=spec.get("decide", 1.0), speed=speed, stop_at_end=True, buyer=BUYER, main=self.main)
        p.update(self.schedule(spec))
        return p

    def schedule(self, spec):
        """The visits of a player: each visit_hours (a visit of visit_minutes), or each round_minutes (a round of
        ROUND_SECONDS). None for a player who is present all the time."""
        hours, minutes = spec.get("visit_hours"), spec.get("round_minutes")
        if spec.get("no_input"):
            return dict(visit_every=self.horizon(spec) * HOUR, visit_seconds=0.0, between="open")
        if not (hours or minutes):
            return {}
        every = hours * HOUR if hours else minutes * 60.0 + ROUND_SECONDS
        visit = self.visit_seconds if hours else ROUND_SECONDS
        return dict(visit_every=every, visit_seconds=visit, between=spec["between"])

    def horizon(self, spec):
        """The simulated hours of a player: 3 x the length, so that a slow player can reach the end. The player
        with no input never ends, so it plays the length only."""
        if spec.get("no_input"):
            return self.hours
        return max(3 * self.hours, VISIT_HORIZON * spec.get("visit_hours", 0))

    def run(self, spec):
        if spec.get("v1"):
            return self._v1_run(spec)
        prof = reset_profile(self.m, spec["reset"], self.primary)
        return Sim(self.m, self.horizon(spec), prof, player=self.player(spec)).play()

    def _v1_run(self, spec):
        found = self.v1_runs.get(spec["name"])
        if found is not None:
            return found
        return Sim(self.m, self.hours, reset_profile(self.m, spec["reset"])).play()

    def run_all(self, names):
        specs = [p for p in PLAYERS if p["name"] in names]
        for spec in specs:
            self.runs[spec["name"]] = (spec, self.run(spec))
        return self.runs

    def speed_run(self, speed):
        spec = dict(decide=HUMAN_DECISION * speed)
        prof = reset_profile(self.m, "model", self.primary)
        return Sim(self.m, self.horizon({}), prof, player=self.player(spec, speed)).play()


def clone(sim):
    return copy.deepcopy(sim, {id(sim.m): sim.m})


def axis_value(sim, ax):
    return sim.ladder_best[ax] if ax else 0


def quarter_steps(count):
    return [(label, max(1, math.ceil(count * q - 1e-9))) for label, q in QUARTERS]


def step_time(sim, ax, step, k):
    """Clock k (0 real, 1 active, 2 game) of the first time the axis reached the step, else None."""
    hit = sim.step_clock.get(ax, {}).get(step)
    return None if hit is None else hit[k]


# ----------------------------------------------------------------------------- the players block
def _decisions(sim):
    """Count of moments with a purchase or a reset: the visits of a player who is present all the time."""
    return len({e["t"] for e in sim.events if e["kind"] in ("purchase", "prestige")})


def _quarter_times(sim, ax, k, scale):
    if not ax:
        return {}
    steps = quarter_steps(sim.lmap[ax]["count"])
    out = {label: _scaled(step_time(sim, ax, s, k), scale) for label, s in steps}
    return dict(out, end=_scaled(end_clock(sim, ax, k), scale)) if sim.end_stops() else out


def end_clock(sim, ax, k):
    """Clock k of the hard end event of the model (game.targets.hard_end) of a kind that stops the run, else of the
    last step of the axis."""
    if sim.end_stops():
        return None if sim.end_clock is None else sim.end_clock[k]
    return step_time(sim, ax, sim.lmap[ax]["count"], k)


def _scaled(x, scale):
    return None if x is None else round(x / scale, 4)


def player_row(spec, sim, ax):
    visits = sim.visits if spec.get("visit_hours") else _decisions(sim)
    return dict(name=spec["name"], text=spec["text"], decide_seconds=spec.get("decide", 0),
                visit_hours=spec.get("visit_hours"), between=spec.get("between"), reset_rule=rule_text(spec["reset"]),
                wall_hours=_quarter_times(sim, ax, 0, HOUR), active_minutes=_quarter_times(sim, ax, 1, 60.0),
                visits=visits, resets=sim.prestiges, axis_reached=axis_value(sim, ax),
                finished=bool(ax) and (sim.axis_done() or sim.end_reached() and sim.end_stops()),
                simulated_wall_hours=round(sim.real / HOUR, 3))


# ----------------------------------------------------------------------------- idle curve
def axis_rate(sim, ax):
    """Progress axis steps for each wall hour: to the end of the axis, else to the end of the run."""
    if not ax or not sim.real:
        return None
    best = sim.ladder_best[ax]
    end = step_time(sim, ax, best, 0) if sim.axis_done() else sim.real
    return best / (end / HOUR) if end else None


def base_state(pl):
    """The state of active_greedy when the progress axis first reaches 25% of its count."""
    spec = next(p for p in PLAYERS if p["name"] == BASE)
    player = pl.player(spec)
    player["stop_at_step"] = quarter_steps(next(l for l in pl.m["ladders"] if l["id"] == pl.axis)["count"])[0][1]
    sim = Sim(pl.m, pl.horizon(spec), reset_profile(pl.m, "model", pl.primary), player=player).play()
    sim.stop_at_step = None
    return sim


def single_wait(pl, base, state, hours):
    """Axis steps from 1 wait and 1 visit with no reset: the rise of the current step."""
    sim = clone(base)
    sim.present = False
    (sim.away_open if state == "open" else sim.away_closed)(hours * HOUR)
    sim.start_visit()
    sim.no_reset = True
    sim.limit = sim.t + pl.visit_seconds
    sim.play()
    return sim.ladder_pos[pl.axis] - base.ladder_pos[pl.axis]


def active_gains(pl, base):
    """Axis steps of active_greedy from the base state after each wait of IDLE_HOURS."""
    sim = clone(base)
    return {h: _gain_after(sim, base, pl.axis, h) for h in IDLE_HOURS}


def _gain_after(sim, base, ax, h):
    """Play sim on to h hours after the base state. Return the axis steps gained since the base state."""
    sim.limit = base.t + h * HOUR
    sim.play()
    return sim.ladder_best[ax] - base.ladder_best[ax]


def _idle_point(pl, base, act, state, h):
    gain = single_wait(pl, base, state, h)
    spec_run = pl.runs.get(f"{state}_{h}h")
    rate, base_rate = (axis_rate(spec_run[1], pl.axis) if spec_run else None), axis_rate(pl.runs[BASE][1], pl.axis)
    value = rate / base_rate if rate is not None and base_rate else None
    return dict(value=_round(value), gain=gain, active_gain=act[h], gain_ratio=_round(gain / act[h] if act[h] else None))


def _round(x, n=4):
    return None if x is None else round(x, n)


def idle_curve(pl):
    """rho at each wait: the axis rate of the player who visits each T over the rate of active_greedy (law L4).
    gain: the axis steps from 1 wait and 1 visit with no reset, from the 25% state of active_greedy."""
    if not pl.axis:
        return None
    base = base_state(pl)
    act = active_gains(pl, base)
    states = idle_states(pl.m)
    out = {state: {str(h): _idle_point(pl, base, act, state, h) for h in IDLE_HOURS} for state in states}
    out["start_step"] = base.ladder_best[pl.axis]
    out["gain_per_doubling"] = {state: _round((out[state]["8"]["gain"] - out[state]["1"]["gain"]) / 3.0)
                                for state in states}
    return out


# ----------------------------------------------------------------------------- growth slope
def _value_at(samples, t):
    before = [s for s in samples if s["t"] <= t + 1e-9]
    return before[-1] if before else None


def _log_income(sample, c):
    if sample is None or sample["income"][c] <= 0:
        return None
    return math.log10(sample["income"][c])


def _axis_at(sim, ax, t):
    return max([s for s, hit in sim.step_clock[ax].items() if hit[2] <= t + 1e-9], default=0)


def _delta(a, b):
    return None if a is None or b is None else round(a - b, 4)


def _growth_row(sim, ax, c, h):
    now, before = _value_at(sim.samples, h * HOUR), _value_at(sim.samples, (h - 1) * HOUR)
    a_now, a_before = _axis_at(sim, ax, h * HOUR), _axis_at(sim, ax, (h - 1) * HOUR)
    return dict(hour=h, log10_income=_round(_log_income(now, c)), axis=a_now,
                d_log10_income=_delta(_log_income(now, c), _log_income(before, c)), d_axis=a_now - a_before)


def growth_slope(m, pl):
    """For each hour of active_greedy: the change of log10(income of the main currency) and of the axis."""
    sim = pl.runs[BASE][1]
    if not pl.axis:
        return None
    hours = min(GROWTH_ROWS, max(1, math.ceil(sim.t / HOUR - 1e-9)))
    c = main_currency(m)
    rows = [_growth_row(sim, pl.axis, c, h) for h in range(1, hours + 1)]
    return dict(currency=c, rows=rows, unlocks_first_hour=rows[0]["d_axis"],
                unlocks_per_hour=_later_unlocks(sim, pl.axis, rows),
                length_hours=_round(_end_hours(sim, pl.axis)))


def _later_unlocks(sim, ax, rows):
    """The median of the axis steps in each full hour after the first, before the end of the axis."""
    full = [r["d_axis"] for r in rows[1:] if r["hour"] * HOUR <= sim.t + 1e-9 and not _ended_before(sim, ax, r)]
    return statistics.median(full) if full else None


def _end_hours(sim, ax):
    end = end_clock(sim, ax, 0)
    return None if end is None else end / HOUR


def _ended_before(sim, ax, row):
    end = _end_hours(sim, ax)
    return end is not None and end < row["hour"]


# ----------------------------------------------------------------------------- policy gap and speed
def common_time(a, b, ax, k):
    """(time of a, time of b) to the highest axis step that both reach, on clock k."""
    top = min(axis_value(a, ax), axis_value(b, ax))
    if top <= 0:
        return None, None
    return step_time(a, ax, top, k), step_time(b, ax, top, k)


def policy_gap(pl):
    early, patient = pl.runs.get("early_reset"), pl.runs.get("patient_reset")
    if not (early and patient and pl.axis):
        return None
    te, tp = common_time(early[1], patient[1], pl.axis, 0)
    return dict(value=_round(te / tp if te and tp else None), early_hours=_round(te and te / HOUR),
                patient_hours=_round(tp and tp / HOUR), axis_step=min(axis_value(early[1], pl.axis),
                                                                     axis_value(patient[1], pl.axis)),
                early_gains=[r["gain"] for r in early[1].runs[:50]],
                patient_gains=[r["gain"] for r in patient[1].runs[:50]])


def _slowdown(base, sim, ax):
    t1, tk = common_time(base, sim, ax, 2)
    return _round(tk / t1 if t1 and tk else None)


def _fast_enough(r):
    return r["slowdown"] is not None and r["slowdown"] < SLOWDOWN_MAX


def _largest_speed(rows):
    ok = list(itertools.takewhile(_fast_enough, rows))
    return ok[-1]["speed"] if ok else None


def speed_test(m, pl, speeds):
    """Law L9: game time to the common axis step at speed k with a human decision each 2 real seconds,
    over the time of active_greedy (1 decision each game second)."""
    if not pl.axis:
        return None
    base = pl.runs[BASE][1]
    play_test = (m["game"].get("targets") or {}).get("play_test_speed", DEFAULT_PLAY_TEST_SPEED)
    tested = sorted(set(speeds) | {play_test})
    rows = [dict(speed=k, decision_game_seconds=HUMAN_DECISION * k, slowdown=_slowdown(base, pl.speed_run(k), pl.axis))
            for k in tested]
    at_test = next(r["slowdown"] for r in rows if r["speed"] == play_test)
    return dict(human_decision_seconds=HUMAN_DECISION, rows=rows, largest_speed=_largest_speed(rows),
                largest_rule=f"largest tested speed with a slow-down under {SLOWDOWN_MAX:g} at each lower speed",
                play_test_speed=play_test, slowdown_at_play_test=at_test)


# ----------------------------------------------------------------------------- lines
def never_bought(m, runs):
    buyers = {p["id"]: [n for n, (_, sim) in runs.items() if p["id"] in sim.ever] for p in m["purchases"]}
    return dict(no_player=sorted(i for i, b in buyers.items() if not b),
                one_player=[dict(id=i, player=b[0]) for i, b in sorted(buyers.items()) if len(b) == 1])


def line_exponent(p):
    """r = ln(mult) / ln(cost growth) of the best mult effect of an exp-cost line. 0 for other lines."""
    c = p["costs"][0]
    if c["formula"] != "exp" or c["growth"] <= 1:
        return 0.0
    mults = [e["value"] for e in p["effects"] if e["kind"] == "mult" and e["value"] > 1]
    return max([math.log(v) / math.log(c["growth"]) for v in mults], default=0.0)


def unbounded(m):
    """Lines with r >= 1 and no cap, and currencies whose only sinks have no cap."""
    open_lines = [p for p in m["purchases"] if p["max_level"] is None]
    lines = [dict(id=p["id"], r=round(line_exponent(p), 4)) for p in open_lines if line_exponent(p) >= 1 - 1e-9]
    sinks = {}
    for p in m["purchases"]:
        for c in p["costs"]:
            sinks.setdefault(c["currency"], []).append(p["max_level"] is None)
    return dict(lines=lines, currencies=sorted(c for c, uncapped in sinks.items() if all(uncapped)))


# ----------------------------------------------------------------------------- bands
def band_row(name, value, target, default):
    """The status of 1 indicator. target and default are [low, high] or None."""
    band, source = (target, "target") if target else (default, "default" if default else None)
    if value is None:
        status = "not run"
    elif band is None:
        status = "no target"
    else:
        status = "in band" if band[0] <= value <= band[1] else "out of band"
    return dict(indicator=name, value=value, low=band and band[0], high=band and band[1], source=source,
                status=status)


def _idle_rows(t, curve, states):
    idle = t.get("idle") or {}
    return [band_row(f"idle value {state} {h} h", curve[state][str(h)]["value"],
                     _idle_target(idle, state, h), [0.0, 1.0])
            for state in states for h in IDLE_HOURS]


def _idle_target(idle, state, h):
    rows = idle.get(state) or {}
    return next((b for k, b in rows.items() if float(k) == h), None)


def _gain_row(curve, state):
    gains = [curve[state][str(h)]["gain"] for h in IDLE_HOURS]
    ok = gains[0] > 0 and all(b >= a for a, b in zip(gains, gains[1:]))
    row = band_row(f"idle gain {state}: above 0 and rising with the wait", 1.0 if ok else 0.0, None, [1.0, 1.0])
    row["value"] = ", ".join(str(g) for g in gains)
    return row


def _value_of(block, key, field):
    part = block.get(key)
    return part.get(field) if part else None


def bands(m, block):
    t = m["game"].get("targets") or {}
    curve = block["idle_curve"]
    states = idle_states(m)
    rows = (_idle_rows(t, curve, states) + [_gain_row(curve, s) for s in states]) if curve else []
    speed = _value_of(block, "speed_test", "play_test_speed") or DEFAULT_PLAY_TEST_SPEED
    return rows + [
        band_row("policy gap (early / patient time)", _value_of(block, "policy_gap", "value"), t.get("policy_gap"),
                 [0.0, 2.0]),
        band_row(f"slow-down at x{speed:g}", _value_of(block, "speed_test", "slowdown_at_play_test"),
                 t.get("slowdown"), [0.0, SLOWDOWN_MAX]),
        band_row("unlocks in the first hour", _value_of(block, "growth_slope", "unlocks_first_hour"),
                 t.get("unlocks_first_hour"), None),
        band_row("unlocks for each later hour (median)", _value_of(block, "growth_slope", "unlocks_per_hour"),
                 t.get("unlocks_per_hour"), None),
        band_row("length (active_greedy hours to the end of the axis)",
                 _value_of(block, "growth_slope", "length_hours"), t.get("length_hours"), None),
        band_row("purchases by hand in the busiest minute (active_greedy)", block.get("busiest_minute"),
                 [0, t["purchase_rate_max_per_minute"]] if t.get("purchase_rate_max_per_minute") else None,
                 [0, PURCHASE_RATE_MAX]),
    ]


def busiest_minute(sim):
    """The most purchases by hand of the player in 1 minute of wall time (an auto-buy purchase does not count)."""
    minutes = [int(e["real"] // 60.0) for e in sim.events if e["kind"] == "purchase" and not e.get("auto")]
    return max((minutes.count(k) for k in set(minutes)), default=0)


# ----------------------------------------------------------------------------- entry point
def run_players(m, hours, names, primary=None, v1_runs=None):
    """The players block and the presence block of the analysis JSON. names: a list from wanted()."""
    pl = Players(m, hours, primary, v1_runs)
    gone, note = left_out(m, names)
    full, runs = _run_kept(pl, names, gone)
    block = dict(mode="all" if full else "subset", players_run=list(runs), unbounded=unbounded(m),
                 never_bought=never_bought(m, runs), idle_curve=idle_curve(pl), growth_slope=growth_slope(m, pl),
                 policy_gap=policy_gap(pl), speed_test=speed_test(m, pl, SPEEDS_ALL if full else SPEEDS_FAST))
    busy = busiest_minute(runs[BASE][1]) if BASE in runs else None
    block["bands"] = bands(m, dict(block, busiest_minute=busy))
    block.update(dict(left_out=gone, left_out_note=note) if note else {})
    rows = [player_row(spec, sim, pl.axis) for spec, sim in runs.values()]
    return rows, block, pl


def _run_kept(pl, names, gone):
    """(full, runs): full is true when names and the left-out players cover each player."""
    runs = pl.run_all([n for n in names if n not in gone])
    return set(names) | set(gone) >= set(NAMES), runs


# ----------------------------------------------------------------------------- console
def _num(x, fmt="{:.2f}"):
    return "n/a" if x is None else fmt.format(x)


def player_line(row):
    end = row["wall_hours"].get("end")
    active = row["active_minutes"].get("end")
    return (f"  PLAYER {row['name']:14s} end at {_num(end)} h wall, {_num(active, '{:.1f}')} active min | "
            f"visits {row['visits']} | resets {row['resets']} | axis {row['axis_reached']}")


def idle_lines(curve):
    if not curve:
        return ["  IDLE CURVE: the model has no progress axis"]
    return [_idle_line(curve, state) for state in IDLE_STATES if state in curve]


def _idle_line(curve, state):
    pts = ", ".join(f"{h} h {_num(curve[state][str(h)]['value'])} (gain {curve[state][str(h)]['gain']})"
                    for h in IDLE_HOURS)
    return f"  IDLE CURVE {state}: {pts}; gain for each doubling 1-8 h: {_num(curve['gain_per_doubling'][state])}"


def presence_lines(block):
    pg, sp, ub = block["policy_gap"], block["speed_test"], block["unbounded"]
    lines = ([f"  LEFT OUT: {block['left_out_note']}"] if block.get("left_out_note") else [])
    lines += idle_lines(block["idle_curve"])
    lines.append("  POLICY GAP: " + (f"{_num(pg['value'])} (early {_num(pg['early_hours'])} h, patient "
                                     f"{_num(pg['patient_hours'])} h)" if pg else "not run"))
    if sp:
        lines.append("  SPEED TEST: " + ", ".join(f"x{r['speed']:g} {_num(r['slowdown'], '{:.3f}')}" for r in sp["rows"])
                     + f"; largest speed under {SLOWDOWN_MAX:g}: {_num(sp['largest_speed'], '{:g}')}")
    lines.append("  UNBOUNDED lines: " + (", ".join(f"{x['id']} (r {x['r']:.2f})" for x in ub["lines"]) or "none")
                 + "; currencies with only uncapped sinks: " + (", ".join(ub["currencies"]) or "none"))
    lines += [f"  BAND OUT: {b['indicator']} = {b['value']} (band {b['low']} to {b['high']}, {b['source']})"
              for b in block["bands"] if b["status"] == "out of band"]
    return lines
