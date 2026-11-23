# Delta review: custody and conformance

## Scope

I reviewed the delta from base `50fe4705` through tip `05ff408109f34d10dce7df94084c9b8979cb2cc9` in these paths:

- `crates/horch-core/src/fleetd/worktree_custody/application`
- `crates/horch-core/src/fleetd/worktree_custody/domain`
- `crates/horch-core/src/fleetd/worker_profiles`
- `crates/horch-core/src/fleetd/integration/application`
- `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_worker.rs`
- `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_custody.rs`
- `crates/horch/src/cmd`

The 32 in-scope commits are:

- `ae08ee095c2571718cc79605496008bd03007434` — fleetd-finish 157: no agent code runs before fleetd has recorded the process of its attempt (SR-8, FM-32, T-172)
- `1f919fea10d6eb5b561aac05ed0153d517d5db1e` — RR-13: fleetd refuses a drop whose tree HEAD moved during the checks (T-287)
- `38736b49cb8a1f327a35f1a7301dca182ac65361` — fleetd: a job host of a pass runs the sibling horch, not this binary (T-287)
- `4d2c7ff3cdb033307a836b563e3429ec00bc8599` — Comment sweep of T-287: delete 5 comments that retell the code (T-305)
- `fb15c55f632e085116a01436db117a33fff3fb69` — fleetd-finish R8-F3: retire the request kind spawn_adopt (T-302)
- `2c6ef0b41acecb742357047f1087310d9eca768f` — T-302 comment sweep: remove 2 comments that retell code or park a retired statement
- `d8a08e3595593d359cf679c53a0f3b4f32edc83f` — T-254 CI FIX-25: a worker_launch_failed report ends the push attempt at once
- `ec95e90245d1a88d00b10f26ac07d3c43ecf84ba` — comment sweep T-254: remove 7 comments from the launch-failed fix and its tests
- `86d911963c17e8c6976a84cf5e4066b517a9a815` — T-254: restore dependency reads and the bind helper after the T-302 rebase
- `0069a17133f7127ee45e2e267abda488759c6a45` — comment sweep T-254: remove 4 comments
- `d15dd7719ecd1354f39bda644e66fa49a4b7dff6` — fleetd-finish 42 stage 2: the disk budget reads the trees in 1 statement (T-292)
- `74c79ed6395689cd32ed25b82ec1cf75e4f92a4d` — fleetd-finish 42 stage 2: reconcile reads the trees with an active removal in 1 statement (T-292)
- `3f6321ffb5fe4330f94e1c514a6f18f37b7840fe` — Comment sweep T-292: delete 36 narrating doc comments (T-316)
- `b7ccafcae3eca8eb3bbc0be0f272c98ecabd2ebb` — fleetd-finish 42: restore the comments of the bounds and whole steps (T-292)
- `583b44023b6ff220bc07f7b9e3792248c976d219` — fleetd-finish 199: an answered question resumes the worker session (RR-12, T-300)
- `90a1fa49d240422d92eee77ce357d714eb31d4dc` — comment sweep T-300: delete 20 comments that retell code or test names (T-317)
- `c6dede2f327ee5852e0ae870e5c3de17adcfee29` — T-320: the operator-boundary probe proves that it sees the real fleet root
- `99414fd154d2740463656a43730ee862fbb3b6c8` — comment sweep T-320: delete 17 comments that retell the code (T-328)
- `7f276a1a27e79f766c932c55e8e553b5cf10382b` — T-320: restore 4 comments that state the security rule of the fix
- `497d9cf7f1da35402a7f1e277451a27ac5fa04d2` — fleetd-finish 168: worktree states landing and landed (T-286)
- `9649d2bbb4c0cb1e91b46611906fea4f9d6496ac` — comment sweep T-286: delete 37 comments (T-326)
- `aa56d46b9ba0e98faedce428278c295a9999c24c` — fleetd-finish 168: restore 7 comments that state why a tree moves (T-286)
- `f18eb76e9663ff47cb7149e3c1157dec8afa2aa2` — Windows: test code that starts git gets a plain path from `for_child` (T-355)
- `5efc8c11075eaf5395ad71869afccaf275045f8b` — fleetd-finish 213: each context reads another context through its owner reader (T-327)
- `f21c54e25d0fac2bdf7be56f29beb0a940b08fb5` — fleetd-finish 228: an unproved check is `not proved`; agy is unsupported (T-347)
- `23bf4f4b9c765ee6732106b21b72f775e881eb57` — fleetd-finish 213: rename the landing-read fixture to `TreeWorld` (T-327)
- `16d454b9b62aef2b6da2bd06c43bfdf5691937bf` — comment sweep T-327: delete 86 added comment lines (T-358)
- `e9e07c766c1d6bcf4de84fdd0efb4750c1950b18` — fleetd-finish 213: restore snapshot, statement, wait, and design comments (T-327)
- `8235f257ddb7343d02a26c2c99a370c3661327ff` — fleetd-finish 233: the last statement belongs to its read-only Store (T-359)
- `30bf47d78c4f791a0a070feda546b5fec23d67cf` — CI4 FIX-34: fix 3 Linux test-setup faults (T-338)
- `46881419634943b954188cccb02ad4ae977c3a34` — Comment sweep T-338: delete 8 comments (T-360)
- `5a6a8cb0e439c6356777457530eab0d34e17ff18` — T-338: restore 6 security and design comments

I also traced the related task cancellation, dispatch, retry, conformance reproof, process-liveness, and result-ingest callers needed to evaluate the changed boundaries. T-289 is not in this tip, so its unlanded branch is not part of this delta.

## Findings

### D244-F1 — Critical — an unproved first conformance leaves a profile dispatchable

**Location:** `crates/horch-core/src/fleetd/worker_profiles/application/health.rs:83`  
**Evidence:** verified by reading

Concrete event order:

1. A new profile has no health event. `health()` therefore treats it as healthy.
2. Its first conformance job runs before the operator channel server is ready.
3. The operator-boundary check reports `NotProved`, and the report has no failed check.
4. `mark_after_conformance()` returns without writing a health event.
5. Dispatch reads only the latest unhealthy marks, so the profile remains eligible.
6. A worker can start before any conformance run has proved the current CLI and adapter.

This breaks the SR-8/FR-8 fail-closed boundary. The reproof job later queued by the pass does not hold dispatch while proof is pending.

Smallest fix: represent missing proof for the current CLI version and adapter hash as a dispatch hold. Keep that hold through `NotProved`, and release it only after a complete passing report. Add a test in which a first-ever `NotProved` report holds the profile and a later passing report releases it.

### D244-F2 — Important — active-tree reconciliation still performs SQL per row

**Location:** `crates/horch-core/src/fleetd/worktree_custody/application/reconcile.rs:577`  
**Evidence:** verified by reading

Concrete event order:

1. One pass reads all active worktree rows and their owner-attempt outcomes in a snapshot.
2. The pass then iterates over every active row.
3. `owner()` calls `state_of()` for the attempt, `workers::find()` for the worker, and `tasks::get()` for the task.
4. The SQL statement count therefore grows with the number of active worktrees.

The T-327 statement-count test covers landing rows and the batched outcome read, but it does not cover the rest of the owner reads. This violates the review requirement that a pass run no SQL statement for each row and weakens the A23 bounded-pass guarantee.

Smallest fix: read attempt states, worker statuses, task states, and parked flags in bounded set reads inside the same snapshot. Make `owner()` consume only those maps. Add a statement-count test comparing 3 active rows with 300 active rows.

### D244-F3 — Important — a missing resume session can consume the final attempt before the required fresh fallback

**Location:** `crates/horch/src/cmd/spawn.rs:246`  
**Evidence:** verified by reading

Concrete event order:

1. A worker asks a question and records a valid session identifier.
2. The operator answers after the external agent has deleted or lost that session.
3. The next worker passes the stale identifier to the adapter's resume form without proving that the session exists.
4. The adapter refuses the resume, and the attempt ends `no_result`.
5. That mechanism failure burns one task attempt and enters retry backoff.
6. If it was the final allowed attempt, `after_failure()` escalates the task, so no fresh worker receives the question, answer, and committed work.

The existing end-to-end test proves a fresh third attempt only when retry budget remains. It also explicitly expects the failed resume to burn an attempt. The maximum-attempt boundary therefore does not satisfy RR-12's fresh-worker fallback.

Smallest fix: classify a resume refusal caused by a missing session separately from task work failure. Clear the resume request and queue one fresh launch without consuming task retry budget. Add a test with one remaining attempt in which a stale resume still launches the fresh fallback.

## Checked and good

- The T-320 operator-boundary probe compares the real fleet-root device and inode before treating `ENOENT` as refusal. Windows reports the identity operation as unsupported, and the no-sandbox boundary fails explicitly rather than claiming proof.
- A missing channel server yields `NotProved` only for the channel reachability check. Token or request failures still make the conformance report fail. The dispatch consequence is the separate issue in D244-F1.
- The unsupported `agy` path suppresses an invalid drift claim without suppressing real failed checks. Matching profiles still become unhealthy when the report contains a failure.
- T-286 records `landing` before integration. It records the landed commit and the `landed` state in one transaction after observing the branch move. The removal effect is queued before execution and uses the landed commit or a verified rescue guard.
- A `result.json` value of `unlanded` parks the task for operator action and retains custody. It does not move the task to done or satisfy verifier acceptance.
- Task cancellation first ends the live attempts and workers, releases the lease, and records `cancel_requested`. A later pass moves the task to cancelled only after every recorded process is observed dead.
- The restored T-286, T-320, T-327, and T-338 comments retain the security rules, transition reasons, statement bounds, wait bounds, and design identifiers removed by their comment sweeps.
- Agent execution remains ordered after the durable attempt, worker, and process records. Launch failure closes the attempt and lets retry policy decide the next action.

## Verdict

The delta is not ready to accept. It has 1 Critical finding and 2 Important findings. D244-F1 permits dispatch without conformance proof, D244-F2 leaves row-count-dependent SQL in custody reconciliation, and D244-F3 can prevent the required fresh fallback after a stale resume.
