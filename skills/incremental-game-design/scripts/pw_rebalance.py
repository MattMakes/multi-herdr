"""The rebalance mode of the design solver: give each element of the content of a game its parameters.

    python3 pw_design.py <selection.json> <targets.json> --content <content.json> [--out ...]

pw_design.py owns the command line and the solve of the reset loop (p, s, R, q, c, the spine). This module
reads the content file (scan_godot.py --content-out, references/reverse-engineering.md), keeps each element,
and writes it into the model:

1. Each multiplicative line of the main currency gets an exponent r. The lines with the role main are uncapped
   and share R after the caps. The other lines (support, late) keep a cap and share the rest of R (BM L1, L7).
2. Each uncapped line paid in the reset currency gets a share of q (BM D8, PO2).
3. An element that the laws do not cover (BM section 7) keeps its values and gets the status simulation only.
4. The unlock point of each element of the main currency and of the reset currency goes into the largest gap
   between 2 unlocks of the active player (proof row PC4). The unlock is a lifetime total of its currency.

references/design-solver.md, section "Rebalance mode", explains the rules and the report.
"""
import bisect
import collections
import copy
import itertools
import json
import math
import re

import pw_players
from pw_sim import Sim

HOUR = 3600.0
KINDS = ("line", "reward", "system", "grant")
ROLES = ("main", "support", "late", "permanent", "generator", "manual")
LINE_FAMILIES = ("add", "pct", "mult", "cap")
MAIN_TIMES = (60.0, 240.0)
MAIN_DEFAULT = 2
COST_SECONDS = 20.0
UNLOCK_LEAD = 10.0
PLACE_ROUNDS = 3
BANK_SINK_VALUE = 1.1
BANK_SINK_GROWTH = 1.15
BANK_CUR = "charge"
GEN_VALUE = 0.5
PERM_CAP = 1
SIDE_CAP = 1
DIFF_LIMIT = 0.20
PO3_MARGIN = 1.05
SOLVER_INPUTS = dict(
    q_lines=0.5,
    bank_levels=1,
    line_first_cost=100.0,
    step_top=None)


FACTOR_TOL = 0.2


class ContentError(Exception):
    """A content file that does not parse or breaks a rule: exit code 2."""


def check(bad, msg):
    if not bad:
        return
    raise ContentError(msg)


def read_content(path):
    try:
        data = _load(path)
    except (OSError, ValueError) as e:
        raise ContentError(f"cannot read the content file {path}: {e}")
    validate(data)
    return data


def _load(path):
    f = open(path, encoding="utf-8")
    with f:
        return json.load(f)


def validate(data):
    check(not isinstance(data, dict), "the content file needs a JSON object")
    cur = data.get("currencies") or {}
    check(not cur.get("main"), "the content file needs currencies.main (the scan did not find the main currency: "
                               "write it by hand, references/design-solver.md section 'The content file')")
    els = data.get("elements")
    check(not isinstance(els, list) or not els, "the content file needs a list of elements")
    ids = [e.get("id") if isinstance(e, dict) else None for e in els]
    check(None in ids or len(set(ids)) != len(ids), "each element needs a unique id")
    validate_elements(els, cur)
    for e in els:
        normalize(e, set(ids))


def validate_elements(els, cur):
    """The reset currency when the content has a reset, and the kind and the role of each element."""
    has_reset = any(e.get("kind") == "system" and e.get("system") == "S04" for e in els)
    check(has_reset and not cur.get("reset"), "the content file has a reset (system S04) and needs currencies.reset")
    for e in els:
        check(e.get("kind") not in KINDS, f"element {e['id']}: kind must be 1 of {', '.join(KINDS)}")
        check(e.get("role") not in ROLES + (None,), f"element {e['id']}: role must be null or 1 of {', '.join(ROLES)}")


UNLOCK_KEYS = {"stage": (), "total": ("amount",), "resets": (), "milestone": (), "level": ("n",)}


def normalize(e, ids):
    """Turn 1 element of the content file into the internal form, in place: effects with stat, target, kind,
    value and active_only; levels; 1 unlock kind; cost.formula. The old keys family, affects, value and cap are
    kept in step with the effects, so that each rule reads 1 form (references/design-solver.md, 'The content
    file')."""
    e["effects"] = [norm_effect(f, e) for f in (e.get("effects") or old_effects(e))]
    lv = e.get("levels", e.get("cap"))
    e["levels"] = e["cap"] = int(lv) if lv is not None else None
    e["unlock"] = norm_unlock(e, ids)
    if e.get("cost"):
        e["cost"] = dict(e["cost"], formula=e["cost"].get("formula") or "exp")
    keep_old_keys(e)


def keep_old_keys(e):
    """The old keys family, affects and value of a line (and affects of a reward) from its first effect that is
    not a cap."""
    fx = e["effects"]
    first = next((f for f in fx if f["kind"] != "cap"), fx[0] if fx else None)
    if first and e["kind"] == "line":
        e["family"], e["affects"], e["value"] = first["kind"], first["stat"], first["value"]
    elif first and e["kind"] == "reward":
        e.setdefault("affects", first["stat"])


def old_effects(e):
    fam = e.get("family")
    if e.get("kind") not in ("line", "reward") or fam not in LINE_FAMILIES or e.get("value") is None:
        return []
    return [dict(stat=e.get("affects"), kind=fam, value=e["value"])]


def norm_effect(f, e):
    stat = f.get("stat", f.get("target"))
    check(stat is None or f.get("kind") not in LINE_FAMILIES or not isinstance(f.get("value"), (int, float)),
          f"element {e.get('id')}: each effect needs stat, kind (1 of {', '.join(LINE_FAMILIES)}) and a number value")
    return dict(stat=stat, target=stat, kind=f["kind"], value=f["value"], active_only=bool(
        f.get("active_only", f.get("active", False))))


def norm_unlock(e, ids):
    u = dict(e.get("unlock") or {})
    if "system" in u:
        u["milestone"] = u.pop("system")
    keys = [k for k in u if k in UNLOCK_KEYS]
    extra = [k for k in u if k not in UNLOCK_KEYS and not any(k in UNLOCK_KEYS[m] for m in keys)]
    check(len(keys) > 1 or extra, f"element {e.get('id')}: unlock {u} must be {{}} or 1 of "
                                  f"{', '.join(UNLOCK_KEYS)} (references/design-solver.md, 'The content file')")
    if not keys:
        return {}
    k = keys[0]
    check(any(m not in u for m in UNLOCK_KEYS[k]), f"element {e.get('id')}: unlock {k} needs "
                                                    f"{', '.join(UNLOCK_KEYS[k])}")
    check(k in ("milestone", "level") and u[k] not in ids, f"element {e.get('id')}: unlock {k} names "
                                                           f"{u[k]!r}, which is not an element id")
    return u


def solver_inputs(given):
    """The stated inputs of the rebalance: the values of targets.solver, else the defaults of SOLVER_INPUTS."""
    given = given or {}
    out = {k: given.get(k, v) for k, v in SOLVER_INPUTS.items()}
    out["bank_levels"] = max(0, int(round(out["bank_levels"])))
    return out


def unlock_rank(e):
    """The order of the unlock points in the game: the stage, else a large number for a total."""
    u = e.get("unlock") or {}
    return u.get("stage", 1000 + u.get("resets", 0) * 100 if u else 0)


class Element:
    """1 element of the content, its role, its status, and the old and new value of each field."""

    def __init__(self, e, role):
        self.e, self.id, self.role = e, e["id"], role
        self.status, self.rows, self.why_not, self.ways = "fitted", [], "", []
        self.model, self.ranges = {}, {}

    def row(self, field, old, new, reason, by):
        self.rows.append(dict(field=field, old=old, new=new, reason=reason, by=by))

    def not_fitted(self, why, ways):
        self.status, self.why_not, self.ways = "not fitted", why, ways


def default_roles(content):
    """The role of each element: the hint of the designer, else the default rule of the solver. The generator is
    the add line with the role generator, else the first add line of the main currency that pays also while the
    player is away (an add line with an active_only effect is the manual action of S05, role manual)."""
    main = content["currencies"]["main"]
    raised = cap_targets(content)
    mults = sorted((e for e in content["elements"] if is_main_mult(e, main) and e["id"] not in raised
                    and not e.get("role") and not raises_y(content, e)), key=unlock_rank)
    first_mains = {e["id"] for e in mults[:MAIN_DEFAULT]}
    gen = generator_of(content)
    roles = {e["id"]: e.get("role") or default_role(e, content, first_mains, gen) for e in content["elements"]}
    return {i: "main" if r == "generator" else r for i, r in roles.items()}


def is_manual(e):
    return e.get("role") == "manual" or any(f.get("active_only") for f in e.get("effects") or [])


def generator_of(content):
    """The id of the generator line: the hint role generator, else the first add line of the main currency with no
    active_only effect, else the first add line."""
    main = content["currencies"]["main"]
    adds = sorted((e for e in content["elements"] if is_line(e, main) and e.get("family") == "add"
                   and (e.get("levels") or 2) > 1 and not raises_y(content, e)), key=unlock_rank)
    hint = [e for e in adds if e.get("role") == "generator"]
    auto = [e for e in adds if not is_manual(e)]
    first = (hint or auto or adds or [None])[0]
    return first["id"] if first else None


def raises_y(content, e):
    """True when the first effect of e raises the second income (BM L10)."""
    y = second_income(content)
    f = next((f for f in e.get("effects") or [] if f["kind"] != "cap"), None)
    return bool(y and f) and (content.get("stats") or {}).get(f["stat"], f["stat"]) == y


def cap_targets(content):
    """The ids of the lines that a cap effect of another element raises."""
    return {f["target"] for e in content["elements"] for f in e.get("effects") or [] if f.get("kind") == "cap"}


def is_line(e, cur):
    return e["kind"] == "line" and e.get("currency") == cur


def is_main_mult(e, main):
    return is_line(e, main) and e.get("family") == "mult"


def default_role(e, content, first_mains, first_add):
    if e["id"] in first_mains or e["id"] == first_add or e["kind"] == "system" and e.get("system") in ("S03", "S04"):
        return "main"
    if e["kind"] == "line" and is_line(e, content["currencies"]["main"]) and e.get("family") == "add" and \
            is_manual(e):
        return "manual"
    return "permanent" if e.get("currency") == content["currencies"].get("reset") and e["kind"] == "line" \
        else "support"


class Plan:
    """The rebalance of 1 content file inside 1 round of the solver."""

    def __init__(self, content):
        self.content = content
        self.cur = content["currencies"]
        roles = default_roles(content)
        self.els = [Element(e, roles[e["id"]]) for e in content["elements"]]
        self.by_id = {x.id: x for x in self.els}
        self.times, self.time_currency, self.reqs = {}, {}, {}
        self.ids = model_ids(content)
        self.req_kind, self.curve, self.N, self.growth = "time", None, None, 0.0
        self.gen_value, self.gen_cost = 0.5, 10.0
        self.bank, self.steps, self.amounts, self.costs, self.system_rows = False, {}, {}, {}, {}
        self.old_N = old_stage_count(self.els)
        self.t_end = math.inf
        self.inputs = solver_inputs(None)
        self.structure, self.accepted, self.cap_options, self.fit_resets = game_structure(content), set(), [], None

    def income_at(self, t):
        """The income of the economy of active_greedy at t; before the first run, e^(growth t)."""
        guess = math.exp(self.growth * t / HOUR)
        return guess if self.curve is None else self.curve.sample(t)["income"].get("coin", 0.0)

    def total_at(self, cur, t):
        found = 1.0 if self.curve is None else self.curve.sample(t)["life"].get(cur, 0.0)
        return max(1.0, found)

    def scaled_stage(self, n):
        """A stage of the game, scaled from the stage count of the game to the stage count N of the design."""
        if not self.N or not self.old_N:
            return int(n)
        return max(1, min(self.N, int(round(n * self.N / self.old_N))))

    def guess_times(self, H):
        """The first unlock times: the coin elements spread over the length, the seed elements after 20 minutes."""
        mv = movable(self)
        self.times.update(main_times(self))
        early = [x for x, c in mv if c == "coin" and r_window(x)]
        self.guess_group(early, "coin", 60.0, R_WINDOW / HOUR)
        for cur, t0 in [("coin", 60.0), ("seed", 1200.0)] + [(self.ids[c], 1800.0) for c in self.cur.get("side") or []]:
            self.guess_group([x for x, c in mv if c == cur and x not in early], cur, t0, H)

    def guess_group(self, group, cur, t0, H):
        n = max(1, len(group))
        self.times.update({x.id: t0 + (H * HOUR - t0) * i / n for i, x in enumerate(group)})
        self.time_currency.update({x.id: cur for x in group})

    def of(self, kind):
        return [x for x in self.els if x.e["kind"] == kind]

    def fitted(self):
        return [x for x in self.els if x.status != "not fitted"]


STRUCTURE_RULES = ("stage_after_reset", "stage_test", "reset_gain_rounding")
STRUCTURE_NAMES = STRUCTURE_RULES + ("reset_count", "cap:<line id>", "all")
STRUCTURE_ROWS = {"stage_after_reset": "PO10, PC1, PC4", "stage_test": "PO10, PC1, PC4",
                  "reset_gain_rounding": "PO5, PC2", "reset_count": "PO5, PO10, PC2, PC4", "cap": "PV, PO2, PO3, PO12, PC1"}


def game_structure(content):
    """The structure of the game, from the values of S03 and S04 of the content file: what a reset does to the stage,
    what the stage tests, the rounding of the reset gain, and the most resets of the game (max_resets). None: the
    content file does not state it."""
    els = {e.get("system"): e for e in content["elements"] if e["kind"] == "system"}
    st, rs = (els.get("S03") or {}).get("values") or {}, (els.get("S04") or {}).get("values") or {}
    return dict(stage_after_reset=None if "keeps_stage" not in rs else bool(rs["keeps_stage"]),
                stage_test=st.get("test"), reset_gain_rounding=rs.get("floor"),
                reset_count=int(rs["max_resets"]) if rs.get("max_resets") else None)


def accepted(plan, name):
    """True when the designer accepts the structure change name (pw_design.py --accept-change)."""
    return bool({name, "all"} & plan.accepted)


def keeps(plan, name):
    """True when the model keeps the rule of the game: the content states it and the designer did not accept a
    change."""
    return (getattr(plan, "structure", None) or {}).get(name) is not None and not accepted(plan, name)


def apply_structure(plan, m):
    """The rules of the game in the model: a reset clears the stage or keeps it, the stage tests the income or the
    run total, and the reset gain is rounded down or not. A rule that the designer accepts to change keeps the rule
    of the solver."""
    pr, req = m.get("prestige") or {}, m["ladders"][0]["requires"]
    if keeps(plan, "stage_after_reset") and pr.get("resets"):
        pr["resets"]["ladders"] = [] if plan.structure["stage_after_reset"] else [m["ladders"][0]["id"]]
    if keeps(plan, "stage_test"):
        req["type"] = "run_total" if plan.structure["stage_test"] == "run_total" else "rate"
    if keeps(plan, "reset_gain_rounding") and pr.get("enabled"):
        pr["floor"] = bool(plan.structure["reset_gain_rounding"])


def model_structure(m):
    pr = m.get("prestige") or {}
    lad = m["ladders"][0]
    return dict(stage_after_reset=lad["id"] not in (pr.get("resets") or {}).get("ladders", []),
                stage_test="run_total" if lad["requires"].get("type") == "run_total" else "rate",
                reset_gain_rounding=bool(pr.get("floor", True)))


RULE_TEXT = {"stage_after_reset": {True: "the stage survives a reset", False: "a reset sets the stage to 0"},
             "stage_test": {"run_total": "the stage tests the run total", "rate": "the stage tests the income"},
             "reset_gain_rounding": {True: "the reset gain is rounded down", False: "the reset gain is not rounded"}}


def structure_changes(plan, m):
    """Each change of the structure of the game in the model: a rule, the count of resets, a cap that the game does
    not have (or a cap that the model removes), and a milestone that moves onto the level of another. Each has
    accepted: true when the designer accepted it."""
    return rule_changes(plan, m) + reset_changes(plan) + cap_changes(plan) + moved_onto(plan, m)


def rule_changes(plan, m):
    if not m.get("ladders"):
        return []
    have = model_structure(m)
    return [change(plan, k, RULE_TEXT[k][plan.structure[k]], RULE_TEXT[k][have[k]]) for k in STRUCTURE_RULES
            if plan.structure.get(k) is not None and have[k] != plan.structure[k]]


def reset_changes(plan):
    top, seen = plan.structure.get("reset_count"), plan.fit_resets
    if top is None or seen is None or seen <= top:
        return []
    return [change(plan, "reset_count", f"at most {top} resets", f"{seen} resets of active_greedy by the hard end")]


def cap_changes(plan):
    return [change(plan, f"cap:{x.id}", cap_word(x.e.get("cap")), cap_word(x.model.get("cap")))
            for x in plan.fitted() if x.e["kind"] == "line" and cap_state_differs(x)]


def change(plan, name, game, model):
    return dict(name=name, game=game, model=model, accepted=accepted(plan, name))


def cap_word(cap):
    return "no cap" if cap is None else f"cap {cap}"


def cap_state_differs(x):
    return x.model.get("kind") == "purchase" and (x.e.get("cap") is None) != (x.model.get("cap") is None)


def moved_onto(plan, m):
    """Each milestone or line that opens at a level of a line, whose game level differs from the game level of
    another element that opens at the same model level."""
    seen = collections.defaultdict(list)
    pairs = [p for x in (m.get("milestones") or []) + (m.get("purchases") or []) for p in level_unlocks(plan, x)]
    for key, item in pairs:
        seen[key].append(item)
    return [moved_change(plan, key, items) for key, items in sorted(seen.items(), key=str)
            if len({n for n, _ in items}) > 1]


def level_unlocks(plan, x):
    """((line id, model level), (game level, id)) of each level requirement of x that is the game unlock of x."""
    game = plan.by_id[x["id"]].e.get("unlock") or {} if x["id"] in plan.by_id else {}
    return [((r["id"], r.get("level")), (game.get("n"), x["id"])) for r in x.get("requires") or []
            if r.get("type") == "level" and game.get("level") == r["id"]]


def moved_change(plan, key, items):
    lid, lv = key
    return change(plan, f"cap:{lid}", ", ".join(f"{i} at level {n}" for n, i in items),
                  f"each at level {lv} of {lid} (a cap of the model moves them onto 1 level)")


def keep_cap_state(plan):
    """The cap state of each line of the game stays: a line with no cap in the game gets no cap, a line with a cap
    keeps a cap, and a line keeps the levels that a level unlock of another element needs. A change that the
    designer accepts (cap:<id>) keeps the cap of the fit. Each kept cap that the fit wanted is an option of the
    report (cap_options)."""
    plan.cap_options = []
    lines = [x for x in plan.fitted() if x.e["kind"] == "line" and x.model.get("kind") == "purchase"]
    for x in [y for y in lines if not accepted(plan, f"cap:{y.id}")]:
        keep_cap(plan, x)


def keep_cap(plan, x):
    want, game = x.model.get("cap"), x.e.get("cap")
    need = level_needs(plan, x.id)
    if (game is None) == (want is None) and (want is None or want >= need):
        return
    x.model["cap"] = game
    x.cap_note = None
    x.row("cap", want, game, "kept: the cap state of the game (structure, ticket 47); the fit wanted "
          f"{cap_word(want)}", "the structure of the game")
    plan.cap_options.append(dict(name=f"cap:{x.id}", change=f"{x.id}: {cap_word(game)} becomes {cap_word(want)}",
                                 rows=STRUCTURE_ROWS["cap"]))


def level_needs(plan, lid):
    """The largest level of line lid that a level unlock of another element of the content needs (0: none)."""
    return max([int((y.e.get("unlock") or {}).get("n") or 0) for y in plan.fitted()
                if (y.e.get("unlock") or {}).get("level") == lid], default=0)


def structure_options(plan, failed):
    """The options of the report: each rule that the model keeps and that the method of the solver changes when the
    designer accepts it, and each cap that the fit wanted. Each names the rows that it can change and that fail."""
    out = [dict(name=k, change=f"{RULE_TEXT[k][plan.structure[k]]} becomes {RULE_TEXT[k][not plan.structure[k]]}"
                if k != "stage_test" else "the stage tests the income", rows=STRUCTURE_ROWS[k])
           for k in STRUCTURE_RULES if keeps(plan, k) and solver_rule(k) != plan.structure[k]]
    if keeps(plan, "reset_count"):
        out.append(dict(name="reset_count", change=f"more than {plan.structure['reset_count']} resets: the run length "
                        "follows the unlock gap target", rows=STRUCTURE_ROWS["reset_count"]))
    out += plan.cap_options
    return [dict(o, failed=[r for r in o["rows"].split(", ") if r in failed]) for o in out]


SOLVER_RULES = dict(stage_after_reset=True, stage_test="rate", reset_gain_rounding=False)


def solver_rule(k):
    """The rule of the method of the solver: 1 stage in each run of a rate test (the stage survives a reset), and a
    reset gain with fractions."""
    return SOLVER_RULES[k]


def structure_lines(plan, m, failed):
    """The section "Structure of the game" of the report: the structure that the model keeps, each change (accepted
    or not), and the options with their cost."""
    st = plan.structure
    kept = [f"- {k}: {RULE_TEXT[k][st[k]]}" for k in STRUCTURE_RULES if st.get(k) is not None and keeps(plan, k)]
    kept += [f"- reset_count: at most {st['reset_count']} resets"] if keeps(plan, "reset_count") else []
    unknown = [k for k in STRUCTURE_RULES + ("reset_count",) if st.get(k) is None]
    ch = structure_changes(plan, m)
    out = ["## Structure of the game", "", "The model keeps each rule of the game, the count of resets of the game and "
           "the cap state of each line (references/design-solver.md, \"Structure first\"). A change is a decision of the "
           "designer: `--accept-change <name>`.", "", "Kept:"] + (kept or ["- none stated"])
    out += [f"- not in the content file: {', '.join(unknown)}"] if unknown else []
    out += ["", "Changes:"] + ([f"- {c['name']}: game: {c['game']}; model: {c['model']} ("
                                f"{'accepted' if c['accepted'] else 'NOT ACCEPTED: CK fails'})" for c in ch] or ["- none"])
    opts = structure_options(plan, failed)
    out += ["", "Options (each one changes the game; the designer decides):"]
    out += [f"- `--accept-change {o['name']}`: {o['change']}. Rows it can change: {o['rows']}; failed now: "
            f"{', '.join(o['failed']) or 'none'}" for o in opts] or ["- none"]
    return out + [""]


def old_stage_count(els):
    stages = next((x for x in els if x.e["kind"] == "system" and x.e.get("system") == "S03"), None)
    return (stages.e.get("cap") if stages else None) or 0


def model_ids(content):
    """{game currency: model currency id}: main -> coin, reset -> seed, a side currency keeps its id."""
    cur = content["currencies"]
    out = {cur["main"]: "coin"}
    if cur.get("reset"):
        out[cur["reset"]] = "seed"
    for c in cur.get("side") or []:
        out[c] = c if c not in ("coin", "seed", BANK_CUR) else "side_" + c
    y = second_income(content)
    if y and y not in out:
        out[y] = y if y not in ("coin", "seed", BANK_CUR) else "spine_" + y
    return out


def second_income(content):
    """The game stat of the second income (BM L10): the spine_stat of the content when it is not a currency of the
    content, else None. The model keeps it as its own currency and does not merge it into the main income."""
    st = content.get("spine_stat")
    if not st:
        return None
    y = (content.get("stats") or {}).get(st, st)
    cur = content["currencies"]
    return None if y in [cur["main"], cur.get("reset")] + list(cur.get("side") or []) else y


def spine_id(plan):
    """The model currency that the stages test: the second income, else None (the main income)."""
    y = second_income(plan.content)
    return plan.ids[y] if y else None


def spine_currencies(plan):
    """The currency of the second income: its base income is the sum of the base income entries of its stat."""
    y = second_income(plan.content)
    if not y:
        return []
    base = sum(b.get("value", 0.0) for b in plan.content.get("base_income") or []
               if (plan.content.get("stats") or {}).get(b.get("stat"), b.get("stat")) == y)
    return [dict(id=plan.ids[y], name=f"{y} (second income: the stages test it, BM L10)", system="s01",
                 base_income=base)]


def main_mults(plan):
    main = plan.cur["main"]
    return [x for x in plan.els if is_main_mult(x.e, main) and x.role == "main" and x.status != "not fitted"]


def check_lines(plan):
    """An element that the laws cannot hold: a line with no geometric cost, or a line of an unknown currency."""
    known = set(plan.ids)
    lines = [x for x in plan.els if x.e["kind"] == "line"]
    for x in lines:
        line_fit_check(x, known)


NO_CURRENCY = ["add the currency to currencies.side", "remove the line from the content file of the solve"]
NO_GEOMETRIC = "a line of more than 1 level needs a cost that grows (an exp ratio above 1, or a linear step above " \
               "0) and a known family (BM L1; BM section 7 limit 4)"
NO_GEOMETRIC_WAYS = ["give the line a cost that grows, or levels 1 for a single purchase, and family add, pct, mult "
                     "or cap",
                     "keep the line out of the solve and check it in a simulation of the built game"]


def line_fit_check(x, known):
    e = x.e
    cost = e.get("cost") or {}
    ratio = cost.get("ratio") or 0.0
    bad_cur = e.get("currency") not in known
    grows = ratio > 0.0 if cost.get("formula") == "linear" else ratio > 1.0
    bad_cost = not (grows or e.get("levels") == 1) or e.get("family") not in LINE_FAMILIES
    if bad_cur:
        x.not_fitted(f"its currency {e.get('currency')!r} is not in currencies of the content file", NO_CURRENCY)
    if bad_cost and not bad_cur:
        x.not_fitted(NO_GEOMETRIC, NO_GEOMETRIC_WAYS)


def fit_all(plan, d):
    """The family, cost ratio, value ratio and cap of each fitted element (step 2 of the rebalance). The support
    lines take their r first; the main lines carry the rest of R; the rewards share the factor budget (BM L1, L6,
    balance-math.md section "Content that carries the growth")."""
    clear_reach_marks(plan)
    for x in plan.fitted():
        x.status, x.rows, x.model, x.factor, x.cap_note, x.ranges = "fitted", [], {}, 1.0, None, {}
    k = fit_context(plan, d)
    for x in sorted(plan.fitted(), key=lambda y: FIT_ORDER.index(fit_stage(y, plan))):
        fit_one(plan, d, k, x)
    keep_cap_state(plan)


def clear_reach_marks(plan):
    """An element that the last round did not reach is fitted again in this round (the mark holds 1 round)."""
    marked = [y for y in plan.els if getattr(y, "reach_mark", False) or getattr(y, "budget_mark", False)]
    for x in marked:
        x.status = "fitted"
        x.reach_mark = x.budget_mark = False
    return marked


def fit_one(plan, d, k, x):
    FITTERS.get(fitter_key(x, plan), fit_keep)(x, d, k)


def fit_context(plan, d):
    floor = d.tv["pv_floor"]
    d.setp("purchase_floor", floor, None, d.tv["pv_source"], "PV (purchase value)")
    lines = [x for x in plan.fitted() if fit_stage(x, plan) in ("support", "main")]
    k = dict(plan=plan, floor=floor, q=d.val("q"), R=d.val("R"), p=d.val("S04.power.p"), **line_bounds(plan, d, floor))
    k["identity"] = identity_fit(plan, d, k, lines)
    plan.cap_moves = cap_moves(plan, k["identity"])
    k["spine"] = spine_fit(plan, d, k, [x for x in plan.fitted() if fitter_key(x, plan) in SPINE_KEYS])
    return k


def line_bounds(plan, d, floor):
    """The step floor, the largest step (step_top, else d max of S02.levelled_factor) and the cost ratio range."""
    dp, bp = d.cat.param("S02.levelled_factor", "d"), d.cat.param("S02.levelled_factor", "b")
    step = max(dp["min"], 1.0 + floor)
    top = plan.inputs.get("step_top") or dp["max"]
    return dict(step=step, b_max=bp["max"], b_min=bp["min"], d_max=max(top, step))


SPINE_KEYS = ("spine.mult", "spine.pct", "spine.mult.main")


SPINE_CEILING = ("dropped: each line of the second income has a game cap, so the second income has a ceiling, and in "
                 "the timing fit no stage came after it before the hard end; the line with the largest r = ln d / ln b "
                 "has no cap (BM L10)")


def spine_fit(plan, d, k, lines):
    """d, b, r and cap of each multiplicative line of the second income (BM L10): the game step (moved into [1 + f,
    d max] when outside), the game cost ratio (moved into the range of S02.levelled_factor when outside) and the game
    cap. When each line has a cap and the timing fit found the ceiling (plan.spine_open: the stages stop before the
    hard end), the line with the largest r has no cap (SPINE_CEILING). R_y is the r sum of the uncapped lines: a
    result, not a target. Reason: R_y = R served only the wall ratio w
    of L8 and L10; the timing fit (pw_timing) sets each wall from the simulated income, so the lines keep their
    game shape (Tide Forge: whetstone kept no cap of its 20 for R_y)."""
    if not lines:
        return {}
    steps, ratios = game_steps(lines, k), game_ratios(lines, d)
    out = open_ceiling(plan, {x.id: line_fit(steps[x.id], ratios[x.id], x.e.get("cap")) for x in lines})
    ry = sum(f["r"] for f in out.values() if f["cap"] is None)
    d.setp("R_y", ry, None, "the r sum of the uncapped lines of the second income with their game shapes (BM L10); "
           "the timing fit sets the walls, so R_y is a result", "none: the walls come from the timing fit")
    return out


def open_ceiling(plan, fits):
    """The fits of the lines of the second income with the cap of the line with the largest r dropped, when each
    line has a cap and the timing fit found the ceiling (plan.spine_open)."""
    plan.spine_capped = all(f["cap"] is not None for f in fits.values())
    if not (plan.spine_capped and getattr(plan, "spine_open", False)):
        return fits
    top = max(fits.items(), key=lambda kv: kv[1]["r"])[0]
    return dict(fits, **{top: dict(fits[top], cap=None, ceiling=True)})


def fit_spine_line(x, d, k):
    """A multiplicative line of the second income (BM L10): the game step, cost ratio and cap of spine_fit."""
    e, f = x.e, k["spine"][x.id]
    x.model.update(kind="purchase", effect="mult", b=f["b"], value=f["d"], cap=f["cap"], r=f["r"])
    if e["family"] != "mult":
        x.row("family", e["family"], "mult (S02.levelled_factor)", "a line of the second income is a factor for each "
              "level (BM L10)", "PV")
    x.row("value ratio", e.get("value"), f["d"], step_reason(e, f), "identity and PV")
    kept = abs(f["b"] - game_ratio(e)) < 1e-9
    x.row("cost ratio", e["cost"]["ratio"], f["b"], "kept: the game cost ratio" if kept else
          "the nearest bound of the proven range of S02.levelled_factor", "ranges of assets/systems.json")
    x.row("cap", e.get("cap"), f["cap"], SPINE_CEILING if f.get("ceiling") else
          "kept: the game cap (the timing fit sets the walls, so R_y is no target)", "identity" if not f.get("ceiling")
          else "PC1 (the hard end inside the length)")


def fit_spine_add(x, d, k):
    """An additive line of the second income: it keeps its game value and cost ratio, and gets the levels that keep
    each level at the floor (PV); the multiplicative lines of the second income carry its growth (BM L10)."""
    e = x.e
    v = e.get("value") or 1.0
    cap = add_levels(k["floor"], v, v)
    if e.get("cap") is not None:
        cap = min(cap, e["cap"])
    reason = ADD_REASON.format(f=k["floor"], n=cap)
    x.model.update(kind="purchase", effect="add", b=e["cost"]["ratio"], value=v, cap=cap)
    x.row("cap", e.get("cap"), cap, reason, "PV")
    x.row("value", v, v, "kept: the value of the second income (BM L10)", "PV")
    cap_reduced(x, cap, reason)


def cap_moves(plan, fits):
    """{raised line: new target} for each raised line of R that keeps no cap in the design: its raises go to the
    first capped line of R, so each raise opens levels (balance-math.md section 6a, item 11). The new target is None
    when no line of R keeps a cap: the game must change the raiser."""
    capped = [i for i, f in fits.items() if f["cap"] is not None]
    lost = sorted(t for t in cap_targets(plan.content) if t in fits and fits[t]["cap"] is None)
    return {t: (capped[0] if capped else None) for t in lost}


def cap_target(plan, t):
    """The line that a cap effect on t raises in the model: t, or the capped line of cap_moves."""
    return (getattr(plan, "cap_moves", {}).get(t) or t)


def cap_move_rows(x, plan):
    """1 row for each cap effect of x whose line keeps no cap in the design."""
    moves = getattr(plan, "cap_moves", {})
    pairs = [(f["target"], moves[f["target"]]) for f in x.e.get("effects") or [] if f.get("kind") == "cap" and
             f["target"] in moves]
    x.rows.extend(dict(field="cap target", old=t, new=new or t, reason=(CAP_MOVE_TEXT if new else CAP_DEAD_TEXT)
                       .format(t=t, new=new), by="PV") for t, new in pairs)


CAP_MOVE_TEXT = "{t} has no cap in the design (the cap room of PO2, BM section 6a item 7); the raise goes to the " \
                "capped line {new}, so it opens levels"
CAP_DEAD_TEXT = "THE GAME MUST CHANGE THIS ELEMENT: {t} has no cap in the design and no line of R keeps a cap (the " \
                "cap room of PO2, BM section 6a item 7), so the raise opens no level; give the element another effect"


FIT_ORDER = ("support", "main", "other", "reward")


def fit_stage(x, plan):
    """support: the lines whose r comes first; main: the main lines that carry the rest of R; other."""
    key = fitter_key(x, plan)
    if key == "main.mult.main":
        return "main"
    if key in ("reward", "main.one", "reset.one", "side.one"):
        return "reward"
    return "support" if key in SUPPORT_KEYS and x is not first_add(plan) else "other"


SUPPORT_KEYS = ("main.mult", "main.pct")


def fitter_key(x, plan):
    e = x.e
    if e["kind"] != "line":
        return e["kind"]
    place, fam = line_place(x, plan), e.get("family")
    if e.get("levels") == 1 and fam != "cap":
        return f"{place}.one"
    key = f"{place}.{fam}"
    return MAIN_ROLE_KEYS.get((fam, x.role), key) if place == "main" else key


MAIN_ROLE_KEYS = {("add", "manual"): "main.manual", ("mult", "main"): "main.mult.main", ("add", "main"): "main.add.gen"}


def line_place(x, plan):
    """main, reset or side: the cost currency of a line. A line of the main currency that feeds a side currency
    is side."""
    cur = x.e.get("currency")
    if cur == plan.cur.get("reset") and cur != plan.cur["main"]:
        return "reset"
    if cur != plan.cur["main"]:
        return "side"
    if feeds_spine(plan, x):
        return "spine"
    return "side" if feeds_side(plan, x) else "main"


def feeds_spine(plan, x):
    """A line of the main currency whose first effect raises the second income (BM L10)."""
    f = next((f for f in x.e.get("effects") or [] if f["kind"] != "cap"), None)
    y = spine_id(plan)
    return bool(f) and y is not None and x.e.get("family") != "cap" and stat_target(plan, f["stat"]) == y




def game_step(e):
    """The step of 1 level in the game: d of a mult line, 1 + v of a pct line, and 2 for an add line (its level 2
    doubles its own pool)."""
    fam, v = e.get("family"), e.get("value") or 0.0
    if fam == "mult":
        return max(v, 1.0 + 1e-9)
    return 1.0 + v if fam == "pct" else 2.0


def identity_steps(lines, lo, hi):
    """{id: d}: the game step of each line on a log scale, mapped from [smallest, largest game step] to [lo, hi].
    A line with a larger step in the game keeps a larger step."""
    g = {x.id: math.log(game_step(x.e)) for x in lines}
    a, b = min(g.values(), default=0.0), max(g.values(), default=0.0)
    pos = {i: (v - a) / (b - a) if b > a else 0.0 for i, v in g.items()}
    return {i: math.exp(math.log(lo) + q * (math.log(hi) - math.log(lo))) for i, q in pos.items()}


def cap_room(k):
    """The largest sum of r of the capped lines. When a cap binds, R falls to R_end and lambda to
    (1 - R) / (1 - R_end) (BM L6, L7); lambda stays inside the tolerance of PO2 (DIFF_LIMIT) and R_end stays
    PO3_MARGIN above 1 - p (PO3)."""
    lam = (1.0 - k["R"]) * (1.0 / (1.0 - DIFF_LIMIT) - 1.0)
    return min(lam, k["R"] - (1.0 - k["p"]) * PO3_MARGIN)


def capped_lines(plan, lines, steps, ratios, k):
    """{id: (r, b)} of the lines that keep a cap. A line that a cap raiser raises comes first (its raise must open
    levels), then each other line with a game cap; the smaller r first. A capped line keeps its game cost ratio when
    its r = ln d / ln b fits in the cap room; else its cost ratio rises to the b that fits, at most b_max; else its
    step falls to b_max^room when that is at least the floor step (steps changes in place), so that the line keeps
    its game cap. The lines keep caps while the sum of r stays inside cap_room."""
    raised = cap_targets(plan.content)
    order = sorted((x for x in lines if x.e.get("cap") or x.id in raised),
                   key=lambda x: cap_rank(x, raised, steps, ratios))
    room, out = cap_room(k), {}
    for x in order:
        add_capped(out, (x.id, steps), ratios[x.id], room, (k["b_max"], k["step"]))
    return out


def cap_rank(x, raised, steps, ratios):
    """The order of the capped lines: a raised line first, then the smaller r, then the unlock order."""
    return x.id not in raised, math.log(steps[x.id]) / math.log(ratios[x.id]), unlock_rank(x.e)


def add_capped(out, line, ratio, room, bounds):
    """Add the line to out when it fits in the room that the lines of out leave, with a lower step if it must."""
    (xid, steps), (b_max, floor) = line, bounds
    left = room - sum(v[0] for v in out.values())
    r, b = fit_capped(steps[xid], ratio, left, b_max)
    low = min(steps[xid], b_max ** max(left, 0.0))
    if r is None and low >= floor - 1e-12:
        steps[xid], r, b = low, math.log(low) / math.log(b_max), b_max
    if r is not None:
        out[xid] = (r, b)


def fit_capped(dv, b, room, b_max):
    """(r, b) of 1 capped line inside the room left: the game b, else the b that fits (at most b_max), else None."""
    r = math.log(dv) / math.log(b)
    if r <= room + 1e-12:
        return r, b
    need = math.exp(math.log(dv) / room) if room > 1e-12 else math.inf
    return (math.log(dv) / math.log(need), need) if need <= b_max else (None, None)


def take_room(rs, room):
    """The (id, r) pairs in order while the sum of r stays inside room; a pair that does not fit is left out."""
    sums = itertools.accumulate((r for _, r in rs), lambda acc, r: acc + r if acc + r <= room + 1e-12 else acc,
                                initial=0.0)
    totals = list(sums)
    return [(i, r) for (i, r), a, b in zip(rs, totals, totals[1:]) if b > a]


def identity_fit(plan, d, k, lines):
    """d, b, r and cap of each line of R. Each line keeps its game step (moved into [1 + f, d max] when the game step
    is outside). A capped line keeps its game cap inside the cap room. The uncapped lines keep the order and the log
    proportions of their game cost ratios: 1 exponent scales ln b so that the r sum is R - R_capped (BM L1: r = ln d /
    ln b). The cost ratio is the free parameter that the law R needs, so the step and the cap stay those of the game."""
    steps, ratios = game_steps(lines, k), game_ratios(lines, d)
    capped = capped_lines(plan, lines, steps, ratios, k)
    free = [x.id for x in lines if x.id not in capped]
    b = scaled_ratios(steps, ratios, free, k["R"] - sum(v[0] for v in capped.values()),
                      (k["b_min"], k["b_max"], k["step"]))
    identity_params(d, k, (sum(v[0] for v in capped.values()), b))
    out = {i: line_fit(steps[i], b[i], None) for i in free}
    out.update({i: line_fit(steps[i], capped[i][1], line_cap(plan.by_id[i], k)) for i in capped})
    return out


def game_steps(lines, k):
    """{id: d}: the game step of each line, moved into [step floor, d max] when it is outside (x.ranges)."""
    return clamp_game(lines, "effect", game_step, (k["step"], k["d_max"]))


def clamp_game(lines, field, read, rng):
    """{id: the game value of field (read) inside rng}; each moved line records the range in x.ranges."""
    out = {x.id: clamp(read(x.e), *rng) for x in lines}
    for x in [x for x in lines if out[x.id] != read(x.e)]:
        x.ranges[field] = rng
    return out


def game_ratios(lines, d):
    """{id: b}: the game cost ratio of each line inside the range of S02.levelled_factor (x.ranges when moved). A
    linear cost counts as the ratio of its first 2 levels."""
    bp = d.cat.param("S02.levelled_factor", "b")
    return clamp_game(lines, "cost ratio", game_ratio, (bp["min"], bp["max"]))


def game_ratio(e):
    cost = e.get("cost") or {}
    if cost.get("formula") == "linear":
        return 1.0 + (cost.get("ratio") or 0.0) / max(cost.get("base") or 1.0, 1e-9)
    return cost.get("ratio") or 1.0


def scaled_ratios(steps, ratios, free, R_free, rng):
    """{id: b} of the uncapped lines: b = b_game^k with 1 exponent k, so that the sum of ln d / ln b is R_free. A
    ratio that leaves the range stays at the bound, and k is solved again for the others. When each ratio is at a
    bound and the r sum is still above R_free, the steps shrink by 1 exponent, not below the floor step rng[2] (in
    place)."""
    fixed, b, out = settle(steps, ratios, (free, {}), R_free, rng[:2])
    if out == {}:
        return dict(fixed, **b)
    b = {i: fixed.get(i, rng[1]) for i in free}
    shrink_steps(steps, b, (free, R_free), rng[2])
    return b


def settle(steps, ratios, lines, R_free, rng):
    """(fixed, b, out): scale_once again with each ratio that left rng fixed at its bound, until no ratio leaves
    rng (out empty) or no line or R is left (out None). Each step fixes 1 line or more, so it ends."""
    b, out = scale_once(steps, ratios, lines, R_free, rng)
    if not out:
        return lines[1], b, out
    return settle(steps, ratios, (lines[0], dict(lines[1], **out)), R_free, rng)


def scale_once(steps, ratios, lines, R_free, rng):
    """(b of the lines not yet fixed, the ratios of b that leave rng) of 1 exponent k; (None, None) when no line or
    no R is left."""
    rest, need = rest_need(steps, lines, R_free)
    if not rest or need <= 1e-12:
        return None, None
    k = r_sum(steps, {i: ratios[i] for i in rest}) / need
    b = {i: math.exp(k * math.log(ratios[i])) for i in rest}
    return b, {i: clamp(v, *rng) for i, v in b.items() if clamp(v, *rng) != v}


def rest_need(steps, lines, R_free):
    """(the lines not yet fixed, the R that they must take)."""
    free, fixed = lines
    return [i for i in free if i not in fixed], R_free - r_sum(steps, fixed)


def r_sum(steps, ratios):
    """The sum of r = ln d / ln b of the lines of ratios."""
    return sum(math.log(steps[i]) / math.log(ratios[i]) for i in ratios)


def shrink_steps(steps, b, lines, floor):
    """Shrink the steps of the free lines by 1 exponent so that their r sum is R_free. A step that falls below the
    floor step stays at the floor, and the exponent is solved again for the others (in place)."""
    free, R_free = lines
    if r_sum(steps, {i: b[i] for i in free}) <= R_free + 1e-12 or R_free <= 0:
        return
    low = shrink_low(steps, b, (free, R_free), floor, set())
    steps.update({i: floor for i in low})
    rest = [i for i in free if i not in low]
    j = shrink_exponent(steps, b, (rest, low), R_free, floor) if rest else 1.0
    steps.update({i: steps[i] ** j for i in rest})


def shrink_low(steps, b, lines, floor, low):
    """The set of lines whose step falls below the floor with the exponent of the others."""
    free, R_free = lines
    rest = [i for i in free if i not in low]
    j = shrink_exponent(steps, b, (rest, low), R_free, floor) if rest else 1.0
    under = {i for i in rest if steps[i] ** j < floor}
    return low if not under else shrink_low(steps, b, lines, floor, low | under)


def shrink_exponent(steps, b, parts, R_free, floor):
    """The exponent j of the steps of rest so that their r sum plus the r of the lines at the floor is R_free."""
    rest, low = parts
    need = R_free - sum(math.log(floor) / math.log(b[i]) for i in low)
    return need / r_sum(steps, {i: b[i] for i in rest})


def line_fit(dv, b, cap):
    return dict(d=dv, b=b, r=math.log(dv) / math.log(b), cap=cap)


def identity_params(d, k, vals):
    capped, b = vals
    d.setp("R_capped", capped, [0.0, cap_room(k)], "the sum of r of the capped lines: r = ln d / ln b (BM L1); at most "
           "cap_room: lambda after the caps inside the PO2 tolerance, R after the caps "
           f"{PO3_MARGIN:g} x (1 - p) or more", "PO2 and PO3")
    if b:
        d.setp("line_ratio_scale", min(b.values()), None, "the smallest cost ratio of the uncapped lines: b = b_game^k "
               "with 1 exponent k, so that the r sum is R - R_capped", "PO1 and PO5")


def line_cap(x, k):
    """The cap of a capped line: the game cap, else the levels of 1 / f."""
    return int(x.e.get("cap") or round(1.0 / k["floor"]))


def clamp(v, lo, hi):
    return min(max(v, lo), hi)


def cap_reduced(x, new, reason):
    """Record a cap below the cap of the game (no cap counts as above each cap): the warning row "cap reduced"."""
    old = x.e.get("cap")
    if new is not None and (old is None or new < old):
        x.cap_note = dict(id=x.id, game_cap=old, cap=new, reason=reason)


def add_levels(floor, base, v):
    """L1 with D4: level k of an additive line adds v / (base + v (k - 1)) to its pool, so the floor f allows
    1 + 1/f - base/v levels."""
    return max(1, int(math.floor(1.0 + 1.0 / floor - base / v + 1e-9)))


ADD_REASON = "level k of an additive line adds v / (B + v (k - 1)) to its pool; the floor {f:.0%} allows " \
             "1 + 1/f - B/v = {n} levels (BM L1, D4)"


GEN_SHARE_REASON = "; the {n} additive lines of the main income add to 1 pool, so they share these levels: {cap} " \
                   "each (PV tests each level of each line)"


def generators(plan):
    """The additive lines of the main income that fit_gen fits (roles main and manual): they add their game values
    to 1 pool, so they share the levels of the floor (pool_caps)."""
    return [y for y in plan.fitted() if fitter_key(y, plan) in GEN_KEYS]


GEN_KEYS = ("main.add.gen", "main.manual", "main.add")


def fit_gen(x, d, k):
    """An additive line of the main income (S01): the game value, the game first cost (base_cost) and the game cost
    ratio inside the nearest S01 range. The levels keep each level at the floor (PV): the lines add to 1 pool in
    their content order, so the cost ratio spreads the levels in time as in the game (pool_caps)."""
    vid, b = nearest_s01(d, x.e["cost"]["ratio"])
    k["plan"].pool = (main_base(k["plan"]) or 1.0 / d.val("S05.active_lane.a"), k["floor"])
    caps = pool_caps(k["plan"], *k["plan"].pool)
    cap = caps[x.id]
    x.model.update(kind="purchase", effect="add", b=b, value=gen_game_value(x), cap=cap)
    reason = ADD_REASON.format(f=k["floor"], n=cap) + (GEN_SHARE_REASON.format(n=len(caps), cap=cap)
                                                       if len(caps) > 1 else "")
    gen_rows(x, (vid, b), cap, reason)
    x.ranges["cost ratio"] = (d.cat.param(vid, "b")["min"], d.cat.param(vid, "b")["max"])
    cap_reduced(x, cap, reason)


def gen_rows(x, shape, cap, reason):
    """The rows of fit_gen: the family (when it was not add), the cost ratio and the cap."""
    e, (vid, b) = x.e, shape
    if e["family"] != "add":
        x.row("family", e["family"], f"add ({vid})", "an additive line has r = 0 (BM L1)", "PO1")
    kept = abs(b - e["cost"]["ratio"]) < 1e-9
    x.row("cost ratio", e["cost"]["ratio"], b, "kept: the game cost ratio" if kept
          else f"the nearest proven range of {vid}", "ranges of assets/systems.json")
    x.row("cap", e.get("cap"), cap, "kept: the game cap is inside the floor levels" if cap == e.get("cap")
          else reason, "PV and PO2")


def main_base(plan):
    """The base income of the game on the main income (the sum of base_income of each stat that maps to the main
    currency), or None when the content has none. The active player plays the whole time, so an active_only base
    counts in full."""
    stats = plan.content.get("stats") or {}
    main = plan.cur["main"]
    v = sum(b.get("value", 0.0) for b in plan.content.get("base_income") or [] if stats.get(b["stat"], b["stat"]) == main)
    return v if v > 0 else None


def gen_game_value(x):
    """The game value of the first add effect of x (GEN_VALUE when the content has none)."""
    f = next((f for f in x.e.get("effects") or [] if f["kind"] == "add"), None)
    return f["value"] if f and f.get("value") else GEN_VALUE


def pool_caps(plan, base, floor):
    """{id: levels} of the additive lines of the main income in the pool order (pool_order). A level of line i adds
    v_i / (the pool before it), so line i stops when the pool reaches v_i / f (L1 with D4): its levels are
    1 + (v_i / f - P_i) / v_i, at least 1, where P_i is the base B plus the earlier lines at their levels."""
    pool, out = base, {}
    for y in pool_order(plan):
        out[y.id] = pool_levels(y, pool, floor)
        pool += gen_game_value(y) * out[y.id]
    return out


def pool_order(plan):
    """The additive lines of the main income from the smallest game value to the largest (then the content order)."""
    lines = generators(plan)
    ranked = sorted((gen_game_value(y), i) for i, y in enumerate(lines))
    return [lines[i] for _, i in ranked]


def pool_gate(plan, x):
    """The requirement that keeps the order of the pool: a line opens when the line before it in the pool order is
    at its levels, when that smaller line would fall below the floor with the full pool. Reason: each reset sets the
    levels to 0, and the buyer then bought the levels of the small line after the large line; 1 level of hammer
    (add 0.5) with apprentice (add 6) at 10 levels gave 0.8% (PV). The unlock point is a timing number, not the
    character of the line."""
    order = pool_order(plan)
    if x not in order or order.index(x) == 0:
        return None
    y = order[order.index(x) - 1]
    v = gen_game_value(y)
    if y.model.get("cap") is None:
        return None
    if v / max(full_pool(plan, order) - v, 1e-12) >= plan.pool[1] - 1e-12:
        return None
    return dict(type="level", id=y.id, level=int(y.model.get("cap") or 1))


def full_pool(plan, order):
    """The pool with each additive line at its levels: the base B plus each v x levels."""
    return plan.pool[0] + sum(gen_game_value(z) * (z.model.get("cap") or 1) for z in order)


def pool_levels(y, pool, floor):
    """The levels of 1 additive line on the pool P: 1 + (v / f - P) / v, at least 1; a smaller game cap stays."""
    v = gen_game_value(y)
    n = max(1, int(math.floor(1.0 + (v / floor - pool) / v + 1e-9)))
    game = y.e.get("cap")
    return game if game is not None and game <= n else n


def nearest_s01(d, b):
    vids = ("S01.geometric", "S01.steep")
    cands = [(abs(v - b), vid, v) for vid, v in ((vid, clamp_param(d, vid, b)) for vid in vids)]
    best = min(cands)
    return best[1], best[2]


def clamp_param(d, vid, b):
    p = d.cat.param(vid, "b")
    return clamp(b, p["min"], p["max"])


def fit_line(x, d, k):
    """A line of R of the main currency (S02.levelled_factor): the step, cost ratio and cap of identity_fit."""
    e, f = x.e, k["identity"][x.id]
    x.model.update(kind="purchase", effect="mult", b=f["b"], value=f["d"], cap=f["cap"], r=f["r"])
    why = "the game family" if e["family"] == "mult" else \
        f"a {e['family']} line adds v / (its pool) for each level, so its later levels fall below the floor (BM L1)"
    x.row("family", e["family"], "mult (S02.levelled_factor)", why, "PV")
    x.row("value ratio", e.get("value"), f["d"], step_reason(e, f), "identity and PV")
    x.row("cost ratio", e["cost"]["ratio"], f["b"], cost_reason(f), "PO1 and PO3")
    x.row("cap", e.get("cap"), f["cap"], cap_reason(x, f, k), "identity and PO3")
    x.row("stat", e.get("affects"), e.get("affects"), "kept: the game stat; the model target is the currency of "
          "the stat (content stats)", "identity")
    cap_reduced(x, f["cap"], cap_reason(x, f, k))


def step_reason(e, f):
    g = game_step(e)
    if abs(f["d"] - g) < 1e-9:
        return f"kept: the game step {g:.4g}"
    return f"game step {g:.4g}: moved into the range [1 + f, d max] of S02.levelled_factor, lowered so that the game " \
           "cap fits the cap room at b max (PO3), or shrunk with the other uncapped steps when each cost ratio is at " \
           "b max (R)"


def cost_reason(f):
    if f["cap"] is not None:
        return f"a capped line: the game cost ratio, or the b that fits the cap room; r = ln d / ln b = {f['r']:.4f} " \
               "(BM L1)"
    return f"r = {f['r']:.4f}: the game cost ratio scaled by 1 exponent for each uncapped line, so that the r sum is " \
           "R - R_capped (BM L1)"


CAP_REASONS = {
    (True, True): "kept: a cap raiser raises this line, so the raise opens levels (identity)",
    (True, False): "kept: the game cap, inside the cap room of PO2 and PO3 (cap_room)",
    (False, True): "dropped: a bound cap takes r out of R (BM L7), so lambda falls to (1 - R) / (1 - R_end); the "
                   "capped lines before it use the cap room (PO2 tolerance 20%, cap_room)",
    (False, False): "uncapped, as in the game"}


def cap_reason(x, f, k):
    if f["cap"] is not None:
        return CAP_REASONS[(True, x.id in cap_targets(k["plan"].content))]
    return CAP_REASONS[(False, bool(x.e.get("cap")))]


def fit_perm_mult(x, d, k):
    """A line paid in the reset currency (S15.geometric): the game step and the game cost ratio, moved into their
    ranges when outside. A line with a game cap keeps it (a constant factor). An uncapped line takes q in the unlock
    order while it fits in q_lines of q (PO2: lambda = p q / (1 - R)); a line that does not fit gets the step that
    fits (at least the floor step), else a cap of PERM_CAP levels."""
    e, (dv, b) = x.e, perm_shape(x, d, k)
    used = k.setdefault("q_used", 0.0)
    room = k["plan"].inputs["q_lines"] * k["q"] - used
    dv, cap = perm_fit(e, dv, b, room, k)
    r = math.log(dv) / math.log(b) if cap is None else 0.0
    x.model.update(kind="purchase", effect="mult", b=b, value=dv, cap=cap, r=r, persist=True)
    k["q_used"] = used + r
    perm_form_rows(x)
    x.factor = 1.0 if cap is None else dv ** cap
    perm_rows(x, (b, dv, r, cap), k, perm_reason(r, cap is None, k, used, e.get("cap")))


def perm_shape(x, d, k):
    """(d, b): the game step inside [floor step, d max] and the game cost ratio inside S15.geometric (x.ranges)."""
    bp = d.cat.param("S15.geometric", "b")
    g, gb = game_step(x.e), game_ratio(x.e)
    dv, b = clamp(g, k["step"], k["d_max"]), clamp(gb, bp["min"], bp["max"])
    if dv != g:
        x.ranges["effect"] = (k["step"], k["d_max"])
    if b != gb:
        x.ranges["cost ratio"] = (bp["min"], bp["max"])
    return dv, b


def perm_fit(e, dv, b, room, k):
    """(d, cap): the game cap; else uncapped when r fits the room; else the step b^room when it is at least the floor
    step; else a cap of PERM_CAP levels."""
    if e.get("cap"):
        return dv, int(e["cap"])
    if math.log(dv) / math.log(b) <= room + 1e-9:
        return dv, None
    fit = b ** room if room > 0 else 0.0
    return (fit, None) if fit >= k["step"] - 1e-12 else (dv, PERM_CAP)


def perm_form_rows(x):
    """The rows of a line of the reset currency whose cost is linear or whose family is not mult."""
    linear = ("cost formula", "linear", "exp", "BM D8 needs a geometric cost: r = ln d / ln b of S15.geometric",
              "PO2")
    family = ("family", x.e.get("family"), "mult (S15.geometric)", "a line of the reset currency carries a share of "
              "q as a factor for each level (BM D8)", "PO2")
    rows = [linear] * (x.e["cost"].get("formula") == "linear") + [family] * (x.e.get("family") != "mult")
    for r in rows:
        x.row(*r)


def perm_reason(r, free, k, used, game_cap=None):
    if free:
        return f"uncapped: r = {r:.4f} is a share of q (BM D8)"
    if game_cap:
        return "kept: the game cap; a capped line adds a constant factor (BM L7)"
    return (f"q {k['q']:.4f}: the lines before it use {used:.4f} of {k['plan'].inputs['q_lines']:g} q (targets.solver."
            "q_lines), so this line gets a cap and adds a constant factor (BM L7, PO2)")


def perm_rows(x, vals, k, reason):
    e, (b, dv, r, cap) = x.e, vals
    x.row("cost ratio", e["cost"]["ratio"], b, "the game cost ratio inside the range of S15.geometric",
          "ranges of assets/systems.json")
    x.row("value ratio", e.get("value"), dv, f"the game step inside [1 + f, d max], or the step that fits q; r = "
          f"{r:.4f}", "PO2 and PV")
    x.row("cap", e.get("cap"), cap, reason, "PO2")
    cap_reduced(x, cap, reason)


def fit_perm_cap(x, d, k):
    """A cap raiser paid in the reset currency (S13): it keeps the value of the game; its cost ratio enters the S15
    range."""
    e = x.e
    bp = d.cat.param("S15.geometric", "b")
    b = clamp(e["cost"]["ratio"], bp["min"], bp["max"])
    if b != e["cost"]["ratio"]:
        x.ranges["cost ratio"] = (bp["min"], bp["max"])
    x.model.update(kind="purchase", effect="cap", b=b, value=e.get("value") or 1, cap=e.get("cap"), persist=True)
    cap_move_rows(x, k["plan"])
    x.row("cost ratio", e["cost"]["ratio"], b, "inside the range of S15.geometric", "ranges of assets/systems.json")
    x.row("value", e.get("value"), x.model["value"], "kept: each raise opens levels of a line at the floor step",
          "PV")


def fit_main_cap(x, d, k):
    x.model.update(kind="purchase", effect="cap", b=x.e["cost"]["ratio"], value=x.e.get("value") or 1,
                   cap=x.e.get("cap") or 1)
    cap_move_rows(x, k["plan"])
    x.row("value", x.e.get("value"), x.model["value"], "kept: each raise opens levels of a line at the floor step",
          "PV")


def fit_side_mult(x, d, k):
    """A mult line paid in a side currency: it keeps its cost, its step and its cap. Its factor is persistent, so the
    measured persistent factor F of the run length takes it (BM L6, pw_proof F_p); an uncapped line of a side currency
    is also a second clock (BM L7). The timing is BM section 7 limit 2: the simulation decides."""
    e = x.e
    dv = max(e.get("value") or k["step"], k["step"])
    if dv != e.get("value"):
        x.ranges["effect"] = (k["step"], math.inf)
    x.status = "simulation only"
    x.model.update(kind="purchase", effect="mult", b=e["cost"]["ratio"], value=dv, cap=e.get("cap"),
                   base=e["cost"]["base"], formula=e["cost"].get("formula", "exp"))
    x.factor = dv ** (e.get("cap") or 1)
    x.row("value", e.get("value"), dv, "kept, at least the floor step: the smallest step that the player feels (PV)",
          "PV")
    x.row("cost and cap", "", "kept", "a line paid in a side currency: BM section 7 limit 2", "simulation only")


def fit_side_add(x, d, k):
    """An additive line paid in a side currency: it keeps its cost and value, and gets the levels that keep each
    level at the floor (PV). Its pool starts with the first sources of its side system."""
    e = x.e
    sysel = side_system_of(k["plan"], fed_currency(k["plan"], x) or e.get("currency"))
    base = side_rate(sysel.e) if sysel else 1.0
    cap = add_levels(k["floor"], base, side_add_value(k["plan"], x))
    if e.get("cap") is not None:
        cap = min(cap, e["cap"])
    reason = ADD_REASON.format(f=k["floor"], n=cap)
    x.status = "simulation only"
    x.model.update(kind="purchase", effect="add", b=e["cost"]["ratio"], value=e.get("value"), cap=cap,
                   base=e["cost"]["base"], formula=e["cost"].get("formula", "exp"))
    x.row("cap", e.get("cap"), cap, reason, "PV")
    x.row("cost and value", "", "kept", "a line paid in a side currency: BM section 7 limit 2", "simulation only")
    cap_reduced(x, cap, reason)


def fed_currency(plan, x):
    """The game currency that the first effect of x feeds: a side currency, else None."""
    f = next((f for f in x.e.get("effects") or [] if f["kind"] != "cap"), None)
    if f is None:
        return None
    cur = (plan.content.get("stats") or {}).get(f["stat"], f["stat"])
    return cur if cur in (plan.cur.get("side") or []) else None


def feeds_side(plan, x):
    """A line of the main currency whose first effect feeds a side currency: it is not a line of R (it does not
    raise the main income), so the rules of a side currency apply (BM section 7 limit 2)."""
    return x.e["kind"] == "line" and x.e.get("family") != "cap" and fed_currency(plan, x) is not None


def side_system_of(plan, cur):
    return next((s for s in plan.of("system") if s.e.get("currency") == cur), None)


def fit_side(x, d, k):
    """A line paid in a side currency (BM section 7 limit 2): it keeps its values; the simulation decides."""
    e = x.e
    x.status = "simulation only"
    x.model.update(kind="purchase", effect=e.get("family"), b=e["cost"]["ratio"], value=e.get("value"),
                   cap=e.get("cap"), base=e["cost"]["base"], formula=e["cost"].get("formula", "exp"))
    x.row("all", "", "kept", "a line paid in a side currency: BM section 7 limit 2", "simulation only")


def game_effects(plan, e, k):
    """The effects of a constant factor (a reward or a purchase of 1 level) at their game values. A factor on the main
    income between 1 and the floor step 1 + f rises to the floor step, so that the purchase passes PV; a penalty
    (below 1) and each other effect keep their values. The run length takes the persistent factors by the measured
    factor F (BM L6, pw_proof F_p), so no factor shrinks to fit a budget."""
    return [floor_effect(plan, f, k["floor"]) for f in e.get("effects") or []]


def floor_effect(plan, f, floor):
    if f["kind"] not in ("mult", "pct") or stat_target(plan, f["stat"]) != "coin" or factor_of(f) <= 1.0:
        return dict(f)
    v = max(factor_of(f), 1.0 + floor)
    return dict(f, value=v if f["kind"] == "mult" else v - 1.0)


def is_budget_factor(plan, f):
    """A mult effect above 1 or a pct effect above 0 on the main income: a persistent constant factor (F of L6)."""
    big = f["value"] > 1.0 if f["kind"] == "mult" else f["value"] > 0.0 if f["kind"] == "pct" else False
    return big and stat_target(plan, f["stat"]) == "coin"


def factor_of(f):
    return f["value"] if f["kind"] == "mult" else 1.0 + f["value"]


def main_factor(plan, effs):
    """The product of the factors on the main income (penalties count: they shrink the product)."""
    return math.prod(factor_of(f) for f in effs if f["kind"] in ("mult", "pct") and stat_target(plan, f["stat"]) ==
                     "coin")


def in_run(plan, x):
    """A constant factor that the player gets again in each run: a reward at a level of a line that each reset sets
    to 0, or a single purchase of the main currency. It does not change from run to run, so the trajectory of each
    run (A) holds it (BM L6)."""
    u = x.e.get("unlock") or {}
    t = plan.by_id.get(u.get("level")) if u.get("level") else None
    line_resets = t is not None and t.e.get("currency") == plan.cur["main"]
    single_main = x.e["kind"] == "line" and x.e.get("currency") == plan.cur["main"]
    return (x.e["kind"] == "reward" and line_resets) or single_main


def fit_reward(x, d, k):
    """A reward: a milestone with its game effects (game_effects). A stage unlock is scaled to N."""
    plan = k["plan"]
    run = in_run(plan, x)
    effs = game_effects(plan, x.e, k)
    x.model.update(kind="milestone", effects=effs, in_run=run)
    x.factor = 1.0 if run else main_factor(plan, effs)
    side = (x.e.get("unlock") or {}).get("total") in (plan.cur.get("side") or [])
    if side:
        x.status = "simulation only"
    note_floor(x, effs)
    x.row("effects", fx_text(x.e), fx_text(dict(effects=effs)), IN_RUN if run else KEPT_FACTOR, "PO5 (F) and PV")
    cap_move_rows(x, plan)


def note_floor(x, effs):
    """x.ranges: an effect that rose to the floor step is moved into the range of PV."""
    rose = any(abs(a["value"] - b["value"]) > 1e-12 for a, b in zip(x.e.get("effects") or [], effs))
    x.ranges.update({"effect": (1.1, math.inf)} if rose else {})


IN_RUN = "kept: the player gets this factor again in each run (a level of a line that each reset sets to 0), so the " \
         "trajectory of each run holds it (BM L6)"
KEPT_FACTOR = "kept: the game values (a factor below the floor step rises to it, PV); a persistent factor shortens the " \
              "later runs, and c takes the measured factor F of the run length (BM L6)"


def fit_single(x, d, k):
    """A purchase of 1 level: a constant factor, as a reward that the player buys, with its game effects
    (game_effects). A line of the main currency costs COST_SECONDS of the income at its unlock (its place in time),
    a line of another currency keeps its game cost."""
    plan, e = k["plan"], x.e
    run = in_run(plan, x)
    effs = [single_factor(plan, f) for f in game_effects(plan, e, k)]
    x.model.update(kind="purchase", effect="single", effects=effs, b=1.0, cap=1, value=None)
    x.factor = 1.0 if run else main_factor(plan, effs)
    note_floor(x, effs)
    why = (IN_RUN if run else KEPT_FACTOR) + (SINGLE_PCT if effs != game_effects(plan, e, k) else "")
    x.row("effects", fx_text(e), fx_text(dict(effects=effs)), why, "PO5 (F) and PV")
    x.row("levels", e.get("levels"), 1, "a single purchase: the cost ratio has no effect", "identity")


SINGLE_PCT = "; a pct effect becomes mult 1 + v: the pct effects add in 1 pool, so purchase k of the pool adds only " \
             "v / (1 + v (k - 1)) and the later purchases fall below the floor (PV, BM L1 with D4)"


def single_factor(plan, f):
    """A pct effect of a single purchase on the main income as the mult 1 + v (the same step, but not in a pool)."""
    if f["kind"] == "pct" and stat_target(plan, f["stat"]) == "coin" and f["value"] > 0:
        return dict(f, kind="mult", value=1.0 + f["value"])
    return f


def fit_manual(x, d, k):
    """The manual action of S05 (role manual): an add line that pays only while the player plays. It keeps the
    family add and its active_only flag, takes the value of the generator, the nearest S01 cost ratio, and the levels
    that keep each level at the floor (PV)."""
    fit_gen(x, d, k)
    x.model["active"] = True
    x.row("active only", True, True, "kept: the manual action of S05 pays only while the player plays", "identity")


def on_main(plan, x):
    """True when the first effect of x (not a cap) acts on the main income."""
    f = next((f for f in x.e.get("effects") or [] if f["kind"] != "cap"), None)
    return f is None or stat_target(plan, f["stat"]) == "coin"


def fx_text(e):
    return ", ".join(f"{f['kind']} {f['value']:g} on {f['target']}" for f in e.get("effects") or [])


SYSTEM_KIND = {"S03": "ladder", "S04": "prestige"}


def fit_system(x, d, k):
    kind = SYSTEM_KIND.get(x.e.get("system"), "side_system")
    x.model.update(kind=kind)
    if kind == "side_system":
        fit_side_system(x)


def fit_side_system(x):
    x.status = "simulation only"
    x.row("values", fmt_values(x.e.get("values")), "kept", "a side system with its own currency: BM section 7 "
          "limit 2", "simulation only")


def fmt_values(vals):
    return ", ".join(f"{k} {v:g}" for k, v in sorted((vals or {}).items()))


def fit_grant(x, d, k):
    x.status = "simulation only"
    x.model.update(kind="grant")
    v = x.e.get("value") or {}
    when = "reset" if v.get("trigger") == "reset" else "stage"
    x.row("grant", f"base {v.get('base')}, growth {v.get('growth')}, at each {when}", "kept", f"a grant at each {when} "
          "(its game trigger): the laws do not cover it", "simulation only")


def fit_keep(x, d, k):
    x.status = "simulation only"
    x.model.update(kind="purchase", effect=x.e.get("family"), b=(x.e.get("cost") or {}).get("ratio") or 1.0,
                   value=x.e.get("value"), cap=x.e.get("cap") or 1)
    x.row("all", "", "kept", "no rule of the solver for this element", "simulation only")


FITTERS = {"main.add": fit_gen, "main.pct": fit_line, "main.mult.main": fit_line, "main.mult": fit_line,
           "main.add.gen": fit_gen, "main.cap": fit_main_cap, "reset.mult": fit_perm_mult, "reset.cap": fit_perm_cap,
           "reset.pct": fit_perm_mult, "main.manual": fit_manual, "main.one": fit_single, "reset.one": fit_single,
           "side.one": fit_single,
           "reset.add": fit_keep, "side.add": fit_side_add, "side.mult": fit_side_mult, "side.pct": fit_side,
           "side.cap": fit_side, "spine.mult": fit_spine_line, "spine.pct": fit_spine_line,
           "spine.mult.main": fit_spine_line, "spine.add": fit_spine_add, "spine.one": fit_single,
           "reward": fit_reward, "system": fit_system, "grant": fit_grant}


def stat_target(plan, stat):
    """The model currency of a game stat or currency, or the id of a purchase."""
    cur = plan.content.get("stats", {}).get(stat, stat)
    return plan.ids.get(cur, cur if cur in plan.by_id else "coin")


def first_add(plan):
    return next((x for x in plan.fitted() if x.model.get("effect") == "add" and x.e.get("currency") ==
                 plan.cur["main"] and x.role == "main"), None)


def purchases(plan, d, econ_line):
    """The purchase lines of the fitted elements, in content order, then the bank line."""
    lines = [x for x in plan.fitted() if x.model.get("kind") == "purchase"]
    return [purchase(plan, d, x, econ_line) for x in lines]


def purchase(plan, d, x, econ_line):
    e, mo = x.e, x.model
    cur = plan.ids[e["currency"]]
    p = dict(id=x.id, name=e.get("name") or x.id, system=system_of(plan, x), cost_currency=cur,
             base_cost=base_cost(plan, d, x), cost_growth=mo["b"], effects=effects_of(plan, x))
    if mo.get("formula", "exp") != "exp":
        p["cost_formula"] = mo["formula"]
    if mo.get("cap") is not None:
        p["max_level"] = mo["cap"]
    reqs = requirements(plan, x)
    if reqs:
        p["requires"] = reqs
    return p


def requirements(plan, x):
    """The unlock requirement of the line and the order gate of its pool (pool_gate), when they apply."""
    gate = pool_gate(plan, x) if x.e["kind"] == "line" else None
    return [r for r in (requirement(plan, x), gate) if r]


def system_of(plan, x):
    cur = x.e.get("currency")
    if cur == plan.cur["main"]:
        return "s01" if x.model.get("effect") == "add" else "s02"
    return "s15" if cur == plan.cur.get("reset") else side_system_id(plan, cur)


def side_system_id(plan, cur):
    return "s23_" + plan.ids.get(cur, cur)


def effects_of(plan, x):
    e, mo = x.e, x.model
    if mo.get("effect") == "cap":
        return cap_effects(plan, x)
    if mo.get("effect") == "single":
        return [model_effect(plan, f) for f in mo["effects"]]
    target = stat_target(plan, e.get("affects"))
    value = add_value(plan, x) if mo["effect"] == "add" else mo["value"]
    eff = dict(target=target, kind=mo["effect"], value=value, **effect_flags(mo))
    return [eff] + extra_effects(plan, x, eff)


def cap_effects(plan, x):
    return [dict(target=cap_target(plan, f["target"]), kind="cap", value=x.model["value"]) for f in
            x.e.get("effects") or []]


def effect_flags(mo):
    """The group and the active flag of the first effect of a line, when the model of the line has them."""
    return {key: val for key, val in (("group", mo.get("group")), ("active", bool(mo.get("active")) or None)) if val}


def model_effect(plan, f):
    g = dict(target=cap_target(plan, f["stat"]) if f["kind"] == "cap" else stat_target(plan, f["stat"]), kind=f["kind"],
             value=f["value"])
    if f.get("active_only"):
        g["active"] = True
    return g


def mult_step(f):
    """The factor of 1 level of an effect: v of a mult, 1 + v of a pct, 2 of an add (its level 2 doubles its pool)."""
    return {"mult": f["value"], "pct": 1.0 + f["value"]}.get(f["kind"], 2.0)


def extra_effects(plan, x, eff):
    """The effects of a line after its first effect (a line with 2 stats). An effect on the same model target keeps
    its size relative to the first effect: an add takes the same ratio of values, a factor g2 becomes d^(ln g2 /
    ln g1), so a smaller effect stays smaller. An effect on another target keeps its game value."""
    fx = [f for f in x.e.get("effects") or [] if f["kind"] != "cap"]
    return [extra_effect(plan, fx[0], f, eff) for f in fx[1:]]


def extra_effect(plan, first, f, eff):
    g = model_effect(plan, f)
    return dict(g, **second_value(first, f, eff)) if g["target"] == eff["target"] else g


def second_value(first, f, eff):
    if eff["kind"] == "add" and f["kind"] == "add" and first["kind"] == "add" and first["value"]:
        return dict(kind="add", value=eff["value"] * f["value"] / first["value"])
    g1, g2 = mult_step(first), mult_step(f)
    if eff["kind"] == "mult" and g1 > 1.0 and g2 > 1.0:
        return dict(kind="mult", value=eff["value"] ** (math.log(g2) / math.log(g1)))
    return {}


def add_value(plan, x):
    """An additive line of the main currency: its value relative to the first generator (GEN_VALUE 0.5 of the
    solver). An additive line of a side currency: the value for each drip of the side system, for each second."""
    if x.e.get("currency") != plan.cur["main"] or feeds_side(plan, x):
        return side_add_value(plan, x)
    if feeds_spine(plan, x) or x in generators(plan):
        return x.model["value"]
    return plan.gen_value


def side_add_value(plan, x):
    """The value of a level of an add line of a side currency for each second: the game value, divided by the
    period of the side system when the side system gives its income in drips (old key drip_seconds)."""
    sysel = side_system_of(plan, fed_currency(plan, x) or x.e.get("currency"))
    vals = (sysel.e.get("values") or {}) if sysel else {}
    period = 1.0 if vals.get("rate_per_second") is not None else vals.get("drip_seconds") or 1.0
    return (x.e.get("value") or 1.0) / period


def base_cost(plan, d, x):
    """An additive line of the main income (fit_gen) keeps its game first cost. A line of the main currency costs
    COST_SECONDS of the income at its unlock. A line of another currency keeps its game cost."""
    e = x.e
    if x in generators(plan):
        return e["cost"]["base"]
    if e.get("currency") == plan.cur["main"]:
        return main_line_cost(plan, x)
    if x.model.get("effect") == "single" and e.get("currency") in (plan.cur.get("side") or []):
        return float("%.3g" % (e["cost"]["base"] * side_cost_scale(plan, e.get("currency"))))
    if e.get("currency") == plan.cur.get("reset"):
        return float("%.3g" % (e["cost"]["base"] * getattr(plan, "seed_scale", 1.0)))
    return e["cost"]["base"]


def main_line_cost(plan, x):
    """A line of R costs line_first_cost; a placed line costs COST_SECONDS of the income at its unlock."""
    if x.model.get("r"):
        return plan.inputs["line_first_cost"]
    if x.id in plan.costs:
        return plan.costs[x.id]
    if x.id in plan.times:
        return float("%.3g" % max(plan.gen_cost, plan.income_at(plan.times[x.id]) * COST_SECONDS))
    return x.e["cost"]["base"]


SIDE_SPEND = 0.8


def side_cost_scale(plan, cur):
    """1 factor for the costs of the single purchases of a side currency: their sum is at most SIDE_SPEND of the
    lifetime total of that currency that the active player has at the hard end, so each one is in reach. The factor
    is at most 1 (never above the game cost), and the order of the costs stays."""
    if plan.curve is None:
        return 1.0
    total = sum(x.e["cost"]["base"] for x in plan.fitted() if x.model.get("effect") == "single"
                and x.e.get("currency") == cur)
    have = plan.total_at(plan.ids[cur], plan.t_end) if total > 0 else 0.0
    return min(1.0, SIDE_SPEND * have / total) if total > 0 else 1.0


def requirement(plan, x):
    req = unlock_req(plan, x)
    plan.reqs[x.id] = req
    return req


def unlock_req(plan, x):
    """The unlock of an element: a lifetime total of its currency at the placed time, a stage of the spine
    scaled to N, a side system, or none. A line that carries q opens with the first reset."""
    q_line, timed = is_q_line(x, plan), x.id in plan.times and plan.req_kind == "time"
    total = plan.amounts.get(x.id)
    if x is first_add(plan):
        return None
    if q_line:
        return dict(type="prestiges", amount=1)
    if timed:
        return dict(type="time", amount=round(plan.times[x.id], 1))
    return dict(type="total", currency=total[0], amount=total[1]) if total else kept_requirement(plan, x)


def kept_requirement(plan, x):
    """The unlock of the game in the model: a stage scaled to N, a lifetime total, a count of resets, the level of
    a line (inside the cap of that line in the model), or another element."""
    u = x.e.get("unlock") or {}
    if u.get("stage"):
        return dict(type="ladder", id="stage", step=plan.scaled_stage(u["stage"]))
    if u.get("total"):
        return dict(type="total", currency=plan.ids.get(u["total"], u["total"]), amount=u["amount"])
    if u.get("resets"):
        return dict(type="prestiges", amount=(getattr(plan, "reset_map", None) or {}).get(u["resets"], u["resets"]))
    if u.get("level"):
        return level_requirement(plan, u["level"], u["n"])
    return element_requirement(plan, u.get("milestone"))


def level_requirement(plan, lid, n):
    """Line lid at level n. A cap of the model below n would block the unlock (F17): the level comes down to that
    cap."""
    t = plan.by_id.get(lid)
    if t is None or t.status == "not fitted":
        return None
    cap = t.model.get("cap")
    return dict(type="level", id=lid, level=int(min(n, cap)) if cap else int(n))


def element_requirement(plan, eid):
    """It opens with another element: a milestone of the model, or level 1 of a line."""
    t = plan.by_id.get(eid) if eid else None
    if t is None or t.status == "not fitted":
        return None
    if t.model.get("kind") == "purchase":
        return dict(type="level", id=eid, level=1)
    return dict(type="milestone", id=eid) if t.model.get("kind") in ("milestone", "side_system") else None


def milestones(plan):
    makers = {"milestone": reward_milestone, "side_system": side_milestone}
    items = [x for x in plan.fitted() if x.model.get("kind") in makers]
    return [makers[x.model["kind"]](plan, x) for x in items]


def reward_milestone(plan, x):
    effs = [dict(target=cap_target(plan, f["target"]) if f["kind"] == "cap" else stat_target(plan, f["target"]),
                 kind=f["kind"], value=f["value"]) for f in x.model.get("effects") or []]
    req = requirement(plan, x)
    x.new_name = moved_name(x, req)
    m = dict(id=x.id, name=x.new_name, system="s03", kind="reward", persists=not x.model.get("in_run"), effects=effs)
    m["requires"] = [req] if req else []
    return m


def moved_name(x, req):
    """The name of a reward whose stage moved: the old stage number in the name becomes the new stage (I6)."""
    name = x.e.get("name") or x.id
    old = (x.e.get("unlock") or {}).get("stage")
    if req and req.get("type") == "ladder" and old is not None and req["step"] != old:
        return re.sub(rf"\b{int(old)}\b", str(req["step"]), name)
    return name


def side_milestone(plan, x):
    """The side system opens: its currency gets the income of its first sources (BM section 7 limit 2)."""
    rate = side_rate(x.e)
    cur = plan.ids.get(x.e.get("currency"), x.e.get("currency"))
    m = dict(id=x.id, name=x.e.get("name") or x.id, system=side_system_id(plan, x.e.get("currency")),
             kind="system_unlock", persists=True, effects=[dict(target=cur, kind="add", value=rate)])
    req = requirement(plan, x)
    m["requires"] = [req] if req else []
    return m


def grants(plan, trigger="stage"):
    """The grants of the fitted content with the trigger: stage (the ladder pays them at each stage) or reset (the
    reset pays them at each reset, step = the count of resets). A grant keeps its trigger: a grant of each move is
    not a grant of each stage."""
    items = [(g, g.e.get("value") or {}) for g in plan.fitted() if g.model.get("kind") == "grant"]
    return [grant_entry(plan, g, v) for g, v in items if (v.get("trigger") or "stage") == trigger]


def grant_entry(plan, g, v):
    """A grant of the reset currency scales by plan.seed_scale, as the first costs of its lines (base_cost): the
    timing fit moves c, and a gain is k (E / c)^p."""
    game_cur = v.get("currency") or g.e.get("currency") or plan.cur["main"]
    cur = plan.ids.get(game_cur, "coin")
    base = v.get("base", 1.0)
    if game_cur == plan.cur.get("reset"):
        base = float("%.3g" % (base * getattr(plan, "seed_scale", 1.0)))
    return grant_count(dict(currency=cur, base=base, growth=v.get("growth", 1.0)), v)


def grant_count(g, v):
    """The grant with the optional count of the game: a reset grant pays only at resets 1 to count."""
    if v.get("count") is not None:
        g["count"] = int(v["count"])
    return g


def set_system_rows(plan, m):
    """The rows of the stage system and the reset system: their values come from the solve, not from fit_all."""
    makers = {"S03": stage_rows, "S04": reset_rows}
    items = [x for x in plan.of("system") if x.e.get("system") in makers]
    plan.system_rows.update({x.id: makers[x.e["system"]](x, m) for x in items})


def stage_rows(x, m):
    lad = m["ladders"][0]
    rule = (f"rate on the full income, growth {lad['requires']['growth']:.4g} after a table of "
            f"{len(lad['requires'].get('table') or [])}")
    return [dict(field="count", old=x.e.get("cap"), new=lad["count"], reason="the stage of active_greedy at H",
                 by="PC1"), dict(field="rule", old=x.e.get("family"), new=rule, reason="BM L8", by="PO10")]


def reset_rows(x, m):
    pr = m["prestige"]
    loop = f"p {pr['exponent']:g}, c {pr['base']:.4g}, reward q {pr['reward']['exponent']:.4g}"
    return [dict(field="unlock", old=unlock_text(x.e.get("unlock") or {}), new=f"gain >= {pr['min_gain']:g}, run >= "
                 f"{pr.get('min_run_seconds', 0):g} s", reason="the rule of the first reset: the gain factor^(-s) "
                 "and a run of half of T_X (first_gain)", by="PO5"),
            dict(field="p, c, q", old=x.e.get("value"), new=loop, reason="BM L6", by="PO2 to PO5")]


def side_currencies(plan):
    return [dict(id=plan.ids[c], name=f"{c} (side currency)", system=side_system_id(plan, c))
            for c in plan.cur.get("side") or []]


def side_system_rows(plan):
    return [dict(id=side_system_id(plan, c), loop="minigame", persists=True) for c in plan.cur.get("side") or []]


def bank_levels(d):
    return d.plan.inputs["bank_levels"] if d.plan is not None else SOLVER_INPUTS["bank_levels"]


def bank_sink(d):
    """Item d: the S08 bank gets a repeated sink: 1 charge for the first level, cost growth 1.15, x1.1 each (the
    floor step), targets.solver.bank_levels levels (a constant factor, BM L7)."""
    if "S08" not in d.chosen or bank_levels(d) < 1:
        return []
    n = bank_levels(d)
    d.setp("bank_line", dict(cost=1, growth=BANK_SINK_GROWTH, d=BANK_SINK_VALUE, cap=n), None,
           "a repeated sink of the bank: the floor step for each level, a constant factor (BM L7)", "PV and PO5")
    return [dict(id="bank", name="Bank line (repeated sink: 1 charge each level)", system="s08",
                 cost_currency=BANK_CUR, base_cost=1.0, cost_growth=BANK_SINK_GROWTH, max_level=n,
                 effects=[dict(target="coin", kind="mult", value=BANK_SINK_VALUE)])]


def caps_reduced(plan):
    """The model key caps_reduced: each line whose cap fell below the cap of the game, with the reason."""
    return [x.cap_note for x in plan.fitted() if getattr(x, "cap_note", None)]


def factor_product(plan, d):
    """The product of the constant factors of the fitted content at its full caps, with the bank sink."""
    out = 1.0
    for x in plan.fitted():
        out *= getattr(x, "factor", 1.0)
    return out * (BANK_SINK_VALUE ** bank_levels(d) if plan.bank else 1.0)


def ladder_factor(plan):
    """The product of the mult rewards that a stage unlocks. The trajectory with no reset (A) never reaches a
    stage, so the solver multiplies A by this factor^alpha (BM L2)."""
    staged = [x for x in plan.fitted() if x.model.get("kind") == "milestone" and "stage" in (x.e.get("unlock") or {})]
    return math.prod(getattr(x, "factor", 1.0) for x in staged)


class LifeSim(Sim):
    """A Sim that also keeps the lifetime total of each currency in each sample."""

    def sample(self, inc, cands):
        super().sample(inc, cands)
        self.samples[-1]["life"] = dict(self.life)


def run_active(m, hours):
    """active_greedy of the players with the lifetime totals in its samples."""
    pl = pw_players.Players(m, hours)
    spec = next(p for p in pw_players.PLAYERS if p["name"] == "active_greedy")
    prof = pw_players.reset_profile(m, spec["reset"])
    return LifeSim(m, pl.horizon(spec), prof, player=pl.player(spec)).play()


class Curve:
    """The lifetime total and the income of the economy of active_greedy against time."""

    def __init__(self, sim):
        self.t = [s["t"] for s in sim.samples]
        self.samples = sim.samples
        self.first_reset = next((e["real"] for e in sim.events if e["kind"] == "prestige"), None)

    def first_total(self, cur):
        """The first time when the lifetime total of cur is above 0, or None."""
        return next((s["t"] for s in self.samples if s["life"].get(cur, 0.0) > 0), None)

    def sample(self, t):
        i = max(0, min(len(self.t) - 1, bisect.bisect_right(self.t, t) - 1))
        return self.samples[i]


def is_q_line(x, plan):
    """An uncapped mult line of the reset currency: it carries q, so it opens with the first reset (BM D8)."""
    return is_line(x.e, plan.cur.get("reset")) and x.model.get("effect") == "mult"


R_WINDOW = HOUR


def movable(plan):
    """(element, currency of its unlock total) of each element that the placement moves: the lines and systems of
    the main currency (but the main lines and the first generator), the cap raisers of the reset currency, and each
    element that opens at a lifetime total of a side currency."""
    ctx = (first_add(plan), plan.cur["main"], plan.cur.get("reset"))
    items = sorted(plan.fitted(), key=lambda x: unlock_rank(x.e))
    return [(x, move_currency(plan, x)) for x in items if is_movable(plan, x, ctx)]


def is_movable(plan, x, ctx):
    gen, main, reset = ctx
    kept = x is gen or x.role in ("main", "manual") or is_q_line(x, plan)
    side_sys = x.model.get("kind") == "side_system"
    sim_only = x.status == "simulation only" and not side_sys and not side_total(plan, x)
    line = x.e["kind"] == "line" and x.e.get("currency") in (main, reset)
    return not kept and not sim_only and (line or side_sys or bool(side_total(plan, x)))


def side_total(plan, x):
    """The side currency of the lifetime total that opens x, or None."""
    cur = (x.e.get("unlock") or {}).get("total")
    return cur if cur in (plan.cur.get("side") or []) else None


def move_currency(plan, x):
    side = side_total(plan, x)
    if side:
        return plan.ids[side]
    return "seed" if x.e.get("currency") == plan.cur.get("reset") and x.e["kind"] == "line" else "coin"


def r_window(x):
    """The latest unlock time of x: R_WINDOW for a line that carries R, else no limit."""
    return R_WINDOW if x.model.get("r") and x.e.get("currency") else None


def fixed_times(sim, ax, moved, t_end):
    """The unlock times of active_greedy that the placement does not move: axis steps, and the first purchase of
    each line or milestone that is not moved. The content repair after the fit (content_slots) counts content only:
    an axis step is not content (fun row FM-1)."""
    steps = [v[0] for v in sim.step_clock[ax].values()] if ax else []
    firsts = [e["real"] for e in sim.events if is_unlock(e) and e["id"] not in moved]
    return sorted(t for t in steps + firsts if 0 < t < t_end)


def is_unlock(e):
    return (e["kind"] == "purchase" and e["note"] == "first") or (e["kind"] == "milestone" and e["note"] != "repeat")


def limit_at(t, tv):
    """The content gap band at the time t, in seconds: the FM-1 band of the fun targets (tv keys fun_gap_h1 and
    fun_gap), else the unlock gap targets of PC4."""
    if t < HOUR:
        return tv.get("fun_gap_h1", tv["ugap_first"]) * 60.0
    return tv.get("fun_gap", tv["ugap_max"]) * 60.0


def fill_slots(marks, n, window, tv):
    """n slots, each at the middle of the largest gap (gap / limit) inside the window (t_min, t_max). Returns the
    sorted slots."""
    marks, slots = sorted(set(marks)), []
    for _ in range(n):
        slots += next_slot(marks + slots, window, tv)
    return sorted(slots)


def window_gaps(points, window):
    """The gaps between the points, cut to the window (t_min, t_max)."""
    t_min, t_max = window
    hi = math.inf if t_max is None else t_max
    pts = sorted(set(points + ([] if t_max is None else [t_max])))
    return [(max(a, t_min), min(b, hi)) for a, b in zip(pts, pts[1:]) if b > t_min and a < hi]


def next_slot(points, window, tv):
    gaps = window_gaps(points, window)
    if not gaps:
        return []
    a, b = max(gaps, key=lambda g: ((g[1] - g[0]) / limit_at(g[0], tv), -g[0]))
    return [(a + b) / 2.0]


def freeze(plan, gen_cost):
    """The unlock totals and the costs of the placed elements, from the curve of the run that placed them."""
    plan.amounts = {i: (plan.time_currency.get(i, "coin"), float("%.4g" % plan.total_at(plan.time_currency.get(
        i, "coin"), t))) for i, t in plan.times.items()}
    plan.costs = {i: float("%.3g" % max(gen_cost, plan.income_at(t) * COST_SECONDS)) for i, t in plan.times.items()}


def freeze_some(plan, gen_cost, ids):
    """freeze for the elements of ids only: the other elements keep their totals and costs."""
    moved = sorted(ids)
    for i in moved:
        freeze_one(plan, gen_cost, i)


def freeze_one(plan, gen_cost, i):
    t, cur = plan.times[i], plan.time_currency.get(i, "coin")
    plan.amounts[i] = (cur, float("%.4g" % plan.total_at(cur, t)))
    plan.costs[i] = float("%.3g" % max(gen_cost, plan.income_at(t) * COST_SECONDS))


REPAIR_STEPS = 200


def content_slots(plan, sim, tv, t_end):
    """{id: new time} of the content repair. The content of the run is each first purchase, each first milestone and
    the first reset (an axis step is not content). While a gap is above the FM-1 band (limit_at), the movable element
    at the start of the gap moves later, or the one at its end moves earlier, to the middle of its 2 neighbours,
    inside its window. A move must lower the larger of the 2 gaps below the gap it splits. The elements keep their
    order. A gap that no move lowers stays: the report names the missing content (proof row CG)."""
    content = content_list(sim, t_end)
    wins = {i: element_window(plan, sim, i, tv, t_end) for i, _ in content if i in plan.times}
    times = [t for _, t in content]
    repair_moves(times, content, wins, (t_end, tv))
    return {i: times[k] for k, (i, t) in enumerate(content) if i in wins and abs(times[k] - t) > 1.0}


def repair_moves(times, content, wins, ctx):
    """The moves of content_slots on times, at most REPAIR_STEPS. A gap that no move lowers goes into skip."""
    skip, steps = set(), 0
    gap = worst_gap(times, ctx[0], ctx[1], skip)
    while gap is not None and steps < REPAIR_STEPS:
        skip |= set() if move_into(gap, times, content, wins, ctx) else {gap}
        gap = worst_gap(times, ctx[0], ctx[1], skip)
        steps += 1
    return steps


def content_list(sim, t_end):
    """[(id, time)] of the content of the run up to t_end, in time order: the first time of each id."""
    first = {}
    for cid, t in ((content_id(e), e["real"]) for e in sim.events if is_content(e, t_end)):
        first.setdefault(cid, t)
    return list(first.items())


def content_id(e):
    return "first reset" if e["kind"] == "prestige" else e["id"]


def is_content(e, t_end):
    return (is_unlock(e) or e["kind"] == "prestige") and 0 < e["real"] < t_end


def element_window(plan, sim, i, tv, t_end):
    """(earliest, latest) time of a movable element: a side system after its demo floor, an element of the reset
    currency after the first reset, an element of a side currency after that currency starts, a line of R inside
    R_WINDOW, each at most 1 unlock gap target before the end."""
    x, cur = plan.by_id[i], plan.time_currency.get(i, "coin")
    lo = demo_floor(plan, tv) if place_kind(x) == "side" else 0.0
    if cur != "coin":
        lo = max(lo, Curve(sim).first_total(cur) or 0.0)
    hi = min(r_window(x) or math.inf, t_end - tv["ugap_max"] * 60.0)
    return lo, hi


def worst_gap(times, t_end, tv, skip):
    """The index k of the gap from point k to point k + 1 of [0] + times + [t_end] with the largest gap / band
    above 1 and not in skip, or None."""
    pts = [0.0] + times + [t_end]
    ratios = [((pts[k + 1] - pts[k]) / limit_at(pts[k], tv), k) for k in range(len(pts) - 1) if k not in skip]
    best = max(ratios, default=(0.0, None))
    return best[1] if best[0] > 1.0 + 1e-9 else None


def move_into(k, times, content, wins, ctx):
    """Move the element at the start of gap k later, else the element at its end earlier. True when 1 moved."""
    t_end, tv = ctx
    pts = [0.0] + times + [t_end]
    sides = [j for j in (k - 1, k) if 0 <= j < len(times)]
    moves = ((j, try_move(j, pts, wins.get(content[j][0]), tv)) for j in sides)
    j, new = next(((j, new) for j, new in moves if new is not None), (None, None))
    if j is not None:
        times[j] = new
    return j is not None


def try_move(j, pts, win, tv):
    """The new time of the element j (point j + 1) at the middle of its 2 neighbours, inside its window, when it
    lowers the larger of its 2 gaps; else None (also for an element that is not movable)."""
    if win is None:
        return None
    p, t, q = pts[j], pts[j + 1], pts[j + 2]
    new = min(max((p + q) / 2.0, win[0]), win[1])
    if not p < new < q:
        return None
    before = max((t - p) / limit_at(p, tv), (q - t) / limit_at(t, tv))
    after = max((new - p) / limit_at(p, tv), (q - new) / limit_at(new, tv))
    return new if after < before - 1e-9 else None


def reach_fix(plan, sim, t_end):
    """After the last round: a placed element that the run of the last round does not reach gets the lifetime total
    of that run at its placed time, so that the active player reaches it (proof row PC5). Its effect is not in that
    run, so the run up to its time does not change."""
    got = {e["id"] for e in sim.events if is_unlock(e) and e["real"] <= t_end}
    missed = {i: v for i, v in plan.amounts.items() if i not in got}
    plan.amounts.update({i: (cur, reach_amount(plan, (cur, amount), min(plan.times.get(i, t_end), t_end)))
                         for i, (cur, amount) in missed.items()})


def reach_amount(plan, placed, t):
    cur, amount = placed
    return float("%.4g" % min(amount, plan.total_at(cur, t)))


def main_times(plan):
    """The main lines unlock early, so that R after the caps holds for each run (BM L6)."""
    mains = sorted(main_mults(plan), key=lambda x: unlock_rank(x.e))
    plan.time_currency.update({x.id: "coin" for x in mains})
    return {x.id: MAIN_TIMES[min(i, len(MAIN_TIMES) - 1)] * (1 + i // len(MAIN_TIMES)) for i, x in enumerate(mains)}


def place_once(plan, sim, ax, tv, t_end):
    """1 round of the placement: the slots of the lines of R inside R_WINDOW, the other coin elements, the seed
    elements after the first reset, and the elements of each side currency after the side currency starts. The
    latest slot is 1 unlock gap target before the hard end, so each element stays reachable (proof row PC5)."""
    mv = movable(plan)
    marks = [0.0] + fixed_times(sim, ax, {x.id for x, _ in mv}, t_end) + [t_end]
    t_last = t_end - tv["ugap_max"] * 60.0
    raised = raise_times(plan, sim, mv, t_last)
    times = place_rest(plan, [(x, c) for x, c in mv if x.id not in raised], (marks, tv, t_last))
    times.update(raised)
    plan.time_currency.update({x.id: cur for x, cur in mv if x.id in raised})
    return times


def place_rest(plan, rest, ctx):
    """The times of the main lines and of each group of the placement (the cap raisers are placed apart)."""
    marks, tv, t_last = ctx
    times = main_times(plan)
    for g in place_groups(plan, tv):
        place_group(plan, members(rest, g), (g[0], cut_window(g[1], t_last)), (marks, times, tv))
    return times


def members(rest, group):
    cur, _, kind = group
    return [x for x, c in rest if (c, place_kind(x)) == (cur, kind)]


def cut_window(window, t_last):
    return window[0], t_last if window[1] is None else window[1]


def place_kind(x):
    """r: a line of R; side: a side system; other."""
    if r_window(x):
        return "r"
    return "side" if x.model.get("kind") == "side_system" else "other"


def place_groups(plan, tv):
    """(currency, window, kind) of each group of the placement, in order. A line of R opens inside R_WINDOW. A side
    system opens inside R_WINDOW after its earliest time, so its own elements have time before the hard end; that
    time is the demo end when the demo end comes before it (targets demo_end.before_system), else 0."""
    floor = demo_floor(plan, tv)
    sides = [(plan.ids[c], (plan.curve.first_total(plan.ids[c]), None), "other") for c in plan.cur.get("side") or []]
    return [("coin", (0.0, R_WINDOW), "r"), ("coin", (floor, floor + R_WINDOW), "side"), ("coin", (0.0, None), "other"),
            ("seed", (plan.curve.first_reset, None), "other")] + sides


def demo_floor(plan, tv):
    """The earliest unlock time of the coin elements of no time limit: the demo end when a side system comes after
    it, else 0."""
    after = tv.get("demo_before") and any(x.e.get("system") == tv["demo_before"] for x in plan.of("system"))
    return tv["demo_min"] * 60.0 if after and tv.get("demo_min") else 0.0


def level_times(sim):
    """{line id: {level: the first real time of that level}} from the purchases of a run."""
    out = {}
    for e in (e for e in sim.events if e["kind"] == "purchase"):
        out.setdefault(e["id"], {}).setdefault(int(e["name"].rsplit("L", 1)[-1]), e["real"])
    return out


def raise_times(plan, sim, mv, t_last):
    """{raiser id: time}: a cap raiser opens when the line that it raises reaches its cap in the run of the round,
    so the cap binds only for a short time and each raise opens levels that the player buys (identity, PV). A
    raiser of a cap that the line never reaches keeps its gap slot."""
    reached, out = level_times(sim), {}
    starts = {c: w[0] for c, w, _ in place_groups(plan, {}) if w[0] is not None}
    for target, raisers in raisers_by_target(plan, mv).items():
        out.update(target_raises(plan.by_id[target].model.get("cap"), raisers, reached.get(target, {}),
                                 (starts, t_last)))
    return out


def target_raises(cap, raisers, levels, limits):
    """{raiser id: time} of the raisers of 1 line, in order, while the line reaches its cap before t_last."""
    starts, t_last = limits
    caps = itertools.accumulate((raise_amount(x) for x, _ in raisers[:-1]), initial=cap)
    times = [(x, cur, levels.get(c) if c is not None else None) for (x, cur), c in zip(raisers, caps)]
    kept = itertools.takewhile(lambda v: v[2] is not None and v[2] <= t_last, times)
    return {x.id: max(t - UNLOCK_LEAD, starts.get(cur, 0.0)) for x, cur, t in kept}


def raisers_by_target(plan, mv):
    pairs = [(f["target"], (x, cur)) for x, cur in mv for f in x.e.get("effects") or []
             if f.get("kind") == "cap" and f.get("target") in plan.by_id]
    out = {}
    for t, xc in pairs:
        out.setdefault(t, []).append(xc)
    return out


def raise_amount(x):
    """The levels that a raiser adds at its full level: the value times the cap of a purchase, the value of a
    reward."""
    v = (x.model.get("value") if x.model.get("effect") == "cap" else None) or x.e.get("value") or 0
    return int(v * (x.model.get("cap") or 1)) if x.e["kind"] == "line" else int(v)


def place_group(plan, group, where, ctx):
    (cur, window), (marks, times, tv) = where, ctx
    t_min = window[0]
    if not group or t_min is None:
        return
    slots = fill_slots(marks + list(times.values()), len(group), window, tv)
    times.update({x.id: max(t_min, t - UNLOCK_LEAD) for x, t in zip(group, slots)})
    plan.time_currency.update({x.id: cur for x in group})


def diff_rows(plan, m):
    """x.diff_rows: the fields of the model that differ from the game and that no row of the fit names: the first
    cost and the effects (a field-by-field diff of the content and the model)."""
    model = {q["id"]: q for key in ("purchases", "milestones") for q in m.get(key) or []}
    for x in plan.fitted():
        x.diff_rows = element_diff(x, model.get(x.id))


def element_diff(x, mo):
    if mo is None:
        return []
    named = {r["field"] for r in x.rows}
    return [r for r in (cost_diff(x, mo, named), effect_diff(x, mo, named)) if r]


def cost_diff(x, mo, named):
    old = (x.e.get("cost") or {}).get("base")
    new = mo.get("base_cost")
    if new is None or old is None or "first cost" in named or abs(new - old) <= 1e-9 * max(abs(old), 1.0):
        return None
    return dict(field="first cost", old=old, new=new, reason="the cost scale of the model: a line costs COST_SECONDS "
                "of the income at its unlock, a line of R costs line_first_cost, the generator the cost of the solver",
                by="PC2 and PC4 (timing)")


def effect_diff(x, mo, named):
    old = fx_text(x.e)
    new = ", ".join(f"{f['kind']} {f['value']:g} on {f['target']}" for f in mo.get("effects") or [])
    if named & {"effects", "all", "cost and value"} or old == new:
        return None
    return dict(field="effects (model)", old=old, new=new, reason="each effect of the model; the rows above give the "
                "reason of each change, and an add value is in the units of the model income", by="model")


def base_income_line(plan, m):
    """The base income of the game against the base income of the model."""
    game = ", ".join(f"{b['stat']} {b['value']:g}" + (" (active only)" if b.get("active_only") else "")
                     for b in plan.content.get("base_income") or []) or "none"
    model = ", ".join(f"{c['id']} {c['base_income']:.4g}" for c in m.get("currencies") or [] if c.get("base_income"))
    why = "kept: the game base income of the main income" if main_base(plan) else "the generator start of the " \
                                                                                 "solver, BM L2"
    return f"Base income: game {game}; model {model or 'none'} ({why})."


def element_rows(plan):
    head = ["| Element | Kind | Role | Status | Field | Old | New | Reason | Set by |",
            "| --- | --- | --- | --- | --- | --- | --- | --- | --- |"]
    out = []
    for x in plan.els:
        out += element_lines(plan, x)
    return head + out


def element_lines(plan, x):
    rows = x.rows + getattr(x, "diff_rows", []) + plan.system_rows.get(x.id, []) + unlock_rows(plan, x)
    if not rows:
        rows = [dict(field="", old="", new="", reason="", by="")]
    cells = (x.id, x.e["kind"], x.role, x.status)
    return [f"| {' | '.join(cells)} | {r['field']} | {cell(r['old'])} | {cell(r['new'])} | {r['reason']} | {r['by']} |"
            for r in rows]


def cell(v):
    if isinstance(v, float):
        return f"{v:.4g}"
    return "none" if v is None else str(v)


def unlock_rows(plan, x):
    if x.e["kind"] == "system" and x.e.get("system") in ("S03", "S04"):
        return []
    if x.status == "not fitted":
        return [dict(field="not fitted", old="", new="", reason=x.why_not, by="; ".join(x.ways))]
    old = unlock_text(x.e.get("unlock") or {})
    req = plan.reqs.get(x.id)
    if req is None:
        return open_rows(old)
    new = req_text(req, plan)
    return [dict(field="unlock", old=old, new=new, reason=unlock_why(plan, x, req, new != old),
                 by=unlock_by(plan, x, req))]


def open_rows(old):
    """The unlock row of an element that opens at the start in the model (none when it opens at the start in the
    game too)."""
    return [dict(field="unlock", old=old, new="open at the start", reason=OPEN_REASON, by="PO5")] if old else []


def unlock_why(plan, x, req, differs):
    """The reason of the unlock row: "kept" only when the text did not change."""
    reason = unlock_reason(plan, x, req)
    if reason != "kept" or not differs:
        return reason
    return level_reason(plan, x, req) if req["type"] == "level" else "the game unlock in the units of the model"


OPEN_REASON = "open at the start: the generator pays from the first second, so the trajectory with no reset (A, BM L2) " \
              "has its income"


def level_reason(plan, x, req):
    t = plan.by_id.get(req["id"])
    cap = t.model.get("cap") if t is not None else None
    return f"the game level, lowered to the cap {cap} of {req['id']} in the model, so the player reaches it (F17)"


def unlock_reason(plan, x, req):
    if x in main_mults(plan):
        return "a main line opens in the first minutes, so that R holds in each run (BM L6)"
    if x.id in plan.amounts:
        return "placed in the largest gap between 2 unlocks of active_greedy, as a lifetime total"
    if is_q_line(x, plan):
        return "a line that carries q opens with the first reset (BM D8)"
    if req["type"] == "ladder":
        return "the game stage scaled to the stage count N"
    if x.e["kind"] == "line" and x.role == "main" and x.e.get("currency") == plan.cur["main"]:
        return "the generator opens at the start: the trajectory with no reset (A, BM L2) needs its income"
    return "kept"


def unlock_by(plan, x, req):
    if x in main_mults(plan):
        return "PO5"
    if x.id in plan.amounts:
        return "PC4"
    return "PO12" if req["type"] == "ladder" else "PO2" if is_q_line(x, plan) else "none: kept"


def unlock_text(u):
    if "stage" in u:
        return f"stage {u['stage']}"
    if "total" in u:
        return f"lifetime {u['total']} {u['amount']:g}"
    if "resets" in u:
        return f"{u['resets']} resets"
    if "level" in u:
        return f"{u['level']} level {u['n']}"
    return f"opens with {u['milestone']}" if "milestone" in u else ""


def req_text(req, plan=None):
    """The text of a requirement; with the plan, a currency has its game id (the report uses the names of the game)."""
    t = req["type"]
    if t == "total":
        cur = game_currency(plan, req["currency"]) if plan is not None else req["currency"]
        return f"lifetime {cur} {req['amount']:.4g}"
    if t == "ladder":
        return f"stage {req['step']}"
    if t == "level":
        return f"{req['id']} level {req.get('level', 1)}"
    return f"{t} {req.get('id') or req.get('amount')}"


def not_fitted_lines(plan):
    bad = [x for x in plan.els if x.status == "not fitted"]
    if not bad:
        return ["- none: each element of the content file is in the model."]
    out = []
    for x in bad:
        out += [f"- {x.id}: {x.why_not}.", f"  1. {x.ways[0]}", f"  2. {x.ways[1]}"]
    return out


def merged_stats(plan):
    """[(game stat, model target)] of each stat of the effects that the rebalance merges into the main income: a
    stat that is not a currency of the content (BM: the solver tests the spine on the main income)."""
    stats = {f["stat"] for e in plan.content["elements"] for f in e.get("effects") or [] if f["kind"] != "cap"}
    cur = plan.content.get("stats") or {}
    out = [(st, "coin") for st in sorted(stats) if cur.get(st, st) not in plan.ids and st not in plan.by_id
           and cur.get(st, st) != plan.cur["main"]]
    return out


def limit_lines(plan):
    """The limits of the rebalance for this content: the merged stats and the spine stat."""
    spine = plan.content.get("spine_stat")
    out = [f"- The stat {st} is merged into the main income ({tgt}): each effect on {st} acts on the main income, and a "
           f"penalty on {st} stays a penalty on the main income." for st, tgt in merged_stats(plan)]
    out += l10_lines(plan, spine) + s05_lines(plan)
    if not second_income(plan.content) and spine and (plan.content.get("stats") or {}).get(spine, spine) != \
            plan.cur["main"]:
        out.append(f"- The stages of the game test the stat {spine}. The solver tests the stages on the main income "
                   "(BM L8): the model of the game as built (--as-built) keeps the stat apart.")
    return out or ["- none: each stat of the content is a currency of the model."]


def l10_lines(plan, spine):
    if not second_income(plan.content):
        return []
    return [f"- The stages test the second income {spine} (BM L10): the lines paid in the main currency that raise "
            f"{spine} keep their game shapes, and the reset reads the main income only. The timing fit sets each wall "
            f"from the simulated income of {spine}."]


def s05_lines(plan):
    if not any(is_manual(x.e) for x in plan.els):
        return []
    return ["- S05 of the game is a manual phase, then automation (S05.manual_then_automate). The solver models "
            "S05.active_lane: a constant active factor; the manual phase before the automation is not modelled (BM "
            "section 7 limit 5, a phase change)."]


def counts_line(plan):
    n = len(plan.els)
    bad = sum(1 for x in plan.els if x.status == "not fitted")
    sim = sum(1 for x in plan.els if x.status == "simulation only")
    return (f"Elements: {n} in the content file, {n - bad} in the model ({sim} of them simulation only), "
            f"{bad} not fitted.")


IDENTITY_FIELDS = ("step", "first cost", "cost ratio", "cap", "stat", "unlock", "currency")
SHAPE_FIELDS = ("step", "cost ratio", "cap", "stat")


def line_traits(plan, p):
    """The traits of 1 purchase line of the model that the player sees: step, cost ratio, cap, stat, unlock point
    and currency."""
    e = p["effects"][0] if p["effects"] else {}
    x = plan.by_id.get(p["id"])
    stat = game_stat(x.e) if x is not None and e.get("kind") != "cap" else None
    stat = stat or e.get("target")
    return {"step": round(e.get("value", 0.0), 4), "first cost": float("%.3g" % p["base_cost"]),
            "cost ratio": round(p["cost_growth"], 4),
            "cap": p.get("max_level"), "stat": stat, "unlock": ", ".join(req_text(r) for r in p.get("requires") or [])
            or "open at the start", "currency": p["cost_currency"]}


def identity_check(plan, m):
    """For each pair of lines of the same kind (the kind of the first effect), the traits that differ. A pair whose
    shape (step, cost ratio, cap, stat) does not differ is a finding: the 2 lines differ only by name or by their
    unlock point. A pair with the same shape in the game is not a finding: the design keeps the game (it is listed
    as 'same in the game')."""
    lines = [(p["id"], (p["effects"][0]["kind"] if p["effects"] else ""), line_traits(plan, p)) for p in m["purchases"]]
    pairs = [pair(u, v) for i, u in enumerate(lines) for v in lines[i + 1:] if u[1] == v[1]]
    for q in pairs:
        q["same_in_game"] = not shape_differs(q) and same_game_shape(plan, q["a"], q["b"])
    return dict(pairs=pairs, findings=[f"{q['a']} and {q['b']}" for q in pairs if not shape_differs(q)
                                       and not q["same_in_game"]])


def shape_differs(q):
    return any(f in SHAPE_FIELDS for f in q["differs"])


def same_game_shape(plan, a, b):
    """True when the game lines a and b have the same step, cost ratio, cap and stat."""
    x, y = plan.by_id.get(a), plan.by_id.get(b)
    return x is not None and y is not None and game_shape(plan, x.e) == game_shape(plan, y.e)


def game_shape(plan, e):
    return round(game_step(e), 6), round(game_ratio(e), 6), e.get("cap"), game_stat(e)


def game_stat(e):
    """The game stat of the first effect that is not a cap (the stat that the game reads)."""
    f = next((f for f in e.get("effects") or [] if f["kind"] != "cap"), {})
    return f.get("stat") or e.get("affects")


def pair(u, v):
    (a, kind, ta), (b, _, tb) = u, v
    return dict(a=a, b=b, kind=kind, differs=[f for f in IDENTITY_FIELDS if ta[f] != tb[f]])


KEEP_TOL = 0.25
CHAR_KINDS = ("line", "reward")
FIELD_ROWS = {"effect": ("value ratio", "value", "effects", "family", "cost and value", "all"),
              "cap": ("cap", "levels"), "cost ratio": ("cost ratio", "cost formula")}
CHANGED_SHARE = 0.5
CHAR_REASON = "the rebalance does not keep the game"


def character(content, m, plan=None):
    """The measure "character kept" (references/proof-sheet.md, row CK): for each line and reward, the effect values,
    the cap and the cost ratio of the game against the design. A field is kept (inside KEEP_TOL of the game value),
    moved into the proven range (the game value is outside it), or changed (an obligation forced it)."""
    model = {x["id"]: x for key in ("purchases", "milestones") for x in m.get(key) or []}
    ctx = dict(plan=plan, scale=add_scales(content, m))
    rows = [char_element(ctx, e, model.get(e["id"])) for e in content["elements"] if e["kind"] in CHAR_KINDS]
    unit = getattr(plan, "seed_scale", 1.0) if plan is not None else 1.0
    rows += [char_other(e, m, unit if (e.get("value") or {}).get("currency") == (plan.cur.get("reset") if plan else None)
                        else 1.0) for e in content["elements"] if is_char_other(e)]
    return char_summary(rows, structure_changes(plan, m) if plan is not None else [])


def is_char_other(e):
    """A side system or a grant: its values are its character. The stages (S03) and the reset (S04) are the walls
    and the loop that the timing fit sets, so the measure does not count them."""
    return e["kind"] == "grant" or e["kind"] == "system" and e.get("system") not in ("S03", "S04")


def char_other(e, m, unit=1.0):
    """The values of a side system (its rate) or of a grant (base, growth and trigger) in the game and the design."""
    game, design = other_values(e, m, unit)
    state = "kept" if same_value(game, design) else "changed"
    note = UNIT_NOTE.format(u=unit) if unit != 1.0 and state == "kept" else "" if state == "kept" else \
        "no rule of the solver names it"
    return dict(id=e["id"], kind=e["kind"], state=state, fields=[dict(field="values", game=show_value(game),
                design=show_value(design), state=state, note=note)])


UNIT_NOTE = ("kept: the base is in units of the reset currency, which the timing fit scales by {u:.3g} (seed_scale = "
             "(c0 / c)^p), as each cost of that currency")


def other_values(e, m, unit=1.0):
    if e["kind"] == "system":
        ms = next((x for x in m.get("milestones") or [] if x["id"] == e["id"]), None)
        rate = next((f["value"] for f in (ms or {}).get("effects") or [] if f["kind"] == "add"), None)
        return side_rate(e), rate
    v = e.get("value") or {}
    where = (m.get("prestige") or {}) if v.get("trigger") == "reset" else ((m.get("ladders") or [{}])[0])
    found = grant_found(where, v, unit)
    return grant_text(v), (grant_text(dict(found, base=v.get("base"))) if found else None)


def grant_found(where, v, unit):
    """The grant of the model with the game base of v times unit (3 digits when the unit is not 1), or None."""
    base = float("%.3g" % (v.get("base", 1.0) * unit)) if unit != 1.0 else v.get("base")
    return next((g for g in where.get("grants") or [] if g.get("base") == base), None)


def grant_text(g):
    return f"{g.get('base')} x {g.get('growth')}^n" + (f", {g['count']} times" if g.get("count") is not None else "")


def add_scales(content, m):
    """{game stat: (game base, model base)}: an add effect is compared as its share of the base income of its stat.
    With no base income on one side, the value of the first add effect on the stat is the scale."""
    adds = {f["stat"] for e in content["elements"] for f in e.get("effects") or [] if f["kind"] == "add"}
    return {st: base_scale(content, m, st) or first_add_scale(content, m, st) for st in adds}


def base_scale(content, m, st):
    """(game base, model base) of the currency of the stat, or None when one side has no base income."""
    stats = content.get("stats") or {}
    model = {c["id"]: c.get("base_income", 0.0) for c in m.get("currencies") or []}
    ids = model_ids(content) if content["currencies"].get("main") else {}
    cur = stats.get(st, st)
    game = sum(b.get("value", 0.0) for b in content.get("base_income") or [] if stats.get(b["stat"], b["stat"]) == cur)
    mod = model.get(ids.get(cur, cur), 0.0)
    return (game, mod) if game > 0 and mod else None


def first_add_scale(content, m, st):
    first = next((e for e in content["elements"] if e["kind"] == "line" and add_on(e, st)), None)
    mp = next((p for p in m.get("purchases") or [] if first and p["id"] == first["id"]), None)
    g = next(f["value"] for f in first["effects"] if f["kind"] == "add" and f["stat"] == st)
    v = next((f["value"] for f in (mp or {}).get("effects") or [] if f["kind"] == "add"), g)
    return g or 1.0, v or 1.0


def add_on(e, st):
    return any(f["kind"] == "add" and f["stat"] == st for f in e.get("effects") or [])


def char_element(ctx, e, mo):
    """The fields of 1 element and its state: changed when a field is changed, else moved, else kept."""
    x = ctx["plan"].by_id.get(e["id"]) if ctx["plan"] is not None else None
    if mo is None:
        why = x.why_not if x is not None and x.why_not else "the element is not in the model"
        return dict(id=e["id"], kind=e["kind"], state="changed", fields=[dict(field="element", game="in the game",
                    design="not in the model", state="changed", note=why)])
    fields = [char_field(ctx, x, f) for f in game_fields(e, mo, ctx["scale"])]
    states = {f["state"] for f in fields}
    state = "changed" if "changed" in states else "moved" if "moved" in states else "kept"
    return dict(id=e["id"], kind=e["kind"], state=state, fields=fields)


def game_fields(e, mo, scale):
    """[(field, game value, design value)]: each effect value (as a step), the cap and the cost ratio."""
    g_fx = [f for f in e.get("effects") or [] if f["kind"] != "cap"]
    m_fx = [f for f in mo.get("effects") or [] if f["kind"] != "cap"]
    out = [effect_field(f, g, scale.get(f["stat"], (1.0, 1.0))) for f, g in itertools.zip_longest(g_fx, m_fx[:len(g_fx)])]
    return out + (line_fields(e, mo) if e["kind"] == "line" else [])


def effect_field(f, g, sc):
    return "effect", step_value(f, sc[0]), step_value(g, sc[1]) if g else None


def line_fields(e, mo):
    """The cap and (for more than 1 level) the cost ratio of a line in the game and the design."""
    out = [("cap", e.get("levels"), mo.get("max_level"))]
    if e.get("levels") != 1:
        out.append(("cost ratio", cost_shape(e.get("cost") or {}), cost_shape(dict(
            ratio=mo.get("cost_growth"), formula=mo.get("cost_formula", "exp")))))
    return out


def step_value(f, base):
    """The size of 1 effect: the factor of a mult, 1 + v of a pct, v / (the base income) of an add."""
    if f is None:
        return None
    if f["kind"] == "add":
        return ("add", f["value"] / base)
    return ("step", f["value"] if f["kind"] == "mult" else 1.0 + f["value"])


def cost_shape(cost):
    return (cost.get("formula") or "exp", cost.get("ratio"))


def char_field(ctx, x, item):
    field, game, design = item
    state = "kept" if same_value(game, design) else "changed"
    rng = (getattr(x, "ranges", {}) or {}).get(field) if x is not None else None
    if state == "changed" and rng:
        state = "moved"
    note = range_note(rng) if state == "moved" else obligation_of(x, field) if state == "changed" else ""
    return dict(field=field, game=show_value(game), design=show_value(design), state=state, note=note)


def same_value(a, b):
    """Equal, or 2 numbers of the same kind within KEEP_TOL of the game value."""
    if a == b:
        return True
    if isinstance(a, tuple) and isinstance(b, tuple):
        return a[0] == b[0] and same_value(*log_size(a, b))
    if isinstance(a, (int, float)) and isinstance(b, (int, float)) and a:
        return abs(b / a - 1.0) <= KEEP_TOL + 1e-9
    return False


def log_size(a, b):
    """A step and an exp cost ratio compare by their size ln v (a step x1.25 against x1.015 is not kept); an add value
    and a linear cost ratio compare by value."""
    if a[0] in ("step", "exp") and a[1] and b[1] and a[1] > 0 and b[1] > 0:
        return math.log(a[1]), math.log(b[1])
    return a[1], b[1]


def show_value(v):
    if isinstance(v, tuple):
        return f"{v[0]} {cell(v[1])}"
    return "none" if v is None else cell(v)


def range_note(rng):
    return f"the game value is outside the proven range {rng[0]:.4g} to {rng[1]:.4g}"


def obligation_of(x, field):
    """The obligation that forced a change: the row of the plan for the field, its reason and the row that set it."""
    rows = [r for r in (x.rows if x is not None else []) if r["field"] in FIELD_ROWS[field]]
    return "; ".join(f"{r['by']}: {r['reason']}" for r in rows) or "no rule of the solver names it"


STRUCT_REASON = "the model changes the structure of the game, and the designer did not accept it"


def char_summary(rows, structure=()):
    """The counts of the measure. Each structure change (structure_changes) counts as 1 changed element. A structure
    change that the designer did not accept makes the row fail."""
    n = len(rows) + len(structure)
    count = {s: sum(1 for r in rows if r["state"] == s) for s in ("kept", "moved", "changed")}
    count["changed"] += len(structure)
    reason = char_reason(count["changed"] <= CHANGED_SHARE * n, [c["name"] for c in structure if not c["accepted"]])
    return dict(elements=rows, total=n, **count, share=round(count["kept"] / n, 4) if n else 1.0,
                verdict="not ready" if reason else "ready", reason=reason, structure=list(structure))


def char_reason(share_ok, bad):
    if bad:
        return STRUCT_REASON + ": " + ", ".join(bad)
    return "" if share_ok else CHAR_REASON


def character_text(ch):
    """The summary line of the measure: the share of elements kept, then the line of the structure changes."""
    return (f"{ch['kept']} of {ch['total']} elements kept ({ch['share']:.0%}), {ch['moved']} moved into the proven "
            f"range, {ch['changed']} changed" + (f": not ready, {ch['reason']}" if ch["reason"] else "") +
            f". {structure_text(ch.get('structure') or [])}")


def structure_text(st):
    """The structure line of CK: the count of structure changes, each with its name, accepted or not."""
    if not st:
        return "Structure: 0 changes"
    return f"Structure: {len(st)} changes (" + ", ".join(
        f"{c['name']} {'accepted' if c['accepted'] else 'not accepted'}" for c in st) + ")"


def character_lines(ch):
    head = ["| Element | State | Field | Game | Design | State of the field | Range or obligation |",
            "| --- | --- | --- | --- | --- | --- | --- |"]
    rows = [f"| {r['id']} | {r['state']} | {f['field']} | {f['game']} | {f['design']} | {f['state']} | {f['note']} |"
            for r in ch["elements"] for f in r["fields"]]
    return [f"Character kept: {character_text(ch)}.", ""] + head + rows + [""]


def pair_text(q):
    if shape_differs(q):
        return ", ".join(q["differs"])
    if q.get("same_in_game"):
        return (", ".join(q["differs"]) or "nothing") + ": the same shape in the game, kept"
    return (", ".join(q["differs"]) or "nothing") + ": FINDING (the shape is the same)"


def identity_lines(ident):
    head = ["| Line | Line | Kind | What differs |", "| --- | --- | --- | --- |"]
    rows = [f"| {q['a']} | {q['b']} | {q['kind']} | {pair_text(q)} |" for q in ident["pairs"]]
    found = ident["findings"]
    text = [f"Identity: {len(ident['pairs'])} pairs of lines of the same kind, {len(found)} findings"
            + (": " + "; ".join(found) + " have the same shape (step, cost ratio, cap, stat)." if found else "."), ""]
    return text + head + rows + [""]


FIELD_KIND = {"purchase": "line", "milestone": "reward", "side_system": "side system", "ladder": "stage rule",
              "prestige": "reset rule", "grant": "stage reward"}


def field_map(plan, m, d):
    """{model id: {kind, game id, game place, fields, stat, unlock}} so that a game task does not make the map by
    hand. stat names the game stat of each effect and the model target; unlock names the unlock kind and the game
    field."""
    out = {x.id: dict(map_entry(x), stat=stat_text(plan, x), game_stat=game_stat_text(plan, x),
                      unlock=unlock_field(plan, plan.reqs.get(x.id))) for x in plan.fitted()}
    for x in [x for x in plan.fitted() if moved_to_new_name(x)]:
        out[x.id]["new_name"] = x.new_name
    out.update(currency_entries(plan))
    out.update(bank_entries(d))
    out.update(new_entries(plan, m, out))
    out.update(stage=dict(kind=FIELD_KIND["ladder"], game_id=None, place=None, fields=STAGE_FIELDS),
               prestige=dict(kind=FIELD_KIND["prestige"], game_id=None, place=None, fields=RESET_FIELDS))
    return dict(sorted(out.items()))


def currency_entries(plan):
    return {mid: dict(kind="currency", game_id=gid, place=None, fields=["base_income"] if mid == "coin" else [])
            for gid, mid in sorted(plan.ids.items())}


def moved_to_new_name(x):
    return bool(getattr(x, "new_name", None)) and x.new_name != (x.e.get("name") or x.id)


def new_entries(plan, m, known):
    """The model entries with no game element (a new stage reward or line): "build it", with their effects."""
    items = [(key, q) for key, qs in (("milestone", m.get("milestones")), ("purchase", m.get("purchases")))
             for q in qs or [] if q["id"] not in known and q["id"] not in plan.ids.values()]
    return {q["id"]: dict(kind=f"new {FIELD_KIND.get(key, key)}: build it", game_id=None, place=None,
                          fields=fields_of(FIELD_KIND.get(key)), stat=effects_stat(q)) for key, q in items}


def effects_stat(q):
    return ", ".join(f"{f['target']} ({f['kind']} {f['value']:g})" for f in q.get("effects") or [])


STAGE_FIELDS = ["count", "stage_requirements (table)", "requirement_growth", "stage_rule rate"]
RESET_FIELDS = ["prestige_divisor (c)", "prestige_exponent (p)", "prestige_min_run_seconds",
                "prestige_min_gain (the smallest gain)", "prestige_gain_floor (false: the gain keeps its fractions)"]
UNLOCK_FIELDS = {"ladder": ("stage", "unlock_stage"), "prestiges": ("count of resets", "unlock_prestiges"),
                 "milestone": ("milestone", "opens with the element {id}"), "time": ("time", "unlock_seconds")}


def game_currency(plan, mid):
    return next((g for g, i in plan.ids.items() if i == mid), mid)


def unlock_field(plan, req):
    """'kind: game field' of an unlock: a stage, a lifetime total of a currency, a count of resets, a milestone."""
    if not req:
        return "open at the start"
    if req["type"] == "total":
        return f"lifetime total: unlock_lifetime_{game_currency(plan, req['currency'])}"
    kind, field = UNLOCK_FIELDS.get(req["type"], (req["type"], req["type"]))
    return f"{kind}: {field.format(id=req.get('id'))}"


def stat_text(plan, x):
    """The game stat of each effect of x and the model target of it ('game stat -> model target')."""
    effs = x.e.get("effects") or ([dict(target=x.e.get("affects"), kind=x.e.get("family"))] if x.e.get("affects")
                                  else [])
    out = []
    for f in effs:
        t = f.get("target")
        model = t if f.get("kind") == "cap" else stat_target(plan, t)
        out.append(f"{t} -> {model}" + (" (cap)" if f.get("kind") == "cap" else ""))
    return ", ".join(out)


def income_stat(plan):
    """The stat that the game reads for the main income: of the stats that map to the main currency, the stat with
    the most mult and pct effects in the content (a factor of the model acts on the full income)."""
    stats = plan.content.get("stats") or {}
    main = plan.cur["main"]
    count = collections.Counter(f["stat"] for e in plan.content["elements"] for f in e.get("effects") or []
                                if f["kind"] in ("mult", "pct") and stats.get(f["stat"], f["stat"]) == main)
    return max(sorted(count), key=count.get) if count else main


def game_stat_text(plan, x):
    """The stat that the game must change for each effect of the model: a factor on the main income acts on the
    income stat (income_stat); each other effect keeps its game stat."""
    effs = (x.model.get("effects") if x.model.get("kind") == "milestone" else None) or x.e.get("effects") or []
    return ", ".join(read_stat(plan, x, f) for f in effs)


def read_stat(plan, x, f):
    """The stat that the game reads for 1 effect."""
    st = f.get("stat") or f.get("target")
    on_main = f["kind"] != "cap" and stat_target(plan, st) == "coin"
    factor = x.model.get("effect") in ("mult", "pct") or f["kind"] in ("mult", "pct")
    return income_stat(plan) if on_main and factor else st


def map_entry(x):
    kind = FIELD_KIND.get(x.model.get("kind"), "line")
    return dict(kind=kind, game_id=x.id, place=x.e.get("place"), fields=fields_of(kind))


def bank_entries(d):
    if "S08" not in d.chosen:
        return {}
    return {"bank": dict(kind="bank line", game_id=None, place=None, fields=["cost", "growth", "max_level", "mult"]),
            BANK_CUR: dict(kind="bank", game_id=None, place=None, fields=["bank_period_s", "bank_cap"])}


def fields_of(kind):
    return {"line": ["cost", "growth", "max_level", "effect", "unlock"], "reward": ["effects", "unlock"],
            "side system": ["unlock", "values kept"], "stage reward": ["reward_base", "reward_growth"],
            "stage rule": [], "reset rule": []}.get(kind, [])


def rules(m):
    """Item c: the rules that the game must copy, read from the model."""
    pr = m.get("prestige") or {}
    floor = pr.get("floor", True)
    return dict(
        reset_gain_rounding="rounded down (floor)" if floor else "not rounded: the gain keeps its fractions",
        stage_after_reset="the stage survives a reset" if "stage" not in (pr.get("resets") or {}).get("ladders", [])
        else "a reset sets the stage to 0",
        stage_rule_income="a stage compares the full income (the active factor counts, also for an absent player)",
        reset_opens=reset_opens_text(pr))


def reset_opens_text(pr):
    if pr.get("requires"):
        return "the reset opens at " + ", ".join(req_text(r) for r in pr["requires"])
    return (f"no requirement: the reset opens when the gain (E / c)^p reaches {pr.get('min_gain', 1)} after a run of "
            f"{pr.get('min_run_seconds', 0):g} s or more, and the "
            f"player resets by the rule G >= {pr.get('policy_ratio', 1):g} x L")


RULE_NAMES = {"reset_gain_rounding": "Reset gain", "stage_after_reset": "Stage after a reset",
              "stage_rule_income": "Stage rule", "reset_opens": "When the reset opens"}


def rule_lines(rl, content=None):
    """Each rule of the model, with the rule of the game beside it (the S03 and S04 values of the content)."""
    game = game_rules(content) if content else {}
    return [f"- {RULE_NAMES[k]}: model: {v}; game: {game.get(k, 'not in the content file')}" +
            (" (the same)." if game.get(k) == v else ".") for k, v in rl.items()]


def game_rules(content):
    """The rules of the game as the content file states them (values of S03 and S04)."""
    els = {e.get("system"): e for e in content["elements"] if e["kind"] == "system"}
    st, rs = (els.get("S03") or {}).get("values") or {}, (els.get("S04") or {}).get("values") or {}
    out = {}
    if "floor" in rs:
        out["reset_gain_rounding"] = "rounded down (floor)" if rs["floor"] else "not rounded: the gain keeps its fractions"
    if "keeps_stage" in rs:
        out["stage_after_reset"] = "the stage survives a reset" if rs["keeps_stage"] else "a reset sets the stage to 0"
    if st.get("test"):
        out["stage_rule_income"] = f"a stage compares the {'run total' if st['test'] == 'run_total' else 'income'} " \
                                   f"of {st.get('stat') or content['currencies']['main']}"
    if els.get("S04"):
        u = unlock_text(els["S04"].get("unlock") or {}) or "no unlock"
        out["reset_opens"] = f"{u}, a gain of {rs.get('min_gain', 1)} or more, a run of {rs.get('min_run_seconds', 0)} " \
                             "s or more"
    return out


def map_lines(fm):
    head = ["| Model id | Kind of game data | Game id | Place | Fields | Game stat -> model target | Stat that the game "
            "reads | Unlock: game field | New name |", "| --- | --- | --- | --- | --- | --- | --- | --- | --- |"]
    return head + [f"| {mid} | {v['kind']} | {v['game_id'] or 'new: build'} | {v['place'] or ''} | "
                   f"{', '.join(v['fields'])} | {v.get('stat', '')} | {v.get('game_stat', '')} | {v.get('unlock', '')} | "
                   f"{v.get('new_name', '')} |" for mid, v in fm.items()]


def copy_model(m):
    return copy.deepcopy(m)


BUILT_RESET_SYSTEM = "meta"


def built_stat(content, stat):
    """The model currency of a game stat in the model of the game as built: the currency of content.stats, else
    the stat itself (a stat that the game keeps apart is a currency of its own)."""
    return (content.get("stats") or {}).get(stat, stat)


def built_currencies(content):
    """The currencies of the game, the stats that the game keeps apart (stat: true, never spent), with the base
    income of content.base_income that pays also while the player is away."""
    cur = content["currencies"]
    spend = [cur["main"]] + ([cur["reset"]] if cur.get("reset") else []) + list(cur.get("side") or [])
    stats = sorted({built_stat(content, s) for s in (content.get("stats") or {})} - set(spend))
    stats += sorted(set(built_spine_stats(content)) - set(spend) - set(stats))
    base = built_base_sums(content)
    out = [dict(id=c, name=c, system=built_system_of(content, c), base_income=base.get(c, 0.0)) for c in spend]
    return out + [dict(id=s, name=f"{s} (stat)", system="main", stat=True, base_income=base.get(s, 0.0))
                  for s in stats]


def built_base_sums(content):
    """The base income that pays also while the player is away, summed for each model target."""
    rows = built_base(content, active=False)
    targets = dict.fromkeys(b["target"] for b in rows)
    return {t: sum((b["value"] for b in rows if b["target"] == t), 0.0) for t in targets}


def built_spine_stats(content):
    st = built_stages(content)
    return [built_stat(content, (st.get("values") or {}).get("stat") or content.get("spine_stat")
                       or content["currencies"]["main"])] if st else []


def built_base(content, active):
    """The base income of content.base_income: [{stat, value, active_only}], as model effects."""
    return [dict(target=built_stat(content, b["stat"]), kind="add", value=b["value"]) for b in
            content.get("base_income") or [] if bool(b.get("active_only")) == active]


def built_system_of(content, cur):
    c = content["currencies"]
    if cur == c["main"]:
        return "main"
    return BUILT_RESET_SYSTEM if cur == c.get("reset") else "side_" + cur


def built_effects(content, e):
    return [built_effect(content, f) for f in e.get("effects") or []]


def built_effect(content, f):
    t = f["stat"] if f["kind"] == "cap" else built_stat(content, f["stat"])
    x = dict(target=t, kind=f["kind"], value=f["value"])
    return dict(x, active=True) if f.get("active_only") else x


def built_requirement(content, e):
    """The unlock of the game as a model requirement, with no change."""
    u = e.get("unlock") or {}
    key = next((k for k in BUILT_UNLOCKS if u.get(k)), None)
    if key:
        return [BUILT_UNLOCKS[key](u)]
    return built_milestone_requirement(content, u.get("milestone"))


BUILT_UNLOCKS = {"stage": lambda u: dict(type="ladder", id="stage", step=u["stage"]),
                 "total": lambda u: dict(type="total", currency=u["total"], amount=u["amount"]),
                 "resets": lambda u: dict(type="prestiges", amount=u["resets"]),
                 "level": lambda u: dict(type="level", id=u["level"], level=u["n"])}


def built_milestone_requirement(content, m):
    """An unlock by another element: level 1 of a line, else the milestone of a reward or a system."""
    kind = next((el["kind"] for el in content["elements"] if el["id"] == m), None) if m else None
    if kind == "line":
        return [dict(type="level", id=m, level=1)]
    return [dict(type="milestone", id=m)] if kind in ("reward", "system") else []


def built_purchase(content, e):
    c = e["cost"]
    p = dict(id=e["id"], name=e.get("name") or e["id"], system=built_system_of(content, e["currency"]),
             cost_currency=e["currency"], base_cost=c["base"], cost_growth=c["ratio"],
             cost_formula=c.get("formula", "exp"), effects=built_effects(content, e),
             requires=built_requirement(content, e))
    if e.get("levels") is not None:
        p["max_level"] = e["levels"]
    return p


def side_rate(e):
    """The income of the currency of a side system for each second: values.rate_per_second, else the old keys
    initial_beds x dew_per_bed / drip_seconds."""
    v = e.get("values") or {}
    if v.get("rate_per_second") is not None:
        return v["rate_per_second"]
    return v.get("initial_beds", 1) * v.get("dew_per_bed", 1) / (v.get("drip_seconds") or 1.0)


def built_milestones(content):
    out = [built_milestone(content, e) for e in content["elements"]]
    active = built_base(content, active=True)
    feature = dict(id="base_active", name="Base income while the player plays", system="main", kind="feature",
                   persists=True, requires=[], effects=[dict(f, active=True) for f in active])
    return [m for m in out if m] + ([feature] if active else [])


def built_milestone(content, e):
    """A reward as a milestone, a side system as a system unlock that pays its rate, else None."""
    name = e.get("name") or e["id"]
    if e["kind"] == "reward":
        return dict(id=e["id"], name=name, system="main", kind="reward", persists=True,
                    effects=built_effects(content, e), requires=built_requirement(content, e))
    if e["kind"] != "system" or e.get("system") in ("S03", "S04") or not e.get("currency"):
        return None
    return dict(id=e["id"], name=name, system=built_system_of(content, e["currency"]), kind="system_unlock",
                persists=True, requires=built_requirement(content, e),
                effects=[dict(target=e["currency"], kind="add", value=side_rate(e))])


def built_stages(content):
    return next((e for e in content["elements"] if e["kind"] == "system" and e.get("system") == "S03"), None)


def built_reset(content):
    return next((e for e in content["elements"] if e["kind"] == "system" and e.get("system") == "S04"), None)


def need(values, keys, who):
    miss = [k for k in keys if values.get(k) is None]
    check(bool(miss), f"--as-built: {who} needs values.{', values.'.join(miss)} (references/design-solver.md, "
                      "'The content file')")


def built_ladder(content):
    st = built_stages(content)
    check(st is None, "--as-built: the content needs the stages (a system element with system S03)")
    v = st.get("values") or {}
    need(v, ("base", "growth"), f"the stages {st['id']}")
    check(not st.get("levels"), f"--as-built: the stages {st['id']} need levels (the count of stages)")
    req = dict(type=v.get("test", "rate"), currency=built_spine_stats(content)[0], base=v["base"], growth=v["growth"])
    if v.get("table"):
        req["table"] = list(v["table"])
    lad = dict(id="stage", name="Stage {n}", system="main", count=st["levels"], excitement=3, requires=req)
    grants = [built_grant(content, g) for g in content["elements"] if g["kind"] == "grant" and
              (g.get("value") or {}).get("trigger", "stage") == "stage"]
    if grants:
        lad["grants"] = grants
    return lad


def built_grant(content, g):
    v = g.get("value") or {}
    return grant_count(dict(currency=v.get("currency") or g.get("currency") or content["currencies"]["main"],
                            base=v.get("base", 1.0), growth=v.get("growth", 1.0)), v)


def built_prestige(content, lines):
    """The reset of the game: the gain (E / divisor)^exponent of the main currency (formula power), or
    k x growth^stage (formula exp); the opening rule, the run floor and the gain floor of the game."""
    r = built_reset(content)
    if r is None:
        return dict(enabled=False)
    v = r.get("values") or {}
    cur = content["currencies"]
    pr = dict(enabled=True, system=BUILT_RESET_SYSTEM, currency=cur["reset"], k=v.get("k", 1.0),
              min_gain=v.get("min_gain", 1), min_run_seconds=v.get("min_run_seconds", 0),
              floor=bool(v.get("floor", True)), policy="ratio", policy_ratio=1.0,
              requires=built_requirement(content, r), resets=built_resets(content, lines, v))
    pr.update(pw_players.reset_rules(v, cur["main"]))
    pr.update(built_gain(r, v, cur["main"]))
    grants = [built_grant(content, g) for g in content["elements"] if g["kind"] == "grant" and
              (g.get("value") or {}).get("trigger") == "reset"]
    if grants:
        pr["grants"] = grants
    return pr


def built_resets(content, lines, v):
    """What a reset sets to 0: the main currency and the stats, the main lines, and the stage unless it is kept."""
    main = content["currencies"]["main"]
    resets = [c["id"] for c in built_currencies(content) if c["id"] == main or c.get("stat")]
    systems = sorted({p["system"] for p in lines if p["system"] == "main"} | {"main"})
    return dict(currencies=resets, purchase_systems=systems, ladders=[] if v.get("keeps_stage") else ["stage"])


def built_gain(r, v, main):
    """The gain formula of the reset: exp on the stage, else power on the main currency."""
    if v.get("formula", "power") == "exp":
        need(v, ("growth",), f"the reset {r['id']}")
        return dict(basis_ladder="stage", formula="exp", growth=v["growth"])
    need(v, ("divisor",), f"the reset {r['id']}")
    return dict(basis_currency=main, formula="power", base=v["divisor"], exponent=v.get("exponent", r.get("value")))


def built_systems(content):
    cur = content["currencies"]
    out = [dict(id="main", loop="core"), dict(id=BUILT_RESET_SYSTEM, loop="meta", persists=True)]
    return out + [dict(id="side_" + c, loop="minigame", persists=True) for c in cur.get("side") or []]


def as_built_model(content):
    """The model of the game with its present numbers and no change (pw_design.py --as-built). The game block
    comes from pw_design.py."""
    lines = [built_purchase(content, e) for e in content["elements"] if e["kind"] == "line"]
    m = dict(currencies=built_currencies(content), systems=built_systems(content), purchases=lines,
             milestones=built_milestones(content), ladders=[built_ladder(content)],
             prestige=built_prestige(content, lines))
    return m


NOT_REACHED = "not reached: the active player of the simulator does not reach it before the hard end (PC5)"


def mark_unreached(plan, proof):
    """'Fitted' means that the active player reaches the element: each element that row PC5 lists is not fitted,
    with the change of the row. The mark holds for 1 round: the next round fits the element again."""
    row = next((r for r in (proof or {}).get("rows") or [] if r["id"] == "PC5"), None)
    units = [(plan.by_id.get(u["id"]), u["change"]) for u in (row or {}).get("unreached") or []]
    marked = [(x, change) for x, change in units if x is not None and x.status != "not fitted"]
    for x, change in marked:
        x.not_fitted(NOT_REACHED, [change, "lower its cost or move its unlock earlier in the content file"])
        x.reach_mark = True
    return marked


def not_fitted_hard(plan):
    """The elements not fitted for a reason that a larger s does not change (not the reach mark)."""
    return [x for x in plan.els if x.status == "not fitted" and not getattr(x, "reach_mark", False)
            and not getattr(x, "budget_mark", False)]
