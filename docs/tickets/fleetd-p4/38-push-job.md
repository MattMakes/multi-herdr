# 38: The push job: the feature branch is pushed with --force-with-lease on seen_remote_tip, a failure backs off, and 3 failures open 1 needs-you item

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::integration::adapters::exec_push && cargo test -p horch-core --test push_job && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/integration/adapters/exec_push.rs`, `crates/horch-core/src/fleetd/integration/application/remote.rs`, `crates/horch-core/tests/push_job.rs`

Spec: `docs/specs/fleetd/design.md`, "Push and pull request results are facts on the feature row", "The kinds of job" (`push`: effect, input key the feature tip and `seen_remote_tip`), I9 (`--force-with-lease`), FM-43, F11.

## Goal

A `push` job pushes the feature branch to its remote with `--force-with-lease=<branch>:<seen_remote_tip>`. A passed run sets `push_state = ok`, `push_failures = 0` and `seen_remote_tip`. A failed run sets `push_state = failed`, adds 1 to `push_failures` and sets `push_not_before` to the end of a backoff on the fleet clock; after it, fleetd queues a new run. At `push_failure_limit` (3) failures in a row, 1 needs-you item opens.

## Requirements

- FM-43: a transient push failure is followed by a new run of the push job (a test of the gate); a result of an old run is never used again.
- Choice Q21: fleetd queues a push when the feature tip differs from `seen_remote_tip` and the feature is `ready` or has an open pull request, and after each rebase (ticket 41).
- F11: push bound 10 min.
- The fleetd git rule (review F4 of T-607). This job runs git with the rights of fleetd and with no sandbox. So git must start no hook and no program of the project. Each git command of the job:
  - runs with `-c core.hooksPath=/dev/null`, `-c core.fsmonitor=false`, `-c commit.gpgSign=false` and `-c tag.gpgSign=false`, and with `GIT_TERMINAL_PROMPT=0`, `GIT_EDITOR=true` and `GIT_SEQUENCE_EDITOR=true` in its environment;
  - first reads `git config --list --show-origin`. A key that starts a program and comes from the repository or the worktree config (not from the global or system config of the user) stops the job: it ends `failed` with the key in its reason, and changes nothing. The keys are `core.fsmonitor`, `core.sshCommand`, `core.askPass`, `core.editor`, `sequence.editor`, `credential.helper`, `gpg.program`, `diff.external`, and each `filter.<name>.clean`, `filter.<name>.smudge`, `filter.<name>.process`, `merge.<name>.driver` and `diff.<name>.textconv`.
  - For this ticket: `git push` runs the `pre-push` hook, and it reads `core.sshCommand` and `credential.helper`. The push uses only the values of these 2 keys from the global config of the user. Q13 of MAP.md stays.

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- Add the push rules (when to queue, the observation, the backoff) to `application/remote.rs` beside the fetch rules of ticket 37.
- The backoff: `job_backoff_s` of Phase 1 (30 s, 120 s, 480 s) on the fleet clock.
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `push_sets_ok_and_the_seen_remote_tip`, `rewrite_uses_force_with_lease_on_the_seen_remote_tip`, `remote_moved_by_someone_else_is_not_overwritten`, `transient_failure_is_followed_by_a_new_run_after_the_backoff`, `3_failures_open_1_needs_you_item`, `push_runs_no_hook_or_program_of_the_project` (a repository with a `pre-push` hook that writes a marker file, and a test with `core.sshCommand` in the repository config; the push runs no hook and the marker does not exist; the config key ends the job `failed` with the key in its reason).
2. Write the executor.
3. This ticket moves a ref, lands, pushes, deletes a branch or a ref, or changes who runs the finish steps. It has a crash test: a fault point before the commit of the intended row, and after the commit and before the effect. In each case the next pass completes or fails the effect 1 time, and no tip and no work is lost.
4. Run the Check. Commit the exact files.

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

Self-hosting: this phase changes `horch spawn`, the claim and the finish steps. The build fleet itself uses the installed `horch` for these. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).

## Done when

The Check passes. A push never overwrites a remote commit that fleetd did not see, and failures are bounded and visible.
