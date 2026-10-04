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
   Optional skill fields (see [Skill fields](#skill-fields)):
   `available_skills` and `operator_skills`.
   A spawnable `agent: claude` teammate needs `disallowed_tools: [Agent]`.
   Do not put a colon followed by a space inside `brief_description`; it
   breaks the YAML.
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

## Skill fields

A teammate gets skills from 4 fields. Each one has a different cost in
the worker's briefing.

| field | what the worker gets | briefing cost |
|---|---|---|
| `phase` | the phase's bundled skills | names only |
| `skills` | named bundled or marketplace skills, expected | name and description |
| `available_skills` | named bundled or marketplace skills, offered | name only, under "Also available" |
| `operator_skills` | skills from a directory on the operator's machine, expected | name and description |

- Use `available_skills` for a related skill that the persona uses
  sometimes. Each name is materialized. The harness still lists every
  materialized skill's description in its own skill list, so keep the list
  short. A name in both `skills` and `available_skills` fails `--check`.
- Use `operator_skills` for a skill that cannot ship in this repo, for
  example Apple's Xcode skills that `xcrun agent skills export` writes:

  ```yaml
  operator_skills:
    dir: ~/.agents/skills
    names: [swiftui-whats-new-27, test-modernizer]
  ```

  - `~/` expands against the launch's home (`$HOME` of the launch context).
  - Each name must be `<dir>/<name>/SKILL.md` with a matching `name:`.
  - The launch copies each directory into the skill bundle and checks the
    copy's tree digest. The plan records the version as
    `operator+<digest12>`. Every harness that exposes skills sees them.
    On Claude they appear as `horch:<name>`, like bundled skills.
  - `--check` fails on a missing `dir`, a missing name, a name that is
    named twice, and a name that a bundled or marketplace skill already
    has.
  - `--check` refuses a subagent skill: `device-interaction`, or a SKILL.md
    that contains "SUBAGENT skill" or "Agent tool". Fleet panes start no
    subagents.
  - The ledger record (`horch spawn`) does not list operator skills,
    because `plan_launch` reads no files. The launch's bundle has them.
  - A teammate with `operator_skills` passes `--check` only on a machine
    that has the directory. Do not add it to a shipped teammate unless every
    operator machine has the directory.
- A fallback launch keeps all 4 fields of the original teammate.

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
