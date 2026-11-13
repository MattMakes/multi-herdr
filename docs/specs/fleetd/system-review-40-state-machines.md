# fleetd systems review 40: the state machines

Scope: `docs/specs/fleetd/design.md` at commit `3b79d9ea` (1,306 lines). Lens: the state machines. Line numbers are lines of that file.

## Verdict

NOT SOUND

The idea, worktree, task branch and question machines have state lists that agree with their `CHECK` lists, and the ledger mechanics (1 transition door, rows before resources, verified rescue, fast-forward only) are correct. But the task machine and the feature machine have no transition table. The judged task and its judge task wait on each other, so no judgment task can reach `done` as written. Several cross-machine rules (task `done` against branch `landed`, 1 sub-orchestrator per feature against 1 live attempt per idea, PR merge against in-flight ideas) give a builder 2 readings, and the literal reading stops work.

## Answers

**Q1. The state machines, their owners, exits and time limits.** The design has 8 stored machines and 2 derived ones (table below). The task machine (12 states) and the feature machine (4 states) have no prose table: the task diagram is a placeholder (1016) and the feature has no diagram. States with no exit: task `failed`, `escalated` and `cancelled`; idea `parked` and `cancelled`; feature `merged` and `abandoned` (no command); question `escalated`; worktree `landing` when its branch does not land. States with no owner that can act: task branch `conflicted` (its owner is an exec worker that has exited) and feature `abandoned`. Lines: 511, 519-522, 543-544, 560, 578-580, 592-594, 623, 763-773, 828-837, 890-899, 965, 1016.

| Machine | States (lines) | Owner | Exit events and limits | No exit or no owner |
| --- | --- | --- | --- | --- |
| Idea | 10 (519-522, 763-773) | dispatcher, sub-orchestrator, you, fleetd | files validate (765-771); git shows the feature in primary and every worktree is removed (772); 7 days waiting on you to `parked` (796). No limit for `clarifying` without an open question, `specifying`, `ticketing` or `integrating`; `phase_attempts` (528) has no bound. A refused `--direct` plan goes to `needs-you/` with no state change (759). | `parked`, `cancelled`: no unpark or restore command (773, 1266 lists `park` only) |
| Feature | 4 (511) | fleetd, you | `ready` when the full gate passes (672, 931); `merged` when git shows it in primary (934). Silent: `ready` back to `open`, who sets `abandoned`, any limit. | `abandoned`: no command, no owner |
| Task | 12 (543-544) | fleetd, verifier, sub-orchestrator | `pending` to `ready` (996); `ready` to `leased` (997); lease expiry to `stalled` (646); exit with result to `verifying` (1002); verifier to `done` or `ready` (265, 982); quota to `waiting_quota`, out on pool reset (646, 973). Silent: `leased` to `running`, entry to and exit from `blocked_on_user`, `failed`, `escalated`, `cancelled`; the state during backoff (980) and replanning (289, 980). | `failed`, `escalated`, `cancelled`; the "attempt-failed state" (1018) is not in the `CHECK` list |
| Attempt | outcome only (560) | wrapper, fleetd | start timeout 90 s (701, 1014), `max_minutes` (710), stall ladder rungs (975-980), 256k context (263). | outcome `no_result` (979) is not in the list at 560 |
| Agent state (derived) | 6 (965) | fleetd | `hung` at `stall_after` 300 s; `idle_no_result` doorbells at 0 and 2 min (975-977). | no state for a usage limit, which RR-11 (160) needs |
| Task branch | 8 (592-594, 890-899) | worker, fleetd, sub-orchestrator, operator | `due_at` (596, 888): past it the row escalates. No `due_at` value is given for any state. | `conflicted`: owner is "the same worker" (897), an exited exec process; `in_bundle` when the feature tip moves (895) |
| Worktree | 8 (578-580, 828-837) | fleetd | `reserved` dropped after 1 min (845); git scan every 60 s (841). No other limit. | `landing` when the branch is `conflicted`, `gate_failed` or `abandoned` (833); `orphaned` when rescue verification fails (861); `rescued` against idea `merged` (836, 772) |
| Lease | row present, expired, paused (563-569, 646) | fleetd | 120 s, renewed only on earned activity (967); awake time only (969). | paused has no column in the schema (563-569) |
| Question | 3 (623) | sub-orchestrator of the feature (622) | `due_at` (626) to `escalated`; no default value; no pass step escalates it (986-1012). | `escalated`: no command records your answer |
| Gate run, trial merge | facts, not machines (602-616) | fleetd | a gate pass is stale once either tip moves (608, 931). | none |

**Q2. `CHECK` lists against prose tables.** Idea (519-522 against 763-773), worktree (578-580 against 828-837), task branch (592-594 against 890-899) and question `answer_case` (624 against 807-812) agree exactly. The task does not: `blocked_on_user`, `failed`, `escalated` and `cancelled` are only in the `CHECK` list (544), the diagram caption says 9 states against 12 (1016), and the "attempt-failed state" (1018) and "parks" (798, 802) are only in prose. Attempt outcomes `rejected`, `crashed`, `timed_out` and `killed` are only at 560, and `no_result` is only at 979. The feature has no prose table, and the idea diagram caption says 8 states against 10 (775).

**Q3. Terminal states and operator actions.** Terminal by statement: idea `merged` (772), task branch `landed` and `abandoned` (896, 899), worktree `removed` (837). Silent: task `done` and `cancelled`, feature `merged` and `abandoned`, idea `cancelled`, question `answered`. Operator actions: `requeue` resets the attempt count (1100) but its source states are silent; `revise` sends `awaiting_approval` to `clarifying` (767) and, after approval, pauses "only the affected tasks" (810, 818) with no rule for which rows reset; `park` and `cancel` rescue worktrees first (773) but do not say what happens to tasks, branches or questions; `abandon` has no command (899); `restore` makes a branch and a working tree (867) with no `branches` or `worktrees` row or state.

**Q4. Invariants.** There are 13 invariants (648-660), and I8 to I13 are the 6 no-loss invariants (915-922). None names its tables; each can be mapped (for example I3 reads `requirements`, `tasks`, `task_covers`), but I1, I5, I7 and I13 also need process and pane facts, I6 needs `git worktree list`, which is read every 60 s, not every pass (841), and I8 "by content" (655) is not monotonic, so it can turn false for a terminal state. I7 (654) contradicts FR-2 (134) and I13 (660) (finding S40-6), and I2 (649) with RR-3 (152) makes a judged task and its judge wait on each other (finding S40-1).

**Q5. Task `done` against task branch `landed`.** The design gives 2 orders. The verifier moves the task to `done` at the check (265, 893, 1018), so the task moves first and its branch is `ready`, not `landed`; Phase 4 says the verifier runs "check, sweeps, land" (1264), so the branch lands first. In the first order a reader sees task `done` while its code is not on the feature branch, and promote_pending (996) starts dependents on a base that does not have it (finding S40-4).

**Q6. `waiting_quota`, `blocked_on_user`, `stalled`, `escalated`.**

| State | Lease | Attempt burned | Worktree kept | Lines |
| --- | --- | --- | --- | --- |
| `waiting_quota` | paused | no | yes | 646, 973, 160 |
| `blocked_on_user` | silent; the task "parks with no process and no time limit" | silent; by rungs 5 and 6 an exit without a result burns 1 | yes, the resume or fresh worker uses the committed work | 798, 802, 814, 979-980 |
| `stalled` | expired | yes | yes, the next attempt uses the same tree | 646, 977, 980, 832 |
| `escalated` | silent | attempts are spent | silent; the tree becomes `orphaned` then `rescued` by 844 | 980, 1100 |

The sections agree on `waiting_quota` and `stalled`. They do not agree on how an exec worker reaches `blocked_on_user` or `waiting_quota`: the Stop hook (735) and finish_exited (1002) treat each exit without `result.json` as a failed attempt (finding S40-3).

**Q7. Races between rules of 1 pass.** Lease expiry against `result.json`: ingest (993) runs before renew_or_expire_lease (1000), which moves the task to `stalled` (646) before finish_exited (1002) sees the result, so the order decides it, and the expiry wins (finding S40-2). A question answered as its limit ends: no step of the pass (986-1012) escalates questions, starts their owner or resumes the worker, so the order does not decide it. A feature rebase while a bundle is in the fast gate: land_bundles (1006) runs before integrate_features (1007), but the gate runs across passes, and after the rebase `in_bundle` has no exit (finding S40-5).

## Findings

### S40-1 - CRITICAL - A judgment task and its judge task wait on each other

Lines: 152, 440, 649, 996, 274-282.

> For a judgment kind, its judge task shall report no open CRITICAL item. (152)
> after: [retry-doc] (440)

Failure scenario: The plan has `retry-doc` (kind `docs`) and `retry-doc-judge` with `after: [retry-doc]`. The worker of `retry-doc` writes its result, and the structural check passes. RR-3 (152) does not let `retry-doc` reach `done` until its judge reports. I2 (649) and promote_pending (996) do not let the judge become `ready` until `retry-doc` is `done`. Neither task moves. The verification of `retry-doc` ends at the check timeout as a failure, a new attempt starts, and after 3 attempts the task goes to replan and then to `needs-you/`. This occurs for every judgment task, including each `--direct` ticket of a judgment kind (FR-12). Its idea never reaches `integrating`. The CRITICAL fix round (278) also needs a fix task and a second judge task that no committed plan contains, and no rule says who adds them.

Smallest change: A judge task's dependency on the task it judges is met when that task passes its structural check. The judged task stays in `verifying` and moves to `done` when its judge task is `done` with no open CRITICAL item. On an open CRITICAL item, fleetd adds the fix task and the second judge task to the plan through plan validation.

### S40-2 - HIGH - The task machine has no transition table

Lines: 543-544, 645, 646, 980, 992, 1000-1002, 1016, 1018.

> All writes go through `transition(entity, id, to_state, cause)`, which looks up the allowed-transition table (645)
> [embedded content: task state machine · 9 states] (1016)

Failure scenario: A worker runs a long, silent test suite. 5 min after its last activity the lease stops renewing, and 120 s later it expires. In that pass the worker writes `result.json` and exits. ingest (993) records the result. renew_or_expire_lease (1000) moves the task to `stalled` (646), and rung 3 records outcome `stalled` (977). finish_exited (1002) then finds a valid result for a `stalled` task. The design does not say if `stalled` to `verifying` is legal. If the builder's table refuses it, `transition` raises (645) inside the single pass transaction (992), the pass rolls back, and the same raise repeats on each pass: fleetd stops for good. If the table does not refuse it, a builder can drop the result and burn an attempt. The same gap leaves `failed` with no owner, exit or limit while the sub-orchestrator "replans once" (289, 980), so a task and its idea can stop there.

Smallest change: Add a task state table with the columns of the worktree table (state, owner, leaves when, time limit) for all 12 states. Name the state that holds a task during backoff and during replanning. State that a valid result ingested in a pass wins over a lease expiry in the same pass. State that an illegal transition fails that row and records an event, not the whole pass.

### S40-3 - HIGH - An exit that is not a failure burns an attempt

Lines: 160, 735, 798, 802, 965, 979-980, 982, 1002.

> `Stop` hook: exit 2 with "run `horch result` first" while `result.json` is missing (735)
> Process exited without a valid result: outcome `no_result`. (979)

Failure scenario: A Claude Code exec worker writes a question with `fleet ask`, commits its work and tries to end its turn (802). The Stop hook refuses because `result.json` is missing (735). The worker writes a result, the check fails, and the task goes back to `ready` (982), or it loops until `max_minutes`. A Codex worker exits, and the wrapper's repair resume asks it for a result (982). With no result, finish_exited (1002) records `no_result` (979), and rung 6 starts a new attempt after backoff (980). The new attempt has no answer and asks again. After 3 attempts the task goes to replan and then to `needs-you/`, and the question flow of D13 never delivers an answer. The same path applies to an exec worker that exits on a usage limit: the agent states (965) have no usage-limit state, so the 3 attempts burn in about 11 min (backoff 30 s, 2 min, 8 min) against RR-11 (160).

Smallest change: Define 2 exits that are not failures. An exit with a valid question file ends with outcome `asked`, moves the task to `blocked_on_user` and burns no attempt. An exit with a usage-limit signal ends with outcome `quota` and moves the task to `waiting_quota`. The Stop hook and the repair pass accept a question file in place of `result.json`. finish_exited checks these 2 outcomes before rung 5.

### S40-4 - HIGH - Task `done` comes before branch `landed`, and dependents start without the dependency

Lines: 265, 416, 577, 649, 893, 996-997, 1006, 1264.

> Writing `verdict.json` and moving the task to `done` or back to `ready`. (265)
> fleetd runs the finish steps (check, sweeps, land) as the verifier (1264)

Failure scenario: The plan has `backoff` and `logging` with `after: [backoff]` (416). The verifier passes `backoff`: the task is `done` and its branch is `ready` (893). On the next pass promote_pending (996) moves `logging` to `ready`, and dispatch (997) leases its worktree with `base_sha` (577) from the feature branch tip. land_bundles (1006) runs later in the pass, and the bundle then needs a fast gate run of minutes. The worktree of `logging` does not have the backoff code. Its worker writes backoff again or fails its check and burns attempts. If both branches change `src/sync/client.py`, the bundle conflicts, and the branch goes to `conflicted`, which has no reachable exit (S40-5). A reader of the ledger sees `backoff` as `done` while its code is not on the feature branch.

Smallest change: I2 (649) and promote_pending count a dependency as met only when the task is `done` and its task branch is `landed`. Phase 4 (1264) names land as a step of Landing, not of the verifier.

### S40-5 - HIGH - The landing states `conflicted` and `in_bundle` have no reachable exit

Lines: 596, 608, 656, 739, 895, 897, 899, 901, 928-929, 1006-1007.

> | `conflicted` | the same worker | its rebase conflict is resolved (today's behavior) | (897)
> | `in_bundle` | fleetd | the bundle passes the fast gate and the feature branch moves forward | (895)

Failure scenario A: A task branch conflicts when fleetd builds the candidate. Its worker is an exec process that exited after its result (739), and its task is `done`, so dispatch never starts a worker for it. The branch waits until `due_at` and escalates. The only operator action on a branch is `abandoned` (899). The work stays on the branch but never lands, and the idea never reaches `integrating` (771).

Failure scenario B: A bundle is in the fast gate, which runs for minutes across passes. The primary branch moves, and integrate_features (1007) rebases the feature branch (929). The gate then passes on the old candidate. I9 (656) allows only a fast-forward, the candidate is not a descendant of the new tip, and the pass is stale (608). `in_bundle` has no other exit, so each branch of the bundle waits until `due_at` and escalates. On a busy primary branch this recurs for each bundle.

Smallest change: The owner of `conflicted` is fleetd. It opens 1 resolver task with the flow of line 928, and the branch goes back to `ready` when the resolver lands. When the feature tip moves under a candidate, fleetd drops the bundle and its branches go back to `ready`. Give a `due_at` value for each landing state.

### S40-6 - HIGH - Invariant I7 contradicts FR-2 and I13

Lines: 134, 262, 647, 654, 660, 765, 781, 882, 1008.

> I7: ... every idea from `clarifying` to `integrating` has exactly one live sub-orchestrator attempt or is waiting on you. (654)
> keeps 1 sub-orchestrator for each feature; 1 process never plans 2 ideas at the same time. (134)

Failure scenario: Idea I-1 of feature F-3 is `in_progress`, and its supervising process runs in the pane of F-3. You run `horch idea add --to F-3` (882). On the next pass the dispatcher moves I-2 from `captured` to `clarifying` (765). I7 now needs a live attempt for I-1 and for I-2, but FR-2 allows 1 process in 1 pane for 1 idea at a time (134, 781). check_invariants (1008) finds a breach on each pass and repairs or escalates (647). A repair either starts a second process (a breach of FR-2) or replaces the supervisor of I-1 with the interview of I-2. In the second case the worker questions of I-1 have no owner, and RR-12 starts the owner again on the next pass, so the 2 rules alternate. An escalation fires every 5 s. No state says where I-2 waits.

Smallest change: I7 counts per feature: each feature with an idea from `clarifying` to `integrating` has exactly 1 live sub-orchestrator attempt or waits on you. An idea stays `captured` while another idea of its feature is in `clarifying`, `specifying` or `ticketing`. The single process answers open questions before it starts a new phase.

### S40-7 - HIGH - The idea roll-up never completes after a cancelled task or a worktree that does not land

Lines: 651, 770-772, 811, 833, 835-836, 861, 865, 867, 899, 931, 980, 1009.

> | `in_progress` | ... | Every task is done. | (770)
> | `rescued` | ... | Adopted by the next attempt of its task, or removed when its idea is merged or cancelled. | (836)

Failure scenario: A worker question shows a flaw in the plan, and the sub-orchestrator replans the task (811). The old task is `cancelled` (I3, 651, counts only tasks that are not cancelled). Its branch can leave `building` only for `abandoned`, which is "operator only" (899). The idea needs "every task is done" (770) and every task branch `landed` (771, 1009), so it stays `in_progress`. The full gate runs only "when its task branches are landed" (931), so the feature never becomes `ready` and gets no pull request. The worktree of the cancelled task goes to `orphaned` and then `rescued` (835-836). A `rescued` tree is removed only when its idea is `merged` (836, 865), and the idea is `merged` only when every tree is removed (772, 867), which is a cycle. A tree in `landing` whose branch is `conflicted`, `gate_failed` or `abandoned` has no exit (833), and an `orphaned` tree whose rescue fails verification stays (861).

Smallest change: The roll-up counts only tasks that are not `cancelled` and branches that are not `abandoned`. A task cancel moves its branch to `abandoned` with owner fleetd, after a rescue ref. Idea `merged` requires each worktree to be `removed` or `rescued`, and fleetd then removes the `rescued` trees. Give `landing` and `orphaned` an exit to `rescued`.

### S40-8 - HIGH - The feature machine has no transitions, and a PR merge strands in-flight ideas

Lines: 161, 511, 647, 655, 660, 672, 883, 931, 933-934, 995, 1006.

> A feature is an intake funnel until its pull request merges. (883)
> I8: a task branch is `landed`, and a feature is `merged`, only when its changes are in the target branch by content. (655)

Failure scenario: Feature F-3 has I-1 in `integrating` and I-2 in `in_progress`. You merge the pull request (933). On the next pass fleetd reads git (934) and marks F-3 `merged`. keep_sub_orchestrators (995) and I13 (660) stop its sub-orchestrator, and RR-12 (161) starts it again when an I-2 worker asks a question, so the 2 rules alternate on each pass. land_bundles (1006) has no check for a merged feature, so I-2's branches land on a feature branch whose pull request is closed, and fleetd opens no new pull request. I8 is checked on each pass (647), the feature's changes are no longer all in the primary branch, and no legal transition leaves `merged`. The same gap exists for `ready`: a later landing moves the tip (931), and no rule moves the feature back to `open`. `abandoned` has no command and no owner, so a feature whose ideas are all cancelled keeps a sub-orchestrator for ever (I13).

Smallest change: Add a feature state table. When the pull request merges, each idea of the feature that is not `integrating` moves to a new feature, and its later task branches land on the new feature branch. fleetd decides `merged` once, at the merge, and does not check it again. Add `ready` to `open` on a tip move, and a command `horch feature abandon` that you own.

## Checked and sound

- 1 transition function writes the row and its event in 1 transaction and refuses an illegal move (645, 630-636, OR-2 at 188).
- Ingestion is idempotent by path and hash, and it runs first in the pass (638-640, 993).
- The whole pass runs in 1 SQLite transaction, so a crash leaves no half-applied state (992, FM-11 at 1223).
- A worktree row exists before its directory, and a `reserved` row with no directory is dropped after 1 min (571, 830, 845).
- Rescue uses a private index, does not touch the branch or index of the tree, and is verified before any removal (848-861, I6 at 653).
- A branch row exists before the branch, and a branch found in git with no row is adopted, never deleted (588, 658, 920).
- A branch moves only by fast-forward to a gated candidate, and a rescue ref of the old tip exists before each rebase (656-657, 918-919).
- A gate pass is stale once either tip moves (608, 931).
- Lease renewal comes only from outside signals, and sleep time does not count (646, 967, 969).
- Approval is recorded only by fleetd with the hash you saw, and I7 checks the hash on each pass (654, 794).
- A plan commits all-or-nothing after schema, DAG and coverage checks (769, FR-3 at 135).
- The idea, worktree and task branch `CHECK` lists match their prose tables state for state (519-522 and 763-773; 578-580 and 828-837; 592-594 and 890-899).
- The 4 `answer_case` values match the 4 answer cases (624, 807-812).
- Each state name is written with its entity, and the 5 shared names have 1 meaning each (664-674).
- A failed bundle is bisected, only the failing branch goes to `gate_failed`, and the others go on (901).
- The ownership check under the land lock keeps a cancelled task from landing (907).
- Retries are bounded: max attempts with backoff, 1 replan, then `needs-you/` (980, RR-6 at 155).
- A usage limit on both pools waits and never moves to a third persona (973).
