# 36: Trial merges: the trial_merges table and the trial_merge job, keyed by the 2 tips, for each feature against the primary branch and each pair of features that share a file

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --test trial_merges && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/integration/domain/trial_merge.rs`, `crates/horch-core/src/fleetd/integration/application/trial_merges.rs`, `crates/horch-core/src/fleetd/integration/adapters/exec_trial_merge.rs`, `crates/horch-core/src/fleetd/integration/adapters/trial_merges_store.rs`, `crates/horch-core/src/fleetd/integration/adapters/executors.rs`, `crates/horch-core/tests/trial_merges.rs`

Spec: `docs/specs/fleetd/design.md` (Between features (Early detection; 1 git writer for each repository); Ledger (`trial_merges`; `jobs`); The kinds of job).

## Goal

Use case `RunTrialMerges`: when a feature tip or the primary tip moves, fleetd queues the trial merges that are due, and their verdicts are facts in the ledger.

## Requirements

- The `trial_merges` table (new work of Phase 6)
- FM-41 and FM-42 need the early detection

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- A `trial_merge` job runs `git merge-tree --write-tree <left> <right>` with no checkout, through the launcher, with `job_limit_s.trial_merge` (Q-B of fleetd-p4). It only reads git, so it may run beside other jobs of the repository.
- It is a pure job: its `input_key` is the 2 tips, and a verdict for the same 2 tips is used again. `trial_merges` holds `clean` (null while the job runs) and `conflict_files`.
- Queue rule: each in-flight feature (`open` or `ready`) against the primary branch; each pair of in-flight features whose sets of changed files share a file. A pair with no shared file needs no job. Never n x n jobs in 1 pass: the work budget of the pass applies.
- The router of the job kinds of integration is `crates/horch-core/src/fleetd/integration/adapters/executors.rs` (ticket 01 of fleetd-p4). Add the route of `trial_merge`.
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `trial_merge_of_2_tips_runs_once`, `pair_with_no_shared_file_gets_no_job`, `conflict_is_recorded_with_its_files`, `trial_merge_job_does_not_check_out`, `n_features_do_not_make_n_times_n_jobs_in_1_pass`. They fail because the files are empty.
2. Write the domain, the store, the executor and the use case.
3. Run the Check and the guard tests. Commit the exact files.

## Constraints

1. This fleet runs on the installed `horch` binary and on the live ledger
   `/Users/MASCOTT/.horch/ledger.sqlite3`. The code that you change is the code of that tool.
   Do not break the fleet that builds it:
   - Each test, and each `horch` or `fleetd` process that you build and start, uses temporary
     folders for `HORCH_DATA_DIR`, `HORCH_STATE_DIR` and `HORCH_LEDGER_DB`. Never point a
     development build at the live ledger or at the live state folders.
   - Never install a development build. Never run `horch self-update`, `cargo install` or a
     release script from a worktree.
   - Never start a development `fleetd`, watchdog or service unit on this machine outside a test
     that uses temporary folders. Never register a launchd job, a systemd unit or a scheduled task
     on this machine.
   - The only `horch` commands that you run on the live fleet are the worker commands of your own
     task (`horch ledger ...`, `horch tell orchestrator ...`).
2. NEVER use `ANTHROPIC_API_KEY`. Never read, print, set, export, pass or depend on it. Never
   write code, config, docs or teammate files that use it. Each `claude` run uses the claude.ai
   subscription login. Remove the variable from each `claude` command:
   `env -u ANTHROPIC_API_KEY claude ...`. If a task seems to need an API key, stop and ask.
3. Do not use a shared `CARGO_TARGET_DIR`. Build in the `target/` folder of your worktree. Run
   `cargo clean` in your worktree before `horch ledger finish` only if the disk has less than
   20 GB free (`df -h /`).
4. Before the first commit, run `git remote -v` and `git config user.email`. The remote must be
   `git@git-alt:MattMakes/multi-herdr.git` and the email must be `m4ttplu5@gmail.com`. If not,
   stop and send BLOCKED.
5. Commit messages have no co-author line. Never write the em dash character; use `-`. Never edit
   `CHANGELOG.md` or a generated file by hand.
6. Write the tests first and see them fail for the correct reason. Then write the code.
7. The source of truth is `docs/specs/fleetd/design.md` (FINAL). If the code of today and the
   design do not agree on a point that the ticket does not settle, stop and send QUESTION. Do not
   change `docs/specs/fleetd/` or `ddd/`.
8. Change only the files of `Own`. If the work needs another file, send QUESTION.
9. Prefer quality, simplicity and robustness to speed. No feature that the design does not name.
10. If the GitHub SSH connection times out at `horch ledger finish`, run it again, 3 times maximum.

## Done when

- The Check passes.
