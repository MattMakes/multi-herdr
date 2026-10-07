# Design: the fleet dataset - run facts for every worker, and a competition startup mode

| | |
|---|---|
| Status | Approved for build, 2026-10-07. |
| Date | 2026-10-07 |
| Author | The fleet orchestrator |
| Builds on | [dataset-competition.md](dataset-competition.md) (the `multi-herdr-dataset` round machinery, `DatasetPaths`, the System One export), [telemetry.md](telemetry.md) (transcript readers, the price table). |

---

## 1. Problem

On 2026-10-07 the operator asked whether the fleet records data that a System
One decision model (Clef, later Laya) can learn model selection from. It does
not:

- Ordinary fleet work (`horch fleet`, the `herdr-fleet` launcher) writes
  nothing to the dataset store. `measure::recorder::NoopRecorder` is "normal
  horch mode".
- Only `multi-herdr-dataset run` records, and nobody runs it during real work.
  The 5 project stores under `~/.local/state/horch/multi-herdr/` hold toy
  rounds (`noop`, `slugify`, `READY`) from live acceptance checks. Readiness is
  `not_ready`: 0 of 50 judged rounds, 1 of 30 distinct tasks.
- The facts of every single worker run already exist, but they are scattered:
  the ledger holds task, teammate, harness, model, effort, phase, routing,
  skills, times and the DONE summary; the transcripts hold tokens. Nobody joins
  them into rows. Claude Code deletes transcripts after 30 days by default, so
  the token facts are lost if nobody snapshots them.

The operator's words: "The orchestrator was meant to have a startup mode that
would enact workers to do similar work with different configurations to test
and record outcomes. But we should also not lose current tasks. Knowing
whether it was higher or lower quality may not be discernable from a single
run by itself, but other facts like, what task, model, and effort was
selected, how long did the run take, how many tokens were used, etc should be
gleanable from the results of even single workers."

## 2. Operator decisions

| # | Decision |
|---|---|
| FD1 | **Run facts are always on.** Every fleet worker gets a run-fact row, under both launchers (`herdr-fleet` and `herdr-run`). Recording makes no model call. |
| FD2 | **Competition is a fleet startup mode.** `horch fleet --compete` adds the competition rules to the orchestrator's briefing. The `herdr-fleet` launcher passes `--compete`. The `herdr-run` launcher is the launcher as it was before this spec: no `--compete`. |
| FD3 | **The orchestrator picks which units compete.** Only a self-contained unit with a deciding gate competes. A fleet session runs at most `--compete-rounds` rounds (default 3), and each round has a hard ceiling of `--compete-budget-usd` (default 10). |
| FD4 | **The real task is never lost.** The round's winner lands on the working branch. A round that ends without a winner, or that preflight refuses, is followed by a normal spawn of the same unit. |
| FD5 | **Inform, never dictate.** No run fact, verdict, export or readiness count changes routing, teammate choice, effort or any plan. The decision model stays inert (`teacher::inert::Inert`, dataset-competition OD4). |
| FD6 | **No API key.** Every child that runs `claude` runs with `ANTHROPIC_API_KEY` removed, as everywhere in horch. Nothing in this spec reads, sets or passes it. |

## 3. Storage

All files live in the project's dataset directory,
`$HORCH_STATE_DIR/multi-herdr/<project-slug>/` (`DatasetPaths`,
dataset-competition OD3), in a new subdirectory:

```
<dataset dir>/fleet/
  fleet.lock        DirLock for every append below
  starts.jsonl      one mh.fleet-start/1.0.0 row per spawn
  runs.jsonl        one mh.fleet-run/1.0.0 row per finished ledger record
  verdicts.jsonl    one mh.fleet-verdict/1.0.0 row per `horch verdict`
```

- `DatasetPaths::fleet_dir()` returns `<dataset dir>/fleet`. The directory is
  0700 and every file is 0600, like the rest of the dataset store.
- Every file is append-only JSONL. A reader skips a torn last line and counts
  it, like `JsonlRecorder::read_all`.
- `runs.jsonl` holds at most 1 row per `(record_id, segment)`. A second write
  for the same pair is a no-op (the writer checks under the lock). A record
  that `horch spawn --resume` reopens finishes again as a new segment.
- The fleet stream is separate from `events/`. The round projection folds every
  event kind into a round (`measure/projection.rs`), and a fleet run is not a
  round.

### 3.1 `mh.fleet-start/1.0.0` (written by `horch spawn`)

| Field | Type | Meaning |
|---|---|---|
| `schema` | string | `mh.fleet-start/1.0.0` |
| `record_id` | string | the ledger record of the spawn |
| `at` | RFC 3339 | when the row was written |
| `base_sha` | string or null | `git rev-parse HEAD` in the project at spawn; null outside a git repository |
| `plan_path` | string or null | the plan file the task names (rule below), relative to the project |
| `plan_digest` | string or null | `sha256:<hex>` of the plan file bytes at spawn |
| `plan_text` | string or null | the plan file text, at most 65 536 bytes (cut on a UTF-8 boundary) |
| `plan_truncated` | bool | true when `plan_text` was cut |

The plan rule: the first whitespace-separated token of the task text that,
with trailing `.,;:)"'` removed, ends in `.md` and names an existing regular
file (absolute, or relative to the project directory). No such token gives
nulls.

### 3.2 `mh.fleet-run/1.0.0` (written at the end of a record)

| Field | Type | Source |
|---|---|---|
| `schema` | string | `mh.fleet-run/1.0.0` |
| `record_id`, `session_id`, `role` | string | ledger record; `session_id` as it is when the row is written |
| `segment` | integer | 1 for the first run of the record; 1 more for each resume. Counted from the record's spawn and resume history events up to the finish. Rows written before segments existed read as 1. |
| `kind` | `worker` or `orchestrator` | ledger record; candidate and judge records get no row (section 4.3) |
| `project`, `project_slug` | string | ledger record, `DatasetPaths` |
| `teammate` | string | ledger `tier` |
| `harness`, `model`, `effort`, `phase` | string or null | ledger `agent`, `model`, `effort`, `phase` |
| `routing` | object or null | ledger `routing`, copied as is |
| `substitution_reason` | string or null | ledger |
| `skills_expected` | array | ledger `skills` (`id`, `version`, `digest`) |
| `task`, `task_digest` | string | ledger `task`; `sha256:` of its UTF-8 bytes |
| `plan_path`, `plan_digest`, `base_sha` | string or null | the record's start row; null when there is none |
| `started_at`, `finished_at` | RFC 3339 | the segment's start (ledger `created_at` for segment 1, else the time of the resume history event) and the ledger `finished_at` (else the last `updated_at`). `horch done` stamps `finished_at`. |
| `duration_ms` | integer | `finished_at - started_at` of the segment |
| `end` | object | `status` (the ledger `status` string), `state` (the ledger `state` object as is), `exit_code` (or null) |
| `done_summary` | string or null | text of the last `done` history event |
| `notes` | integer | count of `note` history events |
| `resumed` | bool | true when the record resumed an earlier session |
| `tokens` | object or null | `input`, `output`, `cache_read`, `cache_write_5m`, `cache_write_1h`: the totals of the `session_id` transcript at `usage_at`, so a resumed session's later segment includes the earlier ones; null when no transcript is found |
| `cost_microusd` | integer or null | `UsageMeter::price` |
| `cost_source` | string | `price_table@<date>` or `unpriced` |
| `transcript_ref`, `transcript_digest` | string or null | path and `sha256:` of the transcript; never a copy (SEC-03) |
| `usage_at` | RFC 3339 | when tokens were read. A row written by `horch done` misses the tokens of the worker's last turn. |
| `head_sha` | string or null | `git rev-parse HEAD` in the project when the row is written |
| `written_by` | `done` or `sync` | which path wrote it |
| `horch_version` | string | `CARGO_PKG_VERSION` |

The token, cost and transcript fields come from the same code that prices a
dataset candidate: `competition::observe::TelemetryUsage` and
`competition::budget::UsageMeter`.

### 3.3 `mh.fleet-verdict/1.0.0` (written by `horch verdict`)

| Field | Type | Meaning |
|---|---|---|
| `schema` | string | `mh.fleet-verdict/1.0.0` |
| `record_id`, `role` | string | the judged worker record |
| `verdict` | `accepted`, `rework` or `rejected` | the orchestrator's check of the DONE |
| `note` | string or null | why, in one sentence |
| `at` | RFC 3339 | when it was recorded |
| `by` | string or null | the orchestrator's record id, when known |

- `accepted`: the work passed the orchestrator's check as delivered.
- `rework`: the work needed a fix - it went back to a worker, or a follow-up unit fixes it.
- `rejected`: the work was discarded.

A record can have many verdict rows. The last one counts.

## 4. When rows are written

### 4.1 At spawn

`horch spawn` appends the start row after the ledger record has a pane. A
failure prints one `horch: NOTE: fleet start row not written: <reason>` line
to stderr and never fails the spawn.

### 4.2 At the end of a record

- `horch done` writes the run row of its own record after it marks the record
  done and before it closes the pane. A failure prints one NOTE line to stderr
  and never fails `horch done`.
- **Sync** writes the run row of every finished record that has none: records
  that ended without `horch done` (pane closed, launch failed, abandoned), and
  records whose `horch done` could not write (for example a Codex sandbox
  that refuses the dataset directory). Sync runs:
  - in `horch spawn` for the current project, after the ledger reconcile,
    best effort, with the same NOTE-on-failure rule;
  - in `multi-herdr-dataset fleet sync` for the current project, or for every
    project with `--all-projects` (every ledger `$HORCH_STATE_DIR/*.json`);
  - at the start of `multi-herdr-dataset export` and `readiness`.
- A finished record is one whose ledger `status` is not `working` and whose
  `state` is terminal. A record that is still running gets no row.
- Backfill: `fleet sync --all-projects` turns every finished record of every
  ledger on the machine into a row. A record whose transcript is gone gets
  `tokens: null`, `cost_source: unpriced`.

### 4.3 What gets no row

- Candidate and judge records of a dataset round (`ExecutionKind::Candidate`,
  `ExecutionKind::Judge`). The round export already holds them.
- Records whose `status` is `working`.

## 5. `horch verdict`

```
horch verdict <role-or-record-id> <accepted|rework|rejected> [--note <text>]
```

- A role resolves to the newest record with that role in this project's
  ledger, in any state. A record id resolves to itself.
- It appends a verdict row and appends a `verdict` history event to the ledger
  record, text `<verdict>: <note>` or `<verdict>`, so `horch sessions` shows it.
- It refuses, with exit code 2 and nothing written: an unknown role or record,
  an orchestrator record, a word that is not one of the 3 verdicts.
- The orchestrator's briefing tells it to run `horch verdict` after it checks
  each DONE (core loop step 7).

## 6. Export and readiness

- `multi-herdr-dataset export` writes the decided rounds as today, and writes
  fleet rows to `exports/fleet-observed-1/<timestamp>.jsonl`: 1 row per
  worker record (`kind: worker`). A record with more than 1 segment gives 1
  row: `facts` is its latest segment row, plus `segments` (the count) and
  `active_ms` (the sum of the segment durations). Its tokens are the latest
  row's tokens per distinct `session_id`, summed over the distinct session
  ids, so a resumed session is not counted twice.
- The fleet export row:

  ```json
  {
    "api": "systemone/v1",
    "schema": "mh.export-fleet/1.0.0",
    "label_policy_version": "fleet-observed-1",
    "task_id": "<task_digest>",
    "state": {
      "task_digest": "...", "plan_digest": "... or null",
      "task_features": {"phase": "...", "routing_mode": "...", "resumed": false}
    },
    "questions": {
      "verdict": {"type": "choice",
                  "instructions": "Did the orchestrator accept this worker's result?",
                  "options": ["accepted", "rework", "rejected"]}
    },
    "answers": {"verdict": {"choice": "accepted", "confidence": null,
                            "probabilities": {"accepted": 1.0}}},
    "config": {"config_id": "<teammate>|<harness>|<model>|<effort or ->",
               "teammate": "...", "harness": "...", "model": "...", "effort": "..."},
    "facts": { "...": "the mh.fleet-run/1.0.0 row, without plan_text" },
    "teacher": {"id": "none", "probabilities": null}
  }
  ```

  `answers.verdict` is null when the record has no verdict. `config_id` uses
  the same format as a round's `config_id`.
- `multi-herdr-dataset readiness` prints its current output unchanged, then a
  `fleet observed:` section: run rows, rows with a verdict, distinct tasks,
  the verdict mix, and per `config_id` the runs and median duration. The
  readiness verdict and the Clef gaps count judged rounds only, as today.

## 7. Competition mode

### 7.1 The orchestrator's briefing

`horch fleet --compete [--compete-rounds N] [--compete-budget-usd X]` appends
`teammates/_base/fleet-compete.md` to the orchestrator's briefing, rendered
with `{compete_rounds}` and `{compete_budget_usd}`. Without `--compete` the
briefing is byte-identical to the briefing before this spec. The file is
prompt data: Rust only substitutes the 2 placeholders.

### 7.2 `multi-herdr-dataset run` additions

| Option | Meaning |
|---|---|
| `--plan <PATH>` | The plan file of the unit. When `<TASK>` is omitted, the task text is `Read and follow <PATH> exactly.` Preflight refuses (exit 4, nothing created) when `<PATH>` is not tracked at the base commit, or when the work-tree copy differs from it: the candidates start from the base commit and would not see the plan. |
| `--detach` | Run preflight in the caller. A refusal prints and exits 4 as today. On a pass, start the coordinator in a new pane of the round's dedicated workspace, print 1 line `round <round-id> started: workspace <name>, report to <pane>`, and exit 0. The coordinator pane is a visible herdr pane, not a background job (CMP-16 is unchanged). |
| `--report-to <PANE>` | When the round reaches a terminal state, the coordinator types 1 report line into `<PANE>` the way `horch tell` does. With `--detach` and no `--report-to`, the default is the caller's `HERDR_PANE_ID`. |

The report line, in STE:

```
[compete-<exp8>] DONE: round <round-id> is <state>. Winner: <config_id or none>. Promoted: <branch>@<sha or none>. Reason: <text or none>.
```

### 7.3 Promotion to a new branch

`--promote-to compete/<exp8>` names a branch that does not exist yet. The
promotion creates it at the winner's integrated commit. No checkout has it,
so PRO-03's dirty-checkout rule never applies. The working branch is never
the promotion target in competition mode, because the shared fleet checkout
is almost never clean.

### 7.4 Landing the winner (the orchestrator, by the briefing)

1. The orchestrator commits the unit's plan file (exact path) before the round.
2. It starts the round:
   `multi-herdr-dataset run --plan <path> --baseline <teammate> --candidates 2 --budget-usd <X> --allow-dirty --promote-to compete/<slug> --detach`.
   The baseline is the teammate the orchestrator would have spawned.
3. On the report line with a promoted branch, it runs
   `git cherry-pick HEAD..compete/<slug>` in the shared checkout: the
   commits on the round branch that the working branch does not have. On a
   conflict it runs `git cherry-pick --abort` and spawns a Claude worker with
   a plan to apply the winner's diff by hand. An orchestrator whose sandbox
   refuses git writes (Codex) spawns a Claude worker for this step.
4. On a report line without a winner, or on a preflight refusal, it spawns the
   unit normally with the baseline teammate.
5. It verifies the landed work like any DONE, and records `horch verdict` on
   the record that did the landing work, when one exists.

## 8. Launchers

| Launcher | File | Runs |
|---|---|---|
| `herdr-run` | `scripts/herdr-run` | `horch fleet "$@" --cwd "$PWD"`: the old `herdr-fleet` |
| `herdr-fleet` | `scripts/herdr-fleet` | `horch fleet --compete "$@" --cwd "$PWD"` |

`just install` installs both into `~/.local/bin/`.

## 9. Requirements

| ID | Requirement | Unit | Tests |
|---|---|---|---|
| FDS-01 | `DatasetPaths::fleet_dir()`; 0700 dir, 0600 files; appends under `fleet.lock` | F1 | fds_01_fleet_dir_modes_and_lock |
| FDS-02 | `horch spawn` writes a start row with `base_sha` and the plan rule of 3.1; a failure never fails the spawn | F1 | fds_02_start_row_plan_rule, fds_02_start_row_failure_is_a_note |
| FDS-03 | The run row has every field of 3.2, tokens and cost from `TelemetryUsage` and `UsageMeter` | F1 | fds_03_run_row_fields, fds_03_run_row_tokens_from_transcript |
| FDS-04 | `horch done` writes its record's run row; a failure never fails `horch done` | F1 | fds_04_done_writes_run_row, fds_04_done_write_failure_is_a_note |
| FDS-05 | At most 1 run row per `(record_id, segment)`; a resumed record that finishes again gets a new segment row | F1, F4 | fds_05_run_row_idempotent, fds_05_resumed_record_new_segment |
| FDS-23 | `horch done` stamps the record's `finished_at` | F4 | fds_23_done_stamps_finished_at |
| FDS-06 | Sync writes rows for finished records without one, skips working, candidate and judge records; `horch spawn` runs it | F1 | fds_06_sync_finished_only, fds_06_sync_skips_candidates_and_judges |
| FDS-07 | A record without a transcript gets `tokens: null`, `cost_source: unpriced` | F1 | fds_07_missing_transcript_unpriced |
| FDS-08 | Verdict core: resolve role or record, append the verdict row and the ledger history event; refuse unknown, orchestrator and bad words with nothing written | F1 | fds_08_verdict_resolves_and_appends, fds_08_verdict_refusals_write_nothing |
| FDS-09 | `multi-herdr-dataset run --plan` builds the task text and refuses an untracked or dirty plan before any worktree | F2 | fds_09_plan_task_text, fds_09_plan_untracked_or_dirty_refused |
| FDS-10 | `--detach` passes preflight in the caller, starts the coordinator in a pane and returns at once; a refusal still exits 4 | F2 | fds_10_detach_returns_after_start, fds_10_detach_refusal_exit_4 |
| FDS-11 | `--report-to` gets 1 STE report line at the terminal state | F2 | fds_11_report_line_on_terminal_state |
| FDS-12 | Promotion to a branch that does not exist creates it at the winner's commit | F2 | fds_12_promote_creates_new_branch |
| FDS-13 | `horch fleet` options carry compete settings; the briefing appends `fleet-compete.md` only with `--compete`; without it the briefing is byte-identical | F3 | fds_13_briefing_without_compete_unchanged, fds_13_briefing_with_compete_rendered |
| FDS-14 | `herdr-run` is the old launcher; `just install` installs it | F3 | fds_14_herdr_run_matches_old_launcher |
| FDS-15 | `multi-herdr-dataset fleet sync [--all-projects]` | F4 | fds_15_fleet_sync_cli, fds_15_fleet_sync_all_projects |
| FDS-16 | Export writes `exports/fleet-observed-1/` rows in the shape of section 6; `plan_text` is never exported; 1 row per record over its segments | F4 | fds_16_export_fleet_rows_shape, fds_16_export_fleet_latest_verdict_wins, fds_16_export_segments_one_row |
| FDS-17 | Readiness prints the `fleet observed:` section; the verdict and the Clef gaps are unchanged | F4 | fds_17_readiness_fleet_section, fds_17_readiness_verdict_unchanged |
| FDS-18 | Export and readiness sync first | F4 | fds_18_export_and_readiness_sync_first |
| FDS-22 | Nothing in this spec routes or decides from recorded data (FD5) | F1 | fds_22_routing_ignores_fleet_store |

### 9.1 Pending requirements (unit F5, not built yet)

These rows move into the table above when their tests exist. Until then the
coverage gate does not read them: this table's second header cell is not
`Requirement`.

| ID | Pending requirement | Unit | Tests |
|---|---|---|---|
| FDS-19 | `horch verdict` command; exit 2 on refusal | F5 | fds_19_verdict_cli |
| FDS-20 | `horch fleet --compete`, `--compete-rounds`, `--compete-budget-usd` flags; `herdr-fleet` passes `--compete` | F5 | fds_20_fleet_compete_flags |
| FDS-21 | The orchestrator briefing tells it to record a verdict after each checked DONE | F5 | fds_21_briefing_names_horch_verdict |

## 10. Build units

| Unit | What | Plan |
|---|---|---|
| F1 | Run facts core: `fleet` store, start row, run row, done hook, sync, verdict core | `ai_docs/plans/fleet-dataset/f1-run-facts.md` |
| F2 | `multi-herdr-dataset run --plan --detach --report-to`, promotion to a new branch | `ai_docs/plans/fleet-dataset/f2-compete-run.md` |
| F3 | Compete briefing wiring in `recipes.rs`, `herdr-run` launcher | `ai_docs/plans/fleet-dataset/f3-compete-briefing.md` |
| F4 | `fleet sync` CLI, fleet export rows, readiness section | after F1 and F2 |
| F5 | CLI registration (`main.rs`, `cmd/mod.rs`), orchestrator briefing, `herdr-fleet --compete`, `skills_loaded`, docs | after wave 3 releases `main.rs`, `cmd/mod.rs`, `teammates/_base/fleet-orchestrator.md`, `README.md` and `cmd/cost` |

## 11. Build status (2026-10-07)

| Unit | State | Commits |
|---|---|---|
| F1 | Done and verified | e20d0b1, cfcc1dc, f9d9f5b, def503e, 01452de |
| F2 | Done and verified | 2a5178b, 67e9385, 82c7b42, 314607d, 5244941 |
| F3 | Done and verified | 976c7a1, 6a9c2f7, 39de7de |
| F4 | Steps 1-6 committed; the DONE summary in `horch sessions` (backend-developer-2) says what is left | 4349dfb, e47d4bf, ef090ff |
| F5 | Not started. It waits for wave 3 to release `crates/horch/src/main.rs`, `crates/horch/src/cmd/mod.rs` and `teammates/_base/fleet-orchestrator.md`. | - |

Open items for F5:

- `horch verdict <role>` must prefer the newest record with that role in the
  caller's `workspace_id`. Several fleets share 1 project ledger, and role
  names repeat across fleets.
- Validate `--compete-budget-usd` with the dataset CLI's money parser, and
  require `--compete-rounds` of 1 or more.
- `skills_loaded` in the fleet export, from the cost report's skill use.
- Docs: README, `docs/fleet-workflow.md`, the dataset docs for `--plan`,
  `--detach`, `--report-to`, the `compete/` rule and the `STOPPED` state.
- After the gate: `just install`, agreed with the operator, then the real
  backfill `multi-herdr-dataset fleet sync --all-projects`.
