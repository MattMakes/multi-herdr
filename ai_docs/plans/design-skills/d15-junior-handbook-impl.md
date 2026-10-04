# D15 junior-handbook: implementation plan

Unit plan: `ai_docs/plans/design-skills/d15-junior-handbook.md` (read it
first). Conventions: `ai_docs/plans/design-skills/00-conventions.md`.
Branch `ds/junior-handbook`, worktree `/Users/mascott/projects/mh-wt/junior-handbook`.

This plan turns the unit plan into tasks with exact sources, file lists and
checks. Facts marked **(seen)** were read in the source on 2026-10-03 at
`dba943a`. The executor must re-check each one before writing it into a page.
A fact that does not hold any more is corrected in the page, not copied.

## 1. Goal, scope, invariants

Goal: a junior engineer can work from `docs/` alone (see the unit plan GOAL).

Scope (the only files this unit writes):

- `docs/README.md`, `docs/architecture.md`, `docs/command-flow.md`,
  `docs/testing-and-gates.md`, `docs/fleet-workflow.md`
- `docs/recipes/add-harness.md`, `docs/recipes/add-skill.md`,
  `docs/recipes/add-teammate.md`, `docs/recipes/add-command.md`,
  `docs/recipes/add-dataset-event.md`
- `README.md`: 1 added link to `docs/README.md`. No other change.
- `ai_docs/reports/design-skills/junior-handbook.md`

Invariants:

- INV-1: No change to code, tests, oracles, goldens, `teammates/`, `skills/`,
  `docs/phase-skills.md`, `docs/runtime-skill-checks.md`. `git diff --stat design-skills`
  shows only the files in the scope list.
- INV-2: The gate stays green: `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate`.
- INV-3: Every path, command, flag, test name and file in a page exists at the
  commit you merge. No page describes a step you did not confirm in source.
- INV-4: Pages state the `ANTHROPIC_API_KEY` prohibition only. No page shows
  the variable set, exported or passed. Nested `claude` examples use
  `env -u ANTHROPIC_API_KEY claude ...`.
- INV-5: Style: match `docs/phase-skills.md` and `README.md`: short plain
  sentences, tables for parallel facts, relative links. One page per concern.
  Target length: each page under about 200 lines; a recipe under about 120.

## 2. Sources to read (in this order)

1. `README.md` sections "Layout", "Architecture", "Tests", "Six harnesses,
   one roster", "Teammates", "Skills for each phase".
2. `crates/horch-core/src/lib.rs` module table (seen: 20 modules, `ids` to
   `teacher`).
3. `ai_docs/designs/2026-10-02-architecture-refactor-design.md`,
   `ai_docs/designs/2026-10-02-dataset-competition-design.md`.
4. Reports: `ai_docs/reports/arch-refactor-dataset/*.md` and
   `ai_docs/reports/design-skills/*.md`. The most useful for recipes:
   `antigravity-harness.md` (harness), `agent-list.md` (command),
   `design-personas.md` (teammate), `skills-infra.md` and `skill-taste.md`
   (skill), `b1-measure.md`, `b3-coordinator.md`, `b5-promotion.md` (events),
   `followups.md`, `a0-gate.md`, `a0-oracles.md` (gate and test gotchas).
5. `teammates/README.md`, `teammates/_template.md`, `skills/README.md`,
   `ai_docs/plans/design-skills/01-skill-authoring.md`.
6. The operator's flowchart `/Users/mascott/projects/multi-herdr/ai_docs/command-flow.md`
   (157 lines, 5 Mermaid diagrams, 1 table). Read-only: copy, do not move.

Merge commits that show a full recipe in one diff (use `git diff --stat <m>^1 <m>`):

| recipe | merge | what it added |
|---|---|---|
| add-harness | `81e6b33` (D08) | Antigravity harness |
| add-command | `b0c241b` (D09) | `horch agent-list` |
| add-teammate | `841f434` (D07) | 7 design personas |
| add-skill | `e4b0262` (D01), `9d7fa5a` (D00) | `ui-taste`, `ui-redesign`; catalog infra |
| add-dataset-event | commits `7ccc935`, `56f5f18`, `d77e93a` | round state table; `operator.promote` |
| harness follow-ups | `4b62d9e` (D10) | `FAKES` gains `agy`, preflight kinds, pools |

## 3. Tasks

Tasks T1 to T6 are independent after T0. T7 depends on T1 to T6. T8 depends on T7.

### T0. Worktree

```
git -C /Users/mascott/projects/multi-herdr worktree add -b ds/junior-handbook /Users/mascott/projects/mh-wt/junior-handbook design-skills
cd /Users/mascott/projects/mh-wt/junior-handbook
cargo build --workspace --bins
```

Check: `git -C /Users/mascott/projects/mh-wt/junior-handbook branch --show-current` prints `ds/junior-handbook`.
The build is needed for the `horch --help` checks in T7 and for the gate.

### T1. `docs/architecture.md` (JH-02)

Purpose: what horch and multi-herdr-dataset do, and where each concern lives.

Content:

- 2 binaries over 1 core: `crates/horch/src/main.rs` (horch) and
  `crates/horch/src/bin/multi-herdr-dataset.rs` (seen). Crates table from
  README "Layout" (6 crates, seen).
- The rules: `bootstrap.rs` reads env once into `RuntimeContext`; exit codes
  from error type (`crates/horch/src/exit.rs`); no re-export shims. Name the
  arch-scan tests that enforce them: `arc_05_no_ambient_env_in_core`,
  `arc_22_no_error_string_matching`, `arc_25_no_shim_modules`. Confirm each with
  `grep -n "fn arc_" crates/horch-core/tests/arch_scan.rs` and list every
  `arc_*` rule that a junior can break by accident.
- Module table: "module | owns | start reading at". Use `lib.rs` as the
  source, plus 1 entry file per module (for example `harness/mod.rs`,
  `skills/selection.rs`, `measure/event.rs`, `competition/coordinator.rs`,
  `execution/service.rs`). Include the 6 harness kinds, Antigravity included
  (seen: `harness/antigravity.rs` exists; the `lib.rs` and README tables omit
  it; see section 6).
- CLI layer map: `crates/horch/src/cmd/*.rs` (20 files, seen) and
  `crates/horch/src/dataset/*.rs` (14 files, seen), 1 line each.
- Where state lives: state root `~/.local/state/horch` or `HORCH_STATE_DIR`
  (`runtime/paths.rs` `state_root`, seen); ledger `<state root>/<project slug>.json`
  (`execution/store.rs:278`, seen); dataset root `<state root>/multi-herdr/<slug>/`
  with `events/YYYY-MM-DD.jsonl` (`measure/paths.rs`, `DATASET_DIR`, seen);
  data root `~/.local/share/horch` for marketplace skills (seen); skill
  bundles under `<state root>/skill-bundles` (seen in `docs/phase-skills.md`).
- How `teammates/` and `skills/` get into the binary: `crates/horch-core/build.rs`
  globs both (seen). The runtime overlay order (`cmd/mod.rs` `load_roster`
  comment, seen): built-ins, `~/.config/horch/teammates`, `$HORCH_TEAMMATES_DIR`,
  explicit dir.
- Links: designs, `command-flow.md`, recipes.

Check: every module in `lib.rs` appears once in the table:
`grep -oE '^pub mod [a-z_]+' crates/horch-core/src/lib.rs | awk '{print $3}' | while read m; do grep -q "\`$m\`" docs/architecture.md || echo "missing $m"; done` prints nothing.

### T2. `docs/command-flow.md` (JH-03)

Purpose: the operator's flowchart, corrected and complete.

Steps:

1. `cp /Users/mascott/projects/multi-herdr/ai_docs/command-flow.md docs/command-flow.md`.
2. Check every claim against code. Write one row per claim into a table in
   the report: claim, source file:line, verdict (kept, fixed, removed). Claims
   to check (not complete; check every arrow and table cell):
   - Subcommand names and flags: `horch --help`, `multi-herdr-dataset --help`,
     and each `--help` of a named subcommand. Source: `crates/horch/src/main.rs`,
     `crates/horch/src/dataset/cli.rs`.
   - Ledger path `state dir/<project>.json` (seen correct, slug of the path).
   - Events path `state dir/multi-herdr/<project>/events` (seen correct, slug).
   - Spawn service stages "plan, record, brief, pane, launch":
     `crates/horch-core/src/execution/service.rs`, `cmd/spawn.rs`.
   - Exit codes: horch 0 to 3 in `crates/horch/src/exit.rs` (seen; REFUSED = 3);
     dataset 4, 5, 6 in `crates/horch/src/dataset/mod.rs:39-43` (seen:
     PREFLIGHT_FAILED, NEEDS_INTERVENTION, REJECTED).
   - Preflight checks list: `crates/horch-core/src/competition/preflight.rs`.
   - Round states and transitions: `crates/horch-core/src/competition/state.rs` `TABLE`.
   - Judge runs `claude -p` with read-only tools: `crates/horch/src/dataset/judge_job.rs`,
     `crates/horch-core/src/competition/judging.rs`.
   - `horch sessions` hides candidates and judge unless `--all` (main.rs:94, seen).
   - `horch cost` harness coverage. Seen: the doc comment says claude, codex,
     pi, prime. Antigravity has no usage reader (`antigravity-harness.md`).
   - The "Together or apart" table: which commands need herdr. Confirm by the
     command's code path (does it build a herdr client?).
3. Add `horch agent-list` (main.rs:207, `cmd/agentlist.rs`, flags `--json`,
   `--no-probe`; seen). Put it in setup (diagram 1) and in the table: no
   herdr, no fleet, no model money; it runs each harness `--version` unless
   `--no-probe` (confirm in `harness/inventory.rs`).
4. Add Antigravity: an orchestrator can spawn an `antigravity` worker (binary
   `agy`, pool `google`). Add the pool list where `horch quota` appears
   (seen: `POOLS` = claude, codex, opencode-zen, google, local in
   `routing/quota.rs:44`; the `Quota` doc comment in main.rs:217 agrees).
5. Keep the operator's structure and voice. Fix only what is wrong or missing.
   Escape `<` as `&lt;` inside Mermaid labels, as the original does.

Check: each Mermaid block is fenced with ```` ```mermaid ```` and has matching
quotes. If `npx` is not allowed (conventions §2 forbids package managers for
sources; do not install `mmdc`), check by eye that every node label is quoted
and that every `subgraph` has an `end`. State in the report that the diagrams
were not rendered locally.

### T3. `docs/testing-and-gates.md` (JH-04)

Content, each with its source:

- The gate: `just gate` runs `scripts/phase-gate.sh` (seen). List its 8 steps
  in order: `cargo fmt --all --check`, `cargo build --workspace --all-targets`,
  `cargo build --workspace --bins`, `cargo test --workspace --no-fail-fast`,
  `teammates --check`, `scripts/check-req-coverage.sh`, `scripts/check-deps.sh`,
  `scripts/verify-telemetry-e2e.sh`. Explain `HORCH_REQUIRE_GIT=1` and
  `HORCH_REQUIRE_SQLITE=1` (grep the tests for both and state what they turn
  from skip into fail). `just verify` versus `just gate` (justfile, seen).
- How to read a failure: one line per step on what a failure means and the
  first file to open.
- Requirement coverage: an ID in a design table needs a test named with the
  lowercase ID prefix (`TEL-05` to `tel_05_...`); phases from
  `ai_docs/gates/architecture-refactor/CURRENT_PHASE` (script header, seen).
- Dependencies: allowed crate lists in `scripts/check-deps.sh` (seen). No new
  crates, no HTTP crate, no async runtime (conventions §6).
- Oracles and goldens: locations `crates/horch-core/tests/oracles/`
  (`launch`, `ledgers`, `routing`, `skills`), `crates/horch-core/tests/golden/`,
  `crates/horch/tests/oracles/`, `crates/horch/tests/golden/`,
  `crates/horch-e2e/tests/golden/` (seen). Oracles are frozen A0 behavior
  (`baseline_oracles.rs` header, seen).
- `HORCH_BLESS=1` rules. Two behaviors exist (seen): `baseline_oracles.rs`
  overwrites; `dataset_export.rs`, `measure.rs` and the e2e scenario write
  only an absent file. Check each of the 7 files that read `HORCH_BLESS`
  (`grep -rln HORCH_BLESS crates`) and document which kind each is. Rules from
  conventions §5: a new teammate may get new oracle files; an existing
  teammate's oracle changes only when the plan says so; check `git status`
  after blessing. The current run's alternative: `SKIP_NEW_TEAMMATES` in
  `tests/baseline_oracles.rs:70` and `tests/skills_catalog.rs:79` (seen).
- Hermetic tests: no network, no real harness binary, no herdr server. The
  fakes: `crates/horch-e2e/src/bin/fake-*.rs` (8, seen) and `FAKES` in
  `crates/horch-e2e/src/harness.rs:19` (seen, includes `agy`). `HORCH_NOW`
  pins the clock (`clock.rs`, seen in `lib.rs`).
- e2e gotchas (from `followups.md`, `session-discovery-race.md`, `a0-gate.md`, seen):
  - Run `cargo build --workspace --bins` before `cargo test -p horch-e2e`.
    The e2e tests do not rebuild `horch`.
  - Never write into a harness `bin/` entry. It can be a hard link to
    `target/debug/fake-*`. Use `Harness::write_bin`. `check_built_fakes` catches it.
    After a mistake, delete the damaged `target/debug/fake-*` and rebuild.
  - `HORCH_E2E_KEEP=1` keeps a temp dir. Judge bundle dirs are mode 0500:
    `chmod -R u+w` before you delete them.
  - macOS kills a copied system binary (exit 137). Symlink it.
  - `sqlite3` reads a leading `-` argument as an option. Feed SQL on stdin.
  - Use your own `CARGO_TARGET_DIR` per worktree.
- Live checks: `horch smoke messaging|fleet|tile` need a herdr server; not in
  the gate. `just verify-perf` is slow and not in the gate (justfile, seen).

Check: the 8 gate steps in the page match `grep '^step ' scripts/phase-gate.sh`.

### T4. Recipes (JH-05 to JH-09)

Format for every recipe (same headings in all 5):

1. "When": one sentence.
2. "Before you start": what to read (1 to 3 links).
3. "Steps": numbered. Each step names the exact file, the change, and the
   check (a test name or a command).
4. "Files this recipe touches": a table "file | change | required or optional".
5. "Tests and oracles": which tests must exist, which oracles or goldens can
   change, and the bless rule that applies.
6. "Worked example": the merge commit from section 2 and `git show --stat` of it.

Walk each recipe against the code: open every file in the table and confirm
the edit point (the enum, match, list or table). Record each confirmation in
the report as `recipe: file:line - what is there`.

#### T4a. `docs/recipes/add-harness.md` (JH-05)

Start list (from the D08 diff and the D10 follow-ups, seen). Confirm each:

- `crates/horch-core/src/harness/<name>.rs`: the adapter (argv, resume,
  session discovery, permission mapping, forbidden env, unit tests).
- `harness/mod.rs`: `HarnessKind` variant, `HarnessKind::ALL`, name and parse,
  the legacy table; test `all_names_every_kind`.
- `harness/capabilities.rs`, `harness/launch.rs` (match arm),
  `harness/inventory.rs` (does `agent-list` derive from `ALL`? confirm).
- `roster/permission.rs` (permission-mode mapping), `roster/validation.rs`
  (per-harness env rule; `builtin_phase_defaults_...` test).
- `routing/quota.rs` `pool_for` (and `POOLS` only for a new pool).
- `runtime/bins.rs` (`HORCH_<NAME>_BIN` override), `runtime/context.rs`
  (the override-count test).
- `crates/horch/src/dataset/preflight.rs` `kind_of`: seen to iterate
  `HarnessKind::ALL` since D10 (line 131), so no edit. Name its test (line 425).
- Optional: `telemetry/readers.rs` and `usage` for `horch cost`.
- `crates/horch-e2e/src/bin/fake-<name>.rs`, `crates/horch-e2e/Cargo.toml`
  (`[[bin]]`), `crates/horch-e2e/src/harness.rs` `FAKES`,
  `crates/horch-e2e/tests/lifecycle.rs` (`arc_26_e2e_lifecycle_matrix_<name>`),
  `crates/horch-e2e/tests/skills_exposure.rs` (`skl_06_e2e_exposure_<name>`).
- `teammates/<name>.md`, `teammates/README.md`, `teammates/_template.md`
  (harness list), `SKIP_NEW_TEAMMATES` in the 2 test files.
- `README.md` harness table and its count heading.

Include the gotchas from `antigravity-harness.md`: forbidden env per adapter
versus global `FORBIDDEN_ENV`; a harness with `SkillExposure::None` must not
get a `phase`; verify against a fake only and state what is unverified.

#### T4b. `docs/recipes/add-skill.md` (JH-06)

Confirm each:

- `skills/<id>/SKILL.md` with `name` and `description` frontmatter
  (seen in `skills/ui-taste/SKILL.md`), optional `references/`.
- `crates/horch-core/build.rs` embeds every file under `skills/` (seen):
  no Rust registration for a bundled skill.
- `skills/README.md` row (alphabetical, conventions §4) and
  `skills/provenance.json` entry; repo-original skills have no sources.
- Phase catalog: `crates/horch-core/src/skills/selection.rs` `phase_skills`
  (seen line 12). Or attach by name in a teammate's `skills:` list.
- Tests: `crates/horch-core/tests/skills_catalog.rs` (provenance pins,
  `DESIGN_SOURCE_PINS`, digests); oracles `tests/oracles/skills/*.txt` and
  `crates/horch/tests/oracles/skills/skills-json.txt` change when a phase
  catalog or a teammate's skills change (D07 re-blessed them on purpose; see
  `design-personas.md`).
- Checks: `horch skills show <id>`, `horch skills --phase <p> --json`,
  `horch teammates --check`.
- Authoring rules: link `ai_docs/plans/design-skills/01-skill-authoring.md`.

#### T4c. `docs/recipes/add-teammate.md` (JH-07)

Confirm each:

- `horch teammates --new <NAME>` or `just teammate-new NAME` scaffolds from
  `teammates/_template.md` (main.rs:112, justfile; seen).
- Fields from `_template.md`; rules in `teammates/README.md` "Rules" and
  "Adding a specialist" (seen headings).
- `build.rs` globs `teammates/` (seen): no Rust registration.
- `horch teammates --check`, `horch teammates --matrix`.
- Oracles: either new oracle files with `HORCH_BLESS=1` (only that teammate's
  files) or `SKIP_NEW_TEAMMATES`. The counts test is
  `arc_01_baseline_oracles_present` (`tests/baseline_oracles.rs:600`, seen).
- No `phase` for a harness without skill exposure; then the
  `builtin_phase_defaults_...` test in `roster/validation.rs` lists it.
- If the orchestrator must know the teammate: `skills/orchestrate/SKILL.md`
  (D07 changed it) and `teammates/README.md` team tables.

#### T4d. `docs/recipes/add-command.md` (JH-08)

Confirm each (D09 diff, seen):

- `crates/horch/src/main.rs`: a `Command` variant with doc comments (they are
  the `--help` text) and a dispatch arm (seen line 488 for `AgentList`).
- `crates/horch/src/cmd/<name>.rs` and `cmd/mod.rs` `pub mod` line.
- Domain logic in the owning `horch-core` module (D09:
  `harness/inventory.rs`), not in the CLI crate.
- Exit codes from `crates/horch/src/exit.rs`; output helpers in
  `crates/horch/src/output.rs`.
- Tests: `crates/horch/tests/<name>.rs` that runs the built binary
  (`CARGO_BIN_EXE_horch`; confirm in `tests/agent_list.rs`).
- The arch rules: core reads no env; no error-string matching; no shims.
- A dataset subcommand instead goes in `crates/horch/src/dataset/cli.rs` and
  `dataset/<name>.rs`, tests in `crates/horch/tests/dataset_cli.rs`. Confirm.
- Docs: `README.md`, `docs/command-flow.md`; `skills/orchestrate/SKILL.md` and
  `teammates/_base/*.md` if agents run the command (golden prompts then change:
  `tests/golden_prompts.rs`).

#### T4e. `docs/recipes/add-dataset-event.md` (JH-09)

Confirm each:

- `crates/horch-core/src/measure/event.rs`: a payload struct and 1 line in
  `event_kinds!` (seen). No `deny_unknown_fields`. A new field on an existing
  payload needs `#[serde(default)]` (seen on `PromotionStarted`). Unknown kinds
  survive as `EventKind::Unknown`. `EVENT_SCHEMA_VERSION` = `1.0.0` (seen):
  find and state when it changes.
- If the event moves a round's state: a `TABLE` row in
  `crates/horch-core/src/competition/state.rs`; `measure/projection.rs`
  applies it (an event with no row is an anomaly; commit `7ccc935`, seen).
- The writer: `competition/coordinator.rs` or `crates/horch/src/dataset/*.rs`
  (D-B5 example: `dataset/promote.rs`).
- Tests: `crates/horch-core/tests/measure.rs` has 1 sample per known kind,
  checked against `EventKind::KNOWN` (seen lines 155 and 444); the new kind
  must get a sample. Export golden `tests/golden/export-1.0.0.jsonl` and
  `tests/dataset_export.rs`: state whether a new kind changes export rows.
- Rebuild and resume: `multi-herdr-dataset rebuild` replays events; confirm
  the new kind replays.

### T5. `docs/fleet-workflow.md` (JH-10)

Sources: `00-conventions.md`, `STATUS.md`, any unit plan (for example `d09-agent-list.md`),
`teammates/_base/fleet-orchestrator.md`, `teammates/_base/fleet-worker.md`,
`skills/orchestrate/SKILL.md`, `docs/phase-skills.md`.

Content:

- Roles: 1 orchestrator spawns workers (`horch spawn`), talks with `horch tell`,
  `horch assign`; workers use `horch note`, `horch tell orchestrator`, `horch done`.
- Plan files: `ai_docs/plans/<run>/` with `00-conventions.md`, unit plans with
  GOAL, CONTEXT, FILES (own / do not touch), STEPS; `STATUS.md` table.
- Phases and skills: research, plan, implementation, validation (link
  `docs/phase-skills.md`).
- Worktrees: the integration worktree belongs to the orchestrator; each unit
  has `ds/<unit>` in its own worktree; own `CARGO_TARGET_DIR`; no checkout in
  the shared tree.
- Merge protocol: rebase, gate, `READY-TO-MERGE <branch> <sha>`, `REBASE` or
  `MERGED`, then `horch done`.
- Reports: `ai_docs/reports/<run>/<unit>.md`, what they hold.
- Message style: Simplified Technical English (role tag plus keyword).
- The orchestrator's merge and gate helpers in `/tmp` (STATUS.md names
  `/tmp/igate-ds.sh`, `/tmp/dsmerge.sh`) are not in the repo. Say so; do not
  document their contents.
- `ai_docs/` map: `designs/`, `plans/`, `reports/`, `gates/`, `checkpoints/`,
  `reflections/` (seen), 1 line each.

### T6. `docs/README.md` and the `README.md` link (JH-01, JH-11)

- `docs/README.md`: who it is for; a "start here" reading order
  (architecture, command-flow, testing-and-gates, fleet-workflow); the 5
  recipes; links to the 2 existing pages `phase-skills.md` and
  `runtime-skill-checks.md`; links to the designs.
- `README.md`: add 1 line with a link to `docs/README.md`. Put it under the
  first paragraph or under "Architecture". Change nothing else.

Check: `git diff design-skills -- README.md | grep -c '^[+-][^+-]'` prints `1`.

### T7. Verification (all JH)

Run from the worktree root. Each command must print nothing unless stated.

1. Relative links resolve:

   ```
   for f in docs/*.md docs/recipes/*.md; do
     grep -oE '\]\([^)#[:space:]]+' "$f" | sed 's/^](//' | grep -vE '^(https?:|mailto:)' |
     while read -r l; do [ -e "$(dirname "$f")/$l" ] || echo "$f: broken link $l"; done
   done
   ```

2. Repo paths in backticks exist (skips templates with `<`, `{`, `*`):

   ```
   grep -ohE '`(crates|teammates|skills|scripts|docs|ai_docs)/[^`]+`' docs/*.md docs/recipes/*.md |
     tr -d '`' | grep -vE '[<{*]' | sort -u | while read -r p; do [ -e "$p" ] || echo "missing path $p"; done
   ```

3. Named tests exist. List every test name the pages use in the report, then:

   ```
   for t in <names>; do grep -rqE "fn $t\b" crates || echo "no test $t"; done
   ```

   For a name pattern such as `arc_26_e2e_lifecycle_matrix_<name>`, check the
   Antigravity instance.

4. Commands exist: for each `horch <sub>` and `multi-herdr-dataset <sub>` in
   the pages, `./target/debug/horch <sub> --help >/dev/null` and
   `./target/debug/multi-herdr-dataset <sub> --help >/dev/null` exit 0.

5. Scope: `git diff --stat design-skills` lists only the scope files (INV-1).

6. INV-4: `grep -rn 'ANTHROPIC_API_KEY' docs README.md` shows only
   prohibition text or `env -u ANTHROPIC_API_KEY`.

7. Gate: `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate` ends with `GATE GREEN`.

8. Read-through: read `docs/README.md` and follow one recipe end to end as a
   junior would. Fix every step where you had to open a file the recipe did
   not name.

### T8. Commit, report, merge

- Commit the pages: `Docs: Add the junior handbook`.
- Write `ai_docs/reports/design-skills/junior-handbook.md`: pages written;
  the command-flow claim table (T2); per recipe the confirmed `file:line`
  list (T4); the T7 results; what was not verified (Mermaid rendering, real
  harness binaries); out-of-scope findings (section 6). Commit it
  (`Reports: Add the junior-handbook report`).
- Merge protocol: conventions §7. At rebase, D11 to D14 may have merged.
  Re-check the claims that they touch: D11 (`fleet-orchestrator.md` pool
  line), D12 (`execution/lifecycle.rs`, `fake-herdr`), D13 (`competition/promotion.rs`,
  `budget.rs`, `coordinator.rs`), D14 (`skills/motion-gsap`). Re-run T7.

## 4. Acceptance requirements

| ID | Requirement | Check | Task |
|---|---|---|---|
| JH-01 | `docs/README.md` indexes every page in `docs/` and `docs/recipes/` | `ls docs/*.md docs/recipes/*.md` names each appear as a link in `docs/README.md` | T6 |
| JH-02 | `architecture.md` names every `horch-core` module and its entry file | T1 check prints nothing | T1 |
| JH-03 | `command-flow.md` is the operator file, checked; includes `agent-list`, Antigravity, the `google` pool | `grep -c 'agent-list\|antigravity\|agy\|google' docs/command-flow.md` ≥ 3; claim table in report | T2 |
| JH-04 | `testing-and-gates.md` covers gate steps, oracles, goldens, bless rules, fakes, e2e gotchas | T3 check; all 6 gotchas present | T3 |
| JH-05..09 | Each recipe names exact files and a check per step, walked against code | T7.2, T7.3; per-recipe `file:line` list in report | T4 |
| JH-10 | `fleet-workflow.md` covers plans, worktrees, merge protocol, STATUS | read-through T7.8 | T5 |
| JH-11 | `README.md` gains exactly 1 link to `docs/` | T6 check prints `1` | T6 |
| JH-12 | No code, test, oracle, teammate or skill change; gate green | T7.5, T7.7 | T7 |
| JH-13 | All links, paths, tests and commands in pages exist | T7.1 to T7.4 print nothing | T7 |
| JH-14 | Report with verification evidence | file exists, committed | T8 |

## 5. Dependencies and external services

- No network, no herdr server, no harness binary. `horch --help` uses the
  built binary only.
- Read access to `/Users/mascott/projects/multi-herdr/ai_docs/command-flow.md`
  (untracked in the main checkout; do not commit or move it there).
- No credentials. INV-4 applies.

## 6. Out-of-scope findings (report them; do not fix)

Seen during planning:

- `crates/horch-core/src/lib.rs:16` and `README.md` "Architecture" table list
  harnesses as "claude, codex, OpenCode, pi, Prime". Antigravity is missing.
- `docs/phase-skills.md:3` says "The sixteen skills". `skills/` now has 24
  bundle directories.
- STATUS.md D10 open items still stand (`fleet-orchestrator.md:115` pool line,
  `horch done` pane race, fake-herdr pane id reuse) unless D11 or D12 merged.

Add any new finding from T1 to T5 to the report.

## 7. Open decisions

None block the work. Default choices (state them in the report):

- The handbook pages use plain technical prose like `README.md`, not strict STE.
  STE applies to fleet messages only.
- Recipes show file tables and checks, not full code. Code sketches only where
  a contract needs it (for example the `event_kinds!` line).

## 8. Completion criteria

- All 11 pages and the 1 README line exist on `ds/junior-handbook`.
- T7 steps 1 to 7 pass; T7.8 done.
- Report committed with the claim table and per-recipe confirmations.
- `READY-TO-MERGE` sent per conventions §7; `horch done` after `MERGED`.
