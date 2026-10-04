#!/usr/bin/env python3
"""Copy GodotPrompter skills into this catalog under `godot-*` names.

Usage:
    scripts/godot/rename.py <upstream skills dir> <skill> [<skill>...] --out skills/
    scripts/godot/rename.py ... --dry-run     # print the diff, write nothing
    scripts/godot/rename.py ... --json        # print provenance `sources` entries

For each named skill the script:
- copies `<upstream skills dir>/<skill>/` (minus dotfiles) to
  `<out>/godot-<skill>/` (a name that already starts with `godot-` is kept);
- sets the frontmatter `name:` to the new name and never touches
  `description:` or any other frontmatter line;
- rewrites references to every upstream skill (the full upstream list is the
  map, not only the skills you copy) in the body of every copied `.md` file:
    godot-prompter:<x>       -> godot-<x>   (godot-prompter:* -> godot-*)
    **<x>**                  -> **godot-<x>**
    `<x>`                    -> `godot-<x>`
    skills/<x>/              -> skills/godot-<x>/
    "<x> skill"              -> "godot-<x> skill"
    an indented "<x>/" line  -> "godot-<x>/"   (directory trees)
- copies the upstream `LICENSE` (the parent of the skills dir) next to
  `SKILL.md`;
- prints `sha256  <repo-relative path>` for each upstream file it copied.

A target directory that exists is an error unless you pass `--force`; with
`--force` the copied files are overwritten and other files (your own
references) are left alone.
"""

from __future__ import annotations

import argparse
import difflib
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path

DEFAULT_REPOSITORY = "https://github.com/jame581/GodotPrompter"


def new_name(name: str) -> str:
    return name if name.startswith("godot-") else f"godot-{name}"


def upstream_skills(skills_dir: Path) -> list[str]:
    return sorted(
        p.name
        for p in skills_dir.iterdir()
        if p.is_dir() and not p.name.startswith(".") and (p / "SKILL.md").is_file()
    )


def build_rewriter(names: list[str]):
    """Return a function that rewrites skill references in markdown text."""
    # Longest first, so a short name never matches inside a longer one.
    alt = "|".join(re.escape(n) for n in sorted(names, key=len, reverse=True))
    end = r"(?![-\w])"
    start = r"(?<![-\w/.])"
    combined = re.compile(
        "|".join([
            r"(?P<star>godot-prompter:\*)",
            rf"godot-prompter:(?P<plugin>{alt}){end}",
            rf"\*\*(?P<bold>{alt})\*\*",
            rf"`(?P<code>{alt})`",
            rf"(?<![-\w.])skills/(?P<path>{alt})/",
            rf"{start}(?P<word>{alt}) skill\b",
            rf"^(?P<indent>[ \t]+)(?P<tree>{alt})/[ \t]*$",
        ]),
        re.MULTILINE,
    )

    def one(m: re.Match) -> str:
        if m["star"]:
            return "godot-*"
        if m["plugin"]:
            return new_name(m["plugin"])
        if m["bold"]:
            return f"**{new_name(m['bold'])}**"
        if m["code"]:
            return f"`{new_name(m['code'])}`"
        if m["path"]:
            return f"skills/{new_name(m['path'])}/"
        if m["word"]:
            return f"{new_name(m['word'])} skill"
        return f"{m['indent']}{new_name(m['tree'])}/"

    return lambda text: combined.sub(one, text)


def split_frontmatter(text: str) -> tuple[str, str]:
    """Split `---\\n...\\n---\\n` from the body. No frontmatter: ("", text)."""
    if not text.startswith("---\n"):
        return "", text
    close = text.find("\n---\n", 4)
    if close < 0:
        return "", text
    cut = close + len("\n---\n")
    return text[:cut], text[cut:]


def set_name(frontmatter: str, name: str) -> str:
    out, n = re.subn(r"^name:.*$", f"name: {name}", frontmatter, count=1, flags=re.MULTILINE)
    if n != 1:
        raise SystemExit("rename.py: SKILL.md frontmatter has no name: line")
    return out


def transform(rel: Path, text: str, rewrite, name: str) -> str:
    if rel.suffix != ".md":
        return text
    front, body = split_frontmatter(text)
    if rel == Path("SKILL.md"):
        if not front:
            raise SystemExit("rename.py: SKILL.md has no frontmatter")
        front = set_name(front, name)
    return front + rewrite(body)


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def git_revision(path: Path) -> str | None:
    try:
        out = subprocess.run(
            ["git", "-C", str(path), "rev-parse", "HEAD"],
            capture_output=True, text=True, check=True,
        )
    except (OSError, subprocess.CalledProcessError):
        return None
    return out.stdout.strip() or None


def skill_files(src: Path) -> list[Path]:
    files = []
    for p in sorted(src.rglob("*")):
        rel = p.relative_to(src)
        if any(part.startswith(".") for part in rel.parts):
            continue
        if p.is_file():
            files.append(rel)
    return files


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("skills_dir", type=Path, help="the upstream skills/ directory")
    ap.add_argument("skills", nargs="+", help="upstream skill names to copy")
    ap.add_argument("--out", type=Path, required=True, help="target skills/ directory")
    ap.add_argument("--license", type=Path, help="upstream LICENSE (default: <skills dir>/../LICENSE)")
    ap.add_argument("--dry-run", action="store_true", help="print the diff, write nothing")
    ap.add_argument("--force", action="store_true", help="overwrite copied files in an existing target")
    ap.add_argument("--json", action="store_true", help="print provenance sources entries as JSON")
    ap.add_argument("--repository", default=DEFAULT_REPOSITORY)
    ap.add_argument("--revision", help="upstream revision (default: git rev-parse HEAD)")
    args = ap.parse_args(argv)

    skills_dir: Path = args.skills_dir.resolve()
    repo_root = skills_dir.parent
    license_path = (args.license or repo_root / "LICENSE").resolve()
    if not license_path.is_file():
        print(f"rename.py: no LICENSE at {license_path}", file=sys.stderr)
        return 2
    all_names = upstream_skills(skills_dir)
    unknown = [s for s in args.skills if s not in all_names]
    if unknown:
        print(f"rename.py: not an upstream skill: {', '.join(unknown)}", file=sys.stderr)
        return 2
    revision = args.revision or git_revision(skills_dir)
    if args.json and not revision:
        print("rename.py: --json needs --revision (the skills dir is not in a git clone)", file=sys.stderr)
        return 2
    rewrite = build_rewriter(all_names)

    sources: dict[str, list[dict]] = {}
    for skill in args.skills:
        name = new_name(skill)
        src = skills_dir / skill
        dst = args.out / name
        if dst.exists() and not args.force and not args.dry_run:
            print(f"rename.py: {dst} exists (pass --force to overwrite the copied files)", file=sys.stderr)
            return 1
        entries = []
        plan = [(src / rel, rel) for rel in skill_files(src)] + [(license_path, Path("LICENSE"))]
        for upstream, rel in plan:
            upstream_rel = upstream.relative_to(repo_root).as_posix()
            digest = sha256(upstream)
            print(f"{digest}  {upstream_rel}")
            entries.append({
                "repository": args.repository,
                "revision": revision,
                "path": upstream_rel,
                "sha256": digest,
                "license": "MIT",
            })
            raw = upstream.read_bytes()
            try:
                text = raw.decode("utf-8")
            except UnicodeDecodeError:
                out_bytes = raw
            else:
                out_bytes = transform(rel, text, rewrite, name).encode("utf-8")
            target = dst / rel
            if args.dry_run:
                old = raw.decode("utf-8", errors="replace").splitlines(keepends=True)
                new = out_bytes.decode("utf-8", errors="replace").splitlines(keepends=True)
                sys.stdout.writelines(difflib.unified_diff(old, new, f"a/{upstream_rel}", f"b/{target.as_posix()}"))
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(out_bytes)
        sources[name] = entries

    if args.json:
        print(json.dumps(sources, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
