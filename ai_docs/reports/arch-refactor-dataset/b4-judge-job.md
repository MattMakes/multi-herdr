# Report: U31 b4-judge-job

Branch `ard/b4-judge-job`. Phase B4 (job part). Requirements JDG-04, JDG-08,
JDG-09, SEC-04, CMP-12, CMP-16, SEC-08 (judge part). Rebased on U26
`a6b-service`. `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate` is green
after every commit. `check-req-coverage.sh --phase B4`: every ID ok.

## Files

| File | Content |
|---|---|
| `crates/horch-core/src/harness/headless.rs` | `headless_command`, `judge_prompt`, `supports_json_schema`, `READ_ONLY_TOOLS`, `HEADLESS_DENIED_TOOLS` |
| `crates/horch-core/src/evaluation/scheduler.rs` | `JudgeJobSpec`, `Heartbeat`, `JobExit`, `ExitReason`, `JobState`, `JobFacts`, pure `decide`, `job_facts`, `discover`, `dataset_exe`, `job_args`, `schedule`, `kill_job` |
| `crates/horch-core/src/competition/judging.rs` | `JudgeEnv`, `JobLauncher`, `DetachedLauncher`, `JudgingStatus`, `start`, `poll`, fault point names, `DEFAULT_JUDGE_TIMEOUT` |
| `crates/horch/src/dataset/judge_job.rs` | the `judge-job` subcommand |
| `crates/horch/src/dataset/{cli,mod}.rs` | `JudgeJobArgs` and its dispatch line |
| `crates/horch-e2e/src/bin/fake-claude.rs` | judge mode |
| `crates/horch-e2e/Cargo.toml` | dev-dependencies `horch-core` and `anyhow` (workspace crates, orchestrator-approved; `check-deps.sh` passes unchanged) |
| `crates/horch-core/tests/judging.rs` | 8 core tests |
| `crates/horch-e2e/tests/judge.rs` | 8 e2e tests added next to U23's `jdg_01` |

## The judging API (for U30 and U32)

```rust
pub struct JudgeEnv<'a> {
    ctx, recorder: &JsonlRecorder, paths: &DatasetPaths, git: &dyn GitClient,
    repo: &Path /* main repo */, store: &ExecutionStore, judge: &Teammate,
    task_text: &str, policy: &WinnerPolicy, promotion: PromotionIntent,
    timeout: Duration, stale_after: Duration, launcher: &dyn JobLauncher,
    clock: &dyn Fn() -> DateTime<Utc>,
}
pub fn start(env: &JudgeEnv, round: &RoundId) -> Result<()>;
pub fn poll(env: &JudgeEnv, round: &RoundId, now: DateTime<Utc>) -> Result<JudgingStatus>;
pub enum JudgingStatus { Waiting, Decided(WinnerOutcome), NeedsIntervention }
```

- The coordinator gives `launcher: &DetachedLauncher(ctx)`. Tests give a
  launcher that runs the job in-process or synchronously.
- `start` refuses (error, nothing written) unless the round is
  JUDGING_BACKGROUND and every candidate is validated (CMP-12). On a round
  that has an attempt or a decision it does nothing.
- `poll` reads the projection each call. `Decided(Winner{label})` carries
  the ORIGINAL round label, not the bundle label. `Decided(Rejected{..})`
  follows `winner.rejected`. `NeedsIntervention` follows
  `round.needs_intervention{source: judge}` or `judge.failed` on attempt 2.
- `poll` also schedules attempt 1 if `start` was never called.
- `now` must be wall-clock time, never a pinned `HORCH_NOW`: it is compared
  with heartbeats and file times.
- Zero eligible candidates is the caller's case (`winner.rejected{no_eligible}`,
  judge skipped). The bundle holds every validated candidate; only eligible
  ones can win.

## The job dir contract

`jobs/<round>/judge-<n>/`:

| File | Writer | Content |
|---|---|---|
| `job.log` | coordinator (`schedule`), before the spawn | the job's stdout and stderr |
| `heartbeat` | job, every 2 s | `{pid, at}`, wall-clock |
| `output.json` | job, once, `create_immutable` 0600 | the answer text, ≤ 1 MiB |
| `exit.json` | job, once, last | `{reason: ok|crash|timeout|too_large, code}` |

- The job takes the answer out of the `--output-format json` envelope:
  `structured_output` when present, else `result`. An `is_error` envelope is
  a crash. Stdout that is not an envelope goes to `output.json` as it is, so
  the strict parser records it as `malformed`.
- The job reads its stdout up to 2 MiB (answer plus envelope). Over that, or
  an answer over 1 MiB, gives `too_large` and no `output.json`.
- The job derives its job dir from `HORCH_STATE_DIR` and
  `HORCH_PROJECT_DIR`, which `schedule` sets. It refuses to run twice.

## Retry and discovery rules

`discover` (pure `decide` over `JobFacts`):

| Facts | State |
|---|---|
| `output.json` exists | OutputPresent (even with an `exit.json`) |
| `exit.json` exists | Exited (an unreadable one is a crash) |
| heartbeat fresh and pid alive | Running{pid} |
| heartbeat stale or pid dead | Lost |
| `job.log` younger than `stale_after`, no heartbeat | Running{pid: 0} |
| `job.log` older, no heartbeat | Lost |
| no `job.log` | NotStarted |

`poll` actions:

| State | Action |
|---|---|
| NotStarted | the coordinator stopped between `judge.scheduled` and the spawn: spawn the same attempt |
| Running | `judge.started{pid}` once, record Running; overdue (timeout + 2 × stale_after after the spawn) → kill, `judge.failed{timed_out}` |
| OutputPresent, valid | `judge.started` if not seen, `judge.completed`, record Done, `judgements/<round>.json`, fault `abort-after-judgment-written`, `decide_winner`, winner event, fault `abort-after-winner-selected` |
| OutputPresent, invalid | `judge.failed{malformed}` (`over_cap` for `TooLarge`) |
| Exited | `judge.failed{crashed{code}|timed_out|over_cap}` |
| Lost | kill the heartbeat's process group, `judge.failed{lost}` |

After `judge.failed`: attempt 1 → schedule attempt 2 at once (status
Waiting); attempt 2 → NeedsIntervention (the fold moves the round). A
restart after `judge.failed` and before the retry schedules the retry. A
restart after `judge.completed` rebuilds the same judgment record bytes from
the same output and events; `create_immutable` accepts them.

Idempotency keys: `judge.scheduled:<round>:<n>`, `judge.started:<round>:<n>`,
`judge.completed:<round>:<n>`, `judge.failed:<round>:<n>`, `winner:<round>`
(selected or rejected), `needs_intervention:<round>`.

Fault points: `abort-after-judge-scheduled` (after the event and the spawn),
`abort-after-judgment-written`, `abort-after-winner-selected` (coordinator);
`abort-in-judge-job-before-output`, `abort-after-judge-output` (job). All use
`Faults::abort_if` (exit 86).

## The judge execution record (JDG-09)

One `Execution` per attempt, `kind: ExecutionKind::Judge{round, attempt}`
(ledger: kind `worker`, `round_id`, `label` `judge:<n>`, no experiment),
teammate `judge`, harness claude, model and effort from the teammate, role
`judge-<round8>-<n>`, workdir = the bundle, no pane, a minted UUIDv7 session
id that the job passes as `--session-id`. Status: Planned at insert,
Starting after the launch (so `recover_abandoned` never closes it), Running
at `judge.started`, then Done or Failed (`AgentExited{code}` for a crash or
a bad answer, `TimedOut`, `Crashed` for lost).

## Decisions and deviations

- `headless_command` returns `Result<Command>` and takes the schema TEXT:
  `claude --help` shows `--json-schema <schema>` takes the JSON itself, not
  a path. It refuses a harness without `headless`, a teammate with no tools,
  and any tool outside Read, Grep, Glob. The denied list is
  Agent, Edit, Write, NotebookEdit, Bash plus the teammate's own. It passes
  `--tools` and `--allowedTools` (same list), `--mcp-config
  {"mcpServers":{}} --strict-mcp-config`, `--disable-slash-commands`, and a
  `--settings` overlay that switches off the operator's plugins.
- `judge_prompt` reads the teammate's `persona` field (the body;
  `judge.md` has no base).
- `JobState::Exited` carries the whole `JobExit`, and `JobState::NotStarted`
  exists. The design had `Exited { code }` only.
- `JudgeJobSpec` has a `session` field.
- `schedule` reaps the child in a thread; a zombie would read as alive.
- `judge-job` takes `--model` and `--effort` and overrides the teammate, so
  the run matches `judge_policy_digest`.
- CMP-16 scan allowlist (orchestrator-approved): `runtime/process.rs`
  (defines `spawn_detached`), `workspace/arrange.rs` (grid tidy),
  `evaluation/validator.rs` (`process_group` to kill a gate at its timeout;
  it is waited for), `horch-e2e/src/bin/fake-herdr.rs` (fake), and
  `evaluation/scheduler.rs`. The test fails on a new file and on a stale
  entry. No competition, measure or dataset command code detaches.

## Tests added: 17

- Core `tests/judging.rs`: `jdg_08_restart_discovers_job`,
  `jdg_04_parent_writes_judgment_atomically`,
  `jdg_04_resume_after_completed_writes_once`,
  `cmp_12_judge_waits_for_terminal_set`, `jdg_09_judge_execution_in_ledger`,
  `jdg_08_second_failure_needs_intervention`,
  `jdg_09_schedule_detaches_and_strips_env`, `jdg_08_overdue_job_times_out`.
- E2E `tests/judge.rs`: `jdg_04_job_writes_only_job_dir`,
  `jdg_08_crash_records_and_retries`, `jdg_08_timeout`,
  `jdg_08_oversize_answer_is_over_cap`, `jdg_09_argv_and_env`,
  `sec_04_judge_cwd_bundle_tools_readonly`,
  `sec_08_e2e_no_api_key_in_judge_job`, `cmp_16_only_judge_detached`.
- Unit: 4 in `headless.rs`, 2 in `scheduler.rs`, 1 in `judge_job.rs`.

The e2e tests run the real `multi-herdr-dataset judge-job` binary and
fake-claude in the sealed harness. The coordinator side runs in-process
(`start`/`poll`), because the B3 `run`/`resume` loop is not merged yet.

## Not done (needs U30)

- `jdg_e2e_round_decided` through `run` and `resume`, and the B4 fault
  points in `cmp_13_crash_every_boundary`. I add them after U30 merges.
- `run` does not wait for the job yet; U30 wires `start` and `poll`.

## Gotchas for U30 and U32

- Use `DetachedLauncher(ctx)`. `schedule` needs the dataset binary: it is
  the current exe, or its sibling `multi-herdr-dataset` next to the current
  exe. A test process is neither.
- The job inherits the coordinator's whole environment (minus
  `FORBIDDEN_ENV`); `HORCH_CLAUDE_BIN`, `HORCH_TEAMMATES_DIR` and
  `HORCH_FAULT` reach it that way.
- Pass wall-clock `now` to `poll`. Use `DEFAULT_STALE_AFTER` (30 s) and a
  timeout from config (`DEFAULT_JUDGE_TIMEOUT` is 20 min).
- `horch sessions` lists the judge records as workers until U30 hides
  `kind: judge`.
- A bundle `Conflict` (a partial bundle with different bytes) is an error
  from `start`/`poll`. The coordinator must stop with NEEDS_INTERVENTION,
  not retry. `round.needs_intervention{source: judge}` is refused by the
  fold before a judgment, so use `source: operator` or keep the error.
- The bundle dirs are 0500. Cleanup must chmod them first.
- `--json-schema` is used when `claude --help` lists it (cached per
  process). LA-8 must confirm the real envelope's `structured_output` key.

## SPEC-RESOLVED (closed: ai_docs/designs/2026-10-02-dataset-competition-design.md §3-§11, ai_docs/reports/finish/spec-b-preflight.md)

None new. The judge prose (`teammates/judge.md`, Spec B §10) and the
Judgment schema (Spec B §11) remain open from U23 and U15.

## Outside my scope (not fixed)

- `crates/horch-core/tests/execution_store.rs:15` still has the unused
  `KIND_ORCHESTRATOR` import warning.
