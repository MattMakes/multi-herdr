#!/usr/bin/env python3
"""Numeric checks for docs/skill-v2/balance-math.md.

Each law L1 to L9 has 1 check. A check runs a small simulation with a greedy
buyer and compares the result with the formula of the law. The script prints
1 line for each law, then the worked examples.

Python 3 standard library only. Run from the repository root:
python3 scripts/verify_math.py
"""

import json
import math
import os
import sys
import time

LN2 = math.log(2.0)
HOUR = 3600.0
DAY = 24 * HOUR


class Line:
    """A purchase line. Level k costs a * b**k.

    kind "add": the line adds v * k to its group.
    kind "mult": the line adds v**k - 1 to its group, so a group of 1 line is v**k.
    steps (n, m): count steps of an "add" line (L11): it adds v * k * m**floor(k / n).
    """

    def __init__(self, a, b, kind, v, group, cap=None, steps=None):
        self.a, self.b, self.kind, self.v, self.group, self.cap = a, b, kind, v, group, cap
        self.steps = steps

    def term(self, k):
        """The value that k levels add to the group."""
        if self.kind != "add":
            return self.v ** k - 1.0
        return self.v * k * (self.steps[1] ** (k // self.steps[0]) if self.steps else 1.0)

    def r(self):
        return math.log(self.v) / math.log(self.b) if self.kind == "mult" else 0.0


GAME_OPTIONS = dict(
    spine=None,
    reset=None,
    f_open=1.0,
    f_closed=1.0,
    cap_closed=None,
    time_mult=None,
)


def game_options(opts):
    """Return the Game options: the defaults with opts on top. Refuse an unknown name."""
    unknown = [name for name in opts if name not in GAME_OPTIONS]
    if unknown:
        raise TypeError("Game.__init__() got an unexpected keyword argument %r" % unknown[0])
    return dict(GAME_OPTIONS, **opts)


class Game:
    """Income = base * prod over groups of (group base + sum of terms) ** group exponent * M.

    M = (1 + L) ** q is the reward of the reset layer. Spine stage n needs power >= h * w ** (n - 1).
    Power is the income at the active rate.
    """

    def __init__(self, lines, groups, base=1.0, gexp=None, **opts):
        self.lines = lines
        self.groups = groups
        self.gexp = gexp or {}
        self.base = base
        self.__dict__.update(game_options(opts))


class PlayRule:
    """The options of Sim.play that stay the same for each decision."""

    def __init__(self, dt, X, gmin, count_active, f):
        self.dt, self.X, self.gmin, self.count_active, self.f = dt, X, gmin, count_active, f


class Sim:
    def __init__(self, game):
        self.g = game
        self.lv = [0] * len(game.lines)
        self.cash = 0.0
        self.E = 0.0
        self.S = 0.0
        self.L = 0.0
        self.t = 0.0
        self.run_t0 = 0.0
        self.hi = 0
        self.vhi = 0.0
        self.unlocks = []
        self.resets = []
        self.active_s = 0.0
        self.P = self._power(self.lv)

    def mult(self):
        r = self.g.reset
        return (1.0 + self.L) ** r["q"] if r else 1.0

    def _power(self, lv):
        g = self.g
        vals = dict(g.groups)
        for k, ln in zip(lv, g.lines):
            if k:
                vals[ln.group] += ln.term(k)
        p = g.base * self.mult()
        for name, val in vals.items():
            p *= val ** g.gexp.get(name, 1.0)
        if g.time_mult:
            p *= g.time_mult(self.t - self.run_t0)
        return p

    def vstage(self, p=None):
        """Stage as a real number, with no cap (progress unit)."""
        h, w, _ = self.g.spine
        p = self.P if p is None else p
        return 1.0 + math.log(p / h) / math.log(w) if p >= h else 0.0

    def _spine(self):
        if not self.g.spine:
            return
        self.vhi = max(self.vhi, self.vstage())
        n = min(self.g.spine[2], int(math.floor(self.vstage() + 1e-9)))
        while self.hi < n:
            self.hi += 1
            self.unlocks.append((self.t, self.hi))

    def best(self):
        best = (None, 0.0, 0.0)
        lines = self.g.lines
        for j, ln in enumerate(lines):
            best = self._better(best, j, ln)
        return best[0], best[2]

    def _better(self, best, j, ln):
        """Return (j, efficiency, cost) of line j if it beats best, else best."""
        eff, cost = self._efficiency(j, ln)
        return (j, eff, cost) if eff > best[1] else best

    def _efficiency(self, j, ln):
        """Return (ln power gain / cost, cost) of 1 more level. A capped line returns (0.0, 0.0)."""
        if ln.cap is not None and self.lv[j] >= ln.cap:
            return 0.0, 0.0
        cost = ln.a * ln.b ** self.lv[j]
        self.lv[j] += 1
        p2 = self._power(self.lv)
        self.lv[j] -= 1
        return math.log(p2 / self.P) / cost, cost

    def buy_all(self):
        n = 0
        while self._buy_one():
            n += 1
        return n

    def _buy_one(self):
        """Buy the best line if the cash pays for it. Return True after a purchase."""
        j, cost = self.best()
        if j is None or cost > self.cash:
            return False
        self.cash -= cost
        self.S += cost
        self.lv[j] += 1
        self.P = self._power(self.lv)
        self._spine()
        return True

    def earn(self, dt, f=1.0):
        if self.g.time_mult:
            return self._earn_steps(dt, f)
        self._add_income(self.P * f * dt, dt)

    def _earn_steps(self, dt, f):
        """Integrate a second clock in small steps."""
        steps = max(1, int(dt / 10.0))
        h = dt / steps
        for _ in range(steps):
            self._add_income(self.P * f * h, h)
            self.P = self._power(self.lv)

    def _add_income(self, x, dt):
        self.cash += x
        self.E += x
        self.t += dt

    def gain(self):
        r = self.g.reset
        if not r:
            return 0.0
        G = (self.E / r["c"]) ** r["p"]
        return math.floor(G) if r.get("floor") else G

    def e_for_gain(self, G):
        r = self.g.reset
        return r["c"] * G ** (1.0 / r["p"])

    def do_reset(self):
        G = self.gain()
        self.L += G
        self.resets.append((self.t, G, self.L, self.t - self.run_t0))
        self.lv = [0] * len(self.g.lines)
        self.cash = self.E = self.S = 0.0
        self.run_t0 = self.t
        self.P = self._power(self.lv)
        self._spine()

    def reset_target(self, X, gmin=1.0):
        return max(X * self.L, gmin)

    def play(self, until, dt=1.0, X=None, gmin=1.0, count_active=True, max_resets=None, f=1.0):
        """Active greedy play to game time `until`. Decision each dt seconds.

        X: reset when gain >= max(X * L, gmin). None: never reset.
        """
        rule = PlayRule(dt, X, gmin, count_active, f)
        while self.t < until - 1e-9 and not self._reset_cap(max_resets):
            self._turn(until, rule)

    def _reset_cap(self, max_resets):
        return max_resets is not None and len(self.resets) >= max_resets

    def _uses_reset(self, rule):
        return rule.X is not None and self.g.reset

    def _turn(self, until, rule):
        """1 decision: reset if the rule says so, buy, then wait to the next decision."""
        if self._uses_reset(rule) and self.gain() >= self.reset_target(rule.X, rule.gmin):
            self.do_reset()
        self.buy_all()
        wait = self._wait(until, rule)
        self.earn(wait, rule.f)
        if rule.count_active:
            self.active_s += wait

    def _wait(self, until, rule):
        wait = min(self._buy_wait(rule.f), self._reset_wait(rule))
        wait = max(rule.dt, math.ceil(wait / rule.dt - 1e-9) * rule.dt)
        return min(wait, until - self.t)

    def _buy_wait(self, f):
        j, cost = self.best()
        return math.inf if j is None else max(cost - self.cash, 0.0) / (self.P * f)

    def _reset_wait(self, rule):
        if not self._uses_reset(rule):
            return math.inf
        need = self.e_for_gain(self.reset_target(rule.X, rule.gmin)) - self.E
        return max(need, 0.0) / (self.P * rule.f)

    def idle(self, T, f=None):
        self.earn(T, self.g.f_open if f is None else f)

    def closed(self, T):
        g = self.g
        eff = T if g.cap_closed is None else min(T, g.cap_closed)
        self.earn(eff, g.f_closed)
        self.t += T - eff


RESULTS = []


def report(law, ok, text, approx=None):
    tag = "PASS" if ok else ("APPROXIMATE" if approx else "FAIL")
    line = "%s %s %s" % (law, tag, text)
    if approx and not ok:
        line += " | measured error: " + approx
    RESULTS.append((law, tag))
    print(line)


def rel(a, b):
    return abs(a - b) / max(abs(b), 1e-300)


def mline(r, b=2.0, a=10.0, group=None, cap=None):
    """A multiplier line with exponent r and cost ratio b."""
    return Line(a, b, "mult", b ** r, group, cap)


def simple_game(rs, base=1.0, **kw):
    """1 multiplier line in its own group for each exponent in rs."""
    lines = [mline(r, group="g%d" % i, a=10.0 * (i + 1)) for i, r in enumerate(rs)]
    groups = {"g%d" % i: 1.0 for i in range(len(rs))}
    return Game(lines, groups, base=base, **kw)


def spend_slope(game, s_lo, s_hi):
    """Greedy buyer with a given total spend. Returns d ln P / d ln S between s_lo and s_hi."""
    sim = Sim(game)
    pts = []
    s = 1.0
    while s <= s_hi * 1.0001:
        spend_point(sim, s, s_lo, pts)
        s *= 1.5
    return fit(pts)


def spend_point(sim, s, s_lo, pts):
    """Raise the total spend to s and buy. Add (ln spend, ln P) to pts from s_lo."""
    sim.cash += s - (sim.S + sim.cash)
    sim.buy_all()
    point = (math.log(sim.S + sim.cash), math.log(sim.P))
    if s >= s_lo:
        pts.append(point)


def growth_exponent(sim, t_end, t_min):
    """Play in steps of 30% to t_end. Return d ln P / d ln t from t_min."""
    pts = []
    while sim.t < t_end:
        sim.play(sim.t * 1.3 + 10)
        time_point(sim, t_min, pts)
    return fit(pts)


def time_point(sim, t_min, pts):
    point = (math.log(sim.t), math.log(sim.P))
    if sim.t > t_min:
        pts.append(point)


def fit(pts):
    n = len(pts)
    mx = sum(x for x, _ in pts) / n
    my = sum(y for _, y in pts) / n
    sxx = sum((x - mx) ** 2 for x, _ in pts)
    sxy = sum((x - mx) * (y - my) for x, y in pts)
    return sxy / sxx


def ell(sim):
    """Reset progress: ln M = q * ln(1 + L)."""
    return sim.g.reset["q"] * math.log(1.0 + sim.L)


def loop_game(rs, p, q, c, w=2.0, **kw):
    return simple_game(rs, reset=dict(c=c, p=p, q=q), spine=(1.0, w, 10 ** 9), **kw)


def traj_A(game, alpha, x=1e6):
    """Constant A of the L2 trajectory E(x) = A * x**alpha, from 1 run with no reset."""
    g = Game(game.lines, game.groups, base=game.base, gexp=game.gexp)
    sim = Sim(g)
    sim.play(x)
    return sim.E / x ** alpha


def visit(sim, X, v=120.0):
    """A visit: spend the bank, reset if the rule says so, then play v seconds."""
    sim.buy_all()
    sim._spine()
    sim.play(sim.t + v, X=X)


def check_l1():
    lines = [mline(0.45, b=1.5, group="a"), mline(0.38, b=2.0, group="b"), mline(0.24, b=3.0, group="b")]
    game = Game(lines, {"a": 1.0, "b": 1.0})
    pred = 0.45 + 0.38
    meas = spend_slope(game, 1e8, 1e16)
    ok1 = rel(meas, pred) < 0.03
    a, b, S = 10.0, 1.15, 1e9
    sim = Sim(Game([Line(a, b, "add", 1.0, "g")], {"g": 1.0}))
    sim.cash = S
    sim.buy_all()
    lvl_pred = math.floor(math.log(S * (b - 1) / a + 1) / math.log(b))
    ok2 = sim.lv[0] == lvl_pred
    report("L1", ok1 and ok2, "R predicted %.3f, simulated %.3f (tolerance 3%%); additive level predicted %d, simulated %d (exact)"
           % (pred, meas, lvl_pred, sim.lv[0]))


def check_l2():
    parts = [l2_polynomial(), l2_exponential(), l2_blow_up()]
    report("L2", all(ok for ok, _ in parts), "; ".join(text for _, text in parts))


def l2_polynomial():
    """R = 0.5 in 2 groups, income ~ t ** (R / (1 - R)) = t ** 1."""
    k_meas = growth_exponent(Sim(simple_game([0.25, 0.25])), 2e7, 1e4)
    return rel(k_meas, 1.0) < 0.05, "polynomial exponent predicted 1.000, simulated %.3f (5%%)" % k_meas


def l2_exponential():
    """1 line with b = d. Rate g = I0 ln d / a (discrete, exact)."""
    a, d = 10.0, 1.1
    sim = Sim(Game([Line(a, d, "mult", d, "g")], {"g": 1.0}))
    sim.play(100.0)
    l0, t0 = math.log(sim.P), sim.t
    sim.play(1100.0)
    g_meas = (math.log(sim.P) - l0) / (sim.t - t0)
    g_pred = math.log(d) / a
    return (rel(g_meas, g_pred) < 0.02,
            "exponential rate predicted %.5f/s, simulated %.5f/s (2%%)" % (g_pred, g_meas))


def l2_blow_up():
    """Finite time: R > 1. Blow-up time t* = (a / I0) * d / (d - b)."""
    a, b, d = 10.0, 1.1, 1.2
    sim = Sim(Game([Line(a, b, "mult", d, "g")], {"g": 1.0}))
    while sim.lv[0] < 300:
        sim.play(sim.t + 0.01, dt=0.01)
    ts_pred = a * d / (d - b)
    return (rel(sim.t, ts_pred) < 0.01,
            "blow-up time predicted %.1f s, simulated %.1f s at 300 levels (1%%)" % (ts_pred, sim.t))


def check_l3():
    parts = [l3_polynomial(0.5, 2.0), l3_loop(0.5, 2.0)]
    report("L3", all(ok for ok, _ in parts), "; ".join(text for _, text in parts))


def l3_polynomial(R, w):
    """Polynomial economy: stages per doubling of time = (R / (1 - R)) ln 2 / ln w."""
    sim = Sim(simple_game([0.25, 0.25], spine=(1.0, w, 10 ** 9)))
    sim.play(1e4)
    n0 = sim.vstage()
    sim.play(16e4)
    per_doubling = (sim.vstage() - n0) / 4.0
    pred_a = R / (1 - R) * LN2 / math.log(w)
    return (abs(per_doubling - pred_a) < 0.1,
            "stages per doubling of time predicted %.3f, simulated %.3f (0.1 stage)" % (pred_a, per_doubling))


def l3_loop(R, w):
    """Reset loop with lambda = 1: stages per hour = alpha * q * ln(1 + X) / (T_X * ln w)."""
    q, X = 0.5, 1.0
    alpha = 1.0 / (1.0 - R)
    game = loop_game([0.25, 0.25], 1.0, q, 1e3, w=w)
    rate_pred = alpha * q * math.log(1 + X) / (l3_run_time(game, alpha, X) * math.log(w)) * HOUR
    sim = Sim(game)
    sim.play(3 * HOUR, X=X)
    v0 = sim.vhi
    sim.play(6 * HOUR, X=X)
    rate_meas = (sim.vhi - v0) / 3.0
    return (rel(rate_meas, rate_pred) < 0.05,
            "reset loop stages per hour predicted %.2f, simulated %.2f (5%%)" % (rate_pred, rate_meas))


def l3_run_time(game, alpha, X, p=1.0, c=1e3):
    """Predicted run time T_X = X ** (1 / s) * (c / A) ** (1 / alpha)."""
    s = p * alpha
    A = traj_A(game, alpha)
    return X ** (1.0 / s) * (c / A) ** (1.0 / alpha)


def l4_state():
    lines = [mline(0.25, b=1.1, group="g0"), mline(0.25, b=1.1, group="g1", a=20.0)]
    sim = Sim(Game(lines, {"g0": 1.0, "g1": 1.0}, spine=(1.0, 2.0, 10 ** 9)))
    sim.play(HOUR)
    return sim


def idle_gain(base, T, f=1.0, bank_cap=None):
    s = Sim.__new__(Sim)
    s.__dict__ = dict(base.__dict__, lv=list(base.lv), unlocks=list(base.unlocks), resets=list(base.resets))
    p0 = s.P
    bank = p0 * f * T if bank_cap is None else min(p0 * f * T, bank_cap)
    s.cash += bank
    s.E += bank
    s.t += T
    s.buy_all()
    return math.log(s.P / p0)


L4_TIMES = (HOUR, 8 * HOUR, DAY, 7 * DAY, 30 * DAY)


def check_l4():
    base = l4_state()
    tau = base.S / base.P
    rows = [l4_row(base, tau, T) for T in L4_TIMES]
    worst = l4_worst(rows)
    shape_ok = l4_shape(rows)
    report("L4", worst < 0.10 and shape_ok, "worst error of idle and active gains against the formulas %.1f%% (10%%); idle gain "
           "positive, rising, below active: %s; ratio %s" % (worst * 100, shape_ok, l4_ratios(rows)))
    return base, tau


def l4_worst(rows):
    return max([0.0] + [max(rel(gi, pi), rel(ga, pa)) for _, gi, ga, pi, pa in rows])


def l4_ratios(rows):
    return ", ".join("%gh %.2f" % (T / HOUR, gi / ga) for T, gi, ga, _, _ in rows)


def copy_sim(base):
    """A copy of base with its own levels and empty unlock and reset lists."""
    a = Sim.__new__(Sim)
    a.__dict__ = dict(base.__dict__, lv=list(base.lv), unlocks=[], resets=[])
    return a


def l4_row(base, tau, T):
    """Return (T, idle gain, active gain, predicted idle gain, predicted active gain)."""
    gi = idle_gain(base, T)
    a = copy_sim(base)
    a.play(base.t + T, dt=10.0)
    ga = math.log(a.P / base.P)
    return (T, gi, ga) + l4_pred(base, tau, T)


def l4_pred(base, tau, T, R=0.5):
    pi = R * math.log(1 + (T + base.cash / base.P) / tau)
    pa = R / (1 - R) * math.log(1 + (1 - R) * T / tau)
    return pi, pa


def l4_shape(rows):
    """True when each idle gain is positive, below the active gain, and above the gain before it."""
    prevs = [0.0] + [gi for _, gi, _, _, _ in rows]
    return all(0 < gi < ga and gi > prev for prev, (_, gi, ga, _, _) in zip(prevs, rows))


L5_T1, L5_T2 = DAY, 4 * DAY
L5_R = 0.5


def l5_slope(g1, g2):
    return (g2 - g1) / math.log(L5_T2 / L5_T1)


def check_l5(base, tau):
    base_meas = [idle_gain(base, T) for T in (L5_T1, L5_T2)]
    s0 = l5_slope(*base_meas)
    graded = [l5_grade(tool, s0) for tool in l5_tools(base, tau, base_meas)]
    worst = max([0.0] + [err for err, _ in graded])
    out = [row for _, row in graded] + [l5_reset()]
    ok = worst < 0.10 and all(o[1] for o in out)
    report("L5", ok, "worst gain error %.1f%% (10%%); classes: %s" % (worst * 100, "; ".join(o[2] for o in out)))


def l5_tools(base, tau, base_meas):
    """(name, measured gains, predicted gains, expected class) for each idle tool."""
    return [l5_rate(base, tau), l5_time_cap(base, tau), l5_bank_cap(base, tau), l5_fight(base_meas)]


def l5_rate(base, tau):
    """Rate below 1 (and an active income term a = 2, which is the same math with f = 1 / a)."""
    return ("rate f=0.5", [idle_gain(base, T, f=0.5) for T in (L5_T1, L5_T2)],
            [L5_R * math.log(1 + 0.5 * T / tau) for T in (L5_T1, L5_T2)], "scale")


def l5_time_cap(base, tau):
    """Hard cap on banked time, 12 h."""
    return ("time cap 12 h", [idle_gain(base, min(T, 12 * HOUR)) for T in (L5_T1, L5_T2)],
            [L5_R * math.log(1 + 12 * HOUR / tau)] * 2, "shape")


def l5_bank_cap(base, tau):
    """A bank that fills and stops at 12 h of income."""
    return ("bank cap", [idle_gain(base, T, bank_cap=base.P * 12 * HOUR) for T in (L5_T1, L5_T2)],
            [L5_R * math.log(1 + 12 * HOUR / tau)] * 2, "shape")


def l5_fight(base_meas, w=2.0):
    """A spine step that needs a fight: cooldown 20 min, visit 2 min, so 1 clear for each visit."""
    fight = [min(g / math.log(w), 1.0) * math.log(w) for g in base_meas]
    return ("fight cooldown", fight, [math.log(w)] * 2, "shape")


def l5_grade(tool, s0):
    """Return (worst error, (name, class as expected, text)) of 1 tool."""
    name, meas, pred, kind = tool
    err = max(rel(m, p_) for m, p_ in zip(meas, pred))
    k = "scale" if abs(l5_slope(*meas) / s0 - 1) < 0.5 else "shape"
    return err, (name, k == kind, "%s %s" % (name, k))


def l5_reset(p=1.0, q=0.5, X=1.0):
    """Reset that needs a visit against a reset that runs while away (lambda = 1 loop, f = 0.5)."""
    st = l5_reset_start(p, q, X)
    gains = [l5_reset_gains(st, T, X) for T in (L5_T1, L5_T2)]
    vis, auto = [v for v, _ in gains], [a for _, a in gains]
    lin = (auto[1] / auto[0]) / (L5_T2 / L5_T1)
    logv = l5_slope(*vis) / (p * q)
    ok_reset = abs(lin - 1) < 0.1 and abs(logv - 1) < 0.25
    return ("reset", ok_reset, "visit reset slope/(p q) %.2f, auto reset gain(4d)/gain(1d)/4 %.2f" % (logv, lin))


def l5_reset_start(p, q, X, c=1e3):
    st = Sim(loop_game([0.25, 0.25], p, q, c))
    st.play(2 * HOUR, X=X)
    st.do_reset()
    st.play(st.t + 120.0, X=X)
    return st


def l5_reset_gains(st, T, X):
    """Return (ln M gain with a reset at the visit, ln M gain with a reset while away) after T."""
    a = copy_sim(st)
    l0 = ell(a)
    a.idle(T, f=0.5)
    visit(a, X)
    b = copy_sim(st)
    b.play(b.t + T, X=X, f=0.5)
    return ell(a) - l0, ell(b) - l0


def check_l6():
    parts = [l6_unstable(), l6_moss()] + l6_stable() + [l6_closed_form()]
    report("L6", all(ok for ok, _ in parts), "; ".join(text for _, text in parts))


def l6_unstable():
    """lambda > 1: run time ratio = (L_{n+1} / L_n) ** (1 / (alpha p) - q)."""
    R, p, q, X = 0.6, 0.5, 1.0, 10.0
    alpha = 1 / (1 - R)
    sim = Sim(loop_game([0.3, 0.3], p, q, 1e3))
    sim.play(1e7, dt=0.05, X=X, max_resets=12)
    rs = sim.resets
    ratios = [rs[i + 1][3] / rs[i][3] for i in range(5, 11)]
    pred_a = (rs[11][2] / rs[10][2]) ** (1 / (alpha * p) - q)
    meas_a = sum(ratios) / len(ratios)
    return rel(meas_a, pred_a) < 0.05, "unstable run-time ratio predicted %.3f, simulated %.3f (5%%)" % (pred_a, meas_a)


def l6_moss():
    """Moss Works P1 with the rule gain >= 10 * L: lower bounds 10 * 51 and 10 * 562."""
    seq_pred = [510, 10 * (51 + 511)]
    seq_meas = [511, 6207]
    ok_b = all(rel(m, p_) < 0.15 for m, p_ in zip(seq_meas, seq_pred))
    return ok_b, "Moss Works gains predicted >= %s, measured %s (15%%)" % (seq_pred, seq_meas)


L6_XS = (1.0, 3.92, 10.0)


def l6_stable(R=0.5, p=1.0, q=0.5, c=1e3):
    """lambda = 1: run time T_X = X ** (1 / s) * (c / A) ** (1 / alpha). Return 2 parts."""
    alpha = 1 / (1 - R)
    game = loop_game([0.25, 0.25], p, q, c)
    A = traj_A(game, alpha)
    sims = [l6_loop_sim(game, X) for X in L6_XS]
    rates = dict(zip(L6_XS, [loop_rate(sm, 3 * HOUR) for sm in sims]))
    T_meas = sims[0].resets[-1][3]
    T_pred = (c / A) ** (1 / alpha)
    stable = (rel(T_meas, T_pred) < 0.05, "stable run time predicted %.0f s, simulated %.0f s (5%%)" % (T_pred, T_meas))
    return [stable, l6_speed(rates, p * alpha)]


def l6_loop_sim(game, X):
    sm = Sim(game)
    sm.play(6 * HOUR, X=X)
    return sm


def l6_speed(rates, s):
    """Speed of the patient and the best reset rule against the early rule X = 1."""
    def v(x):
        return math.log(1 + x) / x ** (1 / s)
    gap_pred = v(10.0) / v(1.0)
    gap_meas = rates[10.0] / rates[1.0]
    best_pred = v(3.92) / v(1.0)
    best_meas = rates[3.92] / rates[1.0]
    ok_d = rel(gap_meas, gap_pred) < 0.05 and rel(best_meas, best_pred) < 0.05
    return ok_d, ("patient/early speed predicted %.3f, simulated %.3f, best/early predicted %.3f, simulated %.3f (5%%)"
                  % (gap_pred, gap_meas, best_pred, best_meas))


def l6_closed_form():
    """Closed form of the source: mean reset multiplier e^(z (y + 1) / (1 - x))."""
    errs = [rel(closed_form_mult(x, y, z), math.exp(z * (y + 1) / (1 - x)))
            for x, y, z in ((0.5, 0.0, 0.5), (0.6, 1.0, 0.4))]
    worst_e = max([0.0] + errs)
    return worst_e < 0.05, "closed form worst error %.1f%% (5%%)" % (worst_e * 100)


def loop_rate(sim, t_from):
    """ln M gained for each second, from reset to reset after t_from. Whole runs only."""
    q = sim.g.reset["q"]
    rs = [r for r in sim.resets if r[0] >= t_from]
    return q * (math.log(1 + rs[-1][2]) - math.log(1 + rs[0][2])) / (rs[-1][0] - rs[0][0])


class ClosedForm:
    """d(rho)/dt = K rho^x t^y rho_pub^z, integrated with RK4 in u = ln rho and v = ln t."""

    def __init__(self, x, y, z, K):
        self.x, self.y, self.z, self.K = x, y, z, K
        self.u_pub = 0.0

    def rate(self, vv, uu):
        tt = math.exp(vv)
        x, y, z, K = self.x, self.y, self.z, self.K
        return tt * K * math.exp((x - 1) * uu) * tt ** y * math.exp(z * self.u_pub)

    def start(self, T, n):
        """Start on the exact small-time branch at t0 = T * 1e-6. Return (u, v0, step)."""
        x, y, z, K = self.x, self.y, self.z, self.K
        t0 = T * 1e-6
        u = math.log(K * (1 - x) * math.exp(z * self.u_pub) * t0 ** (y + 1) / (y + 1)) / (1 - x)
        v0 = math.log(t0)
        return u, v0, (math.log(T) - v0) / n

    def run(self, T, n=400):
        u, vv, h = self.start(T, n)
        for _ in range(n):
            u, vv = self.rk4(vv, u, h)
        return u

    def rk4(self, vv, u, h):
        k1 = self.rate(vv, u)
        k2 = self.rate(vv + h / 2, u + h * k1 / 2)
        k3 = self.rate(vv + h / 2, u + h * k2 / 2)
        k4 = self.rate(vv + h, u + h * k3)
        return u + h * (k1 + 2 * k2 + 2 * k3 + k4) / 6, vv + h

    def score(self, m):
        """(u(T) - u_pub) / T at T = e^m."""
        return (self.run(math.exp(m)) - self.u_pub) / math.exp(m)

    def narrow(self, lo, hi):
        """1 step of the ternary search for the best ln T."""
        m1 = lo + (hi - lo) / 3
        m2 = hi - (hi - lo) / 3
        return (m1, hi) if self.score(m1) < self.score(m2) else (lo, m2)

    def best_time(self):
        lo, hi = math.log(1e-3), math.log(1e6)
        for _ in range(60):
            lo, hi = self.narrow(lo, hi)
        return math.exp((lo + hi) / 2)


def closed_form_mult(x, y, z, K=1.0):
    """Integrate d(rho)/dt = K rho^x t^y rho_pub^z from rho ~ 0 with RK4 in u = ln rho.

    Choose the run time that maximises (u(T) - u_pub) / T. Return the reset multiplier e^(z * delta).
    """
    cf = ClosedForm(x, y, z, K)
    T = cf.best_time()
    return math.exp(z * (cf.run(T) - cf.u_pub))


def l7_cases():
    """(name, lines, groups, group exponents, predicted R) for each L7 case."""
    base = [mline(0.4, b=2.0, group="a")]
    return [
        ("new group r=0.3", base + [mline(0.3, b=2.0, group="b", a=20.0)], {"a": 1, "b": 1}, {}, 0.7),
        ("old group r=0.3", base + [mline(0.3, b=2.0, group="a", a=20.0)], {"a": 1}, {}, 0.4),
        ("old group r=0.6", base + [mline(0.6, b=2.0, group="a", a=20.0)], {"a": 1}, {}, 0.6),
    ] + l7_limit_cases(base)


def l7_limit_cases(base):
    return [
        ("softcap 0.5 on new group r=0.3", base + [mline(0.3, b=2.0, group="b", a=20.0)], {"a": 1, "b": 1}, {"b": 0.5}, 0.55),
        ("cap 20 levels on r=0.3", base + [mline(0.3, b=2.0, group="b", a=20.0, cap=20)], {"a": 1, "b": 1}, {}, 0.4),
        ("additive line in group a", base + [Line(20.0, 1.15, "add", 1.0, "a")], {"a": 1}, {}, 0.4),
    ]


def check_l7():
    graded = [l7_case(*case) for case in l7_cases()]
    worst = max([0.0] + [err for err, _ in graded])
    k, kp = l7_second_clock()
    rows = [row for _, row in graded] + ["second clock y=0.5 income exponent %.2f/%.2f" % (kp, k)]
    ok = worst < 0.02 and rel(k, kp) < 0.05
    report("L7", ok, "R predicted/simulated (0.02): " + "; ".join(rows) + " (5%)")


def l7_case(name, lines, groups, gexp, pred):
    """Return (error, row text) of 1 case."""
    meas = spend_slope(Game(lines, groups, gexp=gexp), 1e10, 1e18)
    return abs(meas - pred), "%s %.3f/%.3f" % (name, pred, meas)


def l7_second_clock():
    """Second clock: income x t^y. Income ~ t^((R + y) / (1 - R)). Return (simulated, predicted)."""
    R, y = 0.5, 0.5
    sim = Sim(simple_game([0.25, 0.25], time_mult=lambda t: max(t, 1.0) ** y))
    k = growth_exponent(sim, 2e5, 2e3)
    return k, (R + y) / (1 - R)


L11_A, L11_B = 10.0, 1.15


def gen_line(n, m):
    """A generator: an additive line with count steps (n, m) and no cap."""
    return Line(L11_A, L11_B, "add", 1.0, "g", steps=(n, m))


def r_eff(n, m):
    return math.log(m) / (n * math.log(L11_B))


def check_l11():
    parts = [l11_alone(10, 1.55), l11_alone(25, 2.0), l11_carrier(), l11_blocks()]
    report("L11", all(ok for ok, _ in parts), "; ".join(text for _, text in parts))


def l11_level(S):
    """The level of the generator with the spend S (L1, bulk-buy formula, as a real number)."""
    return math.log(S * (L11_B - 1) / L11_A + 1) / math.log(L11_B)


def l11_alone(n, m, s_lo=1e10, s_hi=1e20):
    """1 generator: d ln P / d ln S = r_eff + d ln k / d ln S (the secant of the log factor)."""
    meas = spend_slope(Game([gen_line(n, m)], {"g": 1.0}), s_lo, s_hi)
    pred = r_eff(n, m) + math.log(l11_level(s_hi) / l11_level(s_lo)) / math.log(s_hi / s_lo)
    return rel(meas, pred) < 0.03, "n=%d m=%g R predicted %.3f, simulated %.3f (3%%)" % (n, m, pred, meas)


def l11_carrier(s_lo=1e12, s_hi=1e24):
    """A generator and a multiplier line r = 0.3: R = 0.3 + r_eff + the log factor at the simulated levels."""
    sim = Sim(Game([gen_line(10, 1.55), mline(0.3, b=2.0, group="h", a=20.0)], {"g": 1.0, "h": 1.0}))
    pts, ks = [], []
    for i in range(int(math.log(s_hi) / math.log(1.5)) + 1):
        l11_point(sim, (1.5 ** i, s_lo), pts, ks)
    (x1, k1), (x2, k2) = ks[0], ks[-1]
    pred, meas = 0.3 + r_eff(10, 1.55) + math.log(k2 / k1) / (x2 - x1), fit(pts)
    return rel(meas, pred) < 0.03, "with r=0.3 R predicted %.3f, simulated %.3f (3%%)" % (pred, meas)


def l11_point(sim, spend, pts, ks):
    """Raise the spend (spend_point). From s_lo, add (ln spend, generator level) to ks."""
    s, s_lo = spend
    spend_point(sim, s, s_lo, pts)
    ks += [(math.log(sim.S + sim.cash), sim.lv[0])] if s >= s_lo else []


def l11_blocks(n=10, m=1.55, top=150):
    """Active greedy: each block of n levels gains at least (to / (from - 1)) m, and at least m."""
    sim = Sim(Game([gen_line(n, m), mline(0.3, b=2.0, group="h", a=20.0)], {"g": 1.0, "h": 1.0}))
    gains = []
    while sim.lv[0] < top:
        sim.earn(0.0 if l11_buy_one(sim, n, gains) else 5.0)
    worst = min(g / (k / (k - n) * m if k > n else m) for k, g in gains)
    low = min(g for _, g in gains)
    ok = worst > 1 - 0.01 and low >= m
    return ok, ("%d blocks: smallest block gain %.3f (m %g), smallest gain against (to / (from - 1)) m %.3f (1%%)"
                % (len(gains), low, m, worst))


def l11_buy_one(sim, n, gains):
    """Buy the best line if the cash pays for it. Add (level, block gain) to gains at each count step of line 0."""
    j, cost = sim.best()
    if j is None or cost > sim.cash:
        return False
    before, lvl = sim.P, sim.lv[0] + 1
    sim._buy_one()
    l11_step(sim, (j, lvl, n, before), gains)
    return True


def l11_step(sim, item, gains):
    j, lvl, n, before = item
    if j != 0:
        return
    sim.block_start = before if (lvl - 1) % n == 0 else getattr(sim, "block_start", None)
    if lvl % n == 0 and sim.block_start:
        gains.append((lvl, sim.P / sim.block_start))


L9_A, L9_D = 1.0, 1.001


def check_l9():
    g = (L9_D - 1) / L9_A
    rates = {D: l9_rate(D) for D in (1.0, 100.0, 1000.0, 3000.0)}
    graded = [l9_row(g, D, rates) for D in (100.0, 1000.0, 3000.0)]
    worst = max([0.0] + [err for err, _ in graded])
    rows = [row for _, row in graded]
    report("L9", worst < 0.05, "slow-down from decision interval predicted/simulated (5%%): %s" % "; ".join(rows))


def l9_rate(D, target=20.0):
    """Rate of ln P from decision to decision. Whole decisions only."""
    sim = Sim(Game([Line(L9_A, L9_D, "mult", L9_D, "g")], {"g": 1.0}))
    pts = []
    while math.log(sim.P) < target:
        l9_decide(sim, D, pts)
    (t1, l1), (t2, l2) = pts[1], pts[-1]
    return (l2 - l1) / (t2 - t1)


def l9_decide(sim, D, pts):
    sim.play(sim.t + D, dt=D)
    sim.buy_all()
    pts.append((sim.t, math.log(sim.P)))


def l9_row(g, D, rates):
    """Return (error, row text) for decision interval D."""
    pred = (g * D / math.log(1 + g * D)) / (g / math.log(1 + g))
    meas = rates[1.0] / rates[D]
    return rel(meas, pred), "gD=%.1f %.3f/%.3f" % (g * D, pred, meas)


def print_lines(lines):
    """Print each line, as print() does. Print nothing for an empty list."""
    sys.stdout.write("".join(line + "\n" for line in lines))


MOSS_FIXTURE = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                            "assets", "fixtures", "moss-works-built")


def example_moss(root):
    print("\nWorked example 1: Moss Works data files")
    ups, garden, stages = moss_data(root)
    spores, grow = moss_lines(ups + garden["tonics"])
    print_lines(moss_r_lines(spores, grow))
    p = stages["prestige_exponent"]
    q = 1.0
    print_lines(moss_lambda_lines(p, q))
    print(moss_trellis_line(grow, stages["hp_growth"]))
    print(moss_p1_line(p, q))


def moss_data(root):
    ups = json.load(open(os.path.join(root, "data", "upgrades.json")))
    garden = json.load(open(os.path.join(root, "data", "garden.json")))
    stages = json.load(open(os.path.join(root, "data", "stages.json")))
    return ups, garden, stages


def moss_lines(upgrades):
    """Sort each multiplier line into the spore lines or the grow lines, and print it."""
    spores, grow = [], []
    for u in upgrades:
        moss_line(u, spores, grow)
    return spores, grow


def moss_line(u, spores, grow):
    e = u["effect"]
    if not ("mult" in e and u.get("purchasable", True) and u["growth"] > 1):
        return
    r = math.log(e["mult"]) / math.log(u["growth"])
    row = (u["id"], u["currency"], r, u["max_level"])
    (spores if e["stat"] == "spores_per_sec" else grow).append(row)
    print(moss_line_text(u, e["stat"], r))


def moss_line_text(u, stat, r):
    return "  line %-12s currency %-8s stat %-14s r = %.3f cap %s" % (u["id"], u["currency"], stat, r, u["max_level"])


def moss_r_lines(spores, grow):
    R_sp = sum(r for _, cur, r, _ in spores if cur == "spores")
    R_gr = sum(r for _, cur, r, _ in grow if cur == "spores")
    return ["  in-run R of spore lines on income, all uncapped: %.3f (above 1: finite-time growth until caps)" % R_sp,
            "  in-run R of spore lines on grow power: %.3f" % R_gr]


def moss_lambda_lines(p, q):
    return ["  reset loop lambda = p q / (1 - R) at R = %.1f: %.2f" % (R, p * q / (1 - R)) for R in (0.0, 0.5, 0.9)]


def moss_trellis_line(grow, w):
    trellis = [r for i, _, r, _ in grow if i == "trellis"][0]
    return ("  idle stages for each doubling of the bank, trellis only: R_s ln 2 / ln w = %.2f (measured about 1)"
            % (trellis * LN2 / math.log(w)))


def moss_p1_line(p, q, t_ratio=1 / 7.0):
    """P1: run 2 took 7 min, run 3 under 1 min, L went 51 -> 562 -> 6769."""
    ratio_L = 6769 / 562.0
    e = math.log(t_ratio) / math.log(ratio_L)
    alpha = 1 / (p * (e + q))
    return ("  P1 run-time ratio <= 1/7 with L ratio %.1f gives 1/(alpha p) - q <= %.2f, so alpha >= %.1f and lambda >= %.1f"
            % (ratio_L, e, alpha, p * q * alpha))


EX2 = dict(R=0.2, p=3.0, X=1.0, X_patient=10.0, T_X=900.0, f_idle=0.5, f_closed=0.5, cap_closed=12 * HOUR,
           visit=120.0, hours=20.0, unlocks_first_hour=6, unlocks_per_hour=5.0,
           first_times_min=(0.5, 2.0, 5.0, 10.0, 25.0, 45.0))


def ex2_lines(R):
    return [Line(10.0, 1.15, "add", 1.0, "gen", cap=40),
            mline(R / 2, b=1.6, a=100.0, group="m1"),
            mline(R / 2, b=2.2, a=1000.0, group="m2")]


EX2_GROUPS = {"gen": 1.0, "m1": 1.0, "m2": 1.0}


def ex2_design():
    """The forward procedure of the document, step by step. Returns the parameters and the game."""
    P = dict(EX2)
    ex2_reset_params(P)
    ex2_wall_params(P)
    trial = Game(ex2_lines(P["R"]), dict(EX2_GROUPS), reset=dict(c=P["c"], p=P["p"], q=P["q"]))
    hs = ex2_first_powers(trial, P)
    p1h = ex2_peak_1h(trial, P["X"])
    P["h_first"] = hs
    ex2_stage_count(P, hs, p1h)
    return P


def ex2_reset_params(P):
    R = P["R"]
    alpha = 1 / (1 - R)
    P["alpha"] = alpha
    P["q"] = (1 - R) / P["p"]
    P["s"] = P["p"] * alpha
    A = traj_A(Game(ex2_lines(R), dict(EX2_GROUPS)), alpha, x=1e6)
    P["A"] = A
    P["c"] = A * P["T_X"] ** alpha * P["X"] ** (-alpha / P["s"])


def ex2_wall_params(P):
    ldot = P["q"] * math.log(1 + P["X"]) / P["T_X"] * HOUR
    P["ldot"] = ldot
    P["w"] = math.exp(P["alpha"] * ldot / P["unlocks_per_hour"])


def ex2_first_powers(trial, P):
    sim = Sim(trial)
    hs = []
    pmax = 0.0
    for tm in P["first_times_min"]:
        pmax = ex2_peak_until(sim, tm * 60, P["X"], pmax)
        hs.append(float("%.2g" % pmax))
    return hs


def ex2_peak_until(sim, t_end, X, pmax):
    """Play to t_end in steps of 10 s. Return the highest power, from pmax."""
    peak = pmax
    while sim.t < t_end:
        sim.play(min(sim.t + 10, t_end), X=X)
        peak = max(peak, sim.P)
    return peak


def ex2_peak_1h(trial, X):
    sim2 = Sim(trial)
    p1h = 0.0
    while sim2.t < HOUR:
        sim2.play(sim2.t + 10, X=X)
        p1h = max(p1h, sim2.P)
    return p1h


def ex2_stage_count(P, hs, p1h):
    alpha = P["alpha"]
    ln_end = math.log(p1h) + alpha * P["ldot"] * (P["hours"] - 1)
    P["N"] = len(hs) + int(math.ceil((ln_end - math.log(hs[-1])) / math.log(P["w"])))
    P["kappa"] = P["f_idle"] * alpha * (P["visit"] / P["T_X"]) ** (alpha - 1)


def ex2_game(P):
    h1 = P["h_first"]
    w = P["w"]
    game = Game(ex2_lines(P["R"]), {"gen": 1.0, "m1": 1.0, "m2": 1.0}, spine=(h1[0], w, P["N"]),
                reset=dict(c=P["c"], p=P["p"], q=P["q"]), f_open=P["f_idle"], f_closed=P["f_closed"],
                cap_closed=P["cap_closed"])
    game.h_table = h1
    return game


class Ex2Sim(Sim):
    """Spine with a table for the first stages, then ratio w."""

    def vstage(self, p=None):
        p = self.P if p is None else p
        return table_stage(self.g.h_table, p, self.g.spine[1])


def table_stage(hs, p, w):
    """Stage of power p: a table hs for the first stages, then ratio w."""
    if p < hs[0]:
        return 0.0
    i = next((k for k in range(1, len(hs)) if p < hs[k]), len(hs))
    return table_tail(hs, i, p, w)


def table_tail(hs, i, p, w):
    if i == len(hs):
        return len(hs) + math.log(p / hs[-1]) / math.log(w)
    return i + math.log(p / hs[i - 1]) / math.log(hs[i] / hs[i - 1])


def rho_pred(P, T):
    u = T / P["T_X"]
    X = P["X"]
    return math.log(1 + X * (P["kappa"] * u) ** P["p"]) / (u * math.log(1 + X))


def cycle_rate(game, T, X, closed=False, V=120.0, horizon=None):
    sim = Ex2Sim(game)
    sim.play(V, X=X)
    marks = []
    horizon = horizon or max(40 * HOUR, 8 * T)
    finish = None
    while sim.t < horizon:
        marks.append(cycle_visit(sim, T, X, closed, V))
        finish = first_finish(finish, sim, game.spine[2])
    return cycle_slope(marks), finish


def cycle_visit(sim, T, X, closed, V):
    """Stay away T seconds, then visit. Return (time, ln M)."""
    (sim.closed if closed else sim.idle)(T)
    visit(sim, X, V)
    return sim.t, ell(sim)


def first_finish(finish, sim, n_last):
    return sim.t if finish is None and sim.hi >= n_last else finish


def cycle_slope(marks):
    """ln M for each hour from the middle mark to the last mark."""
    (t1, l1), (t2, l2) = marks[len(marks) // 2], marks[-1]
    return (l2 - l1) / (t2 - t1) * HOUR


def finish_time(sim):
    return next((t for t, n in sim.unlocks if n >= sim.g.spine[2]), None)


EX2_VISITS = ((HOUR, False), (8 * HOUR, False), (DAY, False), (7 * DAY, False), (30 * DAY, False), (DAY, True))


class Ex2Run:
    """The player types of worked example 2 and their results."""

    def __init__(self, P):
        self.P = P
        self.game = ex2_game(P)
        self.H = P["hours"] * HOUR
        self.res = {}

    def player(self, dt, X):
        sim = Ex2Sim(self.game)
        sim.play(3 * self.H, dt=dt, X=X)
        return sim

    def run_active(self):
        """1. Active greedy (early reset rule X = 1)."""
        a = self.player(1.0, self.P["X"])
        self.fa = finish_time(a)
        self.first_hour = sum(1 for t, _ in a.unlocks if t <= HOUR)
        self.med_gap, self.max_gap = ex2_gaps(a.unlocks, self.fa)
        self.rate_a = loop_rate(a, 4 * HOUR) * HOUR

    def run_others(self):
        """2. Active slow, 5. patient reset X = 10, and speed x100 and x10 with a human decision interval."""
        X = self.P["X"]
        sims = [self.player(60.0, X), self.player(1.0, self.P["X_patient"]), self.player(200.0, X), self.player(20.0, X)]
        self.fs, self.fp, self.fx, self.fx10 = [finish_time(sim) for sim in sims]
        self.pt = sims[1]

    def run_idle(self):
        """3 and 4. Idle players, and the idle ratio at each visit interval."""
        rows = [self.idle_row(T, closed) for T, closed in EX2_VISITS]
        self.idle_ok = all(0 < rho < 1 for rho, _ in rows)
        self.rows = [text for _, text in rows]

    def idle_row(self, T, closed):
        r, fin = cycle_rate(self.game, T, self.P["X"], closed=closed)
        rho = r / self.rate_a
        pred = ex2_idle_pred(self.P, T, closed)
        self.res[(T, closed)] = (rho, pred, fin)
        return rho, ex2_idle_text(T, closed, rho, pred, fin)

    def player_lines(self):
        us = [self.P["T_X"] * x for x in (0.5, 1, 2, 3, 4, 6, 8, 16, 32, 96)]
        self.rho_max = max(rho_pred(self.P, u) for u in us)
        lines = ["  player types:", self.greedy_line(), self.slow_line(), self.patient_line()] + self.speed_lines()
        return lines + self.rows + ["    largest predicted idle ratio over visit intervals: %.2f" % self.rho_max]

    def greedy_line(self):
        return ("    active greedy (X = 1, decision 1 s): finish %.2f h, unlocks in hour 1: %d, gap after hour 1 median %.1f min, max %.1f min"
                % (self.fa / HOUR, self.first_hour, self.med_gap, self.max_gap))

    def slow_line(self):
        return "    active slow (decision 60 s): finish %.2f h (%.2f x greedy)" % (self.fs / HOUR, self.fs / self.fa)

    def patient_line(self):
        rate_ratio = loop_rate(self.pt, 4 * HOUR) * HOUR / self.rate_a
        predicted = (math.log(11) / 10 ** (1 / self.P["s"])) / math.log(2)
        return ("    patient reset (X = 10): finish %.2f h; early/patient finish ratio %.2f; ln M rate ratio %.2f (predicted %.2f)"
                % (self.fp / HOUR, self.fa / self.fp, rate_ratio, predicted))

    def speed_lines(self):
        fx, fx10, fa = self.fx, self.fx10, self.fa
        return ["    speed x100, decision each 200 game s: finish %.2f h (%.3f x greedy)" % (fx / HOUR, fx / fa),
                "    speed x10, decision each 20 game s: finish %.2f h (%.3f x greedy)" % (fx10 / HOUR, fx10 / fa)]

    def targets(self):
        return self.pace_targets() + self.idle_targets() + self.style_targets()

    def pace_targets(self):
        return [
            ("length 20 h +-15%", abs(self.fa / self.H - 1) <= 0.15),
            ("6 +- 1 unlocks in hour 1", abs(self.first_hour - 6) <= 1),
            ("session 15 min: median gap <= 15 min, max gap <= 20 min", self.med_gap <= 15 and self.max_gap <= 20),
        ]

    def idle_targets(self):
        o8 = self.res[(8 * HOUR, False)][0]
        c24 = self.res[(DAY, True)][0]
        return [
            ("open idle 8 h worth 30% to 50%", 0.30 <= o8 <= 0.50),
            ("closed idle 24 h worth 10% to 25%", 0.10 <= c24 <= 0.25),
        ]

    def style_targets(self):
        return [
            ("patient at most 2x faster than early", self.fa / self.fp <= 2.0),
            ("active slow within 1.25x of greedy", self.fs / self.fa <= 1.25),
            ("idle below active and above 0 from 1 h to 30 d", self.idle_ok and self.rho_max < 1),
        ]

    def extra_targets(self):
        return [
            ("extra: x100 with 200 s decisions within 1.1x", self.fx / self.fa <= 1.1),
            ("extra: x10 with 20 s decisions within 1.1x", self.fx10 / self.fa <= 1.1),
        ]

    def l8_text(self):
        P = self.P
        return ("length predicted %.1f h, simulated %.2f h (15%%); hour-1 unlocks predicted %d, simulated %d (1); "
                "later unlocks per hour predicted %.1f, median gap %.1f min" % (P["hours"], self.fa / HOUR, P["unlocks_first_hour"],
                                                                              self.first_hour, P["unlocks_per_hour"], self.med_gap))


def ex2_gaps(unlocks, fa):
    """Return the median and the largest gap in minutes between the unlocks after hour 1."""
    gaps = [t2 - t1 for (t1, _), (t2, _) in zip(unlocks, unlocks[1:]) if t1 >= HOUR and t2 <= fa]
    gaps.sort()
    return gaps[len(gaps) // 2] / 60, gaps[-1] / 60


def ex2_idle_pred(P, T, closed):
    if not closed:
        return rho_pred(P, T)
    Teff = min(T, P["cap_closed"])
    return rho_pred(dict(P, kappa=P["kappa"] * P["f_closed"] / P["f_idle"]), Teff) * Teff / T


def hours_text(fin):
    return "%.1f h" % (fin / HOUR) if fin else "after horizon"


def ex2_idle_text(T, closed, rho, pred, fin):
    return ("    visit each %5.0f h %-6s ratio simulated %.3f, predicted %.3f, finish %s"
            % (T / HOUR, "closed" if closed else "open", rho, pred, hours_text(fin)))


def ex2_header(P):
    return ["  parameters: R = %.2f, alpha = %.3f, p = %.1f, q = %.4f, s = %.2f, A = %.2f, c = %.4g, T_X = %.0f s"
            % (P["R"], P["alpha"], P["p"], P["q"], P["s"], P["A"], P["c"], P["T_X"]),
            "  ln M for each active hour %.3f, wall ratio w = %.4f, first stage powers %s, N = %d, kappa = %.3f"
            % (P["ldot"], P["w"], P["h_first"], P["N"], P["kappa"])]


def example_2():
    print("\nWorked example 2: corrected small game")
    P = ex2_design()
    print_lines(ex2_header(P))
    run = Ex2Run(P)
    run.run_active()
    run.run_others()
    run.run_idle()
    print_lines(run.player_lines())
    targets = run.targets()
    print("  targets:")
    print_lines(["    %-58s %s" % (name, "MET" if ok else "MISSED") for name, ok in targets + run.extra_targets()])
    l8_ok = targets[0][1] and targets[1][1] and targets[2][1]
    report("L8", l8_ok, run.l8_text())
    return all(ok for _, ok in targets)


def main():
    t0 = time.time()
    root = MOSS_FIXTURE
    check_l1()
    check_l2()
    check_l3()
    base, tau = check_l4()
    check_l5(base, tau)
    check_l6()
    check_l7()
    ex2_ok = example_2()
    check_l9()
    check_l11()
    if os.path.isdir(root):
        example_moss(root)
    else:
        print("\nWorked example 1: left out, the game data files are not in this copy of the skill")
    print("\nlaws: " + ", ".join("%s %s" % r for r in sorted(RESULTS)))
    print("example 2 ticket targets: %s" % ("all met" if ex2_ok else "see MISSED lines"))
    print("run time %.1f s" % (time.time() - t0))
    return 0 if all(tag != "FAIL" for _, tag in RESULTS) else 1


if __name__ == "__main__":
    sys.exit(main())
