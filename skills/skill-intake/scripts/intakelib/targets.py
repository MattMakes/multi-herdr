"""Target rules (spec 7.3) and their check on K/out/<skill>/ (`intake verify`).

`RULES` is the table the writer gets in K/context.json (`intake cr context
--target`) and the table `intake verify` checks the output against. Verify
reads the rules from K/context.json and refuses rules that differ from this
table, so a changed context file cannot loosen them.
"""

from __future__ import annotations

import subprocess
from pathlib import Path

from . import evidence, frontmatter

TARGETS = ("repo", "standalone")
SKILL_MD_MAX = 12 * 1024  # skills_catalog.rs MAX_SKILL_MD_BYTES
FOLDER_MAX = 160 * 1024  # skills_catalog.rs MAX_SKILL_DIR_BYTES
LICENCE_PREFIXES = ("LICENSE", "LICENCE", "NOTICE", "COPYING")  # skills_bundled_no_licence_files
FRONTMATTER_KEYS = ["description", "name"]
STANDALONE_DOTFILES = [".intake-provenance.json"]  # written by install, never by the writer

_COMMON = {
    "skill_md_max_bytes": SKILL_MD_MAX,
    "folder_max_bytes": FOLDER_MAX,
    "licence_files": "none",
    "frontmatter_keys": FRONTMATTER_KEYS,
    "interpreters": ["node 18+", "python3 3.10+", "POSIX sh"],
    "dependencies": "standard library only",
}
RULES = {
    "repo": {
        **_COMMON,
        "files": "only .md and .txt; each script is a fenced code block in references/script-<name>.md",
        "file_suffixes": [".md", ".txt"],
        "budgets": "enforced; a folder over the budget needs a budget_exempt reason",
        "enforce_budgets": True,
        "dotfiles": [],
        "fleet_rules": ["no subagents", "no human approval gate: message the orchestrator instead",
                        "no machine paths", "no ${CLAUDE_*} variables", "no runtime downloads",
                        "no dependence on another skill or an external script"],
    },
    "standalone": {
        **_COMMON,
        "files": "real script files allowed",
        "file_suffixes": None,
        "budgets": "guidance, not enforced",
        "enforce_budgets": False,
        "dotfiles": STANDALONE_DOTFILES,
        "fleet_rules": ["no runtime downloads", "no machine paths"],
    },
}


HARNESS_DIR, HARNESS_MARK = ".claude", ".cc-writes"


def drop_harness_folder(out: Path) -> list[str]:
    """Remove K/out/.claude when it holds only an empty .cc-writes folder, and
    return the K-relative paths removed. The Claude harness makes that folder in
    a role's working folder, and the writer works in K/out. Anything else in
    .claude stays, so the target rules still fail it."""
    d, mark = out / HARNESS_DIR, out / HARNESS_DIR / HARNESS_MARK
    for p in (d, mark):
        if p.is_symlink() or not p.is_dir():
            return []
    if [p.name for p in d.iterdir()] != [HARNESS_MARK] or any(mark.iterdir()):
        return []
    mark.rmdir()
    d.rmdir()
    return [f"out/{HARNESS_DIR}/{HARNESS_MARK}", f"out/{HARNESS_DIR}"]


def files(folder: Path) -> list[tuple[str, Path]]:
    """Every entry under `folder` that is not a directory, sorted, as (rel path, path)."""
    return sorted((p.relative_to(folder).as_posix(), p) for p in folder.rglob("*")
                  if p.is_symlink() or not p.is_dir())


def check_skill(folder: Path, rules: dict, budget_exempt: str | None) -> tuple[list[str], list[str]]:
    """(failures, flags) of 1 output skill folder. Failures fail GATE 2; flags go to the report."""
    fails, flags = [], []
    name = folder.name
    entries = files(folder)
    suffixes = rules["file_suffixes"]
    for rel, p in entries:
        shown = evidence.escape(f"{name}/{rel}")
        base = rel.rsplit("/", 1)[-1]
        if p.is_symlink() or not p.is_file():
            fails.append(f"{shown}: not a regular file")
            continue
        if any(part.startswith(".") for part in rel.split("/")) and rel not in rules["dotfiles"]:
            fails.append(f"{shown}: dotfile")
        if base.upper().startswith(LICENCE_PREFIXES):
            fails.append(f"{shown}: licence or notice file")
        if suffixes is not None and Path(base).suffix.lower() not in suffixes:
            fails.append(f"{shown}: file type not allowed (only {' and '.join(suffixes)})")
    skill_md = folder / "SKILL.md"
    if not skill_md.is_file() or skill_md.is_symlink():
        fails.append(f"{evidence.escape(name)}: no SKILL.md")
    else:
        size = skill_md.stat().st_size
        if size > rules["skill_md_max_bytes"]:
            msg = f"{evidence.escape(name)}/SKILL.md: {size} bytes, over {rules['skill_md_max_bytes']}"
            if rules["enforce_budgets"]:
                fails.append(msg)
            else:
                flags.append(msg + " (guidance)")
        text = skill_md.read_text(encoding="utf-8", errors="replace")
        keys = sorted(frontmatter.parse(text))
        if keys != rules["frontmatter_keys"]:
            fails.append(f"{evidence.escape(name)}/SKILL.md: frontmatter keys {evidence.escape(', '.join(keys))};"
                         f" only {' and '.join(rules['frontmatter_keys'])} are allowed")
    total = sum(p.stat().st_size for _, p in entries if p.is_file() and not p.is_symlink())
    if total > rules["folder_max_bytes"]:
        msg = f"{evidence.escape(name)}: folder is {total} bytes, over {rules['folder_max_bytes']}"
        if not rules["enforce_budgets"]:
            flags.append(msg + " (guidance)")
        elif budget_exempt:
            flags.append(msg + f"; budget_exempt: {evidence.escape(budget_exempt)}")
        else:
            fails.append(msg + " and has no budget_exempt reason")
    return fails, flags


# ---- mode detection (spec 8.1) ----------------------------------------------

MARKERS = ("skills/copied.json", "teammates/_template.md", "crates/horch-core/tests/skills_catalog.rs")


def git_toplevel(cwd: Path) -> Path | None:
    """`git rev-parse --show-toplevel` in `cwd`, or None outside a git work tree."""
    try:
        r = subprocess.run(["git", "rev-parse", "--show-toplevel"], cwd=str(cwd), capture_output=True,
                           text=True, timeout=30, stdin=subprocess.DEVNULL)
    except (OSError, subprocess.SubprocessError):
        return None
    top = r.stdout.strip()
    return Path(top) if r.returncode == 0 and top else None


def is_multi_herdr(root: Path) -> bool:
    return all((root / m).is_file() and not (root / m).is_symlink() for m in MARKERS)


def detect(cwd: Path, flag: str | None = None) -> dict:
    """{"target", "repo", "rule"}: the 8.1 rule run in `cwd`. `flag` (--target) overrides it;
    `rule` says which rule decided. A linked worktree is its own top level, so it is repo mode."""
    top = git_toplevel(cwd)
    if flag is not None:
        return {"target": flag, "repo": str(top) if top else None, "rule": f"flag: --target {flag}"}
    if top is None:
        return {"target": "standalone", "repo": None, "rule": "detected: not a git work tree"}
    missing = [m for m in MARKERS if not ((top / m).is_file() and not (top / m).is_symlink())]
    if missing:
        return {"target": "standalone", "repo": str(top),
                "rule": f"detected: {missing[0]} is not a regular file in the git top level"}
    return {"target": "repo", "repo": str(top), "rule": "detected: the git top level holds the 3 marker files"}
