# U31 b4-judge-job: headless judge, detached job, scheduler, coordinator judging step

Unit slug: `b4-judge-job`. Branch: `ard/b4-judge-job`. Phase: B4 (job part).
Requirements: JDG-04, JDG-08, JDG-09, SEC-04, CMP-12, CMP-16, SEC-08 (judge part).

## GOAL

When a round reaches JUDGING_BACKGROUND, the coordinator builds the blind
judge bundle, schedules a detached `multi-herdr-dataset judge-job` process
that runs the hidden `judge` teammate headless (`claude -p`, no API key,
read-only tools, cwd = the bundle), and later discovers the job's output,
parses it strictly, writes `judgements/<round>.json` once, and records the
winner decision. A crashed, timed-out or lost job is recorded as data and
retried once; then the round goes to NEEDS_INTERVENTION.

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Read in `00-master-plan.md`: §3 "B4" (all of it), §4 JDG, CMP-12, CMP-16,
  SEC-04, SEC-06, SEC-07, SEC-08 rows, the failure matrix, §5 fault points
  `abort-after-judge-scheduled`, `abort-in-judge-job-before-output`,
  `abort-after-judge-output`, `abort-after-judgment-written`,
  `abort-after-winner-selected`.
- Design doc `ai_docs/designs/2026-10-02-dataset-competition-design.md` §4.7
  (scheduler, `JudgeJobSpec`, `JobState`, `schedule`, `discover`,
  `headless_command`, the `decide_winner` table), §5.1 (JUDGING_BACKGROUND
  rows), §7.1, §7.2.
- Merged reports (`ai_docs/reports/arch-refactor-dataset/`):
  `b4-evaluation.md` (parser, winner, rubric, `judge_policy_digest`),
  `b4-judge-input.md` (`build_judge_input`; gotchas: headless must
  substitute `{rubric}` and `{schema}` into the judge.md body; chmod the
  0500 bundle dirs back before cleanup; never retry with a partial bundle:
  a bundle either exists complete with a valid manifest digest or is
  rebuilt into a fresh dir), `a4-harness.md` (`Capabilities.headless`,
  FORBIDDEN_ENV handling, the launch flow), `a3-roster.md` (`HEADLESS_ONLY`),
  `routing` `quota.rs` (the `Child` pattern with a timeout, about line 618),
  `b1-measure.md` (judge events, `JsonlRecorder`, `DatasetPaths::jobs`),
  `b1-primitives.md` (`fsx::create_immutable`, `pid_alive`),
  `b2-binary.md` (the `judge-job` placeholder arm), `a6b-service.md` (how a
  non-pane execution record is inserted and moved to a terminal state),
  `b3-coordinator.md` when merged (the judging call site).
- Single authority (CMP-16): the job process writes ONLY
  `jobs/<round>/judge-<n>/{heartbeat, output.json (≤ 1 MiB), exit.json}`.
  Every `judge.*` event, the judgment file and the winner events are written
  by the coordinator process. The judge job is the only detached process in
  the system.
- "Background" means: the job outlives the coordinator. `run` waits for the
  job by default (it polls the job dir each tick); if `run` dies, `resume
  <exp>` discovers the job (output present → parse; heartbeat fresh and pid
  alive → wait; else `judge.failed{lost}` → retry).
- The judge run is a `kind: judge` execution in the normal execution store
  (JDG-09, ARC-24), so telemetry and cost count it. It never has a pane.
- Parallel units: U30 `b3-coordinator` owns `competition/coordinator.rs`. It
  leaves a call site that calls `competition::judging::start` and
  `competition::judging::poll` (signatures below). You write those
  functions. If U30 has merged before you finish, rebase and wire the call
  site yourself (send `QUESTION:` if the edit to `coordinator.rs` is more
  than the call site).

## FILES

own:
- `crates/horch-core/src/harness/headless.rs` (new), `harness/mod.rs` (1 `pub mod` line)
- `crates/horch-core/src/evaluation/scheduler.rs` (new), `evaluation/mod.rs` (1 line)
- `crates/horch-core/src/competition/judging.rs` (new), `competition/mod.rs` (1 line)
- `crates/horch/src/dataset/judge_job.rs` (new) and the `judge-job` arm in
  `crates/horch/src/bin/multi-herdr-dataset.rs` (replace the placeholder only)
- `crates/horch-e2e/src/bin/fake-claude.rs` (a `judge` mode: reads the stdin
  prompt, writes a scripted judgment, or crashes or hangs per
  `$HORCH_FAKE_LOG.judge.json`; records argv, cwd and env)
- `crates/horch-core/tests/judging.rs` (new), `crates/horch-e2e/tests/judge.rs` (new)
- `ai_docs/reports/arch-refactor-dataset/b4-judge-job.md`

do not touch: `teammates/judge.md` body (the Spec B §10 prose is pending
from the operator), oracle and golden data, `execution/**`, `routing/**`,
`competition/coordinator.rs` except the call site (see CONTEXT).

## STEPS

1. Create the worktree (conventions §2).
2. `harness/headless.rs`: `headless_command(ctx, teammate, session, schema)`
   per design §4.7: `claude -p --output-format json --session-id <minted>`,
   `--model`, `--effort` per the teammate, `--allowedTools Read,Grep,Glob`,
   `--disallowedTools Agent,Edit,Write,NotebookEdit,Bash`, no plugins, no MCP
   (`--strict-mcp-config` with an empty config if the harness supports it),
   `--json-schema <file>` only if `claude --help` lists it (cache the probe
   per process). The env is the context env minus FORBIDDEN_ENV
   (`ANTHROPIC_API_KEY` and the rest). The prompt is the judge.md body with
   `{rubric}` and `{schema}` substituted, on stdin. Check: unit test of the
   argv and env with a `MapEnv` that holds `ANTHROPIC_API_KEY`.
3. `evaluation/scheduler.rs`: `schedule(ctx, spec) -> Result<u32>`
   (`spawn_detached` of the current exe's sibling `multi-herdr-dataset
   judge-job --round R --attempt n ...`: new session/process group, stdin
   null, stdout/stderr to files in the job dir), `discover(job_dir, now,
   stale_after) -> JobState`. Pure parts (the discovery decision from file
   facts) are separate functions with table tests.
4. `dataset/judge_job.rs` (the job): write `heartbeat` every few seconds
   (a thread), run `headless_command` with cwd = the bundle dir, enforce the
   timeout (kill the child), cap stdout at 1 MiB (over the cap → exit.json
   `{reason: too_large}`), extract the result text from the
   `--output-format json` envelope, write `output.json` with
   `fsx::create_immutable`, then `exit.json`. Fault points
   `abort-in-judge-job-before-output`, `abort-after-judge-output`. It
   writes nothing outside its job dir.
5. `competition/judging.rs` (coordinator side):
   - `start(env, round) -> Result<()>`: 0 eligible is handled by U30
     (judge skipped). Build or reuse the bundle (`build_judge_input`; never
     reuse a partial one), insert a `kind: judge` execution record, mint
     the session, `judge.scheduled{attempt, input_digest, judge_policy_digest}`,
     `schedule`. Fault `abort-after-judge-scheduled`.
   - `poll(env, round, now) -> Result<JudgingStatus>` with
     `JudgingStatus { Waiting, Decided(WinnerOutcome), NeedsIntervention }`:
     `discover` → Running: `judge.started` once, Waiting; Output:
     `parse_judgment` (strict, 1 MiB cap); invalid → `judge.failed{parse}`;
     valid → `judge.completed` → `judgements/<round>.json` via
     `create_immutable` (`JudgmentRecord`) → fault
     `abort-after-judgment-written` → `decide_winner` → `winner.selected
     {promotion: not_requested}` or `winner.rejected` → fault
     `abort-after-winner-selected`; Lost, crashed or timed out →
     `judge.failed{lost|crash|timeout}`; attempt 1 → schedule attempt 2;
     attempt 2 → NeedsIntervention. The judge execution record goes to
     Done or Failed to match.
   - Every step is idempotent on re-entry (it reads the projection: no
     second schedule for an attempt that has `judge.scheduled`, no second
     judgment file).
6. fake-claude `judge` mode (see FILES); the scenarios: `valid`, `invalid`,
   `crash`, `hang`, `oversize`.
7. Tests:
   - core (`crates/horch-core/tests/judging.rs`):
     `jdg_04_parent_writes_judgment_atomically`,
     `jdg_08_restart_discovers_job` (table over `discover`),
     `cmp_12_judge_waits_for_terminal_set` (`start` refuses unless every
     candidate is validated or the round deadline passed),
     `jdg_09_judge_execution_in_ledger`.
   - e2e (`crates/horch-e2e/tests/judge.rs`, fakes, `Harness::with_git()`
     where git is needed):
     `jdg_04_job_writes_only_job_dir` (snapshot the dataset dir and the
     repo before and after a job; only the job dir changed),
     `jdg_08_crash_records_and_retries`, `jdg_08_timeout`,
     `jdg_09_argv_and_env` (with `ANTHROPIC_API_KEY=SENTINEL` in the parent:
     the fake's recorded env has no such key; argv has `-p`, the tool lists,
     the session id),
     `sec_04_judge_cwd_bundle_tools_readonly` (cwd is the bundle; the
     disallowed tools are listed; the bundle files are 0400),
     `cmp_16_only_judge_detached` (a source scan: the only `setsid`,
     `process_group` or detached spawn in the workspace is in
     `evaluation/scheduler.rs`),
     `sec_08_e2e_no_api_key_in_any_child` judge part if U30 has not made it
     cover the judge (extend, do not duplicate the name: use
     `sec_08_e2e_no_api_key_in_judge_job`).
   - Full round e2e (needs U30 merged; add after you rebase):
     `jdg_e2e_round_decided` (2 candidates, a valid judgment → DECIDED →
     CLEANUP → COMPLETE, `winner.selected{promotion: not_requested}`) and the
     B4 fault points in `cmp_13_crash_every_boundary` (extend U30's list).
8. Gate after each step. Commits: `B4: Add the headless command`,
   `B4: Add the judge job scheduler`, `B4: Add the judge-job subcommand`,
   `B4: Add the coordinator judging step`, `B4: Add JDG, SEC-04, CMP-12, CMP-16 tests`.
9. Write and commit the report. Follow conventions §6.

## DONE WHEN

- Every named test passes with `HORCH_REQUIRE_GIT=1`.
- `check-req-coverage.sh --phase B4` exits 0.
- `just gate` is green.

## REPORT

- `horch note` after each commit.
- `horch done` summary: the judging API, the job dir contract, the retry
  and discovery rules, gotchas for the B5 CLI unit.
