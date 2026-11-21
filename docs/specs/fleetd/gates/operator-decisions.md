# Operator decisions of the fleetd release

Task T-390, ticket `docs/tickets/fleetd-finish/251-operator-decisions-list.md`.
This file holds each open decision of the operator for the merge of `fleet/fleetd-finish` into `main`.
Each entry stands alone. Read only the entries that you must decide.

Tip of the facts: `72b48f3e` on `fleet/fleetd-finish` (2026-11-21).
The trial merge with `main` is the local branch `keep/fleetd-finish-trial-merge-main` at `3fd534f7`.
The run branch does not hold `origin/main` yet (verified: `git merge-base --is-ancestor origin/main 72b48f3e` exits 1).

Labels:
- **verified**: the evidence follows the label (a commit, a file and line, a report, or a command result).
- **not verified**: no evidence. The claim comes from a report or from a reading with no run.

The log is `ai_docs/plans/fleetd-finish/decisions.md` (copy at 10:04Z: `docs/tickets/fleetd-finish/_decisions-snapshot-1004Z.md`).
"Log 07:31Z" names the entry of the log at that time.

Each entry has 6 parts:
1. Question.
2. Facts, with the source.
3. Options and their cost.
4. Recommendation of the run, with the reason.
5. Blocked until the answer.
6. Code now: the option that the branch has today.

## Decisions that block the merge

### OD-1: the second pane host `hc` under fleetd

1. **Question.** fleetd has no adapter for the `hc` pane host that `main` released in v0.5.2: do you merge fleetd with `hc` refused, build the adapter first, or keep the old flow for `hc`?
2. **Facts.**
   - `main` v0.5.2 (`5863c698`) has PR #363 and #364: `herdr-core` `hc` and the Herdr desktop app as a second pane host. Verified: `merge-main-proof-2.md`, header and "For the operator" item 1.
   - In the trial, `horch fleet` and `horch fleetd` refuse `hc` with `HC_REFUSED`. Verified: `crates/horch/src/cmd/fleetd.rs:123` on `keep/fleetd-finish-trial-merge-main`.
   - The trial removed 6 `hc` tests of `main` in `hc_fleet_start.rs`. Verified: fix `3fd534f7`, `merge-main-proof-2.md` item 2.
   - A full-access worker on `hc` gets the `herdr-core` socket. That socket can run programs outside the sandbox, so SR-8 and FR-10 do not hold on `hc`. Verified by reading: `hc-adapter-scope.md`, section S1. Not verified as a run.
   - 7 docs of `main` describe `hc` fleets of the retired flow (`README.md`, ADR 0003, `docs/specs/pane-host.md` and 4 more). Verified: `merge-main-proof-2.md` item 3.
3. **Options.**
   - (a) Merge with `hc` refused. Work left: unit U8 (docs, CHANGELOG, `horch doctor` text), size S. An operator with `hc` fleets of v0.5.2 loses them at the update. Source: `hc-adapter-scope.md`, option (a).
   - (b) Build the adapter first: units U1 to U7, U9 and U8, about 7 to 9 worker days (not verified, an estimate), a design change, and a live check on a real `hc` by the operator (U9). U5 (no host socket for a worker on `hc`) is a security blocker.
   - (c) Keep the old claim-and-spawn flow for `hc`. It breaks RR-5, FM-16, OR-3, D3 and the 1-control-path rule of Phase 1. It does not fix S1. Size not verified, at least the size of (b).
4. **Recommendation.** (a), then (b) as its own run. Reason: (a) is the only option that the design supports as written (`hc-adapter-scope.md`, "What the design text supports"), and its code is ready.
5. **Blocked.** Unit U8. The docs of `hc` fleets. The CHANGELOG text of OD-2. The orchestrator starts U8 only after the answer (log 07:41Z).
6. **Code now.** Option (a) on the trial branch only. The run branch has no `hc` code, because `main` is not merged yet.

### OD-2: the release and the CHANGELOG line

1. **Question.** Do you want a line under `## Unreleased` in `CHANGELOG.md`, so that the merge of fleetd into `main` publishes a release at once?
2. **Facts.**
   - A merge to `main` with an entry under `## Unreleased` publishes a release at once. Verified: the comment at `CHANGELOG.md:3`.
   - `## Unreleased` has no entry on the run branch. Verified: `CHANGELOG.md` at `72b48f3e`.
   - The goal prompt says "Do not release" (`ai_docs/plans/fleetd-finish/goal-prompt.md`, "Rules that do not change").
   - `merge-main-plan.md` Q3 and `pr-description.md` "Decisions for the operator" item 3 ask the same question.
3. **Options.**
   - (a) No line. The merge publishes no release. fleetd reaches users only with a later release.
   - (b) A line. The merge publishes a release. Each user gets fleetd at the next `horch self-update`, with the host needs of OD-19 and the behavior changes of OD-13.
4. **Recommendation.** The run gives none, because the goal prompt forbids a release by the run. If you choose (b), decide OD-9 (real-CLI run) first.
5. **Blocked.** The release notes. The release handoff T-228 (F7).
6. **Code now.** Option (a): no line.

### OD-3: the comment-sweep step of `main` under fleetd

1. **Question.** `main` runs comment-sweep in each finish profile through `horch ledger finish`, which fleetd retires: do you accept that fleetd does not run it, or must it be ported first?
2. **Facts.**
   - The trial follows option (a). Verified: fixes `05cdf1e9` and `14833c51` (`merge-main-proof.md` item 4).
   - Source: `merge-main-plan.md` Q1, `pr-description.md` item 1.
   - Comment sweeps of this run removed security comments 5 times, and each was restored by hand (log 23:20Z, 03:22Z, 04:31Z, 05:34Z; commit `72b48f3e`). Verified: the log and `git log -1 72b48f3e`.
3. **Options.**
   - (a) fleetd does not run comment-sweep. The docs of S8 say so. No new code.
   - (b) Port it as a fleetd verifier step that starts 1 `comment-remover`, as `code_sweep` in `review.rs` starts the reviewers. New work with its own ticket, after the merge of `main` (E1).
4. **Recommendation.** (a) for the merge, and a ticket for (b). Reason: the merge then keeps the tested finish steps of the run branch.
5. **Blocked.** E1 (merge of `main`) keeps the trial form only after the answer. With (b), the 2 fixes change.
6. **Code now.** Option (a) on the trial branch.

### OD-4: the macOS check sandbox and the LAN address (R8-F2)

1. **Question.** On macOS a task check can connect to a service on a LAN address of the host, while the contract says "loopback and Unix sockets only": which rule do you want?
2. **Facts.**
   - The contract text is at `crates/horch-core/src/fleetd/worker_profiles/adapters/check_sandbox.rs:45`. Verified.
   - The macOS rule allows `localhost:*` (`crates/horch-platform/src/mac/sandbox.rs:78`). Seatbelt treats each address of the host as `localhost`. Verified: `macos-check-sandbox-lan.md`, step 1 (11 profile forms measured).
   - No Seatbelt form limits a check to loopback only. Verified by measure: `macos-check-sandbox-lan.md`.
   - Linux holds loopback only (`--unshare-net`). Verified by reading: `macos-check-sandbox-lan.md`, option (b).
   - The test `sr3_check_that_connects_off_host_is_refused` uses `192.0.2.1`, not the LAN address (`crates/horch-e2e/tests/p4_verifier.rs:605`, commit `2ca577e1`). End review A rates this Important. Verified: `reviews/end-review-a.md`, R8-F2.
3. **Options.**
   - (a) No IP at all for a macOS check. The contract stays true. 24 tests of this repo fail when they run as a macOS task check (21 self-update tests use a loopback server). Verified: `macos-check-sandbox-lan.md`, "Measure of option (a)".
   - (b) Keep the behavior. Change the contract and the design text to "this host only". A macOS check can reach a LAN service of the host.
   - (c) Mark the macOS check sandbox unsupported. Each macOS task check holds on `no_check_sandbox`. No macOS user gets a checked task.
4. **Recommendation.** The worker of T-301 recommends (b). End review A (operator decision 3) recommends the strict contract: (a) or (c). The run gives no single answer.
5. **Blocked.** R8-F2 stays open. Done criterion 4 of the goal needs a fix or a written waiver.
6. **Code now.** The behavior of (b), with the contract text of (a). T-301 landed evidence only.

### OD-5: the 3 expected failures of `i308_fleet_steward`

1. **Question.** `horch fleet` no longer starts the worktree steward under fleetd: do you restore the start, or retire the steward in the design?
2. **Facts.**
   - 3 entries stay in `crates/horch-e2e/expected-failures/i308_fleet_steward.txt`: `a_fleet_with_an_abandoned_worktree_spawns_the_worktree_steward`, `an_exhausted_claude_pool_starts_no_codex_steward`, `the_no_change_steward_task_finishes_without_a_worktree`. Verified: the file at `72b48f3e`.
   - Phase 6 broke the start: `recipes.rs` prints "not queued: horch ledger task is retired". The design does not retire the steward. Verified by the log 07:55Z (T-87).
   - These are the only entries left with a phase other than 3 or 4. Verified: the expected-failures files at `72b48f3e`.
3. **Options.**
   - (a) Restore the steward start under fleetd: a new ticket, likely as an idea or job of fleetd. Size not verified.
   - (b) Retire the steward: a design change, and the 3 tests go with a reason.
4. **Recommendation.** (a). Reason: the design keeps the steward, and fleetd custody (RR-13 to RR-18) does not clean trees that no fleetd task owns.
5. **Blocked.** Done criterion 1: an entry that stays needs a written operator decision.
6. **Code now.** Neither. The steward does not start, and the tests are expected failures.

### OD-6: FR-7 and SR-8 as literal design text, and SR-5 on Windows

1. **Question.** 2 rules have no test of their literal text, and 1 rule has a test only on Windows CI: do you change the design text, or require new code?
2. **Facts.**
   - FR-7 (`design.md:143`) says "a fixed doorbell line". The code has a closed list of 9 lines (`crates/horch-core/src/messaging/delivery.rs:33`, `TYPED_LINES`). The tests prove the list of 9. Verified: `pre-gate-p5.md`, Q1.
   - SR-8 (`design.md:182`) says the sandbox test records refusal rows. An OS sandbox refusal leaves no `requests` row. The tests prove the OS refusal, and a server refusal with a row only after a bypass. Verified: `pre-gate-p5.md`, Q2.
   - The Windows rules of SR-5 and REQUIRED 1 (single pipe instance) run only on Windows CI. Verified: `pre-gate-p5.md`, "Rules with no test". The last Windows CI test job hung and was cancelled at 3 h (`ci-run-4.md`).
3. **Options.**
   - (a) Change the text: "a closed list of fixed lines" for FR-7, and "the OS refuses with no row; the server refuses with a row when the sandbox is bypassed" for SR-8.
   - (b) Cut the code to 1 line, and add a refusal row for an OS refusal. The SR-8 part cannot be built: no process sees an OS refusal.
   - For SR-5: accept Windows CI as the only proof, or decide OD-7 first.
4. **Recommendation.** (a). Reason: the code is stricter than a reading of "1 line" needs, and an OS refusal has no observer.
5. **Blocked.** The final gate lists FR-7 and SR-8 as rules with no test until the answer. Done criterion 1.
6. **Code now.** The behavior of (a). `design.md` is not changed: the run cannot edit it.

### OD-7: Windows as a release target of fleetd

1. **Question.** Is Windows a release target of fleetd, when Windows has no sandbox for git, the folder lock guard holds nothing, and the hold of the termination handler does nothing?
2. **Facts.**
   - Windows has no sandbox for `git`. 83 tests fail there (FIX-37). Verified: `ci-run-4.md`, FIX-37 row. FIX-37 is not queued: it waits for this decision (log 06:44Z).
   - The breaker guard of the folder lock holds nothing on Windows (`crates/horch-platform/src/windows/fs.rs:338` to `:345`). 2 holders of 1 folder lock (land lock, ledger open lock) stay possible there. Verified by reading. Source: log 09:31Z (T-368). T-382 fixed the Unix case only (log 09:46Z).
   - The hold of the termination handler for check and gate groups does nothing on Windows: Windows has no polite stop and no process groups (`crates/horch-core/src/fleetd/fleet_runtime/adapters/effects.rs:72`). Not verified for the T-369 code, which is in its land step.
   - 154 other Windows tests fail (FIX-40), after FIX-35 to FIX-37. Verified: `ci-run-4.md`.
   - Protected-file reads by ACL are not built. Source: `end-review-p5.md` OD-2. Ticket 72 implements the ACL rule. Not verified as landed.
   - Profile validation refuses a worker profile on Windows (amendment A37). Verified: `plan.md`, "Not a gap".
3. **Options.**
   - (a) Windows is not a target of this release. The docs and the CHANGELOG say so. `horch fleetd` refuses to start on Windows, or says that it gives no isolation. Windows CI stays as a build proof.
   - (b) Windows is a target. Work: FIX-37 (a git sandbox), a Windows breaker guard, a Windows group stop, FIX-40, and ACL reads. Size not verified, many worker days.
4. **Recommendation.** (a). Reason: 3 safety rules (SR-3 for git, the lock contract, the stop of a check group) have no Windows form today.
5. **Blocked.** FIX-37 and FIX-40. Done criterion 5 (green Windows CI) is not reachable with (b) before the merge.
6. **Code now.** Neither. fleetd builds on Windows (`build-windows` green in `ci-run-4.md`). Its tests fail, and nothing refuses the start.

### OD-8: dispatch for a profile with no conformance run yet

1. **Question.** Must fleetd hold dispatch for a profile until its first conformance run passes?
2. **Facts.**
   - Delta review D244-F1 (Critical): a new profile with a first `not proved` report gets dispatch. SR-8 and FR-8 are then not fail-closed. Verified: `reviews/delta-review-custody-and-conformance.md`, D244-F1.
   - T-384 part 1 holds a profile after a `not proved` report until a passing report of the same key. Not verified: T-384 is running (`horch ledger show T-384`).
   - Part 2 is the open decision: a profile with no ended run at all. Design lines 908 and 1387 say a profile is unhealthy when a check fails. The FM-17, FM-18 and FM-19 rows say detection comes before dispatch. Source: log 09:55Z.
   - Cost of a hold: 12 s to 60 s before the first dispatch of each new CLI version or horch binary. About 35 e2e files with 316 tests then wait. Source: log 09:55Z (measured by the worker of T-384). Not verified by this task.
3. **Options.**
   - (a) Hold until the first passing run. Fail-closed. Each fleet start waits about 12 s. The e2e tests need a fake that passes fast, or a wait.
   - (b) Dispatch until a run fails. The present text of lines 908 and 1387. A worker can start before any proof of its CLI.
4. **Recommendation.** (a). Reason: SR-8 and FR-8 are fail-closed rules, and the FM rows say "before dispatch".
5. **Blocked.** Part 2 of T-384. The design text of lines 908 and 1387.
6. **Code now.** (b) for a profile with no ended run. Part 1 is in work.

### OD-9: a real-CLI conformance run before the release

1. **Question.** Do you run the conformance suite with the real Claude and Codex CLIs before the release, and keep the report as release evidence?
2. **Facts.**
   - Each e2e test uses a fake CLI. No run of `scripts/live/p7-real-conformance.sh` with real CLIs is recorded. Verified: `pr-description.md`, "Not proved" item 2.
   - The fakes cannot prove that installed Claude and Codex keep the orphan and operator boundary (FR-10, SR-8). Source: `end-review-a.md` operator decision 2, `end-review-p5.md` OD-3, `end-review-p7.md` item 2.
   - The run needs the logged-in subscription of the operator. A worker of the fleet must not use a credential (goal prompt, "Stop and ask the operator"). The run must not use `ANTHROPIC_API_KEY`.
3. **Options.**
   - (a) The operator runs the script for each CLI in a logged-in shell before the release, and after each change of a supported CLI version.
   - (b) Release with fake proof only. FR-10 and SR-8 are then not proved for real CLIs.
4. **Recommendation.** (a). Reason: 3 end reviews ask for it, and no other proof of the real sandbox exists.
5. **Blocked.** The release (OD-2, option b). Not the merge with no CHANGELOG line.
6. **Code now.** The script exists. No run is recorded.

### OD-10: FM-22 stop of an orphan group after a reuse of its id

1. **Question.** FM-22 signals a process group whose leader is gone: do you accept the risk that a reused group id gets the signal?
2. **Facts.**
   - `stop_orphan_group` sends TERM or KILL to group `pgid` when no process has the pid `pgid` (`crates/horch-core/src/fleetd/fleet_runtime/adapters/effects.rs:105`). Verified by reading.
   - After all members end, a new group can take the same id. The stop can then hit a foreign process. Source: log 09:52Z (T-369). Delta review D242-F1 (Critical) found this hole in the job stop. T-369 fixed the job stop and the attempt stop, and left FM-22 as designed. Verified: `reviews/delta-review-jobs-and-signals.md`, D242-F1, and the log.
3. **Options.**
   - (a) Keep FM-22 as designed. The risk needs a new leader with the same pid inside 1 pass.
   - (b) Signal each member by pid and recorded start time, as T-319 does for job groups. Members with no record get no signal.
   - (c) Send no signal. Record a notice, as OD-11 option (a) does.
4. **Recommendation.** None given by the run. This task notes that (b) matches the rule that T-369 applies to each other stop.
5. **Blocked.** The final review of D242-F1 for the attempt path.
6. **Code now.** (a).

## Decisions that do not block the merge

### OD-11: a KILL by fleetd of recorded orphan members after a bound

1. **Question.** After a job is abandoned with its leader gone, may fleetd KILL the recorded members by pid and start time after a time bound?
2. **Facts.**
   - Present rule (T-319, option O1): fleetd sends no signal, records a notice with each member pid and start time, and the git writer of that repository waits while a member runs. The wait has no bound. 1 needs-you item `job_abandoned` comes after `time_limit_s` + `stop_grace_s`. Source: log 08:35Z and 09:06Z. Not verified by this task.
   - Risk of a KILL: a wrong group needs all members to end and the id to be reused by a new leader within 1 pass. Source: log 08:35Z.
3. **Options.**
   - (a) O1: no signal. A hung member holds the git writer of its repository until the operator acts.
   - (b) O2: KILL each recorded member by pid and start time after the bound.
4. **Recommendation.** None given by the run.
5. **Blocked.** Nothing. The present rule is safe and visible.
6. **Code now.** (a).

### OD-12: the 10 s limit of a local git, and work over 1 s in the pass

1. **Question.** Do you accept a new constant of 10 s for a git that runs in the fleetd process, and do you accept merge work over 1 s inside the pass?
2. **Facts.**
   - No git in the fleetd process had a time limit. Some ran inside the write transaction of the pass, so a hung git held the ledger write lock until the watchdog restart (30 s, PL-5). Source: log 09:10Z (T-369 report).
   - T-369 adds an opt-in limit of 10 s in `GitRunner`, with TERM then KILL. The design gives no number. Not verified: T-369 is in its land step.
   - `design.md:1294` says work over 1 s is a job. `merge-tree` and `commit-tree` of the bundle candidate, and the judge rebuild, run in the pass. Source: log 09:10Z.
3. **Options.**
   - (a) Accept 10 s, and add it to the design text. Keep the merge work in the pass.
   - (b) Accept 10 s, and move the merge work to jobs (option 2 of T-369). New work.
4. **Recommendation.** (a) for the merge, and a ticket for (b). Reason: (a) closes the hang now.
5. **Blocked.** Nothing.
6. **Code now.** (a), when T-369 lands.

### OD-13: no off-host network in a task check

1. **Question.** A task check has no off-host network, so a dependency download in a check fails: do you keep this rule?
2. **Facts.**
   - T-17 added `Network::Local` for the check sandbox (SR-3). The error text tells the user why (`check_sandbox.rs:45`, `NETWORK_RULE`). Verified by reading.
   - Before T-17 the check sandbox had `Network::Open`. Source: log 12:55Z.
3. **Options.**
   - (a) Keep. The release notes say that a check needs its dependencies in the local cache.
   - (b) Open the network for a check. SR-3 and SR-4 then do not hold for a check.
4. **Recommendation.** (a). Reason: SR-3 and SR-4. The decision is reversible.
5. **Blocked.** The release notes.
6. **Code now.** (a).

### OD-14: unhealthy review profiles

1. **Question.** When the primary profile of a review task is unhealthy, does the task wait, or does it run on a healthy fallback of another provider?
2. **Facts.**
   - The task waits. "An unhealthy profile has no fallback" (`crates/horch-core/src/fleetd/task_execution/domain/dispatch.rs:16`, ruling Q15 of fleetd-p7). Verified by reading.
   - 1 unavailable reviewer can then hold the finish of an author for all time. Source: `end-review-a.md` operator decision 1.
3. **Options.**
   - (a) Keep Q15. A review waits.
   - (b) A review task runs at once on a healthy fallback of another provider. Small code change and a test.
4. **Recommendation.** (b). Reason: end review A.
5. **Blocked.** Nothing.
6. **Code now.** (a).

### OD-15: Antigravity out of the release claim

1. **Question.** Do you keep Antigravity (`agy`) out of the release claim until its real conformance is green?
2. **Facts.**
   - The adapter is proved with a fake only. Real `agy` has no private home and no Operator boundary. Verified: `end-review-a.md` decision 5, `end-review-p7.md` item 1, `FIX-INDEX-6.md` Q1.
   - fleetd marks `agy` unsupported and adds no drift idea for it (T-347, amendment A37). Verified: `reviews/delta-review-custody-and-conformance.md`, "Checked and good".
3. **Options.** (a) Out of the claim; the release notes say so. (b) In the claim; this needs real conformance first.
4. **Recommendation.** (a). Reason: 4 reviews agree.
5. **Blocked.** The release notes.
6. **Code now.** (a): `agy` gets no dispatch.

### OD-16: the key `unlanded` of `result.json`

1. **Question.** Do you add the optional key `unlanded` of `result.json` to the design text?
2. **Facts.**
   - T-288 (RR-14) added 1 optional key `unlanded` with status `completed`. fleetd then parks the task (`escalated`, `held_reason` `unlanded`, tree kept, no check job). Verified: `crates/horch-core/src/fleetd/task_execution/domain/result.rs:65` to `:68`, and `reviews/delta-review-custody-and-conformance.md`, "Checked and good".
   - The example and the status list of `design.md:543` to `:557` do not show the key. Verified by reading.
3. **Options.** (a) Add the key to the design. (b) Remove the key and find another form of RR-14.
4. **Recommendation.** (a). Reason: a `blocked` status cannot park the task, because a valid `result.json` goes to verifying (`design.md:1332`; log 02:24Z).
5. **Blocked.** Nothing.
6. **Code now.** The key exists.

### OD-17: schema version numbers and other stale facts in the design text

1. **Question.** Do you change the design text to the schema numbers of the code after the merge, and confirm that no real ledger is past version 8 by a run-branch build?
2. **Facts.**
   - In the trial, `main` keeps version 8 (comment sweep) and 9 (`BACKEND_VERSION`). The fleetd steps are 10 to 18. Verified: `merge-main-proof-2.md` item 4.
   - `design.md:1611` says "Schema v8 adds a `leases` table" (code: 11 in the trial). `design.md:1615` says "Schema v9" for ideas (code: 17 in the trial). `preservation-05-contracts-and-values.md:8`, `:9`, `:98` say "v8 leases, v9 ideas". Verified by reading at `72b48f3e`.
   - `merge-main-proof.md` names the older numbers of T-314 (9 to 17). They are out of date.
   - A ledger file that a run-branch build or the trial migrated past version 8 must be built again. No run-branch build was installed (log 03:52Z). Not verified for scratch files outside the fleet.
   - The plan item S4 was wrong: fleetd launches worker seats with Full access. Source: log 05:05Z.
3. **Options.** (a) Renumber as the trial does, and change the text after the merge. (b) Keep the run-branch numbers; the live ledger of 0.5.2 is at version 9 and cannot migrate.
4. **Recommendation.** (a). Reason: the live ledger of `main` is at version 9.
5. **Blocked.** Nothing. E1 uses (a).
6. **Code now.** (a) on the trial branch.

### OD-18: the worktree state table, trees with no task, and the end of a gate tree

1. **Question.** Do you add rows to the worktree state table for a tree with no task and for a gate tree?
2. **Facts.**
   - The table of `design.md:1030` to `:1041` has no row for a tree with no task, and no rule for the end of a gate tree. Verified by reading.
   - T-289 (option 1): a released gate tree is removed (preservation W11), a dropped scratch tree follows RR-13, and an unknown tree with no task stays `orphaned`, then `rescued`, for all time. Source: log 07:24Z. Not verified: T-289 is running.
   - 2 expected failures wait for T-289: `p3_refusals.txt` (`run_gate_folder_is_added_and_removed_through_the_jobs`) and `taskledger.txt` (`the_whole_flow_on_the_fake_herdr`). Verified: the files at `72b48f3e`.
3. **Options.** (a) Add the T-289 rules to the table. (b) Give a tree with no task an end, for example a removal after its rescue and a time limit.
4. **Recommendation.** (a). Reason: it matches the code and loses no work.
5. **Blocked.** Nothing, when T-289 lands and removes the 2 entries.
6. **Code now.** (a) is in work.

### OD-19: new host needs of fleetd

1. **Question.** Do you accept the new host needs, and the start refusal of fleetd on hosts that do not meet them?
2. **Facts.**
   - git 2.41 or newer: the rescue job needs `--attr-source`. T-389 makes fleetd refuse to start with an older git, and `horch doctor` checks it. Debian bookworm (git 2.39.5) is refused. Source: log 07:32Z and 10:01Z. Not verified: T-389 is running. Linux proof with git 2.51.2: `linux-git-241-proof.md`.
   - util-linux `setsid` on Linux: the Linux job sandbox runs it as the first program inside `bwrap` (`crates/horch-platform/src/linux/sandbox.rs:34` to `:37`). Verified by reading. Source: log 05:02Z (T-309).
   - `bubblewrap` and `socat` on Linux. Source: `pr-description.md`, "Release steps" item 5.
   - A fleet root under `/tmp` on Linux gives "Operator boundary" fail and an unhealthy profile, because `bwrap` mounts a private tmpfs on `/tmp`. A real install has its root under the home folder. `scripts/dev-sandbox.sh` puts it under `/tmp`. Source: log 05:41Z. Not verified by this task.
3. **Options.** (a) Accept, and list the needs in the release notes and in `horch doctor`. (b) Lower a need, for example a rescue with no `--attr-source` for git older than 2.41. Size not verified.
4. **Recommendation.** (a).
5. **Blocked.** The release notes.
6. **Code now.** (a) for `setsid`. git refusal in work (T-389).

### OD-20: `ui-prop-tester` against PL-2 and SR-7

1. **Question.** Do you port the crate `ui-prop-tester` of `main` to `horch-platform`, or keep it outside the PL-2 and SR-7 rules?
2. **Facts.**
   - In the trial, `arch_scan` skips the crate for PL-2 (17 OS `cfg` sites) and SR-7 (5 `Command::new`). Verified: `merge-main-proof.md` item 3.
   - The guard test `no_crate_of_horch_depends_on_ui_prop_tester` keeps the skip safe while no horch crate links it.
3. **Options.** (a) Keep the skip and the guard. (b) Port the crate. New work.
4. **Recommendation.** (a) for the merge. Reason: the crate is a test tool, not a part of fleetd.
5. **Blocked.** Nothing.
6. **Code now.** (a) on the trial branch.

### OD-21: the switch of the main checkout to `main`

1. **Question.** May the orchestrator, or will you, switch the main checkout from `fleet/fleetd-build` to `main`?
2. **Facts.**
   - The main checkout is on `fleet/fleetd-build`. Verified: `git branch --show-current` in the main checkout on 2026-11-21.
   - horch 0.5.1 reads `teammates/` from it, so the codex rule file lacked the allow rule for `horch ledger claim`. The workaround is `HORCH_TEAMMATES_DIR` set to a copy of `teammates/` of `origin/main`. Source: log 18:40Z.
   - A new solo run starts from that branch.
3. **Options.** (a) Switch to `main`. The workaround can go. (b) Keep the branch and the workaround.
4. **Recommendation.** (a). The switch is an operator act (goal prompt, "Stop and ask the operator").
5. **Blocked.** Nothing in this run. The workaround holds.
6. **Code now.** Not code. The checkout is on `fleet/fleetd-build`.

### OD-22: the request map: kinds that are not listed, and token-only request files

1. **Question.** Do you add the internal request kinds to the design map, and do you keep the token as the full credential of a request file?
2. **Facts.**
   - `worker_progress`, `worker_done`, `run_adopt` and `question_answer` are used and are not in the Phase 6 map. `spawn_adopt` is retired (T-302, `fb15c55f`). Source: `end-review-p5.md` OD-1, `pr-description.md` item 16. Verified for T-302 by the log 03:29Z.
   - A request file needs an owner-only 0600 token under an owner-only 0700 run folder (`token_file.rs:79`). Source: `end-review-a.md` decision 4 (verified statically by the review).
3. **Options.** (a) Add the 4 kinds to the map. Keep the token rule. (b) Retire the kinds that have no client. Add a peer proof to request files.
4. **Recommendation.** (a). Reason: the clients exist, and no platform-neutral peer proof is specified.
5. **Blocked.** Nothing.
6. **Code now.** (a).

### OD-23: other design text questions of the run

1. **Question.** Do you change the design text to the code for these points?
2. **Facts and options.** Each point has 2 options: (a) change the text to the code, (b) change the code to the text.
   - Q3 of `plan.md`: FM-13 (`design.md:1558`) holds the launcher of SR-7, but FM-13 is green in Phase 3 and SR-7 in Phase 7.
   - Q4 of `plan.md`: the default fleet root is `/fleet` (`design.md:80`). The code uses `<ledger folder>/fleet` (`crates/horch-core/src/runtime/paths.rs:39`).
   - RR-12: a resumed CLI that exits with no result burns 1 attempt (`pr-description.md` item 6). Delta review D244-F3 found that a resume of a lost session can burn the last attempt with no fresh fallback. T-386 changes this. Not verified: T-386 is running.
   - SR-3 boundary: define the destinations and transports that a worker must not reach (`end-review-p4.md`, `FIX-INDEX-3.md` Q2). Ticket 17 tests a finite list.
3. **Recommendation.** (a) for Q3 and Q4 (`plan.md`, "Design text questions"). (a) for RR-12 after T-386 lands. (a) for SR-3 with the list of ticket 17.
4. **Blocked.** Nothing.
5. **Code now.** The behavior of (a) for each point.

### OD-24: design gaps from the phase end reviews of run `fleetd-build`

1. **Question.** Do you accept the recommendation of each design gap that the phase end reviews named?
2. **Facts.** The reviews are in `docs/specs/fleetd/reviews/`. Not verified: whether the old run decided any of them. Its ledger is lost.
3. **Points, with the recommendation of the review.**
   - Idea amendment exits (`end-review-p6.md`): add `ticketing`, `in_progress`, `integrating` to `awaiting_approval` in the state table. Keep the code.
   - Repair persona (`end-review-p6.md`): the resolver is the primary persona of the conflicted task.
   - Drift project (`end-review-p6.md`, `end-review-p7.md` item 4): document the sorted-first repository rule (`pass.rs`), then add a fleet setting.
   - Docs-only sources (`end-review-p7.md` item 3): state that a docs-only source gives no profile health.
   - `horch fleetd stop` (`end-review-p6.md`): add a stop through the service layer by recorded PID and start time.
   - Judge profile and judge verdict path (`end-review-p6.md`): an optional roster field, and a declared review output path.
   - Work kinds (`end-review-p6.md`): add `research`, `design`, `creative`, `judge` to the ledger enum in a later migration.
   - Event wake latency (`end-review-p0-pp-p1.md`): prompt wakes on file events, as RR-4 says, or a maximum latency in RR-4.
   - Windows channel reply (`end-review-p0-pp-p1.md`): a bounded write and shutdown (PL-3). Depends on OD-7.
   - Calendar clock (`end-review-p0-pp-p1.md`, `FIX-INDEX-1.md` Q1): a calendar-clock port for rescue retention tests.
   - Repair with no tree row (`end-review-p2-p3.md` item 2): an invariant breach with a visible `repair_start` item.
4. **Blocked.** Nothing.
5. **Code now.** The behavior that each review names as present.

## Decided by the orchestrator, for the record

Each line is a ruling that changed behavior. The operator can reverse it.

- T-16: `agy` Stop hook in `<worktree>/.agents/hooks.json`, excluded from git (log 05:20Z).
- T-17: the check sandbox gets `Network::Local`; a check with an off-host need fails with SR-3 (log 12:55Z).
- T-87 and later: a test of a retired behavior moves its assert to the designed effect; a delete needs a design reason in the commit (log 08:50Z, 09:15Z).
- T-15: deleted `g52_resume_uses_own_session` and `g52_done_discovery_uses_the_same_launch_token` (log 17:25Z).
- T-90, T-232: 9 tests deleted and 15 rewritten in push form, each with design text (log 17:05Z, 19:40Z). End review A found them justified.
- T-163: the launch window is closed for `idea_approve` and the CLI guards only (log 16:45Z). T-172 replaced it.
- T-172: the wrapper waits until fleetd records its process before the agent starts, and fails closed at a limit (log 18:45Z).
- T-172: the git writers of 1 repository start first in, first out (log 22:50Z).
- T-246: `sr3_check_that_connects_off_host_is_refused` uses `192.0.2.1` (log 22:10Z). R8-F2 overruled it; see OD-4.
- T-254: `worker_launch_failed` ends a push attempt at once (log 02:48Z).
- T-288: the key `unlanded` of `result.json` parks a task (log 02:24Z); see OD-16.
- T-292: each per-row step of the pass is paged by `pass_budget` (log 02:32Z, 03:23Z).
- T-300: an answer resumes the worker session; a fresh worker is the fallback (log 04:51Z).
- T-302: `spawn_adopt` is retired (log 03:29Z).
- T-309: Linux job sandbox = `bwrap --unshare-pid` with inner `setsid`, no `--new-session`, no `--die-with-parent` (log 05:02Z).
- T-312, T-330: the fleetd schema steps are renumbered after `main` (log 03:52Z, 07:31Z); see OD-17.
- T-330: the trial refuses `hc` and removes 6 `hc` tests of `main` (log 07:31Z); see OD-1.
- T-347: a check with no channel server gives `not proved`, with no drift idea and no health mark; `agy` is unsupported (log 07:23Z). T-384 part 1 reverses the dispatch part; see OD-8.
- T-368: the Windows limit of the folder lock is accepted (log 09:31Z). T-382 reversed the Unix part (log 09:46Z); see OD-7.
- T-319: TERM first, KILL after the grace; no signal to an orphan group with its leader gone (log 09:06Z); see OD-11.
- T-369: 10 s limit of a local git in the fleetd process (log 09:10Z); see OD-12.
- T-389: fleetd refuses to start with git older than 2.41 (log 10:01Z); see OD-19.

## Each OPERATOR line of the log

| Log entry | Entry here, or why it is closed |
| --- | --- |
| 18:40Z, line 44: switch of the main checkout | OD-21 |
| Goal blocker 3: PR #361 | Closed. PR #361 is `CLOSED` (verified: `gh pr view 361` on 2026-11-21). |
| 05:20Z: `agy` stays unhealthy | OD-15 |
| 06:55Z: OD-1 to OD-3 of R4 | OD-22, OD-7, OD-9 |
| 07:55Z: `i308_fleet_steward` | OD-5 |
| 12:35Z: change of pool | Closed. The run stayed on the claude pool. Not verified that the operator answered. |
| 12:55Z: no off-host network in a check | OD-13 |
| 17:16Z: T-209 not verified fixed | Closed. T-236 found and fixed the cause (log 19:25Z; `end-review-a.md`, T-209/T-236). |
| 17:25Z: RR-12 resume with the answer | Closed by T-300 (R8-F1 fixed, log 04:51Z). The last-attempt case is in OD-23. |
| 22:40Z: when a gate tree is removed | OD-18 |
| 22:10Z: macOS check reaches the LAN address | OD-4 |
| 02:24Z: key `unlanded` | OD-16 |
| 03:03Z: 5 operator decisions of end review A | OD-14, OD-9, OD-4, OD-22, OD-15 |
| 03:14Z: R8-F2 options | OD-4 |
| 03:52Z: Q1 comment sweep, Q3 CHANGELOG | OD-3, OD-2 |
| 05:02Z: util-linux `setsid` | OD-19 |
| 05:05Z: schema numbers in the design, seats with Full access | OD-17 |
| 05:41Z: fleet root under `/tmp` on Linux | OD-19 |
| 06:44Z: FIX-37 Windows git sandbox | OD-7 |
| 07:24Z: tree with no task | OD-18 |
| 07:31Z and 07:41Z: `hc` pane host | OD-1 |
| 07:32Z: git 2.41 | OD-19 |
| 08:35Z: KILL of orphan members (O2) | OD-11 |
| 09:10Z: 10 s git limit, work over 1 s | OD-12 |
| 09:31Z: Windows folder lock | OD-7 |
| 09:52Z: FM-22 orphan group after id reuse | OD-10 |
| 09:55Z: hold for a profile with no run | OD-8 |
| 10:01Z: start refusal on bookworm | OD-19 |
