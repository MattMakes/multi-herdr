"""Stage 13, repo target: bundled skills and their bookkeeping in a multi-herdr
checkout (spec 8.2, 8.3, 8.4, 9.1).

Without a mapping, install writes only the answer-independent set of 8.2: the
skills, `skills/copied.json`, the skills README section, `docs/cleanroom/<name>/`
and `REPO_ORIGINAL` in the skills catalog test. With a mapping (`--mapping FILE`)
it also writes the files of the chosen option. Every write is idempotent: a
second run replaces its own sections, rows and list entries and never adds a
duplicate. C/repo-install.json records what an earlier run installed, so a
later run reuses those names instead of renaming them as collisions.

The tool leaves the Claude-teammate count assertions alone. The controller runs
the gate and updates the count tests that fail with "left: N, right: N+1".

Mapping file (JSON):
  {"option": 1 | 2 | 3,
   "teammate": "<new name>", "brief_description": "<= 120 chars",   (options 1, 2)
   "offer_when": ["<glob>", ...], "requires": ["godot", ...],      (optional, options 1, 2)
   "existing": ["<teammate>", ...],                                (options 2, 3)
   "field": "available_skills" | "skills"}                         (default available_skills)
The new teammate's persona body is K/persona.md, written by the writer role.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
from pathlib import Path

from . import discover, evidence, install, roster, targets, task, verify

STATE = "repo-install.json"
PERSONA = "persona.md"
KIND = "own text (clean-room)"
FIELDS = ("available_skills", "skills")
REQUIRES = ("xcode", "blender", "godot", "git-lfs")
BRIEF_MAX = 120
PERSONA_MAX = 16 * 1024
MAX_SUFFIX = 99

CATALOG = "crates/horch-core/tests/skills_catalog.rs"
ORACLES = "crates/horch-core/tests/baseline_oracles.rs"
COPIED = "skills/copied.json"
SKILLS_README = "skills/README.md"
TEAMMATES_README = "teammates/README.md"
TEAMS_DOC = "docs/skills-and-teams.md"
CHANGELOG = "CHANGELOG.md"
TEAMMATES_BEFORE = "## The effort-matrix personas"
TEAMS_SECTION = "## The domain teams"


# ---- small text helpers ----------------------------------------------------------

def plain(text: str) -> str:
    """Escape every code point that is not plain (spec 3.6) except newlines, with no cap."""
    return "".join(ch if ch == "\n" or evidence._plain(ord(ch)) else evidence._code(ord(ch)) for ch in text)


def one_line(text: str) -> str:
    return " ".join(plain(text).split())


def code_list(names: list[str]) -> str:
    q = [f"`{n}`" for n in names]
    return q[0] if len(q) == 1 else ", ".join(q[:-1]) + " and " + q[-1]


def _read(repo: Path, rel: str) -> str:
    return (repo / rel).read_text(encoding="utf-8")


class Changed(set):
    """The files a run changed. `touched` holds every file the run wrote or checked;
    `created` holds those that did not exist before this run wrote them."""

    def __init__(self, items=()):
        super().__init__(items)
        self.touched: set[str] = set()
        self.created: set[str] = set()


def _write(repo: Path, rel: str, text: str, changed: Changed) -> None:
    p = repo / rel
    changed.touched.add(rel)
    if p.is_file() and p.read_text(encoding="utf-8") == text:
        return
    if not p.exists():
        changed.created.add(rel)
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(text, encoding="utf-8")
    changed.add(rel)


def add_rust_list(text: str, const: str, names: list[str]) -> str:
    """Add `names` to `const <const>: &[&str] = &[...]`, each at its sorted place."""
    lines = text.splitlines(keepends=True)
    head = next((i for i, ln in enumerate(lines) if ln.startswith(f"const {const}: &[&str] = &[")), None)
    if head is None:
        raise task.TaskError(f"no const {const} list")
    end = next(i for i in range(head + 1, len(lines)) if lines[i].startswith("];"))
    item = re.compile(r'^\s+"([^"]+)",\s*$')
    for name in sorted(set(names)):
        present = [item.match(lines[i]).group(1) for i in range(head + 1, end) if item.match(lines[i])]
        if name in present:
            continue
        at = next((i for i in range(head + 1, end) if item.match(lines[i]) and item.match(lines[i]).group(1) > name),
                  end)
        lines.insert(at, f'    "{name}",\n')
        end += 1
    return "".join(lines)


def put_section(text: str, heading: str, body: str, before: str | None = None, end_of: str | None = None) -> str:
    """Replace the section `heading` (to the next heading of its level or higher) with
    `heading` + `body`, or insert it before the line `before`, at the end of the section
    `end_of`, or at the end of the file."""
    level = len(heading.split(" ", 1)[0])
    lines = text.splitlines(keepends=True)
    section = [heading + "\n", "\n"] + [ln + "\n" for ln in body.rstrip("\n").split("\n")] + ["\n"]

    def next_heading(start: int, lvl: int) -> int:
        for i in range(start, len(lines)):
            m = re.match(r"^(#+) ", lines[i])
            if m and len(m.group(1)) <= lvl:
                return i
        return len(lines)

    if heading + "\n" in lines:
        start = lines.index(heading + "\n")
        lines[start:next_heading(start + 1, level)] = section
        return "".join(lines)
    at = len(lines)
    if before and before + "\n" in lines:
        at = lines.index(before + "\n")
    elif end_of and end_of + "\n" in lines:
        at = next_heading(lines.index(end_of + "\n") + 1, len(end_of.split(" ", 1)[0]))
    if at == len(lines):
        if lines and not lines[-1].endswith("\n"):
            lines[-1] += "\n"
        if lines and lines[-1].strip():
            lines.append("\n")
    lines[at:at] = section
    return "".join(lines)


def remove_rust_item(text: str, const: str, name: str) -> str:
    """Drop `name` from `const <const>: &[&str] = &[...]`."""
    lines = text.splitlines(keepends=True)
    head = next((i for i, ln in enumerate(lines) if ln.startswith(f"const {const}: &[&str] = &[")), None)
    if head is None:
        return text
    end = next(i for i in range(head + 1, len(lines)) if lines[i].startswith("];"))
    keep = [ln for i, ln in enumerate(lines) if not (head < i < end and ln.strip() == f'"{name}",')]
    return "".join(keep)


def drop_section(text: str, heading: str) -> str:
    """Remove the section `heading` (to the next heading of its level or higher)."""
    level = len(heading.split(" ", 1)[0])
    lines = text.splitlines(keepends=True)
    if heading + "\n" not in lines:
        return text
    start = lines.index(heading + "\n")
    end = next((i for i in range(start + 1, len(lines))
                if (m := re.match(r"^(#+) ", lines[i])) and len(m.group(1)) <= level), len(lines))
    return "".join(lines[:start] + lines[end:])


def remove_from_list(text: str, key: str, names: list[str]) -> str:
    """Take `names` out of the frontmatter list `key`; a list left empty loses its line."""
    lines = text.splitlines(keepends=True)
    b = roster.bounds(lines)
    if b is None:
        return text
    for i in range(b[0], b[1]):
        m = roster._KEY.match(lines[i].rstrip("\n"))
        if not m or m.group(1) != key:
            continue
        items = roster.flow(m.group(2).strip())
        if items is not None:
            left = [x for x in items if x not in names]
            lines[i:i + 1] = [f"{key}: [{', '.join(left)}]\n"] if left else []
            return "".join(lines)
        j = i + 1
        while j < b[1] and roster._ITEM.match(lines[j].rstrip("\n")):
            j += 1
        block = [(ln, roster.unquote(roster._ITEM.match(ln.rstrip("\n")).group(1))) for ln in lines[i + 1:j]]
        left = [ln for ln, x in block if x not in names]
        lines[i:j] = ([lines[i]] + left) if left else []
        return "".join(lines)
    return text


# ---- inputs ----------------------------------------------------------------------

def parse_mapping(path: Path, repo: Path) -> dict:
    """Read and check the mapping answer. Every error is a caller mistake (exit 2)."""
    try:
        doc = task.read_json(path)
    except (OSError, ValueError) as e:
        raise task.TaskError(f"--mapping {path}: not a readable JSON file") from e
    if not isinstance(doc, dict):
        raise task.TaskError("--mapping must be a JSON object")
    allowed = {"option", "teammate", "brief_description", "offer_when", "requires", "existing", "field"}
    extra = sorted(set(doc) - allowed)
    if extra:
        raise task.TaskError(f"--mapping has unknown keys: {evidence.escape(', '.join(extra))}")
    option = doc.get("option")
    if option not in (1, 2, 3):
        raise task.TaskError("--mapping option must be 1, 2 or 3")
    field = doc.get("field", "available_skills")
    if field not in FIELDS:
        raise task.TaskError("--mapping field must be available_skills or skills")
    out = {"option": option, "field": field, "teammate": None, "brief_description": None,
           "offer_when": [], "requires": [], "existing": []}
    if option in (1, 2):
        name, brief = doc.get("teammate"), doc.get("brief_description")
        if not isinstance(name, str) or not discover.NAME.match(name):
            raise task.TaskError("--mapping teammate must be a lower-case name with dashes")
        if (not isinstance(brief, str) or not 0 < len(brief) <= BRIEF_MAX or plain(brief) != brief
                or any(c in brief for c in "\"\\\n")):
            raise task.TaskError(f"--mapping brief_description must be 1 line of 1 to {BRIEF_MAX} plain characters"
                                 " with no quote or backslash")
        offer = doc.get("offer_when", [])
        if not isinstance(offer, list) or not all(isinstance(g, str) and re.fullmatch(r"[A-Za-z0-9._*?-]+", g)
                                                  for g in offer):
            raise task.TaskError("--mapping offer_when must be a list of file name globs")
        req = doc.get("requires", [])
        if not isinstance(req, list) or not all(r in REQUIRES for r in req):
            raise task.TaskError(f"--mapping requires takes only {', '.join(REQUIRES)}")
        out.update(teammate=name, brief_description=brief, offer_when=offer, requires=req)
    elif "teammate" in doc or "brief_description" in doc:
        raise task.TaskError("--mapping option 3 adds no new teammate: drop teammate and brief_description")
    existing = doc.get("existing", [])
    if not isinstance(existing, list) or not all(isinstance(n, str) for n in existing):
        raise task.TaskError("--mapping existing must be a list of teammate names")
    if option in (2, 3) and not existing:
        raise task.TaskError(f"--mapping option {option} needs existing teammates")
    if option == 1 and existing:
        raise task.TaskError("--mapping option 1 adds the skills to no existing teammate: drop existing")
    visible = {n for n, _ in roster.visible(repo / "teammates")}
    unknown = [n for n in existing if n not in visible]
    if unknown:
        raise task.TaskError(f"--mapping existing names a teammate that is not on the roster: "
                             f"{evidence.escape(unknown[0])}")
    out["existing"] = sorted(set(existing))
    return out


def read_persona(t: task.Task) -> str:
    """K/persona.md: the writer role's persona body for the new teammate."""
    p = t.k / PERSONA
    if not p.is_file() or p.is_symlink():
        raise task.TaskError(f"no K/{PERSONA}: the writer role writes the new teammate's persona body")
    raw = p.read_bytes()
    if len(raw) > PERSONA_MAX:
        raise task.TaskError(f"K/{PERSONA} is over {PERSONA_MAX} bytes")
    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError as e:
        raise task.TaskError(f"K/{PERSONA} is not UTF-8") from e
    if plain(text) != text:
        raise task.TaskError(f"K/{PERSONA} holds a character that is not plain text")
    if not text.strip() or any(ln.strip() == "---" for ln in text.splitlines()):
        raise task.TaskError(f"K/{PERSONA} is empty or holds a --- line")
    return text.strip("\n") + "\n"


def load_state(t: task.Task, repo: Path) -> dict:
    p = t.c / STATE
    if not p.is_file():
        return {"repo": str(repo), "skills": {}, "cleanroom": None, "teammate": None, "mapping": None,
                "created": [], "edits": {}, "removed": []}
    state = task.read_json(p)
    for key, empty in (("created", []), ("edits", {}), ("removed", [])):
        state.setdefault(key, empty)
    if state.get("repo") != str(repo):
        raise task.TaskError(f"this task installed into {state.get('repo')}, not {repo}")
    return state


# ---- source facts (escaped) ------------------------------------------------------

def source_line(prov: dict) -> tuple[str, str, str]:
    """(source, licence, version) for the docs, escaped and without machine paths. The
    version is `commit <sha>` for git, else `sha256 <content hash>`."""
    src = prov["source"]
    if src["kind"] == "github" and src.get("url"):
        where = evidence.escape(src["url"].removesuffix(".git"))
    else:
        where = f"a {src['kind']} source named `{evidence.escape(Path(src['input'].rstrip('/')).name)}`"
    lic = prov.get("licence") or {}
    licence = evidence.escape(lic.get("spdx") or f"licence class {lic.get('class', 'none')}")
    if src.get("commit"):
        version = f"commit `{evidence.escape(src['commit'][:12])}`"
    else:
        version = f"sha256 `{evidence.escape((prov.get('archive_sha256') or prov.get('content_sha256') or '')[:12])}`"
    return where, licence, version


# ---- the answer-independent set --------------------------------------------------

def write_copied(repo: Path, names: dict[str, str], exempt: dict[str, str], changed: Changed) -> None:
    doc = json.loads(_read(repo, COPIED))
    keep = [e for e in doc["skills"] if e["name"] not in names.values()]
    for skill, name in names.items():
        entry = {"name": name, "copied_files": [], "verbatim": False}
        if exempt.get(skill):
            entry["budget_exempt"] = exempt[skill]
        keep.append(entry)
    doc["skills"] = sorted(keep, key=lambda e: e["name"])
    _write(repo, COPIED, json.dumps(doc, indent=2, ensure_ascii=False) + "\n", changed)


def write_skills_readme(repo: Path, cr: str, names: list[str], prov: dict, changed: Changed) -> None:
    where, licence, version = source_line(prov)
    they = "It is" if len(names) == 1 else "They are"
    noun = "skill" if len(names) == 1 else "skills"
    need = "needs" if len(names) == 1 else "need"
    body = "\n".join([
        f"{code_list(names)} {'belongs' if len(names) == 1 else 'belong'} to no phase. {they} own text, written"
        f" clean-room with `skill-intake`. A security sweep read {where} ({licence}, {version}) in a"
        " quarantine, one role worker wrote behaviour specs in its own words, and a second role worker that never"
        f" saw the source wrote the {noun} from the specs alone. No text or code was copied, so the"
        f" {noun} {need} no notice file and no runtime download.",
        "",
        f"The specs the writer worked from and the sweep result are in [docs/cleanroom/{cr}/](../docs/cleanroom/{cr}/).",
        "",
        "| Skill | Kind |",
        "| --- | --- |",
        *[f"| [{n}]({n}/SKILL.md) | {KIND} |" for n in sorted(names)],
    ])
    _write(repo, SKILLS_README, put_section(_read(repo, SKILLS_README), f"## Clean-room intake: {cr}", body), changed)


def render_spec(spec: dict) -> str:
    s = spec["skill"]
    out = [f"# {one_line(spec['title'])}", "", f"Spec `{spec['spec_id']}` for the skill `{s['name']}`.", "",
           plain(spec["summary"]), "", "## Purpose", "", plain(s["purpose"]), ""]

    def bullets(title: str, items: list[str]) -> None:
        if items:
            out.extend([f"## {title}", "", *[f"- {one_line(x)}" for x in items], ""])

    bullets("Triggers", s.get("triggers", []))
    bullets("Outputs", s.get("outputs", []))
    caps = spec.get("capabilities") or {}
    bullets("Capabilities", [f"{k}: {', '.join(v)}" for k, v in sorted(caps.items()) if v])
    bullets("Replaced or removed", [f"{r['id']} ({r['finding']}): {r['action']}. {r['behavior']}"
                                    for r in spec.get("replace_or_remove", [])])
    bullets("Public contract", [f"{c['kind']} `{c['name']}`: {c['description']}"
                                for c in spec.get("public_contract", [])])
    bullets("Behaviors", [f"{b['id']}. Given {b['given']}, when {b['when']}, then {b['then']}"
                          for b in spec["behaviors"]])
    bullets("Errors", spec.get("errors", []))
    bullets("Invariants", spec.get("invariants", []))
    bullets("Test scenarios", [f"{x['id']}. {x['description']} Expected: {x['expected']}"
                               for x in spec["test_scenarios"]])
    bullets("Open questions", spec.get("open_questions", []))
    return "\n".join(out).rstrip("\n") + "\n"


def write_cleanroom(t: task.Task, repo: Path, cr: str, installed: list[dict], prov: dict,
                    vdoc: dict, changed: Changed) -> None:
    rel = f"docs/cleanroom/{cr}"
    specs = verify.load_specs(t)
    files = []
    for n, spec in enumerate(sorted(specs, key=lambda s: s["spec_id"]), 1):
        fname = f"{n}-{spec['spec_id'].removeprefix('spec-')}.md"
        _write(repo, f"{rel}/{fname}", render_spec(spec), changed)
        files.append((fname, one_line(spec["title"])))
    where, licence, version = source_line(prov)
    names = [r["name"] for r in installed]
    readme = "\n".join([
        f"# Clean-room specs: {cr}", "",
        f"These specs are the only thing that crossed from the source to the writer of {code_list(names)}.", "",
        f"- **Source:** {where}, {licence}, {version}. Imported with `skill-intake`.",
        "- **Spec side:** analyst role workers read the source in a quarantine and wrote these files in their own"
        " words. A sanitizer checked each one for copied text and source identifiers. They describe behaviour"
        " and hold no text or code from the source.",
        f"- **Writer side:** a writer role worker that never opened the source built the"
        f" {'skill' if len(names) == 1 else 'skills'} from these files"
        " alone. A reviewer checked the skills against them.",
        "- **Sweep:** [sweep.md](sweep.md) lists every security finding and what happened to it."
        " [provenance.json](provenance.json) records the source.", "",
        "| Spec | What it covers |", "| --- | --- |",
        *[f"| [{f}]({f}) | {title} |" for f, title in files],
    ]) + "\n"
    _write(repo, f"{rel}/README.md", readme, changed)
    src = prov["source"]
    record = {
        "task_id": t.id, "fetched_at": prov.get("fetched_at"),
        "source": {"kind": src["kind"],
                   "name": evidence.escape(src["url"] if src["kind"] == "github" and src.get("url")
                                           else Path(src["input"].rstrip("/")).name),
                   "ref": evidence.escape(src["ref"]) if src.get("ref") else None,
                   "commit": evidence.escape(src["commit"]) if src.get("commit") else None,
                   "subdir": evidence.escape(src["subdir"]) if src.get("subdir") else None},
        "content_sha256": prov.get("content_sha256"), "archive_sha256": prov.get("archive_sha256"),
        "licence": {k: (evidence.escape(v) if isinstance(v, str) else v)
                    for k, v in (prov.get("licence") or {}).items() if k in ("spdx", "class", "flag")},
        "skills": [{"name": r["name"], "renamed_from": r["renamed_from"], "sha256": r["sha256"]} for r in installed],
        "gate2": vdoc.get("gate"),
    }
    _write(repo, f"{rel}/provenance.json", json.dumps(record, indent=2, ensure_ascii=False) + "\n", changed)
    _write(repo, f"{rel}/sweep.md", render_sweep(t, cr, vdoc), changed)


def render_sweep(t: task.Task, cr: str, vdoc: dict) -> str:
    fpath, mpath = t.c / "sweep" / "findings.json", t.c / "mitigation.json"
    findings = task.read_json(fpath)["findings"] if fpath.is_file() else []
    plan = {e["finding"]: e for e in (task.read_json(mpath)["entries"] if mpath.is_file() else [])}
    status = {r["finding"]: r["status"] for r in vdoc.get("findings", [])}
    sev = ("critical", "high", "medium", "low", "info")
    out = [f"# Security sweep: {cr}", "", f"GATE 1: {t.manifest.get('gate1')}. GATE 2: {vdoc.get('gate')}.", "",
           "| Severity | Findings |", "| --- | --- |",
           *[f"| {s} | {sum(1 for f in findings if f['severity'] == s)} |" for s in sev], ""]
    if findings:
        out += ["| Finding | Severity | Skill | Disposition | Verify status |", "| --- | --- | --- | --- | --- |"]
        for f in sorted(findings, key=lambda f: (sev.index(f["severity"]), f["id"])):
            skill = evidence.escape(f["skill"]) if f.get("skill") else "-"
            disp = plan.get(f["id"], {}).get("disposition", "-")
            out.append(f"| `{f['id']}` | {f['severity']} | `{skill}` | {evidence.escape(str(disp))} |"
                       f" {status.get(f['id'], '-')} |")
        out.append("")
        ev = [f"{f['id']}: {f['evidence']['untrusted']}" for f in findings if (f.get("evidence") or {}).get("untrusted")]
        if ev:
            out.append(evidence.markdown(ev).rstrip("\n"))
            out.append("")
    return "\n".join(out).rstrip("\n") + "\n"


# ---- the mapping-dependent set ---------------------------------------------------

def sibling_lines(repo: Path) -> dict[str, list[str]]:
    """The shared read-only allowed_tools lines (#58) and the compaction amounts of the first
    visible claude/sonnet teammate, copied as they are."""
    for name, fm in roster.visible(repo / "teammates"):
        if fm.get("agent") != "claude" or fm.get("model") != "sonnet":
            continue
        lines = (repo / "teammates" / f"{name}.md").read_text(encoding="utf-8").splitlines()
        at = next((i for i, ln in enumerate(lines) if ln.startswith("allowed_tools: [") and ln != "allowed_tools: []"),
                  None)
        if at is None:
            continue
        tools = ([lines[at - 1]] if at and lines[at - 1].startswith("#") and "#58" in lines[at - 1] else []) + [lines[at]]
        compact = [ln for ln in lines if ln.startswith(("compact_window:", "compact_at:"))]
        return {"tools": tools, "compact": compact}
    raise task.TaskError("no claude/sonnet teammate with a shared allowed_tools list to copy (#58)")


def teammate_name(repo: Path, want: str, state: dict) -> str:
    if state.get("teammate") and (repo / "teammates" / f"{state['teammate']}.md").is_file():
        return state["teammate"]
    name, n = want, 2
    while os.path.lexists(repo / "teammates" / f"{name}.md"):
        if n > MAX_SUFFIX:
            raise install.Unresolvable(f"no free teammate name for {want} after {MAX_SUFFIX} tries")
        name = f"{want}-{n}"
        n += 1
    return name


def teammate_text(name: str, mapping: dict, skills: list[str], persona: str, sib: dict) -> str:
    fm = ["---", f"name: {name}", f'brief_description: "{mapping["brief_description"]}"', "base: fleet-worker",
          "agent: claude", *sib["tools"], "phase: implementation", "model: sonnet"]
    if mapping["offer_when"]:
        fm.append(f"offer_when: [{', '.join(json.dumps(g) for g in mapping['offer_when'])}]")
    else:
        fm.append("# Offered everywhere: the skills need no project marker.")
    if mapping["requires"]:
        fm.append(f"requires: [{', '.join(mapping['requires'])}]")
    fm += ["fallbacks: [codex-sol]",
           "# medium: the worker follows a written skill. tune-fleet can retune it.",
           "effort: medium", *sib["compact"], "permission_mode: auto", "inherit_plugins: false",
           "# Clean-room skills from skill-intake; see docs/cleanroom/.",
           f"skills: [{', '.join(sorted(skills))}]",
           "mcp_servers: {}",
           "# Fleet rule: no subagents. Ask the orchestrator for more workers.",
           "disallowed_tools: [Agent]",
           "# Stale external copies of the fleet briefing; the repo carries the real one.",
           "disabled_skills: [herdr-orchestrator, herdr-worker]", "---"]
    return "\n".join(fm) + "\n" + persona


def write_mapping(repo: Path, mapping: dict, names: list[str], cr: str, persona: str | None, prov: dict,
                  state: dict, changed: Changed) -> str | None:
    option, field = mapping["option"], mapping["field"]
    new = None
    if option == 3 and state.get("teammate"):
        retract_teammate(repo, state["teammate"], state, changed)
    if option in (1, 2):
        new = teammate_name(repo, mapping["teammate"], state)
        state["teammate"] = new
        _write(repo, f"teammates/{new}.md", teammate_text(new, mapping, names, persona, sibling_lines(repo)), changed)
        _write(repo, CATALOG, add_rust_list(_read(repo, CATALOG), "SKIP_NEW_TEAMMATES", [new]), changed)
        _write(repo, ORACLES, add_rust_list(_read(repo, ORACLES), "SKIP_NEW_TEAMMATES", [new]), changed)
        offered = (f"It is offered only in a project with {code_list(mapping['offer_when'])}."
                   if mapping["offer_when"] else "It is offered in every project.")
        body = "\n".join([
            "| teammate | phase | agent, model, effort | fallback | use for |", "|---|---|---|---|---|",
            f"| `{new}` | implementation | claude, sonnet, medium | `codex-sol` | {mapping['brief_description']} |", "",
            f"It carries {code_list(names)}, written clean-room with `skill-intake` (see"
            f" [docs/cleanroom/{cr}/](../docs/cleanroom/{cr}/)). {offered}"])
        _write(repo, TEAMMATES_README, put_section(_read(repo, TEAMMATES_README), f"## The {new} teammate", body,
                                                   before=TEAMMATES_BEFORE), changed)
    targets_now = {f"teammates/{o}.md": field for o in mapping["existing"]}
    for rel, old in list(state["edits"].items()):
        if targets_now.get(rel) == old["field"]:
            continue
        if (repo / rel).is_file():
            _write(repo, rel, remove_from_list(_read(repo, rel), old["field"], old["added"]), changed)
        state["removed"].append(f"{rel} (reverted: {old['field']} {', '.join(old['added'])})")
        del state["edits"][rel]
    for rel in targets_now:
        text = _read(repo, rel)
        absent = [n for n in names if roster.add_to_list(text, field, [n]) != text]
        kept = state["edits"].get(rel, {}).get("added", [])
        state["edits"][rel] = {"field": field, "added": sorted(set(kept) | set(absent))}
        _write(repo, rel, roster.add_to_list(text, field, names), changed)
    one = len(names) == 1
    noun, it = ("skill", "it") if one else ("skills", "them")
    how = (f"under `available_skills`, so a worker loads {it} on demand" if field == "available_skills"
           else f"under `skills`, so {'it loads' if one else 'they load'} at launch")
    verb = "names" if len(mapping["existing"]) == 1 else "name"
    parts = []
    if new:
        parts.append(f"`{new}` carries the clean-room {noun} {code_list(names)}.")
    if mapping["existing"]:
        who = code_list(mapping["existing"])
        parts.append(f"{who} {'also ' if new else ''}{verb} {it if new else f'the clean-room {noun} ' + code_list(names)}"
                     f" {how}.")
    parts.append(f"The {noun} {'was' if one else 'were'} imported with `skill-intake`"
                 " (see [skills/README.md](../skills/README.md)).")
    _write(repo, TEAMS_DOC, put_section(_read(repo, TEAMS_DOC), f"### Clean-room intake: {cr}", " ".join(parts),
                                        end_of=TEAMS_SECTION), changed)
    where, licence, _ = source_line(prov)
    if new:
        line = f"- New teammate `{new}` carries the new clean-room {noun} {code_list(names)}"
        if mapping["existing"]:
            line += f", which {code_list(mapping['existing'])} also {verb}"
    else:
        line = f"- New clean-room {noun} {code_list(names)}, named by {code_list(mapping['existing'])}"
    line += f". Imported with `skill-intake` from {where} ({licence}); specs in `docs/cleanroom/{cr}/`."
    _write(repo, CHANGELOG, changelog(_read(repo, CHANGELOG), line, f"`docs/cleanroom/{cr}/`"), changed)
    return new


def retract_teammate(repo: Path, old: str, state: dict, changed: Changed) -> None:
    """A later mapping adds no new teammate: remove the one an earlier run of this task created."""
    rel = f"teammates/{old}.md"
    if rel in state["created"] and (repo / rel).is_file():
        (repo / rel).unlink()
        changed.add(rel)
        state["created"].remove(rel)
        state["removed"].append(rel)
    for path in (CATALOG, ORACLES):
        _write(repo, path, remove_rust_item(_read(repo, path), "SKIP_NEW_TEAMMATES", old), changed)
    _write(repo, TEAMMATES_README, drop_section(_read(repo, TEAMMATES_README), f"## The {old} teammate"), changed)
    state["teammate"] = None


def changelog(text: str, line: str, marker: str) -> str:
    """Put `line` under `## Unreleased`, replacing an earlier line that holds `marker`."""
    lines = text.splitlines(keepends=True)
    if "## Unreleased\n" not in lines:
        raise task.TaskError("CHANGELOG.md has no ## Unreleased section")
    start = lines.index("## Unreleased\n") + 1
    end = next((i for i in range(start, len(lines)) if lines[i].startswith("## ")), len(lines))
    for i in range(start, end):
        if marker in lines[i]:
            lines[i] = line + "\n"
            return "".join(lines)
    at = start
    while at < end and not lines[at].strip():
        at += 1
    if at < end:
        lines.insert(at, line + "\n")
    else:
        lines[start:end] = ["\n", line + "\n", "\n"]
    return "".join(lines)


# ---- the stage -------------------------------------------------------------------

def install_skills(t: task.Task, repo: Path, state: dict, prov: dict, vdoc: dict,
                   resolve_text: str | None) -> tuple[list[dict], list[str], list[str]]:
    """Install the K/out skills into repo/skills, reusing what an earlier run of this task
    installed. Returns (records of every skill in place, skipped skills, names installed now)."""
    skills = install.out_skills(t)
    skills_dir = repo / "skills"
    resolve = install.parse_resolve(resolve_text, skills)
    done, todo = [], []
    for skill in skills:
        rec = state["skills"].get(skill)
        if rec and (skills_dir / rec["name"]).is_dir() and install.tree_sha256(skills_dir / rec["name"]) == rec["sha256"]:
            done.append(rec)
        else:
            todo.append(skill)
    steps = install.plan(todo, skills_dir, install.owner(prov), {k: v for k, v in resolve.items() if k in todo},
                         t.c / "replaced")
    skipped, new = [], []
    for step in steps:
        if step["action"] == "skip":
            skipped.append(step["skill"])
            continue
        rec = install.install_one(t, prov, step, skills_dir, vdoc["files"], target="repo")
        install.log(t, prov, rec)
        rec = {"skill": step["skill"], "name": rec["name"], "renamed_from": rec["renamed_from"],
               "sha256": rec["sha256"]}
        state["skills"][step["skill"]] = rec
        done.append(rec)
        new.append(rec["name"])
        task.write_json(t.c / STATE, state)
    return sorted(done, key=lambda r: r["skill"]), skipped, new


class DefaultBranch(task.TaskError):
    """The repo is on its default branch: install writes nothing (exit 15)."""


def _git(repo: Path, *args: str) -> str | None:
    try:
        r = subprocess.run(["git", *args], cwd=str(repo), capture_output=True, text=True, timeout=30,
                           stdin=subprocess.DEVNULL)
    except (OSError, subprocess.SubprocessError):
        return None
    return r.stdout.strip() if r.returncode == 0 else None


def default_branch(repo: Path) -> str:
    """`main`, or the branch refs/remotes/origin/HEAD names when it exists."""
    head = _git(repo, "symbolic-ref", "refs/remotes/origin/HEAD")
    return head.removeprefix("refs/remotes/origin/") if head else "main"


def current_branch(repo: Path) -> str | None:
    return _git(repo, "symbolic-ref", "--short", "-q", "HEAD")


def run(t: task.Task, repo: Path, mapping_path: str | None, resolve_text: str | None, rule: str) -> dict:
    repo = task.resolve(repo)
    if not targets.is_multi_herdr(repo):
        raise task.TaskError(f"{repo} is not a multi-herdr checkout: it lacks a marker file of spec 8.1")
    if task.overlaps(repo, t.base):
        raise task.TaskError("the repo and the workspace base overlap")
    branch = current_branch(repo)
    if branch is not None and branch == default_branch(repo):
        raise DefaultBranch(f"refused: {repo} is on its default branch {evidence.escape(branch)}; "
                            "make a feature branch first")
    vdoc = install.load_verify(t, "repo")
    mapping = parse_mapping(Path(mapping_path), repo) if mapping_path else None
    persona = read_persona(t) if mapping and mapping["option"] in (1, 2) else None
    prov = task.read_json(t.c / "provenance.json")
    state = load_state(t, repo)
    installed, skipped, fresh = install_skills(t, repo, state, prov, vdoc, resolve_text)
    if not installed:
        raise task.TaskError("every skill was skipped: nothing to install")
    names = {r["skill"]: r["name"] for r in installed}
    cr = state.get("cleanroom") or installed[0]["name"]
    state["cleanroom"] = cr
    changed = Changed(f"skills/{n}" for n in fresh)
    write_copied(repo, names, vdoc.get("budget_exempt") or {}, changed)
    _write(repo, CATALOG, add_rust_list(_read(repo, CATALOG), "REPO_ORIGINAL", list(names.values())), changed)
    write_skills_readme(repo, cr, sorted(names.values()), prov, changed)
    write_cleanroom(t, repo, cr, installed, prov, vdoc, changed)
    new = None
    if mapping:
        new = write_mapping(repo, mapping, sorted(names.values()), cr, persona, prov, state, changed)
        state["mapping"] = mapping
    state["target_rule"] = rule
    old_created = set(state["created"])
    state["created"] = sorted(old_created | changed.created)
    if mapping:
        for rel in sorted(old_created - changed.touched):
            p = (repo / rel)
            if task.is_within(p.resolve(), repo) and p.is_file():
                p.unlink()
                changed.add(rel)
                state["removed"].append(rel)
            state["created"].remove(rel)
    gone = {r for r in state["removed"]}
    state["written"] = sorted((set(state.get("written", [])) | set(changed)) - gone)
    task.write_json(t.c / STATE, state)
    task.set_phase(t, "installed" if mapping else "installed-awaiting-mapping")
    return {"target": "repo", "target_rule": rule, "repo": str(repo), "cleanroom": f"docs/cleanroom/{cr}",
            "installed": [{"name": r["name"], "renamed_from": r["renamed_from"]} for r in installed],
            "skipped": skipped, "mapping": mapping["option"] if mapping else None, "teammate": new,
            "changed": sorted(changed), "removed": list(state["removed"])}
