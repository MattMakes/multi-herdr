"""Stage 13, standalone target: install the verified skills into a skills dir
(spec 8.5, 3.8).

One folder per skill, the source's own partition. A name that exists gets
`<name>-<owner>`, then `<name>-<owner>-2`, `-3`...; its frontmatter `name` and
every self-reference are renamed to match. REPLACE is never automatic: only a
`--resolve` value asks for it, and the old folder then moves to C/replaced/.

Install is atomic per skill: copy K/out/<skill> to
<skills-dir>/.intake-tmp-<task>-<name>/, check each copied file's sha256
against C/verify.json, apply the rename, write .intake-provenance.json, then
rename to the final name. A failure removes the temp folder.
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
from pathlib import Path
from urllib.parse import urlparse

from . import discover, evidence, task

SKIP, REPLACE = "SKIP", "REPLACE"
PROVENANCE = ".intake-provenance.json"
LOG = "installed.jsonl"
MAX_SUFFIX = 99


class Unresolvable(task.TaskError):
    """A name collision that no rule resolves (exit 7)."""


def default_skills_dir() -> Path:
    return Path.home() / ".claude" / "skills"


def owner(prov: dict) -> str:
    """The GitHub owner, or the stem of the folder or archive (spec 8.5)."""
    src = prov["source"]
    raw = ""
    if src["kind"] == "github" and src.get("url"):
        raw = urlparse(src["url"]).path.strip("/").split("/", 1)[0]
    else:
        raw = Path(src["input"].rstrip("/")).name
        if src["kind"] == "archive":
            raw = raw.rsplit(".", 1)[0]
    return discover.normalize(raw) or "intake"


def _fit(stem: str, suffix: str = "") -> str:
    """`stem` + `suffix`, with `stem` cut so the result fits the 64-character name rule."""
    return stem[:64 - len(suffix)].rstrip("-") + suffix


def parse_resolve(text: str | None, skills: list[str]) -> dict[str, str]:
    """harvest's --resolve JSON: {"<skill>": "new-name" | "SKIP" | "REPLACE"}."""
    if not text:
        return {}
    try:
        doc = json.loads(text)
    except ValueError as e:
        raise task.TaskError("--resolve is not JSON") from e
    if not isinstance(doc, dict) or not all(isinstance(v, str) for v in doc.values()):
        raise task.TaskError('--resolve must be {"<skill>": "new-name" | "SKIP" | "REPLACE"}')
    unknown = [k for k in doc if k not in skills]
    if unknown:
        raise task.TaskError(f"--resolve names a skill that is not in K/out: {evidence.escape(unknown[0])}")
    return doc


def plan(skills: list[str], skills_dir: Path, own: str, resolve: dict[str, str], replaced: Path) -> list[dict]:
    """One step per skill: {"skill", "name", "action": install|skip|replace}. Raises
    Unresolvable before anything is written."""
    taken = set()
    steps = []
    for skill in skills:
        want = resolve.get(skill)
        if want == SKIP:
            steps.append({"skill": skill, "name": skill, "action": "skip"})
            continue
        if want == REPLACE:
            if os.path.lexists(replaced / skill):
                raise Unresolvable(f"C/replaced/{skill} exists: a REPLACE of {skill} already ran in this task")
            steps.append({"skill": skill, "name": skill, "action": "replace"})
            taken.add(skill)
            continue
        if want is not None:
            if not discover.NAME.match(want):
                raise Unresolvable(f"--resolve gives {skill} a bad name: {evidence.escape(want)}")
            if os.path.lexists(skills_dir / want) or want in taken:
                raise Unresolvable(f"--resolve gives {skill} the name {want}, which exists")
            name = want
        else:
            name = skill
            n = 1
            while os.path.lexists(skills_dir / name) or name in taken:
                if n > MAX_SUFFIX:
                    raise Unresolvable(f"no free name for {skill} after {MAX_SUFFIX} tries")
                name = _fit(f"{skill}-{own}") if n == 1 else _fit(f"{skill}-{own}", f"-{n}")
                n += 1
        taken.add(name)
        steps.append({"skill": skill, "name": name, "action": "install"})
    return steps


def _files(root: Path) -> list[str]:
    """Every file under `root` as a relative path. Anything that is not a regular
    file or a folder (a symlink, a device) is refused."""
    out = []
    for d, dirs, names in os.walk(root, followlinks=False):
        dirs.sort()
        for name in sorted(names) + [x for x in dirs if os.path.islink(os.path.join(d, x))]:
            p = Path(d) / name
            if p.is_symlink() or not p.is_file():
                raise task.TaskError(f"K/out holds a non-regular file: {evidence.escape(str(p.relative_to(root)))}")
            out.append(p.relative_to(root).as_posix())
    return out


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def tree_sha256(root: Path) -> str:
    """sha256 over sorted `relpath\\0sha256\\n` lines, the provenance file left out."""
    lines = sorted(f"{rel}\0{sha256(root / rel)}\n" for rel in _files(root) if rel != PROVENANCE)
    return hashlib.sha256("".join(lines).encode("utf-8", "surrogateescape")).hexdigest()


def rename_refs(root: Path, old: str, new: str) -> None:
    """Rename the frontmatter `name` and every whole-word `old` in the skill's text files."""
    word = re.compile(r"(?<![\w-])" + re.escape(old) + r"(?![\w-])")
    for rel in _files(root):
        p = root / rel
        try:
            text = p.read_bytes().decode("utf-8")
        except UnicodeDecodeError:
            continue
        changed = word.sub(new, text)
        if changed != text:
            p.write_bytes(changed.encode("utf-8"))


def copy_checked(src: Path, tmp: Path, skill: str, files: dict[str, str]) -> None:
    """Copy K/out/<skill> into `tmp` and check each copy against verify.json."""
    rels = _files(src)
    want = {r[len(skill) + 1:]: h for r, h in files.items() if r.startswith(skill + "/")}
    if set(rels) != set(want):
        raise task.TaskError(f"K/out/{skill} changed after intake verify: run intake verify again")
    tmp.mkdir()
    for rel in rels:
        dst = tmp / rel
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(src / rel, dst)
        if sha256(dst) != want[rel]:
            raise task.TaskError(f"K/out/{skill}/{evidence.escape(rel)} differs from verify.json: run intake verify again")


def install_one(t: task.Task, prov: dict, step: dict, skills_dir: Path, files: dict[str, str],
                target: str = "standalone") -> dict:
    """Copy, check, rename and move 1 skill into place. The standalone target also gets
    .intake-provenance.json; the repo target keeps its provenance in docs/cleanroom/."""
    skill, name = step["skill"], step["name"]
    final = skills_dir / name
    tmp = skills_dir / f".intake-tmp-{t.id}-{name}"
    if os.path.lexists(tmp):
        shutil.rmtree(tmp)
    try:
        copy_checked(t.k / "out" / skill, tmp, skill, files)
        if name != skill:
            rename_refs(tmp, skill, name)
        digest = tree_sha256(tmp)
        record = {
            "task_id": t.id, "name": name, "renamed_from": skill if name != skill else None,
            "target": target, "path": str(final), "sha256": digest, "installed_at": task.now(),
            "source": {k: prov["source"].get(k) for k in ("kind", "input", "url", "ref", "commit")},
            "licence": prov.get("licence"),
        }
        if target == "standalone":
            task.write_json(tmp / PROVENANCE, record)
        backup = None
        if step["action"] == "replace" and os.path.lexists(final):
            backup = t.c / "replaced" / name
            backup.parent.mkdir(mode=0o700, exist_ok=True)
            shutil.move(str(final), str(backup))
        try:
            os.rename(tmp, final)
        except OSError:
            if backup is not None:
                shutil.move(str(backup), str(final))
            raise
    except BaseException:
        shutil.rmtree(tmp, ignore_errors=True)
        raise
    if backup is not None:
        record["replaced"] = str(backup)
    return record


def load_verify(t: task.Task, target: str) -> dict:
    """C/verify.json, when it is a GATE 2 pass for `target`."""
    if t.manifest.get("gate2") != "pass":
        raise task.TaskError("install needs a GATE 2 pass: run intake verify first")
    vpath = t.c / "verify.json"
    if not vpath.is_file():
        raise task.TaskError("no C/verify.json: run intake verify first")
    vdoc = task.read_json(vpath)
    if vdoc.get("gate") != "pass" or vdoc.get("target") != target or "files" not in vdoc:
        raise task.TaskError(f"C/verify.json is not a {target} GATE 2 pass: run intake verify --target {target}")
    return vdoc


def out_skills(t: task.Task) -> list[str]:
    """The skill folders of K/out, sorted. Refuses an empty K/out or a bad name."""
    out = t.k / "out"
    skills = sorted(p.name for p in out.iterdir() if p.is_dir() and not p.is_symlink()) if out.is_dir() else []
    bad = [s for s in skills if not discover.NAME.match(s)]
    if bad or not skills:
        raise task.TaskError("K/out holds no skill folder" if not skills else
                             f"K/out holds a folder with a bad skill name: {evidence.escape(bad[0])}")
    return skills


def log(t: task.Task, prov: dict, rec: dict) -> None:
    """Append the install record to installed.jsonl and C/provenance.json `installed`."""
    with open(t.base / LOG, "a", encoding="utf-8") as f:
        f.write(json.dumps(rec, ensure_ascii=False) + "\n")
    prov.setdefault("installed", []).append(
        {"name": rec["name"], "target": rec["target"], "renamed_from": rec["renamed_from"],
         "sha256": rec["sha256"], "path": rec["path"]})
    task.write_json(t.c / "provenance.json", prov)


def run(t: task.Task, skills_dir: Path, resolve_text: str | None) -> dict:
    """Install every skill of K/out. Returns {"installed", "skipped"}."""
    vdoc = load_verify(t, "standalone")
    skills_dir = task.resolve(skills_dir)
    if task.overlaps(skills_dir, t.base):
        raise task.TaskError("the skills dir and the workspace base overlap")
    skills = out_skills(t)
    prov = task.read_json(t.c / "provenance.json")
    skills_dir.mkdir(parents=True, exist_ok=True)
    steps = plan(skills, skills_dir, owner(prov), parse_resolve(resolve_text, skills), t.c / "replaced")
    installed, skipped = [], []
    for step in steps:
        if step["action"] == "skip":
            skipped.append(step["skill"])
            continue
        rec = install_one(t, prov, step, skills_dir, vdoc["files"])
        installed.append(rec)
        log(t, prov, rec)
    task.set_phase(t, "installed")
    return {"installed": installed, "skipped": skipped}
