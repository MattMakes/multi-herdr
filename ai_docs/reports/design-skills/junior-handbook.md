# D15 junior-handbook: report

Unit: `junior-handbook`. Branch: `ds/junior-handbook`. Plan:
`ai_docs/plans/design-skills/d15-junior-handbook.md`, executed from
`d15-junior-handbook-impl.md` (tasks T0 to T8). Author: opus-35.

## Pages written

| page | lines | content |
|---|---|---|
| `docs/README.md` | 55 | index, reading order, recipes, reference pages, designs |
| `docs/architecture.md` | 166 | 2 binaries, 6 crates, 5 arch-scan rules, 20 modules with entry files, CLI layer map (21 `cmd/` files, 14 `dataset/` files), state paths, roster layering |
| `docs/command-flow.md` | 189 | the operator's flowchart, checked and corrected (claim table below) |
| `docs/testing-and-gates.md` | 171 | 8 gate steps, REQUIRE envs, coverage, deps, oracle and golden locations, 7 `HORCH_BLESS` readers, fakes, 6 e2e gotchas, live checks |
| `docs/fleet-workflow.md` | 124 | roles, plan files, phases, worktrees, merge protocol, reports, STE, `ai_docs/` map |
| `docs/recipes/add-harness.md` | 145 | 14 steps |
| `docs/recipes/add-skill.md` | 99 | 6 steps |
| `docs/recipes/add-teammate.md` | 106 | 7 steps |
| `docs/recipes/add-command.md` | 116 | 8 + 4 steps (`horch` and `multi-herdr-dataset`) |
| `docs/recipes/add-dataset-event.md` | 111 | 7 steps |

In-scope fixes from the plan's EXECUTION section:

- `crates/horch-core/src/lib.rs:16` and the `README.md` Architecture table:
  the `harness` row names Antigravity.
- `docs/phase-skills.md:3`: "The 24 skills" (14 process, 8 design,
  `orchestrate`, `skill-creator`); "has no sources" instead of "a null source"
  (D00 changed the provenance shape); the phase catalogs use process skills
  only, and the design skills, `orchestrate` and `skill-creator` attach by
  name.
- `README.md`: 1 line under "Architecture" links to `docs/README.md`.

## Decisions

- Pages use plain technical prose like `README.md`, not strict STE. STE is
  for fleet messages.
- Recipes give file tables and checks, not full code. The only code sketch is
  the `event_kinds!` line.
- The architecture module table gives full paths (`crates/horch-core/src/...`)
  so that T7.2 checks every entry file.
- `add-harness.md` is 145 lines, over the 120-line target. The harness
  recipe touches 18 files; I kept every step rather than split the page.
- The T6 README check prints 2, not 1: the plan's EXECUTION section added the
  Architecture table fix to the same file. The link itself is 1 added line.

## command-flow.md claim table

Source: `/Users/mascott/projects/multi-herdr/ai_docs/command-flow.md` (157
lines, read-only, not moved). Each row: claim, source, verdict.

| claim | source | verdict |
|---|---|---|
| `just install` runs `horch install`, which puts both binaries on PATH | `justfile:31-37`; `crates/horch/src/cmd/install.rs:8,82` | kept; label now names both binaries |
| `just install` writes `~/.local/bin/herdr-fleet` | `justfile:34` | kept |
| `horch doctor` checks herdr | `main.rs` `Doctor` doc | kept |
| `horch smoke` self-checks | `cmd/smoke.rs` | kept; named the 3 subcommands |
| `horch skills` / `horch marketplace` install a skill once, pinned | `cmd/skillscmd.rs` `Install` doc | kept; label now `horch skills install` |
| `horch fleet` opens the orchestrator pane and the telemetry space | `cmd/recipes.rs:294,326` | kept |
| orchestrator is Claude or Codex | `cmd/recipes.rs` `FleetFlavor` | kept |
| worker pane runs `horch worker` then the agent | `main.rs` `Worker`; `execution/service.rs:1-7` | kept; added the 6 agent CLIs |
| spawn service stages plan → record → brief → pane → launch | `execution/service.rs:3-7` | kept |
| ledger at `state dir/<project>.json` | `execution/store.rs:276-279` (slug of the path) | kept; added a note on the slug and state dir |
| events at `state dir/multi-herdr/<project>/events` | `measure/paths.rs:20,79,87` | kept |
| the judge writes to the ledger | `competition/judging.rs:429` `insert_execution` | kept |
| `horch sessions` hides candidates and judge unless `--all` | `cmd/ledgercmd.rs:90-97`; `main.rs` `Sessions` | kept |
| `horch cost` / `usage` count workers, candidates, judge | `cmd/cost.rs` (no round filter) | kept |
| sequence: insert orchestrator record | `cmd/recipes.rs:294` | kept |
| sequence: `horch route` spawn / substitute / refuse | `cmd/route.rs`; `main.rs` `Route` doc | kept; added exit 3 (`exit.rs` `REFUSED`) |
| sequence: plan with no I/O, insert Planned, brief, split, run worker | `execution/service.rs:1-7` | kept; added "Starting, pane id recorded" |
| sequence: worker sets Running, session id recorded | `execution/lifecycle.rs:272` | fixed: session id is minted or discovered |
| sequence: `horch done` → Done, DONE message, close, re-tile | `execution/lifecycle.rs:61-64`; `workspace/arrange.rs:618-644` | fixed: a detached re-tile starts, then the pane closes |
| side commands list | `main.rs` | kept; added `agent-list`, the 5 pools |
| preflight list "git · disk · RAM · harness --version · budget · storage · herdr · --promote-to" | `competition/preflight.rs:200-214` (PRE-01..PRE-13) | fixed: added CPU/GPU, limits, provider pools, parallelism, judge |
| preflight fail → exit 4, no worktree, no model call | `dataset/run.rs:203-208`; `dataset/mod.rs:39` | kept |
| (missing) no candidate fits the usage limits → exit 3 | `dataset/run.rs:211-213` | added |
| plan → N worktrees → dataset workspace (root pane runs `watch`) → spawn in waves | `competition/coordinator.rs:256-263,339-362` | kept |
| observe: done, crash, pane gone, deadline → freeze | `competition/observe.rs`; `coordinator.rs` | kept |
| "budget hard limit → cancel the rest" | `competition/budget.rs:5-16`; `coordinator.rs:417-436` | fixed: spend reaches hard ceiling minus judge reserve → cancel running candidates |
| "low disk → stop new launches" | `coordinator.rs:438-452` | kept; merged with StopLaunches (spend + committed) |
| gates from `.multi-herdr/dataset.yaml` | `competition/config.rs:1,19`; `evaluation/validator.rs:3` | kept |
| eligible = completed and every gate passed | `evaluation/validator.rs:78` | kept |
| blind judge bundle, no model names, no cost | `evaluation/judge_input.rs:15` | kept |
| judge-job: `claude -p`, read-only tools | `harness/headless.rs:1-7,26-30` | kept |
| failed twice / tie / abstain / low confidence → NEEDS_INTERVENTION, exit 5 | `measure/projection.rs:35` (`MAX_JUDGE_ATTEMPTS = 2`); `evaluation/winner.rs:93-125`; `dataset/run.rs:386-391` | kept |
| reject all / winner not eligible → REJECTED | `evaluation/winner.rs:97,113` | kept |
| "REJECTED · exit 6" then cleanup → "COMPLETE · exit 0" | `dataset/run.rs:358-393` `outcome_line` | fixed: the exit code follows the outcome line; a table now gives 0 / 3 / 5 / 6 |
| promotion: revalidate, CAS publish; conflict → NI; stale or gates fail → REJ | `competition/state.rs:202-217` | kept |
| cleanup removes worktrees, keeps branches | `competition/cleanup.rs:3,36` | kept; added `--prune-branches` |
| resume adopts executions and the judge job, no duplicates | `coordinator.rs:4-7,19-25` | kept |
| `promote` from NEEDS_INTERVENTION or COMPLETE | `dataset/promote.rs:91`; `state.rs:254,258` | kept; added "with a winner" and that 3 commands are hidden |
| `rollback` only if nobody moved the ref | `dataset/rollback.rs` | kept |
| `cleanup --force` for NEEDS_INTERVENTION | `dataset/cleanup.rs:5,36` | kept |
| outcome, export, readiness | `dataset/cli.rs` | kept |
| table: `note`, `done` need a herdr server | `cmd/messaging.rs:98-101` (`note` writes the ledger only) | fixed: `note` needs no herdr; `done` does |
| table: `sessions`, `cost`, `usage`, `quota`, `route` need no herdr | no `Herdr` in those files or `telemetry/`, `routing/` (except `telemetry/lock.rs`) | kept |
| table: `promote`, `rollback`, `cleanup` "not for the git work" | no `Herdr` in those 3 files | fixed: no herdr at all |
| (missing) `horch agent-list` | `main.rs:207-215`; `cmd/agentlist.rs`; `harness/inventory.rs` | added to setup, side commands and the table |
| (missing) Antigravity, `agy`, pool `google` | `harness/mod.rs:269`; `routing/quota.rs:39,44,137`; `main.rs` `Quota` doc | added; `horch cost` has no `agy` reader (`usage.rs:573-577`) |

The 5 Mermaid blocks were not rendered locally (no `mmdc`; conventions
forbid package managers). A script checked that every block has matching
quotes and that every `subgraph` has an `end`.

## Per-recipe confirmations (file:line, rebased on `41506d2`)

add-harness:
- `crates/horch-core/src/harness/mod.rs:91` - `pub trait Harness`; `:262` `enum HarnessKind`; `:276` `ALL`; `:286,300,313,326,372` `as_str`, `binary`, `adapter`, `capabilities`, `FromStr`; `:409` `all_names_every_kind`; `:434` `legacy` table.
- `crates/horch-core/src/harness/capabilities.rs:161` - `ANTIGRAVITY` const.
- `crates/horch-core/src/harness/launch.rs:598` - test `unreachable!()` arm; `:104` `FORBIDDEN_ENV` (only `ANTHROPIC_API_KEY`).
- `crates/horch-core/src/harness/antigravity.rs:28` - per-adapter forbidden env.
- `crates/horch-core/src/roster/permission.rs:62` - `antigravity_args`.
- `crates/horch-core/src/roster/validation.rs:194` env rule; `:246` permission arm; `:610` phase-defaults test.
- `crates/horch-core/src/routing/quota.rs:44` `POOLS`; `:137` `pool_for` arm.
- `crates/horch-core/src/runtime/bins.rs:51,77` override name and arm; `crates/horch-core/src/runtime/context.rs:452` count 10 in `arc_05_context_from_map_env`.
- `crates/horch-core/src/harness/inventory.rs:68-74` iterates `HarnessKind::ALL`: no edit.
- `crates/horch/src/dataset/preflight.rs:130` `kind_of` iterates `ALL`: no edit; test `:424`.
- `crates/horch-e2e/src/harness.rs:19` `FAKES`; `crates/horch-e2e/Cargo.toml:36` `[[bin]]`.
- `crates/horch-e2e/tests/lifecycle.rs:210,219`; `crates/horch-e2e/tests/skills_exposure.rs:217`.
- `crates/horch-core/src/usage.rs:573-577` and `telemetry/readers.rs:136` - no reader for unknown agents.
- `crates/horch-core/tests/arch_scan.rs` `arc_10_harness_match_only_in_harness`: variants matched only under `harness/` and in `roster/validation.rs`; `pool_for` and `permission.rs` match strings or modes, not variants.

add-skill:
- `crates/horch-core/build.rs:60` - embeds every file under `skills/`.
- `crates/horch-core/src/skills/selection.rs:12` - `phase_skills`.
- `crates/horch-core/tests/skills_catalog.rs:21` `DESIGN_SOURCE_PINS`; `:53` `REPO_ORIGINAL`; `:108` `skl_01_...` (provenance required, pins, digests); `:222-223` 12 KiB / 160 KiB; `:250` text only.
- `crates/horch/tests/baseline_cli.rs:72` `a0_skills_view` filters to A0 skills; `:149` `oracle_cli_skills_match`.
- `./target/debug/horch skills show ui-taste` prints id, `bundled+48ce21ae237b`, digest, description.

add-teammate:
- `justfile:111` `teammate-new`; `crates/horch/src/cmd/teammatescmd.rs:258` `new` (needs `--dir` or `HORCH_TEAMMATES_DIR`).
- `crates/horch-core/build.rs:34` - `BUILTIN_TEAMMATES` glob.
- `crates/horch-core/tests/baseline_oracles.rs:70` `SKIP_NEW_TEAMMATES`; `:585` `TEAMMATES_AT_A0 = 33`; `:600` `arc_01_baseline_oracles_present` counts files.
- `crates/horch-core/tests/skills_catalog.rs:79` `SKIP_NEW_TEAMMATES`.
- `crates/horch-core/src/roster/validation.rs:610` phase arms; `:939,951` Claude fleet-pane count 24.

add-command:
- `crates/horch/src/main.rs:209` `AgentList` variant with doc comments; `:488` dispatch; `:622` `cli_definition_is_valid`.
- `crates/horch/src/cmd/mod.rs:1` `pub mod agentlist`.
- `crates/horch-core/src/harness/mod.rs:14` `pub mod inventory`.
- `crates/horch/tests/agent_list.rs:57` `CARGO_BIN_EXE_horch`, `env_clear`, fakes through `HORCH_*_BIN`.
- `crates/horch/src/dataset/mod.rs:30` `exit`; `crates/horch/tests/dataset_cli.rs:27,34` `BIN`, `cmp_01_cli_args`.
- `crates/horch-core/tests/golden_prompts.rs:89` sanctioned blocks for `_base` changes.

add-dataset-event:
- `crates/horch-core/src/measure/event.rs:167` `event_kinds!`; `:200` `OperatorPromote`; `:204` no `deny_unknown_fields`; `:367-379` `#[serde(default)]` fields; `:32` `EVENT_SCHEMA_VERSION`.
- `crates/horch-core/src/measure/projection.rs:282` `apply_round` (exhaustive match, `:508-512`); `:452` `OperatorPromote` arm; `:132-139` anomalies.
- `crates/horch-core/src/competition/state.rs:23` `RoundEvent`; `:159` `TABLE`; `:254` `operator.promote` row; `:279` `table_has_no_duplicate_keys`.
- `crates/horch-core/src/measure/recorder.rs:22` `NewEvent` with `idempotency_key`; `crates/horch/src/dataset/promote.rs:51` writer.
- `crates/horch-core/tests/measure.rs:156` `sample_kinds`; `:441` `mea_02_envelope_roundtrip_every_kind`.
- `crates/horch/src/dataset/rebuild.rs:13` replays and compares with the live fold.
- `crates/horch-core/src/dataset/export.rs:4-8,28` export reads the projection, not raw events.

## Corrections to the implementation plan

- `HORCH_BLESS`: the plan said the e2e scenario writes only an absent file.
  `crates/horch-e2e/tests/scenario.rs:77` overwrites. The 7 readers are: 4
  overwrite (`baseline_oracles.rs`, `baseline_cli.rs`, `cmd/telemetry.rs`,
  `scenario.rs`), 3 write only when absent (`dataset_export.rs`,
  `measure.rs`, `harness.rs`).
- New teammate oracles: conventions §5 allows new oracle files, but
  `arc_01_baseline_oracles_present` counts exactly 33 launch oracles, so a new
  file fails the gate. `STATUS.md` already says "no new oracle files". The
  pages document `SKIP_NEW_TEAMMATES` only.
- `golden_prompts.rs` goldens have no bless: they hold the old text, and a
  `_base` change needs a named sanctioned block. The plan did not mention it.
- A new teammate also changes 2 tests in `roster/validation.rs` (phase arms
  and the Claude fleet-pane count). The plan named only the first.
- `kind_of` is at `preflight.rs:130`, its test at `:424`.

## T7 results

1. Relative links: 0 broken. Anchors `README.md#recipes` and
   `testing-and-gates.md#horch_bless` match headings.
2. Repo paths in backticks: 0 missing in the 10 new pages. In the 2 existing
   pages the check reports `skills/list` and `skills/horch-probe/SKILL.md`;
   these are a Codex app-server method and a temp path, not repo paths.
3. Test names: every backticked snake_case name that is a test exists as
   `fn <name>` (47 names; non-tests such as `deny_unknown_fields` excluded).
   Patterns checked by the Antigravity instance:
   `arc_26_e2e_lifecycle_matrix_antigravity`, `skl_06_e2e_exposure_antigravity`.
4. Commands: `horch <sub> --help` exits 0 for 33 subcommands named in the
   pages, `multi-herdr-dataset <sub> --help` for 12.
5. Scope: `git diff --stat design-skills` lists the 10 new pages, the
   report, `README.md`, `crates/horch-core/src/lib.rs` (1 doc line) and
   `docs/phase-skills.md` (1 line).
6. `ANTHROPIC_API_KEY` appears only as the prohibition and in
   `env -u ANTHROPIC_API_KEY claude ...`.
7. Gate: `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate` ends with
   `GATE GREEN` (62 test binaries, 828 tests, 0 failed).
8. Read-through: I followed `add-teammate.md` from `docs/README.md`. I added
   the 2 `validation.rs` tests and the rebuild before `teammates --check`
   after the walk.

## Rebase

Rebased on `design-skills` at `41506d2` (D11 and D14 merged). D11 added
`google` to `teammates/_base/fleet-orchestrator.md:115`, so that open item is
gone. D14 changed only `skills/motion-gsap` text. `a6da895` moved
`crates/horch-core/build.rs` lines by 5; the line numbers above are updated.
D12 and D13 are not merged yet; no page states a line number in
`execution/lifecycle.rs`, `fake-herdr.rs`, `competition/promotion.rs`,
`budget.rs` or `coordinator.rs`. The report cites `coordinator.rs` and
`budget.rs` lines in the claim table; re-check them if D13 merges first.

## Not verified

- Mermaid rendering (no renderer installed).
- Every recipe was walked against the code and the merge diffs, not executed
  end to end. No harness, teammate, skill, command or event was added.
- Real harness binaries: none ran. `horch agent-list` was not run with probes.

## Out of scope (found, not fixed)

- `README.md:5` says "Three binaries" and the table omits
  `multi-herdr-dataset`.
- `README.md` "Environment" table omits `HORCH_ANTIGRAVITY_BIN`.
- `README.md` telemetry block: `horch quota --refresh  # the pools: claude,
  codex, opencode-zen, local` omits `google`.
- `README.md` "Skills for each phase": "across all five harnesses".
  `teammates/_template.md` skills comment: "on all five agent harnesses".
  Antigravity exposes no skills.
- `ai_docs/plans/design-skills/00-conventions.md` §5 still permits new
  oracle files for a new teammate, which `arc_01` forbids.
- STATUS.md D10 items `horch done` pane race and fake-herdr pane id reuse
  stand unless D12 merged.
