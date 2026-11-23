# Delta review 3: Windows, project facts, conformance holds and landings

## Scope

- Base: `3d3e9a8ad45f607701b8cedb532b2881f5c0ac62`, the tip of delta review 2.
- Tip: `2906f9cc8c9a9108cd5df26748301451c66d642e` (`fleet/fleetd-finish` at worktree creation).
- I read the landed product changes of T-404, T-407, T-406, T-409, T-384, T-289 and T-398, with the T-397 test and harness fixes. I read their callers and tests to judge the claims. T-408 is not an ancestor of this tip, so I did not review its unlanded commit.
- I did not run a test that needs `scripts/dev-sandbox.sh`, as required by the ticket. The prescribed report check and `cargo fmt --all --check` are the verification commands for this report.

Landed change groups read, oldest first:

1. `61c10969` and `9d464544` — T-384/T-405: hold dispatch after a not-proved conformance report.
2. `5e5a7f56` and `52e3027f` — T-407: bounded Windows named-pipe reply drain and end-of-stream handling.
3. `17a7c6ed` through `1badaabc`, with `7125ae4b` — T-289/T-401: release, rescue and remove ended gate and dropped trees; retry transient refusals; set-read owner facts.
4. `c585c7c5` through `25361a8e` — T-397: fixes of stale e2e assertions, test git selection, project-fact setup and Windows roster tests.
5. `7886f3ab` through `b91cf2eb`, with the documentation fixes `b27dce00`, `6deaaa5f` and `a7520be1` — T-398/T-402: fixes of delta review 2 findings.
6. `b7cd3ccb` through `eb15695f` — T-409: Windows path text, home-variable and LF fixes.
7. `347585ff` through `0e56988d` — T-406: child path forms, owner-only job results and plain canonical roots.
8. `ffa60ff4` and `2906f9cc` — T-404: immutable project facts in push-attempt `START.md`.

## Findings

### D266-F1 — Important — an existing Windows job-result folder keeps the ACL that rejects every new result

**Verified by reading.** Files: `crates/horch-platform/src/windows/fs.rs:111-126`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/job_result_file.rs:91-104`, and `crates/horch-core/src/fleetd/fleet_runtime/adapters/protected_files.rs:74-94`.

Concrete order: an older horch makes `run/jobs` with `std::fs::create_dir_all`; on an elevated Windows token the folder is owned by `Administrators`, and on an ordinary install it can retain an inherited DACL. The upgraded horch writes the next result. `create_dir_owner_only` returns immediately because the folder already exists, so it neither repairs nor rejects the old owner and DACL. The new temp file itself is owner-only, but `read_protected` checks every folder through the root and rejects `run/jobs`. The job stays running from fleetd's point of view. This is the same failure that T-406 fixes for a fresh folder, preserved across the normal upgrade path.

Smallest fix: when `create_dir_owner_only` finds an existing folder, verify that this user owns it and that its DACL is owner-only, then replace its owner and protected DACL when the caller owns the upgrade. Alternatively add an explicit, narrowly scoped ACL-repair operation for `run/jobs` before the first result write. Add a Windows test that first creates `run/jobs` with the old inherited ACL, then writes and reads a result after the upgrade.

### D266-F2 — Important — changing the stored Windows root spelling disconnects existing runs from their project

**Verified by reading.** Files: `crates/horch-core/src/taskledger/runs.rs:108-133` and `crates/horch-core/src/taskledger/runs.rs:382-391`; schema: `crates/horch-core/src/taskledger/schema.rs:67-72`.

Concrete order: an older Windows horch stores a canonical project root as `\\?\C:\repo` in `projects.root`; the user upgrades in place; `root_key` now returns `C:\repo`. `of_project` compares the new plain text to the old verbatim text and finds no active run. `ensure_project` likewise misses the old row and inserts a second project for the same directory because the strings differ. Commands that resolve the current project lose the old run, while a new run and its worktrees use a second project identity. T-406 also leaves every active old run's `project_root` in the verbatim form that its git-path fix was meant to remove.

Smallest fix: migrate every existing project root through `horch_platform::path::canonicalize` before lookups use the new spelling, merging duplicate project rows if both forms already exist. Add an upgrade test with a seeded verbatim Windows root and prove that project lookup and new-run creation retain one project id and the old active run.

### D266-F3 — Important — the durable git-lock notice still has the crash window from delta review 2

**Verified by reading.** Files: `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs:886-900`, `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs:560-605`, and `crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs:807-829`.

Concrete order: a fleetd git exceeds its limit; SIGKILL leaves a `.lock`; the path enters only the process-local `LOCKS_LEFT` set; fleetd crashes before the later transaction in `Pass::record_locks_left`; the restarted process has an empty set, so it opens no durable needs-you item. The comment at `pass.rs:890-893` explicitly retains this window. A later git can print the lock path in a pass error, but that is not the required durable row and does not resolve D254-F3's restart scenario.

Smallest fix: persist the discovered lock path before the timed git operation returns its error, in a fleet-owned journal that does not depend on the pass transaction. On startup, ingest that journal idempotently into needs-you rows and retain each entry until the file is gone. Add the restart between SIGKILL and the next pass transaction that delta review 2 requested.

### D266-F4 — Important — project facts add two SQL reads for every missing task-file row

**Verified by reading.** Files: `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs:898-920` and `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs:859-873`.

Concrete order: a restart leaves 300 push attempts in `launching` without task files; one pass reads their ids once, then calls `write_with_facts` for each row; that function calls `tasks::get` and `runs::get` separately for each attempt, even though `facts` immediately before it already read both rows. T-404 therefore adds 600 SQL statements to this pass step, in addition to its existing per-attempt reads. A large recovery pass can exceed its time budget and delay every later duty. This violates the pass rule that a changed pass step performs no SQL statement for each row.

Smallest fix: read the task, run and project-root facts for all missing attempts in set queries on one snapshot, then render each attempt from those in-memory facts. At minimum, carry the already-read project root in `TaskFacts` instead of querying the same task and run again. Add a statement-count test showing that 3 and 300 missing attempts execute the same count of read statements.

### D266-F5 — Important — the changed custody action loop still performs SQL reads for every removable tree

**Verified by reading.** Files: `crates/horch-core/src/fleetd/worktree_custody/application/reconcile.rs:787-844`, `crates/horch-core/src/fleetd/worktree_custody/application/reconcile.rs:854-868`, and `crates/horch-core/src/fleetd/worktree_custody/application/remove.rs:102-151`.

Concrete order: a pass has 300 rescued dropped trees. `act` set-reads rows and owner-ended facts, but `given_up` calls `jobs_store::get` separately for each drop to prove its rescue run. Before that, `remove::retry` and the idea/removal decisions also perform row-specific reads. The pass thus scales its SQL count with the number of trees and can exceed its duty budget while dispatch and other duties wait. T-408's set-read commit would address these loops, but T-408 is not landed in this tip.

Smallest fix: land the set-read custody change after reviewing it, or read all rescue jobs, latest removals, refusal facts and idea states needed by `act` once and decide from maps. Add a statement-count test for 3 and 300 active trees that includes the `given_up` and retry paths.

## Checked and good

- T-404 records one-line facts with a 2,048-byte limit and retains at most 100 through `project_notes::record`. `data_section` chooses a fence longer than every backtick run, so a recorded heading, backtick run or other Markdown text stays data. CR and LF are refused. The per-attempt `facts.md` snapshot makes the pass writer and `horch spawn` render equal bytes after the first writer wins. Project-note identity resolves linked worktrees to their common repository but keeps unrelated repositories under different hashes.
- T-407 checks the Windows peer before the request is read. Each request read and reply write receives a deadline. The reply drain runs on the I/O thread and `cancel_after` joins its watchdog before the buffer or thread handle is dropped. The `ManuallyDrop<File>` view cannot close the listener's handle and keeps the `Instance` alive through disconnect. A client that does not read can hold the sole instance for the bounded drain only.
- T-406 converts a canonical path to plain form only when its text is a drive-absolute or valid UNC path below the legacy length limit and contains no component whose Win32 interpretation can change. Long paths remain verbatim. Canonicalization resolves links before conversion, and protected-file containment canonicalizes both the root and candidate with the same standard form. The fresh-create file descriptor and rename preserve the result file's owner-only DACL. The upgrade exceptions are D266-F1 and D266-F2.
- T-409's forward-slash product text is accepted as a Windows path when `NewTask.task_file` is later joined to the project root. The other LF changes affect checked-in text and test comparisons, not a path that is parsed back by product code. Windows absolute local skill sources now include drive, UNC and verbatim forms.
- T-384 binds a conformance result to the observed job id and subject and writes the hold in the same guarded transaction that observes the result. The last health/hold event determines dispatch. A failed report keeps or makes the profile unhealthy; a passed report writes a healthy event and releases the hold. The job-result reader checks the filename against the payload run and job ids before the report reaches this code.
- T-289 records the worker's drop decision in the same transaction that queues its rescue. A dropped tree is removable only when the saved rescue job produced the row's current rescue ref. The removal guard quarantines the exact recorded path, rereads the ledger row and HEAD, checks the rescue commit and disk tree, checks the owner and processes with working directories in the tree, and moves the folder back on refusal. A released gate tree is removed only after its feature ends and its rescue is verified.
- T-398's leader-group stop snapshots member pid/start pairs, rechecks the leader, and signals only those identities for TERM and KILL. It no longer signals a reused group id after a separate leader check. The limited-git wait retries `EINTR`; the armed `Held` guard terminates the contained group and reaps the child on each early return. D266-F3 is the remaining durability fault, not a process-cleanup fault.
- T-397 changes product test support and test expectations; it does not add a new production dispatch or custody rule. T-408 is not landed and is outside this tip.

## Verdict

Not ready. No Critical finding is open in this delta, but 5 Important findings are open. Existing Windows state bypasses both the new result-folder ACL and the new project-root identity, the lock notice can still disappear on a crash, and two changed pass loops issue SQL reads per row.
