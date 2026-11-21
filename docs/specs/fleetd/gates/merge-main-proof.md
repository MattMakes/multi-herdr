# E1 proof: the trial merge of origin/main

Task T-314 (E1-prep-2) proved the trial of `merge-main-plan.md` (T-312). Each
claim has a label: **verified** (with the evidence) or **not verified**.

## Tip of the trial branch

| ref | commit | note |
|---|---|---|
| `keep/fleetd-finish-trial-merge-main` | `1e782060` | the result of T-314. Local branch, not pushed |
| merge of the run branch | `254e436b` | parents `52bca240` (the trial of T-312) and `51f007f6` (`fleet/fleetd-finish` at the start of T-314). No conflict |
| trial merge of T-312 | `52bca240` | parents `2c6ef0b4` (run branch) and `e5170752` (`origin/main`, Release v0.5.1) |

Chain: `1e782060` has 9 fix commits on top of `254e436b`. `254e436b` has the
parents `52bca240` and `51f007f6`. `52bca240` has the parents `2c6ef0b4` and
`e5170752`. Verified: `git log --format='%h %p'` and
`git merge-base --is-ancestor origin/main trial-proof`.

The run branch moved during T-314: `fleet/fleetd-finish` is now `9da64804`, 17
commits after `51f007f6`. `git merge-tree --write-tree fleet/fleetd-finish
keep/fleetd-finish-trial-merge-main` exits 0 (no conflict): verified. The tests
of those 17 commits on the trial: not verified.

Commit `38736b49` (the `watch_once` fix) is in the trial: verified with
`git merge-base --is-ancestor 38736b49 HEAD`.

## Result of each suite

Each command ran through `scripts/dev-sandbox.sh`. "first" is the run on
`254e436b` before the fixes. "final" is the run on `1e782060`. Load is the
1-minute load average at the start of the command.

| step | command | first: passed / failed | final: passed / failed | load |
|---|---|---|---|---|
| 2 | `cargo check --workspace --all-targets --locked` | exit 0 | - | 28.9 |
| 2 | `cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0, 0 warnings | exit 0 | 33 / 9.3 |
| 2 | `cargo clippy --workspace --bins --release --locked -- -D warnings` (CI form) | - | exit 0 | 12.9 |
| 2 | `cargo fmt --all --check` | exit 0 | exit 0 | - |
| 3a | `cargo test -p horch-core --lib --locked` | 1900 / 6 | 1906 / 0 (in the full run below) | 33.4 |
| 3b | `--test skills_catalog` | 33 / 0 | 33 / 0 | 30.2 |
| 3b | `--test golden_prompts` | 15 / 2 | 17 / 0 | 30.2 |
| 3b | `--test baseline_oracles` (launch oracles of rows 9 and 10) | 7 / 0 | 7 / 0 | 30.2 |
| 3b | `--test godot_wave` | 21 / 0 | 21 / 0 | 30.2 |
| 3b | `--test harness` | 6 / 0 | 6 / 0 | 30.2 |
| 3b | `--test context_roster`, `--test intake_roster` | 38 / 0 | 38 / 0 | 30.2 |
| 3b | `roster` | no test target has this name. The roster tests (row 7 count) are in the lib: see 3a | - | - |
| 3c | `--test cms_comment_sweep` | 1 / 5 | 6 / 0 | 43.2 |
| 3c | `--test trf_18_shared_diff_base --test trf_27_review_loop --test w3_finish_steps` | 52 / 0 | 52 / 0 | 29.7 |
| 3c | `--test verifier_review` | 10 / 1 | 11 / 0 | 29.7 |
| 3c | `cargo test -p horch-e2e --locked --test p4_release` | 4 / 0 | - | 27.7 |
| 3d | `cargo test -p horch-core --lib -- routing::quota fleet_runtime::application::holds` | 47 / 0 (1 ignored) | in the full run | 27.9 |
| 3d | 10 targets that read `routing::quota`: `execution_plan`, `fleetd_pass_p2`, `fleetd_pass_p7`, `p7_execution_starts`, `r_execution_service`, `r_routing_assess`, `r_routing_parsers`, `r_telemetry_core_collect`, `routing`, `telemetry_executions` | 76 / 0 (1 ignored) | in the full run | 27.9 |
| 3e | `cargo test -p ui-prop-tester --locked` | 241 / 0 | - (no change in the crate) | 18.9 |
| 3f | `cargo test -p horch-core --locked --test taskledger_schema_p7 --test trf_48_ledger_guard --test taskledger_queue` | 22 / 0 | in the full run | 47.7 |
| 3f | `cargo test -p horch-e2e --locked --test p7_release` | 4 / 0 | - | 27.7 |
| 3g | `cargo test -p horch --locked` (49 binaries, no skip) | 614 / 0 | 614 / 0 | 12.6 / 35.6 |
| extra | `cargo test -p horch-core --locked --no-fail-fast` (223 binaries) | 3694 / 4 | 3699 / 0 | 18.9 / 12.4 |

Each row: verified (the run logs of T-314). The suites of step 3 ran with
`--test-threads=4` when the load was above 32, and also in most other runs.

S7, callers of `finish/comments.rs`: verified with `git grep`. The only caller
is `run_step` (`taskledger/finish/mod.rs:1065`), which `finish::finish` runs.
`finish::finish` has no caller in production code. Its callers are the tests
`cms_comment_sweep`, `trf_18_shared_diff_base`, `trf_27_review_loop`,
`verifier_review`, `w3_finish_steps`, `horch-e2e/tests/p4_release.rs` and the
test module of `finish/land.rs`. Each of them is green in the table above.

S4, write access of a sandboxed fleetd worker: verified by reading the code and by 1
existing test. The line numbers are of the trial branch.

- The seats of a fleet worker have `horch_access: full`. It is the default
  (`roster/teammate.rs:361`), and no fleetd code changes it. Only the 5
  intake seats set `horch_access: report`.
- `harness/claude.rs:1024-1028`: a `Full` worker writes its task folder and
  its worktree.
- `harness/claude.rs:1100-1114`: a `Full` teammate gets `allowWrite` for the
  git common dir and `<repo>/.worktrees`, and `additionalDirectories` for
  `.worktrees` (main's change).
- `seal_operator_boundary` (`claude.rs:1336`) closes only `<fleet root>/run/`
  and the ledger files, so it does not close `.git`.
- Proof: the test `a_sandboxed_fleet_worker_can_write_git_and_worktree_paths`
  (`claude.rs:2323`) builds a linked worktree and asserts both `allowWrite`
  paths. It runs on macOS (Seatbelt is present) and passes in 3a.
- A `HorchAccess::Report` pane writes only its task folder
  (`claude.rs:1033-1045`). It cannot commit. The plan's S4 says that a fleetd
  launch uses `Report`. That is wrong: fleetd launches the worker seats with
  `Full`.

S5: not verified as a behavior. The 24 unit tests of `routing/quota.rs` and
the 11 tests of `holds.rs` pass. No test sends a scoped refusal or a refusal
past its window through a fleetd hold.

## Fixes on the trial branch

| commit | file | why |
|---|---|---|
| `983d46ca` | `teammates/addictive-game-design-reviewer.md`, `comment-remover.md`, `game-design-incremental-specialist.md`, `root-cause-auditor.md` | 6 lib tests failed: the 4 worker seats of main had no `sandbox:` block (SR-8). `comment-remover` has `fallbacks: []`, and now has a `no_fallback_reason`: the codex sandbox cannot write `.git` |
| `05cdf1e9` | `crates/horch-core/tests/golden_prompts.rs` | 2 tests failed: main's `CMS_WORKER` and `CMS_ORCHESTRATOR` blocks add comment-sweep text of the retired finish. The trial keeps the run branch's briefings (rows 25, 26), so the file is now equal to the run branch's file. It depends on Q1 option a |
| `1521b249` | `crates/horch-core/tests/cms_comment_sweep.rs` | 5 tests failed: the world added a task without an idea (FR-12) and used `workers::supervisor_started` on a bound worker. It now does both as `trf_18_shared_diff_base.rs` does |
| `14833c51` | `crates/horch-core/tests/verifier_review.rs` | `claim_attempt_finish_is_unchanged` failed: main's S1 puts `comment-sweep` into every profile, so a claim-attempt finish runs it. The test expects the true list, with `comment-sweep` `not_applicable` (the change has no code file). Each status assert stays. It depends on Q1 |
| `2dc1bf51` | `crates/horch-core/tests/fleetd_scans.rs` | 4 tests failed: the test migrated to the literal 8, which is now `COMMENT_SWEEP_VERSION`. It needs `pass_cursors` of `PHASE_1_VERSION` |
| `540f66f4` | `crates/horch-core/src/roster/tests.rs` | the pinned list of `no_fallback_reason` seats names `comment-remover` (from `983d46ca`) |
| `19fb2892` | `taskledger/schema.rs`, `taskledger/model.rs`, `fleetd/fleet_runtime/domain/jobs.rs`, `fleetd/task_execution/domain/task.rs`, `tests/taskledger_schema_v8.rs` | schema renumber, step 6: `V8_JOB_KINDS` is `PHASE_1_JOB_KINDS`, `events_v8_ddl` is `events_phase_1_ddl`, and the comments that named the Phase 1 step "version 8" or `[v8]` name `PHASE_1_VERSION` or `phase_1` |
| `9ca39e21` | `crates/horch-core/tests/arch_scan.rs` | 2 fitness tests failed on main's crate `ui-prop-tester`. The PL-2 scan and the SR-7 scan skip the crate by its exact name. The new test `no_crate_of_horch_depends_on_ui_prop_tester` fails when a crate in `crates/` names it in its `Cargo.toml`. The orchestrator chose this option |
| `1e782060` | the 4 seats above and `teammates/game-design-juicer.md` | `p7_context_256k` failed: the 5 seats of main had `compact_window` 200000 or 150000, and FR-9 sets 256000. `p5_profile_validation` failed: `game-design-juicer` had no sandbox, so macOS refused it |

## Red before the merge

None. Each red test fails on a file, a seat, a crate or a version number of
main, so the merge caused it. Verified from the diff. No test was run on
`fleet/fleetd-finish` to confirm this.

## For the operator

1. **Schema numbers in `docs/specs/fleetd/design.md`.** T-314 did not edit
   this file. The numbers in the code are now: `COMMENT_SWEEP_VERSION` 8,
   `PHASE_1_VERSION` 9, `PHASE_2_VERSION` 10, `PHASE_3_VERSION` 11,
   `EVENT_KINDS_VERSION` 12, `WORKTREE_PATHS_VERSION` 13, `PHASE_4_VERSION`
   14, `PHASE_5_VERSION` 15, `PHASE_6_VERSION` 16, `PHASE_7_VERSION` 17,
   `VERSION` 17.
   - `design.md:119`: "schema v7". This describes the ledger before fleetd.
   - `design.md:1611`: "Schema v8 adds a `leases` table" (Phase 2). The
     code has 10.
   - `design.md:1615`: "Schema v9 gives `ideas` the state machine" (Phase
     6). The code has 16.
2. **Other old numbers, not changed.**
   - `docs/specs/fleetd/preservation-05-contracts-and-values.md:8`, `:9`,
     `:98` say "v8 leases, v9 ideas" (from the design).
   - The SQL temp table names `events_v8` (`schema.rs`) stay. They are
     strings of a migration.
   - The test file name `taskledger_schema_v8.rs` and the ticket
     `docs/tickets/fleetd-p1/03-schema-v8.md` stay. The header of the test
     file now names `PHASE_1_VERSION`.
   - `taskledger/supervisor.rs:1028-1050` has "schema version 8" in a fake
     error text of a test. The number has no meaning there.
   - `docs/specs/comment-sweep.md:34` "version 8" is correct.
3. **ui-prop-tester and arch_scan.** The skip covers 2 rules. PL-2 says no OS
   `cfg` in production code outside `horch-platform`: the crate has 17 sites.
   SR-7 says each process starts through the launcher or the platform start
   function: the crate has 5 `Command::new`. The operator decides later
   whether the crate is ported to `horch-platform`. The skip is safe only
   while no horch crate links it, and `no_crate_of_horch_depends_on_ui_prop_tester`
   checks that.
4. **Q1 (comment-sweep in fleetd)** is still open. The fixes `05cdf1e9` and
   `14833c51` follow option a. With option b, they change.
5. **Not verified.**
   - `cargo test --workspace --locked` as one command. The other crates
     (`horch-platform`, `horch-marketplace`, `herdr-install`,
     `agent-docs-sync` and others) and `horch-e2e` (only `p4_release` and
     `p7_release` ran) were not run.
   - The 17 run-branch commits after `51f007f6`, in the trial.
   - S5 as a behavior (see above).
   - The CI result of the trial (S10).
