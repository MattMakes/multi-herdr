# Godot wave: conventions every Godot unit follows

Read these first, in full:
- `ai_docs/plans/finish/01-single-branch.md` (one branch, pathspec commits,
  never stage, `git show --stat HEAD` before you report COMMITTED).
- `ai_docs/reports/godot-wave.md`, above all "Operator decisions".
- `skills/README.md` "Unreal Engine skills" and the `ue-*` entries in
  `skills/provenance.json`: the precedent for shape and wording.

## Sources (read-only)

- GodotPrompter v1.14.0 (MIT):
  `.worktrees/_scratch/godot-src/GodotPrompter` at `3e8d0f005f9604e1dbdad3de693e39555384c5af`.
- gd-agentic-skills (LGPL-3.0):
  `.worktrees/_scratch/godot-src/gd-agentic-skills` at `4c4d0ff5c4597938cc9257d99d9e35f7692c9c06`.
- Never edit, commit or move these clones.

## The LGPL rule (operator decision 1)

- Copy nothing from gd-agentic-skills: no sentence, no table, no `.gd`
  script, no `.py`. Read it for facts and techniques, close it, then write
  in your own words and your own code. Do not paraphrase line by line.
- A file that uses gd-agentic facts says so in its provenance `adaptation`:
  "own text, consulted thedivergentai/gd-agentic-skills@4c4d0ff: <skill
  dirs>". gd-agentic never appears in `sources` (nothing was copied).
- GodotPrompter text (MIT) may be copied. A copied file is a `sources` entry
  with repository, revision, path, sha256 of the upstream file and
  `license: MIT`, plus the upstream `LICENSE` file next to `SKILL.md` and as
  a second `sources` entry, as the `ue-*` entries do.

## Engine and checks

- Godot 4.7.2: `/Applications/Godot.app/Contents/MacOS/Godot` (not on PATH;
  `GODOT_PATH` may be set). Headless only: always pass `--headless`. Never
  open the editor GUI.
- Each worker uses its own scratch projects under
  `.worktrees/_scratch/godot-<your role>/` (git-ignored). One Godot import
  at a time per project directory.
- Every GDScript code block you write must parse on 4.7.2, and every engine
  API it names must exist in 4.7.2. The tools (from unit GW0, opus role in
  `horch inbox`): `scripts/godot/api_check.py` (API names against the
  `--doctool` dump) and `scripts/godot/gdscript_blocks_check.py` (extracts
  ```gdscript blocks and parse-checks them). Until GW0 sends its NOTE, check
  by hand: a scratch project, the block in a `.gd` file, and a `SceneTree`
  script that `load()`s it and `quit(1)` on null. `--check-only` exits 0 on
  errors: never trust its exit code.
- C# blocks: check names against the doctool dump; a `dotnet build` is not
  required for skill text.
- The rename to `godot-*` is done by `scripts/godot/rename.py` (GW0 step 1).
  Never rename by hand.

## Shared index files: the lock

`skills/provenance.json` and `skills/README.md` are edited by many units.
Edit them only while you hold the lock, and commit them before you release:

```bash
L=/Users/mascott/projects/multi-herdr/.worktrees/skill-lock.sh
$L acquire <your role>          # waits if another unit holds it
# edit skills/provenance.json (entries sorted by name) and skills/README.md
# (rows sorted by the first column, one row per skill, in the "Godot skills"
# section; the first unit creates that section after "Unreal Engine skills")
cargo test -q -p horch-core --test skills_catalog
git commit -m "..." -- skills/provenance.json skills/README.md skills/<your dirs>
git show --stat HEAD
$L release <your role>
```

Do all slow work (writing, checking) before you acquire. Hold the lock for
minutes. If `skills_catalog` fails on something not yours, release, and tell
the orchestrator.

## Every skill

- Directory `skills/godot-<name>/` with `SKILL.md` (frontmatter `name:` equal
  to the directory, `description:` ≤ 1024 bytes) and optional `references/`.
  Text files only (the `skills_bundled_text_only` test).
- `SKILL.md` ≤ 12 KB unless the GW0 size exemption covers it (a renamed
  upstream SKILL.md with additions only under `references/`).
- Skills belong to no phase; teammates attach them by name (unit GW12).
- Address the agent as a fleet worker: no "ask the user"; a decision goes
  to the orchestrator as one `QUESTION:`.
- Write in plain, short sentences. Name the Godot version (4.7) where an API
  is new or changed.

## Report

`horch tell orchestrator "NOTE: COMMITTED <sha>..."` after each commit, a
report in `ai_docs/reports/godot/<unit>.md` (what, sources consulted, API
check output summary, anything dropped and why), then `horch done` with the
summary.
