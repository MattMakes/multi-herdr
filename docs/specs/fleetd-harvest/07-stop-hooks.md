# 07: Refuse an end of turn while the worker has an open task

Status: proposed. Target: `main` at `06930d95`. Source of the idea: fleetd at `9bc3bf70`.
Report id: FM-6. Size: M.

Path short forms: `TL` is `crates/horch-core/src/taskledger`, `H` is
`crates/horch-core/src/harness`, and `CMD` is `crates/horch/src/cmd`. A `CODE` label without a
commit is at MAIN `06930d95`.

## Problem

The problem is real on MAIN, but report R6 describes it incorrectly. R6 says that a worker finishes
and does not report. The ledger shows a different fault: a worker ends its turn to wait for
something, and horch has no record of the wait. 0 of the 7 cases that a Stop hook can reach are a
worker that finished and forgot `horch ledger done` [MEASURED, see `## Evidence on main`].

For the operator, the effect is this. A worker says "I wait for the job" and stops. Its pane stays
open and silent. After about 33 minutes the watchdog reaps the worker as stalled, although the
worker was waiting for a valid reason. The task then goes to a new attempt, and the new worker
repeats the context work.

The exact cause on MAIN has 3 parts:

1. No Stop hook exists. `git grep` for `hooks.Stop`, `"Stop"` and `stop_hook` in `crates/` and
   `teammates/` gives 0 matches [MEASURED].
2. The supervisor acts only when the agent process exits. `settle` requeues a `Working` worker at
   that time [CODE `TL/supervisor.rs:201`, `TL/supervisor.rs:232`]. An agent that ends its turn
   does not exit, so this path does not run.
3. `proof` in the watchdog treats a worker as alive only with a wait row or a moving signal
   [CODE `TL/watchdog.rs:331-361`]. With neither, `stalled` returns `NoProgress`
   [CODE `TL/watchdog.rs:397-404`]. `stall_round` reaps the worker after `stall_rounds` checks
   [CODE `TL/watchdog.rs:419`]. The defaults are 3 checks of 600 s [CODE `TL/schema.rs:45-46`].

## Evidence on main

Query, run on 2026-11-22 at 23:11 UTC with a read-only connection. The ledger covers 2026-10-31 to
2026-11-22, all projects [MEASURED]:

```sh
sqlite3 -readonly ~/.horch/ledger.sqlite3 "SELECT count(*) FROM workers
  WHERE end_reason LIKE '%no ledger call, no transcript growth, no state change%';"
```

Result: 29 workers, of 4103 claims (0.71%). This repeats the count of R6 [MEASURED]. I then read
the last transcript record of each of the 29 workers [MEASURED]:

| Class | Count | Does a Stop hook help |
|---|---|---|
| Claude usage-limit message (`isApiErrorMessage: true`, `error: rate_limit`) | 18 | No. Spec 06 owns this class |
| Worker ended its turn with text, to wait, and no wait row exists | 7 | Yes |
| Last record is a compaction summary, and the agent did not answer | 1 | No. No turn ended |
| No transcript path in the ledger (2 Codex, 1 Antigravity) | 3 | Unknown |

The 7 reachable cases are 0.17% of claims. Their mean time from last progress to reap is 33.1 min,
and the maximum is 39.7 min [MEASURED]. The workers waited for these:

1. A background job: `a0ff47ae` (T-925, `opus-2`) and `f68a7e12` (`opus-domain-14`).
2. Other workers: `ce5a8398` (T-652, `opus-9`) waited for the report of a round.
3. The orchestrator: `5b0abba3` (T-34, `opus-10`, last progress 2026-11-05T23:59:56Z, reaped
   2026-11-06T00:37:59Z), `f8b9eb0b` (T-233) and `e21c88ce` (T-72).
4. The operator: `81215b24` (T-1, `worktree-steward-1`) waited for a reply to a triage table.

MAIN already has the correct tool for each case. `horch ledger progress "waiting: <what>"` opens a
wait row [CODE `TL/workers.rs:376-377`], and the worker prompt tells the worker to use it
[CODE `teammates/_base/fleet-worker.md:66-68`]. These 7 workers did not use it. The fix makes the
worker use it at the moment that it stops.

## How fleetd solves it

The rule at FINISH: a worker may end its turn only when `outbox/result.json` or a question file
exists [CODE `9bc3bf70:crates/horch-core/src/fleetd/worker_profiles/domain/stop_gate.rs:33`].

1. `stop_gate::may_end` is the rule, and `hook_command` builds the shell command with a quoted
   absolute path [CODE `9bc3bf70:.../stop_gate.rs:33`, `:43`].
2. `horch result --check` applies the rule. It exits 0 to allow, 2 to refuse, and 1 outside a live
   attempt [CODE `9bc3bf70:crates/horch/src/cmd/resultcmd.rs:92`, `:99`].
3. Claude gets the hook through `overlay_stop_hook`, after the existing Stop hooks
   [CODE `9bc3bf70:crates/horch-core/src/harness/claude.rs:528`].
4. Codex gets it through `-c hooks.Stop=...`, after the teammate args
   [CODE `9bc3bf70:crates/horch-core/src/harness/codex.rs:1803`].

The idea to port: a Stop hook asks horch whether this worker may stop, and horch answers from its
own records. These parts exist only because fleetd is a daemon, so do not port them:
`result.json`, the outbox, `horch result`, the repair attempt and the conformance runner.

Do not copy these 2 defects of FINISH:

1. FINISH has no loop guard. `stop_hook_active` has 0 matches in `crates/` and `docs/` at FINISH
   [MEASURED]. A worker that cannot satisfy the hook is refused on each stop. Claude Code 2.1.327
   has its own block cap, and its text says to check `stop_hook_active` [MEASURED, binary strings].
   I found no cap for Codex [NOT VERIFIED].
2. FINISH refuses with exit 2. An old `horch` that does not know the subcommand also exits 2
   (see `## Reproduce`, step M3). After a rollback, each stop is then refused.

## Fix

### Behavior

1. **H07-R1.** Add the command `horch ledger turn-check`. It reads the hook JSON from stdin. To
   allow, it prints nothing and exits 0. To refuse, it prints
   `{"decision":"block","reason":"<text>"}` on stdout and exits 0. Claude Code and Codex 0.166.0
   both accept this output for a Stop hook [MEASURED for the Codex schema `stop.command.output`;
   NOT VERIFIED in a live Claude or Codex turn].
2. **H07-R2.** The command allows the end of turn in each of these cases, in this order:
   - a. `HORCH_WORKER_ID` is missing, or the ledger has no such worker.
   - b. The worker row has `kind = 'orchestrator'`.
   - c. The worker status is not `working`. This covers a worker with no task, a worker that is
     done, and a worker that handed off.
   - d. The worker has a live wait row of kind `question`, `reviewers` or `background`. This
     covers `waiting:` progress, a detached finish [CODE `CMD/taskledger_finish.rs:120`],
     comment-sweep [CODE `TL/finish/comments.rs:90`] and code-sweep
     [CODE `TL/finish/codesweep.rs:612`].
   - e. The mail folder has an open `QUESTION:` or `BLOCKED:` message from this role
     [CODE `crates/horch-core/src/messaging/mail.rs:281`, `:542`]. Read the mail directly. The
     wait row is only a copy that `sync_questions` writes [CODE `TL/workers.rs:751`].
   - f. The newest agent `progress` event of this worker starts with `handoff:` and is at most
     900 s old. This is the compaction path [CODE `teammates/_base/fleet-worker.md:193-196`].
3. **H07-R3.** In every other case with `stop_hook_active: false`, the command refuses. The reason
   text names the task and gives the 3 valid actions: run `horch ledger finish`, record
   `horch ledger progress "waiting: <what>"`, or send `QUESTION:`.
4. **H07-R4.** The limit is 1 refusal for each incoming prompt. With `stop_hook_active: true`, the
   command always allows. Both CLIs supply this field [MEASURED, binary schema and strings]. This
   limit needs no counter and no schema change.
5. **H07-R5.** When R4 allows a stop that R2 does not allow, the command queues 1 notice to the
   orchestrator with `mail::queue_notice` [CODE `crates/horch-core/src/messaging/mail.rs:682`].
   The notice names the worker and the task.
6. **H07-R6.** The command fails open. On each error (bad JSON, ledger error, roster error, mail
   error) it prints nothing on stdout and exits 0.
7. **H07-R7.** The command writes nothing to the ledger. It opens the ledger with
   `Store::open_read_only` [CODE `TL/store.rs:160`] and sets a busy timeout of 2 s. It must not
   call `workers::waiting`, because that function deletes expired rows [CODE `TL/workers.rs:670-671`].
8. **H07-R8.** A Claude worker gets 1 `Stop` hook in the single `--settings` overlay of ADR 0016.
   The hook goes after each existing Stop hook. It reaches all 3 routes: `default_overlay`,
   `merged_inline_settings` and the skills route [CODE `H/claude.rs:316`, `:293`, `:72`].
9. **H07-R9.** A Codex worker gets the hook with `-c hooks.Stop=[...]` after `teammate.args`
   [CODE `H/codex.rs:1084`]. MAIN already passes `--dangerously-bypass-hook-trust`
   [CODE `H/codex.rs:1097`], and the `hooks` feature is stable and on in Codex 0.166.0 [MEASURED].
10. **H07-R10.** horch installs the hook only when the child environment has a non-empty
    `HORCH_WORKER_ID`, and only on Unix. The hook command is
    `'<horch>' ledger turn-check 2>/dev/null || true`. The `|| true` makes an old `horch` allow the
    stop. Windows keeps its behavior of today, because `|| true` needs a POSIX shell.
11. **H07-R11.** `turn-check` does not start the watchdog and does not print mail reminders. Output
    from either would corrupt the hook answer [CODE `crates/horch/src/main.rs:873`, `:883`].
12. **H07-R12.** The refusal and notice texts live in `teammates/_base/context-messages.md`. Rust
    only fills the placeholders, as `prompts::context_message` does [CODE
    `crates/horch-core/src/prompts.rs:150`].
13. **H07-R13.** The watchdog does not change. The stall reap stays as the last defense.

### Files to change

1. `TL/turn_gate.rs` (new, declared in `TL/mod.rs`): `TurnFacts`, `TurnDecision`, the pure function
   `decide` for H07-R2 to H07-R4, and the read-only fact reader.
2. `TL/workers.rs`: extract a read-only `live_waits` from `waiting`. It leaves out expired waits
   and finished review waits. `waiting` calls it, then clears those rows as it does today.
3. `CMD/taskledger.rs`: add `LedgerCommand::TurnCheck`, put it in `Group::Worker`, and handle it
   with an early return next to `Path` (line 617), before the ledger opens for write.
4. `crates/horch/src/main.rs`: exclude the command in `reminds()` and `watched()`.
5. `H/launch.rs`: add `LaunchEnv.turn_gate: bool`, default `false` in both constructors (lines 97
   and 127). Set it at line 567 from `req.child_env`, as the code does for `HORCH_TEAMMATES_DIR`.
6. `H/claude.rs`: add `overlay_turn_gate`. Call it in `default_overlay` and in
   `merged_inline_settings`, and add `env.turn_gate` to the merge condition at line 286.
7. `H/codex.rs`: add `push_turn_gate`, and call it directly after `cmd.args(&teammate.args)`.
8. `teammates/_base/context-messages.md`: add the messages `turn-refused` and `turn-ended-open`.
9. `crates/horch-core/src/roster/teammate.rs`: add the 2 keys to the required list (line 284).
10. `crates/horch-e2e/tests/turn_gate.rs` (new) and `crates/horch-core/tests/harness_defaults.rs`.

### Settings

This fix adds no operator setting. It adds 3 constants:
1. Refusals for each prompt: 1. The reason text is complete, so a second refusal adds cost only.
2. Handoff window: 900 s. It covers the delay from `COMPACT-READY` to the compaction, and no more.
3. Ledger busy timeout: 2 s. A hook must not hold the end of a turn for the 30 s default.

### Not ported

1. `result.json`, the outbox and `horch result`: MAIN reports through `horch ledger done`.
2. The other harnesses: only 1 of the 29 reaps is not Claude or Codex, and it has no transcript.
3. A hook for a usage limit: spec 06 owns those 18 cases.
4. A settings file given by path: the hook cannot merge into it [CODE `H/claude.rs:289`]. Only
   `teammates/_template.md` has a `settings:` key [MEASURED].

### Effect at upgrade

1. Ledger and runs: no migration and no change.
2. Workers: a running pane has no hook until it relaunches. A resumed worker gets the hook.
3. Rollback: a pane with the hook and an old `horch` allows each stop, because of `|| true`.

## Reproduce

### Automated (NOT RUN: the run limits forbid `cargo`)

File: `crates/horch-e2e/tests/turn_gate.rs`. Copy `world`, `as_worker`, `ok` and the setup of
`finished_task`, up to the claim, from [CODE `crates/horch-e2e/tests/done_budget.rs:190-206`].

```rust
#[test]
fn h07_r3_refuses_a_working_worker_with_no_wait() {
    let h = world("turn-gate");
    let worker = claimed_task(&h);
    let mut cmd = as_worker(&h, &["ledger", "turn-check"]);
    cmd.env("HORCH_WORKER_ID", &worker);
    let out = run_with_stdin(cmd, r#"{"stop_hook_active":false}"#);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["decision"], "block");
    assert!(v["reason"].as_str().unwrap().contains("waiting:"));
}
```

`claimed_task` is that setup. `run_with_stdin` pipes the text to the child. Result on MAIN: the
test fails at `out.status.success()`, because the command exits 2 (step M3).

### Manual

1. **M1. RUN.** `git grep -c -I -E 'hooks\.Stop|"Stop"|stop_hook' 06930d95 -- crates/ teammates/`
   Output: nothing, exit 1. MAIN has no Stop hook.
2. **M2. RUN.** Run the query of `## Evidence on main`. Output on 2026-11-22: `29`.
3. **M3. RUN.** `horch ledger turn-check </dev/null`
   Output: `error: unrecognized subcommand 'turn-check'`, exit 2.
4. **M4. NOT RUN** (the run limits forbid a spawn). In a scratch project, start 1 Claude worker on a
   ticket that says: "Start `sleep 900` in the background, say that you wait for it, and end your
   turn. Do not run `horch ledger progress`."
5. **M5. NOT RUN.** Run `horch ledger show <task>`. Expect on MAIN: no `waiting:` event.
6. **M6. NOT RUN.** Wait for 3 watchdog checks (about 30 min). Expect on MAIN: the worker ends
   `incomplete` with the reason `no ledger call, no transcript growth, no state change`.

## Validate

Each test is NOT RUN, because the run limits forbid `cargo`. "MAIN today" is the result of the same
test on MAIN. On MAIN, `turn-check` exits 2 with `unrecognized subcommand`.

1. **V1 (H07-R1, H07-R3). NOT RUN.** The test of `## Reproduce` passes. MAIN today: fails.
2. **V2 (H07-R2). NOT RUN.** A table test of `turn_gate::decide` covers cases a to f and the
   opposite of each. A `handoff:` event of 901 s refuses. A `reviewers` wait with all review tasks
   done refuses. MAIN today: does not compile, no such module.
3. **V3 (H07-R2e). NOT RUN.** A worker with an open `QUESTION:` file and no wait row exits 0 with
   empty stdout. MAIN today: exit 2.
4. **V4 (H07-R4). NOT RUN.** The setup of V1 with `{"stop_hook_active":true}` exits 0 with empty
   stdout. MAIN today: exit 2.
5. **V5 (H07-R5). NOT RUN.** After V4, the orchestrator mailbox has 1 notice with the role and the
   task. After an allow from H07-R2, it has 0 notices. MAIN today: exit 2 and 0 notices.
6. **V6 (H07-R6). NOT RUN.** Invalid JSON, and a missing ledger file, each exit 0 with empty
   stdout. MAIN today: exit 2.
7. **V7 (H07-R7). NOT RUN.** With an expired wait row, the command refuses and the row still
   exists. MAIN today: exit 2 and no refusal JSON.
8. **V8 (H07-R8). NOT RUN.** With `settings_arg` and `turn_gate: true` on each of the 3 routes,
   `hooks.Stop` ends with the gate and keeps a teammate Stop hook. MAIN today: `hooks.Stop` is absent.
9. **V9 (H07-R9). NOT RUN.** `config_pairs` of a Codex launch holds `hooks.Stop=...`, after the
   teammate args. MAIN today: the pair is absent.
10. **V10 (H07-R10). NOT RUN.** A launch with `HORCH_WORKER_ID` has the exact command with
    `|| true`. MAIN today: fails. A launch without it has no hook, and the files in
    `crates/horch-core/tests/oracles/launch/` do not change. MAIN today: passes, so this half
    only guards against a regression.
11. **V11 (H07-R11). NOT RUN.** With a pending reminder and no watchdog, stdout is empty or 1 JSON
    object, and no watchdog starts. MAIN today: exit 2.
12. **V12 (H07-R12). NOT RUN.** `horch teammates --check` fails when `turn-refused` is removed from
    a copy of the roster. MAIN today: passes, because the key is not required.
13. **V13 (H07-R13). NOT RUN.** The existing watchdog tests pass with no edit. MAIN today: pass.
    This test only guards against a regression.

Manual steps with the fix:
1. **V-M1. NOT RUN.** Repeat M4 with Claude. Expect 1 refusal, then
   `horch ledger progress "waiting: ..."`, then a stop and no reap. MAIN today: reap in about 30 min.
2. **V-M2. NOT RUN.** Repeat M4 with Codex. Expect the same result. This step also proves that the
   hook can read the ledger inside the Codex sandbox. MAIN today: no refusal.
3. **V-M3. NOT RUN.** Tell the worker to ignore the refusal. Expect 1 refusal, 1 orchestrator
   notice and a later stall reap. MAIN today: 0 refusals and 0 notices.

Ledger count after 2 weeks: run the query of `## Evidence on main` with
`AND ended_at >= '<upgrade date>'`, then classify the transcripts as in the table. The target is 0
Claude and Codex workers in the class "ended its turn with text, to wait". The baseline is 7 in 23
days. Do not use the total of 29 as the baseline, because spec 06 owns 18 of them.

## Size and risks

Estimate: 2 worker-days (M). Most of the time goes to the 3 Claude settings routes and to the live
checks on 2 CLIs.

Risks:
1. **K1. A false `waiting:` can hide a stalled worker for 24 h** [CODE `TL/workers.rs:377`]. Today
   that worker is reaped in about 33 min. Land spec 03 (wait caps) before this spec or with it.
2. **K2. The Codex sandbox can block the ledger read or the notice** [NOT VERIFIED]. H07-R6 then
   allows each stop, so the fix has no effect on Codex and shows no error. V-M2 detects this.
3. **K3. Codex `-c hooks.Stop` can replace other Stop hooks of the session** [NOT VERIFIED]. The
   operator's Codex config has 0 Stop hooks today [MEASURED].
4. **K4. A Stop hook can fire on a usage-limit message** [NOT VERIFIED]. Claude has a separate
   `StopFailure` event [MEASURED, binary strings]. H07-R4 limits the cost to 1 failed request.
5. **K5. A prompt to a working worker with no wait can cost 1 more model turn.**
6. **K6. The benefit is small:** 7 reaps in 23 days (0.17% of claims).

Files that other specs can also change: `TL/workers.rs` (spec 03). I did not read the other specs
for `CMD/taskledger.rs`, `crates/horch/src/main.rs`, `H/launch.rs`, `H/claude.rs` and `H/codex.rs`.
Compare them when the guide orders the work.

Open questions for the operator:
1. **Q1.** Do you accept 1 extra model turn when a worker stops with no recorded wait (K5)? I
   recommend yes, because the alternative is a reap and a full new attempt.
2. **Q2.** Should this spec wait for spec 03 (K1)? I recommend yes, because the refusal increases
   the use of the 24 h wait.
