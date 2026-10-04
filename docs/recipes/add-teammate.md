# Recipe: add a teammate

## When

You want the orchestrator to be able to spawn a new persona: a markdown file
in `teammates/` with launch settings in its frontmatter and the prompt in its
body.

## Before you start

- `teammates/README.md`, sections "Rules" and "Adding a specialist".
- `teammates/_template.md`: every field, with a comment that says what it
  does per harness.
- `ai_docs/reports/design-skills/design-personas.md`: the last 6 teammates
  added, with the test changes they needed.

In the steps, `<name>` is the teammate id, for example `design-critic`.

## Steps

1. Scaffold the file: `just teammate-new <name>` (it sets
   `HORCH_TEAMMATES_DIR` to the repo's `teammates/`), or
   `horch teammates --new <name> --dir teammates`. It copies
   `teammates/_template.md` to `teammates/<name>.md` and sets `name:`.
   Check: the file exists and `name:` equals the file name.
2. Edit the frontmatter. Write `brief_description` first: 1 line, at most
   120 characters, "when would I reach for this?". It is the only field the
   orchestrator reads. Set `agent`, `model`, `effort`, `phase`, `skills`,
   `permission_mode` and tools. Drop `generic:` for a specialist. Do not use
   `fable` or `gpt-6-astra`: the top tier is reserved for the orchestrator.
   A spawnable `agent: claude` teammate needs `disallowed_tools: [Agent]`.
   Do not put a colon followed by a space inside `brief_description`; it
   breaks the YAML.
   A domain teammate (Unreal, Swift) sets `offer_when` to name globs, for
   example `offer_when: ["*.uproject"]`. The orchestrator is then offered it
   only when the project has a matching file or directory at the top level
   or 1 level down. `horch spawn <name>` still works in any project. If it
   needs Xcode, also set `requires: [xcode]` so `horch doctor` checks
   `xcodebuild`. Leave both out for a general teammate.
   Check: `horch teammates` prints an "offered when the project has" line
   under it.
3. Write the body: the persona only. Do not restate the `horch tell` /
   `horch note` / `horch done` protocol; `teammates/_base/fleet-worker.md`
   owns it.
4. Do not register it in Rust. `crates/horch-core/build.rs` globs
   `teammates/`. Check: `cargo build --workspace --bins` and then
   `./target/debug/horch teammates --check` exits 0, and
   `./target/debug/horch teammates --matrix` shows a row for `<name>`.
5. Tell the tests about it. All 3 are needed or the gate fails:
   - Add `<name>` to `SKIP_NEW_TEAMMATES` in
     `crates/horch-core/tests/baseline_oracles.rs` and
     `crates/horch-core/tests/skills_catalog.rs` (alphabetical, 1 name per
     line). Do not create oracle files for it:
     `arc_01_baseline_oracles_present` counts exactly 33 launch oracles.
   - In `crates/horch-core/src/roster/validation.rs`, test
     `builtin_phase_defaults_are_portable_and_disabling_conflicts`: add
     `<name>` to the arm of its phase, unless its phase is `implementation`
     (the default arm). A teammate with no `phase` goes in the `None` arm.
   - In the same file, test
     `roster_check_demands_the_subagent_deny_on_every_claude_fleet_pane`:
     raise the count by 1 for a spawnable `agent: claude` teammate.
   Check: `cargo test -p horch-core --test baseline_oracles`,
   `cargo test -p horch-core --test skills_catalog`,
   `cargo test -p horch-core roster::`.
6. Add the teammate to the team tables in `teammates/README.md` and, for a
   commonly used role, to the teammate table in `README.md`. If the
   orchestrator needs to know when to use it beyond the `brief_description`,
   add a paragraph to `skills/orchestrate/SKILL.md` (D07 added section 9,
   "Design work"). Check: read them once.
7. Run the gate: `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate`.

## Files this recipe touches

| file | change | required or optional |
|---|---|---|
| `teammates/<name>.md` | the teammate | required |
| `crates/horch-core/tests/baseline_oracles.rs` | `SKIP_NEW_TEAMMATES` | required |
| `crates/horch-core/tests/skills_catalog.rs` | `SKIP_NEW_TEAMMATES` | required |
| `crates/horch-core/src/roster/validation.rs` | phase-defaults arm; Claude fleet-pane count | required when not implementation phase; required for a spawnable Claude teammate |
| `teammates/README.md` | team table | required |
| `README.md` | teammate table | optional |
| `skills/orchestrate/SKILL.md` | when to pick it | optional |

## Tests and oracles

- No oracle file is added for a new teammate. The oracle loops skip names in
  `SKIP_NEW_TEAMMATES` that have no oracle file.
- Changing an EXISTING teammate (A0 roster) changes its oracles:
  `crates/horch-core/tests/oracles/launch/<name>.json` and
  `crates/horch-core/tests/oracles/skills/<name>-*.txt`. Change them only
  when your plan says so, bless 1 test at a time (`oracle_launch_matches`,
  `oracle_skills_match`), and report the diff (see
  [testing-and-gates.md](../testing-and-gates.md#horch_bless)).
- Editing `skills/orchestrate/SKILL.md` changes `skill_file_bytes` in
  `crates/horch/tests/oracles/skills/skills-json.txt`.

## Gotchas

- A teammate on a harness with no skill exposure (`antigravity`) must have no
  `phase` and no `skills`; the spawn refuses it otherwise.
- `trains_on_input: true` is required on a teammate whose provider trains on
  what it is sent. It adds a warning block to the worker's briefing.
- The `orchestrate` and `skill-creator` skills and `remote_control: true` are
  for the orchestrator only; `horch teammates --check` refuses them on any
  other teammate.

## Worked example

`git show --stat 841f434` (D07) added 6 design personas: 6 teammate files,
`SKIP_NEW_TEAMMATES` in 2 test files, the phase arms in
`roster/validation.rs`, the README team table and section 9 of
`skills/orchestrate/SKILL.md`. It also attached skills to 2 existing
teammates (`designer`, `frontend-developer`), which re-blessed their oracles
on purpose.
