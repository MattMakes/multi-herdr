"""The simulated buyer for progression_workbook.py.

Time: `t` is seconds of play (the clock of all indicators). `real` is seconds of real time. The two are
equal unless the model sets game.away_minutes.

A player (see pw_players.py) changes 3 things. The buyer decides only at multiples of `decide` seconds. The
player visits on a schedule, and between visits the game is open or closed (see "presence" below). A game
speed `speed` scales the game clock: a source or a timer with the clock "real" does not follow it. With
no player, the run is the run of version 1. `active` is seconds of play while the player is present.
"""
import math
from functools import reduce

from pw_model import BIG, INF, cost_one, presence_of, prestige_on

WEIGHT = {"system_unlock": 5, "prestige_first": 5, "prestige": 3, "feature": 3, "content": 2,
          "reward": 2, "ladder": 2, "first_purchase": 2, "purchase": 1, "repeat": 1, "stall": 0}
SMALL_VALUE = 0.02      # value of a purchase that changes nothing the buyer can measure
WALL_WEIGHT = 3.0       # weight of a stat that blocks the next ladder step or a milestone
BASE = "(base income)"
EFFECT_OWNERS = (("p", "purchases"), ("m", "milestones"), ("l", "ladders"))
LEVEL_NEED_OWNERS = (("p", "purchases"), ("m", "milestones"))
TIMER_KEYS = ("attempt_seconds", "cooldown_seconds", "needs_visit")
AWAY_STEP_LIMIT = 20000  # most event steps in 1 stretch of away time
AUTO_BUY_LIMIT = 1000   # most auto-buy purchases (S24) in 1 step
STALL_NO_CONTENT = ("Nothing left to buy or reach", "content ends here, or a requirement can never be met")
STALL_NO_GOAL = ("No reachable goal", "prestige is enabled, but the buyer did not reset and nothing "
                                      "else changed for this long")


# -- compiled effects -----------------------------------------------------------
def _delta_add(d, c, v, grp):
    d[0][c] = d[0].get(c, 0.0) + v


def _delta_pct(d, c, v, grp):
    d[1].setdefault(c, {})[grp] = d[1].get(c, {}).get(grp, 0.0) + v


def _delta_mult(d, c, v, grp):
    if v <= 0:
        return d[3].add(c)
    d[2][c] = d[2].get(c, 0.0) + math.log(v)


DELTA_STEPS = {"add": _delta_add, "pct": _delta_pct}


def purchase_delta(fx):
    """What one more level of a purchase adds to the income terms: (add, pct, log of mult, zeroed, touched)."""
    d = ({}, {}, {}, set())
    for c, k, v, grp in fx:
        DELTA_STEPS.get(k, _delta_mult)(d, c, v, grp)
    dadd, dpct, dmul, dzero = d
    return (dadd, dpct, dmul, dzero, set(dadd) | set(dpct) | set(dmul) | dzero)


def flat_fx(effects):
    """[(targets, kind, value, group)] -> [(currency, kind, value, group)]."""
    return [(c, k, v, grp) for targets, k, v, grp in effects for c in targets]


def _first(x):
    return x[0]


def step_amount(r, step):
    """The amount that step n of a ladder needs: the table first, then growth from its last entry."""
    table = r.get("table")
    if not table:
        return r["base"] * r["growth"] ** (step - 1)
    if step <= len(table):
        return table[step - 1]
    return table[-1] * r["growth"] ** (step - len(table))


def level_needs(kind, ident, reqs):
    """(purchase id, (kind, owner id, level)) for each 'level' requirement in reqs."""
    return [(r["id"], (kind, ident, r.get("level", 1))) for r in reqs if r["type"] == "level"]


def step_boost(steps, n):
    """The factor of the count steps of a line at n owned levels: mult^floor(n / every) (card S12.count_steps)."""
    return steps["mult"] ** (n // steps["every"]) if steps else 1.0


def block_cost(c, level, n):
    """The cost of the next n levels of a line from level, in the cost entry c."""
    return sum(cost_one(c, level + i) for i in range(n))


def new_purchase_stats():
    return dict(first_avail=None, first_afford=None, first=None, last=None, top=0, spent={},
                cap_at=None, pay_first=None, pay_last=None, bought=0)


def grant_amount(g, step):
    return g["amount"] if "amount" in g else g.get("base", 0) * g.get("growth", 1.0) ** (step - 1)


def scale_by_pct(x, groups, extra):
    """Multiply x by (1 + sum) of each percent group. extra adds to the groups."""
    new = [v for grp, v in extra.items() if grp not in groups]
    for grp, v in groups.items():
        x *= max(0.0, 1 + v + extra.get(grp, 0.0))
    for v in new:
        x *= max(0.0, 1 + v)
    return x


def currency_gain(new, old, weight):
    """Weighted relative change of the income of 1 currency. new and old are not equal."""
    if new > old:
        return weight * (min(10.0, new / old - 1.0) if old > 0 else 10.0)
    return -(weight * (1.0 - new / old))


# -- upkeep (S30) ---------------------------------------------------------------
NO_DELTA = ({}, {}, {}, set(), set())      # the income delta of a line with no income effect (a care line)


def upkeep_sums(up, level, plus=None):
    """(need N, capacity C) of the upkeep block at the levels; plus: 1 more level of this line (or of each line of a
    tuple)."""
    more = plus if isinstance(plus, tuple) else (plus,)
    lv = (lambda pid: level.get(pid, 0) + more.count(pid))
    need = up.get("base_need", 0.0) + sum(n.get("per_level", 1.0) * lv(n["purchase"]) for n in up.get("need") or [])
    cap = up.get("base_capacity", 0.0)
    for c in up.get("capacity") or []:
        cap = cap * c["value"] ** lv(c["purchase"]) if c.get("kind") == "mult" else cap + c["value"] * lv(c["purchase"])
    return need, cap


def upkeep_factor(up, level, plus=None):
    """u = max(floor, min(1, C / N)^e), and 1 with no need (references/model-schema.md, "upkeep")."""
    need, cap = upkeep_sums(up, level, plus)
    if need <= 0:
        return 1.0
    return max(up.get("floor", 0.0), min(1.0, max(cap, 0.0) / need) ** up.get("exponent", 1.0))


def upkeep_lines(up):
    """The purchase ids of the need lines and of the capacity lines."""
    if not up:
        return set(), set()
    return {n["purchase"] for n in up.get("need") or []}, {c["purchase"] for c in up.get("capacity") or []}


def event_limit_note(max_events):
    return f"the run was cut at {max_events} events. Raise costs, set a max_level, or group lines"


class IncomeTerms:
    """The sums that make the income of each currency.

    add: flat adds. pct: percent sums in groups. mul: the log of the product of the factors.
    zero: currencies that a factor of 0 or less sets to 0. src: the flat adds of each system (or None).
    """

    def __init__(self, cids, parts):
        self.add = dict.fromkeys(cids, 0.0)
        self.pct = {c: {} for c in cids}
        self.mul = dict.fromkeys(cids, 0.0)
        self.zero = set()
        self.src = {c: {} for c in cids} if parts else None

    def take(self, system, fx, n, boost=1.0):
        """Add the effects fx of a source that has the count n. boost: the factor of the count steps of the line on
        each add effect (1 with no count step)."""
        steps = TERM_STEPS
        for c, k, v, grp in fx:
            steps.get(k, IncomeTerms._mult)(self, c, v * boost if k == "add" else v, grp, n, system)

    def _add(self, c, v, grp, n, system):
        self.add[c] += v * n
        if self.src is not None and v > 0:
            self.src[c][system] = self.src[c].get(system, 0.0) + v * n

    def _pct(self, c, v, grp, n, system):
        self.pct[c][grp] = self.pct[c].get(grp, 0.0) + v * n

    def _mult(self, c, v, grp, n, system):
        if v <= 0:
            return self.zero.add(c)
        self.mul[c] += n * math.log(v)

    def add_base(self, cids, base_income):
        """Put the base income last in the sources of each currency."""
        if self.src is None:
            return
        paid = [c for c in cids if base_income[c] > 0]
        for c in paid:
            self.src[c][BASE] = base_income[c]

    def as_tuple(self):
        return (self.add, self.pct, self.mul, self.zero)


TERM_STEPS = {"add": IncomeTerms._add, "pct": IncomeTerms._pct}


def _axis_end(sim, v):
    ax = sim._axis_id()
    return ax is not None and sim.ladder_best[ax] >= v


def _never(sim, v):
    return False


# The test of each kind of hard end (game.targets.hard_end): the event happened.
END_TESTS = {"axis_step": _axis_end, "purchase": lambda sim, v: v in sim.ever,
             "resets": lambda sim, v: sim.prestiges >= v, "milestone": lambda sim, v: v in sim.ever_done}


STOP_KINDS = ("resets", "milestone")     # the kinds of hard end after which the game has no more play


class Sim:
    def __init__(self, m, hours, profile=None, beat_weight=3, max_events=40000, player=None):
        self.m = m
        self.limit = hours * 3600.0
        self.beat_weight = beat_weight
        self.max_events = max_events
        self.t = self.real = 0.0
        self._init_balances(m)
        self._init_progress(m)
        self._init_prestige(m, profile)
        self._init_sessions(m["game"])
        self._init_player(m, player or {})
        self._init_upkeep(m)
        self._compile_effects(m)
        self._init_records(m)
        self._init_auto(m)

    def _init_balances(self, m):
        self.cids = [c["id"] for c in m["currencies"]]
        self.base_income = {c["id"]: float(c.get("base_income", 0.0)) for c in m["currencies"]}
        self.cur = {c["id"]: float(c.get("start", 0)) for c in m["currencies"]}
        self.life = dict.fromkeys(self.cur, 0.0)
        self.run = dict.fromkeys(self.cur, 0.0)

    def _init_progress(self, m):
        self.level = {p["id"]: 0 for p in m["purchases"]}
        self.ever = set()
        self.done = {}                      # milestones done now -> time
        self.ever_done = {}                 # milestones done at any time -> first time
        self.ladder_pos = {l["id"]: 0 for l in m["ladders"]}
        self.ladder_best = {l["id"]: 0 for l in m["ladders"]}
        self.lmap = {l["id"]: l for l in m["ladders"]}
        self.pmap = {p["id"]: p for p in m["purchases"]}
        self.prestiges = 0
        self.run_start = 0.0
        self.events, self.samples, self.runs = [], [], []
        self.cut = False

    def _init_prestige(self, m, profile):
        self.pr = dict(m["prestige"]) if prestige_on(m) else None
        if self.pr and profile:
            self.pr.update(profile)
        self.best_rate = 0.0
        self.run_new_beats = 0
        self.run_top = dict(self.ladder_pos)

    def _init_sessions(self, g):
        """Sessions with away time."""
        self.session = g["session_minutes"] * 60.0
        self.away = g["away_minutes"] * 60.0
        self.offline = g["offline_rate"]
        self.block_left = self.session

    def _init_player(self, m, p):
        """The player: decision interval, game speed, and visit schedule. An empty p gives version 1."""
        self.player = p or None
        self.decide = p.get("decide", 0.0)
        self.speed = p.get("speed", 1.0)
        self.stop_at_end = p.get("stop_at_end", False)
        self.stop_at_step = p.get("stop_at_step")
        self._init_end(m)
        self.buyer, self.main = p.get("buyer", "v1"), p.get("main")
        self.no_reset = False
        self.present, self.visits, self.active = True, 0, 0.0
        self._rates = self._ainc = None
        self.step_last, self.attempt_from = {}, {}
        self.step_clock = {l["id"]: {} for l in m["ladders"]}     # step -> (real, active, t) of the first time
        self.bank_caps = {c["id"]: c["bank_cap"] for c in m["currencies"] if "bank_cap" in c}
        self.bank = dict.fromkeys(self.bank_caps, 0.0)
        self.sysclock = {s["id"]: s.get("clock", "game") for s in m["systems"]}
        self._init_timers(m)
        self._init_visits(m["game"], p)

    def _init_end(self, m):
        self.end_mark = (m["game"].get("targets") or {}).get("hard_end")
        self.end_real = self.end_clock = None   # the hard end event: real seconds, and (real, active, t); None before

    def _init_visits(self, g, p):
        if self.player is None:
            return
        self.away, self.offline = 0.0, g["offline_rate"]
        self.pres = presence_of(g)
        self._init_schedule(p)

    def _init_schedule(self, p):
        """A player with visits: the visit is the session, and the time between visits is the away block."""
        if not p.get("visit_every"):
            return
        self.session = p["visit_seconds"]
        self.away = p["visit_every"] - p["visit_seconds"]
        self.block_left = self.session
        self.between = p["between"]

    def _init_upkeep(self, m):
        """S30 upkeep: the factor u on the income of 1 currency (the key upkeep of the model)."""
        self.up = m.get("upkeep") or None
        self.need_lines, self.care_lines = upkeep_lines(self.up)

    def upkeep_u(self, plus=None):
        """The upkeep factor u now (1 with no upkeep block); plus: with 1 more level of this line."""
        return upkeep_factor(self.up, self.level, plus) if self.up else 1.0

    def _take_upkeep(self, terms):
        if not self.up:
            return
        u = self.upkeep_u()
        if u <= 0:
            return terms.zero.add(self.up["target"])
        terms.mul[self.up["target"]] += math.log(u)

    def _clock_of(self, x):
        return x.get("clock") or self.sysclock.get(x.get("system"), "game")

    def _init_timers(self, m):
        """(attempt seconds, cooldown seconds, needs a visit) of each ladder with a timer, in game seconds."""
        self.timed = {l["id"]: self._timer_of(l) for l in m["ladders"] if any(k in l for k in TIMER_KEYS)}

    def _timer_of(self, l):
        k = self.speed if self._clock_of(l) == "real" else 1.0
        return (l.get("attempt_seconds", 0) * k, l.get("cooldown_seconds", 0) * k, l.get("needs_visit", False))

    def _clock(self):
        """The clock of the run limit: real time for a player with visits, else seconds of play."""
        return self.real if self.player and self.player.get("visit_every") else self.t

    def _compile_effects(self, m):
        """Compile the effects one time."""
        g = m["game"]
        self.af = g["active_fraction"]
        by_system = g["pct_grouping"] == "system"
        self.sources = []                   # (kind, id, system, [(currency, kind, value, group)])
        self.away_sources = []              # the same with no active-only effect: the income while away
        self.capfx = {}                     # purchase id -> [(source kind, source id, value)]
        owners = [(kind, x) for kind, key in EFFECT_OWNERS for x in m[key]]
        for kind, x in owners:
            self._compile_owner(kind, x, by_system)
        self.has_income_fx = {s[1] for s in self.sources if s[0] == "p"} | self.need_lines | self.care_lines
        # purchase id -> what one more level adds to the income terms
        self.pdelta = {ident: purchase_delta(fx) for kind, ident, _, fx in self.sources if kind == "p"}
        self.count_steps = {p["id"]: p["count_steps"] for p in m["purchases"] if p.get("count_steps")}
        self._compile_cap_targets()
        self._compile_wanted_by(m)

    def _compile_owner(self, kind, x, by_system):
        self._register_caps(kind, x)
        stat = [e for e in x["effects"] if e["kind"] != "cap"]
        if not stat:
            return
        effects = [self._stat_effect(x, e, by_system) for e in stat]
        away = [f for e, f in zip(stat, effects) if not e.get("active")]
        self.sources.append((kind, x["id"], x["system"], flat_fx(effects)))
        self.away_sources.append((kind, x["id"], x["system"], flat_fx(away)))

    def _register_caps(self, kind, x):
        caps = [e for e in x["effects"] if e["kind"] == "cap"]
        for e in caps:
            self.capfx.setdefault(e["target"], []).append((kind, x["id"], e["value"]))

    def _stat_effect(self, x, e, by_system):
        targets = tuple(self.cids) if e["target"] == "all" else (e["target"],)
        v = e["value"] * (self.af if e.get("active") and e["kind"] == "add" else 1.0)
        if self.speed != 1.0 and e["kind"] == "add" and self._clock_of(x) == "real":
            v /= self.speed                 # a real-time source pays the same for each real second
        return (targets, e["kind"], v, e.get("group") or (x["system"] if by_system else ""))

    def _compile_cap_targets(self):
        self.cap_targets = {}               # source purchase id -> [(target id, value)]
        raises = [(sid, tgt, v) for tgt, lst in self.capfx.items() for kind, sid, v in lst if kind == "p"]
        for sid, tgt, v in raises:
            self.cap_targets.setdefault(sid, []).append((tgt, v))

    def _compile_wanted_by(self, m):
        self.wanted_by = {}                 # purchase id -> [(kind, id, level)]
        needs = [n for kind, key in LEVEL_NEED_OWNERS for x in m[key]
                 for n in level_needs(kind, x["id"], x["requires"])]
        if self.pr:
            needs += level_needs("x", "prestige", self.pr["requires"])
        for pid, need in needs:
            self.wanted_by.setdefault(pid, []).append(need)

    def _init_records(self, m):
        """Records for the sheets."""
        self._inc = self._parts = self._w = self._acc = None
        self.last_change = 0.0
        self.pstat = {p["id"]: new_purchase_stats() for p in m["purchases"]}
        self.steps = {l["id"]: {} for l in m["ladders"]}      # step -> (t, value of the gating quantity)
        self.earn = {c: {} for c in self.cids}
        self.spent = {c: {} for c in self.cids}
        self.lost = dict.fromkeys(self.cids, 0.0)
        self.axis = m["game"].get("progress_axis")

    def _init_auto(self, m):
        """S24 auto-buy: {line: seconds of active play when its auto-buy opens}, from the key automation."""
        rows = [a for a in m.get("automation") or [] if a.get("line") in self.pmap]
        self.auto_from = {a["line"]: 60.0 * float(a.get("from_minutes", 0.0)) for a in rows if not a.get("requires")}
        self.auto_reqs = {a["line"]: a["requires"] for a in rows if a.get("requires")}
        self.auto_open = set()
        self.auto_lines = list(self.auto_from) + list(self.auto_reqs)

    # -- income -----------------------------------------------------------------
    def count(self, kind, ident):
        if kind == "p":
            return self.level[ident]
        if kind == "m":
            return 1 if ident in self.done else 0
        return self.ladder_best[ident] if self.lmap[ident]["effects_persist"] else self.ladder_pos[ident]

    def compute_income(self, parts=False):
        out, terms = self._income_terms(self.sources, parts)
        self._acc = terms.as_tuple()
        return out, terms.src

    def _income_terms(self, sources, parts):
        terms = IncomeTerms(self.cids, parts)
        counts = [(s, self.count(s[0], s[1])) for s in sources]
        active = [(s[2], s[3], n, self._boost(s, n)) for s, n in counts if n]
        for system, fx, n, boost in active:
            terms.take(system, fx, n, boost)
        self._take_reward(terms)
        self._take_upkeep(terms)
        out = {c: self._final_income(c, terms) for c in self.cids}
        terms.add_base(self.cids, self.base_income)
        return out, terms

    def _boost(self, source, n):
        return step_boost(self.count_steps.get(source[1]), n) if source[0] == "p" else 1.0

    def _delta(self, pid, more=1):
        """What `more` levels of a purchase add to the income terms. A line with count steps: the add of the levels
        with the factor of the steps after them, less the add now."""
        delta, steps = self.pdelta.get(pid, NO_DELTA), self.count_steps.get(pid)
        if not steps and more == 1:
            return delta
        n = self.level[pid]
        scale = (n + more) * step_boost(steps, n + more) - n * step_boost(steps, n)
        return ({c: v * scale for c, v in delta[0].items()},) + tuple(delta[1:])

    def _take_reward(self, terms):
        """The reward of the reset layer: income x (1 + lifetime reset currency)^exponent."""
        rw = self.pr.get("reward") if self.pr else None
        if not rw:
            return
        bonus = rw["exponent"] * math.log1p(self.life[self.pr["currency"]])
        for c in self.cids if rw["target"] == "all" else (rw["target"],):
            terms.mul[c] += bonus

    def away_income(self):
        """Income with no active-only effect: what the game earns while the player is away."""
        if self._ainc is None:
            self._ainc = self._income_terms(self.away_sources, False)[0]
        return self._ainc

    def _final_income(self, c, terms):
        x = self.base_income[c] + terms.add[c]
        for s in terms.pct[c].values():
            x *= max(0.0, 1 + s)
        x = 0.0 if c in terms.zero else x * math.exp(min(terms.mul[c], 690.0))
        return min(x, BIG)

    def income(self):
        if self._inc is None:
            self._inc, self._parts = self.compute_income(parts=True)
        return self._inc

    def changed(self):
        self._inc = self._parts = self._w = self._ainc = None
        self.last_change = self.t

    def trial_income(self, pid):
        """Income with one more level of a purchase. Only the currencies that it touches are computed again."""
        out = dict(self.income())
        delta, acc = self._delta(pid), self._acc
        up = self._upkeep_delta(pid)
        for c in delta[4] | set(up):
            out[c] = self._trial_income_of(c, delta, acc, up.get(c, 0.0))
        return out

    def _upkeep_delta(self, pid, plus=None):
        """{target: change of ln u} for 1 more level of a need line or a care line (plus: and 1 more level of each line
        of the tuple plus); -inf when u falls to 0."""
        if pid not in self.need_lines and pid not in self.care_lines:
            return {}
        now, new = self.upkeep_u(), self.upkeep_u((pid,) + (plus or ()))
        if now <= 0 or new <= 0:
            return {self.up["target"]: 0.0 if now <= 0 and new <= 0 else (-math.inf if new <= 0 else 690.0)}
        return {self.up["target"]: math.log(new / now)}

    def care_unlock_gain(self, pid, inc, w):
        """The value of a care level that raises nothing now: the gain that it frees for the best open producer line,
        the gain of both levels over the gain of the producer alone (S30; a 2-purchase view of the buyer)."""
        return max([self._pair_gain(q, pid, inc, w) for q in self._open_need_lines(inc)], default=0.0)

    def _open_need_lines(self, inc):
        return [q for q in self.need_lines if self._line_open(q) and self.all_met(self.pmap[q]["requires"], inc)[0]]

    def _pair_gain(self, q, pid, inc, w):
        both = dict(inc)
        delta, up = self._delta(q), self._upkeep_delta(q, (pid,))
        for c in delta[4] | set(up):
            both[c] = self._trial_income_of(c, delta, self._acc, up.get(c, 0.0))
        pair = sum(currency_gain(both[c], inc[c], w[c]) for c in self.cids if both[c] != inc[c])
        return pair - max(0.0, self.rel_gain(q, inc, w))

    def _trial_income_of(self, c, delta, acc, dlog_u=0.0):
        add, pct, mul, zero = acc
        x = self.base_income[c] + add[c] + delta[0].get(c, 0.0)
        x = scale_by_pct(x, pct[c], delta[1].get(c, {}))
        off = (c in zero and dlog_u <= 0) or c in delta[3] or dlog_u == -math.inf
        return min(0.0 if off else x * math.exp(min(mul[c] + delta[2].get(c, 0.0) + dlog_u, 690.0)), BIG)

    # -- requirements -----------------------------------------------------------
    def quantity(self, req, inc):
        """(value now, rate of change) of the quantity that a requirement compares."""
        t = req["type"]
        if t == "total":
            return self.life[req["currency"]], self._rate(req["currency"], inc)
        if t == "run_total":
            return self.run[req["currency"]], self._rate(req["currency"], inc)
        if t == "have":
            return self.cur[req["currency"]], self._rate(req["currency"], inc)
        if t == "time":
            return self.t, 1.0
        if t == "rate":
            return inc[req["currency"]], None
        if t == "prestiges":
            return self.prestiges, None
        return None, None

    def met(self, req, inc):
        """Return (met_now, seconds_until_met_at_constant_rates)."""
        t = req["type"]
        if t == "level":
            return (self.level[req["id"]] >= req.get("level", 1), INF)
        if t == "milestone":
            return (req["id"] in self.done, INF)
        if t == "ladder":
            return (self.ladder_best[req["id"]] >= req.get("step", 1), INF)
        if t == "run_ladder":
            return (self.ladder_pos[req["id"]] >= req.get("step", 1), INF)
        v, r = self.quantity(req, inc)
        a = req.get("amount", 0)
        if v >= a:
            return (True, 0.0)
        return (False, (a - v) / r if r else INF)

    def _rate(self, c, inc):
        """The rate at which the balances of c grow: the income, or the earning rate while away."""
        return inc[c] if self._rates is None else self._rates[c]

    def all_met(self, reqs, inc):
        """Return (all met now, seconds until the slowest unmet requirement is met at constant rates)."""
        if not reqs:
            return True, 0.0
        waits = [w for ok, w in [self.met(r, inc) for r in reqs] if not ok]
        return not waits, reduce(max, waits, 0.0)

    def ladder_req(self, l, step):
        r = dict(l["requires"])
        try:
            r["amount"] = step_amount(r, step)
        except OverflowError:
            r["amount"] = INF
        return r

    def cap_of(self, p):
        if p["max_level"] is None:
            return None
        cap = p["max_level"]
        for kind, sid, v in self.capfx.get(p["id"], ()):
            cap += v * self.count(kind, sid)
        return math.floor(cap + 1e-9)

    # -- events -----------------------------------------------------------------
    def log(self, kind, ident, name, system, weight, cost=None, currency=None, note=None):
        if weight >= self.beat_weight and kind != "prestige":
            self.run_new_beats += 1
        self.events.append(dict(t=self.t, real=self.real, kind=kind, id=ident, name=name, system=system,
                                weight=weight, cost=cost, currency=currency, note=note, prestiges=self.prestiges,
                                income=dict(self.income()),
                                axis=self.ladder_best.get(self.axis, 0) if self.axis else None))
        if self.up:
            self.events[-1]["u"] = self.upkeep_u()

    def _add_to_balances(self, c, amt):
        self.cur[c] = min(self.cur[c] + amt, BIG)
        self.life[c] = min(self.life[c] + amt, BIG)
        self.run[c] = min(self.run[c] + amt, BIG)

    def gain(self, c, amt, source):
        self._add_to_balances(c, amt)
        self.earn[c][source] = self.earn[c].get(source, 0.0) + amt

    def grant(self, grants, system, step=1):
        source = f"grants: {system}"
        for g in [g for g in grants if g.get("count") is None or step <= g["count"]]:
            self.gain(g["currency"], grant_amount(g, step), source)

    def check_milestones(self):
        """Reach each milestone and ladder step whose requirements are met, until nothing more changes."""
        changed = True
        while changed:
            changed = self._reach_milestones() | self._climb_ladders()

    def _reach_milestones(self):
        reached = [self._try_milestone(ms) for ms in self.m["milestones"]]
        return any(reached)

    def _try_milestone(self, ms):
        if ms["id"] in self.done or not self.all_met(ms["requires"], self.income())[0]:
            return False
        self._reach_milestone(ms)
        return True

    def _reach_milestone(self, ms):
        first = ms["id"] not in self.ever_done
        self.done[ms["id"]] = self.t
        self.changed()
        (self._first_milestone if first else self._repeat_milestone)(ms)

    def _first_milestone(self, ms):
        self.ever_done[ms["id"]] = self.t
        self.grant(ms["grants"], ms["system"])
        self.log("milestone", ms["id"], ms["name"], ms["system"],
                 ms.get("excitement", WEIGHT.get(ms["kind"], 3)), note=ms["kind"])

    def _repeat_milestone(self, ms):
        self.log("milestone", ms["id"], ms["name"], ms["system"], WEIGHT["repeat"], note="repeat")

    def _climb_ladders(self):
        climbed = [self._climb_ladder(l) for l in self.m["ladders"]]
        return any(climbed)

    def _climb_ladder(self, l):
        steps = 0
        inc = self.income()
        req = self._next_step_req(l, inc)
        while req is not None:
            self._take_step(l, req, inc)
            steps += 1
            inc = self.income()
            req = self._next_step_req(l, inc)
        return steps > 0

    def _next_step_req(self, l, inc):
        """The requirement of the next step of a ladder when it is met now, else None."""
        if self.ladder_pos[l["id"]] >= l["count"]:
            return None
        req = self.ladder_req(l, self.ladder_pos[l["id"]] + 1)
        if not self.met(req, inc)[0]:
            self.attempt_from.pop(l["id"], None)
            return None
        return req if self._step_ready(l["id"]) else None

    def _step_ready(self, lid):
        """True when the timers and the presence of the player let a ladder take its met step now."""
        spec = self.timed.get(lid)
        if spec is None:
            return True
        attempt, cooldown, visit = spec
        if (visit and not self.present) or self.t < self.step_last.get(lid, -INF) + cooldown - 1e-9:
            self.attempt_from.pop(lid, None)
            return False
        return self.t >= self.attempt_from.setdefault(lid, self.t) + attempt - 1e-9

    def _timer_wait(self, lid):
        """Seconds until the timers of a ladder let its met step happen. INF when only a visit can."""
        spec = self.timed.get(lid)
        if spec is None:
            return INF
        attempt, cooldown, visit = spec
        if visit and not self.present:
            return INF
        ready = self.step_last.get(lid, -INF) + cooldown
        if self.t < ready:
            return ready - self.t
        return max(0.0, self.attempt_from.get(lid, self.t) + attempt - self.t)

    def _take_step(self, l, req, inc):
        """Go 1 step up a ladder. inc is the income before the step."""
        lid = l["id"]
        step = self.ladder_pos[lid] + 1
        self.ladder_pos[lid] = step
        self.step_last[lid] = self.t
        self.attempt_from.pop(lid, None)
        self.run_top[lid] = max(self.run_top[lid], step)
        new = step > self.ladder_best[lid]
        self.changed()
        if new:
            self._new_best_step(l, step, self.quantity(req, inc)[0])
        self.log("ladder", lid, l["name"].replace("{n}", str(step)), l["system"],
                 l.get("excitement", WEIGHT["ladder"]) if new else WEIGHT["repeat"],
                 note="new" if new else "repeat")

    def _new_best_step(self, l, step, value):
        self.ladder_best[l["id"]] = step
        self.steps[l["id"]][step] = (self.t, value)
        self.step_clock[l["id"]][step] = (self.real, self.active, self.t)
        self.grant(l["grants"], l["system"], step)

    def next_passive(self, inc):
        """Seconds until the next milestone, ladder step, or fixed run end that only needs time to pass."""
        waits = self._event_waits(inc)
        waits.append(self._run_end_wait())
        return reduce(min, waits, INF)

    def _event_waits(self, inc):
        waits = [self._milestone_wait(ms, inc) for ms in self.m["milestones"] if ms["id"] not in self.done]
        return waits + [self._step_wait(l, inc) for l in self.m["ladders"] if self.ladder_pos[l["id"]] < l["count"]]

    def _milestone_wait(self, ms, inc):
        ok, w = self.all_met(ms["requires"], inc)
        return w if not ok and w < INF and self._only_needs_time(ms["requires"], inc) else INF

    def _only_needs_time(self, reqs, inc):
        return all(self.met(r, inc)[0] or self.met(r, inc)[1] < INF for r in reqs)

    def _step_wait(self, l, inc):
        ok, w = self.met(self.ladder_req(l, self.ladder_pos[l["id"]] + 1), inc)
        return self._timer_wait(l["id"]) if ok else w

    def _run_end_wait(self):
        if self.pr and "run_seconds" in self.pr:
            return max(0.0, self.run_start + self.pr["run_seconds"] - self.t)
        return INF

    # -- the buyer --------------------------------------------------------------
    def candidates(self, inc):
        """(purchase, [(currency, cost)], seconds until affordable) for each purchase that is open now."""
        open_ = [p for p in self.m["purchases"] if self._is_open(p, inc) and not self.auto_on(p["id"])]
        return [self._candidate(p, inc) for p in open_]

    # -- the auto-buy (S24) -----------------------------------------------------
    def auto_on(self, pid):
        """True when the auto-buy of the line is open: the player does not buy the line by hand any more. The minute
        counts active play only, so a player who never plays does not open it (the game opens it from progress)."""
        if pid in self.auto_reqs:
            return self._auto_req_open(pid)
        return self.active >= self.auto_from.get(pid, INF) - 1e-9

    def _auto_req_open(self, pid):
        """An auto-buy with requires (a lifetime total or a stage) opens at that progress and stays open."""
        if pid not in self.auto_open and self.all_met(self.auto_reqs[pid], self.income())[0]:
            self.auto_open.add(pid)
        return pid in self.auto_open

    def _auto_candidates(self, inc):
        lines = [self.pmap[pid] for pid in self.auto_lines if self.auto_on(pid)]
        return [self._candidate(p, inc) for p in lines if self._is_open(p, inc)]

    def auto_wait(self, inc):
        """Seconds until the auto-buy can buy the next level or the next auto-buy opens."""
        if not self.auto_lines:
            return INF
        opens = [t - self.active for t in self.auto_from.values() if t > self.active + 1e-9 and self.present]
        return reduce(min, opens + [w for _, _, w in self._auto_candidates(inc)], INF)

    def auto_buy(self):
        """Buy each affordable level of the open auto-buy lines, the cheapest level first, as the game does."""
        bought = 0
        while bought < AUTO_BUY_LIMIT and self._auto_buy_one():
            bought += 1
        if bought:
            self.check_milestones()

    def _auto_buy_one(self):
        """Buy the cheapest affordable level of the open auto-buy lines. False when none is affordable."""
        pick = self._auto_pick()
        if pick:
            self.buy(pick[1], pick[2], self.income(), auto=True)
        return bool(pick)

    def _auto_pick(self):
        cands = self._auto_candidates(self.income()) if self.auto_lines else []
        now = [(costs[0][1], p["id"], p, costs) for p, costs, w in cands if w == 0]
        best = min(now, key=lambda x: x[:2], default=None)
        return best[1:] if best else None

    def _is_open(self, p, inc):
        cap = self.cap_of(p)
        if cap is not None and self.level[p["id"]] >= cap:
            return False
        return self.all_met(p["requires"], inc)[0]

    def _candidate(self, p, inc):
        st = self.pstat[p["id"]]
        if st["first_avail"] is None:
            st["first_avail"] = self.t
        costs = [(c["currency"], cost_one(c, self.level[p["id"]])) for c in p["costs"]]
        wait = reduce(max, [self._cost_wait(c, amt, inc) for c, amt in costs], 0.0)
        if wait == 0 and st["first_afford"] is None:
            st["first_afford"] = self.t
        return (p, costs, wait)

    def _cost_wait(self, c, amt, inc):
        have, rate = self.cur[c], inc[c]
        if have >= amt:
            return 0.0
        return (amt - have) / rate if rate > 0 else INF

    def weights(self, inc):
        """Weight of a relative gain on each currency. A stat that blocks a wall counts more."""
        if self._w is None:
            self._w = self._compute_weights(inc)
        return self._w

    def _compute_weights(self, inc):
        w = dict.fromkeys(self.cids, 1.0)
        for c in self._rate_walls(inc):
            w[c] = WALL_WEIGHT
        return w

    def _rate_walls(self, inc):
        """The currencies whose rate gates the next step of a ladder or an open milestone."""
        walls = [l["requires"]["currency"] for l in self.m["ladders"]
                 if self.ladder_pos[l["id"]] < l["count"] and l["requires"]["type"] == "rate"]
        reqs = [r for ms in self.m["milestones"] if ms["id"] not in self.done for r in ms["requires"]]
        return walls + [r["currency"] for r in reqs if r["type"] == "rate" and not self.met(r, inc)[0]]

    def rel_gain(self, pid, inc, w):
        new = self.trial_income(pid)
        gain = 0.0
        moved = [c for c in self.cids if new[c] > inc[c] or new[c] < inc[c]]
        for c in moved:
            gain += currency_gain(new[c], inc[c], w[c])
        return gain

    def wanted(self, pid):
        """True when a purchase, a milestone or the prestige still needs more levels of pid."""
        lv = self.level[pid]
        needs = [(kind, ident) for kind, ident, need in self.wanted_by.get(pid, ()) if lv < need]
        return any(self._still_needs(kind, ident) for kind, ident in needs)

    def _still_needs(self, kind, ident):
        if kind == "p":
            return self._line_open(ident)
        return kind != "m" or ident not in self.done

    def _line_open(self, pid):
        cap = self.cap_of(self.pmap[pid])
        return cap is None or self.level[pid] < cap

    def value(self, p, inc, w):
        pid = p["id"]
        v = self.rel_gain(pid, inc, w) if pid in self.has_income_fx else 0.0
        for tgt in self._raisable_caps(pid):
            v += self.rel_gain(tgt, inc, w)       # one more level of the capped line becomes possible
        if v <= 0 and self.wanted(pid):
            v = 1.0
        if v <= 0 and pid in self.care_lines:
            v = self.care_unlock_gain(pid, inc, w)      # care that raises nothing now: what it frees (S30)
            return max(v, 0.0) if v < SMALL_VALUE else v
        return 0.0 if v < 0 else max(v, SMALL_VALUE)    # 0: it lowers income and nothing needs it

    def _raisable_caps(self, pid):
        """The capped income lines that one more level of pid lets the buyer raise."""
        return [tgt for tgt, raise_ in self.cap_targets.get(pid, ()) if self._cap_blocks(tgt, raise_)]

    def _cap_blocks(self, tgt, raise_):
        cap = self.cap_of(self.pmap[tgt])
        return raise_ > 0 and cap is not None and self.level[tgt] >= cap and tgt in self.has_income_fx

    def advance(self, dt, inc, factor=1.0, play=True):
        earned = [(c, inc[c] * dt * factor) for c in self.cids]
        for c, d in earned:
            self._earn_income(c, d)
        self.real += dt if self.speed == 1.0 else dt / self.speed
        if play:
            self._spend_play_time(dt)

    def _spend_play_time(self, dt):
        self.t += dt
        self.block_left -= dt
        self.active += dt

    def _earn_income(self, c, d):
        if not d:
            return
        self._add_to_balances(c, d)
        self._book_income(c, d)

    def _book_income(self, c, d):
        """Share the income d among its sources in proportion to their flat adds."""
        parts = self._parts[c] if self._parts else None
        tot = sum(parts.values()) if parts else 0.0
        if tot > 0:
            return self._book_income_parts(self.earn[c], parts, d, tot)
        key = "income: " + BASE
        self.earn[c][key] = self.earn[c].get(key, 0.0) + d

    def _book_income_parts(self, e, parts, d, tot):
        shares = [("income: " + s, d * v / tot) for s, v in parts.items()]
        for k, x in shares:
            e[k] = e.get(k, 0.0) + x

    def buy(self, p, costs, inc, auto=False):
        pid = p["id"]
        self._pay(p, costs)
        before = inc[costs[0][0]]
        self.level[pid] += 1
        self.changed()
        first = pid not in self.ever
        self.ever.add(pid)
        self._record_purchase(p, costs, before, first)
        w = p.get("excitement", WEIGHT["first_purchase"]) if first else WEIGHT["purchase"]
        self.log("purchase", pid, f"{p['name']} L{self.level[pid]}", p["system"], w,
                 cost=costs[0][1], currency=costs[0][0], note="first" if first else None)
        self.events[-1].update({"auto": True} if auto else {})     # a purchase of the auto-buy (S24)

    def _pay(self, p, costs):
        st = self.pstat[p["id"]]
        for c, amt in costs:
            self._pay_cost(p, st, c, amt)

    def _pay_cost(self, p, st, c, amt):
        self.cur[c] = max(0.0, self.cur[c] - amt)
        self.spent[c][p["system"]] = self.spent[c].get(p["system"], 0.0) + amt
        st["spent"][c] = st["spent"].get(c, 0.0) + amt

    def _record_purchase(self, p, costs, before, first):
        """Update the purchase stats. before is the income of the first cost currency before the purchase."""
        st = self.pstat[p["id"]]
        delta = self.income()[costs[0][0]] - before
        pay = costs[0][1] / delta if delta > 0 else None
        if first:
            st["first"], st["pay_first"] = self.t, pay
        st["last"], st["pay_last"], st["bought"] = self.t, pay, st["bought"] + 1
        st["top"] = max(st["top"], self.level[p["id"]])
        self._record_cap(p, st)

    def _record_cap(self, p, st):
        cap = self.cap_of(p)
        if cap is not None and self.level[p["id"]] >= cap and st["cap_at"] is None:
            st["cap_at"] = self.t

    # -- prestige ---------------------------------------------------------------
    def prestige_raw(self):
        """The gain before rounding down."""
        basis_gain = self._ladder_basis_gain if "basis_ladder" in self.pr else self._currency_basis_gain
        try:
            return basis_gain()
        except OverflowError:
            return BIG

    def _ladder_basis_gain(self):
        pr = self.pr
        step = self.ladder_pos[pr["basis_ladder"]]
        if step <= 0:
            return 0.0
        grow = pr["growth"] ** step if pr["formula"] == "exp" else step ** pr["exponent"]
        return min(BIG, pr["k"] * grow)

    def _currency_basis_gain(self):
        pr = self.pr
        basis = self.run[pr["basis_currency"]] / pr["base"]
        if basis <= 0:
            return 0.0
        if pr["formula"] == "log":
            return max(0.0, pr["k"] * math.log10(max(basis, 1.0)))
        return min(BIG, pr["k"] * basis ** pr["exponent"])

    def try_prestige(self, inc):
        """Apply a reset when the buyer chooses one. Return True when it resets."""
        if not self.pr or self.no_reset or self.resets_used():
            return False
        elapsed = self.t - self.run_start
        raw = self.prestige_raw()
        gain = math.floor(raw + 1e-9) if self.pr.get("floor", True) else raw
        if not self._wants_reset(inc, elapsed, raw, gain):
            return False
        self._apply_reset(elapsed, gain)
        return True

    def resets_used(self):
        """True when the run has made the most resets of the game (prestige.max_resets)."""
        top = self.pr.get("max_resets")
        return top is not None and self.prestiges >= top

    def _wants_reset(self, inc, elapsed, raw, gain):
        pr = self.pr
        if "run_seconds" in pr:
            return not elapsed < pr["run_seconds"] - 1e-6
        rate = (gain if pr.get("peak_on_floor") else raw) / max(elapsed, 1.0)
        self.best_rate = max(self.best_rate, rate)
        if not self.all_met(pr["requires"], inc)[0] or elapsed < pr["min_run_seconds"] or gain < pr["min_gain"]:
            return False
        return self._policy_resets(rate, gain)

    def _policy_resets(self, rate, gain):
        pr = self.pr
        if pr["policy"] == "peak_rate":
            # no reset while the run still pays about as much each minute as at its best
            return not rate >= self.best_rate * pr["peak_tolerance"]
        return not gain < pr["policy_ratio"] * self.life[pr["currency"]]

    def _apply_reset(self, elapsed, gain):
        pr = self.pr
        self.prestiges += 1
        self.gain(pr["currency"], gain, "prestige")
        self.grant(pr.get("grants") or [], "prestige", self.prestiges)
        self._reset_progress(pr["resets"])
        self._close_run(elapsed, gain)
        new = self.run_new_beats > 0
        self._start_run(pr["resets"])
        self._log_prestige(elapsed, gain, new)

    def _reset_progress(self, rs):
        """Apply the losses of a reset: currencies, purchase levels, and milestones that do not persist."""
        for c in rs["currencies"]:
            self._lose_currency(c)
        lines = [p["id"] for p in self.m["purchases"] if p["system"] in rs["purchase_systems"]]
        for pid in lines:
            self.level[pid] = 0
        gone = [ms["id"] for ms in self.m["milestones"] if not ms["persists"]]
        for mid in gone:
            self.done.pop(mid, None)

    def _lose_currency(self, c):
        self.lost[c] += self.cur[c]
        self.cur[c] = 0.0

    def _close_run(self, elapsed, gain):
        self.runs.append(dict(run=self.prestiges, start_minutes=self.run_start / 60.0, minutes=elapsed / 60.0,
                              gain=gain, gain_per_minute=gain / max(elapsed / 60.0, 1e-9),
                              top_steps=dict(self.run_top), new_beats=self.run_new_beats))

    def _start_run(self, rs):
        for l in rs["ladders"]:
            self.ladder_pos[l] = 0
        self.run_top = dict(self.ladder_pos)
        self.run = dict.fromkeys(self.cur, 0.0)
        self.run_start = self.t
        self.best_rate = 0.0
        self.run_new_beats = 0
        self.changed()

    def _log_prestige(self, elapsed, gain, new):
        pr = self.pr
        weight = WEIGHT["prestige_first"] if self.prestiges == 1 else (WEIGHT["prestige"] if new else WEIGHT["repeat"])
        note = (f"+{gain:g} {pr['currency']}; run {elapsed / 60.0:.1f} min"
                + ("" if new or self.prestiges == 1 else "; nothing new in this run"))
        self.log("prestige", "prestige", f"Prestige {self.prestiges}", pr["system"], weight, note=note)

    # -- the loop ---------------------------------------------------------------
    def sample(self, inc, cands):
        now = sum(1 for _, _, w in cands if w == 0)
        soon = sum(1 for _, _, w in cands if w <= 300)
        caps = [(p, self.cap_of(p)) for p in self.m["purchases"]]
        capped = [self.level[p["id"]] >= cap for p, cap in caps if cap is not None]
        self.samples.append(dict(t=self.t, income=dict(inc), now=now, soon=soon, capped=sum(capped),
                                 lines=len(capped), total_lines=len(self.m["purchases"]), wall=self._wall(inc)))

    def _wall(self, inc):
        """(ladder id, requirement type, ratio of the next step to the quantity now) of the progress ladder."""
        l = next((l for l in self.m["ladders"]
                  if (self.axis in (None, l["id"])) and self.ladder_pos[l["id"]] < l["count"]), None)
        if l is None:
            return None
        req = self.ladder_req(l, self.ladder_pos[l["id"]] + 1)
        v = self.quantity(req, inc)[0]
        return (l["id"], req["type"], req["amount"] / v if v and v > 0 else INF)

    def stall(self, name, note):
        self.log("stall", "stall", name, "-", 0, note=note)

    def play(self):
        self.check_milestones()
        going = True
        while going and self._clock() < self.limit - 1e-9:
            going = self._play_step()
        return self

    def _axis_id(self):
        return self.axis or (self.m["ladders"][0]["id"] if len(self.m["ladders"]) == 1 else None)

    def axis_done(self):
        ax = self._axis_id()
        return ax is not None and self.ladder_best[ax] >= self.lmap[ax]["count"]

    def end_reached(self):
        """True when the hard end event of game.targets.hard_end happened: a step of the axis, the first purchase of
        a line, a count of resets, or a milestone. It records the clocks of the first such check in end_real and
        end_clock (real, active, game)."""
        if self.end_real is None and self.end_mark and self._end_event():
            self.end_real, self.end_clock = self.real, (self.real, self.active, self.t)
        return self.end_real is not None

    def end_stops(self):
        """True when the hard end of the model is of a kind of STOP_KINDS: the game has no more play after it."""
        return bool(self.end_mark) and next(iter(self.end_mark)) in STOP_KINDS

    def _end_event(self):
        k, v = next(iter(self.end_mark.items()))
        return END_TESTS.get(k, _never)(self, v)

    def _at_stop(self):
        """True when the run must stop: the end of the axis, a hard end of a kind of STOP_KINDS, or the step of
        stop_at_step. A hard end of kind axis_step or purchase does not stop the run (the solver plays to the end of
        the axis); its time is in end_real."""
        stops = self.end_reached() and self.end_stops()
        if self.stop_at_end and (stops or self.axis_done()):
            return True
        ax = self._axis_id()
        return ax is not None and bool(self.stop_at_step) and self.ladder_best[ax] >= self.stop_at_step

    def _play_step(self):
        """Run 1 step of the loop. Return False when the run stops."""
        if len(self.events) >= self.max_events:
            return self._cut_run()
        if self._at_stop():
            return False
        if self.away > 0 and self.block_left <= 1e-9:
            return self._spend_away_time()
        inc = self.income()
        if self.try_prestige(inc):
            self.check_milestones()
            return True
        return self._shop_step(inc)

    def _cut_run(self):
        self.cut = True
        self.stall("Event limit reached", event_limit_note(self.max_events))
        return False

    def _spend_away_time(self):
        if self.player is not None:
            return self._player_away()
        self.advance(self.away, self.income(), factor=self.offline, play=False)
        self.block_left = self.session
        self.check_milestones()     # beats that happened while away are met now
        return True

    # -- presence: the time between 2 visits ----------------------------------------
    def _player_away(self):
        self.present = False
        (self.away_open if self.between == "open" else self.away_closed)(self.away)
        self.start_visit()
        return True

    def away_open(self, seconds):
        """The game runs with no input. It pays open_rate up to the open cap, then nothing."""
        cap = self.pres["open_cap_minutes"]
        paid = seconds if cap is None else min(seconds, cap * 60.0)
        self._away_span(paid, self.pres["open_rate"], True)
        self._away_span(seconds - paid, 0.0, True)

    def away_closed(self, seconds):
        """The game is shut. It pays offline_rate up to the offline cap. The game clock stops."""
        cap = self.pres["offline_cap_minutes"]
        paid = seconds if cap is None else min(seconds, cap * 60.0)
        self._away_span(paid, self.offline, False)
        self.real += seconds - paid

    def _away_span(self, seconds, factor, running):
        """Earn at factor x the away income for seconds, up to each event that only needs time."""
        end = self.real + seconds
        steps = iter(range(AWAY_STEP_LIMIT))         # at most AWAY_STEP_LIMIT steps
        while next(steps, None) is not None and not self._away_over(end):
            self._away_step(min(end - self.real, self._away_wait(factor, running)), factor, running)

    def _away_over(self, end):
        return self.real >= end - 1e-9 or len(self.events) >= self.max_events

    def _away_wait(self, factor, running):
        ainc = self.away_income()
        self._rates = {c: 0.0 if c in self.bank_caps else ainc[c] * factor for c in self.cids}
        try:
            auto = [self.auto_wait(self._rates)] if running else []
            w = reduce(min, self._event_waits(self.income()) + [self._reset_wait()] + auto, INF)
        finally:
            self._rates = None
        return w * (1 + 1e-12) + 1e-9

    def _away_step(self, dt, factor, running):
        ainc = self.away_income()
        for c in self.cids:
            self._away_earn(c, ainc[c] * dt * factor)
        self.real += dt
        self.t += dt if running else 0.0
        self.check_milestones()
        self.end_reached()
        if running:
            self.auto_buy()         # the game runs, so its auto-buy buys
        if self.pr and not self.pr.get("needs_visit", True) and self.try_prestige(self.income()):
            self.check_milestones()

    def _away_earn(self, c, d):
        if c not in self.bank_caps:
            return self._earn_income(c, d)
        self.bank[c] = min(self.bank_caps[c], self.bank[c] + d)

    def _reset_wait(self):
        """Seconds until the reset rule fires while away. INF when the reset needs a visit."""
        pr = self.pr
        if not pr or pr.get("needs_visit", True) or "basis_currency" not in pr or pr["formula"] != "power":
            return INF
        target = max(pr["min_gain"], pr["policy_ratio"] * self.life[pr["currency"]] if pr["policy"] == "ratio" else 0)
        need = pr["base"] * (target / max(pr["k"], 1e-300)) ** (1.0 / max(pr["exponent"], 1e-9))
        rate = self._rates[pr["basis_currency"]]
        return max(0.0, need - self.run[pr["basis_currency"]]) / rate if rate > 0 else INF

    def start_visit(self):
        """The player comes back: it collects each bank, and the session starts."""
        self.present = True
        self.visits += 1
        for c in self.bank_caps:
            self.gain(c, self.bank[c], "bank")
            self.bank[c] = 0.0
        self.block_left = self.session
        self.changed()
        self.check_milestones()

    def _shop_step(self, inc):
        """Pick a purchase, advance time, and buy. Return False when the buyer stalls."""
        cands = self.candidates(inc)
        self.sample(inc, cands)
        best = self.pick_purchase(inc, cands)
        passive = min(self.next_passive(inc), self.auto_wait(inc))
        reason = self._stall_reason() if best is None and passive == INF else None
        if reason:
            self.stall(*reason)
            return False
        self._advance_and_buy(inc, best, passive)
        return True

    def _stall_reason(self):
        if not self.pr:
            return STALL_NO_CONTENT
        if self.t - self.last_change >= max(3600.0, 0.5 * self.last_change):
            return STALL_NO_GOAL
        return None

    def pick_purchase(self, inc, cands):
        """The purchase that the buyer goes for next: (priority, purchase, costs, wait), or None."""
        if self.buyer == "payback":
            return self._payback_pick(inc, cands)
        w = self.weights(inc)
        best = self._best_target(inc, cands, w)
        if best and best[3] > 0:
            best = self._side_purchase(inc, cands, w, best) or best
        return best

    def _best_target(self, inc, cands, w):
        scored = [(self._priority(p, inc, w, wait), p, costs, wait) for p, costs, wait in cands]
        scored = [x for x in scored if x[0] is not None]
        return min(scored, key=lambda x: x[0], default=None)

    def _priority(self, p, inc, w, wait):
        """(wait + 1) / value. Lower is better. None when the buyer cannot or does not buy it."""
        if wait == INF:
            return None
        v = self.value(p, inc, w)
        block = self._block(p, inc)
        if block and block[1] < INF:
            return min((block[1] + 1.0) / block[2], (wait + 1.0) / v if v > 0 else INF)
        return None if v <= 0 else (wait + 1.0) / v

    def _side_purchase(self, inc, cands, w, target):
        """While the buyer saves for the target, it buys what the target does not pay with."""
        side = self._side_options(inc, cands, w, {c for c, _ in target[2]})
        if not side:
            return None
        _, p, costs = max(side, key=lambda x: x[0])
        return (0.0, p, costs, 0.0)

    def _side_options(self, inc, cands, w, saving):
        """(value, purchase, costs) of each purchase that is affordable now and does not pay with saving."""
        side = [(self.value(p, inc, w), p, costs) for p, costs, wait in cands
                if wait == 0 and not saving & {c for c, _ in costs}]
        return [x for x in side if x[0] > 0]

    # -- the payback buyer (the buyer of the measured game, tests/pacing_buyer.gd) ---------------
    def _payback_pick(self, inc, cands):
        """Lines that cost another currency: the cheapest affordable one first. Then the main-currency line
        with the lowest wait + cost / value. value = main income x the sum of the relative income gains."""
        side = [(costs[0][1], p, costs, w) for p, costs, w in cands if costs[0][0] != self.main]
        return self._side_now(side) or self._main_pick(inc, cands) or self._first_side(side)

    @staticmethod
    def _side_now(side):
        """The cheapest affordable line of another currency, or None."""
        now = [x for x in side if x[3] == 0]
        if not now:
            return None
        _, p, costs, _w = min(now, key=_first)
        return (0.0, p, costs, 0.0)

    def _main_pick(self, inc, cands):
        scored = [self._payback_score(p, costs, w, inc) for p, costs, w in cands if costs[0][0] == self.main]
        return min([x for x in scored if x is not None], key=_first, default=None)

    def _first_side(self, side):
        reachable = [x for x in side if x[3] < INF]
        if not reachable:
            return None
        _, p, costs, w = min(reachable, key=lambda x: x[3])
        return (w, p, costs, w)

    def _payback_score(self, p, costs, wait, inc):
        if wait == INF:
            return None
        rel = self._relative_gain(p["id"], inc)
        if rel < 0:
            return None
        value = max(inc[self.main] * rel, 1e-3)
        return (min(wait + costs[0][1] / value, self._block_score(p, inc)), p, costs, wait)

    def _block_score(self, p, inc):
        """A line with count steps: the payback of the block of levels up to the next step, with the factor of the
        step (the buyer plans toward the step, as a player does). INF with no block."""
        block = self._block(p, inc)
        if not block:
            return INF
        cost, wait, rel = block
        return wait + cost / (inc[self.main] * rel)

    def _block(self, p, inc):
        """(cost, seconds until affordable, relative income gain) of the levels of a line up to its next count step.
        None with no count step, a cap before the step, or no gain."""
        left = self._block_levels(p)
        if not left:
            return None
        delta, c0 = self._delta(p["id"], left), p["costs"][0]
        cost = block_cost(c0, self.level[p["id"]], left)
        rel = sum(self._trial_income_of(c, delta, self._acc) / inc[c] - 1.0 for c in delta[0] if inc[c] > 0)
        return (cost, self._cost_wait(c0["currency"], cost, inc), rel) if rel > 0 else None

    def _block_levels(self, p):
        """The count of levels of a line up to its next count step; 0 with no count step or a cap before the step."""
        steps, n, cap = self.count_steps.get(p["id"]), self.level[p["id"]], self.cap_of(p)
        left = steps["every"] - n % steps["every"] if steps else 0
        return 0 if cap is not None and n + left > cap else left

    def _relative_gain(self, pid, inc):
        """Sum over currencies of the relative rise of income from 1 more level. 1.0 for a needed unlock."""
        rel = 0.0
        if pid in self.has_income_fx:
            new = self.trial_income(pid)
            rel = sum((new[c] - inc[c]) / max(inc[c], 1e-3) for c in self.cids if new[c] != inc[c])
        return 1.0 if rel <= 0 and self.wanted(pid) else rel

    def _advance_and_buy(self, inc, best, passive):
        wait = self._decision_wait(best[3]) if best else INF
        dt = self._next_dt(wait, passive)
        if dt > 0:
            self.advance(dt, inc)
        if best and dt >= wait:
            self.buy(best[1], best[2], inc)
        self.auto_buy()
        self.check_milestones()

    def _decision_wait(self, wait):
        """Seconds to the first decision of the player at or after the wait. No decision interval: the wait."""
        if self.decide <= 0 or wait == INF:
            return wait
        return math.ceil((self.t + wait) / self.decide - 1e-9) * self.decide - self.t

    def _next_dt(self, wait, passive):
        """Seconds to advance: to the target, the next passive event, the step cap, the end, or the session end."""
        step_cap = max(60.0, 0.05 * (self.t - self.run_start))
        dt = min(wait, passive * (1 + 1e-12) + 1e-9, step_cap, self.limit - self._clock())
        return min(dt, max(self.block_left, 0.0)) if self.away > 0 else dt
