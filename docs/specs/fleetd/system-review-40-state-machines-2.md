# fleetd systems review 40, round 2: the state machines

Scope: `docs/specs/fleetd/design.md` at `HEAD` (commit `c8bf35ac`, 1,678 lines), and the change since `62337c29`. Lens: the state machines. Line numbers are lines of `design.md` at `HEAD`. The first report is `docs/specs/fleetd/system-review-40-state-machines.md`.

## Verdict

PASS
HIGH findings of the first report that are still open: 1 (S40-8). New HIGH findings: 4 (T40-1 to T40-4).

## Answers

- S40-1 (CRITICAL) closed: 314-325, 822, 484, 506-508, 1268, 1273. A judge task depends on the judged task through `judges:`, which is met when the judged task is in `verifying` and its structural check passed (I2, 822). The judged task waits in `verifying` and moves to `done` after its judge (1273, step 4 at 320). A fix round is a new attempt of each task and needs no plan change (step 5 and 6 at 321-322).
- S40-2 (HIGH) closed: 818, 819, 1262, 1264-1279. The task state table has all 12 states with owner, exit and limit. A valid result in the same pass wins over a lease expiry (819, 1262, 1272). An illegal transition fails only its row and puts it on the needs-you list (818). The backoff state is `ready` with `not_before` (1269), and `failed` has an owner, an exit and the limit `replan_limit` (1277).
- S40-3 (HIGH) closed: 914, 982, 1200, 1271. The Stop hook and the repair pass accept a question file (914, 982). Rung 5 finds the outcomes `asked` and `quota` before `no_result`, and neither burns an attempt (1200). The `running` row sends a usage limit to the fallback or to `waiting_quota` (1271).
- S40-4 (HIGH) closed: 822, 484, 1226, 1276. A normal dependency is met only when its task is `done` and its task branch is `landed` (I2, 822), and `promote_pending` uses I2 (1226). So the worktree of `logging` starts from a feature tip that has the code of `backoff`.
- S40-5 (HIGH) closed: 1094, 1102, 1141, 1086-1095. Scenario A: fleetd owns `conflicted` and starts a resolver attempt (1094). Scenario B: fleetd does not start a rebase of a feature branch while a bundle is in `gating`, and a moved tip drops a bundle in `building` or `gating` (1102, 1141). Each landing state has a time limit (1089-1095). 2 paths that the amendment opened are new findings: a resolver attempt that fails (T40-1), and a tip that moves after the gate passed (T40-2).
- S40-6 (HIGH) closed: 827, 945, 959, 984, 1224. I7 counts 1 live sub-orchestrator for each feature (827). I-2 waits in `captured` while another idea of the feature is in `clarifying`, `specifying` or `ticketing` (945). The 1 live process of the feature owns the open questions of the whole feature in any phase (959, 984).
- S40-7 (HIGH) closed: 950, 1134, 1089-1090, 1015, 1017, 1018, 1049. The roll-up skips cancelled tasks and abandoned branches (950, 1134). fleetd moves the branch of a cancelled task to `abandoned` (1089, 1090, 1279). An idea is `merged` when each tree is `removed` or `rescued`, and fleetd then removes the `rescued` trees, so the cycle is broken (1018, 1049). `landing` and `orphaned` have an exit to `rescued` (1015, 1017). Step 1 of the scenario, the cancel of a task in `blocked_on_user`, is now refused by the task table: that is new finding T40-3.
- S40-8 (HIGH) open: the step "on the next pass fleetd marks F-3 `merged`" still does not occur. The rule "A merge with ideas in flight" (1079) records a feature as `merged` while it has an idea that is not `integrating`, `cancelled` or `parked`. Such a feature is always `open`, because `open` moves to `ready` only when no such idea exists, and `ready` goes back to `open` when an idea is added (1072, 1073). The feature table gives `open` no exit to `merged` (1072). `transition()` refuses `open` to `merged` (818), so fleetd does not move I-2 to a new feature, and I-2's branches land on the feature branch of a merged pull request. Smallest change: add "git shows its changes in the primary branch by content → `merged`, and each idea in flight moves to a new feature (see A merge with ideas in flight)" to the `open` row at 1072.

## New findings

### T40-1 - HIGH - A resolver or fix attempt of a `done` task has no failure exit

Lines: 1098, 1094, 1095, 1201, 1276, 819.

> A resolver attempt and a fix attempt are attempts of the task row. The task stays `done`, because its own result was verified.

Failure scenario: Task branch `fleet/I-0007/T-0007-03` conflicts when fleetd builds its bundle, and the branch goes to `conflicted` (1092). Its task is `done`. fleetd starts a resolver attempt (1094). The resolver crashes, or its result fails its check. Rung 6 applies to each outcome that is not verified: it burns 1 attempt and moves the task to `ready` or `failed` (1201). The task table makes `done` terminal (1276), so `transition()` refuses the move and puts the row on the needs-you list (818). No rule says if fleetd starts a second resolver attempt. The `conflicted` row has only 1 exit, a resolver result that passes (1094). After 60 min the branch goes to the needs-you list (1085). No command leaves `conflicted`: `horch task requeue` needs a task in `escalated` (1278), and `horch feature abandon` drops the whole feature. The idea never reaches `integrating` (950). The same path applies to a fix attempt of a branch in `gate_failed` (1095). Also, rule 2 (819) and I1 (821) give a lease only to a task in `leased` or `running`, so a resolver attempt of a `done` task has no lease, and a hung resolver is not reaped by lease expiry.

Smallest change: In 1098, state the rule for a resolver or fix attempt: it is watched by the stall ladder and has a lease like a `running` task; rung 6 does not move its task; a failed resolver or fix attempt starts a new one after the backoff, up to `max_attempts` counted on the branch row; at that count the branch goes to the needs-you list, and `horch task requeue <task>` starts 1 more resolver or fix attempt.

### T40-2 - HIGH - A bundle in `passed` whose compare-and-set fails has no exit

Lines: 1100, 1102, 829, 1092, 693.

> When the tip of a feature branch moves while a bundle of that feature is in `building` or `gating`, the bundle goes to `dropped`, and its members go back to `ready`.

Failure scenario: Bundle 7 of F-0003 is in `gating`. The fast gate job ends with a pass. In pass N, `land_bundles` moves bundle 7 to `passed` and writes the ref move from its base tip to the candidate as an effect after the commit (1100, 1235). Before the effect runs, the feature tip moves. 2 causes are in the design: a commit by someone else, which I9 expects (829), and a rebase job that `integrate_features` queued in the same pass, because the bundle is no longer in `gating` (1141). The compare-and-set fails. fleetd moves nothing, records the new tip, makes the gate result stale and "decides again on the next pass" (829). The drop rule covers only `building` and `gating` (1102), so no rule moves a `passed` bundle. Its members stay `in_bundle`, whose exits need a landing, a `dropped` bundle or a `failed` bundle (1092). After the time limit the members go to the needs-you list, and no command moves them. The idea never reaches `integrating`. A builder who retries the compare-and-set on each pass makes the same failure, because the expected tip never comes back.

Smallest change: In 1102, add: "A bundle in `passed` whose compare-and-set fails goes to `dropped`, and its members go back to `ready`."

### T40-3 - HIGH - The task table has no exit to `cancelled` from a state in flight

Lines: 1268-1279, 991, 953, 1075, 818.

> | `blocked_on_user` | The live process of the feature that owns the question, then you | The answer is recorded → `ready`. ...

Failure scenario: The worker of T-0007-02 asks a question, and the task moves to `blocked_on_user` (982). The question shows a flaw in the plan, so the sub-orchestrator replans the task (991): the new plan replaces T-0007-02, and fleetd moves it to `cancelled`. The task table allows `cancelled` only from `pending`, `ready`, `failed` and `escalated` (1268, 1269, 1277, 1278). `transition()` refuses `blocked_on_user` to `cancelled` (818). `horch task cancel` is refused for the same reason, and `horch task requeue` needs `escalated`. If the answer is recorded, the task moves to `ready` (1274) and dispatch runs the flawed task again next to its replacement. If not, the task stays in `blocked_on_user` with no limit (1274). The idea needs each task that is not `cancelled` to be `done` (950), so it does not reach `integrating`. The same gap applies to `horch feature abandon` and `horch idea cancel` (1075, 953), which cancel an idea but not its tasks in `leased`, `running`, `stalled`, `verifying`, `blocked_on_user` or `waiting_quota`. It also applies to a judged task in `verifying` whose judge task you cancel: the judged task waits for a judge that never runs (1273), and you cannot cancel it.

Smallest change: In the task table at 1264-1279, add to each state that is not terminal: "A replan, `horch task cancel`, or the cancel of its idea → `cancelled`. fleetd first stops the process group of a live attempt." State that the cancel of a judge task also cancels the task it judges, or that plan validation refuses it.

### T40-4 - HIGH - A failed or abandoned job is never run again

Lines: 737, 1256, 1081, 730, 744-745.

> UNIQUE (kind, input_key)                 -- the same work is not run 2 times; its result is used again

Failure scenario: The push job of F-0003 for tip `a1b2c3` fails on a network error. fleetd sets `push_state` to `failed` and a backoff (1081). After the backoff, fleetd queues a push job with the same kind and the same input key, the tip. The key is `UNIQUE` (737), and the rule says that the result is used again (1256). So the stored `failed` result is used again, and no push runs. After 3 failures the feature has 1 needs-you item, and the push does not run again until the tip moves. A feature in `ready` gets no pull request and does not reach `merged`. The `fetch` job has the same fault: its input key is not stated (730), and if it is the repository, the first failed fetch is used again for ever. `remote_tips` stays stale (744-745), and merge detection stops for each feature of that repository. The same holds for a gate job stopped at its time limit as `abandoned`: the gate never runs again on those tips.

Smallest change: In 1256 and at 737, state: "A result is used again only for a job in `passed`, or a `failed` result of a check or gate. A job in `abandoned`, and a `failed` fetch or push, runs again on the same row after its backoff." State the input key of a fetch job.
