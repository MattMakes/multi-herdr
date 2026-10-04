# Godot wave: conventions every Godot unit follows

Read these first, in full:
- `ai_docs/plans/finish/01-single-branch.md` (one branch, pathspec commits,
  never stage, `git show --stat HEAD` before you report COMMITTED).
- `ai_docs/reports/godot-wave.md`, above all "Operator decisions".
- `skills/README.md` "Unreal Engine skills" and the `ue-*` entries in
  `skills/provenance.json`: the precedent for shape and wording.

## Sources (read-only)

- GodotPrompter v1.14.0:
  `.worktrees/_scratch/godot-src/GodotPrompter` at `3e8d0f005f9604e1dbdad3de693e39555384c5af`.
- gd-agentic-skills:
  `.worktrees/_scratch/godot-src/gd-agentic-skills` at `4c4d0ff5c4597938cc9257d99d9e35f7692c9c06`.
- Never edit, commit or move these clones.

## The own-text rule (operator decision 1)

- Copy nothing from gd-agentic-skills: no sentence, no table, no `.gd`
  script, no `.py`. Read it for facts and techniques, close it, then write
  in your own words and your own code. Do not paraphrase line by line.
- A file written this way is own text: it is not in `copied_files`.
- GodotPrompter text may be copied. A copied file is listed in the
  skill's `copied_files` in `skills/copied.json`. No licence file is copied
  (operator rule, 2026-10-04).

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

`skills/provenance.json`, `skills/README.md` and, for own-text skills (empty `sources`), the `REPO_ORIGINAL` list in `crates/horch-core/tests/skills_catalog.rs` are edited by many units.
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

## Work outside skills/ (rule added during the wave)

horch reads `skills/` in every build and test, so an unfinished skill there
breaks every worker. Build and check skills under
`.worktrees/_scratch/godot-<your role>/skills/` (`rename.py --out` there).
Copy them into `skills/` only while you hold the lock, right before
`skills_catalog` and the commit. A `description:` that holds `: ` must be
quoted.

## Upstream errors (standing rule)

A real API error (api_check) or a fact error you proved on 4.7.2 (a test
run, the doctool dump, or the 4.7 docs) in upstream GodotPrompter text is
fixed in place. List it in provenance as `API fixes on 4.7.2: A -> B` or
`Fact fixes on 4.7: <what> (<evidence>)`, and in your report. A claim you
cannot prove stays as it is and goes in the report.

## Commit with godot-commit.sh (replaces the manual lock steps)

Never edit the index files by hand. Put your work in a unit directory and
run `/Users/mascott/projects/multi-herdr/.worktrees/godot-commit.sh <role>
"<message>" <unit dir> <skill>...`. The unit directory holds `skills/<skill>/`,
and as needed `provenance.json` (a JSON list of your entries),
`readme-rows.md` (your table rows), `repo-original.txt` (own-text names) and
`extra-paths.txt` (your report path). The script takes the lock, refuses to
start when an index file holds uncommitted edits, copies, merges sorted,
runs `skills_catalog`, commits only those paths, and releases. On a failure
it restores everything. The README section intro is written once, at the
end, by the teammates unit: put intro text in `readme-intro.md` instead.
