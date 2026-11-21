# 03: Bound every horch git network call so a stalled connection cannot hang finish

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `sonnet-bugfix` · **Parallel:** no
**Kind:** feature
**Review:** none
**Check:** `cargo test --workspace`
**Own:** `crates/horch-core/src/vcs/`, `crates/horch-core/src/taskledger/finish/`, `crates/horch-core/src/taskledger/triage/apply.rs`, `crates/horch-core/src/execution/lifecycle.rs`, `crates/horch/src/cmd/taskledger.rs`, `crates/horch-core/tests/`, `docs/specs/task-ledger.md`, `CHANGELOG.md`

## Goal

A git `fetch`, `ls-remote` or `push` run by horch fails within a bounded time when the network stalls. The finish step then reports a clear error, and the worker can run `horch ledger finish` again.

## Context

On 2026-11-08 the network dropped most SSH connections to github.com on port 22. 2 `horch ledger finish` runs hung for more than 1 h 50 min:

- `git -C <repo> -c core.hooksPath=/dev/null fetch origin fleet/video-formats` with child `/usr/bin/ssh ... git-upload-pack`.
- `git -C <repo> -c core.hooksPath=/dev/null ls-remote origin refs/heads/fleet/video-formats` with the same ssh child.

`grep -rn "ConnectTimeout\|ServerAlive\|GIT_SSH_COMMAND" crates` finds nothing. These files run git network commands: `crates/horch-core/src/vcs/landing.rs`, `crates/horch-core/src/taskledger/finish/land.rs`, `crates/horch-core/src/taskledger/finish/pr.rs`, `crates/horch-core/src/taskledger/triage/apply.rs`, `crates/horch-core/src/execution/lifecycle.rs`, `crates/horch/src/cmd/taskledger.rs`. Out of scope: `crates/horch-marketplace/`.

The operator's `~/.ssh/config` now routes github.com to ssh.github.com:443. That fixed the cause. This ticket stops a future stall from hanging horch.

## Steps

1. List every place horch spawns git with `fetch`, `ls-remote`, `push` or `pull`. Write the list in a progress note.
2. Route each of those calls through 1 helper. The helper sets these environment values on the child, unless the operator already set them:
   - `GIT_SSH_COMMAND=ssh -o ConnectTimeout=30 -o ServerAliveInterval=15 -o ServerAliveCountMax=4`. Skip it when `GIT_SSH_COMMAND` is set, or when `git config core.sshCommand` prints a value.
   - `GIT_HTTP_LOW_SPEED_LIMIT=1000` and `GIT_HTTP_LOW_SPEED_TIME=60`, unless set.
3. Add a wall-clock bound in the helper: 10 min for a push, 3 min for the others. On expiry, kill that git child by its PID and return an error that names the command and the bound. Never kill by name or pattern.
4. Write tests. Use a fake `GIT_SSH_COMMAND` or a fake remote that never answers, with a short bound set through a debug-only environment override (follow `HORCH_TEST_CHANNEL_SEND_TIMEOUT_MS` in `crates/horch/src/cmd/messaging.rs`). Check: the test fails before the change by hanging past its own limit, and passes after.
5. Add 1 line to `docs/specs/task-ledger.md` near the land step, and 1 line to `CHANGELOG.md` Unreleased.
6. Run `cargo test --workspace`. Check: it exits 0.

## Constraints

- Do not use `ANTHROPIC_API_KEY`.
- Do not change `~/.ssh/config` or any git remote.
- Do not add retries.

## Done when

- Every git network call of step 1 goes through the helper.
- The stall test passes, and `cargo test --workspace` exits 0.

## Report

Send the DONE summary with `horch ledger done`. Include the step 1 list.
