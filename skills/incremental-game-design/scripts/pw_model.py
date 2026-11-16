"""Model loading and validation for progression_workbook.py. Format: references/model-schema.md."""
import json
import math
import os
import sys

INF = float("inf")
BIG = 1e300
FORMULAS = ("exp", "linear", "poly")
REQ_CUR = ("total", "run_total", "have", "rate")
REQ_TYPES = REQ_CUR + ("time", "level", "milestone", "ladder", "prestiges")
LADDER_REQ = REQ_CUR + ("time", "prestiges")
PROFILES = {
    "eager": dict(policy="peak_rate", peak_tolerance=0.95,
                  text="Resets when the prestige gain for each minute falls to 95% of its best value in the run."),
    "patient": dict(policy="peak_rate", peak_tolerance=0.75,
                    text="Resets when the prestige gain for each minute falls to 75% of its best value in the run."),
    "doubler": dict(policy="ratio", policy_ratio=1.0,
                    text="Resets when the gain is at or above the lifetime total of the prestige currency."),
}


def fail(msg):
    sys.exit("model error: " + msg)


def num(x, where, low=None):
    if isinstance(x, bool) or not isinstance(x, (int, float)) or x != x:
        fail(f"{where} must be a number, not {x!r}")
    if low is not None and x < low:
        fail(f"{where} must be {low} or more, not {x!r}")
    return x


def prestige_on(m):
    pr = m.get("prestige")
    return bool(pr) and pr.get("enabled", True)


def cost_one(c, level):
    """Cost of one cost entry at a level. Returns INF on overflow."""
    b, g, f = c["base"], c["growth"], c["formula"]
    try:
        if f == "linear":
            v = b + g * level
        elif f == "poly":
            v = b * (level + 1) ** g
        else:
            if g > 0 and level * math.log(g) > 690:
                return INF
            v = b * g ** level
    except OverflowError:
        return INF
    return v if v < BIG else INF


def cost_at(p, level):
    """Cost of the first cost entry (the single-currency view)."""
    return cost_one(p["costs"][0], level)


def check_reqs(m, owner, reqs, ids):
    if isinstance(reqs, dict):
        reqs = [reqs]
    if not isinstance(reqs, list):
        fail(f"{owner}: 'requires' must be a list of requirements")
    for r in reqs:
        if not isinstance(r, dict):
            fail(f"{owner}: each requirement must be an object with a 'type'")
        t = r.get("type")
        if t not in REQ_TYPES:
            fail(f"{owner}: unknown requirement type '{t}'. Use one of: {', '.join(REQ_TYPES)}")
        if t in REQ_CUR:
            if r.get("currency") not in ids["currencies"]:
                fail(f"{owner}: requirement '{t}' names the unknown currency '{r.get('currency')}'")
            num(r.get("amount", 0), f"{owner}: requirement '{t}' amount")
        elif t in ("time", "prestiges"):
            num(r.get("amount", 0), f"{owner}: requirement '{t}' amount")
        else:
            kind = {"level": "purchases", "milestone": "milestones", "ladder": "ladders"}[t]
            if r.get("id") not in ids[kind]:
                fail(f"{owner}: requirement '{t}' names the unknown id '{r.get('id')}' (not in '{kind}')")
    return reqs


def load_model(path):
    try:
        m = json.load(open(path))
    except OSError as e:
        fail(f"cannot read '{path}': {e.strerror}")
    except ValueError as e:
        fail(f"'{path}' is not valid JSON: {e}")
    if not isinstance(m, dict):
        fail("the top level of the model must be an object")
    warn = m["_warnings"] = []
    g = m.setdefault("game", {})
    g.setdefault("name", os.path.splitext(os.path.basename(path))[0])
    g.setdefault("session_minutes", 20)
    g.setdefault("target_hours", 20)
    g.setdefault("active_fraction", 1.0)
    g.setdefault("away_minutes", 0)
    g.setdefault("offline_rate", 0.0)
    g.setdefault("pct_grouping", "currency")
    num(g["session_minutes"], "game.session_minutes", 0.01)
    num(g["target_hours"], "game.target_hours", 0.001)
    num(g["active_fraction"], "game.active_fraction", 0)
    num(g["away_minutes"], "game.away_minutes", 0)
    if not 0 <= num(g["offline_rate"], "game.offline_rate") <= 1:
        fail("game.offline_rate must be from 0 to 1")
    if g["pct_grouping"] not in ("currency", "system"):
        fail("game.pct_grouping must be 'currency' or 'system'")
    for k in ("currencies", "systems", "purchases", "milestones", "ladders", "interactions"):
        m.setdefault(k, [])
        if not isinstance(m[k], list):
            fail(f"'{k}' must be a list")
    ids = {}
    for kind in ("currencies", "systems", "purchases", "milestones", "ladders"):
        ids[kind] = set()
        for x in m[kind]:
            if not isinstance(x, dict) or "id" not in x:
                fail(f"an entry in '{kind}' has no id")
            if x["id"] in ids[kind]:
                fail(f"duplicate id '{x['id']}' in '{kind}'")
            ids[kind].add(x["id"])
            x.setdefault("name", x["id"])
    cur = ids["currencies"]
    if not cur:
        fail("the model has no currencies")
    for c in m["currencies"]:
        num(c.get("base_income", 0), f"currency '{c['id']}': base_income")
        num(c.get("start", 0), f"currency '{c['id']}': start")

    # ---- purchases: one cost form inside the script (a list), two forms in the file
    for p in m["purchases"]:
        w = f"purchase '{p['id']}'"
        if "costs" in p:
            if not isinstance(p["costs"], list) or not p["costs"]:
                fail(f"{w}: 'costs' must be a list with one entry or more")
            costs = []
            for c in p["costs"]:
                if not isinstance(c, dict):
                    fail(f"{w}: each entry in 'costs' must be an object")
                costs.append(dict(currency=c.get("currency"), base=c.get("base", 1), growth=c.get("growth", 1.15),
                                  formula=c.get("formula", "exp")))
        else:
            costs = [dict(currency=p.get("cost_currency"), base=p.get("base_cost", 1),
                          growth=p.get("cost_growth", 1.15), formula=p.get("cost_formula", "exp"))]
        seen = set()
        for c in costs:
            if c["currency"] not in cur:
                fail(f"{w}: unknown cost currency '{c['currency']}'")
            if c["currency"] in seen:
                fail(f"{w}: the currency '{c['currency']}' is in 'costs' two times")
            seen.add(c["currency"])
            if c["formula"] not in FORMULAS:
                fail(f"{w}: unknown cost formula '{c['formula']}'. Use one of: {', '.join(FORMULAS)}")
            num(c["base"], f"{w}: base cost", 0)
            num(c["growth"], f"{w}: cost growth")
            if c["formula"] == "exp" and c["growth"] <= 0:
                fail(f"{w}: cost growth must be more than 0 for the 'exp' formula")
        p["costs"] = costs
        p["cost_currency"], p["base_cost"] = costs[0]["currency"], costs[0]["base"]
        p["cost_growth"], p["cost_formula"] = costs[0]["growth"], costs[0]["formula"]
        p.setdefault("max_level", None)
        if p["max_level"] is not None:
            num(p["max_level"], f"{w}: max_level", 0)
        elif all(cost_one(c, n) <= 0 for c in costs for n in (0, 1, 2, 5)):
            fail(f"{w}: the cost is 0 at each level and there is no max_level. Set a cost or a max_level")
        elif all(c["formula"] == "exp" and c["growth"] <= 1 for c in costs):
            warn.append(f"Purchase '{p['id']}' has a cost that does not grow and no max_level. The buyer can buy it "
                        "without end, and the event limit can cut the run.")
        p.setdefault("effects", [])
        p.setdefault("requires", [])
    sysids = ids["systems"]
    for x in m["purchases"] + m["milestones"] + m["ladders"]:
        x.setdefault("system", "core")
        if x["system"] not in sysids:
            m["systems"].append({"id": x["system"], "name": x["system"], "loop": "unknown"})
            sysids.add(x["system"])
    pmap = {p["id"]: p for p in m["purchases"]}

    # ---- effects
    for kind in ("purchases", "milestones", "ladders"):
        for x in m[kind]:
            w = f"{kind[:-1]} '{x['id']}'"
            x.setdefault("effects", [])
            if not isinstance(x["effects"], list):
                fail(f"{w}: 'effects' must be a list")
            for e in x["effects"]:
                if not isinstance(e, dict) or e.get("kind") not in ("add", "pct", "mult", "cap"):
                    fail(f"{w}: effect kind must be add, pct, mult, or cap")
                num(e.get("value"), f"{w}: effect value")
                if e["kind"] == "cap":
                    if e.get("target") not in ids["purchases"]:
                        fail(f"{w}: a 'cap' effect needs a purchase id as its target, not '{e.get('target')}'")
                    if pmap[e["target"]]["max_level"] is None:
                        warn.append(f"The cap effect of '{x['id']}' has no result: '{e['target']}' has no max_level.")
                elif e.get("target") not in cur and e.get("target") != "all":
                    fail(f"{w}: effect target '{e.get('target')}' is not a currency id or 'all'")
                if e["kind"] == "mult" and e["value"] < 0:
                    fail(f"{w}: a 'mult' effect cannot be negative")

    # ---- systems
    for s in m["systems"]:
        s.setdefault("loop", "unknown")
        s.setdefault("persists", s["loop"] in ("meta", "collection"))
        s.setdefault("clock", "game")

    # ---- requirements and grants
    def check_grants(owner, grants, ladder=False):
        if not isinstance(grants, list):
            fail(f"{owner}: 'grants' must be a list")
        for gr in grants:
            if not isinstance(gr, dict) or gr.get("currency") not in cur:
                fail(f"{owner}: a grant names the unknown currency "
                     f"'{gr.get('currency') if isinstance(gr, dict) else gr}'")
            if "amount" not in gr and not (ladder and "base" in gr):
                fail(f"{owner}: a grant needs an 'amount'" + (" or a 'base'" if ladder else ""))
            for k in ("amount", "base", "growth"):
                if k in gr:
                    num(gr[k], f"{owner}: grant {k}")

    for p in m["purchases"]:
        p["requires"] = check_reqs(m, f"purchase '{p['id']}'", p["requires"], ids)
    for ms in m["milestones"]:
        ms.setdefault("kind", "feature")
        ms.setdefault("grants", [])
        ms["requires"] = check_reqs(m, f"milestone '{ms['id']}'", ms.setdefault("requires", []), ids)
        check_grants(f"milestone '{ms['id']}'", ms["grants"])
    for l in m["ladders"]:
        w = f"ladder '{l['id']}'"
        r = l.get("requires")
        if isinstance(r, list) and len(r) == 1 and isinstance(r[0], dict):
            r = l["requires"] = r[0]
            warn.append(f"Ladder '{l['id']}': 'requires' is a list. The script read its one entry. Write one object.")
        if not isinstance(r, dict):
            fail(f"{w}: 'requires' must be one object: {{\"type\": ..., \"currency\": ..., \"base\": ..., "
                 f"\"growth\": ...}}")
        if r.get("type") not in LADDER_REQ:
            fail(f"{w}: requirement type must be one of: {', '.join(LADDER_REQ)}")
        if r["type"] in REQ_CUR and r.get("currency") not in cur:
            fail(f"{w}: the requirement names the unknown currency '{r.get('currency')}'")
        if "base" not in r:
            fail(f"{w}: the requirement needs a 'base' (the amount for step 1)")
        num(r["base"], f"{w}: requirement base")
        num(r.setdefault("growth", 1.0), f"{w}: requirement growth", 0)
        num(l.setdefault("count", 50), f"{w}: count", 1)
        l["count"] = int(l["count"])
        l.setdefault("grants", [])
        check_grants(w, l["grants"], ladder=True)

    # ---- prestige
    pr = m.get("prestige")
    if pr is not None and not isinstance(pr, dict):
        fail("'prestige' must be an object")
    if not prestige_on(m):
        m["prestige"] = dict(pr or {}, enabled=False)
    else:
        if pr.get("currency") not in cur:
            fail(f"prestige: 'currency' must be a currency id, not '{pr.get('currency')}'")
        if "basis_ladder" in pr:
            if pr["basis_ladder"] not in ids["ladders"]:
                fail(f"prestige: 'basis_ladder' names the unknown ladder '{pr['basis_ladder']}'")
            pr.setdefault("formula", "power")
            if pr["formula"] not in ("power", "exp"):
                fail("prestige: with 'basis_ladder' the formula must be 'power' or 'exp'")
        else:
            if pr.get("basis_currency") not in cur:
                fail(f"prestige: 'basis_currency' must be a currency id, not '{pr.get('basis_currency')}'. "
                     "Or set 'basis_ladder'")
            pr.setdefault("formula", "power")
            if pr["formula"] not in ("power", "log"):
                fail(f"prestige: unknown formula '{pr['formula']}'. Use 'power' or 'log'")
        for k, d, low in (("base", 1.0, 1e-300), ("k", 1.0, 0), ("exponent", 0.5, 0), ("growth", 1.5, 0),
                          ("min_gain", 1, 0), ("min_run_seconds", 60, 0), ("policy_ratio", 1.0, 0),
                          ("peak_tolerance", 0.9, 0)):
            num(pr.setdefault(k, d), f"prestige.{k}", low)
        if "run_seconds" in pr:
            num(pr["run_seconds"], "prestige.run_seconds", 1)
        pr["_policy_set"] = "policy" in pr
        pr.setdefault("policy", "peak_rate")
        if pr["policy"] not in ("peak_rate", "ratio"):
            fail(f"prestige: unknown policy '{pr['policy']}'. Use 'peak_rate' or 'ratio'")
        pr["requires"] = check_reqs(m, "prestige", pr.setdefault("requires", []), ids)
        rs = pr.setdefault("resets", {})
        if not isinstance(rs, dict):
            fail("prestige: 'resets' must be an object")
        for key, kind in (("currencies", "currencies"), ("purchase_systems", "systems"), ("ladders", "ladders")):
            for i in rs.setdefault(key, []):
                if i not in ids[kind]:
                    fail(f"prestige: resets.{key} names the unknown id '{i}'")
        pr.setdefault("system", "prestige")

    # ---- what survives the reset
    reset_sys = set(m["prestige"].get("resets", {}).get("purchase_systems", [])) if prestige_on(m) else set()
    reset_lad = set(m["prestige"].get("resets", {}).get("ladders", [])) if prestige_on(m) else set()
    for ms in m["milestones"]:
        said = "persists" in ms
        ms.setdefault("persists", ms["system"] not in reset_sys)
        if not said and not ms["persists"] and ms["effects"]:
            warn.append(f"Milestone '{ms['id']}' has an effect and is in the system '{ms['system']}', which the "
                        "prestige resets. The model does not set 'persists' on it, so the script resets it with its "
                        "system. Set \"persists\": true if the effect is permanent.")
    for l in m["ladders"]:
        if l["id"] in reset_lad and l["effects"] and "effects_persist" not in l:
            warn.append(f"Ladder '{l['id']}' restarts at the prestige and has effects. The model does not set "
                        "'effects_persist', so the script keeps the effects of the best step for ever. "
                        "Set \"effects_persist\": false if they follow the current run.")
        l.setdefault("effects_persist", True)
    for s in m["systems"]:
        if prestige_on(m) and s["id"] in reset_sys and s["persists"]:
            warn.append(f"System '{s['id']}' says persists = true, but prestige.resets.purchase_systems names it.")

    # ---- game level ids
    ax = g.get("progress_axis")
    if ax is not None and ax not in ids["ladders"]:
        warn.append(f"game.progress_axis '{ax}' is not a ladder id. The script did not use it.")
        g["progress_axis"] = None
    pp = g.get("primary_profile")
    if pp is not None and pp not in list(PROFILES) + ["model"]:
        fail(f"game.primary_profile must be one of: {', '.join(list(PROFILES) + ['model'])}")
    for i in m["interactions"]:
        if not isinstance(i, dict) or "from" not in i or "to" not in i:
            fail("each entry in 'interactions' needs 'from' and 'to'")
    if not isinstance(m.get("review") or {}, dict):
        fail("'review' must be an object")
    rv = m.get("review") or {}
    for k in ("findings", "lull_notes", "ideas"):
        if not isinstance(rv.get(k) or [], list):
            fail(f"review.{k} must be a list")
    for x in rv.get("lull_notes") or []:
        if not isinstance(x, dict):
            fail("each entry in review.lull_notes must be an object")
        num(x.get("lull_at_hours"), "review.lull_notes: lull_at_hours")
    if not isinstance(m.get("bands") or {}, dict):
        fail("'bands' must be an object")
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
