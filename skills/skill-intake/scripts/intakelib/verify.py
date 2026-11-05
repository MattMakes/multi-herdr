"""Stage 12 of `intake verify`: check K/out/ before anything is installed, and
keep GATE 2 (spec 7.1-7.4, 4.4, 6.10).

Steps, each one check row in C/verify.json:
1. resweep: every scanner rule (4.2, 4.4) runs on K/out/ with the same
   thresholds. A hit is accepted only inside a declared capability (7.4).
2. findings: each source finding closes as `mitigated-by-rewrite`, `removed`,
   or stays `unresolved` (7.1 step 2). An OPQ finding closes only when no
   sha256, file name or 12+ character string of its artifact is in K/out/.
3. handoff-integrity and overlap: the leakage checks that run at verify (6.10).
4. review-coverage: every spec requirement (behavior) and every
   `replace_or_remove` item has a reviewer `pass` verdict in K/review.json,
   and every SUP and OPQ item of mitigation.json has a `replace_or_remove` item.
5. scripts: every rewritten script has a test run through `intake cr verify`.
6. target-rules (7.3), read from K/context.json.
7. domain-snapshot: no domain incident was recorded (6.10).
8. persona, when K/persona.md exists: no INJ or SCR hit of medium or higher
   (SCR runs on every line, prose included), no overlap with the source, and
   every reviewer `persona` verdict is `pass`. verify.json records its sha256.

GATE 2 aborts when any check fails. Failing conditions: a critical or high
finding unresolved; a new high+ hit outside a declared capability; a leakage
check failure; a missing reviewer verdict; a failed or missing script test (with
a usable sandbox); a target rule failure; a domain incident. A medium (or
lower) unresolved finding or new hit does not abort: it is listed for the report.
Nothing here prints source text: details name output files, rule ids and
source paths only.
"""

from __future__ import annotations

import hashlib
import re
from pathlib import Path

from . import domain, evidence, handoff, opaque, overlap, sandbox, scan, schema, sweep, targets, task, triage

PASS, ABORT = "pass", "abort"
SEVERE = ("critical", "high")
OPQ_MIN = 12  # the shortest artifact string the absence check looks for
OVERLAP_MAX_BYTES = 1_000_000

# Never accepted in our output, whatever the spec declares (7.4).
NEVER_ACCEPTED = frozenset({"SCR-03", "SCR-05", "SCR-07", "INJ-01", "INJ-02", "INJ-04", "INJ-06", "SUP-02"})
# The declared capabilities (spec 6.9 `capabilities` keys) a rule hit needs, all of them.
CAPABILITY_RULES = {
    "SCR-01": ("network",), "SCR-02": ("reads",), "SCR-04": ("network", "reads"), "SCR-06": ("runs",),
    "SCR-08": ("writes",), "SCR-11": ("writes",), "INJ-05": ("reads",),
}
TOUCHES = re.compile(r"^ {0,3}#{1,6}\s+What this skill touches\s*#*\s*$", re.IGNORECASE | re.MULTILINE)


def _check(name: str, ok: bool | str, detail: str = "") -> dict:
    result = ok if isinstance(ok, str) else ("pass" if ok else "fail")
    row = {"check": name, "result": result}
    if detail:
        row["detail"] = detail
    return row


def _join(items: list[str], cap: int = 20) -> str:
    more = f"; {len(items) - cap} more" if len(items) > cap else ""
    return "; ".join(items[:cap]) + more


# ---- inputs ------------------------------------------------------------------

def load_specs(t: task.Task) -> list[dict]:
    """The handed-off specs of K/handoff/package.json (integrity is its own check)."""
    pkg = t.k / "handoff" / "package.json"
    if not pkg.is_file():
        return []
    specs = []
    for item in task.read_json(pkg)["items"]:
        p = t.k / "handoff" / item["file"]
        if p.is_file():
            specs.append(task.read_json(p))
    return specs


def load_review(t: task.Task) -> tuple[dict | None, list[str]]:
    """(review, problems). The review must be a valid reviewer report."""
    path = t.k / "review.json"
    if not path.is_file():
        return None, ["no K/review.json"]
    try:
        review = task.read_json(path)
    except ValueError:
        return None, ["K/review.json is not JSON"]
    errors = schema.validate(review, schema.load_schema("report"))
    if errors:
        return None, ["K/review.json: " + evidence.escape(errors[0])]
    if review["role"] != "reviewer":
        return None, ["K/review.json is not a reviewer report"]
    return review, []


def passed_items(review: dict | None) -> set[tuple[str, str]]:
    """The (spec id, item) pairs with a reviewer `pass` verdict."""
    return {(v["spec_id"], v["item"]) for v in (review or {}).get("verdicts", []) if v["result"] == "pass"}


def out_skills(out: Path) -> list[str]:
    return sorted(p.name for p in out.iterdir() if p.is_dir() and not p.is_symlink()) if out.is_dir() else []


def skill_of_hit(hit: dict) -> str:
    return hit["file"].split("/", 1)[0]


# ---- 1. re-sweep and declared capabilities -----------------------------------

def touches(out: Path, skill: str) -> bool:
    p = out / skill / "SKILL.md"
    return p.is_file() and bool(TOUCHES.search(p.read_text(encoding="utf-8", errors="replace")))


def accepted_by(hit: dict, specs: list[dict], passed: set, has_touches: bool) -> str | None:
    """The spec id that accepts `hit` as a declared capability, or None (7.4)."""
    need = CAPABILITY_RULES.get(hit["rule"])
    if hit["rule"] in NEVER_ACCEPTED or not need or not has_touches:
        return None
    for spec in specs:
        caps = spec.get("capabilities") or {}
        if all(caps.get(k) for k in need) and all((spec["spec_id"], f"capability:{k}") in passed for k in need):
            return spec["spec_id"]
    return None


def resweep(out: Path, specs: list[dict], passed: set) -> dict:
    """Scan K/out/. Returns {"hits", "accepted", "new"}: every hit is either accepted
    (with the spec that declares it) or new."""
    hits = sweep.scan_tree(out) if out.is_dir() else []
    by_skill: dict[str, list[dict]] = {}
    for s in specs:
        by_skill.setdefault(s["skill"]["name"], []).append(s)
    has_touches = {name: touches(out, name) for name in out_skills(out)}
    accepted, new = [], []
    for h in hits:
        skill = skill_of_hit(h)
        public = {k: v for k, v in h.items() if not k.startswith("_")}
        spec_id = accepted_by(h, by_skill.get(skill, []), passed, has_touches.get(skill, False))
        if spec_id:
            accepted.append({**public, "skill": skill, "spec_id": spec_id})
        else:
            new.append({**public, "skill": skill})
    return {"hits": hits, "accepted": accepted, "new": new}


# ---- 2. opaque absence and closing findings -----------------------------------

def _strings(data: bytes) -> set[bytes]:
    return {s.strip().encode() for s in opaque.printable_strings(data) if len(s.strip()) >= OPQ_MIN}


def artifact_marks(src: Path, rec: dict) -> tuple[set[str], set[str], set[bytes]]:
    """(sha256s, file names, strings of 12+ characters) of 1 opaque artifact."""
    path = src / rec["path"]
    shas, names, strings = {rec["sha256"]}, {rec["path"].rstrip("/").rsplit("/", 1)[-1]}, set()
    if path.is_file() and not path.is_symlink():
        strings = _strings(path.read_bytes())
    elif path.is_dir():
        for _, p in sweep.walk(path):
            data = p.read_bytes()
            shas.add(hashlib.sha256(data).hexdigest())
            strings |= _strings(data)
    return shas, names, strings


def opaque_present(out_files: list[tuple[str, bytes]], marks) -> list[str]:
    """What of an artifact is in the output: 'sha256 in <file>', 'file name in <file>', ..."""
    shas, names, strings = marks
    found = []
    name_res = [re.compile(rb"(?<![\w-])" + re.escape(n.encode()) + rb"(?![\w-])") for n in names if n]
    for rel, data in out_files:
        shown = evidence.escape(rel)
        if hashlib.sha256(data).hexdigest() in shas:
            found.append(f"sha256 in {shown}")
        if any(r.search(data) for r in name_res) or any(n == rel.rsplit("/", 1)[-1] for n in names):
            found.append(f"file name in {shown}")
        n = sum(1 for s in strings if s in data)
        if n:
            found.append(f"{n} string(s) in {shown}")
    return found


def r_items(specs: list[dict], passed: set) -> dict[str, dict]:
    """finding id -> {"action", "passed"} from every spec's replace_or_remove list."""
    out = {}
    for s in specs:
        for item in s.get("replace_or_remove", []):
            ok = (s["spec_id"], item["id"]) in passed
            prev = out.get(item["finding"])
            out[item["finding"]] = {"action": item["action"], "passed": ok or bool(prev and prev["passed"])}
    return out


def close(f: dict, entry: dict, fired: set[tuple[str | None, str]], present: dict[str, list[str]],
          items: dict[str, dict]) -> tuple[str, str]:
    """(status, why) of 1 source finding (7.1 step 2)."""
    rule, skill = f["rule"], f.get("skill")
    family = rule.split("-", 1)[0]
    if entry["status"] == "removed" or entry["disposition"] == "removed":
        return "removed", "skipped at acquire"
    if (skill, rule) in fired or (skill is None and any(r == rule for _, r in fired)):
        return "unresolved", f"{rule} still fires on the output outside a declared capability"
    if family == "OPQ" and present.get(f["id"]):
        return "unresolved", "the artifact is in the output: " + _join(present[f["id"]], 5)
    if family in ("SUP", "OPQ"):
        item = items.get(f["id"])
        if not item:
            return "unresolved", "no replace_or_remove item in any handed-off spec"
        if not item["passed"]:
            return "unresolved", "the replace_or_remove item has no reviewer pass verdict"
        return ("removed" if item["action"] == "remove" else "mitigated-by-rewrite"), item["action"]
    return ("removed" if entry["disposition"] == "remove" else "mitigated-by-rewrite"), entry["disposition"]


# ---- 3. leakage ----------------------------------------------------------------

FRONT_NAME = re.compile(r"^name\s*:.*$")
FRONT_DESC = re.compile(r"^description\s*:")


def without_front_keys(text: str) -> str:
    """The text with the frontmatter `name:` line and the `description:` key removed.
    Every skill has both, and the name may cross the wall, so they are not wording."""
    lines = text.split("\n")
    if not lines or lines[0].strip() != "---":
        return text
    for i in range(1, len(lines)):
        if lines[i].strip() == "---":
            break
        if FRONT_NAME.match(lines[i]):
            lines[i] = ""
        else:
            lines[i] = FRONT_DESC.sub("", lines[i])
    return "\n".join(lines)


def overlap_hits(src: Path, specs: list[dict], out_texts: list[tuple[str, str]]) -> list[dict]:
    sources, code = [], set()
    for rel, p in sweep.walk(src) if src.is_dir() else []:
        if p.stat().st_size > OVERLAP_MAX_BYTES:
            continue
        text = sweep.read_text(p)
        if text is None:
            continue
        label = "source:" + evidence.escape(rel)
        sources.append((label, without_front_keys(text)))
        if overlap.is_code(rel):
            code.add(label)
    for s in specs:
        sources.append(("spec:" + s["spec_id"], "\n".join(scan.strings_of(s))))
    index = overlap.Index(sources, code)
    hits = []
    for rel, text in out_texts:
        body = without_front_keys(text)  # keeps the line count, so line numbers stay true
        for h in index.hits(rel, body):
            hits.append({"file": evidence.escape(rel), "kind": h["kind"], "source": h["source"],
                         "words": len(h["shared"].split()), "lines": shared_lines(body, h["kind"], h["shared"])})
    return hits


def shared_lines(text: str, kind: str, shared: str) -> list[list[int]]:
    """The [start, end] line ranges of `text` that hold the shared run. The run is
    matched on the same word (or token) sequence the overlap scan uses, read line
    by line; when a run crosses lines in a way that per-line reading cannot see,
    the whole file is the range."""
    seq_of = overlap.tokens if overlap.KINDS[kind][0] == "code" else overlap.words
    flat, at = [], []
    lines = text.split("\n")
    for n, line in enumerate(lines, 1):
        seq = seq_of("\n" + line + "\n")
        flat += seq
        at += [n] * len(seq)
    want = shared.split()
    size = len(want)
    found = [[at[i], at[i + size - 1]] for i in range(len(flat) - size + 1)
             if flat[i] == want[0] and flat[i:i + size] == want]
    return found or [[1, len(lines)]]


# ---- the fix list (K/fixes.json) -----------------------------------------------

FIXES = "fixes.json"
FIX_SEVERITY = ("critical", "high", "medium")


def fix_list(new_hits: list[dict], overlaps: list[dict], persona: dict | None, sizes: dict[str, int]) -> dict:
    """The clean-side lines GATE 2 asks the writer to change (K/fixes.json).
    Each entry is only a K-relative file, a line range, the reason class and a rule
    id: no matched text, no source label, no C or D path, no score. A hit on line 0
    is about the whole file (an OPQ class), so its range is every line: `sizes`
    gives the line count of each K/out file."""
    entries = set()
    for h in new_hits:
        if h["severity"] in FIX_SEVERITY:
            a, b = (h["line"], h["line"]) if h["line"] > 0 else (1, sizes.get(h["file"], 1))
            entries.add(("out/" + h["file"], a, b, "rule", h["rule"]))
    for h in overlaps:
        for a, b in h["lines"]:
            entries.add(("out/" + h["file"], a, b, "overlap", None))
    for h in (persona or {}).get("hits", []):
        if h["severity"] in PERSONA_FAIL:
            entries.add((PERSONA, h["line"], h["line"], "rule", h["rule"]))
    for h in (persona or {}).get("overlap", []):
        for a, b in h["lines"]:
            entries.add((PERSONA, a, b, "overlap", None))
    order = sorted(entries, key=lambda e: (e[0], e[1], e[2], e[3], e[4] or ""))
    return {"fixes": [{"file": f, "lines": [a, b], "kind": k, "rule": r} for f, a, b, k, r in order]}


# ---- 5. script tests -----------------------------------------------------------

SPLIT = re.compile(r"[\s=;&|'\"(),]+")


def scripts_of(out: Path, target: str) -> list[str]:
    """Rewritten scripts, as paths relative to K/out/."""
    found = []
    for skill in out_skills(out):
        for rel, p in sweep.walk(out / skill):
            full = f"{skill}/{rel}"
            if target == "repo":
                if re.fullmatch(r"references/script-[^/]+\.md", rel):
                    found.append(full)
                continue
            text = sweep.read_text(p)
            lines = text.split("\n", 1)[:1] if text is not None else []
            if sweep.is_script(sweep.Source(full, rel.rsplit("/", 1)[-1], lines)):
                found.append(full)
    return found


def covers(argv: list[str], rel: str, out: str) -> bool:
    """True when a test command names the script: its path, name or stem, or a folder that holds it."""
    base = rel.rsplit("/", 1)[-1]
    stem = base.rsplit(".", 1)[0]
    for arg in argv:
        for part in SPLIT.split(arg):
            p = part[len(out) + 1:] if part.startswith(out + "/") else part
            p = p[2:] if p.startswith("./") else p
            p = p.rstrip("/")
            if p in ("", ".", ".."):
                continue
            if p in (rel, base, stem) or rel.startswith(p + "/") or rel.endswith("/" + p):
                return True
    return False


def script_status(t: task.Task, scripts: list[str], box: str | None) -> list[dict]:
    log = t.k / "verify-log" / "log.json"
    runs = task.read_json(log)["runs"] if log.is_file() else []
    out = str(task.resolve(t.k / "out"))
    rows = []
    for rel in scripts:
        mine = [r["status"] for r in runs if covers(r["argv"], rel, out)]
        if box is None:
            status, why = "unverified", "no usable sandbox on this host"
        elif "pass" in mine:
            status, why = "pass", ""
        elif "fail" in mine:
            status, why = "fail", "its test run failed"
        elif mine:
            status, why = "unverified", "its test ran without a sandbox"
        else:
            status, why = "fail", "no test run through intake cr verify"
        row = {"path": evidence.escape(rel), "status": status}
        if why:
            row["why"] = why
        rows.append(row)
    return rows


# ---- 6. target rules -----------------------------------------------------------

def target_rules(t: task.Task, target: str) -> tuple[dict | None, str]:
    """The rules of K/context.json, or (None, why) when they are missing or not ours."""
    path = t.k / "context.json"
    if not path.is_file():
        return None, "no K/context.json: run intake cr context --target " + target
    ctx = task.read_json(path)
    if ctx.get("target") != target:
        return None, f"K/context.json is for target {evidence.escape(str(ctx.get('target')))}, not {target}"
    if ctx.get("target_rules") != targets.RULES[target]:
        return None, "the target rules in K/context.json differ from the intake rules"
    return ctx["target_rules"], ""


# ---- 8. the teammate persona ---------------------------------------------------

PERSONA = "persona.md"
PERSONA_FAIL = ("critical", "high", "medium")


def persona_hits(text: str) -> list[dict]:
    """INJ hits of K/persona.md, and SCR hits with every line read as a command:
    the persona loads into every session of the teammate, so a command in its
    prose is still a command."""
    hits = [h for h in sweep.scan_file(PERSONA, text) if h["rule"].startswith("INJ-")]
    lines = [ln[:-1] if ln.endswith("\r") else ln for ln in text.split("\n")]
    src = sweep.Source(PERSONA, PERSONA, lines)
    hits += [h for h in sweep.script_hits(src, [sweep.Unit(list(enumerate(lines, 1)), False)])
             if h["rule"].startswith("SCR-")]
    return sweep.finish(hits)


def persona_check(t: task.Task, src: Path, review: dict | None) -> tuple[dict, dict] | None:
    """(check row, verify.json record) for K/persona.md, or None when there is none."""
    p = t.k / PERSONA
    if not p.is_file() or p.is_symlink():
        return None
    data = p.read_bytes()
    text = sweep.as_text(data)
    fails, hits, overlaps = [], [], []
    if text is None:
        fails.append("K/persona.md is not text")
    else:
        hits = [{"rule": h["rule"], "severity": h["severity"], "line": h["line"]} for h in persona_hits(text)]
        fails += [f"{h['rule']} {h['severity']} at line {h['line']}" for h in hits if h["severity"] in PERSONA_FAIL]
        overlaps = overlap_hits(src, [], [("K/persona.md", text)])
        fails += sorted({f"shares {h['kind']} text with {h['source']}" for h in overlaps})
    verdicts = [v["result"] for v in (review or {}).get("verdicts", []) if v["item"] == "persona"]
    if not verdicts:
        fails.append("no reviewer persona verdict")
    elif any(r != "pass" for r in verdicts):
        fails.append("a reviewer persona verdict is not pass")
    record = {"sha256": hashlib.sha256(data).hexdigest(), "hits": hits, "overlap": overlaps}
    return _check("persona", not fails, _join(fails)), record


# ---- the stage -----------------------------------------------------------------

def run(t: task.Task, target: str, budget_exempt: dict[str, str]) -> dict:
    """Run every check, write C/verify.json and the gate2 result in C/task.json. Returns verify.json."""
    triage.require_gate1_pass(t)
    if target not in targets.TARGETS:
        raise task.TaskError(f"unknown target {target!r}; known: {', '.join(targets.TARGETS)}")
    c, out, src = t.c, t.k / "out", t.c / "quarantine" / "src"
    findings = task.read_json(c / "sweep" / "findings.json")["findings"]
    entries = {e["finding"]: e for e in task.read_json(c / "mitigation.json")["entries"]}
    specs = load_specs(t)
    review, review_problems = load_review(t)
    passed = passed_items(review)
    out_files = [(rel, p.read_bytes()) for rel, p in sweep.walk(out)] if out.is_dir() else []
    checks = []

    # 1. re-sweep
    rs = resweep(out, specs, passed)
    severe_new = [h for h in rs["new"] if h["severity"] in SEVERE]
    checks.append(_check("resweep", not severe_new, _join(
        [f"{h['rule']} {h['severity']} in {h['file']}:{h['line']}" for h in severe_new])))

    # 2. close findings
    fired = {(h["skill"], h["rule"]) for h in rs["new"]}
    present = {}
    for f in findings:
        if f["rule"].startswith("OPQ-"):
            rec_path = c / "sweep" / "opaque" / f"{f.get('scan_id')}.json"
            if not rec_path.is_file():
                present[f["id"]] = ["no opaque record"]
                continue
            present[f["id"]] = opaque_present(out_files, artifact_marks(src, task.read_json(rec_path)))
    items = r_items(specs, passed)
    rows = []
    for f in findings:
        status, why = close(f, entries[f["id"]], fired, present, items)
        rows.append({"finding": f["id"], "rule": f["rule"], "severity": f["severity"], "skill": f["skill"],
                     "status": status, "why": why})
    severe_open = [r["finding"] for r in rows if r["status"] == "unresolved" and r["severity"] in SEVERE]
    checks.append(_check("findings", not severe_open, ("unresolved: " + _join(severe_open)) if severe_open else ""))

    # 3. leakage
    integrity = handoff.verify_package(t)
    checks.append(_check("handoff-integrity", not integrity, _join(integrity)))
    out_texts = [(rel, text) for rel, data in out_files if (text := sweep.as_text(data)) is not None]
    overlaps = overlap_hits(src, specs, out_texts)
    checks.append(_check("overlap", not overlaps, _join(
        sorted({f"{h['file']} shares {h['kind']} text with {h['source']}" for h in overlaps}))))

    # 4. review coverage
    missing = list(review_problems)
    for s in specs:
        wanted = [b["id"] for b in s["behaviors"]] + [i["id"] for i in s.get("replace_or_remove", [])]
        missing += [f"{s['spec_id']}/{i}" for i in wanted if (s["spec_id"], i) not in passed]
    if not specs:
        missing.append("no handed-off spec")
    no_item = [e["finding"] for e in entries.values()
               if e["finding"].split("-", 1)[0] in ("SUP", "OPQ") and e["disposition"] in ("replace", "remove")
               and e["finding"] not in items]
    missing += [f"{fid}: no replace_or_remove item" for fid in no_item]
    checks.append(_check("review-coverage", not missing, _join(missing)))

    # 5. script tests
    box = sandbox.kind()
    scripts = script_status(t, scripts_of(out, target), box)
    failed = [s["path"] for s in scripts if s["status"] == "fail"]
    unverified = [s["path"] for s in scripts if s["status"] == "unverified"]
    result = "fail" if failed else ("unverified" if unverified else "pass")
    checks.append(_check("scripts", result, _join([f"{p}: fail" for p in failed] + [f"{p}: unverified" for p in unverified])))

    # 6. target rules
    rules, why = target_rules(t, target)
    rule_fails, flags = ([why] if why else []), []
    if rules:
        skills = out_skills(out)
        if not skills:
            rule_fails.append("K/out holds no skill folder")
        loose = [rel for rel, _ in out_files if "/" not in rel]
        rule_fails += [f"{evidence.escape(rel)}: file outside a skill folder" for rel in loose]
        for name in skills:
            fails, fl = targets.check_skill(out / name, rules, budget_exempt.get(name))
            rule_fails += fails
            flags += fl
    checks.append(_check("target-rules", not rule_fails, _join(rule_fails)))

    # 7. domain snapshot
    incidents = []
    for p in sorted(handoff.incidents_dir(t).glob("inc-*.json")):
        inc = task.read_json(p)
        if domain.CHECK in inc.get("checks", []):
            incidents.append(inc["incident_id"])
    checks.append(_check("domain-snapshot", not incidents, _join(incidents)))

    # 8. the teammate persona
    persona = persona_check(t, src, review)
    if persona:
        checks.append(persona[0])

    gate = ABORT if any(ch["result"] == "fail" for ch in checks) else PASS
    listed = [r["finding"] for r in rows if r["status"] == "unresolved" and r["severity"] not in SEVERE]
    result_doc = {
        "task_id": t.id, "target": target, "gate": gate,
        "reasons": [f"{ch['check']}: {ch.get('detail', 'failed')}" for ch in checks if ch["result"] == "fail"],
        "findings": rows, "checks": checks, "scripts": scripts,
        "accepted": rs["accepted"],
        "new_hits": rs["new"],
        "listed": {"unresolved": listed, "new_hits": [h["id"] for h in rs["new"] if h["severity"] not in SEVERE]},
        "overlap": overlaps,
        "flags": flags,
        "budget_exempt": {k: evidence.escape(v) for k, v in sorted(budget_exempt.items())},
        # install checks every file it copies against these (spec 8.5).
        "files": {rel: hashlib.sha256(data).hexdigest() for rel, data in out_files},
        # repo install reads K/persona.md only when its sha256 matches this record.
        "persona": persona[1] if persona else None,
    }
    task.write_json(c / "verify.json", result_doc)
    # Written on every run, so a stale list never outlives the verify that made it.
    sizes = {rel: text.count("\n") + 1 for rel, text in out_texts}
    task.write_json(t.k / FIXES, fix_list(rs["new"], overlaps, persona[1] if persona else None, sizes))
    t.manifest["gate2"] = gate
    task.set_phase(t, "verified" if gate == PASS else "aborted-gate-2")
    return result_doc
