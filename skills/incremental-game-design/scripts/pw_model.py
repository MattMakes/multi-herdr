"""Model loading and validation for progression_workbook.py. Format: references/model-schema.md."""
import json
import math
import os
import pathlib
import sys

INF = float("inf")
BIG = 1e300
FORMULAS = ("exp", "linear", "poly")
REQ_CUR = ("total", "run_total", "have", "rate")
REQ_TYPES = REQ_CUR + ("time", "level", "milestone", "ladder", "run_ladder", "prestiges")
LADDER_REQ = REQ_CUR + ("time", "prestiges")
PROFILES = {
    "eager": dict(policy="peak_rate", peak_tolerance=0.95,
                  text="Resets when the prestige gain for each minute falls to 95% of its best value in the run."),
    "patient": dict(policy="peak_rate", peak_tolerance=0.75,
                    text="Resets when the prestige gain for each minute falls to 75% of its best value in the run."),
    "doubler": dict(policy="ratio", policy_ratio=1.0,
                    text="Resets when the gain is at or above the lifetime total of the prestige currency."),
}


GAME_DEFAULTS = (("session_minutes", 20), ("target_hours", 20), ("active_fraction", 1.0), ("away_minutes", 0),
                 ("offline_rate", 0.0), ("pct_grouping", "currency"))
GAME_MINIMUMS = (("session_minutes", 0.01), ("target_hours", 0.001), ("active_fraction", 0), ("away_minutes", 0))
LIST_KINDS = ("currencies", "systems", "purchases", "milestones", "ladders", "interactions")
ID_KINDS = ("currencies", "systems", "purchases", "milestones", "ladders")
EFFECT_OWNERS = ("purchases", "milestones", "ladders")
EFFECT_KINDS = ("add", "pct", "mult", "cap")
GRANT_NUMBERS = ("amount", "base", "growth", "count")
REQ_ID_KIND = {"level": "purchases", "milestone": "milestones", "ladder": "ladders", "run_ladder": "ladders"}
END_KINDS = {"axis_step": None, "purchase": "purchases", "resets": None, "milestone": "milestones"}
PRESTIGE_DEFAULTS = (("base", 1.0, 1e-300), ("k", 1.0, 0), ("exponent", 0.5, 0), ("growth", 1.5, 0),
                     ("min_gain", 1, 0), ("min_run_seconds", 60, 0), ("policy_ratio", 1.0, 0),
                     ("peak_tolerance", 0.9, 0))
RESET_KINDS = (("currencies", "currencies"), ("purchase_systems", "systems"), ("ladders", "ladders"))
REVIEW_LISTS = ("findings", "lull_notes", "ideas")
PRESENCE_DEFAULTS = (("open_rate", 1.0), ("open_cap_minutes", None), ("offline_cap_minutes", None),
                     ("visit_minutes", 2.0))
PRESENCE_CAPS = ("open_cap_minutes", "offline_cap_minutes")
CLOCKS = ("game", "real")
LADDER_TIMERS = ("attempt_seconds", "cooldown_seconds")
TARGET_BANDS = ("policy_gap", "unlocks_first_hour", "unlocks_per_hour", "length_hours", "slowdown")
IDLE_STATES = ("open", "closed")


def fail(msg):
    sys.exit("model error: " + msg)


def check(bad, msg):
    """Stop with a model error when bad is true."""
    if not bad:
        return
    fail(msg)


def num(x, where, low=None):
    check(isinstance(x, bool) or not isinstance(x, (int, float)) or x != x, f"{where} must be a number, not {x!r}")
    check(low is not None and x < low, f"{where} must be {low} or more, not {x!r}")
    return x


def prestige_on(m):
    pr = m.get("prestige")
    return bool(pr) and pr.get("enabled", True)


def _linear_cost(b, g, level):
    return b + g * level


def _poly_cost(b, g, level):
    return b * (level + 1) ** g


def _exp_cost(b, g, level):
    too_big = g > 0 and level * math.log(g) > 690
    return INF if too_big else b * g ** level


COST_FORMULAS = {"linear": _linear_cost, "poly": _poly_cost}


def cost_one(c, level):
    """Cost of one cost entry at a level. Returns INF on overflow."""
    b, g, f = c["base"], c["growth"], c["formula"]
    try:
        v = COST_FORMULAS.get(f, _exp_cost)(b, g, level)
    except OverflowError:
        return INF
    return v if v < BIG else INF


def cost_at(p, level):
    """Cost of the first cost entry (the single-currency view)."""
    return cost_one(p["costs"][0], level)


def _check_amount_req(owner, r, t, ids):
    num(r.get("amount", 0), f"{owner}: requirement '{t}' amount")


def _check_currency_req(owner, r, t, ids):
    check(r.get("currency") not in ids["currencies"],
          f"{owner}: requirement '{t}' names the unknown currency '{r.get('currency')}'")
    _check_amount_req(owner, r, t, ids)


def _check_id_req(owner, r, t, ids):
    kind = REQ_ID_KIND[t]
    check(r.get("id") not in ids[kind], f"{owner}: requirement '{t}' names the unknown id '{r.get('id')}' (not in '{kind}')")


REQ_CHECKERS = dict(dict.fromkeys(REQ_CUR, _check_currency_req), time=_check_amount_req,
                    prestiges=_check_amount_req, level=_check_id_req, milestone=_check_id_req, ladder=_check_id_req,
                    run_ladder=_check_id_req)


def _check_req(owner, r, ids):
    check(not isinstance(r, dict), f"{owner}: each requirement must be an object with a 'type'")
    t = r.get("type")
    check(t not in REQ_TYPES, f"{owner}: unknown requirement type '{t}'. Use one of: {', '.join(REQ_TYPES)}")
    REQ_CHECKERS[t](owner, r, t, ids)


def check_reqs(m, owner, reqs, ids):
    reqs = [reqs] if isinstance(reqs, dict) else reqs
    check(not isinstance(reqs, list), f"{owner}: 'requires' must be a list of requirements")
    for r in reqs:
        _check_req(owner, r, ids)
    return reqs


def _read_model_file(path):
    try:
        m = json.loads(pathlib.Path(path).read_text(encoding="utf-8"))
    except OSError as e:
        fail(f"cannot read '{path}': {e.strerror}")
    except ValueError as e:
        fail(f"'{path}' is not valid JSON: {e}")
    return m


def _game_defaults(m, path):
    g = m.setdefault("game", {})
    g.setdefault("name", os.path.splitext(os.path.basename(path))[0])
    for k, d in GAME_DEFAULTS:
        g.setdefault(k, d)
    return g


def _check_game(g):
    for k, low in GAME_MINIMUMS:
        num(g[k], f"game.{k}", low)
    check(not 0 <= num(g["offline_rate"], "game.offline_rate") <= 1, "game.offline_rate must be from 0 to 1")
    check(g["pct_grouping"] not in ("currency", "system"), "game.pct_grouping must be 'currency' or 'system'")
    _check_presence(g)
    _check_targets(g)


def check_flag(x, key, where):
    """Stop with a model error when x has the key and its value is not true or false."""
    check(key in x and not isinstance(x[key], bool), f"{where}: '{key}' must be true or false")


def presence_of(g):
    """The presence block of the game with its defaults. A model with no block gets the defaults."""
    out = dict(PRESENCE_DEFAULTS)
    out.update(g.get("presence") or {})
    return out


def _check_presence(g):
    if "presence" not in g:
        return
    check(not isinstance(g["presence"], dict), "game.presence must be an object")
    pr = g["presence"] = presence_of(g)
    check(not 0 <= num(pr["open_rate"], "game.presence.open_rate") <= 1, "game.presence.open_rate must be from 0 to 1")
    for k in [k for k in PRESENCE_CAPS if pr.get(k) is not None]:
        num(pr[k], f"game.presence.{k}", 0)
    num(pr["visit_minutes"], "game.presence.visit_minutes", 0.01)


def check_band(where, b):
    check(not isinstance(b, list) or len(b) != 2, f"{where} must be a list [low, high]")
    num(b[0], where)
    num(b[1], where)


def _check_idle_band(state, hours, b):
    where = f"game.targets.idle.{state}.{hours}"
    try:
        float(hours)
    except ValueError:
        fail(f"{where}: the key must be a number of hours")
    check_band(where, b)


def _check_idle_state(state, rows):
    check(state not in IDLE_STATES, f"game.targets.idle: the key '{state}' must be 'open' or 'closed'")
    check(not isinstance(rows, dict), f"game.targets.idle.{state} must be an object: {{\"8\": [low, high]}}")
    for hours, b in rows.items():
        _check_idle_band(state, hours, b)


def _check_targets(g):
    t = g.get("targets")
    if t is None:
        return
    check(not isinstance(t, dict), "game.targets must be an object")
    for k in [k for k in TARGET_BANDS if k in t]:
        check_band(f"game.targets.{k}", t[k])
    if "play_test_speed" in t:
        num(t["play_test_speed"], "game.targets.play_test_speed", 1)
    for key, v in (t.get("tolerances") or {}).items():
        num(v, f"game.targets.tolerances.{key}", 0)
    idle = t.get("idle") or {}
    check(not isinstance(idle, dict), "game.targets.idle must be an object")
    for state, rows in idle.items():
        _check_idle_state(state, rows)


def _list_default(m, k):
    m.setdefault(k, [])
    check(not isinstance(m[k], list), f"'{k}' must be a list")


def _list_defaults(m):
    for k in LIST_KINDS:
        _list_default(m, k)
    return m


def _add_id(ids, kind, x):
    check(not isinstance(x, dict) or "id" not in x, f"an entry in '{kind}' has no id")
    check(x["id"] in ids, f"duplicate id '{x['id']}' in '{kind}'")
    ids.add(x["id"])
    x.setdefault("name", x["id"])


def _kind_ids(m, kind):
    ids = set()
    for x in m[kind]:
        _add_id(ids, kind, x)
    return ids


def _collect_ids(m):
    return {kind: _kind_ids(m, kind) for kind in ID_KINDS}


def _check_currency(c):
    num(c.get("base_income", 0), f"currency '{c['id']}': base_income")
    num(c.get("start", 0), f"currency '{c['id']}': start")
    if "bank_cap" in c:
        num(c["bank_cap"], f"currency '{c['id']}': bank_cap", 0)


def _check_currencies(m, cur):
    check(not cur, "the model has no currencies")
    for c in m["currencies"]:
        _check_currency(c)


def _entry_cost(w, c):
    check(not isinstance(c, dict), f"{w}: each entry in 'costs' must be an object")
    return dict(currency=c.get("currency"), base=c.get("base", 1), growth=c.get("growth", 1.15),
                formula=c.get("formula", "exp"))


def _list_costs(p, w):
    check(not isinstance(p["costs"], list) or not p["costs"], f"{w}: 'costs' must be a list with one entry or more")
    return [_entry_cost(w, c) for c in p["costs"]]


def _single_cost(p):
    return [dict(currency=p.get("cost_currency"), base=p.get("base_cost", 1),
                 growth=p.get("cost_growth", 1.15), formula=p.get("cost_formula", "exp"))]


def _purchase_costs(p, w):
    return _list_costs(p, w) if "costs" in p else _single_cost(p)


def _check_cost_currency(c, w, cur, seen):
    check(c["currency"] not in cur, f"{w}: unknown cost currency '{c['currency']}'")
    check(c["currency"] in seen, f"{w}: the currency '{c['currency']}' is in 'costs' two times")
    seen.add(c["currency"])


def _check_cost_formula(c, w):
    check(c["formula"] not in FORMULAS, f"{w}: unknown cost formula '{c['formula']}'. Use one of: {', '.join(FORMULAS)}")
    num(c["base"], f"{w}: base cost", 0)
    num(c["growth"], f"{w}: cost growth")
    check(c["formula"] == "exp" and c["growth"] <= 0, f"{w}: cost growth must be more than 0 for the 'exp' formula")


def _check_cost(c, w, cur, seen):
    _check_cost_currency(c, w, cur, seen)
    _check_cost_formula(c, w)


def _check_costs(costs, w, cur):
    seen = set()
    for c in costs:
        _check_cost(c, w, cur, seen)


def _set_cost_fields(p, costs):
    p["costs"] = costs
    p["cost_currency"], p["base_cost"] = costs[0]["currency"], costs[0]["base"]
    p["cost_growth"], p["cost_formula"] = costs[0]["growth"], costs[0]["formula"]
    p.setdefault("max_level", None)


def _free_warning(p):
    return (f"Purchase '{p['id']}' has a cost that does not grow and no max_level. The buyer can buy it "
            "without end, and the event limit can cut the run.")


def _check_level_cap(p, w, costs, warn):
    num(p["max_level"], f"{w}: max_level", 0)


def _check_no_cap(p, w, costs, warn):
    check(all(cost_one(c, n) <= 0 for c in costs for n in (0, 1, 2, 5)),
          f"{w}: the cost is 0 at each level and there is no max_level. Set a cost or a max_level")
    free = all(c["formula"] == "exp" and c["growth"] <= 1 for c in costs)
    if free:
        warn.append(_free_warning(p))


def _check_purchase(p, cur, warn):
    w = f"purchase '{p['id']}'"
    costs = _purchase_costs(p, w)
    _check_costs(costs, w, cur)
    _set_cost_fields(p, costs)
    level_check = _check_level_cap if p["max_level"] is not None else _check_no_cap
    level_check(p, w, costs, warn)
    p.setdefault("effects", [])
    p.setdefault("requires", [])
    _check_count_steps(p, w)


def _check_count_steps(p, w):
    """count_steps {"every": N, "mult": m}: the add effects of the line take x m at each N owned levels (card
    S12.count_steps). A line with no count step keeps no key."""
    steps = p.get("count_steps")
    if steps is None:
        return
    check(not isinstance(steps, dict), f"{w}: count_steps must be an object with the keys every and mult")
    every, mult = num(steps.get("every"), f"{w}: count_steps.every", 1), num(steps.get("mult"), f"{w}: count_steps.mult", 0)
    check(every != int(every), f"{w}: count_steps.every must be a whole number")
    check(not any(e.get("kind") == "add" for e in p["effects"]),
          f"{w}: count_steps needs an add effect on the line (the steps multiply the add effects)")
    p["count_steps"] = dict(every=int(every), mult=mult)


def _add_system(m, sysids, x):
    x.setdefault("system", "core")
    if x["system"] in sysids:
        return
    m["systems"].append({"id": x["system"], "name": x["system"], "loop": "unknown"})
    sysids.add(x["system"])


def _add_systems(m, sysids):
    owners = m["purchases"] + m["milestones"] + m["ladders"]
    for x in owners:
        _add_system(m, sysids, x)


def _check_cap_target(e, w, x, ids, pmap, warn):
    check(e.get("target") not in ids["purchases"],
          f"{w}: a 'cap' effect needs a purchase id as its target, not '{e.get('target')}'")
    if pmap[e["target"]]["max_level"] is None:
        warn.append(f"The cap effect of '{x['id']}' has no result: '{e['target']}' has no max_level.")


def _check_stat_target(e, w, x, ids, pmap, warn):
    cur = ids["currencies"]
    check(e.get("target") not in cur and e.get("target") != "all",
          f"{w}: effect target '{e.get('target')}' is not a currency id or 'all'")


def _check_effect(e, w, x, ids, pmap, warn):
    check(not isinstance(e, dict) or e.get("kind") not in EFFECT_KINDS, f"{w}: effect kind must be add, pct, mult, or cap")
    num(e.get("value"), f"{w}: effect value")
    target_check = _check_cap_target if e["kind"] == "cap" else _check_stat_target
    target_check(e, w, x, ids, pmap, warn)
    check(e["kind"] == "mult" and e["value"] < 0, f"{w}: a 'mult' effect cannot be negative")


def _check_owner_effects(kind, x, ids, pmap, warn):
    w = f"{kind[:-1]} '{x['id']}'"
    check(x.get("clock", "game") not in CLOCKS, f"{w}: 'clock' must be 'game' or 'real'")
    x.setdefault("effects", [])
    check(not isinstance(x["effects"], list), f"{w}: 'effects' must be a list")
    for e in x["effects"]:
        _check_effect(e, w, x, ids, pmap, warn)


def _check_effects(m, ids, warn):
    pmap = {p["id"]: p for p in m["purchases"]}
    owners = [(kind, x) for kind in EFFECT_OWNERS for x in m[kind]]
    for kind, x in owners:
        _check_owner_effects(kind, x, ids, pmap, warn)


def _system_defaults(s):
    s.setdefault("loop", "unknown")
    s.setdefault("persists", s["loop"] in ("meta", "collection"))
    s.setdefault("clock", "game")


def _check_grant(owner, gr, cur, ladder):
    check(not isinstance(gr, dict) or gr.get("currency") not in cur,
          f"{owner}: a grant names the unknown currency '{gr.get('currency') if isinstance(gr, dict) else gr}'")
    check("amount" not in gr and not (ladder and "base" in gr),
          f"{owner}: a grant needs an 'amount'" + (" or a 'base'" if ladder else ""))
    for k in [k for k in GRANT_NUMBERS if k in gr]:
        num(gr[k], f"{owner}: grant {k}")


def check_grants(owner, grants, cur, ladder=False):
    check(not isinstance(grants, list), f"{owner}: 'grants' must be a list")
    for gr in grants:
        _check_grant(owner, gr, cur, ladder)


def _check_milestone(m, ms, ids):
    owner = f"milestone '{ms['id']}'"
    ms.setdefault("kind", "feature")
    ms.setdefault("grants", [])
    ms["requires"] = check_reqs(m, owner, ms.setdefault("requires", []), ids)
    check_grants(owner, ms["grants"], ids["currencies"])


def _check_requirements(m, ids):
    purchases, milestones = m["purchases"], m["milestones"]
    for p in purchases:
        p["requires"] = check_reqs(m, f"purchase '{p['id']}'", p["requires"], ids)
    for ms in milestones:
        _check_milestone(m, ms, ids)


def _ladder_requirement(l, warn):
    r = l.get("requires")
    single = isinstance(r, list) and len(r) == 1 and isinstance(r[0], dict)
    if not single:
        return r
    l["requires"] = r[0]
    warn.append(f"Ladder '{l['id']}': 'requires' is a list. The script read its one entry. Write one object.")
    return r[0]


def _check_ladder_type(w, r, cur):
    check(not isinstance(r, dict), f"{w}: 'requires' must be one object: {{\"type\": ..., \"currency\": ..., \"base\": ..., "
                                   f"\"growth\": ...}}")
    check(r.get("type") not in LADDER_REQ, f"{w}: requirement type must be one of: {', '.join(LADDER_REQ)}")
    check(r["type"] in REQ_CUR and r.get("currency") not in cur,
          f"{w}: the requirement names the unknown currency '{r.get('currency')}'")


def _check_ladder_numbers(w, r):
    check("base" not in r, f"{w}: the requirement needs a 'base' (the amount for step 1)")
    num(r["base"], f"{w}: requirement base")
    num(r.setdefault("growth", 1.0), f"{w}: requirement growth", 0)
    if "table" in r:
        _check_ladder_table(w, r["table"])


def _check_ladder_table(w, table):
    check(not isinstance(table, list) or not table, f"{w}: requirement 'table' must be a list with 1 number or more")
    for x in table:
        num(x, f"{w}: requirement table entry", 0)


def _check_ladder_timers(w, l):
    for k in [k for k in LADDER_TIMERS if k in l]:
        num(l[k], f"{w}: {k}", 0)
    check_flag(l, "needs_visit", w)


def _check_ladder(l, cur, warn):
    w = f"ladder '{l['id']}'"
    r = _ladder_requirement(l, warn)
    _check_ladder_type(w, r, cur)
    _check_ladder_numbers(w, r)
    num(l.setdefault("count", 50), f"{w}: count", 1)
    _check_ladder_timers(w, l)
    l["count"] = int(l["count"])
    l.setdefault("grants", [])
    check_grants(w, l["grants"], cur, ladder=True)


def _check_ladder_basis(pr, ids):
    check(pr["basis_ladder"] not in ids["ladders"], f"prestige: 'basis_ladder' names the unknown ladder '{pr['basis_ladder']}'")
    pr.setdefault("formula", "power")
    check(pr["formula"] not in ("power", "exp"), "prestige: with 'basis_ladder' the formula must be 'power' or 'exp'")


def _check_currency_basis(pr, ids):
    check(pr.get("basis_currency") not in ids["currencies"],
          f"prestige: 'basis_currency' must be a currency id, not '{pr.get('basis_currency')}'. Or set 'basis_ladder'")
    pr.setdefault("formula", "power")
    check(pr["formula"] not in ("power", "log"), f"prestige: unknown formula '{pr['formula']}'. Use 'power' or 'log'")


def _check_prestige_policy(pr):
    if "run_seconds" in pr:
        num(pr["run_seconds"], "prestige.run_seconds", 1)
    if pr.get("max_resets") is not None:
        pr["max_resets"] = int(num(pr["max_resets"], "prestige.max_resets", 1))
    pr["_policy_set"] = "policy" in pr
    pr.setdefault("policy", "peak_rate")
    check(pr["policy"] not in ("peak_rate", "ratio"), f"prestige: unknown policy '{pr['policy']}'. Use 'peak_rate' or 'ratio'")


def _check_prestige_presence(pr, ids):
    """The optional fields needs_visit, floor and reward of the prestige block."""
    check_flag(pr, "needs_visit", "prestige")
    check_flag(pr, "floor", "prestige")
    check_flag(pr, "peak_on_floor", "prestige")
    rw = pr.get("reward")
    if rw is None:
        return
    check(not isinstance(rw, dict), "prestige: 'reward' must be an object: {\"target\": ..., \"exponent\": ...}")
    tgt = rw.setdefault("target", "all")
    check(tgt != "all" and tgt not in ids["currencies"], f"prestige: reward target '{tgt}' is not a currency id or 'all'")
    num(rw.get("exponent"), "prestige: reward exponent", 0)


def _check_reset_ids(rs, key, known):
    values = rs.setdefault(key, [])
    for i in values:
        check(i not in known, f"prestige: resets.{key} names the unknown id '{i}'")


def _check_resets(pr, ids):
    rs = pr.setdefault("resets", {})
    check(not isinstance(rs, dict), "prestige: 'resets' must be an object")
    for key, kind in RESET_KINDS:
        _check_reset_ids(rs, key, ids[kind])


def _run_requires(pr, reqs):
    """With unlock_in_run true, each ladder requirement of the reset tests the step of the current run."""
    check_flag(pr, "unlock_in_run", "prestige")
    if not pr.get("unlock_in_run"):
        return reqs
    return [dict(r, type="run_ladder") if r.get("type") == "ladder" else r for r in reqs]


def _check_prestige_block(m, pr, ids):
    check(pr.get("currency") not in ids["currencies"], f"prestige: 'currency' must be a currency id, not '{pr.get('currency')}'")
    basis_check = _check_ladder_basis if "basis_ladder" in pr else _check_currency_basis
    basis_check(pr, ids)
    for k, d, low in PRESTIGE_DEFAULTS:
        num(pr.setdefault(k, d), f"prestige.{k}", low)
    _check_prestige_policy(pr)
    _check_prestige_presence(pr, ids)
    pr["requires"] = _run_requires(pr, check_reqs(m, "prestige", pr.setdefault("requires", []), ids))
    _check_resets(pr, ids)
    pr.setdefault("system", "prestige")
    check_grants("prestige", pr.get("grants") or [], ids["currencies"], ladder=True)


def _check_prestige(m, ids):
    pr = m.get("prestige")
    check(pr is not None and not isinstance(pr, dict), "'prestige' must be an object")
    if not prestige_on(m):
        m["prestige"] = dict(pr or {}, enabled=False)
        return
    _check_prestige_block(m, pr, ids)


def _reset_set(m, key):
    return set(m["prestige"].get("resets", {}).get(key, [])) if prestige_on(m) else set()


def _milestone_text(ms):
    return (f"Milestone '{ms['id']}' has an effect and is in the system '{ms['system']}', which the "
            "prestige resets. The model does not set 'persists' on it, so the script resets it with its "
            "system. Set \"persists\": true if the effect is permanent.")


def _milestone_warning(ms, reset_sys, warn):
    said = "persists" in ms
    ms.setdefault("persists", ms["system"] not in reset_sys)
    if not said and not ms["persists"] and ms["effects"]:
        warn.append(_milestone_text(ms))


def _ladder_text(l):
    return (f"Ladder '{l['id']}' restarts at the prestige and has effects. The model does not set "
            "'effects_persist', so the script keeps the effects of the best step for ever. "
            "Set \"effects_persist\": false if they follow the current run.")


def _ladder_warning(l, reset_lad, warn):
    if l["id"] in reset_lad and l["effects"] and "effects_persist" not in l:
        warn.append(_ladder_text(l))
    l.setdefault("effects_persist", True)


def _system_warnings(m, reset_sys):
    on = prestige_on(m)
    return [f"System '{s['id']}' says persists = true, but prestige.resets.purchase_systems names it."
            for s in m["systems"] if on and s["id"] in reset_sys and s["persists"]]


def _persistence_warnings(m, warn):
    reset_sys, reset_lad = _reset_set(m, "purchase_systems"), _reset_set(m, "ladders")
    for ms in m["milestones"]:
        _milestone_warning(ms, reset_sys, warn)
    for l in m["ladders"]:
        _ladder_warning(l, reset_lad, warn)
    warn.extend(_system_warnings(m, reset_sys))


def _check_game_ids(g, ids, warn):
    ax = g.get("progress_axis")
    if ax is not None and ax not in ids["ladders"]:
        warn.append(f"game.progress_axis '{ax}' is not a ladder id. The script did not use it.")
        g["progress_axis"] = None
    pp = g.get("primary_profile")
    names = list(PROFILES) + ["model"]
    check(pp is not None and pp not in names, f"game.primary_profile must be one of: {', '.join(names)}")
    _check_hard_end((g.get("targets") or {}).get("hard_end"), ids)


def _check_hard_end(mark, ids):
    """game.targets.hard_end: 1 key of END_KINDS. axis_step and resets are counts; purchase and milestone are ids."""
    if mark is None:
        return
    keys = [k for k in END_KINDS if k in (mark if isinstance(mark, dict) else {})]
    check(len(keys) != 1, "game.targets.hard_end must have 1 key of: " + ", ".join(END_KINDS))
    kind = END_KINDS[keys[0]]
    if kind is None:
        mark[keys[0]] = int(num(mark[keys[0]], f"game.targets.hard_end.{keys[0]}", 1))
        return
    check(mark[keys[0]] not in ids[kind], f"game.targets.hard_end.{keys[0]} names the unknown id '{mark[keys[0]]}'")


def _check_interaction(i):
    check(not isinstance(i, dict) or "from" not in i or "to" not in i, "each entry in 'interactions' needs 'from' and 'to'")


def _check_lull_note(x):
    check(not isinstance(x, dict), "each entry in review.lull_notes must be an object")
    num(x.get("lull_at_hours"), "review.lull_notes: lull_at_hours")


def _check_review(m):
    for i in m["interactions"]:
        _check_interaction(i)
    check(not isinstance(m.get("review") or {}, dict), "'review' must be an object")
    rv = m.get("review") or {}
    for k in REVIEW_LISTS:
        check(not isinstance(rv.get(k) or [], list), f"review.{k} must be a list")
    for x in rv.get("lull_notes") or []:
        _check_lull_note(x)
    check(not isinstance(m.get("bands") or {}, dict), "'bands' must be an object")


def _check_entries(m, ids, warn):
    cur = ids["currencies"]
    _check_currencies(m, cur)
    for p in m["purchases"]:
        _check_purchase(p, cur, warn)
    _add_systems(m, ids["systems"])
    _check_effects(m, ids, warn)
    for s in m["systems"]:
        _system_defaults(s)


def _check_rules(m, g, ids, warn):
    cur = ids["currencies"]
    _check_requirements(m, ids)
    for l in m["ladders"]:
        _check_ladder(l, cur, warn)
    _check_prestige(m, ids)
    _persistence_warnings(m, warn)
    _check_game_ids(g, ids, warn)
    _check_review(m)


UPKEEP_RULES = ("need_capacity",)
UPKEEP_KINDS = ("add", "mult")


def _check_upkeep_entry(x, where, pids, kinds=None):
    check(not isinstance(x, dict) or x.get("purchase") not in pids,
          f"{where}: each entry needs 'purchase', the id of a purchase line")
    num(x.get("per_level", 1.0), f"{where}.per_level", 0)
    if kinds:
        check(x.get("kind", "add") not in kinds, f"{where}.kind must be one of {', '.join(kinds)}")
        num(x.get("value"), f"{where}.value", 0)


def _check_upkeep(m, ids):
    """The key upkeep (S30): u = max(floor, min(1, C / N)^exponent) on the income of 1 currency."""
    up = m.get("upkeep")
    if not up:
        return
    check(not isinstance(up, dict), "upkeep must be an object")
    check(up.get("target") not in ids["currencies"], "upkeep.target must be a currency id")
    check(up.get("rule", "need_capacity") not in UPKEEP_RULES, f"upkeep.rule must be one of {', '.join(UPKEEP_RULES)}")
    _check_upkeep_numbers(up)
    for x in up.get("need") or []:
        _check_upkeep_entry(x, "upkeep.need", ids["purchases"])
    for x in up.get("capacity") or []:
        _check_upkeep_entry(x, "upkeep.capacity", ids["purchases"], UPKEEP_KINDS)


def _check_upkeep_numbers(up):
    for key in ("base_need", "base_capacity", "exponent", "floor"):
        num(up.get(key, 0.0), f"upkeep.{key}", 0)
    check(up.get("floor", 0.0) > 1, "upkeep.floor must be 1 or less")
    band = up.get("band", [0.0, 1.0])
    check(len(band) != 2 or not 0 <= band[0] <= band[1] <= 1, "upkeep.band must be [low, high] inside 0 to 1")


def load_model(path):
    m = _read_model_file(path)
    check(not isinstance(m, dict), "the top level of the model must be an object")
    warn = m["_warnings"] = []
    g = _game_defaults(m, path)
    _check_game(g)
    _list_defaults(m)
    ids = _collect_ids(m)
    _check_entries(m, ids, warn)
    _check_rules(m, g, ids, warn)
    _check_upkeep(m, ids)
    return m


def profiles_for(m):
    """List of (name, settings, text). One entry when the buyer has no reset choice."""
    if not prestige_on(m):
        return [("no reset", {}, "The model has no prestige. One run.")]
    pr = m["prestige"]
    if "run_seconds" in pr:
        return [("fixed run", {}, f"Each run ends after {pr['run_seconds']:g} seconds. The buyer has no reset choice.")]
    out = [(n, {k: v for k, v in s.items() if k != "text"}, s["text"]) for n, s in PROFILES.items()]
    if pr.get("_policy_set") or m["game"].get("primary_profile") == "model":
        out.append(("model", dict(policy=pr["policy"], peak_tolerance=pr["peak_tolerance"],
                                  policy_ratio=pr["policy_ratio"]),
                    "The policy that the model sets in its prestige block."))
    return out


def primary_name(m, profs):
    want = m["game"].get("primary_profile")
    names = [p[0] for p in profs]
    return want if want in names else names[0]
