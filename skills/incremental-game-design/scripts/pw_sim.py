"""The simulated buyer for progression_workbook.py.

Time: `t` is seconds of play (the clock of all indicators). `real` is seconds of real time. The two are
equal unless the model sets game.away_minutes.
"""
import math

from pw_model import BIG, INF, cost_one, prestige_on

WEIGHT = {"system_unlock": 5, "prestige_first": 5, "prestige": 3, "feature": 3, "content": 2,
          "reward": 2, "ladder": 2, "first_purchase": 2, "purchase": 1, "repeat": 1, "stall": 0}
SMALL_VALUE = 0.02      # value of a purchase that changes nothing the buyer can measure
WALL_WEIGHT = 3.0       # weight of a stat that blocks the next ladder step or a milestone
BASE = "(base income)"


class Sim:
    def __init__(self, m, hours, profile=None, beat_weight=3, max_events=40000):
        self.m = m
        g = m["game"]
        self.limit = hours * 3600.0
        self.beat_weight = beat_weight
        self.max_events = max_events
        self.t = self.real = 0.0
        self.cids = [c["id"] for c in m["currencies"]]
        self.base_income = {c["id"]: float(c.get("base_income", 0.0)) for c in m["currencies"]}
        self.cur = {c["id"]: float(c.get("start", 0)) for c in m["currencies"]}
        self.life = dict.fromkeys(self.cur, 0.0)
        self.run = dict.fromkeys(self.cur, 0.0)
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
        # prestige
        self.pr = dict(m["prestige"]) if prestige_on(m) else None
        if self.pr and profile:
            self.pr.update(profile)
        self.best_rate = 0.0
        self.run_new_beats = 0
        self.run_top = dict(self.ladder_pos)
        # sessions with away time
        self.session = g["session_minutes"] * 60.0
        self.away = g["away_minutes"] * 60.0
        self.offline = g["offline_rate"]
        self.block_left = self.session
        # effects, compiled one time
        self.af = g["active_fraction"]
        by_system = g["pct_grouping"] == "system"
        self.sources = []                   # (kind, id, system, [(targets, kind, value, group)])
        self.capfx = {}                     # purchase id -> [(source kind, source id, value)]
        for kind, key in (("p", "purchases"), ("m", "milestones"), ("l", "ladders")):
            for x in m[key]:
                fx = []
                for e in x["effects"]:
                    if e["kind"] == "cap":
                        self.capfx.setdefault(e["target"], []).append((kind, x["id"], e["value"]))
                        continue
                    targets = tuple(self.cids) if e["target"] == "all" else (e["target"],)
                    v = e["value"] * (self.af if e.get("active") and e["kind"] == "add" else 1.0)
                    fx.append((targets, e["kind"], v, e.get("group") or (x["system"] if by_system else "")))
                if fx:
                    self.sources.append((kind, x["id"], x["system"], fx))
        self.has_income_fx = {s[1] for s in self.sources if s[0] == "p"}
        self.pdelta = {}                    # purchase id -> what one more level adds to the income terms
        for kind, ident, _, fx in self.sources:
            if kind != "p":
                continue
            dadd, dpct, dmul, dzero = {}, {}, {}, set()
            for targets, k, v, grp in fx:
                for c in targets:
                    if k == "add":
                        dadd[c] = dadd.get(c, 0.0) + v
                    elif k == "pct":
                        dpct.setdefault(c, {})[grp] = dpct.get(c, {}).get(grp, 0.0) + v
                    elif v <= 0:
                        dzero.add(c)
                    else:
                        dmul[c] = dmul.get(c, 0.0) + math.log(v)
            self.pdelta[ident] = (dadd, dpct, dmul, dzero, set(dadd) | set(dpct) | set(dmul) | dzero)
        self.cap_targets = {}               # source purchase id -> [(target id, value)]
        for tgt, lst in self.capfx.items():
            for kind, sid, v in lst:
                if kind == "p":
                    self.cap_targets.setdefault(sid, []).append((tgt, v))
        self.wanted_by = {}                 # purchase id -> [(kind, id, level)]
        for kind, key in (("p", "purchases"), ("m", "milestones")):
            for x in m[key]:
                for r in x["requires"]:
                    if r["type"] == "level":
                        self.wanted_by.setdefault(r["id"], []).append((kind, x["id"], r.get("level", 1)))
        if self.pr:
            for r in self.pr["requires"]:
                if r["type"] == "level":
                    self.wanted_by.setdefault(r["id"], []).append(("x", "prestige", r.get("level", 1)))
        self._inc = self._parts = self._w = self._acc = None
        self.last_change = 0.0
        # records for the sheets
        self.pstat = {p["id"]: dict(first_avail=None, first_afford=None, first=None, last=None, top=0, spent={},
                                    cap_at=None, pay_first=None, pay_last=None, bought=0) for p in m["purchases"]}
        self.steps = {l["id"]: {} for l in m["ladders"]}      # step -> (t, value of the gating quantity)
        self.earn = {c: {} for c in self.cids}
        self.spent = {c: {} for c in self.cids}
        self.lost = dict.fromkeys(self.cids, 0.0)
        self.axis = g.get("progress_axis")

    # -- income -----------------------------------------------------------------
    def count(self, kind, ident):
        if kind == "p":
            return self.level[ident]
        if kind == "m":
            return 1 if ident in self.done else 0
        return self.ladder_best[ident] if self.lmap[ident]["effects_persist"] else self.ladder_pos[ident]

    def compute_income(self, parts=False):
        add = dict.fromkeys(self.cids, 0.0)
        pct = {c: {} for c in self.cids}
        mul = dict.fromkeys(self.cids, 0.0)         # log of the product
        zero = set()
        src = {c: {} for c in self.cids} if parts else None
        for kind, ident, system, fx in self.sources:
            n = self.count(kind, ident)
            if not n:
                continue
            for targets, k, v, grp in fx:
                for c in targets:
                    if k == "add":
                        add[c] += v * n
                        if parts and v > 0:
                            src[c][system] = src[c].get(system, 0.0) + v * n
                    elif k == "pct":
                        pct[c][grp] = pct[c].get(grp, 0.0) + v * n
                    elif v <= 0:
                        zero.add(c)
                    else:
                        mul[c] += n * math.log(v)
        out = {}
        for c in self.cids:
            x = self.base_income[c] + add[c]
            for s in pct[c].values():
                x *= max(0.0, 1 + s)
            x = 0.0 if c in zero else x * math.exp(min(mul[c], 690.0))
            out[c] = min(x, BIG)
            if parts and self.base_income[c] > 0:
                src[c][BASE] = self.base_income[c]
        self._acc = (add, pct, mul, zero)
        return out, src

    def income(self):
        if self._inc is None:
            self._inc, self._parts = self.compute_income(parts=True)
        return self._inc

    def changed(self):
        self._inc = self._parts = self._w = None
        self.last_change = self.t

    def trial_income(self, pid):
        """Income with one more level of a purchase. Only the currencies that it touches are computed again."""
        out = dict(self.income())
        add, pct, mul, zero = self._acc
        dadd, dpct, dmul, dzero, touched = self.pdelta[pid]
        for c in touched:
            x = self.base_income[c] + add[c] + dadd.get(c, 0.0)
            extra = dpct.get(c, {})
            for grp, v in pct[c].items():
                x *= max(0.0, 1 + v + extra.get(grp, 0.0))
            for grp, v in extra.items():
                if grp not in pct[c]:
                    x *= max(0.0, 1 + v)
            x = 0.0 if (c in zero or c in dzero) else x * math.exp(min(mul[c] + dmul.get(c, 0.0), 690.0))
            out[c] = min(x, BIG)
        return out

    # -- requirements -----------------------------------------------------------
    def quantity(self, req, inc):
        """(value now, rate of change) of the quantity that a requirement compares."""
        t = req["type"]
        if t == "total":
            return self.life[req["currency"]], inc[req["currency"]]
        if t == "run_total":
            return self.run[req["currency"]], inc[req["currency"]]
        if t == "have":
            return self.cur[req["currency"]], inc[req["currency"]]
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
        v, r = self.quantity(req, inc)
        a = req.get("amount", 0)
        if v >= a:
            return (True, 0.0)
        return (False, (a - v) / r if r else INF)

    def all_met(self, reqs, inc):
        ok, wait = True, 0.0
        for r in reqs:
            m_, w = self.met(r, inc)
            if not m_:
                ok = False
                wait = max(wait, w)
        return ok, wait

    def ladder_req(self, l, step):
        r = dict(l["requires"])
        try:
            r["amount"] = r["base"] * r["growth"] ** (step - 1)
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

    def gain(self, c, amt, source):
        self.cur[c] = min(self.cur[c] + amt, BIG)
        self.life[c] = min(self.life[c] + amt, BIG)
        self.run[c] = min(self.run[c] + amt, BIG)
        self.earn[c][source] = self.earn[c].get(source, 0.0) + amt

    def grant(self, grants, system, step=1):
        for g in grants:
            amt = g["amount"] if "amount" in g else g.get("base", 0) * g.get("growth", 1.0) ** (step - 1)
            self.gain(g["currency"], amt, f"grants: {system}")

    def check_milestones(self):
        changed = True
        while changed:
            changed = False
            inc = self.income()
            for ms in self.m["milestones"]:
                if ms["id"] not in self.done and self.all_met(ms["requires"], inc)[0]:
                    self.done[ms["id"]] = self.t
                    first = ms["id"] not in self.ever_done
                    self.changed()
                    if first:
                        self.ever_done[ms["id"]] = self.t
                        self.grant(ms["grants"], ms["system"])
                        self.log("milestone", ms["id"], ms["name"], ms["system"],
                                 ms.get("excitement", WEIGHT.get(ms["kind"], 3)), note=ms["kind"])
                    else:
                        self.log("milestone", ms["id"], ms["name"], ms["system"], WEIGHT["repeat"], note="repeat")
                    changed = True
                    inc = self.income()
            for l in self.m["ladders"]:
                lid = l["id"]
                while self.ladder_pos[lid] < l["count"]:
                    step = self.ladder_pos[lid] + 1
                    req = self.ladder_req(l, step)
                    if not self.met(req, inc)[0]:
                        break
                    self.ladder_pos[lid] = step
                    self.run_top[lid] = max(self.run_top[lid], step)
                    new = step > self.ladder_best[lid]
                    self.changed()
                    if new:
                        self.ladder_best[lid] = step
                        self.steps[lid][step] = (self.t, self.quantity(req, inc)[0])
                        self.grant(l["grants"], l["system"], step)
                    self.log("ladder", lid, l["name"].replace("{n}", str(step)), l["system"],
                             l.get("excitement", WEIGHT["ladder"]) if new else WEIGHT["repeat"],
                             note="new" if new else "repeat")
                    changed = True
                    inc = self.income()

    def next_passive(self, inc):
        """Seconds until the next milestone, ladder step, or fixed run end that only needs time to pass."""
        best = INF
        for ms in self.m["milestones"]:
            if ms["id"] not in self.done:
                ok, w = self.all_met(ms["requires"], inc)
                if not ok and w < INF and all(self.met(r, inc)[0] or self.met(r, inc)[1] < INF
                                              for r in ms["requires"]):
                    best = min(best, w)
        for l in self.m["ladders"]:
            if self.ladder_pos[l["id"]] < l["count"]:
                ok, w = self.met(self.ladder_req(l, self.ladder_pos[l["id"]] + 1), inc)
                if not ok:
                    best = min(best, w)
        if self.pr and "run_seconds" in self.pr:
            best = min(best, max(0.0, self.run_start + self.pr["run_seconds"] - self.t))
        return best

    # -- the buyer --------------------------------------------------------------
    def candidates(self, inc):
        out = []
        for p in self.m["purchases"]:
            lv = self.level[p["id"]]
            cap = self.cap_of(p)
            if cap is not None and lv >= cap:
                continue
            if not self.all_met(p["requires"], inc)[0]:
                continue
            st = self.pstat[p["id"]]
            if st["first_avail"] is None:
                st["first_avail"] = self.t
            costs, wait = [], 0.0
            for c in p["costs"]:
                amt = cost_one(c, lv)
                costs.append((c["currency"], amt))
                have, rate = self.cur[c["currency"]], inc[c["currency"]]
                if have < amt:
                    wait = max(wait, (amt - have) / rate if rate > 0 else INF)
            if wait == 0 and st["first_afford"] is None:
                st["first_afford"] = self.t
            out.append((p, costs, wait))
        return out

    def weights(self, inc):
        """Weight of a relative gain on each currency. A stat that blocks a wall counts more."""
        if self._w is None:
            w = dict.fromkeys(self.cids, 1.0)
            for l in self.m["ladders"]:
                if self.ladder_pos[l["id"]] < l["count"] and l["requires"]["type"] == "rate":
                    w[l["requires"]["currency"]] = WALL_WEIGHT
            for ms in self.m["milestones"]:
                if ms["id"] not in self.done:
                    for r in ms["requires"]:
                        if r["type"] == "rate" and not self.met(r, inc)[0]:
                            w[r["currency"]] = WALL_WEIGHT
            self._w = w
        return self._w

    def rel_gain(self, pid, inc, w):
        new = self.trial_income(pid)
        gain = 0.0
        for c in self.cids:
            if new[c] > inc[c]:
                gain += w[c] * (min(10.0, new[c] / inc[c] - 1.0) if inc[c] > 0 else 10.0)
            elif new[c] < inc[c]:
                gain -= w[c] * (1.0 - new[c] / inc[c])
        return gain

    def wanted(self, pid):
        lv = self.level[pid]
        for kind, ident, need in self.wanted_by.get(pid, ()):
            if lv >= need:
                continue
            if kind == "m" and ident not in self.done:
                return True
            if kind == "p":
                q = self.pmap[ident]
                cap = self.cap_of(q)
                if cap is None or self.level[ident] < cap:
                    return True
            if kind == "x":
                return True
        return False

    def value(self, p, inc, w):
        pid = p["id"]
        v = self.rel_gain(pid, inc, w) if pid in self.has_income_fx else 0.0
        for tgt, raise_ in self.cap_targets.get(pid, ()):
            q = self.pmap[tgt]
            cap = self.cap_of(q)
            if raise_ > 0 and cap is not None and self.level[tgt] >= cap and tgt in self.has_income_fx:
                v += self.rel_gain(tgt, inc, w)       # one more level of the capped line becomes possible
        if v <= 0 and self.wanted(pid):
            v = 1.0
        if v < 0:
            return 0.0                      # it lowers income and nothing needs it: the buyer does not buy it
        return max(v, SMALL_VALUE)

    def advance(self, dt, inc, factor=1.0, play=True):
        for c in self.cids:
            d = inc[c] * dt * factor
            if d:
                self.cur[c] = min(self.cur[c] + d, BIG)
                self.life[c] = min(self.life[c] + d, BIG)
                self.run[c] = min(self.run[c] + d, BIG)
                parts = self._parts[c] if self._parts else None
                tot = sum(parts.values()) if parts else 0.0
                e = self.earn[c]
                if tot > 0:
                    for s, v in parts.items():
                        k = "income: " + s
                        e[k] = e.get(k, 0.0) + d * v / tot
                else:
                    e["income: " + BASE] = e.get("income: " + BASE, 0.0) + d
        self.real += dt
        if play:
            self.t += dt
            self.block_left -= dt

    def buy(self, p, costs, inc):
        pid = p["id"]
        st = self.pstat[pid]
        for c, amt in costs:
            self.cur[c] = max(0.0, self.cur[c] - amt)
            self.spent[c][p["system"]] = self.spent[c].get(p["system"], 0.0) + amt
            st["spent"][c] = st["spent"].get(c, 0.0) + amt
        before = inc[costs[0][0]]
        self.level[pid] += 1
        self.changed()
        first = pid not in self.ever
        self.ever.add(pid)
        delta = self.income()[costs[0][0]] - before
        pay = costs[0][1] / delta if delta > 0 else None
        if first:
            st["first"], st["pay_first"] = self.t, pay
        st["last"], st["pay_last"], st["bought"] = self.t, pay, st["bought"] + 1
        st["top"] = max(st["top"], self.level[pid])
        cap = self.cap_of(p)
        if cap is not None and self.level[pid] >= cap and st["cap_at"] is None:
            st["cap_at"] = self.t
        w = p.get("excitement", WEIGHT["first_purchase"]) if first else WEIGHT["purchase"]
        self.log("purchase", pid, f"{p['name']} L{self.level[pid]}", p["system"], w,
                 cost=costs[0][1], currency=costs[0][0], note="first" if first else None)

    # -- prestige ---------------------------------------------------------------
    def prestige_raw(self):
        """The gain before rounding down."""
        pr = self.pr
        try:
            if "basis_ladder" in pr:
                step = self.ladder_pos[pr["basis_ladder"]]
                if step <= 0:
                    return 0.0
                if pr["formula"] == "exp":
                    return min(BIG, pr["k"] * pr["growth"] ** step)
                return min(BIG, pr["k"] * step ** pr["exponent"])
            basis = self.run[pr["basis_currency"]] / pr["base"]
            if basis <= 0:
                return 0.0
            if pr["formula"] == "log":
                return max(0.0, pr["k"] * math.log10(max(basis, 1.0)))
            return min(BIG, pr["k"] * basis ** pr["exponent"])
        except OverflowError:
            return BIG

    def try_prestige(self, inc):
        pr = self.pr
        if not pr:
            return False
        elapsed = self.t - self.run_start
        raw = self.prestige_raw()
        gain = math.floor(raw + 1e-9)
        if "run_seconds" in pr:
            if elapsed < pr["run_seconds"] - 1e-6:
                return False
        else:
            rate = raw / max(elapsed, 1.0)
            self.best_rate = max(self.best_rate, rate)
            if not self.all_met(pr["requires"], inc)[0] or elapsed < pr["min_run_seconds"] or gain < pr["min_gain"]:
                return False
            if pr["policy"] == "peak_rate":
                if rate >= self.best_rate * pr["peak_tolerance"]:
                    return False            # the run still pays about as much each minute as at its best
            elif gain < pr["policy_ratio"] * self.life[pr["currency"]]:
                return False
        self.prestiges += 1
        self.gain(pr["currency"], gain, "prestige")
        rs = pr["resets"]
        for c in rs["currencies"]:
            self.lost[c] += self.cur[c]
            self.cur[c] = 0.0
        for p in self.m["purchases"]:
            if p["system"] in rs["purchase_systems"]:
                self.level[p["id"]] = 0
        for ms in self.m["milestones"]:
            if not ms["persists"]:
                self.done.pop(ms["id"], None)
        self.runs.append(dict(run=self.prestiges, start_minutes=self.run_start / 60.0, minutes=elapsed / 60.0,
                              gain=gain, gain_per_minute=gain / max(elapsed / 60.0, 1e-9),
                              top_steps=dict(self.run_top), new_beats=self.run_new_beats))
        new = self.run_new_beats > 0
        for l in rs["ladders"]:
            self.ladder_pos[l] = 0
        self.run_top = dict(self.ladder_pos)
        self.run = dict.fromkeys(self.cur, 0.0)
        self.run_start = self.t
        self.best_rate = 0.0
        self.run_new_beats = 0
        self.changed()
        weight = WEIGHT["prestige_first"] if self.prestiges == 1 else (WEIGHT["prestige"] if new else WEIGHT["repeat"])
        self.log("prestige", "prestige", f"Prestige {self.prestiges}", pr["system"], weight,
                 note=f"+{gain:g} {pr['currency']}; run {elapsed / 60.0:.1f} min"
                      + ("" if new or self.prestiges == 1 else "; nothing new in this run"))
        return True

    # -- the loop ---------------------------------------------------------------
    def sample(self, inc, cands):
        now = sum(1 for _, _, w in cands if w == 0)
        soon = sum(1 for _, _, w in cands if w <= 300)
        capped = lines = 0
        for p in self.m["purchases"]:
            cap = self.cap_of(p)
            if cap is not None:
                lines += 1
                capped += self.level[p["id"]] >= cap
        wall = None
        for l in self.m["ladders"]:
            if (self.axis in (None, l["id"])) and self.ladder_pos[l["id"]] < l["count"]:
                req = self.ladder_req(l, self.ladder_pos[l["id"]] + 1)
                v = self.quantity(req, inc)[0]
                wall = (l["id"], req["type"], req["amount"] / v if v and v > 0 else INF)
                break
        self.samples.append(dict(t=self.t, income=dict(inc), now=now, soon=soon, capped=capped, lines=lines,
                                 total_lines=len(self.m["purchases"]), wall=wall))

    def stall(self, name, note):
        self.log("stall", "stall", name, "-", 0, note=note)

    def play(self):
        self.check_milestones()
        while self.t < self.limit - 1e-9:
            if len(self.events) >= self.max_events:
                self.cut = True
                self.stall("Event limit reached", f"the run was cut at {self.max_events} events. Raise costs, set "
                                                  "a max_level, or group lines")
                break
            if self.away > 0 and self.block_left <= 1e-9:
                self.advance(self.away, self.income(), factor=self.offline, play=False)
                self.block_left = self.session
                self.check_milestones()     # beats that happened while away are met now
                continue
            inc = self.income()
            if self.try_prestige(inc):
                self.check_milestones()
                continue
            cands = self.candidates(inc)
            self.sample(inc, cands)
            w = self.weights(inc)
            best = None
            for p, costs, wait in cands:
                if wait == INF:
                    continue
                v = self.value(p, inc, w)
                if v <= 0:
                    continue
                prio = (wait + 1.0) / v
                if best is None or prio < best[0]:
                    best = (prio, p, costs, wait)
            if best and best[3] > 0:
                # while the buyer saves for the target, it buys what the target does not pay with
                saving = {c for c, _ in best[2]}
                side = [(self.value(p, inc, w), p, costs) for p, costs, wait in cands
                        if wait == 0 and not saving & {c for c, _ in costs}]
                side = [x for x in side if x[0] > 0]
                if side:
                    _, p, costs = max(side, key=lambda x: x[0])
                    best = (0.0, p, costs, 0.0)
            passive = self.next_passive(inc)
            if best is None and passive == INF:
                if not self.pr:
                    self.stall("Nothing left to buy or reach", "content ends here, or a requirement can never be met")
                    break
                if self.t - self.last_change >= max(3600.0, 0.5 * self.last_change):
                    self.stall("No reachable goal", "prestige is enabled, but the buyer did not reset and nothing "
                                                    "else changed for this long")
                    break
            step_cap = max(60.0, 0.05 * (self.t - self.run_start))
            wait = best[3] if best else INF
            dt = min(wait, passive * (1 + 1e-12) + 1e-9, step_cap, self.limit - self.t)
            if self.away > 0:
                dt = min(dt, max(self.block_left, 0.0))
            if dt > 0:
                self.advance(dt, inc)
            if best and dt >= wait:
                self.buy(best[1], best[2], inc)
            self.check_milestones()
        return self
