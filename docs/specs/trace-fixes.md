# Spec: trace fixes — making fleet work flow to completion

Status: approved plan, not started. Owner: the operator. Decisions D1-D20 are in
`ai_docs/plans/trace-fixes/decisions.md` (local), summarised in section 2.
Inputs: the trace `ai_docs/trace/fleet-work-lifecycle.md` (ids G1-G14, D1-D7, L1-L4, U1-U6) and the
as-is DDD model `ai_docs/ddd/` (coupling concerns CC1-CC13). Tickets and the map:
`docs/tickets/trace-fixes/MAP.md`.

## 0. How a fleet runs this plan (orchestrator)

1. Start the fleet with the light profile: `horch fleet --verify light`. Code review is not
   automated in this run (it is unbounded until ticket 18 lands); tickets 31 and 32 are the reviews.
2. `horch ledger run new trace-fixes`.
3. `horch ledger add-map docs/tickets/trace-fixes/MAP.md`.
4. `horch ledger plan-workers`, then `horch spawn <persona>` with no task text, exactly as many as it
   says. Run `horch quota` first; every persona here is Claude Opus (operator rule: no Sonnet).
5. End your turn. Wave 1 is 9 tickets with no blockers. When a ticket lands, `plan-workers` again.
6. Ticket 04 commits this spec and the tickets to the run branch. Ticket 33 runs the full gate.
   The operator merges the run PR (a merge to main publishes a release; ticket 30 writes the
   CHANGELOG line).
7. This run uses the tool it is fixing. Known traps until their ticket lands: a lost wake (14, 15),
   no reviewer auto-spawn (16), the greedy board (19), `done` refusing the compaction fresh route
   (23 — use `horch ledger done --unlanded` only as a last resort and say so).

## 1. Goal

Work that enters the fleet reaches an end state with no silent stop: a ticket queues as written, a
worker claims it within minutes, it is verified by a check it cannot skip, it lands, and the run
ends only after a recorded whole-run gate and a merged PR. Every message that can stop work is
durable, carries an id, and is followed up by the watchdog.

Targets after this plan (D18): pickup within 2 min of a task becoming startable; a stall noticed
within about 12 min; at least 90% of tasks finish with no operator help; 0 lost commits.

## 2. Decisions (summary)

| # | Decision |
|---|---|
| D1 | Pull is the only dispatch model. Retire `horch assign` and task-text spawns; one-offs are ledger tasks. |
| D2 | Tickets carry a fixed field block that `add-map` parses (contract over prose); `add-map --dry-run` is the backstop. |
| D3 | One "run settled" predicate for the wake, `ledger status` and `run close`. |
| D4 | `horch ledger run gate` records the whole-run check; `run close` requires it on the tip and the PR merged or declined. The operator merges. |
| D5 | A run default check that tickets inherit; no check means no land unless `**Check:** none`; shared tasks diff from their claim commit; complexity-sweep covers Rust. Profile default stays `light`. |
| D6 | horch respawns a dead orchestrator, rarely and under guards. |
| D7 | Wakes go through a transactional outbox into durable mail; the watchdog reconciles every 30 s. |
| D8 | Every mail line shows its id; answers close one question by id. |
| D9 | Review: at most 3 rounds, incremental, stable finding ids, reviewers see answers. |
| D10 | Retire the JSON session ledger; the task ledger is the single record. |
| D11 | horch spawns code-sweep reviewers itself. |
| D12 | `**Review:**` per ticket, defaulted by `**Kind:**`; at most 3 reviewers per round; cheaper rounds 2-3. |
| D13 | A run belongs to one workspace. |
| D14 | Specs in `docs/specs/`, tickets in `docs/tickets/`, both committed; `add-map` snapshots each ticket. |
| D15 | `horch ledger handoff` is the one handoff path (compaction and respawn). |
| D16 | Dead spots fixed as bugs; unlanded work is parked, not cancelled. |
| D17 | Remove competitive dataset rounds (`multi-herdr-dataset` and its modules). |
| D18-D20 | Housekeeping defaults; DDD model frozen as baseline; this plan. |

## 3. Contracts (new or changed messages)

These replace prose. Producers and consumers implement exactly these shapes; tests pin them.

### C1. Ticket field block (D2, D12, D5) — producer: ticket-writer; consumer: `add-map`

The block is the first lines after the `# NN: Title` heading. One field per line, in this order.
Unknown fields are an error under `--dry-run`. Values in backticks are literal.

```
<!-- horch-ticket: 1 -->
**Persona:** primary `<teammate>`, fallback `<teammate>` · **Parallel:** yes|no
**Kind:** feature|quality|refactor|security|docs|report
**Review:** none|light|full
**Check:** `<command>`            (or the word none, meaning "this ticket has no check")
**Own:** `<path>`, `<dir/>`        (omit only for report tickets)
**Do not touch:** `<path>`         (optional)
```

- `Persona` primary is required; fallback optional.
- `Kind` defaults `Review`: feature, docs and report → `none`; quality, refactor and security → `full`.
  An explicit `Review` wins. `light` = 1 round, at most 2 reviewers.
- `Check` absent → the run's default check (C5). `none` must be written to skip it.
- `Blocked by` stays a MAP column (keys of the same map or `T-<n>`). MAP has no Areas column.
- The version comment lets the parser refuse a future format instead of misreading it (CC13).
- `add-map` stores the ticket text and its sha256 on the task (`ticket_snapshot`, `ticket_sha`);
  `claim` prints the snapshot, then the path.

### C2. Mail line (D8) — producer: `horch tell`, `horch done`, wakes; consumer: an agent

```
[<role> #<mail-id>] <KEYWORD>: <text>
[<role> #<mail-id>] <KEYWORD>: <text> (redelivery <n>)
[orchestrator re #<question-id>] <text>          (an answer)
```

- `<mail-id>` is the id of the mail file (`pending/<id>.json`), shortened to 8 hex characters.
- A redelivery repeats the same id. A reader that has seen an id ignores it.
- Orchestrator mail goes through the channel relay first. The courier's redelivery and digest, and
  the `horch fleet` startup probe, are typed into the orchestrator pane only when the relay is
  down. An idle pane can hold the operator's draft, and typed text lands on it. The relay's answer
  confirms the startup probe: it needs no `horch inbox` and gets no redelivery.
- `horch tell orchestrator` from a role never types into the orchestrator pane while the relay
  answers a ping. When the live relay refuses or does not confirm the line, the mail stays pending
  without a first push, and the courier redelivers it through the relay. `horch tell` prints 1
  stderr line that says so. A tell with no sender has no mail, and it still types the line.
- `horch tell <role> --answers <question-id> "<text>"` closes exactly that question. A tell
  without `--answers` closes nothing.
- A horch-raised `QUESTION`/`BLOCKED` names the ledger action that answers it; that action closes
  it (for example `requeue T-7` closes "T-7 is held"). Otherwise horch sends it as `NOTE`.

### C3. Wake (D7, D3, D11) — producer: task ledger; consumer: orchestrator

Outbox row, written in the same SQLite transaction as the ledger change that causes it:

| column | meaning |
|---|---|
| `id` | uuid |
| `run_id` | the run |
| `event` | `startable`, `settled`, `reaped`, `held`, `cancelled`, `reviews-in`, `no-task`, `unclaimed-spawn`, `orchestrator-respawned`, `question` |
| `key` | dedup key: `<run>:<event>:<subject>[:<board-hash>]`, unique |
| `text` | the line for the orchestrator (STE) |
| `created_at`, `delivered_at` | delivery state |

A drainer (the courier and every watchdog pass) moves undelivered rows into durable mail (C2) and
sets `delivered_at`. A send failure leaves the row undelivered. `startable` wakes name the
`horch spawn` lines; `settled` fires only when `run close` would succeed (C6).

### C4. Handoff (D15, D6) — `horch ledger handoff <path>`

- Precondition: the caller holds a running task; `<path>` exists.
- Effect, in one transaction: worker ends `handed_off` with `handoff_path`; the task returns to
  `queued` at the front, holding its worktree; a `startable` wake is written.
- The replacement's `claim` prints `PRIOR WORK: read <path> first` before the ticket.
- The compaction "fresh route" and the orchestrator respawn both use this path.

### C5. Run check and run gate (D4, D5)

- `horch ledger run new <slug> --check "<cmd>"` sets `runs.default_check`.
- `horch ledger run gate` runs the run check (`--check` overrides) on the run-branch tip and writes a
  `run_gates` row: `run_id`, `tip_sha`, `command`, `exit_code`, `log_path`, `started_at`, `ended_at`.
- `horch ledger run close` refuses unless a passing gate exists for the current tip and the run PR is
  merged, or `--pr-declined "<reason>"` is given (stored in `runs.pr_declined_reason`). It names the
  missing item. `--abandon` is unchanged apart from stopping processes (TRF-36).

### C6. Run settled (D3)

A run is settled when every task is `done` or `cancelled`, no task is parked (held with a worktree),
and every worktree row is `cleaned`, `archived`, `trashed` or `scratch`. `ledger status` exits 0 only
when settled and lists every unsettled item; the `settled` wake fires once per settle; `run close`
uses the same function and adds only the C5 checks.

### C7. Orchestrator respawn (D6)

- Trigger: the registered orchestrator's agent PID is gone (watchdog), the run is active, and no
  deliberate stop is recorded (`horch fleet stop` or `run close` sets `orchestrator_stopped_at`).
- Guards: one orchestrator per workspace (lock); at most 1 respawn per 10 min and 3 per run;
  context-full or idle never triggers it.
- Action: `fleet_with` starts a fresh orchestrator whose first instruction names
  `ai_docs/handoffs/orchestrator-whats-next.md`, `ai_docs/plans/<run>/decisions.md` and
  `horch ledger status`. An `orchestrator-respawned` event goes to the ledger and to status.

## 4. Schema changes (ticket 03 does all of them, once)

One migration, so no 2 tickets race on `PRAGMA user_version`. Confirm each against the code first.

| table | change | used by |
|---|---|---|
| `tasks` | `work_kind TEXT` (C1 `Kind`; the existing `kind` column means work vs review), `review_mode TEXT`, `ticket_snapshot TEXT`, `ticket_sha TEXT`, `check_none INTEGER DEFAULT 0` | 10, 12, 18 |
| `tasks` | `held_reason` gains the value `unlanded` (parked) — no column | 21 |
| `runs` | `default_check TEXT`, `pr_declined_reason TEXT`, `orchestrator_stopped_at TEXT`, `orchestrator_respawns INTEGER DEFAULT 0`, `last_respawn_at TEXT`. Existing columns to reuse, not add: `workspace_id` (ticket 20 sets it at `run new` when run in a pane) and `orchestrator_worker_id` (ticket 24) | 12, 13, 20, 24 |
| `run_gates` | new table (C5) | 13 |
| `outbox` | new table (C3), unique `key` | 14, 15, 16 |
| `workers` | `handoff_path TEXT`; `end_reason` gains `handed_off` | 23 |

## 5. Timings and limits

| what | value | was |
|---|---|---|
| watchdog reconcile pass (board read, outbox drain) | 30 s | per worker check, 600 s |
| re-wake for a startable task with no starting/claiming worker | 2 min | never |
| flag a spawned worker that has not claimed | 3 min | pre-claim sweep, once |
| worker stall check | 600 s x 3 | unchanged |
| land lock stale age | max check timeout + 10 min | 1 h |
| review rounds | 3 | unbounded |
| reviewers per round | 3 | 2 + up to 10 |
| orchestrator respawn | 1 per 10 min, 3 per run | none |
| compaction cycles before handoff | 1 | unbounded |

## 6. Requirements

Each ID needs a test whose name starts with its lowercase form (`trf_07_...`); the gate's
`scripts/check-req-coverage.sh` enforces it.

| ID | Requirement | Ticket | Trace |
|---|---|---|---|
| TRF-01 | The schema migration upgrades an existing ledger with no data loss; new fields take their defaults. | 03 | — |
| TRF-02 | The ticket-writer template's example ticket and MAP parse under the C1 rules with no error. | 04 | G1, G2, U5 |
| TRF-03 | Every mail line carries its mail id, and a redelivery repeats the same id. | 05 | CC11 |
| TRF-04 | The courier does not exit while pending mail exists. | 05 | L4, connect Q2 |
| TRF-05 | A red check after a successful rebase in land resets the branch to its pre-rebase tip. | 06 | L2 |
| TRF-06 | A land-lock wait timeout resets the worktree row from `landing`; a lock is stale only after the max check timeout plus 10 min. | 06 | D7 |
| TRF-07 | complexity-sweep reports changed Rust functions over the cognitive-complexity threshold and passes when there are none. | 07 | G4 |
| TRF-08 | `horch context --over` lists every non-ok state, and each state has an instruction in the briefing. | 08 | D5 |
| TRF-09 | A session still over its threshold after 1 compaction is sent to the handoff route, not a new cycle. | 08 | L3, U3 |
| TRF-10 | `horch doctor` warns about `~/.claude/skills/herdr-*` links that are not the compiled briefing. | 09 | organise Q3 |
| TRF-11 | `add-map --dry-run` checks every ticket against C1, writes nothing, and names each error with its file and line. | 10 | G1 |
| TRF-12 | `add-map` stores each ticket's snapshot and sha; `claim` prints the snapshot even after the file changed or was deleted. | 10 | G7 |
| TRF-13 | `add-map` refuses a ticket already queued in the run unless `--again` is given. | 10 | contracts Q6 |
| TRF-14 | `Kind` sets the default `Review`; an explicit `Review` wins; `**Check:** none` is stored as an explicit no-check. | 10 | G3 |
| TRF-15 | One function decides "run settled"; the wake, `status` exit code and `run close` agree on a fixture of every worktree state. | 11 | D6 |
| TRF-16 | `run new --check` sets the run default check, and a task with no `Check` inherits it. | 12 | G3 |
| TRF-17 | `land` refuses a task with no check unless its ticket says `**Check:** none`. | 12 | G3 |
| TRF-18 | A shared-checkout task diffs from its claim commit, so code-sweep and complexity-sweep apply. | 12 | G5 |
| TRF-19 | `ledger run gate` runs the run check on the run-branch tip and records tip, command, exit code and log. | 13 | G6 |
| TRF-20 | `run close` refuses without a passing gate on the current tip, or while the PR is neither merged nor declined. | 13 | G6 |
| TRF-21 | A wake is written to the outbox in the same transaction as its ledger change and reaches durable mail even after a failed send. | 14 | G9, CC1 |
| TRF-22 | A task startable for 2 min with no starting or claiming worker gets exactly 1 re-wake per board state. | 15 | G9 |
| TRF-23 | A spawned worker that has not claimed within 3 min is flagged once. | 15 | D2 |
| TRF-24 | Code-sweep review tasks are spawned by horch through the spawn gate; the orchestrator gets a NOTE. | 16 | G10 |
| TRF-25 | `horch tell <role> --answers <id>` closes only that question; a tell without it closes none. | 17 | D4 |
| TRF-26 | A horch-raised question closes when the ledger action it names happens. | 17 | D4 |
| TRF-27 | Code-sweep runs at most 3 rounds; after round 3 an unresolved Important finding becomes a QUESTION. | 18 | L1 |
| TRF-28 | Rounds 2 and 3 review only the diff since the last reviewed tree. | 18 | L1 |
| TRF-29 | Finding ids are stable across rounds; answers and waivers carry; reviewers see `answers.md` and accept or reject each answer. | 18 | L1, U6 |
| TRF-30 | A round has at most 3 reviewers; `Review` none, light and full are honoured; rounds 2-3 use the cheaper reviewer model. | 18 | D12 |
| TRF-31 | The board reserves a slot only for tasks whose persona has a live or starting worker. | 19 | D1 |
| TRF-32 | A worker whose claim finds no task ends and the orchestrator is told. | 19 | D2 |
| TRF-33 | `spawn --resume` inside a run gets the claim instruction. | 20 | G11 |
| TRF-34 | A task-less spawn joins only its own workspace's run. | 20 | G12 |
| TRF-35 | `done --unlanded` parks the task with its worktree; dependents stay blocked on it; `requeue` resumes on the same worktree. | 21 | G13 |
| TRF-36 | `cancel` and `run close --abandon` signal the recorded PIDs of the task's worker. | 22 | G14 |
| TRF-37 | `ledger handoff` ends the worker handed off, requeues the task first holding its worktree, and the next claim prints the handoff path. | 23 | U3, D5 |
| TRF-38 | When the orchestrator's agent PID is gone in an active run, horch respawns it with the handoff, decisions and status. | 24 | G8 |
| TRF-39 | No respawn after a deliberate stop or a closed run. | 24 | G8 |
| TRF-40 | One orchestrator per workspace; at most 1 respawn per 10 min and 3 per run; each respawn is an event shown by status. | 24 | G8 |
| TRF-41 | Every spawn-gate decision (NOTE, SUBSTITUTED, REFUSED) is recorded as a ledger event and never changes routing. | 25 | define Q5 |
| TRF-42 | `horch ledger task` queues a one-off task in the workspace's solo run and creates that run when needed. | 26 | U1 |
| TRF-43 | `horch assign` is gone and `horch spawn` with task text refuses, printing the `horch ledger task` command. The 1 exception (T-56, PR #338) is a `spawn_only` controller's brief to a `horch_access: report` role, because a report pane cannot run `horch ledger claim`. | 27 | U1 |
| TRF-44 | `horch sessions` and every session reader read the task ledger. | 28 | U2 |
| TRF-45 | No code writes the JSON session ledger; `ledger import` still reads old JSON once; `done` and `note` are single commands. | 29 | U2, CC4 |
| TRF-46 | The briefings contain no push dispatch, and every `context --over` state has an instruction. | 30 | U1, U3, U4 |
| TRF-47 | `multi-herdr-dataset` and the competition, evaluation, dataset, fleet_runs, measure and teacher modules are gone; release archives hold `horch` and `herdr-install`. | 02 | D17 |
| TRF-48 | A cargo-built horch does not migrate the shared ledger; every migration of an existing ledger writes a backup and keeps the newest 3; the newer-schema error names the backup. | 35 | incident |
| TRF-49 | A repeating checkin failure prints once, then at most 1 line per 10 min, and 1 recovery line. | 35 | incident |
| TRF-50 | The pr step passes when the remote run branch holds the landed commit, even while GitHub's PR head lags. | 40 | D4 |
| TRF-51 | Each 30 s watchdog pass rebuilds every wake that a crash after commit can lose, deduplicated by its C3 key. | 46 | D7 |
| TRF-52 | `run close` never fails because the gate folder cannot be removed; a git error gives 1 NOTE and the run closes. | 51 | D4 |

## 7. Work breakdown

33 tickets in `docs/tickets/trace-fixes/`; the table is generated from them (see MAP.md). Every
ticket is `**Parallel:** yes`: the ledger serialises tickets whose `Own` paths overlap, and the
`Blocked by` edges order the ones whose behaviour depends on another.

<!-- generated-table -->

| # | Ticket | Persona | Wave | Blocked by | Requirements |
|---|---|---|---|---|---|
| 01 | [Verify the suspected behaviours and measure harness-adapter use](../tickets/trace-fixes/01-verify-suspects.md) | `opus` | 1 | — | report |
| 02 | [Remove competitive dataset rounds](../tickets/trace-fixes/02-remove-dataset.md) | `opus` | 1 | — | TRF-47 |
| 03 | [One schema migration for every new field](../tickets/trace-fixes/03-schema-next.md) | `opus` | 1 | — | TRF-01 |
| 04 | [Publish the ticket contract and make the writers produce it](../tickets/trace-fixes/04-ticket-contract.md) | `opus` | 1 | — | TRF-02 |
| 05 | [Mail ids on every line, and a courier that stays while mail is pending](../tickets/trace-fixes/05-mail-ids-courier.md) | `opus-hardening` | 1 | — | TRF-03, TRF-04 |
| 06 | [Land: reset after a red check, recover a timed-out lock](../tickets/trace-fixes/06-land-reset-lock.md) | `opus-hardening` | 1 | — | TRF-05, TRF-06 |
| 07 | [complexity-sweep covers Rust](../tickets/trace-fixes/07-rust-complexity.md) | `opus` | 1 | — | TRF-07 |
| 08 | [Compaction: show every state, and 1 cycle before handoff](../tickets/trace-fixes/08-compaction-single-cycle.md) | `opus` | 1 | — | TRF-08, TRF-09 |
| 09 | [doctor warns about stale ambient herdr skills](../tickets/trace-fixes/09-doctor-stale-skills.md) | `opus` | 1 | — | TRF-10 |
| 10 | [add-map reads the contract, dry-runs, snapshots and refuses duplicates](../tickets/trace-fixes/10-ticket-parser-snapshot.md) | `opus` | 2 | 03, 04 | TRF-11, TRF-12, TRF-13, TRF-14 |
| 11 | [One 'run settled' predicate](../tickets/trace-fixes/11-run-settled.md) | `opus` | 2 | 03 | TRF-15 |
| 12 | [Run default check, no silent no-check, shared diff base](../tickets/trace-fixes/12-run-check-default.md) | `opus` | 2 | 03, 06 | TRF-16, TRF-17, TRF-18 |
| 13 | [ledger run gate, and close guards](../tickets/trace-fixes/13-run-gate-close.md) | `opus` | 3 | 11 | TRF-19, TRF-20 |
| 14 | [Wakes through a transactional outbox](../tickets/trace-fixes/14-durable-wakes.md) | `opus-hardening` | 3 | 03, 05, 11 | TRF-21 |
| 15 | [Watchdog: 30 s reconcile and re-wake](../tickets/trace-fixes/15-fast-reconcile.md) | `opus-hardening` | 4 | 14 | TRF-22, TRF-23 |
| 16 | [horch spawns code-sweep reviewers](../tickets/trace-fixes/16-reviewer-autospawn.md) | `opus` | 4 | 02, 14 | TRF-24 |
| 17 | [Answers close 1 question by id](../tickets/trace-fixes/17-question-answers.md) | `opus` | 2 | 02, 05 | TRF-25, TRF-26 |
| 18 | [Bounded, incremental, cheaper code review](../tickets/trace-fixes/18-review-loop.md) | `opus` | 3 | 03, 10 | TRF-27, TRF-28, TRF-29, TRF-30 |
| 19 | [Board and claim dead spots](../tickets/trace-fixes/19-board-claim.md) | `opus` | 3 | 10 | TRF-31, TRF-32 |
| 20 | [Spawn: resume claims, and runs stay in their workspace](../tickets/trace-fixes/20-spawn-fixes.md) | `opus` | 5 | 02, 13, 16 | TRF-33, TRF-34 |
| 21 | [Park unlanded work instead of cancelling it](../tickets/trace-fixes/21-park-unlanded.md) | `opus` | 4 | 12, 19 | TRF-35 |
| 22 | [cancel and abandon stop the worker](../tickets/trace-fixes/22-cancel-stops.md) | `opus-hardening` | 5 | 21 | TRF-36 |
| 23 | [horch ledger handoff](../tickets/trace-fixes/23-ledger-handoff.md) | `opus` | 6 | 17, 22 | TRF-37 |
| 24 | [Respawn a dead orchestrator, rarely](../tickets/trace-fixes/24-orchestrator-respawn.md) | `opus-hardening` | 7 | 15, 23 | TRF-38, TRF-39, TRF-40 |
| 25 | [Record spawn-gate decisions](../tickets/trace-fixes/25-spawn-gate-record.md) | `opus` | 6 | 20 | TRF-41 |
| 26 | [horch ledger task: a cheap one-off](../tickets/trace-fixes/26-one-off-task.md) | `opus` | 6 | 20 | TRF-42 |
| 27 | [Retire horch assign and task-text spawns](../tickets/trace-fixes/27-retire-push.md) | `opus` | 7 | 17, 25, 26 | TRF-43 |
| 28 | [Every session reader reads the task ledger](../tickets/trace-fixes/28-sessions-from-ledger.md) | `opus` | 8 | 08, 09, 24, 27 | TRF-44 |
| 29 | [Stop writing the JSON session ledger](../tickets/trace-fixes/29-retire-json-ledger.md) | `opus` | 9 | 28 | TRF-45 |
| 30 | [Rewrite the briefings and docs for the new behaviour](../tickets/trace-fixes/30-briefing-rewrite.md) | `opus` | 10 | 07, 18, 29 | TRF-46 |
| 31 | [Review: delivery, supervision and respawn](../tickets/trace-fixes/31-review-supervision.md) | `codex-reviewer` | 8 | 17, 24 | report |
| 32 | [Review: session ledger retirement](../tickets/trace-fixes/32-review-ledger-retirement.md) | `codex-reviewer` | 10 | 29 | report |
| 33 | [Full gate on the run branch](../tickets/trace-fixes/33-final-gate.md) | `opus` | 11 | 30, 31, 32 | report |

Waves: 1: 9, 2: 4, 3: 4, 4: 3, 5: 2, 6: 3, 7: 2, 8: 2, 9: 1, 10: 2, 11: 1. Critical path: 03 → 10 → 19 → 21 → 22 → 23 → 24 → 28 → 29 → 30 → 33 (11 tickets).

## 8. Worker conventions (every ticket)

- Read this spec's section for your ticket, then your ticket. The ticket is the contract.
- Test first. Name each test for its requirement: `fn trf_<nn>_<what>()`. Put tests in the files
  you own (`#[cfg(test)]`) or in a new file `crates/<crate>/tests/trf_<nn>_<slug>.rs`.
- Edit only `Own` paths. If you need another file, send `QUESTION:` with a recommended option.
- A prompt or doc change belongs to ticket 30, except where your ticket owns the file. If your code
  changes a golden prompt, bless only that golden and name it in your report.
- Commit exact files. Never run `just gate` (ticket 33 does). Run your ticket's `Check` before
  `horch ledger finish`.
- Report to `ai_docs/reports/trace-fixes/<nn>.md`: what changed, decisions, what was verified only
  against a fake, follow-ups outside scope.
- Never read or set `ANTHROPIC_API_KEY`.

## 9. Out of scope

- Making `full` the default verify profile: revisit after ticket 18 lands and runs a while (D5).
- Re-running the DDD model as a to-be design (D19).
- Usage and cost reports: they inform, never route (operator rule).
