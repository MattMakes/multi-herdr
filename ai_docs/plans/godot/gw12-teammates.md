# GW12 godot-teammates: the 19 Godot teammates, docs rows, section intro

Single-branch work: follow `ai_docs/plans/finish/01-single-branch.md`.
Read `ai_docs/reports/godot-wave.md` "Teammates" in full; it is the spec.
Precedent: the 11 `teammates/ue-*.md` files (read `ue-gameplay-engineer.md`,
`ue-code-reviewer.md`, `ue-tech-lead.md` first) and `blender-artist.md`.

## GOAL

19 teammate files `teammates/godot-*.md` (waves 1, 2 and 3), each with the
frontmatter the report lists (`base: fleet-worker`, `agent: claude`,
`permission_mode: auto`, `inherit_plugins: false`, `mcp_servers: {}`,
`disallowed_tools: [Agent]` — the code reviewer also denies Edit, Write,
NotebookEdit — `disabled_skills: [herdr-orchestrator, herdr-worker]`,
`offer_when: ["project.godot"]`, `requires: [godot]`), the phase, model,
effort, `skills:` and `available_skills:` from the tables, a
`brief_description` under the roster's length rule, and a persona body that
states the 6 rules in "What every persona must say" in the teammate's own
role words. Fallbacks: the plan and review seats name `codex-sol`; builders
and QA have none (say why in a comment, as the report does).

C#: GW11 added `skills_when` (`ai_docs/reports/godot/gw11-godot-code.md`).
Every builder gets `skills_when: {"*.csproj": [godot-csharp-godot,
godot-csharp-signals]}` instead of a persona line. Read GW11's report for the
exact glob rules.

`godot-release-engineer` never uploads (no `butler push`, no store upload),
like `app-release-preparer`: it ends at a plan for the orchestrator.

## ALSO

- `teammates/README.md`: the roster table rows and a Godot paragraph, as
  for UE.
- `docs/skills-and-teams.md`: the Godot row next to the Unreal Engine row.
- `skills/README.md` "Godot skills" section intro: one paragraph that says
  what the section holds (GodotPrompter-only, combined, gd-agentic own text,
  own skills; the own-text rule; the 4.7 API checks). Fold in the intro
  texts the units left: `.worktrees/_scratch/*/readme-intro.md` and
  `.worktrees/_scratch/*/unit*/readme-intro.md`. Edit `skills/README.md` only
  while you hold the skill lock (`.worktrees/skill-lock.sh acquire <role>`),
  and only when `git diff -- skills/README.md` is empty; commit and release
  at once.

## ORDER

Write the 19 files now. `horch teammates --check` fails until every skill a
teammate names is committed: run `git log --oneline -- skills/` and
`ls skills | grep godot- | wc -l` (64 expected) to see the progress. Commit a
teammate only when all its skills are committed and `--check` passes for it.
Commit wave 1 first. Use the built binary: `cargo build -p horch` then
`HORCH_TEAMMATES_DIR=$PWD/teammates target/debug/horch teammates --check`.

## CHECKS

`--check`, the roster tests (`cargo test -p horch-core --lib roster`), the
template-fields test, and `horch teammates --matrix` showing each seat with
its skills. Then one smoke spawn: in a scratch Godot 4.7.2 project under
`.worktrees/_scratch/godot-<role>/smoke/` (created headless, with a
`project.godot`), confirm `horch teammates` offers the Godot seats there and
not in this repo. Do not run a paid worker for it.

## FILES

own: `teammates/godot-*.md`, `teammates/README.md` (Godot rows and
paragraph), `docs/skills-and-teams.md` (Godot row), the `skills/README.md`
section intro (under the lock), `ai_docs/reports/godot/gw12-teammates.md`.

## REPORT

`horch tell orchestrator "NOTE: COMMITTED <sha>..."` per commit, then
`horch done` with the summary and the seats left uncommitted, if any.
