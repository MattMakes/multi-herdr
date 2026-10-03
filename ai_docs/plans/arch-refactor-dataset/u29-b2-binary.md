# U29 b2-binary: the multi-herdr-dataset entrypoint, preflight end to end, data commands

Unit slug: `b2-binary`. Branch: `ard/b2-binary`. Phase: B2 (binary part) and B6 (CLI part).
Requirements: CMP-01, PRE-06, PRE-07, PRE-12, NFR-10 (already), EXP-06 (CLI path).

## GOAL

A separate, explicitly expensive binary `multi-herdr-dataset` exists, is
installed by `horch install` and `just install`, and runs preflight end to
end: it gathers real facts, evaluates them with the pure preflight, records
the report, and refuses (exit 4) before creating any worktree or invoking any
model. The data commands (`status`, `export`, `readiness`, `outcome`,
`rebuild`) work over the recorded events. `run` stops after a passed
preflight with a clear "rounds not implemented yet" message until the B3
coordinator unit lands (that unit replaces it).

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Read in `00-master-plan.md`: OD2, OD3, §1 layout (`crates/horch/src/bin/multi-herdr-dataset.rs`,
  `crates/horch/src/dataset/{run,preflight,status,watch,judge_job,promote,rollback,cleanup,export,readiness,outcome,rebuild}.rs`),
  §3 "B2" (binary, command, exit codes, preflight refusal) and "B6"
  (`outcome`, `rebuild`), §4 PRE, CMP-01, EXP-06 rows, §5 LA-1 and LA-6.
- Merged code and reports (`ai_docs/reports/arch-refactor-dataset/`):
  `a2-runtime.md` (`crates/horch/src/lib.rs` exposes `bootstrap`, `output`,
  `exit`; `RuntimeContext`; `settings.machine_file` is read but unused — use
  it), `b2-preflight.md` (`config::load`, `RunFlags`, `parse_usd_micro`,
  `preflight::evaluate`, `storage_probe`; `worktree_root` must be absolute;
  `JudgeConfig.policy` is a JSON placeholder — type it now as
  `evaluation::winner::WinnerPolicy`), `machine-teacher.md`
  (`runtime::machine::probe`), `b2-vcs.md` (`GitCli`, `GitClient` for git
  facts), `a5-routing.md` (`routing::quota_probe::harness_version`, pools),
  `b3-planner.md` (`plan_round` to know which harnesses the round needs),
  `b1-measure.md` (`DatasetPaths`, `JsonlRecorder`, events
  `experiment.created`, `preflight.completed`, `experiment.aborted`; `fold`),
  `b6-export.md` (`export`, `readiness`, `record_outcome`; the CLI must
  check the round state before recording an outcome; `export` takes an
  `ExecutionFactsSource`: adapt the execution store), `b5-promotion.md` if
  merged (dataset path validation), `e2e-fakes.md` (`Harness::with_git()`).
- Exit codes: 0 ok, 3 budget or quota refusal, 4 preflight failed,
  5 needs intervention, 6 rejected.
- Parallel units: U18 `a4-harness`, U23, U25, U27 and the coming A6 service
  unit. The B3 coordinator unit comes after you and fills `run`.

## FILES

own:
- `crates/horch/src/bin/multi-herdr-dataset.rs` (new; thin)
- `crates/horch/src/dataset/**` (new)
- `crates/horch/src/lib.rs` (add `pub mod dataset;`)
- `crates/horch/Cargo.toml` (`[[bin]] multi-herdr-dataset`)
- `crates/horch/src/cmd/install.rs` and `justfile` `install` recipe (install the second binary)
- `crates/horch-core/src/competition/config.rs` (type `JudgeConfig.policy`)
- `crates/horch/tests/dataset_cli.rs` (new), `crates/horch-e2e/tests/dataset.rs` (new)
- `ai_docs/reports/arch-refactor-dataset/b2-binary.md`

do not touch: every other file.

## STEPS

1. Create the worktree (conventions §2).
2. Binary skeleton (clap, like `horch`): subcommands
   `run <task> --candidates N --strategy diverse --budget-usd X --judge auto [--baseline T] [--promote-to B] [--worktree-root D] [--allow-dirty]`,
   `status [<exp>]`, `export [--label-policy V]`, `readiness [--policy FILE]`,
   `outcome <round> --kind regression|revert|verified [--score S] [--note N]`,
   `rebuild <exp>`, and hidden placeholders `judge-job`, `promote`,
   `rollback`, `cleanup`, `watch` that print "not implemented in this
   build" and exit 2 (later units fill them). `main` builds the
   `RuntimeContext` with `horch::bootstrap` and dispatches into
   `crates/horch/src/dataset/*`.
3. `dataset/preflight.rs` (impure gatherer + pure evaluate): resolve the
   project and repo (`GitCli` with the git bin from ctx), base SHA, dirty
   state, namespace check for the planned branches; machine snapshot
   (`probe`, honoring `ctx.settings.machine_file`); every planned
   candidate's harness version via `harness_version` (no model turn) —
   this happens BEFORE any worktree (PRE-06); storage probe in the dataset
   dir; herdr reachable; horch exe found. Record `experiment.created`
   (digests), then `preflight.completed{report}`; on any Fail, record
   `experiment.aborted{failed_checks}` and exit 4. Persist the report also
   in `experiments/<id>/manifest.json` with `environment_digest`. Persist
   no secret: harness `--version` output and env snapshot pass through
   `redact` and `envsnap` (PRE-07).
4. `run`: config load → plan the round (`plan_round`) to know the
   candidates → preflight → on pass, print the plan and the report, and
   exit 0 with "rounds not implemented in this build" (B3 replaces this).
5. Data commands: `status` (fold and print experiments and rounds with
   their state), `export` (export → `write_export`; adapt the execution
   store as `ExecutionFactsSource`), `readiness` (print verdict and gaps),
   `outcome` (refuse unless the round is DECIDED/PROMOTED/COMPLETE with a
   winner; then `record_outcome`), `rebuild` (re-derive projections from
   events and print a summary; byte-compare with the live fold).
6. Install: `horch install` and `just install` also install
   `multi-herdr-dataset` next to `horch` (read `cmd/install.rs`).
7. Tests:
   - `cmp_01_cli_args` (`crates/horch/tests/dataset_cli.rs`): every flag
     parses; `--budget-usd` is exact; missing task fails; `--help` lists the
     subcommands.
   - `pre_06_harness_resolution_before_worktree` and
     `pre_12_e2e_refuses_before_worktree_or_model` (in
     `crates/horch-e2e/tests/dataset.rs` with `Harness::with_git()` and the
     fakes): a failing preflight (for example a missing harness, or a
     machine fixture with no disk) exits 4, writes `experiment.aborted`,
     creates no worktree (`git worktree list` has 1 entry) and makes no
     agent call (the fakes' logs have no launch argv; a `--version` probe is
     allowed and must come before any worktree).
   - `pre_07_probe_no_secret_persisted`: with `ANTHROPIC_API_KEY=SENTINEL`
     and a fake whose `--version` prints a `sk-ant-...` token, no file
     under the dataset dir contains `SENTINEL` or the token.
   - `exp_06_outcome_cli_refuses_wrong_state`.
8. Gate after each step. Commits: `B2: Add the multi-herdr-dataset binary`,
   `B2: Run preflight end to end`, `B6: Add dataset data commands`,
   `B2: Install the dataset binary`, `B2: Add CMP-01 and PRE e2e tests`.
9. Write and commit the report. Follow conventions §6.

## DONE WHEN

- The named tests pass with `HORCH_REQUIRE_GIT=1`.
- `cargo run --bin multi-herdr-dataset -- --help` lists every subcommand.
- `just gate` is green; `check-req-coverage.sh --phase B2` exits 0.

## REPORT

- `horch note` after each commit.
- `horch done` summary: CLI surface, the preflight fact gatherer, what the
  B3 coordinator unit must replace, gotchas.
